//! Acceptance scenarios F41-B: the player aircraft's engine loop ends when the
//! aircraft is destroyed and the replacement aircraft's loop binds.
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-B`. Task test prefix: `accept_f41_b_`. Drives the production
//! [`sync_emitter_loops`] system through a real Bevy `App`. All data is newly
//! authored synthetic fixture content.

use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use cs_app::audio::{
    AudioEmitterBinding, AudioSession, LoopRefusal, lower_catalog, sync_emitter_loops,
};
use cs_app::scene::SceneGeneration;
use cs_content::audio::declared_synthetic_audio_catalog;
use cs_sim::audio_events::{
    AudioAssetSpec, AudioBus, AudioEmitterId, AudioRouter, EmitterStopReason, LoopOutcome,
    PlaybackMode,
};
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;

const SESSION: u64 = 5;
const GEN: SceneGeneration = SceneGeneration(1);

fn session_id() -> SessionId {
    SessionId::new(SESSION).expect("a nonzero session generation")
}

fn engine_asset(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Sound, key).expect("valid id")
}

fn engine_spec(key: &str) -> AudioAssetSpec {
    AudioAssetSpec::try_new(engine_asset(key), AudioBus::Engine, 1.0, PlaybackMode::Loop)
        .expect("valid spec")
}

fn emitter(serial: u64) -> AudioEmitterId {
    AudioEmitterId {
        session: session_id(),
        serial,
    }
}

fn binding(serial: u64, key: &str, generation: SceneGeneration) -> AudioEmitterBinding {
    AudioEmitterBinding {
        emitter: emitter(serial),
        bus: AudioBus::Engine,
        asset: engine_asset(key),
        generation,
    }
}

fn app() -> App {
    let mut app = App::new();
    app.insert_resource(AudioSession::new(
        AudioRouter::new(session_id()),
        GEN,
        [
            engine_spec("synthetic.engine.a"),
            engine_spec("synthetic.engine.b"),
        ],
    ));
    app.add_systems(Update, sync_emitter_loops);
    app
}

fn session(app: &mut App) -> &mut AudioSession {
    app.world_mut().resource_mut::<AudioSession>().into_inner()
}

#[test]
fn accept_f41_b_destroyed_aircraft_ends_loop_and_new_aircraft_binds() {
    let mut app = app();
    let old = app
        .world_mut()
        .spawn(binding(1, "synthetic.engine.a", GEN))
        .id();
    app.update();
    assert_eq!(session(&mut app).router.active_loop_count(), 1);
    session(&mut app).drain();

    // Destroy the player aircraft and spawn its replacement in the same frame.
    app.world_mut().entity_mut(old).despawn();
    app.world_mut().spawn(binding(2, "synthetic.engine.b", GEN));
    app.update();

    let s = session(&mut app);
    let (outcomes, refusals) = s.drain();
    assert!(refusals.is_empty());
    assert_eq!(
        outcomes,
        vec![
            LoopOutcome::Stopped {
                emitter: emitter(1),
                reason: EmitterStopReason::Despawned
            },
            LoopOutcome::Started {
                emitter: emitter(2)
            },
        ]
    );
    assert!(s.router.active_loop(&emitter(1)).is_none());
    assert_eq!(
        s.router.active_loop(&emitter(2)).map(|l| l.asset.clone()),
        Some(engine_asset("synthetic.engine.b"))
    );
    assert_eq!(s.router.active_loop_count(), 1);
}

#[test]
fn accept_f41_b_replacement_on_same_emitter_survives_predecessor_teardown() {
    let mut app = app();
    let old: Entity = app
        .world_mut()
        .spawn(binding(1, "synthetic.engine.a", GEN))
        .id();
    app.update();
    // The replacement binds the same emitter first (a swap) ...
    app.world_mut().spawn(binding(1, "synthetic.engine.b", GEN));
    app.update();
    session(&mut app).drain();

    // ... and only later is the predecessor torn down: that must not silence
    // the replacement's loop.
    app.world_mut().entity_mut(old).despawn();
    app.update();
    let s = session(&mut app);
    assert!(s.drain().0.is_empty());
    assert_eq!(s.router.active_loop_count(), 1);
    assert_eq!(
        s.router.active_loop(&emitter(1)).map(|l| l.asset.clone()),
        Some(engine_asset("synthetic.engine.b"))
    );
}

#[test]
fn accept_f41_b_stale_generation_and_unknown_asset_never_play() {
    let mut app = app();
    app.world_mut()
        .spawn(binding(1, "synthetic.engine.a", SceneGeneration(0)));
    app.world_mut()
        .spawn(binding(2, "synthetic.engine.missing", GEN));
    app.update();
    let s = session(&mut app);
    assert_eq!(s.router.active_loop_count(), 0);
    assert!(matches!(s.refusals[0], LoopRefusal::StaleGeneration { .. }));
    assert!(matches!(s.refusals[1], LoopRefusal::UnknownAsset { .. }));
}

#[test]
fn accept_f41_b_one_shot_spec_cannot_bind_as_a_loop() {
    // The synthetic weapon record lowers to a one-shot: binding it as an
    // emitter loop is refused rather than looped.
    let lowered = lower_catalog(&declared_synthetic_audio_catalog()).expect("lowers");
    let weapon = lowered
        .into_values()
        .map(|l| l.spec)
        .find(|s| s.mode() == PlaybackMode::OneShot)
        .expect("a one-shot");
    let key = weapon.asset().clone();
    let mut app = App::new();
    app.insert_resource(AudioSession::new(
        AudioRouter::new(session_id()),
        GEN,
        [weapon],
    ));
    app.add_systems(Update, sync_emitter_loops);
    app.world_mut().spawn(AudioEmitterBinding {
        emitter: emitter(1),
        bus: AudioBus::Weapons,
        asset: key,
        generation: GEN,
    });
    app.update();
    let s = session(&mut app);
    assert_eq!(s.router.active_loop_count(), 0);
    assert!(matches!(s.refusals[0], LoopRefusal::Spec { .. }));
}
