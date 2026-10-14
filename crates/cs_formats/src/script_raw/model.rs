//! The real model of each located program family (spec F13, stage F13-D,
//! task #640; shared contract `docs/contracts/SCRIPT-MISSION.md`).
//!
//! F13-B's probe walked **every** located program as a stream of numeric
//! opcode words and stopped each one at its first counter with
//! `unknown_opcode`. The owner's static analysis of the supplied decrypted
//! executable (`crimson.decrypted.exe`, sha256 `43540fc9…6c37d75`, owner note
//! on #640 dated 2026-10-05) established that **none of the three families is
//! an opcode stream**:
//!
//! * mission reader members are `.zrd` typed trees (tags 1..=4, a list holds
//!   its count minus one child) opened at VA `0x579c60` and read **by
//!   keyword** — data plus a native interpreter;
//! * `interp.zbd` script bodies are a **named text-command language**: each
//!   line is a NUL-separated argument vector whose first token is the command
//!   name, matched by string compare in `zinterp_parse.cpp` (VA `0x5b845c`);
//! * the compiled animation ZBD payload (magic `0x08170616`, version `0x35`)
//!   is a stream of **typed event records**, run over time by the animation
//!   runtime and loaded by VA `0x521b80`; F20 (#690) then measured that
//!   stream's record layout and opcode census over all 61 retail carriers
//!   and joined **31 of the 35 stored event-type ids** to the installation's
//!   own event declarations, leaving four (13, 17, 26, 28) unjoined
//!   (`docs/findings/2026-10-06-f20-event-grammar.md`). Those four ids and
//!   the reconciliation of the owner's 2026-10-05 keyword list with F20's
//!   table are follow-ups #1415/#1416, not a claim this module makes.
//!
//! [`ProgramModel`] is that classification, and
//! [`crate::script_raw::probe::ProbeSession::record_modelled_program`] files a
//! program with its model instead of walking it, so a `.zrd` body or an INTERP
//! command script is never presented as an unknown opcode again. A program
//! whose family's model the evidence does **not** establish keeps the binary
//! walk exactly as before: this module classifies, it never guesses
//! (spec F13 non-negotiable #1, "Unknown means unknown").
//!
//! Every fixture exercised by `crates/cs_formats/tests/script_raw/` is newly
//! authored synthetic bytes; nothing here is derived from original game data.

use std::fmt;

use super::discovery::ProgramKind;
use super::evidence::{Confidence, EvidenceLocator, ResearchMethod, ScriptEvidence};
use super::ledger::ProgramLocator;
use crate::interp::InterpRawLine;

/// The module name the executable-static-analysis evidence points at: the
/// owner-supplied decrypted game executable the #640 model addresses live in.
pub const ANALYSIS_MODULE: &str = "crimson.decrypted.exe";

/// Relative virtual address of the `.zrd` tree opener (owner note #640: VA
/// `0x579c60`, `zrdr_main.cpp`; the image base is `0x400000`).
pub const TYPED_TREE_RVA: u32 = 0x0017_9c60;

/// Relative virtual address of the named-command interpreter (owner note
/// #640: VA `0x5b845c`, `zinterp_parse.cpp`).
pub const NAMED_COMMANDS_RVA: u32 = 0x001b_845c;

/// Relative virtual address of the compiled animation ZBD loader (owner note
/// #640: VA `0x521b80`, reached from `0x522ef0`).
pub const EVENT_RECORDS_RVA: u32 = 0x0012_1b80;

/// Which real model a located program family follows.
///
/// This is stronger than [`ProgramKind`]: the kind is the program's *role*
/// (a name or a path rule), while the model says how the bytes are read at
/// all. Only a model backed by evidence that can establish a meaning is
/// recorded; anything else stays on the binary opcode walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProgramModel {
    /// A `.zrd` typed tree: int/float/text/list nodes read **by keyword** by
    /// a native consumer. No instruction stream exists here.
    TypedDataTree,
    /// A named text-command program: every line is a NUL-separated argument
    /// vector whose head token is the command name the interpreter matches by
    /// string compare. No numeric opcode exists here.
    NamedCommands,
    /// A stream of typed animation event records with a closed event
    /// vocabulary, stepped over time by the animation runtime.
    EventRecords,
}

impl ProgramModel {
    /// Every model, in report order.
    pub const ALL: [ProgramModel; 3] =
        [Self::TypedDataTree, Self::NamedCommands, Self::EventRecords];

    /// Stable lowercase label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::TypedDataTree => "typed_data_tree",
            Self::NamedCommands => "named_commands",
            Self::EventRecords => "event_records",
        }
    }

    /// The model a [`Self::label`] names, when the label is one of theirs.
    ///
    /// The inverse of [`Self::label`]: an unrecognised label is `None`, never
    /// silently a model, so a typo in caller-supplied data is refused rather
    /// than filed under a family.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|model| model.label() == label)
    }

    /// The relative virtual address of the original routine the owner's note
    /// ties this model to.
    pub const fn basis_rva(self) -> u32 {
        match self {
            Self::TypedDataTree => TYPED_TREE_RVA,
            Self::NamedCommands => NAMED_COMMANDS_RVA,
            Self::EventRecords => EVENT_RECORDS_RVA,
        }
    }
}

/// The static-analysis evidence behind each model: the owner-accepted note on
/// #640 (2026-10-05), at most [`super::evidence::MAX_NOTE_BYTES`] of summary,
/// pointing at the executable address — never at copied bytes or lifted code
/// (spec F13 non-negotiable #2).
///
/// # Panics
///
/// Only on programmer error: each note is a fixed short summary.
#[must_use]
pub fn model_basis(model: ProgramModel) -> ScriptEvidence {
    let note = match model {
        ProgramModel::TypedDataTree => {
            "owner note #640: mission reader members are .zrd typed trees (tags 1..=4, list \
             count-1) opened at VA 0x579c60 and read by keyword; not an opcode stream"
        }
        ProgramModel::NamedCommands => {
            "owner note #640: interp.zbd bodies are NUL-separated text command vectors matched \
             by name in zinterp_parse.cpp at VA 0x5b845c; not an opcode stream"
        }
        ProgramModel::EventRecords => {
            "owner note #640: the compiled animation ZBD payload is typed event records loaded \
             at VA 0x521b80; their event-type ids are F20's measured event-stream table (#690, \
             31 of 35 joined to the installation's own declarations); 4 ids stay unjoined"
        }
    };
    ScriptEvidence::new(
        ResearchMethod::ExecutableStaticAnalysis,
        Confidence::Documented,
        EvidenceLocator::Executable {
            module: ANALYSIS_MODULE.to_owned(),
            rva: model.basis_rva(),
        },
        note,
    )
    .expect("a fixed short basis note is valid evidence")
}

/// Why a body does not read as a named-command program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedCommandsError {
    /// The body holds no line at all: there is no command to read.
    EmptyBody,
    /// A line's first argument is empty, so no command name heads it.
    EmptyHead {
        /// Zero-based index of the line.
        line: usize,
        /// Absolute offset of the line's `size` word, when the caller's lines
        /// carry one; `0` for a body whose offset is not known.
        offset: u64,
    },
    /// A line's first argument holds a byte that no text command name can
    /// contain (anything outside printable ASCII).
    HeadNotText {
        /// Zero-based index of the line.
        line: usize,
        /// Absolute offset of the line's `size` word.
        offset: u64,
    },
}

impl NamedCommandsError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::EmptyBody => "empty_body",
            Self::EmptyHead { .. } => "empty_head",
            Self::HeadNotText { .. } => "head_not_text",
        }
    }
}

impl fmt::Display for NamedCommandsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyBody => f.write_str("the body holds no command line"),
            Self::EmptyHead { line, offset } => {
                write!(f, "line {line} at 0x{offset:x} has no command name")
            }
            Self::HeadNotText { line, offset } => write!(
                f,
                "line {line} at 0x{offset:x} starts with bytes no text command name can hold"
            ),
        }
    }
}

impl std::error::Error for NamedCommandsError {}

/// Checks that `lines` read as a named-command program: every line's first
/// argument is a non-empty run of printable ASCII — the command name a
/// name-matching interpreter looks up — and there is at least one line.
///
/// The line layout (`size`, `argument_count`, NUL-separated data) is the one
/// the F07 INTERP reader validates; this check adds the *text* shape of the
/// command head and nothing else. Arguments are not checked: their contents
/// are values, and refusing a value shape here would claim a format rule the
/// evidence does not state.
///
/// Returns the number of lines, or the first refusal.
///
/// # Errors
///
/// [`NamedCommandsError`] naming the first line that does not read as a
/// command.
pub fn reads_as_named_commands(lines: &[InterpRawLine<'_>]) -> Result<usize, NamedCommandsError> {
    if lines.is_empty() {
        return Err(NamedCommandsError::EmptyBody);
    }
    for (index, line) in lines.iter().enumerate() {
        let Some(head) = line.raw_arguments().next() else {
            return Err(NamedCommandsError::EmptyHead {
                line: index,
                offset: line.offset,
            });
        };
        if head.bytes.is_empty() {
            return Err(NamedCommandsError::EmptyHead {
                line: index,
                offset: line.offset,
            });
        }
        if !head.bytes.iter().all(u8::is_ascii_graphic) {
            return Err(NamedCommandsError::HeadNotText {
                line: index,
                offset: line.offset,
            });
        }
    }
    Ok(lines.len())
}

/// One program classified with its family's real model instead of being
/// walked as a stream of numeric opcodes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelledProgramProbe {
    mission: String,
    locator: ProgramLocator,
    kind: ProgramKind,
    model: ProgramModel,
    evidence: ScriptEvidence,
}

impl ModelledProgramProbe {
    /// Files `program` under `model` with the evidence that classifies it.
    pub(crate) fn file(
        mission: String,
        locator: ProgramLocator,
        kind: ProgramKind,
        model: ProgramModel,
        evidence: ScriptEvidence,
    ) -> Self {
        Self {
            mission,
            locator,
            kind,
            model,
            evidence,
        }
    }

    /// The scope the program was filed in.
    pub fn mission(&self) -> &str {
        &self.mission
    }

    /// Where the program lives.
    pub const fn locator(&self) -> &ProgramLocator {
        &self.locator
    }

    /// The role a name or a path supports.
    pub const fn kind(&self) -> ProgramKind {
        self.kind
    }

    /// The real model the evidence established.
    pub const fn model(&self) -> ProgramModel {
        self.model
    }

    /// The evidence behind the classification (a structural decode of the
    /// program's own bytes, or the executable address the model comes from).
    pub const fn evidence(&self) -> &ScriptEvidence {
        &self.evidence
    }

    /// How far the classification is supported.
    pub const fn confidence(&self) -> Confidence {
        self.evidence.confidence()
    }
}
