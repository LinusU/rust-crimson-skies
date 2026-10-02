//! Acceptance scenario F32-A (the minimum scenario — an escort
//! prioritizes an attacker threatening its protected actor — plus the
//! AC03 contract half that a formation reports a declared recovery path).
//!
//! Spec: `specs/F32-ai-combat-formations-aces-and-difficulty.md`, stage
//! `### F32-A`. Task test prefix: `accept_f32_a_`.
//!
//! These tests drive production code only: `cs_sim::ai::combat`'s
//! [`CombatPlanner`], its [`RoleAssignment`], [`CandidateView`],
//! [`ThreatEvidence`] and synthetic fixture.
//!
//! Every value here is newly authored synthetic fixture data, never
//! original game data.

use cs_sim::ai::combat::{
    CandidateVerdict, CombatError, CombatPlanner, CombatRequest, CombatRole, FormationFacts,
    FormationId, FormationSlot, PriorityTerm, ReactionState, RecoveryAction, RecoveryTrigger,
    RejectReason, RoleAssignment, SYNTHETIC_SESSION, synthetic_ace_profile, synthetic_actor,
    synthetic_arsenal, synthetic_candidate, synthetic_combat_planner, synthetic_escort_profile,
    synthetic_fighter_profile, synthetic_recovery_policies, synthetic_rookie_escort_profile,
    synthetic_threat,
};
use cs_sim::damage::{ActorId, HitEventId};
use cs_sim::targeting::Allegiance;
use cs_types::Tick;
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

/// The protected charge of the AC01 scenario: the bomber the escort is
/// assigned to defend.
const CHARGE: u64 = 10;
/// The attacker threatening it, 800 m away.
const ATTACKER: u64 = 2;
/// A harmless hostile, 300 m away — nearer than the attacker, so a policy
/// that scored only proximity would pick it.
const BYSTANDER: u64 = 5;
/// A friendly wingman 150 m away, hostile-gate-failing.
const FRIEND: u64 = 3;

const DECISION_TICK: u64 = 4_000;
/// The tick the AC01 attacker's authoritative attack landed on: 100 ticks
/// before the decision — past the escort's 24-tick reaction delay, inside
/// the 120-tick threat window.
const AC01_ATTACK_TICK: u64 = DECISION_TICK - 100;

fn position(x: f64) -> WorldPosition {
    WorldPosition::try_new([x, 0.0, 0.0]).expect("the fixture position is finite")
}

/// The escort assignment of the AC01 scenario: the observer defends the
/// charge.
fn escort_assignment() -> RoleAssignment {
    RoleAssignment::protecting(
        synthetic_actor(1),
        CombatRole::Escort,
        synthetic_actor(CHARGE),
    )
    .expect("the observer is not its own protected actor")
    .in_formation(FormationSlot {
        formation: FormationId(1),
        slot: 1,
    })
}

/// The AC01 candidate set, in a deliberately unhelpful order: the
/// bystander, the friendly, then the attacker.
fn candidates() -> Vec<cs_sim::ai::combat::CandidateView> {
    vec![
        synthetic_candidate(
            BYSTANDER,
            [300.0, 0.0, 0.0],
            Some(Allegiance::Hostile),
            false,
        ),
        synthetic_candidate(FRIEND, [150.0, 0.0, 0.0], Some(Allegiance::Friendly), false),
        synthetic_candidate(
            ATTACKER,
            [800.0, 0.0, 0.0],
            Some(Allegiance::Hostile),
            false,
        )
        .with_threat(synthetic_threat(
            ATTACKER,
            CHARGE,
            Tick(AC01_ATTACK_TICK),
            2,
            0,
        )),
    ]
}

/// Runs one decision with the synthetic planner.
fn decide(
    planner: &CombatPlanner,
    assignment: &RoleAssignment,
    candidates: &[cs_sim::ai::combat::CandidateView],
) -> cs_sim::ai::combat::CombatDecision {
    planner
        .decide(&CombatRequest {
            observer: synthetic_actor(1),
            now: Tick(DECISION_TICK),
            observer_position: position(0.0),
            assignment,
            formation: None,
            protected_alive: Some(true),
            candidates,
            arsenal: Some(&synthetic_arsenal()),
            profile: None,
        })
        .expect("the AC01 request is well-formed")
}

/// AC01 minimum scenario: the escort selects the 800 m attacker that has
/// an authoritative attack on its protected actor, not the nearer 300 m
/// hostile that has not touched the charge. The declared policy weights
/// the protected actor's threat (2.0) above proximity (0.5).
#[test]
fn accept_f32_a_escort_prioritizes_attacker_threatening_protected_actor() {
    let planner = synthetic_combat_planner();
    let assignment = escort_assignment();
    let decision = decide(&planner, &assignment, &candidates());

    assert_eq!(
        decision.target,
        Some(synthetic_actor(ATTACKER)),
        "the escort must answer what threatens its protected actor, not the nearest hostile"
    );
    assert_eq!(decision.role, CombatRole::Escort);
    assert_eq!(decision.trace.chosen, Some(synthetic_actor(ATTACKER)));
    assert_eq!(decision.trace.hold, None);

    // The trace explains the choice: the attacker's protected-actor term
    // carries the full weight, the bystander's is zero.
    let attacker = decision
        .trace
        .candidate(synthetic_actor(ATTACKER))
        .expect("the attacker is in the trace");
    assert_eq!(attacker.verdict, CandidateVerdict::Eligible);
    assert_eq!(
        attacker.reaction,
        ReactionState::Noticed { age_ticks: 100 },
        "the attack is past the escort's 24-tick reaction delay"
    );
    let threat_term = attacker
        .term(PriorityTerm::ProtectedActorThreat)
        .expect("the protected-actor term was scored");
    assert_eq!(threat_term.value, 1.0);
    assert_eq!(threat_term.weight, 2.0);
    assert_eq!(threat_term.contribution, 2.0);
    assert!(
        attacker.total > 2.0,
        "proximity adds to the attacker's score"
    );

    let bystander = decision
        .trace
        .candidate(synthetic_actor(BYSTANDER))
        .expect("the bystander is in the trace");
    assert_eq!(
        bystander
            .term(PriorityTerm::ProtectedActorThreat)
            .expect("the bystander's term was scored")
            .contribution,
        0.0,
        "a hostile that has not touched the charge scores nothing on that term"
    );
    assert!(bystander.total < attacker.total);
}

/// The negative of AC01 in the same fixture: a policy with no
/// protected-actor weight — the rookie escort — ignores the attack on its
/// charge and takes the nearest hostile. This is what proves the term, not
/// the role name, drives the scenario.
#[test]
fn accept_f32_a_policy_without_protected_actor_weight_selects_the_nearer_hostile() {
    let rookie = synthetic_rookie_escort_profile();
    let planner = CombatPlanner::new(SYNTHETIC_SESSION, &[rookie]).expect("one profile registers");
    let assignment = escort_assignment();
    let decision = decide(&planner, &assignment, &candidates());

    assert_eq!(
        decision.target,
        Some(synthetic_actor(BYSTANDER)),
        "without the protected-actor term the escort has no reason to prefer the attacker"
    );
}

/// The AC01 evidence requirement: a threat is an authoritative
/// [`HitEventId`] against the protected actor. An attacker with no
/// evidence, or evidence against a *different* victim, scores nothing on
/// the protected-actor term and loses to the nearer hostile.
#[test]
fn accept_f32_a_protected_actor_term_requires_authoritative_evidence_against_it() {
    let planner = synthetic_combat_planner();
    let assignment = escort_assignment();

    // No evidence at all: proximity wins.
    let unevidenced = vec![
        synthetic_candidate(
            BYSTANDER,
            [300.0, 0.0, 0.0],
            Some(Allegiance::Hostile),
            false,
        ),
        synthetic_candidate(
            ATTACKER,
            [800.0, 0.0, 0.0],
            Some(Allegiance::Hostile),
            false,
        ),
    ];
    assert_eq!(
        decide(&planner, &assignment, &unevidenced).target,
        Some(synthetic_actor(BYSTANDER))
    );

    // Evidence against a third party is not evidence against the charge.
    let wrong_victim = vec![
        synthetic_candidate(
            BYSTANDER,
            [300.0, 0.0, 0.0],
            Some(Allegiance::Hostile),
            false,
        ),
        synthetic_candidate(
            ATTACKER,
            [800.0, 0.0, 0.0],
            Some(Allegiance::Hostile),
            false,
        )
        .with_threat(synthetic_threat(ATTACKER, 99, Tick(3_990), 2, 0)),
    ];
    let decision = decide(&planner, &assignment, &wrong_victim);
    assert_eq!(
        decision.target,
        Some(synthetic_actor(BYSTANDER)),
        "an attack on someone else must not raise the protected-actor term"
    );
    assert_eq!(
        decision
            .trace
            .candidate(synthetic_actor(ATTACKER))
            .expect("the attacker is in the trace")
            .term(PriorityTerm::ProtectedActorThreat)
            .expect("the term was scored")
            .contribution,
        0.0
    );
}

/// Hostility is a gate, not a weight: a friendly, a neutral and an
/// *undeclared* pair are all refused, and the objective term cannot
/// promote a non-hostile into a target (F32 non-negotiable 3).
#[test]
fn accept_f32_a_hostility_gate_refuses_friendly_neutral_and_undeclared_candidates() {
    let planner = synthetic_combat_planner();
    let assignment = escort_assignment();
    let cases = [
        (
            Some(Allegiance::Friendly),
            RejectReason::NotHostile {
                allegiance: Allegiance::Friendly,
            },
        ),
        (
            Some(Allegiance::Neutral),
            RejectReason::NotHostile {
                allegiance: Allegiance::Neutral,
            },
        ),
        (None, RejectReason::UndeclaredAllegiance),
    ];
    for (allegiance, expected) in cases {
        let candidates = vec![
            synthetic_candidate(
                BYSTANDER,
                [300.0, 0.0, 0.0],
                Some(Allegiance::Hostile),
                false,
            ),
            // The objective is 100 m away: the highest objective weight in
            // the policy still must not make it a target.
            synthetic_candidate(7, [100.0, 0.0, 0.0], allegiance, true),
        ];
        let decision = decide(&planner, &assignment, &candidates);
        assert_eq!(
            decision.target,
            Some(synthetic_actor(BYSTANDER)),
            "a non-hostile objective must stay unselectable (allegiance {allegiance:?})"
        );
        let refused = decision
            .trace
            .candidate(synthetic_actor(7))
            .expect("the objective is in the trace");
        assert_eq!(refused.verdict, CandidateVerdict::Rejected(expected));
    }
}

/// The script-assigned objective is a *separate* predicate that outranks
/// proximity within the hostiles: the fighter-attack policy weights the
/// objective 4.0 against proximity 1.0, so it takes the 350 m objective
/// over the 200 m bystander.
#[test]
fn accept_f32_a_script_assigned_objective_outranks_proximity() {
    let planner = synthetic_combat_planner();
    let assignment = RoleAssignment::new(synthetic_actor(1), CombatRole::FighterAttack);
    let candidates = vec![
        synthetic_candidate(
            BYSTANDER,
            [200.0, 0.0, 0.0],
            Some(Allegiance::Hostile),
            false,
        ),
        synthetic_candidate(ATTACKER, [350.0, 0.0, 0.0], Some(Allegiance::Hostile), true),
    ];
    let decision = decide(&planner, &assignment, &candidates);
    assert_eq!(decision.target, Some(synthetic_actor(ATTACKER)));

    let objective = decision
        .trace
        .candidate(synthetic_actor(ATTACKER))
        .expect("the objective is in the trace");
    assert!(objective.objective);
    assert_eq!(
        objective
            .term(PriorityTerm::ScriptObjective)
            .expect("the objective term was scored")
            .contribution,
        4.0
    );
}

/// Friendly-fire avoidance and line of fire are separate from hostility:
/// the selected hostile is still the target, and the veto is reported on
/// it rather than silently swapping the target for the friendly
/// (non-negotiable 3).
#[test]
fn accept_f32_a_line_of_fire_veto_is_reported_on_the_selected_hostile() {
    let planner = synthetic_combat_planner();
    let assignment = escort_assignment();
    let candidates = vec![
        synthetic_candidate(
            BYSTANDER,
            [300.0, 0.0, 0.0],
            Some(Allegiance::Hostile),
            false,
        ),
        synthetic_candidate(
            ATTACKER,
            [800.0, 0.0, 0.0],
            Some(Allegiance::Hostile),
            false,
        )
        .with_threat(synthetic_threat(
            ATTACKER,
            CHARGE,
            Tick(AC01_ATTACK_TICK),
            2,
            0,
        ))
        .with_friendlies_in_line_of_fire(2),
    ];
    let decision = decide(&planner, &assignment, &candidates);

    assert_eq!(
        decision.target,
        Some(synthetic_actor(ATTACKER)),
        "a friendly in the line of fire must not change who the target is"
    );
    let attacker = decision
        .trace
        .candidate(synthetic_actor(ATTACKER))
        .expect("the attacker is in the trace");
    assert_eq!(attacker.verdict, CandidateVerdict::Eligible);
    assert_eq!(
        attacker.fire_veto,
        Some(cs_sim::ai::combat::FireVeto::FriendlyInLineOfFire { friendlies: 2 }),
        "the veto is reported on the target, separately from the gate"
    );
}

/// An ace is a behavior variant: the same role with a 6-tick reaction sees
/// an attack 10 ticks old that the 24-tick escort has not noticed yet. The
/// ace therefore defends the charge where the rookie escort does not. This
/// is the "aces are data-driven behavior/skill variants" contract, with no
/// health or damage field anywhere in the profile.
#[test]
fn accept_f32_a_ace_variant_reacts_where_a_slow_profile_has_not_noticed_yet() {
    let now = Tick(DECISION_TICK);
    let observed = Tick(DECISION_TICK - 10);
    let candidates = vec![
        synthetic_candidate(
            BYSTANDER,
            [300.0, 0.0, 0.0],
            Some(Allegiance::Hostile),
            false,
        ),
        synthetic_candidate(
            ATTACKER,
            [800.0, 0.0, 0.0],
            Some(Allegiance::Hostile),
            false,
        )
        .with_threat(synthetic_threat(ATTACKER, CHARGE, observed, 2, 0)),
    ];
    let assignment = escort_assignment();

    let ace = CombatPlanner::new(SYNTHETIC_SESSION, &[synthetic_ace_profile()])
        .expect("the ace profile registers");
    let escort = CombatPlanner::new(SYNTHETIC_SESSION, &[synthetic_escort_profile()])
        .expect("the escort profile registers");

    let request = |planner: &CombatPlanner, profile: &cs_sim::ai::combat::SkillProfile| {
        planner
            .decide(&CombatRequest {
                observer: synthetic_actor(1),
                now,
                observer_position: position(0.0),
                assignment: &assignment,
                formation: None,
                protected_alive: Some(true),
                candidates: &candidates,
                arsenal: None,
                profile: Some(profile),
            })
            .expect("the request is well-formed")
    };

    let ace_decision = request(&ace, &synthetic_ace_profile());
    let escort_decision = request(&escort, &synthetic_escort_profile());
    assert_eq!(
        ace_decision.target,
        Some(synthetic_actor(ATTACKER)),
        "a 10-tick-old attack is past the ace's 6-tick reaction delay"
    );
    assert_eq!(
        escort_decision.target,
        Some(synthetic_actor(BYSTANDER)),
        "the same attack is still inside the escort's 24-tick delay, so it is deferred"
    );
    assert_eq!(
        escort_decision
            .trace
            .candidate(synthetic_actor(ATTACKER))
            .expect("the attacker is in the trace")
            .reaction,
        ReactionState::Deferred {
            age_ticks: 10,
            required_ticks: 24,
        },
        "a deferred reaction is recorded, not dropped: the trace shows the age and the delay"
    );
}

/// The threat window bounds the evidence: an attack older than the
/// declared window is no longer a live threat, so the protected-actor term
/// drops out and the escort takes the nearer hostile.
#[test]
fn accept_f32_a_threat_outside_the_declared_window_stops_counting() {
    let planner = synthetic_combat_planner();
    let assignment = escort_assignment();
    let stale = Tick(DECISION_TICK - 1_000);
    let candidates = vec![
        synthetic_candidate(
            BYSTANDER,
            [300.0, 0.0, 0.0],
            Some(Allegiance::Hostile),
            false,
        ),
        synthetic_candidate(
            ATTACKER,
            [800.0, 0.0, 0.0],
            Some(Allegiance::Hostile),
            false,
        )
        .with_threat(synthetic_threat(ATTACKER, CHARGE, stale, 2, 0)),
    ];
    let decision = decide(&planner, &assignment, &candidates);
    assert_eq!(
        decision.target,
        Some(synthetic_actor(BYSTANDER)),
        "an attack older than the 120-tick window is not a live threat"
    );
}

/// A destroyed protected actor drops the protected-actor term — there is
/// nothing left to defend — and the formation reports the declared
/// recovery action for that trigger (non-negotiable 4).
#[test]
fn accept_f32_a_lost_protected_actor_reports_the_declared_recovery_action() {
    let planner = synthetic_combat_planner();
    let assignment = escort_assignment();
    let facts = FormationFacts {
        formation: FormationId(1),
        leader: synthetic_actor(2),
        leader_alive: true,
        observer_is_leader: false,
        assigned_target: None,
        assigned_target_alive: true,
        route_available: true,
    };
    let candidates = vec![
        synthetic_candidate(
            BYSTANDER,
            [300.0, 0.0, 0.0],
            Some(Allegiance::Hostile),
            false,
        ),
        synthetic_candidate(
            ATTACKER,
            [800.0, 0.0, 0.0],
            Some(Allegiance::Hostile),
            false,
        )
        .with_threat(synthetic_threat(
            ATTACKER,
            CHARGE,
            Tick(AC01_ATTACK_TICK),
            2,
            0,
        )),
    ];

    let decision = planner
        .decide(&CombatRequest {
            observer: synthetic_actor(1),
            now: Tick(DECISION_TICK),
            observer_position: position(0.0),
            assignment: &assignment,
            formation: Some(&facts),
            protected_alive: Some(false),
            candidates: &candidates,
            arsenal: None,
            profile: None,
        })
        .expect("the request is well-formed");

    assert_eq!(
        decision.target,
        Some(synthetic_actor(BYSTANDER)),
        "with the charge destroyed the protected-actor term is meaningless"
    );
    let recovery = decision
        .trace
        .recovery
        .expect("a destroyed charge raises the declared recovery trigger");
    assert_eq!(recovery.trigger, RecoveryTrigger::ProtectedActorLost);
    assert_eq!(
        recovery.action,
        synthetic_recovery_policies().protected_actor_lost
    );
    assert_eq!(recovery.action, RecoveryAction::Regroup);
}

/// Every declared recovery trigger is reported with the action the
/// formation declared for it, and the trigger order is fixed so the
/// reported trigger is a function of the facts, not of the order the
/// producers filled them in.
#[test]
fn accept_f32_a_every_recovery_trigger_reports_its_declared_action() {
    let planner = synthetic_combat_planner();
    let assignment = RoleAssignment::new(synthetic_actor(1), CombatRole::FighterAttack)
        .in_formation(FormationSlot {
            formation: FormationId(1),
            slot: 1,
        });
    let policies = synthetic_recovery_policies();

    let cases = [
        (
            FormationFacts {
                formation: FormationId(1),
                leader: synthetic_actor(2),
                leader_alive: false,
                observer_is_leader: false,
                assigned_target: Some(synthetic_actor(8)),
                assigned_target_alive: false,
                route_available: false,
            },
            RecoveryTrigger::LeaderLost,
            policies.leader_loss,
        ),
        (
            FormationFacts {
                formation: FormationId(1),
                leader: synthetic_actor(2),
                leader_alive: true,
                observer_is_leader: false,
                assigned_target: Some(synthetic_actor(8)),
                assigned_target_alive: false,
                route_available: false,
            },
            RecoveryTrigger::AssignedTargetDestroyed,
            policies.assigned_target_destroyed,
        ),
        (
            FormationFacts {
                formation: FormationId(1),
                leader: synthetic_actor(2),
                leader_alive: true,
                observer_is_leader: false,
                assigned_target: None,
                assigned_target_alive: true,
                route_available: false,
            },
            RecoveryTrigger::RouteInterrupted,
            policies.route_interrupted,
        ),
    ];

    for (facts, expected_trigger, expected_action) in cases {
        let decision = planner
            .decide(&CombatRequest {
                observer: synthetic_actor(1),
                now: Tick(DECISION_TICK),
                observer_position: position(0.0),
                assignment: &assignment,
                formation: Some(&facts),
                protected_alive: None,
                candidates: &[],
                arsenal: None,
                profile: None,
            })
            .expect("the request is well-formed");
        let recovery = decision
            .trace
            .recovery
            .expect("the pending trigger is reported");
        assert_eq!(
            recovery.trigger, expected_trigger,
            "trigger priority is fixed"
        );
        assert_eq!(recovery.action, expected_action);
    }
}

/// The decision is a function of the candidates, not of the order the ECS
/// presented them in: every permutation of the AC01 candidate set yields
/// the same target and the same trace order.
#[test]
fn accept_f32_a_decision_is_independent_of_candidate_order() {
    let planner = synthetic_combat_planner();
    let assignment = escort_assignment();
    let ordered = candidates();
    let mut permutations: Vec<Vec<_>> = Vec::new();
    let reversed: Vec<_> = ordered.iter().rev().cloned().collect();
    permutations.push(ordered.clone());
    permutations.push(reversed);
    permutations.push(vec![
        ordered[2].clone(),
        ordered[0].clone(),
        ordered[1].clone(),
    ]);
    permutations.push(vec![
        ordered[1].clone(),
        ordered[2].clone(),
        ordered[0].clone(),
    ]);

    let mut reference: Option<(Option<ActorId>, Vec<ActorId>)> = None;
    for permutation in &permutations {
        let decision = decide(&planner, &assignment, permutation);
        let order: Vec<ActorId> = decision
            .trace
            .candidates
            .iter()
            .map(|trace| trace.actor)
            .collect();
        match &reference {
            None => reference = Some((decision.target, order)),
            Some((target, expected_order)) => {
                assert_eq!(
                    decision.target, *target,
                    "candidate order must not change the target"
                );
                assert_eq!(
                    &order, expected_order,
                    "the trace order is the total order, not the request order"
                );
            }
        }
    }
    // The eligible candidate is emitted first; the rejected ones follow in
    // actor-id order.
    let (target, order) = reference.expect("at least one permutation ran");
    assert_eq!(target, Some(synthetic_actor(ATTACKER)));
    assert_eq!(
        order,
        vec![
            synthetic_actor(ATTACKER),
            synthetic_actor(BYSTANDER),
            synthetic_actor(FRIEND),
        ]
    );
}

/// The engagement range is a gate, not a weight: a hostile beyond the
/// profile's 1.5 km range is refused by name even though it is attacking
/// the protected actor.
#[test]
fn accept_f32_a_candidate_beyond_the_engagement_range_is_refused() {
    let planner = synthetic_combat_planner();
    let assignment = escort_assignment();
    let candidates = vec![
        synthetic_candidate(
            BYSTANDER,
            [300.0, 0.0, 0.0],
            Some(Allegiance::Hostile),
            false,
        ),
        synthetic_candidate(
            ATTACKER,
            [2_000.0, 0.0, 0.0],
            Some(Allegiance::Hostile),
            false,
        )
        .with_threat(synthetic_threat(
            ATTACKER,
            CHARGE,
            Tick(AC01_ATTACK_TICK),
            2,
            0,
        )),
    ];
    let decision = decide(&planner, &assignment, &candidates);
    assert_eq!(decision.target, Some(synthetic_actor(BYSTANDER)));
    let attacker = decision
        .trace
        .candidate(synthetic_actor(ATTACKER))
        .expect("the attacker is in the trace");
    assert_eq!(
        attacker.verdict,
        CandidateVerdict::Rejected(RejectReason::BeyondEngagementRange)
    );
    assert_eq!(attacker.distance_m, 2_000.0);
}

/// The observer is never its own target.
#[test]
fn accept_f32_a_observer_is_never_its_own_target() {
    let planner = synthetic_combat_planner();
    let assignment = escort_assignment();
    let candidates = vec![
        synthetic_candidate(1, [0.0, 0.0, 0.0], Some(Allegiance::Hostile), true),
        synthetic_candidate(
            BYSTANDER,
            [300.0, 0.0, 0.0],
            Some(Allegiance::Hostile),
            false,
        ),
    ];
    let decision = decide(&planner, &assignment, &candidates);
    assert_eq!(decision.target, Some(synthetic_actor(BYSTANDER)));
    assert_eq!(
        decision
            .trace
            .candidate(synthetic_actor(1))
            .expect("the observer is in the trace")
            .verdict,
        CandidateVerdict::Rejected(RejectReason::ObserverItself),
        "not even the objective flag makes the observer its own target"
    );
}

/// The threat evidence is the damage system's own event id, and a charge the
/// damage resolver really destroyed is absent from the candidate set: the
/// planner selects only what perception reported and never invents an actor,
/// so a destroyed actor cannot be "remembered" as a target.
///
/// This drives [`cs_sim::damage::DamageResolver`] for the destruction and
/// hands the planner the very [`HitEventId`] the resolver stamped, so the
/// "authoritative event" claim is checked against the producer rather than
/// against a hand-built id.
#[test]
fn accept_f32_a_a_real_damage_event_destroys_the_charge_and_its_id_is_the_threat_evidence() {
    use cs_sim::damage::{
        AttributionRule, DamageChannel, DamageEventKind, DamageNodeKey, DamagePolicy,
        DamageResolver, HitEvent, LifecycleKind, SYNTHETIC_HULL_INTEGRITY, SYNTHETIC_HULL_NODE,
        synthetic_airframe_graph,
    };

    let planner = synthetic_combat_planner();
    let assignment = escort_assignment();

    let mut resolver = DamageResolver::new(session(SYNTHETIC_SESSION), 2);
    resolver
        .register_actor(
            synthetic_actor(CHARGE),
            synthetic_airframe_graph(),
            DamagePolicy {
                attribution: AttributionRule::FirstLethalHit,
            },
        )
        .expect("the charge is a synthetic-session actor");
    let hit = HitEvent::try_new(
        HitEventId {
            session: session(SYNTHETIC_SESSION),
            tick: Tick(AC01_ATTACK_TICK),
            producer: 2,
            sequence: 0,
        },
        Some(synthetic_actor(ATTACKER)),
        synthetic_actor(CHARGE),
        DamageNodeKey::new(SYNTHETIC_HULL_NODE).expect("the fixture node key is valid"),
        DamageChannel::Internal,
        SYNTHETIC_HULL_INTEGRITY + 1.0,
    )
    .expect("the hit is well-formed");
    let hit_id = hit.id;
    let resolution = resolver
        .resolve(Tick(AC01_ATTACK_TICK), std::slice::from_ref(&hit))
        .expect("the batch resolves");
    assert!(
        resolution.events.iter().any(|event| {
            matches!(
                event.kind,
                DamageEventKind::Lifecycle {
                    actor,
                    kind: LifecycleKind::Destroyed,
                } if actor == synthetic_actor(CHARGE)
            )
        }),
        "the hit really destroyed the charge"
    );
    assert!(resolver.is_destroyed(&synthetic_actor(CHARGE)));

    // Perception reports the attacker but not the destroyed charge, and the
    // evidence the escort reasons about is the resolver's own event id.
    let candidates = vec![
        synthetic_candidate(
            ATTACKER,
            [800.0, 0.0, 0.0],
            Some(Allegiance::Hostile),
            false,
        )
        .with_threat(cs_sim::ai::combat::ThreatEvidence::new(
            synthetic_actor(ATTACKER),
            synthetic_actor(CHARGE),
            Tick(AC01_ATTACK_TICK),
            hit_id,
        )),
    ];
    let decision = decide(&planner, &assignment, &candidates);
    assert_eq!(
        decision.target,
        Some(synthetic_actor(ATTACKER)),
        "a resolver-stamped event id is authoritative evidence like any other"
    );
    assert_eq!(
        decision
            .trace
            .candidate(synthetic_actor(ATTACKER))
            .expect("the attacker is in the trace")
            .term(PriorityTerm::ProtectedActorThreat)
            .expect("the term was scored")
            .contribution,
        2.0
    );
    assert!(
        !decision
            .trace
            .candidates
            .iter()
            .any(|trace| trace.actor == synthetic_actor(CHARGE)),
        "the planner can only target what perception reported, so a destroyed \
         charge that perception dropped is unreachable"
    );
    assert_eq!(
        decision.trace.candidates.len(),
        1,
        "no candidate is invented from the assignment's protected actor"
    );
}

/// An empty or fully refused candidate set holds rather than guessing, and
/// the trace says why.
#[test]
fn accept_f32_a_no_eligible_candidate_holds_with_a_reason() {
    let planner = synthetic_combat_planner();
    let assignment = escort_assignment();
    for candidates in [
        Vec::new(),
        vec![synthetic_candidate(
            FRIEND,
            [150.0, 0.0, 0.0],
            Some(Allegiance::Friendly),
            false,
        )],
    ] {
        let decision = decide(&planner, &assignment, &candidates);
        assert_eq!(decision.target, None);
        assert_eq!(
            decision.trace.hold,
            Some(cs_sim::ai::combat::HoldReason::NoEligibleCandidate)
        );
        assert!(decision.trace.eligible().next().is_none());
    }
}

/// The arsenal snapshot reaches the trace, and an observer with no usable
/// gun and no ready launcher carries the separate arsenal veto. This is the
/// typed input F32-B's firing solution (AC02) reads; F32-A only reports it.
#[test]
fn accept_f32_a_arsenal_snapshot_reports_separate_availability_counts() {
    use cs_sim::ai::combat::{ArsenalSnapshot, FireVeto, MountAvailability, MountKind};
    use cs_sim::damage::DamageNodeKey;

    let planner = synthetic_combat_planner();
    let assignment = escort_assignment();
    let candidates = candidates();
    let armed = synthetic_arsenal();
    let decision = planner
        .decide(&CombatRequest {
            observer: synthetic_actor(1),
            now: Tick(DECISION_TICK),
            observer_position: position(0.0),
            assignment: &assignment,
            formation: None,
            protected_alive: Some(true),
            candidates: &candidates,
            arsenal: Some(&armed),
            profile: None,
        })
        .expect("the request is well-formed");
    let report = decision.trace.arsenal.expect("the report is carried");
    assert_eq!(report.total_mounts, 3);
    assert_eq!(report.usable_guns, 2);
    assert_eq!(report.ready_ordnance, 1);
    assert_eq!(report.disabled_mounts, 0);
    assert_eq!(report.empty_mounts, 0);

    // A disabled gun and an empty rack are two different reasons not to
    // fire, and neither is visible to the planner as anything else.
    let mut stripped = MountAvailability::usable(
        DamageNodeKey::new("gun_mount_1").expect("valid mount key"),
        MountKind::Gun,
        0,
    );
    stripped.disabled = true;
    let disarmed = ArsenalSnapshot::try_new(vec![
        stripped,
        MountAvailability::usable(
            DamageNodeKey::new("gun_mount_2").expect("valid mount key"),
            MountKind::Gun,
            120,
        ),
        MountAvailability::usable(
            DamageNodeKey::new("ordnance_mount_1").expect("valid mount key"),
            MountKind::Ordnance,
            0,
        ),
    ])
    .expect("the stripped arsenal is valid");
    let decision = planner
        .decide(&CombatRequest {
            observer: synthetic_actor(1),
            now: Tick(DECISION_TICK),
            observer_position: position(0.0),
            assignment: &assignment,
            formation: None,
            protected_alive: Some(true),
            candidates: &candidates,
            arsenal: Some(&disarmed),
            profile: None,
        })
        .expect("the request is well-formed");
    let report = decision.trace.arsenal.expect("the report is carried");
    assert_eq!(report.usable_guns, 1);
    assert_eq!(report.ready_ordnance, 0);
    assert_eq!(report.disabled_mounts, 1);
    assert_eq!(report.empty_mounts, 1);
    assert_eq!(
        decision
            .trace
            .candidate(synthetic_actor(ATTACKER))
            .expect("the attacker is in the trace")
            .fire_veto,
        None,
        "one usable gun remains, so the arsenal is not yet unusable"
    );
    assert_eq!(
        decision
            .trace
            .candidate(synthetic_actor(BYSTANDER))
            .expect("the bystander is in the trace")
            .fire_veto,
        None
    );
    assert!(
        synthetic_fighter_profile().arsenal().gun,
        "the fixture fighter profile declares guns"
    );

    // When nothing at all is left the veto is raised on the target, and it
    // names the counts that produced it. AC02's firing solution is F32-B's
    // work, so this is the vocabulary the solution will read: the target is
    // still the target, and the veto says why a shot may not follow.
    let mut dead_gun = MountAvailability::usable(
        DamageNodeKey::new("gun_mount_2").expect("valid mount key"),
        MountKind::Gun,
        120,
    );
    dead_gun.disabled = true;
    let disarmed = ArsenalSnapshot::try_new(vec![
        MountAvailability::usable(
            DamageNodeKey::new("gun_mount_1").expect("valid mount key"),
            MountKind::Gun,
            0,
        ),
        dead_gun,
        MountAvailability::usable(
            DamageNodeKey::new("ordnance_mount_1").expect("valid mount key"),
            MountKind::Ordnance,
            0,
        ),
    ])
    .expect("the disarmed arsenal is valid");
    let decision = planner
        .decide(&CombatRequest {
            observer: synthetic_actor(1),
            now: Tick(DECISION_TICK),
            observer_position: position(0.0),
            assignment: &assignment,
            formation: None,
            protected_alive: Some(true),
            candidates: &candidates,
            arsenal: Some(&disarmed),
            profile: None,
        })
        .expect("the request is well-formed");
    assert_eq!(
        decision.target,
        Some(synthetic_actor(ATTACKER)),
        "an empty arsenal does not change who the target is"
    );
    let report = decision.trace.arsenal.expect("the report is carried");
    assert_eq!(report.disabled_mounts, 1);
    assert_eq!(report.empty_mounts, 2);
    for serial in [ATTACKER, BYSTANDER] {
        assert_eq!(
            decision
                .trace
                .candidate(synthetic_actor(serial))
                .expect("the candidate is in the trace")
                .fire_veto,
            Some(FireVeto::ArsenalUnusable {
                usable_guns: 0,
                ready_ordnance: 0,
            }),
            "the arsenal veto is reported on every candidate while nothing can be fired"
        );
    }
}

/// A profile whose every weight is zero is refused: it would score no
/// candidate and make the decision an artifact of the tie-break.
#[test]
fn accept_f32_a_profile_with_no_scored_term_is_refused() {
    use cs_sim::ai::combat::{PriorityPolicy, RoleArsenal, SkillKnobs, SkillProfile};

    let unscored = SkillProfile::try_new(
        CombatRole::Escort,
        RoleArsenal::guns(),
        SkillKnobs {
            reaction_ticks: 24,
            aim_error_rad: 0.06,
            engagement_range_m: 1_500.0,
            fire_discipline_ticks: 30,
        },
        PriorityPolicy {
            protected_actor_weight: 0.0,
            objective_weight: 0.0,
            self_defense_weight: 0.0,
            proximity_weight: 0.0,
            threat_window_ticks: 120,
        },
    )
    .expect_err("an all-zero policy scores nothing");
    assert!(matches!(unscored, CombatError::NoScoredPriorityTerm));
}
