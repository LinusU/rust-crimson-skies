//! One **original** world area and one **original** aircraft, spawned for a
//! free-flight playtest and captured from a real GPU (`PLAYTEST-RETAIL-SCENE`,
//! task #648).
//!
//! Feature sheets this stage leans on:
//! `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`
//! (`### F18-A`, `### F18-B`, `### F18-D`) and
//! `specs/F10-gamez-mesh-topology-and-material-records.md` (`### F10-C.02`,
//! `### F10-C.03`). Shared contracts: `docs/contracts/IDENTITY-CONTENT.md` and
//! `docs/contracts/CLI-EVIDENCE.md`.
//!
//! # The label every surface of this stage carries
//!
//! [`PLAYTEST_LABEL`] is the one string a screenshot, a log line or a window
//! title must show for this work:
//!
//! ```text
//! ORIGINAL ASSETS / DEVELOPMENT FREE FLIGHT / PROVISIONAL TUNING
//! ```
//!
//! **Never** `M01`, never "faithful", never "campaign". The geometry and the
//! aircraft are the owner's original bytes read through the production readers;
//! the world scale, the handedness, the spawn, the camera, the lighting, the
//! collision classification and the material are **designed development choices**
//! this module names and `docs/PLAYTEST-RETAIL.md` records. `retail` is read
//! access to the owner's files: **no original run happened**, so nothing here is
//! `verified_original` and nothing here is a claim about how the 2000 engine
//! loaded, streamed, drew or collided with a world.
//!
//! # The gap this closes
//!
//! Task #629 (`M01-LC-WORLD-IMPORT`) turned an original `gamez.zbd` into the
//! [`WorldDefinition`] that [`spawn_world`](crate::world::spawn_world) consumes
//! — and it imports what the world record's own **partition grid** and **stored
//! child list** name. Measured over `ZBD/C1C/gamez.zbd` that is **346 of the
//! container's 5 644 nodes**, and what those 346 records draw is 1 786 triangles:
//! the world's *spatial index*, not the world's *visible content*. The remaining
//! 5 298 nodes are the level itself — the parts, panels, interiors and airship
//! hulls — reachable only by descending the node hierarchy. So a mission could
//! load an original world and see almost nothing in it.
//!
//! This module is that hop, in three steps with no step optional:
//!
//! 1. **read** the two containers this scene renders through the production
//!    discovery pass and the production GameZ readers ([`read_playtest_sources`]);
//! 2. **select and compose** one documented area of the world container and the
//!    aircraft's own airframe through the production [`SceneGraph::build`]
//!    validator ([`area_graph`], [`aircraft_graph`]);
//! 3. **spawn** the area as world records whose collision is derived from the
//!    very triangles they draw, place the aircraft, and capture the result on a
//!    real renderer ([`spawn_playtest_scene`], [`capture_playtest_views`]).
//!
//! # The one area and the one aircraft
//!
//! Both are **pinned constants**, not a search and not a guess:
//!
//! * the area is one node subtree of `ZBD/C1C/gamez.zbd`: node slot
//!   [`PLAYTEST_AREA_NODE_SLOT`] ([`PLAYTEST_AREA_NODE_NAME`]), a record of the
//!   world node's own stored child list. Measured over the production readers:
//!   793 nodes in the subtree, 401 of them binding a mesh, 8 673 stored
//!   triangles, composed extent 106 × 218 × 840 canonical metres. Its siblings
//!   are four other airships of the same shape, 20 volumetric fog boxes, a 17 km
//!   horizon dome and ~45 vegetation instances, while the world record's own
//!   spatial index adds 144 flat 1 024-unit quads at `y = 960`.
//! * the aircraft is **one** mesh from `ZBD/planes.zbd`: node slot
//!   [`PLAYTEST_AIRCRAFT_MESH_NODE_SLOT`] ([`PLAYTEST_AIRCRAFT_MESH_NODE_NAME`]), the
//!   fuselage LOD variant of the airframe rooted at
//!   [`PLAYTEST_AIRCRAFT_ROOT_NAME`]. Measured: mesh-array slot 1 436, 140 stored
//!   triangles over 3 material groups, stored extent 2.11 × 1.49 × 10.23 units,
//!   and an identity composed transform — the mesh is already in its airframe's
//!   own frame.
//!
//! One mesh is one mesh on purpose. The airframe's other mesh bindings (wings,
//! propellers, engines, canopies, the wreck variants) are **not** claimed here;
//! the full silhouette is the next step, and a precise subset beats a collection
//! of half-read records.
//!
//! # Why the area's graph is built over a subtree
//!
//! [`cs_content::scene::SceneGraph::build`] refuses the **container-wide** `c1c`
//! hierarchy: measured, node 642 names parent slot 0 while the world record's
//! child list does not name it back, and the production validator refuses that
//! disagreement as [`SceneError::InconsistentParentage`]. #629 recorded the same
//! blocker from the other side (world node names the `scene_node` id grammar
//! refuses). Rather than re-implement the transform conversion here — which would
//! be a second path for one canonical rule (AGENTS rule 7) — this module calls
//! the **same** validator over a narrower node set: the world record plus the
//! documented subtree, with the world record's child list narrowed to the
//! subtree root. Every check the validator makes still runs (unique slots,
//! in-range links, parent↔child agreement on both sides, acyclicity, derived
//! ids, finite transforms), and the composed transforms are the container's own,
//! because the only ancestor above the subtree root is the world record, whose
//! kind carries no transform and therefore converts to the identity.
//!
//! # The designed values, and what stays unknown
//!
//! Every choice below is a **declared development value** with its own claim id,
//! and each is recorded in `docs/PLAYTEST-RETAIL.md`:
//!
//! | choice | value | claim |
//! | --- | --- | --- |
//! | stored unit | 1 stored unit = 1 canonical metre | [`PLAYTEST_UNIT_IS_DESIGNED`] |
//! | axis map / handedness | identity, right-handed, `+Y` up | [`PLAYTEST_UNIT_IS_DESIGNED`] |
//! | area selection | the pinned node subtree | [`PLAYTEST_AREA_IS_DESIGNED`] |
//! | collision classification | every area record `Solid` + `FromMesh` | [`PLAYTEST_COLLISION_IS_THE_DRAWN_MESH`] |
//! | aircraft pose | [`spawn_pose`] off the area's port side, nose on the runtime's forward axis | [`PLAYTEST_AIRCRAFT_POSE_IS_DESIGNED`] |
//! | camera views | [`camera_poses`], three of them, derived from the measured bounds | [`PLAYTEST_VIEWS_ARE_DESIGNED`] |
//! | material | one neutral development material | [`PLAYTEST_NEUTRAL_MATERIAL`] |
//!
//! What stays **unknown**, and is recorded as unknown rather than decided here:
//!
//! * **the original's world-vertex unit and coordinate handedness** (task #436,
//!   blocked). [`PLAYTEST_UNIT_IS_DESIGNED`] is a designed reading of the bytes,
//!   not a measurement: nothing in this module can tell a stored unit from a
//!   foot.
//! * **the original's collision classification.** The area's records are declared
//!   solid and collided from the mesh they draw. Whether the original collided a
//!   panel, an engine cowl or a cloud the same way is unmeasured, and #629's own
//!   rule ("an indexed record is a collider") is a *different* rule over a
//!   *different* record set.
//! * **the original's lighting, sky and weather**, and every material identity.
//!   The material path is the one place this stage is deliberately short: the
//!   per-material texture binding (F10-C.02's audit through F17's image upload)
//!   is **not** built here, so every drawn surface uses
//!   [`PLAYTEST_NEUTRAL_MATERIAL`] — reported once, on [`PlaytestMaterial`], with
//!   the source ids it covers. Drawing a stored texture is the immediate next
//!   step; PLAYTEST-RETAIL-HANDOFF should take it rather than mistake this
//!   stage's flat shading for a property of the original.
//! * **which stored axis is an airframe's nose.**
//!   [`PLAYTEST_AIRCRAFT_POSE_IS_DESIGNED`] maps the measured hint (the
//!   propeller disc lies in the stored `x`/`y` plane and its node sits at
//!   `z = +4.80`, the fuselage mesh's own maximum) onto the runtime's forward
//!   axis. The mapping is a designed presentation choice; the hint is what the
//!   bytes say.
//!
//! # What this module does not claim
//!
//! * **Not a mission, and not `M01`.** It spawns an area and an aircraft. There
//!   is no campaign, no objective, no actor and no script here, and this lane
//!   does not block the synthetic flight loop.
//! * **Not a flight loop.** [`spawn_playtest_scene`] produces a scene and a pose;
//!   a consumer drives it. This module owns no window, no input and no fixed-step
//!   integration, so PLAYTEST-FLY-NOW can keep the generic loop.
//! * **Not an area census.** The counts the retail test pins are the counts of
//!   one documented subtree, not of a world and not of the installation. #639
//!   still covers all eight world containers.
//!
//! # Ownership and teardown
//!
//! [`spawn_playtest_scene`] is the only writer: it spawns entities and adds mesh
//! and material assets, and returns a [`PlaytestScene`] holding every entity it
//! created. [`teardown_playtest_scene`] despawns exactly those entities and
//! releases the assets, so a spawn/teardown/spawn cycle leaves the live entity
//! count it started with (acceptance criterion 2) and a reload cannot find a
//! stale aircraft from a previous generation, because the scene owns its
//! entities and nothing else does.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bevy::app::{App, PluginGroup};
use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::camera::{ClearColorConfig, PerspectiveProjection, Projection};
use bevy::image::{Image, ImageSampler};
use bevy::mesh::Mesh;
use bevy::prelude::{
    Assets, Camera, Camera3d, Color, DefaultPlugins, DirectionalLight, Entity, Handle, Mesh3d,
    MeshMaterial3d, On, Res, Resource, StandardMaterial, Transform, Visibility, WindowPlugin,
    default,
};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::time::TimeUpdateStrategy;
use cs_assets::install::{self, DiscoveryError};
use cs_content::coordinates::SourceAdapter;
use cs_content::mesh::RenderMeshError;
use cs_content::scene::{
    BindingMap, GameZSceneError, MeshSlot, ParsedNode, SceneError, SceneGraph, SceneNodeId,
    scene_graph_from_gamez,
};
use cs_content::world::{
    Aabb, ObjectInstanceError, SurfaceRole, WORLD_SURFACE_UNMEASURED, WorldCollisionRole,
    WorldCollisionShape, WorldDefinition, WorldError, WorldId, WorldIdError, WorldObjectId,
    WorldObjectInstance,
};
use cs_formats::gamez::{GameZMeshes, GameZNodes, read_gamez_meshes, read_gamez_nodes};
use cs_formats::io::ParseContext;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{
    ContentId, ContentIdError, ContentKind, Known, Origin, Provenance, Resolved,
};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};

use crate::world::WorldMeshes;
use crate::world::fixture::MESH_SETTLE_UPDATES;
use crate::world::retail::{container_mesh_key, stored_presentation_unknowns, stored_render_mesh};
use crate::world::spawn::{
    SkipReason, SpawnedWorld, WorldMeshAssets, WorldSpawnError, spawn_object,
};

// ------------------------------------------------------------------- label --

/// The label every surface of this stage carries, verbatim.
///
/// `ORIGINAL ASSETS` — the geometry and the aircraft are the owner's original
/// bytes, read through the production readers. `DEVELOPMENT FREE FLIGHT` — the
/// product is a flyable scene, not a mission. `PROVISIONAL TUNING` — the scale,
/// handedness, spawn, camera, lighting, collision classification and material
/// are designed development values.
pub const PLAYTEST_LABEL: &str = "ORIGINAL ASSETS / DEVELOPMENT FREE FLIGHT / PROVISIONAL TUNING";

// ------------------------------------------------------------------ claims --

/// The area selection is a designed reading of the container, not a measurement.
pub const PLAYTEST_AREA_IS_DESIGNED: &str = "playtest-retail.area-selection-is-designed";

/// The unit and axis reading the scene renders stored geometry under.
pub const PLAYTEST_UNIT_IS_DESIGNED: &str = "playtest-retail.stored-unit-is-one-metre";

/// The collision classification the area's records are given.
pub const PLAYTEST_COLLISION_IS_THE_DRAWN_MESH: &str =
    "playtest-retail.collision-is-derived-from-the-drawn-mesh";

/// The aircraft's pose and its nose-axis mapping.
pub const PLAYTEST_AIRCRAFT_POSE_IS_DESIGNED: &str = "playtest-retail.aircraft-pose-is-designed";

/// The camera views a capture drives.
pub const PLAYTEST_VIEWS_ARE_DESIGNED: &str = "playtest-retail.camera-views-are-designed";

/// The neutral development material every drawn surface uses.
pub const PLAYTEST_NEUTRAL_MATERIAL: &str = "playtest-retail.neutral-development-material";

// -------------------------------------------------------- the pinned choices --

/// The world group this scene renders: `ZBD/C1C/gamez.zbd`.
pub const PLAYTEST_WORLD_GROUP: &str = "C1C";

/// The stored node slot of the one area this scene draws.
///
/// A record of the world node's own stored child list, chosen because it is the
/// container's densest piece of *visible* content: measured, its subtree holds
/// 793 nodes, 401 mesh bindings and 8 673 stored triangles, against the 1 786
/// triangles the whole partition-grid import draws.
pub const PLAYTEST_AREA_NODE_SLOT: u32 = 517;

/// The authored name the pinned area node stores, asserted on every spawn.
pub const PLAYTEST_AREA_NODE_NAME: &str = "piratezep";

/// The airframe root in `ZBD/planes.zbd` this scene's aircraft belongs to.
///
/// Named, not indexed into: F11's identity rule is that an airframe definition
/// references a **root in PLANES.ZBD**, so the root is found by its authored name
/// through [`SceneGraph::root`] and only the chosen mesh is addressed by slot.
pub const PLAYTEST_AIRCRAFT_ROOT_NAME: &str = "bloodhawk";

/// The stored node slot of the **one** aircraft mesh this scene draws.
pub const PLAYTEST_AIRCRAFT_MESH_NODE_SLOT: u32 = 2525;

/// The authored name the pinned aircraft mesh node stores.
pub const PLAYTEST_AIRCRAFT_MESH_NODE_NAME: &str = "fuse03";

/// The container key the aircraft is read from.
pub const AIRCRAFT_CONTAINER_KEY: &str = "zbd/planes.zbd";

// -------------------------------------------------------- the designed values --

/// Where the aircraft starts along the area's width, as a fraction of it from
/// its minimum: `−0.6`, so the start is 0.6 of the area's own width off its port
/// side — outside the geometry, with room to fly towards it and away from it.
pub const SPAWN_FRACTION_X: f64 = -0.6;
/// The start's height, as a fraction of the area's height from its minimum.
pub const SPAWN_FRACTION_Y: f64 = 0.55;
/// The start's position along the area's length, as a fraction from its minimum:
/// amidships.
pub const SPAWN_FRACTION_Z: f64 = 0.5;

/// The aircraft's nose mapping: a half turn about the vertical axis.
///
/// The stored airframe's nose is `+Z` — measured: the propeller disc lies in the
/// stored `x`/`y` plane and its node sits at `z = +4.80`, the fuselage mesh's own
/// maximum — and the runtime's forward axis is `−Z`, so the scene's aircraft
/// carries one half turn about `+Y`. The turn is a **designed** presentation
/// choice; the hint it follows is what the bytes say. See
/// [`PLAYTEST_AIRCRAFT_POSE_IS_DESIGNED`].
pub const AIRCRAFT_NOSE_AXIS: [f32; 3] = [0.0, 1.0, 0.0];

/// The capture frame's width, in pixels.
pub const CAPTURE_WIDTH: u32 = 640;
/// The capture frame's height, in pixels.
pub const CAPTURE_HEIGHT: u32 = 480;

/// How many views a capture drives. Three: two that frame the aircraft and one
/// that frames the area.
pub const VIEW_COUNT: usize = 3;

/// The camera's field of view, vertical, in degrees. Bevy's 3D default, named so
/// the framing distances below can be checked against it.
const CAMERA_FOV_DEGREES: f32 = 45.0;

/// The camera's near plane, as a fraction of its eye-to-target distance, and its
/// far plane as a multiple of the same. Derived rather than defaulted, because
/// this scene's views sit hundreds of metres from their subject and Bevy's
/// default far plane is 1 000 units.
const NEAR_PLANE_FRACTION: f32 = 0.01;
const FAR_PLANE_FACTOR: f32 = 8.0;

/// The clear colour the capture renders onto: an explicit development sky.
///
/// Not a claim about the original's sky, which is weather this stage does not
/// build. Chosen so a frame that drew nothing and a frame that drew geometry are
/// distinguishable by pixel count.
const CLEAR_COLOR: [f32; 4] = [0.36, 0.52, 0.72, 1.0];

/// The neutral development material's two colours: the world's and the
/// aircraft's, so a reader can tell them apart in a frame.
///
/// One flat colour per subject, declared here, applied to every drawn surface.
/// See [`PLAYTEST_NEUTRAL_MATERIAL`] for why this stage draws no stored texture.
const WORLD_MATERIAL_COLOR: [f32; 4] = [0.74, 0.70, 0.62, 1.0];
const AIRCRAFT_MATERIAL_COLOR: [f32; 4] = [0.86, 0.34, 0.30, 1.0];

/// The key light's illuminance in lux, and the fill's fraction of it.
///
/// Declared: a capture is a **render** witness, so the light exists to make a
/// surface visible and its value is not a claim about the original's lighting.
///
/// The value is **derived, not guessed**: Bevy's Lambert term is `albedo · E / π`,
/// so an illuminance near `π` renders a surface at approximately its own albedo —
/// which is the point of this stage, because the aircraft's declared colour is how
/// a reader tells the aircraft from the area in a frame. Measured: at 40 000 lux
/// every albedo above ~0.08 clipped to white, and both the area and the aircraft
/// came back indistinguishable (`playtest-retail-c1c-chase.png`, first capture).
const KEY_LIGHT_ILLUMINANCE: f32 = 3.2;
const FILL_FRACTION: f32 = 0.45;

/// How many updates a capture drives before it asks for the screenshot, and the
/// bound on the whole readback.
///
/// The same reasoning and the same measured values as
/// [`crate::world::gpu_capture`]: a mesh asset needs one `RenderApp` pass to reach
/// the GPU, and the screenshot readback is asynchronous, so a capture needs
/// several updates after the request.
const WARMUP_UPDATES: u32 = 6;
const MAX_CAPTURE_UPDATES: u32 = 48;

/// The smallest share of a frame that counts as "the environment is in frame": a
/// permille of pixels that are neither sky nor nothing. Below it the view framed
/// sky rather than geometry, and the capture says so instead of leaving a file
/// that reads like a good picture.
const MIN_ENVIRONMENT_PERMILLE: u32 = 20;

/// The smallest number of pixels the aircraft itself must contribute. One pixel:
/// the aircraft is 10 m inside an 840 m area, so in the area-framing view it is
/// legitimately small. What a capture must prove is that it is **in frame at
/// all**, and that is the difference between two renders of the same scene, not a
/// guess about its size.
const MIN_AIRCRAFT_PIXELS: usize = 1;

// ------------------------------------------------------------------ errors --

/// Why the retail playtest scene could not be built or captured.
#[derive(Debug)]
pub enum PlaytestError {
    /// The installation could not be inventoried.
    Discovery(DiscoveryError),
    /// Production discovery inventoried no such container.
    Absent {
        /// The container's logical key.
        container: String,
    },
    /// The container could not be read from disk.
    Read {
        /// The container's logical key.
        container: String,
        /// The operating system's message.
        reason: String,
    },
    /// The node array did not decode.
    Nodes {
        /// The container's logical key.
        container: String,
        /// The reader's own message.
        reason: String,
    },
    /// The mesh array did not decode.
    Meshes {
        /// The container's logical key.
        container: String,
        /// The reader's own message.
        reason: String,
    },
    /// A mesh-array slot or an object id was refused by the key grammar.
    Id {
        /// What was being named.
        what: String,
        /// The refusal itself.
        reason: String,
    },
    /// A node did not convert into a scene record.
    Scene {
        /// The container's logical key.
        container: String,
        /// The scene layer's own refusal.
        reason: GameZSceneError,
    },
    /// The scene graph refused the hierarchy.
    Graph {
        /// The container's logical key.
        container: String,
        /// The graph's own refusal.
        reason: SceneError,
    },
    /// The documented area node is not in the container, or is not the record
    /// its pinned name says it is.
    AreaNode {
        /// The stored slot the scene asked for.
        slot: u32,
        /// What the container holds at that slot, when it holds anything.
        found: Option<String>,
    },
    /// The documented aircraft root or mesh node is not in the container.
    AircraftNode {
        /// Which node was missing.
        what: &'static str,
        /// The slot or name the scene asked for.
        asked: String,
    },
    /// The container's mesh-slot table could not be reconciled.
    MeshSlot(GameZSceneError),
    /// A stored mesh did not become a render mesh.
    RenderMesh {
        /// The mesh-array slot.
        index: u32,
        /// The content layer's own refusal.
        reason: RenderMeshError,
    },
    /// A world id, an object id or the definition itself was refused.
    World {
        /// The refusal itself.
        reason: String,
    },
    /// One object record was refused.
    Instance {
        /// The object's id.
        object: String,
        /// The refusal itself.
        reason: String,
    },
    /// The world definition could not be assembled.
    Definition(WorldError),
    /// A spawn was refused outright. Per-object refusals are **not** this: they
    /// are reported on [`PlaytestAreaReport::refused`], so one unplaceable record
    /// cannot take the area down.
    Spawn(WorldSpawnError),
    /// A value this scene computes is not finite, so a pose, a bound or a camera
    /// would be undefined.
    NonFinite {
        /// What was computed.
        what: &'static str,
    },
    /// The documented area holds nothing to draw.
    EmptyArea {
        /// The subtree's node count.
        nodes: usize,
        /// How many of them bind a mesh.
        mesh_records: usize,
    },
    /// The area's geometry has no extent on any axis, so there is no scene to
    /// frame.
    DegenerateArea {
        /// The composed extent that came out.
        extent: [f64; 3],
    },
    /// A capture could not be produced.
    Capture(CaptureError),
}

impl fmt::Display for PlaytestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(error) => write!(f, "{error}"),
            Self::Absent { container } => {
                write!(f, "production discovery inventoried no {container}")
            }
            Self::Read { container, reason } => {
                write!(f, "could not read {container}: {reason}")
            }
            Self::Nodes { container, reason } => {
                write!(
                    f,
                    "could not decode the node array of {container}: {reason}"
                )
            }
            Self::Meshes { container, reason } => {
                write!(
                    f,
                    "could not decode the mesh array of {container}: {reason}"
                )
            }
            Self::Id { what, reason } => write!(f, "{what} is not a usable id: {reason}"),
            Self::Scene { container, reason } => {
                write!(f, "could not convert the nodes of {container}: {reason}")
            }
            Self::Graph { container, reason } => {
                write!(f, "the scene graph of {container} is refused: {reason}")
            }
            Self::AreaNode { slot, found } => match found {
                Some(name) => write!(
                    f,
                    "the documented area is node slot {slot}, but that slot holds {name:?}"
                ),
                None => write!(
                    f,
                    "the documented area is node slot {slot}, which is absent"
                ),
            },
            Self::AircraftNode { what, asked } => {
                write!(
                    f,
                    "the documented {what} is {asked}, which is not in the container"
                )
            }
            Self::MeshSlot(error) => write!(f, "the mesh-slot table was refused: {error}"),
            Self::RenderMesh { index, reason } => {
                write!(f, "mesh slot {index} is not a usable render mesh: {reason}")
            }
            Self::World { reason } => write!(f, "{reason}"),
            Self::Instance { object, reason } => {
                write!(f, "the object record {object} was refused: {reason}")
            }
            Self::Definition(error) => write!(f, "the playtest world definition: {error}"),
            Self::Spawn(error) => write!(f, "{error}"),
            Self::NonFinite { what } => {
                write!(f, "{what} is not finite, so the scene would be undefined")
            }
            Self::EmptyArea {
                nodes,
                mesh_records,
            } => write!(
                f,
                "the documented area holds {nodes} node(s) and {mesh_records} mesh record(s), \
                 so there is nothing to render"
            ),
            Self::DegenerateArea { extent } => write!(
                f,
                "the area's composed extent is {extent:?}, so every corner is one point and \
                 there is no scene to frame"
            ),
            Self::Capture(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for PlaytestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Discovery(error) => Some(error),
            Self::Scene { reason, .. } => Some(reason),
            Self::Graph { reason, .. } => Some(reason),
            Self::MeshSlot(error) => Some(error),
            Self::RenderMesh { reason, .. } => Some(reason),
            Self::Definition(error) => Some(error),
            Self::Spawn(error) => Some(error),
            Self::Capture(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DiscoveryError> for PlaytestError {
    fn from(error: DiscoveryError) -> Self {
        Self::Discovery(error)
    }
}

impl From<WorldIdError> for PlaytestError {
    fn from(error: WorldIdError) -> Self {
        Self::World {
            reason: error.to_string(),
        }
    }
}

impl From<ContentIdError> for PlaytestError {
    fn from(error: ContentIdError) -> Self {
        Self::Id {
            what: "a content id".to_owned(),
            reason: error.to_string(),
        }
    }
}

impl From<WorldError> for PlaytestError {
    fn from(error: WorldError) -> Self {
        Self::Definition(error)
    }
}

impl From<ObjectInstanceError> for PlaytestError {
    fn from(error: ObjectInstanceError) -> Self {
        Self::Instance {
            object: "an area record".to_owned(),
            reason: error.to_string(),
        }
    }
}

// ------------------------------------------------------------ the provenance --

/// The provenance every value this stage resolves from original bytes carries.
///
/// [`ClaimStatus::ObservedTool`], never `VerifiedOriginal`: the facts were read
/// out of the owner's container bytes by the production readers, which is file
/// access, not an original run. `span` is `None` only where no container exists
/// to point at, and the class is then `Unknown` rather than asserted.
fn observed(span: Option<SourceSpan>) -> Result<Provenance, PlaytestError> {
    Provenance::new(
        claim(PLAYTEST_AREA_IS_DESIGNED)?,
        if span.is_some() {
            ClaimStatus::ObservedTool
        } else {
            ClaimStatus::Unknown
        },
        span,
    )
    .map_err(|error| PlaytestError::World {
        reason: error.to_string(),
    })
}

/// The provenance a **designed** development value carries: explicit design with
/// no observed source.
fn designed(which: &str) -> Result<Provenance, PlaytestError> {
    Ok(Provenance::designed(claim(which)?))
}

/// One of this stage's claim ids, validated.
fn claim(id: &str) -> Result<ClaimId, PlaytestError> {
    ClaimId::new(id).map_err(|error| PlaytestError::World {
        reason: format!("the claim id {id:?} is invalid: {error}"),
    })
}

// --------------------------------------------------------------- the sources --

/// One original container, read and held open for the scene builder.
///
/// The node array and the mesh array are decoded **once**, here, and the bytes
/// are dropped again: everything the scene needs afterwards is the decoded
/// arrays, the mesh-slot table and the span, so keeping half a megabyte of
/// original bytes alive for the scene's lifetime would buy nothing.
#[derive(Clone, Debug)]
pub struct PlaytestContainer {
    group: String,
    container_key: String,
    container_sha256: String,
    span: SourceSpan,
    nodes: GameZNodes,
    meshes: GameZMeshes,
    table: Vec<MeshSlot>,
}

impl PlaytestContainer {
    /// The directory name the store spells this container's group with.
    #[must_use]
    pub fn group(&self) -> &str {
        &self.group
    }

    /// The container's logical key inside the installation.
    #[must_use]
    pub fn container_key(&self) -> &str {
        &self.container_key
    }

    /// SHA-256 of the whole container file, from production discovery, so a rerun
    /// over a different installation reports a different digest.
    #[must_use]
    pub fn container_sha256(&self) -> &str {
        &self.container_sha256
    }

    /// Where this container's bytes live, so a reader can go back to them.
    #[must_use]
    pub const fn span(&self) -> &SourceSpan {
        &self.span
    }

    /// The decoded node array.
    #[must_use]
    pub const fn nodes(&self) -> &GameZNodes {
        &self.nodes
    }

    /// The decoded mesh array.
    #[must_use]
    pub const fn meshes(&self) -> &GameZMeshes {
        &self.meshes
    }

    /// The container's mesh-slot table: one [`MeshSlot`] per stored mesh-array
    /// position, named the way the world-import path names it
    /// (`<group>.mesh-<index>`).
    ///
    /// **The one naming rule,
    /// [`crate::world::retail::container_mesh_key`]**, not a copy of it: a mesh the
    /// area draws and a mesh the world import draws are one catalog element
    /// because both spell it through that function. It is still a
    /// **per-container** name rather than an element of the shared render-mesh
    /// catalog, which is task #638's job; that seam is named there rather than
    /// papered over here.
    #[must_use]
    pub fn table(&self) -> &[MeshSlot] {
        &self.table
    }

    /// The mesh-array index a mesh reference names, or `None` when the reference
    /// is not one of this container's own slot names.
    ///
    /// The inverse of [`container_mesh_key`], so it reads the same
    /// `<group>.mesh-` prefix that function writes rather than a second spelling
    /// of it.
    #[must_use]
    pub fn mesh_index_of(&self, mesh: &ContentId) -> Option<u32> {
        let prefix = format!("{}.mesh-", self.group.to_ascii_lowercase());
        mesh.key()
            .strip_prefix(&prefix)
            .and_then(|suffix| suffix.parse::<u32>().ok())
    }

    /// The catalog name of one stored mesh-array slot, from the crate's one rule.
    fn mesh_key(&self, index: usize) -> Result<ContentId, PlaytestError> {
        container_mesh_key(&self.group, index).map_err(|error| PlaytestError::Id {
            what: format!("mesh slot {index} of {}", self.container_key),
            reason: error.to_string(),
        })
    }

    /// The provenance every value resolved from this container carries.
    fn provenance(&self) -> Result<Provenance, PlaytestError> {
        observed(Some(self.span.clone()))
    }
}

/// Everything the scene is read from: one world container, one aircraft
/// container, and the installation fingerprint both came from.
#[derive(Clone, Debug)]
pub struct PlaytestSources {
    installation: String,
    world: PlaytestContainer,
    aircraft: PlaytestContainer,
}

impl PlaytestSources {
    /// The installation fingerprint production discovery measured.
    #[must_use]
    pub fn installation(&self) -> &str {
        &self.installation
    }

    /// The world container the area is read from.
    #[must_use]
    pub const fn world(&self) -> &PlaytestContainer {
        &self.world
    }

    /// The container the aircraft is read from.
    #[must_use]
    pub const fn aircraft(&self) -> &PlaytestContainer {
        &self.aircraft
    }
}

/// Reads the two containers this scene renders, out of an installation.
///
/// One **production discovery pass** ([`install::discover`]) locates both files
/// and fingerprints the installation; both are then read through the production
/// GameZ readers ([`read_gamez_nodes`], [`read_gamez_meshes`]) and given a
/// mesh-slot table over every stored position. Nothing is written inside the
/// installation and nothing derived from it is committed.
///
/// # Errors
///
/// [`PlaytestError::Discovery`] when the installation cannot be inventoried —
/// **including when `install_root` is not an installation at all**, which is how
/// a run with no `CS_GAME_DIR` (or with the wrong path) fails by name rather
/// than producing an empty scene. [`PlaytestError::Absent`] when discovery holds
/// no such container, [`PlaytestError::Read`] when a file cannot be read, and
/// [`PlaytestError::Nodes`] / [`PlaytestError::Meshes`] when a section does not
/// decode. A refusal aborts: this module never imports a container the readers
/// could only partly read.
pub fn read_playtest_sources(
    install_root: &Path,
    world_group: &str,
) -> Result<PlaytestSources, PlaytestError> {
    let found = install::discover(install_root)?;
    let group = world_group.to_ascii_lowercase();
    let world = read_container(&found, world_group, &format!("zbd/{group}/gamez.zbd"))?;
    let aircraft = read_container(&found, "planes", AIRCRAFT_CONTAINER_KEY)?;
    Ok(PlaytestSources {
        installation: install::fingerprint(&found.manifest).to_string(),
        world,
        aircraft,
    })
}

/// Reads one container out of an already-inventoried installation.
///
/// The path comes from the manifest's own `relative_spelling`, not a re-joined
/// logical key, because a case-sensitive filesystem would refuse the join; and
/// the span carries the manifest's own digest, so a value resolved from these
/// bytes names the digest discovery measured for them.
fn read_container(
    found: &install::Discovery,
    group: &str,
    container_key: &str,
) -> Result<PlaytestContainer, PlaytestError> {
    let Some(record) = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == container_key)
    else {
        return Err(PlaytestError::Absent {
            container: container_key.to_owned(),
        });
    };
    let container_sha256 = record.sha256.to_hex();
    let bytes = fs::read(
        found
            .manifest
            .host_root
            .join(record.relative_spelling.as_str()),
    )
    .map_err(|error| PlaytestError::Read {
        container: container_key.to_owned(),
        reason: error.to_string(),
    })?;

    let mut parse = ParseContext::with_defaults(container_key.to_owned());
    let nodes = read_gamez_nodes(&mut parse, &bytes).map_err(|error| PlaytestError::Nodes {
        container: container_key.to_owned(),
        reason: error.to_string(),
    })?;
    let mut mesh_parse = ParseContext::with_defaults(container_key.to_owned());
    let meshes = read_gamez_meshes(&mut mesh_parse, container_key, &bytes).map_err(|error| {
        PlaytestError::Meshes {
            container: container_key.to_owned(),
            reason: error.to_string(),
        }
    })?;

    let span = SourceSpan::new(
        install::fingerprint(&found.manifest),
        record.relative_spelling.as_str(),
        None,
        0,
        u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        Some(record.sha256),
    )
    .map_err(|error| PlaytestError::World {
        reason: format!("the container's source span is not recordable: {error}"),
    })?;
    let provenance = observed(Some(span.clone()))?;

    let mut container = PlaytestContainer {
        group: group.to_owned(),
        container_key: container_key.to_owned(),
        container_sha256,
        span,
        nodes,
        meshes,
        table: Vec::new(),
    };
    let mut table = Vec::with_capacity(container.meshes.meshes.len());
    for index in 0..container.meshes.meshes.len() {
        let id = container.mesh_key(index)?;
        table.push(MeshSlot::new(id, provenance.clone()).map_err(PlaytestError::MeshSlot)?);
    }
    container.table = table;
    Ok(container)
}

// ----------------------------------------------------------- the scene graph --

/// The authored node subtree of one stored slot, in stored order.
///
/// Read from the container's own parent/child slots through the production node
/// reader, so "descendant" means what the container's links say and nothing else.
fn authored_subtree(nodes: &GameZNodes, root: u32) -> Vec<u32> {
    let mut members: Vec<u32> = Vec::new();
    let mut stack = vec![root];
    while let Some(slot) = stack.pop() {
        if members.contains(&slot) {
            continue;
        }
        members.push(slot);
        if let Some(node) = nodes.get(slot) {
            for child in node.children.iter().rev() {
                stack.push(*child);
            }
        }
    }
    members.sort_unstable();
    members
}

/// The stored slot of the container's one world record.
fn world_node_slot(nodes: &GameZNodes) -> Option<u32> {
    nodes
        .nodes
        .iter()
        .find(|node| matches!(node.kind, cs_formats::gamez::NodeKind::World(_)))
        .map(|node| node.index)
}

/// The scene graph of one documented area, and the graph's node for its root.
///
/// The container-wide hierarchy is **not** used: production discovery over
/// `ZBD/C1C/gamez.zbd` measures that the world record's child list and the
/// parent slot some of its descendants name disagree, and [`SceneGraph::build`]
/// refuses that as [`SceneError::InconsistentParentage`] (recorded in
/// `docs/findings/2026-10-04-m01-lc-world-import.md` as the `scene_node` id
/// blocker). This is the narrow adaptation that keeps **one** transform
/// conversion: the same validator, the same adapter and the same
/// [`AuthoredTransform`](cs_content::scene::AuthoredTransform) → canonical rule
/// run over a node set this module chose — and the composed transforms are the
/// container's own, because the only ancestor above the subtree root is the world
/// record, whose kind carries no transform.
///
/// # Errors
///
/// [`PlaytestError::Scene`] when a node does not convert, [`PlaytestError::Graph`]
/// when the validator refuses, and [`PlaytestError::AreaNode`] when the container
/// holds no world record or no node at the documented slot.
pub fn area_graph(
    container: &PlaytestContainer,
    root_slot: u32,
    adapter: &SourceAdapter,
) -> Result<(SceneGraph, SceneNodeId), PlaytestError> {
    let world_slot = world_node_slot(container.nodes()).ok_or(PlaytestError::AreaNode {
        slot: root_slot,
        found: None,
    })?;
    // The documented slot is checked **before** the graph is built, so a slot the
    // container does not hold is this stage's own `AreaNode` refusal rather than the
    // validator's dangling-child report about a link this module wrote itself.
    if container.nodes().get(root_slot).is_none() {
        return Err(PlaytestError::AreaNode {
            slot: root_slot,
            found: None,
        });
    }
    let keep = authored_subtree(container.nodes(), root_slot);
    if keep.is_empty() {
        return Err(PlaytestError::AreaNode {
            slot: root_slot,
            found: None,
        });
    }
    let mut parsed = parsed(container, &keep, world_slot)?;
    // Narrow the world record's child list to the one subtree this scene draws.
    // Everything else about the record is untouched: its kind, its stored
    // transform (the kind carries none) and its derived identity.
    for node in &mut parsed {
        if node.index == world_slot {
            node.children = vec![root_slot];
        }
    }
    let id = container_id(container)?;
    let graph = SceneGraph::build(&id, &parsed, adapter, &bindings()?).map_err(|reason| {
        PlaytestError::Graph {
            container: container.container_key.clone(),
            reason,
        }
    })?;
    let root_id = graph
        .nodes()
        .iter()
        .find(|node| node.index() == root_slot)
        .map(|node| node.id().clone())
        .ok_or(PlaytestError::AreaNode {
            slot: root_slot,
            found: None,
        })?;
    Ok((graph, root_id))
}

/// The scene graph of the **whole** aircraft container, so the airframe root is
/// found by its authored name the way F11's identity rule requires.
///
/// `ZBD/planes.zbd`'s hierarchy passes the production validator whole (measured:
/// 3 317 nodes converted), so no narrowing is needed here and none is done.
pub fn aircraft_graph(
    container: &PlaytestContainer,
    adapter: &SourceAdapter,
) -> Result<SceneGraph, PlaytestError> {
    let id = container_id(container)?;
    scene_graph_from_gamez(
        &id,
        container.nodes(),
        container.table(),
        adapter,
        &bindings()?,
    )
    .map_err(|reason| PlaytestError::Scene {
        container: container.container_key.clone(),
        reason,
    })
}

/// The parsed scene records of the slots `keep` names, plus the world record.
fn parsed(
    container: &PlaytestContainer,
    keep: &[u32],
    world_slot: u32,
) -> Result<Vec<ParsedNode>, PlaytestError> {
    let all = cs_content::scene::parsed_nodes_from_gamez(container.nodes(), container.table())
        .map_err(|reason| PlaytestError::Scene {
            container: container.container_key.clone(),
            reason,
        })?;
    Ok(all
        .into_iter()
        .filter(|node| node.index == world_slot || keep.contains(&node.index))
        .collect())
}

/// The container's own id in the `install_file` namespace, used as the scene
/// graph's container key so a node id spells the container it came from.
///
/// The key grammar allows no `/`, so the container is named by its group plus its
/// basename: `c1c-gamez`, `planes-gamez`. This is the identity of the *converted
/// tree*; the store's own path travels on the span.
fn container_id(container: &PlaytestContainer) -> Result<ContentId, PlaytestError> {
    let base = container
        .container_key
        .rsplit('/')
        .next()
        .unwrap_or(container.container_key.as_str())
        .trim_end_matches(".zbd");
    ContentId::from_source(
        ContentKind::InstallFile,
        &format!("{}-{base}", container.group.to_ascii_lowercase()),
    )
    .map_err(|error| PlaytestError::Id {
        what: format!("the container id of {}", container.container_key),
        reason: error.to_string(),
    })
}

/// The empty binding map: this stage declares no semantic bindings, because no
/// binding rule for a playtest area is evidenced. It is still the production type,
/// so a rule added later has somewhere to go.
fn bindings() -> Result<BindingMap, PlaytestError> {
    BindingMap::new(Vec::new()).map_err(|reason| PlaytestError::World {
        reason: reason.to_string(),
    })
}

// ---------------------------------------------------------------- the config --

/// Which area, which aircraft and which frame this scene uses.
///
/// Every field has a documented value in [`PlaytestConfig::documented`], and the
/// configuration is **not** a search: a caller that wants a different area says
/// which stored subtree it wants, and this module measures the result rather than
/// hunting for a good-looking one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaytestConfig {
    /// The world group whose container the area is read from.
    pub world_group: String,
    /// The stored node slot of the area's root.
    pub area_node_slot: u32,
    /// The authored name the area's root must store, checked on every spawn.
    pub area_node_name: String,
    /// The authored name of the airframe root in the aircraft container.
    pub aircraft_root_name: String,
    /// The stored node slot of the one aircraft mesh to draw.
    pub aircraft_mesh_node_slot: u32,
    /// The authored name that mesh node must store.
    pub aircraft_mesh_node_name: String,
    /// The capture frame's width, in pixels.
    pub capture_width: u32,
    /// The capture frame's height, in pixels.
    pub capture_height: u32,
}

impl PlaytestConfig {
    /// The configuration this task documents: `C1C`, node slot
    /// [`PLAYTEST_AREA_NODE_SLOT`], the `bloodhawk` airframe's node slot
    /// [`PLAYTEST_AIRCRAFT_MESH_NODE_SLOT`], and a [`CAPTURE_WIDTH`] ×
    /// [`CAPTURE_HEIGHT`] frame.
    #[must_use]
    pub fn documented() -> Self {
        Self {
            world_group: PLAYTEST_WORLD_GROUP.to_owned(),
            area_node_slot: PLAYTEST_AREA_NODE_SLOT,
            area_node_name: PLAYTEST_AREA_NODE_NAME.to_owned(),
            aircraft_root_name: PLAYTEST_AIRCRAFT_ROOT_NAME.to_owned(),
            aircraft_mesh_node_slot: PLAYTEST_AIRCRAFT_MESH_NODE_SLOT,
            aircraft_mesh_node_name: PLAYTEST_AIRCRAFT_MESH_NODE_NAME.to_owned(),
            capture_width: CAPTURE_WIDTH,
            capture_height: CAPTURE_HEIGHT,
        }
    }

    /// The capture frame this configuration asks for.
    #[must_use]
    pub const fn capture_size(&self) -> (u32, u32) {
        (self.capture_width, self.capture_height)
    }
}

impl Default for PlaytestConfig {
    fn default() -> Self {
        Self::documented()
    }
}

// --------------------------------------------------------------- the adapter --

/// The coordinate source this scene renders stored geometry under.
///
/// **The identity map**: stored `x`/`y`/`z` are canonical metres, right-handed,
/// `+Y` up, radians. That is a **designed** reading and not a measurement — the
/// original's world-vertex unit and handedness are unmeasured (task #436,
/// blocked) — and it is the reading the rest of this workspace's world path
/// already spawns retail geometry under, so the area and the aircraft are in one
/// frame instead of two. [`PLAYTEST_UNIT_IS_DESIGNED`] records it.
///
/// This is the workspace's **declared canonical source**, taken from
/// [`SourceAdapter::declared`]: no second conversion rule is introduced here.
pub fn playtest_adapter() -> Result<SourceAdapter, PlaytestError> {
    SourceAdapter::declared()
        .into_iter()
        .find(|adapter| adapter.source().label() == "canonical")
        .ok_or(PlaytestError::World {
            reason: "the declared coordinate sources hold no canonical self-map".to_owned(),
        })
}

// --------------------------------------------------------------- the reports --

/// What the scene read out of one area, before and after the spawn.
#[derive(Clone, Debug)]
pub struct PlaytestAreaReport {
    /// The world the area belongs to.
    pub world: WorldId,
    /// The stored slot of the area's root.
    pub node_slot: u32,
    /// The authored name that root stores.
    pub node_name: String,
    /// How many nodes the subtree holds.
    pub nodes: usize,
    /// How many of them bind a mesh the container stores geometry for.
    pub mesh_records: usize,
    /// How many stored triangles those records draw.
    pub triangles: usize,
    /// The composed extent of that geometry, in canonical metres.
    pub bounds: Aabb,
    /// Records the spawn refused outright, with the reason. An area is not taken
    /// down by one unplaceable record, so a refusal is **reported** here rather
    /// than returned as an error.
    pub refused: Vec<(WorldObjectId, String)>,
    /// Records that were presented but given no collider, with the reason.
    pub gaps: Vec<(WorldObjectId, SkipReason)>,
}

impl PlaytestAreaReport {
    /// How many records were presented **and** collided.
    #[must_use]
    pub fn colliders(&self) -> usize {
        self.mesh_records - self.refused.len() - self.gaps.len()
    }
}

/// One original aircraft mesh, as the scene reads it.
#[derive(Clone, Debug)]
pub struct PlaytestAircraftReport {
    /// The container the mesh came from.
    pub container_key: String,
    /// The stored slot of the mesh node, inside the airframe root's subtree.
    pub node_slot: u32,
    /// The authored name that node stores.
    pub node_name: String,
    /// The airframe root this mesh belongs to.
    pub root_name: String,
    /// The mesh-array slot the node binds.
    pub mesh_index: u32,
    /// The catalog element the mesh is registered under.
    pub mesh: ContentId,
    /// How many stored triangles the mesh draws.
    pub triangles: usize,
    /// How many material groups it stores.
    pub groups: usize,
    /// The mesh's stored extent, in canonical metres.
    pub extent_m: [f64; 3],
    /// The F17-B fingerprint of the upload this scene drew.
    pub fingerprint: ContentHash,
    /// The composed transform the mesh node carries in its airframe: measured to
    /// be the identity for the pinned mesh, so the draw places it at the spawn
    /// pose unchanged.
    pub composed_translation_m: [f64; 3],
}

/// One camera view, in canonical metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaytestCameraView {
    /// The view's stable label, used for the artifact's file name.
    pub name: &'static str,
    /// The eye position.
    pub eye: [f32; 3],
    /// What the camera looks at.
    pub target: [f32; 3],
}

// ------------------------------------------------------- the designed framing --

/// The three documented camera views of a spawn inside an area.
///
/// Derived from the **measured** area bounds and the **designed** spawn, so a
/// capture frames geometry that exists rather than a hard-coded point that may
/// not. `chase` and `quarter` look at the spawn, so the **aircraft** is in frame;
/// `overview` looks at the area's centre from outside it, so the **environment**
/// fills the frame with the aircraft small but present.
///
/// Every camera distance is declared as a multiple of the **aircraft's** own
/// extent, so the two aircraft-framing views hold the aircraft at a constant
/// share of the frame instead of at a constant number of metres this file guessed
/// for one installation. The function is total over finite inputs and refuses a
/// non-finite bound, spawn or extent by name, which is what makes the framing
/// checkable without an installation.
///
/// # Errors
///
/// [`PlaytestError::NonFinite`] naming the component that was not finite.
pub fn camera_poses(
    bounds: &Aabb,
    spawn: [f32; 3],
    aircraft_extent_m: [f32; 3],
) -> Result<Vec<PlaytestCameraView>, PlaytestError> {
    let finite = |what: &'static str, value: f64| -> Result<f32, PlaytestError> {
        if value.is_finite() {
            Ok(value as f32)
        } else {
            Err(PlaytestError::NonFinite { what })
        }
    };
    for (axis, value) in spawn.into_iter().enumerate() {
        finite(
            match axis {
                0 => "the spawn's x",
                1 => "the spawn's y",
                _ => "the spawn's z",
            },
            f64::from(value),
        )?;
    }
    // The aircraft's own extent, checked **before** the one-metre floor below.
    // `f64::max` returns its other argument when one side is NaN, so flooring
    // first would silently swallow a non-finite extent and this function would
    // answer with views built from a metre-long "aircraft". The floor exists so a
    // degenerate mesh cannot divide a framing distance away, and it is applied to a
    // value already known to be finite.
    for (axis, value) in aircraft_extent_m.into_iter().enumerate() {
        finite(
            match axis {
                0 => "the aircraft's width",
                1 => "the aircraft's height",
                _ => "the aircraft's length",
            },
            f64::from(value),
        )?;
    }
    // The width is validated above and then not used by a view: an eye abeam of a
    // 10 m fuselage is placed along the **length**, and adding the 2 m width to it
    // would move the camera by less than the framing distance's own precision.
    let height = f64::from(aircraft_extent_m[1].max(1.0));
    let length = f64::from(aircraft_extent_m[2].max(1.0));
    let spawn_x = f64::from(spawn[0]);
    let spawn_y = f64::from(spawn[1]);
    let spawn_z = f64::from(spawn[2]);
    let centre = [
        (bounds.min()[0] + bounds.max()[0]) * 0.5,
        (bounds.min()[1] + bounds.max()[1]) * 0.5,
        (bounds.min()[2] + bounds.max()[2]) * 0.5,
    ];
    let span = [
        bounds.max()[0] - bounds.min()[0],
        bounds.max()[1] - bounds.min()[1],
        bounds.max()[2] - bounds.min()[2],
    ];
    // The two aircraft-framing views stand **outboard and abeam**, looking back
    // towards the area, and both placements are measured rather than aesthetic:
    //
    // * **abeam, not astern.** The pinned aircraft mesh is 10.23 m long and 2.11 m
    //   wide, so a camera directly behind it sees a 2 m cross-section. Measured, an
    //   astern view contributed 807 aircraft pixels; abeam, the fuselage's own
    //   length crosses the frame instead.
    // * **outboard, not inboard.** The aircraft starts 0.6 of the area's own width
    //   off the area's port side, so an inboard eye would put the **camera** between
    //   the aircraft and the area and frame only sky. Measured: an inboard chase
    //   view was refused as `NoEnvironment`, which is this capture's own check
    //   working. An outboard eye looks towards the spawn with the area behind it,
    //   so one frame carries the aircraft in front and the original geometry behind.
    let drafts: [(&'static str, [f64; 3], [f64; 3]); 3] = [
        (
            "chase",
            [
                spawn_x - 1.8 * length,
                spawn_y + 0.7 * height,
                spawn_z + 0.6 * length,
            ],
            [spawn_x, spawn_y, spawn_z],
        ),
        (
            "quarter",
            [
                spawn_x - 1.4 * length,
                spawn_y + 1.2 * height,
                spawn_z - 1.6 * length,
            ],
            [spawn_x, spawn_y, spawn_z],
        ),
        (
            "overview",
            [
                centre[0] - 0.42 * span[0],
                centre[1] + 0.36 * span[1] + 3.0 * height,
                centre[2] + 0.5 * span[2],
            ],
            centre,
        ),
    ];
    drafts
        .into_iter()
        .map(|(name, eye, target)| {
            Ok(PlaytestCameraView {
                name,
                eye: [
                    finite("a camera eye's x", eye[0])?,
                    finite("a camera eye's y", eye[1])?,
                    finite("a camera eye's z", eye[2])?,
                ],
                target: [
                    finite("a camera target's x", target[0])?,
                    finite("a camera target's y", target[1])?,
                    finite("a camera target's z", target[2])?,
                ],
            })
        })
        .collect()
}

/// The start pose of the aircraft: off the area's port side, nose on the
/// runtime's forward axis.
///
/// The position is a declared fraction ([`SPAWN_FRACTION_X`],
/// [`SPAWN_FRACTION_Y`], [`SPAWN_FRACTION_Z`]) of the area's own **measured**
/// extent, so a different area gets a different and still finite pose; the
/// rotation is the declared half turn about [`AIRCRAFT_NOSE_AXIS`] that maps the
/// measured stored nose (`+Z`) onto the runtime's forward axis (`−Z`). Both are
/// design, recorded under [`PLAYTEST_AIRCRAFT_POSE_IS_DESIGNED`].
///
/// # Errors
///
/// [`PlaytestError::NonFinite`] when the bounds are finite but the pose they
/// produce is not, and [`PlaytestError::DegenerateArea`] when the area has no
/// extent on any axis.
pub fn spawn_pose(bounds: &Aabb) -> Result<([f32; 3], bevy::math::Quat), PlaytestError> {
    let extent = [
        bounds.max()[0] - bounds.min()[0],
        bounds.max()[1] - bounds.min()[1],
        bounds.max()[2] - bounds.min()[2],
    ];
    if !extent.iter().any(|side| *side > 0.0) {
        return Err(PlaytestError::DegenerateArea { extent });
    }
    let pose = [
        (bounds.min()[0] + SPAWN_FRACTION_X * extent[0]) as f32,
        (bounds.min()[1] + SPAWN_FRACTION_Y * extent[1]) as f32,
        (bounds.min()[2] + SPAWN_FRACTION_Z * extent[2]) as f32,
    ];
    if pose.iter().any(|value| !value.is_finite()) {
        return Err(PlaytestError::NonFinite {
            what: "the spawn pose",
        });
    }
    let axis = bevy::math::Vec3::from(AIRCRAFT_NOSE_AXIS).normalize();
    Ok((
        pose,
        bevy::math::Quat::from_axis_angle(axis, std::f32::consts::PI),
    ))
}

// --------------------------------------------------------------- the material --

/// What every drawn surface is presented with.
///
/// **One** decision for the whole scene, reported once as the owner requires,
/// with the source ids it covers. The per-material texture binding (F10-C.02's
/// dependency audit through F17's image upload) is **not** built by this stage,
/// so no stored texture is drawn and no mesh's material identity is claimed;
/// [`PLAYTEST_NEUTRAL_MATERIAL`] records that, and the immediate next step is
/// PLAYTEST-RETAIL-HANDOFF's.
#[derive(Clone, Debug, PartialEq)]
pub struct PlaytestMaterial {
    /// The claim every drawn surface's presentation is filed under.
    pub claim: String,
    /// The world's neutral colour.
    pub world_color: [f32; 4],
    /// The aircraft's neutral colour.
    pub aircraft_color: [f32; 4],
    /// The container key and the number of meshes each decision covers.
    pub covered: Vec<(String, usize)>,
}

impl PlaytestMaterial {
    /// The declared decision, with the containers and mesh counts it covers.
    #[must_use]
    pub fn neutral(covered: Vec<(String, usize)>) -> Self {
        Self {
            claim: PLAYTEST_NEUTRAL_MATERIAL.to_owned(),
            world_color: WORLD_MATERIAL_COLOR,
            aircraft_color: AIRCRAFT_MATERIAL_COLOR,
            covered,
        }
    }

    /// Whether this decision is the neutral development material.
    #[must_use]
    pub fn is_neutral(&self) -> bool {
        self.claim == PLAYTEST_NEUTRAL_MATERIAL
    }
}

// ------------------------------------------------------------------ the scene --

/// One spawned playtest scene: the area's records, the aircraft, and everything
/// this stage owns in the Bevy world.
///
/// The scene is the **only** owner of the entities it reports. Nothing else in
/// `cs_app` keeps a handle to them, so [`teardown_playtest_scene`] can despawn
/// exactly what a spawn created, and a reload cannot find a stale aircraft from a
/// previous generation.
pub struct PlaytestScene {
    config: PlaytestConfig,
    definition: WorldDefinition,
    spawned: SpawnedWorld,
    aircraft_entity: Entity,
    aircraft_extent_m: [f64; 3],
    entities: Vec<Entity>,
    report: PlaytestAreaReport,
    aircraft: PlaytestAircraftReport,
    material: PlaytestMaterial,
    spawn: [f32; 3],
    rotation: bevy::math::Quat,
    views: Vec<PlaytestCameraView>,
    camera: Entity,
    lights: Vec<Entity>,
    target: CaptureTarget,
    adapter: SourceAdapter,
}

impl PlaytestScene {
    /// The configuration this scene was built from.
    #[must_use]
    pub const fn config(&self) -> &PlaytestConfig {
        &self.config
    }

    /// The world definition the area was assembled into: one record per mesh the
    /// area draws, each at the node's own composed transform.
    #[must_use]
    pub const fn definition(&self) -> &WorldDefinition {
        &self.definition
    }

    /// What the production spawn produced for the area.
    #[must_use]
    pub const fn spawned(&self) -> &SpawnedWorld {
        &self.spawned
    }

    /// What was read out of the area.
    #[must_use]
    pub const fn area(&self) -> &PlaytestAreaReport {
        &self.report
    }

    /// The one aircraft mesh this scene drew.
    #[must_use]
    pub const fn aircraft(&self) -> &PlaytestAircraftReport {
        &self.aircraft
    }

    /// The material decision, reported once.
    #[must_use]
    pub const fn material(&self) -> &PlaytestMaterial {
        &self.material
    }

    /// The aircraft's entity.
    #[must_use]
    pub const fn aircraft_entity(&self) -> Entity {
        self.aircraft_entity
    }

    /// The aircraft's start position, in canonical metres.
    #[must_use]
    pub const fn spawn(&self) -> [f32; 3] {
        self.spawn
    }

    /// The aircraft's start rotation: the declared nose mapping.
    #[must_use]
    pub const fn rotation(&self) -> bevy::math::Quat {
        self.rotation
    }

    /// The camera views a capture drives.
    #[must_use]
    pub fn views(&self) -> &[PlaytestCameraView] {
        &self.views
    }

    /// The camera entity this scene owns.
    #[must_use]
    pub const fn camera(&self) -> Entity {
        self.camera
    }

    /// Every entity this scene owns, the aircraft first. [`teardown_playtest_scene`]
    /// despawns exactly this list.
    #[must_use]
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// The coordinate source the scene was composed in.
    #[must_use]
    pub const fn adapter(&self) -> &SourceAdapter {
        &self.adapter
    }

    /// The aircraft mesh's stored extent in canonical metres — the length, width
    /// and height every declared camera distance is a multiple of.
    #[must_use]
    pub const fn aircraft_extent_m(&self) -> [f64; 3] {
        self.aircraft_extent_m
    }
}

/// What [`teardown_playtest_scene`] released.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaytestTeardown {
    /// How many entities were despawned.
    pub entities: usize,
    /// How many engine mesh assets the loader was **holding** for the world
    /// records when the teardown began — the size of the release, measured
    /// before it happened.
    ///
    /// Recorded separately from [`Self::world_mesh_assets`] because the count
    /// *after* the release is zero by construction, so on its own it proves
    /// nothing: a teardown that released a populated cache and one that released
    /// an empty one look the same. This is the number that tells them apart.
    pub released_world_mesh_assets: usize,
    /// How many engine mesh assets this scene had registered for the world
    /// records, after the release.
    pub world_mesh_assets: usize,
}

/// Builds the Bevy world this scene is spawned into: physics, the asset stack
/// Avian's mesh-derived colliders need, **and** the renderer.
///
/// [`crate::world::fixture::world_app`] is the headless composition and it has no
/// renderer, so a capture cannot be taken in it. Adding the render plugins to it
/// would mean two asset stacks — Bevy refuses the duplicate — so this module
/// states its own composition from the same parts instead: Bevy's own defaults
/// (which is where `AssetPlugin`, `MeshPlugin`, `ImagePlugin` and
/// `WorldSerializationPlugin` come from) with the window plugin disabled, plus
/// Avian's physics plugins and this crate's passes. The renderer needs no
/// display: every capture is rendered into an image asset and read back.
pub fn playtest_app() -> App {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .build()
            .disable::<bevy::winit::WinitPlugin>()
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                close_when_requested: false,
                ..WindowPlugin::default()
            }),
    );
    // `WorldSerializationPlugin` creates the `WorldInstanceSpawner` Avian's
    // `init_collider_constructor_hierarchies` gates itself on. Bevy's defaults
    // normally carry it through `ScenePlugin`; adding it again would be a
    // duplicate plugin, so it is added only when it is missing.
    if !app.is_plugin_added::<bevy::world_serialization::WorldSerializationPlugin>() {
        app.add_plugins(bevy::world_serialization::WorldSerializationPlugin);
    }
    app.add_plugins((
        avian3d::prelude::PhysicsPlugins::default(),
        crate::physics::RestingBodiesPlugin,
        crate::physics::PhysicsAdapterPlugin::new(crate::physics::BASELINE_FIXED_HZ),
    ));
    // The same three resources the headless world composition installs, and for
    // the same reasons: a manual frame duration so a capture's updates are
    // reproducible rather than wall-clock driven, one substep per fixed tick so
    // the mesh-derived colliders settle the way every other world in this crate
    // settles them, and **zero gravity** because a playtest scene is a static
    // world with one placed aircraft and no simulation of its own — nothing here
    // is a flight loop, so nothing here may fall.
    let frame = std::time::Duration::from_secs_f64(1.0 / crate::physics::BASELINE_FIXED_HZ as f64);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(frame));
    app.insert_resource(avian3d::prelude::SubstepCount(1));
    app.insert_resource(avian3d::prelude::Gravity::ZERO);
    // Finished and cleaned up **before** the caller spawns anything, exactly as
    // [`crate::world::fixture::world_app`] and
    // [`crate::world::gpu_capture::capture_world_mesh`] both do. Measured: leaving
    // the app in its building state and driving it panics a Bevy system on the
    // compute task pool with "Parameter failed validation: Resource does not
    // exist", because the plugins' `finish`/`cleanup` hooks that register the
    // render extract systems never ran.
    app.finish();
    app.cleanup();
    app
}

/// Reads one container's stored mesh as the production render mesh, through the
/// one builder the world-import path also uses.
fn render_of(
    container: &PlaytestContainer,
    index: u32,
) -> Result<cs_content::mesh::RenderMesh, PlaytestError> {
    let Some(slot) = container.meshes.get(index) else {
        return Err(PlaytestError::RenderMesh {
            index,
            reason: RenderMeshError::GroupCount {
                polygons: 0,
                groups: 0,
            },
        });
    };
    stored_render_mesh(slot).map_err(|error| PlaytestError::World {
        reason: format!("stored mesh {index} would not build: {error}"),
    })
}

/// Spawns the whole playtest scene into `app`.
///
/// Four steps, none optional and none able to half-apply:
///
/// 1. **compose.** The area's graph ([`area_graph`]) and the aircraft's graph
///    ([`aircraft_graph`]) are built through the production validator, and the
///    documented node and name are checked against what the containers hold — a
///    pinned slot holding a different record is a refusal, not a silent
///    substitution.
/// 2. **assemble.** Every mesh-bearing node of the area becomes a
///    [`WorldObjectInstance`]: identity `playtest.node-<slot>` (the stored slot, as
///    #629's own `node-<slot>` rule does, stated as identity and not as identity
///    *by name* — the container stores 34 records all named `g27816` in one
///    group), the mesh the node binds, the node's own composed transform, and the
///    **declared** `Solid` + `FromMesh` classification. The definition carries an
///    explicit unknown boundary, so no invisible wall is invented.
/// 3. **spawn.** The production [`spawn_object`] runs per record, so a
///    mesh-derived collider is derived from the very mesh the record draws and the
///    two cannot diverge (F18 non-negotiable behavior 1). A record the spawn
///    refuses is **reported**, not fatal. The declared neutral material goes on
///    every drawn node, because the world spawn presents geometry without one.
/// 4. **place.** The aircraft's one mesh is uploaded through the same F17-B
///    adapter, put on one entity at [`spawn_pose`], and the camera and two lights
///    are created.
///
/// # Errors
///
/// [`PlaytestError::AreaNode`] / [`PlaytestError::AircraftNode`] when a pinned node
/// is not what the container holds, [`PlaytestError::RenderMesh`] when a stored
/// mesh will not build, [`PlaytestError::EmptyArea`] when the area draws nothing,
/// [`PlaytestError::DegenerateArea`] / [`PlaytestError::NonFinite`] when the
/// geometry has no finite extent to frame, and the id/definition refusals.
pub fn spawn_playtest_scene(
    app: &mut App,
    sources: &PlaytestSources,
    config: &PlaytestConfig,
) -> Result<PlaytestScene, PlaytestError> {
    let adapter = playtest_adapter()?;
    let (graph, root) = area_graph(sources.world(), config.area_node_slot, &adapter)?;
    let root_name = graph
        .node(&root)
        .map(|node| node.name().to_owned())
        .unwrap_or_default();
    if root_name != config.area_node_name {
        return Err(PlaytestError::AreaNode {
            slot: config.area_node_slot,
            found: Some(root_name),
        });
    }
    let members = graph.subtree(&root);
    let node_count = members.len();

    // -- the aircraft, read before anything is spawned ----------------------
    let planes = aircraft_graph(sources.aircraft(), &adapter)?;
    let airframe =
        planes
            .root(&config.aircraft_root_name)
            .map_err(|_| PlaytestError::AircraftNode {
                what: "airframe root",
                asked: config.aircraft_root_name.clone(),
            })?;
    let airframe_members = planes.subtree(airframe.id());
    let aircraft_node = airframe_members
        .iter()
        .find(|node| node.index() == config.aircraft_mesh_node_slot)
        .ok_or(PlaytestError::AircraftNode {
            what: "aircraft mesh node",
            asked: format!(
                "slot {} of the {} airframe",
                config.aircraft_mesh_node_slot, config.aircraft_root_name
            ),
        })?;
    if aircraft_node.name() != config.aircraft_mesh_node_name {
        return Err(PlaytestError::AircraftNode {
            what: "aircraft mesh node",
            asked: format!(
                "slot {} holds {:?}, not {:?}",
                config.aircraft_mesh_node_slot,
                aircraft_node.name(),
                config.aircraft_mesh_node_name
            ),
        });
    }
    let aircraft_binding = aircraft_node.mesh().ok_or(PlaytestError::AircraftNode {
        what: "aircraft mesh binding",
        asked: format!("node {} binds no mesh", config.aircraft_mesh_node_slot),
    })?;
    let aircraft_mesh_id = match &aircraft_binding.mesh {
        Resolved::Known(known) => known.value.clone(),
        Resolved::Unknown { reason, .. } => {
            return Err(PlaytestError::AircraftNode {
                what: "aircraft mesh binding",
                asked: reason.clone(),
            });
        }
    };
    let aircraft_render = render_of(sources.aircraft(), aircraft_binding.index)?;
    let aircraft_extent = extent_of(&aircraft_render);
    let composed = aircraft_node.world_transform().translation();

    // -- the area's records --------------------------------------------------
    let provenance = sources.world().provenance()?;
    let collision: Resolved<WorldCollisionRole> = Resolved::Known(Known::new(
        WorldCollisionRole::Solid,
        designed(PLAYTEST_COLLISION_IS_THE_DRAWN_MESH)?,
    ));
    let shape: Resolved<WorldCollisionShape> = Resolved::Known(Known::new(
        WorldCollisionShape::FromMesh,
        designed(PLAYTEST_COLLISION_IS_THE_DRAWN_MESH)?,
    ));
    let surface: Resolved<SurfaceRole> = unknown(
        WORLD_SURFACE_UNMEASURED,
        "the container states no gameplay surface for this record, so no ground or water rule \
         is inherited by a development free-flight contact",
    )?;

    let mut meshes = WorldMeshes::new();
    let mut records: Vec<WorldObjectInstance> = Vec::new();
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    let mut triangles = 0usize;
    let mut mesh_records = 0usize;
    for node in &members {
        let Some(binding) = node.mesh() else {
            continue;
        };
        let Resolved::Known(known) = &binding.mesh else {
            continue;
        };
        let Some(index) = sources.world().mesh_index_of(&known.value) else {
            continue;
        };
        let Ok(render) = render_of(sources.world(), index) else {
            // A slot the container holds no usable geometry for is **not**
            // registered: the spawn then reports the record as `MeshUnavailable`
            // rather than this module handing it a substitute shape.
            continue;
        };
        let unknowns = stored_presentation_unknowns(&render);
        meshes
            .insert_render_mesh(known.value.clone(), &render, &unknowns)
            .map_err(|reason| PlaytestError::World {
                reason: format!("mesh slot {index} of the area would not upload: {reason}"),
            })?;
        grow(&mut min, &mut max, &render, node.world_transform())?;
        triangles += render.triangles().len();
        mesh_records += 1;

        let slot = node.index();
        records.push(
            WorldObjectInstance::resident(
                object_id(slot)?,
                Resolved::Known(known.clone()),
                *node.world_transform(),
                collision.clone(),
                shape.clone(),
                surface.clone(),
                provenance.clone(),
            )
            .map_err(|error| PlaytestError::Instance {
                object: format!("node {slot}"),
                reason: error.to_string(),
            })?,
        );
    }
    if mesh_records == 0 {
        return Err(PlaytestError::EmptyArea {
            nodes: node_count,
            mesh_records,
        });
    }
    let bounds = Aabb::try_new(min, max).map_err(|error| PlaytestError::World {
        reason: format!("the area's composed extent was refused: {error}"),
    })?;
    let extent = [
        bounds.max()[0] - bounds.min()[0],
        bounds.max()[1] - bounds.min()[1],
        bounds.max()[2] - bounds.min()[2],
    ];
    if !extent.iter().any(|side| *side > 0.0) {
        return Err(PlaytestError::DegenerateArea { extent });
    }
    let (spawn, rotation) = spawn_pose(&bounds)?;

    let world = WorldId::from_key(&config.world_group.to_ascii_lowercase())?;
    let boundary = unknown(
        WORLD_SURFACE_UNMEASURED,
        "the container states no floor, ceiling or lateral rule for this world, so a \
         development free flight is bounded by the area's own geometry rather than by an \
         invented wall",
    )?;
    let definition = WorldDefinition::try_new(
        world.clone(),
        Origin::Installation {
            source: sources.world().span().clone(),
        },
        boundary,
        Vec::new(),
        records,
        provenance.clone(),
    )?;

    // -- spawn ---------------------------------------------------------------
    let mut spawned = SpawnedWorld::of(&world);
    let mut refused: Vec<(WorldObjectId, String)> = Vec::new();
    let mut gaps: Vec<(WorldObjectId, SkipReason)> = Vec::new();
    let mut entities: Vec<Entity> = Vec::new();
    for object in definition.objects() {
        match spawn_object(app, &definition, object, &meshes) {
            Ok(instance) => {
                if let Some(reason) = instance.skipped {
                    gaps.push((instance.object.clone(), reason));
                }
                entities.extend(instance.entities());
                spawned.record(instance);
            }
            // A record the spawn refuses outright is reported, not fatal: one
            // unplaceable record must not take a 400-record area down.
            Err(error) => refused.push((object.id().clone(), error.to_string())),
        }
    }

    // -- the aircraft --------------------------------------------------------
    let aircraft_unknowns = stored_presentation_unknowns(&aircraft_render);
    meshes
        .insert_render_mesh(
            aircraft_mesh_id.clone(),
            &aircraft_render,
            &aircraft_unknowns,
        )
        .map_err(|reason| PlaytestError::World {
            reason: format!("the aircraft mesh would not upload: {reason}"),
        })?;
    let uploaded = meshes
        .get(&aircraft_mesh_id)
        .ok_or_else(|| PlaytestError::World {
            reason: format!(
                "the aircraft mesh {} is not registered after being uploaded",
                aircraft_mesh_id.key()
            ),
        })?;
    let aircraft_fingerprint = uploaded.fingerprint();
    let aircraft_triangles = uploaded.triangles();
    let aircraft_groups = uploaded.group_count();
    let handle: Handle<Mesh> = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(uploaded.mesh().clone());
    let world_material = add_material(app, WORLD_MATERIAL_COLOR);
    let aircraft_material = add_material(app, AIRCRAFT_MATERIAL_COLOR);
    for entity in &entities {
        if app.world().get::<Mesh3d>(*entity).is_some() {
            app.world_mut()
                .entity_mut(*entity)
                .insert(MeshMaterial3d(world_material.clone()));
        }
    }
    let aircraft_entity = app
        .world_mut()
        .spawn((
            Mesh3d(handle),
            MeshMaterial3d(aircraft_material),
            Transform::from_translation(bevy::math::Vec3::from(spawn)).with_rotation(rotation),
        ))
        .id();
    entities.push(aircraft_entity);

    // -- the camera and the lights ------------------------------------------
    let views = camera_poses(
        &bounds,
        spawn,
        [
            aircraft_extent[0] as f32,
            aircraft_extent[1] as f32,
            aircraft_extent[2] as f32,
        ],
    )?;
    let (width, height) = config.capture_size();
    let (camera, lights, target) = add_camera_and_lights(app, &views[0], width, height);
    entities.push(camera);
    entities.extend(lights.iter().copied());

    let material = PlaytestMaterial::neutral(vec![
        (
            sources.world().container_key().to_owned(),
            meshes.len().saturating_sub(1),
        ),
        (sources.aircraft().container_key().to_owned(), 1),
    ]);
    let report = PlaytestAreaReport {
        world: world.clone(),
        node_slot: config.area_node_slot,
        node_name: root_name,
        nodes: node_count,
        mesh_records,
        triangles,
        bounds,
        refused,
        gaps,
    };
    let aircraft = PlaytestAircraftReport {
        container_key: sources.aircraft().container_key().to_owned(),
        node_slot: aircraft_node.index(),
        node_name: aircraft_node.name().to_owned(),
        root_name: config.aircraft_root_name.clone(),
        mesh_index: aircraft_binding.index,
        mesh: aircraft_mesh_id,
        triangles: aircraft_triangles,
        groups: aircraft_groups,
        extent_m: aircraft_extent,
        fingerprint: aircraft_fingerprint,
        composed_translation_m: composed,
    };
    Ok(PlaytestScene {
        config: config.clone(),
        definition,
        spawned,
        aircraft_entity,
        aircraft_extent_m: aircraft_extent,
        entities,
        report,
        aircraft,
        material,
        spawn,
        rotation,
        views,
        camera,
        lights,
        target,
        adapter,
    })
}

/// The identity of one area record: `playtest.node-<stored slot>`.
///
/// The **stored slot**, for #629's own reason: a container stores many records
/// under one authored name (`c1c`'s world node lists thirty-four records all
/// named `g27816`), so the slot is the one value the key grammar always accepts
/// and the one the container addresses a record by. It is identity *by slot*,
/// stated as such, never identity by name.
fn object_id(slot: u32) -> Result<WorldObjectId, PlaytestError> {
    WorldObjectId::new(&format!("playtest.node-{slot}")).map_err(|error| PlaytestError::Instance {
        object: format!("node {slot}"),
        reason: error.to_string(),
    })
}

/// One explicit unknown, named by its claim and its reason.
fn unknown<T>(which: &str, reason: &str) -> Result<Resolved<T>, PlaytestError> {
    Resolved::unknown(claim(which)?, reason).map_err(|error| PlaytestError::Instance {
        object: which.to_owned(),
        reason: error.to_string(),
    })
}

/// One node's stored mesh extent, in canonical metres.
fn extent_of(render: &cs_content::mesh::RenderMesh) -> [f64; 3] {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for vertex in render.vertices() {
        for axis in 0..3 {
            let value = f64::from(vertex.position[axis]);
            if value.is_finite() {
                min[axis] = min[axis].min(value);
                max[axis] = max[axis].max(value);
            }
        }
    }
    let mut extent = [0.0; 3];
    for axis in 0..3 {
        extent[axis] = if min[axis].is_finite() && max[axis].is_finite() {
            (max[axis] - min[axis]).max(0.0)
        } else {
            0.0
        };
    }
    extent
}

/// Grows a composed world bound by one mesh's own corners through one node's
/// composed transform.
///
/// Every **corner** of the mesh's stored axis-aligned box is transformed, not
/// just two of them: a rotated node's box is not its own box after the rotation,
/// and taking only the two ends would under-state the geometry the scene frames.
fn grow(
    min: &mut [f64; 3],
    max: &mut [f64; 3],
    render: &cs_content::mesh::RenderMesh,
    transform: &cs_content::scene::CanonicalTransform,
) -> Result<(), PlaytestError> {
    let mut local_min = [f64::INFINITY; 3];
    let mut local_max = [f64::NEG_INFINITY; 3];
    for vertex in render.vertices() {
        for axis in 0..3 {
            let value = f64::from(vertex.position[axis]);
            if value.is_finite() {
                local_min[axis] = local_min[axis].min(value);
                local_max[axis] = local_max[axis].max(value);
            }
        }
    }
    if !local_min.iter().all(|value| value.is_finite()) {
        return Ok(());
    }
    for corner in 0..8 {
        let point = [
            if corner & 1 == 0 {
                local_min[0]
            } else {
                local_max[0]
            },
            if corner & 2 == 0 {
                local_min[1]
            } else {
                local_max[1]
            },
            if corner & 4 == 0 {
                local_min[2]
            } else {
                local_max[2]
            },
        ];
        let world = transform.apply(point);
        for axis in 0..3 {
            if !world[axis].is_finite() {
                return Err(PlaytestError::NonFinite {
                    what: "a composed object corner",
                });
            }
            min[axis] = min[axis].min(world[axis]);
            max[axis] = max[axis].max(world[axis]);
        }
    }
    Ok(())
}

/// One neutral development material, created once per colour.
fn add_material(app: &mut App, color: [f32; 4]) -> Handle<StandardMaterial> {
    app.world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgba(color[0], color[1], color[2], color[3]),
            metallic: 0.0,
            // No back-face culling, and this is a declared decision rather than a
            // default: whether a stored winding is front-facing is F17's open
            // `FrontFaceWinding` question, so culling on an unmeasured rule would
            // make the frame depend on a question this stage does not answer, and
            // a one-sided mesh would come back as empty sky — which a capture
            // would then refuse. Drawing both sides keeps the frame about the
            // stored triangles and nothing else.
            cull_mode: None,
            ..default()
        })
}

/// The camera, the two lights and the render target a capture drives, at `view`.
fn add_camera_and_lights(
    app: &mut App,
    view: &PlaytestCameraView,
    width: u32,
    height: u32,
) -> (Entity, Vec<Entity>, CaptureTarget) {
    let image = capture_image(width, height);
    let handle = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    let distance = bevy::math::Vec3::from(view.target).distance(bevy::math::Vec3::from(view.eye));
    let camera = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            Camera {
                clear_color: ClearColorConfig::Custom(Color::srgba(
                    CLEAR_COLOR[0],
                    CLEAR_COLOR[1],
                    CLEAR_COLOR[2],
                    CLEAR_COLOR[3],
                )),
                ..default()
            },
            RenderTarget::Image(handle.clone().into()),
            Projection::Perspective(PerspectiveProjection {
                fov: CAMERA_FOV_DEGREES.to_radians(),
                near: (distance * NEAR_PLANE_FRACTION).max(0.01),
                far: distance * FAR_PLANE_FACTOR,
                ..PerspectiveProjection::default()
            }),
            Transform::from_xyz(view.eye[0], view.eye[1], view.eye[2])
                .looking_at(bevy::math::Vec3::from(view.target), bevy::math::Vec3::Y),
        ))
        .id();
    let key = DirectionalLight {
        illuminance: KEY_LIGHT_ILLUMINANCE,
        ..default()
    };
    let fill = DirectionalLight {
        illuminance: KEY_LIGHT_ILLUMINANCE * FILL_FRACTION,
        ..default()
    };
    let target = bevy::math::Vec3::from(view.target);
    let key_entity = app
        .world_mut()
        .spawn((
            key,
            Transform::from_xyz(
                view.eye[0],
                view.eye[1] + distance * 0.5,
                view.eye[2] + distance,
            )
            .looking_at(target, bevy::math::Vec3::Y),
        ))
        .id();
    let fill_entity = app
        .world_mut()
        .spawn((
            fill,
            Transform::from_xyz(
                view.eye[0] - distance,
                view.eye[1],
                view.eye[2] - distance * 0.4,
            )
            .looking_at(target, bevy::math::Vec3::Y),
        ))
        .id();
    (
        camera,
        vec![key_entity, fill_entity],
        CaptureTarget { image: handle },
    )
}

/// The render target a frame is drawn into and read back from.
fn capture_image(width: u32, height: u32) -> Image {
    let size = Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[
            (CLEAR_COLOR[0] * 255.0) as u8,
            (CLEAR_COLOR[1] * 255.0) as u8,
            (CLEAR_COLOR[2] * 255.0) as u8,
            255,
        ],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    // The screenshot path reads the target back through a buffer, so the texture
    // needs `COPY_SRC`; it is also sampled, so it needs `TEXTURE_BINDING`.
    image.texture_descriptor.usage = TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC
        | TextureUsages::TEXTURE_BINDING
        | TextureUsages::RENDER_ATTACHMENT;
    image.sampler = ImageSampler::linear();
    image
}

/// Drives [`MESH_SETTLE_UPDATES`] updates so the derived colliders exist.
///
/// Avian's hierarchy constructor is an `Update` system and the collider is
/// attached to its body by a later pass, so a scene inspected before this has no
/// collision at all. Returns how many of the scene's records now carry a
/// [`Collider`](avian3d::prelude::Collider).
pub fn settle_playtest_colliders(app: &mut App, scene: &PlaytestScene) -> usize {
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }
    let world = app.world();
    scene
        .spawned()
        .objects()
        .iter()
        .filter(|object| {
            object.collider.as_ref().is_some_and(|collider| {
                world
                    .get::<avian3d::prelude::Collider>(collider.entity)
                    .is_some()
            })
        })
        .count()
}

/// Despawns exactly what [`spawn_playtest_scene`] created.
///
/// Nothing else in `cs_app` holds a handle to these entities, so this is the whole
/// teardown, and the world mesh assets the loader registered are released with
/// them: `WorldMeshAssets` is the loader's owning handle, so removing the resource
/// drops its strong handles and the engine meshes with them. A spawn after a
/// teardown therefore starts from the same live-entity count and the same asset
/// count it started from the first time.
///
/// The size of the release is measured **before** the resource goes
/// ([`PlaytestTeardown::released_world_mesh_assets`]), because the count after it
/// is zero by construction and therefore says nothing on its own about whether
/// anything was held.
pub fn teardown_playtest_scene(app: &mut App, scene: &PlaytestScene) -> PlaytestTeardown {
    let mut count = 0;
    for entity in &scene.entities {
        if app.world_mut().despawn(*entity) {
            count += 1;
        }
    }
    let released = app
        .world()
        .get_resource::<WorldMeshAssets>()
        .map_or(0, WorldMeshAssets::len);
    app.world_mut().remove_resource::<WorldMeshAssets>();
    PlaytestTeardown {
        entities: count,
        released_world_mesh_assets: released,
        world_mesh_assets: app
            .world()
            .get_resource::<WorldMeshAssets>()
            .map_or(0, WorldMeshAssets::len),
    }
}

/// Whether the aircraft is currently presented.
fn aircraft_visible(app: &mut App, scene: &PlaytestScene, visible: bool) {
    let value = if visible {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    app.world_mut()
        .entity_mut(scene.aircraft_entity)
        .insert(value);
}

/// One captured view, all of it measured from the frames.
#[derive(Clone, Debug, PartialEq)]
pub struct PlaytestCapture {
    /// The view's label.
    pub view: &'static str,
    /// The eye the frame was taken from.
    pub eye: [f32; 3],
    /// What the camera looked at.
    pub target: [f32; 3],
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// How many distinct luminance levels the frame holds. One means the whole
    /// frame is the clear colour.
    pub distinct_luminance: usize,
    /// Pixels that differ from the clear colour.
    pub covered_pixels: usize,
    /// [`Self::covered_pixels`] over the frame's pixel count, in permille.
    pub covered_permille: u32,
    /// Pixels that changed when the aircraft was hidden: the aircraft's own
    /// contribution to this frame, measured as a difference between two renders of
    /// the same scene rather than assumed from its size.
    pub aircraft_pixels: usize,
    /// Pixels the area's geometry contributed, measured as the covered pixels of
    /// the same frame with the aircraft hidden.
    pub environment_pixels: usize,
    /// Where the PNG was written.
    pub png: String,
    /// SHA-256 of the written PNG's bytes.
    pub png_sha256: ContentHash,
    /// How many bytes the PNG has.
    pub png_bytes: u64,
}

impl PlaytestCapture {
    /// Whether this frame drew the area's geometry at a share worth reading.
    #[must_use]
    pub fn drew_environment(&self) -> bool {
        self.distinct_luminance > 1 && self.covered_permille >= MIN_ENVIRONMENT_PERMILLE
    }

    /// Whether this frame drew the aircraft at all.
    #[must_use]
    pub fn drew_aircraft(&self) -> bool {
        self.aircraft_pixels >= MIN_AIRCRAFT_PIXELS
    }
}

/// Why one captured frame is not evidence of a drawn scene.
#[derive(Clone, Debug, PartialEq)]
pub enum CaptureError {
    /// The renderer produced no image within the bound, which is a driver-side
    /// absence rather than a blank frame.
    NoFrame {
        /// The view being captured.
        view: &'static str,
        /// How many updates were driven.
        updates: u32,
    },
    /// The frame came back and every pixel is the clear colour.
    UniformFrame {
        /// The view being captured.
        view: &'static str,
        /// How many distinct luminance levels it held. Always one.
        distinct_luminance: usize,
    },
    /// The frame drew less than [`MIN_ENVIRONMENT_PERMILLE`] of anything that is
    /// not the clear colour: it framed sky rather than geometry.
    NoEnvironment {
        /// The view being captured.
        view: &'static str,
        /// The share of the frame that was not sky, in permille.
        covered_permille: u32,
    },
    /// The aircraft contributed nothing to the frame it was in.
    NoAircraft {
        /// The view being captured.
        view: &'static str,
    },
    /// The frame could not be read back from disk.
    Io {
        /// The path involved.
        path: String,
        /// The operating system's message.
        reason: String,
    },
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFrame { view, updates } => write!(
                f,
                "the renderer drove {updates} updates for view {view:?} and produced no image, \
                 so no frame came back"
            ),
            Self::UniformFrame {
                view,
                distinct_luminance,
            } => write!(
                f,
                "view {view:?} came back with {distinct_luminance} distinct luminance level(s), \
                 i.e. every pixel is the clear colour: nothing was drawn"
            ),
            Self::NoEnvironment {
                view,
                covered_permille,
            } => write!(
                f,
                "view {view:?} framed {covered_permille} permille of non-sky pixels, below the \
                 {MIN_ENVIRONMENT_PERMILLE} this stage requires, so it framed sky rather than \
                 geometry"
            ),
            Self::NoAircraft { view } => write!(
                f,
                "view {view:?} changed by no pixel when the aircraft was hidden, so the aircraft \
                 is not in that frame"
            ),
            Self::Io { path, reason } => {
                write!(f, "the capture image {path} could not be used: {reason}")
            }
        }
    }
}

impl std::error::Error for CaptureError {}

/// The render target every frame is drawn into and read back from.
///
/// Deliberately **not** a `Resource`: the image belongs to the scene that created
/// it, so [`PlaytestScene`] holds it and a second scene in the same `App` captures
/// into its own target instead of overwriting a shared one.
struct CaptureTarget {
    image: Handle<Image>,
}

/// Where the observer writes the frame that just arrived. `None` for the
/// measurement pass, whose pixels are differenced in memory and never written.
#[derive(Resource, Default)]
struct CaptureSlot(Mutex<Option<PathBuf>>);

/// What the observer recorded about the frame that came back.
#[derive(Resource, Default)]
struct CapturedFrame(Mutex<Option<FrameFacts>>);

/// The measured facts about one frame.
#[derive(Clone, Debug, PartialEq)]
struct FrameFacts {
    width: u32,
    height: u32,
    distinct_luminance: usize,
    covered_pixels: usize,
    pixels: Vec<u8>,
}

/// One render of one view: the measurement plus the artifact on disk.
struct RenderedView {
    view: &'static str,
    eye: [f32; 3],
    target: [f32; 3],
    facts: FrameFacts,
    covered_permille: u32,
    png: String,
    png_sha256: ContentHash,
    png_bytes: u64,
}

/// Renders the scene's documented views on the real GPU and writes one PNG each.
///
/// Each view is rendered **twice**: once with the aircraft presented and once
/// with it hidden. The difference between the two frames is the aircraft's own
/// contribution and the hidden frame is the environment's own contribution, both
/// **measured** rather than assumed. That is what turns "the aircraft and the
/// environment are both in frame" into a measurement instead of a claim, and it
/// is why a 10 m aircraft inside an 840 m area is not something this stage has to
/// take on trust. Only the frame with the aircraft is written to disk: the second
/// render is a measurement, not an artifact, so exactly one PNG per documented
/// view ends up on disk.
///
/// Every refusal leaves no PNG behind: a view that drew nothing, framed only sky
/// or showed no aircraft has its file removed, so a file that exists is a frame
/// that was measured. The PNGs go under `out_dir`, which must exist; this stage
/// writes under `private/` (ignored by Git) rather than choosing a location of its
/// own, and no original bytes or screenshots are ever committed.
///
/// # Errors
///
/// [`PlaytestError::Capture`] for every [`CaptureError`].
pub fn capture_playtest_views(
    app: &mut App,
    scene: &PlaytestScene,
    out_dir: &Path,
) -> Result<Vec<PlaytestCapture>, PlaytestError> {
    if !app.world().contains_resource::<CapturedFrame>() {
        app.init_resource::<CapturedFrame>();
        app.add_observer(
            |captured: On<ScreenshotCaptured>,
             frame: Res<CapturedFrame>,
             slot: Res<CaptureSlot>| {
                if captured.image.data.is_some() {
                    let image = captured.image.clone();
                    if let Ok(mut guard) = frame.0.lock() {
                        *guard = Some(measure(&image));
                    }
                }
                // The measurement pass carries no path, so nothing is written for
                // it: one documented view, one PNG.
                if let Ok(path) = slot.0.lock()
                    && let Some(path) = path.clone()
                {
                    save_to_disk(path)(captured);
                }
            },
        );
    }
    if !app.world().contains_resource::<CaptureSlot>() {
        app.init_resource::<CaptureSlot>();
    }
    let mut captures = Vec::with_capacity(scene.views.len());
    for view in scene.views() {
        let png = out_dir.join(format!(
            "playtest-retail-{}-{}.png",
            scene.config.world_group.to_ascii_lowercase(),
            view.name
        ));
        aircraft_visible(app, scene, true);
        let with = match render_view(app, scene, view, Some(png.clone())) {
            Ok(rendered) => rendered,
            Err(error) => {
                let _ = fs::remove_file(&png);
                return Err(error);
            }
        };
        aircraft_visible(app, scene, false);
        let without = match render_view(app, scene, view, None) {
            Ok(rendered) => rendered,
            Err(error) => {
                aircraft_visible(app, scene, true);
                let _ = fs::remove_file(&png);
                return Err(error);
            }
        };
        aircraft_visible(app, scene, true);

        let aircraft_pixels = differing(&with.facts.pixels, &without.facts.pixels);
        let environment_pixels = without.facts.covered_pixels;
        if !with.facts.distinct_luminance.gt(&1) {
            let _ = fs::remove_file(&png);
            return Err(PlaytestError::Capture(CaptureError::UniformFrame {
                view: view.name,
                distinct_luminance: with.facts.distinct_luminance,
            }));
        }
        if with.covered_permille < MIN_ENVIRONMENT_PERMILLE {
            let _ = fs::remove_file(&png);
            return Err(PlaytestError::Capture(CaptureError::NoEnvironment {
                view: view.name,
                covered_permille: with.covered_permille,
            }));
        }
        if aircraft_pixels < MIN_AIRCRAFT_PIXELS {
            let _ = fs::remove_file(&png);
            return Err(PlaytestError::Capture(CaptureError::NoAircraft {
                view: view.name,
            }));
        }
        let rendered = RenderedView {
            view: with.view,
            eye: with.eye,
            target: with.target,
            facts: with.facts,
            covered_permille: with.covered_permille,
            png: with.png,
            png_sha256: with.png_sha256,
            png_bytes: with.png_bytes,
        };
        captures.push(PlaytestCapture {
            view: rendered.view,
            eye: rendered.eye,
            target: rendered.target,
            width: rendered.facts.width,
            height: rendered.facts.height,
            distinct_luminance: rendered.facts.distinct_luminance,
            covered_pixels: rendered.facts.covered_pixels,
            covered_permille: rendered.covered_permille,
            aircraft_pixels,
            environment_pixels,
            png: rendered.png,
            png_sha256: rendered.png_sha256,
            png_bytes: rendered.png_bytes,
        });
    }
    Ok(captures)
}

/// Renders one view once and returns its measured facts.
///
/// `png` is the artifact to write, or `None` for the measurement pass. The
/// observer writes it through the renderer's own screenshot path, so the digest in
/// the result is of the file on disk rather than of a buffer this module imagined.
fn render_view(
    app: &mut App,
    scene: &PlaytestScene,
    view: &PlaytestCameraView,
    png: Option<PathBuf>,
) -> Result<RenderedView, PlaytestError> {
    pose_camera(app, scene, view);
    if let Ok(mut slot) = app.world().resource::<CaptureSlot>().0.lock() {
        *slot = png.clone();
    }
    clear_frame(app);
    let image = scene.target.image.clone();
    let updates = (0..MAX_CAPTURE_UPDATES).find(|&update| {
        if update == WARMUP_UPDATES {
            app.world_mut().spawn(Screenshot::image(image.clone()));
        }
        app.update();
        app.world()
            .resource::<CapturedFrame>()
            .0
            .lock()
            .is_ok_and(|guard| guard.is_some())
    });
    let facts = app
        .world()
        .resource::<CapturedFrame>()
        .0
        .lock()
        .ok()
        .and_then(|guard| guard.clone());
    let Some(facts) = facts else {
        return Err(PlaytestError::Capture(CaptureError::NoFrame {
            view: view.name,
            updates: updates.unwrap_or(MAX_CAPTURE_UPDATES - 1) + 1,
        }));
    };
    let total = u64::from(facts.width) * u64::from(facts.height);
    let covered_permille = if total == 0 {
        0
    } else {
        ((facts.covered_pixels as u128 * 1000) / total as u128) as u32
    };
    let Some(path) = png else {
        return Ok(RenderedView {
            view: view.name,
            eye: view.eye,
            target: view.target,
            facts,
            covered_permille,
            png: String::new(),
            png_sha256: ContentHash::from_bytes([0_u8; 32]),
            png_bytes: 0,
        });
    };
    let bytes = fs::read(&path).map_err(|error| {
        PlaytestError::Capture(CaptureError::Io {
            path: path.display().to_string(),
            reason: error.to_string(),
        })
    })?;
    Ok(RenderedView {
        view: view.name,
        eye: view.eye,
        target: view.target,
        facts,
        covered_permille,
        png: path.display().to_string(),
        png_sha256: cs_assets::install::sha256(&bytes),
        png_bytes: bytes.len() as u64,
    })
}

/// Forgets the previous frame so a render loop can tell its own frame back.
fn clear_frame(app: &mut App) {
    if let Ok(mut guard) = app.world_mut().resource_mut::<CapturedFrame>().0.lock() {
        *guard = None;
    }
}

/// Moves the scene's camera and lights onto one view.
fn pose_camera(app: &mut App, scene: &PlaytestScene, view: &PlaytestCameraView) {
    let eye = bevy::math::Vec3::from(view.eye);
    let target = bevy::math::Vec3::from(view.target);
    let distance = eye.distance(target);
    if let Ok(mut camera) = app.world_mut().get_entity_mut(scene.camera) {
        if let Some(mut transform) = camera.get_mut::<Transform>() {
            *transform = Transform::from_translation(eye).looking_at(target, bevy::math::Vec3::Y);
        }
        if let Some(mut projection) = camera.get_mut::<Projection>() {
            *projection = Projection::Perspective(PerspectiveProjection {
                fov: CAMERA_FOV_DEGREES.to_radians(),
                near: (distance * NEAR_PLANE_FRACTION).max(0.01),
                far: distance * FAR_PLANE_FACTOR,
                ..PerspectiveProjection::default()
            });
        }
    }
    for (index, entity) in scene.lights.iter().enumerate() {
        let Ok(mut light) = app.world_mut().get_entity_mut(*entity) else {
            continue;
        };
        let side = if index == 0 { 0.5 } else { -1.0 };
        let position = bevy::math::Vec3::new(
            view.eye[0] + side * distance,
            view.eye[1] + (side + 1.0) * distance * 0.4,
            view.eye[2] + distance * (1.0 - side * 0.5),
        );
        if let Some(mut transform) = light.get_mut::<Transform>() {
            *transform =
                Transform::from_translation(position).looking_at(target, bevy::math::Vec3::Y);
        }
    }
}

/// How many pixels differ between two frames.
fn differing(with: &[u8], without: &[u8]) -> usize {
    if with.len() != without.len() {
        return 0;
    }
    with.as_chunks::<4>()
        .0
        .iter()
        .zip(without.as_chunks::<4>().0.iter())
        .filter(|(left, right)| left != right)
        .count()
}

/// The measured facts of one frame: its size, how many distinct luminance levels
/// it holds, how many pixels differ from the clear colour, and its own pixels.
fn measure(image: &Image) -> FrameFacts {
    let data = image.data.as_ref().expect("a captured frame has data");
    let clear = [
        (CLEAR_COLOR[0] * 255.0).round() as u8,
        (CLEAR_COLOR[1] * 255.0).round() as u8,
        (CLEAR_COLOR[2] * 255.0).round() as u8,
    ];
    let mut levels: std::collections::BTreeSet<u8> = std::collections::BTreeSet::new();
    let mut covered = 0_usize;
    for pixel in data.as_chunks::<4>().0 {
        // Rec. 601 luminance in eight bits: the frame's own shading order, not a
        // colour-fidelity claim.
        let luminance = ((29 * u32::from(pixel[0])
            + 150 * u32::from(pixel[1])
            + 77 * u32::from(pixel[2]))
            >> 8) as u8;
        levels.insert(luminance);
        if pixel[0] != clear[0] || pixel[1] != clear[1] || pixel[2] != clear[2] {
            covered += 1;
        }
    }
    FrameFacts {
        width: image.width(),
        height: image.height(),
        distinct_luminance: levels.len(),
        covered_pixels: covered,
        pixels: data.clone(),
    }
}
