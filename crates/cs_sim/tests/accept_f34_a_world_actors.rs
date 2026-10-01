//! F34-A acceptance: world-actor motion and dependency contract (synthetic).
//!
//! Spec: `specs/F34-ground-vehicles-boats-trains-and-mission-machinery.md`,
//! stage `### F34-A`. Ordinary build/test only; nothing here is original data.

use cs_script::ir::{ActorId, SymbolId};
use cs_sim::world_actors::Quat;
use cs_sim::world_actors::anchor::{
    AnchorSocket, PickupEnvelope, PickupRefusal, anchor_sample, pickup_eligible,
};
use cs_sim::world_actors::graph::{GraphError, Presence, SupportGraph};
use cs_sim::world_actors::release::{PayloadSpec, release_payload};
use cs_sim::world_actors::trajectory::{
    Keyframe, Trajectory, TrajectoryError, synthetic_train_trajectory,
};
use cs_types::Tick;
use cs_types::content::ContentId;

const TRAIN: ActorId = ActorId(1);
const SOCKET: AnchorSocket = AnchorSocket {
    actor: TRAIN,
    socket: 0,
    offset_m: [0.0, 3.0, 0.0],
};
const ENVELOPE: PickupEnvelope = PickupEnvelope {
    max_distance_m: 5.0,
    max_relative_speed_m_s: 2.0,
};

#[test]
fn accept_f34_a_moving_train_pickup_uses_the_renderer_anchor_pose() {
    let train = synthetic_train_trajectory();
    // 10 m/s along +X; at tick 50 the anchor is at x = 50.
    let tick = Tick(50);
    let render = anchor_sample(tick, &train.sample(tick), &SOCKET);
    assert_eq!(render.position_m, [50.0, 3.0, 0.0]);
    assert_eq!(render.velocity_m_s, [10.0, 0.0, 0.0]);

    // A helicopter matching the train's velocity above that pose picks up.
    let got = pickup_eligible(
        tick,
        &train,
        &SOCKET,
        [50.0, 4.0, 0.0],
        [10.0, 0.0, 0.0],
        ENVELOPE,
    )
    .unwrap();
    assert_eq!(got, render);

    // The same taker hovering at the tick-0 anchor position is far from the
    // moving anchor: a stale pose would accept it.
    let stale = pickup_eligible(
        tick,
        &train,
        &SOCKET,
        [0.0, 4.0, 0.0],
        [10.0, 0.0, 0.0],
        ENVELOPE,
    );
    assert!(matches!(stale, Err(PickupRefusal::TooFar { .. })));
}

#[test]
fn accept_f34_a_pickup_is_judged_by_relative_not_ground_speed() {
    let train = synthetic_train_trajectory();
    let tick = Tick(50);
    // Stationary taker right at the anchor is 10 m/s relative: refused.
    let r = pickup_eligible(tick, &train, &SOCKET, [50.0, 3.0, 0.0], [0.0; 3], ENVELOPE);
    match r {
        Err(PickupRefusal::TooFast { relative_speed_m_s }) => {
            assert!((relative_speed_m_s - 10.0).abs() < 1e-9);
        }
        other => panic!("expected TooFast, got {other:?}"),
    }
    let nan = pickup_eligible(tick, &train, &SOCKET, [f64::NAN; 3], [0.0; 3], ENVELOPE);
    assert_eq!(nan, Err(PickupRefusal::NonFinite));
}

#[test]
fn accept_f34_a_velocity_is_the_derivative_of_position() {
    let train = synthetic_train_trajectory();
    for t in [1_u64, 37, 99] {
        let (a, b) = (train.sample(Tick(t)), train.sample(Tick(t + 1)));
        let dx = (b.position_m[0] - a.position_m[0]) * 10.0; // 10 ticks/s
        assert!((dx - a.velocity_m_s[0]).abs() < 1e-9);
    }
    // Past the end the actor is stopped, not extrapolated.
    let end = train.sample(Tick(500));
    assert_eq!(end.position_m, [100.0, 0.0, 0.0]);
    assert_eq!(end.velocity_m_s, [0.0; 3]);
}

#[test]
fn accept_f34_a_rotating_anchor_adds_the_rotational_velocity() {
    let half = std::f64::consts::FRAC_1_SQRT_2;
    // A turntable: 90 degrees about +Y over 10 s, origin fixed.
    let table = Trajectory::new(
        vec![
            Keyframe {
                tick: Tick(0),
                position_m: [0.0; 3],
                orientation: Quat::IDENTITY,
            },
            Keyframe {
                tick: Tick(100),
                position_m: [0.0; 3],
                orientation: Quat([0.0, half, 0.0, half]),
            },
        ],
        10,
    )
    .unwrap();
    let socket = AnchorSocket {
        actor: TRAIN,
        socket: 1,
        offset_m: [10.0, 0.0, 0.0],
    };
    let a = anchor_sample(Tick(0), &table.sample(Tick(0)), &socket);
    // omega = (pi/2)/10 about +Y; v = omega x r = (0, 0, -omega * 10).
    let omega = std::f64::consts::FRAC_PI_2 / 10.0;
    assert!(a.velocity_m_s[0].abs() < 1e-9);
    assert!((a.velocity_m_s[2] + omega * 10.0).abs() < 1e-9);
}

#[test]
fn accept_f34_a_offscreen_motion_depends_on_the_tick_only() {
    // `sample` takes no visibility or culling input; an actor nobody looks at
    // is at the same place as one on screen, and still moves.
    let train = synthetic_train_trajectory();
    assert_ne!(
        train.sample(Tick(10)).position_m,
        train.sample(Tick(90)).position_m
    );
    assert_eq!(train.sample(Tick(90)), train.sample(Tick(90)));
    assert!(train.speed_m_s(Tick(90)) > 0.0);
}

#[test]
fn accept_f34_a_bad_trajectories_are_refused() {
    let k = |t, x: f64, q| Keyframe {
        tick: Tick(t),
        position_m: [x, 0.0, 0.0],
        orientation: q,
    };
    assert_eq!(Trajectory::new(vec![], 10), Err(TrajectoryError::Empty));
    assert_eq!(
        Trajectory::new(vec![k(0, 0.0, Quat::IDENTITY)], 0),
        Err(TrajectoryError::ZeroTickRate)
    );
    assert_eq!(
        Trajectory::new(
            vec![k(5, 0.0, Quat::IDENTITY), k(5, 1.0, Quat::IDENTITY)],
            10
        ),
        Err(TrajectoryError::NotAscending { index: 1 })
    );
    assert_eq!(
        Trajectory::new(vec![k(0, f64::NAN, Quat::IDENTITY)], 10),
        Err(TrajectoryError::Invalid { index: 0 })
    );
    assert_eq!(
        Trajectory::new(vec![k(0, 0.0, Quat([0.0; 4]))], 10),
        Err(TrajectoryError::Invalid { index: 0 })
    );
}

#[test]
fn accept_f34_a_destroying_a_support_removes_dependents_by_edge_not_name() {
    // Ids are opaque: nothing about 1/2/3/4 says which is a bridge.
    let (bridge, gate, convoy, bystander) = (ActorId(1), ActorId(2), ActorId(3), ActorId(4));
    let mut g = SupportGraph::default();
    for a in [bridge, gate, convoy, bystander] {
        g.add_actor(a).unwrap();
    }
    g.add_support(bridge, gate).unwrap();
    g.add_support(gate, convoy).unwrap();

    let lost = g.destroy(bridge).unwrap();
    assert_eq!(lost, [bridge, gate, convoy]);
    for a in lost {
        assert_eq!(g.presence(a), Some(Presence::Destroyed));
    }
    assert_eq!(g.presence(bystander), Some(Presence::Intact));
    // Idempotent.
    assert!(g.destroy(gate).unwrap().is_empty());
}

#[test]
fn accept_f34_a_graph_refuses_cycles_unknowns_and_duplicates() {
    let (a, b) = (ActorId(1), ActorId(2));
    let mut g = SupportGraph::default();
    g.add_actor(a).unwrap();
    g.add_actor(b).unwrap();
    assert_eq!(g.add_actor(a), Err(GraphError::DuplicateActor(a)));
    g.add_support(a, b).unwrap();
    assert!(matches!(g.add_support(b, a), Err(GraphError::Cycle { .. })));
    assert!(matches!(g.add_support(a, a), Err(GraphError::Cycle { .. })));
    assert_eq!(
        g.add_support(a, ActorId(9)),
        Err(GraphError::UnknownActor(ActorId(9)))
    );
    assert_eq!(
        g.destroy(ActorId(9)),
        Err(GraphError::UnknownActor(ActorId(9)))
    );
}

#[test]
fn accept_f34_a_released_boat_inherits_carrier_velocity_faction_and_objective() {
    let carrier = synthetic_train_trajectory();
    let tick = Tick(20);
    let anchor = anchor_sample(tick, &carrier.sample(tick), &SOCKET);
    let faction = ContentId::parse("faction/allies").unwrap();
    let boat = release_payload(
        &anchor,
        PayloadSpec {
            actor: ActorId(77),
            faction: faction.clone(),
            objective: Some(SymbolId(5)),
            eject_m_s: [0.0, 0.0, 2.0],
        },
    );
    assert_ne!(boat.actor, TRAIN);
    assert_eq!(boat.position_m, anchor.position_m);
    assert_eq!(boat.velocity_m_s, [10.0, 0.0, 2.0]);
    assert_eq!(boat.faction, faction);
    assert_eq!(boat.objective, Some(SymbolId(5)));
}
