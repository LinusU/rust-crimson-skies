//! F51-FONTCELL acceptance tests: the original bitmap fonts' glyph coverage
//! measured with the colour-key cell rule, and the audit verdicts that replace
//! the former "unmeasured" state (Rally #466).
//!
//! Spec: `specs/F51-localization-fonts-text-layout-and-original-media-ids.md`,
//! stage `### F51-D`'s follow-up; shared contract `docs/contracts/UI-NETWORK.md`.
//! Task test prefix: `accept_f51_fontcell_`. Every test calls production code
//! (`cs_app::text::original_font::measure_rimage_bitmap_fonts`, which reads
//! through `cs_formats::texture::read_zbd_textures` and decodes through
//! `ZbdTexture::decode`, and `cs_app::text::audit::audit_localization`); none
//! carries its own separator test, its own cell scan or its own audit walk.
//!
//! The synthetic packages here are **authored fixtures**, built byte by byte
//! from the ZBD layout `docs/findings/2026-09-28-f08-b-02-zbd-texture-package.md`
//! records. They carry no original pixel: what they prove is the cell rule and
//! its refusals. The two `$CS_GAME_DIR` tests are the ones that measure the
//! real archive, so they are `#[ignore]`d for CI and run with
//! `--include-ignored` by the implementing and reviewing agents; a retail test
//! fails loudly, never passes, when its capability is absent.

use cs_app::text::original_font::{
    CELL_COUNT, CHARACTER_COUNT, COLOR_KEY, FIRST_CELL_CHAR, LAST_CELL_CHAR,
    MISSING_GLYPH_SUBSTITUTION, ORIGINAL_BITMAP_FONT_NAMES, RIMAGE_CONTAINER,
    UNUSED_FONT_TGA_REASON, gfont3d_coverage, measure_rimage_bitmap_fonts,
};
use cs_app::text::{
    AuditBlocker, BitmapFontError, GlyphEvidence, LocalizationAudit, LocalizationAuditRequest,
    MediaSource, audit_localization, synthetic_monospace,
};
use cs_content::localization::{FontProvenance, SupportedLocales};
use cs_content::textures::folded_texture_name;
use cs_types::asset_id::SourceSpan;
use cs_types::evidence::ContentHash;

use crate::common::{
    PANEL, RETAIL_FONT_MEDIA, RETAIL_RIMAGE, buttons, grammar, language_map, locale, retail_audit,
    retail_file, retail_game_dir, substitutions,
};

/// A non-zero stored RGB565 word: ink for an authored strip.
const INK: u16 = 0xFFFF;
/// The flags every direct-colour retail texture carries (task F08-B.02).
const OPAQUE: u32 = 0x5;
/// The height of an authored strip, in texels.
const STRIP_HEIGHT: u32 = 8;
/// The width of an authored strip: one leading separator column plus 94 cells
/// of two ink columns and one separator column each.
const STRIP_WIDTH: u32 = 1 + 94 * 3;

// ------------------------------------------------------------ fixtures ---

/// One authored texture of a synthetic package.
struct Tex {
    name: String,
    flags: u32,
    width: u16,
    height: u16,
    palette_count: u16,
    payload: Vec<u8>,
}

impl Tex {
    /// A direct-colour RGB565 texture of `words` stored texels.
    fn direct(name: &str, width: u16, height: u16, words: &[u16]) -> Self {
        let mut payload = Vec::with_capacity(words.len() * 2);
        for word in words {
            payload.extend_from_slice(&word.to_le_bytes());
        }
        Self {
            name: name.to_owned(),
            flags: OPAQUE,
            width,
            height,
            palette_count: 0,
            payload,
        }
    }

    /// A local-palette texture: decoded through the palette, so it has no
    /// stored RGB565 colour key to scan a separator column against.
    fn indexed(name: &str, width: u16, height: u16, indices: &[u8], palette: &[u16]) -> Self {
        let mut payload = indices.to_vec();
        for word in palette {
            payload.extend_from_slice(&word.to_le_bytes());
        }
        Self {
            name: name.to_owned(),
            flags: OPAQUE,
            width,
            height,
            palette_count: u16::try_from(palette.len()).expect("a small palette"),
            payload,
        }
    }

    /// The stored texture: info block, then the payload.
    fn body(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.flags.to_le_bytes());
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&self.height.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&self.palette_count.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&self.payload);
        out
    }
}

/// A complete synthetic package: header, name table, then the textures at
/// exactly the offsets the table declares.
fn package(textures: &[Tex]) -> Vec<u8> {
    let mut out = Vec::new();
    for word in [0u32, 1, 0, textures.len() as u32, 0, 0] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    let bodies: Vec<Vec<u8>> = textures.iter().map(Tex::body).collect();
    let mut offset = u32::try_from(24 + textures.len() * 40).expect("a small package");
    for (texture, body) in textures.iter().zip(&bodies) {
        let mut name = [0u8; 32];
        name[..texture.name.len()].copy_from_slice(texture.name.as_bytes());
        out.extend_from_slice(&name);
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&(-1i32).to_le_bytes());
        offset += u32::try_from(body.len()).expect("a small texture");
    }
    for body in bodies {
        out.extend_from_slice(&body);
    }
    out
}

/// An authored font strip: one leading separator column, then `glyphs` cells
/// of `glyph_width` ink columns separated by single separator columns, padded
/// with separator columns out to `width`.
///
/// Ink fills every row of its column, so a separator column is exactly a
/// column of [`COLOR_KEY`] — the rule the production scan applies.
fn strip(glyphs: usize, width: u32) -> Vec<u16> {
    let needed = 1 + glyphs as u32 * 3;
    assert!(
        needed <= width,
        "the strip needs {needed} columns, got {width}"
    );
    let mut words = vec![COLOR_KEY; (width * STRIP_HEIGHT) as usize];
    let mut x = 1u32;
    for _ in 0..glyphs {
        for column in 0..2u32 {
            for row in 0..STRIP_HEIGHT {
                let at = (row * width + x + column) as usize;
                words[at] = INK;
            }
        }
        x += 3;
    }
    words
}

/// The ten declared fonts stored under their folded spellings: nine full
/// 94-cell strips and, when `short` names one, that font with only `short`
/// cells in it.
fn authored_package(short: Option<(&str, usize)>) -> Vec<u8> {
    let textures: Vec<Tex> = ORIGINAL_BITMAP_FONT_NAMES
        .iter()
        .map(|requested| {
            let stored = folded_texture_name(requested);
            let cells = match short {
                Some((name, cells)) if name == *requested => cells,
                _ => CELL_COUNT,
            };
            Tex::direct(
                &stored,
                u16::try_from(STRIP_WIDTH).expect("a small strip"),
                u16::try_from(STRIP_HEIGHT).expect("a small strip"),
                &strip(cells, STRIP_WIDTH),
            )
        })
        .collect();
    package(&textures)
}

// ------------------------------------------------------------ acceptance ---

/// The cell rule, on authored strips: separator columns are skipped, every
/// cell is one ink run plus its trailing separator column, the mapping is
/// `c - 0x21`, and a font whose scan finds fewer cells keeps the difference
/// in `unresolved` instead of claiming coverage it has no cell for.
#[test]
fn accept_f51_fontcell_cell_rule_maps_authored_cells_to_characters_in_order() {
    let measurement = measure_rimage_bitmap_fonts(&authored_package(None))
        .expect("the authored package is readable and direct colour");

    assert!(
        measurement.missing.is_empty(),
        "every declared font is in the package: {:?}",
        measurement.missing
    );
    assert_eq!(measurement.fonts.len(), ORIGINAL_BITMAP_FONT_NAMES.len());
    assert!(measurement.is_fully_mapped());

    for name in ORIGINAL_BITMAP_FONT_NAMES {
        let font = measurement.font(name).expect(name);
        assert_eq!(font.stored_name, folded_texture_name(name), "{name}");
        assert_eq!((font.width, font.height), (STRIP_WIDTH, STRIP_HEIGHT));
        assert_eq!(font.cells.len(), CELL_COUNT, "{name} stores 94 cells");
        assert_eq!(font.cells[0].ch, FIRST_CELL_CHAR, "{name} cell 0 is '!'");
        assert_eq!(
            font.cells[CELL_COUNT - 1].ch,
            LAST_CELL_CHAR,
            "{name} cell 93 is '~'"
        );
        // One ink run of two columns plus its trailing separator column.
        assert_eq!(font.cells[0].start_col, 1, "{name}");
        assert_eq!(font.cells[0].end_col, 4, "{name}");
        assert_eq!(
            font.cells[1].start_col, 4,
            "{name} the next cell starts at the previous cell's separator"
        );
        assert!(
            font.cells
                .windows(2)
                .all(|pair| pair[0].start_col < pair[1].start_col),
            "{name} cells are in column order and never overlap"
        );
        assert!(font.unresolved.is_empty(), "{name} maps every character");
        assert_eq!(font.stray_cells, 0, "{name} has no unaddressed cell");
        assert_eq!(
            font.coverage.len(),
            CHARACTER_COUNT,
            "{name} covers the space plus the 94 cells"
        );
        assert_eq!(
            font.average_advance, 1,
            "the authored width 283 gives 283 / 95 - 1"
        );
        // The measured substitution: a character with no cell draws '!',
        // which is itself cell 0 and therefore always covered.
        assert!(font.coverage.covers(MISSING_GLYPH_SUBSTITUTION));
        assert!(!font.coverage.covers('é'), "{name} has no cell above 0x7e");
    }

    // The OS font behind `gfont3d` is a *different* coverage: Windows-1252
    // has the Latin-1 and C1 characters a 94-cell strip has no cell for.
    let os = gfont3d_coverage();
    assert!(os.covers(' '), "the space is assigned");
    assert!(os.covers('~'), "printable ASCII is assigned");
    assert!(os.covers('é'), "Latin-1 is assigned");
    assert!(os.covers('€'), "Windows-1252 assigns 0x80 to the euro sign");
    assert!(!os.covers('\u{81}'), "Windows-1252 leaves 0x81 undefined");
    assert!(!os.covers('\u{7f}'), "0x7f is not assigned by Windows-1252");
    assert_eq!(os.len(), 218, "95 ASCII + 27 C1 + 96 Latin-1 assignments");
}

/// A font whose scan finds fewer cells than the rule requires: the missing
/// characters stay uncovered, are reported in `unresolved`, and become a
/// named audit blocker — never a silently claimed glyph.
#[test]
fn accept_f51_fontcell_an_unmapped_character_is_a_named_audit_blocker_not_a_claim() {
    let short = "modern_red_12";
    let measurement = measure_rimage_bitmap_fonts(&authored_package(Some((short, 5))))
        .expect("the authored package is readable and direct colour");
    assert!(measurement.missing.is_empty());

    let font = measurement.font(short).expect(short);
    assert_eq!(font.cells.len(), 5, "only five cells were scanned");
    assert!(!font.is_fully_mapped());
    assert_eq!(
        font.unresolved.len(),
        CELL_COUNT - 5,
        "the characters without a cell are listed"
    );
    assert_eq!(font.unresolved[0], '&', "'!'..'%' have cells, '&' does not");
    for ch in ['!', '"', '#', '$', '%'] {
        assert!(font.coverage.covers(ch), "the measured cells cover {ch:?}");
    }
    for ch in ['&', '5', '~'] {
        assert!(!font.coverage.covers(ch), "no cell, no claim for {ch:?}");
    }
    let report = font.missing_glyphs("6%");
    assert_eq!(report.missing(), ['6'], "'%' has a cell, '6' does not");
    assert_eq!(report.count('6'), 1);
    assert_eq!(report.total(), 1);

    // The audit reports it under its own code, and does not pretend the
    // coverage is simply unknown.
    let audit = audit_of(&measurement_evidence(measurement));
    assert_eq!(
        audit.blockers_with_code("unmeasured_glyphs").count(),
        0,
        "a scanned font is measured, not unmeasured"
    );
    let blocker = audit
        .blockers_with_code("bitmap_font_unmapped")
        .next()
        .expect("the unmapped characters block");
    assert!(
        matches!(
            blocker,
            AuditBlocker::BitmapFontUnmapped { font: name, chars: 89, .. } if name == short
        ),
        "{blocker:?}"
    );
    assert!(blocker.to_string().contains(short), "{blocker}");
    assert!(!audit.is_complete());
}

/// A declared font the package does not hold is reported as missing, never
/// skipped as if it had been measured.
#[test]
fn accept_f51_fontcell_a_declared_font_the_package_lacks_is_reported() {
    let absent = "5PointHUD";
    let textures: Vec<Tex> = ORIGINAL_BITMAP_FONT_NAMES
        .iter()
        .filter(|requested| **requested != absent)
        .map(|requested| {
            Tex::direct(
                &folded_texture_name(requested),
                u16::try_from(STRIP_WIDTH).expect("a small strip"),
                u16::try_from(STRIP_HEIGHT).expect("a small strip"),
                &strip(CELL_COUNT, STRIP_WIDTH),
            )
        })
        .collect();

    let measurement = measure_rimage_bitmap_fonts(&package(&textures))
        .expect("the authored package is readable and direct colour");
    assert_eq!(measurement.missing, vec![absent.to_owned()]);
    assert_eq!(
        measurement.fonts.len(),
        ORIGINAL_BITMAP_FONT_NAMES.len() - 1
    );
    assert!(!measurement.is_fully_mapped());

    let audit = audit_of(&measurement_evidence(measurement));
    assert!(
        matches!(
            audit.blockers_with_code("bitmap_font_missing").next(),
            Some(AuditBlocker::BitmapFontMissing { font, .. }) if font == absent
        ),
        "the missing declared font is named: {:?}",
        audit.blockers
    );
    assert!(!audit.is_complete());
}

/// An image whose texels are not stored RGB565 has no colour key to compare a
/// column against, so the scan refuses it instead of inventing a key.
#[test]
fn accept_f51_fontcell_a_font_image_without_a_stored_colour_key_is_refused() {
    let textures: Vec<Tex> = ORIGINAL_BITMAP_FONT_NAMES
        .iter()
        .map(|requested| {
            Tex::indexed(
                &folded_texture_name(requested),
                4,
                1,
                &[0, 0, 1, 0],
                &[COLOR_KEY, INK],
            )
        })
        .collect();

    let error = measure_rimage_bitmap_fonts(&package(&textures))
        .expect_err("a palette image has no stored RGB565 colour key");
    assert!(
        matches!(
            &error,
            BitmapFontError::NotDirectColor { font, .. } if font == "lucida_console_8"
        ),
        "{error:?}"
    );
    assert!(
        error.to_string().contains("colour key"),
        "the refusal names what is unmeasured: {error}"
    );
}

/// The retail measurement: all ten `fonts.zrd` fonts are in `rimage.zbd`,
/// each stores exactly the 94 cells the rule requires, and the original's
/// `'!'` substitution — not an invisible gap — is what a character above
/// `0x7e` would draw.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f51_fontcell_retail_ten_bitmap_fonts_measure_94_cells_each() {
    let dir = retail_game_dir();
    let bytes = retail_file(&dir, RETAIL_RIMAGE);
    let measurement = measure_rimage_bitmap_fonts(&bytes)
        .expect("the retail archive measures through production");

    assert!(
        measurement.missing.is_empty(),
        "every declared font is stored: {:?}",
        measurement.missing
    );
    assert_eq!(measurement.fonts.len(), ORIGINAL_BITMAP_FONT_NAMES.len());
    assert!(measurement.is_fully_mapped());

    let expected_sizes: [(&str, u32, u32); 10] = [
        ("lucida_console_8", 670, 12),
        ("lucida_console_8b", 761, 12),
        ("modern_white_12", 718, 12),
        ("verdana_red_11", 712, 12),
        ("verdana_white_11", 712, 12),
        ("modern_red_12", 718, 12),
        ("verdana_green_11", 712, 12),
        ("5PointHUD", 463, 6),
        ("5PointHUDBrite", 463, 6),
        ("verdana_Offwhite_11", 712, 12),
    ];
    for (name, width, height) in expected_sizes {
        let font = measurement.font(name).expect(name);
        // The archive stores two of the ten names in lowercase: the request
        // folds, the stored spelling does not.
        assert_eq!(
            font.stored_name,
            folded_texture_name(name),
            "{name} resolves by the measured fold"
        );
        assert_eq!((font.width, font.height), (width, height), "{name}");
        assert_eq!(font.cells.len(), CELL_COUNT, "{name} stores 94 cells");
        assert!(font.unresolved.is_empty(), "{name} has no missing cell");
        assert_eq!(font.stray_cells, 0, "{name} has no unaddressed cell");
        assert_eq!(font.coverage.len(), CHARACTER_COUNT, "{name} coverage");
        assert_eq!(
            font.average_advance,
            i32::try_from(width).expect("a retail width fits") / 95 - 1,
            "{name} average advance"
        );
        for ch in [' ', '!', '0', 'A', 'Z', 'a', 'z', '~'] {
            assert!(font.coverage.covers(ch), "{name} covers {ch:?}");
        }
        for ch in ['\u{7f}', 'é', '€'] {
            assert!(!font.coverage.covers(ch), "{name} has no cell for {ch:?}");
        }
        // The counted, visible missing-glyph path the original draws as '!'
        // (cell 0, which every font covers).
        let missing = font.missing_glyphs("Café café");
        assert_eq!(missing.missing(), ['é']);
        assert_eq!(missing.total(), 2, "'é' occurs twice");
        assert!(font.coverage.covers(MISSING_GLYPH_SUBSTITUTION));
    }
    // Two stored spellings differ from the `fonts.zrd` request, so the fold
    // is doing work rather than passing through.
    assert_ne!(
        measurement.font("5PointHUD").expect("present").stored_name,
        "5PointHUD"
    );
    assert_ne!(
        measurement
            .font("verdana_Offwhite_11")
            .expect("present")
            .stored_name,
        "verdana_Offwhite_11"
    );
}

/// The retail audit verdicts: the two loose TGAs are recorded as unused by
/// the original with their cited evidence, `rimage.zbd` carries the measured
/// per-font coverage, no media is unmeasured any more, and every blocker that
/// remains states its own cause.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f51_fontcell_retail_no_media_is_unmeasured_and_every_blocker_names_its_cause() {
    let dir = retail_game_dir();
    let audit = retail_audit(&dir);

    for spelling in RETAIL_FONT_MEDIA {
        let media = audit.media(spelling).expect("the TGA is audited");
        let reason = media
            .glyphs
            .unused_reason()
            .unwrap_or_else(|| panic!("{spelling} must be classified, not unknown"));
        assert_eq!(
            reason, UNUSED_FONT_TGA_REASON,
            "{spelling} carries the recorded owner-note evidence"
        );
        assert!(media.glyphs.is_measured(), "{spelling} is measured");
        assert!(!media.distributable, "{spelling} never ships");
    }
    assert_eq!(
        audit.blockers_with_code("unmeasured_glyphs").count(),
        0,
        "the F51-D unmeasured-glyph blockers are gone: no audited media is unknown"
    );

    let rimage = audit.media(RETAIL_RIMAGE).expect("rimage is audited");
    let fonts = rimage.glyphs.bitmap_fonts().expect("the scan is measured");
    assert_eq!(
        fonts.fonts.len(),
        ORIGINAL_BITMAP_FONT_NAMES.len(),
        "all ten fonts measured"
    );
    assert!(fonts.is_fully_mapped(), "94 cells in every retail font");

    // What is left is named, and it is never a glyph verdict: the fonts are
    // answered, so the only blockers that may remain are the layout ones the
    // F51-D stage counts.
    assert!(
        !audit.blockers.is_empty(),
        "the F51-D overflow still blocks, so nothing was dropped"
    );
    for blocker in &audit.blockers {
        assert!(
            !blocker.to_string().is_empty(),
            "{blocker:?} states a cause"
        );
        assert!(
            matches!(blocker.code(), "overflow" | "covers_control"),
            "no unmeasured glyph verdict remains: {blocker:?}"
        );
    }
    assert!(!audit.is_complete(), "the overflow keeps the audit honest");
}

// ------------------------------------------------------------- helpers ---

/// The audit of the given media and no string images: the glyph verdicts
/// without the string verdicts.
fn audit_of(media: &[MediaSource<'_>]) -> LocalizationAudit {
    audit_localization(&LocalizationAuditRequest {
        images: &[],
        locales: &SupportedLocales::new([locale("en-us")]).expect("one locale is a valid set"),
        language_map: &language_map(&[(1033, "en-us")]),
        grammar: &grammar(),
        metrics: &synthetic_monospace(16.0),
        panel: PANEL,
        required: &buttons(),
        substitutions: &substitutions(),
        media,
    })
}

/// One measured package as audit media, with an original-private provenance.
fn measurement_evidence(measurement: cs_app::text::RimageBitmapFonts) -> Vec<MediaSource<'static>> {
    let span = SourceSpan::new(
        ContentHash::from_bytes([9; 32]),
        RIMAGE_CONTAINER,
        None,
        0,
        1,
        None,
    )
    .expect("the fixture span is valid");
    vec![MediaSource {
        path: RIMAGE_CONTAINER,
        bytes: &[],
        provenance: FontProvenance::OriginalPrivate {
            span: Box::new(span),
        },
        glyphs: GlyphEvidence::BitmapFonts { fonts: measurement },
    }]
}
