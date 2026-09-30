//! The real Bevy asset stack, and the mesh-derived collision it makes possible
//! (F00-A follow-up task #333,
//! `docs/findings/2026-09-30-t333-real-asset-stack-for-collider-from-mesh.md`).
//!
//! Avian3d 0.7's default `collider-from-mesh` feature registers two systems
//! that assume Bevy's asset stack exists:
//! `ColliderCachePlugin::clear_unused_colliders` takes a
//! `MessageReader<AssetEvent<Mesh>>` and
//! `init_collider_constructor_hierarchies` takes a `Res<Assets<Mesh>>`.
//! `PhysicsPlugins::default()` adds both, so **every** world in this crate that
//! adds it needs the stack — not only the worlds that want a mesh collider.
//! Without one, the first `App::update` aborts with "Message not initialized"
//! / "Resource does not exist".
//!
//! Three things live here, in dependency order:
//!
//! * [`AssetStackPlugin`] installs the stack itself: Bevy's `AssetPlugin`
//!   (which owns the `AssetServer` and the asset IO), `bevy::mesh`'s
//!   `MeshPlugin`, whose `init_asset::<Mesh>()` is what creates both
//!   `Assets<Mesh>` and `Messages<AssetEvent<Mesh>>`, and Bevy's
//!   `WorldSerializationPlugin`, whose `WorldInstanceSpawner` Avian's
//!   hierarchy constructor gates itself on. The mesh asset is what the F17-B
//!   canonical-to-Bevy adapter ([`crate::render::bevy_mesh`]) produces, so
//!   this is where that value stops being a plain `Mesh` and becomes something
//!   the engine and the physics backend can both hold.
//! * [`headless_app`] is the one composition of a headless `cs_app` world. The
//!   asset-free scenes and fixtures ([`crate::synthetic`],
//!   [`crate::physics::fixture`], [`crate::world::fixture`]) all build their
//!   world through it instead of repeating the plugin tuple, because a fourth
//!   caller that spelled the tuple out by hand is how the feature would be
//!   silently broken again.
//! * [`spawn_static_mesh_collider_on_body`] turns one uploaded [`Mesh`] into a
//!   static collider Avian derives from it, with the derived `Collider` landing
//!   on the body entity itself. This is the capability F18-B's world import
//!   needs and the F00-A finding recorded as missing; it is *not* F18-B itself,
//!   which owns the geometry, the simplification policy and the per-role
//!   collision mapping.
//! * [`spawn_static_mesh_collider`] is the *other* shape of the same
//!   conversion: a body plus a child mesh node, with the collider on the child.
//!   It is kept for the layouts that need a collider on a descendant, and it
//!   declares the body [`SweptInvisible`] rather than pretending otherwise.
//!
//! # The collider-on-body rule
//!
//! **A rigid body that swept bodies must stop against — or that sweeps itself
//! — carries at least one [`Collider`] on its own entity.** On the pinned pair
//! this is not a style preference, it is how Avian finds the body at all:
//! `solve_swept_ccd` resolves every contact-graph neighbour through
//! `SweptCcdBodyQuery` (`avian3d-0.7.0/src/dynamics/ccd/mod.rs`), whose
//! `collider: &'static Collider` field is read off the **body** entity. A body
//! whose colliders all live on descendants fails that query, and the pair is
//! skipped without a cast ever being attempted — swept bodies pass straight
//! through it, and a `SweptCcd` body in that position never sweeps at all.
//! Shape is irrelevant: a cuboid on a child node tunnels exactly like a
//! trimesh. Measured 2x2 on the production 400 m/s probe against a 1 m wall:
//! trimesh on the body stopped, trimesh on a child tunnelled, cuboid on a
//! child tunnelled, cuboid on the body stopped. The discriminant is *where the
//! `Collider` component sits*, never the shape it holds. Upstream avian `main`
//! has rewritten swept CCD to iterate `RigidBodyColliders` and so no longer has
//! this requirement, but that rewrite is unreleased; task #420 recorded the
//! decision and the measurement, and task #424 made it an invariant.
//!
//! A body with colliders on children is *not* lost for this: the query only
//! checks the body entity, so a multi-part body stays fully swept-eligible as
//! soon as the root also carries one real collider. No dummy geometry is
//! needed.
//!
//! The rule is machine-checkable, not just prose:
//! [`swept_invisible_bodies`] reports every body that fails it and
//! [`undeclared_swept_invisible_bodies`] reports the ones that did not declare
//! it, so a body-spawning path that regresses is caught by a test rather than
//! by a 400 m/s body quietly flying through a wall.
//!
//! What this module deliberately does not do:
//!
//! * **It loads nothing.** No asset is read from disk, from an installation or
//!   from a Bevy asset path: canonical content is converted in-process and
//!   handed to [`spawn_static_mesh_collider_on_body`] as a `Mesh`. A world built
//!   here may hold assets, but it never *loads* any, which is what keeps the
//!   F00 `SYNTHETIC` scene asset-free in the sense F00 non-negotiable behavior 2
//!   means (see [`crate::synthetic`]).
//! * **It picks no geometry.** [`spawn_static_mesh_collider_on_body`] takes the
//!   mesh and the transform it is given. Whether an original mesh is simplified,
//!   and how, is F18-B's decision under F18 non-negotiable behavior 1; nothing
//!   here may pre-empt it by, say, convex-hulling a shape.
//! * **It invents no collision semantics.** The layer membership is the
//!   caller's [`cs_sim::collision::CollisionLayers`], mapped by the one
//!   conversion the workspace already owns.

use avian3d::prelude::{
    Collider, ColliderConstructor, ColliderConstructorHierarchy, PhysicsPlugins, Position,
    RigidBody, RigidBodyColliders, Rotation,
};
use bevy::app::{App, Plugin};
use bevy::asset::{AssetPlugin, Assets, Handle};
use bevy::ecs::world::World;
use bevy::mesh::{Mesh, Mesh3d, MeshPlugin};
use bevy::prelude::{
    ChildOf, Children, Component, Entity, MinimalPlugins, Transform, TransformPlugin,
};
use bevy::tasks::IoTaskPool;
use bevy::world_serialization::WorldSerializationPlugin;
use cs_sim::collision::CollisionLayers;

use crate::world::spawn::avian_layers;

/// Installs the Bevy asset stack Avian's mesh-derived colliders require.
///
/// Three plugins, and each of them creates a resource one of Avian's
/// `collider-from-mesh` systems reads:
///
/// * `AssetPlugin` owns the `AssetServer` and the asset IO it would need if
///   anything ever loaded a file;
/// * `bevy::mesh::MeshPlugin` runs `init_asset::<Mesh>()`, creating
///   `Assets<Mesh>` and `Messages<AssetEvent<Mesh>>`;
/// * `bevy::world_serialization::WorldSerializationPlugin` creates
///   `WorldInstanceSpawner`, which Avian's `init_collider_constructor_hierarchies`
///   gates itself on (see the comment at its use site).
///
/// Nothing else is added: this is a headless, deterministic world, so there is
/// no renderer, no image asset and no `.scn.ron` scene loader behind it.
///
/// # Panics
///
/// If Bevy's task pools do not exist yet. `AssetServer` needs the IO task
/// pool, and only `MinimalPlugins`/`DefaultPlugins` install it, so this
/// plugin has to be added *after* one of them. Avian would otherwise fail
/// later with an unrelated message; the assertion names the cause.
pub struct AssetStackPlugin;

impl Plugin for AssetStackPlugin {
    fn build(&self, app: &mut App) {
        assert!(
            IoTaskPool::try_get().is_some(),
            "AssetStackPlugin needs Bevy's task pools: add it after MinimalPlugins \
             or DefaultPlugins, which install the IO task pool AssetServer uses"
        );
        app.add_plugins((
            AssetPlugin {
                // Nothing in `cs_app` loads an asset through the asset server,
                // so a filesystem watcher would add nondeterminism to a
                // headless world for files that are never read. Bevy already
                // leaves this off unless the `watch` feature is enabled;
                // stating it makes that a property of this crate rather than
                // of the dependency's feature set.
                watch_for_changes_override: Some(false),
                ..AssetPlugin::default()
            },
            MeshPlugin,
            // Avian's pinned feature set includes `bevy_scene`, which makes
            // `init_collider_constructor_hierarchies` take
            // `If<Res<WorldInstanceSpawner>>` so it can wait for a scene
            // instance to finish spawning. `WorldInstanceSpawner` belongs to
            // this plugin, so without it that system is skipped *silently* and
            // a `ColliderConstructorHierarchy` never produces anything — no
            // panic, no warning, just no collider. It is part of the stack
            // this feature set needs, not an extra.
            WorldSerializationPlugin,
        ));
    }
}

/// Builds the headless Bevy world every `cs_app` scene and fixture runs on.
///
/// This is the single place the base plugin set is written down, because
/// `PhysicsPlugins::default()` is not self-contained under Avian's default
/// features: with `collider-from-mesh` enabled it adds systems that read
/// `Assets<Mesh>` and `AssetEvent<Mesh>`, so an app that adds it without
/// [`AssetStackPlugin`] aborts on the first `App::update`. Callers add their
/// own plugins and resources to the returned [`App`] before driving it.
pub fn headless_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        AssetStackPlugin,
        PhysicsPlugins::default(),
    ));
    app
}

/// The two entities one mesh-derived collider spans: the static rigid body
/// that owns it and the node whose [`Mesh3d`] it was derived from.
///
/// This is the *hierarchy* layout. For anything a swept body must stop against,
/// use [`spawn_static_mesh_collider_on_body`] instead: see "The collider-on-body
/// rule" in the module docs, and [`SweptInvisible`] for how the body that
/// [`spawn_static_mesh_collider`] returns declares what it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshColliderNode {
    /// The static rigid body the derived collider is attached to.
    pub body: Entity,
    /// The node carrying the `Mesh3d` handle the collider was built from. Its
    /// `Collider` does not exist until Avian's
    /// `init_collider_constructor_hierarchies` has run, so a caller that
    /// spawns this inside `Startup` must let a frame pass before reading it.
    pub node: Entity,
}

/// Why a body declares itself invisible to Avian's swept CCD.
///
/// The reason travels with the body rather than living only in a task note, so
/// [`undeclared_swept_invisible_bodies`] can tell a deliberate layout from a
/// regression: a body-spawning path that quietly moves its colliders onto
/// children is reported, and a body that says why is not.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SweptInvisible {
    /// Why this body's colliders live on other entities and the body is
    /// therefore skipped by `SweptCcdBodyQuery`.
    ///
    /// A sentence naming the caller and the layout, not a category: this is the
    /// evidence a reader needs to decide whether the body may be swept against
    /// or sweeps itself, and "mesh" alone would not say it.
    pub reason: &'static str,
}

/// A rigid body that carries no [`Collider`] of its own, and the entities
/// holding its colliders instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SweptInvisibleBody {
    /// The body entity Avian's swept CCD skips.
    pub body: Entity,
    /// Every descendant of `body` that carries a `Collider`, in query order.
    /// Empty when the body has no collider at all.
    pub collider_holders: Vec<Entity>,
    /// The body's own declaration, when it made one.
    pub declared: Option<SweptInvisible>,
}

/// Every rigid body in `world` that carries no [`Collider`] on its own entity.
///
/// This is the collider-on-body rule as a query rather than as a paragraph: each
/// entry is a body Avian's `SweptCcdBodyQuery` cannot resolve, so a swept body
/// passes through it and a `SweptCcd` body in that position never sweeps.
///
/// A body only counts as swept-invisible once the world has been updated at
/// least once: Avian derives a mesh collider in an `Update` system, so before the
/// first update *every* mesh body looks like it has no collider.
#[must_use]
pub fn swept_invisible_bodies(world: &mut World) -> Vec<SweptInvisibleBody> {
    let bodies: Vec<(Entity, Option<SweptInvisible>)> = {
        let mut query = world.query::<(Entity, &RigidBody)>();
        query
            .iter(world)
            .map(|(entity, _)| (entity, world.get::<SweptInvisible>(entity).copied()))
            .filter(|(entity, _)| world.get::<Collider>(*entity).is_none())
            .collect()
    };

    let mut found = Vec::with_capacity(bodies.len());
    for (body, declared) in bodies {
        found.push(SweptInvisibleBody {
            body,
            collider_holders: collider_descendants(world, body),
            declared,
        });
    }
    found
}

/// Every descendant of `root` that carries a [`Collider`], breadth first.
///
/// Walks [`Children`] rather than a `Query` so the walk and the world borrow do
/// not have to be held at once. A malformed hierarchy (a `Children` entry naming
/// an entity that is already gone) is skipped rather than treated as a cycle:
/// a body whose descendant vanished is swept-invisible, which is the fact the
/// caller asked for, and panicking here would turn a report into a crash.
fn collider_descendants(world: &World, root: Entity) -> Vec<Entity> {
    let mut holders = Vec::new();
    let mut pending: Vec<Entity> = world
        .get::<Children>(root)
        .map_or_else(Vec::new, |c| c.to_vec());
    let mut seen: Vec<Entity> = vec![root];
    while let Some(entity) = pending.pop() {
        if seen.contains(&entity) {
            continue;
        }
        seen.push(entity);
        if world.get::<Collider>(entity).is_some() {
            holders.push(entity);
        }
        pending.extend(world.get::<Children>(entity).into_iter().flatten());
    }
    holders
}

/// The swept-invisible bodies that did **not** declare themselves: the actual
/// violations of the collider-on-body rule.
///
/// An empty result is the invariant every production body-spawning path must
/// hold. A non-empty one names a body no caller ever justified, which is the
/// failure mode this exists to catch — a collider quietly moved onto a child
/// node is invisible in review and in the contact log, and only shows up as a
/// body moving faster than geometry can be sampled.
#[must_use]
pub fn undeclared_swept_invisible_bodies(world: &mut World) -> Vec<SweptInvisibleBody> {
    swept_invisible_bodies(world)
        .into_iter()
        .filter(|body| body.declared.is_none())
        .collect()
}

/// Adds `mesh` to the world's asset stack and asks Avian to derive a static
/// collider from it.
///
/// `mesh` is handed over unchanged — in the intended path it is the
/// [`crate::render::bevy_mesh::GroupUpload::into_mesh`] of one canonical
/// material group, so the collider is built from exactly the bit patterns the
/// content pipeline produced, with no second conversion and nothing dropped.
///
/// The collider Avian builds is `ColliderConstructor::TrimeshFromMesh`, the
/// one that cannot close a traversable opening: a triangle mesh keeps every
/// triangle the mesh stored. Simplifying it is a decision F18-B owns under
/// F18 non-negotiable behavior 1 and must make visibly, so nothing here
/// substitutes a convex hull or a bounding box.
///
/// `membership` is the engine-independent layer set from
/// [`cs_sim::collision`], mapped by the one conversion the workspace owns
/// ([`avian_layers`]), so a mesh collider and a hand-built one can never
/// disagree about who interacts with whom.
///
/// `transform`'s scale is honoured exactly, and does not degrade the shape. The
/// derived collider lands on the mesh *node*, a child of the body, so Avian
/// places it with a `ColliderTransform` whose scale it copies from the body's
/// `Transform`; `Collider::set_scale` then scales the trimesh's vertices
/// through parry's `TriMesh::scaled`. Measured here for both a uniform and a
/// non-uniform scale: `Collider::shape_scaled()` is still a `TriMesh` with the
/// per-axis scaled positions, never a convex hull or a bounding box. That
/// matters under F18 non-negotiable behavior 1 — a scale must not become a
/// simplification — and it is a property of the engine, not a decision made
/// here, so a change in it is a change to watch rather than to rely on.
///
/// **This layout is deliberately swept-invisible, and says so on the body.** A
/// collider on a child node is what the collider-on-body rule forbids, so the
/// returned body carries a [`SweptInvisible`] declaring it. A body in this
/// layout is skipped by `SweptCcdBodyQuery`: swept bodies pass through it at
/// any speed above the discrete sampling rate, and a `SweptCcd` body in that
/// position never sweeps. This is kept — with its contract, its name and its
/// `is_attached` check — because it is the layout F00-A task #333 pinned and
/// because `ColliderConstructorHierarchy` per-descendant constructors have no
/// single-entity equivalent; it is *not* the layout world geometry uses, which
/// is [`spawn_static_mesh_collider_on_body`]. Task #424 made that split
/// explicit after task #420 measured the two layouts against each other.
///
/// # Panics
///
/// If `app` was not built by [`headless_app`] (or otherwise given
/// [`AssetStackPlugin`]), because there is then no `Assets<Mesh>` to add to.
#[must_use]
pub fn spawn_static_mesh_collider(
    app: &mut App,
    mesh: Mesh,
    transform: Transform,
    membership: CollisionLayers,
) -> MeshColliderNode {
    let handle: Handle<Mesh> = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);

    // The hierarchy root is the body: Avian's collider hierarchy attaches a
    // descendant's collider to the nearest rigid-body ancestor, so a mesh node
    // without a body of its own would collide with nothing.
    let body = app
        .world_mut()
        .spawn((
            RigidBody::Static,
            ColliderConstructorHierarchy::new(ColliderConstructor::TrimeshFromMesh)
                .with_default_layers(avian_layers(membership)),
            transform,
            Position(transform.translation),
            Rotation(transform.rotation),
            SweptInvisible {
                reason: "spawn_static_mesh_collider builds the collider-on-a-child-node \
                         layout: ColliderConstructorHierarchy derives only onto descendants, \
                         so this body is skipped by Avian's SweptCcdBodyQuery. A body a \
                         swept body must stop against needs spawn_static_mesh_collider_on_body.",
            },
        ))
        .id();

    let node = app.world_mut().spawn((Mesh3d(handle), ChildOf(body))).id();

    MeshColliderNode { body, node }
}

/// Adds `mesh` to the world's asset stack and derives a static collider from it
/// onto **one** entity, which is at the same time the static rigid body, the
/// mesh node and the collider.
///
/// This is the layout the collider-on-body rule asks for, and the one world
/// geometry uses (`crate::world::spawn`). Compared with
/// [`spawn_static_mesh_collider`] it is a strict reduction of the same thing:
/// same upload, same `ColliderConstructor::TrimeshFromMesh`, same every stored
/// triangle, same layer membership, same `transform` — one entity instead of a
/// body and a child. Nothing about the geometry changes; only where the
/// `Collider` component lands does.
///
/// The difference is that Avian's `init_collider_constructors` inserts the
/// derived collider **on the entity that holds the `ColliderConstructor`**, and
/// that entity is the body, so `SweptCcdBodyQuery` can resolve the body and a
/// swept body is stopped by the mesh instead of passing through it. Measured:
/// the production 400 m/s `SweptCcd` probe (3.33 m of travel per tick against
/// a 1 m wall) is clamped at the wall's near face with all four stored triangles
/// in the collider, where the child-node layout tunnels with an empty contact
/// log. See "The collider-on-body rule" in the module docs and
/// `docs/findings/2026-09-30-t420-mesh-ccd-decision.md`.
///
/// The entity returned is the presentation *and* the collider *and* the body,
/// so a caller that presents a world object from this mesh may put its own
/// components on it, but must not also spawn a second rigid body for the same
/// geometry: the collider would then be attached to whichever body Avian's
/// collider hook finds first.
///
/// # Panics
///
/// If `app` was not built by [`headless_app`] (or otherwise given
/// [`AssetStackPlugin`]), because there is then no `Assets<Mesh>` to add to.
#[must_use]
pub fn spawn_static_mesh_collider_on_body(
    app: &mut App,
    mesh: Mesh,
    transform: Transform,
    membership: CollisionLayers,
) -> Entity {
    let handle: Handle<Mesh> = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);

    // `ColliderConstructor::TrimeshFromMesh` needs the `Mesh3d` it derives from
    // on the *same* entity, which is why the body, the presentation and the
    // constructor are one entity here rather than a body with a child node.
    //
    // The layers go on the entity rather than through
    // `ColliderConstructorHierarchy::with_default_layers`, which the hierarchy
    // form has and this one does not: the derived `Collider` lands here, so the
    // `CollisionLayers` component beside it is the membership Avian reads.
    app.world_mut()
        .spawn((
            RigidBody::Static,
            Mesh3d(handle),
            ColliderConstructor::TrimeshFromMesh,
            avian_layers(membership),
            transform,
            Position(transform.translation),
            Rotation(transform.rotation),
        ))
        .id()
}

/// Whether the collider Avian derived for `node` is attached to the body it
/// was spawned under.
///
/// Reported separately from the `Collider` component because they are
/// different facts: a collider that exists but is not attached to a rigid body
/// collides with nothing, and a caller that only checked for the component
/// would call that a working collider.
#[must_use]
pub fn is_attached(world: &World, node: &MeshColliderNode) -> bool {
    world
        .get::<RigidBodyColliders>(node.body)
        .is_some_and(|colliders| colliders.contains(&node.node))
}
