//! Spawning a [`WorldDefinition`] into the Bevy/Avian world (F18-A, F18-B).
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stages `### F18-A` and `### F18-B`.
//!
//! [`spawn_world`] is the one production conversion this stage owns: it walks
//! a validated [`WorldDefinition`] and produces, for every object instance,
//! entities built from the *same* record —
//!
//! * a **presented** entity: for [`WorldCollisionShape::FromMesh`] it carries
//!   the [`Mesh3d`] handle the object draws, and it is the very node Avian
//!   derives the collider onto, so the presented geometry and the collider are
//!   one asset handle; for a cuboid it is the F18-A presentation marker;
//! * a **static collider** for every role that blocks or reports, on the layers
//!   [`cs_sim::collision`] declares: a hand-built cuboid on its own entity, or
//!   a `TrimeshFromMesh` collider derived from that same handle.
//!
//! Both carry the same [`WorldObjectBinding`], so "visual and collision meshes
//! share provenance and coordinate conversion" (F18 non-negotiable behavior 1)
//! is structural: one transform to get right, one mesh reference to resolve,
//! one asset the engine holds. A `TrimeshFromMesh` keeps every triangle the
//! upload stored — no hull, no bounding box, no decimation — so this stage
//! cannot close a traversable opening with a simplification it never made, and
//! [`SpawnedWorld`] reports every instance whose collision could **not** be
//! built instead of silently presenting geometry it never collided with.
//!
//! # One engine asset per mesh, not one per object
//!
//! The **source** already de-duplicates: two object records that name the same
//! mesh resolve to the same [`WorldMesh`](super::meshes::WorldMesh), one upload
//! with one fingerprint, and [`MeshReference`] is what proves it. The **engine**
//! asset has to be shared as well, or N objects naming one mesh add N identical
//! copies of the same geometry to [`Assets<Mesh>`] — the triangles F17-B
//! uploaded once, copied once per instance that draws or collides with them.
//! [`WorldMeshAssets`] is the map that makes it one: the first object to name a
//! reference uploads it, and every later object naming that reference gets a
//! clone of the **same strong handle**, so the second object's `Mesh3d` *is* the
//! first one's and Avian derives both colliders from one asset. The cache is
//! keyed by the authored [`ContentId`] *and* the upload's [`ContentHash`], so a
//! caller that registers different geometry under a reference it used before is
//! given a new asset rather than the one a previous load left behind.
//!
//! **The world loader does keep an owning handle, and it is released with the
//! world.** [`WorldMeshAssets`] holds the strong handles, so a sector that
//! streams away and comes back re-uses the geometry the engine already has
//! instead of uploading the same triangles again — the same lifetime as
//! [`WorldMeshes`] itself, which the caller owns and passes to
//! [`super::residency::load_world`] and [`super::residency::load_sector`] by
//! shared reference, so it outlives every sector. The cost is that the handles
//! must be given up somewhere, and [`super::residency::unload_world`] is that
//! place: it removes the resource, and once the world's entities have been
//! despawned no strong handle is left, so the engine frees the assets rather
//! than the next world paying for the last one's geometry. A **sector** unload
//! releases nothing: per-sector release is a streaming decision, and the policy
//! that would make it ([`super::visibility`]) does not own the assets.
//!
//! **Every body this module spawns carries a `Collider` on its own entity**,
//! which is what makes it visible to Avian's swept CCD: the collider-on-body
//! rule is stated, measured and enforced in [`crate::asset_stack`]. The mesh
//! path is therefore one entity — static body, `Mesh3d` and
//! `ColliderConstructor::TrimeshFromMesh` together — and a cuboid is a second
//! entity beside the presentation. A body whose colliders all lived on children
//! would be skipped by `SweptCcdBodyQuery` and every swept body would pass
//! through the world's geometry, which is the measured failure task #420
//! recorded and task #424 turned into an invariant.
//!
//! **A `Sensor` role is the one exception, and it is the point of the rule
//! rather than a violation of it** (task #401). A trigger volume reports an
//! overlap and never blocks motion, so it must not be something a swept body
//! can be *stopped* by. Avian's `solve_swept_ccd` reaches its candidates
//! through `Query<(&Collider, &ColliderOf)>`
//! (`avian3d-0.7.0/src/dynamics/ccd/mod.rs`), and a collider with no rigid
//! body on itself or an ancestor is given **no** `ColliderOf` at all
//! (`ColliderHierarchyPlugin` only inserts it for a body). A sensor volume is
//! therefore spawned on an entity that carries **no** `RigidBody`: the sweep
//! never sees it, while the narrow phase still does — a standalone collider is
//! a first-class broad-phase citizen (`ColliderTreeType::Standalone`), and
//! `CollisionStart` names a body on neither side of a pair. Measured on the
//! pinned pair (`bevy 0.19.1` / `avian3d 0.7.0`, `SubstepCount(1)`, 120 Hz):
//! see the decision record
//! `docs/findings/2026-10-02-t401-trigger-volume-and-swept-ccd.md` and
//! `accept_f18_b_a_swept_body_crosses_a_world_trigger_volume_untouched`.
//!
//! **Every collider this module spawns also declares its collision layer**, as
//! the [`BodyLayer`] the F23 machinery classifies hits by — including a
//! trigger volume, and that is what makes a trigger volume work for the F23-C
//! spawn preflight rather than only for the discrete narrow phase (task #499).
//! The declared layer is [`static_world_layer`], the very layer the membership
//! mask already carries, so a collider has **one** layer answer and the role is
//! a separate fact: a `Sensor` object is a trigger because of the [`Sensor`]
//! marker and `classify_contact`'s shape class, never because it moved to
//! another layer. Before this, no world collider declared a layer at all, and
//! the preflight's solid cast treats an unclassifiable hit as solid — so a
//! spawn whose first tick swept a trigger volume was clamped against it and had
//! its into-volume velocity removed, while the preflight's second,
//! non-blocking sensor cast recorded nothing. The measurement, the rejected
//! alternatives and the choice are in
//! `docs/findings/2026-10-04-t499-world-sensor-volume-collision-layer.md`.
//!
//! The one place this module is deliberately *not* the single conversion is the
//! mesh path's entity layout, which is spawned here rather than through
//! [`crate::asset_stack::spawn_static_mesh_collider_on_body`] — for two reasons
//! that now have one answer. That helper *is* the collider-on-body layout, and a
//! trigger volume must not use it; and it takes a [`Mesh`] and uploads it itself,
//! which is exactly what a shared [`Handle`] forbids. So the solid layout is
//! written out here too — the same components, in the same order, through the
//! same [`avian_layers`] — and the two bundles are held against each other by
//! `accept_f18_b_the_trigger_and_solid_mesh_paths_differ_only_in_the_body`.
//! `asset_stack` is F00-A's path and this module does not edit it; adding a
//! handle-taking variant there instead would have been a second place to keep in
//! step with this one.

//! What this stage deliberately does *not* do, and where it goes:
//!
//! * the **simplification policy** for real geometry is a decision, not a
//!   default: this stage performs *none*, and a stage that wants one must
//!   record the source mesh, the operation and the openings it affects
//!   (`docs/findings/2026-09-30-f18-b-world-import-and-static-collision.md`);
//! * an authored affine that has no exact placement is refused whole rather
//!   than approximated — but a **shear is not such an affine**: the
//!   *presented* half carries the whole authored matrix in its
//!   `GlobalTransform` (no `Transform` beside it, so Bevy's propagation cannot
//!   overwrite it) and the *collision* half carries the authored linear map
//!   inside its **shape**, which leaves the collider's own pose an exact
//!   translation/rotation/scale. That split is [`super::affine`]'s decision and
//!   this module's one use of it; a mesh-derived object is the exception, for
//!   the reason [`AffinePlacementError::ShearedMeshUndecided`] names;
//! * **mission overlays and visibility-driven streaming policy** are F18-C;
//!   [`super::residency`] owns the load/unload transaction itself;
//! * **retail geometry import** is F18-B/D: everything spawned from a fixture
//!   here is `Origin::SyntheticFixture` and never claims to be original.

use std::collections::BTreeMap;

use avian3d::parry::shape::{Cuboid, SharedShape};
use avian3d::prelude::{
    Collider, ColliderConstructor, CollisionEventsEnabled, CollisionLayers as AvianCollisionLayers,
    Position, RigidBody, Rotation, Sensor,
};
use bevy::asset::{Assets, Handle};
use bevy::mesh::{Mesh, Mesh3d};
use bevy::prelude::{App, Entity, GlobalTransform, Mat4, Quat, Resource, Transform, Vec3, Vec4};
use cs_content::scene::CanonicalTransform;
use cs_content::world::{
    WorldCollisionRole, WorldCollisionShape, WorldDefinition, WorldId, WorldObjectId,
    WorldObjectInstance,
};
use cs_sim::collision::{CollisionLayer, CollisionLayers};
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ContentHash;

use super::affine::{AffinePlacement, AffinePlacementError};
use super::contacts::{WorldColliderInstance, WorldObjectBinding, WorldVisual};
use super::meshes::WorldMeshes;
use crate::physics::BodyLayer;

/// How far the decomposed runtime transform may drift from the authored
/// canonical matrix and still be called the same transform.
///
/// The runtime frame is f32 (`docs/01-ARCHITECTURE.md`) while canonical
/// records are f64, so this is the representation's own rounding bound, not a
/// tolerance for a wrong placement: it is far tighter than any geometry the
/// fixture can move.
pub const INSTANCE_TRANSFORM_TOLERANCE: f32 = 1e-4;

/// Why the geometry an object instance names could not be used, although the
/// object was presented.
///
/// An authored affine that has no exact placement is *not* one of these:
/// [`spawn_world`] refuses that definition outright (see
/// [`WorldSpawnError::UnplaceableAffine`]) before any entity exists, so it
/// never reaches the report. A shear is not such an affine — see
/// [`super::affine`].
///
/// A mesh reason can surface twice over in one report, for two different
/// objects: a colliding object reports it in
/// [`SpawnedObject::skipped`] and a non-colliding one in
/// [`SpawnedObject::presentation_gap`]. An object never reports both, so a
/// consumer reads one reason per object rather than two that must be merged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// The record's collision role is an explicit unknown.
    UnknownCollisionRole,
    /// The record's collision shape is an explicit unknown.
    UnknownCollisionShape,
    /// The record's mesh reference is an explicit unknown, so there is nothing
    /// to resolve.
    ///
    /// A *content* gap: the evidence never named which mesh this object draws
    /// or collides with, which is the same class of fact as an unknown role or
    /// shape. A retail import fills it with evidence; this stage cannot.
    UnknownMesh,
    /// The record names a mesh, but no upload is registered for that reference.
    ///
    /// A *load* gap: the record is complete and this source simply does not hold
    /// the geometry. The alternative would be to invent geometry — a box around
    /// the object's bounds, a hull of nothing — which is how a solid object
    /// becomes passable. This stage reports the gap instead.
    MeshUnavailable,
}

impl SkipReason {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::UnknownCollisionRole => "unknown_collision_role",
            Self::UnknownCollisionShape => "unknown_collision_shape",
            Self::UnknownMesh => "unknown_mesh",
            Self::MeshUnavailable => "mesh_unavailable",
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

/// Which mesh an object was built from, and what that mesh was.
///
/// The reference is the record's own; the fingerprint and the triangle count
/// are the F17-B upload's. A collider carrying a different triangle count than
/// this record has been substituted for the authored geometry, which F18
/// non-negotiable behavior 1 forbids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeshReference {
    /// The authored mesh reference the instance's `mesh` field resolved to.
    pub id: ContentId,
    /// The canonical fingerprint of the upload both consumers used.
    pub fingerprint: ContentHash,
    /// How many triangles that upload stored.
    pub triangles: usize,
}

impl MeshReference {
    /// Records the reference and the provenance of one upload.
    #[must_use]
    pub fn new(id: ContentId, upload: &super::meshes::WorldMesh) -> Self {
        Self {
            id,
            fingerprint: upload.fingerprint(),
            triangles: upload.triangles(),
        }
    }
}

/// One engine mesh asset as this Bevy world holds it: the handle, and the
/// upload that handle is the geometry of.
///
/// The fingerprint is what makes the entry safe to re-use. A `ContentId` says
/// *which* mesh; the fingerprint says *which upload of it*, and a caller that
/// registered different geometry under the same reference must be given a new
/// asset instead of the previous one.
#[derive(Clone, Debug)]
struct SharedWorldMesh {
    fingerprint: ContentHash,
    handle: Handle<Mesh>,
}

/// The engine mesh assets this Bevy world holds for the mesh references its
/// world object records named.
///
/// One [`WorldMesh`](super::meshes::WorldMesh) becomes **one** asset, however
/// many objects name it: the first object to name a reference uploads it into
/// [`Assets<Mesh>`], and every later object gets a clone of that same strong
/// handle. Presentation-only and colliding objects therefore share one
/// [`Mesh3d`] handle, and so does the collider Avian derives from it, which is
/// what "one upload, one fingerprint, one asset" means at the engine boundary.
///
/// **This resource is the world loader's owning handle**, and it is released by
/// [`super::residency::unload_world`], which removes it. See the module docs for
/// why a sector unload deliberately does not.
#[derive(Resource, Clone, Debug, Default)]
pub struct WorldMeshAssets {
    assets: BTreeMap<ContentId, SharedWorldMesh>,
}

impl WorldMeshAssets {
    /// The asset this Bevy world already holds for `id`, when it holds the
    /// upload named by `fingerprint`.
    #[must_use]
    pub fn handle(&self, id: &ContentId, fingerprint: ContentHash) -> Option<Handle<Mesh>> {
        self.assets
            .get(id)
            .filter(|shared| shared.fingerprint == fingerprint)
            .map(|shared| shared.handle.clone())
    }

    /// How many engine mesh assets this world holds for world object records.
    ///
    /// The number of **meshes**, not the number of objects: a world whose
    /// objects name six records over two meshes holds two assets here. It is a
    /// statement about the loader's own bookkeeping, and the engine's own count
    /// is `Assets::<Mesh>::len()` — the two agree for a Bevy world this crate
    /// built, and a consumer that needs the engine's number should read it.
    #[must_use]
    pub fn len(&self) -> usize {
        self.assets.len()
    }

    /// Whether this world holds no world mesh asset at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    /// Records `handle` as the asset for the upload named by `fingerprint`,
    /// replacing any earlier asset for `id`.
    fn insert(&mut self, id: ContentId, fingerprint: ContentHash, handle: Handle<Mesh>) {
        self.assets.insert(
            id,
            SharedWorldMesh {
                fingerprint,
                handle,
            },
        );
    }
}

/// A collider entity produced from one object instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnedCollider {
    /// The object the collider was built from.
    pub object: WorldObjectId,
    /// The entity that holds the [`Collider`] a narrow phase resolves against.
    /// For a mesh-derived object this is also the entity that presents the
    /// object, and the collider does not exist until Avian's
    /// `init_collider_constructors` has run.
    pub entity: Entity,
    /// The static rigid body the collider hangs from, when it hangs from one.
    ///
    /// For world geometry it is `entity` itself: a body that carries no
    /// `Collider` of its own is skipped by Avian's swept CCD
    /// (`crate::asset_stack`, the collider-on-body rule). It is reported
    /// separately because a consumer asking *where is the body* must not have
    /// to know that the two happen to be one entity.
    ///
    /// It is `None` for exactly one role: [`WorldCollisionRole::Sensor`], whose
    /// collider is deliberately spawned on an entity with **no** rigid body so
    /// Avian's swept CCD cannot stop a body there (task #401; the module docs
    /// and [`trigger_volume_layout`] carry the measurement). A standalone
    /// collider is not a broken collider — Avian gives it its own broad-phase
    /// tree and still reports its overlaps — but a consumer that needs a body
    /// entity to stamp, move or despawn must be told there is none rather than
    /// handed an entity that carries no body.
    pub body: Option<Entity>,
    /// The role the record declared for it.
    pub role: WorldCollisionRole,
}

/// One object's entities, as both [`spawn_world`] and the load transaction in
/// [`super::residency`] need them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnedObject {
    /// The object these entities belong to.
    pub object: WorldObjectId,
    /// The entity that presents it.
    pub visual: Entity,
    /// The collider it was given, when the record asked for one.
    pub collider: Option<SpawnedCollider>,
    /// The upload it was built from, when it was built from one.
    pub mesh: Option<MeshReference>,
    /// Whether the record's role was `None`: presented, never blocking, and a
    /// deliberate answer rather than a gap.
    pub non_colliding: bool,
    /// Why no collider was built, when the record asked for one.
    pub skipped: Option<SkipReason>,
    /// Why the object presents no geometry, when its record asked for no
    /// collider at all (role [`WorldCollisionRole::None`]).
    ///
    /// A role-`None` object asked for no collision, so nothing was *skipped* —
    /// but its presentation is still built from the mesh its own record names,
    /// and a gap there is a gap: without this field a banner whose upload nobody
    /// supplied would be presented as a bare marker that draws nothing, and the
    /// report would claim the world is complete. Only set when
    /// [`SpawnedObject::skipped`] is `None`, so one object never reports the
    /// same missing upload twice.
    pub presentation_gap: Option<SkipReason>,
}

impl SpawnedObject {
    /// Every entity this object owns, visual first, then the collider and its
    /// body when they are separate entities. The load transaction despawns
    /// exactly this list, so a despawn can never leave half an object behind.
    ///
    /// A body-less trigger volume contributes its collider entity once: there is
    /// no second entity to despawn, and nothing here can invent one.
    #[must_use]
    pub fn entities(&self) -> Vec<Entity> {
        let mut entities = vec![self.visual];
        if let Some(collider) = &self.collider {
            if !entities.contains(&collider.entity) {
                entities.push(collider.entity);
            }
            if let Some(body) = collider.body
                && !entities.contains(&body)
            {
                entities.push(body);
            }
        }
        entities
    }
}

/// What [`spawn_world`] produced for one [`WorldDefinition`].
#[derive(Clone, Debug, Default)]
pub struct SpawnedWorld {
    world: Option<WorldId>,
    objects: Vec<SpawnedObject>,
}

impl SpawnedWorld {
    /// An empty report for `world`, to be filled by [`SpawnedWorld::record`].
    #[must_use]
    pub fn of(world: &WorldId) -> Self {
        Self {
            world: Some(world.clone()),
            objects: Vec::new(),
        }
    }

    /// Adds one object's spawn to this report.
    pub fn record(&mut self, object: SpawnedObject) {
        self.objects.push(object);
    }

    /// The spawned world's identity.
    #[must_use]
    pub fn world(&self) -> Option<&WorldId> {
        self.world.as_ref()
    }

    /// Every object's spawn, in the order it was walked.
    #[must_use]
    pub fn objects(&self) -> &[SpawnedObject] {
        &self.objects
    }

    /// `(object, visual entity)` pairs, in walk order.
    #[must_use]
    pub fn visuals(&self) -> Vec<(&WorldObjectId, Entity)> {
        self.objects
            .iter()
            .map(|spawned| (&spawned.object, spawned.visual))
            .collect()
    }

    /// The colliders, in walk order.
    #[must_use]
    pub fn colliders(&self) -> Vec<SpawnedCollider> {
        self.objects
            .iter()
            .filter_map(|spawned| spawned.collider.clone())
            .collect()
    }

    /// The objects whose collision role is [`WorldCollisionRole::None`]:
    /// presented, never blocking — a deliberate answer, not a skip.
    #[must_use]
    pub fn non_colliding(&self) -> Vec<WorldObjectId> {
        self.objects
            .iter()
            .filter(|spawned| spawned.non_colliding)
            .map(|spawned| spawned.object.clone())
            .collect()
    }

    /// Every instance whose collision was **not** built, with the reason. A
    /// non-empty result is a visible gap, never a silent pass.
    #[must_use]
    pub fn skipped(&self) -> Vec<SkippedInstance> {
        self.objects
            .iter()
            .filter_map(|spawned| {
                spawned.skipped.map(|reason| SkippedInstance {
                    object: spawned.object.clone(),
                    reason,
                })
            })
            .collect()
    }

    /// How many instances were presented without a collider.
    #[must_use]
    pub fn skipped_count(&self) -> usize {
        self.objects
            .iter()
            .filter(|spawned| spawned.skipped.is_some())
            .count()
    }

    /// Every instance that presents no geometry because no upload answers its own
    /// mesh reference, and which asked for no collider in the first place.
    ///
    /// These objects are *not* in [`SpawnedWorld::skipped`]: a role-`None` record
    /// wanted nothing a body could reach, so no collision was declined. What is
    /// missing is what it would have drawn, which is a gap a consumer has to be
    /// able to see before it presents a world it believes is complete.
    #[must_use]
    pub fn presentation_gaps(&self) -> Vec<SkippedInstance> {
        self.objects
            .iter()
            .filter_map(|spawned| {
                spawned.presentation_gap.map(|reason| SkippedInstance {
                    object: spawned.object.clone(),
                    reason,
                })
            })
            .collect()
    }

    /// How many instances present no geometry.
    #[must_use]
    pub fn presentation_gap_count(&self) -> usize {
        self.objects
            .iter()
            .filter(|spawned| spawned.presentation_gap.is_some())
            .count()
    }

    /// The one object's spawn, by its authored id.
    #[must_use]
    pub fn object(&self, object: &WorldObjectId) -> Option<&SpawnedObject> {
        self.objects
            .iter()
            .find(|spawned| &spawned.object == object)
    }

    /// The collider entity spawned for `object`, if any.
    #[must_use]
    pub fn collider_for(&self, object: &WorldObjectId) -> Option<Entity> {
        self.object(object)
            .and_then(|spawned| spawned.collider.as_ref())
            .map(|collider| collider.entity)
    }

    /// The visual entity spawned for `object`, if any.
    #[must_use]
    pub fn visual_for(&self, object: &WorldObjectId) -> Option<Entity> {
        self.object(object).map(|spawned| spawned.visual)
    }
}

/// Why [`spawn_world`] refused to build the world at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldSpawnError {
    /// One object's authored affine has no exact placement; see
    /// [`AffinePlacementError`] for the reasons, each of them a fact about the
    /// record and never a guess. A shear is deliberately *not* one of them.
    ///
    /// The refusal happens **before the first entity is spawned**: a world that
    /// failed halfway would carry some objects and not others while the caller
    /// holds no [`SpawnedWorld`] to ask which, so nothing is spawned and
    /// nothing is approximated (guessing a placement is exactly how a
    /// traversable opening gets closed). [`instance_placements`] is what makes
    /// that true for *every* reason here, including the one only a bake can
    /// find.
    UnplaceableAffine {
        /// The object whose authored affine was refused.
        object: WorldObjectId,
        /// Why it has no exact placement.
        source: AffinePlacementError,
    },
}

impl std::fmt::Display for WorldSpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnplaceableAffine { object, source } => {
                write!(f, "object `{object}` has no exact placement: {source}")
            }
        }
    }
}

impl std::error::Error for WorldSpawnError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::UnplaceableAffine { source, .. } => Some(source),
        }
    }
}

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

impl InstanceTransform {
    /// The Bevy transform both the visual and the collider are built from.
    #[must_use]
    pub const fn to_transform(self) -> Transform {
        Transform {
            translation: self.translation,
            rotation: self.rotation,
            scale: self.scale,
        }
    }
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

/// Classifies one instance's authored affine: which runtime half carries the
/// linear map, and — for a sheared *mesh* — that this stage does not build it.
///
/// The test for [`AffinePlacement::Trs`] is exactly the one F18-A used, so
/// every matrix F18-A placed is still placed the same way. What changed is the
/// other branch: a matrix that is not a rotation-times-scale product is no
/// longer refused for being one.
///
/// # Errors
///
/// [`WorldSpawnError::UnplaceableAffine`] when the authored affine has no exact
/// placement — see [`AffinePlacement::of`] — or when a mesh-derived object is
/// the sheared one, for
/// [`AffinePlacementError::ShearedMeshUndecided`]. [`spawn_world`] runs this
/// over every instance *before* spawning anything, so such a definition is
/// refused whole rather than built at an approximate pose.
pub fn instance_placement(
    object: &WorldObjectInstance,
) -> Result<AffinePlacement, WorldSpawnError> {
    let refuse = |source: AffinePlacementError| WorldSpawnError::UnplaceableAffine {
        object: object.id().clone(),
        source,
    };
    let placement = AffinePlacement::of(object.transform()).map_err(refuse)?;
    // A sheared cuboid bakes into a convex polyhedron here, so it is placed.
    // A sheared *mesh* cannot be, and the reason is not an engine limit: the
    // collision would have to become a **derived** upload whose fingerprint is
    // no longer the authored one, which is F18-B's one-asset provenance claim
    // and therefore F18-B's decision, not this module's. It is refused by name
    // rather than placed from a second asset the report would misattribute.
    if placement.is_sheared() && object.known_shape() == Some(WorldCollisionShape::FromMesh) {
        return Err(refuse(AffinePlacementError::ShearedMeshUndecided));
    }
    Ok(placement)
}

/// The collision geometry one declared cuboid is built from: the authored box
/// in the instance's local frame, read through the one production conversion.
/// Exactly what [`AffinePlacement::bake`] starts from, so the pre-flight and
/// the spawn cannot disagree about the box.
fn authored_cuboid(half_extents_m: [f64; 3]) -> SharedShape {
    SharedShape::new(Cuboid::new(Vec3::new(
        half_extents_m[0] as f32,
        half_extents_m[1] as f32,
        half_extents_m[2] as f32,
    )))
}

/// Classifies the given instances in order, before anything is spawned.
///
/// This is the **pre-flight**, and it covers every reason
/// [`WorldSpawnError::UnplaceableAffine`] can carry — including the one only
/// the bake can find ([`AffinePlacementError::UnbuildableCollision`]). That
/// matters for the atomicity [`WorldSpawnError`] promises: `spawn_world`
/// spawns object by object, so a reason found during the *second* object's
/// bake would leave the first object behind with no `SpawnedWorld` to ask
/// which. The declared cuboid is therefore baked here and discarded — the same
/// function the spawn calls, on the same record, so the two agree by
/// construction rather than by luck.
///
/// # Errors
///
/// [`WorldSpawnError::UnplaceableAffine`] naming the first object that has no
/// exact placement, with its reason.
pub fn instance_placements(
    objects: &[&WorldObjectInstance],
) -> Result<Vec<AffinePlacement>, WorldSpawnError> {
    let mut placements = Vec::with_capacity(objects.len());
    for object in objects {
        let placement = instance_placement(object)?;
        if let Some(WorldCollisionShape::Cuboid { half_extents_m }) = object.known_shape() {
            placement
                .bake(&authored_cuboid(half_extents_m))
                .map_err(|source| WorldSpawnError::UnplaceableAffine {
                    object: object.id().clone(),
                    source,
                })?;
        }
        placements.push(placement);
    }
    Ok(placements)
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

/// The static world membership every world collider carries, whether it was
/// hand-built or derived from a mesh.
#[must_use]
pub fn static_world_membership() -> CollisionLayers {
    CollisionLayers::from(static_world_layer())
}

/// The one layer [`static_world_membership`] names — and therefore the
/// **declared layer** every world collider carries as its [`BodyLayer`], whatever
/// its role (task #499).
///
/// One answer per collider, on purpose. The world spawn has always put exactly
/// this layer in the engine's membership mask; making it the declared layer too
/// means a consumer never has to ask which of two layer components is the real
/// one. The **role** is a separate fact and stays separate: it is carried by the
/// [`Sensor`] marker, which is what `classify_contact` reads for the shape
/// class, so a trigger volume is a sensor by its record's role and not by
/// living on another layer.
///
/// **What it buys, measured.** `cs_app::physics::preflight` classifies a cast
/// hit by reading the hit entity's [`BodyLayer`] and its [`Sensor`] marker, and
/// had no way to classify a world collider, because none carried one. The
/// preflight's solid cast treats an unclassifiable hit as solid
/// (`is_none_or(|kind| kind == ContactKind::SolidContact)`), so a spawn whose
/// first tick swept a world trigger volume was **clamped against it and had its
/// into-volume velocity removed** — F23-C's
/// `accept_f23_c_preflight_never_stops_on_a_sensor` violated for exactly the
/// volumes a mission cares about — and the preflight's second, non-blocking
/// sensor cast accepts only `Some(ContactKind::SensorOverlap)`, so the crossing
/// was never recorded and the gameplay consumer never saw it. Measured on the
/// arch world's `trigger.sensor` with a 10 cm swept projectile at 120 Hz:
///
/// | fired | travel/tick | clamped | into-volume velocity | `passed` | end position |
/// | --- | --- | --- | --- | --- | --- |
/// | 60 m/s | 0.50 m | **yes**, at 0.200 m | **removed**, `vz` 60 → 0 | `None` | held at the volume's near face |
/// | 600 m/s | 5.00 m | **yes**, at 2.000 m | **removed**, `vz` 600 → 0 | `None` | held at the volume's near face |
///
/// Solid world geometry clamped at 0.200 m in the same geometry, so the defect
/// was specific to the layer-less volume being taken as solid by absence.
/// `accept_t499_a_world_sensor_volume_never_clamps_stops_or_delays_a_spawn` and
/// `accept_t499_a_spawn_tick_crossing_of_a_world_volume_reaches_the_consumer`
/// hold the repaired side; the decision and its rejected alternatives are in
/// `docs/findings/2026-10-04-t499-world-sensor-volume-collision-layer.md`.
#[must_use]
pub fn static_world_layer() -> CollisionLayer {
    CollisionLayer::StaticWorld
}

/// One instance's resolved mesh: the record's own reference and the upload that
/// reference names, if this source holds it.
#[derive(Clone, Copy)]
struct ResolvedUpload<'a> {
    id: &'a ContentId,
    upload: &'a super::meshes::WorldMesh,
}

impl ResolvedUpload<'_> {
    /// The report entry for this upload: the reference and the upload's own
    /// fingerprint and triangle count.
    fn reference(self) -> MeshReference {
        MeshReference::new(self.id.clone(), self.upload)
    }
}

/// What a record's mesh reference resolved to, or why it did not.
///
/// `Ok(None)` is a record whose shape is a box, so it has no mesh to resolve;
/// `Err` is a `FromMesh` record whose reference no upload answers, which is a
/// visible gap rather than a geometry this stage may invent.
type ResolvedMeshes<'a> = Result<Option<ResolvedUpload<'a>>, SkipReason>;

/// Resolves the upload an instance's collision and presentation share.
fn resolve_upload<'a>(
    object: &'a WorldObjectInstance,
    meshes: &'a WorldMeshes,
) -> ResolvedMeshes<'a> {
    if object.known_shape() != Some(WorldCollisionShape::FromMesh) {
        return Ok(None);
    }
    let Resolved::Known(known) = object.mesh() else {
        return Err(SkipReason::UnknownMesh);
    };
    meshes
        .get(&known.value)
        .map(|upload| {
            Some(ResolvedUpload {
                id: &known.value,
                upload,
            })
        })
        .ok_or(SkipReason::MeshUnavailable)
}

/// The one engine asset **both halves** of `upload` use: the asset this Bevy
/// world already holds for that exact upload, or a newly uploaded one.
///
/// This is the whole of the sharing, and it is deliberately the only place the
/// engine mesh is added. Two records that resolve to the same
/// [`WorldMesh`](super::meshes::WorldMesh) end up holding the *same*
/// [`Handle`], and two records that resolve to different meshes cannot meet in
/// the cache: a different [`ContentId`], or the same id with different geometry,
/// is a different entry and a different asset.
///
/// # Panics
///
/// If `app` has no [`Assets<Mesh>`] to add to, because it was not built by
/// [`super::fixture::world_app`] or otherwise given [`crate::asset_stack`].
fn shared_mesh(app: &mut App, upload: ResolvedUpload<'_>) -> Handle<Mesh> {
    let id = upload.id.clone();
    let fingerprint = upload.upload.fingerprint();
    if let Some(handle) = app
        .world()
        .get_resource::<WorldMeshAssets>()
        .and_then(|assets| assets.handle(&id, fingerprint))
    {
        return handle;
    }
    // Uploaded here and nowhere else in this module: this is the only line that
    // puts a world mesh into the engine's asset stack.
    let handle = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(upload.upload.mesh().clone());
    app.world_mut()
        .get_resource_or_insert_with(WorldMeshAssets::default)
        .insert(id, fingerprint, handle.clone());
    handle
}

/// Spawns one object instance into `app`.
///
/// This is the unit [`spawn_world`] and the sector load transaction in
/// [`super::residency`] both use, so a world loaded whole and a sector loaded
/// later produce the *same* entities for the same record.
///
/// # Errors
///
/// [`WorldSpawnError::UnplaceableAffine`] when the instance's authored affine
/// has no exact placement; nothing is spawned then.
pub fn spawn_object(
    app: &mut App,
    definition: &WorldDefinition,
    object: &WorldObjectInstance,
    meshes: &WorldMeshes,
) -> Result<SpawnedObject, WorldSpawnError> {
    let placement = instance_placement(object)?;
    let instance = placement.collider_pose();
    let binding = || {
        WorldObjectBinding::new(
            definition.id().clone(),
            object.id().clone(),
            object.sectors().to_vec(),
            object.surface().clone(),
            object.provenance().clone(),
        )
    };
    let transform = instance.to_transform();
    // Resolved once, before any entity exists, for both consumers.
    let upload = resolve_upload(object, meshes);
    let resolved = upload
        .as_ref()
        .ok()
        .and_then(|upload| upload.as_ref())
        .copied();

    // An unresolved role is a reportable gap, never a default.
    let Some(role) = object.known_collision() else {
        return Ok(gap(
            app,
            object,
            &placement,
            binding(),
            resolved,
            SkipReason::UnknownCollisionRole,
        ));
    };
    // `None` is a deliberate answer, not a gap: presented, and never given
    // anything a body could reach.
    if !role.creates_collider() {
        return Ok(SpawnedObject {
            object: object.id().clone(),
            visual: present(app, object, &placement, binding(), resolved),
            collider: None,
            mesh: resolved.map(ResolvedUpload::reference),
            non_colliding: true,
            skipped: None,
            // Nothing was declined, but what this object would have drawn may be
            // missing: that is a gap of its own, and it is reported here.
            presentation_gap: upload.as_ref().err().copied(),
        });
    }
    let Some(shape) = object.known_shape() else {
        return Ok(gap(
            app,
            object,
            &placement,
            binding(),
            resolved,
            SkipReason::UnknownCollisionShape,
        ));
    };

    match shape {
        WorldCollisionShape::Cuboid { half_extents_m } => {
            // The box is authored in the instance's local frame. A
            // rotation-times-scale instance needs nothing more: Avian applies
            // the entity's own scale to the collider. A **sheared** instance
            // cannot — Avian derives `Position`, `Rotation` and the collider
            // scale from a translation/rotation/scale decomposition — so the
            // authored linear map goes into the *shape* and the pose keeps only
            // the translation. Same vertices, same conversion, one asset.
            let authored_box = authored_cuboid(half_extents_m);
            let geometry = placement.bake(&authored_box).map_err(|source| {
                WorldSpawnError::UnplaceableAffine {
                    object: object.id().clone(),
                    source,
                }
            })?;
            let entity = spawn_cuboid_collider(app, geometry, instance, transform, role, binding());
            Ok(SpawnedObject {
                object: object.id().clone(),
                visual: present(app, object, &placement, binding(), resolved),
                collider: Some(SpawnedCollider {
                    object: object.id().clone(),
                    entity,
                    body: (!is_trigger_volume(role)).then_some(entity),
                    role,
                }),
                mesh: resolved.map(ResolvedUpload::reference),
                non_colliding: false,
                skipped: None,
                presentation_gap: None,
            })
        }
        WorldCollisionShape::FromMesh => {
            // A `FromMesh` object with no upload to build from is presented and
            // reported under the reason the reference itself failed: an
            // unevidenced reference is a content gap, a reference this source
            // does not hold is a load gap, and the two must not look alike.
            // No geometry is invented either way, so it cannot silently become
            // passable.
            let Some(upload) = resolved else {
                let reason = upload
                    .as_ref()
                    .err()
                    .copied()
                    .unwrap_or(SkipReason::UnknownMesh);
                return Ok(gap(app, object, &placement, binding(), None, reason));
            };
            let entity = spawn_mesh_collider(app, transform, upload, role, binding());
            Ok(SpawnedObject {
                object: object.id().clone(),
                visual: entity,
                collider: Some(SpawnedCollider {
                    object: object.id().clone(),
                    entity,
                    body: (!is_trigger_volume(role)).then_some(entity),
                    role,
                }),
                mesh: Some(upload.reference()),
                non_colliding: false,
                skipped: None,
                presentation_gap: None,
            })
        }
    }
}

/// Presents an object whose collision was **not** built, and records why.
///
/// The object keeps its identity and, when its own reference resolved, its
/// geometry: a presented object with a reported gap is a visible hole a
/// consumer can refuse, while a missing object is a world that quietly does not
/// contain what the record said. The reason is reported once, as a skip: an
/// object whose collider was declined does not also report a presentation gap,
/// even when the very same missing upload is why it draws nothing.
fn gap(
    app: &mut App,
    object: &WorldObjectInstance,
    placement: &AffinePlacement,
    binding: WorldObjectBinding,
    upload: Option<ResolvedUpload<'_>>,
    reason: SkipReason,
) -> SpawnedObject {
    SpawnedObject {
        object: object.id().clone(),
        visual: present(app, object, placement, binding, upload),
        collider: None,
        mesh: upload.map(ResolvedUpload::reference),
        non_colliding: false,
        skipped: Some(reason),
        presentation_gap: None,
    }
}

/// Presents one object: with the geometry its own reference names when there is
/// an upload for it, and with the F18-A marker when there is not.
fn present(
    app: &mut App,
    object: &WorldObjectInstance,
    placement: &AffinePlacement,
    binding: WorldObjectBinding,
    upload: Option<ResolvedUpload<'_>>,
) -> Entity {
    let affine = GlobalTransform::from(canonical_matrix(object.transform()));
    let pose = pose_for(placement);
    match upload {
        Some(upload) => {
            // The shared handle, so a record that names a mesh another record
            // already named draws the *same* engine asset rather than a second
            // copy of the same geometry.
            let handle = shared_mesh(app, upload);
            spawn_mesh_presentation(app, pose, affine, handle, binding)
        }
        None => spawn_presentation(app, object, pose, binding),
    }
}

/// The `Transform` a presentation entity for `placement` carries, if any.
///
/// A **representable** placement carries one: the authored matrix decomposed,
/// which for such a matrix equals the authored matrix, so this is exactly what
/// F18-A built and what [`super::overlays::displace_object`] writes to move a
/// drawn object.
///
/// A **sheared** placement carries **none**. Bevy's transform propagation
/// recomputes a `GlobalTransform` from a `Transform` on the same entity, so a
/// second component would silently replace the shear with a
/// translation/rotation/scale on the first update — measured on the pinned pair;
/// see [`super::affine`]. The consequence is recorded rather than hidden: a
/// sheared object has no `Transform` for a displacement to write to, and
/// [`super::overlays::displace_object`] must refuse it rather than move the
/// collision half and leave the drawn half behind.
fn pose_for(placement: &AffinePlacement) -> Option<Transform> {
    match placement {
        AffinePlacement::Trs { transform } => Some(transform.to_transform()),
        AffinePlacement::Sheared { .. } => None,
    }
}

/// Spawns the presentation-only entity of a record: the F18-A marker that
/// carries the object's identity, its authored transform and its binding, with
/// no geometry of its own.
///
/// The entity carries the **whole** authored affine in its `GlobalTransform`,
/// plus a `Transform` only when the affine has one — see
/// [`pose_for`] for why a shear may not have one.
fn spawn_presentation(
    app: &mut App,
    object: &WorldObjectInstance,
    pose: Option<Transform>,
    binding: WorldObjectBinding,
) -> Entity {
    let affine = GlobalTransform::from(canonical_matrix(object.transform()));
    if let Some(pose) = pose {
        app.world_mut()
            .spawn((WorldVisual, binding, pose, affine))
            .id()
    } else {
        app.world_mut().spawn((WorldVisual, binding, affine)).id()
    }
}

/// Presents geometry for an object that never collides: the object's shared
/// asset handle goes on a `Mesh3d` and nothing else is created. No rigid body
/// and no collider are created, so role `None` stays a presentation.
///
/// The handle is the one [`shared_mesh`] hands out, which is what lets a
/// presentation-only record share its engine asset with every other record that
/// names the same mesh.
///
/// The pose components are the same choice [`spawn_presentation`] makes, and for
/// the same reason: see [`pose_for`].
fn spawn_mesh_presentation(
    app: &mut App,
    pose: Option<Transform>,
    affine: GlobalTransform,
    handle: Handle<Mesh>,
    binding: WorldObjectBinding,
) -> Entity {
    if let Some(pose) = pose {
        app.world_mut()
            .spawn((WorldVisual, binding, pose, affine, Mesh3d(handle)))
            .id()
    } else {
        app.world_mut()
            .spawn((WorldVisual, binding, affine, Mesh3d(handle)))
            .id()
    }
}

/// Spawns one hand-built cuboid collider at `instance`'s pose, on the layers
/// [`cs_sim::collision`] declares for world geometry.
///
/// The bundle is the same whichever role the record declared, except for the
/// [`RigidBody`]: a `Solid` cuboid is static world geometry, so it gets
/// [`RigidBody::Static`] and is therefore something a swept body is stopped by
/// (the collider-on-body rule, [`crate::asset_stack`]); a `Sensor` cuboid is a
/// trigger volume, so it gets **no** rigid body at all and Avian's swept CCD
/// cannot reach it — see [`trigger_volume_layout`] and the module docs. The
/// event opt-in, the [`Sensor`] marker and the declared [`BodyLayer`] are on the
/// collider's own entity, which is where Avian reads the first two from and
/// where the F23 preflight reads the third from.
fn spawn_cuboid_collider(
    app: &mut App,
    geometry: SharedShape,
    instance: InstanceTransform,
    transform: Transform,
    role: WorldCollisionRole,
    binding: WorldObjectBinding,
) -> Entity {
    let mut entity = app.world_mut().spawn((
        WorldColliderInstance::new(role),
        binding,
        transform,
        Position(instance.translation),
        Rotation(instance.rotation),
        Collider::from(geometry),
        avian_layers(static_world_membership()),
        // The declared layer, one answer for both roles: see
        // `static_world_layer`. Without it a swept spawn cannot classify this
        // collider at all, and an unclassifiable hit is taken as solid.
        BodyLayer(static_world_layer()),
        CollisionEventsEnabled,
    ));
    if is_trigger_volume(role) {
        entity.insert(Sensor);
    } else {
        entity.insert(RigidBody::Static);
    }
    entity.id()
}

/// Builds a mesh-derived static collider and the node that presents it.
///
/// The node Avian derives the collider onto is the same entity that holds the
/// [`Mesh3d`] the object draws, so there is one asset handle behind both
/// consumers: Avian reads that handle to build the collider, and the render
/// path reads the same handle to draw. That handle is the **shared** one
/// ([`shared_mesh`]), so "one asset handle behind both consumers" now holds
/// *across* objects too: two records naming one mesh present and collide from a
/// single [`Assets<Mesh>`] entry. The collider is
/// `ColliderConstructor::TrimeshFromMesh` — the constructor that keeps every
/// stored triangle, so nothing here can close a traversable opening, and sharing
/// the asset does not change what the constructor derives from it.
///
/// **For `Solid`, that node is also the rigid body.** The entity carries
/// [`RigidBody::Static`], the [`Mesh3d`] and the constructor together, so the
/// derived [`Collider`] lands on the body entity itself rather than on a child
/// of it. This is the collider-on-body rule
/// ([`crate::asset_stack`]): Avian's `solve_swept_ccd` resolves a body through
/// `SweptCcdBodyQuery`, whose `collider: &'static Collider` field is read off
/// the body entity, so a body whose colliders live on descendants is skipped
/// and a swept body passes straight through it. Measured here: against this
/// layout the production 400 m/s swept probe is clamped at the arch's near
/// face, where the parent-body-plus-child-node layout tunnelled with an empty
/// contact log (`docs/findings/2026-09-30-t420-mesh-ccd-decision.md`).
///
/// So the report's `body` and `entity` are the *same* entity for a mesh object
/// and still two for a cuboid (whose presentation is its own entity), and one
/// entity carries the binding, the presentation and the collider because they
/// are one object. No separate body entity is spawned, so nothing is left to
/// despawn on unload and nothing to stamp.
fn spawn_mesh_collider(
    app: &mut App,
    transform: Transform,
    upload: ResolvedUpload<'_>,
    role: WorldCollisionRole,
    binding: WorldObjectBinding,
) -> Entity {
    // The same handle whichever layout this role takes, and the same asset every
    // other record naming this mesh already got: a trigger volume is a
    // presentation too, so sharing has to reach it as well.
    let handle = shared_mesh(app, upload);
    if is_trigger_volume(role) {
        // A trigger volume: the same bundle with the rigid body left off, so the
        // derived collider is a standalone one. Spawned here rather than by
        // removing a body afterwards, so the entity never exists as a body
        // Avian's `ColliderOf` observer may already have bound a collider to.
        return spawn_mesh_trigger_volume(
            app,
            handle,
            transform,
            static_world_membership(),
            binding,
        );
    }
    let entity = spawn_mesh_body(app, transform, handle, static_world_membership());
    // The event opt-in is read from the collider's own entity, and the derived
    // collider lands there, so marking it now is enough.
    app.world_mut().entity_mut(entity).insert((
        WorldVisual,
        WorldColliderInstance::new(role),
        binding,
        CollisionEventsEnabled,
    ));
    entity
}

/// Spawns one mesh-derived **static body**: the collider-on-body layout, on an
/// already-shared asset handle.
///
/// These are the components of
/// [`crate::asset_stack::spawn_static_mesh_collider_on_body`], in its order,
/// through the same [`avian_layers`]. It is written out here rather than called
/// because that helper takes a [`Mesh`] and uploads it itself — which is exactly
/// what sharing one asset between objects forbids — and `asset_stack` is F00-A's
/// path, which this stage does not edit.
///
/// The returned entity is the body, the presentation node and the constructor's
/// entity at once, which is the collider-on-body rule: Avian's
/// `init_collider_constructors` derives onto the entity holding the constructor,
/// so the [`Collider`] lands on the body itself and swept CCD can resolve it.
///
/// # Panics
///
/// If `app` was not given [`Assets<Mesh>`] — it does not add one here, so the
/// caller must have a mesh asset stack (see [`crate::asset_stack`]).
fn spawn_mesh_body(
    app: &mut App,
    transform: Transform,
    handle: Handle<Mesh>,
    membership: CollisionLayers,
) -> Entity {
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
            BodyLayer(static_world_layer()),
            transform,
            Position(transform.translation),
            Rotation(transform.rotation),
        ))
        .id()
}

/// Spawns a mesh-derived **trigger volume**: presentation, binding, layers,
/// declared layer, event opt-in and [`Sensor`] marker on one entity that carries
/// **no** [`RigidBody`].
///
/// This is [`spawn_mesh_body`] minus the rigid body and plus the sensor's own
/// marker, and the difference is the whole decision (task #401): with a
/// body on the entity, Avian's `ColliderHierarchyPlugin` binds the derived
/// collider to it through `ColliderOf`, `solve_swept_ccd` resolves that body
/// through `SweptCcdBodyQuery`, and a swept body is stopped at the volume's
/// near face — measured, 2.416 m of a 400 m/s probe's tick. Without a body the
/// collider is never bound, the sweep skips the pair before it casts anything,
/// and the narrow phase still reports the overlap the volume exists for. The
/// bundle is written out here rather than reached for in `asset_stack`,
/// because that helper is the collider-on-body layout and this is its opposite;
/// `accept_f18_b_the_trigger_and_solid_mesh_paths_differ_only_in_the_body` is
/// what holds the two to each other.
///
/// The declared [`BodyLayer`] is **not** one of the differences (task #499):
/// both layouts declare [`static_world_layer`], and the sensor/solid split
/// reaches the classification through the [`Sensor`] marker and
/// `classify_contact`'s shape class.
fn spawn_mesh_trigger_volume(
    app: &mut App,
    handle: Handle<Mesh>,
    transform: Transform,
    membership: CollisionLayers,
    binding: WorldObjectBinding,
) -> Entity {
    // `ColliderConstructor::TrimeshFromMesh` derives from the `Mesh3d` on the
    // *same* entity, and the constructor resolves no body, so the collider lands
    // here as a standalone one. Every stored triangle is kept, exactly as on
    // the solid path: this changes where the collider sits, never what it is.
    app.world_mut()
        .spawn((
            WorldVisual,
            WorldColliderInstance::new(WorldCollisionRole::Sensor),
            binding,
            Mesh3d(handle),
            ColliderConstructor::TrimeshFromMesh,
            avian_layers(membership),
            BodyLayer(static_world_layer()),
            Sensor,
            CollisionEventsEnabled,
            transform,
            Position(transform.translation),
            Rotation(transform.rotation),
        ))
        .id()
}

/// Whether a record's role asked for a **trigger volume**: a sensor the world
/// reports an overlap for and that must never hold a body still.
///
/// The role is the record's own claim about whether a body can *enter* the
/// volume, and this predicate is the single place that couples that claim to
/// the entity layout: a rigid body is exactly what gives Avian's swept CCD
/// something to stop a body against, so a trigger volume is spawned without
/// one. Both spawn paths and the report's `body` field ask this, so they cannot
/// disagree about which objects a swept body may be held by.
const fn is_trigger_volume(role: WorldCollisionRole) -> bool {
    // Written here rather than as `role == Sensor` at each call site, so a
    // fourth role has one place to be decided and one place to be measured.
    matches!(role, WorldCollisionRole::Sensor)
}

/// Spawns every instance of `definition` into `app`.
///
/// The contact recorder ([`WorldPlugin`]) is part of the **app composition**,
/// not of this call: [`super::fixture::world_app`] adds it before the app is
/// finished, because a world load happens *after* `App::finish` in a real
/// mission and an `add_plugins` at that point would panic. An app that runs a
/// world without it records no contacts — a gap in that composition, not
/// something this conversion can repair.
///
/// Every instance's authored affine is classified **before** the first entity
/// exists, so a refusal leaves the app exactly as it was. An object whose mesh
/// reference resolves to an upload in `meshes` is presented *and* collided from
/// that one upload; anything this source cannot build is reported in
/// [`SpawnedWorld::skipped`] or, for an object that asked for no collider at
/// all, in [`SpawnedWorld::presentation_gaps`].
///
/// # Errors
///
/// [`WorldSpawnError::UnplaceableAffine`] naming the object whose authored
/// affine has no exact placement, with nothing spawned. Every other gap
/// — an unknown role, an unknown shape, a mesh this source does not hold — is
/// *not* an error: it is reported next to the objects that *were* built, so the
/// gap stays visible.
pub fn spawn_world(
    app: &mut App,
    definition: &WorldDefinition,
    meshes: &WorldMeshes,
) -> Result<SpawnedWorld, WorldSpawnError> {
    // Refuse the whole definition before anything changes — no entity at all;
    // see `WorldSpawnError`.
    let objects: Vec<&WorldObjectInstance> = definition.objects().iter().collect();
    instance_placements(&objects)?;

    let mut spawned = SpawnedWorld::of(definition.id());
    for object in objects {
        spawned.record(spawn_object(app, definition, object, meshes)?);
    }
    Ok(spawned)
}
