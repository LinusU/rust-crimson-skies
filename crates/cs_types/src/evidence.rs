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
//! [`FingerprintIndex`] of freshly observed data. Wiring the ledger into
//! `cs-inspect` commands is F01-C. Nothing in this module is derived from
//! original game data.

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
