//! Acceptance tests T-IDENTITY-AUDIO (#496): audio event and emitter identity
//! **are** the shared `cs_types::net` `EventId`/`ActorId` (the F41-A gap,
//! closed exactly as #397 closed the animation one).
//!
//! Before this migration `cs_sim::audio_events` defined its own
//! `AudioEventId { session: u64, tick, producer, sequence }` and
//! `AudioEmitterId { session: u64, serial }` as stopgaps. Both names are now
//! aliases of the shared contract types, and the router, mixer, radio queue
//! and music director are bound to the shared nonzero `SessionId`, so an
//! `AudioEventId` an `AudioRouter` accepts compares equal to a
//! `cs_types::net::EventId` literal and can be passed to code that names the
//! shared type.
//!
//! These tests are discriminating: if the audio-scoped structs are
//! reinstated, `shared_event(event.id)` / `shared_actor(emitter)` and the
//! alias bindings do not compile; if the session rule or the field mapping
//! changes, the equality assertions fail. Every value is newly authored
//! fixture data.

use cs_sim::audio_events::{
    AudioBus, AudioEmitterId, AudioEventId, AudioRouter, LoopOutcome, OneShotOutcome,
    synthetic_engine_loop, synthetic_weapon_one_shot,
};
use cs_types::Tick;
use cs_types::net::{ActorId, EventId, SessionId};

/// Names the shared type: passing an `AudioEventId` to this function is what
/// proves the alias is the shared id and not a look-alike struct.
fn shared_event(id: EventId) -> EventId {
    id
}

/// Names the shared type: passing an `AudioEmitterId` to this function is what
/// proves the alias is the shared id and not a look-alike struct.
fn shared_actor(id: ActorId) -> ActorId {
    id
}

/// The router accepts a one-shot stamped with the shared `EventId` shape and
/// reports it back as exactly that shared id — field for field.
#[test]
fn accept_t496_audio_event_id_is_the_shared_event_id() {
    let session = SessionId::new(11).expect("a nonzero session generation");
    let mut router = AudioRouter::new(session);
    let id = AudioEventId {
        session,
        tick: Tick(4),
        producer: 1,
        sequence: 0,
    };
    let event = synthetic_weapon_one_shot(id);
    assert_eq!(
        router.play_one_shot(&event),
        OneShotOutcome::Accepted {
            id,
            asset: event.asset.clone(),
            bus: AudioBus::Weapons,
            gain: 1.0,
        }
    );

    let expected = EventId {
        session,
        tick: Tick(4),
        producer: 1,
        sequence: 0,
    };
    assert_eq!(event.id, expected, "an audio event is the shared EventId");
    assert_eq!(shared_event(event.id), expected);
}

/// The `AudioEmitterId` name denotes the shared `ActorId` itself, and it drives
/// the loop registry as that shared type.
#[test]
fn accept_t496_audio_emitter_id_is_the_shared_actor_id() {
    let session = SessionId::new(11).expect("a nonzero session generation");
    let emitter: AudioEmitterId = ActorId { session, serial: 3 };
    let through_shared: ActorId = emitter;
    assert_eq!(through_shared.session, session);
    assert_eq!(shared_actor(emitter).serial, 3);

    let mut router = AudioRouter::new(session);
    let bind = AudioEventId {
        session,
        tick: Tick(0),
        producer: 0,
        sequence: 0,
    };
    assert_eq!(
        router.start_loop(&synthetic_engine_loop(emitter, bind)),
        LoopOutcome::Started { emitter },
        "the shared ActorId keys the loop registry"
    );
}

/// The session boundary is the shared nonzero `SessionId`: the router reports
/// it as that type, and a foreign generation is still refused by name with the
/// router's `SessionId` — never aliased, and the zero sentinel cannot be built.
#[test]
fn accept_t496_session_boundary_is_the_shared_nonzero_session_id() {
    assert_eq!(
        SessionId::new(0),
        None,
        "the zero sentinel cannot be a session"
    );
    let session = SessionId::new(11).expect("a nonzero session generation");
    let mut router = AudioRouter::new(session);
    assert_eq!(router.session(), session);

    let foreign_id = AudioEventId {
        session: SessionId::new(12).expect("a nonzero session generation"),
        tick: Tick(4),
        producer: 1,
        sequence: 0,
    };
    let foreign = synthetic_weapon_one_shot(foreign_id);
    assert_eq!(
        router.play_one_shot(&foreign),
        OneShotOutcome::RefusedForeignSession {
            id: foreign_id,
            session,
        },
        "a foreign session is refused by the shared SessionId, not a bare u64"
    );
}
