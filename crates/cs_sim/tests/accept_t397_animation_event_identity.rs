//! Acceptance tests T-IDENTITY-IDS: animation event identity **is** the
//! shared `cs_types::net::EventId` (F20-A follow-up 1, resolved by #397).
//!
//! Before this migration `cs_sim::animated_object` defined its own
//! `AnimationEventId { session: u64, tick, producer, sequence }` as a
//! stopgap. The type is now the shared contract `EventId`, and the
//! evaluator's session parameter is the shared nonzero `SessionId`, so an
//! event an `AnimatedObject` emits compares equal to a
//! `cs_types::net::EventId` literal and can be passed to code that names the
//! shared type.
//!
//! These tests are discriminating: if the animation-scoped struct is
//! reinstated, `shared(event.id)` and the alias binding do not compile; if
//! the session rule or the field mapping changes, the equality assertion
//! fails. Every value is newly authored fixture data.

use cs_sim::animated_object::{
    AnimatedObject, AnimationEventId, SYNTHETIC_DOOR_MARKER, SYNTHETIC_DOOR_OPEN_TICK,
    synthetic_door_clip,
};
use cs_types::Tick;
use cs_types::net::{EventId, SessionId};

/// Names the shared type: passing an `AnimationEventId` to this function is
/// what proves the alias is the shared id and not a look-alike struct.
fn shared(id: EventId) -> EventId {
    id
}

/// The evaluator stamps its session as a nonzero `SessionId` and its emitted
/// event id is exactly the shared `EventId`.
#[test]
fn accept_t397_animation_event_id_is_the_shared_event_id() {
    let session = SessionId::new(9).expect("a nonzero session generation");
    let mut object = AnimatedObject::new(synthetic_door_clip(), session, 4);
    let outcome = object
        .advance_to(SYNTHETIC_DOOR_OPEN_TICK, Tick(SYNTHETIC_DOOR_OPEN_TICK))
        .expect("advancing to the open tick");
    assert_eq!(outcome.events.len(), 1);
    let event = &outcome.events[0];
    assert_eq!(event.marker, SYNTHETIC_DOOR_MARKER);

    let expected = EventId {
        session,
        tick: Tick(SYNTHETIC_DOOR_OPEN_TICK),
        producer: 4,
        sequence: 0,
    };
    assert_eq!(
        event.id, expected,
        "an animation event is the shared contract EventId"
    );
    assert_eq!(shared(event.id), expected);
}

/// The `AnimationEventId` name denotes the shared type itself, and the
/// session it carries is the shared nonzero generation.
#[test]
fn accept_t397_animation_event_id_alias_is_the_shared_type() {
    let session = SessionId::new(1).expect("a nonzero session generation");
    let id: AnimationEventId = EventId {
        session,
        tick: Tick(0),
        producer: 0,
        sequence: 0,
    };
    let through_shared: EventId = id;
    assert_eq!(through_shared.session.get(), 1);
}
