//! The audio application boundary: declared records lowered into runtime
//! routing records, the generation-stamped ECS emitter binding, the loop
//! lifecycle, the engine smoothing and the mixer that drives an output device.
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stages
//! `### F41-A`, `### F41-B` and `### F41-D`. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`.
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
//!   binding looking live;
//! * [`loops::AudioSession`] and [`loops::sync_emitter_loops`] — the loop
//!   lifecycle: bindings starting, swapping and stopping loops, every outcome
//!   and refusal recorded by name and never played;
//! * [`spawn::bind_spawned_emitters`] — the spawn/bind path, the one producer
//!   of [`AudioEmitterBinding`]s: an aircraft's engine loop and its
//!   [`EngineVoiceFollow`](engine::EngineVoiceFollow), and the world's
//!   environment loop on the delivered item entity, each named from the
//!   delivered closure's own [`cs_content::audio`] records and given one
//!   session-qualified emitter id for its whole life;
//! * [`engine`] — engine pitch and volume smoothed from the **fixed-tick**
//!   engine spool, so the mix follows measured engine state rather than render
//!   FPS (F41 non-negotiable behavior 1);
//! * [`mixer`] — the consumer that turns the session's outcomes and the spatial
//!   law into device commands, with [`mixer::device_lost`] as the
//!   device-failure path that leaves the simulation untouched (behavior 2);
//! * [`device`] — the audible backend: the one [`AudioDevice`] that opens real
//!   output hardware, gated on the `audio` capability so a machine that cannot
//!   play refuses by name (task #635, F41-D);
//! * [`samples`] — the population pass: the delivered closure's audio members
//!   decoded into the library that same load's device plays from, so a
//!   `VoiceStart` naming a delivered asset finds it (task #652, M01-LC);
//! * [`handoff::insert_audio_session`] and [`AudioPlugin`] — the wiring: the
//!   loading handoff owns the session, and the plugin registers the systems in
//!   the app schedule.
//!
//! Every value here is newly authored project design; no original audio was
//! read. The loop regions remain unknown (no `smpl` chunks in any retail
//! member) and the attenuation law is designed rather than measured; both are
//! recorded in
//! `docs/findings/2026-10-01-f41-b-loops-and-spatial-emitters.md` and gate
//! every fidelity claim.

pub mod audit;
pub mod device;
pub mod engine;
pub mod handoff;
pub mod loops;
pub mod lower;
pub mod mixer;
pub mod plugin;
pub mod samples;
pub mod spawn;

pub use device::{
    AudibleDevice, AudioBackendKind, AudioBackendLog, AudioBackendRefusal, CODE_CAPABILITY_ABSENT,
    CODE_DEVICE_CLOSED, CODE_INVALID_MIX, CODE_NO_OUTPUT_DEVICE, CODE_SAMPLE_UNAVAILABLE,
    CODE_UNKNOWN_VOICE, CapabilityDeclaration, DeviceSampleLibrary, EXIT_MISSING_CAPABILITY,
    InMemorySamples, LoopingVoice, PcmAudio, PcmError, RefusingAudioDevice, SampleLibrary,
    SampleProbe, audibility_exit_code, classify_refusal, open_audible_device, sound_member_pcm,
};
pub use engine::{EngineVoiceFollow, EngineVoices, smooth_engine_voices};
pub use handoff::{
    AudioHandoffLog, AudioHandoffRefusal, AudioInstall, DeclaredAudioCatalog, insert_audio_session,
};
pub use loops::{AudioSession, LOOP_PRODUCER, LoopRefusal, advance_radio, sync_emitter_loops};
pub use lower::{
    AudioLowerError, LoweredAudioAsset, lower_bus, lower_catalog, lower_mode, lower_record,
};
pub use mixer::{
    AudioMixReport, AudioMixing, AudioOutput, AudioSpatial, device_lost, device_restored,
    mix_session,
};
pub use plugin::AudioPlugin;
pub use samples::{AudioSampleSource, CODE_SAMPLE_ABSENT, ContentSampleSource, SampleSource};
pub use spawn::{
    AudioBindLog, AudioBindRecord, AudioBindRefusal, AudioEmitterIds, AudioEmitterRole,
    bind_spawned_emitters,
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
///
/// An emitter the mixer can place also carries a `Transform`, so
/// [`GlobalTransform`](bevy::prelude::GlobalTransform) gives
/// [`mix_session`](mix_session) a pose. One without one is *not* assumed to be
/// at the listener: it is reported as unplaced and keeps its last mix.
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
