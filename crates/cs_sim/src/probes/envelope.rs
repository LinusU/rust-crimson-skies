//! The reference envelope: a provenance-carrying record of original handling
//! measurements and the tolerance a candidate must meet (F26-A).
//!
//! Spec: `specs/F26-handling-probes-and-original-behavior-calibration.md`,
//! stage `### F26-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`,
//! sections "Inputs and outputs" and "Calibration acceptance".
//!
//! The sheet's deliverable is explicit: "A ReferenceEnvelope records original
//! input, initial state, difficulty, loadout, timing uncertainty and units."
//! [`ReferenceEnvelope`] carries exactly those, and [`EnvelopeEntry`] carries
//! the recorded input schedule, the initial state, the reference measurement
//! and the tolerance selected before the fit.
//!
//! Non-negotiable behavior 2 ("Original executable frame pacing and capture
//! timing can introduce uncertainty. Record it rather than claiming exact
//! original tick rate") is a required field here:
//! [`ReferenceEnvelope::timing_uncertainty`] must carry a measured
//! `plus_minus_s` **and** the method that produced it; a blank method is a
//! defect ([`HandlingError::BlankTimingMethod`]), not a default.
//!
//! Non-negotiable behavior 1 ("... reserve holdout probes to catch
//! overfitting") is enforced structurally: at least one entry must be
//! [`EnvelopeEntry::held_out`], or the envelope is refused
//! ([`HandlingError::NoHeldOutEntry`]). The comparison that consumes this
//! record and refuses a candidate that only matched the fitted entries is
//! [`super::compare`].
//!
//! **Synthetic is never original.** [`ReferenceEnvelope::is_original_reference`]
//! is true only for an [`Origin::Installation`] envelope, so the synthetic
//! fixture cannot be presented as a retail reference no matter what status a
//! caller sets. The capture protocol that produced a real envelope (fingerprints,
//! artifacts, observer) is `cs_inspect::reference_capture` (#357); this module
//! is the simulator-side schema the comparison reads.
//!
//! This is the F25-A `ReferenceManeuverEnvelope`'s sibling, not its
//! replacement: that record says *which* maneuvers a calibration must contain;
//! this one carries *what the original measured*, in which unit, under which
//! recorded conditions, with which tolerance.

use cs_types::content::Origin;

use crate::flight::tuning::ModelKind;
use crate::flight::{FlightInput, FlightInputError};

use super::maneuver::{ProbeKind, ProbeQuantity};

/// Why a reference envelope or a probe measurement was refused.
///
/// Every variant names the offending field or entry, and no value is repaired
/// into a plausible one (`FLIGHT-PHYSICS`: "Reject nonfinite inputs at
/// boundaries ... do not silently clamp corrupted tuning into plausible
/// values").
#[derive(Clone, Debug, PartialEq)]
pub enum HandlingError {
    /// An identity field was empty or only whitespace.
    EmptyIdentity {
        /// The offending field.
        field: &'static str,
    },
    /// The timing uncertainty stated no measurement method.
    BlankTimingMethod,
    /// A declared input step time did not strictly increase.
    NonIncreasingInputTime {
        /// The maneuver whose schedule was refused.
        maneuver: ProbeKind,
    },
    /// A named field contained NaN or infinity.
    NonFinite {
        /// The offending field.
        field: String,
    },
    /// A field that must not be negative was negative.
    Negative {
        /// The offending field.
        field: String,
    },
    /// A fraction fell outside `[0, 1]`.
    OutOfRange {
        /// The offending field.
        field: String,
        /// The rejected value.
        value: f64,
    },
    /// The envelope declared no entry at all.
    NoEntries,
    /// The same `(maneuver, quantity)` pair was declared twice.
    DuplicateEntry {
        /// The duplicated maneuver.
        maneuver: ProbeKind,
        /// The duplicated quantity.
        quantity: ProbeQuantity,
    },
    /// An entry declared the wrong unit for its quantity.
    UnitMismatch {
        /// The quantity whose canonical unit was not declared.
        quantity: ProbeQuantity,
        /// The unit the entry declared.
        declared: String,
    },
    /// An entry scheduled no input at all.
    EmptyInputSchedule {
        /// The offending maneuver.
        maneuver: ProbeKind,
    },
    /// The declared tolerance was not strictly positive, which would be an
    /// exact float comparison the contract forbids.
    NonPositiveTolerance {
        /// The offending maneuver.
        maneuver: ProbeKind,
    },
    /// The declared tolerance carried no rationale, so it is not justified.
    BlankToleranceRationale {
        /// The offending maneuver.
        maneuver: ProbeKind,
    },
    /// No entry was held out of the fit, so a fitted candidate could pass by
    /// matching only the data it was fitted to.
    NoHeldOutEntry,
    /// The candidate was measured on a different airframe than the envelope.
    AirframeMismatch {
        /// The envelope's airframe id.
        envelope: String,
        /// The candidate's airframe id.
        candidate: String,
    },
}

impl std::fmt::Display for HandlingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyIdentity { field } => write!(f, "{field} must not be empty"),
            Self::BlankTimingMethod => {
                write!(f, "the timing uncertainty states no measurement method")
            }
            Self::NonIncreasingInputTime { maneuver } => write!(
                f,
                "the {} input schedule is not strictly increasing in time",
                maneuver.label()
            ),
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::Negative { field } => write!(f, "{field} must not be negative"),
            Self::OutOfRange { field, value } => {
                write!(f, "{field} value {value} is outside [0, 1]")
            }
            Self::NoEntries => write!(f, "a reference envelope must declare at least one entry"),
            Self::DuplicateEntry { maneuver, quantity } => write!(
                f,
                "the {} entry for {} is declared more than once",
                maneuver.label(),
                quantity.label()
            ),
            Self::UnitMismatch { quantity, declared } => write!(
                f,
                "the unit {declared:?} is not the declared unit {} for {}",
                quantity.unit(),
                quantity.label()
            ),
            Self::EmptyInputSchedule { maneuver } => {
                write!(f, "the {} entry schedules no input", maneuver.label())
            }
            Self::NonPositiveTolerance { maneuver } => write!(
                f,
                "the {} tolerance is not greater than zero",
                maneuver.label()
            ),
            Self::BlankToleranceRationale { maneuver } => {
                write!(f, "the {} tolerance states no rationale", maneuver.label())
            }
            Self::NoHeldOutEntry => write!(
                f,
                "a reference envelope must hold out at least one maneuver from the fit"
            ),
            Self::AirframeMismatch {
                envelope,
                candidate,
            } => write!(
                f,
                "the candidate airframe {candidate:?} does not match the envelope airframe {envelope:?}"
            ),
        }
    }
}

impl std::error::Error for HandlingError {}

impl From<FlightInputError> for HandlingError {
    fn from(error: FlightInputError) -> Self {
        match error {
            FlightInputError::NonFinite { field } => Self::NonFinite {
                field: field.to_owned(),
            },
            FlightInputError::OutOfRange { field, value, .. } => Self::OutOfRange {
                field: field.to_owned(),
                value,
            },
        }
    }
}

fn check_finite(field: &str, value: f64) -> Result<(), HandlingError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(HandlingError::NonFinite {
            field: field.to_owned(),
        })
    }
}

/// The measured timing uncertainty of a capture, in seconds.
///
/// A half-width without a stated method is not a measurement, so the method
/// must be non-blank. This is what keeps "record the uncertainty rather than
/// claim an exact original tick rate" honest.
#[derive(Clone, Debug, PartialEq)]
pub struct TimingUncertainty {
    /// Half-width of the measured uncertainty, in seconds.
    pub plus_minus_s: f64,
    /// How the uncertainty was measured (frame-pacing probe, tool timestamp
    /// spread, ...). Must be non-blank.
    pub method: String,
}

impl TimingUncertainty {
    /// Checks the half-width and the stated method.
    ///
    /// # Errors
    ///
    /// [`HandlingError::NonFinite`] for a non-finite half-width,
    /// [`HandlingError::Negative`] for a negative one, and
    /// [`HandlingError::BlankTimingMethod`] when no method is stated.
    pub fn validate(&self) -> Result<(), HandlingError> {
        check_finite("timing_uncertainty.plus_minus_s", self.plus_minus_s)?;
        if self.plus_minus_s < 0.0 {
            return Err(HandlingError::Negative {
                field: "timing_uncertainty.plus_minus_s".to_owned(),
            });
        }
        if self.method.trim().is_empty() {
            return Err(HandlingError::BlankTimingMethod);
        }
        Ok(())
    }
}

/// A tolerance with the reason it was chosen.
///
/// `plus_minus` must be strictly positive: a zero tolerance is the blanket
/// exact float equality the sheet forbids ("Comparisons use justified
/// tolerances, not a blanket exact float equality"). The rationale must be
/// non-blank so the tolerance is recorded as a choice, not a default.
#[derive(Clone, Debug, PartialEq)]
pub struct Tolerance {
    /// The accepted half-width around the reference value, in the entry's unit.
    pub plus_minus: f64,
    /// Why this tolerance was selected, recorded before the fit.
    pub rationale: String,
}

/// The declared initial state a maneuver starts from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProbeInitialState {
    /// True airspeed at the start of the run, in m/s.
    pub airspeed_mps: f64,
    /// World-vertical speed at the start, in m/s (positive is up).
    pub vertical_speed_mps: f64,
    /// Altitude at the start, in metres.
    pub altitude_m: f64,
    /// Engine spool at the start, in `[0, 1]`.
    pub engine_spool: f64,
    /// Whether boost capacity is available at the start.
    pub boost_available: bool,
}

impl ProbeInitialState {
    /// Level, unpowered rest at sea level.
    pub const AT_REST: Self = Self {
        airspeed_mps: 0.0,
        vertical_speed_mps: 0.0,
        altitude_m: 0.0,
        engine_spool: 0.0,
        boost_available: false,
    };

    /// Checks the declared bounds.
    ///
    /// # Errors
    ///
    /// [`HandlingError::NonFinite`], [`HandlingError::Negative`] for a
    /// negative airspeed or altitude, and [`HandlingError::OutOfRange`] for an
    /// engine spool outside `[0, 1]`.
    pub fn validate(&self) -> Result<(), HandlingError> {
        for (field, value) in [
            ("initial_state.airspeed_mps", self.airspeed_mps),
            ("initial_state.vertical_speed_mps", self.vertical_speed_mps),
            ("initial_state.altitude_m", self.altitude_m),
            ("initial_state.engine_spool", self.engine_spool),
        ] {
            check_finite(field, value)?;
        }
        if self.airspeed_mps < 0.0 {
            return Err(HandlingError::Negative {
                field: "initial_state.airspeed_mps".to_owned(),
            });
        }
        if self.altitude_m < 0.0 {
            return Err(HandlingError::Negative {
                field: "initial_state.altitude_m".to_owned(),
            });
        }
        if !(0.0..=1.0).contains(&self.engine_spool) {
            return Err(HandlingError::OutOfRange {
                field: "initial_state.engine_spool".to_owned(),
                value: self.engine_spool,
            });
        }
        Ok(())
    }
}

/// One step of a maneuver's recorded input schedule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProbeInputStep {
    /// When this input is applied, in seconds from the start of the maneuver.
    pub at_s: f64,
    /// The normalized command held from this time.
    pub input: FlightInput,
}

impl ProbeInputStep {
    /// Builds a step, validating the time and the command.
    ///
    /// # Errors
    ///
    /// [`HandlingError::NonFinite`] or [`HandlingError::Negative`] for the
    /// time, and [`HandlingError`] from [`FlightInput::validate`] for the
    /// command.
    pub fn try_new(at_s: f64, input: FlightInput) -> Result<Self, HandlingError> {
        check_finite("input.at_s", at_s)?;
        if at_s < 0.0 {
            return Err(HandlingError::Negative {
                field: "input.at_s".to_owned(),
            });
        }
        input.validate()?;
        Ok(Self { at_s, input })
    }
}

/// One bounded quantity of one maneuver's original reference trace.
#[derive(Clone, Debug, PartialEq)]
pub struct EnvelopeEntry {
    /// The maneuver this entry describes.
    pub maneuver: ProbeKind,
    /// The measured quantity.
    pub quantity: ProbeQuantity,
    /// The unit the reference value carries; must be the quantity's unit.
    pub unit: String,
    /// The initial state the maneuver was run from.
    pub initial_state: ProbeInitialState,
    /// The recorded input schedule, non-empty and strictly increasing in time.
    pub input: Vec<ProbeInputStep>,
    /// The reference measurement of the original, in `unit`.
    pub reference: f64,
    /// The tolerance selected before the fit.
    pub tolerance: Tolerance,
    /// Whether this entry is held out of the fit.
    pub held_out: bool,
}

impl EnvelopeEntry {
    /// The inclusive accepted window around the reference value.
    #[must_use]
    pub fn accepted_window(&self) -> [f64; 2] {
        [
            self.reference - self.tolerance.plus_minus,
            self.reference + self.tolerance.plus_minus,
        ]
    }

    fn validate(&self) -> Result<(), HandlingError> {
        if self.unit != self.quantity.unit() {
            return Err(HandlingError::UnitMismatch {
                quantity: self.quantity,
                declared: self.unit.clone(),
            });
        }
        self.initial_state.validate()?;
        if self.input.is_empty() {
            return Err(HandlingError::EmptyInputSchedule {
                maneuver: self.maneuver,
            });
        }
        for (index, step) in self.input.iter().enumerate() {
            step.input.validate()?;
            check_finite("input.at_s", step.at_s)?;
            if step.at_s < 0.0 {
                return Err(HandlingError::Negative {
                    field: "input.at_s".to_owned(),
                });
            }
            if index > 0 && step.at_s <= self.input[index - 1].at_s {
                return Err(HandlingError::NonIncreasingInputTime {
                    maneuver: self.maneuver,
                });
            }
        }
        check_finite("entry.reference", self.reference)?;
        check_finite("tolerance.plus_minus", self.tolerance.plus_minus)?;
        if self.tolerance.plus_minus <= 0.0 {
            return Err(HandlingError::NonPositiveTolerance {
                maneuver: self.maneuver,
            });
        }
        if self.tolerance.rationale.trim().is_empty() {
            return Err(HandlingError::BlankToleranceRationale {
                maneuver: self.maneuver,
            });
        }
        Ok(())
    }
}

/// The original reference trace of one airframe and loadout.
///
/// See the module docs for what each required field is and why. Construction
/// does not validate: call [`ReferenceEnvelope::validate`] at the boundary, or
/// [`super::compare`], which validates both sides.
#[derive(Clone, Debug, PartialEq)]
pub struct ReferenceEnvelope {
    /// The airframe this envelope belongs to.
    pub airframe_id: String,
    /// Which control law the envelope describes.
    pub model_kind: ModelKind,
    /// The difficulty the reference was captured under, as recorded.
    pub difficulty: String,
    /// The loadout the reference was captured with, as recorded.
    pub loadout: String,
    /// The measured timing uncertainty of the capture.
    pub timing_uncertainty: TimingUncertainty,
    /// The bounded entries of the trace.
    pub entries: Vec<EnvelopeEntry>,
    /// Where the envelope came from; only an installation origin is a real
    /// reference.
    pub origin: Origin,
}

impl ReferenceEnvelope {
    /// The maneuvers the sheet names that this envelope does not bound.
    ///
    /// Coverage is reported, not guessed: a missing maneuver is listed here
    /// rather than silently unmeasured. F26-C's roster audit consumes this.
    #[must_use]
    pub fn missing_maneuvers(&self) -> Vec<ProbeKind> {
        ProbeKind::ALL
            .into_iter()
            .filter(|maneuver| !self.entries.iter().any(|entry| entry.maneuver == *maneuver))
            .collect()
    }

    /// Whether every maneuver the sheet names has an entry.
    #[must_use]
    pub fn covers_every_maneuver(&self) -> bool {
        self.missing_maneuvers().is_empty()
    }

    /// Whether an original installation trace backs this envelope.
    ///
    /// A synthetic fixture is never an original reference, however complete it
    /// is, so this cannot be promoted by filling in fields.
    #[must_use]
    pub fn is_original_reference(&self) -> bool {
        self.origin.is_original()
    }

    /// Checks the identity, coverage and per-entry bounds.
    ///
    /// # Errors
    ///
    /// [`HandlingError`] naming the first problem: an empty identity, a blank
    /// timing method, a non-finite or negative value, no entries, a duplicate
    /// `(maneuver, quantity)` pair, a wrong unit, an empty or non-increasing
    /// input schedule, a non-positive or unjustified tolerance, or no held-out
    /// entry. A missing maneuver is **not** an error here — it is reported by
    /// [`ReferenceEnvelope::missing_maneuvers`] — but a missing holdout is.
    pub fn validate(&self) -> Result<(), HandlingError> {
        for (field, value) in [
            ("airframe_id", self.airframe_id.as_str()),
            ("difficulty", self.difficulty.as_str()),
            ("loadout", self.loadout.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(HandlingError::EmptyIdentity { field });
            }
        }
        self.timing_uncertainty.validate()?;
        if self.entries.is_empty() {
            return Err(HandlingError::NoEntries);
        }

        for (index, entry) in self.entries.iter().enumerate() {
            if self.entries[..index].iter().any(|earlier| {
                earlier.maneuver == entry.maneuver && earlier.quantity == entry.quantity
            }) {
                return Err(HandlingError::DuplicateEntry {
                    maneuver: entry.maneuver,
                    quantity: entry.quantity,
                });
            }
            entry.validate()?;
        }

        if !self.entries.iter().any(|entry| entry.held_out) {
            return Err(HandlingError::NoHeldOutEntry);
        }
        Ok(())
    }
}
