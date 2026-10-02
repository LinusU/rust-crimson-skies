//! The original multiplayer catalog (F56-A): which modes the installation
//! names and which scenario slots it ships.
//!
//! Spec: `specs/F56-original-multiplayer-scenarios-and-mode-rules.md`, stage
//! `### F56-A`. Findings and what stays open:
//! `docs/findings/2026-10-02-f56-a-multiplayer-catalog.md`.
//!
//! Two independent tables are read, both from bytes of the installation and
//! neither from a count or a name assumed beforehand:
//!
//! * **Modes** ([`discover_modes`]): the localized string table names the
//!   selectable modes in one run of rows ([`MODE_NAME_IDS`]) and describes each
//!   in a briefing block of the multiplayer family ([`BRIEFING_FIRST_ID`],
//!   [`BRIEFING_STRIDE`]). A block belongs to the family while its third row
//!   reads `POINTS`; the walk ends at the first block that does not, and that
//!   boundary row is reported. A name with no briefing, or a briefing with no
//!   name, is a *gap* in the catalog, never dropped.
//! * **Scenario slots** ([`discover_slots`]): every `ZBD/<group>/MP<n>/`
//!   directory is one slot, identified by the world group and the slot number,
//!   with the digest of its reader archive and the markers found in its bytes.
//!
//! # What is known and what is not
//!
//! The mode *names* and the briefing text (including the point values the
//! briefing prints) are observed. Everything a mode must define (spawn,
//! respawn, lives, limits, friendly fire, victory and draw, disconnect, human
//! scaling) stays [`Resolved::Unknown`] except team play, which the name or
//! the briefing states. Which mode a slot runs is also unknown: the markers
//! are mixed (a slot can carry zeppelin data and flag bases), so the
//! binding is an explicit unknown rather than a guess. Both gate F56-B.

use std::fmt;
use std::io;

use cs_types::asset_id::{SourceSpan, SourceSpanError};
use cs_types::content::{ContentId, ContentIdError, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimIdError, ClaimStatus, ContentHash};
use cs_types::install::InstallFileRecord;

use crate::config::StringRow;

/// The string ids of the mode-name run, both ends included.
///
/// Observed in the retail string table: `7011..=7014` read `Deathmatch without
/// Teams`, `Deathmatch with Teams`, `Capture the Flag` and `Zeppelin vs.
/// Zeppelin`; `7010` is a column heading and `7015` is the first network
/// transport (`TCP/IP`), so the run is bounded on both sides by rows of a
/// different family. The finding records the boundary rows.
pub const MODE_NAME_IDS: std::ops::RangeInclusive<u32> = 7011..=7014;

/// The id of the first multiplayer briefing block's title row.
pub const BRIEFING_FIRST_ID: u32 = 16600;

/// The id distance between two briefing blocks.
pub const BRIEFING_STRIDE: u32 = 20;

/// The most blocks the walk will visit, so a hostile table cannot make it run
/// unbounded. The original family is far smaller.
pub const MAX_BRIEFING_BLOCKS: u32 = 64;

/// The slot number range of an `MP<n>` directory the catalog accepts.
pub const MAX_SLOT_NUMBER: u8 = 9;

/// A localized string with where it was read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextRef {
    /// The string id.
    pub id: u32,
    /// The decoded text.
    pub text: String,
    /// The bytes of the string block it came from.
    pub span: SourceSpan,
}

/// One point value printed in a briefing.
///
/// Only the printed number is observed. Which game event each number rewards
/// is *not*: the order of the printed instructions suggests a pairing, but the
/// scoring itself lives in code or script that is not decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PointLabel {
    /// The string id of the number row.
    pub id: u32,
    /// The printed value.
    pub value: i32,
    /// The bytes of the string block it came from.
    pub span: SourceSpan,
}

/// One briefing block of the multiplayer family.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Briefing {
    /// The block's title row.
    pub title: TextRef,
    /// The one-line description under the title.
    pub tagline: TextRef,
    /// The instruction lines, in table order. A blank row is the table's
    /// padding to the next block, not an instruction, and is skipped.
    pub instructions: Vec<TextRef>,
    /// The printed point values, in table order.
    pub points: Vec<PointLabel>,
}

/// Whether a mode groups pilots into teams.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeamPlay {
    /// Every pilot scores for themself.
    FreeForAll,
    /// Pilots score for their team.
    Teams,
}

/// A rule the original defines per mode and the installation does not answer
/// anywhere the catalog can read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownRule {
    /// The rule's label (`cs_net::rules::RuleField::label`).
    pub field: &'static str,
    /// The claim the unknown belongs to.
    pub claim: ClaimId,
    /// Why it is unknown.
    pub reason: String,
}

/// One discovered multiplayer mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModeEntry {
    /// The stable id: `multiplayer_rules/mode.name-<name string id>`.
    pub id: ContentId,
    /// The selectable name.
    pub name: TextRef,
    /// The briefing describing it.
    pub briefing: Briefing,
    /// Team play, when the name or briefing states it.
    pub team_play: Resolved<TeamPlay>,
    /// Every rule the catalog could not resolve, in canonical order.
    pub unknown_rules: Vec<UnknownRule>,
}

/// The rule labels a mode defines, in the order of
/// `cs_net::rules::RuleField::ALL`.
pub const RULE_LABELS: [&str; 12] = [
    "teams",
    "late_join",
    "time_limit",
    "score_limit",
    "lives",
    "respawn",
    "friendly_fire",
    "disconnect",
    "humans",
    "human_scaling",
    "custom_planes",
    "component_limit",
];

/// Why mode discovery failed.
#[derive(Debug)]
pub enum ModeError {
    /// A row the table needs is absent for the language.
    MissingRow {
        /// The absent id.
        id: u32,
    },
    /// A row's code units do not decode.
    Undecodable {
        /// The row's id.
        id: u32,
    },
    /// Two rows share an id and language.
    Duplicate {
        /// The shared id.
        id: u32,
    },
    /// No briefing block of the family was found.
    NoBriefing,
    /// An id could not become a content id or claim id.
    Identity(String),
}

impl fmt::Display for ModeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRow { id } => write!(f, "string row {id} is absent"),
            Self::Undecodable { id } => write!(f, "string row {id} does not decode"),
            Self::Duplicate { id } => write!(f, "string row {id} appears twice"),
            Self::NoBriefing => write!(f, "no multiplayer briefing block was found"),
            Self::Identity(detail) => write!(f, "identity refused: {detail}"),
        }
    }
}

impl std::error::Error for ModeError {}

impl From<ContentIdError> for ModeError {
    fn from(error: ContentIdError) -> Self {
        Self::Identity(error.to_string())
    }
}

impl From<ClaimIdError> for ModeError {
    fn from(error: ClaimIdError) -> Self {
        Self::Identity(error.to_string())
    }
}

/// The modes the string table names, with every gap kept visible.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModeCatalog {
    /// One entry per name that pairs with exactly one briefing, in name-id
    /// order.
    pub modes: Vec<ModeEntry>,
    /// Names no briefing answers.
    pub names_without_briefing: Vec<TextRef>,
    /// Briefings no name answers.
    pub briefings_without_name: Vec<Briefing>,
    /// The title row of the first block that is not in the family: the
    /// observed end of the table (absent when the walk hit the block bound or
    /// the end of the rows).
    pub boundary: Option<TextRef>,
    /// How many briefing blocks belong to the family.
    pub briefing_blocks: usize,
}

impl ModeCatalog {
    /// Whether every name has a briefing and every briefing a name.
    pub fn is_complete(&self) -> bool {
        self.names_without_briefing.is_empty() && self.briefings_without_name.is_empty()
    }
}

struct Rows<'a> {
    language: u32,
    rows: &'a [StringRow],
}

impl Rows<'_> {
    fn find(&self, id: u32) -> Result<Option<&StringRow>, ModeError> {
        let mut hits = self
            .rows
            .iter()
            .filter(|row| row.id == id && row.language == self.language);
        let first = hits.next();
        if hits.next().is_some() {
            return Err(ModeError::Duplicate { id });
        }
        Ok(first)
    }

    fn text(&self, id: u32) -> Result<Option<TextRef>, ModeError> {
        let Some(row) = self.find(id)? else {
            return Ok(None);
        };
        let text = row.text.clone().ok_or(ModeError::Undecodable { id })?;
        Ok(Some(TextRef {
            id,
            text,
            span: row.span.clone(),
        }))
    }

    fn required(&self, id: u32) -> Result<TextRef, ModeError> {
        self.text(id)?.ok_or(ModeError::MissingRow { id })
    }
}

/// The comparison key of a mode name or a briefing title: lowercase
/// alphanumerics only, with `X without Teams` read as `X` and `X with Teams`
/// as `team X`, which is how the two deathmatch names pair with the
/// `DEATHMATCH` and `TEAM DEATHMATCH` titles.
fn pairing_key(text: &str) -> String {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|word| {
            word.chars()
                .filter(char::is_ascii_alphanumeric)
                .collect::<String>()
                .to_ascii_lowercase()
        })
        .filter(|word| !word.is_empty())
        .collect();
    let joined = |words: &[String]| words.concat();
    match words.as_slice() {
        [head @ .., with, teams] if with == "with" && teams == "teams" => {
            format!("team{}", joined(head))
        }
        [head @ .., without, teams] if without == "without" && teams == "teams" => joined(head),
        _ => joined(&words),
    }
}

fn claim(text: &str) -> Result<ClaimId, ModeError> {
    Ok(ClaimId::new(text)?)
}

/// Reads the mode catalog from the localized string rows of one language.
///
/// # Errors
///
/// [`ModeError`] when a name row is missing or undecodable, when an id is
/// ambiguous, when no briefing block exists or an identity is refused.
pub fn discover_modes(rows: &[StringRow], language: u32) -> Result<ModeCatalog, ModeError> {
    let table = Rows { language, rows };

    let mut names = Vec::new();
    for id in MODE_NAME_IDS {
        names.push(table.required(id)?);
    }

    let mut briefings: Vec<Briefing> = Vec::new();
    let mut boundary = None;
    for block in 0..MAX_BRIEFING_BLOCKS {
        let base = BRIEFING_FIRST_ID + block * BRIEFING_STRIDE;
        let Some(title) = table.text(base)? else {
            break;
        };
        let in_family = table
            .text(base + 2)?
            .is_some_and(|row| row.text.eq_ignore_ascii_case("POINTS"));
        if !in_family {
            boundary = Some(title);
            break;
        }
        let tagline = table.required(base + 1)?;
        let mut instructions = Vec::new();
        let mut points = Vec::new();
        for offset in 4..BRIEFING_STRIDE {
            let Some(row) = table.text(base + offset)? else {
                continue;
            };
            // The original blocks pad the unused rows of their fixed stride
            // with empty strings (ids 16614..16619 and so on). A blank row is
            // neither an instruction nor a printed point value.
            if row.text.trim().is_empty() {
                continue;
            }
            match row.text.trim().parse::<i32>() {
                Ok(value) => points.push(PointLabel {
                    id: row.id,
                    value,
                    span: row.span,
                }),
                Err(_) => instructions.push(row),
            }
        }
        briefings.push(Briefing {
            title,
            tagline,
            instructions,
            points,
        });
    }
    if briefings.is_empty() {
        return Err(ModeError::NoBriefing);
    }
    let briefing_blocks = briefings.len();

    let mut modes = Vec::new();
    let mut names_without_briefing = Vec::new();
    for name in names {
        let key = pairing_key(&name.text);
        let hits: Vec<usize> = briefings
            .iter()
            .enumerate()
            .filter(|(_, briefing)| pairing_key(&briefing.title.text) == key)
            .map(|(index, _)| index)
            .collect();
        match hits.as_slice() {
            [index] => {
                let briefing = briefings.remove(*index);
                modes.push(entry(name, briefing)?);
            }
            _ => names_without_briefing.push(name),
        }
    }

    Ok(ModeCatalog {
        modes,
        names_without_briefing,
        briefings_without_name: briefings,
        boundary,
        briefing_blocks,
    })
}

/// The catalog identity of the mode the string table names at `name_id`.
///
/// A mode is identified by the string id its **name** was read from, never by
/// its position in a walk, so the identity is the same whether the name paired
/// with a briefing ([`discover_modes`]'s [`ModeEntry::id`]) or not: a caller
/// that inventories the names the parser could not pair asks for the identity
/// here instead of writing the formula out a second time.
///
/// # Errors
///
/// [`ModeError::Identity`] when the derived key is refused as a
/// [`ContentId`].
pub fn mode_name_id(name_id: u32) -> Result<ContentId, ModeError> {
    Ok(ContentId::from_source(
        ContentKind::MultiplayerRules,
        &format!("mode.name-{name_id}"),
    )?)
}

fn entry(name: TextRef, briefing: Briefing) -> Result<ModeEntry, ModeError> {
    let id = mode_name_id(name.id)?;
    let lower = name.text.to_ascii_lowercase();
    let team_claim = claim(&format!("f56.mode.name-{}.team_play", name.id))?;
    let team_play = if lower.ends_with("without teams") {
        known(
            TeamPlay::FreeForAll,
            team_claim,
            ClaimStatus::ObservedTool,
            &name.span,
        )
    } else if lower.ends_with("with teams") {
        known(
            TeamPlay::Teams,
            team_claim,
            ClaimStatus::ObservedTool,
            &name.span,
        )
    } else if briefing
        .instructions
        .iter()
        .any(|line| line.text.to_ascii_lowercase().contains("team"))
    {
        known(
            TeamPlay::Teams,
            team_claim,
            ClaimStatus::Inferred,
            &briefing.title.span,
        )
    } else {
        Resolved::unknown(
            team_claim,
            "neither the name nor the briefing says whether pilots form teams",
        )
        .map_err(|error| ModeError::Identity(error.to_string()))?
    };

    let mut unknown_rules = Vec::new();
    for label in RULE_LABELS {
        if label == "teams" && team_play.is_known() {
            continue;
        }
        unknown_rules.push(UnknownRule {
            field: label,
            claim: claim(&format!("f56.mode.name-{}.{label}", name.id))?,
            reason: "the installation's string table does not state this rule and the mode's \
                     script or code path is not decoded"
                .to_owned(),
        });
    }
    Ok(ModeEntry {
        id,
        name,
        briefing,
        team_play,
        unknown_rules,
    })
}

fn known<T>(value: T, claim_id: ClaimId, class: ClaimStatus, span: &SourceSpan) -> Resolved<T> {
    Resolved::Known(Known::new(
        value,
        Provenance {
            claim_id,
            class,
            source: Some(span.clone()),
        },
    ))
}

// ----------------------------------------------------------------- slots ---

/// A byte pattern found in a slot's reader archive.
///
/// A marker is evidence that the archive *mentions* something, not that the
/// slot runs a given mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SlotMarker {
    /// The ASCII text `Flag base` (a capture-the-flag base name).
    FlagBase,
    /// A `\zeps` path segment (zeppelin data directory).
    ZeppelinData,
}

impl SlotMarker {
    /// The stable label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::FlagBase => "flag_base",
            Self::ZeppelinData => "zeppelin_data",
        }
    }

    fn needle(self) -> &'static [u8] {
        match self {
            Self::FlagBase => b"flag base",
            Self::ZeppelinData => b"\\zeps",
        }
    }
}

/// Which markers occur in `bytes`, matched ASCII-case-insensitively.
pub fn scan_markers(bytes: &[u8]) -> Vec<SlotMarker> {
    [SlotMarker::FlagBase, SlotMarker::ZeppelinData]
        .into_iter()
        .filter(|marker| {
            let needle = marker.needle();
            bytes
                .windows(needle.len())
                .any(|window| window.eq_ignore_ascii_case(needle))
        })
        .collect()
}

/// One `ZBD/<group>/MP<n>/` scenario slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioSlot {
    /// The stable id: `multiplayer_scenario/slot.<group>.mp<n>`.
    pub id: ContentId,
    /// The world-group directory, as spelled.
    pub world_group: String,
    /// The slot number from the directory name.
    pub slot: u8,
    /// The reader archive: its path, size and digest.
    pub program: SourceSpan,
    /// Other files in the directory, as spelled.
    pub companions: Vec<String>,
    /// Markers found in the archive.
    pub markers: Vec<SlotMarker>,
    /// The mode the slot runs: unknown until the slot's program is decoded.
    pub mode: Resolved<ContentId>,
}

/// Why slot discovery failed.
#[derive(Debug)]
pub enum SlotError {
    /// Reading a slot's archive failed.
    Read {
        /// The file's relative spelling.
        path: String,
        /// The error.
        source: io::Error,
    },
    /// An identity or span was refused.
    Identity(String),
}

impl fmt::Display for SlotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => write!(f, "cannot read {path}: {source}"),
            Self::Identity(detail) => write!(f, "identity refused: {detail}"),
        }
    }
}

impl std::error::Error for SlotError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Identity(_) => None,
        }
    }
}

impl From<SourceSpanError> for SlotError {
    fn from(error: SourceSpanError) -> Self {
        Self::Identity(format!("{error:?}"))
    }
}

/// The scenario slots of an installation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotCatalog {
    /// Slots with a reader archive, ordered by world group then slot.
    pub slots: Vec<ScenarioSlot>,
    /// `MP<n>` directories that hold no reader archive: listed, not counted,
    /// spelled as the install manifest spells them.
    pub without_program: Vec<String>,
}

/// `(group, slot, file)` when `path` is `ZBD/<group>/MP<n>/<file>`.
fn parse_slot_path(path: &str) -> Option<(&str, u8, &str)> {
    let mut parts = path.split('/');
    let (zbd, group, dir, file) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || !zbd.eq_ignore_ascii_case("ZBD") {
        return None;
    }
    let digits = dir
        .get(..2)
        .filter(|prefix| prefix.eq_ignore_ascii_case("MP"))
        .and_then(|_| dir.get(2..))?;
    let slot: u8 = digits.parse().ok()?;
    ((1..=MAX_SLOT_NUMBER).contains(&slot) && digits == slot.to_string())
        .then_some((group, slot, file))
}

/// Discovers the scenario slots among an installation's inventoried files.
///
/// `read` returns the bytes of one inventoried file; it is called once per
/// slot reader archive (`zrdr.zbd`).
///
/// # Errors
///
/// [`SlotError::Read`] when an archive cannot be read, [`SlotError::Identity`]
/// when an id or span is refused.
pub fn discover_slots(
    install_sha256: ContentHash,
    files: &[InstallFileRecord],
    mut read: impl FnMut(&InstallFileRecord) -> io::Result<Vec<u8>>,
) -> Result<SlotCatalog, SlotError> {
    use std::collections::BTreeMap;

    type Key = (String, u8);
    // The lower-cased group is the identity key; the spelling as inventoried is
    // kept beside it so a reported slot or gap joins with the install manifest
    // the same way the `ZBD` directories are actually spelled.
    let mut dirs: BTreeMap<Key, (String, Option<&InstallFileRecord>, Vec<String>)> =
        BTreeMap::new();
    for record in files {
        let path = record.relative_spelling.as_str();
        let Some((group, slot, file)) = parse_slot_path(path) else {
            continue;
        };
        let entry = dirs
            .entry((group.to_ascii_lowercase(), slot))
            .or_insert_with(|| (group.to_owned(), None, Vec::new()));
        if file.eq_ignore_ascii_case("zrdr.zbd") {
            entry.1 = Some(record);
        } else {
            entry.2.push(path.to_owned());
        }
    }

    let mut slots = Vec::new();
    let mut without_program = Vec::new();
    for ((group, slot), (spelled, program, mut companions)) in dirs {
        let Some(record) = program else {
            without_program.push(format!("ZBD/{spelled}/MP{slot}"));
            continue;
        };
        let path = record.relative_spelling.as_str();
        let bytes = read(record).map_err(|source| SlotError::Read {
            path: path.to_owned(),
            source,
        })?;
        let key = format!("slot.{group}.mp{slot}");
        let id = ContentId::from_source(ContentKind::MultiplayerScenario, &key)
            .map_err(|error| SlotError::Identity(error.to_string()))?;
        let mode_claim = ClaimId::new(&format!("f56.{key}.mode"))
            .map_err(|error| SlotError::Identity(error.to_string()))?;
        let mode = Resolved::unknown(
            mode_claim,
            "the slot's program selects its mode in undecoded script; the markers found in it \
             are mixed across slots and prove only what the archive mentions",
        )
        .map_err(|error| SlotError::Identity(error.to_string()))?;
        companions.sort();
        slots.push(ScenarioSlot {
            id,
            world_group: spelled,
            slot,
            program: SourceSpan::new(
                install_sha256,
                path,
                None,
                0,
                record.size_bytes,
                Some(record.sha256),
            )?,
            companions,
            markers: scan_markers(&bytes),
            mode,
        });
    }
    Ok(SlotCatalog {
        slots,
        without_program,
    })
}
