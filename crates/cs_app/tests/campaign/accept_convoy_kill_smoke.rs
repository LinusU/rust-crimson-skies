//! Rally #2047: the owner-command smoke's convoy-kill stage
//! (`--smoke-convoy-kill`). Task test prefix: `accept_convoy_kill_`.
//!
//! The judging is pure, so these tests drive the same `evaluate_mission_smoke`
//! the windowed run ends with. The windowed half is the retail run recorded in
//! `docs/findings/2026-10-24-convoy-kill-owner-command-smoke.md`.

use cs_app::cli::{self, CliRequest};
use cs_app::mission_session::{
    Check, ConvoyKillMetrics, ConvoyLayoutRow, MissionSmokeMode, SmokeMeasurements,
    evaluate_mission_smoke,
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

/// Every truck's drawn layout before the first shot: the intact side drawn,
/// the `destroyed` one hidden.
fn layout_before() -> Vec<ConvoyLayoutRow> {
    TRUCKS
        .iter()
        .map(|node| ConvoyLayoutRow {
            node: (*node).to_owned(),
            intact_visible: 4,
            intact_hidden: 0,
            destroyed_visible: 0,
            destroyed_hidden: usize::from(*node != "bridge_truck01") * 3,
            has_destroyed_layout: *node != "bridge_truck01",
        })
        .collect()
}

/// Every truck's drawn layout after the settle frames: the intact side
/// hidden, the `destroyed` one drawn where the record authors it.
fn layout_after() -> Vec<ConvoyLayoutRow> {
    TRUCKS
        .iter()
        .map(|node| ConvoyLayoutRow {
            node: (*node).to_owned(),
            intact_visible: 0,
            intact_hidden: 4,
            destroyed_visible: usize::from(*node != "bridge_truck01") * 3,
            destroyed_hidden: 0,
            has_destroyed_layout: *node != "bridge_truck01",
        })
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
        layout_before: layout_before(),
        layout_after: layout_after(),
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
        "convoy_layout_swaps",
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
    let cases: [(&str, Mutate); 11] = [
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
        // The bug this check exists for: the destroyed truck keeps drawing
        // its intact layout.
        ("convoy_layout_swaps", |c| {
            c.layout_after[0].intact_visible = 4
        }),
        // The wreck layout staying hidden is the other half of the defect.
        ("convoy_layout_swaps", |c| {
            c.layout_after[1].destroyed_visible = 0;
            c.layout_after[1].destroyed_hidden = 3;
        }),
        // A wreck drawn before the first shot is as wrong as one never drawn.
        ("convoy_layout_swaps", |c| {
            c.layout_before[2].destroyed_visible = 3
        }),
        // No visible-node-state sample at all is a loud failure, not a pass.
        ("convoy_layout_swaps", |c| c.layout_after.clear()),
    ];
    for (id, mutate) in cases {
        let mut convoy = full();
        mutate(&mut convoy);
        let checks = evaluate_mission_smoke(&measured(convoy));
        assert!(!check(&checks, id).passed, "{id} must fail");
    }
}
