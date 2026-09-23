//! Claim-record admission checks, ledger validation, the audit wiring for
//! the evidence ledger (F01-A, F01-B, F01-C) and the release-inventory
//! provenance gate (F01-D).
//!
//! `cs-inspect` owns command-line inspection and conversion diagnostics. This
//! module is the inspector's front end for the canonical records in
//! [`cs_types::evidence`]: [`check_claims`] runs the per-record admission
//! rules, [`check_ledger`] runs the full ledger validation — set-level rules
//! plus dependency invalidation against freshly observed fingerprints — and
//! [`audit_claims`] wires the two together the way the `audit` command and
//! content exports consume them: fresh observations in, a report preserving
//! every disagreement and its adjudication state out.
//!
//! The F01-D half produces the release inventory — [`scan_inventory_dir`]
//! for shipped trees and [`committed_inventory`] for the git-tracked file
//! set — and feeds it to [`cs_types::evidence::check_release_inventory`]
//! through [`audit_release_inventory`], which applies this repository's
//! declared [`AUTHORED_CONTENT_ROOTS`].

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use cs_types::evidence::{
    Adjudication, ClaimError, ClaimId, ClaimRecord, ClaimStatus, ContentHash, EvidenceRecord,
    EvidenceSource, Fingerprint, FingerprintIndex, FingerprintKind, INVENTORY_HEADER_LEN,
    InventoryEntry, InventoryReport, LedgerReport, ObservationIndexError, ObservationLocator,
    ObservationMethod, ObservedFingerprint, SourceSpan, TEXT_SAMPLE_LEN, check_release_inventory,
};

/// One claim refused by the admission check, with the error that sank it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimRejection {
    pub id: ClaimId,
    pub error: ClaimError,
}

/// The outcome of checking a set of claim records.
///
/// Rejection is per claim and never collapses to a bare pass/fail: the report
/// keeps every offending claim id and its reason so diagnostics can name them
/// instead of logging a failure as success.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClaimReport {
    /// Ids admitted by the per-record rules, in input order.
    pub admitted: Vec<ClaimId>,
    /// Claims refused by the per-record rules, in input order.
    pub rejected: Vec<ClaimRejection>,
}

impl ClaimReport {
    /// Every record in the checked set passed the per-record rules.
    pub fn is_clean(&self) -> bool {
        self.rejected.is_empty()
    }

    /// One stderr-suitable diagnostic line per rejected claim.
    pub fn diagnostic_lines(&self) -> Vec<String> {
        self.rejected
            .iter()
            .map(|rejection| format!("claim {} rejected: {}", rejection.id, rejection.error))
            .collect()
    }
}

/// Runs the per-record admission rules over a claim set.
///
/// Records keep their input order. A claim that fails
/// [`ClaimRecord::validate`] lands in [`ClaimReport::rejected`] with its
/// error; the check never reports a rejected claim as admitted.
pub fn check_claims(claims: &[ClaimRecord]) -> ClaimReport {
    let mut report = ClaimReport::default();
    for claim in claims {
        match claim.validate() {
            Ok(()) => report.admitted.push(claim.id.clone()),
            Err(error) => report.rejected.push(ClaimRejection {
                id: claim.id.clone(),
                error,
            }),
        }
    }
    report
}

/// Runs the ledger rules over a claim set against freshly observed
/// fingerprints (F01-B).
///
/// This is the inspector's entry point for
/// [`cs_types::evidence::validate_ledger`]: per-record admission, duplicate
/// ids, dangling disputes and invalidation of claims whose fingerprinted
/// evidence no longer matches `observed`. The `audit` command (F01-C) feeds
/// it the ledger under review and the fingerprints it just measured.
pub fn check_ledger(claims: &[ClaimRecord], observed: &FingerprintIndex) -> LedgerReport {
    cs_types::evidence::validate_ledger(claims, observed)
}

/// Why an audit could not run at all (F01-C).
///
/// The producer stage — folding freshly observed fingerprints into a
/// [`FingerprintIndex`] — can fail on its own; the error propagates to the
/// caller instead of being folded into an empty index that would silently
/// mark every fingerprinted dependency unchecked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuditError {
    /// Two observations of the same (kind, container) disagree; the audit
    /// refuses to arbitrate between them.
    ConflictingObservations(ObservationIndexError),
}

impl fmt::Display for AuditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConflictingObservations(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for AuditError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ConflictingObservations(error) => Some(error),
        }
    }
}

/// One edge of a recorded dispute: the claim under dispute and whether the
/// audited set actually contains it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisputeEdge {
    /// The claim the edge points at.
    pub disputed: ClaimId,
    /// Whether a claim with this id exists in the audited set. Absent targets
    /// are also rejected by the ledger rules as `UnknownDispute`; the flag
    /// keeps the contradiction view truthful even for a rejected claim.
    pub present: bool,
}

/// One recorded disagreement as the audit reports it (spec F01, AC03).
///
/// The report names both sides of the dispute and carries the claim's typed
/// adjudication state; it never merges the disagreeing records or picks the
/// convenient source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContradictionReport {
    /// The claim carrying `contradicted` status.
    pub claim: ClaimId,
    /// The disputes it records, in recorded order.
    pub edges: Vec<DisputeEdge>,
    /// The adjudication state on the record.
    pub adjudication: Option<Adjudication>,
}

/// The audit's consumer-facing report (F01-C): the ledger dispositions plus
/// the preserved disagreement view.
///
/// A report is what the `audit` command renders and what content exports
/// attach as provenance; `None` adjudication on a contradicted claim is a
/// record-level defect the ledger report already rejects, so the view never
/// has to hide it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuditReport {
    /// Ledger validation over the audited claim set.
    pub ledger: LedgerReport,
    /// One entry per `contradicted` claim, in input order — including
    /// rejected ones, so a malformed contradiction stays visible rather than
    /// disappearing with its claim's standing.
    pub contradictions: Vec<ContradictionReport>,
}

impl AuditReport {
    /// The audit passed: every claim stands and every fingerprinted
    /// dependency was confirmed against the fresh observations.
    pub fn is_clean(&self) -> bool {
        self.ledger.is_clean()
    }

    /// One stderr-suitable diagnostic line per ledger problem.
    pub fn diagnostic_lines(&self) -> Vec<String> {
        self.ledger.diagnostic_lines()
    }
}

/// Runs the audit: ledger validation wired to its producer and its consumer
/// (F01-C).
///
/// Producer side: `observations` are the fingerprints the caller just
/// measured; they are folded into a [`FingerprintIndex`] here. Conflicting
/// observations of one (kind, container) abort the audit with
/// [`AuditError::ConflictingObservations`] — the conflict is an input
/// contradiction, not something an index may average out.
///
/// Consumer side: the returned [`AuditReport`] carries the ledger
/// dispositions and, for every `contradicted` claim, both sides of the
/// disagreement plus its adjudication state.
///
/// The audit is stateless: it borrows the claim set, consumes the
/// observations and returns a complete report. There is no partial state to
/// tear down, and a failed audit is retried by calling again with corrected
/// observations — nothing from the refused run survives.
pub fn audit_claims(
    claims: &[ClaimRecord],
    observations: Vec<ObservedFingerprint>,
) -> Result<AuditReport, AuditError> {
    let observed = FingerprintIndex::from_observations(observations)
        .map_err(AuditError::ConflictingObservations)?;
    let ledger = check_ledger(claims, &observed);
    let known: std::collections::BTreeSet<&ClaimId> =
        claims.iter().map(|claim| &claim.id).collect();
    let contradictions = claims
        .iter()
        .filter(|claim| claim.status == ClaimStatus::Contradicted)
        .map(|claim| ContradictionReport {
            claim: claim.id.clone(),
            edges: claim
                .disputes
                .iter()
                .map(|disputed| DisputeEdge {
                    disputed: disputed.clone(),
                    present: known.contains(disputed),
                })
                .collect(),
            adjudication: claim.adjudication.clone(),
        })
        .collect();
    Ok(AuditReport {
        ledger,
        contradictions,
    })
}

/// SHA-256 of `fixtures/synthetic/flat-uncompressed.rof`, an authored
/// synthetic fixture. It is an artifact fingerprint, never an original-data
/// one, and is recorded here so fixture claims reference the real file.
const FLAT_ROF_FIXTURE_SHA256: [u8; 32] = [
    0x97, 0xa2, 0xd7, 0x56, 0x5f, 0x88, 0xbe, 0x3b, 0x53, 0x20, 0x4a, 0xb0, 0xe6, 0xeb, 0x5e, 0x39,
    0xff, 0xf8, 0x86, 0xc0, 0x80, 0x15, 0x6f, 0x97, 0xa9, 0xe5, 0x36, 0x5f, 0x27, 0x43, 0xa1, 0x23,
];

/// The minimal synthetic fixture claim set: three authored claims exercising
/// the `designed`, `observed_tool` and `documented` statuses over development
/// and synthetic sources.
///
/// No claim in this set is or can be `verified_original`: fixture and
/// authored content are not original data. The set exists so tests and the
/// future `audit` command have a real input without touching the owner's
/// installation.
pub fn synthetic_claim_fixture() -> Vec<ClaimRecord> {
    vec![
        ClaimRecord {
            id: ClaimId::new("cs.synthetic-scene.falls").expect("fixture claim id is valid"),
            subject: "the synthetic development scene integrates a dynamic body under gravity"
                .to_owned(),
            status: ClaimStatus::Designed,
            evidence: vec![EvidenceRecord {
                source: EvidenceSource::SyntheticFixture,
                fingerprint: None,
                locator: Some(ObservationLocator {
                    container: "cs_types::SyntheticBodySpec::falling_box".to_owned(),
                    span: None,
                }),
                method: ObservationMethod::Authored,
                limitations: vec!["asset-free development scene, not retail content".to_owned()],
            }],
            test_outcome: Some(cs_types::evidence::TestOutcome {
                selector: "accept_f00_a_dynamic_synthetic_body_falls".to_owned(),
                passed: 1,
                failed: 0,
            }),
            disputes: vec![],
            adjudication: None,
        },
        ClaimRecord {
            id: ClaimId::new("fixture.rof.flat-uncompressed").expect("fixture claim id is valid"),
            subject: "flat-uncompressed.rof is an uncompressed synthetic archive fixture"
                .to_owned(),
            status: ClaimStatus::ObservedTool,
            evidence: vec![EvidenceRecord {
                source: EvidenceSource::ToolRun {
                    tool: "cs-inspect".to_owned(),
                    version: env!("CARGO_PKG_VERSION").to_owned(),
                },
                fingerprint: Some(Fingerprint {
                    kind: FingerprintKind::Artifact,
                    sha256: ContentHash::from_bytes(FLAT_ROF_FIXTURE_SHA256),
                }),
                locator: Some(ObservationLocator {
                    container: "fixtures/synthetic/flat-uncompressed.rof".to_owned(),
                    span: Some(SourceSpan {
                        offset: 0,
                        length: 110,
                    }),
                }),
                method: ObservationMethod::ByteInspection,
                limitations: vec![
                    "synthetic archive authored by tools/make_synthetic_fixtures.py; \
                     proves nothing about retail containers"
                        .to_owned(),
                ],
            }],
            test_outcome: None,
            disputes: vec![],
            adjudication: None,
        },
        ClaimRecord {
            id: ClaimId::new("f01.evidence-record.minimum").expect("fixture claim id is valid"),
            subject: "evidence records reference a source, an exact revision or fingerprint, \
                      a locator, an observation method and limitations"
                .to_owned(),
            status: ClaimStatus::Documented,
            evidence: vec![EvidenceRecord {
                source: EvidenceSource::Document("docs/contracts/CLI-EVIDENCE.md".to_owned()),
                fingerprint: None,
                locator: Some(ObservationLocator {
                    container: "doc:CLI-EVIDENCE#evidence-record-minimum".to_owned(),
                    span: None,
                }),
                method: ObservationMethod::DocumentReview,
                limitations: vec!["contract text, not an observation of original data".to_owned()],
            }],
            test_outcome: None,
            disputes: vec![],
            adjudication: None,
        },
    ]
}

/// The [`FingerprintIndex`] that confirms [`synthetic_claim_fixture`]: every
/// fingerprinted fixture container at its current digest.
///
/// Mutating one digest (or dropping the entry) is how tests and the future
/// `audit` command exercise dependency invalidation without touching the
/// owner's installation.
pub fn synthetic_fingerprint_index() -> FingerprintIndex {
    FingerprintIndex::from_observations(vec![ObservedFingerprint {
        container: "fixtures/synthetic/flat-uncompressed.rof".to_owned(),
        fingerprint: Fingerprint {
            kind: FingerprintKind::Artifact,
            sha256: ContentHash::from_bytes(FLAT_ROF_FIXTURE_SHA256),
        },
    }])
    .expect("the fixture index has no conflicting observations")
}

/* ------------------------------------------------------------------ */
/* Release-inventory provenance (F01-D)                                */
/* ------------------------------------------------------------------ */

/// The authored-content roots this repository declares for release
/// inventories (F01-D). Committed binary lookalikes are legitimate only
/// beneath them. `fixtures/synthetic` is a protected path — Rally refuses
/// merges that touch it — so membership there is owner-controlled
/// provenance, not an agent's say-so.
pub const AUTHORED_CONTENT_ROOTS: &[&str] = &["fixtures/synthetic"];

/// Why inventory production failed (F01-D).
///
/// Every variant is loud: an inventory that cannot be produced is an error,
/// never an empty — and therefore clean — report.
#[derive(Debug)]
pub enum InventoryError {
    /// A directory walk or file read failed.
    Io {
        /// The path that could not be read.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// `git ls-files` could not be started at all.
    GitSpawn(std::io::Error),
    /// A git command exited without success — for example on a directory
    /// that is not a checkout at all.
    GitFailed {
        /// The command that failed (`rev-parse` or `ls-files`).
        command: &'static str,
        /// The exit code, when one was reported.
        code: Option<i32>,
        /// What git printed to stderr.
        stderr: String,
    },
    /// `root` is inside a checkout but is not its work-tree root. The
    /// committed inventory is defined for a whole checkout: `git ls-files`
    /// under a subdirectory quietly reports only the tracked prefix, so
    /// accepting it would silently produce a partial — possibly empty —
    /// inventory.
    NotCheckoutRoot {
        /// The root the caller asked for.
        root: PathBuf,
        /// The work-tree root git actually resolved.
        toplevel: PathBuf,
    },
    /// A path was not valid UTF-8. The record cannot name it, so skipping
    /// it would silently drop it from the inventory.
    NonUtf8Path(PathBuf),
}

impl fmt::Display for InventoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    f,
                    "cannot read inventory entry {}: {source}",
                    path.display()
                )
            }
            Self::GitSpawn(error) => {
                write!(f, "cannot run `git ls-files`: {error}")
            }
            Self::GitFailed {
                command,
                code,
                stderr,
            } => write!(
                f,
                "`git {command}` failed (exit {code:?}): {}",
                stderr.trim()
            ),
            Self::NotCheckoutRoot { root, toplevel } => write!(
                f,
                "{} is inside a checkout but is not its work-tree root ({})",
                root.display(),
                toplevel.display()
            ),
            Self::NonUtf8Path(path) => write!(
                f,
                "inventory path {} is not valid UTF-8 and cannot be recorded",
                path.display()
            ),
        }
    }
}

impl std::error::Error for InventoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::GitSpawn(error) => Some(error),
            Self::GitFailed { .. } | Self::NotCheckoutRoot { .. } | Self::NonUtf8Path(_) => None,
        }
    }
}

/// The binary sniff behind [`InventoryEntry::text`]: NUL bytes or invalid
/// UTF-8 mean binary content. When the sample is a strict prefix of a
/// longer file, a multi-byte sequence truncated by the sample boundary does
/// not condemn it.
fn is_text_sample(sample: &[u8], complete: bool) -> bool {
    if sample.contains(&0) {
        return false;
    }
    match std::str::from_utf8(sample) {
        Ok(_) => true,
        Err(error) => !complete && error.error_len().is_none(),
    }
}

/// Reads one file into an [`InventoryEntry`]: the leading
/// [`INVENTORY_HEADER_LEN`] bytes for signature checks and a text verdict
/// over up to [`TEXT_SAMPLE_LEN`] bytes. `relative` is the slash-separated
/// path the inventory records.
fn read_entry(root: &Path, relative: &str) -> Result<InventoryEntry, InventoryError> {
    let full = root.join(relative);
    let io = |source: std::io::Error| InventoryError::Io {
        path: full.clone(),
        source,
    };
    let metadata = std::fs::metadata(&full).map_err(io)?;
    let mut sample = Vec::new();
    std::fs::File::open(&full)
        .map_err(io)?
        .take(TEXT_SAMPLE_LEN as u64)
        .read_to_end(&mut sample)
        .map_err(io)?;
    let complete = sample.len() as u64 >= metadata.len();
    Ok(InventoryEntry {
        path: relative.to_owned(),
        len: metadata.len(),
        header: sample[..sample.len().min(INVENTORY_HEADER_LEN)].to_vec(),
        text: is_text_sample(&sample, complete),
    })
}

/// The slash-separated path `path` has relative to `root`, or an error when
/// it cannot be represented in a record.
fn relative_path(root: &Path, path: &Path) -> Result<String, InventoryError> {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let mut parts = Vec::new();
    for component in relative.components() {
        if let std::path::Component::Normal(part) = component {
            parts.push(
                part.to_str()
                    .ok_or_else(|| InventoryError::NonUtf8Path(path.to_path_buf()))?,
            );
        }
    }
    Ok(parts.join("/"))
}

/// Produces an [`InventoryEntry`] for every regular file beneath `root`,
/// with slash-separated paths relative to `root` and the list sorted by
/// path. Symlinks are read through to their target's content — the check
/// judges the bytes, not the link. This is the producer for
/// release-artifact directories and fixture trees; the committed-tree
/// producer is [`committed_inventory`].
pub fn scan_inventory_dir(root: &Path) -> Result<Vec<InventoryEntry>, InventoryError> {
    let mut entries = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let children = std::fs::read_dir(&dir).map_err(|source| InventoryError::Io {
            path: dir.clone(),
            source,
        })?;
        for child in children {
            let child = child.map_err(|source| InventoryError::Io {
                path: dir.clone(),
                source,
            })?;
            let path = child.path();
            // fs::metadata follows symlinks: a link to a regular file is
            // scanned as the content it resolves to.
            let metadata = std::fs::metadata(&path).map_err(|source| InventoryError::Io {
                path: path.clone(),
                source,
            })?;
            if metadata.is_dir() {
                stack.push(path);
            } else if metadata.is_file() {
                let relative = relative_path(root, &path)?;
                entries.push(read_entry(root, &relative)?);
            }
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

/// The committed release inventory of a git checkout (F01-D): the
/// `git ls-files` path set — tracked files only, so ignored build output
/// and private directories never enter it — with content read from the
/// worktree.
///
/// Paths come from `git ls-files -z`, which reports raw unquoted names.
/// The bytes are the worktree's current bytes, so a staged-but-edited
/// binary is scanned as it would actually be committed.
///
/// `root` must be the work-tree root: `git ls-files` under a subdirectory
/// of a checkout exits 0 while reporting only the tracked prefix, so
/// without the check a mispointed root would silently produce a partial —
/// possibly empty — clean inventory. Any git failure is an
/// [`InventoryError`], never a clean report.
pub fn committed_inventory(root: &Path) -> Result<Vec<InventoryEntry>, InventoryError> {
    let git_failed =
        |command: &'static str, output: &std::process::Output| InventoryError::GitFailed {
            command,
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        };
    let toplevel = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(InventoryError::GitSpawn)?;
    if !toplevel.status.success() {
        return Err(git_failed("rev-parse --show-toplevel", &toplevel));
    }
    let toplevel = PathBuf::from(String::from_utf8_lossy(&toplevel.stdout).trim());
    let canonical = |path: &Path| -> Result<PathBuf, InventoryError> {
        std::fs::canonicalize(path).map_err(|source| InventoryError::Io {
            path: path.to_path_buf(),
            source,
        })
    };
    let (requested, resolved) = (canonical(root)?, canonical(&toplevel)?);
    if requested != resolved {
        return Err(InventoryError::NotCheckoutRoot {
            root: root.to_path_buf(),
            toplevel,
        });
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .map_err(InventoryError::GitSpawn)?;
    if !output.status.success() {
        return Err(git_failed("ls-files -z", &output));
    }
    let mut entries = Vec::new();
    for raw in output.stdout.split(|byte| *byte == 0) {
        if raw.is_empty() {
            continue;
        }
        let relative = std::str::from_utf8(raw).map_err(|_| {
            InventoryError::NonUtf8Path(root.join(String::from_utf8_lossy(raw).as_ref()))
        })?;
        entries.push(read_entry(root, relative)?);
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

/// Producer→consumer wiring for the release-inventory check (F01-D): the
/// committed or shipped file set in, a violation report out, using this
/// repository's declared [`AUTHORED_CONTENT_ROOTS`].
///
/// Like [`audit_claims`] this is stateless: nothing is allocated, opened or
/// cached, a refused scan is retried by producing the inventory again, and
/// the report preserves every violation rather than collapsing to a count.
pub fn audit_release_inventory(entries: &[InventoryEntry]) -> InventoryReport {
    check_release_inventory(entries, AUTHORED_CONTENT_ROOTS)
}
