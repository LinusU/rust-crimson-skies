//! `accept_f17_a_` tests for the material-classification contract:
//! `cs_app::render::material`.

use cs_app::render::material::{
    AddressMode, Classification, ClassificationFailure, Coverage, DeclaredClass, MaterialClass,
    MaterialFacts, MaterialUnknown, RenderPhase, TextureAddress, classify,
};
use cs_formats::gamez::{MATERIAL_FLAG_ALWAYS, MATERIAL_FLAG_TEXTURED, RawMaterialRecord};
use cs_formats::texture::{AlphaSource, AlphaTest};
use cs_types::evidence::ClaimStatus;

fn declared(class: MaterialClass) -> Option<DeclaredClass> {
    Some(DeclaredClass::new(class, ClaimStatus::Designed).expect("Designed asserts a class"))
}

fn facts(class: MaterialClass, coverage: Coverage, alpha_test: AlphaTest) -> MaterialFacts {
    MaterialFacts {
        declared: declared(class),
        coverage,
        alpha_test,
        two_sided: Some(false),
        addressing: Some(TextureAddress {
            u: AddressMode::Repeat,
            v: AddressMode::Repeat,
        }),
        vertex_colors: false,
        unknown_flag_bits: 0,
    }
}

fn reason_codes(classification: &Classification) -> Vec<&'static str> {
    classification
        .reasons()
        .iter()
        .map(ClassificationFailure::code)
        .collect()
}

/// The golden-scene materials classify: a declared class plus consistent
/// coverage facts yields the class's phase and surface attributes.
#[test]
fn accept_f17_a_declared_class_with_consistent_facts_classifies() {
    let fence = facts(
        MaterialClass::Masked,
        Coverage::Texture(AlphaSource::Channel),
        AlphaTest::Threshold(0x80),
    );
    let material = classify(&fence)
        .classified()
        .expect("a fence's facts classify it")
        .clone();
    assert_eq!(material.class(), MaterialClass::Masked);
    assert_eq!(material.phase(), RenderPhase::Masked);
    assert_eq!(material.alpha_test(), AlphaTest::Threshold(0x80));
    assert_eq!(material.coverage(), Coverage::Texture(AlphaSource::Channel));
    assert!(material.two_sided() == Some(false));
    assert!(material.is_release_ready());

    let glass = facts(
        MaterialClass::Blended,
        Coverage::Uniform(102),
        AlphaTest::Disabled,
    );
    assert_eq!(
        classify(&glass).classified().map(|m| m.phase()),
        Some(RenderPhase::Translucent)
    );
    let sprite = facts(
        MaterialClass::Additive,
        Coverage::Opaque,
        AlphaTest::Disabled,
    );
    assert_eq!(
        classify(&sprite).classified().map(|m| m.phase()),
        Some(RenderPhase::Additive)
    );
}

/// A material nobody asserts a class for is unclassified with the
/// `undeclared` reason — never silently drawn as opaque (spec F17
/// non-negotiable #1 and the contract's "unknown means unknown").
#[test]
fn accept_f17_a_undeclared_material_is_never_opaque() {
    let mut silence = facts(
        MaterialClass::Opaque,
        Coverage::Texture(AlphaSource::Channel),
        AlphaTest::Disabled,
    );
    silence.declared = None;
    let classification = classify(&silence);
    assert!(classification.classified().is_none());
    assert_eq!(reason_codes(&classification), ["undeclared"]);
}

/// What a stored GameZ record alone establishes is never enough to
/// classify: the record does not name a render class, and unmapped flag
/// bits make even its textured-ness suspect. Both states are reported.
#[test]
fn accept_f17_a_raw_record_alone_cannot_classify() {
    let textured = RawMaterialRecord {
        alpha: 0xFF,
        flags: MATERIAL_FLAG_TEXTURED | MATERIAL_FLAG_ALWAYS,
        rgb: 0x7FFF,
        color: [255.0, 255.0, 255.0],
        texture_index: 5,
        field20: 0.0,
        field24: 0.5,
        field28: 0.5,
        field32: 0.0,
        cycle_ptr: 0,
    };
    let classification = classify(&MaterialFacts::for_raw_record(&textured));
    assert_eq!(reason_codes(&classification), ["undeclared"]);

    let mut suspect = textured;
    suspect.flags |= 1 << 3; // a bit no reference names
    let classification = classify(&MaterialFacts::for_raw_record(&suspect));
    assert_eq!(
        reason_codes(&classification),
        ["unknown_flag_bits", "undeclared"]
    );
    match &classification {
        Classification::Unclassified { reasons } => assert_eq!(
            reasons[0],
            ClassificationFailure::UnknownFlagBits { bits: 1 << 3 }
        ),
        Classification::Classified(_) => panic!("suspect flags must not classify"),
    }
}

/// A declared class that needs coverage contradicts a surface the facts
/// say has none, and cannot stand on an unestablished coverage source.
#[test]
fn accept_f17_a_declared_class_must_match_coverage() {
    let no_alpha = facts(
        MaterialClass::Blended,
        Coverage::Opaque,
        AlphaTest::Disabled,
    );
    assert_eq!(
        reason_codes(&classify(&no_alpha)),
        ["class_without_coverage"]
    );

    let mystery = facts(
        MaterialClass::Blended,
        Coverage::Unknown,
        AlphaTest::Disabled,
    );
    assert_eq!(reason_codes(&classify(&mystery)), ["coverage_unknown"]);

    let mask_on_opaque = facts(
        MaterialClass::Masked,
        Coverage::Opaque,
        AlphaTest::Threshold(0x80),
    );
    assert_eq!(
        reason_codes(&classify(&mask_on_opaque)),
        ["class_without_coverage"]
    );

    // The declared class is named inside the failure, not just in a code.
    match classify(&no_alpha) {
        Classification::Unclassified { reasons } => assert_eq!(
            reasons[0],
            ClassificationFailure::ClassWithoutCoverage {
                class: MaterialClass::Blended
            }
        ),
        Classification::Classified(_) => panic!("opaque glass must not classify"),
    }
}

/// A mask needs its cut point: `Unknown` is reported, `Disabled`
/// contradicts the declaration, only `Threshold` classifies.
#[test]
fn accept_f17_a_masked_needs_a_threshold() {
    let unknown = facts(
        MaterialClass::Masked,
        Coverage::Texture(AlphaSource::Channel),
        AlphaTest::Unknown,
    );
    assert_eq!(reason_codes(&classify(&unknown)), ["alpha_test_unknown"]);

    let disabled = facts(
        MaterialClass::Masked,
        Coverage::Texture(AlphaSource::Channel),
        AlphaTest::Disabled,
    );
    assert_eq!(reason_codes(&classify(&disabled)), ["masked_test_disabled"]);

    let cut = facts(
        MaterialClass::Masked,
        Coverage::Texture(AlphaSource::StoredValueKey { value: 0 }),
        AlphaTest::Threshold(1),
    );
    assert!(classify(&cut).classified().is_some());
}

/// Classified is not release-ready: two-sidedness, texture addressing and
/// per-corner color meaning stay on the unknowns list until measured.
#[test]
fn accept_f17_a_unmeasured_presentation_stays_unknown() {
    let mut incomplete = facts(MaterialClass::Opaque, Coverage::Opaque, AlphaTest::Disabled);
    incomplete.two_sided = None;
    incomplete.addressing = None;
    incomplete.vertex_colors = true;
    let material = classify(&incomplete)
        .classified()
        .expect("facts classify, decisions stay open")
        .clone();
    assert!(!material.is_release_ready());
    assert_eq!(
        material.unknowns(),
        [
            MaterialUnknown::TwoSided,
            MaterialUnknown::TextureAddressing,
            MaterialUnknown::VertexColorMeaning
        ]
    );
    assert!(material.vertex_colors());
}

/// `Unknown` and `Contradicted` cannot be a class declaration's status —
/// an assertion that asserts nothing is refused at construction.
#[test]
fn accept_f17_a_declaration_status_must_assert() {
    for status in [ClaimStatus::Unknown, ClaimStatus::Contradicted] {
        assert!(DeclaredClass::new(MaterialClass::Opaque, status).is_err());
    }
    for status in [
        ClaimStatus::Designed,
        ClaimStatus::Documented,
        ClaimStatus::ObservedTool,
        ClaimStatus::VerifiedOriginal,
        ClaimStatus::Inferred,
    ] {
        assert!(DeclaredClass::new(MaterialClass::Opaque, status).is_ok());
    }
}

/// A descriptor's `AlphaSource` projects onto the coverage vocabulary:
/// `Opaque` and `Unknown` normalize, real sources stay `Texture`.
#[test]
fn accept_f17_a_coverage_normalizes_descriptor_alpha_source() {
    assert_eq!(Coverage::from_source(AlphaSource::Opaque), Coverage::Opaque);
    assert_eq!(
        Coverage::from_source(AlphaSource::Unknown),
        Coverage::Unknown
    );
    assert_eq!(
        Coverage::from_source(AlphaSource::Channel),
        Coverage::Texture(AlphaSource::Channel)
    );
    assert_eq!(
        Coverage::from_source(AlphaSource::PaletteKey { index: 3 }),
        Coverage::Texture(AlphaSource::PaletteKey { index: 3 })
    );
    // `has_coverage` is what the class check reads.
    assert!(!Coverage::Opaque.has_coverage());
    assert!(!Coverage::Unknown.has_coverage());
    assert!(Coverage::Uniform(0).has_coverage());
    assert!(Coverage::Uniform(255).has_coverage());
}
