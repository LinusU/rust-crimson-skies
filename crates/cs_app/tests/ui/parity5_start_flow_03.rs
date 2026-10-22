//! Task #1831 `PARITY5-START-FLOW.03`: from a mission back to the cabin and
//! replay. Task test prefix: `accept_parity5_start_flow_03_`.
//!
//! The command-line half and the machine walk run the production parser and
//! [`StartFlow`]; the retail half drives the real `PassengerCabin` shell
//! section from `$CS_GAME_DIR` (fails loudly without it, ignored in CI). The
//! mission-side half is `tests/campaign/parity5_start_flow_03.rs`.

use std::path::PathBuf;

use cs_app::cli::{self, CliRequest};
use cs_app::ui::front_end::{
    Action, FIRST_MISSION, FrontEndScreens, MenuShell, MissionEnd, Presentation, Request, Resource,
    Screen, StartFlow, StartFlowRequest, StartStep, handover_args, presentation, return_args,
    script_member,
};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| (*arg).to_owned()).collect()
}

/// The hand-over asks the mission to come back; the return command is the
/// start flow resumed.
#[test]
fn accept_parity5_start_flow_03_handover_and_return_commands_round_trip() {
    let request = StartFlowRequest {
        cs_path: PathBuf::from("/retail"),
        resume: None,
    };
    match cli::parse(handover_args(&request, FIRST_MISSION)) {
        CliRequest::Mission(mission) => assert!(mission.from_start_flow),
        other => panic!("the hand-over parses to {other:?}"),
    }
    // A plain `--mission` run is not from the start flow.
    match cli::parse(args(&["--cs-path", "/retail", "--mission", "M01"])) {
        CliRequest::Mission(mission) => assert!(!mission.from_start_flow),
        other => panic!("{other:?}"),
    }
    for end in [MissionEnd::ReturnedToCabin, MissionEnd::Abandoned] {
        assert_eq!(
            cli::parse(return_args(&PathBuf::from("/retail"), end)),
            CliRequest::StartFlow(StartFlowRequest {
                cs_path: PathBuf::from("/retail"),
                resume: Some(end),
            })
        );
    }
    assert!(matches!(
        cli::parse(args(&[
            "--cs-path",
            "/retail",
            "--resume-start-flow",
            "nowhere"
        ])),
        CliRequest::Invalid { .. }
    ));
    assert!(matches!(
        cli::parse(args(&["--from-start-flow"])),
        CliRequest::Invalid { .. }
    ));
    assert!(matches!(
        cli::parse(args(&[
            "--cs-path",
            "/retail",
            "--mission",
            "M01",
            "--resume-start-flow",
            "cabin"
        ])),
        CliRequest::Invalid { .. }
    ));
}

/// Both ways out of a flight end on the cabin, with the world released and
/// the outcome / abandon transactions recorded.
#[test]
fn accept_parity5_start_flow_03_leaving_a_flight_lands_on_the_cabin() {
    let failed = StartFlow::after_mission(MissionEnd::ReturnedToCabin).expect("the table's walk");
    assert_eq!(failed.screen(), Screen::Cabin);
    assert_eq!(failed.section(), Some("PassengerCabin"));
    assert!(
        !failed.front_end().held().contains(&Resource::World),
        "the flight's world is released"
    );
    assert!(
        failed
            .requests()
            .iter()
            .any(|request| matches!(request, Request::ApplyOutcome(_)))
    );

    let abandoned = StartFlow::after_mission(MissionEnd::Abandoned).expect("the table's walk");
    assert_eq!(abandoned.screen(), Screen::Cabin);
    assert!(!abandoned.front_end().held().contains(&Resource::World));
    assert!(
        !abandoned
            .requests()
            .iter()
            .any(|request| matches!(request, Request::ApplyOutcome(_))),
        "an abandoned flight settles no outcome"
    );
    assert!(matches!(presentation(Screen::Cabin), Presentation::Shell));
}

/// From the cabin the player can fly again (Next Mission → the mission
/// hand-over, a fresh world) or go to the main menu.
#[test]
fn accept_parity5_start_flow_03_cabin_replays_or_returns_to_the_menu() {
    let mut flow = StartFlow::after_mission(MissionEnd::ReturnedToCabin).unwrap();
    match flow.press_button("PC_B_NEWMISSION") {
        StartStep::Launch { mission } => assert_eq!(mission, FIRST_MISSION),
        other => panic!("Next Mission hands the mission over: {other:?}"),
    }
    assert_eq!(flow.handover(), Some(FIRST_MISSION));

    let mut flow = StartFlow::after_mission(MissionEnd::Abandoned).unwrap();
    assert_eq!(
        flow.enter_section("MainMenu"),
        StartStep::Moved {
            from: Screen::Cabin,
            to: Screen::MainMenu
        }
    );
    assert!(
        flow.requests().contains(&Request::CloseProfile),
        "leaving the cabin closes the pilot"
    );
    // Esc is not bound on the cabin (finding 2026-10-22).
    let mut flow = StartFlow::after_mission(MissionEnd::Abandoned).unwrap();
    assert_eq!(flow.escape(), StartStep::Stay);
    assert_eq!(flow.screen(), Screen::Cabin);
    let _ = Action::Back;
}

/// The retail `PassengerCabin` section is what the window opens on.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_start_flow_03_retail_cabin_section_is_shown_and_navigates() {
    let dir = PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    );
    let screens = FrontEndScreens::open(&dir).expect("the installation opens");
    let layout = screens.original_layout().expect("the layout reads");
    let mut shell = MenuShell::build(
        &layout,
        &|art: &str| screens.art_extent(art),
        &|section: &str| screens.script_text(&script_member(section)),
    );
    let mut flow = StartFlow::after_mission(MissionEnd::ReturnedToCabin).unwrap();
    shell
        .open(flow.section().expect("the cabin is a shell section"))
        .expect("the PassengerCabin section exists");
    assert_eq!(shell.section_name(), Some("PassengerCabin"));

    let rect = shell
        .section()
        .unwrap()
        .buttons
        .iter()
        .find(|button| button.id.eq_ignore_ascii_case("PC_B_RETURNMM"))
        .expect("Return to Main Menu is drawn")
        .rect;
    shell.pointer_move(rect.x + 1, rect.y + 1);
    shell.pointer_down();
    let outcome = shell.pointer_up();
    let step = flow.route(&mut shell, outcome);
    assert_eq!(
        step,
        StartStep::Moved {
            from: Screen::Cabin,
            to: Screen::MainMenu
        }
    );
    assert_eq!(shell.section_name(), Some("MainMenu"));
}
