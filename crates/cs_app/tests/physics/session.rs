//! Producer/consumer integration tests for F23-C.
//!
//! The stage's minimum scenario is **AC03: "Switch kinematic to dynamic on
//! aircraft release without a pose or velocity discontinuity"**, covered by
//! `accept_f23_c_release_to_dynamic_keeps_pose_and_velocity` through the
//! production `PhysicsSession` — the object a game loop owns — not through
//! the raw components.
//!
//! The rest of the stage's wiring is covered the same way, end to end:
//! spawn preflight closes the measured first-tick sweep hole, the contact
//! reporter's reports reach the frame's consumer list exactly once per
//! episode with the despawn retention rule applied, presentation reads an
//! eased `Transform` while the simulation reads `Position`, and teardown
//! leaves only `Inactive` errors until `restart` rebuilds a clean world.
//!
//! Every value is newly authored fixture data. No original data and no
//! `CS_GAME_DIR` access.

use core::time::Duration;

use bevy::prelude::Entity;
use cs_app::physics::{
    BodyMode, BodySpec, ContactReports, ForceRequest, PhysicsSession, PhysicsSessionError,
};
use cs_sim::collision::{CollisionLayer, ContactKind, ShapeClass};

/// A session at the declared 120 Hz baseline with zero gravity.
fn session() -> PhysicsSession {
    PhysicsSession::new(120)
}

/// The session's fixed timestep.
fn timestep(session: &PhysicsSession) -> Duration {
    session.timestep()
}

/// An aircraft body spec.
fn aircraft(position: [f32; 3], velocity: [f32; 3], mode: BodyMode) -> BodySpec {
    BodySpec {
        layer: CollisionLayer::Aircraft,
        shape: ShapeClass::Solid,
        mode,
        mass_kg: 10.0,
        half_extents_m: [0.5, 0.5, 0.5],
        position_m: position,
        linear_velocity_m_s: velocity,
    }
}

/// A thin static wall across the x = 0 plane.
fn wall() -> BodySpec {
    BodySpec {
        layer: CollisionLayer::StaticWorld,
        shape: ShapeClass::Solid,
        mode: BodyMode::Static,
        mass_kg: 1.0,
        half_extents_m: [0.01, 5.0, 5.0],
        position_m: [0.0, 0.0, 0.0],
        linear_velocity_m_s: [0.0, 0.0, 0.0],
    }
}

/// The contact reports the session currently retains.
fn active_pairs(session: &PhysicsSession) -> usize {
    session
        .world()
        .expect("the session is active")
        .resource::<ContactReports>()
        .active_len()
}

/// **AC03: switching kinematic to dynamic on aircraft release has no pose or
/// velocity discontinuity.**
///
/// A scripted aircraft flies kinematic at constant velocity — its position
/// integrates from that velocity each tick — and at a tick boundary the
/// session releases it to dynamic flight. `Position` and `LinearVelocity`
/// must be identical across the switch itself (no teleport, no reset), the
/// next tick's position must continue from the scripted trajectory's last
/// point, and a force submitted afterward must integrate — that is what
/// distinguishes "the same body, now dynamic" from "a respawned body".
#[test]
fn accept_f23_c_release_to_dynamic_keeps_pose_and_velocity() {
    let mut session = session();
    let aircraft = session
        .spawn(&aircraft(
            [0.0, 0.0, 0.0],
            [12.0, 0.0, 0.0],
            BodyMode::Kinematic,
        ))
        .expect("the aircraft spec is valid")
        .entity;

    // Scripted flight: the kinematic body integrates position from its
    // velocity and nothing else touches it.
    session.step(4).expect("the frame applies");
    let before = session.pose(aircraft).expect("the aircraft exists");
    let dt = timestep(&session).as_secs_f32();
    assert!(
        (before.position_m[0] - 4.0 * 12.0 * dt).abs() < 1e-4,
        "the kinematic trajectory advanced by its velocity: {before:?}"
    );
    assert_eq!(before.linear_velocity_m_s[0], 12.0);

    // The release itself moves nothing.
    session
        .set_mode(aircraft, BodyMode::Dynamic)
        .expect("the release applies");
    let at_release = session.pose(aircraft).expect("the aircraft exists");
    assert_eq!(
        at_release, before,
        "the release must not write pose or velocity"
    );

    // The next tick continues from where the scripted trajectory stopped.
    session.step(1).expect("the frame applies");
    let after = session.pose(aircraft).expect("the aircraft exists");
    assert!(
        (after.position_m[0] - (before.position_m[0] + 12.0 * dt)).abs() < 1e-4,
        "the first dynamic tick continues the trajectory: before {before:?}, after {after:?}"
    );
    assert_eq!(
        after.linear_velocity_m_s[0], 12.0,
        "the release must not reset velocity"
    );

    // And it is genuinely dynamic now: a submitted force integrates once.
    session
        .submit(ForceRequest::new(aircraft, [120.0, 0.0, 0.0], [0.0; 3]).unwrap())
        .expect("the request queues");
    session.step(1).expect("the frame applies");
    let powered = session.pose(aircraft).expect("the aircraft exists");
    assert!(
        (powered.linear_velocity_m_s[0] - (12.0 + 120.0 / 10.0 * dt)).abs() < 1e-4,
        "a dynamic body takes the tick's force: {powered:?}"
    );
}

/// A release through the session must also be impossible to use as a hidden
/// reset: transitioning a *static* body to dynamic resumes the velocity it
/// was left with, because the transition never rewrites state behind the
/// caller's back (the F23-B limitation this stage resolves by keeping that
/// rule explicit).
#[test]
fn accept_f23_c_release_never_rewrites_state_behind_the_caller() {
    let mut session = session();
    let parked = session
        .spawn(&BodySpec {
            linear_velocity_m_s: [3.0, 0.0, 0.0],
            ..aircraft([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], BodyMode::Static)
        })
        .expect("the spec is valid")
        .entity;

    session
        .set_mode(parked, BodyMode::Dynamic)
        .expect("the release applies");
    let pose = session.pose(parked).expect("the body exists");
    assert_eq!(
        pose.linear_velocity_m_s,
        [3.0, 0.0, 0.0],
        "a static body's stored velocity survives release — never silently zeroed"
    );
}

/// A body spawned inside one tick's travel of a wall does not tunnel.
///
/// F23-B measured the hole: Avian's swept AABB is written at the end of a
/// tick, so a fast body's first tick has no sweep. The session's spawn
/// wiring closes it: the preflight shape-cast moves the spawn to the
/// contact point and records the correction as an authoritative event, and
/// the tick that follows reports the contact like any other.
#[test]
fn accept_f23_c_fast_spawn_preflight_clamps_onto_the_contact() {
    let mut session = session();
    let wall = session.spawn(&wall()).expect("the wall is valid").entity;
    // The collider tree needs one tick to know the wall.
    session.step(1).expect("the frame applies");

    // 240 m/s at 120 Hz is exactly 2 m of travel per tick; spawn the
    // projectile 1.5 m short of the wall face — inside the hole.
    let speed = 240.0;
    let projectile = session
        .spawn(&BodySpec {
            layer: CollisionLayer::Projectile,
            shape: ShapeClass::Solid,
            mode: BodyMode::Dynamic,
            mass_kg: 1.0,
            half_extents_m: [0.01, 0.01, 0.01],
            position_m: [-1.5, 0.0, 0.0],
            linear_velocity_m_s: [speed, 0.0, 0.0],
        })
        .expect("the projectile spec is valid");

    assert!(
        projectile.preflight_pending,
        "a moving swept-layer body must preflight its first tick"
    );

    let mut clamped_at = None;
    let mut max_x = f32::MIN;
    let mut contact_seen = false;
    for _ in 0..6 {
        let frame = session.step(1).expect("the frame applies");
        for event in &frame.spawn_events {
            assert_eq!(event.body, projectile.entity);
            assert!(event.clamped, "the spawn inside the hole was clamped");
            assert_eq!(event.hit, Some(wall));
            clamped_at = Some(event.distance_m.expect("a clamp records the distance"));
        }
        for report in &frame.reports {
            if report.bodies.contains(&projectile.entity) && report.bodies.contains(&wall) {
                assert_eq!(report.kind, ContactKind::SolidContact);
                contact_seen = true;
            }
        }
        if let Some(pose) = session.pose(projectile.entity) {
            max_x = max_x.max(pose.position_m[0]);
        }
    }

    let distance = clamped_at.expect("the preflight event was recorded");
    assert!(
        distance < 1.5,
        "the cast stopped short of the tick's travel: {distance}"
    );
    assert!(contact_seen, "the clamped spawn produced the wall contact");
    assert!(
        max_x < 0.0,
        "the projectile never crossed the wall plane (max x {max_x})"
    );
}

/// The preflight does not eat a spawn that is not headed anywhere solid:
/// a projectile whose first tick only crosses a sensor — or nothing —
/// keeps its spawn position, and the event says so.
#[test]
fn accept_f23_c_preflight_never_stops_on_a_sensor() {
    let mut session = session();
    session
        .spawn(&BodySpec {
            layer: CollisionLayer::Trigger,
            shape: ShapeClass::Sensor,
            mode: BodyMode::Static,
            mass_kg: 1.0,
            half_extents_m: [0.5, 0.5, 0.5],
            position_m: [-1.0, 0.0, 0.0],
            linear_velocity_m_s: [0.0, 0.0, 0.0],
        })
        .expect("the trigger is valid");
    session.step(1).expect("the frame applies");

    // The projectile's first tick crosses the sensor volume entirely, then
    // nothing solid — the spawn stands where it was authored.
    let projectile = session
        .spawn(&BodySpec {
            layer: CollisionLayer::Projectile,
            shape: ShapeClass::Solid,
            mode: BodyMode::Dynamic,
            mass_kg: 1.0,
            half_extents_m: [0.01, 0.01, 0.01],
            position_m: [-2.4, 0.0, 0.0],
            linear_velocity_m_s: [240.0, 0.0, 0.0],
        })
        .expect("the projectile spec is valid")
        .entity;

    let frame = session.step(1).expect("the frame applies");
    let event = frame
        .spawn_events
        .iter()
        .find(|event| event.body == projectile)
        .expect("the preflight resolved");
    assert!(
        !event.clamped,
        "a sensor must never clamp a spawn: {event:?}"
    );
    let pose = session.pose(projectile).expect("the projectile exists");
    assert!(
        pose.position_m[0] > -0.5,
        "the projectile flew through the sensor: {pose:?}"
    );
}

/// The contact reporter is the consumer of Avian's authoritative events,
/// and the session is the consumer of the reporter's: one `SessionFrame`
/// carries each tick's classified reports. A sensor overlap arrives exactly
/// once — the episode start — and never as damage.
#[test]
fn accept_f23_c_contact_reports_reach_the_consumer_once_per_episode() {
    let mut session = session();
    session
        .spawn(&BodySpec {
            layer: CollisionLayer::Trigger,
            shape: ShapeClass::Sensor,
            mode: BodyMode::Static,
            mass_kg: 1.0,
            half_extents_m: [0.6, 0.6, 0.6],
            position_m: [0.0, 0.0, 0.0],
            linear_velocity_m_s: [0.0, 0.0, 0.0],
        })
        .expect("the trigger is valid");
    session.step(1).expect("the frame applies");

    // An aircraft flies a straight line through the sensor: 120 m/s at
    // 120 Hz is 1 m per tick, through a 1.2 m box.
    let aircraft = session
        .spawn(&aircraft(
            [-3.0, 0.0, 0.0],
            [120.0, 0.0, 0.0],
            BodyMode::Dynamic,
        ))
        .expect("the aircraft spec is valid")
        .entity;

    let mut seen = 0;
    for _ in 0..8 {
        let frame = session.step(1).expect("the frame applies");
        for report in &frame.reports {
            if report.bodies.contains(&aircraft) {
                assert_eq!(
                    report.kind,
                    ContactKind::SensorOverlap,
                    "a sensor overlap is reported, never damage"
                );
                seen += 1;
            }
        }
    }
    assert_eq!(seen, 1, "one crossing is exactly one report, seen {seen}");
    assert_eq!(
        active_pairs(&session),
        0,
        "the pair closed when the aircraft left the sensor"
    );
}

/// A consumer that never drains loses nothing, and a despawned body leaves
/// no stale active pair behind (the reporter retention rule of this stage):
/// the pair is pruned when the body's layer marker is gone, because Avian
/// does not guarantee a `CollisionEnd` for a despawned collider.
#[test]
fn accept_f23_c_despawn_during_contact_releases_the_pair() {
    let mut session = session();
    let aircraft = session
        .spawn(&aircraft(
            [-0.2, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            BodyMode::Dynamic,
        ))
        .expect("the aircraft spec is valid")
        .entity;
    session
        .spawn(&BodySpec {
            layer: CollisionLayer::Trigger,
            shape: ShapeClass::Sensor,
            mode: BodyMode::Static,
            mass_kg: 1.0,
            half_extents_m: [0.6, 0.6, 0.6],
            position_m: [0.0, 0.0, 0.0],
            linear_velocity_m_s: [0.0, 0.0, 0.0],
        })
        .expect("the trigger is valid");

    session.step(2).expect("the frame applies");
    assert!(
        active_pairs(&session) > 0,
        "the overlap is an active contact pair"
    );

    session.despawn(aircraft).expect("the despawn applies");
    session.step(1).expect("the frame applies");
    assert_eq!(
        active_pairs(&session),
        0,
        "a despawned body cannot keep a contact pair active"
    );
}

/// Presentation reads the eased `Transform` while the simulation reads the
/// tick-quantized `Position`: inside a render frame that ran only half a
/// tick, the transform sits between the two tick poses instead of snapping,
/// and the authoritative pose does not move at all.
#[test]
fn accept_f23_c_presentation_transform_interpolates_between_fixed_poses() {
    let mut session = session();
    let aircraft = session
        .spawn(&aircraft(
            [0.0, 0.0, 0.0],
            [12.0, 0.0, 0.0],
            BodyMode::Dynamic,
        ))
        .expect("the aircraft spec is valid")
        .entity;
    let dt = timestep(&session).as_secs_f32();

    session.step(1).expect("the frame applies");
    let p1 = session
        .pose(aircraft)
        .expect("the aircraft exists")
        .position_m[0];
    assert!((p1 - 12.0 * dt).abs() < 1e-4, "one tick of travel: {p1}");

    // A render frame half a tick long runs no fixed tick: the eased
    // transform must sit between the previous and the current tick pose
    // while `Position` is exactly the tick pose.
    let frame = session
        .pump_frame(Duration::from_secs_f64(f64::from(dt) / 2.0))
        .expect("the frame applies");
    assert_eq!(frame.ticks_ran, 0, "a sub-tick frame runs no tick");
    let shown = session
        .presentation_translation(aircraft)
        .expect("the aircraft has a transform");
    let p_still = session
        .pose(aircraft)
        .expect("the aircraft exists")
        .position_m[0];
    assert_eq!(p_still, p1, "the authoritative pose did not move");
    assert!(
        shown.x > 0.0 && shown.x < p1,
        "the presented transform eases between tick poses: {shown} within (0, {p1})"
    );

    // A full tick later the presented transform reaches the new pose.
    session.step(1).expect("the frame applies");
    let p2 = session
        .pose(aircraft)
        .expect("the aircraft exists")
        .position_m[0];
    assert!(
        (p2 - 24.0 * dt).abs() < 1e-4,
        "the second tick landed on the trajectory: {p2}"
    );
}

/// Teardown drops the world: every producer call fails `Inactive` until
/// `restart` builds a fresh one with no carried-over reports, requests or
/// active pairs — a retry cannot inherit the torn-down run's state.
#[test]
fn accept_f23_c_teardown_and_restart_rebuild_a_clean_world() {
    let mut session = session();
    let plane = session
        .spawn(&aircraft(
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            BodyMode::Dynamic,
        ))
        .expect("the aircraft spec is valid")
        .entity;
    session
        .submit(ForceRequest::new(plane, [10.0, 0.0, 0.0], [0.0; 3]).unwrap())
        .expect("the request queues");
    session.step(2).expect("the frame applies");
    assert_eq!(session.tick(), 2);

    session.teardown();
    assert!(!session.is_active());
    assert!(matches!(
        session.step(1),
        Err(PhysicsSessionError::Inactive)
    ));
    assert!(matches!(
        session.spawn(&aircraft([0.0, 0.0, 0.0], [0.0; 3], BodyMode::Dynamic)),
        Err(PhysicsSessionError::Inactive)
    ));
    assert!(matches!(
        session.set_mode(plane, BodyMode::Static),
        Err(PhysicsSessionError::Inactive)
    ));
    assert!(matches!(
        session.despawn(plane),
        Err(PhysicsSessionError::Inactive)
    ));
    assert!(session.pose(plane).is_none());
    assert!(session.world().is_none());

    session.restart();
    assert!(session.is_active());
    assert_eq!(session.tick(), 0, "a restarted world starts at tick 0");
    let frame = session.step(1).expect("the rebuilt world steps");
    assert!(frame.reports.is_empty(), "no stale contact reports");
    assert!(frame.spawn_events.is_empty(), "no stale preflight events");
    assert_eq!(frame.applied_requests, 0, "no carried-over force request");
    // The old world's entities are gone with it.
    assert!(session.pose(plane).is_none());
    assert_eq!(active_pairs(&session), 0);
}

/// Error paths are propagated, never swallowed: a spec the boundary
/// rejects, a transition on a non-body, a despawn of something that is not
/// here, and a request for a body that is already gone — each surfaces as
/// its own failure or its own counter.
#[test]
fn accept_f23_c_failures_propagate_and_stale_requests_are_counted() {
    let mut session = session();

    // A non-finite spec is refused before anything spawns.
    let mut bad = aircraft([0.0, 0.0, 0.0], [0.0; 3], BodyMode::Dynamic);
    bad.mass_kg = f32::NAN;
    assert!(matches!(
        session.spawn(&bad),
        Err(PhysicsSessionError::Body(_))
    ));

    // A transition on an entity that is not a body, and a despawn of an
    // entity that does not exist.
    let ghost = Entity::from_bits(9_999);
    assert!(matches!(
        session.set_mode(ghost, BodyMode::Static),
        Err(PhysicsSessionError::Transition(_))
    ));
    assert!(matches!(
        session.despawn(ghost),
        Err(PhysicsSessionError::UnknownBody(_))
    ));

    // A force for a despawned body is counted as dropped, not applied and
    // not crashed.
    let aircraft = session
        .spawn(&aircraft([0.0, 0.0, 0.0], [0.0; 3], BodyMode::Dynamic))
        .expect("the aircraft spec is valid")
        .entity;
    session.despawn(aircraft).expect("the despawn applies");
    session
        .submit(ForceRequest::new(aircraft, [10.0, 0.0, 0.0], [0.0; 3]).unwrap())
        .expect("the request queues");
    let frame = session.step(1).expect("the frame applies");
    assert_eq!(frame.applied_requests, 0);
    assert_eq!(
        frame.dropped_requests, 1,
        "a request for a gone body is a counted drop: {frame:?}"
    );
}

/// A render frame that covers several fixed ticks delivers every tick's
/// events: the consumer cannot see a report overwritten by the next tick of
/// the same frame, and a sub-tick remainder still updates the eased
/// presentation transform without running a tick.
#[test]
fn accept_f23_c_a_multi_tick_frame_delivers_every_tick() {
    let mut session = session();
    session
        .spawn(&BodySpec {
            layer: CollisionLayer::Trigger,
            shape: ShapeClass::Sensor,
            mode: BodyMode::Static,
            mass_kg: 1.0,
            half_extents_m: [0.6, 0.6, 0.6],
            position_m: [0.0, 0.0, 0.0],
            linear_velocity_m_s: [0.0, 0.0, 0.0],
        })
        .expect("the trigger is valid");
    let aircraft = session
        .spawn(&aircraft(
            [-2.6, 0.0, 0.0],
            [120.0, 0.0, 0.0],
            BodyMode::Dynamic,
        ))
        .expect("the aircraft spec is valid")
        .entity;
    session.step(1).expect("the world knows the colliders");

    // Three and a half ticks in one frame: the crossing happens on the
    // second tick of the frame and must still be in the frame's list.
    let dt = timestep(&session);
    let frame = session
        .pump_frame(dt * 3 + dt / 2)
        .expect("the frame applies");
    assert_eq!(frame.ticks_ran, 3);
    let crossing = frame
        .reports
        .iter()
        .find(|report| report.bodies.contains(&aircraft));
    let crossing = crossing.expect("the mid-frame tick's report was delivered");
    assert_eq!(crossing.kind, ContactKind::SensorOverlap);
    assert!(
        crossing.tick >= 1 && crossing.tick <= 4,
        "the report carries its own tick: {crossing:?}"
    );
}
