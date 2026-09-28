//! The version-one member index at the end of a sound or reader archive.
//!
//! Task #340 read the layout from the pinned mech3ax v0.6.0 source
//! (`crates/mech3ax-archive/src/archive.rs`, commit
//! `d3521a9721be731d365504568ddcd78e3f9846bb`, `docs/research/SOURCES.md` S02)
//! and checked it against every retail sound and reader archive
//! (`docs/findings/2026-09-28-t340-zbd-family-headers-and-archive-names.md`):
//!
//! * the last 8 bytes are the trailer: u32 version (`1`) and u32 member count;
//! * immediately before the trailer sit `count` entries of 148 bytes each:
//!   u32 start, u32 length, a 64-byte NUL-padded name and 76 bytes the source
//!   reads as `garbage` without explaining them;
//! * the source requires every member to satisfy
//!   `start < start + length <= table_start`.
//!
//! [`read_version_one_index`] reads that index with the checked reader and
//! turns it into the [`MemberExtent`]s F06-B's [`MemberTable`] takes as input,
//! so the sound and reader readers list what the archive itself declares:
//!
//! ```text
//! let index = read_version_one_index(&mut context, dispatch, &bytes)?;
//! let table = index.member_table();
//! let archive = read_sound_archive(&mut context, &table, index.data())?;
//! ```
//!
//! What it keeps and what it does not claim:
//!
//! * the 76 unexplained bytes of every entry are retained verbatim as
//!   [`UnexplainedBytes`], labelled [`ClaimStatus::Unknown`] — nothing reads a
//!   meaning into them;
//! * names are the bytes before the first NUL, never decoded or normalized,
//!   and duplicate names stay separate entries (spec F06 non-negotiable #3);
//! * what the pinned source *asserts* about an entry (non-empty extent, a
//!   terminated, zero-padded ASCII name) is checked and recorded per entry as
//!   an [`EntryAnomaly`] instead of ending the index, so one odd entry does not
//!   hide its siblings (non-negotiable #4);
//! * a member reaching into the index itself is left to the listing: the
//!   readers are handed [`VersionOneIndex::data`] (the bytes before the
//!   index), so such a member fails its bounds check there as
//!   [`crate::zbd::MemberError::OutOfBounds`].
//!
//! The index runs through [`ParseContext::parse`] (spec F03): the entry and
//! extent tables are booked against the parse's allocation budget before
//! either `Vec` exists, and a refused index leaves the ledger untouched.

use std::fmt;

use cs_types::evidence::{ClaimStatus, SourceSpan};

use crate::error::ParseError;
use crate::io::{ParseContext, Reader};

use super::archive::{MemberExtent, MemberTable};
use super::dispatch::ZbdDispatch;
use super::family::ZbdFamily;

/// Error scope stamped onto failures raised while reading the index.
pub const TRAILER_ENTRYPOINT: &str = "zbd.trailer";

/// The trailer version the pinned source reads for Crimson Skies sound and
/// reader archives (`VERSION_ONE`; `unzbd` maps Crimson Skies to
/// `Version::One`).
pub const TRAILER_VERSION_ONE: u32 = 1;

/// Bytes of the version-one trailer: u32 version, u32 member count.
pub const TRAILER_BYTES: u64 = 8;

/// Bytes of one index entry (`EntryC`, `static_assert_size!(EntryC, 148)`).
pub const INDEX_ENTRY_BYTES: u64 = 148;

/// Bytes of an entry's NUL-padded name field.
pub const INDEX_NAME_BYTES: usize = 64;

/// Bytes after the name the pinned source reads without explaining them.
pub const INDEX_UNEXPLAINED_BYTES: usize = 76;

/// Bytes one parsed [`IndexEntry`] occupies, charged per declared member.
pub const INDEX_ROW_BYTES: u64 = size_of::<IndexEntry<'static>>() as u64;

/// Bytes one derived [`MemberExtent`] occupies, charged per declared member.
pub const MEMBER_EXTENT_BYTES: u64 = size_of::<MemberExtent<'static>>() as u64;

/// Why the 76 trailing bytes of an entry are not interpreted.
pub const UNEXPLAINED_REASON: &str = "the pinned mech3ax v0.6.0 source reads these 76 bytes of every \
     version-one index entry as `garbage` and assigns them no meaning; they are kept verbatim";

/// One stretch of bytes the index carries but no source explains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnexplainedBytes<'a> {
    bytes: &'a [u8],
    span: SourceSpan,
}

impl<'a> UnexplainedBytes<'a> {
    /// The bytes exactly as the archive stores them.
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Where they sit inside the archive.
    pub const fn span(&self) -> SourceSpan {
        self.span
    }

    /// The evidence class of any meaning: `unknown`, by construction.
    pub const fn evidence(&self) -> ClaimStatus {
        ClaimStatus::Unknown
    }

    /// Why the bytes are not interpreted.
    pub const fn reason(&self) -> &'static str {
        UNEXPLAINED_REASON
    }
}

/// Something the pinned source asserts about an entry that this entry breaks.
///
/// Recorded, not fatal: the entry stays in the index with its raw fields, so
/// a diagnostic can show every such entry at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryAnomaly {
    /// `length` is zero, so `start < start + length` does not hold.
    EmptyExtent,
    /// The 64-byte name field holds no NUL; the name is then the whole field.
    UnterminatedName,
    /// A byte after the name's terminating NUL is not zero.
    NonZeroNamePadding,
    /// A name byte is outside ASCII. The name bytes are still kept verbatim.
    NonAsciiName,
}

impl EntryAnomaly {
    /// Every anomaly, in reporting order.
    pub const ALL: [Self; 4] = [
        Self::EmptyExtent,
        Self::UnterminatedName,
        Self::NonZeroNamePadding,
        Self::NonAsciiName,
    ];

    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(self) -> &'static str {
        match self {
            Self::EmptyExtent => "empty_extent",
            Self::UnterminatedName => "unterminated_name",
            Self::NonZeroNamePadding => "non_zero_name_padding",
            Self::NonAsciiName => "non_ascii_name",
        }
    }

    const fn bit(self) -> u8 {
        match self {
            Self::EmptyExtent => 1,
            Self::UnterminatedName => 2,
            Self::NonZeroNamePadding => 4,
            Self::NonAsciiName => 8,
        }
    }
}

/// One 148-byte entry of the version-one index, as declared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexEntry<'a> {
    index: usize,
    record: SourceSpan,
    start: u32,
    length: u32,
    name_field: &'a [u8],
    name_len: usize,
    unexplained: &'a [u8],
    anomalies: u8,
}

impl<'a> IndexEntry<'a> {
    /// Position of the entry in the index.
    pub const fn index(&self) -> usize {
        self.index
    }

    /// Where this 148-byte entry itself sits inside the archive.
    pub const fn record_span(&self) -> SourceSpan {
        self.record
    }

    /// The declared start offset of the member.
    pub const fn start(&self) -> u32 {
        self.start
    }

    /// The declared length of the member.
    pub const fn length(&self) -> u32 {
        self.length
    }

    /// The member's declared extent. Two `u32`s cannot overflow a `u64`, so
    /// this is exact.
    pub const fn span(&self) -> SourceSpan {
        SourceSpan {
            offset: self.start as u64,
            length: self.length as u64,
        }
    }

    /// The member's name: the bytes before the first NUL (the whole field if
    /// there is none), verbatim.
    pub fn name(&self) -> &'a [u8] {
        &self.name_field[..self.name_len]
    }

    /// The whole 64-byte name field, padding included.
    pub const fn name_field(&self) -> &'a [u8] {
        self.name_field
    }

    /// The 76 bytes after the name, kept verbatim and labelled unknown.
    pub const fn unexplained(&self) -> UnexplainedBytes<'a> {
        UnexplainedBytes {
            bytes: self.unexplained,
            span: SourceSpan {
                offset: self.record.offset + 8 + INDEX_NAME_BYTES as u64,
                length: INDEX_UNEXPLAINED_BYTES as u64,
            },
        }
    }

    /// Whether the entry breaks `anomaly`.
    pub const fn has_anomaly(&self, anomaly: EntryAnomaly) -> bool {
        self.anomalies & anomaly.bit() != 0
    }

    /// Every anomaly the entry carries, in [`EntryAnomaly::ALL`] order.
    pub fn anomalies(&self) -> impl Iterator<Item = EntryAnomaly> + '_ {
        EntryAnomaly::ALL
            .into_iter()
            .filter(|anomaly| self.has_anomaly(*anomaly))
    }

    /// Whether the entry satisfies everything the pinned source asserts about
    /// its own fields.
    pub const fn is_conforming(&self) -> bool {
        self.anomalies == 0
    }
}

/// The version-one member index of one sound or reader archive.
///
/// Built only by [`read_version_one_index`], so the family behind it always
/// comes from a dispatch that routed the archive to a family indexed this way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionOneIndex<'a> {
    dispatch: ZbdDispatch<'a>,
    bytes: &'a [u8],
    table_start: u64,
    entries: Vec<IndexEntry<'a>>,
    extents: Vec<MemberExtent<'a>>,
}

impl<'a> VersionOneIndex<'a> {
    /// Provenance label of the archive.
    pub const fn container(&self) -> &'a str {
        self.dispatch.container()
    }

    /// The family the dispatch routed the archive to.
    pub const fn family(&self) -> ZbdFamily {
        self.dispatch.family()
    }

    /// The trailer version: always [`TRAILER_VERSION_ONE`], because any other
    /// version is refused.
    pub const fn version(&self) -> u32 {
        TRAILER_VERSION_ONE
    }

    /// Number of declared members.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the index declares no member.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every entry, in index order.
    pub fn entries(&self) -> &[IndexEntry<'a>] {
        &self.entries
    }

    /// The entry at `index`, or `None` when out of range.
    pub fn entry(&self, index: usize) -> Option<&IndexEntry<'a>> {
        self.entries.get(index)
    }

    /// The declared members as F06-B's container layer takes them. Entries
    /// carry no numeric id, so every extent's id is `None`.
    pub fn extents(&self) -> &[MemberExtent<'a>] {
        &self.extents
    }

    /// Entries that break something the pinned source asserts.
    pub fn anomalous_entries(&self) -> impl Iterator<Item = &IndexEntry<'a>> + '_ {
        self.entries.iter().filter(|entry| !entry.is_conforming())
    }

    /// Offset of the first index entry: where member data must end.
    pub const fn table_start(&self) -> u64 {
        self.table_start
    }

    /// The bytes before the index — the only place a member may live. Hand
    /// these, not the whole archive, to the family reader, so a member that
    /// reaches into the index fails its bounds check.
    pub fn data(&self) -> &'a [u8] {
        // `table_start <= bytes.len()` was checked when the index was read.
        &self.bytes[..self.table_start as usize]
    }

    /// The span the index and trailer occupy: from [`Self::table_start`] to
    /// the end of the archive. These are the bytes this parser consumed.
    pub fn index_span(&self) -> SourceSpan {
        SourceSpan {
            offset: self.table_start,
            length: self.bytes.len() as u64 - self.table_start,
        }
    }

    /// The member index for the family reader, carrying the dispatch that
    /// decided the family.
    pub fn member_table(&self) -> MemberTable<'_> {
        MemberTable::from_dispatch(&self.dispatch, &self.extents)
    }
}

/// Why the index of an archive could not be read.
///
/// Carries counts and offsets only, never archive bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexError {
    /// The archive was routed to a family whose member index is not a
    /// version-one trailer; reading one there would be a guess.
    NotIndexedByTrailer {
        /// The archive's provenance label.
        container: String,
        /// The family dispatch decided.
        family: ZbdFamily,
    },
    /// The trailer names a version the pinned source does not read for
    /// Crimson Skies.
    UnsupportedVersion {
        /// The archive's provenance label.
        container: String,
        /// Offset of the version word.
        offset: u64,
        /// The version the trailer declares.
        version: u32,
    },
    /// `count` entries plus the trailer do not fit in the archive.
    IndexOutOfBounds {
        /// The archive's provenance label.
        container: String,
        /// Offset of the count word.
        offset: u64,
        /// The declared member count.
        count: u32,
        /// Bytes the entries and trailer need.
        needed: u64,
        /// Bytes the archive has.
        container_len: u64,
    },
    /// A failure from the checked reader or the allocation budget, scoped as
    /// `zbd.trailer.<field>`.
    Parse(ParseError),
}

impl IndexError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NotIndexedByTrailer { .. } => "not_indexed_by_trailer",
            Self::UnsupportedVersion { .. } => "unsupported_trailer_version",
            Self::IndexOutOfBounds { .. } => "index_out_of_bounds",
            Self::Parse(_) => "parse",
        }
    }

    /// The archive label the failure came from.
    pub fn container(&self) -> &str {
        match self {
            Self::NotIndexedByTrailer { container, .. }
            | Self::UnsupportedVersion { container, .. }
            | Self::IndexOutOfBounds { container, .. } => container,
            Self::Parse(error) => &error.container,
        }
    }
}

impl From<ParseError> for IndexError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotIndexedByTrailer { container, family } => write!(
                f,
                "{container}: the `{}` family is not indexed by a version-one trailer",
                family.as_str()
            ),
            Self::UnsupportedVersion {
                container,
                offset,
                version,
            } => write!(
                f,
                "{container} at offset {offset}: trailer version {version} is not \
                 {TRAILER_VERSION_ONE}"
            ),
            Self::IndexOutOfBounds {
                container,
                offset,
                count,
                needed,
                container_len,
            } => write!(
                f,
                "{container} at offset {offset}: {count} index entries plus the trailer need \
                 {needed} bytes, the archive has {container_len}"
            ),
            Self::Parse(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for IndexError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            _ => None,
        }
    }
}

/// Whether `family`'s archives carry a version-one trailer index (task #340:
/// sound and reader, and no other family).
pub const fn indexed_by_trailer(family: ZbdFamily) -> bool {
    matches!(family, ZbdFamily::Sound | ZbdFamily::Reader)
}

/// Reads the version-one member index at the end of `bytes`.
///
/// `dispatch` is the decision that routed the archive; only the sound and
/// reader families are indexed this way, so any other family is refused
/// before a byte is read. `bytes` is the whole archive.
///
/// # Errors
///
/// * [`IndexError::NotIndexedByTrailer`] for a family without a trailer index;
/// * [`IndexError::Parse`] (`unexpected_eof`) when the archive is shorter
///   than the trailer, or (`allocation_budget_exceeded`) when the entry and
///   extent tables do not fit the parse's budget;
/// * [`IndexError::UnsupportedVersion`] when the version word is not `1`;
/// * [`IndexError::IndexOutOfBounds`] when the declared entries do not fit
///   in front of the trailer.
pub fn read_version_one_index<'a>(
    context: &mut ParseContext,
    dispatch: ZbdDispatch<'a>,
    bytes: &'a [u8],
) -> Result<VersionOneIndex<'a>, IndexError> {
    let family = dispatch.family();
    if !indexed_by_trailer(family) {
        return Err(IndexError::NotIndexedByTrailer {
            container: dispatch.container().to_owned(),
            family,
        });
    }
    let container_len = bytes.len() as u64;
    let trailer_offset = container_len.saturating_sub(TRAILER_BYTES);

    let (version, count) = context.parse(TRAILER_ENTRYPOINT, bytes, |reader, _, _| {
        // Short archives fail on the version read with `unexpected_eof`.
        reader.skip("data", trailer_offset as usize)?;
        Ok((reader.read_u32("version")?, reader.read_u32("count")?))
    })?;
    if version != TRAILER_VERSION_ONE {
        return Err(IndexError::UnsupportedVersion {
            container: dispatch.container().to_owned(),
            offset: trailer_offset,
            version,
        });
    }
    // A u32 count times 148 cannot overflow u64, nor can adding 8.
    let needed = u64::from(count) * INDEX_ENTRY_BYTES + TRAILER_BYTES;
    if needed > container_len {
        return Err(IndexError::IndexOutOfBounds {
            container: dispatch.container().to_owned(),
            offset: trailer_offset + 4,
            count,
            needed,
            container_len,
        });
    }
    let table_start = container_len - needed;

    let (entries, extents) =
        context.parse(TRAILER_ENTRYPOINT, bytes, |reader, allocation, _| {
            let members = u64::from(count);
            allocation.reserve("entries", table_start, members, INDEX_ROW_BYTES)?;
            allocation.reserve("extents", table_start, members, MEMBER_EXTENT_BYTES)?;
            reader.skip("data", table_start as usize)?;

            let mut entries = Vec::with_capacity(count as usize);
            let mut extents = Vec::with_capacity(count as usize);
            for index in 0..count as usize {
                let entry = read_entry(reader, index)?;
                extents.push(MemberExtent::new(entry.name(), None, entry.span()));
                entries.push(entry);
            }
            Ok((entries, extents))
        })?;

    Ok(VersionOneIndex {
        dispatch,
        bytes,
        table_start,
        entries,
        extents,
    })
}

/// Reads one 148-byte entry at the reader's position.
fn read_entry<'a>(reader: &mut Reader<'a>, index: usize) -> Result<IndexEntry<'a>, ParseError> {
    let offset = reader.position();
    let start = reader.read_u32("entry.start")?;
    let length = reader.read_u32("entry.length")?;
    let name_field = reader.read_bytes("entry.name", INDEX_NAME_BYTES)?;
    let unexplained = reader.read_bytes("entry.unexplained", INDEX_UNEXPLAINED_BYTES)?;

    let mut anomalies = 0u8;
    if length == 0 {
        anomalies |= EntryAnomaly::EmptyExtent.bit();
    }
    let name_len = match name_field.iter().position(|&byte| byte == 0) {
        Some(terminator) => {
            if name_field[terminator..].iter().any(|&byte| byte != 0) {
                anomalies |= EntryAnomaly::NonZeroNamePadding.bit();
            }
            terminator
        }
        None => {
            anomalies |= EntryAnomaly::UnterminatedName.bit();
            INDEX_NAME_BYTES
        }
    };
    if !name_field[..name_len].is_ascii() {
        anomalies |= EntryAnomaly::NonAsciiName.bit();
    }
    Ok(IndexEntry {
        index,
        record: SourceSpan {
            offset,
            length: INDEX_ENTRY_BYTES,
        },
        start,
        length,
        name_field,
        name_len,
        unexplained,
        anomalies,
    })
}
