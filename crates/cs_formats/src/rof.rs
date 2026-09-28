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
//! This stage (**F05-A**) defines the raw structs and validates *one* block:
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
//!   preserved fields `raw_length` / `raw_length_on_disk`);
//! * `flags` is kept as raw bits: `FLAG_DIRECTORY` / `FLAG_COMPRESSED` are
//!   the two observed bits ([S05]), anything else survives untouched so a
//!   later stage can surface `UnsupportedLayout` *before* it extracts a span
//!   (spec non-negotiable #5). This reader never extracts a byte range.
//!
//! What is deliberately **not** here: following a directory record's
//! `start` (cycle detection and bounded depth are F05-B), extents checked
//! against the file length, zlib decoding (F05-B), mounting into the VFS and
//! inspection (F05-C) and the compressed-length semantics (F05-D, retail).
//! Names are returned as bytes, not `str`: locales are not guaranteed to be
//! UTF-8 (spec non-negotiable #2).
//!
//! A *root* directory is the bytes from offset zero of the file; a nested
//! directory is the bytes from its record's `start`. This stage only parses
//! the block it is handed, so error offsets are relative to that byte range.
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

use std::fmt;
use std::slice;

use crate::error::ParseError;
use crate::io::ParseContext;

/// Error scope stamped onto failures raised inside [`read_directory`].
///
/// A structural failure leaves as `rof.directory.<field>`; the name-table
/// checks carry their own field name and offset instead (see [`RofError`]).
pub const DIRECTORY_ENTRYPOINT: &str = "rof.directory";

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
/// non-negotiable #5). This stage never rejects a record for its flags — it
/// extracts nothing.
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
/// Likewise `start` and the lengths are recorded, never bounds-checked:
/// a nested block or member extent can only be validated against the whole
/// file, which is F05-B.
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
        }
    }

    /// The container label the bytes came from.
    pub fn container(&self) -> &str {
        match self {
            Self::Parse(error) => &error.container,
            Self::NameTableLength { container, .. }
            | Self::EmptyName { container, .. }
            | Self::UnterminatedName { container, .. }
            | Self::InteriorNul { container, .. } => container,
        }
    }

    /// Offset of the problem inside the range that was handed to
    /// [`read_directory`] (a root directory starts at the file's offset
    /// zero, so a root failure offset *is* the file offset).
    pub fn offset(&self) -> u64 {
        match self {
            Self::Parse(error) => error.offset,
            Self::NameTableLength { offset, .. }
            | Self::EmptyName { offset, .. }
            | Self::UnterminatedName { offset, .. }
            | Self::InteriorNul { offset, .. } => *offset,
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
            // 1. Header: the two declared lengths, checked little-endian reads.
            let header_offset = reader.position();
            let entry_count = reader.read_u32("header.entry_count")?;
            let names_length = reader.read_u32("header.names_length")?;
            let header = RofRawHeader {
                entry_count,
                names_length,
            };

            // 2. Bounds-checked borrows of the two tables. `checked_byte_len`
            //    does the `count * 24` / `names_length` arithmetic before any
            //    slice, and `read_bytes` only hands out a borrow: a hostile
            //    count costs a refusal, never a buffer.
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

            // 3. Validate the name table against the records while nothing is
            //    charged yet: a block the tables disagree about is rejected
            //    with a name-table error and the ledger stays untouched.
            if let Err(error) =
                validate_names(reader.container(), header, records, names_start, names)
            {
                return Ok(Err(error));
            }

            // 4. Book the decoded record table against this parse's allocation
            //    budget, then build it. Only now does the parse hold memory.
            allocation.reserve(
                "records",
                header_offset,
                u64::from(entry_count),
                RECORD_BYTES as u64,
            )?;
            let (words, remainder) = records.as_chunks::<RECORD_BYTES>();
            debug_assert!(
                remainder.is_empty(),
                "checked_byte_len produced whole records"
            );
            let table = words.iter().map(|record| decode_record(record)).collect();

            Ok(Ok(RofDirectory {
                header,
                records: table,
                names,
                block_len,
            }))
        },
    )?
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
