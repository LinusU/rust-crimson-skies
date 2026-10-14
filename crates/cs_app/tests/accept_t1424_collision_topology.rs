//! A world collider is built from the **canonical** collision topology, not
//! from the drawn (fan) index buffer — task #1424.
//!
//! The drawn buffer is not an arbitrary choice: it is the original renderer's
//! measured `D3DPT_TRIANGLEFAN` outline fanned from corner 0
//! (`OWNER-STATIC-2026-10-05`, `RawMesh::draw_topology`), which is exactly what
//! the renderer must submit. It is also the wrong thing to collide with. A fan
//! triangle that folds against its own outline is a leaf all three of whose
//! corners parry's `Triangle::local_support_point` can tie on (it returns `c`),
//! `closest_points_cuboid_triangle` trusts that tied pick as the closest
//! vertex, and `compute_toi`'s bisection lands mid-window: a phantom time of
//! impact at half a tick, paid for by the solver clamping travel with no
//! contact. The corner-0 emission order F10-D.1 picked dodges that on the
//! measured fixtures and nothing more — any leaf set can trip it under any
//! rotation, which is why the fix is to stop sharing the buffer.
//!
//! The fixture below makes the two views differ *on purpose*: a dart whose
//! reflex corner sits where the corner-0 fan folds over the notch the outline
//! leaves open. Everything asserted here is computed from the stored
//! [`RawMesh`]'s own two views, never from the code under test's opinion of
//! them:
//!
//! * the content side's collision view **is** `RawMesh::topology()`, and the
//!   drawn view **is** `RawMesh::draw_topology()` — the two contracts;
//! * the derived collider's triangles are the canonical view and are **not** the
//!   fan, so a regression back to `TrimeshFromMesh` fails here;
//! * the asset the entity presents from still carries the fan, so decoupling
//!   collision changed no rendering;
//! * an outline the two views disagree about completely — one the fan draws and
//!   the ear clipper refuses — is presented and *reported*, because Avian's
//!   explicit triangle-mesh constructors panic on an empty index buffer.

use std::collections::BTreeSet;

use avian3d::prelude::Collider;
use bevy::asset::Assets;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::{Mesh, Mesh3d};
use cs_app::world::{SkipReason, WorldMeshes, spawn_world, world_app};
use cs_content::mesh::RenderMesh;
use cs_content::world::{
    Aabb, Sector, SectorId, SurfaceRole, WorldCollisionRole, WorldCollisionShape, WorldDefinition,
    WorldId, WorldObjectId, WorldObjectInstance,
};
use cs_formats::gamez::{MeshTopology, PrimitiveKind, RawCorner, RawMesh, RawPolygon};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

/// How many updates the headless world needs before Avian has derived the
/// colliders the spawn asked for: `init_collider_constructors` runs in
/// `Update`, so the first update after the spawn is the earliest a `Collider`
/// exists.
const SETTLE_UPDATES: u64 = 4;

/// The dart: four stored corners at `y = 0`, in `(x, z)`, with the reflex
/// corner at index 3.
///
/// Fanned from corner 0 the outline becomes `(0,1,2)` and `(0,2,3)`; the first
/// of those covers the notch at corner 3, so the two fan triangles overlap and
/// cover ground the outline never encloses. Ear-clipping —
/// `RawMesh::topology()` — must use the other diagonal, `(1,3)`, and produce
/// the two triangles the outline actually describes.
const DART: [[f32; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [2.0, 0.0, 2.0],
    [4.0, 0.0, 0.0],
    [2.0, 0.0, 1.0],
];

/// The stored mesh: one four-corner outline polygon, no normals, one material
/// index. Nothing here is authored for the test's convenience beyond being a
/// legal `RawMesh` — the fan and the ear-clip are what the production readers
/// compute from it.
fn dart_raw_mesh() -> RawMesh {
    RawMesh {
        positions: DART.to_vec(),
        normals: Vec::new(),
        polygons: vec![RawPolygon {
            kind: PrimitiveKind::Polygon,
            raw_flags: 0,
            material: 0,
            corners: (0..DART.len())
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

/// One triangle as the **sorted** bit patterns of its three corners, so two
/// index buffers that name the same geometry compare equal however they order,
/// rotate or duplicate their corners.
type CornerSet = [[u32; 3]; 3];

fn bits(position: [f32; 3]) -> [u32; 3] {
    position.map(f32::to_bits)
}

fn corners(positions: &[[f32; 3]], triangle: [u32; 3]) -> CornerSet {
    let mut corners = triangle.map(|index| {
        let position = *positions
            .get(index as usize)
            .expect("a topology triangle only names positions the mesh holds");
        bits(position)
    });
    corners.sort_unstable();
    corners
}

/// Every triangle of one index buffer, as [`CornerSet`]s.
fn triangles(positions: &[[f32; 3]], indices: &[[u32; 3]]) -> BTreeSet<CornerSet> {
    indices
        .iter()
        .map(|&triangle| corners(positions, triangle))
        .collect()
}

/// The triangles of one of the stored mesh's two views, straight from
/// `cs_formats`.
fn view(raw: &RawMesh, topology: MeshTopology) -> BTreeSet<CornerSet> {
    topology
        .triangles
        .iter()
        .map(|triangle| corners(&raw.positions, triangle.positions))
        .collect()
}

/// The triangles `RenderMesh` will hand the renderer.
fn drawn(render: &RenderMesh) -> BTreeSet<CornerSet> {
    let positions: Vec<[f32; 3]> = render
        .vertices()
        .iter()
        .map(|vertex| vertex.position)
        .collect();
    let indices: Vec<[u32; 3]> = render
        .triangles()
        .iter()
        .map(|triangle| triangle.vertices)
        .collect();
    triangles(&positions, &indices)
}

/// The triangles of a derived collider, as parry built them.
fn collider(collider: &Collider) -> BTreeSet<CornerSet> {
    let trimesh = collider
        .shape()
        .as_trimesh()
        .expect("a mesh-derived collider is a triangle mesh, not a substitute primitive");
    let positions: Vec<[f32; 3]> = trimesh
        .vertices()
        .iter()
        .map(|point| [point.x, point.y, point.z])
        .collect();
    triangles(&positions, trimesh.indices())
}

fn provenance(key: &str) -> Provenance {
    Provenance::designed(ClaimId::new(&format!("t1424.{key}")).expect("the claim id is valid"))
}

fn known<T>(value: T, key: &str) -> Resolved<T> {
    Resolved::Known(Known::new(value, provenance(key)))
}

fn object() -> WorldObjectId {
    WorldObjectId::new("dart.panel").expect("the test object key is valid")
}

/// A one-object world whose single instance collides from `id`'s geometry.
fn dart_world(id: &ContentId) -> WorldDefinition {
    let sector = SectorId::new("only").expect("the sector key is valid");
    let instance = WorldObjectInstance::try_new(
        object(),
        known(id.clone(), "mesh"),
        cs_content::scene::CanonicalTransform::try_new(
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            [0.0, 0.0, 0.0],
        )
        .expect("the identity is exact"),
        known(WorldCollisionRole::Solid, "collision"),
        known(WorldCollisionShape::FromMesh, "shape"),
        known(SurfaceRole::Ground, "surface"),
        vec![sector.clone()],
        provenance("instance"),
    )
    .expect("the sector list has no duplicates");

    WorldDefinition::try_new(
        WorldId::from_key("test.dart_world").expect("the world key is valid"),
        Origin::SyntheticFixture,
        known(cs_content::world::WorldBoundary::default(), "boundary"),
        vec![Sector::new(
            sector,
            Aabb::try_new([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]).expect("the bounds are valid"),
        )],
        vec![instance],
        provenance("definition"),
    )
    .expect("the definition is structurally valid: one sector, one object")
}

/// **The collider a world object is stopped by is `RawMesh::topology()`, and the
/// asset it draws from is still `RawMesh::draw_topology()`.**
///
/// Observable failure if the two views are allowed to share an index buffer
/// again: the derived collider's triangles become the corner-0 fan's — the
/// folding pair `(0,1,2)`, `(0,2,3)` — and this test fails on the very outline
/// that makes the difference visible, not on some incidental triangle count.
/// It also fails the other way round, if "fixing" the collider had quietly
/// re-ordered what the renderer draws.
#[test]
fn accept_t1424_a_world_collider_is_the_canonical_topology_and_the_drawn_asset_is_still_the_fan() {
    let raw = dart_raw_mesh();
    let render = RenderMesh::build(&raw).expect("the dart's single outline decodes");

    let fan = view(&raw, raw.draw_topology());
    let canonical = view(&raw, raw.topology());
    assert_ne!(
        fan, canonical,
        "the fixture must be one this task can actually tell apart: the corner-0 fan \
         and the ear-clipped outline have to be different triangle sets"
    );
    assert_eq!(
        fan.len(),
        canonical.len(),
        "both views of a four-corner outline are two triangles, so the difference is \
         which diagonal, not how many"
    );

    // The content side's contract: one built mesh, two views, each the view it
    // says it is.
    assert_eq!(
        triangles(render.collision().positions(), render.collision().indices()),
        canonical,
        "the collision view of a render mesh must be `RawMesh::topology()`, the view \
         cs_formats documents as the collision one"
    );
    assert_eq!(
        drawn(&render),
        fan,
        "the render mesh must still be the measured corner-0 fan: decoupling collision \
         may not change what the renderer draws"
    );

    // And now the whole production path: adapter, source, spawn, Avian.
    let id = ContentId::from_source(ContentKind::Mesh, "synthetic.dart_panel")
        .expect("the mesh id is valid");
    let mut meshes = WorldMeshes::new();
    meshes
        .insert_render_mesh(id.clone(), &render, &[])
        .expect("a single material group with no per-corner attribute uploads");
    let definition = dart_world(&id);

    let mut app = world_app();
    let report = spawn_world(&mut app, &definition, &meshes).expect("the dart world spawns");
    for _ in 0..SETTLE_UPDATES {
        app.update();
    }

    let collider_entity = report
        .collider_for(&object())
        .expect("a `Solid` `FromMesh` object is collided");
    let derived = app
        .world()
        .get::<Collider>(collider_entity)
        .expect("Avian derived the constructor the spawn asked for");
    let built = collider(derived);
    assert_eq!(
        built, canonical,
        "a world collider must be built from `RawMesh::topology()`, not from the drawn \
         fan"
    );
    assert_ne!(
        built, fan,
        "the collider is still the corner-0 fan, so it still shares the drawn index \
         buffer — task #1424's artefact is back"
    );

    // Rendering is untouched: the entity presents from an asset that still
    // carries exactly the fan.
    let world = app.world();
    let handle = world
        .get::<Mesh3d>(collider_entity)
        .expect("a mesh object presents from the entity it collides on")
        .0
        .clone();
    let asset = world
        .resource::<Assets<Mesh>>()
        .get(&handle)
        .expect("the presented handle resolves in the world's asset stack");
    let positions = match asset.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(values)) => values.clone(),
        other => panic!("the dart uploads positions, saw {other:?}"),
    };
    let indices = match asset.indices() {
        Some(bevy::mesh::Indices::U32(values)) => {
            let (triangles, rest) = values.as_chunks::<3>();
            assert!(
                rest.is_empty(),
                "an index buffer is whole triangles, saw {} stray indices",
                rest.len()
            );
            triangles.to_vec()
        }
        other => panic!("the dart uploads a u32 index buffer, saw {other:?}"),
    };
    assert_eq!(
        triangles(&positions, &indices),
        fan,
        "the presented asset must still be the drawn fan: render fidelity and collision \
         geometry are two views of one mesh, not one buffer with two consumers"
    );
}

/// A four-corner outline whose edges cross: the corner-0 fan spells it into two
/// triangles, and an ear clipper refuses to triangulate it
/// (`FaceIssue::UnsupportedNgon`, reason `ZeroArea` — measured on this exact
/// outline). The two views of one stored mesh therefore disagree about it in
/// the one direction that matters here: it draws, and it has no collision
/// triangulation.
const BOWTIE: [[f32; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [2.0, 0.0, 2.0],
    [2.0, 0.0, 0.0],
    [0.0, 0.0, 2.0],
];

fn bowtie_raw_mesh() -> RawMesh {
    RawMesh {
        positions: BOWTIE.to_vec(),
        normals: Vec::new(),
        polygons: vec![RawPolygon {
            kind: PrimitiveKind::Polygon,
            raw_flags: 0,
            material: 0,
            corners: (0..BOWTIE.len())
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

/// **A mesh whose collision view carries no triangle is presented and reported,
/// never spawned as a solid that could not be built.**
///
/// The two views are allowed to disagree about a mesh — that is the point of
/// having them — and the honest outcome of that disagreement has to be a
/// visible gap. The failure this pins is a panic: Avian's explicit
/// `Trimesh*` constructors **panic** on an empty index buffer, where the old
/// `TrimeshFromMesh` returned `None` and logged, so a world load would take the
/// whole engine down on one outline instead of reporting one object.
#[test]
fn accept_t1424_a_mesh_whose_collision_view_is_empty_is_reported_and_never_spawned_solid() {
    let raw = bowtie_raw_mesh();
    let render =
        RenderMesh::build(&raw).expect("the render gate accepts what the original would fan");

    assert_eq!(
        render.triangles().len(),
        2,
        "the fixture must be one the render gate builds — this is about the collision \
         view, not about a mesh nothing draws"
    );
    assert!(
        render.collision().is_empty(),
        "and the canonical view must have nothing to offer it, or the guard below is \
         never reached"
    );

    let id = ContentId::from_source(ContentKind::Mesh, "synthetic.bowtie_panel")
        .expect("the mesh id is valid");
    let mut meshes = WorldMeshes::new();
    meshes
        .insert_render_mesh(id.clone(), &render, &[])
        .expect("a single material group with no per-corner attribute uploads");
    let definition = dart_world(&id);

    let mut app = world_app();
    let report = spawn_world(&mut app, &definition, &meshes)
        .expect("a mesh with no collision triangulation is a gap, not a spawn error");
    for _ in 0..SETTLE_UPDATES {
        app.update();
    }

    let skipped: Vec<_> = report
        .skipped()
        .into_iter()
        .filter(|entry| entry.object == object())
        .collect();
    assert_eq!(
        skipped.len(),
        1,
        "exactly one skip, for exactly this object, saw {skipped:?}"
    );
    assert_eq!(
        skipped[0].reason,
        SkipReason::CollisionTopologyUnavailable,
        "the report has to name *why* no collider exists rather than either inventing \
         one or staying silent"
    );
    assert!(
        report.collider_for(&object()).is_none(),
        "and no collider entity may exist for a shape nobody could triangulate"
    );
    assert!(
        report.visual_for(&object()).is_some(),
        "the object is still presented: a visible hole is the point of the report"
    );
}
