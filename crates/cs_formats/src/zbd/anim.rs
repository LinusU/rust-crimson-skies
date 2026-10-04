//! The animation family's own container index: `zbd/<group>/cam_anim.zbd` and
//! `zbd/<group>/<mission>/mis_anim.zbd`.
//!
//! Spec F06 routes these two archives to the animation reader
//! ([`ZbdFamily::Animation`]) through the documented signature rule in
//! [`super::family`]; task #633 is the stage that reads what is *inside* one.
//! They are **not** reader archives: [`super::trailer`] reads a version-one
//! member table out of the last eight bytes of a sound or reader archive, and
//! none of these files has one. Their index is at the front, in two declared
//! tables, and this module reads it with the same bounded reader the other
//! family readers use.
//!
//! # What the header declares
//!
//! ```text
//! 0x00  u32          signature 0x08170616
//! 0x04  u32          version (53 in every retail container)
//! 0x08  u32          external count      (2 in every retail container)
//! 0x0C  u32          member count        (2 .. 170, 2595 over 61 containers)
//! 0x10  external_count x { u8 path[128]; u32 stamp }   132 bytes each
//!        member_count   x { u8 path[80];  u32 stamp }    84 bytes each
//!        ... the animation payload ...
//! ```
//!
//! * The signature, the version word's offset and the `u32` counts come from
//!   the pinned mech3ax v0.6.0 source (`crates/mech3ax-anim/src/parse.rs`,
//!   commit `d3521a9721be731d365504568ddcd78e3f9846bb`, `docs/research/SOURCES.md`
//!   S02/S06), which reads `signature`, `version`, `count` and then
//!   `AnimNameC { name: Ascii<80>, unknown: u32 }` — the same 84-byte member
//!   row, field order included. That source documents **no** Crimson Skies
//!   version and reads **no** member of this family for Crimson Skies, so
//!   everything below the header is this repository's own measurement
//!   ([`ClaimStatus::ObservedTool`]), recorded in
//!   `docs/findings/2026-10-04-m01-lc-anim-carriers.md`.
//! * The **external** rows (128-byte path) are the two sibling containers the
//!   animation data refers to, in measured form the world's own
//!   `zbd/<group>\gamez.zbd` and the shared `zbd\planes.zbd`, in that order,
//!   in all 61 retail containers. They are listed, never opened.
//! * The **member** rows are the animation-definition sources whose records
//!   the payload carries: `.zrd` records and `.zan` sequences, named by the
//!   same Windows-style relative path the mission's own animation document
//!   uses. A path may repeat (`climbladder.zan` is 12 consecutive rows in
//!   M01's carrier), so rows stay separate and are addressed by index
//!   (spec F06 non-negotiable #3).
//! * `stamp` holds a build timestamp, not a pointer: the 39 distinct values
//!   over the corpus all fall in 2000-08-26 08:00:56..08:06:58 UTC, the window
//!   a single build session would produce. **What the original engine does
//!   with it is not measured** — no source states it, so it is read as a
//!   number and labelled as such.
//!
//! # The payload
//!
//! [`AnimationPayload`] reads the fixed 68-byte block that follows the index
//! and nothing else. Its fields are the ones this repository measured across
//! all 61 retail containers:
//!
//! * `declared_record_count` (u16 at `+10`) is nonzero everywhere and always
//!   at least the member count, which is what the pinned source's own `count`
//!   at the same offset means for its 68-byte anim info block. **That the CS
//!   value counts animation records is an inference**, not a measurement: the
//!   record walk that would confirm it is the open item in the finding, and
//!   no consumer may treat the number as a record count it can index.
//! * `gravity` (f32 at `+36`) is `-9.8` in all 61 containers, the same
//!   constant the pinned source asserts for its own info block
//!   (`GRAVITY` in `parse.rs`).
//! * the u32 at `+40` is `1` in 48 containers and `0` in 13; the u32 at `+60`
//!   is `1` in all 61. Their meaning is not measured and they are read as
//!   numbers.
//!
//! The 68 bytes are followed by 40 zero bytes in all 61 containers and then a
//! 32-byte name field holding a NUL-terminated ASCII string in all 61
//! ([`AnimationPayload::first_record_name`]). Everything from there to the end
//! of the file is the **animation records**, which this stage does **not**
//! decode: the record header layout, the sizes of the inline sub-tables and
//! the meaning of the record-local pointers are all unmeasured, and a guessed
//! walk would produce names no measured rule supports. That gap is the finding,
//! not a silent omission — see [`RECORDS_NOT_DECODED_REASON`].
//!
//! # Fail-closed
//!
//! A family that is not the animation family is refused before a byte is read
//! ([`AnimationIndexError::NotAnimationFamily`]), counts whose rows do not fit
//! in the file are refused with the arithmetic that failed
//! ([`AnimationIndexError::IndexOutOfBounds`]), and a path field with no NUL
//! or with a non-ASCII byte is recorded per row as an
//! [`AnimationRowAnomaly`] rather than being decoded into a `String`. A
//! non-zero byte after a path's terminating NUL is also an anomaly, and it is
//! a *measured* one: 1115 of the 2595 retail member rows carry one. Those
//! bytes are kept verbatim in [`AnimationRow::padding`] and never interpreted.

use std::fmt;

use cs_types::evidence::{ClaimStatus, SourceSpan};

use crate::error::ParseError;
use crate::io::{ParseContext, Reader};

use super::dispatch::{DispatchBasis, ZbdDispatch};
use super::family::ZbdFamily;

/// Error scope stamped onto failures raised while reading an animation index.
pub const ANIM_ENTRYPOINT: &str = "zbd.anim";

/// Offset of the two count words that follow the signature and the version.
pub const ANIM_COUNTS_OFFSET: u64 = 8;

/// Bytes of one **external** row: an 80-byte path field and a u32 stamp.
pub const ANIM_EXTERNAL_ROW_BYTES: u64 = 132;

/// Bytes of an external row's NUL-padded path field.
pub const ANIM_EXTERNAL_PATH_BYTES: usize = 128;

/// Bytes of one **member** row: the 84-byte `AnimNameC` of the pinned source.
pub const ANIM_MEMBER_ROW_BYTES: u64 = 84;

/// Bytes of a member row's NUL-padded path field (`Ascii<80>`).
pub const ANIM_MEMBER_PATH_BYTES: usize = 80;

/// Bytes of the fixed block at the front of the payload.
pub const ANIM_PAYLOAD_HEADER_BYTES: u64 = 68;

/// Offset of the declared record count inside the payload header (u16).
pub const ANIM_RECORD_COUNT_OFFSET: u64 = 10;

/// Offset of the gravity word inside the payload header (f32).
pub const ANIM_GRAVITY_OFFSET: u64 = 36;

/// Offset of the u32 that is `1` in 48 of 61 retail containers and `0` in 13.
pub const ANIM_FLAG_WORD_OFFSET: u64 = 40;

/// Offset of the u32 that is `1` in every retail container.
pub const ANIM_ONE_WORD_OFFSET: u64 = 60;

/// Bytes one parsed [`AnimationRow`] occupies, charged per declared row.
pub const ANIM_ROW_BYTES: u64 = size_of::<AnimationRow<'static>>() as u64;

/// Why the animation records that follow the payload header are not decoded.
pub const RECORDS_NOT_DECODED_REASON: &str = "the animation-record layout behind this header is \
     unmeasured: no source documents it, the record-local pointers this repository can see are far \
     outside every container's length, and the inline sub-table sizes that would fix each record's \
     length have not been derived. The names this stage reports are the payload's first record only, \
     and nothing here indexes a record by position";

/// What the `stamp` word of a row is: a build timestamp by measurement, with
/// no stated meaning.
pub fn stamp_evidence() -> ClaimStatus {
    ClaimStatus::ObservedTool
}

/// Something this reader asserts about a row's own fields that the row breaks.
///
/// Recorded, not fatal: the row stays in the index with its raw field, so a
/// diagnostic can show every such row at once (spec F06 non-negotiable #4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationRowAnomaly {
    /// The path field holds no NUL; the path is then the whole field.
    UnterminatedPath,
    /// A path byte is outside ASCII. The bytes are still kept verbatim.
    NonAsciiPath,
    /// A byte after the path's terminating NUL is not zero — measured in
    /// 1115 of the 2595 retail member rows.
    NonZeroPathPadding,
}

impl AnimationRowAnomaly {
    /// Every anomaly, in reporting order.
    pub const ALL: [Self; 3] = [
        Self::UnterminatedPath,
        Self::NonAsciiPath,
        Self::NonZeroPathPadding,
    ];

    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnterminatedPath => "unterminated_path",
            Self::NonAsciiPath => "non_ascii_path",
            Self::NonZeroPathPadding => "non_zero_path_padding",
        }
    }

    const fn bit(self) -> u8 {
        match self {
            Self::UnterminatedPath => 1,
            Self::NonAsciiPath => 2,
            Self::NonZeroPathPadding => 4,
        }
    }
}

/// One row of an animation container's index: a path field and a stamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimationRow<'a> {
    index: usize,
    external: bool,
    record: SourceSpan,
    path_field: &'a [u8],
    path_len: usize,
    stamp: u32,
    anomalies: u8,
}

impl<'a> AnimationRow<'a> {
    /// Position of the row in its table.
    pub const fn index(&self) -> usize {
        self.index
    }

    /// Whether this row is one of the external-container rows.
    pub const fn is_external(&self) -> bool {
        self.external
    }

    /// Where this row itself sits inside the container.
    pub const fn record_span(&self) -> SourceSpan {
        self.record
    }

    /// The row's path: the bytes before the first NUL (the whole field if
    /// there is none), verbatim and never decoded or normalized.
    pub fn path(&self) -> &'a [u8] {
        &self.path_field[..self.path_len]
    }

    /// The whole path field, padding included.
    pub const fn path_field(&self) -> &'a [u8] {
        self.path_field
    }

    /// The bytes after the path's terminating NUL, kept verbatim, or nothing
    /// when the field holds no NUL.
    ///
    /// Their meaning is not measured. A non-zero byte here is the measured
    /// [`AnimationRowAnomaly::NonZeroPathPadding`], not a second name this
    /// reader is entitled to decode.
    pub fn padding(&self) -> &'a [u8] {
        if self.path_len < self.path_field.len() {
            &self.path_field[self.path_len + 1..]
        } else {
            &[]
        }
    }

    /// The row's stamp word: a build timestamp by measurement.
    pub const fn stamp(&self) -> u32 {
        self.stamp
    }

    /// The evidence class of interpreting `stamp` as a build timestamp.
    pub const fn stamp_evidence(&self) -> ClaimStatus {
        ClaimStatus::ObservedTool
    }

    /// Whether the row breaks `anomaly`.
    pub const fn has_anomaly(&self, anomaly: AnimationRowAnomaly) -> bool {
        self.anomalies & anomaly.bit() != 0
    }

    /// Every anomaly the row carries, in [`AnimationRowAnomaly::ALL`] order.
    pub fn anomalies(&self) -> impl Iterator<Item = AnimationRowAnomaly> + '_ {
        AnimationRowAnomaly::ALL
            .into_iter()
            .filter(|anomaly| self.has_anomaly(*anomaly))
    }

    /// Whether the row satisfies everything this reader asserts about its own
    /// fields.
    pub const fn is_conforming(&self) -> bool {
        self.anomalies == 0
    }
}

/// The fixed 68-byte block at the front of an animation payload.
///
/// Field offsets are this repository's measurement; see the module docs for
/// what each one's evidence class is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimationPayloadHeader {
    /// u32 at `+0`: zero in all 61 retail containers.
    pub leading_zero: u32,
    /// u32 at `+4`: zero in all 61 retail containers.
    pub second_zero: u32,
    /// u16 at `+8`: zero in all 61 retail containers.
    pub third_zero: u16,
    /// u16 at `+10`: the container's declared animation-record count.
    ///
    /// Nonzero in all 61 retail containers and never below the member count.
    /// That it counts *records* is an inference from the pinned source's own
    /// `count` at the same offset — see [`AnimationIndex`]'s payload section.
    pub declared_record_count: u16,
    /// u16 at `+12`: measured, meaning not measured.
    pub word_12: u16,
    /// u16 at `+14`: measured, meaning not measured.
    pub word_14: u16,
    /// u32 at `+16`: measured, meaning not measured; zero in 30 of 61.
    pub word_16: u32,
    /// u32 at `+20`: measured, meaning not measured; zero in the same 30.
    pub word_20: u32,
    /// u32 at `+24`: zero in all 61 retail containers.
    pub word_24: u32,
    /// u32 at `+28`: zero in all 61 retail containers.
    pub word_28: u32,
    /// u16 at `+32`: measured, meaning not measured.
    pub word_32: u16,
    /// u16 at `+34`: measured, meaning not measured.
    pub word_34: u16,
    /// f32 at `+36`: `-9.8` in all 61 retail containers — the gravity the
    /// pinned source asserts for its own info block.
    pub gravity: f32,
    /// u32 at `+40`: `1` in 48 of 61 retail containers and `0` in 13.
    pub flag_word: u32,
    /// u32 at `+44`: zero in all 61 retail containers.
    pub word_44: u32,
    /// u32 at `+48`: zero in all 61 retail containers.
    pub word_48: u32,
    /// u32 at `+52`: zero in all 61 retail containers.
    pub word_52: u32,
    /// u32 at `+56`: zero in all 61 retail containers.
    pub word_56: u32,
    /// u32 at `+60`: `1` in all 61 retail containers.
    pub one_word: u32,
    /// u32 at `+64`: zero in all 61 retail containers.
    pub word_64: u32,
}

impl AnimationPayloadHeader {
    /// The evidence class of the `gravity` field's value: the pinned source
    /// states the same constant for its own info block ([`ClaimStatus::Documented`]).
    pub const fn gravity_evidence(&self) -> ClaimStatus {
        ClaimStatus::Documented
    }

    /// The evidence class of the `declared_record_count`'s *meaning*.
    ///
    /// [`ClaimStatus::ObservedTool`] for the value; the reading as a record
    /// count is an inference and is recorded as such rather than upgraded.
    pub const fn record_count_evidence(&self) -> ClaimStatus {
        ClaimStatus::ObservedTool
    }

    /// Whether the header is the shape every retail container stores: the six
    /// zero words and the final `1`.
    ///
    /// A container that breaks it is still read and reported; this only lets a
    /// census state how many of them matched.
    pub const fn is_measured_shape(&self) -> bool {
        self.leading_zero == 0
            && self.second_zero == 0
            && self.third_zero == 0
            && self.word_24 == 0
            && self.word_28 == 0
            && self.word_44 == 0
            && self.word_48 == 0
            && self.word_52 == 0
            && self.word_56 == 0
            && self.one_word == 1
            && self.word_64 == 0
    }
}

/// The animation payload: the fixed header, and the records that follow it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimationPayload<'a> {
    span: SourceSpan,
    header: AnimationPayloadHeader,
    record_table_offset: u64,
    bytes: &'a [u8],
    first_record_name: &'a [u8],
}

impl<'a> AnimationPayload<'a> {
    /// Where the payload sits inside the container.
    pub const fn span(&self) -> SourceSpan {
        self.span
    }

    /// The fixed header block.
    pub const fn header(&self) -> &AnimationPayloadHeader {
        &self.header
    }

    /// The bytes of the payload.
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Where the animation records begin: 40 measured zero bytes after the
    /// header, then the first record's 32-byte name field.
    pub const fn record_table_offset(&self) -> u64 {
        self.record_table_offset
    }

    /// The first record's name field, exactly as stored.
    ///
    /// Measured: a NUL-terminated ASCII string in all 61 retail containers.
    /// **This is the payload's first record only** — the records are not
    /// walked, so nothing may index a record by position off it. See
    /// [`RECORDS_NOT_DECODED_REASON`].
    pub const fn first_record_name(&self) -> &'a [u8] {
        self.first_record_name
    }

    /// Why the records after the first are not decoded.
    pub const fn records_not_decoded_reason(&self) -> &'static str {
        RECORDS_NOT_DECODED_REASON
    }
}

/// An animation container's own index: the two declared tables and the
/// payload that follows them.
///
/// Built only by [`read_animation_index`], so the family behind it always
/// comes from a dispatch that routed the container to
/// [`ZbdFamily::Animation`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnimationIndex<'a> {
    dispatch: ZbdDispatch<'a>,
    bytes: &'a [u8],
    externals: Vec<AnimationRow<'a>>,
    members: Vec<AnimationRow<'a>>,
    payload_offset: u64,
}

impl<'a> AnimationIndex<'a> {
    /// Provenance label of the container.
    pub const fn container(&self) -> &'a str {
        self.dispatch.container()
    }

    /// The family the dispatch routed the container to.
    pub const fn family(&self) -> ZbdFamily {
        self.dispatch.family()
    }

    /// How dispatch identified the family — `header+role` when both keys
    /// agree, `header` alone when the container sits at a position no role rule
    /// declares.
    pub const fn basis(&self) -> DispatchBasis {
        self.dispatch.basis()
    }

    /// The version word the container's own header declares.
    ///
    /// Always the value [`super::family`]'s signature rule validated for this
    /// family; the word itself is read here so a caller can report it without
    /// re-reading the header.
    pub fn version(&self) -> u32 {
        u32::from_le_bytes(
            self.bytes[4..8]
                .try_into()
                .expect("four bytes of the version word"),
        )
    }

    /// Every external-container row, in declared order.
    pub fn externals(&self) -> &[AnimationRow<'a>] {
        &self.externals
    }

    /// Every member row, in declared order.
    ///
    /// A path may repeat — one source file can contribute several record
    /// groups — so a caller addresses a row by [`AnimationRow::index`] and
    /// never by name alone.
    pub fn members(&self) -> &[AnimationRow<'a>] {
        &self.members
    }

    /// Number of declared member rows.
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// Number of declared external rows.
    pub fn external_count(&self) -> usize {
        self.externals.len()
    }

    /// The member row at `index`, or `None` when out of range.
    pub fn member(&self, index: usize) -> Option<&AnimationRow<'a>> {
        self.members.get(index)
    }

    /// The first member row whose path equals `path` byte for byte.
    ///
    /// Paths are compared verbatim: the store keeps Windows separators and
    /// mixed case, and this reader never normalizes one spelling into another
    /// (spec F06 non-negotiable #3).
    pub fn member_by_path(&self, path: &[u8]) -> Option<&AnimationRow<'a>> {
        self.members.iter().find(|row| row.path() == path)
    }

    /// Rows that break something this reader asserts about their own fields.
    pub fn anomalous_rows(&self) -> impl Iterator<Item = &AnimationRow<'a>> {
        self.externals
            .iter()
            .chain(self.members.iter())
            .filter(|row| !row.is_conforming())
    }

    /// Offset of the payload: one past the last index row.
    pub const fn payload_offset(&self) -> u64 {
        self.payload_offset
    }

    /// The span the index occupies.
    pub fn index_span(&self) -> SourceSpan {
        SourceSpan {
            offset: ANIM_COUNTS_OFFSET,
            length: self.payload_offset - ANIM_COUNTS_OFFSET,
        }
    }

    /// The animation payload, read as far as this stage measures it.
    ///
    /// # Errors
    ///
    /// [`AnimationIndexError::PayloadTooSmall`] when fewer than
    /// [`ANIM_PAYLOAD_HEADER_BYTES`] bytes follow the index, and
    /// [`AnimationIndexError::Parse`] for a read failure or an allocation
    /// refusal inside the header.
    pub fn payload(&self) -> Result<AnimationPayload<'a>, AnimationIndexError> {
        // `payload_offset <= bytes.len()` was checked when the index was read.
        let container_bytes: &'a [u8] = self.bytes;
        let payload: &'a [u8] = &container_bytes[self.payload_offset as usize..];
        let header = read_payload_header(payload, self.dispatch.container())?;
        // 68 header bytes + 40 measured zero bytes + the first record's
        // 32-byte name field. The 40 and the 32 are this repository's
        // measurement; the name is read as a raw field because the records
        // are not walked.
        let record_table_offset = self.payload_offset + ANIM_PAYLOAD_HEADER_BYTES + 40;
        let name_at = record_table_offset - self.payload_offset;
        let first_record_name = payload
            .get(name_at as usize..name_at as usize + 32)
            .unwrap_or(&[]);
        let name_len = first_record_name
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(first_record_name.len());
        Ok(AnimationPayload {
            span: SourceSpan {
                offset: self.payload_offset,
                length: payload.len() as u64,
            },
            header,
            record_table_offset,
            bytes: payload,
            first_record_name: &first_record_name[..name_len],
        })
    }
}

/// Why an animation container's index could not be read.
///
/// Carries counts and offsets only, never container bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnimationIndexError {
    /// The container was routed to a family this reader does not index;
    /// reading it here would be a guess.
    NotAnimationFamily {
        /// The container's provenance label.
        container: String,
        /// The family dispatch decided.
        family: ZbdFamily,
    },
    /// The container is too short to hold the header's own counts.
    HeaderTooSmall {
        /// The container's provenance label.
        container: String,
        /// Bytes the header needs.
        needed: u64,
        /// Bytes the container has.
        container_len: u64,
    },
    /// The declared rows do not fit in front of the payload.
    IndexOutOfBounds {
        /// The container's provenance label.
        container: String,
        /// Offset of the count word that failed.
        offset: u64,
        /// The declared count.
        count: u32,
        /// Bytes one row of that table needs.
        row_bytes: u64,
        /// Bytes the rows need in total.
        needed: u64,
        /// Bytes the container has after the header.
        available: u64,
    },
    /// Fewer bytes follow the index than the fixed payload header needs.
    PayloadTooSmall {
        /// The container's provenance label.
        container: String,
        /// Offset where the payload starts.
        offset: u64,
        /// Bytes the header needs.
        needed: u64,
        /// Bytes the container has from there.
        available: u64,
    },
    /// A failure from the checked reader or the allocation budget, scoped as
    /// `zbd.anim.<field>`.
    Parse(ParseError),
}

impl AnimationIndexError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NotAnimationFamily { .. } => "not_animation_family",
            Self::HeaderTooSmall { .. } => "header_too_small",
            Self::IndexOutOfBounds { .. } => "index_out_of_bounds",
            Self::PayloadTooSmall { .. } => "payload_too_small",
            Self::Parse(_) => "parse",
        }
    }

    /// The container label the failure came from.
    pub fn container(&self) -> &str {
        match self {
            Self::NotAnimationFamily { container, .. }
            | Self::HeaderTooSmall { container, .. }
            | Self::IndexOutOfBounds { container, .. }
            | Self::PayloadTooSmall { container, .. } => container,
            Self::Parse(error) => &error.container,
        }
    }
}

impl From<ParseError> for AnimationIndexError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl fmt::Display for AnimationIndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnimationFamily { container, family } => write!(
                f,
                "{container}: the `{}` family is not indexed by an animation container header",
                family.as_str()
            ),
            Self::HeaderTooSmall {
                container,
                needed,
                container_len,
            } => write!(
                f,
                "{container}: the animation header needs {needed} bytes, the container has \
                 {container_len}"
            ),
            Self::IndexOutOfBounds {
                container,
                offset,
                count,
                row_bytes,
                needed,
                available,
            } => write!(
                f,
                "{container} at offset {offset}: {count} rows of {row_bytes} bytes need {needed} \
                 bytes, the container has {available}"
            ),
            Self::PayloadTooSmall {
                container,
                offset,
                needed,
                available,
            } => write!(
                f,
                "{container} at offset {offset}: the payload header needs {needed} bytes, \
                 {available} follow the index"
            ),
            Self::Parse(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for AnimationIndexError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            _ => None,
        }
    }
}

/// Whether `family`'s containers carry this front index (task #633: the
/// animation family, and no other).
pub const fn indexed_by_animation_header(family: ZbdFamily) -> bool {
    matches!(family, ZbdFamily::Animation)
}

/// Reads the animation container index at the front of `bytes`.
///
/// `dispatch` is the decision that routed the container; only the animation
/// family is indexed this way, so any other family is refused before a byte is
/// read. `bytes` is the whole container.
///
/// # Errors
///
/// * [`AnimationIndexError::NotAnimationFamily`] for another family;
/// * [`AnimationIndexError::HeaderTooSmall`] when the two count words do not
///   fit;
/// * [`AnimationIndexError::IndexOutOfBounds`] when the declared rows do not
///   fit after the header;
/// * [`AnimationIndexError::Parse`] (`unexpected_eof`) when a row does not
///   fit, or (`allocation_budget_exceeded`) when the row tables exceed the
///   parse's budget.
pub fn read_animation_index<'a>(
    context: &mut ParseContext,
    dispatch: ZbdDispatch<'a>,
    bytes: &'a [u8],
) -> Result<AnimationIndex<'a>, AnimationIndexError> {
    let family = dispatch.family();
    if !indexed_by_animation_header(family) {
        return Err(AnimationIndexError::NotAnimationFamily {
            container: dispatch.container().to_owned(),
            family,
        });
    }
    let container_len = bytes.len() as u64;
    let header_bytes = ANIM_COUNTS_OFFSET + 8;
    if container_len < header_bytes {
        return Err(AnimationIndexError::HeaderTooSmall {
            container: dispatch.container().to_owned(),
            needed: header_bytes,
            container_len,
        });
    }

    let (external_count, member_count) =
        context.parse(ANIM_ENTRYPOINT, bytes, |reader, _, _| {
            reader.skip("header", ANIM_COUNTS_OFFSET as usize)?;
            Ok((
                reader.read_u32("external_count")?,
                reader.read_u32("member_count")?,
            ))
        })?;

    // Both tables start at `16` and each row's bytes are a `u32` count times a
    // constant, so neither product can overflow `u64`.
    let external_bytes = u64::from(external_count) * ANIM_EXTERNAL_ROW_BYTES;
    let member_bytes = u64::from(member_count) * ANIM_MEMBER_ROW_BYTES;
    let available = container_len - header_bytes;
    let needed = external_bytes
        .checked_add(member_bytes)
        .expect("two u32 counts times a constant cannot overflow u64");
    if needed > available {
        // Name the table whose own rows are what fails, so the diagnostic
        // points at the count that is wrong rather than at the sum.
        let (offset, count, row_bytes) = if external_bytes > available {
            (ANIM_COUNTS_OFFSET, external_count, ANIM_EXTERNAL_ROW_BYTES)
        } else {
            (ANIM_COUNTS_OFFSET + 4, member_count, ANIM_MEMBER_ROW_BYTES)
        };
        return Err(AnimationIndexError::IndexOutOfBounds {
            container: dispatch.container().to_owned(),
            offset,
            count,
            row_bytes,
            needed,
            available,
        });
    }

    let (externals, members) = context.parse(ANIM_ENTRYPOINT, bytes, |reader, allocation, _| {
        let members_total = u64::from(external_count) + u64::from(member_count);
        allocation.reserve("rows", header_bytes, members_total, ANIM_ROW_BYTES)?;
        reader.skip("header", header_bytes as usize)?;

        let mut externals = Vec::with_capacity(external_count as usize);
        for index in 0..external_count as usize {
            externals.push(read_row(reader, index, true, ANIM_EXTERNAL_PATH_BYTES)?);
        }
        let mut members = Vec::with_capacity(member_count as usize);
        for index in 0..member_count as usize {
            members.push(read_row(reader, index, false, ANIM_MEMBER_PATH_BYTES)?);
        }
        Ok((externals, members))
    })?;

    let payload_offset = header_bytes + needed;
    Ok(AnimationIndex {
        dispatch,
        bytes,
        externals,
        members,
        payload_offset,
    })
}

/// Reads one index row at the reader's position.
fn read_row<'a>(
    reader: &mut Reader<'a>,
    index: usize,
    external: bool,
    path_bytes: usize,
) -> Result<AnimationRow<'a>, ParseError> {
    let offset = reader.position();
    let path_field = reader.read_bytes("row.path", path_bytes)?;
    let stamp = reader.read_u32("row.stamp")?;

    let mut anomalies = 0u8;
    let path_len = match path_field.iter().position(|&byte| byte == 0) {
        Some(terminator) => {
            if path_field[terminator..].iter().any(|&byte| byte != 0) {
                anomalies |= AnimationRowAnomaly::NonZeroPathPadding.bit();
            }
            terminator
        }
        None => {
            anomalies |= AnimationRowAnomaly::UnterminatedPath.bit();
            path_bytes
        }
    };
    if !path_field[..path_len].is_ascii() {
        anomalies |= AnimationRowAnomaly::NonAsciiPath.bit();
    }
    Ok(AnimationRow {
        index,
        external,
        record: SourceSpan {
            offset,
            length: if external {
                ANIM_EXTERNAL_ROW_BYTES
            } else {
                ANIM_MEMBER_ROW_BYTES
            },
        },
        path_field,
        path_len,
        stamp,
        anomalies,
    })
}

/// Reads the fixed 68-byte payload header.
fn read_payload_header(
    payload: &[u8],
    container: &str,
) -> Result<AnimationPayloadHeader, AnimationIndexError> {
    let available = payload.len() as u64;
    if available < ANIM_PAYLOAD_HEADER_BYTES {
        return Err(AnimationIndexError::PayloadTooSmall {
            container: container.to_owned(),
            offset: 0,
            needed: ANIM_PAYLOAD_HEADER_BYTES,
            available,
        });
    }
    let mut reader = Reader::new(container, payload);
    let header = reader.read_bytes("header", ANIM_PAYLOAD_HEADER_BYTES as usize)?;
    let word = |at: u64| {
        let at = at as usize;
        u32::from_le_bytes(header[at..at + 4].try_into().expect("four header bytes"))
    };
    let half = |at: u64| {
        let at = at as usize;
        u16::from_le_bytes(header[at..at + 2].try_into().expect("two header bytes"))
    };
    let float = |at: u64| {
        let at = at as usize;
        f32::from_bits(u32::from_le_bytes(
            header[at..at + 4].try_into().expect("four header bytes"),
        ))
    };
    Ok(AnimationPayloadHeader {
        leading_zero: word(0),
        second_zero: word(4),
        third_zero: half(8),
        declared_record_count: half(ANIM_RECORD_COUNT_OFFSET),
        word_12: half(12),
        word_14: half(14),
        word_16: word(16),
        word_20: word(20),
        word_24: word(24),
        word_28: word(28),
        word_32: half(32),
        word_34: half(34),
        gravity: float(ANIM_GRAVITY_OFFSET),
        flag_word: word(ANIM_FLAG_WORD_OFFSET),
        word_44: word(44),
        word_48: word(48),
        word_52: word(52),
        word_56: word(56),
        one_word: word(ANIM_ONE_WORD_OFFSET),
        word_64: word(64),
    })
}
