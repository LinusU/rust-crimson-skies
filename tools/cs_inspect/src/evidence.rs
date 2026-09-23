//! Claim-record admission checks for the evidence ledger (F01-A).
//!
//! `cs-inspect` owns command-line inspection and conversion diagnostics. This
//! module is the inspector's front end for the canonical records in
//! [`cs_types::evidence`]: it checks a set of claims and reports which were
//! admitted and which were rejected, naming the rejecting error per claim.
//! Set-level ledger rules — duplicate ids, dependency invalidation when a
//! fingerprint changes — arrive with F01-B, and the `audit` command wiring
//! with F01-C.

use cs_types::evidence::{
    ClaimError, ClaimId, ClaimRecord, ClaimStatus, ContentHash, EvidenceRecord, EvidenceSource,
    Fingerprint, FingerprintKind, ObservationLocator, ObservationMethod, SourceSpan,
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
