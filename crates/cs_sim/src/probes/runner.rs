//! The headless maneuver runner: flies a reference envelope's recorded input
//! through the production [`FlightModel`] and measures the quantity the
//! envelope bounds (F26-B).
//!
//! Spec: `specs/F26-handling-probes-and-original-behavior-calibration.md`,
//! stage `### F26-B`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! [`FlightModel::compute`] is a pure force function; it never advances a
//! body. [`ProbeRunner`] is the smallest fixed-step integrator that lets a
//! maneuver be *flown* without Bevy or Avian: semi-implicit Euler on a point
//! mass with a body-frame angular velocity and a diagonal inertia tensor.
//! It is a **measurement integrator only**. It is not the simulation's physics
//! pose owner, it runs no collision, ground or gyroscopic coupling, and its
//! numbers say nothing about how the original game integrated.
//!
//! **Determinism.** The runner does no I/O, reads no clock and draws no random
//! number, and every step is a pure function of the previous one, so the same
//! build on the same platform and inputs reproduces [`ProbeRun::trace_hash`]
//! bit for bit. That is promised only there: a different platform may differ in
//! the last floating-point bits, so cross-platform agreement is decided by the
//! envelope tolerance, never by comparing hashes.
//!
//! **Unknown stays unknown.** A maneuver whose quantity cannot be extracted
//! (a stall that never happens, a turn with no turn rate) is a
//! [`ProbeError::NotMeasurable`], and [`ProbeRunner::run_envelope`] leaves that
//! row out of the trace, so [`super::compare`] reports `NoMeasurement` rather
//! than a pass.
//!
//! The per-maneuver horizons in [`horizon_s`] are newly authored probe design,
//! not original measurements; the envelope schema records no duration
//! (`docs/findings/handling/2026-10-01-f26-b-headless-probe-runner.md`).

use cs_types::space::Quaternion;

use crate::flight::tuning::{AirframeTuning, DamageState, LoadoutMass, ModelKind};
use crate::flight::{
    BODY_FORWARD, BODY_UP, EngineState, FlightEnvironment, FlightError, FlightInput, FlightModel,
    FlightState,
};

use super::comparison::{ProbeMeasurement, ProbeTrace};
use super::envelope::{EnvelopeEntry, HandlingError, ProbeInitialState, ReferenceEnvelope};
use super::maneuver::ProbeKind;

/// The default fixed probe timestep, in seconds.
pub const DEFAULT_PROBE_DT_S: f64 = 1.0 / 120.0;

/// How long each maneuver is flown, in seconds.
///
/// Authored probe design; see the module documentation.
#[must_use]
pub const fn horizon_s(maneuver: ProbeKind) -> f64 {
    match maneuver {
        ProbeKind::Acceleration | ProbeKind::CoastDown | ProbeKind::Climb => 10.0,
        ProbeKind::Dive => 8.0,
        ProbeKind::Turn => 15.0,
        ProbeKind::Roll | ProbeKind::Yaw | ProbeKind::Damage => 3.0,
        ProbeKind::StallRecovery => 10.0,
        ProbeKind::Boost => 6.0,
    }
}

/// Why a maneuver could not be flown or measured.
#[derive(Clone, Debug, PartialEq)]
pub enum ProbeError {
    /// The envelope or entry was refused by its own validation.
    Envelope(HandlingError),
    /// The equations refused a tick.
    Flight(FlightError),
    /// The runner's timestep was not finite and strictly positive.
    InvalidTimestep,
    /// The runner only flies fixed-wing tuning; an exceptional airframe has its
    /// own control law.
    UnsupportedModel(ModelKind),
    /// The envelope and the tuning disagree on the model kind.
    ModelKindMismatch {
        /// The envelope's model kind.
        envelope: ModelKind,
        /// The tuning's model kind.
        tuning: ModelKind,
    },
    /// The first recorded input step does not start at time zero, so the
    /// command before it would be an assumption.
    ScheduleStartsLate {
        /// The maneuver whose schedule was refused.
        maneuver: ProbeKind,
    },
    /// The run completed but the quantity cannot be extracted from it.
    NotMeasurable {
        /// The maneuver.
        maneuver: ProbeKind,
        /// Why no value exists.
        reason: &'static str,
    },
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Envelope(error) => write!(f, "{error}"),
            Self::Flight(error) => write!(f, "{error}"),
            Self::InvalidTimestep => write!(f, "the probe timestep must be finite and positive"),
            Self::UnsupportedModel(kind) => {
                write!(
                    f,
                    "the {} model kind is not flown by this runner",
                    kind.label()
                )
            }
            Self::ModelKindMismatch { envelope, tuning } => write!(
                f,
                "the envelope is {} but the tuning is {}",
                envelope.label(),
                tuning.label()
            ),
            Self::ScheduleStartsLate { maneuver } => write!(
                f,
                "the {} input schedule does not start at time zero",
                maneuver.label()
            ),
            Self::NotMeasurable { maneuver, reason } => {
                write!(
                    f,
                    "the {} quantity is not measurable: {reason}",
                    maneuver.label()
                )
            }
        }
    }
}

impl std::error::Error for ProbeError {}

impl From<HandlingError> for ProbeError {
    fn from(error: HandlingError) -> Self {
        Self::Envelope(error)
    }
}

impl From<FlightError> for ProbeError {
    fn from(error: FlightError) -> Self {
        Self::Flight(error)
    }
}

/// One flown maneuver.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProbeRun {
    /// The maneuver flown.
    pub maneuver: ProbeKind,
    /// The measured value, in the maneuver quantity's canonical unit.
    pub value: f64,
    /// How many fixed ticks were integrated.
    pub ticks: u32,
    /// Boost capacity accepted over the run, in capacity units.
    pub boost_consumed: f64,
    /// FNV-1a over every tick's state bits; equal across repeat runs of the
    /// same build and platform only.
    pub trace_hash: u64,
}

/// The result of flying every entry of one envelope.
#[derive(Clone, Debug, PartialEq)]
pub struct EnvelopeRun {
    /// The measurements that could be extracted, ready for [`super::compare`].
    pub trace: ProbeTrace,
    /// Every maneuver that was flown.
    pub runs: Vec<ProbeRun>,
    /// Every maneuver that could not be flown or measured, with the reason.
    pub unmeasured: Vec<(ProbeKind, ProbeError)>,
    /// Combined hash of the individual run hashes, in envelope order.
    pub combined_hash: u64,
}

/// Flies maneuvers through one airframe's production [`FlightModel`].
#[derive(Clone, Debug, PartialEq)]
pub struct ProbeRunner {
    airframe_id: String,
    model: FlightModel,
    environment: FlightEnvironment,
    loadout: LoadoutMass,
    damage: DamageState,
    dt_s: f64,
}

impl ProbeRunner {
    /// A runner for `tuning` in the sea-level still-air environment with an
    /// empty loadout, pristine damage and [`DEFAULT_PROBE_DT_S`].
    #[must_use]
    pub fn new(airframe_id: impl Into<String>, tuning: AirframeTuning) -> Self {
        Self {
            airframe_id: airframe_id.into(),
            model: FlightModel::new(tuning),
            environment: FlightEnvironment::SEA_LEVEL,
            loadout: LoadoutMass::EMPTY,
            damage: DamageState::PRISTINE,
            dt_s: DEFAULT_PROBE_DT_S,
        }
    }

    /// Overrides the fixed timestep, in seconds.
    #[must_use]
    pub const fn with_timestep(mut self, dt_s: f64) -> Self {
        self.dt_s = dt_s;
        self
    }

    /// Overrides the loadout mass.
    #[must_use]
    pub const fn with_loadout(mut self, loadout: LoadoutMass) -> Self {
        self.loadout = loadout;
        self
    }

    /// Overrides the damage state.
    #[must_use]
    pub const fn with_damage(mut self, damage: DamageState) -> Self {
        self.damage = damage;
        self
    }

    /// Overrides the environment.
    #[must_use]
    pub const fn with_environment(mut self, environment: FlightEnvironment) -> Self {
        self.environment = environment;
        self
    }

    /// Flies every entry of `envelope` and collects the measurements.
    ///
    /// An entry that cannot be flown or measured is listed in
    /// [`EnvelopeRun::unmeasured`] and absent from the trace; it is never
    /// replaced by a default value.
    ///
    /// # Errors
    ///
    /// [`ProbeError`] when the envelope is invalid, the timestep is invalid or
    /// the tuning's model kind is unsupported or disagrees with the envelope.
    pub fn run_envelope(&self, envelope: &ReferenceEnvelope) -> Result<EnvelopeRun, ProbeError> {
        envelope.validate()?;
        self.check_ready()?;
        let tuning_kind = self.model.tuning().model_kind;
        if envelope.model_kind != tuning_kind {
            return Err(ProbeError::ModelKindMismatch {
                envelope: envelope.model_kind,
                tuning: tuning_kind,
            });
        }

        let mut runs = Vec::new();
        let mut unmeasured = Vec::new();
        for entry in &envelope.entries {
            match self.run_entry(entry) {
                Ok(run) => runs.push(run),
                Err(error) => unmeasured.push((entry.maneuver, error)),
            }
        }

        let mut hash = Fnv::new();
        for run in &runs {
            hash.u64(run.trace_hash);
        }
        let measurements = runs
            .iter()
            .map(|run| ProbeMeasurement {
                maneuver: run.maneuver,
                quantity: run.maneuver.quantity(),
                value: run.value,
            })
            .collect();
        Ok(EnvelopeRun {
            trace: ProbeTrace {
                airframe_id: self.airframe_id.clone(),
                origin: self.model.tuning().origin.clone(),
                measurements,
            },
            runs,
            unmeasured,
            combined_hash: hash.finish(),
        })
    }

    /// Flies one entry's recorded input and measures its quantity.
    ///
    /// # Errors
    ///
    /// [`ProbeError`] for an invalid entry or timestep, a refused tick or a
    /// quantity that cannot be extracted.
    pub fn run_entry(&self, entry: &EnvelopeEntry) -> Result<ProbeRun, ProbeError> {
        self.check_ready()?;
        entry.initial_state.validate()?;
        let first = entry
            .input
            .first()
            .ok_or(HandlingError::EmptyInputSchedule {
                maneuver: entry.maneuver,
            })?;
        if first.at_s > 0.0 {
            return Err(ProbeError::ScheduleStartsLate {
                maneuver: entry.maneuver,
            });
        }

        let flown = self.fly(entry, self.damage)?;
        let value = match entry.maneuver {
            ProbeKind::Damage => {
                let pristine = self.fly(entry, DamageState::PRISTINE)?;
                if pristine.peak_rate <= 1e-9 {
                    return Err(ProbeError::NotMeasurable {
                        maneuver: entry.maneuver,
                        reason: "the undamaged reference run produced no control response",
                    });
                }
                flown.peak_rate / pristine.peak_rate
            }
            maneuver => flown.measure(maneuver, &entry.initial_state, self.model.tuning())?,
        };
        Ok(ProbeRun {
            maneuver: entry.maneuver,
            value,
            ticks: flown.ticks,
            boost_consumed: flown.boost_consumed,
            trace_hash: flown.hash,
        })
    }

    fn check_ready(&self) -> Result<(), ProbeError> {
        if !self.dt_s.is_finite() || self.dt_s <= 0.0 {
            return Err(ProbeError::InvalidTimestep);
        }
        match self.model.tuning().model_kind {
            ModelKind::FixedWing => Ok(()),
            kind @ ModelKind::Exceptional => Err(ProbeError::UnsupportedModel(kind)),
        }
    }

    fn fly(&self, entry: &EnvelopeEntry, damage: DamageState) -> Result<Flown, ProbeError> {
        let tuning = self.model.tuning();
        let inertia = [
            tuning.mass.inertia_kg_m2[1],
            tuning.mass.inertia_kg_m2[2],
            tuning.mass.inertia_kg_m2[0],
        ];
        let total_mass_kg = self
            .loadout
            .total_mass_kg(tuning.mass.mass_kg)
            .map_err(FlightError::from)?;

        let start = &entry.initial_state;
        let mut orientation = [0.0, 0.0, 0.0, 1.0];
        let mut position = [0.0, start.altitude_m, 0.0];
        let mut velocity = [
            BODY_FORWARD[0] * start.airspeed_mps + BODY_UP[0] * start.vertical_speed_mps,
            BODY_FORWARD[1] * start.airspeed_mps + BODY_UP[1] * start.vertical_speed_mps,
            BODY_FORWARD[2] * start.airspeed_mps + BODY_UP[2] * start.vertical_speed_mps,
        ];
        let mut angular = [0.0; 3];
        let mut engine = EngineState::direct(start.engine_spool);
        let boost_available = start.boost_available;

        let horizon = horizon_s(entry.maneuver);
        // Rounded, not truncated, so a horizon that is a multiple of dt is
        // not shortened by one tick of floating-point noise.
        let ticks = (horizon / self.dt_s).round() as u32;
        let mut hash = Fnv::new();
        let mut samples = Vec::with_capacity(ticks as usize + 1);
        let mut boost_consumed = 0.0;
        let mut step_index = 0;

        samples.push(Sample::of(0.0, velocity, angular, 0.0));
        for tick in 0..ticks {
            let time = f64::from(tick) * self.dt_s;
            while step_index + 1 < entry.input.len()
                && entry.input[step_index + 1].at_s <= time + 1e-12
            {
                step_index += 1;
            }
            let input: FlightInput = entry.input[step_index].input;

            let state = FlightState {
                orientation: Quaternion::try_new(orientation).map_err(|_| {
                    FlightError::NonFinite {
                        field: "state.orientation",
                    }
                })?,
                linear_velocity_mps: velocity,
                angular_velocity_radps: angular,
                engine,
                boost_available,
            };
            let output = self.model.compute(
                &self.environment,
                &self.loadout,
                &damage,
                &state,
                &input,
                self.dt_s,
            )?;

            // Semi-implicit Euler: velocity first, then position.
            for axis in 0..3 {
                velocity[axis] += output.world_force_n[axis] / total_mass_kg * self.dt_s;
                position[axis] += velocity[axis] * self.dt_s;
            }
            let body_torque = rotate_inverse(output.world_torque_nm, orientation);
            for axis in 0..3 {
                angular[axis] += body_torque[axis] / inertia[axis] * self.dt_s;
            }
            orientation = integrate_orientation(orientation, angular, self.dt_s);
            engine.advance(
                input.throttle,
                tuning.engine.throttle_response_per_s,
                self.dt_s,
            );
            boost_consumed += output.accepted_boost_consumption;

            let sample = Sample::of(
                f64::from(tick + 1) * self.dt_s,
                velocity,
                angular,
                output.instrument_state.angle_of_attack_rad,
            );
            hash.floats(&position);
            hash.floats(&velocity);
            hash.floats(&angular);
            hash.floats(&orientation);
            samples.push(sample);
        }
        hash.u64(u64::from(ticks));

        let peak_rate = samples
            .iter()
            .map(|sample| norm(sample.angular))
            .fold(0.0, f64::max);
        Ok(Flown {
            samples,
            ticks,
            boost_consumed,
            hash: hash.finish(),
            peak_rate,
            dt_s: self.dt_s,
        })
    }
}

/// One recorded tick.
#[derive(Clone, Copy, Debug)]
struct Sample {
    time_s: f64,
    velocity: [f64; 3],
    angular: [f64; 3],
    angle_of_attack: f64,
}

impl Sample {
    const fn of(time_s: f64, velocity: [f64; 3], angular: [f64; 3], angle_of_attack: f64) -> Self {
        Self {
            time_s,
            velocity,
            angular,
            angle_of_attack,
        }
    }

    fn speed(&self) -> f64 {
        norm(self.velocity)
    }
}

/// A finished flight, before the maneuver's quantity is extracted.
struct Flown {
    samples: Vec<Sample>,
    ticks: u32,
    boost_consumed: f64,
    hash: u64,
    peak_rate: f64,
    dt_s: f64,
}

impl Flown {
    fn last(&self) -> &Sample {
        self.samples.last().expect("a flight records its start")
    }

    fn measure(
        &self,
        maneuver: ProbeKind,
        start: &ProbeInitialState,
        tuning: &AirframeTuning,
    ) -> Result<f64, ProbeError> {
        let not = |reason| ProbeError::NotMeasurable { maneuver, reason };
        let value = match maneuver {
            ProbeKind::Acceleration | ProbeKind::Dive | ProbeKind::Boost => {
                self.last().speed() - start.airspeed_mps.hypot(start.vertical_speed_mps)
            }
            ProbeKind::CoastDown => {
                start.airspeed_mps.hypot(start.vertical_speed_mps) - self.last().speed()
            }
            ProbeKind::Climb => self.last().velocity[1],
            ProbeKind::Roll => self.peak_body_rate(2),
            ProbeKind::Yaw => self.peak_body_rate(1),
            ProbeKind::Turn => self.turn_radius().ok_or_else(|| {
                not("the horizontal velocity did not turn over the measuring window")
            })?,
            ProbeKind::StallRecovery => self.stall_recovery_s(tuning.stall.stall_angle_rad)?,
            ProbeKind::Damage => unreachable!("damage is measured against a pristine run"),
        };
        if value.is_finite() {
            Ok(value)
        } else {
            Err(not("the extracted value was not finite"))
        }
    }

    fn peak_body_rate(&self, component: usize) -> f64 {
        self.samples
            .iter()
            .map(|sample| sample.angular[component].abs())
            .fold(0.0, f64::max)
    }

    /// `horizontal speed / turn rate`, over the last third of the run.
    fn turn_radius(&self) -> Option<f64> {
        let window_start = self.samples.len() - 1 - (self.samples.len() - 1) / 3;
        let first = &self.samples[window_start];
        let last = self.last();
        let heading = |sample: &Sample| (-sample.velocity[0]).atan2(-sample.velocity[2]);
        let mut delta = heading(last) - heading(first);
        // Wrap into (-pi, pi]: the window is short enough that less than half
        // a turn happens between two samples' headings.
        while delta > std::f64::consts::PI {
            delta -= std::f64::consts::TAU;
        }
        while delta <= -std::f64::consts::PI {
            delta += std::f64::consts::TAU;
        }
        let window_s = last.time_s - first.time_s;
        let rate = delta / window_s;
        if !window_s.is_finite() || window_s <= self.dt_s * 0.5 || rate.abs() < 1e-6 {
            return None;
        }
        let horizontal_speed = last.velocity[0].hypot(last.velocity[2]);
        Some(horizontal_speed / rate.abs())
    }

    /// Time from the first angle of attack past the stall angle to the first
    /// later tick back inside it.
    fn stall_recovery_s(&self, stall_angle_rad: f64) -> Result<f64, ProbeError> {
        let not = |reason| ProbeError::NotMeasurable {
            maneuver: ProbeKind::StallRecovery,
            reason,
        };
        let entry = self
            .samples
            .iter()
            .position(|sample| sample.angle_of_attack.abs() >= stall_angle_rad)
            .ok_or_else(|| not("the airframe never reached its stall angle"))?;
        let exit = self.samples[entry..]
            .iter()
            .position(|sample| sample.angle_of_attack.abs() < stall_angle_rad)
            .ok_or_else(|| not("the airframe had not recovered when the probe ended"))?;
        Ok(self.samples[entry + exit].time_s - self.samples[entry].time_s)
    }
}

fn norm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// Hamilton product `a ⊗ b` of `[x, y, z, w]` quaternions.
fn quat_mul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// Rotates a world vector into body space (the inverse of `orientation`).
fn rotate_inverse(vector: [f64; 3], orientation: [f64; 4]) -> [f64; 3] {
    let conjugate = [
        -orientation[0],
        -orientation[1],
        -orientation[2],
        orientation[3],
    ];
    let axis = [conjugate[0], conjugate[1], conjugate[2]];
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let axis_cross = cross(axis, vector);
    let second = cross(axis, axis_cross);
    [
        vector[0] + 2.0 * conjugate[3] * axis_cross[0] + 2.0 * second[0],
        vector[1] + 2.0 * conjugate[3] * axis_cross[1] + 2.0 * second[1],
        vector[2] + 2.0 * conjugate[3] * axis_cross[2] + 2.0 * second[2],
    ]
}

/// Applies a body-frame angular velocity for `dt_s` and renormalizes.
fn integrate_orientation(orientation: [f64; 4], body_rate: [f64; 3], dt_s: f64) -> [f64; 4] {
    let angle = norm(body_rate) * dt_s;
    let delta = if angle > 1e-12 {
        let (sin, cos) = (angle * 0.5).sin_cos();
        let scale = sin / norm(body_rate);
        [
            body_rate[0] * scale,
            body_rate[1] * scale,
            body_rate[2] * scale,
            cos,
        ]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    };
    let next = quat_mul(orientation, delta);
    let length =
        (next[0] * next[0] + next[1] * next[1] + next[2] * next[2] + next[3] * next[3]).sqrt();
    [
        next[0] / length,
        next[1] / length,
        next[2] / length,
        next[3] / length,
    ]
}

/// 64-bit FNV-1a over the raw bits of the values fed to it.
struct Fnv(u64);

impl Fnv {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn u64(&mut self, value: u64) {
        for byte in value.to_le_bytes() {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn floats(&mut self, values: &[f64]) {
        for value in values {
            self.u64(value.to_bits());
        }
    }

    const fn finish(&self) -> u64 {
        self.0
    }
}
