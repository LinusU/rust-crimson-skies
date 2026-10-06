//! Lossless configuration documents with provenance and key accounting
//! (`specs/F12-text-configuration-strings-and-pe-resources.md`, stages
//! `### F12-A`, `### F12-B` and `### F12-C`).
//!
//! A [`ConfigDocument`] is one configuration member turned into owned
//! nodes, one per line, that keep every byte ([`ConfigDocument::reassemble`]
//! gives the member back) and remember where they came from
//! ([`ConfigDocument::source`], [`ConfigNode::offset`]). It is built only
//! for a member the dialect inventory routes to a dialect with a reader
//! ([`cs_formats::text::dialect_for_member`]); anything else is a
//! [`ConfigError`], never a best-effort parse.
//!
//! Stage F12-C wires the F12-B readers into their consumers. [`resolve_tunings`]
//! turns a list of declared [`FieldBinding`]s into checked tuning constants
//! against a document, booking each lookup so the document's accounting
//! reflects what the consumers used; [`StringCatalog`] reads the localizable
//! strings of a PE image through [`cs_formats::read_pe_resources`] and answers
//! `(id, language)` lookups with provenance, retaining the leaves no string
//! reader owns. Neither path loads an image or interprets a language it has not
//! measured.
//!
//! Task #370 adds the `<NAME>` placeholder pass: [`ConfigDocument::read`]
//! resolves every reference of the member against its section-local and
//! global `V`/`G` definitions — the rule task #351 measured — and keeps the
//! result in [`ConfigDocument::placeholders`]. It is a read-only second pass:
//! the raw entries stay as written, an unresolved name is reported and never
//! replaced, and a resolved value is still bytes until a consumer declares a
//! [`FieldSpec`] for it.
//!
//! Consumers look keys up through the document, which counts what they
//! used: every entry nobody consumed is retained and reported by
//! [`ConfigDocument::accounting`] (spec F12, non-negotiable #5). Names —
//! the entry key and the section name — are compared **without regard to
//! ASCII case** ([`ConfigDocument::lookup`]), which task #351 established
//! from the retail data: every UI script binds its objects by names written
//! in a different case than the layout's keys. The bytes are never changed,
//! so a lookup still reports the entry as written; two entries answering one
//! lookup fail visibly as [`Lookup::Ambiguous`].
//!
//! Values stay raw here: [`TuningSchema`] turns a declared field into a
//! checked, typed [`Tuning`], and a field nothing is known about is never
//! converted.
//!
//! Task #371 adds the record-kind schema: the keyed list members document
//! the field list of every record kind in their own comments
//! ([`cs_formats::text::records`]), but no type, unit, signedness or range.
//! [`RecordSchema`] joins the two — it declares, per observed position, the
//! documented field name (where the documented list and the shipped data
//! establish it) and the [`FieldKind`] the retail data measures, or
//! [`FieldKind::Unknown`] where nothing establishes it. A value of an
//! unknown kind stays raw and is counted by
//! [`RecordView::accounting`]; nothing here converts it. What the shipped
//! data does *not* match, and why, is recorded in
//! `docs/findings/2026-09-29-f12-i-record-kind-schemas.md`.

use std::collections::BTreeSet;
use std::fmt;

use cs_formats::AllocationBudget;
use cs_formats::ParseContext;
use cs_formats::error::ParseError;
use cs_formats::text::{
    DialectReader, DocumentedField, Fields, KeyedList, LineKind, LineTerminator,
    PlaceholderAccounting, PlaceholderScope, PlaceholderTable, QuoteIssue, RecordKind,
    ResourceHeader, TextDialect, Unclassified, definition_scope, dialect_for_member,
    documented_fields, documented_scrapbook_fields, read_keyed_list, read_placeholders,
};
use cs_formats::{PeError, PeResources, RT_STRING, ResourceKey, ResourceLeaf, read_pe_resources};
use cs_types::asset_id::SourceSpan;
use cs_types::evidence::ClaimStatus;

/// Entrypoint label [`ConfigDocument::read`] scopes the parse that books
/// its owned nodes with.
const CONFIG_ENTRYPOINT: &str = "content.config_document";

/// One configuration member as lossless nodes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigDocument {
    source: SourceSpan,
    dialect: TextDialect,
    nodes: Vec<ConfigNode>,
    consumed: Vec<bool>,
    placeholders: PlaceholderTable,
}

/// One line of a [`ConfigDocument`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigNode {
    /// 1-based line number.
    pub line: u64,
    /// Offset of the line's first byte inside the member bytes (the
    /// decoded member, for a compressed archive member).
    pub offset: u64,
    /// The line's content, terminator excluded, verbatim.
    pub content: Vec<u8>,
    /// What ended the line.
    pub terminator: LineTerminator,
    /// What the line is.
    pub kind: ConfigNodeKind,
}

/// The classification of a [`ConfigNode`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigNodeKind {
    /// A blank line.
    Blank,
    /// A whole-line comment.
    Comment,
    /// A section header with its name.
    Section {
        /// The bytes between the brackets.
        name: Vec<u8>,
    },
    /// A `key=value` entry.
    Entry(ConfigEntry),
    /// A line no observed rule explains.
    Unclassified {
        /// Why.
        reason: Unclassified,
    },
}

/// A `key=value` entry with the section it belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigEntry {
    /// The 1-based line the entry came from, so a converted value can name
    /// where in the member it was read.
    pub line: u64,
    /// The name of the section header the entry follows, `None` before the
    /// first header.
    pub section: Option<Vec<u8>>,
    /// The key, blank bytes around it removed.
    pub key: Vec<u8>,
    /// The value, verbatim.
    pub value: RawValue,
}

/// An entry's value, uninterpreted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawValue {
    /// The value split into fields.
    Fields(Vec<RawField>),
    /// A value whose quoting the dialect's observed rules do not explain,
    /// kept whole.
    Unsplit {
        /// The bytes after the `=`.
        raw: Vec<u8>,
        /// What was not understood.
        issue: QuoteIssue,
    },
}

/// One field of a [`RawValue::Fields`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawField {
    /// The field as written, quotes included.
    pub raw: Vec<u8>,
    /// Whether it was enclosed in quotes.
    pub quoted: bool,
}

impl RawField {
    /// The field without its enclosing quotes, blank bytes included.
    pub fn text(&self) -> &[u8] {
        if self.quoted {
            &self.raw[1..self.raw.len() - 1]
        } else {
            &self.raw
        }
    }

    /// The field as the original reader hands it to a consumer: the blank
    /// bytes around it dropped, then the enclosing quotes removed (**R4**).
    ///
    /// This is the form a name is compared in, so it is what
    /// [`TuningSchema::tune`] reads. `raw` keeps the bytes as written, so
    /// [`ConfigDocument::reassemble`] is unaffected. A blank byte *inside* a
    /// quoted field is kept, because a quoted field always begins and ends
    /// with its quotes: a blank outside them is quoting the survey never
    /// observed and the value stays
    /// [`RawValue::Unsplit`]/[`QuoteIssue::TextAfterClosingQuote`].
    pub fn value(&self) -> &[u8] {
        let trimmed = trim(&self.raw);
        if self.quoted {
            &trimmed[1..trimmed.len() - 1]
        } else {
            trimmed
        }
    }
}

/// The answer to one [`ConfigDocument::lookup`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lookup<'a> {
    /// No entry has this section and key.
    Missing,
    /// Exactly one entry, now counted as consumed.
    Found(&'a ConfigEntry),
    /// This many entries share the section and key; none is consumed.
    Ambiguous(usize),
}

/// How much of a document its consumers used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyAccounting {
    /// Entries in the document.
    pub entries: usize,
    /// Entries a lookup returned.
    pub consumed: usize,
    /// Entries no lookup returned: retained, and a parity blocker when a
    /// gameplay-critical member has any.
    pub unconsumed: usize,
    /// Lines no observed rule explains.
    pub unclassified_lines: usize,
    /// Entries whose value could not be split into fields.
    pub unsplit_values: usize,
}

/// Why a member did not become a [`ConfigDocument`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// The dialect inventory covers no such member.
    UnknownDialect {
        /// The member's source.
        source: Box<SourceSpan>,
    },
    /// The member's dialect has no configuration reader.
    NoReader {
        /// The member's source.
        source: Box<SourceSpan>,
        /// Its dialect.
        dialect: TextDialect,
        /// The spec stage that owns its reader.
        stage: &'static str,
    },
    /// The bytes handed in are not as long as the source says.
    LengthMismatch {
        /// The member's source.
        source: Box<SourceSpan>,
        /// The byte count handed in.
        actual: u64,
    },
    /// The reader refused the bytes (allocation budget).
    Parse(ParseError),
}

impl ConfigError {
    /// Stable, machine-matchable label.
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownDialect { .. } => "unknown_dialect",
            Self::NoReader { .. } => "no_reader",
            Self::LengthMismatch { .. } => "length_mismatch",
            Self::Parse(_) => "parse",
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownDialect { source } => {
                write!(f, "{source}: no observed dialect covers this member")
            }
            Self::NoReader {
                source,
                dialect,
                stage,
            } => write!(
                f,
                "{source}: dialect {} has no configuration reader (owned by {stage})",
                dialect.code()
            ),
            Self::LengthMismatch { source, actual } => write!(
                f,
                "{source}: {actual} bytes handed in for a source of {} bytes",
                source.length()
            ),
            Self::Parse(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl ConfigDocument {
    /// Reads the member `source` describes from `bytes` (the member's
    /// whole, decoded contents) under the dialect the inventory observed it
    /// with.
    ///
    /// # Errors
    ///
    /// [`ConfigError::UnknownDialect`] for a member the inventory does not
    /// cover (an extension alone routes nothing),
    /// [`ConfigError::NoReader`] for a dialect without a configuration
    /// reader, [`ConfigError::LengthMismatch`] when `bytes` is not the
    /// source's length and [`ConfigError::Parse`] when the node tables or
    /// the buffers the owned nodes copy do not fit `context`'s
    /// allocation budget.
    pub fn read(
        context: &mut ParseContext,
        source: SourceSpan,
        bytes: &[u8],
    ) -> Result<Self, ConfigError> {
        let Some(dialect) = dialect_for_member(source.container_path(), source.member_key()) else {
            return Err(ConfigError::UnknownDialect {
                source: Box::new(source),
            });
        };
        if let DialectReader::Deferred { stage, .. } = dialect.record().reader {
            return Err(ConfigError::NoReader {
                source: Box::new(source),
                dialect,
                stage,
            });
        }
        if bytes.len() as u64 != source.length() {
            return Err(ConfigError::LengthMismatch {
                source: Box::new(source),
                actual: bytes.len() as u64,
            });
        }
        let list = read_keyed_list(context, bytes).map_err(ConfigError::Parse)?;
        let placeholders = read_placeholders(context, &list).map_err(ConfigError::Parse)?;
        context
            .parse(CONFIG_ENTRYPOINT, bytes, |_reader, allocation, _| {
                Self::from_keyed_list(allocation, source, dialect, &list, placeholders)
            })
            .map_err(ConfigError::Parse)
    }

    /// Builds the owned nodes, booking every buffer they allocate — the
    /// node rows, the copied line bytes, the key, section, field and
    /// value copies and the consumed flags — against `allocation`, on top
    /// of what the keyed list reader booked for the borrowed parse. A
    /// document whose copies do not fit the budget is refused as
    /// [`ConfigError::Parse`] instead of allocating them.
    fn from_keyed_list(
        allocation: &mut AllocationBudget,
        source: SourceSpan,
        dialect: TextDialect,
        list: &KeyedList<'_>,
        placeholders: PlaceholderTable,
    ) -> Result<Self, ParseError> {
        let mut section: Option<Vec<u8>> = None;
        allocation.reserve(
            "nodes",
            0,
            list.lines().len() as u64,
            std::mem::size_of::<ConfigNode>() as u64,
        )?;
        let mut nodes = Vec::with_capacity(list.lines().len());
        for line in list.lines() {
            let at = line.line.offset;
            allocation.reserve("node_content", at, line.content.len() as u64, 1)?;
            let kind = match &line.kind {
                LineKind::Blank => ConfigNodeKind::Blank,
                LineKind::Comment { .. } => ConfigNodeKind::Comment,
                LineKind::Section { name } => {
                    allocation.reserve("section_name", at, name.len() as u64, 1)?;
                    let name = name.to_vec();
                    allocation.reserve("section_current", at, name.len() as u64, 1)?;
                    section = Some(name.clone());
                    ConfigNodeKind::Section { name }
                }
                LineKind::Entry(entry) => {
                    allocation.reserve("entry_key", at, entry.key.len() as u64, 1)?;
                    let key = entry.key.to_vec();
                    allocation.reserve(
                        "entry_section",
                        at,
                        section.as_ref().map_or(0, |name| name.len() as u64),
                        1,
                    )?;
                    let section = section.clone();
                    let value = match &entry.fields {
                        Fields::Split(fields) => {
                            allocation.reserve(
                                "entry_fields",
                                at,
                                fields.len() as u64,
                                std::mem::size_of::<RawField>() as u64,
                            )?;
                            let mut owned = Vec::with_capacity(fields.len());
                            for field in fields {
                                allocation.reserve("entry_field", at, field.raw.len() as u64, 1)?;
                                owned.push(RawField {
                                    raw: field.raw.to_vec(),
                                    quoted: field.quoted,
                                });
                            }
                            RawValue::Fields(owned)
                        }
                        Fields::Unsplit { issue, .. } => {
                            allocation.reserve("entry_value", at, entry.value.len() as u64, 1)?;
                            RawValue::Unsplit {
                                raw: entry.value.to_vec(),
                                issue: *issue,
                            }
                        }
                    };
                    ConfigNodeKind::Entry(ConfigEntry {
                        line: line.line.number,
                        section,
                        key,
                        value,
                    })
                }
                LineKind::Unclassified { reason } => {
                    ConfigNodeKind::Unclassified { reason: *reason }
                }
            };
            nodes.push(ConfigNode {
                line: line.line.number,
                offset: line.line.offset,
                content: line.content.to_vec(),
                terminator: line.terminator(),
                kind,
            });
        }
        allocation.reserve("consumed", 0, nodes.len() as u64, 1)?;
        let consumed = vec![false; nodes.len()];
        Ok(Self {
            source,
            dialect,
            nodes,
            consumed,
            placeholders,
        })
    }

    /// Where the member came from.
    pub fn source(&self) -> &SourceSpan {
        &self.source
    }

    /// The dialect it was read as.
    pub fn dialect(&self) -> TextDialect {
        self.dialect
    }

    /// The `<NAME>` references of the member and the scoped name table they
    /// were resolved against (task #370, keyed `F12-E`).
    ///
    /// The pass is built during [`Self::read`] from the same keyed list the
    /// owned nodes come from, so the two never disagree about a line. It does
    /// not change a byte: the raw entry values are still in the nodes, an
    /// unresolved name is reported by
    /// [`PlaceholderTable::unresolved`](cs_formats::text::PlaceholderTable::unresolved)
    /// rather than replaced, and turning a resolved value into a number is
    /// still [`TuningSchema`]'s job against a declared [`FieldSpec`].
    pub fn placeholders(&self) -> &PlaceholderTable {
        &self.placeholders
    }

    /// Every line, in member order.
    pub fn nodes(&self) -> &[ConfigNode] {
        &self.nodes
    }

    /// Every entry, in member order.
    pub fn entries(&self) -> impl Iterator<Item = &ConfigEntry> {
        self.nodes.iter().filter_map(|node| match &node.kind {
            ConfigNodeKind::Entry(entry) => Some(entry),
            _ => None,
        })
    }

    /// Looks up the entry `key` in `section` (`None`: before the first
    /// header) and counts it as consumed when exactly one entry answers.
    ///
    /// Both names compare **without regard to ASCII case** (**R1**,
    /// established by task #351 from the retail data: the 34 UI scripts of
    /// `crimson.rof` bind their objects through names that differ in case
    /// from the layout's keys, and none of the 402 hand-written names match
    /// a key exactly). Nothing is folded in the stored bytes: the returned
    /// entry carries the key and section as the member wrote them, and the
    /// fold is ASCII-only because what the original does with bytes above
    /// `0x7F` is not established (a Windows-1252 name is neither matched
    /// nor rejected differently from any other byte).
    ///
    /// Folding can make two keys collide that were distinct bytes; both
    /// then answer the lookup and it is [`Lookup::Ambiguous`], never a
    /// choice between them. No case-insensitive duplicate key exists in
    /// either surveyed member.
    pub fn lookup(&mut self, section: Option<&[u8]>, key: &[u8]) -> Lookup<'_> {
        let matching: Vec<usize> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                matches!(&node.kind, ConfigNodeKind::Entry(entry)
                    if names_match(entry.section.as_deref(), section)
                        && entry.key.eq_ignore_ascii_case(key))
            })
            .map(|(index, _)| index)
            .collect();
        match matching.as_slice() {
            [] => Lookup::Missing,
            [index] => {
                self.consumed[*index] = true;
                match &self.nodes[*index].kind {
                    ConfigNodeKind::Entry(entry) => Lookup::Found(entry),
                    _ => unreachable!("only entries match"),
                }
            }
            many => Lookup::Ambiguous(many.len()),
        }
    }

    /// The entries no lookup returned, in member order, with their nodes.
    pub fn unconsumed(&self) -> impl Iterator<Item = (&ConfigNode, &ConfigEntry)> {
        self.nodes
            .iter()
            .zip(&self.consumed)
            .filter_map(|(node, consumed)| match &node.kind {
                ConfigNodeKind::Entry(entry) if !consumed => Some((node, entry)),
                _ => None,
            })
    }

    /// Counts of entries, consumed and unconsumed entries, unclassified
    /// lines and unsplit values.
    pub fn accounting(&self) -> KeyAccounting {
        let mut accounting = KeyAccounting::default();
        for (node, consumed) in self.nodes.iter().zip(&self.consumed) {
            match &node.kind {
                ConfigNodeKind::Entry(entry) => {
                    accounting.entries += 1;
                    if *consumed {
                        accounting.consumed += 1;
                    } else {
                        accounting.unconsumed += 1;
                    }
                    if matches!(entry.value, RawValue::Unsplit { .. }) {
                        accounting.unsplit_values += 1;
                    }
                }
                ConfigNodeKind::Unclassified { .. } => accounting.unclassified_lines += 1,
                _ => {}
            }
        }
        accounting
    }

    /// The member bytes, rebuilt from the nodes.
    pub fn reassemble(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for node in &self.nodes {
            out.extend_from_slice(&node.content);
            out.extend_from_slice(node.terminator.bytes());
        }
        out
    }
}

// ---------------------------------------------------------------------------
// The string catalog (stage F12-C)
// ---------------------------------------------------------------------------

/// One localizable string of a PE image: its stable id, the language the
/// image stored it under, the block's code page, the exact code units, the
/// text when those units decode, and where the block's bytes are.
///
/// The id is the identity a localized installation keeps while the display
/// text changes (spec F12 acceptance test AC04): a translation ships the same
/// id with different code units, so a consumer looks the id up rather than
/// the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringRow {
    /// The stable string id: `(block - 1) * 16 + index`
    /// ([`cs_formats::string_id`]).
    pub id: u32,
    /// The third-level language id, verbatim.
    pub language: u32,
    /// The data entry's code page, verbatim. `0` is the resource compiler's
    /// "no code page" value and is **not** treated as a default.
    pub code_page: u32,
    /// The exact UTF-16 code units, kept even when they do not decode.
    pub code_units: Vec<u16>,
    /// The text, or `None` when the units hold an unpaired surrogate. A
    /// missing text is a recorded condition, never a replaced one.
    pub text: Option<String>,
    /// Where the block's bytes are inside the image the catalog was read
    /// from.
    pub span: SourceSpan,
}

/// The answer to one [`StringCatalog::resolve`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StringLookup<'a> {
    /// No string has this id in the requested language.
    Missing,
    /// Exactly one string.
    Found(&'a StringRow),
    /// This many strings share the id and language; none is returned, because
    /// choosing between them would hide a contradiction.
    Ambiguous(usize),
}

/// What one PE image's resource tree yielded for localization.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StringAccounting {
    /// Localizable strings (the `RT_STRING` units).
    pub strings: usize,
    /// Units whose code units do not decode (an unpaired surrogate).
    pub undecodable: usize,
    /// Leaves that are not three-level `RT_STRING` blocks: retained beside
    /// the strings and never read as strings (spec F12, non-negotiable #5
    /// applied to the resource tree).
    pub other_leaves: usize,
    /// `(id, language)` pairs more than one string answers. A translation
    /// that ships two strings for one id is reported, not silently merged.
    pub duplicate_ids: usize,
}

/// Every localizable string of one PE image, with the resources it was read
/// from and the leaves no string reader owns.
///
/// It is built only from the bounded, cycle-checked [`read_pe_resources`]:
/// the image is parsed as inert data and never loaded, so a malformed
/// resource offset is a [`StringCatalogError`] rather than a platform load
/// (spec F12 acceptance test AC03).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringCatalog {
    source: SourceSpan,
    resources: PeResources,
    strings: Vec<StringRow>,
}

impl StringCatalog {
    /// Reads `bytes` as the PE image `source` describes and keeps every
    /// localizable string, retaining the non-string leaves beside them.
    ///
    /// # Errors
    ///
    /// [`StringCatalogError::LengthMismatch`] when `bytes` is not the
    /// source's length, and [`StringCatalogError::Pe`] when the resource
    /// reader refuses the image (a malformed, hostile or over-budget one).
    pub fn read(
        context: &mut ParseContext,
        source: SourceSpan,
        bytes: &[u8],
    ) -> Result<Self, StringCatalogError> {
        if bytes.len() as u64 != source.length() {
            return Err(StringCatalogError::LengthMismatch {
                source: Box::new(source),
                actual: bytes.len() as u64,
            });
        }
        let resources = read_pe_resources(context, bytes).map_err(StringCatalogError::Pe)?;
        Ok(Self::from_resources(source, resources))
    }

    /// Builds the catalog from an already-read image, so a caller with a
    /// [`PeResources`] does not parse the bytes twice.
    pub fn from_resources(source: SourceSpan, resources: PeResources) -> Self {
        let mut strings = Vec::new();
        for block in resources.strings() {
            // The reader checked this extent against the section's raw bytes
            // before it decoded the block, so the span is a checked range.
            let span = SourceSpan::new(
                source.install_sha256(),
                source.container_path(),
                source.member_key(),
                block.data.file_offset,
                u64::from(block.data.size),
                source.member_sha256(),
            )
            .expect("the resource reader checked this extent against the image");
            for unit in &block.units {
                strings.push(StringRow {
                    id: unit.id,
                    language: block.language,
                    code_page: block.code_page,
                    code_units: unit.code_units.clone(),
                    text: unit.text.clone(),
                    span: span.clone(),
                });
            }
        }
        Self {
            source,
            resources,
            strings,
        }
    }

    /// The image the strings were read from.
    pub fn source(&self) -> &SourceSpan {
        &self.source
    }

    /// Everything the resource reader read out of the image, for a caller
    /// that needs the header layout or the whole leaf list.
    pub fn resources(&self) -> &PeResources {
        &self.resources
    }

    /// Every string, in block-then-unit order, with duplicates included.
    pub fn rows(&self) -> &[StringRow] {
        &self.strings
    }

    /// The leaves that are not three-level `RT_STRING` blocks: resource types
    /// and payloads no string reader owns. Retained, never interpreted.
    pub fn other_leaves(&self) -> impl Iterator<Item = &ResourceLeaf> {
        self.resources
            .leaves()
            .iter()
            .filter(|leaf| !is_string_leaf(leaf))
    }

    /// Every language id the image's blocks carry, deduplicated and sorted.
    pub fn languages(&self) -> Vec<u32> {
        let mut languages: Vec<u32> = self
            .resources
            .strings()
            .iter()
            .map(|block| block.language)
            .collect();
        languages.sort_unstable();
        languages.dedup();
        languages
    }

    /// The one string with id `id` in `language` (any language when `None`).
    ///
    /// Two strings answering one `(id, language)` are
    /// [`StringLookup::Ambiguous`], never a silent choice, matching
    /// [`ConfigDocument::lookup`].
    pub fn resolve(&self, id: u32, language: Option<u32>) -> StringLookup<'_> {
        let mut count = 0usize;
        let mut found: Option<&StringRow> = None;
        for row in &self.strings {
            if row.id == id && language.is_none_or(|wanted| row.language == wanted) {
                count += 1;
                if found.is_none() {
                    found = Some(row);
                }
            }
        }
        match (found, count) {
            (None, _) => StringLookup::Missing,
            (Some(row), 1) => StringLookup::Found(row),
            (_, many) => StringLookup::Ambiguous(many),
        }
    }

    /// What the image yielded: string, undecodable, non-string-leaf and
    /// duplicate-id counts.
    pub fn accounting(&self) -> StringAccounting {
        let mut pairs: Vec<(u32, u32)> = self
            .strings
            .iter()
            .map(|row| (row.language, row.id))
            .collect();
        pairs.sort_unstable();
        let mut duplicate_ids = 0usize;
        let mut index = 0usize;
        while index < pairs.len() {
            let mut end = index + 1;
            while end < pairs.len() && pairs[end] == pairs[index] {
                end += 1;
            }
            if end - index > 1 {
                duplicate_ids += 1;
            }
            index = end;
        }
        StringAccounting {
            strings: self.strings.len(),
            undecodable: self.strings.iter().filter(|row| row.text.is_none()).count(),
            other_leaves: self.other_leaves().count(),
            duplicate_ids,
        }
    }
}

/// Whether a leaf is a three-level `RT_STRING` block: the only shape the
/// resource reader decodes as strings.
///
/// This must be exactly the reader's own rule (`cs_formats`'s
/// `string_leaf`): **all three** levels are ids and the outermost is
/// `RT_STRING`. A three-level leaf under `RT_STRING` whose second or third
/// level is a *name* is not a block — the reader retains it as a plain leaf —
/// so counting it as a string leaf here would drop it from the accounting
/// instead of retaining it (spec F12, non-negotiable #5).
fn is_string_leaf(leaf: &ResourceLeaf) -> bool {
    leaf.path.len() == 3
        && leaf.key(0).and_then(ResourceKey::id) == Some(RT_STRING)
        && leaf.key(1).and_then(ResourceKey::id).is_some()
        && leaf.key(2).and_then(ResourceKey::id).is_some()
}

/// Why a PE image did not become a [`StringCatalog`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StringCatalogError {
    /// The bytes handed in are not as long as the source says.
    LengthMismatch {
        /// The image's source.
        source: Box<SourceSpan>,
        /// The byte count handed in.
        actual: u64,
    },
    /// The bounded resource reader refused the image.
    Pe(PeError),
}

impl StringCatalogError {
    /// Stable, machine-matchable label.
    pub fn code(&self) -> &'static str {
        match self {
            Self::LengthMismatch { .. } => "length_mismatch",
            Self::Pe(error) => error.code(),
        }
    }
}

impl fmt::Display for StringCatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LengthMismatch { source, actual } => write!(
                f,
                "{source}: {actual} bytes handed in for a source of {} bytes",
                source.length()
            ),
            Self::Pe(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for StringCatalogError {}

/// Compares two section names as the original reader does (**R1**): the
/// same absence on both sides, otherwise ASCII case does not matter. A
/// `None` name (an entry before the first header) is never equal to a named
/// one, whatever its case.
fn names_match(entry_section: Option<&[u8]>, wanted: Option<&[u8]>) -> bool {
    match (entry_section, wanted) {
        (None, None) => true,
        (Some(entry), Some(wanted)) => entry.eq_ignore_ascii_case(wanted),
        _ => false,
    }
}

/// How wide a configuration value is, and whether it is signed.
///
/// Width and signedness belong to the *schema*, not to the value: a value is
/// only ever converted against a [`TuningSchema`] that declares both, so a
/// consumer cannot read a negative count or an over-wide tuning number into a
/// Rust type that would silently accept it (spec F12, non-negotiable #2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ValueWidth {
    /// `i8` / `u8`.
    Bits8,
    /// `i16` / `u16`.
    Bits16,
    /// `i32` / `u32`.
    Bits32,
    /// `i64` / `u64`.
    Bits64,
    /// An IEEE-754 `f32`.
    Float32,
    /// An IEEE-754 `f64`.
    Float64,
}

impl ValueWidth {
    /// Whether the type is a floating-point one, where a finite value is a
    /// separate question from a range question.
    pub const fn is_float(self) -> bool {
        matches!(self, Self::Float32 | Self::Float64)
    }

    /// The whole-number span the width addresses, as `(minimum, maximum)` for
    /// a signed value. A float width has no such span: the numeric contract's
    /// finiteness rule covers it instead
    /// (`docs/contracts/IDENTITY-CONTENT.md`, "Numeric contract").
    pub const fn signed_range(self) -> Option<(i128, i128)> {
        match self {
            Self::Bits8 => Some((-128, 127)),
            Self::Bits16 => Some((-32_768, 32_767)),
            Self::Bits32 => Some((-2_147_483_648, 2_147_483_647)),
            Self::Bits64 => Some((-9_223_372_036_854_775_808, 9_223_372_036_854_775_807)),
            Self::Float32 | Self::Float64 => None,
        }
    }

    /// The whole-number span the width addresses, as `(minimum, maximum)` for
    /// an unsigned value.
    pub const fn unsigned_range(self) -> Option<(u128, u128)> {
        match self {
            Self::Bits8 => Some((0, 255)),
            Self::Bits16 => Some((0, 65_535)),
            Self::Bits32 => Some((0, 4_294_967_295)),
            Self::Bits64 => Some((0, 18_446_744_073_709_551_615)),
            Self::Float32 | Self::Float64 => None,
        }
    }
}

/// One declared configuration value: its width, its signedness and the range
/// a consumer is allowed to hand the simulation.
///
/// The numeric contract requires that tuning float values be "finite and
/// within measured/approved ranges". `min` and `max` are *that* approved
/// range; when they are `None` the value is only checked for finiteness, and a
/// consumer that needs a bound is expected to declare one rather than accept
/// whatever the file says.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldSpec {
    /// The field's width and, for a whole-number width, whether it is signed.
    pub width: ValueWidth,
    /// Whether a whole-number value may be negative.
    pub signed: bool,
    /// The smallest accepted value, or `None` for no lower bound.
    pub min: Option<f64>,
    /// The largest accepted value, or `None` for no upper bound.
    pub max: Option<f64>,
    /// What the value means, for the diagnostic a consumer shows. Never
    /// interpreted by this module.
    pub unit: &'static str,
}

impl FieldSpec {
    /// A whole-number field of `width`, signed or not.
    pub const fn integer(width: ValueWidth, signed: bool) -> Self {
        Self {
            width,
            signed,
            min: None,
            max: None,
            unit: "",
        }
    }

    /// A float field of `width`, checked for finiteness only.
    pub const fn float(width: ValueWidth) -> Self {
        Self {
            width,
            signed: true,
            min: None,
            max: None,
            unit: "",
        }
    }

    /// The same field with an approved range and a unit label.
    pub const fn with_range(mut self, min: f64, max: f64, unit: &'static str) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self.unit = unit;
        self
    }
}

/// Why a value could not become a [`Tuning`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TuneError {
    /// The entry's value was not split into fields, so there is nothing to
    /// convert. The bytes stay in the document.
    Unsplittable,
    /// The value has more fields than the consumer wants, or fewer: a
    /// `FieldSpec` addresses one field, and which one is the caller's choice.
    FieldCount {
        /// Fields the value has.
        got: usize,
    },
    /// The field is not a number at all.
    NotANumber {
        /// The field's byte length. The bytes themselves stay in the document.
        len: usize,
    },
    /// The number does not fit the declared width. This is the "overflow" of
    /// AC02: `300` against an 8-bit field, or a 20-digit value against any
    /// 64-bit field.
    Overflow {
        /// The declared width.
        width: ValueWidth,
    },
    /// A negative value for a field declared unsigned. This is the "negative"
    /// of AC02.
    Negative {
        /// The declared width.
        width: ValueWidth,
    },
    /// A float that is not finite: `nan`, `inf` or an overflow to infinity.
    /// This is the "NaN" of AC02.
    NotFinite {
        /// Which condition: `nan` or a signed infinity.
        infinity: bool,
    },
    /// The value is inside its width and finite but outside the approved
    /// range, so it is not a tuning constant yet.
    OutOfRange {
        /// The inclusive lower bound of the approved range, when there is one.
        min: Option<f64>,
        /// The inclusive upper bound of the approved range, when there is one.
        max: Option<f64>,
    },
}

impl TuneError {
    /// Stable, machine-matchable label.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Unsplittable => "unsplittable",
            Self::FieldCount { .. } => "field_count",
            Self::NotANumber { .. } => "not_a_number",
            Self::Overflow { .. } => "overflow",
            Self::Negative { .. } => "negative",
            Self::NotFinite { .. } => "not_finite",
            Self::OutOfRange { .. } => "out_of_range",
        }
    }
}

impl fmt::Display for TuneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsplittable => write!(f, "the value is not split into fields"),
            Self::FieldCount { got } => {
                write!(f, "the value has {got} fields and a spec addresses one")
            }
            Self::NotANumber { len } => write!(f, "a {len}-byte field is not a number"),
            Self::Overflow { width } => {
                write!(f, "the value does not fit the declared {width:?} width")
            }
            Self::Negative { width } => {
                write!(f, "a negative value in a field declared {width:?} unsigned")
            }
            Self::NotFinite { infinity } => write!(
                f,
                "the value is {}",
                if *infinity {
                    "an infinity"
                } else {
                    "not a number"
                }
            ),
            Self::OutOfRange { min, max } => match (min, max) {
                (Some(low), Some(high)) => write!(f, "the value is outside [{low}, {high}]"),
                (Some(low), None) => write!(f, "the value is below {low}"),
                (None, Some(high)) => write!(f, "the value is above {high}"),
                (None, None) => write!(f, "the value is outside the approved range"),
            },
        }
    }
}

impl std::error::Error for TuneError {}

/// A checked, typed configuration value together with where it came from.
///
/// A `Tuning` exists only for a value the [`FieldSpec`] accepted, so its
/// presence *is* the proof that the value is in range, in width and (for a
/// float) finite. A value that fails any of those checks is a
/// [`TuneError`] and stays in the document as raw bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    /// The value, converted. For a whole-number spec this is the value as
    /// `f64`; [`Self::signed`] and [`Self::unsigned`] carry it in the declared
    /// integer type without the lossiness `f64` would introduce above 2^53.
    pub value: f64,
    /// The value as the declared whole-number width, when the spec is a
    /// signed one. `None` for a float or an unsigned field, whose value the
    /// simulation reads through [`Self::value`] or [`Self::as_unsigned`].
    pub signed: Option<i64>,
    /// The value as the declared unsigned width, when the spec is unsigned.
    pub unsigned: Option<u64>,
    /// The unit the spec declared, for a consumer's own conversion.
    pub unit: &'static str,
    /// The 1-based line the value came from.
    pub line: u64,
}

impl Tuning {
    /// The value as an unsigned whole number, or `None` when the spec is
    /// signed or floating-point.
    pub fn as_unsigned(&self) -> Option<u64> {
        self.unsigned
    }

    /// The value as a signed whole number, or `None` when the spec is
    /// unsigned or floating-point.
    pub fn as_signed(&self) -> Option<i64> {
        self.signed
    }
}

/// The parsing and checking a [`TuningSchema`] applies to one value.
pub struct TuningSchema<'a> {
    spec: FieldSpec,
    fields: &'a ConfigEntry,
}

impl<'a> TuningSchema<'a> {
    /// Prepares `spec` for `entry`, so a consumer converts a value it looked
    /// up rather than re-searching the document.
    pub fn new(spec: FieldSpec, entry: &'a ConfigEntry) -> Self {
        Self {
            spec,
            fields: entry,
        }
    }

    /// The declared spec.
    pub fn spec(&self) -> FieldSpec {
        self.spec
    }

    /// Converts field `index` of the value, or reports why it cannot be
    /// converted.
    ///
    /// This is the whole of AC02: a value is a [`Tuning`] only when it fits its
    /// declared width and signedness, is finite when it is a float, and lies
    /// inside the approved range. Everything else is an error and the bytes
    /// stay where they were.
    pub fn tune(&self, index: usize) -> Result<Tuning, TuneError> {
        let RawValue::Fields(fields) = &self.fields.value else {
            return Err(TuneError::Unsplittable);
        };
        let Some(field) = fields.get(index) else {
            return Err(TuneError::FieldCount { got: fields.len() });
        };
        // **R4**: the field as the original reader hands it over, so a value
        // written with blank padding around it is the same number as one
        // written without. `RawField::text` would keep those blanks.
        let text = field.value();
        let line = self.fields.line;
        let value = self.number(text)?;

        // A whole-number value must fit the declared width, and may only be
        // negative when the spec is signed. A float must be finite.
        if self.spec.width.is_float() {
            if !value.is_finite() {
                return Err(TuneError::NotFinite {
                    infinity: value.is_infinite(),
                });
            }
        } else {
            let integral = value.fract() == 0.0 && value.is_finite();
            if !integral {
                // A fractional value has no whole-number representation, so it
                // is an overflow of the declared type rather than a silently
                // truncated integer.
                return Err(TuneError::Overflow {
                    width: self.spec.width,
                });
            }
            if value < 0.0 {
                if !self.spec.signed {
                    return Err(TuneError::Negative {
                        width: self.spec.width,
                    });
                }
                let (low, _) = self
                    .spec
                    .width
                    .signed_range()
                    .expect("a whole-number width");
                if value < low as f64 {
                    return Err(TuneError::Overflow {
                        width: self.spec.width,
                    });
                }
            } else {
                // A non-negative value is checked against the largest value its
                // *declared* type addresses: an unsigned field stops at its
                // width's unsigned maximum, a signed one at its signed maximum.
                // So `200` is an overflow of `i8` (whose span is -128..=127)
                // even though it fits `u8`, and never becomes a `Tuning` that
                // claims the signed type could carry it.
                let high = if self.spec.signed {
                    self.spec
                        .width
                        .signed_range()
                        .expect("a whole-number width")
                        .1 as f64
                } else {
                    self.spec
                        .width
                        .unsigned_range()
                        .expect("a whole-number width")
                        .1 as f64
                };
                if value > high {
                    return Err(TuneError::Overflow {
                        width: self.spec.width,
                    });
                }
            }
        }

        // The approved range is the numeric contract's requirement for a
        // tuning float value, and it is checked for every width.
        let below = self.spec.min.is_some_and(|low| value < low);
        let above = self.spec.max.is_some_and(|high| value > high);
        if below || above {
            return Err(TuneError::OutOfRange {
                min: self.spec.min,
                max: self.spec.max,
            });
        }

        Ok(Tuning {
            value,
            signed: (!self.spec.width.is_float() && self.spec.signed).then_some(value as i64),
            unsigned: (!self.spec.width.is_float() && !self.spec.signed).then_some(value as u64),
            unit: self.spec.unit,
            line,
        })
    }

    /// The one number a field spells.
    ///
    /// Deliberately stricter than `str::parse`: the dialect's own rules are
    /// the only ones applied, so a field no rule here establishes is
    /// [`TuneError::NotANumber`] rather than a parse of a prefix.
    ///
    /// Evidence for each accepted spelling:
    ///
    /// * an optionally signed run of decimal digits — `ObservedTool`: the
    ///   F12-A survey of `LAYOUT.CSV` and `SCRAPBOOK.CSV` found decimal field
    ///   values;
    /// * a `0x`-prefixed hexadecimal value — `ObservedTool`:
    ///   [`cs_formats::text::LexicalFeature::HexValues`];
    /// * a single `.`, with digits on at least one side — `Inferred`, and
    ///   measured **absent** rather than merely unobserved. Task #369 read
    ///   every field of both members through the production readers: of 7 272
    ///   `LAYOUT.CSV` field values (3 664 of them numeric) and 7 376
    ///   `SCRAPBOOK.CSV` field values (5 098 numeric), **not one** is a
    ///   fraction. `LAYOUT.CSV` does hold 232 `.` bytes across 229 field
    ///   values, and every one of them is a file name's extension (`png`,
    ///   `Png`, `jpg`, `MPG`, `tga`); the rule refuses those for an unrelated
    ///   reason, because the stem is not a number. So this spelling is a
    ///   **designed** reading rule that no installed value needs, kept
    ///   because a tuning float must be spellable at all, and it is **not**
    ///   parity evidence: F12-D measures the original's own acceptance of a
    ///   fraction before any claim rests on it. The counts and the shapes are
    ///   recorded in
    ///   `docs/findings/2026-09-29-f12-h-fractional-configuration-values.md`.
    ///
    /// Everything else is refused as text, in particular `nan`, `inf` and an
    /// exponent, so a non-finite value cannot be *spelled* here. The
    /// finiteness rule in [`Self::tune`] is still the backstop: it catches a
    /// value whose digits overflow `f64` to an infinity.
    fn number(&self, text: &[u8]) -> Result<f64, TuneError> {
        let not_a_number = TuneError::NotANumber { len: text.len() };
        let overflow = TuneError::Overflow {
            width: self.spec.width,
        };
        if text.is_empty() {
            return Err(not_a_number);
        }
        let (negative, digits) = match text.first() {
            Some(b'-') => (true, &text[1..]),
            Some(b'+') => (false, &text[1..]),
            _ => (false, text),
        };
        if digits.is_empty() {
            return Err(not_a_number);
        }
        let magnitude = if let Some(hex) = digits
            .strip_prefix(b"0x")
            .or_else(|| digits.strip_prefix(b"0X"))
        {
            if hex.is_empty() || !hex.iter().all(u8::is_ascii_hexdigit) {
                return Err(not_a_number);
            }
            let mut value = 0f64;
            for byte in hex {
                let digit = match byte {
                    b'0'..=b'9' => f64::from(byte - b'0'),
                    b'a'..=b'f' => f64::from(byte - b'a') + 10.0,
                    _ => f64::from(byte - b'A') + 10.0,
                };
                value = value * 16.0 + digit;
                if !value.is_finite() {
                    // A hexadecimal value too wide for `f64` is an overflow of
                    // the number, reported as such rather than saturating.
                    return Err(overflow);
                }
            }
            value
        } else {
            // At most one `.`, and not both sides empty: `5.5`, `5.` and `.5`
            // read; `5..5`, `5.5.5` and a bare `.` do not.
            let mut parts = digits.splitn(3, |byte| *byte == b'.');
            let whole = parts.next().expect("a first part");
            let fraction = parts.next();
            if parts.next().is_some() {
                return Err(not_a_number);
            }
            let fraction = fraction.unwrap_or_default();
            if whole.is_empty() && fraction.is_empty() {
                return Err(not_a_number);
            }
            if !whole.iter().all(u8::is_ascii_digit) || !fraction.iter().all(u8::is_ascii_digit) {
                return Err(not_a_number);
            }
            // Accumulated digit by digit, and rejected as soon as the running
            // value stops being finite, so a 20-digit value cannot wrap into a
            // small one and slip through the range check.
            let mut value = 0f64;
            for byte in whole {
                value = value * 10.0 + f64::from(byte - b'0');
            }
            let mut scale = 1f64;
            for byte in fraction {
                scale *= 0.1;
                value += f64::from(byte - b'0') * scale;
            }
            if !value.is_finite() {
                return Err(overflow);
            }
            value
        };
        Ok(if negative { -magnitude } else { magnitude })
    }
}

/// The bytes the dialect treats as blank around a number.
const BLANK: &[u8] = b" \t";

fn trim(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !BLANK.contains(byte))
        .unwrap_or(bytes.len());
    let end = bytes[start..]
        .iter()
        .rposition(|byte| !BLANK.contains(byte))
        .map_or(start, |last| start + last + 1);
    &bytes[start..end]
}

// ---------------------------------------------------------------------------
// Declared tuning fields (stage F12-C)
// ---------------------------------------------------------------------------

/// One declared value a consumer wants out of a [`ConfigDocument`]: which
/// entry to look up, which field of it, and the [`FieldSpec`] the consumer
/// declares.
///
/// A document keeps values raw until a consumer declares what it wants, so a
/// `FieldBinding` is the schema half of the conversion: the width, signedness
/// and approved range belong to the consumer, never to the bytes (spec F12,
/// non-negotiable #2). `consumer` is a label for the report, so a missing or
/// refused value names who needed it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldBinding<'a> {
    /// The consumer that wants the value, e.g. `gun.rate`.
    pub consumer: &'a str,
    /// The section the entry belongs to, `None` before the first header.
    pub section: Option<&'a [u8]>,
    /// The entry key.
    pub key: &'a [u8],
    /// Which field of the entry's value the spec addresses.
    pub index: usize,
    /// The declared width, signedness and approved range.
    pub spec: FieldSpec,
}

/// What one [`FieldBinding`] resolved to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TuningOutcome {
    /// The value passed every check and is a tuning constant.
    Known(Tuning),
    /// No entry has the section and key.
    Missing,
    /// This many entries share the section and key; the lookup refused to
    /// choose, so nothing was converted.
    Ambiguous(usize),
    /// The entry was found and the value could not become a tuning constant.
    Refused(TuneError),
}

/// One declared field and what it resolved to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedTuning<'a> {
    /// The declaration that was resolved.
    pub binding: &'a FieldBinding<'a>,
    /// The outcome.
    pub outcome: TuningOutcome,
}

/// The result of resolving a whole declared list against one document.
///
/// A value is [`TuningOutcome::Known`] only for a value the declared spec
/// accepted; every other outcome is explicit, and the bytes stay in the
/// document. The entries no binding consumed are still visible through
/// [`ConfigDocument::accounting`], so the caller can gate parity on them
/// (spec F12, non-negotiable #5).
#[derive(Clone, Debug, PartialEq)]
pub struct TuningReport<'a> {
    resolved: Vec<ResolvedTuning<'a>>,
}

impl<'a> TuningReport<'a> {
    /// Every declared field, in declaration order.
    pub fn resolved(&self) -> &[ResolvedTuning<'a>] {
        &self.resolved
    }

    /// Whether every declared field became a tuning constant.
    pub fn all_known(&self) -> bool {
        self.resolved
            .iter()
            .all(|entry| matches!(entry.outcome, TuningOutcome::Known(_)))
    }

    /// The declarations that did not become a tuning constant, with why.
    pub fn failures(&self) -> impl Iterator<Item = &ResolvedTuning<'a>> {
        self.resolved
            .iter()
            .filter(|entry| !matches!(entry.outcome, TuningOutcome::Known(_)))
    }
}

/// Resolves every declared field against `document`, counting each found
/// entry as consumed through [`ConfigDocument::lookup`] so the document's own
/// accounting reflects what the consumers used.
pub fn resolve_tunings<'a>(
    document: &mut ConfigDocument,
    bindings: &'a [FieldBinding<'a>],
) -> TuningReport<'a> {
    let mut resolved = Vec::with_capacity(bindings.len());
    for binding in bindings {
        let outcome = match document.lookup(binding.section, binding.key) {
            Lookup::Missing => TuningOutcome::Missing,
            Lookup::Ambiguous(count) => TuningOutcome::Ambiguous(count),
            Lookup::Found(entry) => {
                match TuningSchema::new(binding.spec, entry).tune(binding.index) {
                    Ok(tuning) => TuningOutcome::Known(tuning),
                    Err(error) => TuningOutcome::Refused(error),
                }
            }
        };
        resolved.push(ResolvedTuning { binding, outcome });
    }
    TuningReport { resolved }
}

// ---------------------------------------------------------------------------
// Record kinds and their documented field lists (stage F12-I)
// ---------------------------------------------------------------------------

/// What kind of value one field of a configuration record holds.
///
/// The keyed list members document their *field lists* and nothing else:
/// not one type, unit, signedness or range is stated, so these kinds claim
/// only what the 636 object records of `ASSETS/LAYOUT.CSV` and the 461
/// items of `ASSETS/SCRAPBOOK.CSV` establish. A kind nothing establishes is
/// [`FieldKind::Unknown`], and a value of an unknown kind stays raw and is
/// counted ([`RecordView::accounting`]); it is never converted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldKind {
    /// A whole number. `signed` is true when the retail data spells a
    /// negative value in that position.
    Integer {
        /// Whether the field may be negative.
        signed: bool,
    },
    /// A documented boolean (a `?` in the comment) spelled `0` or `1`.
    Bool,
    /// An eight-hex-digit `0x…` colour literal.
    Color,
    /// A file or asset name (`*.png`, `*.MPG`, …).
    Path,
    /// A name: a UI name, a resource-id name or an `IDS_…` string name.
    Name,
    /// Nothing establishes a kind; the value stays raw and is counted.
    Unknown,
}

impl FieldKind {
    /// Stable, machine-matchable label.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Integer { .. } => "integer",
            Self::Bool => "bool",
            Self::Color => "color",
            Self::Path => "path",
            Self::Name => "name",
            Self::Unknown => "unknown",
        }
    }

    /// Whether a value of this kind can be typed at all. A `false` kind is
    /// retained and counted, never converted.
    pub const fn is_known(self) -> bool {
        !matches!(self, Self::Unknown)
    }
}

/// One field of one record kind's typed schema.
///
/// `name` is the documented field this position carries, or `None` when the
/// documented list and the shipped data leave the position's identity
/// unresolved. `None` is a recorded unknown: [`RecordView::accounting`]
/// counts it only when the *kind* is also unknown. `evidence` records how the
/// entry was established, weakest link first:
/// [`ClaimStatus::Documented`] when the documented list and the data agree
/// position for position, [`ClaimStatus::Inferred`] when the documented
/// list leaves the field after the omissions the shipped data forces,
/// [`ClaimStatus::ObservedTool`] when only the kind is measured (the name
/// is not), and [`ClaimStatus::Unknown`] when the kind is not established:
/// the name may still stand, as it does for the sound object's documented
/// fields and the scrapbook's three positions no kind covers, but no value
/// of that position is ever converted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordFieldSpec {
    /// The observed position, `0` being the record letter / first field.
    pub position: usize,
    /// The documented field name, or `None` for a recorded unknown.
    pub name: Option<&'static str>,
    /// The kind, or [`FieldKind::Unknown`].
    pub kind: FieldKind,
    /// How the name and kind were established.
    pub evidence: ClaimStatus,
}

/// Which documented schema a configuration entry follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordSchema {
    /// A one-letter `ASSETS/LAYOUT.CSV` record kind.
    Layout(RecordKind),
    /// A `ASSETS/SCRAPBOOK.CSV` `Mission_Spread_Item`.
    Scrapbook,
}

impl RecordSchema {
    /// Every schema, in the order the member's comments name the record
    /// kinds and the scrapbook's own list last. An account's schema roll-up
    /// walks this order, so the rows it reports are deterministic rather
    /// than in whatever order the entries happened to appear.
    pub const ALL: [Self; 12] = [
        Self::Layout(RecordKind::Button),
        Self::Layout(RecordKind::Pane),
        Self::Layout(RecordKind::Text),
        Self::Layout(RecordKind::EditBox),
        Self::Layout(RecordKind::Movie),
        Self::Layout(RecordKind::TextList),
        Self::Layout(RecordKind::ScrollingText),
        Self::Layout(RecordKind::Dropdown),
        Self::Layout(RecordKind::Listbox),
        Self::Layout(RecordKind::Slider),
        Self::Layout(RecordKind::SoundObject),
        Self::Scrapbook,
    ];

    /// The observed positions' specs, `0` being the record letter / first
    /// field. The slice covers every position the member's records use, so
    /// a field beyond it cannot occur.
    pub const fn fields(self) -> &'static [RecordFieldSpec] {
        match self {
            Self::Layout(kind) => layout_schema(kind),
            Self::Scrapbook => &SCRAPBOOK_SCHEMA,
        }
    }

    /// The documented field list the schema cites, from the member's own
    /// comments (`cs_formats::text::records`).
    pub fn documented(self) -> &'static [DocumentedField] {
        match self {
            Self::Layout(kind) => documented_fields(kind),
            Self::Scrapbook => documented_scrapbook_fields(),
        }
    }

    /// A short label for a report.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Layout(kind) => kind.documented_name(),
            Self::Scrapbook => "Mission_Spread_Item",
        }
    }

    /// Which schema `entry` follows, or `None` when no documented record
    /// covers it.
    ///
    /// A `LAYOUT.CSV` record is recognized by a one-letter first field the
    /// documented record kinds own, and a `V`/`G` variable-definition key
    /// is excluded first, because a definition's value could in principle
    /// be a one-letter name. Whether the original tells the two apart by
    /// the key or by the first field is **not** established — the two
    /// rules agree on every observed line — so this picks the rule that
    /// cannot misclassify a definition. A `SCRAPBOOK.CSV` item is
    /// recognized by the shape the member uses for all 461 of its entries:
    /// sixteen fields, the first a number. The recorded unknown is in
    /// `docs/findings/2026-09-29-f12-i-record-kind-schemas.md`.
    pub fn for_entry(entry: &ConfigEntry) -> Option<Self> {
        if is_definition_key(&entry.key) {
            return None;
        }
        let RawValue::Fields(fields) = &entry.value else {
            return None;
        };
        let first = fields.first()?.value();
        if let [letter] = first
            && let Some(kind) = RecordKind::from_letter(*letter)
        {
            return Some(Self::Layout(kind));
        }
        if fields.len() == SCRAPBOOK_SCHEMA.len() && is_whole_number(first) {
            return Some(Self::Scrapbook);
        }
        None
    }
}

/// Whether `key` is a `V<digits>` or `G<digits>` name-placeholder
/// definition, the second of the two rules the shipped data cannot tell
/// apart (task #351).
fn is_definition_key(key: &[u8]) -> bool {
    matches!(key.first(), Some(b'V' | b'G'))
        && key.len() > 1
        && key[1..].iter().all(u8::is_ascii_digit)
}

const INTEGER: FieldKind = FieldKind::Integer { signed: false };
const SIGNED_INTEGER: FieldKind = FieldKind::Integer { signed: true };
const BOOL: FieldKind = FieldKind::Bool;
const COLOR: FieldKind = FieldKind::Color;
const PATH: FieldKind = FieldKind::Path;
const NAME: FieldKind = FieldKind::Name;
const UNRESOLVED: FieldKind = FieldKind::Unknown;

const DOCUMENTED: ClaimStatus = ClaimStatus::Documented;
const INFERRED: ClaimStatus = ClaimStatus::Inferred;
const MEASURED: ClaimStatus = ClaimStatus::ObservedTool;
const UNRESOLVED_EVIDENCE: ClaimStatus = ClaimStatus::Unknown;

const fn field(
    position: usize,
    name: Option<&'static str>,
    kind: FieldKind,
    evidence: ClaimStatus,
) -> RecordFieldSpec {
    RecordFieldSpec {
        position,
        name,
        kind,
        evidence,
    }
}

/// `B`: the shipped records have 15, 16, 19 or 20 fields and move the
/// documented coordinates, so only the positions the data pins are named.
/// Position 19 occurs in the one twenty-field record and is spelled empty
/// there; positions 9-12 are empty in 113 of the 119 records, and the six
/// `PX_B_*` records spell an integer, a `<NAME>` placeholder and two
/// integers in them. With no name and no kind that holds across the member,
/// those four stay unknown and are counted. Position 5 holds `0` in the
/// records without colours and a `!` or an `IDS_…` name in those with them,
/// so neither its name nor its kind is established either.
static BUTTON_SCHEMA: [RecordFieldSpec; 20] = [
    field(0, Some("ID"), NAME, DOCUMENTED),
    field(1, Some("ArtPath"), PATH, INFERRED),
    field(2, Some("X"), INTEGER, INFERRED),
    field(3, Some("Y"), INTEGER, INFERRED),
    field(4, Some("Z"), INTEGER, INFERRED),
    field(5, None, UNRESOLVED, UNRESOLVED_EVIDENCE),
    field(6, None, NAME, MEASURED),
    field(7, None, INTEGER, MEASURED),
    field(8, None, INTEGER, MEASURED),
    field(9, None, UNRESOLVED, UNRESOLVED_EVIDENCE),
    field(10, None, UNRESOLVED, UNRESOLVED_EVIDENCE),
    field(11, None, UNRESOLVED, UNRESOLVED_EVIDENCE),
    field(12, None, UNRESOLVED, UNRESOLVED_EVIDENCE),
    field(13, None, INTEGER, MEASURED),
    field(14, Some("Checked?"), BOOL, INFERRED),
    field(15, Some("ColorDisabled"), COLOR, INFERRED),
    field(16, Some("ColorActive"), COLOR, INFERRED),
    field(17, Some("ColorRollover"), COLOR, INFERRED),
    field(18, Some("ColorDepressed"), COLOR, INFERRED),
    field(19, None, UNRESOLVED, UNRESOLVED_EVIDENCE),
];

/// `P`: nine fields against ten documented; `HelpID` is the one the
/// shipped data does not spell, and the eight remaining positions' kinds
/// then line up with the documented list in order.
static PANE_SCHEMA: [RecordFieldSpec; 9] = [
    field(0, Some("ID"), NAME, DOCUMENTED),
    field(1, Some("ArtPath"), PATH, INFERRED),
    field(2, Some("X"), SIGNED_INTEGER, INFERRED),
    field(3, Some("Y"), SIGNED_INTEGER, INFERRED),
    field(4, Some("Z"), INTEGER, INFERRED),
    field(5, Some("NUMFRAMES"), INTEGER, INFERRED),
    field(6, Some("IsRegion?"), BOOL, INFERRED),
    field(7, Some("AlphaType"), INTEGER, INFERRED),
    field(8, Some("Volatile?"), BOOL, INFERRED),
];

/// `T`: nine fields, exactly the documented list, kinds measured.
static TEXT_SCHEMA: [RecordFieldSpec; 9] = [
    field(0, Some("ID"), NAME, DOCUMENTED),
    field(1, Some("ResID"), NAME, DOCUMENTED),
    field(2, Some("X"), INTEGER, DOCUMENTED),
    field(3, Some("Y"), INTEGER, DOCUMENTED),
    field(4, Some("Z"), INTEGER, DOCUMENTED),
    field(5, Some("Width"), INTEGER, DOCUMENTED),
    field(6, Some("Height"), INTEGER, DOCUMENTED),
    field(7, Some("Color"), COLOR, DOCUMENTED),
    field(8, Some("Justify"), INTEGER, DOCUMENTED),
];

/// `E`: eleven fields against thirteen documented; `HelpID` and `TabOrder`
/// are the two the shipped data does not spell before the three colours.
static EDIT_BOX_SCHEMA: [RecordFieldSpec; 11] = [
    field(0, Some("ID"), NAME, DOCUMENTED),
    field(1, Some("FontID"), NAME, INFERRED),
    field(2, Some("X"), INTEGER, INFERRED),
    field(3, Some("Y"), INTEGER, INFERRED),
    field(4, Some("Z"), INTEGER, INFERRED),
    field(5, Some("Width"), INTEGER, INFERRED),
    field(6, Some("Height"), INTEGER, INFERRED),
    field(7, Some("MaxChars"), INTEGER, INFERRED),
    field(8, Some("TexTCOLOR"), COLOR, INFERRED),
    field(9, Some("FrameColor"), COLOR, INFERRED),
    field(10, Some("CursorColor"), COLOR, INFERRED),
];

/// `M`: nine fields like the documented list, but the first field after
/// `ID` is the movie's file name — a field the documented list does not
/// name — while `HelpID` is absent. Position 1 is a recorded unknown.
static MOVIE_SCHEMA: [RecordFieldSpec; 9] = [
    field(0, Some("ID"), NAME, DOCUMENTED),
    field(1, None, PATH, MEASURED),
    field(2, Some("X"), INTEGER, INFERRED),
    field(3, Some("Y"), INTEGER, INFERRED),
    field(4, Some("Z"), INTEGER, INFERRED),
    field(5, Some("ScaleX"), INTEGER, INFERRED),
    field(6, Some("ScaleY"), INTEGER, INFERRED),
    field(7, Some("# Loops"), INTEGER, INFERRED),
    field(8, Some("Region?"), BOOL, INFERRED),
];

/// `A`: nine fields against ten documented; `HelpID` is absent and the
/// remaining eight line up in order.
static TEXT_LIST_SCHEMA: [RecordFieldSpec; 9] = [
    field(0, Some("ID"), NAME, DOCUMENTED),
    field(1, Some("X"), INTEGER, INFERRED),
    field(2, Some("Y"), INTEGER, INFERRED),
    field(3, Some("Z"), INTEGER, INFERRED),
    field(4, Some("Width"), INTEGER, INFERRED),
    field(5, Some("Height"), INTEGER, INFERRED),
    field(6, Some("TexTCOLOR"), COLOR, INFERRED),
    field(7, Some("Justify"), INTEGER, INFERRED),
    field(8, Some("ItemSpacing"), INTEGER, INFERRED),
];

/// `S`: thirteen fields against fourteen documented. Position 11 is empty
/// in every record, so whether it is the documented `TabOrder` or `ResID`
/// (or neither) is not established; the trailing colour's position is.
static SCROLLING_TEXT_SCHEMA: [RecordFieldSpec; 13] = [
    field(0, Some("ID"), NAME, DOCUMENTED),
    field(1, Some("BorderColor"), COLOR, INFERRED),
    field(2, Some("BackColor"), COLOR, INFERRED),
    field(3, Some("Slider"), NAME, INFERRED),
    field(4, Some("UpArrow"), NAME, INFERRED),
    field(5, Some("DownArrow"), NAME, INFERRED),
    field(6, Some("X"), INTEGER, INFERRED),
    field(7, Some("Y"), INTEGER, INFERRED),
    field(8, Some("Z"), INTEGER, INFERRED),
    field(9, Some("Width"), INTEGER, INFERRED),
    field(10, Some("Height"), INTEGER, INFERRED),
    field(11, None, UNRESOLVED, UNRESOLVED_EVIDENCE),
    field(12, Some("Color"), COLOR, INFERRED),
];

/// `D`: twelve fields against fourteen documented; `ScriptPointer` (whose
/// documented position holds a coordinate in the data) and the trailing
/// `TabOrder` are the two the shipped data does not spell.
static DROPDOWN_SCHEMA: [RecordFieldSpec; 12] = [
    field(0, Some("ID"), NAME, DOCUMENTED),
    field(1, Some("Slider"), NAME, INFERRED),
    field(2, Some("UpArrow"), NAME, INFERRED),
    field(3, Some("DownArrow"), NAME, INFERRED),
    field(4, Some("DropUp"), NAME, INFERRED),
    field(5, Some("DropDown"), NAME, INFERRED),
    field(6, Some("X"), INTEGER, INFERRED),
    field(7, Some("Y"), INTEGER, INFERRED),
    field(8, Some("Z"), INTEGER, INFERRED),
    field(9, Some("Width"), INTEGER, INFERRED),
    field(10, Some("Height"), INTEGER, INFERRED),
    field(11, Some("TotalDisplayed"), INTEGER, INFERRED),
];

/// `L`: ten fields against twelve documented; as for the dropdown,
/// `ScriptPointer` and `TabOrder` are absent.
static LISTBOX_SCHEMA: [RecordFieldSpec; 10] = [
    field(0, Some("ID"), NAME, DOCUMENTED),
    field(1, Some("Slider"), NAME, INFERRED),
    field(2, Some("UpArrow"), NAME, INFERRED),
    field(3, Some("DownArrow"), NAME, INFERRED),
    field(4, Some("X"), INTEGER, INFERRED),
    field(5, Some("Y"), INTEGER, INFERRED),
    field(6, Some("Z"), INTEGER, INFERRED),
    field(7, Some("Width"), INTEGER, INFERRED),
    field(8, Some("Height"), INTEGER, INFERRED),
    field(9, Some("TotalDisplayed"), INTEGER, INFERRED),
];

/// `Z`: thirteen fields, exactly the documented list, kinds measured.
static SLIDER_SCHEMA: [RecordFieldSpec; 13] = [
    field(0, Some("ID"), NAME, DOCUMENTED),
    field(1, Some("X"), INTEGER, DOCUMENTED),
    field(2, Some("Y"), INTEGER, DOCUMENTED),
    field(3, Some("Z"), INTEGER, DOCUMENTED),
    field(4, Some("MinVal"), INTEGER, DOCUMENTED),
    field(5, Some("MaxVal"), INTEGER, DOCUMENTED),
    field(6, Some("CurrVal"), INTEGER, DOCUMENTED),
    field(7, Some("RegionFile"), PATH, DOCUMENTED),
    field(8, Some("SliderFile"), PATH, DOCUMENTED),
    field(9, Some("Left"), INTEGER, DOCUMENTED),
    field(10, Some("Top"), SIGNED_INTEGER, DOCUMENTED),
    field(11, Some("Right"), INTEGER, DOCUMENTED),
    field(12, Some("Bottom"), SIGNED_INTEGER, DOCUMENTED),
];

/// `W`: documented but absent from the shipped member, so every field but
/// the record letter is a recorded unknown. Its names come from the
/// documented list and are never claimed to hold a kind.
static SOUND_OBJECT_SCHEMA: [RecordFieldSpec; 6] = [
    field(0, Some("ID"), NAME, DOCUMENTED),
    field(1, Some("WAVFileName"), UNRESOLVED, UNRESOLVED_EVIDENCE),
    field(2, Some("Channel"), UNRESOLVED, UNRESOLVED_EVIDENCE),
    field(3, Some("Volume"), UNRESOLVED, UNRESOLVED_EVIDENCE),
    field(
        4,
        Some("LoopCount (0=continuous)"),
        UNRESOLVED,
        UNRESOLVED_EVIDENCE,
    ),
    field(5, Some("Autostart?"), UNRESOLVED, UNRESOLVED_EVIDENCE),
];

/// The `Mission_Spread_Item` schema: sixteen fields, exactly the documented
/// list, all 461 items consistent. Thirteen positions carry a kind the
/// shipped data measures. Three do not: `ImageType` holds three two-letter
/// codes (`P0`, `PJ`, `PP`), the quoted `Left,Top,Right,Bottom` field holds
/// four comma-separated numbers and `Zoom` holds single-letter codes and
/// `0`. No [`FieldKind`] describes any of those — `Name` would claim a
/// resource or UI name the bytes do not spell — so the three stay
/// [`FieldKind::Unknown`], keep their documented names and are counted by
/// [`RecordView::accounting`].
static SCRAPBOOK_SCHEMA: [RecordFieldSpec; 16] = [
    field(0, Some("Objective"), SIGNED_INTEGER, DOCUMENTED),
    field(1, Some("ResourceID"), NAME, DOCUMENTED),
    field(2, Some("ImageName"), PATH, DOCUMENTED),
    field(3, Some("ImageType"), UNRESOLVED, UNRESOLVED_EVIDENCE),
    field(4, Some("X"), INTEGER, DOCUMENTED),
    field(5, Some("Y"), INTEGER, DOCUMENTED),
    field(6, Some("Alpha"), INTEGER, DOCUMENTED),
    field(7, Some("Width"), INTEGER, DOCUMENTED),
    field(8, Some("Height"), INTEGER, DOCUMENTED),
    field(9, Some("DrawOrder"), INTEGER, DOCUMENTED),
    field(
        10,
        Some("Left,Top,Right,Bottom"),
        UNRESOLVED,
        UNRESOLVED_EVIDENCE,
    ),
    field(11, Some("Zoom"), UNRESOLVED, UNRESOLVED_EVIDENCE),
    field(12, Some("ZoomX"), INTEGER, DOCUMENTED),
    field(13, Some("ZoomY"), INTEGER, DOCUMENTED),
    field(14, Some("TitleResID"), NAME, DOCUMENTED),
    field(15, Some("TextResID"), NAME, DOCUMENTED),
];

/// The typed schema of one `LAYOUT.CSV` record kind.
pub const fn layout_schema(kind: RecordKind) -> &'static [RecordFieldSpec] {
    match kind {
        RecordKind::Button => &BUTTON_SCHEMA,
        RecordKind::Pane => &PANE_SCHEMA,
        RecordKind::Text => &TEXT_SCHEMA,
        RecordKind::EditBox => &EDIT_BOX_SCHEMA,
        RecordKind::Movie => &MOVIE_SCHEMA,
        RecordKind::TextList => &TEXT_LIST_SCHEMA,
        RecordKind::ScrollingText => &SCROLLING_TEXT_SCHEMA,
        RecordKind::Dropdown => &DROPDOWN_SCHEMA,
        RecordKind::Listbox => &LISTBOX_SCHEMA,
        RecordKind::Slider => &SLIDER_SCHEMA,
        RecordKind::SoundObject => &SOUND_OBJECT_SCHEMA,
    }
}

/// The typing of one `SCRAPBOOK.CSV` item.
pub const fn scrapbook_schema() -> &'static [RecordFieldSpec] {
    &SCRAPBOOK_SCHEMA
}

/// What the bytes of one observed field themselves show, before any schema
/// is applied. This is a spelling, not a kind: a `<NAME>` placeholder is
/// resolved separately by the F12-E pass and may stand for a value of any
/// kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldSpelling {
    /// The field has no bytes.
    Empty,
    /// A `<NAME>` reference the F12-E pass resolves.
    Placeholder,
    /// A decimal whole number, optionally signed.
    Integer,
    /// A `0x`/`0X`-prefixed whole number that is not an eight-digit colour.
    Hex,
    /// An eight-hex-digit `0x…` colour.
    Color,
    /// Anything else.
    Text,
}

/// One observed field of a record, matched to its schema position.
#[derive(Clone, Copy, Debug)]
pub struct RecordField<'a> {
    /// The observed position, `0` being the record letter / first field.
    pub position: usize,
    /// The schema entry for the position, `None` when no documented record
    /// covers it (which cannot happen for a schema the view was built for).
    pub spec: Option<&'static RecordFieldSpec>,
    /// The field as written.
    pub field: &'a RawField,
    /// What the bytes themselves show.
    pub spelling: FieldSpelling,
}

impl RecordField<'_> {
    /// The declared kind, or [`FieldKind::Unknown`] when nothing establishes
    /// one. A value of an unknown kind is retained and counted, not
    /// converted.
    pub fn kind(&self) -> FieldKind {
        self.spec.map_or(FieldKind::Unknown, |spec| spec.kind)
    }

    /// The documented field name, or `None` for a recorded unknown.
    pub fn name(&self) -> Option<&'static str> {
        self.spec.and_then(|spec| spec.name)
    }
}

/// One record entry read against its kind's schema, retaining every field.
#[derive(Clone, Debug)]
pub struct RecordView<'a> {
    schema: RecordSchema,
    fields: Vec<RecordField<'a>>,
    unsplit: bool,
}

impl<'a> RecordView<'a> {
    /// Reads `entry` against `schema`, classifying each field's spelling.
    /// Nothing is converted and no byte changes: the view borrows the
    /// document's own fields.
    pub fn new(schema: RecordSchema, entry: &'a ConfigEntry) -> Self {
        let specs = schema.fields();
        let (fields, unsplit) = match &entry.value {
            RawValue::Fields(owned) => (
                owned
                    .iter()
                    .enumerate()
                    .map(|(position, field)| RecordField {
                        position,
                        spec: specs.get(position),
                        field,
                        spelling: spell(field.value()),
                    })
                    .collect(),
                false,
            ),
            RawValue::Unsplit { .. } => (Vec::new(), true),
        };
        Self {
            schema,
            fields,
            unsplit,
        }
    }

    /// Reads `entry` against whichever schema it follows, or `None` when no
    /// documented record covers it.
    pub fn for_entry(entry: &'a ConfigEntry) -> Option<Self> {
        RecordSchema::for_entry(entry).map(|schema| Self::new(schema, entry))
    }

    /// The schema the entry was read against.
    pub fn schema(&self) -> RecordSchema {
        self.schema
    }

    /// Every field, in member order.
    pub fn fields(&self) -> &[RecordField<'a>] {
        &self.fields
    }

    /// Whether the entry's value could not be split into fields at all, so
    /// no field was classified and the whole value stays raw.
    pub fn unsplit(&self) -> bool {
        self.unsplit
    }

    /// What the record yielded: how many fields were typed, how many stayed
    /// unknown, and how many were placeholders or empty.
    ///
    /// `unknown` is the count the spec's non-negotiable #5 cares about: a
    /// field whose kind cannot be established is retained in the document
    /// and counted here rather than dropped. A record whose value did not
    /// split reports `unsplit` and counts the whole value as one unknown.
    pub fn accounting(&self) -> RecordAccounting {
        let mut accounting = RecordAccounting {
            schema: self.schema.label(),
            unsplit: self.unsplit,
            ..RecordAccounting::default()
        };
        if self.unsplit {
            accounting.unknown = 1;
            return accounting;
        }
        for field in &self.fields {
            accounting.fields += 1;
            match field.spelling {
                FieldSpelling::Empty => accounting.empty += 1,
                FieldSpelling::Placeholder => accounting.placeholders += 1,
                _ => {}
            }
            if field.kind().is_known() {
                accounting.typed += 1;
            } else {
                accounting.unknown += 1;
            }
        }
        accounting
    }
}

/// What one record's fields yielded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecordAccounting {
    /// The schema's report label.
    pub schema: &'static str,
    /// Fields the value split into.
    pub fields: usize,
    /// Fields whose kind is established.
    pub typed: usize,
    /// Fields whose kind is not established (including a value that did not
    /// split at all, which is one unknown).
    pub unknown: usize,
    /// Fields spelled as a `<NAME>` placeholder (a spelling compatible with
    /// any kind).
    pub placeholders: usize,
    /// Empty fields.
    pub empty: usize,
    /// Whether the value could not be split into fields at all.
    pub unsplit: bool,
}

/// Whether `text` is a decimal whole number, optionally signed.
fn is_whole_number(text: &[u8]) -> bool {
    let digits = match text.first() {
        Some(b'-' | b'+') => &text[1..],
        _ => text,
    };
    !digits.is_empty() && digits.iter().all(u8::is_ascii_digit)
}

/// What the bytes of one field show, without applying a schema.
fn spell(text: &[u8]) -> FieldSpelling {
    if text.is_empty() {
        return FieldSpelling::Empty;
    }
    if text.first() == Some(&b'<') && text.last() == Some(&b'>') && text.len() > 2 {
        return FieldSpelling::Placeholder;
    }
    if let Some(hex) = text
        .strip_prefix(b"0x")
        .or_else(|| text.strip_prefix(b"0X"))
        && !hex.is_empty()
        && hex.iter().all(u8::is_ascii_hexdigit)
    {
        return if hex.len() == 8 {
            FieldSpelling::Color
        } else {
            FieldSpelling::Hex
        };
    }
    if is_whole_number(text) {
        return FieldSpelling::Integer;
    }
    FieldSpelling::Text
}

/// The colour the original's `T` (text) record reader holds for a `Color`
/// field, as measured from code (task #380, owner decrypted image; static
/// code evidence, never `verified_original`).
///
/// The reader zeroes the record's colour, then, if the token is non-empty,
/// runs `sscanf(token, "%x", &colour)` without checking the result. A token
/// that fails the conversion (`oxff1E283C`, `xff000000`) leaves the zero, so
/// it reads as `0`. `%x` accepts leading whitespace, an optional sign, an
/// optional `0x`/`0X` prefix, then hex digits.
///
/// `None` is the part nothing measured: more than eight hex digits, or a
/// prefix with no digit after it. The raw token is never altered by this.
pub fn legacy_text_colour(token: &[u8]) -> Option<u32> {
    if token.is_empty() {
        return Some(0);
    }
    let rest = token.trim_ascii_start();
    let (negative, rest) = match rest.first() {
        Some(b'-') => (true, &rest[1..]),
        Some(b'+') => (false, &rest[1..]),
        _ => (false, rest),
    };
    let prefixed = rest
        .strip_prefix(b"0x")
        .or_else(|| rest.strip_prefix(b"0X"));
    let digits = prefixed.unwrap_or(rest);
    let run = digits.iter().take_while(|b| b.is_ascii_hexdigit()).count();
    if run == 0 {
        // Nothing converts: the destination is not written. A bare `0x` is
        // not measured, because the CRT may consume its `0` as a digit.
        return if prefixed.is_some() { None } else { Some(0) };
    }
    if run > 8 {
        return None;
    }
    let text = std::str::from_utf8(&digits[..run]).ok()?;
    let value = u32::from_str_radix(text, 16).ok()?;
    Some(if negative {
        value.wrapping_neg()
    } else {
        value
    })
}

/// The RGB the original draws text in for a parsed `T` colour: the alpha
/// byte is dropped, and RGB `0` is drawn as `(15, 15, 15)` (task #380).
pub fn legacy_text_rgb(colour: u32) -> [u8; 3] {
    let [_, red, green, blue] = colour.to_be_bytes();
    if colour & 0x00ff_ffff == 0 {
        [15, 15, 15]
    } else {
        [red, green, blue]
    }
}

/// Whether the bytes of one observed field can carry the kind its schema
/// declares, which is the production form of the check task #371 measured
/// only in a test.
///
/// A `<NAME>` placeholder and an empty field fit every kind — the F12-E pass
/// resolves the one and the shipped data leaves the other empty — and a
/// `0x…` literal is an integer notation wherever an integer is declared.
/// Everything else must look like the kind it is declared to be, so a
/// declared kind the bytes never spell is counted as
/// [`PositionAccount::off_kind`] rather than silently accepted.
///
/// An unknown kind accepts everything: it claims nothing the bytes could
/// contradict, which is the whole point of a recorded unknown.
pub fn spells_kind(kind: FieldKind, spelling: FieldSpelling) -> bool {
    match kind {
        FieldKind::Unknown => true,
        FieldKind::Integer { .. } => {
            matches!(
                spelling,
                FieldSpelling::Integer
                    | FieldSpelling::Hex
                    | FieldSpelling::Placeholder
                    | FieldSpelling::Empty
            )
        }
        FieldKind::Bool => matches!(
            spelling,
            FieldSpelling::Integer | FieldSpelling::Placeholder | FieldSpelling::Empty
        ),
        FieldKind::Color => matches!(
            spelling,
            FieldSpelling::Color | FieldSpelling::Placeholder | FieldSpelling::Empty
        ),
        FieldKind::Name | FieldKind::Path => {
            matches!(
                spelling,
                FieldSpelling::Text
                    | FieldSpelling::Integer
                    | FieldSpelling::Placeholder
                    | FieldSpelling::Empty
            )
        }
    }
}

// ---------------------------------------------------------------------------
// The whole-installation account (stage F12-D)
// ---------------------------------------------------------------------------
//
// Task #482 finishes what stage F12-D owes. Everything above it reads **one
// member at a time**: [`ConfigDocument::accounting`] counts that member's
// entries, [`RecordView::accounting`] counts that record's fields, and the
// `config` command reports one of them per run. What did not exist is an
// account over the *installation*:
//
// * [`ConfigDocument::member_account`] rolls a whole member up — which field
//   positions of which declared schema the member's records actually reach,
//   which of those positions stay a recorded unknown, and which entries no
//   declaration consumed at all — with the member's own [`SourceSpan`], and
//   classifies each unconsumed entry as a parity blocker or not (spec F12,
//   non-negotiable #5).
// * [`account_string_ids`] crosses the ids the original **names** (the
//   `IDS_`/`STR_`/`SB_` defines of the two resource headers) with the ids the
//   string images actually **carry**, and reports the difference in both
//   directions as a first-class account instead of a one-off measurement.
//
// Neither half guesses. A position whose kind is not established stays
// [`FieldKind::Unknown`] and is counted; a header value that is not a plain
// decimal is counted as undecoded and dropped from the id sets; an entry no
// declaration covers is listed with its line and bytes, never silently
// discarded. The numbers this stage measures over the installed data are
// written down in
// `docs/findings/2026-10-02-f12-d-installation-wide-configuration-account.md`.

/// What an entry no declaration consumed means for parity.
///
/// Spec F12 non-negotiable #5: "unknown configuration keys are retained and
/// counted; gameplay-critical unconsumed keys block parity rather than being
/// discarded". Which of the two a member is comes from its **consumer**, not
/// from its file name, so it is a declared argument ([`MemberAccount::parity`])
/// and never inferred here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parity {
    /// The member feeds gameplay, so an entry no declaration consumed is a
    /// parity blocker: the account reports it and a caller must fail.
    GameplayCritical,
    /// The member does not feed gameplay. The entry is still retained and
    /// counted, but it does not block parity.
    NonGameplay,
}

impl Parity {
    /// Stable, machine-matchable label.
    pub const fn code(self) -> &'static str {
        match self {
            Self::GameplayCritical => "gameplay_critical",
            Self::NonGameplay => "non_gameplay",
        }
    }

    /// Whether an unconsumed entry of such a member blocks parity.
    pub const fn blocks(self) -> bool {
        matches!(self, Self::GameplayCritical)
    }
}

/// What a declaration covers of one entry of a routed keyed-list member.
///
/// Exactly one of these answers an entry, and the answer is a declaration —
/// a schema, the placeholder pass — or the recorded fact that nothing
/// declares it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryDeclaration {
    /// A declared record schema reads the entry's fields
    /// ([`RecordSchema::for_entry`]).
    Record(RecordSchema),
    /// The `<NAME>` pass owns the entry as a `V`/`G` definition. This is the
    /// F12-E pass's own rule (`cs_formats::text::definition_scope` plus the
    /// two-field value shape it measured), not a second one.
    Placeholder(PlaceholderScope),
    /// Nothing declared reads the entry. It is retained and listed
    /// ([`MemberAccount::unconsumed`]); a gameplay-critical one is a parity
    /// blocker.
    Undeclared,
}

impl EntryDeclaration {
    /// Stable, machine-matchable label.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Record(_) => "record",
            Self::Placeholder(_) => "placeholder_definition",
            Self::Undeclared => "undeclared",
        }
    }
}

/// One entry of a member, with the declaration that covers it and what its
/// unconsumed state means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountedEntry<'a> {
    /// The 1-based line the entry came from.
    pub line: u64,
    /// The section header the entry follows, `None` before the first one.
    pub section: Option<&'a [u8]>,
    /// The key, as written.
    pub key: &'a [u8],
    /// What covers it.
    pub declaration: EntryDeclaration,
    /// The member's declared parity, so a caller can filter without
    /// re-reading the member's declaration.
    pub parity: Parity,
}

/// One line of a member no observed rule explains, kept with its reason so a
/// caller can see that the member is not fully accounted for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccountedLine {
    /// The 1-based line number.
    pub line: u64,
    /// Why the line is not accounted for.
    pub reason: Unclassified,
}

/// One declared field position of one record schema, against the member's
/// records.
///
/// The four value columns partition [`Self::observed`]: every value at the
/// position is empty, a `<NAME>` reference, or a value, and a value is
/// either described by the position's declared kind
/// ([`Self::typed`]), not described because the kind is **not established**
/// ([`Self::untyped`], the recorded unknown spec F12 non-negotiable #5 asks
/// to be counted), or not describable because the bytes do not look like the
/// kind that *is* declared ([`Self::off_kind`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PositionAccount {
    /// The observed position, `0` being the record letter / first field.
    pub position: usize,
    /// The documented field name, or `None` for a recorded unknown.
    pub name: Option<&'static str>,
    /// The declared kind, or [`FieldKind::Unknown`].
    pub kind: FieldKind,
    /// How the name and the kind were established.
    pub evidence: ClaimStatus,
    /// Records of this kind that reached the position.
    pub observed: usize,
    /// Values a **declared** kind describes: the position's kind is
    /// established and the value's bytes spell it.
    pub typed: usize,
    /// Values whose kind is **not** established — the recorded unknown. The
    /// value stays raw and is never converted.
    pub untyped: usize,
    /// Records whose value is a `<NAME>` reference. The spelling is
    /// compatible with any kind, so it is counted here and is **not** a
    /// mismatch.
    pub placeholders: usize,
    /// Records whose value is empty.
    pub empty: usize,
    /// Records whose value's bytes do not look like the declared kind
    /// ([`spells_kind`]). A non-zero count means the schema's declaration and
    /// the shipped data disagree, which is a defect of the declaration, not
    /// of the data.
    pub off_kind: usize,
}

impl PositionAccount {
    /// Values at this position a declared kind actually describes.
    pub const fn described(&self) -> usize {
        self.typed
    }

    /// Values at this position whose kind is not established.
    pub const fn untyped(&self) -> usize {
        self.untyped
    }
}

/// One record schema's roll-up over a member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaAccount {
    /// The schema these records were read against.
    pub schema: RecordSchema,
    /// The schema's report label.
    pub label: &'static str,
    /// Records of this kind in the member.
    pub records: usize,
    /// Fields the member's records of this kind split into.
    pub fields: usize,
    /// Records whose value did not split at all, so the whole value stayed
    /// raw and no field was classified.
    pub unsplit: usize,
    /// Fields at a position **beyond** the declared schema's slice. A schema
    /// slice covers every position the surveyed members use, so a non-zero
    /// count means a new record shape appeared.
    pub beyond_schema: usize,
    /// One row per declared position, in position order.
    pub positions: Vec<PositionAccount>,
}

impl SchemaAccount {
    /// The declared positions the member's records of this kind reached, in
    /// position order.
    pub fn positions(&self) -> &[PositionAccount] {
        &self.positions
    }

    /// Records whose whole value stayed raw.
    pub fn unsplit(&self) -> usize {
        self.unsplit
    }

    /// Every field at this schema's positions whose kind is not established.
    pub fn untyped(&self) -> usize {
        self.positions.iter().map(PositionAccount::untyped).sum()
    }

    /// Every field at this schema's positions whose bytes do not look like
    /// the declared kind.
    pub fn off_kind(&self) -> usize {
        self.positions
            .iter()
            .map(|position| position.off_kind)
            .sum()
    }

    /// Fields at a position with **no** documented name, whether or not the
    /// kind is established: the identities the shipped data and the
    /// documented list between them leave open.
    pub fn unnamed(&self) -> usize {
        self.positions
            .iter()
            .filter(|position| position.name.is_none())
            .map(|position| position.observed)
            .sum()
    }
}

/// One routed keyed-list member's whole account: every entry, which
/// declaration covers it, the position census of the declared schemas and
/// the lines nothing explains — all against the member's own
/// [`SourceSpan`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberAccount<'a> {
    source: SourceSpan,
    dialect: TextDialect,
    parity: Parity,
    keys: KeyAccounting,
    entries: Vec<AccountedEntry<'a>>,
    schemas: Vec<SchemaAccount>,
    placeholders: PlaceholderAccounting,
    unclassified: Vec<AccountedLine>,
}

impl<'a> MemberAccount<'a> {
    /// The member's provenance: installation, container, member, offset,
    /// length and digest as the caller supplied it.
    pub fn source(&self) -> &SourceSpan {
        &self.source
    }

    /// The dialect the inventory routed the member to.
    pub fn dialect(&self) -> TextDialect {
        self.dialect
    }

    /// The declared parity of the member, which decides whether an
    /// unconsumed entry blocks.
    pub fn parity(&self) -> Parity {
        self.parity
    }

    /// The document's own entry/consumption counts.
    pub fn keys(&self) -> KeyAccounting {
        self.keys
    }

    /// Every entry, in member order, with the declaration that covers it.
    pub fn entries(&self) -> &[AccountedEntry<'a>] {
        &self.entries
    }

    /// The declarations the member's records were read against, in
    /// [`RecordSchema::ALL`] order and only for the schemas at least one
    /// record follows, so the roll-up is deterministic and holds no
    /// empty row.
    pub fn schemas(&self) -> &[SchemaAccount] {
        &self.schemas
    }

    /// The roll-up of one schema, or `None` when no record follows it.
    pub fn schema(&self, schema: RecordSchema) -> Option<&SchemaAccount> {
        self.schemas.iter().find(|row| row.schema == schema)
    }

    /// What the `<NAME>` pass read in the member.
    pub fn placeholders(&self) -> PlaceholderAccounting {
        self.placeholders
    }

    /// The lines no observed rule explains, in member order.
    pub fn unclassified(&self) -> &[AccountedLine] {
        &self.unclassified
    }

    /// The entries no declaration consumed, in member order. They are
    /// retained in the document, never dropped; the list is the parity
    /// blocker set.
    pub fn unconsumed(&self) -> impl Iterator<Item = &AccountedEntry<'a>> {
        self.entries
            .iter()
            .filter(|entry| entry.declaration == EntryDeclaration::Undeclared)
    }

    /// How many unconsumed entries of this member block parity.
    pub fn blocking(&self) -> usize {
        if self.parity.blocks() {
            self.unconsumed().count()
        } else {
            0
        }
    }

    /// Whether the member is accounted for: no gameplay-critical entry
    /// without a declaration. `true` does **not** mean the member is
    /// understood — the recorded unknowns it names are still open.
    pub fn parity_holds(&self) -> bool {
        self.blocking() == 0
    }
}

impl ConfigDocument {
    /// Accounts the whole member against the declared record schemas and the
    /// `<NAME>` pass, with `parity` as the member's declared gameplay
    /// criticality.
    ///
    /// Every entry gets exactly one declaration
    /// ([`EntryDeclaration`]): a declared record schema, the placeholder
    /// pass, or [`EntryDeclaration::Undeclared`]. Nothing is dropped and
    /// nothing is converted — a position whose kind is not established stays
    /// unknown and is counted, and a value the schema's kind does not accept
    /// is counted in [`PositionAccount::off_kind`] rather than coerced.
    pub fn member_account(&self, parity: Parity) -> MemberAccount<'_> {
        let definitions: BTreeSet<u64> = self
            .placeholders
            .definitions()
            .iter()
            .map(|definition| definition.line)
            .collect();

        let mut records: Vec<(RecordSchema, Vec<RecordView<'_>>)> = Vec::new();
        let mut entries: Vec<AccountedEntry<'_>> = Vec::with_capacity(self.keys_len());
        for entry in self.entries() {
            let placeholder = definition_scope(&entry.key)
                .filter(|_| is_placeholder_definition(&definitions, entry.line));
            let declaration = match (placeholder, RecordSchema::for_entry(entry)) {
                (Some(scope), _) => EntryDeclaration::Placeholder(scope),
                (None, Some(schema)) => EntryDeclaration::Record(schema),
                (None, None) => EntryDeclaration::Undeclared,
            };
            if let EntryDeclaration::Record(schema) = declaration {
                let view = RecordView::new(schema, entry);
                match records.iter_mut().find(|(known, _)| *known == schema) {
                    Some((_, views)) => views.push(view),
                    None => records.push((schema, vec![view])),
                }
            }
            entries.push(AccountedEntry {
                line: entry.line,
                section: entry.section.as_deref(),
                key: &entry.key,
                declaration,
                parity,
            });
        }

        // `RecordSchema::ALL` order, not the order the entries arrived in, so
        // two runs over the same member report the same rows in the same
        // sequence.
        let schemas = RecordSchema::ALL
            .iter()
            .filter_map(|schema| {
                records
                    .iter()
                    .find(|(known, _)| known == schema)
                    .map(|(known, views)| roll_up(*known, views))
            })
            .collect();
        let unclassified = self
            .nodes
            .iter()
            .filter_map(|node| match node.kind {
                ConfigNodeKind::Unclassified { reason } => Some(AccountedLine {
                    line: node.line,
                    reason,
                }),
                _ => None,
            })
            .collect();

        MemberAccount {
            source: self.source.clone(),
            dialect: self.dialect,
            parity,
            keys: self.accounting(),
            entries,
            schemas,
            placeholders: self.placeholders.accounting(),
            unclassified,
        }
    }

    /// How many entries the document holds, used to size the account.
    fn keys_len(&self) -> usize {
        self.accounting().entries
    }
}

/// Whether the entry at `line` is one the `<NAME>` pass actually read as a
/// definition.
///
/// The pass's own rule is a `V`/`G` key **and** a value of exactly two
/// fields; this repeats neither of those predicates, it asks the pass's own
/// table, so the account and the placeholder pass cannot disagree about what
/// a definition is.
fn is_placeholder_definition(definitions: &BTreeSet<u64>, line: u64) -> bool {
    definitions.contains(&line)
}

/// The position census of one schema over the member's records of it.
fn roll_up(schema: RecordSchema, views: &[RecordView<'_>]) -> SchemaAccount {
    let specs = schema.fields();
    let positions: Vec<PositionAccount> = specs
        .iter()
        .map(|spec| PositionAccount {
            position: spec.position,
            name: spec.name,
            kind: spec.kind,
            evidence: spec.evidence,
            observed: 0,
            typed: 0,
            untyped: 0,
            placeholders: 0,
            empty: 0,
            off_kind: 0,
        })
        .collect();
    let mut account = SchemaAccount {
        schema,
        label: schema.label(),
        records: views.len(),
        fields: 0,
        unsplit: 0,
        beyond_schema: 0,
        positions,
    };
    for view in views {
        if view.unsplit() {
            account.unsplit += 1;
            continue;
        }
        for field in view.fields() {
            account.fields += 1;
            let Some(position) = account
                .positions
                .iter_mut()
                .find(|position| position.position == field.position)
            else {
                // A position the declared slice does not cover: a record shape
                // the schema has not been told about. Counted, never guessed.
                account.beyond_schema += 1;
                continue;
            };
            position.observed += 1;
            match field.spelling {
                FieldSpelling::Empty => position.empty += 1,
                FieldSpelling::Placeholder => position.placeholders += 1,
                _ => {
                    if !field.kind().is_known() {
                        // A kind nothing establishes is a recorded unknown, not
                        // a value of any kind: it is counted here and never
                        // converted.
                        position.untyped += 1;
                    } else if spells_kind(field.kind(), field.spelling) {
                        position.typed += 1;
                    } else {
                        // The kind is declared and the bytes do not spell it,
                        // so the declaration is what is wrong here.
                        position.off_kind += 1;
                    }
                }
            }
        }
    }
    account
}

// ---------------------------------------------------------------------------
// The referenced string ids (stage F12-D)
// ---------------------------------------------------------------------------

/// The define-name prefixes the survey found carrying a string-table id.
///
/// `ObservedTool` from one English installation: task #368 counted 351
/// `RESOURCE.H` and 181 `RESRC1.H` defines under these three prefixes and
/// found the rest of the defines to be font ids, multiplayer control ids and
/// resource-compiler bookkeeping. A define whose name starts with one of
/// them is a **string** id for the account's `string_*` rows; a define whose
/// name starts with none is still counted, and still crosses the images, in
/// the member's own rows. The prefixes are not a claim that no other prefix
/// can ever name a string, and [`account_string_ids`] does not treat them as
/// one: both scopes are reported.
pub const STRING_NAME_PREFIXES: [&[u8]; 3] = [b"IDS_", b"STR_", b"SB_"];

/// Whether `name` is one of the observed string-table define names.
pub fn is_string_name(name: &[u8]) -> bool {
    STRING_NAME_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

/// One resource-header member's ids, crossed against one string image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderIdAccount {
    /// The member's spelling, so a report can name the source.
    pub member: String,
    /// `#define` lines in the member, duplicates included.
    pub defines: usize,
    /// Defines whose value is not a plain decimal in the 16-bit id space.
    /// They keep their raw bytes in the reader and are **not** in either id
    /// set here: an id nobody can read must not silently become a
    /// coincidence.
    pub undecoded: usize,
    /// Distinct ids the member's defines name, ascending.
    pub distinct: Vec<u32>,
    /// Of [`Self::distinct`], the ones whose block this image has.
    pub present: usize,
    /// Of [`Self::distinct`], the ones whose block this image has **not**,
    /// ascending.
    pub absent: Vec<u32>,
    /// Defines whose name carries an observed string-table prefix.
    pub string_defines: usize,
    /// Distinct ids those string-name defines name, ascending.
    pub string_distinct: Vec<u32>,
    /// Of [`Self::string_distinct`], the ones whose block this image has.
    pub string_present: usize,
    /// Of [`Self::string_distinct`], the ones whose block this image has
    /// **not**, ascending.
    pub string_absent: Vec<u32>,
}

/// One string image's `RT_STRING` blocks against every header's ids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringIdAccount {
    /// The image's provenance.
    pub source: SourceSpan,
    /// `RT_STRING` blocks the image has.
    pub blocks: usize,
    /// String units the image's blocks count, empty ones included — the same
    /// population [`StringCatalog::accounting`] reports as `strings`.
    pub units: usize,
    /// Distinct ids **any** header names, ascending.
    pub named_ids: usize,
    /// Blocks no header value names, ascending. A block can be reached by a
    /// name the headers do not carry, by a resource the `.rc` referenced but
    /// did not define, or by a second `.rc` that is not in the installation;
    /// this account reports the set and does not choose between the three.
    pub unnamed_blocks: Vec<u32>,
    /// One row per header member, in the order they were supplied.
    pub headers: Vec<HeaderIdAccount>,
}

impl StringIdAccount {
    /// Blocks no header names.
    pub fn unnamed_blocks(&self) -> &[u32] {
        &self.unnamed_blocks
    }

    /// The one header's row, or `None` when that member was not supplied.
    pub fn header(&self, member: &str) -> Option<&HeaderIdAccount> {
        self.headers
            .iter()
            .find(|row| row.member.eq_ignore_ascii_case(member))
    }

    /// Every distinct id the headers name that this image has no block for,
    /// across all of them, ascending and de-duplicated.
    pub fn absent_ids(&self) -> Vec<u32> {
        let mut every: Vec<u32> = self
            .headers
            .iter()
            .flat_map(|row| row.absent.iter().copied())
            .collect();
        every.sort_unstable();
        every.dedup();
        every
    }
}

/// Crosses the ids `headers` name with the ids `catalog` actually carries.
///
/// A define's value is a **block** id under the numbering this project
/// reports ([`cs_formats::string_id`], `id = (block - 1) * 16 + index`, pinned
/// by task #374), so the block a named id lives in is `(id / 16) + 1`; an id
/// whose block the image does not have is [`HeaderIdAccount::absent`], and a
/// block no header value names is [`StringIdAccount::unnamed_blocks`]. Both
/// directions are reported, because a difference in one direction alone cannot
/// say which side is wrong.
///
/// Task #368 measured this once by hand with an independent walk of the
/// resource directories; this is the production derivation of the same
/// numbers, and the finding records the agreement.
pub fn account_string_ids<'header, 'bytes>(
    catalog: &StringCatalog,
    headers: &[(&str, &'header ResourceHeader<'bytes>)],
) -> StringIdAccount {
    let blocks: BTreeSet<u32> = catalog
        .resources()
        .strings()
        .iter()
        .map(|block| u32::from(block.block_id))
        .collect();
    let units: usize = catalog
        .resources()
        .strings()
        .iter()
        .map(|block| block.units.len())
        .sum();

    let mut named: BTreeSet<u32> = BTreeSet::new();
    let mut rows = Vec::with_capacity(headers.len());
    for (member, header) in headers {
        let mut distinct = BTreeSet::new();
        let mut string_distinct = BTreeSet::new();
        let mut undecoded = 0usize;
        let mut string_defines = 0usize;
        for define in header.defines() {
            if is_string_name(define.name) {
                string_defines += 1;
            }
            match define.resource_id() {
                Some(id) => {
                    distinct.insert(id);
                    named.insert(id);
                    if is_string_name(define.name) {
                        string_distinct.insert(id);
                    }
                }
                None => undecoded += 1,
            }
        }
        let split = |ids: &BTreeSet<u32>| -> (usize, Vec<u32>) {
            let mut present = 0usize;
            let mut absent = Vec::new();
            for id in ids {
                if blocks.contains(&block_of(*id)) {
                    present += 1;
                } else {
                    absent.push(*id);
                }
            }
            (present, absent)
        };
        let (present, absent) = split(&distinct);
        let (string_present, string_absent) = split(&string_distinct);
        rows.push(HeaderIdAccount {
            member: (*member).to_owned(),
            defines: header.defines().count(),
            undecoded,
            present,
            absent,
            distinct: distinct.into_iter().collect(),
            string_defines,
            string_present,
            string_absent,
            string_distinct: string_distinct.into_iter().collect(),
        });
    }

    let unnamed_blocks = blocks
        .iter()
        .copied()
        .filter(|block| !named.iter().any(|id| block_of(*id) == *block))
        .collect();
    StringIdAccount {
        source: catalog.source().clone(),
        blocks: blocks.len(),
        units,
        named_ids: named.len(),
        unnamed_blocks,
        headers: rows,
    }
}

/// The `RT_STRING` block the string id `id` lives in under the numbering
/// [`cs_formats::string_id`] reports, which is `(id / 16) + 1` — the inverse
/// of `id = (block - 1) * 16 + index`.
///
/// That formula is pinned, not assumed: task #374 measured the `RT_STRING`
/// directory entries of `langui.dll` and recorded the numbering as the Win32
/// string-table numbering (`docs/findings/2026-10-02-t374-string-id-numbering.md`).
const fn block_of(id: u32) -> u32 {
    id / cs_formats::STRING_UNITS_PER_BLOCK as u32 + 1
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use cs_formats::text::dialect::CRIMSON_ROF;
    use cs_types::evidence::ContentHash;

    use super::*;
    use cs_formats::text::read_resource_header;

    const LAYOUT: &str = "ASSETS/LAYOUT.CSV";

    /// Authored; shaped like the observed dialect, not copied from it.
    const MEMBER: &[u8] = b";header comment, with \"quotes\"\r\n\
KNOWN=1\r\n\
[PANEL]\r\n\
\tSLOT = P,\"1,2\",\xe9\r\n\
SLOT=Q\r\n\
DUP=1\r\n\
DUP=2\r\n\
BROKEN=\"open\r\n\
?stray\r\n\
N\xe4me=x";

    fn source(member: &str, length: usize) -> SourceSpan {
        SourceSpan::new(
            ContentHash::from_bytes([7; 32]),
            cs_formats::text::dialect::CRIMSON_ROF,
            Some(member),
            0,
            length as u64,
            None,
        )
        .expect("valid span")
    }

    fn document() -> ConfigDocument {
        let mut context = ParseContext::with_defaults("fixture");
        ConfigDocument::read(&mut context, source(LAYOUT, MEMBER.len()), MEMBER)
            .expect("an observed keyed list member reads")
    }

    fn texts(entry: &ConfigEntry) -> Vec<&[u8]> {
        match &entry.value {
            RawValue::Fields(fields) => fields.iter().map(RawField::text).collect(),
            RawValue::Unsplit { .. } => panic!("unsplit"),
        }
    }

    /// AC01 through the content layer: quoted separators, comments, CRLF
    /// and non-ASCII names survive into owned nodes that rebuild the
    /// member byte for byte.
    #[test]
    fn accept_f12_a_config_document_is_lossless() {
        let mut document = document();
        assert_eq!(document.reassemble(), MEMBER);
        assert_eq!(document.dialect(), TextDialect::KeyedList);
        assert_eq!(document.nodes()[0].kind, ConfigNodeKind::Comment);
        assert_eq!(
            document.nodes()[0].content,
            b";header comment, with \"quotes\""
        );
        assert_eq!(document.nodes()[0].terminator, LineTerminator::CrLf);
        assert_eq!(document.nodes()[3].offset, 50);
        assert_eq!(document.nodes()[3].line, 4);
        assert_eq!(
            document.nodes().last().unwrap().terminator,
            LineTerminator::None
        );

        let ConfigNodeKind::Entry(slot) = &document.nodes()[3].kind else {
            panic!("line 4 is an entry")
        };
        assert_eq!(slot.section.as_deref(), Some(&b"PANEL"[..]));
        assert_eq!(slot.key, b"SLOT");
        assert_eq!(texts(slot), vec![&b" P"[..], b"1,2", b"\xe9"]);
        assert_eq!(document.nodes()[3].content, b"\tSLOT = P,\"1,2\",\xe9");
        let Lookup::Found(name) = document.lookup(Some(b"PANEL"), b"N\xe4me") else {
            panic!("non-ASCII key")
        };
        assert_eq!(texts(name), vec![&b"x"[..]]);
        assert_eq!(document.source().member_key(), Some(LAYOUT));
    }

    /// Every buffer the document owns is booked against the budget, on
    /// top of what the keyed list reader booked for the borrowed parse:
    /// the exact total reads and one byte less is refused.
    #[test]
    fn accept_f12_a_config_document_books_every_copy_it_owns() {
        let mut context = ParseContext::with_defaults("fixture");
        let document = ConfigDocument::read(&mut context, source(LAYOUT, MEMBER.len()), MEMBER)
            .expect("an observed keyed list member reads");
        assert_eq!(document.reassemble(), MEMBER);

        let mut expected = 0u64;
        let mut fields = 0usize;
        for node in document.nodes() {
            expected += std::mem::size_of::<ConfigNode>() as u64;
            expected += node.content.len() as u64;
            if let ConfigNodeKind::Section { name } = &node.kind {
                expected += 2 * name.len() as u64;
            }
            let ConfigNodeKind::Entry(entry) = &node.kind else {
                continue;
            };
            expected += entry.key.len() as u64;
            expected += entry.section.as_deref().map_or(0, |name| name.len() as u64);
            match &entry.value {
                RawValue::Fields(owned) => {
                    fields += owned.len();
                    expected += owned.len() as u64 * std::mem::size_of::<RawField>() as u64;
                    expected += owned
                        .iter()
                        .map(|field| field.raw.len() as u64)
                        .sum::<u64>();
                }
                RawValue::Unsplit { raw, .. } => expected += raw.len() as u64,
            }
        }
        expected += document.nodes().len() as u64; // the consumed flags
        // The borrowed parse the content layer ran underneath: one line
        // row, one node row and one field row for every field.
        let borrowed = document.nodes().len() as u64
            * (std::mem::size_of::<cs_formats::text::TextLine>() as u64
                + std::mem::size_of::<cs_formats::text::KeyedListLine<'_>>() as u64)
            + fields as u64 * std::mem::size_of::<cs_formats::text::Field>() as u64;
        expected += borrowed;
        assert_eq!(context.allocation().used(), expected);

        let mut exact = ParseContext::new("exact", expected, 8);
        ConfigDocument::read(&mut exact, source(LAYOUT, MEMBER.len()), MEMBER)
            .expect("exactly the budget it books is enough");
        assert_eq!(exact.allocation().used(), expected);

        let mut short = ParseContext::new("short", expected - 1, 8);
        let error = ConfigDocument::read(&mut short, source(LAYOUT, MEMBER.len()), MEMBER)
            .expect_err("one byte short of the budget is refused");
        assert_eq!(error.code(), "parse");
        assert_eq!(
            short.allocation().used(),
            borrowed,
            "the refused attempt rolled its own charges back; the keyed \
             list parse before it had already succeeded and keeps them"
        );
    }

    /// Non-negotiable #5: unknown keys are retained and counted, and a
    /// duplicated key fails visibly.
    #[test]
    fn accept_f12_a_config_document_counts_unconsumed_keys() {
        let mut document = document();
        assert_eq!(
            document.accounting(),
            KeyAccounting {
                entries: 7,
                consumed: 0,
                unconsumed: 7,
                unclassified_lines: 1,
                unsplit_values: 1,
            }
        );
        assert!(matches!(
            document.lookup(None, b"KNOWN"),
            Lookup::Found(entry) if texts(entry) == vec![&b"1"[..]]
        ));
        assert_eq!(document.lookup(None, b"SLOT"), Lookup::Missing);
        assert_eq!(
            document.lookup(Some(b"PANEL"), b"DUP"),
            Lookup::Ambiguous(2)
        );
        // A named section never matches an entry before the first header,
        // whatever its case (`KNOWN` is in no section at all).
        assert_eq!(document.lookup(Some(b"panel"), b"KNOWN"), Lookup::Missing);

        let accounting = document.accounting();
        assert_eq!((accounting.consumed, accounting.unconsumed), (1, 6));
        let left: Vec<_> = document
            .unconsumed()
            .map(|(node, entry)| (node.line, entry.key.clone()))
            .collect();
        assert_eq!(
            left,
            vec![
                (4, b"SLOT".to_vec()),
                (5, b"SLOT".to_vec()),
                (6, b"DUP".to_vec()),
                (7, b"DUP".to_vec()),
                (8, b"BROKEN".to_vec()),
                (10, b"N\xe4me".to_vec()),
            ]
        );
        let broken = document
            .entries()
            .find(|entry| entry.key == b"BROKEN")
            .unwrap();
        assert_eq!(
            broken.value,
            RawValue::Unsplit {
                raw: b"\"open".to_vec(),
                issue: QuoteIssue::Unterminated
            }
        );
    }

    /// The typed, checked conversion F12-A left to this stage. A value becomes
    /// a `Tuning` only against a declared spec, and a value the spec rejects
    /// stays in the document as raw bytes.
    ///
    /// Authored; the fields are spelled like the observed dialect's but no
    /// original value is reproduced.
    const TUNED: &[u8] = b"[GUN]\r\n\
RATE=120,0.5\r\n\
AMMO=-1\r\n\
BIG=300\r\n\
HUGE=99999999999999999999\r\n\
HEX=0x20\r\n\
BAD=x1\r\n\
SPACED=  7  \r\n\
[BADFLOAT]\r\n\
NAN=nan\r\n\
INF=inf\r\n\
EXP=1e3\r\n\
FRAC=1.5\r\n\
HUGEFLOAT=1.7976931348623159e999\r\n";

    fn tuned() -> ConfigDocument {
        let mut context = ParseContext::with_defaults("fixture");
        ConfigDocument::read(&mut context, source(LAYOUT, TUNED.len()), TUNED)
            .expect("an observed keyed list member reads")
    }

    fn entry_of<'a>(document: &'a ConfigDocument, key: &[u8]) -> &'a ConfigEntry {
        document
            .entries()
            .find(|entry| entry.key == key)
            .expect("the fixture has this key")
    }

    fn tune(
        document: &ConfigDocument,
        key: &[u8],
        spec: FieldSpec,
        index: usize,
    ) -> Result<Tuning, TuneError> {
        TuningSchema::new(spec, entry_of(document, key)).tune(index)
    }

    /// AC02: a negative, an overflowing and a non-finite value cannot become a
    /// tuning constant. Each is refused with its own code, and the bytes stay
    /// in the document.
    #[test]
    fn accept_f12_b_negative_overflow_and_nan_never_become_tuning_constants() {
        let mut document = tuned();

        // A value the spec accepts is a `Tuning`, and it is the only way to
        // hold one.
        let rate = tune(
            &document,
            b"RATE",
            FieldSpec::integer(ValueWidth::Bits16, true),
            0,
        )
        .expect("120 fits a signed 16-bit field");
        assert_eq!(rate.value, 120.0);
        assert_eq!(rate.as_signed(), Some(120));
        assert_eq!(rate.as_unsigned(), None);
        assert_eq!(rate.line, 2);
        let half = tune(&document, b"RATE", FieldSpec::float(ValueWidth::Float32), 1)
            .expect("0.5 is finite");
        assert_eq!(half.value, 0.5);
        assert_eq!(half.as_signed(), None);
        assert_eq!(half.as_unsigned(), None);

        // Negative: an unsigned field refuses it, and a signed field of the
        // same width would accept it, so the refusal is the spec's and not the
        // value's shape.
        assert_eq!(
            tune(
                &document,
                b"AMMO",
                FieldSpec::integer(ValueWidth::Bits16, false),
                0
            ),
            Err(TuneError::Negative {
                width: ValueWidth::Bits16
            })
        );
        assert_eq!(
            tune(
                &document,
                b"AMMO",
                FieldSpec::integer(ValueWidth::Bits16, false),
                0
            )
            .unwrap_err()
            .code(),
            "negative"
        );
        // The raw bytes are still there, unchanged.
        let RawValue::Fields(fields) = &entry_of(&document, b"AMMO").value else {
            panic!("the value is split")
        };
        assert_eq!(fields[0].text(), b"-1");

        // Overflow: 300 in an 8-bit field, and a 20-digit value in any width.
        assert_eq!(
            tune(
                &document,
                b"BIG",
                FieldSpec::integer(ValueWidth::Bits8, false),
                0
            ),
            Err(TuneError::Overflow {
                width: ValueWidth::Bits8
            })
        );
        assert_eq!(
            tune(
                &document,
                b"HUGE",
                FieldSpec::integer(ValueWidth::Bits64, true),
                0
            ),
            Err(TuneError::Overflow {
                width: ValueWidth::Bits64
            })
        );
        // A hexadecimal value is subject to the same width check.
        let hex = tune(
            &document,
            b"HEX",
            FieldSpec::integer(ValueWidth::Bits8, false),
            0,
        )
        .expect("0x20 is 32");
        assert_eq!(hex.as_unsigned(), Some(32));
        assert_eq!(
            tune(
                &document,
                b"HEX",
                FieldSpec::integer(ValueWidth::Bits8, false),
                0
            )
            .map(|t| t.value),
            Ok(32.0)
        );

        // Not finite. `nan`, `inf` and an exponent are not spellings the
        // dialect's observed rules establish, so they are refused as text
        // before any arithmetic — and a value that *is* a number but not
        // finite is refused by the finiteness rule itself.
        for key in [b"BADFLOAT/NAN".as_slice(), b"BADFLOAT/INF", b"BADFLOAT/EXP"] {
            let (section, name) = (
                key.split(|byte| *byte == b'/').next().expect("a section"),
                key.split(|byte| *byte == b'/').nth(1).expect("a key"),
            );
            let Lookup::Found(entry) = document.lookup(Some(section), name) else {
                panic!("{key:?} is one entry");
            };
            let error = TuningSchema::new(FieldSpec::float(ValueWidth::Float64), entry)
                .tune(0)
                .expect_err("a non-finite spelling is refused");
            assert_eq!(error.code(), "not_a_number", "{key:?}");
        }
        assert_eq!(
            tune(
                &document,
                b"HUGEFLOAT",
                FieldSpec::float(ValueWidth::Float64),
                0
            ),
            Err(TuneError::NotANumber { len: 22 })
        );

        // A fractional value has no whole-number representation, so an integer
        // spec refuses it rather than truncating it into a tuning constant.
        assert_eq!(
            tune(
                &document,
                b"FRAC",
                FieldSpec::integer(ValueWidth::Bits16, true),
                0
            ),
            Err(TuneError::Overflow {
                width: ValueWidth::Bits16
            })
        );
        // The same value is fine as a float.
        assert_eq!(
            tune(&document, b"FRAC", FieldSpec::float(ValueWidth::Float64), 0).map(|t| t.value),
            Ok(1.5)
        );

        // A field no observed rule explains is not a number, whatever width the
        // spec declares.
        assert_eq!(
            tune(
                &document,
                b"BAD",
                FieldSpec::integer(ValueWidth::Bits32, true),
                0
            ),
            Err(TuneError::NotANumber { len: 2 })
        );

        // Blank bytes around a number are the dialect's own padding, so they
        // are trimmed; the value is the number.
        assert_eq!(
            tune(
                &document,
                b"SPACED",
                FieldSpec::integer(ValueWidth::Bits8, false),
                0
            )
            .map(|t| t.as_unsigned()),
            Ok(Some(7))
        );
    }

    /// The approved range is the numeric contract's condition for a tuning
    /// float value, and a value outside it is not a tuning constant.
    #[test]
    fn accept_f12_b_approved_ranges_bound_tuning_values() {
        let document = tuned();
        // 120 is inside [0, 200] and outside [0, 100].
        let spec = FieldSpec::integer(ValueWidth::Bits16, true).with_range(0.0, 200.0, "rounds/s");
        let rate = tune(&document, b"RATE", spec, 0).expect("120 is in range");
        assert_eq!(rate.unit, "rounds/s");
        let narrow =
            FieldSpec::integer(ValueWidth::Bits16, true).with_range(0.0, 100.0, "rounds/s");
        assert_eq!(
            tune(&document, b"RATE", narrow, 0),
            Err(TuneError::OutOfRange {
                min: Some(0.0),
                max: Some(100.0)
            })
        );
        // A half-open bound is still inclusive, so a value exactly on it reads.
        let edge = FieldSpec::float(ValueWidth::Float64).with_range(0.5, 0.5, "s");
        assert_eq!(tune(&document, b"RATE", edge, 1).map(|t| t.value), Ok(0.5));
        assert_eq!(
            FieldSpec::float(ValueWidth::Float64)
                .with_range(0.0, 1.0, "s")
                .min,
            Some(0.0)
        );
        // A negative lower bound accepts a negative signed value.
        let signed =
            FieldSpec::integer(ValueWidth::Bits16, true).with_range(-10.0, 10.0, "degrees");
        assert_eq!(
            tune(&document, b"AMMO", signed, 0).map(|t| t.as_signed()),
            Ok(Some(-1))
        );
        // A float with no approved range still has to be finite, which is the
        // numeric contract's own floor.
        let unbounded = FieldSpec::float(ValueWidth::Float32);
        assert_eq!(unbounded.min, None);
        assert_eq!(unbounded.max, None);
        assert!(tune(&document, b"RATE", unbounded, 1).is_ok());
    }

    /// The conversion reaches the production path a consumer uses: an entry
    /// looked up through the document, then converted against a spec.
    #[test]
    fn accept_f12_b_tuning_converts_a_looked_up_entry() {
        let mut document = tuned();
        // `lookup` marks the entry consumed, so the accounting sees the use.
        let Lookup::Found(entry) = document.lookup(Some(b"GUN"), b"RATE") else {
            panic!("one RATE entry in the section")
        };
        let entry = entry.clone();
        let mut spec = FieldSpec::integer(ValueWidth::Bits16, true);
        spec.unit = "rounds/s";
        let tuning = TuningSchema::new(spec, &entry)
            .tune(0)
            .expect("120 is in range");
        assert_eq!(tuning.value, 120.0);
        assert_eq!(tuning.as_signed(), Some(120));
        assert_eq!(tuning.line, 2);
        // Reading the same value through a spec that rejects it leaves the
        // entry consumed (the lookup happened) and the value raw. `120` is
        // inside `u8`, so the rejecting spec is the approved range.
        let refused = TuningSchema::new(
            FieldSpec::integer(ValueWidth::Bits8, false).with_range(0.0, 100.0, "rounds/s"),
            entry_of(&document, b"RATE"),
        )
        .tune(0);
        assert_eq!(
            refused.unwrap_err(),
            TuneError::OutOfRange {
                min: Some(0.0),
                max: Some(100.0)
            }
        );
        let accounting = document.accounting();
        assert_eq!(accounting.consumed, 1);
        assert_eq!(accounting.unconsumed, accounting.entries - 1);
        // An unsplittable value is refused as such, not as a bad number.
        let broken = b"KEY=\"open\r\n";
        let mut context = ParseContext::with_defaults("fixture");
        let document = ConfigDocument::read(&mut context, source(LAYOUT, broken.len()), broken)
            .expect("reads");
        let RawValue::Unsplit { issue, .. } = &entry_of(&document, b"KEY").value else {
            panic!("the value is unsplit")
        };
        assert_eq!(*issue, QuoteIssue::Unterminated);
        let error = TuningSchema::new(
            FieldSpec::integer(ValueWidth::Bits16, true),
            entry_of(&document, b"KEY"),
        )
        .tune(0)
        .expect_err("an unsplit value is refused");
        assert_eq!(error.code(), "unsplittable");
        // A field index past the end of the value is refused with the count.
        let error = TuningSchema::new(
            FieldSpec::integer(ValueWidth::Bits16, true),
            entry_of(&document, b"KEY"),
        )
        .tune(3);
        assert!(matches!(error, Err(TuneError::Unsplittable)));
        let single = b"A=1\r\n";
        let mut context = ParseContext::with_defaults("fixture");
        let document = ConfigDocument::read(&mut context, source(LAYOUT, single.len()), single)
            .expect("reads");
        assert_eq!(
            TuningSchema::new(
                FieldSpec::integer(ValueWidth::Bits16, true),
                entry_of(&document, b"A")
            )
            .tune(1)
            .unwrap_err(),
            TuneError::FieldCount { got: 1 }
        );
    }

    /// Every width and signedness the schema offers, at both of its edges, so
    /// the boundaries are tested rather than assumed.
    #[test]
    fn accept_f12_b_every_declared_width_rejects_its_own_overflow() {
        let document = tuned();
        // The fixture's `BIG=300` and `HUGE` are the values used.
        // `300` is inside every width but `u8`/`i8`, and it is rejected by
        // those two whether the field is signed or not.
        let cases = [
            (ValueWidth::Bits8, false, true),
            (ValueWidth::Bits8, true, true),
            (ValueWidth::Bits16, false, false),
            (ValueWidth::Bits16, true, false),
            (ValueWidth::Bits32, false, false),
            (ValueWidth::Bits32, true, false),
            (ValueWidth::Bits64, false, false),
            (ValueWidth::Bits64, true, false),
        ];
        for (width, signed, rejects_300) in cases {
            let result = tune(&document, b"BIG", FieldSpec::integer(width, signed), 0);
            assert_eq!(
                result.is_err(),
                rejects_300,
                "{width:?} signed={signed} on 300: {result:?}"
            );
        }
        // The negative value is refused by every unsigned width and accepted
        // by every signed one.
        for width in [
            ValueWidth::Bits8,
            ValueWidth::Bits16,
            ValueWidth::Bits32,
            ValueWidth::Bits64,
        ] {
            assert_eq!(
                tune(&document, b"AMMO", FieldSpec::integer(width, false), 0).unwrap_err(),
                TuneError::Negative { width },
                "{width:?} unsigned"
            );
            assert_eq!(
                tune(&document, b"AMMO", FieldSpec::integer(width, true), 0).map(|t| t.as_signed()),
                Ok(Some(-1)),
                "{width:?} signed"
            );
        }
        // The declared spans are the format's own, at both edges.
        assert_eq!(ValueWidth::Bits8.unsigned_range(), Some((0, 255)));
        assert_eq!(ValueWidth::Bits8.signed_range(), Some((-128, 127)));
        assert_eq!(
            ValueWidth::Bits64.signed_range().unwrap().1,
            i64::MAX as i128
        );
        assert!(ValueWidth::Float64.is_float());
        assert!(!ValueWidth::Bits32.is_float());
        assert_eq!(ValueWidth::Float64.signed_range(), None);
        assert_eq!(ValueWidth::Float32.unsigned_range(), None);
    }

    /// A signed width is bounded by its *own* maximum, not the unsigned
    /// maximum of the same bit count: `200` fits `u8` but not `i8`, and must
    /// be an overflow rather than a `Tuning` whose `as_signed` value the
    /// declared type could not carry (AC02).
    #[test]
    fn accept_f12_b_signed_widths_reject_values_above_their_signed_maximum() {
        // `(width, a value between the signed maximum and the unsigned
        // maximum of the same bit count, the signed maximum when it is
        // exactly representable as `f64`)`. `i64::MAX` is not, because the
        // value reader accumulates digits into an `f64`; the 64-bit boundary
        // is therefore deliberately not asserted, only the strictly greater
        // value.
        let cases = [
            (ValueWidth::Bits8, 200u64, Some(127i64)),
            (ValueWidth::Bits16, 40_000, Some(32_767)),
            (ValueWidth::Bits32, 3_000_000_000, Some(2_147_483_647)),
            (ValueWidth::Bits64, 10_000_000_000_000_000_000, None),
        ];
        for (width, between, max) in cases {
            let over = format!("V={between}\r\n");
            let mut context = ParseContext::with_defaults("fixture");
            let document =
                ConfigDocument::read(&mut context, source(LAYOUT, over.len()), over.as_bytes())
                    .expect("reads");
            assert_eq!(
                tune(&document, b"V", FieldSpec::integer(width, true), 0),
                Err(TuneError::Overflow { width }),
                "{width:?} signed must refuse {between}, above its signed maximum"
            );
            // The same value is fine in the unsigned field of the same width,
            // so the refusal is the signedness' and not the value's shape.
            assert_eq!(
                tune(&document, b"V", FieldSpec::integer(width, false), 0).map(|t| t.as_unsigned()),
                Ok(Some(between)),
                "{width:?} unsigned accepts {between}"
            );

            let Some(max) = max else { continue };
            let at_max = format!("V={max}\r\n");
            let mut context = ParseContext::with_defaults("fixture");
            let document = ConfigDocument::read(
                &mut context,
                source(LAYOUT, at_max.len()),
                at_max.as_bytes(),
            )
            .expect("reads");
            assert_eq!(
                tune(&document, b"V", FieldSpec::integer(width, true), 0).map(|t| t.as_signed()),
                Ok(Some(max)),
                "{width:?} signed accepts its own maximum {max}"
            );
        }
    }

    /// Only members the inventory routes to a reader become documents.
    #[test]
    fn accept_f12_a_config_document_refuses_unrouted_members() {
        let mut context = ParseContext::with_defaults("fixture");
        let cases = [
            (source("ASSETS/OTHER.CSV", MEMBER.len()), "unknown_dialect"),
            (
                source("ASSETS/SCRIPTS/MAINMENU.SCRIPT", MEMBER.len()),
                "no_reader",
            ),
            (source(LAYOUT, MEMBER.len() + 1), "length_mismatch"),
        ];
        for (source, code) in cases {
            let error = ConfigDocument::read(&mut context, source, MEMBER).unwrap_err();
            assert_eq!(error.code(), code, "{error}");
            assert!(error.to_string().contains("crimson.rof"));
        }
        let error = ConfigDocument::read(
            &mut context,
            source("ASSETS/SCRIPTS/MAINMENU.SCRIPT", 1),
            b"x",
        )
        .unwrap_err();
        assert!(matches!(
            error,
            ConfigError::NoReader {
                dialect: TextDialect::UiScript,
                stage: "F13-A",
                ..
            }
        ));

        let mut tiny = ParseContext::new("tiny", 16, 4);
        let error =
            ConfigDocument::read(&mut tiny, source(LAYOUT, MEMBER.len()), MEMBER).unwrap_err();
        assert_eq!(error.code(), "parse");
    }

    // ------------------------------------------------------ F12-C PE fixtures

    // These PE images are **newly authored**: the public PE/COFF layout and
    // the resource-tree shape, with ids, languages, code pages and string
    // texts invented for the test. No original byte, string or resource name
    // is reproduced.

    /// The RVA every fixture image's `.rsrc` section sits at.
    const RSRC_RVA: u32 = 0x2000;

    /// The file offset the fixture's `.rsrc` raw bytes start at: the headers
    /// rounded up to the section alignment. Provenance offsets are file
    /// offsets, not RVAs.
    const RSRC_FILE_OFFSET: u64 = 0x200;

    /// One authored `RT_STRING` block.
    struct Block {
        block_id: u32,
        language: u32,
        code_page: u32,
        entries: &'static [&'static str],
    }

    /// Where the fixture image's whole bytes came from. A loose PE file is
    /// its own container, so the member key is `None`.
    fn image_source(length: usize) -> SourceSpan {
        SourceSpan::new(
            ContentHash::from_bytes([9; 32]),
            "strings.dll",
            None,
            0,
            length as u64,
            None,
        )
        .expect("valid span")
    }

    /// Assembles a minimal single-section PE32 image whose `.rsrc` section is
    /// `rsrc`.
    fn fixture_image(rsrc: &[u8]) -> Vec<u8> {
        let header_end = 0x80 + 4 + 20 + 224 + 40;
        let raw = (header_end + 0x1ff) & !0x1ff;
        let mut out = vec![0u8; header_end];
        out[0..2].copy_from_slice(b"MZ");
        out[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        out[0x80..0x84].copy_from_slice(b"PE\0\0");
        let coff = 0x84;
        out[coff..coff + 2].copy_from_slice(&0x014cu16.to_le_bytes());
        out[coff + 2..coff + 4].copy_from_slice(&1u16.to_le_bytes());
        out[coff + 16..coff + 18].copy_from_slice(&224u16.to_le_bytes());
        let optional = coff + 20;
        out[optional..optional + 2].copy_from_slice(&0x010bu16.to_le_bytes());
        out[optional + 92..optional + 96].copy_from_slice(&16u32.to_le_bytes());
        out[optional + 60..optional + 64].copy_from_slice(&(header_end as u32).to_le_bytes());
        out[optional + 112..optional + 116].copy_from_slice(&RSRC_RVA.to_le_bytes());
        out[optional + 116..optional + 120].copy_from_slice(&(rsrc.len() as u32).to_le_bytes());
        let section = optional + 224;
        out[section..section + 5].copy_from_slice(b".rsrc");
        out[section + 8..section + 12].copy_from_slice(&(rsrc.len() as u32).to_le_bytes());
        out[section + 12..section + 16].copy_from_slice(&RSRC_RVA.to_le_bytes());
        out[section + 16..section + 20].copy_from_slice(&(rsrc.len() as u32).to_le_bytes());
        out[section + 20..section + 24].copy_from_slice(&(raw as u32).to_le_bytes());
        out.resize(raw + rsrc.len(), 0);
        out[raw..raw + rsrc.len()].copy_from_slice(rsrc);
        out
    }

    fn rsrc_dir(bytes: &mut Vec<u8>, ids: usize) -> usize {
        let at = bytes.len();
        bytes.extend_from_slice(&[0u8; 16]);
        bytes.extend_from_slice(&vec![0u8; ids * 8]);
        bytes[at + 14..at + 16].copy_from_slice(&(ids as u16).to_le_bytes());
        at
    }

    fn rsrc_row(dir: usize, index: usize) -> usize {
        dir + 16 + index * 8
    }

    fn rsrc_id(bytes: &mut [u8], dir: usize, index: usize, id: u32) {
        let row = rsrc_row(dir, index);
        bytes[row..row + 4].copy_from_slice(&id.to_le_bytes());
    }

    fn rsrc_sub(bytes: &mut [u8], dir: usize, index: usize, child: usize) {
        let row = rsrc_row(dir, index);
        bytes[row + 4..row + 8].copy_from_slice(&(0x8000_0000u32 | child as u32).to_le_bytes());
    }

    fn rsrc_data(bytes: &mut [u8], dir: usize, index: usize, entry: usize) {
        let row = rsrc_row(dir, index);
        bytes[row + 4..row + 8].copy_from_slice(&(entry as u32).to_le_bytes());
    }

    /// Reserves a directory with `named` name entries and `ids` id entries, in
    /// the format's order (names first).
    fn rsrc_named_dir(bytes: &mut Vec<u8>, named: usize, ids: usize) -> usize {
        let at = bytes.len();
        bytes.extend_from_slice(&[0u8; 16]);
        bytes.extend_from_slice(&vec![0u8; (named + ids) * 8]);
        bytes[at + 12..at + 14].copy_from_slice(&(named as u16).to_le_bytes());
        bytes[at + 14..at + 16].copy_from_slice(&(ids as u16).to_le_bytes());
        at
    }

    /// Writes `text` as a *name* key: the UTF-16 name struct is appended to
    /// the section and the row's name word is pointed at it. The directory
    /// must have a named-entry slot ([`rsrc_named_dir`]).
    fn rsrc_name(bytes: &mut Vec<u8>, dir: usize, index: usize, text: &str) {
        let row = rsrc_row(dir, index);
        let at = bytes.len() as u32;
        let units: Vec<u16> = text.encode_utf16().collect();
        bytes.extend_from_slice(&(units.len() as u16).to_le_bytes());
        for unit in units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes[row..row + 4].copy_from_slice(&(0x8000_0000u32 | at).to_le_bytes());
    }

    /// Sixteen counted UTF-16LE units, the first `entries.len()` non-empty.
    fn string_payload(entries: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        for index in 0..16 {
            let text = entries.get(index).copied().unwrap_or("");
            let units: Vec<u16> = text.encode_utf16().collect();
            out.extend_from_slice(&(units.len() as u16).to_le_bytes());
            for unit in units {
                out.extend_from_slice(&unit.to_le_bytes());
            }
        }
        out
    }

    /// Builds a resource section holding `blocks`, and with `extra_type` a
    /// second resource type (`4001`) whose payload is six opaque bytes, not a
    /// string block.
    fn rsrc_section(blocks: &[Block], extra_type: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        let root = rsrc_dir(&mut bytes, if extra_type { 2 } else { 1 });
        rsrc_id(&mut bytes, root, 0, RT_STRING);
        let strings = rsrc_dir(&mut bytes, blocks.len());
        rsrc_sub(&mut bytes, root, 0, strings);
        for (index, block) in blocks.iter().enumerate() {
            rsrc_id(&mut bytes, strings, index, block.block_id);
            let languages = rsrc_dir(&mut bytes, 1);
            rsrc_id(&mut bytes, languages, 0, block.language);
            rsrc_sub(&mut bytes, strings, index, languages);
            let payload = string_payload(block.entries);
            let rva = RSRC_RVA + bytes.len() as u32;
            bytes.extend_from_slice(&payload);
            let entry = bytes.len();
            bytes.extend_from_slice(&rva.to_le_bytes());
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&block.code_page.to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
            rsrc_data(&mut bytes, languages, 0, entry);
        }
        if extra_type {
            rsrc_id(&mut bytes, root, 1, 4001);
            let names = rsrc_dir(&mut bytes, 1);
            rsrc_id(&mut bytes, names, 0, 0);
            rsrc_sub(&mut bytes, root, 1, names);
            let payload = b"opaque";
            let rva = RSRC_RVA + bytes.len() as u32;
            bytes.extend_from_slice(payload);
            let entry = bytes.len();
            bytes.extend_from_slice(&rva.to_le_bytes());
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&1200u32.to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
            rsrc_data(&mut bytes, names, 0, entry);
        }
        bytes
    }

    fn one_block() -> Vec<u8> {
        fixture_image(&rsrc_section(
            &[Block {
                block_id: 1,
                language: 1033,
                code_page: 1252,
                entries: &["alpha", "", "gamma"],
            }],
            false,
        ))
    }

    // ------------------------------------------------- F12-C catalog tests

    /// The string catalog resolves a stable id (with its language and code
    /// page) to its display text and its provenance. An empty unit is present
    /// and empty, never a missing string.
    #[test]
    fn accept_f12_c_string_catalog_resolves_ids_languages_and_provenance() {
        let image = one_block();
        let mut context = ParseContext::with_defaults("strings.dll");
        let catalog = StringCatalog::read(&mut context, image_source(image.len()), &image)
            .expect("an authored PE image reads");
        assert_eq!(catalog.rows().len(), 16);
        assert_eq!(catalog.languages(), vec![1033]);
        assert_eq!(catalog.source().container_path(), "strings.dll");
        assert_eq!(catalog.source().member_key(), None);

        let StringLookup::Found(alpha) = catalog.resolve(0, Some(1033)) else {
            panic!("string 0 under en-US")
        };
        assert_eq!(alpha.id, 0);
        assert_eq!(alpha.language, 1033);
        assert_eq!(alpha.code_page, 1252);
        assert_eq!(
            alpha.code_units,
            "alpha".encode_utf16().collect::<Vec<u16>>()
        );
        assert_eq!(alpha.text.as_deref(), Some("alpha"));
        // Provenance points inside the resource section, at the block's
        // extent, and names the installation the image belongs to.
        assert_eq!(
            alpha.span.install_sha256(),
            ContentHash::from_bytes([9; 32])
        );
        assert_eq!(alpha.span.container_path(), "strings.dll");
        assert_eq!(alpha.span.member_key(), None);
        assert!(alpha.span.offset() >= RSRC_FILE_OFFSET);
        assert!(alpha.span.length() >= 32);

        let StringLookup::Found(empty) = catalog.resolve(1, Some(1033)) else {
            panic!("an empty unit is present")
        };
        assert!(empty.code_units.is_empty());
        assert_eq!(empty.text.as_deref(), Some(""));

        // The language is part of the identity: the same id under another
        // language is not this string.
        assert_eq!(catalog.resolve(0, Some(1)), StringLookup::Missing);
        assert_eq!(catalog.resolve(9000, None), StringLookup::Missing);
        assert_eq!(
            catalog.accounting(),
            StringAccounting {
                strings: 16,
                undecodable: 0,
                other_leaves: 0,
                duplicate_ids: 0,
            }
        );
    }

    /// Resource types no string reader owns are retained beside the strings
    /// (spec F12, non-negotiable #5 applied to the tree), and two strings for
    /// one `(id, language)` are reported as ambiguous, never merged.
    #[test]
    fn accept_f12_c_string_catalog_retains_other_types_and_reports_duplicates() {
        let image = fixture_image(&rsrc_section(
            &[
                Block {
                    block_id: 1,
                    language: 1033,
                    code_page: 1252,
                    entries: &["one"],
                },
                Block {
                    block_id: 1,
                    language: 1033,
                    code_page: 1252,
                    entries: &["two"],
                },
            ],
            true,
        ));
        let mut context = ParseContext::with_defaults("strings.dll");
        let catalog = StringCatalog::read(&mut context, image_source(image.len()), &image)
            .expect("an authored PE image reads");

        let accounting = catalog.accounting();
        assert_eq!(accounting.strings, 32);
        assert_eq!(accounting.other_leaves, 1);
        assert_eq!(accounting.undecodable, 0);
        // A repeated block id repeats every one of its sixteen ids.
        assert_eq!(accounting.duplicate_ids, 16);

        // Two strings answer `(1033, 0)`: ambiguous, not a silent choice.
        assert_eq!(catalog.resolve(0, Some(1033)), StringLookup::Ambiguous(2));

        // The non-string leaf is retained with its type and code page, and
        // was never read as a string.
        let other: Vec<&ResourceLeaf> = catalog.other_leaves().collect();
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].key(0).and_then(ResourceKey::id), Some(4001));
        assert_eq!(other[0].data.code_page, 1200);
        assert_eq!(
            catalog.resources().leaves().len(),
            other.len() + catalog.resources().strings().len()
        );
    }

    /// A three-level leaf under `RT_STRING` whose second level is a *name* is
    /// not a string block: the resource reader keeps it as a plain leaf, so
    /// the catalog must count it among the other leaves rather than let it
    /// vanish from the accounting (spec F12, non-negotiable #5). The reader
    /// supports names at any level even though no surveyed image uses one.
    #[test]
    fn accept_f12_c_string_catalog_counts_a_name_keyed_leaf_as_another_leaf() {
        let mut rsrc = Vec::new();
        let root = rsrc_dir(&mut rsrc, 1);
        rsrc_id(&mut rsrc, root, 0, RT_STRING);
        // The second level is a name, not a block id: `RT_STRING/<name>/1033`.
        let strings = rsrc_named_dir(&mut rsrc, 1, 0);
        rsrc_sub(&mut rsrc, root, 0, strings);
        rsrc_name(&mut rsrc, strings, 0, "blocks");
        let languages = rsrc_dir(&mut rsrc, 1);
        rsrc_sub(&mut rsrc, strings, 0, languages);
        rsrc_id(&mut rsrc, languages, 0, 1033);
        let payload = string_payload(&["not decoded as a block"]);
        let rva = RSRC_RVA + rsrc.len() as u32;
        rsrc.extend_from_slice(&payload);
        let entry = rsrc.len();
        rsrc.extend_from_slice(&rva.to_le_bytes());
        rsrc.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        rsrc.extend_from_slice(&1252u32.to_le_bytes());
        rsrc.extend_from_slice(&0u32.to_le_bytes());
        rsrc_data(&mut rsrc, languages, 0, entry);
        let image = fixture_image(&rsrc);

        let mut context = ParseContext::with_defaults("strings.dll");
        let catalog = StringCatalog::read(&mut context, image_source(image.len()), &image)
            .expect("an authored PE image reads");

        // No block was decoded from it, and the leaf is retained and counted,
        // not silently dropped because its type number is `RT_STRING`.
        assert!(catalog.rows().is_empty(), "{:?}", catalog.rows());
        assert_eq!(catalog.resources().strings().len(), 0);
        assert_eq!(catalog.resources().leaves().len(), 1);
        assert_eq!(catalog.accounting().strings, 0);
        assert_eq!(catalog.accounting().other_leaves, 1);
        assert_eq!(catalog.resources().leaves().len(), 1);
        let other: Vec<&ResourceLeaf> = catalog.other_leaves().collect();
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].key(0).and_then(ResourceKey::id), Some(RT_STRING));
        assert_eq!(
            other[0].key(1).and_then(ResourceKey::text).as_deref(),
            Some("blocks")
        );
    }

    /// AC03: a malformed PE resource offset is refused structurally, as inert
    /// data, and never becomes a catalog. The same image shape reads when the
    /// offset is sound, so the refusal is the corruption's and not the
    /// fixture's.
    #[test]
    fn accept_f12_c_malformed_pe_resource_offsets_are_refused_not_loaded() {
        let good = rsrc_section(
            &[Block {
                block_id: 1,
                language: 1033,
                code_page: 1252,
                entries: &["alpha"],
            }],
            false,
        );
        let good_image = fixture_image(&good);
        let mut context = ParseContext::with_defaults("strings.dll");
        assert!(
            StringCatalog::read(&mut context, image_source(good_image.len()), &good_image).is_ok(),
            "the uncorrupted fixture reads"
        );

        // A data entry whose RVA no section covers. The data entry is the
        // section's last 16 bytes, and its first word is the RVA.
        let mut bad = good.clone();
        let end = bad.len();
        bad[end - 16..end - 12].copy_from_slice(&0x9000u32.to_le_bytes());
        let bad_image = fixture_image(&bad);
        let mut context = ParseContext::with_defaults("strings.dll");
        let error = StringCatalog::read(&mut context, image_source(bad_image.len()), &bad_image)
            .expect_err("an unmapped data RVA is refused, never loaded");
        assert_eq!(error.code(), "outside_table");
        assert!(error.to_string().contains("strings.dll"));
        match error {
            StringCatalogError::Pe(pe) => {
                assert_eq!(pe.code(), "outside_table");
                assert!(pe.offset() >= RSRC_FILE_OFFSET);
            }
            other => panic!("{other}"),
        }

        // A subdirectory whose offset leaves the resource section. The root
        // directory's first entry's offset word is at byte 20 of the section.
        let mut bad = good.clone();
        bad[20..24].copy_from_slice(&(0x8000_0000u32 | 0x7fff).to_le_bytes());
        let bad_image = fixture_image(&bad);
        let mut context = ParseContext::with_defaults("strings.dll");
        let error = StringCatalog::read(&mut context, image_source(bad_image.len()), &bad_image)
            .expect_err("an out-of-table subdirectory is refused, never followed");
        assert_eq!(error.code(), "outside_table");

        // A resource directory that points at itself is a cycle, refused
        // rather than followed forever.
        let mut rsrc = Vec::new();
        let root = rsrc_dir(&mut rsrc, 1);
        rsrc_sub(&mut rsrc, root, 0, root);
        let cycle_image = fixture_image(&rsrc);
        let mut context = ParseContext::with_defaults("strings.dll");
        let error =
            StringCatalog::read(&mut context, image_source(cycle_image.len()), &cycle_image)
                .expect_err("a self-referential directory is refused");
        assert_eq!(error.code(), "directory_cycle");
    }

    /// A file that is not the image's declared length is refused before it is
    /// parsed, so a truncated read never becomes a catalog.
    #[test]
    fn accept_f12_c_string_catalog_refuses_a_length_mismatch() {
        let image = one_block();
        let mut context = ParseContext::with_defaults("strings.dll");
        let error = StringCatalog::read(
            &mut context,
            image_source(image.len() + 1),
            &image[..image.len() - 1],
        )
        .expect_err("a truncated image is refused before parsing");
        assert_eq!(error.code(), "length_mismatch");
    }

    /// AC03's second half: the string path is a parser, not a platform
    /// loader. The production sources are scanned for the identifiers a
    /// dynamic-loader call or binding would need; adding one there makes this
    /// fail. The behavioural test above pins the other half — a malformed
    /// offset is a structured refusal.
    #[test]
    fn accept_f12_c_no_platform_loader_in_the_string_path() {
        let sources = [
            include_str!("config.rs"),
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../cs_formats/src/pe_resources.rs"
            )),
        ];
        // Split so the guard's own source does not contain the words it
        // searches for.
        let loaders = [
            concat!("Load", "Library"),
            concat!("Get", "ProcAddress"),
            concat!("Free", "Library"),
            concat!("lib", "loading"),
            concat!("dl", "open"),
            concat!("dl", "sym"),
            concat!("Load", "StringW"),
            concat!("Load", "StringA"),
            concat!("win", "api"),
        ];
        for source in sources {
            for loader in loaders {
                assert!(
                    !source.contains(loader),
                    "the string path must not reference {loader}"
                );
            }
        }
        // The scan read real sources, not empty strings.
        assert!(sources[0].contains("StringCatalog"));
        assert!(sources[1].contains("read_pe_resources"));
    }

    /// Typed tuning through the catalog: a declared list of fields is looked
    /// up in the document and converted against each declaration, counting
    /// what it consumed. Missing, ambiguous and refused fields are explicit,
    /// and the bytes stay in the document.
    #[test]
    fn accept_f12_c_declared_tuning_fields_resolve_through_the_document() {
        let mut doc = tuned();
        let bindings = [
            FieldBinding {
                consumer: "gun.rate",
                section: Some(b"GUN"),
                key: b"RATE",
                index: 0,
                spec: FieldSpec::integer(ValueWidth::Bits16, true),
            },
            FieldBinding {
                consumer: "gun.ammo",
                section: Some(b"GUN"),
                key: b"AMMO",
                index: 0,
                spec: FieldSpec::integer(ValueWidth::Bits16, false),
            },
            FieldBinding {
                consumer: "gun.big",
                section: Some(b"GUN"),
                key: b"BIG",
                index: 0,
                spec: FieldSpec::integer(ValueWidth::Bits8, false),
            },
            FieldBinding {
                consumer: "gun.absent",
                section: Some(b"GUN"),
                key: b"ABSENT",
                index: 0,
                spec: FieldSpec::integer(ValueWidth::Bits8, false),
            },
        ];
        let report = resolve_tunings(&mut doc, &bindings);
        assert_eq!(report.resolved().len(), 4);
        assert!(
            matches!(report.resolved()[0].outcome, TuningOutcome::Known(t) if t.as_signed() == Some(120))
        );
        assert!(matches!(
            report.resolved()[1].outcome,
            TuningOutcome::Refused(TuneError::Negative { .. })
        ));
        assert!(matches!(
            report.resolved()[2].outcome,
            TuningOutcome::Refused(TuneError::Overflow { .. })
        ));
        assert_eq!(report.resolved()[3].outcome, TuningOutcome::Missing);
        assert!(!report.all_known());
        assert_eq!(report.failures().count(), 3);
        // The three found entries were counted consumed; the rest stay for
        // the parity accounting.
        let accounting = doc.accounting();
        assert_eq!(accounting.consumed, 3);
        assert_eq!(accounting.unconsumed, accounting.entries - 3);

        // A key two entries share is ambiguous: the lookup refused to choose,
        // so nothing was converted and nothing was consumed.
        let mut member = document();
        let duplicated = [FieldBinding {
            consumer: "panel.dup",
            section: Some(b"PANEL"),
            key: b"DUP",
            index: 0,
            spec: FieldSpec::integer(ValueWidth::Bits8, false),
        }];
        let report = resolve_tunings(&mut member, &duplicated);
        assert_eq!(report.resolved()[0].outcome, TuningOutcome::Ambiguous(2));
        assert_eq!(member.accounting().consumed, 0);
    }

    /// Authored for **R1**: two keys in one section that differ only in case
    /// (so folding makes them collide), a section name, and a key padded
    /// before its `=` on an indented line.
    const CASED: &[u8] = b"[@Panel@]\r\n\
TITLE=T,first\r\n\
title=T,second\r\n\
\x20 PADDED  =P,art.png\r\n\
Caf\xc3\xa9=x\r\n";

    fn cased() -> ConfigDocument {
        let mut context = ParseContext::with_defaults("fixture");
        ConfigDocument::read(&mut context, source(LAYOUT, CASED.len()), CASED)
            .expect("an observed keyed list member reads")
    }

    /// **R1**: a lookup resolves a name written in a different case, keeps
    /// the bytes as the member wrote them, and refuses to choose between two
    /// entries the fold made collide. The fold is ASCII-only: a byte above
    /// `0x7F` is compared as itself, because what the original does with it
    /// is not established.
    #[test]
    fn accept_t351_lookup_resolves_names_without_regard_to_case() {
        let mut document = cased();

        let Lookup::Found(entry) = document.lookup(Some(b"@PANEL@"), b"padded") else {
            panic!("a padded key, an indented line and a folded section")
        };
        assert_eq!(entry.key, b"PADDED", "the bytes stay as written");
        assert_eq!(entry.section.as_deref(), Some(&b"@Panel@"[..]));
        assert_eq!(texts(entry), vec![&b"P"[..], b"art.png"]);
        assert_eq!(entry.line, 4);

        // The two keys that differ only in case both answer, and neither is
        // consumed: a fold that picked one would hide the collision.
        assert_eq!(
            document.lookup(Some(b"@panel@"), b"Title"),
            Lookup::Ambiguous(2)
        );
        assert_eq!(document.accounting().consumed, 1);

        // A name that differs in more than case is still missing.
        assert_eq!(
            document.lookup(Some(b"[@Panel@]"), b"padde"),
            Lookup::Missing
        );
        assert_eq!(document.lookup(None, b"TITLE"), Lookup::Missing);

        // The fold is ASCII: the UTF-8 name is found by its own bytes and
        // not by folding them against any other spelling.
        let Lookup::Found(entry) = document.lookup(Some(b"@Panel@"), b"Caf\xc3\xa9") else {
            panic!("the exact bytes of the name")
        };
        assert_eq!(entry.key, "Café".as_bytes());
        assert_eq!(
            document.lookup(Some(b"@Panel@"), b"CAF\xc3\x89"),
            Lookup::Missing,
            "a byte above 0x7F is not folded"
        );
        assert_eq!(document.accounting().unconsumed, 2, "the colliding pair");
    }

    /// **R4** through owned nodes: a field's `value` is what a consumer
    /// compares, `text` and `raw` keep the bytes, and a padded field still
    /// converts to the same typed constant as an unpadded one.
    #[test]
    fn accept_t351_field_value_drops_surrounding_blanks() {
        /// Authored: a field padded at both ends, one padded at the end and a
        /// numeric field padded around its digits — the three shapes the
        /// retail member actually has — next to a quoted field whose blanks
        /// are *inside* its quotes.
        const PADDED: &[u8] = b"[BOOK]\r\n\
ITEM=  P  ,IDS_TITLE  ,  42  ,\" 0, 0,0 \"\r\n";
        let mut context = ParseContext::with_defaults("fixture");
        let mut document = ConfigDocument::read(&mut context, source(LAYOUT, PADDED.len()), PADDED)
            .expect("an observed keyed list member reads");
        assert_eq!(document.reassemble(), PADDED, "every byte survives");
        let Lookup::Found(entry) = document.lookup(Some(b"BOOK"), b"ITEM") else {
            panic!("the fixture has this entry")
        };
        let RawValue::Fields(fields) = &entry.value else {
            panic!("splits")
        };
        let values: Vec<_> = fields.iter().map(RawField::value).collect();
        assert_eq!(
            values,
            vec![&b"P"[..], b"IDS_TITLE", b"42", b" 0, 0,0 "],
            "the blanks around every field are dropped; a blank inside the \
             quotes is the value's own"
        );
        assert_eq!(fields[1].raw, b"IDS_TITLE  ", "the bytes stay as written");
        assert_eq!(fields[1].text(), b"IDS_TITLE  ");
        assert!(fields[3].quoted);
        assert_eq!(fields[3].raw, b"\" 0, 0,0 \"");
        assert_eq!(fields[3].value(), b" 0, 0,0 ");
        assert_eq!(fields[3].text(), b" 0, 0,0 ");

        // The production path: the padded digits are the same constant as the
        // unpadded ones, because `tune` reads `value`.
        let schema = TuningSchema::new(
            FieldSpec::integer(ValueWidth::Bits32, true).with_range(0.0, 100.0, "px"),
            entry,
        );
        let tuning = schema.tune(2).expect("a padded field reads");
        assert_eq!(tuning.as_signed(), Some(42));
        assert_eq!(tuning.unit, "px");
        assert_eq!(tuning.line, 2);
    }

    // ------------------------------------------------- F12-E placeholders

    /// Authored: a global definition, a section-local one and three
    /// references into `[@Panel@]` — one answered globally, one locally and
    /// one that no definition carries.
    const PLACEHOLDERS: &[u8] = b"[GLOBALVARS]\r\n\
G1=WIDTH,640\r\n\
[@Panel@]\r\n\
V1=LEFT,10\r\n\
W=<WIDTH>\r\n\
L=<LEFT>\r\n\
N=<NOPE>\r\n";

    fn placeholders() -> ConfigDocument {
        let mut context = ParseContext::with_defaults("fixture");
        ConfigDocument::read(
            &mut context,
            source(LAYOUT, PLACEHOLDERS.len()),
            PLACEHOLDERS,
        )
        .expect("an observed keyed list member reads")
    }

    /// Task #370: the document resolves the member's `<NAME>` references
    /// against its section-local and global definitions, reports an
    /// unresolved one rather than guessing, and leaves every byte of the
    /// member as written.
    #[test]
    fn accept_f12_e_document_resolves_placeholders_and_keeps_the_raw_bytes() {
        let document = placeholders();
        assert_eq!(document.reassemble(), PLACEHOLDERS, "every byte survives");
        assert_eq!(
            document.placeholders().accounting(),
            cs_formats::text::PlaceholderAccounting {
                definitions: 2,
                local_definitions: 1,
                global_definitions: 1,
                references: 3,
                resolved_local: 1,
                resolved_global: 1,
                unresolved: 1,
            }
        );
        let unresolved: Vec<&[u8]> = document
            .placeholders()
            .unresolved()
            .map(|reference| reference.name.as_slice())
            .collect();
        assert_eq!(unresolved, vec![&b"NOPE"[..]], "reported, not guessed");

        // The pass is not a conversion: the raw entry still spells the
        // placeholder, a spec refuses it as text, and the resolved value is
        // bytes a consumer may parse itself once it declares a schema
        // (non-negotiable #2).
        let entry = entry_of(&document, b"W");
        assert_eq!(texts(entry), vec![&b"<WIDTH>"[..]]);
        assert_eq!(
            TuningSchema::new(FieldSpec::integer(ValueWidth::Bits16, false), entry).tune(0),
            Err(TuneError::NotANumber { len: 7 }),
            "the document did not convert the placeholder"
        );
        let reference = &document.placeholders().references()[0];
        assert_eq!(reference.key, b"W");
        assert_eq!(reference.field, 0);
        let resolved = document
            .placeholders()
            .resolved(reference)
            .expect("the global definition answers");
        assert_eq!(resolved.scope, cs_formats::text::PlaceholderScope::Global);
        assert_eq!(resolved.value, b"640");

        // Non-negotiable #5: every entry is still retained and counted.
        assert_eq!(document.accounting().entries, 5);
        assert_eq!(document.accounting().consumed, 0);
        assert_eq!(document.accounting().unconsumed, 5);
    }

    // ------------------------------------------------- F12-H: is a value ever
    // a fraction?

    /// Authored; shaped like the surveyed dialect, no original byte in it.
    const FRACTIONS: &[u8] = b"[PANEL]\r\n\
A=5.5\r\n\
B=5.\r\n\
C=.5\r\n\
D=5.5.5\r\n\
E=.\r\n\
F=1.5e3\r\n\
G=640\r\n\
H=-2.5\r\n\
I=0x10\r\n\
J=0x1.8\r\n";

    fn fractions() -> ConfigDocument {
        let mut context = ParseContext::with_defaults("fixture");
        ConfigDocument::read(&mut context, source(LAYOUT, FRACTIONS.len()), FRACTIONS)
            .expect("an observed keyed list member reads")
    }

    /// The single `.` in [`TuningSchema::tune`] is a **designed** reading
    /// rule, and this is the authored half of task #369's measurement that
    /// says so: the retail survey next to it found no fractional field value
    /// in either member of `crimson.rof`, so nothing in the installed data
    /// needs this spelling.
    ///
    /// It pins the rule on both sides, because the evidence class is a claim
    /// about *behaviour* as much as about prose: the reading must accept the
    /// three shapes a tuning float can be written in, must refuse the shapes
    /// the survey never found, and must never hand a whole-number field a
    /// rounded fraction. Removing the `.` branch from the value reader turns
    /// the first group into `NotANumber`, so this test fails; widening it to
    /// an exponent or a second `.` turns the second group into a number, so
    /// it fails there too.
    #[test]
    fn accept_f12_h_the_fractional_spelling_is_a_designed_rule() {
        let document = fractions();
        assert_eq!(document.reassemble(), FRACTIONS, "every byte survives");
        assert_eq!(document.accounting().entries, 10);

        let float = FieldSpec::float(ValueWidth::Float32);
        let whole = FieldSpec::integer(ValueWidth::Bits16, true);
        // The three shapes the rule accepts: digits on both sides, digits
        // before, digits after. `5.` and `.5` are the spellings the F12-B
        // review found the prose and the code disagreeing about; the code was
        // kept, so the prose names them.
        assert_eq!(
            tune(&document, b"A", float, 0),
            Ok(Tuning {
                value: 5.5,
                signed: None,
                unsigned: None,
                unit: "",
                line: 2,
            })
        );
        assert_eq!(
            tune(&document, b"B", float, 0).map(|tuning| tuning.value),
            Ok(5.0)
        );
        assert_eq!(
            tune(&document, b"C", float, 0).map(|tuning| tuning.value),
            Ok(0.5)
        );
        // A whole-number field is never handed a rounded fraction: `5.5` and
        // `.5` have no `i16` representation, and `5.` does.
        assert_eq!(
            tune(&document, b"A", whole, 0),
            Err(TuneError::Overflow {
                width: ValueWidth::Bits16
            })
        );
        assert_eq!(
            tune(&document, b"C", whole, 0),
            Err(TuneError::Overflow {
                width: ValueWidth::Bits16
            })
        );
        assert_eq!(
            tune(&document, b"B", whole, 0).map(|tuning| tuning.signed),
            Ok(Some(5))
        );
        // A sign in front of a fraction is the one sign the rule does apply
        // to, because the sign is a rule in its own right.
        assert_eq!(
            tune(&document, b"H", float, 0).map(|tuning| tuning.value),
            Ok(-2.5)
        );
        // A second `.`, a bare `.` and an exponent are not spellings: an
        // installed value using one would be text this reader cannot
        // interpret, and the survey found none. `J` is the hexadecimal branch
        // asked the same question, because that is a spelling `LAYOUT.CSV`
        // does use (444 values of it).
        for (key, len) in [
            (&b"D"[..], 5usize),
            (&b"E"[..], 1),
            (&b"F"[..], 5),
            (&b"J"[..], 5),
        ] {
            assert_eq!(
                tune(&document, key, float, 0),
                Err(TuneError::NotANumber { len }),
                "{key:?}"
            );
        }
        // A plain decimal is unaffected, which is what the retail members do
        // contain, and so is a hexadecimal value.
        assert_eq!(
            tune(&document, b"G", whole, 0).map(|tuning| tuning.signed),
            Ok(Some(640))
        );
        assert_eq!(
            tune(&document, b"I", whole, 0).map(|tuning| tuning.signed),
            Ok(Some(16))
        );
        // Every refusal left the member exactly as it was.
        assert_eq!(document.reassemble(), FRACTIONS);
    }

    /// Reads the original `crimson.rof` **read-only**, through the production
    /// ROF reader.
    fn retail_crimson_rof() -> Vec<u8> {
        let dir = std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR is not set: this test needs the original installation");
        let path = std::path::PathBuf::from(dir).join(CRIMSON_ROF);
        std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
    }

    /// One member of the retail container, decoded by
    /// [`cs_formats::read_member`] and then parsed, so a survey can count a
    /// byte of the member as well as a value of its document.
    struct RetailMember {
        /// The decoded member, byte for byte as the container stores it.
        bytes: Vec<u8>,
        /// The same bytes read as a keyed field list.
        document: ConfigDocument,
    }

    fn retail_keyed_list(rof: &[u8], member: &str) -> RetailMember {
        let mut context = ParseContext::with_defaults(CRIMSON_ROF);
        let tree = cs_formats::read_tree(&mut context, rof).expect("the retail container walks");
        let record = tree
            .members()
            .iter()
            .find(|record| {
                record
                    .path
                    .iter()
                    .map(|segment| String::from_utf8_lossy(segment).into_owned())
                    .collect::<Vec<_>>()
                    .join("/")
                    .eq_ignore_ascii_case(member)
            })
            .unwrap_or_else(|| panic!("{member} is in the retail container"));
        let read =
            cs_formats::read_member(&context, rof, record, &cs_formats::RofLimits::default())
                .unwrap_or_else(|error| panic!("{member}: {error}"));
        let mut context = ParseContext::with_defaults(member);
        let span = SourceSpan::new(
            ContentHash::from_bytes([0; 32]),
            CRIMSON_ROF,
            Some(member),
            record.start,
            read.data.len() as u64,
            None,
        )
        .expect("a valid span");
        let document = ConfigDocument::read(&mut context, span, &read.data)
            .unwrap_or_else(|error| panic!("{member}: {error}"));
        RetailMember {
            bytes: read.data,
            document,
        }
    }

    /// What one member's survey found.
    struct MemberSurvey {
        /// The member the numbers are from.
        member: String,
        /// `.` bytes in the whole decoded member, values and the rest
        /// together: the population the "no `.` byte at all" claim is about.
        dots: usize,
        /// Field values over every entry, the population the count is a share
        /// of.
        values: usize,
        /// Field values the value reader reads as a number with a float
        /// width: the values a consumer could ask for at all.
        numeric: usize,
        /// `(line, key, index, text)` per field value containing a `.`.
        dotted: Vec<(u64, String, usize, String)>,
        /// `(line, key, index, text)` per field value carrying an `e` or an
        /// `E` immediately next to a digit: the *spelling* of an exponent,
        /// counted whether or not the value is a number.
        exponent_shaped: Vec<(u64, String, usize, String)>,
        /// `(line, index)` per field value that both contains a `.` and reads
        /// as a number — the values that would be a fraction.
        dotted_numeric: Vec<(u64, usize)>,
        /// `(line, index)` per field value the float width converts to
        /// something other than a whole number: a fraction in the only sense
        /// this task has, measured where it can actually be observed.
        fractional: Vec<(u64, usize)>,
    }

    /// The colour `LAYOUT.CSV` lines 1156-1158 misspell, matched in full and
    /// never widened: a letter `o` where the `0` of `0xff1E283C` belongs. A
    /// predicate as loose as "contains an `o`" would also swallow every
    /// resource-id name with a lowercase letter in it
    /// (`SB_24_01_crawnote2`), which is a name and not a misspelling.
    const MISSPELT_COLOUR: &str = "oxff1E283C";

    /// Task #369, the measured half: **is a configuration value ever
    /// fractional in the installed data?**
    ///
    /// Both members the keyed-list dialect covers are read through the
    /// production readers — the ROF tree walk, the bounded member decoder, the
    /// keyed-list grammar and this module's own `ConfigDocument` — and every
    /// field of every entry is put to [`TuningSchema`], so the counts are
    /// measured the way a consumer would meet them, not by grepping bytes.
    ///
    /// The answer is no, and the test says so exactly. `SCRAPBOOK.CSV` holds
    /// no `.` at all. `LAYOUT.CSV` holds 229 dotted field values and every one
    /// of them is a file name, so none of them is a number and none of them
    /// needs the fractional spelling. The counts and the extension shapes are
    /// the ones recorded in
    /// `docs/findings/2026-09-29-f12-h-fractional-configuration-values.md`.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f12_h_retail_configuration_values_are_never_fractional() {
        let rof = retail_crimson_rof();
        let float = FieldSpec::float(ValueWidth::Float64);
        let mut surveys = Vec::new();

        for member in ["ASSETS/LAYOUT.CSV", "ASSETS/SCRAPBOOK.CSV"] {
            let RetailMember { bytes, document } = retail_keyed_list(&rof, member);
            let mut survey = MemberSurvey {
                member: member.to_owned(),
                dots: bytes.iter().filter(|byte| **byte == b'.').count(),
                values: 0,
                numeric: 0,
                dotted: Vec::new(),
                exponent_shaped: Vec::new(),
                dotted_numeric: Vec::new(),
                fractional: Vec::new(),
            };
            for entry in document.entries() {
                let RawValue::Fields(fields) = &entry.value else {
                    continue;
                };
                for (index, field) in fields.iter().enumerate() {
                    survey.values += 1;
                    let text = field.value();
                    let row = (
                        entry.line,
                        String::from_utf8_lossy(&entry.key).into_owned(),
                        index,
                        String::from_utf8_lossy(text).into_owned(),
                    );
                    // A float width accepts any finite value the rule can
                    // read, so this is the whole of "could a consumer convert
                    // this field at all".
                    let as_float = TuningSchema::new(float, entry).tune(index);
                    if as_float.is_ok() {
                        survey.numeric += 1;
                    }
                    if text.contains(&b'.') {
                        survey.dotted.push(row.clone());
                        if as_float.is_ok() {
                            survey.dotted_numeric.push((entry.line, index));
                        }
                    }
                    // The task asked about an exponent too. It is a spelling
                    // question, not a value question: a `0xff8e8e8e` colour and
                    // a name like `SB_24_01_crawnote2` both put a letter next
                    // to a digit without being an exponent, so the count is
                    // kept and every hit is named, never assumed away.
                    let mark = |byte: u8| byte == b'e' || byte == b'E';
                    if text.windows(2).any(|pair| {
                        (pair[0].is_ascii_digit() && mark(pair[1]))
                            || (mark(pair[0]) && pair[1].is_ascii_digit())
                    }) {
                        survey.exponent_shaped.push(row);
                    }
                    // A conversion that is not a whole number is a fraction in
                    // the only sense this task has, so it is measured through
                    // the float width. It is deliberately *not* asked of a
                    // whole-number schema as well: `tune` refuses a fractional
                    // value as an overflow of the declared type rather than
                    // rounding it, so that reading could never report one and
                    // the second schema would only restate the first.
                    if let Ok(tuning) = TuningSchema::new(float, entry).tune(index)
                        && tuning.value.fract() != 0.0
                    {
                        survey.fractional.push((entry.line, index));
                    }
                }
            }
            println!(
                "{member}: {} field values, {} numeric, {} with a `.`, {} \
                 exponent-shaped",
                survey.values,
                survey.numeric,
                survey.dotted.len(),
                survey.exponent_shaped.len()
            );
            surveys.push(survey);
        }

        let layout = &surveys[0];
        let scrapbook = &surveys[1];
        assert_eq!(layout.member, "ASSETS/LAYOUT.CSV");
        assert_eq!(scrapbook.member, "ASSETS/SCRAPBOOK.CSV");

        // Per-member counts, as the finding records them.
        assert_eq!((scrapbook.values, scrapbook.numeric), (7376, 5098));
        assert_eq!((layout.values, layout.numeric), (7272, 3664));
        // `SCRAPBOOK.CSV` holds no `.` byte at all — the whole member, not
        // only its field values — so no value of it is fractional in any
        // sense.
        assert_eq!(scrapbook.dots, 0);
        assert!(scrapbook.dotted.is_empty());
        assert!(scrapbook.dotted_numeric.is_empty());
        assert!(
            scrapbook.fractional.is_empty(),
            "a value SCRAPBOOK.CSV converts to a fraction: {:?}",
            scrapbook.fractional
        );
        // `LAYOUT.CSV` does hold `.` bytes — 229 values, 232 bytes — and none
        // of them is a number. This is the difference between "no fractional
        // *value*" and "no `.` *byte*", and the F12-B survey only said the
        // first. The 3 bytes the 229 values do not account for are the
        // ellipsis of the one line the dialect does not classify.
        assert_eq!(layout.dots, 232);
        assert_eq!(layout.dotted.len(), 229);
        assert!(
            layout.dotted_numeric.is_empty(),
            "a dotted value read as a number: {:?}",
            layout.dotted_numeric
        );
        assert!(
            layout.fractional.is_empty(),
            "a value LAYOUT.CSV converts to a fraction: {:?}",
            layout.fractional
        );
        // The shape of every one of them: exactly one `.`, an alphabetic
        // extension after it, and a stem that is not a number. A stem of
        // digits would be the fraction this task was looking for.
        let mut extensions: BTreeMap<String, usize> = BTreeMap::new();
        for (line, key, index, text) in &layout.dotted {
            let (stem, extension) = text.rsplit_once('.').expect("a dotted value");
            assert_eq!(
                text.matches('.').count(),
                1,
                "line {line} {key}[{index}] = {text:?} carries a second `.`"
            );
            assert!(
                extension.bytes().all(|byte| byte.is_ascii_alphabetic()),
                "line {line} {key}[{index}] = {text:?} is not `<stem>.<letters>`"
            );
            assert!(
                !stem.bytes().all(|byte| byte.is_ascii_digit()),
                "line {line} {key}[{index}] = {text:?} is a fraction"
            );
            *extensions.entry(extension.to_owned()).or_default() += 1;
        }
        // The extensions observed, case included: the members write them both
        // ways, which is one more reason a `.` here is part of a name and not
        // a decimal point.
        assert_eq!(
            extensions,
            BTreeMap::from([
                ("MPG".to_owned(), 6),
                ("Png".to_owned(), 22),
                ("jpg".to_owned(), 13),
                ("png".to_owned(), 186),
                ("tga".to_owned(), 2),
            ])
        );
        // The exponent question, answered as a spelling question. Neither
        // member writes one: every value with a letter next to a digit is a
        // hexadecimal colour, a resource-id name or a `<NAME>` placeholder
        // (F12-E), and none of those reads as a number, so the exponent
        // spelling is absent rather than merely unobserved. The rule refuses an
        // exponent as text in any case, so this is a statement about the data,
        // not a safety net.
        for survey in &surveys {
            for (line, key, index, text) in &survey.exponent_shaped {
                let hexadecimal = text.starts_with("0x") || text.starts_with("0X");
                let placeholder = text.starts_with('<') && text.ends_with('>');
                // A resource-id name, case and digits and underscores
                // included: `SB_24_01_crawnote2` is one, and a predicate
                // demanding upper case would hand it to the misspelling
                // branch below instead, which is how a real name and a real
                // data slip stop being told apart.
                let name = !text.is_empty()
                    && text
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
                // The misspelling is excused by its exact spelling and by
                // nothing else, so this assertion still says "nothing here is
                // unaccounted for".
                assert!(
                    hexadecimal || placeholder || name || text == MISSPELT_COLOUR,
                    "line {line} {key}[{index}] = {text:?} is none of a hexadecimal \
                     value, a `<NAME>` placeholder, a resource-id name or the \
                     {MISSPELT_COLOUR:?} misspelling named below"
                );
            }
        }
        assert_eq!(layout.exponent_shaped.len(), 115);
        assert_eq!(scrapbook.exponent_shaped.len(), 4);
        // Three of them are none of those: lines 1156-1158 of `LAYOUT.CSV`
        // spell their colour `oxff1E283C`, a letter `o` where a zero belongs,
        // so the value is not a hexadecimal literal as written and the rule
        // refuses it as text. Every other colour on those three entries'
        // neighbours is a well-formed `0x…`, so this is a repeated slip in the
        // original data, not a spelling the data uses. The original's own
        // reader evidently tolerates it (these are live `SBZ_T_*` text
        // colours), but nothing in the workspace says how: it is recorded, not
        // guessed at and not "fixed" here, because the bytes are the
        // original's and a consumer that needs the colour declares what it
        // accepts. Only `LAYOUT.CSV` carries one: the four
        // exponent-shaped values of `SCRAPBOOK.CSV` are resource-id names,
        // and the pin below is what proves it.
        let misspellings: Vec<&(u64, String, usize, String)> = layout
            .exponent_shaped
            .iter()
            .filter(|(_, _, _, text)| *text == MISSPELT_COLOUR)
            .collect();
        assert_eq!(
            misspellings
                .iter()
                .map(|(line, key, index, text)| (*line, key.as_str(), *index, text.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (1156, "SBZ_T_TITLEJ", 7, MISSPELT_COLOUR),
                (1157, "SBZ_T_CAPTIONJ", 7, MISSPELT_COLOUR),
                (1158, "SBZ_T_TEXTJ", 7, MISSPELT_COLOUR),
            ]
        );
        assert!(
            scrapbook
                .exponent_shaped
                .iter()
                .all(|(_, _, _, text)| *text != MISSPELT_COLOUR),
            "SCRAPBOOK.CSV carries the misspelling too: {:?}",
            scrapbook.exponent_shaped
        );
    }

    /// Authored from the observed *shapes*: a twenty-field `B` button record
    /// — the widest the retail member has — carrying a number at the one
    /// position whose kind the documented list and the data do not establish,
    /// a `<NAME>` placeholder, and four empty fields between the coordinates.
    /// No original byte or value is reproduced.
    const BUTTON_RECORD: &[u8] = b"BTN_OK=B,art.png,1,2,3,0,IDS_LABEL,4,5,,,,,6,1,\
0xff000000,0xff0000ff,0x00ff00ff,0x0000ffff,<GROUP>\r\n";

    /// An entry whose value never split, shaped like the observed dialect.
    const BROKEN_BUTTON: &[u8] = b"BTN_BAD=B,\"open\r\n";

    fn records(member: &[u8]) -> ConfigDocument {
        let mut context = ParseContext::with_defaults("fixture");
        ConfigDocument::read(&mut context, source(LAYOUT, member.len()), member)
            .expect("an observed keyed list member reads")
    }

    /// Every declared field spec is traceable: a named position carries a name
    /// from that kind's own documented list, evidence of a measured kind (or
    /// an inferred one) is paired with that name, a kind nothing establishes
    /// is the weakest evidence, and a measured kind with no documented name
    /// records the missing name as `None`. Positions are contiguous, so every
    /// observed field has a spec, and a documented list the data matches
    /// position for position is transcribed into the schema in that order.
    #[test]
    fn accept_f12_i_record_schemas_are_traceable_to_documented_lists() {
        for kind in RecordKind::ALL {
            let schema = layout_schema(kind);
            assert!(!schema.is_empty(), "{}", kind.documented_name());
            for (index, spec) in schema.iter().enumerate() {
                assert_eq!(spec.position, index, "{}", kind.documented_name());
                if let Some(name) = spec.name {
                    assert!(
                        documented_fields(kind)
                            .iter()
                            .any(|field| field.name == name),
                        "{} names `{name}`, which its documented list does not",
                        kind.documented_name()
                    );
                }
                match spec.evidence {
                    // A name and a kind both established, or the name inferred
                    // once the data's omissions are removed: both are known.
                    ClaimStatus::Documented | ClaimStatus::Inferred => {
                        assert!(spec.kind.is_known(), "{}", kind.documented_name());
                        assert!(spec.name.is_some(), "{}", kind.documented_name());
                    }
                    // Only the kind was measured; the name is recorded as
                    // missing rather than invented.
                    ClaimStatus::ObservedTool => {
                        assert!(spec.kind.is_known(), "{}", kind.documented_name());
                        assert_eq!(spec.name, None, "{}", kind.documented_name());
                    }
                    // Nothing establishes the kind: the recorded unknown that
                    // the accounting retains and counts.
                    ClaimStatus::Unknown => {
                        assert_eq!(spec.kind, FieldKind::Unknown, "{}", kind.documented_name());
                    }
                    other => panic!("{}: unexpected evidence {other:?}", kind.documented_name()),
                }
                // A kind nothing establishes is exactly the weakest evidence.
                if spec.kind == FieldKind::Unknown {
                    assert_eq!(
                        spec.evidence,
                        ClaimStatus::Unknown,
                        "{}",
                        kind.documented_name()
                    );
                }
            }
            // A name is never reused inside one kind's schema.
            let mut names: Vec<&str> = schema.iter().filter_map(|spec| spec.name).collect();
            let before = names.len();
            names.sort_unstable();
            names.dedup();
            assert_eq!(
                names.len(),
                before,
                "{} reclaims a name",
                kind.documented_name()
            );
        }

        // The scrapbook schema is the documented Mission_Spread_Item list:
        // sixteen contiguous positions, every one carrying the name the
        // member's own comment documents. Thirteen positions also carry a
        // kind the shipped data measures; the three whose values no
        // `FieldKind` covers keep their documented names and record the
        // missing kind as the weakest evidence.
        let scrapbook = scrapbook_schema();
        assert_eq!(scrapbook.len(), 16);
        assert_eq!(RecordSchema::Scrapbook.fields(), scrapbook);
        assert_eq!(
            RecordSchema::Scrapbook.documented(),
            documented_scrapbook_fields()
        );
        for (index, spec) in scrapbook.iter().enumerate() {
            assert_eq!(spec.position, index);
            assert_eq!(spec.name, Some(documented_scrapbook_fields()[index].name));
            if spec.kind.is_known() {
                assert_eq!(spec.evidence, ClaimStatus::Documented);
            } else {
                assert_eq!(spec.evidence, ClaimStatus::Unknown);
            }
        }
        // ImageType's three two-letter codes, the quoted four-number
        // rectangle and Zoom's single-letter codes: documented names, no
        // established kind — three recorded unknowns out of sixteen.
        assert_eq!(
            scrapbook
                .iter()
                .filter(|spec| spec.kind == FieldKind::Unknown)
                .map(|spec| spec.position)
                .collect::<Vec<_>>(),
            vec![3, 10, 11],
            "the positions no `FieldKind` covers"
        );

        // The two kinds whose documented list and data agree position for
        // position are transcribed whole: name and kind, in order.
        for kind in [RecordKind::Text, RecordKind::Slider] {
            let schema = layout_schema(kind);
            let documented = documented_fields(kind);
            assert_eq!(schema.len(), documented.len(), "{}", kind.documented_name());
            for (index, spec) in schema.iter().enumerate() {
                assert_eq!(spec.name, Some(documented[index].name));
                assert_eq!(spec.evidence, ClaimStatus::Documented);
                assert!(spec.kind.is_known());
            }
        }

        // The sound object was never observed: its documented names stand,
        // but no field of it but the record letter claims a kind.
        let sound = layout_schema(RecordKind::SoundObject);
        assert_eq!(
            sound
                .iter()
                .filter_map(|spec| spec.name)
                .collect::<Vec<_>>(),
            documented_fields(RecordKind::SoundObject)
                .iter()
                .map(|field| field.name)
                .collect::<Vec<_>>()
        );
        assert!(
            sound
                .iter()
                .skip(1)
                .all(|spec| spec.kind == FieldKind::Unknown)
        );
    }

    /// Non-negotiable #5 applied to record fields: a field whose kind the
    /// documented list and the data do not establish stays in the document
    /// and is counted as unknown even when its bytes look like a number; a
    /// value that never split is one unknown rather than a silent success.
    /// The accounting is reached through the production
    /// `RecordView::for_entry`.
    #[test]
    fn accept_f12_i_unknown_kinds_stay_raw_and_are_counted() {
        let document = records(BUTTON_RECORD);
        let entry = entry_of(&document, b"BTN_OK");
        let view = RecordView::for_entry(entry).expect("a B record has a schema");
        assert_eq!(view.schema(), RecordSchema::Layout(RecordKind::Button));
        assert!(!view.unsplit());
        assert_eq!(view.fields().len(), 20);
        assert_eq!(
            view.accounting(),
            RecordAccounting {
                schema: "Button",
                fields: 20,
                typed: 14,
                unknown: 6,
                placeholders: 1,
                empty: 4,
                unsplit: false,
            }
        );
        // The unresolved position holds a number and is neither converted nor
        // dropped: it is still the member's own bytes, and the spelling says
        // `Integer` while the kind stays `Unknown`.
        let unresolved = view.fields()[5];
        assert_eq!(unresolved.name(), None);
        assert_eq!(unresolved.kind(), FieldKind::Unknown);
        assert_eq!(unresolved.spelling, FieldSpelling::Integer);
        assert_eq!(unresolved.field.value(), b"0");
        // A placeholder is a spelling compatible with any kind, so its
        // position is an unknown too — counted once and kept.
        assert_eq!(view.fields()[19].spelling, FieldSpelling::Placeholder);
        assert_eq!(view.fields()[19].kind(), FieldKind::Unknown);
        // Nothing was converted: every byte survives and the entry is still
        // the document's own.
        assert_eq!(document.reassemble(), BUTTON_RECORD);
        assert_eq!(document.accounting().entries, 1);
        assert_eq!(document.accounting().unconsumed, 1);

        // A value that never split has no field to classify: `for_entry` finds
        // no schema for it, and reading it directly against a schema reports
        // the whole value as one unknown, never as success.
        let broken = records(BROKEN_BUTTON);
        let entry = entry_of(&broken, b"BTN_BAD");
        assert!(RecordView::for_entry(entry).is_none());
        let view = RecordView::new(RecordSchema::Layout(RecordKind::Button), entry);
        assert!(view.unsplit());
        assert!(view.fields().is_empty());
        assert_eq!(
            view.accounting(),
            RecordAccounting {
                schema: "Button",
                fields: 0,
                typed: 0,
                unknown: 1,
                placeholders: 0,
                empty: 0,
                unsplit: true,
            }
        );
        assert_eq!(broken.reassemble(), BROKEN_BUTTON);
    }

    /// The documented record kinds are recognized through the production
    /// entry point: a record letter selects its kind, a `V`/`G` definition is
    /// excluded first — its value may itself begin with a record letter — and
    /// a `SCRAPBOOK.CSV` item is the sixteen-field numeric-first shape. A
    /// shape no documented record has is left without a schema.
    #[test]
    fn accept_f12_i_for_entry_selects_documented_kinds_and_excludes_definitions() {
        /// Authored: two definitions, one whose value spells a record letter;
        /// a normal record; a lower-case letter no kind owns; and a
        /// sixteen-field numeric-first scrapbook item.
        const SHAPES: &[u8] = b"V9=B,1\r\n\
G2=T,2\r\n\
BTN_OK=B,art.png,1,2,3,0,IDS_LABEL,4,5,,,,,6,1,0xff000000,0xff0000ff,0x00ff00ff,0x0000ffff,<GROUP>\r\n\
LOWER=b,1\r\n\
SBROW=1,IDS_IMG,thumb.png,PNG,10,20,255,64,64,0,\"0,0,64,64\",1,1,1,TITLE,TEXT\r\n";
        let document = records(SHAPES);

        // The definition rule wins: a definition whose value spells a record
        // letter is not a record, which is the one distinction the two
        // candidate rules cannot disagree about here.
        assert_eq!(RecordSchema::for_entry(entry_of(&document, b"V9")), None);
        assert_eq!(RecordSchema::for_entry(entry_of(&document, b"G2")), None);
        assert!(RecordView::for_entry(entry_of(&document, b"V9")).is_none());
        assert_eq!(
            RecordSchema::for_entry(entry_of(&document, b"BTN_OK")),
            Some(RecordSchema::Layout(RecordKind::Button))
        );
        assert_eq!(
            RecordView::for_entry(entry_of(&document, b"BTN_OK"))
                .expect("a record")
                .schema(),
            RecordSchema::Layout(RecordKind::Button)
        );
        // A lower-case letter is not a documented record letter.
        assert_eq!(RecordSchema::for_entry(entry_of(&document, b"LOWER")), None);
        // Sixteen fields with a numeric first field is the scrapbook shape.
        assert_eq!(
            RecordSchema::for_entry(entry_of(&document, b"SBROW")),
            Some(RecordSchema::Scrapbook)
        );
        assert_eq!(RecordSchema::Scrapbook.label(), "Mission_Spread_Item");
        assert_eq!(RecordSchema::Layout(RecordKind::Button).label(), "Button");

        // The schema a consumer gets names the documented list it cites.
        assert_eq!(
            RecordSchema::Layout(RecordKind::EditBox).documented(),
            documented_fields(RecordKind::EditBox)
        );
    }

    // ------------------------------------------------- F12-I retail reads

    /// The original installation's `crimson.rof`, or a loud panic naming
    /// `CS_GAME_DIR`.
    fn retail_rof() -> Vec<u8> {
        let dir = std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR is not set: this test needs the original installation");
        let path = std::path::PathBuf::from(dir).join(cs_formats::text::dialect::CRIMSON_ROF);
        std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
    }

    /// Reads one member of the retail archive through the production
    /// directory and bounded-member readers; `path` is relative to the
    /// archive root, e.g. `ASSETS/LAYOUT.CSV`.
    fn retail_member(rof: &[u8], path: &str) -> Vec<u8> {
        let mut offset = 0u32;
        let segments: Vec<&str> = path.split('/').collect();
        for (depth, segment) in segments.iter().enumerate() {
            let mut context = ParseContext::with_defaults(cs_formats::text::dialect::CRIMSON_ROF);
            let block = cs_formats::read_directory(&mut context, &rof[offset as usize..])
                .expect("a retail directory block reads");
            let (_, record) = block
                .entries()
                .map(|entry| {
                    let name = entry.name.strip_suffix(&[0]).unwrap_or(entry.name);
                    (String::from_utf8_lossy(name).into_owned(), entry.record)
                })
                .find(|(name, _)| name.eq_ignore_ascii_case(segment))
                .unwrap_or_else(|| panic!("{path}: no `{segment}` in the retail archive"));
            if depth + 1 < segments.len() {
                assert!(
                    record.flags.is_directory(),
                    "{path}: {segment} is a directory"
                );
                offset = record.start;
                continue;
            }
            let member = cs_formats::RofMember {
                path: Vec::new(),
                record,
                start: u64::from(record.start),
                stored_end: u64::from(record.start) + u64::from(record.raw_length_on_disk),
            };
            let context = ParseContext::with_defaults(cs_formats::text::dialect::CRIMSON_ROF);
            return cs_formats::read_member(
                &context,
                rof,
                &member,
                &cs_formats::RofLimits::default(),
            )
            .expect("a retail member reads")
            .data;
        }
        unreachable!("paths have at least one segment")
    }

    fn read_member_document(member: &str, bytes: &[u8]) -> ConfigDocument {
        let mut context = ParseContext::with_defaults(member);
        ConfigDocument::read(&mut context, source(member, bytes.len()), bytes)
            .expect("an observed keyed list member reads")
    }

    /// Retail: the shipped `ASSETS/LAYOUT.CSV` has exactly the 636 object
    /// records the survey measured — one per record letter but the absent
    /// sound object — every observed field position covered by its schema,
    /// and the fully-documented kinds type every field while the button's
    /// underdetermined positions are retained and counted. The shipped
    /// `ASSETS/SCRAPBOOK.CSV` has 461 sixteen-field items, of whose
    /// positions thirteen are typed and three are recorded unknowns. Every
    /// declared kind is also what the member's own bytes spell, bar the
    /// four colour fields the original misspells.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f12_i_retail_record_kinds_are_typed_and_unknowns_are_counted() {
        use std::collections::HashMap;

        let rof = retail_rof();

        let layout_bytes = retail_member(&rof, "ASSETS/LAYOUT.CSV");
        let layout = read_member_document("ASSETS/LAYOUT.CSV", &layout_bytes);
        assert_eq!(layout.accounting().entries, 822);
        let mut per_kind: HashMap<RecordKind, usize> = HashMap::new();
        let mut button_shapes: HashMap<usize, usize> = HashMap::new();
        let mut unknown_total: HashMap<RecordKind, usize> = HashMap::new();
        let mut unspellable: HashMap<(&str, usize), usize> = HashMap::new();
        let mut classified = 0usize;
        for entry in layout.entries() {
            let Some(view) = RecordView::for_entry(entry) else {
                continue;
            };
            let RecordSchema::Layout(kind) = view.schema() else {
                panic!("the layout member holds layout records");
            };
            classified += 1;
            *per_kind.entry(kind).or_default() += 1;
            if kind == RecordKind::Button {
                *button_shapes.entry(view.fields().len()).or_default() += 1;
            }
            let accounting = view.accounting();
            assert_eq!(accounting.typed + accounting.unknown, accounting.fields);
            assert!(
                view.fields().len() <= view.schema().fields().len(),
                "{}: an observed position has no spec",
                kind.documented_name()
            );
            *unknown_total.entry(kind).or_default() += accounting.unknown;
            for field in view.fields() {
                if !spells_kind(field.kind(), field.spelling) {
                    *unspellable
                        .entry((view.schema().label(), field.position))
                        .or_default() += 1;
                }
            }
        }
        assert_eq!(classified, 636, "the shipped member's object records");
        assert_eq!(per_kind.len(), 10, "the sound object is absent");
        assert_eq!(per_kind.get(&RecordKind::Button), Some(&119));
        assert_eq!(per_kind.get(&RecordKind::Pane), Some(&106));
        assert_eq!(per_kind.get(&RecordKind::Text), Some(&297));
        assert_eq!(per_kind.get(&RecordKind::EditBox), Some(&4));
        assert_eq!(per_kind.get(&RecordKind::Movie), Some(&6));
        assert_eq!(per_kind.get(&RecordKind::TextList), Some(&16));
        assert_eq!(per_kind.get(&RecordKind::ScrollingText), Some(&8));
        assert_eq!(per_kind.get(&RecordKind::Dropdown), Some(&70));
        assert_eq!(per_kind.get(&RecordKind::Listbox), Some(&6));
        assert_eq!(per_kind.get(&RecordKind::Slider), Some(&4));
        assert_eq!(
            button_shapes,
            HashMap::from([(15, 65), (16, 9), (19, 44), (20, 1)]),
            "the button's four observed shapes"
        );
        // A kind whose documented list and data agree position for position
        // types every field...
        assert_eq!(unknown_total.get(&RecordKind::Text), Some(&0));
        assert_eq!(unknown_total.get(&RecordKind::Slider), Some(&0));
        assert_eq!(unknown_total.get(&RecordKind::Pane), Some(&0));
        assert_eq!(unknown_total.get(&RecordKind::EditBox), Some(&0));
        assert_eq!(unknown_total.get(&RecordKind::Movie), Some(&0));
        assert_eq!(unknown_total.get(&RecordKind::TextList), Some(&0));
        assert_eq!(unknown_total.get(&RecordKind::Dropdown), Some(&0));
        assert_eq!(unknown_total.get(&RecordKind::Listbox), Some(&0));
        // The scrolling text's one empty position carries no measured kind:
        // eight records, one unknown each.
        assert_eq!(unknown_total.get(&RecordKind::ScrollingText), Some(&8));
        // ...while the button's underdetermined positions are retained and
        // counted rather than guessed away: five in a fifteen-, sixteen- or
        // nineteen-field record and six in the twenty-field one.
        assert_eq!(unknown_total.get(&RecordKind::Button), Some(&(5 * 118 + 6)));

        let scrapbook_bytes = retail_member(&rof, "ASSETS/SCRAPBOOK.CSV");
        let scrapbook = read_member_document("ASSETS/SCRAPBOOK.CSV", &scrapbook_bytes);
        let mut items = 0usize;
        for entry in scrapbook.entries() {
            let view = RecordView::for_entry(entry).expect("a scrapbook item");
            assert_eq!(view.schema(), RecordSchema::Scrapbook);
            assert_eq!(view.fields().len(), 16);
            let accounting = view.accounting();
            assert_eq!(accounting.fields, 16);
            // Thirteen positions type; `ImageType`, the quoted rectangle and
            // `Zoom` keep their documented names and stay recorded unknowns.
            assert_eq!(accounting.typed, 13, "the kind the data measures");
            assert_eq!(accounting.unknown, 3, "the kind nothing establishes");
            for field in view.fields() {
                if !spells_kind(field.kind(), field.spelling) {
                    *unspellable
                        .entry((view.schema().label(), field.position))
                        .or_default() += 1;
                }
            }
            items += 1;
        }
        assert_eq!(items, 461);

        // Every declared kind is a kind the member's own bytes spell, with
        // one exception the original really contains: four `T` records write
        // a colour without a valid `0x` prefix (`xff000000`, and
        // `oxff1E283C` where a zero is meant), so those four values can
        // never become colour constants while the position keeps the colour
        // kind the other 293 records measure.
        assert_eq!(
            unspellable,
            HashMap::from([(("Text", 7usize), 4usize)]),
            "a declared kind the shipped bytes do not spell"
        );
    }

    // ------------------------------------------------- #380

    /// The measured read of the misspelt colours: a failed `%x` leaves the
    /// zeroed field, which draws as RGB(15,15,15); alpha never matters.
    #[test]
    fn accept_f12_j_misspelt_colour_parses_to_zero_and_renders_near_black() {
        let member = format!(
            "TXT_J=T,!,1,2,0,3,4,{MISSPELT_COLOUR},0\r\nTXT_A=T,!,1,2,0,3,4,xff000000,0\r\n"
        );
        let document = records(member.as_bytes());
        for key in [b"TXT_J".as_slice(), b"TXT_A".as_slice()] {
            let entry = entry_of(&document, key);
            let view = RecordView::for_entry(entry).expect("a T record");
            let token = view.fields()[7].field.value();
            assert!(token == b"oxff1E283C" || token == b"xff000000");
            let colour = legacy_text_colour(token).expect("measured");
            assert_eq!(colour, 0);
            assert_eq!(legacy_text_rgb(colour), [15, 15, 15]);
        }
        // Raw bytes survive.
        assert_eq!(document.reassemble(), member.as_bytes());

        // Well-spelled values: RGB from the value, alpha ignored, 0 -> 15.
        assert_eq!(legacy_text_colour(b"0xff1E283C"), Some(0xff1e_283c));
        assert_eq!(legacy_text_rgb(0xff1e_283c), [0x1e, 0x28, 0x3c]);
        assert_eq!(legacy_text_rgb(0x001e_283c), [0x1e, 0x28, 0x3c]);
        assert_eq!(legacy_text_colour(b"0xff000000"), Some(0xff00_0000));
        assert_eq!(legacy_text_rgb(0xff00_0000), [15, 15, 15]);
        assert_eq!(legacy_text_rgb(0x0100_0001), [0, 0, 1]);
        // Empty field: not read, stays zero. Unmeasured shapes stay None.
        assert_eq!(legacy_text_colour(b""), Some(0));
        assert_eq!(legacy_text_colour(b"0x"), None);
        assert_eq!(legacy_text_colour(b"0x123456789"), None);
    }

    // ------------------------------------------------- F12-J (task #376)

    /// A colour spelled with a letter `o` where the `0` of `0x…` belongs.
    ///
    /// `ASSETS/LAYOUT.CSV` lines 1156-1158 write `oxff1E283C` at field 7
    /// of the three `SBZ_T_*J` text records of `[@ScrapbookZoom@]` (and
    /// `SBZ_T_CAPTIONA` drops the `0` outright on line 1121). What the
    /// original's own reader does with such bytes is **not measurable from
    /// the data** — the layout reader is code inside the SafeDisc-encrypted
    /// engine image, and the finding
    /// `docs/findings/2026-09-29-f12-j-letter-o-colour.md` records why the
    /// question stays `Unknown`. So this reader keeps the designed rule:
    /// the field is refused as a number, classified `Text` rather than
    /// `Color`, and retained byte for byte. No consumer silently
    /// substitutes a colour for it.
    #[test]
    fn accept_f12_j_a_misspelt_colour_is_retained_unconverted() {
        // Authored; the record shape is the observed `T` one and the value
        // is the misspelling the retail test below pins on the real member.
        let member = format!(
            "TXT_J=T,!,1,2,0,3,4,{MISSPELT_COLOUR},0\r\n\
             TXT_A=T,!,1,2,0,3,4,xff000000,0\r\n\
             TXT_OK=T,!,1,2,0,3,4,0xff1E283C,0\r\n"
        );
        let document = records(member.as_bytes());

        for key in [b"TXT_J".as_slice(), b"TXT_A".as_slice()] {
            let entry = entry_of(&document, key);
            let view = RecordView::for_entry(entry).expect("a T record has a schema");
            assert_eq!(view.schema(), RecordSchema::Layout(RecordKind::Text));
            // Position 7 keeps the documented colour kind; the bytes
            // themselves are not a colour spelling, so no conversion can
            // ever take them.
            let colour = &view.fields()[7];
            assert_eq!(colour.name(), Some("Color"));
            assert_eq!(colour.kind(), FieldKind::Color);
            assert_eq!(colour.spelling, FieldSpelling::Text);
            let len = colour.field.value().len();
            for spec in [
                FieldSpec::integer(ValueWidth::Bits32, false),
                FieldSpec::float(ValueWidth::Float64),
            ] {
                assert_eq!(
                    TuningSchema::new(spec, entry).tune(7),
                    Err(TuneError::NotANumber { len })
                );
            }
            // The record is still whole: the position's kind is measured,
            // so the field counts as typed — typed does not mean read.
            assert_eq!(view.accounting().typed, 9);
            assert_eq!(view.accounting().unknown, 0);
        }
        assert_eq!(document.reassemble(), member.as_bytes());

        // The control proves the refusal is about the bytes, not the
        // field: the same position written `0x…` spells a colour and
        // converts.
        let entry = entry_of(&document, b"TXT_OK");
        let view = RecordView::for_entry(entry).expect("a T record");
        assert_eq!(view.fields()[7].spelling, FieldSpelling::Color);
        let tuning = TuningSchema::new(FieldSpec::integer(ValueWidth::Bits32, false), entry)
            .tune(7)
            .expect("a well-formed colour reads");
        assert_eq!(tuning.unsigned, Some(0xff1e_283c));
    }

    /// The retail member carries the misspelling on live content: the three
    /// records are bound by `ASSETS/SCRIPTS/SCRAPBOOKZOOM.SCRIPT` as
    /// `sbz_t_title`/`sbz_t_caption`/`sbz_t_text` plus the slot letter, and
    /// no script writes the objects' `goscolor` — so the field is what
    /// renders, whatever the original reader made of it. This test pins the
    /// records, the bytes, the refusals and the container, and records the
    /// original's own tolerance as what the finding leaves `Unknown`.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f12_j_retail_misspelt_colour_fields_stay_raw() {
        let rof = retail_crimson_rof();
        let RetailMember {
            bytes,
            mut document,
        } = retail_keyed_list(&rof, LAYOUT);

        // The three records of `[@ScrapbookZoom@]`, found through the same
        // case-insensitive lookup the binding script relies on (R1).
        for (key, line) in [
            ("sbz_t_titlej", 1156u64),
            ("sbz_t_captionj", 1157),
            ("sbz_t_textj", 1158),
        ] {
            let Lookup::Found(entry) = document.lookup(Some(b"@ScrapbookZoom@"), key.as_bytes())
            else {
                panic!("{key}: the zoom slot's record is missing")
            };
            assert_eq!(entry.line, line);
            let view = RecordView::for_entry(entry).expect("a T record");
            assert_eq!(view.schema(), RecordSchema::Layout(RecordKind::Text));
            let colour = &view.fields()[7];
            assert_eq!(colour.name(), Some("Color"));
            assert_eq!(colour.kind(), FieldKind::Color);
            assert_eq!(colour.spelling, FieldSpelling::Text);
            assert_eq!(colour.field.value(), MISSPELT_COLOUR.as_bytes());
            assert_eq!(
                TuningSchema::new(FieldSpec::integer(ValueWidth::Bits32, false), entry).tune(7),
                Err(TuneError::NotANumber { len: 10 })
            );
        }

        // The sibling slip on the same record kind: `SBZ_T_CAPTIONA`
        // writes `xff000000`, dropping the `0` of the prefix. The original
        // reader's handling of it is the same recorded unknown.
        let Lookup::Found(caption) = document.lookup(Some(b"@ScrapbookZoom@"), b"sbz_t_captiona")
        else {
            panic!("sbz_t_captiona: the record is missing")
        };
        assert_eq!(caption.line, 1121);
        let view = RecordView::for_entry(caption).expect("a T record");
        assert_eq!(view.fields()[7].spelling, FieldSpelling::Text);
        assert_eq!(view.fields()[7].field.value(), b"xff000000");
        assert_eq!(
            TuningSchema::new(FieldSpec::integer(ValueWidth::Bits32, false), caption).tune(7),
            Err(TuneError::NotANumber { len: 9 })
        );

        // A well-formed neighbour converts: the refusal is the bytes', not
        // the field's.
        let Lookup::Found(neighbour) = document.lookup(Some(b"@ScrapbookZoom@"), b"sbz_t_titlei")
        else {
            panic!("sbz_t_titlei: the record is missing")
        };
        let view = RecordView::for_entry(neighbour).expect("a T record");
        assert_eq!(view.fields()[7].spelling, FieldSpelling::Color);
        TuningSchema::new(FieldSpec::integer(ValueWidth::Bits32, false), neighbour)
            .tune(7)
            .expect("a well-formed colour reads");

        // Every slot letter A-Z is present, so the J rows are reachable
        // content, not dead data.
        let titles = document
            .entries()
            .filter(|entry| {
                entry.section.as_deref() == Some(b"@ScrapbookZoom@".as_slice())
                    && entry.key.starts_with(b"SBZ_T_TITLE")
            })
            .count();
        assert_eq!(titles, 26, "one title record per zoom slot");

        // Nothing was repaired or normalised: the document is the member.
        assert_eq!(document.reassemble(), bytes.as_slice());

        // The script that binds the three objects is a member of the same
        // container, which is what makes the colour live UI data.
        let mut context = ParseContext::with_defaults(CRIMSON_ROF);
        let tree = cs_formats::read_tree(&mut context, &rof).expect("the retail container walks");
        let bound = tree.members().iter().any(|record| {
            record
                .path
                .iter()
                .map(|segment| String::from_utf8_lossy(segment).into_owned())
                .collect::<Vec<_>>()
                .join("/")
                .eq_ignore_ascii_case("ASSETS/SCRIPTS/SCRAPBOOKZOOM.SCRIPT")
        });
        assert!(bound, "SCRAPBOOKZOOM.SCRIPT is in the retail container");
    }
    // -------------------------------------------------- F12-D the census
    //
    // The synthetic cases below author every byte: a keyed-list member with
    // two record kinds, a `V`/`G` definition, an entry nothing declares and a
    // misspelt colour, and a PE image plus resource headers whose ids cross
    // it in both directions. No original byte is used.

    /// An authored keyed-list member: three `T` records (one with the
    /// misspelt colour F12-J measured, one with a `<NAME>` placeholder), a
    /// `P` record, a `V`/`G` definition pair the placeholder pass owns, and an
    /// entry nothing declares.
    const CENSUS: &[u8] = b"; authored\r\n\
[GLOBALVARS]\r\n\
G1=WIDE,64\r\n\
[PANEL]\r\n\
V1=BACK,0xff112233\r\n\
TITLE_A=T,IDS_TITLE,1,2,3,4,5,0xAABBCCDD,0\r\n\
TITLE_B=T,IDS_OTHER,1,2,3,4,5,oxff1E283C,0\r\n\
TITLE_C=T,IDS_THIRD,<WIDE>,3,4,5,0,0x00FF00FF,0\r\n\
PANE_1=P,back.png,-1,-2,3,4,0,1,1\r\n\
LOOSE=not,declared,anywhere\r\n";

    fn census_document() -> ConfigDocument {
        let mut context = ParseContext::with_defaults(LAYOUT);
        ConfigDocument::read(&mut context, source(LAYOUT, CENSUS.len()), CENSUS)
            .expect("the authored member reads")
    }

    /// A whole member is accounted for: every entry is attributed to the one
    /// declaration that covers it, the position census separates what a
    /// declared kind describes from what stays a recorded unknown, and the
    /// entry nothing declares is listed and — on a gameplay-critical member —
    /// blocks parity.
    #[test]
    fn accept_f12_d_accounting_member_account_attributes_every_entry_and_position() {
        let document = census_document();
        let account = document.member_account(Parity::GameplayCritical);

        // The member's own provenance comes back with the account, so a caller
        // can name where in the installation the numbers are from.
        assert_eq!(account.source(), document.source());
        assert_eq!(account.dialect(), TextDialect::KeyedList);
        assert_eq!(account.parity(), Parity::GameplayCritical);

        // Seven entries: one `G` definition, one `V` definition, three `T`
        // records, one `P` record and one entry nothing declares.
        let entries = account.entries();
        assert_eq!(entries.len(), 7);
        assert_eq!(account.keys().entries, 7);
        let declared = |code: &str| {
            entries
                .iter()
                .filter(|entry| entry.declaration.code() == code)
                .count()
        };
        assert_eq!(declared("placeholder_definition"), 2, "V1 and G1");
        assert_eq!(declared("record"), 4, "three T records and one P record");
        assert_eq!(declared("undeclared"), 1, "LOOSE names nothing");
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.declaration == EntryDeclaration::Undeclared)
                .map(|entry| entry.key.to_vec()),
            Some(b"LOOSE".to_vec()),
            "the undeclared entry is listed, not dropped"
        );
        // The placeholder pass is the one that decides what a definition is,
        // so the account cannot disagree with it about `V1`/`G1`.
        assert_eq!(
            entries[0].declaration,
            EntryDeclaration::Placeholder(PlaceholderScope::Global)
        );
        assert_eq!(
            entries[1].declaration,
            EntryDeclaration::Placeholder(PlaceholderScope::Local)
        );
        assert_eq!(account.placeholders().definitions, 2);
        assert_eq!(account.placeholders().references, 1);
        assert_eq!(account.placeholders().unresolved, 0);

        // The `Text` roll-up: the documented colour position types the two
        // well-spelled values, counts the misspelt one as *off kind* (the
        // declaration is what is wrong there, not the value) and never
        // converts any of them.
        let text = account
            .schema(RecordSchema::Layout(RecordKind::Text))
            .expect("the member holds text records");
        assert_eq!(text.records, 3);
        assert_eq!(text.fields, 27, "three nine-field records");
        assert_eq!(text.unsplit(), 0);
        assert_eq!(text.beyond_schema, 0);
        let colour = text.positions()[7];
        assert_eq!(colour.name, Some("Color"));
        assert_eq!(colour.kind, FieldKind::Color);
        assert_eq!(colour.evidence, ClaimStatus::Documented);
        assert_eq!(colour.observed, 3);
        assert_eq!(colour.described(), 2);
        assert_eq!(colour.off_kind, 1, "oxff1E283C is not a colour spelling");
        assert_eq!(colour.untyped(), 0);
        assert_eq!(text.off_kind(), 1);
        assert_eq!(text.untyped(), 0);
        // Position 2 of `TITLE_C` is `<WIDE>`, the one global definition the
        // F12-E pass resolves, so it is a placeholder rather than a value.
        assert_eq!(text.positions()[2].placeholders, 1);
        // Every position partitions its observations.
        for position in text.positions() {
            assert_eq!(
                position.observed,
                position.described()
                    + position.untyped()
                    + position.off_kind
                    + position.placeholders
                    + position.empty,
                "position {} partitions its values",
                position.position
            );
        }

        // The `Pane` roll-up carries the signed coordinates, so its position 2
        // is the schema's only signed integer.
        let pane = account
            .schema(RecordSchema::Layout(RecordKind::Pane))
            .expect("the member holds pane records");
        assert_eq!(pane.records, 1);
        assert_eq!(
            pane.positions()[2].kind,
            FieldKind::Integer { signed: true }
        );
        assert_eq!(pane.positions()[2].described(), 1);

        // A schema no record follows contributes no row at all, so the roll-up
        // never carries an empty one; and the rows follow `RecordSchema::ALL`
        // order rather than the order the entries arrived in — the first
        // record read is a `T`, and the first row is the pane's.
        assert!(
            account.schema(RecordSchema::Scrapbook).is_none(),
            "no scrapbook item in a layout member"
        );
        let labels: Vec<&str> = account.schemas().iter().map(|row| row.label).collect();
        assert_eq!(labels, vec!["Pane", "Text"]);
        assert!(account.schemas().iter().all(|row| row.records > 0));

        // The one unconsumed entry is a parity blocker on a gameplay-critical
        // member, and the count is what a caller fails on.
        assert_eq!(account.unconsumed().count(), 1);
        assert_eq!(account.blocking(), 1);
        assert!(!account.parity_holds());
    }

    /// The same member under a non-gameplay declaration still retains and
    /// counts the undeclared entry and still lists it, but does not block: the
    /// two halves of spec F12 non-negotiable #5 are separate decisions.
    #[test]
    fn accept_f12_d_accounting_a_non_gameplay_member_counts_without_blocking() {
        let document = census_document();
        let account = document.member_account(Parity::NonGameplay);
        assert_eq!(account.parity(), Parity::NonGameplay);
        assert!(!account.parity().blocks());
        // Retained, not discarded: the entry is still in the account and still
        // enumerable.
        assert_eq!(account.unconsumed().count(), 1);
        assert_eq!(account.blocking(), 0);
        assert!(account.parity_holds());
        // The position census is the member's, not the declaration's: the
        // misspelt colour is still counted wherever the member lives.
        assert_eq!(
            account
                .schema(RecordSchema::Layout(RecordKind::Text))
                .map(SchemaAccount::off_kind),
            Some(1)
        );
    }

    /// Every value at a position whose kind nothing establishes is counted as
    /// a recorded unknown, apart from the empty ones, and the count is
    /// per value rather than per record.
    #[test]
    fn accept_f12_d_accounting_a_recorded_unknown_position_is_counted_per_value() {
        // One `B` record: the letter, an art path, three coordinates, one
        // value at the position whose kind nothing establishes, a name the
        // schema measures without documenting, the two slider counts it also
        // measures, the four positions the shipped data leaves empty, a flag
        // and the four documented colours.
        const BUTTON: &[u8] = b"; authored\r\n\
[PANEL]\r\n\
B1=B,back.png,1,2,3,0,SB_1,7,8,,,,,10,1,0x11111111,0x22222222,0x33333333,0x44444444,\r\n";
        let mut context = ParseContext::with_defaults(LAYOUT);
        let document = ConfigDocument::read(&mut context, source(LAYOUT, BUTTON.len()), BUTTON)
            .expect("the authored button reads");
        let account = document.member_account(Parity::GameplayCritical);
        let button = account
            .schema(RecordSchema::Layout(RecordKind::Button))
            .expect("the member holds a button");
        assert_eq!(button.records, 1);
        assert_eq!(button.positions().len(), 20, "the whole declared slice");
        // Position 5 is the one whose kind nothing establishes and the record
        // spells `0` there: one recorded unknown, retained and never converted.
        let unresolved = button.positions()[5];
        assert_eq!(unresolved.kind, FieldKind::Unknown);
        assert_eq!(unresolved.evidence, ClaimStatus::Unknown);
        assert_eq!(unresolved.observed, 1);
        assert_eq!(unresolved.untyped(), 1);
        assert_eq!(unresolved.described(), 0);
        assert_eq!(unresolved.off_kind, 0, "an unknown kind claims nothing");
        assert_eq!(button.off_kind(), 0, "every value spells its declared kind");
        assert_eq!(button.untyped(), 1);
        // Position 6 is a name the schema measures without documenting, so it
        // is typed, and the `unnamed` roll-up counts it separately.
        assert_eq!(button.positions()[6].kind, FieldKind::Name);
        assert_eq!(button.positions()[6].described(), 1);
        assert_eq!(button.positions()[6].name, None);
        assert!(button.unnamed() > 0, "the button's unnamed positions exist");
        assert_eq!(button.positions()[0].name, Some("ID"));
    }

    /// The string-id account crosses the ids a header names with the blocks an
    /// image actually has, in **both** directions, and counts a define whose
    /// value is not a plain decimal instead of letting it coincide with one.
    #[test]
    fn accept_f12_d_accounting_string_ids_cross_names_and_blocks_both_ways() {
        // One image with blocks 1, 3 and 5: block 2 is missing, so the ids of
        // blocks 2 and 4 are named with no block behind them, and block 5
        // carries ids nobody names.
        let blocks: Vec<Block> = [1u32, 3, 5]
            .into_iter()
            .map(|block_id| Block {
                block_id,
                language: 1033,
                code_page: 1252,
                entries: &[],
            })
            .collect();
        let image = fixture_image(&rsrc_section(&blocks, false));
        let mut context = ParseContext::with_defaults("strings.dll");
        let catalog = StringCatalog::read(&mut context, image_source(image.len()), &image)
            .expect("the authored image reads");
        assert_eq!(
            catalog.accounting().strings,
            48,
            "three counted blocks of sixteen units"
        );

        // Ids 0, 16 and 32 are the first unit of blocks 1, 2 and 3 under
        // `(block - 1) * 16 + index`; 48 and 64 are blocks 4 and 5.
        const HEADER: &[u8] = b"//{{NO_DEPENDENCIES}}\r\n\
#define IDS_ALPHA 0\r\n\
#define IDS_BETA 16\r\n\
#define IDS_GAMMA 32\r\n\
#define STR_DELTA 48\r\n\
#define SB_ALPHA 0\r\n\
#define FONT_MAIN 900\r\n\
#define IDS_BROKEN 0x20\r\n";
        let mut header_context = ParseContext::with_defaults("ASSETS/SCRIPTS/RESOURCE.H");
        let header =
            read_resource_header(&mut header_context, HEADER).expect("the authored header reads");

        let account = account_string_ids(&catalog, &[("ASSETS/SCRIPTS/RESOURCE.H", &header)]);
        assert_eq!(account.blocks, 3, "blocks 1, 3 and 5");
        assert_eq!(account.units, 48);
        let row = account
            .header("assets/scripts/resource.h")
            .expect("a row is found whatever the spelling's case");
        assert_eq!(row.member, "ASSETS/SCRIPTS/RESOURCE.H");
        assert_eq!(row.defines, 7);
        // The `0x20` value is not a plain decimal, so it is counted and kept
        // out of both id sets rather than silently becoming id 32 a second
        // time.
        assert_eq!(row.undecoded, 1);
        assert_eq!(row.distinct, vec![0, 16, 32, 48, 900]);
        // Blocks 1 and 3 are present; ids 16 (block 2), 48 (block 4) and 900
        // (block 57) name blocks the image lacks.
        assert_eq!(row.present, 2);
        assert_eq!(row.absent, vec![16, 48, 900]);
        // The string-name scope: `IDS_`/`STR_`/`SB_` only, so the six of the
        // seven defines that carry one are in it and `FONT_MAIN` is out.
        assert_eq!(row.string_defines, 6);
        assert_eq!(row.string_distinct, vec![0, 16, 32, 48]);
        assert_eq!(row.string_present, 2);
        assert_eq!(row.string_absent, vec![16, 48]);
        // The image side: blocks 1 and 3 are named, block 5 is not.
        assert_eq!(account.unnamed_blocks, vec![5]);
        assert_eq!(account.named_ids, 5);
        assert_eq!(account.absent_ids(), vec![16, 48, 900]);
        assert_eq!(
            account
                .header("ASSETS/SCRIPTS/RESOURCE.H")
                .map(|row| row.absent.clone()),
            Some(vec![16, 48, 900])
        );

        // A second header naming only id 0: blocks 3 and 5 are then unnamed
        // and the account says so rather than passing.
        const SECOND: &[u8] = b"#define IDS_ONLY 0\r\n";
        let mut second_context = ParseContext::with_defaults("ASSETS/SCRIPTS/RESRC1.H");
        let second =
            read_resource_header(&mut second_context, SECOND).expect("the authored header reads");
        let account = account_string_ids(&catalog, &[("ASSETS/SCRIPTS/RESRC1.H", &second)]);
        assert_eq!(account.unnamed_blocks, vec![3, 5]);
        assert_eq!(
            account
                .header("ASSETS/SCRIPTS/RESRC1.H")
                .map(|row| row.present),
            Some(1)
        );
        // Both headers at once: the account is over the union of what they
        // name, and every header keeps its own row.
        let both = account_string_ids(
            &catalog,
            &[
                ("ASSETS/SCRIPTS/RESOURCE.H", &header),
                ("ASSETS/SCRIPTS/RESRC1.H", &second),
            ],
        );
        assert_eq!(both.unnamed_blocks, vec![5]);
        assert_eq!(both.absent_ids(), vec![16, 48, 900]);
        assert_eq!(both.headers.len(), 2);
    }

    /// Retail: the census over the installed member reproduces the per-member
    /// counts task #371 measured, the recorded-unknown positions it named, the
    /// four misspelt colours task #369 measured and the string-id difference
    /// task #368 measured — through the production readers, not a re-derivation.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f12_d_accounting_retail_census_matches_the_recorded_measurements() {
        let rof = retail_rof();

        // ASSETS/LAYOUT.CSV: 822 entries, 636 object records, 186 `V`/`G`
        // definitions, one unclassified line, nothing undeclared.
        let layout_bytes = retail_member(&rof, "ASSETS/LAYOUT.CSV");
        let layout = read_member_document("ASSETS/LAYOUT.CSV", &layout_bytes);
        let account = layout.member_account(Parity::GameplayCritical);
        assert_eq!(account.source().member_key(), Some("ASSETS/LAYOUT.CSV"));
        assert_eq!(account.keys().entries, 822);
        assert_eq!(account.keys().unclassified_lines, 1);
        assert_eq!(account.unconsumed().count(), 0);
        assert_eq!(account.blocking(), 0);
        assert!(account.parity_holds());
        assert_eq!(account.placeholders().definitions, 186);
        assert_eq!(account.placeholders().local_definitions, 157);
        assert_eq!(account.placeholders().global_definitions, 29);
        assert_eq!(account.placeholders().references, 1313);
        assert_eq!(account.placeholders().unresolved, 0);
        assert_eq!(account.unclassified()[0].line, 101);
        assert_eq!(account.unclassified()[0].reason, Unclassified::NoSeparator);
        assert_eq!(
            account.schemas().len(),
            10,
            "the ten record letters present"
        );

        // The record-kind table #371 measured, position census included.
        let expected: [(RecordKind, usize, usize, usize, usize); 10] = [
            (RecordKind::Button, 119, 1975, 137, 0),
            (RecordKind::Pane, 106, 954, 0, 0),
            (RecordKind::Text, 297, 2673, 0, 4),
            (RecordKind::EditBox, 4, 44, 0, 0),
            (RecordKind::Movie, 6, 54, 0, 0),
            (RecordKind::TextList, 16, 144, 0, 0),
            (RecordKind::ScrollingText, 8, 104, 0, 0),
            (RecordKind::Dropdown, 70, 840, 0, 0),
            (RecordKind::Listbox, 6, 60, 0, 0),
            (RecordKind::Slider, 4, 52, 0, 0),
        ];
        let mut records = 0usize;
        for (kind, count, fields, untyped, off_kind) in expected {
            let row = account
                .schema(RecordSchema::Layout(kind))
                .unwrap_or_else(|| panic!("the member holds {} records", kind.documented_name()));
            assert_eq!(row.records, count, "{kind:?} records");
            assert_eq!(row.fields, fields, "{kind:?} fields");
            assert_eq!(row.untyped(), untyped, "{kind:?} recorded unknowns");
            assert_eq!(row.off_kind(), off_kind, "{kind:?} off-kind values");
            assert_eq!(row.unsplit(), 0, "{kind:?} unsplit values");
            assert_eq!(row.beyond_schema, 0, "{kind:?} fields past the schema");
            records += row.records;
        }
        assert_eq!(records, 636, "the 636 object records of the member");

        // The button's five underdetermined positions: position 5 spells a
        // value in every record, positions 9, 11 and 12 in six each, and
        // position 10 is a `<NAME>` in those same six.
        let button = account
            .schema(RecordSchema::Layout(RecordKind::Button))
            .expect("the button roll-up");
        assert_eq!(button.positions()[5].untyped(), 119);
        assert_eq!(button.positions()[9].untyped(), 6);
        assert_eq!(button.positions()[10].placeholders, 6);
        assert_eq!(button.positions()[11].untyped(), 6);
        assert_eq!(button.positions()[12].untyped(), 6);
        assert_eq!(button.positions()[5].empty, 0);

        // The text colour position: 199 well-spelled values, 94 placeholders
        // and the four records #369 measured as misspelt.
        let text = account
            .schema(RecordSchema::Layout(RecordKind::Text))
            .expect("the text roll-up");
        let colour = text.positions()[7];
        assert_eq!(colour.observed, 297);
        assert_eq!(colour.described(), 199);
        assert_eq!(colour.placeholders, 94);
        assert_eq!(colour.off_kind, 4);

        // ASSETS/SCRAPBOOK.CSV: 461 sixteen-field items whose positions 3, 10
        // and 11 no kind covers.
        let scrapbook_bytes = retail_member(&rof, "ASSETS/SCRAPBOOK.CSV");
        let scrapbook = read_member_document("ASSETS/SCRAPBOOK.CSV", &scrapbook_bytes);
        let account = scrapbook.member_account(Parity::GameplayCritical);
        assert_eq!(account.keys().entries, 461);
        assert_eq!(account.unconsumed().count(), 0);
        assert_eq!(account.placeholders().definitions, 0);
        let item = account
            .schema(RecordSchema::Scrapbook)
            .expect("the scrapbook roll-up");
        assert_eq!(item.records, 461);
        assert_eq!(item.fields, 7376, "461 items of sixteen fields");
        assert_eq!(item.positions().len(), 16);
        assert_eq!(item.untyped(), 1377, "455 + 461 + 461");
        assert_eq!(item.positions()[3].untyped(), 455);
        assert_eq!(item.positions()[10].untyped(), 461);
        assert_eq!(item.positions()[11].untyped(), 461);
        assert_eq!(item.off_kind(), 0);

        // The string-id account: the two headers against the three images,
        // reproducing #368's numbers through the production readers.
        let resource = retail_member(&rof, "ASSETS/SCRIPTS/RESOURCE.H");
        let resrc1 = retail_member(&rof, "ASSETS/SCRIPTS/RESRC1.H");
        let mut context = ParseContext::with_defaults("ASSETS/SCRIPTS/RESOURCE.H");
        let first = read_resource_header(&mut context, &resource).expect("RESOURCE.H reads");
        let mut context = ParseContext::with_defaults("ASSETS/SCRIPTS/RESRC1.H");
        let second = read_resource_header(&mut context, &resrc1).expect("RESRC1.H reads");
        assert_eq!(first.defines().count(), 635);
        assert_eq!(second.defines().count(), 185);
        let headers = [
            ("ASSETS/SCRIPTS/RESOURCE.H", &first),
            ("ASSETS/SCRIPTS/RESRC1.H", &second),
        ];

        for (path, blocks, units, named, absent) in [
            ("strings.dll", 112usize, 1792usize, 123usize, 782usize - 123),
            ("GOSDATA/ASSETS/BINARIES/language.dll", 3, 48, 2, 782 - 2),
            ("GOSDATA/ASSETS/BINARIES/langui.dll", 101, 1616, 775, 7),
        ] {
            let bytes = std::fs::read(
                std::path::PathBuf::from(
                    std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR is set"),
                )
                .join(path),
            )
            .unwrap_or_else(|error| panic!("{path}: {error}"));
            let span = SourceSpan::new(
                ContentHash::from_bytes([3; 32]),
                path,
                None,
                0,
                bytes.len() as u64,
                None,
            )
            .expect("valid span");
            let mut context = ParseContext::with_defaults(path.to_owned());
            let catalog = StringCatalog::read(&mut context, span, &bytes).expect("the image reads");
            let account = account_string_ids(&catalog, &headers);
            assert_eq!(account.blocks, blocks, "{path}: blocks");
            assert_eq!(account.units, units, "{path}: counted units");
            assert_eq!(
                account.named_ids - account.absent_ids().len(),
                named,
                "{path}: distinct header ids this image carries"
            );
            assert_eq!(account.absent_ids().len(), absent, "{path}: absent ids");
            assert_eq!(
                account
                    .header("ASSETS/SCRIPTS/RESOURCE.H")
                    .map(|row| row.undecoded),
                Some(0),
                "{path}: every shipped value is a plain decimal"
            );
        }

        // The seven string-table ids #368 recorded as naming no `langui.dll`
        // block, and the eighteen of its blocks no header value names.
        let bytes = std::fs::read(
            std::path::PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR is set"))
                .join("GOSDATA/ASSETS/BINARIES/langui.dll"),
        )
        .expect("langui.dll reads");
        let span = SourceSpan::new(
            ContentHash::from_bytes([3; 32]),
            "GOSDATA/ASSETS/BINARIES/langui.dll",
            None,
            0,
            bytes.len() as u64,
            None,
        )
        .expect("valid span");
        let mut context = ParseContext::with_defaults("langui".to_owned());
        let catalog = StringCatalog::read(&mut context, span, &bytes).expect("the image reads");
        let account = account_string_ids(&catalog, &headers);
        assert_eq!(
            account
                .header("ASSETS/SCRIPTS/RESOURCE.H")
                .map(|row| row.string_absent.clone()),
            Some(vec![600, 620, 2002, 2050, 2054, 3510, 3540]),
            "the seven string-table ids RESOURCE.H names that langui.dll has no block for"
        );
        assert_eq!(
            account.unnamed_blocks,
            vec![
                2, 3, 4, 5, 190, 195, 196, 197, 198, 200, 201, 202, 204, 205, 206, 217, 219, 227
            ],
            "the eighteen langui.dll blocks neither header addresses"
        );
    }
}
