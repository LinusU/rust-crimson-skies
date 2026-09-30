//! World instances, sectors and collision roles at the Bevy/Avian boundary
//! (F18).
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! Stage **F18-A** declares the typed world contract and the smallest
//! synthetic fixture that proves it; it is deliberately *not* a runtime. The
//! records live engine-free in [`cs_content::world`]; this module is the one
//! place they meet Bevy and Avian:
//!
//! * [`spawn`] converts a validated [`cs_content::world::WorldDefinition`]
//!   into two entities per instance — a [`spawn::WorldVisual`] and a
//!   [`spawn::WorldColliderInstance`] — both built from the *same* authored
//!   transform and stamped with the same [`contacts::WorldObjectBinding`],
//!   so visual and collision cannot drift apart (F18 non-negotiable behavior
//!   1). Every instance it cannot build honestly is listed in
//!   [`spawn::SpawnedWorld::skipped`] with a reason instead of being filled
//!   in with a guess.
//! * [`contacts`] owns [`contacts::WorldObjectBinding`] and the
//!   [`contacts::WorldContacts`] log: which authored object, in which sector,
//!   an actor actually reached, read from Avian's contact events after the
//!   fixed physics step.
//! * [`fixture`] authors the synthetic arch world and the swept probe the
//!   acceptance tests fly through it. It is production bootstrap code in the
//!   same sense as [`crate::synthetic`] and [`crate::physics::fixture`].
//!
//! Stage F18-B (world import, mesh-derived static collision) and F18-C
//! (mission overlays, streaming and teardown) build on these; F18-D audits
//! retail world variants and needs `gpu` + `retail`, which this stage does
//! not claim.
//!
//! What is **not** claimed here: no original world data was read, no sector
//! layout of the original is reproduced, and no runtime consumer beyond this
//! fixture exists yet. The unknowns this stage met are recorded in
//! `docs/findings/2026-09-30-f18-a-world-instances-sectors-and-collision-roles.md`.

pub mod contacts;
pub mod fixture;
pub mod spawn;

pub use contacts::{
    WorldColliderInstance, WorldContact, WorldContacts, WorldObjectBinding, WorldPlugin,
    WorldVisual, record_world_contacts,
};
pub use fixture::{
    NON_COLLIDING_HALF_M, NON_COLLIDING_POS_M, OBJECT_GROUND, OBJECT_LEG_LEFT, OBJECT_LEG_RIGHT,
    OBJECT_LINTEL, OBJECT_NON_COLLIDING, OBJECT_SENSOR, OBJECT_UNEVIDENCED_ROLE,
    OBJECT_UNEVIDENCED_SHAPE, OBJECT_WATER, ProbeError, ProbeSpec, SECTOR_APPROACH, SECTOR_ARCH,
    SECTOR_BEYOND, SENSOR_HALF_M, SENSOR_POS_M, WORLD_KEY, WorldFixture, WorldFixtureBuilder,
    WorldFixtureError, arch_world, object_set, probe_layers, spawn_discrete_probe,
    spawn_swept_probe, static_world_layers,
};
pub use spawn::{
    INSTANCE_TRANSFORM_TOLERANCE, InstanceTransform, SkipReason, SkippedInstance, SpawnedCollider,
    SpawnedWorld, WorldSpawnError, avian_layers, canonical_matrix, instance_transform, spawn_world,
};
