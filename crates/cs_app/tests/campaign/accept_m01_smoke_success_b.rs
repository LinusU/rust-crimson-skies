//! Rally #2082 (`M01-SMOKE-SUCCESS-B`): the owner-command smoke's success
//! stage, part B (`--smoke-success`): the drop-off landing (`OBJECTIVE21`,
//! `OBJECTIVE23`) and the Kestrel kills with the fire key. Task test prefix:
//! `accept_m01_smoke_success_b_`.
//!
//! The judging is pure, so these tests drive the same `evaluate_mission_smoke`
//! the windowed run ends with. The windowed half is the retail run recorded in
//! `docs/findings/2026-10-24-m01-smoke-success-b.md`.

use cs_app::mission_session::{Check, SmokeMeasurements, SuccessMetrics, evaluate_mission_smoke};
use cs_sim::damage::ActorId;
use cs_types::net::SessionId;

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

pub(super) fn full() -> SuccessMetrics {
    let names = ["medkestrel_1", "medkestrel_2", "medkestrel_3"];
    SuccessMetrics {
        start_frame: Some(2_600),
        centre: Some([0.0, 500.0, 0.0]),
        placements: 90,
        completed: vec![
            (11, 2_610),
            (26, 2_740),
            (20, 3_000),
            (21, 3_001),
            (22, 3_700),
        ],
        frames_delivered: true,
        near_permille: 900,
        marked_permille: 900,
        marked_shot_frame: Some(2_740),
        landing_at: Some([10.0, 5.0, 10.0]),
        landing_placements: 40,
        kestrel_actors: names
            .iter()
            .enumerate()
            .map(|(i, n)| ((*n).to_owned(), actor(i as u64 + 1)))
            .collect(),
        fire_presses: 200,
        kestrel_placements: 150,
        kestrels_destroyed: names
            .iter()
            .enumerate()
            .map(|(i, n)| ((*n).to_owned(), 3_900 + i as u32 * 100))
            .collect(),
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
fn accept_m01_smoke_success_b_whole_sequence_passes() {
    // The stage's later beats (part C) are checks too, so the whole sequence is
    // the complete run, which part C's fixture holds.
    let checks = evaluate_mission_smoke(&measured(super::accept_m01_smoke_success_c::full()));
    for id in [
        "success_landing_block_completed",
        "success_kestrel_wake_block_completed",
        "success_kestrels_destroyed_by_fire_key",
    ] {
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
fn accept_m01_smoke_success_b_empty_stage_fails_the_new_checks() {
    let checks = evaluate_mission_smoke(&measured(SuccessMetrics::default()));
    for id in [
        "success_landing_block_completed",
        "success_kestrel_wake_block_completed",
        "success_kestrels_destroyed_by_fire_key",
    ] {
        assert!(
            !check(&checks, id).passed,
            "{id} must fail on the empty stage"
        );
    }
}

#[test]
fn accept_m01_smoke_success_b_each_missing_piece_fails_its_own_check() {
    type Mutate = fn(&mut SuccessMetrics);
    let cases: [(&str, Mutate); 8] = [
        ("success_landing_block_completed", |c| {
            c.completed.retain(|(block, _)| *block != 20)
        }),
        ("success_landing_block_completed", |c| {
            c.landing_placements = 0
        }),
        ("success_kestrel_wake_block_completed", |c| {
            c.completed.retain(|(block, _)| *block != 22)
        }),
        ("success_kestrels_destroyed_by_fire_key", |c| {
            c.kestrels_destroyed.pop();
        }),
        ("success_kestrels_destroyed_by_fire_key", |c| {
            c.fire_presses = 0
        }),
        ("success_kestrels_destroyed_by_fire_key", |c| {
            c.kestrel_actors.clear()
        }),
        ("success_kestrels_destroyed_by_fire_key", |c| {
            c.gave_up_on = Some("medkestrel_3".to_owned())
        }),
        ("success_frames_drawn", |c| c.kestrels_permille = 10),
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
