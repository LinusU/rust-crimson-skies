//! Acceptance task F52-W2: the presentation settings reach the real output
//! (`docs/findings/2026-10-27-f52-w2-presentation-output.md`).
//!
//! Every test reads a production `SettingsSession` and drives the production
//! presenter: the effect producer, the colour filter, the HUD layout and the
//! `mix_session` system on a recording device. Nothing asserts a pixel on a
//! screen or a sound in a room; that is the `gpu`/`audio` evidence the finding
//! records as outstanding.

use std::path::PathBuf;

use bevy::app::App;
use bevy::prelude::{MinimalPlugins, Transform, TransformPlugin};
use cs_app::accessibility::motion::Effect;
use cs_app::accessibility::session::SettingsSession;
use cs_app::audio::{AudioEmitterBinding, AudioMixing, AudioPlugin, AudioSession, AudioSpatial};
use cs_app::camera::effects::{EffectFrame, GameEvent};
use cs_app::render::presentation::{FilterError, LivePresentation, filter_frame, present_frame};
use cs_app::scene::SceneGenerations;
use cs_app::ui::hud::presentation::{HudPresentation, SubtitleLine};
use cs_content::audio::{AudioBus as SettingsBus, AudioLevel, declared_synthetic_audio_catalog};
use cs_content::settings::{ColourFilter, Settings};
use cs_sim::audio_events::{
    AudioBus, AudioEmitterId, AudioRouter, DeviceCommand, Listener, PlaybackMode,
    RecordingAudioDevice, SpatialPolicy,
};
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;

fn session_at(name: &str) -> SettingsSession {
    let path: PathBuf = std::env::temp_dir().join(format!(
        "cs-f52-w2-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&path);
    SettingsSession::open(path, false)
}

#[test]
fn accept_f52_w2_a_hit_shakes_and_flashes_only_while_the_switches_are_off() {
    let mut session = session_at("effects");
    let events = [
        GameEvent::PlayerHit { damage: 10.0 },
        GameEvent::TargetChanged,
    ];
    let full = EffectFrame::present(&session, &events);
    assert!(full.shake() > 0.0 && full.flash() > 0.0, "{full:?}");
    assert!(full.presented.contains(&Effect::DamageNotice));
    assert!(full.presented.contains(&Effect::TargetNotice));

    // The change applies to the very next frame of the same session.
    session
        .apply(|s| {
            s.presentation.reduce_shake = true;
            s.presentation.reduce_flash = true;
        })
        .expect("apply");
    let calm = EffectFrame::present(&session, &events);
    assert_eq!(calm.shake(), 0.0);
    assert_eq!(calm.flash(), 0.0);
    assert_eq!(
        calm.presented,
        vec![Effect::DamageNotice, Effect::TargetNotice],
        "the required notices survive"
    );
}

#[test]
fn accept_f52_w2_an_undrawable_event_is_reported_not_swallowed() {
    let session = session_at("bad-effect");
    let frame = EffectFrame::present(
        &session,
        &[
            GameEvent::Impact { strength: f32::NAN },
            GameEvent::WeaponFlash { intensity: -1.0 },
            GameEvent::Impact { strength: 0.5 },
        ],
    );
    assert_eq!(frame.errors.len(), 2, "{frame:?}");
    assert_eq!(frame.shake(), 0.5, "the drawable event is still presented");
}

#[test]
fn accept_f52_w2_the_colour_filter_changes_the_drawn_frame_and_never_alpha() {
    let mut session = session_at("filter");
    let original = [200_u8, 40, 40, 255, 10, 220, 30, 128];
    let mut live = LivePresentation(session_at("filter-live"));

    let mut frame = original;
    present_frame(&live, &mut frame).expect("filter off");
    assert_eq!(frame, original, "off leaves the frame untouched");

    live.0
        .apply(|s| s.presentation.colour_filter = ColourFilter::Monochrome)
        .expect("apply");
    present_frame(&live, &mut frame).expect("filter on");
    assert_ne!(frame, original);
    assert_eq!(frame[0], frame[1]);
    assert_eq!(frame[1], frame[2]);
    assert_eq!((frame[3], frame[7]), (255, 128), "alpha is untouched");

    for filter in [
        ColourFilter::Protanopia,
        ColourFilter::Deuteranopia,
        ColourFilter::Tritanopia,
    ] {
        let mut f = original;
        filter_frame(filter, &mut f).expect("filter");
        assert_ne!(f, original, "{filter:?}");
    }

    let mut ragged = [1_u8, 2, 3];
    assert_eq!(
        filter_frame(ColourFilter::Monochrome, &mut ragged),
        Err(FilterError::NotRgba { len: 3 })
    );
    assert_eq!(ragged, [1, 2, 3], "a refused frame is unchanged");
    drop(session.teardown());
}

#[test]
fn accept_f52_w2_ui_scale_and_subtitles_reach_the_hud_layout() {
    let mut session = session_at("hud");
    let lines = [
        SubtitleLine {
            speaker: "Fitzroy".into(),
            text: "Form up on me.".into(),
        },
        SubtitleLine {
            speaker: "Static".into(),
            text: "  ".into(),
        },
    ];
    session
        .apply(|s| s.presentation.subtitles = false)
        .expect("apply");
    let off = HudPresentation::from_session(&session, &lines);
    assert!(off.subtitles.is_empty() && off.errors.is_empty());

    session
        .apply(|s| {
            s.presentation.subtitles = true;
            s.presentation.ui_scale_percent = 200;
        })
        .expect("apply");
    let on = HudPresentation::from_session(&session, &lines);
    assert_eq!(on.subtitles, vec![lines[0].clone()]);
    assert_eq!(on.errors.len(), 1, "the empty line is reported");
    assert_eq!(on.ui_scale_percent, 200);
    assert!(
        on.text_px > off.text_px,
        "{} vs {}",
        on.text_px,
        off.text_px
    );
}

/// The mix pass reads the live session: the gain the recording device is asked
/// for follows the engine bus level, and a change applies on the next pass.
#[test]
fn accept_f52_w2_the_bus_level_reaches_the_mixer_and_a_change_applies_next_pass() {
    let session_id = SessionId::new(0x52).expect("session");
    let key = ContentId::from_source(ContentKind::Sound, "synthetic.engine.loop").expect("id");
    let device = RecordingAudioDevice::new();
    let spy = device.handle();
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        AudioPlugin::new(
            declared_synthetic_audio_catalog(),
            AudioSpatial::new(
                SpatialPolicy::try_new(10.0, 100.0).expect("policy"),
                Listener::try_new([0.0; 3], [1.0, 0.0, 0.0]).expect("listener"),
            ),
        )
        .with_device(Box::new(device)),
    ));
    app.init_resource::<SceneGenerations>();
    app.world_mut()
        .resource_mut::<SceneGenerations>()
        .take_next();
    let generation = app.world().resource::<SceneGenerations>().latest();
    let spec = cs_sim::audio_events::AudioAssetSpec::try_new(
        key.clone(),
        AudioBus::Engine,
        1.0,
        PlaybackMode::Loop,
    )
    .expect("spec");
    app.world_mut().insert_resource(AudioSession::new(
        AudioRouter::new(session_id),
        generation,
        [spec],
    ));
    app.world_mut()
        .insert_resource(AudioMixing::new(session_id));
    app.world_mut().spawn((
        AudioEmitterBinding {
            emitter: AudioEmitterId {
                session: session_id,
                serial: 1,
            },
            bus: AudioBus::Engine,
            asset: key,
            generation,
        },
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));

    let mut live = LivePresentation(session_at("audio"));
    live.0
        .apply(|s: &mut Settings| {
            s.presentation.bus_levels.insert(
                SettingsBus::Engine,
                AudioLevel::try_new(0.5).expect("level"),
            );
        })
        .expect("apply");
    app.insert_resource(live);

    let last_gain = |spy: &RecordingAudioDevice| {
        spy.commands()
            .iter()
            .rev()
            .find_map(|c| match c {
                DeviceCommand::Started { start, .. } => Some(start.gain),
                DeviceCommand::Updated { update, .. } => Some(update.gain),
                _ => None,
            })
            .expect("the device was asked to start or update the engine voice")
    };
    app.update();
    let half = last_gain(&spy);

    app.world_mut()
        .resource_mut::<LivePresentation>()
        .0
        .apply(|s| {
            s.presentation.bus_levels.insert(
                SettingsBus::Engine,
                AudioLevel::try_new(1.0).expect("level"),
            );
        })
        .expect("apply");
    app.update();
    let full = last_gain(&spy);
    assert!(half > 0.0 && full > 0.0, "{half} {full}");
    assert!(
        (full - 2.0 * half).abs() < 1e-6,
        "unity is twice the half level on the next pass: {half} -> {full}"
    );
}
