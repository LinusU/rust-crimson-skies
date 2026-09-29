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
//! Consumers look keys up through the document, which counts what they
//! used: every entry nobody consumed is retained and reported by
//! [`ConfigDocument::accounting`] (spec F12, non-negotiable #5). Keys and
//! section names compare as exact bytes, because whether the original
//! reader folds case is unknown; two entries answering one lookup fail
//! visibly as [`Lookup::Ambiguous`].
//!
//! Values stay raw here: [`TuningSchema`] turns a declared field into a
//! checked, typed [`Tuning`], and a field nothing is known about is never
//! converted.

use std::fmt;

use cs_formats::AllocationBudget;
use cs_formats::ParseContext;
use cs_formats::error::ParseError;
use cs_formats::text::{
    DialectReader, Fields, KeyedList, LineKind, LineTerminator, QuoteIssue, TextDialect,
    Unclassified, dialect_for_member, read_keyed_list,
};
use cs_formats::{PeError, PeResources, RT_STRING, ResourceKey, ResourceLeaf, read_pe_resources};
use cs_types::asset_id::SourceSpan;

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
    /// The field without its enclosing quotes.
    pub fn text(&self) -> &[u8] {
        if self.quoted {
            &self.raw[1..self.raw.len() - 1]
        } else {
            &self.raw
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
        context
            .parse(CONFIG_ENTRYPOINT, bytes, |_reader, allocation, _| {
                Self::from_keyed_list(allocation, source, dialect, &list)
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
    pub fn lookup(&mut self, section: Option<&[u8]>, key: &[u8]) -> Lookup<'_> {
        let matching: Vec<usize> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                matches!(&node.kind, ConfigNodeKind::Entry(entry)
                    if entry.section.as_deref() == section && entry.key == key)
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
        let text = trim(field.text());
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
    /// * a single `.`, with digits on at least one side — `Inferred`: the
    ///   survey found **no** fractional value in either member, so this
    ///   spelling is a *designed* reading rule and not a measured one. It is
    ///   recorded as an unknown in
    ///   `docs/findings/2026-09-29-f12-b-pe-resource-reader-and-typed-values.md`.
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

#[cfg(test)]
mod tests {
    use cs_types::evidence::ContentHash;

    use super::*;

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
}
