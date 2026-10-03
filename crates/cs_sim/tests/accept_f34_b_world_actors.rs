//! F34-B acceptance: the rail/road/water/kinematic world-actor runtime
//! (synthetic).
//!
//! Spec: `specs/F34-ground-vehicles-boats-trains-and-mission-machinery.md`,
//! stage `### F34-B`. Ordinary build/test only; nothing here is original
//! data. The minimum scenario is AC02: destroy a gate before and after its
//! convoy arrives — both supported orders must behave correctly.
//!
//! The fixture: a 100 m route along +X driven at an authored cruise speed
//! by a road convoy, with a kinematic gate standing at the 50 m mark and a
//! 5 m stop distance, so a closed gate holds the convoy at the 45 m stop
//! line. The set steps 10 ticks/s, so one tick is 0.1 s.

use cs_script::ir::{ActorId, SymbolId};
use cs_sim::world_actors::Quat;
use cs_sim::world_actors::anchor::{
    AnchorSocket, PickupEnvelope, PickupRefusal, anchor_sample, pickup_eligible_pose,
};
use cs_sim::world_actors::graph::{GraphError, Presence};
use cs_sim::world_actors::release::PayloadSpec;
use cs_sim::world_actors::route::{RouteError, RouteGate, RoutePlan};
use cs_sim::world_actors::runtime::{
    ActorMotion, WorldActorError, WorldActorEvent, WorldActorKind, WorldActorSet, WorldActorSpec,
};
use cs_sim::world_actors::trajectory::synthetic_train_trajectory;
use cs_types::Tick;
use cs_types::content::ContentId;

const GATE: ActorId = ActorId(1);
const CONVOY: ActorId = ActorId(2);
const BRIDGE: ActorId = ActorId(3);
const BOAT: ActorId = ActorId(4);
const CARRIER: ActorId = ActorId(5);
const TRAIN: ActorId = ActorId(6);
const BYSTANDER: ActorId = ActorId(7);
const FRESH: ActorId = ActorId(40);

const CARRIER_SOCKET: AnchorSocket = AnchorSocket {
    actor: CARRIER,
    socket: 0,
    offset_m: [0.0, 3.0, 0.0],
};

const TRAIN_SOCKET: AnchorSocket = AnchorSocket {
    actor: TRAIN,
    socket: 0,
    offset_m: [0.0, 3.0, 0.0],
};

const ENVELOPE: PickupEnvelope = PickupEnvelope {
    max_distance_m: 5.0,
    max_relative_speed_m_s: 2.0,
};

fn faction() -> ContentId {
    ContentId::parse("faction/raiders").unwrap()
}

fn held_machine(actor: ActorId, position_m: [f64; 3]) -> WorldActorSpec {
    WorldActorSpec {
        actor,
        kind: WorldActorKind::Kinematic,
        faction: faction(),
        objective: None,
        motion: ActorMotion::Held {
            position_m,
            orientation: Quat::IDENTITY,
        },
    }
}

/// The 100 m +X route with the gate passage: gate at 50 m, stop line at
/// 45 m. `speed_m_s` is the authored cruise speed.
fn gated_plan(speed_m_s: f64) -> RoutePlan {
    RoutePlan::try_new(
        vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
        speed_m_s,
        vec![RouteGate {
            gate: GATE,
            at_m: 50.0,
            stop_before_m: 5.0,
        }],
    )
    .unwrap()
}

fn route_spec(actor: ActorId, kind: WorldActorKind, plan: RoutePlan) -> WorldActorSpec {
    WorldActorSpec {
        actor,
        kind,
        faction: faction(),
        objective: Some(SymbolId(9)),
        motion: ActorMotion::Route {
            plan,
            start_progress_m: 0.0,
        },
    }
}

/// Gate standing at [50,0,0] plus a road convoy cruising 10 m/s on the
/// gated route: stop line at progress 45, reached at tick 45.
fn gated_set() -> WorldActorSet {
    let mut set = WorldActorSet::new(10).unwrap();
    set.register(held_machine(GATE, [50.0, 0.0, 0.0])).unwrap();
    set.register(route_spec(CONVOY, WorldActorKind::Road, gated_plan(10.0)))
        .unwrap();
    set
}

fn held_at_gate(actor: ActorId, gate: ActorId, at: Tick) -> WorldActorEvent {
    WorldActorEvent::HeldAtGate { actor, gate, at }
}

fn resumed(actor: ActorId, gate: ActorId, at: Tick) -> WorldActorEvent {
    WorldActorEvent::ResumedFromGate { actor, gate, at }
}

fn completed(actor: ActorId, at: Tick) -> WorldActorEvent {
    WorldActorEvent::RouteCompleted { actor, at }
}

// ---------------------------------------------------------- AC02 order 1 ---

#[test]
fn accept_f34_b_gate_destroyed_before_convoy_arrival_never_holds_it() {
    let mut set = gated_set();
    // Destroy the gate while the convoy is still at the route start.
    assert_eq!(set.destroy(GATE).unwrap(), [GATE]);
    assert_eq!(set.presence(GATE), Some(Presence::Destroyed));

    let events = set.advance_to(Tick(50)).unwrap();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, WorldActorEvent::HeldAtGate { gate, .. } if *gate == GATE)),
        "a destroyed gate must never hold: {events:?}"
    );
    // At tick 50 the convoy stands exactly where the gate stood; an intact
    // gate would have pinned it at the 45 m stop line.
    assert_eq!(set.route_progress_m(CONVOY).unwrap(), Some(50.0));
    let pose = set.pose(CONVOY).unwrap();
    assert_eq!(pose.position_m, [50.0, 0.0, 0.0]);
    assert_eq!(pose.velocity_m_s, [10.0, 0.0, 0.0]);
    assert_eq!(set.presence(CONVOY), Some(Presence::Intact));

    let events = set.advance_to(Tick(101)).unwrap();
    assert!(events.contains(&completed(CONVOY, Tick(100))));
    let pose = set.pose(CONVOY).unwrap();
    assert_eq!(pose.position_m, [100.0, 0.0, 0.0]);
    assert_eq!(pose.velocity_m_s, [0.0, 0.0, 0.0]);
}

// ---------------------------------------------------------- AC02 order 2 ---

#[test]
fn accept_f34_b_convoy_held_at_closed_gate_resumes_when_gate_destroyed() {
    let mut set = gated_set();
    let events = set.advance_to(Tick(45)).unwrap();
    assert!(events.contains(&held_at_gate(CONVOY, GATE, Tick(45))));
    assert_eq!(set.held_gate(CONVOY).unwrap(), Some(GATE));

    // Held: no creep — the pose pins at the stop line with zero velocity
    // for as long as the gate stands.
    let events = set.advance_to(Tick(50)).unwrap();
    assert_eq!(events, Vec::new());
    let pose = set.pose(CONVOY).unwrap();
    assert_eq!(pose.position_m, [45.0, 0.0, 0.0]);
    assert_eq!(pose.velocity_m_s, [0.0, 0.0, 0.0]);

    // Destroy the gate after the convoy arrived: it resumes on the very
    // next tick and still completes the route.
    assert_eq!(set.destroy(GATE).unwrap(), [GATE]);
    let events = set.advance_to(Tick(51)).unwrap();
    assert_eq!(events, [resumed(CONVOY, GATE, Tick(51))]);
    assert_eq!(set.held_gate(CONVOY).unwrap(), None);
    let pose = set.pose(CONVOY).unwrap();
    assert_eq!(pose.position_m, [46.0, 0.0, 0.0]);
    assert_eq!(pose.velocity_m_s, [10.0, 0.0, 0.0]);

    let events = set.advance_to(Tick(105)).unwrap();
    assert!(events.contains(&completed(CONVOY, Tick(105))));
    assert_eq!(set.pose(CONVOY).unwrap().position_m, [100.0, 0.0, 0.0]);
}

// ------------------------------------------------- destruction cascade ---

#[test]
fn accept_f34_b_destroying_a_support_freezes_dependents_as_wrecks() {
    let mut set = gated_set();
    set.register(held_machine(BRIDGE, [30.0, 0.0, 0.0]))
        .unwrap();
    set.register(WorldActorSpec {
        actor: BYSTANDER,
        kind: WorldActorKind::Road,
        faction: faction(),
        objective: None,
        motion: ActorMotion::Held {
            position_m: [0.0, 0.0, 5.0],
            orientation: Quat::IDENTITY,
        },
    })
    .unwrap();
    // The dependency is an explicit edge, never a name check: the convoy
    // rests on the bridge.
    set.declare_support(BRIDGE, CONVOY).unwrap();

    set.advance_to(Tick(30)).unwrap();
    assert_eq!(set.pose(CONVOY).unwrap().position_m, [30.0, 0.0, 0.0]);

    let lost = set.destroy(BRIDGE).unwrap();
    assert_eq!(lost, [BRIDGE, CONVOY]);
    assert_eq!(set.presence(BRIDGE), Some(Presence::Destroyed));
    assert_eq!(set.presence(CONVOY), Some(Presence::Destroyed));
    assert_eq!(set.presence(GATE), Some(Presence::Intact));
    assert_eq!(set.presence(BYSTANDER), Some(Presence::Intact));

    // The wreck freezes at its destruction pose with zeroed velocity; no
    // later tick moves it.
    let wreck = set.pose(CONVOY).unwrap();
    assert_eq!(wreck.position_m, [30.0, 0.0, 0.0]);
    assert_eq!(wreck.velocity_m_s, [0.0, 0.0, 0.0]);
    assert!(set.advance_to(Tick(60)).unwrap().is_empty());
    assert_eq!(set.pose(CONVOY).unwrap(), wreck);
}

// --------------------------------------------------------- offscreen -----

#[test]
fn accept_f34_b_offscreen_actor_moves_identically_to_a_sampled_one() {
    // The runtime step takes no visibility input: an actor nobody ever
    // samples — a culled one — is exactly where a sampled one is.
    let ungated = |actor| {
        route_spec(
            actor,
            WorldActorKind::Road,
            RoutePlan::try_new(vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]], 10.0, vec![]).unwrap(),
        )
    };
    let mut sampled = WorldActorSet::new(10).unwrap();
    let mut culled = WorldActorSet::new(10).unwrap();
    sampled.register(ungated(CONVOY)).unwrap();
    culled.register(ungated(CONVOY)).unwrap();

    for _ in 0..60 {
        sampled.step().unwrap();
        let _ = sampled.pose(CONVOY); // the presentation read
    }
    culled.advance_to(Tick(60)).unwrap();

    // The culled convoy still moved — and to the same pose.
    let pose = culled.pose(CONVOY).unwrap();
    assert_eq!(pose.position_m, [60.0, 0.0, 0.0]);
    assert_eq!(pose.velocity_m_s, [10.0, 0.0, 0.0]);
    assert_eq!(pose, sampled.pose(CONVOY).unwrap());
}

// -------------------------------------------------- released payload -----

#[test]
fn accept_f34_b_released_boat_keeps_carrier_motion_faction_and_objective() {
    let mut set = WorldActorSet::new(10).unwrap();
    set.register(WorldActorSpec {
        actor: CARRIER,
        kind: WorldActorKind::Rail,
        faction: faction(),
        objective: None,
        motion: ActorMotion::Trajectory(synthetic_train_trajectory()),
    })
    .unwrap();
    set.advance_to(Tick(20)).unwrap();

    let pose = set.pose(CARRIER).unwrap();
    let anchor = anchor_sample(Tick(20), &pose, &CARRIER_SOCKET);
    let payload = set
        .release(
            &anchor,
            PayloadSpec {
                actor: BOAT,
                faction: faction(),
                objective: Some(SymbolId(5)),
                eject_m_s: [0.0, 0.0, 2.0],
            },
            WorldActorKind::Water,
        )
        .unwrap();

    assert_eq!(payload.actor, BOAT);
    assert_eq!(set.kind(BOAT).unwrap(), WorldActorKind::Water);
    assert_eq!(set.faction(BOAT).unwrap(), &faction());
    assert_eq!(set.objective(BOAT).unwrap(), Some(SymbolId(5)));

    // The boat starts on the anchor and inherits the carrier's motion
    // plus the authored ejection — and keeps drifting on it.
    let pose = set.pose(BOAT).unwrap();
    assert_eq!(pose.position_m, [20.0, 3.0, 0.0]);
    assert_eq!(pose.velocity_m_s, [10.0, 0.0, 2.0]);
    set.advance_to(Tick(30)).unwrap();
    let drifted = set.pose(BOAT).unwrap().position_m;
    for (got, want) in drifted.into_iter().zip([30.0, 3.0, 2.0]) {
        assert!((got - want).abs() < 1e-9, "{drifted:?}");
    }
}

// ------------------------------------------- pickup on the runtime pose ---

#[test]
fn accept_f34_b_pickup_is_judged_on_the_pose_the_renderer_reads() {
    let mut set = WorldActorSet::new(10).unwrap();
    set.register(WorldActorSpec {
        actor: TRAIN,
        kind: WorldActorKind::Rail,
        faction: faction(),
        objective: None,
        motion: ActorMotion::Trajectory(synthetic_train_trajectory()),
    })
    .unwrap();
    set.advance_to(Tick(50)).unwrap();

    // One canonical pose: the renderer's anchor and the pickup's anchor
    // are the same value.
    let pose = set.pose(TRAIN).unwrap();
    let render = anchor_sample(Tick(50), &pose, &TRAIN_SOCKET);
    let got = pickup_eligible_pose(
        Tick(50),
        &pose,
        &TRAIN_SOCKET,
        [50.0, 4.0, 0.0],
        [10.0, 0.0, 0.0],
        ENVELOPE,
    )
    .unwrap();
    assert_eq!(got, render);

    // A stationary taker right on the moving anchor is refused on
    // relative speed — never on ground speed.
    let refused = pickup_eligible_pose(
        Tick(50),
        &pose,
        &TRAIN_SOCKET,
        [50.0, 3.0, 0.0],
        [0.0; 3],
        ENVELOPE,
    );
    assert!(matches!(refused, Err(PickupRefusal::TooFast { .. })));
}

// ---------------------------------- velocity is the actual displacement ---

#[test]
fn accept_f34_b_partial_move_into_a_gate_reports_the_reduced_speed() {
    // Cruise 7 m/s = 0.7 m/tick: progress reaches the 45 m stop line in
    // the middle of tick 65, so that tick reports the true 2 m/s partial
    // displacement, then zero while held — never the stale cruise speed.
    let mut set = WorldActorSet::new(10).unwrap();
    set.register(held_machine(GATE, [50.0, 0.0, 0.0])).unwrap();
    set.register(route_spec(CONVOY, WorldActorKind::Road, gated_plan(7.0)))
        .unwrap();

    set.advance_to(Tick(64)).unwrap();
    let progress = set.route_progress_m(CONVOY).unwrap().unwrap();
    assert!((progress - 44.8).abs() < 1e-9, "{progress}");

    let events = set.step().unwrap();
    assert!(events.contains(&held_at_gate(CONVOY, GATE, Tick(65))));
    let pose = set.pose(CONVOY).unwrap();
    assert_eq!(pose.position_m, [45.0, 0.0, 0.0]);
    assert!((pose.velocity_m_s[0] - 2.0).abs() < 1e-9);

    let events = set.step().unwrap();
    assert_eq!(events, Vec::new());
    assert_eq!(set.pose(CONVOY).unwrap().velocity_m_s, [0.0, 0.0, 0.0]);
}

// ------------------------------------------------------------- refusals ---

#[test]
fn accept_f34_b_refuses_duplicate_unknown_self_gated_and_past_gate_actors() {
    assert!(matches!(
        WorldActorSet::new(0),
        Err(WorldActorError::ZeroTickRate)
    ));
    let mut set = gated_set();

    assert_eq!(
        set.register(held_machine(GATE, [0.0, 0.0, 0.0])),
        Err(WorldActorError::DuplicateActor(GATE))
    );

    // A route may not be gated by its own follower.
    let self_gated = RoutePlan::try_new(
        vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
        10.0,
        vec![RouteGate {
            gate: FRESH,
            at_m: 50.0,
            stop_before_m: 5.0,
        }],
    )
    .unwrap();
    assert_eq!(
        set.register(route_spec(FRESH, WorldActorKind::Road, self_gated)),
        Err(WorldActorError::SelfGate { actor: FRESH })
    );

    // Every declared gate must name a registered actor.
    let unknown_gate = RoutePlan::try_new(
        vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
        10.0,
        vec![RouteGate {
            gate: ActorId(99),
            at_m: 50.0,
            stop_before_m: 5.0,
        }],
    )
    .unwrap();
    assert_eq!(
        set.register(route_spec(FRESH, WorldActorKind::Road, unknown_gate)),
        Err(WorldActorError::UnknownGate {
            actor: FRESH,
            gate: ActorId(99),
        })
    );

    // Spawning past a closed gate's stop line is impossible: the follower
    // could never have got there while the gate stood.
    let mut past = route_spec(FRESH, WorldActorKind::Road, gated_plan(10.0));
    past.motion = ActorMotion::Route {
        plan: gated_plan(10.0),
        start_progress_m: 60.0,
    };
    assert_eq!(
        set.register(past),
        Err(WorldActorError::BeyondClosedGate {
            actor: FRESH,
            gate: GATE,
            progress_m: 60.0,
            stop_line_m: 45.0,
        })
    );

    // …but once the gate is destroyed the same spawn is legal: a
    // destroyed gate opens the passage in both temporal orders.
    set.destroy(GATE).unwrap();
    set.register(past_rebuild()).unwrap();
    assert_eq!(set.held_gate(FRESH).unwrap(), None);

    // Session ids are never reused, not even over a destroyed record.
    set.destroy(CONVOY).unwrap();
    assert_eq!(
        set.register(route_spec(CONVOY, WorldActorKind::Road, gated_plan(10.0))),
        Err(WorldActorError::DuplicateActor(CONVOY))
    );

    assert!(matches!(
        set.destroy(ActorId(99)),
        Err(WorldActorError::Graph(GraphError::UnknownActor(ActorId(
            99
        ))))
    ));

    // Ticks never replay backwards.
    set.advance_to(Tick(5)).unwrap();
    assert!(matches!(
        set.advance_to(Tick(4)),
        Err(WorldActorError::NonMonotonicTick { .. })
    ));
}

/// The same spec `past` built again after the borrow ends.
fn past_rebuild() -> WorldActorSpec {
    let mut spec = route_spec(FRESH, WorldActorKind::Road, gated_plan(10.0));
    spec.motion = ActorMotion::Route {
        plan: gated_plan(10.0),
        start_progress_m: 60.0,
    };
    spec
}

#[test]
fn accept_f34_b_route_plans_refuse_degenerate_geometry_and_gates() {
    let points = || vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]];
    assert!(matches!(
        RoutePlan::try_new(vec![[0.0, 0.0, 0.0]], 10.0, vec![]),
        Err(RouteError::TooFewPoints { .. })
    ));
    assert!(matches!(
        RoutePlan::try_new(points(), 0.0, vec![]),
        Err(RouteError::InvalidSpeed { .. })
    ));
    assert!(matches!(
        RoutePlan::try_new(vec![[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]], 10.0, vec![]),
        Err(RouteError::DegenerateSegment { .. })
    ));
    assert!(matches!(
        RoutePlan::try_new(vec![[0.0, f64::NAN, 0.0], [100.0, 0.0, 0.0]], 10.0, vec![]),
        Err(RouteError::NonFinitePoint { .. })
    ));
    assert!(matches!(
        RoutePlan::try_new(
            points(),
            10.0,
            vec![RouteGate {
                gate: GATE,
                at_m: 200.0,
                stop_before_m: 5.0,
            }],
        ),
        Err(RouteError::GateBeyondRoute { .. })
    ));
    assert!(matches!(
        RoutePlan::try_new(
            points(),
            10.0,
            vec![
                RouteGate {
                    gate: GATE,
                    at_m: 60.0,
                    stop_before_m: 5.0,
                },
                RouteGate {
                    gate: BRIDGE,
                    at_m: 50.0,
                    stop_before_m: 5.0,
                },
            ],
        ),
        Err(RouteError::GatesNotAscending { .. })
    ));
    assert!(matches!(
        RoutePlan::try_new(
            points(),
            10.0,
            vec![RouteGate {
                gate: GATE,
                at_m: 50.0,
                stop_before_m: 60.0,
            }],
        ),
        Err(RouteError::InvalidStop { .. })
    ));
}

// ------------------------------------------------------- determinism -----

#[test]
fn accept_f34_b_identical_sessions_step_identically() {
    let mut a = gated_set();
    let mut b = gated_set();
    assert_eq!(
        a.advance_to(Tick(60)).unwrap(),
        b.advance_to(Tick(60)).unwrap()
    );
    a.destroy(GATE).unwrap();
    b.destroy(GATE).unwrap();
    assert_eq!(
        a.advance_to(Tick(105)).unwrap(),
        b.advance_to(Tick(105)).unwrap()
    );
    assert_eq!(a, b);
}

// ------------------------------------------------------------ kind catalog ---

#[test]
fn accept_f34_b_catalog_kinds_and_a_water_follower_at_its_lock_gate() {
    assert_eq!(
        WorldActorKind::ALL.map(|k| k.label()),
        ["rail", "road", "water", "kinematic"]
    );
    // A boat waits at a lock gate by exactly the convoy rule: the gate
    // rule is on the route's declared passage, not on the actor kind.
    let mut set = WorldActorSet::new(10).unwrap();
    set.register(held_machine(GATE, [50.0, 0.0, 0.0])).unwrap();
    set.register(route_spec(BOAT, WorldActorKind::Water, gated_plan(10.0)))
        .unwrap();
    set.advance_to(Tick(60)).unwrap();
    assert_eq!(set.held_gate(BOAT).unwrap(), Some(GATE));
    assert_eq!(set.pose(BOAT).unwrap().position_m, [45.0, 0.0, 0.0]);
}

// ------------------------------------------------- shared stop line -----

#[test]
fn accept_f34_b_spawn_on_a_shared_stop_line_holds_the_nearest_gate() {
    // Two intact gates share the 45 m stop line. A follower registered
    // exactly on it is held by the first gate along the route — the same
    // one the step loop reports — so the first step emits no spurious
    // resume/hold pair.
    let mut set = WorldActorSet::new(10).unwrap();
    set.register(held_machine(GATE, [50.0, 0.0, 0.0])).unwrap();
    set.register(held_machine(BRIDGE, [60.0, 0.0, 0.0]))
        .unwrap();
    let plan = RoutePlan::try_new(
        vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
        10.0,
        vec![
            RouteGate {
                gate: GATE,
                at_m: 50.0,
                stop_before_m: 5.0,
            },
            RouteGate {
                gate: BRIDGE,
                at_m: 60.0,
                stop_before_m: 15.0,
            },
        ],
    )
    .unwrap();
    set.register(WorldActorSpec {
        actor: CONVOY,
        kind: WorldActorKind::Road,
        faction: faction(),
        objective: None,
        motion: ActorMotion::Route {
            plan,
            start_progress_m: 45.0,
        },
    })
    .unwrap();

    assert_eq!(set.held_gate(CONVOY).unwrap(), Some(GATE));
    assert_eq!(set.step().unwrap(), Vec::new());
    assert_eq!(set.held_gate(CONVOY).unwrap(), Some(GATE));
    assert_eq!(set.pose(CONVOY).unwrap().position_m, [45.0, 0.0, 0.0]);
}
