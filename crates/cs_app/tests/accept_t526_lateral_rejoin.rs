//! Task #526: a **laterally** displaced actor rejoins its next mandatory
//! marker through the production Avian fixed-tick loop.
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, stage
//! `### F31-C`, AC03; shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//! Task test prefix: `accept_t526_`.
//!
//! This is the F31-C acceptance case the #451 finding could only record as
//! unachieved: the vertical displacement of `accept_t446_...` closes because
//! the pitch channel answers a height error, but the lateral channel was an
//! undamped double integrator — heading error straight onto a roll *rate*
//! command, on an airframe with no bank holding — so the commanded bank ran
//! away and the offset grew instead of closing. Task #526 gives the follower
//! measured bank (`NavState::bank_rad`, sampled from the live Avian
//! `Rotation`) and an inner bank-hold loop; this test flies the result.
//!
//! Everything here drives production code: the real [`PhysicsFixture`] world
//! with the production [`FlightForcesPlugin`] and [`AiNavigationPlugin`], the
//! `cs_content` declared route projected through [`cs_app::ai::bind_route`],
//! and the production [`spawn_flight_body`] path. Every value is newly
//! authored synthetic fixture data — no original game data, and no test reads
//! `CS_GAME_DIR`.

use avian3d::prelude::{Position, Rotation};
use bevy::prelude::{Entity, Transform, Vec3};

use cs_app::ai::{
    AiNavigation, AiNavigationPlugin, AnchorBinding, BoundRoute, MovingAnchor,
    NavigationTickReport, RoutePursuit, SYNTHETIC_MOVING_ANCHOR_RUNTIME_ID,
    SYNTHETIC_NAVIGATION_SEED, SYNTHETIC_NAVIGATION_SESSION, bind_route,
    declared_synthetic_moving_route, synthetic_moving_anchor_id,
};
use cs_app::physics::{
    FixtureBodySpec, FlightForcesPlugin, FlightSpawnSpec, PhysicsFixture, spawn_flight_body,
};
use cs_content::routes::AnchorKind;
use cs_sim::ai::navigation::synthetic_maneuver_envelope;
use cs_sim::damage::ActorId;
use cs_sim::flight::{EngineState, FlightInput, FlightModel, synthetic_fixed_wing};
use cs_types::net::SessionId;

/// The serial of the fixture AI actor.
const ACTOR_SERIAL: u64 = 7;

/// The first mandatory marker of the declared synthetic route, in world meters
/// for an anchor at the origin, and its authored arrival radius.
const MARKER_M: [f64; 3] = [0.0, 0.0, -120.0];
const MARKER_RADIUS_M: f64 = 10.0;

/// How far off the route's first leg the actor starts, in meters: twice the
/// marker's arrival radius, so flying straight on cannot arrive by accident.
const DISPLACEMENT_M: f32 = 20.0;

/// The fixed-tick budget: 50 simulated seconds at the declared 120 Hz.
const BUDGET_TICKS: u64 = 6_000;

/// The one tick of travel the swept arrival test may cover, at cruise: the
/// arrival is a segment test, so the aircraft can be one step outside the
/// sphere on the tick that fires it.
const ONE_TICK_M: f64 = 40.0 / 120.0;

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

/// The fixture actor, in the fixture session.
fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session(SYNTHETIC_NAVIGATION_SESSION),
        serial,
    }
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

/// The declared synthetic route projected into the runtime graph and bound to
/// the fixture anchor's runtime id.
fn bound_route() -> BoundRoute {
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

/// Spawns the live moving anchor entity at `position_m` (identity rotation).
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

/// Spawns one AI aircraft through the production flight path and gives it its
/// pursuit record, resumed past the (non-mandatory) spawn node.
fn spawn_ai_aircraft(
    fixture: &mut PhysicsFixture,
    actor: ActorId,
    route: BoundRoute,
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
        .insert(RoutePursuit::new(actor, route, 1));
    aircraft
}

/// Reads how many leading nodes the actor has reached.
fn reached(fixture: &PhysicsFixture, actor: ActorId) -> Option<usize> {
    fixture.world().resource::<AiNavigation>().reached(actor)
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

/// The world-Y component of the body-up axis: the cosine of the bank angle, so
/// a value at or below zero means the airframe has rolled past 90 degrees —
/// the #451 defect a runaway roll command produced.
fn body_up_y(fixture: &PhysicsFixture, entity: Entity) -> f32 {
    (fixture
        .world()
        .get::<Rotation>(entity)
        .expect("a flight body has a rotation")
        .0
        * Vec3::Y)
        .y
}

fn distance_m(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// F31-C AC03 through the integrated loop: an actor displaced **laterally**
/// from its route rejoins the next mandatory marker inside the marker's own
/// authored arrival radius, without skipping it and without a runaway bank.
///
/// The displacement (`DISPLACEMENT_M`, twice the marker radius) is in `X`,
/// across the route's first leg, and the actor starts pointed straight down
/// that leg: it must bank, hold that bank and roll back out again to arrive.
/// The pre-#526 follower fails this (its commanded bank grew without bound and
/// the offset grew with it), and so does a follower whose bank feedback is
/// removed or sign-flipped — see the mutation notes in
/// `docs/findings/2026-10-07-t526-follower-bank-hold.md`.
#[test]
fn accept_t526_laterally_displaced_actor_rejoins_before_the_next_mandatory_marker() {
    let mut fixture = fixture();
    let route = bound_route();
    let _anchor = spawn_anchor(&mut fixture, [0.0, 0.0, 0.0]);
    let plane = spawn_ai_aircraft(
        &mut fixture,
        actor(ACTOR_SERIAL),
        route,
        [DISPLACEMENT_M, 0.0, 0.0],
        [0.0, 0.0, -40.0],
    );

    // The route's first leg runs down `X = 0`; this actor starts `DISPLACEMENT_M`
    // across it, so a straight run passes `DISPLACEMENT_M` from the marker and
    // can never arrive inside `MARKER_RADIUS_M`.
    assert!(f64::from(DISPLACEMENT_M) > MARKER_RADIUS_M);

    let mut previous = 1usize;
    let mut arrival: Option<([f32; 3], u64)> = None;
    let mut min_up_y = 1.0f32;
    for tick in 0..BUDGET_TICKS {
        fixture.step(1);
        min_up_y = min_up_y.min(body_up_y(&fixture, plane));
        let current = reached(&fixture, actor(ACTOR_SERIAL)).expect("registered on tick 0");
        assert!(
            current >= previous,
            "progress is monotonic across the fixed tick: {previous} -> {current}"
        );
        previous = current;
        if current >= 2 && arrival.is_none() {
            arrival = Some((position_of(&fixture, plane), tick + 1));
        }
        if current >= 2 {
            break;
        }
    }

    let (at, tick) = arrival.expect(
        "the laterally displaced actor must reach its next mandatory marker inside the \
         integrated tick budget",
    );
    let to_marker = distance_m(
        [f64::from(at[0]), f64::from(at[1]), f64::from(at[2])],
        MARKER_M,
    );
    assert!(
        to_marker <= MARKER_RADIUS_M + ONE_TICK_M,
        "the actor rejoined inside the marker's own authored radius ({MARKER_RADIUS_M} m): \
         {to_marker} m at tick {tick}, position {at:?}"
    );
    // The lateral channel is the point of this test: the `X` offset must be
    // closed, not bought back with altitude.
    assert!(
        f64::from(at[0]).abs() <= MARKER_RADIUS_M,
        "the lateral offset must be closed, not traded for altitude: x = {} at {at:?}",
        at[0]
    );

    let tick_report = report(&fixture);
    assert!(
        tick_report.ticks > 0 && tick_report.applied > 0,
        "the follower's command reached the flight record: {tick_report:?}"
    );
    assert!(
        tick_report.last_refusal.is_none(),
        "a clean rejoin records no refusal: {:?}",
        tick_report.last_refusal
    );

    // The bank-hold loop keeps the airframe inside its declared envelope: a
    // follower without measured bank rolls past 90 degrees (the #451 held-roll
    // measurement) long before it arrives.
    assert!(
        min_up_y > 0.0,
        "the follower must never roll the airframe past 90 degrees of bank, lowest up.Y was \
         {min_up_y}"
    );
    let max_bank = synthetic_maneuver_envelope().max_bank_rad;
    assert!(
        f64::from(min_up_y).acos() < max_bank + std::f64::consts::FRAC_PI_4,
        "the bank stays near the declared envelope: lowest up.Y {min_up_y} is past {} degrees",
        (max_bank + std::f64::consts::FRAC_PI_4).to_degrees()
    );
}
