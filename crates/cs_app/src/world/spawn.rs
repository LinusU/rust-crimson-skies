//! Spawning a [`WorldDefinition`] into the Bevy/Avian world (F18-A).
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-A`.
//!
//! [`spawn_world`] is the one production conversion this stage owns: it walks
//! a validated [`WorldDefinition`] and produces, for every object instance,
//! **two entities built from the same record** —
//!
//! * a [`WorldVisual`] entity carrying the object's transform as the render
//!   path would draw it, and
//! * a [`WorldColliderInstance`] entity carrying the *same* transform plus a
//!   static Avian collider on the layers [`cs_sim::collision`] declares.
//!
//! Both are stamped with the same [`WorldObjectBinding`], so "visual and
//! collision meshes share provenance and coordinate conversion" (F18
//! non-negotiable behavior 1) is structural: there is one transform to get
//! right, and [`SpawnedWorld`] reports every instance whose collision could
//! **not** be built instead of silently presenting geometry it never
//! collided with.
//!
//! What this stage deliberately does *not* do, and where it goes:
//!
//! * mesh-derived colliders ([`WorldCollisionShape::FromMesh`]) are reported
//!   as [`SkipReason::MeshColliderDeferred`] — the Avian collider-from-mesh
//!   path on a real asset stack now exists
//!   ([`crate::asset_stack::spawn_static_mesh_collider`], task #333), and
//!   building these instances with it is **F18-B**;
//! * mission overlays and streaming teardown are **F18-C**;
//! * retail geometry import is **F18-B/D**; everything this module spawns
//!   comes from a synthetic fixture (`super::fixture`).

use avian3d::prelude::{
    Collider, CollisionEventsEnabled, CollisionLayers as AvianCollisionLayers, Position, RigidBody,
    Rotation, Sensor,
};
use bevy::prelude::{App, Entity, GlobalTransform, Mat4, Quat, Transform, Vec3, Vec4};
use cs_content::scene::CanonicalTransform;
use cs_content::world::{
    WorldCollisionRole, WorldCollisionShape, WorldDefinition, WorldId, WorldObjectId,
    WorldObjectInstance,
};
use cs_sim::collision::{CollisionLayer, CollisionLayers};

use super::contacts::{WorldColliderInstance, WorldObjectBinding, WorldPlugin, WorldVisual};

/// How far the decomposed runtime transform may drift from the authored
/// canonical matrix and still be called the same transform.
///
/// The runtime frame is f32 (`docs/01-ARCHITECTURE.md`) while canonical
/// records are f64, so this is the representation's own rounding bound, not a
/// tolerance for a wrong placement: it is far tighter than any geometry the
/// fixture can move.
pub const INSTANCE_TRANSFORM_TOLERANCE: f32 = 1e-4;

/// Why an object instance's collider was not built, although its visual was.
///
/// An authored matrix that no runtime transform can hold is *not* one of
/// these: [`spawn_world`] refuses that definition outright (see
/// [`WorldSpawnError::UnrepresentableTransform`]) before any entity exists,
/// so it never reaches the report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// The record's collision role is an explicit unknown.
    UnknownCollisionRole,
    /// The record's collision shape is an explicit unknown.
    UnknownCollisionShape,
    /// The record declares a mesh-derived collider, which F18-B builds.
    MeshColliderDeferred,
}

impl SkipReason {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::UnknownCollisionRole => "unknown_collision_role",
            Self::UnknownCollisionShape => "unknown_collision_shape",
            Self::MeshColliderDeferred => "mesh_collider_deferred",
        }
    }
}

/// One instance that was presented but not given a collider, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedInstance {
    /// The object that was skipped.
    pub object: WorldObjectId,
    /// The reason its collision was not built.
    pub reason: SkipReason,
}

/// A collider entity produced from one object instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnedCollider {
    /// The object the collider was built from.
    pub object: WorldObjectId,
    /// The spawned collider entity.
    pub entity: Entity,
    /// The role the record declared for it.
    pub role: WorldCollisionRole,
}

/// What [`spawn_world`] produced for one [`WorldDefinition`].
#[derive(Clone, Debug, Default)]
pub struct SpawnedWorld {
    world: Option<WorldId>,
    visuals: Vec<(WorldObjectId, Entity)>,
    colliders: Vec<SpawnedCollider>,
    non_colliding: Vec<WorldObjectId>,
    skipped: Vec<SkippedInstance>,
}

impl SpawnedWorld {
    /// The spawned world's identity.
    #[must_use]
    pub fn world(&self) -> Option<&WorldId> {
        self.world.as_ref()
    }

    /// `(object, visual entity)` pairs, in definition order.
    #[must_use]
    pub fn visuals(&self) -> &[(WorldObjectId, Entity)] {
        &self.visuals
    }

    /// The collider entities, in definition order.
    #[must_use]
    pub fn colliders(&self) -> &[SpawnedCollider] {
        &self.colliders
    }

    /// The objects whose collision role is [`WorldCollisionRole::None`]:
    /// presented, never blocking — a deliberate answer, not a skip.
    #[must_use]
    pub fn non_colliding(&self) -> &[WorldObjectId] {
        &self.non_colliding
    }

    /// Every instance whose collision was **not** built, with the reason. A
    /// non-empty result is a visible gap, never a silent pass.
    #[must_use]
    pub fn skipped(&self) -> &[SkippedInstance] {
        &self.skipped
    }

    /// The collider entity spawned for `object`, if any.
    #[must_use]
    pub fn collider_for(&self, object: &WorldObjectId) -> Option<Entity> {
        self.colliders
            .iter()
            .find(|spawned| &spawned.object == object)
            .map(|spawned| spawned.entity)
    }

    /// The visual entity spawned for `object`, if any.
    #[must_use]
    pub fn visual_for(&self, object: &WorldObjectId) -> Option<Entity> {
        self.visuals
            .iter()
            .find(|(id, _)| id == object)
            .map(|(_, entity)| *entity)
    }
}

/// Why [`spawn_world`] refused to build the world at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldSpawnError {
    /// An object's authored matrix cannot be represented as a runtime
    /// transform at all. The refusal happens **before the first entity is
    /// spawned**: a world that failed halfway would carry some objects and
    /// not others while the caller holds no [`SpawnedWorld`] to ask which,
    /// so nothing is spawned and nothing is approximated (guessing a
    /// placement is exactly how a traversable opening gets closed).
    UnrepresentableTransform {
        /// The object whose transform was refused.
        object: WorldObjectId,
    },
}

impl std::fmt::Display for WorldSpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnrepresentableTransform { object } => write!(
                f,
                "object `{object}` has an authored matrix no runtime transform can represent"
            ),
        }
    }
}

impl std::error::Error for WorldSpawnError {}

/// The decomposed runtime form of one instance's authored transform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InstanceTransform {
    /// Translation, in meters.
    pub translation: Vec3,
    /// Rotation.
    pub rotation: Quat,
    /// Per-axis scale; every component is non-negative.
    pub scale: Vec3,
}

/// The authored canonical matrix as the runtime sees it: column-major, in
/// meters, narrowed from the record's f64 to the runtime's f32.
///
/// The visual entity and the collider entity are both built from this one
/// value, which is what makes a mismatch between them a code bug rather than
/// two independent derivations that could each be "right".
#[must_use]
pub fn canonical_matrix(transform: &CanonicalTransform) -> Mat4 {
    let linear = transform.linear();
    let translation = transform.translation();
    Mat4::from_cols(
        Vec4::new(
            linear[0][0] as f32,
            linear[1][0] as f32,
            linear[2][0] as f32,
            0.0,
        ),
        Vec4::new(
            linear[0][1] as f32,
            linear[1][1] as f32,
            linear[2][1] as f32,
            0.0,
        ),
        Vec4::new(
            linear[0][2] as f32,
            linear[1][2] as f32,
            linear[2][2] as f32,
            0.0,
        ),
        Vec4::new(
            translation[0] as f32,
            translation[1] as f32,
            translation[2] as f32,
            1.0,
        ),
    )
}

/// Decomposes an authored canonical transform into the translation, rotation
/// and scale a runtime entity is built from, refusing any matrix the
/// decomposition cannot reproduce.
///
/// # Errors
///
/// [`WorldSpawnError::UnrepresentableTransform`] when rebuilding from the
/// decomposition does not land back on the authored matrix within
/// [`INSTANCE_TRANSFORM_TOLERANCE`] — a shear, a mirror or a degenerate
/// scale. [`spawn_world`] runs this over every instance *before* spawning
/// anything, so such a definition is refused whole rather than built at an
/// approximate pose.
pub fn instance_transform(
    object: &WorldObjectInstance,
) -> Result<InstanceTransform, WorldSpawnError> {
    let authored = canonical_matrix(object.transform());
    let (scale, rotation, translation) = authored.to_scale_rotation_translation();
    let rebuilt = Mat4::from_scale_rotation_translation(scale, rotation, translation);
    if !authored.abs_diff_eq(rebuilt, INSTANCE_TRANSFORM_TOLERANCE) {
        return Err(WorldSpawnError::UnrepresentableTransform {
            object: object.id().clone(),
        });
    }
    Ok(InstanceTransform {
        translation,
        rotation,
        scale,
    })
}

/// Maps the engine-independent [`CollisionLayers`] membership onto Avian's
/// layer mask, deriving the filter from [`CollisionLayer::designed_collides_with`]
/// so the two sides can never disagree about who interacts with whom.
#[must_use]
pub fn avian_layers(membership: CollisionLayers) -> AvianCollisionLayers {
    let mut filters = 0_u32;
    for this in CollisionLayer::ALL {
        if membership.contains(this) {
            for other in CollisionLayer::ALL {
                if this.designed_collides_with(other) {
                    filters |= u32::from(other.bit());
                }
            }
        }
    }
    AvianCollisionLayers::from_bits(u32::from(membership.bits()), filters)
}

/// Spawns every instance of `definition` into `app`.
///
/// Installs [`WorldPlugin`] (the contact recorder) the first time it is
/// called, so a caller cannot accidentally run a world whose contacts are
/// never read.
///
/// Every instance's authored matrix is decomposed **before** the first
/// entity exists, so a refusal leaves the app exactly as it was.
///
/// # Errors
///
/// [`WorldSpawnError::UnrepresentableTransform`] naming the object whose
/// authored matrix has no runtime form, with nothing spawned. Every other
/// gap — an unknown role, an unknown shape, a mesh collider deferred to
/// F18-B — is *not* an error: it is reported in [`SpawnedWorld::skipped`] so
/// the gap stays visible.
pub fn spawn_world(
    app: &mut App,
    definition: &WorldDefinition,
) -> Result<SpawnedWorld, WorldSpawnError> {
    // Refuse the whole definition before anything changes — no plugin, no
    // entity; see `WorldSpawnError`.
    let transforms: Vec<InstanceTransform> = definition
        .objects()
        .iter()
        .map(instance_transform)
        .collect::<Result<_, WorldSpawnError>>()?;

    if !app.is_plugin_added::<WorldPlugin>() {
        app.add_plugins(WorldPlugin);
    }

    let world = app.world_mut();
    let mut spawned = SpawnedWorld {
        world: Some(definition.id().clone()),
        ..SpawnedWorld::default()
    };

    for (object, instance) in definition.objects().iter().zip(transforms) {
        let transform = Transform {
            translation: instance.translation,
            rotation: instance.rotation,
            scale: instance.scale,
        };
        let binding = WorldObjectBinding::new(
            definition.id().clone(),
            object.id().clone(),
            object.sectors().to_vec(),
            object.provenance().clone(),
        );

        let visual = world
            .spawn((
                WorldVisual,
                binding.clone(),
                transform,
                GlobalTransform::from(canonical_matrix(object.transform())),
            ))
            .id();
        spawned.visuals.push((object.id().clone(), visual));

        let Some(role) = object.known_collision() else {
            spawned.skipped.push(SkippedInstance {
                object: object.id().clone(),
                reason: SkipReason::UnknownCollisionRole,
            });
            continue;
        };
        if !role.creates_collider() {
            spawned.non_colliding.push(object.id().clone());
            continue;
        }
        let Some(shape) = object.known_shape() else {
            spawned.skipped.push(SkippedInstance {
                object: object.id().clone(),
                reason: SkipReason::UnknownCollisionShape,
            });
            continue;
        };
        let half_extents_m = match shape {
            WorldCollisionShape::Cuboid { half_extents_m } => half_extents_m,
            WorldCollisionShape::FromMesh => {
                spawned.skipped.push(SkippedInstance {
                    object: object.id().clone(),
                    reason: SkipReason::MeshColliderDeferred,
                });
                continue;
            }
        };

        // The box is authored in the instance's local frame; the entity's
        // `Transform` carries the same rotation and scale the visual entity
        // carries, and Avian applies that scale to the collider itself.
        let mut entity = world.spawn((
            WorldColliderInstance::new(role),
            binding,
            transform,
            Position(instance.translation),
            Rotation(instance.rotation),
            RigidBody::Static,
            Collider::cuboid(
                (half_extents_m[0] * 2.0) as f32,
                (half_extents_m[1] * 2.0) as f32,
                (half_extents_m[2] * 2.0) as f32,
            ),
            avian_layers(CollisionLayers::from(CollisionLayer::StaticWorld)),
            CollisionEventsEnabled,
        ));
        if role == WorldCollisionRole::Sensor {
            entity.insert(Sensor);
        }
        let entity = entity.id();
        spawned.colliders.push(SpawnedCollider {
            object: object.id().clone(),
            entity,
            role,
        });
    }

    Ok(spawned)
}
