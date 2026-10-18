//! Objects that name one mesh share **one derived collision shape** — task
//! #1430, the follow-up to #1424.
//!
//! # The regression this pins
//!
//! Before #1424 the world spawn used `ColliderConstructor::TrimeshFromMesh`,
//! which goes through Avian's `ColliderCache`: keyed by mesh handle, it built
//! the `TriMesh` once per mesh and handed every entity an `Arc` clone. #1424
//! had to stop deriving from the drawn buffer — the original's measured
//! `D3DPT_TRIANGLEFAN` fan, whose folded leaves trip parry's support-point
//! tie-break into a phantom time of impact — and the explicit `Trimesh*`
//! constructor it put in its place does **not** consult that cache. So N
//! objects naming one mesh each built their own copy of the collision view's
//! vertices, indices and BVH, while the *drawn* upload stayed shared.
//!
//! The fix keeps one shape per upload: `WorldMeshAssets` — the map that already
//! makes one `Handle<Mesh>` per mesh — also holds the collision shape derived
//! from that upload once, and every colliding record gets clones of that one
//! `Collider`. Cloning a `Collider` clones its `SharedShape` arcs, so the shape
//! is one allocation; each entity still owns its own `Collider` component, so
//! each keeps its own transform and scale.
//!
//! # What is pinned here
//!
//! * Two (and three) records naming one stored mesh share **one** unscaled
//!   shape allocation, probed by `Arc::ptr_eq` on `Collider::shape()` and by
//!   `Arc::strong_count`, which a per-object derivation cannot reach. A record
//!   naming a different mesh shares nothing with them.
//! * A differing entity scale does not leak: the shared shape stays the stored
//!   geometry bit-for-bit, and each object's `shape_scaled()` is its own scale
//!   applied to it. `Collider::set_scale` is copy-on-write, and this is
//!   measured rather than assumed.
//! * The shape is still the **collision view**, every triangle it stored, not a
//!   hull, a bounding box or the drawn fan.
//!
//! Every test drives production code only (`spawn_world`, the synthetic
//! `twin_harbor_world`/`twin_harbor_meshes` fixture — the same functions the
//! runtime calls) and is `Origin::SyntheticFixture`: no original data, no
//! `CS_GAME_DIR`. Removing the sharing — deriving a collider per object — makes
//! `accept_t1430_two_records_naming_one_mesh_share_one_derived_shape` fail on
//! `Arc::ptr_eq`, so the property is held by the code and not by the comment.

use std::collections::BTreeSet;
use std::sync::Arc;

use avian3d::prelude::Collider;
use bevy::prelude::{App, Entity};
use cs_app::world::{
    HARBOR_HANGAR_TRIANGLES, MESH_SETTLE_UPDATES, SpawnedWorld, TWIN_OBJECT_PANEL,
    TWIN_OBJECT_SHELL_A, TWIN_OBJECT_SHELL_B, TWIN_OBJECT_TRIGGER, WorldMeshes, spawn_world,
    twin_harbor_meshes, twin_harbor_world, world_app,
};
use cs_content::mesh::RenderMesh;
use cs_content::scene::CanonicalTransform;
use cs_content::world::{
    Aabb, Sector, SectorId, SurfaceRole, WorldCollisionRole, WorldCollisionShape, WorldDefinition,
    WorldId, WorldObjectId, WorldObjectInstance,
};
use cs_formats::gamez::{PrimitiveKind, RawCorner, RawMesh, RawPolygon};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

/// The one allocation behind a collider's **unscaled** shape.
///
/// `SharedShape` is `Arc<dyn Shape>`, and cloning a `Collider` clones that arc
/// rather than the geometry, so pointer identity here is exactly "these two
/// colliders collide from one allocation" — the property task #1430 restores —
/// and `strong_count` is the same fact counted.
fn shape_arc(collider: &Collider) -> &Arc<dyn avian3d::parry::shape::Shape> {
    &collider.shape().0
}

/// Whether two colliders' unscaled shapes are the same allocation.
fn shares_unscaled_shape(a: &Collider, b: &Collider) -> bool {
    Arc::ptr_eq(shape_arc(a), shape_arc(b))
}

fn provenance(key: &str) -> Provenance {
    Provenance::designed(ClaimId::new(&format!("t1430.{key}")).expect("the claim id is valid"))
}

fn known<T>(value: T, key: &str) -> Resolved<T> {
    Resolved::Known(Known::new(value, provenance(key)))
}

fn object(key: &str) -> WorldObjectId {
    WorldObjectId::new(key).expect("the test object key is valid")
}

/// The collider Avian holds on the entity a report names for `key`.
fn collider_of(app: &App, report: &SpawnedWorld, key: &str) -> Collider {
    let entity = report
        .collider_for(&object(key))
        .unwrap_or_else(|| panic!("`{key}` was given a collider"));
    app.world()
        .get::<Collider>(entity)
        .unwrap_or_else(|| panic!("`{key}` holds the collider on its own entity"))
        .clone()
}

/// The twin harbor world through the production conversion, settled, so Avian
/// has applied every collider's own scale.
fn twin_spawned() -> (App, SpawnedWorld) {
    let mut app = world_app();
    let report = spawn_world(
        &mut app,
        &twin_harbor_world().expect("the twin harbor world is well formed"),
        &twin_harbor_meshes(),
    )
    .expect("the twin world spawns");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }
    (app, report)
}

/// **Two records naming one mesh collide from one shape allocation, and a
/// record naming a different mesh does not share it.**
///
/// The twin world has four records on the shell — two solids, a presentation-only
/// banner and a trigger volume — so this also pins that the *body-less* trigger
/// layout reaches the same shape as the solid one, exactly as it reaches the
/// same engine mesh asset.
///
/// Observable failure when the sharing is removed: each record's `Collider`
/// carries its own derived `SharedShape`, so `Arc::ptr_eq` is `false` and each
/// `strong_count` is the two references a lone `Collider` holds rather than the
/// several one shared shape accumulates.
#[test]
fn accept_t1430_two_records_naming_one_mesh_share_one_derived_shape() {
    let (app, report) = twin_spawned();

    let shell_a = collider_of(&app, &report, TWIN_OBJECT_SHELL_A);
    let shell_b = collider_of(&app, &report, TWIN_OBJECT_SHELL_B);
    let trigger = collider_of(&app, &report, TWIN_OBJECT_TRIGGER);
    let panel = collider_of(&app, &report, TWIN_OBJECT_PANEL);

    // The geometry is still the canonical collision view, every stored triangle
    // of the merged shell — sharing may not have substituted a primitive or one
    // material group for the whole.
    for (key, collider) in [
        (TWIN_OBJECT_SHELL_A, &shell_a),
        (TWIN_OBJECT_SHELL_B, &shell_b),
        (TWIN_OBJECT_TRIGGER, &trigger),
    ] {
        assert_eq!(
            collider
                .shape()
                .as_trimesh()
                .unwrap_or_else(|| panic!("`{key}`: the collider is a triangle mesh"))
                .indices()
                .len(),
            HARBOR_HANGAR_TRIANGLES,
            "`{key}` must collide from every triangle the stored shell has"
        );
    }

    // One allocation, however many records name it.
    for (key, collider) in [
        (TWIN_OBJECT_SHELL_B, &shell_b),
        (TWIN_OBJECT_TRIGGER, &trigger),
    ] {
        assert!(
            shares_unscaled_shape(&shell_a, collider),
            "`{key}` names the same stored mesh as `{TWIN_OBJECT_SHELL_A}`, so its \
             collider must be built from the same derived shape allocation, not a \
             second copy of the collision view"
        );
    }
    assert_eq!(
        Arc::strong_count(shape_arc(&shell_a)),
        Arc::strong_count(shape_arc(&shell_b)),
        "the two solids' shapes are the same allocation, so their reference counts \
         are the same count"
    );
    assert!(
        Arc::strong_count(shape_arc(&shell_a)) > 2,
        "a lone `Collider` holds two references to its shape (unscaled and scaled); \
         more than that is the cache and the sibling colliders sharing one \
         allocation. Got {}",
        Arc::strong_count(shape_arc(&shell_a))
    );

    // A record on a **different** mesh shares no shape with them, and the
    // different geometry really is different: the panel is 12 triangles, not 36.
    assert!(
        !shares_unscaled_shape(&shell_a, &panel),
        "the panel names its own mesh, so it must derive its own shape"
    );
    assert_eq!(
        panel.shape().as_trimesh().map(|t| t.indices().len()),
        Some(12),
        "and its shape is the panel's own twelve triangles"
    );

    // The shared shape is geometry only: the two solids still hold their own
    // poses, so sharing it did not share where an object is.
    let translation = |key: &str| {
        let entity = report
            .collider_for(&object(key))
            .unwrap_or_else(|| panic!("`{key}` was given a collider"));
        app.world()
            .get::<bevy::prelude::Transform>(entity)
            .unwrap_or_else(|| panic!("`{key}` carries its authored transform"))
            .translation
    };
    assert_ne!(
        translation(TWIN_OBJECT_SHELL_A),
        translation(TWIN_OBJECT_SHELL_B),
        "the two solids are placed from their own records, so one derived shape \
         must not have brought one object's pose along with it"
    );
}

// --------------------------------------------------------------------------
// A differing entity scale stays on its own collider.
// --------------------------------------------------------------------------

/// The two authored corners sets' shapes: a right triangle in the `y = 0`
/// plane, so a per-axis scale is measurable on every vertex and no vertex is
/// already axis-aligned by accident.
const TRIANGLE: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 0.0, 3.0]];

/// Two *different* non-uniform scales. Non-uniform because a scale Avian could
/// not represent is the case that would fall back to a hull or a box and seal
/// the shape, and different so a leaked scale is visible as the wrong one.
const SCALE_A: [f64; 3] = [2.0, 1.0, 0.5];
const SCALE_B: [f64; 3] = [0.5, 2.0, 1.0];

const SMALL_KEY: &str = "scale.small";
const LARGE_KEY: &str = "scale.large";

/// The stored outline: one triangle polygon, the simplest mesh whose collision
/// view is non-empty and whose geometry is checkable corner by corner.
fn triangle_raw_mesh() -> RawMesh {
    RawMesh {
        positions: TRIANGLE.to_vec(),
        normals: Vec::new(),
        polygons: vec![RawPolygon {
            kind: PrimitiveKind::Polygon,
            raw_flags: 0,
            material: 0,
            corners: (0..TRIANGLE.len())
                .map(|position| RawCorner {
                    position: position as u32,
                    normal: None,
                    uv: None,
                    color: None,
                })
                .collect(),
        }],
    }
}

/// A diagonal scale matrix and a translation, as one authored instance
/// transform.
fn scaled_transform(scale: [f64; 3], translation: [f64; 3]) -> CanonicalTransform {
    CanonicalTransform::try_new(
        [
            [scale[0], 0.0, 0.0],
            [0.0, scale[1], 0.0],
            [0.0, 0.0, scale[2]],
        ],
        translation,
    )
    .expect("a positive diagonal scale with a translation is exact")
}

/// A world with two objects naming **one** mesh, each at its own scale, so
/// sharing and per-object placement are both exercised by the production path.
fn two_record_world(id: &ContentId, scales: [[f64; 3]; 2], world_key: &str) -> WorldDefinition {
    let sector = SectorId::new("only").expect("the sector key is valid");
    let object_sector = sector.clone();
    let mesh_ref = || known(id.clone(), "mesh");
    let instance = |key: &str, scale: [f64; 3], x: f64| {
        WorldObjectInstance::try_new(
            object(key),
            mesh_ref(),
            scaled_transform(scale, [x, 0.0, 0.0]),
            known(WorldCollisionRole::Solid, "collision"),
            known(WorldCollisionShape::FromMesh, "shape"),
            known(SurfaceRole::Ground, "surface"),
            vec![object_sector.clone()],
            provenance(key),
        )
        .expect("the sector list has no duplicates")
    };

    WorldDefinition::try_new(
        WorldId::from_key(world_key).expect("the world key is valid"),
        Origin::SyntheticFixture,
        known(cs_content::world::WorldBoundary::default(), "boundary"),
        vec![Sector::new(
            sector,
            Aabb::try_new([-10.0, -10.0, -10.0], [10.0, 10.0, 10.0]).expect("the bounds are valid"),
        )],
        vec![
            instance(SMALL_KEY, scales[0], -2.0),
            instance(LARGE_KEY, scales[1], 2.0),
        ],
        provenance("definition"),
    )
    .expect("the definition is structurally valid: one sector, two objects")
}

/// The two-record world at two **different** non-uniform scales: the shape must
/// be shared while each object's own scale stays its own.
fn scaled_world(id: &ContentId) -> WorldDefinition {
    two_record_world(id, [SCALE_A, SCALE_B], "test.scaled_world")
}

/// The stored corners scaled per axis, as sorted bit patterns so the comparison
/// is about geometry rather than vertex order.
fn scaled_corners(scale: [f64; 3]) -> BTreeSet<[u32; 3]> {
    TRIANGLE
        .iter()
        .map(|corner| {
            let scaled = [
                corner[0] as f64 * scale[0],
                corner[1] as f64 * scale[1],
                corner[2] as f64 * scale[2],
            ];
            scaled.map(|component| component as f32).map(f32::to_bits)
        })
        .collect()
}

/// A collider's **unscaled** corners, as sorted bit patterns.
fn unscaled_corners(collider: &Collider) -> BTreeSet<[u32; 3]> {
    collider
        .shape()
        .as_trimesh()
        .expect("the shared shape is a triangle mesh")
        .vertices()
        .iter()
        .map(|vertex| vertex.to_array().map(f32::to_bits))
        .collect()
}

/// A collider's **scaled** corners, as sorted bit patterns.
fn scaled_shape_corners(collider: &Collider) -> BTreeSet<[u32; 3]> {
    collider
        .shape_scaled()
        .as_trimesh()
        .expect("scaling a trimesh keeps it a trimesh, not a hull or a box")
        .vertices()
        .iter()
        .map(|vertex| vertex.to_array().map(f32::to_bits))
        .collect()
}

/// **A differing entity scale does not leak one object's scale into another's
/// shape.**
///
/// The two objects name one mesh and share one unscaled shape, but Avian applies
/// each entity's own `Transform` scale to its own `Collider`. The shared shape
/// must stay the stored geometry bit-for-bit, and each object's `shape_scaled()`
/// must be that geometry times its own scale. `Collider::set_scale` is
/// copy-on-write — a scaled `SharedShape` is a new allocation — and this measures
/// it rather than assuming it, because the whole point of sharing is that both
/// colliders hold the same `Arc` until one is scaled.
///
/// Observable failure if the scaling were applied in place on the shared shape:
/// both `shape_scaled()` readings collapse onto the last-written scale, and the
/// object whose scale is not it fails here; if the unscaled `shape()` changed,
/// the `unscaled_corners` assertion fails for both.
#[test]
fn accept_t1430_a_per_entity_scale_does_not_leak_into_a_shared_shape() {
    let render = RenderMesh::build(&triangle_raw_mesh())
        .expect("the triangle's single outline decodes through the production gate");
    let id = ContentId::from_source(ContentKind::Mesh, "synthetic.t1430_triangle")
        .expect("the mesh id is valid");
    let mut meshes = WorldMeshes::new();
    meshes
        .insert_render_mesh(id.clone(), &render, &[])
        .expect("a single group with no per-corner attribute uploads");

    let mut app = world_app();
    let report =
        spawn_world(&mut app, &scaled_world(&id), &meshes).expect("the scaled world spawns");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }

    let small = collider_of(&app, &report, SMALL_KEY);
    let large = collider_of(&app, &report, LARGE_KEY);

    // One allocation, as in the twin world: the scale is per collider, not per
    // shape.
    assert!(
        shares_unscaled_shape(&small, &large),
        "two records naming one mesh must still share one unscaled collision shape"
    );

    // The shared shape is the stored geometry, untouched by either scale.
    let stored: BTreeSet<[u32; 3]> = TRIANGLE
        .iter()
        .map(|corner| corner.map(f32::to_bits))
        .collect();
    assert_eq!(
        unscaled_corners(&small),
        stored,
        "the shared unscaled shape must stay the stored geometry: scaling one \
         collider may not write through the `Arc` the other holds"
    );

    // And each collider's scaled shape is its own scale, not the sibling's.
    assert!(
        !Arc::ptr_eq(&small.shape_scaled().0, &large.shape_scaled().0),
        "two different scales must be two different scaled shapes; one allocation \
         for both is a leaked scale"
    );
    assert_eq!(
        scaled_shape_corners(&small),
        scaled_corners(SCALE_A),
        "the first object's collider must be scaled by its own authored scale"
    );
    assert_eq!(
        scaled_shape_corners(&large),
        scaled_corners(SCALE_B),
        "the second object's collider must be scaled by its own authored scale, not \
         the first object's"
    );

    // The entity the collider sits on carries the scale it was built from, so the
    // two facts are one object's rather than two independent derivations.
    for (key, scale) in [(SMALL_KEY, SCALE_A), (LARGE_KEY, SCALE_B)] {
        let entity: Entity = report
            .collider_for(&object(key))
            .unwrap_or_else(|| panic!("`{key}` was given a collider"));
        let transform = app
            .world()
            .get::<bevy::prelude::Transform>(entity)
            .unwrap_or_else(|| panic!("`{key}` carries its authored transform"));
        assert_eq!(
            transform.scale,
            bevy::prelude::Vec3::new(scale[0] as f32, scale[1] as f32, scale[2] as f32),
            "`{key}`'s entity must carry the scale its collider was scaled by"
        );
    }
}

// --------------------------------------------------------------------------
// The declared subnormal flush reaches the shared shape.
// --------------------------------------------------------------------------

/// The corpus's own subnormal bit pattern: `0x0000_0003` is the smallest
/// positive subnormal, the one parry's binned BVH builder cannot bin when it
/// divides by a leaf-AABB extent. Authored here as the *stored* value; only the
/// declared [`SUBNORMAL_POSITION_CLAIM`] rule may change it.
const STORED_SUBNORMAL_BITS: u32 = 0x0000_0003;

/// The same right triangle as [`TRIANGLE`], with its first corner's `x` a
/// **stored subnormal**.
///
/// A non-degenerate triangle after the flush, so the shape is a real trimesh
/// whatever the rule does to that component: the premise under test is the bit
/// pattern, not a collapsed solid.
fn subnormal_triangle_raw_mesh() -> RawMesh {
    RawMesh {
        positions: vec![
            [f32::from_bits(STORED_SUBNORMAL_BITS), 0.0, 0.0],
            [4.0, 0.0, 0.0],
            [0.0, 0.0, 3.0],
        ],
        normals: Vec::new(),
        polygons: vec![RawPolygon {
            kind: PrimitiveKind::Polygon,
            raw_flags: 0,
            material: 0,
            corners: (0..TRIANGLE.len())
                .map(|position| RawCorner {
                    position: position as u32,
                    normal: None,
                    uv: None,
                    color: None,
                })
                .collect(),
        }],
    }
}

/// **The declared subnormal flush applies to the shape every record shares, not
/// just to the first one's copy.**
///
/// Task #1430's third acceptance criterion: the upload boundary canonicalises
/// subnormal position components under
/// [`SUBNORMAL_POSITION_CLAIM`](cs_app::render::bevy_mesh::SUBNORMAL_POSITION_CLAIM),
/// and a shape derived once per upload and cloned must therefore carry the
/// **flushed** corners — a per-object derivation that forgot the rule, or a
/// cache fed from a different buffer, would hand parry the raw subnormal and
/// the binned BVH builder would panic on it.
///
/// Observable failure when the flush is removed from the derivation: the shared
/// shape's corner set keeps `0x0000_0003` instead of `+0.0`, so the equality
/// fails, and the spawn itself may panic inside parry.
#[test]
fn accept_t1430_the_declared_subnormal_flush_reaches_the_shared_shape() {
    let render = RenderMesh::build(&subnormal_triangle_raw_mesh())
        .expect("the subnormal triangle's single outline decodes through the production gate");
    // The premise: the reader is faithful and the stored corner really is
    // subnormal, so a flush that does nothing would be visible below.
    assert_eq!(
        render
            .vertices()
            .iter()
            .flat_map(|vertex| vertex.position)
            .filter(|component| component.is_subnormal())
            .count(),
        1,
        "the authored premise: exactly one stored position component is subnormal"
    );

    let id = ContentId::from_source(ContentKind::Mesh, "synthetic.t1430_subnormal_triangle")
        .expect("the mesh id is valid");
    let mut meshes = WorldMeshes::new();
    meshes
        .insert_render_mesh(id.clone(), &render, &[])
        .expect("a single group with no per-corner attribute uploads");
    let uploaded = meshes.get(&id).expect("the mesh is registered");
    assert!(
        uploaded.subnormal_components() >= 1,
        "the upload boundary reports the component it flushed, so the premise \
         survives the production adapter: got {}",
        uploaded.subnormal_components()
    );

    let mut app = world_app();
    let report = spawn_world(
        &mut app,
        &two_record_world(&id, [[1.0; 3]; 2], "test.subnormal_shared_world"),
        &meshes,
    )
    .expect("the subnormal world spawns");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }

    let small = collider_of(&app, &report, SMALL_KEY);
    let large = collider_of(&app, &report, LARGE_KEY);
    assert!(
        shares_unscaled_shape(&small, &large),
        "both records name one mesh, so they must share one derived shape before \
         this test can say anything about what that shape carries"
    );

    // The shared shape is the **canonicalised** triangle: the subnormal `x` is
    // the positive signed zero, and nothing else moved.
    let canonicalised: BTreeSet<[u32; 3]> = [[0.0f32, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 0.0, 3.0]]
        .iter()
        .map(|corner| corner.map(f32::to_bits))
        .collect();
    assert_eq!(
        unscaled_corners(&small),
        canonicalised,
        "the shared shape must carry the corners the declared \
         SUBNORMAL_POSITION_CLAIM rule flushed, bit for bit: a subnormal left in \
         the cache would fail here and can panic parry's BVH builder"
    );
    assert!(
        unscaled_corners(&small)
            .iter()
            .flatten()
            .all(|bits| !f32::from_bits(*bits).is_subnormal()),
        "no shared corner may keep a subnormal component"
    );
}
