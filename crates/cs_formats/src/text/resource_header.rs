//! The `#define` resource-id header dialect
//! ([`super::dialect::TextDialect::ResourceHeader`], stage F12-B).
//!
//! `ASSETS/SCRIPTS/RESOURCE.H` and `ASSETS/SCRIPTS/RESRC1.H` of
//! `crimson.rof` are Microsoft Developer Studio resource headers: a
//! `//{{NO_DEPENDENCIES}}` marker, a licence comment, an include guard
//! (`#ifndef` / `#define` / `#endif`) and several hundred `#define NAME
//! <decimal>` lines naming the resource ids the linker assigned. F12-A
//! deferred this dialect to F12-B because "the header-to-resource id mapping
//! needs the PE resource reader": the ids in the header are the same id space
//! [`crate::pe_resources`] walks in a PE resource directory, so the two are
//! read by the same value rules.
//!
//! The reader keeps every line and every byte. A `#define`'s value is
//! `ResourceIdValue`, which interprets **only** the one shape the survey
//! observed — a plain decimal inside the 16-bit id space — and keeps anything
//! else (`0x` hexadecimal, an expression, a negative number, a value too
//! wide) as raw bytes with a label of why it is not an id. No value is
//! coerced, widened or truncated.

use std::ops::Range;

use crate::error::ParseError;
use crate::io::ParseContext;
use crate::text::lines::{LineTerminator, TextLine, scan_in};

/// Entrypoint label [`read_resource_header`] scopes its errors with.
pub const RESOURCE_HEADER_ENTRYPOINT: &str = "text.resource_header";

/// The highest resource id the PE resource format addresses, and so the
/// highest value a decimal `#define` can be read as (`Documented`).
pub const MAX_RESOURCE_ID: u32 = u16::MAX as u32;

/// One line of a resource header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceHeaderLine<'a> {
    /// Where the line is and what ended it.
    pub line: TextLine,
    /// The line's content, terminator excluded.
    pub content: &'a [u8],
    /// The leading blank bytes.
    pub indent: &'a [u8],
    /// What the line is.
    pub kind: ResourceHeaderKind<'a>,
}

/// The classification of one [`ResourceHeaderLine`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceHeaderKind<'a> {
    /// Only blank bytes, or nothing.
    Blank,
    /// A whole-line comment: the first non-blank bytes are `//`.
    Comment {
        /// Everything after the `//`.
        text: &'a [u8],
    },
    /// A preprocessor directive that is not a `#define` (`#ifndef`,
    /// `#endif`, `#include`, …). The bytes after the directive name are
    /// kept verbatim; no conditional is evaluated.
    Directive {
        /// The directive's name without its `#`.
        name: &'a [u8],
        /// The rest of the line after the name.
        rest: &'a [u8],
    },
    /// A `#define NAME value` line.
    Define(Define<'a>),
    /// A line none of the observed rules explain, kept verbatim and counted.
    Unclassified {
        /// The line's content.
        text: &'a [u8],
    },
}

/// One `#define` line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Define<'a> {
    /// 1-based line number of the `#define` in the member.
    pub line: u64,
    /// The macro name, blank bytes around it removed. Empty when the line is
    /// a bare `#define`; such a line is kept and counted, not guessed at.
    pub name: &'a [u8],
    /// Range of [`Self::name`] inside the line's content.
    pub name_range: Range<usize>,
    /// The value after the name, verbatim.
    pub value: ResourceIdValue<'a>,
}

/// A `#define`'s value: the raw bytes, and what (if anything) they mean as a
/// resource id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceIdValue<'a> {
    /// The value's bytes, verbatim: no padding removed, so a caller that
    /// needs the exact spelling has it.
    pub raw: &'a [u8],
    /// Range of [`Self::raw`] inside the line's content.
    pub range: Range<usize>,
}

impl<'a> ResourceIdValue<'a> {
    /// The value with its surrounding blank bytes removed — the bytes a
    /// number would have to be spelled in. Blank bytes are *not* removed
    /// from [`Self::raw`], so the original spacing survives.
    pub fn text(&self) -> &'a [u8] {
        let start = self
            .raw
            .iter()
            .position(|byte| !BLANK.contains(byte))
            .unwrap_or(self.raw.len());
        let end = self.raw[start..]
            .iter()
            .rposition(|byte| !BLANK.contains(byte))
            .map_or(start, |last| start + last + 1);
        &self.raw[start..end]
    }

    /// Whether the value is a plain decimal inside
    /// [`MAX_RESOURCE_ID`], and so a resource id the PE resource reader could
    /// be asked for.
    ///
    /// Exactly the shape the survey found in both members: one to five
    /// decimal digits naming a value of at most [`MAX_RESOURCE_ID`]. The blank
    /// bytes around the number are the dialect's own padding and are not part
    /// of it, so ` 42 ` is `true`; a leading zero is a different spelling of
    /// the same number but still a decimal, so `007` is `true` too. A
    /// `0x`-prefixed value, a sign, a blank-separated expression, a value
    /// wider than five digits and anything above [`MAX_RESOURCE_ID`] are all
    /// `false`; each keeps its raw bytes and is reported as unknown rather
    /// than converted.
    pub fn is_decimal(&self) -> bool {
        let text = self.text();
        !text.is_empty()
            && text.len() <= 5
            && text.iter().all(u8::is_ascii_digit)
            && self.resource_id().is_some()
    }

    /// The resource id this value names, or `None` when it does not name
    /// one.
    ///
    /// Only a plain decimal of at most five digits counts: the surveyed
    /// values are 1 to 5 digits and never exceed `u16::MAX`, and this is the
    /// only conversion the reader performs. A value that is not a decimal is
    /// `None`, never a partial parse.
    pub fn resource_id(&self) -> Option<u32> {
        let text = self.text();
        if text.is_empty() || text.len() > 5 || !text.iter().all(u8::is_ascii_digit) {
            return None;
        }
        let mut id = 0u32;
        for byte in text {
            id = id.checked_mul(10)?.checked_add(u32::from(byte - b'0'))?;
        }
        (id <= MAX_RESOURCE_ID).then_some(id)
    }
}

impl Define<'_> {
    /// The resource id this define names, or `None`.
    pub fn resource_id(&self) -> Option<u32> {
        self.value.resource_id()
    }
}

/// The answer to one [`ResourceHeader::lookup`]: `'a` is the header's byte
/// lifetime, `'b` the borrow of the header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderLookup<'a, 'b> {
    /// No define has this name.
    Missing,
    /// Exactly one define.
    Found(&'b Define<'a>),
    /// This many defines share the name; none is returned.
    Ambiguous(usize),
}

/// A resource header: one node per line, borrowing the input.
#[derive(Clone, Debug)]
pub struct ResourceHeader<'a> {
    bytes: &'a [u8],
    lines: Vec<ResourceHeaderLine<'a>>,
}

impl<'a> ResourceHeader<'a> {
    /// The input the nodes borrow.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Every line in input order.
    pub fn lines(&self) -> &[ResourceHeaderLine<'a>] {
        &self.lines
    }

    /// Every `#define`, in input order, with duplicates included.
    pub fn defines(&self) -> impl Iterator<Item = &Define<'a>> {
        self.lines.iter().filter_map(|line| match &line.kind {
            ResourceHeaderKind::Define(define) => Some(define),
            _ => None,
        })
    }

    /// The lines no observed rule explains.
    pub fn unclassified(&self) -> impl Iterator<Item = &ResourceHeaderLine<'a>> {
        self.lines
            .iter()
            .filter(|line| matches!(line.kind, ResourceHeaderKind::Unclassified { .. }))
    }

    /// Looks up the defines named `name`; a name two lines share is
    /// [`HeaderLookup::Ambiguous`], never a silent choice.
    pub fn lookup(&self, name: &[u8]) -> HeaderLookup<'_, '_> {
        let mut count = 0usize;
        let mut found: Option<&Define<'a>> = None;
        for define in self.defines() {
            if define.name == name {
                count += 1;
                if found.is_none() {
                    found = Some(define);
                }
            }
        }
        match (found, count) {
            (None, _) => HeaderLookup::Missing,
            (Some(define), 1) => HeaderLookup::Found(define),
            (_, many) => HeaderLookup::Ambiguous(many),
        }
    }

    /// The one define named `name`, or `None` when it is missing or
    /// ambiguous.
    pub fn define(&self, name: &[u8]) -> Option<&Define<'a>> {
        let mut count = 0usize;
        let mut found: Option<&Define<'a>> = None;
        for define in self.defines() {
            if define.name == name {
                count += 1;
                if found.is_none() {
                    found = Some(define);
                }
            }
        }
        if count == 1 { found } else { None }
    }

    /// The resource id the single define `name` names, or `None` when the
    /// name is missing, ambiguous, or names no id.
    ///
    /// This is the pairing F12-A recorded as the reason the resource headers
    /// were deferred: the id a `.H` member names is the id
    /// [`crate::pe_resources`] resolves in a PE resource directory.
    pub fn resource_id(&self, name: &[u8]) -> Option<u32> {
        self.define(name).and_then(Define::resource_id)
    }

    /// Concatenates every line's content and terminator. Equal to the input
    /// for every input.
    pub fn reassemble(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.bytes.len());
        for line in &self.lines {
            out.extend_from_slice(line.content);
            out.extend_from_slice(line.terminator().bytes());
        }
        out
    }
}

impl ResourceHeaderLine<'_> {
    /// The terminator that ended the line.
    pub fn terminator(&self) -> LineTerminator {
        self.line.terminator
    }
}

/// Reads `bytes` as a resource header.
///
/// Never fails on content: every line becomes a node and the ones no observed
/// rule explains are kept as [`ResourceHeaderKind::Unclassified`]. The only
/// errors are the line and node tables not fitting this parse's allocation
/// budget, which leaves the ledger as it found it.
pub fn read_resource_header<'a>(
    context: &mut ParseContext,
    bytes: &'a [u8],
) -> Result<ResourceHeader<'a>, ParseError> {
    context.parse(
        RESOURCE_HEADER_ENTRYPOINT,
        bytes,
        |_reader, allocation, _| {
            let scanned = scan_in(allocation, bytes)?;
            allocation.reserve(
                "header_lines",
                0,
                scanned.lines().len() as u64,
                std::mem::size_of::<ResourceHeaderLine<'_>>() as u64,
            )?;
            let lines = scanned
                .lines()
                .iter()
                .map(|line| classify(line.clone(), scanned.content(line)))
                .collect::<Vec<_>>();
            Ok(ResourceHeader { bytes, lines })
        },
    )
}

/// Bytes the dialect treats as blank around names and markers.
const BLANK: &[u8] = b" \t";

fn classify<'a>(line: TextLine, content: &'a [u8]) -> ResourceHeaderLine<'a> {
    let body_start = content
        .iter()
        .position(|byte| !BLANK.contains(byte))
        .unwrap_or(content.len());
    let indent = &content[..body_start];
    let body = &content[body_start..];
    let kind = match body.first() {
        None => ResourceHeaderKind::Blank,
        Some(b'/') if body.starts_with(b"//") => ResourceHeaderKind::Comment { text: &body[2..] },
        Some(b'#') => directive(line.number, body_start, content),
        Some(_) => ResourceHeaderKind::Unclassified { text: body },
    };
    ResourceHeaderLine {
        line,
        content,
        indent,
        kind,
    }
}

/// Classifies a line whose first non-blank byte is `#`. `body_start` is where
/// the body begins inside `content`, so every range a node reports is a range
/// of the line's content.
fn directive<'a>(number: u64, body_start: usize, content: &'a [u8]) -> ResourceHeaderKind<'a> {
    let after_hash = body_start + 1;
    let body = &content[after_hash..];
    let name_end = after_hash
        + body
            .iter()
            .position(|byte| BLANK.contains(byte))
            .unwrap_or(body.len());
    let name = &content[after_hash..name_end];
    if name != b"define" {
        return ResourceHeaderKind::Directive {
            name,
            rest: &content[name_end..],
        };
    }
    // `#define NAME value`: the name runs to the first blank byte, so a value
    // may itself contain blanks (an expression) and is kept whole.
    let name_start = content[name_end..]
        .iter()
        .position(|byte| !BLANK.contains(byte))
        .map_or(name_end, |relative| name_end + relative);
    let value_start = content[name_start..]
        .iter()
        .position(|byte| BLANK.contains(byte))
        .map_or(content.len(), |relative| name_start + relative);
    ResourceHeaderKind::Define(Define {
        line: number,
        name: &content[name_start..value_start],
        name_range: name_start..value_start,
        value: ResourceIdValue {
            raw: &content[value_start..],
            range: value_start..content.len(),
        },
    })
}
