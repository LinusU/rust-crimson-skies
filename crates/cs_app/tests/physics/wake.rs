//! The force queue's wake path and its drop accounting (F23-A limitation 1,
//! owned by F23-B).
//!
//! A sleeping body has no solver body, so a queued force aimed at it would be
//! parked where the sleeping step neither applies it nor clears it: it would
//! surface a tick late, scaled by the timestep a second time. The adapter
//! wakes the target before it drains the queue, so the request applies to
//! exactly the tick that submitted it.
//!
//! Every value is authored fixture data.

use avian3d::prelude::Sleeping;
use cs_app::physics::{BodyMode, BodySpec};
use cs_sim::collision::{CollisionLayer, ShapeClass};

use crate::common;

/// Small relative tolerance: one integration is separated from a missed or
/// double-scaled one by orders of magnitude, so this still fails loudly.
const REL_TOL: f32 = 1e-4;

fn approx_eq(actual: f32, expected: f32) -> bool {
    (actual - expected).abs() <= REL_TOL * expected.abs().max(1.0)
}

/// A queued force wakes a sleeping body and lands in the same tick.
///
/// Observable failure if the wake path is removed: the body stays asleep
/// during the step, so the measured delta-v is `0`, or it surfaces next tick
/// already scaled by `dt` (`F/m * dt * dt` ≈ 1e-3 of the expected value).
/// Both are far outside the tolerance, and the body is still asleep.
#[test]
fn accept_f23_b_force_wakes_a_sleeping_body_in_the_same_tick() {
    let mass_kg = 2.0_f32;
    let force_n = [10.0, 0.0, 0.0];
    let mut fixture = common::fixture(mass_kg);

    let mut slept = false;
    for _ in 0..300 {
        fixture.step(1);
        if fixture
            .world()
            .entity(fixture.body())
            .contains::<Sleeping>()
        {
            slept = true;
            break;
        }
    }
    assert!(
        slept,
        "the premise: an at-rest body must fall asleep within 300 ticks"
    );

    let dt = fixture.timestep_s();
    let expected_delta_v = force_n[0] / mass_kg * dt;

    fixture.submit(common::force_request(fixture.body(), force_n));
    fixture.step(1);

    let velocity = fixture.sample().linear_velocity_m_s;
    assert!(
        approx_eq(velocity[0], expected_delta_v),
        "a force on a sleeping body must add exactly F/m*dt = {expected_delta_v} in the submitting tick, got {velocity:?}"
    );
    assert!(
        !fixture
            .world()
            .entity(fixture.body())
            .contains::<Sleeping>(),
        "the request must have woken the body"
    );

    let ledger = fixture.ledger();
    assert_eq!(ledger.applied_requests, 1);
    assert_eq!(ledger.woken_requests, 1, "the wake must be accounted for");
    assert_eq!(ledger.total_woken_requests, 1);
    assert_eq!(ledger.dropped_requests, 0);

    // The wake must not repeat: a second tick without a request changes
    // nothing.
    fixture.step(1);
    assert_eq!(fixture.sample().linear_velocity_m_s, velocity);
    assert_eq!(fixture.ledger().woken_requests, 0);
}

/// A request that cannot reach a dynamic body is counted as dropped, never
/// applied and never lost.
///
/// Observable failure if a stale generation or a scripted body silently
/// swallows the request: `dropped_requests` stays 0 while the queue is drained,
/// or the kinematic body starts accelerating.
#[test]
fn accept_f23_b_requests_that_reach_no_dynamic_body_are_counted_dropped() {
    let mut fixture = common::fixture(2.0);
    let kinematic = common::spawn(
        &mut fixture,
        &BodySpec {
            layer: CollisionLayer::Aircraft,
            shape: ShapeClass::Solid,
            mode: BodyMode::Kinematic,
            mass_kg: 0.0,
            half_extents_m: [0.5, 0.5, 0.5],
            position_m: [10.0, 0.0, 0.0],
            linear_velocity_m_s: [0.0, 0.0, 0.0],
        },
    );
    let stale = fixture.world_mut().spawn_empty().id();
    fixture.world_mut().despawn(stale);

    fixture.submit(common::force_request(stale, [4.0, 0.0, 0.0]));
    fixture.submit(common::force_request(kinematic, [4.0, 0.0, 0.0]));
    fixture.step(1);

    let ledger = fixture.ledger();
    assert_eq!(
        ledger.dropped_requests, 2,
        "both requests reached no dynamic body: {ledger:?}"
    );
    assert_eq!(ledger.total_dropped_requests, 2);
    assert_eq!(ledger.applied_requests, 0);
    assert_eq!(
        fixture.sample().linear_velocity_m_s,
        [0.0, 0.0, 0.0],
        "the fixture body must not be touched by requests aimed elsewhere"
    );
    assert_eq!(
        fixture
            .world()
            .entity(kinematic)
            .get::<avian3d::prelude::LinearVelocity>()
            .expect("the kinematic body has a velocity")
            .0
            .x,
        0.0,
        "a scripted body must not be accelerated by the force queue"
    );
}
