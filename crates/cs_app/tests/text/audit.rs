//! F51-D acceptance tests: the per-locale, per-string-image coverage and
//! overflow audit, the media license/glyph review, and the real-GPU witness
//! that the laid-out line geometry is drawable.
//!
//! Spec: `specs/F51-localization-fonts-text-layout-and-original-media-ids.md`,
//! stage `### F51-D`; shared contract `docs/contracts/UI-NETWORK.md`. Task test
//! prefix: `accept_f51_d_`. Every test calls production code
//! (`cs_app::text::audit::audit_localization`, `cs_app::text::gpu_capture`);
//! none carries its own decode, its own coverage walk or its own frame reader.
//!
//! The synthetic tests are unignored so CI runs them. The two GPU tests need a
//! real adapter and the retail test needs `$CS_GAME_DIR`, so CI skips them with
//! `#[ignore]`; the implementing and reviewing agents run them with
//! `--include-ignored`. A retail test fails loudly, never passes, when its
//! capability is absent.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use bevy::math::Rect;

use cs_app::text::{
    AuditBlocker, GlyphEvidence, LocalizationAuditRequest, MediaSource, StringImageSource,
    TEXT_CAPTURE_HEIGHT, TEXT_CAPTURE_WIDTH, TextCaptureError, audit_localization,
    capture_text_boxes, synthetic_monospace, text_boxes,
};
use cs_content::localization::{
    FontProvenance, GlyphCoverage, LocaleChain, ResourceDecode, SupportedLocales, TextId,
    TextResolution, parse_markup, synthetic_font_face,
};
use cs_types::asset_id::SourceSpan;
use cs_types::content::{Origin, Provenance};
use cs_types::evidence::ContentHash;

use crate::common::{
    PANEL, RETAIL_FONT_MEDIA, RETAIL_STRING_IMAGES, buttons, claim, grammar, language_map, locale,
    resource_row, retail_audit, retail_game_dir, substitutions,
};
use cs_app::text::layout::{LayoutRequest, layout_text};

/// A validated declared supported-locale set.
fn supported(labels: &[&str]) -> SupportedLocales {
    SupportedLocales::new(labels.iter().copied().map(locale))
        .expect("the fixture supported-locale set is valid")
}

/// A validated declared supported-locale set of exactly one locale.
fn one_locale(label: &str) -> SupportedLocales {
    supported(&[label])
}

/// A disposable directory, removed on drop, for captures that are not evidence.
struct TempBase(PathBuf);

impl TempBase {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f51-d-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the fixture base is created");
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempBase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Where the GPU witness writes its PNGs: the evidence directory when the
/// caller names one (the evidence run sets `CS_EVIDENCE_DIR`), otherwise a
/// leaked temporary directory, because a capture that is not being collected as
/// evidence need not survive the test.
///
/// Cargo runs the test binary with its working directory set to the *package*
/// root, so a relative `CS_EVIDENCE_DIR` is re-anchored to the workspace root
/// here — the same rule the evidence harness applies.
fn capture_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CS_EVIDENCE_DIR") {
        let dir = PathBuf::from(dir);
        let dir = if dir.is_absolute() {
            dir
        } else {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("crates/")
                .parent()
                .expect("workspace root")
                .join(dir)
        };
        fs::create_dir_all(&dir).expect("the evidence directory is created");
        dir
    } else {
        let temp = TempBase::new("captures");
        let path = temp.path().to_path_buf();
        std::mem::forget(temp);
        path
    }
}

/// An original-private provenance for a font inside the installation.
fn original_provenance(path: &str, len: u64) -> FontProvenance {
    let span = SourceSpan::new(ContentHash::from_bytes([9; 32]), path, None, 0, len, None)
        .expect("the fixture font span is valid");
    FontProvenance::OriginalPrivate {
        span: Box::new(span),
    }
}

/// A long localized string, so the panel has to scroll rather than fit.
fn long_string() -> String {
    "word ".repeat(600)
}

/// The audit finds every declared locale, counts the ids each answers itself and
/// the ids it borrows, and reports a fully covered synthetic image as complete.
#[test]
fn accept_f51_d_every_declared_locale_is_audited_against_each_string_image() {
    let rows = [
        resource_row(10, 1033, Some("New Game")),
        resource_row(10, 1031, Some("Neues Spiel")),
        resource_row(11, 1033, Some("Airspeed")),
        resource_row(12, 1031, Some("Bandit")),
    ];
    let declared = supported(&["en-us", "de-de", "fr-fr"]);
    let languages = language_map(&[(1033, "en-us"), (1031, "de-de")]);
    let grammar = grammar();
    let metrics = synthetic_monospace(16.0);
    let required = buttons();
    let substitutions = substitutions();
    let images = [StringImageSource {
        path: "strings.dll",
        rows: &rows,
        origin: Origin::SyntheticFixture,
        provenance: Provenance::designed(claim()),
    }];

    let audit = audit_localization(&LocalizationAuditRequest {
        images: &images,
        locales: &declared,
        language_map: &languages,
        grammar: &grammar,
        metrics: &metrics,
        panel: PANEL,
        required: &required,
        substitutions: &substitutions,
        media: &[],
    });

    assert!(
        audit.is_complete(),
        "a fully covered synthetic image is complete, got {:?}",
        audit.blockers
    );
    let image = audit.image("strings.dll").expect("the image was audited");
    assert_eq!(image.ids, 3, "three distinct ids, not four rows");
    assert_eq!(image.rows, 4);
    assert_eq!(image.decoded, 4);
    assert!(image.undeclared.is_empty());
    assert!(image.missing_everywhere.is_empty());

    let en = image.locale(&locale("en-us")).expect("en-us was declared");
    let de = image.locale(&locale("de-de")).expect("de-de was declared");
    let fr = image.locale(&locale("fr-fr")).expect("fr-fr was declared");
    assert_eq!(
        (en.translated, en.via_fallback, en.missing),
        (2, 1, 0),
        "en-us answers 10 and 11 itself and borrows 12 from de-de"
    );
    assert_eq!((de.translated, de.via_fallback, de.missing), (2, 1, 0));
    assert_eq!(
        (fr.translated, fr.via_fallback, fr.missing),
        (0, 3, 0),
        "fr-fr answers nothing itself and borrows every id"
    );
    assert_eq!(
        en.laid_out, 3,
        "every id resolved somewhere in en-us's chain"
    );
    assert_eq!(en.overflowing, 0);
    assert_eq!(en.covering_controls, 0);
}

/// An id no declared locale answers, a locale nobody declared, an undecodable
/// row, an unmapped language and a duplicated `(id, locale)` pair are each a
/// named blocker; none is dropped.
#[test]
fn accept_f51_d_unanswered_ids_and_unaccounted_rows_are_named_blockers() {
    let rows = [
        resource_row(10, 1033, Some("New Game")),
        // Mapped to de-de, but de-de is not a declared supported locale.
        resource_row(11, 1031, Some("Nur Deutsch")),
        // Code units that did not decode.
        resource_row(12, 1033, None),
        // A language the caller's map does not declare.
        resource_row(13, 9999, Some("Unmapped")),
        // Two strings for one (id, locale) contradict each other; neither is kept.
        resource_row(14, 1033, Some("First")),
        resource_row(14, 1033, Some("Second")),
    ];
    let declared = one_locale("en-us");
    let languages = language_map(&[(1033, "en-us"), (1031, "de-de")]);
    let grammar = grammar();
    let metrics = synthetic_monospace(16.0);
    let required = buttons();
    let substitutions = substitutions();
    let images = [StringImageSource {
        path: "langui.dll",
        rows: &rows,
        origin: Origin::SyntheticFixture,
        provenance: Provenance::designed(claim()),
    }];

    let audit = audit_localization(&LocalizationAuditRequest {
        images: &images,
        locales: &declared,
        language_map: &languages,
        grammar: &grammar,
        metrics: &metrics,
        panel: PANEL,
        required: &required,
        substitutions: &substitutions,
        media: &[],
    });

    assert!(!audit.is_complete());
    let image = audit.image("langui.dll").expect("the image was audited");
    // Only the two mapped, decodable, unique rows became catalog rows.
    assert_eq!(image.ids, 2, "ids 10 (en-us) and 11 (de-de) survive");
    assert_eq!(image.decoded, 2);
    assert_eq!(image.rows, 6);
    assert_eq!(image.undeclared, vec![locale("de-de")]);
    assert_eq!(image.missing_everywhere, vec![TextId::from_resource_id(11)]);
    assert_eq!(image.unmapped_languages, vec![9999]);
    assert_eq!(image.undecodable_ids, vec![12]);
    assert_eq!(image.duplicates, vec![(14, locale("en-us"))]);
    let en = image.locale(&locale("en-us")).expect("en-us was declared");
    assert_eq!((en.translated, en.missing), (1, 1));

    let codes: Vec<&str> = audit.blockers.iter().map(AuditBlocker::code).collect();
    for expected in [
        "undeclared_locale",
        "missing_everywhere",
        "unmapped_language",
        "undecodable_row",
        "duplicate_row",
    ] {
        assert_eq!(
            codes.iter().filter(|code| **code == expected).count(),
            1,
            "expected exactly one {expected} blocker, got {codes:?}"
        );
    }
}

/// A long translation is counted as overflowing per locale, and the layout never
/// paints a line over a required control however long the text is.
#[test]
fn accept_f51_d_overflowing_text_is_counted_per_locale_and_never_covers_a_control() {
    let long = long_string();
    let rows = [
        resource_row(10, 1033, Some("New Game")),
        resource_row(11, 1033, Some(&long)),
    ];
    let declared = one_locale("en-us");
    let languages = language_map(&[(1033, "en-us")]);
    let grammar = grammar();
    let metrics = synthetic_monospace(16.0);
    let required = buttons();
    let substitutions = substitutions();
    let images = [StringImageSource {
        path: "strings.dll",
        rows: &rows,
        origin: Origin::SyntheticFixture,
        provenance: Provenance::designed(claim()),
    }];

    let audit = audit_localization(&LocalizationAuditRequest {
        images: &images,
        locales: &declared,
        language_map: &languages,
        grammar: &grammar,
        metrics: &metrics,
        panel: PANEL,
        required: &required,
        substitutions: &substitutions,
        media: &[],
    });

    let image = audit.image("strings.dll").expect("the image was audited");
    let en = image.locale(&locale("en-us")).expect("en-us was declared");
    assert_eq!(en.laid_out, 2);
    assert_eq!(
        en.overflowing, 1,
        "the long string must scroll in the free band"
    );
    assert_eq!(
        en.covering_controls, 0,
        "AC01: a laid-out line never reaches a reserved button"
    );
    assert_eq!(
        audit.blockers_with_code("overflow").count(),
        1,
        "the overflow is named once"
    );
    assert_eq!(audit.blockers_with_code("covers_control").count(), 0);
    match audit.blockers.iter().find(|b| b.code() == "overflow") {
        Some(AuditBlocker::Overflow {
            locale: audited,
            strings,
            ..
        }) => {
            assert_eq!(audited, &locale("en-us"));
            assert_eq!(*strings, 1);
        }
        other => panic!("expected the overflow blocker, got {other:?}"),
    }
}

/// An original private font is never distributable and its unmeasured glyph
/// coverage is a blocker; a licensed fallback with verified permission is
/// distributable and its declared coverage is evidence.
#[test]
fn accept_f51_d_media_license_and_unmeasured_glyphs_are_audited_not_assumed() {
    let rows = [resource_row(10, 1033, Some("New Game"))];
    let declared = one_locale("en-us");
    let languages = language_map(&[(1033, "en-us")]);
    let grammar = grammar();
    let metrics = synthetic_monospace(16.0);
    let required = buttons();
    let substitutions = substitutions();
    let images = [StringImageSource {
        path: "strings.dll",
        rows: &rows,
        origin: Origin::SyntheticFixture,
        provenance: Provenance::designed(claim()),
    }];

    let font_bytes = [0u8; 24];
    let media = [
        MediaSource {
            path: "GOSDATA/ASSETS/GRAPHICS/font.tga",
            bytes: &font_bytes,
            provenance: original_provenance("GOSDATA/ASSETS/GRAPHICS/font.tga", 24),
            glyphs: GlyphEvidence::Unmeasured {
                reason: "the original bitmap font's cell-to-character mapping is unmeasured"
                    .to_owned(),
            },
        },
        MediaSource {
            path: "synthetic/fallback.ttf",
            bytes: &font_bytes,
            provenance: synthetic_font_face().provenance().clone(),
            glyphs: GlyphEvidence::Declared {
                coverage: GlyphCoverage::from_chars("NewGame ".chars()),
            },
        },
    ];

    let audit = audit_localization(&LocalizationAuditRequest {
        images: &images,
        locales: &declared,
        language_map: &languages,
        grammar: &grammar,
        metrics: &metrics,
        panel: PANEL,
        required: &required,
        substitutions: &substitutions,
        media: &media,
    });

    let original = audit
        .media("GOSDATA/ASSETS/GRAPHICS/font.tga")
        .expect("the original font was audited");
    assert!(
        !original.distributable,
        "an original private font never ships"
    );
    assert!(!original.glyphs.is_measured());
    assert_eq!(original.bytes, 24);
    let fallback = audit
        .media("synthetic/fallback.ttf")
        .expect("the fallback was audited");
    assert!(
        fallback.distributable,
        "a licensed fallback with verified permission ships"
    );
    assert!(fallback.glyphs.is_measured());
    assert_eq!(audit.distributable_media(), 1);

    assert_eq!(audit.blockers.len(), 1, "only the unmeasured font blocks");
    assert_eq!(audit.blockers[0].code(), "unmeasured_glyphs");
    assert!(!audit.is_complete());
}

/// The real adapter draws the production line boxes: the capture is produced,
/// measured non-uniform, and carries the frame's geometry facts.
#[test]
#[ignore = "requires a GPU adapter; run with --include-ignored"]
fn accept_f51_d_a_gpu_capture_proves_the_laid_out_lines_were_drawn() {
    let rows = [
        resource_row(10, 1033, Some("New Game")),
        resource_row(10, 1031, Some("Neues Spiel")),
        resource_row(10, 1036, Some("Nouvelle partie")),
    ];
    let declared = supported(&["en-us", "de-de", "fr-fr"]);
    let languages = language_map(&[(1033, "en-us"), (1031, "de-de"), (1036, "fr-fr")]);
    let catalog = ResourceDecode::decode(
        &rows,
        &languages,
        Origin::SyntheticFixture,
        Provenance::designed(claim()),
    )
    .into_catalog();
    let grammar = grammar();
    let metrics = synthetic_monospace(16.0);
    let required = buttons();
    let substitutions = substitutions();
    let dir = capture_dir();

    let mut captures = 0usize;
    let mut digests = std::collections::BTreeSet::new();
    for wanted in declared.locales() {
        let chain = LocaleChain::new(
            wanted.clone(),
            declared
                .locales()
                .iter()
                .filter(|other| *other != wanted)
                .cloned(),
        )
        .expect("the declared chain is valid");
        let id = TextId::from_resource_id(10);
        let TextResolution::Resolved { row, .. } = catalog.resolve(&id, &chain) else {
            panic!("the fixture id resolves for {wanted}");
        };
        let document = parse_markup(row.text(), &grammar);
        let layout = layout_text(&LayoutRequest {
            id: Some(id.clone()),
            document: &document,
            metrics: &metrics,
            substitutions: &substitutions,
            panel: PANEL,
            required: &required,
        })
        .expect("the fixture string lays out");
        let boxes = text_boxes(&layout, PANEL);
        assert!(
            !boxes.is_empty(),
            "a laid-out string paints at least one line box"
        );
        let png = dir.join(format!("render-{}.png", wanted.as_str()));
        let capture = capture_text_boxes(wanted.as_str(), &boxes, &png)
            .unwrap_or_else(|error| panic!("the capture for {wanted} drew nothing: {error}"));
        assert!(
            capture.drew_lines(),
            "the frame must differ from its clear colour"
        );
        assert_eq!(capture.width, TEXT_CAPTURE_WIDTH);
        assert_eq!(capture.height, TEXT_CAPTURE_HEIGHT);
        assert!(capture.covered_pixels > 0);
        assert!(capture.png_bytes > 0);
        assert!(
            png.is_file(),
            "the capture's PNG {} must exist on disk",
            png.display()
        );
        digests.insert(capture.png_sha256.to_hex());
        captures += 1;
    }
    assert_eq!(captures, 3, "one measured frame per declared locale");
    assert_eq!(
        digests.len(),
        3,
        "the frames must differ by locale, so the capture is not a static fixture"
    );
}

/// A capture that would draw nothing is refused with a named error and leaves no
/// PNG behind; the production geometry filter is what makes the box list empty.
#[test]
#[ignore = "requires a GPU adapter; run with --include-ignored"]
fn accept_f51_d_a_capture_that_drew_nothing_is_refused_rather_than_written() {
    let dir = capture_dir();
    let png = dir.join("render-refused.png");

    let error = capture_text_boxes("empty", &[], &png)
        .expect_err("an empty box list is refused before the renderer runs");
    assert!(matches!(error, TextCaptureError::NoVisibleLines));
    assert!(!png.exists(), "a refused capture leaves no file");

    // A degenerate panel yields no box, so even a laid-out document cannot be
    // captured as something it is not.
    let document = parse_markup("New Game", &grammar());
    let metrics = synthetic_monospace(16.0);
    let required = buttons();
    let substitutions = substitutions();
    let layout = layout_text(&LayoutRequest {
        id: None,
        document: &document,
        metrics: &metrics,
        substitutions: &substitutions,
        panel: PANEL,
        required: &required,
    })
    .expect("the fixture lays out");
    let boxes = text_boxes(&layout, Rect::new(0.0, 0.0, 0.0, 0.0));
    assert!(boxes.is_empty(), "an empty panel maps to no box");
    let error = capture_text_boxes("degenerate", &boxes, &png)
        .expect_err("a capture with nothing to draw is refused");
    assert!(matches!(error, TextCaptureError::NoVisibleLines));
    assert!(!png.exists(), "a refused capture leaves no file");
}

/// The retail half of the audit: every PE string image is read through the
/// production reader, every row under the declared locale is counted, and the
/// two original bitmap fonts are recorded with their unmeasured glyph coverage.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f51_d_retail_every_string_image_and_font_is_audited_for_the_declared_locale() {
    let dir = retail_game_dir();
    // The whole installation audit, assembled by the shared test scaffolding
    // exactly as the evidence harness assembles it.
    let audit = retail_audit(&dir);

    // The measured magnitudes F12-A/F12-D recorded, re-read through production.
    let expected_rows = [1792usize, 1616, 48];
    for (spelling, expected) in RETAIL_STRING_IMAGES.iter().zip(expected_rows) {
        let image = audit.image(spelling).expect("every image was audited");
        assert_eq!(image.rows, expected, "{spelling} RT_STRING units");
        assert!(image.ids > 0, "{spelling} has string ids");
        assert_eq!(
            image.decoded, image.ids,
            "every {spelling} unit decodes and maps under the declared language map"
        );
        let en = image.locale(&locale("en-us")).expect("en-us was declared");
        assert_eq!(en.missing, 0, "en-us answers every {spelling} id");
        assert_eq!(en.translated, image.ids);
        assert_eq!(en.via_fallback, 0);
        assert_eq!(en.covering_controls, 0, "AC01: no line reaches a button");
    }

    // The two fonts are audited, are never distributable, and their unmeasured
    // glyph coverage is a named blocker rather than a guessed pass.
    for spelling in RETAIL_FONT_MEDIA {
        let media = audit.media(spelling).expect("every font was audited");
        assert!(media.bytes > 0, "{spelling} has bytes");
        assert!(
            !media.distributable,
            "{spelling} is original and never ships"
        );
        assert!(!media.glyphs.is_measured());
    }
    assert_eq!(
        audit.blockers_with_code("unmeasured_glyphs").count(),
        RETAIL_FONT_MEDIA.len(),
        "one unmeasured-glyph blocker per original font"
    );
    assert_eq!(audit.distributable_media(), 0);
    assert!(
        !audit.is_complete(),
        "the audit is honestly incomplete while the original font mapping is unmeasured"
    );
}
