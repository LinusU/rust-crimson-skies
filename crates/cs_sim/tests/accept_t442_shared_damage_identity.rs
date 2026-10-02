//! T-442 acceptance: the damage node key and the damage actor/event identity
//! are the shared `cs_types` types, not damage-local look-alikes.
//!
//! Task test prefix: `accept_t442_`.
//!
//! The discriminating property is *type identity*, not a field-by-field
//! comparison: every value is handed to a function whose parameter names the
//! `cs_types` type (`cs_types::net::ActorId`, `cs_types::net::EventId`,
//! `cs_types::content::DamageNodeKey`). If `cs_sim::damage` ever reverted to
//! its own structs, these tests would fail to compile. The resolver run keeps
//! the check on production code that actually emits the identities rather than
//! on free functions alone.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_sim::damage::{
    ActorId, AttributionRule, DamageChannel, DamageEventId, DamageEventKind, DamageNodeKey,
    DamagePolicy, DamageResolver, HitEvent, HitEventId, LifecycleKind, SYNTHETIC_HULL_INTEGRITY,
    SYNTHETIC_HULL_NODE, synthetic_airframe_graph,
};
use cs_types::Tick;
use cs_types::content::DamageNodeKey as SharedNodeKey;
use cs_types::net::{ActorId as SharedActorId, EventId as SharedEventId, SessionId};

const SESSION: u64 = 9;
const PRODUCER: u32 = 2;

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

// These four adapters name the shared `cs_types` type in their signature. A
// damage-local struct would not coerce, so a reverted definition fails to
// compile here.
fn as_shared_actor(id: ActorId) -> SharedActorId {
    id
}

fn as_shared_hit(id: HitEventId) -> SharedEventId {
    id
}

fn as_shared_damage_event(id: DamageEventId) -> SharedEventId {
    id
}

fn as_shared_node_key(key: DamageNodeKey) -> SharedNodeKey {
    key
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session(SESSION),
        serial,
    }
}

fn lethal_hit(sequence: u32, target: ActorId) -> HitEvent {
    HitEvent::try_new(
        HitEventId {
            session: session(SESSION),
            tick: Tick(0),
            producer: PRODUCER,
            sequence,
        },
        Some(actor(1)),
        target,
        DamageNodeKey::new(SYNTHETIC_HULL_NODE).expect("the fixture hull key is valid"),
        DamageChannel::Internal,
        SYNTHETIC_HULL_INTEGRITY + 1.0,
    )
    .expect("the fixture hit is well-formed")
}

/// The damage-facing names are the shared types: a `cs_sim::damage` actor,
/// hit id, damage-event id and node key each pass through a signature that
/// names the `cs_types` type, and compare equal to a literal of that type.
#[test]
fn accept_t442_damage_ids_are_the_shared_types() {
    let actor = actor(3);
    let hit = HitEventId {
        session: session(SESSION),
        tick: Tick(4),
        producer: PRODUCER,
        sequence: 7,
    };
    let node = DamageNodeKey::new(SYNTHETIC_HULL_NODE).expect("the fixture hull key is valid");

    assert_eq!(
        as_shared_actor(actor),
        SharedActorId {
            session: session(SESSION),
            serial: 3,
        },
        "a damage actor is the shared cs_types::net::ActorId"
    );
    assert_eq!(
        as_shared_hit(hit),
        SharedEventId {
            session: session(SESSION),
            tick: Tick(4),
            producer: PRODUCER,
            sequence: 7,
        },
        "a damage hit id is the shared cs_types::net::EventId"
    );
    assert_eq!(
        as_shared_damage_event(hit),
        as_shared_hit(hit),
        "HitEventId and DamageEventId are the one shared EventId"
    );
    assert_eq!(
        as_shared_node_key(node.clone()),
        SharedNodeKey::new(SYNTHETIC_HULL_NODE).expect("the fixture hull key is valid"),
        "a damage node key is the shared cs_types::content::DamageNodeKey"
    );
}

/// The resolver stamps its emitted identities with the shared types: the
/// per-node event carries the shared node key, the destruction lifecycle
/// event carries the shared actor identity, and the event id is the shared
/// `EventId`. Removing the resolver's lethal path, or reverting any of the
/// four identities, fails this test.
#[test]
fn accept_t442_resolver_emits_shared_identities() {
    let target = actor(1);
    let mut resolver = DamageResolver::new(session(SESSION), PRODUCER);
    resolver
        .register_actor(
            target,
            synthetic_airframe_graph(),
            DamagePolicy {
                attribution: AttributionRule::FirstLethalHit,
            },
        )
        .expect("the synthetic actor registers");

    let resolution = resolver
        .resolve(Tick(0), std::slice::from_ref(&lethal_hit(0, target)))
        .expect("the batch resolves");

    // Every emitted event carries the shared `EventId`.
    for event in &resolution.events {
        let stamped = as_shared_damage_event(event.id);
        assert_eq!(
            stamped.session,
            session(SESSION),
            "every damage event is stamped with the shared session"
        );
        assert_eq!(stamped.tick, Tick(0));
    }

    let applied = resolution
        .events
        .iter()
        .find_map(|event| match &event.kind {
            DamageEventKind::HitApplied { node, .. } => Some(node.clone()),
            _ => None,
        })
        .expect("the lethal hit applies to the hull");
    assert_eq!(
        as_shared_node_key(applied),
        SharedNodeKey::new(SYNTHETIC_HULL_NODE).expect("the fixture hull key is valid"),
        "the applied hit names the shared node key"
    );

    let destroyed = resolution
        .events
        .iter()
        .find_map(|event| match &event.kind {
            DamageEventKind::Lifecycle { actor, kind } if *kind == LifecycleKind::Destroyed => {
                Some(*actor)
            }
            _ => None,
        })
        .expect("the lethal hit destroys the target");
    assert_eq!(
        as_shared_actor(destroyed),
        actor(1),
        "the destruction lifecycle names the shared actor identity"
    );
}
