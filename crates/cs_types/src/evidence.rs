//! Canonical claim and evidence records for the evidence ledger (F01-A).
//!
//! Every factual compatibility claim is a [`ClaimRecord`]: a [`ClaimId`], a
//! [`ClaimStatus`] and the [`EvidenceRecord`]s that back it. Each evidence
//! record references a source, an exact revision or game fingerprint, an
//! observation locator, an observation method and its limitations. Automated
//! test success travels in a separate [`TestOutcome`] field and never doubles
//! as an evidence status (spec F01, "Deliverable and interfaces").
//!
//! This module also carries the set-level ledger rules added by F01-B:
//! [`validate_ledger`] rejects duplicate ids and dangling disputes and
//! invalidates claims whose fingerprinted evidence no longer matches the
//! [`FingerprintIndex`] of freshly observed data. F01-C replaced the free-text
//! adjudication note with the typed [`Adjudication`] state machine and wired
//! the ledger into `cs-inspect`'s audit path. F01-D added the
//! release-inventory provenance check: [`check_release_inventory`] detects
//! committed executables, original data, bundled media and unidentified
//! binaries in the shipped file set (spec F01, AC04). Rally task #337
//! tightened `verified_original` further: only evidence recorded with a
//! direct-observation method — byte inspection, tool probe or runtime
//! observation — can back it; authored, inferred and document-review
//! records cannot. Nothing in this module is derived from original game
//! data.

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

impl FingerprintKind {
    /// The schema-vocabulary name of the kind.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Installation => "installation",
            Self::Content => "content",
            Self::Artifact => "artifact",
        }
    }
}

impl fmt::Display for FingerprintKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
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

impl ObservationMethod {
    /// Whether the method directly observes the data under claim.
    ///
    /// Only `ByteInspection`, `ToolProbe` and `RuntimeObservation` qualify:
    /// `verified_original` means someone looked at fingerprinted original
    /// bytes or at the running original program. `DocumentReview` reports
    /// what a cited source *says* — that backs `documented`, even when the
    /// document describes original data — and `Inference` and `Authored`
    /// are not observations at all.
    pub const fn is_direct_observation(self) -> bool {
        matches!(
            self,
            Self::ByteInspection | Self::ToolProbe | Self::RuntimeObservation
        )
    }
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
    /// original installation or content data, locates the observation and
    /// records a method that directly observes the data
    /// ([`ObservationMethod::is_direct_observation`]). Synthetic fixture
    /// content can never verify originality, regardless of the fingerprint
    /// kind its recorder attached; authored or inferred records are not
    /// observations and cannot verify it either, even when they point at
    /// fingerprinted original data.
    pub fn verifies_original(&self) -> bool {
        !matches!(self.source, EvidenceSource::SyntheticFixture)
            && self
                .fingerprint
                .is_some_and(Fingerprint::identifies_original)
            && self.locator.is_some()
            && self.method.is_direct_observation()
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

/// Where the adjudication of a recorded dispute stands (F01-C).
///
/// Adjudication never merges or deletes the disagreeing claims: whatever the
/// state, every side keeps its record, status and evidence in the ledger. The
/// state only tracks where the *disagreement* stands (spec F01, AC03).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Adjudication {
    /// The disagreement is on record; no ruling has been issued.
    Open,
    /// A ruling was issued. `upholds` names the claim the ruling keeps
    /// standing — it must be a party to the recorded dispute, meaning the
    /// claim itself or one of its `disputes` — and `rationale` carries the
    /// reasoning. Claims the ruling does not uphold stay in the ledger with
    /// their `contradicted` status and evidence; nothing is rewritten or
    /// dropped.
    Ruled {
        /// The claim the ruling keeps standing.
        upholds: ClaimId,
        /// Why the ruling went that way.
        rationale: String,
    },
}

impl Adjudication {
    /// The short state label for diagnostics: `open` or `ruled`.
    pub const fn state_label(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Ruled { .. } => "ruled",
        }
    }
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
    /// The typed adjudication state of the recorded dispute (F01-C). A
    /// `contradicted` claim must carry one — AC03 preserves the adjudication
    /// state, not just the fact of disagreement — and a claim that disputes
    /// nothing may not carry one.
    pub adjudication: Option<Adjudication>,
}

/// Why a [`ClaimRecord`] failed the per-record admission rules.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaimError {
    /// The claim statement was empty or all whitespace.
    EmptySubject,
    /// The status requires at least one evidence record.
    MissingEvidence { status: ClaimStatus },
    /// A `verified_original` claim has no evidence that fingerprints
    /// original data (installation or content), locates the observation and
    /// records a direct-observation method.
    UnverifiedOriginalEvidence,
    /// A `contradicted` claim names no claim it disagrees with.
    ContradictionWithoutDispute,
    /// A `contradicted` claim carries no adjudication state.
    ContradictionWithoutAdjudication,
    /// An adjudication state was recorded on a claim that disputes nothing.
    AdjudicationWithoutDispute,
    /// A ruling upholds a claim that is not a party to the recorded dispute.
    RulingOutsideDispute {
        /// The claim the ruling named: neither this claim nor one of its
        /// disputes.
        upheld: ClaimId,
    },
    /// A ruling's rationale was empty or all whitespace.
    EmptyRationale,
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
                 installation or content data, locates the observation and was made by \
                 byte inspection, tool probe or runtime observation"
            ),
            Self::ContradictionWithoutDispute => write!(
                f,
                "a contradicted claim must name the claims it disagrees with"
            ),
            Self::ContradictionWithoutAdjudication => {
                write!(f, "a contradicted claim must carry its adjudication state")
            }
            Self::AdjudicationWithoutDispute => {
                write!(f, "an adjudication state requires a recorded dispute")
            }
            Self::RulingOutsideDispute { upheld } => write!(
                f,
                "a ruling can only uphold a party to the dispute, not {upheld}"
            ),
            Self::EmptyRationale => {
                write!(f, "a ruling's rationale must not be empty text")
            }
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
        if self.status == ClaimStatus::Contradicted && self.adjudication.is_none() {
            return Err(ClaimError::ContradictionWithoutAdjudication);
        }
        if let Some(adjudication) = &self.adjudication {
            if self.disputes.is_empty() {
                return Err(ClaimError::AdjudicationWithoutDispute);
            }
            if let Adjudication::Ruled { upholds, rationale } = adjudication {
                if *upholds != self.id && !self.disputes.contains(upholds) {
                    return Err(ClaimError::RulingOutsideDispute {
                        upheld: upholds.clone(),
                    });
                }
                if rationale.trim().is_empty() {
                    return Err(ClaimError::EmptyRationale);
                }
            }
        }
        Ok(())
    }
}

/// A fingerprint freshly observed for one asset container.
///
/// `container` is the asset identity an [`ObservationLocator`] names (an
/// archive path, a file member or a document anchor); `fingerprint` is what
/// the current observation measured there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedFingerprint {
    /// The container whose content was hashed.
    pub container: String,
    /// The digest measured now.
    pub fingerprint: Fingerprint,
}

/// Why a set of [`ObservedFingerprint`]s was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObservationIndexError {
    /// Two observations name the same kind and container but measured
    /// different digests; the index refuses to choose between them.
    ConflictingObservations {
        /// The role of the conflicting fingerprint.
        kind: FingerprintKind,
        /// The container both observations name.
        container: String,
    },
}

impl fmt::Display for ObservationIndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConflictingObservations { kind, container } => write!(
                f,
                "conflicting {kind} observations for container {container:?}"
            ),
        }
    }
}

impl std::error::Error for ObservationIndexError {}

/// The fingerprints currently observed for the assets a ledger depends on.
///
/// Entries are keyed by (kind, container): the role the hash plays plus the
/// asset identity an [`ObservationLocator`] names. An evidence record depends
/// on an entry when both match; if the recorded digest differs from the
/// observed one the dependent claim is invalidated by [`validate_ledger`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FingerprintIndex {
    entries: Vec<ObservedFingerprint>,
}

impl FingerprintIndex {
    /// An empty index: every fingerprinted dependency reports unchecked.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds an index from observations.
    ///
    /// Two observations of the same (kind, container) with different digests
    /// are a contradiction in the input, not something to average out: the
    /// build refuses them with
    /// [`ObservationIndexError::ConflictingObservations`]. Identical
    /// duplicates collapse to one entry.
    pub fn from_observations(
        observations: Vec<ObservedFingerprint>,
    ) -> Result<Self, ObservationIndexError> {
        let mut index = Self::new();
        for observation in observations {
            let existing = index
                .entries
                .iter()
                .find(|entry| {
                    entry.container == observation.container
                        && entry.fingerprint.kind == observation.fingerprint.kind
                })
                .map(|entry| entry.fingerprint.sha256);
            match existing {
                Some(sha256) if sha256 != observation.fingerprint.sha256 => {
                    return Err(ObservationIndexError::ConflictingObservations {
                        kind: observation.fingerprint.kind,
                        container: observation.container,
                    });
                }
                Some(_) => {}
                None => index.entries.push(observation),
            }
        }
        Ok(index)
    }

    /// The digest currently observed for `kind` + `container`, if the pair
    /// was observed at all.
    pub fn current(&self, kind: FingerprintKind, container: &str) -> Option<ContentHash> {
        self.entries
            .iter()
            .find(|entry| entry.container == container && entry.fingerprint.kind == kind)
            .map(|entry| entry.fingerprint.sha256)
    }

    /// The observations the index holds, in insertion order. Consumers that
    /// re-emit an index as producer input — for example feeding a stored
    /// index into `cs-inspect`'s audit — go through this view.
    pub fn observations(&self) -> &[ObservedFingerprint] {
        &self.entries
    }

    /// How many distinct (kind, container) pairs the index holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the index holds no observations.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Why a claim was refused by the ledger-level rules (F01-B).
///
/// These are violations one record cannot see on its own; per-record failures
/// surface through [`LedgerError::Record`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LedgerError {
    /// The record itself failed [`ClaimRecord::validate`].
    Record(ClaimError),
    /// Another claim in the set uses the same id. The ledger refuses to
    /// choose between them, so every occurrence is rejected.
    DuplicateId,
    /// The claim disputes an id that no claim in the set carries.
    UnknownDispute {
        /// The disputed id that does not exist in the set.
        disputed: ClaimId,
    },
}

impl fmt::Display for LedgerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Record(error) => write!(f, "{error}"),
            Self::DuplicateId => {
                write!(f, "another claim in the ledger already uses this id")
            }
            Self::UnknownDispute { disputed } => {
                write!(f, "disputed claim {disputed} is not in the ledger")
            }
        }
    }
}

impl std::error::Error for LedgerError {}

/// One claim refused by ledger validation, with the reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerRejection {
    pub claim: ClaimId,
    pub error: LedgerError,
}

/// One evidence record whose recorded digest no longer matches the observed
/// one: the asset changed underneath the claim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaleEvidence {
    /// The container the stale observation names.
    pub container: String,
    /// The role of the changed fingerprint.
    pub kind: FingerprintKind,
    /// The digest the evidence recorded.
    pub recorded: ContentHash,
    /// The digest the index observes now.
    pub observed: ContentHash,
}

/// A claim whose standing is revoked because observed data changed beneath
/// its evidence (spec F01, non-negotiable behavior 1).
///
/// Invalidation never deletes or rewrites the claim: it stays in the ledger
/// with its original status and evidence, and this report entry explains why
/// it no longer stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimInvalidation {
    pub claim: ClaimId,
    /// Every evidence record whose fingerprint went stale.
    pub stale: Vec<StaleEvidence>,
}

/// Why a fingerprinted evidence record could not be re-checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UncheckedReason {
    /// The record carries a fingerprint but no locator, so it cannot be tied
    /// to an observed asset.
    MissingLocator,
    /// The index holds no observation for the record's kind and container.
    NotObserved,
}

impl fmt::Display for UncheckedReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingLocator => {
                f.write_str("fingerprinted evidence has no observation locator")
            }
            Self::NotObserved => {
                f.write_str("no current observation for this fingerprinted container")
            }
        }
    }
}

/// Fingerprinted evidence the index can neither confirm nor refute.
///
/// An unchecked dependency does not invalidate its claim — nothing showed it
/// stale — but a strict audit must see it rather than treat silence as a
/// confirmation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UncheckedDependency {
    pub claim: ClaimId,
    /// The role of the fingerprint that could not be re-checked.
    pub kind: FingerprintKind,
    /// The container the evidence names, when it names one.
    pub container: Option<String>,
    pub reason: UncheckedReason,
}

/// The result of validating a claim set against observed fingerprints.
///
/// Every claim lands in exactly one disposition: `rejected` (broke a rule),
/// `invalidated` (a dependency went stale) or `valid` (neither). Fingerprinted
/// evidence that could not be re-checked is additionally listed in
/// `unchecked` for `valid` and `invalidated` claims; a rejected claim's
/// evidence is not evaluated at all, since its standing is already refused.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LedgerReport {
    /// Claims admitted by every rule with no stale dependency, input order.
    pub valid: Vec<ClaimId>,
    /// Claims refused by per-record or set-level rules, input order.
    pub rejected: Vec<LedgerRejection>,
    /// Claims whose fingerprinted evidence went stale, input order.
    pub invalidated: Vec<ClaimInvalidation>,
    /// Fingerprinted evidence the index cannot confirm or refute, input order.
    pub unchecked: Vec<UncheckedDependency>,
}

impl LedgerReport {
    /// Every claim stands and every fingerprinted dependency was confirmed:
    /// no rejection, no invalidation and nothing left unchecked.
    pub fn is_clean(&self) -> bool {
        self.rejected.is_empty() && self.invalidated.is_empty() && self.unchecked.is_empty()
    }

    /// One stderr-suitable diagnostic line per problem, in input order.
    pub fn diagnostic_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for rejection in &self.rejected {
            lines.push(format!(
                "claim {} rejected: {}",
                rejection.claim, rejection.error
            ));
        }
        for invalidation in &self.invalidated {
            for stale in &invalidation.stale {
                lines.push(format!(
                    "claim {} invalidated: {} fingerprint of {:?} changed from {} to {}",
                    invalidation.claim, stale.kind, stale.container, stale.recorded, stale.observed
                ));
            }
        }
        for unchecked in &self.unchecked {
            let target = unchecked.container.as_deref().unwrap_or("<no locator>");
            lines.push(format!(
                "claim {} unchecked: {} fingerprint of {:?}: {}",
                unchecked.claim, unchecked.kind, target, unchecked.reason
            ));
        }
        lines
    }
}

/// Validates a claim set as a ledger (F01-B).
///
/// On top of each record's own [`ClaimRecord::validate`], this enforces the
/// set-level rules — duplicate ids and disputes that name no claim in the
/// set — and re-checks every fingerprinted evidence record against
/// `observed`. A recorded digest that differs from the current observation
/// invalidates the dependent claim; a dependency the index does not cover is
/// reported unchecked rather than silently trusted.
///
/// Claims that broke a rule are reported `rejected` and are not evaluated for
/// invalidation or unchecked dependencies: their standing is already refused.
/// A claim may appear in `unchecked` and still be `valid` — unconfirmed is
/// not falsified.
pub fn validate_ledger(claims: &[ClaimRecord], observed: &FingerprintIndex) -> LedgerReport {
    let mut report = LedgerReport::default();

    let mut occurrences: std::collections::BTreeMap<&ClaimId, usize> =
        std::collections::BTreeMap::new();
    for claim in claims {
        *occurrences.entry(&claim.id).or_insert(0) += 1;
    }
    let known: std::collections::BTreeSet<&ClaimId> = claims.iter().map(|c| &c.id).collect();

    for claim in claims {
        let mut errors: Vec<LedgerError> = Vec::new();
        if let Err(error) = claim.validate() {
            errors.push(LedgerError::Record(error));
        }
        if occurrences[&claim.id] > 1 {
            errors.push(LedgerError::DuplicateId);
        }
        for disputed in &claim.disputes {
            if !known.contains(disputed) {
                errors.push(LedgerError::UnknownDispute {
                    disputed: disputed.clone(),
                });
            }
        }
        if !errors.is_empty() {
            report
                .rejected
                .extend(errors.into_iter().map(|error| LedgerRejection {
                    claim: claim.id.clone(),
                    error,
                }));
            continue;
        }

        let mut stale: Vec<StaleEvidence> = Vec::new();
        for evidence in &claim.evidence {
            let Some(fingerprint) = evidence.fingerprint else {
                continue;
            };
            let Some(locator) = &evidence.locator else {
                report.unchecked.push(UncheckedDependency {
                    claim: claim.id.clone(),
                    kind: fingerprint.kind,
                    container: None,
                    reason: UncheckedReason::MissingLocator,
                });
                continue;
            };
            match observed.current(fingerprint.kind, &locator.container) {
                Some(observed_hash) if observed_hash != fingerprint.sha256 => {
                    stale.push(StaleEvidence {
                        container: locator.container.clone(),
                        kind: fingerprint.kind,
                        recorded: fingerprint.sha256,
                        observed: observed_hash,
                    });
                }
                Some(_) => {}
                None => report.unchecked.push(UncheckedDependency {
                    claim: claim.id.clone(),
                    kind: fingerprint.kind,
                    container: Some(locator.container.clone()),
                    reason: UncheckedReason::NotObserved,
                }),
            }
        }
        if stale.is_empty() {
            report.valid.push(claim.id.clone());
        } else {
            report.invalidated.push(ClaimInvalidation {
                claim: claim.id.clone(),
                stale,
            });
        }
    }
    report
}

/* ------------------------------------------------------------------ */
/* Release-inventory provenance (F01-D)                                */
/* ------------------------------------------------------------------ */

/// Bytes a producer samples from the start of a file to fill
/// [`InventoryEntry::header`]. Every signature in the check tables fits in
/// it.
pub const INVENTORY_HEADER_LEN: usize = 512;

/// Bytes a producer may sample to decide [`InventoryEntry::text`]: the
/// usual binary sniff — a NUL byte or invalid UTF-8 inside the sample means
/// binary content.
pub const TEXT_SAMPLE_LEN: usize = 8192;

/// One file in a committed or shipped release inventory (F01-D).
///
/// `path` is the slash-separated path relative to the inventory root — the
/// `git ls-files` spelling for the committed tree, or a package-relative
/// name for a release archive. `header` carries up to
/// [`INVENTORY_HEADER_LEN`] leading bytes for signature checks. `text`
/// records whether the producer's sample — up to [`TEXT_SAMPLE_LEN`]
/// leading bytes, or the whole file when smaller — decoded as UTF-8 without
/// NUL bytes; it is a sample verdict, not a whole-file guarantee.
///
/// The record intentionally carries no content hash: detection here is by
/// signature, name and provenance location. Identity matching against
/// fingerprinted original files belongs to the F02 installation inventory,
/// which owns hashing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InventoryEntry {
    /// Slash-separated path relative to the inventory root.
    pub path: String,
    /// Total file size in bytes.
    pub len: u64,
    /// Leading bytes sampled for signature checks.
    pub header: Vec<u8>,
    /// Whether the producer's text sample decoded as UTF-8 without NULs.
    pub text: bool,
}

/// Executable-image signatures (leading bytes → name). An executable is
/// prohibited anywhere in the inventory — authored roots excuse authored
/// data lookalikes, never executables.
pub const EXECUTABLE_SIGNATURES: &[(&[u8], &str)] = &[
    (b"MZ", "MZ (DOS/PE executable)"),
    (b"\x7fELF", "ELF executable"),
    (&[0xfe, 0xed, 0xfa, 0xce], "Mach-O 32-bit"),
    (&[0xfe, 0xed, 0xfa, 0xcf], "Mach-O 64-bit"),
    (&[0xce, 0xfa, 0xed, 0xfe], "Mach-O 32-bit byte-swapped"),
    (&[0xcf, 0xfa, 0xed, 0xfe], "Mach-O 64-bit byte-swapped"),
    (&[0xca, 0xfe, 0xba, 0xbe], "Mach-O fat"),
    (&[0xbe, 0xba, 0xfe, 0xca], "Mach-O fat byte-swapped"),
];

/// Original-data container signatures checked on binary entries outside the
/// authored roots. The INTERP-family signature `0x08971119` (little-endian
/// on disk) is an `observed_tool` lead from `docs/research/FORMAT-NOTES.md`;
/// authored synthetic fixtures legitimately carry it, which is why the
/// check is provenance-aware rather than signature-only.
pub const GAME_DATA_SIGNATURES: &[(&[u8], &str)] = &[(
    &[0x19, 0x11, 0x97, 0x08],
    "INTERP/ZBD-family signature 0x08971119",
)];

/// Media, image and font signatures checked on binary entries outside the
/// authored roots.
pub const MEDIA_SIGNATURES: &[(&[u8], &str)] = &[
    (b"RIFF", "RIFF media (WAV/AVI)"),
    (b"OggS", "Ogg media"),
    (b"ID3", "MP3 audio with ID3 tag"),
    (b"BM", "BMP bitmap"),
    (b"\x89PNG\r\n\x1a\n", "PNG image"),
    (&[0xff, 0xd8, 0xff], "JPEG image"),
    (b"GIF8", "GIF image"),
    (b"DDS ", "DDS texture"),
    (b"OTTO", "OpenType/CFF font"),
    (b"ttcf", "TrueType collection"),
    (&[0x00, 0x01, 0x00, 0x00], "TrueType font"),
    (b"true", "TrueType font"),
    (b"wOFF", "WOFF font"),
    (b"wOF2", "WOFF2 font"),
];

/// Document signatures checked even on text entries outside the authored
/// roots: PDF and RTF are text-compatible formats, so a clean text sample
/// must not excuse a copied manual.
pub const DOCUMENT_SIGNATURES: &[(&[u8], &str)] =
    &[(b"%PDF", "PDF document"), (b"{\\rtf", "RTF document")];

/// Extensions that name an executable image outright.
pub const EXECUTABLE_EXTENSIONS: &[&str] = &[
    "exe", "dll", "com", "sys", "ocx", "cpl", "scr", "drv", "msi",
];

/// Extensions that name original game data or installer containers.
pub const GAME_DATA_EXTENSIONS: &[&str] = &[
    "zbd", "rof", "bm", "gamez", "interp", "big", "cab", "icd", "ifr",
];

/// Extensions that name bundled media, fonts or manuals — extracted art,
/// voice, fonts and commercial manuals per spec F01 non-negotiable
/// behavior 2.
pub const MEDIA_DOC_EXTENSIONS: &[&str] = &[
    "wav", "mp3", "ogg", "flac", "aif", "aiff", "mid", "midi", "bmp", "tga", "png", "jpg", "jpeg",
    "gif", "dds", "tif", "tiff", "ico", "ttf", "otf", "ttc", "fnt", "fon", "woff", "woff2", "pdf",
    "rtf", "doc", "docx", "chm", "hlp", "mpg", "mpeg", "avi", "bik", "smk", "wmv", "mov", "mp4",
];

/// The policy an inventory entry violates (spec F01, non-negotiable
/// behavior 2: no game executables, decompiled game source, extracted art,
/// voice, fonts or commercial manuals in fixtures or releases).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProhibitedContent {
    /// An executable image — PE/COFF, ELF or Mach-O — or a name that claims
    /// one. Prohibited everywhere, authored roots included.
    ExecutableImage,
    /// Original game data: a container signature or a game-data name
    /// outside the authored roots.
    GameData,
    /// Bundled media, a font, or a manual/document outside the authored
    /// roots.
    MediaOrDocument,
    /// Non-text content outside the authored roots that no rule recognized:
    /// a binary with no declared provenance.
    UnidentifiedBinary,
}

impl ProhibitedContent {
    /// The short label for diagnostics.
    pub const fn label(self) -> &'static str {
        match self {
            Self::ExecutableImage => "executable image",
            Self::GameData => "original game data",
            Self::MediaOrDocument => "media, font or document",
            Self::UnidentifiedBinary => "unidentified binary",
        }
    }
}

impl fmt::Display for ProhibitedContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// What condemned an [`InventoryEntry`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InventoryMatch {
    /// A leading-bytes signature, named by the check tables.
    Signature(&'static str),
    /// The lowercased extension that matched.
    Extension(String),
    /// Non-text content nothing recognized.
    NonText,
}

impl fmt::Display for InventoryMatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Signature(name) => write!(f, "signature {name}"),
            Self::Extension(ext) => write!(f, "extension .{ext}"),
            Self::NonText => f.write_str("non-text content"),
        }
    }
}

/// One prohibited entry in a release inventory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InventoryViolation {
    /// The inventory path that broke the policy.
    pub path: String,
    /// Which prohibition it broke.
    pub content: ProhibitedContent,
    /// The signature, extension or binary fact that condemned it.
    pub matched: InventoryMatch,
}

/// The result of checking a release inventory (F01-D).
///
/// Like the ledger report, the outcome is a list, not a bare pass/fail:
/// every violating entry is named with its reason so diagnostics and
/// review can see exactly what was condemned.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InventoryReport {
    /// Entries examined.
    pub checked: usize,
    /// Every prohibited entry, in input order.
    pub violations: Vec<InventoryViolation>,
}

impl InventoryReport {
    /// No prohibited content was found.
    pub fn is_clean(&self) -> bool {
        self.violations.is_empty()
    }

    /// One stderr-suitable diagnostic line per violation, in input order.
    pub fn diagnostic_lines(&self) -> Vec<String> {
        self.violations
            .iter()
            .map(|violation| {
                format!(
                    "inventory entry {:?} prohibited: {} ({})",
                    violation.path, violation.content, violation.matched
                )
            })
            .collect()
    }
}

/// The final component's extension, lowercased — `None` when there is none
/// or when the name is a dotfile (`.gitignore` is a name, not an
/// extension).
fn entry_extension(path: &str) -> Option<String> {
    let name = path.rsplit('/').next()?;
    let (stem, ext) = name.rsplit_once('.')?;
    if stem.is_empty() || ext.is_empty() {
        return None;
    }
    Some(ext.to_lowercase())
}

/// Normalizes a producer-supplied path for the authored-root test:
/// backslashes become slashes and a leading `./` is dropped.
fn normalize_inventory_path(path: &str) -> String {
    let mut normalized = path.replace('\\', "/");
    while let Some(rest) = normalized.strip_prefix("./") {
        normalized = rest.to_owned();
    }
    normalized
}

/// Whether `path` sits beneath one of the authored roots. Comparison is
/// ASCII case-insensitive: on a case-insensitive filesystem a differently
/// cased spelling is the same directory, so the exemption must match it —
/// and a lookalike directory that merely dodges the check by case is not a
/// declared authored root either way the comparison lands.
fn under_authored_root(path: &str, authored_roots: &[&str]) -> bool {
    let path = normalize_inventory_path(path);
    authored_roots.iter().any(|root| {
        let root = normalize_inventory_path(root);
        let root = root.trim_matches('/');
        path.len() > root.len()
            && path.as_bytes()[root.len()] == b'/'
            && path[..root.len()].eq_ignore_ascii_case(root)
    })
}

/// The first signature in `table` present at the start of `header`.
fn match_signature<'a>(header: &[u8], table: &'a [(&'a [u8], &'a str)]) -> Option<&'a str> {
    table
        .iter()
        .find(|(signature, _)| header.starts_with(signature))
        .map(|(_, name)| *name)
}

/// An MP3 frame sync word: `0xff` followed by three set sync bits.
fn is_mp3_sync(header: &[u8]) -> bool {
    header.len() >= 2 && header[0] == 0xff && header[1] & 0xe0 == 0xe0
}

/// The per-entry policy: which prohibition `entry` breaks, if any.
///
/// Order matters. Executables are condemned before the authored-root
/// exemption is consulted, so a committed binary cannot hide inside a
/// declared authored directory. Data and media rules apply only outside the
/// authored roots — authored lookalike fixtures are the legitimate reason
/// those roots exist. Extension rules apply to text and binary entries
/// alike: a name that claims a retail container or a commercial document is
/// suspect even when its content happens to decode as text. Anything
/// non-text nothing recognized still falls to
/// [`ProhibitedContent::UnidentifiedBinary`], so a renamed original file
/// cannot slip through by dropping its extension and signature.
fn classify_entry(
    entry: &InventoryEntry,
    authored_roots: &[&str],
) -> Option<(ProhibitedContent, InventoryMatch)> {
    let extension = entry_extension(&entry.path);

    if !entry.text
        && let Some(name) = match_signature(&entry.header, EXECUTABLE_SIGNATURES)
    {
        return Some((
            ProhibitedContent::ExecutableImage,
            InventoryMatch::Signature(name),
        ));
    }
    if let Some(ext) = &extension
        && EXECUTABLE_EXTENSIONS.contains(&ext.as_str())
    {
        return Some((
            ProhibitedContent::ExecutableImage,
            InventoryMatch::Extension(ext.clone()),
        ));
    }

    if under_authored_root(&entry.path, authored_roots) {
        return None;
    }

    if !entry.text
        && let Some(name) = match_signature(&entry.header, GAME_DATA_SIGNATURES)
    {
        return Some((ProhibitedContent::GameData, InventoryMatch::Signature(name)));
    }
    if let Some(name) = match_signature(&entry.header, DOCUMENT_SIGNATURES) {
        return Some((
            ProhibitedContent::MediaOrDocument,
            InventoryMatch::Signature(name),
        ));
    }
    if !entry.text {
        if let Some(name) = match_signature(&entry.header, MEDIA_SIGNATURES) {
            return Some((
                ProhibitedContent::MediaOrDocument,
                InventoryMatch::Signature(name),
            ));
        }
        if is_mp3_sync(&entry.header) {
            return Some((
                ProhibitedContent::MediaOrDocument,
                InventoryMatch::Signature("MP3 frame sync"),
            ));
        }
    }
    if let Some(ext) = &extension
        && GAME_DATA_EXTENSIONS.contains(&ext.as_str())
    {
        return Some((
            ProhibitedContent::GameData,
            InventoryMatch::Extension(ext.clone()),
        ));
    }
    if let Some(ext) = &extension
        && MEDIA_DOC_EXTENSIONS.contains(&ext.as_str())
    {
        return Some((
            ProhibitedContent::MediaOrDocument,
            InventoryMatch::Extension(ext.clone()),
        ));
    }
    if !entry.text {
        return Some((
            ProhibitedContent::UnidentifiedBinary,
            InventoryMatch::NonText,
        ));
    }
    None
}

/// Checks a release inventory for committed prohibited content (spec F01,
/// AC04 and non-negotiable behavior 2).
///
/// `entries` is the shipped or committed file set; `authored_roots` are the
/// slash-separated subtrees the owner declares as authored content (this
/// repository's is `fixtures/synthetic`, a protected path). Executable
/// images are condemned wherever they sit; original data, bundled media and
/// unidentified binaries only outside the authored roots. Violations are
/// reported, never silently dropped — the report names every offending
/// entry with what condemned it.
pub fn check_release_inventory(
    entries: &[InventoryEntry],
    authored_roots: &[&str],
) -> InventoryReport {
    let mut report = InventoryReport {
        checked: entries.len(),
        ..InventoryReport::default()
    };
    for entry in entries {
        if let Some((content, matched)) = classify_entry(entry, authored_roots) {
            report.violations.push(InventoryViolation {
                path: entry.path.clone(),
                content,
                matched,
            });
        }
    }
    report
}
