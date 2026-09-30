//! The contact reporter's refusal paths (F23-B): an event it must not guess
//! and an event the declared matrix forbids.
//!
//! Spec: `specs/F23-avian-integration-collision-and-fixed-step-authority.md`,
//! stage `### F23-B`, non-negotiable behavior 2 (a sensor overlap is never
//! damage by itself) and the reporter's contract in
//! `crates/cs_app/src/physics/contacts.rs`: an unbound body is counted, never
//! guessed, and a pair the matrix forbids is counted, never reported.
//!
//! Every value is authored fixture data: no original data and no `CS_GAME_DIR`
//! access.

use avian3d::prelude::CollisionLayers as AvianCollisionLayers;
use bevy::prelude::Entity;
use cs_app::physics::{BodyLayer, BodyMode, BodySpec, ContactReports, PhysicsFixture};
use cs_sim::collision::{CollisionLayer, CollisionLayers, ShapeClass};

use crate::common;

/// Half the obstacle thickness: a 2 cm trigger.
const HALF_THICKNESS_M: f32 = 0.01;

/// 120 m/s at 120 Hz is 1 m per tick.
const CROSSING_SPEED_M_S: f32 = 120.0;

/// Off the tick grid, so the body never lands exactly on the trigger.
const START_X_M: f32 = -9.37;

/// Long enough for the crossing and for what happens afterwards.
const TICKS: u64 = 40;

/// A thin static trigger at x = 0 and a fast projectile crossing it.
fn crossing(fixture: &mut PhysicsFixture) -> (Entity, Entity) {
    let trigger = common::spawn(
        fixture,
        &BodySpec {
            layer: CollisionLayer::Trigger,
            shape: ShapeClass::Sensor,
            mode: BodyMode::Static,
            mass_kg: 1.0,
            half_extents_m: [HALF_THICKNESS_M, 2.0, 2.0],
            position_m: [0.0, 0.0, 0.0],
            linear_velocity_m_s: [0.0, 0.0, 0.0],
        },
    );
    let projectile = common::spawn(
        fixture,
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
    (trigger, projectile)
}

/// An event from a body that never went through the production creation path
/// is counted as unclassified instead of being guessed from the engine's own
/// flags.
///
/// Observable failure if the reporter guesses: the trigger would be classified
/// by whatever component it happens to carry, `unclassified` would stay 0 and
/// a body the game never declared would still produce a report.
#[test]
fn accept_f23_b_an_event_from_an_unbound_body_is_counted_unclassified() {
    let mut fixture = common::empty_fixture();
    let (trigger, _projectile) = crossing(&mut fixture);

    // The same trigger, but with its declared layer taken away: this is a body
    // spawned outside `spawn_body`, which carries no `BodyLayer` to key off.
    fixture
        .world_mut()
        .entity_mut(trigger)
        .remove::<BodyLayer>();

    for _ in 0..TICKS {
        fixture.step(1);
    }

    let reports = fixture.world().resource::<ContactReports>();
    assert_eq!(
        reports.total(),
        0,
        "an event with no declared layer must not be reported as anything"
    );
    assert_eq!(
        reports.unclassified(),
        1,
        "the crossing must be counted, not guessed: {reports:?}"
    );
    assert_eq!(reports.ignored(), 0, "{reports:?}");
    assert_eq!(reports.suppressed(), 0, "{reports:?}");
}

/// A pair the declared matrix forbids is counted as ignored, never reported —
/// and the counter is what a collision-group binding mistake shows up as.
///
/// Observable failure if the binding is restated wider than the declared
/// matrix (`Trigger` does not interact with `Trigger`, so `spawn_body` filters
/// it out): the narrow phase generates the pair, the event arrives, and
/// without the counter it would be reported as a real overlap. The test
/// recreates exactly that mis-binding by widening both filters after spawn,
/// because the production path refuses to make it.
#[test]
fn accept_f23_b_a_pair_the_declared_matrix_forbids_is_counted_ignored() {
    let mut fixture = common::empty_fixture();
    let spec = |position_m: [f32; 3]| BodySpec {
        layer: CollisionLayer::Trigger,
        shape: ShapeClass::Sensor,
        mode: BodyMode::Static,
        mass_kg: 1.0,
        half_extents_m: [0.5, 0.5, 0.5],
        position_m,
        linear_velocity_m_s: [0.0, 0.0, 0.0],
    };
    let first = common::spawn(&mut fixture, &spec([0.0; 3]));
    let second = common::spawn(&mut fixture, &spec([0.25, 0.0, 0.0]));

    // Deliberate mis-binding: filters widened past `designed_partners`, so the
    // engine pairs two triggers the declared matrix keeps apart.
    let widened = AvianCollisionLayers::from_bits(
        u32::from(CollisionLayer::Trigger.bit()),
        u32::from(CollisionLayers::ALL.bits()),
    );
    for entity in [first, second] {
        fixture.world_mut().entity_mut(entity).insert(widened);
    }

    for _ in 0..2 {
        fixture.step(1);
    }

    let reports = fixture.world().resource::<ContactReports>();
    assert_eq!(
        reports.total(),
        0,
        "a forbidden pair must never become a report: {reports:?}"
    );
    assert_eq!(
        reports.ignored(),
        1,
        "the mis-binding must surface as the ignored counter: {reports:?}"
    );
    assert_eq!(reports.unclassified(), 0, "{reports:?}");
    assert_eq!(reports.suppressed(), 0, "{reports:?}");
}
