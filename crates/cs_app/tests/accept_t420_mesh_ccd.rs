//! Task #420 (F18-B follow-up): how world collision detects a body that
//! outruns its own sampling — the measured engine behaviour and the layout the
//! decision is built on.
//!
//! F18-B measured a real gap: a body swept at 400 m/s passes through a
//! `TrimeshFromMesh` collider with an empty contact log while the same probe
//! is clamped by a cuboid of the same thickness. Its recorded attribution —
//! "parry's `cast_shapes` has no `TriMesh` case, so the cast returns
//! `Unsupported`" — is **wrong**, and these tests pin the corrected cause:
//!
//! * parry 0.27's `cast_shapes`/`cast_shapes_nonlinear` route a `TriMesh`
//!   through the composite-shape branch (`TriMesh::as_composite_shape`
//!   returns `Some(self)`, `parry3d-0.27.0/src/shape/shape.rs:1141`) and do
//!   produce times of impact; a cuboid is *not* special to parry either.
//! * The actual defect is Avian's `solve_swept_ccd`
//!   (`avian3d-0.7.0/src/dynamics/ccd/mod.rs`): `SweptCcdBodyQuery` requires
//!   `collider: &'static Collider` **on the body entity** (line 510). A
//!   collider attached to a child node — the layout every
//!   `ColliderConstructorHierarchy` build produces — makes the body fail that
//!   query, so the pair is skipped and no cast is ever attempted. Shape is
//!   irrelevant: a *cuboid* on a child node tunnels exactly like a trimesh.
//! * The decided shipping layout — one entity carrying `RigidBody` +
//!   `Mesh3d` + `ColliderConstructor::TrimeshFromMesh` — puts the derived
//!   collider on the body entity itself: the same uploaded mesh, every
//!   stored triangle, and swept CCD sees it.
//! * `SubstepCount` is *not* a fix: substeps subdivide the solver, not the
//!   detection pipeline; measured at 2/4/8 substeps the probe still tunnels.
//! * A nonzero `SpeculativeMargin` *would* stop the miss — the speculative
//!   narrow phase has a `TriMesh` case — but only once the margin is large
//!   enough to matter, and only by predicting contacts ahead of the body. It is
//!   pinned here as a rejected alternative, so the cost the decision record
//!   quotes stays checkable.
//!
//! Everything below runs the production composition: the
//! `cs_app::asset_stack` headless world, the `cs_app::physics` fixed-rate
//! adapter, `cs_app::world::spawn_swept_probe` (the same `SweptCcd` +
//! `SpeculativeMargin::ZERO` body F18-A/B measure with), and
//! `cs_app::asset_stack::spawn_static_mesh_collider` for the
//! collider-on-child arm and `spawn_static_mesh_collider_on_body` — the
//! helper `world/spawn` itself calls — for the decided-layout arm. No
//! original data is used.
//!
//! Re-measure signal: when a released Avian makes `SweptCcdBodyQuery` not
//! require `Collider` on the body entity (upstream `main` already rewrote
//! swept CCD to iterate `RigidBodyColliders`), the pinning assertions about
//! child-node colliders fail — that failure is the cue to re-measure, not a
//! regression. See `docs/findings/2026-09-30-t420-mesh-ccd-decision.md`.

use std::time::Duration;

use avian3d::prelude::{
    Collider, ColliderAabb, Gravity, Position, RigidBody, SpeculativeMargin, SubstepCount, SweptCcd,
};
use bevy::asset::RenderAssetUsages;
use bevy::math::Vec3;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::prelude::{App, ChildOf, Entity, Transform};
use bevy::time::{Real, Time, TimeUpdateStrategy};
use cs_app::asset_stack::{
    MeshColliderNode, headless_app, is_attached, spawn_static_mesh_collider,
    spawn_static_mesh_collider_on_body,
};
use cs_app::physics::{BASELINE_FIXED_HZ, PhysicsAdapterPlugin};
use cs_app::world::{ProbeSpec, spawn_discrete_probe, spawn_swept_probe, static_world_layers};
use cs_sim::collision::{CollisionLayer, CollisionLayers};

/// The probe's start, size and flight line — the same numbers the F18-A/B
/// sweeps use so the results are comparable.
const PROBE_START_X_M: f64 = -12.0;
const PROBE_Y_M: f64 = 1.5;
const PROBE_Z_M: f64 = 1.5;
const PROBE_HALF_M: f64 = 0.25;
const TUNNELLING_SPEED_M_S: f64 = 400.0;
/// The wall's thickness on `x`: 1 m — thin enough that one 3.33 m tick outruns
/// it. Every arm of the 2x2 uses this thickness, so the shape arms differ in
/// nothing but the shape and the placement arms differ in nothing but the
/// placement, which is what makes the matrix discriminate at all.
const WALL_THICKNESS_M: f64 = 1.0;

/// The headless world composition the world fixtures run: the real asset
/// stack (Avian's collider-from-mesh needs it), the real fixed-rate adapter,
/// a manual clock producing one 120 Hz step per update, gravity zero.
fn t420_app(substeps: u32) -> App {
    let mut app = headless_app();
    app.add_plugins(PhysicsAdapterPlugin::new(BASELINE_FIXED_HZ));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / BASELINE_FIXED_HZ as f64,
    )));
    app.insert_resource(SubstepCount(substeps));
    app.insert_resource(Gravity::ZERO);
    // Seed the clock baseline so the first update produces the full manual
    // delta and exactly one fixed step (t334).
    let startup = app.world().resource::<Time<Real>>().startup();
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .update_with_instant(startup);
    app.finish();
    app.cleanup();
    app
}

/// The wall mesh: an open box, `x ∈ [-0.5, 0.5]`, `y ∈ [0, 4]`,
/// `z ∈ [-2, 2]` — two quads, four triangles, an open top and bottom so no
/// convex substitution can fake it. Winding gives outward-pointing normals
/// (−x on the near face): a discrete contact must push a body back out, not
/// drag it into the slab.
fn wall_mesh() -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-0.5_f32, 0.0, -2.0],
            [-0.5, 4.0, -2.0],
            [-0.5, 4.0, 2.0],
            [-0.5, 0.0, 2.0],
            [0.5, 0.0, -2.0],
            [0.5, 4.0, -2.0],
            [0.5, 4.0, 2.0],
            [0.5, 0.0, 2.0],
        ],
    );
    mesh.insert_indices(Indices::U32(vec![0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7]));
    mesh
}

/// The wall as raw collider geometry, for the arms that do not derive from a
/// `Mesh` asset.
fn wall_trimesh_collider() -> Collider {
    let vertices = vec![
        Vec3::new(-0.5, 0.0, -2.0),
        Vec3::new(-0.5, 4.0, -2.0),
        Vec3::new(-0.5, 4.0, 2.0),
        Vec3::new(-0.5, 0.0, 2.0),
        Vec3::new(0.5, 0.0, -2.0),
        Vec3::new(0.5, 4.0, -2.0),
        Vec3::new(0.5, 4.0, 2.0),
        Vec3::new(0.5, 0.0, 2.0),
    ];
    let indices = vec![[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7]];
    Collider::trimesh(vertices, indices)
}

/// A static cuboid wall spanning exactly what the trimesh fixture spans, for the
/// shape-comparison arms: [`Collider::cuboid`] takes *half* extents, so this is
/// the same 1 m thickness, 4 m tall and 4 m deep as [`wall_mesh`].
fn wall_cuboid_collider() -> Collider {
    Collider::cuboid(WALL_THICKNESS_M as f32 / 2.0, 4.0, 4.0)
}

/// The decided shipping layout, through the production helper world import
/// uses: one entity is the static body *and* the mesh node, so the collider
/// `TrimeshFromMesh` derives lands on the body itself and swept CCD sees it.
fn wall_mesh_on_body(app: &mut App) -> Entity {
    spawn_static_mesh_collider_on_body(
        app,
        wall_mesh(),
        Transform::from_xyz(0.0, 0.0, 0.0),
        CollisionLayers::from(CollisionLayer::StaticWorld),
    )
}

/// The same trimesh collider written directly on a static body entity.
fn wall_trimesh_on_body(app: &mut App) -> Entity {
    app.world_mut()
        .spawn((
            RigidBody::Static,
            wall_trimesh_collider(),
            static_world_layers(),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Position::new(Vec3::ZERO),
        ))
        .id()
}

/// A static body whose only collider is a *cuboid* on a child node: the
/// support-map control that isolates placement from shape. Returned as a
/// [`MeshColliderNode`] so the tests can assert the child collider is really
/// attached to the body — a tunnel caused by an unattached collider would
/// prove nothing about swept CCD.
fn wall_cuboid_on_child(app: &mut App) -> MeshColliderNode {
    let body = app
        .world_mut()
        .spawn((
            RigidBody::Static,
            Transform::default(),
            Position::new(Vec3::ZERO),
        ))
        .id();
    let node = app
        .world_mut()
        .spawn((
            wall_cuboid_collider(),
            static_world_layers(),
            ChildOf(body),
            Transform::from_xyz(0.0, 2.0, 0.0),
        ))
        .id();
    MeshColliderNode { body, node }
}

/// A cuboid collider directly on a static body — the F18-A arch layout.
fn wall_cuboid_on_body(app: &mut App) -> Entity {
    app.world_mut()
        .spawn((
            RigidBody::Static,
            wall_cuboid_collider(),
            static_world_layers(),
            Transform::from_xyz(0.0, 2.0, 0.0),
            Position::new(Vec3::new(0.0, 2.0, 0.0)),
        ))
        .id()
}

fn tunnelling_probe() -> ProbeSpec {
    ProbeSpec {
        position_m: [PROBE_START_X_M, PROBE_Y_M, PROBE_Z_M],
        velocity_m_s: [TUNNELLING_SPEED_M_S, 0.0, 0.0],
        half_extents_m: [PROBE_HALF_M, PROBE_HALF_M, PROBE_HALF_M],
        mass_kg: 250.0,
    }
}

/// Builds the app, places `wall`, lets the world settle, fires the swept
/// probe and returns its final position after `ticks` fixed steps.
fn sweep<W>(substeps: u32, wall: impl FnOnce(&mut App) -> W, ticks: u64) -> (App, W, Entity) {
    let mut app = t420_app(substeps);
    let wall_e = wall(&mut app);
    // Let the collider constructor derive and the broad phase register the
    // wall before the probe exists.
    for _ in 0..6 {
        app.update();
    }
    let probe_e =
        spawn_swept_probe(&mut app, &tunnelling_probe()).expect("the probe spec is valid");
    for _ in 0..ticks {
        app.update();
    }
    (app, wall_e, probe_e)
}

/// Where the probe ended up, for the stopped/tunnelled arms to compare.
///
/// The assertions read `x` against the wall's span rather than against an
/// exact position, and that is deliberate: Avian's swept CCD truncates the
/// clamping tick's translation exactly — the probe lands on the wall's near
/// face less its own half, `x = -0.7496` on the tick it is stopped — but it
/// leaves the body's velocity alone, so from the next tick the discrete narrow
/// phase resolves the remaining contact against one of this fixture's
/// zero-thickness faces and the body creeps along it. Measured, `y` and `z`
/// each rise about 0.05 m per tick afterwards, ending near `x = -0.92`. A
/// property of the open two-quad wall mesh, not of the swept path, which is
/// why "before the wall" and "past the wall" is what the arms assert.
fn end_position(app: &App, entity: Entity) -> Vec3 {
    app.world()
        .get::<Position>(entity)
        .expect("the probe still exists")
        .0
}

/// The wall's measured span and the one-tick travel, asserted so the test
/// cannot silently stop measuring a tunnel. Every arm's wall is
/// [`WALL_THICKNESS_M`] thick, so this holds for the trimesh and the cuboid
/// arms alike rather than only for the one the constant was written for.
fn precondition(app: &App) {
    let dt = app
        .world()
        .resource::<Time<bevy::time::Fixed>>()
        .timestep()
        .as_secs_f32();
    let step_m = TUNNELLING_SPEED_M_S as f32 * dt;
    assert!(
        step_m > WALL_THICKNESS_M as f32 + PROBE_HALF_M as f32,
        "a tick of {step_m} m must outrun wall-plus-probe ({WALL_THICKNESS_M} m \
         of wall plus {PROBE_HALF_M} m of probe) or this test measures nothing"
    );
}

/// **The pinned limitation, with the corrected attribution.** A mesh-derived
/// collider that lives on a *child node* of the static body — the layout
/// `spawn_static_mesh_collider` and `ColliderConstructorHierarchy` produce —
/// is invisible to Avian's swept CCD: the 400 m/s probe tunnels through it.
/// The cause is `SweptCcdBodyQuery`'s `collider: &'static Collider`
/// requirement on the *body* entity, not any property of the triangle mesh.
///
/// This test fails the day the pinned engine learns to sweep against
/// collider-hierarchy bodies — the re-measure signal, not a bug.
#[test]
fn accept_t420_a_mesh_collider_on_a_child_node_is_invisible_to_swept_ccd() {
    let (app, wall, probe) = sweep(
        1,
        |app| {
            spawn_static_mesh_collider(
                app,
                wall_mesh(),
                Transform::default(),
                CollisionLayers::from(CollisionLayer::StaticWorld),
            )
        },
        14,
    );
    precondition(&app);
    // The collider must exist and be attached to the body — otherwise the
    // tunnel would measure a missing collider, not a swept-CCD skip.
    assert!(
        is_attached(app.world(), &wall),
        "the child-node collider must be attached to the static body"
    );
    let end = end_position(&app, probe);
    assert!(
        end.x > WALL_THICKNESS_M as f32,
        "on the pinned pair a child-node collider is invisible to swept CCD; \
         this body ended at {end:?}. When it is stopped, re-measure the \
         limitation in the t420 finding before changing anything else"
    );
}

/// **Attached but invisible to the swept path only, not missing.** The same
/// wall whose child-node collider the 400 m/s probe tunnels through stops a
/// *discrete* body at 30 m/s — 0.25 m per tick, well inside the wall's 1 m
/// thickness: the collider detects everything its sampling can reach and
/// only the swept query skips it.
///
/// (The control uses the cuboid arm: a probe between this fixture's two
/// zero-thickness trimesh faces is wedged inside the slab rather than held
/// at its face, which is a thin-shell contact question, not this task's.)
#[test]
fn accept_t420_a_child_node_collider_still_stops_a_discrete_body() {
    let mut app = t420_app(1);
    let wall = wall_cuboid_on_child(&mut app);
    for _ in 0..6 {
        app.update();
    }
    assert!(
        is_attached(app.world(), &wall),
        "the child-node collider must be attached to the static body"
    );
    let probe = spawn_discrete_probe(
        &mut app,
        &ProbeSpec {
            velocity_m_s: [30.0, 0.0, 0.0],
            ..tunnelling_probe()
        },
    )
    .expect("the probe spec is valid");
    // 30 m/s at 120 Hz is 0.25 m per tick; the probe needs ~46 ticks to
    // reach the wall from x = -12.
    for _ in 0..60 {
        app.update();
    }
    let end = end_position(&app, probe);
    assert!(
        end.x < 0.0,
        "a discrete body is stopped by the child-node collider at the wall \
         face; it ended at {end:?}"
    );
}

/// **Placement, not shape.** A *cuboid* — a support map, the shape class the
/// F18-B finding named as working — tunnels exactly like the trimesh when it
/// lives on a child node. This is the measurement that proves the recorded
/// "parry has no `TriMesh` case" attribution wrong.
#[test]
fn accept_t420_a_cuboid_on_a_child_node_is_ignored_the_same_way() {
    let (app, wall, probe) = sweep(1, wall_cuboid_on_child, 14);
    precondition(&app);
    assert!(
        is_attached(app.world(), &wall),
        "the child-node collider must be attached to the static body"
    );
    let end = end_position(&app, probe);
    assert!(
        end.x > WALL_THICKNESS_M as f32,
        "a cuboid on a child node tunnels too, so the miss is about collider \
         placement rather than parry shape support; it ended at {end:?}"
    );
}

/// **The decided shipping layout.** One entity carries `RigidBody` +
/// `Mesh3d` + `ColliderConstructor::TrimeshFromMesh`: the collider is derived
/// from the same uploaded mesh the presentation draws, lands on the body
/// entity itself, and swept CCD stops the 400 m/s probe at the wall's near
/// face — with every stored triangle intact, nothing proxied.
#[test]
fn accept_t420_a_mesh_collider_on_the_body_entity_stops_the_swept_probe() {
    let (app, wall, probe) = sweep(1, wall_mesh_on_body, 14);
    precondition(&app);

    // The derived collider really is a trimesh and kept every triangle —
    // the F18 non-negotiable 1 check, asserted not assumed.
    let collider = app
        .world()
        .get::<Collider>(wall)
        .expect("the decided layout puts the collider on the body entity");
    let trimesh = collider
        .shape()
        .as_trimesh()
        .expect("a mesh-derived collider is a triangle mesh, not a substitute");
    assert_eq!(
        trimesh.indices().len(),
        4,
        "the four stored triangles must all reach the collider"
    );

    let end = end_position(&app, probe);
    assert!(
        end.x < 0.0,
        "swept CCD clamps the probe at the wall's near face (x ≈ -0.75) when \
         the collider sits on the body entity; it ended at {end:?}"
    );
    // And it was a real stop, not a despawn: the probe is still there and the
    // wall's AABB is unchanged.
    assert!(app.world().get::<ColliderAabb>(wall).is_some());
}

/// The same decided behaviour written as a direct `Collider::trimesh` on the
/// body — identical outcome, so the stop is a property of the layout rather
/// than of the derive path.
#[test]
fn accept_t420_a_direct_trimesh_collider_on_the_body_also_stops_the_probe() {
    let (app, _wall, probe) = sweep(1, wall_trimesh_on_body, 14);
    precondition(&app);
    let end = end_position(&app, probe);
    assert!(
        end.x < 0.0,
        "a trimesh collider on the body entity stops the probe; it ended at {end:?}"
    );
}

/// The control arm: a cuboid on the body entity is clamped, matching the
/// F18-A arch measurement on the same numbers.
#[test]
fn accept_t420_a_cuboid_on_the_body_entity_stops_the_same_probe() {
    let (app, _wall, probe) = sweep(1, wall_cuboid_on_body, 14);
    precondition(&app);
    let end = end_position(&app, probe);
    assert!(
        end.x < 0.0,
        "a cuboid on the body entity stops the probe at the wall face; it \
         ended at {end:?}"
    );
}

/// **Substeps are not the fix.** Measured: at `SubstepCount(2)`, `(4)` and
/// `(8)` the probe still tunnels the child-node collider — substeps subdivide
/// the solver, not detection, so a skipped pair stays skipped at any rate.
/// This pins the "more substeps" alternative as measured-and-rejected.
#[test]
fn accept_t420_substeps_do_not_make_a_child_node_collider_visible() {
    for substeps in [2, 4, 8] {
        let (app, wall, probe) = sweep(substeps, wall_cuboid_on_child, 14);
        assert!(
            is_attached(app.world(), &wall),
            "the child-node collider must be attached to the static body"
        );
        let end = end_position(&app, probe);
        assert!(
            end.x > WALL_THICKNESS_M as f32,
            "even at {substeps} substeps a child-node collider is skipped by \
             swept CCD; it ended at {end:?}"
        );
    }
}

/// **The margin workaround really works, which is why it was rejected on cost
/// and not on effect.** The same child-node trimesh that tunnels above is
/// stopped when the probe is given a large `SpeculativeMargin`, because the
/// speculative narrow phase *does* have a `TriMesh` case — it is the swept
/// query that skips the pair, not the narrow phase.
///
/// Pinning a rejected alternative is the point: the decision record's cost
/// ("it predicts contacts metres ahead, which is the globally inflated hitbox
/// F23 non-negotiable behavior 3 forbids") is only checkable if the behaviour
/// it rejects is measured. Measured here, the threshold is between 1 m and
/// 2 m on this fixture — comparable to the 3.33 m a tick travels, i.e. a
/// hitbox grown by metres, and not a small tolerance.
///
/// Observable failure: the probe tunnelling here means the engine no longer
/// stops fast bodies on a predicted contact at all, so the recorded cost of the
/// rejected alternative has to be re-measured.
#[test]
fn accept_t420_a_speculative_margin_stops_what_the_swept_query_skips() {
    let mut app = t420_app(1);
    let wall = spawn_static_mesh_collider(
        &mut app,
        wall_mesh(),
        Transform::default(),
        CollisionLayers::from(CollisionLayer::StaticWorld),
    );
    for _ in 0..6 {
        app.update();
    }
    assert!(
        is_attached(app.world(), &wall),
        "the child-node collider must be attached to the static body"
    );
    let probe = spawn_swept_probe(&mut app, &tunnelling_probe()).expect("the probe spec is valid");
    // Production zeroes the margin so a sweep is what the arms measure; this
    // one arm deliberately reverses that to price the alternative.
    app.world_mut()
        .entity_mut(probe)
        .insert(SpeculativeMargin(10.0));
    for _ in 0..14 {
        app.update();
    }
    let end = end_position(&app, probe);
    assert!(
        end.x < 0.0,
        "a speculative margin large enough to cover a tick stops the probe at \
         the wall the swept query skipped; it ended at {end:?}"
    );
}

/// The probe really does carry [`SweptCcd`]: without this guard a fixture
/// regression that dropped the component would read as "the limitation
/// widened" and pass unnoticed.
///
/// It also carries a [`Collider`] on its own entity, which is the *other* half
/// of the rule: `solve_swept_ccd` reads the same required `collider` field off
/// the swept body, so a `SweptCcd` body whose colliders all live on children
/// never sweeps at all. The static arms below would still measure correctly
/// with a sweep-less probe, so nothing else here catches that.
#[test]
fn accept_t420_the_probe_is_a_swept_body() {
    let mut app = t420_app(1);
    let probe = spawn_swept_probe(&mut app, &tunnelling_probe()).expect("the probe spec is valid");
    assert!(
        app.world().get::<SweptCcd>(probe).is_some(),
        "the measured body must carry SweptCcd"
    );
    assert!(
        app.world().get::<Collider>(probe).is_some(),
        "and a collider on its own entity: the same query reads a Collider off \
         both bodies, so a swept body without one never sweeps and every arm \
         above would quietly measure a discrete overlap"
    );
}
