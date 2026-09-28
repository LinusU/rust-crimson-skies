//! Raw ROF directory blocks: observed layout, fields preserved verbatim.
//!
//! Spec `specs/F05-rof-directory-trees-and-compressed-members.md` asks for a
//! reader "from observed layout, not a guessed generic ZIP wrapper": a
//! directory starts with little-endian `u32 entry_count` and `u32
//! names_length`, then `entry_count` records of six little-endian `u32`
//! fields (`start`, `length`, `length_on_disk`, `flags`, `name_length`,
//! `id`), then the name table (`docs/research/FORMAT-NOTES.md`, "ROF
//! observed structure [S05]", which summarizes the reference extractor;
//! the header/record/name layout is additionally confirmed by the
//! independently authored `fixtures/synthetic/flat-uncompressed.rof` and its
//! generator `tools/make_synthetic_fixtures.py`).
//!
//! The first stage (**F05-A**) defines the raw structs and validates *one*
//! block, [`read_directory`]:
//!
//! * every read runs through [`ParseContext`] (provenance, checked
//!   little-endian decoding, independent budgets), so a truncated field is a
//!   [`ParseError`] and never a panic;
//! * the name table is validated the way spec non-negotiable #2 demands:
//!   the number of NUL-delimited names equals `entry_count`, every declared
//!   name ends in its NUL, no name holds an interior NUL, and the declared
//!   `name_length` values (which count the name *plus* its terminator, as
//!   authored in the synthetic fixture) sum to exactly `names_length`;
//! * the two length fields stay separate: the on-disk words `length` and
//!   `length_on_disk` ([S05]) are exposed as [`RofRawRecord::raw_length`]
//!   and [`RofRawRecord::raw_length_on_disk`] without any interpretation,
//!   because the reference extractor reads `length` for compressed members
//!   and ignores `length_on_disk` and no local corpus has resolved the
//!   difference (spec non-negotiable #4, and the F05 deliverable names the
//!   preserved fields `raw_length` / `raw_length_on_disk`).
//!
//! The second stage (**F05-B**) follows those blocks and reads members:
//!
//! * [`read_tree`] walks the whole directory tree from the root at offset
//!   zero, with cycle detection (a `start` already open on the path from the
//!   root), bounded depth (this parse's [`RecursionBudget`]) and a work bound
//!   taken from the container length — the blocks the walk visits may not add
//!   up to more bytes than the container holds, so a shared or cyclic block
//!   cannot make the traversal run away (spec non-negotiable #3);
//! * every extent is established against the container length before
//!   anything is read: both length words of *every* record — directory
//!   records included — and the header a nested block must fit inside, so
//!   a member or directory pointer that reaches past the end is an
//!   [`RofError::ExtentOutOfBounds`], and no span is extracted
//!   from an entry whose `flags` hold a bit or a combination with no
//!   observed meaning, or from extents that overlap — spec non-negotiable
//!   #5 says to surface [`RofError::UnsupportedLayout`] absent evidence of
//!   legitimate sharing;
//! * [`read_member`] decodes one established extent through a **bounded**
//!   zlib decoder: the stored extent is the record's `raw_length` (the
//!   field the reference extractor reads [S05]), the decoded bytes are
//!   capped by [`RofLimits::max_decoded_bytes`] (an expansion bomb is
//!   refused as [`RofError::ExpansionBomb`]), and the bytes the stream did
//!   not consume are reported as trailing data rather than silently
//!   dropped (spec non-negotiable #3 and #4).
//!
//! What is deliberately **not** here: mounting into the VFS and inspection
//! (F05-C) and the compressed-length semantics, which stay unresolved until
//! a retail corpus can compare them (F05-D). Names are returned as bytes,
//! not `str`: locales are not guaranteed to be UTF-8 (spec non-negotiable
//! #2).
//!
//! A *root* directory is the bytes from offset zero of the file; a nested
//! directory is the bytes from its record's `start`. [`read_directory`]
//! only parses the block it is handed, so its error offsets are relative to
//! that byte range; [`read_tree`] reports absolute container offsets for
//! every failure, including the ones raised inside a nested block.
//!
//! Every fixture exercised below is newly authored synthetic bytes; nothing
//! here is derived from original game data.
//!
//! ```
//! use cs_formats::{read_directory, ParseContext};
//!
//! // One authored block: header, a single record, then the name table.
//! let mut bytes = Vec::new();
//! bytes.extend_from_slice(&1u32.to_le_bytes()); // entry_count
//! bytes.extend_from_slice(&5u32.to_le_bytes()); // names_length
//! for field in [37u32, 0, 0, 0, 5, 7] {
//!     bytes.extend_from_slice(&field.to_le_bytes());
//! }
//! bytes.extend_from_slice(b"PLAN\0");
//!
//! let mut context = ParseContext::with_defaults("synthetic/rof_doc.rof");
//! let directory = read_directory(&mut context, &bytes).expect("the block is valid");
//! assert_eq!(directory.header().entry_count, 1);
//! assert_eq!(directory.name_bytes(0), Some(b"PLAN".as_slice()));
//! assert_eq!(directory.record(0).map(|record| record.id), Some(7));
//! ```
//!
//! The same bytes, read as a whole tree with one member:
//!
//! ```
//! use cs_formats::{ParseContext, RofLimits, read_member, read_tree};
//!
//! let names = b"PLAN\0";
//! let block_len = 8 + 24 + names.len();
//! let mut bytes = Vec::new();
//! bytes.extend_from_slice(&1u32.to_le_bytes()); // entry_count
//! bytes.extend_from_slice(&(names.len() as u32).to_le_bytes()); // names_length
//! for field in [
//!     block_len as u32, // start
//!     6,                // raw_length
//!     6,                // raw_length_on_disk
//!     0,                // flags: uncompressed file
//!     5,                // name_length
//!     7,                // id
//! ] {
//!     bytes.extend_from_slice(&field.to_le_bytes());
//! }
//! bytes.extend_from_slice(names);
//! bytes.extend_from_slice(b"flight");
//!
//! let mut context = ParseContext::with_defaults("synthetic/rof_doc.rof");
//! let tree = read_tree(&mut context, &bytes).expect("the tree is valid");
//! assert_eq!(tree.members().len(), 1);
//! assert_eq!(tree.members()[0].path, vec![b"PLAN".as_slice()]);
//!
//! let read = read_member(&context, &bytes, &tree.members()[0], &RofLimits::default())
//!     .expect("the extent is inside the container");
//! assert_eq!(read.data, b"flight");
//! assert_eq!((read.stored_len, read.decoded_len, read.trailing_len), (6, 6, 0));
//! ```

use std::fmt;
use std::mem::size_of;
use std::slice;

use miniz_oxide::inflate::stream::{InflateState, inflate};
use miniz_oxide::{DataFormat, MZError, MZFlush, MZStatus};

use crate::error::ParseError;
use crate::io::{AllocationBudget, ParseContext, Reader, RecursionBudget};

/// Error scope stamped onto failures raised inside [`read_directory`].
///
/// A structural failure leaves as `rof.directory.<field>`; the name-table
/// checks carry their own field name and offset instead (see [`RofError`]).
pub const DIRECTORY_ENTRYPOINT: &str = "rof.directory";

/// Error scope stamped onto failures raised inside [`read_tree`].
///
/// A structural failure inside the root block leaves as `rof.tree.<field>`,
/// one raised inside a nested block as `rof.tree.directory.<field>`, and the
/// reservation that books the whole tree as `rof.tree.records`. The
/// domain failures ([`RofError::Cycle`], [`RofError::UnsupportedLayout`], …)
/// carry their own `code()` and an absolute offset instead (see [`RofError`]).
pub const TREE_ENTRYPOINT: &str = "rof.tree";

/// Bytes of decoded member output the bounded zlib decoder produces per
/// call ([`read_member`]). This is an I/O chunk, not a limit: the limit is
/// [`RofLimits::max_decoded_bytes`], checked against the running total
/// before any chunk is appended.
const DECODE_CHUNK_BYTES: usize = 8 * 1024;

/// Bytes in a directory header: `entry_count` then `names_length`, both
/// little-endian `u32`.
pub const DIRECTORY_HEADER_BYTES: usize = 8;

/// Bytes in one directory record: six little-endian `u32` fields. This is
/// also the size of [`RofRawRecord`], which is what lets the allocation
/// budget model the decoded table exactly (`records * RECORD_BYTES`).
pub const RECORD_BYTES: usize = 24;

/// Observed flag bit 1: the entry is a directory whose `start` is the
/// absolute offset of a nested block ([S05]).
pub const FLAG_DIRECTORY: u32 = 1;

/// Observed flag bit 2: the entry is compressed; the reference extractor
/// passes `length` bytes to zlib ([S05]).
pub const FLAG_COMPRESSED: u32 = 2;

/// Every flag bit with an observed meaning. Bits outside this mask are
/// preserved by the raw reader and unknown to this stage.
pub const KNOWN_FLAG_MASK: u32 = FLAG_DIRECTORY | FLAG_COMPRESSED;

/// The raw `flags` word of one record.
///
/// Bits are kept exactly as authored: the two observed bits are exposed
/// through [`Self::is_directory`] and [`Self::is_compressed`], and
/// [`Self::unknown_bits`] reports everything else so a consumer can refuse
/// an unexplained combination before it reads any span (spec F05,
/// non-negotiable #5). [`read_directory`] never rejects a record for its
/// flags — it extracts nothing; [`read_tree`] and [`read_member`] are the
/// readers that refuse to act on a flag word they cannot explain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RofFlags(pub u32);

impl RofFlags {
    /// Wraps the verbatim `flags` word.
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// The verbatim `flags` word.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Whether observed bit 1 (directory) is set.
    pub const fn is_directory(self) -> bool {
        self.0 & FLAG_DIRECTORY != 0
    }

    /// Whether observed bit 2 (compressed) is set.
    pub const fn is_compressed(self) -> bool {
        self.0 & FLAG_COMPRESSED != 0
    }

    /// The set bits with no observed meaning ([`KNOWN_FLAG_MASK`]).
    pub const fn unknown_bits(self) -> u32 {
        self.0 & !KNOWN_FLAG_MASK
    }

    /// Whether any bit with no observed meaning is set.
    pub const fn has_unknown_bits(self) -> bool {
        self.unknown_bits() != 0
    }
}

impl fmt::Display for RofFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:08x}", self.0)
    }
}

/// The two little-endian `u32` fields a directory block starts with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RofRawHeader {
    /// Number of 24-byte records that follow the header.
    pub entry_count: u32,
    /// Declared byte length of the name table at the end of the block.
    pub names_length: u32,
}

/// One record's six little-endian `u32` fields, in on-disk order.
///
/// The struct is exactly [`RECORD_BYTES`] bytes — no padding, no
/// reinterpretation — so the allocation budget charged for a decoded table
/// matches the memory it occupies.
///
/// The on-disk words are `length` and `length_on_disk` ([S05]); the F05
/// deliverable asks the reader to preserve them as `raw_length` and
/// `raw_length_on_disk`, and that `raw_` prefix is the reminder that
/// neither is interpreted here (spec F05, non-negotiable #4): they are two
/// independent values, never collapsed into one.
///
/// Likewise `start` and the lengths are recorded, never bounds-checked
/// here: a nested block or member extent can only be validated against the
/// whole file, which is what [`read_tree`] does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RofRawRecord {
    /// Absolute offset of the entry's data or nested directory block.
    pub start: u32,
    /// First length field, verbatim (on-disk word `length`).
    pub raw_length: u32,
    /// Second length field, verbatim (on-disk word `length_on_disk`);
    /// deliberately not collapsed into [`Self::raw_length`].
    pub raw_length_on_disk: u32,
    /// Raw `flags` word.
    pub flags: RofFlags,
    /// Declared byte length of this entry's name *including* its NUL.
    pub name_length: u32,
    /// The record's id, verbatim: stable identity inside the block.
    pub id: u32,
}

/// One validated directory block, parsed from the start of a byte range.
///
/// Built by [`read_directory`], which checks the header, the record table
/// bounds and the name table before anything is allocated, so every
/// `RofDirectory` that exists has a name table its records describe
/// exactly. The block occupies [`Self::block_len`] bytes of the input;
/// anything after that (member payloads, further blocks) is left alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RofDirectory<'a> {
    header: RofRawHeader,
    records: Vec<RofRawRecord>,
    /// Name table, `header.names_length` bytes, borrowed from the input.
    names: &'a [u8],
    block_len: usize,
}

/// One entry of a directory: its index, its raw record and its name
/// (without the terminating NUL, which is counted in the record's
/// [`RofRawRecord::name_length`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RofEntry<'a> {
    /// Position of the entry in the block, `0..entry_count`.
    pub index: usize,
    /// The entry's raw record.
    pub record: RofRawRecord,
    /// The entry's name bytes as authored, terminator removed.
    pub name: &'a [u8],
}

impl<'a> RofDirectory<'a> {
    /// The block's header.
    pub fn header(&self) -> RofRawHeader {
        self.header
    }

    /// Number of records in the block.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the block declares no records.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The raw record table, in block order.
    pub fn records(&self) -> &[RofRawRecord] {
        &self.records
    }

    /// The raw record at `index`, or `None` when `index` is out of range.
    pub fn record(&self, index: usize) -> Option<&RofRawRecord> {
        self.records.get(index)
    }

    /// The name of entry `index` without its NUL, or `None` when `index` is
    /// out of range.
    ///
    /// The bytes are borrowed from the input: no copy, no UTF-8 assumption
    /// (spec F05, non-negotiable #2).
    pub fn name_bytes(&self, index: usize) -> Option<&'a [u8]> {
        let record = self.records.get(index)?;
        let mut start = 0usize;
        for earlier in &self.records[..index] {
            start += earlier.name_length as usize;
        }
        // `read_directory` proved the declared names cover the table
        // exactly, so `start + record.name_length` ends inside `names`
        // (and cannot overflow: both bounds are bounded by `names.len()`).
        let end = start + record.name_length as usize;
        self.names.get(start..end).map(|name| {
            if name.is_empty() {
                name
            } else {
                &name[..name.len() - 1]
            }
        })
    }

    /// Entries of this block, in block order.
    pub fn entries(&self) -> RofEntries<'_, 'a> {
        RofEntries {
            records: self.records.iter(),
            names: self.names,
            cursor: 0,
            index: 0,
        }
    }

    /// Bytes this block occupies at the start of the input: header plus
    /// record table plus name table. Member payloads and further blocks
    /// start after it.
    pub fn block_len(&self) -> usize {
        self.block_len
    }
}

/// Iterator over a directory's [`RofEntry`]s, in block order.
///
/// Names are sliced with a cursor that walks the validated name table once,
/// so iterating a block is linear in the table, not quadratic.
#[derive(Clone, Debug)]
pub struct RofEntries<'dir, 'a> {
    records: slice::Iter<'dir, RofRawRecord>,
    names: &'a [u8],
    /// Byte cursor of the next name inside `names`.
    cursor: usize,
    /// Index of the next entry.
    index: usize,
}

impl<'a> Iterator for RofEntries<'_, 'a> {
    type Item = RofEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let &record = self.records.next()?;
        let start = self.cursor;
        // The record's name is `name_length` bytes, its last one the NUL
        // `read_directory` validated, and the validated sums keep the range
        // inside `names`, so this hands out the name without its NUL.
        let names = self.names;
        let name = names
            .get(start..start + record.name_length as usize)
            .map(|name| {
                debug_assert!(!name.is_empty(), "a validated name is never empty");
                if name.is_empty() {
                    name
                } else {
                    &name[..name.len() - 1]
                }
            })
            .unwrap_or(&[]);
        self.cursor = start + record.name_length as usize;
        let index = self.index;
        self.index += 1;
        Some(RofEntry {
            index,
            record,
            name,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.records.size_hint()
    }
}

impl ExactSizeIterator for RofEntries<'_, '_> {}

/// Why a directory block was rejected.
///
/// Every variant names the container and the byte offset of the problem
/// inside the range that was handed to [`read_directory`], plus a
/// machine-matchable [`Self::code`]. Payload bytes never appear in an
/// error: a diagnostic must not echo private installation data.
///
/// The name-table checks are their own variants rather than
/// [`ParseError`]s because no existing [`crate::ParseErrorKind`] describes
/// "the records describe a different number of name bytes than the header
/// declares": reusing one would make a machine-matched kind lie. Structural
/// failures of the reader itself (truncated fields, an allocation budget
/// refused) are [`RofError::Parse`] and keep the F03 field path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RofError {
    /// A structural failure from the checked reader, already scoped as
    /// `rof.directory.<field>` by the parser entrypoint.
    Parse(ParseError),
    /// The declared `name_length` fields do not sum to the header's
    /// `names_length`, so the name table and the record table disagree
    /// about where names end.
    NameTableLength {
        /// Container label the bytes came from.
        container: String,
        /// Offset of the name table inside the handed-in range.
        offset: u64,
        /// Bytes the header declares for the name table.
        declared: u64,
        /// Bytes the records' `name_length` fields describe.
        described: u64,
    },
    /// A record declares `name_length == 0`, i.e. a name with no room for
    /// its terminator.
    EmptyName {
        /// Container label the bytes came from.
        container: String,
        /// Offset where that name would start.
        offset: u64,
        /// Index of the offending record.
        index: usize,
    },
    /// A declared name does not end in `0x00`.
    UnterminatedName {
        /// Container label the bytes came from.
        container: String,
        /// Offset of the offending name.
        offset: u64,
        /// Index of the offending record.
        index: usize,
    },
    /// A declared name holds a `0x00` before its final byte, so NUL
    /// splitting and the records disagree about how many names exist.
    InteriorNul {
        /// Container label the bytes came from.
        container: String,
        /// Offset of the offending name.
        offset: u64,
        /// Index of the offending record.
        index: usize,
    },
    /// A directory record points at a block that is already open on the
    /// path from the root, so following `start` would loop (spec F05,
    /// non-negotiable #3).
    Cycle {
        /// Container label the bytes came from.
        container: String,
        /// Absolute offset of the block that closes the cycle.
        offset: u64,
        /// Directory levels open when the repeat was found (root is 1).
        depth: u32,
    },
    /// An entry's declared extent reaches past the end of the container, so
    /// no span of it can be read ("outside-file pointer", spec F05
    /// acceptance AC03).
    ExtentOutOfBounds {
        /// Container label the bytes came from.
        container: String,
        /// Absolute offset that made the extent interesting: the extent's
        /// own `start` for a record whose length word reaches past the end
        /// (file record or directory record), the block offset for a
        /// directory block that does not even fit its header.
        offset: u64,
        /// Absolute start of the extent.
        start: u64,
        /// Declared byte length of the extent: `raw_length` or
        /// `raw_length_on_disk` for a file entry, [`DIRECTORY_HEADER_BYTES`]
        /// for the smallest directory block.
        length: u64,
        /// Length of the container the extent was checked against.
        file_len: u64,
    },
    /// The read would produce (or has already produced) more decoded bytes
    /// than [`RofLimits::max_decoded_bytes`] allows: an expansion bomb
    /// (spec F05 acceptance AC03). Raised before the excess is written.
    ExpansionBomb {
        /// Container label the bytes came from.
        container: String,
        /// Absolute offset of the member's stored extent.
        offset: u64,
        /// The configured ceiling on decoded bytes.
        limit: u64,
        /// Bytes the read would produce at least.
        observed: u64,
    },
    /// A layout this reader does not act on: flag bits with no observed
    /// meaning, the unobserved directory+compressed combination, extents
    /// that overlap, or a directory block reached from more than one
    /// parent. Spec F05 non-negotiable #5: absent independently documented
    /// evidence of legitimate sharing, this is surfaced *instead of*
    /// extracting an arbitrary span.
    UnsupportedLayout {
        /// Container label the bytes came from.
        container: String,
        /// Absolute offset where the problem was found.
        offset: u64,
        /// What makes the layout unsupported. Counts, offsets and flag
        /// words only — never a member name out of the container.
        detail: String,
    },
    /// The bounded zlib decoder refused the stream: truncated, corrupt, a
    /// missing preset dictionary or an adler32 mismatch.
    DecodeFailure {
        /// Container label the bytes came from.
        container: String,
        /// Absolute offset of the member's stored extent.
        offset: u64,
        /// What the decoder reported. Never payload bytes.
        detail: String,
    },
}

impl RofError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Parse(_) => "parse",
            Self::NameTableLength { .. } => "name_table_length",
            Self::EmptyName { .. } => "empty_name",
            Self::UnterminatedName { .. } => "unterminated_name",
            Self::InteriorNul { .. } => "interior_nul",
            Self::Cycle { .. } => "cycle",
            Self::ExtentOutOfBounds { .. } => "extent_out_of_bounds",
            Self::ExpansionBomb { .. } => "expansion_bomb",
            Self::UnsupportedLayout { .. } => "unsupported_layout",
            Self::DecodeFailure { .. } => "decode_failure",
        }
    }

    /// The container label the bytes came from.
    pub fn container(&self) -> &str {
        match self {
            Self::Parse(error) => &error.container,
            Self::NameTableLength { container, .. }
            | Self::EmptyName { container, .. }
            | Self::UnterminatedName { container, .. }
            | Self::InteriorNul { container, .. }
            | Self::Cycle { container, .. }
            | Self::ExtentOutOfBounds { container, .. }
            | Self::ExpansionBomb { container, .. }
            | Self::UnsupportedLayout { container, .. }
            | Self::DecodeFailure { container, .. } => container,
        }
    }

    /// Offset of the problem. Relative to the range that was handed to
    /// [`read_directory`] (so a root failure offset *is* the file offset,
    /// a nested one is relative to its block); [`read_tree`] shifts nested
    /// failures to absolute container offsets with [`Self::in_block`].
    pub fn offset(&self) -> u64 {
        match self {
            Self::Parse(error) => error.offset,
            Self::NameTableLength { offset, .. }
            | Self::EmptyName { offset, .. }
            | Self::UnterminatedName { offset, .. }
            | Self::InteriorNul { offset, .. }
            | Self::Cycle { offset, .. }
            | Self::ExtentOutOfBounds { offset, .. }
            | Self::ExpansionBomb { offset, .. }
            | Self::UnsupportedLayout { offset, .. }
            | Self::DecodeFailure { offset, .. } => *offset,
        }
    }

    /// Re-bases an error raised *inside* a directory block onto the
    /// absolute container offset of that block and scopes it to that block.
    ///
    /// [`read_directory`] only ever sees the range it is handed, so its
    /// failures are relative to the start of that range; [`read_tree`] reads
    /// a block from `file[offset..]` and must report where the block
    /// actually is — and *which* block raised it, since a nested field such
    /// as `header.entry_count` is otherwise indistinguishable from the same
    /// field failing in the root block. `scope` is `"directory"` for a
    /// nested block (so the field reads `rof.tree.directory.<field>` once
    /// the entrypoint scope is applied) and empty for the root block, whose
    /// failures the entrypoint scope already describes. Only the variants
    /// [`borrow_block`] can raise are ever passed through here — the
    /// traversal's own failures are constructed with absolute offsets and
    /// no scope of their own.
    fn in_block(self, delta: u64, scope: &str) -> Self {
        match self {
            Self::Parse(mut error) => {
                error.offset += delta;
                Self::Parse(error.in_scope(scope))
            }
            Self::NameTableLength {
                container,
                offset,
                declared,
                described,
            } => Self::NameTableLength {
                container,
                offset: offset + delta,
                declared,
                described,
            },
            Self::EmptyName {
                container,
                offset,
                index,
            } => Self::EmptyName {
                container,
                offset: offset + delta,
                index,
            },
            Self::UnterminatedName {
                container,
                offset,
                index,
            } => Self::UnterminatedName {
                container,
                offset: offset + delta,
                index,
            },
            Self::InteriorNul {
                container,
                offset,
                index,
            } => Self::InteriorNul {
                container,
                offset: offset + delta,
                index,
            },
            Self::Cycle { .. }
            | Self::ExtentOutOfBounds { .. }
            | Self::ExpansionBomb { .. }
            | Self::UnsupportedLayout { .. }
            | Self::DecodeFailure { .. } => self,
        }
    }
}

impl From<ParseError> for RofError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl fmt::Display for RofError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "{error}"),
            Self::NameTableLength {
                container,
                offset,
                declared,
                described,
            } => write!(
                f,
                "name table at offset {offset} in {container}: records describe \
                 {described} bytes of names, header declares {declared}"
            ),
            Self::EmptyName {
                container,
                offset,
                index,
            } => write!(
                f,
                "record {index} at offset {offset} in {container}: declared name \
                 has no room for a terminator"
            ),
            Self::UnterminatedName {
                container,
                offset,
                index,
            } => write!(
                f,
                "record {index} at offset {offset} in {container}: declared name \
                 does not end in 0x00"
            ),
            Self::InteriorNul {
                container,
                offset,
                index,
            } => write!(
                f,
                "record {index} at offset {offset} in {container}: declared name \
                 holds an interior 0x00"
            ),
            Self::Cycle {
                container,
                offset,
                depth,
            } => write!(
                f,
                "directory at offset {offset} in {container} is already open {depth} \
                 levels up: following it would cycle"
            ),
            Self::ExtentOutOfBounds {
                container,
                offset,
                start,
                length,
                file_len,
            } => write!(
                f,
                "entry at offset {offset} in {container} declares extent [{start}, \
                 {}) outside the {file_len}-byte container",
                start + length
            ),
            Self::ExpansionBomb {
                container,
                offset,
                limit,
                observed,
            } => write!(
                f,
                "member at offset {offset} in {container} would decode to at least \
                 {observed} bytes, over the {limit}-byte limit"
            ),
            Self::UnsupportedLayout {
                container,
                offset,
                detail,
            } => write!(
                f,
                "unsupported layout at offset {offset} in {container}: {detail}"
            ),
            Self::DecodeFailure {
                container,
                offset,
                detail,
            } => write!(
                f,
                "member at offset {offset} in {container} failed to decode: {detail}"
            ),
        }
    }
}

impl std::error::Error for RofError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            _ => None,
        }
    }
}

/// Reads the directory block at the start of `bytes`.
///
/// `bytes` is the block itself: the whole file for a root directory (offset
/// zero, as the reference extractor reads it), or the slice from a record's
/// `start` for a nested one. Trailing bytes after the name table — member
/// payloads, other blocks — are not consumed and not validated; use
/// [`RofDirectory::block_len`] to know where the block ends.
///
/// The parse runs through [`ParseContext::parse`], so failures carry the
/// container label, the absolute offset, the logical field and the
/// expected/observed condition, and a failed attempt leaves the allocation
/// ledger exactly as it found it (the caller can retry the same bytes
/// honestly).
///
/// Validation happens **before** anything is charged or allocated: the
/// record table and name table are bounds-checked borrows first, the name
/// table is validated against the records next, and only then is the
/// decoded record table booked against the allocation budget and built. So
/// a rejected block never leaves a stale charge, and a record table that
/// would not fit the parse's budget is refused before a `Vec` exists.
///
/// # Errors
///
/// [`RofError::Parse`] for a truncated header/record table/name table or a
/// decoded table beyond the parse's allocation budget, and the
/// [`RofError`] name-table variants when the records and the name table do
/// not describe each other.
pub fn read_directory<'bytes>(
    context: &mut ParseContext,
    bytes: &'bytes [u8],
) -> Result<RofDirectory<'bytes>, RofError> {
    context.parse(
        DIRECTORY_ENTRYPOINT,
        bytes,
        |reader, allocation, _recursion| {
            // 1.-3. Header, bounds-checked borrows, name-table validation:
            // allocation-free, so a rejected block cannot leave a charge
            // behind and the ledger stays untouched either way.
            let view = match borrow_block(reader) {
                Ok(view) => view,
                // Structural failures keep the F03 field path and are
                // scoped by the entrypoint (and rolled back) as before.
                Err(RofError::Parse(error)) => return Err(error),
                // The name-table checks carry their own code and offset.
                Err(domain) => return Ok(Err(domain)),
            };

            // 4. Book the decoded record table against this parse's
            //    allocation budget, then build it. Only now does the parse
            //    hold memory.
            allocation.reserve(
                "records",
                0,
                u64::from(view.header.entry_count),
                RECORD_BYTES as u64,
            )?;
            Ok(Ok(view.into_directory()))
        },
    )?
}

/// One validated directory block, borrowed from the bytes it was read from.
///
/// This is everything [`RofDirectory`] holds except the decoded record
/// table, which needs an allocation: [`borrow_block`] produces it without
/// charging anything, so both [`read_directory`] (one block, one charge) and
/// [`read_tree`] (the whole tree, one charge after the traversal proved the
/// structure valid) can book the record table only once they know it will
/// be kept.
#[derive(Clone, Copy, Debug)]
struct BlockView<'a> {
    header: RofRawHeader,
    /// The record table, `entry_count * RECORD_BYTES` bytes.
    records: &'a [u8],
    /// The name table, `names_length` bytes.
    names: &'a [u8],
    /// Bytes the block occupies: header + records + names.
    block_len: usize,
}

impl<'a> BlockView<'a> {
    /// Builds the returned directory. The caller must have booked
    /// `entry_count * RECORD_BYTES` first.
    fn into_directory(self) -> RofDirectory<'a> {
        RofDirectory {
            header: self.header,
            records: decode_table(self.records),
            names: self.names,
            block_len: self.block_len,
        }
    }
}

/// Borrows and validates the directory block at the start of `reader`
/// without allocating anything (see [`BlockView`]).
///
/// Everything checks runs in order: the header is read with checked
/// little-endian reads, both tables are bounds-checked borrows (a hostile
/// count costs a refusal, never a buffer), and only then is the name table
/// validated against the records that point into it. A structural failure
/// surfaces as [`RofError::Parse`] so its caller can scope it with the
/// entrypoint; a name-table disagreement surfaces as its own variant.
fn borrow_block<'bytes>(reader: &mut Reader<'bytes>) -> Result<BlockView<'bytes>, RofError> {
    // 1. Header: the two declared lengths, checked little-endian reads.
    let header_offset = reader.position();
    let entry_count = reader.read_u32("header.entry_count")?;
    let names_length = reader.read_u32("header.names_length")?;
    let header = RofRawHeader {
        entry_count,
        names_length,
    };

    // 2. Bounds-checked borrows of the two tables.
    let records_len =
        reader.checked_byte_len("records", u64::from(entry_count), RECORD_BYTES as u64)?;
    let names_len = reader.checked_byte_len("name_table", 1, u64::from(names_length))?;
    let records = reader.read_bytes("records", records_len)?;
    let names_start = reader.position();
    let names = reader.read_bytes("name_table", names_len)?;
    let block_len = records_len
        .checked_add(names_len)
        .and_then(|total| total.checked_add(DIRECTORY_HEADER_BYTES))
        .ok_or_else(|| {
            ParseError::length_overflow(
                reader.container().to_owned(),
                header_offset,
                "block.len",
                "header + records + name_table to fit in usize".to_owned(),
                format!("{records_len} record bytes plus {names_len} name bytes"),
            )
        })?;

    // 3. Validate the name table against the records. Nothing has been
    //    charged yet, so a rejected block leaves the ledger untouched.
    validate_names(reader.container(), header, records, names_start, names)?;

    Ok(BlockView {
        header,
        records,
        names,
        block_len,
    })
}

/// Decodes a validated record table into the record vector.
fn decode_table(records: &[u8]) -> Vec<RofRawRecord> {
    let (words, remainder) = records.as_chunks::<RECORD_BYTES>();
    debug_assert!(
        remainder.is_empty(),
        "checked_byte_len produced whole records"
    );
    words.iter().map(|record| decode_record(record)).collect()
}

/// Decodes one 24-byte record. The slice is `records` from
/// [`read_directory`], already bounds-checked to `count * 24`.
fn decode_record(record: &[u8]) -> RofRawRecord {
    debug_assert_eq!(record.len(), RECORD_BYTES);
    let word = |at: usize| {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&record[at..at + 4]);
        u32::from_le_bytes(bytes)
    };
    RofRawRecord {
        start: word(0),
        raw_length: word(4),
        raw_length_on_disk: word(8),
        flags: RofFlags(word(12)),
        name_length: word(16),
        id: word(20),
    }
}

/// Checks the name table against the records that point into it
/// (spec F05, non-negotiable #2: count, termination and declared lengths).
///
/// `records` is the bounds-checked record table and `names` is exactly
/// `header.names_length` bytes at offset `names_start` of the handed-in
/// range, so this only compares what the two tables *say*, never the bytes
/// outside them.
fn validate_names(
    container: &str,
    header: RofRawHeader,
    records: &[u8],
    names_start: u64,
    names: &[u8],
) -> Result<(), RofError> {
    debug_assert_eq!(records.len(), header.entry_count as usize * RECORD_BYTES);
    let (words, remainder) = records.as_chunks::<RECORD_BYTES>();
    debug_assert!(
        remainder.is_empty(),
        "checked_byte_len produced whole records"
    );

    // Pass one: every declared name has room for a terminator, and the
    // declared lengths sum to exactly what the header declares.
    let mut described: u64 = 0;
    let mut cursor: u64 = 0;
    for (index, record) in words.iter().enumerate() {
        let name_length = decode_record(record).name_length;
        if name_length == 0 {
            return Err(RofError::EmptyName {
                container: container.to_owned(),
                offset: names_start + cursor,
                index,
            });
        }
        described = described
            .checked_add(u64::from(name_length))
            .ok_or_else(|| {
                RofError::Parse(ParseError::length_overflow(
                    container.to_owned(),
                    names_start,
                    "name_table.declared_length",
                    "sum of declared name lengths to fit in u64".to_owned(),
                    format!("{described} plus {name_length}"),
                ))
            })?;
        cursor += u64::from(name_length);
    }
    if described != u64::from(header.names_length) {
        return Err(RofError::NameTableLength {
            container: container.to_owned(),
            offset: names_start,
            declared: u64::from(header.names_length),
            described,
        });
    }

    // Pass two: the sums match, so every declared slice is inside the
    // table and can be checked for its terminator and for interior NULs.
    let mut start = 0usize;
    for (index, record) in words.iter().enumerate() {
        let name_length = decode_record(record).name_length as usize;
        let end = start + name_length;
        let name = &names[start..end];
        if *name.last().expect("name_length >= 1") != 0 {
            return Err(RofError::UnterminatedName {
                container: container.to_owned(),
                offset: names_start + start as u64,
                index,
            });
        }
        if name[..name_length - 1].contains(&0) {
            return Err(RofError::InteriorNul {
                container: container.to_owned(),
                offset: names_start + start as u64,
                index,
            });
        }
        start = end;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// F05-B: directory traversal and bounded member reads
// ---------------------------------------------------------------------------

/// The limits one member read is bounded by.
///
/// These are *Designed* values (EvidenceClass `Designed`): no original game
/// limit is observed or implied by them. [`RofLimits::default`] is the
/// configured surface every read must be handed, so no call site silently
/// picks a ceiling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RofLimits {
    /// Ceiling on the decoded bytes one read may produce.
    ///
    /// The stored extent cannot exceed the container — [`read_tree`] and
    /// [`read_member`] both check that first — but a small compressed
    /// stream can decode to far more than it occupies, so this is the
    /// expansion-bomb bound: a read that would cross it fails as
    /// [`RofError::ExpansionBomb`] with the bytes it would have produced
    /// (spec F05, non-negotiable #3 and acceptance AC03).
    pub max_decoded_bytes: u64,
}

impl RofLimits {
    /// The designed default ceiling: [`AllocationBudget::DEFAULT_LIMIT`]
    /// (64 MiB), the same bound the parse budgets use for a decoded table.
    pub const DEFAULT_MAX_DECODED_BYTES: u64 = AllocationBudget::DEFAULT_LIMIT;

    /// Limits with an explicit decoded-byte ceiling (the tested
    /// configuration surface: `0` refuses every read that produces a byte).
    pub const fn new(max_decoded_bytes: u64) -> Self {
        Self { max_decoded_bytes }
    }
}

impl Default for RofLimits {
    fn default() -> Self {
        Self::new(Self::DEFAULT_MAX_DECODED_BYTES)
    }
}

/// One directory block visited by [`read_tree`].
#[derive(Clone, Debug)]
pub struct RofTreeDirectory<'a> {
    /// Directory names from the root down to this block, the block's own
    /// entries not included; empty for the root block at offset zero.
    ///
    /// Bytes, not `str`: names are not guaranteed to be UTF-8 in every
    /// locale (spec F05, non-negotiable #2).
    pub path: Vec<&'a [u8]>,
    /// Absolute offset of the block inside the container.
    pub offset: u64,
    /// The parsed block. Its record table was booked against the parse's
    /// allocation budget before the tree was returned.
    pub directory: RofDirectory<'a>,
}

/// A file member of a [`RofTree`], with **both** declared extents
/// validated against the container length.
///
/// The two length words stay separate and uninterpreted (spec F05,
/// non-negotiable #4): [`Self::length_end`] is `start + raw_length` and
/// [`Self::length_on_disk_end`] is `start + raw_length_on_disk`, and which
/// of the two means "stored" is exactly what a retail corpus still has to
/// resolve (F05-D). The reader acts on `raw_length` alone, because that is
/// the field the reference extractor reads ([S05]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RofMember<'a> {
    /// Path segments from the root, the member's own name last.
    pub path: Vec<&'a [u8]>,
    /// The record verbatim, both length words preserved.
    pub record: RofRawRecord,
    /// Absolute start of the member (`start`, zero-extended to `u64`).
    pub start: u64,
    /// `start + raw_length`, validated `<= container_len`.
    pub length_end: u64,
    /// `start + raw_length_on_disk`, validated `<= container_len`.
    pub length_on_disk_end: u64,
}

/// The whole directory tree: every block and every file member, in
/// depth-first visit order (the root block first).
///
/// Built by [`read_tree`], which validates the traversal — cycle detection,
/// bounded depth, extents inside the container, explainable flags and no
/// overlapping spans — *before* it books anything, so a tree that exists
/// has exactly the record tables its blocks declared and members whose
/// extents lie inside the container.
#[derive(Clone, Debug)]
pub struct RofTree<'a> {
    directories: Vec<RofTreeDirectory<'a>>,
    members: Vec<RofMember<'a>>,
}

impl<'a> RofTree<'a> {
    /// The root block at offset zero. It is always present: a tree only
    /// exists once the root block parsed.
    pub fn root(&self) -> &RofTreeDirectory<'a> {
        self.directories
            .first()
            .expect("read_tree always visits the root block")
    }

    /// Every visited directory block, depth-first with the root first.
    pub fn directories(&self) -> &[RofTreeDirectory<'a>] {
        &self.directories
    }

    /// Every file member, depth-first, each with a validated extent.
    pub fn members(&self) -> &[RofMember<'a>] {
        &self.members
    }
}

/// What one [`read_member`] call returned, with the selected profile's two
/// lengths reported side by side (spec F05 acceptance AC02).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RofMemberRead {
    /// The decoded bytes: a verbatim copy of the extent for an
    /// uncompressed member, the bounded zlib decoder's output for a
    /// compressed one.
    pub data: Vec<u8>,
    /// Bytes taken from the container: the member's `raw_length` extent,
    /// which is the field the reference extractor reads for compressed
    /// members ([S05]). Never the `raw_length_on_disk` word.
    pub stored_len: u64,
    /// Bytes of [`Self::stored_len`] the zlib stream did not consume:
    /// data sitting after the end of the stream inside the same extent
    /// (spec F05, non-negotiable #4 asks for exact boundaries and trailing
    /// data rather than a silent skip).
    pub trailing_len: u64,
    /// Decoded byte count ([`Self::data`].`len()`), always
    /// `<= RofLimits::max_decoded_bytes`.
    pub decoded_len: u64,
}

/// Walks the whole directory tree of `file` and returns every block and
/// member it contains.
///
/// The root block is read from offset zero — the way the reference
/// extractor starts ([S05]) — and each directory record's `start` is
/// followed with (spec F05, non-negotiable #3):
///
/// * **cycle detection** — a `start` already open on the path from the
///   root is [`RofError::Cycle`];
/// * **bounded depth** — this parse's [`RecursionBudget`] refuses level
///   `max_depth + 1`, exactly as it does for any other nested parser;
/// * **bounded work** — the blocks the walk visits may not add up to more
///   bytes than the container holds: distinct blocks are disjoint and at
///   least [`DIRECTORY_HEADER_BYTES`] long, so a block reached again from
///   another parent, or blocks that reinterpret each other's bytes, stop
///   the walk after work linear in the container rather than being
///   entered again, surfaced as [`RofError::UnsupportedLayout`].
///
/// Before anything is booked the walk also proves that every declared
/// extent lies inside the container ([`RofError::ExtentOutOfBounds`]
/// otherwise), that every `flags` word has an observed meaning and an
/// observed combination ([`RofError::UnsupportedLayout`] otherwise), and
/// that no two non-empty extents share a byte (spec F05, non-negotiable
/// #5: absent independently documented evidence of legitimate sharing,
/// the overlap is surfaced instead of extracting arbitrary spans).
///
/// # Allocation accounting
///
/// The walk itself charges nothing: it collects borrowed views of `file`
/// plus the path vectors, all bounded by the container's own entry count
/// and this parse's depth limit. **Nothing is charged while the read can
/// still fail.** The single reservation that follows the successful walk
/// covers everything the returned tree keeps — every record table, both
/// node arrays and the path slice headers — and the tree is built only
/// after that reservation was accepted. So:
///
/// * every failure path leaves [`ParseContext::allocation`] exactly as it
///   found it (F03-C's retry contract);
/// * a tree that does not fit the budget is refused before a single
///   `Vec<RofRawRecord>` exists — the decoded records the budget is there
///   for are built only after the reservation was accepted;
/// * one `reserve` call rather than one per block, because the budget has
///   no release operation: a partially booked tree could never be undone.
///
/// The member nodes and path vectors are allocated during the walk, before
/// that reservation, and are either booked with it or dropped with a
/// failure; nothing unbooked is ever returned.
/// The walk's own transient buffers (the visited offsets, the spans to
/// compare) are dropped either way and are never charged.
///
/// # Errors
///
/// [`RofError::Parse`] for a structurally invalid block (truncated header
/// or tables, a record table beyond the budget, depth past this parse's
/// recursion limit) with absolute container offsets, and the domain
/// variants listed above for everything the walk refuses to act on.
pub fn read_tree<'bytes>(
    context: &mut ParseContext,
    file: &'bytes [u8],
) -> Result<RofTree<'bytes>, RofError> {
    // Everything runs in one attempt, in three phases, so the entrypoint's
    // rollback covers the whole read:
    //
    // 1. walk and validate. Structural failures leave as `Err(ParseError)`
    //    so `parse` scopes them; domain failures leave as their own variant
    //    and offset. Nothing is charged on either path, so a failure leaves
    //    the ledger exactly as it found it.
    // 2. book everything the returned tree keeps, in one reservation, so a
    //    refusal charges nothing at all.
    // 3. build the tree: the structures are validated and booked, so
    //    decoding the record tables cannot fail.
    context.parse(TREE_ENTRYPOINT, file, |reader, allocation, recursion| {
        let mut walker = Walker::new(reader.container(), file, recursion);
        let mut path = Vec::new();
        let plan = match walker
            .visit(0, &mut path, true)
            .and_then(|()| walker.plan.check_overlaps(reader.container()))
        {
            Ok(()) => walker.plan,
            Err(RofError::Parse(error)) => return Err(error),
            Err(domain) => return Ok(Err(domain)),
        };

        let bytes = plan.booking_bytes().ok_or_else(|| {
            ParseError::length_overflow(
                reader.container().to_owned(),
                0,
                "records",
                "the booked tree to fit in u64".to_owned(),
                format!(
                    "{} directory blocks and {} members",
                    plan.directories.len(),
                    plan.members.len()
                ),
            )
        })?;
        allocation.reserve("records", 0, 1, bytes)?;

        Ok(Ok(plan.into_tree()))
    })?
}

/// One validated block before its record table is booked.
struct DirectoryPlan<'bytes> {
    path: Vec<&'bytes [u8]>,
    offset: u64,
    view: BlockView<'bytes>,
}

/// What the validation walk produced: borrowed blocks, members, the
/// extents to compare for overlap, and the counts [`read_tree`] books
/// before it builds anything.
#[derive(Default)]
struct Plan<'bytes> {
    directories: Vec<DirectoryPlan<'bytes>>,
    members: Vec<RofMember<'bytes>>,
    /// Non-empty `[start, end)` extents of every visited block and every
    /// member. Only spans the reader would actually touch: a directory
    /// block's bytes and a member's `raw_length` extent.
    spans: Vec<(u64, u64)>,
    /// Path slice headers the tree keeps (`Vec<&[u8]>` buffers), counted
    /// so the booking covers them.
    path_slices: u64,
    /// Sum of `entry_count` over every visited block.
    total_records: u64,
}

impl<'bytes> Plan<'bytes> {
    /// Bytes [`read_tree`] books in one reservation: every record table of
    /// every visited block, both node arrays, and the path slice headers.
    ///
    /// Returns `None` only if the arithmetic overflows, which cannot
    /// happen for a container a `usize` slice can hold; the caller turns it
    /// into a `LengthOverflow` failure rather than a smaller booking.
    fn booking_bytes(&self) -> Option<u64> {
        let records = self.total_records.checked_mul(RECORD_BYTES as u64)?;
        let directories = (self.directories.len() as u64)
            .checked_mul(size_of::<RofTreeDirectory<'bytes>>() as u64)?;
        let members = (self.members.len() as u64).checked_mul(size_of::<RofMember>() as u64)?;
        let paths = self.path_slices.checked_mul(size_of::<&[u8]>() as u64)?;
        directories
            .checked_add(members)?
            .checked_add(records)?
            .checked_add(paths)
    }

    /// Builds the returned tree from the validated plan.
    ///
    /// `read_tree` books [`Self::booking_bytes`] before this runs, so the
    /// record tables decoded here are exactly the bytes the reservation
    /// paid for and nothing here can fail.
    fn into_tree(self) -> RofTree<'bytes> {
        let Plan {
            directories,
            members,
            spans: _,
            path_slices: _,
            total_records: _,
        } = self;
        RofTree {
            directories: directories
                .into_iter()
                .map(|directory| RofTreeDirectory {
                    path: directory.path,
                    offset: directory.offset,
                    directory: directory.view.into_directory(),
                })
                .collect(),
            members,
        }
    }

    /// Rejects two non-empty extents that share a byte.
    ///
    /// Sorted once, so only adjacent pairs have to be compared: if span `i`
    /// overlaps span `j` (`j > i`), span `i + 1` starts no later than `j`
    /// and overlaps span `i` too.
    fn check_overlaps(&mut self, container: &str) -> Result<(), RofError> {
        self.spans.sort_unstable();
        for pair in self.spans.windows(2) {
            let (first_start, first_end) = pair[0];
            let (second_start, second_end) = pair[1];
            if second_start < first_end {
                return Err(RofError::UnsupportedLayout {
                    container: container.to_owned(),
                    offset: second_start,
                    detail: format!(
                        "the extent [{first_start}, {first_end}) overlaps the extent \
                         [{second_start}, {second_end}) and no source documents sharing \
                         as legitimate"
                    ),
                });
            }
        }
        Ok(())
    }
}

/// The traversal state shared by every level of the walk.
struct Walker<'ctx, 'bytes> {
    /// Container label every failure of this walk reports.
    container: &'ctx str,
    file: &'bytes [u8],
    recursion: &'ctx RecursionBudget,
    plan: Plan<'bytes>,
    /// Offsets of the blocks open on the path from the root: a `start`
    /// found here closes a cycle.
    ancestors: Vec<u64>,
    /// Bytes of directory block visited so far. Every block is at least a
    /// header long and disjoint blocks cannot add up to more than the
    /// container holds, so this caps both the number of blocks and the work
    /// the walk does at something linear in the container — however the
    /// records point, a block reinterpretation cannot be paid for twice.
    visited_bytes: u64,
}

impl<'ctx, 'bytes> Walker<'ctx, 'bytes> {
    fn new(container: &'ctx str, file: &'bytes [u8], recursion: &'ctx RecursionBudget) -> Self {
        Self {
            container,
            file,
            recursion,
            plan: Plan::default(),
            ancestors: Vec::new(),
            visited_bytes: 0,
        }
    }

    /// Visits the directory block at `offset`, validating its entries and
    /// recursing into its directories.
    ///
    /// `path` holds the directory names leading to this block; the
    /// ancestor chain is tracked separately because cycle detection is
    /// about offsets, not names. `root` marks the block at offset zero:
    /// its structural failures keep the entrypoint's own field
    /// (`rof.tree.<field>`), a nested block's are scoped
    /// `rof.tree.directory.<field>` so a consumer can tell them apart even
    /// when both report the same field name.
    fn visit(
        &mut self,
        offset: u64,
        path: &mut Vec<&'bytes [u8]>,
        root: bool,
    ) -> Result<(), RofError> {
        // Bounded depth: one guard per open level. Reaching
        // `max_depth + 1` fails as a structural `RecursionDepthExceeded`
        // and unwinds every guard on the way out.
        let _guard = self.recursion.enter("directory", offset)?;

        // The block itself must fit at least its header inside the
        // container: an outside-file directory pointer fails before a
        // single byte of it is read.
        let block_end = offset.saturating_add(DIRECTORY_HEADER_BYTES as u64);
        if block_end > self.file.len() as u64 {
            return Err(RofError::ExtentOutOfBounds {
                container: self.container.to_owned(),
                offset,
                start: offset,
                length: DIRECTORY_HEADER_BYTES as u64,
                file_len: self.file.len() as u64,
            });
        }

        // Read the block itself. `borrow_block` works on a range that
        // starts at the block, so its failures are relative to it;
        // `in_block` shifts them onto the absolute container offset the
        // walk reports and scopes the structural ones to this block.
        let file = self.file;
        let mut reader = Reader::new(self.container, &file[offset as usize..]);
        let view = borrow_block(&mut reader)
            .map_err(|error| error.in_block(offset, if root { "" } else { "directory" }))?;

        // Bounded work: what the walk has visited may not add up to more
        // bytes than the container holds. Disjoint blocks cannot reach that
        // sum — every block is at least a header long — so a block
        // reinterpretation (a block under two parents, or blocks pointing
        // into each other's bytes) is refused here, after work linear in
        // the container and never before it.
        self.visited_bytes += view.block_len as u64;
        if self.visited_bytes > file.len() as u64 {
            return Err(RofError::UnsupportedLayout {
                container: self.container.to_owned(),
                offset,
                detail: format!(
                    "the visited directory blocks add up to more than the {}-byte \
                     container: they overlap (block at offset {offset} reuses bytes \
                     another block already owns)",
                    file.len()
                ),
            });
        }

        let container = self.container;
        let file_len = file.len() as u64;

        // This block owns `[offset, offset + block_len)`; it joins the
        // spans the overlap check compares, and its records join the
        // booking `read_tree` makes after the walk succeeded.
        self.plan
            .spans
            .push((offset, offset + view.block_len as u64));
        self.plan.total_records += u64::from(view.header.entry_count);
        self.plan.path_slices += path.len() as u64;
        self.plan.directories.push(DirectoryPlan {
            path: path.clone(),
            offset,
            view,
        });

        self.ancestors.push(offset);
        let (words, remainder) = view.records.as_chunks::<RECORD_BYTES>();
        debug_assert!(remainder.is_empty(), "borrow_block bounds the record table");
        let mut cursor = 0usize;
        for (index, word) in words.iter().enumerate() {
            let record = decode_record(word);
            let name_length = record.name_length as usize;
            // `borrow_block` proved the declared names cover the table
            // exactly and each holds at least its terminator, so this
            // range is inside the table and has a byte to drop.
            debug_assert!(
                cursor + name_length <= view.names.len(),
                "a validated name table covers every declared name"
            );
            let declared = &view.names[cursor..cursor + name_length];
            cursor += name_length;
            let name = &declared[..name_length - 1];

            let flags = record.flags;
            let record_offset =
                offset + DIRECTORY_HEADER_BYTES as u64 + index as u64 * RECORD_BYTES as u64;
            if flags.has_unknown_bits() {
                return Err(RofError::UnsupportedLayout {
                    container: container.to_owned(),
                    offset: record_offset,
                    detail: format!(
                        "entry {index} of the directory at offset {offset} declares \
                         flags {flags} with bits that have no observed meaning"
                    ),
                });
            }
            if flags.is_directory() && flags.is_compressed() {
                return Err(RofError::UnsupportedLayout {
                    container: container.to_owned(),
                    offset: record_offset,
                    detail: format!(
                        "entry {index} of the directory at offset {offset} declares \
                         flags {flags}: the directory+compressed combination has no \
                         observed meaning"
                    ),
                });
            }

            let start = u64::from(record.start);
            // A directory record's extent is the nested block, so that
            // block must fit its header before its own length words mean
            // anything; `visit` checks the same bound again for the root
            // block and as the last word before it reads anything.
            if flags.is_directory() {
                let block_end = start.saturating_add(DIRECTORY_HEADER_BYTES as u64);
                if block_end > file_len {
                    return Err(RofError::ExtentOutOfBounds {
                        container: container.to_owned(),
                        offset: start,
                        start,
                        length: DIRECTORY_HEADER_BYTES as u64,
                        file_len,
                    });
                }
            }
            // Every record declares two extents and both must lie inside
            // the container before either is offered to a reader —
            // directory records included: their length words have no
            // observed meaning and the reference extractor ignores them
            // ([S05]), so a word that reaches past the end is exactly the
            // outside-file pointer AC03 refuses, surfaced here instead of
            // being read later. `u32 + u32` widened to `u64` cannot
            // overflow.
            let length_end = start + u64::from(record.raw_length);
            let length_on_disk_end = start + u64::from(record.raw_length_on_disk);
            if length_end > file_len {
                return Err(RofError::ExtentOutOfBounds {
                    container: container.to_owned(),
                    offset: start,
                    start,
                    length: u64::from(record.raw_length),
                    file_len,
                });
            }
            if length_on_disk_end > file_len {
                return Err(RofError::ExtentOutOfBounds {
                    container: container.to_owned(),
                    offset: start,
                    start,
                    length: u64::from(record.raw_length_on_disk),
                    file_len,
                });
            }

            if flags.is_directory() {
                // Cycle detection: this block is already open above us.
                if self.ancestors.contains(&start) {
                    return Err(RofError::Cycle {
                        container: container.to_owned(),
                        offset: start,
                        depth: self.ancestors.len() as u32,
                    });
                }
                path.push(name);
                self.visit(start, path, false)?;
                path.pop();
                continue;
            }

            let mut member_path = path.clone();
            member_path.push(name);
            self.plan.path_slices += member_path.len() as u64;
            self.plan.members.push(RofMember {
                path: member_path,
                record,
                start,
                length_end,
                length_on_disk_end,
            });
            // Only non-empty spans can share bytes: an empty member
            // extracts nothing, so it cannot overlap anything.
            if length_end > start {
                self.plan.spans.push((start, length_end));
            }
        }
        self.ancestors.pop();
        Ok(())
    }
}

/// Reads one member of `file`, byte for byte for an uncompressed member and
/// through a **bounded** zlib decoder for a compressed one.
///
/// The order of operations is the spec's (non-negotiable #3): the extent
/// is established first — `start + raw_length` must lie inside the
/// container — and only then does the decoder see a single input byte. The
/// flags decide how the span may be read at all: an unobserved word is
/// refused as [`RofError::UnsupportedLayout`] instead of being guessed at
/// (non-negotiable #5), which makes this function safe to call with a
/// hand-built [`RofMember`] as well as one from [`read_tree`].
///
/// The selected read profile, stated in full because the two length words
/// are still unresolved (spec F05, non-negotiable #4):
///
/// * **stored** = `raw_length` bytes at `start`. That is the extent the
///   reference extractor reads for a compressed member ([S05]); the
///   `raw_length_on_disk` word is validated as an extent by [`read_tree`]
///   but never read here, exactly as the reference ignores it. Which word
///   is "stored" and which is "decoded" in the original stays a retail
///   question (F05-D).
/// * **decoded** = what the zlib decoder produces from those bytes, capped
///   by [`RofLimits::max_decoded_bytes`]; a stream that would exceed the
///   cap fails as [`RofError::ExpansionBomb`] before the excess is
///   appended.
/// * **trailing** = stored bytes after the end of the zlib stream, which
///   are reported in [`RofMemberRead::trailing_len`] rather than silently
///   dropped, so an extent that runs past its stream stays visible.
///
/// The decoded buffer is the caller's: it is not booked against the
/// parse's allocation budget, because that budget has no way to release a
/// charge and a mount reading many members would exhaust it through no
/// fault of its own. The per-read ceiling above is what bounds it.
///
/// # Errors
///
/// [`RofError::ExtentOutOfBounds`] when the extent is not inside the
/// container, [`RofError::UnsupportedLayout`] for flags this reader cannot
/// explain, [`RofError::ExpansionBomb`] past the limit, and
/// [`RofError::DecodeFailure`] for a truncated, corrupt or checksum-mismatched
/// stream. A failure never leaves partial output behind.
pub fn read_member(
    context: &ParseContext,
    file: &[u8],
    member: &RofMember<'_>,
    limits: &RofLimits,
) -> Result<RofMemberRead, RofError> {
    let container = context.container();
    let start = member.start;
    let stored_len = u64::from(member.record.raw_length);

    // 1. Establish the extent. `start + raw_length` is computed with
    //    checked arithmetic (a hand-built member can carry anything) and
    //    must end inside the container.
    let end = start.checked_add(stored_len).ok_or_else(|| {
        RofError::Parse(ParseError::length_overflow(
            container.to_owned(),
            start,
            "member.extent",
            "start + length to fit in u64".to_owned(),
            format!("start {start} plus length {stored_len}"),
        ))
    })?;
    if end > file.len() as u64 {
        return Err(RofError::ExtentOutOfBounds {
            container: container.to_owned(),
            offset: start,
            start,
            length: stored_len,
            file_len: file.len() as u64,
        });
    }

    // 2. Decide how the span may be read, before reading it.
    let flags = member.record.flags;
    if flags.is_directory() {
        return Err(RofError::UnsupportedLayout {
            container: container.to_owned(),
            offset: start,
            detail: format!(
                "the entry at offset {start} declares the directory flag and has no \
                 member bytes to read"
            ),
        });
    }
    if flags.has_unknown_bits() {
        return Err(RofError::UnsupportedLayout {
            container: container.to_owned(),
            offset: start,
            detail: format!(
                "the member at offset {start} declares flags {flags} with bits that \
                 have no observed meaning"
            ),
        });
    }

    let extent = &file[start as usize..end as usize];
    if flags.is_compressed() {
        return decode_zlib(container, start, extent, limits);
    }

    // 3. An uncompressed member is its own extent, byte for byte.
    if stored_len > limits.max_decoded_bytes {
        return Err(RofError::ExpansionBomb {
            container: container.to_owned(),
            offset: start,
            limit: limits.max_decoded_bytes,
            observed: stored_len,
        });
    }
    Ok(RofMemberRead {
        data: extent.to_vec(),
        stored_len,
        decoded_len: stored_len,
        trailing_len: 0,
    })
}

/// The bounded zlib decoder: stream `input` through `miniz_oxide` in
/// [`DECODE_CHUNK_BYTES`] slices, appending nothing past the ceiling.
///
/// The decoder is told the input may continue (`MZFlush::None`), so a
/// stream that ends early is a `Buf` failure rather than a silent
/// truncation, and the adler32 trailer is verified before `StreamEnd`
/// (`Data` on a mismatch). Both map to [`RofError::DecodeFailure`]: no
/// partial output ever leaves this function.
fn decode_zlib(
    container: &str,
    offset: u64,
    input: &[u8],
    limits: &RofLimits,
) -> Result<RofMemberRead, RofError> {
    let mut state = InflateState::new_boxed(DataFormat::Zlib);
    let mut data: Vec<u8> = Vec::new();
    let mut chunk = [0u8; DECODE_CHUNK_BYTES];
    let mut rest = input;
    loop {
        let result = inflate(&mut state, rest, &mut chunk, MZFlush::None);
        if result.bytes_consumed > rest.len() || result.bytes_written > chunk.len() {
            return Err(decode_failure(
                container,
                offset,
                "the decoder reported progress beyond the buffers it was given",
            ));
        }
        rest = &rest[result.bytes_consumed..];

        // Check the ceiling against the running total *before* appending,
        // so a bomb never materialises past the limit.
        let observed = data.len() as u64 + result.bytes_written as u64;
        if observed > limits.max_decoded_bytes {
            return Err(RofError::ExpansionBomb {
                container: container.to_owned(),
                offset,
                limit: limits.max_decoded_bytes,
                observed,
            });
        }
        data.extend_from_slice(&chunk[..result.bytes_written]);

        match result.status {
            Ok(MZStatus::StreamEnd) => {
                return Ok(RofMemberRead {
                    decoded_len: data.len() as u64,
                    stored_len: input.len() as u64,
                    trailing_len: rest.len() as u64,
                    data,
                });
            }
            Ok(MZStatus::Ok) => {
                // No progress with input left is a decoder that cannot
                // continue; refuse instead of spinning.
                if result.bytes_consumed == 0 && result.bytes_written == 0 {
                    return Err(decode_failure(
                        container,
                        offset,
                        "the decoder made no progress",
                    ));
                }
            }
            Ok(MZStatus::NeedDict) => {
                return Err(decode_failure(
                    container,
                    offset,
                    "the stream requires a preset dictionary this reader does not have",
                ));
            }
            Err(MZError::Buf) => {
                return Err(decode_failure(
                    container,
                    offset,
                    "the zlib stream ends before its data does",
                ));
            }
            Err(MZError::Data) => {
                return Err(decode_failure(
                    container,
                    offset,
                    "the zlib stream is invalid or its adler32 checksum does not match",
                ));
            }
            Err(_) => {
                return Err(decode_failure(
                    container,
                    offset,
                    "the zlib decoder refused the stream",
                ));
            }
        }
    }
}

/// Builds a [`RofError::DecodeFailure`] carrying the container and the
/// member's absolute offset. The detail comes from the decoder or from
/// this reader, never from payload bytes.
fn decode_failure(container: &str, offset: u64, detail: &str) -> RofError {
    RofError::DecodeFailure {
        container: container.to_owned(),
        offset,
        detail: detail.to_owned(),
    }
}
