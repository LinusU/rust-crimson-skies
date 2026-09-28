//! Raw INTERP loading-script records: observed layout, fields preserved
//! verbatim.
//!
//! Spec `specs/F07-interp-loading-script-container.md` describes the
//! container the inspected legacy parser expects ([S07],
//! `docs/research/FORMAT-NOTES.md`, "INTERP observed subset"):
//!
//! * a 12-byte header: little-endian `u32` signature `0x08971119`, `u32`
//!   version `7`, `u32` script count;
//! * `script_count` index entries of 128 bytes: a 120-byte NUL-padded name,
//!   a `u32` timestamp and a `u32` absolute script offset;
//! * at each script offset, lines of `u32 size`, `u32 argument_count` and
//!   `size` bytes of NUL-separated argument data; a `size` of zero ends the
//!   script (the terminator is that single zero word, as the independently
//!   authored `fixtures/synthetic/synthetic.interp` writes it).
//!
//! This stage (**F07-A**) defines the raw records and reads them:
//!
//! * every read runs through [`ParseContext`], so a truncated header, index
//!   entry, line or a script that runs out of bytes before its zero
//!   terminator is a [`ParseError`] carrying the absolute offset and the
//!   logical field (`interp.scripts[1].lines[0].size`), never a panic;
//! * the signature and the version are checked, because only that exact
//!   header is documented; anything else is an [`InterpError`] rather than a
//!   guessed variant;
//! * every line keeps its raw argument bytes as a borrowed slice and its
//!   `argument_count` word verbatim; [`InterpRawLine::raw_arguments`] is a
//!   lossless view of the NUL boundaries (spec non-negotiable #1: joining
//!   the arguments into one string can destroy their boundaries);
//! * every script keeps its origin — index position, index-entry offset and
//!   script offset — so two scripts with equal names stay distinct;
//! * the timestamp is exposed as [`InterpRawIndexEntry::raw_timestamp`]
//!   and nothing more: it is metadata, never an identity (non-negotiable
//!   #5).
//!
//! What is deliberately **not** here (F07-B): checking `argument_count`
//! against the NUL count, rejecting argument data without a final NUL,
//! requiring a script to terminate before the next script or index region,
//! name-field padding and encoding checks, and reporting trailing or
//! unreferenced regions as findings. Command meaning and the loading plan
//! are F07-C/F07-D. Names and arguments are bytes, not `str`: no encoding is
//! established for them.
//!
//! Every fixture exercised below is newly authored synthetic bytes; nothing
//! here is derived from original game data.
//!
//! ```
//! use cs_formats::{read_interp, ParseContext};
//!
//! // Header, one index entry, then one script with a single two-argument line.
//! let mut bytes = Vec::new();
//! for word in [0x0897_1119u32, 7, 1] {
//!     bytes.extend_from_slice(&word.to_le_bytes());
//! }
//! let mut name = [0u8; 120];
//! name[..4].copy_from_slice(b"demo");
//! bytes.extend_from_slice(&name);
//! bytes.extend_from_slice(&0u32.to_le_bytes()); // timestamp
//! bytes.extend_from_slice(&140u32.to_le_bytes()); // script offset
//! bytes.extend_from_slice(&6u32.to_le_bytes()); // line size
//! bytes.extend_from_slice(&2u32.to_le_bytes()); // argument count
//! bytes.extend_from_slice(b"ab\0cd\0");
//! bytes.extend_from_slice(&0u32.to_le_bytes()); // terminator
//!
//! let mut context = ParseContext::with_defaults("synthetic/interp_doc.interp");
//! let file = read_interp(&mut context, &bytes).expect("the container is valid");
//! let script = &file.scripts()[0];
//! assert_eq!(script.entry.name_bytes(), b"demo");
//! let arguments: Vec<&[u8]> = script.lines[0].raw_arguments().map(|a| a.bytes).collect();
//! assert_eq!(arguments, [b"ab".as_slice(), b"cd".as_slice()]);
//! ```

use std::fmt;
use std::mem::size_of;

use crate::error::ParseError;
use crate::io::{AllocationBudget, ParseContext, Reader};
use crate::zbd::{INTERP_SIGNATURE, INTERP_VERSION};

/// Error scope stamped onto failures raised inside [`read_interp`].
pub const INTERP_ENTRYPOINT: &str = "interp";

/// Bytes in the container header: signature, version, script count.
pub const INTERP_HEADER_BYTES: usize = 12;

/// Bytes in one index entry: name field, timestamp, script offset.
pub const INDEX_ENTRY_BYTES: usize = 128;

/// Bytes in the NUL-padded name field of an index entry.
pub const NAME_FIELD_BYTES: usize = 120;

/// Bytes in a line header: `size` then `argument_count`.
pub const LINE_HEADER_BYTES: usize = 8;

/// Bytes in the zero `size` word that ends a script.
pub const TERMINATOR_BYTES: usize = 4;

/// The three little-endian `u32` fields the container starts with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterpRawHeader {
    /// Signature word; [`read_interp`] only accepts [`INTERP_SIGNATURE`].
    pub signature: u32,
    /// Version word; [`read_interp`] only accepts [`INTERP_VERSION`].
    pub version: u32,
    /// Number of 128-byte index entries that follow the header.
    pub script_count: u32,
}

/// One 128-byte index entry, fields in on-disk order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterpRawIndexEntry<'a> {
    /// Position of the entry in the index, `0..script_count`.
    pub index: usize,
    /// Absolute offset of the entry inside the container.
    pub entry_offset: u64,
    /// The whole 120-byte name field, padding included, borrowed verbatim.
    pub name_field: &'a [u8],
    /// Timestamp word, verbatim. Metadata only: never an identity or cache
    /// key (spec F07, non-negotiable #5).
    pub raw_timestamp: u32,
    /// Absolute offset of the script's first line.
    pub script_offset: u32,
}

impl<'a> InterpRawIndexEntry<'a> {
    /// The name: the name field up to its first `0x00`, or the whole field
    /// when it holds none (whether that is valid is F07-B's decision).
    pub fn name_bytes(&self) -> &'a [u8] {
        let field = self.name_field;
        let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
        &field[..end]
    }
}

/// One script line exactly as stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterpRawLine<'a> {
    /// Absolute offset of the line's `size` word.
    pub offset: u64,
    /// The `size` word: byte length of [`Self::data`], never zero (a zero
    /// `size` is the script terminator, not a line).
    pub size: u32,
    /// The `argument_count` word, verbatim. Not yet checked against the
    /// data (F07-B).
    pub argument_count: u32,
    /// The `size` bytes of argument data, borrowed verbatim.
    pub data: &'a [u8],
}

impl<'a> InterpRawLine<'a> {
    /// Absolute offset of the first data byte.
    pub fn data_offset(&self) -> u64 {
        self.offset + LINE_HEADER_BYTES as u64
    }

    /// The argument data split at every `0x00`, losslessly.
    ///
    /// Each yielded [`RawArgument`] is the bytes before one NUL, with its
    /// absolute offset. Bytes after the last NUL, if any, are yielded as a
    /// final argument with `terminated == false`, so concatenating every
    /// argument's bytes (plus a NUL for each terminated one) reproduces
    /// [`Self::data`] exactly. Nothing is trimmed, decoded or joined.
    pub fn raw_arguments(&self) -> RawArguments<'a> {
        RawArguments {
            rest: self.data,
            offset: self.data_offset(),
        }
    }
}

/// One NUL-delimited argument of a line, borrowed from the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawArgument<'a> {
    /// Absolute offset of the argument's first byte.
    pub offset: u64,
    /// The argument's bytes, without the delimiting NUL.
    pub bytes: &'a [u8],
    /// Whether a NUL followed the bytes inside the line's data.
    pub terminated: bool,
}

/// Iterator over a line's [`RawArgument`]s, in data order.
#[derive(Clone, Debug)]
pub struct RawArguments<'a> {
    rest: &'a [u8],
    offset: u64,
}

impl<'a> Iterator for RawArguments<'a> {
    type Item = RawArgument<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        let offset = self.offset;
        let rest = self.rest;
        let argument = match rest.iter().position(|&b| b == 0) {
            Some(end) => {
                self.rest = &rest[end + 1..];
                self.offset += end as u64 + 1;
                RawArgument {
                    offset,
                    bytes: &rest[..end],
                    terminated: true,
                }
            }
            None => {
                self.rest = &[];
                self.offset += rest.len() as u64;
                RawArgument {
                    offset,
                    bytes: rest,
                    terminated: false,
                }
            }
        };
        Some(argument)
    }
}

/// One script: its index entry (the origin) and its lines up to the zero
/// terminator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterpRawScript<'a> {
    /// The index entry that points at this script.
    pub entry: InterpRawIndexEntry<'a>,
    /// Lines in stored order, terminator excluded.
    pub lines: Vec<InterpRawLine<'a>>,
    /// Absolute offset of the zero `size` word that ended the script.
    pub terminator_offset: u64,
}

impl InterpRawScript<'_> {
    /// Absolute offset just past the terminator: the script occupies
    /// `entry.script_offset..end()`.
    pub fn end(&self) -> u64 {
        self.terminator_offset + TERMINATOR_BYTES as u64
    }
}

/// A read INTERP container: header and every indexed script, in index
/// order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterpFile<'a> {
    header: InterpRawHeader,
    scripts: Vec<InterpRawScript<'a>>,
}

impl<'a> InterpFile<'a> {
    /// The container header.
    pub fn header(&self) -> InterpRawHeader {
        self.header
    }

    /// Every script, in index order.
    pub fn scripts(&self) -> &[InterpRawScript<'a>] {
        &self.scripts
    }

    /// Absolute offset just past the index table.
    pub fn index_end(&self) -> u64 {
        (INTERP_HEADER_BYTES + self.scripts.len() * INDEX_ENTRY_BYTES) as u64
    }
}

/// Why an INTERP container was rejected.
///
/// Every variant names the container and an absolute offset, plus a
/// machine-matchable [`Self::code`]. Payload bytes never appear in an error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InterpError {
    /// A structural failure from the checked reader (truncation, a script
    /// offset past the end, no terminator before the end, a budget refusal),
    /// scoped as `interp.<field>`.
    Parse(ParseError),
    /// The signature word is not [`INTERP_SIGNATURE`].
    Signature {
        /// Container label the bytes came from.
        container: String,
        /// Offset of the signature word.
        offset: u64,
        /// The signature word found.
        observed: u32,
    },
    /// The version word is not [`INTERP_VERSION`]: no other version's
    /// layout is documented.
    Version {
        /// Container label the bytes came from.
        container: String,
        /// Offset of the version word.
        offset: u64,
        /// The version word found.
        observed: u32,
    },
}

impl InterpError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Parse(_) => "parse",
            Self::Signature { .. } => "signature",
            Self::Version { .. } => "version",
        }
    }

    /// The container label the bytes came from.
    pub fn container(&self) -> &str {
        match self {
            Self::Parse(error) => &error.container,
            Self::Signature { container, .. } | Self::Version { container, .. } => container,
        }
    }

    /// Absolute offset of the problem.
    pub fn offset(&self) -> u64 {
        match self {
            Self::Parse(error) => error.offset,
            Self::Signature { offset, .. } | Self::Version { offset, .. } => *offset,
        }
    }
}

impl From<ParseError> for InterpError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl fmt::Display for InterpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "{error}"),
            Self::Signature {
                container,
                offset,
                observed,
            } => write!(
                f,
                "signature at offset {offset} in {container}: expected \
                 0x{INTERP_SIGNATURE:08x}, found 0x{observed:08x}"
            ),
            Self::Version {
                container,
                offset,
                observed,
            } => write!(
                f,
                "version at offset {offset} in {container}: expected \
                 {INTERP_VERSION}, found {observed}"
            ),
        }
    }
}

impl std::error::Error for InterpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            _ => None,
        }
    }
}

/// Reads a whole INTERP container.
///
/// The header is checked first; then the index table is borrowed with
/// checked arithmetic (so a hostile `script_count` costs a refusal, never a
/// buffer) and each script is read from its absolute offset until its zero
/// terminator. The decoded script and line tables are booked against the
/// parse's allocation budget before they grow, and a failed attempt leaves
/// the ledger as it found it ([`ParseContext::parse`]).
///
/// # Errors
///
/// [`InterpError::Signature`] / [`InterpError::Version`] for an
/// undocumented header, [`InterpError::Parse`] for truncation, a script
/// offset past the end, a script without a terminator before the end of the
/// bytes, or a table beyond the allocation budget.
pub fn read_interp<'bytes>(
    context: &mut ParseContext,
    bytes: &'bytes [u8],
) -> Result<InterpFile<'bytes>, InterpError> {
    context.parse(
        INTERP_ENTRYPOINT,
        bytes,
        |reader, allocation, _recursion| {
            // 1. Header: only the documented signature and version are read on.
            let signature_offset = reader.position();
            let signature = reader.read_u32("header.signature")?;
            if signature != INTERP_SIGNATURE {
                return Ok(Err(InterpError::Signature {
                    container: reader.container().to_owned(),
                    offset: signature_offset,
                    observed: signature,
                }));
            }
            let version_offset = reader.position();
            let version = reader.read_u32("header.version")?;
            if version != INTERP_VERSION {
                return Ok(Err(InterpError::Version {
                    container: reader.container().to_owned(),
                    offset: version_offset,
                    observed: version,
                }));
            }
            let script_count = reader.read_u32("header.script_count")?;
            let header = InterpRawHeader {
                signature,
                version,
                script_count,
            };

            // 2. Bounds-checked borrow of the index table before any allocation.
            let index_offset = reader.position();
            let index_len = reader.checked_byte_len(
                "index",
                u64::from(script_count),
                INDEX_ENTRY_BYTES as u64,
            )?;
            let index = reader.read_bytes("index", index_len)?;
            allocation.reserve(
                "scripts",
                index_offset,
                u64::from(script_count),
                size_of::<InterpRawScript<'_>>() as u64,
            )?;
            let mut scripts = Vec::with_capacity(script_count as usize);

            // 3. Each script from its own absolute offset, line by line.
            let (entries, remainder) = index.as_chunks::<INDEX_ENTRY_BYTES>();
            debug_assert!(
                remainder.is_empty(),
                "checked_byte_len produced whole entries"
            );
            for (position, raw) in entries.iter().enumerate() {
                let entry_offset = index_offset + (position * INDEX_ENTRY_BYTES) as u64;
                let word = |at: usize| {
                    let mut word = [0u8; 4];
                    word.copy_from_slice(&raw[at..at + 4]);
                    u32::from_le_bytes(word)
                };
                let entry = InterpRawIndexEntry {
                    index: position,
                    entry_offset,
                    name_field: &raw[..NAME_FIELD_BYTES],
                    raw_timestamp: word(NAME_FIELD_BYTES),
                    script_offset: word(NAME_FIELD_BYTES + 4),
                };
                scripts.push(read_script(reader.container(), bytes, entry, allocation)?);
            }

            Ok(Ok(InterpFile { header, scripts }))
        },
    )?
}

/// Reads the lines of one script from `entry.script_offset` up to and
/// including its zero terminator.
fn read_script<'bytes>(
    container: &str,
    bytes: &'bytes [u8],
    entry: InterpRawIndexEntry<'bytes>,
    allocation: &mut AllocationBudget,
) -> Result<InterpRawScript<'bytes>, ParseError> {
    let index = entry.index;
    let mut reader = Reader::new(container, bytes);
    reader.skip(
        &format!("scripts[{index}].offset"),
        entry.script_offset as usize,
    )?;
    let mut lines = Vec::new();
    loop {
        let offset = reader.position();
        let line = lines.len();
        let size = reader.read_u32(&format!("scripts[{index}].lines[{line}].size"))?;
        if size == 0 {
            return Ok(InterpRawScript {
                entry,
                lines,
                terminator_offset: offset,
            });
        }
        let argument_count =
            reader.read_u32(&format!("scripts[{index}].lines[{line}].argument_count"))?;
        let data = reader.read_bytes(
            &format!("scripts[{index}].lines[{line}].data"),
            size as usize,
        )?;
        allocation.reserve(
            &format!("scripts[{index}].lines"),
            offset,
            1,
            size_of::<InterpRawLine<'_>>() as u64,
        )?;
        lines.push(InterpRawLine {
            offset,
            size,
            argument_count,
            data,
        });
    }
}
