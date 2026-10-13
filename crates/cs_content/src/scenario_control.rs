//! The scenario control surface of a reader archive that declares **no**
//! numbered objective block, measured by rule.
//!
//! Task: `M01-LC-SCENARIOS` (#642). Shared contract:
//! `docs/contracts/SCRIPT-MISSION.md`. The retail walk over the installation is
//! [`cs_app::scenario_control`]; this module is the half that needs no
//! installation: the selection rule, the measured shapes it reads and the
//! vocabularies it counts.
//!
//! # The question, and the rule that answers it
//!
//! `M01-LC-MISSION-PROGRAM` (#630) measured 40 of the installation's 53
//! mission-scoped reader archives as carrying a control member — the member
//! whose decoded record declares numbered `OBJECTIVE<N>` blocks — and found
//! **13** that declare none. Each of those 13 is an instant-action (`ia1`) or
//! multiplayer (`MP*`) scenario, and #630 kept them as a measured absence
//! rather than guessing where their control data lives. This module answers
//! that question **by contents, never by name**:
//!
//! > The scenario control member of a reader archive is **the member whose
//! > decoded record declares the measured scenario descriptor surface** — the
//! > member for which [`scenario_mission_type`] answers: a flat record carrying
//! > the `mission_type` field whose value is the one-element text list the
//! > original writes (`["dogfight_squadron"]`).
//!
//! The rule reuses F42-A's scenario grammar ([`scenario_mission_type`],
//! [`scenario_zone_bindings`], [`scenario_non_player_aircraft`]) rather than
//! defining a second reading of the same bytes. Nothing about a member's name,
//! length or archive position counts: a descriptor under a name the corpus has
//! never seen is still selected, and a member *named* `ia.zrd` that stops
//! declaring a scenario mode stops being selected. Zero candidates is a
//! **measured absence** (measured: the five multiplayer readers have none) and
//! two are a **refusal**, never a choice.
//!
//! # What is measured, and what is not
//!
//! The 13 readers split into two surfaces, and this module keeps them apart:
//!
//! * The eight instant-action readers each carry exactly one descriptor
//!   member. Its vocabulary is measured in full ([`MeasuredScenarioDescriptor`]:
//!   the declared mode, every flat key with its argument shape, the declared
//!   aircraft, the zone bindings and the per-mode spawn-point lists).
//! * The five multiplayer readers declare **no** descriptor member anywhere in
//!   their member list. Their control data is a **different, thinner surface**,
//!   and each half of it is measured by its own shape reader: the record-level
//!   keys of `objectives.zrd` ([`measure_scenario_objective_record`]), the bare
//!   spawn-point list of `net.zrd` ([`measure_net_spawns`]) and the
//!   objective-marked target records of `targets.zrd`
//!   ([`count_target_markers`]).
//!
//! What no measurement here claims: what any of it **does**. No original
//! executable has been run; a mode name, a spawn coordinate and an `objective`
//! marker are spellings and shapes, never observed behaviour. The unmeasured
//! fourth number of a spawn entry, the meaning of a scenario mode and the rule
//! by which a multiplayer objective completes are recorded as unknown with the
//! affected content named in the finding
//! (`docs/findings/2026-10-13-m01-lc-scenarios.md`).

use crate::mission_control::{ControlRecordField, DecodedMember, DirectiveShape, MeasuredArg};
use crate::stunts::{
    SCENARIO_MISSION_TYPE_KEY, SCENARIO_PLAYER_PLANE_KEY, SCENARIO_WINGMEN_KEY,
    ScenarioNonPlayerAircraft, TARGET_OBJECTIVE_KEY, TARGET_OTHER_TARGET_KEY, ZrdValue,
    objective_record, scenario_mission_type, scenario_non_player_aircraft, scenario_zone_bindings,
    zrd_field, zrd_flat_fields,
};

/// The scenario descriptor field holding the per-mode spawn-point lists
/// (`spawn_points`). The spelling is measured over the owner's installation:
/// every instant-action descriptor declares it, and its value is the
/// `(mode, points)` pair list [`scenario_spawn_modes`] reads.
const SCENARIO_SPAWN_POINTS_KEY: &str = "spawn_points";

/// Why a reader archive's scenario control member could not be selected.
///
/// Measured over the owner's installation: zero archives raise it — each of the
/// eight instant-action readers declares exactly one descriptor member and the
/// five multiplayer readers declare none. Carried because picking one of two
/// candidates would be a guess about which half drives the scenario, exactly
/// the refusal #630's block rule carries for two block-bearing members.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScenarioMemberError {
    /// **More than one** member declares the measured scenario descriptor
    /// surface, so the archive has no single scenario control member.
    AmbiguousScenarioMember {
        /// The qualifying members' names, in member-table order.
        candidates: Vec<String>,
    },
}

impl std::fmt::Display for ScenarioMemberError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AmbiguousScenarioMember { candidates } => write!(
                f,
                "the archive declares {} members carrying a scenario descriptor ({}); \
                 picking one would be a guess about which drives the scenario",
                candidates.len(),
                candidates.join(", ")
            ),
        }
    }
}

impl std::error::Error for ScenarioMemberError {}

/// The members of one reader archive that declare the measured scenario
/// descriptor surface, in member-table order.
///
/// A member qualifies when [`scenario_mission_type`] answers for its decoded
/// document — a content rule, so the member's name is never consulted.
#[must_use]
pub fn scenario_descriptor_candidates(members: &[DecodedMember]) -> Vec<&DecodedMember> {
    members
        .iter()
        .filter(|member| scenario_mission_type(&member.document).is_some())
        .collect()
}

/// The scenario control member of one reader archive, by contents.
///
/// The unique descriptor candidate, `None` when the archive declares none
/// (a **measured absence**, not an error), and
/// [`ScenarioMemberError::AmbiguousScenarioMember`] when it declares more than
/// one (a refusal, never a choice).
///
/// # Errors
///
/// [`ScenarioMemberError::AmbiguousScenarioMember`] when two or more members
/// declare the descriptor surface.
pub fn scenario_control_member(
    members: &[DecodedMember],
) -> Result<Option<&DecodedMember>, ScenarioMemberError> {
    let candidates = scenario_descriptor_candidates(members);
    match candidates.as_slice() {
        [] => Ok(None),
        [only] => Ok(Some(only)),
        many => Err(ScenarioMemberError::AmbiguousScenarioMember {
            candidates: many.iter().map(|member| member.name.clone()).collect(),
        }),
    }
}

/// The measured shape of one `.zrd` value, described by its children — the same
/// four-tag description `mission_control` measures directive arguments with, so
/// the two vocabularies cannot drift apart.
fn descriptor_arg_shape(value: &ZrdValue) -> MeasuredArg {
    match value {
        ZrdValue::Int(_) => MeasuredArg::Int,
        ZrdValue::Float(_) => MeasuredArg::Float,
        ZrdValue::Text(_) => MeasuredArg::Text,
        ZrdValue::List(children) if children.is_empty() => MeasuredArg::Empty,
        ZrdValue::List(children) => {
            MeasuredArg::List(children.iter().map(descriptor_arg_shape).collect())
        }
    }
}

/// One scenario descriptor's declared mode, with the shape of every field it
/// spells and the surfaces F42-A's readers derive from it.
///
/// Every field is a **declaration**, not a behaviour: the mode names the
/// scenario's kind, the spawn points are coordinates, and what any of them does
/// at runtime is unmeasured (no original executable has been run).
#[derive(Clone, Debug, PartialEq)]
pub struct MeasuredScenarioDescriptor {
    /// The declared scenario mode ([`crate::stunts::scenario_mission_type`]),
    /// measured. `None` when the record declares the field in a shape the F42-A
    /// reader does not accept — never invented from the member's name.
    mission_type: Option<String>,
    /// Every flat key the descriptor declares, sorted, with each value's
    /// measured shape. The complete inventory: a consumer sees the whole
    /// vocabulary, including keys no reader consumes.
    field_shapes: Vec<(String, DirectiveShape)>,
    /// The declared player aircraft, wingmen, enemy groups and ace
    /// ([`crate::stunts::scenario_non_player_aircraft`], F42-A's reader).
    aircraft: ScenarioNonPlayerAircraft,
    /// The declared `(world node, scenario label)` zone bindings
    /// ([`crate::stunts::scenario_zone_bindings`], F42-A's reader).
    zone_bindings: Vec<(String, String)>,
    /// The declared per-mode spawn-point lists, as `(mode, entry count)` pairs
    /// in author order ([`scenario_spawn_modes`]).
    spawn_modes: Vec<(String, u32)>,
}

impl MeasuredScenarioDescriptor {
    /// The declared scenario mode, when the record declares one the F42-A
    /// reader accepts.
    #[must_use]
    pub fn mission_type(&self) -> Option<&str> {
        self.mission_type.as_deref()
    }

    /// Every flat key the descriptor declares, sorted, with its shape.
    #[must_use]
    pub fn field_shapes(&self) -> &[(String, DirectiveShape)] {
        &self.field_shapes
    }

    /// The declared player aircraft, wingmen, enemy groups and ace.
    #[must_use]
    pub const fn aircraft(&self) -> &ScenarioNonPlayerAircraft {
        &self.aircraft
    }

    /// The declared `(world node, scenario label)` zone bindings.
    #[must_use]
    pub fn zone_bindings(&self) -> &[(String, String)] {
        &self.zone_bindings
    }

    /// The declared per-mode spawn-point lists as `(mode, entry count)` pairs.
    #[must_use]
    pub fn spawn_modes(&self) -> &[(String, u32)] {
        &self.spawn_modes
    }
}

/// Measures one scenario descriptor document: its declared mode, every flat
/// key with its shape, the declared aircraft, the zone bindings and the
/// per-mode spawn-point lists.
///
/// The record is read with the **flat** field walk ([`crate::stunts::zrd_flat_fields`]),
/// the shape the original writes `ia.zrd` in; a document that is not a flat
/// record measures as an all-empty descriptor rather than being paired into
/// keys that are not there.
#[must_use]
pub fn measure_scenario_descriptor(document: &ZrdValue) -> MeasuredScenarioDescriptor {
    let mut field_shapes: Vec<(String, DirectiveShape)> = zrd_flat_fields(document)
        .into_iter()
        .map(|(key, value)| {
            let shape = match value.as_list() {
                Some(children) => {
                    DirectiveShape::Arguments(children.iter().map(descriptor_arg_shape).collect())
                }
                None => DirectiveShape::NotAList,
            };
            (key.to_owned(), shape)
        })
        .collect();
    field_shapes.sort();
    MeasuredScenarioDescriptor {
        mission_type: scenario_mission_type(document).map(str::to_owned),
        field_shapes,
        aircraft: scenario_non_player_aircraft(document),
        zone_bindings: scenario_zone_bindings(document)
            .into_iter()
            .map(|(node, label)| (node.to_owned(), label.to_owned()))
            .collect(),
        spawn_modes: scenario_spawn_modes(document),
    }
}

/// The spawn-point modes a scenario descriptor declares, as `(mode, entry
/// count)` pairs in author order.
///
/// Measured shape: the `spawn_points` field is a **flat alternating** list of
/// mode text and point list — `["dogfight_ace", [[x, y, z, heading], …], …]`,
/// the same flat key/value grammar the record itself uses — so each declared
/// mode carries its own spawn list. A child that is not text where a mode is
/// expected ends the walk rather than shifting the pairing, and an entry that
/// is not a four-float list is not counted.
#[must_use]
pub fn scenario_spawn_modes(scenario: &ZrdValue) -> Vec<(String, u32)> {
    let Some(points) = zrd_field(scenario, SCENARIO_SPAWN_POINTS_KEY) else {
        return Vec::new();
    };
    let Some(children) = points.as_list() else {
        return Vec::new();
    };
    let mut modes = Vec::new();
    let mut index = 0;
    while index < children.len() {
        let Some(mode) = children[index].as_text() else {
            break;
        };
        let Some(list) = children.get(index + 1).and_then(ZrdValue::as_list) else {
            break;
        };
        let count = list
            .iter()
            .filter(|point| {
                point.as_list().is_some_and(|point| {
                    point.len() == 4
                        && point
                            .iter()
                            .all(|value| matches!(value, ZrdValue::Float(_)))
                })
            })
            .count();
        modes.push((mode.to_owned(), count as u32));
        index += 2;
    }
    modes
}

/// The record-level objective keys one `objectives.zrd` declares, classified
/// against the measured record vocabulary of #630.
///
/// A scenario's `objectives.zrd` is a record with **no** numbered blocks (the
/// measured reason #630 reports it as an absent control program), and it
/// carries the record-level keys — `MISSION_TIMER`, `PLAYER_INIT`, … — beside
/// keys the campaign vocabulary does not measure. Keys outside
/// [`crate::mission_control::CONTROL_RECORD_KEY_VOCABULARY`] are kept as
/// **unclassified** rather than absorbed into a documented field, so a new
/// spelling is visible instead of silent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioObjectiveRecord {
    /// The record-level keys the member declares, with the number of sites
    /// each has (a flat record spells each key once), sorted by key.
    keys: Vec<(String, u32)>,
    /// The keys outside the measured record vocabulary, sorted.
    unclassified: Vec<String>,
}

impl ScenarioObjectiveRecord {
    /// Every record-level key the member declares, with its site count.
    #[must_use]
    pub fn keys(&self) -> &[(String, u32)] {
        &self.keys
    }

    /// The declared keys outside #630's measured record vocabulary, sorted.
    #[must_use]
    pub fn unclassified(&self) -> &[String] {
        &self.unclassified
    }

    /// Whether the member declares at least one key of the measured record
    /// vocabulary.
    #[must_use]
    pub fn declares_measured_key(&self) -> bool {
        self.keys
            .iter()
            .any(|(key, _)| ControlRecordField::from_key(key).is_some())
    }
}

/// Measures one `objectives.zrd` member of a scenario reader: its record-level
/// keys and which of them fall outside the measured record vocabulary.
///
/// The record is unwrapped with [`crate::stunts::objective_record`] — the same
/// one-element-root reading #630's block census uses — and walked with the flat
/// field reader, because a scenario record is flat.
#[must_use]
pub fn measure_scenario_objective_record(document: &ZrdValue) -> ScenarioObjectiveRecord {
    let mut counts: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    for (key, _) in zrd_flat_fields(objective_record(document)) {
        *counts.entry(key.to_owned()).or_insert(0) += 1;
    }
    let unclassified: Vec<String> = counts
        .keys()
        .filter(|key| ControlRecordField::from_key(key).is_none())
        .cloned()
        .collect();
    ScenarioObjectiveRecord {
        keys: counts.into_iter().collect(),
        unclassified,
    }
}

/// What one `net.zrd` member declares, by its measured shape.
///
/// Measured over the installation: `net.zrd` is a **bare spawn-point list** —
/// the root is one list of four-float entries (`[[x, y, z, heading], …]`) with
/// **no keys at all** — declared by multiplayer scenario readers and by
/// campaign readers alike. A member in any other shape is
/// [`NetSpawnSurface::UnmeasuredShape`], never read as spawn points.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetSpawnSurface {
    /// The archive declares no `net.zrd` member.
    Absent,
    /// The member declares the measured bare list, with that many entries.
    Measured(u32),
    /// The member declares a shape this rule does not measure. The count is not
    /// guessed; the finding names the archive.
    UnmeasuredShape,
}

/// Measures one `net.zrd` member by the measured bare spawn-point shape: the
/// root is one list whose children are all four-float lists.
#[must_use]
pub fn measure_net_spawns(net: &ZrdValue) -> NetSpawnSurface {
    let Some(root) = net.as_list() else {
        return NetSpawnSurface::UnmeasuredShape;
    };
    let [only] = root else {
        return NetSpawnSurface::UnmeasuredShape;
    };
    let Some(entries) = only.as_list() else {
        return NetSpawnSurface::UnmeasuredShape;
    };
    let bare = entries.iter().all(|entry| {
        entry.as_list().is_some_and(|point| {
            point.len() == 4
                && point
                    .iter()
                    .all(|value| matches!(value, ZrdValue::Float(_)))
        })
    });
    if bare {
        NetSpawnSurface::Measured(entries.len() as u32)
    } else {
        NetSpawnSurface::UnmeasuredShape
    }
}

/// How many objective-target records one `targets.zrd` declares, and how many
/// carry each measured marker key.
///
/// The multiplayer readers declare their numbered objectives **here**, as
/// records carrying the [`crate::stunts::TARGET_OBJECTIVE_KEY`] marker, rather
/// than as `OBJECTIVE<N>` blocks in `objectives.zrd` — the measured reason
/// #630's block rule finds nothing in them. A marker counts a record that
/// spells the key at all, whatever its value's shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TargetMarkerCounts {
    /// The records the member declares.
    pub records: u32,
    /// The records carrying the `objective` marker.
    pub objective_marked: u32,
    /// The records carrying the `other_target` marker.
    pub other_target_marked: u32,
}

/// Counts one `targets.zrd` member's records and their measured markers.
///
/// The records are read with the pair-shape walk
/// ([`crate::stunts::objective_record_keys`]' rule): the root is the list of
/// records and each record is a list of `[key, value]` pairs. A record that is
/// not a pair list contributes a record and no marker rather than disappearing.
#[must_use]
pub fn count_target_markers(targets: &ZrdValue) -> TargetMarkerCounts {
    let mut counts = TargetMarkerCounts::default();
    for record in targets.as_list().unwrap_or_default() {
        counts.records += 1;
        let Some(children) = record.as_list() else {
            continue;
        };
        let mut has_objective = false;
        let mut has_other = false;
        for child in children {
            let Some(pair) = child.as_list() else {
                continue;
            };
            match pair.first().and_then(ZrdValue::as_text) {
                Some(TARGET_OBJECTIVE_KEY) => has_objective = true,
                Some(TARGET_OTHER_TARGET_KEY) => has_other = true,
                _ => {}
            }
        }
        counts.objective_marked += u32::from(has_objective);
        counts.other_target_marked += u32::from(has_other);
    }
    counts
}

/// The keys of the scenario descriptor surface this module's rule reads, as a
/// queryable list: the mode field and the two fields F42-A's aircraft reader
/// consumes from the descriptor's flat record directly.
///
/// Measured: the eight instant-action descriptors declare all three fields
/// (`mission_type`, `player_plane`, `num_wingmen`) except `ZBD/C2B/IA1`, which
/// declares no `player_plane` and no `num_wingmen` at all — the rule does not
/// require them, only the mode, and this list exists so a consumer can see
/// which fields the surface is built from instead of taking it on trust.
pub const SCENARIO_DESCRIPTOR_KEY_VOCABULARY: [&str; 3] = [
    SCENARIO_MISSION_TYPE_KEY,
    SCENARIO_PLAYER_PLANE_KEY,
    SCENARIO_WINGMEN_KEY,
];
