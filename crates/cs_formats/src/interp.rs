//! Raw INTERP loading-script records and their validated, lossless token
//! decoding.
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
//! **F07-A** defines the raw records and reads them with [`read_interp`]:
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
//! **F07-B** adds [`decode_interp`], which walks the same bytes and turns
//! them into a [`DecodedInterp`] of lossless [`InterpToken`]s:
//!
//! * every line's `argument_count` must equal the `0x00` delimiters actually
//!   stored in its data, and the data must end with the delimiter that closes
//!   its last argument — a tail without one would force the decoder to invent
//!   a boundary or drop bytes (spec non-negotiable #1, AC02);
//! * each script's extent is bounded by the next script's offset (or the end
//!   of the container), so a script must reach its zero `size` terminator
//!   inside its own range and can never swallow its neighbour; a script
//!   offset inside the header or index table is refused (non-negotiable #2);
//! * the extents are computed from the index, not from the order the entries
//!   happen to be in, so an index that is not sorted by offset still decodes;
//! * the whole decode runs in one attempt in three phases — walk and
//!   validate (nothing booked), book every decoded record in one
//!   reservation, then build — so a rejected container leaves the parse's
//!   allocation ledger exactly as it found it (F03-C's teardown/retry
//!   contract);
//! * what this stage cannot measure is kept instead of guessed: bytes no
//!   script claims, two entries pointing at one offset, a name field with no
//!   `0x00` and non-zero name padding are reported as [`InterpFinding`]s by
//!   [`DecodedInterp::findings`], never as a failure and never silently
//!   dropped.
//!
//! **F07-C** adds [`plan_interp_loading`], which turns a validated container
//! into a loading plan *without touching a filesystem*:
//!
//! * [`LoadCommandTable`] is the registry of commands somebody has classified
//!   as resource-loading, each with the argument positions that spell an asset
//!   key, a [`ClaimStatus`] and where the classification came from. It is
//!   **empty by default and this stage ships no entries**: no retail evidence
//!   exists yet (F07-D measures it), so an unclassified head token is not
//!   quietly treated as a loading command and not quietly treated as a
//!   non-loading one either;
//! * every line of every script becomes a [`PlanLine`], classified as a
//!   registered loading command with the tokens its key is spelled by
//!   ([`PlanLineKind::Loading`]), a registered command whose stored arguments
//!   do not match its registered domain ([`PlanLineKind::Malformed`]) or
//!   unclassified ([`PlanLineKind::Unclassified`]);
//! * each script keeps its [`ScriptOrigin`] — index position, index-entry
//!   offset, script offset, terminator and end — so two scripts with equal
//!   names never collapse into one plan entry, and the `timestamp` word is
//!   carried as `raw_timestamp` metadata only (non-negotiable #5: content
//!   hashes, computed by the consumer that owns the hashing, determine
//!   identity; nothing here keys a cache on a name or a timestamp);
//! * [`PlanStats`] counts raw records, lines, classified commands,
//!   malformed commands, unclassified commands and distinct heads, which is
//!   the "count raw records, decoded instructions, unknown instructions"
//!   tally `docs/contracts/SCRIPT-MISSION.md` asks a source adapter for.
//!
//! Resolving a plan against a real installation — asset keys, the VFS,
//! dependency spans and the session lifecycle — is the consumer's job
//! (`cs_content::loading`), because this crate must not depend on the VFS.
//! Which commands load resources and which refer to game behaviour is F07-D's
//! measurement and is **not** decided here: names and arguments are bytes, not
//! `str`, so no encoding is established for them, the "encoding" validation
//! `docs/research/FORMAT-NOTES.md` asks for stays unknown and no [`String`] is
//! built from a container's own bytes.
//!
//! **F07-D** adds the classification and the audit over it, still without
//! deciding anything itself:
//!
//! * [`OpcodeClassTable`] holds one [`ClassifiedOpcode`] per head spelling:
//!   [`OpcodeClass::Loading`] with the argument positions that spell an asset
//!   key, [`OpcodeClass::Unsupported`] for a command recognized as loading but
//!   with no supported key domain (a directory or a `%VARIABLE%` spelling), or
//!   [`OpcodeClass::Behavior`] for a command that is not a resource load. Every
//!   entry carries a [`ClaimStatus`] and a `source`, and the table refuses
//!   duplicates and self-awarded statuses, exactly as [`LoadCommandTable`] does;
//! * [`audit_interp_opcodes`] walks the same lines and reports **every distinct
//!   head** with its occurrence count and class, or as unknown when nothing
//!   classified it. [`OpcodeAudit::is_complete`] is the "classify every opcode"
//!   property: it is true only when no head is unknown;
//! * [`plan_interp_loading_classified`] turns the classification into a plan —
//!   a `Behavior` line contributes no dependency and does not block its script,
//!   an `Unsupported` line blocks with its source offset, and an unknown head is
//!   still [`PlanLineKind::Unclassified`]. [`plan_interp_loading`] keeps the
//!   F07-C entry point and delegates with every registration classified
//!   `Loading`, so its behaviour is unchanged.
//!
//! The empty table remains the shipped state: a classification of an installed
//! corpus is evidence somebody measured, and this crate still builds none. The
//! retail audit that supplies one is `docs/findings/`.
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
//!
//! The validating decoder ([`decode_interp`], stage F07-B) reads the same
//! bytes and hands out the tokens with their absolute offsets:
//!
//! ```
//! use cs_formats::{ParseContext, decode_interp};
//!
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
//! let decoded = decode_interp(&mut context, &bytes).expect("the container validates");
//! let line = &decoded.script(0).expect("one script").line(0).expect("one line");
//! let tokens: Vec<(u64, &[u8])> = line.tokens().iter().map(|t| (t.offset(), t.bytes())).collect();
//! assert_eq!(tokens, [(148, b"ab".as_slice()), (151, b"cd".as_slice())]);
//! assert_eq!(line.len(), line.argument_count() as usize);
//! assert!(decoded.findings().is_empty());
//! ```
//!
//! The loading plan (stage F07-C) classifies the same lines. No command is
//! registered by default, so this line is unclassified and the plan says so
//! with its offset rather than pretending it named an asset:
//!
//! ```
//! use cs_formats::{LoadCommandTable, ParseContext, decode_interp, plan_interp_loading};
//!
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
//! let table = LoadCommandTable::new(); // nothing is classified yet
//! let mut context = ParseContext::with_defaults("synthetic/interp_doc.interp");
//! let decoded = decode_interp(&mut context, &bytes).expect("the container validates");
//! let plan = plan_interp_loading(&decoded, &table);
//! assert_eq!(plan.stats().unclassified_commands, 1);
//! assert!(!plan.is_complete());
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::mem::size_of;

use crate::error::ParseError;
use crate::io::{AllocationBudget, ParseContext, Reader};
use crate::zbd::{INTERP_SIGNATURE, INTERP_VERSION};
use cs_types::evidence::ClaimStatus;

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
    /// A script's `script_offset` points inside the header or the index
    /// table, so it cannot start a script: the two regions would overlap
    /// (spec F07 non-negotiable #2, index and script extents are validated
    /// independently).
    ScriptOffset {
        /// Container label the bytes came from.
        container: String,
        /// Index position of the offending entry.
        index: usize,
        /// The `script_offset` word found.
        offset: u32,
        /// Absolute offset just past the index table: the first byte a
        /// script may occupy.
        index_end: u64,
    },
    /// A script reached the end of its own extent without a zero `size`
    /// word. Its neighbour's bytes were refused rather than decoded as
    /// this script's lines.
    Unterminated {
        /// Container label the bytes came from.
        container: String,
        /// Index position of the offending script.
        index: usize,
        /// Offset the missing terminator would have occupied.
        offset: u64,
        /// Exclusive end of the script's extent.
        limit: u64,
    },
    /// A line's declared data crosses the end of its script's extent.
    LineOverrun {
        /// Container label the bytes came from.
        container: String,
        /// Index position of the script the line belongs to.
        script: usize,
        /// Position of the line in its script.
        line: usize,
        /// Offset of the line's `size` word.
        offset: u64,
        /// The `size` word found.
        size: u32,
        /// Exclusive end of the script's extent.
        limit: u64,
    },
    /// A line's `argument_count` does not equal the number of `0x00`
    /// delimiters stored in its data. The observed behaviour is a NUL count
    /// matching the argument count; a file where the two disagree is
    /// refused, not guessed at (spec F07 non-negotiable #1, AC02).
    ArgumentCount {
        /// Container label the bytes came from.
        container: String,
        /// Index position of the script the line belongs to.
        script: usize,
        /// Position of the line in its script.
        line: usize,
        /// Offset of the first data byte.
        offset: u64,
        /// The `argument_count` word found.
        declared: u32,
        /// Number of `0x00` bytes actually present in the data.
        found: u32,
    },
    /// A line's data ends without the `0x00` that closes its last
    /// argument, so the last argument's end is not stored anywhere in the
    /// file. Decoding it would have to invent that boundary.
    UnterminatedArguments {
        /// Container label the bytes came from.
        container: String,
        /// Index position of the script the line belongs to.
        script: usize,
        /// Position of the line in its script.
        line: usize,
        /// Offset just past the line's last data byte.
        offset: u64,
    },
}

impl InterpError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Parse(_) => "parse",
            Self::Signature { .. } => "signature",
            Self::Version { .. } => "version",
            Self::ScriptOffset { .. } => "script_offset",
            Self::Unterminated { .. } => "unterminated",
            Self::LineOverrun { .. } => "line_overrun",
            Self::ArgumentCount { .. } => "argument_count",
            Self::UnterminatedArguments { .. } => "unterminated_arguments",
        }
    }

    /// The container label the bytes came from.
    pub fn container(&self) -> &str {
        match self {
            Self::Parse(error) => &error.container,
            Self::Signature { container, .. }
            | Self::Version { container, .. }
            | Self::ScriptOffset { container, .. }
            | Self::Unterminated { container, .. }
            | Self::LineOverrun { container, .. }
            | Self::ArgumentCount { container, .. }
            | Self::UnterminatedArguments { container, .. } => container,
        }
    }

    /// Absolute offset of the problem.
    ///
    /// [`InterpError::ScriptOffset`] reports the `script_offset` word as the
    /// container stores it, widened to `u64`; every other variant reports the
    /// offset of the field or the byte position the failure is about.
    pub fn offset(&self) -> u64 {
        match self {
            Self::Parse(error) => error.offset,
            Self::ScriptOffset { offset, .. } => u64::from(*offset),
            Self::Signature { offset, .. }
            | Self::Version { offset, .. }
            | Self::Unterminated { offset, .. }
            | Self::LineOverrun { offset, .. }
            | Self::ArgumentCount { offset, .. }
            | Self::UnterminatedArguments { offset, .. } => *offset,
        }
    }

    /// Index position of the script the failure is about, when the failure
    /// is about one script.
    pub fn script(&self) -> Option<usize> {
        match self {
            Self::ScriptOffset { index, .. } | Self::Unterminated { index, .. } => Some(*index),
            Self::LineOverrun { script, .. }
            | Self::ArgumentCount { script, .. }
            | Self::UnterminatedArguments { script, .. } => Some(*script),
            Self::Parse(_) | Self::Signature { .. } | Self::Version { .. } => None,
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
            Self::ScriptOffset {
                container,
                index,
                offset,
                index_end,
            } => write!(
                f,
                "script offset {offset} of index entry {index} in {container} lies inside \
                 the header and index table, which end at {index_end}"
            ),
            Self::Unterminated {
                container,
                index,
                offset,
                limit,
            } => write!(
                f,
                "script {index} in {container} has no zero size word between its start and \
                 offset {limit}: its extent is unterminated at offset {offset}"
            ),
            Self::LineOverrun {
                container,
                script,
                line,
                offset,
                size,
                limit,
            } => write!(
                f,
                "line {line} of script {script} in {container} declares {size} bytes at \
                 offset {offset}, past the script's extent ending at {limit}"
            ),
            Self::ArgumentCount {
                container,
                script,
                line,
                offset,
                declared,
                found,
            } => write!(
                f,
                "line {line} of script {script} in {container} declares {declared} \
                 arguments but its data at offset {offset} holds {found} 0x00 delimiters"
            ),
            Self::UnterminatedArguments {
                container,
                script,
                line,
                offset,
            } => write!(
                f,
                "line {line} of script {script} in {container} does not end with a 0x00 \
                 delimiter: the last argument has no stored end at offset {offset}"
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
            // 1. Header and index table: the same shared read the validating
            //    decoder starts from, so both entrypoints walk one layout.
            let (header, index) = match read_header_and_index(reader) {
                Ok(parts) => parts,
                Err(InterpError::Parse(error)) => return Err(error),
                Err(domain) => return Ok(Err(domain)),
            };

            // 2. Book the script table before it grows.
            let index_offset = INTERP_HEADER_BYTES as u64;
            allocation.reserve(
                "scripts",
                index_offset,
                u64::from(header.script_count),
                size_of::<InterpRawScript<'_>>() as u64,
            )?;
            let mut scripts = Vec::with_capacity(header.script_count as usize);

            // 3. Each script from its own absolute offset, line by line.
            for entry in index_entries(index_offset, index) {
                scripts.push(read_script(reader.container(), bytes, entry, allocation)?);
            }

            Ok(Ok(InterpFile { header, scripts }))
        },
    )?
}

/// Reads the header and borrows the whole index table.
///
/// Only the documented signature and version are read on; both are
/// [`InterpError`]s rather than guessed variants. The index length is computed
/// with checked arithmetic and borrowed, so a hostile `script_count` costs a
/// refusal and never a buffer. Returns the header and the index bytes, which
/// are a sub-slice of the input.
fn read_header_and_index<'bytes>(
    reader: &mut Reader<'bytes>,
) -> Result<(InterpRawHeader, &'bytes [u8]), InterpError> {
    let signature_offset = reader.position();
    let signature = reader.read_u32("header.signature")?;
    if signature != INTERP_SIGNATURE {
        return Err(InterpError::Signature {
            container: reader.container().to_owned(),
            offset: signature_offset,
            observed: signature,
        });
    }
    let version_offset = reader.position();
    let version = reader.read_u32("header.version")?;
    if version != INTERP_VERSION {
        return Err(InterpError::Version {
            container: reader.container().to_owned(),
            offset: version_offset,
            observed: version,
        });
    }
    let script_count = reader.read_u32("header.script_count")?;
    let header = InterpRawHeader {
        signature,
        version,
        script_count,
    };
    let index_len =
        reader.checked_byte_len("index", u64::from(script_count), INDEX_ENTRY_BYTES as u64)?;
    let index = reader.read_bytes("index", index_len)?;
    Ok((header, index))
}

/// The index entries of an already bounds-checked index table, in index order.
fn index_entries<'bytes>(
    index_offset: u64,
    index: &'bytes [u8],
) -> impl Iterator<Item = InterpRawIndexEntry<'bytes>> {
    let (entries, remainder) = index.as_chunks::<INDEX_ENTRY_BYTES>();
    debug_assert!(
        remainder.is_empty(),
        "checked_byte_len produced whole entries"
    );
    entries.iter().enumerate().map(move |(position, raw)| {
        let word = |at: usize| {
            let mut word = [0u8; 4];
            word.copy_from_slice(&raw[at..at + 4]);
            u32::from_le_bytes(word)
        };
        InterpRawIndexEntry {
            index: position,
            entry_offset: index_offset + (position * INDEX_ENTRY_BYTES) as u64,
            name_field: &raw[..NAME_FIELD_BYTES],
            raw_timestamp: word(NAME_FIELD_BYTES),
            script_offset: word(NAME_FIELD_BYTES + 4),
        }
    })
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

// ---------------------------------------------------------------------------
// Stage F07-B: lossless token decoding and validation
// ---------------------------------------------------------------------------

/// One argument of a validated line: the stored bytes with the absolute
/// offset they occupy in the container.
///
/// A token is a *view*, never a copy and never a `String`. Its bytes are the
/// bytes between two stored `0x00` delimiters (or the last one and the end of
/// the line's data, which [`decode_interp`] guarantees is a delimiter), so
/// two arguments that a joined rendering would have made indistinguishable —
/// `"a b","c"` and `"a","b c"` — stay distinct here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterpToken<'a> {
    offset: u64,
    bytes: &'a [u8],
}

impl<'a> InterpToken<'a> {
    /// Absolute offset of the token's first byte.
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// The argument's bytes, borrowed verbatim from the container.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Length in bytes; an empty argument is a real argument of length 0.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether the argument holds no bytes.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// One validated line: the raw record plus the tokens decoded from it.
///
/// The line keeps both views, so a caller that needs the stored bytes
/// verbatim has them ([`Self::raw`]) and a caller that needs the arguments has
/// them ([`Self::tokens`]) without either losing the other. Re-joining the
/// tokens with one `0x00` each reproduces [`Self::data`] exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterpLine<'a> {
    raw: InterpRawLine<'a>,
    tokens: Vec<InterpToken<'a>>,
}

impl<'a> InterpLine<'a> {
    /// The line exactly as stored, terminator excluded.
    pub fn raw(&self) -> InterpRawLine<'a> {
        self.raw
    }

    /// Absolute offset of the line's `size` word.
    pub fn offset(&self) -> u64 {
        self.raw.offset
    }

    /// Absolute offset of the line's first data byte.
    pub fn data_offset(&self) -> u64 {
        self.raw.data_offset()
    }

    /// The `size` word: byte length of [`Self::data`], never zero.
    pub fn size(&self) -> u32 {
        self.raw.size
    }

    /// The `argument_count` word, which validation has matched against the
    /// stored delimiters.
    pub fn argument_count(&self) -> u32 {
        self.raw.argument_count
    }

    /// The `size` bytes of argument data, borrowed verbatim.
    pub fn data(&self) -> &'a [u8] {
        self.raw.data
    }

    /// The decoded arguments, in data order.
    pub fn tokens(&self) -> &[InterpToken<'a>] {
        &self.tokens
    }

    /// The first argument: where a command name would be read, kept as bytes.
    ///
    /// This stage does not decide whether the first argument is a command, a
    /// verb or an ordinary value; that classification is F07-C/F07-D.
    pub fn head(&self) -> Option<InterpToken<'a>> {
        self.tokens.first().copied()
    }

    /// Number of decoded arguments, which equals [`Self::argument_count`].
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// Whether the line holds no arguments. A line always has at least one:
    /// a zero `size` is a terminator, not a line.
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }
}

/// One validated script: its index entry (the origin), its bounded extent and
/// its lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterpScript<'a> {
    entry: InterpRawIndexEntry<'a>,
    name: &'a [u8],
    limit: u64,
    terminator_offset: u64,
    lines: Vec<InterpLine<'a>>,
}

impl<'a> InterpScript<'a> {
    /// The index entry that points at this script.
    pub fn entry(&self) -> InterpRawIndexEntry<'a> {
        self.entry
    }

    /// The name bytes: the name field up to its first `0x00`, or the whole
    /// field when it holds none.
    pub fn name(&self) -> &'a [u8] {
        self.name
    }

    /// Exclusive end of the extent this script was validated against: the
    /// next script's offset, or the end of the container for the last one.
    ///
    /// A script that reaches this offset without a zero `size` word is
    /// refused, so this value is also the reason a script cannot decode its
    /// neighbour's bytes as its own lines.
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// Absolute offset of the zero `size` word that ended the script.
    pub fn terminator_offset(&self) -> u64 {
        self.terminator_offset
    }

    /// Absolute offset just past the terminator: the bytes this script
    /// occupies. The rest of [`Self::limit`] belongs to no script and is
    /// reported by [`DecodedInterp::findings`].
    pub fn end(&self) -> u64 {
        self.terminator_offset + TERMINATOR_BYTES as u64
    }

    /// Lines in stored order, terminator excluded.
    pub fn lines(&self) -> &[InterpLine<'a>] {
        &self.lines
    }

    /// The line at `position`, or `None` when it is out of range.
    pub fn line(&self, position: usize) -> Option<&InterpLine<'a>> {
        self.lines.get(position)
    }

    /// Number of lines.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether the script holds no lines.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// What the validating decoder could not measure but did not drop.
///
/// Every variant is an observation about the container, not a failure: the
/// decode already succeeded. They exist because the spec asks for trailing
/// and unknown regions to be retained (non-negotiable #2) instead of being
/// silently skipped, and because a retail corpus has not yet been measured
/// (F07-D), so these shapes are not yet known to be errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InterpFinding {
    /// A stretch of the container no index entry's script extent covers.
    ///
    /// This is how bytes between the end of the index and the first script,
    /// bytes between two scripts and bytes after the last script are
    /// reported. Nothing is interpreted; the region is only located.
    Unclaimed {
        /// Absolute offset of the first unclaimed byte.
        offset: u64,
        /// Length of the region.
        length: u64,
    },
    /// Two or more index entries point at the same `script_offset`.
    ///
    /// Both scripts decode and both keep their own index position, so no
    /// origin is lost (non-negotiable #2's "index and script extents
    /// independently"). Whether the original format ever does this is
    /// unmeasured, so it is a finding rather than a refusal.
    SharedScriptOffset {
        /// The shared `script_offset`.
        offset: u32,
        /// Index positions that point at it, in index order.
        entries: Vec<usize>,
    },
    /// An index entry's 120-byte name field holds no `0x00`.
    ///
    /// The whole field is then the name. This is a fixed-width field, so a
    /// full-width name is not structurally impossible; the format has not
    /// been measured to say it never happens.
    UnterminatedName {
        /// Index position of the entry.
        index: usize,
        /// Absolute offset of the entry's name field.
        offset: u64,
    },
    /// Bytes after a name's first `0x00` are not zero.
    ///
    /// The padding is kept (F07-A's raw record) and located here rather than
    /// being read as part of the name or used as an identity.
    NamePadding {
        /// Index position of the entry.
        index: usize,
        /// Absolute offset of the first non-zero padding byte.
        offset: u64,
        /// Number of non-zero padding bytes from there to the end of the
        /// field.
        length: u64,
    },
}

impl InterpFinding {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Unclaimed { .. } => "unclaimed",
            Self::SharedScriptOffset { .. } => "shared_script_offset",
            Self::UnterminatedName { .. } => "unterminated_name",
            Self::NamePadding { .. } => "name_padding",
        }
    }
}

impl fmt::Display for InterpFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unclaimed { offset, length } => {
                write!(f, "{length} unclaimed bytes at offset {offset}")
            }
            Self::SharedScriptOffset { offset, entries } => {
                write!(f, "index entries {entries:?} share script offset {offset}")
            }
            Self::UnterminatedName { index, offset } => write!(
                f,
                "name field of index entry {index} at offset {offset} holds no 0x00"
            ),
            Self::NamePadding {
                index,
                offset,
                length,
            } => write!(
                f,
                "{length} non-zero padding bytes after the name of index entry {index} \
                 from offset {offset}"
            ),
        }
    }
}

/// A validated INTERP container: lossless tokens, bounded extents and the
/// findings the walk could not resolve.
///
/// The order of [`Self::scripts`] is index order, not offset order, so two
/// scripts with equal names keep distinct origins whatever order the index
/// lists them in (AC03).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedInterp<'a> {
    header: InterpRawHeader,
    /// The container the tokens point into, borrowed read-only. Exposed so a
    /// consumer that has to hash or re-read a script's own bytes (F07-C's
    /// content-hash identity, non-negotiable #5) does not have to be handed a
    /// second copy of the same slice and hope it is the same one.
    bytes: &'a [u8],
    index_end: u64,
    container_len: u64,
    scripts: Vec<InterpScript<'a>>,
    findings: Vec<InterpFinding>,
}

impl<'a> DecodedInterp<'a> {
    /// The container header.
    pub fn header(&self) -> InterpRawHeader {
        self.header
    }

    /// The whole container, borrowed: the bytes every token offset and every
    /// line offset in this result is relative to.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Every validated script, in index order.
    pub fn scripts(&self) -> &[InterpScript<'a>] {
        &self.scripts
    }

    /// The script at `index`, or `None` when the index is out of range.
    pub fn script(&self, index: usize) -> Option<&InterpScript<'a>> {
        self.scripts.get(index)
    }

    /// Absolute offset just past the index table: the first byte a script may
    /// occupy.
    pub fn index_end(&self) -> u64 {
        self.index_end
    }

    /// Length of the container in bytes.
    pub fn container_len(&self) -> u64 {
        self.container_len
    }

    /// The observations the walk retained instead of resolving, in the order
    /// the walk reports them: the unclaimed regions in offset order, then the
    /// name findings in index order, then the shared script offsets in offset
    /// order.
    pub fn findings(&self) -> &[InterpFinding] {
        &self.findings
    }
}

/// Reads a whole INTERP container and validates it, decoding every line into
/// lossless tokens.
///
/// This is the production path of stage F07-B and the smallest one that
/// exercises the declared behaviour. It runs in one attempt with three
/// phases, in the shape `specs/F05-*.md` established for the ROF tree:
///
/// 1. **walk and validate.** The header, the index and every script are read
///    and checked. Nothing is booked, so a failure leaves the parse's
///    allocation ledger exactly as it found it (F03-C's teardown/retry
///    contract).
/// 2. **book.** Every decoded script, line and token is reserved in one call
///    against the parse's allocation budget, so a container that would not fit
///    is refused before a single decoded table exists.
/// 3. **build.** The tokens are split out of the already-validated data. This
///    phase cannot fail.
///
/// The rules the walk enforces, all of them with an absolute offset:
///
/// * a script's `script_offset` must be at or past the end of the index
///   table ([`InterpError::ScriptOffset`]);
/// * a script must reach a zero `size` word before the next script's offset
///   ([`InterpError::Unterminated`]), and no line's data may cross that bound
///   ([`InterpError::LineOverrun`]) — a script can never decode its
///   neighbour's bytes;
/// * a line's `argument_count` must equal the number of `0x00` delimiters in
///   its data ([`InterpError::ArgumentCount`]), and the data must end with
///   one ([`InterpError::UnterminatedArguments`]), because the end of the last
///   argument is not stored anywhere else.
///
/// What is retained rather than refused: unclaimed regions, two entries
/// pointing at one script offset, a name field that holds no `0x00`, and
/// non-zero bytes after a name's `0x00`. Those become [`InterpFinding`]s on
/// the result, because nothing in the observed format says they are errors
/// and F07-D measures the retail corpus.
///
/// # Errors
///
/// [`InterpError`] for an undocumented header, truncation, a budget refusal or
/// any of the rules above. Which tokens are loading commands, what they load
/// and how they are executed are F07-C and F07-D; this function classifies
/// nothing and executes nothing.
pub fn decode_interp<'bytes>(
    context: &mut ParseContext,
    bytes: &'bytes [u8],
) -> Result<DecodedInterp<'bytes>, InterpError> {
    context.parse(
        INTERP_ENTRYPOINT,
        bytes,
        |reader, allocation, _recursion| {
            let (header, index) = match read_header_and_index(reader) {
                Ok(parts) => parts,
                Err(InterpError::Parse(error)) => return Err(error),
                Err(domain) => return Ok(Err(domain)),
            };
            let container_len = bytes.len() as u64;
            let index_end = INTERP_HEADER_BYTES as u64
                + u64::from(header.script_count) * INDEX_ENTRY_BYTES as u64;

            // Phase 1: walk and validate. Nothing is charged, so a failure
            // here leaves the ledger untouched.
            let walk = match walk_scripts(reader.container(), bytes, index_end, index) {
                Ok(walk) => walk,
                Err(InterpError::Parse(error)) => return Err(error),
                Err(domain) => return Ok(Err(domain)),
            };

            // Phase 2: book every decoded record in one reservation.
            let record_bytes = decoded_record_bytes(reader.container(), &walk)?;
            allocation.reserve("records", index_end, 1, record_bytes)?;

            // Phase 3: build. Every line's data already ended with a 0x00 and
            // held exactly `argument_count` of them, so the split below is
            // total: it cannot run out of bytes and cannot disagree with the
            // declared count.
            let scripts = walk
                .scripts
                .into_iter()
                .map(|raw| InterpScript {
                    entry: raw.entry,
                    name: raw.entry.name_bytes(),
                    limit: raw.limit,
                    terminator_offset: raw.terminator_offset,
                    lines: raw
                        .lines
                        .into_iter()
                        .map(|line| InterpLine {
                            tokens: line
                                .raw
                                .raw_arguments()
                                .map(|argument| InterpToken {
                                    offset: argument.offset,
                                    bytes: argument.bytes,
                                })
                                .collect(),
                            raw: line.raw,
                        })
                        .collect(),
                })
                .collect();

            Ok(Ok(DecodedInterp {
                header,
                bytes,
                index_end,
                container_len,
                scripts,
                findings: walk.findings,
            }))
        },
    )?
}

/// Bytes one decoded token is booked at, including its shared `Vec`
/// bookkeeping: the budget must cover the tokens a line hands out, not only
/// the records around them.
const TOKEN_BOOKING_BYTES: u64 = size_of::<InterpToken<'static>>() as u64 + size_of::<u64>() as u64;

/// The bytes the decoded container of `walk` will hold: one
/// [`InterpScript`] per script, one [`InterpLine`] per line and one booked
/// slot per token.
///
/// Computed with checked arithmetic, so a hostile `argument_count` costs a
/// length overflow here instead of a wrapping total that would let an
/// oversized container through the budget.
fn decoded_record_bytes(container: &str, walk: &Walk<'_>) -> Result<u64, ParseError> {
    let overflow = |what: &str| {
        ParseError::length_overflow(
            container.to_owned(),
            0,
            "records",
            "the decoded records to fit in u64".to_owned(),
            what.to_owned(),
        )
    };
    let mut total: u64 = 0;
    for script in &walk.scripts {
        total = total
            .checked_add(size_of::<InterpScript<'_>>() as u64)
            .ok_or_else(|| overflow("script table"))?;
        for line in &script.lines {
            let tokens = u64::from(line.token_count)
                .checked_mul(TOKEN_BOOKING_BYTES)
                .ok_or_else(|| overflow("token table"))?;
            total = total
                .checked_add(size_of::<InterpLine<'_>>() as u64)
                .and_then(|sum| sum.checked_add(tokens))
                .ok_or_else(|| overflow("line table"))?;
        }
    }
    Ok(total)
}

/// One validated line, before its tokens are built.
struct WalkedLine<'a> {
    raw: InterpRawLine<'a>,
    /// Arguments the validation proved the data holds. Recorded so the
    /// booking phase knows the token count without splitting the data twice.
    token_count: u32,
}

/// One validated script, before its tokens are built.
struct WalkedScript<'a> {
    entry: InterpRawIndexEntry<'a>,
    limit: u64,
    /// Offset of the zero `size` word that ended the script.
    terminator_offset: u64,
    lines: Vec<WalkedLine<'a>>,
}

/// Everything the walk found, before anything is booked.
struct Walk<'a> {
    scripts: Vec<WalkedScript<'a>>,
    findings: Vec<InterpFinding>,
}

/// Walks and validates every script in an index, booking nothing.
///
/// The walk reads each script from its own `script_offset` up to the next
/// script's offset (or the end of the container), so a script that never
/// terminates is caught at the right byte instead of running into its
/// neighbour. The extent bound is computed from the set of script offsets, not
/// from index order, so an index whose entries are not in offset order still
/// bounds each script correctly.
fn walk_scripts<'a>(
    container: &str,
    bytes: &'a [u8],
    index_end: u64,
    index: &'a [u8],
) -> Result<Walk<'a>, InterpError> {
    let entries: Vec<InterpRawIndexEntry<'a>> =
        index_entries(INTERP_HEADER_BYTES as u64, index).collect();

    // The extent of a script is the next strictly greater script offset. A
    // shared offset gets the same limit for both entries, so neither can read
    // past the scripts that follow them.
    let mut sorted: Vec<(u32, usize)> = entries
        .iter()
        .map(|entry| (entry.script_offset, entry.index))
        .collect();
    sorted.sort_unstable();

    let mut shared: Vec<(u32, Vec<usize>)> = Vec::new();
    let mut position = 0usize;
    while position < sorted.len() {
        let offset = sorted[position].0;
        let mut group = vec![sorted[position].1];
        let mut next = position + 1;
        while next < sorted.len() && sorted[next].0 == offset {
            group.push(sorted[next].1);
            next += 1;
        }
        if group.len() > 1 {
            group.sort_unstable();
            shared.push((offset, group));
        }
        position = next;
    }
    let limit_for = |offset: u32| -> u64 {
        let next = sorted.partition_point(|(candidate, _)| *candidate <= offset);
        match sorted.get(next) {
            Some((following, _)) => u64::from(*following),
            None => bytes.len() as u64,
        }
    };

    let mut scripts = Vec::with_capacity(entries.len());
    for entry in &entries {
        let start = u64::from(entry.script_offset);
        if start < index_end {
            return Err(InterpError::ScriptOffset {
                container: container.to_owned(),
                index: entry.index,
                offset: entry.script_offset,
                index_end,
            });
        }
        let limit = limit_for(entry.script_offset);
        let (terminator_offset, lines) = walk_script(container, bytes, *entry, limit)?;
        scripts.push(WalkedScript {
            entry: *entry,
            limit,
            terminator_offset,
            lines,
        });
    }

    // A script claims its own bytes only: from its start to just past the
    // terminator that ended it. The rest of its extent, up to the next
    // script, is padding no script claimed — which is exactly the "trailing
    // or unreferenced region" the spec asks to be retained as a finding.
    let mut claimed: Vec<(u64, u64)> = scripts
        .iter()
        .map(|script| {
            (
                u64::from(script.entry.script_offset),
                script.terminator_offset + TERMINATOR_BYTES as u64,
            )
        })
        .collect();
    claimed.sort_unstable();
    let mut unclaimed = Vec::new();
    let mut cursor = index_end;
    for (start, end) in claimed {
        if start > cursor {
            unclaimed.push((cursor, start - cursor));
        }
        if end > cursor {
            cursor = end;
        }
    }
    if cursor < bytes.len() as u64 {
        unclaimed.push((cursor, bytes.len() as u64 - cursor));
    }
    // Unclaimed regions first: the report reads in offset order, and the
    // regions are the only findings with no index entry of their own.
    let mut ordered: Vec<InterpFinding> = unclaimed
        .into_iter()
        .map(|(offset, length)| InterpFinding::Unclaimed { offset, length })
        .collect();
    for entry in &entries {
        let field = entry.name_field;
        let name_end = field.iter().position(|&byte| byte == 0);
        match name_end {
            None => ordered.push(InterpFinding::UnterminatedName {
                index: entry.index,
                offset: entry.entry_offset,
            }),
            Some(end) => {
                if let Some(padding) = field[end..].iter().position(|&byte| byte != 0) {
                    let non_zero = field[end + padding..].iter().filter(|b| **b != 0).count();
                    ordered.push(InterpFinding::NamePadding {
                        index: entry.index,
                        offset: entry.entry_offset + (end + padding) as u64,
                        length: non_zero as u64,
                    });
                }
            }
        }
    }
    for (offset, group) in shared {
        ordered.push(InterpFinding::SharedScriptOffset {
            offset,
            entries: group,
        });
    }

    Ok(Walk {
        scripts,
        findings: ordered,
    })
}

/// Walks one script from `entry.script_offset` to `limit`, validating every
/// line, and books nothing.
///
/// Returns the offset of the zero `size` word that ended the script and its
/// lines, terminator excluded.
fn walk_script<'a>(
    container: &str,
    bytes: &'a [u8],
    entry: InterpRawIndexEntry<'a>,
    limit: u64,
) -> Result<(u64, Vec<WalkedLine<'a>>), InterpError> {
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
        if offset + 4 > limit {
            // The script reached its extent without a terminator. If the
            // container itself ends here the bytes are truncated, which the
            // reader reports; either way no neighbour's bytes are decoded.
            let error = reader.read_u32(&format!("scripts[{index}].lines[{line}].size"));
            return match error {
                Err(truncated) => Err(InterpError::Parse(truncated)),
                Ok(_) => Err(InterpError::Unterminated {
                    container: container.to_owned(),
                    index,
                    offset,
                    limit,
                }),
            };
        }
        let size = reader.read_u32(&format!("scripts[{index}].lines[{line}].size"))?;
        if size == 0 {
            return Ok((offset, lines));
        }
        let line_end = offset + LINE_HEADER_BYTES as u64 + u64::from(size);
        if line_end > limit {
            return Err(InterpError::LineOverrun {
                container: container.to_owned(),
                script: index,
                line,
                offset,
                size,
                limit,
            });
        }
        let argument_count =
            reader.read_u32(&format!("scripts[{index}].lines[{line}].argument_count"))?;
        let data = reader.read_bytes(
            &format!("scripts[{index}].lines[{line}].data"),
            size as usize,
        )?;
        let data_offset = offset + LINE_HEADER_BYTES as u64;
        let token_count =
            check_arguments(container, index, line, data_offset, data, argument_count)?;
        lines.push(WalkedLine {
            raw: InterpRawLine {
                offset,
                size,
                argument_count,
                data,
            },
            token_count,
        });
    }
}

/// Checks one line's data against its `argument_count` and returns the number
/// of arguments it holds.
///
/// The observed behaviour is a `0x00` count matching the argument count
/// (`docs/research/FORMAT-NOTES.md`, "INTERP observed subset" [S07]). Both
/// halves are checked: the count of stored delimiters must be the declared
/// count, and the data must end with a delimiter, because the end of the last
/// argument is stored nowhere else. A disagreement is refused with the
/// container, script, line and offset; the argument bytes themselves stay out
/// of the error.
fn check_arguments(
    container: &str,
    script: usize,
    line: usize,
    data_offset: u64,
    data: &[u8],
    argument_count: u32,
) -> Result<u32, InterpError> {
    let delimiters = data.iter().filter(|byte| **byte == 0).count();
    if delimiters != usize::try_from(argument_count).unwrap_or(usize::MAX) {
        return Err(InterpError::ArgumentCount {
            container: container.to_owned(),
            script,
            line,
            offset: data_offset,
            declared: argument_count,
            found: u32::try_from(delimiters).unwrap_or(u32::MAX),
        });
    }
    if !data.ends_with(&[0]) {
        return Err(InterpError::UnterminatedArguments {
            container: container.to_owned(),
            script,
            line,
            offset: data_offset + data.len() as u64,
        });
    }
    Ok(u32::try_from(delimiters).unwrap_or(u32::MAX))
}

// ---------------------------------------------------------------------------
// Stage F07-C: the loading plan
// ---------------------------------------------------------------------------

/// Where a registered loading command keeps the parts of an asset key, counted
/// as argument positions from the head token, which is position 0.
///
/// The positions describe the *stored* line, so a registration is a claim
/// about the command's argument domain and can be wrong. A line that does not
/// carry the registered positions is reported as
/// [`PlanLineKind::Malformed`] with the position that is missing or empty,
/// never padded, skipped or read from a neighbouring argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyArguments {
    /// Position of the mount-namespace argument, required.
    pub namespace: usize,
    /// Position of the logical-path argument, required.
    pub path: usize,
    /// Position of the variant argument, when the command spells one. When
    /// `None` the consumer resolves under its own default variant.
    pub variant: Option<usize>,
}

/// Whether a registered command's key is spelled literally where it stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySpelling {
    /// The argument's bytes are the key. The consumer may resolve them.
    Literal,
    /// The command's argument is assembled from other arguments (a variable,
    /// a join, a computed name) before any key exists, so there is nothing to
    /// resolve at plan time.
    ///
    /// This is a *registration's* claim, not a guess made from the bytes: the
    /// plan never decides on its own that some spelling is "really" a
    /// variable reference, because the variable syntax of the loading
    /// language is unmeasured (F07-D).
    Composed,
}

impl KeySpelling {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Literal => "literal",
            Self::Composed => "composed",
        }
    }
}

/// One command registered as a resource-loading command.
///
/// A registration is a claim about the original format, so it carries how much
/// is known about it ([`status`]) and where the claim came from
/// ([`source`], a doc section, a finding or a probe run). Nothing in this
/// crate registers anything by itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadCommand {
    /// The head spelling that identifies the command, as stored bytes. It is
    /// never decoded: no encoding is established for arguments
    /// (non-negotiable #1).
    pub spelling: Vec<u8>,
    /// Where the key's parts are.
    pub arguments: KeyArguments,
    /// Whether the key is spelled literally where it stands.
    pub spelling_kind: KeySpelling,
    /// How much is known about this command. A registration may not claim
    /// [`ClaimStatus::VerifiedOriginal`] on its own: that status belongs to a
    /// fingerprinted evidence record, never to a table somebody typed.
    pub status: ClaimStatus,
    /// Where the classification came from, recorded so a claim can be traced.
    /// Empty is refused by [`LoadCommandTable::insert`].
    pub source: String,
}

/// Why a registration was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableError {
    /// The spelling was empty, so it would match every line that has a head.
    EmptySpelling,
    /// The source was empty, so the claim could not be traced.
    EmptySource,
    /// A key argument sat at position 0, which is the head token itself: a
    /// command cannot name an asset with its own name.
    HeadArgument {
        /// Which argument: `namespace` or `path`.
        part: KeyPart,
    },
    /// The namespace and path arguments, or two arguments of one key, name the
    /// same position, which could not be two different values.
    RepeatedArgument {
        /// Which argument repeated the earlier one.
        part: KeyPart,
        /// The position both of them name.
        position: usize,
    },
    /// A second registration claims the same spelling. One spelling, one
    /// argument domain: a duplicate would make the plan's classification
    /// depend on table order.
    Duplicate {
        /// Position of the first registration of this spelling.
        first: usize,
        /// Position of the refused one.
        second: usize,
    },
    /// A registration claimed [`ClaimStatus::VerifiedOriginal`], which only a
    /// fingerprinted evidence record may award.
    SelfAwardedVerifiedOriginal,
}

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySpelling => write!(f, "a loading command's spelling must not be empty"),
            Self::EmptySource => {
                write!(
                    f,
                    "a loading command must record where its classification came from"
                )
            }
            Self::HeadArgument { part } => write!(
                f,
                "the {} argument cannot be position 0: that is the head token itself",
                part.label()
            ),
            Self::RepeatedArgument { part, position } => write!(
                f,
                "the {} argument repeats another part of the same key at position {position}",
                part.label()
            ),
            Self::Duplicate { first, second } => write!(
                f,
                "loading command {second} re-registers the spelling of command {first}"
            ),
            Self::SelfAwardedVerifiedOriginal => write!(
                f,
                "a command table may not claim verified_original: that status belongs to a \
                 fingerprinted evidence record"
            ),
        }
    }
}

impl std::error::Error for TableError {}

/// One part of an asset key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyPart {
    /// The mount namespace.
    Namespace,
    /// The logical path.
    Path,
    /// The variant.
    Variant,
}

impl KeyPart {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Namespace => "namespace",
            Self::Path => "path",
            Self::Variant => "variant",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Namespace => "namespace",
            Self::Path => "path",
            Self::Variant => "variant",
        }
    }
}

/// The commands somebody has registered as resource-loading.
///
/// **The table this crate builds is empty and stays empty.** Which commands
/// load resources is F07-D's measurement over the installed container
/// (non-negotiable #3: this container does not by itself identify the full
/// mission language, so a head token may be a loading command, a game-behaviour
/// command or neither). A caller supplies the registrations it has evidence
/// for; every head token the table does not name is unclassified, and an
/// unclassified line fails the plan of the world it belongs to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoadCommandTable {
    commands: Vec<LoadCommand>,
}

impl LoadCommandTable {
    /// A table with no registrations: every line is unclassified.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers one command, refusing the registrations that would make the
    /// plan's classification ambiguous or self-awarding.
    ///
    /// Registrations are matched by exact bytes, so two spellings that differ
    /// only in case are two commands — the plan does not fold case, because no
    /// evidence says the original loader does.
    pub fn insert(&mut self, command: LoadCommand) -> Result<usize, TableError> {
        if command.spelling.is_empty() {
            return Err(TableError::EmptySpelling);
        }
        if command.source.trim().is_empty() {
            return Err(TableError::EmptySource);
        }
        if command.status == ClaimStatus::VerifiedOriginal {
            return Err(TableError::SelfAwardedVerifiedOriginal);
        }
        check_key_arguments(command.arguments)?;
        if let Some(first) = self.find(&command.spelling) {
            return Err(TableError::Duplicate {
                first,
                second: self.commands.len(),
            });
        }
        self.commands.push(command);
        Ok(self.commands.len() - 1)
    }

    /// Registers every command of `commands`, stopping at the first refusal
    /// and leaving the table as it was: either the whole set is registered or
    /// none of it is, so a table never mixes a half-applied classification.
    pub fn extend(
        &mut self,
        commands: impl IntoIterator<Item = LoadCommand>,
    ) -> Result<(), TableError> {
        let mut staged = self.clone();
        for command in commands {
            staged.insert(command)?;
        }
        *self = staged;
        Ok(())
    }

    /// The registration of `spelling`, by exact bytes.
    pub fn get(&self, spelling: &[u8]) -> Option<(usize, &LoadCommand)> {
        self.find(spelling)
            .map(|index| (index, &self.commands[index]))
    }

    /// Every registration, in insertion order.
    pub fn commands(&self) -> &[LoadCommand] {
        &self.commands
    }

    /// How many commands are registered.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether no command is registered.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    fn find(&self, spelling: &[u8]) -> Option<usize> {
        self.commands
            .iter()
            .position(|command| command.spelling == spelling)
    }
}

// ---------------------------------------------------------------------------
// Stage F07-D: opcode classification and audit
// ---------------------------------------------------------------------------

/// What the loading audit established about one opcode head.
///
/// This is the classification `specs/F07-interp-loading-script-container.md`
/// non-negotiable #3 asks for — which commands load resources and which refer
/// to game behaviour. It is deliberately **not** two-valued, because a command
/// nobody has established anything about is not the same as one known not to
/// load:
///
/// * [`OpcodeClass::Loading`] names the argument positions that spell an asset
///   key, so the plan resolves the line's key from them;
/// * [`OpcodeClass::Unsupported`] says the command is recognized as a
///   resource-loading command but this stage has no supported key domain for
///   it — its arguments are a directory, a `%VARIABLE%` spelling or a path that
///   is not a mount-namespace key. The plan reports it, with its source offset
///   and affected world, rather than pretending it loaded nothing;
/// * [`OpcodeClass::Behavior`] says the command is not a resource load: game,
///   scene, camera, world or language work that contributes no dependency. The
///   plan carries it without blocking on it, and does not interpret it either.
///
/// A head that no entry names is **unclassified**: unknown, and the plan fails
/// the world it belongs to. Absence is never a default class, and nothing here
/// decides a class by itself — a caller supplies the evidence (the container
/// cannot identify the language on its own).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpcodeClass {
    /// The command's key is spelled from these argument positions.
    Loading {
        /// Which stored argument positions spell the key.
        arguments: KeyArguments,
        /// Whether the key is literal where it stands or assembled later.
        spelling_kind: KeySpelling,
    },
    /// Recognized as resource-loading, but with no supported key domain here.
    Unsupported,
    /// Not a resource load: contributes no dependency to the loading plan.
    Behavior,
}

impl OpcodeClass {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Loading { .. } => "loading",
            Self::Unsupported => "unsupported",
            Self::Behavior => "behavior",
        }
    }

    /// Whether this class contributes an asset key the plan can resolve.
    pub const fn is_loading(&self) -> bool {
        matches!(self, Self::Loading { .. })
    }
}

impl fmt::Display for OpcodeClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// One opcode head, classified, with how much is known about it and where the
/// claim came from.
///
/// A classification is a claim about the original language, so every entry
/// carries a [`ClaimStatus`] and a `source` (a doc section, a finding or a
/// probe run). Nothing in this crate registers anything by itself: the
/// classification of an installed corpus is data a caller measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassifiedOpcode {
    /// The head spelling, as stored bytes. Never decoded: no encoding is
    /// established for a container's own bytes (non-negotiable #1).
    pub spelling: Vec<u8>,
    /// What the command was classified as.
    pub class: OpcodeClass,
    /// How much is known about this classification. [`ClaimStatus::VerifiedOriginal`]
    /// may not be awarded by a table: that status belongs to a fingerprinted
    /// evidence record.
    pub status: ClaimStatus,
    /// Where the classification came from. Empty is refused by
    /// [`OpcodeClassTable::insert`].
    pub source: String,
}

/// The opcode classifications somebody has established for a corpus.
///
/// **The table this crate builds is empty.** Which commands load resources is
/// a measurement over the installed container, and the container does not
/// identify the language by itself (non-negotiable #3), so this type refuses a
/// table that would classify a head twice or self-award a status that only
/// fingerprinted evidence may, and an unclassified head fails the plan rather
/// than defaulting to either "loads" or "does not load".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpcodeClassTable {
    opcodes: Vec<ClassifiedOpcode>,
}

impl OpcodeClassTable {
    /// A table with no classifications: every head is unclassified.
    pub fn new() -> Self {
        Self::default()
    }

    /// The classifications an F07-C [`LoadCommandTable`] implies: each
    /// registered command is `Loading` with the key domain it registered.
    ///
    /// This is how the F07-C plan keeps its exact behaviour while the F07-D
    /// plan speaks the wider classification: the loading registry is a table of
    /// [`OpcodeClass::Loading`] entries and nothing else.
    pub fn from_load_commands(commands: &[LoadCommand]) -> Self {
        let mut table = Self::new();
        for command in commands {
            table
                .insert(ClassifiedOpcode {
                    spelling: command.spelling.clone(),
                    class: OpcodeClass::Loading {
                        arguments: command.arguments,
                        spelling_kind: command.spelling_kind,
                    },
                    status: command.status,
                    source: command.source.clone(),
                })
                .expect("a load command table is already validated");
        }
        table
    }

    /// Classifies one opcode, refusing the entries that would make the audit
    /// ambiguous, untraceable or self-awarding.
    ///
    /// Matching is by exact bytes, so two spellings that differ only in case
    /// are two opcodes — no evidence says the original loader folds case.
    pub fn insert(&mut self, opcode: ClassifiedOpcode) -> Result<usize, TableError> {
        if opcode.spelling.is_empty() {
            return Err(TableError::EmptySpelling);
        }
        if opcode.source.trim().is_empty() {
            return Err(TableError::EmptySource);
        }
        if opcode.status == ClaimStatus::VerifiedOriginal {
            return Err(TableError::SelfAwardedVerifiedOriginal);
        }
        if let OpcodeClass::Loading { arguments, .. } = opcode.class {
            check_key_arguments(arguments)?;
        }
        if let Some(first) = self.find(&opcode.spelling) {
            return Err(TableError::Duplicate {
                first,
                second: self.opcodes.len(),
            });
        }
        self.opcodes.push(opcode);
        Ok(self.opcodes.len() - 1)
    }

    /// Classifies every opcode of `opcodes`, stopping at the first refusal and
    /// leaving the table as it was: either the whole set applies or none of it.
    pub fn extend(
        &mut self,
        opcodes: impl IntoIterator<Item = ClassifiedOpcode>,
    ) -> Result<(), TableError> {
        let mut staged = self.clone();
        for opcode in opcodes {
            staged.insert(opcode)?;
        }
        *self = staged;
        Ok(())
    }

    /// The classification of `spelling`, by exact bytes.
    pub fn get(&self, spelling: &[u8]) -> Option<(usize, &ClassifiedOpcode)> {
        self.find(spelling)
            .map(|index| (index, &self.opcodes[index]))
    }

    /// Every classification, in insertion order.
    pub fn opcodes(&self) -> &[ClassifiedOpcode] {
        &self.opcodes
    }

    /// The classification at `index`, or `None` when it is out of range.
    pub fn opcode(&self, index: usize) -> Option<&ClassifiedOpcode> {
        self.opcodes.get(index)
    }

    /// How many opcodes are classified.
    pub fn len(&self) -> usize {
        self.opcodes.len()
    }

    /// Whether no opcode is classified.
    pub fn is_empty(&self) -> bool {
        self.opcodes.is_empty()
    }

    fn find(&self, spelling: &[u8]) -> Option<usize> {
        self.opcodes
            .iter()
            .position(|opcode| opcode.spelling == spelling)
    }
}

/// Refuses a `Loading` entry whose argument domain could not spell a key.
fn check_key_arguments(arguments: KeyArguments) -> Result<(), TableError> {
    if arguments.namespace == 0 {
        return Err(TableError::HeadArgument {
            part: KeyPart::Namespace,
        });
    }
    if arguments.path == 0 {
        return Err(TableError::HeadArgument {
            part: KeyPart::Path,
        });
    }
    if arguments.path == arguments.namespace {
        return Err(TableError::RepeatedArgument {
            part: KeyPart::Path,
            position: arguments.path,
        });
    }
    if let Some(variant) = arguments.variant {
        if variant == 0 {
            return Err(TableError::HeadArgument {
                part: KeyPart::Variant,
            });
        }
        if variant == arguments.namespace || variant == arguments.path {
            return Err(TableError::RepeatedArgument {
                part: KeyPart::Variant,
                position: variant,
            });
        }
    }
    Ok(())
}

/// One distinct head in a container and how the audit classified it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpcodeAuditEntry {
    spelling: Vec<u8>,
    occurrences: usize,
    classification: Option<usize>,
}

impl OpcodeAuditEntry {
    /// The head spelling, as stored bytes.
    pub fn spelling(&self) -> &[u8] {
        &self.spelling
    }

    /// How many lines in the container have this head.
    pub fn occurrences(&self) -> usize {
        self.occurrences
    }

    /// Position of the classification in the audit's own snapshot, or `None`
    /// when the head is unclassified.
    pub fn classification(&self) -> Option<usize> {
        self.classification
    }

    /// Whether the head is classified at all.
    pub fn is_classified(&self) -> bool {
        self.classification.is_some()
    }

    /// Stable lowercase identifier for logs, `"unknown"` when unclassified.
    pub fn code(&self) -> &'static str {
        match self.classification {
            Some(_) => "classified",
            None => "unknown",
        }
    }
}

/// The result of auditing every distinct opcode head of a container.
///
/// The audit is the count `docs/contracts/SCRIPT-MISSION.md` asks a source
/// adapter for — raw records and unknown instructions — made inspectable: one
/// entry per distinct head, in spelling order, each naming its class and how
/// many lines carry it. [`OpcodeAudit::is_complete`] is true only when **every**
/// distinct head is classified, so "classify every opcode" is a property a
/// caller can check rather than assert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpcodeAudit {
    opcodes: Vec<ClassifiedOpcode>,
    entries: Vec<OpcodeAuditEntry>,
    lines: usize,
}

impl OpcodeAudit {
    /// Every distinct head, in spelling order.
    pub fn entries(&self) -> &[OpcodeAuditEntry] {
        &self.entries
    }

    /// The classification snapshot the audit was taken against.
    pub fn opcodes(&self) -> &[ClassifiedOpcode] {
        &self.opcodes
    }

    /// The classification at `index`, or `None` when it is out of range.
    pub fn opcode(&self, index: usize) -> Option<&ClassifiedOpcode> {
        self.opcodes.get(index)
    }

    /// Lines decoded across every script.
    pub fn lines(&self) -> usize {
        self.lines
    }

    /// How many distinct heads the container holds.
    pub fn distinct_heads(&self) -> usize {
        self.entries.len()
    }

    /// How many distinct heads no entry classified.
    pub fn unknown_heads(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.classification.is_none())
            .count()
    }

    /// How many distinct heads are classified with `class.code()`.
    pub fn heads_of(&self, code: &str) -> usize {
        self.entries
            .iter()
            .filter(|entry| {
                entry
                    .classification
                    .and_then(|index| self.opcodes.get(index))
                    .is_some_and(|opcode| opcode.class.code() == code)
            })
            .count()
    }

    /// Whether every distinct head is classified. False as soon as one head is
    /// unknown, so an incomplete classification is visible instead of assumed.
    pub fn is_complete(&self) -> bool {
        self.entries
            .iter()
            .all(|entry| entry.classification.is_some())
    }
}

/// Audits every distinct opcode head of a decoded container against `table`.
///
/// The walk is the same one the plan makes, but grouped by head instead of by
/// line: every distinct spelling becomes exactly one [`OpcodeAuditEntry`] with
/// its occurrence count and the classification it matched, or `None` when
/// nothing classified it. A head listed twice in a table cannot happen — the
/// table refuses duplicates — so an entry's class is unambiguous.
///
/// # Panics
///
/// Never.
pub fn audit_interp_opcodes(decoded: &DecodedInterp<'_>, table: &OpcodeClassTable) -> OpcodeAudit {
    let mut counts: BTreeMap<Vec<u8>, usize> = BTreeMap::new();
    let mut lines = 0usize;
    for script in decoded.scripts() {
        for line in script.lines() {
            let head = line.head().expect("a line always has a head token");
            *counts.entry(head.bytes().to_vec()).or_insert(0) += 1;
            lines += 1;
        }
    }
    let entries: Vec<OpcodeAuditEntry> = counts
        .into_iter()
        .map(|(spelling, occurrences)| {
            let classification = table.get(&spelling).map(|(index, _)| index);
            OpcodeAuditEntry {
                spelling,
                occurrences,
                classification,
            }
        })
        .collect();
    OpcodeAudit {
        opcodes: table.opcodes().to_vec(),
        entries,
        lines,
    }
}

/// Where one script lives in its container, and therefore where a failure
/// about it points.
///
/// Equal names are not an identity: two entries may carry the same name, the
/// same `timestamp` and even the same body, and this record still differs
/// between them (spec F07 AC03). Nothing here may be used as a cache key —
/// the consumer computes a content hash over
/// `script_offset..end` for identity (non-negotiable #5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptOrigin {
    index: usize,
    entry_offset: u64,
    script_offset: u32,
    terminator_offset: u64,
    end: u64,
}

impl ScriptOrigin {
    /// Position of the script's index entry, `0..script_count`.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Absolute offset of the index entry.
    pub fn entry_offset(&self) -> u64 {
        self.entry_offset
    }

    /// The stored `script_offset` word.
    pub fn script_offset(&self) -> u32 {
        self.script_offset
    }

    /// Absolute offset of the zero `size` word that ended the script.
    pub fn terminator_offset(&self) -> u64 {
        self.terminator_offset
    }

    /// Absolute offset just past the terminator: the bytes this script
    /// occupies, and the range a consumer hashes for identity.
    pub fn end(&self) -> u64 {
        self.end
    }
}

/// Why a registered command's stored arguments do not match its registration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MalformedKey {
    /// The line has fewer arguments than the registration names.
    MissingArgument {
        /// The position the registration names.
        position: usize,
    },
    /// The named argument holds no bytes, so it names no value.
    EmptyArgument {
        /// The position the registration names.
        position: usize,
    },
}

impl MalformedKey {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(self) -> &'static str {
        match self {
            Self::MissingArgument { .. } => "missing_argument",
            Self::EmptyArgument { .. } => "empty_argument",
        }
    }

    /// The argument position the registration named.
    pub const fn position(self) -> usize {
        match self {
            Self::MissingArgument { position } | Self::EmptyArgument { position } => position,
        }
    }
}

impl fmt::Display for MalformedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingArgument { position } => {
                write!(f, "the line has no argument at position {position}")
            }
            Self::EmptyArgument { position } => {
                write!(f, "the argument at position {position} is empty")
            }
        }
    }
}

/// The three argument tokens a registered command spells its key with, and
/// the absolute offset each of them occupies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyTokens<'a> {
    namespace: InterpToken<'a>,
    path: InterpToken<'a>,
    variant: Option<InterpToken<'a>>,
}

impl<'a> KeyTokens<'a> {
    /// The mount-namespace argument, as stored bytes.
    pub fn namespace(&self) -> InterpToken<'a> {
        self.namespace
    }

    /// The logical-path argument, as stored bytes.
    pub fn path(&self) -> InterpToken<'a> {
        self.path
    }

    /// The variant argument, when the registration names one.
    pub fn variant(&self) -> Option<InterpToken<'a>> {
        self.variant
    }
}

/// What one line of a script turned out to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanLineKind<'a> {
    /// A registered loading command whose stored arguments carry the key its
    /// registration names.
    Loading {
        /// Position of the registration in the plan's own command snapshot.
        command: usize,
        /// The tokens the key is spelled with.
        key: KeyTokens<'a>,
    },
    /// A registered command whose stored arguments do not match its
    /// registration. The line is not interpreted at all, because there is
    /// nothing to interpret.
    Malformed {
        /// Position of the registration in the plan's own command snapshot.
        command: usize,
        /// What the line is missing.
        reason: MalformedKey,
    },
    /// A command classified as resource-loading for which this stage has no
    /// supported key domain.
    ///
    /// The command is understood to load *something*, but not by a key the plan
    /// can spell, so the line fails its world with its source offset rather
    /// than being treated as loaded or as no load at all (spec F07-D, AC04).
    Unsupported {
        /// Position of the classification in the plan's own opcode snapshot.
        opcode: usize,
    },
    /// A command classified as *not* a resource load.
    ///
    /// It contributes no dependency and does not block its script: a world is
    /// not unloadable because a scene-graph or camera command sits beside its
    /// loading commands. This stage does not interpret it either — what it does
    /// is the mission language's business (F13).
    Behavior {
        /// Position of the classification in the plan's own opcode snapshot.
        opcode: usize,
    },
    /// The head token matches no classification.
    ///
    /// This is a failure, not a guess: the plan cannot say this line loads
    /// nothing, only that nobody has established what it does.
    Unclassified,
}

impl PlanLineKind<'_> {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Loading { .. } => "loading",
            Self::Malformed { .. } => "malformed",
            Self::Unsupported { .. } => "unsupported",
            Self::Behavior { .. } => "behavior",
            Self::Unclassified => "unclassified",
        }
    }

    /// Whether this line contributes an asset the consumer may resolve.
    pub const fn is_loading(&self) -> bool {
        matches!(self, Self::Loading { .. })
    }

    /// Whether this line is classified as not a resource load.
    pub const fn is_behavior(&self) -> bool {
        matches!(self, Self::Behavior { .. })
    }

    /// Whether this line stops its script's plan from being usable.
    ///
    /// A classified [`Self::Behavior`] line does not: the plan knows it
    /// contributes nothing, so it is not an unknown. Everything else that is
    /// not a resolvable [`Self::Loading`] line does.
    pub const fn is_blocking(&self) -> bool {
        matches!(
            self,
            Self::Malformed { .. } | Self::Unsupported { .. } | Self::Unclassified
        )
    }
}

/// One line of a script as the loading plan sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanLine<'a> {
    position: usize,
    line: InterpLine<'a>,
    head_offset: u64,
    kind: PlanLineKind<'a>,
}

impl<'a> PlanLine<'a> {
    /// Position of the line inside its script.
    pub fn position(&self) -> usize {
        self.position
    }

    /// The decoded line, tokens and raw record both.
    pub fn line(&self) -> &InterpLine<'a> {
        &self.line
    }

    /// Absolute offset of the line's `size` word: the offset a diagnostic
    /// about this line quotes.
    pub fn source_offset(&self) -> u64 {
        self.line.offset()
    }

    /// Absolute offset of the head token's first byte.
    pub fn head_offset(&self) -> u64 {
        self.head_offset
    }

    /// The head token, as stored bytes.
    pub fn head(&self) -> InterpToken<'a> {
        self.line.head().expect("a line always has a head token")
    }

    /// How the line was classified.
    pub fn kind(&self) -> &PlanLineKind<'a> {
        &self.kind
    }

    /// Whether this line stops its script's plan from being usable.
    pub fn is_blocking(&self) -> bool {
        self.kind.is_blocking()
    }
}

/// One script's place in the loading plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanScript<'a> {
    origin: ScriptOrigin,
    name: &'a [u8],
    raw_timestamp: u32,
    lines: Vec<PlanLine<'a>>,
}

impl<'a> PlanScript<'a> {
    /// Where the script lives: index position, entry offset, script offset,
    /// terminator and end.
    pub fn origin(&self) -> ScriptOrigin {
        self.origin
    }

    /// The name bytes, exactly as stored. Not an identity.
    pub fn name(&self) -> &'a [u8] {
        self.name
    }

    /// The `timestamp` word, verbatim. Metadata only: it is never a cache
    /// identifier and never a plan identity (non-negotiable #5).
    pub fn raw_timestamp(&self) -> u32 {
        self.raw_timestamp
    }

    /// Every line, in stored order.
    pub fn lines(&self) -> &[PlanLine<'a>] {
        &self.lines
    }

    /// The line at `position`, or `None` when it is out of range.
    pub fn line(&self, position: usize) -> Option<&PlanLine<'a>> {
        self.lines.get(position)
    }

    /// How many lines the script has.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether the script has no lines.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Whether every line of this script is a usable loading command.
    ///
    /// A script with no lines is usable: a script that loads nothing is not
    /// the same as one whose commands are unknown.
    pub fn is_usable(&self) -> bool {
        self.lines.iter().all(|line| !line.is_blocking())
    }

    /// The lines that block this script, in stored order.
    pub fn blocking_lines(&self) -> impl Iterator<Item = &PlanLine<'a>> {
        self.lines.iter().filter(|line| line.is_blocking())
    }
}

/// The tally `docs/contracts/SCRIPT-MISSION.md` asks a source adapter for:
/// how much was read, how much decoded, and how much is not understood.
///
/// Nothing here is a success measure. A plan whose `unclassified_commands` is
/// large has read a great deal and understood little.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlanStats {
    /// Scripts in the container.
    pub scripts: usize,
    /// Lines decoded across every script.
    pub lines: usize,
    /// Lines that matched a registered loading command.
    pub loading_commands: usize,
    /// Lines whose arguments did not match their registration.
    pub malformed_commands: usize,
    /// Lines whose command is recognized as loading but has no supported key
    /// domain (`unsupported_command` in a report).
    pub unsupported_commands: usize,
    /// Lines whose command is classified as not a resource load.
    pub behavior_commands: usize,
    /// Lines whose head token no classification names.
    pub unclassified_commands: usize,
    /// Distinct head spellings seen, whether or not they are classified.
    pub distinct_heads: usize,
    /// Lines in scripts that are not usable, so a caller can tell a container
    /// with one bad command from one where every script is affected.
    pub blocked_scripts: usize,
}

/// A validated container turned into a loading plan.
///
/// The plan owns **snapshots** of the tables it was built with — the loading
/// registrations ([`Self::commands`]) and the full classification
/// ([`Self::opcodes`]) — rather than borrowing them, so a plan stays readable
/// after the tables it was classified against have been dropped or extended,
/// and a report cannot be read against different tables than the ones that
/// produced it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterpLoadPlan<'a> {
    commands: Vec<LoadCommand>,
    opcodes: Vec<ClassifiedOpcode>,
    scripts: Vec<PlanScript<'a>>,
    stats: PlanStats,
}

impl<'a> InterpLoadPlan<'a> {
    /// The loading-command snapshot this plan was built with, in registration
    /// order. Only [`OpcodeClass::Loading`] classifications appear here.
    pub fn commands(&self) -> &[LoadCommand] {
        &self.commands
    }

    /// The loading registration at `index`, or `None` when out of range.
    pub fn command(&self, index: usize) -> Option<&LoadCommand> {
        self.commands.get(index)
    }

    /// The full classification snapshot this plan was built with.
    pub fn opcodes(&self) -> &[ClassifiedOpcode] {
        &self.opcodes
    }

    /// The classification at `index`, or `None` when out of range.
    pub fn opcode(&self, index: usize) -> Option<&ClassifiedOpcode> {
        self.opcodes.get(index)
    }

    /// Every script, in index order.
    pub fn scripts(&self) -> &[PlanScript<'a>] {
        &self.scripts
    }

    /// The script at `index`, or `None` when the index is out of range.
    pub fn script(&self, index: usize) -> Option<&PlanScript<'a>> {
        self.scripts.get(index)
    }

    /// The tally of what was read, decoded and not understood.
    pub fn stats(&self) -> PlanStats {
        self.stats
    }

    /// Whether every line of every script is classified and usable.
    pub fn is_complete(&self) -> bool {
        self.scripts.iter().all(PlanScript::is_usable)
    }

    /// The scripts that are not usable, in index order.
    pub fn blocked_scripts(&self) -> impl Iterator<Item = &PlanScript<'a>> {
        self.scripts.iter().filter(|script| !script.is_usable())
    }
}

/// Turns a validated container into a loading plan against the F07-C loading
/// registry (stage F07-C).
///
/// This is the F07-C entry point kept for compatibility: it builds a
/// classification in which every registered command is
/// [`OpcodeClass::Loading`] and nothing is classified as behaviour or
/// unsupported, then delegates to [`plan_interp_loading_classified`]. Its
/// behaviour is therefore exactly the F07-C plan's: an unregistered head is
/// [`PlanLineKind::Unclassified`] and fails its script.
///
/// # Panics
///
/// Never. Every input is either classified or reported.
pub fn plan_interp_loading<'a>(
    decoded: &DecodedInterp<'a>,
    table: &LoadCommandTable,
) -> InterpLoadPlan<'a> {
    let classes = OpcodeClassTable::from_load_commands(table.commands());
    plan_interp_loading_classified(decoded, &classes)
}

/// Turns a validated container into a loading plan against a full opcode
/// classification (stage F07-D).
///
/// Every line of every script becomes a [`PlanLine`], classified against
/// `table` by exact head bytes. This function never decides what a command
/// *means*: it reports which classification a line matched, which argument
/// positions spell a loading command's key, and — for every head no
/// classification names — that the line is unclassified, with its offset.
///
/// A [`OpcodeClass::Behavior`] line is classified, contributes no dependency
/// and does not block its script. A [`OpcodeClass::Unsupported`] line blocks
/// with its source offset: it is understood to load something, but by no key
/// this stage can spell, so pretending it loaded nothing would be a fabricated
/// loading state (spec F07-D, AC04).
///
/// The plan is a read of the container, not an execution of it: nothing here
/// resolves a path, a world or an asset. The consumer owns all of that
/// (`cs_content::loading`).
///
/// # Panics
///
/// Never. Every input is either classified or reported.
pub fn plan_interp_loading_classified<'a>(
    decoded: &DecodedInterp<'a>,
    table: &OpcodeClassTable,
) -> InterpLoadPlan<'a> {
    let opcodes = table.opcodes().to_vec();
    // The loading registrations the F07-C consumer reads, in classification
    // order, and where each classification's registration lives.
    let mut commands = Vec::new();
    let mut command_of = Vec::with_capacity(opcodes.len());
    for opcode in &opcodes {
        match &opcode.class {
            OpcodeClass::Loading {
                arguments,
                spelling_kind,
            } => {
                command_of.push(Some(commands.len()));
                commands.push(LoadCommand {
                    spelling: opcode.spelling.clone(),
                    arguments: *arguments,
                    spelling_kind: *spelling_kind,
                    status: opcode.status,
                    source: opcode.source.clone(),
                });
            }
            OpcodeClass::Unsupported | OpcodeClass::Behavior => command_of.push(None),
        }
    }
    let lookup: BTreeMap<&[u8], usize> = opcodes
        .iter()
        .enumerate()
        .map(|(index, opcode)| (opcode.spelling.as_slice(), index))
        .collect();

    let mut heads: BTreeSet<&[u8]> = BTreeSet::new();
    let mut stats = PlanStats {
        scripts: decoded.scripts().len(),
        ..PlanStats::default()
    };
    let scripts = decoded
        .scripts()
        .iter()
        .map(|script| {
            let lines: Vec<PlanLine<'a>> = script
                .lines()
                .iter()
                .enumerate()
                .map(|(position, line)| {
                    let head = line.head().expect("a line always has a head token");
                    let head_offset = head.offset();
                    heads.insert(head.bytes());
                    stats.lines += 1;
                    let kind = classify_line(line, &opcodes, &command_of, &lookup, &mut stats);
                    PlanLine {
                        position,
                        line: line.clone(),
                        head_offset,
                        kind,
                    }
                })
                .collect();
            let usable = lines.iter().all(|line| !line.is_blocking());
            if !usable {
                stats.blocked_scripts += 1;
            }
            PlanScript {
                origin: ScriptOrigin {
                    index: script.entry().index,
                    entry_offset: script.entry().entry_offset,
                    script_offset: script.entry().script_offset,
                    terminator_offset: script.terminator_offset(),
                    end: script.end(),
                },
                name: script.name(),
                raw_timestamp: script.entry().raw_timestamp,
                lines,
            }
        })
        .collect();
    stats.distinct_heads = heads.len();
    InterpLoadPlan {
        commands,
        opcodes,
        scripts,
        stats,
    }
}

/// Classifies one line against the classification snapshot, counting the
/// outcome in `stats`.
fn classify_line<'a>(
    line: &InterpLine<'a>,
    opcodes: &[ClassifiedOpcode],
    command_of: &[Option<usize>],
    lookup: &BTreeMap<&[u8], usize>,
    stats: &mut PlanStats,
) -> PlanLineKind<'a> {
    let head = line.head().expect("a line always has a head token");
    let Some(&index) = lookup.get(head.bytes()) else {
        stats.unclassified_commands += 1;
        return PlanLineKind::Unclassified;
    };
    match opcodes[index].class {
        OpcodeClass::Behavior => {
            stats.behavior_commands += 1;
            PlanLineKind::Behavior { opcode: index }
        }
        OpcodeClass::Unsupported => {
            stats.unsupported_commands += 1;
            PlanLineKind::Unsupported { opcode: index }
        }
        OpcodeClass::Loading { arguments, .. } => {
            let command =
                command_of[index].expect("a loading classification always has a registration");
            let token = |position: usize| line.tokens().get(position).copied();
            let mut key = KeyTokens {
                namespace: match token(arguments.namespace) {
                    Some(namespace) if !namespace.is_empty() => namespace,
                    Some(_) => {
                        stats.malformed_commands += 1;
                        return PlanLineKind::Malformed {
                            command,
                            reason: MalformedKey::EmptyArgument {
                                position: arguments.namespace,
                            },
                        };
                    }
                    None => {
                        stats.malformed_commands += 1;
                        return PlanLineKind::Malformed {
                            command,
                            reason: MalformedKey::MissingArgument {
                                position: arguments.namespace,
                            },
                        };
                    }
                },
                path: match token(arguments.path) {
                    Some(path) if !path.is_empty() => path,
                    Some(_) => {
                        stats.malformed_commands += 1;
                        return PlanLineKind::Malformed {
                            command,
                            reason: MalformedKey::EmptyArgument {
                                position: arguments.path,
                            },
                        };
                    }
                    None => {
                        stats.malformed_commands += 1;
                        return PlanLineKind::Malformed {
                            command,
                            reason: MalformedKey::MissingArgument {
                                position: arguments.path,
                            },
                        };
                    }
                },
                variant: None,
            };
            if let Some(position) = arguments.variant {
                match token(position) {
                    Some(variant) if !variant.is_empty() => key.variant = Some(variant),
                    Some(_) => {
                        stats.malformed_commands += 1;
                        return PlanLineKind::Malformed {
                            command,
                            reason: MalformedKey::EmptyArgument { position },
                        };
                    }
                    None => {
                        stats.malformed_commands += 1;
                        return PlanLineKind::Malformed {
                            command,
                            reason: MalformedKey::MissingArgument { position },
                        };
                    }
                }
            }
            stats.loading_commands += 1;
            PlanLineKind::Loading { command, key }
        }
    }
}
