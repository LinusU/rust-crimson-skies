//! F52-W2: a cinematic voice takes the live Music bus level (#2200).

use cs_app::cinematics::movie_player::cinematic_voice_start;
use cs_content::audio::{AudioBus as SettingsBus, AudioLevel};
use cs_content::settings::Presentation;
use cs_sim::audio_events::AudioBus;
use cs_types::content::{ContentId, ContentKind};

fn media() -> ContentId {
    ContentId::from_source(ContentKind::Video, "chap1").unwrap()
}

#[test]
fn accept_f52_w2_cinematics_voice_start_takes_the_music_bus_level() {
    let mut presentation = Presentation::designed();
    presentation
        .bus_levels
        .insert(SettingsBus::Music, AudioLevel::try_new(0.25).unwrap());
    let start = cinematic_voice_start(&media(), &presentation);
    assert_eq!(start.bus, AudioBus::Music);
    assert!((start.gain - 0.25).abs() < 1e-12, "{}", start.gain);
    assert!((start.pitch - 1.0).abs() < 1e-12);
}

#[test]
fn accept_f52_w2_cinematics_other_buses_do_not_move_the_movie() {
    let mut presentation = Presentation::designed();
    presentation
        .bus_levels
        .insert(SettingsBus::Radio, AudioLevel::try_new(0.0).unwrap());
    let start = cinematic_voice_start(&media(), &presentation);
    assert!((start.gain - 1.0).abs() < 1e-12, "{}", start.gain);
}

#[test]
fn accept_f52_w2_cinematics_designed_defaults_play_at_unity() {
    let start = cinematic_voice_start(&media(), &Presentation::designed());
    assert!((start.gain - 1.0).abs() < 1e-12);
}
