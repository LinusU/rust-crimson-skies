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
//! PreUpdate   insert_audio_session    the load's handoff owns the session
//! FixedUpdate smooth_engine_voices     one fixed tick of throttle state
//! Update      bind_spawned_emitters -> sync_emitter_loops -> advance_radio
//!                 -> mix_session
//! ```
//!
//! Four of those orderings are load-bearing:
//!
//! * `insert_audio_session` runs **before** the loop systems, so the session the
//!   load owns exists before anything reads it, and a reload's fresh session is
//!   never a frame behind.
//! * `bind_spawned_emitters` runs **before** `sync_emitter_loops`, so an
//!   aircraft or world emitter spawned this frame gets its binding, its
//!   session-qualified emitter id and (on an aircraft) its engine voice in the
//!   same frame the loop system looks for `Added` bindings — the spawn path and
//!   the loop lifecycle see one spawn, not two frames of it.
//! * `smooth_engine_voices` runs in **`FixedUpdate`** and reads `Time<Fixed>`,
//!   never a render delta: engine pitch and volume follow measured engine state
//!   at a fixed rate, so a frame-rate change cannot change the mix (F41
//!   non-negotiable behavior 1).
//! * `mix_session` runs **after** `sync_emitter_loops`, so the loops of this
//!   frame are the ones the mixer carries to the device.
//!
//! # Which device it mixes to
//!
//! Three, and the world says which in [`AudioBackendLog`]:
//!
//! * [`Self::audible`] asks for the real
//!   [`AudibleDevice`](super::device::AudibleDevice), which opens the machine's
//!   default output stream **only** when `$CS_CAPABILITIES` declares `audio`
//!   (task #635, F41-D). A machine that cannot play gets a
//!   [`RefusingAudioDevice`](super::device::RefusingAudioDevice) reporting the
//!   gate's own named refusal on every command — never a silent mute.
//! * [`Self::with_device`] takes a caller's device as-is.
//! * Neither asks for a backend, and the world is a headless or test one: the
//!   output is a
//!   [`RecordingAudioDevice`](cs_sim::audio_events::RecordingAudioDevice), which
//!   records commands and makes no sound.
//!
//! The plugin loads no audio either: the session's specs come from the delivered
//! load, not from here. It holds the declared catalog the loading path publishes
//! and lowers only what the handoff actually delivered — and
//! [`bind_spawned_emitters`](super::spawn::bind_spawned_emitters) names emitter
//! bindings from exactly that content, never from ids it invents.

use std::sync::{Arc, Mutex};

use bevy::app::{App, Plugin};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::{FixedUpdate, PreUpdate, Update};
use cs_content::audio::AudioCatalog;
use cs_sim::audio_events::AudioDevice;

use super::device::{
    AudioBackendLog, CapabilityDeclaration, DeviceSampleLibrary, RefusingAudioDevice,
    SampleLibrary, open_audible_device,
};
use super::engine::{EngineVoices, smooth_engine_voices};
use super::handoff::{AudioHandoffLog, DeclaredAudioCatalog, insert_audio_session};
use super::loops::{advance_radio, sync_emitter_loops};
use super::mixer::{AudioMixReport, AudioOutput, AudioSpatial, mix_session};
use super::spawn::{AudioBindLog, bind_spawned_emitters};

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
    /// The sample library the audible backend plays from, when the world asked
    /// for real output hardware.
    audible: Option<Arc<dyn SampleLibrary>>,
    /// The capability declaration the gate reads; `None` reads
    /// `$CS_CAPABILITIES` when the plugin is built.
    declaration: Option<CapabilityDeclaration>,
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
            audible: None,
            declaration: None,
        }
    }

    /// Mixes to `device` instead of the default recording device.
    ///
    /// The recording device makes no sound. This exists so the wiring has one
    /// place an alternative device is plugged in; [`Self::audible`] is the
    /// production one (task #635).
    ///
    /// The device is *moved* into the world the plugin builds, so a plugin
    /// value installs one device and no more: `build` is called once per
    /// `add_plugins`, and a plugin added twice has no device left to give.
    #[must_use]
    pub fn with_device(mut self, device: Box<dyn AudioDevice>) -> Self {
        self.device = Mutex::new(Some(device));
        self
    }

    /// Mixes to the real audible backend, playing `library`'s samples.
    ///
    /// This is the production device: the world gets an [`AudibleDevice`] over
    /// the machine's default output stream when — and only when — that machine
    /// declares the `audio` capability. It needs the `audio` capability, which
    /// is exactly F41-D's gate.
    ///
    /// A machine that cannot play is **not** silently muted. The gate refuses by
    /// name, and the world installs a [`RefusingAudioDevice`] carrying that same
    /// refusal, so every mixer pass reports it and
    /// [`AudioBackendLog`] records why. Which backend a world ended up with is
    /// readable from [`AudioBackendLog`] rather than inferred from silence.
    #[must_use]
    pub fn audible(self, library: Arc<dyn SampleLibrary>) -> Self {
        Self {
            audible: Some(library),
            ..self
        }
    }

    /// Reads the capability gate from `declaration` instead of
    /// `$CS_CAPABILITIES`.
    ///
    /// The environment is the production source; this is how a caller states a
    /// machine's capabilities itself, and how a test exercises the absent-
    /// capability refusal without mutating the process environment.
    #[must_use]
    pub fn with_capabilities(mut self, declaration: CapabilityDeclaration) -> Self {
        self.declaration = Some(declaration);
        self
    }
}

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(DeclaredAudioCatalog::new(self.catalog.clone()));
        app.insert_resource(self.spatial);
        let declaration = self
            .declaration
            .clone()
            .unwrap_or_else(CapabilityDeclaration::from_environment);
        let device = self
            .device
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        // One device, and the log says which: an explicitly given device, the
        // real audible backend, or the recording stand-in. The audible branch
        // never falls back to the stand-in on failure — a refused gate installs
        // a device that reports the refusal on every command, because a
        // recording device would make a capability failure look like a mission
        // with nothing to say. The library travels with the audible branch
        // only: it is published as a resource so the load's population pass
        // fills the very object this device reads from, and a world that never
        // asked for audible playback has nothing to fill.
        let (output, backend, samples) = match device {
            Some(device) => (
                AudioOutput::new(device),
                AudioBackendLog::stand_in(&declaration),
                None,
            ),
            None => match &self.audible {
                Some(library) => {
                    let (output, backend) =
                        match open_audible_device(Arc::clone(library), &declaration) {
                            Ok(device) => (
                                AudioOutput::new(Box::new(device)),
                                AudioBackendLog::audible(&declaration),
                            ),
                            Err(error) => (
                                AudioOutput::new(Box::new(RefusingAudioDevice::new(error.clone()))),
                                AudioBackendLog::refused(&declaration, &error),
                            ),
                        };
                    (output, backend, Some(Arc::clone(library)))
                }
                None => (
                    AudioOutput::default(),
                    AudioBackendLog::stand_in(&declaration),
                    None,
                ),
            },
        };
        app.insert_resource(output);
        app.insert_resource(backend);
        if let Some(library) = samples {
            app.insert_resource(DeviceSampleLibrary::new(library));
        }
        app.init_resource::<AudioHandoffLog>();
        app.init_resource::<AudioMixReport>();
        app.init_resource::<AudioBindLog>();
        app.init_resource::<EngineVoices>();
        app.add_systems(PreUpdate, insert_audio_session);
        app.add_systems(FixedUpdate, smooth_engine_voices);
        app.add_systems(
            Update,
            (
                bind_spawned_emitters,
                sync_emitter_loops,
                advance_radio,
                mix_session,
            )
                .chain(),
        );
    }
}
