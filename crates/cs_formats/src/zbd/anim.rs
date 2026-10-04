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
//!   uses. A path may repeat (`climbladder.zan` is 14 consecutive rows in
//!   M01's carrier), so rows stay separate and are addressed by index
//!   (spec F06 non-negotiable #3). Measured: **the first member row of all 61
//!   containers is the scope's own paired document** — `..\data\<group>\
//!   <mission>\zrdr\mis_anim.zrd` in a mission scope, and the camera
//!   container's own `cam_anim.zrd` spelled `..\\data\\<group>\\zrdr\
//!   cam_anim.zrd` (every separator but the last doubled) in the 8 camera
//!   carriers — and no paired document names its own row, so member 0 is
//!   unreferenced in every one.
//! * `stamp` holds a build timestamp, not a pointer. Measured separately for
//!   the two tables, because they differ: the 2595 member rows carry 39
//!   distinct values, all inside 2000-08-26 08:00:56..08:06:58 UTC, the window
//!   a single build session would produce, and every container's member rows
//!   carry more than one of them; the 122 external rows carry 9 further
//!   values, all **later** (08:11:17..08:48:20 UTC), one per world group plus
//!   a single shared `zbd\planes.zbd` stamp repeated in all 61 containers
//!   (48 distinct stamps over all rows). **What the original engine does
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
//! ([`AnimationPayload::first_record_name`]). Fewer than
//! [`ANIM_FIRST_RECORD_NAME_OFFSET`] + 32 bytes after the index is a named
//! refusal ([`AnimationIndexError::FirstRecordNameTruncated`]) rather than an
//! empty name.
//!
//! # The records (task #650)
//!
//! [`AnimationPayload::records`] walks the animation records: the declared
//! count says how many there are, and **each record's length is derived from
//! fields inside it** — a 272-byte fixed part, `count x entry-size` tables, and
//! 64-byte sequence blocks that state their own event length (see
//! [`AnimationRecord`] for the arithmetic). Record *n* starts where record
//! *n - 1* ended, record 0 at payload `+108`. The recurrence reproduces all
//! 15 024 records of the 61 retail containers and never reads past a payload;
//! 30 containers end exactly at their last record, the other 31 carry 29 690 ..
//! 1 361 762 bytes after it ([`AnimationRecords::trailing`]), which are **not
//! walked**. What is *inside* a record beyond its names, counts and tables is
//! not decoded — see [`RECORDS_NOT_DECODED_REASON`] and
//! `docs/findings/2026-10-05-m01-lc-anim-records.md`.
//!
//! The record-local pointer words are **not** used: all are values between
//! `0x01fadcf8` and `0x04f7fe60`, beyond the largest 2 MB container, so none
//! can be an offset, and the walk needs none ([`POINTERS_UNRESOLVED_REASON`]).
//!
//! # Fail-closed
//!
//! A family that is not the animation family is refused before a byte is read
//! ([`AnimationIndexError::NotAnimationFamily`]), counts whose rows do not fit
//! in the file are refused with the arithmetic that failed
//! ([`AnimationIndexError::IndexOutOfBounds`]), a payload too short for the
//! fixed header or for the first record's name field is refused with what it
//! had and what it needed, and a path field with no NUL or with a non-ASCII
//! byte is recorded per row as an [`AnimationRowAnomaly`] rather than being
//! decoded into a `String`. A non-zero byte after a path's terminating NUL is
//! also an anomaly, and it is a *measured* one: 1115 of the 2595 retail member
//! rows carry one. Those bytes are kept verbatim in [`AnimationRow::padding`]
//! and never interpreted.

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

/// Measured zero bytes between the payload header and the first record.
pub const ANIM_RECORD_GAP_BYTES: u64 = 40;

/// Bytes of the first record's NUL-padded name field (`reserved_anim_0`).
pub const ANIM_FIRST_RECORD_NAME_BYTES: u64 = 32;

/// Offset of the first record's name field, counted from the payload start:
/// the fixed header, the measured gap, then the name.
pub const ANIM_FIRST_RECORD_NAME_OFFSET: u64 = ANIM_PAYLOAD_HEADER_BYTES + ANIM_RECORD_GAP_BYTES;

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

/// Bytes of an animation record's fixed part.
///
/// Measured: record 0 (`reserved_anim_0`) is exactly this long, and so is every
/// retail record that carries no table, no sequence and no reset state.
pub const ANIM_RECORD_FIXED_BYTES: u64 = 272;

/// Offset of record 0 inside the payload: the 68-byte header, 40 measured zero
/// bytes, and then record 0's own first field (its 32-byte `anim_name`).
pub const ANIM_RECORD_AREA_OFFSET: u64 = ANIM_FIRST_RECORD_NAME_OFFSET;

/// Bytes of a sequence, reset or damage info block that precedes its events.
pub const ANIM_SEQUENCE_INFO_BYTES: u64 = 64;

/// What is still not decoded after the record walk.
///
/// The walk itself is derived and checked (see [`AnimationPayload::records`]);
/// what stays open is the content *inside* it.
pub const RECORDS_NOT_DECODED_REASON: &str = "the record walk is measured, but what lies inside a \
     record is not decoded: the event streams of the reset, damage and ordinary sequences are \
     kept as raw bytes (no event layout is measured for this family), the record-local pointer \
     words are 0x03/0x04-prefixed values far outside every container's length and no rule maps \
     one back to a record or a member row, the meaning of the small id words inside table \
     entries is unmeasured, the table entries of the unknowns, lights, puffers, sounds and \
     prerequisites are read as raw fixed-size entries, and the region after the last record \
     (30 of 61 retail carriers have none, the other 31 carry 29 690 .. 1 361 762 bytes) is not walked";

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
    container: &'a str,
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

    /// What inside the records is still not decoded.
    pub const fn records_not_decoded_reason(&self) -> &'static str {
        RECORDS_NOT_DECODED_REASON
    }

    /// Walks the animation records: record *n* by index, and the region after
    /// the last one.
    ///
    /// The header's declared record count (`+10`) says how many records there
    /// are, and each record's own length is derived from fields inside it, so
    /// the walk reads no byte it was not told to read. See [`AnimationRecord`]
    /// for the derivation.
    ///
    /// # Errors
    ///
    /// [`AnimationRecordError`], naming the record and what it lacked, when a
    /// record does not fit in the payload or declares a table this reader has
    /// no measured size for.
    pub fn records(&self) -> Result<AnimationRecords<'a>, AnimationRecordError> {
        walk_records(
            self.container,
            self.bytes,
            self.header.declared_record_count,
        )
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
    /// [`ANIM_PAYLOAD_HEADER_BYTES`] bytes follow the index,
    /// [`AnimationIndexError::FirstRecordNameTruncated`] when the payload
    /// holds the header but stops before
    /// [`ANIM_FIRST_RECORD_NAME_OFFSET`] + [`ANIM_FIRST_RECORD_NAME_BYTES`],
    /// and [`AnimationIndexError::Parse`] for a read failure or an allocation
    /// refusal inside the header.
    pub fn payload(&self) -> Result<AnimationPayload<'a>, AnimationIndexError> {
        // `payload_offset <= bytes.len()` was checked when the index was read.
        let container_bytes: &'a [u8] = self.bytes;
        let payload: &'a [u8] = &container_bytes[self.payload_offset as usize..];
        let header = read_payload_header(payload, self.dispatch.container())?;
        // 68 header bytes + 40 measured zero bytes + the first record's
        // 32-byte name field. The 40 and the 32 are this repository's
        // measurement; the name is read as a raw field because the records
        // are not walked. A payload that stops inside the name field names
        // what it lacked instead of reporting an empty name.
        let needed = ANIM_FIRST_RECORD_NAME_OFFSET + ANIM_FIRST_RECORD_NAME_BYTES;
        if (payload.len() as u64) < needed {
            return Err(AnimationIndexError::FirstRecordNameTruncated {
                container: self.dispatch.container().to_owned(),
                offset: self.payload_offset,
                needed: ANIM_FIRST_RECORD_NAME_BYTES,
                available: (payload.len() as u64).saturating_sub(ANIM_PAYLOAD_HEADER_BYTES),
            });
        }
        let record_table_offset = self.payload_offset + ANIM_FIRST_RECORD_NAME_OFFSET;
        let first_record_name = &payload[ANIM_FIRST_RECORD_NAME_OFFSET as usize
            ..(ANIM_FIRST_RECORD_NAME_OFFSET + ANIM_FIRST_RECORD_NAME_BYTES) as usize];
        let name_len = first_record_name
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(first_record_name.len());
        Ok(AnimationPayload {
            container: self.dispatch.container(),
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
    /// The payload holds the fixed header but stops before the first
    /// record's name field, so no record name can be reported: reported
    /// rather than returned as an empty name.
    FirstRecordNameTruncated {
        /// The container's provenance label.
        container: String,
        /// Offset where the payload starts.
        offset: u64,
        /// Bytes the name field needs.
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
            Self::FirstRecordNameTruncated { .. } => "first_record_name_truncated",
            Self::Parse(_) => "parse",
        }
    }

    /// The container label the failure came from.
    pub fn container(&self) -> &str {
        match self {
            Self::NotAnimationFamily { container, .. }
            | Self::HeaderTooSmall { container, .. }
            | Self::IndexOutOfBounds { container, .. }
            | Self::PayloadTooSmall { container, .. }
            | Self::FirstRecordNameTruncated { container, .. } => container,
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
            Self::FirstRecordNameTruncated {
                container,
                offset,
                needed,
                available,
            } => write!(
                f,
                "{container} at offset {offset}: the payload holds the fixed header but stops \
                 {available} bytes in, short of the first record's {needed}-byte name field"
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

// ---------------------------------------------------------------------------
// The animation records
// ---------------------------------------------------------------------------

/// Why the record-local pointer words are not resolved.
pub const POINTERS_UNRESOLVED_REASON: &str = "record-local pointer words (seq_defs, reset, damage, \
     objects, nodes, ...) are values around 0x03xxxxxx..0x04xxxxxx, i.e. addresses in the original \
     engine's heap rather than offsets into the container; no rule in this tree maps one back to \
     a record or a table, so none is ever used as an offset (the walk below derives every length \
     from counts and sizes instead)";

/// The kinds of fixed-size table that follow a record's fixed part.
///
/// They appear in this order, each one only when its count is nonzero. Entry
/// sizes are measured: they are exactly what lets 15 024 records of 61
/// containers tile their payloads (see [`AnimationRecord`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationRecordTableKind {
    /// 36-byte entries counted by the u32 at record `+36`: a name field and a
    /// word. What they are for is not measured.
    Unknowns,
    /// 92-byte object references (count byte at `+217`).
    Objects,
    /// 44-byte node references (count byte at `+218`).
    Nodes,
    /// 44-byte light references (count byte at `+219`).
    Lights,
    /// 44-byte puffer references (count byte at `+220`).
    Puffers,
    /// 44-byte dynamic-sound references (count byte at `+221`).
    DynamicSounds,
    /// 40-byte static-sound references (count byte at `+222`).
    StaticSounds,
    /// 48-byte activation prerequisites: an 8-byte header and a 40-byte
    /// entry (count byte at `+224`).
    ActivationPrerequisites,
    /// 72-byte animation references: a 64-byte name and two words (count byte
    /// at `+226`).
    AnimationRefs,
    /// 4-byte index words (count byte at `+227`): `0, 1, 2, ...` in the one
    /// record measured by hand, meaning not measured.
    IndexWords,
}

impl AnimationRecordTableKind {
    /// Every kind, in on-disk order.
    pub const ALL: [Self; 10] = [
        Self::Unknowns,
        Self::Objects,
        Self::Nodes,
        Self::Lights,
        Self::Puffers,
        Self::DynamicSounds,
        Self::StaticSounds,
        Self::ActivationPrerequisites,
        Self::AnimationRefs,
        Self::IndexWords,
    ];

    /// Bytes of one entry.
    pub const fn entry_bytes(self) -> usize {
        match self {
            Self::Unknowns => 36,
            Self::Objects => 92,
            Self::Nodes | Self::Lights | Self::Puffers | Self::DynamicSounds => 44,
            Self::StaticSounds => 40,
            Self::ActivationPrerequisites => 48,
            Self::AnimationRefs => 72,
            Self::IndexWords => 4,
        }
    }

    /// Where an entry's name field starts and how wide it is, when the entry
    /// has exactly one at a fixed place. Prerequisites do not: their name sits
    /// at a position that depends on the prerequisite's type word.
    pub const fn name_field(self) -> Option<(usize, usize)> {
        match self {
            Self::Unknowns
            | Self::Objects
            | Self::Lights
            | Self::Puffers
            | Self::DynamicSounds
            | Self::StaticSounds => Some((0, 32)),
            Self::Nodes => Some((4, 32)),
            Self::AnimationRefs => Some((0, 64)),
            Self::ActivationPrerequisites | Self::IndexWords => None,
        }
    }

    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Unknowns => "unknowns",
            Self::Objects => "objects",
            Self::Nodes => "nodes",
            Self::Lights => "lights",
            Self::Puffers => "puffers",
            Self::DynamicSounds => "dynamic_sounds",
            Self::StaticSounds => "static_sounds",
            Self::ActivationPrerequisites => "activation_prerequisites",
            Self::AnimationRefs => "animation_refs",
            Self::IndexWords => "index_words",
        }
    }
}

/// One fixed-size table of a record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimationRecordTable<'a> {
    kind: AnimationRecordTableKind,
    offset: u64,
    count: usize,
    bytes: &'a [u8],
}

impl<'a> AnimationRecordTable<'a> {
    /// Which table this is.
    pub const fn kind(&self) -> AnimationRecordTableKind {
        self.kind
    }

    /// Where the table starts, counted from the start of the payload.
    pub const fn payload_offset(&self) -> u64 {
        self.offset
    }

    /// Number of entries, exactly as the record's count says.
    pub const fn count(&self) -> usize {
        self.count
    }

    /// The table's bytes, entry after entry.
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Entry `index`, verbatim.
    pub fn entry(&self, index: usize) -> Option<&'a [u8]> {
        let size = self.kind.entry_bytes();
        (index < self.count).then(|| &self.bytes[index * size..(index + 1) * size])
    }

    /// The text before the first NUL of entry `index`'s name field, verbatim,
    /// for kinds that have one fixed name field. Bytes after the NUL are
    /// left-over memory (`ode_name`, ...) and are never read as a name.
    pub fn name(&self, index: usize) -> Option<&'a [u8]> {
        let (offset, width) = self.kind.name_field()?;
        let entry = self.entry(index)?;
        Some(nul_prefix(&entry[offset..offset + width]))
    }
}

/// The kind of a sequence info block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationRecordSequenceKind {
    /// The reset state: present when the record's reset pointer (`+208`) is
    /// nonzero, named `RESET_SEQUENCE` in every retail instance.
    Reset,
    /// The damage sequence: present when the record's pointer at `+212` is
    /// nonzero, named `DAMAGE_SEQUENCE` in every retail instance. The pinned
    /// source calls that pointer `unknown_seq_ptr` and asserts it null; this
    /// family does not.
    Damage,
    /// An ordinary sequence (`count` of them, from the byte at `+216`).
    Sequence,
}

/// One sequence info block and its raw event bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimationRecordSequence<'a> {
    kind: AnimationRecordSequenceKind,
    record_offset: u64,
    info: &'a [u8],
    events: &'a [u8],
}

impl<'a> AnimationRecordSequence<'a> {
    /// Reset, damage or ordinary.
    pub const fn kind(&self) -> AnimationRecordSequenceKind {
        self.kind
    }

    /// Where the info block starts, counted from the start of its record.
    pub const fn record_offset(&self) -> u64 {
        self.record_offset
    }

    /// The 64-byte info block, verbatim.
    pub const fn info(&self) -> &'a [u8] {
        self.info
    }

    /// The sequence's name: the text before the first NUL of the first 32
    /// bytes. Empty for some ordinary sequences.
    pub fn name(&self) -> &'a [u8] {
        nul_prefix(&self.info[..32])
    }

    /// The u32 at info `+32`: `0` or `0x303` in all 56 994 retail blocks.
    pub fn flags(&self) -> u32 {
        word_at(self.info, 32)
    }

    /// The pointer word at info `+56`, **unresolved** (see
    /// [`POINTERS_UNRESOLVED_REASON`]).
    pub fn pointer(&self) -> u32 {
        word_at(self.info, 56)
    }

    /// The event stream: `size` bytes (the u32 at info `+60`), **not decoded**.
    pub const fn events(&self) -> &'a [u8] {
        self.events
    }
}

/// The count fields of a record's fixed part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimationRecordCounts {
    /// u32 at `+36`.
    pub unknowns: u32,
    /// Byte at `+216`: ordinary sequences.
    pub sequences: u8,
    /// Byte at `+217`.
    pub objects: u8,
    /// Byte at `+218`.
    pub nodes: u8,
    /// Byte at `+219`.
    pub lights: u8,
    /// Byte at `+220`.
    pub puffers: u8,
    /// Byte at `+221`.
    pub dynamic_sounds: u8,
    /// Byte at `+222`.
    pub static_sounds: u8,
    /// Byte at `+223`: zero in all 15 024 retail records; a record that sets
    /// it is refused ([`AnimationRecordError::UnmeasuredEffectTable`]).
    pub effects: u8,
    /// Byte at `+224`.
    pub activation_prerequisites: u8,
    /// Byte at `+225`: how many prerequisites must hold. Not a table size.
    pub prerequisites_min_to_satisfy: u8,
    /// Byte at `+226`.
    pub animation_refs: u8,
    /// Byte at `+227`.
    pub index_words: u8,
}

impl AnimationRecordCounts {
    /// The entry count of `kind`.
    pub const fn of(&self, kind: AnimationRecordTableKind) -> u64 {
        match kind {
            AnimationRecordTableKind::Unknowns => self.unknowns as u64,
            AnimationRecordTableKind::Objects => self.objects as u64,
            AnimationRecordTableKind::Nodes => self.nodes as u64,
            AnimationRecordTableKind::Lights => self.lights as u64,
            AnimationRecordTableKind::Puffers => self.puffers as u64,
            AnimationRecordTableKind::DynamicSounds => self.dynamic_sounds as u64,
            AnimationRecordTableKind::StaticSounds => self.static_sounds as u64,
            AnimationRecordTableKind::ActivationPrerequisites => {
                self.activation_prerequisites as u64
            }
            AnimationRecordTableKind::AnimationRefs => self.animation_refs as u64,
            AnimationRecordTableKind::IndexWords => self.index_words as u64,
        }
    }
}

/// The pointer words of a record's fixed part, **all unresolved**
/// ([`POINTERS_UNRESOLVED_REASON`]): raw values, never offsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimationRecordPointers {
    /// `+32`.
    pub unknowns: u32,
    /// `+72`: small ids, equal to `anim_root` (`+108`) in 12 051 of 14 963
    /// retail records.
    pub anim: u32,
    /// `+108`.
    pub anim_root: u32,
    /// `+204`.
    pub seq_defs: u32,
    /// `+208`: nonzero exactly when a reset block follows the tables.
    pub reset_state: u32,
    /// `+212`: nonzero exactly when a damage block follows the tables.
    pub damage_sequence: u32,
    /// `+228`.
    pub objects: u32,
    /// `+232`.
    pub nodes: u32,
    /// `+236`.
    pub lights: u32,
    /// `+240`.
    pub puffers: u32,
    /// `+244`.
    pub dynamic_sounds: u32,
    /// `+248`.
    pub static_sounds: u32,
    /// `+252`.
    pub effects: u32,
    /// `+256`.
    pub activation_prerequisites: u32,
    /// `+260`.
    pub animation_refs: u32,
}

/// One animation record.
///
/// # The derived length
///
/// A record is `272` fixed bytes (`ANIM_RECORD_FIXED_BYTES`) followed by, in
/// this order and each only when present:
///
/// 1. the [`AnimationRecordTableKind`] tables, `count × entry size` bytes each
///    (the unknowns count is the u32 at `+36`, the rest are the bytes
///    `+217..+227`);
/// 2. the reset block when the u32 at `+208` is nonzero: a 64-byte info block
///    whose u32 at `+60` is the byte length of the events that follow it;
/// 3. the damage block when the u32 at `+212` is nonzero, in the same shape;
/// 4. `+216` ordinary sequence blocks, each in that shape.
///
/// So `len = 272 + Σ count × entry + Σ (64 + events)`, and record *n + 1*
/// starts at `start(n) + len(n)`. Record 0 starts at payload `+108`. This
/// recurrence reproduces every record start of all 61 retail containers
/// (15 024 records) and ends each container's record area inside its payload
/// (exactly at its end in 30 of the 61), which is the check; no pointer word
/// takes part in it.
///
/// The fixed-part offsets coincide with the Pirate's Moon `AnimDefC` of the
/// upstream mech3ax project's later source (not the pinned v0.6.0 one, which
/// documents only the MechWarrior 3 layout): this repository **measured**
/// them in the retail files, so they are [`ClaimStatus::ObservedTool`].
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationRecord<'a> {
    index: usize,
    offset: u64,
    bytes: &'a [u8],
    counts: AnimationRecordCounts,
    tables: Vec<AnimationRecordTable<'a>>,
    sequences: Vec<AnimationRecordSequence<'a>>,
}

impl<'a> AnimationRecord<'a> {
    /// Position of the record in the payload's record list.
    pub const fn index(&self) -> usize {
        self.index
    }

    /// Where the record starts, counted from the start of the payload.
    pub const fn payload_offset(&self) -> u64 {
        self.offset
    }

    /// The record's derived length in bytes.
    pub const fn len(&self) -> usize {
        self.bytes.len()
    }

    /// A record is never empty: its fixed part alone is
    /// [`ANIM_RECORD_FIXED_BYTES`] bytes.
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// The record's bytes.
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// The evidence class of every fixed-part field below.
    pub const fn field_evidence(&self) -> ClaimStatus {
        ClaimStatus::ObservedTool
    }

    /// The animation's identity: the text before the first NUL of the first
    /// 32 bytes (`+0`). This is the name `startanims.zrd` and the `.zrd`
    /// `CALL_ANIMATION` events use.
    pub fn anim_name(&self) -> &'a [u8] {
        nul_prefix(&self.bytes[..32])
    }

    /// The object the animation moves: `+40`, 32 bytes.
    pub fn object_name(&self) -> &'a [u8] {
        nul_prefix(&self.bytes[40..72])
    }

    /// The root object of the animation: `+76`, 32 bytes.
    pub fn root_name(&self) -> &'a [u8] {
        nul_prefix(&self.bytes[76..108])
    }

    /// The flag word at `+156`, meaning not measured bit by bit.
    pub fn flags(&self) -> u32 {
        word_at(self.bytes, 156)
    }

    /// The byte at `+160`: `0` in all 14 963 non-zero retail records.
    pub fn status(&self) -> u8 {
        self.bytes[160]
    }

    /// The byte at `+161`: `0`, `2`, `3` or `4` in retail.
    pub fn activation(&self) -> u8 {
        self.bytes[161]
    }

    /// The byte at `+162`: `1`, `4`, `5` or `6` in retail (the pinned source's
    /// MechWarrior records are all `4`).
    pub fn execution_priority(&self) -> u8 {
        self.bytes[162]
    }

    /// The byte at `+163`: `2` in all 14 963 non-zero retail records.
    pub fn two_word(&self) -> u8 {
        self.bytes[163]
    }

    /// The f32 at `+172`, `-1.0` unless the flag word says otherwise.
    pub fn reset_time(&self) -> f32 {
        f32::from_bits(word_at(self.bytes, 172))
    }

    /// The f32 at `+180`.
    pub fn max_health(&self) -> f32 {
        f32::from_bits(word_at(self.bytes, 180))
    }

    /// The count fields.
    pub const fn counts(&self) -> &AnimationRecordCounts {
        &self.counts
    }

    /// The raw pointer words, **unresolved**.
    pub fn pointers(&self) -> AnimationRecordPointers {
        let b = self.bytes;
        AnimationRecordPointers {
            unknowns: word_at(b, 32),
            anim: word_at(b, 72),
            anim_root: word_at(b, 108),
            seq_defs: word_at(b, 204),
            reset_state: word_at(b, 208),
            damage_sequence: word_at(b, 212),
            objects: word_at(b, 228),
            nodes: word_at(b, 232),
            lights: word_at(b, 236),
            puffers: word_at(b, 240),
            dynamic_sounds: word_at(b, 244),
            static_sounds: word_at(b, 248),
            effects: word_at(b, 252),
            activation_prerequisites: word_at(b, 256),
            animation_refs: word_at(b, 260),
        }
    }

    /// Why the pointer words are not resolved.
    pub const fn pointers_unresolved_reason(&self) -> &'static str {
        POINTERS_UNRESOLVED_REASON
    }

    /// The tables that are present (count nonzero), in on-disk order.
    pub fn tables(&self) -> &[AnimationRecordTable<'a>] {
        &self.tables
    }

    /// The table of `kind`, when the record has one.
    pub fn table(&self, kind: AnimationRecordTableKind) -> Option<&AnimationRecordTable<'a>> {
        self.tables.iter().find(|table| table.kind == kind)
    }

    /// The sequence blocks in on-disk order: reset, damage, then ordinary.
    pub fn sequences(&self) -> &[AnimationRecordSequence<'a>] {
        &self.sequences
    }
}

/// The records of a payload, and what follows the last one.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationRecords<'a> {
    records: Vec<AnimationRecord<'a>>,
    trailing_offset: u64,
    trailing: &'a [u8],
}

impl<'a> AnimationRecords<'a> {
    /// Number of records walked: the header's declared count.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the walk produced no record.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Record `index`, or `None` when out of range.
    pub fn get(&self, index: usize) -> Option<&AnimationRecord<'a>> {
        self.records.get(index)
    }

    /// Every record in order.
    pub fn iter(&self) -> std::slice::Iter<'_, AnimationRecord<'a>> {
        self.records.iter()
    }

    /// The records whose `anim_name` equals `name` byte for byte, in order.
    ///
    /// A name may repeat (c1c's camera carrier has 5 repeated names in 307
    /// records), so a caller that needs one record must check the count.
    pub fn by_anim_name<'s>(
        &'s self,
        name: &'s [u8],
    ) -> impl Iterator<Item = &'s AnimationRecord<'a>> + 's {
        self.records
            .iter()
            .filter(move |record| record.anim_name() == name)
    }

    /// Where the region after the last record starts, counted from the start
    /// of the payload.
    pub const fn trailing_offset(&self) -> u64 {
        self.trailing_offset
    }

    /// The bytes after the last record: **not walked**. Empty in 30 of the 61
    /// retail carriers; 29 690 .. 1 361 762 bytes in the other 31.
    pub const fn trailing(&self) -> &'a [u8] {
        self.trailing
    }
}

/// Why the record walk stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnimationRecordError {
    /// A record, or one of its parts, does not fit in the payload.
    Truncated {
        /// The container's provenance label.
        container: String,
        /// The record that did not fit.
        record: usize,
        /// Which part of the record, named.
        part: &'static str,
        /// Where the part starts, counted from the start of the payload.
        payload_offset: u64,
        /// Bytes the part needs.
        needed: u64,
        /// Bytes the payload has from there.
        available: u64,
    },
    /// A record sets the effect-table count, for which no entry size is
    /// measured (it is zero in all 15 024 retail records).
    UnmeasuredEffectTable {
        /// The container's provenance label.
        container: String,
        /// The record.
        record: usize,
        /// The count the record declares.
        count: u8,
    },
}

impl AnimationRecordError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Truncated { .. } => "record_truncated",
            Self::UnmeasuredEffectTable { .. } => "unmeasured_effect_table",
        }
    }
}

impl fmt::Display for AnimationRecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated {
                container,
                record,
                part,
                payload_offset,
                needed,
                available,
            } => write!(
                f,
                "{container}: record {record}'s {part} at payload offset {payload_offset} needs \
                 {needed} bytes, the payload has {available}"
            ),
            Self::UnmeasuredEffectTable {
                container,
                record,
                count,
            } => write!(
                f,
                "{container}: record {record} declares {count} effect entries, and no entry size \
                 is measured for them"
            ),
        }
    }
}

impl std::error::Error for AnimationRecordError {}

fn nul_prefix(field: &[u8]) -> &[u8] {
    let end = field
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(field.len());
    &field[..end]
}

fn word_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes of a word"))
}

fn walk_records<'a>(
    container: &str,
    payload: &'a [u8],
    declared: u16,
) -> Result<AnimationRecords<'a>, AnimationRecordError> {
    let truncated = |record: usize, part: &'static str, at: usize, needed: u64| {
        AnimationRecordError::Truncated {
            container: container.to_owned(),
            record,
            part,
            payload_offset: at as u64,
            needed,
            available: payload.len().saturating_sub(at) as u64,
        }
    };
    // A record is never shorter than its fixed part, so the declared count
    // cannot reserve more than the payload could hold.
    let room = payload.len() / ANIM_RECORD_FIXED_BYTES as usize;
    let mut records = Vec::with_capacity(usize::from(declared).min(room));
    let mut position = ANIM_RECORD_AREA_OFFSET as usize;
    for index in 0..usize::from(declared) {
        let fixed_end = position + ANIM_RECORD_FIXED_BYTES as usize;
        if fixed_end > payload.len() {
            return Err(truncated(
                index,
                "fixed part",
                position,
                ANIM_RECORD_FIXED_BYTES,
            ));
        }
        let fixed = &payload[position..fixed_end];
        let counts = AnimationRecordCounts {
            unknowns: word_at(fixed, 36),
            sequences: fixed[216],
            objects: fixed[217],
            nodes: fixed[218],
            lights: fixed[219],
            puffers: fixed[220],
            dynamic_sounds: fixed[221],
            static_sounds: fixed[222],
            effects: fixed[223],
            activation_prerequisites: fixed[224],
            prerequisites_min_to_satisfy: fixed[225],
            animation_refs: fixed[226],
            index_words: fixed[227],
        };
        if counts.effects != 0 {
            return Err(AnimationRecordError::UnmeasuredEffectTable {
                container: container.to_owned(),
                record: index,
                count: counts.effects,
            });
        }

        let mut cursor = fixed_end;
        let mut tables = Vec::new();
        for kind in AnimationRecordTableKind::ALL {
            let count = counts.of(kind);
            if count == 0 {
                continue;
            }
            // `count` is at most `u32::MAX` and an entry at most 92 bytes.
            let size = count * kind.entry_bytes() as u64;
            let available = (payload.len() - cursor) as u64;
            if size > available {
                return Err(truncated(index, kind.code(), cursor, size));
            }
            let end = cursor + size as usize;
            tables.push(AnimationRecordTable {
                kind,
                offset: cursor as u64,
                count: count as usize,
                bytes: &payload[cursor..end],
            });
            cursor = end;
        }

        let reset = word_at(fixed, 208) != 0;
        let damage = word_at(fixed, 212) != 0;
        let mut sequences = Vec::new();
        let plan = [
            (reset, AnimationRecordSequenceKind::Reset, 1),
            (damage, AnimationRecordSequenceKind::Damage, 1),
            (
                counts.sequences != 0,
                AnimationRecordSequenceKind::Sequence,
                usize::from(counts.sequences),
            ),
        ];
        for (present, kind, count) in plan {
            if !present {
                continue;
            }
            for _ in 0..count {
                let info_end = cursor + ANIM_SEQUENCE_INFO_BYTES as usize;
                if info_end > payload.len() {
                    return Err(truncated(
                        index,
                        "sequence info",
                        cursor,
                        ANIM_SEQUENCE_INFO_BYTES,
                    ));
                }
                let info = &payload[cursor..info_end];
                let size = u64::from(word_at(info, 60));
                if size > (payload.len() - info_end) as u64 {
                    return Err(truncated(index, "sequence events", info_end, size));
                }
                let events_end = info_end + size as usize;
                sequences.push(AnimationRecordSequence {
                    kind,
                    record_offset: (cursor - position) as u64,
                    info,
                    events: &payload[info_end..events_end],
                });
                cursor = events_end;
            }
        }

        records.push(AnimationRecord {
            index,
            offset: position as u64,
            bytes: &payload[position..cursor],
            counts,
            tables,
            sequences,
        });
        position = cursor;
    }
    Ok(AnimationRecords {
        records,
        trailing_offset: position as u64,
        trailing: &payload[position..],
    })
}
