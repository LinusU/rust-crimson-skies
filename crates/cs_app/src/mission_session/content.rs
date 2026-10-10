//! The preparation itself: [`MissionContent`] and its one `pub` entry,
//! [`MissionContent::prepare`]. See the [module documentation](super) for the
//! rule this stage is built on.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use cs_assets::install::{self, Discovery};
use cs_content::coordinates::{CoordinateSource, SourceAdapter};
use cs_content::original_airframe::{import_retail_airframe, import_retail_globals};
use cs_content::textures::WorldTextureLoad;
use cs_content::world::{ImportedWorld, WorldId, WorldInstance, WorldPopulation};
use cs_formats::zbd::{ZbdProbe, dispatch};
use cs_script::ir::MissionProgram;
use cs_sim::flight::original::PROVENANCE_LABEL;
use cs_sim::flight::{OriginalAirframe, OriginalFlightModel, OriginalGlobals};
use cs_sim::time::TickRate;
use cs_types::content::{Origin, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::install::InstallFileRecord;

use crate::animation::mission::{MissionAnimationBinding, bind_mission_animation};
use crate::environment::{EnvironmentSession, RunSeeds, read_mission_weather};
use crate::mission_launch::MissionLaunchPlan;
use crate::mission_start::{MissionStartConfiguration, recover_retail_start_configuration};
use crate::mission_world_actors::{
    CarrierRead, MissionWorldActors, SESSION_TICKS_PER_SECOND, bind_mission_world_actors,
};
use crate::objectives::{LoweredObjectives, lower_program, recover_retail_objectives};
use crate::physics::BASELINE_FIXED_HZ;
use crate::world::{WorldMeshes, retail};

/// Row 5 of the measured airframe table: the scene root the campaign start
/// recovery binds for M01, and the `vehicle.zrd` record that row maps to.
///
/// Both halves come from
/// `docs/findings/2026-10-06-m01-lc-player-airframe-source.md`, whose table
/// spells row 5 `Devastator | player_pfighter | piratefighter |
/// pdevastator …`, and from `crate::mission_start::CAMPAIGN_AIRFRAME_ROW`,
/// which is the row the engine-state byte selects. The pair is a table
/// lookup, never a name match: a resolved airframe that is not
/// [`CAMPAIGN_FLIGHT_ROW`]'s scene root is refused by name rather than
/// mapped onto the nearest record.
const CAMPAIGN_FLIGHT_ROW: (&str, &str) = ("player_pfighter", "pdevastator");

/// How many bytes an archive header probe reads before classification — the
/// same bounded prefix the launch closure classifies sound archives with, so
/// a 128 MiB sound container is never read whole just to learn its family.
const ARCHIVE_PROBE_BYTES: usize = 65_536;

/// The claim the load record's still-unmeasured variant is filed under.
const VARIANT_CLAIM: &str = "vs-m01-rt-content.world-variant-unmeasured";

/// Derives the environment session's root seed from the installation
/// fingerprint the plan was bound under.
///
/// A seed is a run parameter, not a recovered value — no original run ever
/// recorded one — so this derives it from production discovery instead of
/// declaring a magic number: the same installation always replays the same
/// streams, and nothing here invents a value out of the content.
#[must_use]
pub fn root_seed_from(install_sha256: &str) -> u64 {
    install_sha256
        .as_bytes()
        .iter()
        .take(8)
        .fold(0_u64, |acc, byte| (acc << 8) | u64::from(*byte))
}

/// One sound-family archive in the mission's scope, by logical key and the
/// installation's own digest of its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundArchive {
    /// The archive's installation-relative logical key.
    pub key: String,
    /// The installation's SHA-256 of the container bytes.
    pub sha256: String,
}

/// The original flight law the mission launches its player with, and what
/// that law was read from.
///
/// This is the record chain [`crate::playtest::retail::read_flight`] runs for
/// the playtest's documented airframe, applied to the campaign airframe M01's
/// start configuration resolves. `provenance` is the law's own label: static
/// analysis of the owner's decrypted image, never `verified_original`.
#[derive(Clone, Debug)]
pub struct MissionFlight {
    /// The statically recovered law with this record's imported parameters.
    pub model: OriginalFlightModel,
    /// The `vehicle.zrd` record the parameters came from (`pdevastator`).
    pub record: String,
    /// The imported player fuel load, in the law's own fuel units.
    pub fuel: f64,
    /// The record's `kind_of` chain, ancestor first.
    pub inheritance_chain: Vec<String>,
    /// The provenance label of the law and of every parameter it consumes.
    pub provenance: &'static str,
}

/// Every record the windowed mission composition consumes, read through the
/// production readers before a window, an app or an entity exists.
///
/// Build it with [`MissionContent::prepare`]; nothing else constructs one, so
/// a composition can never start from a partly filled value.
#[derive(Debug)]
pub struct MissionContent {
    /// The world container imported into the definition the runtime spawns.
    pub world: ImportedWorld,
    /// The engine meshes the definition names, uploaded through the F17-B
    /// adapter.
    pub meshes: WorldMeshes,
    /// This mission's load record: population, initial damage and provenance.
    pub instance: WorldInstance,
    /// The player's airframe and initial pose, as the `aiv.zrd` records and
    /// the measured engine-state source resolve them.
    pub start: MissionStartConfiguration,
    /// The flight law the player's airframe maps to.
    pub flight: MissionFlight,
    /// The bound weather running on this session's timeline and seeds.
    pub environment: EnvironmentSession,
    /// The scope's world actors, with the session the composition spawns from.
    pub world_actors: MissionWorldActors,
    /// The scope's animation join: startup rows, placements and carriers.
    pub animation: MissionAnimationBinding,
    /// The mission's authored objectives, lowered for the objective session.
    pub objectives: LoweredObjectives,
    /// The lowered control program the script host runs.
    pub control: MissionProgram,
    /// The sound-family archives in the mission's scope.
    pub sound_archives: Vec<SoundArchive>,
}

/// Why a mission's content could not be prepared.
///
/// Every variant carries the refusing reader's own message and the source
/// path or logical key it refused at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MissionSessionError {
    /// Production discovery could not inventory the installation.
    Discovery {
        /// The installation root the caller passed.
        path: String,
        /// The discovery reader's own reason.
        reason: String,
    },
    /// The world container could not be read or imported.
    World {
        /// The container's logical key.
        key: String,
        /// The reader's own reason.
        reason: String,
    },
    /// The mission's start configuration could not be read.
    Start {
        /// The mission directory key.
        mission: String,
        /// The start reader's own message (it names the archive itself).
        reason: String,
    },
    /// The start configuration read, but a field it must answer stayed
    /// unknown.
    StartUnresolved {
        /// The mission directory key.
        mission: String,
        /// Which field stayed unknown, spelled out.
        field: &'static str,
        /// The configuration's own reason for the unknown.
        reason: String,
    },
    /// The start configuration resolved an airframe the measured row does not
    /// map, so no flight record was picked for it.
    AirframeUnmapped {
        /// The airframe the configuration resolved, as its content key.
        airframe: String,
        /// The scene root the measured row maps.
        expected: &'static str,
    },
    /// The flight record could not be imported or was refused.
    Flight {
        /// The `vehicle.zrd` record the import was asked for.
        record: String,
        /// The importer's own reason.
        reason: String,
    },
    /// The mission's weather could not be read or bound.
    Weather {
        /// The mission directory key.
        mission: String,
        /// The weather reader's own message.
        reason: String,
    },
    /// The weather read, but its session could not start.
    Environment {
        /// The mission directory key.
        mission: String,
        /// The session's own refusal.
        reason: String,
    },
    /// The world-actor binding did not produce a launchable session.
    WorldActors {
        /// The reader archive the carrier lives in.
        archive: String,
        /// The binding's own detail, verbatim.
        detail: String,
    },
    /// The scope's animation join refused.
    Animation {
        /// The mission scope.
        scope: String,
        /// The animation reader's own message.
        reason: String,
    },
    /// The objectives could not be recovered or would not lower.
    Objectives {
        /// The mission directory key.
        mission: String,
        /// The recovery's or the lowering's own message.
        reason: String,
    },
    /// The control program could not be surveyed, has no row, or its lowering
    /// is incomplete.
    Control {
        /// The mission directory key.
        mission: String,
        /// The census's or the lowering's own message.
        reason: String,
    },
    /// No sound-family archive could be classified in the mission's scope.
    SoundArchives {
        /// The scope that was walked.
        scope: String,
        /// What the walk found instead.
        reason: String,
    },
    /// The load record's variant could not be declared.
    Variant {
        /// The world the load reads from.
        world: String,
        /// Why.
        reason: String,
    },
    /// A claim id or provenance could not be built.
    Provenance {
        /// The claim the build failed for.
        claim: &'static str,
        /// The refusal itself.
        reason: String,
    },
}

impl fmt::Display for MissionSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery { path, reason } => {
                write!(formatter, "{path}: installation discovery failed: {reason}")
            }
            Self::World { key, reason } => write!(formatter, "{key}: {reason}"),
            Self::Start { mission, reason } => {
                write!(
                    formatter,
                    "{mission}: the start configuration refuses: {reason}"
                )
            }
            Self::StartUnresolved {
                mission,
                field,
                reason,
            } => write!(
                formatter,
                "{mission}: the start configuration leaves {field} unknown: {reason}"
            ),
            Self::AirframeUnmapped { airframe, expected } => write!(
                formatter,
                "the start configuration resolves the airframe {airframe}, which is not the \
                 measured row {expected}; no flight record was picked for it \
                 (docs/findings/2026-10-06-m01-lc-player-airframe-source.md)"
            ),
            Self::Flight { record, reason } => write!(formatter, "{record}: {reason}"),
            Self::Weather { mission, reason } => {
                write!(formatter, "{mission}: the weather refuses: {reason}")
            }
            Self::Environment { mission, reason } => {
                write!(
                    formatter,
                    "{mission}: the environment session refuses: {reason}"
                )
            }
            Self::WorldActors { archive, detail } => {
                write!(formatter, "{archive}: {detail}")
            }
            Self::Animation { scope, reason } => {
                write!(formatter, "{scope}: the animation join refuses: {reason}")
            }
            Self::Objectives { mission, reason } => {
                write!(formatter, "{mission}: the objectives refuse: {reason}")
            }
            Self::Control { mission, reason } => {
                write!(
                    formatter,
                    "{mission}: the control program refuses: {reason}"
                )
            }
            Self::SoundArchives { scope, reason } => {
                write!(formatter, "{scope}: no sound archive: {reason}")
            }
            Self::Variant { world, reason } => {
                write!(
                    formatter,
                    "{world}: the load record's variant refuses: {reason}"
                )
            }
            Self::Provenance { claim, reason } => {
                write!(formatter, "provenance {claim}: {reason}")
            }
        }
    }
}

impl std::error::Error for MissionSessionError {}

impl MissionContent {
    /// Reads every record the mission composition consumes out of
    /// `install_root`, through the production readers.
    ///
    /// `plan` is the measured closure for this mission; the caller gates on
    /// [`crate::mission_launch::MissionLaunchPlan::launchable`] before it
    /// calls here, so this function reads content and does not re-judge the
    /// surfaces.
    ///
    /// # Errors
    ///
    /// [`MissionSessionError`] naming the refusing reader and its source path
    /// or logical key, for every record the readers would not stand behind.
    pub fn prepare(
        install_root: &Path,
        plan: &MissionLaunchPlan,
    ) -> Result<Self, MissionSessionError> {
        // Production discovery first: every reader below inventories the
        // installation itself, and a root that cannot be inventoried is
        // refused here with the path the caller passed.
        let found: Discovery =
            install::discover(install_root).map_err(|error| MissionSessionError::Discovery {
                path: install_root.display().to_string(),
                reason: error.to_string(),
            })?;

        let (world, meshes, instance) = prepare_world(install_root, plan)?;
        let start = prepare_start(install_root, plan)?;
        let flight = prepare_flight(install_root, &start)?;
        let environment = prepare_environment(plan, &found)?;
        let world_actors = prepare_world_actors(install_root, &found, plan)?;
        let animation =
            bind_mission_animation(install_root, &plan.mission_dir).map_err(|error| {
                MissionSessionError::Animation {
                    scope: plan.mission_dir.clone(),
                    reason: error.to_string(),
                }
            })?;
        let control = prepare_control(install_root, plan)?;
        let sound_archives = prepare_sound_archives(install_root, &found, plan)?;
        // The objective recovery is asked **last**: `ObjectiveRecovery::program`
        // refuses for every original mission today (its own doc says "always
        // today"), so reading it last keeps that refusal from masking a reader
        // behind it — an `Objectives` refusal means every other record of this
        // list is already in hand.
        let objectives = prepare_objectives(install_root, plan)?;

        Ok(Self {
            world,
            meshes,
            instance,
            start,
            flight,
            environment,
            world_actors,
            animation,
            objectives,
            control,
            sound_archives,
        })
    }

    /// The environment session's tick rate: the fixed tick the composition
    /// flies on.
    #[must_use]
    pub fn tick_rate() -> TickRate {
        TickRate::new(BASELINE_FIXED_HZ).expect("the baseline fixed rate is nonzero")
    }
}

/// The module's `pub` entry: the composition's one call into this stage.
///
/// It exists so a caller (and a test) reaches preparation through a plain
/// function rather than through the type's constructor, and so
/// [`MissionContent::prepare`] has a production caller inside this crate.
///
/// # Errors
///
/// [`MissionContent::prepare`]'s refusals, unchanged.
pub fn prepare_mission_content(
    install_root: &Path,
    plan: &MissionLaunchPlan,
) -> Result<MissionContent, MissionSessionError> {
    MissionContent::prepare(install_root, plan)
}

/// World: the container, its import and this mission's load record.
fn prepare_world(
    install_root: &Path,
    plan: &MissionLaunchPlan,
) -> Result<(ImportedWorld, WorldMeshes, WorldInstance), MissionSessionError> {
    let group = plan.group_dir.trim_start_matches("zbd/");
    let key = format!("{}/gamez.zbd", plan.group_dir);
    let container =
        retail::read_world_container(install_root, group, &WorldTextureLoad::project_default())
            .map_err(|error| MissionSessionError::World {
                key: key.clone(),
                reason: error.to_string(),
            })?;

    // The measured coordinate source and origin the launch closure imports
    // with: a retail GameZ container's own span decides both.
    let adapter = SourceAdapter::new(CoordinateSource::retail_gamez(container.span().clone()));
    let origin = Origin::Installation {
        source: container.span().clone(),
    };
    let imported =
        container
            .definition(origin, &adapter)
            .map_err(|error| MissionSessionError::World {
                key: key.clone(),
                reason: error.to_string(),
            })?;
    let meshes = container
        .uploaded_meshes(imported.definition())
        .map_err(|error| MissionSessionError::World {
            key: key.clone(),
            reason: error.to_string(),
        })?;

    let world_id: WorldId = container
        .world()
        .map_err(|error| MissionSessionError::World {
            key: key.clone(),
            reason: error.to_string(),
        })?;
    let provenance = container
        .provenance()
        .map_err(|error| MissionSessionError::World {
            key: key.clone(),
            reason: error.to_string(),
        })?;

    // Population: the documented all-objects default. M01's binding states
    // no object population of its own (`missions/bindings/M01.json` keeps
    // "actor/spawn/route sets" unbound), so every authored object is
    // activated — never a guessed subset.
    //
    // Variant: no measured mission record names a world variant for this
    // load, so the record carries the explicit unknown and its claim instead
    // of picking one (IDENTITY-CONTENT: an unknown states itself).
    let variant = Resolved::unknown(
        ClaimId::new(VARIANT_CLAIM).map_err(|error| MissionSessionError::Provenance {
            claim: VARIANT_CLAIM,
            reason: error.to_string(),
        })?,
        "no measured mission record names a world variant for this load",
    )
    .map_err(|error| MissionSessionError::Variant {
        world: world_id.key().to_owned(),
        reason: error.to_string(),
    })?;
    let instance = WorldInstance::try_new(
        world_id,
        variant,
        WorldPopulation::AllAuthored,
        BTreeSet::new(),
        provenance,
    )
    .map_err(|error| MissionSessionError::Variant {
        world: key,
        reason: error.to_string(),
    })?;
    Ok((imported, meshes, instance))
}

/// Player start: the airframe and the metric initial pose, both known.
fn prepare_start(
    install_root: &Path,
    plan: &MissionLaunchPlan,
) -> Result<MissionStartConfiguration, MissionSessionError> {
    let start =
        recover_retail_start_configuration(install_root, &plan.mission_dir).map_err(|error| {
            MissionSessionError::Start {
                mission: plan.mission_dir.clone(),
                reason: error.to_string(),
            }
        })?;
    match start.airframe() {
        Resolved::Known(_) => {}
        Resolved::Unknown { reason, .. } => {
            return Err(MissionSessionError::StartUnresolved {
                mission: plan.mission_dir.clone(),
                field: "the player's airframe",
                reason: reason.clone(),
            });
        }
    }
    match start.initial_pose() {
        Resolved::Known(_) => {}
        Resolved::Unknown { reason, .. } => {
            return Err(MissionSessionError::StartUnresolved {
                mission: plan.mission_dir.clone(),
                field: "the player's initial pose",
                reason: reason.clone(),
            });
        }
    }
    Ok(start)
}

/// Player flight law: the record the measured campaign airframe row maps to.
fn prepare_flight(
    install_root: &Path,
    start: &MissionStartConfiguration,
) -> Result<MissionFlight, MissionSessionError> {
    let airframe_key = match start.airframe() {
        Resolved::Known(known) => known.value.key().to_owned(),
        Resolved::Unknown { reason, .. } => {
            return Err(MissionSessionError::StartUnresolved {
                mission: start.mission().to_owned(),
                field: "the player's airframe",
                reason: reason.clone(),
            });
        }
    };
    if airframe_key != CAMPAIGN_FLIGHT_ROW.0 {
        return Err(MissionSessionError::AirframeUnmapped {
            airframe: airframe_key,
            expected: CAMPAIGN_FLIGHT_ROW.0,
        });
    }
    let record = CAMPAIGN_FLIGHT_ROW.1;
    let refused = |reason: String| MissionSessionError::Flight {
        record: record.to_owned(),
        reason,
    };
    let parameters = import_retail_airframe(install_root, record)
        .map_err(|error| refused(format!("the parameters could not be imported: {error}")))?;
    let law = import_retail_globals(install_root)
        .map_err(|error| refused(format!("the flight globals could not be imported: {error}")))?;
    let airframe = OriginalAirframe::from_values(&parameters.field_values())
        .map_err(|error| refused(format!("the parameters were refused: {error}")))?;
    let globals = OriginalGlobals::from_values(&law.law_values())
        .map_err(|error| refused(format!("the flight globals were refused: {error}")))?;
    let fuel = match &parameters.initial_fuel {
        Resolved::Known(known) => known.value,
        Resolved::Unknown { reason, .. } => {
            // An explicit unknown is never replaced by a number.
            return Err(refused(format!(
                "the {record} chain states no fuel load ({reason})"
            )));
        }
    };
    Ok(MissionFlight {
        model: OriginalFlightModel::full(airframe, globals),
        record: record.to_owned(),
        fuel,
        inheritance_chain: parameters.inheritance_chain.clone(),
        provenance: PROVENANCE_LABEL,
    })
}

/// Environment: the bound weather, started on the composition's tick.
fn prepare_environment(
    plan: &MissionLaunchPlan,
    found: &Discovery,
) -> Result<EnvironmentSession, MissionSessionError> {
    let weather = read_mission_weather(found, &plan.mission_dir).map_err(|error| {
        MissionSessionError::Weather {
            mission: plan.mission_dir.clone(),
            reason: error.to_string(),
        }
    })?;
    let seeds = root_seed_from(&plan.install_sha256);
    weather
        .session(MissionContent::tick_rate(), RunSeeds::from_root(seeds))
        .map_err(|error| MissionSessionError::Environment {
            mission: plan.mission_dir.clone(),
            reason: error.to_string(),
        })
}

/// World actors: the binding the composition spawns from, kept whole.
fn prepare_world_actors(
    install_root: &Path,
    found: &Discovery,
    plan: &MissionLaunchPlan,
) -> Result<MissionWorldActors, MissionSessionError> {
    let actors = bind_mission_world_actors(
        install_root,
        found,
        &plan.mission_dir,
        &plan.group_dir,
        &plan.catalog_id,
        SESSION_TICKS_PER_SECOND,
    );
    // A scope whose archive declares no carrier places no actors: that is
    // what the launch closure measures as a satisfied surface, so it is not
    // a refusal here either.
    if matches!(actors.carrier(), CarrierRead::Absent) || actors.is_satisfied() {
        return Ok(actors);
    }
    let detail = match actors.carrier() {
        CarrierRead::Unreadable(reason) => {
            format!("the carrier cannot be read: {reason}")
        }
        CarrierRead::Refused(reason) => format!("the carrier refuses to decode: {reason}"),
        _ => world_actor_refusal(&actors),
    };
    Err(MissionSessionError::WorldActors {
        archive: actors.archive().to_owned(),
        detail,
    })
}

/// The refusal detail for a binding that decoded but never launched: the
/// production lowering's and the session launch's own words, then every field
/// still open with its claim id.
fn world_actor_refusal(actors: &MissionWorldActors) -> String {
    let mut detail = String::from("the world-actor program does not launch");
    if let Some(error) = actors.lower_error() {
        detail.push_str(&format!(": the production lowering refuses with {error}"));
    }
    if let Some(error) = actors.launch_error() {
        detail.push_str(&format!("; the session launch refuses with {error}"));
    }
    if !actors.open_fields().is_empty() {
        let open: Vec<String> = actors
            .open_fields()
            .iter()
            .map(|field| {
                format!(
                    "{}{} ({})",
                    field
                        .actor
                        .as_ref()
                        .map(|actor| format!("{actor}."))
                        .unwrap_or_default(),
                    field.field,
                    field.claim_id.as_str()
                )
            })
            .collect();
        detail.push_str(&format!("; still open: {}", open.join(", ")));
    }
    detail
}

/// Objectives: the declared program the recovery yields, lowered.
fn prepare_objectives(
    install_root: &Path,
    plan: &MissionLaunchPlan,
) -> Result<LoweredObjectives, MissionSessionError> {
    let recovery = recover_retail_objectives(install_root, &plan.mission_dir).map_err(|error| {
        MissionSessionError::Objectives {
            mission: plan.mission_dir.clone(),
            reason: error.to_string(),
        }
    })?;
    let program = recovery
        .program()
        .map_err(|error| MissionSessionError::Objectives {
            mission: plan.mission_dir.clone(),
            reason: error.to_string(),
        })?;
    lower_program(&program).map_err(|error| MissionSessionError::Objectives {
        mission: plan.mission_dir.clone(),
        reason: error.to_string(),
    })
}

/// Script host: the mission's control row, its complete lowering and the
/// program that lowering produced.
fn prepare_control(
    install_root: &Path,
    plan: &MissionLaunchPlan,
) -> Result<MissionProgram, MissionSessionError> {
    let refused = |reason: String| MissionSessionError::Control {
        mission: plan.mission_dir.clone(),
        reason,
    };
    let census = crate::mission_control::survey_mission_control_programs(install_root)
        .map_err(|error| refused(format!("the control census refuses: {error}")))?;
    let row = census
        .row(&plan.mission_dir)
        .ok_or_else(|| refused("the census has no row for this mission".to_owned()))?;
    let lowering = row
        .lowering()
        .ok_or_else(|| refused("the reader archive declares no control member".to_owned()))?;
    if !lowering.complete() {
        let unmet: Vec<String> = lowering.unmet().map(|row| row.label()).collect();
        return Err(refused(format!(
            "the lowering is incomplete: unmet requirements [{}]; directive meanings still \
             unmeasured: {}",
            unmet.join(", "),
            lowering.unmeasured_fields().join(", ")
        )));
    }
    let program = row
        .lowering_attempt()
        .and_then(|attempt| attempt.program())
        .ok_or_else(|| refused("the complete lowering produced no program".to_owned()))?;
    Ok(program.clone())
}

/// Audio: every sound-family archive in the mission's scope.
///
/// The scope is the walk the launch closure performs — the mission directory,
/// the world group's directory and `zbd/` itself — and the classification is
/// `cs_formats::zbd::dispatch` over a bounded header probe, so this lists the
/// same archives the `mission_audio` surface judged without reading one whole.
fn prepare_sound_archives(
    install_root: &Path,
    found: &Discovery,
    plan: &MissionLaunchPlan,
) -> Result<Vec<SoundArchive>, MissionSessionError> {
    let scope = format!("{} (and {})", plan.mission_dir, plan.group_dir);
    let mut archives = Vec::new();
    let mut unreadable = Vec::new();
    for record in &found.manifest.files {
        let key = record.relative_spelling.logical_key();
        let in_scope = key.starts_with(&format!("{}/", plan.mission_dir))
            || key.starts_with(&format!("{}/", plan.group_dir))
            || (key.starts_with("zbd/") && key.matches('/').count() == 1);
        if !(in_scope && key.ends_with(".zbd")) {
            continue;
        }
        match archive_family(install_root, record, &key) {
            Ok(family) if family.eq_ignore_ascii_case("sound") => archives.push(SoundArchive {
                key: key.to_owned(),
                sha256: record.sha256.to_hex(),
            }),
            Ok(_) => {}
            Err(reason) => unreadable.push(format!("{key}: {reason}")),
        }
    }
    if archives.is_empty() {
        let reason = if unreadable.is_empty() {
            "the scope holds no sound-family archive".to_owned()
        } else {
            format!("no archive could be classified ({})", unreadable.join("; "))
        };
        return Err(MissionSessionError::SoundArchives { scope, reason });
    }
    archives.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(archives)
}

/// Classifies one container by dispatching over a bounded header probe.
fn archive_family(
    install_root: &Path,
    record: &InstallFileRecord,
    key: &str,
) -> Result<String, String> {
    use std::io::Read;

    let host: PathBuf = install_root.join(record.relative_spelling.as_str());
    let header = std::fs::File::open(&host)
        .and_then(|mut file| {
            let mut header = vec![0_u8; ARCHIVE_PROBE_BYTES];
            let read = file.read(&mut header)?;
            header.truncate(read);
            Ok(header)
        })
        .map_err(|error| format!("cannot read {}: {error}", host.display()))?;
    let decision = dispatch(ZbdProbe::new(key, &record.relative_spelling, &header))
        .map_err(|error| format!("dispatch refused: {error}"))?;
    Ok(format!("{:?}", decision.family()))
}
