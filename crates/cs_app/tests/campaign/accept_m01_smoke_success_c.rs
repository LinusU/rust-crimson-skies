//! Rally #2083 (`M01-SMOKE-SUCCESS-C`): the owner-command smoke's success
//! stage, part C (`--smoke-success`): the hydrogen tank (`OBJECTIVE25`), the
//! group-clear block (`OBJECTIVE13`), the docking (`OBJECTIVE14`) and the
//! host's own terminal (`OBJECTIVE15`'s `INSTANTWIN`). Task test prefix:
//! `accept_m01_smoke_success_c_`.
//!
//! The judging is pure, so these tests drive the same `evaluate_mission_smoke`
//! the windowed run ends with. The windowed half is the retail run recorded in
//! `docs/findings/2026-10-25-m01-smoke-success-c.md`.

use cs_app::mission_session::{
    Check, SmokeMeasurements, SuccessMetrics, SuccessTerminal, evaluate_mission_smoke,
};

fn check<'a>(checks: &'a [Check], id: &str) -> &'a Check {
    checks
        .iter()
        .find(|check| check.id == id)
        .unwrap_or_else(|| panic!("no check named {id}: {checks:#?}"))
}

/// The whole run: part B's metrics plus the end of the route.
pub(super) fn full() -> SuccessMetrics {
    let mut metrics = super::accept_m01_smoke_success_b::full();
    metrics
        .completed
        .extend([(24, 4_664), (12, 4_665), (13, 5_085)]);
    metrics.tank_at = Some([-6_359.0, 227.0, -7_703.0]);
    metrics.tank_placements = 70;
    metrics.tank_fire_presses = 22;
    metrics.tank_shot_frame = Some(4_702);
    metrics.hydrogen_shot_frame = Some(4_754);
    metrics.dock_at = Some([-1_401.0, 464.0, -1_295.0]);
    metrics.dock_placements = 55;
    metrics.terminal = Some(SuccessTerminal {
        outcome: "success".to_owned(),
        source: "control_program".to_owned(),
        requested_by: Some(14),
        tick: 10_169,
        exit_code: 0,
        frame: 5_086,
    });
    metrics.settled_shot_frame = Some(5_206);
    metrics.tank_permille = 900;
    metrics.hydrogen_permille = 900;
    metrics.settled_permille = 900;
    metrics
}

fn measured(success: SuccessMetrics) -> SmokeMeasurements {
    SmokeMeasurements {
        success: Some(success),
        ..SmokeMeasurements::default()
    }
}

const NEW_CHECKS: [&str; 4] = [
    "success_hydrogen_fire_block_completed",
    "success_group_clear_block_completed",
    "success_dock_block_completed",
    "success_terminal_is_control_program_success",
];

#[test]
fn accept_m01_smoke_success_c_whole_route_passes() {
    let checks = evaluate_mission_smoke(&measured(full()));
    for id in NEW_CHECKS {
        assert!(
            check(&checks, id).passed,
            "{id}: {}",
            check(&checks, id).detail
        );
    }
    for check in &checks {
        assert!(check.passed, "{} failed: {}", check.id, check.detail);
    }
}

#[test]
fn accept_m01_smoke_success_c_empty_stage_fails_the_new_checks() {
    let checks = evaluate_mission_smoke(&measured(SuccessMetrics::default()));
    for id in NEW_CHECKS {
        assert!(
            !check(&checks, id).passed,
            "{id} must fail on the empty stage"
        );
    }
}

#[test]
fn accept_m01_smoke_success_c_each_missing_piece_fails_its_own_check() {
    type Mutate = fn(&mut SuccessMetrics);
    let cases: [(&str, Mutate); 13] = [
        ("success_hydrogen_fire_block_completed", |c| {
            c.completed.retain(|(block, _)| *block != 24)
        }),
        ("success_hydrogen_fire_block_completed", |c| {
            c.tank_fire_presses = 0
        }),
        ("success_hydrogen_fire_block_completed", |c| {
            c.tank_gave_up = true
        }),
        ("success_group_clear_block_completed", |c| {
            c.completed.retain(|(block, _)| *block != 12)
        }),
        ("success_dock_block_completed", |c| {
            c.completed.retain(|(block, _)| *block != 13)
        }),
        ("success_dock_block_completed", |c| c.dock_placements = 0),
        ("success_dock_block_completed", |c| c.dock_gave_up = true),
        ("success_terminal_is_control_program_success", |c| {
            c.terminal = None
        }),
        ("success_terminal_is_control_program_success", |c| {
            c.terminal.as_mut().unwrap().outcome = "failure".to_owned()
        }),
        ("success_terminal_is_control_program_success", |c| {
            c.terminal.as_mut().unwrap().source = "player_destroyed".to_owned()
        }),
        ("success_terminal_is_control_program_success", |c| {
            c.terminal.as_mut().unwrap().requested_by = Some(3)
        }),
        ("success_terminal_is_control_program_success", |c| {
            c.terminal.as_mut().unwrap().exit_code = 1
        }),
        ("success_frames_drawn", |c| c.settled_permille = 10),
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
