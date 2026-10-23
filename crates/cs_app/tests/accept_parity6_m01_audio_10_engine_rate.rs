//! PARITY6-M01-AUDIO-RESIDUAL (#1955): the player engine loop's rate and
//! volume follow the original's measured `player.zrd` `engine_sound` pairs
//! instead of the designed smoothing law.
//!
//! Task test prefix: `accept_parity6_m01_audio_10_`. Finding:
//! `docs/findings/2026-10-23-m01-audio-residual-engine-rate.md`.
//!
//! The retail scenario reads the installation named by `CS_GAME_DIR` and
//! fails loudly without it; it runs the production measurement
//! (`measure_launch_sound`).

use std::path::{Path, PathBuf};

use bevy::ecs::system::RunSystemOnce;
use bevy::ecs::world::World;
use bevy::time::{Fixed, Time};
use cs_app::audio::{
    AudioEmitterBinding, EngineVoiceFollow, EngineVoices, MeasuredEngineRate,
    measured_voice::MeasuredVoice, smooth_engine_voices,
};
use cs_app::mission_session::{
    LAUNCH_SOUND_CONTAINER, SoundArchive, engine_rate_law, measure_launch_sound,
};
use cs_app::physics::FlightAircraft;
use cs_app::scene::SceneGeneration;
use cs_content::stunts::ZrdValue;
use cs_sim::audio_events::{AudioBus, EngineRateLaw, EngineSmoothing};
use cs_sim::flight::{EngineState, FlightEnvironment, FlightModel, synthetic_fixed_wing};
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::{ActorId, SessionId};

/// The retail pairs: `min/max_throttle_volume` and `min/max_throttle_pitch`.
fn retail_law() -> EngineRateLaw {
    EngineRateLaw::try_new((0.1, 1.0), (1.0, 1.0), (0.1, 0.6), (1.0, 1.0)).expect("a valid law")
}

#[test]
fn accept_parity6_m01_audio_10_the_law_interpolates_the_measured_pairs() {
    let law = retail_law();
    for throttle in [0.0, 0.05, 0.1] {
        assert!((law.pitch_at(throttle) - 0.6).abs() < 1e-12, "{throttle}");
    }
    assert!((law.pitch_at(1.0) - 1.0).abs() < 1e-12);
    assert!((law.pitch_at(2.0) - 1.0).abs() < 1e-12, "clamped above");
    // Halfway through 0.1..1.0 is 0.55: 0.6 + 0.4 * 0.5.
    assert!((law.pitch_at(0.55) - 0.8).abs() < 1e-12);
    assert!((law.gain_at(0.0) - 1.0).abs() < 1e-12);
    assert!((law.gain_at(1.0) - 1.0).abs() < 1e-12);
    // A stopped engine is silent; a corrupt throttle reads as zero.
    assert_eq!(law.target(false, 1.0).gain, 0.0);
    assert!((law.pitch_at(f64::NAN) - 0.6).abs() < 1e-12);
}

#[test]
fn accept_parity6_m01_audio_10_the_law_refuses_a_corrupt_pair() {
    assert!(EngineRateLaw::try_new((1.0, 1.0), (1.0, 1.0), (0.1, 0.6), (1.0, 1.0)).is_err());
    assert!(EngineRateLaw::try_new((0.1, 1.0), (1.0, 1.0), (1.0, 0.6), (0.1, 1.0)).is_err());
    assert!(EngineRateLaw::try_new((0.1, 1.0), (1.0, 1.0), (0.1, 0.0), (1.0, 1.0)).is_err());
    assert!(EngineRateLaw::try_new((0.1, 1.0), (1.0, 1.0), (0.1, f64::NAN), (1.0, 1.0)).is_err());
}

fn text(s: &str) -> ZrdValue {
    ZrdValue::Text(s.to_owned())
}

fn pair(key: &str, x: f32, y: f32) -> [ZrdValue; 2] {
    [
        text(key),
        ZrdValue::List(vec![ZrdValue::Float(x), ZrdValue::Float(y)]),
    ]
}

#[test]
fn accept_parity6_m01_audio_10_the_player_section_reads_into_the_law() {
    let mut section = Vec::new();
    section.extend(pair("min_throttle_volume", 0.1, 1.0));
    section.extend(pair("max_throttle_volume", 1.0, 1.0));
    section.extend(pair("min_throttle_pitch", 0.1, 0.6));
    section.extend(pair("max_throttle_pitch", 1.0, 1.0));
    let player = ZrdValue::List(vec![text("engine_sound"), ZrdValue::List(section.clone())]);
    let law = engine_rate_law(&player).expect("all four pairs present");
    assert!((law.pitch_at(0.55) - 0.8).abs() < 1e-6);
    // A section missing one pair measures nothing.
    section.truncate(6);
    let partial = ZrdValue::List(vec![text("engine_sound"), ZrdValue::List(section)]);
    assert_eq!(engine_rate_law(&partial), None);
}

fn emitter() -> ActorId {
    ActorId {
        session: SessionId::new(1).expect("a nonzero session"),
        serial: 1,
    }
}

/// Spawns one followed engine emitter over the production aircraft record at
/// `spool`, runs the production system once and returns the voice level.
fn voice_level(measured: bool, with_law: bool, spool: f64) -> cs_sim::audio_events::VoiceLevel {
    let mut world = World::new();
    world.insert_resource(Time::<Fixed>::default());
    world.insert_resource(EngineVoices::default());
    if with_law {
        world.insert_resource(MeasuredEngineRate(retail_law()));
    }
    let mut aircraft = FlightAircraft::new(
        FlightModel::new(synthetic_fixed_wing()),
        FlightEnvironment::SEA_LEVEL,
    )
    .expect("a valid aircraft");
    aircraft
        .set_engine(EngineState::direct(spool))
        .expect("a valid spool");
    let mut entity = world.spawn((
        AudioEmitterBinding {
            emitter: emitter(),
            bus: AudioBus::Engine,
            asset: ContentId::from_source(ContentKind::Sound, "synthetic.engine.loop")
                .expect("a valid id"),
            generation: SceneGeneration(1),
        },
        EngineVoiceFollow::default(),
        aircraft,
    ));
    if measured {
        entity.insert(MeasuredVoice);
    }
    world
        .run_system_once(smooth_engine_voices)
        .expect("the system runs");
    world.resource::<EngineVoices>().level(&emitter())
}

#[test]
fn accept_parity6_m01_audio_10_a_measured_voice_jumps_to_the_law() {
    let level = voice_level(true, true, 0.55);
    assert!((level.pitch - 0.8).abs() < 1e-9, "{level:?}");
    assert!((level.gain - 1.0).abs() < 1e-9, "{level:?}");
    // The designed law is untouched without the marker or without the law.
    let designed = EngineSmoothing::DESIGNED_DEFAULT.target(true, 0.0);
    for (measured, with_law) in [(false, true), (true, false)] {
        let level = voice_level(measured, with_law, 0.55);
        assert!(
            (level.pitch - designed.pitch).abs() < 1e-9,
            "a voice off the measured path starts at the designed idle: {level:?}"
        );
    }
}

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this test"),
    )
}

fn launch_sound(root: &Path) -> cs_app::mission_session::LaunchSound {
    let bytes = std::fs::read(root.join(LAUNCH_SOUND_CONTAINER)).expect("soundsh.zbd readable");
    let archives = [SoundArchive {
        key: LAUNCH_SOUND_CONTAINER.to_owned(),
        sha256: cs_assets::install::sha256(&bytes).to_hex(),
    }];
    measure_launch_sound(root, "pdevastator", &archives).expect("the launch sound measures")
}

/// `measure_launch_sound` reads the retail `player.zrd` pairs: pitch 0.6 at
/// throttle 0.1 and below, 1.0 at full throttle, volume 1.0 throughout.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity6_m01_audio_10_retail_player_zrd_measures_the_engine_rate_law() {
    let launch = launch_sound(&game_dir());
    let law = launch
        .engine_rate
        .expect("player.zrd carries all four pairs");
    assert!((law.pitch_at(0.1) - 0.6).abs() < 1e-6);
    assert!((law.pitch_at(1.0) - 1.0).abs() < 1e-6);
    assert!((law.gain_at(0.0) - 1.0).abs() < 1e-6);
}
