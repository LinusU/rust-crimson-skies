//! Acceptance scenarios F41-A for the runtime audio event identity, one-shot
//! dedup and loop-emitter lifecycle.
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-A`. Task test prefix: `accept_f41_a_`.
//!
//! These tests drive the production public API of `cs_sim::audio_events` from
//! outside the crate. The minimum scenario — a weapon event replayed twice
//! plays one accepted one-shot — is
//! [`accept_f41_a_a_replayed_weapon_event_plays_one_accepted_one_shot`];
//! removing the dedup ledger makes it fail.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_sim::audio_events::{
    AudioBus, AudioEmitterId, AudioEventError, AudioEventId, AudioRouter, EmitterStopReason,
    LoopOutcome, OneShotOutcome, PausePolicy, PlaybackMode, synthetic_engine_loop,
    synthetic_weapon_one_shot,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

const SESSION: u64 = 11;

fn event_id(producer: u32, sequence: u32) -> AudioEventId {
    AudioEventId {
        session: SESSION,
        tick: Tick(4),
        producer,
        sequence,
    }
}

/// **AC01, the stage's minimum scenario:** the same weapon event delivered
/// twice produces exactly one accepted one-shot and one suppressed replay.
#[test]
fn accept_f41_a_a_replayed_weapon_event_plays_one_accepted_one_shot() {
    let mut router = AudioRouter::new(SESSION);
    let event = synthetic_weapon_one_shot(event_id(1, 0));

    let first = router.play_one_shot(&event);
    assert_eq!(
        first,
        OneShotOutcome::Accepted {
            id: event.id,
            asset: event.asset.clone(),
            bus: AudioBus::Weapons,
            gain: 1.0,
        }
    );

    // The very same event id, replayed, must not play a second time.
    let replay = router.play_one_shot(&event);
    assert_eq!(replay, OneShotOutcome::SuppressedDuplicate { id: event.id });

    // Replaying a whole batch of the same event still yields one shot.
    let suppressed = (0..8)
        .filter(|_| {
            matches!(
                router.play_one_shot(&event),
                OneShotOutcome::SuppressedDuplicate { .. }
            )
        })
        .count();
    assert_eq!(suppressed, 8);
}

/// A distinct sequence from the same producer is a genuinely new cue; a stale
/// sequence behind the accepted mark is suppressed rather than replayed.
#[test]
fn accept_f41_a_dedup_tracks_the_highest_sequence_per_producer() {
    let mut router = AudioRouter::new(SESSION);
    assert!(matches!(
        router.play_one_shot(&synthetic_weapon_one_shot(event_id(1, 0))),
        OneShotOutcome::Accepted { .. }
    ));
    assert!(matches!(
        router.play_one_shot(&synthetic_weapon_one_shot(event_id(1, 1))),
        OneShotOutcome::Accepted { .. }
    ));
    // The now-stale sequence 0 is a duplicate of what was already accepted.
    assert_eq!(
        router.play_one_shot(&synthetic_weapon_one_shot(event_id(1, 0))),
        OneShotOutcome::SuppressedDuplicate { id: event_id(1, 0) }
    );
    // A different producer has its own ledger.
    assert!(matches!(
        router.play_one_shot(&synthetic_weapon_one_shot(event_id(2, 0))),
        OneShotOutcome::Accepted { .. }
    ));
}

/// An event from a previous or future session generation is refused by name.
#[test]
fn accept_f41_a_events_from_another_session_are_refused() {
    let mut router = AudioRouter::new(SESSION);
    let mut foreign = synthetic_weapon_one_shot(event_id(1, 0));
    foreign.id.session = SESSION + 1;
    assert_eq!(
        router.play_one_shot(&foreign),
        OneShotOutcome::RefusedForeignSession {
            id: foreign.id,
            session: SESSION
        }
    );
    // A refused event must not have occupied the ledger.
    assert!(matches!(
        router.play_one_shot(&synthetic_weapon_one_shot(event_id(1, 0))),
        OneShotOutcome::Accepted { .. }
    ));
}

/// A loop binds to an idle emitter, swaps cleanly, and a stop names its reason;
/// a double stop is reported, not swallowed.
#[test]
fn accept_f41_a_loop_emitters_bind_swap_and_stop() {
    let emitter = AudioEmitterId {
        session: SESSION,
        serial: 3,
    };
    let mut router = AudioRouter::new(SESSION);

    assert_eq!(
        router.start_loop(&synthetic_engine_loop(emitter, event_id(1, 0))),
        LoopOutcome::Started { emitter }
    );
    assert_eq!(router.active_loop_count(), 1);
    assert!(router.active_loop(&emitter).is_some());

    // A new aircraft binds the same emitter: the old loop is stopped.
    assert_eq!(
        router.start_loop(&synthetic_engine_loop(emitter, event_id(1, 1))),
        LoopOutcome::Swapped {
            emitter,
            stopped: event_id(1, 0)
        }
    );
    assert_eq!(router.active_loop_count(), 1);
    assert_eq!(
        router
            .active_loop(&emitter)
            .map(|loop_binding| loop_binding.id),
        Some(event_id(1, 1))
    );

    // Despawn stops it exactly once.
    assert_eq!(
        router.stop_loop(&emitter, EmitterStopReason::Despawned),
        LoopOutcome::Stopped {
            emitter,
            reason: EmitterStopReason::Despawned
        }
    );
    assert_eq!(router.active_loop_count(), 0);
    assert_eq!(
        router.stop_loop(&emitter, EmitterStopReason::Despawned),
        LoopOutcome::NotActive {
            emitter,
            reason: EmitterStopReason::Despawned
        }
    );
}

/// A foreign-session emitter is refused, so a reload cannot stop a live loop.
#[test]
fn accept_f41_a_loops_refuse_foreign_sessions() {
    let emitter = AudioEmitterId {
        session: SESSION,
        serial: 1,
    };
    let mut router = AudioRouter::new(SESSION);
    let mut foreign = synthetic_engine_loop(emitter, event_id(1, 0));
    foreign.emitter.session = SESSION + 1;
    assert_eq!(
        router.start_loop(&foreign),
        LoopOutcome::RefusedForeignSession {
            emitter: foreign.emitter,
            session: SESSION
        }
    );
    assert_eq!(
        router.stop_loop(&foreign.emitter, EmitterStopReason::Despawned),
        LoopOutcome::RefusedForeignSession {
            emitter: foreign.emitter,
            session: SESSION
        }
    );
}

/// The declared pause policy suspends or keeps loops; a device loss always
/// stops them and is total.
#[test]
fn accept_f41_a_pause_policy_and_device_loss_stop_loops() {
    let emitter = |serial| AudioEmitterId {
        session: SESSION,
        serial,
    };
    let mut router = AudioRouter::new(SESSION);
    router.start_loop(&synthetic_engine_loop(emitter(1), event_id(1, 0)));
    router.start_loop(&synthetic_engine_loop(emitter(2), event_id(1, 1)));

    assert!(router.apply_pause(PausePolicy::Continue).is_empty());
    assert_eq!(router.active_loop_count(), 2, "Continue keeps the loops");

    let suspended = router.apply_pause(PausePolicy::Suspend);
    assert_eq!(suspended.len(), 2);
    assert!(suspended.iter().all(|outcome| matches!(
        outcome,
        LoopOutcome::Stopped {
            reason: EmitterStopReason::Paused,
            ..
        }
    )));
    assert_eq!(router.active_loop_count(), 0);

    router.start_loop(&synthetic_engine_loop(emitter(1), event_id(1, 2)));
    assert_eq!(
        router.device_lost(),
        vec![LoopOutcome::Stopped {
            emitter: emitter(1),
            reason: EmitterStopReason::DeviceLost
        }]
    );
    assert!(router.device_lost().is_empty(), "a second loss is a no-op");
}

/// Inputs are validated at the runtime boundary.
#[test]
fn accept_f41_a_runtime_inputs_are_validated() {
    assert!(matches!(
        cs_sim::audio_events::OneShotEvent::try_new(
            event_id(1, 0),
            ContentId::from_source(ContentKind::Sound, "synthetic.x").expect("valid"),
            AudioBus::Weapons,
            f64::NAN
        ),
        Err(AudioEventError::NonFiniteGain { value }) if value.is_nan()
    ));
    assert_eq!(
        cs_sim::audio_events::OneShotEvent::try_new(
            event_id(1, 0),
            ContentId::from_source(ContentKind::Image, "synthetic.x").expect("valid"),
            AudioBus::Weapons,
            1.0
        ),
        Err(AudioEventError::NotAnAudioAsset {
            kind: ContentKind::Image
        })
    );
    assert_eq!(PlaybackMode::OneShot.to_string(), "one_shot");
}
