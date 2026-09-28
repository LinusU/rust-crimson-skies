//! The disassembly-neutral evidence schema of the script inventory (spec F13,
//! stage F13-A; non-negotiable #1 and #2).
//!
//! A claim about a script record — "these bytes are a mission program",
//! "this word is an opcode", "this record names an animation" — carries one
//! [`ScriptEvidence`]: the [`ResearchMethod`] that produced it, a
//! [`Confidence`], an [`EvidenceLocator`] and a short note. The schema is
//! *neutral* about how the research was done: a disassembly of the owner's
//! executable, a runtime capture of the original and a structural decode of
//! a container are all recorded the same way — as a method, a place and a
//! summary. It has no field that could hold lifted code, instruction bytes
//! or a disassembly listing, and the note is capped at
//! [`MAX_NOTE_BYTES`], so the project's evidence cannot become a copy of the
//! original program (non-negotiable #2).
//!
//! The rules [`ScriptEvidence::new`] enforces:
//!
//! * a header/string scan ([`ResearchMethod::ContainerScan`]) yields at most
//!   a [`Confidence::Lead`] — "a scan is not a decoder";
//! * each method names its own kind of locator (a container span, an
//!   executable address, a capture, a document);
//! * a document review is at most [`Confidence::Documented`];
//! * there is no `verified_original` confidence at all: it is never
//!   self-awarded by an inventory (AGENTS rule 8).

use std::fmt;

/// Maximum bytes in an evidence note: a summary, never a listing.
pub const MAX_NOTE_BYTES: usize = 280;

/// A half-open byte range `offset..offset + len` inside one container.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByteSpan {
    /// Absolute offset of the first byte.
    pub offset: u64,
    /// Number of bytes.
    pub len: u64,
}

impl ByteSpan {
    /// The span `offset..offset + len`.
    pub const fn new(offset: u64, len: u64) -> Self {
        Self { offset, len }
    }

    /// The span `start..end`; `None` when `end < start`.
    pub const fn from_range(start: u64, end: u64) -> Option<Self> {
        if end < start {
            None
        } else {
            Some(Self::new(start, end - start))
        }
    }

    /// Offset just past the last byte, saturating at `u64::MAX`.
    pub const fn end(&self) -> u64 {
        self.offset.saturating_add(self.len)
    }
}

impl fmt::Display for ByteSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:x}..0x{:x}", self.offset, self.end())
    }
}

/// How a piece of script evidence was obtained.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResearchMethod {
    /// A header or printable-string scan over container bytes. It finds
    /// leads only and never establishes a meaning.
    ContainerScan,
    /// A validating reader of this project decoded the structure (F07's
    /// INTERP reader, F06's member index, …).
    StructuralDecode,
    /// A cited research or design document.
    DocumentReview,
    /// Static analysis of the owner's original executable, documented
    /// separately as a research method. Only an address and a summary are
    /// recorded here.
    ExecutableStaticAnalysis,
    /// An observation of an owner-supplied original run.
    OriginalRuntimeObservation,
}

impl ResearchMethod {
    /// Stable lowercase label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::ContainerScan => "container_scan",
            Self::StructuralDecode => "structural_decode",
            Self::DocumentReview => "document_review",
            Self::ExecutableStaticAnalysis => "executable_static_analysis",
            Self::OriginalRuntimeObservation => "original_runtime_observation",
        }
    }

    /// Whether evidence from this method can establish what a record *means*
    /// (for example that it is an instruction stream). A scan cannot.
    pub const fn can_establish_semantics(self) -> bool {
        !matches!(self, Self::ContainerScan)
    }

    /// The locator kind evidence from this method must carry.
    pub const fn locator_kind(self) -> LocatorKind {
        match self {
            Self::ContainerScan | Self::StructuralDecode => LocatorKind::ContainerSpan,
            Self::DocumentReview => LocatorKind::Document,
            Self::ExecutableStaticAnalysis => LocatorKind::Executable,
            Self::OriginalRuntimeObservation => LocatorKind::RuntimeCapture,
        }
    }
}

/// How far a piece of evidence supports its claim, weakest first.
///
/// Deliberately has no `verified_original` level: an inventory never awards
/// that status (AGENTS rule 8, spec F13 "Evidence and completion").
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Confidence {
    /// Explicitly not known.
    Unknown,
    /// A scan hit worth following; no meaning is established.
    Lead,
    /// Reasoned from other evidence (a naming convention, a neighbour).
    Inferred,
    /// Stated in a cited source.
    Documented,
    /// Observed through a tool or probe run; not original-verified.
    ObservedTool,
}

impl Confidence {
    /// Stable lowercase label for reports; `inferred`, `documented`,
    /// `observed_tool` and `unknown` match the F01 claim-status vocabulary.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Lead => "lead",
            Self::Inferred => "inferred",
            Self::Documented => "documented",
            Self::ObservedTool => "observed_tool",
        }
    }
}

/// The kind of place an [`EvidenceLocator`] points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LocatorKind {
    /// A byte range in a data container.
    ContainerSpan,
    /// An address in an executable module.
    Executable,
    /// A position in a recorded original run.
    RuntimeCapture,
    /// A cited document.
    Document,
}

impl LocatorKind {
    /// Stable lowercase label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::ContainerSpan => "container_span",
            Self::Executable => "executable",
            Self::RuntimeCapture => "runtime_capture",
            Self::Document => "document",
        }
    }
}

/// Where the evidence lives. Every variant points; none carries content.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EvidenceLocator {
    /// A byte range of a container (its provenance label and span).
    ContainerSpan {
        /// Container provenance label.
        container: String,
        /// Where in the container.
        span: ByteSpan,
    },
    /// An address inside an executable module of the owner's installation.
    Executable {
        /// Module name as installed (for example the game executable).
        module: String,
        /// Relative virtual address the observation is about.
        rva: u32,
    },
    /// A position in an owner-supplied original capture.
    RuntimeCapture {
        /// Capture identifier (for example a reference-capture id).
        capture: String,
        /// Simulation tick or frame, when the capture has one.
        tick: Option<u64>,
    },
    /// A cited document (`S07`, a `docs/findings/` note, a spec section).
    Document {
        /// The citation.
        citation: String,
    },
}

impl EvidenceLocator {
    /// The kind of place this locator points at.
    pub const fn kind(&self) -> LocatorKind {
        match self {
            Self::ContainerSpan { .. } => LocatorKind::ContainerSpan,
            Self::Executable { .. } => LocatorKind::Executable,
            Self::RuntimeCapture { .. } => LocatorKind::RuntimeCapture,
            Self::Document { .. } => LocatorKind::Document,
        }
    }
}

/// Why a [`ScriptEvidence`] was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvidenceError {
    /// The note is empty: evidence must say what was observed.
    EmptyNote,
    /// The note exceeds [`MAX_NOTE_BYTES`]; evidence summarizes, it does not
    /// carry listings or copied program text.
    NoteTooLong {
        /// Bytes in the refused note.
        len: usize,
    },
    /// The method cannot support the requested confidence (a scan above
    /// [`Confidence::Lead`], a document above [`Confidence::Documented`]).
    Overclaim {
        /// The method.
        method: ResearchMethod,
        /// The confidence asked for.
        requested: Confidence,
        /// The highest confidence the method supports.
        ceiling: Confidence,
    },
    /// The locator is not the kind the method records.
    LocatorMismatch {
        /// The method.
        method: ResearchMethod,
        /// The locator kind the method requires.
        expected: LocatorKind,
        /// The locator kind given.
        found: LocatorKind,
    },
}

impl EvidenceError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::EmptyNote => "empty_note",
            Self::NoteTooLong { .. } => "note_too_long",
            Self::Overclaim { .. } => "overclaim",
            Self::LocatorMismatch { .. } => "locator_mismatch",
        }
    }
}

impl fmt::Display for EvidenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyNote => f.write_str("script evidence needs a non-empty note"),
            Self::NoteTooLong { len } => write!(
                f,
                "script evidence note has {len} bytes, at most {MAX_NOTE_BYTES} are allowed"
            ),
            Self::Overclaim {
                method,
                requested,
                ceiling,
            } => write!(
                f,
                "{} evidence supports at most `{}`, not `{}`",
                method.label(),
                ceiling.label(),
                requested.label()
            ),
            Self::LocatorMismatch {
                method,
                expected,
                found,
            } => write!(
                f,
                "{} evidence needs a {} locator, got {}",
                method.label(),
                expected.label(),
                found.label()
            ),
        }
    }
}

impl std::error::Error for EvidenceError {}

/// One validated piece of evidence about a script record.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ScriptEvidence {
    method: ResearchMethod,
    confidence: Confidence,
    locator: EvidenceLocator,
    note: String,
}

impl ScriptEvidence {
    /// Validates and records one piece of evidence.
    ///
    /// # Errors
    ///
    /// [`EvidenceError::EmptyNote`] / [`EvidenceError::NoteTooLong`] for a
    /// note that is missing or is not a summary,
    /// [`EvidenceError::Overclaim`] when the method cannot support the
    /// confidence and [`EvidenceError::LocatorMismatch`] when the locator is
    /// not the method's kind.
    pub fn new(
        method: ResearchMethod,
        confidence: Confidence,
        locator: EvidenceLocator,
        note: impl Into<String>,
    ) -> Result<Self, EvidenceError> {
        let note = note.into();
        if note.trim().is_empty() {
            return Err(EvidenceError::EmptyNote);
        }
        if note.len() > MAX_NOTE_BYTES {
            return Err(EvidenceError::NoteTooLong { len: note.len() });
        }
        let ceiling = match method {
            ResearchMethod::ContainerScan => Confidence::Lead,
            ResearchMethod::DocumentReview => Confidence::Documented,
            ResearchMethod::StructuralDecode
            | ResearchMethod::ExecutableStaticAnalysis
            | ResearchMethod::OriginalRuntimeObservation => Confidence::ObservedTool,
        };
        if confidence > ceiling {
            return Err(EvidenceError::Overclaim {
                method,
                requested: confidence,
                ceiling,
            });
        }
        let expected = method.locator_kind();
        if locator.kind() != expected {
            return Err(EvidenceError::LocatorMismatch {
                method,
                expected,
                found: locator.kind(),
            });
        }
        Ok(Self {
            method,
            confidence,
            locator,
            note,
        })
    }

    /// How the evidence was obtained.
    pub const fn method(&self) -> ResearchMethod {
        self.method
    }

    /// How far it supports its claim.
    pub const fn confidence(&self) -> Confidence {
        self.confidence
    }

    /// Where it lives.
    pub const fn locator(&self) -> &EvidenceLocator {
        &self.locator
    }

    /// The summary.
    pub fn note(&self) -> &str {
        &self.note
    }

    /// Whether this evidence can establish a record's meaning: a method
    /// other than a scan, at [`Confidence::Documented`] or above. An
    /// inference or a lead names a hypothesis, not a meaning.
    pub fn establishes_semantics(&self) -> bool {
        self.method.can_establish_semantics() && self.confidence >= Confidence::Documented
    }
}
