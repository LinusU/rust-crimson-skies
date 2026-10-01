//! The declared-layout, bounded legacy-profile reader (stage F64-A).
//!
//! A legacy profile, save or custom-aircraft file is **unmeasured**: this
//! stage did not open one, so it does not know the byte layout, the version
//! field or the id encoding of any original file. What it defines instead is
//! the reading *contract* those measurements will plug into:
//!
//! * a [`LegacyLayout`] is **data** — a magic, fixed-width header slots, a
//!   declared record-count field, fixed-width record slots, the slots that
//!   carry an id and the evidence state of the whole declaration. F64-B
//!   supplies the measured layout as such a value; nothing about a layout is
//!   hard-coded in the reader, so a measurement is an addition, not a rewrite.
//! * Every extent is **declared**, never read from the input. The input cannot
//!   say how long anything is, which removes the classic hostile-allocation
//!   path: the only number an untrusted file controls is its record count.
//! * That one number is checked against [`LegacyLimits::max_records`] and
//!   against the F03 [`AllocationBudget`] **before** a `Vec` is reserved, and
//!   the record table's extent is checked against the input length with
//!   overflow-checked arithmetic.
//! * Every declared field's width is checked against the limits **before** the
//!   first read, so a layout that declares an absurd field is refused without a
//!   single byte being interpreted.
//! * Everything the layout does not name is **retained, never dropped or
//!   interpreted**: record bytes past the last declared slot are kept in
//!   [`LegacyRecord::undeclared`], bytes past the record table are kept in
//!   [`LegacyProfileDocument::trailing`], and a declared `Text` slot that is
//!   not valid UTF-8 is a *refusal*, not a lossy conversion to some other
//!   type. The reader has no field semantics at all, so it cannot guess what a
//!   value means (spec F64 non-negotiable 2).
//!
//! **Designed, not original.** The only layout shipped here is
//! [`synthetic_layout`], a fixture layout marked [`ClaimStatus::Designed`] used
//! by the acceptance tests. It is not a claim about any original file, and
//! [`LegacyLayout::evidence`] is what the import layer gates on.

use std::collections::BTreeMap;
use std::fmt;

use cs_types::evidence::ClaimStatus;

use crate::error::{ParseError, ParseErrorKind};
use crate::io::{AllocationBudget, Reader};

/// Length of the magic every layout declares.
pub const LEGACY_MAGIC_BYTES: usize = 8;

/// The classes of legacy id a layout may declare a slot for.
///
/// A vocabulary, not a claim: a slot is declared as carrying an id *of one of
/// these classes*, and the class is what the import layer resolves through
/// verified content identities. The numeric values are opaque — what a raw id
/// means is decided by the measured id table, never by its magnitude.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LegacyIdClass {
    /// An airframe.
    Airframe,
    /// A weapon.
    Weapon,
    /// A rocket or other ordnance item.
    Ordnance,
    /// An engine.
    Engine,
    /// A mission.
    Mission,
}

impl LegacyIdClass {
    /// Every class, in inventory order.
    pub const ALL: [Self; 5] = [
        Self::Airframe,
        Self::Weapon,
        Self::Ordnance,
        Self::Engine,
        Self::Mission,
    ];

    /// Stable label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Airframe => "airframe",
            Self::Weapon => "weapon",
            Self::Ordnance => "ordnance",
            Self::Engine => "engine",
            Self::Mission => "mission",
        }
    }
}

impl fmt::Display for LegacyIdClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The declared type of one fixed-width slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacySlotType {
    /// A little-endian `u8`.
    U8,
    /// A little-endian `u16`.
    U16,
    /// A little-endian `u32`.
    U32,
    /// A little-endian `u64`.
    U64,
    /// `len` bytes that must be valid UTF-8.
    Text {
        /// Declared extent in bytes, including any padding.
        len: usize,
    },
    /// `len` opaque bytes.
    Bytes {
        /// Declared extent in bytes.
        len: usize,
    },
}

impl LegacySlotType {
    /// The declared extent of the slot in bytes.
    #[must_use]
    pub const fn extent(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::U16 => 2,
            Self::U32 => 4,
            Self::U64 => 8,
            Self::Text { len } | Self::Bytes { len } => len,
        }
    }

    /// Whether the slot carries a small unsigned integer.
    #[must_use]
    pub const fn is_integer(self) -> bool {
        matches!(self, Self::U8 | Self::U16 | Self::U32 | Self::U64)
    }
}

/// One declared fixed-width field of a header or record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacySlot {
    name: String,
    ty: LegacySlotType,
    offset: usize,
}

impl LegacySlot {
    /// A slot named `name`, of type `ty`, at `offset` from the start of the
    /// header or record it belongs to.
    pub fn new(name: impl Into<String>, ty: LegacySlotType, offset: usize) -> Self {
        Self {
            name: name.into(),
            ty,
            offset,
        }
    }

    /// The declared field name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The declared type.
    pub fn ty(&self) -> LegacySlotType {
        self.ty
    }

    /// The declared offset from the start of the enclosing header or record.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// The first byte past this slot.
    pub fn end(&self) -> usize {
        self.offset.saturating_add(self.ty.extent())
    }
}

/// One declared id-carrying slot of a record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyIdSlot {
    field: String,
    class: LegacyIdClass,
}

impl LegacyIdSlot {
    /// The record slot `field` carries a legacy id of class `class`.
    pub fn new(field: impl Into<String>, class: LegacyIdClass) -> Self {
        Self {
            field: field.into(),
            class,
        }
    }

    /// The record slot that carries the id.
    pub fn field(&self) -> &str {
        &self.field
    }

    /// The class the id belongs to.
    pub fn class(&self) -> LegacyIdClass {
        self.class
    }
}

/// What the reader does with bytes past the record table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrailingPolicy {
    /// Refuse a document that has bytes the layout does not account for.
    Reject,
    /// Keep them in [`LegacyProfileDocument::trailing`]. This is the default:
    /// a layout that has not been measured against a real file may legitimately
    /// stop short, and dropping the difference would hide it.
    Retain,
}

/// Why a layout declaration was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LegacyLayoutError {
    /// The layout declares no record field, so a record carries nothing.
    NoRecordFields,
    /// The named record-count field is not a declared integer header slot.
    RecordCountNotDeclared {
        /// The declared field name.
        field: String,
    },
    /// The named version-major field is not a declared integer header slot.
    VersionFieldNotDeclared {
        /// The declared field name.
        field: String,
    },
    /// A declared id slot names a record field the layout does not declare.
    IdSlotNotDeclared {
        /// The declared field name.
        field: String,
    },
    /// Two slots of the same header or record share a name.
    DuplicateField {
        /// The repeated field name.
        field: String,
    },
    /// A declared slot's `offset + extent` overflows the address space.
    SlotExtentOverflow {
        /// The field name.
        field: String,
    },
    /// Two declared slots of the same group overlap.
    OverlappingSlots {
        /// The field name that overlaps.
        field: String,
    },
    /// A declared record size is below the end of the last declared slot.
    RecordSizeTooSmall {
        /// The smallest size the declaration needs.
        declared: usize,
        /// The size that was asked for.
        requested: usize,
    },
    /// A declared slot overlaps the magic.
    SlotOverlapsMagic {
        /// The field name.
        field: String,
    },
}

impl fmt::Display for LegacyLayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRecordFields => write!(f, "the layout declares no record field"),
            Self::RecordCountNotDeclared { field } => {
                write!(
                    f,
                    "record-count field {field:?} is not a declared integer slot"
                )
            }
            Self::VersionFieldNotDeclared { field } => {
                write!(f, "version field {field:?} is not a declared integer slot")
            }
            Self::IdSlotNotDeclared { field } => {
                write!(f, "id slot {field:?} names no declared record field")
            }
            Self::DuplicateField { field } => write!(f, "field {field:?} is declared twice"),
            Self::SlotExtentOverflow { field } => {
                write!(f, "field {field:?} has an extent that overflows")
            }
            Self::OverlappingSlots { field } => {
                write!(f, "field {field:?} overlaps another slot")
            }
            Self::RecordSizeTooSmall {
                declared,
                requested,
            } => write!(
                f,
                "a record size of {requested} is below the {declared} bytes the \
                 declared record fields need"
            ),
            Self::SlotOverlapsMagic { field } => {
                write!(f, "field {field:?} overlaps the leading magic")
            }
        }
    }
}

impl std::error::Error for LegacyLayoutError {}

/// A declared byte layout of one legacy document, with its evidence state.
///
/// The layout is the *only* thing that says what the bytes mean. Building one
/// is a measurement (F64-B) or a design ([`synthetic_layout`]); validating one
/// is [`LegacyLayout::validate`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyLayout {
    id: String,
    evidence: ClaimStatus,
    magic: [u8; LEGACY_MAGIC_BYTES],
    header: Vec<LegacySlot>,
    version_major_field: String,
    version_minor_field: String,
    supported_version_major: u32,
    record_count_field: String,
    record: Vec<LegacySlot>,
    record_size: usize,
    id_refs: Vec<LegacyIdSlot>,
    trailing: TrailingPolicy,
}

impl LegacyLayout {
    /// Declares a layout.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        evidence: ClaimStatus,
        magic: [u8; LEGACY_MAGIC_BYTES],
        header: Vec<LegacySlot>,
        version_major_field: impl Into<String>,
        version_minor_field: impl Into<String>,
        supported_version_major: u32,
        record_count_field: impl Into<String>,
        record: Vec<LegacySlot>,
        id_refs: Vec<LegacyIdSlot>,
        trailing: TrailingPolicy,
    ) -> Self {
        Self {
            id: id.into(),
            evidence,
            magic,
            header,
            version_major_field: version_major_field.into(),
            version_minor_field: version_minor_field.into(),
            supported_version_major,
            record_count_field: record_count_field.into(),
            record,
            record_size: 0,
            id_refs,
            trailing,
        }
    }

    /// Declares the fixed size of one record.
    ///
    /// A record may be wider than its last declared slot — a measured layout
    /// knows the record's real stride even when it has not accounted for every
    /// field in it. The difference is **retained** per record in
    /// [`LegacyRecord::undeclared`] rather than skipped, which is what lets the
    /// import layer report such a record as unresolved instead of silently
    /// short. Without this call the record size is exactly the end of the last
    /// declared slot.
    ///
    /// # Errors
    ///
    /// [`LegacyLayoutError::RecordSizeTooSmall`] when `record_size` is below
    /// the end of the last declared slot, which would mean reading the next
    /// record's bytes as this one's fields.
    pub fn with_record_size(mut self, record_size: usize) -> Result<Self, LegacyLayoutError> {
        if record_size < self.record_bytes() {
            return Err(LegacyLayoutError::RecordSizeTooSmall {
                declared: self.record_bytes(),
                requested: record_size,
            });
        }
        self.record_size = record_size;
        Ok(self)
    }

    /// The layout's label, used as the reader's container provenance.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The evidence state of this declaration.
    ///
    /// [`ClaimStatus::Designed`] for the shipped fixture layout and
    /// `VerifiedOriginal` only for a layout read from fingerprinted original
    /// bytes. The import layer gates on this, so a developer placeholder
    /// cannot pass as an importable original format.
    pub fn evidence(&self) -> ClaimStatus {
        self.evidence
    }

    /// The declared magic.
    pub fn magic(&self) -> [u8; LEGACY_MAGIC_BYTES] {
        self.magic
    }

    /// The version major this build can read.
    pub fn supported_version_major(&self) -> u32 {
        self.supported_version_major
    }

    /// What the reader does with bytes past the record table.
    pub fn trailing_policy(&self) -> TrailingPolicy {
        self.trailing
    }

    /// The declared id-carrying record slots.
    pub fn id_refs(&self) -> &[LegacyIdSlot] {
        &self.id_refs
    }

    /// The declared header slots.
    pub fn header_slots(&self) -> &[LegacySlot] {
        &self.header
    }

    /// The declared record slots.
    pub fn record_slots(&self) -> &[LegacySlot] {
        &self.record
    }

    /// The first byte past the header, counting the magic.
    pub fn header_bytes(&self) -> usize {
        self.header
            .iter()
            .map(LegacySlot::end)
            .max()
            .unwrap_or(LEGACY_MAGIC_BYTES)
    }

    /// The declared size of one record.
    ///
    /// Returns `0` for a layout with no record field, which
    /// [`Self::validate`] refuses.
    pub fn record_bytes(&self) -> usize {
        self.record.iter().map(LegacySlot::end).max().unwrap_or(0)
    }

    /// The size of one record in the table, which is the end of the last
    /// declared slot unless [`Self::with_record_size`] declared a wider stride.
    pub fn record_size(&self) -> usize {
        self.record_size.max(self.record_bytes())
    }

    /// Checks the declaration's own rules.
    ///
    /// # Errors
    ///
    /// [`LegacyLayoutError`] when the declaration could not describe a
    /// document: a record with no field, a record-count or version field that
    /// is not a declared integer slot, an id slot naming no field, a repeated
    /// field name, a slot whose extent overflows, or two slots that overlap.
    pub fn validate(&self) -> Result<(), LegacyLayoutError> {
        if self.record.is_empty() {
            return Err(LegacyLayoutError::NoRecordFields);
        }
        self.check_slots(&self.header, LEGACY_MAGIC_BYTES)?;
        self.check_slots(&self.record, 0)?;
        self.integer_header_slot(&self.record_count_field)
            .map_err(|_| LegacyLayoutError::RecordCountNotDeclared {
                field: self.record_count_field.clone(),
            })?;
        for field in [&self.version_major_field, &self.version_minor_field] {
            self.integer_header_slot(field).map_err(|_| {
                LegacyLayoutError::VersionFieldNotDeclared {
                    field: field.clone(),
                }
            })?;
        }
        for id_ref in &self.id_refs {
            if !self.record.iter().any(|slot| slot.name() == id_ref.field()) {
                return Err(LegacyLayoutError::IdSlotNotDeclared {
                    field: id_ref.field().to_owned(),
                });
            }
        }
        Ok(())
    }

    /// The declared integer header slot named `field`, if it is one.
    fn integer_header_slot(&self, field: &str) -> Result<&LegacySlot, ()> {
        self.header
            .iter()
            .find(|slot| slot.name() == field && slot.ty().is_integer())
            .ok_or(())
    }

    /// Checks one slot group for overflow, duplicate names and overlap.
    ///
    /// `base` is the first offset the group may use: a header slot may not
    /// reach into the magic, while a record slot starts at its own record's
    /// zero.
    fn check_slots(&self, slots: &[LegacySlot], base: usize) -> Result<(), LegacyLayoutError> {
        let mut spans: Vec<(usize, usize)> = Vec::with_capacity(slots.len());
        let mut seen: Vec<&str> = Vec::with_capacity(slots.len());
        for slot in slots {
            if seen.contains(&slot.name()) {
                return Err(LegacyLayoutError::DuplicateField {
                    field: slot.name().to_owned(),
                });
            }
            seen.push(slot.name());
            let Some(end) = slot.offset().checked_add(slot.ty().extent()) else {
                return Err(LegacyLayoutError::SlotExtentOverflow {
                    field: slot.name().to_owned(),
                });
            };
            if slot.offset() < base {
                return Err(LegacyLayoutError::SlotOverlapsMagic {
                    field: slot.name().to_owned(),
                });
            }
            spans.push((slot.offset(), end));
        }
        spans.sort_unstable();
        for pair in spans.windows(2) {
            if pair[0].1 > pair[1].0 {
                let name = slots
                    .iter()
                    .find(|slot| slot.offset() >= pair[1].0)
                    .map_or_else(|| slots[0].name().to_owned(), |slot| slot.name().to_owned());
                return Err(LegacyLayoutError::OverlappingSlots { field: name });
            }
        }
        Ok(())
    }
}

/// Bounds every read of one legacy document.
///
/// The limits are **designed** bounds, not measured original values. Each one
/// exists so an untrusted file is refused at a checkable point: a size, a
/// record count, a text field and an opaque field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegacyLimits {
    /// Largest accepted document, in bytes.
    pub max_bytes: u64,
    /// Largest accepted record count.
    pub max_records: u32,
    /// Largest accepted declared `Text` field extent, in bytes.
    pub max_text_bytes: usize,
    /// Largest accepted declared `Bytes` field extent, in bytes.
    pub max_bytes_field_bytes: usize,
}

impl LegacyLimits {
    /// The designed defaults.
    ///
    /// `max_bytes` is [`crate::legacy_profile::MAX_LEGACY_SOURCE_BYTES`], so
    /// the document bound and the proposal bound agree. The record count is
    /// well above any plausible legacy record set and far below anything a
    /// hostile file would need to make the reader reserve a large table.
    #[must_use]
    pub const fn designed() -> Self {
        Self {
            max_bytes: crate::legacy_profile::MAX_LEGACY_SOURCE_BYTES,
            max_records: 4_096,
            max_text_bytes: 1_024,
            max_bytes_field_bytes: 4_096,
        }
    }
}

impl Default for LegacyLimits {
    fn default() -> Self {
        Self::designed()
    }
}

/// Machine-matchable class of a reader refusal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LegacyProfileErrorKind {
    /// The document is larger than the limit.
    DocumentTooLarge,
    /// The leading bytes are not the declared magic.
    MagicMismatch,
    /// The document's version major is not the supported one.
    UnsupportedVersion,
    /// The document declares more records than the limit allows.
    TooManyRecords,
    /// The record table does not fit the document.
    RecordTableOutOfRange,
    /// A declared `Text` slot is not valid UTF-8.
    TextNotUtf8,
    /// A declared field is wider than the limit allows.
    FieldTooWide,
    /// Bytes past the record table exist and the layout rejects them.
    TrailingBytes,
    /// The layout declaration itself is invalid.
    Layout,
    /// A bounded read through the F03 reader failed.
    Read(ParseErrorKind),
}

impl LegacyProfileErrorKind {
    /// Stable lowercase identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DocumentTooLarge => "document_too_large",
            Self::MagicMismatch => "magic_mismatch",
            Self::UnsupportedVersion => "unsupported_version",
            Self::TooManyRecords => "too_many_records",
            Self::RecordTableOutOfRange => "record_table_out_of_range",
            Self::TextNotUtf8 => "text_not_utf8",
            Self::FieldTooWide => "field_too_wide",
            Self::TrailingBytes => "trailing_bytes",
            Self::Layout => "layout_invalid",
            Self::Read(_) => "read_failed",
        }
    }
}

/// A reader refusal, with provenance and no payload.
///
/// The `expected`/`observed` pair carries counts, offsets and labels only, the
/// same rule `ParseError` follows: a diagnostic can be attached to a migration
/// report without copying bytes out of a user's private file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyProfileError {
    /// Absolute offset the refusal is about.
    pub offset: u64,
    /// The field the refusal is about.
    pub field: String,
    /// The machine-matchable class.
    pub kind: LegacyProfileErrorKind,
    /// What the reader required.
    pub expected: String,
    /// What the reader saw.
    pub observed: String,
    /// The layout refusal, when `kind` is [`LegacyProfileErrorKind::Layout`].
    pub layout: Option<LegacyLayoutError>,
}

impl LegacyProfileError {
    fn new(
        offset: u64,
        field: impl Into<String>,
        kind: LegacyProfileErrorKind,
        expected: impl Into<String>,
        observed: impl Into<String>,
    ) -> Self {
        Self {
            offset,
            field: field.into(),
            kind,
            expected: expected.into(),
            observed: observed.into(),
            layout: None,
        }
    }

    fn layout_error(offset: u64, field: impl Into<String>, error: LegacyLayoutError) -> Self {
        let mut refused = Self::new(
            offset,
            field,
            LegacyProfileErrorKind::Layout,
            "a valid layout declaration",
            error.to_string(),
        );
        refused.layout = Some(error);
        refused
    }
}

impl fmt::Display for LegacyProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at offset {} (field {}): expected {}, observed {}",
            self.kind.as_str(),
            self.offset,
            self.field,
            self.expected,
            self.observed
        )
    }
}

impl std::error::Error for LegacyProfileError {}

impl From<ParseError> for LegacyProfileError {
    fn from(error: ParseError) -> Self {
        Self {
            offset: error.offset,
            field: error.field,
            kind: LegacyProfileErrorKind::Read(error.kind),
            expected: error.expected,
            observed: error.observed,
            layout: None,
        }
    }
}

/// One declared field's value, with the offset it was read from.
///
/// The reader attaches no meaning to a value. A `U32` may be an id, a count
/// or a price; deciding which is the content layer's job and only through a
/// measured id table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyField {
    /// The declared field name.
    pub name: String,
    /// The value as declared.
    pub value: LegacyValue,
    /// Absolute offset of the field inside the document.
    pub offset: u64,
}

/// One declared field value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LegacyValue {
    /// A little-endian `u8`.
    U8(u8),
    /// A little-endian `u16`.
    U16(u16),
    /// A little-endian `u32`.
    U32(u32),
    /// A little-endian `u64`.
    U64(u64),
    /// A declared text field, valid UTF-8 by construction.
    Text(String),
    /// Opaque declared bytes, kept verbatim.
    Bytes(Vec<u8>),
}

impl LegacyValue {
    /// The value as a `u64` when it is a declared integer, else `None`.
    ///
    /// This is a widening of a declared integer, not an interpretation: a
    /// `Text` or `Bytes` field has no integer reading and returns `None`
    /// rather than a parse of its bytes.
    #[must_use]
    pub const fn as_u64(&self) -> Option<u64> {
        match self {
            Self::U8(value) => Some(*value as u64),
            Self::U16(value) => Some(*value as u64),
            Self::U32(value) => Some(*value as u64),
            Self::U64(value) => Some(*value),
            Self::Text(_) | Self::Bytes(_) => None,
        }
    }
}

/// One record of the record table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyRecord {
    /// The record's index in the table.
    pub index: u32,
    /// Absolute offset of the record inside the document.
    pub offset: u64,
    /// The declared fields, in declaration order.
    pub fields: Vec<LegacyField>,
    /// Record bytes the layout declares no field for.
    ///
    /// Retained verbatim. This is where a field the measurement did not
    /// account for survives, and it is what makes such a record
    /// *unresolved* rather than silently short (spec F64 non-negotiable 2).
    pub undeclared: Vec<u8>,
}

impl LegacyRecord {
    /// The declared field named `name`.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&LegacyValue> {
        self.fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| &field.value)
    }

    /// The declared integer field named `name`.
    #[must_use]
    pub fn integer(&self, name: &str) -> Option<u64> {
        self.field(name).and_then(LegacyValue::as_u64)
    }
}

/// A read legacy document: declared fields, records and retained remainder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyProfileDocument {
    layout_id: String,
    layout_evidence: ClaimStatus,
    version_major: u32,
    version_minor: u32,
    header: Vec<LegacyField>,
    records: Vec<LegacyRecord>,
    trailing: Vec<u8>,
}

impl LegacyProfileDocument {
    /// The layout the document was read through.
    pub fn layout_id(&self) -> &str {
        &self.layout_id
    }

    /// The evidence state of that layout.
    pub fn layout_evidence(&self) -> ClaimStatus {
        self.layout_evidence
    }

    /// The document's declared version major.
    pub fn version_major(&self) -> u32 {
        self.version_major
    }

    /// The document's declared version minor.
    pub fn version_minor(&self) -> u32 {
        self.version_minor
    }

    /// The declared header fields.
    pub fn header(&self) -> &[LegacyField] {
        &self.header
    }

    /// The declared header field named `name`.
    #[must_use]
    pub fn header_value(&self, name: &str) -> Option<&LegacyValue> {
        self.header
            .iter()
            .find(|field| field.name == name)
            .map(|field| &field.value)
    }

    /// The record table.
    pub fn records(&self) -> &[LegacyRecord] {
        &self.records
    }

    /// The record at `index`.
    #[must_use]
    pub fn record(&self, index: u32) -> Option<&LegacyRecord> {
        self.records.iter().find(|record| record.index == index)
    }

    /// Bytes past the record table, kept rather than dropped.
    pub fn trailing(&self) -> &[u8] {
        &self.trailing
    }
}

/// Checks every declared field extent against the limits.
///
/// This is the first thing [`read_legacy_profile`] does after validating the
/// declaration, so a layout that declares a field wider than the limits is
/// refused before a single byte of the document is interpreted.
pub fn check_slot_widths(
    layout: &LegacyLayout,
    limits: &LegacyLimits,
) -> Result<(), LegacyProfileError> {
    for slot in layout.header_slots().iter().chain(layout.record_slots()) {
        let too_wide = match slot.ty() {
            LegacySlotType::Text { len } => len > limits.max_text_bytes,
            LegacySlotType::Bytes { len } => len > limits.max_bytes_field_bytes,
            _ => false,
        };
        if too_wide {
            return Err(LegacyProfileError::new(
                slot.offset() as u64,
                slot.name(),
                LegacyProfileErrorKind::FieldTooWide,
                format!(
                    "a text field of at most {} bytes and a byte field of at most {} bytes",
                    limits.max_text_bytes, limits.max_bytes_field_bytes
                ),
                format!("{} bytes", slot.ty().extent()),
            ));
        }
    }
    Ok(())
}

/// Reads a legacy document through a declared layout, within declared limits.
///
/// The entry point named in the evidence ledger. It is a pure function of its
/// arguments: it opens no file, writes nothing and keeps no state, which is
/// what lets the import layer hold a read-only borrow of the source bytes and
/// still be unable to touch the source.
///
/// # Errors
///
/// [`LegacyProfileError`] when the layout is invalid, a declared field is over
/// its limit, the document is over [`LegacyLimits::max_bytes`], the magic does
/// not match, the version major is not the supported one, the declared record
/// count is over [`LegacyLimits::max_records`], the record table does not fit,
/// a `Text` slot is not valid UTF-8, or the layout rejects trailing bytes.
pub fn read_legacy_profile(
    bytes: &[u8],
    layout: &LegacyLayout,
    limits: &LegacyLimits,
) -> Result<LegacyProfileDocument, LegacyProfileError> {
    layout
        .validate()
        .map_err(|error| LegacyProfileError::layout_error(0, layout.id(), error))?;
    check_slot_widths(layout, limits)?;

    let size = bytes.len() as u64;
    if size > limits.max_bytes {
        return Err(LegacyProfileError::new(
            0,
            "document",
            LegacyProfileErrorKind::DocumentTooLarge,
            format!("at most {} bytes", limits.max_bytes),
            format!("{size} bytes"),
        ));
    }

    let reader = Reader::new(layout.id(), bytes);
    let magic = reader
        .window_bytes(0, LEGACY_MAGIC_BYTES as u64, "magic")
        .map_err(LegacyProfileError::from)?;
    if magic != layout.magic() {
        // The observed bytes are deliberately not reported: a mismatch is a
        // fact about the file, not something to copy into a diagnostic.
        return Err(LegacyProfileError::new(
            0,
            "magic",
            LegacyProfileErrorKind::MagicMismatch,
            "the layout's declared magic",
            "a different leading 8 bytes",
        ));
    }

    let header_bytes = layout.header_bytes() as u64;
    let mut header = Vec::with_capacity(layout.header_slots().len());
    let mut declared: BTreeMap<&str, u64> = BTreeMap::new();
    for slot in layout.header_slots() {
        let value = read_slot(&reader, slot, 0)?;
        if let Some(raw) = value.as_u64() {
            declared.insert(slot.name(), raw);
        }
        header.push(LegacyField {
            name: slot.name().to_owned(),
            value,
            offset: slot.offset() as u64,
        });
    }

    if header_bytes > size {
        return Err(LegacyProfileError::new(
            size,
            "header",
            LegacyProfileErrorKind::RecordTableOutOfRange,
            format!("{header_bytes} header bytes"),
            format!("{size} bytes"),
        ));
    }

    let declared_integer = |field: &str, missing: LegacyLayoutError| {
        declared.get(field).copied().ok_or_else(|| {
            LegacyProfileError::layout_error(header_bytes, field.to_owned(), missing)
        })
    };
    let version_major = declared_integer(
        &layout.version_major_field,
        LegacyLayoutError::VersionFieldNotDeclared {
            field: layout.version_major_field.clone(),
        },
    )? as u32;
    let version_minor = declared_integer(
        &layout.version_minor_field,
        LegacyLayoutError::VersionFieldNotDeclared {
            field: layout.version_minor_field.clone(),
        },
    )? as u32;
    if version_major != layout.supported_version_major() {
        return Err(LegacyProfileError::new(
            header_bytes,
            layout.version_major_field.clone(),
            LegacyProfileErrorKind::UnsupportedVersion,
            format!("version major {}", layout.supported_version_major()),
            format!("version major {version_major}"),
        ));
    }

    let record_count = declared_integer(
        &layout.record_count_field,
        LegacyLayoutError::RecordCountNotDeclared {
            field: layout.record_count_field.clone(),
        },
    )?;
    if record_count > u64::from(limits.max_records) {
        return Err(LegacyProfileError::new(
            header_bytes,
            layout.record_count_field.clone(),
            LegacyProfileErrorKind::TooManyRecords,
            format!("at most {} records", limits.max_records),
            format!("{record_count} records"),
        ));
    }

    // The record table's extent is booked against an independent F03 budget
    // and checked against the document before a single record is reserved.
    let declared_record_bytes = layout.record_size() as u64;
    let mut budget = AllocationBudget::new(layout.id(), limits.max_bytes);
    let table_end = budget.reserve_extent(
        "record_table",
        header_bytes,
        record_count.saturating_mul(declared_record_bytes),
    )?;
    if table_end > size {
        return Err(LegacyProfileError::new(
            size,
            "record_table",
            LegacyProfileErrorKind::RecordTableOutOfRange,
            format!("{table_end} bytes"),
            format!("{size} bytes"),
        ));
    }

    let capacity = usize::try_from(record_count).map_err(|_| {
        LegacyProfileError::new(
            header_bytes,
            layout.record_count_field.clone(),
            LegacyProfileErrorKind::TooManyRecords,
            "a record count that fits in memory",
            format!("{record_count} records"),
        )
    })?;
    let mut records = Vec::with_capacity(capacity);
    let declared_end = layout.record_bytes() as u64;
    for index in 0..record_count {
        let base = header_bytes + index * declared_record_bytes;
        let mut fields = Vec::with_capacity(layout.record_slots().len());
        for slot in layout.record_slots() {
            fields.push(LegacyField {
                name: slot.name().to_owned(),
                value: read_slot(&reader, slot, base)?,
                offset: base + slot.offset() as u64,
            });
        }
        let undeclared = if declared_record_bytes > declared_end {
            reader
                .window_bytes(
                    base + declared_end,
                    declared_record_bytes - declared_end,
                    "record.undeclared",
                )
                .map_err(LegacyProfileError::from)?
                .to_vec()
        } else {
            Vec::new()
        };
        records.push(LegacyRecord {
            index: u32::try_from(index).unwrap_or(u32::MAX),
            offset: base,
            fields,
            undeclared,
        });
    }

    let trailing = bytes
        .get(table_end as usize..)
        .map(<[u8]>::to_vec)
        .unwrap_or_default();
    if !trailing.is_empty() && layout.trailing_policy() == TrailingPolicy::Reject {
        return Err(LegacyProfileError::new(
            table_end,
            "trailing",
            LegacyProfileErrorKind::TrailingBytes,
            "no bytes past the record table",
            format!("{} bytes", trailing.len()),
        ));
    }

    Ok(LegacyProfileDocument {
        layout_id: layout.id().to_owned(),
        layout_evidence: layout.evidence(),
        version_major,
        version_minor,
        header,
        records,
        trailing,
    })
}

/// Reads one declared slot at `base + slot.offset()`.
fn read_slot(
    reader: &Reader<'_>,
    slot: &LegacySlot,
    base: u64,
) -> Result<LegacyValue, LegacyProfileError> {
    let extent = slot.ty().extent();
    let at = base + slot.offset() as u64;
    let raw = reader
        .window_bytes(at, extent as u64, slot.name())
        .map_err(LegacyProfileError::from)?;
    Ok(match slot.ty() {
        LegacySlotType::U8 => LegacyValue::U8(raw[0]),
        LegacySlotType::U16 => LegacyValue::U16(u16::from_le_bytes([raw[0], raw[1]])),
        LegacySlotType::U32 => {
            LegacyValue::U32(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
        }
        LegacySlotType::U64 => LegacyValue::U64(u64::from_le_bytes([
            raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7],
        ])),
        LegacySlotType::Text { .. } => {
            let text = std::str::from_utf8(raw).map_err(|error| {
                LegacyProfileError::new(
                    at,
                    slot.name(),
                    LegacyProfileErrorKind::TextNotUtf8,
                    "valid UTF-8",
                    format!("invalid at byte {}", error.valid_up_to()),
                )
            })?;
            LegacyValue::Text(text.to_owned())
        }
        LegacySlotType::Bytes { .. } => LegacyValue::Bytes(raw.to_vec()),
    })
}

/// The designed fixture layout: a synthetic profile the acceptance tests read.
///
/// **Not a claim about any original file.** Its magic, field names, field
/// widths and version are chosen to exercise the reader — a counted record
/// table, declared id slots, a text field and a retained tail — and its
/// evidence is [`ClaimStatus::Designed`], which the import layer's strict
/// policy refuses. F64-B replaces it with a measured layout; until then no
/// import of original data is possible, which is the correct state.
#[must_use]
pub fn synthetic_layout() -> LegacyLayout {
    let header = vec![
        LegacySlot::new("version_major", LegacySlotType::U32, LEGACY_MAGIC_BYTES),
        LegacySlot::new("version_minor", LegacySlotType::U32, 12),
        LegacySlot::new("record_count", LegacySlotType::U32, 16),
        LegacySlot::new("label", LegacySlotType::Text { len: 8 }, 20),
    ];
    LegacyLayout::new(
        "synthetic.fixture_profile/v1",
        ClaimStatus::Designed,
        *b"CSPROF01",
        header,
        "version_major",
        "version_minor",
        1,
        "record_count",
        vec![
            LegacySlot::new("airframe_id", LegacySlotType::U32, 0),
            LegacySlot::new("weapon_id", LegacySlotType::U32, 4),
            LegacySlot::new("name", LegacySlotType::Text { len: 8 }, 8),
        ],
        vec![
            LegacyIdSlot::new("airframe_id", LegacyIdClass::Airframe),
            LegacyIdSlot::new("weapon_id", LegacyIdClass::Weapon),
        ],
        TrailingPolicy::Retain,
    )
}
