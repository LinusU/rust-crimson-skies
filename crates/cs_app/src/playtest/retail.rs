//! The original-assets free flight (`cs --playtest --cs-path <dir>`, task #649).
//!
//! The synthetic playtest's input → F24 flight → Avian → chase-camera loop with
//! its scene swapped for the content `PLAYTEST-RETAIL-SCENE` (#648) prepared:
//! one documented area of the original `c1c` world, spawned as world records
//! whose colliders are derived from the triangles they draw, and the whole intact
//! original `bloodhawk` airframe (every mesh binding of one LOD band, plus a static
//! propeller) as the player aircraft's visual.
//!
//! Nothing here is `verified_original`, and the label says so
//! ([`RETAIL_LABEL`]). The airframe's tuning is still the synthetic fixed-wing,
//! the aircraft's collider is one box measured from the composed extent of the
//! whole drawn set, the propeller is static, the spawn is [`crate::playtest_retail::spawn_pose`]'s
//! designed fraction of the area's extent, and the area has no ground, so flying
//! off the area keeps falling or flying until `R`. Every one of those is a
//! development choice recorded in `docs/PLAYTEST.md`.
//!
//! An explicit `--cs-path` that cannot be read **fails** with the reason; it never
//! falls back to the synthetic scene.

use std::path::PathBuf;

use avian3d::prelude::Collider;
use bevy::asset::{AssetApp, Assets};
use bevy::prelude::{App, Component, Entity, Handle, Quat, Resource, StandardMaterial};

use super::PlaytestError;
use crate::playtest_retail::{
    self as scene_source, AircraftPartAsset, PLAYTEST_AIRCRAFT_ROOT_NAME, PLAYTEST_WORLD_GROUP,
    PlaytestAircraftReport, PlaytestAreaReport, PlaytestConfig, PlaytestSources,
};

/// The label every surface of the original-assets playtest shows.
pub const RETAIL_LABEL: &str = scene_source::PLAYTEST_LABEL;

/// The only world with a documented area: `ZBD/C1C/gamez.zbd`.
pub const DEFAULT_WORLD: &str = "c1c";

/// The only aircraft with a documented original mesh: the `bloodhawk` airframe
/// of `ZBD/planes.zbd`, and the default.
pub const DEFAULT_AIRCRAFT: &str = "bloodhawk";

/// The worlds `--world` accepts.
pub const DOCUMENTED_WORLDS: &[&str] = &[DEFAULT_WORLD];

/// The aircraft ids `--aircraft` accepts.
pub const DOCUMENTED_AIRCRAFT: &[&str] = &[DEFAULT_AIRCRAFT];

/// What `cs --playtest --cs-path <dir>` was asked to load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetailRequest {
    /// The read-only original installation.
    pub cs_path: PathBuf,
    /// The world id (lower case), one of [`DOCUMENTED_WORLDS`].
    pub world: String,
    /// The aircraft id (lower case), one of [`DOCUMENTED_AIRCRAFT`].
    pub aircraft: String,
}

impl RetailRequest {
    /// Validates the optional selectors against the documented ids.
    ///
    /// # Errors
    ///
    /// The reason, naming the flag and the documented values, when a selector is
    /// not documented: an unknown id is refused, never mapped to a default.
    pub fn new(
        cs_path: PathBuf,
        world: Option<&str>,
        aircraft: Option<&str>,
    ) -> Result<Self, String> {
        let pick = |flag: &str, value: Option<&str>, default: &str, documented: &[&str]| {
            let value = value.unwrap_or(default).to_ascii_lowercase();
            if documented.contains(&value.as_str()) {
                Ok(value)
            } else {
                Err(format!(
                    "{flag} {value:?} has no documented original content; documented: {}",
                    documented.join(", ")
                ))
            }
        };
        Ok(Self {
            cs_path,
            world: pick("--world", world, DEFAULT_WORLD, DOCUMENTED_WORLDS)?,
            aircraft: pick(
                "--aircraft",
                aircraft,
                DEFAULT_AIRCRAFT,
                DOCUMENTED_AIRCRAFT,
            )?,
        })
    }
}

/// Marks every entity of the original area (the drawn node and the collider
/// body); a contact with one is the observable collision.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct PlaytestAreaBody;

/// The original content the playtest is flying over, and what it was read from.
#[derive(Resource)]
pub struct RetailContent {
    /// The installation fingerprint production discovery measured.
    pub installation: String,
    /// `(container key, sha-256)` of each container the scene was read from.
    pub containers: Vec<(String, String)>,
    /// What the area read and spawned.
    pub area: PlaytestAreaReport,
    /// What the aircraft read.
    pub aircraft: PlaytestAircraftReport,
    /// The entities of the area (drawn nodes and collider bodies).
    pub area_entities: Vec<Entity>,
    /// The designed spawn position, metres.
    pub spawn_m: [f32; 3],
    /// Half extents of the aircraft's collider: half the composed extent of the
    /// drawn set.
    pub half_extents_m: [f32; 3],
    /// The half turn that maps the stored nose onto the flight body's forward.
    pub visual_rotation: Quat,
    /// The aircraft's engine meshes, one per drawn binding, each with its composed
    /// placement in the airframe.
    pub parts: Vec<AircraftPartAsset>,
    /// The development material the aircraft is drawn with.
    pub material: Handle<StandardMaterial>,
}

impl RetailContent {
    /// The source manifest as one JSON object: which installation and which
    /// containers (by digest) the flown content came from.
    #[must_use]
    pub fn manifest_json(&self) -> String {
        let containers = self
            .containers
            .iter()
            .map(|(key, sha)| format!("{{\"container\":\"{key}\",\"sha256\":\"{sha}\"}}"))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"label\":\"{RETAIL_LABEL}\",\"installation\":\"{}\",\"containers\":[{containers}],\
\"area_node\":\"{}\",\"area_mesh_records\":{},\"area_triangles\":{},\"area_colliders\":{},\
{}}}",
            self.installation,
            self.area.node_name,
            self.area.mesh_records,
            self.area.triangles,
            self.area.colliders(),
            self.aircraft.json_fields(),
        )
    }
}

/// Reads the two containers the scene needs out of the installation.
///
/// # Errors
///
/// [`PlaytestError::Retail`] when the installation is missing, is not one, or
/// lacks or cannot decode a required container. No fallback exists.
pub fn read_sources(request: &RetailRequest) -> Result<PlaytestSources, PlaytestError> {
    debug_assert_eq!(request.world, PLAYTEST_WORLD_GROUP.to_ascii_lowercase());
    debug_assert_eq!(request.aircraft, PLAYTEST_AIRCRAFT_ROOT_NAME);
    scene_source::read_playtest_sources(&request.cs_path, PLAYTEST_WORLD_GROUP).map_err(|source| {
        PlaytestError::Retail {
            path: request.cs_path.clone(),
            source: Box::new(source),
        }
    })
}

/// Spawns the original area into `app` and records it as [`RetailContent`].
///
/// Call after the playtest plugins are added and before the app runs; the
/// playtest's Startup then spawns the player aircraft over this content.
///
/// # Errors
///
/// [`PlaytestError::Retail`] when the pinned area or aircraft is not what the
/// container holds or will not build.
pub fn install(
    app: &mut App,
    sources: &PlaytestSources,
    request: &RetailRequest,
) -> Result<(), PlaytestError> {
    // The headless composition has no PBR plugin; the asset collection is all
    // the development material needs.
    if !app.world().contains_resource::<Assets<StandardMaterial>>() {
        app.init_asset::<StandardMaterial>();
    }
    let config = PlaytestConfig::documented();
    let content =
        scene_source::spawn_playtest_content(app, sources, &config).map_err(|source| {
            PlaytestError::Retail {
                path: request.cs_path.clone(),
                source: Box::new(source),
            }
        })?;
    let mut area_entities = Vec::new();
    for object in content.spawned.objects() {
        area_entities.extend(object.entities());
    }
    for entity in &area_entities {
        app.world_mut().entity_mut(*entity).insert(PlaytestAreaBody);
    }
    let extent = content.aircraft.extent_m;
    let half_extents_m = extent.map(|side| ((side / 2.0) as f32).max(0.25));
    let containers = [sources.world(), sources.aircraft()]
        .iter()
        .map(|c| {
            (
                c.container_key().to_owned(),
                c.container_sha256().to_owned(),
            )
        })
        .collect();
    let mut state = app.world_mut().resource_mut::<super::PlaytestState>();
    state.spawn_m = content.spawn;
    state.label = RETAIL_LABEL;
    app.insert_resource(RetailContent {
        installation: sources.installation().to_owned(),
        containers,
        area: content.report,
        aircraft: content.aircraft,
        area_entities,
        spawn_m: content.spawn,
        half_extents_m,
        visual_rotation: content.rotation,
        parts: content.aircraft_parts,
        material: content.aircraft_material,
    });
    Ok(())
}

/// How many of the area's entities carry a derived [`Collider`] right now.
pub fn area_colliders(world: &mut bevy::prelude::World) -> usize {
    world
        .query_filtered::<(), (
            bevy::prelude::With<PlaytestAreaBody>,
            bevy::prelude::With<Collider>,
        )>()
        .iter(world)
        .count()
}
