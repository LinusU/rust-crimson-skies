//! Exceptional flight configurations: the shared telemetry interface, the
//! rotor drive, the reference maneuver envelope (F25-A) and the declared
//! exceptional control law (F25-B).
//!
//! Spec: `specs/F25-hoplite-autogyro-and-exceptional-flight-configurations.md`,
//! stages `### F25-A` and `### F25-B`. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`, sections "Inputs and outputs", "Boost and
//! special models" and "Calibration acceptance".
//!
//! **F25-A is the typed boundary, not a control law.** The sheet asks for
//! "typed inputs/outputs and a minimal synthetic fixture first; do not jump
//! ahead to a whole runtime", and that half of the module is exactly that. It
//! adds three things the exceptional stages need and nothing else:
//!
//! * [`FlightTelemetry`] — the one interface HUD, AI and probes read
//!   (non-negotiable behavior 5: "A shared `FlightTelemetry` interface keeps
//!   HUD, AI and probes model-agnostic"). [`TelemetryFrame`] is its only
//!   implementation so far, and it is built from a production
//!   [`FlightOutput`], so the shared channel is the same numbers for every
//!   model kind and a consumer never has to know which law produced them.
//! * [`RotorDrive`] and [`RotorSpeedMapping`] — the rotor's physics state plus
//!   the **explicit** mapping from the authoritative physical rate to the rate
//!   a rotor mesh is drawn at (non-negotiable behavior 3: "Physical and
//!   visual rotor speeds may differ but require an explicit mapping. Rotor
//!   animation cannot drive physics dt"). Two separate mechanisms keep that
//!   true rather than merely asserted: the physical rate moves only inside
//!   [`RotorDrive::advance_tick`], which is the one `&mut self` mutator, and
//!   it refuses a tick that is not strictly newer than the last one, so a
//!   render frame that tried to drive it would be rejected instead of
//!   silently integrating a frame-rate-dependent rate. The visual side reads
//!   the drive through `&self` and, with no declared mapping, reports
//!   `None` rather than assuming a 1:1 rate.
//! * [`ReferenceManeuverEnvelope`] — the "separate reference maneuver
//!   envelope" the contract requires, with the maneuver kinds an exceptional
//!   airframe must be recorded on and an [`EnvelopeStatus`] that is
//!   [`EnvelopeStatus::Unmeasured`] until an original trace exists. Readiness
//!   also requires an installation origin and an observed provenance, so a
//!   fixture cannot be promoted into an approved reference trace by setting a
//!   status.
//!
//! **F25-B is the control law itself.** [`ExceptionalControlLaw`] evaluates one
//! tick of an exceptional airframe: the shared flight boundary (wing, engine,
//! world-space gravity, declared assist) plus a rotor that contributes lift
//! along its shaft axis, drag against the air-relative velocity, a
//! torque-reaction yaw and an exact gyroscopic precession torque, and an
//! attitude law whose authority comes from the rotor as well as from airspeed.
//! [`ExceptionalProfile`] is its provenance-carrying, per-airframe record, and
//! [`HoverCapability`] makes "may this law hold the airframe's weight with no
//! forward airspeed" a declared field rather than an inference from the model
//! kind. [`ExceptionalDiagnostics`] records every contribution separately, so a
//! probe can assert the total is their sum.
//!
//! **No helicopter hover is invented here, and no original number is claimed.**
//! Non-negotiable behavior 1 and `FLIGHT-PHYSICS` ("Do not use the word autogyro
//! as permission to invent helicopter hover") are enforced structurally: the
//! rotor's drive takes an air-relative speed and nothing else, so no throttle
//! setting reaches it, and a profile that claims hover is refused by name. The
//! exceptional law's numbers are authored project design on
//! [`Origin::SyntheticFixture`]; the Hoplite name and prefix are source-observed
//! while the exact control law remains measurement-dependent (`F25` "Research
//! boundary"), which is why the declared envelope ships unmeasured,
//! [`ReferenceManeuverEnvelope::is_ready_as_reference`] is `false`, and
//! [`ExceptionalProfile::is_measured`] is `false`.

use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Origin, Provenance};
use cs_types::evidence::ClaimStatus;

use super::model::{
    FlightDiagnostics, FlightEnvironment, FlightError, FlightInput, FlightModel, FlightOutput,
    FlightState,
};
use super::tuning::{AIRSPEED_EPSILON_MPS, AirframeTuning, DamageState, LoadoutMass, ModelKind};

/// Two full turns of a rotor, in radians; the visual phase is wrapped into
/// `[0, TAU)` so a long session cannot accumulate phase into a lossy float.
const TAU: f64 = std::f64::consts::TAU;

/// Why a telemetry or rotor value was refused.
///
/// Every refusal names the offending field or tick, and no value is repaired
/// into a plausible one (`FLIGHT-PHYSICS`: "Reject nonfinite inputs at
/// boundaries ... do not silently clamp corrupted tuning into plausible
/// values").
#[derive(Clone, Debug, PartialEq)]
pub enum TelemetryError {
    /// A named field contained NaN or infinity.
    NonFinite {
        /// The offending field.
        field: &'static str,
    },
    /// A field that must be strictly positive was zero or negative.
    NonPositive {
        /// The offending field.
        field: &'static str,
    },
    /// A field that must not be negative was negative.
    Negative {
        /// The offending field.
        field: &'static str,
    },
    /// A fraction fell outside `[0, 1]`.
    OutOfRange {
        /// The offending field.
        field: &'static str,
        /// The rejected value.
        value: f64,
    },
    /// The tick is not strictly newer than the tick this drive last advanced
    /// at, so the rotor would be integrated twice in one simulation tick.
    NonMonotonicTick {
        /// The tick the drive last advanced at.
        last: Tick,
        /// The refused tick.
        got: Tick,
    },
    /// A fixed-wing frame carried a rotor channel, or an exceptional frame did
    /// not: the declared kind and the channels must agree.
    ModelKindMismatch {
        /// The declared model kind.
        declared: ModelKind,
        /// Whether a rotor channel was present.
        has_rotor: bool,
    },
}

impl std::fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::NonPositive { field } => write!(f, "{field} must be greater than zero"),
            Self::Negative { field } => write!(f, "{field} must not be negative"),
            Self::OutOfRange { field, value } => {
                write!(f, "{field} value {value} is outside [0, 1]")
            }
            Self::NonMonotonicTick { last, got } => write!(
                f,
                "the rotor drive advanced at tick {} and cannot advance again at tick {}",
                last.0, got.0
            ),
            Self::ModelKindMismatch {
                declared,
                has_rotor,
            } => write!(
                f,
                "a {} frame {} a rotor channel",
                declared.label(),
                if *has_rotor {
                    "carries"
                } else {
                    "does not carry"
                }
            ),
        }
    }
}

impl std::error::Error for TelemetryError {}

fn check_finite(field: &'static str, value: f64) -> Result<(), TelemetryError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(TelemetryError::NonFinite { field })
    }
}

fn check_positive(field: &'static str, value: f64) -> Result<(), TelemetryError> {
    check_finite(field, value)?;
    if value > 0.0 {
        Ok(())
    } else {
        Err(TelemetryError::NonPositive { field })
    }
}

fn check_fraction(field: &'static str, value: f64) -> Result<(), TelemetryError> {
    check_finite(field, value)?;
    if (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(TelemetryError::OutOfRange { field, value })
    }
}

/// The model-agnostic readouts every consumer may rely on.
///
/// These are the numbers the flight equations already produce
/// ([`super::model::InstrumentState`] and
/// [`super::model::FlightDiagnostics`]), copied without interpretation, so a
/// HUD, an AI controller and a calibration probe read the same values for a
/// fixed wing and for an exceptional model. A model-specific channel is never
/// smuggled in here; it is a separate optional accessor on
/// [`FlightTelemetry::rotor`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SharedTelemetry {
    /// The simulation tick this sample describes.
    pub tick: Tick,
    /// True airspeed, in m/s.
    pub airspeed_mps: f64,
    /// World-vertical speed, in m/s (positive is up).
    pub vertical_speed_mps: f64,
    /// Angle of attack, in radians.
    pub angle_of_attack_rad: f64,
    /// Sideslip angle, in radians.
    pub sideslip_rad: f64,
    /// The stall scale the model actually applied, in `[0, 1]`.
    pub stall_scale: f64,
    /// Thrust commanded this tick, in newtons.
    pub thrust_n: f64,
    /// The fraction of control authority in force this tick, in `[0, 1]`.
    pub control_authority: f64,
    /// Whether the engine is producing thrust at all.
    pub engine_running: bool,
    /// Boost capacity accepted this tick, in capacity units.
    pub boost_consumption: f64,
}

impl SharedTelemetry {
    /// Copies the shared readouts out of one tick's production output.
    ///
    /// # Errors
    ///
    /// [`TelemetryError`] for a non-finite reading, a stall scale or control
    /// authority outside `[0, 1]`, or a negative boost consumption.
    pub fn sample(
        tick: Tick,
        state: &FlightState,
        output: &FlightOutput,
    ) -> Result<Self, TelemetryError> {
        let instruments = &output.instrument_state;
        let sample = Self {
            tick,
            airspeed_mps: instruments.airspeed_mps,
            vertical_speed_mps: state.linear_velocity_mps[1],
            angle_of_attack_rad: instruments.angle_of_attack_rad,
            sideslip_rad: instruments.sideslip_rad,
            stall_scale: instruments.stall_scale,
            thrust_n: instruments.thrust_n,
            control_authority: output.diagnostics.control_authority,
            engine_running: state.engine.running,
            boost_consumption: output.accepted_boost_consumption,
        };
        sample.validate()?;
        Ok(sample)
    }

    /// Checks the sample's declared bounds.
    ///
    /// # Errors
    ///
    /// [`TelemetryError`] naming the first offending field.
    pub fn validate(&self) -> Result<(), TelemetryError> {
        for (field, value) in [
            ("telemetry.airspeed_mps", self.airspeed_mps),
            ("telemetry.vertical_speed_mps", self.vertical_speed_mps),
            ("telemetry.angle_of_attack_rad", self.angle_of_attack_rad),
            ("telemetry.sideslip_rad", self.sideslip_rad),
            ("telemetry.thrust_n", self.thrust_n),
        ] {
            check_finite(field, value)?;
        }
        check_fraction("telemetry.stall_scale", self.stall_scale)?;
        check_fraction("telemetry.control_authority", self.control_authority)?;
        check_finite("telemetry.boost_consumption", self.boost_consumption)?;
        if self.boost_consumption < 0.0 {
            return Err(TelemetryError::Negative {
                field: "telemetry.boost_consumption",
            });
        }
        Ok(())
    }
}

/// The explicit mapping from an authoritative physical rotor rate to the rate a
/// rotor mesh is drawn at (non-negotiable behavior 3).
///
/// The ratio is a declared constant, never an implicit `1.0`: a caller that has
/// no mapping holds `Option<&RotorSpeedMapping>` and gets no visual rate at
/// all, which is what keeps "physical and visual rotor speeds may differ"
/// honest instead of accidental.
#[derive(Clone, Debug, PartialEq)]
pub struct RotorSpeedMapping {
    visual_radps_per_physical_radps: f64,
    origin: Origin,
}

impl RotorSpeedMapping {
    /// Declares the mapping, refusing a non-finite or non-positive ratio.
    ///
    /// A ratio of `0.0` would draw a motionless rotor over a turning one and a
    /// negative one would reverse it, so both are refused rather than
    /// accepted as a curiosity.
    ///
    /// # Errors
    ///
    /// [`TelemetryError::NonFinite`] or
    /// [`TelemetryError::NonPositive`] naming
    /// `rotor.visual_radps_per_physical_radps`.
    pub fn new(
        visual_radps_per_physical_radps: f64,
        origin: Origin,
    ) -> Result<Self, TelemetryError> {
        check_positive(
            "rotor.visual_radps_per_physical_radps",
            visual_radps_per_physical_radps,
        )?;
        Ok(Self {
            visual_radps_per_physical_radps,
            origin,
        })
    }

    /// Visual radians per physical radian.
    #[must_use]
    pub const fn visual_radps_per_physical_radps(&self) -> f64 {
        self.visual_radps_per_physical_radps
    }

    /// Where the ratio came from; never an extracted original constant unless
    /// the origin says so.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The visual rate a physical rate maps to, in rad/s.
    ///
    /// # Errors
    ///
    /// [`TelemetryError::NonFinite`] for a non-finite physical rate.
    pub fn visual_speed_radps(&self, physical_speed_radps: f64) -> Result<f64, TelemetryError> {
        check_finite("rotor.physical_speed_radps", physical_speed_radps)?;
        Ok(physical_speed_radps * self.visual_radps_per_physical_radps)
    }
}

/// A visual rotor rate together with where its mapping came from.
#[derive(Clone, Debug, PartialEq)]
pub struct MappedVisualRate {
    /// The rate the rotor mesh is drawn at, in rad/s.
    pub speed_radps: f64,
    /// The origin of the mapping that produced it.
    pub origin: Origin,
}

/// The exceptional model's own telemetry channel.
///
/// `visual` is `None` when no mapping is declared, and
/// [`RotorTelemetry::visual_speed_radps`] then answers `None` too: a rotor is
/// never assumed to spin at its physical rate.
#[derive(Clone, Debug, PartialEq)]
pub struct RotorTelemetry {
    /// The authoritative physical rate, in rad/s. This is the only rate a
    /// simulation tick integrates.
    pub physical_speed_radps: f64,
    /// The mapped visual rate, when a mapping is declared.
    pub visual: Option<MappedVisualRate>,
}

impl RotorTelemetry {
    /// The visual rate, or `None` when no explicit mapping produced one.
    #[must_use]
    pub fn visual_speed_radps(&self) -> Option<f64> {
        self.visual.as_ref().map(|rate| rate.speed_radps)
    }

    /// Checks the channel's declared bounds.
    ///
    /// # Errors
    ///
    /// [`TelemetryError`] for a non-finite rate.
    pub fn validate(&self) -> Result<(), TelemetryError> {
        check_finite("rotor.physical_speed_radps", self.physical_speed_radps)?;
        if let Some(ref rate) = self.visual {
            check_finite("rotor.visual_speed_radps", rate.speed_radps)?;
        }
        Ok(())
    }
}

/// The authoritative rotor state a simulation tick advances.
///
/// The physical rate is private and [`RotorDrive::advance_tick`] is the only
/// way to move it. That is the mechanism behind "rotor animation cannot drive
/// physics dt": a renderer reads the drive through `&self`, and a caller that
/// tries to advance it twice in one tick, or with a stale tick, is refused by
/// [`TelemetryError::NonMonotonicTick`] instead of integrating a
/// frame-rate-dependent rate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RotorDrive {
    physical_speed_radps: f64,
    last_tick: Option<Tick>,
}

impl RotorDrive {
    /// A rotor at rest, never advanced.
    #[must_use]
    pub const fn stopped() -> Self {
        Self {
            physical_speed_radps: 0.0,
            last_tick: None,
        }
    }

    /// The authoritative physical rate, in rad/s.
    #[must_use]
    pub const fn physical_speed_radps(&self) -> f64 {
        self.physical_speed_radps
    }

    /// The tick this drive last advanced at, if any.
    #[must_use]
    pub const fn last_tick(&self) -> Option<Tick> {
        self.last_tick
    }

    /// Advances the physical rate toward `commanded_radps` for one fixed
    /// simulation tick.
    ///
    /// `tick_dt_s` is the **fixed simulation timestep**, never a render frame
    /// time: the spool moves by `response_per_s · dt`, exactly like
    /// [`super::model::EngineState::advance`], so the rotor's physics is
    /// frame-rate independent. `tick` must be strictly newer than
    /// [`RotorDrive::last_tick`], which is what refuses a second advance inside
    /// one tick.
    ///
    /// # Errors
    ///
    /// [`TelemetryError::NonFinite`] or
    /// [`TelemetryError::NonPositive`] for a rejected `commanded_radps`,
    /// `response_per_s` or `tick_dt_s`, and
    /// [`TelemetryError::NonMonotonicTick`] when `tick` is not strictly newer
    /// than the last one.
    pub fn advance_tick(
        &mut self,
        commanded_radps: f64,
        response_per_s: f64,
        tick: Tick,
        tick_dt_s: f64,
    ) -> Result<(), TelemetryError> {
        check_finite("rotor.commanded_radps", commanded_radps)?;
        check_positive("rotor.response_per_s", response_per_s)?;
        check_positive("rotor.tick_dt_s", tick_dt_s)?;
        if let Some(last) = self.last_tick
            && tick <= last
        {
            return Err(TelemetryError::NonMonotonicTick { last, got: tick });
        }

        let step = response_per_s * tick_dt_s;
        let delta = commanded_radps - self.physical_speed_radps;
        if delta.abs() <= step {
            self.physical_speed_radps = commanded_radps;
        } else {
            self.physical_speed_radps += step * delta.signum();
        }
        self.last_tick = Some(tick);
        Ok(())
    }

    /// The exceptional telemetry channel for this drive.
    ///
    /// With `mapping == None` the channel reports no visual rate at all: the
    /// missing mapping stays visible instead of becoming an implicit `1.0`.
    #[must_use]
    pub fn telemetry(&self, mapping: Option<&RotorSpeedMapping>) -> RotorTelemetry {
        let visual = mapping.and_then(|mapping| {
            mapping
                .visual_speed_radps(self.physical_speed_radps)
                .ok()
                .map(|speed_radps| MappedVisualRate {
                    speed_radps,
                    origin: mapping.origin().clone(),
                })
        });
        RotorTelemetry {
            physical_speed_radps: self.physical_speed_radps,
            visual,
        }
    }

    /// Advances a *visual* phase from the authoritative rate.
    ///
    /// This is a pure function of `self`, `previous` and the render frame time:
    /// it cannot move the physical rate, and the physical rate it reports back
    /// is unchanged however many times it is called. `render_dt_s` therefore
    /// reaches the picture only, never the simulation.
    ///
    /// # Errors
    ///
    /// [`TelemetryError::NonFinite`] for a non-finite `previous.phase_rad` or
    /// `render_dt_s`, [`TelemetryError::NonPositive`] for a non-positive
    /// `render_dt_s`, and [`TelemetryError::NonFinite`] naming the drawn field
    /// when the finite inputs still produce a non-finite rate or phase (an
    /// absurd physical rate times an absurd frame time overflows). The sample
    /// is checked before it is handed out, so no consumer receives a
    /// non-finite drawn phase as a plausible number.
    pub fn visual_sample(
        &self,
        mapping: Option<&RotorSpeedMapping>,
        previous: RotorVisualSample,
        render_dt_s: f64,
    ) -> Result<RotorVisualSample, TelemetryError> {
        check_finite("rotor_visual.phase_rad", previous.phase_rad)?;
        check_positive("rotor_visual.render_dt_s", render_dt_s)?;
        let visual = mapping.and_then(|mapping| {
            mapping
                .visual_speed_radps(self.physical_speed_radps)
                .ok()
                .map(|speed_radps| MappedVisualRate {
                    speed_radps,
                    origin: mapping.origin().clone(),
                })
        });
        let phase = match visual {
            Some(ref rate) => (previous.phase_rad + rate.speed_radps * render_dt_s).rem_euclid(TAU),
            None => previous.phase_rad,
        };
        let sample = RotorVisualSample {
            phase_rad: phase,
            physical_speed_radps: self.physical_speed_radps,
            visual,
        };
        sample.validate()?;
        Ok(sample)
    }
}

/// One drawn rotor's phase and the rates behind it.
#[derive(Clone, Debug, PartialEq)]
pub struct RotorVisualSample {
    /// The drawn phase, in radians, wrapped into `[0, 2π)`.
    pub phase_rad: f64,
    /// The authoritative physical rate, in rad/s, unchanged by sampling.
    pub physical_speed_radps: f64,
    /// The mapped visual rate, when a mapping is declared.
    pub visual: Option<MappedVisualRate>,
}

impl RotorVisualSample {
    /// A stationary rotor with no phase.
    #[must_use]
    pub const fn at_rest() -> Self {
        Self {
            phase_rad: 0.0,
            physical_speed_radps: 0.0,
            visual: None,
        }
    }

    /// The visual rate, or `None` when no explicit mapping produced one.
    #[must_use]
    pub fn visual_speed_radps(&self) -> Option<f64> {
        self.visual.as_ref().map(|rate| rate.speed_radps)
    }

    /// Checks the sample's declared bounds.
    ///
    /// # Errors
    ///
    /// [`TelemetryError`] for a non-finite phase or rate.
    pub fn validate(&self) -> Result<(), TelemetryError> {
        check_finite("rotor_visual.phase_rad", self.phase_rad)?;
        check_finite(
            "rotor_visual.physical_speed_radps",
            self.physical_speed_radps,
        )?;
        if let Some(ref rate) = self.visual {
            check_finite("rotor_visual.visual_speed_radps", rate.speed_radps)?;
        }
        Ok(())
    }
}

/// The one telemetry interface HUD, AI and probes read.
///
/// A consumer takes `&dyn FlightTelemetry` and never learns which control law
/// produced the numbers: the shared channel is identical for every kind, and
/// a model-specific channel is an optional accessor rather than a downcast
/// (non-negotiable behavior 5).
pub trait FlightTelemetry {
    /// Which control law produced this frame.
    fn model_kind(&self) -> ModelKind;

    /// The simulation tick this frame describes.
    fn tick(&self) -> Tick;

    /// The model-agnostic readouts.
    fn shared(&self) -> &SharedTelemetry;

    /// The exceptional model's rotor channel, when this frame has one.
    fn rotor(&self) -> Option<&RotorTelemetry>;
}

/// One tick of telemetry: the shared channel plus an optional exceptional
/// channel.
///
/// The two constructors enforce that the declared [`ModelKind`] and the
/// present channels agree, so a fixed-wing frame cannot carry a rotor and an
/// exceptional frame cannot silently lose one.
#[derive(Clone, Debug, PartialEq)]
pub struct TelemetryFrame {
    model_kind: ModelKind,
    shared: SharedTelemetry,
    rotor: Option<RotorTelemetry>,
}

impl TelemetryFrame {
    /// A frame for a model with no exceptional channel.
    ///
    /// # Errors
    ///
    /// [`TelemetryError`] from [`SharedTelemetry::sample`], and
    /// [`TelemetryError::ModelKindMismatch`] if `kind` is
    /// [`ModelKind::Exceptional`], because an exceptional frame without a rotor
    /// channel would hide the model's own state from every consumer.
    pub fn standard(
        kind: ModelKind,
        state: &FlightState,
        output: &FlightOutput,
        tick: Tick,
    ) -> Result<Self, TelemetryError> {
        let frame = Self {
            model_kind: kind,
            shared: SharedTelemetry::sample(tick, state, output)?,
            rotor: None,
        };
        frame.validate()?;
        Ok(frame)
    }

    /// A frame for an exceptional model, with its rotor channel mapped through
    /// `mapping`.
    ///
    /// # Errors
    ///
    /// [`TelemetryError`] from [`SharedTelemetry::sample`] or
    /// [`RotorDrive::telemetry`], and
    /// [`TelemetryError::ModelKindMismatch`] if `kind` is not
    /// [`ModelKind::Exceptional`].
    pub fn exceptional(
        kind: ModelKind,
        state: &FlightState,
        output: &FlightOutput,
        tick: Tick,
        rotor: &RotorDrive,
        mapping: Option<&RotorSpeedMapping>,
    ) -> Result<Self, TelemetryError> {
        let channel = rotor.telemetry(mapping);
        channel.validate()?;
        let frame = Self {
            model_kind: kind,
            shared: SharedTelemetry::sample(tick, state, output)?,
            rotor: Some(channel),
        };
        frame.validate()?;
        Ok(frame)
    }

    /// Checks that the declared kind and the present channels agree and that
    /// every value is inside its declared bound.
    ///
    /// # Errors
    ///
    /// [`TelemetryError::ModelKindMismatch`] for an inconsistent frame, or
    /// [`TelemetryError`] naming the first offending value.
    pub fn validate(&self) -> Result<(), TelemetryError> {
        let expects_rotor = self.model_kind == ModelKind::Exceptional;
        if expects_rotor != self.rotor.is_some() {
            return Err(TelemetryError::ModelKindMismatch {
                declared: self.model_kind,
                has_rotor: self.rotor.is_some(),
            });
        }
        self.shared.validate()?;
        if let Some(ref rotor) = self.rotor {
            rotor.validate()?;
        }
        Ok(())
    }
}

impl FlightTelemetry for TelemetryFrame {
    fn model_kind(&self) -> ModelKind {
        self.model_kind
    }

    fn tick(&self) -> Tick {
        self.shared.tick
    }

    fn shared(&self) -> &SharedTelemetry {
        &self.shared
    }

    fn rotor(&self) -> Option<&RotorTelemetry> {
        self.rotor.as_ref()
    }
}

/// One maneuver an exceptional airframe's reference envelope must record.
///
/// The fields are the ones `FLIGHT-PHYSICS` "Calibration acceptance" requires
/// a reference trace to state: initial conditions, input timing, measurement
/// error and a tolerance selected *before* fitting, plus whether the maneuver
/// was held out of the fit.
#[derive(Clone, Debug, PartialEq)]
pub struct ManeuverSpec {
    /// Which maneuver this is.
    pub kind: ManeuverKind,
    /// The declared initial state, in words.
    pub initial_conditions: String,
    /// When the control input is applied, in seconds from the start.
    pub input_timing_s: f64,
    /// The camera/instrument measurement error, in the maneuver's own unit.
    pub measurement_error: f64,
    /// The tolerance selected before the final fit.
    pub tolerance: f64,
    /// Whether this maneuver is held out of the fit.
    pub held_out: bool,
}

/// Which maneuvers an airframe's reference envelope must record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ManeuverKind {
    /// Accelerated level run.
    Acceleration,
    /// Throttled-back coast-down.
    CoastDown,
    /// Sustained turn.
    Turn,
    /// Roll response.
    Roll,
    /// Pitch loop.
    PitchLoop,
    /// Stall and recovery.
    StallRecovery,
    /// Engine-out handling.
    EngineOut,
    /// Handling with damaged control authority.
    DamagedControl,
    /// Low-speed behavior.
    LowSpeedBehaviour,
    /// Yaw behavior.
    YawBehaviour,
    /// Lift behavior.
    LiftBehaviour,
    /// Rotor visual behavior against the physical rate.
    RotorVisual,
}

impl ManeuverKind {
    /// Every declared kind, in a stable order.
    pub const ALL: [Self; 12] = [
        Self::Acceleration,
        Self::CoastDown,
        Self::Turn,
        Self::Roll,
        Self::PitchLoop,
        Self::StallRecovery,
        Self::EngineOut,
        Self::DamagedControl,
        Self::LowSpeedBehaviour,
        Self::YawBehaviour,
        Self::LiftBehaviour,
        Self::RotorVisual,
    ];

    /// The stable label used in reports and persisted records.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Acceleration => "acceleration",
            Self::CoastDown => "coast_down",
            Self::Turn => "turn",
            Self::Roll => "roll",
            Self::PitchLoop => "pitch_loop",
            Self::StallRecovery => "stall_recovery",
            Self::EngineOut => "engine_out",
            Self::DamagedControl => "damaged_control",
            Self::LowSpeedBehaviour => "low_speed",
            Self::YawBehaviour => "yaw",
            Self::LiftBehaviour => "lift",
            Self::RotorVisual => "rotor_visual",
        }
    }

    /// Looks a kind up by its label; `None` for an unknown label.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.label() == label)
    }

    /// Whether an airframe of `kind` must record this maneuver.
    ///
    /// Every kind requires the contract's common calibration set; an
    /// exceptional model additionally requires the four the sheet names for it —
    /// "derive its low-speed, yaw, lift and rotor visual behavior from data and
    /// observation" (non-negotiable behavior 1).
    #[must_use]
    pub const fn required_for(self, kind: ModelKind) -> bool {
        match self {
            Self::Acceleration
            | Self::CoastDown
            | Self::Turn
            | Self::Roll
            | Self::PitchLoop
            | Self::StallRecovery
            | Self::EngineOut
            | Self::DamagedControl => true,
            Self::LowSpeedBehaviour
            | Self::YawBehaviour
            | Self::LiftBehaviour
            | Self::RotorVisual => matches!(kind, ModelKind::Exceptional),
        }
    }
}

/// Whether an original reference trace backs an envelope.
#[derive(Clone, Debug, PartialEq)]
pub enum EnvelopeStatus {
    /// No original reference has been recorded yet; `reason` says why.
    Unmeasured {
        /// Why no reference exists.
        reason: String,
    },
    /// An original reference trace backs this envelope.
    ///
    /// The provenance must be an **observation** — a
    /// [`ClaimStatus::VerifiedOriginal`] or [`ClaimStatus::ObservedTool`] claim
    /// — because a designed, inferred or contradicted claim is not a
    /// measurement. [`ReferenceManeuverEnvelope::validate`] refuses the others
    /// by name.
    Measured {
        /// The claim the reference backs. A `verified_original` provenance must
        /// name the source span it observed, so this variant cannot assert an
        /// unlocated original measurement.
        provenance: Provenance,
    },
}

impl EnvelopeStatus {
    /// Whether an original reference backs this envelope.
    #[must_use]
    pub const fn is_measured(&self) -> bool {
        matches!(self, Self::Measured { .. })
    }
}

/// Why a reference maneuver envelope was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum EnvelopeError {
    /// The airframe id was empty or only whitespace.
    EmptyAirframeId,
    /// The unmeasured status carried no reason.
    BlankUnmeasuredReason,
    /// A maneuver was declared twice.
    DuplicateManeuver {
        /// The kind declared more than once.
        kind: ManeuverKind,
    },
    /// A maneuver the airframe's kind requires is absent.
    MissingRequired {
        /// The required kind.
        kind: ManeuverKind,
    },
    /// A maneuver stated no initial conditions.
    EmptyInitialConditions {
        /// The offending maneuver.
        kind: ManeuverKind,
    },
    /// A maneuver's `input_timing_s` was negative.
    NegativeInputTiming {
        /// The offending maneuver.
        kind: ManeuverKind,
    },
    /// A maneuver's `measurement_error` was negative.
    NegativeMeasurementError {
        /// The offending maneuver.
        kind: ManeuverKind,
    },
    /// A maneuver's `tolerance` was not strictly positive.
    NonPositiveTolerance {
        /// The offending maneuver.
        kind: ManeuverKind,
    },
    /// No maneuver was held out of the fit.
    NoHeldOutManeuver,
    /// A `measured` status was backed by a claim that is not an observation, so
    /// the envelope asserts a reference trace nobody measured.
    MeasuredWithoutObservation {
        /// The claim class the status carried.
        class: ClaimStatus,
    },
}

impl std::fmt::Display for EnvelopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyAirframeId => {
                write!(f, "a reference envelope airframe id must not be empty")
            }
            Self::BlankUnmeasuredReason => {
                write!(f, "an unmeasured envelope must carry a reason")
            }
            Self::DuplicateManeuver { kind } => {
                write!(
                    f,
                    "the maneuver {} was declared more than once",
                    kind.label()
                )
            }
            Self::MissingRequired { kind } => write!(
                f,
                "the reference envelope does not record the required maneuver {}",
                kind.label()
            ),
            Self::EmptyInitialConditions { kind } => write!(
                f,
                "the maneuver {} states no initial conditions",
                kind.label()
            ),
            Self::NegativeInputTiming { kind } => write!(
                f,
                "the maneuver {} has a negative input timing",
                kind.label()
            ),
            Self::NegativeMeasurementError { kind } => write!(
                f,
                "the maneuver {} has a negative measurement error",
                kind.label()
            ),
            Self::NonPositiveTolerance { kind } => write!(
                f,
                "the maneuver {} has a tolerance that is not greater than zero",
                kind.label()
            ),
            Self::NoHeldOutManeuver => write!(
                f,
                "a reference envelope must hold out at least one maneuver from the fit"
            ),
            Self::MeasuredWithoutObservation { class } => write!(
                f,
                "a measured envelope must be backed by an observed or verified original claim, not {class}"
            ),
        }
    }
}

impl std::error::Error for EnvelopeError {}

/// The maneuvers one airframe's reference traces must cover.
///
/// This is the "separate reference maneuver envelope" `FLIGHT-PHYSICS` requires
/// for an exceptional airframe, as a closed vocabulary: an airframe that omits
/// a maneuver its kind requires is refused by name rather than quietly
/// unmeasured.
#[derive(Clone, Debug, PartialEq)]
pub struct ReferenceManeuverEnvelope {
    /// The airframe this envelope belongs to.
    pub airframe_id: String,
    /// Which control law the envelope describes.
    pub model_kind: ModelKind,
    /// Where the envelope's declarations came from.
    pub origin: Origin,
    /// The declared maneuvers.
    pub maneuvers: Vec<ManeuverSpec>,
    /// Whether an original reference backs them yet.
    pub status: EnvelopeStatus,
}

impl ReferenceManeuverEnvelope {
    /// The kinds `model_kind` requires and this envelope does not record, in
    /// [`ManeuverKind::ALL`] order.
    #[must_use]
    pub fn missing_required(&self) -> Vec<ManeuverKind> {
        ManeuverKind::ALL
            .into_iter()
            .filter(|kind| kind.required_for(self.model_kind))
            .filter(|kind| !self.maneuvers.iter().any(|spec| spec.kind == *kind))
            .collect()
    }

    /// Whether an original reference trace backs every required maneuver, so
    /// the envelope may be compared against as a reference.
    ///
    /// A synthetic fixture's envelope is deliberately **not** ready: an
    /// [`EnvelopeStatus::Measured`] status with no missing maneuver still needs
    /// an envelope read from the original installation, and a
    /// `SyntheticFixture` or `Designed` origin never is one
    /// (`FLIGHT-PHYSICS`: "A synthetic physics pass cannot promote faithful
    /// handling"). Flipping a fixture's status therefore cannot turn it into an
    /// approved reference trace.
    #[must_use]
    pub fn is_ready_as_reference(&self) -> bool {
        self.status.is_measured() && self.origin.is_original() && self.missing_required().is_empty()
    }

    /// Checks the envelope's identity, coverage and per-maneuver bounds.
    ///
    /// # Errors
    ///
    /// [`EnvelopeError`] naming the first problem: an empty airframe id, a
    /// blank unmeasured reason, a measured status backed by a claim that is not
    /// an observation, a duplicate maneuver, a missing required maneuver, a
    /// maneuver with no initial conditions or an out-of-bound number, or no
    /// held-out maneuver.
    pub fn validate(&self) -> Result<(), EnvelopeError> {
        if self.airframe_id.trim().is_empty() {
            return Err(EnvelopeError::EmptyAirframeId);
        }
        if let EnvelopeStatus::Measured { provenance } = &self.status
            && !matches!(
                provenance.class,
                ClaimStatus::ObservedTool | ClaimStatus::VerifiedOriginal
            )
        {
            return Err(EnvelopeError::MeasuredWithoutObservation {
                class: provenance.class,
            });
        }
        if let EnvelopeStatus::Unmeasured { reason } = &self.status
            && reason.trim().is_empty()
        {
            return Err(EnvelopeError::BlankUnmeasuredReason);
        }

        for (index, spec) in self.maneuvers.iter().enumerate() {
            if self.maneuvers[..index]
                .iter()
                .any(|earlier| earlier.kind == spec.kind)
            {
                return Err(EnvelopeError::DuplicateManeuver { kind: spec.kind });
            }
            if spec.initial_conditions.trim().is_empty() {
                return Err(EnvelopeError::EmptyInitialConditions { kind: spec.kind });
            }
            if !spec.input_timing_s.is_finite() {
                return Err(EnvelopeError::NegativeInputTiming { kind: spec.kind });
            }
            if spec.input_timing_s < 0.0 {
                return Err(EnvelopeError::NegativeInputTiming { kind: spec.kind });
            }
            if !spec.measurement_error.is_finite() || spec.measurement_error < 0.0 {
                return Err(EnvelopeError::NegativeMeasurementError { kind: spec.kind });
            }
            if !spec.tolerance.is_finite() || spec.tolerance <= 0.0 {
                return Err(EnvelopeError::NonPositiveTolerance { kind: spec.kind });
            }
        }

        if let Some(kind) = self.missing_required().first() {
            return Err(EnvelopeError::MissingRequired { kind: *kind });
        }
        if !self.maneuvers.iter().any(|spec| spec.held_out) {
            return Err(EnvelopeError::NoHeldOutManeuver);
        }
        Ok(())
    }
}

/// The synthetic rotor mapping the fixture uses: a drawn rate 1.5× the
/// authoritative physical rate.
///
/// It exists to make "physical and visual rates may differ" testable, and it is
/// [`Origin::SyntheticFixture`] development data, not a measured original
/// ratio. A real airframe's mapping comes from its `cs_content::airframe_roles`
/// record, which may legitimately hold an explicit unknown instead.
#[must_use]
pub fn synthetic_rotor_mapping() -> RotorSpeedMapping {
    RotorSpeedMapping::new(1.5, Origin::SyntheticFixture)
        .expect("the synthetic rotor ratio is positive and finite")
}

/// The synthetic rotor drive the fixture starts from: stopped, never advanced.
#[must_use]
pub fn synthetic_rotor_drive() -> RotorDrive {
    RotorDrive::stopped()
}

/// The fixed simulation timestep the synthetic fixture advances at, in seconds.
pub const SYNTHETIC_TICK_DT_S: f64 = 1.0 / 120.0;

/// The synthetic exceptional envelope: every maneuver the contract requires,
/// with a held-out one, and **no** original reference.
///
/// The maneuver set is project design (which maneuvers must be recorded), not a
/// measured autogyro reference. The status is
/// [`EnvelopeStatus::Unmeasured`] because no original capture exists, so
/// [`ReferenceManeuverEnvelope::is_ready_as_reference`] is `false` and the
/// fixture cannot be mistaken for an approved reference trace.
#[must_use]
pub fn synthetic_exceptional_envelope() -> ReferenceManeuverEnvelope {
    let spec = |kind: ManeuverKind, initial_conditions: &str, held_out: bool| ManeuverSpec {
        kind,
        initial_conditions: initial_conditions.to_owned(),
        input_timing_s: 0.0,
        measurement_error: 0.05,
        tolerance: 0.1,
        held_out,
    };
    ReferenceManeuverEnvelope {
        airframe_id: "fixture.synthetic-exceptional".to_owned(),
        model_kind: ModelKind::Exceptional,
        origin: Origin::SyntheticFixture,
        maneuvers: vec![
            spec(
                ManeuverKind::Acceleration,
                "level, sea level, full throttle from rest",
                false,
            ),
            spec(
                ManeuverKind::CoastDown,
                "level cruise, throttle to idle",
                false,
            ),
            spec(ManeuverKind::Turn, "level, 30 degree bank, held", false),
            spec(ManeuverKind::Roll, "level cruise, full roll input", false),
            spec(
                ManeuverKind::PitchLoop,
                "level cruise, full nose-up then recovery",
                false,
            ),
            spec(
                ManeuverKind::StallRecovery,
                "slow flight to stall and recovery",
                false,
            ),
            spec(
                ManeuverKind::EngineOut,
                "level cruise, engine stopped",
                false,
            ),
            spec(
                ManeuverKind::DamagedControl,
                "level cruise, half control authority",
                false,
            ),
            spec(
                ManeuverKind::LowSpeedBehaviour,
                "hover-adjacent low speed, declared start",
                false,
            ),
            spec(
                ManeuverKind::YawBehaviour,
                "low speed, full yaw input, held",
                false,
            ),
            spec(
                ManeuverKind::LiftBehaviour,
                "slow climb and descent through zero airspeed",
                false,
            ),
            spec(
                ManeuverKind::RotorVisual,
                "rotor rate ramp against drawn phase",
                true,
            ),
        ],
        status: EnvelopeStatus::Unmeasured {
            reason: "no original capture of an exceptional airframe exists".to_owned(),
        },
    }
}

// ---------------------------------------------------------------------------
// F25-B: the declared exceptional control law.
// ---------------------------------------------------------------------------

/// Whether an exceptional law may hold the airframe's weight with no forward
/// airspeed.
///
/// `FLIGHT-PHYSICS` ("Boost and special models") says: "Do not use the word
/// autogyro as permission to invent helicopter hover." Making the capability a
/// **declared field** rather than something inferred from the model kind is what
/// keeps that checkable: a consumer reads [`ExceptionalProfile::hover`] instead
/// of assuming, and the only value this project declares for a rotor-driven
/// airframe is [`HoverCapability::NoHover`].
///
/// A profile that claims [`HoverCapability::Hover`] is refused by name
/// ([`ProfileError::HoverNotMeasured`]) rather than accepted as a capability
/// nothing has measured: the Hoplite name and prefix are source-observed but its
/// handling is not (`F25` "Research boundary"), so a hover claim has no evidence
/// behind it either way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HoverCapability {
    /// The law cannot support the airframe's weight without forward airspeed.
    NoHover,
    /// The law can support the airframe's weight with no forward airspeed.
    ///
    /// Declared by no profile in this project; see the type's documentation.
    Hover,
}

impl HoverCapability {
    /// Every declared capability, in a stable order.
    pub const ALL: [Self; 2] = [Self::NoHover, Self::Hover];

    /// The stable label used in reports and persisted records.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::NoHover => "no_hover",
            Self::Hover => "hover",
        }
    }

    /// Whether the law may hold the airframe's weight with no forward airspeed.
    #[must_use]
    pub const fn is_hover(self) -> bool {
        matches!(self, Self::Hover)
    }
}

/// The declared per-airframe parameters the exceptional control law needs and a
/// fixed-wing tuning does not have.
///
/// This is the *provenance-carrying* record for the law: every number is
/// authored project design with an [`Origin`] and a [`Provenance`], and
/// [`ExceptionalProfile::is_measured`] is `false` for all of them, because no
/// original capture of an exceptional airframe exists. `FLIGHT-PHYSICS`
/// ("Coordinate convention") allows this: "If original tuning does not map to
/// physical coefficients, fit a clearly documented empirical model instead of
/// pretending extracted values are SI coefficients." The rotor terms below are
/// therefore documented empirical forms, not aerodynamic coefficients read out
/// of the original game; the reference traces that would replace them are
/// F25-D's, and they are exactly the maneuvers
/// [`ReferenceManeuverEnvelope`] already demands.
#[derive(Clone, Debug, PartialEq)]
pub struct ExceptionalProfile {
    /// The airframe this profile flies.
    ///
    /// Gameplay keys on this, never on a filename or a mission id (F25
    /// deliverable).
    pub airframe_id: ContentId,
    /// Whether this law may hold the airframe's weight with no forward
    /// airspeed. See [`HoverCapability`].
    pub hover: HoverCapability,
    /// Rotor tip radius, in metres. Strictly positive.
    pub rotor_radius_m: f64,
    /// How fast the airflow over a free rotor drives its rate, in rad/s per m/s
    /// of air-relative speed. Strictly positive.
    pub rotor_drive_radps_per_mps: f64,
    /// First-order response of the rotor's rate, in `s⁻¹`: the rate moves
    /// toward its command by at most this much per second. Strictly positive.
    pub rotor_response_per_s: f64,
    /// Rotor lift along the shaft axis, in newtons per m/s of rotor tip speed.
    /// Strictly positive.
    pub rotor_lift_n_per_mps_tip: f64,
    /// The most rotor lift the law may produce, in newtons. Strictly positive.
    pub rotor_lift_max_n: f64,
    /// Rotor drag against the air-relative velocity, in newtons per
    /// (m/s of tip speed)·(m/s of air speed). Must not be negative.
    pub rotor_drag_n_per_tip_air: f64,
    /// The rotor's torque-reaction yaw about the shaft axis, in newton-metres
    /// per rad/s of rotor rate. Must not be negative.
    ///
    /// The sign convention is the same as a positive yaw command, so the
    /// declared law turns the airframe toward the reaction. Which way the
    /// original airframe actually yawed is a measured question, not a designed
    /// one, and is recorded as unknown in the F25-B finding.
    pub rotor_yaw_nm_per_radps: f64,
    /// The rotor's polar inertia about the shaft axis, in kg·m², for the
    /// gyroscopic precession term. Strictly positive.
    pub rotor_polar_inertia_kg_m2: f64,
    /// Rotor tip speed below which the rotor contributes no control authority,
    /// in m/s. Must not be negative.
    pub control_tip_speed_zero_mps: f64,
    /// Rotor tip speed at which the rotor's own control authority reaches its
    /// full value, in m/s. Strictly greater than
    /// [`Self::control_tip_speed_zero_mps`].
    pub control_tip_speed_full_mps: f64,
    /// Where the declared numbers came from.
    pub origin: Origin,
    /// The claim the declaration backs.
    pub provenance: Provenance,
}

impl ExceptionalProfile {
    /// Checks the airframe identity, the hover declaration, the origin and
    /// every numeric bound, in that order.
    ///
    /// # Errors
    ///
    /// [`ProfileError`] naming the first problem: an id that is not an
    /// airframe, a hover claim nothing measured, an installation origin whose
    /// claim is not an observation, a non-finite or out-of-bound number, or a
    /// control band whose full value is not above its zero value.
    pub fn validate(&self) -> Result<(), ProfileError> {
        if self.airframe_id.kind() != ContentKind::Airframe {
            return Err(ProfileError::NotAnAirframe {
                kind: self.airframe_id.kind(),
            });
        }
        if self.hover.is_hover() {
            return Err(ProfileError::HoverNotMeasured {
                declared: self.hover,
            });
        }
        if self.origin.is_original()
            && !matches!(
                self.provenance.class,
                ClaimStatus::ObservedTool | ClaimStatus::VerifiedOriginal
            )
        {
            return Err(ProfileError::OriginalOriginWithoutObservation {
                class: self.provenance.class,
            });
        }
        profile_positive("profile.rotor_radius_m", self.rotor_radius_m)?;
        profile_positive(
            "profile.rotor_drive_radps_per_mps",
            self.rotor_drive_radps_per_mps,
        )?;
        profile_positive("profile.rotor_response_per_s", self.rotor_response_per_s)?;
        profile_positive(
            "profile.rotor_lift_n_per_mps_tip",
            self.rotor_lift_n_per_mps_tip,
        )?;
        profile_positive("profile.rotor_lift_max_n", self.rotor_lift_max_n)?;
        profile_non_negative(
            "profile.rotor_drag_n_per_tip_air",
            self.rotor_drag_n_per_tip_air,
        )?;
        profile_non_negative(
            "profile.rotor_yaw_nm_per_radps",
            self.rotor_yaw_nm_per_radps,
        )?;
        profile_positive(
            "profile.rotor_polar_inertia_kg_m2",
            self.rotor_polar_inertia_kg_m2,
        )?;
        profile_non_negative(
            "profile.control_tip_speed_zero_mps",
            self.control_tip_speed_zero_mps,
        )?;
        profile_positive(
            "profile.control_tip_speed_full_mps",
            self.control_tip_speed_full_mps,
        )?;
        if self.control_tip_speed_full_mps <= self.control_tip_speed_zero_mps {
            return Err(ProfileError::InvertedControlBand {
                zero_mps: self.control_tip_speed_zero_mps,
                full_mps: self.control_tip_speed_full_mps,
            });
        }
        Ok(())
    }

    /// Whether an original measurement backs these numbers.
    ///
    /// It is `false` for every profile this project declares, and a `true` here
    /// still does not make the airframe reference-calibrated: that needs the
    /// [`ReferenceManeuverEnvelope`] an original trace fills in.
    #[must_use]
    pub fn is_measured(&self) -> bool {
        self.origin.is_original()
            && matches!(
                self.provenance.class,
                ClaimStatus::ObservedTool | ClaimStatus::VerifiedOriginal
            )
    }
}

/// Why an [`ExceptionalProfile`] was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum ProfileError {
    /// The id does not name an airframe.
    NotAnAirframe {
        /// The kind the id actually names.
        kind: ContentKind,
    },
    /// The profile claims the law can hold the airframe's weight with no
    /// forward airspeed, which nothing has measured.
    HoverNotMeasured {
        /// The capability the profile claimed.
        declared: HoverCapability,
    },
    /// A named field contained NaN or infinity.
    NonFinite {
        /// The offending field.
        field: &'static str,
    },
    /// A field that must be strictly positive was zero or negative.
    NonPositive {
        /// The offending field.
        field: &'static str,
    },
    /// A field that must not be negative was negative.
    Negative {
        /// The offending field.
        field: &'static str,
    },
    /// The rotor's control band is empty or inverted: its full-authority tip
    /// speed is not above its zero-authority tip speed.
    InvertedControlBand {
        /// The declared zero-authority tip speed.
        zero_mps: f64,
        /// The declared full-authority tip speed.
        full_mps: f64,
    },
    /// The profile claims an installation origin while its claim is not an
    /// observation, so it asserts an original measurement its own provenance
    /// denies.
    OriginalOriginWithoutObservation {
        /// The claim class the provenance carried.
        class: ClaimStatus,
    },
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnAirframe { kind } => {
                write!(
                    f,
                    "an exceptional profile must reference an airframe, got {kind}"
                )
            }
            Self::HoverNotMeasured { declared } => write!(
                f,
                "the {} capability is declared by no measurement and this project refuses it",
                declared.label()
            ),
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::NonPositive { field } => write!(f, "{field} must be greater than zero"),
            Self::Negative { field } => write!(f, "{field} must not be negative"),
            Self::InvertedControlBand { zero_mps, full_mps } => write!(
                f,
                "the rotor control band is empty: {full_mps} m/s of tip speed does not reach full authority above {zero_mps} m/s"
            ),
            Self::OriginalOriginWithoutObservation { class } => write!(
                f,
                "a profile with an installation origin must carry an observed or verified original claim, not {class}"
            ),
        }
    }
}

impl std::error::Error for ProfileError {}

/// Why an [`ExceptionalControlLaw`] tick was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum ExceptionalLawError {
    /// The tuning does not declare the exceptional control law, so this law
    /// must not evaluate it.
    NotAnExceptionalAirframe {
        /// The model kind the tuning declared.
        declared: ModelKind,
    },
    /// The profile failed its own boundary.
    Profile(ProfileError),
    /// The shared flight boundary refused one of its inputs.
    Flight(FlightError),
    /// The rotor drive refused the tick, so no rotor state was advanced.
    Rotor(TelemetryError),
    /// The tick this law produced held a non-finite value, which is refused by
    /// name rather than handed on.
    NonFiniteOutput {
        /// The offending field.
        field: &'static str,
    },
    /// The produced world force is not the sum of the contributions this law
    /// recorded, so something in the law produced an unaccounted force.
    UnaccountedForce {
        /// The largest component difference, in newtons.
        residual_n: f64,
    },
}

impl std::fmt::Display for ExceptionalLawError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnExceptionalAirframe { declared } => write!(
                f,
                "the exceptional control law does not fly a {} airframe",
                declared.label()
            ),
            Self::Profile(error) => write!(f, "{error}"),
            Self::Flight(error) => write!(f, "{error}"),
            Self::Rotor(error) => write!(f, "{error}"),
            Self::NonFiniteOutput { field } => {
                write!(f, "the computed exceptional tick's {field} is not finite")
            }
            Self::UnaccountedForce { residual_n } => write!(
                f,
                "the exceptional force is not the sum of its contributions; the largest component differs by {residual_n} N"
            ),
        }
    }
}

impl std::error::Error for ExceptionalLawError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Profile(error) => Some(error),
            Self::Flight(error) => Some(error),
            Self::Rotor(error) => Some(error),
            Self::NotAnExceptionalAirframe { .. }
            | Self::NonFiniteOutput { .. }
            | Self::UnaccountedForce { .. } => None,
        }
    }
}

impl From<ProfileError> for ExceptionalLawError {
    fn from(error: ProfileError) -> Self {
        Self::Profile(error)
    }
}

impl From<FlightError> for ExceptionalLawError {
    fn from(error: FlightError) -> Self {
        Self::Flight(error)
    }
}

impl From<TelemetryError> for ExceptionalLawError {
    fn from(error: TelemetryError) -> Self {
        Self::Rotor(error)
    }
}

/// The control-axis order the tuning's `angular` block uses, restated here.
///
/// Each entry is the body component its axis turns and the sign of a positive
/// command, in tuning order `(roll, pitch, yaw)`. It is identical to the
/// fixed-wing law's private `CONTROL_AXIS`, which this module cannot reach, so
/// the two laws agree on the sign convention by restatement plus an
/// `accept_f25_b_*` test that compares them at an airspeed where both reach full
/// authority.
const EXCEPTIONAL_CONTROL_AXIS: [(usize, f64); 3] = [(2, -1.0), (0, 1.0), (1, 1.0)];

/// The exceptional control law: the shared flight boundary plus a rotor.
///
/// # What it is
///
/// `FLIGHT-PHYSICS` ("Boost and special models") requires that "Exceptional
/// airframes implement the same input/output boundary but can use a different
/// control law", and this is that law. It is deliberately assembled from the
/// fixed-wing equations plus a declared rotor rather than as a second,
/// parallel set of aerodynamic formulas, because the contract also forbids
/// integrating a body twice:
///
/// * [`super::model::FlightModel::compute`] produces the **shared boundary**:
///   wing lift, parasitic drag, engine/boost thrust, world-space gravity and the
///   declared assist, all validated at the edge. This law never adds a second
///   gravity, a second drag or a second integrator.
/// * The **rotor** is the exceptional part: its authoritative rate is
///   [`RotorDrive`]'s, advanced exactly once per strictly newer tick, and it
///   contributes lift along the shaft axis, drag against the air-relative
///   velocity, a torque-reaction yaw and an exact gyroscopic precession torque.
/// * The **attitude law** is the fixed wing's rate command with a different
///   authority source: authority is the larger of the wing's airspeed ramp and
///   the rotor's own support, so a spinning rotor answers the stick at airspeeds
///   where a wing cannot. Nothing else about the rate command changes, so the
///   two laws are directly comparable.
///
/// # What it is not
///
/// It is **not** a fixed wing with a spinning mesh and **not** a hovering
/// helicopter, and both are structural rather than documentary:
///
/// * [`ExceptionalControlLaw::commanded_rotor_radps`] takes an air-relative
///   speed and nothing else — no [`super::model::FlightInput`], no
///   [`super::model::EngineState`]. The throttle has no path to the rotor, so no
///   engine setting can create lift at zero airspeed, and a rotor at rest stays
///   at rest however hard the throttle is pushed.
/// * A pre-spun rotor with no airspeed decays toward zero at the profile's
///   response rate, so even a spinning rotor cannot hold the airframe up: it
///   has no thrust to replace the energy the rotor gives back.
///
/// **Designed, not measured.** Every value in the profile is authored project
/// design with an [`Origin`] and a [`Provenance`], and
/// [`ExceptionalProfile::is_measured`] is `false` for the synthetic fixture. The
/// law's *shape* is this project's engineering design; the original Hoplite's
/// numbers are not recovered (`F25` "Research boundary"), the reference envelope
/// ships [`EnvelopeStatus::Unmeasured`], and only a calibration against an
/// original trace (F25-D) can turn any of it into a fidelity claim.
#[derive(Clone, Debug, PartialEq)]
pub struct ExceptionalControlLaw {
    tuning: AirframeTuning,
    profile: ExceptionalProfile,
    /// The shared flight boundary this law adds rotor terms to.
    wing: FlightModel,
}

impl ExceptionalControlLaw {
    /// Builds the law from an exceptional tuning and a validated profile.
    ///
    /// # Errors
    ///
    /// [`ExceptionalLawError::NotAnExceptionalAirframe`] when the tuning does
    /// not declare [`ModelKind::Exceptional`], or
    /// [`ExceptionalLawError::Profile`] for the first profile field that fails
    /// its boundary.
    pub fn new(
        tuning: AirframeTuning,
        profile: ExceptionalProfile,
    ) -> Result<Self, ExceptionalLawError> {
        if tuning.model_kind != ModelKind::Exceptional {
            return Err(ExceptionalLawError::NotAnExceptionalAirframe {
                declared: tuning.model_kind,
            });
        }
        profile.validate()?;
        Ok(Self {
            wing: FlightModel::new(tuning.clone()),
            tuning,
            profile,
        })
    }

    /// The tuning this law evaluates.
    #[must_use]
    pub const fn tuning(&self) -> &AirframeTuning {
        &self.tuning
    }

    /// The declared profile this law flies.
    #[must_use]
    pub const fn profile(&self) -> &ExceptionalProfile {
        &self.profile
    }

    /// The rotor rate the airflow over the disc commands, in rad/s.
    ///
    /// This is the whole of the rotor's drive, and its **signature is the
    /// anti-hover mechanism**: it takes an air-relative speed and returns a
    /// rate, so neither the throttle nor the engine state can reach it. A rotor
    /// whose drive had any engine coupling would need a different, declared and
    /// measured law here rather than a hidden term.
    ///
    /// The speed is the **total** air-relative magnitude the shared boundary
    /// reports, not its forward component. Whether the original rotor is driven
    /// by forward flight alone, by the total relative airflow, or by a climb
    /// rate as well is a measured question this project cannot answer, so the
    /// choice is recorded as an unknown in the F25-B finding and this signature
    /// is the seam a measured law would replace.
    ///
    /// # Errors
    ///
    /// [`ProfileError::NonFinite`] for a non-finite airspeed and
    /// [`ProfileError::Negative`] for a negative one.
    pub fn commanded_rotor_radps(&self, airspeed_mps: f64) -> Result<f64, ProfileError> {
        profile_finite("rotor.airspeed_mps", airspeed_mps)?;
        if airspeed_mps < 0.0 {
            return Err(ProfileError::Negative {
                field: "rotor.airspeed_mps",
            });
        }
        Ok(self.profile.rotor_drive_radps_per_mps * airspeed_mps)
    }

    /// The rotor's tip speed for a given rate, in m/s.
    ///
    /// # Errors
    ///
    /// [`ProfileError::NonFinite`] for a non-finite rate.
    pub fn rotor_tip_speed_mps(&self, rotor_radps: f64) -> Result<f64, ProfileError> {
        profile_finite("rotor.radps", rotor_radps)?;
        Ok(self.profile.rotor_radius_m * rotor_radps)
    }

    /// The rotor's own contribution to control authority, in `[0, 1]`.
    ///
    /// It ramps from [`ExceptionalProfile::control_tip_speed_zero_mps`] of tip
    /// speed to [`ExceptionalProfile::control_tip_speed_full_mps`], so a rotor
    /// at rest contributes nothing and the ramp is a declared linear band, not
    /// a fitted curve.
    ///
    /// # Errors
    ///
    /// [`ProfileError::NonFinite`] for a non-finite rate.
    pub fn rotor_support(&self, rotor_radps: f64) -> Result<f64, ProfileError> {
        let tip = self.rotor_tip_speed_mps(rotor_radps)?;
        let zero = self.profile.control_tip_speed_zero_mps;
        let full = self.profile.control_tip_speed_full_mps;
        Ok(((tip - zero) / (full - zero)).clamp(0.0, 1.0))
    }

    /// Computes one tick's force, torque and instruments for the exceptional
    /// airframe, advancing `rotor` exactly once.
    ///
    /// `dt_s` is the fixed simulation timestep and `tick` must be strictly newer
    /// than the rotor's last, which is what refuses a second advance in one tick
    /// and stops a render frame from driving the rotor's physics. At zero
    /// airspeed with a rotor at rest every produced value is finite, the rotor
    /// lift and drag are exactly zero, and the only vertical force is gravity.
    ///
    /// `dt_s` must be strictly positive here, where the shared boundary only
    /// requires it to be non-negative: a zero-length tick has no rotor step to
    /// take, and this law says so by name rather than integrating one anyway.
    ///
    /// # Errors
    ///
    /// [`ExceptionalLawError`] for a rejected profile, environment, loadout,
    /// damage, state, input or timestep (through the shared boundary), a rotor
    /// tick the drive refused, or a produced tick this law's own check refused
    /// ([`ExceptionalLawError::NonFiniteOutput`],
    /// [`ExceptionalLawError::UnaccountedForce`]).
    ///
    /// **Every refusal leaves `rotor` exactly where it was.** The advance is
    /// applied to a copy and committed only after the produced tick passes
    /// [`ExceptionalTick::validate`], so a caller may retry a refused tick with
    /// a newer tick without the rotor having been integrated twice.
    // The argument list is the contract's own vocabulary — environment, loadout,
    // damage, state, input, timestep — plus the tick and the rotor the
    // exceptional airframe additionally carries. Grouping them would hide which
    // value came from where at the boundary, so the lint is allowed here rather
    // than the shape changed.
    #[allow(clippy::too_many_arguments)]
    pub fn compute(
        &self,
        environment: &FlightEnvironment,
        loadout: &LoadoutMass,
        damage: &DamageState,
        state: &FlightState,
        input: &FlightInput,
        dt_s: f64,
        tick: Tick,
        rotor: &mut RotorDrive,
    ) -> Result<ExceptionalTick, ExceptionalLawError> {
        self.profile.validate()?;

        // The shared boundary: wing lift, parasitic drag, engine/boost thrust,
        // world-space gravity and the declared assist, validated once.
        let base = self
            .wing
            .compute(environment, loadout, damage, state, input, dt_s)?;

        // The rotor: driven by the airflow, advanced once per strictly newer
        // tick. A refused tick leaves `rotor` untouched.
        let airspeed_mps = base.instrument_state.airspeed_mps;
        let commanded_rotor_radps = self.commanded_rotor_radps(airspeed_mps)?;
        // The advance is applied to a *copy* and committed only once the tick
        // this law produces has passed its own check. Advancing the caller's
        // rotor first would leave it half-integrated whenever the produced-tick
        // check refuses, so a caller that retried the same tick would integrate
        // the rotor twice — the very thing the strictly-newer-tick rule exists
        // to prevent.
        let mut advanced = *rotor;
        advanced.advance_tick(
            commanded_rotor_radps,
            self.profile.rotor_response_per_s,
            tick,
            dt_s,
        )?;
        let rotor_radps = advanced.physical_speed_radps();
        let rotor_tip_speed_mps = self.rotor_tip_speed_mps(rotor_radps)?;

        // Rotor forces. Lift acts along the shaft axis (body up) and is capped
        // by the profile's declared maximum, which bounds the rotor's share of
        // the total but is **not** what keeps the airframe down: whether the cap
        // sits above or below the weight is a property of the tuning's mass, not
        // an invariant of the law. The structural reason a spinning rotor cannot
        // hold the airframe up is the drive above — it has no engine input, and
        // with no airspeed its command is zero, so a pre-spun rotor decays to
        // rest. Drag acts against the air-relative velocity and vanishes with
        // it, which is the term that spends the rotor's energy.
        let rotor_lift_n = (self.profile.rotor_lift_n_per_mps_tip * rotor_tip_speed_mps)
            .clamp(0.0, self.profile.rotor_lift_max_n);
        let rotor_drag_n =
            self.profile.rotor_drag_n_per_tip_air * rotor_tip_speed_mps * airspeed_mps;
        let up_world = rotated(super::model::BODY_UP, state.orientation);
        let air_velocity_world = environment.air_relative_velocity_m_s(state.linear_velocity_mps);
        let air_direction = scaled(
            air_velocity_world,
            1.0 / airspeed_mps.max(AIRSPEED_EPSILON_MPS),
        );
        let rotor_lift_force_n = scaled(up_world, rotor_lift_n);
        let rotor_drag_force_n = scaled(air_direction, -rotor_drag_n);

        // Attitude. The same rate command the fixed wing uses, with an
        // authority that comes from the rotor as well as from airspeed, plus the
        // two torque terms a spinning rotor adds and a wing has not got.
        let wing_authority =
            (airspeed_mps / self.tuning.angular.control_airspeed_full_mps).clamp(0.0, 1.0);
        let rotor_support = self.rotor_support(rotor_radps)?;
        let control_authority = (base.instrument_state.stall_scale
            * damage.control_authority
            * wing_authority.max(rotor_support))
        .clamp(0.0, 1.0);

        let rate_command_torque_nm = self.rate_command_torque(state, input, control_authority);
        // Gyroscopic precession is an identity, not a fitted curve: a symmetric
        // rotor's angular momentum points along the shaft, so the airframe must
        // supply `ω × L` to hold attitude, which couples body pitch rate into
        // roll torque and body roll rate into pitch torque. Only the rotor's
        // polar inertia and rate are declared numbers.
        let precession_arm = self.profile.rotor_polar_inertia_kg_m2 * rotor_radps;
        let rotor_yaw_torque_nm = self.profile.rotor_yaw_nm_per_radps * rotor_radps;
        let precession_torque_nm = [
            -precession_arm * state.angular_velocity_radps[0],
            -precession_arm * state.angular_velocity_radps[2],
            0.0,
        ];
        let max_torque = self.tuning.angular.max_torque_nm;
        let mut control_axis_torque_nm = [0.0; 3];
        for axis in 0..3 {
            let total = rate_command_torque_nm[axis]
                + precession_torque_nm[axis]
                + rotor_yaw_torque_nm * f64::from(axis == 2);
            control_axis_torque_nm[axis] = total.clamp(-max_torque[axis], max_torque[axis]);
        }

        let body_torque_nm = axis_torque_to_body(control_axis_torque_nm);
        let world_torque_nm = added(
            rotated(body_torque_nm, state.orientation),
            base.diagnostics.assist_torque_nm,
        );
        let world_force_n = added(
            added(base.world_force_n, rotor_lift_force_n),
            rotor_drag_force_n,
        );

        let output = FlightOutput {
            world_force_n,
            world_torque_nm,
            instrument_state: base.instrument_state,
            accepted_boost_consumption: base.accepted_boost_consumption,
            diagnostics: FlightDiagnostics {
                // `lift_n` and `drag_n` are what a shared consumer must read, so
                // they are the wing's term **plus** the rotor's: `drag_n` is
                // exact, because the wing's drag and the rotor's both act
                // against the air-relative velocity, while `lift_n` is the sum
                // of two magnitudes (the wing's is perpendicular to the airflow,
                // the rotor's along the shaft) and is therefore a gauge reading
                // rather than the norm of a single vector. The per-source split
                // that a probe needs is in `ExceptionalDiagnostics`.
                lift_n: base.diagnostics.lift_n + rotor_lift_n,
                drag_n: base.diagnostics.drag_n + rotor_drag_n,
                // The authority is this law's, not the wing's: the shared
                // channel would otherwise report an exceptional airframe as
                // though it had no rotor at all.
                control_authority,
                ..base.diagnostics
            },
        };
        let computed = ExceptionalTick {
            output,
            diagnostics: ExceptionalDiagnostics {
                base_force_n: base.world_force_n,
                base: base.diagnostics,
                commanded_rotor_radps,
                rotor_radps,
                rotor_tip_speed_mps,
                rotor_lift_n,
                rotor_lift_force_n,
                rotor_drag_n,
                rotor_drag_force_n,
                rotor_yaw_torque_nm,
                precession_torque_nm,
                rate_command_torque_nm,
                control_axis_torque_nm,
                wing_authority,
                rotor_support,
                control_authority,
            },
            rotor: advanced,
        };
        computed.validate()?;
        // Committed only now: a tick this law refused leaves the caller's rotor
        // exactly as it was.
        *rotor = advanced;
        Ok(computed)
    }

    /// The bounded rate-command torque in control-axis order `(roll, pitch,
    /// yaw)`.
    ///
    /// The same law the fixed wing uses, with the exceptional authority in
    /// place of the wing's: the desired rate is the command times the declared
    /// maximum rate times the authority, and the torque is the inertia-scaled
    /// rate error with the declared damping, bounded per axis.
    fn rate_command_torque(
        &self,
        state: &FlightState,
        input: &FlightInput,
        authority: f64,
    ) -> [f64; 3] {
        let commands = [input.roll, input.pitch, input.yaw];
        let angular = &self.tuning.angular;
        let mut torque = [0.0; 3];
        for axis in 0..3 {
            let &(component, sign) = &EXCEPTIONAL_CONTROL_AXIS[axis];
            let rate = sign * state.angular_velocity_radps[component];
            let desired_rate =
                commands[axis].clamp(-1.0, 1.0) * angular.max_rate_radps[axis] * authority;
            let rate_error = desired_rate - rate;
            let inertia = self.tuning.mass.inertia_kg_m2[axis];
            let raw = inertia
                * (angular.rate_gain_per_s * rate_error - angular.rate_damping_per_s * rate);
            torque[axis] = raw.clamp(-angular.max_torque_nm[axis], angular.max_torque_nm[axis]);
        }
        torque
    }
}

/// The exceptional law's per-source record for one tick.
///
/// Every force is a world-space vector and every rotor term a scalar, so a
/// probe can see exactly what the rotor added to the shared boundary and can
/// assert that the total is their sum ([`Self::total_force_n`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExceptionalDiagnostics {
    /// The shared boundary's world force for this tick, before the rotor terms.
    pub base_force_n: [f64; 3],
    /// The shared boundary's per-source contributions, so the wing's own lift,
    /// drag, thrust, gravity and assist stay visible beside the rotor's.
    pub base: FlightDiagnostics,
    /// The rotor rate the airflow commanded this tick, in rad/s.
    pub commanded_rotor_radps: f64,
    /// The authoritative rotor rate after this tick's single advance, in rad/s.
    pub rotor_radps: f64,
    /// `rotor_radius_m · rotor_radps`, in m/s.
    pub rotor_tip_speed_mps: f64,
    /// Rotor lift along the shaft axis, in newtons.
    pub rotor_lift_n: f64,
    /// Rotor lift as a world-space vector.
    pub rotor_lift_force_n: [f64; 3],
    /// Rotor drag against the air-relative velocity, in newtons.
    pub rotor_drag_n: f64,
    /// Rotor drag as a world-space vector.
    pub rotor_drag_force_n: [f64; 3],
    /// The rotor's torque-reaction yaw, in newton-metres, in the same sign as a
    /// positive yaw command.
    pub rotor_yaw_torque_nm: f64,
    /// The gyroscopic precession torque in control-axis order `(roll, pitch,
    /// yaw)`.
    pub precession_torque_nm: [f64; 3],
    /// The rate-command torque in control-axis order, before the rotor terms.
    pub rate_command_torque_nm: [f64; 3],
    /// The total torque actually applied, in control-axis order, bounded per
    /// axis by the tuning's maximum.
    pub control_axis_torque_nm: [f64; 3],
    /// The wing's airspeed authority ramp, in `[0, 1]`.
    pub wing_authority: f64,
    /// The rotor's own control support, in `[0, 1]`.
    pub rotor_support: f64,
    /// The authority actually applied this tick, in `[0, 1]`.
    pub control_authority: f64,
}

impl ExceptionalDiagnostics {
    /// The world force this law produced, as the sum of its recorded
    /// contributions.
    ///
    /// This must equal the produced [`FlightOutput`]'s `world_force_n`;
    /// [`ExceptionalTick::validate`] checks that, so a term this law added
    /// without recording it is a refusal rather than a silent force.
    #[must_use]
    pub fn total_force_n(&self) -> [f64; 3] {
        added(
            added(self.base_force_n, self.rotor_lift_force_n),
            self.rotor_drag_force_n,
        )
    }

    /// The control law's total torque as a body-space vector.
    ///
    /// This is what this law contributes about the airframe's own axes. The
    /// declared bank/level assist is **not** in it: the assist is computed in
    /// world space by the shared boundary and added to the rotated control
    /// torque when the law produces `FlightOutput::world_torque_nm`, so a
    /// consumer must not read this vector as the whole applied couple.
    #[must_use]
    pub fn body_torque_nm(&self) -> [f64; 3] {
        axis_torque_to_body(self.control_axis_torque_nm)
    }
}

/// One evaluated exceptional tick: the shared output, this law's per-source
/// record, and the rotor state the single advance produced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExceptionalTick {
    /// The shared input/output boundary's result, unchanged in shape from a
    /// fixed-wing tick.
    pub output: FlightOutput,
    /// The exceptional law's per-source record.
    pub diagnostics: ExceptionalDiagnostics,
    /// The authoritative rotor state after this tick's single advance.
    pub rotor: RotorDrive,
}

impl ExceptionalTick {
    /// Checks that the produced tick is finite and that its world force is the
    /// sum of the contributions it recorded.
    ///
    /// # Errors
    ///
    /// [`ExceptionalLawError::NonFiniteOutput`] naming the first non-finite
    /// value, or [`ExceptionalLawError::UnaccountedForce`] with the largest
    /// component difference.
    pub fn validate(&self) -> Result<(), ExceptionalLawError> {
        for (field, value) in [
            ("world_force_n[0]", self.output.world_force_n[0]),
            ("world_force_n[1]", self.output.world_force_n[1]),
            ("world_force_n[2]", self.output.world_force_n[2]),
            ("world_torque_nm[0]", self.output.world_torque_nm[0]),
            ("world_torque_nm[1]", self.output.world_torque_nm[1]),
            ("world_torque_nm[2]", self.output.world_torque_nm[2]),
            ("rotor_lift_n", self.diagnostics.rotor_lift_n),
            ("rotor_drag_n", self.diagnostics.rotor_drag_n),
            ("rotor_yaw_torque_nm", self.diagnostics.rotor_yaw_torque_nm),
            (
                "commanded_rotor_radps",
                self.diagnostics.commanded_rotor_radps,
            ),
            ("rotor_radps", self.diagnostics.rotor_radps),
            ("rotor_tip_speed_mps", self.diagnostics.rotor_tip_speed_mps),
            ("control_authority", self.diagnostics.control_authority),
            ("rotor_support", self.diagnostics.rotor_support),
        ] {
            if !value.is_finite() {
                return Err(ExceptionalLawError::NonFiniteOutput { field });
            }
        }
        let total = self.diagnostics.total_force_n();
        let residual_n = (0..3)
            .map(|axis| (total[axis] - self.output.world_force_n[axis]).abs())
            .fold(0.0_f64, f64::max);
        let scale = total
            .into_iter()
            .map(f64::abs)
            .fold(0.0_f64, f64::max)
            .max(1.0);
        if residual_n > 1.0e-9 * scale {
            return Err(ExceptionalLawError::UnaccountedForce { residual_n });
        }
        Ok(())
    }

    /// The validated telemetry frame for this tick.
    ///
    /// This is how the exceptional law reaches the shared
    /// [`FlightTelemetry`] channel HUD, AI and probes read: the same
    /// [`SharedTelemetry`] numbers a fixed wing reports, plus this airframe's
    /// rotor channel mapped through an explicit
    /// [`RotorSpeedMapping`] (or no visual rate at all when none is declared).
    ///
    /// # Errors
    ///
    /// [`TelemetryError`] from [`TelemetryFrame::exceptional`]: a non-finite
    /// reading, a value outside its declared bound, or a rotor mapping that
    /// produces a non-finite drawn rate.
    pub fn telemetry(
        &self,
        state: &FlightState,
        tick: Tick,
        mapping: Option<&RotorSpeedMapping>,
    ) -> Result<TelemetryFrame, TelemetryError> {
        TelemetryFrame::exceptional(
            self.diagnostics_model_kind(),
            state,
            &self.output,
            tick,
            &self.rotor,
            mapping,
        )
    }

    /// The model kind this law declares. It is always
    /// [`ModelKind::Exceptional`]; the method exists so [`Self::telemetry`]
    /// cannot pass a fixed-wing kind by accident.
    const fn diagnostics_model_kind(&self) -> ModelKind {
        ModelKind::Exceptional
    }
}

fn profile_finite(field: &'static str, value: f64) -> Result<(), ProfileError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(ProfileError::NonFinite { field })
    }
}

fn profile_positive(field: &'static str, value: f64) -> Result<(), ProfileError> {
    profile_finite(field, value)?;
    if value > 0.0 {
        Ok(())
    } else {
        Err(ProfileError::NonPositive { field })
    }
}

fn profile_non_negative(field: &'static str, value: f64) -> Result<(), ProfileError> {
    profile_finite(field, value)?;
    if value >= 0.0 {
        Ok(())
    } else {
        Err(ProfileError::Negative { field })
    }
}

/// Rotates a body-space vector into world space.
fn rotated(vector: [f64; 3], orientation: cs_types::space::Quaternion) -> [f64; 3] {
    let [x, y, z, w] = orientation.components();
    let axis = [x, y, z];
    let axis_cross = cross(axis, vector);
    let twice = scaled(axis_cross, 2.0 * w);
    let second = cross(axis, axis_cross);
    added(added(vector, twice), scaled(second, 2.0))
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn added(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scaled(a: [f64; 3], factor: f64) -> [f64; 3] {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

/// Maps a control-axis torque onto the body components its axes turn.
fn axis_torque_to_body(axis_torque_nm: [f64; 3]) -> [f64; 3] {
    let mut body = [0.0; 3];
    for (axis, &(component, sign)) in EXCEPTIONAL_CONTROL_AXIS.iter().enumerate() {
        body[component] = sign * axis_torque_nm[axis];
    }
    body
}

/// The synthetic exceptional tuning: the F24 fixture's numbers with the
/// exceptional model kind.
///
/// Only [`AirframeTuning::model_kind`] differs from
/// [`super::synthetic::synthetic_fixed_wing`], so a test can compare the two
/// control laws on identical mass, wing, engine and stall behavior and see only
/// the exceptional difference. The *airframe's* real mass, area and thrust are
/// unknown and F25-D's; this fixture claims nothing about them
/// ([`Origin::SyntheticFixture`], [`ExceptionalProfile::is_measured`] is
/// `false`).
#[must_use]
pub fn synthetic_exceptional_tuning() -> AirframeTuning {
    AirframeTuning {
        model_kind: ModelKind::Exceptional,
        ..super::synthetic::synthetic_fixed_wing()
    }
}

/// The synthetic exceptional profile: an 8 m rotor that the airflow alone
/// drives, with lift capped at half the fixture's weight and no hover.
///
/// Every value is authored design with [`Origin::SyntheticFixture`] and a
/// `designed` claim; [`ExceptionalProfile::is_measured`] is `false`, so nothing
/// built from it may be read as a measured original value. The numbers are
/// chosen only to make the law's declared behavior testable and *plausible in
/// order of magnitude*: 0.4 rad/s per m/s puts a 4 m rotor at 16 rad/s and 64 m/s
/// of tip speed in 40 m/s of flight, the lift cap keeps the rotor from carrying
/// the 1200 kg fixture's 11.8 kN, and the 0–24 m/s tip-speed band gives the
/// rotor more control authority than the wing below about 15 m/s.
#[must_use]
pub fn synthetic_exceptional_profile() -> ExceptionalProfile {
    ExceptionalProfile {
        airframe_id: ContentId::from_source(ContentKind::Airframe, "fixture.synthetic-autogyro")
            .expect("the synthetic airframe id is valid"),
        hover: HoverCapability::NoHover,
        rotor_radius_m: 4.0,
        rotor_drive_radps_per_mps: 0.4,
        rotor_response_per_s: 1.0,
        rotor_lift_n_per_mps_tip: 220.0,
        rotor_lift_max_n: 6_000.0,
        rotor_drag_n_per_tip_air: 2.0,
        rotor_yaw_nm_per_radps: 60.0,
        rotor_polar_inertia_kg_m2: 220.0,
        control_tip_speed_zero_mps: 0.0,
        control_tip_speed_full_mps: 24.0,
        origin: Origin::SyntheticFixture,
        provenance: Provenance::designed(
            cs_types::evidence::ClaimId::new("f25b.profile.synthetic-autogyro")
                .expect("the declared claim id is valid"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flight::model::{EngineState, FlightEnvironment, FlightInput, FlightModel};
    use crate::flight::synthetic::synthetic_fixed_wing;
    use cs_types::asset_id::SourceSpan;
    use cs_types::evidence::ContentHash;

    /// A **synthetic** installation span, used only to drive the readiness gate
    /// below. It names no real installation and backs no original-data claim:
    /// the test asserts that such an envelope becomes reference-ready, not that
    /// any reference exists.
    fn synthetic_span() -> SourceSpan {
        SourceSpan::new(
            ContentHash::from_bytes([0x2a; 32]),
            "fixture.synthetic-exceptional.zbd",
            None,
            0,
            16,
            None,
        )
        .expect("a valid synthetic span")
    }

    fn state_at(speed_mps: f64, running: bool) -> FlightState {
        FlightState {
            linear_velocity_mps: [0.0, -1.0, -speed_mps],
            engine: if running {
                EngineState::direct(0.8)
            } else {
                EngineState::STOPPED
            },
            ..FlightState::at_rest(cs_types::space::Quaternion::IDENTITY)
        }
    }

    fn output_at(speed_mps: f64, running: bool) -> FlightOutput {
        let state = state_at(speed_mps, running);
        FlightModel::new(synthetic_fixed_wing())
            .compute(
                &FlightEnvironment::SEA_LEVEL,
                &super::super::tuning::LoadoutMass::EMPTY,
                &super::super::tuning::DamageState::PRISTINE,
                &state,
                &FlightInput::try_new(0.0, 0.0, 0.0, 0.8, false)
                    .expect("the probe input is in range"),
                SYNTHETIC_TICK_DT_S,
            )
            .expect("the synthetic probe case is valid")
    }

    /// The shared channel is the same numbers for both kinds and is finite at
    /// every declared state; an exceptional frame carries its rotor channel and
    /// a fixed-wing frame carries none.
    #[test]
    fn accept_f25_a_shared_telemetry_is_model_agnostic_and_kind_checked() {
        let state = state_at(55.0, true);
        let output = output_at(55.0, true);

        let fixed = TelemetryFrame::standard(ModelKind::FixedWing, &state, &output, Tick(7))
            .expect("a fixed-wing frame is valid");
        assert_eq!(fixed.tick(), Tick(7));
        assert_eq!(fixed.rotor(), None);
        assert!(fixed.shared().airspeed_mps.is_finite());
        assert_eq!(
            fixed.shared().airspeed_mps,
            output.instrument_state.airspeed_mps
        );
        assert_eq!(
            fixed.shared().stall_scale,
            output.instrument_state.stall_scale
        );

        let drive = {
            let mut drive = synthetic_rotor_drive();
            drive
                .advance_tick(40.0, 2.0, Tick(7), SYNTHETIC_TICK_DT_S)
                .expect("the first advance is valid");
            drive
        };
        let exceptional = TelemetryFrame::exceptional(
            ModelKind::Exceptional,
            &state,
            &output,
            Tick(7),
            &drive,
            Some(&synthetic_rotor_mapping()),
        )
        .expect("an exceptional frame is valid");
        assert!(exceptional.rotor().is_some());
        assert_eq!(
            exceptional.shared().airspeed_mps,
            fixed.shared().airspeed_mps,
            "the shared channel does not depend on the model kind"
        );

        // The declared kind and the present channels must agree.
        assert_eq!(
            TelemetryFrame::standard(ModelKind::Exceptional, &state, &output, Tick(7)).err(),
            Some(TelemetryError::ModelKindMismatch {
                declared: ModelKind::Exceptional,
                has_rotor: false,
            })
        );
        assert_eq!(
            TelemetryFrame::exceptional(
                ModelKind::FixedWing,
                &state,
                &output,
                Tick(7),
                &drive,
                None
            )
            .err(),
            Some(TelemetryError::ModelKindMismatch {
                declared: ModelKind::FixedWing,
                has_rotor: true,
            })
        );
    }

    /// A non-finite production value is refused by name at the telemetry
    /// boundary rather than reported to a HUD as a plausible number.
    #[test]
    fn accept_f25_a_telemetry_boundary_rejects_nonfinite_by_name() {
        let state = state_at(55.0, true);
        let mut output = output_at(55.0, true);
        output.instrument_state.stall_scale = f64::NAN;
        assert_eq!(
            SharedTelemetry::sample(Tick(0), &state, &output).err(),
            Some(TelemetryError::NonFinite {
                field: "telemetry.stall_scale"
            })
        );

        let mut corrupt = output_at(55.0, true);
        corrupt.diagnostics.control_authority = 1.5;
        assert_eq!(
            SharedTelemetry::sample(Tick(0), &state, &corrupt).err(),
            Some(TelemetryError::OutOfRange {
                field: "telemetry.control_authority",
                value: 1.5,
            })
        );
    }

    /// The physical rotor rate moves only on a strictly newer fixed tick: a
    /// second advance in the same tick, or a stale one, is refused, which is
    /// what stops a render frame from driving physics dt.
    #[test]
    fn accept_f25_a_rotor_rate_advances_only_on_a_newer_fixed_tick() {
        let mut drive = synthetic_rotor_drive();
        assert_eq!(drive.physical_speed_radps(), 0.0);
        drive
            .advance_tick(40.0, 4.0, Tick(0), SYNTHETIC_TICK_DT_S)
            .expect("the first tick advances");
        let after_one = drive.physical_speed_radps();
        assert!(after_one > 0.0, "the rotor spooled up");
        assert_eq!(drive.last_tick(), Some(Tick(0)));

        assert_eq!(
            drive.advance_tick(40.0, 4.0, Tick(0), SYNTHETIC_TICK_DT_S),
            Err(TelemetryError::NonMonotonicTick {
                last: Tick(0),
                got: Tick(0),
            })
        );
        assert_eq!(
            drive.advance_tick(40.0, 4.0, Tick(0), 0.016),
            Err(TelemetryError::NonMonotonicTick {
                last: Tick(0),
                got: Tick(0),
            }),
            "a render-sized dt for an old tick is refused too"
        );
        assert_eq!(
            drive.physical_speed_radps(),
            after_one,
            "a refused advance leaves the rate untouched"
        );

        assert_eq!(
            drive.advance_tick(40.0, 4.0, Tick(1), 0.0),
            Err(TelemetryError::NonPositive {
                field: "rotor.tick_dt_s"
            })
        );
        assert_eq!(
            drive.advance_tick(f64::NAN, 4.0, Tick(1), SYNTHETIC_TICK_DT_S),
            Err(TelemetryError::NonFinite {
                field: "rotor.commanded_radps"
            })
        );
    }

    /// The visual rate requires an explicit mapping and honors a ratio that is
    /// not 1:1; with no mapping there is no visual rate at all, never an
    /// implicit 1:1. Sampling the visual also leaves the physical rate
    /// unchanged, so the render frame rate cannot reach the simulation.
    #[test]
    fn accept_f25_a_visual_rotor_rate_needs_a_declared_mapping() {
        let mut drive = synthetic_rotor_drive();
        for tick in 1..=120 {
            drive
                .advance_tick(40.0, 48.0, Tick(tick), SYNTHETIC_TICK_DT_S)
                .expect("each tick is newer than the last");
        }
        let physical = drive.physical_speed_radps();
        assert!((physical - 40.0).abs() < 1e-9, "the spool reached command");

        let unmapped = drive.telemetry(None);
        assert_eq!(unmapped.physical_speed_radps, physical);
        assert_eq!(
            unmapped.visual_speed_radps(),
            None,
            "no declared mapping means no visual rate"
        );
        let unmapped_sample = drive
            .visual_sample(None, RotorVisualSample::at_rest(), 0.016)
            .expect("sampling without a mapping is finite");
        assert_eq!(unmapped_sample.visual, None);
        assert_eq!(unmapped_sample.phase_rad, 0.0);

        let mapping = synthetic_rotor_mapping();
        assert_eq!(mapping.visual_radps_per_physical_radps(), 1.5);
        assert_eq!(mapping.origin(), &Origin::SyntheticFixture);
        let mapped = drive.telemetry(Some(&mapping));
        let visual = mapped
            .visual_speed_radps()
            .expect("a declared mapping produces a visual rate");
        assert!(
            (visual - physical * 1.5).abs() < 1e-9,
            "physical and visual rates differ by the declared ratio"
        );
        assert_eq!(
            mapped.visual.expect("mapped").origin,
            Origin::SyntheticFixture
        );

        assert_eq!(
            RotorSpeedMapping::new(0.0, Origin::SyntheticFixture).err(),
            Some(TelemetryError::NonPositive {
                field: "rotor.visual_radps_per_physical_radps"
            })
        );
        assert_eq!(
            RotorSpeedMapping::new(f64::INFINITY, Origin::SyntheticFixture).err(),
            Some(TelemetryError::NonFinite {
                field: "rotor.visual_radps_per_physical_radps"
            })
        );
    }

    /// Drawing the rotor at one frame per tick and at many frames per tick
    /// leaves the simulation identical: only the visual phase differs, and it
    /// advances by exactly the visual rate times the elapsed render time.
    #[test]
    fn accept_f25_a_rotor_visual_sampling_cannot_change_the_simulation() {
        let mapping = synthetic_rotor_mapping();

        let run = |frames_per_tick: u32| {
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
                        .expect("the visual sample is finite");
                }
            }
            (drive, sample)
        };

        let (slow_drive, slow_sample) = run(1);
        let (fast_drive, fast_sample) = run(60);
        assert_eq!(
            slow_drive.physical_speed_radps(),
            fast_drive.physical_speed_radps(),
            "the authoritative rate does not depend on the render frame rate"
        );
        assert_eq!(
            slow_sample.physical_speed_radps,
            fast_sample.physical_speed_radps
        );

        // 120 ticks is one second of render time either way, so both draws end
        // on the same phase.
        assert!(
            (slow_sample.phase_rad - fast_sample.phase_rad).abs() < 1e-9,
            "the drawn phase depends only on elapsed render time"
        );
        assert!(slow_sample.phase_rad >= 0.0 && slow_sample.phase_rad < TAU);
        assert!((fast_sample.phase_rad - slow_sample.phase_rad).abs() < 1e-9);

        assert_eq!(
            slow_drive
                .visual_sample(Some(&mapping), RotorVisualSample::at_rest(), 0.0)
                .err(),
            Some(TelemetryError::NonPositive {
                field: "rotor_visual.render_dt_s"
            })
        );
        assert_eq!(
            slow_drive
                .visual_sample(Some(&mapping), RotorVisualSample::at_rest(), f64::NAN)
                .err(),
            Some(TelemetryError::NonFinite {
                field: "rotor_visual.render_dt_s"
            })
        );
    }

    /// A drawn phase is checked before it leaves the boundary: finite inputs
    /// whose product overflows (an absurd physical rate times an absurd frame
    /// time) would otherwise hand a consumer a `NaN` phase that compares false
    /// against every plausible expectation.
    #[test]
    fn accept_f25_a_visual_rotor_sample_never_returns_a_nonfinite_phase() {
        let mut drive = synthetic_rotor_drive();
        drive
            .advance_tick(1e300, 1e300, Tick(1), SYNTHETIC_TICK_DT_S)
            .expect("a finite commanded rate is accepted however absurd");
        assert!(
            drive.physical_speed_radps() > 1e290,
            "the spool reached an absurd but finite rate"
        );
        let absurd = RotorSpeedMapping::new(1e300, Origin::SyntheticFixture)
            .expect("a finite positive ratio is accepted however absurd");

        assert_eq!(
            drive
                .visual_sample(Some(&absurd), RotorVisualSample::at_rest(), 1e300)
                .err(),
            Some(TelemetryError::NonFinite {
                field: "rotor_visual.phase_rad"
            }),
            "an overflowing draw is refused by name, not returned as a NaN phase"
        );
        // The physical rate itself is finite, and the overflowing mapped rate is
        // refused where it is produced.
        assert!(drive.physical_speed_radps().is_finite());
        assert_eq!(
            drive.telemetry(Some(&absurd)).validate().err(),
            Some(TelemetryError::NonFinite {
                field: "rotor.visual_speed_radps"
            })
        );
    }

    /// The exceptional envelope demands every maneuver its kind requires, holds
    /// one out of the fit, and is **not** ready as a reference: no original
    /// capture backs the synthetic fixture, and neither setting a designed
    /// provenance nor setting an observed one on a synthetic envelope can make
    /// it one.
    #[test]
    fn accept_f25_a_exceptional_envelope_demands_its_maneuvers_and_is_unmeasured() {
        let envelope = synthetic_exceptional_envelope();
        assert_eq!(envelope.validate(), Ok(()));
        assert_eq!(envelope.origin, Origin::SyntheticFixture);
        assert_eq!(envelope.model_kind, ModelKind::Exceptional);
        assert!(envelope.missing_required().is_empty());
        assert!(envelope.maneuvers.iter().any(|spec| spec.held_out));
        assert!(
            !envelope.is_ready_as_reference(),
            "a synthetic fixture is never an approved reference trace"
        );
        assert_eq!(
            envelope.status,
            EnvelopeStatus::Unmeasured {
                reason: "no original capture of an exceptional airframe exists".to_owned()
            }
        );

        // An exceptional envelope missing a rotor-visual maneuver is refused by
        // name, not silently unmeasured.
        let mut missing = synthetic_exceptional_envelope();
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
        assert!(!missing.is_ready_as_reference());

        // A measured status backed by a claim nobody observed is refused: a
        // designed envelope is not a measurement.
        let claim =
            || cs_types::evidence::ClaimId::new("f25a.test.envelope").expect("a valid claim id");
        let mut measured = synthetic_exceptional_envelope();
        measured.status = EnvelopeStatus::Measured {
            provenance: Provenance::designed(claim()),
        };
        assert_eq!(
            measured.validate(),
            Err(EnvelopeError::MeasuredWithoutObservation {
                class: ClaimStatus::Designed
            }),
            "an engineered fixture cannot declare itself a measured reference"
        );
        assert!(!measured.is_ready_as_reference());

        // An observed provenance alone is still not enough: the envelope itself
        // must have come from the original installation, so the synthetic
        // fixture stays not-ready however its status is set.
        let observed_source = synthetic_span();
        let mut observed = synthetic_exceptional_envelope();
        observed.status = EnvelopeStatus::Measured {
            provenance: Provenance::new(
                claim(),
                ClaimStatus::VerifiedOriginal,
                Some(observed_source.clone()),
            )
            .expect("a located verified_original provenance is accepted"),
        };
        assert_eq!(observed.validate(), Ok(()));
        assert!(
            !observed.is_ready_as_reference(),
            "a synthetic envelope is never reference-ready"
        );

        // Only an installation-sourced envelope with an observed reference, full
        // coverage and a held-out maneuver may be compared against.
        let mut original = observed.clone();
        original.origin = Origin::Installation {
            source: observed_source,
        };
        assert_eq!(original.validate(), Ok(()));
        assert!(original.is_ready_as_reference());

        let mut undocumented = observed.clone();
        undocumented.status = EnvelopeStatus::Measured {
            provenance: Provenance::new(claim(), ClaimStatus::Documented, None)
                .expect("a documented claim needs no source span"),
        };
        assert_eq!(
            undocumented.validate(),
            Err(EnvelopeError::MeasuredWithoutObservation {
                class: ClaimStatus::Documented
            })
        );
        assert!(!undocumented.is_ready_as_reference());

        let mut incomplete = original.clone();
        incomplete
            .maneuvers
            .retain(|spec| spec.kind != ManeuverKind::Turn);
        assert!(
            !incomplete.is_ready_as_reference(),
            "a missing maneuver keeps the envelope out of reference use"
        );

        let mut no_holdout = original.clone();
        for spec in &mut no_holdout.maneuvers {
            spec.held_out = false;
        }
        assert_eq!(no_holdout.validate(), Err(EnvelopeError::NoHeldOutManeuver));

        let mut no_tolerance = synthetic_exceptional_envelope();
        no_tolerance.maneuvers[0].tolerance = 0.0;
        assert_eq!(
            no_tolerance.validate(),
            Err(EnvelopeError::NonPositiveTolerance {
                kind: ManeuverKind::Acceleration
            })
        );

        let mut duplicated = synthetic_exceptional_envelope();
        duplicated.maneuvers.push(duplicated.maneuvers[0].clone());
        assert_eq!(
            duplicated.validate(),
            Err(EnvelopeError::DuplicateManeuver {
                kind: ManeuverKind::Acceleration
            })
        );

        let mut blank = synthetic_exceptional_envelope();
        blank.status = EnvelopeStatus::Unmeasured {
            reason: "   ".to_owned(),
        };
        assert_eq!(blank.validate(), Err(EnvelopeError::BlankUnmeasuredReason));

        // A fixed-wing envelope does not require the exceptional maneuvers.
        assert!(!ManeuverKind::RotorVisual.required_for(ModelKind::FixedWing));
        assert!(ManeuverKind::RotorVisual.required_for(ModelKind::Exceptional));
        assert!(ManeuverKind::Turn.required_for(ModelKind::FixedWing));
        assert_eq!(
            ManeuverKind::from_label("rotor_visual"),
            Some(ManeuverKind::RotorVisual)
        );
        assert_eq!(ManeuverKind::from_label("warp_drive"), None);
    }
}

// ---------------------------------------------------------------------------
// F25-B tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod law_tests {
    use super::*;
    use crate::flight::model::{BODY_UP, EngineState};
    use crate::flight::synthetic::synthetic_fixed_wing;
    use cs_types::asset_id::SourceSpan;
    use cs_types::evidence::ContentHash;
    use cs_types::space::Quaternion;

    /// A **synthetic** installation span, used only to drive the provenance
    /// gate below. It names no real installation and backs no original-data
    /// claim: the test asserts that such a profile becomes *measured* on its
    /// boundary, not that any measurement exists.
    fn synthetic_span() -> SourceSpan {
        SourceSpan::new(
            ContentHash::from_bytes([0x2b; 32]),
            "fixture.synthetic-exceptional.zbd",
            None,
            0,
            16,
            None,
        )
        .expect("a valid synthetic span")
    }

    /// Ticks that let the declared first-order rotor response settle on its
    /// command. With `rotor_response_per_s == 1.0` and a `1/120` s tick the
    /// remaining error after `n` ticks is `exp(-n/120)`, so 2400 ticks is
    /// twenty seconds and leaves under `1e-6` rad/s of error.
    const SETTLE_TICKS: u64 = 2400;

    fn law() -> ExceptionalControlLaw {
        ExceptionalControlLaw::new(
            synthetic_exceptional_tuning(),
            synthetic_exceptional_profile(),
        )
        .expect("the declared synthetic exceptional law is valid")
    }

    fn wing_model() -> FlightModel {
        FlightModel::new(synthetic_fixed_wing())
    }

    fn level(speed_mps: f64, spool: Option<f64>) -> FlightState {
        FlightState {
            linear_velocity_mps: [0.0, 0.0, -speed_mps],
            engine: spool.map_or(EngineState::STOPPED, EngineState::direct),
            ..FlightState::at_rest(Quaternion::IDENTITY)
        }
    }

    fn stick(pitch: f64, roll: f64, yaw: f64, throttle: f64) -> FlightInput {
        FlightInput::try_new(pitch, roll, yaw, throttle, false)
            .expect("the probe input is inside its declared range")
    }

    fn one_tick(
        law: &ExceptionalControlLaw,
        state: &FlightState,
        input: &FlightInput,
        tick: Tick,
        rotor: &mut RotorDrive,
    ) -> ExceptionalTick {
        law.compute(
            &FlightEnvironment::SEA_LEVEL,
            &LoadoutMass::EMPTY,
            &DamageState::PRISTINE,
            state,
            input,
            SYNTHETIC_TICK_DT_S,
            tick,
            rotor,
        )
        .expect("the probe tick is a legal exceptional tick")
    }

    /// A rotor already spun up to `commanded_radps`, together with the first
    /// tick a caller may advance it at. Returning the tick matters: the drive
    /// refuses a tick that is not strictly newer, so a test that pre-spins a
    /// rotor cannot then evaluate a tick from the start of the sequence.
    fn prespun(commanded_radps: f64) -> (RotorDrive, u64) {
        let mut rotor = RotorDrive::stopped();
        let mut tick = 0;
        while (rotor.physical_speed_radps() - commanded_radps).abs() > 1e-9 {
            tick += 1;
            assert!(
                tick <= SETTLE_TICKS,
                "the pre-spin settles inside the declared tick budget"
            );
            rotor
                .advance_tick(commanded_radps, 400.0, Tick(tick), SYNTHETIC_TICK_DT_S)
                .expect("each pre-spin tick is newer than the last");
        }
        assert!((rotor.physical_speed_radps() - commanded_radps).abs() < 1e-6);
        (rotor, tick + 1)
    }

    /// A rotor already settled on the command the airflow declares for
    /// `airspeed_mps`, with the first tick a caller may use.
    fn settled_rotor(law: &ExceptionalControlLaw, airspeed_mps: f64) -> (RotorDrive, u64) {
        let commanded = law
            .commanded_rotor_radps(airspeed_mps)
            .expect("a positive airspeed commands a finite rate");
        prespun(commanded)
    }

    /// Rotates a world-space vector into body space, which is how a probe reads
    /// the two laws' torques in the same frame.
    fn into_body(vector: [f64; 3], orientation: Quaternion) -> [f64; 3] {
        let [x, y, z, w] = orientation.components();
        let conjugate =
            Quaternion::try_new([-x, -y, -z, w]).expect("a conjugate of a unit rotation is unit");
        rotated(vector, conjugate)
    }

    fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    /// AC02's low-speed half: every declared low-speed, engine-on and engine-off
    /// state is a legal tick, every produced value is finite, and the rotor
    /// answers the profile's own closed form rather than a plausible-looking
    /// number.
    #[test]
    fn accept_f25_b_low_speed_states_stay_finite_and_answer_the_profile() {
        let law = law();
        let inputs = [
            FlightInput::NEUTRAL,
            stick(0.0, 0.0, 0.0, 1.0),
            stick(1.0, 0.0, 0.0, 0.0),
            stick(-1.0, 0.0, 0.0, 0.0),
            stick(0.0, 1.0, 0.0, 0.0),
            stick(0.0, -1.0, 0.0, 0.0),
            stick(0.0, 0.0, 1.0, 0.0),
            stick(0.5, -0.5, 0.5, 0.5),
        ];
        let engines = [Some(0.0), Some(0.5), Some(1.0), None];

        for speed in [0.0, 0.25, 1.0, 3.0, 8.0, 15.0] {
            for spool in engines {
                for input in inputs {
                    let state = level(speed, spool);
                    let mut rotor = RotorDrive::stopped();
                    let computed = one_tick(&law, &state, &input, Tick(1), &mut rotor);
                    computed
                        .validate()
                        .expect("every produced value is finite and fully accounted");

                    // The rotor's rate after one tick is the profile's own
                    // first-order response, not a plausible number.
                    let commanded = law
                        .commanded_rotor_radps(speed)
                        .expect("the airspeed is a legal rotor command");
                    assert_eq!(computed.diagnostics.commanded_rotor_radps, commanded);
                    let profile = law.profile();
                    let expected =
                        commanded.min(profile.rotor_response_per_s * SYNTHETIC_TICK_DT_S);
                    assert!(
                        (computed.rotor.physical_speed_radps() - expected).abs() < 1e-12,
                        "at {speed} m/s the rotor moved to {expected} rad/s, not {}",
                        computed.rotor.physical_speed_radps()
                    );
                    // Rotor lift is capped and never negative, and both rotor
                    // force terms are zero without rotation.
                    assert!(computed.diagnostics.rotor_lift_n >= 0.0);
                    assert!(computed.diagnostics.rotor_lift_n <= profile.rotor_lift_max_n);
                    assert!(computed.diagnostics.rotor_drag_n >= 0.0);
                    if speed == 0.0 {
                        assert_eq!(computed.diagnostics.rotor_lift_n, 0.0);
                        assert_eq!(computed.diagnostics.rotor_drag_n, 0.0);
                        assert_eq!(computed.diagnostics.rotor_radps, 0.0);
                    }
                }
            }
        }

        // At rest, with a rotor at rest and the throttle wide open, the only
        // vertical force is gravity: the exceptional airframe falls.
        let at_rest = FlightState::at_rest(Quaternion::IDENTITY);
        let mut rotor = RotorDrive::stopped();
        let computed = one_tick(
            &law,
            &at_rest,
            &stick(0.0, 0.0, 0.0, 1.0),
            Tick(1),
            &mut rotor,
        );
        assert_eq!(computed.rotor.physical_speed_radps(), 0.0);
        assert_eq!(computed.diagnostics.control_authority, 0.0);
        let weight_n = 1200.0 * FlightEnvironment::SEA_LEVEL.gravity_mps2;
        assert!(
            (computed.output.world_force_n[1] + weight_n).abs() < 1e-9,
            "at rest the vertical force is gravity alone, got {}",
            computed.output.world_force_n[1]
        );
        assert!(
            computed.output.world_force_n[1] < 0.0,
            "an exceptional airframe with no airflow and no thrust falls"
        );
    }

    /// Non-negotiable behavior 1, first half: the airframe is not a fixed wing
    /// with a spinning mesh. At low speed the rotor carries the authority and
    /// most of the lift, where the fixed wing on the identical airframe has
    /// neither.
    #[test]
    fn accept_f25_b_the_rotor_carries_low_speed_authority_and_lift() {
        let law = law();
        let profile = law.profile().clone();
        let speed = 3.0;
        let state = level(speed, Some(0.5));
        let input = stick(0.0, 0.0, 0.5, 0.5);
        let mut rotor = RotorDrive::stopped();
        let mut computed = None;
        for tick in 1..=SETTLE_TICKS {
            computed = Some(one_tick(&law, &state, &input, Tick(tick), &mut rotor));
        }
        let computed = computed.expect("the settled run produced a tick");

        // The rotor settled on the airflow's command, and every derived reading
        // is the profile's own function of it.
        let commanded = profile.rotor_drive_radps_per_mps * speed;
        assert!((computed.rotor.physical_speed_radps() - commanded).abs() < 1e-6);
        let tip = profile.rotor_radius_m * commanded;
        assert!((computed.diagnostics.rotor_tip_speed_mps - tip).abs() < 1e-5);
        assert!(
            (computed.diagnostics.rotor_support - tip / profile.control_tip_speed_full_mps).abs()
                < 1e-5
        );
        assert!(
            (computed.diagnostics.wing_authority
                - speed / law.tuning().angular.control_airspeed_full_mps)
                .abs()
                < 1e-12
        );
        assert!(
            (computed.diagnostics.control_authority - computed.diagnostics.rotor_support).abs()
                < 1e-5,
            "at 3 m/s the rotor, not the wing, sets the authority"
        );
        assert!(
            computed.diagnostics.rotor_support > computed.diagnostics.wing_authority,
            "the rotor's declared band puts it above the wing's airspeed ramp here"
        );

        // Rotor lift and drag are real, and rotor lift dominates the wing's at
        // this airspeed.
        let expected_lift = profile.rotor_lift_n_per_mps_tip * tip;
        assert!((computed.diagnostics.rotor_lift_n - expected_lift).abs() < 1e-3);
        assert!(computed.diagnostics.rotor_lift_n < profile.rotor_lift_max_n);
        assert!(computed.diagnostics.rotor_drag_n > 0.0);
        assert!(
            computed.diagnostics.rotor_lift_n > 50.0 * computed.diagnostics.base.lift_n,
            "the rotor, not the wing, is the low-speed lift source"
        );
        assert!(
            dot(
                computed.diagnostics.rotor_drag_force_n,
                state.linear_velocity_mps
            ) < 0.0
        );

        // The identical airframe under the fixed-wing law has far less of both.
        let fixed = wing_model()
            .compute(
                &FlightEnvironment::SEA_LEVEL,
                &LoadoutMass::EMPTY,
                &DamageState::PRISTINE,
                &state,
                &input,
                SYNTHETIC_TICK_DT_S,
            )
            .expect("the fixed-wing tick is legal");
        assert!(fixed.diagnostics.control_authority < computed.diagnostics.control_authority * 0.5);
        assert_ne!(fixed.world_force_n, computed.output.world_force_n);

        // Rotor lift acts along the shaft axis whatever the airframe's
        // attitude, so the law follows the body, not the world.
        let banked = FlightState {
            orientation: Quaternion::try_new([0.258_819_045_1, 0.0, 0.0, 0.965_925_826_3])
                .expect("a unit rotation"),
            ..state
        };
        let (mut banked_rotor, next_tick) = settled_rotor(&law, speed);
        let banked_tick = one_tick(&law, &banked, &input, Tick(next_tick), &mut banked_rotor);
        let up_world = rotated(BODY_UP, banked.orientation);
        assert!(
            (banked_tick.diagnostics.rotor_lift_force_n[1]
                - banked_tick.diagnostics.rotor_lift_n * up_world[1])
                .abs()
                < 1e-9
        );
        assert!(
            up_world[1] < 1.0,
            "the test banked the airframe away from level"
        );
    }

    /// Both rotor forces are functions of the **air-relative** velocity, not of
    /// the airframe's own velocity: a headwind raises the drive, the lift and the
    /// drag, and the drag still opposes the relative airflow rather than the
    /// ground track. A tailwind of the same magnitude lowers all three.
    #[test]
    fn accept_f25_b_rotor_forces_follow_the_air_relative_velocity_not_the_ground_track() {
        let law = law();
        let profile = law.profile().clone();
        let speed = 6.0;
        let state = level(speed, Some(0.6));
        let input = stick(0.0, 0.0, 0.0, 0.6);
        // The airframe flies along −Z. The contract's `v_air = v_world −
        // wind_world` therefore makes a wind blowing along **+Z** — against the
        // flight — the one that raises the air-relative speed, so that is the
        // headwind here and its z component is positive.
        let headwind = 4.0_f64;

        // Each case settles its rotor on the command *its own* airflow declares,
        // so the probe reads a settled law rather than a transient — otherwise
        // the lift would be identical in all three and would prove nothing. The
        // airspeeds stay well below the profile's lift cap, so the ordering is
        // the cap-free one.
        let sample = |wind_z: f64| {
            let environment = FlightEnvironment {
                wind_velocity_mps: [0.0, 0.0, wind_z],
                ..FlightEnvironment::SEA_LEVEL
            };
            let air_velocity = [
                state.linear_velocity_mps[0],
                state.linear_velocity_mps[1],
                state.linear_velocity_mps[2] - wind_z,
            ];
            let airspeed = dot(air_velocity, air_velocity).sqrt();
            let commanded = law
                .commanded_rotor_radps(airspeed)
                .expect("a non-negative airspeed commands a finite rate");
            let (mut rotor, tick) = prespun(commanded);
            let computed = law
                .compute(
                    &environment,
                    &LoadoutMass::EMPTY,
                    &DamageState::PRISTINE,
                    &state,
                    &input,
                    SYNTHETIC_TICK_DT_S,
                    Tick(tick),
                    &mut rotor,
                )
                .expect("a windy tick is still a legal tick");
            assert!(
                (computed.rotor.physical_speed_radps() - commanded).abs() < 1e-6,
                "the rotor is settled on the air-relative command"
            );
            (computed, air_velocity)
        };

        let (into, into_air) = sample(headwind);
        let (still, still_air) = sample(0.0);
        let (tail, tail_air) = sample(-headwind);

        // The reported airspeed is the relative one, and the drive follows it.
        assert!((into.output.instrument_state.airspeed_mps - (speed + headwind)).abs() < 1e-9);
        assert!((tail.output.instrument_state.airspeed_mps - (speed - headwind)).abs() < 1e-9);
        assert!(
            (into.diagnostics.commanded_rotor_radps
                - profile.rotor_drive_radps_per_mps * (speed + headwind))
                .abs()
                < 1e-9,
            "a headwind commands a faster rotor"
        );
        assert!(
            tail.diagnostics.commanded_rotor_radps < still.diagnostics.commanded_rotor_radps
                && still.diagnostics.commanded_rotor_radps < into.diagnostics.commanded_rotor_radps,
            "the drive orders with the relative airflow"
        );

        // Lift is a function of the tip speed, so it rises into the wind, and in
        // every case it is the profile's own capped function of that tip speed.
        assert!(into.diagnostics.rotor_lift_n > still.diagnostics.rotor_lift_n);
        assert!(still.diagnostics.rotor_lift_n > tail.diagnostics.rotor_lift_n);
        for tick in [into, still, tail] {
            let uncapped = profile.rotor_lift_n_per_mps_tip * tick.diagnostics.rotor_tip_speed_mps;
            assert!(
                (tick.diagnostics.rotor_lift_n - uncapped.min(profile.rotor_lift_max_n)).abs()
                    < 1e-6,
                "rotor lift is the profile's own capped function of the tip speed"
            );
        }

        // Drag is a function of the tip speed *and* the relative airspeed, and it
        // opposes the relative airflow. The airframe's own velocity is identical
        // in all three cases, so only the wind can explain the difference — and a
        // law that used the ground track would order these the other way round
        // in the tailwind case, where the airframe is still moving forward but
        // the air it is moving through is not.
        assert!(into.diagnostics.rotor_drag_n > still.diagnostics.rotor_drag_n);
        assert!(still.diagnostics.rotor_drag_n > tail.diagnostics.rotor_drag_n);
        for (air_velocity, tick) in [(into_air, into), (still_air, still), (tail_air, tail)] {
            assert!(
                dot(tick.diagnostics.rotor_drag_force_n, air_velocity) < 0.0,
                "the rotor drag must oppose the air-relative velocity {air_velocity:?}, got {:?}",
                tick.diagnostics.rotor_drag_force_n
            );
        }

        // A *cross*wind is what separates the two vectors: with the wind along
        // the flight both the ground track and the relative airflow point the
        // same way, so only a crosswind can tell a law that uses the air-relative
        // velocity from one that uses the airframe's own. The rotor is pre-spun
        // on the crosswind case's own command, so its rate is right before the
        // single tick is evaluated.
        let crosswind = [0.0, 6.0, 0.0];
        let environment = FlightEnvironment {
            wind_velocity_mps: crosswind,
            ..FlightEnvironment::SEA_LEVEL
        };
        let cross_air = environment.air_relative_velocity_m_s(state.linear_velocity_mps);
        let cross_airspeed = dot(cross_air, cross_air).sqrt();
        let (mut cross_rotor, cross_tick) = prespun(
            law.commanded_rotor_radps(cross_airspeed)
                .expect("the crosswind airspeed is a legal command"),
        );
        let cross = law
            .compute(
                &environment,
                &LoadoutMass::EMPTY,
                &DamageState::PRISTINE,
                &state,
                &input,
                SYNTHETIC_TICK_DT_S,
                Tick(cross_tick),
                &mut cross_rotor,
            )
            .expect("a crosswind tick is still a legal tick");
        assert!(
            (cross.output.instrument_state.airspeed_mps - cross_airspeed).abs() < 1e-9,
            "the reported airspeed is the relative one, not the ground-track 6 m/s"
        );
        assert!(
            cross.diagnostics.rotor_drag_n > 0.0,
            "a crosswind still loads the rotor disc"
        );
        // The drag opposes the relative airflow, which is neither the ground
        // track nor the wind: it is their difference.
        assert!(
            dot(cross.diagnostics.rotor_drag_force_n, cross_air) < 0.0,
            "the rotor drag must oppose the air-relative velocity {cross_air:?}, got {:?}",
            cross.diagnostics.rotor_drag_force_n
        );
        assert!(
            dot(cross.diagnostics.rotor_drag_force_n, crosswind) > 0.0,
            "a crosswind pushes the airframe downwind, so the drag is not against the wind"
        );
        assert!(
            dot(
                cross.diagnostics.rotor_drag_force_n,
                state.linear_velocity_mps
            ) < 0.0,
            "the rotor drag still has a component against the forward ground track"
        );
    }

    /// AC02's engine-off half: with the engine stopped the rotor winds down to
    /// the airflow's command rather than to nothing, the thrust is exactly
    /// zero, and no value becomes non-finite — and the same airspeed with the
    /// engine running settles on the *same* rotor rate, which is what shows the
    /// rotor is driven by the air and not by the engine.
    #[test]
    fn accept_f25_b_engine_off_winds_the_rotor_down_and_stays_finite() {
        let law = law();
        let profile = law.profile().clone();
        let speed = 20.0;
        let input = stick(0.0, 0.0, 0.0, 0.0);
        let engine_off = level(speed, None);
        let commanded = law
            .commanded_rotor_radps(speed)
            .expect("the airspeed is a legal rotor command");

        // A rotor pre-spun well above the airflow's command, as a launch would
        // leave it.
        let (mut rotor, next_tick) = prespun(20.0);
        let pre_spun = rotor.physical_speed_radps();
        assert!((pre_spun - 20.0).abs() < 1e-6);

        let mut previous = pre_spun;
        let mut computed = None;
        for step in 0..SETTLE_TICKS {
            let tick = next_tick + step;
            let out = one_tick(&law, &engine_off, &input, Tick(tick), &mut rotor);
            assert_eq!(
                out.output.diagnostics.thrust_n, 0.0,
                "a stopped engine produces no thrust at all"
            );
            assert_eq!(out.output.accepted_boost_consumption, 0.0);
            out.validate()
                .expect("an engine-off tick stays finite and fully accounted");

            // The step is bounded by the profile's response, so nothing but the
            // fixed tick can move the rate.
            let max_step = profile.rotor_response_per_s * SYNTHETIC_TICK_DT_S;
            let moved_radps = (out.rotor.physical_speed_radps() - previous).abs();
            assert!(
                moved_radps <= max_step + 1e-12,
                "the rotor moved {moved_radps} rad/s in one tick"
            );
            assert!(
                out.rotor.physical_speed_radps() <= previous + 1e-12,
                "with the engine off the rotor never speeds up"
            );
            if previous - commanded > max_step {
                assert!(
                    out.rotor.physical_speed_radps() < previous,
                    "with the engine off the rotor winds down toward the airflow's command"
                );
            }
            previous = out.rotor.physical_speed_radps();
            computed = Some(out);
        }
        let computed = computed.expect("the settled engine-off run produced a tick");
        assert!(
            (computed.rotor.physical_speed_radps() - commanded).abs() < 1e-6,
            "the rotor settles on the airflow's command, not on zero"
        );
        assert!(computed.diagnostics.rotor_lift_n > 0.0);

        // The same airspeed with the engine running settles on the same rate.
        let mut running_rotor = RotorDrive::stopped();
        let mut running = None;
        for tick in 1..=SETTLE_TICKS {
            running = Some(one_tick(
                &law,
                &level(speed, Some(0.8)),
                &stick(0.0, 0.0, 0.0, 0.8),
                Tick(tick),
                &mut running_rotor,
            ));
        }
        let running = running.expect("the settled engine-on run produced a tick");
        assert!(running.output.diagnostics.thrust_n > 0.0);
        assert!(
            (running.rotor.physical_speed_radps() - computed.rotor.physical_speed_radps()).abs()
                < 1e-6,
            "the rotor's steady state is the airflow's, not the engine's"
        );

        // With no airspeed at all a pre-spun rotor gives its energy back: the
        // rate reaches exactly zero, the lift with it, and the airframe falls.
        let (mut quiet, quiet_tick) = prespun(20.0);
        let at_rest = FlightState::at_rest(Quaternion::IDENTITY);
        let mut step = 0;
        let last = loop {
            let tick = one_tick(
                &law,
                &at_rest,
                &stick(0.0, 0.0, 0.0, 1.0),
                Tick(quiet_tick + step),
                &mut quiet,
            );
            step += 1;
            if tick.rotor.physical_speed_radps() == 0.0 {
                break tick;
            }
            assert!(
                step <= SETTLE_TICKS,
                "a pre-spun rotor gives its energy back inside the declared budget"
            );
        };
        assert_eq!(last.rotor.physical_speed_radps(), 0.0);
        assert_eq!(last.diagnostics.rotor_lift_n, 0.0);
        assert!(last.output.world_force_n[1] < 0.0);
    }

    /// Non-negotiable behavior 1, second half: the airframe is not a hovering
    /// helicopter. The throttle reaches the engine and the engine's spool
    /// reaches the thrust, but **no path exists from either to the rotor** — and
    /// a profile that claims hover is refused by name rather than flown.
    #[test]
    fn accept_f25_b_throttle_has_no_path_to_the_rotor() {
        let law = law();
        let speed = 12.0;
        let full = stick(0.0, 0.0, 0.0, 1.0);
        let idle = stick(0.0, 0.0, 0.0, 0.0);

        // The engine spool is the throttle's path into the simulation, and it
        // changes the thrust and nothing about the rotor.
        let spooled_down = one_tick(
            &law,
            &level(speed, Some(0.0)),
            &full,
            Tick(1),
            &mut RotorDrive::stopped(),
        );
        let spooled_up = one_tick(
            &law,
            &level(speed, Some(1.0)),
            &full,
            Tick(1),
            &mut RotorDrive::stopped(),
        );
        assert!(
            spooled_up.output.diagnostics.thrust_n > spooled_down.output.diagnostics.thrust_n,
            "the engine state is the throttle's path into the simulation"
        );
        for (field, down, up) in [
            (
                "commanded_rotor_radps",
                spooled_down.diagnostics.commanded_rotor_radps,
                spooled_up.diagnostics.commanded_rotor_radps,
            ),
            (
                "rotor_radps",
                spooled_down.diagnostics.rotor_radps,
                spooled_up.diagnostics.rotor_radps,
            ),
            (
                "rotor_tip_speed_mps",
                spooled_down.diagnostics.rotor_tip_speed_mps,
                spooled_up.diagnostics.rotor_tip_speed_mps,
            ),
            (
                "rotor_lift_n",
                spooled_down.diagnostics.rotor_lift_n,
                spooled_up.diagnostics.rotor_lift_n,
            ),
            (
                "rotor_drag_n",
                spooled_down.diagnostics.rotor_drag_n,
                spooled_up.diagnostics.rotor_drag_n,
            ),
            (
                "rotor_yaw_torque_nm",
                spooled_down.diagnostics.rotor_yaw_torque_nm,
                spooled_up.diagnostics.rotor_yaw_torque_nm,
            ),
            (
                "rotor_support",
                spooled_down.diagnostics.rotor_support,
                spooled_up.diagnostics.rotor_support,
            ),
            (
                "control_authority",
                spooled_down.diagnostics.control_authority,
                spooled_up.diagnostics.control_authority,
            ),
        ] {
            assert!(
                (down - up).abs() < 1e-12,
                "the engine spool must not reach {field}: {down} against {up}"
            );
        }

        // The throttle command itself changes no rotor term either, and cannot
        // start a rotor at rest.
        let throttled = one_tick(
            &law,
            &level(speed, Some(0.5)),
            &full,
            Tick(1),
            &mut RotorDrive::stopped(),
        );
        let unthrottled = one_tick(
            &law,
            &level(speed, Some(0.5)),
            &idle,
            Tick(1),
            &mut RotorDrive::stopped(),
        );
        assert_eq!(
            throttled.diagnostics.rotor_radps,
            unthrottled.diagnostics.rotor_radps
        );
        assert_eq!(
            throttled.diagnostics.rotor_lift_n,
            unthrottled.diagnostics.rotor_lift_n
        );
        assert_eq!(
            throttled.output.world_force_n[1], unthrottled.output.world_force_n[1],
            "the throttle changes no vertical force, because it changes no rotor term"
        );

        let at_rest = FlightState::at_rest(Quaternion::IDENTITY);
        let held = one_tick(&law, &at_rest, &full, Tick(1), &mut RotorDrive::stopped());
        assert_eq!(held.rotor.physical_speed_radps(), 0.0);
        assert_eq!(held.diagnostics.rotor_lift_n, 0.0);
        assert!(held.output.world_force_n[1] < 0.0);

        // Hover is a declared capability, and the only value this project
        // declares for a rotor-driven airframe is `NoHover`.
        assert!(!HoverCapability::NoHover.is_hover());
        assert_eq!(HoverCapability::NoHover.label(), "no_hover");
        let mut hovering = synthetic_exceptional_profile();
        hovering.hover = HoverCapability::Hover;
        assert_eq!(
            hovering.validate(),
            Err(ProfileError::HoverNotMeasured {
                declared: HoverCapability::Hover
            })
        );
        assert_eq!(
            ExceptionalControlLaw::new(synthetic_exceptional_tuning(), hovering).err(),
            Some(ExceptionalLawError::Profile(
                ProfileError::HoverNotMeasured {
                    declared: HoverCapability::Hover
                }
            )),
            "a law that claims hover is refused, not flown"
        );
    }

    /// The refusals are named. A fixed-wing tuning, a corrupt profile, a
    /// repeated tick, a zero-length tick, a corrupt command and an unusable
    /// environment are each refused by name, and a refused tick leaves the
    /// rotor exactly where it was.
    #[test]
    fn accept_f25_b_the_law_refuses_a_fixed_wing_tuning_and_a_corrupt_profile() {
        assert_eq!(
            ExceptionalControlLaw::new(synthetic_fixed_wing(), synthetic_exceptional_profile())
                .err(),
            Some(ExceptionalLawError::NotAnExceptionalAirframe {
                declared: ModelKind::FixedWing
            })
        );

        let corrupt = |edit: &dyn Fn(&mut ExceptionalProfile)| {
            let mut profile = synthetic_exceptional_profile();
            edit(&mut profile);
            profile
        };
        assert_eq!(
            corrupt(&|profile| profile.rotor_radius_m = 0.0).validate(),
            Err(ProfileError::NonPositive {
                field: "profile.rotor_radius_m"
            })
        );
        assert_eq!(
            corrupt(&|profile| profile.rotor_drive_radps_per_mps = f64::NAN).validate(),
            Err(ProfileError::NonFinite {
                field: "profile.rotor_drive_radps_per_mps"
            })
        );
        assert_eq!(
            corrupt(&|profile| profile.rotor_drag_n_per_tip_air = -1.0).validate(),
            Err(ProfileError::Negative {
                field: "profile.rotor_drag_n_per_tip_air"
            })
        );
        assert_eq!(
            corrupt(&|profile| profile.control_tip_speed_zero_mps = 30.0).validate(),
            Err(ProfileError::InvertedControlBand {
                zero_mps: 30.0,
                full_mps: 24.0
            })
        );
        assert_eq!(
            corrupt(&|profile| {
                profile.airframe_id =
                    ContentId::from_source(ContentKind::Mesh, "fixture.synthetic-autogyro")
                        .expect("a valid id");
            })
            .validate(),
            Err(ProfileError::NotAnAirframe {
                kind: ContentKind::Mesh
            })
        );
        let claimed = corrupt(&|profile| {
            profile.origin = Origin::Installation {
                source: synthetic_span(),
            };
        });
        assert_eq!(
            claimed.validate(),
            Err(ProfileError::OriginalOriginWithoutObservation {
                class: ClaimStatus::Designed
            }),
            "an installation origin with a designed claim asserts a measurement nobody made"
        );
        assert!(!claimed.is_measured());

        // A profile that *is* backed by an observation passes its boundary, and
        // only then reports itself measured.
        let mut observed = claimed.clone();
        observed.provenance = Provenance::new(
            cs_types::evidence::ClaimId::new("f25b.test.observed").expect("a valid claim id"),
            ClaimStatus::VerifiedOriginal,
            Some(synthetic_span()),
        )
        .expect("a located verified_original provenance is accepted");
        assert_eq!(observed.validate(), Ok(()));
        assert!(observed.is_measured());
        assert!(!synthetic_exceptional_profile().is_measured());

        // A repeated tick is refused by the rotor drive and changes nothing.
        let law = law();
        let state = level(15.0, Some(0.5));
        let input = stick(0.0, 0.0, 0.0, 0.5);
        let mut rotor = RotorDrive::stopped();
        one_tick(&law, &state, &input, Tick(5), &mut rotor);
        let after_first = rotor.physical_speed_radps();
        assert_eq!(
            law.compute(
                &FlightEnvironment::SEA_LEVEL,
                &LoadoutMass::EMPTY,
                &DamageState::PRISTINE,
                &state,
                &input,
                SYNTHETIC_TICK_DT_S,
                Tick(5),
                &mut rotor,
            )
            .err(),
            Some(ExceptionalLawError::Rotor(
                TelemetryError::NonMonotonicTick {
                    last: Tick(5),
                    got: Tick(5)
                }
            ))
        );
        assert_eq!(rotor.physical_speed_radps(), after_first);

        assert_eq!(
            law.compute(
                &FlightEnvironment::SEA_LEVEL,
                &LoadoutMass::EMPTY,
                &DamageState::PRISTINE,
                &state,
                &input,
                0.0,
                Tick(6),
                &mut rotor,
            )
            .err(),
            Some(ExceptionalLawError::Rotor(TelemetryError::NonPositive {
                field: "rotor.tick_dt_s"
            }))
        );

        let mut hot_throttle = FlightInput::NEUTRAL;
        hot_throttle.throttle = 1.5;
        assert_eq!(
            law.compute(
                &FlightEnvironment::SEA_LEVEL,
                &LoadoutMass::EMPTY,
                &DamageState::PRISTINE,
                &state,
                &hot_throttle,
                SYNTHETIC_TICK_DT_S,
                Tick(6),
                &mut rotor,
            )
            .err(),
            Some(ExceptionalLawError::Flight(FlightError::OutOfRange {
                field: "input.throttle",
                value: 1.5,
                min: 0.0,
                max: 1.0
            }))
        );

        let mut thin_air = FlightEnvironment::SEA_LEVEL;
        thin_air.air_density_kg_m3 = 0.0;
        assert_eq!(
            law.compute(
                &thin_air,
                &LoadoutMass::EMPTY,
                &DamageState::PRISTINE,
                &state,
                &input,
                SYNTHETIC_TICK_DT_S,
                Tick(6),
                &mut rotor,
            )
            .err(),
            Some(ExceptionalLawError::Flight(FlightError::Environment(
                "environment.air_density_kg_m3"
            )))
        );
    }

    /// The rotor's torque terms are separate, bounded and physically coupled:
    /// precession turns a body pitch rate into a roll torque and a body roll
    /// rate into a pitch torque, the rotor drags the airframe in yaw, and at an
    /// airspeed where both laws reach full authority the two agree on the sign
    /// convention of every control axis.
    #[test]
    fn accept_f25_b_rotor_torque_is_bounded_and_couples_attitude() {
        let law = law();
        let profile = law.profile().clone();
        let speed = 40.0;
        let state = FlightState {
            angular_velocity_radps: [0.3, 0.0, -0.2],
            ..level(speed, Some(0.8))
        };
        let input = stick(0.4, -0.6, 0.8, 0.8);
        let (mut rotor, next_tick) = settled_rotor(&law, speed);
        let computed = one_tick(&law, &state, &input, Tick(next_tick), &mut rotor);
        computed.validate().expect("the tick is finite");

        let max_torque = law.tuning().angular.max_torque_nm;
        for (axis, limit) in max_torque.iter().enumerate() {
            assert!(
                computed.diagnostics.control_axis_torque_nm[axis].abs() <= limit + 1e-9,
                "axis {axis} torque is bounded by the tuning"
            );
        }

        // The gyroscopic term is `ω × L` for a symmetric rotor.
        let arm = profile.rotor_polar_inertia_kg_m2 * computed.diagnostics.rotor_radps;
        assert!(arm > 0.0);
        assert!(
            (computed.diagnostics.precession_torque_nm[0] + arm * 0.3).abs() < 1e-6,
            "a body pitch rate precesses into a roll torque"
        );
        assert!(
            (computed.diagnostics.precession_torque_nm[1] - arm * 0.2).abs() < 1e-6,
            "a body roll rate precesses into a pitch torque"
        );
        assert_eq!(computed.diagnostics.precession_torque_nm[2], 0.0);
        assert!(
            (computed.diagnostics.rotor_yaw_torque_nm
                - profile.rotor_yaw_nm_per_radps * computed.diagnostics.rotor_radps)
                .abs()
                < 1e-6
        );
        // The full-stick case saturates the yaw axis, which is what the bound is
        // for: the rate command alone already asks for more than the tuning
        // allows.
        let rate_command = computed.diagnostics.rate_command_torque_nm;
        let precession = computed.diagnostics.precession_torque_nm;
        let applied = computed.diagnostics.control_axis_torque_nm;
        assert!(
            rate_command[2] + precession[2] + computed.diagnostics.rotor_yaw_torque_nm
                > max_torque[2],
            "the probe is one where the yaw axis saturates"
        );
        assert!(
            (applied[2] - max_torque[2]).abs() < 1e-6,
            "a saturated axis is clamped to the tuning's maximum, not passed on"
        );
        assert!(rate_command[0] < max_torque[0] + 1e-9);
        // With no airflow the rotor never starts, so there is no gyroscopic
        // term and no yaw reaction at all — only the wing's rate command.
        let mut stopped = RotorDrive::stopped();
        let no_rotor = one_tick(
            &law,
            &level(0.0, Some(0.8)),
            &input,
            Tick(next_tick),
            &mut stopped,
        );
        assert_eq!(no_rotor.rotor.physical_speed_radps(), 0.0);
        assert_eq!(no_rotor.diagnostics.precession_torque_nm, [0.0, 0.0, 0.0]);
        assert_eq!(no_rotor.diagnostics.rotor_yaw_torque_nm, 0.0);

        // The control-axis order maps onto the body components both laws share.
        let body = computed.diagnostics.body_torque_nm();
        assert!((body[0] - computed.diagnostics.control_axis_torque_nm[1]).abs() < 1e-12);
        assert!((body[1] - computed.diagnostics.control_axis_torque_nm[2]).abs() < 1e-12);
        assert!((body[2] + computed.diagnostics.control_axis_torque_nm[0]).abs() < 1e-12);

        // At 40 m/s the rotor's support band is saturated, so both laws command
        // full authority and their rate-command parts must agree in sign and
        // size on the roll and pitch axes.
        let quiet = level(speed, Some(0.8));
        let roll_only = stick(0.0, 1.0, 0.0, 0.0);
        let (mut quiet_rotor, quiet_tick) = settled_rotor(&law, speed);
        let exceptional = one_tick(&law, &quiet, &roll_only, Tick(quiet_tick), &mut quiet_rotor);
        let fixed = wing_model()
            .compute(
                &FlightEnvironment::SEA_LEVEL,
                &LoadoutMass::EMPTY,
                &DamageState::PRISTINE,
                &quiet,
                &roll_only,
                SYNTHETIC_TICK_DT_S,
            )
            .expect("the fixed-wing tick is legal");
        assert!((exceptional.diagnostics.control_authority - 1.0).abs() < 1e-12);
        assert!((fixed.diagnostics.control_authority - 1.0).abs() < 1e-12);
        let exceptional_body = exceptional.diagnostics.body_torque_nm();
        let fixed_body = into_body(fixed.world_torque_nm, quiet.orientation);
        assert!(
            (exceptional_body[0] - fixed_body[0]).abs() < 1e-6,
            "both laws pitch the same way"
        );
        assert!(
            (exceptional_body[2] - fixed_body[2]).abs() < 1e-6,
            "both laws roll the same way"
        );
        assert!(
            exceptional_body[2] < 0.0,
            "a positive roll command is right-wing-down, a negative torque about body +Z"
        );
        assert!(
            exceptional.diagnostics.rotor_yaw_torque_nm > 0.0,
            "a spinning rotor drags the airframe in the declared yaw direction"
        );

        // With no stick on the yaw axis and no body rate there, the applied yaw
        // torque *is* the rotor's reaction: the term reaches the integrator
        // rather than only the diagnostics.
        let applied = exceptional.diagnostics.control_axis_torque_nm;
        let rate_command = exceptional.diagnostics.rate_command_torque_nm;
        let precession = exceptional.diagnostics.precession_torque_nm;
        let reaction = exceptional.diagnostics.rotor_yaw_torque_nm;
        assert!(reaction > 0.0);
        assert!(reaction < max_torque[2]);
        assert!((rate_command[2] - 0.0).abs() < 1e-12);
        assert!((precession[2] - 0.0).abs() < 1e-12);
        assert!(
            (applied[2] - reaction).abs() < 1e-6,
            "the applied yaw torque is the rotor's reaction"
        );

        // The yaw axis needs its own cross-law probe, because the roll-only case
        // above leaves it at zero and therefore cannot tell a correct yaw sign
        // from an inverted one. Here the stick and the body rate are on the yaw
        // axis, so both laws command a real yaw torque and the *difference*
        // between them must be exactly the rotor's declared reaction: same sign
        // as a positive yaw command, and no more.
        let yawing = FlightState {
            angular_velocity_radps: [0.0, 0.2, 0.0],
            ..level(speed, Some(0.8))
        };
        let yaw_only = stick(0.0, 0.0, 0.5, 0.0);
        let (mut yaw_rotor, yaw_tick) = settled_rotor(&law, speed);
        let yaw_exceptional = one_tick(&law, &yawing, &yaw_only, Tick(yaw_tick), &mut yaw_rotor);
        let yaw_fixed = wing_model()
            .compute(
                &FlightEnvironment::SEA_LEVEL,
                &LoadoutMass::EMPTY,
                &DamageState::PRISTINE,
                &yawing,
                &yaw_only,
                SYNTHETIC_TICK_DT_S,
            )
            .expect("the fixed-wing tick is legal");
        assert!((yaw_exceptional.diagnostics.control_authority - 1.0).abs() < 1e-12);
        assert!((yaw_fixed.diagnostics.control_authority - 1.0).abs() < 1e-12);
        let yaw_body = yaw_exceptional.diagnostics.body_torque_nm();
        let yaw_reference = into_body(yaw_fixed.world_torque_nm, yawing.orientation);
        // Roll and pitch carry no rotor term here, so they must agree exactly.
        assert!((yaw_body[0] - yaw_reference[0]).abs() < 1e-6);
        assert!((yaw_body[2] - yaw_reference[2]).abs() < 1e-6);
        let yaw_reaction = yaw_exceptional.diagnostics.rotor_yaw_torque_nm;
        assert!(
            yaw_reaction > 0.0 && yaw_reaction < max_torque[2],
            "the probe is one where the reaction is real and the yaw axis does not saturate"
        );
        assert!(
            (yaw_body[1] - yaw_reference[1] - yaw_reaction).abs() < 1e-6,
            "a positive yaw command turns the same way under both laws, plus the rotor's reaction"
        );
        assert!(
            yaw_reference[1] > 0.0,
            "a positive yaw command is a positive torque about the body up axis"
        );
    }

    /// AC02's minimum scenario from the producing side: a tick the law could
    /// have got wrong is **refused**, not handed on. A non-finite value is named,
    /// and a force the law produced without recording where it came from is
    /// caught by the accounting check, so neither can reach an integrator.
    #[test]
    fn accept_f25_b_a_doctored_or_nonfinite_tick_is_refused_by_name() {
        let law = law();
        let state = level(12.0, Some(0.6));
        let input = stick(0.2, 0.2, 0.0, 0.6);
        let mut rotor = RotorDrive::stopped();
        let good = one_tick(&law, &state, &input, Tick(1), &mut rotor);
        assert_eq!(good.validate(), Ok(()));

        type Doctor = Box<dyn Fn(ExceptionalTick) -> ExceptionalTick>;
        let doctors: [(&str, Doctor); 4] = [
            (
                "world_force_n[0]",
                Box::new(|mut tick: ExceptionalTick| {
                    tick.output.world_force_n[0] = f64::NAN;
                    tick
                }),
            ),
            (
                "world_torque_nm[2]",
                Box::new(|mut tick: ExceptionalTick| {
                    tick.output.world_torque_nm[2] = f64::INFINITY;
                    tick
                }),
            ),
            (
                "rotor_lift_n",
                Box::new(|mut tick: ExceptionalTick| {
                    tick.diagnostics.rotor_lift_n = f64::NAN;
                    tick
                }),
            ),
            (
                "control_authority",
                Box::new(|mut tick: ExceptionalTick| {
                    tick.diagnostics.control_authority = f64::NAN;
                    tick
                }),
            ),
        ];
        for (field, doctor) in doctors {
            assert_eq!(
                doctor(good).validate().err(),
                Some(ExceptionalLawError::NonFiniteOutput { field }),
                "a non-finite {field} must be refused by name"
            );
        }

        // A force the law produced without recording a contribution for is
        // caught by the accounting check rather than integrated silently.
        let mut unaccounted = good;
        unaccounted.output.world_force_n[1] += 1.0;
        match unaccounted.validate() {
            Err(ExceptionalLawError::UnaccountedForce { residual_n }) => {
                assert!(
                    (residual_n - 1.0).abs() < 1e-9,
                    "the residual is the unaccounted component, got {residual_n}"
                );
            }
            other => panic!("an unaccounted force must be refused, got {other:?}"),
        }
        // And a recorded contribution the force does not contain is caught the
        // same way.
        let mut phantom = good;
        phantom.diagnostics.rotor_lift_force_n[0] += 25.0;
        assert!(matches!(
            phantom.validate(),
            Err(ExceptionalLawError::UnaccountedForce { .. })
        ));
    }

    /// The two declared bounds the *authority* law carries, and the lift cap it
    /// cannot be removed from, are all reached by the applied values rather than
    /// only being present in the profile: a stalled airframe loses authority in
    /// proportion to the wing's stall factor, a damaged one in proportion to the
    /// damage, and rotor lift saturates at the declared cap instead of growing
    /// without limit.
    #[test]
    fn accept_f25_b_stall_damage_and_the_lift_cap_reach_the_applied_values() {
        let law = law();
        let profile = law.profile().clone();
        let speed = 20.0;
        let (rotor, next_tick) = settled_rotor(&law, speed);
        let input = stick(0.0, 0.0, 0.5, 0.5);

        // The declared band and the wing ramp are both saturated at this
        // airspeed, so the authority is exactly `stall · damage`.
        let fast = level(speed, Some(0.5));
        let mut clean_rotor = rotor;
        let clean = one_tick(&law, &fast, &input, Tick(next_tick), &mut clean_rotor);
        assert!((clean.diagnostics.rotor_support - 1.0).abs() < 1e-12);
        // The wing ramp is only half open at 20 m/s, so this airspeed is one
        // where the rotor's band alone carries the authority: the product below
        // is `stall · damage` with nothing else in it.
        assert!((clean.diagnostics.wing_authority - 0.5).abs() < 1e-12);
        assert!((clean.diagnostics.control_authority - 1.0).abs() < 1e-12);

        // Damage scales the authority the law actually applied.
        let wounded = DamageState {
            control_authority: 0.4,
            ..DamageState::PRISTINE
        };
        let mut damaged_rotor = rotor;
        let damaged = law
            .compute(
                &FlightEnvironment::SEA_LEVEL,
                &LoadoutMass::EMPTY,
                &wounded,
                &fast,
                &input,
                SYNTHETIC_TICK_DT_S,
                Tick(next_tick),
                &mut damaged_rotor,
            )
            .expect("a damaged tick is still a legal tick");
        assert!((damaged.diagnostics.control_authority - 0.4).abs() < 1e-12);
        let applied_damaged = damaged.diagnostics.rate_command_torque_nm[2].abs()
            / clean.diagnostics.rate_command_torque_nm[2].abs();
        assert!(
            (applied_damaged - 0.4).abs() < 1e-9,
            "the applied yaw rate command must fall with the damage, got {applied_damaged}"
        );

        // A stalled airframe loses authority in proportion to the wing's stall
        // factor, which is the declared simplification the finding records: a
        // spinning rotor is not treated as keeping authority in a stalled wing.
        let stalled = FlightState {
            linear_velocity_mps: [0.0, -speed * 0.7, -speed * 0.7],
            ..level(speed, Some(0.5))
        };
        let mut stalled_rotor = rotor;
        let stalled_tick = one_tick(&law, &stalled, &input, Tick(next_tick), &mut stalled_rotor);
        let stall_scale = stalled_tick.output.instrument_state.stall_scale;
        assert!(
            stall_scale < 0.9,
            "the probe is genuinely stalled, got a stall scale of {stall_scale}"
        );
        assert!(
            (stalled_tick.diagnostics.control_authority - stall_scale).abs() < 1e-12,
            "the authority must be the wing's stall factor times the damage, got {} against {stall_scale}",
            stalled_tick.diagnostics.control_authority
        );

        // Rotor lift saturates at the declared cap. The profile's uncapped gain
        // would give far more than the cap at this airspeed, so the bound is
        // what the applied force shows.
        let fast_speed = 45.0;
        let (fast_rotor, fast_tick) = settled_rotor(&law, fast_speed);
        let mut capped_rotor = fast_rotor;
        let capped = one_tick(
            &law,
            &level(fast_speed, Some(1.0)),
            &input,
            Tick(fast_tick),
            &mut capped_rotor,
        );
        let uncapped = profile.rotor_lift_n_per_mps_tip * capped.diagnostics.rotor_tip_speed_mps;
        assert!(
            uncapped > profile.rotor_lift_max_n,
            "the probe is one where the cap actually binds, got {uncapped} N uncapped"
        );
        assert!((capped.diagnostics.rotor_lift_n - profile.rotor_lift_max_n).abs() < 1e-9);
        // And the recorded force agrees with the capped magnitude, so the cap is
        // a bound on the force and not only on the diagnostic.
        let up_world = rotated(BODY_UP, Quaternion::IDENTITY);
        for (axis, value) in capped.diagnostics.rotor_lift_force_n.iter().enumerate() {
            assert!(
                (value - capped.diagnostics.rotor_lift_n * up_world[axis]).abs() < 1e-9,
                "the capped lift force must be the capped magnitude along the shaft axis"
            );
        }
    }

    /// A refused tick leaves the caller's rotor **exactly** where it was, for
    /// every refusal path including the produced-tick check — the advance is
    /// committed only after the tick validates, so a caller that retries cannot
    /// integrate the rotor twice.
    #[test]
    fn accept_f25_b_a_refused_tick_leaves_the_caller_s_rotor_untouched() {
        let law = law();
        let state = level(20.0, Some(0.6));
        let input = stick(0.0, 0.0, 0.4, 0.6);

        // A refusal that happens before the advance: a stale tick.
        let mut rotor = RotorDrive::stopped();
        one_tick(&law, &state, &input, Tick(5), &mut rotor);
        let after_first = rotor;
        assert!(
            law.compute(
                &FlightEnvironment::SEA_LEVEL,
                &LoadoutMass::EMPTY,
                &DamageState::PRISTINE,
                &state,
                &input,
                SYNTHETIC_TICK_DT_S,
                Tick(5),
                &mut rotor,
            )
            .is_err()
        );
        assert_eq!(
            rotor, after_first,
            "a stale-tick refusal must not move the rotor"
        );

        // A refusal that happens *after* the advance would be computed: the
        // produced-tick check. Reaching it needs a profile whose declared fields
        // are all finite and inside their bounds, so the profile boundary passes,
        // but whose rotor drag overflows to infinity once multiplied by the tip
        // and air speeds. Rotor lift cannot be used for this: it is clamped to a
        // finite declared cap, so it saturates instead of overflowing.
        let mut hot = synthetic_exceptional_profile();
        hot.rotor_drag_n_per_tip_air = f64::MAX;
        let hot_law = ExceptionalControlLaw::new(synthetic_exceptional_tuning(), hot)
            .expect("the overflowing profile still passes its own boundary");
        hot_law
            .profile()
            .validate()
            .expect("and validates on its own");
        // A fast enough pass that the drag term overflows on its first tick.
        let fast_state = level(200.0, Some(0.6));
        let mut hot_rotor = RotorDrive::stopped();
        let refusal = hot_law.compute(
            &FlightEnvironment::SEA_LEVEL,
            &LoadoutMass::EMPTY,
            &DamageState::PRISTINE,
            &fast_state,
            &input,
            SYNTHETIC_TICK_DT_S,
            Tick(1),
            &mut hot_rotor,
        );
        let refusal = refusal.expect_err("an overflowing rotor drag must be refused");
        assert!(
            matches!(
                refusal,
                ExceptionalLawError::NonFiniteOutput { .. }
                    | ExceptionalLawError::UnaccountedForce { .. }
            ),
            "the produced-tick check is what refuses it, got {refusal:?}"
        );
        assert_eq!(
            hot_rotor,
            RotorDrive::stopped(),
            "a produced-tick refusal must not commit the rotor advance either"
        );
    }
}
