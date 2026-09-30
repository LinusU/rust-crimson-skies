//! F26-B acceptance: headless maneuver probes and comparisons.
//!
//! Spec: `specs/F26-handling-probes-and-original-behavior-calibration.md`,
//! stage `### F26-B`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//! Required capabilities: ordinary build/test only.
//!
//! Every test flies the production `FlightModel` through the production
//! `ProbeRunner` and compares with the production `compare`. Replacing the
//! runner with a constant, dropping the trace hash or skipping the holdout
//! makes them fail. The synthetic airframe is authored development data, so
//! nothing here is an original-fidelity claim.

use cs_sim::flight::tuning::{DamageState, LoadoutMass, ModelKind};
use cs_sim::flight::{FlightInput, synthetic_fixed_wing};
use cs_sim::probes::{
    EnvelopeRun, ProbeError, ProbeInputStep, ProbeKind, ProbeRunner, ReferenceEnvelope,
    SYNTHETIC_HANDLING_AIRFRAME, Tolerance, VerdictStatus, compare, synthetic_reference_envelope,
};
use cs_types::content::Origin;

fn runner() -> ProbeRunner {
    ProbeRunner::new(SYNTHETIC_HANDLING_AIRFRAME, synthetic_fixed_wing())
}

fn value(run: &EnvelopeRun, maneuver: ProbeKind) -> f64 {
    run.runs
        .iter()
        .find(|flown| flown.maneuver == maneuver)
        .unwrap_or_else(|| panic!("{} was not flown", maneuver.label()))
        .value
}

/// An envelope over `maneuvers` whose references are what `baseline` measured,
/// with a 5 % tolerance and the turn held out.
fn fitted_envelope(baseline: &ProbeRunner, maneuvers: &[ProbeKind]) -> ReferenceEnvelope {
    let mut envelope = synthetic_reference_envelope();
    envelope
        .entries
        .retain(|entry| maneuvers.contains(&entry.maneuver));
    let run = baseline.run_envelope(&envelope).expect("valid envelope");
    for entry in &mut envelope.entries {
        entry.reference = value(&run, entry.maneuver);
        entry.tolerance = Tolerance {
            plus_minus: entry.reference.abs() * 0.05,
            rationale: "test: 5 % of the baseline measurement".to_owned(),
        };
        entry.held_out = entry.maneuver == ProbeKind::Turn;
    }
    envelope
}

/// The minimum scenario (AC02): repeated same-build probe hashes agree, and
/// the hash is sensitive to what was flown.
#[test]
fn accept_f26_b_repeated_same_build_probe_hashes_agree() {
    let envelope = synthetic_reference_envelope();
    let first = runner().run_envelope(&envelope).expect("valid");
    let second = runner().run_envelope(&envelope).expect("valid");

    assert!(!first.runs.is_empty());
    assert_eq!(first.combined_hash, second.combined_hash);
    assert_eq!(
        first, second,
        "the whole result is reproduced, not only the hash"
    );
    for (a, b) in first.runs.iter().zip(&second.runs) {
        assert_eq!(a.trace_hash, b.trace_hash, "{}", a.maneuver.label());
        assert_eq!(a.value.to_bits(), b.value.to_bits());
    }

    // The hash must discriminate: a different loadout, damage or timestep is a
    // different flight.
    let loaded = runner().with_loadout(LoadoutMass {
        armor_kg: 300.0,
        ..LoadoutMass::EMPTY
    });
    assert_ne!(
        loaded.run_envelope(&envelope).unwrap().combined_hash,
        first.combined_hash
    );
    let coarse = runner().with_timestep(1.0 / 60.0);
    assert_ne!(
        coarse.run_envelope(&envelope).unwrap().combined_hash,
        first.combined_hash
    );
    // Distinct maneuvers hash differently from each other.
    let hashes: Vec<u64> = first.runs.iter().map(|run| run.trace_hash).collect();
    for (index, hash) in hashes.iter().enumerate() {
        assert!(!hashes[..index].contains(hash));
    }
}

/// AC01 through real flights: a candidate that keeps the fitted acceleration
/// and roll but has weaker yaw authority flies a held-out turn outside the
/// envelope and fails on it.
#[test]
fn accept_f26_b_flown_candidate_with_tuned_acceleration_fails_the_held_out_turn() {
    let maneuvers = [ProbeKind::Acceleration, ProbeKind::Roll, ProbeKind::Turn];
    let envelope = fitted_envelope(&runner(), &maneuvers);

    let same = compare(&envelope, &runner().run_envelope(&envelope).unwrap().trace).unwrap();
    assert!(
        same.passes(),
        "the fitted airframe matches its own envelope"
    );

    let mut tuning = synthetic_fixed_wing();
    tuning.angular.max_rate_radps[2] *= 0.5;
    let weak_yaw = ProbeRunner::new(SYNTHETIC_HANDLING_AIRFRAME, tuning);
    let run = weak_yaw.run_envelope(&envelope).unwrap();
    let assessment = compare(&envelope, &run.trace).unwrap();

    assert!(!assessment.passes());
    let failures = assessment.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].maneuver, ProbeKind::Turn);
    assert!(failures[0].held_out);
    assert!(matches!(
        failures[0].status,
        VerdictStatus::OutOfEnvelope { deviation } if deviation > 0.0
    ));
    let acceleration = assessment
        .verdicts
        .iter()
        .find(|verdict| verdict.maneuver == ProbeKind::Acceleration)
        .unwrap();
    assert_eq!(acceleration.status, VerdictStatus::WithinEnvelope);
    assert!(!assessment.holdout_passes());
    assert!(!assessment.supports_original_fidelity_claim());
}

/// A maneuver that cannot be measured is left out of the trace and reported as
/// unavailable, never as a pass (AC03's shape at the runner).
#[test]
fn accept_f26_b_unmeasurable_maneuver_is_reported_not_passed() {
    let envelope = synthetic_reference_envelope();
    let run = runner().run_envelope(&envelope).unwrap();

    // The fixture's stall entry never recovers within the probe horizon.
    let (maneuver, error) = run
        .unmeasured
        .iter()
        .find(|(maneuver, _)| *maneuver == ProbeKind::StallRecovery)
        .expect("the stall entry is not measurable on this airframe");
    assert_eq!(*maneuver, ProbeKind::StallRecovery);
    assert!(matches!(error, ProbeError::NotMeasurable { .. }), "{error}");
    assert!(
        run.trace
            .measurement(
                ProbeKind::StallRecovery,
                ProbeKind::StallRecovery.quantity()
            )
            .is_none()
    );

    let assessment = compare(&envelope, &run.trace).unwrap();
    assert!(!assessment.passes());
    assert!(
        assessment
            .unavailable()
            .iter()
            .any(|verdict| verdict.maneuver == ProbeKind::StallRecovery)
    );
    assert_eq!(run.trace.origin, Origin::SyntheticFixture);
}

/// The measured quantities have the physical sign and ordering the maneuver
/// implies, so the runner cannot be a constant.
#[test]
fn accept_f26_b_measurements_respond_to_loadout_and_damage() {
    let envelope = synthetic_reference_envelope();
    let light = runner().run_envelope(&envelope).unwrap();
    let heavy = runner()
        .with_loadout(LoadoutMass {
            fuel_kg: 200.0,
            ordnance_kg: 300.0,
            armor_kg: 600.0,
        })
        .run_envelope(&envelope)
        .unwrap();

    for maneuver in [ProbeKind::Acceleration, ProbeKind::Boost] {
        let (light, heavy) = (value(&light, maneuver), value(&heavy, maneuver));
        assert!(light.is_finite() && heavy.is_finite());
        assert!(heavy > 0.0, "{} stays flyable", maneuver.label());
        assert!(
            heavy < light,
            "{}: extra mass must not accelerate faster ({heavy} vs {light})",
            maneuver.label()
        );
    }
    assert!(value(&light, ProbeKind::CoastDown) > 0.0);

    assert!((value(&light, ProbeKind::Damage) - 1.0).abs() < 1e-9);
    let damaged = runner()
        .with_damage(DamageState {
            control_authority: 0.5,
            ..DamageState::PRISTINE
        })
        .run_envelope(&envelope)
        .unwrap();
    let retained = value(&damaged, ProbeKind::Damage);
    assert!(retained > 0.0 && retained < 0.9, "retained {retained}");
    assert!(value(&damaged, ProbeKind::Roll) < value(&light, ProbeKind::Roll));
}

/// Boundaries refuse by name instead of flying something else.
#[test]
fn accept_f26_b_runner_refuses_unflyable_configuration() {
    let envelope = synthetic_reference_envelope();

    assert_eq!(
        runner().with_timestep(0.0).run_envelope(&envelope),
        Err(ProbeError::InvalidTimestep)
    );
    assert_eq!(
        runner().with_timestep(f64::NAN).run_envelope(&envelope),
        Err(ProbeError::InvalidTimestep)
    );

    let mut exceptional = synthetic_fixed_wing();
    exceptional.model_kind = ModelKind::Exceptional;
    assert_eq!(
        ProbeRunner::new(SYNTHETIC_HANDLING_AIRFRAME, exceptional).run_envelope(&envelope),
        Err(ProbeError::UnsupportedModel(ModelKind::Exceptional))
    );

    let mut late = synthetic_reference_envelope();
    late.entries[0].input = vec![ProbeInputStep {
        at_s: 0.5,
        input: FlightInput::NEUTRAL,
    }];
    let run = runner().run_envelope(&late).unwrap();
    assert!(run.unmeasured.iter().any(|(maneuver, error)| {
        *maneuver == late.entries[0].maneuver
            && matches!(error, ProbeError::ScheduleStartsLate { .. })
    }));

    let mut invalid = synthetic_reference_envelope();
    invalid
        .entries
        .iter_mut()
        .for_each(|entry| entry.held_out = false);
    assert!(matches!(
        runner().run_envelope(&invalid),
        Err(ProbeError::Envelope(_))
    ));
}
