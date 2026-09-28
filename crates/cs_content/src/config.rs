//! Lossless configuration documents with provenance and key accounting
//! (`specs/F12-text-configuration-strings-and-pe-resources.md`, stage
//! `### F12-A`).
//!
//! A [`ConfigDocument`] is one configuration member turned into owned
//! nodes, one per line, that keep every byte ([`ConfigDocument::reassemble`]
//! gives the member back) and remember where they came from
//! ([`ConfigDocument::source`], [`ConfigNode::offset`]). It is built only
//! for a member the dialect inventory routes to a dialect with a reader
//! ([`cs_formats::text::dialect_for_member`]); anything else is a
//! [`ConfigError`], never a best-effort parse.
//!
//! Consumers look keys up through the document, which counts what they
//! used: every entry nobody consumed is retained and reported by
//! [`ConfigDocument::accounting`] (spec F12, non-negotiable #5). Keys and
//! section names compare as exact bytes, because whether the original
//! reader folds case is unknown; two entries answering one lookup fail
//! visibly as [`Lookup::Ambiguous`].
//!
//! Values stay raw here: typed, checked numeric conversion is stage F12-B.

use std::fmt;

use cs_formats::AllocationBudget;
use cs_formats::ParseContext;
use cs_formats::error::ParseError;
use cs_formats::text::{
    DialectReader, Fields, KeyedList, LineKind, LineTerminator, QuoteIssue, TextDialect,
    Unclassified, dialect_for_member, read_keyed_list,
};
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
}
