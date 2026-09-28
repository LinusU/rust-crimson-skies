//! Acceptance stage F08-A: image descriptors and the asymmetric fixture
//! (`specs/F08-texture-archives-and-conventional-image-decoding.md`, section
//! `### F08-A`, AC01 plus the failure cases the scenario implies).
//!
//! Every byte in this file is authored here: newly authored synthetic
//! content, no original game data, no `CS_GAME_DIR` access.
//!
//! The fixture is a 3x2 image whose six texels all have different colors,
//! so no rotation, transposition or flip of it equals itself. The expected
//! texels are written out per coordinate ([`EXPECTED`]) independently of
//! the byte builders, so a decoder that swaps rows and columns or flips the
//! image the wrong number of times fails on a named coordinate.

mod mips;
mod zbd_package;

use cs_formats::ParseErrorKind;
use cs_formats::io::AllocationBudget;
use cs_formats::texture::{
    AlphaSource, AlphaTest, ColorSpace, DecodedFormat, DecodedImage, DescriptorError,
    DescriptorParts, Extent, ImageDescriptor, MAX_DIMENSION, MAX_MIP_LEVELS, MAX_PALETTE_ENTRIES,
    Palette, PaletteEntry, PixelFormat, RowOrder, TextureError, decode_base_level,
};

/// Provenance label carried by every result and error these tests assert on.
const CONTAINER: &str = "synthetic/f08_a_asymmetric_3x2";

const RED: [u8; 3] = [0xFF, 0x00, 0x00];
const GREEN: [u8; 3] = [0x00, 0xFF, 0x00];
const BLUE: [u8; 3] = [0x00, 0x00, 0xFF];
const YELLOW: [u8; 3] = [0xFF, 0xFF, 0x00];
const CYAN: [u8; 3] = [0x00, 0xFF, 0xFF];
const MAGENTA: [u8; 3] = [0xFF, 0x00, 0xFF];

/// The intended image, as `(x from the left, y from the top, color)`.
///
/// ```text
///        x=0     x=1    x=2
/// y=0    red     green  blue
/// y=1    yellow  cyan   magenta
/// ```
const EXPECTED: [(u32, u32, [u8; 3]); 6] = [
    (0, 0, RED),
    (1, 0, GREEN),
    (2, 0, BLUE),
    (0, 1, YELLOW),
    (1, 1, CYAN),
    (2, 1, MAGENTA),
];

/// The top row, then the bottom row, three bytes per texel.
fn rgb_top_down() -> Vec<u8> {
    [RED, GREEN, BLUE, YELLOW, CYAN, MAGENTA].concat()
}

/// The bottom row stored first, as a bottom-up file stores it.
fn rgb_bottom_up() -> Vec<u8> {
    [YELLOW, CYAN, MAGENTA, RED, GREEN, BLUE].concat()
}

/// A palette whose entry order differs from the image order, so an index
/// is never accidentally equal to its texel position.
fn palette() -> Vec<PaletteEntry> {
    [MAGENTA, BLUE, [0, 0, 0], YELLOW, RED, CYAN, GREEN]
        .iter()
        .map(|&[r, g, b]| PaletteEntry::new(r, g, b))
        .collect()
}

/// Indices into [`palette`] for the fixture, top row first.
const INDICES_TOP_DOWN: [u8; 6] = [4, 6, 1, 3, 5, 0];

fn parts(extent: Extent, format: PixelFormat, row_order: RowOrder) -> DescriptorParts {
    DescriptorParts {
        extent,
        format,
        row_order,
        palette: (format == PixelFormat::Indexed8).then(|| Palette::Rgb8(palette())),
        mips: Vec::new(),
        alpha_source: AlphaSource::Unknown,
        alpha_test: AlphaTest::Unknown,
        color_space: ColorSpace::Unknown,
    }
}

fn descriptor(extent: Extent, format: PixelFormat, row_order: RowOrder) -> ImageDescriptor {
    ImageDescriptor::new(parts(extent, format, row_order)).expect("fixture descriptor is valid")
}

fn decode(descriptor: &ImageDescriptor, stored: &[u8]) -> Result<DecodedImage, TextureError> {
    let mut budget = AllocationBudget::with_defaults(CONTAINER);
    decode_base_level(CONTAINER, descriptor, stored, &mut budget)
}

fn assert_fixture(image: &DecodedImage) {
    assert_eq!(image.extent(), Extent::new(3, 2));
    assert_eq!(image.format(), DecodedFormat::Rgb8);
    for (x, y, color) in EXPECTED {
        assert_eq!(
            image.texel(x, y),
            Some(&color[..]),
            "texel ({x}, {y}) from the top-left"
        );
    }
    assert_eq!(
        image.texel(3, 0),
        None,
        "column 3 is outside a 3-wide image"
    );
    assert_eq!(image.texel(0, 2), None, "row 2 is outside a 2-high image");
}

// --- AC01: the 3x2 asymmetric fixture -------------------------------------

#[test]
fn accept_f08_a_asymmetric_3x2_top_down_decodes_every_texel_in_place() {
    let image = decode(
        &descriptor(Extent::new(3, 2), PixelFormat::Rgb8, RowOrder::TopDown),
        &rgb_top_down(),
    )
    .expect("the top-down fixture decodes");
    assert_fixture(&image);
    assert_eq!(
        image.texels(),
        &rgb_top_down()[..],
        "row-major from the top"
    );
    assert_eq!(image.indices(), None, "a direct-color image has no indices");
}

#[test]
fn accept_f08_a_bottom_up_rows_are_flipped_exactly_once() {
    // The same picture stored bottom row first decodes to the same image.
    let image = decode(
        &descriptor(Extent::new(3, 2), PixelFormat::Rgb8, RowOrder::BottomUp),
        &rgb_bottom_up(),
    )
    .expect("the bottom-up fixture decodes");
    assert_fixture(&image);

    // Top-down bytes declared bottom-up are the vertical mirror: the stored
    // first row lands at the bottom, columns stay where they are.
    let mirrored = decode(
        &descriptor(Extent::new(3, 2), PixelFormat::Rgb8, RowOrder::BottomUp),
        &rgb_top_down(),
    )
    .expect("same size, different row order");
    assert_eq!(mirrored.texel(0, 0), Some(&YELLOW[..]));
    assert_eq!(mirrored.texel(2, 0), Some(&MAGENTA[..]));
    assert_eq!(mirrored.texel(0, 1), Some(&RED[..]));
    assert_eq!(mirrored.texel(2, 1), Some(&BLUE[..]));
}

#[test]
fn accept_f08_a_row_and_column_counts_are_not_interchangeable() {
    // The same 18 bytes read as a 2-wide, 3-high image are a different
    // image: a decoder that swaps width and height cannot pass both halves.
    let transposed = decode(
        &descriptor(Extent::new(2, 3), PixelFormat::Rgb8, RowOrder::TopDown),
        &rgb_top_down(),
    )
    .expect("2x3 accounts for the same byte count");
    assert_eq!(transposed.extent(), Extent::new(2, 3));
    assert_eq!(transposed.texel(0, 1), Some(&BLUE[..]));
    assert_eq!(transposed.texel(1, 2), Some(&MAGENTA[..]));
    assert_eq!(
        transposed.texel(2, 0),
        None,
        "column 2 is outside a 2-wide image"
    );

    let image = decode(
        &descriptor(Extent::new(3, 2), PixelFormat::Rgb8, RowOrder::TopDown),
        &rgb_top_down(),
    )
    .expect("the fixture decodes");
    assert_ne!(image, transposed);
    assert_eq!(image.texel(0, 1), Some(&YELLOW[..]));
    assert_eq!(image.texel(2, 0), Some(&BLUE[..]));
}

#[test]
fn accept_f08_a_indexed_fixture_matches_direct_color_and_keeps_indices() {
    let top_down = decode(
        &descriptor(Extent::new(3, 2), PixelFormat::Indexed8, RowOrder::TopDown),
        &INDICES_TOP_DOWN,
    )
    .expect("the indexed fixture decodes");
    assert_fixture(&top_down);
    assert_eq!(top_down.indices(), Some(&INDICES_TOP_DOWN[..]));
    assert_eq!(top_down.index(0, 0), Some(4));
    assert_eq!(top_down.index(2, 1), Some(0));

    let stored_bottom_up = [3, 5, 0, 4, 6, 1];
    let bottom_up = decode(
        &descriptor(Extent::new(3, 2), PixelFormat::Indexed8, RowOrder::BottomUp),
        &stored_bottom_up,
    )
    .expect("the bottom-up indexed fixture decodes");
    assert_fixture(&bottom_up);
    assert_eq!(
        bottom_up.indices(),
        Some(&INDICES_TOP_DOWN[..]),
        "indices are reordered with the texels"
    );
}

// --- Values pass through unchanged -----------------------------------------

#[test]
fn accept_f08_a_black_is_not_transparent_and_alpha_is_reported_as_stored() {
    // A black texel with full stored alpha stays black and opaque; a
    // coloured texel with zero stored alpha keeps its colour (no
    // premultiplication).
    let stored = [
        [0x00, 0x00, 0x00, 0xFF],
        [0xFF, 0x00, 0x00, 0x00],
        [0x00, 0xFF, 0x00, 0x80],
        [0x00, 0x00, 0xFF, 0x01],
        [0x00, 0x00, 0x00, 0x00],
        [0x10, 0x20, 0x30, 0xFE],
    ]
    .concat();
    let mut rgba = parts(Extent::new(3, 2), PixelFormat::Rgba8, RowOrder::TopDown);
    rgba.alpha_source = AlphaSource::Channel;
    rgba.alpha_test = AlphaTest::Threshold(0x80);
    let image = decode(&ImageDescriptor::new(rgba).expect("valid"), &stored).expect("decodes");
    assert_eq!(image.format(), DecodedFormat::Rgba8);
    assert_eq!(image.texels(), &stored[..]);
    assert_eq!(image.texel(0, 0), Some(&[0x00, 0x00, 0x00, 0xFF][..]));
    assert_eq!(image.texel(1, 0), Some(&[0xFF, 0x00, 0x00, 0x00][..]));
    assert_eq!(image.alpha_source(), AlphaSource::Channel);
    assert_eq!(image.alpha_test(), AlphaTest::Threshold(0x80));

    // A palette key is metadata: the keyed texel keeps its palette colour,
    // and the black entry (index 2) that is *not* keyed is not transparent.
    let mut keyed = parts(Extent::new(3, 2), PixelFormat::Indexed8, RowOrder::TopDown);
    keyed.alpha_source = AlphaSource::PaletteKey { index: 4 };
    keyed.color_space = ColorSpace::Srgb;
    let image = decode(
        &ImageDescriptor::new(keyed).expect("valid"),
        &[4, 6, 1, 2, 5, 0],
    )
    .expect("decodes");
    assert_eq!(image.format(), DecodedFormat::Rgb8);
    assert_eq!(
        image.texel(0, 0),
        Some(&RED[..]),
        "keyed texel keeps its colour"
    );
    assert_eq!(image.index(0, 0), Some(4));
    assert_eq!(image.texel(0, 1), Some(&[0, 0, 0][..]));
    assert_eq!(image.index(0, 1), Some(2));
    assert_eq!(image.alpha_source(), AlphaSource::PaletteKey { index: 4 });
    assert_eq!(image.alpha_test(), AlphaTest::Unknown);
    assert_eq!(
        image.color_space(),
        ColorSpace::Srgb,
        "no conversion, carried"
    );
}

// --- Failure cases ---------------------------------------------------------

#[test]
fn accept_f08_a_partial_or_padded_base_level_is_rejected() {
    let described = descriptor(Extent::new(3, 2), PixelFormat::Rgb8, RowOrder::TopDown);
    assert_eq!(described.base_level_bytes(), 18);

    let mut short = rgb_top_down();
    short.pop();
    let error = decode(&described, &short).expect_err("one byte short");
    assert_eq!(error.code(), "unexpected_eof");
    assert_eq!(error.container(), CONTAINER);
    match &error {
        TextureError::Parse(parse) => {
            assert_eq!(parse.kind, ParseErrorKind::UnexpectedEof);
            assert_eq!(parse.field, "texture.base_level");
        }
        other => panic!("expected a checked-read failure, got {other:?}"),
    }

    let mut long = rgb_top_down();
    long.push(0);
    let error = decode(&described, &long).expect_err("one byte too many");
    assert_eq!(
        error,
        TextureError::TrailingBytes {
            container: CONTAINER.to_owned(),
            expected: 18,
            observed: 19,
        }
    );
    assert_eq!(error.code(), "trailing_bytes");

    // The 3x2 fixture's bytes do not fit a 3x3 description either: no
    // silent resize to whatever the bytes allow.
    let error = decode(
        &descriptor(Extent::new(3, 3), PixelFormat::Rgb8, RowOrder::TopDown),
        &rgb_top_down(),
    )
    .expect_err("3x3 needs 27 bytes");
    assert_eq!(error.code(), "unexpected_eof");
}

#[test]
fn accept_f08_a_palette_index_out_of_range_names_the_canonical_texel() {
    // Stored bottom-up: the bad index is the first stored byte, which is
    // the bottom-left texel of the image.
    let stored = [7, 5, 0, 4, 6, 1];
    let error = decode(
        &descriptor(Extent::new(3, 2), PixelFormat::Indexed8, RowOrder::BottomUp),
        &stored,
    )
    .expect_err("index 7 is outside the 7-entry palette");
    assert_eq!(
        error,
        TextureError::PaletteIndexOutOfRange {
            container: CONTAINER.to_owned(),
            offset: 0,
            x: 0,
            y: 1,
            index: 7,
            entries: 7,
        }
    );
    assert_eq!(error.code(), "palette_index_out_of_range");
    assert!(error.to_string().contains("texel 0,1"), "{error}");
}

#[test]
fn accept_f08_a_decoded_allocation_is_charged_to_the_budget() {
    let described = descriptor(Extent::new(3, 2), PixelFormat::Indexed8, RowOrder::TopDown);

    // 6 texels * 3 channels + 6 indices.
    let mut budget = AllocationBudget::new(CONTAINER, 24);
    decode_base_level(CONTAINER, &described, &INDICES_TOP_DOWN, &mut budget)
        .expect("exactly enough budget");
    assert_eq!(budget.used(), 24);

    let mut budget = AllocationBudget::new(CONTAINER, 23);
    let error = decode_base_level(CONTAINER, &described, &INDICES_TOP_DOWN, &mut budget)
        .expect_err("one byte short of budget");
    assert_eq!(error.code(), "allocation_budget_exceeded");
}

#[test]
fn accept_f08_a_descriptor_rejects_inconsistent_parts() {
    let rgb = || parts(Extent::new(3, 2), PixelFormat::Rgb8, RowOrder::TopDown);
    let indexed = || parts(Extent::new(3, 2), PixelFormat::Indexed8, RowOrder::TopDown);
    let reject = |parts: DescriptorParts| ImageDescriptor::new(parts).expect_err("rejected");

    let mut p = rgb();
    p.extent = Extent::new(0, 2);
    assert_eq!(
        reject(p),
        DescriptorError::ZeroDimension {
            level: None,
            extent: Extent::new(0, 2),
        }
    );

    let mut p = rgb();
    p.extent = Extent::new(3, MAX_DIMENSION + 1);
    assert_eq!(reject(p).code(), "dimension_too_large");
    let mut p = rgb();
    p.extent = Extent::new(MAX_DIMENSION, MAX_DIMENSION);
    ImageDescriptor::new(p).expect("the limit itself is accepted");

    let mut p = rgb();
    p.mips = vec![Extent::new(1, 1); MAX_MIP_LEVELS + 1];
    assert_eq!(reject(p).code(), "too_many_mip_levels");

    let mut p = rgb();
    p.mips = vec![Extent::new(3, 2)];
    assert_eq!(
        reject(p),
        DescriptorError::MipNotSmaller {
            level: 1,
            extent: Extent::new(3, 2),
            above: Extent::new(3, 2),
        }
    );
    let mut p = rgb();
    p.mips = vec![Extent::new(1, 1), Extent::new(0, 1)];
    assert_eq!(
        reject(p),
        DescriptorError::ZeroDimension {
            level: Some(2),
            extent: Extent::new(0, 1),
        }
    );
    let mut p = rgb();
    p.mips = vec![Extent::new(2, 1), Extent::new(1, 1)];
    let mipped = ImageDescriptor::new(p).expect("a non-square shrinking chain is accepted");
    assert_eq!(mipped.mips(), &[Extent::new(2, 1), Extent::new(1, 1)]);

    let mut p = indexed();
    p.palette = None;
    assert_eq!(reject(p).code(), "palette_missing");
    let mut p = indexed();
    p.palette = Some(Palette::Rgb8(Vec::new()));
    assert_eq!(reject(p).code(), "palette_size");
    let mut p = indexed();
    p.palette = Some(Palette::Rgb8(vec![
        PaletteEntry::new(0, 0, 0);
        MAX_PALETTE_ENTRIES + 1
    ]));
    assert_eq!(reject(p).code(), "palette_size");
    let mut p = rgb();
    p.palette = Some(Palette::Rgb8(palette()));
    assert_eq!(reject(p).code(), "palette_not_allowed");

    let mut p = rgb();
    p.alpha_source = AlphaSource::Channel;
    assert_eq!(
        reject(p),
        DescriptorError::AlphaChannelMissing {
            format: PixelFormat::Rgb8,
        }
    );
    let mut p = indexed();
    p.alpha_source = AlphaSource::PaletteKey { index: 7 };
    assert_eq!(
        reject(p),
        DescriptorError::PaletteKeyOutOfRange {
            index: 7,
            entries: 7,
        }
    );
    let mut p = rgb();
    p.alpha_source = AlphaSource::PaletteKey { index: 0 };
    assert_eq!(reject(p).code(), "palette_key_out_of_range");
}
