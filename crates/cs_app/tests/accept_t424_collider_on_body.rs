//! Task #424 (T420 follow-up): the collider-on-body rule, as an invariant
//! every production body-spawning path holds.
//!
//! # The rule
//!
//! **A rigid body that swept bodies must stop against — or that sweeps itself
//! — carries at least one [`Collider`] on its own entity.** On the pinned pair
//! (`bevy 0.19.1` / `avian3d 0.7.0` / `parry3d 0.27.0`) this is how Avian finds
//! the body at all: `solve_swept_ccd` resolves each contact-graph neighbour
//! through `SweptCcdBodyQuery`
//! (`avian3d-0.7.0/src/dynamics/ccd/mod.rs`), whose `collider: &'static
//! Collider` field is read off the **body** entity. A body whose colliders all
//! live on descendants fails that query, and the pair is skipped without a cast
//! ever being attempted: swept bodies pass straight through it, and a `SweptCcd`
//! body in that position never sweeps at all.
//!
//! Shape is irrelevant. Measured 2x2 on the production 400 m/s `SweptCcd` probe
//! against a 1 m wall (3.33 m of travel per tick): a trimesh on the body is
//! **stopped** at the wall's near face, a trimesh on a child **tunnels**, a
//! cuboid on a child **tunnels**, a cuboid on the body is **stopped**. The
//! discriminant is *where the `Collider` component sits*, never the shape it
//! holds. Task #420 recorded the measurement and the decision; this task makes
//! the rule an invariant instead of a paragraph somebody has to remember.
//!
//! # What is pinned here
//!
//! These tests drive production code only and use the production audit rather
//! than re-deriving the rule:
//!
//! * [`cs_app::asset_stack::undeclared_swept_invisible_bodies`] is **empty**
//!   after every production body-spawning path has run and the world has been
//!   updated. A non-empty result is the violation: a body nobody declared, whose
//!   colliders are somewhere a swept body cannot reach it. This is the
//!   invariant itself, so a future path that regresses fails here rather than in
//!   play.
//! * Each path's *own* claim is asserted too, so the audit cannot pass on a body
//!   that is invisible for an unrelated reason (a body with no collider at all,
//!   or a probe that is not swept).
//! * The one deliberate exception is named at its call site
//!   ([`spawn_static_mesh_collider`], kept for the F00-A #333 contract and for
//!   per-descendant constructor configurations) and it *declares itself* through
//!   [`SweptInvisible`], with a reason a reader can check. The audit reports
//!   such a body as declared, not as a violation — and the test asserts the
//!   declaration is really there, so the exception cannot be spread by accident.
//! * The audit's *report* is pinned as well as the invariant: every descendant
//!   holding a collider, breadth first, in the order the audit documents. A
//!   report a reader has to act on has to be the same report twice.
//! * The single-entity layout is a reduction of the two-entity one, not a
//!   different collider: same every stored triangle, and the same honoured
//!   per-axis scale. A layout fix that quietly collided at the wrong size, or
//!   that sealed a mesh opening with a hull, would pass every other test here.
//! * The two layouts really do differ in swept behaviour, measured through the
//!   production composition: a body a 400 m/s sweep must stop is stopped on the
//!   body layout and tunnels on the child layout. That is what makes the audit
//!   worth having, and it is the re-measure signal when a released Avian drops
//!   the requirement.
//!
//! # Re-measure signal
//!
//! Upstream avian `main` has rewritten swept CCD to iterate `RigidBodyColliders`
//! rather than requiring `&Collider` on the body, so the limitation is fixed
//! there but unreleased. When a release containing it lands and the pin moves,
//! the child-layout arm below fails — that failure means "re-measure and
//! re-decide", not "regress". The decision record is
//! `docs/findings/2026-09-30-t420-mesh-ccd-decision.md`; this task's own findings
//! are `docs/findings/2026-09-30-t424-collider-on-body-invariant.md`.

use std::time::Duration;

use avian3d::prelude::{Collider, Gravity, Position, RigidBody, SubstepCount, SweptCcd};
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::prelude::{App, ChildOf, Entity, Mesh3d, Transform, Vec3};
use bevy::time::{Fixed, Real, Time, TimeUpdateStrategy};
use cs_app::asset_stack::{
    SweptInvisibleBody, headless_app, spawn_static_mesh_collider,
    spawn_static_mesh_collider_on_body, swept_invisible_bodies, undeclared_swept_invisible_bodies,
};
use cs_app::physics::{BASELINE_FIXED_HZ, BodyMode, BodySpec, PhysicsAdapterPlugin, spawn_body};
use cs_app::world::{
    HARBOR_OBJECT_ABSENT, HARBOR_OBJECT_BANNER, HARBOR_OBJECT_GROUND, HARBOR_OBJECT_HANGAR,
    HARBOR_OBJECT_SENSOR, HARBOR_OBJECT_WATER, ProbeSpec, harbor_meshes, harbor_world, load_world,
    spawn_swept_probe, static_world_layers, static_world_membership, world_app, world_instance,
};
use cs_content::world::WorldCollisionRole;
use cs_sim::collision::{CollisionLayer, ShapeClass};

/// The probe's start, size and flight line — the same numbers the F18-A/B
/// sweeps use, so the results are comparable with the recorded ones.
const PROBE_START_X_M: f64 = -12.0;
const PROBE_Y_M: f64 = 1.5;
const PROBE_Z_M: f64 = 1.5;
const PROBE_HALF_M: f64 = 0.25;
const TUNNELLING_SPEED_M_S: f64 = 400.0;

/// The wall's half extents: 1 m thick on `x`, 4 m tall, 4 m deep — thin enough
/// that one 3.33 m tick outruns it. An open box, so no convex substitution can
/// fake a wall.
const WALL_THICKNESS_M: f64 = 1.0;

/// How far past the wall a stop is still "at the wall": the wall's near face is
/// `x = -0.5` and the probe half is `0.25`, so a swept clamp lands near
/// `x = -0.75`. Anything beyond `x = 1.0` is on the far side, i.e. a tunnel.
const STOPPED_BEFORE_X: f32 = 1.0;

/// The headless world the physics arms run: the real asset stack (Avian's
/// collider-from-mesh needs it), the real fixed-rate adapter, a manual clock
/// producing one 120 Hz step per update, gravity zero.
fn t424_app() -> App {
    let mut app = headless_app();
    app.add_plugins(PhysicsAdapterPlugin::new(BASELINE_FIXED_HZ));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / BASELINE_FIXED_HZ as f64,
    )));
    app.insert_resource(SubstepCount(1));
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

/// The wall mesh: an open box, `x ∈ [-0.5, 0.5]`, `y ∈ [0, 4]`, `z ∈ [-2, 2]`
/// — two quads, four triangles, open top and bottom.
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
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3, 4, 6, 5, 4, 7, 6]));
    mesh
}

/// The wall built through the production body-entity conversion: one entity
/// that is the static body, the mesh node and the collider.
fn wall_on_body(app: &mut App) -> Entity {
    spawn_static_mesh_collider_on_body(
        app,
        wall_mesh(),
        Transform::default(),
        static_world_membership(),
    )
}

/// The same wall through the production hierarchy conversion, which keeps the
/// collider on a child node.
fn wall_on_child(app: &mut App) -> (Entity, Entity) {
    let node = spawn_static_mesh_collider(
        app,
        wall_mesh(),
        Transform::default(),
        static_world_membership(),
    );
    (node.body, node.node)
}

fn tunnelling_probe() -> ProbeSpec {
    ProbeSpec {
        position_m: [PROBE_START_X_M, PROBE_Y_M, PROBE_Z_M],
        velocity_m_s: [TUNNELLING_SPEED_M_S, 0.0, 0.0],
        half_extents_m: [PROBE_HALF_M, PROBE_HALF_M, PROBE_HALF_M],
        mass_kg: 250.0,
    }
}

/// Builds the app, places `wall`, lets the collider constructor derive and the
/// broad phase register it, fires the swept probe and steps `ticks` times.
fn sweep(wall: impl FnOnce(&mut App) -> Entity, ticks: u64) -> (App, Entity) {
    let mut app = t424_app();
    let _ = wall(&mut app);
    for _ in 0..6 {
        app.update();
    }
    let probe = spawn_swept_probe(&mut app, &tunnelling_probe()).expect("the probe spec is valid");
    for _ in 0..ticks {
        app.update();
    }
    (app, probe)
}

fn end_x(app: &App, entity: Entity) -> f32 {
    app.world()
        .get::<Position>(entity)
        .expect("the probe still exists")
        .0
        .x
}

/// The triangle count of a derived collider's trimesh. `TriMesh::indices` yields
/// one `[u32; 3]` per triangle, so its length is already the count — and the
/// `expect` refuses a substituted primitive, which is F18 non-negotiable
/// behavior 1's requirement that a shape is never swapped for a simpler one.
fn collider_triangles(collider: &Collider) -> usize {
    collider
        .shape()
        .as_trimesh()
        .expect("a mesh-derived collider is a triangle mesh, not a substitute primitive")
        .indices()
        .len()
}

/// The probe really is a swept body, so the sweep arms below measure a sweep
/// rather than a discrete overlap.
#[test]
fn accept_t424_the_probe_is_a_swept_body_so_the_arms_measure_a_sweep() {
    let (app, probe) = sweep(wall_on_body, 1);
    assert!(
        app.world().get::<SweptCcd>(probe).is_some(),
        "the production probe must opt in to swept CCD, or a stop could be a \
         discrete overlap and the arms below would measure nothing"
    );
    assert!(
        app.world().get::<Collider>(probe).is_some(),
        "and it must carry a collider on its own entity, which is the same rule \
         the static paths are held to"
    );
}

/// The undeclared bodies, which is the invariant restated as the single call a
/// future path has to satisfy. Kept as a named helper so the message reads the
/// same everywhere the rule is checked.
fn invisible_of_undeclared(world: &mut bevy::prelude::World) -> Vec<SweptInvisibleBody> {
    undeclared_swept_invisible_bodies(world)
}

/// **The rule, measured.** The same wall, the same 400 m/s swept probe, the same
/// production composition — stopped when the collider is on the body entity,
/// tunnelled when it is on a child node.
///
/// This is the claim the whole invariant rests on, and it is a claim about
/// *placement*, not shape: the two arms differ in which entity holds the
/// `Collider` component and in nothing else. Without this, "a collider on the
/// body is what makes a body visible to a sweep" would be an assertion about
/// the pinned engine nobody had checked.
///
/// Observable failure: both arms agreeing means the pinned engine no longer has
/// the requirement — upstream avian `main` already dropped it, so a release that
/// does means re-measure and re-decide the rule rather than delete the test.
#[test]
fn accept_t424_a_collider_on_the_body_is_what_a_swept_body_stops_against() {
    // The wall's own span and the one-tick travel, asserted so the arms cannot
    // silently stop measuring a tunnel.
    let probe_app = t424_app();
    let dt = probe_app
        .world()
        .resource::<Time<Fixed>>()
        .timestep()
        .as_secs_f32();
    let step_m = TUNNELLING_SPEED_M_S as f32 * dt;
    assert!(
        step_m > WALL_THICKNESS_M as f32 + PROBE_HALF_M as f32,
        "a tick of {step_m} m must outrun wall-plus-probe ({WALL_THICKNESS_M} m \
         of wall plus {PROBE_HALF_M} m of probe) or this test measures nothing"
    );
    let free_end_x = PROBE_START_X_M as f32 + TUNNELLING_SPEED_M_S as f32 * (20.0 * dt);
    assert!(
        free_end_x > STOPPED_BEFORE_X,
        "a body this fast, unstopped, ends at {free_end_x} m, past the wall, so a \
         stop before {STOPPED_BEFORE_X} m can only be the wall"
    );

    let (body_app, body_probe) = sweep(wall_on_body, 20);
    let stopped = end_x(&body_app, body_probe);
    assert!(
        stopped < STOPPED_BEFORE_X,
        "a swept body must be stopped by a collider on the body entity's own \
         mesh; it ended at x = {stopped}"
    );

    let (child_app, child_probe) = sweep(|app| wall_on_child(app).0, 20);
    let tunnelled = end_x(&child_app, child_probe);
    assert!(
        tunnelled > STOPPED_BEFORE_X,
        "the same wall with its collider on a child node must still be invisible \
         to a sweep on the pinned engine, or the discriminant this rule is \
         built on is not placement; the body ended at x = {tunnelled}"
    );
}

/// **The production world-import path holds the invariant.** Every body the
/// harbor world spawns — a `FromMesh` hangar, the cuboid ground slab and the
/// cuboid water patch — is reachable by a swept body, and
/// `undeclared_swept_invisible_bodies` is empty after the load has settled.
///
/// Four colliders with four roles is the whole population: the banner is role
/// `None` and the absent object has no upload, so neither has a body at all.
/// Asserting the count means the empty audit cannot pass on a world that
/// silently failed to spawn its geometry. The `FromMesh` trigger volume is the
/// one object in the four that is deliberately **not** a body (task #401: a
/// volume a body may not be stopped by must not carry a rigid body, or Avian's
/// swept CCD holds one at its face), and it is outside the audit's query for
/// exactly that reason.
///
/// Observable failure: an undeclared entry, a count below four, or a trigger
/// volume that acquired a body.
#[test]
fn accept_t424_the_world_import_path_leaves_no_body_invisible_to_a_sweep() {
    let definition = harbor_world().expect("the synthetic harbor world is well formed");
    let population = [
        HARBOR_OBJECT_HANGAR,
        HARBOR_OBJECT_SENSOR,
        HARBOR_OBJECT_WATER,
        HARBOR_OBJECT_GROUND,
    ];
    let instance = world_instance(&definition, None, &population, &[])
        .expect("the fixture load record is valid");
    let mut app = world_app();
    let report = load_world(&mut app, &definition, &instance, &harbor_meshes())
        .expect("the harbor world loads");
    for _ in 0..4 {
        app.update();
    }

    let invisible = swept_invisible_bodies(app.world_mut());
    assert_eq!(
        invisible.len(),
        0,
        "every body the world import spawns carries a collider on its own \
         entity, so none of them is invisible to a sweep; the audit found {:?}",
        invisible
    );
    assert_eq!(
        report.colliders().len(),
        4,
        "and there really are four bodies to hold: the hangar, the trigger \
         volume, the water patch and the ground slab, so the empty audit above \
         is not an audit of nothing"
    );
    for collider in report.colliders() {
        match collider.body {
            Some(body) => assert!(
                app.world().get::<Collider>(body).is_some(),
                "`{}` is a body a swept body must be able to stop against, so its \
                 collider must be on the body entity",
                collider.object
            ),
            // A **trigger volume** is the one deliberate exception, and it is not
            // an exception to this rule: it is not a body a swept body must stop
            // against, so it must *not* be one (task #401 — a `Sensor` object is
            // spawned on an entity with no rigid body, because a resolvable body
            // is exactly what lets Avian's swept CCD hold a body at the volume's
            // face). The invariant still holds: an undeclared swept-invisible body
            // is what the audit above reports, and a trigger volume cannot be one.
            None => {
                assert_eq!(
                    collider.role,
                    WorldCollisionRole::Sensor,
                    "`{}` reports no rigid body, which only a trigger volume may do",
                    collider.object
                );
                assert!(
                    app.world().get::<RigidBody>(collider.entity).is_none(),
                    "`{}` must carry no rigid body at all, or a swept body is held at \
                     its face",
                    collider.object
                );
            }
        }
    }
}

/// **`spawn_body`, the one production path a runtime actor enters through,
/// holds the invariant** for every mode and both shape classes — a static
/// projectile's target, a dynamic aircraft part, a kinematic scripted actor and
/// a sensor volume.
///
/// A kinematic body is included on purpose: it is released to dynamic by
/// [`cs_app::physics::set_body_mode`], and a body that could not be stopped
/// while scripted and became stoppable on release would be exactly the kind of
/// layout bug this invariant exists to catch.
#[test]
fn accept_t424_every_body_spawn_body_produces_carries_a_collider_on_its_own_entity() {
    let mut app = headless_app();
    let cases = [
        (
            "dynamic solid",
            CollisionLayer::Aircraft,
            ShapeClass::Solid,
            BodyMode::Dynamic,
        ),
        (
            "dynamic sensor",
            CollisionLayer::Aircraft,
            ShapeClass::Sensor,
            BodyMode::Dynamic,
        ),
        (
            "kinematic solid",
            CollisionLayer::Projectile,
            ShapeClass::Solid,
            BodyMode::Kinematic,
        ),
        (
            "static solid",
            CollisionLayer::StaticWorld,
            ShapeClass::Solid,
            BodyMode::Static,
        ),
    ];
    let mut entities = Vec::new();
    for (index, (name, layer, shape, mode)) in cases.into_iter().enumerate() {
        let spec = BodySpec {
            layer,
            shape,
            mode,
            mass_kg: 20.0,
            half_extents_m: [0.5, 0.5, 0.5],
            position_m: [index as f32 * 4.0, 0.0, 0.0],
            linear_velocity_m_s: [0.0, 0.0, 0.0],
        };
        let entity = spawn_body(app.world_mut(), &spec)
            .unwrap_or_else(|error| panic!("the {name} spec is valid: {error}"));
        assert!(
            app.world().get::<RigidBody>(entity).is_some(),
            "the {name} case must really be a rigid body, or it measures nothing"
        );
        assert!(
            app.world().get::<Collider>(entity).is_some(),
            "a body spawned by `spawn_body` carries its collider on its own \
             entity, whatever the mode and shape class: a swept body could not \
             stop against the {name} body otherwise"
        );
        entities.push(entity);
    }
    app.finish();
    app.cleanup();
    app.update();

    let invisible = swept_invisible_bodies(app.world_mut());
    assert!(
        invisible.is_empty(),
        "no body `spawn_body` produced is invisible to a sweep; the audit found \
         {invisible:?}"
    );
    assert_eq!(
        invisible_of_undeclared(app.world_mut()),
        Vec::<SweptInvisibleBody>::new(),
        "and none of them needed to declare an exception either"
    );
    for entity in entities {
        assert!(
            app.world().get::<RigidBody>(entity).is_some(),
            "the {entity:?} case survived the update, so the audit above was not \
             reading a world that had lost its bodies"
        );
    }
}

/// The undeclared audit is the invariant, restated as the single call a future
/// path has to satisfy, and the exception it accepts is checked to be a real
/// exception rather than a loophole.
#[test]
fn accept_t424_the_audit_reports_a_declared_exception_and_nothing_else() {
    let mut app = headless_app();
    let (hierarchy_body, hierarchy_node) = wall_on_child(&mut app);
    let on_body = wall_on_body(&mut app);
    app.finish();
    app.cleanup();
    for _ in 0..6 {
        app.update();
    }

    // The body layout is invisible to nothing, so it never appears in the audit
    // at all — not even as a declared exception.
    assert!(
        swept_invisible_bodies(app.world_mut())
            .iter()
            .all(|body| body.body != on_body),
        "a body carrying its own collider satisfies the rule, so the audit must \
         not report it as invisible"
    );
    assert!(
        app.world().get::<Collider>(on_body).is_some(),
        "and it must really carry one, or the assertion above proves nothing"
    );

    // The hierarchy layout is invisible, and declares itself as such with a
    // reason a reader can check.
    let invisible = swept_invisible_bodies(app.world_mut());
    let hierarchy = invisible
        .iter()
        .find(|body| body.body == hierarchy_body)
        .expect("a body whose colliders live on children is swept-invisible");
    assert_eq!(
        hierarchy.collider_holders,
        vec![hierarchy_node],
        "and the audit names where its colliders actually are, so a reader can \
         see the layout rather than take the word for it"
    );
    let declared = hierarchy
        .declared
        .expect("the hierarchy body declares itself, with a reason");
    assert!(
        declared.reason.contains("SweptCcdBodyQuery"),
        "the declaration has to name the mechanism, or it is a category rather \
         than evidence: {:?}",
        declared.reason
    );
    assert!(
        declared
            .reason
            .contains("spawn_static_mesh_collider_on_body"),
        "and the call site that does hold the rule, so the exception says what \
         to do instead: {:?}",
        declared.reason
    );
    assert!(
        app.world().get::<Collider>(hierarchy_body).is_none(),
        "the premise: the hierarchy body really carries no collider, only its \
         child does"
    );

    // A world holding both layouts declares exactly one exception and reports
    // no undeclared body: the one production path that needs the exception has
    // made it, and nothing else has.
    assert_eq!(
        invisible.len(),
        1,
        "exactly one body in this world is swept-invisible, and it is the one \
         that declared it; saw {invisible:?}"
    );
    assert!(
        undeclared_swept_invisible_bodies(app.world_mut()).is_empty(),
        "an undeclared swept-invisible body is the violation the audit exists \
         for, and this world has none"
    );
}

/// A body that moved its colliders onto children without declaring it is
/// reported, and the report is specific enough to act on. This is the mutation
/// guard: without it, the audit could return an empty list for the wrong reason.
#[test]
fn accept_t424_an_undeclared_child_node_collider_is_reported_rather_than_ignored() {
    let mut app = t424_app();
    // A static body with a hand-built collider *moved onto a child node* and no
    // declaration: exactly the regression the invariant forbids, and exactly
    // what a well-meaning refactor produces.
    let body = app
        .world_mut()
        .spawn((RigidBody::Static, Transform::default(), Position::default()))
        .id();
    let child = app
        .world_mut()
        .spawn((
            Collider::cuboid(1.0, 4.0, 4.0),
            static_world_layers(),
            ChildOf(body),
        ))
        .id();
    app.finish();
    app.cleanup();
    for _ in 0..6 {
        app.update();
    }

    let undeclared = undeclared_swept_invisible_bodies(app.world_mut());
    assert_eq!(
        undeclared.len(),
        1,
        "a body with a collider on a child and no declaration is the violation; \
         the audit found {undeclared:?}"
    );
    assert_eq!(
        undeclared[0].body, body,
        "and it names the body, not the child"
    );
    assert_eq!(
        undeclared[0].collider_holders,
        vec![child],
        "together with the entity that holds the collider, so the fix is to \
         move it or declare the layout"
    );
    assert_eq!(
        undeclared[0].declared, None,
        "a body nobody declared has no declaration to report"
    );
}

/// A body with **several** descendants holding colliders reports all of them,
/// breadth first and within a level in `Children` order — the order the audit
/// documents. One holder is the easy case; a multi-part layout is the one a
/// reader has to act on, and a report whose order is unspecified is a report
/// nobody can diff against the world they are looking at.
#[test]
fn accept_t424_the_audit_reports_every_collider_descendant_in_a_stable_order() {
    let mut app = t424_app();
    let body = app
        .world_mut()
        .spawn((RigidBody::Static, Transform::default(), Position::default()))
        .id();
    let mut part = |parent: Entity, y: f32| {
        app.world_mut()
            .spawn((
                Collider::cuboid(0.5, 0.4, 0.4),
                static_world_layers(),
                ChildOf(parent),
                Transform::from_xyz(0.0, y, 0.0),
            ))
            .id()
    };
    let first = part(body, 1.0);
    let second = part(body, 2.0);
    // A collider two levels down, so breadth first and depth first disagree:
    // depth first would report it before `second`.
    let grandchild = part(first, 0.5);
    app.finish();
    app.cleanup();
    for _ in 0..6 {
        app.update();
    }

    let undeclared = undeclared_swept_invisible_bodies(app.world_mut());
    assert_eq!(
        undeclared.len(),
        1,
        "the body is the one swept-invisible entity; the audit found {undeclared:?}"
    );
    assert_eq!(
        undeclared[0].collider_holders,
        vec![first, second, grandchild],
        "every descendant holding a collider is reported, breadth first and in \
         the order Children lists it, so the report names all three parts \
         exactly once and in the order it documents"
    );
}

/// The F18-B findings and the F00-A #333 contract both name
/// `ColliderConstructorHierarchy`, and the *other* production path in the
/// workspace that spawns bodies from a mesh is the presentation-only one: a
/// role-`None` object gets a `Mesh3d` and no body, which satisfies the rule
/// vacuously and must not be mistaken for a body that lost its collider.
#[test]
fn accept_t424_a_presentation_only_object_has_no_body_to_hide_a_collider_on() {
    let definition = harbor_world().expect("the synthetic harbor world is well formed");
    let population = [HARBOR_OBJECT_BANNER, HARBOR_OBJECT_ABSENT];
    let instance = world_instance(&definition, None, &population, &[])
        .expect("the fixture load record is valid");
    let mut app = world_app();
    let report = load_world(&mut app, &definition, &instance, &harbor_meshes())
        .expect("the harbor world loads");
    for _ in 0..4 {
        app.update();
    }

    assert!(
        report.colliders().is_empty(),
        "a role-`None` object and an object with no upload get no collider at \
         all, so this world spawns no bodies for the rule to bind"
    );
    assert!(
        swept_invisible_bodies(app.world_mut()).is_empty(),
        "and an empty audit here is genuinely empty, not a rule that quietly \
         passes on a world with nothing in it"
    );
    let banner = report
        .object(&cs_content::world::WorldObjectId::new(HARBOR_OBJECT_BANNER).expect("valid key"))
        .expect("the banner is reported as presented");
    assert!(
        app.world().get::<RigidBody>(banner.visual).is_none(),
        "a presented-only object must carry no body: a body with no collider \
         would be swept-invisible for no declared reason"
    );
    assert!(
        app.world().get::<Mesh3d>(banner.visual).is_some(),
        "while still presenting the geometry its record names"
    );
}

/// A body with colliders on children is *not* lost for the rule, as long as the
/// body entity also carries one real collider: `SweptCcdBodyQuery` reads the
/// collider off the body and then casts using the *child* collider's own shape.
/// This is the multi-part case the rule has to keep working (aircraft-part
/// colliders, capital-ship subsystems) without a dummy geometry standing in for
/// a real part.
#[test]
fn accept_t424_a_multipart_body_needs_only_one_real_collider_on_its_root() {
    let mut app = t424_app();
    // The multi-part shape: a body with a real cuboid on itself and two more
    // part colliders on children. Every collider is a real part; none is a
    // placeholder.
    let body = app
        .world_mut()
        .spawn((
            RigidBody::Static,
            Collider::cuboid(1.0, 4.0, 4.0),
            static_world_layers(),
            Transform::default(),
            Position::default(),
        ))
        .id();
    for (index, y) in [1.0_f32, 3.0].into_iter().enumerate() {
        app.world_mut().spawn((
            Collider::cuboid(0.5, 0.4, 0.4),
            static_world_layers(),
            ChildOf(body),
            Transform::from_xyz(0.0, y, 0.0),
        ));
        assert!(
            index < 2,
            "the loop writes two parts, so the count assertion below is about \
             the world and not about a constant"
        );
    }
    app.finish();
    app.cleanup();
    for _ in 0..6 {
        app.update();
    }

    assert!(
        swept_invisible_bodies(app.world_mut()).is_empty(),
        "a body whose root carries one real collider is swept-eligible however \
         many part colliders its children add, so the audit must not report it"
    );

    // And the real point: a sweep is stopped by it.
    let probe = spawn_swept_probe(&mut app, &tunnelling_probe()).expect("the probe spec is valid");
    for _ in 0..20 {
        app.update();
    }
    let stopped = end_x(&app, probe);
    assert!(
        stopped < STOPPED_BEFORE_X,
        "a swept body is stopped by the multi-part body as a whole, not only by \
         whichever part happens to sit on the root; it ended at x = {stopped}"
    );
}

/// The `ColliderConstructor` the body-entity layout uses is *not* the
/// `ColliderConstructorHierarchy` the child layout uses, and the difference is
/// where the collider lands: `init_collider_constructors` inserts it on the
/// entity that holds the constructor, `init_collider_constructor_hierarchies`
/// iterates `children.iter_descendants` and never touches the body.
///
/// Pinning the mechanism stops the rule from being a superstition: if a future
/// Avian moved the derived collider onto a child, this fails even if the sweep
/// behaviour has not been re-measured yet.
#[test]
fn accept_t424_the_body_layout_derives_the_collider_onto_the_body_itself() {
    let mut app = t424_app();
    let on_body = wall_on_body(&mut app);
    let (child_body, child_node) = wall_on_child(&mut app);
    app.finish();
    app.cleanup();
    for _ in 0..6 {
        app.update();
    }

    let world = app.world_mut();
    assert_eq!(
        world.get::<Collider>(on_body).map(collider_triangles),
        Some(4),
        "the wall mesh stored four triangles, so the collider derived on the \
         body entity is the real trimesh and not a substituted shape"
    );
    assert!(
        world.get::<Collider>(child_body).is_none(),
        "`init_collider_constructor_hierarchies` walks descendants only, so the \
         hierarchy body never receives a collider"
    );
    assert_eq!(
        world.get::<Collider>(child_node).map(collider_triangles),
        Some(4),
        "the same four triangles, on the child — the geometry is identical and \
         only the placement differs"
    );
}

/// The single-entity layout is a *reduction* of the two-entity one, not a
/// different collider: the same upload, the same constructor, the same every
/// stored triangle, and the transform applied once. A mesh object must not lose
/// geometry on the way to being swept-eligible — F18 non-negotiable behavior 1
/// is about the shape, and the rule is about the layout.
#[test]
fn accept_t424_the_body_layout_keeps_every_stored_triangle_and_the_authored_transform() {
    let mut app = t424_app();
    let transform = Transform::from_xyz(3.0, -2.0, 1.0).with_scale(Vec3::new(2.0, 2.0, 2.0));
    let on_body = spawn_static_mesh_collider_on_body(
        &mut app,
        wall_mesh(),
        transform,
        static_world_membership(),
    );
    let child =
        spawn_static_mesh_collider(&mut app, wall_mesh(), transform, static_world_membership());
    app.finish();
    app.cleanup();
    for _ in 0..6 {
        app.update();
    }

    let world = app.world_mut();
    let on_body_collider = world.get::<Collider>(on_body).expect("derived on the body");
    let child_collider = world
        .get::<Collider>(child.node)
        .expect("derived on the child node");
    assert_eq!(
        collider_triangles(on_body_collider),
        collider_triangles(child_collider),
        "both layouts derive from the same upload, so neither is a simplification \
         of the other"
    );
    assert_eq!(
        world
            .get::<Position>(on_body)
            .expect("the body carries a position")
            .0,
        transform.translation,
        "and the authored transform is applied once, to the body that owns the \
         collider"
    );
}

/// **A scale is honoured on the body layout too, and by no substitution.** The
/// world path spawns a mesh object from the instance's *decomposed* matrix, so a
/// non-uniform scale reaches `spawn_static_mesh_collider_on_body` for real, and
/// F18 non-negotiable behavior 1 says a scale may not become a simplification:
/// `Collider::set_scale` falls back to a hull or a bounding box for a shape it
/// cannot scale, which on an open mesh seals the opening.
///
/// The mechanism differs between the layouts — the hierarchy form is scaled
/// through the `ColliderTransform` Avian derives from the body's `Transform`,
/// the body form through the `Transform` of the collider's own entity — so
/// "same every stored triangle, same transform" is a claim about two code
/// paths, and this measures both against the same eight uploaded corners. A
/// scale the collider quietly ignored still looks right in `shape()`, so this
/// reads `shape_scaled()`, which is what the narrow phase collides against.
///
/// Observable failure: a vertex that is not an uploaded corner scaled per axis,
/// a triangle count that is not the mesh's, or the two layouts disagreeing.
#[test]
fn accept_t424_the_body_layout_honours_a_scaled_placement_without_simplifying_the_mesh() {
    /// Non-uniform, because a hull or a box substitution is most tempting there
    /// and least likely to be caught by a comparison of triangle counts alone.
    const SCALE: Vec3 = Vec3::new(2.0, 1.0, 0.5);
    const TRIANGLES: usize = 4;

    let mut app = t424_app();
    let transform = Transform::from_xyz(1.0, 0.0, -3.0).with_scale(SCALE);
    let on_body = spawn_static_mesh_collider_on_body(
        &mut app,
        wall_mesh(),
        transform,
        static_world_membership(),
    );
    let child =
        spawn_static_mesh_collider(&mut app, wall_mesh(), transform, static_world_membership());
    app.finish();
    app.cleanup();
    for _ in 0..6 {
        app.update();
    }

    let world = app.world_mut();
    // The uploaded corners both layouts were built from, once: a scaled vertex
    // has to be one of these times the scale, or the collider is not the mesh.
    let corners: Vec<Vec3> = match wall_mesh().attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(bevy::mesh::VertexAttributeValues::Float32x3(values)) => {
            values.iter().copied().map(Vec3::from).collect()
        }
        _ => panic!("the wall stores a Float32x3 position attribute"),
    };
    let scaled_vertices = |entity: Entity, what: &str| {
        let collider = world
            .get::<Collider>(entity)
            .unwrap_or_else(|| panic!("the {what} layout derived a collider"));
        let scaled = collider.shape_scaled().as_trimesh().unwrap_or_else(|| {
            panic!(
                "scaling the {what} layout's collider must not substitute a \
                 primitive for the trimesh: that is how a traversable opening is \
                 sealed"
            )
        });
        assert_eq!(
            scaled.indices().len(),
            TRIANGLES,
            "the {what} layout's scaled collider keeps the {TRIANGLES} stored \
             triangles, not the ones a convex hull of the same corners would have"
        );
        for (index, vertex) in scaled.vertices().iter().enumerate() {
            assert!(
                corners
                    .iter()
                    .any(|corner| (*corner * SCALE).abs_diff_eq(*vertex, 0.0)),
                "the {what} layout's scaled collider vertex {index} is {vertex:?}, \
                 which is not an uploaded corner scaled per axis by {SCALE:?}"
            );
        }
        scaled
            .vertices()
            .iter()
            .map(|vertex| vertex.to_array().map(f32::to_bits))
            .collect::<Vec<[u32; 3]>>()
    };

    let from_body = scaled_vertices(on_body, "body-entity");
    assert_eq!(
        from_body,
        scaled_vertices(child.node, "child-node"),
        "both layouts scale the same upload the same way, so neither trades a \
         real scale for a simpler shape on the way to being swept-eligible"
    );
}
