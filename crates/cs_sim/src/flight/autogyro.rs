//! Exceptional flight configurations: the shared telemetry interface, the
//! rotor drive and the reference maneuver envelope (F25-A).
//!
//! Spec: `specs/F25-hoplite-autogyro-and-exceptional-flight-configurations.md`,
//! stage `### F25-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`,
//! sections "Inputs and outputs", "Boost and special models" and "Calibration
//! acceptance".
//!
//! **This stage is the typed boundary, not a control law.** The sheet asks for
//! "typed inputs/outputs and a minimal synthetic fixture first; do not jump
//! ahead to a whole runtime", and this module is exactly that. It adds three
//! things the exceptional stages need and nothing else:
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
//! **No helicopter hover is invented here.** Non-negotiable behavior 1 and
//! `FLIGHT-PHYSICS` ("Do not use the word autogyro as permission to invent
//! helicopter hover") mean the exceptional *force* law is F25-B's, derived
//! from measurement; this module contributes no force, no lift curve and no
//! hover. Nothing here is an extracted original coefficient, a measured
//! autogyro handling value or a verified reference trace: the Hoplite name and
//! prefix are source-observed while the exact control law remains
//! measurement-dependent (`F25` "Research boundary"), which is why the
//! declared envelope ships unmeasured and
//! [`ReferenceManeuverEnvelope::is_ready_as_reference`] is `false`.

use cs_types::Tick;
use cs_types::content::{Origin, Provenance};
use cs_types::evidence::ClaimStatus;

use super::model::{FlightOutput, FlightState};
use super::tuning::ModelKind;

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
