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

/// One of the four AC03 maneuver classes a synthetic trace covers.
///
/// They are the declared synthetic maneuvers F24-C measures through the
/// production runtime; comparing them against original reference envelopes is
/// F24-D's separate, capability-gated job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SyntheticManeuver {
    /// Straight-line acceleration from cruise at full throttle.
    Acceleration,
    /// A held yaw command: a sustained turn.
    SustainedTurn,
    /// A held roll command followed by a release.
    Roll,
    /// An entry into a stalled attitude and a recovery from it.
    StallRecovery,
}

impl SyntheticManeuver {
    /// Every maneuver, in a stable order.
    pub const ALL: [Self; 4] = [
        Self::Acceleration,
        Self::SustainedTurn,
        Self::Roll,
        Self::StallRecovery,
    ];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Acceleration => "acceleration",
            Self::SustainedTurn => "sustained_turn",
            Self::Roll => "roll",
            Self::StallRecovery => "stall_recovery",
        }
    }

    /// Looks a maneuver up by its label; `None` for an unknown label.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|maneuver| maneuver.label() == label)
    }
}

/// A declared synthetic envelope: the inclusive range one measured quantity of
/// one synthetic maneuver must fall inside.
///
/// **Designed, not original.** These bounds are newly authored project design
/// chosen so the equations' synthetic behavior is bounded and observable; they
/// are not derived from any original capture. F24-D replaces them with
/// envelopes fitted to fingerprinted original reference traces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SyntheticEnvelope {
    /// Stable name, e.g. `roll.peak_rate_radps`.
    pub name: &'static str,
    /// The maneuver whose trace this envelope bounds.
    pub maneuver: SyntheticManeuver,
    /// The inclusive lower bound.
    pub min: f64,
    /// The inclusive upper bound.
    pub max: f64,
}

impl SyntheticEnvelope {
    /// Whether `value` is finite and inside the declared range.
    #[must_use]
    pub fn contains(&self, value: f64) -> bool {
        value.is_finite() && (self.min..=self.max).contains(&value)
    }

    /// Checks `value` against the declared range.
    ///
    /// # Errors
    ///
    /// [`SyntheticEnvelopeError`] naming the envelope and the measurement. A
    /// non-finite measurement is always refused: an uninstrumented trace is
    /// not an in-envelope one.
    pub fn check(&self, value: f64) -> Result<(), SyntheticEnvelopeError> {
        if self.contains(value) {
            Ok(())
        } else {
            Err(SyntheticEnvelopeError {
                name: self.name,
                value,
                min: self.min,
                max: self.max,
            })
        }
    }
}

/// A measurement that fell outside its declared synthetic envelope.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SyntheticEnvelopeError {
    /// The envelope's stable name.
    pub name: &'static str,
    /// The measured value.
    pub value: f64,
    /// The inclusive lower bound.
    pub min: f64,
    /// The inclusive upper bound.
    pub max: f64,
}

impl std::fmt::Display for SyntheticEnvelopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the synthetic measurement {} = {} is outside the declared envelope [{}, {}]",
            self.name, self.value, self.min, self.max
        )
    }
}

impl std::error::Error for SyntheticEnvelopeError {}

/// The declared synthetic trace envelopes, one per measured AC03 maneuver
/// quantity.
///
/// Every bound is newly authored design (see [`SyntheticEnvelope`]). They are
/// deliberately wide enough to survive the fixed-step integrator sampling but
/// narrow enough that removing the force path, the controller or a loadout/
/// damage binding moves a measurement out of range.
#[must_use]
pub fn synthetic_trace_envelopes() -> Vec<SyntheticEnvelope> {
    vec![
        envelope(
            "acceleration.full_throttle_speed_gain_mps",
            SyntheticManeuver::Acceleration,
            5.0,
            40.0,
        ),
        envelope("roll.peak_rate_radps", SyntheticManeuver::Roll, 1.0, 2.05),
        envelope(
            "roll.released_settled_rate_radps",
            SyntheticManeuver::Roll,
            0.0,
            0.05,
        ),
        envelope(
            "turn.heading_change_rad",
            SyntheticManeuver::SustainedTurn,
            1.0,
            3.0,
        ),
        envelope(
            "stall.minimum_stall_scale",
            SyntheticManeuver::StallRecovery,
            0.2,
            0.6,
        ),
        envelope(
            "stall.recovered_stall_scale",
            SyntheticManeuver::StallRecovery,
            0.9,
            1.0,
        ),
    ]
}

/// The declared envelope `name`, or `None` for an undeclared name.
#[must_use]
pub fn synthetic_trace_envelope(name: &str) -> Option<SyntheticEnvelope> {
    synthetic_trace_envelopes()
        .into_iter()
        .find(|envelope| envelope.name == name)
}

/// Builds one declared envelope.
fn envelope(
    name: &'static str,
    maneuver: SyntheticManeuver,
    min: f64,
    max: f64,
) -> SyntheticEnvelope {
    assert!(min.is_finite() && max.is_finite() && min <= max);
    SyntheticEnvelope {
        name,
        maneuver,
        min,
        max,
    }
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

    /// F24-C: the declared synthetic envelopes are a complete, unique set
    /// covering every AC03 maneuver, and their boundary behaves: a value inside
    /// passes, a value outside and a non-finite value both fail by name.
    #[test]
    fn accept_f24_c_trace_envelopes_cover_every_maneuver_and_check_bounds() {
        let envelopes = synthetic_trace_envelopes();
        assert!(!envelopes.is_empty());
        for (index, envelope) in envelopes.iter().enumerate() {
            assert!(envelope.min.is_finite() && envelope.max.is_finite());
            assert!(envelope.min <= envelope.max);
            assert!(
                envelopes[..index]
                    .iter()
                    .all(|earlier| earlier.name != envelope.name),
                "{} is declared twice",
                envelope.name
            );
        }
        for maneuver in SyntheticManeuver::ALL {
            assert!(
                envelopes
                    .iter()
                    .any(|envelope| envelope.maneuver == maneuver),
                "{} has no declared envelope",
                maneuver.label()
            );
            assert_eq!(
                SyntheticManeuver::from_label(maneuver.label()),
                Some(maneuver)
            );
        }

        let envelope = synthetic_trace_envelope("roll.peak_rate_radps")
            .expect("the roll envelope is declared");
        assert!(envelope.contains(1.5));
        assert_eq!(envelope.check(1.5), Ok(()));
        assert!(envelope.check(0.5).is_err(), "too slow is out of envelope");
        assert!(envelope.check(99.0).is_err(), "too fast is out of envelope");
        assert!(
            envelope.check(f64::NAN).is_err(),
            "an uninstrumented measurement is never in envelope"
        );
        assert_eq!(
            synthetic_trace_envelope("landing.rollout_m"),
            None,
            "an undeclared envelope does not exist"
        );
    }
}
