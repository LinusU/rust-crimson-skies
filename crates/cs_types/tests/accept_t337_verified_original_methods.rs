//! Acceptance tests for Rally task #337: `verified_original` evidence must
//! be a *direct observation* of fingerprinted original data. `Authored`,
//! `Inference` and `DocumentReview` methods cannot verify originality even
//! with source `OriginalInstallation`, a content or installation
//! fingerprint and an observation locator.
//!
//! These tests exercise production code only:
//! [`EvidenceRecord::verifies_original`], [`ObservationMethod::is_direct_observation`]
//! and [`ClaimRecord::validate`]. Removing the method constraint makes them
//! fail.

use cs_types::evidence::{
    ClaimError, ClaimId, ClaimRecord, ClaimStatus, ContentHash, EvidenceRecord, EvidenceSource,
    Fingerprint, FingerprintKind, ObservationLocator, ObservationMethod, SourceSpan,
};

/// A content hash standing in for fingerprinted original data in tests.
fn original_hash() -> ContentHash {
    ContentHash::from_bytes([0x5a; 32])
}

/// Evidence that is admissible for `verified_original` in every respect
/// except the caller-chosen method: original-installation source,
/// original-data fingerprint and an observation locator.
fn original_evidence_with(method: ObservationMethod, kind: FingerprintKind) -> EvidenceRecord {
    EvidenceRecord {
        source: EvidenceSource::OriginalInstallation,
        fingerprint: Some(Fingerprint {
            kind,
            sha256: original_hash(),
        }),
        locator: Some(ObservationLocator {
            container: "data/example.zbd:member.bin".to_owned(),
            span: Some(SourceSpan {
                offset: 0x40,
                length: 0x20,
            }),
        }),
        method,
        limitations: vec!["synthetic test stand-in, not a real observation".to_owned()],
    }
}

fn verified_claim(evidence: Vec<EvidenceRecord>) -> ClaimRecord {
    ClaimRecord {
        id: ClaimId::new("test.claim").expect("test claim id is valid"),
        subject: "a factual statement under test".to_owned(),
        status: ClaimStatus::VerifiedOriginal,
        evidence,
        test_outcome: None,
        disputes: vec![],
        adjudication: None,
    }
}

/// The minimum acceptance scenario: `Authored` and `Inference` methods can
/// never back `verified_original`, even with `OriginalInstallation` source,
/// a content or installation fingerprint and a locator.
#[test]
fn accept_t337_authored_and_inferred_methods_cannot_verify() {
    for method in [ObservationMethod::Authored, ObservationMethod::Inference] {
        for kind in [FingerprintKind::Content, FingerprintKind::Installation] {
            let evidence = original_evidence_with(method, kind);
            assert!(
                !evidence.verifies_original(),
                "{method:?} over a {kind} fingerprint must not verify originality"
            );
            assert_eq!(
                verified_claim(vec![evidence]).validate(),
                Err(ClaimError::UnverifiedOriginalEvidence),
                "a verified_original claim backed only by {method:?} must be rejected"
            );
        }
    }
}

/// `DocumentReview` restates what a source says; it is not a direct
/// observation of the data and cannot back `verified_original` — not even
/// when the record claims the original installation as its source.
#[test]
fn accept_t337_document_review_does_not_verify_original() {
    let evidence = original_evidence_with(
        ObservationMethod::DocumentReview,
        FingerprintKind::Installation,
    );
    assert!(
        !evidence.verifies_original(),
        "a document review must not verify originality"
    );
    assert_eq!(
        verified_claim(vec![evidence]).validate(),
        Err(ClaimError::UnverifiedOriginalEvidence),
        "verified_original on document review alone must be rejected"
    );
}

/// The positive half: the three direct-observation methods still verify
/// originality over both original-data fingerprint kinds.
#[test]
fn accept_t337_direct_observation_methods_verify_original() {
    for method in [
        ObservationMethod::ByteInspection,
        ObservationMethod::ToolProbe,
        ObservationMethod::RuntimeObservation,
    ] {
        assert!(
            method.is_direct_observation(),
            "{method:?} must be classified as a direct observation"
        );
        for kind in [FingerprintKind::Content, FingerprintKind::Installation] {
            let evidence = original_evidence_with(method, kind);
            assert!(
                evidence.verifies_original(),
                "{method:?} over a {kind} fingerprint must verify originality"
            );
            assert_eq!(
                verified_claim(vec![evidence]).validate(),
                Ok(()),
                "verified_original backed by {method:?} must be admitted"
            );
        }
    }
}

/// The method classification is total: every non-direct method is excluded,
/// so a newly added variant cannot silently become admissible.
#[test]
fn accept_t337_only_direct_methods_are_admissible() {
    for (method, direct) in [
        (ObservationMethod::DocumentReview, false),
        (ObservationMethod::ByteInspection, true),
        (ObservationMethod::ToolProbe, true),
        (ObservationMethod::RuntimeObservation, true),
        (ObservationMethod::Inference, false),
        (ObservationMethod::Authored, false),
    ] {
        assert_eq!(
            method.is_direct_observation(),
            direct,
            "unexpected direct-observation verdict for {method:?}"
        );
    }
}

/// A claim mixing weak records with one direct observation is still
/// admitted: the rule constrains what *supports* the status, it does not
/// forbid secondary records alongside the verifying one.
#[test]
fn accept_t337_direct_record_among_weak_ones_still_verifies() {
    let evidence = vec![
        original_evidence_with(ObservationMethod::Authored, FingerprintKind::Content),
        original_evidence_with(ObservationMethod::Inference, FingerprintKind::Content),
        original_evidence_with(ObservationMethod::ByteInspection, FingerprintKind::Content),
    ];
    assert_eq!(
        verified_claim(evidence).validate(),
        Ok(()),
        "one direct observation among non-observing records must still verify"
    );
}
