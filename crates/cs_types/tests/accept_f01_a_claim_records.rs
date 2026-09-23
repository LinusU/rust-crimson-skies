//! Acceptance scenario F01-A (AC01): a `verified_original` claim without a
//! content hash and observation locator is rejected — plus the failure cases
//! around the record rules.
//!
//! These tests exercise production code only: [`ClaimRecord::validate`] and
//! the typed record constructors in `cs_types::evidence`. Removing or
//! neutering that implementation makes them fail.

use cs_types::evidence::{
    ClaimError, ClaimId, ClaimIdError, ClaimRecord, ClaimStatus, ContentHash, EvidenceRecord,
    EvidenceSource, Fingerprint, FingerprintKind, HashError, MAX_CLAIM_ID_LEN, ObservationLocator,
    ObservationMethod, SourceSpan, TestOutcome,
};

/// A content hash standing in for fingerprinted original data in tests.
fn original_hash() -> ContentHash {
    ContentHash::from_bytes([0x5a; 32])
}

/// Evidence that fully supports `verified_original`: original-content
/// fingerprint plus an observation locator.
fn original_evidence() -> EvidenceRecord {
    EvidenceRecord {
        source: EvidenceSource::OriginalInstallation,
        fingerprint: Some(Fingerprint {
            kind: FingerprintKind::Content,
            sha256: original_hash(),
        }),
        locator: Some(ObservationLocator {
            container: "data/example.zbd:member.bin".to_owned(),
            span: Some(SourceSpan {
                offset: 0x40,
                length: 0x20,
            }),
        }),
        method: ObservationMethod::ByteInspection,
        limitations: vec!["synthetic test stand-in, not a real observation".to_owned()],
    }
}

fn claim(status: ClaimStatus, evidence: Vec<EvidenceRecord>) -> ClaimRecord {
    ClaimRecord {
        id: ClaimId::new("test.claim").expect("test claim id is valid"),
        subject: "a factual statement under test".to_owned(),
        status,
        evidence,
        test_outcome: None,
        disputes: vec![],
        adjudication: None,
    }
}

/// The minimum acceptance scenario: `verified_original` without a content
/// hash and observation locator must be rejected.
#[test]
fn accept_f01_a_verified_original_needs_hash_and_locator() {
    // No fingerprint at all.
    let mut no_hash = original_evidence();
    no_hash.fingerprint = None;
    assert_eq!(
        claim(ClaimStatus::VerifiedOriginal, vec![no_hash]).validate(),
        Err(ClaimError::UnverifiedOriginalEvidence),
        "verified_original without a content hash must be rejected"
    );

    // Fingerprint present, but no observation locator.
    let mut no_locator = original_evidence();
    no_locator.locator = None;
    assert_eq!(
        claim(ClaimStatus::VerifiedOriginal, vec![no_locator]).validate(),
        Err(ClaimError::UnverifiedOriginalEvidence),
        "verified_original without an observation locator must be rejected"
    );

    // No evidence at all.
    assert_eq!(
        claim(ClaimStatus::VerifiedOriginal, vec![]).validate(),
        Err(ClaimError::MissingEvidence {
            status: ClaimStatus::VerifiedOriginal
        }),
        "verified_original with zero evidence must be rejected"
    );
}

/// The positive half of the scenario: the same claim, fully backed, is
/// admitted. This is what a stub that rejects everything cannot pass.
#[test]
fn accept_f01_a_fully_backed_verified_original_is_admitted() {
    assert_eq!(
        claim(ClaimStatus::VerifiedOriginal, vec![original_evidence()]).validate(),
        Ok(()),
        "verified_original with a content hash and a locator must be admitted"
    );

    // An installation fingerprint also identifies original data.
    let mut install_evidence = original_evidence();
    install_evidence.fingerprint = Some(Fingerprint {
        kind: FingerprintKind::Installation,
        sha256: original_hash(),
    });
    assert_eq!(
        claim(ClaimStatus::VerifiedOriginal, vec![install_evidence]).validate(),
        Ok(()),
        "an installation fingerprint must also back verified_original"
    );
}

/// Fingerprints of produced artifacts are not original data, and synthetic
/// fixture content can never verify originality — the shortcut "any hash plus
/// any locator verifies" must fail.
#[test]
fn accept_f01_a_non_original_fingerprints_do_not_verify() {
    let mut artifact = original_evidence();
    artifact.fingerprint = Some(Fingerprint {
        kind: FingerprintKind::Artifact,
        sha256: original_hash(),
    });
    assert_eq!(
        claim(ClaimStatus::VerifiedOriginal, vec![artifact]).validate(),
        Err(ClaimError::UnverifiedOriginalEvidence),
        "an artifact fingerprint must not back verified_original"
    );

    // Even if a recorder mislabels a synthetic fixture's hash as content, the
    // fixture source disqualifies it.
    let mut fixture = original_evidence();
    fixture.source = EvidenceSource::SyntheticFixture;
    assert_eq!(
        claim(ClaimStatus::VerifiedOriginal, vec![fixture]).validate(),
        Err(ClaimError::UnverifiedOriginalEvidence),
        "synthetic fixture evidence must never back verified_original"
    );

    // Hash and locator must live on the same observation: two records each
    // carrying only one half do not combine into verification.
    let mut hash_only = original_evidence();
    hash_only.locator = None;
    let mut locator_only = original_evidence();
    locator_only.fingerprint = None;
    assert_eq!(
        claim(ClaimStatus::VerifiedOriginal, vec![hash_only, locator_only]).validate(),
        Err(ClaimError::UnverifiedOriginalEvidence),
        "split hash/locator records must not combine into verified_original"
    );
}

/// Automated test success is a separate field: a green test outcome cannot
/// rescue a claim whose evidence cannot verify originality.
#[test]
fn accept_f01_a_passing_tests_do_not_upgrade_status() {
    let mut claim = claim(ClaimStatus::VerifiedOriginal, vec![]);
    claim.test_outcome = Some(TestOutcome {
        selector: "accept_f01_a_*".to_owned(),
        passed: 42,
        failed: 0,
    });
    assert!(
        claim.validate().is_err(),
        "a verified_original claim with passing tests but no evidence must still be rejected"
    );
}

/// Contradictions are preserved structurally: `contradicted` must name the
/// claims it disagrees with, and a claim cannot dispute itself.
#[test]
fn accept_f01_a_contradicted_claim_must_name_its_disputes() {
    assert_eq!(
        claim(ClaimStatus::Contradicted, vec![]).validate(),
        Err(ClaimError::ContradictionWithoutDispute),
        "contradicted without dispute links must be rejected"
    );

    let mut disputed = claim(ClaimStatus::Contradicted, vec![]);
    disputed.disputes = vec![ClaimId::new("test.other-claim").expect("valid id")];
    disputed.adjudication = Some("awaiting adjudication".to_owned());
    assert_eq!(
        disputed.validate(),
        Ok(()),
        "contradicted claims keep their dispute links and adjudication state"
    );

    let mut self_dispute = claim(ClaimStatus::Contradicted, vec![]);
    self_dispute.disputes = vec![self_dispute.id.clone()];
    assert_eq!(
        self_dispute.validate(),
        Err(ClaimError::SelfDispute),
        "a claim must not dispute itself"
    );
}

/// Statuses that claim support need evidence; `designed` and `unknown` may
/// stand without it.
#[test]
fn accept_f01_a_evidence_requiring_statuses() {
    for status in [
        ClaimStatus::Documented,
        ClaimStatus::ObservedTool,
        ClaimStatus::Inferred,
    ] {
        assert_eq!(
            claim(status, vec![]).validate(),
            Err(ClaimError::MissingEvidence { status }),
            "a {status} claim with no evidence must be rejected"
        );
    }
    for status in [ClaimStatus::Designed, ClaimStatus::Unknown] {
        assert_eq!(
            claim(status, vec![]).validate(),
            Ok(()),
            "a {status} claim may stand without evidence"
        );
    }
    assert_eq!(ClaimStatus::VerifiedOriginal.label(), "verified_original");
    assert_eq!(ClaimStatus::ObservedTool.label(), "observed_tool");
}

/// Record hygiene: empty subjects, containers, spans and limitations are
/// rejected by name.
#[test]
fn accept_f01_a_record_hygiene_rejects_empty_fields() {
    let mut empty_subject = claim(ClaimStatus::Designed, vec![]);
    empty_subject.subject = "   ".to_owned();
    assert_eq!(empty_subject.validate(), Err(ClaimError::EmptySubject));

    let mut empty_container = claim(ClaimStatus::Designed, vec![original_evidence()]);
    empty_container.evidence[0]
        .locator
        .as_mut()
        .expect("locator present")
        .container = " ".to_owned();
    assert_eq!(
        empty_container.validate(),
        Err(ClaimError::EmptyLocatorContainer)
    );

    let mut zero_span = claim(ClaimStatus::Designed, vec![original_evidence()]);
    zero_span.evidence[0]
        .locator
        .as_mut()
        .expect("locator present")
        .span = Some(SourceSpan {
        offset: 4,
        length: 0,
    });
    assert_eq!(zero_span.validate(), Err(ClaimError::ZeroLengthSpan));

    let mut empty_limitation = claim(ClaimStatus::Designed, vec![original_evidence()]);
    empty_limitation.evidence[0].limitations = vec![String::new()];
    assert_eq!(
        empty_limitation.validate(),
        Err(ClaimError::EmptyLimitation)
    );
}

/// `ContentHash` hex parsing follows the evidence schema: exactly 64
/// lowercase hex characters, nothing else.
#[test]
fn accept_f01_a_content_hash_hex_rules() {
    let hex = "97a2d7565f88be3b53204ab0e6eb5e39fff886c080156f97a9e5365f2743a123";
    let hash = ContentHash::from_hex(hex).expect("valid lowercase hex must decode");
    assert_eq!(hash.to_hex(), hex, "hex form must round-trip");

    assert_eq!(
        ContentHash::from_hex("abc"),
        Err(HashError::BadLength { len: 3 }),
        "short hex must be rejected"
    );
    let long_upper = "97A2D7565F88BE3B53204AB0E6EB5E39FFF886C080156F97A9E5365F2743A123";
    assert!(matches!(
        ContentHash::from_hex(long_upper),
        Err(HashError::BadCharacter { .. })
    ));
    let bad_char = format!("{}g", &hex[..63]);
    assert!(matches!(
        ContentHash::from_hex(&bad_char),
        Err(HashError::BadCharacter { index: 63, .. })
    ));
}

/// `ClaimId` grammar: bounded length, ASCII alphanumerics plus `. _ - : /`.
#[test]
fn accept_f01_a_claim_id_grammar() {
    assert!(ClaimId::new("f05.rof.header-magic").is_ok());
    assert!(ClaimId::new("m01/objective.spawn").is_ok());
    assert_eq!(ClaimId::new(""), Err(ClaimIdError::Empty));
    assert!(matches!(
        ClaimId::new("has space"),
        Err(ClaimIdError::BadCharacter { ch: ' ' })
    ));
    let too_long = "a".repeat(MAX_CLAIM_ID_LEN + 1);
    assert_eq!(
        ClaimId::new(&too_long),
        Err(ClaimIdError::TooLong {
            len: MAX_CLAIM_ID_LEN + 1
        })
    );
}
