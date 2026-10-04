//! World instances, sectors, collision roles, mission overlays and streaming
//! policy at the Bevy/Avian boundary (F18-A, F18-B, F18-C).
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stages `### F18-A`, `### F18-B` and `### F18-C`. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! * [`contacts`] owns [`contacts::WorldObjectBinding`] and the
//!   [`contacts::WorldContacts`] log: which authored object, in which sector,
//!   under which gameplay surface rule, an actor actually reached — read from
//!   Avian's contact events after the fixed physics step.
//! * [`crossings`] is the swept trigger-crossing producer (task #498): it
//!   decides a crossing from the body's own per-tick segment swept against a
//!   `WorldCollisionRole::Sensor` collider, keeps the per-pair inside bit, and
//!   records each pair's entry once into the shared
//!   [`crate::objectives::TriggerCrossings`] stream — the crossings a discrete
//!   overlap cannot see because no sample lands inside a volume thinner than a
//!   tick. Its entries feed the same [`overlays::OverlayTriggerRequests`]
//!   hand-off the contact stream feeds.
//! * [`meshes`] is the one place a mesh reference meets an upload
//!   ([`meshes::WorldMeshes`], [`meshes::WorldMesh`]), so the geometry a
//!   collision is built from and the geometry that is drawn are one value with
//!   one fingerprint.
//! * [`spawn`] converts a validated [`cs_content::world::WorldDefinition`]
//!   into entities per instance, all built from the *same* authored transform
//!   and stamped with the same [`contacts::WorldObjectBinding`], so visual and
//!   collision cannot drift apart (F18 non-negotiable behavior 1). A `FromMesh`
//!   object is presented and collided by **one** node holding one `Mesh3d`
//!   handle, which Avian derives a `TrimeshFromMesh` collider from — and that
//!   handle is the **shared** one in [`spawn::WorldMeshAssets`], so every record
//!   naming a given mesh presents and collides from a single engine asset rather
//!   than one copy per object. No hull, no
//!   bounding box, no decimation, so a traversable opening cannot be closed by a
//!   simplification this stage never made. That node is also the static rigid
//!   body, so the derived collider lands on the body entity itself and the
//!   object is visible to swept CCD (the collider-on-body rule in
//!   [`crate::asset_stack`], tasks #420/#424). Every instance it cannot build
//!   honestly is listed in [`spawn::SpawnedWorld::skipped`] (or, for an object
//!   that asked for no collider but has no geometry to draw, in
//!   [`spawn::SpawnedWorld::presentation_gaps`]) with a reason instead of being
//!   filled in with a guess.
//! * [`residency`] is the load transaction: one world at a time, one sector in
//!   and out at a time, and the per-object condition that survives both
//!   (acceptance scenario AC02). It carries no streaming *policy* — that is
//!   [`visibility`] and [`overlays`] (acceptance scenarios AC02 and AC03).
//! * [`affine`] owns the one decision F18-A's review left open (#421): how an
//!   authored affine that no translation/rotation/scale triple reproduces — a
//!   shear — is placed. The presentation keeps the whole affine and the
//!   collision carries the linear map inside its **shape**, so neither half
//!   approximates and no affine is refused for being sheared.
//! * [`overlays`] is the mission-overlay runtime: the producer that turns a
//!   sensor volume a body reached into a request, the hand-off between the two
//!   ends, and the consumer that applies the load's declared effect to an
//!   object's *every* entity, so the drawn geometry and the collided geometry
//!   cannot end up in different places (acceptance scenario AC03). Which
//!   overlays have been applied lives in the load record, so it survives a
//!   sector unload and is gone with the world.
//! * [`crossings`] is the swept half of what feeds [`overlays`] (task #498): a
//!   body whose fixed tick completely outruns a thin volume, or lands a
//!   mesh-derived one's deep-inside gap, still produces its `TriggerCrossing` —
//!   decided from the body's own tick segment, once per pair, as a read that
//!   never touches the body — and the same hand-off applies its overlay.
//! * [`triggers`] measures the **original's** trigger volumes rather than the
//!   fixtures': it reads every world container's node array through the
//!   production F11-A node reader and reports one measured detection-zone
//!   extent per numbered `dzpath<N>` node, with the container key, the
//!   container's SHA-256, the installation fingerprint and the node's own byte
//!   span. It supplies **no** stored-unit-to-metre factor, because the
//!   original's world-vertex unit is unmeasured, so the one-tick question comes
//!   back as `UnitUnmeasured` carrying the factor at which it would flip rather
//!   than as a verdict (task #427).
//! * [`visibility`] is the streaming policy: which sectors a focus holds, which
//!   it holds *anyway* because gameplay requires an object in them, and what a
//!   streamed-out sector's state is summarized as when it comes back.
//! * [`fixture`] authors the synthetic arch world, the mesh-authored harbor
//!   world, the door-equipped depot world, the swept probe and the headless
//!   harness the acceptance tests fly through. It is production bootstrap code
//!   in the same sense as [`crate::synthetic`] and [`crate::physics::fixture`].
//!
//! * [`audit`] is the F18-D evidence instrument: it discovers every world group
//!   an installation declares, reads each group's own geometry container through
//!   the production GameZ readers, and hands
//!   [`cs_content::world::WorldGroupAudit`] one measured census per group. What
//!   it cannot establish — the world placement and the stored vertex unit — is
//!   reported as the blocker it is, not filled in.
//! * [`gpu_capture`] is the F18-D `gpu` half: it draws one group's **real**
//!   stored mesh on the real renderer, offscreen, and writes a PNG — refusing a
//!   blank frame and deleting the file on every refusal, so an artifact on disk
//!   is evidence the geometry was drawn rather than a decoration.
//!
//! What is **not** claimed here: original world data has been read and counted,
//! but **no traversal route and no stunt opening has been located in it**, because
//! the GameZ node array is undecoded and the stored vertex unit is unmeasured; the
//! original's sector layout is not reproduced, no simplification policy for retail
//! geometry exists yet, and the original's own streaming rule is unmeasured. The
//! unknowns this feature met are recorded in
//! `docs/findings/2026-09-30-f18-a-world-instances-sectors-and-collision-roles.md`,
//! `docs/findings/2026-09-30-f18-b-world-import-and-static-collision.md`,
//! `docs/findings/2026-09-30-f18-c-mission-overlays-and-visibility-streaming.md` and
//! `docs/findings/2026-09-30-f18-d-world-group-audit-and-gpu-capture.md`.

pub mod affine;
pub mod audit;
pub mod contacts;
pub mod crossings;
pub mod fixture;
pub mod gpu_capture;
pub mod meshes;
pub mod overlays;
pub mod residency;
pub mod retail;
pub mod spawn;
pub mod triggers;
pub mod visibility;

pub use affine::{AffinePlacement, AffinePlacementError, bake_shape, shear_residual};
pub use audit::{
    GEOMETRY_CONTAINER_FILE, PRESENTABLE_PROBE_MESHES, REPRESENTATIVE_MESHES, SurveyedContainer,
    SurveyedWorldGroup, TEXTURE_ARCHIVE_FILE, WorldGroupSurvey, WorldGroupSurveyError,
    audit_survey, audit_world_groups, declared_rows, survey_world_groups, upload_verdict,
};
pub use contacts::{
    WorldColliderInstance, WorldContact, WorldContacts, WorldObjectBinding, WorldPlugin,
    WorldVisual, record_world_contacts,
};
pub use crossings::{SweptBodyTracks, WorldSweptCrossingPlugin, sweep_volume_crossings};
pub use fixture::{
    DEPOT_CRATE_HALF_M, DEPOT_CRATE_POS_M, DEPOT_DOOR_HALF_M, DEPOT_DOOR_OPEN_OFFSET_M,
    DEPOT_DOOR_POS_M, DEPOT_OBJECT_CRATE, DEPOT_OBJECT_DOOR, DEPOT_OBJECT_GROUND,
    DEPOT_OBJECT_HANGAR, DEPOT_OBJECT_TRIGGER, DEPOT_SECTOR_ANNEX, DEPOT_SECTOR_APPROACH,
    DEPOT_SECTOR_YARD, DEPOT_TRIGGER_HALF_M, DEPOT_TRIGGER_POS_M, DEPOT_WORLD_KEY,
    HARBOR_HANGAR_HULL_TRIANGLES, HARBOR_HANGAR_TRIANGLES, HARBOR_OBJECT_ABSENT,
    HARBOR_OBJECT_BANNER, HARBOR_OBJECT_GROUND, HARBOR_OBJECT_HANGAR, HARBOR_OBJECT_SENSOR,
    HARBOR_OBJECT_WATER, HARBOR_SECTOR_APPROACH, HARBOR_SECTOR_YARD, HARBOR_SENSOR_HALF_M,
    HARBOR_SENSOR_POS_M, HARBOR_WATER_OFF_AXIS_Z_M, HARBOR_WATER_POS_M, HARBOR_WORLD_KEY,
    MESH_SETTLE_UPDATES, NON_COLLIDING_HALF_M, NON_COLLIDING_POS_M, OBJECT_GROUND, OBJECT_LEG_LEFT,
    OBJECT_LEG_RIGHT, OBJECT_LINTEL, OBJECT_NON_COLLIDING, OBJECT_SENSOR, OBJECT_UNEVIDENCED_ROLE,
    OBJECT_UNEVIDENCED_SHAPE, OBJECT_WATER, ProbeError, ProbeSpec, SECTOR_APPROACH, SECTOR_ARCH,
    SECTOR_BEYOND, SENSOR_HALF_M, SENSOR_POS_M, TWIN_OBJECT_BANNER, TWIN_OBJECT_GROUND,
    TWIN_OBJECT_PANEL, TWIN_OBJECT_SHELL_A, TWIN_OBJECT_SHELL_B, TWIN_OBJECT_TRIGGER, TWIN_SECTOR,
    TWIN_WORLD_KEY, WORLD_KEY, WorldFixture, WorldFixtureBuilder, WorldFixtureError, arch_world,
    depot_meshes, depot_mission, depot_population, depot_world, door_overlay, fixture_provenance,
    harbor_meshes, harbor_world, mesh_reference, object_set, probe_layers, spawn_discrete_probe,
    spawn_swept_probe, static_world_layers, twin_harbor_meshes, twin_harbor_world, world_app,
    world_app_with_spawn_preflight, world_instance,
};
pub use gpu_capture::{
    CAPTURE_HEIGHT, CAPTURE_WIDTH, CaptureRequest, FRAMING_DISTANCE_FACTOR, GpuCapture,
    GpuCaptureError, capture_world_mesh,
};
pub use meshes::{WorldMesh, WorldMeshBuildError, WorldMeshGroup, WorldMeshes};
pub use overlays::{
    AppliedOverlay, OverlayError, OverlayOutcome, OverlayTriggerRequests, WorldOverlayLog,
    WorldOverlayPlugin, apply_overlay, apply_overlay_requests, displace_object, overlay_log,
    queue_overlay_triggers, reapply_object, request_overlay,
};
pub use residency::{
    ObjectCondition, ResidentWorld, SectorLoad, WorldLoadError, WorldResidency, condition_of,
    damage_object, load_sector, load_world, residency, unload_sector, unload_world,
};
pub use retail::{
    GRID_IS_THE_SECTOR_INDEX, RETAIL_WORLD_IMPORT, RetailWorldContainer, RetailWorldError,
    read_world_container,
};
pub use spawn::{
    INSTANCE_TRANSFORM_TOLERANCE, InstanceTransform, MeshReference, SkipReason, SkippedInstance,
    SpawnedCollider, SpawnedObject, SpawnedWorld, WorldMeshAssets, WorldSpawnError, avian_layers,
    canonical_matrix, instance_placement, instance_placements, spawn_object, spawn_world,
    static_world_layer, static_world_membership,
};
pub use triggers::{
    TriggerVolumeSurveyError, ZONE_PREFIX, survey_retail_trigger_volumes, zone_box_field,
};
pub use visibility::{
    VisibilityError, VisibilityRequest, VisibilityUpdate, holds_sector, retained_sectors,
    update_visibility,
};
