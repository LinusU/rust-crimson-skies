//! Acceptance scenarios F41-C: radio queue, music transitions and subtitles
//! wired into the audio session; losing the audio device mid-mission leaves
//! simulation and dialogue completion intact.
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-C`. Task test prefix: `accept_f41_c_`. All data is synthetic.

use bevy::app::{App, Update};
use cs_app::audio::{AudioEmitterBinding, AudioSession, advance_radio, sync_emitter_loops};
use cs_app::scene::SceneGeneration;
use cs_sim::audio_events::{
    AudioAssetSpec, AudioBus, AudioEmitterId, AudioEventId, AudioRouter, EmitterStopReason,
    LoopOutcome, MusicCue, MusicOutcome, PlaybackMode, RadioEvent, RadioLine,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

const SESSION: u64 = 9;
const GEN: SceneGeneration = SceneGeneration(1);

fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("valid id")
}

fn eid(producer: u32, sequence: u32) -> AudioEventId {
    AudioEventId {
        session: SESSION,
        tick: Tick(0),
        producer,
        sequence,
    }
}

fn line(sequence: u32, priority: u8, interruptible: bool, ticks: u64) -> RadioLine {
    RadioLine::try_new(
        eid(1, sequence),
        "Nathan",
        id(ContentKind::Dialogue, &format!("synthetic.line.{sequence}")),
        priority,
        interruptible,
        Some(&format!("subtitle {sequence}")),
        ticks,
    )
    .expect("valid line")
}

fn app() -> App {
    let spec = AudioAssetSpec::try_new(
        id(ContentKind::Sound, "synthetic.engine.a"),
        AudioBus::Engine,
        1.0,
        PlaybackMode::Loop,
    )
    .expect("spec");
    let mut app = App::new();
    app.insert_resource(AudioSession::new(AudioRouter::new(SESSION), GEN, [spec]));
    app.add_systems(Update, (sync_emitter_loops, advance_radio));
    app
}

fn set_tick(app: &mut App, tick: u64) {
    app.world_mut().resource_mut::<AudioSession>().tick = Tick(tick);
}

/// Minimum scenario: lose the device mid-line; the line still completes on its
/// tick, the subtitle stays, the engine loop is stopped as DeviceLost and comes
/// back on retry.
#[test]
fn accept_f41_c_device_loss_mid_mission_keeps_dialogue_completing() {
    let mut app = app();
    let emitter = AudioEmitterId {
        session: SESSION,
        serial: 1,
    };
    app.world_mut().spawn(AudioEmitterBinding {
        emitter,
        bus: AudioBus::Engine,
        asset: id(ContentKind::Sound, "synthetic.engine.a"),
        generation: GEN,
    });
    app.world_mut()
        .resource_mut::<AudioSession>()
        .enqueue_radio(line(1, 5, true, 10));
    app.update();
    {
        let s = app.world().resource::<AudioSession>();
        assert!(s.router.active_loop(&emitter).is_some());
        assert_eq!(s.radio.subtitle(), Some("subtitle 1"));
    }

    app.world_mut().resource_mut::<AudioSession>().device_lost();
    {
        let s = app.world().resource::<AudioSession>();
        assert!(s.router.active_loop(&emitter).is_none());
        assert!(s.outcomes.iter().any(|o| matches!(
            o,
            LoopOutcome::Stopped {
                reason: EmitterStopReason::DeviceLost,
                ..
            }
        )));
        assert_eq!(s.radio.subtitle(), Some("subtitle 1"), "subtitle survives");
    }
    // A second loss is a no-op.
    app.world_mut().resource_mut::<AudioSession>().device_lost();

    set_tick(&mut app, 9);
    app.update();
    assert!(
        app.world()
            .resource::<AudioSession>()
            .radio
            .active()
            .is_some()
    );
    set_tick(&mut app, 10);
    app.update();
    let events = app.world_mut().resource_mut::<AudioSession>().drain_radio();
    assert!(
        events.contains(&RadioEvent::PlaybackLost { id: eid(1, 1) }),
        "{events:?}"
    );
    assert!(
        events.contains(&RadioEvent::Completed {
            id: eid(1, 1),
            voiced: false
        }),
        "{events:?}"
    );

    let restart = app
        .world_mut()
        .resource_mut::<AudioSession>()
        .device_restored();
    assert!(restart.is_none(), "no music was playing");
    assert!(
        app.world()
            .resource::<AudioSession>()
            .router
            .active_loop(&emitter)
            .is_some(),
        "retry rebinds the live engine loop"
    );
}

#[test]
fn accept_f41_c_despawn_during_device_loss_is_not_resurrected() {
    let mut app = app();
    let emitter = AudioEmitterId {
        session: SESSION,
        serial: 2,
    };
    let e = app
        .world_mut()
        .spawn(AudioEmitterBinding {
            emitter,
            bus: AudioBus::Engine,
            asset: id(ContentKind::Sound, "synthetic.engine.a"),
            generation: GEN,
        })
        .id();
    app.update();
    app.world_mut().resource_mut::<AudioSession>().device_lost();
    app.world_mut().despawn(e);
    app.update();
    app.world_mut()
        .resource_mut::<AudioSession>()
        .device_restored();
    assert_eq!(
        app.world()
            .resource::<AudioSession>()
            .router
            .active_loop_count(),
        0
    );
}

#[test]
fn accept_f41_c_radio_priority_interrupt_and_ordering() {
    let mut app = app();
    let mut s = app.world_mut().resource_mut::<AudioSession>();
    s.enqueue_radio(line(1, 1, true, 10));
    s.enqueue_radio(line(2, 2, false, 5));
    let events = s.drain_radio();
    assert!(matches!(events[0], RadioEvent::Started { id, .. } if id == eid(1, 1)));
    assert_eq!(
        events[1],
        RadioEvent::Interrupted {
            id: eid(1, 1),
            by: eid(1, 2)
        }
    );
    // Line 2 is not interruptible: later lines wait, highest priority first.
    s.enqueue_radio(line(3, 3, false, 5));
    s.enqueue_radio(line(4, 9, true, 5));
    assert_eq!(s.radio.active().map(|l| l.id), Some(eid(1, 2)));
    assert_eq!(s.radio.pending_len(), 2);
    // A replay of line 4 is suppressed.
    s.enqueue_radio(line(4, 9, true, 5));
    assert!(matches!(
        s.drain_radio().last(),
        Some(RadioEvent::SuppressedDuplicate { .. })
    ));
    s.tick = Tick(5);
    app.update();
    let s = app.world().resource::<AudioSession>();
    assert_eq!(
        s.radio.active().map(|l| l.id),
        Some(eid(1, 4)),
        "priority order"
    );
    set_tick(&mut app, 10);
    app.update();
    let s = app.world().resource::<AudioSession>();
    assert_eq!(s.radio.active().map(|l| l.id), Some(eid(1, 3)));
}

#[test]
fn accept_f41_c_music_transitions_are_authored_and_survive_device_loss() {
    let mut app = app();
    let mut s = app.world_mut().resource_mut::<AudioSession>();
    let a = id(ContentKind::Music, "synthetic.music.a");
    let b = id(ContentKind::Music, "synthetic.music.b");
    let cue = |seq, asset: &ContentId| MusicCue {
        id: eid(2, seq),
        asset: asset.clone(),
    };
    assert_eq!(
        s.request_music(cue(1, &a)),
        MusicOutcome::Started {
            to: a.clone(),
            audible: true
        }
    );
    assert_eq!(
        s.request_music(cue(1, &b)),
        MusicOutcome::SuppressedDuplicate { id: eid(2, 1) }
    );
    s.device_lost();
    assert_eq!(
        s.request_music(cue(2, &b)),
        MusicOutcome::Transition {
            from: a,
            to: b.clone(),
            audible: false
        }
    );
    assert_eq!(s.device_restored().map(|c| c.asset), Some(b));
    assert!(matches!(
        s.request_music(MusicCue {
            id: eid(2, 3),
            asset: id(ContentKind::Sound, "synthetic.not.music")
        }),
        MusicOutcome::RefusedNotMusic { .. }
    ));
}
