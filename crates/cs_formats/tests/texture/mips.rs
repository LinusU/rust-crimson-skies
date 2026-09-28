//! Acceptance stage F08-B.01: decoding every declared mip level
//! (`specs/F08-texture-archives-and-conventional-image-decoding.md`, section
//! `### F08-B`, AC02 "non-square mip chains" and non-negotiable #2
//! "validate every mip level and palette index").
//!
//! Every byte in this file is authored here: newly authored synthetic
//! content, no original game data, no `CS_GAME_DIR` access. The chains are
//! non-square on purpose and their extents are stated per level, never
//! derived from a halving rule: the reduction rule is a variant fact the
//! decoder does not assume.

use cs_formats::ParseErrorKind;
use cs_formats::io::AllocationBudget;
use cs_formats::texture::{
    AlphaSource, AlphaTest, ColorSpace, DecodedFormat, DecodedImage, DecodedLevels,
    DescriptorParts, Extent, ImageDescriptor, PaletteEntry, PixelFormat, RowOrder, TextureError,
    decode_base_level, decode_levels,
};

const CONTAINER: &str = "synthetic/f08_b_01_mip_chains";

const RED: [u8; 3] = [0xFF, 0x00, 0x00];
const GREEN: [u8; 3] = [0x00, 0xFF, 0x00];
const BLUE: [u8; 3] = [0x00, 0x00, 0xFF];
const YELLOW: [u8; 3] = [0xFF, 0xFF, 0x00];
const CYAN: [u8; 3] = [0x00, 0xFF, 0xFF];
const MAGENTA: [u8; 3] = [0xFF, 0x00, 0xFF];
const ORANGE: [u8; 3] = [0xFF, 0x80, 0x00];
const PURPLE: [u8; 3] = [0x80, 0x00, 0xFF];
const GREY: [u8; 3] = [0x80, 0x80, 0x80];
const WHITE: [u8; 3] = [0xFF, 0xFF, 0xFF];

fn parts(
    extent: Extent,
    mips: &[Extent],
    format: PixelFormat,
    row_order: RowOrder,
    palette: Option<Vec<PaletteEntry>>,
) -> DescriptorParts {
    DescriptorParts {
        extent,
        format,
        row_order,
        palette,
        mips: mips.to_vec(),
        alpha_source: AlphaSource::Unknown,
        alpha_test: AlphaTest::Unknown,
        color_space: ColorSpace::Unknown,
    }
}

fn decode(descriptor: &ImageDescriptor, levels: &[&[u8]]) -> Result<DecodedLevels, TextureError> {
    let mut budget = AllocationBudget::with_defaults(CONTAINER);
    decode_levels(CONTAINER, descriptor, levels, &mut budget)
}

fn assert_texels(image: &DecodedImage, level: usize, expected: &[(u32, u32, [u8; 3])]) {
    for &(x, y, color) in expected {
        assert_eq!(
            image.texel(x, y),
            Some(&color[..]),
            "level {level}, texel ({x}, {y}) from the top-left"
        );
    }
}

// --- Chain A: RGB 3x2 -> 2x1 -> 1x1 ----------------------------------------

const CHAIN_A: [Extent; 2] = [Extent::new(2, 1), Extent::new(1, 1)];

fn chain_a(row_order: RowOrder) -> ImageDescriptor {
    ImageDescriptor::new(parts(
        Extent::new(3, 2),
        &CHAIN_A,
        PixelFormat::Rgb8,
        row_order,
        None,
    ))
    .expect("chain A descriptor is valid")
}

/// Chain A stored top row first.
fn chain_a_top_down() -> [Vec<u8>; 3] {
    [
        [RED, GREEN, BLUE, YELLOW, CYAN, MAGENTA].concat(),
        [ORANGE, PURPLE].concat(),
        GREY.to_vec(),
    ]
}

fn assert_chain_a(levels: &DecodedLevels) {
    assert_eq!(levels.levels().len(), 3);
    assert_eq!(levels.mips().len(), 2);
    let extents: Vec<Extent> = levels.levels().iter().map(DecodedImage::extent).collect();
    assert_eq!(
        extents,
        [Extent::new(3, 2), Extent::new(2, 1), Extent::new(1, 1)]
    );
    assert_texels(
        levels.base(),
        0,
        &[
            (0, 0, RED),
            (1, 0, GREEN),
            (2, 0, BLUE),
            (0, 1, YELLOW),
            (1, 1, CYAN),
            (2, 1, MAGENTA),
        ],
    );
    let mip1 = levels.level(1).expect("mip level 1");
    assert_texels(mip1, 1, &[(0, 0, ORANGE), (1, 0, PURPLE)]);
    assert_eq!(mip1.texel(0, 1), None, "mip level 1 is one row high");
    assert_eq!(mip1.texel(2, 0), None, "mip level 1 is two columns wide");
    let mip2 = levels.level(2).expect("mip level 2");
    assert_texels(mip2, 2, &[(0, 0, GREY)]);
    assert_eq!(mip2.texel(1, 0), None);
    assert!(
        levels.level(3).is_none(),
        "no level beyond the declared chain"
    );
}

// --- Chain B: indexed 4x1 -> 2x1 -> 1x1 ------------------------------------

const CHAIN_B: [Extent; 2] = [Extent::new(2, 1), Extent::new(1, 1)];

/// Palette order unrelated to the image order.
fn palette_b() -> Vec<PaletteEntry> {
    [GREEN, MAGENTA, PURPLE, RED, WHITE, ORANGE, BLUE, GREY]
        .iter()
        .map(|&[r, g, b]| PaletteEntry::new(r, g, b))
        .collect()
}

// --- Chain C: 3x3 -> 3x2 -> 1x2 -> 1x1, for row order ----------------------

const CHAIN_C: [Extent; 3] = [Extent::new(3, 2), Extent::new(1, 2), Extent::new(1, 1)];

/// The intended color of texel `(x, y)` (from the top-left) of level
/// `level` in chain C: unique across the whole chain.
const fn chain_c_color(level: u8, x: u8, y: u8) -> [u8; 3] {
    [(level << 4) | x, y, 0xC0]
}

/// Stores level `level` of chain C (`width` x `height`) with its rows in
/// `rows` order (top-down: 0, 1, ...; bottom-up: ..., 1, 0).
fn chain_c_level(level: u8, width: u8, rows: impl Iterator<Item = u8>) -> Vec<u8> {
    rows.flat_map(|y| (0..width).flat_map(move |x| chain_c_color(level, x, y)))
        .collect()
}

fn chain_c_stored(row_order: RowOrder) -> [Vec<u8>; 4] {
    let level = |level: u8, width: u8, height: u8| match row_order {
        RowOrder::TopDown => chain_c_level(level, width, 0..height),
        RowOrder::BottomUp => chain_c_level(level, width, (0..height).rev()),
    };
    [
        level(0, 3, 3),
        level(1, 3, 2),
        level(2, 1, 2),
        level(3, 1, 1),
    ]
}

fn chain_c(format: PixelFormat, row_order: RowOrder) -> ImageDescriptor {
    let palette = (format == PixelFormat::Indexed8).then(palette_c);
    ImageDescriptor::new(parts(
        Extent::new(3, 3),
        &CHAIN_C,
        format,
        row_order,
        palette,
    ))
    .expect("chain C descriptor is valid")
}

/// Sixteen entries; entry `i` is `[i, 0x40 + i, 0x80 + i]`.
fn palette_c() -> Vec<PaletteEntry> {
    (0..16)
        .map(|i| PaletteEntry::new(i, 0x40 + i, 0x80 + i))
        .collect()
}

/// Chain C as palette indices, bottom-up: index `(level * 4 + x + y) % 16`.
fn chain_c_indices_bottom_up() -> [Vec<u8>; 4] {
    let level = |level: u8, width: u8, height: u8| -> Vec<u8> {
        (0..height)
            .rev()
            .flat_map(|y| (0..width).map(move |x| (level * 4 + x + y) % 16))
            .collect()
    };
    [
        level(0, 3, 3),
        level(1, 3, 2),
        level(2, 1, 2),
        level(3, 1, 1),
    ]
}

fn slices<const N: usize>(levels: &[Vec<u8>; N]) -> Vec<&[u8]> {
    levels.iter().map(Vec::as_slice).collect()
}

// --- Decoding every level --------------------------------------------------

#[test]
fn accept_f08_b_01_non_square_3x2_chain_decodes_every_level_at_named_texels() {
    let stored = chain_a_top_down();
    let described = chain_a(RowOrder::TopDown);
    let levels = decode(&described, &slices(&stored)).expect("chain A decodes");
    assert_chain_a(&levels);

    // The base level is exactly what the base-level entrypoint returns.
    let mut budget = AllocationBudget::with_defaults(CONTAINER);
    let base = decode_base_level(CONTAINER, &described, &stored[0], &mut budget)
        .expect("the base level alone decodes");
    assert_eq!(levels.base(), &base);
    for level in levels.levels() {
        assert_eq!(level.format(), DecodedFormat::Rgb8);
        assert_eq!(level.indices(), None);
    }
}

#[test]
fn accept_f08_b_01_non_square_4x1_indexed_chain_decodes_every_level() {
    let mut described = parts(
        Extent::new(4, 1),
        &CHAIN_B,
        PixelFormat::Indexed8,
        RowOrder::TopDown,
        Some(palette_b()),
    );
    described.alpha_source = AlphaSource::PaletteKey { index: 4 };
    described.alpha_test = AlphaTest::Threshold(0x80);
    described.color_space = ColorSpace::Srgb;
    let described = ImageDescriptor::new(described).expect("chain B descriptor is valid");

    let stored: [&[u8]; 3] = [&[3, 0, 6, 4], &[5, 2], &[7]];
    let levels = decode(&described, &stored).expect("chain B decodes");

    assert_texels(
        levels.base(),
        0,
        &[(0, 0, RED), (1, 0, GREEN), (2, 0, BLUE), (3, 0, WHITE)],
    );
    assert_eq!(levels.base().extent(), Extent::new(4, 1));
    assert_eq!(levels.base().texel(0, 1), None, "the base is one row high");
    let mip1 = levels.level(1).expect("mip level 1");
    assert_eq!(mip1.extent(), Extent::new(2, 1));
    assert_texels(mip1, 1, &[(0, 0, ORANGE), (1, 0, PURPLE)]);
    assert_eq!(mip1.indices(), Some(&[5, 2][..]));
    let mip2 = levels.level(2).expect("mip level 2");
    assert_eq!(mip2.extent(), Extent::new(1, 1));
    assert_texels(mip2, 2, &[(0, 0, GREY)]);
    assert_eq!(mip2.index(0, 0), Some(7));

    // Metadata travels with every level, unchanged; the keyed entry keeps
    // its colour.
    for level in levels.levels() {
        assert_eq!(level.alpha_source(), AlphaSource::PaletteKey { index: 4 });
        assert_eq!(level.alpha_test(), AlphaTest::Threshold(0x80));
        assert_eq!(level.color_space(), ColorSpace::Srgb);
    }
    assert_eq!(levels.base().index(3, 0), Some(4));
}

#[test]
fn accept_f08_b_01_bottom_up_storage_flips_every_level_exactly_once() {
    // Bottom-up storage of chain C decodes to the intended picture at
    // every level, including the two-row mips.
    let described = chain_c(PixelFormat::Rgb8, RowOrder::BottomUp);
    let levels = decode(&described, &slices(&chain_c_stored(RowOrder::BottomUp)))
        .expect("bottom-up chain C decodes");
    let top_down = decode(
        &chain_c(PixelFormat::Rgb8, RowOrder::TopDown),
        &slices(&chain_c_stored(RowOrder::TopDown)),
    )
    .expect("top-down chain C decodes");
    assert_eq!(levels, top_down, "row order does not change the picture");

    let extents = [(3, 3), (3, 2), (1, 2), (1, 1)];
    for (level, (&(width, height), image)) in extents.iter().zip(levels.levels()).enumerate() {
        assert_eq!(image.extent(), Extent::new(width, height), "level {level}");
        for y in 0..height {
            for x in 0..width {
                assert_eq!(
                    image.texel(x, y),
                    Some(&chain_c_color(level as u8, x as u8, y as u8)[..]),
                    "level {level}, texel ({x}, {y}) from the top-left"
                );
            }
        }
    }

    // Top-down bytes declared bottom-up are the vertical mirror at every
    // level: flipped once, not zero or two times.
    let mirrored = decode(&described, &slices(&chain_c_stored(RowOrder::TopDown)))
        .expect("same sizes, other row order");
    let mip1 = mirrored.level(1).expect("mip level 1");
    assert_eq!(mip1.texel(0, 0), Some(&chain_c_color(1, 0, 1)[..]));
    assert_eq!(mip1.texel(2, 1), Some(&chain_c_color(1, 2, 0)[..]));
    let mip2 = mirrored.level(2).expect("mip level 2");
    assert_eq!(mip2.texel(0, 0), Some(&chain_c_color(2, 0, 1)[..]));
    assert_eq!(mip2.texel(0, 1), Some(&chain_c_color(2, 0, 0)[..]));
    assert_eq!(
        mirrored.base().texel(1, 0),
        Some(&chain_c_color(0, 1, 2)[..])
    );

    // Bottom-up chain A: one-row mips are their own mirror, the base is
    // flipped.
    let a = chain_a_top_down();
    let bottom_up_base = [YELLOW, CYAN, MAGENTA, RED, GREEN, BLUE].concat();
    let levels = decode(
        &chain_a(RowOrder::BottomUp),
        &[&bottom_up_base, &a[1], &a[2]],
    )
    .expect("bottom-up chain A decodes");
    assert_chain_a(&levels);
}

// --- Per-level validation --------------------------------------------------

#[test]
fn accept_f08_b_01_palette_index_out_of_range_names_mip_level_and_canonical_texel() {
    let described = chain_c(PixelFormat::Indexed8, RowOrder::BottomUp);
    let good = chain_c_indices_bottom_up();
    decode(&described, &slices(&good)).expect("the indexed chain decodes");

    // Mip level 2 is 1x2 stored bottom-up: its first stored byte is the
    // bottom texel, canonical (0, 1).
    let mut bad = good.clone();
    bad[2][0] = 16;
    let error = decode(&described, &slices(&bad)).expect_err("index 16 of 16 entries");
    assert_eq!(
        error,
        TextureError::InMipLevel {
            level: 2,
            error: Box::new(TextureError::PaletteIndexOutOfRange {
                container: CONTAINER.to_owned(),
                offset: 0,
                x: 0,
                y: 1,
                index: 16,
                entries: 16,
            }),
        }
    );
    assert_eq!(error.code(), "palette_index_out_of_range");
    assert_eq!(error.mip_level(), Some(2));
    assert_eq!(error.container(), CONTAINER);
    let message = error.to_string();
    assert!(message.contains("mip level 2"), "{message}");
    assert!(message.contains("texel 0,1"), "{message}");

    // The smallest level is checked too.
    let mut bad = good.clone();
    bad[3][0] = 0xFF;
    let error = decode(&described, &slices(&bad)).expect_err("index 255 in the last level");
    assert_eq!(error.mip_level(), Some(3));
    assert_eq!(error.code(), "palette_index_out_of_range");

    // A bad base index is a base-level error, not a mip error.
    let mut bad = good;
    bad[0][8] = 16;
    let error = decode(&described, &slices(&bad)).expect_err("bad base index");
    assert_eq!(error.mip_level(), None);
    assert!(matches!(
        error,
        TextureError::PaletteIndexOutOfRange { x: 2, y: 0, .. }
    ));
}

#[test]
fn accept_f08_b_01_missing_extra_short_or_padded_level_is_rejected() {
    let described = chain_a(RowOrder::TopDown);
    let [base, mip1, mip2] = chain_a_top_down();

    // Missing level: no partial chain is accepted.
    let error = decode(&described, &[&base, &mip1]).expect_err("mip level 2 missing");
    assert_eq!(
        error,
        TextureError::LevelCountMismatch {
            container: CONTAINER.to_owned(),
            expected: 3,
            observed: 2,
        }
    );
    assert_eq!(error.code(), "level_count_mismatch");
    let error = decode(&described, &[&base]).expect_err("only the base level");
    assert_eq!(error.code(), "level_count_mismatch");

    // Extra level: a 1x1 level beyond the declared chain.
    let error = decode(&described, &[&base, &mip1, &mip2, &mip2]).expect_err("one level too many");
    assert_eq!(
        error,
        TextureError::LevelCountMismatch {
            container: CONTAINER.to_owned(),
            expected: 3,
            observed: 4,
        }
    );

    // Short mip level 2: a checked-read failure named with its level.
    let error = decode(&described, &[&base, &mip1, &mip2[..2]]).expect_err("mip level 2 short");
    assert_eq!(error.code(), "unexpected_eof");
    assert_eq!(error.mip_level(), Some(2));
    match &error {
        TextureError::InMipLevel { error, .. } => match error.as_ref() {
            TextureError::Parse(parse) => {
                assert_eq!(parse.kind, ParseErrorKind::UnexpectedEof);
                assert_eq!(parse.field, "texture.mip_level");
            }
            other => panic!("expected a checked-read failure, got {other:?}"),
        },
        other => panic!("expected a mip-level error, got {other:?}"),
    }

    // Padded mip level 1: trailing bytes, not cropped.
    let mut padded = mip1.clone();
    padded.push(0);
    let error = decode(&described, &[&base, &padded, &mip2]).expect_err("mip level 1 padded");
    assert_eq!(
        error,
        TextureError::InMipLevel {
            level: 1,
            error: Box::new(TextureError::TrailingBytes {
                container: CONTAINER.to_owned(),
                expected: 6,
                observed: 7,
            }),
        }
    );

    // Levels are not resized: mip level 1's bytes in the 1x1 slot and the
    // other way round are rejected, not scaled to fit.
    let error = decode(&described, &[&base, &mip2, &mip1]).expect_err("levels swapped");
    assert_eq!(error.code(), "unexpected_eof");
    assert_eq!(error.mip_level(), Some(1));

    // An empty level is short, whatever its position.
    let error = decode(&described, &[&base, &mip1, &[]]).expect_err("empty mip level 2");
    assert_eq!(error.mip_level(), Some(2));
    assert_eq!(error.code(), "unexpected_eof");

    // A descriptor without mips takes exactly one level.
    let base_only = ImageDescriptor::new(parts(
        Extent::new(3, 2),
        &[],
        PixelFormat::Rgb8,
        RowOrder::TopDown,
        None,
    ))
    .expect("valid");
    decode(&base_only, &[&base]).expect("base level only");
    assert_eq!(
        decode(&base_only, &[&base, &mip1])
            .expect_err("undeclared mip")
            .code(),
        "level_count_mismatch"
    );
    assert_eq!(
        decode(&base_only, &[]).expect_err("no levels").code(),
        "level_count_mismatch"
    );
}

#[test]
fn accept_f08_b_01_budget_is_charged_for_every_level() {
    let described = chain_c(PixelFormat::Indexed8, RowOrder::BottomUp);
    let stored = chain_c_indices_bottom_up();
    // (9 + 6 + 2 + 1) texels * (3 channels + 1 index).
    let total = 72;

    let mut budget = AllocationBudget::new(CONTAINER, total);
    decode_levels(CONTAINER, &described, &slices(&stored), &mut budget)
        .expect("exactly enough budget for the whole chain");
    assert_eq!(budget.used(), total);

    // One byte short: the last (1x1) level cannot be booked.
    let mut budget = AllocationBudget::new(CONTAINER, total - 1);
    let error = decode_levels(CONTAINER, &described, &slices(&stored), &mut budget)
        .expect_err("one byte short of budget");
    assert_eq!(error.code(), "allocation_budget_exceeded");
    assert_eq!(error.mip_level(), Some(3));

    // Enough for the base level only: mip level 1 is refused.
    let mut budget = AllocationBudget::new(CONTAINER, 9 * 4);
    let error = decode_levels(CONTAINER, &described, &slices(&stored), &mut budget)
        .expect_err("the base level alone fits");
    assert_eq!(error.code(), "allocation_budget_exceeded");
    assert_eq!(error.mip_level(), Some(1));

    // A wrong level count is refused before anything is charged.
    let mut budget = AllocationBudget::new(CONTAINER, total);
    let error = decode_levels(CONTAINER, &described, &slices(&stored)[..3], &mut budget)
        .expect_err("one level missing");
    assert_eq!(error.code(), "level_count_mismatch");
    assert_eq!(budget.used(), 0);
}
