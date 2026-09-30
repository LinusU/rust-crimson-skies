//! Acceptance tests for the Rgb565 texel expansion policy and the
//! coverage-key policy the renderer adapter now uses
//! (task #408, `F17-B-followup-rgb565-expansion`; shared contract
//! `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! Two decisions are under test, and the module under test is
//! `cs_app::render::rgb565` — the production policy the F17-B adapter
//! calls, not a re-implementation of it here.
//!
//! 1. **The expansion is `Designed`, not an F17-D evidence gate.** Bit
//!    replication is the only standard answer to widening a 5/6/5 channel
//!    to eight bits. These tests pin the widening over *every* stored word
//!    and every one of the 32 and 64 channel levels, from tables written
//!    out here rather than from the production formula, so a change to
//!    `expand5`/`expand6`/`Rule` shows up as a wrong channel at a named
//!    level. The deviation between candidate rules is *computed* by
//!    `Rule::max_channel_deviation` and cross-checked against a brute-force
//!    sweep here, because the bound is the thing that makes the residual
//!    unknown safe.
//! 2. **Both coverage keys survive to a GPU alpha channel** with no new
//!    decision, because F08's decoder keeps the plane the key lives in.
//!    `StoredValueKey` is compared on the stored 16-bit word and
//!    `PaletteKey` on the retained index plane, both *before* any
//!    expansion, so a coverage answer provably cannot depend on the rule.
//!    The palette test deliberately gives two palette entries the same
//!    565 word, which is the case a "resolve the color, then compare"
//!    order gets wrong.
//!
//! Every byte in the synthetic fixtures is authored here: no original game
//! data and no `CS_GAME_DIR`. The one test that reads the installation is
//! the census at the end, `#[ignore = "requires CS_GAME_DIR"]`, and it only
//! counts — it asserts no original expansion is reproduced.

use cs_app::render::rgb565::{
    coverage_byte, expand, expand_image, expand_texel, fields, CoverageSource, ExpansionPolicy,
    Rgb565PolicyError, Rule,
};
use cs_formats::io::AllocationBudget;
use cs_formats::texture::{
    decode_base_level, AlphaSource, AlphaTest, ColorSpace, DecodedFormat, DecodedImage,
    DescriptorParts, Extent, ImageDescriptor, Palette, PixelFormat, RowOrder,
};
use cs_types::evidence::ClaimStatus;

/// Provenance label carried by the synthetic fixtures.
const CONTAINER: &str = "synthetic/f17_b_rgb565_expansion";

/// The decided 5-bit widening, written out level by level.
///
/// Authored here as data, not as `(level << 3) | (level >> 2)`: a test that
/// recomputed the production formula would pass whatever the formula
/// became.
const REPLICATION_5: [u8; 32] = [
    0, 8, 16, 24, 33, 41, 49, 57, 66, 74, 82, 90, 99, 107, 115, 123, 132, 140, 148, 156, 165, 173,
    181, 189, 198, 206, 214, 222, 231, 239, 247, 255,
];

/// The decided 6-bit widening, written out level by level.
const REPLICATION_6: [u8; 64] = [
    0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 44, 48, 52, 56, 60, 65, 69, 73, 77, 81, 85, 89, 93, 97,
    101, 105, 109, 113, 117, 121, 125, 130, 134, 138, 142, 146, 150, 154, 158, 162, 166, 170, 174,
    178, 182, 186, 190, 195, 199, 203, 207, 211, 215, 219, 223, 227, 231, 235, 239, 243, 247, 251,
    255,
];

fn descriptor(
    extent: Extent,
    format: PixelFormat,
    palette: Option<Palette>,
    alpha_source: AlphaSource,
) -> ImageDescriptor {
    ImageDescriptor::new(DescriptorParts {
        extent,
        format,
        row_order: RowOrder::TopDown,
        palette,
        mips: Vec::new(),
        alpha_source,
        alpha_test: AlphaTest::Unknown,
        color_space: ColorSpace::Unknown,
    })
    .expect("the fixture descriptor is valid")
}

fn decode(descriptor: &ImageDescriptor, stored: &[u8]) -> DecodedImage {
    let mut budget = AllocationBudget::with_defaults(CONTAINER);
    decode_base_level(CONTAINER, descriptor, stored, &mut budget)
        .expect("the fixture image decodes")
}

/// A 3x2 direct-color 565 image.
///
/// ```text
///        x=0        x=1        x=2
/// y=0    0xFFFF     0x0000     0xF800
/// y=1    0x07E0     0x001F     0x8410
/// ```
///
/// The words are chosen so the top row alone touches both channel
/// extremes: 31/63/31 (white), 0/0/0 (black) and 31/0/0 (red). The key
/// word `0x0000` is therefore a stored texel, not a value invented for the
/// test.
fn words_565() -> [u16; 6] {
    [0xFFFF, 0x0000, 0xF800, 0x07E0, 0x001F, 0x8410]
}

fn word_bytes(words: &[u16]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn direct_565(alpha_source: AlphaSource) -> (ImageDescriptor, DecodedImage) {
    let d = descriptor(
        Extent::new(3, 2),
        PixelFormat::Rgb565,
        None,
        alpha_source,
    );
    let image = decode(&d, &word_bytes(&words_565()));
    (d, image)
}

/// A 565 palette whose entry 0 and entry 4 hold the **same** stored word.
///
/// `0xF800` is full-scale red, so keying entry 0 transparent must leave
/// entry 4 opaque even though the two resolve to the same three channels.
/// This is the case a "resolve the color, then compare against the key
/// color" order gets wrong, and the reason the index plane — not the
/// resolved color — is the source.
const DUPLICATE_PALETTE: [u16; 6] = [0xF800, 0x0000, 0x07E0, 0x001F, 0xF800, 0x8410];

/// An indexed 3x2 image using `DUPLICATE_PALETTE`.
///
/// ```text
///        x=0        x=1        x=2
/// y=0    5          0          1
/// y=1    4          2          2
/// ```
///
/// Entries 0 and 4 (identical words) both appear, at `(2, 0)` and `(0, 1)`,
/// and entry 3 is a valid palette entry the image never uses.
fn indexed_565(alpha_source: AlphaSource) -> (ImageDescriptor, DecodedImage) {
    let d = descriptor(
        Extent::new(3, 2),
        PixelFormat::Indexed8,
        Some(Palette::Rgb565(DUPLICATE_PALETTE.to_vec())),
        alpha_source,
    );
    let image = decode(&d, &[5, 0, 1, 4, 2, 2]);
    (d, image)
}

// --- Decision 1: the expansion --------------------------------------------

#[test]
fn accept_f17_b_rgb565_every_channel_level_widens_to_the_decided_value() {
    // 32 five-bit and 64 six-bit levels, against tables authored here.
    for (level, expected) in REPLICATION_5.iter().enumerate() {
        assert_eq!(
            Rule::Replication.expand5(level as u8),
            *expected,
            "5-bit level {level}"
        );
    }
    for (level, expected) in REPLICATION_6.iter().enumerate() {
        assert_eq!(
            Rule::Replication.expand6(level as u8),
            *expected,
            "6-bit level {level}"
        );
    }
    // The decided free functions and the decided policy must agree, and the
    // policy is the thing an adapter actually calls.
    for level in 0u8..32 {
        assert_eq!(cs_app::render::rgb565::expand5(level), REPLICATION_5[level as usize]);
    }
    for level in 0u8..64 {
        assert_eq!(cs_app::render::rgb565::expand6(level), REPLICATION_6[level as usize]);
    }
    let policy = ExpansionPolicy::DECIDED;
    assert_eq!(policy.rule(), Rule::Replication);
    for level in 0u8..32 {
        assert_eq!(policy.expand((u16::from(level) << 11) | 0x0000)[0], REPLICATION_5[level as usize]);
    }
    for level in 0u8..64 {
        assert_eq!(policy.expand((u16::from(level) << 5) | 0x0000)[1], REPLICATION_6[level as usize]);
    }
}

#[test]
fn accept_f17_b_rgb565_every_one_of_the_65_536_stored_words_expands() {
    // The whole input domain, not a sample: each word must widen to the
    // three table entries for its own fields, and the fields must be the
    // established 5/6/5 layout.
    for word in 0u16..=u16::MAX {
        let (red, green, blue) = fields(word);
        assert_eq!(usize::from(red) < 32 && usize::from(green) < 64, true, "{word:04X}");
        assert_eq!(expand(word)[0], REPLICATION_5[usize::from(red)], "red of {word:04X}");
        assert_eq!(expand(word)[1], REPLICATION_6[usize::from(green)], "green of {word:04X}");
        assert_eq!(expand(word)[2], REPLICATION_5[usize::from(blue)], "blue of {word:04X}");
    }
    // Endpoints exact, and the widening is monotone in the level, so no
    // stored word can invert order.
    assert_eq!(expand(0x0000), [0, 0, 0], "black stays black");
    assert_eq!(expand(u16::MAX), [255, 255, 255], "white reaches full scale");
    assert_eq!(expand(0xF800), [255, 0, 0]);
    assert_eq!(expand(0x07E0), [0, 255, 0]);
    assert_eq!(expand(0x001F), [0, 0, 255]);
    for level in 1..32usize {
        assert!(
            REPLICATION_5[level] > REPLICATION_5[level - 1],
            "5-bit level {level} is not above its predecessor"
        );
    }
    for level in 1..64usize {
        assert!(
            REPLICATION_6[level] > REPLICATION_6[level - 1],
            "6-bit level {level} is not above its predecessor"
        );
    }
}

#[test]
fn accept_f17_b_rgb565_an_image_expands_texel_by_texel_through_the_policy() {
    let (_, image) = direct_565(AlphaSource::Opaque);
    assert_eq!(image.format(), DecodedFormat::Rgb565);

    let whole = expand_image(&image, CoverageSource::Opaque, &ExpansionPolicy::DECIDED)
        .expect("a decided 565 image expands");
    let mut expected = Vec::new();
    for (x, y, word) in [
        (0u32, 0u32, 0xFFFFu16),
        (1, 0, 0x0000),
        (2, 0, 0xF800),
        (0, 1, 0x07E0),
        (1, 1, 0x001F),
        (2, 1, 0x8410),
    ] {
        let [r, g, b] = expand(word);
        expected.push([r, g, b, u8::MAX]);
        assert_eq!(
            expand_texel(&image, CoverageSource::Opaque, &ExpansionPolicy::DECIDED, x, y),
            Ok([r, g, b, u8::MAX]),
            "texel ({x}, {y})"
        );
    }
    assert_eq!(whole, expected, "row-major from the top-left, as the adapter writes them");

    // An 8-bit image has no 16-bit word to widen: that is a refusal, not
    // a silent three-channel pass-through.
    let rgb8 = decode(
        &descriptor(Extent::new(2, 1), PixelFormat::Rgb8, None, AlphaSource::Opaque),
        &[1, 2, 3, 4, 5, 6],
    );
    assert_eq!(
        expand_texel(&rgb8, CoverageSource::Opaque, &ExpansionPolicy::DECIDED, 0, 0),
        Err(Rgb565PolicyError::NotRgb565 {
            format: DecodedFormat::Rgb8
        })
    );
}

#[test]
fn accept_f17_b_rgb565_the_decision_is_designed_and_a_refusal_is_not_an_expansion() {
    let decided = ExpansionPolicy::DECIDED;
    assert_eq!(decided.status(), ClaimStatus::Designed);
    assert_eq!(decided.status().label(), "designed");
    assert_ne!(
        decided.status(),
        ClaimStatus::VerifiedOriginal,
        "no original expansion was measured; this must never claim otherwise"
    );
    assert_ne!(decided.status(), ClaimStatus::Documented);
    // A refusal is not an expansion: `Unknown` and `Contradicted` cannot
    // become a policy a caller may widen texels with.
    for status in [ClaimStatus::Unknown, ClaimStatus::Contradicted] {
        assert_eq!(
            ExpansionPolicy::new(Rule::Replication, status).err(),
            Some(cs_app::render::rgb565::ExpansionPolicyError(status)),
            "status {status} is a refusal"
        );
    }
    // Every enumerated rule is reachable with an asserting status, or the
    // deviation bound would be a bound over rules nothing can build.
    for rule in Rule::ALL {
        let policy = ExpansionPolicy::new(rule, ClaimStatus::Inferred)
            .expect("an inferred rule is a usable policy");
        assert_eq!(policy.rule(), rule);
    }
}

#[test]
fn accept_f17_b_rgb565_the_deviation_between_rules_is_computed_and_bounded() {
    // The bound is a number over the whole domain, not a claim in prose.
    // Brute-force it here first, then require the production function to
    // produce the same answer for every pair.
    for a in Rule::ALL {
        for b in Rule::ALL {
            let mut brute = 0u8;
            for level in 0u8..32 {
                brute = brute.max(a.expand5(level).abs_diff(b.expand5(level)));
            }
            for level in 0u8..64 {
                brute = brute.max(a.expand6(level).abs_diff(b.expand6(level)));
            }
            assert_eq!(a.max_channel_deviation(b), brute, "{a} vs {b}");
        }
    }
    // The two rules that can both represent a full-scale channel are within
    // 1/255 of each other, so the residual unknown cannot move a channel by
    // a visible amount.
    assert_eq!(
        Rule::Replication.max_channel_deviation(Rule::FixedPointScale),
        1,
        "replication and the fixed-point scale agree to 1/255"
    );
    // Truncation is the outlier, and it is bounded too: it cannot represent
    // white at all, which is the edge that rules it out.
    assert_eq!(Rule::Replication.max_channel_deviation(Rule::Truncation), 7);
    assert_eq!(Rule::FixedPointScale.max_channel_deviation(Rule::Truncation), 7);
    assert!(Rule::Replication.reaches_white());
    assert!(Rule::FixedPointScale.reaches_white());
    assert!(!Rule::Truncation.reaches_white());
    assert_eq!(Rule::Truncation.expand(u16::MAX), [248, 252, 248]);
    assert_eq!(Rule::ALL.len(), 3, "the enumerated set is closed, so a bound cannot miss one");
}

// --- Decision 2: the coverage keys ----------------------------------------

#[test]
fn accept_f17_b_rgb565_every_alpha_source_projects_to_the_plane_its_key_lives_in() {
    assert_eq!(
        CoverageSource::from_source(AlphaSource::Opaque),
        Ok(CoverageSource::Opaque)
    );
    assert_eq!(
        CoverageSource::from_source(AlphaSource::Channel),
        Ok(CoverageSource::Channel)
    );
    assert_eq!(
        CoverageSource::from_source(AlphaSource::Plane),
        Ok(CoverageSource::StoredPlane)
    );
    assert_eq!(
        CoverageSource::from_source(AlphaSource::StoredValueKey { value: 0x0000 }),
        Ok(CoverageSource::StoredWord { key: 0x0000 })
    );
    assert_eq!(
        CoverageSource::from_source(AlphaSource::PaletteKey { index: 7 }),
        Ok(CoverageSource::PaletteIndex { key: 7 })
    );
    // The one variant that names no plane stays a refusal, and it is a
    // refusal with a reason code rather than a default of "opaque".
    assert_eq!(
        CoverageSource::from_source(AlphaSource::Unknown),
        Err(Rgb565PolicyError::CoverageSourceUnknown)
    );
    assert_eq!(
        Rgb565PolicyError::CoverageSourceUnknown.code(),
        "coverage_source_unknown"
    );
    assert!(!CoverageSource::Opaque.carries_coverage());
    for carries in [
        CoverageSource::Channel,
        CoverageSource::StoredPlane,
        CoverageSource::StoredWord { key: 0 },
        CoverageSource::PaletteIndex { key: 0 },
    ] {
        assert!(carries.carries_coverage(), "{carries}");
    }
}

#[test]
fn accept_f17_b_rgb565_a_stored_word_key_marks_exactly_the_texels_storing_the_key() {
    // The retail key: a direct 565 word of 0x0000 is transparent.
    let (_, image) = direct_565(AlphaSource::StoredValueKey { value: 0x0000 });
    let source = CoverageSource::from_source(image.alpha_source())
        .expect("a stored word key names a plane");
    assert_eq!(source, CoverageSource::StoredWord { key: 0x0000 });

    // Only (1, 0) stores 0x0000; the other five texels are opaque.
    assert_eq!(coverage_byte(&image, source, 1, 0), Ok(0), "the keyed texel");
    for (x, y) in [(0u32, 0u32), (2, 0), (0, 1), (1, 1), (2, 1)] {
        assert_eq!(
            coverage_byte(&image, source, x, y),
            Ok(u8::MAX),
            "texel ({x}, {y}) does not store the key"
        );
    }
    // A key no texel stores makes the whole image opaque, which is a
    // different answer from the same source on the same image.
    let absent = CoverageSource::StoredWord { key: 0x1234 };
    for (x, y) in [(0u32, 0u32), (1, 0), (2, 1)] {
        assert_eq!(coverage_byte(&image, absent, x, y), Ok(u8::MAX));
    }
    // F08 non-negotiable #1: the key is metadata, never baked. The texel
    // still stores its own word after the coverage byte is composed.
    assert_eq!(image.texel565(1, 0), Some(0x0000), "the transparent texel keeps its word");
    assert_eq!(image.texel(1, 0), Some(&[0x00, 0x00][..]));
    assert_eq!(expand_texel(&image, source, &ExpansionPolicy::DECIDED, 1, 0), Ok([0, 0, 0, 0]));
    // and the composed byte is visible next to it.
    assert_eq!(
        expand_texel(&image, source, &ExpansionPolicy::DECIDED, 0, 0),
        Ok([255, 255, 255, 255]),
        "the white texel is opaque"
    );
}

#[test]
fn accept_f17_b_rgb565_a_palette_index_key_survives_a_duplicate_palette_entry() {
    // Key palette index 0. Entry 4 holds the same 565 word, so a resolve
    // first order would clear both or neither.
    let (_, image) = indexed_565(AlphaSource::PaletteKey { index: 0 });
    assert_eq!(image.format(), DecodedFormat::Rgb565, "a 565 palette decodes to 565 words");
    let source = CoverageSource::from_source(image.alpha_source())
        .expect("a palette key names a plane");
    assert_eq!(source, CoverageSource::PaletteIndex { key: 0 });

    // (1, 0) stores index 0 -> transparent.
    assert_eq!(image.index(1, 0), Some(0));
    assert_eq!(coverage_byte(&image, source, 1, 0), Ok(0));
    // (0, 1) stores index 4, whose word is identical -> still opaque.
    assert_eq!(image.index(0, 1), Some(4));
    assert_eq!(image.texel565(0, 1), image.texel565(1, 0), "the two entries share a word");
    assert_eq!(coverage_byte(&image, source, 0, 1), Ok(u8::MAX), "a shared color is not a shared key");
    for (x, y) in [(0u32, 0u32), (2, 0), (1, 1), (2, 1)] {
        assert_eq!(coverage_byte(&image, source, x, y), Ok(u8::MAX), "texel ({x}, {y})");
    }
    // Keying the other half of the duplicate pair swaps exactly those two.
    let other = CoverageSource::PaletteIndex { key: 4 };
    assert_eq!(coverage_byte(&image, other, 0, 1), Ok(0));
    assert_eq!(coverage_byte(&image, other, 1, 0), Ok(u8::MAX));
    // A valid palette entry the image never uses leaves it fully opaque.
    let unused = CoverageSource::PaletteIndex { key: 3 };
    assert_eq!(coverage_byte(&image, unused, 1, 0), Ok(u8::MAX));
    assert_eq!(coverage_byte(&image, unused, 0, 1), Ok(u8::MAX));
    // The keyed texel keeps its stored word, and the composition reports it.
    assert_eq!(expand_texel(&image, source, &ExpansionPolicy::DECIDED, 1, 0), Ok([255, 0, 0, 0]));
    assert_eq!(image.index(1, 0), Some(0), "the index plane survives the composition");
}

#[test]
fn accept_f17_b_rgb565_a_coverage_key_does_not_depend_on_the_expansion_rule() {
    // The property that makes deciding the expansion safe for the keyed
    // rows: coverage is compared on the *stored* value, so no rule can
    // change an alpha. Held for every rule and every keyed source.
    let keyed = [
        ("stored word key", direct_565(AlphaSource::StoredValueKey { value: 0x0000 })),
        ("palette index key", indexed_565(AlphaSource::PaletteKey { index: 0 })),
    ];
    for (label, (descriptor, image)) in keyed {
        let source = CoverageSource::from_source(descriptor.alpha_source())
            .expect("a key names a plane");
        let extent = image.extent();
        for rule in Rule::ALL {
            let policy = ExpansionPolicy::new(rule, ClaimStatus::Designed)
                .expect("designed is an asserting status");
            for y in 0..extent.height {
                for x in 0..extent.width {
                    let texel = expand_texel(&image, source, &policy, x, y)
                        .expect("a keyed 565 texel resolves");
                    assert_eq!(
                        texel[3],
                        coverage_byte(&image, source, x, y).expect("the same texel"),
                        "{label}: the alpha at ({x}, {y}) moved with the rule"
                    );
                    // The color half is the only thing the rule may change,
                    // and it must change for a rule that is not replication.
                    let word = image.texel565(x, y).expect("a stored word");
                    assert_eq!([texel[0], texel[1], texel[2]], rule.expand(word), "{label}");
                }
            }
        }
    }
    // And the halves really are separable: on the truncation rule the
    // colors of a keyed image differ from the decided ones while no single
    // alpha moves.
    let (descriptor, image) = direct_565(AlphaSource::StoredValueKey { value: 0x0000 });
    let source = CoverageSource::from_source(descriptor.alpha_source()).unwrap_or(CoverageSource::Opaque);
    let truncating = ExpansionPolicy::new(Rule::Truncation, ClaimStatus::Designed)
        .expect("designed is an asserting status");
    let decided = expand_texel(&image, source, &ExpansionPolicy::DECIDED, 0, 0).unwrap_or_default();
    let truncated = expand_texel(&image, source, &truncating, 0, 0).unwrap_or_default();
    assert_ne!(decided[..3], truncated[..3], "truncation really does change the color");
    assert_eq!(decided[3], truncated[3], "and never the coverage");
}

#[test]
fn accept_f17_b_rgb565_an_absent_key_plane_and_an_out_of_range_texel_stay_refusals() {
    // A palette key on an image that stores no index plane: the source
    // names a plane the image does not have.
    let direct = direct_565(AlphaSource::Opaque).1;
    assert_eq!(direct.indices(), None);
    assert_eq!(
        coverage_byte(&direct, CoverageSource::PaletteIndex { key: 1 }, 0, 0),
        Err(Rgb565PolicyError::KeyPlaneAbsent {
            source: CoverageSource::PaletteIndex { key: 1 },
            format: DecodedFormat::Rgb565,
        })
    );
    // A stored word key on an 8-bit image: the word is not stored.
    let rgb8 = decode(
        &descriptor(Extent::new(2, 1), PixelFormat::Rgb8, None, AlphaSource::Opaque),
        &[1, 2, 3, 4, 5, 6],
    );
    assert_eq!(
        coverage_byte(&rgb8, CoverageSource::StoredWord { key: 0 }, 0, 0),
        Err(Rgb565PolicyError::KeyPlaneAbsent {
            source: CoverageSource::StoredWord { key: 0 },
            format: DecodedFormat::Rgb8,
        })
    );
    // A coverage plane requested from an image that stores none.
    assert_eq!(
        coverage_byte(&direct, CoverageSource::StoredPlane, 0, 0),
        Err(Rgb565PolicyError::KeyPlaneAbsent {
            source: CoverageSource::StoredPlane,
            format: DecodedFormat::Rgb565,
        })
    );
    // A coverage channel requested from a three-channel image.
    assert_eq!(
        coverage_byte(&rgb8, CoverageSource::Channel, 0, 0),
        Err(Rgb565PolicyError::KeyPlaneAbsent {
            source: CoverageSource::Channel,
            format: DecodedFormat::Rgb8,
        })
    );
    // Out of bounds, on every source, before any plane is read.
    for source in [
        CoverageSource::Opaque,
        CoverageSource::Channel,
        CoverageSource::StoredPlane,
        CoverageSource::StoredWord { key: 0 },
        CoverageSource::PaletteIndex { key: 0 },
    ] {
        assert_eq!(
            coverage_byte(&direct, source, 3, 0),
            Err(Rgb565PolicyError::TexelOutOfBounds { x: 3, y: 0 }),
            "{source}"
        );
        assert_eq!(
            coverage_byte(&direct, source, 0, 2),
            Err(Rgb565PolicyError::TexelOutOfBounds { x: 0, y: 2 }),
            "{source}"
        );
    }
    // Distinct, stable reason codes: a consumer groups by these.
    let codes = [
        Rgb565PolicyError::CoverageSourceUnknown.code(),
        Rgb565PolicyError::KeyPlaneAbsent {
            source: CoverageSource::Opaque,
            format: DecodedFormat::Rgb565,
        }
        .code(),
        Rgb565PolicyError::TexelOutOfBounds { x: 0, y: 0 }.code(),
        Rgb565PolicyError::NotRgb565 {
            format: DecodedFormat::Rgb8,
        }
        .code(),
    ];
    let unique: std::collections::BTreeSet<&str> = codes.iter().copied().collect();
    assert_eq!(unique.len(), codes.len(), "every refusal has its own code: {codes:?}");
}

// --- The retail census: what the decision covers ---------------------------

/// How many of the installation's textures each decision reaches.
///
/// Counts only. It asserts that the 565 rows are the rows the decision
/// covers and that the refusals are the rows the findings document names;
/// it does **not** assert that the decided expansion reproduces the
/// original renderer, which is unmeasured and belongs to F17-D.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f17_b_rgb565_retail_565_rows_and_coverage_keys_are_counted() {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    fn game_dir() -> PathBuf {
        let dir = std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR must point at the original installation for this test");
        let dir = PathBuf::from(dir);
        assert!(dir.is_dir(), "CS_GAME_DIR {} is not a directory", dir.display());
        dir
    }

    fn texture_archives(dir: &Path, out: &mut Vec<PathBuf>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap_or_else(|error| panic!("reading {}: {error}", dir.display()))
            .map(|entry| entry.expect("directory entry").path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                texture_archives(&path, out);
                continue;
            }
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if name.ends_with(".zbd")
                && (name.starts_with("texture")
                    || name.starts_with("rtexture")
                    || name == "rimage.zbd")
            {
                out.push(path);
            }
        }
    }

    let dir = game_dir();
    let mut archives = Vec::new();
    texture_archives(&dir.join("ZBD"), &mut archives);
    assert_eq!(archives.len(), 49, "texture archives under ZBD/");

    // (is a 565 palette, alpha source) -> how many textures.
    let mut rows: BTreeMap<(bool, &'static str), usize> = BTreeMap::new();
    let mut words: BTreeMap<u16, u64> = BTreeMap::new();
    let mut words_in_palette: BTreeMap<u16, u64> = BTreeMap::new();
    let mut duplicate_palette_entries = 0usize;
    let mut shared_index_max = 0usize;
    let mut palettes = 0usize;
    let mut packed = 0usize;
    let mut saturated_texels = 0u64;
    let mut texels = 0u64;
    let mut keyed_texels = 0u64;
    let mut keyed_rows = 0u64;
    let mut scale_disagreeing = 0u64;
    for path in &archives {
        let container = path.strip_prefix(&dir).unwrap_or(path).display().to_string();
        let bytes = std::fs::read(path).unwrap_or_else(|error| panic!("{container}: {error}"));
        let mut budget = AllocationBudget::with_defaults(&container);
        let package = cs_formats::texture::read_zbd_textures(&container, &bytes, &mut budget)
            .unwrap_or_else(|error| panic!("{container}: {error}"));
        for texture in package.textures() {
            let descriptor = texture.descriptor();
            let label = |alpha: &AlphaSource| -> &'static str {
                match alpha {
                    AlphaSource::Opaque => "opaque",
                    AlphaSource::Plane => "plane",
                    AlphaSource::StoredValueKey { .. } => "stored_word_key",
                    AlphaSource::PaletteKey { .. } => "palette_index_key",
                    AlphaSource::Channel => "channel",
                    AlphaSource::Unknown => "unknown",
                }
            };
            let source = descriptor.alpha_source();
            let indexed = descriptor.palette().is_some();
            *rows.entry((indexed, label(&source))).or_default() += 1;
            assert!(
                !matches!(source, AlphaSource::PaletteKey { .. }),
                "{}: the installation stores no palette-keyed row",
                texture.label()
            );

            if let Some(Palette::Rgb565(entries)) = descriptor.palette() {
                assert!(
                    entries.iter().all(|word| *word <= u16::MAX),
                    "{}: every palette entry is a stored word",
                    texture.label()
                );
                palettes += 1;
                let unique: std::collections::BTreeSet<u16> = entries.iter().copied().collect();
                if unique.len() != entries.len() {
                    duplicate_palette_entries += 1;
                }
                let most_shared = unique
                    .iter()
                    .map(|word| entries.iter().filter(|stored| *stored == word).count())
                    .max()
                    .unwrap_or(0);
                shared_index_max = shared_index_max.max(most_shared);
                for word in entries {
                    *words_in_palette.entry(*word).or_default() += 1;
                }
            }
            if !cs_app::render::rgb565::stores_texel_words(
                // The decoded layout is what a 565 row resolves to; the
                // reader decides it and this test only checks the claim.
                match (descriptor.format(), descriptor.palette()) {
                    (PixelFormat::Rgb565, _) | (PixelFormat::Indexed8, Some(Palette::Rgb565(_))) => {
                        DecodedFormat::Rgb565
                    }
                    _ => DecodedFormat::Rgb8,
                },
            ) {
                continue;
            }
            packed += 1;
            let image = texture
                .decode(&mut AllocationBudget::with_defaults(texture.label()))
                .unwrap_or_else(|error| panic!("{}: {error}", texture.label()));
            let key = match source {
                AlphaSource::StoredValueKey { value } => Some(value),
                _ => None,
            };
            if key.is_some() {
                keyed_rows += 1;
            }
            let extent = image.extent();
            for y in 0..extent.height {
                for x in 0..extent.width {
                    let word = image
                        .texel565(x, y)
                        .unwrap_or_else(|| panic!("{}: a 565 row stores a word", texture.label()));
                    *words.entry(word).or_default() += 1;
                    texels += 1;
                    let (red, green, blue) = fields(word);
                    if red == 31 || green == 63 || blue == 31 {
                        saturated_texels += 1;
                    }
                    if key == Some(word) {
                        keyed_texels += 1;
                    }
                    // The two rules that can both represent full scale
                    // differ on a handful of levels; count the texels that
                    // actually sit on one.
                    if Rule::Replication.expand(word) != Rule::FixedPointScale.expand(word) {
                        scale_disagreeing += 1;
                    }
                }
            }
        }
    }

    // Every ZBD texture is 565-backed, directly or through a 565 palette:
    // this is why the expansion refusal covered the whole texture set.
    assert_eq!(packed, 37_004, "every package texture stores 16-bit texel words");
    let total: usize = rows.values().sum();
    assert_eq!(total, 37_004, "every package texture is counted in exactly one row");
    assert_eq!(
        rows,
        BTreeMap::from([
            ((false, "opaque"), 15_399),
            ((false, "plane"), 15_343),
            ((false, "stored_word_key"), 137),
            ((true, "opaque"), 3_089),
            ((true, "plane"), 3_014),
            ((true, "unknown"), 22),
        ]),
        "the retail row breakdown, so the census is a measured number and not a description"
    );
    assert_eq!(palettes, 6_125, "indexed 565 rows carry a local palette");
    assert!(palettes > 0);
    // The palette-index key is the only coverage key the installation
    // never uses, so its refusal costs no retail content today.
    assert_eq!(rows.get(&(true, "palette_index_key")), None);
    assert_eq!(rows.get(&(false, "palette_index_key")), None);

    assert!(texels > 0, "the installation stores texels");
    assert!(
        saturated_texels > 0,
        "the installation stores saturated 565 texels, so an expansion that cannot \
         represent full scale did not produce them"
    );
    // The residual unknown is bounded, and the bound is computed over the
    // whole domain rather than described.
    let replication = ExpansionPolicy::DECIDED;
    for rule in [Rule::FixedPointScale, Rule::Truncation] {
        let bound = rule.max_channel_deviation(replication.rule());
        assert!(bound <= 7, "{rule} is within 7/255 of the decided rule");
    }
    assert_eq!(Rule::Replication.max_channel_deviation(Rule::FixedPointScale), 1);
    assert_eq!(replication.expand(u16::MAX), [255, 255, 255]);
    assert_eq!(Rule::Truncation.expand(u16::MAX), [248, 252, 248]);

    // How many texels sit at a level where the candidate rules disagree,
    // and how many stored words the installation actually uses. Recorded so
    // the residual unknown is quantified rather than asserted.
    let disjoint = usize::from(
        Rule::Replication.max_channel_deviation(Rule::Truncation) > 0,
    );
    assert_eq!(disjoint, 1, "the rules do differ somewhere in the domain");
    let mut disagreeing = 0u64;
    for (word, count) in &words {
        if expand(*word) != Rule::Truncation.expand(*word) {
            disagreeing += count;
        }
    }
    let mut palette_disagreeing = 0u64;
    for (word, count) in &words_in_palette {
        if expand(*word) != Rule::Truncation.expand(*word) {
            palette_disagreeing += count;
        }
    }
    eprintln!(
        "retail 565: {texels} texels in {} distinct words, {saturated_texels} saturated; \
         {disagreeing} texels differ between replication and truncation; \
         {scale_disagreeing} differ between replication and the fixed-point scale; \
         {keyed_texels} texels are the stored key across {keyed_rows} keyed rows; \
         {duplicate_palette_entries} of {palettes} palettes hold a duplicate word \
         (up to {shared_index_max} indices share one word), \
         {palette_disagreeing} palette entries differ between the two rules",
        words.len()
    );
    assert!(duplicate_palette_entries > 0, "at least one retail palette holds a duplicate word");
    assert!(
        shared_index_max >= 2,
        "so a resolve-first order provably loses a key in real content"
    );
    assert!(
        keyed_rows > 0,
        "the stored-word key is a real retail coverage source"
    );
    assert!(
        keyed_texels > 0,
        "and it actually covers texels: the key is a live coverage source, not a formality"
    );
    assert!(
        scale_disagreeing < texels,
        "the two full-scale-capable rules do not disagree everywhere, so the plausible \
         residual is small even though it is not zero"
    );
    assert!(
        !words_in_palette.is_empty(),
        "the indexed rows contribute their palette words"
    );
}
