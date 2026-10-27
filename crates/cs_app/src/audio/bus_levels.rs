//! The per-bus levels of the accessibility settings, applied to the mix (F52-W2).
//!
//! [`mix_session`](super::mixer::mix_session) multiplies each placed emitter's
//! gain by its bus's [`Presentation::bus_level`](cs_content::settings::Presentation)
//! read from the live session, so a level the player sets applies on the next
//! pass. Only the emitter loops pass through that function today; the radio,
//! music, one-shot and UI paths are named as a limit in
//! `docs/findings/2026-10-27-f52-w2-presentation-output.md`.

use cs_content::audio::{AudioBus as SettingsBus, MAX_AUDIO_GAIN};
use cs_content::settings::Presentation;
use cs_sim::audio_events::{AudioBus, VoiceLevel};

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
