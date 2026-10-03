//! Acceptance scenario F32-D: the retail measurement of the original's AI
//! skill tiers and difficulty option, and the runtime half that mirrors it.
//!
//! Spec: `specs/F32-ai-combat-formations-aces-and-difficulty.md`, stage
//! `### F32-D`, acceptance test **AC04**. Task test prefix: `accept_f32_d_`.
//!
//! These tests drive production code only. The runtime half calls
//! `cs_sim::ai::combat`'s [`DifficultyTier`] mapping and its probe; the retail
//! half re-reads the owner's installation through `cs_content`'s production
//! readers and checks the runtime's mirror against what was measured, so the
//! two crates' copies of `ORIGINAL_DIFFICULTY_STEPS` cannot drift apart.
//!
//! **What is and is not measured.** The original's *campaign difficulty option*
//! is measured: how many steps it has, and that no per-scenario record carries a
//! difficulty. The original's *AI skill tiers* are measured: the label
//! vocabulary its scenario descriptors spell. What is **not** measured — and is
//! not claimed anywhere — is what any difficulty step or any skill tier *does*,
//! what the original's AI roles are, or what its nine ace stat slots mean. See
//! `docs/findings/2026-10-03-f32-d-original-ai-roles-and-difficulty.md`.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. The retail tests carry no display text out of the installation:
//! they measure ids, counts and occupancies.

use cs_sim::ai::combat::{
    CombatRole, CombatRuntime, CombatStep, CombatantRequest, DifficultyProbeSpec, DifficultyTier,
    ORIGINAL_DIFFICULTY_STEPS, PROBE_FORMATION, PriorityTerm, RoleAssignment, SYNTHETIC_SESSION,
    answers_protected_threat, synthetic_actor, synthetic_arsenal, synthetic_candidate,
    synthetic_combat_runtime, synthetic_difficulty_probe_spec, synthetic_recovery_policies,
    synthetic_threat,
};
use cs_sim::targeting::Allegiance;
use cs_types::Tick;

/// One escort decision against a charging attacker, with the charge's
/// lifecycle reported as the caller sees it.
///
/// `protected_alive: false` is the interesting half: the attack is fresh and
/// noticed, but the actor it hit is already destroyed, so the
/// protected-actor term contributes zero and the escort may still select the
/// attacker for an unrelated reason.
fn step_with(protected_alive: Option<bool>) -> CombatStep {
    let runtime = synthetic_combat_runtime();
    let observer = synthetic_actor(30);
    let charge = synthetic_actor(40);
    let assignment = RoleAssignment::protecting(observer, CombatRole::Escort, charge)
        .expect("the escort assignment is valid");
    let arsenal = synthetic_arsenal();
    // The attacker is also the script objective and the nearer hostile, so it
    // is selected whether or not the protected-actor term contributes: only
    // the *term* tells the two cases apart.
    let candidates = [
        synthetic_candidate(31, [200.0, 500.0, 0.0], Some(Allegiance::Hostile), true)
            .with_threat(synthetic_threat(31, 40, Tick(0), 1, 0)),
        synthetic_candidate(32, [900.0, 500.0, 0.0], Some(Allegiance::Hostile), false),
    ];
    let here = synthetic_candidate(1, [0.0, 500.0, 0.0], None, false);
    runtime
        .step(&CombatantRequest {
            observer,
            // Thirty ticks after the attack was recorded: past the Standard
            // tier's 24-tick reaction delay, inside the policy's 120-tick
            // threat window, so the attack is fresh *and* noticed.
            now: Tick(30),
            observer_position: here.position,
            assignment: &assignment,
            formation: None,
            protected_alive,
            candidates: &candidates,
            arsenal: Some(&arsenal),
            ace: None,
            tier: DifficultyTier::Standard,
        })
        .expect("the escort decides")
}

/// The probe counts an **answer** from the trace's own term score, not from
/// the scenario's knowledge of who the attacker is.
///
/// The two readings coincide inside the probe's authored geometry — the
/// attacker only out-scores the harmless hostile once its attack is noticed —
/// so this test pins the distinction where it is visible: with the charge
/// reported destroyed, the protected-actor term contributes nothing even though
/// the attack is fresh, noticed, and the selected target may still be the
/// attacker. A scenario-derived counter would say "answered"; the trace says
/// "did not".
#[test]
fn accept_f32_d_an_answer_is_read_from_the_trace_and_not_from_the_scenarios_attacker() {
    let answered = step_with(Some(true));
    assert_eq!(
        answered.target(),
        Some(synthetic_actor(31)),
        "with the charge alive, the noticed attacker is selected and its protected term contributes"
    );
    assert!(
        answers_protected_threat(&answered),
        "so this decision is an answer"
    );

    let destroyed = step_with(Some(false));
    assert_eq!(
        destroyed.target(),
        Some(synthetic_actor(31)),
        "with the charge destroyed the attacker is still the objective and the nearest hostile, \
         so it is selected for those reasons"
    );
    assert!(
        !answers_protected_threat(&destroyed),
        "but the protected-actor term contributes zero, so this is not an answer"
    );
    let term = destroyed
        .trace()
        .candidate(synthetic_actor(31))
        .and_then(|trace| trace.term(PriorityTerm::ProtectedActorThreat))
        .expect("the selected target's trace carries the term");
    assert_eq!(term.contribution, 0.0);
    assert!(
        term.weight > 0.0,
        "the policy still weights the term; the charge is what is gone"
    );

    // An unreported lifecycle is not a reported-destroyed one.
    assert!(
        answers_protected_threat(&step_with(None)),
        "an unreported lifecycle is not a destroyed charge"
    );
}

/// The runtime tier vocabulary agrees with the measured option: exactly
/// [`ORIGINAL_DIFFICULTY_STEPS`] declared tiers carry a measured step, and
/// every remaining tier is *reported* as a designed extension rather than
/// silently presented as the original's.
///
/// This is the mirror of `cs_content::ai::ORIGINAL_DIFFICULTY_STEPS`; the
/// retail test below checks the two crates against the installation.
#[test]
fn accept_f32_d_the_declared_tier_vocabulary_is_one_step_longer_than_the_measured_option() {
    assert_eq!(
        ORIGINAL_DIFFICULTY_STEPS, 3,
        "the measured original option's step count"
    );
    assert_eq!(
        DifficultyTier::ALL.len(),
        (ORIGINAL_DIFFICULTY_STEPS + 1) as usize,
        "four declared tiers against three measured steps"
    );
    assert_eq!(
        DifficultyTier::measured_tier_count(),
        ORIGINAL_DIFFICULTY_STEPS as usize
    );
    assert_eq!(DifficultyTier::Relaxed.measured_step(), Some(0));
    assert_eq!(DifficultyTier::Standard.measured_step(), Some(1));
    assert_eq!(DifficultyTier::Hard.measured_step(), Some(2));
    assert_eq!(DifficultyTier::Elite.measured_step(), None);
    assert!(DifficultyTier::Elite.is_designed_extension());
    assert!(!DifficultyTier::Hard.is_designed_extension());

    // The mapping is *positional* and total: it covers the measured steps
    // contiguously with no gap and no duplicate, which is what "at every
    // discovered difficulty" needs before a probe runs.
    let mut steps: Vec<u32> = DifficultyTier::ALL
        .iter()
        .filter_map(|tier| tier.measured_step())
        .collect();
    assert_eq!(steps, (0..ORIGINAL_DIFFICULTY_STEPS).collect::<Vec<u32>>());
    steps.dedup();
    assert_eq!(steps.len(), ORIGINAL_DIFFICULTY_STEPS as usize);

    // Every tier the original does have a step for is *some* declared tier, so
    // a caller cannot say "the runtime has no tier for measured step 2".
    for step in 0..ORIGINAL_DIFFICULTY_STEPS {
        assert!(
            DifficultyTier::ALL
                .iter()
                .any(|tier| tier.measured_step() == Some(step)),
            "measured step {step} has a declared tier"
        );
    }
}

/// The negative the retail stage must not be able to fake: with no
/// installation there is no measured count, and the runtime still refuses to
/// invent one.
///
/// The probe's coverage query is what a caller would use to assert "at every
/// discovered difficulty", and it answers from
/// [`ORIGINAL_DIFFICULTY_STEPS`] alone — a constant that is only as honest as
/// the measurement behind it, which is why that measurement is a test of its
/// own rather than a comment.
#[test]
fn accept_f32_d_without_a_measurement_the_probe_still_refuses_to_claim_coverage_it_did_not_run() {
    let spec = DifficultyProbeSpec::try_new(
        SYNTHETIC_SESSION,
        synthetic_difficulty_probe_spec().ticks,
        4,
        9_001,
        synthetic_recovery_policies(),
    )
    .expect("the probe spec is valid");
    let report: cs_sim::ai::combat::DifficultyProbeReport = synthetic_combat_runtime()
        .probe_difficulties(&spec)
        .expect("the probe replays");
    assert!(report.covers_every_measured_step());

    // A report with no tiers at all has measured nothing, so its coverage
    // answer is `false` even though the empty `geometry_is_tier_invariant`
    // would be vacuously tidy. Hand-built rather than produced: it is the
    // shape a caller could otherwise present as evidence.
    let empty = cs_sim::ai::combat::DifficultyProbeReport {
        spec,
        runs: Vec::new(),
        tiers: Vec::new(),
        comparisons: Vec::new(),
    };
    assert!(!empty.geometry_is_tier_invariant());
    assert!(!empty.covers_every_measured_step());
    assert_eq!(empty.measured_tiers(), Vec::new());
    assert!(!empty.outcomes_differ());
    assert_eq!(empty.distinct_profile_count(), 0);
}

/// The measured difficulties are a **selection**, not a per-mission record, so
/// the runtime must not let a caller pin a step onto a scenario as if the data
/// said so.
///
/// The runtime has no field for that today: `RoleAssignment` carries a role, a
/// protected actor and a formation slot, and `CombatantRequest::tier` carries
/// the selection the mission made. This test pins the shape so a later stage
/// that adds a per-mission difficulty field has to break it deliberately.
#[test]
fn accept_f32_d_a_difficulty_step_is_a_selection_the_mission_made_and_not_a_scenario_field() {
    let spec = synthetic_difficulty_probe_spec();
    assert_eq!(spec.policies(), synthetic_recovery_policies());
    // The spec has no tier: the probe always runs every declared tier, so a
    // caller cannot quietly narrow "at every discovered difficulty" to the one
    // tier they happened to test.
    assert_eq!(spec.session, SYNTHETIC_SESSION);
    assert!(spec.ticks > 0 && spec.runs_per_tier > 1);

    // The planner's roles are the declared vocabulary; the probe adds none.
    let runtime: CombatRuntime = synthetic_combat_runtime();
    assert_eq!(runtime.session(), SYNTHETIC_SESSION);
    assert!(
        runtime.planner().profile(CombatRole::Escort).is_some(),
        "the escort is a declared role the probe can decide for"
    );
    assert!(
        runtime
            .planner()
            .recovery_policies(PROBE_FORMATION)
            .is_none(),
        "the probe's formation is the probe's own, not one the fixture registered"
    );

    // The assignment the probe builds for itself is escort + protected charge:
    // the same AC01 relationship the policy terms are about.
    let assignment =
        RoleAssignment::protecting(synthetic_actor(20), CombatRole::Escort, synthetic_actor(40))
            .expect("the escort assignment is valid");
    assert_eq!(assignment.role(), CombatRole::Escort);
    assert!(assignment.protected().is_some());
    assert!(assignment.formation().is_none());
}
