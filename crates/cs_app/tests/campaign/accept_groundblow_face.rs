//! #2217 (`GROUNDBLOW-POLYGON-FACE`): the ground blow's face record reads the
//! **stored polygon** a hit collision triangle stands in for — its corners in
//! stored order and its stored flag word's two-sided bit — beside the solver's
//! unchanged triangle buffer.
//!
//! Task test prefix: `accept_groundblow_face_`. The decode this implements is
//! the soup path `0x55c9c0` into the shared face test `0x55d6c0`
//! (`docs/findings/2026-10-29-groundblow-normal-scale.md`): the record's
//! normal is the stored face's first-three-corner cross product — a stored
//! quad's parallelogram, not twice an ear-clipped triangle's area — and the
//! polygon's stored two-sided bit overrides the one-sided accept gate.
//!
//! The no-retail tests drive the whole provenance chain on a synthetic stored
//! mesh — one four-corner outline polygon that ear-clips to two triangles:
//! [`CollisionMesh::of`] records the face beside the triangles,
//! [`ColliderMesh::of`] canonicalises it and [`ColliderFaces`] carries it on
//! the entity the sweep tests. The ignored retail test composes the real M01
//! (`CS_GAME_DIR`) and reads the same records off water tile `g28530`
//! (mesh 371, the tile `docs/findings/2026-10-29-groundblow-normal-scale.md`
//! measured): a `world1` direct child, spawned as a world-object collider
//! (`node-547`), whose `ColliderFaces` the composed world carries.
use std::path::PathBuf;

use avian3d::prelude::{Collider, Position, Rotation};
use bevy::prelude::*;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{LaunchCinematic, build_headless, stage_for, teardown};
use cs_app::physics::BodyLayer;
use cs_app::playtest::scene::PlaytestSea;
use cs_app::playtest::world_sweep::{
    OBJECT3D_CLASS_TYPE, SweepTarget, ground_blow_segment, nearest_contact, probe_in_world,
};
use cs_app::render::bevy_mesh::{ColliderFaces, ColliderMesh};
use cs_content::mesh::CollisionMesh;
use cs_formats::gamez::{FLAG_TWO_SIDED, PrimitiveKind, RawCorner, RawMesh, RawPolygon};
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

/// A `cs_types` orientation from an Avian rotation, as the playtest's own
/// conversion states it.
fn orientation(rotation: Quat) -> cs_types::space::Quaternion {
    let [x, y, z, w] = rotation.normalize().to_array().map(f64::from);
    cs_types::space::Quaternion::try_new([x, y, z, w]).expect("a unit quaternion")
}

fn close(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() <= 1e-3 * expected.abs().max(1.0)
}

/// The synthetic stored mesh: one four-corner `Polygon` outline flat at
/// `y = 0`, wound so its stored face normal is `+Y`. It is deliberately **not
/// a rectangle** — corner 3 is pulled forward — so the stored face's
/// first-three-corner parallelogram (`|N| = 20`) differs from the ear-clipped
/// halves' `2 * area` (the fan's two triangles report `20` and `25`), and a
/// hit inside the `(1,2,3)` half reads `25` under the old triangle soup.
fn stored_quad(two_sided: bool) -> (ColliderMesh, ColliderFaces, Collider) {
    let mesh = RawMesh {
        positions: vec![
            [0.0, 0.0, 0.0],
            [0.0, 0.0, -4.0],
            [-5.0, 0.0, -4.0],
            [-5.0, 0.0, 1.0],
        ],
        normals: Vec::new(),
        polygons: vec![RawPolygon {
            kind: PrimitiveKind::Polygon,
            raw_flags: if two_sided { FLAG_TWO_SIDED } else { 0 },
            material: 0,
            corners: (0u32..4)
                .map(|position| RawCorner {
                    position,
                    normal: None,
                    uv: None,
                    color: None,
                })
                .collect(),
        }],
    };
    let collision = ColliderMesh::of(&CollisionMesh::of(&mesh));
    let faces = ColliderFaces::of(&collision);
    // The same buffers `world::spawn`'s mesh collider is built from — the
    // solver's triangle soup, unchanged by the side table.
    let collider = Collider::trimesh(
        collision
            .vertices()
            .iter()
            .copied()
            .map(Vec3::from)
            .collect(),
        collision.indices().to_vec(),
    );
    (collision, faces, collider)
}

/// A probe target over `collider` with the side table `faces` beside it.
fn target<'a>(
    entity: Entity,
    collider: &'a Collider,
    faces: Option<&'a ColliderFaces>,
) -> SweepTarget<'a> {
    SweepTarget {
        entity,
        collider,
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        owner: SegmentProbeOwner::World,
        class_type: OBJECT3D_CLASS_TYPE,
        faces,
    }
}

/// A stored quad is tested **as its stored polygon**: both collision
/// triangles map to the one face record, the record's normal is the
/// first-three-corner parallelogram of the stored corners — not twice a
/// triangle's area — and the flag word's clear two-sided bit keeps the
/// one-sided gate. The solver's triangle buffer is untouched.
#[test]
fn accept_groundblow_face_stored_quad_reports_its_parallelogram() {
    let (collision, faces, collider) = stored_quad(false);

    // The side table: one stored face for both collision triangles, the
    // polygon's four corners in stored order, the stored flag word carried.
    assert_eq!(collision.triangles(), 2, "two ear-clipped triangles");
    assert_eq!(faces.face_of(), &[Some(0), Some(0)]);
    assert_eq!(faces.faces().len(), 1);
    let face = &faces.faces()[0];
    assert_eq!(face.polygon, 0);
    assert_eq!(
        face.corners,
        vec![
            [0.0, 0.0, 0.0],
            [0.0, 0.0, -4.0],
            [-5.0, 0.0, -4.0],
            [-5.0, 0.0, 1.0],
        ]
    );
    assert_eq!(face.raw_flags, 0);
    assert!(!face.is_two_sided());

    // The solver's collider is still the two ear-clipped triangles.
    let trimesh = collider
        .shape_scaled()
        .as_trimesh()
        .expect("a trimesh collider");
    assert_eq!(
        trimesh.num_triangles(),
        2,
        "the solver's triangles are unchanged"
    );

    // Inside the second ear-clipped half — the centroid of corners (1,2,3):
    // the stored face answers `(0, 20, 0)` where a triangle soup reports that
    // half's `2 * area`, `(0, 25, 0)`.
    let hit_point = Vec3::new(-10.0 / 3.0, 0.0, -7.0 / 3.0);
    let entity = Entity::from_raw_u32(31).expect("an entity id");
    let segment = ground_blow_segment(
        [f64::from(hit_point.x), 50.0, f64::from(hit_point.z)],
        orientation(nose_down()),
        400.0,
    );
    let contact = nearest_contact(&segment, [target(entity, &collider, Some(&faces))])
        .expect("the face answers");
    assert_eq!(contact.entity, entity);
    assert!(close(contact.point_m[1], 0.0), "{contact:?}");
    assert!(
        close(contact.point_m[0], f64::from(hit_point.x)),
        "{contact:?}"
    );
    assert!(
        close(contact.point_m[2], f64::from(hit_point.z)),
        "{contact:?}"
    );
    assert_eq!(
        contact.normal,
        [0.0, 20.0, 0.0],
        "the stored quad's parallelogram"
    );
    assert!(close(contact.distance_m, 50.0), "{contact:?}");

    // The same segment over the same triangles with no face record keeps the
    // per-triangle answer — the fallback for a collider with no stored
    // polygon provenance.
    let fallback = nearest_contact(&segment, [target(entity, &collider, None)])
        .expect("the triangle still answers");
    assert_eq!(
        fallback.normal,
        [0.0, 25.0, 0.0],
        "the fallback reports the hit triangle's own cross product"
    );

    // Flag word clear: the one-sided gate still rejects the back side.
    let from_below = ground_blow_segment(
        [f64::from(hit_point.x), -50.0, f64::from(hit_point.z)],
        orientation(nose_up()),
        400.0,
    );
    assert_eq!(
        nearest_contact(&from_below, [target(entity, &collider, Some(&faces))]),
        None,
        "a flag-clear stored face is still one-sided"
    );
}

/// The stored two-sided bit overrides the one-sided gate (`0x55d75b`): a
/// flag-set stored quad answers a segment that meets it from the `-N` side,
/// where the same mesh flag-clear reports no face — and a collider with no
/// face record still rejects, because the override lives on the stored flag
/// word, not on the collider.
#[test]
fn accept_groundblow_face_two_sided_quad_accepts_a_back_side_hit() {
    let (_collision, two_sided, collider) = stored_quad(true);
    let face = &two_sided.faces()[0];
    assert_eq!(face.raw_flags, FLAG_TWO_SIDED);
    assert!(face.is_two_sided());

    // From below, nose up: the segment's start is on the face's `-N` side and
    // its far end on the `+N` side — the one-sided gate rejects unless the
    // stored flag overrides it.
    let entity = Entity::from_raw_u32(37).expect("an entity id");
    let from_below = ground_blow_segment(
        [-10.0 / 3.0, -50.0, -7.0 / 3.0],
        orientation(nose_up()),
        400.0,
    );
    let contact = nearest_contact(&from_below, [target(entity, &collider, Some(&two_sided))])
        .expect("the two-sided bit accepts the back side");
    assert_eq!(
        contact.normal,
        [0.0, 20.0, 0.0],
        "the stored face's own normal"
    );
    assert!(close(contact.point_m[1], 0.0), "{contact:?}");
    assert!(close(contact.distance_m, 50.0), "{contact:?}");

    // The override is the stored flag word's, not the collider's: the same
    // segment over the same triangles without the face record is still
    // rejected by the per-triangle one-sided gate.
    assert_eq!(
        nearest_contact(&from_below, [target(entity, &collider, None)]),
        None,
        "no face record: the triangle path stays one-sided"
    );
}

/// The production read path: a spawned sea entity's `ColliderFaces` component
/// is what `probe_in_world` reads — the same query the tick fill runs.
#[test]
fn accept_groundblow_face_probe_in_world_reads_the_entities_face_table() {
    let (_collision, faces, collider) = stored_quad(false);
    let mut world = World::new();
    let prober = world.spawn_empty().id();
    let tile = world
        .spawn((
            collider,
            Position(Vec3::ZERO),
            Rotation(Quat::IDENTITY),
            BodyLayer(CollisionLayer::StaticWorld),
            PlaytestSea,
            faces,
        ))
        .id();

    // A downward probe inside the second ear-clipped half reads the stored
    // quad's `(0, 20, 0)`, not the triangle's `(0, 25, 0)`.
    let contact = probe_in_world(
        &world,
        Vec3::new(-10.0 / 3.0, 50.0, -7.0 / 3.0),
        nose_down(),
        400.0,
        prober,
    )
    .expect("the stored face answers");
    assert_eq!(contact.entity, tile);
    assert_eq!(contact.normal, [0.0, 20.0, 0.0]);
    assert!(close(contact.distance_m, 50.0), "{contact:?}");

    // The same entity without its side table is a plain triangle soup again.
    world.entity_mut(tile).remove::<ColliderFaces>();
    let fallback = probe_in_world(
        &world,
        Vec3::new(-10.0 / 3.0, 50.0, -7.0 / 3.0),
        nose_down(),
        400.0,
        prober,
    )
    .expect("the triangle still answers");
    assert_eq!(fallback.normal, [0.0, 25.0, 0.0]);
}

// ------------------------------------------------------------- retail M01 ---

/// The two retail probes below compose M01 at once and stage its launch
/// container into one private cache path — not safe in parallel.
static RETAIL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn retail_lock() -> std::sync::MutexGuard<'static, ()> {
    RETAIL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// M01 composed headless, with the launch cinematic removed so the sweep's
/// world is live from the first boundary.
fn compose() -> App {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("plan");
    let stage = stage_for(&root, &plan).expect("stage");
    let mut app = build_headless(&stage).expect("M01 composes");
    app.world_mut().remove_resource::<LaunchCinematic>();
    app.update();
    app
}

/// Water tile `g28530` (node 547, mesh 371) carries the face records the
/// sweep answers: the eight stored outline polygons #2212 measured — the
/// quad parallelograms `262144` and `196608` and the triangle's
/// `125611.5` m² — with the stored flag words applied (all clear, so the
/// tile's faces stay one-sided).
///
/// The tile is a **direct child** of `world1`, so the flat world draw spawns
/// it as a `WorldColliderInstance` object collider (object id `node-547`),
/// marked a `WorldSweepNode` because its stored node carries the contact bits
/// — a swept target `probe_in_world` reaches. This test reads the record off
/// the entity's own `ColliderFaces` and probes it through the production
/// sweep.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_groundblow_face_m01_g28530_reports_the_stored_faces() {
    let _retail = retail_lock();
    let mut app = compose();

    // Ground truth: mesh 371's collision view, read straight off the
    // container — the stored polygons the world-object collider stands in
    // for and the ear-clipped triangles the solver walks.
    let path = "ZBD/C3/gamez.zbd";
    let root = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR");
    let bytes = std::fs::read(format!("{root}/{path}")).expect("the container reads");
    let mut context = cs_formats::io::ParseContext::with_defaults(path.to_ascii_lowercase());
    let meshes = cs_formats::gamez::read_gamez_meshes(&mut context, path, &bytes).expect("meshes");
    let raw = meshes.meshes[371].as_ref().expect("mesh 371");
    let collision = CollisionMesh::of(&raw.mesh);
    let canonical = ColliderMesh::of(&collision);

    // The entity the world spawned for node 547 — a mesh-collider object
    // carrying the stored-face table beside its collider.
    let tile = app
        .world_mut()
        .query_filtered::<(Entity, &cs_app::world::WorldObjectBinding), With<Collider>>()
        .iter(app.world())
        .find(|(_, binding)| binding.object().as_str() == "node-547")
        .map(|(entity, _)| entity)
        .expect("the g28530 object collider is placed");
    let carried = app
        .world()
        .get::<ColliderFaces>(tile)
        .expect("the g28530 collider carries its stored faces")
        .clone();
    let collider = app
        .world()
        .get::<Collider>(tile)
        .expect("the tile has a collider")
        .clone();
    let position = app.world().get::<Position>(tile).expect("a pose").0;
    let rotation = app.world().get::<Rotation>(tile).expect("a pose").0;
    assert!(
        app.world().get::<ColliderFaces>(tile).is_some()
            && app
                .world()
                .get::<cs_app::mission_session::WorldSweepNode>(tile)
                .is_some(),
        "the swept object collider carries its node marker and its faces"
    );

    // The carried table is the collision view's own: the eight stored
    // outline polygons, each with its stored corners and flag word, and the
    // triangle → face map in solver-triangle order.
    assert_eq!(carried.faces(), canonical.faces());
    assert_eq!(carried.face_of(), canonical.face_of());
    assert_eq!(collision.faces().len(), 8, "eight stored outline polygons");
    assert_eq!(collision.face_of().len(), collision.triangles());
    assert!(
        collision.face_of().iter().all(Option::is_some),
        "every collision triangle stands in for a stored face"
    );
    let normal_of = |face: &cs_content::mesh::CollisionFace| {
        let a = Vec3::from(face.corners[0]);
        let b = Vec3::from(face.corners[1]);
        let c = Vec3::from(face.corners[2]);
        (b - a).cross(c - a)
    };
    // The measured |N|s: polygons 6 and 5 (quads) and 1 (a triangle) —
    // `2026-10-29-groundblow-normal-scale.md`'s own table.
    for (polygon, expected) in [(6, 262_144.0_f64), (5, 196_608.0), (1, 125_611.5)] {
        let face = collision
            .faces()
            .iter()
            .find(|face| face.polygon == polygon)
            .unwrap_or_else(|| panic!("stored polygon {polygon}"));
        let normal = normal_of(face);
        let magnitude = f64::from(normal.length());
        assert!(
            close(magnitude, expected),
            "polygon {polygon}: |N| = {magnitude}, expected {expected}"
        );
        // Corners are the stored outline's, in stored order: quads carry
        // four, the triangle three.
        assert_eq!(face.corners.len(), if polygon == 1 { 3 } else { 4 });
    }
    // Every stored flag word is `0x0` — the tile's faces are all one-sided,
    // so the override is carried (and correctly off).
    assert!(
        collision
            .faces()
            .iter()
            .all(|face| face.raw_flags == 0 && !face.is_two_sided()),
        "g28530's stored flag words are all clear"
    );

    // The solver's collider is still the ear-clipped triangles — the polygon
    // reconstruction added nothing to the buffer Avian walks.
    let trimesh = collider
        .shape_scaled()
        .as_trimesh()
        .expect("a trimesh collider");
    assert_eq!(
        trimesh.num_triangles(),
        collision.triangles(),
        "the solver's triangle count is the collision view's, unchanged"
    );

    // The entity is a real sweep target: a `WorldSweepNode` with the stored
    // node's class type, so `target_of` would class it the original's way.
    let sweep_node = app
        .world()
        .get::<cs_app::mission_session::WorldSweepNode>(tile)
        .expect("the tile is a marked sweep node");
    assert_eq!(sweep_node.node_slot, 547);
    let target = SweepTarget {
        entity: tile,
        collider: &collider,
        position,
        rotation,
        owner: SegmentProbeOwner::World,
        class_type: sweep_node.class_type,
        faces: Some(&carried),
    };

    // Probe each of polygon 5's collision triangles through the production
    // sweep: whichever ear-clipped half the segment meets, the record is the
    // stored quad's `(0, 196608, 0)` — a triangle soup would report one
    // half's `262144` there.
    let face5 = collision
        .faces()
        .iter()
        .position(|face| face.polygon == 5)
        .expect("polygon 5") as u32;
    let halves: Vec<usize> = collision
        .face_of()
        .iter()
        .enumerate()
        .filter_map(|(triangle, slot)| (*slot == Some(face5)).then_some(triangle))
        .collect();
    assert_eq!(halves.len(), 2, "the quad ear-clips to two triangles");
    let mut distinct = std::collections::BTreeSet::new();
    let mut probed_population = false;
    for half in halves {
        let [a, b, c] = collision.indices()[half];
        let centroid = (Vec3::from(canonical.vertices()[a as usize])
            + Vec3::from(canonical.vertices()[b as usize])
            + Vec3::from(canonical.vertices()[c as usize]))
            / 3.0;
        let triangle_normal = (Vec3::from(canonical.vertices()[b as usize])
            - Vec3::from(canonical.vertices()[a as usize]))
        .cross(
            Vec3::from(canonical.vertices()[c as usize])
                - Vec3::from(canonical.vertices()[a as usize]),
        );
        distinct.insert(triangle_normal.length().to_bits());
        // Mesh-local to world: the same pose `contact_of` applies.
        let world = rotation * centroid + position;
        let segment = ground_blow_segment(
            [
                f64::from(world.x),
                f64::from(world.y) + 100.0,
                f64::from(world.z),
            ],
            orientation(nose_down()),
            400.0,
        );
        let contact = nearest_contact(&segment, [target]).expect("the tile answers the probe");
        assert_eq!(contact.entity, tile, "the tile's own face answered");
        let expected = rotation * Vec3::new(0.0, 196_608.0, 0.0);
        for axis in 0..3 {
            assert!(
                close(contact.normal[axis], f64::from(expected[axis])),
                "half {half}: normal {contact:?} ≠ stored face's {expected:?}"
            );
        }
        // The same segment run through the whole composed population answers
        // (several water surfaces overlap here — which entity the probe picks
        // among them is the sweep's own nearest rule).
        if !probed_population {
            let prober = app.world_mut().spawn_empty().id();
            assert!(
                probe_in_world(
                    app.world(),
                    world + Vec3::Y * 100.0,
                    nose_down(),
                    400.0,
                    prober
                )
                .is_some(),
                "the composed world's sweep answers at the tile"
            );
            probed_population = true;
        }
    }
    assert_eq!(
        distinct.len(),
        2,
        "the two halves' triangle normals differ (196608 vs 262144), so the \
         probe saw the stored face rather than a coincidentally-equal triangle"
    );
    assert!(
        distinct.contains(&196_608.0_f32.to_bits()) && distinct.contains(&262_144.0_f32.to_bits()),
        "{distinct:?}"
    );

    // The flag word is carried and applied: clear on every stored face, so a
    // segment that meets the tile from its back side is still no contact.
    let corner = Vec3::from(collision.faces()[0].corners[0]);
    let world = rotation * corner + position;
    let segment = ground_blow_segment(
        [
            f64::from(world.x),
            f64::from(world.y) - 50.0,
            f64::from(world.z),
        ],
        orientation(nose_up()),
        400.0,
    );
    assert_eq!(
        nearest_contact(&segment, [target]),
        None,
        "a flag-clear stored face is still one-sided"
    );

    teardown(&mut app);
}
