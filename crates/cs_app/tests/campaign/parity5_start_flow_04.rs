//! Task #1832 `PARITY5-START-FLOW.04`, mission side: the
//! `--smoke-start-flow` mission leg's own checks — the short flight's pause
//! map, its Quit and how the flight reported ending. Task test prefix:
//! `accept_parity5_start_flow_04_`. The command-line half is
//! `tests/ui/parity5_start_flow_04.rs`.

use cs_app::cli::{self, CliRequest};
use cs_app::mission_session::{
    Check, CinematicMetrics, MissionSmokeMode, SmokeMeasurements, StartFlowMetrics,
    evaluate_mission_smoke,
};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| (*arg).to_owned()).collect()
}

fn check<'a>(checks: &'a [Check], id: &str) -> &'a Check {
    checks
        .iter()
        .find(|check| check.id == id)
        .unwrap_or_else(|| panic!("no check named {id}: {checks:#?}"))
}

/// `--smoke-start-flow` parses to its own mission smoke mode — the flag the
/// chain's hand-overs carry.
#[test]
fn accept_parity5_start_flow_04_mission_flag_parses_to_the_mode() {
    match cli::parse(args(&[
        "--cs-path",
        "/retail",
        "--mission",
        "M01",
        "--smoke-seconds",
        "60",
        "--smoke-start-flow",
        "--capture-dir",
        "/out/mission",
    ])) {
        CliRequest::Mission(mission) => {
            assert!(mission.smoke_start_flow);
            assert!(mission.smoke.is_some());
        }
        other => panic!("{other:?}"),
    }
    // The mode exists alongside the others; a leg is never one of them.
    let _ = MissionSmokeMode::StartFlow;
    for pair in [
        &["--smoke-no-teleport"][..],
        &["--smoke-engagement"][..],
        &["--smoke-failure"][..],
    ] {
        let mut line = args(&[
            "--cs-path",
            "/retail",
            "--mission",
            "M01",
            "--smoke-seconds",
            "60",
            "--smoke-start-flow",
        ]);
        line.extend(pair.iter().map(|arg| (*arg).to_owned()));
        assert!(
            matches!(cli::parse(line.clone()), CliRequest::Invalid { .. }),
            "{line:?}: --smoke-start-flow combines with no other mode"
        );
    }
}

/// The leg's report names its screens: a pause map that never drew and a
/// Quit the click path never took are failed checks naming the screen, and
/// a `mission` end the handed-over leg never reported is named `mission`.
#[test]
fn accept_parity5_start_flow_04_missing_screens_fail_naming_them() {
    let mut m = SmokeMeasurements {
        start_flow: Some(StartFlowMetrics {
            return_to_front_end: true,
            ..StartFlowMetrics::default()
        }),
        ..SmokeMeasurements::default()
    };
    let checks = evaluate_mission_smoke(&m);
    for (id, name) in [
        ("briefing_screen_drawn", "briefing"),
        ("start_flow_pause_map_opened", "pause map"),
        ("start_flow_quit_clicked", "Quit"),
        ("start_flow_mission_left", "reported its end"),
        ("launch_cinematic_plays", "launch cinematic"),
        ("first_control_frame", "control"),
    ] {
        let check = check(&checks, id);
        assert!(!check.passed, "{id} must fail on the empty leg");
        assert!(
            check.detail.contains(name),
            "{id}'s failure must name the screen — {:?} lacks {name:?}",
            check.detail
        );
    }
    // A leg whose screens all drew passes them.
    m.cinematic = Some(CinematicMetrics {
        handed_over: true,
        played_ticks: 10,
        total_ticks: 10,
        bars_on_while_playing: true,
        bars_off_after: true,
        ..CinematicMetrics::default()
    });
    m.start_flow = Some(StartFlowMetrics {
        return_to_front_end: true,
        map_shown: true,
        quit_clicked: true,
        mission_left: Some("abandoned".to_owned()),
    });
    let checks = evaluate_mission_smoke(&m);
    assert!(check(&checks, "start_flow_pause_map_opened").passed);
    assert!(check(&checks, "start_flow_quit_clicked").passed);
    assert!(check(&checks, "start_flow_mission_left").passed);
    // The replay leg — never handed over — does not ask for a mission end:
    // the check does not apply, and a check that does not apply cannot fail
    // the report.
    m.start_flow = Some(StartFlowMetrics {
        return_to_front_end: false,
        map_shown: true,
        quit_clicked: true,
        mission_left: None,
    });
    let checks = evaluate_mission_smoke(&m);
    let left = check(&checks, "start_flow_mission_left");
    assert!(
        !left.applies,
        "the replay leg asks for no mission end: {left:?}"
    );
    m.start_flow = Some(StartFlowMetrics {
        return_to_front_end: true,
        map_shown: true,
        quit_clicked: true,
        mission_left: Some("cabin".to_owned()),
    });
    let checks = evaluate_mission_smoke(&m);
    let left = check(&checks, "start_flow_mission_left");
    assert!(
        !left.passed,
        "a cabin end on the quit-the-flight leg must fail: {left:?}"
    );
}
