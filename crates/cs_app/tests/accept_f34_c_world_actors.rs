//! Acceptance scenario F34-C: the declared world-actor program lowers into
//! the wired session, whose `step` is the producer→runtime→consumer path:
//! scripted gate transitions drive the route followers, the cargo pickups
//! attach and collect on the canonical pose, commands refuse by name, and a
//! retry leaves no old generation behind.
//!
//! Spec: `specs/F34-ground-vehicles-boats-trains-and-mission-machinery.md`,
//! stage `### F34-C`; shared contract `docs/contracts/STATE-TRANSACTIONS.md`.
//! Task test prefix: `accept_f34_c_`. Minimum scenario: *a boat released
//! from a carrier retains appropriate velocity and faction*.
//!
//! These tests drive production code only:
//! [`cs_app::world_actors::lower_world_actors`] lowers the
//! [`cs_content::world_actors::declared_synthetic_world_actors`] fixture
//! into a launchable [`cs_app::world_actors::WorldActorSession`], whose
//! `step` hands commands, the declared gate schedule and the pickup
//! envelopes to `cs_sim::world_actors::runtime::WorldActorSet` and
//! dispatches the ordered event/refusal stream. Removing the lowering, the
//! carriage/pickup dispatch, the gate schedule or the retry teardown fails
//! them.
//!
//! Every value here is newly authored synthetic fixture data, never
//! original game data.
//!
//! Fixture timing (10 ticks/s): the convoy cruises 1 m/tick, holds at the
//! gate's 45 m stop line at tick 45, is released by the scripted open at
//! tick 60 and completes the 100 m route at tick 100; the truck cruises
//! 0.4 m/tick and is still short of the gate when the scripted close lands
//! at tick 80, holding at the 45 m line at tick 113. The carrier moves +Y
//! at 0.8 m/tick and crosses the crate's 2 m pickup envelope at tick 48.

use cs_app::world_actors::{
    LoweredWorldActors, SessionCompletion, SessionTaker, TakerProbe, WorldActorCommand,
    WorldActorLaunchError, WorldActorLowerError, WorldActorRefusal, WorldActorSession,
    WorldActorSessionEvent, WorldActorSessionTick, WorldActorTick, lower_world_actors,
};
use cs_content::objectives::{ProgramActor, ProgramSymbol};
use cs_content::world_actors::{
    DeclaredMotion, DeclaredPickup, DeclaredPickupCompletion, DeclaredPickupEnvelope,
    DeclaredRoute, DeclaredTaker, DeclaredWorldActorParts, DeclaredWorldActorProgram,
    declared_synthetic_world_actors,
};
use cs_script::ir::{ActorId, SymbolId};
use cs_script::runtime::SessionGeneration;
use cs_sim::world_actors::graph::GraphError;
use cs_sim::world_actors::release::PayloadSpec;
use cs_sim::world_actors::route::RouteError;
use cs_sim::world_actors::runtime::{WorldActorError, WorldActorEvent, WorldActorKind};
use cs_types::Tick;
use cs_types::content::{ContentId, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

const GEN1: SessionGeneration = SessionGeneration(1);
const GEN2: SessionGeneration = SessionGeneration(2);

const GATE: ActorId = ActorId(1);
const CONVOY: ActorId = ActorId(2);
const BRIDGE: ActorId = ActorId(3);
const BOAT: ActorId = ActorId(4);
const CARRIER: ActorId = ActorId(5);
const TRAIN: ActorId = ActorId(6);
const CRATE: ActorId = ActorId(8);
const TRUCK: ActorId = ActorId(9);

const PICKUP_BOAT: SymbolId = SymbolId(10);
const PICKUP_CRATE: SymbolId = SymbolId(11);
const PICKUP_TRAIN: SymbolId = SymbolId(12);

const FRESH: ActorId = ActorId(40);

const DECK_SOCKET: u16 = 1;
const STERN_SOCKET: u16 = 2;
const PICKUP_SOCKET: u16 = 1;

fn raiders() -> ContentId {
    ContentId::parse("faction/synthetic.raiders").unwrap()
}

fn lowered() -> LoweredWorldActors {
    lower_world_actors(&declared_synthetic_world_actors()).expect("the fixture lowers")
}

fn launch() -> WorldActorSession {
    WorldActorSession::launch(lowered(), GEN1).expect("the lowered program launches")
}

fn input(to: u64) -> WorldActorTick {
    WorldActorTick {
        to: Tick(to),
        commands: vec![],
        probes: vec![],
    }
}

fn step_to(session: &mut WorldActorSession, to: u64) -> WorldActorSessionTick {
    session.step(&input(to)).expect("a legal tick")
}

fn probe(pickup: SymbolId, position_m: [f64; 3], velocity_m_s: [f64; 3]) -> TakerProbe {
    TakerProbe {
        pickup,
        position_m,
        velocity_m_s,
    }
}

/// The `Pickup` events one step emitted.
fn pickups(stepped: &WorldActorSessionTick) -> Vec<(SymbolId, SessionCompletion, Tick)> {
    stepped
        .events
        .iter()
        .filter_map(|e| match e {
            WorldActorSessionEvent::Pickup {
                pickup,
                completion,
                at,
                ..
            } => Some((*pickup, *completion, *at)),
            _ => None,
        })
        .collect()
}

/// The `Gate` events one step emitted as `(gate, open, at)`.
fn gate_events(stepped: &WorldActorSessionTick) -> Vec<(ActorId, bool, Tick)> {
    stepped
        .events
        .iter()
        .filter_map(|e| match e {
            WorldActorSessionEvent::Gate { gate, open, at } => Some((*gate, *open, *at)),
            _ => None,
        })
        .collect()
}

/// The runtime `WorldActorEvent`s one step emitted.
fn runtime_events(stepped: &WorldActorSessionTick) -> Vec<WorldActorEvent> {
    stepped
        .events
        .iter()
        .filter_map(|e| match e {
            WorldActorSessionEvent::Runtime(event) => Some(*event),
            _ => None,
        })
        .collect()
}

/// Rebuilds the fixture's declared program with `parts` swapped in; every
/// argument replaces the matching fixture slice.
fn rebuild(parts: DeclaredWorldActorParts) -> DeclaredWorldActorProgram {
    let base = declared_synthetic_world_actors();
    DeclaredWorldActorProgram::try_new(
        base.subject().clone(),
        base.origin().clone(),
        base.provenance().clone(),
        parts,
    )
    .expect("the mutated program is still a valid record")
}

fn base_parts() -> DeclaredWorldActorParts {
    let base = declared_synthetic_world_actors();
    DeclaredWorldActorParts {
        ticks_per_second: base.ticks_per_second().clone(),
        actors: base.actors().to_vec(),
        support: base.support().to_vec(),
        pickups: base.pickups().to_vec(),
        transitions: base.transitions().to_vec(),
    }
}

fn designed<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(
        value,
        Provenance::designed(ClaimId::new("f34c.test.designed").expect("valid claim id")),
    ))
}

// ---------------------------------------------------------------------------
// The lowering boundary
// ---------------------------------------------------------------------------

#[test]
fn accept_f34_c_the_declared_program_lowers_and_launches() {
    let lowered = lowered();

    // Every declaration arrives: eight actors in authored order, the
    // support edge, three pickups, the sorted gate schedule.
    assert_eq!(lowered.ticks_per_second, 10);
    assert_eq!(lowered.actors.len(), 8);
    assert_eq!(
        lowered.actors.iter().map(|s| s.actor).collect::<Vec<_>>(),
        vec![GATE, BRIDGE, CARRIER, CONVOY, TRUCK, BOAT, CRATE, TRAIN]
    );
    assert_eq!(lowered.support, vec![(BRIDGE, CONVOY)]);
    assert_eq!(lowered.pickups.len(), 3);
    assert_eq!(
        lowered.transitions,
        vec![
            cs_app::world_actors::LoweredGateTransition {
                at: Tick(60),
                gate: GATE,
                open: true,
            },
            cs_app::world_actors::LoweredGateTransition {
                at: Tick(80),
                gate: GATE,
                open: false,
            },
        ]
    );

    // The boat's carriage resolved to the carrier's deck socket; the
    // crate pickup resolved its `Attach` socket onto the taker.
    let boat = lowered
        .actors
        .iter()
        .find(|s| s.actor == BOAT)
        .expect("declared");
    let cs_sim::world_actors::runtime::ActorMotion::Carried { carrier, socket } = &boat.motion
    else {
        panic!("the boat is carried");
    };
    assert_eq!(*carrier, CARRIER);
    assert_eq!(socket.actor, CARRIER);
    assert_eq!(socket.socket, DECK_SOCKET);
    assert_eq!(socket.offset_m, [0.0, 3.0, 0.0]);
    let crate_pickup = lowered
        .pickups
        .iter()
        .find(|p| p.symbol == PICKUP_CRATE)
        .expect("declared");
    assert_eq!(crate_pickup.target, CRATE);
    assert!(matches!(
        crate_pickup.completion,
        cs_app::world_actors::LoweredPickupCompletion::Attach { socket }
            if socket.actor == CARRIER && socket.socket == STERN_SOCKET
    ));

    // The catalog subject rides the binding surface.
    assert_eq!(
        lowered.subjects[&GATE].as_str(),
        "scene_node/synthetic.f34c.gate"
    );
    let binding = cs_app::world_actors::WorldActorBinding {
        actor: GATE,
        subject: lowered.subjects[&GATE].clone(),
        generation: cs_app::scene::SceneGeneration(1),
    };
    assert_eq!(binding.actor, GATE);
    assert_eq!(binding.generation, cs_app::scene::SceneGeneration(1));

    let session = WorldActorSession::launch(lowered, GEN1).expect("launch");
    assert_eq!(session.session(), GEN1);
    assert_eq!(session.tick(), Tick(0));
    assert_eq!(session.set().actors().count(), 8);
    assert_eq!(session.set().carried_by(BOAT).unwrap().unwrap().0, CARRIER);
    assert_eq!(session.set().faction(BOAT).unwrap(), &raiders());
    assert_eq!(session.set().objective(BOAT).unwrap(), Some(SymbolId(21)));
    assert!(!session.set().gate_open(GATE).unwrap());
    assert_eq!(binding.actor, GATE);
    assert_eq!(binding.generation, cs_app::scene::SceneGeneration(1));
}

#[test]
fn accept_f34_c_lowering_refuses_unknowns_dangling_references_and_cycles() {
    // An unmeasured tick rate is refused by name, never defaulted.
    let mut parts = base_parts();
    parts.ticks_per_second = Resolved::unknown(
        ClaimId::new("f34c.test.unmeasured-rate").expect("valid claim id"),
        "the original tick rate is unmeasured",
    )
    .expect("a reasoned unknown");
    assert!(matches!(
        lower_world_actors(&rebuild(parts)).unwrap_err(),
        WorldActorLowerError::UnknownValue {
            field: "ticks_per_second",
            ..
        }
    ));

    // A carried actor's carrier must be declared.
    let mut parts = base_parts();
    let boat = parts
        .actors
        .iter_mut()
        .find(|a| a.actor == ProgramActor(BOAT.0))
        .expect("declared");
    boat.motion = DeclaredMotion::Carried {
        carrier: ProgramActor(77),
        socket: DECK_SOCKET,
    };
    assert_eq!(
        lower_world_actors(&rebuild(parts)).unwrap_err(),
        WorldActorLowerError::UnknownCarrier {
            actor: ProgramActor(BOAT.0),
            carrier: ProgramActor(77),
        }
    );

    // A carried actor's socket must exist on the carrier.
    let mut parts = base_parts();
    let boat = parts
        .actors
        .iter_mut()
        .find(|a| a.actor == ProgramActor(BOAT.0))
        .expect("declared");
    boat.motion = DeclaredMotion::Carried {
        carrier: ProgramActor(CARRIER.0),
        socket: 99,
    };
    assert_eq!(
        lower_world_actors(&rebuild(parts)).unwrap_err(),
        WorldActorLowerError::UnknownSocket {
            actor: Some(ProgramActor(BOAT.0)),
            owner: ProgramActor(CARRIER.0),
            socket: 99,
        }
    );

    // Declared carriage cannot close a cycle: the carrier riding the boat
    // it already carries is refused at the boundary.
    let mut parts = base_parts();
    let carrier = parts
        .actors
        .iter_mut()
        .find(|a| a.actor == ProgramActor(CARRIER.0))
        .expect("declared");
    carrier.motion = DeclaredMotion::Carried {
        carrier: ProgramActor(BOAT.0),
        socket: PICKUP_SOCKET,
    };
    assert_eq!(
        lower_world_actors(&rebuild(parts)).unwrap_err(),
        WorldActorLowerError::CarriageCycle {
            actor: ProgramActor(CARRIER.0),
            carrier: ProgramActor(BOAT.0),
        }
    );

    // A pickup's target must be declared.
    let mut parts = base_parts();
    parts.pickups[0].target = ProgramActor(77);
    assert_eq!(
        lower_world_actors(&rebuild(parts)).unwrap_err(),
        WorldActorLowerError::UnknownPickupTarget {
            symbol: ProgramSymbol(PICKUP_BOAT.0),
            target: ProgramActor(77),
        }
    );

    // An `Attach` completion's socket must exist on the taker.
    let mut parts = base_parts();
    parts.pickups[1].completion = DeclaredPickupCompletion::Attach { socket: 99 };
    assert_eq!(
        lower_world_actors(&rebuild(parts)).unwrap_err(),
        WorldActorLowerError::UnknownSocket {
            actor: None,
            owner: ProgramActor(CARRIER.0),
            socket: 99,
        }
    );

    // A degenerate declared route propagates the runtime's refusal.
    let mut parts = base_parts();
    let convoy = parts
        .actors
        .iter_mut()
        .find(|a| a.actor == ProgramActor(CONVOY.0))
        .expect("declared");
    convoy.motion = DeclaredMotion::Route(DeclaredRoute {
        points: designed(vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]]),
        speed_m_s: designed(0.0),
        start_progress_m: designed(0.0),
        gates: vec![],
    });
    assert!(matches!(
        lower_world_actors(&rebuild(parts)).unwrap_err(),
        WorldActorLowerError::Route {
            actor,
            source: RouteError::InvalidSpeed { .. },
        } if actor == ProgramActor(CONVOY.0)
    ));
}

#[test]
fn accept_f34_c_launch_propagates_the_runtime_refusal_by_actor() {
    // A declared path ticking at a different rate than the set launches
    // nothing — the error names the actor whose spec was refused.
    let mut parts = base_parts();
    let train = parts
        .actors
        .iter_mut()
        .find(|a| a.actor == ProgramActor(TRAIN.0))
        .expect("declared");
    let DeclaredMotion::Path(path) = &mut train.motion else {
        panic!("the train is a path follower");
    };
    path.ticks_per_second = designed(20);
    let error =
        WorldActorSession::launch(lower_world_actors(&rebuild(parts)).unwrap(), GEN1).unwrap_err();
    assert!(matches!(
        error,
        WorldActorLaunchError::Actor {
            actor: TRAIN,
            source: WorldActorError::TickRateMismatch { .. },
        }
    ));
}

// ---------------------------------------------------------------------------
// Scripted gate transitions: schedule and commands
// ---------------------------------------------------------------------------

#[test]
fn accept_f34_c_the_declared_schedule_drives_gate_followers() {
    let mut session = launch();

    // The convoy reaches the 45 m stop line at tick 45 and holds; the
    // truck is still short of it.
    let stepped = step_to(&mut session, 45);
    assert!(
        runtime_events(&stepped).contains(&WorldActorEvent::HeldAtGate {
            actor: CONVOY,
            gate: GATE,
            at: Tick(45),
        })
    );
    assert_eq!(session.set().held_gate(CONVOY).unwrap(), Some(GATE));
    assert_eq!(session.set().held_gate(TRUCK).unwrap(), None);

    // The declared open fires on its tick: the gate event lands before the
    // same tick's runtime transitions, which report the resumed follower.
    let stepped = step_to(&mut session, 60);
    assert_eq!(gate_events(&stepped), vec![(GATE, true, Tick(60))]);
    assert!(
        runtime_events(&stepped).contains(&WorldActorEvent::ResumedFromGate {
            actor: CONVOY,
            gate: GATE,
            at: Tick(60),
        })
    );
    assert!(session.set().gate_open(GATE).unwrap());
    let progress = session.set().route_progress_m(CONVOY).unwrap().unwrap();
    assert!(
        progress > 45.0,
        "the convoy crossed the stop line: {progress}"
    );

    // The declared close fires on its tick: the convoy is already past the
    // passage and keeps moving; the truck has not crossed and will hold.
    let stepped = step_to(&mut session, 80);
    assert_eq!(gate_events(&stepped), vec![(GATE, false, Tick(80))]);
    assert!(!session.set().gate_open(GATE).unwrap());
    let progress = session.set().route_progress_m(CONVOY).unwrap().unwrap();
    assert!(progress > 50.0, "the convoy crossed the gate: {progress}");

    // The truck held at the re-closed gate on tick 113 — a scripted close
    // re-imposes the passage on a follower that never crossed it.
    let stepped = step_to(&mut session, 113);
    assert!(
        runtime_events(&stepped).contains(&WorldActorEvent::HeldAtGate {
            actor: TRUCK,
            gate: GATE,
            at: Tick(113),
        })
    );
    assert_eq!(session.set().held_gate(TRUCK).unwrap(), Some(GATE));
    assert_eq!(
        session.set().pose(TRUCK).unwrap().position_m,
        [45.0, 0.0, 0.0]
    );

    // The convoy resumed from the 45 m line at tick 60 and completes the
    // 100 m route at tick 114 — the re-closed gate behind it applies to
    // nothing it already crossed.
    let stepped = step_to(&mut session, 114);
    assert!(
        runtime_events(&stepped).contains(&WorldActorEvent::RouteCompleted {
            actor: CONVOY,
            at: Tick(114),
        })
    );
    assert_eq!(
        session.set().pose(CONVOY).unwrap().position_m,
        [100.0, 0.0, 0.0]
    );
}

#[test]
fn accept_f34_c_commanded_transitions_and_a_close_never_pulls_back() {
    let mut session = launch();

    // Command the gate open at tick 30 — before the convoy reaches the
    // stop line — then close it again at tick 70 when the convoy has
    // legitimately crossed. Commands apply at the session's current tick.
    let stepped = step_to(&mut session, 30);
    assert!(gate_events(&stepped).is_empty());

    let mut to_seventy = input(70);
    to_seventy.commands = vec![WorldActorCommand::Gate {
        gate: GATE,
        open: true,
    }];
    let stepped = session.step(&to_seventy).expect("a legal tick");
    assert_eq!(gate_events(&stepped), vec![(GATE, true, Tick(30))]);
    // The declared open at 60 was a no-op on an already-open gate: no
    // duplicate gate event.
    assert_eq!(
        gate_events(&stepped)
            .iter()
            .filter(|(_, open, _)| *open)
            .count(),
        1
    );
    assert!(
        !runtime_events(&stepped)
            .iter()
            .any(|e| matches!(e, WorldActorEvent::HeldAtGate { actor: CONVOY, .. })),
        "the convoy never held at an open gate"
    );
    let progress = session.set().route_progress_m(CONVOY).unwrap().unwrap();
    assert!(
        progress > 50.0,
        "the convoy crossed the passage: {progress}"
    );

    // Closing behind a crossed follower neither pulls it back nor pins it
    // where it crossed: it completes the route on schedule.
    let mut to_end = input(100);
    to_end.commands = vec![WorldActorCommand::Gate {
        gate: GATE,
        open: false,
    }];
    let stepped = session.step(&to_end).expect("a legal tick");
    assert_eq!(gate_events(&stepped), vec![(GATE, false, Tick(70))]);
    assert!(
        runtime_events(&stepped).contains(&WorldActorEvent::RouteCompleted {
            actor: CONVOY,
            at: Tick(100),
        })
    );
    assert_eq!(
        session.set().pose(CONVOY).unwrap().position_m,
        [100.0, 0.0, 0.0]
    );
}

#[test]
fn accept_f34_c_a_destroyed_gate_stays_open_forever() {
    let mut session = launch();

    let mut to_thirty = input(30);
    to_thirty.commands = vec![WorldActorCommand::Destroy { actor: GATE }];
    let stepped = session.step(&to_thirty).expect("a legal tick");
    assert_eq!(
        stepped.events,
        vec![WorldActorSessionEvent::Destroyed {
            actor: GATE,
            cascade: vec![GATE],
            at: Tick(0),
        }]
    );

    // A scripted close on a wreck is dead state: presence is monotonic and
    // the passage stays open — the convoy crosses the line without ever
    // holding and completes on schedule.
    let mut to_end = input(100);
    to_end.commands = vec![WorldActorCommand::Gate {
        gate: GATE,
        open: false,
    }];
    let stepped = session.step(&to_end).expect("a legal tick");
    assert!(
        !runtime_events(&stepped)
            .iter()
            .any(|e| matches!(e, WorldActorEvent::HeldAtGate { actor: CONVOY, .. })),
        "a destroyed gate can never hold"
    );
    assert!(
        runtime_events(&stepped).contains(&WorldActorEvent::RouteCompleted {
            actor: CONVOY,
            at: Tick(100),
        })
    );
}

// ---------------------------------------------------------------------------
// Cargo: carried motion, detach, collect, attach, release
// ---------------------------------------------------------------------------

#[test]
fn accept_f34_c_detaching_the_boat_keeps_carrier_velocity_and_faction() {
    let mut session = launch();
    step_to(&mut session, 10);

    // At tick 10 the carrier stands at [200,8,0]; the boat rides the deck
    // socket three meters up. Detaching releases it at that anchor on the
    // socket's velocity plus the authored ejection.
    let mut command = input(10);
    command.commands = vec![WorldActorCommand::Detach {
        actor: BOAT,
        eject_m_s: [0.0, 0.0, 2.0],
    }];
    let stepped = session.step(&command).expect("a legal tick");
    let WorldActorSessionEvent::Detached { payload } = &stepped.events[0] else {
        panic!("a detach emits the payload: {:?}", stepped.events);
    };
    assert_eq!(payload.actor, BOAT);
    assert_eq!(payload.position_m, [200.0, 11.0, 0.0]);
    assert_eq!(payload.velocity_m_s, [0.0, 8.0, 2.0]);
    assert!(stepped.refusals.is_empty());

    // The released boat keeps its declared faction and objective identity,
    // registers no carrier and drifts on its release velocity.
    assert_eq!(session.set().faction(BOAT).unwrap(), &raiders());
    assert_eq!(session.set().objective(BOAT).unwrap(), Some(SymbolId(21)));
    assert_eq!(session.set().carried_by(BOAT).unwrap(), None);
    assert_eq!(session.set().kind(BOAT).unwrap(), WorldActorKind::Water);

    let stepped = step_to(&mut session, 20);
    assert!(stepped.refusals.is_empty());
    let pose = session.set().pose(BOAT).unwrap();
    for (got, want) in pose.position_m.into_iter().zip([200.0, 19.0, 2.0]) {
        assert!((got - want).abs() < 1e-9, "{pose:?}");
    }
    assert_eq!(pose.velocity_m_s, [0.0, 8.0, 2.0]);
}

#[test]
fn accept_f34_c_the_winch_pickup_attaches_the_crate_mid_advance() {
    let mut session = launch();

    // One call stepping forty-five ticks: the carrier crosses the crate's
    // two-meter envelope at tick 48 (y = 0.8t) — a world-actor taker is
    // judged per tick, so the latch happens on the tick the envelope was
    // satisfied, not whenever the step happened to land.
    let stepped = step_to(&mut session, 60);
    assert_eq!(
        pickups(&stepped),
        vec![(PICKUP_CRATE, SessionCompletion::Attached, Tick(48))]
    );
    assert!(stepped.refusals.is_empty());

    let (carrier, socket) = session.set().carried_by(CRATE).unwrap().unwrap();
    assert_eq!(carrier, CARRIER);
    assert_eq!(socket.socket, STERN_SOCKET);
    // The crate rides the stern socket: carrier at [200,48,0] plus the
    // [-5,3,0] offset.
    for (got, want) in session
        .set()
        .pose(CRATE)
        .unwrap()
        .position_m
        .into_iter()
        .zip([195.0, 51.0, 0.0])
    {
        assert!((got - want).abs() < 1e-9, "{got} != {want}");
    }

    // A latched pickup fires once: later ticks emit nothing for it.
    let stepped = step_to(&mut session, 90);
    assert!(pickups(&stepped).is_empty());
}

#[test]
fn accept_f34_c_an_external_taker_collects_the_boat_off_the_deck() {
    let mut session = launch();
    step_to(&mut session, 10);

    // The boat rides the deck socket: its pickup anchor is [200,11,0]
    // drifting +Y at 8 m/s. An external taker probed right on it collects
    // it; the judged anchor is the same value the renderer reads.
    let socket = lowered().socket(BOAT, PICKUP_SOCKET).unwrap();
    let anchor = session.anchor_pose(BOAT, &socket).unwrap();
    assert_eq!(anchor.position_m, [200.0, 11.0, 0.0]);

    let mut with_probe = input(10);
    with_probe.probes = vec![probe(PICKUP_BOAT, [200.0, 11.0, 0.0], [0.0, 8.0, 0.0])];
    let stepped = session.step(&with_probe).expect("a legal tick");
    let [
        WorldActorSessionEvent::Pickup {
            pickup,
            target,
            taker,
            completion,
            at,
            anchor: judged,
        },
    ] = stepped.events.as_slice()
    else {
        panic!("exactly one pickup event: {:?}", stepped.events);
    };
    assert_eq!(*pickup, PICKUP_BOAT);
    assert_eq!(*target, BOAT);
    assert_eq!(*taker, SessionTaker::External);
    assert_eq!(*completion, SessionCompletion::Collected);
    assert_eq!(*at, Tick(10));
    assert_eq!(*judged, anchor);

    // Collected: the actor keeps its id, freezes where it left the world
    // and can never be carried or re-collected — the latch fired once.
    assert!(session.set().is_collected(BOAT).unwrap());
    assert_eq!(
        session.set().pose(BOAT).unwrap().position_m,
        [200.0, 11.0, 0.0]
    );
    let stepped = step_to(&mut session, 40);
    assert!(pickups(&stepped).is_empty());
    assert_eq!(
        session.set().pose(BOAT).unwrap().position_m,
        [200.0, 11.0, 0.0],
        "a collected actor leaves the world's motion behind"
    );
}

#[test]
fn accept_f34_c_a_latch_reports_the_anchor_the_renderer_reads() {
    let mut session = launch();
    step_to(&mut session, 30);

    // The train rides its own timetable along z=-100: at tick 30 its roof
    // socket's anchor is [30,3,-100] moving +X at 10 m/s. The plane
    // latching to it is probed with the same pose — relative speed, not
    // ground speed, is what the envelope judges.
    let socket = lowered().socket(TRAIN, PICKUP_SOCKET).unwrap();
    let anchor = session.anchor_pose(TRAIN, &socket).unwrap();
    assert_eq!(anchor.position_m, [30.0, 3.0, -100.0]);
    assert_eq!(anchor.velocity_m_s, [10.0, 0.0, 0.0]);

    let mut with_probe = input(30);
    with_probe.probes = vec![probe(PICKUP_TRAIN, [30.0, 3.0, -100.0], [10.0, 0.0, 0.0])];
    let stepped = session.step(&with_probe).expect("a legal tick");
    let [
        WorldActorSessionEvent::Pickup {
            pickup,
            taker,
            completion,
            anchor: judged,
            ..
        },
    ] = stepped.events.as_slice()
    else {
        panic!("exactly one pickup event: {:?}", stepped.events);
    };
    assert_eq!(*pickup, PICKUP_TRAIN);
    assert_eq!(*taker, SessionTaker::External);
    assert_eq!(*completion, SessionCompletion::Bound);
    // AC01: the judged anchor is the renderer's read of the same socket.
    assert_eq!(*judged, anchor);

    // The target keeps moving under its own motion — a bound taker's own
    // body is its consumer's transaction, not the session's.
    let stepped = step_to(&mut session, 40);
    assert!(pickups(&stepped).is_empty());
    assert_eq!(
        session.set().pose(TRAIN).unwrap().position_m,
        [40.0, 0.0, -100.0]
    );
}

#[test]
fn accept_f34_c_a_release_command_spawns_the_payload_at_the_socket() {
    let mut session = launch();
    step_to(&mut session, 10);

    let mut command = input(10);
    command.commands = vec![WorldActorCommand::Release {
        carrier: CARRIER,
        socket: STERN_SOCKET,
        spec: PayloadSpec {
            actor: FRESH,
            faction: raiders(),
            objective: Some(SymbolId(33)),
            eject_m_s: [0.0, 0.0, 2.0],
        },
        kind: WorldActorKind::Water,
    }];
    let stepped = session.step(&command).expect("a legal tick");
    let WorldActorSessionEvent::Released { payload } = &stepped.events[0] else {
        panic!("a release emits the payload: {:?}", stepped.events);
    };
    // The stern socket: carrier at [200,8,0] plus the [-5,3,0] offset.
    assert_eq!(payload.actor, FRESH);
    assert_eq!(payload.position_m, [195.0, 11.0, 0.0]);
    assert_eq!(payload.velocity_m_s, [0.0, 8.0, 2.0]);

    // The new actor is live with its declared identity — and ids are never
    // reused, so a second release of the same id is a named refusal.
    assert_eq!(session.set().faction(FRESH).unwrap(), &raiders());
    assert_eq!(session.set().objective(FRESH).unwrap(), Some(SymbolId(33)));
    assert_eq!(session.set().kind(FRESH).unwrap(), WorldActorKind::Water);

    let mut again = input(10);
    again.commands = vec![WorldActorCommand::Release {
        carrier: CARRIER,
        socket: STERN_SOCKET,
        spec: PayloadSpec {
            actor: FRESH,
            faction: raiders(),
            objective: None,
            eject_m_s: [0.0; 3],
        },
        kind: WorldActorKind::Water,
    }];
    let stepped = session.step(&again).expect("a legal tick");
    assert_eq!(stepped.refusals.len(), 1);
    assert!(matches!(
        &stepped.refusals[0],
        WorldActorRefusal::Command {
            error: WorldActorError::DuplicateActor(FRESH),
            ..
        }
    ));
}

#[test]
fn accept_f34_c_destroying_the_bridge_cascades_the_convoy_by_name() {
    let mut session = launch();
    step_to(&mut session, 30);

    // The declared support edge lowered and launched: destroying the
    // bridge loses the convoy with it, in graph order, and the wreck
    // freezes where it stood.
    let mut command = input(30);
    command.commands = vec![WorldActorCommand::Destroy { actor: BRIDGE }];
    let stepped = session.step(&command).expect("a legal tick");
    assert_eq!(
        stepped.events,
        vec![WorldActorSessionEvent::Destroyed {
            actor: BRIDGE,
            cascade: vec![BRIDGE, CONVOY],
            at: Tick(30),
        }]
    );
    assert_eq!(
        session.set().pose(CONVOY).unwrap().position_m,
        [30.0, 0.0, 0.0]
    );
    assert_eq!(session.set().pose(CONVOY).unwrap().velocity_m_s, [0.0; 3]);
}

// ---------------------------------------------------------------------------
// Refusals and error propagation
// ---------------------------------------------------------------------------

#[test]
fn accept_f34_c_refusals_are_named_and_errors_propagate() {
    let mut session = launch();

    let mut commands = input(10);
    commands.commands = vec![
        WorldActorCommand::Destroy { actor: ActorId(99) },
        WorldActorCommand::Gate {
            gate: ActorId(99),
            open: true,
        },
        WorldActorCommand::Detach {
            actor: CONVOY,
            eject_m_s: [0.0; 3],
        },
        WorldActorCommand::Detach {
            actor: ActorId(99),
            eject_m_s: [0.0; 3],
        },
        WorldActorCommand::Release {
            carrier: GATE,
            socket: 9,
            spec: PayloadSpec {
                actor: FRESH,
                faction: raiders(),
                objective: None,
                eject_m_s: [0.0; 3],
            },
            kind: WorldActorKind::Water,
        },
    ];
    let stepped = session.step(&commands).expect("a legal tick");
    assert!(stepped.events.is_empty());
    assert_eq!(stepped.refusals.len(), 5);
    assert!(matches!(
        &stepped.refusals[0],
        WorldActorRefusal::Command {
            error: WorldActorError::Graph(GraphError::UnknownActor(ActorId(99))),
            ..
        }
    ));
    assert!(matches!(
        &stepped.refusals[1],
        WorldActorRefusal::Command {
            error: WorldActorError::UnknownActor(ActorId(99)),
            ..
        }
    ));
    assert!(matches!(
        &stepped.refusals[2],
        WorldActorRefusal::Command {
            error: WorldActorError::NotCarried { actor: CONVOY },
            ..
        }
    ));
    assert!(matches!(
        &stepped.refusals[3],
        WorldActorRefusal::Command {
            error: WorldActorError::UnknownActor(ActorId(99)),
            ..
        }
    ));
    // A release needs a declared socket on the carrier: the gate declares
    // none, so the refusal names the missing socket, not a guess.
    assert_eq!(
        stepped.refusals[4],
        WorldActorRefusal::UnknownSocket {
            carrier: GATE,
            socket: 9,
        }
    );

    // A refused tick is an error, not a silent drop: the clock never runs
    // backwards and nothing else moved.
    let stale = session.step(&input(5)).unwrap_err();
    assert!(matches!(stale, WorldActorError::NonMonotonicTick { .. }));
    assert_eq!(session.tick(), Tick(10));
}

#[test]
fn accept_f34_c_a_refused_completion_is_reported_every_tick() {
    // A bespoke program: the boat's winch tries to attach the carrier
    // itself to the boat's pickup socket — but the boat already rides the
    // carrier, so the carriage would close a cycle. The envelope is
    // satisfied every tick; the completion is refused every tick.
    let mut parts = base_parts();
    parts.pickups = vec![DeclaredPickup {
        symbol: ProgramSymbol(90),
        target: ProgramActor(CARRIER.0),
        socket: DECK_SOCKET,
        envelope: DeclaredPickupEnvelope {
            max_distance_m: designed(5.0),
            max_relative_speed_m_s: designed(12.0),
        },
        taker: DeclaredTaker::WorldActor(ProgramActor(BOAT.0)),
        completion: DeclaredPickupCompletion::Attach {
            socket: PICKUP_SOCKET,
        },
    }];
    let mut session =
        WorldActorSession::launch(lower_world_actors(&rebuild(parts)).unwrap(), GEN1).unwrap();

    let stepped = step_to(&mut session, 5);
    assert!(pickups(&stepped).is_empty());
    assert_eq!(stepped.refusals.len(), 5);
    assert!(
        stepped.refusals.iter().all(|r| matches!(
            r,
            WorldActorRefusal::Completion {
                pickup,
                error: WorldActorError::CarriageCycle { .. },
            } if *pickup == SymbolId(90)
        )),
        "every stepped tick refuses the cycle: {:?}",
        stepped.refusals
    );
    assert_eq!(session.set().carried_by(CARRIER).unwrap(), None);
}

// ---------------------------------------------------------------------------
// Teardown and retry
// ---------------------------------------------------------------------------

#[test]
fn accept_f34_c_retry_restores_the_authored_state_and_names_the_teardown() {
    let mut session = launch();
    step_to(&mut session, 10);

    // Mutate generation 1: collect the boat off the deck, let the carrier
    // attach the crate mid-advance and open the gate on schedule. The
    // probe is judged at `to` — at tick 60 the deck anchor is [200,51,0].
    let mut with_probe = input(60);
    with_probe.probes = vec![probe(PICKUP_BOAT, [200.0, 51.0, 0.0], [0.0, 8.0, 0.0])];
    let stepped = session.step(&with_probe).expect("a legal tick");
    assert_eq!(
        pickups(&stepped),
        vec![
            (PICKUP_CRATE, SessionCompletion::Attached, Tick(48)),
            (PICKUP_BOAT, SessionCompletion::Collected, Tick(60)),
        ]
    );
    assert!(session.set().is_collected(BOAT).unwrap());
    assert!(session.set().gate_open(GATE).unwrap());
    let convoy_progress = session.set().route_progress_m(CONVOY).unwrap().unwrap();
    assert!(
        convoy_progress > 45.0,
        "the convoy resumed: {convoy_progress}"
    );

    // Retry: the teardown names what generation 1 still owned — every
    // registered actor to despawn, the collected boat, the latched
    // pickups and the gate close still on the schedule.
    let teardown = session.retry(GEN2).expect("retry");
    assert_eq!(teardown.session, GEN1);
    // The registry's id order, not the authored order.
    assert_eq!(
        teardown.actors,
        vec![GATE, CONVOY, BRIDGE, BOAT, CARRIER, TRAIN, CRATE, TRUCK]
    );
    assert_eq!(teardown.collected, vec![BOAT]);
    assert_eq!(teardown.latched_pickups, vec![PICKUP_BOAT, PICKUP_CRATE]);
    assert_eq!(
        teardown.pending_transitions,
        vec![cs_app::world_actors::LoweredGateTransition {
            at: Tick(80),
            gate: GATE,
            open: false,
        }]
    );

    // The new generation owns nothing of the old one: authored initial
    // state restored — gate closed, convoy at the start, the boat back on
    // the deck socket, the crate still held at its anchor, no pickup
    // latched.
    assert_eq!(session.session(), GEN2);
    assert_eq!(session.tick(), Tick(0));
    assert!(!session.set().gate_open(GATE).unwrap());
    assert_eq!(session.set().route_progress_m(CONVOY).unwrap(), Some(0.0));
    assert_eq!(session.set().carried_by(BOAT).unwrap().unwrap().0, CARRIER);
    assert_eq!(session.set().carried_by(CRATE).unwrap(), None);
    assert!(!session.set().is_collected(BOAT).unwrap());
    assert_eq!(
        session.set().pose(CRATE).unwrap().position_m,
        [200.0, 40.0, 0.0]
    );

    // The schedule replays from the top and the pickups latch again.
    let mut replay = input(60);
    replay.probes = vec![probe(PICKUP_BOAT, [200.0, 51.0, 0.0], [0.0, 8.0, 0.0])];
    let stepped = session.step(&replay).expect("a legal tick");
    assert_eq!(gate_events(&stepped), vec![(GATE, true, Tick(60))]);
    assert_eq!(
        pickups(&stepped),
        vec![
            (PICKUP_CRATE, SessionCompletion::Attached, Tick(48)),
            (PICKUP_BOAT, SessionCompletion::Collected, Tick(60)),
        ]
    );
    assert_eq!(
        stepped.session, GEN2,
        "the live generation stamps the answer"
    );
}

#[test]
fn accept_f34_c_a_retry_cannot_rebuild_the_live_generation() {
    let mut session = launch();
    step_to(&mut session, 10);

    // A retry into the live generation would let old artifacts share the
    // new session's stamp — refused by name, and the session is untouched.
    assert_eq!(
        session.retry(GEN1).unwrap_err(),
        WorldActorLaunchError::SameGeneration { session: GEN1 }
    );
    assert_eq!(session.session(), GEN1);
    assert_eq!(session.tick(), Tick(10));
}

#[test]
fn accept_f34_c_identical_sessions_step_identically() {
    let mut a = launch();
    let mut b = launch();

    let mut shared = input(60);
    shared.commands = vec![
        WorldActorCommand::Gate {
            gate: GATE,
            open: true,
        },
        WorldActorCommand::Detach {
            actor: BOAT,
            eject_m_s: [0.0, 0.0, 2.0],
        },
    ];
    shared.probes = vec![probe(PICKUP_TRAIN, [30.0, 3.0, -100.0], [10.0, 0.0, 0.0])];
    assert_eq!(
        a.step(&shared).expect("a legal tick"),
        b.step(&shared).expect("a legal tick"),
        "the same producer surface yields the same session answer"
    );
    assert_eq!(a.set(), b.set());
}
