//! F32-LOWERING (#551) acceptance tests: the declared AI combat records reach
//! the runtime through `cs_app::ai::combat`.
//!
//! Task test prefix: `accept_f32_e_`. Every value is the newly authored
//! synthetic fixture of `cs_content::ai`; none is original data and no test
//! reads `CS_GAME_DIR`.

use cs_app::ai::combat::{
    LoweringError, lower_ace, lower_assignment, lower_difficulty, lower_formation_roster,
    lower_profile, lower_profiles, lower_recovery_policies, lower_runtime, lower_tier,
};
use cs_content::ai::{
    DeclaredCombatRole, DeclaredCombatRules, DeclaredRoleProfile, DifficultyTier, RoleArsenal,
    SkillKnobs, declared_synthetic_ace_profile, declared_synthetic_combat_rules,
    declared_synthetic_escort_knobs, declared_synthetic_escort_policy,
    declared_synthetic_formation, synthetic_combat_claim,
};
use cs_sim::ai::combat::{
    self as runtime, CombatRole, CombatantRequest, ProfileSource, RecoveryAction, synthetic_actor,
    synthetic_arsenal, synthetic_candidate,
};
use cs_sim::targeting::Allegiance;
use cs_types::Tick;
use cs_types::content::{Origin, Provenance, Resolved};
use cs_types::space::WorldPosition;
use std::collections::BTreeMap;

const SESSION: u64 = runtime::SYNTHETIC_SESSION;

fn rules() -> DeclaredCombatRules {
    declared_synthetic_combat_rules()
}

fn escort_with(knobs: SkillKnobs) -> DeclaredRoleProfile {
    DeclaredRoleProfile::try_new(
        DeclaredCombatRole::Escort,
        RoleArsenal::guns(),
        knobs,
        declared_synthetic_escort_policy(),
        Origin::SyntheticFixture,
        Provenance::designed(synthetic_combat_claim()),
    )
    .expect("an unknown knob still builds a declared record")
}

/// Every declared field arrives at its runtime mirror unchanged.
#[test]
fn accept_f32_e_profiles_lower_field_wise() {
    let profiles = lower_profiles(&rules()).expect("the synthetic rules lower");
    let escort = profiles
        .iter()
        .find(|profile| profile.role() == CombatRole::Escort)
        .expect("the escort role is lowered");
    assert_eq!(escort.knobs(), runtime::synthetic_escort_profile().knobs());
    assert_eq!(
        escort.priority(),
        runtime::synthetic_escort_profile().priority()
    );
    assert_eq!(escort.arsenal(), runtime::RoleArsenal::guns());
    assert_eq!(profiles.len(), rules().roles().len());
}

/// An unknown knob refuses to lower and names the record, field and claim.
#[test]
fn accept_f32_e_unknown_knob_is_refused_by_name() {
    let claim = synthetic_combat_claim();
    let knobs = SkillKnobs {
        aim_error_rad: Resolved::unknown(claim.clone(), "aim error is unmeasured")
            .expect("a reason is given"),
        ..declared_synthetic_escort_knobs()
    };
    let error = lower_profile(&escort_with(knobs)).expect_err("an unknown is never a zero");
    assert_eq!(
        error,
        LoweringError::UnknownValue {
            record: "role escort".to_owned(),
            field: "aim_error_rad",
            claim_id: claim,
            reason: "aim error is unmeasured".to_owned(),
        }
    );
}

/// An unknown priority weight is refused the same way.
#[test]
fn accept_f32_e_unknown_weight_is_refused_by_name() {
    let claim = synthetic_combat_claim();
    let mut policy = declared_synthetic_escort_policy();
    policy.objective_weight = Resolved::unknown(claim, "weight unmeasured").expect("reason");
    let profile = DeclaredRoleProfile::try_new(
        DeclaredCombatRole::Escort,
        RoleArsenal::guns(),
        declared_synthetic_escort_knobs(),
        policy,
        Origin::SyntheticFixture,
        Provenance::designed(synthetic_combat_claim()),
    )
    .expect("builds");
    assert!(matches!(
        lower_profile(&profile),
        Err(LoweringError::UnknownValue {
            field: "objective_weight",
            ..
        })
    ));
}

/// An ace assigned to a role other than its base role is refused.
#[test]
fn accept_f32_e_ace_role_mismatch_is_refused() {
    let ace = declared_synthetic_ace_profile();
    let error = lower_assignment(
        synthetic_actor(1),
        DeclaredCombatRole::FighterAttack,
        Some(&ace),
        None,
    )
    .expect_err("an escort ace cannot fly the fighter role");
    assert_eq!(
        error,
        LoweringError::AceRoleMismatch {
            ace: ace.id().clone(),
            declared: DeclaredCombatRole::Escort,
            assigned: DeclaredCombatRole::FighterAttack,
        }
    );
    let ok = lower_assignment(
        synthetic_actor(1),
        DeclaredCombatRole::Escort,
        Some(&ace),
        Some(synthetic_actor(2)),
    )
    .expect("the base role matches");
    assert_eq!(ok.role(), CombatRole::Escort);
    assert_eq!(ok.protected(), Some(synthetic_actor(2)));
}

/// A role the rules never declared, and a tier they never declared, are
/// refused by name.
#[test]
fn accept_f32_e_undeclared_role_and_tier_are_refused() {
    let rules = rules();
    let mut ace = declared_synthetic_ace_profile();
    // Rebuild the ace on a role the synthetic rules do not declare.
    ace = cs_content::ai::DeclaredAceProfile::try_new(
        ace.id().clone(),
        DeclaredCombatRole::Retreat,
        ace.overrides().to_vec(),
        Origin::SyntheticFixture,
        ace.provenance().clone(),
    )
    .expect("valid");
    assert_eq!(
        lower_ace(&rules, &ace, DifficultyTier::Standard),
        Err(LoweringError::NoProfileForRole {
            role: DeclaredCombatRole::Retreat
        })
    );
    assert_eq!(
        lower_ace(
            &rules,
            &declared_synthetic_ace_profile(),
            DifficultyTier::Hard
        ),
        Err(LoweringError::UndeclaredTier {
            tier: DifficultyTier::Hard
        })
    );
}

/// Every declared tier is lowered, composed onto every declared role.
#[test]
fn accept_f32_e_difficulty_roster_composes_tiers() {
    let rules = rules();
    let roster = lower_difficulty(&rules).expect("lowers");
    for tier in rules.difficulties() {
        let runtime_tier = lower_tier(tier.tier());
        assert!(roster.has_tier(runtime_tier));
        assert_eq!(roster.roles(runtime_tier).len(), rules.roles().len());
    }
    assert!(!roster.has_tier(runtime::DifficultyTier::Hard));
}

/// Formation membership and recovery paths arrive; an unbound slot is refused.
#[test]
fn accept_f32_e_formation_lowers_and_refuses_unbound_slots() {
    let formation = declared_synthetic_formation();
    let actors: BTreeMap<u32, _> = formation
        .members()
        .iter()
        .map(|member| (member.slot, synthetic_actor(10 + u64::from(member.slot))))
        .collect();
    let roster = lower_formation_roster(&formation, &actors).expect("lowers");
    assert_eq!(roster.leader_slot(), formation.leader_slot());
    assert_eq!(roster.members().len(), formation.members().len());
    let policies = lower_recovery_policies(&formation);
    assert_eq!(
        policies.leader_loss,
        match formation.recovery().leader_loss {
            cs_content::ai::RecoveryPolicy::ReassignLead => RecoveryAction::ReassignLead,
            cs_content::ai::RecoveryPolicy::Regroup => RecoveryAction::Regroup,
            cs_content::ai::RecoveryPolicy::HoldFormation => RecoveryAction::HoldFormation,
            cs_content::ai::RecoveryPolicy::Withdraw => RecoveryAction::Withdraw,
            cs_content::ai::RecoveryPolicy::ResumeRoute => RecoveryAction::ResumeRoute,
        }
    );
    let mut missing = actors;
    let slot = formation.leader_slot();
    missing.remove(&slot);
    assert_eq!(
        lower_formation_roster(&formation, &missing),
        Err(LoweringError::UnboundFormationSlot {
            formation: formation.id(),
            slot
        })
    );
}

/// End to end: a declared ace variant reaches `CombatRuntime::step`, and the
/// firing solution reports the variant's aim error and cadence — not the
/// role's.
#[test]
fn accept_f32_e_declared_ace_reaches_the_decision_and_firing_solution() {
    let rules = rules();
    let mut runtime_rules =
        lower_runtime(SESSION, &rules, DifficultyTier::Standard).expect("the rules lower");
    let ace = declared_synthetic_ace_profile();
    let ace_id: &'static runtime::AceId = Box::leak(Box::new(
        runtime::AceId::try_new(ace.id().clone()).expect("pilot id"),
    ));
    let assignment = lower_assignment(
        synthetic_actor(1),
        DeclaredCombatRole::Escort,
        Some(&ace),
        Some(synthetic_actor(2)),
    )
    .expect("assignment lowers");
    let candidates = vec![synthetic_candidate(
        3,
        [400.0, 0.0, 0.0],
        Some(Allegiance::Hostile),
        false,
    )];
    let arsenal = synthetic_arsenal();
    let request = |ace: Option<&'static runtime::AceId>| -> CombatantRequest<'_> {
        CombatantRequest {
            observer: synthetic_actor(1),
            now: Tick(1_000),
            observer_position: WorldPosition::try_new([0.0, 0.0, 0.0]).expect("finite"),
            assignment: &assignment,
            formation: None,
            protected_alive: Some(true),
            candidates: &candidates,
            arsenal: Some(&arsenal),
            ace,
            tier: runtime::DifficultyTier::Standard,
        }
    };

    let step = runtime_rules
        .step(&request(Some(ace_id)))
        .expect("ace step");
    assert!(matches!(step.source, ProfileSource::Ace { .. }));
    let solution = step.trace().firing.clone().expect("a target was chosen");
    let declared_aim = ace
        .knob_value(cs_content::ai::SkillKnob::AimErrorRad)
        .expect("declared")
        .known();
    assert!(matches!(
        declared_aim,
        Some(cs_content::ai::SkillKnobValue::Angle(a)) if (a - 0.015).abs() < f64::EPSILON
    ));
    assert!((solution.aim_error_rad - 0.015).abs() < f64::EPSILON);
    assert_eq!(
        solution.fire_discipline_ticks,
        runtime::synthetic_escort_profile()
            .knobs()
            .fire_discipline_ticks
    );

    // The same actor without the ace flies the tier's role profile, which aims worse.
    let plain = runtime_rules.step(&request(None)).expect("role step");
    let role_aim = plain.trace().firing.clone().expect("fires").aim_error_rad;
    assert!(role_aim > solution.aim_error_rad);
}
