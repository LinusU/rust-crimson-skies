//! Acceptance scenario F32-B (the minimum scenario — an ace cannot fire a
//! disabled gun or an empty rocket rack — plus the maneuver selection that
//! goes with the target decision).
//!
//! Spec: `specs/F32-ai-combat-formations-aces-and-difficulty.md`, stage
//! `### F32-B`. Task test prefix: `accept_f32_b_`.
//!
//! These tests drive production code only: `cs_sim::ai::combat`'s
//! [`CombatPlanner`], its [`FiringSolution`] and [`CombatManeuver`]
//! vocabulary, the arsenal snapshot and the synthetic fixtures. Removing
//! the availability classification, the cooldown deference, the kind rule
//! or the maneuver map fails the test that names it.
//!
//! Every value here is newly authored synthetic fixture data, never
//! original game data.

use cs_sim::ai::combat::{
    ArsenalSnapshot, CombatError, CombatManeuver, CombatPlanner, CombatRequest, CombatRole,
    FireHoldReason, FiringSolution, MountAvailability, MountFireState, MountKind, PriorityPolicy,
    RoleArsenal, RoleAssignment, SYNTHETIC_SESSION, SkillKnobs, SkillProfile,
    synthetic_ace_bomber_profile, synthetic_actor, synthetic_arsenal, synthetic_bomber_profile,
    synthetic_candidate, synthetic_escort_profile,
};
use cs_sim::damage::{ActorId, DamageNodeKey};
use cs_sim::targeting::Allegiance;
use cs_types::Tick;
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

/// The target the ace is shooting at.
const TARGET: u64 = 2;

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn position(x: f64) -> WorldPosition {
    WorldPosition::try_new([x, 0.0, 0.0]).expect("the fixture position is finite")
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("the fixture mount key is valid")
}

/// One mount with every availability field set explicitly.
fn mount(
    name: &str,
    kind: MountKind,
    rounds: u64,
    disabled: bool,
    cooldown_ticks: u64,
) -> MountAvailability {
    MountAvailability {
        mount: key(name),
        kind,
        rounds,
        disabled,
        cooldown_ticks,
    }
}

/// The minimum scenario: an ordnance-carrying ace (a bomber variant) cannot
/// fire a gun whose damage node was destroyed, nor an intact but empty
/// rocket rack. Skill reaches the solution only as the aim error; it can
/// never override availability.
#[test]
fn accept_f32_b_ace_cannot_fire_a_disabled_gun_or_an_empty_rocket_rack() {
    let planner = CombatPlanner::new(SYNTHETIC_SESSION, &[synthetic_bomber_profile()])
        .expect("the bomber profile registers");
    let ace = synthetic_ace_bomber_profile();
    let arsenal = ArsenalSnapshot::try_new(vec![
        mount("gun_mount_1", MountKind::Gun, 300, true, 0),
        mount("ordnance_mount_1", MountKind::Ordnance, 0, false, 0),
    ])
    .expect("the two-mount arsenal is valid");

    let solution = planner
        .firing_solution(&ace, synthetic_actor(TARGET), Tick(4_000), &arsenal)
        .expect("the target belongs to the planner's session");

    assert_eq!(
        solution.state(&key("gun_mount_1")),
        Some(MountFireState::Disabled),
        "a destroyed gun mount is never fired"
    );
    assert_eq!(
        solution.state(&key("ordnance_mount_1")),
        Some(MountFireState::Empty),
        "an intact but empty rocket rack is never fired"
    );
    assert!(!solution.is_firing(), "the ace has nothing to shoot with");
    assert_eq!(solution.firing().count(), 0);
    assert_eq!(
        solution.hold(),
        Some(FireHoldReason::NoUsableMount {
            disabled: 1,
            empty: 1,
            cooling: 0,
            wrong_kind: 0,
        }),
        "the hold report separates destruction from exhaustion"
    );
    // The ace's skill is visible in the solution's aim error and cadence, so
    // the same mount availability can still produce a better shot when it is
    // loaded.
    assert_eq!(solution.aim_error_rad, ace.knobs().aim_error_rad);
    assert_eq!(solution.aim_error_rad, 0.015);
    assert_eq!(
        solution.fire_discipline_ticks,
        ace.knobs().fire_discipline_ticks
    );
}

/// The negative of the minimum scenario: the same disabled gun and empty
/// rack fire for nobody, and a loaded mount of an allowed kind fires for
/// both the ace and the slower bomber. Availability, not skill, decides.
#[test]
fn accept_f32_b_firing_availability_is_independent_of_shooter_skill() {
    let planner = CombatPlanner::new(SYNTHETIC_SESSION, &[synthetic_bomber_profile()])
        .expect("the bomber profile registers");
    let ace = synthetic_ace_bomber_profile();
    let rookie = synthetic_bomber_profile();

    let unavailable = ArsenalSnapshot::try_new(vec![
        mount("gun_mount_1", MountKind::Gun, 300, true, 0),
        mount("ordnance_mount_1", MountKind::Ordnance, 0, false, 0),
    ])
    .expect("the two-mount arsenal is valid");
    for profile in [&ace, &rookie] {
        let solution = planner
            .firing_solution(profile, synthetic_actor(TARGET), Tick(4_000), &unavailable)
            .expect("the target is in-session");
        assert!(
            !solution.is_firing(),
            "skill {profile:?} must not make a disabled gun or an empty rack fire"
        );
    }

    let loaded = ArsenalSnapshot::try_new(vec![
        mount("gun_mount_1", MountKind::Gun, 300, false, 0),
        mount("ordnance_mount_1", MountKind::Ordnance, 4, false, 0),
    ])
    .expect("the loaded arsenal is valid");
    for profile in [&ace, &rookie] {
        let solution = planner
            .firing_solution(profile, synthetic_actor(TARGET), Tick(4_000), &loaded)
            .expect("the target is in-session");
        assert_eq!(
            solution.state(&key("gun_mount_1")),
            Some(MountFireState::Firing)
        );
        assert_eq!(
            solution.state(&key("ordnance_mount_1")),
            Some(MountFireState::Firing)
        );
        assert!(solution.is_firing());
        assert_eq!(solution.hold(), None);
        assert_eq!(solution.firing().count(), 2);
    }
}

/// Every refusal is named, and the precedence is fixed so the reported
/// reason is a function of the mount, not of the order the snapshot was
/// built in: kind first, then destruction, then exhaustion, then cadence.
#[test]
fn accept_f32_b_firing_solution_names_each_refusal_kind() {
    let planner = CombatPlanner::new(SYNTHETIC_SESSION, &[synthetic_escort_profile()])
        .expect("the escort profile registers");
    let escort = synthetic_escort_profile();
    let arsenal = ArsenalSnapshot::try_new(vec![
        mount("gun_ready", MountKind::Gun, 120, false, 0),
        mount("gun_empty", MountKind::Gun, 0, false, 0),
        mount("gun_dead", MountKind::Gun, 50, true, 0),
        mount("gun_dead_and_cooling", MountKind::Gun, 50, true, 12),
        mount("gun_cooling", MountKind::Gun, 40, false, 12),
        mount("rack_ready", MountKind::Ordnance, 4, false, 0),
    ])
    .expect("the mixed arsenal is valid");

    let solution = planner
        .firing_solution(&escort, synthetic_actor(TARGET), Tick(4_000), &arsenal)
        .expect("the target is in-session");

    assert_eq!(
        solution.state(&key("gun_ready")),
        Some(MountFireState::Firing)
    );
    assert_eq!(
        solution.state(&key("gun_empty")),
        Some(MountFireState::Empty)
    );
    assert_eq!(
        solution.state(&key("gun_dead")),
        Some(MountFireState::Disabled)
    );
    assert_eq!(
        solution.state(&key("gun_dead_and_cooling")),
        Some(MountFireState::Disabled),
        "destruction outranks cadence"
    );
    assert_eq!(
        solution.state(&key("gun_cooling")),
        Some(MountFireState::CoolingDown {
            remaining_ticks: 12
        })
    );
    assert_eq!(
        solution.state(&key("rack_ready")),
        Some(MountFireState::WrongKind),
        "a guns-only role never fires ordnance, however ready the rack is"
    );
    assert_eq!(
        solution
            .firing()
            .map(|fire| fire.mount.clone())
            .collect::<Vec<_>>(),
        vec![key("gun_ready")],
        "only the ready gun fires"
    );
    assert_eq!(solution.hold(), None, "one ready gun is enough to fire");
}

/// A cooling mount is a *schedule* refusal, not an availability one: with
/// nothing ready it produces a hold whose cooling count is distinct from
/// the disabled and empty counts, and once the cadence expires it fires.
#[test]
fn accept_f32_b_a_cooling_mount_defers_without_being_unavailable() {
    let planner = CombatPlanner::new(SYNTHETIC_SESSION, &[synthetic_escort_profile()])
        .expect("the escort profile registers");
    let escort = synthetic_escort_profile();
    let cooling =
        ArsenalSnapshot::try_new(vec![mount("gun_mount_1", MountKind::Gun, 120, false, 30)])
            .expect("the cooling arsenal is valid");

    let solution = planner
        .firing_solution(&escort, synthetic_actor(TARGET), Tick(4_000), &cooling)
        .expect("the target is in-session");
    assert!(!solution.is_firing());
    assert_eq!(
        solution.hold(),
        Some(FireHoldReason::NoUsableMount {
            disabled: 0,
            empty: 0,
            cooling: 1,
            wrong_kind: 0,
        })
    );

    let ready = ArsenalSnapshot::try_new(vec![mount("gun_mount_1", MountKind::Gun, 120, false, 0)])
        .expect("the ready arsenal is valid");
    let solution = planner
        .firing_solution(&escort, synthetic_actor(TARGET), Tick(4_030), &ready)
        .expect("the target is in-session");
    assert_eq!(
        solution.state(&key("gun_mount_1")),
        Some(MountFireState::Firing),
        "the same mount fires once its cadence has expired"
    );
}

/// A role that declares no weapon fires nothing even with a full arsenal,
/// and reports that as its own hold reason rather than as a pile of
/// wrong-kind refusals.
#[test]
fn accept_f32_b_unarmed_roles_hold_fire_whatever_the_snapshot_carries() {
    let knobs = SkillKnobs {
        reaction_ticks: 12,
        aim_error_rad: 0.04,
        engagement_range_m: 600.0,
        fire_discipline_ticks: 20,
    };
    let priority = PriorityPolicy {
        protected_actor_weight: 0.0,
        objective_weight: 0.0,
        self_defense_weight: 1.0,
        proximity_weight: 0.0,
        threat_window_ticks: 60,
    };
    let evade = SkillProfile::try_new(CombatRole::Evade, RoleArsenal::none(), knobs, priority)
        .expect("an unarmed evasion profile is valid");

    let solution = FiringSolution::solve(
        &evade,
        synthetic_actor(TARGET),
        Tick(4_000),
        &synthetic_arsenal(),
    );
    assert!(!solution.is_firing());
    assert_eq!(
        solution.hold(),
        Some(FireHoldReason::RoleUnarmed),
        "a role with no declared weapon holds fire by role, not by count"
    );
    assert_eq!(
        solution.firing().count(),
        0,
        "the fixture arsenal is full, and still nothing fires"
    );
}

/// A firing solution is per-session state: a target from a stale generation
/// is refused by name even though the classification itself only reads the
/// profile and the snapshot.
#[test]
fn accept_f32_b_firing_solution_refuses_a_foreign_target() {
    let planner = CombatPlanner::new(SYNTHETIC_SESSION, &[synthetic_bomber_profile()])
        .expect("the bomber profile registers");
    let foreign = ActorId {
        session: session(SYNTHETIC_SESSION + 1),
        serial: TARGET,
    };
    let err = planner
        .firing_solution(
            &synthetic_bomber_profile(),
            foreign,
            Tick(4_000),
            &synthetic_arsenal(),
        )
        .expect_err("a foreign target is refused");
    match err {
        CombatError::ForeignSession { actor, session } => {
            assert_eq!(actor, foreign);
            assert_eq!(session, SYNTHETIC_SESSION);
        }
        other => panic!("expected a session refusal, got {other}"),
    }
}

/// The maneuver is a total, deterministic function of the role and whether
/// a target was chosen: the same input always yields the same intent.
#[test]
fn accept_f32_b_maneuver_follows_the_role_and_the_situation() {
    use CombatManeuver::{AttackRun, BreakAway, Engage, Hold, Screen, Withdraw};
    let cases = [
        (CombatRole::FighterAttack, true, Engage),
        (CombatRole::FighterAttack, false, Hold),
        (CombatRole::Intercept, true, Engage),
        (CombatRole::Intercept, false, Hold),
        (CombatRole::BomberRun, true, AttackRun),
        (CombatRole::BomberRun, false, Hold),
        (CombatRole::TorpedoRun, true, AttackRun),
        (CombatRole::TorpedoRun, false, Hold),
        (CombatRole::Escort, true, Engage),
        (CombatRole::Escort, false, Screen),
        (CombatRole::Evade, true, BreakAway),
        (CombatRole::Evade, false, BreakAway),
        (CombatRole::Retreat, true, Withdraw),
        (CombatRole::Retreat, false, Withdraw),
    ];
    for (role, has_target, expected) in cases {
        assert_eq!(
            CombatManeuver::select(role, has_target),
            expected,
            "{role} with target={has_target}"
        );
    }
    assert_eq!(CombatManeuver::ALL.len(), 6);
    for maneuver in CombatManeuver::ALL {
        assert!(!maneuver.label().is_empty());
    }
}

/// The decision carries the maneuver and the firing solution: an ace bomber
/// that selects a hostile still flies the attack run and still holds fire
/// when its mounts are unavailable. A decision that reported only the
/// target would not say what the role does about it.
#[test]
fn accept_f32_b_decide_reports_the_maneuver_and_the_firing_solution() {
    let planner = CombatPlanner::new(SYNTHETIC_SESSION, &[synthetic_bomber_profile()])
        .expect("the bomber profile registers");
    let ace = synthetic_ace_bomber_profile();
    let assignment = RoleAssignment::new(synthetic_actor(1), CombatRole::BomberRun);
    let candidates = vec![synthetic_candidate(
        TARGET,
        [400.0, 0.0, 0.0],
        Some(Allegiance::Hostile),
        true,
    )];
    let arsenal = ArsenalSnapshot::try_new(vec![
        mount("gun_mount_1", MountKind::Gun, 300, true, 0),
        mount("ordnance_mount_1", MountKind::Ordnance, 0, false, 0),
    ])
    .expect("the two-mount arsenal is valid");

    let decision = planner
        .decide(&CombatRequest {
            observer: synthetic_actor(1),
            now: Tick(4_000),
            observer_position: position(0.0),
            assignment: &assignment,
            formation: None,
            protected_alive: None,
            candidates: &candidates,
            arsenal: Some(&arsenal),
            profile: Some(&ace),
        })
        .expect("the request is well-formed");

    assert_eq!(decision.target, Some(synthetic_actor(TARGET)));
    assert_eq!(
        decision.trace.maneuver,
        CombatManeuver::AttackRun,
        "a bomber with a target flies the attack run"
    );
    let firing = decision
        .trace
        .firing
        .as_ref()
        .expect("the decision carries the firing solution");
    assert_eq!(firing.target, synthetic_actor(TARGET));
    assert_eq!(firing.tick, Tick(4_000));
    assert_eq!(firing.aim_error_rad, ace.knobs().aim_error_rad);
    assert!(!firing.is_firing());
    assert_eq!(
        firing.hold(),
        Some(FireHoldReason::NoUsableMount {
            disabled: 1,
            empty: 1,
            cooling: 0,
            wrong_kind: 0,
        })
    );
}

/// With no eligible candidate there is no target to solve fire against, so
/// the maneuver is chosen for "no target" and no firing solution is
/// invented. An escort with no target screens; a fighter holds.
#[test]
fn accept_f32_b_no_target_reports_the_no_target_maneuver_and_no_firing() {
    let planner = CombatPlanner::new(SYNTHETIC_SESSION, &[synthetic_escort_profile()])
        .expect("the escort profile registers");
    let assignment = RoleAssignment::new(synthetic_actor(1), CombatRole::Escort);
    let friend = vec![synthetic_candidate(
        TARGET,
        [200.0, 0.0, 0.0],
        Some(Allegiance::Friendly),
        false,
    )];
    let arsenal = synthetic_arsenal();
    let decision = planner
        .decide(&CombatRequest {
            observer: synthetic_actor(1),
            now: Tick(4_000),
            observer_position: position(0.0),
            assignment: &assignment,
            formation: None,
            protected_alive: None,
            candidates: &friend,
            arsenal: Some(&arsenal),
            profile: None,
        })
        .expect("the request is well-formed");

    assert_eq!(decision.target, None);
    assert_eq!(decision.trace.maneuver, CombatManeuver::Screen);
    assert!(
        decision.trace.firing.is_none(),
        "there is no target to solve fire against"
    );
}
