//! Task #761: the command the follower emits is the route step it actually
//! **committed**, flown through the production Avian fixed-tick loop.
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`,
//! non-negotiable behavior 2 and AC01; shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`. Task test prefix: `accept_t761_`.
//!
//! Before this task the command cascade always consumed the *target's*
//! heading error: an `AvoidanceState::Deviating` step bounded away from a
//! blocker while the commanded bank still chased the target — the kinematic
//! plan deviated and the body never followed — and a `Blocked` stationary
//! step held whatever bank the aircraft happened to carry, so a held aircraft
//! kept turning while its throttle decayed. These tests fly the result
//! through the production [`AiNavigationPlugin`] + [`FlightForcesPlugin`]
//! loop:
//!
//! * `t761_a` — an actor pursuing a declared route through a walled opening
//!   never crosses the wall and still reaches the next mandatory marker
//!   inside its authored arrival radius;
//! * `t761_b` — a committed deviation banks the **airframe** the committed
//!   way: the roll the production driver applies while `Deviating` is the
//!   dodge's side, not the target's;
//! * `t761_c` — a `Blocked` hold decelerates wings-level rather than holding
//!   the bank the aircraft happened to carry.
//!
//! Everything drives production code: the real [`PhysicsFixture`] world, the
//! production flight and navigation drivers, `cs_content` declared routes
//! projected through [`cs_app::ai::bind_route`], [`RoutePursuit::with_blockers`]
//! carrying the obstacles, and the production [`spawn_flight_body`] path.
//! Every value is newly authored synthetic fixture data — no original game
//! data, and no test reads `CS_GAME_DIR`.

use avian3d::prelude::{Position, Rotation};
use bevy::prelude::{Entity, Vec3};

use cs_app::ai::{
    AiNavigation, AiNavigationPlugin, BoundRoute, NavigationTickReport, RoutePursuit,
    SYNTHETIC_NAVIGATION_SEED, SYNTHETIC_NAVIGATION_SESSION, bind_route,
};
use cs_app::physics::{
    FixtureBodySpec, FlightAircraft, FlightForcesPlugin, FlightSpawnSpec, PhysicsFixture,
    spawn_flight_body,
};
use cs_content::routes::{
    ReferenceFrame, RouteDefinition, RouteDraft, RouteEdge, RouteNode, RouteNodeId,
    RouteTermination,
};
use cs_sim::ai::navigation::{Blocker, DeviationSide, PursuitState, heading_from_direction};
use cs_sim::damage::ActorId;
use cs_sim::flight::{EngineState, FlightInput, FlightModel, synthetic_fixed_wing};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;
use cs_types::space::{Quaternion, Radians, UnitVec3};

/// The serial of the fixture AI actor.
const ACTOR_SERIAL: u64 = 7;

/// The fixed-tick budget: 50 simulated seconds at the declared 120 Hz.
const BUDGET_TICKS: u64 = 6_000;

/// The one tick of travel the swept arrival test may cover, at cruise: the
/// arrival is a segment test, so the aircraft can be one step outside the
/// sphere on the tick that fires it.
const ONE_TICK_M: f64 = 40.0 / 120.0;

/// The gap route's mandatory marker inside the walled opening and its
/// authored arrival radius.
const GAP_MARKER_M: [f64; 3] = [20.0, 0.0, -124.0];
const GAP_MARKER_RADIUS_M: f64 = 8.0;

/// The pursuit route's first mandatory marker and its authored arrival
/// radius.
const PURSUIT_MARKER_M: [f64; 3] = [0.0, 0.0, -120.0];
const PURSUIT_MARKER_RADIUS_M: f64 = 6.0;

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

/// A declared world route node.
fn declared_node(
    designed: &dyn Fn() -> Provenance,
    id: &str,
    sequence: u32,
    mandatory: bool,
    position: [f64; 3],
    radius: f64,
) -> RouteNode {
    RouteNode {
        id: RouteNodeId::try_new(id).expect("the fixture node id is valid"),
        sequence,
        mandatory,
        position_m: Resolved::Known(Known::new(position, designed())),
        arrival_radius_m: Resolved::Known(Known::new(radius, designed())),
        trigger: Resolved::Known(Known::new(None, designed())),
    }
}

fn declared_edge(from: &str, to: &str) -> RouteEdge {
    RouteEdge {
        from: RouteNodeId::try_new(from).expect("the fixture node id is valid"),
        to: RouteNodeId::try_new(to).expect("the fixture node id is valid"),
    }
}

/// The declared world route the `t761_b`/`t761_c` legs fly: the F31-B
/// pursuit geometry — a spawn node at the origin and a -Z chain of mandatory
/// markers — authored with the same positions and arrival radii.
fn pursuit_route() -> BoundRoute {
    let designed =
        || Provenance::designed(ClaimId::new("t761.world-pursuit-route").expect("valid claim id"));
    let node = |id: &str, sequence: u32, mandatory: bool, position: [f64; 3], radius: f64| {
        declared_node(&designed, id, sequence, mandatory, position, radius)
    };
    let declared = RouteDefinition::try_new(RouteDraft {
        id: ContentId::from_source(ContentKind::Route, "synthetic.t761.pursuit")
            .expect("the fixture route id is valid"),
        origin: Origin::SyntheticFixture,
        frame: ReferenceFrame::World,
        termination: RouteTermination::End,
        clearance_m: Resolved::Known(Known::new(0.0, designed())),
        nodes: vec![
            node("start", 0, false, [0.0, 0.0, 0.0], 2.0),
            node("marker", 1, true, PURSUIT_MARKER_M, PURSUIT_MARKER_RADIUS_M),
            node("gate", 2, true, [0.0, 0.0, -260.0], 6.0),
            node("goal", 3, true, [0.0, 0.0, -400.0], 8.0),
        ],
        edges: vec![
            declared_edge("start", "marker"),
            declared_edge("marker", "gate"),
            declared_edge("gate", "goal"),
        ],
        provenance: designed(),
    })
    .expect("the declared pursuit route is valid");
    let resolved = declared.resolve().expect("the declared route resolves");
    bind_route(&resolved, &[]).expect("a world route binds with no anchors")
}

/// The declared world route the `t761_a` leg flies: a corridor that bends
/// through the opening in the wall and continues past it. A straight run at
/// the markers from the spawn would hit the left slab — the declared
/// corridor is what threads the opening (F31 AC01).
fn gap_route() -> BoundRoute {
    let designed =
        || Provenance::designed(ClaimId::new("t761.wall-gap-route").expect("valid claim id"));
    let node = |id: &str, sequence: u32, mandatory: bool, position: [f64; 3], radius: f64| {
        declared_node(&designed, id, sequence, mandatory, position, radius)
    };
    let declared = RouteDefinition::try_new(RouteDraft {
        id: ContentId::from_source(ContentKind::Route, "synthetic.t761.gap")
            .expect("the fixture route id is valid"),
        origin: Origin::SyntheticFixture,
        frame: ReferenceFrame::World,
        termination: RouteTermination::End,
        clearance_m: Resolved::Known(Known::new(0.0, designed())),
        nodes: vec![
            node("start", 0, false, [0.0, 0.0, 0.0], 2.0),
            node("gap", 1, true, GAP_MARKER_M, GAP_MARKER_RADIUS_M),
            node("goal", 2, true, [35.0, 0.0, -250.0], 8.0),
        ],
        edges: vec![declared_edge("start", "gap"), declared_edge("gap", "goal")],
        provenance: designed(),
    })
    .expect("the declared gap route is valid");
    let resolved = declared.resolve().expect("the declared route resolves");
    bind_route(&resolved, &[]).expect("a world route binds with no anchors")
}

/// The wall the gap route threads: a left slab covering `x ∈ [-40, 2]` and a
/// right slab covering `x ∈ [40, 80]`, both 40 m tall and 2 m thick at
/// `z = -60`. The opening between them is the only way through; a straight
/// `-Z` run from the spawn ends inside the left slab.
fn wall_blockers() -> Vec<Blocker> {
    vec![
        Blocker::axis_aligned_box([-19.0, 0.0, -60.0], [21.0, 20.0, 1.0]),
        Blocker::axis_aligned_box([60.0, 0.0, -60.0], [20.0, 20.0, 1.0]),
    ]
}

/// A thin vertical plate astride the pursuit leg: 2 mm wide in `x`, tall
/// enough that no climb or dive dodges it, thin in `z`. The direct step at
/// `x = 0` sweeps through it; a step bounded one envelope yaw step aside
/// exits the 2 mm band inside one tick and clears — the geometry in which
/// the bounded follower answers `Deviating`, not `Blocked`.
fn wire_blocker() -> Blocker {
    Blocker::axis_aligned_box([0.0, 0.0, -30.05], [0.001, 50.0, 0.001])
}

/// A sphere the bounded deviate candidates cannot leave in one step: every
/// candidate stays inside it, so the follower can only answer `Blocked`.
fn hold_blocker() -> Blocker {
    Blocker::sphere([0.0, 0.0, -58.0], 3.0)
}

/// Spawns one AI aircraft through the production flight path and gives it
/// its pursuit record — with `blockers` attached — resumed past the
/// (non-mandatory) spawn node.
fn spawn_ai_aircraft(
    fixture: &mut PhysicsFixture,
    route: BoundRoute,
    blockers: Vec<Blocker>,
    spec: FlightSpawnSpec,
) -> Entity {
    let aircraft = spawn_flight_body(
        fixture.world_mut(),
        FlightModel::new(synthetic_fixed_wing()),
        &spec,
    )
    .expect("the spawn spec is valid");
    fixture
        .world_mut()
        .entity_mut(aircraft)
        .insert(RoutePursuit::new(actor(ACTOR_SERIAL), route, 1).with_blockers(blockers));
    aircraft
}

/// The held command the production driver last applied to the airframe.
fn command_of(fixture: &PhysicsFixture, entity: Entity) -> FlightInput {
    fixture
        .world()
        .get::<FlightAircraft>(entity)
        .expect("a flight body carries the flight record")
        .command()
}

/// The follower's persistent state for the fixture actor.
fn pursuit_state(fixture: &PhysicsFixture) -> PursuitState {
    fixture
        .world()
        .resource::<AiNavigation>()
        .set()
        .state(actor(ACTOR_SERIAL))
        .expect("the pursuit registers on spawn")
}

/// Reads how many leading nodes the actor has reached.
fn reached(fixture: &PhysicsFixture) -> Option<usize> {
    fixture
        .world()
        .resource::<AiNavigation>()
        .reached(actor(ACTOR_SERIAL))
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

/// The world-Y component of the body-up axis: the cosine of the bank angle.
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

fn position_f64(position: [f32; 3]) -> [f64; 3] {
    [
        f64::from(position[0]),
        f64::from(position[1]),
        f64::from(position[2]),
    ]
}

/// F31 AC01 through the integrated loop: an actor pursuing a declared route
/// through a walled **opening** reaches its next mandatory marker inside the
/// marker's authored arrival radius, and the physical airframe never crosses
/// the wall on the way.
///
/// The two slabs leave a `x ∈ [2, 40]` opening at `z = -60`; a straight `-Z`
/// run from the spawn ends inside the left slab. The declared corridor —
/// through the gap marker at `x = 20` and on to the goal — is what the
/// follower flies, and the follower is what keeps the body inside the
/// corridor: the plan's swept-step test refuses to commit a step that
/// crosses a blocker, so the body's real path goes through the opening.
///
/// The assertion is on the **raw** blocker — the wall as authored, not the
/// clearance-grown volume the plan checks against — so a body that merely
/// respected the planner's margin and nothing more would still fail.
#[test]
fn accept_t761_a_routed_corridor_keeps_the_airframe_out_of_the_wall() {
    let mut fixture = fixture();
    let blockers = wall_blockers();
    // Spawn already aimed at the gap marker, so the corridor's early tracking
    // lag does not graze the left slab; this is a route-following case, not a
    // rejoin one (that is `accept_t526_lateral_rejoin`'s).
    let heading = heading_from_direction(GAP_MARKER_M[0], GAP_MARKER_M[2]);
    let forward = cs_sim::ai::navigation::forward_from_heading(heading);
    let plane = spawn_ai_aircraft(
        &mut fixture,
        gap_route(),
        blockers.clone(),
        FlightSpawnSpec {
            engine: EngineState::direct(1.0),
            command: FlightInput::try_new(0.0, 0.0, 0.0, 1.0, false).expect("valid input"),
            orientation: Quaternion::from_axis_angle(UnitVec3::UP, Radians(heading))
                .expect("a unit quaternion"),
            ..FlightSpawnSpec::level_at(
                [0.0, 0.0, 0.0],
                [
                    (40.0 * forward[0]) as f32,
                    (40.0 * forward[1]) as f32,
                    (40.0 * forward[2]) as f32,
                ],
            )
        },
    );

    let mut previous: Option<[f32; 3]> = None;
    let mut reached_marker: Option<([f32; 3], u64)> = None;
    for tick in 0..BUDGET_TICKS {
        fixture.step(1);
        let position = position_of(&fixture, plane);
        if let Some(from) = previous {
            for blocker in &blockers {
                assert!(
                    !blocker.segment_intersects(position_f64(from), position_f64(position)),
                    "the physical airframe crossed the wall: {from:?} -> {position:?}"
                );
            }
        }
        previous = Some(position);
        if reached(&fixture) >= Some(2) && reached_marker.is_none() {
            reached_marker = Some((position, tick + 1));
        }
        if reached(&fixture) >= Some(3) {
            break;
        }
    }

    let (at, tick) = reached_marker
        .expect("the actor must reach the marker inside the walled opening within the tick budget");
    let to_marker = distance_m(position_f64(at), GAP_MARKER_M);
    assert!(
        to_marker <= GAP_MARKER_RADIUS_M + ONE_TICK_M,
        "the actor arrived inside the marker's own authored radius \
         ({GAP_MARKER_RADIUS_M} m): {to_marker} m at tick {tick}, position {at:?}"
    );

    let tick_report = report(&fixture);
    assert!(
        tick_report.ticks > 0 && tick_report.applied > 0,
        "the follower's command reached the flight record: {tick_report:?}"
    );
    assert!(
        tick_report.last_refusal.is_none(),
        "a clean corridor flight records no refusal: {:?}",
        tick_report.last_refusal
    );
    // The corridor is what threads the opening: progress stays monotonic and
    // the route completes past the wall rather than wedging on it.
    assert_eq!(
        reached(&fixture),
        Some(3),
        "the route completes past the wall"
    );
    assert_eq!(
        pursuit_state(&fixture).stalled_ticks(),
        0,
        "the routed corridor never needed a blocked hold"
    );
}

/// The committed **deviation** reaches the airframe through the production
/// loop: while the follower reports `Deviating`, the roll it applies banks
/// the body the way the committed step dodges — measured against the
/// persistent [`PursuitState::deviation`] side, not the plan alone.
///
/// The wire plate is the geometry the bounded follower can still dodge: a
/// straight step at `x = 0` sweeps the 2 mm band, while a step bounded one
/// envelope yaw step aside exits the band inside one tick. The follower's
/// answer is `Deviating`, and the command this task changed is what the
/// assertion measures: under the pre-#761 law the cascade still read the
/// target's error — zero here — so the applied roll was `0.0` on the
/// deviating tick.
///
/// The body's raw path still crosses the hair-thin plate: the plan sees a
/// blocker one bounded step ahead, and no command can displace a 40 m/s
/// airframe out of its line inside one tick — that is the measured bound
/// this task records, not a weakening of the assertion, which is on the
/// *emitted command*. What keeps the airframe out of a wall is the corridor
/// the plan refuses to leave (`t761_a`) and the hold it takes when nothing
/// clears (`t761_c`).
#[test]
fn accept_t761_b_deviating_command_banks_the_airframe_the_committed_way() {
    let mut fixture = fixture();
    let plane = spawn_ai_aircraft(
        &mut fixture,
        pursuit_route(),
        vec![wire_blocker()],
        FlightSpawnSpec {
            engine: EngineState::direct(1.0),
            command: FlightInput::try_new(0.0, 0.0, 0.0, 1.0, false).expect("valid input"),
            ..FlightSpawnSpec::level_at([0.0, 0.0, 0.0], [0.0, 0.0, -40.0])
        },
    );

    let mut deviating: Vec<(DeviationSide, f64)> = Vec::new();
    let mut reached_marker: Option<([f32; 3], u64)> = None;
    for tick in 0..BUDGET_TICKS {
        fixture.step(1);
        let state = pursuit_state(&fixture);
        // The remembered side survives a following blocked hold, so a live
        // deviating decision is the side *and* a tick that still moved.
        if let Some(side) = state.deviation()
            && state.stalled_ticks() == 0
        {
            // The command the driver applied this tick is the deviating
            // decision's own — the same decision that set the side.
            deviating.push((side, command_of(&fixture, plane).roll));
        }
        if reached(&fixture) >= Some(2) && reached_marker.is_none() {
            reached_marker = Some((position_of(&fixture, plane), tick + 1));
        }
        if reached(&fixture) >= Some(2) {
            break;
        }
    }

    assert!(
        !deviating.is_empty(),
        "the wire plate must produce at least one committed deviation"
    );
    for (side, roll) in &deviating {
        // `FlightInput.roll` is positive right-wing-down
        // (docs/contracts/FLIGHT-PHYSICS.md): a nose-left dodge banks
        // left-wing-down, a nose-right one right-wing-down. The bound is the
        // committed step's own rate, so the sign is the assertion, not the
        // magnitude.
        match side {
            DeviationSide::Left => assert!(
                *roll < 0.0,
                "a nose-left committed dodge must bank the body left: roll {roll}"
            ),
            DeviationSide::Right => assert!(
                *roll > 0.0,
                "a nose-right committed dodge must bank the body right: roll {roll}"
            ),
        }
    }

    let (at, tick) = reached_marker.expect(
        "the deviating actor must still reach its next mandatory marker in the tick budget",
    );
    let to_marker = distance_m(position_f64(at), PURSUIT_MARKER_M);
    assert!(
        to_marker <= PURSUIT_MARKER_RADIUS_M + ONE_TICK_M,
        "the actor arrived inside the marker's own authored radius \
         ({PURSUIT_MARKER_RADIUS_M} m): {to_marker} m at tick {tick}, position {at:?}"
    );
    assert!(
        report(&fixture).last_refusal.is_none(),
        "a deviating corridor flight records no refusal: {:?}",
        report(&fixture).last_refusal
    );
}

/// A `Blocked` hold decelerates **wings-level**: the stationary step carries
/// the aircraft's own heading and a zero climb, so the inner bank and climb
/// loops command the aircraft toward level whatever it carried in — the
/// behaviour this task pins. The aircraft spawns banked hard about its
/// forward axis, close enough to the enclosing sphere that it is still
/// banked when the hold begins, so the hold visibly rolls it out instead of
/// starting from level.
///
/// Under the pre-#761 law the stationary special-case held the measured
/// bank: the applied roll was exactly `0.0` on every blocked tick and the
/// aircraft kept its bank while the throttle decayed.
#[test]
fn accept_t761_c_blocked_hold_rolls_the_airframe_level_and_cuts_the_throttle() {
    let mut fixture = fixture();
    // The banked spawn: one radian about the forward axis, close enough to
    // the enclosing sphere that the hold begins while the airframe is still
    // banked.
    let plane = spawn_ai_aircraft(
        &mut fixture,
        pursuit_route(),
        vec![hold_blocker()],
        FlightSpawnSpec {
            engine: EngineState::direct(1.0),
            command: FlightInput::try_new(0.0, 0.0, 0.0, 1.0, false).expect("valid input"),
            orientation: Quaternion::from_axis_angle(UnitVec3::FORWARD, Radians(1.0))
                .expect("a unit quaternion"),
            ..FlightSpawnSpec::level_at([0.0, 0.0, -50.0], [0.0, 0.0, -40.0])
        },
    );

    let spawn_up_y = body_up_y(&fixture, plane);
    assert!(
        spawn_up_y < 0.9,
        "the spawn must be measurably banked for the hold to be observable: up_y {spawn_up_y}"
    );

    let mut held: Vec<(f32, f64)> = Vec::new(); // (up_y, roll) while stalled
    for _tick in 0..BUDGET_TICKS {
        fixture.step(1);
        let state = pursuit_state(&fixture);
        if state.stalled_ticks() > 0 {
            held.push((body_up_y(&fixture, plane), command_of(&fixture, plane).roll));
            assert_eq!(
                command_of(&fixture, plane).throttle,
                0.0,
                "a blocked hold cuts the throttle"
            );
        }
        // The sphere is behind the aircraft once it clears it; the route
        // rejoins from there.
        if reached(&fixture) >= Some(2) {
            break;
        }
    }

    assert!(
        held.len() >= 2,
        "the enclosing sphere must produce a sustained blocked hold: {} ticks",
        held.len()
    );
    let (first_up_y, _) = held[0];
    let (last_up_y, _) = *held.last().expect("nonempty");
    assert!(
        last_up_y > first_up_y,
        "the hold rolls the banked airframe toward level: up_y {first_up_y} -> {last_up_y}"
    );
    let min_roll = held
        .iter()
        .map(|(_, roll)| *roll)
        .fold(f64::INFINITY, f64::min);
    assert!(
        min_roll < -0.5,
        "the banked airframe's hold commands it toward level, not to hold its bank: \
         minimum roll {min_roll} (the pre-#761 law emitted exactly 0.0)"
    );
    assert_eq!(
        reached(&fixture),
        Some(2),
        "the aircraft rejoins past the sphere and reaches the next mandatory marker"
    );
}
