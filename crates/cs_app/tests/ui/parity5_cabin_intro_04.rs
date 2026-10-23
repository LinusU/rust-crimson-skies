//! Acceptance task PARITY5-CABIN-INTRO.04 (#1840): the cabin hub drawn from
//! the original `PassengerCabin` section.
//!
//! * The synthetic tests run over layout text in the original's format and
//!   prove the rules the hub applies (the padded Previous Missions strip, the
//!   deactivated Save button, the hangar art, the pins, hover/press/focus);
//!   they run in CI.
//! * The retail test reads `$CS_GAME_DIR`'s real section and art and checks
//!   the measured claims the rules rest on. The windowed test drives
//!   `cs --front-end --smoke-seconds` on the cabin scene on a GPU and, when
//!   the owner's capture is in `private/ref-drydock/cabin/original.png`,
//!   compares the frame with it. Both are `#[ignore]`d because CI has neither
//!   the installation nor a GPU.

use std::path::PathBuf;

use cs_app::ui::front_end::{
    ButtonVisual, CABIN_SECTION, CabinError, CabinHub, FrontEndScreens, MenuShell, OriginalLayout,
    SAVE_BUTTON, script_member,
};

const LAYOUT: &str = "\
[@PassengerCabin@]
PC_BACKGROUND=P,bg.png,0,0,4,1,0,2,0
PC_PLANE=P,planetemp.png,46,69,1,1,0,0,0
PC_PIN0=P,pins.png,39,541,0,5,0,1,1
PC_PIN1=P,pins.png,0,458,0,5,0,1,1
PC_PIN2=P,pins.png,7,507,0,5,0,1,1
PC_B_NEXT=B,next.png,87,504,0,0,,0x1000,0
PC_B_PREVIOUS=B,previous.png,361,539,0,0,ScrapBook_TOC,0x1000,0
PC_B_SAVE=B,next.png,600,385,0,0,,0x1000,0

[@ScrapBook_TOC@]
SB_P=P,bg.png,0,0,0,1,0,1,0
";

fn extents(art: &str) -> Option<(u32, u32)> {
    match art.to_ascii_lowercase().as_str() {
        "bg.png" => Some((800, 600)),
        "planetemp.png" => Some((728, 339)),
        "pc_p_hangar5.jpg" => Some((720, 334)),
        "pins.png" => Some((50, 100)),
        "next.png" => Some((221, 168)),
        // The measured padded strip (see `PADDED_BUTTON_STRIPS`).
        "previous.png" => Some((201, 128)),
        _ => None,
    }
}

fn shell() -> MenuShell {
    let layout = OriginalLayout::parse(LAYOUT).expect("the fixture parses");
    MenuShell::build(&layout, &|art: &str| extents(art), &|_: &str| None)
}

fn pane<'a>(shell: &'a MenuShell, id: &str) -> Option<&'a cs_app::ui::front_end::ShellPane> {
    shell.sections()["PASSENGERCABIN"]
        .panes
        .iter()
        .find(|pane| pane.id == id)
}

#[test]
fn accept_parity5_cabin_intro_04_the_hub_hides_save_swaps_the_plane_and_keeps_the_pins() {
    let mut shell = shell();
    let hub = CabinHub { hangar: 5, pins: 2 };
    let applied = hub
        .apply(&mut shell, &|art: &str| extents(art))
        .expect("the hub applies");
    assert!(applied.save_hidden);
    assert_eq!(applied.plane_art, "pc_p_hangar5.jpg");
    assert_eq!(applied.pins, ["PC_PIN0", "PC_PIN1"]);

    let buttons: Vec<&str> = shell.sections()["PASSENGERCABIN"]
        .buttons
        .iter()
        .map(|button| button.id.as_str())
        .collect();
    assert_eq!(
        buttons,
        ["PC_B_NEXT", "PC_B_PREVIOUS"],
        "Save is deactivated"
    );

    let plane = pane(&shell, "PC_PLANE").expect("the plane pane stays");
    assert_eq!(plane.art, "pc_p_hangar5.jpg");
    assert_eq!(
        (
            plane.rect.x,
            plane.rect.y,
            plane.rect.width,
            plane.rect.height
        ),
        (46, 69, 720, 334),
        "the jpg at the row's X,Y, at its own extent"
    );
    assert!(pane(&shell, "PC_PIN2").is_none(), "a pin never created");
    assert_eq!(pane(&shell, "PC_PIN1").expect("pin 1").frame, 1);

    // The drawn crop of pin 1 is its own 20-row frame of the strip.
    shell.open(CABIN_SECTION).expect("the cabin opens");
    let view = shell.view((800, 600)).expect("a view");
    let pin1 = view
        .panes
        .iter()
        .find(|pane| pane.id == "PC_PIN1")
        .expect("pin 1 is drawn");
    assert_eq!(pin1.texel_offset, (0, 20));
    let pin0 = view.panes.iter().find(|pane| pane.id == "PC_PIN0").unwrap();
    assert_eq!(pin0.texel_offset, (0, 0));
}

#[test]
fn accept_parity5_cabin_intro_04_a_missing_hangar_or_section_leaves_the_layout_alone() {
    let mut shell = shell();
    let before = shell.sections()["PASSENGERCABIN"].clone();
    let hub = CabinHub {
        hangar: 99,
        pins: 1,
    };
    let error = hub
        .apply(&mut shell, &|art: &str| extents(art))
        .expect_err("no such hangar art");
    assert_eq!(
        error,
        CabinError::NoHangar {
            art: "pc_p_hangar99.jpg".to_owned()
        }
    );
    assert_eq!(shell.sections()["PASSENGERCABIN"], before);

    let layout = OriginalLayout::parse("[@Other@]\nP=P,bg.png,0,0,0,1,0,1,0\n").unwrap();
    let mut other = MenuShell::build(&layout, &|art: &str| extents(art), &|_: &str| None);
    assert_eq!(
        CabinHub::NEW_PILOT.apply(&mut other, &|art: &str| extents(art)),
        Err(CabinError::NoSection)
    );
}

#[test]
fn accept_parity5_cabin_intro_04_the_padded_previous_missions_strip_is_a_button() {
    // 129 rows: four 32-row frames and one padding row, measured on the retail
    // art; any other art whose height is not a multiple of four is refused.
    let layout = OriginalLayout::parse(
        "[@PassengerCabin@]\n\
         PC_B_PREVIOUS=B,PC_B_PreviousMissions.png,361,539,0,0,,0x1000,0\n\
         PC_B_OTHER=B,other.png,10,10,0,0,,0x1000,0\n",
    )
    .unwrap();
    let measure = |art: &str| match art.to_ascii_lowercase().as_str() {
        "pc_b_previousmissions.png" => Some((201, 129)),
        "other.png" => Some((201, 129)),
        _ => None,
    };
    let shell = MenuShell::build(&layout, &measure, &|_: &str| None);
    let section = &shell.sections()["PASSENGERCABIN"];
    assert_eq!(section.buttons.len(), 1);
    assert_eq!(section.buttons[0].id, "PC_B_PREVIOUS");
    assert_eq!(
        (
            section.buttons[0].rect.width,
            section.buttons[0].rect.height
        ),
        (201, 32)
    );
    assert_eq!(section.skipped.len(), 1);
    assert_eq!(section.skipped[0].id, "PC_B_OTHER");
}

#[test]
fn accept_parity5_cabin_intro_04_hover_press_and_focus_draw_the_strip_frames() {
    let mut shell = shell();
    CabinHub::NEW_PILOT
        .apply(&mut shell, &|art: &str| extents(art))
        .expect("the hub applies");
    shell.open(CABIN_SECTION).expect("the cabin opens");
    // The script focuses the control host, not a button: both rest active.
    assert_eq!(shell.focus(), None);
    assert_eq!(shell.visual(0), ButtonVisual::Active);
    assert_eq!(shell.visual(1), ButtonVisual::Active);
    // The first Down focuses the first button (rollover frame); the walk
    // wraps over the two buttons Save's removal leaves.
    shell.move_focus(true);
    assert_eq!(shell.focus(), Some(0));
    assert_eq!(shell.visual(0), ButtonVisual::Rollover);
    shell.move_focus(true);
    assert_eq!(shell.focus(), Some(1));
    shell.move_focus(true);
    assert_eq!(shell.focus(), Some(0));
    // The pointer over Previous Missions: rollover; held: depressed.
    shell.pointer_move(370, 545);
    assert_eq!(shell.visual(1), ButtonVisual::Rollover);
    shell.pointer_down();
    assert_eq!(shell.visual(1), ButtonVisual::Depressed);
    // The press frame is the strip's fourth 32-row frame.
    let view = shell.view((800, 600)).unwrap();
    let previous = view
        .buttons
        .iter()
        .find(|button| button.id == "PC_B_PREVIOUS")
        .unwrap();
    assert_eq!(previous.texel_offset.1, 3 * 32);
    // The deactivated Save button is not drawn or focusable.
    assert!(
        shell.sections()["PASSENGERCABIN"]
            .buttons
            .iter()
            .all(|button| button.id != SAVE_BUTTON)
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_cabin_intro_04_retail_cabin_has_five_brass_buttons_the_hangar_and_one_pin() {
    let dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must point at the original installation"),
    );
    let screens = FrontEndScreens::open(&dir).expect("the original screen container opens");
    let layout = screens.original_layout().expect("the layout member reads");
    let mut shell = MenuShell::build(
        &layout,
        &|art: &str| screens.art_extent(art),
        &|section: &str| screens.script_text(&script_member(section)),
    );
    let applied = CabinHub::NEW_PILOT
        .apply(&mut shell, &|art: &str| screens.art_extent(art))
        .expect("the hub applies to the retail section");
    assert!(applied.save_hidden);
    assert_eq!(applied.plane_art, "pc_p_hangar5.jpg");
    assert_eq!(applied.pins, ["PC_PIN0"]);

    let section = &shell.sections()["PASSENGERCABIN"];
    let rects: Vec<(&str, i32, i32, u32, u32)> = section
        .buttons
        .iter()
        .map(|b| {
            (
                b.id.as_str(),
                b.rect.x,
                b.rect.y,
                b.rect.width,
                b.rect.height,
            )
        })
        .collect();
    assert_eq!(
        rects,
        [
            ("PC_B_CHANGEMOMENTO", 259, 384, 182, 32),
            ("PC_B_NEWMISSION", 87, 504, 221, 42),
            ("PC_B_PREVIOUS", 361, 539, 201, 32),
            ("PC_B_RETURNMM", 593, 561, 200, 32),
            ("PC_B_PLANEX", 452, 431, 200, 32),
        ],
        "the five brass buttons, in the script's focus order, at the layout's rects"
    );
    // Only the memento picture is not drawn: its choice is engine state.
    let skipped: Vec<&str> = section.skipped.iter().map(|row| row.id.as_str()).collect();
    assert_eq!(skipped, ["PC_MEMENTO"]);
    let plane = section
        .panes
        .iter()
        .find(|pane| pane.id == "PC_PLANE")
        .unwrap();
    assert_eq!((plane.rect.width, plane.rect.height), (720, 334));

    // The padded strip: the art's four frames repeat at period 32 and its
    // 129th row has no opaque texel (the claim `PADDED_BUTTON_STRIPS` rests on).
    let art = screens
        .graphic_artwork("PC_B_PreviousMissions.png")
        .expect("the strip decodes");
    let (w, h) = (art.width() as usize, art.height() as usize);
    assert_eq!((w, h), (201, 129));
    let row = |y: usize| &art.rgba()[y * w * 4..(y + 1) * w * 4];
    let opaque = |y: usize| row(y).chunks(4).filter(|p| p[3] > 8).count();
    assert_eq!(opaque(128), 0, "the padding row is empty");
    for y in 0..32 {
        assert_eq!(
            opaque(y),
            opaque(y + 32),
            "frame 1 repeats frame 0 at row {y}"
        );
        assert_eq!(opaque(y), opaque(y + 64));
        assert_eq!(opaque(y), opaque(y + 96));
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR + gpu and a built windowed binary"]
fn accept_parity5_cabin_intro_04_the_windowed_smoke_draws_the_cabin_and_its_button_states() {
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is unset: this run needs the original installation");
    };
    let Ok(capabilities) = std::env::var("CS_CAPABILITIES") else {
        panic!("CS_CAPABILITIES is unset: the front-end launch needs gpu");
    };
    assert!(
        capabilities.split(',').any(|part| part.trim() == "gpu"),
        "CS_CAPABILITIES must list gpu: {capabilities}"
    );
    let dir = std::env::var("CS_CABIN_SMOKE_OUT").map_or_else(
        |_| std::env::temp_dir().join("parity5-cabin-hub-smoke"),
        PathBuf::from,
    );
    std::fs::create_dir_all(&dir).expect("the capture directory is created");
    for stale in [
        "report.json",
        "cabin.png",
        "cabin-hover.png",
        "cabin-pressed.png",
        "cabin-focus.png",
    ] {
        let _ = std::fs::remove_file(dir.join(stale));
    }
    let output = std::process::Command::new(
        std::env::var("CS_FRONT_END_BIN")
            .map_or_else(|_| PathBuf::from(env!("CARGO_BIN_EXE_cs")), PathBuf::from),
    )
    .args(["--cs-path", &game_dir, "--front-end"])
    .args(["--smoke-seconds", "14", "--capture-dir"])
    .arg(&dir)
    .env("CS_CAPABILITIES", capabilities)
    .env(cs_app::ui::front_end::SMOKE_SCENE_ENV, "cabin")
    .output()
    .expect("the cs binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let report = std::fs::read_to_string(dir.join("report.json"))
        .unwrap_or_else(|error| panic!("report.json is written: {error}\n{stderr}"));
    assert!(
        report.contains("\"passed\":true"),
        "the run passed its own checks: {report}\nstderr: {stderr}"
    );
    assert!(
        report.contains("\"visited\":[\"PassengerCabin\"]"),
        "{report}"
    );
    for label in ["cabin", "cabin-hover", "cabin-pressed", "cabin-focus"] {
        assert!(
            dir.join(format!("{label}.png")).is_file(),
            "{label}.png captured: {report}"
        );
    }
    assert!(output.status.success(), "exit 0; stderr: {stderr}");
}
