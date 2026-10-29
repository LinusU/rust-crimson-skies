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

use bevy::prelude::{Local, Res, ResMut, Resource};
use cs_sim::collision::CollisionLayer;
use cs_sim::damage::ActorId;
use cs_types::Tick;

use crate::accessibility::motion::Effect;
use crate::accessibility::session::SettingsSession;
use crate::mission_session::{MissionOrdnanceReport, MissionWeapons, PlayerVictim};
use crate::physics::{ContactReports, PhysicsTickLedger};
use crate::render::presentation::LivePresentation;
use crate::targeting::{PendingTargetDamage, TargetingScheduleReport};
use crate::weapons::WeaponScheduleReport;

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

// ------------------------------------------------------------ the wire ---
//
// F52-W2-wire: the production systems that *know* something happened raise a
// [`GameEvent`] here; `present_game_events` is the only way those events reach
// the screen, through [`EffectFrame::present`] and the live session's reduced
// motion switches. The queue and the presented frame are resources so a test —
// or the mission composition — sees exactly what a frame presented.

/// Resource: the gameplay events raised since the last presentation pass.
///
/// Emission is one-way: a producer pushes, `present_game_events` drains once
/// per rendered frame. Nothing reads the queue as state.
#[derive(Resource, Debug, Default)]
pub struct GameEventQueue {
    /// The events, in raise order.
    pub events: Vec<GameEvent>,
}

impl GameEventQueue {
    /// Queues `event` for the next presentation pass.
    pub fn push(&mut self, event: GameEvent) {
        self.events.push(event);
    }

    /// Takes every queued event, oldest first.
    pub fn drain(&mut self) -> Vec<GameEvent> {
        std::mem::take(&mut self.events)
    }
}

/// Resource: the most recent frame's presented effects.
///
/// `None` while no [`LivePresentation`] is installed — a session-less world
/// has no settings to present under. [`present_game_events`] rewrites it every
/// rendered frame, so a reader always sees the frame being drawn.
#[derive(Resource, Debug, Default)]
pub struct PresentedEffects {
    /// The frame `present_game_events` last produced.
    pub frame: Option<EffectFrame>,
}

/// The impact strength a contact on the player's aircraft asks for
/// (designed, in the `0.0..=1.0` `GameEvent::Impact` scale): the contact
/// reports carry no impulse, so a reach is presented at a designed mid
/// strength rather than a guessed one.
pub const CONTACT_IMPACT_STRENGTH: f32 = 0.5;
/// The flash intensity an accepted shot or rocket launch of the player's
/// asks for (designed, in the `0.0..=1.0` `GameEvent::WeaponFlash` scale).
pub const MUZZLE_FLASH_INTENSITY: f32 = 0.6;

/// Emits one committed tick's combat events (F52-W2-wire).
///
/// Runs in `FixedLast`, after this tick's contact recording, weapon step,
/// ordnance driver and damage publication have all run, so every record it
/// reads is the tick's own — filtered by the tick stamp the producers minted,
/// exactly as `record_collisions` reads them:
///
/// * a contact pair involving an `Aircraft`-layer body (the player's) becomes
///   [`GameEvent::Impact`];
/// * the player's accepted shots and accepted ordnance launches become
///   [`GameEvent::WeaponFlash`];
/// * a hit the published damage batch names on the player's victim becomes
///   [`GameEvent::PlayerHit`].
///
/// A world without those resources — the plain playtest has no weapon
/// session — emits only what it has. Nothing here changes gameplay: the
/// queue is presentation input only.
#[allow(clippy::too_many_arguments)]
pub fn emit_combat_events(
    ledger: Option<Res<PhysicsTickLedger>>,
    contacts: Option<Res<ContactReports>>,
    weapons: Option<Res<WeaponScheduleReport>>,
    ordnance: Option<Res<MissionOrdnanceReport>>,
    mission: Option<Res<MissionWeapons>>,
    victim: Option<Res<PlayerVictim>>,
    pending: Option<Res<PendingTargetDamage>>,
    mut queue: ResMut<GameEventQueue>,
    mut ordnance_seen: Local<usize>,
) {
    let Some(ledger) = ledger else {
        return;
    };
    let at = Tick(ledger.ticks);
    if let Some(contacts) = contacts {
        for report in contacts.reports() {
            if report.tick == ledger.ticks && report.involves(CollisionLayer::Aircraft) {
                queue.push(GameEvent::Impact {
                    strength: CONTACT_IMPACT_STRENGTH,
                });
            }
        }
    }
    if let (Some(weapons), Some(mission)) = (weapons, mission)
        && let Some(stepped) = &weapons.stepped
        && stepped.at == at
    {
        for _ in stepped
            .accepted
            .iter()
            .filter(|shot| shot.shooter == mission.player())
        {
            queue.push(GameEvent::WeaponFlash {
                intensity: MUZZLE_FLASH_INTENSITY,
            });
        }
    }
    if let (Some(victim), Some(pending)) = (victim, pending) {
        for hit in pending.tick().hits {
            if hit.target == victim.actor && hit.id.tick == at {
                queue.push(GameEvent::PlayerHit {
                    damage: hit.damage as f32,
                });
            }
        }
    }
    if let Some(ordnance) = ordnance {
        // The ordnance report accumulates launches for the session and is
        // recreated on a restart, so a cursor — not a tick — is the dedup.
        // A cursor ahead of the list means the report is new.
        if *ordnance_seen > ordnance.projectiles.len() {
            *ordnance_seen = 0;
        }
        for _ in &ordnance.projectiles[*ordnance_seen..] {
            queue.push(GameEvent::WeaponFlash {
                intensity: MUZZLE_FLASH_INTENSITY,
            });
        }
        *ordnance_seen = ordnance.projectiles.len();
    }
}

/// Emits [`GameEvent::TargetChanged`] when the frame's selection pass moved
/// the observer's target (F52-W2-wire).
///
/// Runs in `Update` after `drive_targeting_schedule`, so the selection the
/// report records is this frame's own. A frame whose selection pass did not
/// run — no session or no frame inputs — is no change, not a clear.
pub fn emit_targeting_events(
    report: Option<Res<TargetingScheduleReport>>,
    mut queue: ResMut<GameEventQueue>,
    mut last: Local<Option<ActorId>>,
) {
    let Some(report) = report else {
        return;
    };
    if let Some(selection) = &report.selection
        && selection.selection != *last
    {
        *last = selection.selection;
        queue.push(GameEvent::TargetChanged);
    }
}

/// Presents the queued gameplay events under the live session (F52-W2-wire).
///
/// [`EffectFrame::present`] is the only way the events reach the screen:
/// reduced motion drops the cosmetic shake and flash while the required
/// damage and target notices always pass, and a magnitude the producer got
/// wrong is recorded in the frame's `errors`, never dropped. The session is
/// read on the call, not cached — a setting the player changes mid-frame is
/// what this frame presents.
///
/// Runs in `Update` after [`emit_targeting_events`], which is itself after
/// the fixed loop's emit, so one frame carries every event this rendered
/// frame raised. A world with no session drains the queue and presents
/// nothing — the fallback a session-less world already had.
pub fn present_game_events(
    live: Option<Res<LivePresentation>>,
    mut queue: ResMut<GameEventQueue>,
    mut presented: ResMut<PresentedEffects>,
) {
    let events = queue.drain();
    presented.frame = live
        .as_deref()
        .map(|live| EffectFrame::present(&live.0, &events));
}
