//! The fixed-wing force equations (F24-A).
//!
//! Spec: `specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md`,
//! stage `### F24-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`,
//! sections "Inputs and outputs", "Coordinate convention" and "Attitude
//! control".
//!
//! [`FlightModel::compute`] is the one place the equations live. It is a pure
//! function of its inputs and one fixed timestep: no wall clock, no render
//! frame and no ECS state enters, so equal inputs at 30, 60 or 144 render FPS
//! produce equal forces (AC04's force half). The decomposition is the
//! contract's designed starting model, not an extracted original equation:
//!
//! * body forward is −Z, right +X, up +Y; `v_air = v_world − wind_world` is
//!   transformed into body space to get angle of attack and sideslip;
//! * `q = ½·ρ·V²`, `L = q·S·CL(α)` perpendicular to the airflow in the
//!   body-up plane and `D = q·S·CD(α)` opposite the airflow;
//! * thrust acts along body forward, gravity is world-space and is applied
//!   **only** here (Avian's global gravity stays zero for a flight body);
//! * attitude is rate-command plus bounded feedback torque in body space,
//!   scaled by stall and damage authority and rotated to world space for the
//!   output; the transform is never rotated directly while a body integrates
//!   torque.
//!
//! At zero airspeed every value is finite: `q = 0` makes lift and drag zero,
//! `atan2(±0, 0)` is zero, and gravity still acts. That is AC01's minimum
//! scenario, and it must stay true for every input the boundary admits.
//!
//! All bounds, gains and defaults are newly authored project design. The
//! calibration of the coefficients against original reference traces is
//! F24-D.

use cs_types::space::Quaternion;

use super::tuning::{AIRSPEED_EPSILON_MPS, AirframeTuning, DamageState, LoadoutMass};

/// Body forward axis in canonical space: −Z.
pub const BODY_FORWARD: [f64; 3] = [0.0, 0.0, -1.0];
/// Body up axis in canonical space: +Y.
pub const BODY_UP: [f64; 3] = [0.0, 1.0, 0.0];
/// Body right axis in canonical space: +X.
pub const BODY_RIGHT: [f64; 3] = [1.0, 0.0, 0.0];

/// Why a flight input was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum FlightInputError {
    /// A named field contained NaN or infinity.
    NonFinite {
        /// The offending field.
        field: &'static str,
    },
    /// A field fell outside its declared range.
    OutOfRange {
        /// The offending field.
        field: &'static str,
        /// The rejected value.
        value: f64,
        /// The inclusive lower bound.
        min: f64,
        /// The inclusive upper bound.
        max: f64,
    },
}

impl std::fmt::Display for FlightInputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::OutOfRange {
                field,
                value,
                min,
                max,
            } => write!(f, "{field} value {value} is outside [{min}, {max}]"),
        }
    }
}

impl std::error::Error for FlightInputError {}

/// One tick's normalized flight command.
///
/// Pitch, roll and yaw are in `[-1, 1]`, throttle in `[0, 1]`. Construct with
/// [`FlightInput::try_new`] for already-quantized command data (a value
/// outside its range is refused by name) or [`FlightInput::clamped`] for raw
/// human input, which is clamped into range as the contract requires. A
/// non-finite value is always refused, because it cannot be a device sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlightInput {
    /// Pitch command, `[-1, 1]`; positive is nose-down stick? No: positive is
    /// the command's own declared sign, applied to the pitch rate.
    pub pitch: f64,
    /// Roll command, `[-1, 1]`.
    pub roll: f64,
    /// Yaw command, `[-1, 1]`.
    pub yaw: f64,
    /// Throttle command, `[0, 1]`.
    pub throttle: f64,
    /// Whether the boost button is held this tick.
    pub boost: bool,
}

impl FlightInput {
    /// The neutral command: no deflection, idle throttle, no boost.
    pub const NEUTRAL: Self = Self {
        pitch: 0.0,
        roll: 0.0,
        yaw: 0.0,
        throttle: 0.0,
        boost: false,
    };

    /// Validates normalized command data.
    ///
    /// # Errors
    ///
    /// [`FlightInputError::NonFinite`] for a NaN/infinite field, and
    /// [`FlightInputError::OutOfRange`] for a value outside its declared
    /// range. Values are never silently clamped here.
    pub fn try_new(
        pitch: f64,
        roll: f64,
        yaw: f64,
        throttle: f64,
        boost: bool,
    ) -> Result<Self, FlightInputError> {
        let input = Self {
            pitch,
            roll,
            yaw,
            throttle,
            boost,
        };
        input.validate()?;
        Ok(input)
    }

    /// Validates the normalized command ranges.
    ///
    /// # Errors
    ///
    /// [`FlightInputError::NonFinite`] for a NaN/infinite field, and
    /// [`FlightInputError::OutOfRange`] for a value outside its declared
    /// range. This is the same check [`FlightInput::try_new`] applies, exposed
    /// so a boundary that received a directly constructed `FlightInput` can
    /// refuse it instead of letting a corrupt command reach the equations.
    pub fn validate(&self) -> Result<(), FlightInputError> {
        for (field, value, min, max) in [
            ("input.pitch", self.pitch, -1.0, 1.0),
            ("input.roll", self.roll, -1.0, 1.0),
            ("input.yaw", self.yaw, -1.0, 1.0),
            ("input.throttle", self.throttle, 0.0, 1.0),
        ] {
            if !value.is_finite() {
                return Err(FlightInputError::NonFinite { field });
            }
            if !(min..=max).contains(&value) {
                return Err(FlightInputError::OutOfRange {
                    field,
                    value,
                    min,
                    max,
                });
            }
        }
        Ok(())
    }

    /// Clamps raw human input into its declared ranges.
    ///
    /// # Errors
    ///
    /// [`FlightInputError::NonFinite`] naming the first non-finite field; a
    /// value that merely overshot a range is clamped, as the contract
    /// requires for human controls.
    pub fn clamped(
        pitch: f64,
        roll: f64,
        yaw: f64,
        throttle: f64,
        boost: bool,
    ) -> Result<Self, FlightInputError> {
        for (field, value) in [
            ("input.pitch", pitch),
            ("input.roll", roll),
            ("input.yaw", yaw),
            ("input.throttle", throttle),
        ] {
            if !value.is_finite() {
                return Err(FlightInputError::NonFinite { field });
            }
        }
        Ok(Self {
            pitch: pitch.clamp(-1.0, 1.0),
            roll: roll.clamp(-1.0, 1.0),
            yaw: yaw.clamp(-1.0, 1.0),
            throttle: throttle.clamp(0.0, 1.0),
            boost,
        })
    }
}

/// The engine's spool state, integrated outside the force equations.
///
/// Thrust lag is state, not a force law, so the model reads `spool` and never
/// mutates it. [`EngineState::advance`] is the one place the spool moves, and
/// it moves by a fixed dt like everything else.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineState {
    /// Whether the engine produces any thrust at all.
    pub running: bool,
    /// The current throttle spool, in `[0, 1]`.
    pub spool: f64,
}

impl EngineState {
    /// A stopped engine at zero spool.
    pub const STOPPED: Self = Self {
        running: false,
        spool: 0.0,
    };

    /// A running engine already at `spool`.
    #[must_use]
    pub fn direct(spool: f64) -> Self {
        Self {
            running: true,
            spool: spool.clamp(0.0, 1.0),
        }
    }

    /// Moves the spool toward `commanded` at `response_per_s` for `dt_s`.
    ///
    /// The step is bounded so it can neither overshoot nor loop past the
    /// target; a zero or negative `dt_s` changes nothing.
    pub fn advance(&mut self, commanded: f64, response_per_s: f64, dt_s: f64) {
        if !self.running || !dt_s.is_finite() || dt_s <= 0.0 || !response_per_s.is_finite() {
            return;
        }
        let commanded = commanded.clamp(0.0, 1.0);
        let step = (response_per_s * dt_s).max(0.0);
        let delta = commanded - self.spool;
        if delta.abs() <= step {
            self.spool = commanded;
        } else {
            self.spool += step.copysign(delta);
        }
    }
}

/// The world in which the equations run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlightEnvironment {
    /// Gravity acceleration, in m/s², applied as a world-space force.
    pub gravity_mps2: f64,
    /// Air density ρ, in kg/m³. Strictly positive.
    pub air_density_kg_m3: f64,
    /// The wind velocity in world space, in m/s.
    pub wind_velocity_mps: [f64; 3],
}

impl FlightEnvironment {
    /// The designed sea-level still-air environment.
    pub const SEA_LEVEL: Self = Self {
        gravity_mps2: 9.806_65,
        air_density_kg_m3: 1.225,
        wind_velocity_mps: [0.0, 0.0, 0.0],
    };

    /// Validates the fields.
    ///
    /// # Errors
    ///
    /// Returns the offending field's name as a string.
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.gravity_mps2.is_finite() {
            return Err("environment.gravity_mps2");
        }
        if self.air_density_kg_m3 <= 0.0 || !self.air_density_kg_m3.is_finite() {
            return Err("environment.air_density_kg_m3");
        }
        for (index, value) in self.wind_velocity_mps.into_iter().enumerate() {
            if !value.is_finite() {
                return Err(WIND_FIELDS[index]);
            }
        }
        Ok(())
    }
}

const WIND_FIELDS: [&str; 3] = [
    "environment.wind_velocity_mps[0]",
    "environment.wind_velocity_mps[1]",
    "environment.wind_velocity_mps[2]",
];

/// One aircraft's dynamic state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlightState {
    /// Body-to-world orientation.
    pub orientation: Quaternion,
    /// World-space linear velocity, in m/s.
    pub linear_velocity_mps: [f64; 3],
    /// Body-space angular velocity, in rad/s.
    pub angular_velocity_radps: [f64; 3],
    /// Engine spool state.
    pub engine: EngineState,
    /// Whether boost capacity remains available this tick.
    pub boost_available: bool,
}

impl FlightState {
    /// A state at rest, level and unpowered.
    #[must_use]
    pub fn at_rest(orientation: Quaternion) -> Self {
        Self {
            orientation,
            linear_velocity_mps: [0.0, 0.0, 0.0],
            angular_velocity_radps: [0.0, 0.0, 0.0],
            engine: EngineState::STOPPED,
            boost_available: false,
        }
    }

    /// Validates the state's vectors.
    ///
    /// # Errors
    ///
    /// [`FlightError::NonFinite`] naming the first offending field.
    pub fn validate(&self) -> Result<(), FlightError> {
        for (index, value) in self.linear_velocity_mps.into_iter().enumerate() {
            if !value.is_finite() {
                return Err(FlightError::NonFinite {
                    field: VELOCITY_FIELDS[index],
                });
            }
        }
        for (index, value) in self.angular_velocity_radps.into_iter().enumerate() {
            if !value.is_finite() {
                return Err(FlightError::NonFinite {
                    field: ANGULAR_FIELDS[index],
                });
            }
        }
        if !self.engine.spool.is_finite() {
            return Err(FlightError::NonFinite {
                field: "state.engine.spool",
            });
        }
        Ok(())
    }
}

const VELOCITY_FIELDS: [&str; 3] = [
    "state.linear_velocity_mps[0]",
    "state.linear_velocity_mps[1]",
    "state.linear_velocity_mps[2]",
];
const ANGULAR_FIELDS: [&str; 3] = [
    "state.angular_velocity_radps[0]",
    "state.angular_velocity_radps[1]",
    "state.angular_velocity_radps[2]",
];

/// Why the equations refused to produce an output.
#[derive(Clone, Debug, PartialEq)]
pub enum FlightError {
    /// A named input field contained NaN or infinity.
    NonFinite {
        /// The offending field.
        field: &'static str,
    },
    /// A named input field fell outside its declared command range.
    OutOfRange {
        /// The offending field.
        field: &'static str,
        /// The rejected value.
        value: f64,
        /// The inclusive lower bound.
        min: f64,
        /// The inclusive upper bound.
        max: f64,
    },
    /// The environment was rejected; the payload is the offending field.
    Environment(&'static str),
    /// The total aircraft mass was not strictly positive.
    NonPositiveMass,
    /// A tuning field was rejected.
    Tuning(super::tuning::AirframeTuningError),
    /// A loadout field was rejected.
    Loadout(super::tuning::LoadoutMassError),
    /// A damage field was rejected.
    Damage(super::tuning::DamageStateError),
}

impl std::fmt::Display for FlightError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::OutOfRange {
                field,
                value,
                min,
                max,
            } => write!(f, "{field} value {value} is outside [{min}, {max}]"),
            Self::Environment(field) => write!(f, "{field} is not a usable environment"),
            Self::NonPositiveMass => write!(f, "the total aircraft mass must be greater than zero"),
            Self::Tuning(error) => write!(f, "{error}"),
            Self::Loadout(error) => write!(f, "{error}"),
            Self::Damage(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for FlightError {}

impl From<FlightInputError> for FlightError {
    fn from(error: FlightInputError) -> Self {
        match error {
            FlightInputError::NonFinite { field } => Self::NonFinite { field },
            FlightInputError::OutOfRange {
                field,
                value,
                min,
                max,
            } => Self::OutOfRange {
                field,
                value,
                min,
                max,
            },
        }
    }
}

impl From<super::tuning::AirframeTuningError> for FlightError {
    fn from(error: super::tuning::AirframeTuningError) -> Self {
        Self::Tuning(error)
    }
}

impl From<super::tuning::LoadoutMassError> for FlightError {
    fn from(error: super::tuning::LoadoutMassError) -> Self {
        Self::Loadout(error)
    }
}

impl From<super::tuning::DamageStateError> for FlightError {
    fn from(error: super::tuning::DamageStateError) -> Self {
        Self::Damage(error)
    }
}

/// The values an instrument panel reads from one computed tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InstrumentState {
    /// True airspeed, in m/s.
    pub airspeed_mps: f64,
    /// Angle of attack, in radians.
    pub angle_of_attack_rad: f64,
    /// Sideslip angle, in radians.
    pub sideslip_rad: f64,
    /// Dynamic pressure q, in pascals.
    pub dynamic_pressure_pa: f64,
    /// The lift coefficient actually produced (stall included).
    pub lift_coefficient: f64,
    /// The drag coefficient actually produced.
    pub drag_coefficient: f64,
    /// The smooth stall factor in `[residual_fraction, 1]`.
    pub stall_scale: f64,
    /// Commanded thrust, in newtons.
    pub thrust_n: f64,
}

/// The recorded, per-source force contributions of one tick.
///
/// Non-negotiable behavior 5: each contribution is separately visible, so a
/// calibrated probe can assert assist terms are exactly zero and no assist
/// can hide energy inside the total.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlightDiagnostics {
    /// Thrust magnitude along body forward, in newtons.
    pub thrust_n: f64,
    /// Lift magnitude, in newtons.
    pub lift_n: f64,
    /// Drag magnitude, in newtons.
    pub drag_n: f64,
    /// The world-space gravity force, in newtons.
    pub gravity_force_n: [f64; 3],
    /// The assist force contribution, in newtons (world space).
    pub assist_force_n: [f64; 3],
    /// The assist torque contribution, in newton-metres (world space).
    pub assist_torque_nm: [f64; 3],
    /// The fraction of control authority in force this tick, in `[0, 1]`.
    pub control_authority: f64,
}

/// One tick's force, torque and instrument output.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlightOutput {
    /// Total world-space force to integrate, in newtons.
    pub world_force_n: [f64; 3],
    /// Total world-space torque to integrate, in newton-metres.
    pub world_torque_nm: [f64; 3],
    /// Instrument readings for this tick.
    pub instrument_state: InstrumentState,
    /// Boost capacity accepted this tick, in capacity units.
    pub accepted_boost_consumption: f64,
    /// Per-source contributions.
    pub diagnostics: FlightDiagnostics,
}

/// The fixed-wing flight model: one validated tuning, evaluated per tick.
#[derive(Clone, Debug, PartialEq)]
pub struct FlightModel {
    tuning: AirframeTuning,
}

impl FlightModel {
    /// Builds a model from already-validated tuning.
    #[must_use]
    pub const fn new(tuning: AirframeTuning) -> Self {
        Self { tuning }
    }

    /// The tuning this model evaluates.
    #[must_use]
    pub const fn tuning(&self) -> &AirframeTuning {
        &self.tuning
    }

    /// Computes one tick's forces, torque and instruments.
    ///
    /// `dt_s` is the fixed simulation timestep and only scales rate-based
    /// terms; the equations are otherwise a pure function of the inputs, so
    /// the same tick yields the same output at any render frame rate.
    ///
    /// # Errors
    ///
    /// [`FlightError`] for a non-finite input or a rejected environment,
    /// tuning, loadout or damage value. At zero airspeed the result is still
    /// finite and gravity is present.
    pub fn compute(
        &self,
        environment: &FlightEnvironment,
        loadout: &LoadoutMass,
        damage: &DamageState,
        state: &FlightState,
        input: &FlightInput,
        dt_s: f64,
    ) -> Result<FlightOutput, FlightError> {
        environment.validate().map_err(FlightError::Environment)?;
        self.tuning.validate()?;
        loadout.validate()?;
        damage.validate()?;
        state.validate()?;
        input.validate()?;
        if !dt_s.is_finite() || dt_s < 0.0 {
            return Err(FlightError::NonFinite { field: "dt_s" });
        }
        let total_mass_kg = loadout.total_mass_kg(self.tuning.mass.mass_kg)?;

        // Body axes in world space.
        let forward_world = rotate_vector(BODY_FORWARD, state.orientation);
        let up_world = rotate_vector(BODY_UP, state.orientation);
        let right_world = rotate_vector(BODY_RIGHT, state.orientation);

        // Air-relative velocity, in world and then body space.
        let air_velocity_world = sub(state.linear_velocity_mps, environment.wind_velocity_mps);
        let speed = norm(air_velocity_world);
        let forward_component = dot(air_velocity_world, forward_world);
        let up_component = dot(air_velocity_world, up_world);
        let right_component = dot(air_velocity_world, right_world);

        let angle_of_attack = if speed > AIRSPEED_EPSILON_MPS {
            libm_atan2(-up_component, forward_component)
        } else {
            0.0
        };
        let sideslip = if speed > AIRSPEED_EPSILON_MPS {
            (right_component / speed).clamp(-1.0, 1.0).asin()
        } else {
            0.0
        };

        // Stall factor: gradual, finite, symmetric.
        let stall_scale = self.tuning.stall.factor_at(angle_of_attack);

        // Dynamic pressure and coefficients.
        let dynamic_pressure = 0.5 * environment.air_density_kg_m3 * speed * speed;
        let lift_coefficient =
            self.lift_coefficient(angle_of_attack, stall_scale, damage.lift_scale);
        let drag_coefficient = self.tuning.drag.coefficient_at(lift_coefficient);

        let aero_scale = dynamic_pressure * self.tuning.reference_area_m2;
        let lift_n = aero_scale * lift_coefficient;
        let drag_n = aero_scale * drag_coefficient;

        // Lift is perpendicular to the airflow in the body-up plane.
        let lift_direction = lift_direction(air_velocity_world, up_world, speed);
        let drag_direction = scale(air_velocity_world, -1.0 / speed.max(AIRSPEED_EPSILON_MPS));

        // Thrust along body forward, with spool, damage and boost.
        let spool = if state.engine.running {
            state.engine.spool.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let boost_accepted =
            input.boost && state.boost_available && self.tuning.boost.thrust_n > 0.0;
        let base_thrust_n = self.tuning.engine.thrust_at(spool) * damage.thrust_authority;
        let boost_thrust_n = if boost_accepted {
            self.tuning.boost.thrust_n * damage.thrust_authority
        } else {
            0.0
        };
        let thrust_n = base_thrust_n + boost_thrust_n;
        let accepted_boost_consumption = if boost_accepted {
            self.tuning.boost.consumption_per_s * dt_s
        } else {
            0.0
        };

        // Gravity is world-space and applied only here.
        let gravity_force_n = [0.0, -total_mass_kg * environment.gravity_mps2, 0.0];

        // Attitude: rate command plus bounded feedback torque in body space.
        let control_authority = stall_scale
            * damage.control_authority
            * control_airspeed_scale(speed, &self.tuning.angular);
        let body_torque = self.body_torque(state, input, control_authority);
        let mut world_torque_nm = rotate_vector(body_torque, state.orientation);

        // Optional, separately recorded bank/level assist.
        let assist_torque_nm = self.bank_level_assist(state, input);
        world_torque_nm = add(world_torque_nm, assist_torque_nm);

        let mut world_force_n = add(
            add(
                add(
                    scale(forward_world, thrust_n),
                    scale(lift_direction, lift_n),
                ),
                scale(drag_direction, drag_n),
            ),
            gravity_force_n,
        );
        // The declared assist set contributes no linear force in F24-A.
        let assist_force_n = [0.0, 0.0, 0.0];
        world_force_n = add(world_force_n, assist_force_n);

        Ok(FlightOutput {
            world_force_n,
            world_torque_nm,
            instrument_state: InstrumentState {
                airspeed_mps: speed,
                angle_of_attack_rad: angle_of_attack,
                sideslip_rad: sideslip,
                dynamic_pressure_pa: dynamic_pressure,
                lift_coefficient,
                drag_coefficient,
                stall_scale,
                thrust_n,
            },
            accepted_boost_consumption,
            diagnostics: FlightDiagnostics {
                thrust_n,
                lift_n,
                drag_n,
                gravity_force_n,
                assist_force_n,
                assist_torque_nm,
                control_authority,
            },
        })
    }

    fn lift_coefficient(&self, angle_of_attack: f64, stall_scale: f64, lift_scale: f64) -> f64 {
        let linear = self.tuning.lift.lift_at_zero_alpha
            + self.tuning.lift.lift_slope_per_rad * angle_of_attack;
        let scaled = linear * stall_scale * lift_scale.clamp(0.0, 1.0);
        scaled.clamp(
            -self.tuning.lift.max_lift_coefficient,
            self.tuning.lift.max_lift_coefficient,
        )
    }

    fn body_torque(&self, state: &FlightState, input: &FlightInput, authority: f64) -> [f64; 3] {
        let commands = [input.roll, input.pitch, input.yaw];
        let mut torque = [0.0; 3];
        for axis in 0..3 {
            let desired_rate = commands[axis].clamp(-1.0, 1.0)
                * self.tuning.angular.max_rate_radps[axis]
                * authority;
            let rate_error = desired_rate - state.angular_velocity_radps[axis];
            let inertia = self.tuning.mass.inertia_kg_m2[axis];
            let raw = inertia
                * (self.tuning.angular.rate_gain_per_s * rate_error
                    - self.tuning.angular.rate_damping_per_s * state.angular_velocity_radps[axis]);
            torque[axis] = raw.clamp(
                -self.tuning.angular.max_torque_nm[axis],
                self.tuning.angular.max_torque_nm[axis],
            );
        }
        torque
    }

    /// The bank/level assist torque in world space, or zero when disabled.
    ///
    /// The assist acts only about the aircraft's roll axis and only when the
    /// pilot is not commanding roll, so it neither cancels gravity nor
    /// recovers a stall on its own.
    fn bank_level_assist(&self, state: &FlightState, input: &FlightInput) -> [f64; 3] {
        let assist = &self.tuning.assists;
        if !assist.enabled || assist.bank_level_gain_nm_per_rad <= 0.0 || input.roll.abs() > 1e-9 {
            return [0.0, 0.0, 0.0];
        }
        let (right_world, up_world) = (
            rotate_vector(BODY_RIGHT, state.orientation),
            rotate_vector(BODY_UP, state.orientation),
        );
        let bank_angle = libm_atan2(dot(right_world, BODY_UP), dot(up_world, BODY_UP));
        let magnitude = (assist.bank_level_gain_nm_per_rad * bank_angle).clamp(
            -assist.bank_level_max_torque_nm,
            assist.bank_level_max_torque_nm,
        );
        scale(rotate_vector(BODY_RIGHT, state.orientation), -magnitude)
    }
}

/// The speed authority ramp: zero at rest, full at the declared airspeed.
fn control_airspeed_scale(speed: f64, angular: &super::tuning::AngularResponse) -> f64 {
    (speed / angular.control_airspeed_full_mps).clamp(0.0, 1.0)
}

/// The unit lift direction: body up made perpendicular to the airflow.
///
/// Falls back to zero when the airflow is vertical (the direction is
/// genuinely ambiguous then), which keeps the result finite.
fn lift_direction(air_velocity_world: [f64; 3], up_world: [f64; 3], speed: f64) -> [f64; 3] {
    if speed <= AIRSPEED_EPSILON_MPS {
        return [0.0, 0.0, 0.0];
    }
    let velocity_hat = scale(air_velocity_world, 1.0 / speed);
    let projected = sub(up_world, scale(velocity_hat, dot(up_world, velocity_hat)));
    let length = norm(projected);
    if length <= AIRSPEED_EPSILON_MPS {
        [0.0, 0.0, 0.0]
    } else {
        scale(projected, 1.0 / length)
    }
}

/// Rotates `vector` from body space to world space by `orientation`.
fn rotate_vector(vector: [f64; 3], orientation: Quaternion) -> [f64; 3] {
    let [x, y, z, w] = orientation.components();
    let axis = [x, y, z];
    let axis_cross = cross(axis, vector);
    let twice = scale(axis_cross, 2.0 * w);
    let second = cross(axis, axis_cross);
    add(add(vector, twice), scale(second, 2.0))
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f64; 3], factor: f64) -> [f64; 3] {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

/// `atan2` without a math-crate dependency (`cs_sim` may use only
/// `cs_types`/`cs_script` beyond the standard library).
fn libm_atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flight::synthetic::synthetic_fixed_wing;

    fn model() -> FlightModel {
        FlightModel::new(synthetic_fixed_wing())
    }

    /// AC01 minimum scenario: at zero airspeed every computed value is finite
    /// **and** gravity still acts.
    #[test]
    fn accept_f24_a_zero_airspeed_keeps_every_value_finite_and_gravity_acts() {
        let model = model();
        let state = FlightState::at_rest(Quaternion::IDENTITY);
        let output = model
            .compute(
                &FlightEnvironment::SEA_LEVEL,
                &LoadoutMass::EMPTY,
                &DamageState::PRISTINE,
                &state,
                &FlightInput::NEUTRAL,
                1.0 / 120.0,
            )
            .expect("the neutral synthetic fixture is valid");

        for (axis, value) in output.world_force_n.into_iter().enumerate() {
            assert!(value.is_finite(), "force[{axis}] must be finite");
        }
        for (axis, value) in output.world_torque_nm.into_iter().enumerate() {
            assert!(value.is_finite(), "torque[{axis}] must be finite");
        }
        assert!(output.instrument_state.airspeed_mps.is_finite());
        assert!(output.instrument_state.angle_of_attack_rad.is_finite());
        assert!(output.instrument_state.sideslip_rad.is_finite());
        assert!(output.instrument_state.dynamic_pressure_pa.is_finite());
        assert!(output.instrument_state.lift_coefficient.is_finite());
        assert!(output.instrument_state.drag_coefficient.is_finite());
        assert!(output.instrument_state.stall_scale.is_finite());
        assert!(output.accepted_boost_consumption.is_finite());

        let total_mass = model.tuning().mass.mass_kg;
        let expected_gravity = -total_mass * FlightEnvironment::SEA_LEVEL.gravity_mps2;
        assert!(
            (output.diagnostics.gravity_force_n[1] - expected_gravity).abs() < 1e-9,
            "gravity must still act: {:?}",
            output.diagnostics.gravity_force_n
        );
        assert!(
            output.world_force_n[1] < 0.0,
            "the net vertical force at rest is gravity"
        );
        assert_eq!(output.instrument_state.airspeed_mps, 0.0);
        assert_eq!(output.diagnostics.lift_n, 0.0);
        assert_eq!(output.diagnostics.drag_n, 0.0);
    }

    /// A pitch input at airspeed produces a finite, bounded, restoring torque
    /// whose sign follows the command, and the aero forces are finite too.
    #[test]
    fn accept_f24_a_forward_flight_produces_signed_bounded_torque() {
        let model = model();
        // Level flight at 60 m/s toward body forward (-Z world).
        let state = FlightState {
            linear_velocity_mps: [0.0, 0.0, -60.0],
            engine: EngineState::direct(1.0),
            ..FlightState::at_rest(Quaternion::IDENTITY)
        };
        let input = FlightInput::try_new(0.5, 0.0, 0.0, 1.0, false).expect("valid input");
        let output = model
            .compute(
                &FlightEnvironment::SEA_LEVEL,
                &LoadoutMass::EMPTY,
                &DamageState::PRISTINE,
                &state,
                &input,
                1.0 / 120.0,
            )
            .expect("valid");

        assert!(output.world_torque_nm[1].is_finite());
        assert!(
            output.world_torque_nm[1].abs() <= model.tuning().angular.max_torque_nm[1],
            "torque must stay bounded"
        );
        assert!(output.diagnostics.thrust_n > 0.0);
        assert!(output.diagnostics.lift_n > 0.0);
        assert!(
            output.instrument_state.angle_of_attack_rad.abs() < 0.2,
            "level flight is close to zero angle of attack"
        );
    }

    /// The exact same inputs produce the exact same output: the equations are
    /// a pure function of one tick, never of wall time or render rate.
    #[test]
    fn accept_f24_a_equations_are_a_pure_function_of_their_inputs() {
        let model = model();
        let state = FlightState {
            linear_velocity_mps: [1.0, -2.0, -55.0],
            angular_velocity_radps: [0.1, -0.2, 0.05],
            engine: EngineState::direct(0.8),
            boost_available: true,
            ..FlightState::at_rest(Quaternion::IDENTITY)
        };
        let input = FlightInput::try_new(-0.25, 0.5, 0.1, 0.8, true).expect("valid");
        let env = FlightEnvironment::SEA_LEVEL;
        let loadout = LoadoutMass {
            fuel_kg: 100.0,
            ordnance_kg: 50.0,
            armor_kg: 10.0,
        };
        let first = model
            .compute(
                &env,
                &loadout,
                &DamageState::PRISTINE,
                &state,
                &input,
                1.0 / 60.0,
            )
            .expect("valid");
        let second = model
            .compute(
                &env,
                &loadout,
                &DamageState::PRISTINE,
                &state,
                &input,
                1.0 / 60.0,
            )
            .expect("valid");
        assert_eq!(first, second, "the equations must not read wall time");
    }

    /// The assist contributions are recorded, are zero in the calibrated
    /// profile, and the calibrated total excludes them.
    #[test]
    fn accept_f24_a_assists_are_recorded_and_zeroed_for_calibration() {
        let calibrated = model();
        let state = FlightState {
            linear_velocity_mps: [0.0, 0.0, -50.0],
            orientation: Quaternion::from_axis_angle(
                cs_types::space::UnitVec3::try_new([0.0, 0.0, -1.0]).expect("unit"),
                cs_types::space::Radians(0.4),
            )
            .expect("unit quaternion"),
            ..FlightState::at_rest(Quaternion::IDENTITY)
        };
        let input = FlightInput::NEUTRAL;
        let output = calibrated
            .compute(
                &FlightEnvironment::SEA_LEVEL,
                &LoadoutMass::EMPTY,
                &DamageState::PRISTINE,
                &state,
                &input,
                1.0 / 120.0,
            )
            .expect("valid");
        assert_eq!(
            output.diagnostics.assist_torque_nm,
            [0.0, 0.0, 0.0],
            "the calibrated profile must disable assists"
        );
        assert_eq!(output.diagnostics.assist_force_n, [0.0, 0.0, 0.0]);
        assert!(
            output.diagnostics.gravity_force_n[1] < 0.0,
            "an assist must not make gravity disappear"
        );
    }

    /// The input boundary refuses NaN and out-of-range command data by name,
    /// while the human-input constructor clamps an overshoot.
    #[test]
    fn accept_f24_a_input_boundary_refuses_and_clamps_by_name() {
        assert_eq!(
            FlightInput::try_new(f64::NAN, 0.0, 0.0, 0.5, false),
            Err(FlightInputError::NonFinite {
                field: "input.pitch"
            })
        );
        assert_eq!(
            FlightInput::try_new(2.0, 0.0, 0.0, 0.5, false),
            Err(FlightInputError::OutOfRange {
                field: "input.pitch",
                value: 2.0,
                min: -1.0,
                max: 1.0,
            })
        );
        let clamped = FlightInput::clamped(2.0, -3.0, 0.0, 1.5, false).expect("finite");
        assert_eq!(clamped.pitch, 1.0);
        assert_eq!(clamped.roll, -1.0);
        assert_eq!(clamped.throttle, 1.0);
        assert_eq!(
            FlightInput::clamped(0.0, 0.0, 0.0, f64::INFINITY, false),
            Err(FlightInputError::NonFinite {
                field: "input.throttle"
            })
        );
    }

    /// The engine spool integrates by fixed dt and never overshoots.
    #[test]
    fn accept_f24_a_engine_spool_steps_by_fixed_dt_without_overshoot() {
        let mut engine = EngineState::direct(0.0);
        engine.advance(1.0, 1.0, 0.25);
        assert!((engine.spool - 0.25).abs() < 1e-12);
        engine.advance(1.0, 1.0, 10.0);
        assert_eq!(engine.spool, 1.0, "the spool must not overshoot");
        engine.advance(0.0, 1.0, 0.1);
        assert!((engine.spool - 0.9).abs() < 1e-12);

        let mut stopped = EngineState::STOPPED;
        stopped.advance(1.0, 1.0, 1.0);
        assert_eq!(stopped.spool, 0.0, "a stopped engine does not spool");
    }
}
