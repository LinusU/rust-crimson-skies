//! Acceptance scenario F32-PROBE-GEOMETRY: the difficulty probe's scenario is
//! sealed as a designed fixture, in code rather than in prose.
//!
//! Task: #570 `F32-PROBE-GEOMETRY`, resolving `f32.d.limit.probe_geometry` from
//! `docs/findings/2026-10-03-f32-d-original-ai-roles-and-difficulty.md`. Task
//! test prefix: `accept_f32_pg_`.
//!
//! Every number in the scenario `probe_difficulties` replays is authored
//! project design — no measured file describes an original AI encounter — so
//! the outcome table is a measurement of **this engine's** per-tier decision
//! behavior. These tests pin the machine-readable marker that keeps a later
//! stage from quoting it as the original's: [`DifficultyProbeSpec`] declares a
//! [`ProbeProvenance`] at construction, [`DifficultyProbeReport`] carries it,
//! and the one reading a designed fixture may not have — its outcome as the
//! original's difficulty evidence — is refused by name.
//!
//! Removing the marker, defaulting the provenance, or letting the as-original
//! read succeed fails the test that names it. Every value here is newly
//! authored synthetic fixture data, never original game data.

use cs_sim::ai::combat::{
    CombatError, CombatRuntime, DifficultyProbeReport, DifficultyProbeSpec, DifficultyTier,
    ProbeDifference, ProbeProvenance, SYNTHETIC_SESSION, synthetic_combat_runtime,
    synthetic_difficulty_probe_spec, synthetic_recovery_policies,
};

/// The probe every scenario here starts from.
fn report() -> DifficultyProbeReport {
    synthetic_combat_runtime()
        .probe_difficulties(&synthetic_difficulty_probe_spec())
        .expect("the synthetic probe replays at every tier")
}

/// The marker: the only provenance a probe may declare today is the designed
/// fixture, the spec declares it at construction, and the report carries it.
///
/// `ProbeProvenance::ALL` is pinned at exactly one variant on purpose: a
/// measured scenario has to arrive as a new variant *together with* the
/// geometry binding it — so this assertion, not a comment, is what fails if
/// "measured" ever appears without its content.
#[test]
fn accept_f32_pg_the_probe_scenario_is_marked_a_designed_fixture() {
    assert_eq!(
        ProbeProvenance::ALL,
        &[ProbeProvenance::DesignedFixture],
        "the designed fixture is the only provenance a spec can declare"
    );
    for provenance in ProbeProvenance::ALL {
        assert!(
            !provenance.is_measured(),
            "{provenance}: no measured encounter content exists"
        );
        assert_eq!(provenance.label(), "designed_fixture");
        assert_eq!(provenance.to_string(), "designed_fixture");
    }

    // The declaration is part of the spec itself — the caller names it at
    // `try_new`, and a spec struct literal needs the same field.
    let spec = synthetic_difficulty_probe_spec();
    assert_eq!(spec.provenance, ProbeProvenance::DesignedFixture);
    assert_eq!(spec.provenance(), ProbeProvenance::DesignedFixture);

    // The report carries the marker it was produced under, so an outcome
    // table never travels without it.
    let report = report();
    assert_eq!(report.provenance(), ProbeProvenance::DesignedFixture);
    assert!(!report.provenance().is_measured());
    assert_eq!(
        report.spec.provenance(),
        report.provenance(),
        "the report's marker is the spec's own declaration"
    );
}

/// The construction site declares the marker: `try_new` takes the provenance,
/// so no spec can exist unmarked — and the report keeps the same marker.
#[test]
fn accept_f32_pg_a_spec_declares_its_provenance_at_construction() {
    let spec = DifficultyProbeSpec::try_new(
        SYNTHETIC_SESSION,
        60,
        2,
        9_001,
        synthetic_recovery_policies(),
        ProbeProvenance::DesignedFixture,
    )
    .expect("the probe spec is valid");
    assert_eq!(spec.provenance, ProbeProvenance::DesignedFixture);

    let report = synthetic_combat_runtime()
        .probe_difficulties(&spec)
        .expect("the probe replays");
    assert_eq!(
        report.provenance(),
        ProbeProvenance::DesignedFixture,
        "the produced report carries the declared marker"
    );
}

/// The refused read: a designed fixture's outcome table measures this
/// engine's per-tier behavior, so the read that would present it as the
/// original's difficulty evidence is refused by name — while the honest
/// reading of the same table stays available.
#[test]
fn accept_f32_pg_a_designed_outcome_cannot_be_read_as_original_evidence() {
    let report = report();

    let error = report
        .tiers_as_original_evidence()
        .expect_err("a designed fixture's outcome is not original evidence");
    assert_eq!(error, CombatError::ProbeOutcomeIsDesigned);
    // The refusal is legible: an error nobody can read is a way to fail
    // quietly in a log.
    let text = error.to_string();
    assert!(text.contains("designed"), "{text}");
    assert!(text.contains("not evidence"), "{text}");

    // The honest reading is untouched: the same report still reports its
    // outcome as this engine's measurement, marker attached.
    assert!(!report.tiers.is_empty());
    assert_eq!(report.provenance(), ProbeProvenance::DesignedFixture);
}

/// The measured tier *order* is a property of the designed fixture's
/// profiles: sealing the scenario does not change what the probe measures,
/// and the order is a project claim — never an original-difficulty claim.
#[test]
fn accept_f32_pg_the_tier_order_is_a_designed_fixture_claim() {
    let report = report();

    assert_eq!(
        report.provenance(),
        ProbeProvenance::DesignedFixture,
        "the order below is measured over designed geometry"
    );
    assert!(report.outcomes_differ());
    assert!(report.protected_answers_never_regress());
    for comparison in &report.comparisons {
        assert_eq!(
            comparison.protected_answers,
            ProbeDifference::Higher,
            "{} -> {} answered more authoritative attacks on the designed fixture",
            comparison.lower.label(),
            comparison.higher.label()
        );
    }

    // Even the tier that stands for no measured step — the designed
    // extension — is reported under the same designed provenance: the marker
    // is about the *scenario*, not about which tier was replayed.
    let elite = report.tier(DifficultyTier::Elite).expect("elite is probed");
    assert!(elite.measured_step.is_none());
    assert_eq!(report.provenance(), ProbeProvenance::DesignedFixture);
}

/// A degenerate roster does not launder the marker: four identical
/// distributions are still a designed fixture's measurement of nothing
/// differing, not original evidence.
#[test]
fn accept_f32_pg_a_collapsed_roster_still_reports_designed_provenance() {
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
    let report = runtime
        .probe_difficulties(&synthetic_difficulty_probe_spec())
        .expect("a degenerate roster still replays");

    assert!(!report.outcomes_differ());
    assert_eq!(report.provenance(), ProbeProvenance::DesignedFixture);
    assert_eq!(
        report.tiers_as_original_evidence().unwrap_err(),
        CombatError::ProbeOutcomeIsDesigned,
        "a probe that measured nothing to compare still cannot be read as original"
    );
}
