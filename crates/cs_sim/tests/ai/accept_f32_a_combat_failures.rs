//! F32-A acceptance tests for the combat-AI refusal paths: what the
//! planner and the profile/formation records must *refuse* rather than
//! decide on.
//!
//! Spec: `specs/F32-ai-combat-formations-aces-and-difficulty.md`, stage
//! `### F32-A`. Task test prefix: `accept_f32_a_`.
//!
//! Every test drives production code: `cs_sim::ai::combat`'s validators and
//! [`CombatPlanner::decide`]. Each names the error it expects, so removing
//! a guard fails the test that depends on it.

use cs_sim::ai::combat::{
    ArsenalSnapshot, CandidateView, CombatError, CombatPlanner, CombatRequest, CombatRole,
    FormationFacts, FormationId, FormationSlot, MAX_ENGAGEMENT_RANGE_M, MountAvailability,
    MountKind, PriorityPolicy, RoleArsenal, RoleAssignment, SYNTHETIC_SESSION, SkillKnobs,
    SkillProfile, synthetic_actor, synthetic_arsenal, synthetic_candidate,
    synthetic_combat_planner, synthetic_escort_profile, synthetic_recovery_policies,
    synthetic_threat,
};
use cs_sim::damage::{ActorId, DamageNodeKey, HitEventId};
use cs_sim::targeting::Allegiance;
use cs_types::Tick;
use cs_types::space::WorldPosition;

const CHARGE: u64 = 10;
const ATTACKER: u64 = 2;
const BYSTANDER: u64 = 5;

fn position(x: f64) -> WorldPosition {
    WorldPosition::try_new([x, 0.0, 0.0]).expect("the fixture position is finite")
}

fn escort_assignment() -> RoleAssignment {
    RoleAssignment::protecting(
        synthetic_actor(1),
        CombatRole::Escort,
        synthetic_actor(CHARGE),
    )
    .expect("the observer is not its own protected actor")
}

fn candidates() -> Vec<cs_sim::ai::combat::CandidateView> {
    vec![
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
        .with_threat(synthetic_threat(ATTACKER, CHARGE, Tick(3_900), 2, 0)),
    ]
}

fn request<'a>(
    observer: ActorId,
    assignment: &'a RoleAssignment,
    candidates: &'a [cs_sim::ai::combat::CandidateView],
    formation: Option<&'a FormationFacts>,
    arsenal: &'a ArsenalSnapshot,
) -> CombatRequest<'a> {
    CombatRequest {
        observer,
        now: Tick(4_000),
        observer_position: position(0.0),
        assignment,
        formation,
        protected_alive: Some(true),
        candidates,
        arsenal: Some(arsenal),
        profile: None,
    }
}

/// The synthetic arsenal, for callers that do not need their own.
fn arsenal() -> ArsenalSnapshot {
    synthetic_arsenal()
}

/// An actor from another session generation is refused by name: the
/// planner is per-session state, and a stale-generation identity must
/// never be decided on (STATE-TRANSACTIONS).
#[test]
fn accept_f32_a_foreign_session_identities_are_refused() {
    let planner = synthetic_combat_planner();
    let assignment = escort_assignment();
    let candidates = candidates();
    let foreign = ActorId {
        session: SYNTHETIC_SESSION + 1,
        serial: 1,
    };

    let err = planner
        .decide(&request(
            foreign,
            &assignment,
            &candidates,
            None,
            &arsenal(),
        ))
        .expect_err("a foreign observer is refused");
    assert!(
        matches!(err, CombatError::ForeignSession { .. }),
        "expected a session refusal, got {err}"
    );

    // A candidate from another session generation is refused too, even
    // behind a valid observer and with valid-looking evidence.
    let foreign_candidate = ActorId {
        session: SYNTHETIC_SESSION + 1,
        serial: 9,
    };
    let mixed = vec![
        candidates[0].clone(),
        CandidateView::new(
            foreign_candidate,
            position(700.0),
            Some(Allegiance::Hostile),
            false,
        )
        .with_threat(ThreatEvidenceForeign::build()),
    ];
    let err = planner
        .decide(&request(
            synthetic_actor(1),
            &assignment,
            &mixed,
            None,
            &arsenal(),
        ))
        .expect_err("a foreign candidate is refused");
    match err {
        CombatError::ForeignSession { actor, session } => {
            assert_eq!(actor, foreign_candidate);
            assert_eq!(session, SYNTHETIC_SESSION);
        }
        other => panic!("expected a session refusal for the candidate, got {other}"),
    }

    // A threat event from a stale generation cannot mint a threat either.
    let stale_event = vec![
        candidates[0].clone(),
        synthetic_candidate(
            ATTACKER,
            [800.0, 0.0, 0.0],
            Some(Allegiance::Hostile),
            false,
        )
        .with_threat(cs_sim::ai::combat::ThreatEvidence::new(
            synthetic_actor(ATTACKER),
            synthetic_actor(CHARGE),
            Tick(3_990),
            HitEventId {
                session: SYNTHETIC_SESSION + 5,
                tick: Tick(3_990),
                producer: 2,
                sequence: 0,
            },
        )),
    ];
    let err = planner
        .decide(&request(
            synthetic_actor(1),
            &assignment,
            &stale_event,
            None,
            &arsenal(),
        ))
        .expect_err("a stale-generation attack event is refused");
    assert!(matches!(err, CombatError::ForeignSession { .. }), "{err}");
}

/// A helper namespace so the mixed-session candidate above reads clearly.
struct ThreatEvidenceForeign;

impl ThreatEvidenceForeign {
    /// Evidence that is internally consistent — attacker, victim and event
    /// all agree — so the only thing wrong with the candidate that carries
    /// it is the session generation of that candidate.
    fn build() -> cs_sim::ai::combat::ThreatEvidence {
        cs_sim::ai::combat::ThreatEvidence::new(
            ActorId {
                session: SYNTHETIC_SESSION + 1,
                serial: 9,
            },
            synthetic_actor(CHARGE),
            Tick(3_900),
            HitEventId {
                session: SYNTHETIC_SESSION + 1,
                tick: Tick(3_900),
                producer: 2,
                sequence: 0,
            },
        )
    }
}

/// A candidate's threat evidence must name that candidate as the attacker:
/// evidence attached to the wrong actor is a producer bug and is refused
/// rather than credited to whoever it was found on.
#[test]
fn accept_f32_a_threat_evidence_naming_another_attacker_is_refused() {
    let planner = synthetic_combat_planner();
    let assignment = escort_assignment();
    let misattributed = vec![
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
        .with_threat(synthetic_threat(BYSTANDER, CHARGE, Tick(3_990), 2, 0)),
    ];
    let err = planner
        .decide(&request(
            synthetic_actor(1),
            &assignment,
            &misattributed,
            None,
            &arsenal(),
        ))
        .expect_err("evidence naming another attacker is refused");
    match err {
        CombatError::ThreatAttackerMismatch {
            candidate,
            attacker,
        } => {
            assert_eq!(candidate, synthetic_actor(ATTACKER));
            assert_eq!(attacker, synthetic_actor(BYSTANDER));
        }
        other => panic!("expected a misattribution refusal, got {other}"),
    }
}

/// An attack stamped on a later tick than the decision is refused: a
/// decision cannot be justified by evidence from its own future.
#[test]
fn accept_f32_a_threat_evidence_from_the_future_is_refused() {
    let planner = synthetic_combat_planner();
    let assignment = escort_assignment();
    let from_the_future = vec![
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
        .with_threat(synthetic_threat(ATTACKER, CHARGE, Tick(4_001), 2, 0)),
    ];
    let err = planner
        .decide(&request(
            synthetic_actor(1),
            &assignment,
            &from_the_future,
            None,
            &arsenal(),
        ))
        .expect_err("evidence from the future is refused");
    match err {
        CombatError::ThreatFromTheFuture { attacker, at, now } => {
            assert_eq!(attacker, synthetic_actor(ATTACKER));
            assert_eq!(at, Tick(4_001));
            assert_eq!(now, Tick(4_000));
        }
        other => panic!("expected a future-evidence refusal, got {other}"),
    }
}

/// A request whose role assignment belongs to another actor is refused: the
/// decision's trace would otherwise name an observer that did not decide.
#[test]
fn accept_f32_a_assignment_for_another_actor_is_refused() {
    let planner = synthetic_combat_planner();
    let assignment = RoleAssignment::new(synthetic_actor(42), CombatRole::FighterAttack);
    let candidates = candidates();
    let err = planner
        .decide(&request(
            synthetic_actor(1),
            &assignment,
            &candidates,
            None,
            &arsenal(),
        ))
        .expect_err("another actor's assignment is refused");
    assert!(
        matches!(err, CombatError::AssignmentObserverMismatch { .. }),
        "{err}"
    );
}

/// A role with no registered profile has no approved policy, so the
/// planner refuses instead of running the role on invented numbers.
#[test]
fn accept_f32_a_role_without_a_profile_is_refused() {
    let planner = CombatPlanner::new(SYNTHETIC_SESSION, &[synthetic_escort_profile()])
        .expect("one profile registers");
    let assignment = RoleAssignment::new(synthetic_actor(1), CombatRole::Intercept);
    let candidates = candidates();
    let err = planner
        .decide(&request(
            synthetic_actor(1),
            &assignment,
            &candidates,
            None,
            &arsenal(),
        ))
        .expect_err("no profile for the role");
    assert!(matches!(err, CombatError::NoProfileForRole { .. }), "{err}");
}

/// Two profiles for one role are refused at construction: resolving the
/// ambiguity by iteration order would make a decision depend on the order
/// the ECS handed the profiles over.
#[test]
fn accept_f32_a_duplicate_role_profile_is_refused() {
    let err = CombatPlanner::new(
        SYNTHETIC_SESSION,
        &[synthetic_escort_profile(), synthetic_escort_profile()],
    )
    .expect_err("two profiles for one role");
    assert!(
        matches!(err, CombatError::DuplicateRoleProfile { .. }),
        "{err}"
    );
}

/// Registering a formation twice is refused.
#[test]
fn accept_f32_a_duplicate_formation_registration_is_refused() {
    let err = synthetic_combat_planner()
        .with_formation(FormationId(1), synthetic_recovery_policies())
        .expect_err("formation 1 is already registered");
    assert!(
        matches!(err, CombatError::DuplicateFormation { .. }),
        "{err}"
    );
}

/// A pending recovery trigger for a formation the planner carries no
/// declared recovery paths for is refused: the decision must not silently
/// drop a declared recovery case.
#[test]
fn accept_f32_a_pending_recovery_without_declared_policies_is_refused() {
    // A planner with no formations registered at all.
    let planner = CombatPlanner::new(SYNTHETIC_SESSION, &[synthetic_escort_profile()])
        .expect("one profile registers");
    let assignment = escort_assignment().in_formation(FormationSlot {
        formation: FormationId(1),
        slot: 1,
    });
    let facts = FormationFacts {
        formation: FormationId(1),
        leader: synthetic_actor(2),
        leader_alive: false,
        observer_is_leader: false,
        assigned_target: None,
        assigned_target_alive: true,
        route_available: true,
    };
    let candidates = candidates();
    let err = planner
        .decide(&request(
            synthetic_actor(1),
            &assignment,
            &candidates,
            Some(&facts),
            &arsenal(),
        ))
        .expect_err("no declared recovery paths for the pending trigger");
    assert!(matches!(err, CombatError::NoRecoveryPolicy { .. }), "{err}");
}

/// An assignment that protects the assigned actor is refused: "escort"
/// with a self-charge is a content bug, and collapsing it into
/// self-defense would silently change the role.
#[test]
fn accept_f32_a_self_protecting_assignment_is_refused() {
    let err =
        RoleAssignment::protecting(synthetic_actor(1), CombatRole::Escort, synthetic_actor(1))
            .expect_err("an actor cannot protect itself");
    assert!(matches!(err, CombatError::ProtectedIsSelf { .. }), "{err}");
}

/// Every profile knob is bounded and finite, and a bomber or torpedo run
/// must declare ordnance. A corrupt or mis-scaled value is refused by name
/// rather than clamped.
#[test]
fn accept_f32_a_profile_knobs_are_bounded_and_finite() {
    let base_knobs = SkillKnobs {
        reaction_ticks: 24,
        aim_error_rad: 0.06,
        engagement_range_m: 1_500.0,
        fire_discipline_ticks: 30,
    };
    let base_policy = PriorityPolicy {
        protected_actor_weight: 2.0,
        objective_weight: 1.0,
        self_defense_weight: 1.5,
        proximity_weight: 0.5,
        threat_window_ticks: 120,
    };

    /// One bounded-knob case: the knobs, the policy, and a predicate naming
    /// the error the validator must produce for them.
    type KnobCase = (SkillKnobs, PriorityPolicy, fn(&CombatError) -> bool);

    let cases: Vec<KnobCase> = vec![
        (
            SkillKnobs {
                aim_error_rad: f64::NAN,
                ..base_knobs
            },
            base_policy,
            |err| {
                matches!(
                    err,
                    CombatError::NonFiniteKnob {
                        knob: "aim_error_rad"
                    }
                )
            },
        ),
        (
            SkillKnobs {
                engagement_range_m: f64::INFINITY,
                ..base_knobs
            },
            base_policy,
            |err| {
                matches!(
                    err,
                    CombatError::NonFiniteKnob {
                        knob: "engagement_range_m"
                    }
                )
            },
        ),
        (
            SkillKnobs {
                aim_error_rad: 10.0,
                ..base_knobs
            },
            base_policy,
            |err| {
                matches!(
                    err,
                    CombatError::KnobOutOfRange {
                        knob: "aim_error_rad",
                        ..
                    }
                )
            },
        ),
        (
            SkillKnobs {
                engagement_range_m: MAX_ENGAGEMENT_RANGE_M * 2.0,
                ..base_knobs
            },
            base_policy,
            |err| {
                matches!(
                    err,
                    CombatError::KnobOutOfRange {
                        knob: "engagement_range_m",
                        ..
                    }
                )
            },
        ),
        (
            SkillKnobs {
                engagement_range_m: 0.0,
                ..base_knobs
            },
            base_policy,
            |err| {
                matches!(
                    err,
                    CombatError::KnobOutOfRange {
                        knob: "engagement_range_m",
                        ..
                    }
                )
            },
        ),
        (
            SkillKnobs {
                reaction_ticks: 10_000,
                ..base_knobs
            },
            base_policy,
            |err| {
                matches!(
                    err,
                    CombatError::KnobOutOfRange {
                        knob: "reaction_ticks",
                        ..
                    }
                )
            },
        ),
        (
            base_knobs,
            PriorityPolicy {
                threat_window_ticks: 99_999,
                ..base_policy
            },
            |err| {
                matches!(
                    err,
                    CombatError::KnobOutOfRange {
                        knob: "threat_window_ticks",
                        ..
                    }
                )
            },
        ),
        (
            base_knobs,
            PriorityPolicy {
                proximity_weight: -1.0,
                ..base_policy
            },
            |err| {
                matches!(
                    err,
                    CombatError::KnobOutOfRange {
                        knob: "proximity_weight",
                        ..
                    }
                )
            },
        ),
    ];

    for (knobs, policy, expected) in cases {
        let err = SkillProfile::try_new(CombatRole::Escort, RoleArsenal::guns(), knobs, policy)
            .expect_err("an out-of-bounds knob is refused");
        assert!(expected(&err), "unexpected error for {knobs:?}: {err}");
    }

    // A torpedo run declared without ordnance could never be executed.
    let err = SkillProfile::try_new(
        CombatRole::TorpedoRun,
        RoleArsenal::guns(),
        base_knobs,
        base_policy,
    )
    .expect_err("a torpedo run needs ordnance");
    assert!(
        matches!(err, CombatError::RoleArsenalMissing { .. }),
        "{err}"
    );
    assert!(
        SkillProfile::try_new(
            CombatRole::TorpedoRun,
            RoleArsenal::guns_and_ordnance(),
            base_knobs,
            base_policy
        )
        .is_ok()
    );
}

/// The same rule on the bumping side: a mount that is intact but empty and
/// a mount whose damage node was destroyed are two distinct, counted
/// states, and a snapshot cannot list one mount twice.
#[test]
fn accept_f32_a_arsenal_snapshot_refuses_a_duplicated_mount() {
    let mount = DamageNodeKey::new("gun_mount_1").expect("valid mount key");
    let err = ArsenalSnapshot::try_new(vec![
        MountAvailability::usable(mount.clone(), MountKind::Gun, 200),
        MountAvailability::usable(mount.clone(), MountKind::Gun, 10),
    ])
    .expect_err("one mount cannot be listed twice");
    assert!(matches!(err, CombatError::DuplicateMount { .. }), "{err}");

    let snapshot = ArsenalSnapshot::try_new(vec![MountAvailability::usable(
        mount.clone(),
        MountKind::Gun,
        1,
    )])
    .expect("one mount is valid");
    assert!(snapshot.mounts()[0].is_usable());
    let report = snapshot.report();
    assert_eq!((report.usable_guns, report.ready_ordnance), (1, 0));
    assert!(ArsenalSnapshot::empty().report().total_mounts == 0);
}

/// The planner is per-session state, and a formation's member identities
/// from another session are refused with the same rule as any other actor.
#[test]
fn accept_f32_a_foreign_formation_leader_is_refused() {
    let planner = synthetic_combat_planner();
    // The observer is a member of formation 1, so its assignment and the
    // facts agree on which formation this is; only the leader's session is
    // wrong.
    let assignment = escort_assignment().in_formation(FormationSlot {
        formation: FormationId(1),
        slot: 1,
    });
    let facts = FormationFacts {
        formation: FormationId(1),
        leader: ActorId {
            session: SYNTHETIC_SESSION + 2,
            serial: 0,
        },
        leader_alive: false,
        observer_is_leader: false,
        assigned_target: None,
        assigned_target_alive: true,
        route_available: true,
    };
    let candidates = candidates();
    let err = planner
        .decide(&request(
            synthetic_actor(1),
            &assignment,
            &candidates,
            Some(&facts),
            &arsenal(),
        ))
        .expect_err("a foreign leader is refused");
    assert!(matches!(err, CombatError::ForeignSession { .. }), "{err}");

    // The formation's assigned target is an actor identity too, and a
    // stale generation in that slot is refused the same way.
    let foreign_target = FormationFacts {
        leader: synthetic_actor(2),
        assigned_target: Some(ActorId {
            session: SYNTHETIC_SESSION + 3,
            serial: 8,
        }),
        assigned_target_alive: true,
        ..facts
    };
    let err = planner
        .decide(&request(
            synthetic_actor(1),
            &assignment,
            &candidates,
            Some(&foreign_target),
            &arsenal(),
        ))
        .expect_err("a foreign assigned target is refused");
    match err {
        CombatError::ForeignSession { actor, session } => {
            assert_eq!(actor.session, SYNTHETIC_SESSION + 3);
            assert_eq!(session, SYNTHETIC_SESSION);
        }
        other => panic!("expected a session refusal for the assigned target, got {other}"),
    }
}

/// A protected actor from another session generation is refused on the
/// identity alone. The check does not wait for a lifecycle report: a
/// request that omits `protected_alive` must not be able to carry a stale
/// generation into a decision.
#[test]
fn accept_f32_a_foreign_protected_actor_is_refused_without_a_lifecycle_report() {
    let planner = synthetic_combat_planner();
    let candidates = candidates();
    let foreign_charge = ActorId {
        session: SYNTHETIC_SESSION + 4,
        serial: CHARGE,
    };
    let assignment =
        RoleAssignment::protecting(synthetic_actor(1), CombatRole::Escort, foreign_charge)
            .expect("the protected actor is not the observer itself");

    for protected_alive in [None, Some(true), Some(false)] {
        let err = planner
            .decide(&CombatRequest {
                observer: synthetic_actor(1),
                now: Tick(4_000),
                observer_position: position(0.0),
                assignment: &assignment,
                formation: None,
                protected_alive,
                candidates: &candidates,
                arsenal: Some(&arsenal()),
                profile: None,
            })
            .expect_err("a foreign protected actor is refused");
        match err {
            CombatError::ForeignSession { actor, session } => {
                assert_eq!(actor, foreign_charge);
                assert_eq!(session, SYNTHETIC_SESSION);
            }
            other => panic!("expected a session refusal, got {other}"),
        }
    }
}

/// The assignment and the formation facts are two statements about which
/// formation the observer is in. A request that makes both, and disagrees,
/// is refused: otherwise the recovery path would be resolved from whichever
/// formation the caller happened to fill in.
#[test]
fn accept_f32_a_formation_facts_that_contradict_the_assignment_are_refused() {
    let planner = synthetic_combat_planner();
    let candidates = candidates();
    let facts = FormationFacts {
        formation: FormationId(1),
        leader: synthetic_actor(2),
        leader_alive: true,
        observer_is_leader: false,
        assigned_target: None,
        assigned_target_alive: true,
        route_available: true,
    };

    // The assignment places the observer in formation 2 while the facts
    // describe formation 1.
    let wrong_formation = escort_assignment().in_formation(FormationSlot {
        formation: FormationId(2),
        slot: 0,
    });
    let err = planner
        .decide(&request(
            synthetic_actor(1),
            &wrong_formation,
            &candidates,
            Some(&facts),
            &arsenal(),
        ))
        .expect_err("two formations at once is refused");
    assert_eq!(
        err,
        CombatError::FormationAssignmentMismatch {
            assigned: Some(FormationId(2)),
            facts: FormationId(1),
        }
    );

    // Facts for a formation the assignment does not place the observer in
    // are the same contradiction in the other direction.
    let err = planner
        .decide(&request(
            synthetic_actor(1),
            &escort_assignment(),
            &candidates,
            Some(&facts),
            &arsenal(),
        ))
        .expect_err("facts for a formation the actor does not hold a slot in are refused");
    assert_eq!(
        err,
        CombatError::FormationAssignmentMismatch {
            assigned: None,
            facts: FormationId(1),
        }
    );
    assert!(
        err.to_string().contains("no formation at all"),
        "the refusal names what the assignment declared: {err}"
    );
}

/// A destroyed leader is not its own recovery case: the observer that *is*
/// the leader reports no leader-loss trigger, while a follower does.
#[test]
fn accept_f32_a_destroyed_leader_is_not_its_own_recovery_case() {
    let facts = FormationFacts {
        formation: FormationId(1),
        leader: synthetic_actor(1),
        leader_alive: false,
        observer_is_leader: true,
        assigned_target: None,
        assigned_target_alive: true,
        route_available: true,
    };
    assert_eq!(facts.pending_trigger(), None);

    let follower = FormationFacts {
        observer_is_leader: false,
        ..facts
    };
    assert_eq!(
        follower.pending_trigger(),
        Some(cs_sim::ai::combat::RecoveryTrigger::LeaderLost)
    );
    // A live leader with a live route raises nothing.
    let healthy = FormationFacts {
        leader_alive: true,
        route_available: true,
        ..follower
    };
    assert_eq!(healthy.pending_trigger(), None);
}
