//! AC02: a high-speed crossing of a thin wall or trigger is detected exactly
//! once (F23 non-negotiable behavior 3, contract "Collision and ballistic
//! tests").
//!
//! The projectile moves 1 m per 1/120 s tick while the obstacle is 2 cm thin,
//! so `speed * dt` is fifty times the obstacle thickness: an endpoint-only
//! implementation can neither overlap the obstacle nor predict it and would
//! report nothing at all. Detection therefore has to come from the sweep the
//! production body-creation path binds onto the fast layer.
//!
//! Every value is authored fixture data.

use avian3d::prelude::Position;
use bevy::prelude::Entity;
use cs_app::physics::{BodyMode, BodySpec, ContactReport, ContactReports, PhysicsFixture};
use cs_sim::collision::{CollisionLayer, ContactKind, ShapeClass};

use crate::common;

/// Half the obstacle thickness: a 2 cm wall.
const HALF_THICKNESS_M: f32 = 0.01;

/// 120 m/s at 120 Hz is 1 m per tick, fifty wall thicknesses per tick.
const CROSSING_SPEED_M_S: f32 = 120.0;

/// Off the tick grid, so the body never lands exactly on the obstacle.
const START_X_M: f32 = -9.37;

/// Long enough for the crossing and for what happens afterwards.
const TICKS: u64 = 40;

/// A thin static obstacle at x = 0 and one fast projectile approaching it,
/// both created through the production body-creation path.
fn crossing(layer: CollisionLayer, shape: ShapeClass) -> (PhysicsFixture, Entity, Entity) {
    let mut fixture = common::empty_fixture();
    let wall = common::spawn(
        &mut fixture,
        &BodySpec {
            layer,
            shape,
            mode: BodyMode::Static,
            mass_kg: 1.0,
            half_extents_m: [HALF_THICKNESS_M, 2.0, 2.0],
            position_m: [0.0, 0.0, 0.0],
            linear_velocity_m_s: [0.0, 0.0, 0.0],
        },
    );
    let projectile = common::spawn(
        &mut fixture,
        &BodySpec {
            layer: CollisionLayer::Projectile,
            shape: ShapeClass::Solid,
            mode: BodyMode::Dynamic,
            mass_kg: 1.0,
            half_extents_m: [0.05, 0.05, 0.05],
            position_m: [START_X_M, 0.0, 0.0],
            linear_velocity_m_s: [CROSSING_SPEED_M_S, 0.0, 0.0],
        },
    );
    (fixture, wall, projectile)
}

/// What one crossing produced.
#[derive(Debug)]
struct Outcome {
    /// Farthest x the projectile reached.
    max_x: f32,
    /// Where the projectile ended up.
    last_x: f32,
    /// The contact report the crossing produced, if any.
    report: Option<ContactReport>,
    /// Reports recorded across the whole run.
    total: u64,
    /// Duplicate starts of an already-active pair.
    suppressed: u64,
    /// Events with no declared layer.
    unclassified: u64,
    /// Events the declared matrix forbids.
    ignored: u64,
}

/// Steps the crossing to completion and reads the production reporter.
fn run(fixture: &mut PhysicsFixture, projectile: Entity) -> Outcome {
    let mut max_x = f32::MIN;
    let mut last_x = 0.0;
    let mut report = None;
    for _ in 0..TICKS {
        fixture.step(1);
        last_x = fixture
            .world()
            .entity(projectile)
            .get::<Position>()
            .expect("the projectile has a position")
            .0
            .x;
        max_x = max_x.max(last_x);
        // The reporter keeps the current tick's batch, so read it each tick.
        if let Some(found) = fixture
            .world()
            .resource::<ContactReports>()
            .reports()
            .first()
            .copied()
        {
            report = Some(found);
        }
    }
    let reports = fixture.world().resource::<ContactReports>();
    Outcome {
        max_x,
        last_x,
        report,
        total: reports.total(),
        suppressed: reports.suppressed(),
        unclassified: reports.unclassified(),
        ignored: reports.ignored(),
    }
}

/// AC02, solid half: the projectile's crossing of a thin wall is reported
/// exactly once and the sweep keeps it from tunnelling.
///
/// Observable failure if the sweep binding is removed from body creation: the
/// 2 cm wall is never found (the body's own tick movement is 1 m), `total` is
/// 0 and `max_x` shows the projectile sailing past the wall untouched.
#[test]
fn accept_f23_b_fast_crossing_of_a_thin_wall_is_detected_exactly_once() {
    let (mut fixture, wall, projectile) = crossing(CollisionLayer::StaticWorld, ShapeClass::Solid);
    let outcome = run(&mut fixture, projectile);

    assert_eq!(
        outcome.total, 1,
        "the crossing must be reported exactly once: {outcome:?}"
    );
    assert_eq!(outcome.suppressed, 0, "no duplicate report: {outcome:?}");
    assert_eq!(
        outcome.unclassified, 0,
        "both bodies are declared: {outcome:?}"
    );
    assert_eq!(
        outcome.ignored, 0,
        "projectile and static world are designed to interact: {outcome:?}"
    );

    let report = outcome.report.expect("the crossing produced a report");
    assert_eq!(
        report.kind,
        ContactKind::SolidContact,
        "a solid wall must classify as a solid contact: {report:?}"
    );
    assert!(
        report.bodies.contains(&wall),
        "the report must name the wall it hit: {report:?}"
    );
    assert!(report.involves(CollisionLayer::Projectile));
    assert!(report.involves(CollisionLayer::StaticWorld));

    assert!(
        outcome.max_x < 0.0,
        "the swept body must be stopped at the wall instead of tunnelling: {outcome:?}"
    );
}

/// AC02, sensor half: the projectile's crossing of a thin trigger is reported
/// exactly once as an overlap, and the trigger does not stop it.
///
/// Observable failure if the sensor binding is dropped: the trigger resolves
/// as a solid obstacle (`kind` becomes `SolidContact`) or, without the sweep,
/// the crossing is not reported at all.
#[test]
fn accept_f23_b_fast_crossing_of_a_thin_trigger_is_detected_exactly_once() {
    let (mut fixture, trigger, projectile) = crossing(CollisionLayer::Trigger, ShapeClass::Sensor);
    let outcome = run(&mut fixture, projectile);

    assert_eq!(
        outcome.total, 1,
        "the crossing must be reported exactly once: {outcome:?}"
    );
    assert_eq!(outcome.suppressed, 0, "no duplicate report: {outcome:?}");
    assert_eq!(
        outcome.unclassified, 0,
        "both bodies are declared: {outcome:?}"
    );
    assert_eq!(
        outcome.ignored, 0,
        "a trigger reports overlaps: {outcome:?}"
    );

    let report = outcome.report.expect("the crossing produced a report");
    assert_eq!(
        report.kind,
        ContactKind::SensorOverlap,
        "a trigger overlap must never be a solid contact: {report:?}"
    );
    assert!(report.bodies.contains(&trigger), "{report:?}");
    assert!(report.involves(CollisionLayer::Projectile));
    assert!(report.involves(CollisionLayer::Trigger));

    assert!(
        outcome.last_x > 5.0,
        "a sensor must let the projectile through: {outcome:?}"
    );
}
