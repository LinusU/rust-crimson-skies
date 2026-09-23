//! Acceptance scenario F01-A for `cs-inspect`: the claim-record admission
//! check rejects a `verified_original` claim that lacks a content hash and
//! observation locator — by name, never silently — while admitting the
//! synthetic fixture set.
//!
//! These tests exercise production code only: `cs_inspect::evidence::
//! check_claims`, `ClaimReport` and `synthetic_claim_fixture` over the
//! canonical `cs_types::evidence` records. Removing or neutering that
//! implementation makes them fail.

use cs_inspect::evidence::{check_claims, synthetic_claim_fixture};
use cs_types::evidence::{
    ClaimError, ClaimId, ClaimRecord, ClaimStatus, ContentHash, EvidenceRecord, EvidenceSource,
    Fingerprint, FingerprintKind, ObservationLocator, ObservationMethod, SourceSpan,
};

fn fixture_evidence() -> EvidenceRecord {
    EvidenceRecord {
        source: EvidenceSource::SyntheticFixture,
        fingerprint: Some(Fingerprint {
            kind: FingerprintKind::Artifact,
            sha256: ContentHash::from_bytes([0x11; 32]),
        }),
        locator: Some(ObservationLocator {
            container: "fixtures/synthetic/example.bin".to_owned(),
            span: Some(SourceSpan {
                offset: 0,
                length: 16,
            }),
        }),
        method: ObservationMethod::ByteInspection,
        limitations: vec!["synthetic fixture, not original data".to_owned()],
    }
}

fn overclaimed(id: &str, evidence: Vec<EvidenceRecord>) -> ClaimRecord {
    ClaimRecord {
        id: ClaimId::new(id).expect("test claim id is valid"),
        subject: "claims original behavior without original evidence".to_owned(),
        status: ClaimStatus::VerifiedOriginal,
        evidence,
        test_outcome: None,
        disputes: vec![],
        adjudication: None,
    }
}

/// The shipped fixture set must pass the admission check cleanly and keep
/// input order — a stub that rejects everything fails here.
#[test]
fn accept_f01_a_synthetic_claim_fixture_is_admitted() {
    let fixture = synthetic_claim_fixture();
    assert_eq!(fixture.len(), 3, "the fixture must carry its three claims");
    let report = check_claims(&fixture);
    assert!(
        report.is_clean(),
        "fixture claims must all be admitted: {:?}",
        report.diagnostic_lines()
    );
    let expected: Vec<ClaimId> = fixture.iter().map(|claim| claim.id.clone()).collect();
    assert_eq!(report.admitted, expected, "admitted ids keep input order");
}

/// The minimum acceptance scenario through the inspector front end: a
/// `verified_original` claim missing the content hash is rejected by name.
#[test]
fn accept_f01_a_unverified_original_is_rejected_by_name() {
    let mut evidence = fixture_evidence();
    evidence.fingerprint = None;
    let claims = vec![overclaimed("test.unverified", vec![evidence])];

    let report = check_claims(&claims);
    assert!(
        !report.is_clean(),
        "an unverifiable claim must dirty the report"
    );
    assert_eq!(report.admitted.len(), 0);
    assert_eq!(
        report.rejected.as_slice(),
        [cs_inspect::evidence::ClaimRejection {
            id: ClaimId::new("test.unverified").expect("valid id"),
            error: ClaimError::UnverifiedOriginalEvidence,
        }],
        "the rejection must name the claim and its reason"
    );
    let diagnostics = report.diagnostic_lines();
    assert_eq!(diagnostics.len(), 1);
    assert!(
        diagnostics[0].contains("test.unverified") && diagnostics[0].contains("rejected"),
        "diagnostics must name the rejected claim: {diagnostics:?}"
    );
}

/// Synthetic fixture evidence — even with a hash and a locator — can never
/// back `verified_original`; synthetic observations prove nothing about
/// retail data.
#[test]
fn accept_f01_a_synthetic_fixture_evidence_cannot_verify() {
    let claims = vec![overclaimed(
        "test.fixture-as-original",
        vec![fixture_evidence()],
    )];
    let report = check_claims(&claims);
    assert_eq!(
        report.rejected.len(),
        1,
        "fixture-backed verified_original must be rejected"
    );
    assert_eq!(
        report.rejected[0].error,
        ClaimError::UnverifiedOriginalEvidence
    );
}

/// A mixed set splits correctly: good claims stay admitted in order, bad
/// ones are named. A check that always passes or always fails cannot satisfy
/// this.
#[test]
fn accept_f01_a_mixed_set_keeps_order_and_names_failures() {
    let mut claims = synthetic_claim_fixture();
    let good_ids: Vec<ClaimId> = claims.iter().map(|claim| claim.id.clone()).collect();
    let mut bad = overclaimed("test.bad", vec![]);
    claims.insert(1, bad.clone());
    bad.id = ClaimId::new("test.bad2").expect("valid id");
    claims.push(bad);

    let report = check_claims(&claims);
    assert_eq!(report.admitted, good_ids, "admissions keep input order");
    let rejected_ids: Vec<&ClaimId> = report.rejected.iter().map(|r| &r.id).collect();
    assert_eq!(
        rejected_ids,
        [
            &ClaimId::new("test.bad").expect("valid id"),
            &ClaimId::new("test.bad2").expect("valid id")
        ],
        "rejections keep input order and name every offender"
    );
    assert!(
        report.rejected.iter().all(|r| r.error
            == ClaimError::MissingEvidence {
                status: ClaimStatus::VerifiedOriginal
            }),
        "both offenders lack evidence entirely"
    );
}
