//! Lossless line scanning (stage F12-A).
//!
//! Every configuration dialect in the inventory ([`super::dialect`]) is a
//! sequence of lines, and the first thing a lossy reader gets wrong is the
//! line itself: splitting on `\n` and trimming `\r`, or collapsing the last
//! unterminated line, loses bytes that a round trip must reproduce. The
//! scanner here records for every line its absolute offset, its content
//! range and the terminator that ended it, so the input is always
//! reassembled byte for byte ([`TextLines::reassemble`]).
//!
//! Only `\n` ends a line. A `\r` directly in front of it makes the
//! terminator [`LineTerminator::CrLf`]; a `\r` anywhere else is content,
//! because no surveyed member uses a lone carriage return as a line break
//! (`docs/findings/2026-09-28-f12-a-text-dialects-and-lossless-config-nodes.md`).

use std::ops::Range;

use crate::error::ParseError;
use crate::io::{AllocationBudget, ParseContext};

/// Entrypoint label [`scan_lines`] scopes its errors with.
pub const LINES_ENTRYPOINT: &str = "text.lines";

/// What ended one line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineTerminator {
    /// `\r\n`, the terminator of every surveyed `.CSV`, `.SCRIPT` and `.H`
    /// member.
    CrLf,
    /// A bare `\n` (the surveyed `DEBUGINFO.TXT`).
    Lf,
    /// No terminator: the last line of an input that does not end in `\n`.
    None,
}

impl LineTerminator {
    /// The terminator's bytes, empty for [`LineTerminator::None`].
    pub const fn bytes(self) -> &'static [u8] {
        match self {
            Self::CrLf => b"\r\n",
            Self::Lf => b"\n",
            Self::None => b"",
        }
    }
}

/// One scanned line: where it is and what ended it. The bytes stay in the
/// input; [`TextLines::content`] and [`TextLines::raw`] borrow them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextLine {
    /// 1-based line number.
    pub number: u64,
    /// Absolute offset of the line's first byte in the scanned input.
    pub offset: u64,
    /// The line's content, terminator excluded, as a range of the input.
    pub content: Range<usize>,
    /// The terminator that follows the content.
    pub terminator: LineTerminator,
}

impl TextLine {
    /// Content plus terminator length.
    pub fn raw_len(&self) -> usize {
        self.content.len() + self.terminator.bytes().len()
    }
}

/// How many lines ended with each terminator.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminatorCounts {
    /// Lines ended by `\r\n`.
    pub crlf: u64,
    /// Lines ended by a bare `\n`.
    pub lf: u64,
    /// Lines without a terminator (at most one, the last).
    pub none: u64,
}

/// The scanned lines of one input, borrowing the input.
#[derive(Clone, Debug)]
pub struct TextLines<'a> {
    bytes: &'a [u8],
    lines: Vec<TextLine>,
}

impl<'a> TextLines<'a> {
    /// The scanned input.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Every line in input order. An empty input has no lines; an input
    /// ending in `\n` has no empty trailing line.
    pub fn lines(&self) -> &[TextLine] {
        &self.lines
    }

    /// The content of `line`, terminator excluded.
    pub fn content(&self, line: &TextLine) -> &'a [u8] {
        &self.bytes[line.content.clone()]
    }

    /// The content of `line` with its terminator.
    pub fn raw(&self, line: &TextLine) -> &'a [u8] {
        &self.bytes[line.content.start..line.content.end + line.terminator.bytes().len()]
    }

    /// Terminator statistics, the dialect inventory's observed-terminator
    /// check.
    pub fn terminator_counts(&self) -> TerminatorCounts {
        let mut counts = TerminatorCounts::default();
        for line in &self.lines {
            match line.terminator {
                LineTerminator::CrLf => counts.crlf += 1,
                LineTerminator::Lf => counts.lf += 1,
                LineTerminator::None => counts.none += 1,
            }
        }
        counts
    }

    /// Concatenates every line with its own terminator. Equal to the input
    /// for every input.
    pub fn reassemble(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.bytes.len());
        for line in &self.lines {
            out.extend_from_slice(self.raw(line));
        }
        out
    }
}

/// Scans `bytes` into lines without dropping or rewriting a byte.
///
/// The line table is booked against the context's allocation budget before
/// it is built, so an input with more lines than the budget allows fails
/// as [`crate::ParseErrorKind::AllocationBudgetExceeded`] and leaves the
/// ledger as it was.
pub fn scan_lines<'a>(
    context: &mut ParseContext,
    bytes: &'a [u8],
) -> Result<TextLines<'a>, ParseError> {
    context.parse(
        LINES_ENTRYPOINT,
        bytes,
        |_reader, allocation, _recursion| scan_in(allocation, bytes),
    )
}

/// [`scan_lines`] inside a parse another entrypoint already opened, so the
/// line table is rolled back with that parse when it fails.
pub(crate) fn scan_in<'a>(
    allocation: &mut AllocationBudget,
    bytes: &'a [u8],
) -> Result<TextLines<'a>, ParseError> {
    let newlines = bytes.iter().filter(|&&byte| byte == b'\n').count() as u64;
    let tail = u64::from(!bytes.is_empty() && !bytes.ends_with(b"\n"));
    let count = newlines + tail;
    allocation.reserve("lines", 0, count, std::mem::size_of::<TextLine>() as u64)?;
    Ok(TextLines {
        bytes,
        lines: split(bytes, count as usize),
    })
}

fn split(bytes: &[u8], count: usize) -> Vec<TextLine> {
    let mut lines = Vec::with_capacity(count);
    let mut start = 0usize;
    while start < bytes.len() {
        let line = match bytes[start..].iter().position(|&byte| byte == b'\n') {
            Some(relative) => {
                let newline = start + relative;
                if newline > start && bytes[newline - 1] == b'\r' {
                    (start..newline - 1, LineTerminator::CrLf)
                } else {
                    (start..newline, LineTerminator::Lf)
                }
            }
            None => (start..bytes.len(), LineTerminator::None),
        };
        let (content, terminator) = line;
        let next = content.end + terminator.bytes().len();
        lines.push(TextLine {
            number: lines.len() as u64 + 1,
            offset: start as u64,
            content,
            terminator,
        });
        start = next;
    }
    lines
}
