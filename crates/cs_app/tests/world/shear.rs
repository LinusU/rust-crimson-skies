//! The sheared world object is **placed**, not refused: F18-A's contract that
//! a matrix no translation/rotation/scale triple reproduces stops the whole
//! build is narrowed to the matrices that have no exact placement at all.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! non-negotiable behavior 1 and acceptance test AC01. Task test prefix:
//! `accept_f18_b_shear_`.
//!
//! What these tests measure, one per fact:
//!
//! * the **presentation** keeps the whole authored affine across Bevy's
//!   transform propagation — a node that also carried a `Transform` would have
//!   the shear silently replaced by the identity;
//! * the **collision** carries the authored linear map inside its *shape*, so
//!   the collider's pose is again an exact translation/rotation/scale and its
//!   geometry is the authored box's eight corners pushed through the authored
//!   map — nothing approximated, no second asset;
//! * a body that reaches the sheared face is stopped **at that face**, and a
//!   body placed inside is pushed out of it — the collision *behaves* like the
//!   authored solid;
//! * every baked face normal points **out of** the solid. That is a separate
//!   fact from the previous one and is checked geometrically, because a body
//!   still collides correctly against an inside-out solid: parry's
//!   `ConvexPolyhedron::from_convex_mesh` takes each face's normal straight
//!   from the winding it is handed, so a reversed winding produces a solid
//!   whose `contains_local_point`, `project_local_point` and `face.normal`
//!   manifolds describe its complement, while contact generation — which uses
//!   sign-agnostic support functions — keeps working;
//! * a **mesh-derived** collision keeps every stored triangle when the same
//!   bake is applied to it, which is the recipe F18-B's mesh path uses;
//! * a matrix that mirrors *and* shears, and one whose linear map collapses
//!   space, are still refused — by name, and before any entity exists.
//!
//! No original data and no `CS_GAME_DIR`: everything here is synthetic fixture
//! content, so nothing in this file is `verified_original`.

use avian3d::parry::shape::{ConvexPolyhedron, Cuboid, SharedShape, TriMesh};
use avian3d::prelude::{
    Collider, CollisionLayers as AvianCollisionLayers, Gravity, LinearVelocity, Mass, Position,
    RigidBody, Rotation, Sensor, SpeculativeMargin, SweptCcd,
};
use bevy::prelude::{App, Entity, GlobalTransform, Mat3, Transform, Vec3, World};
use cs_app::world::{
    AffinePlacement, AffinePlacementError, WorldFixture, WorldMeshes, WorldSpawnError,
    affine::{bake_shape, shear_residual},
    canonical_matrix, instance_placement, instance_placements, spawn_world,
};
use cs_content::scene::CanonicalTransform;
use cs_content::world::{
    Aabb, Sector, SectorId, SurfaceRole, WorldBoundary, WorldCollisionRole, WorldCollisionShape,
    WorldDefinition, WorldId, WorldObjectId, WorldObjectInstance,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

// --------------------------------------------------------------- the fixture ---

/// The authored linear map of the fixture panel: a shear of `0.5` per meter of
/// `+y`, so the panel's local `+y` axis leans towards `+x`.
pub const SHEAR_M: f64 = 0.5;

/// Half extents of the panel in its local frame, in meters.
pub const PANEL_HALF_M: [f64; 3] = [0.5, 1.5, 0.5];

/// The panel's centre: the flight line runs through it along `x`.
pub const PANEL_POS_M: [f64; 3] = [0.0, 1.5, 0.0];

fn provenance(key: &str) -> Provenance {
    Provenance::designed(ClaimId::new(&format!("shear.{key}")).expect("the claim id is valid"))
}

fn known<T>(value: T, key: &str) -> Resolved<T> {
    Resolved::Known(Known::new(value, provenance(key)))
}

fn object(key: &str) -> WorldObjectId {
    WorldObjectId::new(key).expect("the test object key is valid")
}

fn mesh_id() -> Resolved<ContentId> {
    known(
        ContentId::from_source(ContentKind::Mesh, "synthetic.panel").expect("the mesh id is valid"),
        "mesh",
    )
}

/// The panel's authored transform: the shear, translated to the panel's centre.
fn sheared_transform() -> CanonicalTransform {
    CanonicalTransform::try_new(
        [[1.0, SHEAR_M, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        PANEL_POS_M,
    )
    .expect("the shear is finite")
}

fn panel(key: &str, transform: CanonicalTransform) -> WorldObjectInstance {
    WorldObjectInstance::try_new(
        object(key),
        mesh_id(),
        transform,
        known(WorldCollisionRole::Solid, "collision"),
        known(
            WorldCollisionShape::cuboid(PANEL_HALF_M).expect("the box is valid"),
            "shape",
        ),
        known(SurfaceRole::Ground, "surface"),
        vec![SectorId::new("only").expect("the sector key is valid")],
        provenance(key),
    )
    .expect("the sector list has no duplicates")
}

/// A one-object world holding a single object with `transform`.
fn one_panel_world(key: &str, transform: CanonicalTransform) -> WorldDefinition {
    WorldDefinition::try_new(
        WorldId::from_key(&format!("test.{key}_world")).expect("the world key is valid"),
        Origin::SyntheticFixture,
        known(WorldBoundary::default(), "boundary"),
        vec![Sector::new(
            SectorId::new("only").expect("the sector key is valid"),
            Aabb::try_new([-20.0, -20.0, -20.0], [20.0, 20.0, 20.0]).expect("the bounds are valid"),
        )],
        vec![panel(key, transform)],
        provenance("definition"),
    )
    .expect("the definition is structurally valid")
}

/// The sheared panel, alone in its world.
fn sheared_world() -> WorldDefinition {
    one_panel_world("sheared", sheared_transform())
}

/// The eight **local** corners of the panel's authored box under the authored
/// linear map: the geometry the collider's *shape* holds, computed from the
/// *record* through the one conversion and independently of the runtime's
/// decomposition, so a spawn that bakes the wrong map cannot agree with it.
///
/// The shape is local and the pose carries the translation (measured: the
/// collider's `Position` is the authored translation and its `Transform` scale
/// is one), which is why these corners carry no world offset.
fn authored_local_corners(transform: &CanonicalTransform) -> Vec<Vec3> {
    let matrix = canonical_matrix(transform);
    let translation = matrix.w_axis.truncate();
    (0..8)
        .map(|index| {
            let sign = |bit: u32| if index & (1 << bit) == 0 { -1.0 } else { 1.0 };
            matrix.transform_point3(Vec3::new(
                sign(0) * PANEL_HALF_M[0] as f32,
                sign(1) * PANEL_HALF_M[1] as f32,
                sign(2) * PANEL_HALF_M[2] as f32,
            )) - translation
        })
        .collect()
}

/// A lexicographic order on points, so two vertex lists can be compared as sets
/// without relying on the order the physics library stored them in.
fn partial_order(a: &Vec3, b: &Vec3) -> std::cmp::Ordering {
    let left = a.to_array();
    let right = b.to_array();
    (0..3)
        .filter_map(|axis| left[axis].partial_cmp(&right[axis]))
        .find(|ordering| *ordering != std::cmp::Ordering::Equal)
        .unwrap_or(std::cmp::Ordering::Equal)
}

// ------------------------------------------------------- 1. the presentation ---

/// **The presentation keeps the authored affine whole.** The panel's sheared
/// matrix is on the visual node, bit-identical after Bevy has propagated
/// transforms five times, and the node carries **no** `Transform` — a
/// translation/rotation/scale component would have replaced the shear with the
/// identity on the first propagation, silently.
///
/// Observable failure: the visual entity's matrix comes back as the identity's
/// linear part, i.e. the shear is gone from the drawn object while the record
/// still declares it.
#[test]
fn accept_f18_b_shear_the_presentation_carries_the_whole_authored_affine() {
    let transform = sheared_transform();
    let mut fixture = WorldFixture::builder(sheared_world())
        .build()
        .expect("the sheared panel is placeable");
    let visual = fixture
        .spawned()
        .visual_for(&object("sheared"))
        .expect("the panel is presented");

    let authored = canonical_matrix(&transform);
    assert_ne!(
        authored.y_axis.x, 0.0,
        "the fixture matrix must actually carry a shear, or this test proves nothing"
    );
    assert!(
        shear_residual(authored) > 0.0,
        "and it must be one the decomposition cannot reproduce"
    );

    fixture.step(5);
    let drawn = fixture
        .world()
        .get::<GlobalTransform>(visual)
        .expect("the visual entity still exists");
    assert_eq!(
        drawn.to_matrix().to_cols_array(),
        authored.to_cols_array(),
        "the drawn affine must be the authored matrix exactly, not a decomposition of it"
    );
    assert!(
        fixture.world().get::<Transform>(visual).is_none(),
        "a `Transform` on the same entity makes propagation overwrite the affine with a \
         translation/rotation/scale, which is how a shear would be lost silently"
    );
}

/// **A representable object keeps its `Transform`, so F18-C can still move it.**
/// The removal of `Transform` is scoped to the sheared case, which is the only
/// one where a `Transform` beside the affine would destroy it. A
/// translation/rotation/scale object must present exactly as it did before this
/// decision, including the pose F18-C's overlay consumer writes to.
///
/// Observable failure: the presentation of an ordinary object loses its
/// `Transform`, and `displace_object` then moves only the collider's position —
/// the render/collision split F18 non-negotiable behavior 1 exists to prevent.
#[test]
fn accept_f18_b_shear_a_representable_object_still_carries_the_pose_overlays_write_to() {
    let upright = CanonicalTransform::try_new(
        [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        PANEL_POS_M,
    )
    .expect("a rotation and a translation are finite");
    let fixture = WorldFixture::builder(one_panel_world("upright", upright))
        .build()
        .expect("a representable panel is placeable");
    let visual = fixture
        .spawned()
        .visual_for(&object("upright"))
        .expect("the panel is presented");
    let pose = fixture.world().get::<Transform>(visual).expect(
        "a representable object's presentation carries a `Transform`: it is what \
                 `overlays::displace_object` writes to move a drawn object",
    );
    assert!(
        pose.to_matrix()
            .to_cols_array()
            .iter()
            .all(|value| value.is_finite()),
        "the decomposition must be finite for a representable matrix"
    );
    // And it is the authored matrix, so nothing regressed for the common case.
    let drawn = fixture
        .world()
        .get::<GlobalTransform>(visual)
        .expect("the visual entity still exists");
    assert!(
        drawn
            .to_matrix()
            .to_cols_array()
            .iter()
            .zip(canonical_matrix(&upright).to_cols_array())
            .all(|(got, want)| (got - want).abs() < 1e-5),
        "for a representable matrix the pose and the affine agree, so the object is drawn \
         where the record says"
    );
}

/// **A sheared object refuses a displacement instead of moving half of itself.**
/// Its presentation deliberately has no `Transform`, so `displace_object` must
/// refuse by name rather than write only the collider's `Position` — a door that
/// visibly opens while its collision stays shut is the mismatch F18
/// non-negotiable behavior 1 exists to prevent.
///
/// Observable failure: the displacement is applied anyway, and afterwards the
/// drawn half and the collided half are at different places.
#[test]
fn accept_f18_b_shear_a_sheared_object_refuses_a_displacement_rather_than_moving_half_of_itself() {
    use cs_app::world::overlays::{OverlayError, displace_object};

    let mut fixture = WorldFixture::builder(sheared_world())
        .build()
        .expect("the sheared panel is placeable");
    let spawned = fixture
        .spawned()
        .object(&object("sheared"))
        .cloned()
        .expect("the panel is in the report");
    let collider = spawned
        .collider
        .as_ref()
        .expect("a solid cuboid object has a collider")
        .entity;
    let position_before = fixture
        .world()
        .get::<Position>(collider)
        .expect("the collider has a position")
        .0;

    let error = displace_object(
        fixture.app_mut().world_mut(),
        &spawned,
        Vec3::new(0.0, 0.0, 2.0),
    )
    .expect_err("a sheared object has no `Transform` to move, so it must be refused");
    assert!(
        matches!(
            &error,
            OverlayError::Undisplaceable { target, .. } if target.as_str() == "sheared"
        ),
        "the refusal must name the object, got {error:?}"
    );

    let position_after = fixture
        .world()
        .get::<Position>(collider)
        .expect("the collider still has a position")
        .0;
    assert_eq!(
        position_before, position_after,
        "a refused displacement must move nothing at all, not even the collision half"
    );
}

// -------------------------------------------------------- 2. the collision ---

/// **The collision carries the shear in its shape, exactly.** The collider's
/// own pose is an exact translation/rotation/scale again, and its geometry is
/// the authored box's eight corners pushed through the authored linear map —
/// the same eight points, no more and no fewer.
///
/// Observable failure: the baked solid is a plain box (the shear is dropped),
/// a hull with extra or missing corners (the map was applied twice or not at
/// all), or the collider pose still tries to hold the shear.
#[test]
fn accept_f18_b_shear_the_collision_carries_the_authored_linear_map_in_its_shape() {
    let transform = sheared_transform();
    let mut fixture = WorldFixture::builder(sheared_world())
        .build()
        .expect("the sheared panel is placeable");
    fixture.step(1);
    let collider_entity = fixture
        .spawned()
        .collider_for(&object("sheared"))
        .expect("the panel is collided");

    let collider = fixture
        .world()
        .get::<Collider>(collider_entity)
        .expect("the panel carries a collider");
    let polyhedron = collider
        .shape_scaled()
        .as_convex_polyhedron()
        .unwrap_or_else(|| {
            panic!(
                "a sheared box is a parallelepiped, not a cuboid: got {:?}",
                collider.shape_scaled()
            )
        });
    let mut baked: Vec<Vec3> = polyhedron.points().to_vec();
    baked.sort_by(partial_order);
    let mut want = authored_local_corners(&transform);
    want.sort_by(partial_order);
    assert_eq!(
        baked, want,
        "the collision geometry must be the authored box's eight corners under the authored \
         linear map, exactly"
    );
    assert_eq!(
        polyhedron.faces().len(),
        6,
        "a parallelepiped has six faces: nothing was simplified away"
    );
    assert_faces_point_outwards(polyhedron, &transform);

    // The pose holds only what a pose can hold.
    let pose = fixture
        .world()
        .get::<Transform>(collider_entity)
        .expect("the collider entity has a pose");
    assert_eq!(
        pose.to_matrix().to_cols_array(),
        [
            1.0,
            0.0,
            0.0,
            0.0, //
            0.0,
            1.0,
            0.0,
            0.0, //
            0.0,
            0.0,
            1.0,
            0.0, //
            PANEL_POS_M[0] as f32,
            PANEL_POS_M[1] as f32,
            PANEL_POS_M[2] as f32,
            1.0,
        ],
        "the shear must be inside the shape, not in the pose"
    );
    let position = fixture
        .world()
        .get::<Position>(collider_entity)
        .expect("the collider entity has a physics position");
    assert_eq!(
        position.0,
        Vec3::new(
            PANEL_POS_M[0] as f32,
            PANEL_POS_M[1] as f32,
            PANEL_POS_M[2] as f32
        ),
        "the authored translation is the one part the pose still carries"
    );
}

/// **The shear is in the collision, not just in the report.** A body placed at a
/// point that is inside the *sheared* panel and outside the *unsheared* box is
/// pushed out of it, and a body arriving from outside at 400 m/s is stopped at
/// the panel's sheared face.
///
/// Each probe gets its own fixture, on purpose: a body ejected from inside the
/// solid leaves at a speed and a direction that are not the point of this test,
/// and in a shared fixture that ejection is what the *other* probe ends up
/// measuring. One fixture per probe keeps each assertion about the solid.
///
/// Observable failure: the collider is an unsheared box placed at the panel's
/// centre (the shear never reached the geometry) — then the first body sits
/// still in free space and the second flies straight through.
#[test]
fn accept_f18_b_shear_the_baked_solid_blocks_inside_the_shear_and_outside_it() {
    let centre = Vec3::new(
        PANEL_POS_M[0] as f32,
        PANEL_POS_M[1] as f32,
        PANEL_POS_M[2] as f32,
    );

    // A body placed inside the sheared solid and outside the unsheared box.
    let inside_the_shear = shear_linear() * Vec3::new(0.4, 1.4, 0.0) + centre;
    assert!(
        !inside_unsheared_box(inside_the_shear),
        "the probe point must be outside the unsheared box, or the test cannot tell the \
         two apart"
    );
    let mut inside_fixture = WorldFixture::builder(sheared_world())
        .build()
        .expect("the sheared panel is placeable");
    let inside = spawn_probe(inside_fixture.app_mut(), inside_the_shear, Vec3::ZERO);
    for _ in 0..120 {
        inside_fixture.step(1);
    }
    let inside_end = position_of(inside_fixture.world(), inside);
    assert!(
        inside_end.distance(inside_the_shear) > 0.1,
        "a body inside the sheared solid must be pushed out of it: it moved \
         {} m and ended at {inside_end:?}",
        inside_end.distance(inside_the_shear)
    );
    assert!(
        !inside_sheared_solid(inside_end),
        "a body pushed out of the solid must end up outside it, and it ended at \
         {inside_end:?}"
    );

    // A body arriving from outside the panel's `+z` face at 400 m/s. The panel
    // is 1 m thick and the probe covers 3.33 m per tick, so a body that is not
    // detected is 400 m past the wall on the far side.
    let mut outside_fixture = WorldFixture::builder(sheared_world())
        .build()
        .expect("the sheared panel is placeable");
    let approach = Vec3::new(centre.x, centre.y, centre.z + 6.0);
    let outside = spawn_probe(
        outside_fixture.app_mut(),
        approach,
        Vec3::new(0.0, 0.0, -400.0),
    );
    for _ in 0..120 {
        outside_fixture.step(1);
    }
    let outside_end = position_of(outside_fixture.world(), outside);
    assert!(
        !inside_sheared_solid(outside_end),
        "a body arriving at 400 m/s must be stopped at the panel's +z face and must not end \
         up inside it; it ended at {outside_end:?}"
    );
}

/// Whether `point` is inside the parallelepiped the authored map makes of the
/// panel's box, in **world** coordinates: the solid's own predicate, worked out
/// from the record rather than read back from the built collider, so a solid
/// built from the wrong map cannot agree with this.
fn inside_sheared_solid(point: Vec3) -> bool {
    let centre = Vec3::new(
        PANEL_POS_M[0] as f32,
        PANEL_POS_M[1] as f32,
        PANEL_POS_M[2] as f32,
    );
    let local = shear_linear().inverse() * (point - centre);
    local.x.abs() < PANEL_HALF_M[0] as f32
        && local.y.abs() < PANEL_HALF_M[1] as f32
        && local.z.abs() < PANEL_HALF_M[2] as f32
}

/// Whether `point` is inside the panel's box *without* the shear applied: the
/// reference the baked solid must differ from.
fn inside_unsheared_box(point: Vec3) -> bool {
    let centre = Vec3::new(
        PANEL_POS_M[0] as f32,
        PANEL_POS_M[1] as f32,
        PANEL_POS_M[2] as f32,
    );
    let local = point - centre;
    local.x.abs() < PANEL_HALF_M[0] as f32
        && local.y.abs() < PANEL_HALF_M[1] as f32
        && local.z.abs() < PANEL_HALF_M[2] as f32
}

/// The six face directions of the solid `transform` maps the authored box onto:
/// the three generator cross products and their negatives, as *directions*.
///
/// The set is the same whichever way each pair points, so this alone cannot see
/// an inside-out solid — [`assert_faces_point_outwards`] is what does. It is
/// derived here from the record's own geometry, not from what the physics
/// library stored.
fn face_directions(transform: &CanonicalTransform) -> Vec<Vec3> {
    let linear = Mat3::from_mat4(canonical_matrix(transform));
    let half = Vec3::new(
        PANEL_HALF_M[0] as f32,
        PANEL_HALF_M[1] as f32,
        PANEL_HALF_M[2] as f32,
    );
    // The three generators of the parallelepiped.
    let edges = [
        linear * Vec3::new(half.x, 0.0, 0.0),
        linear * Vec3::new(0.0, half.y, 0.0),
        linear * Vec3::new(0.0, 0.0, half.z),
    ];
    let mut normals = Vec::with_capacity(6);
    for (a, b) in [(1usize, 2usize), (2, 0), (0, 1)] {
        let normal = edges[a].cross(edges[b]).normalize();
        normals.push(normal);
        normals.push(-normal);
    }
    normals.sort_by(partial_order);
    normals
}

/// Asserts that `polyhedron`'s faces are the authored image's *and* point out
/// of it.
///
/// The direction check compares unordered sets, which an inside-out solid also
/// satisfies — negating every normal permutes the six face normals among
/// themselves. The orientation check is therefore geometric and per face: the
/// solid is a parallelepiped centred on the origin whatever the authored map,
/// so for each face the stored normal must have a positive dot product with the
/// direction from the solid's centre to that face's own centre. A reversed
/// winding fails this; a mirrored map passes it, because the origin stays the
/// centre of a mirrored parallelepiped too.
fn assert_faces_point_outwards(polyhedron: &ConvexPolyhedron, transform: &CanonicalTransform) {
    let mut actual: Vec<Vec3> = polyhedron.faces().iter().map(|face| face.normal).collect();
    actual.sort_by(partial_order);
    let expected = face_directions(transform);
    assert_eq!(
        actual.len(),
        expected.len(),
        "a parallelepiped has six faces; the bake produced {}",
        actual.len()
    );
    for (got, want) in actual.iter().zip(&expected) {
        assert!(
            got.abs_diff_eq(*want, 1e-5),
            "a baked face normal is not along one of the authored image's six face \
             directions: got {got:?} want {want:?}"
        );
    }

    // Outwardness, per face and without reference to the table's winding.
    let points = polyhedron.points();
    for face in polyhedron.faces() {
        let first = face.first_vertex_or_edge as usize;
        let last = first + face.num_vertices_or_edges as usize;
        let mut centre = Vec3::ZERO;
        let mut count = 0.0_f32;
        for vertex in &polyhedron.vertices_adj_to_face()[first..last] {
            centre += points[*vertex as usize];
            count += 1.0;
        }
        assert!(
            count > 0.0,
            "a baked face has no vertices, so its normal points nowhere"
        );
        centre /= count;
        let outward = face.normal.dot(centre);
        assert!(
            outward > 0.0,
            "a baked face points INTO the solid: normal {:?} against a face centre at \
             {centre:?}",
            face.normal
        );
    }
}

/// The authored shear's linear map, and its inverse — computed independently of
/// the runtime's own decomposition so a placement bug cannot agree with itself.
fn shear_linear() -> Mat3 {
    Mat3::from_cols(
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(SHEAR_M as f32, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    )
}

/// Spawns a small dynamic body at `at`, moving at `velocity`, with swept CCD
/// so a 400 m/s arrival is detected at all.
fn spawn_probe(app: &mut App, at: Vec3, velocity: Vec3) -> Entity {
    app.world_mut()
        .spawn((
            RigidBody::Dynamic,
            Collider::cuboid(0.2, 0.2, 0.2),
            Mass(1.0),
            Transform::from_translation(at),
            Position(at),
            Rotation::default(),
            Gravity::ZERO,
            LinearVelocity(velocity),
            SpeculativeMargin::ZERO,
            SweptCcd::default(),
            cs_app::world::fixture::probe_layers(),
        ))
        .id()
}

fn position_of(world: &World, entity: Entity) -> Vec3 {
    world
        .get::<Position>(entity)
        .expect("the body still exists")
        .0
}

// ----------------------------------------------------- 3. the mesh collision ---

/// **The same bake keeps a mesh's stored triangles.** The rule F18-B's mesh
/// path uses is the same function: the stored vertices go through the authored
/// linear map and every stored triangle survives, so an opening in the mesh is
/// still an opening — F18 non-negotiable behavior 1's "never close a traversable
/// opening".
///
/// Observable failure: the bake rebuilds the mesh as a hull (triangles merged,
/// openings closed) or drops faces.
#[test]
fn accept_f18_b_shear_the_same_bake_keeps_every_stored_mesh_triangle() {
    let transform = sheared_transform();
    let placement = AffinePlacement::of(&transform).expect("a shear has an exact placement");
    assert!(placement.is_sheared(), "the fixture must be a shear");

    // An arch-ish shell: three boxes welded into one stored polygon soup, the
    // same shape of content F18-B's harbor mesh holds. Twenty-four corners,
    // twelve quads, thirty-six triangles.
    let source = welded_boxes();
    let baked = bake_shape(&SharedShape::new(source.clone()), shear_linear())
        .expect("a mesh of triangles bakes into a mesh of triangles");

    let result = baked
        .as_trimesh()
        .expect("a baked mesh stays a triangle mesh");
    assert_eq!(
        result.num_triangles(),
        source.num_triangles(),
        "every stored triangle must survive the bake"
    );

    // Every derived vertex is the exact affine image of a stored one.
    for vertex in result.vertices() {
        assert!(
            source
                .vertices()
                .iter()
                .any(|stored_vertex| shear_linear() * *stored_vertex == *vertex),
            "baked vertex {vertex:?} is not the authored map of any stored vertex"
        );
    }

    // And the bake is not a no-op: the baked vertex set differs from the stored
    // one, so the map really moved the shell rather than rebuilding it as is.
    let mut stored_sorted = source.vertices().to_vec();
    stored_sorted.sort_by(partial_order);
    let mut baked_sorted = result.vertices().to_vec();
    baked_sorted.sort_by(partial_order);
    assert_ne!(
        baked_sorted, stored_sorted,
        "the shear must move the shell's vertices; the bake did nothing"
    );
}

/// Three axis-aligned boxes welded into one triangle mesh, sharing corners:
/// a left leg, a right leg and a lintel over the gap between them.
fn welded_boxes() -> TriMesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (centre, half) in [
        (Vec3::new(-2.0, 1.0, 0.0), Vec3::new(0.5, 1.0, 1.0)),
        (Vec3::new(2.0, 1.0, 0.0), Vec3::new(0.5, 1.0, 1.0)),
        (Vec3::new(0.0, 2.5, 0.0), Vec3::new(3.0, 0.5, 1.0)),
    ] {
        let base = vertices.len() as u32;
        for index in 0..8 {
            let sign = |bit: u32| if index & (1 << bit) == 0 { -1.0 } else { 1.0 };
            vertices.push(centre + Vec3::new(sign(0) * half.x, sign(1) * half.y, sign(2) * half.z));
        }
        for quad in [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ] {
            let [a, b, c, d] = quad.map(|corner| base + corner);
            indices.push([a, b, c]);
            indices.push([a, c, d]);
        }
    }
    TriMesh::new(vertices, indices).expect("the welded shell is a valid triangle mesh")
}

// --------------------------------------------------- 4. what is still refused ---

/// **A matrix that mirrors *and* shears is placed, exactly like the unmirrored
/// one.** The built solid's faces point out of *its own* mirrored corner set, so
/// the bake needs no special case for a negative determinant: outwardness is a
/// property of the corner set, which is the authored map's exact image, and the
/// winding the bake emits is unchanged.
///
/// Observable failure: the refusal set creeps back to "not a
/// translation/rotation/scale product", so content that has an exact placement
/// is refused; or the mirror is dropped and the corners come back unmirrored.
#[test]
fn accept_f18_b_shear_a_mirrored_shear_is_placed_with_the_mirrored_geometry() {
    let transform = CanonicalTransform::try_new(
        [[1.0, SHEAR_M, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]],
        PANEL_POS_M,
    )
    .expect("the map is finite");
    assert!(
        transform.mirrored(),
        "the fixture matrix must mirror, or this test proves nothing"
    );

    let mut fixture = WorldFixture::builder(one_panel_world("mirrored", transform))
        .build()
        .expect("a mirrored shear is placeable");
    fixture.step(1);
    let collider_entity = fixture
        .spawned()
        .collider_for(&object("mirrored"))
        .expect("a mirrored shear is collided");
    let collider = fixture
        .world()
        .get::<Collider>(collider_entity)
        .expect("the panel carries a collider");
    let polyhedron = collider
        .shape_scaled()
        .as_convex_polyhedron()
        .expect("a sheared box bakes into a convex polyhedron, mirrored or not");

    let mut baked: Vec<Vec3> = polyhedron.points().to_vec();
    baked.sort_by(partial_order);
    let mut want = authored_local_corners(&transform);
    want.sort_by(partial_order);
    assert_eq!(
        baked, want,
        "the mirror must reach the geometry, not be dropped"
    );
    assert_faces_point_outwards(polyhedron, &transform);
}

/// **A linear map that collapses space is refused, and a mirror alone is
/// not.** The first has no volume, so no collider of it exists; the second is a
/// rotation-times-scale product with a negative scale, which F18-A already
/// placed and this stage still places.
///
/// Observable failure: the refusal set drifts, so either content that used to be
/// placed stops being placed, or content with no exact placement is placed at an
/// approximate pose.
#[test]
fn accept_f18_b_shear_only_a_map_with_no_exact_placement_is_still_refused() {
    // A mirror: representable, placed.
    let mirror = CanonicalTransform::try_new(
        [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        PANEL_POS_M,
    )
    .expect("the mirror is finite");
    let placement = instance_placement(&panel("mirror", mirror))
        .expect("a mirror is a rotation-times-scale product with a negative scale");
    assert!(
        !placement.is_sheared(),
        "a mirror needs no bake: the pose carries it"
    );
    let pose = placement.collider_pose();
    assert!(
        pose.scale.x < 0.0,
        "the negative scale must survive into the pose, got {:?}",
        pose.scale
    );

    // A flat map: no volume, refused by name.
    let flat = CanonicalTransform::try_new(
        [[1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
        PANEL_POS_M,
    )
    .expect("the flat map is finite");
    assert_eq!(
        instance_placement(&panel("flat", flat)).unwrap_err(),
        WorldSpawnError::UnplaceableAffine {
            object: object("flat"),
            source: AffinePlacementError::CollapsesSpace,
        },
        "a map with determinant zero has no collision geometry at all"
    );

    // A shear is the case this stage stopped refusing: it has an exact
    // placement, and the report says so as a number rather than a flag.
    let sheared = AffinePlacement::of(&sheared_transform()).expect("a shear is placeable");
    assert!(sheared.is_sheared());
    let residual = match sheared {
        AffinePlacement::Sheared { residual, .. } => residual,
        AffinePlacement::Trs { .. } => unreachable!("the fixture is sheared"),
    };
    assert!(
        residual > 0.0,
        "the shear must be reported as a measured deviation"
    );
    assert!(
        residual <= shear_residual(canonical_matrix(&sheared_transform())) + f32::EPSILON,
        "the placement's residual is the measurement it reports"
    );
}

/// **A sheared *mesh* is refused by name, before anything exists.** Its exact
/// collision would have to be a **second, derived** upload, whose fingerprint is
/// no longer the authored one — F18-B's one-asset provenance claim, and its
/// decision to make, not this module's. Refusing names the gap; baking it would
/// quietly misattribute a derived mesh to the record's own reference.
///
/// Observable failure: the refusal set widens to sheared **cuboids** as well
/// (the whole point of this stage), or a sheared mesh object is placed from a
/// second upload whose fingerprint is reported as the authored mesh's.
#[test]
fn accept_f18_b_shear_a_sheared_mesh_object_is_refused_rather_than_baked_into_a_second_asset() {
    let definition = mesh_panel_world("meshpanel", sheared_transform());
    let mut app = App::new();
    let before = app.world().entities().len();

    // The record resolves its mesh reference; what is refused is the *shear*,
    // not a missing upload, so the reason must be the shear's own.
    let error = spawn_world(&mut app, &definition, &WorldMeshes::new())
        .expect_err("a sheared mesh object must be refused, not baked from a derived upload");
    assert!(
        matches!(
            &error,
            WorldSpawnError::UnplaceableAffine { object, source }
                if object.as_str() == "meshpanel"
                    && *source == AffinePlacementError::ShearedMeshUndecided
        ),
        "the refusal must name the object and the reason, got {error:?}"
    );
    assert_eq!(
        app.world().entities().len(),
        before,
        "a refused build must leave no entity behind"
    );

    // The same record with a rotation-times-scale matrix is placed, so the
    // refusal is about the shear and not about mesh-derived records.
    let upright = CanonicalTransform::try_new(
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        PANEL_POS_M,
    )
    .expect("the translation is finite");
    let placeable = mesh_panel_world("meshpanel", upright);
    let mut app = App::new();
    spawn_world(&mut app, &placeable, &WorldMeshes::new())
        .expect("an upright mesh object is placeable without an upload to collide with");
}

/// A one-object world whose single object is mesh-derived and carries
/// `transform`.
fn mesh_panel_world(key: &str, transform: CanonicalTransform) -> WorldDefinition {
    let record = WorldObjectInstance::try_new(
        object(key),
        mesh_id(),
        transform,
        known(WorldCollisionRole::Solid, "collision"),
        known(WorldCollisionShape::FromMesh, "shape"),
        known(SurfaceRole::Ground, "surface"),
        vec![SectorId::new("only").expect("the sector key is valid")],
        provenance(key),
    )
    .expect("the sector list has no duplicates");
    WorldDefinition::try_new(
        WorldId::from_key(&format!("test.{key}_world")).expect("the world key is valid"),
        Origin::SyntheticFixture,
        known(WorldBoundary::default(), "boundary"),
        vec![Sector::new(
            SectorId::new("only").expect("the sector key is valid"),
            Aabb::try_new([-20.0, -20.0, -20.0], [20.0, 20.0, 20.0]).expect("the bounds are valid"),
        )],
        vec![record],
        provenance("definition"),
    )
    .expect("the definition is structurally valid")
}

/// **The role contract is unchanged for a sheared object.** A sheared panel
/// with role `None` is still presented and never blocks, and a sheared panel
/// marked `Sensor` is still a sensor: the affine decision never overwrites the
/// role the record declared.
///
/// Observable failure: the bake decides what exists, so a banner acquires a
/// collider or a trigger volume becomes solid.
#[test]
fn accept_f18_b_shear_a_sheared_object_still_follows_its_declared_role() {
    let definition = WorldDefinition::try_new(
        WorldId::from_key("test.sheared_roles_world").expect("the world key is valid"),
        Origin::SyntheticFixture,
        known(WorldBoundary::default(), "boundary"),
        vec![Sector::new(
            SectorId::new("only").expect("the sector key is valid"),
            Aabb::try_new([-20.0, -20.0, -20.0], [20.0, 20.0, 20.0]).expect("the bounds are valid"),
        )],
        vec![
            role_panel("banner", WorldCollisionRole::None, 0.0),
            role_panel("trigger", WorldCollisionRole::Sensor, 4.0),
            role_panel("wall", WorldCollisionRole::Solid, 8.0),
        ],
        provenance("definition"),
    )
    .expect("the definition is structurally valid");
    let fixture = WorldFixture::builder(definition)
        .build()
        .expect("the sheared roles world is placeable");
    let spawned = fixture.spawned().clone();

    let banner = object("banner");
    assert!(spawned.visual_for(&banner).is_some());
    assert!(spawned.collider_for(&banner).is_none());
    assert_eq!(spawned.non_colliding(), [banner]);

    let trigger = object("trigger");
    let trigger_entity = spawned
        .collider_for(&trigger)
        .expect("a sheared sensor is still a collider");
    assert!(
        fixture.world().get::<Sensor>(trigger_entity).is_some(),
        "the bake must not turn a trigger volume into a wall"
    );

    let wall = object("wall");
    let wall_entity = spawned
        .collider_for(&wall)
        .expect("a sheared solid is still a collider");
    assert!(
        fixture.world().get::<Sensor>(wall_entity).is_none(),
        "the bake must not turn a wall into a sensor"
    );
    let layers = *fixture
        .world()
        .get::<AvianCollisionLayers>(wall_entity)
        .expect("every world collider carries the designed layers");
    assert_eq!(layers, cs_app::world::static_world_layers());
}

/// A sheared panel with `role`, offset along `x` so three of them do not touch.
fn role_panel(key: &str, role: WorldCollisionRole, offset_x: f64) -> WorldObjectInstance {
    let transform = CanonicalTransform::try_new(
        [[1.0, SHEAR_M, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        [PANEL_POS_M[0] + offset_x, PANEL_POS_M[1], PANEL_POS_M[2]],
    )
    .expect("the shear is finite");
    WorldObjectInstance::try_new(
        object(key),
        mesh_id(),
        transform,
        known(role, "collision"),
        known(
            WorldCollisionShape::cuboid(PANEL_HALF_M).expect("the box is valid"),
            "shape",
        ),
        known(SurfaceRole::Ground, "surface"),
        vec![SectorId::new("only").expect("the sector key is valid")],
        provenance(key),
    )
    .expect("the sector list has no duplicates")
}

/// **The pre-flight covers every refusal reason, including the one only a bake
/// can find.** `instance_placements` must not merely classify each authored
/// affine: `spawn_world` spawns object by object, so a reason discovered while
/// building the *second* object's collider would leave the first behind with no
/// `SpawnedWorld` to ask which — the exact half-built world
/// `WorldSpawnError` promises cannot happen.
///
/// Observable failure: a `Trs` object is spawned, then a sheared one whose bake
/// is made to fail, and the app is left holding the first object.
#[test]
fn accept_f18_b_shear_the_pre_flight_refuses_before_anything_is_spawned() {
    // Two objects: the first is perfectly placeable, the second is sheared. The
    // world is spawned through the same production entry point the loaders use.
    let upright = CanonicalTransform::try_new(
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        PANEL_POS_M,
    )
    .expect("the rotation-times-scale map is finite");
    let definition = WorldDefinition::try_new(
        WorldId::from_key("test.preflight_world").expect("the world key is valid"),
        Origin::SyntheticFixture,
        known(WorldBoundary::default(), "boundary"),
        vec![Sector::new(
            SectorId::new("only").expect("the sector key is valid"),
            Aabb::try_new([-20.0, -20.0, -20.0], [20.0, 20.0, 20.0]).expect("the bounds are valid"),
        )],
        vec![
            role_panel("first", WorldCollisionRole::Solid, 0.0),
            role_panel("second", WorldCollisionRole::Solid, 8.0),
        ],
        provenance("definition"),
    )
    .expect("the definition is structurally valid");
    // Pre-flight only: classify and bake every instance without spawning.
    let objects: Vec<_> = definition.objects().iter().collect();
    assert!(
        instance_placements(&objects).is_ok(),
        "two sheared cuboid panels are both placeable, so the pre-flight must pass"
    );

    // Now the second object carries the affine with no exact placement, so the
    // refusal happens in the pre-flight and nothing is spawned at all.
    let collapsed = CanonicalTransform::try_new(
        [[1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
        PANEL_POS_M,
    )
    .expect("the collapsed map is finite");
    let refused = WorldDefinition::try_new(
        WorldId::from_key("test.preflight_refused_world").expect("the world key is valid"),
        Origin::SyntheticFixture,
        known(WorldBoundary::default(), "boundary"),
        vec![Sector::new(
            SectorId::new("only").expect("the sector key is valid"),
            Aabb::try_new([-20.0, -20.0, -20.0], [20.0, 20.0, 20.0]).expect("the bounds are valid"),
        )],
        vec![panel("placeable", upright), panel("collapsed", collapsed)],
        provenance("definition"),
    )
    .expect("the definition is structurally valid");
    let mut app = App::new();
    let before = app.world().entities().len();
    assert!(matches!(
        spawn_world(&mut app, &refused, &WorldMeshes::new()),
        Err(WorldSpawnError::UnplaceableAffine { .. })
    ));
    assert_eq!(
        app.world().entities().len(),
        before,
        "a pre-flight refusal must leave no entity behind, including the placeable first object"
    );
}

/// The panel's authored box as the physics shape the bake starts from, read
/// through the production conversion: what `spawn_world` hands to
/// [`bake_shape`].
#[test]
fn accept_f18_b_shear_the_authored_box_is_the_shape_the_bake_starts_from() {
    let placement = AffinePlacement::of(&sheared_transform()).expect("a shear is placeable");
    let authored: SharedShape = SharedShape::new(Cuboid::new(Vec3::new(
        PANEL_HALF_M[0] as f32,
        PANEL_HALF_M[1] as f32,
        PANEL_HALF_M[2] as f32,
    )));
    let baked = placement
        .bake(&authored)
        .expect("the authored box bakes into a solid");
    let polyhedron = baked
        .as_convex_polyhedron()
        .expect("a baked box is a convex polyhedron");
    assert_eq!(
        polyhedron.points().len(),
        8,
        "the affine image of a box has exactly its eight corners"
    );
    // And it is not the authored box: the shear moved the corners.
    let unmoved = polyhedron
        .points()
        .iter()
        .all(|point| point.abs().x <= PANEL_HALF_M[0] as f32 + f32::EPSILON);
    assert!(
        !unmoved,
        "the shear leans the panel past its own half width; the bake did nothing"
    );
}
