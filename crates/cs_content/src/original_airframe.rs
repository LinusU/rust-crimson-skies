//! The retail per-airframe flight-parameter importer (task #796).
//!
//! The original keeps its flight law's *numbers* in data, not in code:
//! `vehicle.zrd` holds each record's `dynamics` block and its engine id,
//! `engines.zrd` maps that id to a thrust factor and the first `player.zrd`
//! directory entry holds the global constants the law reads. This module
//! reads those three members of `ZBD/zrdr.zbd` through the production reader
//! chain (`cs_formats::zbd` + [`decode_zrd`]), resolves the `kind_of`
//! inheritance the original resolves (`0x477b70` copy, `0x479240` overlay) and
//! hands back [`OriginalAirframeParameters`] / [`OriginalGlobalParameters`]:
//! one [`OriginalFieldValue`] per value with its provenance and its byte span
//! inside the member, plus the inheritance chain the value came through.
//!
//! Nothing is repaired: a required key that is missing, not a number or not
//! finite is an [`OriginalImportError`], never a default. The only defaults
//! this module supplies are the ones the image itself supplies where no record
//! can state them — [`LEVEL_OFF_RATE_DEFAULT`] (`0x478a00`, no `vehicle.zrd`
//! record has the key) and `gravity`, which falls back to the `player.zrd`
//! `nom_gravity` because no record sets it either. Both carry a provenance
//! that says so.
//!
//! `cs_content` may not depend on `cs_sim` (`docs/01-ARCHITECTURE.md`), so the
//! flat field vocabulary is declared here as [`AIRFRAME_FIELDS`] /
//! [`GLOBAL_FIELDS`] and mirrored in `cs_sim::flight::original`; the
//! acceptance tests compare the two lists so they cannot drift.
//!
//! [`decode_zrd`]: crate::stunts::decode_zrd

use std::collections::BTreeMap;
use std::path::Path;

use cs_formats::io::ParseContext;
use cs_formats::zbd::{ZbdProbe, dispatch, read_reader_archive, read_version_one_index};
use cs_types::asset_id::SourceSpan;
use cs_types::content::{Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};

use crate::stunts::{ZrdValue, decode_zrd};

/// The reader archive all three members live in, in the installation's own
/// spelling.
pub const READER_ARCHIVE: &str = "ZBD/zrdr.zbd";
/// The member the per-airframe records live in.
pub const VEHICLE_MEMBER: &str = "vehicle.zrd";
/// The member the engine table lives in.
pub const ENGINES_MEMBER: &str = "engines.zrd";
/// The member the global flight constants live in.
///
/// Two directory entries carry this name; the original's lookup takes the
/// **first** (`0x59ddb0`), which is what [`OriginalDocuments::read`] uses.
pub const PLAYER_MEMBER: &str = "player.zrd";

/// The `level_off_rate` the image's default table gives (`0x478a00`): no
/// `vehicle.zrd` record states the key, so the importer supplies this.
pub const LEVEL_OFF_RATE_DEFAULT: f64 = 4.0;

/// The claim id of [`LEVEL_OFF_RATE_DEFAULT`]: static analysis of the image,
/// not an observed behaviour and never `verified_original`.
pub const LEVEL_OFF_RATE_CLAIM: &str = "f796.default.level_off_rate.0x478a00";

/// The flat field names the per-airframe record produces; the mirror of
/// `cs_sim::flight::original::AIRFRAME_FIELDS`.
pub const AIRFRAME_FIELDS: [&str; 15] = [
    "roll_torque",
    "pitch_torque",
    "rudder_torque",
    "level_off_rate",
    "return_rate",
    "ang_momentum_damp",
    "rec_moments_inertia_x",
    "rec_moments_inertia_y",
    "rec_moments_inertia_z",
    "fd_speed",
    "engine_factor",
    "drag_factor",
    "veh_weight",
    "ref_area",
    "gravity",
];

/// The flat field names the global record produces; the mirror of
/// `cs_sim::flight::original::GLOBAL_FIELDS`.
pub const GLOBAL_FIELDS: [&str; 17] = [
    "nom_gravity",
    "lift_aoa_0_deg",
    "lift_aoa_1_deg",
    "max_aoa_deg",
    "high_g_0",
    "high_g_1",
    "low_g_0",
    "low_g_1",
    "lift_accel_rate",
    "stall_mag",
    "turn_fade_in_mph",
    "turn_fade_out_mph",
    "yaw_low_speed",
    "yaw_high_speed",
    "yaw_fade_in_mph",
    "yaw_max_mph",
    "yaw_fade_out_mph",
];

/// The `player.zrd` keys the law reads but never uses: they are recorded with
/// their spans so a reader can see they were parsed and ignored, exactly as
/// the static analysis states (`0x4735b0` parses them; nothing reads them).
pub const UNUSED_GLOBAL_FIELDS: [&str; 2] = ["drag_factor", "drag_fade_speed"];

// --------------------------------------------------------------- errors ---

/// Why an import was refused. No variant is recoverable by substituting a
/// value: a missing or unusable parameter blocks the claim it would back.
#[derive(Debug)]
pub enum OriginalImportError {
    /// The installation could not be discovered or the archive not read.
    Io(String),
    /// The archive dispatched, indexed or decoded through `cs_formats` and
    /// the reader refused it.
    Archive(String),
    /// The named member is not in the archive.
    MemberMissing {
        /// The member the lookup wanted.
        member: String,
    },
    /// A member's bytes are not a decodable `.zrd` document.
    Decode {
        /// Which member failed.
        member: String,
        /// Why.
        detail: String,
    },
    /// The span-recording walk and the production `.zrd` reader disagree, so
    /// no span this module reports can be trusted.
    CrossCheck {
        /// Which member disagreed.
        member: String,
    },
    /// The document does not have the shape the original writes.
    Shape {
        /// What was found instead.
        detail: String,
    },
    /// The requested vehicle record is not in the document.
    RecordMissing {
        /// The record name.
        record: String,
    },
    /// A `kind_of` names a record that is not earlier in the file, which the
    /// original would not resolve either.
    ParentOrder {
        /// The record whose `kind_of` is unusable.
        record: String,
        /// The parent it names.
        parent: String,
    },
    /// A required key is absent from the resolved record.
    KeyMissing {
        /// The record being imported.
        record: String,
        /// The flattened key path, e.g. `dynamics/veh_weight`.
        key: String,
    },
    /// A key that must be a number is not one, or is not finite.
    NotFinite {
        /// The record being imported.
        record: String,
        /// The flattened key path.
        key: String,
    },
    /// The `engine` key names an id or a name the table does not carry.
    EngineMissing {
        /// The record being imported.
        record: String,
        /// The id or name as the record spelled it.
        engine: String,
    },
    /// A `SourceSpan` could not be built from a member's own span.
    Span(String),
}

impl std::fmt::Display for OriginalImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(detail) => write!(f, "the installation could not be read: {detail}"),
            Self::Archive(detail) => write!(f, "the reader archive refused: {detail}"),
            Self::MemberMissing { member } => {
                write!(f, "{READER_ARCHIVE} does not carry {member}")
            }
            Self::Decode { member, detail } => write!(f, "{member} does not decode: {detail}"),
            Self::CrossCheck { member } => write!(
                f,
                "the span walk of {member} disagrees with the production .zrd reader, so its \
                 spans cannot be reported"
            ),
            Self::Shape { detail } => write!(f, "unexpected .zrd document shape: {detail}"),
            Self::RecordMissing { record } => write!(f, "vehicle.zrd has no record {record}"),
            Self::ParentOrder { record, parent } => {
                write!(
                    f,
                    "{record} inherits from {parent}, which is not an earlier record"
                )
            }
            Self::KeyMissing { record, key } => {
                write!(f, "{record} does not state {key}")
            }
            Self::NotFinite { record, key } => {
                write!(f, "{record}'s {key} is not a finite number")
            }
            Self::EngineMissing { record, engine } => {
                write!(
                    f,
                    "{record} names engine {engine}, which engines.zrd does not carry"
                )
            }
            Self::Span(detail) => write!(f, "the member span is unusable: {detail}"),
        }
    }
}

impl std::error::Error for OriginalImportError {}

impl From<std::io::Error> for OriginalImportError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

// ------------------------------------------------------- spanned documents ---

#[derive(Clone, Debug, PartialEq)]
enum SpannedKind {
    Int(u32),
    Float(f32),
    Text(String),
    List(Vec<SpannedValue>),
}

/// One `.zrd` node with the byte range it occupies inside its member.
///
/// The walk records spans and the production [`decode_zrd`] is authoritative
/// for the grammar: [`SpannedDocument::decode`] runs both and refuses the
/// member when they disagree, so a span can never be attributed to the wrong
/// value (`CrossCheck`).
#[derive(Clone, Debug, PartialEq)]
pub struct SpannedValue {
    kind: SpannedKind,
    /// First byte of the node, relative to the member's start.
    pub offset: u64,
    /// Length of the node in bytes.
    pub length: u64,
}

impl SpannedValue {
    /// The value as the production reader spells it.
    #[must_use]
    pub fn to_zrd_value(&self) -> ZrdValue {
        match &self.kind {
            SpannedKind::Int(value) => ZrdValue::Int(*value),
            SpannedKind::Float(value) => ZrdValue::Float(*value),
            SpannedKind::Text(value) => ZrdValue::Text(value.clone()),
            SpannedKind::List(children) => {
                ZrdValue::List(children.iter().map(SpannedValue::to_zrd_value).collect())
            }
        }
    }

    /// The node's children, when it is a list.
    #[must_use]
    pub fn as_list(&self) -> Option<&[SpannedValue]> {
        match &self.kind {
            SpannedKind::List(children) => Some(children),
            _ => None,
        }
    }

    /// The node's text, when it is text.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match &self.kind {
            SpannedKind::Text(text) => Some(text),
            _ => None,
        }
    }

    /// The node as `f64`: an `f32` node, an `int` node, or a one-element list
    /// holding either. Anything else is `None`.
    #[must_use]
    pub fn as_scalar_f64(&self) -> Option<f64> {
        match &self.kind {
            SpannedKind::Float(value) => Some(f64::from(*value)),
            SpannedKind::Int(value) => Some(f64::from(*value)),
            SpannedKind::List(children) if children.len() == 1 => children[0].as_scalar_f64(),
            _ => None,
        }
    }

    /// The node's text: a `text` node or a one-element list holding one.
    #[must_use]
    pub fn as_scalar_text(&self) -> Option<&str> {
        match &self.kind {
            SpannedKind::Text(text) => Some(text),
            SpannedKind::List(children) if children.len() == 1 => children[0].as_scalar_text(),
            _ => None,
        }
    }

    /// The node as `u32`: an `int` node or a one-element list holding one.
    #[must_use]
    pub fn as_scalar_u32(&self) -> Option<u32> {
        match &self.kind {
            SpannedKind::Int(value) => Some(*value),
            SpannedKind::List(children) if children.len() == 1 => children[0].as_scalar_u32(),
            _ => None,
        }
    }

    /// The node's `index`th child, when it is a list.
    #[must_use]
    pub fn element(&self, index: usize) -> Option<&SpannedValue> {
        self.as_list()?.get(index)
    }

    /// Whether this is a keyed record: a list that alternates text keys with
    /// list values, the shape the original writes for `dynamics`,
    /// `engine_sound` and friends.
    ///
    /// A list of rows (`weapons`) or a list of scalars (`start_anims`) is
    /// **not** keyed, which is what keeps the overlay from pairing a row's
    /// first element with its second and calling it a key.
    #[must_use]
    pub fn is_keyed(&self) -> bool {
        let Some(children) = self.as_list() else {
            return false;
        };
        if children.is_empty() || children.len() % 2 != 0 {
            return false;
        }
        children.iter().enumerate().all(|(index, child)| {
            if index % 2 == 0 {
                matches!(child.kind, SpannedKind::Text(_))
            } else {
                matches!(child.kind, SpannedKind::List(_))
            }
        })
    }

    /// The `(key, value)` pairs of a keyed record.
    fn keyed_pairs(&self) -> Vec<(&str, &SpannedValue)> {
        let Some(children) = self.as_list() else {
            return Vec::new();
        };
        children
            .chunks(2)
            .filter_map(|pair| match (pair.first(), pair.get(1)) {
                (Some(key), Some(value)) => key.as_text().map(|key| (key, value)),
                _ => None,
            })
            .collect()
    }
}

/// One member decoded with the byte range of every node.
#[derive(Clone, Debug)]
pub struct SpannedDocument {
    install_sha256: ContentHash,
    member: String,
    member_offset: u64,
    member_sha256: ContentHash,
    root: SpannedValue,
}

impl SpannedDocument {
    /// Decodes `bytes` (member `member`, at `member_offset` in the archive,
    /// digested `member_sha256`) twice: once through the production
    /// [`decode_zrd`] and once with spans, refusing the member if the two
    /// trees differ.
    ///
    /// # Errors
    ///
    /// [`OriginalImportError::Decode`] when either walk fails and
    /// [`OriginalImportError::CrossCheck`] when they disagree.
    pub fn decode(
        install_sha256: ContentHash,
        member: &str,
        bytes: &[u8],
        member_offset: u64,
        member_sha256: ContentHash,
    ) -> Result<Self, OriginalImportError> {
        let production = decode_zrd(bytes).map_err(|error| OriginalImportError::Decode {
            member: member.to_owned(),
            detail: error.to_string(),
        })?;
        let mut walker = Walker { bytes, position: 0 };
        let root = walker
            .node(0)
            .map_err(|detail| OriginalImportError::Decode {
                member: member.to_owned(),
                detail,
            })?;
        if walker.position != bytes.len() {
            return Err(OriginalImportError::Decode {
                member: member.to_owned(),
                detail: format!(
                    "the span walk stopped at {} of {} bytes",
                    walker.position,
                    bytes.len()
                ),
            });
        }
        if root.to_zrd_value() != production {
            return Err(OriginalImportError::CrossCheck {
                member: member.to_owned(),
            });
        }
        Ok(Self {
            install_sha256,
            member: member.to_owned(),
            member_offset,
            member_sha256,
            root,
        })
    }

    /// The member this document was decoded from.
    #[must_use]
    pub fn member(&self) -> &str {
        &self.member
    }

    /// The document's root node.
    #[must_use]
    pub fn root(&self) -> &SpannedValue {
        &self.root
    }

    /// The archive-absolute span of `node`.
    ///
    /// # Errors
    ///
    /// [`OriginalImportError::Span`] when the node's relative range does not
    /// fit the member's.
    pub fn span_of(&self, node: &SpannedValue) -> Result<(u64, u64), OriginalImportError> {
        self.member_offset
            .checked_add(node.offset)
            .ok_or_else(|| OriginalImportError::Span("offset overflow".to_owned()))
            .and_then(|offset| {
                if node.length > u64::MAX - offset {
                    return Err(OriginalImportError::Span("length overflow".to_owned()));
                }
                Ok((offset, node.length))
            })
    }

    /// The provenance of `node`: the installation, the archive, this member
    /// and the node's own bytes.
    ///
    /// # Errors
    ///
    /// [`OriginalImportError::Span`] or [`OriginalImportError::Io`] when the
    /// span or claim id cannot be built.
    pub fn provenance(
        &self,
        node: &SpannedValue,
        claim: &str,
    ) -> Result<Provenance, OriginalImportError> {
        let (offset, length) = self.span_of(node)?;
        let source = SourceSpan::new(
            self.install_sha256,
            READER_ARCHIVE,
            Some(&self.member),
            offset,
            length,
            Some(self.member_sha256),
        )
        .map_err(|error| OriginalImportError::Span(error.to_string()))?;
        let claim_id =
            ClaimId::new(claim).map_err(|error| OriginalImportError::Span(error.to_string()))?;
        Provenance::new(claim_id, ClaimStatus::ObservedTool, Some(source))
            .map_err(|error| OriginalImportError::Span(error.to_string()))
    }
}

/// The span-recording walk: the same grammar `decode_zrd` reads (tags 1–4,
/// a list's count word is its children plus one), keeping byte ranges.
struct Walker<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Walker<'a> {
    fn u32(&mut self) -> Result<u32, String> {
        let end = self
            .position
            .checked_add(4)
            .ok_or_else(|| "offset overflow".to_owned())?;
        let word = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| "truncated".to_owned())?;
        self.position = end;
        Ok(u32::from_le_bytes(word.try_into().expect("four bytes")))
    }

    fn node(&mut self, depth: u32) -> Result<SpannedValue, String> {
        if depth > 64 {
            return Err("depth_exceeded".to_owned());
        }
        let start = self.position;
        let tag = self.u32()?;
        let kind = match tag {
            1 => SpannedKind::Int(self.u32()?),
            2 => {
                let word = self.u32()?;
                SpannedKind::Float(f32::from_bits(word))
            }
            3 => {
                let length = self.u32()? as usize;
                let end = self
                    .position
                    .checked_add(length)
                    .ok_or_else(|| "offset overflow".to_owned())?;
                let body = self
                    .bytes
                    .get(self.position..end)
                    .ok_or_else(|| "truncated".to_owned())?;
                self.position = end;
                SpannedKind::Text(
                    std::str::from_utf8(body)
                        .map_err(|_| "invalid_text".to_owned())?
                        .to_owned(),
                )
            }
            4 => {
                let count = self.u32()?;
                let children = count.saturating_sub(1) as usize;
                let remaining = self.bytes.len().saturating_sub(self.position);
                if children > remaining / 8 + 1 {
                    return Err("count_exceeds_bytes".to_owned());
                }
                let mut kids = Vec::with_capacity(children);
                for _ in 0..children {
                    kids.push(self.node(depth + 1)?);
                }
                SpannedKind::List(kids)
            }
            _ => return Err("unknown_tag".to_owned()),
        };
        let end = self.position;
        Ok(SpannedValue {
            kind,
            offset: start as u64,
            length: (end - start) as u64,
        })
    }
}

// ------------------------------------------------------------- resolution ---

/// One vehicle record after its `kind_of` chain has been resolved and every
/// record's own keys have been overlaid, with the chain it came through.
#[derive(Clone, Debug)]
pub struct ResolvedVehicleRecord {
    /// The inheritance chain, ancestor first, e.g.
    /// `["basic_airplane", "player_airplane", "pbloodhawk"]`.
    pub chain: Vec<String>,
    keys: BTreeMap<String, SpannedValue>,
    sources: BTreeMap<String, String>,
}

impl ResolvedVehicleRecord {
    /// The leaf value at a flattened key path (`dynamics/veh_weight`).
    #[must_use]
    pub fn value(&self, key: &str) -> Option<&SpannedValue> {
        self.keys.get(key)
    }

    /// The record in `chain` the value at `key` came from.
    #[must_use]
    pub fn source_of(&self, key: &str) -> Option<&str> {
        self.sources.get(key).map(String::as_str)
    }

    /// The finite `f64` at `key`, or
    /// [`OriginalImportError::NotFinite`] / `KeyMissing` through
    /// [`ResolvedVehicleRecord::require_f64`].
    ///
    /// # Errors
    ///
    /// [`OriginalImportError::KeyMissing`] when the key is absent,
    /// [`OriginalImportError::NotFinite`] when it is not a finite number.
    pub fn require_f64(&self, record: &str, key: &str) -> Result<f64, OriginalImportError> {
        let node = self
            .value(key)
            .ok_or_else(|| OriginalImportError::KeyMissing {
                record: record.to_owned(),
                key: key.to_owned(),
            })?;
        node.as_scalar_f64()
            .filter(|value| value.is_finite())
            .ok_or_else(|| OriginalImportError::NotFinite {
                record: record.to_owned(),
                key: key.to_owned(),
            })
    }
}

/// The record list of a `vehicle.zrd` document: the root list holds one list
/// that alternates record names and record bodies, in file order (`0x477b70`
/// walks it exactly so).
fn record_list(
    document: &SpannedDocument,
) -> Result<Vec<(&str, &SpannedValue)>, OriginalImportError> {
    let root = document.root();
    let outer = root.as_list().ok_or_else(|| OriginalImportError::Shape {
        detail: "the root is not a list".to_owned(),
    })?;
    if outer.len() != 1 {
        return Err(OriginalImportError::Shape {
            detail: format!("the root holds {} lists, expected one", outer.len()),
        });
    }
    let inner = outer[0]
        .as_list()
        .ok_or_else(|| OriginalImportError::Shape {
            detail: "the root's child is not a list".to_owned(),
        })?;
    if inner.len() % 2 != 0 {
        return Err(OriginalImportError::Shape {
            detail: "the record list has an odd child count".to_owned(),
        });
    }
    let mut records = Vec::with_capacity(inner.len() / 2);
    for pair in inner.chunks(2) {
        let name = pair[0]
            .as_text()
            .ok_or_else(|| OriginalImportError::Shape {
                detail: "a record name is not text".to_owned(),
            })?;
        if pair[1].as_list().is_none() {
            return Err(OriginalImportError::Shape {
                detail: format!("the record {name} is not a list"),
            });
        }
        records.push((name, &pair[1]));
    }
    Ok(records)
}

/// Flattens one record body into `(key path, leaf)` pairs: a keyed record
/// recurses (`dynamics` → `dynamics/veh_weight`), anything else is a leaf.
fn flatten(node: &SpannedValue, prefix: &str, out: &mut Vec<(String, SpannedValue)>) {
    if node.is_keyed() {
        for (key, value) in node.keyed_pairs() {
            let path = if prefix.is_empty() {
                key.to_owned()
            } else {
                format!("{prefix}/{key}")
            };
            flatten(value, &path, out);
        }
        return;
    }
    out.push((prefix.to_owned(), node.clone()));
}

/// Overlays `source` onto `target` the way the original's `0x479240` does:
/// a key the later record states replaces the inherited value, and a key it
/// does not state keeps it.
fn overlay(
    target: &mut BTreeMap<String, SpannedValue>,
    sources: &mut BTreeMap<String, String>,
    source: &SpannedValue,
    record: &str,
) {
    let mut pairs = Vec::new();
    flatten(source, "", &mut pairs);
    for (key, value) in pairs {
        target.insert(key.clone(), value);
        sources.insert(key, record.to_owned());
    }
}

/// Resolves `record` through its `kind_of` chain, copying each earlier
/// record and overlaying the record's own keys.
///
/// # Errors
///
/// [`OriginalImportError::RecordMissing`] for a name the document does not
/// carry, [`OriginalImportError::ParentOrder`] for a `kind_of` that does not
/// name an **earlier** record (the original reads records in file order and
/// only resolves earlier ones), and [`OriginalImportError::Shape`] for a
/// malformed document.
pub fn resolve_vehicle_record(
    document: &SpannedDocument,
    record: &str,
) -> Result<ResolvedVehicleRecord, OriginalImportError> {
    let records = record_list(document)?;
    let position_of = |name: &str| records.iter().position(|(candidate, _)| *candidate == name);

    let mut chain = Vec::new();
    let mut index = position_of(record).ok_or_else(|| OriginalImportError::RecordMissing {
        record: record.to_owned(),
    })?;
    let mut order = Vec::new();
    loop {
        let (name, body) = records[index];
        order.push(index);
        chain.push(name.to_owned());
        let parent = body.value_of_key("kind_of").and_then(|node| {
            node.as_list()
                .and_then(|children| children.first())
                .and_then(SpannedValue::as_text)
        });
        let Some(parent) = parent else {
            break;
        };
        let parent_index =
            position_of(parent).ok_or_else(|| OriginalImportError::RecordMissing {
                record: parent.to_owned(),
            })?;
        if parent_index >= index {
            return Err(OriginalImportError::ParentOrder {
                record: name.to_owned(),
                parent: parent.to_owned(),
            });
        }
        index = parent_index;
    }
    order.reverse();
    chain.reverse();

    let mut keys = BTreeMap::new();
    let mut sources = BTreeMap::new();
    for index in order {
        let (name, body) = records[index];
        overlay(&mut keys, &mut sources, body, name);
    }
    Ok(ResolvedVehicleRecord {
        chain,
        keys,
        sources,
    })
}

impl SpannedValue {
    /// The value of `key` in a flat alternating key/value record.
    fn value_of_key(&self, key: &str) -> Option<&SpannedValue> {
        let children = self.as_list()?;
        let mut index = 0;
        while index < children.len() {
            if let Some(name) = children[index].as_text() {
                if name == key {
                    return children.get(index + 1);
                }
                index += 2;
            } else {
                index += 1;
            }
        }
        None
    }
}

// ---------------------------------------------------------------- records ---

/// One imported value: its field name, its number and where the number was
/// read from.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalFieldValue {
    /// The flat field name (see [`AIRFRAME_FIELDS`] / [`GLOBAL_FIELDS`]).
    pub field: &'static str,
    /// The value, exactly as stored (no clamping, no rounding).
    pub value: f64,
    /// The `.zrd` key path it came from, e.g. `dynamics/pitch_torque` or
    /// `liftAOAs[0]`.
    pub key_path: String,
    /// The record in the inheritance chain the value came from, or `None`
    /// for a global.
    pub source_record: Option<String>,
    /// Where the bytes live.
    pub provenance: Provenance,
}

/// The engine row `engines.zrd` resolved for a record's `engine` key.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalEngine {
    /// The id as the record spelled it.
    pub id: u32,
    /// The table's name for that id.
    pub name: String,
    /// The thrust factor.
    pub factor: f64,
    /// Where the factor was read from.
    pub provenance: Provenance,
}

/// One player airframe's imported parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalAirframeParameters {
    /// The requested record, e.g. `pbloodhawk`.
    pub record: String,
    /// Its `kind_of` chain, ancestor first.
    pub inheritance_chain: Vec<String>,
    /// The `mode` the chain carries (`jet` comes from `basic_airplane`).
    pub mode: Option<String>,
    /// `is_autogyro: 1` selects the fake-dynamics branch.
    pub is_autogyro: bool,
    /// The record's `fuel`, or an explicit unknown when the chain does not
    /// state one (AI records do not).
    pub initial_fuel: Resolved<f64>,
    /// The resolved engine row.
    pub engine: OriginalEngine,
    /// The parameters, grouped by where they came from (`dynamics` scalars,
    /// the inertia triple, the image default, the engine row, gravity) and
    /// looked up by field name with [`Self::value`]: the list is a set of
    /// [`AIRFRAME_FIELDS`] entries, not an ordered one.
    pub values: Vec<OriginalFieldValue>,
}

impl OriginalAirframeParameters {
    /// The flat `(field, value)` list `cs_sim::flight::original` consumes.
    #[must_use]
    pub fn field_values(&self) -> Vec<(&'static str, f64)> {
        self.values
            .iter()
            .map(|entry| (entry.field, entry.value))
            .collect()
    }

    /// One imported value, with its provenance.
    #[must_use]
    pub fn value(&self, field: &str) -> Option<&OriginalFieldValue> {
        self.values.iter().find(|entry| entry.field == field)
    }
}

/// The global flight constants imported from the first `player.zrd` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalGlobalParameters {
    /// The parameters the law consumes, looked up by field name with
    /// [`Self::value`]: the list is a set of [`GLOBAL_FIELDS`] entries, not
    /// an ordered one.
    pub values: Vec<OriginalFieldValue>,
    /// The keys that are parsed but never read by the law, with their spans.
    pub unused: Vec<OriginalFieldValue>,
}

impl OriginalGlobalParameters {
    /// The flat `(field, value)` list `cs_sim::flight::original` consumes.
    #[must_use]
    pub fn law_values(&self) -> Vec<(&'static str, f64)> {
        self.values
            .iter()
            .map(|entry| (entry.field, entry.value))
            .collect()
    }

    /// One consumed value, with its provenance.
    #[must_use]
    pub fn value(&self, field: &str) -> Option<&OriginalFieldValue> {
        self.values.iter().find(|entry| entry.field == field)
    }

    /// One parsed-but-unused value, with its provenance.
    #[must_use]
    pub fn unused_value(&self, field: &str) -> Option<&OriginalFieldValue> {
        self.unused.iter().find(|entry| entry.field == field)
    }
}

// --------------------------------------------------------------- importer ---

/// The three members the flight law reads, decoded once with their spans.
#[derive(Clone, Debug)]
pub struct OriginalDocuments {
    install_sha256: ContentHash,
    vehicle: SpannedDocument,
    engines: SpannedDocument,
    player: SpannedDocument,
}

impl OriginalDocuments {
    /// Reads `ZBD/zrdr.zbd` from `install_root` through the production
    /// discovery, dispatch, trailer-index and reader-archive chain.
    ///
    /// `player.zrd` takes the **first** directory entry of the two that carry
    /// the name, which is the one the original's lookup reads (`0x59ddb0`).
    ///
    /// # Errors
    ///
    /// [`OriginalImportError`] for an undiscoverable installation, an
    /// unreadable or unindexed archive, a missing member or a member that
    /// does not decode.
    pub fn read(install_root: &Path) -> Result<Self, OriginalImportError> {
        let found = cs_assets::install::discover(install_root)
            .map_err(|error| OriginalImportError::Io(error.to_string()))?;
        let install_sha256 = cs_assets::install::fingerprint(&found.manifest);
        let bytes = std::fs::read(install_root.join(READER_ARCHIVE))?;

        let spelling = cs_types::install::RelativePath::new(READER_ARCHIVE)
            .map_err(|error| OriginalImportError::Io(error.to_string()))?;
        let mut context = ParseContext::with_defaults("original_airframe");
        let decision = dispatch(ZbdProbe::new(READER_ARCHIVE, &spelling, &bytes))
            .map_err(|error| OriginalImportError::Archive(error.to_string()))?;
        let index = read_version_one_index(&mut context, decision, &bytes)
            .map_err(|error| OriginalImportError::Archive(error.to_string()))?;
        let table = index.member_table();
        let archive = read_reader_archive(&mut context, &table, index.data())
            .map_err(|error| OriginalImportError::Archive(error.to_string()))?;

        let build = |member: &str| -> Result<SpannedDocument, OriginalImportError> {
            let entry = archive
                .entries()
                .find(|entry| String::from_utf8_lossy(entry.name()).eq_ignore_ascii_case(member))
                .ok_or_else(|| OriginalImportError::MemberMissing {
                    member: member.to_owned(),
                })?;
            let offset = entry.span().offset;
            let content = entry.content().to_vec();
            let member_sha256 = cs_assets::install::sha256(&content);
            SpannedDocument::decode(install_sha256, member, &content, offset, member_sha256)
        };
        Ok(Self {
            install_sha256,
            vehicle: build(VEHICLE_MEMBER)?,
            engines: build(ENGINES_MEMBER)?,
            player: build(PLAYER_MEMBER)?,
        })
    }

    /// The installation fingerprint every span in these documents names.
    #[must_use]
    pub fn install_sha256(&self) -> ContentHash {
        self.install_sha256
    }

    /// The decoded `vehicle.zrd` member.
    #[must_use]
    pub fn vehicle(&self) -> &SpannedDocument {
        &self.vehicle
    }

    /// The decoded `engines.zrd` member.
    #[must_use]
    pub fn engines(&self) -> &SpannedDocument {
        &self.engines
    }

    /// The decoded **first** `player.zrd` member.
    #[must_use]
    pub fn player(&self) -> &SpannedDocument {
        &self.player
    }

    /// Imports one vehicle record's parameters.
    ///
    /// # Errors
    ///
    /// [`OriginalImportError`] for an unknown record, an unresolved
    /// `kind_of`, a missing or non-finite key, or an engine id or name the
    /// table does not carry.
    pub fn airframe(
        &self,
        record: &str,
    ) -> Result<OriginalAirframeParameters, OriginalImportError> {
        let resolved = resolve_vehicle_record(&self.vehicle, record)?;
        let globals = self.globals()?;

        let mut values = Vec::with_capacity(AIRFRAME_FIELDS.len());

        // The nine scalar `dynamics` keys every record in the chain states.
        for field in [
            "roll_torque",
            "pitch_torque",
            "rudder_torque",
            "return_rate",
            "ang_momentum_damp",
            "fd_speed",
            "drag_factor",
            "veh_weight",
            "ref_area",
        ] {
            let key_path = format!("dynamics/{field}");
            let value = resolved.require_f64(record, &key_path)?;
            let node = resolved
                .value(&key_path)
                .expect("require_f64 found the key");
            let source = resolved.source_of(&key_path);
            values.push(self.vehicle_field(record, field, key_path, value, node, source)?);
        }

        // `rec_moments_inertia` is one triple: x pitch, y yaw, z roll.
        let inertia_path = "dynamics/rec_moments_inertia";
        let node = resolved
            .value(inertia_path)
            .ok_or_else(|| OriginalImportError::KeyMissing {
                record: record.to_owned(),
                key: inertia_path.to_owned(),
            })?;
        let source = resolved.source_of(inertia_path);
        for (index, field) in [
            "rec_moments_inertia_x",
            "rec_moments_inertia_y",
            "rec_moments_inertia_z",
        ]
        .into_iter()
        .enumerate()
        {
            let element = node
                .element(index)
                .ok_or_else(|| OriginalImportError::NotFinite {
                    record: record.to_owned(),
                    key: format!("{inertia_path}[{index}]"),
                })?;
            let value = element
                .as_scalar_f64()
                .filter(|value| value.is_finite())
                .ok_or_else(|| OriginalImportError::NotFinite {
                    record: record.to_owned(),
                    key: format!("{inertia_path}[{index}]"),
                })?;
            values.push(self.vehicle_field(
                record,
                field,
                format!("{inertia_path}[{index}]"),
                value,
                element,
                source,
            )?);
        }

        // `level_off_rate`: no record states it; the image's default table
        // gives 4.0 (`0x478a00`), recorded as static analysis, not as data.
        values.push(OriginalFieldValue {
            field: "level_off_rate",
            value: LEVEL_OFF_RATE_DEFAULT,
            key_path: "(image default, 0x478a00)".to_owned(),
            source_record: None,
            provenance: Provenance::new(
                ClaimId::new(LEVEL_OFF_RATE_CLAIM)
                    .map_err(|error| OriginalImportError::Span(error.to_string()))?,
                ClaimStatus::Documented,
                None,
            )
            .map_err(|error| OriginalImportError::Span(error.to_string()))?,
        });

        // The engine factor comes from engines.zrd, keyed by the record's
        // `engine` id (or name); its provenance is the table row's span.
        let engine = self.engine(record, &resolved)?;
        values.push(OriginalFieldValue {
            field: "engine_factor",
            value: engine.factor,
            key_path: "engine".to_owned(),
            source_record: None,
            provenance: engine.provenance.clone(),
        });

        // Gravity: no record sets it, so the law runs on the player.zrd
        // `nom_gravity` the globals carry.
        let gravity_node = resolved.value("gravity");
        match gravity_node {
            Some(node) => {
                let value = node
                    .as_scalar_f64()
                    .filter(|value| value.is_finite())
                    .ok_or_else(|| OriginalImportError::NotFinite {
                        record: record.to_owned(),
                        key: "gravity".to_owned(),
                    })?;
                values.push(self.vehicle_field(
                    record,
                    "gravity",
                    "gravity".to_owned(),
                    value,
                    node,
                    resolved.source_of("gravity"),
                )?);
            }
            None => {
                let nominal = globals
                    .value("nom_gravity")
                    .expect("globals always carry nom_gravity");
                values.push(OriginalFieldValue {
                    field: "gravity",
                    value: nominal.value,
                    key_path: "player.zrd nom_gravity".to_owned(),
                    source_record: None,
                    provenance: nominal.provenance.clone(),
                });
            }
        }

        let mode = resolved
            .value("mode")
            .and_then(SpannedValue::as_scalar_text)
            .map(str::to_owned);
        let is_autogyro = resolved
            .value("is_autogyro")
            .and_then(SpannedValue::as_scalar_u32)
            .is_some_and(|value| value != 0);
        let initial_fuel = match resolved.value("fuel") {
            Some(node) => {
                let value = node
                    .as_scalar_f64()
                    .filter(|value| value.is_finite())
                    .ok_or_else(|| OriginalImportError::NotFinite {
                        record: record.to_owned(),
                        key: "fuel".to_owned(),
                    })?;
                let claim = format!("f796.airframe.{record}.fuel");
                Resolved::Known(cs_types::content::Known::new(
                    value,
                    self.vehicle.provenance(node, &claim)?,
                ))
            }
            None => Resolved::unknown(
                ClaimId::new(&format!("f796.airframe.{record}.fuel"))
                    .map_err(|error| OriginalImportError::Span(error.to_string()))?,
                "no record in the inheritance chain states `fuel`, and the original's spawn fuel \
                 for such an airframe was not recovered",
            )
            .map_err(|error| OriginalImportError::Span(error.to_string()))?,
        };

        Ok(OriginalAirframeParameters {
            record: record.to_owned(),
            inheritance_chain: resolved.chain.clone(),
            mode,
            is_autogyro,
            initial_fuel,
            engine,
            values,
        })
    }

    /// One value read out of `vehicle.zrd`, with its provenance and the
    /// record in the chain the value came through.
    fn vehicle_field(
        &self,
        record: &str,
        field: &'static str,
        key_path: String,
        value: f64,
        node: &SpannedValue,
        source: Option<&str>,
    ) -> Result<OriginalFieldValue, OriginalImportError> {
        let claim = match source {
            Some(source) => format!("f796.airframe.{record}.{source}.{field}"),
            None => format!("f796.airframe.{record}.{field}"),
        };
        let provenance = self.vehicle.provenance(node, &claim)?;
        Ok(OriginalFieldValue {
            field,
            value,
            key_path,
            source_record: source.map(str::to_owned),
            provenance,
        })
    }

    /// Resolves a record's `engine` key against `engines.zrd`.
    ///
    /// # Errors
    ///
    /// [`OriginalImportError::EngineMissing`] when the table has no row for
    /// the id or name, [`OriginalImportError::NotFinite`] when the row's
    /// factor is not a finite number.
    pub fn engine(
        &self,
        record: &str,
        resolved: &ResolvedVehicleRecord,
    ) -> Result<OriginalEngine, OriginalImportError> {
        let node = resolved
            .value("engine")
            .ok_or_else(|| OriginalImportError::KeyMissing {
                record: record.to_owned(),
                key: "engine".to_owned(),
            })?;
        let id = node
            .as_scalar_u32()
            .ok_or_else(|| OriginalImportError::NotFinite {
                record: record.to_owned(),
                key: "engine".to_owned(),
            })?;
        let rows = engine_rows(&self.engines)?;
        let row = rows.iter().find(|row| row.id == id).ok_or_else(|| {
            OriginalImportError::EngineMissing {
                record: record.to_owned(),
                engine: id.to_string(),
            }
        })?;
        Ok(OriginalEngine {
            id,
            name: row.name.clone(),
            factor: row.factor,
            provenance: self
                .engines
                .provenance(&row.factor_node, &format!("f796.engine.{id}"))?,
        })
    }

    /// Imports the global constants from the first `player.zrd` entry.
    ///
    /// # Errors
    ///
    /// [`OriginalImportError`] for a missing or non-finite key.
    pub fn globals(&self) -> Result<OriginalGlobalParameters, OriginalImportError> {
        let root = self.player.root();
        let document = root
            .as_list()
            .and_then(|outer| outer.first())
            .ok_or_else(|| OriginalImportError::Shape {
                detail: "player.zrd does not open with a list".to_owned(),
            })?;

        let mut values = Vec::with_capacity(GLOBAL_FIELDS.len());
        let mut unused = Vec::with_capacity(UNUSED_GLOBAL_FIELDS.len());
        for (field, key) in [
            ("nom_gravity", "nom_gravity"),
            ("max_aoa_deg", "maxAOA"),
            ("lift_accel_rate", "lift_accel_rate"),
            ("stall_mag", "stall_mag"),
            ("yaw_low_speed", "yaw_low_speed"),
            ("yaw_high_speed", "yaw_high_speed"),
        ] {
            values.push(self.global_scalar(document, field, key)?);
        }
        for (field, key, index) in [
            ("lift_aoa_0_deg", "liftAOAs", 0usize),
            ("lift_aoa_1_deg", "liftAOAs", 1),
            ("high_g_0", "highGs", 0),
            ("high_g_1", "highGs", 1),
            ("low_g_0", "lowGs", 0),
            ("low_g_1", "lowGs", 1),
            ("turn_fade_in_mph", "turn_fade_in", 0),
            ("turn_fade_out_mph", "turn_fade_out", 0),
            ("yaw_fade_in_mph", "yaw_fade_in", 0),
            ("yaw_max_mph", "yaw_max", 0),
            ("yaw_fade_out_mph", "yaw_fade_out", 0),
        ] {
            values.push(self.global_element(document, field, key, index)?);
        }
        for field in UNUSED_GLOBAL_FIELDS {
            unused.push(self.global_scalar(document, field, field)?);
        }

        // The vocabulary and the produced fields must agree as a set, or a
        // rename on either side would silently drop a value. Duplicates are
        // refused too, so the comparison stays a set.
        let mut produced: Vec<&str> = values.iter().map(|entry| entry.field).collect();
        let mut declared: Vec<&str> = GLOBAL_FIELDS.to_vec();
        produced.sort_unstable();
        declared.sort_unstable();
        if produced != declared {
            return Err(OriginalImportError::Shape {
                detail: format!("global fields {produced:?} are not the declared vocabulary"),
            });
        }
        Ok(OriginalGlobalParameters { values, unused })
    }

    fn global_scalar(
        &self,
        document: &SpannedValue,
        field: &'static str,
        key: &str,
    ) -> Result<OriginalFieldValue, OriginalImportError> {
        let node = document
            .value_of_key(key)
            .ok_or_else(|| OriginalImportError::KeyMissing {
                record: PLAYER_MEMBER.to_owned(),
                key: key.to_owned(),
            })?;
        let element = node.element(0).unwrap_or(node);
        let value = element
            .as_scalar_f64()
            .filter(|value| value.is_finite())
            .ok_or_else(|| OriginalImportError::NotFinite {
                record: PLAYER_MEMBER.to_owned(),
                key: key.to_owned(),
            })?;
        Ok(self.field_value(document, field, key, value, element))
    }

    fn global_element(
        &self,
        document: &SpannedValue,
        field: &'static str,
        key: &str,
        index: usize,
    ) -> Result<OriginalFieldValue, OriginalImportError> {
        let node = document
            .value_of_key(key)
            .ok_or_else(|| OriginalImportError::KeyMissing {
                record: PLAYER_MEMBER.to_owned(),
                key: key.to_owned(),
            })?;
        let element = node
            .element(index)
            .ok_or_else(|| OriginalImportError::NotFinite {
                record: PLAYER_MEMBER.to_owned(),
                key: format!("{key}[{index}]"),
            })?;
        let value = element
            .as_scalar_f64()
            .filter(|value| value.is_finite())
            .ok_or_else(|| OriginalImportError::NotFinite {
                record: PLAYER_MEMBER.to_owned(),
                key: format!("{key}[{index}]"),
            })?;
        Ok(self.field_value(document, field, &format!("{key}[{index}]"), value, element))
    }

    fn field_value(
        &self,
        _document: &SpannedValue,
        field: &'static str,
        key_path: &str,
        value: f64,
        node: &SpannedValue,
    ) -> OriginalFieldValue {
        let claim = format!("f796.globals.{field}");
        let provenance = self
            .player
            .provenance(node, &claim)
            .expect("the member span is valid for its own nodes");
        OriginalFieldValue {
            field,
            value,
            key_path: key_path.to_owned(),
            source_record: None,
            provenance,
        }
    }
}

/// One `engines.zrd` row.
#[derive(Clone, Debug)]
struct EngineRow {
    id: u32,
    name: String,
    factor: f64,
    factor_node: SpannedValue,
}

/// Every `engines.zrd` row: `(id "name" factor)` in file order.
fn engine_rows(document: &SpannedDocument) -> Result<Vec<EngineRow>, OriginalImportError> {
    let rows = document
        .root()
        .as_list()
        .ok_or_else(|| OriginalImportError::Shape {
            detail: "engines.zrd is not a list".to_owned(),
        })?;
    let mut parsed = Vec::with_capacity(rows.len());
    for row in rows {
        let children = row.as_list().ok_or_else(|| OriginalImportError::Shape {
            detail: "an engine row is not a list".to_owned(),
        })?;
        if children.len() != 3 {
            return Err(OriginalImportError::Shape {
                detail: format!("an engine row has {} fields, expected 3", children.len()),
            });
        }
        let id = children[0]
            .as_scalar_u32()
            .ok_or_else(|| OriginalImportError::Shape {
                detail: "an engine id is not an int".to_owned(),
            })?;
        let name = children[1]
            .as_text()
            .ok_or_else(|| OriginalImportError::Shape {
                detail: "an engine name is not text".to_owned(),
            })?
            .to_owned();
        let factor = children[2]
            .as_scalar_f64()
            .filter(|value| value.is_finite())
            .ok_or_else(|| OriginalImportError::Shape {
                detail: "an engine factor is not a finite number".to_owned(),
            })?;
        parsed.push(EngineRow {
            id,
            name,
            factor,
            factor_node: children[2].clone(),
        });
    }
    Ok(parsed)
}

/// Reads one vehicle record straight from `install_root`.
///
/// # Errors
///
/// [`OriginalImportError`]; see [`OriginalDocuments::read`] and
/// [`OriginalDocuments::airframe`].
pub fn import_retail_airframe(
    install_root: &Path,
    record: &str,
) -> Result<OriginalAirframeParameters, OriginalImportError> {
    OriginalDocuments::read(install_root)?.airframe(record)
}

/// Reads the globals straight from `install_root`.
///
/// # Errors
///
/// [`OriginalImportError`]; see [`OriginalDocuments::read`].
pub fn import_retail_globals(
    install_root: &Path,
) -> Result<OriginalGlobalParameters, OriginalImportError> {
    OriginalDocuments::read(install_root)?.globals()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------- a synthetic zrd --

    fn node_int(value: u32) -> Vec<u8> {
        let mut bytes = 1_u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&value.to_le_bytes());
        bytes
    }

    fn node_float(value: f32) -> Vec<u8> {
        let mut bytes = 2_u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
        bytes
    }

    fn node_text(value: &str) -> Vec<u8> {
        let mut bytes = 3_u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
        bytes
    }

    fn node_list(children: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = 4_u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&((children.len() + 1) as u32).to_le_bytes());
        for child in children {
            bytes.extend_from_slice(child);
        }
        bytes
    }

    fn single_float(value: f32) -> Vec<u8> {
        node_list(&[node_float(value)])
    }

    fn single_text(value: &str) -> Vec<u8> {
        node_list(&[node_text(value)])
    }

    fn single_int(value: u32) -> Vec<u8> {
        node_list(&[node_int(value)])
    }

    /// A synthetic `vehicle.zrd` in the original's own shape: the root list
    /// holds one list alternating record names and record bodies, in file
    /// order, with `kind_of` chains, a partial `dynamics` block and a row
    /// list that must not be mistaken for a keyed record.
    fn synthetic_vehicle_zrd() -> Vec<u8> {
        let basic = node_list(&[
            node_text("mode"),
            single_text("jet"),
            node_text("dynamics"),
            node_list(&[
                node_text("pitch_torque"),
                single_float(2.4),
                node_text("veh_weight"),
                single_float(3500.0),
            ]),
            node_text("engine"),
            single_int(0),
            // A list of rows, not a keyed record: the overlay must leave it
            // whole instead of pairing `wep_00` with `6`.
            node_text("weapons"),
            node_list(&[node_list(&[node_text("wep_00"), node_int(6)])]),
        ]);
        let player = node_list(&[
            node_text("kind_of"),
            single_text("basic_airplane"),
            node_text("fuel"),
            single_float(54926.0),
            node_text("dynamics"),
            node_list(&[node_text("veh_weight"), single_float(3000.0)]),
        ]);
        let child = node_list(&[
            node_text("kind_of"),
            single_text("player_airplane"),
            node_text("engine"),
            single_int(11),
            node_text("dynamics"),
            node_list(&[node_text("veh_weight"), single_float(1900.0)]),
        ]);
        let forward = node_list(&[
            node_text("kind_of"),
            single_text("zlater"),
            node_text("dynamics"),
            node_list(&[node_text("veh_weight"), single_float(1.0)]),
        ]);
        let later = node_list(&[node_text("title"), single_text("LATER")]);
        node_list(&[node_list(&[
            node_text("basic_airplane"),
            basic,
            node_text("player_airplane"),
            player,
            node_text("ptest"),
            child,
            node_text("zforward"),
            forward,
            node_text("zlater"),
            later,
        ])])
    }

    fn synthetic_document() -> SpannedDocument {
        let bytes = synthetic_vehicle_zrd();
        SpannedDocument::decode(
            cs_types::evidence::ContentHash::from_bytes([7; 32]),
            VEHICLE_MEMBER,
            &bytes,
            1_000,
            cs_types::evidence::ContentHash::from_bytes([9; 32]),
        )
        .expect("the synthetic document decodes under both readers")
    }

    /// The `kind_of` chain is a copy of every earlier record and then an
    /// overlay of the record's own keys: an inherited key survives when the
    /// record does not state it, a stated one replaces it, a row list stays
    /// whole, and every value keeps the span it was read from.
    #[test]
    fn accept_flight_original_synthetic_vehicle_zrd_is_kind_of_copy_then_overlay() {
        let document = synthetic_document();
        let resolved =
            resolve_vehicle_record(&document, "ptest").expect("the synthetic record resolves");

        assert_eq!(
            resolved.chain,
            ["basic_airplane", "player_airplane", "ptest"],
            "the chain is ancestors first"
        );

        // Copied from the root record two levels up.
        assert_eq!(
            resolved
                .value("dynamics/pitch_torque")
                .and_then(SpannedValue::as_scalar_f64),
            Some(f64::from(2.4_f32)),
            "an unset dynamics key is inherited"
        );
        assert_eq!(
            resolved.source_of("dynamics/pitch_torque"),
            Some("basic_airplane")
        );
        assert_eq!(
            resolved
                .value("mode")
                .and_then(SpannedValue::as_scalar_text),
            Some("jet"),
            "`mode \"jet\"` comes from basic_airplane"
        );
        assert_eq!(
            resolved.value("fuel").and_then(SpannedValue::as_scalar_f64),
            Some(54926.0),
            "the intermediate record's `fuel` survives"
        );

        // Overlaid by the record itself, and by the intermediate record when
        // the leaf record does not state it.
        assert_eq!(
            resolved
                .value("dynamics/veh_weight")
                .and_then(SpannedValue::as_scalar_f64),
            Some(1900.0),
            "the record's own key overlays the inherited one"
        );
        assert_eq!(resolved.source_of("dynamics/veh_weight"), Some("ptest"));

        let overlaid = resolve_vehicle_record(&document, "player_airplane")
            .expect("the intermediate record resolves");
        assert_eq!(
            overlaid
                .value("dynamics/veh_weight")
                .and_then(SpannedValue::as_scalar_f64),
            Some(3000.0)
        );
        assert_eq!(
            overlaid.source_of("dynamics/veh_weight"),
            Some("player_airplane")
        );
        assert_eq!(
            overlaid
                .value("dynamics/pitch_torque")
                .and_then(SpannedValue::as_scalar_f64),
            Some(f64::from(2.4_f32)),
            "a partial `dynamics` block keeps the keys it does not state"
        );

        // A list of rows is one value, not a keyed record.
        assert!(
            resolved.value("weapons").is_some(),
            "the row list is a leaf"
        );
        assert!(
            resolved.value("weapons/wep_00").is_none(),
            "a row's first element is not a key"
        );

        // The reported span really is the value's bytes: tag 2 then the f32.
        let node = resolved
            .value("dynamics/veh_weight")
            .expect("the key exists");
        let (offset, length) = document.span_of(node).expect("the span fits the member");
        let bytes = synthetic_vehicle_zrd();
        let start = (offset - 1_000) as usize;
        let range = &bytes[start..start + length as usize];
        // The node is the `[float]` list the document writes: list tag, count
        // word, then the float tag and its payload.
        assert_eq!(length, 16, "a one-element float list spans its whole node");
        assert_eq!(
            u32::from_le_bytes(range[0..4].try_into().expect("the tag")),
            4,
            "the span starts at the list tag"
        );
        assert_eq!(
            u32::from_le_bytes(range[8..12].try_into().expect("the inner tag")),
            2,
            "and holds the float tag"
        );
        assert_eq!(
            f32::from_le_bytes(range[12..16].try_into().expect("the payload")).to_bits(),
            1900.0_f32.to_bits()
        );

        // And the provenance names the installation, container and member.
        let field = document
            .provenance(node, "f796.test.veh_weight")
            .expect("the provenance builds");
        let source = field.source.as_ref().expect("a data value has a source");
        assert_eq!(source.container_path(), READER_ARCHIVE);
        assert_eq!(source.member_key(), Some(VEHICLE_MEMBER));
        assert_eq!(source.offset(), offset);
        assert_eq!(source.length(), length);
        assert_eq!(field.class, ClaimStatus::ObservedTool);
        assert!(!matches!(field.class, ClaimStatus::VerifiedOriginal));
    }

    /// An unknown record and a `kind_of` that does not name an earlier record
    /// are both refused, never resolved against something else.
    #[test]
    fn accept_flight_original_unresolvable_records_are_refused() {
        let document = synthetic_document();
        assert!(matches!(
            resolve_vehicle_record(&document, "not_a_record"),
            Err(OriginalImportError::RecordMissing { record }) if record == "not_a_record"
        ));
        assert!(matches!(
            resolve_vehicle_record(&document, "zforward"),
            Err(OriginalImportError::ParentOrder { record, parent })
                if record == "zforward" && parent == "zlater"
        ));
    }
}
