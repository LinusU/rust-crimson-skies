//! PARITY4-CRASH-E (#1857): the owner-command smoke's crash phase
//! (`--smoke-crash`). Task test prefix: `accept_crash_e_`.
//!
//! The judging is pure, so these tests drive the same `evaluate_mission_smoke`
//! the windowed run ends with: an empty crash stage fails every check naming
//! what it lacks, a stage that saw the whole sequence passes, and each missing
//! piece of the sequence fails its own check. The windowed half is the
//! retail run recorded in `docs/findings/2026-10-23-crash-e-owner-command-smoke.md`.

use cs_app::cli::{self, CliRequest};
use cs_app::mission_session::{
    Check, CrashMetrics, MissionSmokeMode, SmokeMeasurements, evaluate_mission_smoke,
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

fn full() -> CrashMetrics {
    CrashMetrics {
        dive_frame: Some(2_471),
        dive_from_m: Some([-1426.0, 400.0, -1813.0]),
        contacts_judged: 2,
        track: vec![[2520.0, -1495.0, 357.0, -1895.0]],
        crash_frame: Some(2_900),
        crash_tick: Some(5_800),
        crash_position_m: Some([-100.0, 1.0, -200.0]),
        severity: Some(0.7),
        max_sparks: 6,
        max_fire_puffs: 12,
        frames_delivered: true,
        contact_permille: 900,
        camera_permille: 950,
        camera_error_m: Some(0.01),
        message_text: Some("Fatal Crash!".to_owned()),
        message_visible: true,
        key_frame: Some(2_990),
        released: true,
        settled_outcome: Some("failure".to_owned()),
        settled_source: Some("player_destroyed".to_owned()),
        screen_frame: Some(3_000),
        exits_seen: 0,
        failed_frame_delivered: true,
        failed_permille: 800,
    }
}

fn measured(crash: CrashMetrics) -> SmokeMeasurements {
    SmokeMeasurements {
        crash: Some(crash),
        ..SmokeMeasurements::default()
    }
}

/// `--smoke-crash` parses to its own mission smoke mode and combines with no
/// other mode.
#[test]
fn accept_crash_e_flag_parses_to_the_mode() {
    let line = |extra: &[&str]| {
        let mut line = args(&[
            "--cs-path",
            "/retail",
            "--mission",
            "M01",
            "--smoke-seconds",
            "60",
            "--smoke-crash",
        ]);
        line.extend(extra.iter().map(|arg| (*arg).to_owned()));
        line
    };
    match cli::parse(line(&["--capture-dir", "/out"])) {
        CliRequest::Mission(mission) => {
            assert!(mission.smoke_crash);
            assert!(mission.smoke.is_some());
        }
        other => panic!("{other:?}"),
    }
    let _ = MissionSmokeMode::Crash;
    for other in [
        "--smoke-no-teleport",
        "--smoke-engagement",
        "--smoke-failure",
        "--smoke-start-flow",
    ] {
        assert!(
            matches!(cli::parse(line(&[other])), CliRequest::Invalid { .. }),
            "--smoke-crash combines with no other mode, not {other}"
        );
    }
    // Without a mission or a smoke it is not a mode of anything.
    assert!(matches!(
        cli::parse(args(&["--cs-path", "/retail", "--smoke-crash"])),
        CliRequest::Invalid { .. } | CliRequest::Unsupported { .. }
    ));
}

/// A run that never reached the crash stage, or whose dive never ended the
/// flight, fails the checks naming what is missing.
#[test]
fn accept_crash_e_empty_stage_fails_every_check() {
    let checks = evaluate_mission_smoke(&measured(CrashMetrics::default()));
    for id in [
        "crash_flight_ended",
        "crash_frames_drawn",
        "crash_contact_effects",
        "crash_camera_detached",
        "crash_message_shown",
        "crash_key_hands_over",
        "crash_failed_screen_shown",
    ] {
        assert!(
            !check(&checks, id).passed,
            "{id} must fail on the empty stage"
        );
    }
    let checks = evaluate_mission_smoke(&SmokeMeasurements {
        crash: None,
        ..SmokeMeasurements::default()
    });
    assert!(
        checks.iter().all(|check| !check.id.starts_with("crash_")),
        "a run without a crash stage is judged as the scripted run"
    );
}

/// A stage that saw the whole sequence passes every check.
#[test]
fn accept_crash_e_whole_sequence_passes() {
    let checks = evaluate_mission_smoke(&measured(full()));
    for check in &checks {
        assert!(check.passed, "{} failed: {}", check.id, check.detail);
    }
    assert!(
        checks
            .iter()
            .any(|check| check.id == "crash_failed_screen_shown")
    );
}

/// Each piece of the sequence the run lacks fails its own check.
#[test]
fn accept_crash_e_each_missing_piece_fails_its_own_check() {
    type Mutate = fn(&mut CrashMetrics);
    let cases: [(&str, Mutate); 7] = [
        ("crash_flight_ended", |c| c.crash_frame = None),
        ("crash_frames_drawn", |c| c.contact_permille = 10),
        ("crash_contact_effects", |c| c.max_sparks = 0),
        ("crash_contact_effects", |c| c.max_fire_puffs = 0),
        ("crash_camera_detached", |c| c.camera_error_m = Some(40.0)),
        ("crash_message_shown", |c| {
            c.message_text = Some("MSG_CRASH".to_owned())
        }),
        ("crash_key_hands_over", |c| c.released = false),
    ];
    for (id, mutate) in cases {
        let mut crash = full();
        mutate(&mut crash);
        let checks = evaluate_mission_smoke(&measured(crash));
        assert!(!check(&checks, id).passed, "{id} must fail");
    }
    let mut crash = full();
    crash.screen_frame = None;
    let checks = evaluate_mission_smoke(&measured(crash));
    assert!(!check(&checks, "crash_failed_screen_shown").passed);
    let mut crash = full();
    crash.exits_seen = 1;
    let checks = evaluate_mission_smoke(&measured(crash));
    assert!(
        !check(&checks, "crash_failed_screen_shown").passed,
        "an AppExit between the crash and the screen fails the check"
    );
    let mut m = measured(full());
    m.missing_shots = vec!["crash_camera".to_owned()];
    let checks = evaluate_mission_smoke(&m);
    assert!(!check(&checks, "smoke_shots_delivered").passed);
}
