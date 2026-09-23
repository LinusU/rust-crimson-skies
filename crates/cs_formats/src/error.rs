//! Structured parse failures with provenance.
//!
//! Every failure produced by [`crate::io::Reader`] records where the bytes
//! came from (the archive/container), the absolute offset of the failing read,
//! the logical field being read, and the expected versus observed condition
//! (`specs/F03-bounded-binary-parsing-primitives.md`, deliverable paragraph).
//!
//! Errors carry metadata only. Payload bytes are never copied into an error
//! message, so a diagnostic can never dump unrelated private data from the
//! read-only original installation.

use std::fmt;

/// Machine-matchable class of a [`ParseError`].
///
/// Callers match on this instead of parsing [`ParseError`]'s `Display` text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParseErrorKind {
    /// Fewer bytes remained in the current range than the operation needs.
    UnexpectedEof,
    /// `count * element_size` or `offset + length` overflowed.
    LengthOverflow,
    /// The bytes were structurally present but not valid UTF-8 text.
    InvalidEncoding,
    /// A bounded string held no terminator inside its bound.
    MissingTerminator,
}

impl ParseErrorKind {
    /// Stable lowercase identifier, for logs and structured diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnexpectedEof => "unexpected_eof",
            Self::LengthOverflow => "length_overflow",
            Self::InvalidEncoding => "invalid_encoding",
            Self::MissingTerminator => "missing_terminator",
        }
    }
}

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Contextual failure from one reader operation.
///
/// All fields are plain data: an error is safe to log and to attach to a
/// catalog row without redaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// Archive/container the bytes were read from (provenance, not a path
    /// that gets joined anywhere).
    pub container: String,
    /// Absolute byte offset of the failing read within `container`.
    pub offset: u64,
    /// Logical field name, e.g. `header.count`.
    pub field: String,
    /// Machine-matchable class of the failure.
    pub kind: ParseErrorKind,
    /// Expected condition, e.g. `4 bytes available`.
    pub expected: String,
    /// Observed value, e.g. `0 bytes available`. Counts and indices only,
    /// never file payload.
    pub observed: String,
}

impl ParseError {
    pub(crate) fn unexpected_eof(
        container: String,
        offset: u64,
        field: &str,
        needed: u64,
        available: u64,
    ) -> Self {
        Self {
            container,
            offset,
            field: field.to_owned(),
            kind: ParseErrorKind::UnexpectedEof,
            expected: format!("{needed} bytes available"),
            observed: format!("{available} bytes available"),
        }
    }

    pub(crate) fn length_overflow(
        container: String,
        offset: u64,
        field: &str,
        expected: String,
        observed: String,
    ) -> Self {
        Self {
            container,
            offset,
            field: field.to_owned(),
            kind: ParseErrorKind::LengthOverflow,
            expected,
            observed,
        }
    }

    pub(crate) fn invalid_encoding(
        container: String,
        offset: u64,
        field: &str,
        valid_up_to: usize,
    ) -> Self {
        Self {
            container,
            offset,
            field: field.to_owned(),
            kind: ParseErrorKind::InvalidEncoding,
            expected: "valid UTF-8 text".to_owned(),
            // Index only: the offending bytes themselves stay out of the error.
            observed: format!("first invalid byte at index {valid_up_to}"),
        }
    }

    pub(crate) fn missing_terminator(
        container: String,
        offset: u64,
        field: &str,
        bound: u64,
    ) -> Self {
        Self {
            container,
            offset,
            field: field.to_owned(),
            kind: ParseErrorKind::MissingTerminator,
            expected: format!("a 0x00 terminator within {bound} bytes"),
            observed: format!("no 0x00 terminator in {bound} bytes"),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at offset {} field `{}`: expected {}, observed {} ({})",
            self.container, self.offset, self.field, self.expected, self.observed, self.kind
        )
    }
}

impl std::error::Error for ParseError {}
