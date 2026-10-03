//! The per-tick state a replay session measures (F59-B).
//!
//! Spec: `specs/F59-replays-captures-probes-and-acceptance-evidence.md`,
//! stage `### F59-B`. Shared contract: `docs/contracts/CLI-EVIDENCE.md`.
//!
//! F59-A defined the [`StateEnvelope`] and its chain, compare and document form,
//! and said in the module documentation that the *hashes* are produced by a
//! running session. This is that producer. It is deliberately narrow, because a
//! state hash that is not a measurement is worse than none:
//!
//! * A [`StateReading`] can only be built from a body's pose read back out of
//!   the world and the forces the tick's own law computed. It has no field a
//!   caller can fill with "what the input said": nothing here is derived from
//!   the input frame, so a hash cannot agree with the promise merely because the
//!   input was copied into it. A changed input that does not move the aircraft
//!   produces an unchanged state hash, which is the correct answer.
//! * [`StateReading::digest`] writes every float as its IEEE-754 bit pattern
//!   and every flag as a byte, under a domain separation constant, so two
//!   readings cannot collide by reordering fields or by agreeing to fewer
//!   decimal places.
//! * A reading whose state is not finite is [`refused`](StateProbe::measure)
//!   rather than hashed. `NaN` bits compare unequal to themselves in most
//!   comparisons and a hash over them is a number that means nothing.
//! * [`StateProbe`] keeps the run's initial state beside the envelope, because
//!   the chain digest starts from it: without it the promise is a list of
//!   per-tick hashes with nothing to hang from.

use std::fmt;

use cs_assets::install::Sha256;
use cs_content::replay::{
    EnvelopeError, InitialState, MAX_ENVELOPE_ENTRIES, ReplayError, StateEnvelope,
};
use cs_sim::flight::FlightOutput;
use cs_types::Tick;
use cs_types::evidence::ContentHash;

use crate::physics::PhysicsSample;

/// The domain separation constant of [`StateReading::digest`].
///
/// Separate from every other digest in this crate, so a state reading cannot
/// collide with a content digest, a rules digest, a compatibility signature or
/// a capture digest by feeding one the other's bytes.
pub const STATE_DIGEST_DOMAIN: &[u8] = b"cs.f59.state.reading.v1";

/// One measured state reading: everything a replay has to reproduce at one tick.
///
/// Both halves are read out of the world rather than supplied: `pose` is the
/// authoritative Avian read-back and `output` is what the tick's own flight law
/// computed. [`at_spawn`](Self::at_spawn) is the one reading without an output,
/// because no tick has run yet.
#[derive(Clone, Debug, PartialEq)]
pub struct StateReading {
    /// The tick the reading describes.
    pub tick: Tick,
    /// The body's authoritative pose and velocities.
    pub pose: PhysicsSample,
    /// The tick's computed forces and instruments; `None` before the first
    /// driven tick.
    pub output: Option<FlightOutput>,
}

impl StateReading {
    /// The state the run started from, measured before its first tick ran.
    #[must_use]
    pub const fn at_spawn(pose: PhysicsSample) -> Self {
        Self {
            tick: Tick(0),
            pose,
            output: None,
        }
    }

    /// The state after one driven tick.
    #[must_use]
    pub const fn after_tick(tick: Tick, pose: PhysicsSample, output: FlightOutput) -> Self {
        Self {
            tick,
            pose,
            output: Some(output),
        }
    }

    /// The digest of this reading.
    ///
    /// Domain-separated from every other digest in this crate, length-prefixed
    /// per field and bit-exact per float, so it is a statement about these
    /// numbers rather than about their rounded spelling.
    #[must_use]
    pub fn digest(&self) -> ContentHash {
        let mut hasher = Sha256::new();
        hasher.update(STATE_DIGEST_DOMAIN);
        hasher.update(&self.tick.0.to_be_bytes());
        for value in self.pose.position_m {
            hasher.update(&value.to_bits().to_be_bytes());
        }
        for value in self.pose.linear_velocity_m_s {
            hasher.update(&value.to_bits().to_be_bytes());
        }
        for value in self.pose.angular_velocity_rad_s {
            hasher.update(&value.to_bits().to_be_bytes());
        }
        match &self.output {
            None => hasher.update(&[0u8]),
            Some(output) => {
                hasher.update(&[1u8]);
                for value in output.world_force_n {
                    hasher.update(&value.to_bits().to_be_bytes());
                }
                for value in output.world_torque_nm {
                    hasher.update(&value.to_bits().to_be_bytes());
                }
                hasher.update(&output.accepted_boost_consumption.to_bits().to_be_bytes());
                let instruments = &output.instrument_state;
                for value in [
                    instruments.airspeed_mps,
                    instruments.angle_of_attack_rad,
                    instruments.sideslip_rad,
                    instruments.dynamic_pressure_pa,
                    instruments.lift_coefficient,
                    instruments.drag_coefficient,
                    instruments.stall_scale,
                    instruments.thrust_n,
                ] {
                    hasher.update(&value.to_bits().to_be_bytes());
                }
                let sources = &output.diagnostics;
                hasher.update(&sources.thrust_n.to_bits().to_be_bytes());
                hasher.update(&sources.lift_n.to_bits().to_be_bytes());
                hasher.update(&sources.drag_n.to_bits().to_be_bytes());
                for value in sources.gravity_force_n {
                    hasher.update(&value.to_bits().to_be_bytes());
                }
                for value in sources.assist_force_n {
                    hasher.update(&value.to_bits().to_be_bytes());
                }
                for value in sources.assist_torque_nm {
                    hasher.update(&value.to_bits().to_be_bytes());
                }
                hasher.update(&sources.control_authority.to_bits().to_be_bytes());
            }
        }
        hasher.finalize()
    }

    /// The first field of the reading that is not finite, with its name.
    #[must_use]
    pub fn non_finite_field(&self) -> Option<&'static str> {
        for (field, values) in [
            ("position_m", self.pose.position_m),
            ("linear_velocity_m_s", self.pose.linear_velocity_m_s),
            ("angular_velocity_rad_s", self.pose.angular_velocity_rad_s),
        ] {
            if values.iter().any(|value| !value.is_finite()) {
                return Some(field);
            }
        }
        let Some(output) = &self.output else {
            return None;
        };
        for (field, value) in [
            ("world_force_n", output.world_force_n),
            ("world_torque_nm", output.world_torque_nm),
        ] {
            if value.iter().any(|value| !value.is_finite()) {
                return Some(field);
            }
        }
        if !output.accepted_boost_consumption.is_finite() {
            return Some("accepted_boost_consumption");
        }
        let instruments = &output.instrument_state;
        for (field, value) in [
            ("airspeed_mps", instruments.airspeed_mps),
            ("angle_of_attack_rad", instruments.angle_of_attack_rad),
            ("sideslip_rad", instruments.sideslip_rad),
            ("dynamic_pressure_pa", instruments.dynamic_pressure_pa),
            ("lift_coefficient", instruments.lift_coefficient),
            ("drag_coefficient", instruments.drag_coefficient),
            ("stall_scale", instruments.stall_scale),
            ("thrust_n", instruments.thrust_n),
        ] {
            if !value.is_finite() {
                return Some(field);
            }
        }
        let sources = &output.diagnostics;
        for (field, value) in [
            ("thrust_n", sources.thrust_n),
            ("lift_n", sources.lift_n),
            ("drag_n", sources.drag_n),
            ("control_authority", sources.control_authority),
        ] {
            if !value.is_finite() {
                return Some(field);
            }
        }
        for (field, values) in [
            ("gravity_force_n", sources.gravity_force_n),
            ("assist_force_n", sources.assist_force_n),
            ("assist_torque_nm", sources.assist_torque_nm),
        ] {
            if values.iter().any(|value| !value.is_finite()) {
                return Some(field);
            }
        }
        None
    }
}

/// Why a state probe refused a reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateProbeError {
    /// A tick was measured before the run's initial state was recorded. The
    /// chain digest starts from that state, so an envelope without it cannot be
    /// compared against anything.
    NotStarted,
    /// The run's initial state was recorded twice, at two different readings.
    AlreadyStarted,
    /// A reading's state was NaN or infinite. Hashing it would produce a number
    /// that does not describe any state.
    NonFinite {
        /// The tick the reading was stamped for.
        tick: Tick,
        /// The offending field.
        field: &'static str,
    },
    /// The reading's tick did not follow the last measured tick.
    TickOrder(EnvelopeError),
    /// The envelope is full.
    TooManyEntries {
        /// The declared bound.
        max: usize,
    },
}

impl fmt::Display for StateProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotStarted => write!(
                f,
                "the run's initial state must be measured before its first tick"
            ),
            Self::AlreadyStarted => write!(f, "the run's initial state was already measured"),
            Self::NonFinite { tick, field } => {
                write!(f, "state read at tick {} has a non-finite {field}", tick.0)
            }
            Self::TickOrder(error) => write!(f, "{error}"),
            Self::TooManyEntries { max } => {
                write!(f, "a state envelope holds at most {max} ticks")
            }
        }
    }
}

impl std::error::Error for StateProbeError {}

impl From<EnvelopeError> for StateProbeError {
    fn from(error: EnvelopeError) -> Self {
        match error {
            EnvelopeError::NonIncreasingTick { .. } => Self::TickOrder(error),
            EnvelopeError::TooManyEntries { max } => Self::TooManyEntries { max },
        }
    }
}

/// The initial state and the per-tick state hashes one measured run produced.
///
/// This is the only producer of [`StateEnvelope`] values in the runtime: the
/// F59-A fixture builds one from labelled fixture descriptions, and every other
/// envelope in the crate arrives through [`measure`](Self::measure).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StateProbe {
    initial: Option<ContentHash>,
    envelope: StateEnvelope,
}

impl StateProbe {
    /// A probe with nothing measured yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Measures the state the run started from.
    ///
    /// # Errors
    ///
    /// [`StateProbeError::AlreadyStarted`] for a second call, and
    /// [`StateProbeError::NonFinite`] for a reading that is not a real state.
    pub fn start(&mut self, reading: &StateReading) -> Result<(), StateProbeError> {
        if self.initial.is_some() {
            return Err(StateProbeError::AlreadyStarted);
        }
        if let Some(field) = reading.non_finite_field() {
            return Err(StateProbeError::NonFinite {
                tick: reading.tick,
                field,
            });
        }
        self.initial = Some(reading.digest());
        Ok(())
    }

    /// Measures one tick's state and records its promised hash.
    ///
    /// # Errors
    ///
    /// [`StateProbeError::NotStarted`] before [`start`](Self::start),
    /// [`StateProbeError::NonFinite`] for a reading that is not a real state, and
    /// the tick-order and size refusals of [`StateEnvelope::push`]. A refused
    /// reading changes nothing.
    pub fn measure(&mut self, reading: &StateReading) -> Result<(), StateProbeError> {
        if self.initial.is_none() {
            return Err(StateProbeError::NotStarted);
        }
        if let Some(field) = reading.non_finite_field() {
            return Err(StateProbeError::NonFinite {
                tick: reading.tick,
                field,
            });
        }
        if self.envelope.len() >= MAX_ENVELOPE_ENTRIES {
            return Err(StateProbeError::TooManyEntries {
                max: MAX_ENVELOPE_ENTRIES,
            });
        }
        self.envelope
            .push(reading.tick, reading.digest())
            .map_err(StateProbeError::from)
    }

    /// The digest of the state the run started from.
    #[must_use]
    pub fn initial(&self) -> Option<ContentHash> {
        self.initial
    }

    /// The measured per-tick hashes.
    #[must_use]
    pub fn envelope(&self) -> &StateEnvelope {
        &self.envelope
    }

    /// The last tick measured.
    #[must_use]
    pub fn last_tick(&self) -> Option<Tick> {
        self.envelope.last_tick()
    }

    /// Whether the run measured no tick at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.envelope.is_empty()
    }

    /// The initial state as the record's [`InitialState`], labelled with
    /// `label`.
    ///
    /// # Errors
    ///
    /// [`ReplayError::Blank`] when the probe never started, and every error of
    /// [`InitialState::new`] for a blank or multi-line label.
    pub fn initial_state(&self, label: &str) -> Result<InitialState, ReplayError> {
        let digest = self.initial.ok_or(ReplayError::MissingInitialState)?;
        InitialState::new(label, digest)
    }
}
