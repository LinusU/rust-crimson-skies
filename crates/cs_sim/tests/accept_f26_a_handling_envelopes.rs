//! F26-A acceptance: handling telemetry and reference-envelope schemas.
//!
//! Spec: `specs/F26-handling-probes-and-original-behavior-calibration.md`,
//! stage `### F26-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//! Required capabilities: ordinary build/test only.
//!
//! These tests call the production schema and the production comparison
//! (`cs_sim::probes`), not a test-only reimplementation. Removing `compare`,
//! the holdout rule or the boundary validation makes them fail.

use cs_sim::flight::FlightInput;
use cs_sim::probes::{
    EnvelopeEntry, HandlingError, ProbeInitialState, ProbeInputStep, ProbeKind, ProbeMeasurement,
    ProbeQuantity, ProbeTrace, ReferenceEnvelope, SYNTHETIC_HANDLING_AIRFRAME, TimingUncertainty,
    VerdictStatus, compare, synthetic_acceleration_tuned_turn_outside, synthetic_covering_trace,
    synthetic_reference_envelope,
};
use cs_types::content::Origin;

/// The fixture entry for one maneuver, mutably.
fn find(envelope: &mut ReferenceEnvelope, kind: ProbeKind) -> &mut EnvelopeEntry {
    envelope
        .entries
        .iter_mut()
        .find(|entry| entry.maneuver == kind)
        .expect("the fixture declares this maneuver")
}

/// The minimum scenario (AC01): a tuned acceleration curve cannot pass if its
/// held-out turn radius is outside the envelope.
///
/// The candidate matches the fitted acceleration entry and is inside its
/// window; the comparison still fails, and it fails on the held-out turn. An
/// implementation that only checked fitted entries, or that treated a holdout
/// as informational, passes the candidate and fails this test.
#[test]
fn accept_f26_a_tuned_acceleration_curve_cannot_pass_with_a_held_out_turn_outside_the_envelope() {
    let envelope = synthetic_reference_envelope();
    let trace = synthetic_acceleration_tuned_turn_outside();
    let assessment = compare(&envelope, &trace).expect("the declared fixture is valid");

    assert!(!assessment.passes(), "the held-out turn is out of envelope");
    assert!(!assessment.holdout_passes());

    let failures = assessment.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].maneuver, ProbeKind::Turn);
    assert!(failures[0].held_out);
    assert_eq!(
        failures[0].status,
        VerdictStatus::OutOfEnvelope { deviation: 40.0 }
    );

    let acceleration = assessment
        .verdicts
        .iter()
        .find(|verdict| verdict.maneuver == ProbeKind::Acceleration)
        .expect("acceleration is compared");
    assert_eq!(acceleration.status, VerdictStatus::WithinEnvelope);
    assert!(!acceleration.held_out);

    // The comparison is a pure function of its inputs: the same pair yields the
    // same assessment.
    let repeated = compare(&envelope, &trace).expect("valid");
    assert_eq!(assessment, repeated);
}

/// A candidate that misses a required measurement reports the entry as
/// unavailable and cannot pass: a hole in a trace is never read as agreement.
#[test]
fn accept_f26_a_a_missing_measurement_is_unavailable_not_a_pass() {
    let envelope = synthetic_reference_envelope();
    let mut trace = synthetic_covering_trace();
    trace
        .measurements
        .retain(|row| row.maneuver != ProbeKind::Turn);

    let assessment = compare(&envelope, &trace).expect("valid");
    assert!(!assessment.passes());
    let unavailable = assessment.unavailable();
    assert_eq!(unavailable.len(), 1);
    assert_eq!(unavailable[0].maneuver, ProbeKind::Turn);
    assert!(unavailable[0].held_out);
    assert_eq!(unavailable[0].status, VerdictStatus::NoMeasurement);
}

/// The envelope boundary refuses a corrupt or unprovenanced record by name and
/// never repairs it into a plausible one: a zero tolerance (the exact float
/// equality the sheet forbids), an unjustified tolerance, a wrong unit, a
/// blank identity, a blank timing method, no held-out maneuver, a duplicate
/// entry and a non-finite reference are each refused.
#[test]
fn accept_f26_a_envelope_boundary_refuses_corrupt_records_by_name() {
    let mut zero_tolerance = synthetic_reference_envelope();
    find(&mut zero_tolerance, ProbeKind::Acceleration)
        .tolerance
        .plus_minus = 0.0;
    assert_eq!(
        zero_tolerance.validate(),
        Err(HandlingError::NonPositiveTolerance {
            maneuver: ProbeKind::Acceleration
        })
    );

    let mut unjustified = synthetic_reference_envelope();
    find(&mut unjustified, ProbeKind::Dive).tolerance.rationale = "   ".to_owned();
    assert_eq!(
        unjustified.validate(),
        Err(HandlingError::BlankToleranceRationale {
            maneuver: ProbeKind::Dive
        })
    );

    let mut wrong_unit = synthetic_reference_envelope();
    find(&mut wrong_unit, ProbeKind::Turn).unit = "rad/s".to_owned();
    assert_eq!(
        wrong_unit.validate(),
        Err(HandlingError::UnitMismatch {
            quantity: ProbeQuantity::TurnRadiusM,
            declared: "rad/s".to_owned()
        })
    );

    let mut blank_airframe = synthetic_reference_envelope();
    blank_airframe.airframe_id = "  ".to_owned();
    assert_eq!(
        blank_airframe.validate(),
        Err(HandlingError::EmptyIdentity {
            field: "airframe_id"
        })
    );

    let mut blank_method = synthetic_reference_envelope();
    blank_method.timing_uncertainty = TimingUncertainty {
        plus_minus_s: 0.001,
        method: String::new(),
    };
    assert_eq!(
        blank_method.validate(),
        Err(HandlingError::BlankTimingMethod)
    );

    let mut no_holdout = synthetic_reference_envelope();
    for entry in &mut no_holdout.entries {
        entry.held_out = false;
    }
    assert_eq!(no_holdout.validate(), Err(HandlingError::NoHeldOutEntry));

    let mut duplicate = synthetic_reference_envelope();
    let copy = find(&mut duplicate, ProbeKind::Roll).clone();
    duplicate.entries.push(copy);
    assert_eq!(
        duplicate.validate(),
        Err(HandlingError::DuplicateEntry {
            maneuver: ProbeKind::Roll,
            quantity: ProbeQuantity::RollRateRadps
        })
    );

    let mut non_finite = synthetic_reference_envelope();
    find(&mut non_finite, ProbeKind::Climb).reference = f64::NAN;
    assert!(matches!(
        non_finite.validate(),
        Err(HandlingError::NonFinite { .. })
    ));

    let mut empty_schedule = synthetic_reference_envelope();
    find(&mut empty_schedule, ProbeKind::Boost).input.clear();
    assert_eq!(
        empty_schedule.validate(),
        Err(HandlingError::EmptyInputSchedule {
            maneuver: ProbeKind::Boost
        })
    );

    // The declared fixture itself is valid, so every refusal above is the
    // mutation and not a pre-existing defect.
    assert_eq!(synthetic_reference_envelope().validate(), Ok(()));
}

/// A maneuver the envelope does not bound is reported as missing coverage
/// rather than silently unmeasured.
#[test]
fn accept_f26_a_removing_a_maneuver_reports_missing_coverage() {
    let mut envelope = synthetic_reference_envelope();
    envelope
        .entries
        .retain(|entry| entry.maneuver != ProbeKind::StallRecovery);
    assert!(!envelope.covers_every_maneuver());
    assert_eq!(envelope.missing_maneuvers(), vec![ProbeKind::StallRecovery]);
}

/// The candidate boundary refuses a non-finite value, a duplicate row and a
/// blank identity, and refuses to compare a different airframe's trace against
/// the envelope.
#[test]
fn accept_f26_a_candidate_boundary_refuses_corrupt_traces_by_name() {
    let envelope = synthetic_reference_envelope();

    let non_finite = ProbeTrace {
        airframe_id: SYNTHETIC_HANDLING_AIRFRAME.to_owned(),
        origin: Origin::SyntheticFixture,
        measurements: vec![ProbeMeasurement {
            maneuver: ProbeKind::Turn,
            quantity: ProbeQuantity::TurnRadiusM,
            value: f64::INFINITY,
        }],
    };
    assert!(matches!(
        compare(&envelope, &non_finite),
        Err(HandlingError::NonFinite { .. })
    ));

    let mut duplicate = synthetic_covering_trace();
    duplicate.measurements.push(duplicate.measurements[0]);
    assert_eq!(
        duplicate.validate(),
        Err(HandlingError::DuplicateEntry {
            maneuver: ProbeKind::Acceleration,
            quantity: ProbeQuantity::SpeedGainMps
        })
    );

    let mut blank = synthetic_covering_trace();
    blank.airframe_id = String::new();
    assert_eq!(
        blank.validate(),
        Err(HandlingError::EmptyIdentity {
            field: "trace.airframe_id"
        })
    );

    let mut other_airframe = synthetic_covering_trace();
    other_airframe.airframe_id = "fixture.some-other-plane".to_owned();
    assert_eq!(
        compare(&envelope, &other_airframe),
        Err(HandlingError::AirframeMismatch {
            envelope: SYNTHETIC_HANDLING_AIRFRAME.to_owned(),
            candidate: "fixture.some-other-plane".to_owned()
        })
    );

    let mut extra = synthetic_covering_trace();
    extra.measurements.push(ProbeMeasurement {
        maneuver: ProbeKind::Yaw,
        quantity: ProbeQuantity::YawRateRadps,
        value: 0.6,
    });
    assert!(
        compare(&envelope, &extra).is_err(),
        "the envelope holds no second reference for a repeated row"
    );
}

/// The envelope records the sheet's required provenance fields: the input
/// schedule, initial state, difficulty, loadout, timing uncertainty and units
/// are present and validated, and the timing uncertainty must carry a method.
#[test]
fn accept_f26_a_envelope_records_input_state_difficulty_loadout_timing_and_units() {
    let envelope = synthetic_reference_envelope();
    assert!(!envelope.difficulty.trim().is_empty());
    assert!(!envelope.loadout.trim().is_empty());
    assert_eq!(envelope.timing_uncertainty.plus_minus_s, 0.001);
    assert!(!envelope.timing_uncertainty.method.trim().is_empty());

    for entry in &envelope.entries {
        assert_eq!(entry.unit, entry.quantity.unit());
        assert!(!entry.input.is_empty());
        assert_eq!(
            entry.input[0].input.validate(),
            Ok(()),
            "the recorded input is a valid command"
        );
        let window = entry.accepted_window();
        assert!(window[0] < window[1], "the tolerance widens the window");
        assert_eq!(window[0], entry.reference - entry.tolerance.plus_minus);
        assert_eq!(window[1], entry.reference + entry.tolerance.plus_minus);
    }

    // The boundary helpers are production code too.
    let step = ProbeInputStep::try_new(
        -1.0,
        FlightInput::try_new(0.0, 0.0, 0.0, 0.5, false).expect("valid"),
    );
    assert!(matches!(step, Err(HandlingError::Negative { .. })));
    assert_eq!(
        ProbeInitialState {
            engine_spool: 1.5,
            ..ProbeInitialState::AT_REST
        }
        .validate(),
        Err(HandlingError::OutOfRange {
            field: "initial_state.engine_spool".to_owned(),
            value: 1.5
        })
    );
}
