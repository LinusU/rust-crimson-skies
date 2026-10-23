//! Task #1830 `PARITY5-START-FLOW.02`: bare `cs --cs-path <retail>` opens the
//! front end driven by the F45 machine. Task test prefix:
//! `accept_parity5_start_flow_02_`.
//!
//! The tests parse the real command line, walk the real [`StartFlow`] machine
//! (over the retail `LAYOUT.CSV` shell for the retail ones) and check the
//! transitions against
//! `docs/findings/2026-10-22-parity5-start-flow-transitions.md`. Retail tests
//! read `$CS_GAME_DIR`, fail loudly without it and are ignored in CI.

use std::path::PathBuf;

use cs_app::cli::{self, CliRequest};
use cs_app::ui::front_end::{
    FIRST_MISSION, FrontEndScreens, MenuShell, Presentation, Screen, ShellOutcome, StartFlow,
    StartFlowRequest, StartRefusal, StartStep, handover_args, presentation, script_member,
};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| (*arg).to_owned()).collect()
}

fn retail_shell() -> MenuShell {
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
    shell.open("MainMenu").expect("the main menu section");
    shell
}

/// Presses the button `id` of the shell's current section with the pointer and
/// returns what the flow made of it.
fn press(flow: &mut StartFlow, shell: &mut MenuShell, id: &str) -> StartStep {
    let (rect, _) = {
        let section = shell.section().expect("a section is open");
        let button = section
            .buttons
            .iter()
            .find(|button| button.id.eq_ignore_ascii_case(id))
            .unwrap_or_else(|| panic!("{id} is a button of {}", section.name));
        (button.rect, ())
    };
    shell.pointer_move(rect.x + 1, rect.y + 1);
    shell.pointer_down();
    let outcome = shell.pointer_up();
    flow.route(shell, outcome)
}

/// **The command line.** Bare `--cs-path <dir>` is the start flow; every other
/// shape keeps its meaning.
#[test]
fn accept_parity5_start_flow_02_bare_cs_path_is_the_start_flow() {
    assert_eq!(
        cli::parse(args(&["--cs-path", "/retail"])),
        CliRequest::StartFlow(StartFlowRequest {
            cs_path: PathBuf::from("/retail"),
            resume: None,
            smoke: None,
        })
    );
    // `--mission` is untouched: it still parses to the mission request.
    assert!(matches!(
        cli::parse(args(&["--cs-path", "/retail", "--mission", "M01"])),
        CliRequest::Mission(_)
    ));
    assert!(matches!(
        cli::parse(args(&["--cs-path", "/retail", "--front-end"])),
        CliRequest::FrontEnd(_)
    ));
    assert!(matches!(
        cli::parse(args(&["--playtest", "--cs-path", "/retail"])),
        CliRequest::Playtest(_)
    ));
    // Flags that belong to other modes do not turn into a start flow.
    for extra in [
        &["--smoke-seconds", "9"][..],
        &["--world", "c1c"],
        &["--dev-overlay"],
        &["--headless"],
    ] {
        let mut line = args(&["--cs-path", "/retail"]);
        line.extend(extra.iter().map(|arg| (*arg).to_owned()));
        assert!(
            !matches!(cli::parse(line.clone()), CliRequest::StartFlow(_)),
            "{line:?} must not be the start flow"
        );
    }
}

/// **The hand-over is the `--mission` command.** What the flow runs on
/// reaching the briefing parses to the existing mission request.
#[test]
fn accept_parity5_start_flow_02_handover_is_the_mission_command() {
    let request = StartFlowRequest {
        cs_path: PathBuf::from("/retail"),
        resume: None,
        smoke: None,
    };
    match cli::parse(handover_args(&request, FIRST_MISSION)) {
        CliRequest::Mission(mission) => {
            assert_eq!(mission.cs_path, PathBuf::from("/retail"));
            assert_eq!(mission.mission, "M01");
            assert!(mission.smoke.is_none());
        }
        other => panic!("the hand-over parses to {other:?}"),
    }
}

/// **The machine walk to the mission.** With the sibling screens assumed
/// presented, the table takes the main menu through the Campaign page and the
/// cabin to the briefing, where the hand-over is made; the profile request is
/// recorded, not performed.
#[test]
fn accept_parity5_start_flow_02_machine_walk_reaches_the_mission_launch() {
    use cs_app::ui::front_end::{Action, Request};
    let mut flow = StartFlow::new().assuming_siblings_landed();
    assert_eq!(flow.screen(), Screen::MainMenu);
    assert_eq!(
        flow.enter_section("Campaign"),
        StartStep::Moved {
            from: Screen::MainMenu,
            to: Screen::ProfileSelect
        }
    );
    // The pilot page's choice, then Continue.
    flow.press_machine(Action::NewProfile);
    flow.press_machine(Action::ConfirmProfile);
    assert_eq!(flow.screen(), Screen::Cabin);
    assert!(matches!(flow.requests(), [Request::OpenProfile(_)]));
    // Next Mission opens the briefing, which the mission session presents.
    assert_eq!(
        flow.press_button("PC_B_NEWMISSION"),
        StartStep::Launch {
            mission: "M01".to_owned()
        }
    );
    assert_eq!(flow.handover(), Some("M01"));
}

/// **Esc.** Unbound on the main menu (never quits), bound on Credits.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_start_flow_02_esc_is_bound_only_where_the_original_binds_it() {
    let mut shell = retail_shell();
    let mut flow = StartFlow::new();
    assert_eq!(flow.route_escape(&mut shell), StartStep::Stay);
    assert_eq!(flow.screen(), Screen::MainMenu);
    assert_eq!(shell.section_name(), Some("MainMenu"));

    let step = press(&mut flow, &mut shell, "MM_B_CREDITS");
    assert_eq!(
        step,
        StartStep::Moved {
            from: Screen::MainMenu,
            to: Screen::Credits
        }
    );
    assert_eq!(
        shell.section_name().map(str::to_ascii_lowercase).as_deref(),
        Some("credits")
    );
    assert_eq!(
        flow.route_escape(&mut shell),
        StartStep::Moved {
            from: Screen::Credits,
            to: Screen::MainMenu
        }
    );
    assert_eq!(shell.section_name(), Some("MainMenu"));
}

/// **Preferences, Cancel, Multiplayer and Quit** come from the buttons and
/// the table; Campaign reaches its shell-drawn section (the pilot page's
/// interactive controls stay #1835's), and Multiplayer is refused by name
/// while the lobby's presenter is unbuilt.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_start_flow_02_buttons_follow_the_table_and_refuse_unbuilt_screens() {
    let mut shell = retail_shell();
    let mut flow = StartFlow::new();

    // Campaign: the shell draws the `Campaign` layout section as the pilot
    // page (FE-MENU-2); the controls #1835 owns stay drawn-but-recorded.
    let step = press(&mut flow, &mut shell, "MM_B_CAMPAIGN");
    assert_eq!(
        step,
        StartStep::Moved {
            from: Screen::MainMenu,
            to: Screen::ProfileSelect
        }
    );
    assert_eq!(flow.screen(), Screen::ProfileSelect);
    assert_eq!(shell.section_name(), Some("Campaign"));
    assert!(matches!(
        presentation(Screen::ProfileSelect),
        Presentation::Shell
    ));
    // Its Cancel hands back to MainMenu.
    let step = press(&mut flow, &mut shell, "CM_B_CANCEL");
    assert_eq!(
        step,
        StartStep::Moved {
            from: Screen::ProfileSelect,
            to: Screen::MainMenu
        }
    );
    assert_eq!(flow.screen(), Screen::MainMenu);
    assert_eq!(shell.section_name(), Some("MainMenu"));

    // Multiplayer: the layout holds no `MultiPlayerMain` section, so the
    // press stands for the table's `OpenLobby` — refused by name while the
    // lobby presenter is unbuilt (never a guessed screen).
    let step = press(&mut flow, &mut shell, "MM_B_MULTIPLAYER");
    assert_eq!(
        step,
        StartStep::Refused(StartRefusal::NotYetPresented {
            screen: Screen::Lobby,
            what: "Multiplayer",
            task: None,
        })
    );
    assert_eq!(flow.screen(), Screen::MainMenu, "the machine did not move");
    assert_eq!(shell.section_name(), Some("MainMenu"));

    // Preferences and back through its main-menu button.
    let step = press(&mut flow, &mut shell, "MM_B_PREFERENCES");
    assert_eq!(
        step,
        StartStep::Moved {
            from: Screen::MainMenu,
            to: Screen::Settings
        }
    );
    let step = press(&mut flow, &mut shell, "PF_B_MAINMENU");
    assert_eq!(
        step,
        StartStep::Moved {
            from: Screen::Settings,
            to: Screen::MainMenu
        }
    );
    assert_eq!(shell.section_name(), Some("MainMenu"));

    // Quit ends, with no confirmation.
    assert_eq!(press(&mut flow, &mut shell, "MM_B_QUIT"), StartStep::Quit);
    assert!(matches!(
        flow.route(&mut shell, ShellOutcome::Nothing),
        StartStep::Stay
    ));
}
