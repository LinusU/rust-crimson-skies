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
//! * [`spawn_static_mesh_collider`] turns one uploaded [`Mesh`] into a static
//!   collider Avian derives from it: the mesh is added to the asset stack, a
//!   `ColliderConstructorHierarchy` asks for a trimesh, and a child entity
//!   holds the `Mesh3d` the constructor reads. This is the capability F18-B's
//!   world import needs and the F00-A finding recorded as missing; it is *not*
//!   F18-B itself, which owns the geometry, the simplification policy and the
//!   per-role collision mapping.
//!
//! What this module deliberately does not do:
//!
//! * **It loads nothing.** No asset is read from disk, from an installation or
//!   from a Bevy asset path: canonical content is converted in-process and
//!   handed to [`spawn_static_mesh_collider`] as a `Mesh`. A world built here
//!   may hold assets, but it never *loads* any, which is what keeps the F00
//!   `SYNTHETIC` scene asset-free in the sense F00 non-negotiable behavior 2
//!   means (see [`crate::synthetic`]).
//! * **It picks no geometry.** [`spawn_static_mesh_collider`] takes the mesh
//!   and the transform it is given. Whether an original mesh is simplified, and
//!   how, is F18-B's decision under F18 non-negotiable behavior 1; nothing
//!   here may pre-empt it by, say, convex-hulling a shape.
//! * **It invents no collision semantics.** The layer membership is the
//!   caller's [`cs_sim::collision::CollisionLayers`], mapped by the one
//!   conversion the workspace already owns.

use avian3d::prelude::{
    ColliderConstructor, ColliderConstructorHierarchy, PhysicsPlugins, Position, RigidBody,
    RigidBodyColliders, Rotation,
};
use bevy::app::{App, Plugin};
use bevy::asset::{AssetPlugin, Assets, Handle};
use bevy::ecs::world::World;
use bevy::mesh::{Mesh, Mesh3d, MeshPlugin};
use bevy::prelude::{ChildOf, Entity, MinimalPlugins, Transform, TransformPlugin};
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
        ))
        .id();

    let node = app.world_mut().spawn((Mesh3d(handle), ChildOf(body))).id();

    MeshColliderNode { body, node }
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
