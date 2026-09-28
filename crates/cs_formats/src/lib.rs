//! Byte-level parsers, raw records and parse diagnostics.
//!
//! This stage provides the checked reader, the structured errors, the
//! bounded-allocation / recursion utilities and the contextual-error
//! entrypoint every later parser builds on (`specs/F03-bounded-binary-
//! parsing-primitives.md`, stages F03-A, F03-B and F03-C), plus the first
//! format built on them: the raw ROF directory block of `specs/F05-rof-
//! directory-trees-and-compressed-members.md` (stage F05-A, [`rof`]).
//! Allowed
//! dependency: [`cs_types`]. This crate must never depend on Bevy or Avian,
//! and parsing must stay independent of renderer, window, network, game state
//! and asset-directory enumeration.
//!
//! The fixtures exercised below are newly authored synthetic bytes; nothing
//! here is derived from original game data.

pub mod error;
pub mod io;
pub mod rof;

pub use error::{ParseError, ParseErrorKind};
pub use io::{AllocationBudget, ParseContext, Reader, RecursionBudget, RecursionGuard};
pub use rof::{
    DIRECTORY_ENTRYPOINT, DIRECTORY_HEADER_BYTES, FLAG_COMPRESSED, FLAG_DIRECTORY, KNOWN_FLAG_MASK,
    RECORD_BYTES, RofDirectory, RofEntries, RofEntry, RofError, RofFlags, RofRawHeader,
    RofRawRecord, read_directory,
};
