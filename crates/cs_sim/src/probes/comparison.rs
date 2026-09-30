//! The holdout comparison: does a candidate trace meet a reference envelope,
//! including the maneuvers it was not fitted to? (F26-A)
//!
//! Spec: `specs/F26-handling-probes-and-original-behavior-calibration.md`,
//! stage `### F26-A`, acceptance test AC01. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`, "Calibration acceptance".
//!
//! [`compare`] is the one place the pass/fail rule lives. It is deliberately
//! **not** "did the fitted numbers match": it walks *every* envelope entry —
//! fitted and held out alike — and a maneuver with no candidate measurement is
//! `NoMeasurement`, which is not a pass. A tuned acceleration curve that
//! matched acceleration but flew an out-of-envelope held-out turn therefore
//! fails AC01, because the held-out entry is compared like any other
//! ([`HandlingAssessment::passes`] is false and
//! [`HandlingAssessment::failures`] names the turn).
//!
//! This stage defines the typed comparison and the rule; running the headless
//! maneuver that produces a real candidate trace is F26-B, and the roster-wide
//! deviation report is F26-C. The synthetic candidate here is supplied as
//! measurements, not produced by an integrator, which is why this stage claims
//! no runtime behavior.

use cs_types::content::Origin;

use crate::flight::tuning::ModelKind;

use super::envelope::{EnvelopeEntry, HandlingError, ReferenceEnvelope, Tolerance};
use super::maneuver::{ProbeKind, ProbeQuantity};

/// One measured quantity from a candidate probe run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProbeMeasurement {
    /// The maneuver the measurement came from.
    pub maneuver: ProbeKind,
    /// The quantity measured.
    pub quantity: ProbeQuantity,
    /// The measured value, in the quantity's canonical unit.
    pub value: f64,
}

impl ProbeMeasurement {
    /// Builds a measurement, refusing a non-finite value.
    ///
    /// # Errors
    ///
    /// [`HandlingError::NonFinite`] naming the field.
    pub fn try_new(
        maneuver: ProbeKind,
        quantity: ProbeQuantity,
        value: f64,
    ) -> Result<Self, HandlingError> {
        let measurement = Self {
            maneuver,
            quantity,
            value,
        };
        measurement.validate()?;
        Ok(measurement)
    }

    /// Checks that the value is finite.
    ///
    /// # Errors
    ///
    /// [`HandlingError::NonFinite`] naming the field.
    pub fn validate(&self) -> Result<(), HandlingError> {
        if self.value.is_finite() {
            Ok(())
        } else {
            Err(HandlingError::NonFinite {
                field: format!("measurement.{}.value", self.quantity.label()),
            })
        }
    }
}

/// A candidate airframe's probe results, ready to compare.
///
/// A trace may measure more than the envelope bounds; extras are ignored by
/// [`compare`] because the envelope has no reference for them. Duplicates are
/// refused, so one `(maneuver, quantity)` row cannot be silently overwritten.
#[derive(Clone, Debug, PartialEq)]
pub struct ProbeTrace {
    /// The airframe the trace was measured on; must match the envelope.
    pub airframe_id: String,
    /// Where the measurements came from.
    pub origin: Origin,
    /// The measured quantities.
    pub measurements: Vec<ProbeMeasurement>,
}

impl ProbeTrace {
    /// Checks the identity, the values and the row uniqueness.
    ///
    /// # Errors
    ///
    /// [`HandlingError::EmptyIdentity`] for a blank airframe id,
    /// [`HandlingError::NonFinite`] for a non-finite value, and
    /// [`HandlingError::DuplicateEntry`] for a repeated
    /// `(maneuver, quantity)` row.
    pub fn validate(&self) -> Result<(), HandlingError> {
        if self.airframe_id.trim().is_empty() {
            return Err(HandlingError::EmptyIdentity {
                field: "trace.airframe_id",
            });
        }
        for (index, measurement) in self.measurements.iter().enumerate() {
            measurement.validate()?;
            if self.measurements[..index].iter().any(|earlier| {
                earlier.maneuver == measurement.maneuver && earlier.quantity == measurement.quantity
            }) {
                return Err(HandlingError::DuplicateEntry {
                    maneuver: measurement.maneuver,
                    quantity: measurement.quantity,
                });
            }
        }
        Ok(())
    }

    /// The measurement for `(maneuver, quantity)`, if the trace carries one.
    #[must_use]
    pub fn measurement(
        &self,
        maneuver: ProbeKind,
        quantity: ProbeQuantity,
    ) -> Option<&ProbeMeasurement> {
        self.measurements
            .iter()
            .find(|row| row.maneuver == maneuver && row.quantity == quantity)
    }
}

/// What one envelope entry's comparison found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VerdictStatus {
    /// The measurement is inside the accepted window.
    WithinEnvelope,
    /// The measurement is outside the accepted window; `deviation` is
    /// `measured - reference`, signed.
    OutOfEnvelope {
        /// The signed difference from the reference value.
        deviation: f64,
    },
    /// The candidate carries no measurement for this entry, so it cannot be
    /// shown to be in the envelope. Never a pass.
    NoMeasurement,
}

/// The comparison of one envelope entry against the candidate.
#[derive(Clone, Debug, PartialEq)]
pub struct ProbeVerdict {
    /// The maneuver compared.
    pub maneuver: ProbeKind,
    /// The quantity compared.
    pub quantity: ProbeQuantity,
    /// Whether the envelope held this entry out of the fit.
    pub held_out: bool,
    /// The reference value.
    pub reference: f64,
    /// The tolerance the reference was compared with.
    pub tolerance: Tolerance,
    /// The candidate's measurement, when it had one.
    pub measured: Option<f64>,
    /// The outcome.
    pub status: VerdictStatus,
}

impl ProbeVerdict {
    /// Whether this entry is inside its accepted window.
    #[must_use]
    pub const fn is_within_envelope(&self) -> bool {
        matches!(self.status, VerdictStatus::WithinEnvelope)
    }
}

/// The result of comparing one candidate trace against one reference envelope.
///
/// `passes` is true only when **every** envelope entry was measured **and**
/// inside its accepted window. A missing measurement or an out-of-envelope
/// entry makes it false, so neither can be read as a pass. This structural
/// result is separate from the evidence question: only an original envelope
/// met by an original-observed candidate supports an original-fidelity claim
/// ([`HandlingAssessment::supports_original_fidelity_claim`]).
#[derive(Clone, Debug, PartialEq)]
pub struct HandlingAssessment {
    /// The envelope's airframe id.
    pub envelope_airframe: String,
    /// The candidate's airframe id.
    pub candidate_airframe: String,
    /// Where the envelope came from.
    pub envelope_origin: Origin,
    /// Where the candidate came from.
    pub candidate_origin: Origin,
    /// Which control law was compared.
    pub model_kind: ModelKind,
    /// One verdict per envelope entry, in envelope order.
    pub verdicts: Vec<ProbeVerdict>,
}

impl HandlingAssessment {
    /// Whether every envelope entry was measured and inside its window.
    #[must_use]
    pub fn passes(&self) -> bool {
        !self.verdicts.is_empty() && self.verdicts.iter().all(ProbeVerdict::is_within_envelope)
    }

    /// One diagnostic line per out-of-envelope entry.
    #[must_use]
    pub fn failures(&self) -> Vec<&ProbeVerdict> {
        self.verdicts
            .iter()
            .filter(|verdict| matches!(verdict.status, VerdictStatus::OutOfEnvelope { .. }))
            .collect()
    }

    /// One diagnostic line per entry the candidate did not measure.
    #[must_use]
    pub fn unavailable(&self) -> Vec<&ProbeVerdict> {
        self.verdicts
            .iter()
            .filter(|verdict| matches!(verdict.status, VerdictStatus::NoMeasurement))
            .collect()
    }

    /// The verdicts for entries the envelope held out of the fit.
    #[must_use]
    pub fn holdout_verdicts(&self) -> Vec<&ProbeVerdict> {
        self.verdicts
            .iter()
            .filter(|verdict| verdict.held_out)
            .collect()
    }

    /// Whether every held-out entry was measured and inside its window.
    #[must_use]
    pub fn holdout_passes(&self) -> bool {
        let held_out = self.holdout_verdicts();
        !held_out.is_empty() && held_out.iter().all(|verdict| verdict.is_within_envelope())
    }

    /// Whether this assessment can back a claim about original behavior.
    ///
    /// It needs both an original-installation envelope and an
    /// original-observed candidate, and it needs [`Self::passes`]: a synthetic
    /// fixture can never support an original-fidelity claim however complete it
    /// is.
    #[must_use]
    pub fn supports_original_fidelity_claim(&self) -> bool {
        self.envelope_origin.is_original() && self.candidate_origin.is_original() && self.passes()
    }
}

/// Compares a candidate trace against a reference envelope under the holdout
/// rule.
///
/// Both sides are validated first: an invalid envelope or trace is a defect,
/// not an out-of-envelope result. Then every envelope entry is compared — a
/// candidate that is missing an entry, or outside its accepted window, does not
/// pass. The entries the envelope held out are compared exactly like the fitted
/// ones, which is what stops a tuned acceleration curve from passing when its
/// held-out turn radius is outside the envelope (AC01).
///
/// # Errors
///
/// [`HandlingError`] from [`ReferenceEnvelope::validate`] or
/// [`ProbeTrace::validate`], and [`HandlingError::AirframeMismatch`] when the
/// candidate was measured on a different airframe than the envelope.
pub fn compare(
    envelope: &ReferenceEnvelope,
    trace: &ProbeTrace,
) -> Result<HandlingAssessment, HandlingError> {
    envelope.validate()?;
    trace.validate()?;
    if envelope.airframe_id != trace.airframe_id {
        return Err(HandlingError::AirframeMismatch {
            envelope: envelope.airframe_id.clone(),
            candidate: trace.airframe_id.clone(),
        });
    }

    let verdicts = envelope
        .entries
        .iter()
        .map(|entry| compare_entry(entry, trace))
        .collect();

    Ok(HandlingAssessment {
        envelope_airframe: envelope.airframe_id.clone(),
        candidate_airframe: trace.airframe_id.clone(),
        envelope_origin: envelope.origin.clone(),
        candidate_origin: trace.origin.clone(),
        model_kind: envelope.model_kind,
        verdicts,
    })
}

/// Compares one envelope entry against the candidate's matching measurement.
fn compare_entry(entry: &EnvelopeEntry, trace: &ProbeTrace) -> ProbeVerdict {
    let measured = trace
        .measurement(entry.maneuver, entry.quantity)
        .map(|row| row.value);
    let status = match measured {
        None => VerdictStatus::NoMeasurement,
        Some(value) => {
            let deviation = value - entry.reference;
            if deviation.abs() <= entry.tolerance.plus_minus {
                VerdictStatus::WithinEnvelope
            } else {
                VerdictStatus::OutOfEnvelope { deviation }
            }
        }
    };
    ProbeVerdict {
        maneuver: entry.maneuver,
        quantity: entry.quantity,
        held_out: entry.held_out,
        reference: entry.reference,
        tolerance: entry.tolerance.clone(),
        measured,
        status,
    }
}
