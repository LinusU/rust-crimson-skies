//! The declared synthetic handling fixture: a reference envelope and two
//! candidate traces (F26-A).
//!
//! **Designed, not original.** Every value here is newly authored project
//! design chosen to exercise the schema and the holdout rule. The envelope's
//! origin is [`Origin::SyntheticFixture`], so
//! [`ReferenceEnvelope::is_original_reference`] is false and no candidate can
//! support an original-fidelity claim. F26-D replaces this fixture with
//! envelopes fitted to fingerprinted original captures.
//!
//! Two candidates exercise the two outcomes AC01 turns on:
//!
//! * [`synthetic_covering_trace`] measures every entry inside its window, so
//!   [`compare`] passes it.
//! * [`synthetic_acceleration_tuned_turn_outside`] matches the fitted
//!   acceleration entry but has a held-out turn radius outside its window, so
//!   [`compare`] must **not** pass it — the minimum scenario.

use cs_types::content::Origin;

use crate::flight::FlightInput;
use crate::flight::tuning::ModelKind;

use super::comparison::{HandlingAssessment, ProbeMeasurement, ProbeTrace, compare};
use super::envelope::{
    EnvelopeEntry, HandlingError, ProbeInitialState, ProbeInputStep, ReferenceEnvelope,
    TimingUncertainty, Tolerance,
};
use super::maneuver::ProbeKind;

/// The synthetic airframe id shared by the fixture's envelope and traces.
pub const SYNTHETIC_HANDLING_AIRFRAME: &str = "fixture.synthetic-handling";

/// The synthetic reference envelope: every sheet maneuver, a held-out turn and
/// no original reference.
///
/// The turn is held out because the sheet's minimum scenario reserves a turn
/// radius; the rest is authored design data standing in for `FLIGHT-PHYSICS`
/// "Calibration acceptance"'s per-airframe maneuver list.
#[must_use]
pub fn synthetic_reference_envelope() -> ReferenceEnvelope {
    let spec = |maneuver: ProbeKind,
                reference: f64,
                tolerance: f64,
                initial_state: ProbeInitialState,
                input: FlightInput,
                held_out: bool| {
        entry(
            maneuver,
            reference,
            tolerance,
            "fixture: authored tolerance, not selected from a fit",
            initial_state,
            input,
            held_out,
        )
    };

    let cruise = ProbeInitialState {
        airspeed_mps: 60.0,
        ..ProbeInitialState::AT_REST
    };
    let stalled = ProbeInitialState {
        airspeed_mps: 12.0,
        engine_spool: 0.4,
        ..ProbeInitialState::AT_REST
    };
    let rest = ProbeInitialState {
        engine_spool: 0.0,
        ..ProbeInitialState::AT_REST
    };
    let accelerating = FlightInput::try_new(0.0, 0.0, 0.0, 1.0, false).expect("valid");
    let coasting = FlightInput::try_new(0.0, 0.0, 0.0, 0.0, false).expect("valid");
    let climbing = FlightInput::try_new(0.3, 0.0, 0.0, 1.0, false).expect("valid");
    let diving = FlightInput::try_new(-0.3, 0.0, 0.0, 0.6, false).expect("valid");
    let turning = FlightInput::try_new(0.0, 0.6, 0.25, 0.9, false).expect("valid");
    let rolling = FlightInput::try_new(0.0, 1.0, 0.0, 0.7, false).expect("valid");
    let yawing = FlightInput::try_new(0.0, 0.0, 1.0, 0.7, false).expect("valid");
    let stalled_input = FlightInput::try_new(-1.0, 0.0, 0.0, 1.0, false).expect("valid");
    let damaged = FlightInput::try_new(0.0, 0.5, 0.0, 0.8, false).expect("valid");
    let boosting = FlightInput::try_new(0.0, 0.0, 0.0, 1.0, true).expect("valid");

    ReferenceEnvelope {
        airframe_id: SYNTHETIC_HANDLING_AIRFRAME.to_owned(),
        model_kind: ModelKind::FixedWing,
        difficulty: "fixture difficulty (authored)".to_owned(),
        loadout: "fixture empty loadout (authored)".to_owned(),
        timing_uncertainty: TimingUncertainty {
            plus_minus_s: 0.001,
            method: "fixture: authored, not measured".to_owned(),
        },
        entries: vec![
            spec(
                ProbeKind::Acceleration,
                25.0,
                4.0,
                rest,
                accelerating,
                false,
            ),
            spec(ProbeKind::CoastDown, 18.0, 3.0, cruise, coasting, false),
            spec(ProbeKind::Climb, 8.0, 1.5, cruise, climbing, false),
            spec(ProbeKind::Dive, 40.0, 6.0, cruise, diving, false),
            spec(ProbeKind::Turn, 120.0, 15.0, cruise, turning, true),
            spec(ProbeKind::Roll, 1.8, 0.3, cruise, rolling, false),
            spec(ProbeKind::Yaw, 0.6, 0.15, cruise, yawing, false),
            spec(
                ProbeKind::StallRecovery,
                2.5,
                0.5,
                stalled,
                stalled_input,
                false,
            ),
            spec(ProbeKind::Damage, 0.5, 0.1, cruise, damaged, false),
            spec(ProbeKind::Boost, 12.0, 2.5, cruise, boosting, false),
        ],
        origin: Origin::SyntheticFixture,
    }
}

/// A candidate that measures every entry inside its accepted window.
#[must_use]
pub fn synthetic_covering_trace() -> ProbeTrace {
    let envelope = synthetic_reference_envelope();
    let measurements = envelope
        .entries
        .iter()
        .map(|entry| measurement(entry.maneuver, entry.reference))
        .collect();
    ProbeTrace {
        airframe_id: SYNTHETIC_HANDLING_AIRFRAME.to_owned(),
        origin: Origin::SyntheticFixture,
        measurements,
    }
}

/// A **tuned acceleration curve** candidate: acceleration is inside its window,
/// but the held-out turn radius is far outside its window.
///
/// This is the minimum scenario: a candidate fitted to acceleration cannot pass
/// on that fit alone, because the held-out turn is compared too.
#[must_use]
pub fn synthetic_acceleration_tuned_turn_outside() -> ProbeTrace {
    let envelope = synthetic_reference_envelope();
    let measurements = envelope
        .entries
        .iter()
        .map(|entry| {
            let value = match entry.maneuver {
                // Fitted, and inside its window.
                ProbeKind::Acceleration => entry.reference + 1.0,
                // Held out, and far outside its window (radius 160 m vs a
                // 120 m ± 15 m accepted window).
                ProbeKind::Turn => 160.0,
                _ => entry.reference,
            };
            measurement(entry.maneuver, value)
        })
        .collect();
    ProbeTrace {
        airframe_id: SYNTHETIC_HANDLING_AIRFRAME.to_owned(),
        origin: Origin::SyntheticFixture,
        measurements,
    }
}

/// Compares [`synthetic_covering_trace`] against the synthetic envelope.
///
/// # Errors
///
/// [`HandlingError`] from [`compare`]; the declared fixture is valid.
pub fn synthetic_covering_assessment() -> Result<HandlingAssessment, HandlingError> {
    compare(&synthetic_reference_envelope(), &synthetic_covering_trace())
}

/// Compares [`synthetic_acceleration_tuned_turn_outside`] against the synthetic
/// envelope.
///
/// # Errors
///
/// [`HandlingError`] from [`compare`]; the declared fixture is valid.
pub fn synthetic_out_of_envelope_assessment() -> Result<HandlingAssessment, HandlingError> {
    compare(
        &synthetic_reference_envelope(),
        &synthetic_acceleration_tuned_turn_outside(),
    )
}

fn measurement(maneuver: ProbeKind, value: f64) -> ProbeMeasurement {
    ProbeMeasurement {
        maneuver,
        quantity: maneuver.quantity(),
        value,
    }
}

fn entry(
    maneuver: ProbeKind,
    reference: f64,
    plus_minus: f64,
    rationale: &str,
    initial_state: ProbeInitialState,
    input: FlightInput,
    held_out: bool,
) -> EnvelopeEntry {
    EnvelopeEntry {
        maneuver,
        quantity: maneuver.quantity(),
        unit: maneuver.quantity().unit().to_owned(),
        initial_state,
        input: vec![ProbeInputStep { at_s: 0.0, input }],
        reference,
        tolerance: Tolerance {
            plus_minus,
            rationale: rationale.to_owned(),
        },
        held_out,
    }
}
