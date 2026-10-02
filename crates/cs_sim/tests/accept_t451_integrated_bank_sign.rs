//! Task #451: the F31 follower's bank command turns the **integrated** body
//! toward the target, not away from it.
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md` (stage
//! `### F31-C`, AC03); shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//! Task test prefix: `accept_t451_`.
//!
//! This drives production code end to end: the production
//! [`Navigator::decide`] produces the bounded [`cs_sim::flight::FlightInput`],
//! and the production [`FlightModel`] (`cs_sim::flight::synthetic_fixed_wing`)
//! turns it into forces and torque. The fixed-tick integrator here is only
//! glue — the semi-implicit Euler a probe needs to fly the equations,
//! mirroring `cs_sim::probes::ProbeRunner` — while every force, torque and
//! command comes from the production types. Inverting the follower's roll sign
//! makes the heading turn the other way, so the test fails when the #451 fix is
//! removed.
//!
//! Every value is newly authored synthetic fixture data, never original game
//! data, and nothing here reads `CS_GAME_DIR`.

use cs_sim::ai::navigation::{
    NavState, NavigationCadence, NavigationRequest, Navigator, ReferenceFrameSample, RouteFrame,
    RouteGraph, RouteNode, RouteNodeId, RouteProgress, RouteTermination, heading_from_direction,
    synthetic_maneuver_envelope,
};
use cs_sim::flight::{
    BODY_FORWARD, BODY_UP, DamageState, EngineState, FlightEnvironment, FlightInput, FlightModel,
    FlightState, LoadoutMass, synthetic_fixed_wing,
};
use cs_types::Tick;
use cs_types::space::Quaternion;

/// The designed fixed step of the integrated probe, matching the F24 probe
/// cadence. Newly authored probe design.
const DT_S: f64 = 1.0 / 120.0;

/// How long the probe flies before it measures the heading. Short enough that
/// the (intentionally unstable, see the finding) open-loop bank has not yet
/// wrapped, long enough that the turn direction is unambiguous.
const PROBE_TICKS: usize = 120;

/// The single mandatory marker the follower aims at.
const MARKER_M: [f64; 3] = [0.0, 0.0, -120.0];

/// The lateral displacement of the actor from the marker's line, in meters.
const OFFSET_M: f64 = 40.0;

/// Flies the production follower + production flight model for [`PROBE_TICKS`]
/// ticks and returns the final body heading.
///
/// `target_left` puts the marker to the actor's left (canonical `+X`, the actor
/// at `+X`); `flip_roll` negates the produced roll command, which is the #451
/// mutation (the pre-fix sign).
fn fly(target_left: bool, flip_roll: bool) -> f64 {
    let model = FlightModel::new(synthetic_fixed_wing());
    let tuning = model.tuning();
    // `probes::ProbeRunner` reorders the per-control inertia `[roll, pitch,
    // yaw]` into body `[x, y, z]`; the same mapping is needed here.
    let inertia = [
        tuning.mass.inertia_kg_m2[1],
        tuning.mass.inertia_kg_m2[2],
        tuning.mass.inertia_kg_m2[0],
    ];
    let mass_kg = tuning.mass.mass_kg;
    let navigator = Navigator::new(
        synthetic_maneuver_envelope(),
        NavigationCadence::designed_default(),
    )
    .expect("the designed synthetic envelope and cadence are valid");
    let route = RouteGraph {
        frame: RouteFrame::World,
        termination: RouteTermination::End,
        clearance_m: 0.0,
        nodes: vec![RouteNode {
            id: RouteNodeId(0),
            sequence: 0,
            mandatory: true,
            position_m: MARKER_M,
            arrival_radius_m: 10.0,
        }],
    };

    let mut orientation = Quaternion::IDENTITY.components();
    let mut position: [f64; 3] = [if target_left { OFFSET_M } else { -OFFSET_M }, 0.0, 0.0];
    let mut velocity: [f64; 3] = [0.0, 0.0, -40.0];
    let mut angular: [f64; 3] = [0.0; 3];
    let mut engine = EngineState::direct(1.0);

    for tick in 0..PROBE_TICKS {
        let rotation =
            Quaternion::try_new(orientation).expect("the integrated rotation stays unit");
        let forward = rotate(BODY_FORWARD, rotation);
        let heading = heading_from_direction(forward[0], forward[2]);
        let speed = (velocity[0] * velocity[0] + velocity[2] * velocity[2]).sqrt();
        let state = NavState {
            position_m: position,
            heading_rad: heading,
            speed_mps: speed,
            climb_mps: velocity[1],
        };
        let decision = navigator
            .decide(&NavigationRequest {
                tick: Tick(tick as u64),
                generation: 1,
                state,
                route: &route,
                progress: RouteProgress::start(),
                frame: ReferenceFrameSample::IDENTITY,
                blockers: &[],
                dt_s: DT_S,
            })
            .expect("the follower's request is valid");
        let mut command = decision.command;
        if flip_roll {
            command.roll = -command.roll;
        }

        let flight_state = FlightState {
            orientation: rotation,
            linear_velocity_mps: velocity,
            angular_velocity_radps: angular,
            engine,
            boost_available: false,
        };
        let output = model
            .compute(
                &FlightEnvironment::SEA_LEVEL,
                &LoadoutMass::EMPTY,
                &DamageState::PRISTINE,
                &flight_state,
                &command,
                DT_S,
            )
            .expect("the production flight model accepts the bounded command");

        // Semi-implicit Euler: velocity first, then position (as ProbeRunner).
        for axis in 0..3 {
            velocity[axis] += output.world_force_n[axis] / mass_kg * DT_S;
            position[axis] += velocity[axis] * DT_S;
        }
        let body_torque = rotate_inverse(output.world_torque_nm, orientation);
        for axis in 0..3 {
            angular[axis] += body_torque[axis] / inertia[axis] * DT_S;
        }
        orientation = integrate_orientation(orientation, angular, DT_S);
        engine.advance(
            command.throttle,
            tuning.engine.throttle_response_per_s,
            DT_S,
        );
    }

    let rotation = Quaternion::try_new(orientation).expect("the integrated rotation stays unit");
    let forward = rotate(BODY_FORWARD, rotation);
    heading_from_direction(forward[0], forward[2])
}

/// The minimum world-Y of the body-up axis while the production `FlightModel`
/// flies `ticks` fixed steps with a **held** roll command (no follower, no
/// feedback). `up[1]` is the cosine of the bank angle, so a value below zero
/// means the body rolled past 90 degrees.
fn held_roll_min_up_y(roll: f64, ticks: usize) -> f64 {
    let model = FlightModel::new(synthetic_fixed_wing());
    let tuning = model.tuning();
    let inertia = [
        tuning.mass.inertia_kg_m2[1],
        tuning.mass.inertia_kg_m2[2],
        tuning.mass.inertia_kg_m2[0],
    ];
    let mass_kg = tuning.mass.mass_kg;
    let mut orientation = Quaternion::IDENTITY.components();
    let mut velocity: [f64; 3] = [0.0, 0.0, -40.0];
    let mut angular: [f64; 3] = [0.0; 3];
    let mut engine = EngineState::direct(1.0);
    let command = FlightInput::try_new(0.0, roll, 0.0, 0.9, false).expect("valid held roll");
    let mut min_up_y = f64::INFINITY;

    for _ in 0..ticks {
        let rotation =
            Quaternion::try_new(orientation).expect("the integrated rotation stays unit");
        let flight_state = FlightState {
            orientation: rotation,
            linear_velocity_mps: velocity,
            angular_velocity_radps: angular,
            engine,
            boost_available: false,
        };
        let output = model
            .compute(
                &FlightEnvironment::SEA_LEVEL,
                &LoadoutMass::EMPTY,
                &DamageState::PRISTINE,
                &flight_state,
                &command,
                DT_S,
            )
            .expect("the production flight model accepts the held command");
        for (axis, component) in velocity.iter_mut().enumerate() {
            *component += output.world_force_n[axis] / mass_kg * DT_S;
        }
        let body_torque = rotate_inverse(output.world_torque_nm, orientation);
        for axis in 0..3 {
            angular[axis] += body_torque[axis] / inertia[axis] * DT_S;
        }
        orientation = integrate_orientation(orientation, angular, DT_S);
        engine.advance(
            command.throttle,
            tuning.engine.throttle_response_per_s,
            DT_S,
        );
        let up = rotate(
            BODY_UP,
            Quaternion::try_new(orientation).expect("the integrated rotation stays unit"),
        );
        min_up_y = min_up_y.min(up[1]);
    }
    min_up_y
}

/// A positive heading step command (target to the left) turns the integrated
/// body nose-left; the pre-#451 sign turns it nose-right. The mirror holds for
/// a target to the right.
#[test]
fn accept_t451_a_integrated_follower_turns_toward_the_target_it_commands() {
    let toward_left = fly(true, false);
    assert!(
        toward_left > 0.0,
        "the follower must bank toward the left target (nose-left), got heading {toward_left}"
    );
    let flipped_left = fly(true, true);
    assert!(
        flipped_left < 0.0,
        "flipping the roll sign must bank away from the left target, got heading {flipped_left}"
    );

    let toward_right = fly(false, false);
    assert!(
        toward_right < 0.0,
        "the follower must bank toward the right target (nose-right), got heading {toward_right}"
    );
    let flipped_right = fly(false, true);
    assert!(
        flipped_right > 0.0,
        "flipping the roll sign must bank away from the right target, got heading {flipped_right}"
    );
}

/// Records #451's "not the envelope's subject" evidence, reproducibly: with a
/// **held** maximum roll command the production F24 synthetic airframe rolls
/// past 90 degrees instead of settling at the envelope's `max_bank_rad` of
/// `pi/3`. Its roll channel is a rate command with no bank holding, so a
/// follower bounded by a bank-angle envelope cannot fly it without measured
/// bank state. See `docs/findings/2026-10-02-t451-bank-sign-and-envelope-subject.md`.
#[test]
fn accept_t451_the_f24_synthetic_airframe_does_not_hold_a_bank() {
    // Five synthetic seconds: long enough that a rate-command roll has carried
    // the body well past 90 degrees, far short of the F31 route legs.
    let min_up_y = held_roll_min_up_y(1.0, 600);
    assert!(
        min_up_y < 0.0,
        "a held full roll must roll the body past 90 degrees (up.y < 0), got {min_up_y}"
    );
}

// ---------------------------------------------------------- integrator glue --
//
// The same body-frame integration `cs_sim::probes::ProbeRunner` uses to fly the
// production equations without Bevy/Avian. It owns no force law: it only
// advances the state the production model produced.

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f64; 3], factor: f64) -> [f64; 3] {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

fn norm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// Rotates `vector` from body space to world space by `rotation`.
fn rotate(vector: [f64; 3], rotation: Quaternion) -> [f64; 3] {
    let [x, y, z, w] = rotation.components();
    let axis = [x, y, z];
    let axis_cross = cross(axis, vector);
    add(
        add(vector, scale(axis_cross, 2.0 * w)),
        scale(cross(axis, axis_cross), 2.0),
    )
}

/// Rotates a world vector into body space (the inverse of `orientation`).
fn rotate_inverse(vector: [f64; 3], orientation: [f64; 4]) -> [f64; 3] {
    let conjugate = [
        -orientation[0],
        -orientation[1],
        -orientation[2],
        orientation[3],
    ];
    let q = Quaternion::try_new(conjugate).expect("the conjugate stays unit");
    rotate(vector, q)
}

/// Hamilton product `a ⊗ b` of `[x, y, z, w]` quaternions.
fn quat_mul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// Applies a body-frame angular velocity for `dt_s` and renormalizes.
fn integrate_orientation(orientation: [f64; 4], body_rate: [f64; 3], dt_s: f64) -> [f64; 4] {
    let rate = norm(body_rate);
    let angle = rate * dt_s;
    let delta = if angle > 1e-12 {
        let (sin, cos) = (angle * 0.5).sin_cos();
        let factor = sin / rate;
        [
            body_rate[0] * factor,
            body_rate[1] * factor,
            body_rate[2] * factor,
            cos,
        ]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    };
    let next = quat_mul(orientation, delta);
    let length =
        (next[0] * next[0] + next[1] * next[1] + next[2] * next[2] + next[3] * next[3]).sqrt();
    [
        next[0] / length,
        next[1] / length,
        next[2] / length,
        next[3] / length,
    ]
}
