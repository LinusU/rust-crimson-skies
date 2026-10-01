//! Acceptance scenarios for task #457: the ECS-integrated F31 follower binds and
//! flies a declared loop-terminated route.
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md` (stage
//! `### F31-C`, spec non-negotiable behaviors 1 and 3). Task test prefix:
//! `accept_t457_`.
//!
//! #447 gave the runtime follower real loop semantics (`RouteTermination`,
//! `RouteProgress::laps`, a re-armed target) and `tools/cs_inspect`'s
//! `project_route` carries the declared termination across. This task closes
//! the last gap: `cs_app::ai::bind_route` now carries the same declaration
//! into the mission ECS, so a declared `Loop` record reaches the aircraft that
//! flies it instead of being refused at the binding boundary.
//!
//! These tests drive production code only: the real [`PhysicsFixture`] world
//! with the production [`FlightForcesPlugin`] and [`AiNavigationPlugin`], the
//! `cs_content` declared loop mapped through [`bind_route`], and the
//! production [`spawn_flight_body`] path. Every value is newly authored
//! synthetic fixture data — no original game data, and no test reads
//! `CS_GAME_DIR`.
//!
//! # What this does and does not claim
//!
//! The wrap this asserts is real: the follower reaches the loop's last node,
//! the target re-arms to node 0, `reached()` stays monotonic across the wrap
//! and `laps()` counts it. The scenario is also **causal**: an identical spawn
//! flown without the navigation plugin (`unguided_flight`) diverges from the
//! guided aircraft by more than 5 m of altitude, so the loop flight cannot be an
//! artifact of a route laid out along the airframe's own trim path.
//!
//! **Several** laps through the *integrated* loop are
//! not claimed, and cannot be until #451: a closed circuit needs a U-turn, and
//! through the production flight loop the F24 synthetic airframe turns the
//! wrong way (`roll` sign) and far too slowly (envelope/airframe mismatch), so
//! the aircraft cannot fly back to node 0. That is filed, measured and
//! unclaimed in `docs/findings/2026-10-01-f31-ecs-integrated-follower-gap.md`;
//! the several-lap behavior of the follower itself is pinned at the runtime
//! level in `crates/cs_sim/tests/accept_t447_loop_route_progress.rs`. This
//! task asserts exactly the re-arming and laps the integrated follower can
//! reach, and no lateral rejoin.
//!
//! Whether the original 2000 route encoding expresses a loop at all is
//! **unmeasured** (F13; F31-D measured only the `aiv.zrd` carrier, and the
//! route decode is filed as #455 `F31-ROUTE-ENCODING`). These tests pin the
//! designed loop semantics, not an original-data claim.

use avian3d::prelude::Position;
use bevy::prelude::Entity;

use cs_app::ai::{
    AiNavigation, AiNavigationPlugin, BoundRoute, NavigationRefusalReason, NavigationTickReport,
    RoutePursuit, SYNTHETIC_NAVIGATION_SEED, SYNTHETIC_NAVIGATION_SESSION, bind_route,
};
use cs_app::physics::{
    FixtureBodySpec, FlightForcesPlugin, FlightSpawnSpec, PhysicsFixture, spawn_flight_body,
};
use cs_content::routes::{
    ReferenceFrame, RouteDefinition, RouteDraft, RouteEdge, RouteNode, RouteNodeId,
    RouteTermination,
};
use cs_sim::ai::navigation::{
    RouteNodeId as NavRouteNodeId, RouteTermination as NavRouteTermination,
};
use cs_sim::damage::ActorId;
use cs_sim::flight::{EngineState, FlightInput, FlightModel, synthetic_fixed_wing};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

/// The serial of the fixture AI actor.
const ACTOR_SERIAL: u64 = 7;

/// The fixture actor, in the fixture session.
fn actor(serial: u64) -> ActorId {
    ActorId {
        session: SYNTHETIC_NAVIGATION_SESSION,
        serial,
    }
}

/// The `route` content id of the fixture loop route.
const LOOP_ROUTE_ID: &str = "synthetic.ecs-loop-patrol";

/// A declared, world-anchored **loop** route: three collinear nodes straight
/// down `-Z`, the last of which re-arms the first.
///
/// The geometry is deliberately collinear rather than a circle: the follower
/// must converge on every node it can reach through the integrated flight loop
/// before the wrap, so each leg is one the production pitch channel and the
/// straight-line body can actually fly. See the module doc for what the wrap
/// leg cannot do yet (#451).
///
/// Every value is newly authored project design, not measured original data.
fn declared_loop_route(termination: RouteTermination) -> RouteDefinition {
    let designed = || {
        Provenance::designed(ClaimId::new("t457.ecs-loop-route").expect("the claim id is valid"))
    };
    let node = |id: &str, sequence: u32, position_z_m: f64, radius_m: f64| RouteNode {
        id: RouteNodeId::try_new(id).expect("the fixture node id is valid"),
        sequence,
        mandatory: sequence > 0,
        position_m: Resolved::Known(Known::new([0.0, 0.0, position_z_m], designed())),
        arrival_radius_m: Resolved::Known(Known::new(radius_m, designed())),
        trigger: Resolved::Known(Known::new(None, designed())),
    };
    RouteDefinition::try_new(RouteDraft {
        id: ContentId::from_source(ContentKind::Route, LOOP_ROUTE_ID)
            .expect("the fixture route id is valid"),
        origin: Origin::SyntheticFixture,
        frame: ReferenceFrame::World,
        termination,
        clearance_m: Resolved::Known(Known::new(0.0, designed())),
        nodes: vec![
            node("entry", 0, 0.0, 8.0),
            node("leg", 1, -120.0, 10.0),
            node("turn", 2, -240.0, 10.0),
        ],
        edges: vec![
            RouteEdge {
                from: RouteNodeId::try_new("entry").expect("valid"),
                to: RouteNodeId::try_new("leg").expect("valid"),
            },
            RouteEdge {
                from: RouteNodeId::try_new("leg").expect("valid"),
                to: RouteNodeId::try_new("turn").expect("valid"),
            },
        ],
        provenance: designed(),
    })
    .expect("the declared loop route is valid")
}

/// The declared loop route resolved and bound into the runtime graph.
fn bound_loop_route() -> BoundRoute {
    let resolved = declared_loop_route(RouteTermination::Loop)
        .resolve()
        .expect("the declared loop route resolves");
    bind_route(&resolved, &[]).expect("a declared loop binds to the follower")
}

/// The shared fixture body spec: one tiny free-flying box far from any fixture
/// route, so nothing is inherited from the fixture's own body.
fn fixture_spec() -> FixtureBodySpec {
    FixtureBodySpec {
        mass_kg: 1.0,
        half_extents_m: [0.05, 0.05, 0.05],
        position_m: [-10_000.0, 0.0, 0.0],
        linear_velocity_m_s: [0.0; 3],
    }
}

/// A flight world: the real physics fixture with the production flight driver
/// and the F31 navigation driver.
fn fixture() -> PhysicsFixture {
    PhysicsFixture::builder(fixture_spec())
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

/// Spawns one flight body through the production flight path, at cruise down
/// `-Z` from `position_m`.
fn spawn_aircraft(fixture: &mut PhysicsFixture, position_m: [f32; 3]) -> Entity {
    spawn_flight_body(
        fixture.world_mut(),
        FlightModel::new(synthetic_fixed_wing()),
        &FlightSpawnSpec {
            engine: EngineState::direct(1.0),
            command: FlightInput::try_new(0.0, 0.0, 0.0, 1.0, false).expect("valid input"),
            ..FlightSpawnSpec::level_at(position_m, [0.0, 0.0, -40.0])
        },
    )
    .expect("the spawn spec is valid")
}

/// The control flight: the **identical** production spawn in a world with the
/// production flight driver but **no** AI navigation plugin, so it flies on the
/// airframe's own trim alone. The differential against this flight is what makes
/// "the follower flew the loop" a causal claim rather than a coincidence of
/// geometry (see `accept_t457_a_bound_loop_route_is_flown_and_re_arms_at_the_wrap`).
fn unguided_flight() -> (PhysicsFixture, Entity) {
    let mut fixture = PhysicsFixture::builder(fixture_spec())
        .configure(|app| {
            app.add_plugins(FlightForcesPlugin);
        })
        .build()
        .expect("the fixture spec is valid");
    let aircraft = spawn_aircraft(&mut fixture, [0.0, 0.0, 0.0]);
    (fixture, aircraft)
}

/// Spawns one AI aircraft through the production flight path, on node 0 of the
/// loop heading down it, and gives it its pursuit record resuming past the node
/// it starts on.
fn spawn_ai_aircraft(
    fixture: &mut PhysicsFixture,
    actor: ActorId,
    route: BoundRoute,
    resume_reached: usize,
) -> Entity {
    let aircraft = spawn_aircraft(fixture, [0.0, 0.0, 0.0]);
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

/// Reads how many nodes the actor has reached in total, if registered.
fn reached(fixture: &PhysicsFixture, actor: ActorId) -> Option<usize> {
    navigation(fixture).reached(actor)
}

/// Reads how many times the actor's route has re-armed, if registered.
fn laps(fixture: &PhysicsFixture, actor: ActorId) -> Option<u32> {
    navigation(fixture).laps(actor)
}

/// Reads the index of the node the actor is currently targeting, if registered.
fn target_index(fixture: &PhysicsFixture, actor: ActorId) -> Option<usize> {
    navigation(fixture)
        .set()
        .state(actor)
        .map(|state| state.progress().next_index())
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

// ---------------------------------------------------------------------------
// bind_route carries the declared loop into the runtime graph
// ---------------------------------------------------------------------------

/// A declared `Loop` record binds as a loop: the termination reaches the graph
/// the follower consumes, the authored node id -> runtime node id map is
/// unchanged, and the route reports the resume bound a loop route has.
#[test]
fn accept_t457_bind_route_carries_a_declared_loop_termination_into_the_graph() {
    let bound = bound_loop_route();
    assert_eq!(
        bound.termination(),
        NavRouteTermination::Loop,
        "the bound route reads back the declared termination"
    );
    assert_eq!(
        bound.graph().termination(),
        NavRouteTermination::Loop,
        "the graph the follower consumes is a loop, not an ending route"
    );
    assert_eq!(bound.graph().node_count(), 3);

    // The authored id -> runtime id map is the authored sequence, untouched by
    // the loop: a mission event bound by authored name survives the wrap.
    assert_eq!(bound.runtime_node_id("entry"), Some(NavRouteNodeId(0)));
    assert_eq!(bound.runtime_node_id("leg"), Some(NavRouteNodeId(1)));
    assert_eq!(bound.runtime_node_id("turn"), Some(NavRouteNodeId(2)));

    // A loop is resumed at most from its last node: any higher count is a
    // progress with no live target, which can never wrap.
    assert_eq!(
        bound.max_resume_reached(),
        bound.graph().node_count() - 1,
        "a loop route's resume headroom stops at its last node"
    );

    // An ending route over the same geometry keeps the ending semantics and its
    // own resume bound, so the loop branch is not a blanket rule.
    let ended = declared_loop_route(RouteTermination::End);
    let bound_end = bind_route(&ended.resolve().expect("the end route resolves"), &[])
        .expect("a declared ending route binds");
    assert_eq!(bound_end.termination(), NavRouteTermination::End);
    assert_eq!(
        bound_end.max_resume_reached(),
        bound_end.graph().node_count(),
        "an ending route may legitimately resume in its finished state"
    );
}

// ---------------------------------------------------------------------------
// The ECS-integrated follower flies the loop and re-arms at the wrap
// ---------------------------------------------------------------------------

/// The core scenario: an AI aircraft bound to a declared loop route through
/// `bind_route` is driven by the production ECS navigation loop, converges on
/// every node before the wrap, reaches the loop's last node, and re-arms: the
/// target goes back to node 0, `reached()` is monotonic across the wrap, and
/// `laps()` counts it. The route is never reported complete.
///
/// The flight is *caused* by the follower, not by the geometry: the same
/// production spawn without the navigation plugin (`unguided_flight`) flies a
/// materially different trajectory over the same ticks, so "the loop was flown"
/// cannot be an artifact of a route laid out along the spawn's own trim path.
/// The arrivals are the set's (`NavigationSet` owns progress), the command
/// reaches the flight record, and the wrap is real.
///
/// See the module doc: additional laps need the wrap leg, which the integrated
/// follower cannot fly until #451. This asserts the wrap and the re-arm, which
/// is what the integrated loop can reach, and no lateral rejoin.
#[test]
fn accept_t457_a_bound_loop_route_is_flown_and_re_arms_at_the_wrap() {
    let mut fixture = fixture();
    let (mut unguided, unguided_plane) = unguided_flight();
    let route = bound_loop_route();
    let actor = actor(ACTOR_SERIAL);

    assert!(
        !navigation(&fixture).is_registered(actor),
        "no actor is registered before the first fixed tick"
    );

    let plane = spawn_ai_aircraft(&mut fixture, actor, route.clone(), 1);

    // Fly the loop. Progress must never decrease, not even across the wrap. The
    // control flight is advanced in lockstep, so the differential below compares
    // two aircraft that have flown exactly the same number of ticks.
    let mut previous = reached(&fixture, actor).unwrap_or(1);
    for _ in 0..6_000 {
        fixture.step(1);
        unguided.step(1);
        let current = reached(&fixture, actor).expect("the actor registers on its first tick");
        assert!(
            current >= previous,
            "progress is monotonic across the wrap: {previous} -> {current}"
        );
        previous = current;
        if laps(&fixture, actor).is_some_and(|laps| laps > 0) {
            break;
        }
    }

    // Every node before the wrap was really converged on, through the real
    // flight body: the total equals the node count and the body flew the legs.
    let total = reached(&fixture, actor).expect("registered");
    assert_eq!(
        total,
        route.graph().node_count(),
        "the follower converged on every node of the loop before the wrap"
    );
    assert_eq!(
        laps(&fixture, actor),
        Some(1),
        "reaching the last node re-armed the route once"
    );
    assert_eq!(
        target_index(&fixture, actor),
        Some(0),
        "the re-armed target is node 0 again, not a node past the end"
    );
    assert!(
        !navigation(&fixture).is_complete(actor, route.graph()),
        "a loop route re-arms instead of reporting completion"
    );
    let position = position_of(&fixture, plane);
    assert!(
        position[2] < -200.0,
        "the aircraft actually flew down the loop's legs to its last node: {position:?}"
    );

    // The differential: the flight that reached the loop's nodes is the
    // follower's flight and not the airframe's own trim. The follower commands a
    // climb back toward the marker line (measured: the guided aircraft rises
    // ~10 m above the route by the wrap, while the trim-only control sinks ~8 m
    // below it). A collinear route would otherwise be satisfiable by flying
    // straight, so this is what makes the scenario discriminating.
    let unguided_position = position_of(&unguided, unguided_plane);
    assert!(
        (position[1] - unguided_position[1]).abs() > 5.0,
        "the guided flight is materially different from the trim-only control: guided \
         {position:?} vs unguided {unguided_position:?}"
    );

    let tick_report = report(&fixture);
    assert!(
        tick_report.applied > 0,
        "the follower's command reached the flight record: {tick_report:?}"
    );
    assert!(
        tick_report.last_refusal.is_none(),
        "a clean loop follow records no refusal: {:?}",
        tick_report.last_refusal
    );

    // Progress keeps its monotonic contract after the wrap as well, across the
    // leg the follower cannot yet fly (#451).
    let mut previous = reached(&fixture, actor).expect("still registered");
    for _ in 0..600 {
        fixture.step(1);
        let current = reached(&fixture, actor).expect("still registered");
        assert!(
            current >= previous,
            "progress is monotonic after the wrap: {previous} -> {current}"
        );
        previous = current;
    }
    assert_eq!(
        laps(&fixture, actor),
        Some(1),
        "the wrap is recorded once; further laps need the #451 turn fix"
    );
}

// ---------------------------------------------------------------------------
// A resume past the last node of a loop is refused by name
// ---------------------------------------------------------------------------

/// Resuming a loop at its node count would register a progress with no live
/// target: the follower holds station forever and the wrap can never happen.
/// The registration API takes no route, so the driver checks the bound where
/// the route is in hand and refuses by name instead of registering it.
#[test]
fn accept_t457_a_loop_resume_past_the_last_node_is_refused_by_name() {
    let mut refused = fixture();
    let route = bound_loop_route();
    let actor = actor(ACTOR_SERIAL);

    spawn_ai_aircraft(
        &mut refused,
        actor,
        route.clone(),
        route.graph().node_count(),
    );
    refused.step(2);

    assert!(
        !navigation(&refused).is_registered(actor),
        "an un-flyable resume is never registered"
    );
    assert_eq!(reached(&refused, actor), None, "no progress is recorded");
    match &report(&refused).last_refusal {
        Some(refusal) => assert!(
            matches!(
                refusal.reason,
                NavigationRefusalReason::ResumePastRouteEnd {
                    resume_reached,
                    max_resume_reached
                } if resume_reached == route.graph().node_count()
                    && max_resume_reached == route.graph().node_count() - 1
            ),
            "the refusal names the bound: {:?}",
            refusal.reason
        ),
        None => panic!("an un-flyable resume must be refused loudly"),
    }

    // The largest legal resume count registers and flies, so the refusal is
    // about the over-count and not about loops in general.
    let mut legal = fixture();
    let aircraft = spawn_ai_aircraft(&mut legal, actor, route.clone(), route.max_resume_reached());
    legal.step(4);
    assert!(
        navigation(&legal).is_registered(actor),
        "resuming from the last node is a legal state: {:?}",
        report(&legal).last_refusal
    );
    assert_eq!(
        laps(&legal, actor),
        Some(0),
        "a legal resume has not wrapped yet"
    );
    assert!(
        position_of(&legal, aircraft)
            .iter()
            .all(|value| value.is_finite()),
        "the aircraft keeps flying from the last node"
    );
}

/// An **ending** route may still be resumed in its finished state (past its
/// last node), which is a real mission placement; the new bound does not
/// refuse it.
#[test]
fn accept_t457_an_ending_route_resume_past_its_end_is_still_legal() {
    let declared = declared_loop_route(RouteTermination::End);
    let route = bind_route(&declared.resolve().expect("the end route resolves"), &[])
        .expect("a declared ending route binds");
    let mut fixture = fixture();
    let actor = actor(ACTOR_SERIAL);

    spawn_ai_aircraft(
        &mut fixture,
        actor,
        route.clone(),
        route.graph().node_count(),
    );
    fixture.step(2);

    assert!(
        navigation(&fixture).is_registered(actor),
        "an ending route's finished state is a legal placement: {:?}",
        report(&fixture).last_refusal
    );
    assert!(
        navigation(&fixture).is_complete(actor, route.graph()),
        "and it really is complete, holding station"
    );
}
