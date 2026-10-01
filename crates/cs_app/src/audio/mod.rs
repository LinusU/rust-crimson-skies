//! The audio application boundary: declared records lowered into runtime
//! routing records and the generation-stamped ECS emitter binding (F41-A).
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! This module sits between the declared audio catalog
//! ([`cs_content::audio`]) and the session router
//! ([`cs_sim::audio_events`]), which cannot see each other — `cs_sim` must not
//! depend on `cs_content` (`docs/01-ARCHITECTURE.md`). It contains:
//!
//! * [`lower::lower_record`] / [`lower::lower_catalog`] — the conversion
//!   boundary: a validated declared record becomes the runtime
//!   [`cs_sim::audio_events::AudioAssetSpec`], with every mandatory playback
//!   field that is [`Resolved::Unknown`] **refused** by claim rather than
//!   played as a guessed bus, level or loop mode;
//! * [`AudioEmitterBinding`] — the ECS record tying an entity to its
//!   session-qualified [`cs_sim::audio_events::AudioEmitterId`], bus and asset,
//!   generation-stamped like [`crate::scene::SceneNodeBinding`] and
//!   [`crate::damage::DamageActorBinding`] so a reload can never leave a stale
//!   binding looking live.
//!
//! Nothing here owns audio state: the one-shot ledger, the loop registry and
//! the actual mixing are F41-B's; these are the conversion and binding records
//! its wiring consumes.
//!
//! Every value is newly authored project design; no original audio was read.

pub mod loops;
pub mod lower;

pub use loops::{AudioSession, LOOP_PRODUCER, LoopRefusal, advance_radio, sync_emitter_loops};

pub use lower::{
    AudioLowerError, LoweredAudioAsset, lower_bus, lower_catalog, lower_mode, lower_record,
};

use bevy::ecs::component::Component;
use cs_sim::audio_events::{AudioBus, AudioEmitterId};
use cs_types::content::ContentId;

use crate::scene::SceneGeneration;

/// Component: marks an entity as the emitter of one audio loop.
///
/// `emitter` is the session-qualified [`AudioEmitterId`] the router keys the
/// loop by, `bus` and `asset` name what plays, and `generation` is the scene
/// generation the binding was spawned under — so a reload stamps new bindings
/// and stale ones are identified by mismatch, never by surviving pointers (the
/// `STATE-TRANSACTIONS` session-generation discipline; the same rule
/// [`crate::damage::DamageActorBinding`] follows).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct AudioEmitterBinding {
    /// The router emitter this entity drives.
    pub emitter: AudioEmitterId,
    /// The bus the loop mixes on.
    pub bus: AudioBus,
    /// The audio asset the loop plays.
    pub asset: ContentId,
    /// The scene generation that spawned the binding.
    pub generation: SceneGeneration,
}
