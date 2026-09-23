//! Acceptance scenario F01-C (AC03) for `cs-inspect`: the audit wiring feeds
//! freshly observed fingerprints into ledger validation and reports every
//! recorded disagreement with both sides and its typed adjudication state
//! preserved. Observation conflicts propagate as errors; a refused audit can
//! be retried with corrected input.
//!
//! These tests exercise production code only: `cs_inspect::evidence::
//! audit_claims` over `check_ledger`/`FingerprintIndex` and the canonical
//! `cs_types::evidence` records. Removing or neutering that implementation —
//! dropping the contradiction view, swallowing the index error or skipping
//! ledger validation — makes them fail.

use cs_inspect::evidence::{
    AuditError, audit_claims, synthetic_claim_fixture, synthetic_fingerprint_index,
};
use cs_types::evidence::{
    Adjudication, ClaimError, ClaimId, ClaimRecord, ClaimStatus, ContentHash, EvidenceRecord,
    EvidenceSource, Fingerprint, FingerprintKind, LedgerError, ObservationIndexError,
    ObservationLocator, ObservationMethod, ObservedFingerprint, SourceSpan,
};

const HASH_A: [u8; 32] = [0xaa; 32];
const HASH_B: [u8; 32] = [0xbb; 32];

fn id(name: &str) -> ClaimId {
    ClaimId::new(name).expect("test claim id is valid")
}

fn documented_evidence() -> EvidenceRecord {
    EvidenceRecord {
        source: EvidenceSource::Document("docs/research/SOURCES.md#S01".to_owned()),
        fingerprint: None,
        locator: Some(ObservationLocator {
            container: "doc:SOURCES#S01".to_owned(),
            span: None,
        }),
        method: ObservationMethod::DocumentReview,
        limitations: vec!["secondary documentation, not a byte observation".to_owned()],
    }
}

fn probed_evidence() -> EvidenceRecord {
    EvidenceRecord {
        source: EvidenceSource::ToolRun {
            tool: "reference-probe".to_owned(),
            version: "0.0-test".to_owned(),
        },
        fingerprint: Some(Fingerprint {
            kind: FingerprintKind::Content,
            sha256: ContentHash::from_bytes(HASH_A),
        }),
        locator: Some(ObservationLocator {
            container: "install/data.zbd".to_owned(),
            span: Some(SourceSpan {
                offset: 0x10,
                length: 4,
            }),
        }),
        method: ObservationMethod::ToolProbe,
        limitations: vec!["probe build, not yet original-verified".to_owned()],
    }
}

fn claim(name: &str, status: ClaimStatus, evidence: Vec<EvidenceRecord>) -> ClaimRecord {
    ClaimRecord {
        id: id(name),
        subject: format!("subject of {name}"),
        status,
        evidence,
        test_outcome: None,
        disputes: vec![],
        adjudication: None,
    }
}

/// Two claims asserting different values for the same fact from different
/// sources: one documented, one probed.
fn disagreeing_pair() -> (ClaimRecord, ClaimRecord) {
    let mut documented = claim(
        "test.magic-word.documented",
        ClaimStatus::Documented,
        vec![documented_evidence()],
    );
    documented.subject = "the container magic word is 0x52304C42 per S01".to_owned();

    let mut probed = claim(
        "test.magic-word.probed",
        ClaimStatus::Contradicted,
        vec![probed_evidence()],
    );
    probed.subject = "the container magic word is 0x524F4646 per probe".to_owned();
    probed.disputes = vec![documented.id.clone()];
    probed.adjudication = Some(Adjudication::Open);
    (documented, probed)
}

fn observed(container: &str, kind: FingerprintKind, sha256: [u8; 32]) -> ObservedFingerprint {
    ObservedFingerprint {
        container: container.to_owned(),
        fingerprint: Fingerprint {
            kind,
            sha256: ContentHash::from_bytes(sha256),
        },
    }
}

/// The minimum acceptance scenario: two disagreeing source claims and the
/// adjudication state survive the audit intact. Both records stay in the
/// ledger — the audit report names the dispute and carries the typed state
/// instead of merging the claims or picking the convenient source.
#[test]
fn accept_f01_c_disagreeing_claims_and_adjudication_preserved() {
    let (documented, probed) = disagreeing_pair();
    let documented_id = documented.id.clone();
    let probed_id = probed.id.clone();
    let claims = vec![documented, probed];

    // The probed claim fingerprints install/data.zbd at HASH_A; the fresh
    // observation confirms it unchanged.
    let report = audit_claims(
        &claims,
        vec![observed(
            "install/data.zbd",
            FingerprintKind::Content,
            HASH_A,
        )],
    )
    .expect("the audit must run");

    assert!(
        report.is_clean(),
        "no rule is broken: {:?}",
        report.diagnostic_lines()
    );
    assert_eq!(
        report.ledger.valid,
        [documented_id.clone(), probed_id.clone()],
        "both sides of the disagreement still stand, in input order"
    );

    assert_eq!(report.contradictions.len(), 1);
    let contradiction = &report.contradictions[0];
    assert_eq!(contradiction.claim, probed_id);
    assert_eq!(contradiction.edges.len(), 1);
    assert_eq!(contradiction.edges[0].disputed, documented_id);
    assert!(
        contradiction.edges[0].present,
        "the disputed claim is in the audited set"
    );
    assert_eq!(
        contradiction.adjudication,
        Some(Adjudication::Open),
        "the adjudication state is preserved verbatim"
    );

    // The input is untouched: both disagreeing claims keep their records.
    assert_eq!(claims.len(), 2);
    assert_eq!(claims[1].status, ClaimStatus::Contradicted);
}

/// A ruling narrows the disagreement but deletes nothing: the disputed claim
/// keeps its record and standing, and the report shows which side the ruling
/// kept — on whichever side it landed.
#[test]
fn accept_f01_c_ruled_adjudication_keeps_both_sides() {
    let (documented, mut probed) = disagreeing_pair();
    let documented_id = documented.id.clone();
    probed.adjudication = Some(Adjudication::Ruled {
        upholds: documented_id.clone(),
        rationale: "probe build mismatched the container it hashed".to_owned(),
    });
    let probed_id = probed.id.clone();
    let claims = vec![documented, probed];

    let report = audit_claims(
        &claims,
        vec![observed(
            "install/data.zbd",
            FingerprintKind::Content,
            HASH_A,
        )],
    )
    .expect("the audit must run");

    assert_eq!(report.contradictions.len(), 1);
    assert_eq!(
        report.contradictions[0].adjudication,
        Some(Adjudication::Ruled {
            upholds: documented_id.clone(),
            rationale: "probe build mismatched the container it hashed".to_owned(),
        }),
        "the ruling and its rationale are preserved"
    );
    assert!(
        report.ledger.valid.contains(&documented_id) && report.ledger.valid.contains(&probed_id),
        "a ruling upholds a side; it never erases the other"
    );

    // A ruling may also keep the contradicted claim itself standing.
    let (documented, mut probed) = disagreeing_pair();
    probed.adjudication = Some(Adjudication::Ruled {
        upholds: probed.id.clone(),
        rationale: "the document describes a different revision".to_owned(),
    });
    let claims = vec![documented, probed];
    let report = audit_claims(
        &claims,
        vec![observed(
            "install/data.zbd",
            FingerprintKind::Content,
            HASH_A,
        )],
    )
    .expect("the audit must run");
    assert!(report.is_clean(), "{:?}", report.diagnostic_lines());
}

/// Error propagation: two disagreeing observations of one container abort the
/// audit with the index error — they are never folded into an index that
/// would report every dependency unchecked. Retrying with consistent
/// observations succeeds; the refused run left nothing behind.
#[test]
fn accept_f01_c_conflicting_observations_propagate_and_retry() {
    let claims = synthetic_claim_fixture();
    let conflict = vec![
        observed(
            "fixtures/synthetic/flat-uncompressed.rof",
            FingerprintKind::Artifact,
            HASH_A,
        ),
        observed(
            "fixtures/synthetic/flat-uncompressed.rof",
            FingerprintKind::Artifact,
            HASH_B,
        ),
    ];

    let error = audit_claims(&claims, conflict).expect_err("conflicting observations must abort");
    assert_eq!(
        error,
        AuditError::ConflictingObservations(ObservationIndexError::ConflictingObservations {
            kind: FingerprintKind::Artifact,
            container: "fixtures/synthetic/flat-uncompressed.rof".to_owned(),
        }),
        "the producer's error is propagated, not swallowed"
    );

    // Retry with the corrected observation: nothing from the failed run
    // poisons the next audit.
    let retry = audit_claims(
        &claims,
        synthetic_fingerprint_index().observations().to_vec(),
    );
    assert!(
        retry.is_ok_and(|report| report.is_clean()),
        "a corrected retry must succeed and stay clean"
    );
}

/// The audit wires the real ledger path end to end: a fingerprint that
/// changed since the claim was recorded invalidates the dependent claim and
/// names both digests — not a parallel, always-clean check.
#[test]
fn accept_f01_c_audit_surfaces_invalidation() {
    let fixture = synthetic_claim_fixture();
    let clean = audit_claims(
        &fixture,
        synthetic_fingerprint_index().observations().to_vec(),
    )
    .expect("the audit must run");
    assert!(clean.is_clean());

    let stale = audit_claims(
        &fixture,
        vec![observed(
            "fixtures/synthetic/flat-uncompressed.rof",
            FingerprintKind::Artifact,
            HASH_A,
        )],
    )
    .expect("the audit must run");
    assert!(!stale.is_clean());
    assert_eq!(stale.ledger.invalidated.len(), 1);
    assert_eq!(
        stale.ledger.invalidated[0].claim,
        id("fixture.rof.flat-uncompressed")
    );
    assert!(
        stale
            .diagnostic_lines()
            .iter()
            .any(|line| line.contains("fixture.rof.flat-uncompressed")),
        "diagnostics reach the consumer unchanged"
    );
}

/// A `contradicted` claim without an adjudication state is a defect, not a
/// legal record: AC03 preserves the state, so the ledger refuses the claim
/// by name — and the audit still lists the contradiction so it stays visible.
#[test]
fn accept_f01_c_contradicted_without_adjudication_is_rejected() {
    let (documented, mut probed) = disagreeing_pair();
    probed.adjudication = None;
    let claims = vec![documented, probed];

    let report = audit_claims(&claims, vec![]).expect("the audit must run");
    assert!(
        report
            .ledger
            .rejected
            .iter()
            .any(|rejection| rejection.claim == id("test.magic-word.probed")
                && rejection.error
                    == LedgerError::Record(ClaimError::ContradictionWithoutAdjudication)),
        "missing adjudication must be rejected by name: {:?}",
        report.ledger.rejected
    );
    assert_eq!(
        report.contradictions.len(),
        1,
        "even a rejected contradiction stays visible in the report"
    );
    assert_eq!(report.contradictions[0].adjudication, None);
}

/// Adjudication belongs to a dispute: a ruling that names a non-party, an
/// empty rationale, or an adjudication on a claim that disputes nothing are
/// each refused by name.
#[test]
fn accept_f01_c_adjudication_rules_are_enforced() {
    let (documented, mut probed) = disagreeing_pair();
    probed.adjudication = Some(Adjudication::Ruled {
        upholds: id("test.unrelated"),
        rationale: "picked a claim outside the dispute".to_owned(),
    });
    assert_eq!(
        probed.validate(),
        Err(ClaimError::RulingOutsideDispute {
            upheld: id("test.unrelated")
        }),
        "a ruling must uphold a party to the dispute"
    );
    let _ = documented;

    let (_, mut probed) = disagreeing_pair();
    probed.adjudication = Some(Adjudication::Ruled {
        upholds: probed.id.clone(),
        rationale: "   ".to_owned(),
    });
    assert_eq!(
        probed.validate(),
        Err(ClaimError::EmptyRationale),
        "a ruling must carry its reasoning"
    );

    let mut adjudicated = claim("test.no-dispute", ClaimStatus::Designed, vec![]);
    adjudicated.adjudication = Some(Adjudication::Open);
    assert_eq!(
        adjudicated.validate(),
        Err(ClaimError::AdjudicationWithoutDispute),
        "adjudication without a recorded dispute is meaningless"
    );
}
