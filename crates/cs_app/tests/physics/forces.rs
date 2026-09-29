//! The one-tick force/torque path (F23-A minimum scenario, AC01).

use cs_app::physics::{ForceRequest, ForceRequestError};

use crate::common;

/// Small relative tolerance: one integration is separated from two by a factor
/// of two, so this still fails loudly on a double-apply.
const REL_TOL: f32 = 1e-4;

fn approx_eq(actual: f32, expected: f32) -> bool {
    (actual - expected).abs() <= REL_TOL * expected.abs().max(1.0)
}

/// AC01: apply a known force to a known synthetic mass and verify delta-v
/// exactly once per tick.
///
/// Observable failure if the adapter's force step is removed or applied twice:
/// the velocity after one tick is `0` or `2 * F/m * dt` instead of
/// `F/m * dt`.
#[test]
fn accept_f23_a_known_force_on_known_mass_gives_one_tick_delta_v() {
    let mass_kg = 2.0_f32;
    let force_n = [0.0, 0.0, 12.0_f32];
    let mut fixture = common::fixture(mass_kg);
    let dt = fixture.timestep_s();
    let expected_delta_v = force_n[2] / mass_kg * dt;

    let request = common::force_request(fixture.body(), force_n);
    fixture.submit(request);
    fixture.step(1);

    let velocity = fixture.sample().linear_velocity_m_s;
    assert_eq!(
        (velocity[0], velocity[1]),
        (0.0, 0.0),
        "a force along +Z must not move the other axes: {velocity:?}"
    );
    assert!(
        approx_eq(velocity[2], expected_delta_v),
        "one tick must add exactly F/m*dt = {expected_delta_v}, got {}",
        velocity[2]
    );
    assert_eq!(
        fixture.ledger().total_applied_requests,
        1,
        "exactly one request must have been applied"
    );
}

/// A request is consumed by the tick that submitted it and does not persist.
///
/// Observable failure if the queue is not drained or the force is stored on the
/// body: the second tick keeps accelerating it.
#[test]
fn accept_f23_a_force_is_applied_once_and_not_carried_to_the_next_tick() {
    let mass_kg = 4.0_f32;
    let force_n = [5.0, 0.0, 0.0];
    let mut fixture = common::fixture(mass_kg);
    let dt = fixture.timestep_s();
    let expected_delta_v = force_n[0] / mass_kg * dt;

    fixture.submit(common::force_request(fixture.body(), force_n));
    fixture.step(1);
    let after_first = fixture.sample().linear_velocity_m_s;
    assert!(
        approx_eq(after_first[0], expected_delta_v),
        "first tick must add {expected_delta_v}, got {after_first:?}"
    );

    // No request this tick: with zero gravity and no damping the velocity must
    // be untouched.
    fixture.step(1);
    let after_second = fixture.sample().linear_velocity_m_s;
    assert_eq!(
        after_second, after_first,
        "the first tick's force must not leak into the second"
    );
    assert_eq!(fixture.ledger().applied_requests, 0);
    assert_eq!(fixture.ledger().total_applied_requests, 1);
}

/// One request per tick accumulates one delta-v per tick, never more.
///
/// Observable failure if a request is applied on every substep or tick twice:
/// four ticks of the same request would overshoot the expected four deltas.
#[test]
fn accept_f23_a_one_request_per_tick_accumulates_one_delta_v_per_tick() {
    let mass_kg = 8.0_f32;
    let force_n = [0.0, -16.0, 0.0];
    let ticks = 4_u64;
    let mut fixture = common::fixture(mass_kg);
    let dt = fixture.timestep_s();
    let per_tick = force_n[1] / mass_kg * dt;

    for _ in 0..ticks {
        fixture.submit(common::force_request(fixture.body(), force_n));
        fixture.step(1);
    }

    let velocity = fixture.sample().linear_velocity_m_s;
    assert!(
        approx_eq(velocity[1], per_tick * ticks as f32),
        "{ticks} ticks must add {} total, got {}",
        per_tick * ticks as f32,
        velocity[1]
    );
    assert_eq!(fixture.ledger().total_applied_requests, ticks);
}

/// A torque request affects angular velocity once, then stops.
///
/// Observable failure if the torque path is missing (angular velocity stays
/// zero) or persists (the second tick keeps spinning the body up).
#[test]
fn accept_f23_a_torque_request_changes_angular_velocity_once() {
    let mut fixture = common::fixture(2.0);
    let torque_nm = [0.0, 0.0, 3.0];

    fixture.submit(common::torque_request(fixture.body(), torque_nm));
    fixture.step(1);
    let after_first = fixture.sample().angular_velocity_rad_s;
    assert!(
        after_first[2] != 0.0,
        "a torque about +Z must produce angular velocity about +Z: {after_first:?}"
    );

    fixture.step(1);
    assert_eq!(
        fixture.sample().angular_velocity_rad_s,
        after_first,
        "the torque must not persist past its tick"
    );
}

/// Non-finite force or torque input is rejected at the typed boundary.
#[test]
fn accept_f23_a_nonfinite_force_and_torque_are_rejected() {
    let body = common::fixture(1.0).body();

    assert_eq!(
        ForceRequest::new(body, [f32::NAN, 0.0, 0.0], [0.0; 3]),
        Err(ForceRequestError::NonFinite {
            field: "force_n[0]"
        })
    );
    assert_eq!(
        ForceRequest::new(body, [0.0; 3], [0.0, f32::INFINITY, 0.0]),
        Err(ForceRequestError::NonFinite {
            field: "torque_nm[1]"
        })
    );
    assert!(ForceRequest::new(body, [1.0, 2.0, 3.0], [0.0; 3]).is_ok());
}
