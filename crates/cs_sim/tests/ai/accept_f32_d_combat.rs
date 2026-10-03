//! Acceptance scenario F32-D: run repeated mission-combat probes at **every
//! discovered difficulty** and compare the outcomes statistically.
//!
//! Spec: `specs/F32-ai-combat-formations-aces-and-difficulty.md`, stage
//! `### F32-D`, acceptance test **AC04**. Task test prefix: `accept_f32_d_`.
//!
//! Every test drives production code only: `cs_sim::ai::combat`'s
//! [`CombatRuntime::probe_difficulties`], the [`DifficultyProbeSpec`] it
//! replays, the [`DifficultyProbeReport`] / [`TierProbeOutcome`] /
//! [`TierComparison`] it aggregates and the [`DifficultyTier`] vocabulary with
//! its measured-step mapping. Removing the per-tier profile selection, the
//! trace-driven counting, the per-index geometry digest, the arsenal digest,
//! the variance, the adjacent comparison or the measured-step coverage query
//! fails the test that names it.
//!
//! Two kinds of test live here and both run in CI:
//!
//! * **integration** — the probe itself, its statistics and the invariants it
//!   exists to prove;
//! * **retail** — `#[ignore = "requires CS_GAME_DIR"]` tests that re-derive the
//!   measured facts from the owner's installation and check them against the
//!   constants this crate mirrors.
//!
//! Every value here is newly authored synthetic fixture or probe-geometry data,
//! never original game data. The *measurements* those fixture constants stand
//! for are recorded in
//! `docs/findings/2026-10-03-f32-d-original-ai-roles-and-difficulty.md`.

use cs_sim::ai::combat::{
    CombatRole, CombatRuntime, DIFFICULTY_PROBE_DOMAIN, DifficultyProbeReport, DifficultyProbeSpec,
    DifficultyTier, ORIGINAL_DIFFICULTY_STEPS, PROBE_FORMATION, PROBE_LATERAL_JITTER_M,
    ProbeDifference, RecoveryTrigger, RoleAssignment, SYNTHETIC_SESSION, synthetic_actor,
    synthetic_combat_runtime, synthetic_difficulty_probe_spec, synthetic_recovery_policies,
};
use cs_types::random::SplitMix64;

/// The probe every scenario here starts from.
fn report() -> DifficultyProbeReport {
    let runtime = synthetic_combat_runtime();
    let spec = synthetic_difficulty_probe_spec();
    runtime
        .probe_difficulties(&spec)
        .expect("the synthetic probe replays at every tier")
}

/// A shorter probe for the tests that only need the machinery.
fn short_report(runs: u32, ticks: u64, seed: u64) -> DifficultyProbeReport {
    let spec = DifficultyProbeSpec::try_new(
        SYNTHETIC_SESSION,
        ticks,
        runs,
        seed,
        synthetic_recovery_policies(),
    )
    .expect("the short probe spec is valid");
    synthetic_combat_runtime()
        .probe_difficulties(&spec)
        .expect("the short probe replays")
}

/// **AC04.** The minimum scenario: the probe runs at **every** discovered
/// difficulty, more than once, and the outcomes are compared rather than
/// assumed.
///
/// The measured result over the synthetic fixture: the ticks that answered an
/// authoritative attack against the charge rose at every step
/// (`relaxed` 190.0 per run, `standard` 303.0, `hard` 351.0, `elite` 375.0)
/// while the deferred-threat count fell (184.0, 97.0, 49.0, 25.0 per run) —
/// the same ordering in opposite directions, which is what "a more demanding
/// tier notices sooner" looks like in decisions rather than in a comment.
#[test]
fn accept_f32_d_every_discovered_difficulty_is_probed_repeatedly_and_compared() {
    let report = report();

    // Every declared tier was probed, with the requested number of runs.
    assert_eq!(
        report.tiers.len(),
        DifficultyTier::ALL.len(),
        "every declared tier gets an aggregate"
    );
    for tier in &report.tiers {
        assert_eq!(tier.runs, report.spec.runs_per_tier, "{:?} runs", tier.tier);
        assert!(tier.runs > 1, "a distribution needs more than one run");
    }
    assert_eq!(
        report.runs.len(),
        DifficultyTier::ALL.len() * report.spec.runs_per_tier as usize
    );

    // Every *discovered* difficulty — every measured original step — was probed,
    // contiguously and once each.
    assert_eq!(
        ORIGINAL_DIFFICULTY_STEPS, 3,
        "the measured option's step count"
    );
    assert!(
        report.covers_every_measured_step(),
        "measured steps 0..{ORIGINAL_DIFFICULTY_STEPS} were each probed"
    );
    assert_eq!(
        report.measured_tiers(),
        vec![
            DifficultyTier::Relaxed,
            DifficultyTier::Standard,
            DifficultyTier::Hard
        ],
        "the three measured steps, in option order"
    );
    assert_eq!(
        report.tier(DifficultyTier::Elite).map(|t| t.measured_step),
        Some(None),
        "the fourth tier is reported as a declared extension, not a measured step"
    );

    // The comparison is between adjacent tiers, one per pair.
    assert_eq!(report.comparisons.len(), DifficultyTier::ALL.len() - 1);
    assert!(
        report.outcomes_differ(),
        "the tiers' aggregates are distinguishable, so there was something to compare"
    );
    assert_eq!(
        report.distinct_profile_count(),
        DifficultyTier::ALL.len(),
        "each tier resolved its own effective profile"
    );

    // The measured direction: answering more, deferring less, never regressing.
    for comparison in &report.comparisons {
        assert_eq!(
            comparison.protected_answers,
            ProbeDifference::Higher,
            "{} -> {} answered more authoritative attacks",
            comparison.lower.label(),
            comparison.higher.label()
        );
        assert_eq!(
            comparison.deferred_threats,
            ProbeDifference::Lower,
            "{} -> {} deferred fewer",
            comparison.lower.label(),
            comparison.higher.label()
        );
    }
    assert!(report.protected_answers_never_regress());
}

/// The comparison is *statistical*: the reported aggregates carry a mean and a
/// population variance over the runs, and the relaxed tier's spread is
/// non-zero — a report whose only content were four identical deterministic
/// traces would not be a distribution.
#[test]
fn accept_f32_d_the_comparison_is_statistical_and_the_slowest_tier_spreads() {
    let report = report();

    for tier in &report.tiers {
        assert!(
            tier.mean_engagements.is_finite() && tier.mean_protected_answers.is_finite(),
            "{:?}: the means are finite",
            tier.tier
        );
        assert!(
            tier.variance_engagements >= 0.0 && tier.variance_protected_answers >= 0.0,
            "{:?}: a population variance is never negative",
            tier.tier
        );
        assert!(
            tier.mean_protected_answers > 0.0,
            "{:?}: the probe actually answered something",
            tier.tier
        );
        assert!(
            tier.mean_protected_answers < tier.ticks_per_run as f64,
            "{:?}: not every tick answered, so the reaction gate is doing work",
            tier.tier
        );
    }

    // The slowest tier reacts to the same authored geometry at slightly
    // different ticks from run to run, so its answer count is a distribution.
    let relaxed = report
        .tier(DifficultyTier::Relaxed)
        .expect("relaxed is probed");
    assert!(
        relaxed.variance_protected_answers > 0.0,
        "the seeded jitter makes the slowest tier's answers vary between runs"
    );
    // A faster tier notices every threat the authored schedule produces, so its
    // spread collapses to zero — a different reason, not the same one.
    let elite = report.tier(DifficultyTier::Elite).expect("elite is probed");
    assert_eq!(
        elite.variance_protected_answers, 0.0,
        "the fastest tier notices every threat the schedule produces"
    );
}

/// Non-negotiable 1, mechanically: the tier may not change the world or the
/// clock. Run `n` at every tier replays byte-identical geometry, and every run
/// replayed the requested number of ticks.
///
/// This is the probe's own guard: without it a "difficulty" that sped the
/// simulation up or moved an aircraft would produce a plausible-looking
/// outcome difference that meant nothing.
#[test]
fn accept_f32_d_a_tier_changes_the_behaviour_and_never_the_world_or_the_clock() {
    let report = report();

    assert!(
        report.geometry_is_tier_invariant(),
        "run n replayed the same positions and the same threat stamps at every tier"
    );
    assert!(
        report.clock_is_tier_invariant(),
        "every tier replayed the same number of ticks"
    );
    for run in &report.runs {
        assert_eq!(run.ticks, report.spec.ticks);
    }

    // The per-index view, so a failure says *which* run diverged.
    for tier in DifficultyTier::ALL {
        let outcome = report.tier(*tier).expect("every tier is aggregated");
        for run in 0..report.spec.runs_per_tier {
            let digest = outcome
                .geometry_fingerprint(run)
                .expect("every run has a geometry digest");
            let reference = report
                .tier(DifficultyTier::Relaxed)
                .and_then(|outcome| outcome.geometry_fingerprint(run));
            assert_eq!(
                Some(digest),
                reference,
                "{:?} run {run} replayed the world the most forgiving tier replayed",
                tier
            );
        }
    }

    // Two different runs really are different worlds, or the invariance above
    // would be vacuous: the seeded jitter must reach the geometry.
    let relaxed = report
        .tier(DifficultyTier::Relaxed)
        .expect("relaxed is probed");
    let first = relaxed.geometry_fingerprint(0).expect("run 0");
    let second = relaxed.geometry_fingerprint(1).expect("run 1");
    assert_ne!(
        first, second,
        "two runs of the same tier replayed different geometry"
    );
}

/// Non-negotiable 2, mechanically: the AI fires the same weapons at every
/// tier, from one snapshot, and the tiers never resolve to the same profile.
#[test]
fn accept_f32_d_a_tier_never_hands_the_ai_different_weapons() {
    let report = report();

    assert!(
        report.arsenal_is_tier_invariant(),
        "every tier replayed one weapons snapshot"
    );
    for tier in DifficultyTier::ALL {
        let outcome = report.tier(*tier).expect("every tier is aggregated");
        assert!(
            outcome.arsenal_is_uniform(),
            "{:?}: one arsenal digest for every run",
            tier
        );
        assert!(
            outcome.profile_is_uniform(),
            "{:?}: one effective profile for every run of the tier",
            tier
        );
    }
    let digests: Vec<u64> = report
        .tiers
        .iter()
        .map(|tier| tier.arsenal_fingerprints[0])
        .collect();
    let mut sorted = digests.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        1,
        "one arsenal for the whole probe: {digests:?}"
    );
}

/// The negative of AC04, and the reason the probe exists: a runtime that
/// resolved **one** profile for every tier measures four identical
/// distributions and there is nothing to compare.
///
/// The roster here is deliberately degenerate — every tier carries the same
/// escort profile — so the probe must report `outcomes_differ() == false` and
/// `distinct_profile_count() == 1`. An implementation that compared something
/// other than the resolved profile (a tier index, a clock, a geometry digest)
/// would say `true` here and would be measuring its own bookkeeping.
#[test]
fn accept_f32_d_a_roster_whose_tiers_collapse_measures_nothing_to_compare() {
    use cs_sim::ai::combat::{DifficultyRoster, synthetic_escort_profile};

    let flat = synthetic_escort_profile();
    let mut roster = DifficultyRoster::new();
    for tier in DifficultyTier::ALL {
        roster = roster
            .with_tier(*tier, &[flat])
            .expect("the flat roster declares every tier");
    }
    let runtime = CombatRuntime::new(SYNTHETIC_SESSION, &[flat], Vec::new(), roster)
        .expect("the flat runtime is valid");
    let spec = DifficultyProbeSpec::try_new(
        SYNTHETIC_SESSION,
        synthetic_difficulty_probe_spec().ticks,
        8,
        4_242,
        synthetic_recovery_policies(),
    )
    .expect("the flat probe spec is valid");

    let report = runtime
        .probe_difficulties(&spec)
        .expect("a degenerate roster still replays");

    assert_eq!(
        report.distinct_profile_count(),
        1,
        "one profile for four tiers"
    );
    assert!(
        !report.outcomes_differ(),
        "four identical distributions have nothing to compare"
    );
    assert!(
        report
            .comparisons
            .iter()
            .all(|comparison| comparison.protected_answers == ProbeDifference::Same),
        "every adjacent pair is identical, not a regression"
    );
    assert!(report.protected_answers_never_regress());
    // The invariants still hold — the world did not change, it simply had
    // nothing to change *with*.
    assert!(report.geometry_is_tier_invariant());
    assert!(report.arsenal_is_tier_invariant());
    assert!(report.clock_is_tier_invariant());
}

/// The probe is a *mission* probe, not a duel: it drives the formation
/// coordinator over a scenario that loses a follower mid-run and then loses the
/// formation's assigned target, and the run reports what the coordinator
/// actually did about it.
///
/// The attribution matters and is not the obvious one: the coordinator recovers
/// a **leader** loss, so the probe's follower (slot 1, while the deciding escort
/// leads in slot 0) is a membership change and raises nothing. The one recovery
/// each run measures is the declared assigned-target-destruction path, at the
/// tick the attacker disappears. A reader who assumed the follower loss
/// answered a recovery would be reading a scenario into the count.
#[test]
fn accept_f32_d_the_probe_replays_a_mission_formation_loss_at_every_tier() {
    let report = report();

    for tier in DifficultyTier::ALL {
        let outcome = report.tier(*tier).expect("every tier is aggregated");
        assert_eq!(
            outcome.recoveries,
            u64::from(report.spec.runs_per_tier),
            "{:?}: one applied recovery per run",
            tier
        );
    }
    // Which declared path answered, measured rather than narrated: the assigned
    // target's destruction, and never a leader loss the scenario does not have.
    for run in &report.runs {
        assert_eq!(
            run.recovery_triggers,
            vec![(RecoveryTrigger::AssignedTargetDestroyed, 400)],
            "{:?} run {}: the applied recovery and the tick it answered on",
            run.tier,
            run.run
        );
        assert!(
            !run.recovery_triggers
                .iter()
                .any(|(trigger, _)| *trigger == RecoveryTrigger::LeaderLost),
            "a lost follower is not a leader loss, so it raises no recovery"
        );
    }

    // The recovery is a declared path the coordinator applied, not a per-tick
    // invention: the probe registers the declared policies with the runtime, and
    // the action that answered is the policy's.
    assert_eq!(report.spec.policies(), synthetic_recovery_policies());
    assert_eq!(
        report.spec.policies().assigned_target_destroyed,
        cs_sim::ai::combat::RecoveryAction::Regroup,
        "the declared path the probe's one recovery answers"
    );
    assert_eq!(
        report.spec.policies().leader_loss,
        cs_sim::ai::combat::RecoveryAction::ReassignLead,
        "the declared leader-loss path the probe registers but never triggers"
    );
    // And the formation it drives is the probe's own, not the fixture's.
    assert_ne!(
        PROBE_FORMATION,
        cs_sim::ai::combat::FormationId(1),
        "the probe registers its own formation id"
    );
}

/// The probe's stream is a function of `(root_seed, run)` and of nothing else,
/// under the documented domain — so a run is reproducible from a recorded seed
/// and two seeds produce two different distributions.
#[test]
fn accept_f32_d_the_probe_is_reproducible_from_its_root_seed_and_nothing_else() {
    let first = short_report(8, 600, 77);
    let second = short_report(8, 600, 77);
    assert_eq!(
        first, second,
        "the same root seed replays the same probe, bit for bit"
    );

    let other = short_report(8, 600, 78);
    assert_ne!(
        first.runs[0].geometry_fingerprint, other.runs[0].geometry_fingerprint,
        "a different root seed replays a different world"
    );
    // The domain is a fixed, documented constant under the
    // `docs/contracts/CLI-EVIDENCE.md` recipe, not a value derived from the
    // tier: two tiers must draw the same stream.
    assert_eq!(
        first.spec.stream(0).next_u64(),
        second.spec.stream(0).next_u64()
    );
    assert_ne!(
        DIFFICULTY_PROBE_DOMAIN,
        cs_types::random::SYNTHETIC_BODY_DOMAIN,
        "the probe does not consume the synthetic body's stream"
    );
}

/// The domain constant really separates the probe's stream, under the contract
/// recipe: the stream seed is the SplitMix64 output of
/// `root_seed ^ run << 32 ^ DOMAIN`.
///
/// Mixing [`DIFFICULTY_PROBE_DOMAIN`] into the *root* argument as well as
/// passing it as the domain cancels it inside
/// [`SplitMix64::for_domain`] — the probe would then draw exactly the stream a
/// domain-less consumer draws for the same seed, which is the collision the
/// recipe exists to prevent, while every other assertion in this file still
/// passed. So the stream is pinned to the recipe here, and separately shown to
/// *differ* from the domain-free stream.
#[test]
fn accept_f32_d_the_probe_stream_follows_the_documented_domain_recipe() {
    let spec = DifficultyProbeSpec::try_new(
        SYNTHETIC_SESSION,
        600,
        4,
        20_260_903,
        synthetic_recovery_policies(),
    )
    .expect("the probe spec is valid");

    for run in 0..spec.runs_per_tier {
        let recipe_root = spec.root_seed ^ (u64::from(run) << 32);
        assert_eq!(
            spec.stream(run).next_u64(),
            SplitMix64::for_domain(recipe_root, DIFFICULTY_PROBE_DOMAIN).next_u64(),
            "run {run}: the contract recipe, domain applied once"
        );
        assert_ne!(
            spec.stream(run).next_u64(),
            SplitMix64::for_domain(recipe_root, 0).next_u64(),
            "run {run}: the probe's stream is not the domain-free one"
        );
        assert_ne!(
            spec.stream(run).next_u64(),
            SplitMix64::for_domain(
                recipe_root ^ DIFFICULTY_PROBE_DOMAIN,
                DIFFICULTY_PROBE_DOMAIN
            )
            .next_u64(),
            "run {run}: the domain is not mixed into the root seed as well"
        );
    }
}

/// The bounds and the refusal: a spec that would measure nothing, or would run
/// unbounded, is refused by name rather than clamped.
#[test]
fn accept_f32_d_a_probe_spec_that_would_measure_nothing_is_refused_by_name() {
    let policies = synthetic_recovery_policies();
    assert_eq!(
        DifficultyProbeSpec::try_new(SYNTHETIC_SESSION, 600, 0, 1, policies).unwrap_err(),
        cs_sim::ai::combat::CombatError::ProbeWithoutRuns
    );
    assert_eq!(
        DifficultyProbeSpec::try_new(SYNTHETIC_SESSION, 0, 4, 1, policies).unwrap_err(),
        cs_sim::ai::combat::CombatError::ProbeWithoutTicks
    );
    assert_eq!(
        DifficultyProbeSpec::try_new(
            SYNTHETIC_SESSION,
            600,
            cs_sim::ai::combat::MAX_PROBE_RUNS_PER_TIER + 1,
            1,
            policies
        )
        .unwrap_err(),
        cs_sim::ai::combat::CombatError::ProbeTooManyRuns {
            runs: cs_sim::ai::combat::MAX_PROBE_RUNS_PER_TIER + 1,
            max: cs_sim::ai::combat::MAX_PROBE_RUNS_PER_TIER,
        }
    );
    assert_eq!(
        DifficultyProbeSpec::try_new(
            SYNTHETIC_SESSION,
            cs_sim::ai::combat::MAX_PROBE_TICKS + 1,
            4,
            1,
            policies
        )
        .unwrap_err(),
        cs_sim::ai::combat::CombatError::ProbeTooManyTicks {
            ticks: cs_sim::ai::combat::MAX_PROBE_TICKS + 1,
            max: cs_sim::ai::combat::MAX_PROBE_TICKS,
        }
    );
    // Every refusal is legible: an error nobody can read is a way to fail
    // quietly in a log.
    for error in [
        cs_sim::ai::combat::CombatError::ProbeWithoutRuns,
        cs_sim::ai::combat::CombatError::ProbeWithoutTicks,
    ] {
        let text = error.to_string();
        assert!(text.contains("probe"), "{text}");
    }
}

/// The jitter bound is real: the probe's authored perturbation is small enough
/// that no run can move a candidate across the engagement-range gate by
/// itself, which is what lets the range refusal count be a *control* that is
/// identical at every tier.
#[test]
fn accept_f32_d_the_probe_jitter_cannot_move_a_candidate_across_a_gate() {
    let report = report();
    let ranges: Vec<u64> = report.tiers.iter().map(|tier| tier.range_rejects).collect();
    let mut sorted = ranges.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        1,
        "the range gate refused the same candidates at every tier: {ranges:?}"
    );
    assert!(
        ranges[0] > 0,
        "the gate is actually refusing candidates, so the control is meaningful"
    );
    const { assert!(PROBE_LATERAL_JITTER_M < 1_000.0) };
    // The probe's own role vocabulary is the declared one: it decides for an
    // escort, never for a role it invented.
    let assignment =
        RoleAssignment::protecting(synthetic_actor(20), CombatRole::Escort, synthetic_actor(40))
            .expect("the probe's escort assignment is valid");
    assert_eq!(assignment.role(), CombatRole::Escort);
    let _ = synthetic_combat_runtime();
}
