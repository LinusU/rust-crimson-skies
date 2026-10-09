//! Acceptance scenario F58-B: a refused ask authorizes no request, so no
//! projectile spawns — through the whole production chain the host receive
//! boundary offers.
//!
//! Spec: `specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md`,
//! stage `### F58-B`, minimum scenario "Client requests damage/score directly;
//! server rejects it"; contract `docs/contracts/UI-NETWORK.md` ("Local
//! prediction never commits damage or rewards", "Server owns ... hit/damage
//! ... score and result"). Task test prefix: `accept_f58_b_`.
//!
//! The path is the designed chain: [`SessionReceiver`] admits the decoded
//! packet through the `cs_net::validation` session identity gate, judges the
//! ask it carries with the intent validator, and only an accepted intent
//! produces the F27 request the real `cs_sim::weapons::FireResolver` resolves.
//! Every value is newly authored synthetic fixture data, never original game
//! data.

use std::collections::{BTreeMap, BTreeSet};

use cs_app::network::recovery::{SessionReceiver, fire_intent};
use cs_net::lobby::Phase;
use cs_net::validation::{
    ClientIntent, FireRequest, IntentRefusal, MAX_INPUT_TICKS_AHEAD, MatchStage, ThreatDisposition,
    synthetic_fire_message,
};
use cs_sim::damage::ActorId as SimActorId;
use cs_sim::weapons::{
    FireResolver, GunBank, MountTransform, SYNTHETIC_STARTING_ROUNDS, WeaponState,
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

/// How many rounds the fixture aircraft still holds.
fn rounds_left(resolver: &FireResolver, actor: &SimActorId) -> u64 {
    resolver
        .state(actor)
        .expect("registered")
        .ammunition(&synthetic_gun_definition().mount().clone())
}

/// Resolves every authorized request and returns how many projectiles
/// spawned.
fn spawn_from(
    resolver: &mut FireResolver,
    transforms: &BTreeMap<cs_sim::damage::DamageNodeKey, MountTransform>,
    requests: &[FireRequest],
) -> usize {
    let mut spawned = 0;
    for request in requests {
        if let Ok(resolution) = resolver.resolve(&fire_intent(request), transforms) {
            spawned += resolution.accepted.len();
        }
    }
    spawned
}

#[test]
fn accept_f58_b_a_refused_ask_authorizes_no_projectile() {
    let live = SessionId::new(71).expect("a nonzero session");
    let pilot = PeerId::new(1).expect("a nonzero peer");
    let net_actor = ActorId {
        session: live,
        serial: 4,
    };
    let sim_actor = SimActorId {
        session: live,
        serial: 4,
    };

    let mut receiver = SessionReceiver::new(live);
    receiver.admit_peer(pilot);
    receiver
        .bind_actor(pilot, net_actor)
        .expect("the aircraft is free");

    let bans = BTreeSet::new();
    let server_tick = Tick(100);
    let stage = MatchStage::new(Phase::InMatch, server_tick, &bans);

    // The minimum acceptance scenario: the client requests damage, then
    // score, directly — and the server rejects both before anything moves.
    let damage = ClientIntent::ClaimDamage {
        target: net_actor,
        amount: 50,
    };
    let refusal = match receiver.validate_intent(pilot, &damage, stage) {
        Err(refusal) => refusal,
        Ok(()) => panic!("the server accepted a client-authored damage request"),
    };
    assert_eq!(refusal.label(), "client_authored_truth");
    assert_eq!(refusal.disposition(), ThreatDisposition::Disconnect);
    let score = ClientIntent::ClaimScore { points: 5_000 };
    assert!(matches!(
        receiver.validate_intent(pilot, &score, stage),
        Err(IntentRefusal::ClientAuthoredTruth { .. })
    ));
    assert_eq!(
        receiver.actor_of(pilot),
        Some(net_actor),
        "the refusals left the ownership table alone"
    );

    let (mut resolver, transforms) = resolver_fixture(live, server_tick, sim_actor);
    let starting = rounds_left(&resolver, &sim_actor);

    // A packet the identity gate admits but whose ask sits outside the server
    // tick window: refused, and it authorizes nothing at all.
    let too_new = synthetic_fire_message(live, Tick(server_tick.0 + MAX_INPUT_TICKS_AHEAD + 1), 1);
    let validated = receiver.receive_validated(pilot, &too_new, stage);
    assert!(
        validated.admitted(),
        "the identity gate knows nothing of the server's clock"
    );
    assert!(matches!(
        validated.refusal(),
        Some(IntentRefusal::TickOutsideWindow { .. })
    ));
    assert!(validated.fires().is_empty());
    assert_eq!(spawn_from(&mut resolver, &transforms, validated.fires()), 0);
    assert_eq!(
        rounds_left(&resolver, &sim_actor),
        starting,
        "no round was spent by a refused ask"
    );

    // Positive control: an in-window packet from the same peer is accepted
    // and spawns exactly one, so the zeroes are refusals and not a fixture
    // that can never fire.
    let live_packet = synthetic_fire_message(live, server_tick, 2);
    let validated = receiver.receive_validated(pilot, &live_packet, stage);
    assert!(
        validated
            .verdict()
            .is_some_and(cs_net::validation::IntentVerdict::accepted),
        "an in-window input ask is accepted: {:?}",
        validated.verdict()
    );
    assert_eq!(validated.fires().len(), 1);
    assert_eq!(spawn_from(&mut resolver, &transforms, validated.fires()), 1);
    let after_one = rounds_left(&resolver, &sim_actor);
    assert!(after_one < starting, "the accepted ask fired exactly one");

    // Flood the rest of the window's budget with fresh, in-window sequences:
    // the packets are individually perfect, and the count is what cuts them.
    let limit = receiver.budget().limits().max_intents_per_window;
    for sequence in 3..=limit {
        let packet = synthetic_fire_message(live, server_tick, sequence);
        let validated = receiver.receive_validated(pilot, &packet, stage);
        assert!(
            validated
                .verdict()
                .is_some_and(cs_net::validation::IntentVerdict::accepted),
            "sequence {sequence} is still inside the design"
        );
        assert_eq!(validated.fires().len(), 1);
    }

    // One intent too many: refused with a bounded reason, disconnected by
    // disposition, and — the property the scenario is about — nothing to
    // resolve, so no projectile spawns.
    let flood = synthetic_fire_message(live, server_tick, limit + 1);
    let validated = receiver.receive_validated(pilot, &flood, stage);
    let refusal = validated
        .refusal()
        .expect("one intent past the design must be refused");
    assert_eq!(
        refusal,
        &IntentRefusal::RateExceeded {
            limit,
            window: server_tick.0 / receiver.budget().limits().window_ticks,
        }
    );
    assert_eq!(refusal.disposition(), ThreatDisposition::Disconnect);
    assert!(
        validated.fires().is_empty(),
        "a refused ask authorizes nothing"
    );
    assert_eq!(spawn_from(&mut resolver, &transforms, validated.fires()), 0);
    assert_eq!(
        rounds_left(&resolver, &sim_actor),
        after_one,
        "the flood spent no further rounds"
    );
    assert_eq!(
        receiver.budget().peer_count(),
        1,
        "the flood grew no state beyond the peer's counter"
    );
}
