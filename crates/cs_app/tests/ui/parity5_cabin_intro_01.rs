//! Task #1837 `PARITY5-CABIN-INTRO.01` (findings only): which retail movie is
//! the post-pilot intro, what gates and ends it, and the `PassengerCabin`
//! layout with its five brass buttons. Task test prefix:
//! `accept_parity5_cabin_intro_01_`.
//!
//! Every test reads `$CS_GAME_DIR` through production readers
//! ([`FrontEndScreens`], [`OriginalLayout`], [`ScreenHandlers`],
//! [`MpegVideoSource`], [`MpegAudioSource`]) and pins the numbers the finding
//! `docs/findings/2026-10-21-parity5-cabin-intro-measurement.md` records. Each
//! fails loudly without `CS_GAME_DIR` (`.expect`), and is ignored in CI. The
//! original executable was not run: nothing here is `verified_original`.

use std::path::PathBuf;

use cs_app::cinematics::mpeg::{MpegAudioSource, MpegVideoSource};
use cs_app::cinematics::playback::FrameSource;
use cs_app::ui::front_end::{BUTTON_STATE_FRAMES, FrontEndScreens, ScreenHandlers};
use cs_types::content::{ContentId, ContentKind};

type PaneRow<'a> = (&'a str, &'a str, [&'a str; 3], Option<(u32, u32)>);
type ButtonRow<'a> = (&'a str, &'a str, u32, u32, &'a str, Option<(u32, u32)>);

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

fn movie(name: &str) -> (PathBuf, ContentId) {
    (
        game_dir()
            .join("GOSDATA/ASSETS/GRAPHICS/MPG")
            .join(format!("{name}.mpg")),
        ContentId::from_source(ContentKind::Video, name).expect("a valid content id"),
    )
}

/// The widest difference between two colour channels of any pixel in the
/// frame, and the first and last row holding a pixel brighter than 24.
fn frame_profile(rgba: &[u8]) -> (u8, usize, usize) {
    let (mut spread, mut first, mut last) = (0_u8, usize::MAX, 0_usize);
    for (y, row) in rgba.chunks(320 * 4).enumerate() {
        for px in row.chunks(4) {
            spread = spread
                .max(px[0].abs_diff(px[1]))
                .max(px[1].abs_diff(px[2]))
                .max(px[0].abs_diff(px[2]));
        }
        if row.chunks(4).any(|px| px[..3].iter().any(|&c| c > 24)) {
            first = first.min(y);
            last = y;
        }
    }
    (spread, first, last)
}

/// The decoded frame `wanted` of a movie and its [`frame_profile`].
fn profile_at(name: &str, wanted: u64) -> (u8, usize, usize) {
    let (path, id) = movie(name);
    let mut video = MpegVideoSource::open(&path, &id).expect("the movie opens");
    let mut index = 0;
    while video.next_frame().expect("the movie decodes").is_some() {
        if index == wanted {
            return frame_profile(video.current_rgba().expect("a decoded frame"));
        }
        index += 1;
    }
    panic!("{name} has no frame {wanted}");
}

/// **(a) The intro movie is `chap1.mpg`.** Its measured stream properties and
/// the two frames that match the owner's captures (the man at the window
/// desk, then the woman in the jacket): by mean absolute grey difference
/// against the captures `chap1` frame 462 scores 6.4 and frame 942 scores 4.8
/// where the best frame of every other movie scores 36 or worse (recorded in
/// the finding; the captures stay in `private/`). Here: those frames are
/// grey, and `chap0` (the sepia newsreel) is not.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_cabin_intro_01_the_intro_movie_is_chap1_in_grey_with_audio() {
    let (path, id) = movie("chap1");
    let mut video = MpegVideoSource::open(&path, &id).expect("chap1 opens");
    let info = video.info();
    assert_eq!((info.width, info.height), (320, 240));
    assert!((info.frame_rate - 30.0).abs() < 0.001);
    let seconds = video.duration().unwrap().expect("a duration").as_secs_f64();
    // The decoder reports the video stream's last timestamp; ffprobe's
    // container duration is 130.82 s (see the F40-D1 baseline).
    assert!((seconds - 130.82).abs() < 0.5, "duration {seconds}");
    let mut frames = 0_u64;
    while video.next_frame().unwrap().is_some() {
        frames += 1;
    }
    assert_eq!(frames, 3927);

    let mut audio = MpegAudioSource::open(&path, &id).expect("chap1 has audio");
    assert_eq!(audio.sample_rate(), 44_100);
    let mut samples = 0_u64;
    while let Some(chunk) = audio.next_samples().unwrap() {
        assert_eq!(chunk.len(), 2 * 1152, "interleaved stereo MP2 frames");
        samples += 1152;
    }
    let audio_seconds = samples as f64 / 44_100.0;
    assert!(
        (audio_seconds - 130.87).abs() < 0.1,
        "audio {audio_seconds}"
    );

    // The picture is letterboxed inside the file: rows 28..=211 hold the
    // image, so 2.5x scaling to 800x600 (the layout's `250,250`) leaves the
    // 460-row band the owner's captures show (y 70..530).
    for wanted in [462, 942] {
        let (spread, first, last) = profile_at("chap1", wanted);
        assert!(
            spread <= 8,
            "chap1 frame {wanted} is grey (spread {spread})"
        );
        assert_eq!((first, last), (28, 211), "chap1 frame {wanted} letterbox");
    }
    let (spread, _, _) = profile_at("chap0", 33);
    assert!(
        spread > 100,
        "chap0 is a coloured (sepia) newsreel: {spread}"
    );
}

/// **(b) The gate, the skip and the end.** The cabin runs the intro only when
/// the first-flag global `XQ` (the symbol map names it `fflag`) is set, and
/// only if native callback 2151 (`nmissid`) returns non-zero; `XQ` is set by
/// the Campaign (pilot) page, Instant Action and the Scrapbook, and cleared by
/// the cabin itself. The intro page's four skip inputs and its single-shot end.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_cabin_intro_01_the_gate_skip_and_end_are_the_scripts_statements() {
    let screens = FrontEndScreens::open(&game_dir()).expect("the installation opens");
    let script = |member: &str| {
        screens
            .script_text(&format!("ASSETS/SCRIPTS/{member}.SCRIPT"))
            .unwrap_or_else(|| panic!("{member} reads"))
    };
    let lines = |text: &str| -> Vec<String> { text.lines().map(|l| l.trim().to_owned()).collect() };
    let at = |lines: &[String], needle: &str| -> usize {
        lines
            .iter()
            .position(|l| l == needle)
            .unwrap_or_else(|| panic!("the script states `{needle}`"))
    };

    // The cabin's gate, in order.
    let cabin = lines(&script("PASSENGERCABIN"));
    let gate = at(&cabin, "if (@globals@XQ)");
    let want = [
        "{",
        "int FC",
        "callback($$E$$, 2151, FC)",
        "if (FC != 0)",
        "{",
        "script_run \"assets\\\\scripts\\\\\" \"campaignintro.script\", 0x1999",
        "script_pause @passengercabin@, 2",
        "}",
        "}",
        "else",
        "{",
        "mail (11004, @globals@RR)",
        "}",
        "@globals@XQ = 0",
    ];
    assert_eq!(&cabin[gate + 1..gate + 1 + want.len()], &want);

    // Who sets the flag.
    for (member, set) in [
        ("CAMPAIGN", true),
        ("INSTANTACTION", true),
        ("SCRAPBOOK", true),
    ] {
        let text = lines(&script(member));
        assert_eq!(text.iter().any(|l| l == "@globals@XQ = 1"), set, "{member}");
    }
    assert!(lines(&script("GLOBALS")).iter().any(|l| l == "int XQ = 0"));

    // The Campaign page enters the cabin only after the start button's path
    // and after `callback($$UB$$, 2141, 0)`.
    let campaign = lines(&script("CAMPAIGN"));
    let enter = at(&campaign, "callback($$UB$$, 2141, 0)");
    assert_eq!(
        campaign[enter + 1],
        "script_run \"assets\\\\scripts\\\\\" \"passengercabin.script\", 0x1000"
    );

    // The intro page: name construction, skip inputs, one-shot end.
    let intro = lines(&script("CAMPAIGNINTRO"));
    at(&intro, "GC.BL = \"chap\" conv$(FC) \".mpg\"");
    at(&intro, "GC.YC = \"cm_movie\"");
    let keys = at(&intro, "switch (getchar())");
    assert_eq!(
        &intro[keys + 2..keys + 6],
        [
            "case key_escape:",
            "case key_space:",
            "case key_return:",
            "mail (11006, this)"
        ]
    );
    let click = at(&intro, "lbutton_update");
    assert_eq!(intro[click + 2], "if (mouse.left == button_pressed)");
    assert_eq!(intro[click + 4], "mail (11006, this)");
    let done = at(&intro, "if (11006 == getmessage())");
    assert_eq!(
        &intro[done + 2..done + 9],
        [
            "if (!EC)",
            "{",
            "EC = 1",
            "script_continue @passengercabin@",
            "script_end @campaignintro@",
            "}",
            "}"
        ]
    );

    // The layout record the intro page binds, and that no other movie row is
    // an intro: `CM_MOVIE` names CrimFlag.MPG but the page overwrites `BL`.
    let layout = screens.original_layout().expect("the layout parses");
    let section = layout.section("CampaignIntro").expect("the intro section");
    assert_eq!(section.other.len(), 1);
    let row = &section.other[0];
    assert_eq!((row.id.as_str(), row.kind.as_str()), ("CM_MOVIE", "M"));
    assert_eq!(
        row.fields,
        ["CrimFlag.MPG", "0", "0", "0", "250", "250", "1", "0"]
    );
}

/// **(c) The `PassengerCabin` layout.** Every pane and button row with the art
/// extents the production loader measures, the five brass buttons' rectangles
/// (art width by one of [`BUTTON_STATE_FRAMES`] frames), the screens they
/// start and the `case` arms the script gives them.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_cabin_intro_01_the_cabin_layout_and_its_five_buttons() {
    let screens = FrontEndScreens::open(&game_dir()).expect("the installation opens");
    let layout = screens.original_layout().expect("the layout parses");
    let section = layout.section("PassengerCabin").expect("the cabin section");

    // Panes: id, art, x, y, z, frames, region, alpha type, volatile; extent.
    let panes: Vec<PaneRow<'_>> = section
        .panes
        .iter()
        .map(|r| {
            (
                r.id.as_str(),
                r.fields[0].as_str(),
                [
                    r.fields[1].as_str(),
                    r.fields[2].as_str(),
                    r.fields[3].as_str(),
                ],
                screens.art_extent(&r.fields[0]),
            )
        })
        .collect();
    assert_eq!(
        panes,
        [
            (
                "PC_BACKGROUND",
                "PC_BackGround.png",
                ["0", "0", "4"],
                Some((800, 600))
            ),
            (
                "PC_PLANE",
                "PC_PlaneTemp.png",
                ["46", "69", "1"],
                Some((728, 339))
            ),
            ("PC_MEMENTO", "", ["169", "325", "2"], None),
            (
                "PC_FRAME",
                "PC_Mementopicframe.png",
                ["169", "317", "5"],
                Some((93, 115))
            ),
            (
                "PC_PIN0",
                "PC_mappins.png",
                ["39", "541", "0"],
                Some((50, 100))
            ),
            (
                "PC_PIN1",
                "PC_mappins.png",
                ["0", "458", "0"],
                Some((50, 100))
            ),
            (
                "PC_PIN2",
                "PC_mappins.png",
                ["7", "507", "0"],
                Some((50, 100))
            ),
            (
                "PC_PIN3",
                "PC_mappins.png",
                ["78", "483", "0"],
                Some((50, 100))
            ),
            (
                "PC_PIN4",
                "PC_mappins.png",
                ["332", "445", "0"],
                Some((50, 100))
            ),
        ]
    );

    // Buttons: id, art, x, y, started section, art extent. The columns are
    // `ID=B,art,X,Y,Z,ResID,Script,Priority,EndScript`.
    let buttons: Vec<ButtonRow<'_>> = section
        .buttons
        .iter()
        .map(|r| {
            (
                r.id.as_str(),
                r.fields[0].as_str(),
                r.fields[1].parse().unwrap(),
                r.fields[2].parse().unwrap(),
                r.fields[5].as_str(),
                screens.art_extent(&r.fields[0]),
            )
        })
        .collect();
    assert_eq!(
        buttons,
        [
            (
                "PC_B_CHANGEMOMENTO",
                "PC_B_ChangeMemento.png",
                259,
                384,
                "MomentoSelection",
                Some((182, 128))
            ),
            (
                "PC_B_NEWMISSION",
                "PC_B_NextMission.png",
                87,
                504,
                "",
                Some((221, 168))
            ),
            (
                "PC_B_PREVIOUS",
                "PC_B_PreviousMissions.png",
                361,
                539,
                "ScrapBook_TOC",
                Some((201, 129))
            ),
            (
                "PC_B_RETURNMM",
                "PC_B_ReturnMainMenu.png",
                593,
                561,
                "MainMenu",
                Some((200, 128))
            ),
            (
                "PC_B_PLANEX",
                "PC_B_PlaneConstruction.png",
                452,
                431,
                "PlaneName",
                Some((200, 128))
            ),
            (
                "PC_B_SAVE",
                "PC_B_SaveGame.png",
                600,
                385,
                "",
                Some((182, 128))
            ),
        ]
    );
    // Four equal frames give the rectangle; the Previous Missions strip
    // (129 rows) does not divide and stays unmeasured.
    let rect =
        |(w, h): (u32, u32)| (h % BUTTON_STATE_FRAMES == 0).then_some((w, h / BUTTON_STATE_FRAMES));
    assert_eq!(rect((182, 128)), Some((182, 32)));
    assert_eq!(rect((221, 168)), Some((221, 42)));
    assert_eq!(rect((200, 128)), Some((200, 32)));
    assert_eq!(rect((201, 129)), None);

    // The script creates the six buttons in this order and gives three of
    // them an arm; the others start their section through the layout row.
    let script = screens
        .script_text("ASSETS/SCRIPTS/PASSENGERCABIN.SCRIPT")
        .expect("the cabin script reads");
    let handlers = ScreenHandlers::parse(&script);
    assert_eq!(
        handlers.buttons(),
        [
            "pc_b_changemomento",
            "pc_b_newmission",
            "pc_b_previous",
            "pc_b_returnmm",
            "pc_b_planex",
            "pc_b_save"
        ]
    );
    assert_eq!(handlers.handler("pc_b_changemomento"), None);
    assert_eq!(handlers.handler("pc_b_previous"), None);
    assert_eq!(handlers.handler("pc_b_returnmm"), None);
    assert_eq!(
        handlers.handler("pc_b_planex"),
        Some(&["$$TGA$$ = 0".to_owned(), "$$CV$$ = 0".to_owned()][..])
    );
    // Next Mission's arm (the line reader stops at the first `}`; the full
    // arm is checked on the raw text).
    let normalised: String = script
        .lines()
        .map(|line| line.trim().to_owned() + "\n")
        .collect();
    for statement in [
        "callback($$E$$, 2104, (QMA.QG + 1))",
        "callback($$E$$, 2104, -2)",
        "mail (11003, @globals@RR)",
        "script_end @passengercabin@",
        "callback($$A$$, 6)",
        "case GMA:\nmail (10003, KMA)",
        "if (!callback($$E$$, 2600))\n{\nmail (10000, LMA)\n}",
        "deactivate(PMA)",
    ] {
        assert!(
            normalised.contains(statement),
            "the cabin script states `{statement}`"
        );
    }
}
