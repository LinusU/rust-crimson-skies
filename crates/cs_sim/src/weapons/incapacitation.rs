//! Pilot incapacitation: what a `SONIC`/`FLASH` hit does to the actor it
//! strikes (task #2172, `T452-R05-STATUS-KIND`).
//!
//! The values are the static measurements of
//! `docs/findings/2026-10-28-t452-r05-slot-map-and-incapacitation.md` (image
//! base `0x400000`; none is `verified_original`):
//!
//! * the exposure routine `0x42e840` returns nothing for a victim whose
//!   `+0x91d` byte is set;
//! * `ratio = squared distance / IMPACT_PROXIMITY²`, and
//!   `intensity = clamp(1 - 2.5 * (ratio - 0.6), 0, 1)`;
//! * `FLASH` additionally scales the intensity by
//!   `clamp(2 * facing_dot, 0, 1)`, where `facing_dot` is the dot product of
//!   the victim's forward vector with the victim-minus-source offset;
//! * the duration is `intensity * 5.0` seconds;
//! * the player gets a screen/audio exposure and **no** control loss;
//! * any other actor passing the `0x4200d0` gate (not the player, `+0x91d`
//!   and `+0xf8` clear, `+0x67c` in `{0, 4}`) gets mode `4`, six zeroed
//!   control words and a deadline of clock + duration; mode `4` is cleared
//!   once the clock has reached the deadline (`0x489acd`..`0x489ae9`).
//!
//! The simulation expresses the mode and the deadline as one
//! [`StatusEffectKind::Incapacitation`] entry in the session's
//! [`StatusEffectLedger`]: the deadline is a whole-tick expiry, so the mode
//! clears on the first tick at or after `now + duration`, and nothing here
//! reads a wall clock. This module carries no renderer or audio dependency;
//! the player's screen and sound are reported as an [`Exposure`] for the
//! presentation layer to consume.

use cs_types::Tick;
use cs_types::net::SessionId;

use crate::damage::ActorId;
use crate::time::TickRate;
use crate::weapons::ordnance::{
    OrdnanceDefinitionError, OrdnanceId, OrdnanceStatusEffect, StatusEffectError,
    StatusEffectInstanceId, StatusEffectKind, StatusEffectLedger, StatusEffectTarget,
};

/// Ratio at or below which the intensity is full (`0x60411c`).
pub const FULL_INTENSITY_RATIO: f64 = 0.6;
/// Slope of the intensity falloff (`0x6040a8`).
pub const FALLOFF_SLOPE: f64 = 2.5;
/// Seconds of incapacitation at full intensity (`0x6036bc`).
pub const MAX_DURATION_S: f64 = 5.0;
/// Scale of the `FLASH` facing term (`2 * dot`, clamped).
pub const FLASH_FACING_SCALE: f64 = 2.0;
/// The mode word `+0x358` an incapacitated actor holds.
pub const INCAPACITATED_MODE: u32 = 4;
/// The number of control words the original zeroes.
pub const CONTROL_WORD_COUNT: usize = 6;

/// Which effect bit struck.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExposureBit {
    /// `SONIC`: range falloff only.
    Sonic,
    /// `FLASH`: range falloff scaled by the victim's facing.
    Flash,
}

/// Where the victim was when the shot struck.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExposureGeometry {
    /// Squared distance from the detonation to the victim, m².
    pub distance_squared_m2: f64,
    /// The record's `IMPACT_PROXIMITY`, metres (unsquared).
    pub impact_proximity_m: f64,
    /// `dot(victim_forward, victim_pos - source_pos)`; read only by `FLASH`.
    pub facing_dot: f64,
}

/// The measured exposure: a raw intensity and the duration it implies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Exposure {
    /// Intensity in `[0, 1]`.
    pub intensity: f64,
    /// `intensity * 5.0`, seconds.
    pub duration_s: f64,
}

/// Why an exposure could not be measured.
#[derive(Clone, Debug, PartialEq)]
pub enum IncapacitationError {
    /// A geometry value is not finite, or the radius is not positive.
    InvalidGeometry,
    /// The ledger refused the status.
    Status(StatusEffectError),
    /// The status could not be assembled.
    Definition(OrdnanceDefinitionError),
}

impl std::fmt::Display for IncapacitationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidGeometry => {
                f.write_str("the exposure geometry must be finite with a positive radius")
            }
            Self::Status(source) => write!(f, "the status ledger refused the effect: {source}"),
            Self::Definition(source) => write!(f, "the status effect is invalid: {source}"),
        }
    }
}

impl std::error::Error for IncapacitationError {}

impl From<StatusEffectError> for IncapacitationError {
    fn from(source: StatusEffectError) -> Self {
        Self::Status(source)
    }
}

/// Measures the exposure of `bit` at `geometry` (`0x42e85b`..`0x42e9b1`).
///
/// # Errors
///
/// [`IncapacitationError::InvalidGeometry`] for a non-finite value or a
/// non-positive `impact_proximity_m`.
pub fn measure_exposure(
    bit: ExposureBit,
    geometry: &ExposureGeometry,
) -> Result<Exposure, IncapacitationError> {
    if !geometry.distance_squared_m2.is_finite()
        || geometry.distance_squared_m2 < 0.0
        || !geometry.impact_proximity_m.is_finite()
        || geometry.impact_proximity_m <= 0.0
        || !geometry.facing_dot.is_finite()
    {
        return Err(IncapacitationError::InvalidGeometry);
    }
    let ratio = geometry.distance_squared_m2 / (geometry.impact_proximity_m.powi(2));
    let mut intensity = (1.0 - FALLOFF_SLOPE * (ratio - FULL_INTENSITY_RATIO)).clamp(0.0, 1.0);
    if bit == ExposureBit::Flash {
        intensity *= (FLASH_FACING_SCALE * geometry.facing_dot).clamp(0.0, 1.0);
    }
    Ok(Exposure {
        intensity,
        duration_s: intensity * MAX_DURATION_S,
    })
}

/// The victim state the original's gate reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IncapacitationVictim {
    /// The victim is the player actor.
    pub is_player: bool,
    /// The `+0x91d` byte is set: the victim is exempt from the exposure.
    pub exempt: bool,
    /// The `+0xf8` word is non-zero.
    pub busy: bool,
    /// The `+0x67c` word.
    pub state_word: u32,
}

impl IncapacitationVictim {
    /// Whether `0x4200d0` lets the incapacitation through for a non-player.
    #[must_use]
    pub const fn admits_incapacitation(&self) -> bool {
        !self.is_player
            && !self.exempt
            && !self.busy
            && (self.state_word == 0 || self.state_word == 4)
    }
}

/// What one exposure did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExposureOutcome {
    /// The victim's `+0x91d` byte is set: nothing happens.
    Exempt,
    /// The intensity is zero: nothing happens.
    NoEffect,
    /// The player: a screen/audio exposure, and no control loss.
    PlayerScreenAudio(Exposure),
    /// A non-player that failed the `0x4200d0` gate: no incapacitation.
    Gated,
    /// The victim is incapacitated until `expires_at` (exclusive: it is
    /// recovered on that tick).
    Incapacitated {
        /// The ledger entry.
        instance: StatusEffectInstanceId,
        /// The measured exposure.
        exposure: Exposure,
        /// The whole-tick length of the status.
        ticks: u64,
        /// The first tick on which the actor is recovered.
        expires_at: Tick,
    },
}

/// Applies one `SONIC`/`FLASH` hit to a victim at `tick`.
///
/// A second hit on an already incapacitated victim replaces the first one's
/// deadline with `tick + duration`, as the original overwrites `+0xc0`.
///
/// # Errors
///
/// [`IncapacitationError`] for bad geometry or a ledger refusal.
#[allow(clippy::too_many_arguments)]
pub fn apply_exposure(
    ledger: &mut StatusEffectLedger,
    session: SessionId,
    tick: Tick,
    rate: TickRate,
    actor: ActorId,
    source: &OrdnanceId,
    bit: ExposureBit,
    geometry: &ExposureGeometry,
    victim: &IncapacitationVictim,
) -> Result<ExposureOutcome, IncapacitationError> {
    let exposure = measure_exposure(bit, geometry)?;
    if victim.exempt {
        return Ok(ExposureOutcome::Exempt);
    }
    if exposure.intensity <= 0.0 {
        return Ok(ExposureOutcome::NoEffect);
    }
    if victim.is_player {
        return Ok(ExposureOutcome::PlayerScreenAudio(exposure));
    }
    if !victim.admits_incapacitation() {
        return Ok(ExposureOutcome::Gated);
    }
    // The first tick whose time reaches `now + duration` clears the mode.
    let ticks = ((exposure.duration_s * f64::from(rate.ticks_per_second())).ceil() as u64).max(1);
    let effect =
        OrdnanceStatusEffect::try_new(StatusEffectKind::Incapacitation, ticks, exposure.intensity)
            .map_err(IncapacitationError::Definition)?;
    let target = StatusEffectTarget::whole_actor(actor);
    ledger.clear_kind(&target, StatusEffectKind::Incapacitation);
    let instance = ledger.apply(session, tick, target, source, &effect)?;
    Ok(ExposureOutcome::Incapacitated {
        instance,
        exposure,
        ticks,
        expires_at: Tick(tick.0 + ticks),
    })
}

/// The actor's mode word as the incapacitation sets it: [`INCAPACITATED_MODE`]
/// while the status is live, `None` when the status holds no mode.
#[must_use]
pub fn incapacitated_mode(ledger: &StatusEffectLedger, actor: ActorId) -> Option<u32> {
    ledger
        .is_under(
            &StatusEffectTarget::whole_actor(actor),
            StatusEffectKind::Incapacitation,
        )
        .then_some(INCAPACITATED_MODE)
}

/// The six control words an actor's update reads, after the incapacitation
/// gate: all zero while the actor is incapacitated, otherwise `requested`.
#[must_use]
pub fn effective_controls(
    ledger: &StatusEffectLedger,
    actor: ActorId,
    requested: [f64; CONTROL_WORD_COUNT],
) -> [f64; CONTROL_WORD_COUNT] {
    if incapacitated_mode(ledger, actor).is_some() {
        [0.0; CONTROL_WORD_COUNT]
    } else {
        requested
    }
}
