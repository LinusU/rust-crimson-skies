//! The original-assets free flight (`cs --playtest --cs-path <dir>`, task #649).
//!
//! The synthetic playtest's input → F24 flight → Avian → chase-camera loop with
//! its scene swapped for the content `PLAYTEST-RETAIL-SCENE` (#648) prepared:
//! one documented area of the original `c1c` world, spawned as world records
//! whose colliders are derived from the triangles they draw, and the whole intact
//! original `bloodhawk` airframe (every mesh binding of one LOD band, plus the
//! propeller disc that spins with the engine) as the player aircraft's visual.
//!
//! Nothing here is `verified_original`, and the label says so
//! ([`RETAIL_LABEL`]). The flight law is the original 2000 PC game's own,
//! recovered statically under **`OWNER-STATIC-2026-10-08`** (#796) and flown
//! with `pbloodhawk`'s imported parameters (task #797, [`RetailFlight`]): it is
//! still *uncalibrated against an original run* (#358), so no handling of this
//! scene is `verified_original` either. The aircraft's collider is one box
//! measured from the composed extent of the whole drawn set, the drawn
//! propeller turns about a hub measured from its own geometry at a designed
//! rate (#710), the spawn is [`crate::playtest_retail::spawn_pose`]'s designed
//! fraction of the area's extent, and the area has no ground, so flying off the
//! area keeps falling or flying until `R`. Every one of those is a development
//! choice recorded in `docs/PLAYTEST.md`.
//!
//! An explicit `--cs-path` that cannot be read **fails** with the reason; it never
//! falls back to the synthetic scene.

use std::path::PathBuf;

use avian3d::prelude::Collider;
use bevy::asset::{AssetApp, Assets};
use bevy::image::Image;
use bevy::prelude::{App, Component, Entity, Quat, Resource, StandardMaterial};
use cs_content::original_airframe::{import_retail_airframe, import_retail_globals};
use cs_sim::flight::original::PROVENANCE_LABEL;
use cs_sim::flight::{OriginalAirframe, OriginalFlightModel, OriginalGlobals};
use cs_types::content::Resolved;

use super::PlaytestError;
use crate::playtest::propeller::propeller_spin_json;
use crate::playtest_retail::{
    self as scene_source, AircraftPartAsset, PLAYTEST_AIRCRAFT_ROOT_NAME, PLAYTEST_WORLD_GROUP,
    PlaytestAircraftReport, PlaytestAreaReport, PlaytestConfig, PlaytestSources, PropellerSpinSpec,
};
use crate::playtest_textures::PlaytestTextureReport;

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
    /// The nose mapping that lands the stored nose on the flight body's forward
    /// (the yaw of [`crate::playtest_retail::nose_mapping`]; the identity for the
    /// measured `−Z` stored nose).
    pub visual_rotation: Quat,
    /// The aircraft's drawn bindings, each with its composed placement in the
    /// airframe and its pieces textured from the flown world's archive.
    pub parts: Vec<AircraftPartAsset>,
    /// The drawn propeller, with the hub **measured from its own triangles**:
    /// what `spawn_aircraft` puts [`PropellerSpin`](crate::playtest::propeller::PropellerSpin)
    /// on, and what the `propeller_spin` object of [`Self::manifest_json`]
    /// reports.
    pub propeller: Option<PropellerSpinSpec>,
    /// What the area's and the aircraft's materials resolved to.
    pub textures: PlaytestTextureReport,
}

impl RetailContent {
    /// The source manifest as one JSON object: which installation and which
    /// containers (by digest) the flown content came from, what the area and
    /// the aircraft read, and the propeller spin rule with its claims.
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
{},{},{},\"textures\":{}}}",
            self.installation,
            self.area.node_name,
            self.area.mesh_records,
            self.area.triangles,
            self.area.colliders(),
            self.area.json_fields(),
            self.aircraft.json_fields(),
            propeller_spin_json(self.propeller.as_ref()),
            self.textures.json(),
        )
    }
}

/// The `vehicle.zrd` record each documented aircraft flies.
///
/// The playtest flies the **player's** aircraft, and the player records carry
/// the `p` prefix: the owner's task names `pbloodhawk` for the documented
/// `bloodhawk` airframe, and #796's measured table lists the same prefix on
/// every player fighter (`pbloodhawk`, `pdevastator`, ...). An aircraft id with
/// no entry here is refused by name rather than mapped onto a guessed record.
const FLIGHT_RECORDS: &[(&str, &str)] = &[("bloodhawk", "pbloodhawk")];

/// The speed the retail aircraft spawns and resets at, m/s.
///
/// **A declared development value, not a recovered one.** The original's
/// player spawn speed was never recovered (#796 records it as an open unknown,
/// unresolved until an original run, #358), so every start speed here is a
/// choice and says so. This is the speed the playtest scene has always started
/// its aircraft at: below the recovered law's own cruise, so a free flight
/// begins with a run-up — full throttle carries the aircraft from here to the
/// imported `fd_speed` cruise in a few seconds — and it is the speed the
/// scripted smoke's arrival at the original area's collider was written for.
///
/// What the imported record decides instead is the **cruise**: `fd_speed`
/// (135 m/s for `pbloodhawk`), measured at 134 m/s under full throttle by the
/// acceptance test and reported as `fd_speed_m_s` in the `playtest flight`
/// line. That number is data, not a choice.
pub const RETAIL_START_SPEED_M_S: f64 = 55.0;

/// The original flight law the original-assets playtest flies (task #797).
///
/// Installed beside [`RetailContent`] from the same installation, so a scene
/// that cannot state its flight parameters fails as loudly as one that cannot
/// state its geometry. Every number in [`Self::model`] was read from
/// `ZBD/zrdr.zbd` (`vehicle.zrd` + `engines.zrd` + `player.zrd`) by the
/// production importer `cs_content::original_airframe`; the law itself was
/// recovered by static analysis of the owner's decrypted image under
/// [`PROVENANCE_LABEL`] (#796).
///
/// **`OWNER-STATIC-2026-10-08` is static evidence, never `verified_original`:**
/// no original executable ran, and nothing here is calibrated against an
/// original run (#358).
#[derive(Resource, Clone, Debug)]
pub struct RetailFlight {
    /// The statically recovered law with this record's imported parameters.
    pub model: OriginalFlightModel,
    /// The `vehicle.zrd` record the parameters came from (`pbloodhawk`).
    pub record: String,
    /// The record's `kind_of` chain, ancestor first.
    pub inheritance_chain: Vec<String>,
    /// The `engines.zrd` row the record resolved, `(id, name, factor)`.
    pub engine: (u32, String, f64),
    /// `fd_speed`, m/s: the level cruise the imported parameters decide, kept
    /// for reporting (see [`Self::start_speed_m_s`] for what the aircraft
    /// actually spawns at).
    pub fd_speed_m_s: f64,
    /// The imported player fuel load, in the law's own fuel units.
    pub fuel: f64,
    /// The provenance label of the law and of every parameter it consumes.
    pub provenance: &'static str,
}

impl RetailFlight {
    /// The speed the retail aircraft spawns and resets at, in m/s.
    ///
    /// **The original's player spawn speed was not recovered** (#796 records it
    /// as an unknown, unresolved until an original run, #358), so no start
    /// speed here can be a claim about the original: this one is a declared
    /// development value ([`RETAIL_START_SPEED_M_S`]). What the record *does*
    /// decide is the cruise the aircraft settles at — the imported `fd_speed`
    /// (134 m/s measured under full throttle by the acceptance test) — and that
    /// is data, not a choice.
    #[must_use]
    pub const fn start_speed_m_s(&self) -> f64 {
        RETAIL_START_SPEED_M_S
    }

    /// The imported `fd_speed`, m/s: the level cruise these same parameters
    /// settle at, reported rather than used as a start speed.
    #[must_use]
    pub const fn fd_speed_m_s(&self) -> f64 {
        self.fd_speed_m_s
    }

    /// The flight-law statement the `playtest flight` line carries, verbatim.
    #[must_use]
    pub fn json(&self) -> String {
        let fields = &self.model.airframe;
        format!(
            "{{\"record\":\"{}\",\"chain\":[{}],\"engine\":{{\"id\":{},\"name\":\"{}\",\"factor\":{}}},\
\"fd_speed_m_s\":{},\"veh_weight\":{},\"ref_area\":{},\"gravity\":{},\"start_speed_m_s\":{},\
\"provenance\":\"{}\",\"verified_original\":false,\"calibrated_against_an_original_run\":false,\
\"level_off_toggle_wired\":false,\"level_off_resolving_task\":\"Rally #1134 \
FLIGHT-ORIGINAL-LEVELOFF-INPUT: the input layer has no slot for command 47\"}}",
            self.record,
            self.inheritance_chain
                .iter()
                .map(|name| format!("\"{name}\""))
                .collect::<Vec<_>>()
                .join(","),
            self.engine.0,
            self.engine.1,
            self.engine.2,
            fields.fd_speed,
            fields.veh_weight,
            fields.ref_area,
            fields.gravity,
            self.start_speed_m_s(),
            self.provenance,
        )
    }
}

/// Imports the original flight parameters of `request`'s aircraft.
///
/// The production importer reads the three members of `ZBD/zrdr.zbd` and the
/// law's own boundary refuses anything it cannot consume, so this fails with
/// the reason and never falls back to the synthetic airframe.
///
/// # Errors
///
/// [`PlaytestError::Flight`] naming the installation and the cause: an
/// undocumented aircraft id, an import the reader refused, a parameter record
/// the law refused, or a chain that states no fuel load (the player chain
/// does; an explicit unknown is not replaced by a number).
pub fn read_flight(request: &RetailRequest) -> Result<RetailFlight, PlaytestError> {
    let refused = |detail: String| PlaytestError::Flight {
        path: request.cs_path.clone(),
        detail,
    };
    let record = FLIGHT_RECORDS
        .iter()
        .find_map(|(aircraft, record)| (*aircraft == request.aircraft).then_some(*record))
        .ok_or_else(|| {
            refused(format!(
                "no documented flight record for the aircraft {:?}; documented: {}",
                request.aircraft,
                FLIGHT_RECORDS
                    .iter()
                    .map(|(aircraft, _)| *aircraft)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;
    let parameters = import_retail_airframe(&request.cs_path, record).map_err(|error| {
        refused(format!(
            "the {record} parameters could not be imported: {error}"
        ))
    })?;
    let globals = import_retail_globals(&request.cs_path)
        .map_err(|error| refused(format!("the flight globals could not be imported: {error}")))?;
    let airframe = OriginalAirframe::from_values(&parameters.field_values())
        .map_err(|error| refused(format!("the {record} parameters were refused: {error}")))?;
    let globals = OriginalGlobals::from_values(&globals.law_values())
        .map_err(|error| refused(format!("the flight globals were refused: {error}")))?;
    let fuel = match &parameters.initial_fuel {
        Resolved::Known(known) => known.value,
        Resolved::Unknown { reason, .. } => {
            // An explicit unknown is never replaced by a number: the player
            // chain states `fuel`, so this is a real refusal, not a default.
            return Err(refused(format!(
                "the {record} chain states no fuel load ({reason})"
            )));
        }
    };
    let fd_speed_m_s = match parameters.value("fd_speed") {
        Some(entry) => entry.value,
        None => {
            return Err(refused(format!(
                "the imported {record} record has no fd_speed"
            )));
        }
    };
    Ok(RetailFlight {
        model: OriginalFlightModel::full(airframe, globals),
        record: record.to_owned(),
        inheritance_chain: parameters.inheritance_chain.clone(),
        engine: (
            parameters.engine.id,
            parameters.engine.name.clone(),
            parameters.engine.factor,
        ),
        fd_speed_m_s,
        fuel,
        provenance: PROVENANCE_LABEL,
    })
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
/// [`PlaytestError::Flight`] when the installation's original flight
/// parameters cannot be imported (there is no fallback to the synthetic
/// airframe), and [`PlaytestError::Retail`] when the pinned area or aircraft
/// is not what the container holds or will not build.
pub fn install(
    app: &mut App,
    sources: &PlaytestSources,
    request: &RetailRequest,
) -> Result<(), PlaytestError> {
    // The flight parameters first: an installation whose flight law cannot be
    // read must fail before any of the scene exists, exactly like the geometry
    // readers, and never fall back to the synthetic airframe.
    let flight = read_flight(request)?;
    // The headless composition has no PBR plugin; the asset collection is all
    // the development material needs.
    if !app.world().contains_resource::<Assets<StandardMaterial>>() {
        app.init_asset::<StandardMaterial>();
    }
    if !app.world().contains_resource::<Assets<Image>>() {
        app.init_asset::<Image>();
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
        propeller: content.propeller,
        textures: content.textures,
    });
    app.insert_resource(flight);
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
