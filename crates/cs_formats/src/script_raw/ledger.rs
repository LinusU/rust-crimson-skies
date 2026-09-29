//! The opcode ledger and the fail-closed program walk (spec F13, stage F13-B;
//! shared contract `docs/contracts/SCRIPT-MISSION.md`).
//!
//! F13-B has to locate the campaign's programs and say what each one is before
//! anyone picks a VM. This module is the second half of that: the typed place a
//! **reachable opcode's meaning** is recorded, and the cursor that refuses to
//! continue past one that is not.
//!
//! * [`OpcodeLedger`] is a checked table of `opcode -> [`OpcodeEntry`]`, each
//!   entry carrying the opcode's spelling, the [`ProgramKind`] it belongs to
//!   and the evidence for the claim. It ships **empty**: the public research
//!   did not establish a Crimson Skies mission opcode table (spec F13 "Research
//!   boundary"), so this stage has nothing to insert. A caller that has measured
//!   a subset (F13-C's isolated probes) builds one, exactly as F07-D's opcode
//!   classification is caller-supplied data rather than embedded code.
//! * [`walk_program`] walks a located program from its entry point. Every
//!   reached program counter is looked up; an opcode the ledger does not name
//!   ends the walk with [`ProgramError::UnknownOpcode`], which carries the
//!   mission and the byte range of the offending word. It never skips an
//!   instruction or treats an unknown one as a no-op (spec F13 non-negotiable
//!   #4, SCRIPT-MISSION "Control flow is explicit and bounded").
//!
//! The instruction *width* is an explicit input of [`walk_program`], not a
//! constant of this module: the observed fleet has not measured the original
//! instruction unit, so a walk states the width it assumed instead of hiding a
//! guess in code. An empty ledger therefore refuses a program at its first
//! reached counter, which is the honest state for the retail corpus today.

use std::collections::BTreeMap;
use std::fmt;

use super::discovery::ProgramKind;
use super::evidence::{ByteSpan, Confidence};

/// The widest opcode word a walk can read: the evidence so far is little-endian
/// `u32` fields, and a wider one would have to be measured first.
pub const MAX_OPCODE_BYTES: u32 = 4;

/// Where a program lives: its container, the member inside it and the byte
/// range. This is the "source location" every failure carries.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProgramLocator {
    container: String,
    member: Option<String>,
    span: ByteSpan,
}

impl ProgramLocator {
    /// A locator for `span` in `container`, optionally inside `member`.
    pub fn new(container: impl Into<String>, member: Option<String>, span: ByteSpan) -> Self {
        Self {
            container: container.into(),
            member,
            span,
        }
    }

    /// The container's provenance label.
    pub fn container(&self) -> &str {
        &self.container
    }

    /// The archive member, when the program is one member of a container.
    pub fn member(&self) -> Option<&str> {
        self.member.as_deref()
    }

    /// The byte range the program occupies.
    pub const fn span(&self) -> ByteSpan {
        self.span
    }
}

impl fmt::Display for ProgramLocator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.member {
            Some(member) => write!(
                f,
                "container `{}` member `{member}` at {}",
                self.container, self.span
            ),
            None => write!(f, "container `{}` at {}", self.container, self.span),
        }
    }
}

/// Why [`OpcodeLedger::insert`] refused an entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LedgerError {
    /// The spelling is empty: an entry must name something.
    EmptySpelling {
        /// The opcode the refused entry named.
        opcode: u32,
    },
    /// The source is empty: a claim with no provenance is not evidence.
    EmptySource {
        /// The opcode the refused entry named.
        opcode: u32,
    },
    /// The confidence is [`Confidence::Unknown`]: an entry that says nothing is
    /// not a claim, and a later stage could not tell it from a guess.
    UnknownConfidence {
        /// The opcode the refused entry named.
        opcode: u32,
    },
    /// Another entry already names this opcode. Opcodes are a flat namespace;
    /// one value with two meanings is exactly the ambiguity F13 has to resolve,
    /// so it is refused rather than overwritten.
    Duplicate {
        /// The opcode both entries named.
        opcode: u32,
        /// The spelling already in the ledger.
        existing: String,
    },
}

impl LedgerError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::EmptySpelling { .. } => "empty_spelling",
            Self::EmptySource { .. } => "empty_source",
            Self::UnknownConfidence { .. } => "unknown_confidence",
            Self::Duplicate { .. } => "duplicate_opcode",
        }
    }
}

impl fmt::Display for LedgerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySpelling { opcode } => {
                write!(f, "opcode {opcode:#x} has an empty spelling")
            }
            Self::EmptySource { opcode } => {
                write!(f, "opcode {opcode:#x} has no source")
            }
            Self::UnknownConfidence { opcode } => write!(
                f,
                "opcode {opcode:#x} carries `unknown` confidence, which is not a claim"
            ),
            Self::Duplicate { opcode, existing } => write!(
                f,
                "opcode {opcode:#x} is already `{existing}`; two meanings for one value is ambiguous"
            ),
        }
    }
}

impl std::error::Error for LedgerError {}

/// One claimed meaning for one opcode value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpcodeEntry {
    opcode: u32,
    spelling: String,
    kind: ProgramKind,
    confidence: Confidence,
    source: String,
}

impl OpcodeEntry {
    /// Validates and records one opcode claim.
    ///
    /// # Errors
    ///
    /// [`LedgerError::EmptySpelling`], [`LedgerError::EmptySource`] and
    /// [`LedgerError::UnknownConfidence`] for a claim that says nothing.
    pub fn new(
        opcode: u32,
        spelling: impl Into<String>,
        kind: ProgramKind,
        confidence: Confidence,
        source: impl Into<String>,
    ) -> Result<Self, LedgerError> {
        let spelling = spelling.into();
        if spelling.trim().is_empty() {
            return Err(LedgerError::EmptySpelling { opcode });
        }
        let source = source.into();
        if source.trim().is_empty() {
            return Err(LedgerError::EmptySource { opcode });
        }
        if confidence == Confidence::Unknown {
            return Err(LedgerError::UnknownConfidence { opcode });
        }
        Ok(Self {
            opcode,
            spelling,
            kind,
            confidence,
            source,
        })
    }

    /// The opcode value.
    pub const fn opcode(&self) -> u32 {
        self.opcode
    }

    /// The claimed spelling.
    pub fn spelling(&self) -> &str {
        &self.spelling
    }

    /// The program family the opcode belongs to.
    pub const fn kind(&self) -> ProgramKind {
        self.kind
    }

    /// How far the claim is supported.
    pub const fn confidence(&self) -> Confidence {
        self.confidence
    }

    /// The provenance of the claim.
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// The checked table of reachable opcodes known so far. Empty by construction:
/// every insertion is a caller's measured claim, never a built-in table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpcodeLedger {
    entries: BTreeMap<u32, OpcodeEntry>,
}

impl OpcodeLedger {
    /// An empty ledger: no opcode is known.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records `entry`, refusing an ambiguous or empty claim.
    ///
    /// # Errors
    ///
    /// The [`LedgerError`] of the first rule the entry breaks.
    pub fn insert(&mut self, entry: OpcodeEntry) -> Result<(), LedgerError> {
        if let Some(existing) = self.entries.get(&entry.opcode) {
            return Err(LedgerError::Duplicate {
                opcode: entry.opcode,
                existing: existing.spelling.clone(),
            });
        }
        self.entries.insert(entry.opcode, entry);
        Ok(())
    }

    /// The entry for `opcode`, when one was recorded.
    pub fn lookup(&self, opcode: u32) -> Option<&OpcodeEntry> {
        self.entries.get(&opcode)
    }

    /// Every entry, in opcode order.
    pub fn entries(&self) -> impl Iterator<Item = &OpcodeEntry> {
        self.entries.values()
    }

    /// Number of known opcodes.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no opcode is known.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// One opcode the walk reached and the ledger named.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReachedOpcode {
    /// Byte range of the opcode word inside the program.
    pub pc: ByteSpan,
    /// The opcode value read there.
    pub opcode: u32,
}

/// Why a program walk stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgramError {
    /// The program has no bytes, so there is no first reached counter.
    EmptyProgram {
        /// The mission the program belongs to.
        mission: String,
        /// Where the program lives.
        locator: ProgramLocator,
    },
    /// A word width outside `1..=MAX_OPCODE_BYTES` was requested.
    InvalidWordWidth {
        /// The width asked for.
        word_bytes: u32,
    },
    /// The opcode at a reached program counter is not in the ledger. This is the
    /// F13-B minimum scenario: the failure carries the mission and the source
    /// location of the offending word.
    UnknownOpcode {
        /// The mission the program belongs to.
        mission: String,
        /// Where the program lives.
        locator: ProgramLocator,
        /// Byte range of the unknown word: the reached program counter.
        pc: ByteSpan,
        /// The opcode value read there.
        opcode: u32,
    },
    /// The program ends with fewer bytes than one opcode word.
    TruncatedOpcode {
        /// The mission the program belongs to.
        mission: String,
        /// Where the program lives.
        locator: ProgramLocator,
        /// Byte range of the trailing bytes.
        pc: ByteSpan,
        /// How many bytes are left.
        remaining: u64,
    },
    /// The walk reached its instruction budget, so the program is unbounded.
    BudgetExceeded {
        /// The mission the program belongs to.
        mission: String,
        /// Where the program lives.
        locator: ProgramLocator,
        /// The budget the walk was given.
        budget: u32,
    },
}

impl ProgramError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::EmptyProgram { .. } => "empty_program",
            Self::InvalidWordWidth { .. } => "invalid_word_width",
            Self::UnknownOpcode { .. } => "unknown_opcode",
            Self::TruncatedOpcode { .. } => "truncated_opcode",
            Self::BudgetExceeded { .. } => "budget_exceeded",
        }
    }

    /// The mission the failure is about, when it has one.
    pub fn mission(&self) -> Option<&str> {
        match self {
            Self::EmptyProgram { mission, .. }
            | Self::UnknownOpcode { mission, .. }
            | Self::TruncatedOpcode { mission, .. }
            | Self::BudgetExceeded { mission, .. } => Some(mission),
            Self::InvalidWordWidth { .. } => None,
        }
    }

    /// Where the failure is, when it has one.
    pub fn locator(&self) -> Option<&ProgramLocator> {
        match self {
            Self::EmptyProgram { locator, .. }
            | Self::UnknownOpcode { locator, .. }
            | Self::TruncatedOpcode { locator, .. }
            | Self::BudgetExceeded { locator, .. } => Some(locator),
            Self::InvalidWordWidth { .. } => None,
        }
    }
}

impl fmt::Display for ProgramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyProgram { mission, locator } => {
                write!(f, "mission `{mission}`: {locator} holds no program")
            }
            Self::InvalidWordWidth { word_bytes } => write!(
                f,
                "an opcode word of {word_bytes} bytes is not in 1..={MAX_OPCODE_BYTES}"
            ),
            Self::UnknownOpcode {
                mission,
                locator,
                pc,
                opcode,
            } => write!(
                f,
                "mission `{mission}`: unknown opcode {opcode:#x} at program counter {pc} in {locator}"
            ),
            Self::TruncatedOpcode {
                mission,
                locator,
                pc,
                remaining,
            } => write!(
                f,
                "mission `{mission}`: {remaining} trailing bytes at {pc} in {locator} are not a \
                 whole opcode"
            ),
            Self::BudgetExceeded {
                mission,
                locator,
                budget,
            } => write!(
                f,
                "mission `{mission}`: {locator} exceeds the {budget}-instruction budget"
            ),
        }
    }
}

impl std::error::Error for ProgramError {}

/// Reads one little-endian opcode word of `word_bytes` at `offset`.
fn read_opcode(bytes: &[u8], offset: usize, word_bytes: u32) -> u32 {
    let mut value = 0u32;
    for index in 0..word_bytes as usize {
        value |= u32::from(bytes[offset + index]) << (8 * index);
    }
    value
}

/// Walks `bytes` from its entry point, one opcode word at a time, looking every
/// reached opcode up in `ledger`.
///
/// The first opcode the ledger does not name ends the walk with
/// [`ProgramError::UnknownOpcode`] carrying `mission` and the program's
/// [`ProgramLocator`] (spec F13 AC02, non-negotiable #4). Nothing is skipped and
/// no unknown opcode is treated as a no-op.
///
/// `word_bytes` is the assumed instruction unit; it is an input because F13 has
/// not measured the original unit. `budget` caps how many instructions the walk
/// will decode, so a ledger that knows every value cannot make it run forever.
///
/// # Errors
///
/// [`ProgramError::InvalidWordWidth`] for a width outside `1..=MAX_OPCODE_BYTES`,
/// [`ProgramError::EmptyProgram`] for empty bytes,
/// [`ProgramError::UnknownOpcode`] for the first unnamed opcode,
/// [`ProgramError::TruncatedOpcode`] for trailing bytes that are not a whole
/// word and [`ProgramError::BudgetExceeded`] when `budget` instructions are
/// decoded.
pub fn walk_program(
    mission: &str,
    locator: &ProgramLocator,
    bytes: &[u8],
    word_bytes: u32,
    ledger: &OpcodeLedger,
    budget: u32,
) -> Result<Vec<ReachedOpcode>, ProgramError> {
    if word_bytes == 0 || word_bytes > MAX_OPCODE_BYTES {
        return Err(ProgramError::InvalidWordWidth { word_bytes });
    }
    if bytes.is_empty() {
        return Err(ProgramError::EmptyProgram {
            mission: mission.to_owned(),
            locator: locator.clone(),
        });
    }

    let width = word_bytes as usize;
    let mut reached = Vec::new();
    let mut offset = 0usize;
    loop {
        let remaining = bytes.len() - offset;
        if remaining == 0 {
            return Ok(reached);
        }
        if remaining < width {
            return Err(ProgramError::TruncatedOpcode {
                mission: mission.to_owned(),
                locator: locator.clone(),
                pc: ByteSpan::new(offset as u64, remaining as u64),
                remaining: remaining as u64,
            });
        }
        let opcode = read_opcode(bytes, offset, word_bytes);
        if ledger.lookup(opcode).is_none() {
            return Err(ProgramError::UnknownOpcode {
                mission: mission.to_owned(),
                locator: locator.clone(),
                pc: ByteSpan::new(offset as u64, word_bytes as u64),
                opcode,
            });
        }
        reached.push(ReachedOpcode {
            pc: ByteSpan::new(offset as u64, word_bytes as u64),
            opcode,
        });
        offset += width;
        if reached.len() as u32 >= budget {
            if offset < bytes.len() {
                return Err(ProgramError::BudgetExceeded {
                    mission: mission.to_owned(),
                    locator: locator.clone(),
                    budget,
                });
            }
            return Ok(reached);
        }
    }
}
