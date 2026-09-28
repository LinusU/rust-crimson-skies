//! Documented ZBD header rules.
//!
//! "ZBD" is a family label, not one layout (spec F06, deliverable
//! paragraph), and the spec's research boundary is explicit: "No universal
//! ZBD header is asserted by this specification"; precise sound/reader
//! header variants must be read from the pinned source and checked against
//! the installed game (`specs/F06-…`, "Research boundary").
//!
//! Exactly **one** ZBD header layout is documented in the committed research
//! pack: the INTERP loading-script container — a little-endian `u32`
//! signature `0x08971119` at offset 0 followed by a little-endian `u32`
//! version `7` at offset 4 (`docs/research/FORMAT-NOTES.md`, "INTERP
//! observed subset" [S07]; the 12-byte header is repeated in
//! `specs/F07-interp-loading-script-container.md`).
//!
//! A family's [`HeaderRule`] is therefore either a documented
//! [`HeaderRule::Signature`] rule or an explicit
//! [`HeaderRule::Undocumented`] record saying that no layout is known. An
//! undocumented rule can never match, so no code path in this crate can
//! pretend it validated bytes it does not understand: dispatch records the
//! probe as unvalidated instead (`crate::zbd::dispatch::HeaderStatus`).

use cs_types::evidence::ClaimStatus;

/// Offset of the documented INTERP signature word inside the header.
pub const INTERP_SIGNATURE_OFFSET: usize = 0;

/// The documented INTERP signature ([S07]).
///
/// `docs/research/FORMAT-NOTES.md` records it as `0x08971119`, read as a
/// little-endian `u32`, so on disk the first four bytes are
/// `0x19 0x11 0x97 0x08`.
pub const INTERP_SIGNATURE: u32 = 0x0897_1119;

/// Offset of the documented INTERP version word (bytes `4..8`).
pub const INTERP_VERSION_OFFSET: usize = 4;

/// The documented INTERP version ([S07]).
pub const INTERP_VERSION: u32 = 7;

/// How one family's container header is recognized, if it is known at all.
///
/// Copyable so an inventory row and a probe result can carry the rule that
/// decided a dispatch without borrowing the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderRule {
    /// No header layout for this family is documented, so its bytes cannot
    /// be validated here. The reason is recorded verbatim (spec F04/F06:
    /// unknown stays unknown; nothing is inferred from the extension).
    Undocumented {
        /// Why this family's bytes cannot be validated yet.
        reason: &'static str,
    },
    /// A documented little-endian `u32` signature and version word at fixed
    /// offsets.
    Signature(SignatureRule),
}

impl HeaderRule {
    /// The documented signature rule, when the family has one.
    pub const fn signature(self) -> Option<SignatureRule> {
        match self {
            Self::Undocumented { .. } => None,
            Self::Signature(rule) => Some(rule),
        }
    }

    /// Why the bytes of this family cannot be validated, when they cannot.
    pub const fn undocumented_reason(self) -> Option<&'static str> {
        match self {
            Self::Undocumented { reason } => Some(reason),
            Self::Signature(_) => None,
        }
    }

    /// Evidence class of the rule: an undocumented rule is `unknown` by
    /// construction, a signature rule carries the class it was recorded
    /// with (`documented`, or a weaker class if a later stage adds a rule
    /// that has not been read from the pinned source).
    pub const fn evidence(self) -> ClaimStatus {
        match self {
            Self::Undocumented { .. } => ClaimStatus::Unknown,
            Self::Signature(rule) => rule.evidence(),
        }
    }
}

/// A documented header layout: a `u32` signature word and a `u32` version
/// word, both little-endian, at fixed offsets.
///
/// The rule never leaves the packed layout: it says which words must be
/// where, nothing about what the rest of the container holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignatureRule {
    offset: usize,
    value: u32,
    version_offset: usize,
    version: u32,
    source: &'static str,
    evidence: ClaimStatus,
}

impl SignatureRule {
    /// Records a documented signature/version rule with its citation.
    pub const fn new(
        offset: usize,
        value: u32,
        version_offset: usize,
        version: u32,
        source: &'static str,
        evidence: ClaimStatus,
    ) -> Self {
        Self {
            offset,
            value,
            version_offset,
            version,
            source,
            evidence,
        }
    }

    /// Byte offset of the signature word.
    pub const fn offset(self) -> usize {
        self.offset
    }

    /// The documented signature word.
    pub const fn value(self) -> u32 {
        self.value
    }

    /// Byte offset of the version word.
    pub const fn version_offset(self) -> usize {
        self.version_offset
    }

    /// The documented version word.
    pub const fn version(self) -> u32 {
        self.version
    }

    /// Where this rule was documented (a `docs/` citation).
    pub const fn source(self) -> &'static str {
        self.source
    }

    /// Evidence class of this rule.
    pub const fn evidence(self) -> ClaimStatus {
        self.evidence
    }

    /// How many probe bytes the rule needs before it can be evaluated: the
    /// furthest of the two words, inclusive.
    pub const fn required_bytes(self) -> usize {
        let furthest = if self.offset > self.version_offset {
            self.offset
        } else {
            self.version_offset
        };
        furthest + 4
    }

    /// Evaluates the rule against a probe: the first bytes of a container.
    ///
    /// A probe shorter than [`Self::required_bytes`] is `TooShort` — the
    /// rule was *not* evaluated, which is different from a mismatch and is
    /// reported as such by dispatch (a role that names this family must
    /// supply enough bytes to check it).
    pub fn probe(self, header: &[u8]) -> HeaderProbe {
        let available = header.len();
        let Some(signature) = le_u32(header, self.offset) else {
            return HeaderProbe::TooShort {
                needed: self.required_bytes(),
                available,
            };
        };
        if signature != self.value {
            return HeaderProbe::Mismatch;
        }
        let Some(version) = le_u32(header, self.version_offset) else {
            return HeaderProbe::TooShort {
                needed: self.required_bytes(),
                available,
            };
        };
        HeaderProbe::Match { version }
    }
}

/// What one [`SignatureRule`] found in a probe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderProbe {
    /// The probe is shorter than the rule's words, so the rule could not be
    /// evaluated at all.
    TooShort {
        /// Bytes the rule needs (`SignatureRule::required_bytes`).
        needed: usize,
        /// Bytes the probe actually has.
        available: usize,
    },
    /// The word at the signature offset is present and is not the
    /// documented signature.
    Mismatch,
    /// The documented signature matched. `version` is the *observed*
    /// version word, which dispatch compares with the documented one.
    Match {
        /// The observed little-endian version word.
        version: u32,
    },
}

/// Reads a little-endian `u32` at `offset`, or `None` when the probe does
/// not hold the four bytes.
fn le_u32(header: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let bytes = header.get(offset..end)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}
