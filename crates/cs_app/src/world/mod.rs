//! World instances, sectors and collision roles at the Bevy/Avian boundary
//! (F18-A, F18-B).
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stages `### F18-A` and `### F18-B`. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! * [`contacts`] owns [`contacts::WorldObjectBinding`] and the
//!   [`contacts::WorldContacts`] log: which authored object, in which sector,
//!   under which gameplay surface rule, an actor actually reached — read from
//!   Avian's contact events after the fixed physics step.
//! * [`meshes`] is the one place a mesh reference meets an upload
//!   ([`meshes::WorldMeshes`], [`meshes::WorldMesh`]), so the geometry a
//!   collision is built from and the geometry that is drawn are one value with
//!   one fingerprint.
//! * [`spawn`] converts a validated [`cs_content::world::WorldDefinition`]
//!   into entities per instance, all built from the *same* authored transform
//!   and stamped with the same [`contacts::WorldObjectBinding`], so visual and
//!   collision cannot drift apart (F18 non-negotiable behavior 1). A
//!   `FromMesh` object is presented and collided by **one** node holding one
//!   `Mesh3d` handle, which Avian derives a `TrimeshFromMesh` collider from —
//!   no hull, no bounding box, no decimation, so a traversable opening cannot
//!   be closed by a simplification this stage never made. Every instance it
//!   cannot build honestly is listed in [`spawn::SpawnedWorld::skipped`] with a
//!   reason instead of being filled in with a guess.
//! * [`residency`] is the load transaction: one world at a time, one sector in
//!   and out at a time, and the per-object condition that survives both
//!   (acceptance scenario AC02). It carries no streaming *policy* — that is
//!   F18-C.
//! * [`fixture`] authors the synthetic arch world, the mesh-authored harbor
//!   world, the swept probe and the headless harness the acceptance tests fly
//!   through. It is production bootstrap code in the same sense as
//!   [`crate::synthetic`] and [`crate::physics::fixture`].
//!
//! What is **not** claimed here: no original world data was read, no sector
//! layout of the original is reproduced, no simplification policy for retail
//! geometry exists yet, and no runtime consumer beyond these fixtures exists.
//! The unknowns this stage met are recorded in
//! `docs/findings/2026-09-30-f18-b-world-import-and-static-collision.md`.

pub mod contacts;
pub mod fixture;
pub mod meshes;
pub mod residency;
pub mod spawn;

pub use contacts::{
    WorldColliderInstance, WorldContact, WorldContacts, WorldObjectBinding, WorldPlugin,
    WorldVisual, record_world_contacts,
};
pub use fixture::{
    HARBOR_HANGAR_HULL_TRIANGLES, HARBOR_HANGAR_TRIANGLES, HARBOR_OBJECT_ABSENT,
    HARBOR_OBJECT_BANNER, HARBOR_OBJECT_GROUND, HARBOR_OBJECT_HANGAR, HARBOR_OBJECT_SENSOR,
    HARBOR_OBJECT_WATER, HARBOR_SECTOR_APPROACH, HARBOR_SECTOR_YARD, HARBOR_SENSOR_HALF_M,
    HARBOR_SENSOR_POS_M, HARBOR_WATER_OFF_AXIS_Z_M, HARBOR_WATER_POS_M, MESH_SETTLE_UPDATES,
    NON_COLLIDING_HALF_M, NON_COLLIDING_POS_M, OBJECT_GROUND, OBJECT_LEG_LEFT, OBJECT_LEG_RIGHT,
    OBJECT_LINTEL, OBJECT_NON_COLLIDING, OBJECT_SENSOR, OBJECT_UNEVIDENCED_ROLE,
    OBJECT_UNEVIDENCED_SHAPE, OBJECT_WATER, ProbeError, ProbeSpec, SECTOR_APPROACH, SECTOR_ARCH,
    SECTOR_BEYOND, SENSOR_HALF_M, SENSOR_POS_M, WORLD_KEY, WorldFixture, WorldFixtureBuilder,
    WorldFixtureError, arch_world, harbor_meshes, harbor_world, mesh_reference, object_set,
    probe_layers, spawn_discrete_probe, spawn_swept_probe, static_world_layers, world_app,
    world_instance,
};
pub use meshes::{WorldMesh, WorldMeshes};
pub use residency::{
    ObjectCondition, ResidentWorld, SectorLoad, WorldLoadError, WorldResidency, condition_of,
    damage_object, load_sector, load_world, residency, unload_sector, unload_world,
};
pub use spawn::{
    INSTANCE_TRANSFORM_TOLERANCE, InstanceTransform, MeshReference, SkipReason, SkippedInstance,
    SpawnedCollider, SpawnedObject, SpawnedWorld, WorldSpawnError, avian_layers, canonical_matrix,
    instance_transform, instance_transforms, spawn_object, spawn_world, static_world_membership,
};
