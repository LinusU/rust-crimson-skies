//! The retail `InstantActionCatalog` (F49-D.03).
//!
//! Spec: `specs/F49-instant-action-presets-and-custom-scenarios.md`,
//! non-negotiable 1 ("Preserve original preset identities and parameters") and
//! AC01. Finding: `docs/findings/2026-10-18-f49-d-03-retail-catalog.md`.
//!
//! The catalog is assembled from what the installation measures, through the
//! production readers, and **only** from that:
//!
//! * the preset and scenario identities, the offered mission types and the
//!   declared aircraft come from F49-D.01's `ia.zrd` survey
//!   ([`cs_content::instant_action::survey_retail_instant_action`]);
//! * a plane's display name becomes an airframe id through the executable's own
//!   table, [`crate::mission_start::airframe_entry`];
//! * the world is the directory's world group when the F14-D.3 world rows hold
//!   it; the environment is the `weather.zrd` the IA archive itself carries,
//!   read by [`crate::environment::retail::read_mission_weather`];
//! * the option table is F49-D.02's, its pilots the ace identities the survey
//!   measured ([`IaDenominator::ace_pilot_ids`], F49-D.02-FU-PILOT), validated
//!   with its remaining unmeasured dimensions kept unmeasured
//!   ([`ScenarioOptionTable::validate_partial`](cs_content::instant_action::ScenarioOptionTable::validate_partial)).
//!
//! Everything the descriptors do not say stays explicitly unmeasured with a
//! claim id and a reason: the actors' factions, loadouts and survivability, the
//! wingmen's planes, the preset's title and difficulty. The victory rules are
//! **measured**: the engine image's Instant Action end check (`0x45b9d0`)
//! binds one [`VictoryCondition`](cs_content::instant_action::VictoryCondition) per `mission_type` name
//! ([`measured_ia_victory_rules`], F49-D.03-FU-RULES-END), so each preset
//! carries `Declared` rules. The lowering ([`super::lower_preset`])
//! therefore still refuses every retail preset, naming the first unmeasured
//! field, and [`super::lower_preset_roster`] exposes the measured part of the
//! roster. Nothing here picks a designed default for an original preset.
//!
//! The only designed value is the scenario seed (F49 non-negotiable 4): an
//! explicit, deterministic function of the preset id, so a preset always
//! launches under the same seed and the seed is visible in the catalog.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use cs_assets::install::{self, Discovery};
use cs_content::ai::DeclaredSkillTier;
use cs_content::catalog::baseline::{Baseline, BaselineError, retail_baseline};
use cs_content::environment::EnvironmentId;
use cs_content::instant_action::{
    ActorIntensity, GameIntensity, IA_ENGINE_DEFAULTS_CLAIM, IA_MISSION_TYPE_VOCABULARY,
    IaDenominator, IaDirectory, IaPlayerPlane, IaSurveyError, IaWingmanPlane, IaWingmanTemplate,
    IaWingmen, InstantActionCatalog, InstantActionPreset, RosterSlot, SCENARIO_SEED_DOMAIN,
    ScenarioActorSpec, ScenarioOptions, ScenarioParameters, ScenarioRoster, ScenarioSchemaError,
    ScenarioSeed, ScenarioSide, ia_spawns_named_ace, ia_wingman_template,
    measured_ia_victory_rules, retail_scenario_options_with_pilots, survey_retail_instant_action,
};
use cs_content::world::WorldId;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};

use crate::environment::retail::read_mission_weather;
use crate::mission_start::{AIRFRAME_TABLE, AirframeEntry, airframe_entry};

/// The claim every retail preset record is observed under.
pub const IA_PRESET_CLAIM: &str = "f49.d.03.ia-preset";
/// The claim the world binding is observed under.
pub const IA_WORLD_CLAIM: &str = "f49.d.03.ia-world-binding";
/// The claim the environment binding is observed under.
pub const IA_ENVIRONMENT_CLAIM: &str = "f49.d.03.ia-environment";
/// The claim the declared aircraft are observed under.
pub const IA_ROSTER_CLAIM: &str = "f49.d.03.ia-roster";
/// The claim of the whole assembled catalog.
pub const IA_CATALOG_CLAIM: &str = "f49.d.03.ia-catalog";

/// Unmeasured: the actors' factions.
pub const IA_FACTION_UNMEASURED_CLAIM: &str = "f49.d.03.unmeasured.faction";
/// Unmeasured: the actors' loadouts.
pub const IA_LOADOUT_UNMEASURED_CLAIM: &str = "f49.d.03.unmeasured.loadout";
/// Unmeasured: the actors' survivability.
pub const IA_SURVIVABILITY_UNMEASURED_CLAIM: &str = "f49.d.03.unmeasured.survivability";
/// Unmeasured: an enemy group without a plane.
pub const IA_GROUP_PLANE_UNMEASURED_CLAIM: &str = "f49.d.03.unmeasured.group-plane";
/// Unmeasured: the preset title.
pub const IA_TITLE_UNMEASURED_CLAIM: &str = "f49.d.03.unmeasured.title";
/// The preset difficulty is carried per actor (`GameIntensity`), not per scenario.
pub const IA_DIFFICULTY_PER_ACTOR_CLAIM: &str = "f49.d.03.difficulty-per-actor";
/// Unmeasured: the preset victory rules.
pub const IA_RULES_UNMEASURED_CLAIM: &str = "f49.d.03.unmeasured.rules";

/// Why the actors' factions are unmeasured.
pub const FACTION_UNMEASURED_REASON: &str = "neither the ia.zrd descriptor nor the executable's \
     scenario setup (0x459390 reads it, 0x45a390 spawns from it) passes a faction or allegiance \
     to the spawn call 0x47c210, which takes only a scene node, a placement and the plane; the \
     allegiance lives in state 0x47c210 derives downstream from the node and the airframe \
     template and has not been traced, and the F14-D.5 faction rows are paint patterns \
     (docs/findings/2026-10-18-f49-d-03-fu-faction-survivability.md)";
/// Why the actors' loadouts are unmeasured.
pub const LOADOUT_UNMEASURED_REASON: &str = "the ia.zrd descriptor carries no loadout and no \
     production reader mints a loadout identity (F27-D/F27-E measure counts and ammunition, not \
     loadouts)";
/// Why the actors' survivability is unmeasured.
pub const SURVIVABILITY_UNMEASURED_REASON: &str = "neither the ia.zrd descriptor nor the \
     executable's scenario setup passes mortality to the spawn call 0x47c210; the only \
     per-group value it sets around the call is the GameIntensity console variable (always 1, \
     a difficulty value, not mortality), so whether an actor is mortal is not measured \
     (docs/findings/2026-10-18-f49-d-03-fu-faction-survivability.md)";
/// The role of a wingman seated as the airframe row's `w` follower record.
pub const IA_WINGMAN_FOLLOWER_ROLE: &str = "wingman";
/// The role of a wingman seated as the airframe row's `base` record (the plain
/// aircraft an enemy group of that airframe also uses).
pub const IA_WINGMAN_BASE_ROLE: &str = "wingman_base";
/// Why a preset's title is unmeasured.
pub const TITLE_UNMEASURED_REASON: &str = "the original's Instant Action preset titles are not \
     measured (F49-D.01 records the identities, not the display names)";
/// Why a preset carries no scenario-level difficulty.
pub const DIFFICULTY_PER_ACTOR_REASON: &str = "the original binds no single difficulty to a \
     scenario: no ia.zrd key names one (F32-D census) and the executable never reads the group \
     `enemy_skill` key; its spawn routine 0x45a390 overwrites the GameIntensity console variable \
     around each spawn call 0x47c210 (the ace from `ace_skill` as 0/1/2, every enemy group as 1) \
     and writes nothing around the wingmen's, which run under the player's option value, so \
     each actor carries its own intensity \
     (docs/findings/2026-10-25-f49-d-03-fu-intensity.md)";

/// Why the rules of a preset with mission type `mission_type` are unmeasured.
///
/// Only reachable for a name the engine image's end check does not measure:
/// every name in the observed vocabulary is bound by
/// [`measured_ia_victory_rules`], so this stands for a descriptor carrying a
/// mission type nobody has measured (which the survey already refuses on the
/// vocabulary — the record stays correct if that ever changes).
fn rules_unmeasured_reason(mission_type: &str) -> String {
    format!(
        "the original declares mission type `{mission_type}` for this preset; the end check \
         `0x45b9d0` in the engine image measures no branch for that name, so how the type ends \
         the scenario is unmeasured and is not mapped by intuition"
    )
}

/// The mission type the record initialiser `0x458ff0` leaves in the live type
/// (`mov dword [ebp],0`), which a descriptor without a recognised
/// `mission_type` keeps.
const RESET_MISSION_TYPE: &str = "dogfight_ace";

/// Why a retail catalog could not be assembled.
#[derive(Debug)]
pub enum RetailCatalogError {
    /// Production discovery could not read the installation.
    Discovery(String),
    /// The `ia.zrd` survey refused.
    Survey(IaSurveyError),
    /// The baseline read refused.
    Baseline(BaselineError),
    /// The option table could not be validated.
    Options(ScenarioSchemaError),
    /// A claim id or source span could not be built.
    Provenance {
        /// What failed to build.
        what: String,
    },
    /// A descriptor names a plane the executable's airframe table does not.
    UnknownPlane {
        /// The container the descriptor is in.
        container: String,
        /// The actor role that names it.
        role: String,
        /// The plane name as the descriptor spells it.
        plane: String,
    },
    /// A descriptor's plane resolves to an airframe the option table lacks.
    PlaneNotOffered {
        /// The container the descriptor is in.
        container: String,
        /// The actor role that names it.
        role: String,
        /// The airframe id the plane resolves to.
        airframe: ContentId,
    },
    /// A descriptor names a skill outside the measured vocabulary.
    UnknownSkill {
        /// The container the descriptor is in.
        container: String,
        /// The actor role that names it.
        role: String,
        /// The skill as the descriptor spells it.
        skill: String,
    },
    /// A preset or catalog record was refused by the F49-A schema.
    Schema {
        /// The preset (or catalog) being assembled.
        subject: String,
        /// The schema refusal.
        error: ScenarioSchemaError,
    },
}

impl fmt::Display for RetailCatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(text) => write!(f, "the installation cannot be discovered: {text}"),
            Self::Survey(error) => write!(f, "the ia.zrd survey refused: {error}"),
            Self::Baseline(error) => write!(f, "the baseline read refused: {error}"),
            Self::Options(error) => write!(f, "the option table refused: {error}"),
            Self::Provenance { what } => write!(f, "provenance cannot be recorded: {what}"),
            Self::UnknownPlane {
                container,
                role,
                plane,
            } => write!(
                f,
                "{container}: {role} names plane `{plane}`, which the airframe table does not hold"
            ),
            Self::PlaneNotOffered {
                container,
                role,
                airframe,
            } => write!(
                f,
                "{container}: {role} resolves to {airframe}, which the option table does not offer"
            ),
            Self::UnknownSkill {
                container,
                role,
                skill,
            } => write!(
                f,
                "{container}: {role} names skill `{skill}`, outside the measured vocabulary"
            ),
            Self::Schema { subject, error } => write!(f, "{subject}: {error}"),
        }
    }
}

impl std::error::Error for RetailCatalogError {}

fn claim(id: &str) -> Result<ClaimId, RetailCatalogError> {
    ClaimId::new(id).map_err(|error| RetailCatalogError::Provenance {
        what: format!("claim {id}: {error}"),
    })
}

fn observed(id: &str, source: &SourceSpan) -> Result<Provenance, RetailCatalogError> {
    Provenance::new(claim(id)?, ClaimStatus::ObservedTool, Some(source.clone())).map_err(|error| {
        RetailCatalogError::Provenance {
            what: format!("claim {id}: {error}"),
        }
    })
}

fn unknown<T>(id: &str, reason: &str) -> Result<Resolved<T>, RetailCatalogError> {
    Resolved::unknown(claim(id)?, reason).map_err(|error| RetailCatalogError::Provenance {
        what: format!("unknown {id}: {error}"),
    })
}

/// What the assembly needs about one IA directory beyond its descriptor.
#[derive(Debug)]
pub struct DirectoryInputs<'a> {
    /// The installation fingerprint the spans are recorded against.
    pub install_sha256: ContentHash,
    /// The container's path as the installation spells it.
    pub container_path: &'a str,
    /// The validated option table (worlds and airframes are looked up in it).
    pub options: &'a ScenarioOptions,
    /// The IA archive's own bound weather, or why it could not be read.
    pub environment: Result<(EnvironmentId, SourceSpan), String>,
}

/// The seed a preset launches under: FNV-1a over the preset id, mixed with the
/// scenario seed domain. Designed, explicit and a pure function of the id.
#[must_use]
pub fn preset_seed(preset: &ContentId) -> ScenarioSeed {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in preset.as_str().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    ScenarioSeed::new(hash ^ SCENARIO_SEED_DOMAIN)
}

/// Resolves a descriptor's plane name onto an airframe the option table offers.
fn resolve_airframe(
    plane: &str,
    role: &str,
    inputs: &DirectoryInputs<'_>,
) -> Result<ContentId, RetailCatalogError> {
    let container = inputs.container_path.to_owned();
    let (_, entry) = airframe_entry(plane).ok_or_else(|| RetailCatalogError::UnknownPlane {
        container: container.clone(),
        role: role.to_owned(),
        plane: plane.to_owned(),
    })?;
    airframe_id(entry, role, inputs)
}

/// Resolves an airframe-table row index (the executable's own default) onto an
/// airframe the option table offers.
fn resolve_airframe_index(
    index: usize,
    role: &str,
    inputs: &DirectoryInputs<'_>,
) -> Result<ContentId, RetailCatalogError> {
    let entry = AIRFRAME_TABLE
        .get(index)
        .ok_or_else(|| RetailCatalogError::UnknownPlane {
            container: inputs.container_path.to_owned(),
            role: role.to_owned(),
            plane: format!("airframe table row {index}"),
        })?;
    airframe_id(entry, role, inputs)
}

fn airframe_id(
    entry: &AirframeEntry,
    role: &str,
    inputs: &DirectoryInputs<'_>,
) -> Result<ContentId, RetailCatalogError> {
    let container = inputs.container_path.to_owned();
    let id = ContentId::from_source(ContentKind::Airframe, entry.scene_root).map_err(|error| {
        RetailCatalogError::Provenance {
            what: format!("airframe key {}: {error}", entry.scene_root),
        }
    })?;
    if !inputs.options.airframes().contains(&id) {
        return Err(RetailCatalogError::PlaneNotOffered {
            container,
            role: role.to_owned(),
            airframe: id,
        });
    }
    Ok(id)
}

/// The `GameIntensity` the spawn routine holds around every enemy group's
/// spawn call (the group record's last dword, `1` and written by no reader).
const GROUP_INTENSITY: GameIntensity = match GameIntensity::new(1) {
    Some(value) => value,
    None => unreachable!(),
};

/// The `GameIntensity` the spawn routine holds around the ace's spawn call:
/// `ace_skill` `novice`/`veteran`/`ace` is stored as 0/1/2 and mapped 0->0,
/// 2->2, else 1 (`0x45acbe..0x45acfc`).
const fn ace_intensity(skill: DeclaredSkillTier) -> GameIntensity {
    let value = match skill {
        DeclaredSkillTier::Novice => 0,
        DeclaredSkillTier::Veteran => 1,
        DeclaredSkillTier::Ace => 2,
    };
    match GameIntensity::new(value) {
        Some(value) => value,
        None => unreachable!(),
    }
}

fn resolve_skill(
    skill: Option<&str>,
    role: &str,
    inputs: &DirectoryInputs<'_>,
) -> Result<Option<DeclaredSkillTier>, RetailCatalogError> {
    skill
        .map(|label| {
            DeclaredSkillTier::from_label(label).ok_or_else(|| RetailCatalogError::UnknownSkill {
                container: inputs.container_path.to_owned(),
                role: role.to_owned(),
                skill: label.to_owned(),
            })
        })
        .transpose()
}

/// One declared actor with every dimension the descriptor does not carry left
/// explicitly unmeasured.
fn actor(
    side: ScenarioSide,
    slot: u32,
    role: &str,
    airframe: Resolved<ContentId>,
    provenance: &Provenance,
    subject: &str,
) -> Result<ScenarioActorSpec, RetailCatalogError> {
    let schema = |error| RetailCatalogError::Schema {
        subject: subject.to_owned(),
        error,
    };
    ScenarioActorSpec::try_new_resolved(
        side,
        RosterSlot(slot),
        unknown(IA_FACTION_UNMEASURED_CLAIM, FACTION_UNMEASURED_REASON)?,
        airframe,
        unknown(IA_LOADOUT_UNMEASURED_CLAIM, LOADOUT_UNMEASURED_REASON)?,
        None,
        unknown(
            IA_SURVIVABILITY_UNMEASURED_CLAIM,
            SURVIVABILITY_UNMEASURED_REASON,
        )?,
        provenance.clone(),
    )
    .map(|actor| actor.with_role(role.to_owned()))
    .map_err(schema)
}

/// The roster the executable's spawn routine (`0x45a390`) seats for one
/// directory under one mission type: the declared aircraft, with the
/// executable's own defaults for a `player_plane` / `num_wingmen` /
/// `wingman_plane` the descriptor leaves out, and without the named ace for
/// every type but `dogfight_ace`.
///
/// # Errors
///
/// [`RetailCatalogError`] naming the container and actor for a plane outside
/// the airframe table or option table and for a skill outside the vocabulary.
pub fn directory_roster(
    directory: &IaDirectory,
    mission_type: &str,
    inputs: &DirectoryInputs<'_>,
    source: &SourceSpan,
) -> Result<ScenarioRoster, RetailCatalogError> {
    let subject = directory.container().to_owned();
    let provenance = observed(IA_ROSTER_CLAIM, source)?;
    let known = |id: ContentId| Resolved::Known(Known::new(id, provenance.clone()));
    let mut actors = Vec::new();

    // A scenario that declares no `player_plane` / `num_wingmen` keeps the
    // values the executable's record initialiser wrote (see `IaPlayerPlane`).
    // Those are recorded as inferred from the absent key, not as declared.
    let default_provenance = Provenance::new(
        claim(IA_ENGINE_DEFAULTS_CLAIM)?,
        ClaimStatus::Inferred,
        Some(source.clone()),
    )
    .map_err(|error| RetailCatalogError::Provenance {
        what: format!("claim {IA_ENGINE_DEFAULTS_CLAIM}: {error}"),
    })?;
    let (player_airframe, player_provenance) = match directory.player_plane() {
        IaPlayerPlane::Declared(plane) => (
            known(resolve_airframe(&plane, "player_plane", inputs)?),
            provenance.clone(),
        ),
        IaPlayerPlane::EngineDefault { index } => (
            Resolved::Known(Known::new(
                resolve_airframe_index(index, "player_plane", inputs)?,
                default_provenance.clone(),
            )),
            default_provenance.clone(),
        ),
    };
    actors.push(actor(
        ScenarioSide::Player,
        0,
        "player",
        player_airframe,
        &player_provenance,
        &subject,
    )?);

    // The wingmen fly the record's `wingman_plane`; no retail descriptor
    // declares the key, so they keep the initialiser's row 5. The vehicle
    // record they are seated as depends on the mission type (the role names
    // it).
    let wingmen = directory.wingmen();
    let wingman_provenance = match wingmen {
        IaWingmen::Declared(_) => &provenance,
        IaWingmen::EngineDefault(_) => &default_provenance,
    };
    let (wingman_airframe, wingman_plane_provenance) = match directory.wingman_plane() {
        IaWingmanPlane::Declared(plane) => (
            known(resolve_airframe(&plane, "wingman_plane", inputs)?),
            wingman_provenance.clone(),
        ),
        IaWingmanPlane::EngineDefault { index } => (
            Resolved::Known(Known::new(
                resolve_airframe_index(index, "wingman_plane", inputs)?,
                default_provenance.clone(),
            )),
            default_provenance.clone(),
        ),
    };
    let wingman_role = match ia_wingman_template(mission_type) {
        Some(IaWingmanTemplate::Base) => IA_WINGMAN_BASE_ROLE,
        Some(IaWingmanTemplate::Follower) | None => IA_WINGMAN_FOLLOWER_ROLE,
    };
    for slot in 0..wingmen.count() {
        actors.push(
            actor(
                ScenarioSide::Ally,
                slot,
                wingman_role,
                wingman_airframe.clone(),
                &wingman_plane_provenance,
                &subject,
            )?
            .with_intensity(ActorIntensity::PlayerOption),
        );
    }

    let mut next_enemy_slot = 0_u32;
    for group in directory.aircraft().enemy_groups() {
        let role = format!("group{}", group.index());
        let airframe = match group.plane() {
            Some(plane) => known(resolve_airframe(plane, &role, inputs)?),
            None => unknown(
                IA_GROUP_PLANE_UNMEASURED_CLAIM,
                "the enemy group record declares no plane",
            )?,
        };
        let mut spec = actor(
            ScenarioSide::Enemy,
            next_enemy_slot,
            &role,
            airframe,
            &provenance,
            &subject,
        )?
        .with_intensity(ActorIntensity::Set(GROUP_INTENSITY));
        if let Some(skill) = resolve_skill(group.skill(), &role, inputs)? {
            spec = spec.with_skill(skill);
        }
        if let Some(count) = group.count() {
            spec = spec.with_authored_count(count);
        }
        if let Some(label) = group.name_label() {
            spec = spec.with_label(label.to_owned());
        }
        actors.push(spec);
        next_enemy_slot += 1;
    }

    // The named ace. The descriptor states no side for it; it is seated on the
    // enemy side beside the `enemy_*` groups, which the finding records as an
    // inference from the key set rather than a measured side.
    //
    // The spawn routine seats the ace for `dogfight_ace` only.
    let ace = directory.aircraft().ace();
    let seats_ace = ia_spawns_named_ace(mission_type).unwrap_or(true);
    if seats_ace && (ace.plane().is_some() || ace.skill().is_some() || ace.name_label().is_some()) {
        let airframe = match ace.plane() {
            Some(plane) => known(resolve_airframe(plane, "ace", inputs)?),
            None => unknown(
                IA_GROUP_PLANE_UNMEASURED_CLAIM,
                "the ace record declares no plane",
            )?,
        };
        let mut spec = actor(
            ScenarioSide::Enemy,
            next_enemy_slot,
            "ace",
            airframe,
            &provenance,
            &subject,
        )?;
        if let Some(skill) = resolve_skill(ace.skill(), "ace", inputs)? {
            spec = spec
                .with_skill(skill)
                .with_intensity(ActorIntensity::Set(ace_intensity(skill)));
        }
        if let Some(label) = ace.name_label() {
            spec = spec.with_label(label.to_owned());
        }
        actors.push(spec);
    }

    let roster = ScenarioRoster::try_new(actors)
        .map_err(|error| RetailCatalogError::Schema { subject, error })?;
    Ok(roster)
}

/// One preset per offered mission type of one directory.
///
/// # Errors
///
/// [`RetailCatalogError`] from the roster, the identities or the F49-A schema.
pub fn directory_presets(
    directory: &IaDirectory,
    inputs: &DirectoryInputs<'_>,
) -> Result<Vec<InstantActionPreset>, RetailCatalogError> {
    let subject = directory.container().to_owned();
    let (offset, length) = directory.member_span();
    let source = SourceSpan::new(
        inputs.install_sha256,
        inputs.container_path,
        Some(cs_content::stunts::SCENARIO_MEMBER),
        offset,
        length,
        None,
    )
    .map_err(|error| RetailCatalogError::Provenance {
        what: format!("{subject} span: {error}"),
    })?;
    let schema = |error| RetailCatalogError::Schema {
        subject: subject.clone(),
        error,
    };

    let world_id = WorldId::from_key(directory.world_group()).map_err(|error| {
        RetailCatalogError::Provenance {
            what: format!("{subject} world group: {error}"),
        }
    })?;
    let world = if inputs.options.worlds().contains(&world_id) {
        Resolved::Known(Known::new(world_id, observed(IA_WORLD_CLAIM, &source)?))
    } else {
        unknown(
            IA_WORLD_CLAIM,
            "the directory's world group has no F14-D.3 world row",
        )?
    };
    let environment = match &inputs.environment {
        Ok((id, span)) => Resolved::Known(Known::new(
            id.clone(),
            observed(IA_ENVIRONMENT_CLAIM, span)?,
        )),
        Err(reason) => unknown(IA_ENVIRONMENT_CLAIM, reason)?,
    };
    let mut presets = Vec::new();
    for ((preset_id, scenario_id), offered) in directory
        .preset_ids()
        .map_err(RetailCatalogError::Survey)?
        .into_iter()
        .zip(directory.offered())
    {
        // The menu row's type never runs: the load resets the live type and
        // re-reads the descriptor's own `mission_type`
        // (docs/findings/2026-10-24-f49-d-03-fu-rules-type-order.md), so the
        // roster and the rules are those of the directory's default type. A
        // descriptor without the key keeps the reset value `dogfight_ace`.
        // The 22 non-default rows stay presets of their own identity and
        // launch exactly what the default row launches.
        let offered_type = offered.mission_type();
        let mission_type = directory
            .default_mission_type()
            .filter(|name| IA_MISSION_TYPE_VOCABULARY.contains(name))
            .unwrap_or(RESET_MISSION_TYPE);
        let roster = directory_roster(directory, mission_type, inputs, &source)?;
        let mut parameters = ScenarioParameters::new_unmeasured_rules(
            world.clone(),
            environment.clone(),
            roster.clone(),
            preset_seed(&preset_id),
            (
                claim(IA_DIFFICULTY_PER_ACTOR_CLAIM)?,
                DIFFICULTY_PER_ACTOR_REASON,
            ),
            (
                claim(IA_RULES_UNMEASURED_CLAIM)?,
                &rules_unmeasured_reason(mission_type),
            ),
        );
        // The original binds the difficulty per actor (`GameIntensity` around
        // each spawn call), not per scenario.
        parameters = parameters.with_per_actor_difficulty(
            claim(IA_DIFFICULTY_PER_ACTOR_CLAIM)?,
            DIFFICULTY_PER_ACTOR_REASON,
        );
        // The engine image's Instant Action end check (`0x45b9d0`) measures
        // one end condition per `mission_type` name; a name it does not cover
        // keeps the unmeasured record above.
        if let Some(rules) = measured_ia_victory_rules(mission_type) {
            parameters = parameters.with_measured_rules(rules);
        }
        let preset = InstantActionPreset::try_new(
            preset_id,
            scenario_id,
            unknown(IA_TITLE_UNMEASURED_CLAIM, TITLE_UNMEASURED_REASON)?,
            parameters,
            Origin::Installation {
                source: source.clone(),
            },
            observed(IA_PRESET_CLAIM, &source)?,
        )
        .map_err(schema)?
        .with_original_mission_type(offered_type.to_owned())
        .with_launched_mission_type(mission_type.to_owned());
        presets.push(preset);
    }
    Ok(presets)
}

/// The environments the option table offers: every campaign mission's bound
/// weather plus every IA archive's own.
fn option_environments(
    install_root: &Path,
    found: &Discovery,
    denominator: &IaDenominator,
) -> Vec<EnvironmentId> {
    let mut ids: BTreeSet<EnvironmentId> = BTreeSet::new();
    if let Ok(layout) = cs_content::campaign_bindings::campaign_layout(install_root) {
        for entry in &layout {
            let program = entry.mission.program_asset.to_ascii_lowercase();
            if let Some(mission_key) = program.strip_suffix("/zrdr.zbd")
                && let Ok(weather) = read_mission_weather(found, mission_key)
            {
                ids.insert(weather.binding.definition.id().clone());
            }
        }
    }
    for directory in denominator.directories() {
        if let Some(mission_key) = directory.container().strip_suffix("/zrdr.zbd")
            && let Ok(weather) = read_mission_weather(found, mission_key)
        {
            ids.insert(weather.binding.definition.id().clone());
        }
    }
    ids.into_iter().collect()
}

/// Assembles the catalog over an already-read installation.
///
/// # Errors
///
/// [`RetailCatalogError`] for any directory that cannot be bound, and for a
/// catalog the F49-A schema refuses (a preset without a player-side actor, a
/// duplicate identity).
pub fn assemble_retail_catalog(
    found: &Discovery,
    denominator: &IaDenominator,
    baseline: &Baseline,
    environments: Vec<EnvironmentId>,
) -> Result<InstantActionCatalog, RetailCatalogError> {
    let options = retail_scenario_options_with_pilots(
        baseline,
        environments,
        denominator
            .ace_pilot_ids()
            .map_err(RetailCatalogError::Survey)?,
    )
    .validate_partial()
    .map_err(RetailCatalogError::Options)?;
    let install_sha256 = install::fingerprint(&found.manifest);

    let mut presets = Vec::new();
    for directory in denominator.directories() {
        let record = found
            .manifest
            .files
            .iter()
            .find(|record| record.relative_spelling.logical_key() == directory.container())
            .ok_or_else(|| {
                RetailCatalogError::Survey(IaSurveyError::Read {
                    container: directory.container().to_owned(),
                    reason: "the container is not in the installation inventory".to_owned(),
                })
            })?;
        let environment = match directory.container().strip_suffix("/zrdr.zbd") {
            Some(mission_key) => read_mission_weather(found, mission_key)
                .map(|weather| (weather.binding.definition.id().clone(), weather.span))
                .map_err(|error| error.to_string()),
            None => Err("the container key has no reader-archive suffix".to_owned()),
        };
        let inputs = DirectoryInputs {
            install_sha256,
            container_path: record.relative_spelling.as_str(),
            options: &options,
            environment,
        };
        presets.extend(directory_presets(directory, &inputs)?);
    }
    InstantActionCatalog::try_new(
        presets,
        options,
        Provenance::new(claim(IA_CATALOG_CLAIM)?, ClaimStatus::ObservedTool, None).map_err(
            |error| RetailCatalogError::Provenance {
                what: error.to_string(),
            },
        )?,
    )
    .map_err(|error| RetailCatalogError::Schema {
        subject: "the catalog".to_owned(),
        error,
    })
}

/// Reads the installation and assembles the retail Instant Action catalog.
///
/// # Errors
///
/// [`RetailCatalogError`] naming the stage that failed.
pub fn retail_instant_action_catalog(
    install_root: &Path,
) -> Result<InstantActionCatalog, RetailCatalogError> {
    let found = install::discover(install_root)
        .map_err(|error| RetailCatalogError::Discovery(format!("{error:?}")))?;
    let denominator =
        survey_retail_instant_action(install_root).map_err(RetailCatalogError::Survey)?;
    let baseline = retail_baseline(install_root).map_err(RetailCatalogError::Baseline)?;
    let environments = option_environments(install_root, &found, &denominator);
    assemble_retail_catalog(&found, &denominator, &baseline, environments)
}
