//! Task #710 (`PLAYTEST-PROP-SPIN`) acceptance tests. Prefix:
//! `accept_playtest_prop_spin_`.
//!
//! Owner playtest feedback 2026-10-06: "the propeller doesn't spin". These
//! tests hold the fix to that one observation, and every one of them drives
//! production code: the hub **measurement** is
//! `playtest_retail::measure_propeller_hub`, the component and its rate curve
//! are `playtest::propeller`, the system is registered in `PlaytestPlugin`,
//! the app is the playtest's own `headless_app` / `headless_app_with` plus
//! `retail::install`, and the throttle moves through the production F22
//! input session (`1` / `4` are the shipped default bindings).
//!
//! The retail half is `#[ignore]`d (`requires CS_GAME_DIR`) and **fails**
//! without the variable; it reads the owner's installation read-only and
//! commits no original bytes. The rest needs no installation: they build a
//! synthetic disc, measure it with the same production function the retail
//! path uses, and spin it on the real flight body.
//!
//! These tests share the `tests/playtest_retail.rs` binary rather than linking
//! another copy of the engine (the reason #666's and #709's tests do too).

use std::path::PathBuf;

use avian3d::prelude::{Position, Rotation};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput, NativeKey};
use bevy::math::{Quat, Vec3};
use bevy::prelude::{App, ChildOf, Entity, Transform, With};
use cs_app::physics::FlightAircraft;
use cs_app::playtest::PlaytestState;
use cs_app::playtest::headless_app;
use cs_app::playtest::propeller::{
    PLAYTEST_PROP_SPIN_RATE_IS_DESIGNED, PROP_SPIN_FULL_REV_PER_S, PROP_SPIN_IDLE_REV_PER_S,
    PropellerSpin, propeller_spin_rate_rev_per_s,
};
use cs_app::playtest::retail::{self, RetailContent, RetailRequest};
use cs_app::playtest::scene::PlaytestAircraft;
use cs_app::playtest_retail::{
    PLAYTEST_AIRCRAFT_PROP_NODE_NAME, PLAYTEST_AIRCRAFT_PROP_NODE_SLOT,
    PLAYTEST_AIRCRAFT_ROOT_NAME, PLAYTEST_PROP_SPIN_SENSE_IS_DESIGNED, PLAYTEST_WORLD_GROUP,
    PlaytestAircraftReport, PlaytestAreaReport, PropellerHub, PropellerSpinSpec,
    STORED_AIRCRAFT_NOSE_AXIS, UndrawnBinding, aircraft_graph, measure_propeller_hub,
    playtest_adapter, read_playtest_sources,
};
use cs_app::playtest_textures::PlaytestTextureReport;
use cs_app::world::retail::stored_render_mesh;
use cs_content::mesh::RenderMesh;
use cs_content::world::{Aabb, WorldId};
use cs_formats::gamez::{PrimitiveKind, RawCorner, RawMesh, RawPolygon};
use cs_sim::flight::{BODY_FORWARD, EngineState};
use cs_types::evidence::ClaimId;

// --------------------------------------------------------------- the fixture --

/// The synthetic disc's radius, in metres: a flat blade set the measurement can
/// be checked against exactly.
const DISC_RADIUS: f32 = 0.5;
/// Where the synthetic disc sits along the stored `z` axis, in metres, so the
/// measured pivot is a value no rule could accidentally produce.
const DISC_Z: f32 = 1.5;
/// How many triangles the synthetic disc fans into.
const DISC_SEGMENTS: u32 = 8;

/// A flat disc of `DISC_SEGMENTS` triangles in the `z = DISC_Z` plane, wound
/// so its normals point `+z`, built through the production mesh reader
/// (`RenderMesh::build`) from a stored-shaped `RawMesh`.
fn synthetic_disc() -> RenderMesh {
    let mut positions = vec![[0.0, 0.0, DISC_Z]];
    for step in 0..DISC_SEGMENTS {
        let angle = (step as f32) * std::f32::consts::TAU / DISC_SEGMENTS as f32;
        positions.push([DISC_RADIUS * angle.cos(), DISC_RADIUS * angle.sin(), DISC_Z]);
    }
    let polygons = (0..DISC_SEGMENTS)
        .map(|step| {
            let corners = [0, 1 + step, 1 + (step + 1) % DISC_SEGMENTS]
                .into_iter()
                .map(|position| RawCorner {
                    position,
                    normal: None,
                    uv: None,
                    color: None,
                })
                .collect();
            RawPolygon {
                kind: PrimitiveKind::Polygon,
                raw_flags: 0,
                material: 0,
                corners,
            }
        })
        .collect();
    RenderMesh::build(&RawMesh {
        positions,
        normals: Vec::new(),
        polygons,
    })
    .expect("the authored disc has a decodable outline")
}

/// The synthetic disc's hub, through the **production measurement** — the same
/// call the retail scene makes for `staticprop1`.
fn measured_hub() -> PropellerHub {
    let hub = measure_propeller_hub(&synthetic_disc()).expect("the authored disc measures a hub");
    // The fixture is exact, so the measurement is checked against it here as
    // well as in its own test below: axis `+z` (aft of the measured `-z` nose),
    // pivot on the disc's own plane, radius and thickness the authored values.
    assert!(
        (Vec3::from(hub.axis) - Vec3::Z).length() < 1.0e-5,
        "the disc's measured axis is its own +z normal, oriented aft: {:?}",
        hub.axis
    );
    assert!(
        (Vec3::from(hub.pivot) - Vec3::new(0.0, 0.0, DISC_Z)).length() < 1.0e-5,
        "the pivot is the disc's own centroid: {:?}",
        hub.pivot
    );
    assert!((hub.radius_m - DISC_RADIUS).abs() < 1.0e-5, "{hub:?}");
    assert!(hub.thickness_m < 1.0e-6, "{hub:?}");
    hub
}

/// Where the test's own propeller sits under the body: a placement that is not
/// axis aligned, so the affine carry of the measured hub is exercised rather
/// than assumed.
fn propeller_base() -> Transform {
    Transform::from_xyz(0.0, 0.0, -4.5).with_rotation(Quat::from_rotation_y(0.35))
}

// ------------------------------------------------------------------- helpers --

fn install_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the original installation"),
    )
}

fn request() -> RetailRequest {
    RetailRequest::new(install_dir(), None, None).expect("the documented defaults are valid")
}

fn key(app: &mut App, code: KeyCode, state: ButtonState) {
    app.world_mut().write_message(KeyboardInput {
        key_code: code,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
}

/// One key press and release through the production input path, two frames.
fn tap(app: &mut App, code: KeyCode) {
    key(app, code, ButtonState::Pressed);
    app.update();
    key(app, code, ButtonState::Released);
    app.update();
}

fn run(app: &mut App, frames: u32) {
    for _ in 0..frames {
        app.update();
    }
}

/// The one player aircraft's body entity.
fn body_of(app: &mut App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, With<PlaytestAircraft>>()
        .single(app.world())
        .expect("one player aircraft")
}

/// Attaches a propeller child carrying the measured hub to the live flight
/// body, exactly as `spawn_retail_parts` does over retail content.
fn attach_propeller(app: &mut App, hub: &PropellerHub) -> Entity {
    let body = body_of(app);
    let base = propeller_base();
    app.world_mut()
        .spawn((base, ChildOf(body), PropellerSpin::from_hub(hub, base)))
        .id()
}

/// The same child **without** the spin component: the only difference between
/// the two apps of the body-pose test.
fn attach_plain_child(app: &mut App) -> Entity {
    let body = body_of(app);
    app.world_mut()
        .spawn((propeller_base(), ChildOf(body)))
        .id()
}

fn spin_of(app: &mut App, propeller: Entity) -> PropellerSpin {
    *app.world()
        .get::<PropellerSpin>(propeller)
        .expect("the propeller carries its spin state")
}

fn transform_of(app: &mut App, propeller: Entity) -> Transform {
    *app.world()
        .get::<Transform>(propeller)
        .expect("the propeller has a placement")
}

fn revolutions(app: &mut App, propeller: Entity) -> f32 {
    spin_of(app, propeller).revolutions()
}

/// Every entity that carries a spin state, in entity order.
fn spinners(app: &mut App) -> Vec<Entity> {
    app.world_mut()
        .query_filtered::<Entity, With<PropellerSpin>>()
        .iter(app.world())
        .collect()
}

fn throttle(app: &App) -> f64 {
    app.world().resource::<PlaytestState>().command.throttle
}

/// Stops the engine through the production flight record.
fn engine_off(app: &mut App) {
    let mut query = app
        .world_mut()
        .query_filtered::<&mut FlightAircraft, With<PlaytestAircraft>>();
    let mut aircraft = query
        .single_mut(app.world_mut())
        .expect("the player aircraft flies the production model");
    aircraft
        .set_engine(EngineState::STOPPED)
        .expect("an engine can be stopped");
}

/// The disc really turns **about the measured hub axis through the measured
/// pivot**, checked on the drawn transform rather than on the component's own
/// bookkeeping: the pivot is a fixed point of the applied placement, the axis
/// line through it is invariant, and the rotation the body sees is the
/// recorded turn about that very axis.
fn assert_turns_about_the_measured_hub(app: &mut App, propeller: Entity, hub: &PropellerHub) {
    let spin = spin_of(app, propeller);
    let transform = transform_of(app, propeller);
    let pivot_local = Vec3::from(hub.pivot);
    let axis_local = Vec3::from(hub.axis);

    let drawn_pivot = transform.transform_point(pivot_local);
    assert!(
        (drawn_pivot - spin.pivot()).length() < 1.0e-4,
        "the measured pivot {:?} must stay at {:?} while the disc turns, got {:?}",
        spin.pivot(),
        spin.pivot(),
        drawn_pivot,
    );

    let drawn_axis =
        (transform.transform_point(pivot_local + axis_local) - drawn_pivot).normalize();
    assert!(
        (drawn_axis - spin.axis()).length() < 1.0e-4,
        "the disc must turn about its measured axis {:?}, got {:?}",
        spin.axis(),
        drawn_axis,
    );

    let applied = transform.rotation * spin.base().rotation.inverse();
    let expected = Quat::from_axis_angle(spin.axis(), spin.rotation_radians());
    // A rounding bound, not a structural one: `transform.rotation` is a
    // composed f32 quaternion and `expected` is rebuilt from the recorded
    // angle, so they agree to the last bits rather than bit for bit. The two
    // checks above are the structural ones — the pivot is fixed and the axis
    // line through it is invariant — and a rotation about any other axis fails
    // them by centimetres and degrees, far outside this tolerance.
    assert!(
        applied.angle_between(expected) < 5.0e-3,
        "the drawn rotation must be the recorded {:?} turns ({:?} rad) about the measured axis, \
         got {applied:?} against {expected:?}",
        spin.revolutions(),
        spin.rotation_radians(),
    );
}

/// The point the disc's rim passes through, in the mesh frame: off the hub, so
/// it must move while the hub does not.
fn rim_point(hub: &PropellerHub) -> Vec3 {
    let axis = Vec3::from(hub.axis);
    let reference = if axis.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let across = axis.cross(reference).normalize();
    Vec3::from(hub.pivot) + across * hub.radius_m
}

// ------------------------------------------------------- the designed rate --

/// **The rate curve is a claim, not a measurement: idle is slower than full,
/// both are positive and visible, and a stopped engine turns nothing.**
///
/// The curve is the one designed value this task adds (no airframe record of
/// `ZBD/planes.zbd` stores an rpm and no original run has been watched), so it
/// is filed under its own claim id and read through the production
/// `propeller_spin_rate_rev_per_s` — the same function the spin system calls.
#[test]
fn accept_playtest_prop_spin_rate_curve_is_claimed_idle_positive_and_monotonic() {
    assert!(
        ClaimId::new(PLAYTEST_PROP_SPIN_RATE_IS_DESIGNED).is_ok(),
        "the rate curve must be a valid claim id: {PLAYTEST_PROP_SPIN_RATE_IS_DESIGNED}"
    );
    assert!(
        ClaimId::new(PLAYTEST_PROP_SPIN_SENSE_IS_DESIGNED).is_ok(),
        "the spin sense is a claim of its own: {PLAYTEST_PROP_SPIN_SENSE_IS_DESIGNED}"
    );
    assert_ne!(
        PLAYTEST_PROP_SPIN_RATE_IS_DESIGNED, PLAYTEST_PROP_SPIN_SENSE_IS_DESIGNED,
        "two designed decisions, two claims"
    );

    // Engine off is zero, whatever the spool says.
    assert_eq!(propeller_spin_rate_rev_per_s(EngineState::STOPPED), 0.0);
    assert_eq!(
        propeller_spin_rate_rev_per_s(EngineState {
            running: false,
            spool: 1.0
        }),
        0.0
    );
    // Engine on: idle and full are the declared endpoints, both positive, and
    // the curve never runs backwards between them. The endpoints are read back
    // out of the production function — the one the spin system calls — so this
    // measures the curve rather than restating its literals.
    let idle = propeller_spin_rate_rev_per_s(EngineState::direct(0.0));
    let full = propeller_spin_rate_rev_per_s(EngineState::direct(1.0));
    assert_eq!(idle, PROP_SPIN_IDLE_REV_PER_S);
    assert_eq!(full, PROP_SPIN_FULL_REV_PER_S);
    assert!(idle > 0.0, "idle still turns the disc, got {idle} rev/s");
    assert!(
        full > 2.0 * idle,
        "full must be visibly faster than idle: {full} against {idle} rev/s"
    );
    // Well under half the frame rate: a faster disc would alias backwards on a
    // 60 fps frame instead of showing which way it turns.
    assert!(
        full * 2.0 <= cs_app::playtest::smoke::SMOKE_FRAME_HZ,
        "the full-throttle rate {full} rev/s must not alias on the playtest's own {} fps clock",
        cs_app::playtest::smoke::SMOKE_FRAME_HZ
    );

    let mut previous = 0.0;
    for step in 0..=100 {
        let spool = f64::from(step) / 100.0;
        let rate = propeller_spin_rate_rev_per_s(EngineState::direct(spool));
        assert!(rate.is_finite(), "a finite rate at spool {spool}");
        assert!(
            rate >= previous,
            "the curve never turns back: {previous} then {rate} at spool {spool}"
        );
        previous = rate;
    }
}

// ------------------------------------------------------ the drawn spin turn --

/// **The propeller's local rotation advances about the measured hub axis, and
/// a throttle change makes it visibly faster at full than at idle.**
///
/// This is the owner's observation made checkable: on `main` at `70fff350` the
/// drawn `staticprop1` never moved (`git grep PropellerSpin 70fff350` finds
/// nothing), so this test cannot even name its subject there.
///
/// The throttle moves through the production path the window uses — `4` and
/// `1` are the shipped default bindings, read by the F22 session and handed to
/// the flight model, whose **engine spool** (not the raw key) is what the spin
/// reads — so the disc spools up and down instead of jumping.
#[test]
fn accept_playtest_prop_spin_the_propeller_turns_faster_at_full_throttle_than_at_idle() {
    let hub = measured_hub();
    let mut app = headless_app();
    app.update();
    let propeller = attach_propeller(&mut app, &hub);
    assert_eq!(
        transform_of(&mut app, propeller),
        propeller_base(),
        "at zero revolutions the drawn placement is the spawn placement"
    );

    // One second at the spawn's cruise throttle: it turns, about the measured
    // hub, and a rim point travels while the hub does not.
    let rim = rim_point(&hub);
    let rim_before = transform_of(&mut app, propeller).transform_point(rim);
    run(&mut app, 60);
    let cruise_revs = revolutions(&mut app, propeller);
    assert!(
        cruise_revs > 0.0,
        "the disc must turn while the engine runs, got {cruise_revs} revolutions"
    );
    assert_turns_about_the_measured_hub(&mut app, propeller, &hub);
    let rim_after = transform_of(&mut app, propeller).transform_point(rim);
    assert!(
        (rim_after - rim_before).length() > 0.1 * hub.radius_m,
        "a point on the disc's rim must travel while the hub stays put: {:?} then {:?}",
        rim_before,
        rim_after,
    );

    // Full throttle, then idle, each settled before the window that measures
    // it: 3 s at a 1.5 /s throttle response puts the spool on its target.
    let measure = |app: &mut App| -> f32 {
        let before = revolutions(app, propeller);
        run(app, 60);
        revolutions(app, propeller) - before
    };

    tap(&mut app, KeyCode::Digit4);
    assert!((throttle(&app) - 1.0).abs() < 1.0e-6, "4 is full throttle");
    run(&mut app, 180);
    let full = measure(&mut app);

    tap(&mut app, KeyCode::Digit1);
    assert!((throttle(&app)).abs() < 1.0e-6, "1 is idle throttle");
    run(&mut app, 180);
    let idle = measure(&mut app);

    assert_turns_about_the_measured_hub(&mut app, propeller, &hub);
    assert!(
        full > idle,
        "full throttle must turn the disc faster than idle: {full} against {idle} revolutions \
         per second"
    );
    assert!(
        idle > 0.0,
        "an idling engine still turns the disc, got {idle}"
    );
    // The measured rates are the designed curve: 60 frames are one second of
    // the playtest's own 60 fps clock, so revolutions per second is the raw
    // count. 15 % covers the frame's nanosecond rounding, nothing else.
    assert!(
        (f64::from(full) - PROP_SPIN_FULL_REV_PER_S).abs() <= 0.15 * PROP_SPIN_FULL_REV_PER_S,
        "full throttle is the designed {PROP_SPIN_FULL_REV_PER_S} rev/s, got {full}"
    );
    assert!(
        (f64::from(idle) - PROP_SPIN_IDLE_REV_PER_S).abs() <= 0.15 * PROP_SPIN_IDLE_REV_PER_S,
        "idle is the designed {PROP_SPIN_IDLE_REV_PER_S} rev/s, got {idle}"
    );
    println!(
        "PLAYTEST-PROP-SPIN cruise={cruise_revs} full={full} idle={idle} rev/s (designed \
         {PROP_SPIN_IDLE_REV_PER_S}..{PROP_SPIN_FULL_REV_PER_S})"
    );
}

// ----------------------------------------------------- pause and engine off --

/// **A paused session freezes the disc where it is, and a stopped engine stops
/// it; resuming turns it again.**
///
/// Both are the production paths: the pause is the same `Esc` meta key the
/// window uses, read by the F22 session policy that also freezes the fixed
/// clock, and the stop is the flight model's own `EngineState`. The advance is
/// measured on the recorded revolutions of the spawned component, which is
/// what the drawn transform is derived from.
#[test]
fn accept_playtest_prop_spin_the_propeller_freezes_while_paused_and_with_the_engine_off() {
    let hub = measured_hub();
    let mut app = headless_app();
    app.update();
    let propeller = attach_propeller(&mut app, &hub);
    run(&mut app, 30);
    let turning = revolutions(&mut app, propeller);
    assert!(turning > 0.0, "the disc turns before the pause: {turning}");

    // Esc: the production pause. Nothing advances from the frame it begins.
    tap(&mut app, KeyCode::Escape);
    assert!(
        app.world().resource::<PlaytestState>().paused,
        "Esc pauses the session"
    );
    let paused_at = revolutions(&mut app, propeller);
    run(&mut app, 120);
    assert_eq!(
        revolutions(&mut app, propeller),
        paused_at,
        "a paused session advances the disc by nothing"
    );
    assert_turns_about_the_measured_hub(&mut app, propeller, &hub);

    // Esc again: it picks up where it stopped.
    tap(&mut app, KeyCode::Escape);
    assert!(
        !app.world().resource::<PlaytestState>().paused,
        "Esc resumes the session"
    );
    run(&mut app, 60);
    let resumed = revolutions(&mut app, propeller);
    assert!(
        resumed > paused_at,
        "resuming turns the disc again: {paused_at} then {resumed}"
    );

    // Engine off: the rate is zero, so the system writes nothing at all.
    engine_off(&mut app);
    let stopped_at = revolutions(&mut app, propeller);
    let stopped_transform = transform_of(&mut app, propeller);
    run(&mut app, 120);
    assert_eq!(
        revolutions(&mut app, propeller),
        stopped_at,
        "a stopped engine stops the disc"
    );
    assert_eq!(
        transform_of(&mut app, propeller),
        stopped_transform,
        "and leaves the drawn placement exactly where it was"
    );
}

// ------------------------------------------------------ the flight body pose --

/// **Two identical apps, one of them with the spin component: after a second
/// of flight their bodies are in the same place, so the spin system wrote the
/// propeller and nothing else.**
///
/// This is the acceptance criterion "the flight body's `Transform`/pose is
/// untouched by the system" as a measurement rather than a reading of the
/// source: a system that rotated the body (the naive implementation, and the
/// way the whole aircraft would tumble at 6 rev/s) leaves the two runs
/// visibly apart, while the propeller child of the spinning run has turned
/// and the plain child of the other has not moved at all.
#[test]
fn accept_playtest_prop_spin_the_flight_body_pose_is_untouched_by_the_spin_system() {
    let hub = measured_hub();
    let mut spinning = headless_app();
    let mut plain = headless_app();
    spinning.update();
    plain.update();
    let turning_propeller = attach_propeller(&mut spinning, &hub);
    let plain_child = attach_plain_child(&mut plain);
    let plain_transform = transform_of(&mut plain, plain_child);

    // The same frames, the same keys, in both apps: one second of cruise plus
    // one second at full throttle.
    tap(&mut spinning, KeyCode::Digit4);
    tap(&mut plain, KeyCode::Digit4);
    run(&mut spinning, 60);
    run(&mut plain, 60);

    let spun = revolutions(&mut spinning, turning_propeller);
    assert!(
        spun > 2.0,
        "the spinning app really turned its disc during the window: {spun} revolutions"
    );
    assert_eq!(
        transform_of(&mut plain, plain_child),
        plain_transform,
        "the same child without the component does not move: nothing else spins it"
    );

    let pose = |app: &mut App| -> ([f32; 3], [f32; 4], Transform) {
        let body = body_of(app);
        let position = app
            .world()
            .get::<Position>(body)
            .map(|pose| pose.0.to_array());
        let rotation = app
            .world()
            .get::<Rotation>(body)
            .map(|pose| pose.0.to_array());
        let (Some(position), Some(rotation)) = (position, rotation) else {
            panic!("the flight body carries the authoritative pose");
        };
        (
            position,
            rotation,
            *app.world()
                .get::<Transform>(body)
                .expect("the body has a transform"),
        )
    };

    let spun_pose = pose(&mut spinning);
    let plain_pose = pose(&mut plain);
    for component in 0..3 {
        assert!(
            (spun_pose.0[component] - plain_pose.0[component]).abs() < 1.0e-4,
            "the body's position is the physics path's own: {:?} against {:?}",
            spun_pose.0,
            plain_pose.0
        );
    }
    let spun_rotation = Quat::from_array(spun_pose.1);
    let plain_rotation = Quat::from_array(plain_pose.1);
    assert!(
        spun_rotation.angle_between(plain_rotation) < 1.0e-4,
        "the body's orientation is the physics path's own: {:?} against {:?}",
        spun_rotation,
        plain_rotation
    );
    let spun_transform_rotation = spun_pose.2.rotation;
    let plain_transform_rotation = plain_pose.2.rotation;
    assert!(
        (spun_pose.2.translation - plain_pose.2.translation).length() < 1.0e-4
            && spun_transform_rotation.angle_between(plain_transform_rotation) < 1.0e-4,
        "and so is its drawn transform: {:?} against {:?}",
        spun_pose.2,
        plain_pose.2
    );
    println!(
        "PLAYTEST-PROP-SPIN body position {:?} rotation {:?} unchanged by {} revolutions",
        spun_pose.0, spun_pose.1, spun
    );
}

// ------------------------------------------------------- the startup record --

/// **The startup `playtest sources` object records the spin rule, the rate
/// curve's claim and the propeller mesh that is shown.**
///
/// Everything the owner reads off the startup line and out of the smoke
/// `report.json` has to name the rule, say which value in it is designed and
/// by what claim, and name the mesh actually drawn — otherwise a reader of the
/// artifact cannot tell a spinning original propeller from this stage's own
/// development curve. Needs no installation: it builds the production
/// `RetailContent` the free-flight app holds and asks it for its manifest.
#[test]
fn accept_playtest_prop_spin_sources_line_records_the_rule_the_rate_claim_and_the_shown_mesh() {
    let hub = measured_hub();
    let content = RetailContent {
        installation: "fixture".to_owned(),
        containers: vec![(
            "zbd/planes.zbd".to_owned(),
            "0000000000000000000000000000000000000000000000000000000000000000".to_owned(),
        )],
        area: PlaytestAreaReport {
            world: WorldId::from_key("c1c").expect("c1c is a world id"),
            node_slot: 517,
            node_name: "piratezep".to_owned(),
            nodes: 793,
            mesh_records: 401,
            stored_bindings: 401,
            lod_distance_m: 300.0,
            hidden_lods: Vec::new(),
            undrawn: Vec::new(),
            triangles: 8_673,
            bounds: Aabb::try_new([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]).expect("a fixture box"),
            refused: Vec::new(),
            gaps: Vec::new(),
        },
        aircraft: PlaytestAircraftReport {
            container_key: "zbd/planes.zbd".to_owned(),
            root_name: "bloodhawk".to_owned(),
            intact_node_slot: 2_296,
            intact_node_name: "healthy".to_owned(),
            lod_distance_m: 20.0,
            selected_lod: Some((2_297, "nearest".to_owned())),
            lod_coverage: None,
            hidden_lods: Vec::new(),
            parts: Vec::new(),
            undrawn: vec![UndrawnBinding {
                node_slot: 2_542,
                node_name: "prop1".to_owned(),
                mesh_index: 9,
                reason: "a propeller state the original shows under conditions that are \
                         unmeasured"
                    .to_owned(),
            }],
            airframe_bindings: 17,
            extent_m: [2.1, 1.5, 10.2],
        },
        area_entities: Vec::new(),
        spawn_m: [-60.0, 200.0, 200.0],
        half_extents_m: [1.0, 1.0, 1.0],
        visual_rotation: Quat::IDENTITY,
        parts: Vec::new(),
        propeller: Some(PropellerSpinSpec {
            node_slot: PLAYTEST_AIRCRAFT_PROP_NODE_SLOT,
            node_name: PLAYTEST_AIRCRAFT_PROP_NODE_NAME.to_owned(),
            hub,
        }),
        textures: PlaytestTextureReport {
            claims: [
                "playtest-retail.texture-archive-tier-is-designed",
                "playtest-retail.texture-name-reading-is-designed",
                "playtest-retail.texture-material-is-designed",
                "playtest-textures.decal-offset-is-designed",
            ],
            archive: "ZBD/C1C/rtexture10.zbd".to_owned(),
            archive_sha256: "00".to_owned(),
            group: "c1c".to_owned(),
            selection: "the world group's highest-numbered tier".to_owned(),
            name_reading: "stored texture name up to the first dot, ASCII lower case",
            subjects: Vec::new(),
        },
    };
    let json = content.manifest_json();
    println!("playtest sources: {json}");

    // Machine-readable: the object stays balanced around the new field.
    let balanced =
        |open: char, close: char| json.matches(open).count() == json.matches(close).count();
    assert!(balanced('{', '}') && balanced('[', ']'), "{json}");

    let shown_mesh = format!("\"shown\":[\"{PLAYTEST_AIRCRAFT_PROP_NODE_NAME}\"]");
    let rate_claim = format!("\"rate_claim\":\"{PLAYTEST_PROP_SPIN_RATE_IS_DESIGNED}\"");
    let sense_claim = format!("\"sense_claim\":\"{PLAYTEST_PROP_SPIN_SENSE_IS_DESIGNED}\"");
    let rates = format!(
        "\"rate_rev_per_s\":{{\"idle\":{PROP_SPIN_IDLE_REV_PER_S:?},\
         \"full\":{PROP_SPIN_FULL_REV_PER_S:?}}}"
    );
    let checks: Vec<String> = vec![
        "\"propeller_spin\":{".to_owned(),
        shown_mesh,
        "\"mesh_rule\":\"staticprop1 only".to_owned(),
        "\"rule\":\"the drawn propeller disc turns about the hub axis".to_owned(),
        rate_claim,
        sense_claim,
        rates,
        "\"hub\":{\"measured\":\"area-weighted centroid".to_owned(),
        "\"radius_m\":".to_owned(),
        "\"thickness_m\":".to_owned(),
    ];
    for expected in checks {
        assert!(json.contains(&expected), "{expected} missing from {json}");
    }

    // The mesh of the record is the mesh the scene pins, and the hub beside it
    // is the measurement this test made from the disc's own triangles.
    assert!(
        json.contains(&format!(
            "\"pivot\":[{:?},{:?},{:?}]",
            hub.pivot[0], hub.pivot[1], hub.pivot[2]
        )),
        "the measured hub travels with the record: {json}"
    );

    // Nothing in the record may claim a measurement the original run never
    // produced: the rate and the sense both name their claim, and the label
    // the same line carries still says PROVISIONAL TUNING.
    assert!(json.contains("PROVISIONAL TUNING"), "{json}");
    assert!(
        !json.contains("verified_original") && !json.contains("\"verified\""),
        "a development spin is never a verified original behaviour: {json}"
    );
}

// ------------------------------------------------------------ the retail hub --

/// **The hub axis and pivot are measured from `staticprop1`'s own geometry,
/// are finite, and the pivot lies on the propeller disc.**
///
/// Read through the production readers (`read_playtest_sources` →
/// `aircraft_graph` → `stored_render_mesh`) and measured by the production
/// `measure_propeller_hub` — the retail test of the task's criterion 2, so the
/// numbers the playtest spins about come out of the owner's own bytes and
/// nowhere else. Nothing is committed: the container is read, measured and
/// dropped.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_prop_spin_hub_is_measured_from_the_drawn_propeller_disc() {
    let sources =
        read_playtest_sources(&install_dir(), PLAYTEST_WORLD_GROUP).expect("the containers read");
    let adapter = playtest_adapter().expect("the canonical source is declared");
    let graph = aircraft_graph(sources.aircraft(), &adapter).expect("the airframe graph is built");
    let airframe = graph
        .root(PLAYTEST_AIRCRAFT_ROOT_NAME)
        .expect("the bloodhawk root is a root of the container");
    let members = graph.subtree(airframe.id());
    let node = members
        .iter()
        .find(|node| node.index() == PLAYTEST_AIRCRAFT_PROP_NODE_SLOT)
        .expect("the pinned propeller node is in the airframe's subtree");
    assert_eq!(
        node.name(),
        PLAYTEST_AIRCRAFT_PROP_NODE_NAME,
        "the pinned slot holds the drawn propeller"
    );
    let binding = node.mesh().expect("the propeller node binds a mesh");
    let stored = sources
        .aircraft()
        .meshes()
        .get(binding.index)
        .expect("the propeller's mesh is stored");
    let render = stored_render_mesh(stored).expect("the propeller's mesh builds");

    let hub = measure_propeller_hub(&render).expect("the disc measures a hub");

    // Finite, unit, and aft of the measured nose.
    for value in hub.axis.iter().chain(hub.pivot.iter()) {
        assert!(
            value.is_finite(),
            "every measured component is finite: {hub:?}"
        );
    }
    let axis = Vec3::from(hub.axis);
    assert!(
        (axis.length() - 1.0).abs() < 1.0e-5,
        "the axis is a unit vector: {:?}",
        hub.axis
    );
    let aft = -Vec3::from(STORED_AIRCRAFT_NOSE_AXIS);
    assert!(
        axis.dot(aft) > 0.9,
        "the measured normal is oriented aft, by the designed sense: {axis:?}"
    );
    assert!(
        hub.radius_m.is_finite() && hub.radius_m > 0.0,
        "the disc has a radius: {hub:?}"
    );
    assert!(
        hub.thickness_m <= 0.25 * hub.radius_m,
        "a propeller disc is flat against its radius: {hub:?}"
    );

    // The pivot lies **on the disc**: within its own thickness of the plane it
    // lies in, and at the middle of its silhouette rather than off its edge.
    let pivot = Vec3::from(hub.pivot);
    let reference = if axis.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let u = axis.cross(reference).normalize();
    let v = axis.cross(u).normalize();
    let mut plane = 0.0f32;
    let mut bounds = [
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
    ];
    for vertex in render.vertices() {
        let offset = Vec3::from(vertex.position) - pivot;
        plane = plane.max(offset.dot(axis).abs());
        let across = offset.dot(u);
        let along = offset.dot(v);
        bounds[0] = bounds[0].min(across);
        bounds[1] = bounds[1].max(across);
        bounds[2] = bounds[2].min(along);
        bounds[3] = bounds[3].max(along);
    }
    assert!(
        plane <= hub.thickness_m + 1.0e-5,
        "the pivot sits in the disc's own plane: {plane} m against a thickness of {} m",
        hub.thickness_m
    );
    let silhouette_centre = Vec3::new(
        0.5 * (bounds[0] + bounds[1]),
        0.5 * (bounds[2] + bounds[3]),
        0.0,
    );
    assert!(
        silhouette_centre.length() <= 0.25 * hub.radius_m,
        "the pivot is at the disc's hub, not off its rim: {} m from the centre of a {} m disc",
        silhouette_centre.length(),
        hub.radius_m
    );

    // How it was measured, printed for the finding that records it.
    let composed = node.world_transform().apply([
        f64::from(hub.pivot[0]),
        f64::from(hub.pivot[1]),
        f64::from(hub.pivot[2]),
    ]);
    println!(
        "PLAYTEST-PROP-SPIN measured slot={} name={} triangles={} axis={:?} pivot={:?} \
         radius_m={} thickness_m={} composed_pivot={:?}",
        node.index(),
        node.name(),
        render.triangles().len(),
        hub.axis,
        hub.pivot,
        hub.radius_m,
        hub.thickness_m,
        composed,
    );

    // The direction it spins about is the fuselage's own length axis once the
    // node's composed transform carries it into the airframe — the disc is
    // perpendicular to the flight, not to something the read invented.
    let linear = node.world_transform().linear();
    let composed_axis = linear
        .iter()
        .map(|row| {
            row[0] * f64::from(hub.axis[0])
                + row[1] * f64::from(hub.axis[1])
                + row[2] * f64::from(hub.axis[2])
        })
        .collect::<Vec<_>>();
    let fuselage = Vec3::new(
        composed_axis[0] as f32,
        composed_axis[1] as f32,
        composed_axis[2] as f32,
    )
    .normalize();
    let forward = Vec3::new(
        BODY_FORWARD[0] as f32,
        BODY_FORWARD[1] as f32,
        BODY_FORWARD[2] as f32,
    );
    assert!(
        fuselage.dot(forward).abs() > 0.9,
        "the drawn disc turns about the body's length axis, got {fuselage:?} against {forward:?}"
    );
}

// ------------------------------------------------- the spawned, resetting one --

/// **The spawned retail aircraft carries exactly one propeller, built from the
/// measured hub, which spins with the engine and is still exactly one after
/// `R` reset.**
///
/// The wiring the owner plays with: `retail::install` → `spawn_aircraft` →
/// `spawn_retail_parts` attaches the component, the production flight path
/// turns it, and a reset despawns the body **with** its parts and spawns a
/// fresh one — so repeated resets can never stack a second spinning disc on
/// the aircraft, and the hub the scene uses is the one measured from the
/// container's own mesh.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_prop_spin_the_spawned_aircraft_spins_one_measured_propeller_across_reset() {
    // One discovery pass feeds both the scene and the independent measurement
    // it is compared against: production discovery hashes the whole
    // installation, so reading it twice would only slow the test down.
    let request = request();
    let sources = retail::read_sources(&request).expect("the installation reads");
    let expected = {
        let adapter = playtest_adapter().expect("the canonical source is declared");
        let graph =
            aircraft_graph(sources.aircraft(), &adapter).expect("the airframe graph is built");
        let airframe = graph
            .root(PLAYTEST_AIRCRAFT_ROOT_NAME)
            .expect("the bloodhawk root is a root of the container");
        let members = graph.subtree(airframe.id());
        let node = members
            .iter()
            .find(|node| node.index() == PLAYTEST_AIRCRAFT_PROP_NODE_SLOT)
            .expect("the pinned propeller node is drawn");
        let binding = node.mesh().expect("the propeller node binds a mesh");
        let stored = sources
            .aircraft()
            .meshes()
            .get(binding.index)
            .expect("the propeller's mesh is stored");
        measure_propeller_hub(&stored_render_mesh(stored).expect("the mesh builds"))
            .expect("the hub measures")
    };

    let mut app = cs_app::playtest::headless_app_with(|app| {
        retail::install(app, &sources, &request).expect("the original area installs")
    });
    // The playtest's Startup spawns the player aircraft over this content.
    app.update();

    let spec = app
        .world()
        .resource::<RetailContent>()
        .propeller
        .clone()
        .expect("the drawn propeller carries its measured hub");
    assert_eq!(spec.node_slot, PLAYTEST_AIRCRAFT_PROP_NODE_SLOT);
    assert_eq!(spec.node_name, PLAYTEST_AIRCRAFT_PROP_NODE_NAME);
    assert_eq!(
        spec.hub, expected,
        "the scene spins about the hub measured from the container's own mesh"
    );

    let propellers = spinners(&mut app);
    assert_eq!(
        propellers.len(),
        1,
        "one drawn propeller on the spawned aircraft"
    );
    let propeller = propellers[0];
    let body = body_of(&mut app);
    assert_eq!(
        app.world()
            .get::<ChildOf>(propeller)
            .map(|child| child.parent()),
        Some(body),
        "and it hangs off the flight body"
    );
    // Its measured axis, carried into the body's frame, is the length axis.
    let spin = spin_of(&mut app, propeller);
    let forward = Vec3::new(
        BODY_FORWARD[0] as f32,
        BODY_FORWARD[1] as f32,
        BODY_FORWARD[2] as f32,
    );
    assert!(
        spin.axis().dot(forward).abs() > 0.9,
        "the drawn disc turns about the body's length axis, got {:?}",
        spin.axis()
    );
    assert!(
        (spin.pivot()
            - transform_of(&mut app, propeller).transform_point(Vec3::from(spec.hub.pivot)))
        .length()
            < 1.0e-5,
        "the pivot the component holds is the measured hub under the drawn placement"
    );

    // It turns with the engine, about that hub.
    run(&mut app, 60);
    let before_reset = revolutions(&mut app, propeller);
    assert!(
        before_reset > 0.0,
        "the retail disc spins with the engine, got {before_reset} revolutions"
    );
    assert_turns_about_the_measured_hub(&mut app, propeller, &spec.hub);

    // `R` reset: one fresh propeller, still measured, still one.
    tap(&mut app, KeyCode::KeyR);
    run(&mut app, 4);
    assert_eq!(
        app.world().resource::<PlaytestState>().resets,
        1,
        "the reset ran"
    );
    let after_reset = spinners(&mut app);
    assert_eq!(
        after_reset.len(),
        1,
        "a reset leaves exactly one propeller entity, got {after_reset:?}"
    );
    assert_ne!(
        after_reset[0], propeller,
        "and it is the fresh body's own, not the despawned one"
    );
    let fresh = spin_of(&mut app, after_reset[0]);
    assert!(
        fresh.revolutions() < 1.0 && fresh.revolutions() < before_reset,
        "a fresh disc starts over rather than inheriting the despawned body's angle: {} against \
         {before_reset} revolutions",
        fresh.revolutions()
    );
    assert_eq!(
        fresh.pivot(),
        spin.pivot(),
        "and it is the same measured hub the container produced"
    );
    println!(
        "PLAYTEST-PROP-SPIN retail hub axis={:?} pivot={:?} radius_m={} revolutions_before_reset=\
         {before_reset} propellers_after_reset={}",
        spec.hub.axis,
        spec.hub.pivot,
        spec.hub.radius_m,
        after_reset.len(),
    );
}
