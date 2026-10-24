//! Rally #2047: the owner-command smoke's convoy-kill stage
//! (`--smoke-convoy-kill`). Task test prefix: `accept_convoy_kill_`.
//!
//! The judging is pure, so these tests drive the same `evaluate_mission_smoke`
//! the windowed run ends with. The windowed half is the retail run recorded in
//! `docs/findings/2026-10-24-convoy-kill-owner-command-smoke.md`.

use cs_app::cli::{self, CliRequest};
use cs_app::mission_session::{
    Check, ConvoyKillMetrics, MissionSmokeMode, SmokeMeasurements, evaluate_mission_smoke,
};
use cs_sim::damage::ActorId;

const TRUCKS: [&str; 4] = ["t_truck01", "t_truck02", "t_truck03", "bridge_truck01"];

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| (*arg).to_owned()).collect()
}

fn check<'a>(checks: &'a [Check], id: &str) -> &'a Check {
    checks
        .iter()
        .find(|check| check.id == id)
        .unwrap_or_else(|| panic!("no check named {id}: {checks:#?}"))
}

fn rows(in_play: bool) -> Vec<(String, Option<bool>)> {
    TRUCKS
        .iter()
        .map(|node| ((*node).to_owned(), Some(in_play)))
        .collect()
}

fn full() -> ConvoyKillMetrics {
    ConvoyKillMetrics {
        start_frame: Some(2_600),
        actors: TRUCKS
            .iter()
            .enumerate()
            .map(|(serial, node)| {
                let actor = ActorId {
                    session: cs_types::net::SessionId::new(1).expect("nonzero"),
                    serial: serial as u64,
                };
                ((*node).to_owned(), actor)
            })
            .collect(),
        centres: TRUCKS
            .iter()
            .map(|node| ((*node).to_owned(), [0.0, 0.0, 0.0]))
            .collect(),
        in_play_before: rows(true),
        fire_presses: 400,
        destroyed: TRUCKS
            .iter()
            .enumerate()
            .map(|(index, node)| ((*node).to_owned(), 2_700 + index as u32 * 100))
            .collect(),
        in_play_after: rows(false),
        last_destroyed_frame: Some(3_000),
        placements: 90,
        frames_delivered: true,
        intact_permille: 900,
        destroyed_permille: 900,
        gave_up_on: None,
    }
}

fn measured(convoy_kill: ConvoyKillMetrics) -> SmokeMeasurements {
    SmokeMeasurements {
        convoy_kill: Some(convoy_kill),
        ..SmokeMeasurements::default()
    }
}

#[test]
fn accept_convoy_kill_flag_parses_to_the_mode() {
    let line = |extra: &[&str]| {
        let mut line = args(&[
            "--cs-path",
            "/retail",
            "--mission",
            "M01",
            "--smoke-seconds",
            "60",
            "--smoke-convoy-kill",
        ]);
        line.extend(extra.iter().map(|arg| (*arg).to_owned()));
        line
    };
    match cli::parse(line(&["--capture-dir", "/out"])) {
        CliRequest::Mission(mission) => {
            assert!(mission.smoke_convoy_kill);
            assert!(!mission.smoke_trucks);
            assert!(mission.smoke.is_some());
        }
        other => panic!("{other:?}"),
    }
    let _ = MissionSmokeMode::ConvoyKill;
    for other in [
        "--smoke-no-teleport",
        "--smoke-engagement",
        "--smoke-failure",
        "--smoke-crash",
        "--smoke-start-flow",
    ] {
        assert!(
            matches!(cli::parse(line(&[other])), CliRequest::Invalid { .. }),
            "--smoke-convoy-kill combines with no other mode, not {other}"
        );
    }
}

#[test]
fn accept_convoy_kill_empty_stage_fails_every_check() {
    let checks = evaluate_mission_smoke(&measured(ConvoyKillMetrics::default()));
    for id in [
        "convoy_destroyed_by_fire_key",
        "convoy_observation_follows",
        "convoy_frames_drawn",
    ] {
        assert!(
            !check(&checks, id).passed,
            "{id} must fail on the empty stage"
        );
    }
}

#[test]
fn accept_convoy_kill_whole_sequence_passes() {
    let checks = evaluate_mission_smoke(&measured(full()));
    for check in &checks {
        assert!(check.passed, "{} failed: {}", check.id, check.detail);
    }
}

#[test]
fn accept_convoy_kill_each_missing_piece_fails_its_own_check() {
    type Mutate = fn(&mut ConvoyKillMetrics);
    let cases: [(&str, Mutate); 7] = [
        ("convoy_destroyed_by_fire_key", |c| {
            c.destroyed.pop();
        }),
        ("convoy_destroyed_by_fire_key", |c| c.fire_presses = 0),
        ("convoy_destroyed_by_fire_key", |c| {
            c.gave_up_on = Some("t_truck02".to_owned())
        }),
        ("convoy_observation_follows", |c| {
            c.in_play_after[1].1 = Some(true)
        }),
        ("convoy_observation_follows", |c| {
            c.in_play_before[0].1 = Some(false)
        }),
        ("convoy_frames_drawn", |c| c.destroyed_permille = 10),
        ("convoy_frames_drawn", |c| c.frames_delivered = false),
    ];
    for (id, mutate) in cases {
        let mut convoy = full();
        mutate(&mut convoy);
        let checks = evaluate_mission_smoke(&measured(convoy));
        assert!(!check(&checks, id).passed, "{id} must fail");
    }
}
