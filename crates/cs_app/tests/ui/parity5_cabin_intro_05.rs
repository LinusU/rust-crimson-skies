//! Acceptance task PARITY5-CABIN-INTRO.05 (#1841): the cabin's buttons lead
//! somewhere. Task test prefix: `accept_parity5_cabin_intro_05_`.
//!
//! Next Mission opens the briefing (the mission session's screen, so the flow
//! hands over), Return to Main Menu goes back with the profile closed, and the
//! Previous Missions opens its real screen (#1956) and Plane Construction its
//! name dialog (#1958); the button whose screen is not built opens the marked
//! "not implemented" screen. The layout is
//! synthetic text in the original's format; the buttons are pressed through
//! the shell's pointer path and routed by the production [`StartFlow`].

use cs_app::ui::front_end::{
    MenuShell, MissionEnd, NOT_IMPLEMENTED, OriginalLayout, Screen, StartFlow, StartStep,
};

const LAYOUT: &str = "\
[@PassengerCabin@]
PC_BACKGROUND=P,bg.png,0,0,4,1,0,2,0
PC_B_NEWMISSION=B,btn.png,0,0,0,0,,0x1000,0
PC_B_PREVIOUS=B,btn.png,0,40,0,0,ScrapBook_TOC,0x1000,0
PC_B_CHANGEMOMENTO=B,btn.png,0,80,0,0,MomentoSelection,0x1000,0
PC_B_PLANEX=B,btn.png,0,120,0,0,PlaneName,0x1000,0
PC_B_RETURNMM=B,btn.png,0,160,0,0,MainMenu,0x1000,0

[@ScrapBook_TOC@]
SB_P=P,bg.png,0,0,0,1,0,1,0

[@MomentoSelection@]
MS_P=P,bg.png,0,0,0,1,0,1,0

[@PlaneName@]
PN_P=P,bg.png,0,0,0,1,0,1,0

[@MainMenu@]
MM_P=P,bg.png,0,0,0,1,0,1,0
";

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

fn cabin() -> (StartFlow, MenuShell) {
    let flow = StartFlow::after_mission(MissionEnd::ReturnedToCabin).expect("the walk");
    assert_eq!(flow.screen(), Screen::Cabin);
    let mut shell = shell();
    shell.open("PassengerCabin").expect("the section opens");
    (flow, shell)
}

fn press(flow: &mut StartFlow, shell: &mut MenuShell, y: i32) -> StartStep {
    shell.pointer_move(10, y);
    shell.pointer_down();
    let outcome = shell.pointer_up();
    flow.route(shell, outcome)
}

#[test]
fn accept_parity5_cabin_intro_05_next_mission_opens_the_briefing() {
    let (mut flow, mut shell) = cabin();
    let step = press(&mut flow, &mut shell, 10);
    assert!(
        matches!(step, StartStep::Launch { .. }),
        "Next Mission hands over to the mission session's briefing, got {step:?}"
    );
    assert!(flow.handover().is_some());
    assert!(flow.not_implemented_seen().is_empty());
}

#[test]
fn accept_parity5_cabin_intro_05_return_to_main_menu_closes_the_profile() {
    let (mut flow, mut shell) = cabin();
    let before = flow.requests().len();
    let step = press(&mut flow, &mut shell, 170);
    assert_eq!(
        step,
        StartStep::Moved {
            from: Screen::Cabin,
            to: Screen::MainMenu
        }
    );
    assert_eq!(flow.screen(), Screen::MainMenu);
    assert_eq!(shell.section_name(), Some("MainMenu"));
    assert!(flow.requests().len() > before, "the profile is released");
    assert!(flow.notice().is_none());
}

#[test]
fn accept_parity5_cabin_intro_05_the_unbuilt_buttons_open_a_marked_screen() {
    // Every cabin button now has a screen; any entry that returns is
    // exercised here, one row per entry.
    let rows: [i32; 0] = [];
    for (entry, y) in NOT_IMPLEMENTED.iter().zip(rows) {
        let (mut flow, mut shell) = cabin();
        let step = press(&mut flow, &mut shell, y);
        assert_eq!(step, StartStep::NotImplemented(*entry));
        // The machine stays on the cabin, the shell is back on it, and the
        // notice is up and recorded.
        assert_eq!(flow.screen(), Screen::Cabin);
        assert_eq!(shell.section_name(), Some("PassengerCabin"));
        assert_eq!(flow.notice(), Some(entry));
        assert_eq!(flow.not_implemented_seen(), [*entry]);
        assert!(flow.handover().is_none());
        assert!(flow.dismiss_notice());
        assert!(flow.notice().is_none());
        // Dismissed: the cabin works again.
        assert!(matches!(
            press(&mut flow, &mut shell, 10),
            StartStep::Launch { .. }
        ));
    }
    let names: Vec<&str> = NOT_IMPLEMENTED.iter().map(|entry| entry.what).collect();
    assert!(names.is_empty(), "{names:?}");
}

#[test]
fn accept_parity5_cabin_intro_05_previous_missions_opens_its_screen_not_a_notice() {
    let (mut flow, mut shell) = cabin();
    let step = press(&mut flow, &mut shell, 50);
    assert_eq!(
        step,
        StartStep::Moved {
            from: Screen::Cabin,
            to: Screen::Scrapbook
        }
    );
    assert_eq!(shell.section_name(), Some("ScrapBook_TOC"));
    assert!(flow.notice().is_none());
    assert!(flow.not_implemented_seen().is_empty());
}
