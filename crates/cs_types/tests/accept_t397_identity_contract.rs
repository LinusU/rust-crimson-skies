//! Acceptance tests T-IDENTITY-IDS: the shared identity contract types in
//! `cs_types` (`docs/contracts/IDENTITY-CONTENT.md`).
//!
//! `SessionId`, `ActorId` and `EventId` are the single canonical definition
//! of a session generation, an actor inside a session and a semantic event
//! id. F54-A placed them in `cs_types::net`; the simulation and content
//! crates name them instead of carrying their own copies (F20-A follow-up 1,
//! resolved by task #397).
//!
//! These tests drive the production constructors and the host allocator:
//! removing `cs_types::net` fails to compile, and changing the nonzero
//! session rule, the contract field shapes or the allocator's non-recycling
//! contract fails the assertions.

use cs_types::Tick;
use cs_types::net::{ActorAllocator, ActorId, EventId, SessionId};

/// `SessionId` is the contract's session generation and it is **never zero**:
/// zero is the "no session" sentinel, so a default or truncated field can
/// never alias a live session.
#[test]
fn accept_t397_session_ids_are_nonzero_generations() {
    assert_eq!(SessionId::new(0), None, "zero is never a live session");
    let session = SessionId::new(42).expect("a nonzero generation is valid");
    assert_eq!(session.get(), 42);
    assert_eq!(session.to_string(), "session 42");
}

/// `ActorId { session, serial }`: identity is session-qualified, so the same
/// serial under two sessions names two different actors.
#[test]
fn accept_t397_actor_ids_are_session_qualified() {
    let first = SessionId::new(1).expect("a nonzero generation");
    let second = SessionId::new(2).expect("a nonzero generation");
    let a = ActorId {
        session: first,
        serial: 7,
    };
    let b = ActorId {
        session: second,
        serial: 7,
    };
    assert_ne!(a, b, "the same serial in another session is another actor");
    assert_eq!(a.to_string(), "actor 1:7");
}

/// `EventId(session, tick, producer, sequence)`: one definition of event
/// identity, totally ordered by session, then tick, then producer, then
/// sequence.
#[test]
fn accept_t397_event_ids_match_the_contract_shape() {
    let session = SessionId::new(3).expect("a nonzero generation");
    let earlier = EventId {
        session,
        tick: Tick(1),
        producer: 0,
        sequence: 0,
    };
    let later = EventId {
        session,
        tick: Tick(2),
        producer: 0,
        sequence: 0,
    };
    assert!(earlier < later, "the tick orders events within one session");
    assert_eq!(later.session.get(), 3);
    assert_eq!(later.tick, Tick(2));
    assert_eq!(later.producer, 0);
    assert_eq!(later.sequence, 0);
    assert_eq!(later.to_string(), "event 3:2:0:0");
}

/// The host allocator mints nonzero, non-recycled serials for one session:
/// the first serial is 1, so serial 0 is never a live actor and a despawned
/// actor's id can never name a later spawn.
#[test]
fn accept_t397_actor_allocator_mints_nonrecycled_serials() {
    let session = SessionId::new(9).expect("a nonzero generation");
    let mut allocator = ActorAllocator::new(session);
    let first = allocator.allocate().expect("a fresh serial");
    let second = allocator.allocate().expect("a fresh serial");
    assert_eq!(first, ActorId { session, serial: 1 }, "serials start at 1");
    assert_eq!(second, ActorId { session, serial: 2 });
    assert_ne!(first, second, "serials are never recycled");
    assert_eq!(allocator.session(), session);
}
