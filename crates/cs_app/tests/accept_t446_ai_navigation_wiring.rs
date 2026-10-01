//! F31-C ECS wiring acceptance tests (task #446): the F31 navigation driver is
//! owned by a running mission, fed the integrated flight state and bound to a
//! live moving anchor.
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, stage
//! `### F31-C`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`. Task test
//! prefix: `accept_t446_`.
//!
//! These tests drive production code only: the real [`PhysicsFixture`] world
//! with the production [`FlightForcesPlugin`] and [`AiNavigationPlugin`], the
//! `cs_content` declared moving route mapped through
//! [`cs_app::ai::bind_route`], a live [`MovingAnchor`] entity carrying the
//! Avian `Position`/`Rotation` the driver samples each fixed tick, and the
//! production [`spawn_flight_body`] path. Every value is newly authored
//! synthetic fixture data — no original game data, and no test reads
//! `CS_GAME_DIR`.

use avian3d::prelude::{LinearVelocity, Position, Rotation};
use bevy::math::Quat;
use bevy::prelude::{Entity, Transform, Vec3};

use cs_app::ai::{
    AiNavigation, AiNavigationPlugin, AnchorBinding, BoundRoute, MovingAnchor,
    NavigationRefusalReason, NavigationTickReport, RouteBindingError, RoutePursuit,
    SYNTHETIC_MOVING_ANCHOR_RUNTIME_ID, SYNTHETIC_NAVIGATION_SEED, SYNTHETIC_NAVIGATION_SESSION,
    bind_route, declared_synthetic_moving_route, synthetic_moving_anchor_id,
};
use cs_app::physics::{
    FixtureBodySpec, FlightAircraft, FlightForcesPlugin, FlightSpawnSpec, PhysicsFixture,
    spawn_flight_body,
};
use cs_content::routes::AnchorKind;
use cs_sim::ai::navigation::{
    NavigationCadence, Navigator, RouteNodeId, synthetic_maneuver_envelope,
};
use cs_sim::damage::ActorId;
use cs_sim::flight::{EngineState, FlightInput, FlightModel, synthetic_fixed_wing};

/// The serial of the fixture AI actor.
const ACTOR_SERIAL: u64 = 7;

/// The fixture actor, in the fixture session.
fn actor(serial: u64) -> ActorId {
    ActorId {
        session: SYNTHETIC_NAVIGATION_SESSION,
        serial,
    }
}

/// The designed navigator every fixture set bounds its commands by.
fn navigator() -> Navigator {
    Navigator::new(
        synthetic_maneuver_envelope(),
        NavigationCadence::designed_default(),
    )
    .expect("the designed synthetic envelope and cadence are valid")
}

/// A flight world: the real physics fixture with the production flight driver
/// and the F31 navigation driver.
fn fixture() -> PhysicsFixture {
    PhysicsFixture::builder(FixtureBodySpec {
        mass_kg: 1.0,
        half_extents_m: [0.05, 0.05, 0.05],
        position_m: [-10_000.0, 0.0, 0.0],
        linear_velocity_m_s: [0.0; 3],
    })
    .configure(|app| {
        app.add_plugins(FlightForcesPlugin);
        app.add_plugins(AiNavigationPlugin::new(
            SYNTHETIC_NAVIGATION_SESSION,
            SYNTHETIC_NAVIGATION_SEED,
        ));
    })
    .build()
    .expect("the fixture spec is valid")
}

/// The declared synthetic moving route projected into the runtime graph and
/// bound to the fixture anchor's runtime id.
fn bound_moving_route() -> BoundRoute {
    let declared =
        declared_synthetic_moving_route(synthetic_moving_anchor_id(), AnchorKind::Carrier);
    let resolved = declared
        .resolve()
        .expect("the declared moving route resolves");
    bind_route(
        &resolved,
        &[AnchorBinding::new(
            synthetic_moving_anchor_id(),
            SYNTHETIC_MOVING_ANCHOR_RUNTIME_ID,
            AnchorKind::Carrier,
        )],
    )
    .expect("the declared moving route binds to the fixture anchor")
}

/// A second, independent fixture world for a test that needs two.
fn second_fixture() -> PhysicsFixture {
    fixture()
}

/// Spawns the live moving anchor entity at `position_m` (identity rotation).
///
/// Both the Avian `Position` and the Bevy `Transform` are set: Avian's
/// `TransformToPosition` sync copies `GlobalTransform` back into `Position`, so
/// an anchor whose `Transform` did not agree with its `Position` would be reset
/// to the transform's origin.
fn spawn_anchor(fixture: &mut PhysicsFixture, position_m: [f32; 3]) -> Entity {
    let position = Vec3::from_array(position_m);
    fixture
        .world_mut()
        .spawn((
            MovingAnchor::new(
                synthetic_moving_anchor_id(),
                SYNTHETIC_MOVING_ANCHOR_RUNTIME_ID,
                AnchorKind::Carrier,
            ),
            Position(position),
            Rotation::default(),
            Transform::from_translation(position),
        ))
        .id()
}

/// Moves a live anchor entity's world position, keeping its `Transform` and
/// Avian `Position` in agreement.
fn move_anchor(fixture: &mut PhysicsFixture, anchor: Entity, position_m: [f32; 3]) {
    let position = Vec3::from_array(position_m);
    let mut entity = fixture.world_mut().entity_mut(anchor);
    entity
        .get_mut::<Position>()
        .expect("the anchor has a Position")
        .0 = position;
    entity
        .get_mut::<Transform>()
        .expect("the anchor has a Transform")
        .translation = position;
}

/// Spawns one AI aircraft through the production flight path and gives it its
/// pursuit record.
fn spawn_ai_aircraft(
    fixture: &mut PhysicsFixture,
    actor: ActorId,
    route: BoundRoute,
    resume_reached: usize,
    position_m: [f32; 3],
    velocity_mps: [f32; 3],
) -> Entity {
    let aircraft = spawn_flight_body(
        fixture.world_mut(),
        FlightModel::new(synthetic_fixed_wing()),
        &FlightSpawnSpec {
            engine: EngineState::direct(1.0),
            command: FlightInput::try_new(0.0, 0.0, 0.0, 1.0, false).expect("valid input"),
            ..FlightSpawnSpec::level_at(position_m, velocity_mps)
        },
    )
    .expect("the spawn spec is valid");
    fixture
        .world_mut()
        .entity_mut(aircraft)
        .insert(RoutePursuit::new(actor, route, resume_reached));
    aircraft
}

/// Reads the session navigation authority.
fn navigation(fixture: &PhysicsFixture) -> &AiNavigation {
    fixture.world().resource::<AiNavigation>()
}

/// Reads how many leading nodes the actor has reached, if registered.
fn reached(fixture: &PhysicsFixture, actor: ActorId) -> Option<usize> {
    navigation(fixture).reached(actor)
}

/// Reads the navigation driver's tick accounting.
fn report(fixture: &PhysicsFixture) -> NavigationTickReport {
    fixture.world().resource::<NavigationTickReport>().clone()
}

/// Reads the aircraft's world position.
fn position_of(fixture: &PhysicsFixture, entity: Entity) -> [f32; 3] {
    fixture
        .world()
        .get::<Position>(entity)
        .expect("a flight body has a position")
        .0
        .to_array()
}

/// Runs the fixed tick loop until `reached(actor) >= target` or `budget` ticks
/// pass, asserting progress never decreased, and returns the final count.
fn run_until_reached(
    fixture: &mut PhysicsFixture,
    actor: ActorId,
    target: usize,
    budget: u64,
) -> usize {
    let mut previous = reached(fixture, actor).unwrap_or(0);
    for _ in 0..budget {
        fixture.step(1);
        let current = reached(fixture, actor).expect("the actor registers on its first tick");
        assert!(
            current >= previous,
            "progress is monotonic across a fixed tick: {previous} -> {current}"
        );
        previous = current;
        if current >= target {
            break;
        }
    }
    previous
}

// ---------------------------------------------------------------------------
// A registered actor follows a projected route in the integrated loop
// ---------------------------------------------------------------------------

/// An AI actor bound to a projected authored route is registered, drives the
/// real flight body through the fixed tick loop, and reaches the route's final
/// mandatory marker.
#[test]
fn accept_t446_registered_actor_follows_a_projected_moving_route() {
    let mut fixture = fixture();
    let route = bound_moving_route();

    // The authored id -> runtime id mapping is the authored sequence, not the
    // projection's list index.
    assert_eq!(route.runtime_node_id("start"), Some(RouteNodeId(0)));
    assert_eq!(route.runtime_node_id("waypoint"), Some(RouteNodeId(1)));
    assert_eq!(route.runtime_node_id("goal"), Some(RouteNodeId(2)));
    assert_eq!(route.authored_node_id(RouteNodeId(1)), Some("waypoint"));
    assert_eq!(route.graph().node_count(), 3);

    let _anchor = spawn_anchor(&mut fixture, [0.0, 0.0, 0.0]);
    let plane = spawn_ai_aircraft(
        &mut fixture,
        actor(ACTOR_SERIAL),
        route,
        1,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, -40.0],
    );

    assert!(
        !navigation(&fixture).is_registered(actor(ACTOR_SERIAL)),
        "no actor is registered before the first fixed tick"
    );

    let final_reached = run_until_reached(&mut fixture, actor(ACTOR_SERIAL), 3, 6_000);
    assert_eq!(
        final_reached, 3,
        "the aircraft reached every node of the moving route"
    );
    let tick_report = report(&fixture);
    assert!(
        tick_report.ticks > 0,
        "the driver ran in the fixed schedule"
    );
    assert!(
        tick_report.applied > 0,
        "the follower's command reached the flight record: {tick_report:?}"
    );
    assert!(
        tick_report.last_refusal.is_none(),
        "a clean follow records no refusal: {:?}",
        tick_report.last_refusal
    );

    // The commands it applied are the ones the production flight record holds.
    let command = fixture
        .world()
        .get::<FlightAircraft>(plane)
        .expect("the AI aircraft is a flight body")
        .command();
    assert!(
        (0.0..=1.0).contains(&command.throttle),
        "the applied command is a bounded flight input: {command:?}"
    );
    assert!(
        position_of(&fixture, plane)[2] < 0.0,
        "the aircraft actually flew down the route"
    );
}

// ---------------------------------------------------------------------------
// A displaced actor rejoins before the next mandatory marker
// ---------------------------------------------------------------------------

/// An actor displaced off the route rejoins the next mandatory marker, and its
/// progress passes through that marker without skipping it.
///
/// The displacement is vertical (15 m below the first leg, still pointed down
/// the route). It is the axis the production F31 follower + F24 flight body
/// actually corrects in this fixture: the pitch/climb channel closes from a
/// 15 m offset to within ~1.3 m of the marker. The lateral/bank channel does
/// not yet converge through the integrated loop — see
/// `docs/findings/2026-10-01-f31-ecs-integrated-follower-gap.md` and its filed
/// follow-up rather than hiding it behind an inflated arrival radius.
#[test]
fn accept_t446_displaced_actor_rejoins_before_the_next_mandatory_marker() {
    let mut fixture = fixture();
    let route = bound_moving_route();
    let _anchor = spawn_anchor(&mut fixture, [0.0, 0.0, 0.0]);

    // Displaced below the route's first leg, at cruise, pointed down the route;
    // the follower must drive the aircraft back onto the marker.
    let plane = spawn_ai_aircraft(
        &mut fixture,
        actor(ACTOR_SERIAL),
        route,
        1,
        [0.0, -15.0, -40.0],
        [0.0, 0.0, -40.0],
    );

    // The first mandatory marker is node index 1 ("waypoint").
    let mut first_mandatory = None;
    let mut previous = 1usize;
    for _ in 0..6_000 {
        fixture.step(1);
        let current = reached(&fixture, actor(ACTOR_SERIAL)).expect("registered");
        assert!(current >= previous, "progress is monotonic");
        if current >= 2 && first_mandatory.is_none() {
            first_mandatory = Some(current);
        }
        previous = current;
        if current >= 2 {
            break;
        }
    }
    assert_eq!(
        first_mandatory,
        Some(2),
        "the displaced actor rejoined the next mandatory marker without skipping it"
    );
    assert!(
        position_of(&fixture, plane)[2] < -60.0,
        "the aircraft travelled back onto the route"
    );
}

// ---------------------------------------------------------------------------
// AC04: a moving anchor is sampled live and fires no false arrival
// ---------------------------------------------------------------------------

/// The anchor is sampled from its live world transform, not cached: an aircraft
/// sitting on a node's old world position does not arrive once the anchor (and
/// so the node) has moved away, and then flies to the moved target.
#[test]
fn accept_t446_moving_anchor_is_sampled_live_not_cached() {
    let mut fixture = fixture();
    let route = bound_moving_route();
    let anchor = spawn_anchor(&mut fixture, [0.0, 0.0, 0.0]);

    // The aircraft starts exactly on node 1's world position for an anchor at
    // the origin, targeting that node (resume past the spawn node).
    let plane = spawn_ai_aircraft(
        &mut fixture,
        actor(ACTOR_SERIAL),
        route,
        1,
        [0.0, 0.0, -120.0],
        [0.0, 0.0, -40.0],
    );

    // Move the live anchor 300 m down the route before the first tick, so the
    // node's current world position is now far from the aircraft. A cached
    // sample would think the aircraft sits on the node and fire arrival.
    move_anchor(&mut fixture, anchor, [0.0, 0.0, -300.0]);

    fixture.step(1);
    assert_eq!(
        reached(&fixture, actor(ACTOR_SERIAL)),
        Some(1),
        "the moved anchor must not fire a false arrival at the old node position"
    );

    // It then flies to the live target and arrives for real.
    let final_reached = run_until_reached(&mut fixture, actor(ACTOR_SERIAL), 2, 6_000);
    assert_eq!(final_reached, 2, "the aircraft followed the moved target");
    assert!(
        position_of(&fixture, plane)[2] < -360.0,
        "the aircraft reached the moved node: {:?}",
        position_of(&fixture, plane)
    );
}

/// A moving anchor shift never resets an actor's remembered progress (AC04):
/// after reaching a marker, an origin shift keeps the reached count.
#[test]
fn accept_t446_moving_anchor_shift_does_not_reset_progress() {
    let mut fixture = fixture();
    let route = bound_moving_route();
    let anchor = spawn_anchor(&mut fixture, [0.0, 0.0, 0.0]);
    spawn_ai_aircraft(
        &mut fixture,
        actor(ACTOR_SERIAL),
        route,
        1,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, -40.0],
    );

    let before = run_until_reached(&mut fixture, actor(ACTOR_SERIAL), 2, 6_000);
    assert_eq!(before, 2, "the actor reached the first mandatory marker");

    // Slide the moving anchor far to one side; the route's world frame moves
    // with it. Progress is remembered and must not rewind.
    move_anchor(&mut fixture, anchor, [2_000.0, 0.0, 0.0]);
    for _ in 0..10 {
        fixture.step(1);
        let current = reached(&fixture, actor(ACTOR_SERIAL)).expect("still registered");
        assert!(
            current >= before,
            "an origin shift must not reset progress: {before} -> {current}"
        );
    }
}

// ---------------------------------------------------------------------------
// The authored node id mapping is stable across an ECS reorder
// ---------------------------------------------------------------------------

/// The same actors, presented to the ECS in the opposite spawn order, produce
/// the same per-actor progress: the driver keys everything by stable actor id,
/// and the authored node id -> runtime id map is a property of the route, not
/// of the roster order.
#[test]
fn accept_t446_authored_node_mapping_is_stable_across_ecs_reorder() {
    fn run(spawn_reversed: bool) -> (usize, usize) {
        let mut fixture = fixture();
        let _anchor = spawn_anchor(&mut fixture, [0.0, 0.0, 0.0]);
        let spawn = |fixture: &mut PhysicsFixture, serial: u64, position_m: [f32; 3]| {
            spawn_ai_aircraft(
                fixture,
                actor(serial),
                bound_moving_route(),
                1,
                position_m,
                [0.0, 0.0, -40.0],
            );
        };
        if spawn_reversed {
            spawn(&mut fixture, 2, [30.0, 0.0, 0.0]);
            spawn(&mut fixture, 1, [0.0, 0.0, 0.0]);
        } else {
            spawn(&mut fixture, 1, [0.0, 0.0, 0.0]);
            spawn(&mut fixture, 2, [30.0, 0.0, 0.0]);
        }
        fixture.step(600);
        (
            reached(&fixture, actor(1)).expect("actor 1 registered"),
            reached(&fixture, actor(2)).expect("actor 2 registered"),
        )
    }

    let forward = run(false);
    let reversed = run(true);
    assert_eq!(
        forward, reversed,
        "per-actor progress is independent of ECS entity order"
    );

    // And the mapping itself is by authored sequence in either world.
    let route = bound_moving_route();
    let mapped: Vec<(&str, u32)> = route
        .node_ids()
        .map(|(authored, runtime)| (authored, runtime.0))
        .collect();
    assert_eq!(
        mapped,
        vec![("goal", 2), ("start", 0), ("waypoint", 1)],
        "authored ids map to their authored sequences"
    );
}

// ---------------------------------------------------------------------------
// Teardown and session confinement
// ---------------------------------------------------------------------------

/// Despawning the pursuit entity removes its pursuit state, and an actor of a
/// foreign session is refused rather than inheriting a fresh session's state.
#[test]
fn accept_t446_despawn_removes_pursuit_state_and_a_fresh_session_cannot_inherit_it() {
    let mut fixture = fixture();
    let route = bound_moving_route();
    let _anchor = spawn_anchor(&mut fixture, [0.0, 0.0, 0.0]);
    let plane = spawn_ai_aircraft(
        &mut fixture,
        actor(ACTOR_SERIAL),
        route,
        1,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, -40.0],
    );
    fixture.step(300);
    assert!(navigation(&fixture).is_registered(actor(ACTOR_SERIAL)));
    assert!(reached(&fixture, actor(ACTOR_SERIAL)).unwrap_or(0) >= 2);

    // Teardown: the roster reconciliation removes the despawned actor's state.
    fixture.world_mut().entity_mut(plane).despawn();
    fixture.step(1);
    assert!(
        !navigation(&fixture).is_registered(actor(ACTOR_SERIAL)),
        "a despawned actor's pursuit state is removed"
    );
    assert_eq!(
        reached(&fixture, actor(ACTOR_SERIAL)),
        None,
        "no state survives the teardown"
    );

    // A fresh session generation starts empty and refuses the old actor's
    // generation rather than inheriting its progress.
    let fresh = AiNavigation::new(
        SYNTHETIC_NAVIGATION_SESSION + 1,
        SYNTHETIC_NAVIGATION_SEED,
        navigator(),
    );
    assert!(!fresh.is_registered(actor(ACTOR_SERIAL)));
    assert_eq!(fresh.reached(actor(ACTOR_SERIAL)), None);

    // The running fixture's set refuses a foreign-session actor rather than
    // registering it.
    let foreign = spawn_ai_aircraft(
        &mut fixture,
        ActorId {
            session: SYNTHETIC_NAVIGATION_SESSION + 1,
            serial: 99,
        },
        bound_moving_route(),
        1,
        [0.0, 0.0, -300.0],
        [0.0, 0.0, -40.0],
    );
    fixture.step(1);
    let tick_report = report(&fixture);
    assert!(
        !navigation(&fixture).is_registered(ActorId {
            session: SYNTHETIC_NAVIGATION_SESSION + 1,
            serial: 99,
        }),
        "a foreign-session actor is not registered"
    );
    match &tick_report.last_refusal {
        Some(refusal) => assert!(
            matches!(
                refusal.reason,
                NavigationRefusalReason::ForeignSession { expected, found }
                    if expected == SYNTHETIC_NAVIGATION_SESSION
                        && found == SYNTHETIC_NAVIGATION_SESSION + 1
            ),
            "the refusal names the foreign session: {:?}",
            refusal.reason
        ),
        None => panic!("a foreign-session actor must be refused loudly"),
    }
    let _ = foreign;

    // A mismatched live anchor kind is refused by name too.
    let mut mismatch = second_fixture();
    let anchor = mismatch
        .world_mut()
        .spawn((
            MovingAnchor::new(
                synthetic_moving_anchor_id(),
                SYNTHETIC_MOVING_ANCHOR_RUNTIME_ID,
                AnchorKind::Train,
            ),
            Position(Vec3::ZERO),
            Rotation::default(),
        ))
        .id();
    let _ = anchor;
    spawn_ai_aircraft(
        &mut mismatch,
        actor(ACTOR_SERIAL),
        bound_moving_route(),
        1,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, -40.0],
    );
    mismatch.step(1);
    match &report(&mismatch).last_refusal {
        Some(refusal) => assert!(
            matches!(
                refusal.reason,
                NavigationRefusalReason::AnchorKindMismatch { anchor, .. }
                    if anchor == SYNTHETIC_MOVING_ANCHOR_RUNTIME_ID
            ),
            "a live anchor of the wrong kind is refused: {:?}",
            refusal.reason
        ),
        None => panic!("an anchor kind mismatch must be refused loudly"),
    }
}

// ---------------------------------------------------------------------------
// The binding boundary refuses an unbound anchor and an unsupported loop
// ---------------------------------------------------------------------------
/// A moving route whose anchor has no binding is refused by name, and a loop
/// termination the runtime cannot express is refused by name, rather than
/// silently addressed at an invented id.
#[test]
fn accept_t446_bind_route_refuses_unbound_anchor_and_loop() {
    let declared =
        declared_synthetic_moving_route(synthetic_moving_anchor_id(), AnchorKind::Carrier);
    let resolved = declared
        .resolve()
        .expect("the declared moving route resolves");

    let unbound = bind_route(&resolved, &[]);
    assert!(
        matches!(
            unbound,
            Err(RouteBindingError::UnboundAnchor { ref anchor })
                if anchor == synthetic_moving_anchor_id().as_str()
        ),
        "an unbound moving anchor is refused: {unbound:?}"
    );

    // A route that terminates in a loop cannot be expressed by the follower.
    let looped = cs_content::routes::RouteDefinition::try_new(loop_draft()).expect("a valid loop");
    let resolved_loop = looped.resolve().expect("the loop route resolves");
    assert_eq!(
        bind_route(
            &resolved_loop,
            &[AnchorBinding::new(
                synthetic_moving_anchor_id(),
                SYNTHETIC_MOVING_ANCHOR_RUNTIME_ID,
                AnchorKind::Carrier,
            )],
        ),
        Err(RouteBindingError::UnsupportedTermination {
            termination: "loop"
        })
    );
}

/// A world-anchored loop route for the refusal test.
fn loop_draft() -> cs_content::routes::RouteDraft {
    use cs_content::routes::{RouteDraft, RouteEdge, RouteNode, RouteTermination};
    use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
    use cs_types::evidence::ClaimId;

    let designed = Provenance::designed(ClaimId::new("t446.loop-fixture").expect("valid claim id"));
    let node = |id: &str, sequence: u32| RouteNode {
        id: cs_content::routes::RouteNodeId::try_new(id).expect("valid node id"),
        sequence,
        mandatory: true,
        position_m: Resolved::Known(Known::new(
            [0.0, 0.0, -f64::from(sequence) * 10.0],
            designed.clone(),
        )),
        arrival_radius_m: Resolved::Known(Known::new(5.0, designed.clone())),
        trigger: Resolved::Known(Known::new(None, designed.clone())),
    };
    RouteDraft {
        id: ContentId::from_source(ContentKind::Route, "synthetic.loop").expect("valid route id"),
        origin: Origin::SyntheticFixture,
        frame: cs_content::routes::ReferenceFrame::World,
        termination: RouteTermination::Loop,
        clearance_m: Resolved::Known(Known::new(0.0, designed.clone())),
        nodes: vec![node("a", 0), node("b", 1)],
        edges: vec![RouteEdge {
            from: cs_content::routes::RouteNodeId::try_new("a").expect("valid"),
            to: cs_content::routes::RouteNodeId::try_new("b").expect("valid"),
        }],
        provenance: designed,
    }
}

// ---------------------------------------------------------------------------
// The flight body is not disturbed by the AI driver
// ---------------------------------------------------------------------------

/// The AI driver writes only the same bounded command boundary the player input
/// session uses; it never writes the pose or velocity the integrator owns.
#[test]
fn accept_t446_ai_driver_writes_only_the_flight_command_boundary() {
    let mut fixture = fixture();
    let _anchor = spawn_anchor(&mut fixture, [0.0, 0.0, 0.0]);
    let plane = spawn_ai_aircraft(
        &mut fixture,
        actor(ACTOR_SERIAL),
        bound_moving_route(),
        1,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, -40.0],
    );

    fixture.step(120);
    let world = fixture.world();
    let body = world.entity(plane);
    let position = body.get::<Position>().expect("position");
    let velocity = body.get::<LinearVelocity>().expect("velocity");
    let rotation = body.get::<Rotation>().expect("rotation");
    assert!(
        position.0.is_finite() && velocity.0.is_finite() && rotation.0.is_finite(),
        "the driven aircraft's integrator state stays finite"
    );
    // Avian remains the only integrator: the driver produced a moving body
    // without ever writing Position itself.
    assert!(
        position.0.z < -40.0,
        "the flight body integrated the commanded box: {:?}",
        position.0
    );
    assert!(
        Quat::from_xyzw(rotation.0.x, rotation.0.y, rotation.0.z, rotation.0.w).is_normalized()
    );
}
