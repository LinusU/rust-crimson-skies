//! Font provenance and glyph coverage (F51-A).
//!
//! Non-negotiable behavior 1: no operating-system font and no proprietary game
//! font is ever bundled. Non-negotiable behavior 3: a missing glyph is counted,
//! not silently invisible.

use cs_content::localization::{
    FontCatalog, FontCatalogError, FontFace, FontFaceDraft, FontLicense, FontProvenance,
    FontRefusal, FontSource, GlyphCoverage, LicensePermission, synthetic_font_face,
};
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Origin, Provenance};
use cs_types::evidence::ContentHash;

use crate::common::claim;

fn font_id(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Font, key).expect("the font id is valid")
}

fn ascii_coverage() -> GlyphCoverage {
    GlyphCoverage::from_chars((0x20u8..0x7Fu8).map(char::from))
}

fn original_span() -> SourceSpan {
    SourceSpan::new(
        ContentHash::from_bytes([7; 32]),
        "fonts/ui.fnt",
        None,
        0,
        4096,
        None,
    )
    .expect("the fixture span is valid")
}

/// A draft for a font that came from the owner's installation and declares
/// `coverage`.
fn original_draft(id: &str, coverage: GlyphCoverage) -> FontFaceDraft {
    FontFaceDraft {
        id: font_id(id),
        family: "Original UI".to_owned(),
        source: FontSource::OriginalInstallation {
            span: Box::new(original_span()),
        },
        coverage,
        origin: Origin::SyntheticFixture,
        provenance: Provenance::designed(claim()),
    }
}

/// A draft for a source that is expected to be refused.
fn draft(source: FontSource) -> FontFaceDraft {
    FontFaceDraft {
        id: font_id("original.ui"),
        family: "Original UI".to_owned(),
        source,
        coverage: ascii_coverage(),
        origin: Origin::SyntheticFixture,
        provenance: Provenance::designed(claim()),
    }
}

/// An operating-system font and a proprietary game font are **refused**: they
/// can be named only so that the refusal can be tested. A `FontFace` can hold
/// only an original private font or a licensed fallback with verified
/// permission.
#[test]
fn accept_f51_a_operating_system_and_proprietary_fonts_are_refused() {
    assert_eq!(
        FontFace::try_new(draft(FontSource::OperatingSystem {
            family: "Arial".to_owned(),
        }))
        .expect_err("an operating-system font is never bundled"),
        FontRefusal::OperatingSystem {
            family: "Arial".to_owned()
        }
    );
    assert_eq!(
        FontFace::try_new(draft(FontSource::ProprietaryGame {
            family: "CrimsonSans".to_owned(),
        }))
        .expect_err("a proprietary game font is never redistributed"),
        FontRefusal::ProprietaryGame {
            family: "CrimsonSans".to_owned()
        }
    );
    assert_eq!(
        FontFace::try_new(draft(FontSource::LicensedFallback {
            license: FontLicense {
                name: "Unverified Face".to_owned(),
                permission: LicensePermission::Unverified {
                    reason: "the permission letter was never filed".to_owned(),
                },
            },
        }))
        .expect_err("an unverified license cannot ship"),
        FontRefusal::UnverifiedLicense {
            name: "Unverified Face".to_owned(),
            reason: "the permission letter was never filed".to_owned(),
        }
    );

    // The two accepted provenances, and nothing else.
    let original = FontFace::try_new(draft(FontSource::OriginalInstallation {
        span: Box::new(original_span()),
    }))
    .expect("an original font is loaded privately from the installation");
    assert!(matches!(
        original.provenance(),
        FontProvenance::OriginalPrivate { .. }
    ));
    assert!(
        !original.provenance().is_distributable(),
        "an original font is never redistributed"
    );

    let licensed = FontFace::try_new(draft(FontSource::LicensedFallback {
        license: FontLicense {
            name: "Verified Face".to_owned(),
            permission: LicensePermission::Verified {
                license: "ofl-1.1".to_owned(),
            },
        },
    }))
    .expect("a verified licensed fallback is admitted");
    assert!(licensed.provenance().is_distributable());
}

/// The packaging question — "may this ship?" — is answered by the catalog, and
/// an original private font is excluded from it, so a release build cannot ship
/// one by asking the wrong method. A duplicate font id is refused by name.
#[test]
fn accept_f51_a_the_font_catalog_only_offers_distributable_fonts() {
    let mut catalog = FontCatalog::new();
    let original = FontFace::try_new(draft(FontSource::OriginalInstallation {
        span: Box::new(original_span()),
    }))
    .expect("an original font is admitted");
    catalog
        .insert(original.clone())
        .expect("the original font inserts");
    assert!(matches!(
        catalog
            .insert(original)
            .expect_err("a duplicate font id is refused"),
        FontCatalogError::DuplicateId { .. }
    ));
    assert_eq!(catalog.len(), 1, "the refusal left the first row in place");
    catalog
        .insert(synthetic_font_face())
        .expect("the licensed fallback inserts");
    assert_eq!(catalog.len(), 2);
    let distributable: Vec<&str> = catalog.distributable().map(|face| face.family()).collect();
    assert_eq!(distributable, vec!["Synthetic UI"]);
}

/// A character the font has no glyph for is *counted*, with its occurrences,
/// rather than being dropped from the text.
#[test]
fn accept_f51_a_missing_glyphs_are_counted_not_silently_invisible() {
    let ascii = ascii_coverage();
    assert!(!ascii.covers('ä'));
    assert!(ascii.covers('A'));

    let report = ascii.missing_in("Grüße");
    assert_eq!(report.missing(), &['ü', 'ß']);
    assert_eq!(report.count('ü'), 1);
    assert_eq!(report.count('ß'), 1);
    // Whitespace is not a glyph and is never reported missing.
    assert!(!report.missing().contains(&' '));
    assert_eq!(report.total(), 2);
    assert!(ascii.missing_in("plain ascii").is_empty());

    // Across a catalog a character is only reported when *no* declared font has
    // it, because a screen may legitimately draw with any of them.
    let mut catalog = FontCatalog::new();
    catalog
        .insert(synthetic_font_face())
        .expect("the ascii face inserts");
    assert_eq!(
        catalog.missing_glyphs(&["Grüße".to_owned()]).missing(),
        &['ü', 'ß']
    );

    let mut wide = ascii_coverage();
    wide.insert('ä');
    wide.insert('ü');
    catalog
        .insert(
            FontFace::try_new(original_draft("wide.ui", wide))
                .expect("the wider original font is admitted"),
        )
        .expect("the wider font inserts");
    let report = catalog.missing_glyphs(&["Grüße".to_owned()]);
    assert_eq!(
        report.missing(),
        &['ß'],
        "'ä' is covered by the second face"
    );
    assert_eq!(report.count('ß'), 1);
}
