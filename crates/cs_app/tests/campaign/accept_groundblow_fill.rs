//! #2214 (`GROUNDBLOW-SWEEP-FILL`): the ground blow's world segment sweep
//! `0x4c8ec0` over the playtest's own colliders, and the probe it fills into
//! `OriginalInput::segment_probe` at the playtest's fixed-tick flight step.
//!
//! Task test prefix: `accept_groundblow_fill_`. The decode this implements is
//! `docs/findings/2026-10-29-groundblow-world-segment-sweep.md` (the sweep
//! finding) with the class-type measurement in
//! `docs/findings/2026-10-29-groundblow-class-type.md`; the seam it fills is
//! `docs/findings/2026-10-29-groundblow-probe-seam.md`.
//!
//! The no-retail tests build the colliders the sweep must answer for directly:
//! a water quad as two triangles and an aircraft body's box, and check the
//! record's own conventions — the **unnormalised** face normal (magnitude =
//! the face's parallelogram area), the chord `|point - from|`, the one-sided
//! face rule, the class-type filter and the owner resolution. The retail half
//! composes the real M01 (`CS_GAME_DIR`) and proves the production fill carries
//! the query's answer at the production step, and that handing it to the law
//! changes no trajectory.
use std::path::PathBuf;

use avian3d::prelude::{Collider, LinearVelocity, Position, Rotation};
use bevy::prelude::*;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{LaunchCinematic, build_headless, stage_for};
use cs_app::physics::BodyLayer;
use cs_app::playtest::scene::{PlaytestAircraft, PlaytestOriginalFlight, PlaytestSea};
use cs_app::playtest::world_sweep::{
    GroundBlowProbes, GroundBlowSegment, INTERSECTED_CLASS_TYPES, OBJECT3D_CLASS_TYPE,
    REFUSED_CLASS_TYPES, SweepColliders, SweepTarget, class_type_intersects, ground_blow_segment,
    nearest_contact, probe_in_world,
};
use cs_app::playtest::{OriginalFlightSet, to_quaternion};
use cs_sim::collision::CollisionLayer;
use cs_sim::flight::original::SegmentProbeOwner;

use crate::campaign::common::label;

/// The nose pointed straight down: the port's nose is `-Z` body, so a -90°
/// rotation about `X` takes it to `-Y`.
fn nose_down() -> Quat {
    Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
}

/// The nose pointed straight up.
fn nose_up() -> Quat {
    Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)
}

/// A 200 x 200 m horizontal water quad at `y = 0` as two triangles, wound so
/// each triangle's stored cross product is `+Y` with magnitude `40000` — the
/// face's parallelogram area. The four corners are `(-100, 0, -100)`,
/// `(-100, 0, 100)`, `(100, 0, 100)` and `(100, 0, -100)`.
fn sea_quad() -> Collider {
    let corners = [
        Vec3::new(-100.0, 0.0, -100.0),
        Vec3::new(-100.0, 0.0, 100.0),
        Vec3::new(100.0, 0.0, 100.0),
        Vec3::new(100.0, 0.0, -100.0),
    ];
    Collider::trimesh(corners.to_vec(), vec![[0, 1, 2], [0, 2, 3]])
}

/// The two-triangle quad's per-triangle unnormalised normal: `(0, 40000, 0)`.
const SEA_QUAD_NORMAL: [f64; 3] = [0.0, 40_000.0, 0.0];

fn close(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() <= 1e-3 * expected.abs().max(1.0)
}

/// The sweep's dispatch is the measured one: `1, 5, 6, 7, 8, 9, 10` intersect,
/// `2, 3, 4` are refused by name and everything outside `1..=10` is refused by
/// the guard (`0x4c9ac8`, table `0x4c9de0`).
#[test]
fn accept_groundblow_fill_class_type_filter_matches_the_measured_dispatch() {
    for class in 0..=12u32 {
        assert_eq!(
            class_type_intersects(class),
            INTERSECTED_CLASS_TYPES.contains(&class),
            "class {class}"
        );
    }
    for refused in REFUSED_CLASS_TYPES {
        assert!(!class_type_intersects(refused), "class {refused}");
    }
    assert!(class_type_intersects(OBJECT3D_CLASS_TYPE));
    assert_eq!(
        INTERSECTED_CLASS_TYPES.len() + REFUSED_CLASS_TYPES.len(),
        10
    );
    // Every M01 collider is object3d (#2213), so the filter removes nothing
    // from the port's population.
    assert_eq!(OBJECT3D_CLASS_TYPE, 5);
}

/// The record's normal is the hit triangle's **unnormalised** edge cross
/// product, and its distance is the chord from the body's position — not
/// parry's normalised normal and not the along-ray time of impact.
#[test]
fn accept_groundblow_fill_sweep_reports_the_unnormalised_triangle_normal() {
    let collider = sea_quad();
    let entity = Entity::from_raw_u32(7).expect("an entity id");
    let target = SweepTarget {
        entity,
        collider: &collider,
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        owner: SegmentProbeOwner::World,
        class_type: OBJECT3D_CLASS_TYPE,
    };
    // Nose down from 100 m over the quad, 400 m long.
    let segment = ground_blow_segment([0.0, 100.0, 0.0], orientation(nose_down()), 400.0);
    assert!(close(segment.length_m, 400.0));
    assert!(close(segment.axis[1], -1.0), "{:?}", segment.axis);

    let contact = nearest_contact(&segment, [target]).expect("the quad answers");
    assert_eq!(contact.entity, entity);
    assert_eq!(contact.owner, SegmentProbeOwner::World);
    // The point is the quad, 100 m below the start.
    assert!(close(contact.point_m[0], 0.0));
    assert!(close(contact.point_m[1], 0.0));
    assert!(close(contact.point_m[2], 0.0));
    assert!(close(contact.distance_m, 100.0), "{:?}", contact);
    // The normal is the face's own cross product: `|N| = 2 * triangle area`,
    // so a normalised `(0, 1, 0)` would be wrong by four orders of magnitude.
    assert_eq!(contact.normal, SEA_QUAD_NORMAL);
    let stored =
        (contact.normal[0].powi(2) + contact.normal[1].powi(2) + contact.normal[2].powi(2)).sqrt();
    assert!(close(stored, 40_000.0), "|N| = {stored}");
    // And the distance really is the chord of the point the record carries.
    let chord = [
        contact.point_m[0] - segment.from_m[0],
        contact.point_m[1] - segment.from_m[1],
        contact.point_m[2] - segment.from_m[2],
    ];
    assert!(close(
        contact.distance_m,
        chord[0].hypot(chord[1]).hypot(chord[2])
    ));
    // Nothing but the prober's own segment: an empty target list is no contact.
    assert_eq!(nearest_contact(&segment, []), None);
}

/// The measured face rule is one-sided (`0x55d6c0`: the segment's far end on
/// the `-N` side), and a class the dispatch refuses never answers.
#[test]
fn accept_groundblow_fill_sweep_rejects_a_back_face_and_a_refused_class() {
    let collider = sea_quad();
    let entity = Entity::from_raw_u32(9).expect("an entity id");
    let target = SweepTarget {
        entity,
        collider: &collider,
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        owner: SegmentProbeOwner::World,
        class_type: OBJECT3D_CLASS_TYPE,
    };
    // From below, the quad's stored winding faces away: `N` is `+Y`, the
    // segment's start is on the `-N` side, so the original reports no face.
    let from_below = ground_blow_segment([0.0, -100.0, 0.0], orientation(nose_up()), 400.0);
    assert_eq!(nearest_contact(&from_below, [target]), None);
    // The same segment from above does answer.
    let from_above = ground_blow_segment([0.0, 100.0, 0.0], orientation(nose_down()), 400.0);
    assert!(nearest_contact(&from_above, [target]).is_some());

    // A refused class type is skipped even when the segment hits the face.
    for class in REFUSED_CLASS_TYPES {
        let refused = SweepTarget {
            class_type: class,
            ..target
        };
        assert_eq!(
            nearest_contact(&from_above, [refused]),
            None,
            "class {class} must not answer"
        );
    }
}

/// An aircraft body answers as its box's face record (the engine's measured
/// box path: the face's outward normal scaled by the face's area) and resolves
/// to a vehicle; the prober's own collider is never tested.
#[test]
fn accept_groundblow_fill_sweep_resolves_a_box_to_its_vehicle_and_skips_the_prober() {
    // The playtest's declared aircraft box: 12 x 2 x 10 m.
    let collider = Collider::cuboid(12.0, 2.0, 10.0);
    let aircraft = Entity::from_raw_u32(11).expect("an entity id");
    // 200 m ahead of the probe's start, on the `Aircraft` layer with no
    // scenery marker.
    let owner =
        cs_app::playtest::world_sweep::owner_of(false, false, CollisionLayer::Aircraft, false);
    assert_eq!(owner, Some(SegmentProbeOwner::Vehicle { on_path: false }));
    let target = SweepTarget {
        entity: aircraft,
        collider: &collider,
        position: Vec3::new(0.0, 100.0, -200.0),
        rotation: Quat::IDENTITY,
        owner: owner.expect("an aircraft resolves"),
        class_type: OBJECT3D_CLASS_TYPE,
    };
    let segment = ground_blow_segment([0.0, 100.0, 0.0], orientation(Quat::IDENTITY), 400.0);
    let contact = nearest_contact(&segment, [target]).expect("the box answers");
    assert_eq!(contact.owner, SegmentProbeOwner::Vehicle { on_path: false });
    // The near face is at z = -195: the chord is 195 m.
    assert!(close(contact.point_m[2], -195.0), "{:?}", contact);
    assert!(close(contact.distance_m, 195.0), "{:?}", contact);
    // `0x55d270`'s box record: the `+Z` face's area (12 x 2) along outward `+Z`.
    assert!(close(contact.normal[2], 24.0), "{:?}", contact);
    assert!(close(contact.normal[0], 0.0));
    assert!(close(contact.normal[1], 0.0));

    // The prober's own geometry is excluded (`0x4cca30(node, 0)`).
    assert_eq!(
        cs_app::playtest::world_sweep::owner_of(false, false, CollisionLayer::Aircraft, true),
        None
    );
    // Anything that is neither scenery nor an aircraft body is not a world
    // node the port's sweep answers for.
    assert_eq!(
        cs_app::playtest::world_sweep::owner_of(false, false, CollisionLayer::Debris, false),
        None
    );
    assert_eq!(
        cs_app::playtest::world_sweep::owner_of(true, false, CollisionLayer::StaticWorld, true),
        None
    );
}

/// The production population is what the markers say: the sea and the scenery
/// answer as `World`, an `Aircraft` body as a vehicle, the prober never as its
/// own target, and the nearest of them wins.
#[test]
fn accept_groundblow_fill_population_is_the_marked_colliders() {
    let mut world = World::new();
    let prober = world.spawn_empty().id();
    let sea = world
        .spawn((
            sea_quad(),
            Position(Vec3::ZERO),
            Rotation(Quat::IDENTITY),
            BodyLayer(CollisionLayer::StaticWorld),
            PlaytestSea,
        ))
        .id();
    let aircraft = world
        .spawn((
            Collider::cuboid(12.0, 2.0, 10.0),
            Position(Vec3::new(0.0, 50.0, 0.0)),
            Rotation(Quat::IDENTITY),
            BodyLayer(CollisionLayer::Aircraft),
        ))
        .id();

    // Straight down from 100 m, clear of the aircraft's box in x: the sea
    // answers as the world.
    let dive = probe_in_world(
        &world,
        Vec3::new(50.0, 100.0, 0.0),
        nose_down(),
        400.0,
        prober,
    )
    .expect("the sea answers");
    assert_eq!(dive.owner, SegmentProbeOwner::World);
    assert!(close(dive.distance_m, 100.0), "{dive:?}");
    assert_eq!(dive.normal, SEA_QUAD_NORMAL);

    // From 300 m behind the aircraft along `-Z`, its box is the nearest
    // contact and resolves to a vehicle, at its `+Z` face (z = 5).
    let boxed = probe_in_world(
        &world,
        Vec3::new(0.0, 50.0, 300.0),
        Quat::IDENTITY,
        400.0,
        prober,
    )
    .expect("the aircraft box answers");
    assert_eq!(boxed.entity, aircraft);
    assert_eq!(boxed.owner, SegmentProbeOwner::Vehicle { on_path: false });
    assert!(close(boxed.point_m[2], 5.0), "{boxed:?}");
    assert!(close(boxed.distance_m, 295.0), "{boxed:?}");

    // A probing aircraft never hits its own box: from its own centre, nose
    // down, the sea beneath is what answers (the aircraft's own box starts at
    // y = 49 and the sea at y = 0, and the prober is excluded anyway).
    let own = probe_in_world(
        &world,
        Vec3::new(0.0, 50.0, 0.0),
        nose_down(),
        400.0,
        aircraft,
    )
    .expect("the sea still answers beneath it");
    assert_eq!(own.owner, SegmentProbeOwner::World);
    assert_ne!(own.entity, aircraft);
    let _ = sea;
}

/// A `cs_types` orientation from an Avian rotation, as the playtest's own
/// conversion states it.
fn orientation(rotation: Quat) -> cs_types::space::Quaternion {
    let [x, y, z, w] = rotation.normalize().to_array().map(f64::from);
    cs_types::space::Quaternion::try_new([x, y, z, w]).expect("a unit quaternion")
}

// ------------------------------------------------------------- retail M01 ---

/// Two retail tests here compose M01 at once and both stage the launch sound
/// container into one private cache path, which is not safe in parallel.
static RETAIL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn retail_lock() -> std::sync::MutexGuard<'static, ()> {
    RETAIL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// M01 composed headless, with the launch cinematic (4818 fixed ticks of held
/// control) removed so the law and the sweep fly from the first boundary.
fn compose() -> App {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("plan");
    let stage = stage_for(&root, &plan).expect("stage");
    let mut app = build_headless(&stage).expect("M01 composes");
    app.world_mut().remove_resource::<LaunchCinematic>();
    app
}

fn player(app: &mut App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, With<PlaytestAircraft>>()
        .single(app.world())
        .expect("the player aircraft")
}

fn elev(app: &App, player: Entity) -> f64 {
    app.world()
        .get::<PlaytestOriginalFlight>(player)
        .expect("the player flies the original law")
        .model
        .globals
        .groundblow_elev_m
}

/// A pose 150 m above M01's **mapped** flat sea: the centre of the first sea
/// collider whose own bounding box is flat in `y`. M01's hand-over pose sits
/// outside the world container's water tiles (recorded in the task's finding),
/// so a 400 m probe there meets nothing; this puts the aircraft over water the
/// world really carries.
fn above_the_flat_sea(app: &mut App) -> Vec3 {
    let colliders: Vec<Entity> = app
        .world_mut()
        .query_filtered::<Entity, With<PlaytestSea>>()
        .iter(app.world())
        .collect();
    for entity in colliders {
        let Some(collider) = app.world().get::<Collider>(entity) else {
            continue;
        };
        let aabb = collider.shape_scaled().compute_local_aabb();
        if (aabb.maxs.y - aabb.mins.y).abs() < 0.01 {
            let centre = (aabb.mins + aabb.maxs) * 0.5;
            return Vec3::new(centre.x, aabb.maxs.y + 150.0, centre.z);
        }
    }
    panic!("M01's world carries no flat sea collider");
}

/// Puts `player` at `position` flying `nose` (the unit nose direction) at
/// `speed`, as a test's controlled start.
fn place(app: &mut App, player: Entity, position: Vec3, nose: Vec3, speed: f32) {
    let rotation = Quat::from_rotation_arc(Vec3::NEG_Z, nose.normalize());
    let mut body = app.world_mut().entity_mut(player);
    *body.get_mut::<Position>().expect("a body pose") = Position(position);
    *body.get_mut::<Rotation>().expect("a body pose") = Rotation(rotation);
    *body.get_mut::<Transform>().expect("a body pose") =
        Transform::from_translation(position).with_rotation(rotation);
    if let Some(mut velocity) = body.get_mut::<LinearVelocity>() {
        velocity.0 = nose.normalize() * speed;
    }
}

fn pose(app: &App, player: Entity) -> (Vec3, Quat) {
    let position = app.world().get::<Position>(player).expect("a body pose").0;
    let rotation = app.world().get::<Rotation>(player).expect("a body pose").0;
    (position, rotation)
}

/// What the fill-versus-query check saw.
#[derive(Resource, Default)]
struct ProbeCheck {
    /// Ticks on which a player record's fill was compared with the query.
    compared: u64,
    /// Ticks on which that fill carried a hit.
    hits: u64,
    /// Any tick whose fill was not the query's own answer, by description.
    mismatches: Vec<String>,
}

/// Compares every tick's stored fill with the query's answer for the same
/// pre-step pose, in the same tick and before the driver spends it.
fn check_fill(
    colliders: SweepColliders,
    probes: Res<GroundBlowProbes>,
    records: Query<(Entity, &PlaytestOriginalFlight, &Position, &Rotation)>,
    mut check: ResMut<ProbeCheck>,
) {
    for (entity, flight, position, rotation) in &records {
        if !flight.is_player {
            continue;
        }
        let segment = ground_blow_segment(
            [
                f64::from(position.0.x),
                f64::from(position.0.y),
                f64::from(position.0.z),
            ],
            to_quaternion(rotation.0),
            flight.model.globals.groundblow_elev_m,
        );
        let expected = colliders
            .nearest(&segment, entity)
            .map(|contact| contact.hit());
        let filled = probes.hit(entity);
        check.compared += 1;
        if filled != expected {
            check
                .mismatches
                .push(format!("{filled:?} != {expected:?} at {position:?}"));
        }
        if filled.is_some() {
            check.hits += 1;
        }
    }
}

/// The production fill casts the player's probe from the tick's pre-step pose,
/// stores the query's own answer, and answers over M01's mapped sea.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_groundblow_fill_m01_fills_the_probe_from_the_production_world() {
    let _guard = retail_lock();
    let mut app = compose();
    app.init_resource::<ProbeCheck>();
    app.add_systems(
        FixedUpdate,
        check_fill
            .after(OriginalFlightSet::GroundBlowFill)
            .before(OriginalFlightSet::OriginalFlight),
    );
    app.update();
    let player = player(&mut app);
    let elevation = elev(&app, player);
    assert_eq!(elevation, 400.0, "the retail `groundblow_elev` is 400 m");
    assert_eq!(
        app.world().resource::<GroundBlowProbes>().cast_this_tick,
        1,
        "one player record casts one probe per fixed tick"
    );

    // Over M01's mapped flat sea, nose 30 degrees down: the probe meets water
    // within its own length, so the check below is not a run of `None`s.
    let above = above_the_flat_sea(&mut app);
    place(
        &mut app,
        player,
        above,
        Quat::from_rotation_x(-30_f32.to_radians()) * Vec3::NEG_Z,
        60.0,
    );
    for _ in 0..4 {
        app.update();
    }

    let check = app.world().resource::<ProbeCheck>();
    assert!(check.mismatches.is_empty(), "{:?}", check.mismatches);
    assert!(check.compared >= 5, "compared {} fills", check.compared);
    assert!(
        check.hits >= 2,
        "the sweep answered on only {} of {} fills",
        check.hits,
        check.compared
    );

    // The record's own unnormalised normal and chord distance, from the same
    // query over the same world.
    let (position, rotation) = pose(&app, player);
    let dive = probe_in_world(app.world(), position, rotation, elevation, player)
        .expect("the sea answers beneath the placed aircraft");
    assert_eq!(dive.owner, SegmentProbeOwner::World);
    assert!(
        (0.0..=elevation).contains(&dive.distance_m),
        "{dive:?} must lie on the 400 m segment"
    );
    let magnitude =
        (dive.normal[0].powi(2) + dive.normal[1].powi(2) + dive.normal[2].powi(2)).sqrt();
    assert!(magnitude > 1.0, "the normal is unnormalised: {magnitude}");
    let chord = [
        dive.point_m[0] - f64::from(position.x),
        dive.point_m[1] - f64::from(position.y),
        dive.point_m[2] - f64::from(position.z),
    ];
    assert!(close(
        dive.distance_m,
        chord[0].hypot(chord[1]).hypot(chord[2])
    ));
}

/// Withholds the fill's answer between the sweep and the law's driver: the one
/// difference between the two runs below.
fn withhold_groundblow_probes(mut probes: ResMut<GroundBlowProbes>) {
    probes.hits.clear();
}

/// M01 flies identically whether the driver hands the law the sweep's answer or
/// nothing at all: nothing reads `segment_probe` until #2107, and the fill on
/// the production step changes no tick of the trajectory.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_groundblow_fill_m01_trajectory_is_identical_with_the_fill_on() {
    let _guard = retail_lock();
    let mut filled = compose();
    let mut withheld = compose();
    withheld.add_systems(
        FixedUpdate,
        withhold_groundblow_probes
            .after(OriginalFlightSet::GroundBlowFill)
            .before(OriginalFlightSet::OriginalFlight),
    );
    filled.update();
    withheld.update();
    let a = player(&mut filled);
    let b = player(&mut withheld);
    assert!(filled.world().get::<PlaytestOriginalFlight>(a).is_some());
    assert!(withheld.world().get::<PlaytestOriginalFlight>(b).is_some());

    // Both aircraft start from the same pose over M01's mapped flat sea, nose
    // 30 degrees down at 60 m/s, so the 400 m probe meets water within its own
    // length on the first tick: the comparison must run over ticks where the
    // sweep really answers.
    let above = above_the_flat_sea(&mut filled);
    let nose = Quat::from_rotation_x(-30_f32.to_radians()) * Vec3::NEG_Z;
    place(&mut filled, a, above, nose, 60.0);
    place(&mut withheld, b, above, nose, 60.0);

    let mut answers = 0;
    for tick in 0..120 {
        filled.update();
        withheld.update();
        let pa = (
            filled.world().get::<Position>(a).expect("a body pose").0,
            filled.world().get::<Rotation>(a).expect("a body pose").0,
            filled
                .world()
                .get::<LinearVelocity>(a)
                .expect("a body pose")
                .0,
        );
        let pb = (
            withheld.world().get::<Position>(b).expect("a body pose").0,
            withheld.world().get::<Rotation>(b).expect("a body pose").0,
            withheld
                .world()
                .get::<LinearVelocity>(b)
                .expect("a body pose")
                .0,
        );
        assert_eq!(pa, pb, "tick {tick} differs with the fill on");
        if filled
            .world()
            .resource::<GroundBlowProbes>()
            .hit(a)
            .is_some()
        {
            answers += 1;
        }
    }
    assert!(
        answers >= 100,
        "the sweep answered on only {answers} of 120 ticks: the comparison is \
         vacuous without a hit"
    );
    // The withhold step ran and the fill still cast every tick: the runs differ
    // only in what the driver was handed.
    assert_eq!(
        filled.world().resource::<GroundBlowProbes>().cast_this_tick,
        withheld
            .world()
            .resource::<GroundBlowProbes>()
            .cast_this_tick
    );
}

/// The 2200-line marker keeps `GroundBlowSegment` in this file's imports honest:
/// a segment built with no length is no contact.
#[test]
fn accept_groundblow_fill_degenerate_segment_answers_nothing() {
    let collider = sea_quad();
    let entity = Entity::from_raw_u32(13).expect("an entity id");
    let target = SweepTarget {
        entity,
        collider: &collider,
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        owner: SegmentProbeOwner::World,
        class_type: OBJECT3D_CLASS_TYPE,
    };
    let degenerate = GroundBlowSegment {
        from_m: [0.0, 100.0, 0.0],
        to_m: [0.0, 100.0, 0.0],
        axis: [0.0, -1.0, 0.0],
        length_m: 0.0,
    };
    assert!(degenerate.is_degenerate());
    assert_eq!(nearest_contact(&degenerate, [target]), None);
}
