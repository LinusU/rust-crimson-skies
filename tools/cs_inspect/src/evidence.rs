//! Claim-record admission checks and ledger validation for the evidence
//! ledger (F01-A, F01-B).
//!
//! `cs-inspect` owns command-line inspection and conversion diagnostics. This
//! module is the inspector's front end for the canonical records in
//! [`cs_types::evidence`]: [`check_claims`] runs the per-record admission
//! rules, and [`check_ledger`] runs the full ledger validation — set-level
//! rules plus dependency invalidation against freshly observed fingerprints.
//! The `audit` command wiring arrives with F01-C.

use cs_types::evidence::{
    ClaimError, ClaimId, ClaimRecord, ClaimStatus, ContentHash, EvidenceRecord, EvidenceSource,
    Fingerprint, FingerprintIndex, FingerprintKind, LedgerReport, ObservationLocator,
    ObservationMethod, ObservedFingerprint, SourceSpan,
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
