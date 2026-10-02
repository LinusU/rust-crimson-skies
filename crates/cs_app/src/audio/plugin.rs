//! The audio plugin: what the app schedule runs, and in what order (F41-B
//! follow-up #445).
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-B`. F41-B recorded as unmet that "the system is not yet registered
//! in the app schedule (no `AudioSession` is inserted by the loading handoff)";
//! this module is that wiring, in one place.
//!
//! # The schedule
//!
//! ```text
//! PreUpdate   insert_audio_session   the load's handoff owns the session
//! FixedUpdate smooth_engine_voices   one fixed tick of throttle state
//! Update      sync_emitter_loops  ->  advance_radio  ->  mix_session
//! ```
//!
//! Three of those orderings are load-bearing:
//!
//! * `insert_audio_session` runs **before** the loop systems, so the session the
//!   load owns exists before anything reads it, and a reload's fresh session is
//!   never a frame behind.
//! * `smooth_engine_voices` runs in **`FixedUpdate`** and reads `Time<Fixed>`,
//!   never a render delta: engine pitch and volume follow measured engine state
//!   at a fixed rate, so a frame-rate change cannot change the mix (F41
//!   non-negotiable behavior 1).
//! * `mix_session` runs **after** `sync_emitter_loops`, so the loops of this
//!   frame are the ones the mixer carries to the device.
//!
//! # What the plugin does not do
//!
//! It opens no device and loads no audio. The output is a
//! [`RecordingAudioDevice`](cs_sim::audio_events::RecordingAudioDevice) until a
//! hardware backend exists (F41-D, which needs the `audio` capability), and the
//! session's specs come from the delivered load, not from this plugin: it holds
//! the declared catalog the loading path publishes and lowers only what the
//! handoff actually delivered.

use std::sync::Mutex;

use bevy::app::{App, Plugin};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::{FixedUpdate, PreUpdate, Update};
use cs_content::audio::AudioCatalog;
use cs_sim::audio_events::{AudioDevice, Listener, SpatialPolicy};

use super::engine::{EngineVoices, smooth_engine_voices};
use super::handoff::{AudioHandoffLog, DeclaredAudioCatalog, insert_audio_session};
use super::loops::{advance_radio, sync_emitter_loops};
use super::mixer::{AudioMixReport, AudioOutput, AudioSpatial, mix_session};

/// Registers the F41 audio systems, the declared catalog the handoff lowers
/// from, and the spatial configuration and device the mixer runs against.
///
/// [`Self::new`] takes the declared catalog and the spatial configuration,
/// because both are content decisions a plugin cannot make: which records exist
/// is the content layer's, and where the listener is and how far sound carries
/// is the mission's. Everything else — the session, the mixer, the smoothed
/// engine voices, the device — is owned by the world this plugin is added to.
pub struct AudioPlugin {
    catalog: AudioCatalog,
    spatial: AudioSpatial,
    device: Mutex<Option<Box<dyn AudioDevice>>>,
}

impl AudioPlugin {
    /// The audio plugin for `catalog`, mixing against `spatial`, mixing to a
    /// [`RecordingAudioDevice`].
    #[must_use]
    pub fn new(catalog: AudioCatalog, spatial: AudioSpatial) -> Self {
        Self {
            catalog,
            spatial,
            device: Mutex::new(None),
        }
    }

    /// Mixes to `device` instead of the default recording device.
    ///
    /// The recording device makes no sound; a hardware backend is F41-D's and
    /// needs the `audio` capability. This exists so the wiring has one place a
    /// backend is plugged in.
    ///
    /// The device is *moved* into the world the plugin builds, so a plugin
    /// value installs one device and no more: `build` is called once per
    /// `add_plugins`, and a plugin added twice has no device left to give.
    #[must_use]
    pub fn with_device(self, device: Box<dyn AudioDevice>) -> Self {
        Self {
            catalog: self.catalog,
            spatial: self.spatial,
            device: Mutex::new(Some(device)),
        }
    }
}

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(DeclaredAudioCatalog::new(self.catalog.clone()));
        app.insert_resource(self.spatial);
        let device = self
            .device
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        app.insert_resource(match device {
            Some(device) => AudioOutput::new(device),
            None => AudioOutput::default(),
        });
        app.init_resource::<AudioHandoffLog>();
        app.init_resource::<AudioMixReport>();
        app.init_resource::<EngineVoices>();
        app.add_systems(PreUpdate, insert_audio_session);
        app.add_systems(FixedUpdate, smooth_engine_voices);
        app.add_systems(
            Update,
            (sync_emitter_loops, advance_radio, mix_session).chain(),
        );
    }
}

/// The designed spatial configuration the fixture worlds use: full gain inside
/// 10 m, inverse distance to 100 m, silence beyond, and a listener at the
/// world origin facing canonical `+X` as right.
///
/// Designed, not measured: the original attenuation curve is unmeasured (see
/// `docs/findings/2026-10-01-f41-b-loops-and-spatial-emitters.md`), so this is
/// a fixture convenience, not a claim about the original game.
#[must_use]
pub fn designed_spatial(policy: SpatialPolicy, listener: Listener) -> AudioSpatial {
    AudioSpatial::new(policy, listener)
}
