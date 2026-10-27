//! Acceptance scenario for task #556 (`F32-STATION-CONSUME`): navigation
//! consumes the stations a real [`FormationUpdate`] assigns.
//!
//! Spec: `specs/F32-ai-combat-formations-aces-and-difficulty.md`,
//! non-negotiable 4. Task test prefix: `accept_f32_station_`.
//!
//! The updates come from `CombatRuntime::update_formation` and are applied to
//! the production `StationKeeper`, which steers through `Navigator::decide`.
//! Removing the keeper's `apply` (or its decision) fails every test here.
//!
//! Every value is newly authored synthetic fixture data.

use cs_sim::ai::combat::{
    FORMATION_STATION_RADIUS_M, FormationId, FormationMemberReport, FormationTick,
    SYNTHETIC_FORMATION_LEADER, SYNTHETIC_FORMATION_WINGMEN, SYNTHETIC_SESSION, StationAnchor,
    synthetic_actor, synthetic_combat_runtime, synthetic_member_report,
};
use cs_sim::ai::navigation::{
    NavState, NavigationCadence, Navigator, StationChange, StationKeeper, StationRequest,
    StationStatus, synthetic_maneuver_envelope,
};
use cs_types::Tick;

const FORMATION: FormationId = FormationId(1);
const DT_S: f64 = 1.0 / 60.0;
const START: u64 = 4_000;

fn keeper() -> StationKeeper {
    let navigator = Navigator::new(
        synthetic_maneuver_envelope(),
        NavigationCadence::designed_default(),
    )
    .expect("the synthetic envelope and cadence are valid");
    StationKeeper::new(SYNTHETIC_SESSION, navigator)
}

fn members(leader_alive: bool, leader_at: [f64; 3]) -> Vec<FormationMemberReport> {
    vec![
        synthetic_member_report(0, SYNTHETIC_FORMATION_LEADER, leader_at, leader_alive),
        synthetic_member_report(1, SYNTHETIC_FORMATION_WINGMEN[0], [800.0, 0.0, 0.0], true),
        synthetic_member_report(2, SYNTHETIC_FORMATION_WINGMEN[1], [700.0, 0.0, 0.0], true),
    ]
}

fn tick<'a>(now: u64, members: &'a [FormationMemberReport]) -> FormationTick<'a> {
    FormationTick {
        formation: FORMATION,
        now: Tick(now),
        members,
        assigned_target: Some(synthetic_actor(2)),
        assigned_target_alive: true,
        route_available: true,
    }
}

fn request(actor: u64, now: u64, state: NavState) -> StationRequest<'static> {
    StationRequest {
        actor: synthetic_actor(actor),
        tick: Tick(now),
        generation: SYNTHETIC_SESSION,
        state,
        blockers: &[],
        clearance_m: 5.0,
        dt_s: DT_S,
    }
}

fn state_at(position_m: [f64; 3]) -> NavState {
    NavState {
        position_m,
        heading_rad: 0.0,
        bank_rad: 0.0,
        speed_mps: 40.0,
        climb_mps: 0.0,
    }
}

fn advance(step: &cs_sim::ai::navigation::RouteStep) -> NavState {
    NavState {
        position_m: step.to_m,
        heading_rad: step.heading_rad,
        bank_rad: 0.0,
        speed_mps: step.speed_mps,
        climb_mps: step.climb_mps,
    }
}

#[test]
fn accept_f32_station_follower_flies_to_its_station_and_reports_arrival() {
    let mut runtime = synthetic_combat_runtime();
    let mut keeper = keeper();
    let reports = members(true, [1_000.0, 0.0, 0.0]);
    let update = runtime
        .update_formation(&tick(START, &reports))
        .expect("the intact tick applies");
    let wingman = SYNTHETIC_FORMATION_WINGMEN[0];
    let station = update.station(synthetic_actor(wingman)).expect("a station");

    let changes = keeper.apply(&update).expect("the update applies");
    assert_eq!(changes.len(), 2, "both wingmen gain a station");
    assert!(matches!(changes[0], StationChange::Assigned { .. }));

    let mut state = state_at([station.position.to_array()[0], 0.0, 600.0]);
    let mut first_distance = None;
    let mut arrived_at = None;
    for index in 0..2_000_u64 {
        let outcome = keeper
            .decide(&request(wingman, START + index, state))
            .expect("the decision is valid")
            .expect("a follower with a station is steered");
        assert_eq!(outcome.station, station);
        first_distance.get_or_insert(outcome.decision.diagnostics.distance_to_target_m);
        if outcome.status == StationStatus::OnStation {
            arrived_at = Some(index);
            break;
        }
        state = advance(&outcome.decision.step);
    }
    assert!(first_distance.expect("ran") > FORMATION_STATION_RADIUS_M);
    assert!(arrived_at.is_some(), "the follower reaches its station");
}

#[test]
fn accept_f32_station_no_station_means_route_navigation_keeps_the_follower() {
    let keeper = keeper();
    let outcome = keeper
        .decide(&request(
            SYNTHETIC_FORMATION_WINGMEN[0],
            START,
            state_at([0.0; 3]),
        ))
        .expect("an unassigned follower is not an error");
    assert!(outcome.is_none());
}

#[test]
fn accept_f32_station_leader_loss_reanchors_the_remaining_follower() {
    let mut runtime = synthetic_combat_runtime();
    let mut keeper = keeper();
    let before = members(true, [1_000.0, 0.0, 0.0]);
    let intact = runtime
        .update_formation(&tick(START, &before))
        .expect("intact");
    keeper.apply(&intact).expect("applies");
    let wingman = SYNTHETIC_FORMATION_WINGMEN[1];
    let old = keeper.station(synthetic_actor(wingman)).expect("station");
    let old_decision = keeper
        .decide(&request(wingman, START, state_at([0.0; 3])))
        .expect("valid")
        .expect("steered");

    let after = members(false, [1_000.0, 0.0, 0.0]);
    let recovered = runtime
        .update_formation(&tick(START + 60, &after))
        .expect("recovery applies");
    let changes = keeper.apply(&recovered).expect("applies");

    let new = keeper.station(synthetic_actor(wingman));
    match recovered.anchor() {
        Some(StationAnchor::Leader(promoted)) => {
            assert!(changes.iter().any(|change| matches!(
                change,
                StationChange::Reanchored { actor, from, to }
                    if *actor == synthetic_actor(wingman)
                        && *from != *to
                        && *to == StationAnchor::Leader(promoted)
            )));
            let new = new.expect("the follower keeps a station");
            assert_ne!(new.anchor, old.anchor);
        }
        other => panic!("the synthetic policy promotes a leader, got {other:?}"),
    }
    let new_decision = keeper
        .decide(&request(wingman, START + 60, state_at([0.0; 3])))
        .expect("valid")
        .expect("steered");
    assert_ne!(
        new_decision.station.anchor, old_decision.station.anchor,
        "the follower now flies to the new anchor"
    );
}

#[test]
fn accept_f32_station_withdrawn_when_a_member_is_retired_or_the_formation_dissolves() {
    let mut runtime = synthetic_combat_runtime();
    let mut keeper = keeper();
    let reports = members(true, [1_000.0, 0.0, 0.0]);
    let update = runtime
        .update_formation(&tick(START, &reports))
        .expect("intact");
    keeper.apply(&update).expect("applies");
    let wingman = synthetic_actor(SYNTHETIC_FORMATION_WINGMEN[0]);
    assert!(keeper.station(wingman).is_some());

    // The member is retired directly: it no longer holds a station.
    assert!(keeper.withdraw(wingman));
    assert!(
        keeper
            .decide(&request(
                SYNTHETIC_FORMATION_WINGMEN[0],
                START,
                state_at([0.0; 3])
            ))
            .expect("valid")
            .is_none()
    );

    // An update that stops reporting the other wingman withdraws it too.
    let mut shrunk = update.clone();
    shrunk.now = Tick(START + 1);
    shrunk.stations.clear();
    shrunk.dissolved = true;
    let changes = keeper.apply(&shrunk).expect("applies");
    assert_eq!(
        changes,
        vec![StationChange::Withdrawn {
            actor: synthetic_actor(SYNTHETIC_FORMATION_WINGMEN[1])
        }]
    );
    assert_eq!(keeper.actors().count(), 0);
}

#[test]
fn accept_f32_station_a_stale_update_does_not_restore_a_withdrawn_station() {
    let mut runtime = synthetic_combat_runtime();
    let mut keeper = keeper();
    let reports = members(true, [1_000.0, 0.0, 0.0]);
    let first = runtime
        .update_formation(&tick(START, &reports))
        .expect("intact");
    let mut later = first.clone();
    later.now = Tick(START + 10);
    later.stations.clear();
    keeper.apply(&first).expect("applies");
    keeper.apply(&later).expect("applies");
    assert_eq!(keeper.actors().count(), 0);
    assert!(keeper.apply(&first).expect("ignored").is_empty());
    assert_eq!(keeper.actors().count(), 0);
}
