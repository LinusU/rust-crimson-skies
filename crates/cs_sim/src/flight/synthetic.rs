//! Synthetic fixed-wing fixture and probe harness (F24-A).
//!
//! Spec: `specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md`,
//! stage `### F24-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! The spec asks for "a minimal synthetic fixture first", and the contract
//! says synthetic tests must define exact mass, forces and geometry. This
//! module is production bootstrap code, not a test-only reimplementation: the
//! acceptance tests drive [`SyntheticProbe`] and [`synthetic_fixed_wing`], so
//! deleting the fixture makes them fail.
//!
//! **Synthetic, not original.** The airframe carries
//! [`Origin::SyntheticFixture`], so nothing here can be mistaken for a retail
//! airframe: it exists to exercise the equations and the boundary, not to
//! reproduce an original tuning (`F24` "Research boundary"). The probe runs
//! the equations open-loop — it never integrates a body, because Avian owns
//! pose and velocity integration (`docs/01-ARCHITECTURE.md`, "Body
//! ownership") and a probe that integrated would be a second integrator.

use cs_types::content::Origin;
use cs_types::space::Quaternion;

use super::model::{
    EngineState, FlightEnvironment, FlightError, FlightInput, FlightModel, FlightOutput,
    FlightState,
};
use super::tuning::{
    AirframeTuning, AngularResponse, AssistProfile, BoostParameters, DamageState, DragParameters,
    EngineCurve, HandlingProfile, LiftCurve, LoadoutMass, MassProperties, ModelKind, StallBehavior,
};

/// A 100-flight synthetic fixed-wing tuning: a 1200 kg airframe with a
/// 21 m² wing, 9 kN of thrust and a gentle stall.
///
/// Every value is newly authored development data. It is deliberately
/// civilian-slow and does not claim to match any original airframe; F24-D
/// calibrates the real roster against fingerprinted reference captures.
#[must_use]
pub fn synthetic_fixed_wing() -> AirframeTuning {
    AirframeTuning {
        model_kind: ModelKind::FixedWing,
        profile: HandlingProfile::Fidelity,
        origin: Origin::SyntheticFixture,
        mass: MassProperties {
            mass_kg: 1200.0,
            inertia_kg_m2: [1400.0, 2100.0, 2600.0],
        },
        engine: EngineCurve {
            idle_thrust_n: 400.0,
            max_thrust_n: 9000.0,
            throttle_response_per_s: 1.5,
        },
        boost: BoostParameters {
            thrust_n: 3000.0,
            consumption_per_s: 0.25,
        },
        drag: DragParameters {
            zero_lift_coefficient: 0.03,
            induced_coefficient: 0.06,
        },
        lift: LiftCurve {
            lift_at_zero_alpha: 0.15,
            lift_slope_per_rad: 4.5,
            max_lift_coefficient: 1.6,
        },
        stall: StallBehavior {
            stall_angle_rad: 0.28,
            stall_width_rad: 0.18,
            residual_fraction: 0.25,
        },
        angular: AngularResponse {
            rate_gain_per_s: 4.0,
            rate_damping_per_s: 0.5,
            max_rate_radps: [2.0, 1.5, 1.0],
            max_torque_nm: [20_000.0, 30_000.0, 8_000.0],
            control_airspeed_full_mps: 40.0,
        },
        assists: AssistProfile::CALIBRATED,
        reference_area_m2: 21.0,
    }
}

/// One declared synthetic probe case: a stated state and command.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SyntheticCase {
    /// A stable label for diagnostics.
    pub name: &'static str,
    /// The aircraft state to evaluate.
    pub state: FlightState,
    /// The command to apply.
    pub input: FlightInput,
}

/// The declared synthetic cases the acceptance tests evaluate.
///
/// They are the bounded minimum set: at rest, level cruise, a dive and a
/// stalled attitude. All are newly authored fixture data.
#[must_use]
pub fn synthetic_cases() -> Vec<SyntheticCase> {
    let level = Quaternion::IDENTITY;
    vec![
        SyntheticCase {
            name: "zero_airspeed",
            state: FlightState::at_rest(level),
            input: FlightInput::NEUTRAL,
        },
        SyntheticCase {
            name: "level_cruise",
            state: FlightState {
                linear_velocity_mps: [0.0, 0.0, -55.0],
                engine: EngineState::direct(0.75),
                ..FlightState::at_rest(level)
            },
            input: FlightInput::try_new(0.0, 0.0, 0.0, 0.75, false)
                .expect("the declared neutral cruise input is valid"),
        },
        SyntheticCase {
            name: "steep_dive",
            state: FlightState {
                linear_velocity_mps: [0.0, -40.0, -40.0],
                engine: EngineState::direct(0.0),
                ..FlightState::at_rest(level)
            },
            input: FlightInput::try_new(0.0, 0.0, 0.0, 0.0, false).expect("idle is a valid input"),
        },
        SyntheticCase {
            name: "high_angle_of_attack",
            state: FlightState {
                linear_velocity_mps: [0.0, -8.0, -30.0],
                engine: EngineState::direct(0.4),
                ..FlightState::at_rest(level)
            },
            input: FlightInput::try_new(-1.0, 0.0, 0.0, 0.4, false)
                .expect("full nose-up is a valid input"),
        },
    ]
}

/// Runs declared synthetic cases through a production [`FlightModel`].
///
/// The probe evaluates the equations open-loop and returns one output per
/// case; it never advances a body, so it cannot become a second integrator.
#[derive(Clone, Debug, PartialEq)]
pub struct SyntheticProbe {
    model: FlightModel,
    environment: FlightEnvironment,
    loadout: LoadoutMass,
    damage: DamageState,
    dt_s: f64,
}

impl Default for SyntheticProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl SyntheticProbe {
    /// The declared synthetic probe: the synthetic airframe, sea-level
    /// environment, empty loadout, pristine damage and a 1/120 s tick.
    #[must_use]
    pub fn new() -> Self {
        Self {
            model: FlightModel::new(synthetic_fixed_wing()),
            environment: FlightEnvironment::SEA_LEVEL,
            loadout: LoadoutMass::EMPTY,
            damage: DamageState::PRISTINE,
            dt_s: 1.0 / 120.0,
        }
    }

    /// Overrides the fixed timestep, in seconds.
    #[must_use]
    pub fn with_timestep(mut self, dt_s: f64) -> Self {
        self.dt_s = dt_s;
        self
    }

    /// The model under test.
    #[must_use]
    pub const fn model(&self) -> &FlightModel {
        &self.model
    }

    /// Evaluates every case, returning one output per case in order.
    ///
    /// # Errors
    ///
    /// [`FlightError`] from the equations; the declared synthetic cases are
    /// valid, so a failure means the fixture or a boundary regressed.
    pub fn run(&self, cases: &[SyntheticCase]) -> Result<Vec<FlightOutput>, FlightError> {
        cases
            .iter()
            .map(|case| {
                self.model.compute(
                    &self.environment,
                    &self.loadout,
                    &self.damage,
                    &case.state,
                    &case.input,
                    self.dt_s,
                )
            })
            .collect()
    }

    /// Evaluates the declared [`synthetic_cases`].
    ///
    /// # Errors
    ///
    /// [`FlightError`] from the equations.
    pub fn run_declared(&self) -> Result<Vec<FlightOutput>, FlightError> {
        self.run(&synthetic_cases())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The declared fixture validates and every synthetic case is finite.
    #[test]
    fn accept_f24_a_synthetic_fixture_validates_and_every_case_is_finite() {
        let tuning = synthetic_fixed_wing();
        assert_eq!(tuning.validate(), Ok(()));
        assert_eq!(tuning.origin, Origin::SyntheticFixture);
        assert_eq!(tuning.profile, HandlingProfile::Fidelity);

        let probe = SyntheticProbe::new();
        let outputs = probe.run_declared().expect("the declared cases are valid");
        assert_eq!(outputs.len(), synthetic_cases().len());
        for output in outputs {
            for value in output
                .world_force_n
                .into_iter()
                .chain(output.world_torque_nm)
            {
                assert!(value.is_finite(), "every synthetic output must be finite");
            }
        }
    }
}
