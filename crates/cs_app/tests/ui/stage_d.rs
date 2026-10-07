//! Acceptance stage F45-D: the end-to-end navigation review and the captures
//! (`specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`, section
//! `### F45-D`). Shared contracts: `docs/contracts/UI-NETWORK.md` and
//! `docs/contracts/CLI-EVIDENCE.md`.
//!
//! The stage's minimum scenario is **AC04 — capture and review all original
//! front-end screens and navigation paths**, and it needs both of the stage's
//! declared capabilities:
//!
//! * `gpu` — the two capture tests draw a real frame through
//!   [`cs_app::ui::front_end::capture_screen`] /
//!   [`cs_app::ui::front_end::capture_artwork`] and refuse every frame that is
//!   not evidence of a drawn screen. CI selects no adapter, so they are
//!   `#[ignore]`d and run with `--include-ignored`.
//! * `retail` — the two `retail` tests read `$CS_GAME_DIR` through production
//!   discovery and the production readers (`FrontEndScreens`), measure the
//!   complete original screen inventory and capture every screen-capable
//!   original image. They are `#[ignore]`d with `requires CS_GAME_DIR`.
//!
//! The first test is neither: the navigation review runs on the state table
//! alone, so CI exercises it on every push.
//!
//! Nothing here claims the original's *appearance* or its screen→artwork
//! binding: the binding is unread (resolving task #742) and no original
//! executable runs in any agent session.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use cs_app::ui::front_end::{
    Action, Artwork, ConstructionDraft, FrontEndScreens, Loadout, MINIMUM_SCREEN_EXTENT,
    NavigationInputs, Screen, ScreenSession, capture_artwork, capture_screen, review_navigation,
    review_navigation_with,
};
use cs_types::content::ContentKind;

use super::id;
use super::screens::preflight_deck;

/// The logical size every fixture screen in `screens.rs` is authored at.
const IMAGE: (u32, u32) = (640, 480);

/// Where captures are written: the evidence directory when the acceptance run
/// supplies one, otherwise this checkout's own private directory (ignored by
/// Git, like every other artifact of this task).
fn capture_dir() -> PathBuf {
    let dir = match std::env::var_os("CS_EVIDENCE_DIR") {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            if dir.is_absolute() {
                dir
            } else {
                workspace_root().join(dir)
            }
        }
        None => workspace_root().join("private/evidence/F45-D-captures"),
    };
    std::fs::create_dir_all(&dir).expect("the capture directory is created");
    dir
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// The original installation, for the `retail` tests.
fn retail_game_dir() -> PathBuf {
    let dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must point at the original installation"),
    );
    assert!(
        dir.is_dir(),
        "CS_GAME_DIR {} is not a directory",
        dir.display()
    );
    dir
}

/// The original screen inventory, opened once: the whole archive walk is the
/// expensive part and three tests share it.
fn screens() -> &'static FrontEndScreens {
    static SCREENS: OnceLock<FrontEndScreens> = OnceLock::new();
    SCREENS.get_or_init(|| {
        FrontEndScreens::open(&retail_game_dir()).unwrap_or_else(|error| {
            panic!("the original screen artwork could not be read: {error}")
        })
    })
}

/// Authored fixture artwork: a non-uniform two-tone **grey** picture whose two
/// levels are derived from `key`, so two screens never draw the same frame and
/// the picture itself never carries the red/blue tint the capture test counts
/// on the hotspot regions.
///
/// This is the *synthetic* half of the stage: the deck's artwork ids are
/// authored (F45-B), no original pixel reaches a synthetic test, and the
/// retail tests below are the ones that draw original images.
fn fixture_art(key: &str, extent: (u32, u32)) -> Artwork {
    let seed = key.bytes().fold(0u32, |acc, byte| {
        acc.wrapping_mul(31).wrapping_add(u32::from(byte))
    });
    let top = (seed % 50) as u8 + 30;
    let bottom = ((seed >> 7) % 50) as u8 + 110;
    let mut rgba = Vec::with_capacity(extent.0 as usize * extent.1 as usize * 4);
    for y in 0..extent.1 {
        let level = if y < extent.1 / 2 { top } else { bottom };
        for _ in 0..extent.0 {
            rgba.extend_from_slice(&[level, level, level, 255]);
        }
    }
    Artwork::new(extent.0, extent.1, rgba).expect("the fixture artwork is well formed")
}

/// Strictly parses `text` as exactly one JSON value (RFC 8259) and fails at
/// the first byte that is not.
///
/// Written by hand because this workspace carries no JSON dependency: what is
/// under test here is that the production artifacts ([`cs_app::ui::
/// front_end::PathReview::json`], [`cs_app::ui::front_end::FrontEndInventory::
/// json`]) are readable as JSON by any consumer, not that a JSON library
/// works. A bare `START` or `MainMenu` where a string belongs is the failure
/// this catches.
fn assert_parses_as_json(text: &str) {
    let bytes = text.as_bytes();
    let mut cursor = 0;
    if let Err(error) = json_value(bytes, &mut cursor) {
        panic!("the artifact is not valid JSON at byte {cursor}: {error}; artifact: {text}");
    }
    json_space(bytes, &mut cursor);
    assert_eq!(
        cursor,
        bytes.len(),
        "the artifact has {} trailing bytes after its value; artifact: {text}",
        bytes.len() - cursor
    );
}

fn json_space(bytes: &[u8], cursor: &mut usize) {
    while matches!(bytes.get(*cursor), Some(b' ' | b'\t' | b'\n' | b'\r')) {
        *cursor += 1;
    }
}

fn json_value(bytes: &[u8], cursor: &mut usize) -> Result<(), String> {
    json_space(bytes, cursor);
    match bytes.get(*cursor) {
        Some(b'{') => json_object(bytes, cursor),
        Some(b'[') => json_array(bytes, cursor),
        Some(b'"') => json_string(bytes, cursor),
        Some(b't') => json_literal(bytes, cursor, "true"),
        Some(b'f') => json_literal(bytes, cursor, "false"),
        Some(b'n') => json_literal(bytes, cursor, "null"),
        Some(byte) if byte.is_ascii_digit() || *byte == b'-' => json_number(bytes, cursor),
        Some(byte) => Err(format!("unexpected byte {byte:#04x}")),
        None => Err("the value is missing".to_owned()),
    }
}

fn json_object(bytes: &[u8], cursor: &mut usize) -> Result<(), String> {
    *cursor += 1; // `{`
    json_space(bytes, cursor);
    if bytes.get(*cursor) == Some(&b'}') {
        *cursor += 1;
        return Ok(());
    }
    loop {
        json_space(bytes, cursor);
        json_string(bytes, cursor)?;
        json_space(bytes, cursor);
        if bytes.get(*cursor) != Some(&b':') {
            return Err("a member needs ':'".to_owned());
        }
        *cursor += 1;
        json_value(bytes, cursor)?;
        json_space(bytes, cursor);
        match bytes.get(*cursor) {
            Some(b',') => *cursor += 1,
            Some(b'}') => {
                *cursor += 1;
                return Ok(());
            }
            _ => return Err("a member needs ',' or the object needs '}'".to_owned()),
        }
    }
}

fn json_array(bytes: &[u8], cursor: &mut usize) -> Result<(), String> {
    *cursor += 1; // `[`
    json_space(bytes, cursor);
    if bytes.get(*cursor) == Some(&b']') {
        *cursor += 1;
        return Ok(());
    }
    loop {
        json_value(bytes, cursor)?;
        json_space(bytes, cursor);
        match bytes.get(*cursor) {
            Some(b',') => *cursor += 1,
            Some(b']') => {
                *cursor += 1;
                return Ok(());
            }
            _ => return Err("an element needs ',' or the array needs ']'".to_owned()),
        }
    }
}

fn json_string(bytes: &[u8], cursor: &mut usize) -> Result<(), String> {
    if bytes.get(*cursor) != Some(&b'"') {
        return Err("a string must open with '\"'".to_owned());
    }
    *cursor += 1;
    loop {
        let byte = *bytes
            .get(*cursor)
            .ok_or_else(|| "the string is not closed".to_owned())?;
        match byte {
            b'"' => {
                *cursor += 1;
                return Ok(());
            }
            b'\\' => {
                *cursor += 1;
                let escape = *bytes
                    .get(*cursor)
                    .ok_or_else(|| "the escape is cut short".to_owned())?;
                *cursor += 1;
                match escape {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                    b'u' => {
                        for _ in 0..4 {
                            let digit = *bytes
                                .get(*cursor)
                                .ok_or_else(|| "the escape is cut short".to_owned())?;
                            *cursor += 1;
                            if !digit.is_ascii_hexdigit() {
                                return Err(format!("escape byte {digit:#04x} is not a hex digit"));
                            }
                        }
                    }
                    other => return Err(format!("unknown escape {other:#04x}")),
                }
            }
            control if control < 0x20 => {
                return Err(format!("raw control byte {control:#04x} inside a string"));
            }
            _ => *cursor += 1,
        }
    }
}

fn json_number(bytes: &[u8], cursor: &mut usize) -> Result<(), String> {
    if bytes.get(*cursor) == Some(&b'-') {
        *cursor += 1;
    }
    let digits = |bytes: &[u8], cursor: &mut usize| {
        let start = *cursor;
        while bytes.get(*cursor).is_some_and(u8::is_ascii_digit) {
            *cursor += 1;
        }
        *cursor > start
    };
    if !digits(bytes, cursor) {
        return Err("a number needs an integer part".to_owned());
    }
    if bytes.get(*cursor) == Some(&b'.') {
        *cursor += 1;
        if !digits(bytes, cursor) {
            return Err("a fraction needs digits after '.'".to_owned());
        }
    }
    if matches!(bytes.get(*cursor), Some(b'e' | b'E')) {
        *cursor += 1;
        if matches!(bytes.get(*cursor), Some(b'+' | b'-')) {
            *cursor += 1;
        }
        if !digits(bytes, cursor) {
            return Err("an exponent needs digits".to_owned());
        }
    }
    Ok(())
}

fn json_literal(bytes: &[u8], cursor: &mut usize, literal: &str) -> Result<(), String> {
    let word = literal.as_bytes();
    if bytes.get(*cursor..*cursor + word.len()) == Some(word) {
        *cursor += word.len();
        Ok(())
    } else {
        Err(format!("expected {literal}"))
    }
}

/// Reads a written PNG back as RGBA8 pixels, row-major from the top.
fn read_png(path: &Path) -> Vec<u8> {
    let image = image::ImageReader::open(path)
        .unwrap_or_else(|error| panic!("open {}: {error}", path.display()))
        .decode()
        .unwrap_or_else(|error| panic!("decode {}: {error}", path.display()))
        .into_rgba8();
    image.into_raw()
}

/// **The navigation half of AC04.** Every row of the state table is applied on
/// the real machine, every screen is either reached or named as not reached,
/// and no row of the table is left unclassified.
///
/// This test runs in CI (no capability needed). It fails when the review
/// cannot account for the table — the observable failure this stage fixes:
/// before it, "every navigation path" was a structural property of the table
/// that no test ever *walked*.
#[test]
fn accept_f45_d_the_navigation_review_classifies_every_row_and_names_every_screen() {
    // 1. No domain input: the walk stops exactly where a player who has
    //    selected nothing stops, and says which screens are behind that
    //    instead of pretending it walked them.
    let bare = review_navigation(Screen::START);
    assert!(
        bare.is_complete(),
        "even the inputless walk classifies every row: {bare}"
    );
    assert_eq!(bare.inputs.len(), 0, "no input was declared to it");
    assert_eq!(
        bare.not_reached.len(),
        5,
        "five screens sit behind the flight check: {:?}",
        bare.not_reached
    );
    for behind in [
        Screen::Loading,
        Screen::Flight,
        Screen::Pause,
        Screen::PauseSettings,
        Screen::Results,
    ] {
        assert!(
            bare.not_reached.contains(&behind),
            "{behind:?} is behind a guard the inputless walk cannot pass: {:?}",
            bare.not_reached
        );
    }
    assert!(
        bare.refusals()
            .iter()
            .any(|(_, _, code)| *code == "invalid_loadout"),
        "the launch is refused by name rather than skipped: {:?}",
        bare.refusals()
    );

    // 2. With the inputs a player supplies: every screen is reached and every
    //    row of the table is applied — the whole navigation path set.
    let inputs = NavigationInputs {
        loadout: Some(Loadout {
            player: Some(id(ContentKind::Airframe, "player-plane")),
            wingmate: Some(id(ContentKind::Airframe, "wing-plane")),
            ammunition: Some(id(ContentKind::Ammo, "standard")),
        }),
        construction: Some(ConstructionDraft {
            blueprint: id(ContentKind::Blueprint, "blueprint.0"),
            saved: vec![id(ContentKind::Armor, "armor.nose")],
            components: vec![
                id(ContentKind::Armor, "armor.nose"),
                id(ContentKind::Engine, "engine.0"),
            ],
        }),
    };
    let review = review_navigation_with(Screen::START, &inputs);
    assert_eq!(review.start, Screen::START);
    assert!(
        review.is_complete(),
        "the review must classify every row of the table and account for every screen: {review} \
         (steps {}, unreached rows {})",
        review.steps.len(),
        review.rows_unreached
    );
    assert!(
        review.not_reached.is_empty(),
        "with the player's own inputs every screen is reached: {:?}",
        review.not_reached
    );
    assert_eq!(
        review.reached.len(),
        Screen::ALL.len(),
        "every screen was walked"
    );
    assert_eq!(
        review.steps.len(),
        cs_app::ui::front_end::TABLE.len(),
        "every row of the table applied exactly once"
    );
    assert_eq!(review.rows_unreached, 0, "no row is left behind a screen");
    assert_eq!(review.reached[0], Screen::START, "the walk starts at boot");

    // The declared inputs are recorded, so a reader can see which paths exist
    // only because a player selected something first.
    let mut supplied: Vec<&str> = review
        .supplied_inputs()
        .iter()
        .map(|input| input.input)
        .collect();
    supplied.sort_unstable();
    assert_eq!(supplied, ["construction_draft", "loadout"], "{supplied:?}");

    // The paths the contract names are walked, and their domain transactions
    // are the ones the table declares: a player's actions reach the domain
    // through the machine, never around it.
    let requests: Vec<(Screen, cs_app::ui::front_end::Action, _)> = review.requests();
    let kinds: Vec<cs_app::ui::front_end::RequestKind> =
        requests.iter().map(|(_, _, kind)| *kind).collect();
    for expected in [
        cs_app::ui::front_end::RequestKind::OpenProfile,
        cs_app::ui::front_end::RequestKind::CloseProfile,
        cs_app::ui::front_end::RequestKind::CommitBlueprint,
        cs_app::ui::front_end::RequestKind::CommitLoadout,
        cs_app::ui::front_end::RequestKind::ApplyOutcome,
        cs_app::ui::front_end::RequestKind::AbandonMission,
    ] {
        assert!(
            kinds.contains(&expected),
            "{expected:?} was asked: {kinds:?}"
        );
    }

    // The complete paths `docs/contracts/UI-NETWORK.md` names for this
    // feature, each leg checked against the row the walk applied: the review
    // does not only count rows, it shows that the contract's own sequences
    // exist end to end on the machine.
    for (from, action, to) in [
        // missing install -> choose install -> main
        (
            Screen::InstallSelect,
            Action::InstallVerified,
            Screen::MainMenu,
        ),
        // new profile -> cabin -> briefing -> flight check -> loading ->
        // mission -> success -> scrapbook
        (Screen::MainMenu, Action::NewProfile, Screen::ProfileSelect),
        (Screen::ProfileSelect, Action::ConfirmProfile, Screen::Cabin),
        (Screen::Cabin, Action::OpenBriefing, Screen::Briefing),
        (
            Screen::Briefing,
            Action::ContinueToFlightCheck,
            Screen::FlightCheck,
        ),
        (Screen::FlightCheck, Action::Launch, Screen::Loading),
        (Screen::Loading, Action::LoadSucceeded, Screen::Flight),
        (Screen::Flight, Action::MissionSucceeded, Screen::Results),
        (Screen::Results, Action::OpenScrapbook, Screen::Scrapbook),
        // construction edit -> cancel
        (Screen::Construction, Action::Cancel, Screen::Cabin),
        // mission failure -> retry
        (Screen::Results, Action::Retry, Screen::Loading),
        // pause -> settings -> resume
        (Screen::Flight, Action::Pause, Screen::Pause),
        (Screen::Pause, Action::OpenSettings, Screen::PauseSettings),
        (Screen::PauseSettings, Action::Back, Screen::Pause),
        (Screen::Pause, Action::Resume, Screen::Flight),
        // missing content -> diagnosis -> selection
        (
            Screen::MainMenu,
            Action::ContentMissing,
            Screen::ContentDiagnosis,
        ),
        (
            Screen::ContentDiagnosis,
            Action::ChooseAnotherInstall,
            Screen::InstallSelect,
        ),
    ] {
        let step = review
            .steps
            .iter()
            .find(|step| step.from == from && step.action == action)
            .unwrap_or_else(|| panic!("{action:?} on {from:?} was never applied: {review}"));
        assert_eq!(
            step.to, to,
            "{action:?} on {from:?} must lead to {to:?} (UI-NETWORK's named path)"
        );
        assert_ne!(
            step.outcome,
            cs_app::ui::front_end::StepOutcome::Refused,
            "{action:?} on {from:?} is a path a player takes and must not be refused: {review}"
        );
    }

    // Back/Cancel from a dirty screen asks before discarding (F45
    // non-negotiable 1), and the walk answers it rather than stopping.
    assert!(
        review
            .steps
            .iter()
            .any(|step| step.outcome == cs_app::ui::front_end::StepOutcome::Prompted),
        "a dirty draft's Back/Cancel prompts and the walk confirms: {:?}",
        review
            .steps
            .iter()
            .filter(|step| step.outcome == cs_app::ui::front_end::StepOutcome::Prompted)
            .map(|step| (step.from, step.action))
            .collect::<Vec<_>>()
    );

    // Any refusal that remains carries its code, so "why does this path stop"
    // is always answerable from the report.
    for (screen, action, code) in review.refusals() {
        assert!(
            !code.is_empty(),
            "{action:?} on {screen:?} was refused without a code"
        );
    }
    assert!(
        review.json().contains("\"complete\":true"),
        "the artifact states its own completeness: {}",
        review.json()
    );
    // The artifact is *readable* as JSON: every screen, action, outcome and
    // refusal is a JSON string, never a bare word.
    let artifact = review.json();
    assert_parses_as_json(&artifact);
    assert!(
        artifact.contains("\"screens_reached\":[\"InstallSelect\""),
        "the reached screens are quoted strings: {artifact}"
    );
    assert!(
        artifact.contains("\"from\":\"FlightCheck\""),
        "the row detail is quoted strings: {artifact}"
    );
    // With the player's own inputs no row is refused, so every visible button
    // of the table has a functioning transition on some explored state
    // (F45 non-negotiable 1), and the refusal list is not hiding a dead path.
    assert!(
        review.refusals().is_empty(),
        "no row is refused once the player's inputs are supplied: {:?}",
        review.refusals()
    );
    eprintln!("navigation review: {review}");
    eprintln!(
        "screens not reached: {:?}",
        review
            .not_reached
            .iter()
            .map(|s| format!("{s:?}"))
            .collect::<Vec<_>>()
    );
    eprintln!("refusals: {:?}", review.refusals());
    eprintln!("requests: {:?}", review.requests());
    eprintln!("supplied inputs: {:?}", review.supplied_inputs());
}

/// **The GPU half of AC04 (synthetic):** a real screen drawn on the real
/// renderer, with its artwork, every hotspot region and the focused button
/// visible in the frame — and a second screen that draws a *different* frame,
/// so the capture is not a static fixture.
///
/// Fails when `capture_screen` is removed or stops drawing: no PNG is
/// produced, and the focused/unfocused pixel counts come back empty.
#[test]
#[ignore = "requires a GPU adapter; run with --include-ignored"]
fn accept_f45_d_a_screen_capture_draws_the_artwork_its_hotspots_and_the_focused_button() {
    let dir = capture_dir();
    // The main menu: four authored buttons, so the frame carries a focused
    // region *and* unfocused ones to tell apart.
    let mut session = ScreenSession::new(preflight_deck());
    session
        .press(cs_app::ui::front_end::Action::InstallVerified)
        .expect("the install verifies");
    let view = session
        .view(cs_app::ui::front_end::SCREEN_CAPTURE_SURFACE)
        .expect("the deck carries the screen");
    assert_eq!(view.screen, Screen::MainMenu);
    let art = fixture_art(&view.art.to_string(), view.image);
    assert_eq!(
        view.image, IMAGE,
        "the fixture screens are authored at 640x480"
    );
    assert!(
        view.buttons.len() >= 2,
        "a screen the deck carries has a focused button and at least one other"
    );

    let png = dir.join("f45-d-screen-main-menu.png");
    let _ = std::fs::remove_file(&png);
    let capture = capture_screen(&view, &art, &png).unwrap_or_else(|error| {
        panic!(
            "the screen {:?} could not be captured: {error}",
            view.screen
        )
    });
    assert!(capture.drew_screen(), "the frame is not the clear colour");
    assert_eq!(
        capture.width, 800,
        "the frame is the measured screen extent"
    );
    assert_eq!(capture.height, 600);
    assert_eq!(capture.buttons, view.buttons.len());
    assert!(png.is_file(), "the capture wrote {}", png.display());
    let bytes = std::fs::read(&png).expect("the capture reads back");
    assert_eq!(
        capture.png_sha256.to_hex(),
        cs_assets::install::sha256(&bytes).to_hex(),
        "the recorded digest is the digest of the file on disk"
    );
    assert!(
        capture.adapter != "no adapter reported",
        "a real adapter drew this frame: {}",
        capture.adapter
    );

    // The frame's own content: the focused button's tint and the other
    // buttons' tint, counted by colour relation so neither the sampler nor the
    // row order can fake them.
    let pixels = read_png(&png);
    let focus = view
        .buttons
        .iter()
        .find(|button| button.focused)
        .expect("entering a screen focuses its authored first button");
    let focus_rect = focus.rect;
    let focus_area = focus_rect.width as usize * focus_rect.height as usize;
    let other_area: usize = view
        .buttons
        .iter()
        .filter(|button| !button.focused)
        .map(|button| button.rect.width as usize * button.rect.height as usize)
        .sum();
    let (mut focus_pixels, mut button_pixels) = (0usize, 0usize);
    for pixel in pixels.as_chunks::<4>().0 {
        let (r, g, b) = (
            i32::from(pixel[0]),
            i32::from(pixel[1]),
            i32::from(pixel[2]),
        );
        let _ = g;
        if b - r > 60 {
            focus_pixels += 1;
        }
        if r - b > 60 {
            button_pixels += 1;
        }
    }
    assert!(
        focus_pixels * 10 >= focus_area * 6,
        "the focused button's region is drawn: {focus_pixels} of {focus_area} pixels"
    );
    assert!(
        focus_pixels <= focus_area + focus_area / 10,
        "the focused tint stays inside its own region: {focus_pixels} of {focus_area}"
    );
    assert!(
        other_area > 0 && button_pixels * 10 >= other_area * 6,
        "every other hotspot region is drawn: {button_pixels} of {other_area} pixels"
    );

    // A different screen draws a different frame: the artwork's own halves are
    // keyed by the artwork id, so this is the picture changing, not the path.
    let mut other = ScreenSession::new(preflight_deck());
    other
        .press(cs_app::ui::front_end::Action::InstallVerified)
        .expect("the install verifies");
    other
        .press(cs_app::ui::front_end::Action::OpenSettings)
        .expect("the menu opens settings");
    let view = other
        .view(cs_app::ui::front_end::SCREEN_CAPTURE_SURFACE)
        .expect("the deck carries settings");
    let art = fixture_art(&view.art.to_string(), view.image);
    let png = dir.join("f45-d-screen-settings.png");
    let _ = std::fs::remove_file(&png);
    let second = capture_screen(&view, &art, &png)
        .unwrap_or_else(|error| panic!("settings could not be captured: {error}"));
    assert!(second.drew_screen());
    assert_ne!(
        capture.png_sha256, second.png_sha256,
        "two different screens must not produce the same frame"
    );
}

/// Every way a capture could produce a file that is not evidence of a drawn
/// screen is refused, named, and leaves nothing behind.
#[test]
#[ignore = "requires a GPU adapter; run with --include-ignored"]
fn accept_f45_d_a_frame_that_is_not_evidence_of_a_drawn_screen_is_refused() {
    let dir = capture_dir();
    let png = dir.join("f45-d-refused.png");
    let _ = std::fs::remove_file(&png);

    // A picture with no pixels never reaches the renderer.
    let empty = Artwork::new(0, 0, Vec::new());
    assert!(matches!(
        empty,
        Err(cs_app::ui::front_end::ScreenCaptureError::EmptyArtwork { .. })
    ));

    // The pixel buffer must match the extent it claims.
    let short = Artwork::new(4, 4, vec![0; 15]);
    assert!(matches!(
        short,
        Err(cs_app::ui::front_end::ScreenCaptureError::PixelCount {
            expected: 64,
            actual: 15,
        })
    ));

    // A view fitted against another surface, or against another image, is
    // refused before anything is drawn: its hotspots would land on the wrong
    // pixels.
    let session = ScreenSession::new(preflight_deck());
    let wrong_surface = session.view((1920, 1080)).expect("a view");
    let art = fixture_art(&wrong_surface.art.to_string(), wrong_surface.image);
    let error = capture_screen(&wrong_surface, &art, &png)
        .expect_err("a view fitted against 1920x1080 is refused");
    assert!(matches!(
        error,
        cs_app::ui::front_end::ScreenCaptureError::SurfaceMismatch { .. }
    ));
    assert!(!png.exists(), "a refused capture leaves no file");

    let view = session
        .view(cs_app::ui::front_end::SCREEN_CAPTURE_SURFACE)
        .expect("the deck carries the screen");
    let wrong_art = fixture_art(&view.art.to_string(), (320, 240));
    let error = capture_screen(&view, &wrong_art, &png)
        .expect_err("artwork that is not the fitted image is refused");
    assert!(matches!(
        error,
        cs_app::ui::front_end::ScreenCaptureError::SizeMismatch { .. }
    ));
    assert!(!png.exists(), "a refused capture leaves no file");

    // A full-frame flat picture draws nothing distinguishable, and the
    // uniform-frame gate refuses it instead of writing a plausible PNG.
    let flat = Artwork::new(
        cs_app::ui::front_end::SCREEN_CAPTURE_WIDTH,
        cs_app::ui::front_end::SCREEN_CAPTURE_HEIGHT,
        [128, 128, 128, 255].repeat(
            (cs_app::ui::front_end::SCREEN_CAPTURE_WIDTH
                * cs_app::ui::front_end::SCREEN_CAPTURE_HEIGHT) as usize,
        ),
    )
    .expect("the flat artwork is well formed");
    assert!(flat.is_uniform());
    let error = capture_artwork("flat", &flat, &[], &png)
        .expect_err("a frame that drew nothing is refused");
    assert!(matches!(
        error,
        cs_app::ui::front_end::ScreenCaptureError::UniformFrame { .. }
    ));
    assert!(
        !png.exists(),
        "a refused capture never leaves a file that reads like a good one"
    );
}

/// **The retail half of AC04, part one: the complete inventory.** Every image
/// of both original sources is measured — extent, format, digest — and every
/// one that production code cannot decode is named rather than skipped. The
/// artifact `front-end-screens.json` is written when `CS_EVIDENCE_DIR` is set.
///
/// Known measured facts this pins: `rimage.zbd` holds 254 stored textures,
/// the screen container holds 563 graphics images under `ASSETS/GRAPHICS/`,
/// `mainmenu` and `escapemenu` are 640x480 (the stage's declared minimum
/// screen extent) and the front-end backgrounds are 800x600.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f45_d_retail_the_original_front_end_screen_inventory_is_complete_and_measured() {
    let inventory = screens()
        .inventory()
        .expect("both original sources are read end to end");
    assert_eq!(
        inventory.images.len(),
        817,
        "254 UI textures + 563 graphics images"
    );
    assert_eq!(
        inventory
            .images
            .iter()
            .filter(|image| image.source == cs_app::ui::front_end::ImageSource::UiArchive)
            .count(),
        254,
        "the shared UI/HUD archive's measured texture count"
    );
    assert_eq!(
        inventory.undecodable(),
        0,
        "every original front-end image decodes: {:?}",
        inventory
            .images
            .iter()
            .filter(|image| !image.decodable)
            .map(|image| (image.name.as_str(), image.refusal.as_str()))
            .collect::<Vec<_>>()
    );

    let by_name = |name: &str| {
        inventory
            .images
            .iter()
            .find(|image| image.name.eq_ignore_ascii_case(name))
            .unwrap_or_else(|| panic!("the original installation stores {name}"))
    };
    let mainmenu = by_name("mainmenu");
    assert_eq!((mainmenu.width, mainmenu.height), MINIMUM_SCREEN_EXTENT);
    assert!(mainmenu.screen_capable());
    assert_eq!(
        (by_name("escapemenu").width, by_name("escapemenu").height),
        MINIMUM_SCREEN_EXTENT
    );
    let background = by_name("ASSETS/GRAPHICS/MM_BACKGROUND.PNG");
    assert_eq!(
        (background.width, background.height),
        (800, 600),
        "the front-end backgrounds are the measured 800x600"
    );
    assert_eq!(
        (
            by_name("ASSETS/GRAPHICS/FC_BACKGROUND.JPG").width,
            by_name("ASSETS/GRAPHICS/FC_BACKGROUND.JPG").height
        ),
        (800, 600)
    );

    let capable = inventory.screen_capable();
    assert_eq!(
        capable.len(),
        68,
        "every original image large enough to be a screen is selected: {}",
        capable
            .iter()
            .map(|image| image.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    assert!(
        capable
            .iter()
            .any(|image| image.name == "ASSETS/GRAPHICS/PC_BACKGROUND.PNG"),
        "the 800x600 backgrounds are in the selection"
    );

    // The artifact every reader of this inventory consumes is parseable JSON,
    // not a report that only looks like one.
    let artifact = inventory.json();
    assert_parses_as_json(&artifact);

    if let Ok(dir) = std::env::var("CS_EVIDENCE_DIR") {
        let dir = PathBuf::from(dir);
        let dir = if dir.is_absolute() {
            dir
        } else {
            workspace_root().join(dir)
        };
        std::fs::create_dir_all(&dir).expect("the evidence directory is created");
        let path = dir.join("front-end-screens.json");
        std::fs::write(&path, &artifact).expect("the inventory artifact is written");
        assert!(path.is_file());
    }
}

/// **The retail half of AC04, part two: the captures.** Every screen-capable
/// original image is decoded through production code and drawn on the real
/// adapter, one PNG each, and an image whose frame turns out to be a flat slab
/// is refused by name instead of being written.
#[test]
#[ignore = "requires CS_GAME_DIR and a GPU adapter; run with --include-ignored"]
fn accept_f45_d_retail_gpu_every_screen_capable_original_image_draws_a_measured_frame() {
    let dir = capture_dir();
    let inventory = screens()
        .inventory()
        .expect("both original sources are read end to end");
    let capable = inventory.screen_capable();
    assert_eq!(
        capable.len(),
        68,
        "the measured selection of screen-capable art"
    );

    let mut drawn = 0usize;
    let mut refused_flat = Vec::new();
    let mut digests = std::collections::BTreeSet::new();
    // Two captures may only agree when the pictures themselves agree: a frame
    // that repeats over different pixels means the capture ignored its
    // artwork, and a picture that repeats over different frames means the
    // capture is unstable. The original corpus may legitimately store the same
    // picture twice, so the *pairing* is what is asserted, not uniqueness.
    let mut frame_by_art: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let mut art_by_frame: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let mut art_set = std::collections::BTreeSet::new();
    for image in &capable {
        let art = screens()
            .artwork(image)
            .unwrap_or_else(|error| panic!("{} could not be decoded: {error}", image.name));
        assert_eq!(
            art.extent(),
            (image.width, image.height),
            "{}'s pixels match its measured extent",
            image.name
        );
        let png = dir.join(format!("f45-d-original-{}.png", image.file_stem()));
        let _ = std::fs::remove_file(&png);
        if art.is_uniform() {
            let error = capture_artwork(&image.name, &art, &[], &png)
                .expect_err("a flat original draws nothing and must be refused");
            assert!(matches!(
                error,
                cs_app::ui::front_end::ScreenCaptureError::UniformFrame { .. }
            ));
            assert!(!png.exists(), "a refused capture leaves no file");
            refused_flat.push(image.name.clone());
            continue;
        }
        let capture = capture_artwork(&image.name, &art, &[], &png).unwrap_or_else(|error| {
            panic!(
                "the original screen {} could not be captured: {error}",
                image.name
            )
        });
        assert!(
            capture.drew_screen(),
            "{} drew a frame of {} distinct luminance levels",
            image.name,
            capture.distinct_luminance
        );
        assert!(png.is_file(), "{} wrote {}", image.name, png.display());
        let bytes = std::fs::read(&png).expect("the capture reads back");
        assert_eq!(capture.png_sha256.to_hex(), sha256_hex(&bytes));
        let frame = capture.png_sha256.to_hex();
        let art_digest = sha256_hex(art.rgba());
        if let Some(previous) = frame_by_art.get(&art_digest) {
            assert_eq!(
                *previous, frame,
                "{} is the same picture as an earlier capture but drew another frame",
                image.name
            );
        } else {
            frame_by_art.insert(art_digest.clone(), frame.clone());
        }
        if let Some(previous) = art_by_frame.get(&frame) {
            assert_eq!(
                *previous, art_digest,
                "{} drew a frame another picture already drew: the capture ignored its pixels",
                image.name
            );
        } else {
            art_by_frame.insert(frame.clone(), art_digest.clone());
        }
        art_set.insert(art_digest);
        digests.insert(frame);
        drawn += 1;
    }

    assert_eq!(
        drawn + refused_flat.len(),
        capable.len(),
        "every selected image was either drawn or refused by name"
    );
    assert!(
        drawn > 0,
        "at least the measured front-end backgrounds draw: refused flat {:?}",
        refused_flat
    );
    assert_eq!(
        art_set.len(),
        digests.len(),
        "frames and pictures are in one-to-one correspondence: {} distinct frames over {} \
         distinct pictures from {drawn} captures",
        digests.len(),
        art_set.len()
    );
    eprintln!(
        "{drawn} original screens captured, {} distinct frames, {} distinct pictures, refused \
         flat {:?}",
        digests.len(),
        art_set.len(),
        refused_flat
    );
}

/// SHA-256 of bytes as the workspace spells it, for a capture digest check.
fn sha256_hex(bytes: &[u8]) -> String {
    cs_assets::install::sha256(bytes).to_hex()
}
