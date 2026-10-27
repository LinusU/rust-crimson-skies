//! The retail census of **scenario** control surfaces: which member of a
//! reader archive that declares no numbered objective block carries its
//! control data, and what that surface spells.
//!
//! Task: `M01-LC-SCENARIOS` (#642). Shared contract:
//! `docs/contracts/SCRIPT-MISSION.md`. The rule and every measured shape live
//! in [`cs_content::scenario_control`]; this module is the half that reads the
//! installation, and it answers the question
//! `cs_app::mission_control`'s census left open: its 13 `ControlProgram::Absent`
//! rows are instant-action and multiplayer scenarios, and this census locates
//! each one's control surface **by contents**, exactly as that census locates a
//! mission's control member.
//!
//! # Population: the same 13, derived by the same rule
//!
//! The census walks every mission-scoped reader archive (F13-B's
//! `zbd/<group>/<mission>` scope) and applies
//! [`cs_content::mission_control::control_member`] to it — the same block rule
//! #630 measured with. An archive that declares a block-bearing member is a
//! **campaign-mission reader** and produces no scenario row; the archive that
//! declares none produces one. The two censuses therefore cannot disagree
//! about the population, and the acceptance suite reconciles this census's
//! rows against
//! [`crate::mission_control::RetailControlCensus::archives_without_control_program`]
//! on every retail run.
//!
//! **An instant-action scenario is not a campaign mission.** A scenario row
//! never counts toward the mission denominator, never lowers to a
//! `MissionProgram` and never changes `campaign_ready()`: the two populations
//! exist to prevent exactly the fold-in this census refuses to make.
//!
//! # What each row measures
//!
//! * The scenario control member, selected by
//!   [`cs_content::scenario_control::scenario_control_member`] (the unique
//!   member declaring the measured scenario descriptor), or the measured
//!   absence of one (the multiplayer readers declare none anywhere).
//! * The record-level keys of the archive's `objectives.zrd`, with keys
//!   outside #630's measured record vocabulary kept **unclassified**.
//! * The archive's `net.zrd`, read only as the measured bare spawn-point
//!   shape, and the objective/other-target markers of its `targets.zrd`.
//! * The declared zeppelin records of its `zeppelins.zrd`, counted only — the
//!   field vocabulary of a placed zeppelin is F35's and T574's measurement,
//!   not re-read here.
//!
//! What it does **not** claim: no original executable has been run. Every
//! figure is a declaration measured from bytes; what a scenario mode, a spawn
//! point or an `objective` marker does at runtime is unmeasured and is named
//! as such in `docs/findings/2026-10-13-m01-lc-scenarios.md`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use cs_assets::install::{Discovery, sha256};
use cs_content::mission_control::{
    ControlMemberError, DecodedMember, control_member, objective_blocks_of,
};
use cs_content::scenario_control::{
    MeasuredScenarioDescriptor, NetSpawnSurface, ScenarioMemberError, ScenarioObjectiveRecord,
    TargetMarkerCounts, count_target_markers, measure_net_spawns,
    measure_scenario_objective_record, scenario_control_member,
};
use cs_content::stunts::{ZrdValue, decode_zrd, scenario_mission_type};
use cs_formats::script_raw::mission_scope;
use cs_types::install::RelativePath;

/// The reader archive every scenario row is measured from, as the census
/// selects it: the logical key's last segment (the same selection
/// `cs_app::mission_control` uses).
const MISSION_READER_ARCHIVE: &str = "zrdr.zbd";

/// The member a scenario's record-level objective keys are read from.
const OBJECTIVES_MEMBER: &str = "objectives.zrd";

/// The member a scenario's target records are read from.
const TARGETS_MEMBER: &str = "targets.zrd";

/// The member a scenario's bare spawn-point list is read from.
const NET_MEMBER: &str = "net.zrd";

/// The member a scenario's placed-zeppelin records are read from.
const ZEPPELINS_MEMBER: &str = "zeppelins.zrd";

/// Why the scenario control census could not be produced.
#[derive(Clone, Debug, PartialEq)]
pub enum ScenarioCensusError {
    /// The installation could not be discovered.
    Discovery(String),
    /// A reader archive could not be read from disk.
    Read {
        /// The archive's logical key.
        container: String,
        /// Why the read failed.
        reason: String,
    },
    /// A member of a reader archive did not decode as `.zrd`.
    ///
    /// The whole census fails, exactly as `cs_app::mission_control`'s does: a
    /// member that does not decode is a member whose surface nobody measured.
    Decode {
        /// The archive's logical key.
        container: String,
        /// The member's name.
        member: String,
        /// The decoder's refusal code.
        code: &'static str,
        /// The offset of the refusal inside the member.
        offset: u64,
    },
    /// An archive declares **more than one** member carrying numbered
    /// objective blocks, so it is not a scenario archive at all.
    ///
    /// Measured: zero archives in the owner's installation. Carried so this
    /// census and `cs_app::mission_control` refuse the same archive for the
    /// same reason rather than one of them resolving it silently.
    AmbiguousControlMember(ControlMemberError),
    /// An archive declares **more than one** scenario descriptor member.
    ///
    /// Measured: zero archives in the owner's installation.
    AmbiguousScenarioMember(ScenarioMemberError),
}

impl fmt::Display for ScenarioCensusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(reason) => {
                write!(f, "the installation could not be discovered: {reason}")
            }
            Self::Read { container, reason } => {
                write!(f, "reader archive {container} could not be read: {reason}")
            }
            Self::Decode {
                container,
                member,
                code,
                offset,
            } => write!(
                f,
                "{container}'s member {member} did not decode: {code} at offset {offset}"
            ),
            Self::AmbiguousControlMember(error) => write!(f, "{error}"),
            Self::AmbiguousScenarioMember(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ScenarioCensusError {}

/// One member of a scenario reader archive, as the census measured it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioMemberRow {
    /// The member's name, as production discovery spells it.
    pub name: String,
    /// The member's byte offset inside its archive.
    pub offset: u64,
    /// The member's length in bytes.
    pub len: u64,
    /// How many numbered `OBJECTIVE<N>` blocks its decoded record declares.
    ///
    /// Measured: **0** for every member of every scenario row — that is the
    /// rule by which this census's population is defined. The count travels on
    /// the row so a reader can see the measured fact rather than trust the
    /// filter.
    pub objective_blocks: u32,
}

/// What one scenario reader archive carries as its control surface.
#[derive(Clone, Debug, PartialEq)]
pub enum ScenarioProgram {
    /// The archive declares exactly one scenario descriptor member, and the
    /// rule selected it.
    Descriptor {
        /// The selected member's name inside the archive.
        member: String,
        /// The member's byte offset inside the archive.
        offset: u64,
        /// The member's length in bytes.
        len: u64,
        /// SHA-256 of the member's own bytes.
        sha256: String,
        /// The measured descriptor: the declared mode, every flat key with its
        /// shape, the declared aircraft, zone bindings and per-mode spawn
        /// lists.
        descriptor: Box<MeasuredScenarioDescriptor>,
    },
    /// The archive declares **no** member carrying the scenario descriptor
    /// surface anywhere in its member list.
    ///
    /// Measured: the five multiplayer readers. Their control data is a
    /// different, thinner surface, and each row measures its three halves
    /// beside this absence: the record-level `objectives.zrd` keys, the bare
    /// `net.zrd` spawn list and the objective-marked `targets.zrd` records.
    NoDescriptor {
        /// How many members the archive declared and therefore offered.
        scanned: usize,
    },
}

impl ScenarioProgram {
    /// The measured descriptor, or `None` for an archive with none.
    #[must_use]
    pub const fn descriptor(&self) -> Option<&MeasuredScenarioDescriptor> {
        match self {
            Self::Descriptor { descriptor, .. } => Some(descriptor),
            Self::NoDescriptor { .. } => None,
        }
    }

    /// The stable label a report carries.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Descriptor { .. } => "descriptor",
            Self::NoDescriptor { .. } => "no-descriptor",
        }
    }
}

/// One scenario reader archive and everything this census measured about it.
#[derive(Clone, Debug, PartialEq)]
pub struct RetailScenarioRow {
    /// The scenario, as `zbd/<group>/<mission>` (F13-B's scope rule).
    pub mission: String,
    /// The reader archive's installation spelling.
    pub container: String,
    /// SHA-256 of that whole archive, from production discovery.
    pub container_sha256: String,
    /// Every member the archive declares, in member-table order, so a reader
    /// sees the candidates the rule chose between.
    pub members: Vec<ScenarioMemberRow>,
    /// What the archive carries as its scenario control surface.
    pub program: ScenarioProgram,
    /// The record-level keys of the archive's `objectives.zrd`, or `None` when
    /// it declares no such member.
    pub record_objective: Option<ScenarioObjectiveRecord>,
    /// The archive's `net.zrd`, read only as the measured bare spawn-point
    /// shape.
    pub net_spawns: NetSpawnSurface,
    /// The markers of the archive's `targets.zrd`, or `None` when it declares
    /// no such member.
    pub targets: Option<TargetMarkerCounts>,
    /// How many placed-zeppelin records the archive's `zeppelins.zrd`
    /// declares, or `None` when it declares no such member. Counted only; the
    /// field vocabulary is F35's and T574's measurement.
    pub zeppelin_records: Option<u32>,
}

impl RetailScenarioRow {
    /// The scenario, as `zbd/<group>/<mission>`.
    #[must_use]
    pub fn mission(&self) -> &str {
        &self.mission
    }

    /// Whether the archive declares a scenario descriptor member.
    #[must_use]
    pub fn has_descriptor(&self) -> bool {
        matches!(self.program, ScenarioProgram::Descriptor { .. })
    }
}

/// The measured scenario control surfaces of every reader archive that
/// declares no numbered objective block.
#[derive(Clone, Debug, PartialEq)]
pub struct RetailScenarioCensus {
    install_sha256: String,
    rows: Vec<RetailScenarioRow>,
    block_bearing_readers: u32,
    descriptor_sites_outside_scenarios: Vec<String>,
    net_readers: Vec<(String, NetSpawnSurface)>,
}

impl RetailScenarioCensus {
    /// SHA-256 of the whole installation manifest, from production discovery.
    #[must_use]
    pub fn install_sha256(&self) -> &str {
        &self.install_sha256
    }

    /// The measured rows, one per scenario reader, sorted by scenario.
    #[must_use]
    pub fn rows(&self) -> &[RetailScenarioRow] {
        &self.rows
    }

    /// The row for one scenario, by its `zbd/<group>/<mission>` label.
    #[must_use]
    pub fn row(&self, mission: &str) -> Option<&RetailScenarioRow> {
        self.rows.iter().find(|row| row.mission == mission)
    }

    /// How many scenario readers the census measured.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the census measured nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The rows whose archive declares exactly one scenario descriptor member.
    pub fn descriptor_rows(&self) -> impl Iterator<Item = &RetailScenarioRow> {
        self.rows.iter().filter(|row| row.has_descriptor())
    }

    /// The rows whose archive declares **no** scenario descriptor member, with
    /// the scenario labels — the measured absence, kept as a positive name so
    /// a reader can report it instead of assuming every scenario carries a
    /// descriptor.
    #[must_use]
    pub fn archives_without_descriptor(&self) -> Vec<&str> {
        self.rows
            .iter()
            .filter(|row| !row.has_descriptor())
            .map(|row| row.mission.as_str())
            .collect()
    }

    /// How many mission-scoped readers the same walk measured as carrying a
    /// numbered-block control member — the campaign-mission population, which
    /// this census does not re-measure and does not widen.
    ///
    /// Measured over the owner's installation: 40, matching
    /// `cs_app::mission_control`'s census on every retail run.
    #[must_use]
    pub fn block_bearing_readers(&self) -> u32 {
        self.block_bearing_readers
    }

    /// Members of block-bearing (campaign-mission) readers that declare the
    /// scenario descriptor surface, as `archive/member` labels.
    ///
    /// The corpus-wide uniqueness check behind the rule: measured over the
    /// owner's installation, **empty** — no campaign-mission reader declares a
    /// `mission_type`-carrying member anywhere, so the descriptor rule cannot
    /// misidentify a mission's member even if it were applied to one.
    #[must_use]
    pub fn descriptor_sites_outside_scenarios(&self) -> &[String] {
        &self.descriptor_sites_outside_scenarios
    }

    /// Every mission-scoped reader (both populations) that declares a
    /// `net.zrd`, with what its measured shape says, sorted by scenario.
    ///
    /// The corpus-wide statement about `net.zrd`: it is the same bare
    /// spawn-point surface wherever it appears, campaign readers included, and
    /// a member in any other shape is reported as
    /// [`NetSpawnSurface::UnmeasuredShape`] rather than read as spawn points.
    #[must_use]
    pub fn net_readers(&self) -> &[(String, NetSpawnSurface)] {
        &self.net_readers
    }

    /// Every record-level `objectives.zrd` key the scenario rows declare,
    /// with the number of rows carrying it, sorted by key.
    ///
    /// Published beside the classification so a reader sees the whole
    /// vocabulary — including the keys outside the measured record vocabulary
    /// — instead of taking the classification on trust.
    #[must_use]
    pub fn record_key_vocabulary(&self) -> Vec<(String, u32)> {
        let mut totals: BTreeMap<String, u32> = BTreeMap::new();
        for row in &self.rows {
            if let Some(record) = &row.record_objective {
                for (key, sites) in record.keys() {
                    *totals.entry(key.clone()).or_insert(0) += sites;
                }
            }
        }
        totals.into_iter().collect()
    }

    /// The record-level keys the scenario rows declare that fall **outside**
    /// the measured record vocabulary, sorted and deduplicated.
    ///
    /// Measured over the owner's installation: the multiplayer-only spellings
    /// `OBJECTIVE_DELAY`, `PLAY_LOSS_ANIM_WHEN_MAVERICK_ON_RUNWAY`,
    /// `READ_TIME` and `REVIEW_DELAY`. Kept as unclassified rather than
    /// absorbed into a documented field.
    #[must_use]
    pub fn unclassified_record_keys(&self) -> Vec<String> {
        let mut keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for row in &self.rows {
            if let Some(record) = &row.record_objective {
                keys.extend(record.unclassified().iter().cloned());
            }
        }
        keys.into_iter().collect()
    }
}

/// Measures the scenario control surface of every reader archive in
/// `install_root` that declares no numbered objective block, sorted by
/// scenario.
///
/// Mission scope is F13-B's rule — exactly `zbd/<group>/<mission>` — so the
/// world-group readers and the root reader are not scenarios and are not
/// measured as any. Every member of every archive is decoded with the
/// production `.zrd` reader before any rule is applied, so an archive cannot
/// vanish from the denominator by having one unreadable member.
///
/// # Errors
///
/// [`ScenarioCensusError`] for an undiscoverable installation, an unreadable
/// archive, a member that does not decode, an archive with two block-bearing
/// members or an archive with two scenario descriptors. Every one fails the
/// whole census: each is a member whose surface nobody measured, or an
/// ambiguity this census refuses to resolve.
pub fn survey_scenario_control_programs(
    install_root: &Path,
) -> Result<RetailScenarioCensus, ScenarioCensusError> {
    let found = cs_assets::install::discover(install_root)
        .map_err(|error| ScenarioCensusError::Discovery(error.to_string()))?;
    survey_scenario_control(&found)
}

/// The census walk over an already-discovered installation.
///
/// Split from [`survey_scenario_control_programs`] so the acceptance suite can
/// drive the same production walk on an authored installation tree as well as
/// on the owner's.
///
/// # Errors
///
/// See [`survey_scenario_control_programs`].
pub fn survey_scenario_control(
    found: &Discovery,
) -> Result<RetailScenarioCensus, ScenarioCensusError> {
    let install_sha256 = cs_assets::install::fingerprint(&found.manifest).to_hex();
    let mut rows = Vec::new();
    let mut block_bearing_readers = 0_u32;
    let mut descriptor_sites_outside_scenarios = Vec::new();
    let mut net_readers = Vec::new();

    for record in &found.manifest.files {
        let container_key = record.relative_spelling.logical_key();
        if !container_key.ends_with(MISSION_READER_ARCHIVE) {
            continue;
        }
        let spelling = record.relative_spelling.as_str().to_owned();
        let path = RelativePath::new(&spelling.to_lowercase()).map_err(|error| {
            ScenarioCensusError::Read {
                container: container_key.clone(),
                reason: error.to_string(),
            }
        })?;
        let Some(mission) = mission_scope(&path) else {
            continue;
        };
        let bytes = std::fs::read(found.manifest.host_root.join(&spelling)).map_err(|error| {
            ScenarioCensusError::Read {
                container: container_key.clone(),
                reason: error.to_string(),
            }
        })?;
        let Some(measured) = measure_scenario_reader(
            &mission,
            &container_key,
            &spelling,
            &record.sha256.to_hex(),
            &path,
            &bytes,
        )?
        else {
            // A campaign-mission reader: the block rule found its control
            // member. It still contributes the two corpus-wide cross-checks —
            // whether any of its members declares a scenario descriptor, and
            // what its `net.zrd` declares.
            for (name, document) in decoded_members(&container_key, &path, &bytes)? {
                if scenario_mission_type(&document).is_some() {
                    descriptor_sites_outside_scenarios.push(format!("{mission}/{name}"));
                }
                if name == NET_MEMBER {
                    net_readers.push((mission.clone(), measure_net_spawns(&document)));
                }
            }
            block_bearing_readers += 1;
            continue;
        };
        if !matches!(measured.net_spawns, NetSpawnSurface::Absent) {
            net_readers.push((measured.mission.clone(), measured.net_spawns.clone()));
        }
        rows.push(measured);
    }

    rows.sort_by(|left, right| left.mission.cmp(&right.mission));
    net_readers.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(RetailScenarioCensus {
        install_sha256,
        rows,
        block_bearing_readers,
        descriptor_sites_outside_scenarios,
        net_readers,
    })
}

/// Every member-named program of one archive, decoded, in member-table order.
///
/// A program with no member name is the whole container (an animation
/// payload); no reader-archive rule here considers one, and `discovery`
/// reports it, so it is not silently dropped from the census's view — it is
/// simply never a candidate.
fn decoded_members(
    container_key: &str,
    path: &RelativePath,
    bytes: &[u8],
) -> Result<Vec<(String, ZrdValue)>, ScenarioCensusError> {
    let discovery = cs_formats::script_raw::discover_container(container_key, path, bytes);
    let mut members = Vec::new();
    for program in discovery.programs() {
        let Some(name) = program.locator().member() else {
            continue;
        };
        let document =
            decode_zrd(program.bytes()).map_err(|error| ScenarioCensusError::Decode {
                container: container_key.to_owned(),
                member: name.to_owned(),
                code: error.code(),
                offset: error.offset(),
            })?;
        members.push((name.to_owned(), document));
    }
    Ok(members)
}

/// One archive's measurement: its scenario row, or `None` when the archive is
/// a campaign-mission reader (it declares a numbered-block control member).
///
/// The walk's unit, public so the acceptance suite can drive it on authored
/// archive bytes: the same production discovery, the same block rule and the
/// same descriptor rule the retail census runs.
///
/// # Errors
///
/// [`ScenarioCensusError::Decode`] for a member that does not decode,
/// [`ScenarioCensusError::AmbiguousControlMember`] for two block-bearing
/// members and [`ScenarioCensusError::AmbiguousScenarioMember`] for two
/// descriptors.
pub fn measure_scenario_reader(
    mission: &str,
    container_key: &str,
    spelling: &str,
    container_sha256: &str,
    path: &RelativePath,
    bytes: &[u8],
) -> Result<Option<RetailScenarioRow>, ScenarioCensusError> {
    let discovery = cs_formats::script_raw::discover_container(container_key, path, bytes);
    let mut decoded: Vec<DecodedMember> = Vec::new();
    let mut spans: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for program in discovery.programs() {
        let Some(name) = program.locator().member() else {
            continue;
        };
        let document =
            decode_zrd(program.bytes()).map_err(|error| ScenarioCensusError::Decode {
                container: container_key.to_owned(),
                member: name.to_owned(),
                code: error.code(),
                offset: error.offset(),
            })?;
        let span = program.locator().span();
        spans.insert(name.to_owned(), (span.offset, span.len));
        decoded.push(DecodedMember::new(name.to_owned(), document));
    }

    match control_member(container_key, &decoded) {
        Ok(_) => {
            // A campaign-mission reader, measured by #630's rule. Not a
            // scenario row: this census never widens the mission population.
            return Ok(None);
        }
        Err(ControlMemberError::NoControlMember { .. }) => {}
        Err(error) => return Err(ScenarioCensusError::AmbiguousControlMember(error)),
    }

    let program = match scenario_control_member(&decoded)? {
        Some(descriptor) => {
            let (offset, len) = spans[&descriptor.name];
            let start = usize::try_from(offset).map_err(|_| ScenarioCensusError::Read {
                container: container_key.to_owned(),
                reason: format!("member offset {offset} does not fit an address"),
            })?;
            let end = start.saturating_add(len as usize);
            let slice = bytes
                .get(start..end)
                .ok_or_else(|| ScenarioCensusError::Read {
                    container: container_key.to_owned(),
                    reason: format!("member extent {start}..{end} is outside the archive"),
                })?;
            ScenarioProgram::Descriptor {
                member: descriptor.name.clone(),
                offset,
                len,
                sha256: sha256(slice).to_hex(),
                descriptor: Box::new(cs_content::scenario_control::measure_scenario_descriptor(
                    &descriptor.document,
                )),
            }
        }
        None => ScenarioProgram::NoDescriptor {
            scanned: decoded.len(),
        },
    };

    let members = decoded
        .iter()
        .map(|member| {
            let (offset, len) = spans[&member.name];
            ScenarioMemberRow {
                name: member.name.clone(),
                offset,
                len,
                objective_blocks: objective_blocks_of(member),
            }
        })
        .collect();

    let by_name: BTreeMap<&str, &ZrdValue> = decoded
        .iter()
        .map(|member| (member.name.as_str(), &member.document))
        .collect();

    let record_objective = by_name
        .get(OBJECTIVES_MEMBER)
        .map(|document| measure_scenario_objective_record(document));
    let net_spawns = by_name
        .get(NET_MEMBER)
        .map_or(NetSpawnSurface::Absent, |document| {
            measure_net_spawns(document)
        });
    let targets = by_name
        .get(TARGETS_MEMBER)
        .map(|document| count_target_markers(document));
    let zeppelin_records = by_name.get(ZEPPELINS_MEMBER).map(|document| {
        document
            .as_list()
            .and_then(|root| root.first())
            .and_then(ZrdValue::as_list)
            .map_or(0, <[ZrdValue]>::len) as u32
    });

    Ok(Some(RetailScenarioRow {
        mission: mission.to_owned(),
        container: spelling.to_owned(),
        container_sha256: container_sha256.to_owned(),
        members,
        program,
        record_objective,
        net_spawns,
        targets,
        zeppelin_records,
    }))
}

/// Maps a [`ScenarioMemberError`] into the census's error type, so the rule's
/// refusal surfaces with the archive it was raised in rather than bare.
impl From<ScenarioMemberError> for ScenarioCensusError {
    fn from(error: ScenarioMemberError) -> Self {
        Self::AmbiguousScenarioMember(error)
    }
}
