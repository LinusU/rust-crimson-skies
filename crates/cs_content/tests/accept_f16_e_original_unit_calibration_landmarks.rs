//! Acceptance tests for F16-E: the original unit-calibration landmarks the
//! owner measured by static analysis, recorded against the two real formats
//! this project reads (task #390, `F16-E`).
//!
//! Spec: `specs/F16-coordinates-units-origin-management-and-clocks.md`
//! (non-negotiable behavior 1) and the owner's 2026-10-05 note on #390.
//! Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`. Write-up of every
//! landmark: `docs/findings/2026-10-06-f16-e-original-unit-calibration-landmarks.md`.
//!
//! These tests exercise production code only: `cs_content::coordinates`'s
//! [`CoordinateSource::retail_gamez`], [`CoordinateSource::retail_zrd`],
//! [`CoordinateSource::with_calibration`] and the [`UnitCalibration`] they
//! hand back.
//!
//! What makes them discriminating:
//!
//! * If the measured convention were dropped back to the designed identity
//!   declaration, `accept_f16_e_real_format_sources_carry_the_measured_convention_and_their_calibration`
//!   fails: the `.zrd` angle unit is degrees, and no designed source records
//!   that measured distinction with `Origin::Installation`.
//! * If a quantity had fewer than three **independent** artifact landmarks,
//!   `accept_f16_e_every_quantity_has_at_least_three_independent_artifact_landmarks`
//!   fails — it compares each quantity's observations, so re-wording one
//!   inspection into three landmarks is not enough.
//! * If a behavior landmark were invented (no original run exists), the gaps
//!   would close and
//!   `accept_f16_e_gaps_report_exactly_the_missing_behavior_landmark_per_quantity`
//!   fails; if the missing behavior were *not* reported, `is_complete()` would
//!   go true and the same test fails.
//! * If the evidence were read as stronger than it is,
//!   `accept_f16_e_claim_status_is_what_the_evidence_supports_and_never_verified_original`
//!   fails: static analysis without an original run never claims
//!   `verified_original`.
//! * If a calibration could be attached to a source it is not about,
//!   `accept_f16_e_with_calibration_refuses_a_calibration_for_another_source`
//!   fails.
//! * The retail test below re-hashes the installation and the owner's
//!   decrypted image and re-reads the landmark's own bytes, so a landmark
//!   recorded about different files fails loudly instead of standing.

use cs_content::coordinates::{
    AngleUnit, Axis, CalibratedQuantity, CoordinateSource, GAMEZ_SOURCE_LABEL, LandmarkKind,
    ORIGINAL_IMAGE_SHA256, RotationSense, SourceAxis, SourceConvention, SourceError,
    UnitCalibration, ZRD_ANIMATION_MEMBER, ZRD_DOCUMENT_CONVENTION_IS_MEASURED,
    ZRD_GRAVITY_VALUE_LENGTH, ZRD_GRAVITY_VALUE_OFFSET, ZRD_READER_ARCHIVE,
    ZRD_READER_ARCHIVE_SHA256, ZRD_SOURCE_LABEL,
};
use cs_types::asset_id::SourceSpan as InstallSpan;
use cs_types::content::{Origin, Provenance};
use cs_types::evidence::{
    ClaimId, ClaimStatus, ContentHash, EvidenceSource, ObservationMethod,
    SourceSpan as EvidenceSpan,
};
use cs_types::space::Winding;

/// The installation hash and content hash of the read-only retail
/// installation the landmarks were re-derived against (#798): the retail test
/// below re-derives them from the manifest.
///
/// Both were first recorded with the owner's decrypted image still sitting
/// inside `$CS_GAME_DIR`, where it was an inventoried file and therefore part
/// of both digests (`c14a876f…` / `148a24b7…`, still what
/// `docs/findings/evidence/M01-LC-WORLD-UNIT-ROLES.json` records of that
/// run). The owner has moved the image out of the installation for good
/// (#798), so these are re-derived from the read-only tree, which hashes to
/// the same values the pre-image findings recorded
/// (`2026-09-29-f14-d-retail-baseline-inventory.md`).
const INSTALL_SHA256: &str = "b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978";
const CONTENT_SHA256: &str = "a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d";

/// The installation envelope a measured source is declared with; the retail
/// test builds the same thing from the discovered installation.
fn install_span(container: &str, member: Option<&str>) -> InstallSpan {
    InstallSpan::new(
        ContentHash::from_bytes([0; 32]),
        container,
        member,
        0,
        0,
        None,
    )
    .expect("a source span for a measured source's declaration is valid")
}

/// The two real formats the project reads, each with its own installation
/// span the way a production caller supplies one.
fn measured_sources() -> [CoordinateSource; 2] {
    [
        CoordinateSource::retail_gamez(install_span("zbd/c1c/gamez.zbd", None)),
        CoordinateSource::retail_zrd(install_span(ZRD_READER_ARCHIVE, Some(ZRD_ANIMATION_MEMBER))),
    ]
}

/// A valid declaration for the constructor tests below.
fn convention() -> SourceConvention {
    SourceConvention::new(
        [
            SourceAxis::positive(Axis::X),
            SourceAxis::positive(Axis::Y),
            SourceAxis::positive(Axis::Z),
        ],
        Axis::Z,
        1.0,
        AngleUnit::Radians,
        RotationSense::RightHandRule,
        Winding::CounterClockwise,
    )
    .expect("the identity declaration is valid")
}

fn provenance(claim: &str) -> Provenance {
    Provenance::designed(ClaimId::new(claim).expect("a claim id is valid"))
}

/// **Each real format declares the measured convention and carries the
/// calibration that measures it.**
///
/// The numbers are the owner's measurements: one stored unit to the metre,
/// +Y up with X/Z horizontal, right-handed with no mirror between content and
/// screen, radians in the GameZ binaries and degrees in `.zrd` text.
#[test]
fn accept_f16_e_real_format_sources_carry_the_measured_convention_and_their_calibration() {
    let [gamez, zrd] = measured_sources();
    assert_eq!(gamez.label(), GAMEZ_SOURCE_LABEL);
    assert_eq!(gamez.label(), "retail.gamez");
    assert_eq!(zrd.label(), ZRD_SOURCE_LABEL);
    assert_eq!(zrd.label(), "retail.zrd");

    for source in [&gamez, &zrd] {
        assert!(
            matches!(source.origin(), Origin::Installation { .. }),
            "{}: a measured source is declared over the installation it was measured in, not as \
             a design",
            source.label()
        );
        let convention = source.convention();
        assert_eq!(
            convention.axes(),
            [
                SourceAxis::positive(Axis::X),
                SourceAxis::positive(Axis::Y),
                SourceAxis::positive(Axis::Z),
            ],
            "{}: identity axis map, no axis is remapped or flipped",
            source.label()
        );
        assert_eq!(
            convention.winding_reference(),
            Axis::Z,
            "{}",
            source.label()
        );
        assert_eq!(
            convention.meters_per_unit(),
            1.0,
            "{}: one stored unit is one metre",
            source.label()
        );
        assert_eq!(
            convention.rotation_sense(),
            RotationSense::RightHandRule,
            "{}",
            source.label()
        );
        assert_eq!(
            convention.front_face(),
            Winding::CounterClockwise,
            "{}",
            source.label()
        );
        assert!(
            convention.is_orientation_preserving(),
            "{}: no mirror anywhere in the content-to-screen chain",
            source.label()
        );
        assert_eq!(
            source.provenance().class,
            ClaimStatus::ObservedTool,
            "{}: static analysis is the strongest class without an original run",
            source.label()
        );
        assert_eq!(
            source.calibration().source(),
            source.label(),
            "the attached calibration is about this source"
        );
    }

    // The one quantity the two formats spell differently.
    assert_eq!(
        gamez.convention().angle_unit(),
        AngleUnit::Radians,
        "radians in the engine and in the GameZ binaries"
    );
    assert_eq!(
        zrd.convention().angle_unit(),
        AngleUnit::Degrees,
        "degrees in the .zrd text fields"
    );
    assert_eq!(
        zrd.provenance().claim_id.as_str(),
        ZRD_DOCUMENT_CONVENTION_IS_MEASURED,
        "the .zrd declaration names the claim it stands on"
    );
}

/// **Three independent artifact landmarks per quantity, and no behavior.**
///
/// Independence is checked on the *observation* — source, fingerprint,
/// locator and method — the same rule `UnitCalibration::record` applies, so a
/// landmark cannot be manufactured by re-wording one inspection. The behavior
/// count is what keeps the requirement honestly open: static analysis cannot
/// supply one, and inventing one would be fabrication (owner decision 1).
#[test]
fn accept_f16_e_every_quantity_has_at_least_three_independent_artifact_landmarks() {
    let [gamez, zrd] = measured_sources();
    for source in [&gamez, &zrd] {
        let calibration = source.calibration();
        for quantity in CalibratedQuantity::ALL {
            let landmarks: Vec<_> = calibration
                .landmarks()
                .iter()
                .filter(|landmark| landmark.quantity() == quantity)
                .collect();
            let artifacts = landmarks
                .iter()
                .filter(|landmark| landmark.kind() == LandmarkKind::Artifact)
                .count();
            assert!(
                artifacts >= UnitCalibration::MIN_LANDMARKS,
                "{}: {} has {artifacts} artifact landmarks, the rule needs {}: {}",
                source.label(),
                quantity.label(),
                UnitCalibration::MIN_LANDMARKS,
                calibration.describe()
            );

            // Every landmark rests on its own observation.
            let mut observations: Vec<String> = landmarks
                .iter()
                .map(|landmark| {
                    let evidence = landmark.evidence();
                    format!(
                        "{:?}|{:?}|{:?}|{:?}",
                        evidence.source, evidence.fingerprint, evidence.locator, evidence.method
                    )
                })
                .collect();
            let total = observations.len();
            observations.sort();
            observations.dedup();
            assert_eq!(
                observations.len(),
                total,
                "{}: {} has two landmarks resting on one observation: {}",
                source.label(),
                quantity.label(),
                calibration.describe()
            );

            // And each description says what was observed.
            for landmark in &landmarks {
                assert!(
                    !landmark.description().trim().is_empty(),
                    "{}: {} has an undescribed landmark",
                    source.label(),
                    quantity.label()
                );
            }
        }
    }

    // The .zrd source recorded no behavior landmark at all...
    let zrd_calibration = zrd.calibration();
    for quantity in CalibratedQuantity::ALL {
        assert_eq!(
            zrd_calibration.behavior_landmark_count(quantity),
            0,
            "{}: no original run exists, so no behavior landmark may be recorded",
            quantity.label()
        );
    }

    // ...and the only behaviors the GameZ source carries are #677's tool-run
    // census, never a static-analysis artifact promoted to a behavior.
    let gamez_calibration = gamez.calibration();
    for quantity in CalibratedQuantity::ALL {
        let behaviors = gamez_calibration.behavior_landmark_count(quantity);
        if quantity == CalibratedQuantity::Scale {
            assert!(
                behaviors >= 1,
                "the #677 scale census still closes the scale quantity"
            );
        } else {
            assert_eq!(
                behaviors,
                0,
                "{}: no behavior landmark was recorded for F16-E",
                quantity.label()
            );
        }
    }
    for landmark in gamez_calibration.landmarks() {
        if landmark.kind() == LandmarkKind::Behavior {
            assert!(
                matches!(landmark.evidence().source, EvidenceSource::ToolRun { .. }),
                "a behavior landmark must come from an observed run, not from static analysis: {}",
                landmark.description()
            );
        }
    }
}

/// **The gaps say exactly what is missing: one behavior landmark per quantity.**
///
/// Every reported gap has its three artifacts already (`landmarks_recorded >=
/// MIN_LANDMARKS`, `landmarks_required == MIN_LANDMARKS`) and
/// `behavior_landmarks == 0`, so "0/1 behaviors" is the whole story of each
/// gap. `is_complete()` stays false — an artifact-only calibration can never
/// read as measured (`accept_f16_d_artifacts_alone_never_calibrate_a_quantity`
/// still guards the rule itself).
#[test]
fn accept_f16_e_gaps_report_exactly_the_missing_behavior_landmark_per_quantity() {
    let [gamez, zrd] = measured_sources();

    // retail.gamez: the scale quantity is closed by #677's observed
    // behaviors, the other three have their artifacts and lack a behavior.
    let gamez_gaps = gamez.calibration().gaps();
    assert_eq!(
        gamez_gaps
            .iter()
            .map(|gap| gap.quantity)
            .collect::<Vec<_>>(),
        [
            CalibratedQuantity::Handedness,
            CalibratedQuantity::AxisOrder,
            CalibratedQuantity::AngleUnit,
        ],
        "{}: {}",
        gamez.label(),
        gamez.calibration().describe()
    );

    // retail.zrd: no behavior anywhere, so every quantity is a gap.
    let zrd_gaps = zrd.calibration().gaps();
    assert_eq!(
        zrd_gaps.iter().map(|gap| gap.quantity).collect::<Vec<_>>(),
        CalibratedQuantity::ALL,
        "{}: {}",
        zrd.label(),
        zrd.calibration().describe()
    );

    for source in [&gamez, &zrd] {
        let calibration = source.calibration();
        assert!(
            !calibration.is_complete(),
            "{}: no original run, so the calibration cannot be complete: {}",
            source.label(),
            calibration.describe()
        );
        for gap in calibration.gaps() {
            assert!(
                gap.landmarks_recorded >= UnitCalibration::MIN_LANDMARKS,
                "{gap}: the artifacts are there, only the behavior is missing"
            );
            assert_eq!(
                gap.landmarks_required,
                UnitCalibration::MIN_LANDMARKS,
                "{gap}"
            );
            assert_eq!(
                gap.behavior_landmarks, 0,
                "{gap}: static analysis records artifacts, never behaviors"
            );
            assert!(
                gap.to_string().contains("0/1 behaviors"),
                "{gap}: the gap names the missing behavior landmark"
            );
        }
    }
}

/// **The claim is what the evidence supports, never `verified_original`.**
///
/// An incomplete calibration claims nothing at the convention level, the
/// quantities whose gaps are reported claim `unknown`, and the one quantity
/// #677 measured keeps its `observed_tool` class.
#[test]
fn accept_f16_e_claim_status_is_what_the_evidence_supports_and_never_verified_original() {
    let [gamez, zrd] = measured_sources();
    for source in [&gamez, &zrd] {
        let calibration = source.calibration();
        assert_eq!(
            calibration.claim_status(),
            ClaimStatus::Unknown,
            "{}: static analysis without an original run claims nothing at the convention level",
            source.label()
        );
        assert_ne!(
            calibration.claim_status(),
            ClaimStatus::VerifiedOriginal,
            "{}: never verified_original without a run of the original",
            source.label()
        );
        assert_ne!(
            source.provenance().class,
            ClaimStatus::VerifiedOriginal,
            "{}: the declaration's own class is observed_tool",
            source.label()
        );
        for gap in calibration.gaps() {
            assert_eq!(
                calibration.quantity_status(gap.quantity),
                ClaimStatus::Unknown,
                "{}: {} is a gap, so it claims nothing",
                source.label(),
                gap.quantity.label()
            );
        }
    }

    // The quantity #677 measured still reports the class its evidence reaches.
    assert_eq!(
        gamez
            .calibration()
            .quantity_status(CalibratedQuantity::Scale),
        ClaimStatus::ObservedTool,
        "the GameZ scale rests on a tool-run census over retail data"
    );
    assert_eq!(
        zrd.calibration().quantity_status(CalibratedQuantity::Scale),
        ClaimStatus::Unknown,
        "the .zrd scale has artifacts and no behavior, so it is still a gap"
    );
}

/// **A calibration belongs to the source it names.** The F16-D review note
/// (#1488) asked for this: a calibration about source A attached to source B
/// would report A's landmarks under B's gaps, and nothing in the type
/// caught it. `CoordinateSource::new` still starts empty, so the F16-A
/// fixtures and the F16-D declaration tests are untouched.
#[test]
fn accept_f16_e_with_calibration_refuses_a_calibration_for_another_source() {
    let mismatch = CoordinateSource::with_calibration(
        "this.source",
        convention(),
        Origin::Designed,
        provenance("f16e.test.mismatch"),
        UnitCalibration::new("another.source").expect("a named calibration is valid"),
    );
    assert_eq!(
        mismatch,
        Err(SourceError::CalibrationSourceMismatch {
            label: "this.source".to_owned(),
            calibration_source: "another.source".to_owned(),
        }),
        "a calibration about another source is refused at the boundary"
    );

    let empty_label = CoordinateSource::with_calibration(
        "",
        convention(),
        Origin::Designed,
        provenance("f16e.test.empty"),
        UnitCalibration::new("this.source").expect("a named calibration is valid"),
    );
    assert_eq!(empty_label, Err(SourceError::EmptyLabel));

    let attached = CoordinateSource::with_calibration(
        "this.source",
        convention(),
        Origin::Designed,
        provenance("f16e.test.attached"),
        UnitCalibration::new("this.source").expect("a named calibration is valid"),
    )
    .expect("a calibration about this source is accepted");
    assert_eq!(attached.calibration().source(), "this.source");
    assert!(attached.calibration().landmarks().is_empty());

    // `new` keeps the empty default: a declaration is not a measurement.
    let declared = CoordinateSource::new(
        "this.source",
        convention(),
        Origin::Designed,
        provenance("f16e.test.declared"),
    )
    .expect("a declaration is valid");
    let calibration = declared.calibration();
    assert!(
        calibration.landmarks().is_empty(),
        "a source built from a declaration arrives uncalibrated"
    );
    assert_eq!(calibration.gaps().len(), CalibratedQuantity::ALL.len());
    assert_eq!(calibration.claim_status(), ClaimStatus::Unknown);
}

/// **Every quantity's evidence is a static-analysis record a reader can go
/// back to: the image (or a retail file) and a locator.**
///
/// The landmarks F16-E recorded are `OriginalInstallation` records: byte
/// inspection of the decrypted image, located by a virtual address inside the
/// owner's measured range — or, for the one data landmark, the retail
/// archive's digest and the member span the value lives in. Each quantity of
/// each source needs at least `MIN_LANDMARKS` of them.
///
/// The only other records a measured source may carry are #677's tool-run
/// census landmarks: `ToolRun` source, `ToolProbe` method, located, and never
/// able to verify the original on their own.
#[test]
fn accept_f16_e_landmark_evidence_names_the_image_or_a_retail_file_and_where_it_was_read() {
    let [gamez, zrd] = measured_sources();
    for source in [&gamez, &zrd] {
        let calibration = source.calibration();
        for quantity in CalibratedQuantity::ALL {
            let mut static_analysis = 0usize;
            for landmark in calibration
                .landmarks()
                .iter()
                .filter(|landmark| landmark.quantity() == quantity)
            {
                let evidence = landmark.evidence();
                match &evidence.source {
                    EvidenceSource::OriginalInstallation => {
                        static_analysis += 1;
                        assert_eq!(
                            evidence.method,
                            ObservationMethod::ByteInspection,
                            "{}: static analysis inspects stored bytes: {}",
                            source.label(),
                            landmark.description()
                        );
                        let locator = evidence.locator.as_ref().unwrap_or_else(|| {
                            panic!("{}: every landmark is located", source.label())
                        });
                        let fingerprint = evidence.fingerprint.as_ref().unwrap_or_else(|| {
                            panic!(
                                "{}: every landmark fingerprints the image sha256 or the retail digest",
                                source.label()
                            )
                        });
                        if let Some(address) =
                            locator.container.strip_prefix("crimson.decrypted.exe VA ")
                        {
                            assert_eq!(
                                fingerprint.sha256.to_hex(),
                                ORIGINAL_IMAGE_SHA256,
                                "{}: code landmarks cite the owner's decrypted image",
                                source.label()
                            );
                            let va = u64::from_str_radix(address.trim_start_matches("0x"), 16)
                                .expect("a locator address is hexadecimal");
                            assert!(
                                va < 0x643000,
                                "the owner's file-offset rule (VA − 0x400000) covers addresses below \
                                 0x643000, so every recorded address is checkable in the image: {address}"
                            );
                            assert!(
                                locator.span.is_none(),
                                "a code landmark locates by address, not by a byte span"
                            );
                        } else {
                            assert_eq!(
                                locator.container,
                                format!("{ZRD_READER_ARCHIVE} member {ZRD_ANIMATION_MEMBER}"),
                                "{}: the data landmark locates a member",
                                source.label()
                            );
                            assert_eq!(
                                fingerprint.sha256.to_hex(),
                                ZRD_READER_ARCHIVE_SHA256,
                                "{}: the data landmark fingerprints the retail file",
                                source.label()
                            );
                            assert_eq!(
                                locator.span,
                                Some(EvidenceSpan {
                                    offset: ZRD_GRAVITY_VALUE_OFFSET,
                                    length: ZRD_GRAVITY_VALUE_LENGTH,
                                }),
                                "the data landmark points at the exact bytes it read"
                            );
                        }
                    }
                    EvidenceSource::ToolRun { .. } => {
                        assert_eq!(
                            evidence.method,
                            ObservationMethod::ToolProbe,
                            "{}: {} carries a tool-run census landmark",
                            source.label(),
                            landmark.description()
                        );
                        assert!(
                            evidence.locator.is_some(),
                            "{}: {} names the container the census ran over",
                            source.label(),
                            landmark.description()
                        );
                        assert!(
                            !evidence.verifies_original(),
                            "a tool run without an original-run fingerprint never verifies the \
                             original: {}",
                            landmark.description()
                        );
                    }
                    other => panic!(
                        "{}: {} carries evidence from {other:?}, which neither F16-E's static \
                         analysis nor the scale census records: {}",
                        source.label(),
                        quantity.label(),
                        landmark.description()
                    ),
                }
            }
            assert!(
                static_analysis >= UnitCalibration::MIN_LANDMARKS,
                "{}: {} has {static_analysis} static-analysis landmarks, F16-E recorded at least \
                 {}: {}",
                source.label(),
                quantity.label(),
                UnitCalibration::MIN_LANDMARKS,
                calibration.describe()
            );
        }
    }
}

/// **The designed registry is untouched by the measured sources**: the F16-A
/// declarations still arrive with an empty calibration and no installation
/// claim, and neither measured source is smuggled into that registry.
#[test]
fn accept_f16_e_declared_sources_still_arrive_uncalibrated() {
    let declared = CoordinateSource::declared();
    assert!(!declared.is_empty(), "the F16-A registry must not be empty");
    for source in &declared {
        assert!(
            matches!(source.origin(), Origin::Designed | Origin::SyntheticFixture),
            "{} must not claim installation data",
            source.label()
        );
        let calibration = source.calibration();
        assert!(
            calibration.landmarks().is_empty(),
            "{} must arrive with no landmarks",
            source.label()
        );
        assert_eq!(calibration.gaps().len(), CalibratedQuantity::ALL.len());
        assert_eq!(calibration.claim_status(), ClaimStatus::Unknown);
        assert!(
            source.label() != GAMEZ_SOURCE_LABEL && source.label() != ZRD_SOURCE_LABEL,
            "a measured source is not a designed declaration"
        );
    }
}

/// **The recorded hashes still describe this installation, and the data
/// landmark's bytes are the bytes that were recorded** (retail).
///
/// Every fingerprint the calibration cites is re-derived here: the owner's
/// decrypted image from `$CS_ENGINE_IMAGE` (#798), and from `$CS_GAME_DIR`
/// the installation and content hashes, the retail archive's digest, and the
/// `anim.zrd` member span that holds the `-9.8` gravity word. A landmark
/// recorded about different files fails here instead of standing as confident
/// prose.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_f16_e_the_recorded_hashes_still_describe_this_installation() {
    let root = std::path::PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set"));
    let found = cs_assets::install::discover(&root).expect("the installation is discovered");

    // The image every code landmark cites, read from `$CS_ENGINE_IMAGE` —
    // never from the installation: the image is not installation content
    // (#798), and the loader already refused anything that does not hash to
    // `ORIGINAL_IMAGE_SHA256`.
    let image = cs_content::coordinates::load_engine_image()
        .unwrap_or_else(|error| panic!("the owner's decrypted image loads: {error}"));
    assert_eq!(
        image.digest.to_hex(),
        ORIGINAL_IMAGE_SHA256,
        "the image the landmarks were measured in is byte-identical to the one recorded"
    );

    // The installation envelope the declaration's span sits in.
    assert_eq!(
        cs_assets::install::fingerprint(&found.manifest).to_hex(),
        INSTALL_SHA256,
        "the installation the landmarks describe"
    );
    assert_eq!(
        cs_assets::install::content_fingerprint(&found.manifest).to_hex(),
        CONTENT_SHA256,
        "and its content hash"
    );

    // The retail archive behind the one data landmark.
    let archive_path = root.join(ZRD_READER_ARCHIVE);
    let bytes = std::fs::read(&archive_path)
        .unwrap_or_else(|error| panic!("read {ZRD_READER_ARCHIVE}: {error}"));
    assert_eq!(
        cs_assets::install::sha256(&bytes).to_hex(),
        ZRD_READER_ARCHIVE_SHA256,
        "the retail file the data landmark fingerprints"
    );

    // Through the production reader chain: the member the locator names, the
    // span it claims, and the value at that span.
    let spelling = cs_types::install::RelativePath::new(ZRD_READER_ARCHIVE)
        .expect("the archive's own spelling is relative");
    let mut context = cs_formats::io::ParseContext::with_defaults("accept_f16_e");
    let decision = cs_formats::zbd::dispatch(cs_formats::zbd::ZbdProbe::new(
        ZRD_READER_ARCHIVE,
        &spelling,
        &bytes,
    ))
    .unwrap_or_else(|error| panic!("{ZRD_READER_ARCHIVE}: dispatch refused: {error}"));
    let index = cs_formats::zbd::read_version_one_index(&mut context, decision, &bytes)
        .expect("the reader archive indexes");
    let table = index.member_table();
    let archive = cs_formats::zbd::read_reader_archive(&mut context, &table, index.data())
        .expect("the reader archive reads");
    let member = archive
        .entries()
        .find(|entry| {
            String::from_utf8_lossy(entry.name()).eq_ignore_ascii_case(ZRD_ANIMATION_MEMBER)
        })
        .unwrap_or_else(|| panic!("{ZRD_READER_ARCHIVE} carries {ZRD_ANIMATION_MEMBER}"));
    let span = member.span();
    assert_eq!(
        (span.offset, span.length),
        (28_623, 8_335),
        "the anim.zrd member the findings note records"
    );
    assert!(
        span.offset <= ZRD_GRAVITY_VALUE_OFFSET
            && ZRD_GRAVITY_VALUE_OFFSET + ZRD_GRAVITY_VALUE_LENGTH <= span.offset + span.length,
        "the recorded value lives inside the {} member: member {}..{}, landmark {}..{}",
        ZRD_ANIMATION_MEMBER,
        span.offset,
        span.offset + span.length,
        ZRD_GRAVITY_VALUE_OFFSET,
        ZRD_GRAVITY_VALUE_OFFSET + ZRD_GRAVITY_VALUE_LENGTH
    );
    let start = usize::try_from(ZRD_GRAVITY_VALUE_OFFSET).expect("the offset fits a usize");
    let end = start + usize::try_from(ZRD_GRAVITY_VALUE_LENGTH).expect("the length fits a usize");
    let value = f32::from_le_bytes(
        bytes[start..end]
            .try_into()
            .expect("the recorded span is four bytes"),
    );
    assert_eq!(
        value.to_bits(),
        (-9.8_f32).to_bits(),
        "ANIMATION_DEFINITIONS/GRAVITY is the f32 -9.8 at the recorded span, an SI acceleration \
         in m/s^2"
    );

    // And a declaration built over this installation's own span, the way a
    // production caller supplies one.
    let install = cs_assets::install::fingerprint(&found.manifest);
    let member_row = archive
        .entries()
        .find(|entry| {
            String::from_utf8_lossy(entry.name()).eq_ignore_ascii_case(ZRD_ANIMATION_MEMBER)
        })
        .expect("the member was listed above");
    let real_span = InstallSpan::new(
        install,
        ZRD_READER_ARCHIVE,
        Some(ZRD_ANIMATION_MEMBER),
        member_row.span().offset,
        member_row.span().length,
        Some(cs_assets::install::sha256(member_row.content())),
    )
    .expect("the installation's own .zrd span is valid");
    let source = CoordinateSource::retail_zrd(real_span);
    assert!(matches!(source.origin(), Origin::Installation { .. }));
    assert_eq!(source.provenance().class, ClaimStatus::ObservedTool);
    assert_eq!(
        source
            .provenance()
            .source
            .as_ref()
            .map(|span| span.install_sha256()),
        Some(install),
        "the declaration's provenance points at this installation"
    );
}
