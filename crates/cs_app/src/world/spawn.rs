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
//! What this stage deliberately does *not* do, and where it goes:
//!
//! * the **simplification policy** for real geometry is a decision, not a
//!   default: this stage performs *none*, and a stage that wants one must
//!   record the source mesh, the operation and the openings it affects
//!   (`docs/findings/2026-09-30-f18-b-world-import-and-static-collision.md`);
//! * an authored matrix no runtime transform can hold (a shear) is refused
//!   whole rather than approximated; whether mesh colliders may instead follow
//!   the render path's full affine is an open limitation, because the
//!   *presented* half of the object cannot;
//! * **mission overlays and visibility-driven streaming policy** are F18-C;
//!   [`super::residency`] owns the load/unload transaction itself;
//! * **retail geometry import** is F18-B/D: everything spawned from a fixture
//!   here is `Origin::SyntheticFixture` and never claims to be original.

use avian3d::prelude::{
    Collider, CollisionEventsEnabled, CollisionLayers as AvianCollisionLayers, Position, RigidBody,
    Rotation, Sensor,
};
use bevy::asset::Assets;
use bevy::mesh::{Mesh, Mesh3d};
use bevy::prelude::{App, Entity, GlobalTransform, Mat4, Quat, Transform, Vec3, Vec4};
use cs_content::scene::CanonicalTransform;
use cs_content::world::{
    WorldCollisionRole, WorldCollisionShape, WorldDefinition, WorldId, WorldObjectId,
    WorldObjectInstance,
};
use cs_sim::collision::{CollisionLayer, CollisionLayers};
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ContentHash;

use super::contacts::{WorldColliderInstance, WorldObjectBinding, WorldVisual};
use super::meshes::WorldMeshes;

/// How far the decomposed runtime transform may drift from the authored
/// canonical matrix and still be called the same transform.
///
/// The runtime frame is f32 (`docs/01-ARCHITECTURE.md`) while canonical
/// records are f64, so this is the representation's own rounding bound, not a
/// tolerance for a wrong placement: it is far tighter than any geometry the
/// fixture can move.
pub const INSTANCE_TRANSFORM_TOLERANCE: f32 = 1e-4;

/// Why an object instance's collider was not built, although it was presented.
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
    /// The record declares a mesh-derived collider, but no upload is
    /// registered for the mesh reference it names.
    ///
    /// The alternative would be to invent geometry — a box around the object's
    /// bounds, a hull of nothing — which is how a solid object becomes
    /// passable. This stage reports the gap instead.
    MeshUnavailable,
}

impl SkipReason {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::UnknownCollisionRole => "unknown_collision_role",
            Self::UnknownCollisionShape => "unknown_collision_shape",
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

/// A collider entity produced from one object instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnedCollider {
    /// The object the collider was built from.
    pub object: WorldObjectId,
    /// The entity that holds the [`Collider`] a narrow phase resolves against.
    /// For a mesh-derived object this is the node Avian derived it onto, which
    /// is also the entity that presents the object.
    pub entity: Entity,
    /// The static rigid body the collider hangs from. For a hand-built collider
    /// this is `entity` itself; for a mesh-derived one it is the node's parent,
    /// because a collider that is not attached to a body collides with nothing
    /// however convincing its shape.
    pub body: Entity,
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
}

impl SpawnedObject {
    /// Every entity this object owns, visual first, then the collider and its
    /// body when they are separate entities. The load transaction despawns
    /// exactly this list, so a despawn can never leave half an object behind.
    #[must_use]
    pub fn entities(&self) -> Vec<Entity> {
        let mut entities = vec![self.visual];
        if let Some(collider) = &self.collider {
            if !entities.contains(&collider.entity) {
                entities.push(collider.entity);
            }
            if !entities.contains(&collider.body) {
                entities.push(collider.body);
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

/// Decomposes an authored canonical transform into the translation, rotation
/// and scale a runtime entity is built from, refusing any matrix the
/// decomposition cannot reproduce.
///
/// # Errors
///
/// [`WorldSpawnError::UnrepresentableTransform`] when rebuilding from the
/// decomposition does not land back on the authored matrix within
/// [`INSTANCE_TRANSFORM_TOLERANCE`] — a shear, or a degenerate scale.
/// [`spawn_world`] runs this over every instance *before* spawning anything,
/// so such a definition is refused whole rather than built at an approximate
/// pose.
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

/// Decomposes the given instances in order, before anything is spawned.
///
/// # Errors
///
/// [`WorldSpawnError::UnrepresentableTransform`] naming the first object whose
/// authored matrix has no runtime form.
pub fn instance_transforms(
    objects: &[&WorldObjectInstance],
) -> Result<Vec<InstanceTransform>, WorldSpawnError> {
    objects
        .iter()
        .map(|object| instance_transform(object))
        .collect()
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
    CollisionLayers::from(CollisionLayer::StaticWorld)
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
        return Err(SkipReason::MeshUnavailable);
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

/// Spawns one object instance into `app`.
///
/// This is the unit [`spawn_world`] and the sector load transaction in
/// [`super::residency`] both use, so a world loaded whole and a sector loaded
/// later produce the *same* entities for the same record.
///
/// # Errors
///
/// [`WorldSpawnError::UnrepresentableTransform`] when the instance's authored
/// matrix has no runtime form; nothing is spawned then.
pub fn spawn_object(
    app: &mut App,
    definition: &WorldDefinition,
    object: &WorldObjectInstance,
    meshes: &WorldMeshes,
) -> Result<SpawnedObject, WorldSpawnError> {
    let instance = instance_transform(object)?;
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
            &instance,
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
            visual: present(app, object, &instance, binding(), resolved),
            collider: None,
            mesh: resolved.map(ResolvedUpload::reference),
            non_colliding: true,
            skipped: None,
        });
    }
    let Some(shape) = object.known_shape() else {
        return Ok(gap(
            app,
            object,
            &instance,
            binding(),
            resolved,
            SkipReason::UnknownCollisionShape,
        ));
    };

    match shape {
        WorldCollisionShape::Cuboid { half_extents_m } => {
            // The box is authored in the instance's local frame; the entity's
            // `Transform` carries the same rotation and scale the visual entity
            // carries, and Avian applies that scale to the collider itself.
            let entity = app
                .world_mut()
                .spawn((
                    WorldColliderInstance::new(role),
                    binding(),
                    transform,
                    Position(instance.translation),
                    Rotation(instance.rotation),
                    RigidBody::Static,
                    Collider::cuboid(
                        (half_extents_m[0] * 2.0) as f32,
                        (half_extents_m[1] * 2.0) as f32,
                        (half_extents_m[2] * 2.0) as f32,
                    ),
                    avian_layers(static_world_membership()),
                    CollisionEventsEnabled,
                ))
                .id();
            if role == WorldCollisionRole::Sensor {
                app.world_mut().entity_mut(entity).insert(Sensor);
            }
            Ok(SpawnedObject {
                object: object.id().clone(),
                visual: spawn_presentation(app, object, &instance, binding()),
                collider: Some(SpawnedCollider {
                    object: object.id().clone(),
                    entity,
                    body: entity,
                    role,
                }),
                mesh: resolved.map(ResolvedUpload::reference),
                non_colliding: false,
                skipped: None,
            })
        }
        WorldCollisionShape::FromMesh => {
            // A `FromMesh` object whose upload this source does not hold is
            // presented and reported; no geometry is invented for it, so it
            // cannot silently become passable.
            let Some(upload) = resolved else {
                return Ok(gap(
                    app,
                    object,
                    &instance,
                    binding(),
                    None,
                    SkipReason::MeshUnavailable,
                ));
            };
            let (visual, body) = spawn_mesh_collider(app, transform, upload, role, binding());
            Ok(SpawnedObject {
                object: object.id().clone(),
                visual,
                collider: Some(SpawnedCollider {
                    object: object.id().clone(),
                    entity: visual,
                    body,
                    role,
                }),
                mesh: Some(upload.reference()),
                non_colliding: false,
                skipped: None,
            })
        }
    }
}

/// Presents an object whose collision was **not** built, and records why.
///
/// The object keeps its identity and, when its own reference resolved, its
/// geometry: a presented object with a reported gap is a visible hole a
/// consumer can refuse, while a missing object is a world that quietly does not
/// contain what the record said.
fn gap(
    app: &mut App,
    object: &WorldObjectInstance,
    instance: &InstanceTransform,
    binding: WorldObjectBinding,
    upload: Option<ResolvedUpload<'_>>,
    reason: SkipReason,
) -> SpawnedObject {
    SpawnedObject {
        object: object.id().clone(),
        visual: present(app, object, instance, binding, upload),
        collider: None,
        mesh: upload.map(ResolvedUpload::reference),
        non_colliding: false,
        skipped: Some(reason),
    }
}

/// Presents one object: with the geometry its own reference names when there is
/// an upload for it, and with the F18-A marker when there is not.
fn present(
    app: &mut App,
    object: &WorldObjectInstance,
    instance: &InstanceTransform,
    binding: WorldObjectBinding,
    upload: Option<ResolvedUpload<'_>>,
) -> Entity {
    match upload {
        Some(upload) => spawn_mesh_presentation(
            app,
            instance.to_transform(),
            upload.upload.mesh().clone(),
            binding,
        ),
        None => spawn_presentation(app, object, instance, binding),
    }
}

/// Spawns the presentation-only entity of a record: the F18-A marker that
/// carries the object's identity, its authored transform and its binding, with
/// no geometry of its own.
fn spawn_presentation(
    app: &mut App,
    object: &WorldObjectInstance,
    instance: &InstanceTransform,
    binding: WorldObjectBinding,
) -> Entity {
    app.world_mut()
        .spawn((
            WorldVisual,
            binding,
            instance.to_transform(),
            GlobalTransform::from(canonical_matrix(object.transform())),
        ))
        .id()
}

/// Presents geometry for an object that never collides: the upload goes into
/// the world's asset stack once and a `Mesh3d` points at it. No rigid body and
/// no collider are created, so role `None` stays a presentation.
fn spawn_mesh_presentation(
    app: &mut App,
    transform: Transform,
    mesh: Mesh,
    binding: WorldObjectBinding,
) -> Entity {
    let handle = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);
    app.world_mut()
        .spawn((
            WorldVisual,
            binding,
            transform,
            GlobalTransform::from(transform),
            Mesh3d(handle),
        ))
        .id()
}

/// Builds a mesh-derived static collider and the node that presents it.
///
/// The node Avian derives the collider onto is the same entity that holds the
/// [`Mesh3d`] the object draws, so there is one asset handle behind both
/// consumers: Avian reads that handle to build the collider, and the render
/// path reads the same handle to draw. The collider is
/// `ColliderConstructor::TrimeshFromMesh` — the constructor that keeps every
/// stored triangle, so nothing here can close a traversable opening.
///
/// The *body* carries the authored transform and is the placement authority: a
/// collider attached to it follows the body's pose, while the node's own
/// transform stays identity in body-local space. That is why the report's
/// `body` and `entity` are two entities here and one for a cuboid.
fn spawn_mesh_collider(
    app: &mut App,
    transform: Transform,
    upload: ResolvedUpload<'_>,
    role: WorldCollisionRole,
    binding: WorldObjectBinding,
) -> (Entity, Entity) {
    let node = crate::asset_stack::spawn_static_mesh_collider(
        app,
        upload.upload.mesh().clone(),
        transform,
        static_world_membership(),
    );
    // The `Sensor` marker and the event opt-in belong to the collider, which
    // Avian only creates on a later update; both are read from the collider
    // entity itself, so marking the node now is enough.
    let mut visual = app.world_mut().entity_mut(node.node);
    visual.insert((
        WorldVisual,
        WorldColliderInstance::new(role),
        binding,
        Transform::default(),
        CollisionEventsEnabled,
    ));
    if role == WorldCollisionRole::Sensor {
        visual.insert(Sensor);
    }
    (node.node, node.body)
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
/// Every instance's authored matrix is decomposed **before** the first entity
/// exists, so a refusal leaves the app exactly as it was. An object whose mesh
/// reference resolves to an upload in `meshes` is presented *and* collided from
/// that one upload; anything this source cannot build is reported in
/// [`SpawnedWorld::skipped`].
///
/// # Errors
///
/// [`WorldSpawnError::UnrepresentableTransform`] naming the object whose
/// authored matrix has no runtime form, with nothing spawned. Every other gap
/// — an unknown role, an unknown shape, a mesh this source does not hold — is
/// *not* an error: it is reported in [`SpawnedWorld::skipped`] so the gap stays
/// visible.
pub fn spawn_world(
    app: &mut App,
    definition: &WorldDefinition,
    meshes: &WorldMeshes,
) -> Result<SpawnedWorld, WorldSpawnError> {
    // Refuse the whole definition before anything changes — no entity at all;
    // see `WorldSpawnError`.
    let objects: Vec<&WorldObjectInstance> = definition.objects().iter().collect();
    instance_transforms(&objects)?;

    let mut spawned = SpawnedWorld::of(definition.id());
    for object in objects {
        spawned.record(spawn_object(app, definition, object, meshes)?);
    }
    Ok(spawned)
}
