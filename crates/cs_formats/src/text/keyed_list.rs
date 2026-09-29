//! Lossless configuration nodes of the keyed field list dialect
//! ([`super::dialect::TextDialect::KeyedList`], stage F12-A).
//!
//! The dialect is what the surveyed `ASSETS/LAYOUT.CSV` and
//! `ASSETS/SCRAPBOOK.CSV` members of `crimson.rof` share: `[SECTION]`
//! header lines, whole-line `;` comments, blank lines and `KEY=field,field,…`
//! entries whose fields may be enclosed in double quotes that protect the
//! commas inside them. It is **not** CSV (there is no header row and the
//! first separator is `=`) and **not** a generic INI (fields, quoting); the
//! `.CSV` extension is only a name.
//!
//! This module turns every line into a node and keeps the bytes: nothing is
//! decoded or interpreted away, and the bytes a rule drops are still in
//! [`Field::raw`]. Only the lexical features the survey observed are
//! recognized; everything else is kept as a [`LineKind::Unclassified`] node
//! with the reason, never guessed:
//!
//! * a `;` is a comment only as the first non-blank byte of a line;
//!   whether the original reader strips a `;` *after* a value is unknown
//!   (no surveyed entry contains one), so such a `;` stays in the value;
//! * a quote only opens a field as the field's first byte and must close
//!   right before a `,` or the end of the value; escaped (`""`) quotes and
//!   quotes inside a field were never observed, so a value using them keeps
//!   its raw bytes as [`Fields::Unsplit`] with the offending offset;
//! * names and values are bytes. The survey found ASCII only, and the code
//!   page of a localized installation is unknown, so a non-ASCII key is
//!   neither rejected nor transcoded.
//!
//! # What the original reader does with a name (task #351)
//!
//! The F12-A survey could not settle how the original resolves a name. It
//! can now, from the retail data itself, and three rules are *established*
//! (`docs/findings/2026-09-29-t351-keyed-list-reading-rules.md`):
//!
//! * **R1 — a name is compared without regard to ASCII case.** Every one of
//!   the 34 `[@…@]` sections is named by a `.SCRIPT` member in a different
//!   case, and all 402 object names the 34 UI scripts bind by hand match an
//!   entry key only case-insensitively (0 match exactly). The reader keeps
//!   the bytes; the fold belongs to the consumer that resolves a name (in
//!   this workspace, `cs_content::config::ConfigDocument::lookup`), because
//!   a node is a node however it is looked up.
//! * **R2 — the blank bytes before a marker are not part of the name.**
//!   Twelve of the 34 section headers are indented (ten with a tab, two with
//!   four spaces) and 145 of the 402 bound object names sit on an indented
//!   entry line; a tab and four spaces are used interchangeably for the same
//!   nesting, so the *width* of the indent means nothing either.
//! * **R3 — the blank bytes around a key are not part of it**: 271 of the
//!   402 bound names sit on a line whose key is padded before the `=`, up
//!   to sixteen blanks wide.
//!
//! [`Field::value`] is the fourth established rule, **R4**: the blank bytes
//! around a *field* are dropped before the field is used. Seven fields of
//! `SCRAPBOOK.CSV` are padded, and each of the seven is a resource-id name
//! that a `.H` header defines byte for byte without the padding.
//!
//! The rules this stage could **not** establish stay recorded unknowns: a
//! `;` after a value, escaped or embedded quotes, whether a blank *inside*
//! quotes is part of the value, and what the original does with the one
//! line that starts with `:` (see [`Unclassified::NoSeparator`]).

use std::ops::Range;

use crate::error::ParseError;
use crate::io::{AllocationBudget, ParseContext};
use crate::text::lines::{LineTerminator, TextLine, scan_in};

/// Entrypoint label [`read_keyed_list`] scopes its errors with.
pub const KEYED_LIST_ENTRYPOINT: &str = "text.keyed_list";

/// The bytes the dialect treats as blank around names and markers.
const BLANK: &[u8] = b" \t";

/// One line of a keyed field list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyedListLine<'a> {
    /// Where the line is and what ended it.
    pub line: TextLine,
    /// The line's content, terminator excluded.
    pub content: &'a [u8],
    /// The leading blank bytes (spaces and tabs). They are not part of any
    /// name (**R2**): the survey found indented sections, comments and
    /// entries whose names the original resolves all the same, with a tab
    /// and four spaces used interchangeably. They are kept because the
    /// nesting they show is data, not noise.
    pub indent: &'a [u8],
    /// What the line is.
    pub kind: LineKind<'a>,
}

/// The classification of one line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineKind<'a> {
    /// Only blank bytes, or nothing.
    Blank,
    /// A whole-line comment: the first non-blank byte is `;`. `text` is
    /// everything after that `;`.
    Comment { text: &'a [u8] },
    /// `[name]`, optionally preceded by blank bytes and followed by them.
    /// `name` is the bytes between the brackets, verbatim: the indentation
    /// before the `[` is not part of it (**R2**).
    Section { name: &'a [u8] },
    /// `key=value`.
    Entry(Entry<'a>),
    /// A line none of the observed rules explains, kept verbatim and
    /// counted.
    Unclassified { reason: Unclassified },
}

/// Why a line is [`LineKind::Unclassified`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unclassified {
    /// No `=`, no comment marker and no section brackets, so there is
    /// nothing to call a key. The one such line of the surveyed members is
    /// line 101 of `LAYOUT.CSV`, whose first non-blank byte is `:`; this
    /// reader yields no entry for it, and that much is settled: every entry
    /// of both members *has* an `=`, so a line without one cannot be one of
    /// the record kinds either member documents. What the original does with
    /// such a line *besides* producing nothing — skip it silently, warn,
    /// count it — is **not** established, and neither is whether `:` is
    /// itself a comment marker. Both are recorded in
    /// `docs/findings/2026-09-29-t351-keyed-list-reading-rules.md`.
    NoSeparator,
    /// The `=` is the first non-blank byte, so the key is empty.
    EmptyKey,
    /// A line starting with `[` that is not exactly a bracketed name
    /// followed by blank bytes.
    MalformedSection,
}

impl Unclassified {
    /// Stable, machine-matchable label.
    pub const fn code(self) -> &'static str {
        match self {
            Self::NoSeparator => "no_separator",
            Self::EmptyKey => "empty_key",
            Self::MalformedSection => "malformed_section",
        }
    }
}

/// A `key=value` line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    /// The bytes before the first `=`, blank bytes around them removed
    /// (**R3**). Never empty.
    pub key: &'a [u8],
    /// Range of [`Self::key`] inside the line's content.
    pub key_range: Range<usize>,
    /// Range of the `=` inside the line's content.
    pub separator: usize,
    /// Everything after the first `=`, verbatim (blank bytes included).
    /// [`Field::value`] is the form a consumer compares (**R4**); this is
    /// the bytes as written.
    pub value: &'a [u8],
    /// The value split into fields, or why it could not be split.
    pub fields: Fields<'a>,
}

/// The fields of an entry's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fields<'a> {
    /// The value split at every `,` outside quotes. An empty value is one
    /// empty field, `a,` is two fields.
    Split(Vec<Field<'a>>),
    /// The value uses quoting the survey never observed; its raw bytes stay
    /// in [`Entry::value`] and nothing is split.
    Unsplit {
        /// What was not understood.
        issue: QuoteIssue,
        /// Offset of the offending quote inside [`Entry::value`].
        at: usize,
    },
}

/// Quoting the dialect's observed rules do not explain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuoteIssue {
    /// A field opens a quote that never closes.
    Unterminated,
    /// A closing quote is followed by something other than `,` or the end
    /// of the value (this includes a doubled `""` escape).
    TextAfterClosingQuote,
    /// A quote inside a field that did not start with one.
    QuoteInsideField,
}

impl QuoteIssue {
    /// Stable, machine-matchable label.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Unterminated => "unterminated_quote",
            Self::TextAfterClosingQuote => "text_after_closing_quote",
            Self::QuoteInsideField => "quote_inside_field",
        }
    }
}

/// One field of a [`Fields::Split`] value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field<'a> {
    /// The field as written, quotes included, blank bytes included.
    pub raw: &'a [u8],
    /// Range of [`Self::raw`] inside [`Entry::value`].
    pub range: Range<usize>,
    /// Whether the field was enclosed in quotes.
    pub quoted: bool,
    /// The field's text: [`Self::raw`] without its enclosing quotes. The
    /// blank bytes around the field are still in it; [`Self::value`] is the
    /// form the original reader hands to a consumer.
    pub text: &'a [u8],
}

impl Field<'_> {
    /// The field as the original reader hands it to a consumer: the blank
    /// bytes around it dropped, then the enclosing quotes removed (**R4**).
    ///
    /// Established from the retail data: seven fields of `SCRAPBOOK.CSV`
    /// carry trailing blanks, and each of the seven is a resource-id name
    /// that a `.H` header defines byte for byte without them, so a consumer
    /// that compares the field against the header needs the padding gone.
    /// [`Self::raw`] and [`Self::text`] keep the bytes either way, so
    /// [`KeyedList::reassemble`] is unaffected.
    ///
    /// A blank byte *inside* a quoted field is kept, because it is part of
    /// the value: a quoted field always starts and ends with its quotes (a
    /// blank outside them makes the value
    /// [`Fields::Unsplit`]/[`QuoteIssue::TextAfterClosingQuote`]), so the
    /// blanks this drops are always the field's own padding.
    pub fn value(&self) -> &[u8] {
        let trimmed = trim(self.raw);
        if self.quoted {
            &trimmed[1..trimmed.len() - 1]
        } else {
            trimmed
        }
    }
}

/// A keyed field list: one node per line, borrowing the input.
#[derive(Clone, Debug)]
pub struct KeyedList<'a> {
    bytes: &'a [u8],
    lines: Vec<KeyedListLine<'a>>,
}

impl<'a> KeyedList<'a> {
    /// The input the nodes borrow.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Every line in input order.
    pub fn lines(&self) -> &[KeyedListLine<'a>] {
        &self.lines
    }

    /// Every entry with the index of the section header it follows (`None`
    /// before the first header), in input order. Entries with the same key
    /// are all kept.
    pub fn entries(&self) -> impl Iterator<Item = (Option<usize>, &KeyedListLine<'a>, &Entry<'a>)> {
        let mut section = None;
        self.lines
            .iter()
            .enumerate()
            .filter_map(move |(index, line)| {
                match &line.kind {
                    LineKind::Section { .. } => section = Some(index),
                    LineKind::Entry(entry) => return Some((section, line, entry)),
                    _ => {}
                }
                None
            })
    }

    /// The lines no observed rule explains.
    pub fn unclassified(&self) -> impl Iterator<Item = &KeyedListLine<'a>> {
        self.lines
            .iter()
            .filter(|line| matches!(line.kind, LineKind::Unclassified { .. }))
    }

    /// Concatenates every line's content and terminator. Equal to the
    /// input for every input.
    pub fn reassemble(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.bytes.len());
        for line in &self.lines {
            out.extend_from_slice(line.content);
            out.extend_from_slice(line.terminator().bytes());
        }
        out
    }
}

impl KeyedListLine<'_> {
    /// The terminator that ended the line.
    pub fn terminator(&self) -> LineTerminator {
        self.line.terminator
    }
}

/// Reads `bytes` as a keyed field list.
///
/// Never fails on content: every line becomes a node, the ones no observed
/// rule explains as [`LineKind::Unclassified`]. The only errors are the
/// line and node tables and the entry field vectors not fitting this
/// parse's allocation budget — every buffer the reader allocates is
/// booked against it, so a refused read leaves the ledger as it found
/// it.
pub fn read_keyed_list<'a>(
    context: &mut ParseContext,
    bytes: &'a [u8],
) -> Result<KeyedList<'a>, ParseError> {
    context.parse(KEYED_LIST_ENTRYPOINT, bytes, |_reader, allocation, _| {
        let scanned = scan_in(allocation, bytes)?;
        allocation.reserve(
            "nodes",
            0,
            scanned.lines().len() as u64,
            std::mem::size_of::<KeyedListLine<'_>>() as u64,
        )?;
        let lines = scanned
            .lines()
            .iter()
            .map(|line| classify(allocation, line.clone(), scanned.content(line)))
            .collect::<Result<Vec<_>, ParseError>>()?;
        Ok(KeyedList { bytes, lines })
    })
}

fn classify<'a>(
    allocation: &mut AllocationBudget,
    line: TextLine,
    content: &'a [u8],
) -> Result<KeyedListLine<'a>, ParseError> {
    let body_start = content
        .iter()
        .position(|byte| !BLANK.contains(byte))
        .unwrap_or(content.len());
    let indent = &content[..body_start];
    let body = &content[body_start..];
    let kind = match body.first() {
        None => LineKind::Blank,
        Some(b';') => LineKind::Comment { text: &body[1..] },
        Some(b'[') => section(body),
        Some(_) => entry(allocation, line.offset, content, body_start)?,
    };
    Ok(KeyedListLine {
        line,
        content,
        indent,
        kind,
    })
}

fn section(body: &[u8]) -> LineKind<'_> {
    let trimmed = trim_end(body);
    match trimmed.iter().position(|&byte| byte == b']') {
        Some(close) if close + 1 == trimmed.len() => LineKind::Section {
            name: &trimmed[1..close],
        },
        _ => LineKind::Unclassified {
            reason: Unclassified::MalformedSection,
        },
    }
}

fn entry<'a>(
    allocation: &mut AllocationBudget,
    line_offset: u64,
    content: &'a [u8],
    body_start: usize,
) -> Result<LineKind<'a>, ParseError> {
    let Some(separator) = content.iter().position(|&byte| byte == b'=') else {
        return Ok(LineKind::Unclassified {
            reason: Unclassified::NoSeparator,
        });
    };
    let key_end = body_start + trim_end(&content[body_start..separator]).len();
    if key_end == body_start {
        return Ok(LineKind::Unclassified {
            reason: Unclassified::EmptyKey,
        });
    }
    let value = &content[separator + 1..];
    let fields = split_fields(allocation, line_offset + separator as u64 + 1, value)?;
    Ok(LineKind::Entry(Entry {
        key: &content[body_start..key_end],
        key_range: body_start..key_end,
        separator,
        value,
        fields,
    }))
}

/// Splits `value` at the commas its observed quoting rules allow, booking
/// every field row of the resulting vector against `allocation` (its
/// anchor is `value_offset`, the value's first byte in the container) so
/// that a value with more fields than the budget allows is refused
/// instead of allocating them. A value that quoting the survey never
/// observed keeps its bytes unsplit and books nothing.
fn split_fields<'a>(
    allocation: &mut AllocationBudget,
    value_offset: u64,
    value: &'a [u8],
) -> Result<Fields<'a>, ParseError> {
    let mut fields = Vec::new();
    let mut start = 0usize;
    loop {
        let (end, quoted) = if value.get(start) == Some(&b'"') {
            let Some(close) = value[start + 1..].iter().position(|&byte| byte == b'"') else {
                return Ok(Fields::Unsplit {
                    issue: QuoteIssue::Unterminated,
                    at: start,
                });
            };
            let end = start + 1 + close + 1;
            if end < value.len() && value[end] != b',' {
                return Ok(Fields::Unsplit {
                    issue: QuoteIssue::TextAfterClosingQuote,
                    at: end - 1,
                });
            }
            (end, true)
        } else {
            let end = value[start..]
                .iter()
                .position(|&byte| byte == b',')
                .map_or(value.len(), |relative| start + relative);
            if let Some(quote) = value[start..end].iter().position(|&byte| byte == b'"') {
                return Ok(Fields::Unsplit {
                    issue: QuoteIssue::QuoteInsideField,
                    at: start + quote,
                });
            }
            (end, false)
        };
        allocation.reserve(
            "fields",
            value_offset + start as u64,
            1,
            std::mem::size_of::<Field>() as u64,
        )?;
        let raw = &value[start..end];
        fields.push(Field {
            raw,
            range: start..end,
            quoted,
            text: if quoted { &raw[1..raw.len() - 1] } else { raw },
        });
        if end == value.len() {
            return Ok(Fields::Split(fields));
        }
        start = end + 1;
    }
}

fn trim_end(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .rposition(|byte| !BLANK.contains(byte))
        .map_or(0, |last| last + 1);
    &bytes[..end]
}

/// The bytes of `bytes` without the blank bytes at either end (**R4**).
fn trim(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !BLANK.contains(byte))
        .unwrap_or(bytes.len());
    trim_end(&bytes[start..])
}
