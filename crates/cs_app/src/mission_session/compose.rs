//! The windowed mission composition and its no-window face (VS-M01-RT-WINDOW,
//! Rally #1215): what `cs --cs-path <install> --mission M01` opens.
//!
//! A [`MissionStage`] is every record the composition spawns from, taken
//! from the prepared [`super::MissionContent`] by [`stage_for`] — the world
//! definition, its engine meshes, this mission's load record, the measured
//! start pose and the flight law, plus the one announced load this
//! composition runs through the F15 machinery and the [`MissionHostSeed`]
//! the mission host drives every committed tick. [`build_windowed`] turns a
//! stage into the real window (renderer, wall-clock fixed loop);
//! [`build_headless`] turns the same stage into the app the acceptance tests
//! drive. The two differ only in their plugin sets — the composition, the
//! systems, the spawn and the teardown are one code path.
//!
//! What the composition wires, and what it deliberately does not:
//!
//! * **Flight, input, camera, pause/reset/quit and telemetry are the
//!   playtest's own systems** (`crate::playtest`): the player body carries
//!   [`PlaytestAircraft`](crate::playtest::scene::PlaytestAircraft) so the
//!   existing query systems drive it, and the one additive seam is
//!   [`AircraftSpawner`](crate::playtest::AircraftSpawner), which replaces
//!   the playtest's designed aircraft with the mission's player body at the
//!   mission's pose — `--playtest` itself is unchanged without a mission
//!   present.
//! * **The world is M01's own world**, loaded through
//!   [`crate::world::residency::load_world`] — the residency record is the
//!   structural guarantee that a second build starts from nothing and that
//!   [`teardown`] releases the engine mesh assets.
//! * **The LoadingSession attach** is the F15 controlled handoff: the load
//!   is announced to the Bevy world before anything is read, driven to
//!   `Ready` through the production [`LoadingSession`] driver with
//!   [`SessionIo`] reading the announced member through a mounted
//!   `ContentSession`, and delivered at the ready boundary through
//!   [`LoadingSession::deliver`] — never mid-tick, never a bundle the world
//!   is not waiting for.
//! * **The player body** is spawned through the production physics path
//!   (`spawn_body` on the `Aircraft` layer) and flies the mission's own
//!   flight law through the retail scene's original-law driver
//!   (`drive_original_flight`): the body integrates the linear pose, the
//!   law owns the attitude, exactly as `docs/findings/
//!   2026-10-08-flight-original-fixed-wing-law.md` records.
//!
//! What this stage does **not** claim, each named rather than papered over:
//!
//! * The player's airframe mesh is not drawn (VS-M01-RT-PLAYER-AIRFRAME-VISUAL,
//!   #1216): the body's collider is the playtest's declared development box,
//!   and no child mesh is attached to it.
//! * The mission host runs the stage's records per committed tick
//!   (VS-M01-RT-MISSION-HOST, #1217): this stage prepares them, leaves them
//!   on [`MissionHostSeed`] and launches them through [`add_composition`].
//!   A terminal outcome and a restart are still open (#1217's `.02` and
//!   `.03`), so neither half runs yet.
//! * No mission-bound audio source is started, so teardown stops none; the
//!   sound archives travel on [`super::MissionContent`] for the host stage.
//! * `R` restarts the player body at the mission's start pose with a fresh
//!   flight state; a full mission restart (world unload, reload and a new
//!   loading attach) belongs to #1217.
//! * The flight law is the statically recovered one (its provenance label
//!   travels on [`super::MissionFlight`]) — never `verified_original`, and
//!   uncalibrated against an original run (#358).
//! * Which body axis the original airframe node's nose occupies in the
//!   canonical frame was measured for neither the pose heading nor the
//!   recovered law's `-Z` forward; the heading is applied through the
//!   measured `Object3d` compose and the residue is recorded in
//!   `docs/findings/2026-10-10-vs-m01-rt-window-composition.md`.

use std::fmt;
use std::path::{Path, PathBuf};

use avian3d::prelude::{AngularVelocity, NoAutoMass, PhysicsPlugins, Rotation};
use bevy::app::{App, AppExit};
use bevy::prelude::{Entity, Or, Quat, Resource, Transform, Vec3, With, World};
use cs_assets::cache::{CacheBudget, CacheDirectory, CacheStore};
use cs_assets::vfs::{ContentSession, MountBuilder, SessionBuilder};
use cs_content::world::{WorldDefinition, WorldInstance};
use cs_sim::flight::original::FORCE_TO_ACCEL;
use cs_sim::flight::{FlightInput, OriginalFlightModel, OriginalState};
use cs_types::asset_id::{
    AssetKey, MissionScope, MountId, MountNamespace, PrecedenceClass, ResolveContext, WorldGroup,
};
use cs_types::content::{ContentId, ContentKind, Resolved};
use cs_types::evidence::ContentHash;
use cs_types::space::Quaternion;

use crate::loading::{
    Criticality, ExpectedLoad, LoadItem, LoadRequest, LoadState, LoadTarget, LoadedItemBinding,
    LoadingSession, SessionIo,
};
use crate::mission_launch::MissionLaunchPlan;
use crate::mission_markers::MissionMarkerBindings;
use crate::mission_session::MissionFlight;
use crate::mission_session::host::{
    MissionHost, MissionHostRefusal, MissionHostSeed, host_session_id, install_mission_host,
    mint_host_generation, no_declared_objectives,
};
use crate::mission_start::StartPose;
use crate::mission_world_actors::object3d_orientation;
use crate::objectives::LoweredObjectives;
use crate::physics::{BodyMode, BodySpec, spawn_body};
use crate::playtest::retail::RETAIL_START_SPEED_M_S;
use crate::playtest::scene::{
    AIRCRAFT_HALF_EXTENTS_M, PlaytestAircraft, PlaytestOriginalFlight, SceneError,
};
use crate::playtest::{AircraftSpawner, PlaytestPlugin, PlaytestState};
use crate::world::WorldMeshes;
use crate::world::residency::{load_world, unload_world};

/// The label every surface of the mission composition shows while the
/// player-airframe visual is pending (#1216). The composition draws no
/// player mesh of its own, so a plain marker states what is flying and what
/// is not — never a claim the composition cannot back.
pub const MISSION_COMPOSITION_LABEL: &str =
    "ORIGINAL MISSION M01 / ORIGINAL WORLD / PLAYER AIRFRAME VISUAL PENDING";

/// Where the composition keeps its private cache by default: the
/// Git-ignored `private/` tree beside the working directory, the same
/// convention the playtest's captures use. [`CacheDirectory`] refuses a
/// root inside the installation, and this never writes into `$CS_GAME_DIR`.
pub const DEFAULT_MISSION_CACHE_DIR: &str = "private/mission-cache";

/// The mount id the composition's one announced load resolves through.
const WINDOW_MOUNT_ID: &str = "mission-window";

/// The namespace the announced items are keyed under: the world mount's own
/// namespace, so a key addresses the mounted group directory.
const WINDOW_MOUNT_NAMESPACE: &str = "world";

/// One item of the composition's announced load: the member path the
/// [`SessionIo`] producer resolves inside the mounted directory, the stable
/// content id the delivered entity records, the criticality that decides
/// whether the load may reach interactivity without it, and the member's
/// measured byte length (the load's declared work units).
#[derive(Clone, Debug)]
pub struct StageItem {
    /// The member path relative to the mount's directory.
    pub path: String,
    /// The stable content id the delivered binding records.
    pub content: ContentId,
    /// Whether the load may become interactive without this item.
    pub criticality: Criticality,
    /// The member's byte length, measured from the installation — the
    /// progress unit the load declares; zero is refused by `LoadItem`.
    pub bytes: u64,
}

/// The one mount the stage's announced items resolve through: a directory
/// of the read-only installation, mounted into a fresh content session the
/// way the F15 contract tests mount theirs.
#[derive(Clone, Debug)]
pub struct StageMount {
    /// The mount's id inside the session.
    pub id: &'static str,
    /// The namespace its keys are addressed under.
    pub namespace: &'static str,
    /// How strongly the mount competes for a key.
    pub precedence: PrecedenceClass,
    /// The mount's logical directory spelling, e.g. `zbd/c1c`.
    pub directory: String,
    /// The host directory of the read-only installation behind it.
    pub host: PathBuf,
    /// The world group the session binds.
    pub world_group: WorldGroup,
    /// The mission scope the session binds, when the load is mission-scoped.
    pub mission: Option<MissionScope>,
}

/// Everything one composition spawns from.
///
/// [`stage_for`] builds it from a satisfied launch plan and the prepared
/// content; the acceptance tests build one over the synthetic harbor world
/// to drive the same composition code without an installation.
#[derive(Clone, Debug)]
pub struct MissionStage {
    /// What the announced load builds.
    pub target: LoadTarget,
    /// The mount the announced items resolve through.
    pub mount: StageMount,
    /// The load's closure: announced before anything is delivered, attached
    /// at the ready boundary.
    pub items: Vec<StageItem>,
    /// The world definition the residency spawns.
    pub world: WorldDefinition,
    /// The engine meshes the definition names.
    pub meshes: WorldMeshes,
    /// This mission's load record: population, initial damage, provenance.
    pub instance: WorldInstance,
    /// The player's measured start pose.
    pub start: StartPose,
    /// The flight law the player's body flies.
    pub flight: MissionFlight,
    /// The label every surface shows.
    pub label: String,
    /// The private cache root, outside the installation.
    pub cache_root: PathBuf,
    /// The installation fingerprint the session binds under.
    pub installation: ContentHash,
    /// The records the mission host drives every committed tick: the five the
    /// announced-load stage used to drop, the declared control program, the
    /// objective program, the cue table, the chain resolver and every absence
    /// the stage already knows about (VS-M01-RT-MISSION-HOST, #1217).
    pub host: MissionHostSeed,
}

/// Why a mission composition could not be built or could not run.
///
/// Every variant carries the refusing stage's own message; a failure is
/// never returned as success (`docs/contracts/CLI-EVIDENCE.md`).
#[derive(Debug)]
pub enum MissionCompositionError {
    /// The content could not be prepared through the production readers.
    Session(crate::mission_session::MissionSessionError),
    /// A stage identity (world group, mission scope, installation hash)
    /// could not be spelled.
    Identity(String),
    /// The announced item's byte length could not be measured: the member
    /// is missing from the host directory or unreadable.
    Item {
        /// The member that was asked for.
        key: String,
        /// The refusing path and reason.
        reason: String,
    },
    /// The content session could not be opened over the mount.
    Mount(String),
    /// The private cache could not be opened.
    Cache(String),
    /// The load did not reach `Ready`; every recorded failure is named.
    Load(Vec<String>),
    /// The driver refused the load.
    Driver(String),
    /// The ready bundle would not attach to the world that announced it.
    Handoff(String),
    /// The world could not become resident.
    World(crate::world::WorldLoadError),
    /// The player body could not be spawned through the production physics
    /// path.
    Player(String),
    /// The mission host's records could not be launched as a session.
    Host(crate::mission_session::MissionHostLaunchError),
    /// The app exited with an error.
    Exit(String),
}

impl fmt::Display for MissionCompositionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Session(error) => write!(f, "the mission's content refuses: {error}"),
            Self::Identity(reason) => write!(f, "the composition's identity refuses: {reason}"),
            Self::Item { key, reason } => {
                write!(f, "{key}: the announced load item refuses: {reason}")
            }
            Self::Mount(reason) => write!(f, "the mission mount refuses: {reason}"),
            Self::Cache(reason) => write!(f, "the mission cache refuses: {reason}"),
            Self::Load(failures) => write!(
                f,
                "the mission load failed before the world could become interactive: {}",
                failures.join("; ")
            ),
            Self::Driver(reason) => write!(f, "the mission load driver refuses: {reason}"),
            Self::Handoff(reason) => write!(f, "the ready bundle refuses to attach: {reason}"),
            Self::World(error) => write!(f, "the mission's world refuses to load: {error}"),
            Self::Player(reason) => write!(f, "the mission's player body refuses: {reason}"),
            Self::Host(error) => write!(f, "the mission host refuses: {error}"),
            Self::Exit(reason) => write!(f, "the mission window exited with an error: {reason}"),
        }
    }
}

impl std::error::Error for MissionCompositionError {}

/// The player body's own marker: what teardown despawns and what the
/// acceptance tests count. The body also carries
/// [`PlaytestAircraft`](crate::playtest::scene::PlaytestAircraft) so the
/// playtest's input, camera, telemetry and reset systems drive it
/// unchanged.
#[derive(bevy::prelude::Component, Clone, Copy, Debug, Default)]
pub struct MissionPlayerBody;

/// How the composition's player body is (re)spawned: the mission's flight
/// law, the measured start pose, and the declared development stand-ins
/// this stage still needs (#1216 draws the real airframe).
#[derive(Resource, Clone, Debug)]
pub struct MissionPlayerStart {
    /// The mission's flight law with its imported parameters.
    pub flight: OriginalFlightModel,
    /// The imported fuel load, in the law's own units.
    pub fuel: f64,
    /// The measured start position, metres.
    pub position_m: [f32; 3],
    /// The measured start orientation: the heading composed through the
    /// production [`object3d_orientation`] (`M = Ry·Rx·Rz`, #770 §12.2).
    pub orientation: Quaternion,
    /// The spawn velocity, m/s: the body's nose (`BODY_FORWARD`) at the
    /// declared development start speed.
    pub velocity_m_s: [f32; 3],
    /// Half extents of the body's collider.
    ///
    /// **A declared development stand-in** (the playtest's own box), not a
    /// measured extent of the `pdevastator` airframe: the player-airframe
    /// measurement and its mesh belong to VS-M01-RT-PLAYER-AIRFRAME-VISUAL
    /// (#1216), and this stage presents it as nothing more than the body a
    /// collision solver needs.
    pub half_extents_m: [f32; 3],
    /// The law's provenance label, carried onto the record.
    pub provenance: &'static str,
}

/// What one built composition holds: the load it announced, the entities
/// the ready bundle attached, and the player body the Startup spawned.
#[derive(Resource, Debug)]
pub struct MissionComposition {
    /// The announced load's identity (session generation + serial).
    pub load: crate::loading::LoadIdentity,
    /// The entities [`LoadingSession::deliver`] attached, despawned by
    /// [`teardown`].
    pub delivered: Vec<Entity>,
    /// The player body; replaced when `R` restarts the player.
    pub player: Entity,
}

/// Derives the retail stage from a satisfied launch plan and the
/// installation.
///
/// The content is read through [`super::MissionContent::prepare`] (the
/// VS-M01-RT-CONTENT stage); the announced load is the world group's own
/// `gamez.zbd`, the container the residency will spawn from, measured to
/// the byte from the installation the plan was bound under. The private
/// cache defaults to [`DEFAULT_MISSION_CACHE_DIR`] under the working
/// directory — never inside the installation.
///
/// # Errors
///
/// [`MissionCompositionError::Session`] when a production reader refuses a
/// record, and the identity/item variants when a stage identity or the
/// announced member cannot be spelled against this installation.
pub fn stage_for(
    install_root: &Path,
    plan: &MissionLaunchPlan,
) -> Result<MissionStage, MissionCompositionError> {
    let content = super::MissionContent::prepare(install_root, plan)
        .map_err(MissionCompositionError::Session)?;
    let start = match content.start.initial_pose() {
        Resolved::Known(known) => known.value,
        Resolved::Unknown { reason, .. } => {
            return Err(MissionCompositionError::Identity(format!(
                "the start configuration leaves the player's initial pose unknown: {reason}"
            )));
        }
    };
    let world_group = WorldGroup::new(&plan.group_dir).map_err(|error| {
        MissionCompositionError::Identity(format!(
            "the world group {:?} is not a valid spelling: {error}",
            plan.group_dir
        ))
    })?;
    let mission_scope = MissionScope::new(plan.catalog_id.key()).map_err(|error| {
        MissionCompositionError::Identity(format!(
            "the catalog id {:?} is not a valid mission scope: {error}",
            plan.catalog_id.key()
        ))
    })?;
    let installation = ContentHash::from_hex(&plan.install_sha256).map_err(|error| {
        MissionCompositionError::Identity(format!(
            "the installation fingerprint is not a content hash: {error}"
        ))
    })?;
    let target = LoadTarget::world(world_group.clone()).with_mission(mission_scope.clone());

    // The announced item: the group container the residency will spawn
    // from, addressed inside the mounted group directory.
    let container_member = "gamez.zbd";
    let container_key = format!("{}/{}", plan.group_dir, container_member);
    let host = install_root.join(&container_key);
    let bytes = std::fs::metadata(&host)
        .map_err(|error| MissionCompositionError::Item {
            key: container_key.clone(),
            reason: format!("cannot measure {}: {error}", host.display()),
        })?
        .len();
    if bytes == 0 {
        return Err(MissionCompositionError::Item {
            key: container_key,
            reason: "the announced member holds no bytes".to_owned(),
        });
    }
    let content_id = ContentId::from_source(
        ContentKind::SceneNode,
        &format!("container.{}", container_key.replace('/', ".")),
    )
    .map_err(|error| MissionCompositionError::Identity(error.to_string()))?;
    let item = StageItem {
        path: container_member.to_owned(),
        content: content_id,
        criticality: Criticality::GameplayCritical,
        bytes,
    };

    let mut refusals = Vec::new();
    let host = MissionHostSeed {
        environment: content.environment.clone(),
        world_actors: content.world_actors.lowered().cloned(),
        animation: Some(content.animation.clone()),
        control: content.control.clone(),
        sound_archives: content.sound_archives.clone(),
        objectives: declared_objectives(install_root, plan, &mut refusals),
        // M01's `mission_markers` ships no cue table on purpose: nobody has
        // declared a cue → signal row for an original mission, so the marker
        // consumer refuses every gameplay cue by name instead of resolving it
        // against a table this stage would have to invent.
        markers: MissionMarkerBindings::default(),
        resolver: content.resolver.clone(),
        refusals,
    };

    Ok(MissionStage {
        target,
        mount: StageMount {
            id: WINDOW_MOUNT_ID,
            namespace: WINDOW_MOUNT_NAMESPACE,
            precedence: PrecedenceClass::MissionWorld,
            directory: plan.group_dir.clone(),
            host: install_root.join(&plan.group_dir),
            world_group,
            mission: Some(mission_scope),
        },
        items: vec![item],
        world: content.world.definition().clone(),
        meshes: content.meshes.clone(),
        instance: content.instance.clone(),
        start,
        flight: content.flight.clone(),
        label: MISSION_COMPOSITION_LABEL.to_owned(),
        cache_root: Path::new(DEFAULT_MISSION_CACHE_DIR).to_path_buf(),
        installation,
        host,
    })
}

/// The objective program this stage's session launches from, and the refusal
/// that names what the F39 reader would not produce.
///
/// The reader is `crate::objectives::recover_retail_objectives` →
/// `ObjectiveRecovery::program()`, called here so the detail a run reports is
/// **the reader's own message** and not a summary of it. Today that call
/// refuses unconditionally for every original mission — the 358 fields of 58
/// blocks are unrecovered and Rally #1219 owns that measurement
/// (`docs/findings/2026-10-10-vs-m01-rt-content-objectives-program-refuses-
/// and-plan-premises.md`) — so the stage carries
/// [`no_declared_objectives`] and records why.
///
/// The `Ok` arms are the forward path: when the recovery does lower a
/// declared program, the stage carries **that** program and raises no
/// refusal, because silently running an empty session where a declared one
/// exists would be the same invention the refusal is there to prevent.
fn declared_objectives(
    install_root: &Path,
    plan: &MissionLaunchPlan,
    refusals: &mut Vec<MissionHostRefusal>,
) -> LoweredObjectives {
    let mut refuse = |detail: String| {
        refusals.push(MissionHostRefusal::ObjectiveDeclarations { detail });
        no_declared_objectives()
    };
    let recovery =
        match crate::objectives::recover_retail_objectives(install_root, &plan.mission_dir) {
            Ok(recovery) => recovery,
            Err(error) => return refuse(error.to_string()),
        };
    let declared = match recovery.program() {
        Ok(declared) => declared,
        Err(refusal) => return refuse(refusal.to_string()),
    };
    match crate::objectives::lower_program(&declared) {
        Ok(lowered) => lowered,
        Err(error) => refuse(error.to_string()),
    }
}

/// The heading-to-orientation compose, through the production
/// `object3d_orientation` (`M = Ry(yaw)` when the other slots are empty).
fn composed_orientation(heading: f32) -> Result<Quaternion, MissionCompositionError> {
    let [x, y, z, w] = object3d_orientation(0.0, f64::from(heading), 0.0);
    Quaternion::try_new([x, y, z, w]).map_err(|error| {
        MissionCompositionError::Identity(format!(
            "the composed start orientation is not a unit quaternion: {error}"
        ))
    })
}

fn quat_from(orientation: Quaternion) -> Quat {
    let [x, y, z, w] = orientation.components();
    Quat::from_xyzw(x as f32, y as f32, z as f32, w as f32)
}

/// Builds the windowed composition: the real window, renderer and
/// wall-clock fixed loop, over the exact same composition
/// [`build_headless`] runs.
///
/// # Errors
///
/// [`MissionCompositionError`] from any stage of the build; nothing is
/// opened or spawned when one refuses.
pub fn build_windowed(stage: &MissionStage) -> Result<App, MissionCompositionError> {
    use bevy::prelude::{DefaultPlugins, PluginGroup, Window, WindowPlugin};
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: format!("Crimson Skies — {}", stage.label),
            resolution: (1280, 720).into(),
            ..Window::default()
        }),
        ..WindowPlugin::default()
    }));
    app.add_plugins(PhysicsPlugins::default());
    add_composition(&mut app, stage, true)?;
    Ok(app)
}

/// Builds the no-window composition: the same plugins and systems as the
/// window, without a window, a GPU or real time. One `update` is one
/// rendered frame of 1/60 s, which runs the fixed loop twice at the
/// baseline rate — the seam the acceptance tests drive.
///
/// # Errors
///
/// [`MissionCompositionError`] from any stage of the build.
pub fn build_headless(stage: &MissionStage) -> Result<App, MissionCompositionError> {
    let mut app = crate::asset_stack::headless_app();
    app.add_plugins(bevy::input::InputPlugin);
    let frame = std::time::Duration::from_secs_f64(1.0 / 60.0);
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(frame));
    add_composition(&mut app, stage, false)?;
    // The playtest's own headless face primes the real clock so the first
    // `update` already carries a frame of time and the fixed loop runs.
    let startup = app
        .world()
        .resource::<bevy::time::Time<bevy::time::Real>>()
        .startup();
    app.world_mut()
        .resource_mut::<bevy::time::Time<bevy::time::Real>>()
        .update_with_instant(startup);
    app.finish();
    app.cleanup();
    Ok(app)
}

/// The shared half of both faces: the playtest core, the announced load
/// through the F15 controlled handoff, the resident world, and the player
/// spawn the Startup runs. `visuals` selects the window's presentation.
fn add_composition(
    app: &mut App,
    stage: &MissionStage,
    visuals: bool,
) -> Result<(), MissionCompositionError> {
    app.add_plugins(PlaytestPlugin);
    if visuals {
        app.add_plugins(crate::playtest::visuals::PlaytestVisualsPlugin);
    }

    // 1. The announced load: a fresh content session over the stage's
    //    mount, announced to the world *before* the first byte is read,
    //    driven to Ready through the production driver, and delivered at
    //    the ready boundary — the F15 handoff exactly as the contract
    //    tests run it.
    let session = open_session(stage)?;
    let request = LoadRequest {
        session: session.generation(),
        target: stage.target.clone(),
        items: load_items(stage)?,
    };
    std::fs::create_dir_all(&stage.cache_root).map_err(|error| {
        MissionCompositionError::Cache(format!(
            "cannot create {}: {error}",
            stage.cache_root.display()
        ))
    })?;
    let directory =
        CacheDirectory::open(&stage.cache_root, &stage.mount.host).map_err(|error| {
            MissionCompositionError::Cache(format!(
                "cannot open the private cache at {}: {error}",
                stage.cache_root.display()
            ))
        })?;
    let store = CacheStore::open(
        directory,
        CacheBudget::new(64, 512 << 20).map_err(|error| {
            MissionCompositionError::Cache(format!("the cache budget refuses: {error}"))
        })?,
    )
    .map_err(|error| MissionCompositionError::Cache(format!("the cache store refuses: {error}")))?;
    let mut loading = LoadingSession::new(request, store);
    let load = loading.identity();
    loading.announce(app.world_mut());
    let mut io = SessionIo::new(&session, |_item, payload| Ok(payload.bytes().to_vec()));
    loading
        .begin()
        .map_err(|error| MissionCompositionError::Driver(error.to_string()))?;
    loading
        .run(&mut io)
        .map_err(|error| MissionCompositionError::Driver(error.to_string()))?;
    if loading.state() != LoadState::Ready {
        return Err(MissionCompositionError::Load(
            loading.failures().iter().map(ToString::to_string).collect(),
        ));
    }
    let delivered = loading
        .deliver(app.world_mut())
        .map_err(|error| MissionCompositionError::Handoff(error.to_string()))?;
    let _store = loading.close();

    // 2. The world: the mission's own, through the residency path.
    load_world(app, &stage.world, &stage.instance, &stage.meshes)
        .map_err(MissionCompositionError::World)?;

    // 3. The player spawn recipe and the playtest seam that uses it: the
    //    Startup spawns the body, and `R` restarts it from the same
    //    recipe.
    let orientation = composed_orientation(stage.start.heading)?;
    let nose = quat_from(orientation) * Vec3::new(0.0, 0.0, -RETAIL_START_SPEED_M_S as f32);
    app.insert_resource(MissionPlayerStart {
        flight: stage.flight.model.clone(),
        fuel: stage.flight.fuel,
        position_m: stage.start.position,
        orientation,
        velocity_m_s: nose.to_array(),
        half_extents_m: AIRCRAFT_HALF_EXTENTS_M,
        provenance: stage.flight.provenance,
    });
    app.insert_resource(MissionComposition {
        load,
        delivered,
        player: Entity::PLACEHOLDER,
    });
    app.insert_resource(AircraftSpawner(spawn_player));
    {
        let world = app.world_mut();
        let mut state = world.resource_mut::<PlaytestState>();
        // The playtest's HUD label is a `&'static str`; the composition's
        // label lives as long as the process, which is at least as long as
        // any app built from this stage.
        state.label = Box::leak(stage.label.clone().into_boxed_str());
        state.spawn_m = stage.start.position;
    }

    // 4. The mission host: every record the stage carries, launched as one
    //    session, and the one composed per-tick entry that advances it in the
    //    fixed-tick schedule after the same tick's physics step.
    let generation = mint_host_generation();
    let host = MissionHost::launch(stage, generation, host_session_id(generation))
        .map_err(MissionCompositionError::Host)?;
    app.insert_resource(host);
    install_mission_host(app);
    Ok(())
}

/// Opens the stage's content session: its mount directory mounted into a
/// fresh session, the way the F15 contract tests mount theirs. The session
/// mints its own generation — a composition never supplies one.
fn open_session(stage: &MissionStage) -> Result<ContentSession, MissionCompositionError> {
    let refused = |reason: String| MissionCompositionError::Mount(reason);
    let mut context =
        ResolveContext::new(stage.installation).with_world_group(stage.mount.world_group.clone());
    if let Some(scope) = &stage.mount.mission {
        context = context.with_mission(scope.clone());
    }
    let mut mount = MountBuilder::new(
        MountId::new(stage.mount.id)
            .map_err(|error| refused(format!("the mount id refuses: {error}")))?,
        MountNamespace::new(stage.mount.namespace)
            .map_err(|error| refused(format!("the mount namespace refuses: {error}")))?,
        stage.mount.precedence,
        &stage.mount.directory,
    )
    .with_world_group(stage.mount.world_group.clone())
    .retail();
    if let Some(scope) = &stage.mount.mission {
        mount = mount.with_mission(scope.clone());
    }
    let mut builder = SessionBuilder::new(context);
    builder
        .mount_directory(mount, &stage.mount.host)
        .map_err(|error| refused(format!("the mount directory refuses: {error}")))?;
    Ok(builder.open())
}

/// Declares the stage's announced closure as real load items: each member's
/// measured byte length is the item's work unit, and the key is the member
/// path inside the mounted directory.
fn load_items(stage: &MissionStage) -> Result<Vec<LoadItem>, MissionCompositionError> {
    let mut items = Vec::with_capacity(stage.items.len());
    for item in &stage.items {
        let key = AssetKey::from_spelling(stage.mount.namespace, &item.path, "default").map_err(
            |error| {
                MissionCompositionError::Identity(format!(
                    "the item key {}/{} refuses: {error}",
                    stage.mount.namespace, item.path
                ))
            },
        )?;
        let declared = LoadItem::new(key, item.content.clone(), item.criticality, item.bytes)
            .map_err(|error| MissionCompositionError::Item {
                key: format!("{}/{}", stage.mount.namespace, item.path),
                reason: error.to_string(),
            })?;
        items.push(declared);
    }
    Ok(items)
}

/// Spawns the mission's player body through the production physics path,
/// exactly as the playtest spawns its aircraft: one dynamic body on the
/// `Aircraft` layer whose declared mass is the imported weight over the
/// law's force scale, carrying the original-law record the playtest's
/// fixed-tick driver steps.
///
/// The pose is the stage's measured start pose: position in metres, the
/// heading composed through the production `object3d_orientation`, and the
/// spawn velocity on the law's own nose axis (`BODY_FORWARD`, `-Z`) at the
/// declared development start speed — the original's player spawn speed
/// was not recovered (#796, unresolved until an original run, #358), so
/// that speed is a documented choice, not a measurement. Which body axis
/// the original airframe node's nose occupies was not measured either; the
/// heading is applied with the measured `Ry` compose and the residue is
/// recorded in `docs/findings/2026-10-10-vs-m01-rt-window-composition.md`.
///
/// The spawned body is marked [`PlaytestAircraft`] (so the playtest's
/// systems drive it) and [`MissionPlayerBody`] (so teardown despawns it).
pub fn spawn_player(world: &mut World) -> Result<Entity, SceneError> {
    let start = world
        .get_resource::<MissionPlayerStart>()
        .cloned()
        .expect("the composition inserted the player's spawn recipe");
    let orientation = quat_from(start.orientation);
    let weight = start.flight.airframe.veh_weight;
    // `BodySpec::validate` refuses a non-positive mass by name, so a law
    // that cannot state one never spawns half a body.
    let mass_kg = (weight / FORCE_TO_ACCEL) as f32;
    let spec = BodySpec {
        layer: cs_sim::collision::CollisionLayer::Aircraft,
        shape: cs_sim::collision::ShapeClass::Solid,
        mode: BodyMode::Dynamic,
        mass_kg,
        half_extents_m: start.half_extents_m,
        position_m: start.position_m,
        linear_velocity_m_s: start.velocity_m_s,
    };
    let entity = spawn_body(world, &spec).map_err(SceneError::Body)?;
    let record = PlaytestOriginalFlight {
        model: start.flight.clone(),
        state: OriginalState {
            position_m: start.position_m.map(f64::from),
            velocity_mps: start.velocity_m_s.map(f64::from),
            orientation: start.orientation,
            angular_momentum_world: [0.0; 3],
            throttle: f64::from(crate::playtest::command::CRUISE_THROTTLE),
            fuel: start.fuel,
            level_off: false,
        },
        command: FlightInput {
            throttle: f64::from(crate::playtest::command::CRUISE_THROTTLE),
            ..FlightInput::NEUTRAL
        },
        hold_attitude: false,
        provenance: start.provenance,
    };
    // The body's `Transform` must carry the same start rotation as the
    // physics `Rotation`: the first physics sync copies the `Transform`'s
    // rotation into `Rotation` when it prepares the body, and a heading
    // that exists on only one of the two is overwritten before the first
    // tick flies it.
    world.entity_mut(entity).insert((
        NoAutoMass,
        record,
        Rotation(orientation),
        Transform::from_translation(Vec3::from_array(start.position_m)).with_rotation(orientation),
        AngularVelocity::ZERO,
        PlaytestAircraft,
        MissionPlayerBody,
    ));
    if let Ok(mut composition) = world.query::<&mut MissionComposition>().single_mut(world) {
        composition.player = entity;
    }
    Ok(entity)
}

/// Tears the composition down: the world is unloaded through the residency
/// path (its entities despawned, its engine mesh assets released), the
/// player body and every delivered item entity are despawned, the
/// composition's resources are removed and the world's load expectation is
/// cleared. A second [`build_headless`] or [`build_windowed`] in the same
/// process therefore starts from nothing.
///
/// No mission-bound audio source is started at this stage, so none is
/// stopped here; the sound archives travel on the stage record for the
/// mission-host stage (#1217).
pub fn teardown(app: &mut App) {
    let _ = unload_world(app);
    let world = app.world_mut();
    let leftovers: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, (
            Or<(
                With<MissionPlayerBody>,
                With<PlaytestAircraft>,
                With<LoadedItemBinding>,
            )>,
        )>();
        query.iter(world).collect()
    };
    for entity in leftovers {
        if let Ok(target) = world.get_entity_mut(entity) {
            target.despawn();
        }
    }
    world.remove_resource::<MissionComposition>();
    world.remove_resource::<MissionPlayerStart>();
    world.remove_resource::<AircraftSpawner>();
    world.remove_resource::<ExpectedLoad>();
    world.remove_resource::<MissionHost>();
    world.remove_resource::<crate::mission_session::MissionHostReport>();
}

/// Runs `cs --mission`: composes the window over the satisfied plan and
/// blocks until it closes. On exit the composition is torn down, so a
/// process that returns here leaves nothing loaded.
///
/// # Errors
///
/// [`MissionCompositionError`] when a production reader refuses a record,
/// the load fails before interactivity, the world will not load, or the
/// window exits with an error — each carrying the refusing stage's own
/// message, so the caller's nonzero exit is never without diagnostics.
pub fn run_windowed(
    install_root: &Path,
    plan: &MissionLaunchPlan,
) -> Result<(), MissionCompositionError> {
    let stage = stage_for(install_root, plan)?;
    let mut app = build_windowed(&stage)?;
    println!(
        "mission composition: {} — {}, world {}",
        plan.label.as_str(),
        stage.label,
        plan.world_id
    );
    println!(
        "mission flight: record {} provenance {} verified_original=false",
        stage.flight.record, stage.flight.provenance
    );
    let exit = app.run();
    teardown(&mut app);
    match exit {
        AppExit::Success => Ok(()),
        AppExit::Error(code) => Err(MissionCompositionError::Exit(format!("exit code {code}"))),
    }
}
