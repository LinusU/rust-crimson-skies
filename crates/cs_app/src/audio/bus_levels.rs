//! The per-bus levels of the accessibility settings, applied to the mix (F52-W2).
//!
//! [`mix_session`](super::mixer::mix_session) hands its device calls a
//! [`BusLevelDevice`] wrapped around the output — loop, radio, music and
//! one-shot voices alike — and every direct device call site in a live world
//! goes through [`with_live_device`](super::mixer::with_live_device), which
//! wraps the same way, so a level the player sets applies on the next voice
//! command on every bus. The front-end `MenuAudio` owns a separate device and
//! applies [`apply_bus_level`] against its own `SharedSettings` copy of the
//! session (main, `f52_w2_menu_audio`), and the player-facing movie voice is
//! scaled by `cinematic_voice_start` (main, `f52_w2_cinematics`); the only
//! voice left outside the live levels is the cinematic *measurement* probe
//! `play_on_device`, which deliberately starts at unity so the timings it
//! reports do not move with the player's settings.

use std::collections::BTreeMap;

use cs_content::audio::{AudioBus as SettingsBus, MAX_AUDIO_GAIN};
use cs_content::settings::Presentation;
use cs_sim::audio_events::{
    AudioBus, AudioDevice, DeviceError, DeviceVoiceId, VoiceLevel, VoiceStart, VoiceStop,
    VoiceUpdate,
};

/// The settings vocabulary's name for a runtime bus.
#[must_use]
pub const fn settings_bus(bus: AudioBus) -> SettingsBus {
    match bus {
        AudioBus::Engine => SettingsBus::Engine,
        AudioBus::Weapons => SettingsBus::Weapons,
        AudioBus::Impacts => SettingsBus::Impacts,
        AudioBus::Environment => SettingsBus::Environment,
        AudioBus::Music => SettingsBus::Music,
        AudioBus::Radio => SettingsBus::Radio,
        AudioBus::Ui => SettingsBus::Ui,
    }
}

/// `level` with its gain scaled by `bus`'s level; pitch is untouched.
///
/// Both factors are valid on their own, so a product above the mixer's ceiling
/// is held at [`MAX_AUDIO_GAIN`] rather than handed to the mixer to refuse.
#[must_use]
pub fn apply_bus_level(
    presentation: &Presentation,
    bus: AudioBus,
    level: VoiceLevel,
) -> VoiceLevel {
    let factor = presentation.bus_level(settings_bus(bus)).get();
    VoiceLevel {
        gain: (level.gain * factor).min(MAX_AUDIO_GAIN),
        pitch: level.pitch,
    }
}

/// An [`AudioDevice`] that scales every voice command by the live session's
/// bus levels (F52-W2-wire).
///
/// A voice start carries its [`AudioBus`], so the wrapper scales the gain it
/// forwards and records the voice's bus in `opened`: a [`VoiceUpdate`] carries
/// no bus of its own, so the bus a voice started on is the only place its
/// update's level can come from. The presentation itself is a copy the caller
/// made for this one pass — the session stays the settings' one owner, and
/// the next call reads it again rather than mixing on a stale snapshot.
///
/// The wrapper exists for one call: the world's [`AudioOutput`] keeps `opened`
/// and lends the device, so a bus table survives across passes while the
/// settings never do.
///
/// [`AudioOutput`]: super::mixer::AudioOutput
#[derive(Debug)]
pub struct BusLevelDevice<'a> {
    inner: &'a mut dyn AudioDevice,
    presentation: Presentation,
    opened: &'a mut BTreeMap<DeviceVoiceId, AudioBus>,
}

impl<'a> BusLevelDevice<'a> {
    /// Wraps `inner`, scaling every command by `presentation`'s bus levels and
    /// keeping the voice→bus record in `opened`.
    #[must_use]
    pub fn new(
        inner: &'a mut dyn AudioDevice,
        presentation: Presentation,
        opened: &'a mut BTreeMap<DeviceVoiceId, AudioBus>,
    ) -> Self {
        Self {
            inner,
            presentation,
            opened,
        }
    }

    /// `gain` scaled by `bus`'s level under the pass's presentation.
    fn level(&self, bus: AudioBus, gain: f64) -> f64 {
        apply_bus_level(&self.presentation, bus, VoiceLevel { gain, pitch: 1.0 }).gain
    }
}

impl AudioDevice for BusLevelDevice<'_> {
    fn is_open(&self) -> bool {
        self.inner.is_open()
    }

    fn open(&mut self) -> Result<(), DeviceError> {
        self.inner.open()
    }

    fn close(&mut self) {
        self.opened.clear();
        self.inner.close();
    }

    fn start_voice(&mut self, mut start: VoiceStart) -> Result<DeviceVoiceId, DeviceError> {
        let bus = start.bus;
        start.gain = self.level(bus, start.gain);
        let voice = self.inner.start_voice(start)?;
        self.opened.insert(voice, bus);
        Ok(voice)
    }

    fn stop_voice(&mut self, stop: VoiceStop) -> Result<(), DeviceError> {
        self.inner.stop_voice(stop)?;
        self.opened.remove(&stop.voice);
        Ok(())
    }

    fn update_voice(
        &mut self,
        voice: DeviceVoiceId,
        mut update: VoiceUpdate,
    ) -> Result<(), DeviceError> {
        if let Some(bus) = self.opened.get(&voice) {
            update.gain = self.level(*bus, update.gain);
        }
        self.inner.update_voice(voice, update)
    }

    fn start_one_shot(&mut self, start: VoiceStart) -> Result<DeviceVoiceId, DeviceError> {
        // `start_voice` on this wrapper already scales and records the bus;
        // the only difference is which inner entry the command reaches.
        let bus = start.bus;
        let gain = self.level(bus, start.gain);
        let voice = self.inner.start_one_shot(VoiceStart { gain, ..start })?;
        self.opened.insert(voice, bus);
        Ok(voice)
    }
}
