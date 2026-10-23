//! Force feedback for a flight stick: what the player feels, and when it stops
//! (SIDEWINDER-FFB2, #1968).
//!
//! [`FlightFeel`] turns the flight's state into force-feedback [`Command`]s for
//! any [`ForceFeedbackOutput`]: the HID PID backend of
//! [`crate::input::hid`] plays them on a SideWinder Force Feedback 2, a test
//! records them. It knows nothing about a particular device, and the backend
//! knows nothing about flight.
//!
//! # What is felt (designed, not original)
//!
//! The original game's force-feedback effects (Immersion's `crimsonff.ifr`,
//! played through IFC) are **not** reproduced here: their parameters and the
//! game events that trigger them are unmeasured in this project. Every effect
//! below is a newly designed default, claimed as nothing more:
//!
//! | Effect | Kind | Driven by |
//! | --- | --- | --- |
//! | centring | spring on both axes | stiffens with airspeed ([`spring_coefficient`]) |
//! | damping | damper on both axes | constant, light |
//! | buffet | sine on the pitch axis | the flight law's stall margin ([`buffet_magnitude`]) |
//! | guns | square on both axes | the player's guns firing |
//! | hit | short constant force | the player being hit by another actor |
//! | impact | short constant force | the player striking terrain or an obstacle |
//!
//! # Safety
//!
//! * **Bounded strength.** The backend scales every force by
//!   [`ForceFeedbackSettings::gain_percent`], [`DESIGNED_DEFAULT_GAIN_PERCENT`]
//!   unless the player asks for more, never above [`MAX_GAIN_PERCENT`]. Effect
//!   magnitudes here stay below full scale too.
//! * **No jolt.** Nothing plays until the player has control of a flying
//!   aircraft, and the centring spring then ramps in over [`SPRING_RAMP_S`]
//!   instead of snapping a held stick to the centre.
//! * **Nothing stuck.** Whenever flight stops being live — pause, focus loss,
//!   a cinematic, a crash, leaving the mission, the device going away,
//!   shutdown — [`FlightFeel`] sends [`Command::Reset`], which stops and frees
//!   every effect, and plays nothing until flight is live again.

use std::collections::BTreeSet;

use bevy::prelude::Resource;

use crate::input::hid::effect::{
    Axis, Command, Condition, ConditionKind, Effect, EffectKind, Envelope, INFINITE, Waveform,
};

/// The strength force feedback starts at, percent of the device's full force.
///
/// Designed: low enough that the first effect a player feels is gentle.
pub const DESIGNED_DEFAULT_GAIN_PERCENT: u32 = 25;

/// The highest strength a player may ask for, percent.
pub const MAX_GAIN_PERCENT: u32 = 100;

/// How long the centring spring takes to reach its stiffness, seconds.
pub const SPRING_RAMP_S: f32 = 1.5;

/// Effect ids [`FlightFeel`] downloads (the backend's own names for them).
pub mod ids {
    /// The centring spring.
    pub const SPRING: u32 = 1;
    /// The damper.
    pub const DAMPER: u32 = 2;
    /// The stall buffet.
    pub const BUFFET: u32 = 3;
    /// The guns.
    pub const GUNS: u32 = 4;
    /// A hit.
    pub const HIT: u32 = 5;
    /// An impact.
    pub const IMPACT: u32 = 6;
}

/// The player's force-feedback choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForceFeedbackSettings {
    /// Whether force feedback is used at all.
    pub enabled: bool,
    /// Strength, percent of the device's full force, at most
    /// [`MAX_GAIN_PERCENT`].
    pub gain_percent: u32,
}

impl Default for ForceFeedbackSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            gain_percent: DESIGNED_DEFAULT_GAIN_PERCENT,
        }
    }
}

/// Why a [`ForceFeedbackSettings`] was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GainOutOfRange(pub u32);

impl std::fmt::Display for GainOutOfRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "force-feedback gain {}% is outside 0..={MAX_GAIN_PERCENT}%",
            self.0
        )
    }
}

impl std::error::Error for GainOutOfRange {}

impl ForceFeedbackSettings {
    /// Enabled at `gain_percent`.
    ///
    /// # Errors
    ///
    /// [`GainOutOfRange`] above [`MAX_GAIN_PERCENT`]: a strength is refused, not
    /// clamped, so a typo cannot ask a motor for more than the player meant.
    pub const fn with_gain(gain_percent: u32) -> Result<Self, GainOutOfRange> {
        if gain_percent > MAX_GAIN_PERCENT {
            return Err(GainOutOfRange(gain_percent));
        }
        Ok(Self {
            enabled: true,
            gain_percent,
        })
    }

    /// Force feedback off.
    #[must_use]
    pub const fn off() -> Self {
        Self {
            enabled: false,
            gain_percent: 0,
        }
    }
}

/// Receives force-feedback commands (a device backend, or a test's recorder).
pub trait ForceFeedbackOutput {
    /// Play one command.
    fn send(&mut self, command: Command);
}

impl ForceFeedbackOutput for Vec<Command> {
    fn send(&mut self, command: Command) {
        self.push(command);
    }
}

/// Whether flight is live for force feedback, and why not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeelGate {
    /// The player has control of a flying aircraft.
    Flying,
    /// No force-feedback device is connected.
    NoDevice,
    /// The session is paused (the pause map, a menu).
    Paused,
    /// The window lost focus.
    Unfocused,
    /// The player has no control (a cinematic, a crash, a screen).
    NoControl,
    /// The game is shutting down.
    Shutdown,
}

/// The player aircraft's latest flight-law step, for force feedback: written by
/// the flight driver ([`crate::playtest`]) when the resource exists.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct PlayerFlightFeel {
    /// Airspeed, m/s.
    pub airspeed_mps: f32,
    /// The flight law's stall margin (`> 0` stalled).
    pub stall_margin: f32,
}

/// What the flight did since the last frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FlightCues {
    /// The player's airspeed, m/s.
    pub airspeed_mps: f32,
    /// The flight law's stall margin (`> 0` stalled).
    pub stall_margin: f32,
    /// Whether the player's guns fired.
    pub guns_firing: bool,
    /// The heaviest hit the player took, `0..=1`; `0` none.
    pub hit: f32,
    /// The hardest impact the player suffered, `0..=1`; `0` none.
    pub impact: f32,
}

/// The airspeed at which the centring spring is at its stiffest, m/s
/// (designed: about the cruise of the M01 aircraft).
const STIFF_AIRSPEED_MPS: f32 = 100.0;

/// The centring spring's coefficient at `airspeed_mps`, in `0..=10000`.
///
/// Designed: a light spring on the ground that stiffens linearly with airspeed,
/// as control loads grow with dynamic pressure.
#[must_use]
pub fn spring_coefficient(airspeed_mps: f32) -> i32 {
    let speed = if airspeed_mps.is_finite() {
        airspeed_mps.max(0.0)
    } else {
        0.0
    };
    let fraction = (speed / STIFF_AIRSPEED_MPS).min(1.0);
    (1500.0 + fraction * 4500.0).round() as i32
}

/// The buffet's magnitude at a stall margin, in `0..=10000`.
///
/// Designed: nothing while flying, a light shake at the onset of the stall,
/// growing with the margin the flight law drops the nose by.
#[must_use]
pub fn buffet_magnitude(stall_margin: f32) -> u32 {
    if !stall_margin.is_finite() || stall_margin <= 0.0 {
        return 0;
    }
    let depth = (stall_margin / 0.3).min(1.0);
    (1500.0 + depth * 3500.0).round() as u32
}

/// A spring or damper on both axes.
fn condition(kind: ConditionKind, coefficient: i32) -> Effect {
    let c = Condition {
        offset: 0,
        positive_coefficient: coefficient,
        negative_coefficient: coefficient,
        positive_saturation: 10000,
        negative_saturation: 10000,
        dead_band: 0,
    };
    Effect {
        kind: EffectKind::Condition {
            kind,
            axes: vec![c, c],
        },
        duration_us: INFINITE,
        sample_period_us: 0,
        gain: 10000,
        trigger_button: None,
        trigger_repeat_us: 0,
        axes: vec![Axis::X, Axis::Y],
        direction: 0,
        envelope: None,
        start_delay_us: 0,
    }
}

/// A force with no condition: `kind` along `direction` for `duration_us`.
fn force(kind: EffectKind, direction: u32, duration_us: u32, envelope: Option<Envelope>) -> Effect {
    Effect {
        kind,
        duration_us,
        sample_period_us: 0,
        gain: 10000,
        trigger_button: None,
        trigger_repeat_us: 0,
        axes: vec![Axis::X, Axis::Y],
        direction,
        envelope,
        start_delay_us: 0,
    }
}

fn buffet(magnitude: u32) -> Effect {
    force(
        EffectKind::Periodic {
            waveform: Waveform::Sine,
            magnitude,
            offset: 0,
            phase: 0,
            period_us: 80_000,
        },
        0,
        INFINITE,
        None,
    )
}

fn guns() -> Effect {
    force(
        EffectKind::Periodic {
            waveform: Waveform::Square,
            magnitude: 2500,
            offset: 0,
            phase: 0,
            period_us: 60_000,
        },
        18000,
        INFINITE,
        None,
    )
}

/// A short push, fading out, along `direction`.
fn jolt(magnitude: i32, direction: u32, duration_us: u32) -> Effect {
    force(
        EffectKind::Constant { magnitude },
        direction,
        duration_us,
        Some(Envelope {
            attack_level: magnitude.unsigned_abs(),
            attack_us: 0,
            fade_level: 0,
            fade_us: duration_us / 2,
        }),
    )
}

/// The force-feedback state of one flight: which effects are on the device and
/// playing, and the rules that start and stop them.
#[derive(Clone, Debug, Default)]
pub struct FlightFeel {
    live: bool,
    /// Seconds since flight became live, for the spring's ramp.
    live_s: f32,
    /// Effect ids on the device.
    downloaded: BTreeSet<u32>,
    /// Effect ids playing until stopped.
    playing: BTreeSet<u32>,
    spring: i32,
    buffet: u32,
    /// Alternates the side hits push from.
    hits: u32,
    resets: u32,
}

impl FlightFeel {
    /// A flight with nothing on the device.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether flight is live (effects may be playing).
    #[must_use]
    pub const fn is_live(&self) -> bool {
        self.live
    }

    /// The effect ids playing until stopped.
    #[must_use]
    pub const fn playing(&self) -> &BTreeSet<u32> {
        &self.playing
    }

    /// The centring spring's current coefficient.
    #[must_use]
    pub const fn spring(&self) -> i32 {
        self.spring
    }

    /// How many times the device was reset because flight stopped being live.
    #[must_use]
    pub const fn resets(&self) -> u32 {
        self.resets
    }

    /// Stop every effect now (a reset), whatever the gate.
    pub fn stop(&mut self, out: &mut dyn ForceFeedbackOutput) {
        if self.live || !self.downloaded.is_empty() {
            out.send(Command::Reset);
            self.resets += 1;
        }
        *self = Self {
            resets: self.resets,
            hits: self.hits,
            ..Self::default()
        };
    }

    /// One frame: `dt_s` seconds passed, flight is `gate`, and the flight did
    /// `cues`.
    pub fn update(
        &mut self,
        gate: FeelGate,
        dt_s: f32,
        cues: &FlightCues,
        out: &mut dyn ForceFeedbackOutput,
    ) {
        if gate != FeelGate::Flying {
            self.stop(out);
            return;
        }
        if !self.live {
            self.live = true;
            self.live_s = 0.0;
            self.download(out, ids::DAMPER, condition(ConditionKind::Damper, 1200));
            self.start(out, ids::DAMPER);
        }
        if dt_s.is_finite() && dt_s > 0.0 {
            self.live_s += dt_s;
        }

        // The spring ramps in, then follows the airspeed in coarse steps so a
        // steady flight does not rewrite it every frame.
        let ramp = (self.live_s / SPRING_RAMP_S).min(1.0);
        let target = (spring_coefficient(cues.airspeed_mps) as f32 * ramp).round() as i32;
        if !self.playing.contains(&ids::SPRING) || (target - self.spring).abs() >= 150 {
            self.spring = target;
            self.download(out, ids::SPRING, condition(ConditionKind::Spring, target));
            self.start(out, ids::SPRING);
        }

        let buffet_now = buffet_magnitude(cues.stall_margin);
        if buffet_now == 0 {
            self.halt(out, ids::BUFFET);
            self.buffet = 0;
        } else if self.buffet.abs_diff(buffet_now) >= 250 || !self.playing.contains(&ids::BUFFET) {
            self.buffet = buffet_now;
            self.download(out, ids::BUFFET, buffet(buffet_now));
            self.start(out, ids::BUFFET);
        }

        if cues.guns_firing {
            if !self.playing.contains(&ids::GUNS) {
                self.download(out, ids::GUNS, guns());
                self.start(out, ids::GUNS);
            }
        } else {
            self.halt(out, ids::GUNS);
        }

        if cues.hit > 0.0 && cues.hit.is_finite() {
            self.hits = self.hits.wrapping_add(1);
            let side = if self.hits.is_multiple_of(2) {
                9000
            } else {
                27000
            };
            let magnitude = (3000.0 + cues.hit.min(1.0) * 3500.0).round() as i32;
            self.download(out, ids::HIT, jolt(magnitude, side, 150_000));
            self.once(out, ids::HIT);
        }
        if cues.impact > 0.0 && cues.impact.is_finite() {
            let magnitude = (4000.0 + cues.impact.min(1.0) * 4000.0).round() as i32;
            self.download(out, ids::IMPACT, jolt(magnitude, 18000, 250_000));
            self.once(out, ids::IMPACT);
        }
    }

    fn download(&mut self, out: &mut dyn ForceFeedbackOutput, id: u32, effect: Effect) {
        out.send(Command::Download { id, effect });
        self.downloaded.insert(id);
    }

    fn start(&mut self, out: &mut dyn ForceFeedbackOutput, id: u32) {
        if self.playing.insert(id) {
            out.send(Command::Start {
                id,
                iterations: INFINITE,
                solo: false,
            });
        }
    }

    /// Play a finite effect once (restarting it if it is still playing).
    fn once(&mut self, out: &mut dyn ForceFeedbackOutput, id: u32) {
        out.send(Command::Start {
            id,
            iterations: 1,
            solo: false,
        });
    }

    fn halt(&mut self, out: &mut dyn ForceFeedbackOutput, id: u32) {
        if self.playing.remove(&id) {
            out.send(Command::Stop { id });
        }
    }
}
