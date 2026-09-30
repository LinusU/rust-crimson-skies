//! `accept_t333_` tests for the real Bevy asset stack and the mesh-derived
//! collision it makes possible.
//!
//! Follow-up to `specs/F00-workspace-toolchain-and-first-executable.md` stage
//! `### F00-A` (task #333), resolving
//! `docs/findings/2026-09-23-avian-collider-from-mesh-needs-bevy-asset-stack.md`.
//!
//! F00-A turned Avian's default `collider-from-mesh` feature off because the
//! two systems it registers read `AssetEvent<Mesh>` and `Assets<Mesh>`, which
//! its asset-free headless worlds did not have. This task supplies the real
//! asset stack (`cs_app::asset_stack`) and turns the feature back on, so what
//! is pinned here is:
//!
//! * the coupling is **expressed, not hidden**: a headless world runs on a
//!   real `AssetServer`, and Avian's `ColliderCachePlugin` — which only exists
//!   when the feature is compiled in — is part of the plugin set, so the
//!   feature cannot be switched off again without breaking this file;
//! * a `ColliderConstructorHierarchy` over a `Mesh3d` really produces a
//!   collider, built from the uploaded mesh's own triangles rather than from a
//!   substitute primitive, attached to a rigid body so it is live collision
//!   rather than an inert component;
//! * a mesh with an opening in it keeps that opening: nothing here convex-hulls
//!   or decomposition-substitutes a shape, which is F18 non-negotiable
//!   behavior 1's requirement not to close a traversable opening;
//! * the F00 `SYNTHETIC` scene still runs on that stack and still loads
//!   nothing, so "asset-free" is a property of the stack's contents and not
//!   something the fix quietly abandoned.

use std::time::Duration;

use avian3d::prelude::{Collider, ColliderCachePlugin, RigidBodyColliders};
use bevy::asset::{AssetEvent, AssetServer, Assets};
use bevy::ecs::message::Messages;
use bevy::mesh::{Mesh, Mesh3d, VertexAttributeValues};
use bevy::prelude::{Real, Transform, Vec3};
use bevy::time::{Fixed, Time, TimeUpdateStrategy};
use cs_app::asset_stack::{headless_app, is_attached, spawn_static_mesh_collider};
use cs_app::render::bevy_mesh::upload_group;
use cs_app::synthetic::SyntheticScene;
use cs_content::mesh::{MeshPresentationUnknown, RenderMesh};
use cs_formats::gamez::{PrimitiveKind, RawCorner, RawMesh, RawPolygon};
use cs_sim::collision::{CollisionLayer, CollisionLayers};
use cs_types::{BodyKind, SceneProvenance, SyntheticBodySpec};

/// The eight corners of the unit box the mesh fixtures are built from. Every
/// coordinate is distinct, so a derived trimesh's vertex set can be compared
/// against this list as bit patterns and a substitute shape — a cuboid, a
/// convex hull, a bounding box — cannot pass by coincidence.
const BOX_POSITIONS: [[f32; 3]; 8] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [1.0, 1.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [1.0, 0.0, 1.0],
    [1.0, 1.0, 1.0],
    [0.0, 1.0, 1.0],
];

/// The box's faces as stored polygon outlines, one material group.
///
/// The last entry is `x = 0`, so the first `faces` of this table describe a
/// box with that face missing: an opening. Two triangles per face.
const BOX_FACES: [[u32; 4]; 6] = [
    [0, 1, 2, 3], // z = 0
    [4, 5, 6, 7], // z = 1
    [0, 1, 5, 4], // y = 0
    [1, 2, 6, 5], // x = 1
    [2, 3, 7, 6], // y = 1
    [3, 0, 4, 7], // x = 0
];

/// The presentation questions this synthetic mesh leaves open. They travel
/// with the upload untouched; nothing here settles them.
const MESH_UNKNOWNS: [MeshPresentationUnknown; 2] = [
    MeshPresentationUnknown::FrontFaceWinding,
    MeshPresentationUnknown::UvOrigin,
];

/// How many updates a mesh collider needs: the `ColliderConstructorHierarchy`
/// is a normal `Update` system, and the collider is attached to its body by a
/// pass that can only see it on a later frame.
const SETTLE_UPDATES: usize = 4;

/// Uploads a box with `faces` of [`BOX_FACES`] through the production path:
/// stored records through `RenderMesh::build`, then the F17-B canonical-to-Bevy
/// adapter.
fn uploaded_box(faces: usize) -> (Mesh, usize) {
    let polygons = BOX_FACES[..faces]
        .iter()
        .map(|face| RawPolygon {
            kind: PrimitiveKind::Polygon,
            raw_flags: 0,
            material: 0,
            corners: face
                .iter()
                .map(|position| RawCorner {
                    position: *position,
                    normal: None,
                    uv: None,
                    color: None,
                })
                .collect(),
        })
        .collect();
    let render = RenderMesh::build(&RawMesh {
        positions: BOX_POSITIONS.to_vec(),
        normals: Vec::new(),
        polygons,
    })
    .expect("the authored box has a decodable outline");
    let upload = upload_group(&render, 0, &MESH_UNKNOWNS).expect("a bare box uploads");
    let triangles = upload.report().triangles;
    assert_eq!(
        triangles,
        faces * 2,
        "one stored quad decodes to two triangles, so the fixture's triangle \
         count is the one the collider is checked against"
    );
    (upload.into_mesh(), triangles)
}

/// The positions an uploaded mesh handed to the asset stack, as bit patterns,
/// so no comparison in this file depends on float equality.
fn uploaded_positions(mesh: &Mesh) -> Vec<[u32; 3]> {
    match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(values)) => {
            values.iter().map(|value| value.map(f32::to_bits)).collect()
        }
        other => panic!("positions are Float32x3, got {other:?}"),
    }
}

/// The vertices of a derived collider's trimesh, as bit patterns.
fn collider_vertices(collider: &Collider) -> Vec<[u32; 3]> {
    let trimesh = collider
        .shape()
        .as_trimesh()
        .expect("TrimeshFromMesh must produce a triangle mesh, not a substitute primitive");
    trimesh
        .vertices()
        .iter()
        .map(|point| [point.x, point.y, point.z].map(f32::to_bits))
        .collect()
}

/// The triangle count of a derived collider's trimesh. `TriMesh::indices`
/// yields one `[u32; 3]` per triangle, so its length is already the count.
fn collider_triangles(collider: &Collider) -> usize {
    collider
        .shape()
        .as_trimesh()
        .expect("a mesh-derived collider is a triangle mesh")
        .indices()
        .len()
}

/// Builds a headless world and prepares it to be driven by whole fixed ticks,
/// the same way every other `cs_app` fixture does, so nothing in this file
/// depends on wall time.
fn ticked_app() -> bevy::app::App {
    let mut app = headless_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 64.0,
    )));
    app.insert_resource(Time::<Fixed>::from_seconds(1.0 / 64.0));
    let startup = app.world().resource::<Time<Real>>().startup();
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .update_with_instant(startup);
    app.finish();
    app.cleanup();
    app
}

/// The asset stack a headless world runs on is a real asset system, and it is
/// exactly what Avian's `collider-from-mesh` systems read.
///
/// Observable failure if either half goes: with the stack gone the first
/// update aborts ("Message not initialized" / "Resource does not exist") and
/// the assertions below fail; with the feature off, `ColliderCachePlugin`
/// stops existing and this file stops compiling — which is why the plugin is
/// named rather than described in a comment.
#[test]
fn accept_t333_a_headless_world_runs_the_asset_stack_collider_from_mesh_reads() {
    let app = ticked_app();

    assert!(
        app.is_plugin_added::<ColliderCachePlugin>(),
        "PhysicsPlugins::default() must be the pinned group with \
         `collider-from-mesh` compiled in: without the feature the group has no \
         ColliderCachePlugin to add"
    );
    assert!(
        app.world().contains_resource::<AssetServer>(),
        "a real AssetServer must exist: a bare Assets<Mesh> would satisfy the \
         readers while no asset system backs them"
    );
    assert!(
        app.world().contains_resource::<Assets<Mesh>>(),
        "init_asset::<Mesh>() must have run: clear_unused_colliders and \
         init_collider_constructor_hierarchies both read Assets<Mesh>"
    );
    assert!(
        app.world()
            .contains_resource::<Messages<AssetEvent<Mesh>>>(),
        "AssetEvent<Mesh> must be a real message: ColliderCachePlugin's \
         clear_unused_colliders takes a MessageReader<AssetEvent<Mesh>>"
    );
}

/// A `ColliderConstructorHierarchy` over a `Mesh3d` produces a collider built
/// from the mesh's own triangles, attached to a body.
///
/// Observable failure if the derivation is replaced or skipped: a collider that
/// is not a trimesh, or that has the wrong triangle count, was not built from
/// the uploaded index buffer; an unattached collider collides with nothing,
/// however convincing its shape.
#[test]
fn accept_t333_collider_constructor_hierarchy_builds_a_collider_from_the_uploaded_mesh() {
    let (mesh, triangles) = uploaded_box(BOX_FACES.len());
    let allowed = uploaded_positions(&mesh);

    let mut app = ticked_app();
    let node = spawn_static_mesh_collider(
        &mut app,
        mesh,
        Transform::from_translation(Vec3::new(2.0, 0.0, 0.0)),
        CollisionLayers::from(CollisionLayer::StaticWorld),
    );
    for _ in 0..SETTLE_UPDATES {
        app.update();
    }

    // The collider must come from the asset the node holds, not from a
    // substitute inserted alongside it.
    let handle = app
        .world()
        .get::<Mesh3d>(node.node)
        .expect("a mesh collider node holds the Mesh3d the constructor reads")
        .0
        .clone();
    assert!(
        app.world()
            .resource::<Assets<Mesh>>()
            .get(&handle)
            .is_some(),
        "the constructor's handle must resolve in the world's asset stack"
    );

    let collider = app
        .world()
        .get::<Collider>(node.node)
        .expect("a ColliderConstructorHierarchy over a Mesh3d must produce a Collider");
    assert_eq!(
        collider_triangles(collider),
        triangles,
        "the collider must carry exactly the triangles the uploaded mesh stored"
    );

    // Every derived vertex is one of the positions the adapter handed over,
    // bit-exact: nothing was re-centered, scaled or regenerated on the way
    // into the collider. And every corner is present, so a collider built from
    // one triangle of the mesh cannot pass.
    let derived = collider_vertices(collider);
    for vertex in &derived {
        assert!(
            allowed.contains(vertex),
            "collider vertex {vertex:?} is not one of the uploaded positions"
        );
    }
    for corner in BOX_POSITIONS.map(|position| position.map(f32::to_bits)) {
        assert!(
            derived.contains(&corner),
            "collider is missing the uploaded corner {corner:?}"
        );
    }

    assert!(
        is_attached(app.world(), &node),
        "the derived collider must be attached to the static body, or it \
         collides with nothing"
    );
    assert!(
        app.world()
            .get::<RigidBodyColliders>(node.body)
            .is_some_and(|colliders| colliders.len() == 1),
        "the body must list exactly the one derived collider"
    );
}

/// A mesh with a face missing keeps that opening: the derived collider holds
/// the stored triangles and nothing else.
///
/// F18 non-negotiable behavior 1 forbids closing a traversable opening through
/// simplification. Observable failure if the shape were substituted: a convex
/// hull or decomposition of these eight corners has 12 triangles, so the
/// missing face — 10 stored triangles — would be filled in and the count would
/// not match.
#[test]
fn accept_t333_a_mesh_with_an_opening_keeps_it_in_the_derived_collider() {
    const FACES: usize = BOX_FACES.len() - 1;
    const HULL_TRIANGLES: usize = BOX_FACES.len() * 2;

    let (mesh, triangles) = uploaded_box(FACES);
    assert_eq!(triangles, 10, "the open fixture really is missing one face");

    let mut app = ticked_app();
    let node = spawn_static_mesh_collider(
        &mut app,
        mesh,
        Transform::IDENTITY,
        CollisionLayers::from(CollisionLayer::StaticWorld),
    );
    for _ in 0..SETTLE_UPDATES {
        app.update();
    }

    let collider = app
        .world()
        .get::<Collider>(node.node)
        .expect("an open mesh still derives a collider");
    assert_eq!(
        collider_triangles(collider),
        triangles,
        "the derived collider must keep the {triangles} stored triangles, not \
         the {HULL_TRIANGLES} a convex hull of the same corners would have"
    );
}

/// The F00 `SYNTHETIC` scene still runs on the asset stack the feature
/// requires, and still loads nothing through it.
///
/// Observable failure if the fix quietly gave the scene content, or if the
/// scene were moved back to a world without the stack: the first shows up in
/// `mesh_asset_count`, the second aborts the first update.
#[test]
fn accept_t333_synthetic_scene_runs_on_the_asset_stack_and_still_loads_nothing() {
    let mut scene = SyntheticScene::new(SyntheticBodySpec::falling_box(BodyKind::Dynamic))
        .expect("the fixture spec must build a scene");

    assert_eq!(
        scene.provenance(),
        SceneProvenance::Synthetic,
        "the dev scene must stay explicitly marked SYNTHETIC"
    );
    assert_eq!(
        scene.mesh_asset_count(),
        0,
        "the scene loads no assets: the asset stack exists for Avian's systems, \
         not so the scene can hold geometry the F00 fixture does not describe"
    );

    let start = scene.spec().position_m;
    scene.step(60);
    let sample = scene.sample();
    assert!(
        sample.position_m[1] < start[1] - 1.0,
        "the scene must still integrate: a body that stopped falling means the \
         world stopped stepping, not that it is still asset-free"
    );
    assert_eq!(
        scene.mesh_asset_count(),
        0,
        "sixty ticks of simulation must not have loaded an asset either"
    );
}
