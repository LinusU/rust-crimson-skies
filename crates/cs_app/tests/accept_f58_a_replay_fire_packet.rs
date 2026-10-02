//! Acceptance scenario F58-A: replay a prior-session fire packet and prove no
//! projectile spawns.
//!
//! Spec: `specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md`,
//! stage `### F58-A`; contract `docs/contracts/UI-NETWORK.md` ("Epoch mismatch
//! rejects stale packets", "Reliable delivery does not replace application
//! idempotency"). Task test prefix: `accept_f58_a_`.
//!
//! The path is the whole designed chain: `cs_app::network::recovery`'s
//! [`SessionReceiver`] admits or refuses the decoded client packet through the
//! `cs_net::validation` session identity gate, turns an admitted fire packet
//! into the F27 request, and the real `cs_sim::weapons::FireResolver` resolves
//! it. A prior-session packet produces no request (and the resolver would
//! refuse a forged one as a foreign session anyway), so nothing spawns.
//!
//! Every value is newly authored synthetic fixture data, never original game
//! data.

use std::collections::BTreeMap;

use cs_app::network::recovery::{SessionReceiver, fire_intent};
use cs_net::message::ClientMessage;
use cs_net::validation::{
    Admission, FireRequest, SessionViolation, ThreatDisposition, synthetic_fire_message,
};
use cs_sim::damage::ActorId as SimActorId;
use cs_sim::weapons::{
    FireResolver, GunBank, IntentRefusal, MountTransform, SYNTHETIC_STARTING_ROUNDS, WeaponState,
    synthetic_gun_definition,
};
use cs_types::Tick;
use cs_types::net::{ActorId, PeerId, SessionId};
use cs_types::space::{UnitVec3, WorldPosition};

fn position(value: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(value).expect("test positions are finite")
}

/// A one-gun resolver fixture for `session`, positioned at `tick`, plus the
/// mount transform its shot needs.
fn resolver_fixture(
    session: SessionId,
    tick: Tick,
    actor: SimActorId,
) -> (
    FireResolver,
    BTreeMap<cs_sim::damage::DamageNodeKey, MountTransform>,
) {
    let gun = synthetic_gun_definition();
    let mount = gun.mount().clone();
    let state = WeaponState::try_new(
        std::slice::from_ref(&gun),
        GunBank::try_new([mount.clone()]).expect("a one-mount bank"),
        SYNTHETIC_STARTING_ROUNDS,
    )
    .expect("the fixture weapon state is valid");
    let mut resolver = FireResolver::new(session.get(), tick);
    resolver
        .register(actor, vec![gun], state)
        .expect("the fixture gun registers");
    let transform = MountTransform::try_new(position([0.0, 0.0, 0.0]), UnitVec3::FORWARD, [0.0; 3])
        .expect("a finite inherited velocity");
    (resolver, BTreeMap::from([(mount, transform)]))
}

/// Resolves every fire request an admitted packet produced and returns how
/// many projectiles the resolver spawned.
fn spawn_from(
    receiver: &mut SessionReceiver,
    peer: PeerId,
    message: &ClientMessage,
    resolver: &mut FireResolver,
    transforms: &BTreeMap<cs_sim::damage::DamageNodeKey, MountTransform>,
) -> usize {
    let inbound = receiver.receive(peer, message);
    let mut spawned = 0;
    for request in inbound.fires() {
        if let Ok(resolution) = resolver.resolve(&fire_intent(request), transforms) {
            spawned += resolution.accepted.len();
        }
    }
    spawned
}

#[test]
fn accept_f58_a_replaying_a_prior_session_fire_packet_spawns_no_projectile() {
    let live = SessionId::new(31).expect("a nonzero session");
    let prior = SessionId::new(30).expect("a nonzero session");
    let peer = PeerId::new(1).expect("a nonzero peer");
    let net_actor = ActorId {
        session: live,
        serial: 4,
    };
    let (mut resolver, transforms) = resolver_fixture(
        live,
        Tick(10),
        SimActorId {
            session: live.get(),
            serial: 4,
        },
    );
    let before = resolver
        .state(&SimActorId {
            session: live.get(),
            serial: 4,
        })
        .expect("registered")
        .ammunition(&synthetic_gun_definition().mount().clone());

    let mut receiver = SessionReceiver::new(live);
    receiver.admit_peer(peer);
    receiver
        .bind_actor(peer, net_actor)
        .expect("the aircraft is free");

    // The prior session's fire packet, replayed verbatim.
    let stale = synthetic_fire_message(prior, Tick(10), 1);
    let inbound = receiver.receive(peer, &stale);
    assert!(!inbound.accepted(), "a prior-session packet is refused");
    match inbound.violation() {
        Some(SessionViolation::StaleSession { expected, found }) => {
            assert_eq!(*expected, live);
            assert_eq!(*found, prior);
        }
        other => panic!("expected a stale-session refusal, got {other:?}"),
    }
    assert_eq!(
        inbound.violation().unwrap().disposition(),
        ThreatDisposition::Absorb
    );
    assert!(
        inbound.fires().is_empty(),
        "a refused packet yields no fire request, so no projectile"
    );
    assert_eq!(
        spawn_from(&mut receiver, peer, &stale, &mut resolver, &transforms),
        0
    );

    // Defence in depth: even a hand-forged prior-session request is refused
    // whole by the authoritative resolver before it can spawn.
    let forged = FireRequest {
        session: prior,
        peer,
        actor: net_actor,
        tick: Tick(10),
        sequence: 1,
    };
    assert!(matches!(
        resolver.resolve(&fire_intent(&forged), &transforms),
        Err(IntentRefusal::ForeignSession { expected, found }) if expected == live.get() && found == prior.get()
    ));
    assert_eq!(
        resolver
            .state(&SimActorId {
                session: live.get(),
                serial: 4,
            })
            .expect("registered")
            .ammunition(&synthetic_gun_definition().mount().clone()),
        before,
        "no round was consumed by the replay"
    );

    // Positive control: a live-session fire packet does spawn exactly one,
    // so the zero above is the replay being refused and not the fixture
    // failing to fire at all.
    let fresh = synthetic_fire_message(live, Tick(10), 1);
    let inbound = receiver.receive(peer, &fresh);
    assert!(inbound.accepted(), "a live-session fire packet is admitted");
    assert_eq!(inbound.fires().len(), 1);
    let request = &inbound.fires()[0];
    assert_eq!(request.session, live);
    assert_eq!(request.actor, net_actor);
    let resolution = resolver
        .resolve(&fire_intent(request), &transforms)
        .expect("the live fire request resolves");
    assert_eq!(
        resolution.accepted.len(),
        1,
        "a live fire request spawns exactly one projectile"
    );
    assert_eq!(
        resolution.accepted[0].projectile.projectile.session,
        live.get()
    );
}

#[test]
fn accept_f58_a_a_replayed_live_fire_packet_spawns_only_once() {
    let live = SessionId::new(41).expect("a nonzero session");
    let peer = PeerId::new(2).expect("a nonzero peer");
    let net_actor = ActorId {
        session: live,
        serial: 1,
    };
    let (mut resolver, transforms) = resolver_fixture(
        live,
        Tick(7),
        SimActorId {
            session: live.get(),
            serial: 1,
        },
    );
    let mut receiver = SessionReceiver::new(live);
    receiver.admit_peer(peer);
    receiver.bind_actor(peer, net_actor).expect("free aircraft");

    let packet = synthetic_fire_message(live, Tick(7), 1);
    assert_eq!(
        spawn_from(&mut receiver, peer, &packet, &mut resolver, &transforms),
        1,
        "the first delivery spawns"
    );
    // The same packet again: the gate absorbs it and it spawns nothing.
    let replay = receiver.receive(peer, &packet);
    assert!(matches!(
        replay.violation(),
        Some(SessionViolation::ReplayedSequence { .. })
    ));
    assert!(replay.fires().is_empty());
    assert_eq!(
        spawn_from(&mut receiver, peer, &packet, &mut resolver, &transforms),
        0,
        "the replay spawns no second projectile"
    );
}

#[test]
fn accept_f58_a_a_peer_without_an_aircraft_fires_nothing() {
    let live = SessionId::new(51).expect("a nonzero session");
    let peer = PeerId::new(3).expect("a nonzero peer");
    let mut receiver = SessionReceiver::new(live);
    receiver.admit_peer(peer);
    assert_eq!(receiver.actor_of(peer), None);

    // The input is admitted, but with no bound aircraft there is nothing to
    // fire for and no request is produced.
    let inbound = receiver.receive(peer, &synthetic_fire_message(live, Tick(1), 1));
    assert!(inbound.accepted());
    assert!(inbound.fires().is_empty());
}

#[test]
fn accept_f58_a_reopening_on_a_fresh_epoch_makes_the_old_connection_stale() {
    let old = SessionId::new(61).expect("a nonzero session");
    let new = SessionId::new(62).expect("a nonzero session");
    let peer = PeerId::new(1).expect("a nonzero peer");
    let net_actor = ActorId {
        session: old,
        serial: 2,
    };
    let mut receiver = SessionReceiver::new(old);
    receiver.admit_peer(peer);
    receiver.bind_actor(peer, net_actor).expect("free aircraft");

    receiver.reopen(new);
    assert_eq!(receiver.session(), new);
    assert_eq!(
        receiver.actor_of(peer),
        None,
        "the new epoch's table is empty; match bindings are rebound separately"
    );

    let old_packet = synthetic_fire_message(old, Tick(1), 1);
    // The new epoch has no members: the old connection's peer is unknown.
    let unknown = receiver.receive(peer, &old_packet);
    assert!(matches!(
        unknown.violation(),
        Some(SessionViolation::UnauthenticatedPeer { .. })
    ));

    // With the pilot readmitted under the fresh epoch, the old epoch's packet
    // is stale.
    receiver.admit_peer(peer);
    let stale = receiver.receive(peer, &old_packet);
    assert!(
        matches!(
            stale.violation(),
            Some(SessionViolation::StaleSession { .. })
        ),
        "the pre-reconnect packet is stale"
    );

    // The same sequence is legal against the fresh epoch once the pilot is
    // rebound to its aircraft.
    receiver
        .bind_actor(
            peer,
            ActorId {
                session: new,
                serial: 2,
            },
        )
        .expect("free aircraft");
    let fresh = synthetic_fire_message(new, Tick(2), 1);
    let inbound = receiver.receive(peer, &fresh);
    assert_eq!(inbound.admission, Admission::Accepted { sequence: 1 });
    assert_eq!(inbound.fires().len(), 1);
}
