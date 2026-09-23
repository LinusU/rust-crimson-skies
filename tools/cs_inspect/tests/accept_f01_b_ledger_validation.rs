//! Acceptance scenario F01-B for `cs-inspect`: ledger validation over a claim
//! set — duplicate ids and dangling disputes are rejected by name, and a
//! changed asset fingerprint invalidates every claim that depends on it while
//! leaving the claim record itself untouched.
//!
//! These tests exercise production code only:
//! `cs_inspect::evidence::check_ledger` delegating to
//! `cs_types::evidence::validate_ledger`, `FingerprintIndex` and the
//! synthetic fixture pair. Removing or neutering the ledger rules makes them
//! fail.

use cs_inspect::evidence::{check_ledger, synthetic_claim_fixture, synthetic_fingerprint_index};
use cs_types::evidence::{
    ClaimId, ClaimRecord, ClaimStatus, ContentHash, EvidenceRecord, EvidenceSource, Fingerprint,
    FingerprintIndex, FingerprintKind, LedgerError, ObservationIndexError, ObservationLocator,
    ObservationMethod, ObservedFingerprint, SourceSpan, UncheckedReason,
};

const HASH_A: [u8; 32] = [0xaa; 32];
const HASH_B: [u8; 32] = [0xbb; 32];
const HASH_C: [u8; 32] = [0xcc; 32];

fn fingerprinted_evidence(
    source: EvidenceSource,
    kind: FingerprintKind,
    sha256: [u8; 32],
    container: &str,
) -> EvidenceRecord {
    EvidenceRecord {
        source,
        fingerprint: Some(Fingerprint {
            kind,
            sha256: ContentHash::from_bytes(sha256),
        }),
        locator: Some(ObservationLocator {
            container: container.to_owned(),
            span: Some(SourceSpan {
                offset: 0,
                length: 16,
            }),
        }),
        method: ObservationMethod::ByteInspection,
        limitations: vec![],
    }
}

fn claim(id: &str, status: ClaimStatus, evidence: Vec<EvidenceRecord>) -> ClaimRecord {
    ClaimRecord {
        id: ClaimId::new(id).expect("test claim id is valid"),
        subject: format!("subject of {id}"),
        status,
        evidence,
        test_outcome: None,
        disputes: vec![],
        adjudication: None,
    }
}

fn index_of(observations: Vec<ObservedFingerprint>) -> FingerprintIndex {
    FingerprintIndex::from_observations(observations).expect("test index has no conflicts")
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

/// Baseline: the fixture set validated against the index that re-observes its
/// only fingerprinted asset unchanged is completely clean.
#[test]
fn accept_f01_b_fixture_claims_validate_clean_against_current_index() {
    let fixture = synthetic_claim_fixture();
    let report = check_ledger(&fixture, &synthetic_fingerprint_index());
    assert!(
        report.is_clean(),
        "a matching index must leave the fixture clean: {:?}",
        report.diagnostic_lines()
    );
    let expected: Vec<ClaimId> = fixture.iter().map(|claim| claim.id.clone()).collect();
    assert_eq!(report.valid, expected, "all fixture claims stand, in order");
}

/// The minimum acceptance scenario: after the asset's fingerprint changes,
/// the claim whose evidence recorded the old digest is invalidated by name
/// with both digests, and claims that do not depend on it still stand.
#[test]
fn accept_f01_b_changed_fingerprint_invalidates_dependent_claim() {
    let fixture = synthetic_claim_fixture();
    let dependent = ClaimId::new("fixture.rof.flat-uncompressed").expect("valid id");

    let before = check_ledger(&fixture, &synthetic_fingerprint_index());
    assert!(before.invalidated.is_empty());
    assert!(before.valid.contains(&dependent));

    // The same container is re-observed with a different digest.
    let changed = index_of(vec![observed(
        "fixtures/synthetic/flat-uncompressed.rof",
        FingerprintKind::Artifact,
        HASH_A,
    )]);
    let after = check_ledger(&fixture, &changed);

    assert!(
        !after.valid.contains(&dependent),
        "the dependent claim must no longer stand"
    );
    assert_eq!(after.invalidated.len(), 1);
    assert_eq!(after.invalidated[0].claim, dependent);
    assert_eq!(after.invalidated[0].stale.len(), 1);
    let stale = &after.invalidated[0].stale[0];
    assert_eq!(stale.container, "fixtures/synthetic/flat-uncompressed.rof");
    assert_eq!(stale.kind, FingerprintKind::Artifact);
    assert_eq!(stale.observed, ContentHash::from_bytes(HASH_A));
    assert_ne!(
        stale.recorded, stale.observed,
        "the stale entry must record the digest the claim was built on"
    );

    let still_valid: Vec<ClaimId> = fixture
        .iter()
        .map(|claim| claim.id.clone())
        .filter(|id| *id != dependent)
        .collect();
    assert_eq!(
        after.valid, still_valid,
        "claims not depending on the changed asset stay valid"
    );
    assert!(!after.is_clean());
    let diagnostics = after.diagnostic_lines();
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("fixture.rof.flat-uncompressed")
                && line.contains("invalidated")),
        "diagnostics must name the invalidated claim: {diagnostics:?}"
    );
}

/// Invalidation applies equally to `verified_original` claims: a changed
/// content fingerprint revokes the approval without rewriting the record.
#[test]
fn accept_f01_b_verified_original_invalidated_by_content_change() {
    let claims = vec![claim(
        "test.content-closure",
        ClaimStatus::VerifiedOriginal,
        vec![fingerprinted_evidence(
            EvidenceSource::OriginalInstallation,
            FingerprintKind::Content,
            HASH_A,
            "install/game.zbd",
        )],
    )];

    let matching = index_of(vec![observed(
        "install/game.zbd",
        FingerprintKind::Content,
        HASH_A,
    )]);
    assert!(check_ledger(&claims, &matching).is_clean());

    let changed = index_of(vec![observed(
        "install/game.zbd",
        FingerprintKind::Content,
        HASH_B,
    )]);
    let report = check_ledger(&claims, &changed);
    assert_eq!(report.invalidated.len(), 1);
    assert_eq!(
        report.invalidated[0].claim,
        ClaimId::new("test.content-closure").expect("valid id")
    );
    assert_eq!(
        claims[0].status,
        ClaimStatus::VerifiedOriginal,
        "invalidation must not rewrite the stored claim"
    );
}

/// Dependency matching is keyed by container, not digest alone: two assets
/// sharing one hash, only one re-observed changed — the claim on the
/// unchanged container still stands.
#[test]
fn accept_f01_b_same_hash_other_container_stays_valid() {
    let claims = vec![
        claim(
            "test.asset-a",
            ClaimStatus::ObservedTool,
            vec![fingerprinted_evidence(
                EvidenceSource::ToolRun {
                    tool: "cs-inspect".to_owned(),
                    version: "test".to_owned(),
                },
                FingerprintKind::Content,
                HASH_A,
                "install/a.zbd",
            )],
        ),
        claim(
            "test.asset-b",
            ClaimStatus::ObservedTool,
            vec![fingerprinted_evidence(
                EvidenceSource::ToolRun {
                    tool: "cs-inspect".to_owned(),
                    version: "test".to_owned(),
                },
                FingerprintKind::Content,
                HASH_A,
                "install/b.zbd",
            )],
        ),
    ];

    let changed = index_of(vec![
        observed("install/a.zbd", FingerprintKind::Content, HASH_C),
        observed("install/b.zbd", FingerprintKind::Content, HASH_A),
    ]);
    let report = check_ledger(&claims, &changed);
    assert_eq!(report.invalidated.len(), 1);
    assert_eq!(
        report.invalidated[0].claim,
        ClaimId::new("test.asset-a").expect("valid id")
    );
    assert_eq!(
        report.valid,
        [ClaimId::new("test.asset-b").expect("valid id")],
        "the claim on the unchanged container must still stand"
    );
}

/// Two claims carrying one id are a contradiction the ledger must not choose
/// between: every occurrence is rejected as a duplicate.
#[test]
fn accept_f01_b_duplicate_claim_ids_are_all_rejected() {
    let first = claim("test.dup", ClaimStatus::Designed, vec![]);
    let mut second = claim("test.dup", ClaimStatus::Unknown, vec![]);
    second.subject = "a different subject under the same id".to_owned();
    let claims = vec![
        claim("test.ok", ClaimStatus::Designed, vec![]),
        first,
        second,
    ];

    let report = check_ledger(&claims, &FingerprintIndex::new());
    let rejected: Vec<&LedgerError> = report
        .rejected
        .iter()
        .map(|rejection| &rejection.error)
        .collect();
    assert_eq!(
        rejected,
        [&LedgerError::DuplicateId, &LedgerError::DuplicateId]
    );
    assert!(
        report
            .rejected
            .iter()
            .all(|rejection| rejection.claim.as_str() == "test.dup"),
        "both occurrences of the duplicated id are named"
    );
    assert_eq!(
        report.valid,
        [ClaimId::new("test.ok").expect("valid id")],
        "the unambiguous claim still stands"
    );
}

/// A claim may only dispute claims that exist in the ledger; a dangling
/// dispute is rejected by name. Disputes between present claims are preserved.
#[test]
fn accept_f01_b_dispute_of_unknown_claim_is_rejected() {
    let mut contradicting = claim("test.contradiction", ClaimStatus::Contradicted, vec![]);
    contradicting.disputes = vec![
        ClaimId::new("test.missing").expect("valid id"),
        ClaimId::new("test.also-missing").expect("valid id"),
    ];
    let claims = vec![
        claim("test.ok", ClaimStatus::Designed, vec![]),
        contradicting,
    ];

    let report = check_ledger(&claims, &FingerprintIndex::new());
    assert_eq!(report.rejected.len(), 2);
    assert!(
        report.rejected.iter().all(|rejection| {
            rejection.claim.as_str() == "test.contradiction"
                && matches!(rejection.error, LedgerError::UnknownDispute { .. })
        }),
        "each dangling dispute is its own rejection: {:?}",
        report.rejected
    );

    // When the disputed claim exists, the contradiction stands: the ledger
    // preserves disagreement instead of picking the convenient source.
    let mut present = claim("test.contested", ClaimStatus::Contradicted, vec![]);
    present.disputes = vec![ClaimId::new("test.ok").expect("valid id")];
    let claims = vec![claim("test.ok", ClaimStatus::Designed, vec![]), present];
    let report = check_ledger(&claims, &FingerprintIndex::new());
    assert!(
        report.is_clean(),
        "a contradiction between present claims is kept, not rejected: {:?}",
        report.diagnostic_lines()
    );
}

/// A dependency the index does not cover is reported unchecked — visible to a
/// strict audit — rather than silently trusted or spuriously invalidated.
#[test]
fn accept_f01_b_unobserved_dependency_is_unchecked_not_invalidated() {
    let claims = vec![claim(
        "test.unobserved",
        ClaimStatus::ObservedTool,
        vec![fingerprinted_evidence(
            EvidenceSource::ToolRun {
                tool: "cs-inspect".to_owned(),
                version: "test".to_owned(),
            },
            FingerprintKind::Content,
            HASH_A,
            "install/not-indexed.zbd",
        )],
    )];

    let report = check_ledger(&claims, &FingerprintIndex::new());
    assert!(
        report.invalidated.is_empty(),
        "no observation, no staleness"
    );
    assert_eq!(
        report.valid,
        [ClaimId::new("test.unobserved").expect("valid id")],
        "an unrefuted claim still stands"
    );
    assert_eq!(report.unchecked.len(), 1);
    assert_eq!(report.unchecked[0].reason, UncheckedReason::NotObserved);
    assert_eq!(
        report.unchecked[0].container.as_deref(),
        Some("install/not-indexed.zbd")
    );
    assert!(
        !report.is_clean(),
        "an unchecked dependency must dirty a strict report"
    );
}

/// A fingerprint with no locator cannot be tied to an observed asset; it is
/// reported unchecked instead of ignored.
#[test]
fn accept_f01_b_fingerprint_without_locator_is_unchecked() {
    let mut evidence = fingerprinted_evidence(
        EvidenceSource::ToolRun {
            tool: "cs-inspect".to_owned(),
            version: "test".to_owned(),
        },
        FingerprintKind::Artifact,
        HASH_A,
        "ignored-container",
    );
    evidence.locator = None;
    let claims = vec![claim(
        "test.no-locator",
        ClaimStatus::ObservedTool,
        vec![evidence],
    )];

    let report = check_ledger(
        &claims,
        &index_of(vec![observed(
            "ignored-container",
            FingerprintKind::Artifact,
            HASH_B,
        )]),
    );
    assert!(
        report.invalidated.is_empty(),
        "unlocatable evidence cannot be shown stale"
    );
    assert_eq!(report.unchecked.len(), 1);
    assert_eq!(report.unchecked[0].reason, UncheckedReason::MissingLocator);
    assert_eq!(report.unchecked[0].container, None);
}

/// Two different digests observed for the same kind and container are a
/// contradiction in the input; the index refuses to pick one.
#[test]
fn accept_f01_b_conflicting_observations_are_refused() {
    let result = FingerprintIndex::from_observations(vec![
        observed("install/a.zbd", FingerprintKind::Content, HASH_A),
        observed("install/a.zbd", FingerprintKind::Content, HASH_B),
    ]);
    assert_eq!(
        result.unwrap_err(),
        ObservationIndexError::ConflictingObservations {
            kind: FingerprintKind::Content,
            container: "install/a.zbd".to_owned(),
        }
    );

    // Identical duplicates collapse; a different container or kind is a
    // different asset and does not conflict.
    let index = index_of(vec![
        observed("install/a.zbd", FingerprintKind::Content, HASH_A),
        observed("install/a.zbd", FingerprintKind::Content, HASH_A),
        observed("install/b.zbd", FingerprintKind::Content, HASH_A),
    ]);
    assert_eq!(index.len(), 2);
    assert_eq!(
        index.current(FingerprintKind::Content, "install/a.zbd"),
        Some(ContentHash::from_bytes(HASH_A))
    );
    assert_eq!(
        index.current(FingerprintKind::Installation, "install/a.zbd"),
        None,
        "kind is part of the dependency key"
    );
}

/// A claim that already broke a rule is rejected, not additionally
/// invalidated: dispositions stay disjoint and each problem list stays true.
#[test]
fn accept_f01_b_rejected_claim_is_not_also_invalidated() {
    let mut broken = claim("test.broken", ClaimStatus::VerifiedOriginal, vec![]);
    broken.evidence = vec![fingerprinted_evidence(
        EvidenceSource::SyntheticFixture,
        FingerprintKind::Artifact,
        HASH_A,
        "fixtures/synthetic/x.bin",
    )];
    let claims = vec![broken];

    let report = check_ledger(
        &claims,
        &index_of(vec![observed(
            "fixtures/synthetic/x.bin",
            FingerprintKind::Artifact,
            HASH_B,
        )]),
    );
    assert_eq!(report.rejected.len(), 1);
    assert!(
        matches!(report.rejected[0].error, LedgerError::Record(_)),
        "the per-record failure is reported"
    );
    assert!(
        report.invalidated.is_empty(),
        "a refused claim has no standing left to invalidate"
    );
    assert!(report.valid.is_empty());
}
