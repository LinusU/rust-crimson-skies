//! Canonical claim and evidence records for the evidence ledger (F01-A).
//!
//! Every factual compatibility claim is a [`ClaimRecord`]: a [`ClaimId`], a
//! [`ClaimStatus`] and the [`EvidenceRecord`]s that back it. Each evidence
//! record references a source, an exact revision or game fingerprint, an
//! observation locator, an observation method and its limitations. Automated
//! test success travels in a separate [`TestOutcome`] field and never doubles
//! as an evidence status (spec F01, "Deliverable and interfaces").
//!
//! These are the records plus their per-record admission rules only.
//! Set-level ledger validation and fingerprint invalidation are F01-B; wiring
//! them into `cs-inspect` commands is F01-C. Nothing in this module is
//! derived from original game data.

use std::fmt;

/// Maximum byte length of a [`ClaimId`].
pub const MAX_CLAIM_ID_LEN: usize = 128;

/// Stable identifier of one factual compatibility claim.
///
/// The grammar is ASCII alphanumerics plus `.`, `_`, `-`, `:` and `/`, so ids
/// such as `f05.rof.header-magic` or `m01/objective.spawn` stay readable in
/// ledger files and diagnostics.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClaimId(String);

/// Why a [`ClaimId`] string was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaimIdError {
    /// The id was empty.
    Empty,
    /// The id exceeded [`MAX_CLAIM_ID_LEN`] bytes.
    TooLong { len: usize },
    /// The id contained a character outside the allowed grammar.
    BadCharacter { ch: char },
}

impl fmt::Display for ClaimIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "claim id must not be empty"),
            Self::TooLong { len } => {
                write!(f, "claim id is {len} bytes, max is {MAX_CLAIM_ID_LEN}")
            }
            Self::BadCharacter { ch } => {
                write!(f, "claim id contains disallowed character {ch:?}")
            }
        }
    }
}

impl std::error::Error for ClaimIdError {}

impl ClaimId {
    /// Validates and wraps a claim id string.
    pub fn new(id: &str) -> Result<Self, ClaimIdError> {
        if id.is_empty() {
            return Err(ClaimIdError::Empty);
        }
        if id.len() > MAX_CLAIM_ID_LEN {
            return Err(ClaimIdError::TooLong { len: id.len() });
        }
        for ch in id.chars() {
            if !ch.is_ascii_alphanumeric() && !matches!(ch, '.' | '_' | '-' | ':' | '/') {
                return Err(ClaimIdError::BadCharacter { ch });
            }
        }
        Ok(Self(id.to_owned()))
    }

    /// The id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ClaimId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A SHA-256 digest. The text form is exactly 64 lowercase hex characters,
/// matching the `sha256` patterns in `schemas/evidence.schema.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ContentHash([u8; 32]);

/// Why a [`ContentHash`] hex string was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HashError {
    /// The hex string was not exactly 64 characters.
    BadLength { len: usize },
    /// A character was not a lowercase hex digit.
    BadCharacter { index: usize, ch: char },
}

impl fmt::Display for HashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadLength { len } => {
                write!(f, "sha256 hex must be 64 characters, got {len}")
            }
            Self::BadCharacter { index, ch } => {
                write!(
                    f,
                    "sha256 hex has invalid character {ch:?} at index {index}"
                )
            }
        }
    }
}

impl std::error::Error for HashError {}

fn hex_digit(byte: u8, index: usize) -> Result<u8, HashError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(HashError::BadCharacter {
            index,
            ch: byte as char,
        }),
    }
}

impl ContentHash {
    /// Wraps an already-decoded digest.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The digest as raw bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Decodes a 64-character lowercase hex string.
    ///
    /// Uppercase input is rejected: the evidence schema pattern is
    /// `^[0-9a-f]{64}$`, so accepting it here would produce records the
    /// schema refuses.
    pub fn from_hex(hex: &str) -> Result<Self, HashError> {
        if hex.len() != 64 {
            return Err(HashError::BadLength { len: hex.len() });
        }
        let mut bytes = [0u8; 32];
        let (pairs, _remainder) = hex.as_bytes().as_chunks::<2>();
        for (i, pair) in pairs.iter().enumerate() {
            let hi = hex_digit(pair[0], i * 2)?;
            let lo = hex_digit(pair[1], i * 2 + 1)?;
            bytes[i] = (hi << 4) | lo;
        }
        Ok(Self(bytes))
    }

    /// The 64-character lowercase hex form.
    pub fn to_hex(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// What a fingerprint identifies, using the vocabulary of
/// `schemas/evidence.schema.json` (`install_sha256`, `content_sha256`) plus
/// produced artifacts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FingerprintKind {
    /// The owner's original installation (an aggregate or one member).
    Installation,
    /// Canonical game content extracted or normalized from the installation.
    Content,
    /// A produced artifact: tool output, synthetic fixture, report. Never
    /// original data.
    Artifact,
}

/// An exact revision or game fingerprint: which thing was hashed and its
/// digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    pub kind: FingerprintKind,
    pub sha256: ContentHash,
}

impl Fingerprint {
    /// Whether the fingerprint identifies original game data. Only
    /// installation and content fingerprints can back `verified_original`.
    pub const fn identifies_original(self) -> bool {
        matches!(
            self.kind,
            FingerprintKind::Installation | FingerprintKind::Content
        )
    }
}

/// A byte span inside a source container.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceSpan {
    pub offset: u64,
    pub length: u64,
}

/// Where inside a source an observation was made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservationLocator {
    /// The container the observation lives in: an archive path, a file
    /// member, or a document anchor such as `doc:CLI-EVIDENCE#record-minimum`.
    pub container: String,
    /// The byte span inside the container, when the observation is one.
    pub span: Option<SourceSpan>,
}

/// What an observation was taken from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvidenceSource {
    /// A cited research or design document by stable id or path (for example
    /// `S01` or a `docs/findings/` note).
    Document(String),
    /// Data inside the owner's original installation, opened read-only.
    OriginalInstallation,
    /// A run of an inspection, probe or reference tool.
    ToolRun { tool: String, version: String },
    /// Newly authored synthetic fixture content. It can never support
    /// `verified_original`: synthetic fixtures do not prove retail behavior.
    SyntheticFixture,
}

/// How an observation was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationMethod {
    /// Read from a cited document.
    DocumentReview,
    /// Inspected stored bytes directly.
    ByteInspection,
    /// Ran a tool or probe against the data.
    ToolProbe,
    /// Observed a running program's behavior.
    RuntimeObservation,
    /// Reasoned from other evidence.
    Inference,
    /// Newly authored by this project.
    Authored,
}

/// One piece of evidence backing (or refuting) a claim.
///
/// `fingerprint` and `locator` are optional on the record because some
/// statuses cite documents or authored work that has neither; the claim-level
/// rules decide which statuses may stand on which evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceRecord {
    /// What the observation was taken from.
    pub source: EvidenceSource,
    /// The exact revision or game fingerprint observed.
    pub fingerprint: Option<Fingerprint>,
    /// Where inside the source the observation lives.
    pub locator: Option<ObservationLocator>,
    /// How the observation was made.
    pub method: ObservationMethod,
    /// Recorded limitations of this evidence.
    pub limitations: Vec<String>,
}

impl EvidenceRecord {
    /// Whether this record can support `verified_original`: it fingerprints
    /// original installation or content data and locates the observation.
    /// Synthetic fixture content can never verify originality, regardless of
    /// the fingerprint kind its recorder attached.
    pub fn verifies_original(&self) -> bool {
        !matches!(self.source, EvidenceSource::SyntheticFixture)
            && self
                .fingerprint
                .is_some_and(Fingerprint::identifies_original)
            && self.locator.is_some()
    }
}

/// The epistemic status of a factual compatibility claim.
///
/// Automated test success is **not** a status; it lives in
/// [`ClaimRecord::test_outcome`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimStatus {
    /// Stated in a cited source document.
    Documented,
    /// Observed through a tool run; not original-verified.
    ObservedTool,
    /// Backed by fingerprinted original-data evidence.
    VerifiedOriginal,
    /// Reasoned from evidence; not directly observed.
    Inferred,
    /// Newly authored engineering design.
    Designed,
    /// Explicitly not known.
    Unknown,
    /// Sources or evidence disagree. The claim is kept, never silently merged
    /// into the convenient source.
    Contradicted,
}

impl ClaimStatus {
    /// The spec-vocabulary name of the status.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Documented => "documented",
            Self::ObservedTool => "observed_tool",
            Self::VerifiedOriginal => "verified_original",
            Self::Inferred => "inferred",
            Self::Designed => "designed",
            Self::Unknown => "unknown",
            Self::Contradicted => "contradicted",
        }
    }
}

impl fmt::Display for ClaimStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// An automated test outcome attached to a claim.
///
/// Informational only: a passing test never upgrades an evidence status, and
/// a failing one never lowers it (spec F01, non-negotiable behavior 5 and the
/// CLI-EVIDENCE contract's "test success is a separate field").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestOutcome {
    /// The test selector that ran (a name or a task prefix).
    pub selector: String,
    /// Tests that passed under that selector.
    pub passed: u32,
    /// Tests that failed under that selector.
    pub failed: u32,
}

/// One factual compatibility claim and everything that backs or contests it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimRecord {
    pub id: ClaimId,
    /// The factual statement under claim.
    pub subject: String,
    pub status: ClaimStatus,
    /// The evidence backing the claim.
    pub evidence: Vec<EvidenceRecord>,
    /// Automated test outcome; never an evidence status.
    pub test_outcome: Option<TestOutcome>,
    /// Other claims this record disagrees with. Contradictions are preserved,
    /// never resolved by picking the convenient source.
    pub disputes: Vec<ClaimId>,
    /// Free-text adjudication note for a contradiction. The typed
    /// adjudication state machine is wired by F01-C.
    pub adjudication: Option<String>,
}

/// Why a [`ClaimRecord`] failed the per-record admission rules.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaimError {
    /// The claim statement was empty or all whitespace.
    EmptySubject,
    /// The status requires at least one evidence record.
    MissingEvidence { status: ClaimStatus },
    /// A `verified_original` claim has no evidence that both fingerprints
    /// original data (installation or content) and locates the observation.
    UnverifiedOriginalEvidence,
    /// A `contradicted` claim names no claim it disagrees with.
    ContradictionWithoutDispute,
    /// A claim cannot dispute itself.
    SelfDispute,
    /// A locator was present but its container named nothing.
    EmptyLocatorContainer,
    /// A locator span had zero length.
    ZeroLengthSpan,
    /// A recorded limitation was empty or all whitespace.
    EmptyLimitation,
}

impl fmt::Display for ClaimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySubject => write!(f, "claim subject must not be empty"),
            Self::MissingEvidence { status } => {
                write!(f, "a {status} claim requires at least one evidence record")
            }
            Self::UnverifiedOriginalEvidence => write!(
                f,
                "a verified_original claim requires evidence that fingerprints original \
                 installation or content data and locates the observation"
            ),
            Self::ContradictionWithoutDispute => write!(
                f,
                "a contradicted claim must name the claims it disagrees with"
            ),
            Self::SelfDispute => write!(f, "a claim cannot dispute itself"),
            Self::EmptyLocatorContainer => {
                write!(f, "an observation locator must name its container")
            }
            Self::ZeroLengthSpan => write!(f, "an observation span must not be empty"),
            Self::EmptyLimitation => {
                write!(f, "an evidence limitation must not be empty text")
            }
        }
    }
}

impl std::error::Error for ClaimError {}

impl ClaimRecord {
    /// The per-record admission rules (F01-A).
    ///
    /// Set-level rules — duplicate ids, dependency invalidation when a
    /// fingerprint changes — belong to the ledger in F01-B and are not
    /// checked here. The first violation found is returned; any non-`Ok`
    /// result refuses the claim.
    pub fn validate(&self) -> Result<(), ClaimError> {
        if self.subject.trim().is_empty() {
            return Err(ClaimError::EmptySubject);
        }
        if self.disputes.contains(&self.id) {
            return Err(ClaimError::SelfDispute);
        }
        for evidence in &self.evidence {
            if let Some(locator) = &evidence.locator {
                if locator.container.trim().is_empty() {
                    return Err(ClaimError::EmptyLocatorContainer);
                }
                if let Some(span) = locator.span
                    && span.length == 0
                {
                    return Err(ClaimError::ZeroLengthSpan);
                }
            }
            if evidence
                .limitations
                .iter()
                .any(|limit| limit.trim().is_empty())
            {
                return Err(ClaimError::EmptyLimitation);
            }
        }
        match self.status {
            ClaimStatus::Documented
            | ClaimStatus::ObservedTool
            | ClaimStatus::VerifiedOriginal
            | ClaimStatus::Inferred => {
                if self.evidence.is_empty() {
                    return Err(ClaimError::MissingEvidence {
                        status: self.status,
                    });
                }
            }
            ClaimStatus::Designed | ClaimStatus::Unknown | ClaimStatus::Contradicted => {}
        }
        if self.status == ClaimStatus::VerifiedOriginal
            && !self.evidence.iter().any(EvidenceRecord::verifies_original)
        {
            return Err(ClaimError::UnverifiedOriginalEvidence);
        }
        if self.status == ClaimStatus::Contradicted && self.disputes.is_empty() {
            return Err(ClaimError::ContradictionWithoutDispute);
        }
        Ok(())
    }
}
