//! Task #1957: the cabin's Change Memento screen (`MomentoSelection`) behind
//! the cabin button. Task test prefix: `accept_parity5_cabin_change_memento_`.
//!
//! The synthetic tests run the production [`StartFlow`] over a layout in the
//! original's format and press the screen's buttons through the shell's pointer
//! path. The retail tests read the installation's own layout, art and the
//! decrypted engine image and are ignored in CI.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cs_app::ui::front_end::{
    DEFAULT_MEMENTO, FrontEndScreens, MEMENTO_ACCEPT_BUTTON, MEMENTO_ARROW_LEFT_BUTTON,
    MEMENTO_ARROW_RIGHT_BUTTON, MEMENTO_CANCEL_BUTTON, MEMENTO_PANE, MEMENTO_SECTION, MEMENTOS,
    MementoDirection, MementoGate, MementoUnlocks, MenuShell, MissionEnd, NOT_IMPLEMENTED,
    OriginalLayout, Screen, StartFlow, StartStep, memento_step, picture_art,
};

const LAYOUT: &str = "\
[@PassengerCabin@]
PC_BACKGROUND=P,bg.png,0,0,4,1,0,2,0
PC_B_NEWMISSION=B,btn.png,0,0,0,0,,0x1000,0
PC_B_PREVIOUS=B,btn.png,0,40,0,0,ScrapBook_TOC,0x1000,0
PC_B_CHANGEMOMENTO=B,btn.png,0,80,0,0,MomentoSelection,0x1000,0
PC_B_RETURNMM=B,btn.png,0,160,0,0,MainMenu,0x1000,0

[@ScrapBook_TOC@]
SB_P=P,bg.png,0,0,0,1,0,1,0

[@MomentoSelection@]
MS_BACKGROUND=P,bg.png,0,0,2,1,0,2,0
MS_MEMENTO=P,,184,48,1,1,0,0,0
MS_B_ACCEPT=B,btn.png,0,200,0,0,PassengerCabin,0x1000,1
MS_B_CANCEL=B,btn.png,0,240,0,0,PassengerCabin,0x1000,1
MS_B_ARROWL=B,btn.png,0,280,0,0,,0x1000,
MS_B_ARROWR=B,btn.png,0,320,0,0,,0x1000,

[@MainMenu@]
MM_P=P,bg.png,0,0,0,1,0,1,0
";

const ACCEPT_Y: i32 = 200;
const CANCEL_Y: i32 = 240;
const LEFT_Y: i32 = 280;
const RIGHT_Y: i32 = 320;

fn shell() -> MenuShell {
    let layout = OriginalLayout::parse(LAYOUT).expect("the fixture parses");
    let extents = |art: &str| match art {
        "bg.png" => Some((800, 600)),
        // Four 32-row frames.
        "btn.png" => Some((200, 128)),
        _ => None,
    };
    MenuShell::build(&layout, &extents, &|_: &str| None)
}

/// A few pictures' extents, by lower-cased art name.
fn pictures() -> BTreeMap<String, (u32, u32)> {
    MEMENTOS
        .iter()
        .map(|entry| (picture_art(entry.name).to_ascii_lowercase(), (300, 400)))
        .collect()
}

fn open_screen(unlocks: MementoUnlocks) -> (StartFlow, MenuShell) {
    let mut flow = StartFlow::after_mission(MissionEnd::ReturnedToCabin)
        .expect("the walk")
        .with_memento_unlocks(unlocks);
    flow.set_memento_pictures(pictures());
    let mut shell = shell();
    shell.open("PassengerCabin").expect("the cabin opens");
    let step = press(&mut flow, &mut shell, 90);
    assert_eq!(step, StartStep::Memento);
    // The picture's pane was placed when the press was routed.
    assert_eq!(picture_pane(&shell).0, picture_art(DEFAULT_MEMENTO));
    (flow, shell)
}

fn press(flow: &mut StartFlow, shell: &mut MenuShell, y: i32) -> StartStep {
    shell.pointer_move(10, y);
    shell.pointer_down();
    let outcome = shell.pointer_up();
    let step = flow.route(shell, outcome);
    flow.sync_memento(shell);
    step
}

fn shown(flow: &StartFlow) -> &'static str {
    flow.memento_screen().expect("the screen is open").name()
}

fn picture_pane(shell: &MenuShell) -> (String, i32, i32, u32, u32) {
    let section = shell.section().expect("a section");
    let pane = section
        .panes
        .iter()
        .find(|pane| pane.id.eq_ignore_ascii_case(MEMENTO_PANE))
        .expect("the memento pane");
    (
        pane.art.clone(),
        pane.rect.x,
        pane.rect.y,
        pane.rect.width,
        pane.rect.height,
    )
}

#[test]
fn accept_parity5_cabin_change_memento_button_opens_the_screen_not_the_notice() {
    let (flow, shell) = open_screen(MementoUnlocks::new_pilot());
    assert_eq!(flow.screen(), Screen::Cabin);
    assert_eq!(shell.section_name(), Some(MEMENTO_SECTION));
    assert!(flow.notice().is_none());
    assert!(flow.not_implemented_seen().is_empty());
    assert!(
        NOT_IMPLEMENTED
            .iter()
            .all(|entry| entry.button != "PC_B_CHANGEMOMENTO"),
        "Change Memento is no longer a marked screen"
    );
    // It opens on the pilot's memento, drawn under the layout's panes at the
    // row's position and the picture's own extent.
    assert_eq!(shown(&flow), DEFAULT_MEMENTO);
    let (art, x, y, w, h) = picture_pane(&shell);
    assert_eq!(art, picture_art(DEFAULT_MEMENTO));
    assert_eq!((x, y, w, h), (184, 48, 300, 400));
    let first = &shell.section().expect("a section").panes[0];
    assert!(first.id.eq_ignore_ascii_case(MEMENTO_PANE));
}

#[test]
fn accept_parity5_cabin_change_memento_arrows_walk_the_engine_table_and_skip_locked_rows() {
    let (mut flow, mut shell) = open_screen(MementoUnlocks::new_pilot());
    let mut seen = vec![shown(&flow)];
    for _ in 0..7 {
        assert_eq!(press(&mut flow, &mut shell, RIGHT_Y), StartStep::Memento);
        seen.push(shown(&flow));
    }
    // A new pilot has the seven unconditional pictures and nothing else: the
    // walk wraps past the locked rows and MyMemento (no file) to the first.
    assert_eq!(
        seen,
        [
            "MS_P_InitialPinup1.jpg",
            "MS_P_InitialPinup3.jpg",
            "MS_P_JustineBattleax.jpg",
            "MS_P_Mom.jpg",
            "MS_P_DoggiePhoto.jpg",
            "MS_P_Swan&NathaninCabin.jpg",
            "MS_P_ZacharyandPlane.jpg",
            "MS_P_InitialPinup1.jpg",
        ]
    );
    // The left arrow goes back, wrapping from the first to the last offered.
    assert_eq!(press(&mut flow, &mut shell, LEFT_Y), StartStep::Memento);
    assert_eq!(shown(&flow), "MS_P_ZacharyandPlane.jpg");
    assert_eq!(
        picture_pane(&shell).0,
        picture_art("MS_P_ZacharyandPlane.jpg")
    );
}

#[test]
fn accept_parity5_cabin_change_memento_mission_flags_unlock_pictures() {
    let none = MementoUnlocks::new_pilot();
    let last_always = 7;
    let after_always = memento_step(last_always, MementoDirection::Next, &none);
    assert_eq!(after_always, 1);

    // Mission 2's flags word needs bit 0 and bit 1.
    let bit1_only = MementoUnlocks::new_pilot().with_mission_flags(2, 0b10);
    assert_eq!(memento_step(7, MementoDirection::Next, &bit1_only), 1);
    let both = MementoUnlocks::new_pilot().with_mission_flags(2, 0b11);
    assert_eq!(
        MEMENTOS[memento_step(7, MementoDirection::Next, &both)].name,
        "MS_P_02_01_hawaiianPrinces2.jpg"
    );
    // The row after it is locked again, so the walk wraps.
    assert_eq!(memento_step(8, MementoDirection::Next, &both), 1);
    // Mission 7's picture is gated on bit 0 alone.
    let seven = MementoUnlocks::new_pilot().with_mission_flags(7, 1);
    assert_eq!(
        MEMENTOS[memento_step(7, MementoDirection::Next, &seven)].name,
        "MS_P_IllsaandSparks.jpg"
    );
    // MyMemento.jpg is offered once the pilot's file exists, and is the row
    // the left arrow reaches from the first.
    let own = MementoUnlocks::new_pilot().with_own_file();
    assert_eq!(memento_step(1, MementoDirection::Previous, &own), 0);
    assert_eq!(memento_step(1, MementoDirection::Previous, &none), 7);

    // Through the screen: the unlocked pilot's right arrow reaches it.
    let (mut flow, mut shell) = open_screen(both);
    for _ in 0..7 {
        press(&mut flow, &mut shell, RIGHT_Y);
    }
    assert_eq!(shown(&flow), "MS_P_02_01_hawaiianPrinces2.jpg");
}

#[test]
fn accept_parity5_cabin_change_memento_accept_stores_and_cancel_keeps() {
    let (mut flow, mut shell) = open_screen(MementoUnlocks::new_pilot());
    press(&mut flow, &mut shell, RIGHT_Y);
    press(&mut flow, &mut shell, RIGHT_Y);
    assert_eq!(shown(&flow), "MS_P_JustineBattleax.jpg");
    // Cancel: the cabin is back and the choice is unchanged.
    assert_eq!(press(&mut flow, &mut shell, CANCEL_Y), StartStep::Memento);
    assert_eq!(shell.section_name(), Some("PassengerCabin"));
    assert_eq!(flow.screen(), Screen::Cabin);
    assert!(flow.memento_screen().is_none());
    assert_eq!(flow.memento(), DEFAULT_MEMENTO);

    // Reopened on the stored memento; Accept stores the shown one.
    assert_eq!(press(&mut flow, &mut shell, 90), StartStep::Memento);
    assert_eq!(shown(&flow), DEFAULT_MEMENTO);
    press(&mut flow, &mut shell, RIGHT_Y);
    press(&mut flow, &mut shell, RIGHT_Y);
    press(&mut flow, &mut shell, RIGHT_Y);
    assert_eq!(press(&mut flow, &mut shell, ACCEPT_Y), StartStep::Memento);
    assert_eq!(shell.section_name(), Some("PassengerCabin"));
    assert_eq!(flow.memento(), "MS_P_Mom.jpg");

    // The next visit opens on it, and the other cabin buttons still work.
    assert_eq!(press(&mut flow, &mut shell, 90), StartStep::Memento);
    assert_eq!(shown(&flow), "MS_P_Mom.jpg");
    assert_eq!(flow.memento_screen().expect("open").shown(), 4);
    assert!(flow.not_implemented_seen().is_empty());
}

#[test]
fn accept_parity5_cabin_change_memento_esc_is_not_bound_on_the_screen() {
    // The script has no key handler: Esc neither cancels nor leaves.
    let (mut flow, mut shell) = open_screen(MementoUnlocks::new_pilot());
    assert_eq!(flow.route_escape(&mut shell), StartStep::Stay);
    assert_eq!(shell.section_name(), Some(MEMENTO_SECTION));
    assert!(flow.memento_screen().is_some());
}

// ------------------------------------------------------------- retail ---

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// The retail layout, art and script handlers as the window builds them.
fn retail() -> (FrontEndScreens, MenuShell) {
    let screens = FrontEndScreens::open(&game_dir()).expect("the installation opens");
    let layout = screens.original_layout().expect("the layout parses");
    let shell = MenuShell::build(
        &layout,
        &|art: &str| screens.art_extent(art),
        &|section: &str| screens.script_text(&cs_app::ui::front_end::script_member(section)),
    );
    (screens, shell)
}

fn press_button(flow: &mut StartFlow, shell: &mut MenuShell, id: &str) -> StartStep {
    let (x, y) = {
        let section = shell.section().expect("a section");
        let button = section
            .buttons
            .iter()
            .find(|button| button.id.eq_ignore_ascii_case(id))
            .unwrap_or_else(|| panic!("{id} is on the section"));
        (
            button.rect.x + (button.rect.width / 2) as i32,
            button.rect.y + (button.rect.height / 2) as i32,
        )
    };
    shell.pointer_move(x, y);
    shell.pointer_down();
    let outcome = shell.pointer_up();
    let step = flow.route(shell, outcome);
    flow.sync_memento(shell);
    step
}

/// **The retail screen.** The installation's own `MomentoSelection` section
/// opens from the cabin button; every picture of the engine's table that the
/// installation holds resolves; the arrows are the layout's, and Accept and
/// Cancel return to the cabin.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_cabin_change_memento_retail_screen() {
    let (screens, mut shell) = retail();
    let mut flow = StartFlow::after_mission(MissionEnd::ReturnedToCabin).expect("the walk");
    let mut extents = BTreeMap::new();
    let mut missing = Vec::new();
    for entry in &MEMENTOS {
        match screens.art_extent(&picture_art(entry.name)) {
            Some(extent) => {
                extents.insert(picture_art(entry.name), extent);
            }
            None => missing.push(entry.name),
        }
    }
    // Only the pilot's own file is absent from the installation.
    assert_eq!(
        missing,
        ["MyMemento.jpg"],
        "pictures the installation lacks"
    );
    flow.set_memento_pictures(extents);
    shell.open("PassengerCabin").expect("the cabin opens");

    let step = press_button(&mut flow, &mut shell, "PC_B_CHANGEMOMENTO");
    assert_eq!(step, StartStep::Memento);
    assert_eq!(shell.section_name(), Some(MEMENTO_SECTION));
    assert!(
        flow.sync_memento(&mut shell) || picture_pane(&shell).0 == picture_art(DEFAULT_MEMENTO)
    );
    let (art, x, y, w, h) = picture_pane(&shell);
    assert_eq!(art, picture_art(DEFAULT_MEMENTO));
    assert_eq!((x, y), (184, 48));
    assert!(w > 0 && h > 0);

    let ids: Vec<&str> = shell
        .section()
        .expect("a section")
        .buttons
        .iter()
        .map(|button| button.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            MEMENTO_ACCEPT_BUTTON,
            MEMENTO_CANCEL_BUTTON,
            MEMENTO_ARROW_LEFT_BUTTON,
            MEMENTO_ARROW_RIGHT_BUTTON
        ]
    );

    assert_eq!(
        press_button(&mut flow, &mut shell, MEMENTO_ARROW_RIGHT_BUTTON),
        StartStep::Memento
    );
    assert_eq!(shown(&flow), "MS_P_InitialPinup3.jpg");
    assert_eq!(
        press_button(&mut flow, &mut shell, MEMENTO_ARROW_LEFT_BUTTON),
        StartStep::Memento
    );
    assert_eq!(shown(&flow), DEFAULT_MEMENTO);
    press_button(&mut flow, &mut shell, MEMENTO_ARROW_RIGHT_BUTTON);
    assert_eq!(
        press_button(&mut flow, &mut shell, MEMENTO_ACCEPT_BUTTON),
        StartStep::Memento
    );
    assert_eq!(shell.section_name(), Some("PassengerCabin"));
    assert_eq!(flow.memento(), "MS_P_InitialPinup3.jpg");
    assert!(flow.not_implemented_seen().is_empty());
}

/// **The table is the engine's.** Re-reads the 24 twelve-byte records at
/// `0x61af60` of the decrypted image and the terminator, and compares every
/// name and gate with [`MEMENTOS`].
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity5_cabin_change_memento_table_matches_the_engine_image() {
    let path = std::env::var("CS_ENGINE_IMAGE")
        .expect("CS_ENGINE_IMAGE must name the decrypted engine image for this test");
    let image = std::fs::read(path).expect("the image reads");
    let base = 0x40_0000_u32;
    let word = |va: u32| {
        let at = (va - base) as usize;
        u32::from_le_bytes(image[at..at + 4].try_into().expect("four bytes"))
    };
    let text = |va: u32| {
        let at = (va - base) as usize;
        let end = image[at..]
            .iter()
            .position(|b| *b == 0)
            .expect("terminated");
        String::from_utf8(image[at..at + end].to_vec()).expect("ascii")
    };
    let table = cs_app::ui::front_end::MEMENTO_TABLE_VA;
    for (index, entry) in MEMENTOS.iter().enumerate() {
        let row = table + 12 * index as u32;
        assert_eq!(text(word(row)), entry.name, "row {index}");
        let record = word(row + 4) as i32;
        let bit = word(row + 8);
        match entry.gate {
            MementoGate::OwnFile => assert_eq!(record, -1, "row {index}"),
            MementoGate::Always => assert_eq!(record, 0, "row {index}"),
            MementoGate::Mission { mission, bit: want } => {
                assert_eq!((record, bit), (mission as i32, want), "row {index}");
            }
        }
    }
    // The 25th pointer names a string in `.data`'s uninitialised tail (the
    // section's raw data ends at RVA 0x243000), which is empty at run time:
    // the table ends at 24 rows.
    let end = table + 12 * MEMENTOS.len() as u32;
    let terminator = word(end);
    assert!(
        (0x24_3000..0x24_3000 + 0x40_0000).contains(&(terminator - base)),
        "the terminator row points into .data's zero-filled tail"
    );
}
