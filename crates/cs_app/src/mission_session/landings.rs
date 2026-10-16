//! The mission's own landing table and the designed landing path
//! (M01-PLAY-TRIGGERS-ANIMS-LAND, Rally #1498).
//!
//! Task `docs/findings/2026-10-15-m01-play-triggers-anims-wake-anim-record-join.md`
//! measured the join half of #1482: the world-group reader
//! `zbd/c1c/zrdr.zbd :: landings.zrd` carries exactly three rows, the landing
//! table runs the gated records, and `WAKE_ANIM` events arm landing nodes but
//! never run a gated record. This module is the landing half: the production
//! **reader** for that table and its nodes' positions, and the production
//! **consumer** that decides when the player landed and starts the row's
//! record.
//!
//! # What is measured, and what is designed
//!
//! | Piece | Evidence |
//! | --- | --- |
//! | The table itself: three rows, each a flat `(key, value)` list over `anim`, `node`, `angle`, `speed`, `auto` | read verbatim through production discovery + `decode_zrd` ([`LANDING_ROWS_CLAIM`]) |
//! | The three named nodes exist in the world container, with positions | `gamez.zbd`'s node array through `world_scene_graph_from_gamez` |
//! | Which record publishes `OBJECT_ACTIVE_STATE` over which node vocabulary | the arm task's designed arming surface (`OBJECT_ACTIVE_ARMING_CLAIM`) |
//! | **Where a node sits relative to the actor the observation reports** | designed: [`LANDING_NODE_POSITION_CLAIM`] |
//! | **What counts as a landing** (radius + vertical speed) | designed: [`LANDING_DETECTION_CLAIM`] |
//! | **What the row's `angle`/`speed` numbers gate** | designed: [`LANDING_WINDOW_CLAIM`] |
//! | **When a record's `animation_refs` fire** | designed: [`LANDING_CALL_CHAIN_CLAIM`] |
//!
//! The three designed rules each carry their own claim id and are stated in
//! `docs/findings/2026-10-16-m01-play-triggers-anims-land.md` with what would
//! resolve them. Nothing here is `verified_original`; no original run exists.
//!
//! # Fail-closed rules
//!
//! * A row whose shape cannot be read verbatim is **refused by name** and
//!   dropped; the rows that read stay (a malformed sibling never silences a
//!   good row). No defaults, no guessed arity.
//! * A node the container does not state (or states twice) gets a named
//!   refusal and its row can never run.
//! * A node whose **anchor** the stage-4 observation reports no row for gets
//!   no position row: the landing consumer then finds no node and runs
//!   nothing. Absence is not a position.
//! * A landing consumer with no table, or with no single player body, runs
//!   nothing.
//!
//! # Why this lives here
//!
//! The consumer has to read three records the mission host already holds —
//! the stage-4 observation, the armed-node set and the name index — and start
//! a record through the host's own player, so it sits beside them in
//! `mission_session` rather than in the script or the content crates (which
//! must stay engine-free, `AGENTS.md` rule 7). The reader is here too because
//! it answers to the **stage mount** (`zbd/c1c` → its reader archive and its
//! world container), the same seam `stage_for` reads every other mission
//! record from.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use bevy::prelude::Resource;
use cs_assets::install;
use cs_content::coordinates::{CoordinateSource, SourceAdapter};
use cs_content::scene::{BindingMap, SceneGraph, SceneNode};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_content::world::world_scene_graph_from_gamez;
use cs_formats::gamez::read_gamez_nodes;
use cs_formats::io::ParseContext;
use cs_formats::script_raw::discover_container;
use cs_types::Tick;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Provenance};
use cs_types::evidence::{ClaimId, ClaimStatus};
use cs_types::install::RelativePath;

use crate::animation::mission::{AnimationNameIndex, NamedAnimationResolution, StartupAnimation};
use crate::animation::survey::CarrierKind;
use crate::mission_animations::{ActivationOutcome, ArmedNode, MissionAnimationPlayer};
use crate::world_facts::{WorldObservation, member_chain};

use super::observation::PLAYER_MEMBER;

/// The claim the landing rows are read under: verbatim production reads of
/// the installation's own table.
///
/// Evidence class [`ClaimStatus::ObservedTool`] — a description of the retail
/// files this repository's own code reads, not of the original executable.
pub const LANDING_ROWS_CLAIM: &str = "m01-landings.landings-zrd-rows-verbatim";

/// The designed join that gives a landing node its world position.
///
/// The container states a node's position inside its **static** hierarchy
/// (M01's `wv_manual_land` sits 150.46 m astern and 67.05 m below the
/// `world1.workersvoyagezep` node, whose own transform in the container is
/// the origin), while the mission's live zeppelin is a **bound world actor**
/// at the pose `zeppelins.zrd` stored. The rule joins the two: a node's
/// landing position is the observed position of the bound actor whose
/// subject node is the node's ancestor directly under the world root, plus
/// the node's measured offset from that ancestor in the container. An anchor
/// the observation reports no row for gives **no node row** — the join never
/// invents a position. Whether the original composed the static node under
/// the live actor the same way is unmeasured and stays this claim's unknown.
pub const LANDING_NODE_POSITION_CLAIM: &str = "m01-landing.node-position-designed";

/// The designed geometric rule for "the player landed here".
///
/// The player body has landed at a node when its stage-4 position is within
/// [`LANDING_RADIUS_M`] of the node's composed position **and** the
/// magnitude of its committed vertical speed is at most
/// [`LANDING_SINK_RATE_MPS`] (near-level flight, not a cruise climb or a
/// dive). Both bounds are designed: the authored landing volumes the
/// container keeps under the nodes (`half_cone`, `sphere` around `land_on`)
/// are not decoded as extents, so no measurement sizes a radius, and no
/// original run was watched. The resolving measurement is a decoded
/// landing-volume extent or an owner original capture of the docking
/// approach.
pub const LANDING_DETECTION_CLAIM: &str = "m01-landing.player-landing-detection-designed";

/// The designed reading of one row's own numbers.
///
/// For a row without `auto`: the body's speed magnitude must lie inside the
/// row's `speed [min, max]` values (inclusive, read as the composition's own
/// metres per second — the unit the flight law and every pose use) and its
/// tilt off world-level — the angle between the body's up vector and world
/// up, one number covering bank and pitch — must not exceed the row's single
/// `angle` value. A row spelling `auto` has no window: proximity and vertical speed
/// alone satisfy it. The table's own unit, the meaning of `angle`, and what
/// `auto` switched in the original are all unmeasured; an arity this rule
/// cannot read is a **named refusal**, never a guessed bound.
pub const LANDING_WINDOW_CLAIM: &str = "m01-landing.row-window-designed";

/// The designed rule for when a landing record's calls start.
///
/// The record player publishes a record's statements as it runs, but no
/// payload field is read out of a `CALL_ANIMATION` event
/// (`animation::events`), so a ref cannot be tied to the statement that
/// made it — the gap this rule fills: **every record the landing consumer
/// starts has the names of its measured `animation_refs` started at that
/// record's own start**, transitively through each name so started, each
/// identity once per landing. A name that resolves to no playable row is a
/// refusal on the landing's ledger, never a silent skip. Whether the
/// original started those calls at their statement's own `START_TIME`
/// instead is unmeasured and stays this claim's unknown.
pub const LANDING_CALL_CHAIN_CLAIM: &str = "m01-landing.call-chain-at-start-designed";

/// How far from a node's composed position the player body counts as having
/// reached it, in metres. Designed — see [`LANDING_DETECTION_CLAIM`].
pub const LANDING_RADIUS_M: f64 = 150.0;

/// The largest vertical speed magnitude (metres per second) that still counts
/// as a landing rather than a pass — in either direction, so a near-level
/// touchdown qualifies and a cruise climb or dive does not. Designed — see
/// [`LANDING_DETECTION_CLAIM`].
pub const LANDING_SINK_RATE_MPS: f64 = 10.0;

/// The world-group reader's landing member, as the installation spells it.
const LANDING_MEMBER: &str = "landings.zrd";

/// One row of the world-group's `landings.zrd`, read verbatim.
///
/// Every field is the member's own text: `anim` and `node` are the single
/// texts their lists spell, `angle` and `speed` keep the list's numbers in
/// order (empty when the row spells no such key), and `auto` records the
/// key's presence. No value is interpreted here — the interpretation is the
/// designed [`LANDING_WINDOW_CLAIM`] and runs where the row is evaluated.
#[derive(Clone, Debug, PartialEq)]
pub struct LandingRow {
    /// The row's ordinal in the member, 0-based — how a refusal names it.
    pub row: usize,
    /// The record the row runs, as the row spells it.
    pub anim: String,
    /// The landing node the row runs at, as the row spells it.
    pub node: String,
    /// The row's `angle` numbers, verbatim and in order.
    pub angle: Vec<f64>,
    /// The row's `speed` numbers, verbatim and in order.
    pub speed: Vec<f64>,
    /// Whether the row spells the `auto` key (its value was empty).
    pub auto: bool,
    /// The member span the row was read from, when it came from an
    /// installation; `None` for an authored row a test writes directly.
    pub span: Option<SourceSpan>,
    /// Where the row came from: [`ClaimStatus::ObservedTool`] for a read row,
    /// [`ClaimStatus::Designed`] for an authored one.
    pub provenance: Provenance,
}

impl LandingRow {
    /// One authored row, for a test that writes the table itself instead of
    /// reading it from an installation.
    ///
    /// An authored row carries [`ClaimStatus::Designed`] provenance and no
    /// span, so a report can never pass it off as read from the retail
    /// files.
    #[must_use]
    pub fn authored(
        row: usize,
        anim: &str,
        node: &str,
        angle: &[f64],
        speed: &[f64],
        auto: bool,
    ) -> Self {
        let provenance = Provenance::new(
            ClaimId::new(LANDING_ROWS_CLAIM).expect("the claim id constant is valid"),
            ClaimStatus::Designed,
            None,
        )
        .expect("Designed provenance needs no source");
        Self {
            row,
            anim: anim.to_owned(),
            node: node.to_owned(),
            angle: angle.to_vec(),
            speed: speed.to_vec(),
            auto,
            span: None,
            provenance,
        }
    }
}

/// One landing node's position rule: which observed member anchors it, and
/// the container's measured offset from that anchor.
///
/// Produced from the container's own node tree ([`landing_node_entries`]),
/// consumed by the observation writer, which reports the node under its own
/// name when the anchor's row exists ([`LANDING_NODE_POSITION_CLAIM`]).
#[derive(Clone, Debug, PartialEq)]
pub struct LandingNodeEntry {
    /// The node name the landing rows spell.
    pub node: String,
    /// The observed member that anchors the node: the container node the
    /// node hangs from, directly under the world root, whose name the
    /// observation reports for a bound actor.
    pub anchor: String,
    /// The node's world position minus its anchor's, from the container —
    /// the measured part of [`LANDING_NODE_POSITION_CLAIM`].
    pub offset: [f64; 3],
}

/// The landing table one composition carries: the rows it can run, the node
/// entries those rows name, and every read refusal either answer earned.
///
/// Installed by the composition (`mission_session::compose`) from the stage
/// mount's own archives, and replaced by a test that authors its own table.
/// A mount whose installation declares no landing member carries no rows
/// with the reader's absence named in [`Self::refusals`] — the landing path
/// then has nothing to run and says why.
#[derive(Resource, Clone, Debug, Default)]
pub struct MissionLandings {
    /// The rows that read verbatim.
    pub rows: Vec<LandingRow>,
    /// The position rules for the nodes those rows name.
    pub nodes: Vec<LandingNodeEntry>,
    /// Every row, node or archive the reader refused, by name.
    pub refusals: Vec<String>,
}

/// Why a landing table could not be read from an installation.
///
/// Every variant names the archive or member so a report says exactly which
/// file was missing or refused, never "the table is empty" without cause.
#[derive(Clone, Debug, PartialEq)]
pub enum LandingReadError {
    /// The installation could not be discovered at all.
    Discovery(String),
    /// No manifest record spells this archive under this mount.
    ArchiveAbsent {
        /// The logical key that was looked for, e.g. `zbd/c1c/zrdr.zbd`.
        archive: String,
    },
    /// The archive declares no landing member.
    MemberAbsent {
        /// The archive's logical key.
        archive: String,
        /// The member that was looked for ([`LANDING_MEMBER`]).
        member: String,
    },
    /// The member's bytes did not decode through the production `.zrd`
    /// grammar.
    Decode {
        /// The member that failed to decode.
        member: String,
        /// The decoder's own words.
        reason: String,
    },
    /// The member decoded, but is no landing table at all.
    Parse(String),
    /// A path or span the reader needed did not spell.
    Format(String),
}

impl fmt::Display for LandingReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(reason) => {
                write!(f, "the installation could not be discovered: {reason}")
            }
            Self::ArchiveAbsent { archive } => {
                write!(f, "the installation declares no archive {archive}")
            }
            Self::MemberAbsent { archive, member } => {
                write!(f, "{archive} declares no {member} member")
            }
            Self::Decode { member, reason } => {
                write!(f, "{member} did not decode: {reason}")
            }
            Self::Parse(reason) => write!(f, "the landing member is no landing table: {reason}"),
            Self::Format(reason) => {
                write!(f, "the landing reader could not spell a path: {reason}")
            }
        }
    }
}

impl std::error::Error for LandingReadError {}

/// Why one document was no landing table at all.
#[derive(Clone, Debug, PartialEq)]
pub enum LandingParseError {
    /// The document's top level is not a list.
    NotAList,
    /// A value that is neither a flat row's first key nor a list of rows.
    UnexpectedShape(&'static str),
    /// The row claim id could not be built.
    Claim(String),
}

impl fmt::Display for LandingParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAList => write!(f, "the landing member decodes to no list"),
            Self::UnexpectedShape(what) => {
                write!(
                    f,
                    "the landing member begins with {what}, which is neither a flat row nor a list of rows"
                )
            }
            Self::Claim(reason) => write!(f, "the row claim id is invalid: {reason}"),
        }
    }
}

impl std::error::Error for LandingParseError {}

/// The parse's whole answer: the rows that read, and the rows that refused.
///
/// A refused row is dropped **and named** — never partially read, and never
/// able to silence a sibling row that reads fine.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedLandings {
    /// The rows that read verbatim.
    pub rows: Vec<LandingRow>,
    /// One named refusal per row that could not be read.
    pub refusals: Vec<String>,
}

/// Reads one decoded landing document: a flat `(key, value)` list per row,
/// at whatever nesting level the member stores them at.
///
/// The production `.zrd` grammar gives every value as
/// [`ZrdValue::Text`]/[`ZrdValue::Int`]/[`ZrdValue::Float`]/[`ZrdValue::List`]
/// and no per-row byte offsets, so each row carries the **member's** span
/// plus its ordinal — the position within the member, not an invented byte
/// range.
///
/// # Errors
///
/// [`LandingParseError`] when the document is no table at all: a non-list
/// top level, or a list that begins with a number (neither a row nor a
/// container of rows); or when the row claim id itself would not spell.
/// Row-level problems are not errors — they are the named refusals in
/// [`ParsedLandings::refusals`].
pub fn parse_landings(
    document: &ZrdValue,
    span: &SourceSpan,
) -> Result<ParsedLandings, LandingParseError> {
    let provenance = Provenance::new(
        ClaimId::new(LANDING_ROWS_CLAIM)
            .map_err(|error| LandingParseError::Claim(error.to_string()))?,
        ClaimStatus::ObservedTool,
        Some(span.clone()),
    )
    .map_err(|error| LandingParseError::Claim(error.to_string()))?;

    let mut flat: Vec<&[ZrdValue]> = Vec::new();
    collect_rows(document, &mut flat)?;

    let mut rows = Vec::new();
    let mut refusals = Vec::new();
    for (index, cells) in flat.iter().enumerate() {
        match parse_row(index, cells, span, &provenance) {
            Ok(row) => rows.push(row),
            Err(reason) => refusals.push(reason),
        }
    }
    Ok(ParsedLandings { rows, refusals })
}

/// Collects every flat row of a possibly nested landing document.
fn collect_rows<'a>(
    value: &'a ZrdValue,
    out: &mut Vec<&'a [ZrdValue]>,
) -> Result<(), LandingParseError> {
    let ZrdValue::List(cells) = value else {
        return Err(LandingParseError::NotAList);
    };
    match cells.first() {
        // A flat row: the cells are alternating key/value pairs.
        Some(ZrdValue::Text(_)) => out.push(cells),
        // A container of rows.
        Some(ZrdValue::List(_)) => {
            for cell in cells {
                collect_rows(cell, out)?;
            }
        }
        Some(ZrdValue::Int(_)) => return Err(LandingParseError::UnexpectedShape("a number")),
        Some(ZrdValue::Float(_)) => return Err(LandingParseError::UnexpectedShape("a number")),
        None => {}
    }
    Ok(())
}

/// Parses one flat row, refusing it by name for any shape it cannot read.
fn parse_row(
    index: usize,
    cells: &[ZrdValue],
    span: &SourceSpan,
    provenance: &Provenance,
) -> Result<LandingRow, String> {
    let mut anim: Option<String> = None;
    let mut node: Option<String> = None;
    let mut angle: Option<Vec<f64>> = None;
    let mut speed: Option<Vec<f64>> = None;
    let mut auto = false;
    let mut seen: BTreeSet<&str> = BTreeSet::new();

    let mut position = 0;
    while position < cells.len() {
        let key = match &cells[position] {
            ZrdValue::Text(key) => key.as_str(),
            other => {
                return Err(format!(
                    "row {index}: cell {position} is a {other:?} value where the flat (key, \
                     value) shape needs a key — the row is refused rather than misread"
                ));
            }
        };
        let Some(value) = cells.get(position + 1) else {
            return Err(format!(
                "row {index}: the key {key:?} carries no value — the row is refused rather than \
                 defaulted"
            ));
        };
        if !seen.insert(key) {
            return Err(format!(
                "row {index}: the key {key:?} appears twice — the row is refused rather than \
                 keeping one of two values"
            ));
        }
        match key {
            "anim" | "node" => {
                let text = match value {
                    ZrdValue::List(items) if items.len() == 1 => match &items[0] {
                        ZrdValue::Text(text) if !text.is_empty() => text.clone(),
                        _ => {
                            return Err(format!(
                                "row {index}: the key {key:?} is not exactly one text — the row \
                                 is refused, no record or node can be named"
                            ));
                        }
                    },
                    _ => {
                        return Err(format!(
                            "row {index}: the key {key:?} is not a one-text list — the row is \
                             refused, no record or node can be named"
                        ));
                    }
                };
                if key == "anim" {
                    anim = Some(text);
                } else {
                    node = Some(text);
                }
            }
            "angle" | "speed" => {
                let numbers = match value {
                    ZrdValue::List(items) => items
                        .iter()
                        .map(|item| match item {
                            ZrdValue::Int(number) => Some(f64::from(*number)),
                            ZrdValue::Float(number) => Some(f64::from(*number)),
                            _ => None,
                        })
                        .collect::<Option<Vec<f64>>>(),
                    _ => None,
                };
                let Some(numbers) = numbers else {
                    return Err(format!(
                        "row {index}: the key {key:?} is not a list of numbers, so its window \
                         cannot be read verbatim — the row is refused"
                    ));
                };
                if key == "angle" {
                    angle = Some(numbers);
                } else {
                    speed = Some(numbers);
                }
            }
            "auto" => {
                if !matches!(value, ZrdValue::List(_)) {
                    return Err(format!(
                        "row {index}: the key {key:?} is not a list, so its flag cannot be read \
                         verbatim — the row is refused"
                    ));
                }
                auto = true;
            }
            other => {
                return Err(format!(
                    "row {index}: the key {other:?} is not one of the measured landing keys \
                     (anim, node, angle, speed, auto) — the row is refused rather than partially \
                     read"
                ));
            }
        }
        position += 2;
    }

    let (Some(anim), Some(node)) = (anim, node) else {
        return Err(format!(
            "row {index}: the row spells no anim or no node — the row is refused, a landing needs \
             both"
        ));
    };
    Ok(LandingRow {
        row: index,
        anim,
        node,
        angle: angle.unwrap_or_default(),
        speed: speed.unwrap_or_default(),
        auto,
        span: Some(span.clone()),
        provenance: provenance.clone(),
    })
}

/// Reads the mount's own landing table and the position rules for the nodes
/// its rows name, through the production readers.
///
/// `directory` is the mount's logical directory (the stage's
/// [`super::compose::StageMount::directory`], e.g. `zbd/c1c`): the rows come
/// from `<directory>/zrdr.zbd`'s [`LANDING_MEMBER`] member and the nodes
/// from `<directory>/gamez.zbd`'s node array — the same world-group
/// archives every other reader of this mount reads. The container's node
/// positions go through `world_scene_graph_from_gamez` with the **retail
/// GameZ** coordinate source, the same conversion the definition import
/// uses, so a node's offset and a bound actor's pose are in one frame;
/// mesh slots and semantic bindings are passed empty because neither takes
/// part in a node's world transform (the launch closure builds its own
/// verdict graph the same way).
///
/// Row refusals and node refusals accumulate in
/// [`MissionLandings::refusals`]; only the **rows**' archive, member and
/// decode are [`LandingReadError`]s, because without them there is no table
/// to carry. A node half that cannot be read leaves the rows standing and
/// the nodes named as refused — rows that can never fire, and say so.
///
/// # Errors
///
/// [`LandingReadError`] for an undiscoverable installation, an absent
/// archive or member, a member that did not decode, or a path that would
/// not spell.
pub fn read_landings(
    install_root: &Path,
    directory: &str,
) -> Result<MissionLandings, LandingReadError> {
    let found = install::discover(install_root)
        .map_err(|error| LandingReadError::Discovery(error.to_string()))?;

    let archive = format!("{directory}/zrdr.zbd").to_ascii_lowercase();
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == archive)
        .ok_or_else(|| LandingReadError::ArchiveAbsent {
            archive: archive.clone(),
        })?;
    let spelling = record.relative_spelling.as_str().to_owned();
    let path = RelativePath::new(&spelling.to_ascii_lowercase())
        .map_err(|error| LandingReadError::Format(error.to_string()))?;
    let bytes = std::fs::read(found.manifest.host_root.join(&spelling))
        .map_err(|error| LandingReadError::Format(error.to_string()))?;
    let discovery = discover_container(&archive, &path, &bytes);
    let program = discovery
        .programs()
        .iter()
        .find(|program| {
            program
                .locator()
                .member()
                .is_some_and(|name| name.eq_ignore_ascii_case(LANDING_MEMBER))
        })
        .ok_or_else(|| LandingReadError::MemberAbsent {
            archive: archive.clone(),
            member: LANDING_MEMBER.to_owned(),
        })?;
    let document = decode_zrd(program.bytes()).map_err(|error| LandingReadError::Decode {
        member: LANDING_MEMBER.to_owned(),
        reason: error.to_string(),
    })?;
    // The member's own byte span inside the archive, wrapped as the
    // provenance span the rows carry: the grammar decoder yields no
    // per-row offsets, so a row's span is its member's span plus its
    // ordinal, never an invented byte range.
    let byte_span = program.locator().span();
    let member_span = SourceSpan::new(
        install::fingerprint(&found.manifest),
        &spelling,
        Some(LANDING_MEMBER),
        byte_span.offset,
        byte_span.len,
        None,
    )
    .map_err(|error| LandingReadError::Format(error.to_string()))?;
    let parsed = parse_landings(&document, &member_span)
        .map_err(|error| LandingReadError::Parse(error.to_string()))?;

    // The node half: every distinct node the rows name, located in the
    // world-group container's own tree. A node half that cannot be read
    // leaves the rows standing and the absence named — rows that can never
    // fire, and say so.
    let mut nodes = Vec::new();
    let mut refusals = parsed.refusals;
    if !parsed.rows.is_empty() {
        match read_landing_nodes(&found, directory) {
            Ok(graph) => {
                let (entries, node_refusals) = landing_node_entries(&graph, &parsed.rows);
                nodes = entries;
                refusals.extend(node_refusals);
            }
            Err(reason) => refusals.push(reason),
        }
    }

    Ok(MissionLandings {
        rows: parsed.rows,
        nodes,
        refusals,
    })
}

/// Builds the world-group container's scene graph for the node half, or
/// says why it could not.
///
/// The mesh-slot table and the semantic bindings are passed empty: neither
/// takes part in a node's world transform (the graph's node build reads a
/// mesh index only to resolve a mesh binding), and the launch closure
/// builds its own verdict graph over the same container the same way.
fn read_landing_nodes(found: &install::Discovery, directory: &str) -> Result<SceneGraph, String> {
    let archive = format!("{directory}/gamez.zbd").to_ascii_lowercase();
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == archive)
        .ok_or_else(|| format!("the installation declares no archive {archive}"))?;
    let spelling = record.relative_spelling.as_str().to_owned();
    let bytes = std::fs::read(found.manifest.host_root.join(&spelling))
        .map_err(|error| format!("{archive} could not be read: {error}"))?;
    let span = SourceSpan::new(
        install::fingerprint(&found.manifest),
        &spelling,
        None,
        0,
        u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        None,
    )
    .map_err(|error| format!("{archive} has no whole-file span: {error}"))?;

    let mut context = ParseContext::with_defaults(&archive);
    let records =
        read_gamez_nodes(&mut context, &bytes).map_err(|error| format!("{archive}: {error}"))?;
    let container = ContentId::from_source(
        ContentKind::SceneNode,
        &format!("container.{}", archive.replace('/', ".")),
    )
    .map_err(|error| format!("the container id for {archive} would not spell: {error:?}"))?;
    let adapter = SourceAdapter::new(CoordinateSource::retail_gamez(span));
    let graph =
        world_scene_graph_from_gamez(&container, &records, &[], &adapter, &BindingMap::default())
            .map_err(|error| format!("{archive} would not build a scene graph: {error:?}"))?;
    Ok(graph.graph().clone())
}

/// The position rules for the rows' distinct node names, and a named refusal
/// for every node the container cannot answer.
///
/// The container may state a node **twice** (two paths share one bare name,
/// as `land_on` does under both zeppelins): such a node gets a refusal
/// listing its paths rather than a first-match pick — rule 4 never assigns
/// a position to a plausible node.
#[must_use]
pub fn landing_node_entries(
    graph: &SceneGraph,
    rows: &[LandingRow],
) -> (Vec<LandingNodeEntry>, Vec<String>) {
    let mut entries: Vec<LandingNodeEntry> = Vec::new();
    let mut refusals: Vec<String> = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for row in rows {
        if !seen.insert(row.node.as_str()) {
            continue;
        }
        let matches: Vec<&SceneNode> = graph
            .nodes()
            .iter()
            .filter(|node| node.name() == row.node)
            .collect();
        match matches.as_slice() {
            [] => refusals.push(format!(
                "the landing node {:?} is not a node of the world container, so where a landing \
                 there would be is unknown — the row that names it can never run",
                row.node
            )),
            [node] => {
                let anchor = anchor_of(graph, node);
                let [node_x, node_y, node_z] = node.world_transform().translation();
                let [anchor_x, anchor_y, anchor_z] = anchor.world_transform().translation();
                entries.push(LandingNodeEntry {
                    node: row.node.clone(),
                    anchor: anchor.name().to_owned(),
                    offset: [node_x - anchor_x, node_y - anchor_y, node_z - anchor_z],
                });
            }
            many => {
                let paths = many
                    .iter()
                    .map(|node| node.path())
                    .collect::<Vec<_>>()
                    .join(", ");
                refusals.push(format!(
                    "the landing node {:?} appears at {} paths ({paths}) — the row cannot choose \
                     between them",
                    row.node,
                    many.len()
                ));
            }
        }
    }
    (entries, refusals)
}

/// The ancestor of `node` that hangs directly under the graph's root — the
/// node a bound actor's subject names for M01's three rows
/// (`workersvoyagezep`, `piratezep`).
///
/// A node that **is** the root has no such ancestor and anchors to itself
/// (its offset then reads 0), and the observation's actor row for its name
/// is what the position join looks up. `SceneGraph::build` refuses dangling
/// or cyclic links, so every walk terminates at the root; the defensive
/// `None` arms are kept because a walk that could not end would otherwise
/// hang a composed run.
fn anchor_of<'a>(graph: &'a SceneGraph, node: &'a SceneNode) -> &'a SceneNode {
    let mut current = node;
    loop {
        let Some(parent_id) = current.parent() else {
            return current;
        };
        let Some(parent) = graph.node(parent_id) else {
            return current;
        };
        if parent.parent().is_none() {
            return current;
        }
        current = parent;
    }
}

/// Whether one row's own window admits this landing.
///
/// `Ok(true)` the landing satisfies the row's numbers, `Ok(false)` it falls
/// outside them (the landing runs nothing, and a later landing may still
/// satisfy it), and `Err` the row spells a number this designed rule cannot
/// read — a refusal recorded once, never a guessed bound
/// ([`LANDING_WINDOW_CLAIM`]).
///
/// # Errors
///
/// A `speed`/`angle` list whose arity the designed reading does not cover,
/// or a `speed` range whose minimum exceeds its maximum.
pub fn window_satisfied(row: &LandingRow, speed_m_s: f64, tilt_deg: f64) -> Result<bool, String> {
    if row.auto {
        return Ok(true);
    }
    let [minimum, maximum] = row.speed.as_slice() else {
        return Err(format!(
            "row {}: the speed entry holds {} value(s), so the designed [min,max] window cannot \
             be read",
            row.row,
            row.speed.len()
        ));
    };
    if minimum > maximum {
        return Err(format!(
            "row {}: the speed window {}..{maximum} runs backwards, so no speed can satisfy it",
            row.row, minimum
        ));
    }
    let [angle] = row.angle.as_slice() else {
        return Err(format!(
            "row {}: the angle entry holds {} value(s), so the designed single bound cannot be \
             read",
            row.row,
            row.angle.len()
        ));
    };
    Ok(speed_m_s >= *minimum && speed_m_s <= *maximum && tilt_deg <= *angle)
}

/// Whether the player body reached a landing at `node`.
///
/// The designed geometric half of [`LANDING_DETECTION_CLAIM`]: within
/// [`LANDING_RADIUS_M`] and its vertical speed no faster than
/// [`LANDING_SINK_RATE_MPS`] in either direction. Pure, so the rule is
/// unit-testable without a world.
#[must_use]
pub fn landing_reached(player: [f64; 3], node: [f64; 3], vertical_m_s: f64) -> bool {
    let [px, py, pz] = player;
    let [nx, ny, nz] = node;
    let distance_squared =
        (px - nx).mul_add(px - nx, (py - ny) * (py - ny)) + (pz - nz) * (pz - nz);
    distance_squared <= LANDING_RADIUS_M * LANDING_RADIUS_M
        && vertical_m_s.abs() <= LANDING_SINK_RATE_MPS
}

/// One name the landing's call chain started (or refused), and how.
///
/// The chain is the designed [`LANDING_CALL_CHAIN_CLAIM`]: the refusals of
/// an unbindable name sit here so a report shows which link of the call
/// chain no record joined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LandingCall {
    /// The name as the record's `animation_refs` table spells it.
    pub name: String,
    /// The tick the chain started it on.
    pub at: Tick,
    /// What the player did with it: started, already running, refused.
    pub outcome: ActivationOutcome,
}

/// The landing that ran one row's record, with its call chain.
#[derive(Clone, Debug, PartialEq)]
pub struct LandingActivation {
    /// The row's ordinal in the table.
    pub row: usize,
    /// The node the landing happened at, as the row spells it.
    pub node: String,
    /// The record the row names, as the row spells it.
    pub anim: String,
    /// The tick the consumer ran it on.
    pub at: Tick,
    /// What the player did with the row's record.
    pub outcome: ActivationOutcome,
    /// The names its measured `animation_refs` chain activated, in chain
    /// order. Empty when the row's record did not start here.
    pub calls: Vec<LandingCall>,
}

/// One name the landing path refused, with the row it happened under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LandingRefusal {
    /// The row's ordinal in the table.
    pub row: usize,
    /// The node the landing happened at, as the row spells it.
    pub node: String,
    /// The name the refusal is about: the row's `anim` for a join or window
    /// refusal, the chain's name for a call-chain refusal.
    pub name: String,
    /// The refusing side's own words.
    pub reason: String,
    /// The records the name matched, when a join refused it by ambiguity.
    pub matches: Vec<(CarrierKind, usize)>,
    /// The tick the refusal was recorded on.
    pub at: Tick,
}

/// What one composed run's landing path has done: which rows have been
/// attempted, and everything they started or refused.
///
/// Cleared on restart beside the wake ledger — a fresh generation's
/// landings are its own.
#[derive(Clone, Debug, Default)]
pub struct LandingLedger {
    done: BTreeSet<usize>,
    warned: BTreeSet<usize>,
    activated: Vec<LandingActivation>,
    refused: Vec<LandingRefusal>,
}

impl LandingLedger {
    /// Every landing that reached a row's record, in order.
    #[must_use]
    pub fn activated(&self) -> &[LandingActivation] {
        &self.activated
    }

    /// Every name or window this path refused, in order.
    #[must_use]
    pub fn refused(&self) -> &[LandingRefusal] {
        &self.refused
    }

    /// The rows a landing has already been attempted for — one attempt per
    /// row per run, whether the record started or refused.
    pub fn done(&self) -> impl Iterator<Item = usize> + '_ {
        self.done.iter().copied()
    }

    /// The chain the row's record opened: the measured `animation_refs`
    /// names no `CALL_ANIMATION` event of the same record also spells,
    /// transitively, each identity once, starting each playable row through
    /// the player's own activation path ([`LANDING_CALL_CHAIN_CLAIM`]).
    ///
    /// The spelled calls themselves are the measured chain and start through
    /// the publish path ([`crate::mission_animations::CallAnimationLedger`]),
    /// never here — a name both the table and an event state fires exactly
    /// once, at its statement's own time, and never twice. What remains for
    /// this designed chain is the remainder: names the record's table
    /// references but no event spells — in every one of M01's measured
    /// records that set is empty, so this path is quiet on the owner's data.
    ///
    /// Returns the calls in the order the chain offered them: every name
    /// that bound, with the outcome the player gave it (a row the player
    /// refuses — undeclared, undecodable — lands here with
    /// [`ActivationOutcome::Refused`], its reasons on the player's own
    /// ledger). A name that binds no record at all never reaches the
    /// player and lands on the ledger as a [`LandingRefusal`] instead.
    fn run_call_chain(
        &mut self,
        names: &AnimationNameIndex,
        player: &mut MissionAnimationPlayer,
        row: &LandingRow,
        root: &StartupAnimation,
        tick: Tick,
    ) -> Vec<LandingCall> {
        // Names a record's `animation_refs` table states but no
        // `CALL_ANIMATION` event spells: the designed remainder this chain
        // exists for. The spelled calls are the publish path's business.
        let remainder = |facts: &crate::animation::mission::AnimationRecordFacts| -> Vec<String> {
            let spelled: BTreeSet<&str> = facts
                .call_targets()
                .iter()
                .map(String::as_str)
                .collect();
            facts
                .animation_refs()
                .iter()
                .filter(|name| !spelled.contains(name.as_str()))
                .cloned()
                .collect()
        };
        let mut visited: BTreeSet<String> = BTreeSet::new();
        visited.insert(root.identity().to_owned());
        let mut queue: Vec<String> = root.bound_record().map(&remainder).unwrap_or_default();
        let mut calls = Vec::new();
        while let Some(name) = queue.pop() {
            if !visited.insert(name.clone()) {
                continue;
            }
            match names.resolve(&name) {
                NamedAnimationResolution::Bound(anim_row) => {
                    let outcome = player.activate(tick, &anim_row);
                    if outcome == ActivationOutcome::Started
                        && let Some(facts) = anim_row.bound_record()
                    {
                        queue.extend(remainder(facts));
                    }
                    calls.push(LandingCall {
                        name,
                        at: tick,
                        outcome,
                    });
                }
                NamedAnimationResolution::Unbound { reason, matches } => {
                    self.refused.push(LandingRefusal {
                        row: row.row,
                        node: row.node.clone(),
                        name,
                        reason: reason.to_owned(),
                        matches,
                        at: tick,
                    });
                }
            }
        }
        calls
    }
}

/// The inputs one tick's landing evaluation reads: the table, the stage-4
/// observation, this tick's armed set, the name index, and the player
/// body's committed velocity and tilt.
///
/// Assembled by [`super::host::MissionHost`] so every borrow it holds is
/// one field of the host, and the ledger and player it mutates are two
/// others.
pub struct LandingTick<'a> {
    /// The composed landing table.
    pub table: &'a MissionLandings,
    /// The stage-4 observation of this tick — the sole authority for the
    /// player's position and for each node's composed position.
    pub observation: &'a WorldObservation,
    /// Every active-state statement's node vocabulary recorded so far.
    pub armed: &'a [ArmedNode],
    /// The mission scope's measured name → carrier-record join.
    pub names: &'a AnimationNameIndex,
    /// The player body's committed velocity, metres per second.
    pub velocity: [f64; 3],
    /// The player body's tilt off world level, degrees.
    pub tilt_deg: f64,
}

/// One tick of the landing path: for every row not yet attempted, whether
/// this tick holds a landing at an armed node whose window the row admits,
/// and what that landing ran.
///
/// Runs where [`super::host::MissionHost::step`] places it (stage 6d, after
/// the armed set has absorbed this tick's wake statements and before the
/// sound stage): the record it starts publishes its statements on the next
/// script advance, so the stage-4 fold of the **next** step is the first
/// place the started record — and the gates its calls satisfy — are
/// observed, exactly as a `WAKE_ANIM`-started record behaves.
///
/// Fail-closed throughout: a row whose node the observation reports no
/// position for, an unarmed node, a player with no observation row, or a
/// window outside the row's numbers each run nothing — absence is never a
/// landing.
pub fn step_landings(
    ledger: &mut LandingLedger,
    player: &mut MissionAnimationPlayer,
    tick: Tick,
    input: &LandingTick<'_>,
) {
    let Some(player_row) = input
        .observation
        .members()
        .get(&member_chain([PLAYER_MEMBER]))
        .filter(|row| row.in_play)
    else {
        return;
    };
    let [vx, vy, vz] = input.velocity;
    let speed = vz.mul_add(vz, vx * vx + vy * vy).sqrt();

    for (index, row) in input.table.rows.iter().enumerate() {
        if ledger.done.contains(&index) {
            // One attempt per row per run: a landing that already ran (or
            // already refused) this row does not run it twice.
            continue;
        }
        if !input
            .armed
            .iter()
            .any(|armed| armed.nodes().iter().any(|node| node == &row.node))
        {
            // Not armed: the row's node vocabulary is not among the
            // recorded active-state statements, so nothing runs — no
            // refusal either, because the arm surface simply has no
            // statement yet.
            continue;
        }
        let Some(node_row) = input
            .observation
            .members()
            .get(&member_chain([row.node.as_str()]))
            .filter(|row| row.in_play)
        else {
            continue;
        };
        if !landing_reached(player_row.position, node_row.position, vy) {
            continue;
        }
        match window_satisfied(row, speed, input.tilt_deg) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(reason) => {
                if ledger.warned.insert(index) {
                    ledger.refused.push(LandingRefusal {
                        row: index,
                        node: row.node.clone(),
                        name: row.anim.clone(),
                        reason,
                        matches: Vec::new(),
                        at: tick,
                    });
                }
                continue;
            }
        }

        ledger.done.insert(index);
        match input.names.resolve(&row.anim) {
            NamedAnimationResolution::Bound(anim_row) => {
                let outcome = player.activate(tick, &anim_row);
                // The row's record starts only here: the call chain fires
                // with it, so an already-running record does not re-fire
                // the chain it fired when it first started.
                let calls = if outcome == ActivationOutcome::Started {
                    ledger.run_call_chain(input.names, player, row, &anim_row, tick)
                } else {
                    Vec::new()
                };
                ledger.activated.push(LandingActivation {
                    row: index,
                    node: row.node.clone(),
                    anim: row.anim.clone(),
                    at: tick,
                    outcome,
                    calls,
                });
            }
            NamedAnimationResolution::Unbound { reason, matches } => {
                ledger.refused.push(LandingRefusal {
                    row: index,
                    node: row.node.clone(),
                    name: row.anim.clone(),
                    reason: reason.to_owned(),
                    matches,
                    at: tick,
                });
            }
        }
    }
}
