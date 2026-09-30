//! F18-B: world import and static collision generation.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-B`. Task test prefix: `accept_f18_b_`.
//!
//! These tests drive production code only. The world is
//! `cs_app::world::harbor_world`, the geometry is `cs_app::world::harbor_meshes`
//! (stored records → `RenderMesh` → the F17-B upload adapter → `WorldMeshes`),
//! the load is `cs_app::world::load_world`, and the collision is built by
//! `cs_app::world::spawn` on the real asset stack. No test carries its own world
//! builder, its own mesh or its own collision path.
//!
//! What is pinned here:
//!
//! * **one asset, one set of triangles.** A mesh object's collider is the
//!   `TrimeshFromMesh` Avian derives from the *same* `Mesh3d` handle the object
//!   draws, and it carries exactly the triangles the upload stored. The
//!   fixture's arch has a tunnel through it, so a substituted shape (a convex
//!   hull, a bounding box, a decimation) shows up in the triangle count and in
//!   the flight itself (F18 non-negotiable behavior 1).
//! * **each declared role decides what a mesh object gets**: `Solid` blocks,
//!   `Sensor` reports and never blocks, `None` draws its mesh and collides with
//!   nothing.
//! * **water is the authored patch.** A body inside the patch's footprint
//!   reaches it and the contact carries the water surface rule; a body flying
//!   the same line beside it reaches nothing (F18 non-negotiable behavior 2).
//! * **missing geometry is reported, never faked.** An object whose mesh
//!   reference this source does not hold is presented, reported as
//!   `SkipReason::MeshUnavailable`, and given no collider at all.

use avian3d::prelude::{
    Collider, CollisionLayers as AvianCollisionLayers, Position, RigidBodyColliders, Rotation,
};
use bevy::asset::Assets;
use bevy::mesh::{Mesh, Mesh3d, VertexAttributeValues};
use bevy::prelude::{App, Entity, Quat, Vec3, With};
use bevy::time::{Fixed, Time};
use cs_app::world::{
    HARBOR_HANGAR_HULL_TRIANGLES, HARBOR_HANGAR_TRIANGLES, HARBOR_OBJECT_ABSENT,
    HARBOR_OBJECT_BANNER, HARBOR_OBJECT_GROUND, HARBOR_OBJECT_HANGAR, HARBOR_OBJECT_SENSOR,
    HARBOR_OBJECT_WATER, HARBOR_SENSOR_HALF_M, HARBOR_SENSOR_POS_M, HARBOR_WATER_OFF_AXIS_Z_M,
    HARBOR_WATER_POS_M, MESH_SETTLE_UPDATES, ProbeSpec, SkipReason, SpawnedWorld, WorldContacts,
    WorldFixture, WorldVisual, fixture_provenance, harbor_meshes, harbor_world, load_world,
    mesh_reference, probe_layers, spawn_discrete_probe, spawn_swept_probe, static_world_layers,
    unload_world, world_app, world_instance,
};
use cs_content::world::{
    SurfaceRole, WorldCollisionRole, WorldCollisionShape, WorldDefinition, WorldInstance,
    WorldObjectId, WorldObjectInstance,
};
use cs_types::content::Resolved;

/// The probe's box half extents, in meters.
const PROBE_HALF_M: f64 = 0.25;

/// The speed the flight paths are measured at, in m/s.
///
/// At the workspace's fixed rate this is a quarter of a meter per tick, so a
/// tick is smaller than the arch's one-meter wall and a discrete overlap test is
/// enough to notice it. The measurement deliberately stays inside what the
/// pinned engine can detect against a triangle mesh: a body that outruns its own
/// sampling is measured separately, in
/// `accept_f18_b_a_tunnelling_body_misses_mesh_geometry_which_is_a_pinned_engine_limit`.
const PROBE_SPEED_M_S: f64 = 30.0;

/// Where a probe starts on the flight axis, in meters.
const PROBE_START_X_M: f64 = -12.0;

/// How far the arch's leg is thick along the flight axis, in meters. It is the
/// thickness a tick must stay under for discrete detection to be trustworthy.
const ARCH_WALL_THICKNESS_M: f64 = 1.0;

/// The flight height through the arch opening (`y ∈ (0, 3)`).
const ARCH_Y_M: f64 = 1.5;

/// How far the right leg sits from the opening's centreline in `z`.
const ARCH_LEG_Z_M: f64 = 1.5;

/// Ticks a probe needs to cross the whole fixture: 80 ticks is 20 m of travel,
/// past every geometry these flight paths meet.
const TICKS: u64 = 80;

/// A speed at which one tick outruns the arch's wall, in m/s, for the engine
/// limitation test.
const TUNNELLING_SPEED_M_S: f64 = 400.0;

/// Every object of the harbor world, in definition order: the population a
/// mission load record activates.
const HARBOR_POPULATION: [&str; 6] = [
    HARBOR_OBJECT_HANGAR,
    HARBOR_OBJECT_SENSOR,
    HARBOR_OBJECT_BANNER,
    HARBOR_OBJECT_WATER,
    HARBOR_OBJECT_GROUND,
    HARBOR_OBJECT_ABSENT,
];

/// The harbor world, built by production code.
fn harbor() -> WorldDefinition {
    harbor_world().expect("the synthetic harbor world is well formed")
}

/// A mission load record for `definition`: every authored object, no variant
/// named, nothing damaged.
fn mission(definition: &WorldDefinition) -> WorldInstance {
    world_instance(definition, None, &HARBOR_POPULATION, &[])
        .expect("the fixture load record is valid")
}

fn object(key: &str) -> WorldObjectId {
    WorldObjectId::new(key).expect("the fixture object key is valid")
}

/// A probe flying straight down the `+x` axis at `y`, `z`, at the measured
/// speed.
fn probe_at(y: f64, z: f64) -> ProbeSpec {
    probe_flying(y, z, PROBE_SPEED_M_S)
}

/// The same body at an explicit speed.
fn probe_flying(y: f64, z: f64, speed_m_s: f64) -> ProbeSpec {
    ProbeSpec {
        position_m: [PROBE_START_X_M, y, z],
        velocity_m_s: [speed_m_s, 0.0, 0.0],
        half_extents_m: [PROBE_HALF_M, PROBE_HALF_M, PROBE_HALF_M],
        mass_kg: 250.0,
    }
}

/// The same body, at the tunnelling speed, from the arch world's own start, so
/// the cuboid comparison runs on the same numbers F18-A measured.
fn probe_at_cuboid(z: f64) -> ProbeSpec {
    ProbeSpec {
        position_m: [-28.5, ARCH_Y_M, z],
        velocity_m_s: [TUNNELLING_SPEED_M_S, 0.0, 0.0],
        half_extents_m: [PROBE_HALF_M, PROBE_HALF_M, PROBE_HALF_M],
        mass_kg: 250.0,
    }
}

/// The headless world with the harbor world loaded, its mesh-derived colliders
/// derived, and the load's report.
fn loaded() -> (
    App,
    WorldDefinition,
    cs_app::world::WorldMeshes,
    SpawnedWorld,
) {
    let definition = harbor();
    let meshes = harbor_meshes();
    let mut app = world_app();
    let report = load_world(&mut app, &definition, &mission(&definition), &meshes)
        .expect("the harbor world loads");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }
    (app, definition, meshes, report)
}

/// Advances the world by exactly `ticks` fixed steps.
fn step(app: &mut App, ticks: u64) {
    for _ in 0..ticks {
        app.update();
    }
}

/// Every contact recorded so far.
fn contacts(app: &App) -> Vec<cs_app::world::WorldContact> {
    app.world().resource::<WorldContacts>().contacts().to_vec()
}

/// The triangle count of a derived collider's trimesh. `TriMesh::indices` yields
/// one `[u32; 3]` per triangle, so its length is already the count.
fn collider_triangles(collider: &Collider) -> usize {
    collider
        .shape()
        .as_trimesh()
        .expect("a mesh-derived collider is a triangle mesh, not a substitute primitive")
        .indices()
        .len()
}

/// The vertices of a derived collider's trimesh, as bit patterns.
fn collider_vertices(collider: &Collider) -> Vec<[u32; 3]> {
    collider
        .shape()
        .as_trimesh()
        .expect("a mesh-derived collider is a triangle mesh")
        .vertices()
        .iter()
        .map(|point| [point.x, point.y, point.z].map(f32::to_bits))
        .collect()
}

/// The positions one uploaded mesh holds, as bit patterns, so no comparison in
/// this file depends on float equality.
fn uploaded_positions(mesh: &cs_app::world::WorldMesh) -> Vec<[u32; 3]> {
    match mesh.mesh().attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(values)) => {
            values.iter().map(|value| value.map(f32::to_bits)).collect()
        }
        other => panic!("positions are Float32x3, got {other:?}"),
    }
}

/// **The provenance claim, as a fact about the engine's world:** the object
/// whose collision is derived from a mesh is presented and collided by *one*
/// entity holding one `Mesh3d` handle, that handle resolves to the upload the
/// record named, and the collider Avian derived from it carries exactly that
/// upload's triangles and vertices.
///
/// A convex hull of the hangar shell's 24 corners is a closed box of 12
/// triangles, so a substituted shape cannot match the authored 36 — and every
/// derived vertex is one of the uploaded positions, bit-exact, so a shape that
/// was re-centered, re-welded or regenerated fails too.
///
/// Observable failure if any of this is done twice or differently: a second
/// conversion of the mesh, a hull, a bounding box, or a placement that reads
/// from a different transform.
#[test]
fn accept_f18_b_a_mesh_collision_is_the_geometry_the_object_draws_and_keeps_its_opening() {
    let (mut app, definition, meshes, report) = loaded();

    let hangar = definition
        .object(&object(HARBOR_OBJECT_HANGAR))
        .expect("the fixture declares the hangar");
    assert_eq!(
        hangar.known_shape(),
        Some(WorldCollisionShape::FromMesh),
        "the fixture's damaged objective is a mesh-derived object, or this test \
         proves nothing about the mesh path"
    );
    let reference = hangar
        .mesh()
        .clone()
        .known()
        .expect("the hangar's mesh reference is known");
    let upload = meshes
        .get(&reference)
        .expect("the source holds the hangar's mesh");
    assert_eq!(
        upload.triangles(),
        HARBOR_HANGAR_TRIANGLES,
        "the fixture's shell really is three boxes' worth of triangles, so the \
         comparison below is against an authored number and not a tautology"
    );

    let spawned = report
        .object(&object(HARBOR_OBJECT_HANGAR))
        .expect("the report names the hangar");
    let mesh = spawned
        .mesh
        .as_ref()
        .expect("a mesh-derived object reports the mesh it was built from");
    assert_eq!(
        mesh.id, reference,
        "the collision must come from the very reference the record names"
    );
    assert_eq!(
        mesh.fingerprint,
        upload.fingerprint(),
        "the reported upload must be the one the source holds, by fingerprint"
    );
    assert_eq!(
        mesh.triangles,
        upload.triangles(),
        "the reported triangle count must be the upload's own"
    );

    // One entity, one handle: the same node presents the object and carries the
    // collider Avian derived from that handle.
    let collider = spawned
        .collider
        .as_ref()
        .expect("a FromMesh Solid object is collided");
    assert_eq!(
        collider.entity, spawned.visual,
        "a mesh object is presented and collided by one entity, so its \
         presentation and its collision cannot be moved apart"
    );
    assert_ne!(
        collider.body, collider.entity,
        "the derived collider hangs off its static body, which is a separate \
         entity the report must name"
    );
    let world = app.world_mut();
    assert!(world.get::<WorldVisual>(spawned.visual).is_some());
    assert_eq!(
        world
            .get::<RigidBodyColliders>(collider.body)
            .map(|colliders| colliders.len()),
        Some(1),
        "the body must own exactly the one derived collider"
    );

    let handle = world
        .get::<Mesh3d>(spawned.visual)
        .expect("the node presents the mesh the collider was derived from")
        .0
        .clone();
    let asset = world
        .resource::<Assets<Mesh>>()
        .get(&handle)
        .expect("the presented handle must resolve in the world's asset stack");
    assert_eq!(
        asset.count_vertices(),
        upload.mesh().count_vertices(),
        "the presented asset is the uploaded mesh, vertex for vertex"
    );

    let derived = world
        .get::<Collider>(collider.entity)
        .expect("Avian's hierarchy constructor must have derived a collider");
    assert_eq!(
        collider_triangles(derived),
        HARBOR_HANGAR_TRIANGLES,
        "the collider must carry the upload's triangles, not the {} of a convex \
         hull of the same corners: a hull seals the arch's opening",
        HARBOR_HANGAR_HULL_TRIANGLES
    );
    let allowed = uploaded_positions(upload);
    for vertex in collider_vertices(derived) {
        assert!(
            allowed.contains(&vertex),
            "collider vertex {vertex:?} is not one of the uploaded positions"
        );
    }
}

/// The same claim, **travelled**: a swept body passes through the opening the
/// stored mesh describes, and a second body is stopped by the leg the same mesh
/// describes. The opening is in the mesh, not in a subtraction the collision
/// builder performed, so this is what "never close a traversable opening through
/// convex-hull simplification" means for a world object.
///
/// The precondition is asserted, not assumed: a tick of this body must be
/// shorter than probe-plus-wall, or the leg half of the claim would be satisfied
/// by a body that simply stepped over the geometry.
///
/// Observable failure if the collision is not the authored geometry: the
/// through-body names a contact, or the leg-body sails past a sealed arch.
#[test]
fn accept_f18_b_a_swept_body_flies_through_the_mesh_opening_and_is_stopped_by_its_leg() {
    let (mut app, _, _, _) = loaded();
    let dt = app
        .world()
        .resource::<Time<Fixed>>()
        .timestep()
        .as_secs_f32();
    let step_m = PROBE_SPEED_M_S as f32 * dt;
    assert!(
        step_m < ARCH_WALL_THICKNESS_M as f32,
        "a tick of {step_m} m outruns the {ARCH_WALL_THICKNESS_M} m wall, so the \
         leg half of this test would pass on a body that never noticed the wall"
    );
    // Spawned after the settle, so the collider each body meets is the one the
    // record built rather than one that arrives after the body has passed it.
    let through =
        spawn_swept_probe(&mut app, &probe_at(ARCH_Y_M, 0.0)).expect("the probe spec is valid");
    let into_leg = spawn_swept_probe(&mut app, &probe_at(ARCH_Y_M, ARCH_LEG_Z_M))
        .expect("the probe spec is valid");
    let start = app
        .world()
        .get::<Position>(through)
        .expect("the probe was spawned")
        .0;
    let dt = app
        .world()
        .resource::<Time<Fixed>>()
        .timestep()
        .as_secs_f32();
    let free_end = start + Vec3::X * PROBE_SPEED_M_S as f32 * (TICKS as f32 * dt);

    step(&mut app, TICKS);

    let log = contacts(&app);
    assert!(
        !log.iter().any(|contact| contact.other == through),
        "the body on the opening's centreline must reach nothing: the opening \
         is in the stored mesh, and it met {:?}",
        log.iter()
            .map(|contact| (contact.object.as_str(), contact.other.index()))
            .collect::<Vec<_>>()
    );
    let through_end = app
        .world()
        .get::<Position>(through)
        .expect("the probe still exists")
        .0;
    let drift = (through_end - free_end).length();
    assert!(
        drift < 0.01,
        "a body that reached nothing must keep its velocity: drift {drift} m, \
         ended at {through_end:?} instead of {free_end:?}"
    );

    let hangar = object(HARBOR_OBJECT_HANGAR);
    let contact = log
        .iter()
        .find(|contact| contact.object == hangar)
        .expect("a swept body aimed at the leg must reach the leg");
    assert_eq!(contact.role, WorldCollisionRole::Solid);
    assert_eq!(contact.other, into_leg);
    let stopped = app
        .world()
        .get::<Position>(into_leg)
        .expect("the probe still exists")
        .0;
    assert!(
        stopped.x < 1.0,
        "the arch is 1 m thick, so a body aimed at the leg must be stopped near \
         x = 0; it ended at {stopped:?}"
    );
    assert!(
        stopped.x < free_end.x,
        "the body must have lost travel to the leg: {stopped:?} against {free_end:?}"
    );
}

/// **A pinned engine limitation, not a design choice.** On the pinned pair a body
/// whose tick outruns a mesh-derived wall passes through it, while the same body
/// is stopped by the cuboid wall of the arch world — and both halves are
/// measured here so the gap cannot go unnoticed.
///
/// The cause is in the pinned dependencies, not in this stage: Avian's swept CCD
/// asks parry for a shape cast
/// (`avian3d-0.7.0/src/dynamics/ccd/mod.rs::compute_ccd_toi`), and parry's
/// `DefaultQueryDispatcher::cast_shapes` has no `TriMesh` case, so a cast against
/// a `TrimeshFromMesh` collider returns `Unsupported` and no time of impact is
/// ever found (`parry3d-0.27.0/src/query/default_query_dispatcher.rs:437`). A
/// cuboid takes the support-map path, which is why F18-A's 400 m/s arch test
/// clamps at the wall.
///
/// Affected content: every world object whose collision is mesh-derived and thin
/// relative to one tick, at any body speed above the discrete sampling rate —
/// which is the whole of retail world geometry once F18-D imports it. F18-B does
/// not paper over it with invented geometry; the follow-up is filed as a task
/// and recorded in
/// `docs/findings/2026-09-30-f18-b-world-import-and-static-collision.md`.
///
/// Observable failure in the *good* direction: when the pinned engine learns to
/// cast against a triangle mesh this test fails, which is the moment the
/// limitation has to be re-measured and re-decided rather than assumed away.
#[test]
fn accept_f18_b_a_tunnelling_body_misses_mesh_geometry_which_is_a_pinned_engine_limit() {
    // The mesh world, at a speed where a tick outruns the wall.
    let (mut app, _, _, _) = loaded();
    let dt = app
        .world()
        .resource::<Time<Fixed>>()
        .timestep()
        .as_secs_f32();
    let step_m = TUNNELLING_SPEED_M_S as f32 * dt;
    assert!(
        step_m > ARCH_WALL_THICKNESS_M as f32 + PROBE_HALF_M as f32,
        "a tick of {step_m} m must outrun probe-plus-wall ({ARCH_WALL_THICKNESS_M} m \
         of arch plus {PROBE_HALF_M} m of probe) or this test measures nothing"
    );
    let fast = spawn_swept_probe(
        &mut app,
        &probe_flying(ARCH_Y_M, ARCH_LEG_Z_M, TUNNELLING_SPEED_M_S),
    )
    .expect("the probe spec is valid");
    step(&mut app, 20);
    let end = app
        .world()
        .get::<Position>(fast)
        .expect("the probe still exists")
        .0;
    assert!(
        end.x > 1.0,
        "the pinned engine does not stop a tunnelling body against a triangle \
         mesh; this body ended at {end:?}. When it is stopped, re-measure the \
         limitation in the F18-B finding before changing anything else"
    );
    assert!(
        !contacts(&app)
            .iter()
            .any(|contact| contact.object == object(HARBOR_OBJECT_HANGAR)),
        "and no contact is reported either: the body was never detected against \
         the wall it flew through"
    );

    // The cuboid path, at the same speed, is stopped — which is what makes the
    // difference a property of the collider's shape rather than of the fixture.
    let mut cuboid = WorldFixture::arch();
    let blocked = cuboid
        .spawn_swept_probe(probe_at_cuboid(ARCH_LEG_Z_M))
        .expect("the probe spec is valid");
    cuboid.step(20);
    let cuboid_end = cuboid
        .world()
        .get::<Position>(blocked)
        .expect("the probe still exists")
        .0;
    assert!(
        cuboid_end.x < 0.0,
        "the same speed against the arch world's cuboid leg is stopped, so the \
         measurement above is about the derived shape; it ended at {cuboid_end:?}"
    );
}

/// **The role contract on the mesh path:** `Solid` mesh geometry stops a body and
/// `Sensor` mesh geometry reports a body without stopping it.
///
/// The sensor half is measured with a **discrete** probe, because Avian's swept
/// CCD stops a body at a sensor volume's near face (task #401): the role's own
/// claim is "reports an overlap and never blocks motion".
///
/// Observable failure if the mesh path ignores the role or the Avian `Sensor`
/// marker: the trigger volume blocks a body that must cross it, or the hangar
/// lets a body through a leg it must stop.
#[test]
fn accept_f18_b_a_mesh_role_solid_stops_a_body_and_sensor_only_reports_one() {
    let (mut app, _, _, _) = loaded();
    let into_leg = spawn_swept_probe(&mut app, &probe_at(ARCH_Y_M, ARCH_LEG_Z_M))
        .expect("the probe spec is valid");
    let across_sensor = spawn_discrete_probe(
        &mut app,
        &probe_at(HARBOR_SENSOR_POS_M[1], HARBOR_SENSOR_POS_M[2]),
    )
    .expect("the probe spec is valid");
    let start = app
        .world()
        .get::<Position>(across_sensor)
        .expect("the probe was spawned")
        .0;
    let dt = app
        .world()
        .resource::<Time<Fixed>>()
        .timestep()
        .as_secs_f32();
    let free_end = start + Vec3::X * PROBE_SPEED_M_S as f32 * (TICKS as f32 * dt);

    step(&mut app, TICKS);

    let log = contacts(&app);
    let hangar = object(HARBOR_OBJECT_HANGAR);
    let solid = log
        .iter()
        .find(|contact| contact.object == hangar)
        .expect("a swept body aimed at the leg must reach the leg");
    assert_eq!(solid.role, WorldCollisionRole::Solid);
    assert_eq!(solid.other, into_leg);
    let stopped = app
        .world()
        .get::<Position>(into_leg)
        .expect("the probe still exists")
        .0;
    assert!(
        stopped.x < 1.0,
        "a solid mesh leg must stop the body at the arch, it ended at {stopped:?}"
    );

    let trigger = object(HARBOR_OBJECT_SENSOR);
    let reported = log
        .iter()
        .find(|contact| contact.object == trigger)
        .expect("a discrete body crossing the trigger volume must be reported");
    assert_eq!(reported.role, WorldCollisionRole::Sensor);
    assert_eq!(reported.other, across_sensor);
    let sensor_end = app
        .world()
        .get::<Position>(across_sensor)
        .expect("the discrete probe still exists")
        .0;
    let drift = (sensor_end - free_end).length();
    assert!(
        drift < 0.01,
        "a sensor volume must report and never block: drift {drift} m, ended at \
         {sensor_end:?} instead of {free_end:?}"
    );
    let volume_near_face = HARBOR_SENSOR_POS_M[0] - HARBOR_SENSOR_HALF_M[0];
    assert!(
        sensor_end.x as f64 > volume_near_face,
        "the body must be inside the volume that reported it, it ended at \
         {sensor_end:?} against a volume starting at {volume_near_face}"
    );
}

/// A role of `None` still draws its mesh and still collides with nothing:
/// geometry that is presented is not geometry a body can reach.
///
/// Observable failure if the mesh path gave a `None` object a collider: a body
/// flies through a banner the record never declared as blocking.
#[test]
fn accept_f18_b_a_non_colliding_object_draws_its_mesh_and_blocks_nothing() {
    let (mut app, _, _, report) = loaded();
    let banner = object(HARBOR_OBJECT_BANNER);
    let spawned = report
        .object(&banner)
        .expect("a role-`None` object is still reported as presented");
    assert_eq!(
        report.non_colliding(),
        vec![banner.clone()],
        "role `None` must be reported as a deliberate answer"
    );
    assert!(
        spawned.collider.is_none(),
        "role `None` must never produce a collider, however its geometry is built"
    );
    assert!(
        spawned.mesh.is_some(),
        "the banner still names the mesh it presents"
    );
    let world = app.world_mut();
    assert!(
        world.get::<Mesh3d>(spawned.visual).is_some(),
        "a role-`None` object presents the geometry its record names"
    );
    assert!(
        world.get::<Collider>(spawned.visual).is_none(),
        "a role-`None` object gets no collider at all"
    );
    assert!(
        world.get::<RigidBodyColliders>(spawned.visual).is_none(),
        "and no rigid body to hang one on"
    );
}

/// **Water is the authored patch, not a plane over low flight** (F18
/// non-negotiable behavior 2). One body flies the patch's own line and reaches
/// it; a second flies the same height twenty meters beside it and reaches
/// nothing.
///
/// Observable failure if a collision plane, a water rule that invents geometry
/// or an unbounded patch were involved: the off-axis body names a contact, or
/// the on-axis one never arrives.
#[test]
fn accept_f18_b_water_collision_is_the_authored_patch_and_not_a_plane_over_low_flight() {
    let (mut app, _, _, _) = loaded();
    let on_axis = spawn_swept_probe(
        &mut app,
        &probe_at(HARBOR_WATER_POS_M[1], HARBOR_WATER_POS_M[2]),
    )
    .expect("the probe spec is valid");
    let off_axis = spawn_swept_probe(
        &mut app,
        &probe_at(HARBOR_WATER_POS_M[1], HARBOR_WATER_OFF_AXIS_Z_M),
    )
    .expect("the probe spec is valid");

    step(&mut app, TICKS);

    let water = object(HARBOR_OBJECT_WATER);
    let log = contacts(&app);
    let hit = log
        .iter()
        .find(|contact| contact.object == water)
        .expect("a body flying the patch's own line must reach the patch");
    assert_eq!(hit.other, on_axis);
    assert_eq!(
        hit.surface.clone().known(),
        Some(SurfaceRole::Water),
        "the water contact must carry the water surface rule the record authored"
    );
    assert!(
        !log.iter()
            .any(|contact| contact.object == water && contact.other == off_axis),
        "twenty meters beside the patch there is no water to collide with"
    );
    let off_end = app
        .world()
        .get::<Position>(off_axis)
        .expect("the off-axis probe still exists")
        .0;
    assert!(
        off_end.x > HARBOR_WATER_POS_M[0] as f32 + 4.0,
        "the off-axis body must have flown the whole way past the patch's \
         lateral extent, it ended at {off_end:?}"
    );
}

/// A `FromMesh` object whose reference this source does not hold is presented
/// and **reported**, never given invented geometry.
///
/// Observable failure if a substitute were built — a box around the record, a
/// hull of nothing, a default cuboid — the object would acquire a collider and
/// the report would be empty; if it were dropped instead, the world would
/// quietly lack an object the definition declares.
#[test]
fn accept_f18_b_an_object_whose_mesh_is_missing_is_reported_and_never_faked() {
    let (mut app, definition, meshes, report) = loaded();
    let absent = object(HARBOR_OBJECT_ABSENT);
    let record = definition
        .object(&absent)
        .expect("the fixture declares the object");
    let reference = record
        .mesh()
        .clone()
        .known()
        .expect("the reference is known");
    assert!(
        meshes.get(&reference).is_none(),
        "the fixture must really not hold this mesh, or this test proves nothing"
    );

    let spawned = report
        .object(&absent)
        .expect("the object is still reported as presented");
    assert_eq!(
        spawned.skipped,
        Some(SkipReason::MeshUnavailable),
        "a solid object with no geometry must be reported by that reason"
    );
    assert!(
        spawned.collider.is_none(),
        "no collider may be invented for it"
    );
    assert!(
        spawned.mesh.is_none(),
        "no upload was used, so none is reported"
    );
    let world = app.world_mut();
    assert!(
        world.get::<Collider>(spawned.visual).is_none(),
        "the presented entity must carry no collider"
    );
    assert!(
        report
            .skipped()
            .iter()
            .any(|skip| skip.object == absent && skip.reason == SkipReason::MeshUnavailable),
        "the report must name the gap, saw {:?}",
        report.skipped()
    );
    assert!(
        report.collider_for(&object(HARBOR_OBJECT_HANGAR)).is_some(),
        "one missing upload must not take the other objects' collision with it"
    );

    // The two ways a `FromMesh` object can have no upload are different facts
    // and must be reported differently: the record names a mesh this source does
    // not hold (a load gap), versus the evidence never named a mesh at all (a
    // content gap, the same class as an unknown role or shape).
    let definition = harbor();
    let unevidenced = WorldObjectInstance::try_new(
        WorldObjectId::new("sign.unevidenced_mesh").expect("the key is valid"),
        Resolved::Unknown {
            claim_id: cs_types::evidence::ClaimId::new("mesh.unmeasured")
                .expect("the claim id is valid"),
            reason: "no evidence has named which mesh this instance uses".to_owned(),
        },
        cs_content::scene::CanonicalTransform::IDENTITY,
        Resolved::Known(cs_types::content::Known::new(
            cs_content::world::WorldCollisionRole::Solid,
            fixture_provenance("role.solid"),
        )),
        Resolved::Known(cs_types::content::Known::new(
            WorldCollisionShape::FromMesh,
            fixture_provenance("shape.from_mesh"),
        )),
        Resolved::Known(cs_types::content::Known::new(
            SurfaceRole::Ground,
            fixture_provenance("surface.ground"),
        )),
        Vec::new(),
        fixture_provenance("sign.record"),
    )
    .expect("no duplicate sectors");
    let with_unknown_mesh = WorldDefinition::try_new(
        definition.id().clone(),
        definition.origin().clone(),
        definition.boundary().clone(),
        definition.sectors().to_vec(),
        definition
            .objects()
            .iter()
            .cloned()
            .chain(std::iter::once(unevidenced))
            .collect(),
        fixture_provenance("harbor_plus_sign"),
    )
    .expect("the added object is structurally valid");
    let mut population: Vec<&str> = HARBOR_POPULATION.to_vec();
    population.push("sign.unevidenced_mesh");
    let instance = world_instance(&with_unknown_mesh, None, &population, &[])
        .expect("the load record is valid");
    let mut app = world_app();
    let report = load_world(&mut app, &with_unknown_mesh, &instance, &harbor_meshes())
        .expect("the world with an unevidenced mesh reference still loads");
    let sign = report
        .object(&object("sign.unevidenced_mesh"))
        .expect("the sign is reported as presented");
    assert_eq!(
        sign.skipped,
        Some(SkipReason::UnknownMesh),
        "an unevidenced mesh reference is a content gap and must not be reported \
         as a missing upload: no evidence named a mesh at all"
    );
    assert_eq!(
        report.skipped_count(),
        2,
        "the two gaps are distinct records, not one reason used twice: {:?}",
        report.skipped()
    );
}

/// **A missing upload is a gap whether or not the object collides.** A record
/// whose role is `None` asked for no collider, so nothing was *skipped* — but the
/// geometry it would have drawn is still the geometry its own record names, and a
/// banner that draws nothing must be named as a gap rather than presented as a
/// world that is complete.
///
/// This is the one report path the missing-mesh test above cannot reach, because
/// every object it exercises asks for collision.
///
/// Observable failure if the presentation half were ignored: the banner would be
/// presented as a bare marker with no `Mesh3d`, `skipped()` would stay empty, and
/// a consumer checking `skipped()` alone would draw a world with a hole in it.
#[test]
fn accept_f18_b_a_non_colliding_object_with_no_geometry_is_reported_as_a_presentation_gap() {
    let base = harbor();
    let banner = WorldObjectInstance::try_new(
        WorldObjectId::new("banner.absent_mesh").expect("the key is valid"),
        mesh_reference("banner.absent_mesh"),
        cs_content::scene::CanonicalTransform::IDENTITY,
        Resolved::Known(cs_types::content::Known::new(
            WorldCollisionRole::None,
            fixture_provenance("role.none"),
        )),
        Resolved::Known(cs_types::content::Known::new(
            WorldCollisionShape::FromMesh,
            fixture_provenance("shape.from_mesh"),
        )),
        Resolved::Known(cs_types::content::Known::new(
            SurfaceRole::Ground,
            fixture_provenance("surface.ground"),
        )),
        Vec::new(),
        fixture_provenance("banner_absent.record"),
    )
    .expect("no duplicate sectors");
    let definition = WorldDefinition::try_new(
        base.id().clone(),
        base.origin().clone(),
        base.boundary().clone(),
        base.sectors().to_vec(),
        base.objects()
            .iter()
            .cloned()
            .chain(std::iter::once(banner))
            .collect(),
        fixture_provenance("harbor_plus_absent_banner"),
    )
    .expect("the added object is structurally valid");
    let mut population: Vec<&str> = HARBOR_POPULATION.to_vec();
    population.push("banner.absent_mesh");
    let instance =
        world_instance(&definition, None, &population, &[]).expect("the load record is valid");
    let meshes = harbor_meshes();
    let reference = definition
        .object(&object("banner.absent_mesh"))
        .expect("the fixture declares the object")
        .mesh()
        .clone()
        .known()
        .expect("the mesh reference is known");
    assert!(
        meshes.get(&reference).is_none(),
        "the source must really not hold this mesh, or this test proves nothing"
    );

    let mut app = world_app();
    let report =
        load_world(&mut app, &definition, &instance, &meshes).expect("the world still loads");
    let spawned = report
        .object(&object("banner.absent_mesh"))
        .expect("the object is reported as presented");
    assert_eq!(
        spawned.skipped, None,
        "role `None` declined no collision, so nothing was skipped — reporting a \
         skip here would claim a collider the record never asked for"
    );
    assert_eq!(
        spawned.presentation_gap,
        Some(SkipReason::MeshUnavailable),
        "but the geometry it would have drawn is missing, and that is reported"
    );
    assert_eq!(
        report.presentation_gap_count(),
        1,
        "exactly one object presents nothing: {:?}",
        report.presentation_gaps()
    );
    assert_eq!(
        report.presentation_gaps()[0].object,
        object("banner.absent_mesh"),
        "and it is named by its own authored id"
    );
    assert!(
        app.world().get::<Mesh3d>(spawned.visual).is_none(),
        "there is no geometry to draw, which is why the gap is reported"
    );
    assert!(
        app.world().get::<Collider>(spawned.visual).is_none(),
        "and still nothing a body can reach, whatever the record's role was"
    );
    assert_eq!(
        report.skipped_count(),
        1,
        "the colliding object's own gap is still reported separately: {:?}",
        report.skipped()
    );
}

/// A contact names the gameplay surface rule its object was authored with, so a
/// consumer can tell a water hit from a landing without re-reading the world
/// record.
///
/// Observable failure if the surface were dropped on the way to the contact:
/// every contact would report the same rule.
#[test]
fn accept_f18_b_a_contact_names_the_surface_rule_its_object_was_authored_with() {
    let (mut app, _, _, _) = loaded();
    spawn_swept_probe(
        &mut app,
        &probe_at(HARBOR_WATER_POS_M[1], HARBOR_WATER_POS_M[2]),
    )
    .expect("the probe spec is valid");
    spawn_swept_probe(&mut app, &probe_at(ARCH_Y_M, ARCH_LEG_Z_M))
        .expect("the probe spec is valid");
    step(&mut app, TICKS);

    let log = contacts(&app);
    let water = log
        .iter()
        .find(|contact| contact.object == object(HARBOR_OBJECT_WATER))
        .expect("the water patch was reached");
    let ground = log
        .iter()
        .find(|contact| contact.object == object(HARBOR_OBJECT_HANGAR))
        .expect("the hangar's leg was reached");
    assert_eq!(
        water.surface.clone().known(),
        Some(SurfaceRole::Water),
        "the water patch reports the water rule"
    );
    assert_eq!(
        ground.surface.clone().known(),
        Some(SurfaceRole::Ground),
        "the hangar's leg reports the ground rule, not the water one"
    );
    assert!(
        water.sectors.is_empty(),
        "the water patch names no sector, so a contact must not invent one"
    );
    assert!(
        !ground.sectors.is_empty(),
        "the hangar does name its sector, and the contact says which"
    );
}

/// Every world collider — the three mesh-derived ones and the cuboid — carries
/// the static-world layer set, and the probe carries the aircraft one, so the
/// two interact: the same designed matrix on both collision paths.
///
/// The count is checked *by name*, because a count alone cannot tell which
/// object a collider belongs to: the harbor world builds colliders for the
/// hangar, the trigger volume, the water patch and the ground slab, and none for
/// the banner (role `None`) or the object whose mesh nobody supplied.
///
/// Observable failure if a world collider were built with another membership: the
/// layers stop matching the designed sets, or the two sides stop interacting and
/// every contact in this file goes quiet.
#[test]
fn accept_f18_b_every_mesh_collider_carries_the_designed_static_world_layers() {
    let (mut app, _, _, _) = loaded();
    let expected = static_world_layers();
    let mut query = app
        .world_mut()
        .query_filtered::<(Entity, &AvianCollisionLayers), With<cs_app::world::WorldColliderInstance>>();
    let colliders: Vec<(Entity, AvianCollisionLayers)> =
        query.iter(app.world()).map(|(e, l)| (e, *l)).collect();
    let mut owners: Vec<WorldObjectId> = colliders
        .iter()
        .map(|(entity, _)| {
            app.world()
                .get::<cs_app::world::WorldObjectBinding>(*entity)
                .expect("every world collider entity carries its object's binding")
                .object()
                .clone()
        })
        .collect();
    owners.sort();
    let mut expected_owners: Vec<WorldObjectId> = [
        HARBOR_OBJECT_HANGAR,
        HARBOR_OBJECT_SENSOR,
        HARBOR_OBJECT_WATER,
        HARBOR_OBJECT_GROUND,
    ]
    .iter()
    .map(|key| object(key))
    .collect();
    expected_owners.sort();
    assert_eq!(
        owners, expected_owners,
        "the harbor world collides the hangar, the trigger volume, the water \
         patch and the cuboid ground slab — and nothing else: role `None` draws \
         without a collider, and an object with no geometry gets none"
    );
    for (_, layer) in &colliders {
        assert_eq!(
            *layer, expected,
            "a world collider must carry the static-world set"
        );
    }
    assert!(
        expected.interacts_with(probe_layers()),
        "the designed sets must interact, or no contact in this file happens"
    );
}

/// A mesh object's placement is the authored one: the body that owns the derived
/// collider sits where the record's canonical matrix puts it, with the rotation
/// that matrix carries.
///
/// Observable failure if the mesh path read the transform a second time or
/// ignored it: the body lands somewhere else, or it carries a rotation the
/// record never authored.
#[test]
fn accept_f18_b_a_mesh_object_lands_where_its_record_puts_it() {
    let (app, definition, _, _) = loaded();
    let hangar = definition
        .object(&object(HARBOR_OBJECT_HANGAR))
        .expect("the fixture declares the hangar");
    let spawned = cs_app::world::residency(app.world())
        .expect("the harbor world is resident")
        .resident()
        .object(&object(HARBOR_OBJECT_HANGAR))
        .expect("the hangar is present")
        .clone();
    let body = spawned
        .collider
        .as_ref()
        .expect("the hangar is collided")
        .body;

    let authored = cs_app::world::canonical_matrix(hangar.transform());
    let position = app
        .world()
        .get::<Position>(body)
        .expect("the body carries a position")
        .0;
    for (axis, expected) in hangar.transform().translation().iter().enumerate() {
        assert!(
            (position[axis] as f64 - expected).abs() < 1e-4,
            "axis {axis}: the body is at {} but the record says {expected}",
            position[axis]
        );
    }
    let rotation = app
        .world()
        .get::<Rotation>(body)
        .expect("the body carries a rotation")
        .0;
    let expected = Quat::from_mat4(&authored);
    assert!(
        rotation.abs_diff_eq(expected, 1e-4),
        "the body's rotation is {rotation:?} but the record's matrix says {expected:?}"
    );
    assert_eq!(
        rotation.abs_diff_eq(Quat::IDENTITY, 1e-4),
        hangar.transform().linear() == [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        "this fixture authors no rotation, so the two must agree on identity"
    );
    assert!(
        !app.world().get::<Collider>(body).is_some(),
        "the collider is on the node, not the body: an unattached collider \
         collides with nothing"
    );
}

/// A mesh object's condition is stamped on every entity it owns, so a query
/// over the binding reads the same value the load record holds.
///
/// Observable failure if only one of the object's entities carried it (or the
/// wrong one): a query would see `Authored` for an object the record holds as
/// `Damaged`.
#[test]
fn accept_f18_b_every_entity_of_a_mesh_object_carries_its_condition() {
    let definition = harbor();
    let meshes = harbor_meshes();
    let instance = world_instance(
        &definition,
        Some("synthetic.harbor_world.mission_07"),
        &HARBOR_POPULATION,
        &[HARBOR_OBJECT_HANGAR],
    )
    .expect("the fixture load record is valid");
    let mut app = world_app();
    let report =
        load_world(&mut app, &definition, &instance, &meshes).expect("the harbor world loads");

    let hangar = object(HARBOR_OBJECT_HANGAR);
    let spawned = report.object(&hangar).expect("the hangar is reported");
    assert!(
        spawned.entities().len() >= 2,
        "a mesh object owns at least a body and a node, saw {:?}",
        spawned.entities()
    );
    let world = app.world_mut();
    for entity in spawned.entities() {
        let condition = world
            .get::<cs_app::world::ObjectCondition>(entity)
            .unwrap_or_else(|| panic!("entity {entity} carries no condition"));
        assert_eq!(
            condition.condition(),
            cs_content::world::WorldObjectCondition::Damaged,
            "the load authors the hangar damaged, so every one of its entities \
             must say so"
        );
    }
    assert_eq!(
        cs_app::world::condition_of(world, &hangar),
        Some(cs_content::world::WorldObjectCondition::Damaged),
        "and the record itself agrees"
    );
}

/// The load's report is a *record*, not a live view: it says what the load did
/// — every activated object, in definition order, and the gaps among them — and
/// it survives the world it came from.
///
/// This is what makes a gap nameable after the fact: the report is what a
/// consumer inspects before it starts a mission, so a world must not be
/// presented as complete while one of its objects is uncollidable.
///
/// Observable failure if the report were a live view or a partial walk: the
/// object list would shrink as sectors unloaded, or the gap would disappear.
#[test]
fn accept_f18_b_the_load_report_records_every_object_it_walked() {
    let (mut app, definition, _, report) = loaded();
    assert_eq!(
        report.world(),
        Some(definition.id()),
        "the report names the world it was built from"
    );
    let activated: Vec<&WorldObjectId> = definition
        .objects()
        .iter()
        .map(|object| object.id())
        .collect();
    let reported: Vec<&WorldObjectId> = report.objects().iter().map(|o| &o.object).collect();
    assert_eq!(
        reported, activated,
        "every activated object is reported, in definition order, whether it \
         collided or not"
    );
    assert_eq!(
        report.skipped_count(),
        1,
        "exactly the object with no mesh is a gap, saw {:?}",
        report.skipped()
    );

    // The report is a value, not a borrow of the world: it still describes the
    // load after the world is gone.
    let cloned = report.clone();
    unload_world(&mut app).expect("the harbor world is resident");
    assert_eq!(
        cloned.visuals().len(),
        activated.len(),
        "a report outlives the world it describes, or it cannot be inspected \
         after the fact"
    );
    assert_eq!(cloned.skipped_count(), 1);
}
