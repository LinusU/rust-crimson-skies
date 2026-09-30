//! Acceptance scenario F25-A for the exceptional flight boundary: the shared
//! telemetry interface, the rotor drive and the reference maneuver envelope.
//!
//! Spec: `specs/F25-hoplite-autogyro-and-exceptional-flight-configurations.md`,
//! stage `### F25-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//! Task test prefix: `accept_f25_a_`.
//!
//! These tests drive [`cs_sim`]'s public API only: they fail to compile if the
//! telemetry interface, the rotor drive or the envelope is removed, and they
//! fail at run time if the declared kind/channel agreement, the explicit
//! visual mapping or the envelope's coverage regress.

use cs_sim::flight::model::{EngineState, FlightEnvironment, FlightInput, FlightModel};
use cs_sim::flight::tuning::{DamageState, LoadoutMass};
use cs_sim::flight::{
    EnvelopeError, EnvelopeStatus, FlightTelemetry, ManeuverKind, ModelKind, RotorDrive,
    RotorSpeedMapping, RotorVisualSample, SYNTHETIC_TICK_DT_S, SharedTelemetry, TelemetryError,
    TelemetryFrame, synthetic_exceptional_envelope, synthetic_fixed_wing, synthetic_rotor_drive,
    synthetic_rotor_mapping,
};
use cs_types::Tick;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{Origin, Provenance};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};
use cs_types::space::Quaternion;

fn state() -> cs_sim::flight::FlightState {
    cs_sim::flight::FlightState {
        linear_velocity_mps: [0.0, -2.0, -55.0],
        engine: EngineState::direct(0.8),
        ..cs_sim::flight::FlightState::at_rest(Quaternion::IDENTITY)
    }
}

fn probe_output() -> cs_sim::flight::FlightOutput {
    FlightModel::new(synthetic_fixed_wing())
        .compute(
            &FlightEnvironment::SEA_LEVEL,
            &LoadoutMass::EMPTY,
            &DamageState::PRISTINE,
            &state(),
            &FlightInput::try_new(0.0, 0.0, 0.0, 0.8, false).expect("the probe input is in range"),
            SYNTHETIC_TICK_DT_S,
        )
        .expect("the synthetic probe case is valid")
}

fn ramped_rotor() -> RotorDrive {
    let mut drive = synthetic_rotor_drive();
    for tick in 1..=120u64 {
        drive
            .advance_tick(40.0, 48.0, Tick(tick), SYNTHETIC_TICK_DT_S)
            .expect("each tick is newer than the last");
    }
    drive
}

/// A HUD, an AI controller or a probe that only knows
/// [`FlightTelemetry`] reads the same shared numbers for a fixed wing and for
/// an exceptional model, and the exceptional rotor channel is an optional
/// accessor rather than a downcast.
#[test]
fn accept_f25_a_telemetry_consumer_is_model_agnostic_across_both_kinds() {
    let state = state();
    let output = probe_output();
    let drive = ramped_rotor();

    let frames: Vec<TelemetryFrame> = vec![
        TelemetryFrame::standard(ModelKind::FixedWing, &state, &output, Tick(120))
            .expect("a fixed-wing frame"),
        TelemetryFrame::exceptional(
            ModelKind::Exceptional,
            &state,
            &output,
            Tick(120),
            &drive,
            Some(&synthetic_rotor_mapping()),
        )
        .expect("an exceptional frame"),
    ];

    let mut airspeeds = Vec::new();
    for frame in &frames {
        let telemetry: &dyn FlightTelemetry = frame;
        assert_eq!(telemetry.tick(), Tick(120));
        assert!(telemetry.shared().airspeed_mps.is_finite());
        assert!(telemetry.shared().vertical_speed_mps.is_finite());
        assert!((0.0..=1.0).contains(&telemetry.shared().stall_scale));
        airspeeds.push(telemetry.shared().airspeed_mps);
    }
    assert_eq!(
        airspeeds[0], airspeeds[1],
        "one consumer reads identical shared values from both kinds"
    );

    assert_eq!(frames[0].model_kind(), ModelKind::FixedWing);
    assert_eq!(frames[0].rotor(), None);
    assert_eq!(frames[1].model_kind(), ModelKind::Exceptional);
    let rotor = frames[1].rotor().expect("an exceptional frame has a rotor");
    assert!((rotor.physical_speed_radps - 40.0).abs() < 1e-9);
    assert!(rotor.visual_speed_radps().is_some());
}

/// The declared model kind and the present channels must agree: an exceptional
/// frame cannot lose its rotor channel and a fixed-wing frame cannot acquire
/// one, because either would let a consumer read a channel the law never
/// produced.
#[test]
fn accept_f25_a_frame_kind_must_agree_with_its_channels() {
    let state = state();
    let output = probe_output();
    let drive = ramped_rotor();

    assert_eq!(
        TelemetryFrame::standard(ModelKind::Exceptional, &state, &output, Tick(0)).err(),
        Some(TelemetryError::ModelKindMismatch {
            declared: ModelKind::Exceptional,
            has_rotor: false,
        })
    );
    assert_eq!(
        TelemetryFrame::exceptional(ModelKind::FixedWing, &state, &output, Tick(0), &drive, None)
            .err(),
        Some(TelemetryError::ModelKindMismatch {
            declared: ModelKind::FixedWing,
            has_rotor: true,
        })
    );

    // A corrupt production reading is refused by name, not reported as a
    // plausible instrument value.
    let mut corrupt = probe_output();
    corrupt.instrument_state.airspeed_mps = f64::INFINITY;
    assert_eq!(
        SharedTelemetry::sample(Tick(0), &state, &corrupt).err(),
        Some(TelemetryError::NonFinite {
            field: "telemetry.airspeed_mps"
        })
    );
}

/// The authoritative rotor rate is a fixed-tick quantity. Advancing it twice in
/// one tick — which is what a render frame driving it would look like — is
/// refused by name and leaves the rate untouched.
#[test]
fn accept_f25_a_rotor_rate_is_integrated_only_by_the_fixed_tick() {
    let mut drive = synthetic_rotor_drive();
    drive
        .advance_tick(40.0, 48.0, Tick(3), SYNTHETIC_TICK_DT_S)
        .expect("the first tick advances");
    let rate = drive.physical_speed_radps();
    assert!(rate > 0.0);
    assert_eq!(drive.last_tick(), Some(Tick(3)));

    for stale in [Tick(3), Tick(2), Tick(0)] {
        assert_eq!(
            drive.advance_tick(40.0, 48.0, stale, SYNTHETIC_TICK_DT_S),
            Err(TelemetryError::NonMonotonicTick {
                last: Tick(3),
                got: stale,
            })
        );
        assert_eq!(drive.physical_speed_radps(), rate);
    }

    drive
        .advance_tick(40.0, 48.0, Tick(4), SYNTHETIC_TICK_DT_S)
        .expect("a newer tick advances");
    assert!(drive.physical_speed_radps() > rate);
}

/// Physical and visual rotor speeds may differ, but only through an explicit
/// declared mapping: with none there is no visual rate at all rather than an
/// implicit 1:1, and a declared ratio is honored exactly.
#[test]
fn accept_f25_a_visual_rotor_rate_requires_the_explicit_mapping() {
    let drive = ramped_rotor();
    let physical = drive.physical_speed_radps();

    let unmapped = drive.telemetry(None);
    assert_eq!(unmapped.physical_speed_radps, physical);
    assert_eq!(unmapped.visual_speed_radps(), None);

    let mapping =
        RotorSpeedMapping::new(0.25, Origin::SyntheticFixture).expect("a positive finite ratio");
    let mapped = drive.telemetry(Some(&mapping));
    assert_eq!(
        mapped.visual_speed_radps(),
        Some(physical * 0.25),
        "the declared ratio, not the physical rate, is drawn"
    );
    assert_eq!(
        mapped.visual.expect("mapped").origin,
        Origin::SyntheticFixture,
        "the mapping carries its own origin"
    );

    assert_eq!(
        RotorSpeedMapping::new(-1.0, Origin::SyntheticFixture).err(),
        Some(TelemetryError::NonPositive {
            field: "rotor.visual_radps_per_physical_radps"
        })
    );
    assert_eq!(
        synthetic_rotor_mapping().visual_radps_per_physical_radps(),
        1.5
    );
}

/// Drawing the rotor once per tick and sixty times per tick produces the same
/// simulation and the same drawn phase; only the number of samples differs.
#[test]
fn accept_f25_a_rotor_visual_sampling_never_reaches_the_simulation() {
    let mapping = synthetic_rotor_mapping();

    let draw = |frames_per_tick: u32| {
        let mut drive = synthetic_rotor_drive();
        let mut sample = RotorVisualSample::at_rest();
        let frame_dt = SYNTHETIC_TICK_DT_S / f64::from(frames_per_tick);
        for tick in 1..=120u64 {
            drive
                .advance_tick(40.0, 48.0, Tick(tick), SYNTHETIC_TICK_DT_S)
                .expect("each tick is newer than the last");
            for _ in 0..frames_per_tick {
                sample = drive
                    .visual_sample(Some(&mapping), sample, frame_dt)
                    .expect("the drawn sample is finite");
            }
        }
        (drive, sample)
    };

    let (one_per_tick, one) = draw(1);
    let (sixty_per_tick, sixty) = draw(60);
    assert_eq!(
        one_per_tick.physical_speed_radps(),
        sixty_per_tick.physical_speed_radps()
    );
    assert_eq!(one.physical_speed_radps, sixty.physical_speed_radps);
    assert!(
        (one.phase_rad - sixty.phase_rad).abs() < 1e-9,
        "both draws cover one second of render time"
    );
    assert!(
        (0.0..std::f64::consts::TAU).contains(&one.phase_rad),
        "the drawn phase stays wrapped"
    );
    assert_eq!(sixty.visual_speed_radps(), Some(40.0 * 1.5));

    assert_eq!(
        one_per_tick
            .visual_sample(Some(&mapping), RotorVisualSample::at_rest(), 0.0)
            .err(),
        Some(TelemetryError::NonPositive {
            field: "rotor_visual.render_dt_s"
        })
    );
    assert_eq!(
        one_per_tick
            .visual_sample(Some(&mapping), RotorVisualSample::at_rest(), f64::NAN)
            .err(),
        Some(TelemetryError::NonFinite {
            field: "rotor_visual.render_dt_s"
        })
    );

    // Finite inputs that overflow are refused by name rather than handed out as
    // a non-finite drawn phase.
    let mut absurd_drive = synthetic_rotor_drive();
    absurd_drive
        .advance_tick(1e300, 1e300, Tick(1), SYNTHETIC_TICK_DT_S)
        .expect("a finite commanded rate is accepted however absurd");
    let absurd = RotorSpeedMapping::new(1e300, Origin::SyntheticFixture)
        .expect("a finite positive ratio is accepted however absurd");
    assert_eq!(
        absurd_drive
            .visual_sample(Some(&absurd), RotorVisualSample::at_rest(), 1e300)
            .err(),
        Some(TelemetryError::NonFinite {
            field: "rotor_visual.phase_rad"
        })
    );
}

/// The exceptional envelope records every maneuver its kind requires, holds one
/// out of the fit, and is **not** usable as a reference until an original trace
/// backs it. The fixture is synthetic and says so.
#[test]
fn accept_f25_a_exceptional_envelope_covers_its_maneuvers_and_is_unmeasured() {
    let envelope = synthetic_exceptional_envelope();
    assert_eq!(envelope.validate(), Ok(()));
    assert_eq!(envelope.model_kind, ModelKind::Exceptional);
    assert_eq!(envelope.origin, Origin::SyntheticFixture);
    assert!(!envelope.origin.is_original());
    assert!(envelope.missing_required().is_empty());
    assert!(envelope.maneuvers.iter().any(|spec| spec.held_out));
    assert!(
        !envelope.is_ready_as_reference(),
        "a synthetic fixture cannot be an approved reference trace"
    );

    // The four maneuvers the sheet names for an exceptional airframe are
    // required for it and not for a fixed wing.
    for kind in [
        ManeuverKind::LowSpeedBehaviour,
        ManeuverKind::YawBehaviour,
        ManeuverKind::LiftBehaviour,
        ManeuverKind::RotorVisual,
    ] {
        assert!(kind.required_for(ModelKind::Exceptional), "{kind:?}");
        assert!(!kind.required_for(ModelKind::FixedWing), "{kind:?}");
    }

    let mut missing = envelope.clone();
    missing
        .maneuvers
        .retain(|spec| spec.kind != ManeuverKind::RotorVisual);
    assert_eq!(
        missing.validate(),
        Err(EnvelopeError::MissingRequired {
            kind: ManeuverKind::RotorVisual
        })
    );
    assert_eq!(missing.missing_required(), vec![ManeuverKind::RotorVisual]);

    let mut measured = envelope.clone();
    measured.status = EnvelopeStatus::Unmeasured {
        reason: "  ".to_owned(),
    };
    assert_eq!(
        measured.validate(),
        Err(EnvelopeError::BlankUnmeasuredReason)
    );

    let mut no_tolerance = envelope;
    no_tolerance.maneuvers[0].tolerance = f64::NAN;
    assert_eq!(
        no_tolerance.validate(),
        Err(EnvelopeError::NonPositiveTolerance {
            kind: ManeuverKind::Acceleration
        })
    );

    // A synthetic envelope stays out of reference use however its status is set:
    // a `measured` status needs an observed provenance, and even an observed
    // provenance cannot make a `SyntheticFixture` envelope an original
    // reference.
    let mut designed = synthetic_exceptional_envelope();
    designed.status = EnvelopeStatus::Measured {
        provenance: Provenance::designed(ClaimId::new("f25a.it.envelope").expect("a claim id")),
    };
    assert_eq!(
        designed.validate(),
        Err(EnvelopeError::MeasuredWithoutObservation {
            class: ClaimStatus::Designed
        })
    );
    assert!(!designed.is_ready_as_reference());

    // A synthetic span: it names no real installation and asserts no original
    // claim. It exists only to drive the readiness gate.
    let source = SourceSpan::new(
        ContentHash::from_bytes([0x2a; 32]),
        "fixture.synthetic-exceptional.zbd",
        None,
        0,
        8,
        None,
    )
    .expect("a valid synthetic span");
    let mut observed = designed.clone();
    observed.status = EnvelopeStatus::Measured {
        provenance: Provenance::new(
            ClaimId::new("f25a.it.observed").expect("a claim id"),
            ClaimStatus::ObservedTool,
            Some(source.clone()),
        )
        .expect("an observed claim is accepted"),
    };
    assert_eq!(observed.validate(), Ok(()));
    assert!(
        !observed.is_ready_as_reference(),
        "the envelope still came from a synthetic fixture"
    );

    let mut original = observed;
    original.origin = Origin::Installation { source };
    assert_eq!(original.validate(), Ok(()));
    assert!(
        original.is_ready_as_reference(),
        "an installation envelope with an observed reference and full coverage is ready"
    );
}
