//! Text configuration dialects and their lossless nodes
//! (`specs/F12-text-configuration-strings-and-pe-resources.md`, stage
//! `### F12-A`).
//!
//! * [`dialect`] is the inventory of the text, configuration and
//!   string-resource dialects a survey of the installation found, and the
//!   member rules that route a member to one.
//! * [`lines`] scans any input into lines with their offsets and
//!   terminators, so every byte survives.
//! * [`keyed_list`] turns the one configuration dialect with a reader
//!   ([`dialect::TextDialect::KeyedList`]) into one node per line: blank,
//!   comment, section, entry with quote-aware fields, or unclassified.
//! * [`resource_header`] reads the `#define NAME value` resource-id headers
//!   ([`dialect::TextDialect::ResourceHeader`]) that name the same ids
//!   [`crate::pe_resources`] walks in a PE resource directory (stage F12-B).
//!
//! The survey, the design decisions and the recorded unknowns are written
//! down in
//! `docs/findings/2026-09-28-f12-a-text-dialects-and-lossless-config-nodes.md`.
//! The fixtures exercised by the tests are newly authored; nothing here is
//! derived from original game text.

pub mod dialect;
pub mod keyed_list;
pub mod lines;
pub mod resource_header;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_f12_b;

pub use dialect::{
    DialectReader, DialectRecord, LexicalFeature, MemberRule, ObservedEncoding,
    TEXT_DIALECT_INVENTORY, TextDialect, dialect_for_member,
};
pub use keyed_list::{
    Entry, Field, Fields, KEYED_LIST_ENTRYPOINT, KeyedList, KeyedListLine, LineKind, QuoteIssue,
    Unclassified, read_keyed_list,
};
pub use lines::{
    LINES_ENTRYPOINT, LineTerminator, TerminatorCounts, TextLine, TextLines, scan_lines,
};
pub use resource_header::{
    Define, HeaderLookup, MAX_RESOURCE_ID, RESOURCE_HEADER_ENTRYPOINT, ResourceHeader,
    ResourceHeaderKind, ResourceHeaderLine, ResourceIdValue, read_resource_header,
};
