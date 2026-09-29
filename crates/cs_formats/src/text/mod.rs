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
//! * [`placeholder`] resolves the `<NAME>` references of a parsed list
//!   through the section-local and global `V`/`G` definitions, reporting an
//!   unresolved name instead of guessing one (task #370).
//! * [`resource_header`] reads the `#define NAME value` resource-id headers
//!   ([`dialect::TextDialect::ResourceHeader`]) that name the same ids
//!   [`crate::pe_resources`] walks in a PE resource directory (stage F12-B).
//! * [`records`] transcribes the record kinds and the field lists the keyed
//!   list members document in their own comments — names and structure only,
//!   no types (task #371, keyed F12-I).
//!
//! The survey, the design decisions and the recorded unknowns are written
//! down in
//! `docs/findings/2026-09-28-f12-a-text-dialects-and-lossless-config-nodes.md`.
//! What the original reader does with a *name* — case, indentation, key
//! padding and field padding — was settled afterwards from the retail data
//! itself and is in
//! `docs/findings/2026-09-29-t351-keyed-list-reading-rules.md`; the
//! `keyed_list` module doc and [`keyed_list::Field::value`] carry the rules
//! (R1-R4) and name the ones that stayed unknown.
//! The fixtures exercised by the tests are newly authored; nothing here is
//! derived from original game text.

pub mod dialect;
pub mod keyed_list;
pub mod lines;
pub mod placeholder;
pub mod records;
pub mod resource_header;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_f12_b;
#[cfg(test)]
mod tests_f12_e;
#[cfg(test)]
mod tests_f12_i;
#[cfg(test)]
mod tests_t351;

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
pub use placeholder::{
    PLACEHOLDER_ENTRYPOINT, PlaceholderAccounting, PlaceholderDefinition, PlaceholderReference,
    PlaceholderScope, PlaceholderTable, ResolvedPlaceholder, definition_scope, placeholder_name,
    read_placeholders, scan_placeholders,
};
pub use records::{
    BOOL_MARKER_NOTE, BUTTON_COLOR_NOTE, DocumentedField, RecordKind, documented_fields,
    documented_scrapbook_fields, optional_field_count,
};
pub use resource_header::{
    Define, HeaderLookup, MAX_RESOURCE_ID, RESOURCE_HEADER_ENTRYPOINT, ResourceHeader,
    ResourceHeaderKind, ResourceHeaderLine, ResourceIdValue, read_resource_header,
};
