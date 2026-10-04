//! The retail census of mission control programs: which reader-archive member
//! carries each mission's, what its directives spell, and which of them the
//! engine can honour.
//!
//! Task: `M01-LC-MISSION-PROGRAM` (#630). Shared contract:
//! `docs/contracts/SCRIPT-MISSION.md` ("Source adapter acceptance"). The
//! measurement vocabulary and every refusal live in
//! [`cs_content::mission_control`]; this module is the half that reads the
//! installation.
//!
//! # Why this census exists and what it does not claim
//!
//! F13-B located 1452 programs in the installation and resolved **none** of them:
//! the instruction unit was an assumption, the opcode ledger shipped empty, and
//! the honest report was `0 resolved / 1452 stopped at their first counter`. That
//! search looked for a bytecode mission language inside every script container.
//! For a mission-scoped reader the mission's control program is not that: it is a
//! typed keyed list in one named member, and the member is found by a rule rather
//! than by name — see [`cs_content::mission_control::control_member`].
//!
//! This census therefore answers a narrower, checkable question than F13-B's:
//! **which member is the control program, and what does it declare?** Every
//! number is re-derived from `$CS_GAME_DIR` on each run, so a stale constant
//! fails the acceptance suite instead of passing it.
//!
//! What it does **not** claim:
//!
//! * No original executable has been run. Every directive key is a **spelling**;
//!   what it does is unmeasured, and the two outcome keys are recorded as a
//!   reading of their names (`cs_content::mission_control::terminal_outcome_of`),
//!   not as an observation of behaviour.
//! * A mission is **not** playable because this census measured it.
//!   [`RetailControlRow::is_complete`] is `false` for every measured row and
//!   [`RetailControlCensus::campaign_ready`] is `false` while any row is
//!   incomplete: the record declares directives the engine cannot honour, and a
//!   contract's "the mission remains Unsupported" is the only honest reading.
//! * A member that fails to decode is a **refusal**
//!   ([`ControlCensusError::Decode`]) and fails the whole census, so a
//!   mission cannot vanish from the denominator by having one unreadable member.
//!   The census measures the members production discovery **yields**: a member
//!   the reader index itself refuses to slice is recorded in
//!   `ContainerDiscovery::findings` and never reaches this module, so an archive
//!   that yields no member at all would arrive here as an absent program with
//!   `scanned: 0`. That cannot pass unnoticed: the corpus-level acceptance test
//!   requires every row to have offered candidates, so a reader that parses into
//!   nothing fails the suite instead of being counted as a scenario.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use cs_assets::install::{Discovery, sha256};
use cs_content::mission_control::{
    ControlLowering, ControlMemberError, DecodedMember, MeasuredControlRecord, control_member,
    measure_control_record, objective_blocks_of,
};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_formats::script_raw::mission_scope;
use cs_types::install::RelativePath;

/// The reader archive every mission's control program lives in, as the census
/// selects it: the logical key's last segment, so `zbd/<group>/<mission>/zrdr.zbd`
/// and the root `zbd/zrdr.zbd` are told apart before mission scope is applied.
const MISSION_READER_ARCHIVE: &str = "zrdr.zbd";

/// Why the mission control census could not be produced.
#[derive(Clone, Debug, PartialEq)]
pub enum ControlCensusError {
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
    /// The whole census fails. A member that does not decode is a member whose
    /// directives nobody read, and a census that skipped it would report a
    /// vocabulary smaller than the installation's without saying so.
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
    /// An archive declares **more than one** member carrying numbered objective
    /// blocks, so it has no single control program.
    ///
    /// Measured: zero archives in the owner's installation. Carried because
    /// picking one of two candidates would be a guess about which half drives
    /// the mission, and a census that resolved the ambiguity silently would be
    /// the failure this whole stage exists to prevent.
    AmbiguousControlMember(ControlMemberError),
}

impl fmt::Display for ControlCensusError {
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
        }
    }
}

impl std::error::Error for ControlCensusError {}

/// One member of a reader archive as the census measured it.
#[derive(Clone, Debug, PartialEq)]
pub struct RetailMemberRow {
    /// The member's name, as production discovery spells it.
    pub name: String,
    /// The member's byte offset inside its archive.
    pub offset: u64,
    /// The member's length in bytes.
    pub len: u64,
    /// How many numbered `OBJECTIVE<N>` blocks its decoded record declares.
    pub objective_blocks: u32,
    /// Whether this member **is** the archive's control program.
    pub is_control: bool,
}

/// What a mission-scoped reader archive carries as its control program.
///
/// **Two populations, not one.** Measured over the owner's installation: 40 of the
/// 53 mission-scoped readers declare numbered `OBJECTIVE<N>` blocks and hold a
/// measurable control program; the other 13 declare **none**, and every one of
/// them is an instant-action (`IA1`) or multiplayer (`MP1`–`MP3`) scenario whose
/// reader carries a record-level `objectives.zrd` with no blocks beside it.
///
/// That second population is not a failure of the rule — a scenario with no
/// numbered objectives genuinely has no objective program — and it is not a
/// mission either. It is carried as [`ControlProgram::Absent`] rather than
/// dropped or counted as a campaign mission, because "the archive declares no
/// numbered block" is a **measured fact about a reader someone would otherwise
/// assume is a mission**, and the census has to be able to say it.
#[derive(Clone, Debug, PartialEq)]
pub enum ControlProgram {
    /// The archive declares a control member and it was measured.
    Measured {
        /// The control member's name inside that archive (`objectives.zrd`).
        member: String,
        /// The member's byte offset inside the archive.
        offset: u64,
        /// The member's length in bytes.
        len: u64,
        /// SHA-256 of the member's own bytes.
        sha256: String,
        /// The measured record: blocks, directive sites, the key vocabulary and
        /// the lowering accounting.
        record: MeasuredControlRecord,
    },
    /// The archive declares no member carrying numbered objective blocks.
    Absent {
        /// How many members the archive declared and therefore offered.
        scanned: usize,
    },
}

impl ControlProgram {
    /// The measured record, or `None` for an absent program.
    #[must_use]
    pub const fn record(&self) -> Option<&MeasuredControlRecord> {
        match self {
            Self::Measured { record, .. } => Some(record),
            Self::Absent { .. } => None,
        }
    }

    /// Whether this archive declares a measurable control program.
    #[must_use]
    pub const fn is_measured(&self) -> bool {
        matches!(self, Self::Measured { .. })
    }

    /// The stable label a report carries.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Measured { .. } => "measured",
            Self::Absent { .. } => "absent",
        }
    }
}

/// One mission-scoped reader archive and what it carries as its control program.
#[derive(Clone, Debug, PartialEq)]
pub struct RetailControlRow {
    /// The mission, as `zbd/<group>/<mission>` (F13-B's mission-scope rule).
    pub mission: String,
    /// The reader archive's installation spelling
    /// (`ZBD/<GROUP>/<MISSION>/zrdr.zbd`).
    pub container: String,
    /// SHA-256 of that whole archive, from production discovery.
    pub container_sha256: String,
    /// Every member the archive declares, in member-table order, so a reader can
    /// see the candidates the control rule chose between.
    pub members: Vec<RetailMemberRow>,
    /// What the archive carries as its control program.
    pub program: ControlProgram,
}

impl RetailControlRow {
    /// The mission, as `zbd/<group>/<mission>`.
    #[must_use]
    pub fn mission(&self) -> &str {
        &self.mission
    }

    /// The measured record, or `None` when the archive declares no control
    /// member.
    #[must_use]
    pub fn record(&self) -> Option<&MeasuredControlRecord> {
        self.program.record()
    }

    /// Whether this archive declares a measurable control program.
    #[must_use]
    pub fn is_measured(&self) -> bool {
        self.program.is_measured()
    }

    /// Whether every directive of this archive's control program has an
    /// implemented disposition and every block was read.
    ///
    /// `false` for every measured retail row. See the module documentation for
    /// why that is the correct reading rather than a gap.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.record()
            .is_some_and(MeasuredControlRecord::is_complete)
    }

    /// The record's requirement-by-requirement lowering accounting, or `None` for
    /// an absent program.
    ///
    /// Derived from [`RetailControlRow::record`] on every call rather than stored
    /// beside it: a cached copy could disagree with the record it was built from,
    /// and a gate reading a stale gate is the failure mode this accounting exists
    /// to prevent.
    #[must_use]
    pub fn lowering(&self) -> Option<ControlLowering> {
        self.record().map(MeasuredControlRecord::lowering)
    }
}

/// The measured control programs of every mission-scoped reader archive.
#[derive(Clone, Debug, PartialEq)]
pub struct RetailControlCensus {
    install_sha256: String,
    rows: Vec<RetailControlRow>,
}

impl RetailControlCensus {
    /// SHA-256 of the whole installation manifest, from production discovery.
    #[must_use]
    pub fn install_sha256(&self) -> &str {
        &self.install_sha256
    }

    /// The measured rows, one per mission, sorted by mission.
    #[must_use]
    pub fn rows(&self) -> &[RetailControlRow] {
        &self.rows
    }

    /// The row for one mission, by its `zbd/<group>/<mission>` label.
    #[must_use]
    pub fn row(&self, mission: &str) -> Option<&RetailControlRow> {
        self.rows.iter().find(|row| row.mission == mission)
    }

    /// How many missions the census measured.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the census measured nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The rows whose archive declares a measurable control program.
    ///
    /// The campaign's denominator: every reader that carries numbered objective
    /// blocks. The rows this excludes are the ones
    /// [`Self::archives_without_control_program`] names, and they are excluded
    /// because they declare no program rather than because they were filtered out
    /// of a count.
    pub fn measured_rows(&self) -> impl Iterator<Item = &RetailControlRow> {
        self.rows.iter().filter(|row| row.is_measured())
    }

    /// The rows whose archive declares **no** member carrying numbered objective
    /// blocks, with the mission labels.
    ///
    /// Measured over the installation: 13 of 53, every one of them an
    /// instant-action (`IA1`) or multiplayer (`MP1`–`MP3`) scenario. Kept as a
    /// positive name so the census can report them rather than let a reader
    /// assume a mission path always means an objective program.
    #[must_use]
    pub fn archives_without_control_program(&self) -> Vec<&str> {
        self.rows
            .iter()
            .filter(|row| !row.is_measured())
            .map(|row| row.mission.as_str())
            .collect()
    }

    /// How many archives declare a measurable control program.
    #[must_use]
    pub fn measured_len(&self) -> usize {
        self.measured_rows().count()
    }

    /// How many numbered blocks the measured control programs declare in total.
    #[must_use]
    pub fn blocks(&self) -> u32 {
        self.measured_rows()
            .map(|row| {
                row.record()
                    .expect("a measured row carries a record")
                    .blocks()
            })
            .sum()
    }

    /// How many directive sites the measured control programs declare in total.
    #[must_use]
    pub fn sites(&self) -> u32 {
        self.measured_rows()
            .map(|row| {
                row.record()
                    .expect("a measured row carries a record")
                    .sites()
            })
            .sum()
    }

    /// The union of every measured directive key, sorted, with the total number
    /// of sites each carries.
    ///
    /// Published beside every classification so a reader can see the vocabulary a
    /// family was drawn from instead of taking it on trust — the same reason
    /// `RetailObjectiveCensus::vocabulary` publishes its own.
    #[must_use]
    pub fn vocabulary(&self) -> Vec<(String, u32)> {
        let mut totals: BTreeMap<String, u32> = BTreeMap::new();
        for row in self.measured_rows() {
            for key in row
                .record()
                .expect("a measured row carries a record")
                .keys()
            {
                *totals.entry(key.key.clone()).or_insert(0) += key.sites;
            }
        }
        totals.into_iter().collect()
    }

    /// The distinct directive keys, sorted.
    #[must_use]
    pub fn directive_keys(&self) -> Vec<&str> {
        let keys: BTreeSet<&str> = self
            .measured_rows()
            .flat_map(|row| {
                row.record()
                    .expect("a measured row carries a record")
                    .keys()
            })
            .map(|key| key.key.as_str())
            .collect();
        keys.into_iter().collect()
    }

    /// Every unmet lowering requirement, keyed by its requirement kind and the
    /// mission that raised it.
    ///
    /// The corpus-wide view of [`RetailControlRow::lowering`]: every measured
    /// mission is listed under every requirement it does not meet, so a reader
    /// sees which requirements block *all* missions and which block only some.
    #[must_use]
    pub fn unmet_by_requirement(&self) -> BTreeMap<&'static str, Vec<String>> {
        let mut unmet: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
        for row in self.measured_rows() {
            for requirement in row
                .lowering()
                .expect("a measured row carries a record")
                .unmet()
            {
                unmet
                    .entry(requirement.kind.code())
                    .or_default()
                    .push(row.mission.clone());
            }
        }
        unmet
    }

    /// Every unmeasured directive field the census names, deduplicated and
    /// sorted.
    #[must_use]
    pub fn unmeasured_fields(&self) -> Vec<String> {
        let mut fields: BTreeSet<String> = BTreeSet::new();
        for row in self.measured_rows() {
            fields.extend(
                row.lowering()
                    .expect("a measured row carries a record")
                    .unmeasured_fields(),
            );
        }
        fields.into_iter().collect()
    }

    /// The missions whose control program is complete.
    ///
    /// Empty for the measured installation. Kept as a method so a reader has a
    /// positive name for the gate: a corpus with a complete row would report it
    /// here, and today it reports none.
    #[must_use]
    pub fn complete_missions(&self) -> Vec<&str> {
        self.rows
            .iter()
            .filter(|row| row.is_complete())
            .map(|row| row.mission.as_str())
            .collect()
    }

    /// Whether every mission-scoped reader carries a complete control program.
    ///
    /// The campaign gate. It asks about **every** row, not only the measured
    /// ones, for two reasons: a reader with no control program at all is not a
    /// mission the engine can run, and a gate that only looked at the rows it
    /// found would answer "ready" for a corpus whose missing half nobody read. An
    /// **empty** census answers `false` too, so a run that measured nothing cannot
    /// report itself ready — the same fail-closed rule
    /// `cs_script::bindings::observed::ObservedCoverage::complete` uses.
    #[must_use]
    pub fn campaign_ready(&self) -> bool {
        !self.rows.is_empty()
            && self.measured_len() == self.rows.len()
            && self.complete_missions().len() == self.rows.len()
    }
}

/// Measures the control program of every mission-scoped reader archive in
/// `install_root`, sorted by mission.
///
/// Mission scope is F13-B's rule — exactly `zbd/<group>/<mission>` — so the
/// world-group readers (`zbd/c1c/zrdr.zbd`) and the root reader are not
/// missions and are not measured as any.
///
/// # Errors
///
/// [`ControlCensusError`] for an undiscoverable installation, an unreadable
/// archive, a member that does not decode, or an archive that declares no single
/// control member. Every one fails the whole census: each is a member or a
/// mission whose directives nobody read.
pub fn survey_mission_control_programs(
    install_root: &Path,
) -> Result<RetailControlCensus, ControlCensusError> {
    let found = cs_assets::install::discover(install_root)
        .map_err(|error| ControlCensusError::Discovery(error.to_string()))?;
    let install_sha256 = cs_assets::install::fingerprint(&found.manifest).to_hex();

    let mut rows = Vec::new();
    for record in &found.manifest.files {
        let container_key = record.relative_spelling.logical_key();
        if !container_key.ends_with(MISSION_READER_ARCHIVE) {
            continue;
        }
        let spelling = record.relative_spelling.as_str().to_owned();
        let path = RelativePath::new(&spelling.to_lowercase()).map_err(|error| {
            ControlCensusError::Read {
                container: container_key.clone(),
                reason: error.to_string(),
            }
        })?;
        let Some(mission) = mission_scope(&path) else {
            continue;
        };
        rows.push(measure_archive(
            &mission,
            &container_key,
            &spelling,
            &record.sha256.to_hex(),
            &found,
            &path,
        )?);
    }

    rows.sort_by(|left, right| left.mission.cmp(&right.mission));
    Ok(RetailControlCensus {
        install_sha256,
        rows,
    })
}

/// Reads one reader archive, finds its control member by the rule and measures it.
///
/// The **whole** member list is decoded, not only the control member, because the
/// rule needs something to choose between: a census that looked for
/// `objectives.zrd` by name and reported success would pass on an installation
/// where that member carries no blocks at all.
fn measure_archive(
    mission: &str,
    container_key: &str,
    spelling: &str,
    container_sha256: &str,
    found: &Discovery,
    path: &RelativePath,
) -> Result<RetailControlRow, ControlCensusError> {
    let bytes = std::fs::read(found.manifest.host_root.join(spelling)).map_err(|error| {
        ControlCensusError::Read {
            container: container_key.to_owned(),
            reason: error.to_string(),
        }
    })?;
    let discovery = cs_formats::script_raw::discover_container(container_key, path, &bytes);

    let mut decoded: Vec<(DecodedMember, RetailMemberRow)> = Vec::new();
    for program in discovery.programs() {
        let locator = program.locator();
        let Some(name) = locator.member() else {
            // A program with no member name is the whole container (an animation
            // payload), which no reader archive's control rule considers. It is
            // not silently dropped: `discovery` reports it, and this census only
            // ever looks at member-named programs, so a reader archive whose
            // control program turned out to be unnamed finds no control member
            // and is refused by `control_member`.
            continue;
        };
        let document = decode_zrd(program.bytes()).map_err(|error| ControlCensusError::Decode {
            container: container_key.to_owned(),
            member: name.to_owned(),
            code: error.code(),
            offset: error.offset(),
        })?;
        let member = DecodedMember::new(name.to_owned(), document);
        let span = locator.span();
        let row = RetailMemberRow {
            name: name.to_owned(),
            offset: span.offset,
            len: span.len,
            objective_blocks: objective_blocks_of(&member),
            is_control: false,
        };
        decoded.push((member, row));
    }

    let only: Vec<DecodedMember> = decoded.iter().map(|(member, _)| member.clone()).collect();

    // The absent case is a **measured** outcome, not an error: measured over the
    // installation, 13 of the 53 mission-scoped readers declare no member
    // carrying numbered objective blocks, and every one of them is an
    // instant-action or multiplayer scenario. Refusing the whole census for a
    // scenario with no objective program would report nothing about the 40
    // archives that do have one, so the row is carried with its member list and
    // its absence, and the campaign gate counts it as not-ready.
    let members: Vec<RetailMemberRow> = decoded
        .iter()
        .map(|(_, row)| RetailMemberRow {
            is_control: false,
            ..row.clone()
        })
        .collect();
    let control = match control_member(container_key, &only) {
        Ok(control) => control,
        // One measured outcome: the archive declares no objective program. Every
        // instant-action and multiplayer reader in the installation looks like
        // this, and the census reports each of them rather than failing.
        Err(ControlMemberError::NoControlMember { .. }) => {
            return Ok(RetailControlRow {
                mission: mission.to_owned(),
                container: spelling.to_owned(),
                container_sha256: container_sha256.to_owned(),
                members,
                program: ControlProgram::Absent {
                    scanned: decoded.len(),
                },
            });
        }
        // The other is a refusal: two candidates, no evidence to choose.
        Err(error) => return Err(ControlCensusError::AmbiguousControlMember(error)),
    };

    let control_index = decoded
        .iter()
        .position(|(member, _)| member.name == control.name)
        .expect("the control member came from this list");
    let (_, control_row) = &decoded[control_index];
    let control_bytes = slice_member(&bytes, control_row.offset, control_row.len, container_key)?;
    let members = decoded
        .iter()
        .map(|(_, row)| RetailMemberRow {
            is_control: row.name == control.name,
            ..row.clone()
        })
        .collect();

    Ok(RetailControlRow {
        mission: mission.to_owned(),
        container: spelling.to_owned(),
        container_sha256: container_sha256.to_owned(),
        members,
        program: ControlProgram::Measured {
            member: control.name.clone(),
            offset: control_row.offset,
            len: control_row.len,
            sha256: sha256(&control_bytes).to_hex(),
            record: measure_control_record(&control.document),
        },
    })
}

/// The archive bytes of one member's located extent.
fn slice_member(
    archive: &[u8],
    offset: u64,
    len: u64,
    container: &str,
) -> Result<Vec<u8>, ControlCensusError> {
    let start = usize::try_from(offset).map_err(|_| ControlCensusError::Read {
        container: container.to_owned(),
        reason: format!("member offset {offset} does not fit an address"),
    })?;
    let end = start
        .checked_add(usize::try_from(len).map_err(|_| ControlCensusError::Read {
            container: container.to_owned(),
            reason: format!("member length {len} does not fit an address"),
        })?)
        .ok_or_else(|| ControlCensusError::Read {
            container: container.to_owned(),
            reason: "member extent overflows".to_owned(),
        })?;
    archive
        .get(start..end)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| ControlCensusError::Read {
            container: container.to_owned(),
            reason: format!("member extent {start}..{end} is outside the archive"),
        })
}

/// The decoded control member of one archive, for a caller that wants the
/// document rather than the measurement.
///
/// Not used by the census itself, which measures the document in place; it exists
/// so an acceptance test can read the **same** member the rule chose without
/// repeating the rule. It re-reads and re-decodes, which is a deliberate
/// simplicity trade rather than a cache.
#[must_use]
pub fn read_control_member(
    install_root: &Path,
    mission: &str,
) -> Option<(ZrdValue, RetailMemberRow)> {
    let found = cs_assets::install::discover(install_root).ok()?;
    for record in &found.manifest.files {
        let container_key = record.relative_spelling.logical_key();
        if !container_key.ends_with(MISSION_READER_ARCHIVE) {
            continue;
        }
        let spelling = record.relative_spelling.as_str();
        let path = RelativePath::new(&spelling.to_lowercase()).ok()?;
        if mission_scope(&path).as_deref() != Some(mission) {
            continue;
        }
        let bytes = std::fs::read(found.manifest.host_root.join(spelling)).ok()?;
        let discovery = cs_formats::script_raw::discover_container(&container_key, &path, &bytes);
        let mut decoded = Vec::new();
        for program in discovery.programs() {
            let Some(name) = program.locator().member() else {
                continue;
            };
            let Ok(document) = decode_zrd(program.bytes()) else {
                continue;
            };
            let span = program.locator().span();
            let member = DecodedMember::new(name.to_owned(), document);
            decoded.push((
                member.clone(),
                RetailMemberRow {
                    name: name.to_owned(),
                    offset: span.offset,
                    len: span.len,
                    objective_blocks: objective_blocks_of(&member),
                    is_control: false,
                },
            ));
        }
        let only: Vec<DecodedMember> = decoded.iter().map(|(member, _)| member.clone()).collect();
        let control = control_member(&container_key, &only).ok()?;
        let index = decoded
            .iter()
            .position(|(member, _)| member.name == control.name)?;
        // The row describes the member the rule chose, so it carries what the rule
        // measured about it: a row reporting zero blocks for the control member
        // would contradict the very selection this function performed.
        let mut row = decoded[index].1.clone();
        row.is_control = true;
        return Some((decoded[index].0.document.clone(), row));
    }
    None
}
