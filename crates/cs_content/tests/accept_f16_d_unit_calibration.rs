//! Acceptance tests for F16-D's calibration half: the three-independent-
//! landmark rule of `F16` non-negotiable behavior 1, made checkable.
//!
//! Spec: `specs/F16-coordinates-units-origin-management-and-clocks.md`,
//! stage `### F16-D` ("Calibrate original units"). Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! These tests exercise production code only: `cs_content::coordinates`'s
//! [`UnitCalibration`], [`Landmark`], [`CalibratedQuantity`] and
//! [`CoordinateSource::calibration`].
//!
//! What makes them discriminating:
//!
//! * If the rule counted landmarks without requiring an observed
//!   [`LandmarkKind::Behavior`], a calibration built only from stored
//!   transforms — the "Blender transform is insufficient proof" case — would
//!   pass, and `accept_f16_d_artifacts_alone_never_calibrate_a_quantity` fails.
//! * If a repeated description or a reused [`EvidenceRecord`] counted as a new
//!   landmark, one inspection could be pasted three times and satisfy the
//!   rule: `accept_f16_d_landmarks_must_be_independent` fails.
//! * If [`UnitCalibration::claim_status`] were derived from completeness
//!   rather than from the evidence, a complete calibration over synthetic
//!   fixtures would claim `verified_original`:
//!   `accept_f16_d_a_complete_calibration_claims_only_what_its_evidence_supports`
//!   fails.
//! * If a declared source came with a pre-filled calibration,
//!   `accept_f16_d_no_declared_source_claims_a_measured_original_convention`
//!   fails.
//!
//! No test here fabricates evidence that an original run produced. The
//! strongest claim any of these tests reaches is `observed_tool` over
//! newly authored fixture content; the `verified_original` branch is not
//! reachable in this tree, and reaching it needs an owner-supplied original
//! run (recorded in
//! `docs/findings/2026-09-29-f16-d-original-units-and-clock-probes.md`).

use cs_content::coordinates::{
    CalibratedQuantity, CalibrationError, CoordinateSource, Landmark, LandmarkKind, UnitCalibration,
};
use cs_types::content::Origin;
use cs_types::evidence::{
    ClaimStatus, ContentHash, EvidenceRecord, EvidenceSource, Fingerprint, FingerprintKind,
    ObservationLocator, ObservationMethod, SourceSpan,
};

/// Distinct observations, so a test that wants three *independent* landmarks
/// has three to work with.
fn evidence(container: &str) -> EvidenceRecord {
    EvidenceRecord {
        source: EvidenceSource::SyntheticFixture,
        fingerprint: Some(Fingerprint {
            kind: FingerprintKind::Artifact,
            sha256: ContentHash::from_bytes([container.len() as u8; 32]),
        }),
        locator: Some(ObservationLocator {
            container: container.to_string(),
            span: Some(SourceSpan {
                offset: 0,
                length: 1,
            }),
        }),
        method: ObservationMethod::Authored,
        limitations: vec!["newly authored fixture content".to_string()],
    }
}

/// A `ToolRun` record over a produced artifact: still not original data.
fn tool_run_evidence(container: &str) -> EvidenceRecord {
    EvidenceRecord {
        source: EvidenceSource::ToolRun {
            tool: "cs_inspect".to_string(),
            version: "0.0.0".to_string(),
        },
        fingerprint: Some(Fingerprint {
            kind: FingerprintKind::Artifact,
            sha256: ContentHash::from_bytes([7; 32]),
        }),
        locator: Some(ObservationLocator {
            container: container.to_string(),
            span: None,
        }),
        method: ObservationMethod::ToolProbe,
        limitations: vec!["produced artifact, not original data".to_string()],
    }
}

/// A record citing a document.
fn document_evidence(container: &str) -> EvidenceRecord {
    EvidenceRecord {
        source: EvidenceSource::Document("docs/research/SOURCES.md".to_string()),
        fingerprint: None,
        locator: Some(ObservationLocator {
            container: container.to_string(),
            span: None,
        }),
        method: ObservationMethod::DocumentReview,
        limitations: vec!["a citation is not an observation".to_string()],
    }
}

/// A described landmark. Only the *description* is what these tests vary
/// freely; everything else a landmark needs to be valid is checked once, in
/// `accept_f16_d_landmarks_must_be_independent`.
fn described(
    quantity: CalibratedQuantity,
    kind: LandmarkKind,
    description: impl Into<String>,
    evidence: EvidenceRecord,
) -> Landmark {
    Landmark::new(quantity, kind, description, evidence).expect("a described landmark is valid")
}

fn calibration() -> UnitCalibration {
    UnitCalibration::new("fixture.source").expect("a named source is valid")
}

/// Records `count` artifact landmarks for one quantity starting at
/// `first_index`, each with its own observation.
fn artifacts(
    calibration: &mut UnitCalibration,
    quantity: CalibratedQuantity,
    first_index: usize,
    count: usize,
) {
    for index in first_index..first_index + count {
        let description = format!("{quantity:?} artifact {index}");
        let evidence = evidence(&format!("artifact-{quantity:?}-{index}"));
        calibration
            .record(described(
                quantity,
                LandmarkKind::Artifact,
                description,
                evidence,
            ))
            .expect("distinct artifact landmarks are independent");
    }
}

/// The rule needs three independent landmarks per quantity...
#[test]
fn accept_f16_d_a_quantity_needs_three_independent_landmarks() {
    assert_eq!(UnitCalibration::MIN_LANDMARKS, 3);

    for quantity in CalibratedQuantity::ALL {
        let mut record = calibration();
        artifacts(&mut record, quantity, 0, 2);
        assert_eq!(record.landmark_count(quantity), 2);
        assert!(
            !record.is_complete(),
            "two landmarks of {} are not three",
            quantity.label()
        );
        assert!(
            record
                .gaps()
                .iter()
                .any(|gap| gap.quantity == quantity && gap.landmarks_recorded == 2),
            "{}",
            record.describe()
        );

        artifacts(&mut record, quantity, 2, 1);
        assert_eq!(record.landmark_count(quantity), 3);
        assert_eq!(
            record.landmark_count(quantity),
            UnitCalibration::MIN_LANDMARKS
        );
    }

    // All four quantities filled: the record is complete.
    let mut record = calibration();
    for quantity in CalibratedQuantity::ALL {
        artifacts(&mut record, quantity, 0, UnitCalibration::MIN_LANDMARKS);
        record
            .record(described(
                quantity,
                LandmarkKind::Behavior,
                format!("{quantity:?} behavior"),
                evidence(&format!("behavior-{quantity:?}")),
            ))
            .expect("a behavior landmark is independent");
    }
    assert!(record.is_complete(), "{}", record.describe());
    assert!(record.gaps().is_empty(), "{}", record.describe());
}

/// ...and at least one of them must be an observed behavior: "a Blender
/// transform is insufficient proof."
#[test]
fn accept_f16_d_artifacts_alone_never_calibrate_a_quantity() {
    for quantity in CalibratedQuantity::ALL {
        let mut record = calibration();
        artifacts(&mut record, quantity, 0, UnitCalibration::MIN_LANDMARKS);
        assert_eq!(record.landmark_count(quantity), 3);
        assert_eq!(record.behavior_landmark_count(quantity), 0);
        assert!(
            !record.is_complete(),
            "three stored transforms are not three landmarks *and* a behavior: {}",
            record.describe()
        );
        let gap = record
            .gaps()
            .into_iter()
            .find(|gap| gap.quantity == quantity)
            .expect("the quantity is reported as a gap");
        assert_eq!(gap.landmarks_recorded, 3);
        assert_eq!(gap.landmarks_required, 3);
        assert_eq!(gap.behavior_landmarks, 0, "the behavior is what is missing");
        assert!(gap.to_string().contains("0/1 behaviors"));

        // One behavior closes that quantity, and only that quantity.
        record
            .record(described(
                quantity,
                LandmarkKind::Behavior,
                format!("{quantity:?} behavior"),
                evidence(&format!("behavior-{quantity:?}")),
            ))
            .expect("independent");
        assert_eq!(record.behavior_landmark_count(quantity), 1);
        assert!(
            !record.is_complete(),
            "the other three quantities are still gaps: {}",
            record.describe()
        );
        assert!(
            !record.gaps().iter().any(|gap| gap.quantity == quantity),
            "{}",
            record.describe()
        );
    }
}

/// Independence is enforced at the boundary: a repeated description and a
/// reused observation are both refused, so one inspection cannot be pasted
/// three times to satisfy the rule.
#[test]
fn accept_f16_d_landmarks_must_be_independent() {
    assert_eq!(
        UnitCalibration::new("  "),
        Err(CalibrationError::EmptySource)
    );
    assert_eq!(
        Landmark::new(
            CalibratedQuantity::Scale,
            LandmarkKind::Behavior,
            "   ",
            evidence("e")
        ),
        Err(CalibrationError::EmptyDescription)
    );

    let mut record = calibration();
    let first = described(
        CalibratedQuantity::Scale,
        LandmarkKind::Behavior,
        "Scale landmark 0",
        evidence("measurement-a"),
    );
    record.record(first.clone()).expect("recorded");

    // The same description again.
    let repeated = described(
        CalibratedQuantity::Scale,
        LandmarkKind::Behavior,
        "Scale landmark 0",
        evidence("measurement-b"),
    );
    assert_eq!(
        record.record(repeated),
        Err(CalibrationError::RepeatedDescription {
            quantity: CalibratedQuantity::Scale,
            description: "Scale landmark 0".to_string(),
        })
    );

    // The same evidence behind a different description is one observation, not
    // two.
    let reused = described(
        CalibratedQuantity::Scale,
        LandmarkKind::Behavior,
        "the same observation described differently",
        first.evidence().clone(),
    );
    assert_eq!(
        record.record(reused),
        Err(CalibrationError::RepeatedObservation {
            quantity: CalibratedQuantity::Scale,
            first: "Scale landmark 0".to_string(),
            second: "the same observation described differently".to_string(),
        })
    );

    // Re-wording the *limitations* does not make it a second observation
    // either: the caveat is the recorder's self-assessment, not what was
    // observed, so one inspection cannot be pasted three times by editing a
    // free-text field between pastes.
    let mut reworded = first.evidence().clone();
    reworded
        .limitations
        .push("re-read on a second day".to_string());
    let cosmetically_different = described(
        CalibratedQuantity::Scale,
        LandmarkKind::Behavior,
        "the same span, described differently again",
        reworded,
    );
    assert_eq!(
        record.record(cosmetically_different),
        Err(CalibrationError::RepeatedObservation {
            quantity: CalibratedQuantity::Scale,
            first: "Scale landmark 0".to_string(),
            second: "the same span, described differently again".to_string(),
        }),
        "a changed limitations string is not a new observation"
    );

    assert_eq!(
        record.landmarks().len(),
        1,
        "a refused landmark must not be recorded"
    );

    // The same description for a *different* quantity is fine: one inspection
    // can legitimately speak to more than one property, as long as it is
    // counted once per quantity.
    record
        .record(described(
            CalibratedQuantity::Handedness,
            LandmarkKind::Behavior,
            "Scale landmark 0",
            evidence("measurement-c"),
        ))
        .expect("recorded");
    assert_eq!(record.landmarks().len(), 2);
    assert_eq!(record.landmark_count(CalibratedQuantity::Scale), 1);
    assert_eq!(record.landmark_count(CalibratedQuantity::Handedness), 1);
}

/// Completeness is a shape; the claim is decided by the evidence. A complete
/// calibration over newly authored content is `unknown`, a document-only one
/// is `documented`, and a tool run over produced artifacts is `observed_tool` —
/// never `verified_original`.
#[test]
fn accept_f16_d_a_complete_calibration_claims_only_what_its_evidence_supports() {
    // Newly authored fixtures: complete, and claiming nothing.
    let mut fixtures = calibration();
    for quantity in CalibratedQuantity::ALL {
        for index in 0..UnitCalibration::MIN_LANDMARKS {
            let kind = if index == 0 {
                LandmarkKind::Behavior
            } else {
                LandmarkKind::Artifact
            };
            fixtures
                .record(described(
                    quantity,
                    kind,
                    format!("{quantity:?} {index}"),
                    evidence(&format!("fixture-{quantity:?}-{index}")),
                ))
                .expect("independent");
        }
    }
    assert!(fixtures.is_complete(), "{}", fixtures.describe());
    assert_eq!(
        fixtures.claim_status(),
        ClaimStatus::Unknown,
        "a complete calibration over synthetic fixtures must claim nothing"
    );
    assert!(fixtures.describe().contains("claim unknown"));

    // Documents only: complete, and claiming what a citation can claim.
    let mut documents = calibration();
    for quantity in CalibratedQuantity::ALL {
        for index in 0..UnitCalibration::MIN_LANDMARKS {
            documents
                .record(described(
                    quantity,
                    LandmarkKind::Behavior,
                    format!("{quantity:?} doc {index}"),
                    document_evidence(&format!("doc-{quantity:?}-{index}")),
                ))
                .expect("independent");
        }
    }
    assert!(documents.is_complete(), "{}", documents.describe());
    assert_eq!(documents.claim_status(), ClaimStatus::Documented);

    // A tool run over produced artifacts: the strongest honest claim here.
    let mut tools = calibration();
    for quantity in CalibratedQuantity::ALL {
        for index in 0..UnitCalibration::MIN_LANDMARKS {
            tools
                .record(described(
                    quantity,
                    LandmarkKind::Behavior,
                    format!("{quantity:?} tool {index}"),
                    tool_run_evidence(&format!("tool-{quantity:?}-{index}")),
                ))
                .expect("independent");
        }
    }
    assert!(tools.is_complete(), "{}", tools.describe());
    assert_eq!(tools.claim_status(), ClaimStatus::ObservedTool);
    for landmark in tools.landmarks() {
        assert!(
            !landmark.evidence().verifies_original(),
            "an artifact fingerprint can never verify the original"
        );
    }

    // One synthetic landmark poisons the whole claim: a set that mixes
    // authored content with tool runs cannot claim to have measured anything.
    let mut mixed = calibration();
    for quantity in CalibratedQuantity::ALL {
        for index in 0..UnitCalibration::MIN_LANDMARKS {
            let record = if index == 0 {
                evidence(&format!("mixed-fixture-{quantity:?}"))
            } else {
                tool_run_evidence(&format!("mixed-tool-{quantity:?}-{index}"))
            };
            mixed
                .record(described(
                    quantity,
                    LandmarkKind::Behavior,
                    format!("{quantity:?} mixed {index}"),
                    record,
                ))
                .expect("independent");
        }
    }
    assert!(mixed.is_complete(), "{}", mixed.describe());
    assert_eq!(
        mixed.claim_status(),
        ClaimStatus::Unknown,
        "a calibration containing newly authored content claims nothing"
    );

    // Incomplete is `unknown` whatever the evidence says: the strongest claim
    // needs all four quantities, and the gaps say which one is missing.
    let mut partial = calibration();
    for quantity in CalibratedQuantity::ALL {
        let wanted = if quantity == CalibratedQuantity::AngleUnit {
            UnitCalibration::MIN_LANDMARKS - 1
        } else {
            UnitCalibration::MIN_LANDMARKS
        };
        for index in 0..wanted {
            partial
                .record(described(
                    quantity,
                    LandmarkKind::Behavior,
                    format!("{quantity:?} partial {index}"),
                    tool_run_evidence(&format!("partial-{quantity:?}-{index}")),
                ))
                .expect("independent");
        }
    }
    assert!(!partial.is_complete(), "{}", partial.describe());
    assert_eq!(partial.claim_status(), ClaimStatus::Unknown);
    let gaps = partial.gaps();
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0].quantity, CalibratedQuantity::AngleUnit);
    assert_eq!(gaps[0].landmarks_recorded, 2);
    assert_eq!(gaps[0].landmarks_required, 3);
    assert_eq!(gaps[0].behavior_landmarks, 2);
}

/// No declared source may arrive with a measured convention: every one starts
/// with an empty calibration and claims `unknown`.
#[test]
fn accept_f16_d_no_declared_source_claims_a_measured_original_convention() {
    let declared = CoordinateSource::declared();
    assert!(!declared.is_empty(), "the registry must not be empty");
    let mut labels: Vec<&str> = Vec::new();
    for source in &declared {
        let calibration = source.calibration();
        assert_eq!(calibration.source(), source.label());
        assert!(
            calibration.landmarks().is_empty(),
            "{} must arrive with no landmarks",
            source.label()
        );
        assert!(!calibration.is_complete());
        assert_eq!(calibration.claim_status(), ClaimStatus::Unknown);
        assert!(
            !source.origin().is_original(),
            "{} must not claim installation data",
            source.label()
        );
        assert!(matches!(
            source.origin(),
            Origin::Designed | Origin::SyntheticFixture
        ));
        labels.push(source.label());
    }
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), declared.len(), "labels must be unique");

    // Every quantity is reported as a gap for an uncalibrated source, so the
    // report is actionable rather than a bare "not calibrated".
    let calibration = declared[0].calibration();
    let gaps = calibration.gaps();
    assert_eq!(gaps.len(), CalibratedQuantity::ALL.len());
    for quantity in CalibratedQuantity::ALL {
        let gap = gaps
            .iter()
            .find(|gap| gap.quantity == quantity)
            .expect("every quantity is reported");
        assert_eq!(gap.landmarks_recorded, 0);
        assert_eq!(gap.landmarks_required, UnitCalibration::MIN_LANDMARKS);
        assert_eq!(gap.behavior_landmarks, 0);
    }
}
