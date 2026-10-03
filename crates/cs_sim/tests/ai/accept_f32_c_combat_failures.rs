//! The refusal half of F32-C: what the combat runtime and the formation
//! coordinator refuse, and the guarantee that a refused tick changes nothing.
//!
//! Spec: `specs/F32-ai-combat-formations-aces-and-difficulty.md`, stage
//! `### F32-C`. Task test prefix: `accept_f32_c_`.
//!
//! These tests drive production code only: `cs_sim::ai::combat`'s
//! [`CombatRuntime`] and [`FormationCoordinator`]. Every case here is a
//! refusal with a name, a reason and no state change, because a recovery that
//! half-applied itself is worse than one that refused.

use cs_sim::ai::combat::{
    AceId, AceVariant, CombatError, CombatRole, CombatRuntime, CombatantRequest, DifficultyRoster,
    DifficultyTier, FormationId, FormationMemberReport, FormationRoster, FormationRosterMember,
    FormationTick, RecoveryPolicySet, RoleArsenal, RoleAssignment, SYNTHETIC_FORMATION_LEADER,
    SYNTHETIC_FORMATION_WINGMEN, SYNTHETIC_SESSION, SkillKnobs, SkillProfile, synthetic_ace_id,
    synthetic_ace_variant, synthetic_actor, synthetic_arsenal, synthetic_candidate,
    synthetic_combat_runtime, synthetic_difficulty_roster, synthetic_escort_profile,
    synthetic_fighter_profile, synthetic_formation_roster, synthetic_member_report,
    synthetic_recovery_policies,
};
use cs_sim::damage::ActorId;
use cs_sim::targeting::Allegiance;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

/// The formation the fixture declares.
const FORMATION: FormationId = FormationId(1);

/// The tick the fixture starts on.
const START: u64 = 4_000;

fn intact<'a>(now: u64, members: &'a [FormationMemberReport]) -> FormationTick<'a> {
    FormationTick {
        formation: FORMATION,
        now: Tick(now),
        members,
        assigned_target: None,
        assigned_target_alive: true,
        route_available: true,
    }
}

/// One intact formation report.
fn intact_members() -> Vec<FormationMemberReport> {
    vec![
        synthetic_member_report(0, SYNTHETIC_FORMATION_LEADER, [1_000.0, 0.0, 0.0], true),
        synthetic_member_report(1, SYNTHETIC_FORMATION_WINGMEN[0], [800.0, 0.0, 0.0], true),
        synthetic_member_report(2, SYNTHETIC_FORMATION_WINGMEN[1], [700.0, 0.0, 0.0], true),
    ]
}

fn foreign(serial: u64) -> ActorId {
    ActorId {
        session: SessionId::new(SYNTHETIC_SESSION + 1).expect("a nonzero session"),
        serial,
    }
}

/// A refused formation tick leaves the coordinator exactly as it was, and the
/// corrected report for the *same* tick then applies: the retry the contract
/// promises. A runtime that committed half a tick would make the retry produce
/// a different formation than the one the caller thought it was correcting.
#[test]
fn accept_f32_c_a_refused_formation_tick_changes_nothing_and_is_retryable() {
    let mut runtime = synthetic_combat_runtime();
    let members = intact_members();
    runtime
        .update_formation(&intact(START, &members))
        .expect("the first tick applies");

    // A report that names the wrong actor in a slot is refused.
    let mut wrong_actor = members.clone();
    wrong_actor[1].actor = synthetic_actor(99);
    let err = runtime
        .update_formation(&intact(START + 1, &wrong_actor))
        .expect_err("a stranger in a slot is refused");
    assert_eq!(
        err,
        CombatError::FormationMemberMismatch {
            formation: FORMATION,
            slot: 1,
            registered: synthetic_actor(SYNTHETIC_FORMATION_WINGMEN[0]),
            reported: synthetic_actor(99),
        }
    );

    // Nothing moved: the leadership is untouched and the tick did not advance.
    let facts = runtime
        .facts(FORMATION, synthetic_actor(SYNTHETIC_FORMATION_WINGMEN[0]))
        .expect("the wingman is still a member");
    assert_eq!(facts.leader, synthetic_actor(SYNTHETIC_FORMATION_LEADER));
    assert!(facts.leader_alive);

    // The corrected report for the same tick applies and does recover.
    let loss = vec![
        synthetic_member_report(0, SYNTHETIC_FORMATION_LEADER, [1_000.0, 0.0, 0.0], false),
        synthetic_member_report(1, SYNTHETIC_FORMATION_WINGMEN[0], [810.0, 0.0, 0.0], true),
        synthetic_member_report(2, SYNTHETIC_FORMATION_WINGMEN[1], [710.0, 0.0, 0.0], true),
    ];
    let retry = runtime
        .update_formation(&intact(START + 1, &loss))
        .expect("the corrected report for the same tick applies");
    assert_eq!(
        retry.trigger,
        Some(cs_sim::ai::combat::RecoveryTrigger::LeaderLost)
    );
    assert_eq!(
        retry.leader,
        Some(synthetic_actor(SYNTHETIC_FORMATION_WINGMEN[0]))
    );
}

/// A tick that is not strictly newer than the last applied one is refused, so a
/// replayed or out-of-order report cannot bring a destroyed leader back or
/// elect a second one.
#[test]
fn accept_f32_c_a_replayed_or_backwards_formation_tick_is_refused() {
    let mut runtime = synthetic_combat_runtime();
    let members = intact_members();
    runtime
        .update_formation(&intact(START, &members))
        .expect("the first tick applies");

    assert_eq!(
        runtime
            .update_formation(&intact(START, &members))
            .expect_err("the same tick twice would elect twice"),
        CombatError::StaleFormationTick {
            formation: FORMATION,
            now: Tick(START),
            last: Tick(START),
        }
    );
    assert_eq!(
        runtime
            .update_formation(&intact(START - 1, &members))
            .expect_err("time does not run backwards"),
        CombatError::StaleFormationTick {
            formation: FORMATION,
            now: Tick(START - 1),
            last: Tick(START),
        }
    );
    // A formation the runtime does not hold is not one it can recover.
    assert_eq!(
        runtime
            .update_formation(&FormationTick {
                formation: FormationId(9),
                ..intact(START + 1, &members)
            })
            .expect_err("an unknown formation has no declared recovery"),
        CombatError::NoRecoveryPolicy {
            formation: FormationId(9)
        }
    );
}

/// The report must describe the whole formation. A partial report would make a
/// centroid a different point, so it is refused rather than reconciled.
#[test]
fn accept_f32_c_an_incomplete_or_confused_formation_report_is_refused() {
    let mut runtime = synthetic_combat_runtime();
    let members = intact_members();
    runtime
        .update_formation(&intact(START, &members))
        .expect("the first tick applies");

    let missing = vec![members[0], members[1]];
    assert_eq!(
        runtime
            .update_formation(&intact(START + 1, &missing))
            .expect_err("every registered member must be reported"),
        CombatError::FormationMemberNotReported {
            formation: FORMATION,
            slot: 2,
        }
    );

    let unknown_slot = vec![
        members[0],
        members[1],
        FormationMemberReport {
            slot: 7,
            ..members[2]
        },
    ];
    assert_eq!(
        runtime
            .update_formation(&intact(START + 1, &unknown_slot))
            .expect_err("a slot the formation does not have is refused"),
        CombatError::UnknownFormationSlot {
            formation: FORMATION,
            slot: 7,
        }
    );

    let twice = vec![members[0], members[1], members[1]];
    assert_eq!(
        runtime
            .update_formation(&intact(START + 1, &twice))
            .expect_err("a slot reported twice is ambiguous"),
        CombatError::FormationMemberReportedTwice {
            formation: FORMATION,
            slot: 1,
        }
    );

    // None of the four refusals advanced the tick, so a correct report still
    // applies afterwards.
    let next = runtime
        .update_formation(&intact(START + 1, &members))
        .expect("a correct report still applies after four refusals");
    assert_eq!(next.trigger, None);
}

/// A retired member leaves the formation for good. A stale report that brings
/// it back is refused rather than resurrecting it, because a resurrected
/// leader would be a leader the damage system has already destroyed.
#[test]
fn accept_f32_c_a_retired_member_is_not_brought_back_to_life() {
    let mut runtime = synthetic_combat_runtime();
    let loss = vec![
        synthetic_member_report(0, SYNTHETIC_FORMATION_LEADER, [1_000.0, 0.0, 0.0], false),
        synthetic_member_report(1, SYNTHETIC_FORMATION_WINGMEN[0], [810.0, 0.0, 0.0], true),
        synthetic_member_report(2, SYNTHETIC_FORMATION_WINGMEN[1], [710.0, 0.0, 0.0], true),
    ];
    runtime
        .update_formation(&intact(START, &loss))
        .expect("the leader-loss tick applies");

    let resurrected = vec![
        synthetic_member_report(0, SYNTHETIC_FORMATION_LEADER, [1_000.0, 0.0, 0.0], true),
        synthetic_member_report(1, SYNTHETIC_FORMATION_WINGMEN[0], [810.0, 0.0, 0.0], true),
        synthetic_member_report(2, SYNTHETIC_FORMATION_WINGMEN[1], [710.0, 0.0, 0.0], true),
    ];
    assert_eq!(
        runtime
            .update_formation(&intact(START + 1, &resurrected))
            .expect_err("a destroyed leader does not come back"),
        CombatError::RetiredFormationMember {
            formation: FORMATION,
            slot: 0,
            actor: synthetic_actor(SYNTHETIC_FORMATION_LEADER),
        }
    );
    // And the promotion it caused stands: the survivor still leads.
    assert!(
        runtime
            .facts(FORMATION, synthetic_actor(SYNTHETIC_FORMATION_WINGMEN[0]))
            .is_some_and(|facts| facts.leader == synthetic_actor(SYNTHETIC_FORMATION_WINGMEN[0]))
    );
}

/// A station that cannot be computed as a finite world point is refused by
/// name, and the formation is left exactly as it was so the tick can be retried
/// with coordinates that are actually a position.
#[test]
fn accept_f32_c_a_station_that_cannot_be_finite_is_refused_by_name() {
    let regrouping = RecoveryPolicySet {
        leader_loss: cs_sim::ai::combat::RecoveryAction::Regroup,
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

    // Two survivors at the largest finite coordinate: their sum overflows
    // `f64`, so the centroid would be an infinite point.
    let impossible = vec![
        synthetic_member_report(0, SYNTHETIC_FORMATION_LEADER, [1_000.0, 0.0, 0.0], false),
        synthetic_member_report(
            1,
            SYNTHETIC_FORMATION_WINGMEN[0],
            [f64::MAX, 0.0, 0.0],
            true,
        ),
        synthetic_member_report(
            2,
            SYNTHETIC_FORMATION_WINGMEN[1],
            [f64::MAX, 0.0, 0.0],
            true,
        ),
    ];
    assert_eq!(
        runtime
            .update_formation(&intact(START, &impossible))
            .expect_err("an infinite station is never handed out"),
        CombatError::StationNotFinite {
            formation: FORMATION
        }
    );

    // The refusal changed nothing: the same tick with coordinates that are
    // actually a position works, and only then is the recovery applied.
    let possible = vec![
        synthetic_member_report(0, SYNTHETIC_FORMATION_LEADER, [1_000.0, 0.0, 0.0], false),
        synthetic_member_report(1, SYNTHETIC_FORMATION_WINGMEN[0], [810.0, 0.0, 0.0], true),
        synthetic_member_report(2, SYNTHETIC_FORMATION_WINGMEN[1], [710.0, 0.0, 0.0], true),
    ];
    let retry = runtime
        .update_formation(&intact(START, &possible))
        .expect("the retry applies");
    assert_eq!(
        retry.trigger,
        Some(cs_sim::ai::combat::RecoveryTrigger::LeaderLost)
    );
    assert!(
        retry.stations.iter().all(|station| station
            .position
            .to_array()
            .iter()
            .all(|c| c.is_finite()))
    );
}

/// State never leaks across session generations: a member, an assigned target
/// and a decision's observer from another generation are all refused.
#[test]
fn accept_f32_c_foreign_generations_never_touch_the_formation_or_the_decision() {
    let mut runtime = synthetic_combat_runtime();

    let foreign_members = vec![
        synthetic_member_report(0, SYNTHETIC_FORMATION_LEADER, [1_000.0, 0.0, 0.0], true),
        FormationMemberReport {
            actor: foreign(SYNTHETIC_FORMATION_WINGMEN[0]),
            ..synthetic_member_report(1, SYNTHETIC_FORMATION_WINGMEN[0], [800.0, 0.0, 0.0], true)
        },
        synthetic_member_report(2, SYNTHETIC_FORMATION_WINGMEN[1], [700.0, 0.0, 0.0], true),
    ];
    assert_eq!(
        runtime
            .update_formation(&intact(START, &foreign_members))
            .expect_err("another generation's member is refused"),
        CombatError::ForeignSession {
            actor: foreign(SYNTHETIC_FORMATION_WINGMEN[0]),
            session: SYNTHETIC_SESSION,
        }
    );
    let intact_now = intact_members();
    let foreign_target = FormationTick {
        assigned_target: Some(foreign(2)),
        ..intact(START, &intact_now)
    };
    assert_eq!(
        runtime
            .update_formation(&foreign_target)
            .expect_err("another generation's target is refused"),
        CombatError::ForeignSession {
            actor: foreign(2),
            session: SYNTHETIC_SESSION,
        }
    );
    // A formation registered with a foreign member never exists at all.
    let roster = FormationRoster::try_new(
        FormationId(4),
        0,
        vec![FormationRosterMember {
            slot: 0,
            actor: foreign(1),
        }],
    )
    .expect("the roster itself is well-formed");
    assert_eq!(
        CombatRuntime::new(
            SYNTHETIC_SESSION,
            &[synthetic_escort_profile()],
            Vec::new(),
            synthetic_difficulty_roster(),
        )
        .expect("the runtime is valid")
        .with_formation(roster, synthetic_recovery_policies())
        .expect_err("a foreign member cannot be registered"),
        CombatError::ForeignSession {
            actor: foreign(1),
            session: SYNTHETIC_SESSION,
        }
    );

    // A decision whose observer is not a living member of the formation it
    // names is refused rather than answered from a stranger's state.
    let candidates = vec![synthetic_candidate(
        2,
        [300.0, 0.0, 0.0],
        Some(Allegiance::Hostile),
        true,
    )];
    // The outsider holds a declared slot, so the request is coherent and the
    // refusal that follows is the one under test: it is not a member.
    let outsider =
        RoleAssignment::protecting(synthetic_actor(99), CombatRole::Escort, synthetic_actor(20))
            .expect("an escort never protects itself")
            .in_formation(cs_sim::ai::combat::FormationSlot {
                formation: FORMATION,
                slot: 5,
            });
    let arsenal = synthetic_arsenal();
    assert_eq!(
        runtime
            .step(&CombatantRequest {
                observer: synthetic_actor(99),
                now: Tick(START),
                observer_position: WorldPosition::try_new([0.0, 0.0, 0.0]).expect("finite"),
                assignment: &outsider,
                formation: Some(FORMATION),
                protected_alive: Some(true),
                candidates: &candidates,
                arsenal: Some(&arsenal),
                ace: None,
                tier: DifficultyTier::Standard,
            })
            .expect_err("an outsider has no formation facts"),
        CombatError::UnknownFormationMember {
            formation: FORMATION,
            observer: synthetic_actor(99),
        }
    );
    // The same actor without a slot decides without any formation: an actor
    // outside a formation decides without any.
    let detached =
        RoleAssignment::protecting(synthetic_actor(99), CombatRole::Escort, synthetic_actor(20))
            .expect("an escort never protects itself");
    assert!(
        runtime
            .step(&CombatantRequest {
                observer: synthetic_actor(99),
                now: Tick(START),
                observer_position: WorldPosition::try_new([0.0, 0.0, 0.0]).expect("finite"),
                assignment: &detached,
                formation: None,
                protected_alive: Some(true),
                candidates: &candidates,
                arsenal: Some(&arsenal),
                ace: None,
                tier: DifficultyTier::Standard,
            })
            .is_ok()
    );
}

/// A decision may not run without the formation facts its own assignment
/// declares. The coordinator builds the facts, so a request that withholds the
/// formation — or names another one — would decide against state the mission
/// never assigned, and report no recovery path for an actor that has one.
#[test]
fn accept_f32_c_a_request_may_not_withhold_or_replace_the_formations_facts() {
    let runtime = synthetic_combat_runtime();
    let candidates = vec![synthetic_candidate(
        2,
        [300.0, 0.0, 0.0],
        Some(Allegiance::Hostile),
        true,
    )];
    let arsenal = synthetic_arsenal();
    let in_formation = RoleAssignment::protecting(
        synthetic_actor(SYNTHETIC_FORMATION_WINGMEN[0]),
        CombatRole::Escort,
        synthetic_actor(20),
    )
    .expect("an escort never protects itself")
    .in_formation(cs_sim::ai::combat::FormationSlot {
        formation: FORMATION,
        slot: 1,
    });
    let step = |assignment: &RoleAssignment, formation: Option<FormationId>| {
        runtime.step(&CombatantRequest {
            observer: synthetic_actor(SYNTHETIC_FORMATION_WINGMEN[0]),
            now: Tick(START),
            observer_position: WorldPosition::try_new([800.0, 0.0, 0.0]).expect("finite"),
            assignment,
            formation,
            protected_alive: Some(true),
            candidates: &candidates,
            arsenal: Some(&arsenal),
            ace: None,
            tier: DifficultyTier::Standard,
        })
    };

    // The formation the assignment declares, withheld by the request.
    assert_eq!(
        step(&in_formation, None).expect_err("the facts may not be withheld"),
        CombatError::FormationFactsOmitted {
            formation: FORMATION
        }
    );
    // Named as a different formation than the one the assignment declares.
    assert_eq!(
        step(&in_formation, Some(FormationId(9))).expect_err("two formations at once is refused"),
        CombatError::FormationAssignmentMismatch {
            assigned: Some(FORMATION),
            facts: FormationId(9),
        }
    );
    // Named at all by an assignment that places the observer in no formation.
    let detached = RoleAssignment::protecting(
        synthetic_actor(SYNTHETIC_FORMATION_WINGMEN[0]),
        CombatRole::Escort,
        synthetic_actor(20),
    )
    .expect("an escort never protects itself");
    assert_eq!(
        step(&detached, Some(FORMATION))
            .expect_err("facts for a formation the assignment does not declare"),
        CombatError::FormationAssignmentMismatch {
            assigned: None,
            facts: FORMATION,
        }
    );
    // And the one agreeing pair still decides.
    assert!(step(&in_formation, Some(FORMATION)).is_ok());
}

/// The ace registry refuses a variant that modifies one role and carries
/// another's profile, a duplicated variant and an ace key from another
/// namespace. A wrong-role ace would hand an actor a behavior its mission
/// never assigned.
#[test]
fn accept_f32_c_the_ace_registry_refuses_incoherent_variants() {
    assert_eq!(
        AceVariant::try_new(
            synthetic_ace_id(),
            CombatRole::Escort,
            DifficultyTier::Standard,
            synthetic_fighter_profile(),
        )
        .expect_err("an escort ace cannot carry a fighter profile"),
        CombatError::AceRoleMismatch {
            id: synthetic_ace_id(),
            base: CombatRole::Escort,
            profile: CombatRole::FighterAttack,
        }
    );
    assert_eq!(
        CombatRuntime::new(
            SYNTHETIC_SESSION,
            &[synthetic_escort_profile()],
            vec![synthetic_ace_variant(), synthetic_ace_variant()],
            synthetic_difficulty_roster(),
        )
        .expect_err("one variant per pilot"),
        CombatError::DuplicateAce {
            id: synthetic_ace_id()
        }
    );
    assert_eq!(
        AceId::try_new(
            ContentId::from_source(ContentKind::Mission, "synthetic.ace-mission")
                .expect("a valid mission id")
        )
        .expect_err("an ace is not a mission"),
        CombatError::AceKindMismatch {
            id: ContentId::from_source(ContentKind::Mission, "synthetic.ace-mission")
                .expect("a valid mission id")
        }
    );

    let runtime = synthetic_combat_runtime();
    let escort = RoleAssignment::protecting(
        synthetic_actor(SYNTHETIC_FORMATION_WINGMEN[0]),
        CombatRole::Escort,
        synthetic_actor(20),
    )
    .expect("an escort never protects itself");
    let stranger = AceId::try_new(
        ContentId::from_source(ContentKind::Pilot, "synthetic.somebody-else")
            .expect("a valid pilot id"),
    )
    .expect("the pilot id is in the pilot namespace");
    assert_eq!(
        runtime
            .resolve_profile(&escort, Some(&stranger), DifficultyTier::Standard)
            .expect_err("an unknown pilot has no declared behavior"),
        CombatError::UnknownAce { id: stranger }
    );
}

/// Two profiles for one role, or two tiers with the same name, are refused at
/// construction: whichever of them won would otherwise be an accident of
/// iteration order.
#[test]
fn accept_f32_c_a_difficulty_roster_refuses_duplicates() {
    assert_eq!(
        DifficultyRoster::new()
            .with_tier(
                DifficultyTier::Standard,
                &[synthetic_escort_profile(), synthetic_escort_profile()],
            )
            .expect_err("one profile per role per tier"),
        CombatError::DuplicateRoleProfile {
            role: CombatRole::Escort
        }
    );
    assert_eq!(
        DifficultyRoster::new()
            .with_tier(DifficultyTier::Standard, &[synthetic_escort_profile()])
            .expect("the tier is valid")
            .with_tier(DifficultyTier::Standard, &[synthetic_fighter_profile()])
            .expect_err("one profile per tier"),
        CombatError::DuplicateDifficultyTier {
            tier: DifficultyTier::Standard
        }
    );
}

/// A role that cannot carry its own weapon is refused when its profile is
/// built, so no tier can declare a torpedo run with no launcher and then run
/// it. The declared record (F32-A) and the lowered runtime profile agree.
#[test]
fn accept_f32_c_a_difficulty_profile_cannot_declare_an_unflyable_role() {
    let knobs = SkillKnobs {
        reaction_ticks: 6,
        aim_error_rad: 0.015,
        engagement_range_m: 800.0,
        fire_discipline_ticks: 24,
    };
    assert_eq!(
        SkillProfile::try_new(
            CombatRole::TorpedoRun,
            RoleArsenal::guns(),
            knobs,
            synthetic_fighter_profile().priority(),
        )
        .expect_err("a torpedo run needs a launcher"),
        CombatError::RoleArsenalMissing {
            role: CombatRole::TorpedoRun
        }
    );
    assert!(
        SkillProfile::try_new(
            CombatRole::BomberRun,
            RoleArsenal::guns(),
            knobs,
            synthetic_fighter_profile().priority(),
        )
        .is_err(),
        "and the lowered runtime profile refuses it the same way the declared \
         record (F32-A) does"
    );
}
