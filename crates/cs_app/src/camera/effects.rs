//! The producer of cosmetic and required effects (F52-W2).
//!
//! The place that knows a hit, an impact or a weapon event happened turns it
//! into [`Effect`] requests here, and [`EffectFrame::present`] is the only way
//! they reach the screen: through [`SettingsSession::present`], never raw. An
//! event the presenter cannot draw (a non-finite or negative magnitude) is
//! returned as an [`EffectError`], not dropped. Nothing here reads or writes
//! simulation state: the events are inputs, the result is a drawing request.
//! The amplitudes are **designed**; see
//! `docs/findings/2026-10-27-f52-w2-presentation-output.md`.

use std::fmt;

use crate::accessibility::motion::Effect;
use crate::accessibility::session::SettingsSession;

/// Shake amplitude (designed, screen fraction) of a hit of unit damage.
pub const SHAKE_PER_DAMAGE: f32 = 0.02;
/// Largest shake or flash intensity requested.
pub const MAX_INTENSITY: f32 = 1.0;

/// Something that happened this frame that the player should feel or be told.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GameEvent {
    /// The player's aircraft took damage of this size.
    PlayerHit {
        /// Damage taken, in the damage model's own units.
        damage: f32,
    },
    /// A collision or explosion near the camera, of this normalised strength.
    Impact {
        /// Strength in `0.0..=1.0`.
        strength: f32,
    },
    /// The player fired a weapon that flashes the screen.
    WeaponFlash {
        /// Intensity in `0.0..=1.0`.
        intensity: f32,
    },
    /// The player's target changed.
    TargetChanged,
}

/// An event that could not be turned into a drawable effect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectError {
    /// The event.
    pub event: GameEvent,
    /// The offending magnitude.
    pub value: f32,
}

impl fmt::Display for EffectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} has an undrawable magnitude {}",
            self.event, self.value
        )
    }
}

impl std::error::Error for EffectError {}

fn checked(event: GameEvent, value: f32) -> Result<f32, EffectError> {
    if value.is_finite() && value >= 0.0 {
        Ok(value.min(MAX_INTENSITY))
    } else {
        Err(EffectError { event, value })
    }
}

/// The raw effect requests of `event`, before the settings filter them.
///
/// # Errors
///
/// [`EffectError`] for a non-finite or negative magnitude.
pub fn effects_of(event: GameEvent) -> Result<Vec<Effect>, EffectError> {
    Ok(match event {
        GameEvent::PlayerHit { damage } => {
            let shake = checked(event, damage * SHAKE_PER_DAMAGE)?;
            vec![
                Effect::CameraShake(shake),
                Effect::ScreenFlash(checked(event, damage)?),
                Effect::DamageNotice,
            ]
        }
        GameEvent::Impact { strength } => vec![Effect::CameraShake(checked(event, strength)?)],
        GameEvent::WeaponFlash { intensity } => {
            vec![Effect::ScreenFlash(checked(event, intensity)?)]
        }
        GameEvent::TargetChanged => vec![Effect::TargetNotice],
    })
}

/// What one frame draws after the settings have filtered its events.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectFrame {
    /// The presented effects, in event order.
    pub presented: Vec<Effect>,
    /// Events that could not be drawn.
    pub errors: Vec<EffectError>,
}

impl EffectFrame {
    /// Produces and presents `events` under `session`'s current settings.
    #[must_use]
    pub fn present(session: &SettingsSession, events: &[GameEvent]) -> Self {
        let mut raw = Vec::new();
        let mut errors = Vec::new();
        for event in events {
            match effects_of(*event) {
                Ok(effects) => raw.extend(effects),
                Err(error) => errors.push(error),
            }
        }
        Self {
            presented: session.present(&raw),
            errors,
        }
    }

    /// The camera shake to apply: the largest presented amplitude, `0.0` if none.
    #[must_use]
    pub fn shake(&self) -> f32 {
        self.max_of(|effect| match effect {
            Effect::CameraShake(amplitude) => Some(*amplitude),
            _ => None,
        })
    }

    /// The screen flash to draw: the largest presented intensity, `0.0` if none.
    #[must_use]
    pub fn flash(&self) -> f32 {
        self.max_of(|effect| match effect {
            Effect::ScreenFlash(intensity) => Some(*intensity),
            _ => None,
        })
    }

    fn max_of(&self, pick: impl Fn(&Effect) -> Option<f32>) -> f32 {
        self.presented.iter().filter_map(pick).fold(0.0, f32::max)
    }
}
