//! Engine pitch and volume driven by the authoritative throttle state (F41-B
//! follow-up #445).
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-B`, non-negotiable behavior 1: *"Engine pitch/volume depend on
//! measured throttle/engine state with stable smoothing, not render FPS."*
//!
//! The ECS side of that rule has two halves:
//!
//! * [`EngineVoiceFollow`] marks the emitter of an engine loop as a voice whose
//!   level follows the flight model. It is a component rather than a flag on
//!   [`AudioEmitterBinding`](super::AudioEmitterBinding) because the binding is
//!   F41-B's *lifecycle* record — which asset plays on which emitter — and this
//!   is a different question: how loudly, and how fast. The spawn path attaches
//!   it alongside the binding it produces (`super::spawn`).
//! * [`smooth_engine_voices`] advances every followed voice by exactly one
//!   **fixed** tick of the authoritative [`cs_sim::flight::EngineState`] spool
//!   the F24-B driver integrates. It reads `Time<Fixed>`, never a render
//!   delta, so a frame-rate change cannot change the mix — and because the
//!   smoothing law is a clamped linear ramp, the same elapsed time lands on the
//!   same level however the frame budget divided it.
//!
//! The smoothing law itself ([`cs_sim::audio_events::EngineSmoothing`]) is
//! designed project data, not a measurement of the original engine's audio; see
//! `docs/findings/2026-10-01-f41-b-loops-and-spatial-emitters.md`. Nothing
//! here claims the original's pitch range or attack time.
//!
//! The voices live in the [`EngineVoices`] resource rather than on the entity:
//! the entity may despawn in the middle of a fade-out, and the level the mixer
//! reads has to survive that, so a voice is dropped when its emitter is gone
//! rather than when its body is.

use std::collections::{BTreeMap, BTreeSet};

use bevy::ecs::component::Component;
use bevy::ecs::query::{Or, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::time::{Fixed, Time};
use cs_sim::audio_events::{AudioEmitterId, EngineSmoothing, EngineVoice, VoiceLevel};

use crate::physics::flight::FlightAircraft;
use crate::playtest::scene::PlaytestOriginalFlight;

use super::AudioEmitterBinding;

/// Component: this emitter's engine loop follows the flight model's throttle.
///
/// The law is per emitter on purpose: a fleet of different aircraft types does
/// not share one engine note, and a mission may override the designed default
/// per type. [`Default`] is [`EngineSmoothing::DESIGNED_DEFAULT`].
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct EngineVoiceFollow {
    /// The designed smoothing law this voice obeys.
    pub smoothing: EngineSmoothing,
}

impl Default for EngineVoiceFollow {
    fn default() -> Self {
        Self {
            smoothing: EngineSmoothing::DESIGNED_DEFAULT,
        }
    }
}

/// Resource: every live engine voice's smoothed level, keyed by emitter.
///
/// The mixer reads [`Self::level`]; an emitter with no voice here is an
/// emitter nothing drives, and gets [`VoiceLevel::UNITY`] rather than silence —
/// that is the whole meaning of "not engine driven".
#[derive(Resource, Clone, Debug, Default)]
pub struct EngineVoices(BTreeMap<AudioEmitterId, EngineVoice>);

impl EngineVoices {
    /// How many voices are live.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether no voice is live.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The live emitters, in emitter order.
    pub fn emitters(&self) -> impl Iterator<Item = &AudioEmitterId> {
        self.0.keys()
    }

    /// One voice's smoothed state, if it is live.
    #[must_use]
    pub fn voice(&self, emitter: &AudioEmitterId) -> Option<&EngineVoice> {
        self.0.get(emitter)
    }

    /// The level one emitter's loop plays at: its smoothed level when a voice
    /// follows it, and [`VoiceLevel::UNITY`] when nothing does.
    #[must_use]
    pub fn level(&self, emitter: &AudioEmitterId) -> VoiceLevel {
        self.0
            .get(emitter)
            .map_or(VoiceLevel::UNITY, EngineVoice::level)
    }

    /// Moves one voice one step toward `target`, creating it at its law's idle
    /// level when it is new.
    pub fn advance(
        &mut self,
        emitter: AudioEmitterId,
        smoothing: &EngineSmoothing,
        target: VoiceLevel,
        dt_s: f64,
    ) {
        self.0
            .entry(emitter)
            .or_insert_with(|| EngineVoice::at_idle(smoothing))
            .advance(smoothing, target, dt_s);
    }

    /// Drops every voice whose emitter is not in `live`.
    pub fn retain(&mut self, live: &BTreeSet<AudioEmitterId>) {
        self.0.retain(|emitter, _| live.contains(emitter));
    }
}

/// Advances every followed engine voice by one fixed tick of throttle state.
///
/// The authority is the flight model's own spool: the F24-B driver integrates it
/// from the commanded throttle with the airframe's declared response rate, so
/// this system reacts to *measured* engine state rather than re-deriving
/// throttle from the input, and an engine that is not running asks for silence.
/// A body spawned over the recovered original law carries no `FlightAircraft`
/// record at all, so the query reads whichever authority the body has — the
/// designed record or [`PlaytestOriginalFlight`]'s own actual throttle — and a
/// body with neither is not followed.
///
/// Voices whose emitter no longer has a live binding are dropped in the same
/// pass, so a destroyed aircraft cannot leave a voice behind: the loop stops
/// through [`super::sync_emitter_loops`] and the level stops being maintained
/// here, in one tick, with no window in which a stale emitter is still mixed.
pub fn smooth_engine_voices(
    time: Res<Time<Fixed>>,
    mut voices: ResMut<EngineVoices>,
    followed: Query<
        (
            &AudioEmitterBinding,
            &EngineVoiceFollow,
            Option<&FlightAircraft>,
            Option<&PlaytestOriginalFlight>,
        ),
        Or<(With<FlightAircraft>, With<PlaytestOriginalFlight>)>,
    >,
) {
    let dt_s = f64::from(time.delta_secs());
    let live: BTreeSet<AudioEmitterId> = followed
        .iter()
        .map(|(binding, ..)| binding.emitter)
        .collect();
    voices.retain(&live);
    for (binding, follow, flight, original) in &followed {
        // Two engine authorities and one law per body, exactly as the
        // propeller spin reads them: the designed F24 record when the body
        // carries it, otherwise the recovered original law's own throttle. A
        // body with neither cannot reach this system — the query filters for
        // one of them — so nothing here ever guesses a target level.
        let engine = match (flight, original) {
            (Some(record), _) => record.engine(),
            (_, Some(record)) => record.engine(),
            (None, None) => continue,
        };
        let target = follow.smoothing.target(engine.running, engine.spool);
        voices.advance(binding.emitter, &follow.smoothing, target, dt_s);
    }
}
