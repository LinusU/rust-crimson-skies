//! The source map of the measured UI script programs (F38-C).
//!
//! `specs/F38-original-program-adapters-and-native-behavior-bindings.md`,
//! stage `### F38-C`, non-negotiable behavior 4: a runtime event or a
//! diagnostic must be traceable to its archive member, byte offset and line.
//!
//! [`ui_host_calls`](super::ui_host_calls) measures each site's byte span
//! relative to its member. This module turns that span into a
//! [`SiteOrigin`] — member spelling, offset, length, 1-based line and column —
//! by reading the program's own bytes, and never retains any of them. The
//! container is not named here: a consumer knows which container it opened and
//! prefixes it.
//!
//! The walk is bounded and cannot panic on untrusted input: a span outside the
//! program is [`SourceMapError::SpanOutsideProgram`], not an index.

use std::fmt;

use crate::script_raw::ui_host_calls::{HostCallSite, UiProgramScan};

/// Most sites one program's map holds (the scanner's own bound is the same).
pub const MAX_MAPPED_SITES: usize = crate::script_raw::ui_host_calls::MAX_HOST_CALL_SITES;

/// Where one measured site lives: enough to find it again in the member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiteOrigin {
    /// The member's spelling inside its container.
    pub member: String,
    /// Byte offset of the call statement's first byte in the member.
    pub offset: u64,
    /// Length of the call statement in bytes.
    pub len: u64,
    /// 1-based line of the first byte (`\n` terminates a line).
    pub line: u32,
    /// 1-based byte column of the first byte on its line.
    pub column: u32,
}

impl fmt::Display for SiteOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{} (0x{:x}+{})",
            self.member, self.line, self.column, self.offset, self.len
        )
    }
}

/// A site paired with where it was found.
#[derive(Clone, Debug, PartialEq)]
pub struct MappedSite {
    pub site: HostCallSite,
    pub origin: SiteOrigin,
}

/// Why a program could not be mapped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceMapError {
    /// A site's span does not lie inside the bytes it was measured from: the
    /// scan and the bytes disagree, so no location is claimed.
    SpanOutsideProgram {
        offset: u64,
        len: u64,
        program: usize,
    },
    /// The program has more sites than [`MAX_MAPPED_SITES`].
    TooManySites { sites: usize },
    /// The program is larger than a `u32` line/column can address.
    ProgramTooLarge { len: usize },
}

impl SourceMapError {
    /// Stable short code for reports.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::SpanOutsideProgram { .. } => "span_outside_program",
            Self::TooManySites { .. } => "too_many_sites",
            Self::ProgramTooLarge { .. } => "program_too_large",
        }
    }
}

impl fmt::Display for SourceMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SpanOutsideProgram {
                offset,
                len,
                program,
            } => write!(
                f,
                "site span 0x{offset:x}+{len} lies outside the {program}-byte program"
            ),
            Self::TooManySites { sites } => {
                write!(f, "{sites} sites exceed the {MAX_MAPPED_SITES} map bound")
            }
            Self::ProgramTooLarge { len } => write!(f, "a {len}-byte program is too large"),
        }
    }
}

impl std::error::Error for SourceMapError {}

/// The 1-based `(line, column)` of byte `offset` in `bytes`; `None` when
/// `offset > bytes.len()`. Pure and O(offset).
pub fn line_column(bytes: &[u8], offset: u64) -> Option<(u32, u32)> {
    let offset = usize::try_from(offset).ok()?;
    let head = bytes.get(..offset)?;
    let line = head.iter().filter(|&&b| b == b'\n').count();
    let line_start = head
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |at| at + 1);
    Some((
        u32::try_from(line).ok()?.checked_add(1)?,
        u32::try_from(offset - line_start).ok()?.checked_add(1)?,
    ))
}

/// Maps every measured site of `scan` to its origin in `bytes`.
///
/// # Errors
///
/// [`SourceMapError`] when a span is outside `bytes`, the program has too many
/// sites or is too large to address. No partial map is returned.
pub fn map_sites(scan: &UiProgramScan, bytes: &[u8]) -> Result<Vec<MappedSite>, SourceMapError> {
    if scan.sites.len() > MAX_MAPPED_SITES {
        return Err(SourceMapError::TooManySites {
            sites: scan.sites.len(),
        });
    }
    if u32::try_from(bytes.len()).is_err() {
        return Err(SourceMapError::ProgramTooLarge { len: bytes.len() });
    }
    let mut out = Vec::with_capacity(scan.sites.len());
    for site in &scan.sites {
        let outside = SourceMapError::SpanOutsideProgram {
            offset: site.span.offset,
            len: site.span.len,
            program: bytes.len(),
        };
        if site.span.end() > bytes.len() as u64 {
            return Err(outside);
        }
        // The span is inside the program now, so `line_column` can only fail
        // because a line or column number no longer fits a `u32`. That is the
        // program being too large to address, not a span outside it, and the
        // diagnostic says which of the two happened.
        let (line, column) = line_column(bytes, site.span.offset)
            .ok_or(SourceMapError::ProgramTooLarge { len: bytes.len() })?;
        out.push(MappedSite {
            site: site.clone(),
            origin: SiteOrigin {
                member: scan.spelling.clone(),
                offset: site.span.offset,
                len: site.span.len,
                line,
                column,
            },
        });
    }
    Ok(out)
}
