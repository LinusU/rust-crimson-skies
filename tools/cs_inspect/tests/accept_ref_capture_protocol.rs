//! Acceptance scenario `REF-CAPTURE-PROTOCOL` (#357) for `cs-inspect`: the
//! capture-record validator admits a well-formed synthetic record, refuses
//! every named defect (missing fingerprints, ambiguous timebase, non-finite
//! values, mismatched artifact hashes, undeclared units, a synthetic record
//! labeled original, mixed sources, a file trace offered as flight evidence),
//! reports missing capture data as *unavailable* rather than as a pass, and
//! keeps fitting acceleration alone from standing in for handling fidelity.
//!
//! These tests exercise production code only: `cs_inspect::reference_capture::
//! validate_capture`, `fidelity_comparison` and `synthetic_capture_record`.
//! Removing or neutering that implementation makes them fail. Every value in
//! the fixtures is authored test data — no original unit, coordinate or timing
//! appears here, and no test reads the original installation.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use cs_inspect::reference_capture::{
    ArtifactRef, ArtifactRole, CaptureBasis, CaptureError, CaptureRecord, FidelityComparison,
    FileAccessTraceRef, FingerprintField, HoldoutReservation, ManeuverKind, Measurement,
    OriginalIdentity, RecordClaim, ReferenceSet, SampleRole, SampleSource, UnavailableReason,
    ValidationContext, Worksheet, fidelity_comparison, synthetic_capture_record, validate_capture,
};
use cs_types::evidence::ContentHash;

/// The authored bytes of the fixture artifact every record references.
const ARTIFACT_BYTES: &[u8] = b"reference-capture-protocol fixture artifact bytes";

/// Serial counter so parallel test binaries cannot collide on one name.
static NEXT_TREE: AtomicU64 = AtomicU64::new(0);

/// A unique, disposable directory holding the fixture artifacts of one test.
struct TempArtifacts {
    root: PathBuf,
}

impl TempArtifacts {
    /// Creates a fresh empty artifact root with a collision-free name.
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after the Unix epoch")
            .as_nanos();
        let serial = NEXT_TREE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "cs-ref-capture-{label}-{}-{nanos}-{serial}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("the artifact root is created");
        Self { root }
    }

    /// The artifact root, ready to be handed to [`ValidationContext`].
    fn root(&self) -> &Path {
        &self.root
    }

    /// Writes `bytes` at `spelling`, creating parent directories as needed.
    fn write(&self, spelling: &str, bytes: &[u8]) {
        let path = self.root.join(spelling);
        let parent = path.parent().expect("fixture spellings have a parent");
        fs::create_dir_all(parent).expect("fixture directories are created");
        fs::write(&path, bytes).expect("fixture bytes are written");
    }
}

impl Drop for TempArtifacts {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// The fixture artifact: written under `tree` and referenced by `path`, with
/// the digest its bytes really hash to.
fn fixture_artifact(tree: &TempArtifacts, path: &str) -> ArtifactRef {
    tree.write(path, ARTIFACT_BYTES);
    ArtifactRef {
        relative_path: path.to_owned(),
        sha256: cs_assets::install::sha256(ARTIFACT_BYTES),
        role: ArtifactRole::TelemetryLog,
    }
}

/// The well-formed **synthetic** record every mutation starts from.
fn synthetic_record(tree: &TempArtifacts) -> CaptureRecord {
    synthetic_capture_record(fixture_artifact(tree, "captures/synthetic-01.log"))
}

/// The same record shaped as an original-observed capture: fingerprints known,
/// a flight-observing basis, an original claim.
///
/// Like every other test double in this repository it is authored fixture data
/// and says so in its notes — it exists so the original-path rules can be
/// exercised without original data, never as evidence about the original.
fn original_record(tree: &TempArtifacts) -> CaptureRecord {
    let mut record = synthetic_capture_record(fixture_artifact(tree, "captures/original-01.log"));
    record.id = "fixture.original-probe".to_owned();
    record.source = SampleSource::OriginalObserved;
    record.series.source = SampleSource::OriginalObserved;
    record.claim = RecordClaim::OriginalBehavior;
    record.identity = OriginalIdentity {
        edition: Measurement::Known("fixture edition (authored)".to_owned()),
        executable_sha256: Measurement::Known(ContentHash::from_bytes([0xEE; 32])),
        installation_sha256: Measurement::Known(ContentHash::from_bytes([0xCC; 32])),
    };
    record.mission = Measurement::Known("fixture mission (authored)".to_owned());
    record.airframe = Measurement::Known("fixture airframe (authored)".to_owned());
    record.loadout = Measurement::Known("fixture loadout (authored)".to_owned());
    record.basis = vec![CaptureBasis::RuntimeObservation {
        detail: "fixture observation (authored)".to_owned(),
    }];
    record.notes.push(
        "test fixture shaped like an original record; authored data, not original evidence"
            .to_owned(),
    );
    record
}

/// A well-formed synthetic record validates cleanly: every field present, the
/// artifact re-hashed to its declared digest, nothing missing. A validator that
/// rejects everything fails here.
#[test]
fn accept_ref_capture_protocol_well_formed_synthetic_record_validates() {
    let tree = TempArtifacts::new("valid");
    let record = synthetic_record(&tree);

    let report = validate_capture(&record, &ValidationContext::with_artifact_root(tree.root()));
    assert!(
        report.is_valid(),
        "a complete synthetic record must be valid: {:?}",
        report.diagnostic_lines()
    );
    assert!(!report.is_invalid(), "no defect in a complete record");
    assert!(
        !report.is_unavailable(),
        "no missing data in a complete record"
    );
    assert_eq!(report.record, record.id, "the report names its record");
}

/// An original-observed record that does not fingerprint the original it claims
/// to have observed is refused, by field name.
#[test]
fn accept_ref_capture_protocol_missing_fingerprint_is_rejected() {
    let tree = TempArtifacts::new("fingerprint");
    let mut record = original_record(&tree);
    record.identity.installation_sha256 = Measurement::Unknown {
        reason: "not recorded".to_owned(),
    };

    let report = validate_capture(&record, &ValidationContext::with_artifact_root(tree.root()));
    assert!(report.is_invalid(), "a missing fingerprint is a defect");
    assert_eq!(
        report.invalid,
        [CaptureError::MissingFingerprint {
            field: FingerprintField::Installation
        }],
        "only the missing fingerprint is reported"
    );
    assert!(
        report.diagnostic_lines()[0].contains("installation_sha256"),
        "the diagnostic names the missing field: {:?}",
        report.diagnostic_lines()
    );

    // All three fields are required, not just the installation digest.
    let mut missing_edition = original_record(&tree);
    missing_edition.identity.edition = Measurement::Unknown {
        reason: "not recorded".to_owned(),
    };
    let report = validate_capture(
        &missing_edition,
        &ValidationContext::with_artifact_root(tree.root()),
    );
    assert_eq!(
        report.invalid,
        [CaptureError::MissingFingerprint {
            field: FingerprintField::Edition
        }]
    );
}

/// Samples whose times cannot be placed on a timeline are refused: an
/// unestablished clock, a rate that does not determine a timeline, and an
/// uncertainty asserted without a measurement method are all ambiguous.
#[test]
fn accept_ref_capture_protocol_ambiguous_timebase_is_rejected() {
    let tree = TempArtifacts::new("timebase");
    let context = ValidationContext::with_artifact_root(tree.root());

    let mut unknown_clock = synthetic_record(&tree);
    unknown_clock.timebase.clock = Measurement::Unknown {
        reason: "not measured".to_owned(),
    };
    let report = validate_capture(&unknown_clock, &context);
    assert_eq!(
        report.invalid,
        [CaptureError::AmbiguousTimebase {
            detail: "samples exist but clock is not established".to_owned()
        }],
        "samples on an unknown clock are ambiguous"
    );

    let mut zero_rate = synthetic_record(&tree);
    zero_rate.timebase.nominal_rate_hz = Measurement::Known(0.0);
    let report = validate_capture(&zero_rate, &context);
    assert!(
        matches!(
            report.invalid.as_slice(),
            [CaptureError::AmbiguousTimebase { .. }]
        ),
        "a zero sample rate does not determine a timeline: {:?}",
        report.diagnostic_lines()
    );

    let mut unmeasured_uncertainty = synthetic_record(&tree);
    unmeasured_uncertainty.timebase.uncertainty =
        Measurement::Known(cs_inspect::reference_capture::TimingUncertainty {
            plus_minus: 0.001,
            method: "   ".to_owned(),
        });
    let report = validate_capture(&unmeasured_uncertainty, &context);
    assert!(
        matches!(
            report.invalid.as_slice(),
            [CaptureError::AmbiguousTimebase { .. }]
        ),
        "an uncertainty without a measurement method is refused: {:?}",
        report.diagnostic_lines()
    );
}

/// Non-finite values — sample time, observation, input and declared conversion
/// factor — are refused wherever they appear in the data path.
#[test]
fn accept_ref_capture_protocol_nonfinite_value_is_rejected() {
    let tree = TempArtifacts::new("nonfinite");
    let context = ValidationContext::with_artifact_root(tree.root());

    let mut nan_observation = synthetic_record(&tree);
    nan_observation.series.samples[1].observations[0].value = f64::NAN;
    let report = validate_capture(&nan_observation, &context);
    assert_eq!(
        report.invalid,
        [CaptureError::NonFinite {
            field: "samples[1].observations[0].value".to_owned()
        }]
    );

    let mut infinite_time = synthetic_record(&tree);
    infinite_time.series.samples[1].t = f64::INFINITY;
    let report = validate_capture(&infinite_time, &context);
    assert_eq!(
        report.invalid,
        [CaptureError::NonFinite {
            field: "samples[1].t".to_owned()
        }]
    );

    let mut nan_input = synthetic_record(&tree);
    nan_input.series.samples[0].input.pitch = f64::NAN;
    let report = validate_capture(&nan_input, &context);
    assert_eq!(
        report.invalid,
        [CaptureError::NonFinite {
            field: "samples[0].input.pitch".to_owned()
        }]
    );

    let mut nan_conversion = synthetic_record(&tree);
    nan_conversion.units[1].provenance = cs_inspect::reference_capture::UnitProvenance::Converted {
        from_unit: "fixture_units".to_owned(),
        factor: f64::NAN,
    };
    let report = validate_capture(&nan_conversion, &context);
    assert!(
        report.invalid.iter().any(
            |error| matches!(error, CaptureError::NonFinite { field } if field
                .contains("conversion_factor"))
        ),
        "a non-finite conversion factor is refused: {:?}",
        report.diagnostic_lines()
    );
}

/// An artifact whose bytes no longer hash to the declared digest is a defect,
/// not missing data: the record says one thing and the file says another.
#[test]
fn accept_ref_capture_protocol_artifact_hash_mismatch_is_rejected() {
    let tree = TempArtifacts::new("hash-mismatch");
    let mut record = synthetic_record(&tree);
    record.artifacts[0].sha256 = ContentHash::from_bytes([0x00; 32]);

    let report = validate_capture(&record, &ValidationContext::with_artifact_root(tree.root()));
    assert!(report.is_invalid(), "a digest disagreement is a defect");
    assert!(
        !report.is_unavailable(),
        "the artifact was present and read; nothing is missing"
    );
    assert_eq!(report.invalid.len(), 1, "exactly the mismatch: {report:?}");
    match &report.invalid[0] {
        CaptureError::ArtifactHashMismatch {
            path,
            declared,
            observed,
        } => {
            assert_eq!(path, "captures/synthetic-01.log");
            assert_eq!(
                observed,
                &cs_assets::install::sha256(ARTIFACT_BYTES).to_hex(),
                "the observed digest is the one the bytes hash to"
            );
            assert_ne!(declared, observed, "declared and observed disagree");
        }
        other => panic!("expected an artifact hash mismatch, got {other:?}"),
    }
}

/// A record that has not been captured yet is **unavailable**, never a pass
/// and never a silent success: no samples, no artifact, an unreadable artifact,
/// or an unchecked artifact root all report as missing data.
#[test]
fn accept_ref_capture_protocol_missing_capture_is_unavailable_not_pass() {
    let tree = TempArtifacts::new("missing");

    // An empty worksheet: nothing captured yet.
    let mut empty = synthetic_record(&tree);
    empty.series.samples.clear();
    empty.artifacts.clear();
    let report = validate_capture(&empty, &ValidationContext::with_artifact_root(tree.root()));
    assert!(!report.is_valid(), "an empty worksheet is not a pass");
    assert!(!report.is_invalid(), "an empty worksheet is not a defect");
    assert!(report.is_unavailable(), "it is unavailable instead");
    assert_eq!(
        report.unavailable,
        [
            UnavailableReason::NoSamples {
                record: empty.id.clone()
            },
            UnavailableReason::NoArtifacts {
                record: empty.id.clone()
            },
        ]
    );

    // The referenced artifact is not there to re-hash.
    let mut missing_file = synthetic_record(&tree);
    missing_file.artifacts[0].relative_path = "captures/absent.log".to_owned();
    let report = validate_capture(
        &missing_file,
        &ValidationContext::with_artifact_root(tree.root()),
    );
    assert!(
        report.is_unavailable(),
        "an unreadable artifact is missing data"
    );
    assert!(!report.is_invalid());
    assert_eq!(
        report.unavailable,
        [UnavailableReason::ArtifactMissing {
            record: missing_file.id.clone(),
            path: "captures/absent.log".to_owned()
        }]
    );

    // Without an artifact root nothing was hashed — unchecked is not a pass.
    let record = synthetic_record(&tree);
    let report = validate_capture(&record, &ValidationContext::without_artifacts());
    assert!(
        report.is_unavailable(),
        "an unchecked artifact is unavailable"
    );
    assert!(!report.is_valid(), "unchecked never reads as valid");
    assert_eq!(
        report.unavailable,
        [UnavailableReason::ArtifactRootNotSupplied {
            record: record.id.clone(),
            path: "captures/synthetic-01.log".to_owned()
        }]
    );
}

/// Units are declared, never guessed: an observation or a time unit whose
/// (quantity, unit) pair is missing from the record's unit table is refused.
#[test]
fn accept_ref_capture_protocol_undeclared_unit_is_rejected() {
    let tree = TempArtifacts::new("units");
    let context = ValidationContext::with_artifact_root(tree.root());

    let mut undeclared_observation = synthetic_record(&tree);
    undeclared_observation.series.samples[0].observations[0].unit = "ft".to_owned();
    let report = validate_capture(&undeclared_observation, &context);
    assert_eq!(
        report.invalid,
        [CaptureError::UndeclaredUnit {
            quantity: "world_speed".to_owned(),
            unit: "ft".to_owned()
        }],
        "a unit that is not declared for its quantity is refused"
    );

    let mut undeclared_time = synthetic_record(&tree);
    undeclared_time.timebase.unit = "frame".to_owned();
    let report = validate_capture(&undeclared_time, &context);
    assert_eq!(
        report.invalid,
        [CaptureError::UndeclaredUnit {
            quantity: "time".to_owned(),
            unit: "frame".to_owned()
        }],
        "the time unit must be declared like any other"
    );
}

/// The acceptance case: a record produced from synthetic data is refused when
/// it is offered as original-game behavior, and a remake capture is refused
/// for the same claim. Provenance and claim must agree.
#[test]
fn accept_ref_capture_protocol_synthetic_record_labeled_original_is_rejected() {
    let tree = TempArtifacts::new("labeled-original");
    let context = ValidationContext::with_artifact_root(tree.root());

    let mut synthetic = synthetic_record(&tree);
    synthetic.claim = RecordClaim::OriginalBehavior;
    let report = validate_capture(&synthetic, &context);
    assert!(
        report.is_invalid(),
        "a synthetic record cannot claim original"
    );
    assert!(
        report.invalid.contains(&CaptureError::SourceClaimMismatch {
            source: SampleSource::SyntheticFixture,
            claim: RecordClaim::OriginalBehavior
        }),
        "the provenance/claim contradiction must be named: {:?}",
        report.invalid
    );
    assert!(
        report
            .diagnostic_lines()
            .iter()
            .any(|line| line.contains("synthetic_fixture") && line.contains("original_behavior")),
        "the diagnostic names the source and the claim: {:?}",
        report.diagnostic_lines()
    );

    let mut remake = original_record(&tree);
    remake.source = SampleSource::RemakeSample;
    remake.series.source = SampleSource::RemakeSample;
    let report = validate_capture(&remake, &context);
    assert!(
        report.invalid.contains(&CaptureError::SourceClaimMismatch {
            source: SampleSource::RemakeSample,
            claim: RecordClaim::OriginalBehavior
        }),
        "a remake capture is refused as original behavior: {:?}",
        report.diagnostic_lines()
    );
}

/// Original-observed samples and remake samples are separate sources: a
/// record whose series was produced elsewhere is refused by name.
#[test]
fn accept_ref_capture_protocol_original_and_remake_samples_stay_separate() {
    let tree = TempArtifacts::new("mixed-sources");
    let mut record = original_record(&tree);
    record.series.source = SampleSource::RemakeSample;

    let report = validate_capture(&record, &ValidationContext::with_artifact_root(tree.root()));
    assert_eq!(
        report.invalid,
        [CaptureError::MixedSampleSources {
            record: SampleSource::OriginalObserved,
            series: SampleSource::RemakeSample
        }],
        "an original record carrying remake samples is a provenance break"
    );
}

/// A path that would escape the artifact root is refused before anything is
/// opened, so a record can never make the validator read outside it.
#[test]
fn accept_ref_capture_protocol_artifact_path_escape_is_rejected() {
    let tree = TempArtifacts::new("path-escape");
    let mut record = synthetic_record(&tree);
    record.artifacts[0].relative_path = "../outside/the-root.bin".to_owned();

    let report = validate_capture(&record, &ValidationContext::with_artifact_root(tree.root()));
    assert_eq!(
        report.invalid,
        [CaptureError::UnsafeArtifactPath {
            path: "../outside/the-root.bin".to_owned()
        }]
    );
    assert!(
        report.unavailable.is_empty(),
        "the refusal is a defect, not missing data: {:?}",
        report.diagnostic_lines()
    );
}

/// The file-access trace shared with Rally #341 may accompany a record, but a
/// record offered as flight/mission behavior must have been observed with
/// something that observes flight: a file trace records which files opened,
/// never how the aircraft flew. Sharing the metadata as worksheet data is fine.
#[test]
fn accept_ref_capture_protocol_file_trace_alone_cannot_support_flight_behavior() {
    let tree = TempArtifacts::new("file-trace");
    let trace_bytes = b"authored fixture file-access trace";
    tree.write("traces/lookup-order.pml", trace_bytes);
    let trace = FileAccessTraceRef {
        task: "Rally #341".to_owned(),
        tool: "fixture trace tool".to_owned(),
        platform: "fixture platform".to_owned(),
        relative_path: "traces/lookup-order.pml".to_owned(),
        sha256: cs_assets::install::sha256(trace_bytes),
        covers: vec!["fixture world group".to_owned()],
    };

    let mut as_original = original_record(&tree);
    as_original.basis = vec![CaptureBasis::FileAccessTrace {
        trace: trace.clone(),
    }];
    let report = validate_capture(
        &as_original,
        &ValidationContext::with_artifact_root(tree.root()),
    );
    assert_eq!(
        report.invalid,
        [CaptureError::FileTraceCannotSupportFlightBehavior {
            claim: RecordClaim::OriginalBehavior
        }],
        "a file trace alone cannot back a behavior claim"
    );
    assert!(
        report.unavailable.is_empty(),
        "the trace itself was present and hashed: {:?}",
        report.diagnostic_lines()
    );

    // The same metadata, shared as worksheet data instead of as flight evidence.
    let mut as_worksheet = as_original;
    as_worksheet.claim = RecordClaim::WorksheetOnly;
    let report = validate_capture(
        &as_worksheet,
        &ValidationContext::with_artifact_root(tree.root()),
    );
    assert!(
        report.is_valid(),
        "sharing #341 trace metadata without claiming flight must validate: {:?}",
        report.diagnostic_lines()
    );
}

/// The reserved holdout: fitting acceleration alone can never establish
/// handling fidelity. The comparison stays unavailable until the reserved
/// maneuver is captured as original data, the reservation is enforced, and a
/// non-original holdout is not original evidence.
#[test]
fn accept_ref_capture_protocol_acceleration_alone_cannot_establish_fidelity() {
    let tree = TempArtifacts::new("holdout");
    let context = ValidationContext::with_artifact_root(tree.root());
    let holdout = HoldoutReservation {
        maneuver: ManeuverKind::Turn,
        rationale: "fixture: reserved before fitting".to_owned(),
    };

    // Calibration only: the fit has seen everything, so nothing is comparable.
    let calibration = original_record(&tree);
    let set = ReferenceSet {
        holdout: holdout.clone(),
        records: vec![calibration.clone()],
    };
    let outcome = fidelity_comparison(&set, &context);
    assert!(
        matches!(
            &outcome,
            FidelityComparison::Unavailable(reasons)
                if reasons.contains(&UnavailableReason::HoldoutNotCaptured {
                    maneuver: "turn".to_owned()
                })
        ),
        "an acceleration-only set must stay unavailable: {outcome:?}"
    );
    assert!(!outcome.is_ready(), "unavailable is never ready");

    // With the reserved maneuver captured as original data the comparison can
    // be run: this is structural readiness, not a fidelity verdict.
    let mut holdout_record = original_record(&tree);
    holdout_record.id = "fixture.holdout-turn".to_owned();
    holdout_record.role = SampleRole::Holdout;
    holdout_record.worksheet = Worksheet::BaselineFlight {
        maneuver: ManeuverKind::Turn,
    };
    let set = ReferenceSet {
        holdout: holdout.clone(),
        records: vec![calibration.clone(), holdout_record.clone()],
    };
    assert_eq!(
        fidelity_comparison(&set, &context),
        FidelityComparison::Ready {
            calibration: vec![calibration.id.clone()],
            holdout: "turn".to_owned(),
        },
        "a captured original holdout makes the comparison runnable"
    );

    // The reservation is enforced: the holdout maneuver may not be fitted.
    let mut leaked = holdout_record.clone();
    leaked.id = "fixture.leaked-turn".to_owned();
    leaked.role = SampleRole::Calibration;
    let set = ReferenceSet {
        holdout: holdout.clone(),
        records: vec![calibration.clone(), leaked],
    };
    assert!(
        matches!(
            fidelity_comparison(&set, &context),
            FidelityComparison::Invalid(errors)
                if errors.contains(&CaptureError::HoldoutUsedForCalibration {
                    maneuver: "turn".to_owned()
                })
        ),
        "using the reserved maneuver as calibration data is a defect"
    );

    // A holdout that is not original-observed cannot support original fidelity.
    let mut synthetic_holdout = holdout_record;
    synthetic_holdout.id = "fixture.synthetic-holdout".to_owned();
    synthetic_holdout.source = SampleSource::SyntheticFixture;
    synthetic_holdout.series.source = SampleSource::SyntheticFixture;
    synthetic_holdout.claim = RecordClaim::WorksheetOnly;
    synthetic_holdout.basis = vec![CaptureBasis::SyntheticSimulation {
        detail: "fixture".to_owned(),
    }];
    synthetic_holdout.identity = OriginalIdentity {
        edition: Measurement::Unknown {
            reason: "synthetic fixture".to_owned(),
        },
        executable_sha256: Measurement::Unknown {
            reason: "synthetic fixture".to_owned(),
        },
        installation_sha256: Measurement::Unknown {
            reason: "synthetic fixture".to_owned(),
        },
    };
    let set = ReferenceSet {
        holdout,
        records: vec![calibration, synthetic_holdout],
    };
    assert!(
        matches!(
            fidelity_comparison(&set, &context),
            FidelityComparison::Unavailable(reasons)
                if reasons.contains(&UnavailableReason::HoldoutNotOriginal {
                    maneuver: "turn".to_owned()
                })
        ),
        "a synthetic holdout is not original evidence"
    );
}
