//! Rally #2081 (`M01-SMOKE-SUCCESS-A`): the owner-command smoke's success
//! stage, part A (`--smoke-success`): the `shipwreck` proximity beat
//! (`OBJECTIVE12`, `OBJECTIVE27`). Task test prefix: `accept_m01_smoke_success_a_`.
//!
//! The judging is pure, so these tests drive the same `evaluate_mission_smoke`
//! the windowed run ends with. The windowed half is the retail run recorded in
//! `docs/findings/2026-10-24-m01-smoke-success-a.md`.

use cs_app::cli::{self, CliRequest};
use cs_app::mission_session::{
    Check, MissionSmokeMode, SmokeMeasurements, SuccessMetrics, evaluate_mission_smoke,
};
use cs_sim::damage::ActorId;
use cs_types::net::SessionId;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| (*arg).to_owned()).collect()
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: SessionId::new(1).expect("a nonzero session generation"),
        serial,
    }
}

fn check<'a>(checks: &'a [Check], id: &str) -> &'a Check {
    checks
        .iter()
        .find(|check| check.id == id)
        .unwrap_or_else(|| panic!("no check named {id}: {checks:#?}"))
}

fn full() -> SuccessMetrics {
    SuccessMetrics {
        start_frame: Some(2_600),
        centre: Some([0.0, 500.0, 0.0]),
        placements: 90,
        completed: vec![(11, 2_610), (26, 2_740), (20, 3_000), (22, 3_700)],
        frames_delivered: true,
        near_permille: 900,
        marked_permille: 900,
        marked_shot_frame: Some(2_740),
        landing_placements: 40,
        kestrel_actors: vec![("medkestrel_1".to_owned(), actor(1))],
        fire_presses: 100,
        kestrel_placements: 50,
        kestrels_destroyed: vec![("medkestrel_1".to_owned(), 3_900)],
        landed_permille: 900,
        kestrels_permille: 900,
        ..SuccessMetrics::default()
    }
}

fn measured(success: SuccessMetrics) -> SmokeMeasurements {
    SmokeMeasurements {
        success: Some(success),
        ..SmokeMeasurements::default()
    }
}

#[test]
fn accept_m01_smoke_success_a_flag_parses_to_the_mode() {
    let line = |extra: &[&str]| {
        let mut line = args(&[
            "--cs-path",
            "/retail",
            "--mission",
            "M01",
            "--smoke-seconds",
            "60",
            "--smoke-success",
        ]);
        line.extend(extra.iter().map(|arg| (*arg).to_owned()));
        line
    };
    match cli::parse(line(&["--capture-dir", "/out"])) {
        CliRequest::Mission(mission) => {
            assert!(mission.smoke_success);
            assert!(!mission.smoke_convoy_kill);
            assert!(mission.smoke.is_some());
        }
        other => panic!("{other:?}"),
    }
    let _ = MissionSmokeMode::Success;
    for other in [
        "--smoke-no-teleport",
        "--smoke-engagement",
        "--smoke-failure",
        "--smoke-crash",
        "--smoke-trucks",
        "--smoke-convoy-kill",
        "--smoke-start-flow",
    ] {
        assert!(
            matches!(cli::parse(line(&[other])), CliRequest::Invalid { .. }),
            "--smoke-success combines with no other mode, not {other}"
        );
    }
}

#[test]
fn accept_m01_smoke_success_a_empty_stage_fails_every_check() {
    let checks = evaluate_mission_smoke(&measured(SuccessMetrics::default()));
    for id in [
        "success_proximity_block_completed",
        "success_mark_block_completed",
        "success_frames_drawn",
    ] {
        assert!(
            !check(&checks, id).passed,
            "{id} must fail on the empty stage"
        );
    }
}

#[test]
fn accept_m01_smoke_success_a_whole_sequence_passes() {
    // The later beats are checks of the same stage: the whole sequence is the
    // complete run, which part C's fixture holds.
    let checks = evaluate_mission_smoke(&measured(super::accept_m01_smoke_success_c::full()));
    for check in &checks {
        assert!(check.passed, "{} failed: {}", check.id, check.detail);
    }
}

#[test]
fn accept_m01_smoke_success_a_each_missing_piece_fails_its_own_check() {
    type Mutate = fn(&mut SuccessMetrics);
    let cases: [(&str, Mutate); 5] = [
        ("success_proximity_block_completed", |c| {
            c.completed.retain(|(block, _)| *block != 11)
        }),
        ("success_proximity_block_completed", |c| c.placements = 0),
        ("success_mark_block_completed", |c| {
            c.completed.retain(|(block, _)| *block != 26)
        }),
        ("success_frames_drawn", |c| c.marked_permille = 10),
        ("success_frames_drawn", |c| c.frames_delivered = false),
    ];
    for (id, mutate) in cases {
        let mut metrics = full();
        mutate(&mut metrics);
        let checks = evaluate_mission_smoke(&measured(metrics));
        assert!(
            !check(&checks, id).passed,
            "{id} must fail when a piece is missing"
        );
    }
}
