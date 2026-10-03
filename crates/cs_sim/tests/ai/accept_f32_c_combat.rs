//! Acceptance scenario F32-C (the minimum scenario — destroy the formation
//! leader mid-turn and the followers recover without NaNs or permanent orbit —
//! plus the ace variants and difficulty profiles the runtime selects).
//!
//! Spec: `specs/F32-ai-combat-formations-aces-and-difficulty.md`, stage
//! `### F32-C`. Task test prefix: `accept_f32_c_`.
//!
//! These tests drive production code only: `cs_sim::ai::combat`'s
//! [`CombatRuntime`], its [`FormationCoordinator`], [`DifficultyRoster`],
//! [`AceVariant`] and station vocabulary, and the F32-A/F32-B planner they
//! sit on. Removing the recovery application, the promotion, the survivor-only
//! centroid, the tick guard, the tier/ace selection or the finite-station
//! validation fails the test that names it.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_sim::ai::combat::{
    AceId, ArsenalSnapshot, CandidateView, CombatRole, CombatRuntime, CombatStep, CombatantRequest,
    DifficultyRoster, DifficultyTier, FORMATION_STATION_RADIUS_M, FORMATION_TRAIL_SPACING_M,
    FormationFacts, FormationId, FormationMemberReport, FormationRoster, FormationRosterMember,
    FormationSlot, FormationTick, MountAvailability, MountKind, ProfileSource, RecoveryAction,
    RecoveryPolicySet, RecoveryTrigger, RoleAssignment, SYNTHETIC_FORMATION_LEADER,
    SYNTHETIC_FORMATION_WINGMEN, SYNTHETIC_SESSION, StationAnchor, station_offset_m,
    synthetic_ace_id, synthetic_actor, synthetic_arsenal, synthetic_candidate,
    synthetic_combat_runtime, synthetic_difficulty_roster, synthetic_escort_profile,
    synthetic_fighter_profile, synthetic_formation_roster, synthetic_member_report,
    synthetic_recovery_policies,
};
use cs_sim::damage::DamageNodeKey;
use cs_sim::targeting::Allegiance;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::space::WorldPosition;

/// The formation the fixture declares.
const FORMATION: FormationId = FormationId(1);

/// The tick the scenario starts on.
const START: u64 = 4_000;

/// The hostile the followers still see.
const HOSTILE: u64 = 2;

/// The escort's charge, which no report ever destroys in this scenario.
const CHARGE: u64 = 20;

/// One formation tick's report: every registered member, exactly once.
fn report(
    leader_alive: bool,
    leader_at: [f64; 3],
    wingmen_at: [[f64; 3]; 2],
) -> Vec<FormationMemberReport> {
    vec![
        synthetic_member_report(0, SYNTHETIC_FORMATION_LEADER, leader_at, leader_alive),
        synthetic_member_report(1, SYNTHETIC_FORMATION_WINGMEN[0], wingmen_at[0], true),
        synthetic_member_report(2, SYNTHETIC_FORMATION_WINGMEN[1], wingmen_at[1], true),
    ]
}

/// A whole-mission tick: the formation intact and engaged with a hostile.
fn intact_tick(now: u64, members: &[FormationMemberReport]) -> FormationTick<'_> {
    FormationTick {
        formation: FORMATION,
        now: Tick(now),
        members,
        assigned_target: Some(synthetic_actor(HOSTILE)),
        assigned_target_alive: true,
        route_available: true,
    }
}

/// One escort follower of the synthetic formation, protecting its charge.
fn follower(serial: u64) -> RoleAssignment {
    let slot = SYNTHETIC_FORMATION_WINGMEN
        .iter()
        .position(|member| *member == serial)
        .map_or(0, |index| {
            u32::try_from(index + 1).expect("a small slot index")
        });
    RoleAssignment::protecting(
        synthetic_actor(serial),
        CombatRole::Escort,
        synthetic_actor(CHARGE),
    )
    .expect("an escort never protects itself")
    .in_formation(FormationSlot {
        formation: FORMATION,
        slot,
    })
}

/// What one follower is deciding about, apart from the actor and the tick.
#[derive(Clone, Copy)]
struct Scenario<'a> {
    position: [f64; 3],
    candidates: &'a [CandidateView],
    arsenal: &'a ArsenalSnapshot,
    ace: Option<&'a AceId>,
    tier: DifficultyTier,
}

/// A follower request against one hostile, at the follower's own position.
fn request<'a>(
    runtime: &'a CombatRuntime,
    serial: u64,
    now: u64,
    scenario: Scenario<'a>,
) -> CombatStep {
    let assignment = follower(serial);
    runtime
        .step(&CombatantRequest {
            observer: synthetic_actor(serial),
            now: Tick(now),
            observer_position: WorldPosition::try_new(scenario.position)
                .expect("the fixture position is finite"),
            assignment: &assignment,
            formation: Some(FORMATION),
            protected_alive: Some(true),
            candidates: scenario.candidates,
            arsenal: Some(scenario.arsenal),
            ace: scenario.ace,
            tier: scenario.tier,
        })
        .expect("the follower request is well-formed")
}

/// The scenario every call in a test shares unless it says otherwise.
fn standard<'a>(
    position: [f64; 3],
    candidates: &'a [CandidateView],
    arsenal: &'a ArsenalSnapshot,
) -> Scenario<'a> {
    Scenario {
        position,
        candidates,
        arsenal,
        ace: None,
        tier: DifficultyTier::Standard,
    }
}

fn hostile_at(x: f64) -> Vec<CandidateView> {
    vec![synthetic_candidate(
        HOSTILE,
        [x, 0.0, 0.0],
        Some(Allegiance::Hostile),
        true,
    )]
}

/// The minimum acceptance scenario (AC03): the formation's leader is destroyed
/// mid-turn and its followers recover.
///
/// Three things are checked, one per phrase of the requirement:
///
/// * **no NaNs** — every station is a finite world point, on every tick, and
///   every station is exactly its anchor plus the slot's designed offset, so
///   nothing was produced by normalizing a degenerate vector;
/// * **no permanent orbit** — no station and no anchor ever names the destroyed
///   actor again, and 40 further ticks of an unchanged situation produce a
///   bit-identical station, so the follower is not chasing a moving reference;
/// * **recovery, not a trace** — the leader actually changes, the coordinator's
///   post-recovery facts are what the planner then reasons about, and the
///   planner reported the very same declared path on the tick it fired.
#[test]
fn accept_f32_c_leader_destroyed_mid_turn_its_followers_recover() {
    let mut runtime = synthetic_combat_runtime();
    let hostile = hostile_at(300.0);
    let arsenal = synthetic_arsenal();

    // The turn before the loss: the formation flies behind its leader.
    let before = report(
        true,
        [1_000.0, 0.0, 0.0],
        [[800.0, 0.0, 0.0], [700.0, 0.0, 0.0]],
    );
    let update = runtime
        .update_formation(&intact_tick(START, &before))
        .expect("the intact tick applies");
    assert_eq!(update.trigger, None, "an intact formation recovers nothing");
    assert_eq!(update.action, None);
    assert_eq!(
        update.leader,
        Some(synthetic_actor(SYNTHETIC_FORMATION_LEADER))
    );
    assert_eq!(
        update.previous_leader,
        Some(synthetic_actor(SYNTHETIC_FORMATION_LEADER)),
        "the leader is unchanged and the trace says which one"
    );
    assert_eq!(
        update.anchor(),
        Some(StationAnchor::Leader(synthetic_actor(
            SYNTHETIC_FORMATION_LEADER
        )))
    );
    assert_eq!(
        update
            .stations
            .iter()
            .map(|station| station.position.to_array())
            .collect::<Vec<_>>(),
        vec![[880.0, 0.0, 0.0], [760.0, 0.0, 0.0]],
        "each follower trails the leader by its slot offset"
    );

    // The turn the leader is destroyed on.
    let loss = report(
        false,
        [1_000.0, 0.0, 0.0],
        [[810.0, 0.0, 0.0], [710.0, 0.0, 0.0]],
    );

    // The turn before the loss, the facts the coordinator produces say the
    // leader is alive and nothing is pending.
    let facts = runtime
        .facts(FORMATION, synthetic_actor(SYNTHETIC_FORMATION_WINGMEN[0]))
        .expect("the wingman is a living member");
    assert_eq!(facts.leader, synthetic_actor(SYNTHETIC_FORMATION_LEADER));
    assert!(facts.leader_alive);
    assert_eq!(facts.pending_trigger(), None);

    // The facts a producer derives from the *loss* report are the same
    // statements F32-A's `FormationFacts::pending_trigger` and the declared
    // policy read, and they are the pair the coordinator will apply. This is
    // the producer/consumer agreement: the trace's recovery and the applied
    // recovery cannot disagree, because both come from these two.
    let loss_facts = FormationFacts {
        formation: FORMATION,
        leader: synthetic_actor(SYNTHETIC_FORMATION_LEADER),
        leader_alive: false,
        observer_is_leader: false,
        assigned_target: Some(synthetic_actor(HOSTILE)),
        assigned_target_alive: true,
        route_available: true,
    };
    let expected_trigger = loss_facts
        .pending_trigger()
        .expect("a destroyed leader with a living follower is a leader loss");
    let expected_action = synthetic_recovery_policies().action(expected_trigger);
    assert_eq!(expected_trigger, RecoveryTrigger::LeaderLost);
    assert_eq!(
        expected_action,
        RecoveryAction::ReassignLead,
        "the declared policy, not an invented one"
    );

    let recovery = runtime
        .update_formation(&intact_tick(START + 1, &loss))
        .expect("the leader-loss tick applies");
    assert_eq!(recovery.trigger, Some(expected_trigger));
    assert_eq!(
        recovery.action,
        Some(expected_action),
        "the applied action is the one the declared policy names"
    );
    assert_eq!(
        recovery.previous_leader,
        Some(synthetic_actor(SYNTHETIC_FORMATION_LEADER)),
        "the trace names the leader that was lost"
    );
    assert_eq!(
        recovery.leader,
        Some(synthetic_actor(SYNTHETIC_FORMATION_WINGMEN[0])),
        "a survivor is promoted, deterministically the lowest living slot"
    );
    assert!(!recovery.dissolved);
    assert_eq!(recovery.latched, None);

    // The recovered formation: the promoted leader leads, and the remaining
    // follower trails it.
    assert_eq!(
        recovery.anchor(),
        Some(StationAnchor::Leader(synthetic_actor(
            SYNTHETIC_FORMATION_WINGMEN[0]
        )))
    );
    let promoted = synthetic_actor(SYNTHETIC_FORMATION_WINGMEN[0]);
    let trailing = synthetic_actor(SYNTHETIC_FORMATION_WINGMEN[1]);
    let dead = synthetic_actor(SYNTHETIC_FORMATION_LEADER);
    assert!(
        recovery.station(promoted).is_none(),
        "the promoted leader leads: it holds no station of its own"
    );
    let station = recovery
        .station(trailing)
        .expect("the surviving follower gets a station");
    assert_eq!(station.slot, 2);
    assert_eq!(
        station.position.to_array(),
        [810.0 - 2.0 * FORMATION_TRAIL_SPACING_M, 0.0, 0.0]
    );
    assert_eq!(station.radius_m, FORMATION_STATION_RADIUS_M);
    for coordinate in station.position.to_array() {
        assert!(
            coordinate.is_finite(),
            "a station is a finite world point: {station:?}"
        );
    }

    // After the recovery the planner is quiet: the same decision no longer
    // reports a pending recovery, because the leader it names is alive.
    let after = request(
        &runtime,
        SYNTHETIC_FORMATION_WINGMEN[0],
        START + 1,
        standard([810.0, 0.0, 0.0], &hostile, &arsenal),
    );
    assert_eq!(
        after.trace().recovery,
        None,
        "the applied recovery is visible to the next decision"
    );
    let facts = after.formation.expect("the follower is still a member");
    assert_eq!(facts.leader, promoted);
    assert!(facts.leader_alive);

    // No permanent orbit: forty further ticks of an unchanged situation keep
    // naming the same living anchor and the same station, and the destroyed
    // actor appears in neither.
    let mut settled: Option<[f64; 3]> = None;
    for offset in 2..42 {
        let now = START + offset;
        let members = report(
            false,
            [1_000.0, 0.0, 0.0],
            [[810.0, 0.0, 0.0], [710.0, 0.0, 0.0]],
        );
        let update = runtime
            .update_formation(&intact_tick(now, &members))
            .expect("the settled tick applies");
        assert_eq!(update.trigger, None, "a recovered formation recovers once");
        assert_eq!(update.leader, Some(promoted));
        assert_eq!(update.anchor(), Some(StationAnchor::Leader(promoted)));
        assert!(
            update.stations.iter().all(|station| station.actor != dead),
            "a retired member is never handed a station again"
        );
        assert!(
            update
                .stations
                .iter()
                .all(|station| station.anchor != StationAnchor::Leader(dead)),
            "a retired member is never an anchor again"
        );
        let position = update
            .station(trailing)
            .expect("the follower keeps its station")
            .position
            .to_array();
        match settled {
            None => settled = Some(position),
            Some(first) => assert_eq!(
                position, first,
                "an unchanged situation yields an unchanged station, not an orbit"
            ),
        }
        assert_eq!(
            position,
            [810.0 - 2.0 * FORMATION_TRAIL_SPACING_M, 0.0, 0.0],
            "the station is the anchor plus the slot offset, every tick"
        );
        assert_eq!(station_offset_m(2), [-240.0, 0.0, 0.0]);
    }
}

/// A formation whose declared leader-loss path is `Regroup` closes up on the
/// point its survivors already share — and that point is computed from the
/// survivors alone, so a destroyed leader parked at an absurd coordinate cannot
/// contaminate it.
#[test]
fn accept_f32_c_regroup_recovery_anchors_the_survivors_not_the_destroyed_leader() {
    let regrouping = RecoveryPolicySet {
        leader_loss: RecoveryAction::Regroup,
        ..synthetic_recovery_policies()
    };
    let mut runtime = CombatRuntime::new(
        SYNTHETIC_SESSION,
        &[synthetic_escort_profile()],
        Vec::new(),
        synthetic_difficulty_roster(),
    )
    .expect("the runtime is valid")
    .with_formation(synthetic_formation_roster(), regrouping)
    .expect("the formation registers");

    let members = report(
        false,
        [f64::MAX, f64::MAX, f64::MAX],
        [[810.0, 0.0, 0.0], [710.0, 0.0, 0.0]],
    );
    let update = runtime
        .update_formation(&intact_tick(START, &members))
        .expect("the regroup applies");

    assert_eq!(update.trigger, Some(RecoveryTrigger::LeaderLost));
    assert_eq!(update.action, Some(RecoveryAction::Regroup));
    assert_eq!(update.leader, None, "regroup promotes nobody");
    assert_eq!(
        update.anchor(),
        Some(StationAnchor::RegroupPoint),
        "the survivors' centroid is the anchor, not the wreck"
    );
    let centroid = [760.0, 0.0, 0.0];
    assert_eq!(
        update
            .stations
            .iter()
            .map(|station| station.position.to_array())
            .collect::<Vec<_>>(),
        vec![
            [centroid[0] + station_offset_m(1)[0], 0.0, 0.0],
            [centroid[0] + station_offset_m(2)[0], 0.0, 0.0],
        ],
        "every survivor takes a station on the regroup point"
    );
    assert_eq!(update.stations.len(), 2, "every survivor is a follower now");
    for station in &update.stations {
        for coordinate in station.position.to_array() {
            assert!(
                coordinate.is_finite(),
                "the destroyed leader's coordinate never reaches a station: {station:?}"
            );
        }
    }
}

/// The other declared leader-loss path, `HoldFormation`, keeps the shape but
/// never anchors it on a wreck: with no living leader the survivors close up on
/// the point they share.
#[test]
fn accept_f32_c_holding_a_shape_never_anchors_it_on_a_destroyed_leader() {
    let holding = RecoveryPolicySet {
        leader_loss: RecoveryAction::HoldFormation,
        ..synthetic_recovery_policies()
    };
    let mut runtime = CombatRuntime::new(
        SYNTHETIC_SESSION,
        &[synthetic_escort_profile()],
        Vec::new(),
        synthetic_difficulty_roster(),
    )
    .expect("the runtime is valid")
    .with_formation(synthetic_formation_roster(), holding)
    .expect("the formation registers");

    let members = report(
        false,
        [900.0, 500.0, 0.0],
        [[800.0, 500.0, 0.0], [700.0, 500.0, 0.0]],
    );
    let update = runtime
        .update_formation(&intact_tick(START, &members))
        .expect("the hold applies");
    assert_eq!(update.action, Some(RecoveryAction::HoldFormation));
    assert_eq!(update.leader, None);
    assert_eq!(update.anchor(), Some(StationAnchor::RegroupPoint));
    assert_eq!(
        update
            .stations
            .iter()
            .map(|station| station.position.to_array())
            .collect::<Vec<_>>(),
        vec![[630.0, 500.0, 0.0], [510.0, 500.0, 0.0]],
        "the survivors close on their own centroid"
    );
}

/// Teardown: when nothing survives, the formation is dissolved rather than
/// electing a leader out of the dead, and a later report cannot resurrect it.
#[test]
fn accept_f32_c_a_formation_with_no_survivor_is_torn_down_not_recovered() {
    let mut runtime = synthetic_combat_runtime();
    let members = report(
        false,
        [1_000.0, 0.0, 0.0],
        [[800.0, 0.0, 0.0], [700.0, 0.0, 0.0]],
    );
    let tick = FormationTick {
        members: &members
            .iter()
            .map(|member| FormationMemberReport {
                alive: false,
                ..*member
            })
            .collect::<Vec<_>>(),
        ..intact_tick(START, &members)
    };
    let update = runtime
        .update_formation(&tick)
        .expect("the last-member tick applies");
    assert!(
        update.dissolved,
        "nothing survived, so there is nothing to lead"
    );
    assert_eq!(update.leader, None);
    assert!(update.stations.is_empty(), "no station without a survivor");
    assert!(
        update.released.is_empty(),
        "there is nobody left to release a station to: a retired member has none"
    );
    assert_eq!(update.trigger, None, "there is nobody left to recover");

    // Teardown means teardown: nothing about the dissolved formation survives.
    assert!(runtime.formations().next().is_none());
    assert!(
        runtime
            .facts(FORMATION, synthetic_actor(SYNTHETIC_FORMATION_WINGMEN[0]))
            .is_none()
    );
    assert!(
        runtime.dissolve_formation(FORMATION).is_err(),
        "a dissolved formation is not there to dissolve twice"
    );
}

/// Teardown, second form: a declared `Withdraw` releases every station and
/// dissolves the formation even though its members are still flying.
#[test]
fn accept_f32_c_a_declared_withdraw_releases_every_station() {
    let withdrawing = RecoveryPolicySet {
        leader_loss: RecoveryAction::Withdraw,
        ..synthetic_recovery_policies()
    };
    let mut runtime = CombatRuntime::new(
        SYNTHETIC_SESSION,
        &[synthetic_escort_profile()],
        Vec::new(),
        synthetic_difficulty_roster(),
    )
    .expect("the runtime is valid")
    .with_formation(synthetic_formation_roster(), withdrawing)
    .expect("the formation registers");

    let members = report(
        false,
        [1_000.0, 0.0, 0.0],
        [[800.0, 0.0, 0.0], [700.0, 0.0, 0.0]],
    );
    let update = runtime
        .update_formation(&intact_tick(START, &members))
        .expect("the withdrawal applies");
    assert_eq!(update.action, Some(RecoveryAction::Withdraw));
    assert!(update.dissolved);
    assert!(update.stations.is_empty());
    assert_eq!(update.released.len(), 2);
    assert!(runtime.formations().next().is_none());
}

/// A recovery is answered once, not on every tick: while the fact that raised
/// it is still unhealthy the update reports the trigger as latched, so a
/// declared path cannot become a permanent state.
#[test]
fn accept_f32_c_a_recovery_is_answered_once_until_its_fact_recovers() {
    let mut runtime = synthetic_combat_runtime();
    let first = report(
        true,
        [1_000.0, 0.0, 0.0],
        [[800.0, 0.0, 0.0], [700.0, 0.0, 0.0]],
    );
    runtime
        .update_formation(&intact_tick(START, &first))
        .expect("the intact tick applies");

    // The assigned target is destroyed: the declared path regroups.
    let members = report(
        true,
        [1_000.0, 0.0, 0.0],
        [[800.0, 0.0, 0.0], [700.0, 0.0, 0.0]],
    );
    let broken = |now: u64| FormationTick {
        assigned_target_alive: false,
        ..intact_tick(now, &members)
    };
    let update = runtime
        .update_formation(&broken(START + 1))
        .expect("the target-loss tick applies");
    assert_eq!(
        update.trigger,
        Some(RecoveryTrigger::AssignedTargetDestroyed)
    );
    assert_eq!(update.action, Some(RecoveryAction::Regroup));
    assert_eq!(
        update.leader,
        Some(synthetic_actor(SYNTHETIC_FORMATION_LEADER))
    );
    assert_eq!(
        update.anchor(),
        Some(StationAnchor::RegroupPoint),
        "regrouping moves the anchor even though the leader lives"
    );

    let again = runtime
        .update_formation(&broken(START + 2))
        .expect("the next tick with the same fact applies");
    assert_eq!(again.trigger, None, "the path was already answered");
    assert_eq!(
        again.latched,
        Some(RecoveryTrigger::AssignedTargetDestroyed)
    );
    assert_eq!(
        again.anchor(),
        Some(StationAnchor::RegroupPoint),
        "and the shape it left behind is kept, not redone"
    );

    // A new, living assigned target releases the latch.
    let recovered = runtime
        .update_formation(&intact_tick(START + 3, &members))
        .expect("the recovered tick applies");
    assert_eq!(recovered.latched, None);
    assert_eq!(recovered.trigger, None);
}

/// An ace variant is selected as data: the runtime runs the profile the
/// boundary lowered, records where it came from, and refuses an ace that does
/// not match the actor's role or the mission's selected tier.
#[test]
fn accept_f32_c_an_ace_variant_is_selected_as_data_for_its_tier() {
    let runtime = synthetic_combat_runtime();
    let ace = synthetic_ace_id();
    let escort = follower(SYNTHETIC_FORMATION_WINGMEN[0]);
    let fighter = RoleAssignment::new(
        synthetic_actor(SYNTHETIC_FORMATION_LEADER),
        CombatRole::FighterAttack,
    );

    let (plain, source) = runtime
        .resolve_profile(&escort, None, DifficultyTier::Standard)
        .expect("the standard tier is declared");
    assert_eq!(
        source,
        ProfileSource::Role {
            tier: DifficultyTier::Standard
        }
    );
    assert_eq!(plain.knobs().aim_error_rad, 0.06);
    assert_eq!(plain.knobs().reaction_ticks, 24);

    let (variant, source) = runtime
        .resolve_profile(&escort, Some(&ace), DifficultyTier::Standard)
        .expect("the ace matches the role and the tier");
    assert_eq!(
        source,
        ProfileSource::Ace {
            id: ace.clone(),
            tier: DifficultyTier::Standard,
        }
    );
    assert_eq!(
        variant.knobs().aim_error_rad,
        0.015,
        "the ace flies the behavior the boundary lowered"
    );
    assert_eq!(variant.knobs().reaction_ticks, 6);
    assert_eq!(variant.arsenal(), plain.arsenal());
    assert_eq!(
        variant.priority().protected_actor_weight,
        4.0,
        "an ace answers its charge harder"
    );

    // The decision itself carries the variant, so a trace can say which
    // profile produced it.
    let hostile = hostile_at(300.0);
    let arsenal = synthetic_arsenal();
    let rookies = request(
        &runtime,
        SYNTHETIC_FORMATION_WINGMEN[0],
        START,
        standard([800.0, 0.0, 0.0], &hostile, &arsenal),
    );
    let aces = request(
        &runtime,
        SYNTHETIC_FORMATION_WINGMEN[0],
        START,
        Scenario {
            ace: Some(&ace),
            ..standard([800.0, 0.0, 0.0], &hostile, &arsenal)
        },
    );
    assert_eq!(
        rookies.source,
        ProfileSource::Role {
            tier: DifficultyTier::Standard
        }
    );
    assert!(matches!(aces.source, ProfileSource::Ace { .. }));
    assert_eq!(
        rookies.target(),
        aces.target(),
        "the ace picks the same target"
    );
    assert_eq!(
        aces.trace().firing.as_ref().map(|fire| fire.aim_error_rad),
        Some(0.015),
        "the firing solution runs under the ace's profile"
    );
    assert_eq!(
        rookies
            .trace()
            .firing
            .as_ref()
            .map(|fire| fire.aim_error_rad),
        Some(0.06)
    );

    // An ace of the escort role cannot defend a charge as a fighter, and an
    // ace lowered for the standard tier is not silently run at the elite one.
    assert!(
        runtime
            .resolve_profile(&fighter, Some(&ace), DifficultyTier::Standard)
            .is_err()
    );
    assert!(
        runtime
            .resolve_profile(&escort, Some(&ace), DifficultyTier::Elite)
            .is_err(),
        "an ace lowered for one tier is not run at another"
    );
}

/// Difficulty selects a behavior profile and nothing else. Non-negotiable 1
/// ("never increase simulation speed to fake difficulty") and 2 ("the same
/// weapon availability as the player") are checked as observable properties of
/// every declared tier.
#[test]
fn accept_f32_c_difficulty_moves_behavior_parameters_and_nothing_else() {
    let runtime = synthetic_combat_runtime();
    let escort = follower(SYNTHETIC_FORMATION_WINGMEN[0]);
    let hostile = hostile_at(300.0);
    let arsenal = synthetic_arsenal();

    let mut aims = Vec::new();
    for tier in DifficultyTier::ALL {
        let (profile, source) = runtime
            .resolve_profile(&escort, None, *tier)
            .expect("every declared tier resolves");
        assert_eq!(source, ProfileSource::Role { tier: *tier });
        aims.push(profile.knobs().aim_error_rad);

        let base = synthetic_escort_profile();
        assert_eq!(
            profile.knobs().engagement_range_m,
            base.knobs().engagement_range_m,
            "no tier changes what counts as an engagement"
        );
        assert_eq!(
            profile.knobs().fire_discipline_ticks,
            base.knobs().fire_discipline_ticks,
            "no tier changes the actor's fire cadence"
        );
        assert_eq!(
            profile.priority().threat_window_ticks,
            base.priority().threat_window_ticks,
            "no tier changes how long an attack counts as a threat"
        );
        assert_eq!(
            profile.arsenal(),
            base.arsenal(),
            "no tier changes what the role may shoot with"
        );

        let step = request(
            &runtime,
            SYNTHETIC_FORMATION_WINGMEN[0],
            START,
            Scenario {
                tier: *tier,
                ..standard([800.0, 0.0, 0.0], &hostile, &arsenal)
            },
        );
        assert_eq!(
            step.trace().tick,
            Tick(START),
            "a tier never rescales the clock a decision is made on"
        );
        let firing = step.trace().firing.as_ref().expect("a target was selected");
        assert_eq!(
            firing
                .state(&DamageNodeKey::new("ordnance_mount_1").expect("a mount key"))
                .map(|state| state.label()),
            Some("wrong_kind"),
            "no tier lets a guns-only escort fire a rocket rack"
        );
        assert_eq!(
            firing
                .state(&DamageNodeKey::new("gun_mount_1").expect("a mount key"))
                .map(|state| state.label()),
            Some("firing"),
            "the guns a guns-only escort may fire fire at every tier"
        );
    }
    aims.dedup();
    assert_eq!(
        aims.len(),
        DifficultyTier::ALL.len(),
        "each declared tier carries its own behavior: {aims:?}"
    );

    // The declared tiers are a total order, so F32-D's probe can iterate them.
    assert_eq!(
        runtime.difficulty().tiers().collect::<Vec<_>>(),
        DifficultyTier::ALL.to_vec()
    );
    assert_eq!(
        runtime.difficulty().roles(DifficultyTier::Elite),
        vec![CombatRole::FighterAttack, CombatRole::Escort],
        "a tier's roles are reported in the role vocabulary's own order"
    );
}

/// The runtime is one authority per session generation: the ace registry, the
/// difficulty roster and the formation coordinator all answer for the session
/// they were built for, and a request naming a formation the observer does not
/// belong to is refused rather than answered from a stranger's state.
#[test]
fn accept_f32_c_the_runtime_is_scoped_to_one_session_generation() {
    let runtime = synthetic_combat_runtime();
    assert_eq!(runtime.session(), SYNTHETIC_SESSION);
    assert_eq!(runtime.planner().session(), SYNTHETIC_SESSION);
    assert_eq!(
        runtime.planner().formations().collect::<Vec<_>>(),
        vec![FORMATION]
    );
    assert_eq!(
        runtime
            .difficulty()
            .profile(DifficultyTier::Standard, CombatRole::FighterAttack),
        Ok(synthetic_fighter_profile())
    );

    // The ace registry is keyed by the content identity, not by an actor: one
    // pilot, one behavior variant, reachable from either query.
    let (id, variant) = runtime
        .aces()
        .next()
        .expect("the runtime carries the fixture ace");
    assert_eq!(*id, synthetic_ace_id());
    assert_eq!(variant.base_role(), CombatRole::Escort);
    assert_eq!(variant.tier(), DifficultyTier::Standard);
    assert_eq!(
        cs_sim::ai::combat::AceId::try_new(id.as_content().clone()),
        Ok(synthetic_ace_id()),
        "an ace id round-trips through its catalog id"
    );
    assert_eq!(variant.profile().role(), variant.base_role());

    // An empty arsenal is a valid snapshot: a tier cannot conjure a mount.
    let empty = ArsenalSnapshot::try_new(Vec::new()).expect("an empty arsenal is valid");
    assert_eq!(empty.report().total_mounts, 0);
    let bare = MountAvailability::usable(
        DamageNodeKey::new("gun_mount_1").expect("a mount key"),
        MountKind::Gun,
        0,
    );
    assert!(!bare.is_usable(), "an empty gun is not usable at any tier");
}

/// The roster the runtime hands to the coordinator is validated once, at
/// registration: a formation whose leader is not a member, a formation with no
/// member and a doubled slot are all refused by name.
#[test]
fn accept_f32_c_formation_registration_refuses_an_incoherent_shape() {
    assert!(
        FormationRoster::try_new(FORMATION, 0, Vec::new()).is_err(),
        "a formation with nobody in it cannot lead or regroup"
    );
    assert!(
        FormationRoster::try_new(
            FORMATION,
            9,
            vec![FormationRosterMember {
                slot: 0,
                actor: synthetic_actor(SYNTHETIC_FORMATION_LEADER),
            }],
        )
        .is_err(),
        "a formation led by a slot it does not have cannot elect a successor"
    );
    assert!(
        FormationRoster::try_new(
            FORMATION,
            0,
            vec![
                FormationRosterMember {
                    slot: 0,
                    actor: synthetic_actor(SYNTHETIC_FORMATION_LEADER),
                },
                FormationRosterMember {
                    slot: 0,
                    actor: synthetic_actor(SYNTHETIC_FORMATION_WINGMEN[0]),
                },
            ],
        )
        .is_err(),
        "two actors in one slot would make the station ambiguous"
    );
    assert_eq!(synthetic_formation_roster().leader_slot(), 0);
    assert_eq!(synthetic_formation_roster().members().len(), 3);
}

/// An actor identity is not a content identity: the ace key is a `pilot` id,
/// and the runtime refuses one from another namespace.
#[test]
fn accept_f32_c_an_ace_key_is_a_pilot_id_and_not_an_actor_or_airframe() {
    assert!(
        cs_sim::ai::combat::AceId::try_new(
            ContentId::from_source(ContentKind::Airframe, "synthetic.ace-plane")
                .expect("a valid airframe id")
        )
        .is_err(),
        "an ace is a pilot's behavior, not an airframe's"
    );
    // The actor identity and the ace identity are two different kinds of key,
    // so no assertion of inequality is even expressible between them: one is
    // session-qualified, the other is a `pilot` catalog id. What *is*
    // expressible is that the ace key names a pilot and round-trips through
    // its catalog id.
    assert_eq!(synthetic_ace_id().as_content().kind(), ContentKind::Pilot);
    assert_eq!(
        synthetic_ace_id().as_content().as_str(),
        "pilot/synthetic.ace-wing-leader",
        "the runtime and the declared fixture name one pilot"
    );
}

/// A roster that declares only the baseline tier refuses every other tier by
/// name: choosing difficulty is an explicit mission decision, and defaulting
/// it to the baseline would hide a content bug behind a plausible profile.
#[test]
fn accept_f32_c_an_undeclared_difficulty_tier_is_refused_not_defaulted() {
    let baseline = DifficultyRoster::new()
        .with_tier(DifficultyTier::Standard, &[synthetic_escort_profile()])
        .expect("the baseline tier is valid");
    let runtime = CombatRuntime::new(
        SYNTHETIC_SESSION,
        &[synthetic_escort_profile()],
        Vec::new(),
        baseline,
    )
    .expect("the runtime is valid");
    let escort = follower(SYNTHETIC_FORMATION_WINGMEN[0]);
    assert!(
        runtime
            .resolve_profile(&escort, None, DifficultyTier::Standard)
            .is_ok()
    );
    for tier in [
        DifficultyTier::Relaxed,
        DifficultyTier::Hard,
        DifficultyTier::Elite,
    ] {
        assert!(
            runtime.resolve_profile(&escort, None, tier).is_err(),
            "{tier} was never declared, so there is no profile to run it under"
        );
    }
    // And a declared tier that says nothing about the assigned role.
    let partial = DifficultyRoster::new()
        .with_tier(DifficultyTier::Elite, &[synthetic_fighter_profile()])
        .expect("the tier is valid");
    let fighter = RoleAssignment::new(
        synthetic_actor(SYNTHETIC_FORMATION_LEADER),
        CombatRole::FighterAttack,
    );
    let other = CombatRuntime::new(
        SYNTHETIC_SESSION,
        &[synthetic_escort_profile()],
        Vec::new(),
        partial,
    )
    .expect("the runtime is valid");
    assert!(
        other
            .resolve_profile(&escort, None, DifficultyTier::Elite)
            .is_err(),
        "the elite tier declares no escort profile"
    );
    assert!(
        other
            .resolve_profile(&fighter, None, DifficultyTier::Elite)
            .is_ok()
    );
}
