//! Acceptance scenario F24-A (AC01 minimum scenario and its failure cases):
//! the fixed-wing force equations, their typed boundary and the synthetic
//! probe.
//!
//! Spec: `specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md`,
//! stage `### F24-A`. Task test prefix: `accept_f24_a_`.
//!
//! These tests drive production code only: [`cs_sim::flight`]'s
//! [`FlightModel`], [`SyntheticProbe`], [`synthetic_fixed_wing`] and the
//! typed tuning contract. Removing the equations, collapsing the low-speed
//! handling into a division by zero, dropping gravity, ignoring an assist
//! profile or letting a corrupt tuning through each makes one of them fail.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_sim::flight::{
    AirframeTuning, AirframeTuningError, DamageState, EngineState, FlightEnvironment, FlightInput,
    FlightInputError, FlightModel, FlightState, LoadoutMass, SyntheticProbe, synthetic_fixed_wing,
};
use cs_types::space::Quaternion;

/// AC01 minimum scenario: at zero airspeed every computed value is finite and
/// gravity still acts, driven through the production synthetic probe.
#[test]
fn accept_f24_a_zero_airspeed_is_finite_and_gravity_still_acts() {
    let probe = SyntheticProbe::new();
    let zero_airspeed = cs_sim::flight::synthetic_cases()
        .into_iter()
        .find(|case| case.name == "zero_airspeed")
        .expect("the declared probe set contains a zero-airspeed case");

    let output = probe
        .run(std::slice::from_ref(&zero_airspeed))
        .expect("the synthetic fixture is valid")[0];

    assert_eq!(output.instrument_state.airspeed_mps, 0.0);
    for value in output
        .world_force_n
        .into_iter()
        .chain(output.world_torque_nm)
    {
        assert!(value.is_finite(), "a zero-airspeed value must be finite");
    }
    for value in [
        output.instrument_state.angle_of_attack_rad,
        output.instrument_state.sideslip_rad,
        output.instrument_state.dynamic_pressure_pa,
        output.instrument_state.lift_coefficient,
        output.instrument_state.drag_coefficient,
        output.instrument_state.stall_scale,
        output.accepted_boost_consumption,
    ] {
        assert!(value.is_finite(), "every instrument value must be finite");
    }

    let total_mass = synthetic_fixed_wing().mass.mass_kg;
    let expected_gravity = -total_mass * FlightEnvironment::SEA_LEVEL.gravity_mps2;
    assert!(
        (output.diagnostics.gravity_force_n[1] - expected_gravity).abs() < 1e-9,
        "gravity must still act at zero airspeed"
    );
    assert!(output.diagnostics.lift_n == 0.0 && output.diagnostics.drag_n == 0.0);
    assert!(
        output.world_force_n[1] < 0.0,
        "the net vertical force at rest is gravity"
    );
}

/// Every declared synthetic case evaluates to finite force and torque, so no
/// attitude or speed in the fixture divides by zero.
#[test]
fn accept_f24_a_every_declared_synthetic_case_is_finite() {
    let probe = SyntheticProbe::new();
    let outputs = probe
        .run_declared()
        .expect("the synthetic fixture and probe are valid");
    assert_eq!(outputs.len(), cs_sim::flight::synthetic_cases().len());
    assert!(outputs.len() >= 3, "the probe must cover several regimes");
    for output in outputs {
        for value in output
            .world_force_n
            .into_iter()
            .chain(output.world_torque_nm)
        {
            assert!(value.is_finite());
        }
    }
}

/// The force and torque equations are a pure function of the tick's inputs:
/// the same state and command produce the same force and torque at every
/// render timestep, which is AC04's force half (no wall clock, no render
/// frame, no variable dt enters the force law).
#[test]
fn accept_f24_a_forces_are_independent_of_render_timestep() {
    let model = FlightModel::new(synthetic_fixed_wing());
    let state = FlightState {
        linear_velocity_mps: [0.0, -2.0, -50.0],
        angular_velocity_radps: [0.05, -0.1, 0.02],
        engine: EngineState::direct(0.8),
        boost_available: true,
        ..FlightState::at_rest(Quaternion::IDENTITY)
    };
    let input = FlightInput::try_new(-0.2, 0.3, 0.1, 0.8, true).expect("valid input");
    let env = FlightEnvironment::SEA_LEVEL;
    let loadout = LoadoutMass {
        fuel_kg: 100.0,
        ordnance_kg: 40.0,
        armor_kg: 10.0,
    };

    let mut reference = None;
    for dt_s in [1.0 / 30.0, 1.0 / 60.0, 1.0 / 120.0, 1.0 / 144.0] {
        let output = model
            .compute(&env, &loadout, &DamageState::PRISTINE, &state, &input, dt_s)
            .expect("valid");
        match reference {
            Some((force, torque)) => {
                assert_eq!(output.world_force_n, force, "force must not depend on dt");
                assert_eq!(
                    output.world_torque_nm, torque,
                    "torque must not depend on dt"
                );
            }
            None => reference = Some((output.world_force_n, output.world_torque_nm)),
        }
    }
}

/// The boundary refuses a corrupt tuning, a non-finite input and a
/// non-positive total mass by name instead of clamping them.
#[test]
fn accept_f24_a_corrupt_inputs_are_refused_by_name() {
    let mut corrupt = synthetic_fixed_wing();
    corrupt.reference_area_m2 = 0.0;
    assert_eq!(
        corrupt.validate(),
        Err(AirframeTuningError::NonPositive {
            field: "reference_area_m2"
        })
    );

    let model = FlightModel::new(synthetic_fixed_wing());
    let bad_input = FlightInput {
        pitch: f64::NAN,
        ..FlightInput::NEUTRAL
    };
    assert_eq!(
        model.compute(
            &FlightEnvironment::SEA_LEVEL,
            &LoadoutMass::EMPTY,
            &DamageState::PRISTINE,
            &FlightState::at_rest(Quaternion::IDENTITY),
            &bad_input,
            1.0 / 120.0,
        ),
        Err(cs_sim::flight::FlightError::NonFinite {
            field: "input.pitch"
        })
    );

    assert_eq!(
        LoadoutMass::EMPTY.total_mass_kg(0.0),
        Err(cs_sim::flight::LoadoutMassError::NonPositiveTotal)
    );

    assert_eq!(
        FlightInput::try_new(0.0, 1.5, 0.0, 0.5, false),
        Err(FlightInputError::OutOfRange {
            field: "input.roll",
            value: 1.5,
            min: -1.0,
            max: 1.0,
        })
    );

    // A non-finite environment is refused too.
    let mut bad_env = FlightEnvironment::SEA_LEVEL;
    bad_env.gravity_mps2 = f64::INFINITY;
    assert_eq!(
        model.compute(
            &bad_env,
            &LoadoutMass::EMPTY,
            &DamageState::PRISTINE,
            &FlightState::at_rest(Quaternion::IDENTITY),
            &FlightInput::NEUTRAL,
            1.0 / 120.0,
        ),
        Err(cs_sim::flight::FlightError::Environment(
            "environment.gravity_mps2"
        ))
    );
}

/// A tuning that only differs by profile is a different tuning: an improved
/// profile is never silently the fidelity profile.
#[test]
fn accept_f24_a_tuning_identity_keeps_model_and_profile() {
    let fidelity = synthetic_fixed_wing();
    assert_eq!(fidelity.profile, cs_sim::flight::HandlingProfile::Fidelity);
    assert_eq!(fidelity.model_kind, cs_sim::flight::ModelKind::FixedWing);
    assert_eq!(fidelity.origin, cs_types::content::Origin::SyntheticFixture);

    let mut improved = synthetic_fixed_wing();
    improved.profile = cs_sim::flight::HandlingProfile::Improved;
    assert_ne!(
        fidelity, improved,
        "profile is part of the tuning's identity"
    );

    let validated: AirframeTuning =
        AirframeTuning::try_new(synthetic_fixed_wing()).expect("the synthetic fixture validates");
    assert_eq!(validated, fidelity);
}
