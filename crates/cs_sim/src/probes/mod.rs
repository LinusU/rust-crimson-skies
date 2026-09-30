//! Handling telemetry and reference-envelope schemas (F26-A).
//!
//! Spec: `specs/F26-handling-probes-and-original-behavior-calibration.md`,
//! stage `### F26-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! F26 exists to calibrate the flight model against the original game instead
//! of trusting one top-speed number. This stage defines the typed boundary and
//! the comparison rule; it does not fly anything.
//!
//! * [`maneuver`] is the closed probe vocabulary: the ten maneuvers the sheet
//!   names ([`ProbeKind`]) and the quantity each one's envelope primarily
//!   bounds ([`ProbeQuantity`]), with canonical units.
//! * [`envelope`] is the provenance-carrying [`ReferenceEnvelope`]: the
//!   recorded input schedule, initial state, difficulty, loadout, timing
//!   uncertainty and units of an original handling trace, plus the tolerance
//!   selected before the fit. At least one entry must be held out, so a
//!   candidate cannot pass by matching only the data it was fitted to.
//! * [`comparison`] is the holdout comparison. [`compare`] walks every entry —
//!   fitted and held out alike — and a missing measurement is never a pass.
//!   `passes()` is true only when every entry was measured inside its accepted
//!   window. The structural result is separate from evidence:
//!   [`HandlingAssessment::supports_original_fidelity_claim`] additionally
//!   requires an original-installation envelope and an original-observed
//!   candidate.
//! * [`synthetic`] is the minimal declared fixture and the two candidates AC01
//!   turns on.
//!
//! **The contract's minimum scenario** is
//! `accept_f26_a_tuned_acceleration_curve_cannot_pass_with_a_held_out_turn_outside_the_envelope`:
//! [`synthetic_acceleration_tuned_turn_outside`] matches the fitted
//! acceleration entry but flies a held-out turn radius outside its window, and
//! [`compare`] must refuse it because [`synthetic_reference_envelope`] holds
//! the turn out.
//!
//! **Nothing here is original data.** The fixture's origin is
//! [`Origin::SyntheticFixture`](cs_types::content::Origin::SyntheticFixture),
//! so it cannot be mistaken for a reference trace. The original's exact
//! equations, units, tick rate and the tolerance each real envelope needs are
//! unknown (`F26` "Research boundary"); the capture protocol that produces a
//! real envelope is `cs_inspect::reference_capture` (#357), and running the
//! headless probes that produce a real candidate is F26-B.
//!
//! **Three records must not be conflated.** F25-A's
//! `flight::autogyro::ReferenceManeuverEnvelope` says *which* maneuvers a
//! calibration must contain. `cs_inspect::reference_capture::ReferenceSet` is
//! the operator's capture worksheet and holdout reservation. This module's
//! [`ReferenceEnvelope`] is the simulator-side schema that carries *what the
//! original measured*, in which unit, under which recorded conditions, and
//! [`compare`] reads it.

pub mod comparison;
pub mod envelope;
pub mod maneuver;
pub mod runner;
pub mod synthetic;

pub use comparison::{
    HandlingAssessment, ProbeMeasurement, ProbeTrace, ProbeVerdict, VerdictStatus, compare,
};
pub use envelope::{
    EnvelopeEntry, HandlingError, ProbeInitialState, ProbeInputStep, ReferenceEnvelope,
    TimingUncertainty, Tolerance,
};
pub use maneuver::{ProbeKind, ProbeQuantity};
pub use runner::{DEFAULT_PROBE_DT_S, EnvelopeRun, ProbeError, ProbeRun, ProbeRunner, horizon_s};
pub use synthetic::{
    SYNTHETIC_HANDLING_AIRFRAME, synthetic_acceleration_tuned_turn_outside,
    synthetic_covering_assessment, synthetic_covering_trace, synthetic_out_of_envelope_assessment,
    synthetic_reference_envelope,
};

#[cfg(test)]
mod tests {
    use super::*;

    /// The synthetic fixture validates, covers every sheet maneuver and holds
    /// exactly the turn out of the fit.
    #[test]
    fn accept_f26_a_synthetic_envelope_covers_every_maneuver_and_holds_one_out() {
        let envelope = synthetic_reference_envelope();
        assert_eq!(envelope.validate(), Ok(()));
        assert!(envelope.covers_every_maneuver());
        assert_eq!(envelope.missing_maneuvers(), Vec::new());
        assert!(
            !envelope.is_original_reference(),
            "a synthetic fixture is never an original reference"
        );

        let held_out: Vec<ProbeKind> = envelope
            .entries
            .iter()
            .filter(|entry| entry.held_out)
            .map(|entry| entry.maneuver)
            .collect();
        assert_eq!(held_out, vec![ProbeKind::Turn]);
    }

    /// AC01: a tuned acceleration curve cannot pass when its held-out turn
    /// radius is outside the envelope.
    #[test]
    fn accept_f26_a_tuned_acceleration_curve_cannot_pass_with_a_held_out_turn_outside_the_envelope()
    {
        let assessment =
            synthetic_out_of_envelope_assessment().expect("the declared fixture is valid");

        assert!(
            !assessment.passes(),
            "an out-of-envelope held-out turn must not pass"
        );
        let failures = assessment.failures();
        assert_eq!(failures.len(), 1, "only the turn is outside its window");
        let turn = failures[0];
        assert_eq!(turn.maneuver, ProbeKind::Turn);
        assert!(
            turn.held_out,
            "the failing entry is the held-out one, not a fitted one"
        );
        assert_eq!(
            turn.status,
            VerdictStatus::OutOfEnvelope { deviation: 40.0 }
        );

        // The fitted acceleration entry is inside its window: the candidate
        // fails on the holdout, not on the fit.
        let acceleration = assessment
            .verdicts
            .iter()
            .find(|verdict| verdict.maneuver == ProbeKind::Acceleration)
            .expect("the acceleration entry is compared");
        assert_eq!(acceleration.status, VerdictStatus::WithinEnvelope);
        assert!(!acceleration.held_out);
        assert!(!assessment.holdout_passes());

        assert!(
            !assessment.supports_original_fidelity_claim(),
            "a synthetic fixture cannot back an original-fidelity claim"
        );
    }

    /// The covering candidate passes, and the difference between the two
    /// outcomes really is the held-out turn.
    #[test]
    fn accept_f26_a_covering_candidate_passes_and_is_not_original_fidelity() {
        let assessment = synthetic_covering_assessment().expect("the declared fixture is valid");
        assert!(assessment.passes());
        assert!(assessment.failures().is_empty());
        assert!(assessment.unavailable().is_empty());
        assert!(assessment.holdout_passes());
        assert!(!assessment.supports_original_fidelity_claim());
    }
}
