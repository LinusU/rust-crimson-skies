//! The ECS loop lifecycle: emitter bindings starting, swapping and stopping
//! the session's engine/gunfire loops (F41-B).
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-B`. [`sync_emitter_loops`] is the one system that turns the
//! lifetime of [`AudioEmitterBinding`] entities into
//! [`cs_sim::audio_events::AudioRouter`] commands:
//!
//! * a newly added binding of the current scene generation binds its asset's
//!   loop to its emitter;
//! * a binding from a stale scene generation is refused by name, never
//!   played;
//! * a despawned binding stops its loop as
//!   [`EmitterStopReason::Despawned`] — but only if the loop that is still
//!   active is the one *that entity* bound, so a replacement bound to the same
//!   emitter earlier in the frame is never silenced by its predecessor's
//!   teardown.
//!
//! Removals are processed before additions, so destroying the player aircraft
//! and spawning its replacement in one frame ends the old loop and binds the
//! new one, whatever order the commands were queued in.
//!
//! The system never opens a device: it records every [`LoopOutcome`] and
//! refusal in [`AudioSession`] for the mixer to consume, so simulation does not
//! depend on audio hardware (F41 non-negotiable behavior 2).

use std::collections::BTreeMap;

use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::query::Added;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use cs_sim::audio_events::{
    AudioAssetSpec, AudioEmitterId, AudioEventError, AudioEventId, AudioRouter, EmitterStopReason,
    LoopBinding, LoopOutcome, MusicCue, MusicDirector, MusicOutcome, RadioEvent, RadioLine,
    RadioQueue,
};
use cs_types::Tick;
use cs_types::content::ContentId;

use crate::scene::SceneGeneration;

use super::AudioEmitterBinding;

/// The producer serial stamped on loop bindings this system creates.
pub const LOOP_PRODUCER: u32 = 0;

/// Why a binding could not start a loop.
#[derive(Clone, Debug, PartialEq)]
pub enum LoopRefusal {
    /// The binding was spawned under another scene generation.
    StaleGeneration {
        /// The binding's emitter.
        emitter: AudioEmitterId,
        /// The generation the binding carries.
        binding: SceneGeneration,
        /// The session's current generation.
        current: SceneGeneration,
    },
    /// The asset has no lowered spec in the session.
    UnknownAsset {
        /// The binding's emitter.
        emitter: AudioEmitterId,
        /// The missing asset.
        asset: ContentId,
    },
    /// The spec exists but could not form a loop (for example a one-shot).
    Spec {
        /// The binding's emitter.
        emitter: AudioEmitterId,
        /// Why.
        error: AudioEventError,
    },
}

/// The session's audio state: router, lowered specs and the consumer-visible
/// outcome log. A Bevy resource.
#[derive(Resource, Debug)]
pub struct AudioSession {
    /// The loop registry and one-shot ledger.
    pub router: AudioRouter,
    /// The scene generation bindings must carry.
    pub generation: SceneGeneration,
    /// The simulation tick loop ids are stamped with.
    pub tick: Tick,
    specs: BTreeMap<ContentId, AudioAssetSpec>,
    bound: BTreeMap<Entity, (AudioEmitterId, AudioEventId)>,
    next_sequence: u32,
    lost_loops: BTreeMap<AudioEmitterId, LoopBinding>,
    device_available: bool,
    /// The radio queue; its completion is tick-driven, never device-driven.
    pub radio: RadioQueue,
    /// The authored music cue director.
    pub music: MusicDirector,
    /// Every radio event since the last [`Self::drain_radio`], in order.
    pub radio_events: Vec<RadioEvent>,
    /// Every loop outcome since the last [`Self::drain`], in order.
    pub outcomes: Vec<LoopOutcome>,
    /// Every refused binding since the last [`Self::drain`].
    pub refusals: Vec<LoopRefusal>,
}

impl AudioSession {
    /// A session over `router` for scene `generation`, with lowered `specs`.
    #[must_use]
    pub fn new(
        router: AudioRouter,
        generation: SceneGeneration,
        specs: impl IntoIterator<Item = AudioAssetSpec>,
    ) -> Self {
        let session = router.session();
        Self {
            router,
            generation,
            lost_loops: BTreeMap::new(),
            device_available: true,
            radio: RadioQueue::new(session),
            music: MusicDirector::new(session),
            radio_events: Vec::new(),
            tick: Tick(0),
            specs: specs.into_iter().map(|s| (s.asset().clone(), s)).collect(),
            bound: BTreeMap::new(),
            next_sequence: 0,
            outcomes: Vec::new(),
            refusals: Vec::new(),
        }
    }

    /// Takes the accumulated outcomes and refusals.
    pub fn drain(&mut self) -> (Vec<LoopOutcome>, Vec<LoopRefusal>) {
        (
            std::mem::take(&mut self.outcomes),
            std::mem::take(&mut self.refusals),
        )
    }

    /// The specs the installed load lowered, in content-id order.
    ///
    /// This is the session's view of *playable* content: the declared records
    /// of the delivered closure that [`lower_record`](super::lower_record)
    /// accepted. [`bind_spawned_emitters`](super::bind_spawned_emitters) reads
    /// it to resolve an emitter role delivered-first, so an emitter names
    /// something the load really delivered whenever the load carries it.
    pub fn specs(&self) -> impl Iterator<Item = &AudioAssetSpec> {
        self.specs.values()
    }

    /// Takes the accumulated outcomes, leaving the refusals for a consumer that
    /// reports them.
    ///
    /// This is what [`mix_session`](super::mix_session) calls every frame: the
    /// mixer is the only routine consumer of outcomes, while refusals name
    /// content that will *not* play and belong in a UI log or a report rather
    /// than in a per-frame drain nobody reads.
    pub fn drain_outcomes(&mut self) -> Vec<LoopOutcome> {
        std::mem::take(&mut self.outcomes)
    }

    /// Takes the accumulated radio events.
    pub fn drain_radio(&mut self) -> Vec<RadioEvent> {
        std::mem::take(&mut self.radio_events)
    }

    /// Offers a radio line at the session's current tick.
    pub fn enqueue_radio(&mut self, line: RadioLine) {
        let events = self.radio.enqueue(line, self.tick);
        self.radio_events.extend(events);
    }

    /// Requests an authored music cue.
    pub fn request_music(&mut self, cue: MusicCue) -> MusicOutcome {
        self.music.request(cue)
    }

    /// Whether an output device is currently available.
    #[must_use]
    pub const fn device_available(&self) -> bool {
        self.device_available
    }

    /// The output device is lost: every loop stops as `DeviceLost` but is
    /// remembered for retry; radio keeps its tick timing unvoiced. Total and
    /// idempotent; simulation never depends on it.
    pub fn device_lost(&mut self) {
        if !self.device_available {
            return;
        }
        self.device_available = false;
        let lost: Vec<LoopBinding> = self.router.active_loops().cloned().collect();
        for binding in lost {
            self.lost_loops.insert(binding.emitter, binding);
        }
        let outcomes = self.router.device_lost();
        self.outcomes.extend(outcomes);
        let events = self.radio.device_lost();
        self.radio_events.extend(events);
        self.music.device_lost();
    }

    /// The device returned: rebinds every loop whose emitter is still alive and
    /// returns the music cue to restart, if any.
    pub fn device_restored(&mut self) -> Option<MusicCue> {
        if self.device_available {
            return None;
        }
        self.device_available = true;
        for (_, binding) in std::mem::take(&mut self.lost_loops) {
            let outcome = self.router.start_loop(&binding);
            self.outcomes.push(outcome);
        }
        self.radio.device_restored();
        self.music.device_restored().cloned()
    }

    fn bind(&mut self, entity: Entity, binding: &AudioEmitterBinding) {
        let emitter = binding.emitter;
        if binding.generation != self.generation {
            self.refusals.push(LoopRefusal::StaleGeneration {
                emitter,
                binding: binding.generation,
                current: self.generation,
            });
            return;
        }
        let Some(spec) = self.specs.get(&binding.asset) else {
            self.refusals.push(LoopRefusal::UnknownAsset {
                emitter,
                asset: binding.asset.clone(),
            });
            return;
        };
        let id = AudioEventId {
            session: self.router.session(),
            tick: self.tick,
            producer: LOOP_PRODUCER,
            sequence: self.next_sequence,
        };
        match spec.loop_binding(emitter, id) {
            Ok(loop_binding) => {
                self.next_sequence += 1;
                if !self.device_available {
                    // Remembered; started when the device returns.
                    self.bound.insert(entity, (emitter, id));
                    self.lost_loops.insert(emitter, loop_binding);
                    return;
                }
                let outcome = self.router.start_loop(&loop_binding);
                if matches!(
                    outcome,
                    LoopOutcome::Started { .. } | LoopOutcome::Swapped { .. }
                ) {
                    self.bound.insert(entity, (emitter, id));
                }
                self.outcomes.push(outcome);
            }
            Err(error) => self.refusals.push(LoopRefusal::Spec { emitter, error }),
        }
    }

    fn unbind(&mut self, entity: Entity) {
        let Some((emitter, id)) = self.bound.remove(&entity) else {
            return;
        };
        if self.lost_loops.get(&emitter).map(|l| l.id) == Some(id) {
            self.lost_loops.remove(&emitter);
            return;
        }
        // A newer entity may have swapped the emitter's loop already.
        if self.router.active_loop(&emitter).map(|l| l.id) == Some(id) {
            let outcome = self
                .router
                .stop_loop(&emitter, EmitterStopReason::Despawned);
            self.outcomes.push(outcome);
        }
    }
}

/// Binds added emitters and stops despawned ones; removals first.
///
/// A world with no [`AudioSession`] syncs nothing: the loading handoff
/// installs the session when a load is delivered
/// ([`insert_audio_session`](super::insert_audio_session)), and a world that
/// has loaded nothing has no audio to bind. The system therefore tolerates the
/// resource's absence instead of aborting the frame, which is what lets
/// [`AudioPlugin`](super::AudioPlugin) register it unconditionally.
pub fn sync_emitter_loops(
    mut session: Option<ResMut<AudioSession>>,
    mut removed: RemovedComponents<AudioEmitterBinding>,
    added: Query<(Entity, &AudioEmitterBinding), Added<AudioEmitterBinding>>,
) {
    let Some(session) = session.as_mut() else {
        return;
    };
    for entity in removed.read() {
        session.unbind(entity);
    }
    for (entity, binding) in &added {
        session.bind(entity, binding);
    }
}

/// Advances the radio queue to the session tick; completion is tick-driven.
///
/// Like [`sync_emitter_loops`], a world with no session advances nothing.
pub fn advance_radio(mut session: Option<ResMut<AudioSession>>) {
    let Some(session) = session.as_mut() else {
        return;
    };
    let now = session.tick;
    let events = session.radio.advance(now);
    session.radio_events.extend(events);
}
