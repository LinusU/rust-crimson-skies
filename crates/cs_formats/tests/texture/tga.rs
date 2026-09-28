//! Acceptance stage F08-B.04: conventional type 2 and RLE type 10 true color
//! TGAs with explicit alpha-bit handling
//! (`specs/F08-texture-archives-and-conventional-image-decoding.md`, section
//! `### F08-B`, AC02 "alpha edge", non-negotiables #1, #2 and #5).
//!
//! Every file here is built byte by byte in this file from the published
//! TGA 2.0 layout recorded in
//! `docs/findings/2026-09-28-f08-b-04-conventional-tga.md`: newly authored
//! synthetic content, no original game data, no `CS_GAME_DIR` access.
//! Expected texels are written per coordinate in red, green, blue, alpha
//! order, independently of the stored blue, green, red, alpha bytes, and
//! the RLE packets are written out as literal headers.

use cs_formats::ParseErrorKind;
use cs_formats::io::AllocationBudget;
use cs_formats::texture::tga::{TGA_EXTENSION_BYTES, TGA_FOOTER_SIGNATURE, TGA_HEADER_BYTES};
use cs_formats::texture::{
    AlphaSource, AlphaTest, ColorSpace, DecodedFormat, DecodedImage, DescriptorError, Extent,
    MAX_DIMENSION, PixelFormat, RowOrder, TgaError, TgaExtension, TgaFooter, TgaImage, TgaRleStats,
    read_tga,
};

const CONTAINER: &str = "synthetic/f08_b_04_asymmetric.tga";

const RED: [u8; 3] = [0xFF, 0x00, 0x00];
const GREEN: [u8; 3] = [0x00, 0xFF, 0x00];
const BLUE: [u8; 3] = [0x00, 0x00, 0xFF];
const YELLOW: [u8; 3] = [0xFF, 0xFF, 0x00];
const CYAN: [u8; 3] = [0x00, 0xFF, 0xFF];
const MAGENTA: [u8; 3] = [0xFF, 0x00, 0xFF];
const ORANGE: [u8; 3] = [0xF0, 0x80, 0x10];

/// The intended 3x2 image, as `(x from the left, y from the top, red,
/// green, blue, alpha)`. Alpha 0 and 255 sit on the corner texels.
///
/// ```text
///        x=0          x=1          x=2
/// y=0    red     0    green  0x40  blue     255
/// y=1    yellow  255  cyan   0x80  magenta  0
/// ```
const EXPECTED_3X2: [(u32, u32, [u8; 4]); 6] = [
    (0, 0, [0xFF, 0x00, 0x00, 0x00]),
    (1, 0, [0x00, 0xFF, 0x00, 0x40]),
    (2, 0, [0x00, 0x00, 0xFF, 0xFF]),
    (0, 1, [0xFF, 0xFF, 0x00, 0xFF]),
    (1, 1, [0x00, 0xFF, 0xFF, 0x80]),
    (2, 1, [0xFF, 0x00, 0xFF, 0x00]),
];

/// One stored texel: blue, green, red, then `alpha` when given.
fn bgr(color: [u8; 3], alpha: Option<u8>) -> Vec<u8> {
    let [r, g, b] = color;
    let mut texel = vec![b, g, r];
    texel.extend(alpha);
    texel
}

fn bgra(color: [u8; 3], alpha: u8) -> Vec<u8> {
    bgr(color, Some(alpha))
}

/// Stored 32 bpp texels of the top row, then of the bottom row.
fn top_row() -> Vec<Vec<u8>> {
    vec![bgra(RED, 0), bgra(GREEN, 0x40), bgra(BLUE, 0xFF)]
}

fn bottom_row() -> Vec<Vec<u8>> {
    vec![bgra(YELLOW, 0xFF), bgra(CYAN, 0x80), bgra(MAGENTA, 0)]
}

/// The six stored texels of the 3x2 image in `order`.
fn stored_texels(order: RowOrder) -> Vec<Vec<u8>> {
    match order {
        RowOrder::TopDown => [top_row(), bottom_row()].concat(),
        RowOrder::BottomUp => [bottom_row(), top_row()].concat(),
    }
}

/// RLE packets for the six stored texels: a run of one, a raw packet of
/// three that crosses from the first stored row into the second, a raw
/// packet of two.
fn rle_packets(texels: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0x80];
    out.extend_from_slice(&texels[0]);
    out.push(0x02);
    out.extend(texels[1..4].concat());
    out.push(0x01);
    out.extend(texels[4..6].concat());
    out
}

/// Image descriptor bit 5: top-left origin.
const TOP_LEFT: u8 = 0x20;

/// One authored TGA.
struct Tga {
    image_type: u8,
    width: u16,
    height: u16,
    pixel_depth: u8,
    image_descriptor: u8,
    image_id: Vec<u8>,
    /// Stored pixel data: texels for type 2, packets for type 10.
    pixels: Vec<u8>,
    /// Bytes after the pixels (extension area, footer, garbage).
    trailer: Vec<u8>,
}

impl Tga {
    fn bytes(&self) -> Vec<u8> {
        let mut out = vec![self.image_id.len() as u8, 0, self.image_type];
        out.extend_from_slice(&[0; 5]);
        out.extend_from_slice(&7u16.to_le_bytes());
        out.extend_from_slice(&9u16.to_le_bytes());
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&self.height.to_le_bytes());
        out.push(self.pixel_depth);
        out.push(self.image_descriptor);
        out.extend_from_slice(&self.image_id);
        out.extend_from_slice(&self.pixels);
        out.extend_from_slice(&self.trailer);
        out
    }

    fn pixels_end(&self) -> u64 {
        u64::from(TGA_HEADER_BYTES) + self.image_id.len() as u64 + self.pixels.len() as u64
    }
}

/// The 3x2 image at 32 bpp with 8 alpha bits, uncompressed (type 2) or RLE
/// (type 10), stored in `order`.
fn asymmetric(image_type: u8, order: RowOrder) -> Tga {
    let texels = stored_texels(order);
    let pixels = match image_type {
        2 => texels.concat(),
        10 => rle_packets(&texels),
        _ => unreachable!(),
    };
    let origin = match order {
        RowOrder::TopDown => TOP_LEFT,
        RowOrder::BottomUp => 0,
    };
    Tga {
        image_type,
        width: 3,
        height: 2,
        pixel_depth: 32,
        image_descriptor: origin | 8,
        image_id: Vec::new(),
        pixels,
        trailer: Vec::new(),
    }
}

/// A TGA 2.0 footer with the given area offsets.
fn footer(extension_offset: u32, developer_offset: u32) -> Vec<u8> {
    let mut out = extension_offset.to_le_bytes().to_vec();
    out.extend_from_slice(&developer_offset.to_le_bytes());
    out.extend_from_slice(&TGA_FOOTER_SIGNATURE);
    out
}

/// A 495-byte extension area with the given attributes type.
fn extension(attributes_type: u8) -> Vec<u8> {
    let mut out = vec![0x20; usize::from(TGA_EXTENSION_BYTES)];
    out[..2].copy_from_slice(&TGA_EXTENSION_BYTES.to_le_bytes());
    out[482..494].fill(0);
    out[494] = attributes_type;
    out
}

fn budget() -> AllocationBudget {
    AllocationBudget::with_defaults(CONTAINER)
}

fn read(bytes: &[u8]) -> Result<TgaImage, TgaError> {
    read_tga(CONTAINER, bytes, &mut budget())
}

fn decode(image: &TgaImage) -> DecodedImage {
    image.decode(&mut budget()).expect("decodes")
}

fn assert_asymmetric(decoded: &DecodedImage) {
    assert_eq!(decoded.extent(), Extent::new(3, 2));
    assert_eq!(decoded.format(), DecodedFormat::Rgba8);
    for (x, y, rgba) in EXPECTED_3X2 {
        assert_eq!(decoded.texel(x, y), Some(&rgba[..]), "texel ({x}, {y})");
    }
    assert_eq!(decoded.alpha(), None);
}

#[test]
fn accept_f08_b_04_uncompressed_3x2_decodes_every_texel_as_rgba_in_both_origins() {
    for order in [RowOrder::BottomUp, RowOrder::TopDown] {
        let image = read(&asymmetric(2, order).bytes()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(image.image_type(), 2);
        assert_eq!(image.pixel_depth(), 32);
        assert_eq!(image.alpha_bits(), 8);
        assert_eq!(image.origin(), (7, 9));
        assert_eq!(image.rle(), None);
        assert_eq!(image.footer(), None);
        let descriptor = image.descriptor();
        assert_eq!(descriptor.extent(), Extent::new(3, 2));
        assert_eq!(descriptor.format(), PixelFormat::Rgba8);
        assert_eq!(descriptor.row_order(), order);
        assert_eq!(descriptor.mips(), &[]);
        assert_eq!(descriptor.palette(), None);
        assert_eq!(descriptor.alpha_source(), AlphaSource::Channel);
        assert_eq!(descriptor.alpha_test(), AlphaTest::Unknown);
        assert_eq!(descriptor.color_space(), ColorSpace::Unknown);
        let decoded = decode(&image);
        assert_asymmetric(&decoded);
        assert_eq!(decoded.alpha_source(), AlphaSource::Channel);
    }
}

#[test]
fn accept_f08_b_04_rle_3x2_decodes_every_texel_as_rgba_in_both_origins() {
    for order in [RowOrder::BottomUp, RowOrder::TopDown] {
        let tga = asymmetric(10, order);
        let image = read(&tga.bytes()).unwrap_or_else(|e| panic!("{order:?}: {e}"));
        assert_eq!(image.image_type(), 10);
        assert_eq!(image.descriptor().row_order(), order);
        assert_eq!(
            image.rle(),
            Some(TgaRleStats {
                packets: 3,
                row_crossing_packets: 1,
            })
        );
        assert_asymmetric(&decode(&image));
        // The same image uncompressed gives the same stored texels.
        let plain = read(&asymmetric(2, order).bytes()).unwrap();
        assert_eq!(image.stored_texels(), plain.stored_texels());
        assert_eq!(image.descriptor(), plain.descriptor());
    }
}

#[test]
fn accept_f08_b_04_bgr_order_is_reordered_per_texel_and_values_are_kept() {
    // Every channel of every texel distinct, so any swap shows.
    let colors = [
        [0x01, 0x02, 0x03, 0x04],
        [0x11, 0x12, 0x13, 0x14],
        [0x21, 0x22, 0x23, 0x24],
    ];
    let pixels: Vec<u8> = colors
        .iter()
        .flat_map(|&[r, g, b, a]| [b, g, r, a])
        .collect();
    let tga = Tga {
        image_type: 2,
        width: 3,
        height: 1,
        pixel_depth: 32,
        image_descriptor: 8,
        image_id: Vec::new(),
        pixels,
        trailer: Vec::new(),
    };
    let image = read(&tga.bytes()).unwrap();
    assert_eq!(image.stored_texels(), colors.concat());
    let decoded = decode(&image);
    for (x, rgba) in colors.iter().enumerate() {
        assert_eq!(decoded.texel(x as u32, 0), Some(&rgba[..]), "texel {x}");
    }
}

#[test]
fn accept_f08_b_04_alpha_bits_8_and_0_differ_only_in_alpha_metadata() {
    for image_type in [2, 10] {
        let mut declared = asymmetric(image_type, RowOrder::BottomUp);
        let with_alpha = read(&declared.bytes()).unwrap();
        declared.image_descriptor = 0;
        let undeclared = read(&declared.bytes()).unwrap();

        assert_eq!(with_alpha.alpha_bits(), 8);
        assert_eq!(undeclared.alpha_bits(), 0);
        assert_eq!(with_alpha.descriptor().alpha_source(), AlphaSource::Channel);
        assert_eq!(undeclared.descriptor().alpha_source(), AlphaSource::Unknown);
        // Identical channel bytes, alpha byte included.
        assert_eq!(
            undeclared.descriptor().format(),
            with_alpha.descriptor().format()
        );
        assert_eq!(undeclared.stored_texels(), with_alpha.stored_texels());
        let (a, b) = (decode(&with_alpha), decode(&undeclared));
        assert_eq!(a.texels(), b.texels());
        assert_asymmetric(&b);
        assert_eq!(a.alpha_source(), AlphaSource::Channel);
        assert_eq!(b.alpha_source(), AlphaSource::Unknown);
    }
}

#[test]
fn accept_f08_b_04_24bpp_is_opaque_rgb_in_both_types() {
    let top: Vec<Vec<u8>> = [RED, GREEN, BLUE].map(|c| bgr(c, None)).to_vec();
    let bottom: Vec<Vec<u8>> = [YELLOW, CYAN, MAGENTA].map(|c| bgr(c, None)).to_vec();
    let stored = [bottom, top].concat();
    for (image_type, pixels) in [(2, stored.concat()), (10, rle_packets(&stored))] {
        let tga = Tga {
            image_type,
            width: 3,
            height: 2,
            pixel_depth: 24,
            image_descriptor: 0,
            image_id: Vec::new(),
            pixels,
            trailer: Vec::new(),
        };
        let image = read(&tga.bytes()).unwrap_or_else(|e| panic!("type {image_type}: {e}"));
        assert_eq!(image.descriptor().format(), PixelFormat::Rgb8);
        assert_eq!(image.descriptor().alpha_source(), AlphaSource::Opaque);
        let decoded = decode(&image);
        assert_eq!(decoded.format(), DecodedFormat::Rgb8);
        for (x, y, rgba) in EXPECTED_3X2 {
            assert_eq!(decoded.texel(x, y), Some(&rgba[..3]), "texel ({x}, {y})");
        }
    }
}

#[test]
fn accept_f08_b_04_non_square_2x3_rle_runs_may_span_rows() {
    // 2 columns by 3 rows, top-left origin:
    //   y=0: orange orange, y=1: orange green, y=2: blue blue
    // A run of three crosses rows 0 and 1; a run of two fills row 2.
    let mut pixels = vec![0x82];
    pixels.extend(bgr(ORANGE, None));
    pixels.push(0x00);
    pixels.extend(bgr(GREEN, None));
    pixels.push(0x81);
    pixels.extend(bgr(BLUE, None));
    let tga = Tga {
        image_type: 10,
        width: 2,
        height: 3,
        pixel_depth: 24,
        image_descriptor: TOP_LEFT,
        image_id: Vec::new(),
        pixels,
        trailer: Vec::new(),
    };
    let image = read(&tga.bytes()).unwrap();
    assert_eq!(
        image.rle(),
        Some(TgaRleStats {
            packets: 3,
            row_crossing_packets: 1,
        })
    );
    let decoded = decode(&image);
    assert_eq!(decoded.extent(), Extent::new(2, 3));
    let expected = [
        (0, 0, ORANGE),
        (1, 0, ORANGE),
        (0, 1, ORANGE),
        (1, 1, GREEN),
        (0, 2, BLUE),
        (1, 2, BLUE),
    ];
    for (x, y, color) in expected {
        assert_eq!(decoded.texel(x, y), Some(&color[..]), "texel ({x}, {y})");
    }
}

#[test]
fn accept_f08_b_04_rle_packets_running_past_the_image_are_rejected() {
    let texels = stored_texels(RowOrder::BottomUp);
    let first = u64::from(TGA_HEADER_BYTES);

    // Five texels, then a run of two: one past the end.
    let mut run = vec![0x04];
    run.extend(texels[..5].concat());
    let run_at = first + run.len() as u64;
    run.push(0x81);
    run.extend_from_slice(&texels[5]);
    // Three texels, then a raw packet of four.
    let mut raw = vec![0x02];
    raw.extend(texels[..3].concat());
    let raw_at = first + raw.len() as u64;
    raw.push(0x03);
    raw.extend(texels[3..].concat());
    raw.extend_from_slice(&texels[0]);
    // One packet declaring the maximum of 128 texels.
    let long = [vec![0xFF], texels[0].clone()].concat();

    for (pixels, offset, count, remaining) in [
        (run, run_at, 2, 1),
        (raw, raw_at, 4, 3),
        (long, first, 128, 6),
    ] {
        let mut tga = asymmetric(10, RowOrder::BottomUp);
        tga.pixels = pixels;
        let error = read(&tga.bytes()).unwrap_err();
        assert_eq!(error.code(), "rle_run_past_image");
        assert_eq!(
            error,
            TgaError::RunPastImage {
                container: CONTAINER.to_owned(),
                offset,
                count,
                remaining,
            }
        );
    }
}

#[test]
fn accept_f08_b_04_truncated_rle_packets_are_rejected() {
    let texels = stored_texels(RowOrder::BottomUp);
    // A raw packet of five texels missing its last byte.
    let mut short_raw = vec![0x04];
    short_raw.extend(texels[..5].concat());
    short_raw.truncate(short_raw.len() - 1);
    // A run packet header without its texel, and one with half a texel.
    let mut no_run_texel = vec![0x04];
    no_run_texel.extend(texels[..5].concat());
    no_run_texel.push(0x80);
    let mut half_run_texel = no_run_texel.clone();
    half_run_texel.extend_from_slice(&texels[5][..2]);
    // Packets that stop one texel short: the next header is missing.
    let mut no_header = vec![0x04];
    no_header.extend(texels[..5].concat());

    for (pixels, field) in [
        (short_raw, "tga.rle.raw_texels"),
        (no_run_texel, "tga.rle.run_texel"),
        (half_run_texel, "tga.rle.run_texel"),
        (no_header, "tga.rle.packet_header"),
    ] {
        let mut tga = asymmetric(10, RowOrder::BottomUp);
        tga.pixels = pixels;
        let error = read(&tga.bytes()).unwrap_err();
        let TgaError::Parse(parse) = &error else {
            panic!("{field}: expected a parse error, got {error:?}");
        };
        assert_eq!(parse.kind, ParseErrorKind::UnexpectedEof, "{field}");
        assert_eq!(parse.field, field);
    }

    // Uncompressed pixels one byte short.
    let mut tga = asymmetric(2, RowOrder::TopDown);
    tga.pixels.pop();
    let error = read(&tga.bytes()).unwrap_err();
    assert_eq!(error.code(), "unexpected_eof");
    assert_eq!(error.field(), Some("tga.pixels"));
}

#[test]
fn accept_f08_b_04_trailing_bytes_are_an_error_unless_they_are_a_tga2_footer() {
    for image_type in [2, 10] {
        let base = asymmetric(image_type, RowOrder::BottomUp);
        let end = base.pixels_end();

        // A footer without an extension area.
        let mut tga = asymmetric(image_type, RowOrder::BottomUp);
        tga.trailer = footer(0, 0);
        let image = read(&tga.bytes()).unwrap();
        assert_eq!(
            image.footer(),
            Some(TgaFooter {
                offset: end,
                extension_offset: 0,
                developer_offset: 0,
            })
        );
        assert_eq!(image.extension(), None);
        assert_asymmetric(&decode(&image));

        // A footer pointing at an extension area directly after the pixels.
        tga.trailer = [extension(3), footer(end as u32, 0)].concat();
        let image = read(&tga.bytes()).unwrap();
        assert_eq!(
            image.extension(),
            Some(TgaExtension {
                offset: end,
                attributes_type: 3,
            })
        );
        assert_eq!(image.footer().unwrap().offset, end + 495);
        assert_asymmetric(&decode(&image));

        // Plain trailing bytes, and a byte before a footer.
        for trailer in [vec![0], vec![0; 30], [vec![0], footer(0, 0)].concat()] {
            tga.trailer = trailer;
            let bytes = tga.bytes();
            assert_eq!(
                read(&bytes).unwrap_err(),
                TgaError::TrailingBytes {
                    container: CONTAINER.to_owned(),
                    offset: end,
                    len: bytes.len() as u64 - end,
                }
            );
        }
        // A byte between the extension area and the footer.
        tga.trailer = [extension(0), vec![0], footer(end as u32, 0)].concat();
        let bytes = tga.bytes();
        assert_eq!(
            read(&bytes).unwrap_err(),
            TgaError::TrailingBytes {
                container: CONTAINER.to_owned(),
                offset: end + 495,
                len: bytes.len() as u64 - end - 495,
            }
        );
    }
}

#[test]
fn accept_f08_b_04_footer_and_extension_fields_are_checked() {
    let base = asymmetric(10, RowOrder::BottomUp);
    let end = base.pixels_end() as u32;
    let cases: [(Vec<u8>, &str, &str); 6] = [
        (
            [extension(0), footer(end + 1, 0)].concat(),
            "header_field",
            "tga.footer.extension_offset",
        ),
        (
            footer(0, end),
            "unsupported_variant",
            "tga.footer.developer_offset",
        ),
        (
            {
                let mut area = extension(0);
                area[..2].copy_from_slice(&494u16.to_le_bytes());
                [area, footer(end, 0)].concat()
            },
            "unsupported_variant",
            "tga.extension.size",
        ),
        (
            {
                let mut area = extension(0);
                area[486] = 1;
                [area, footer(end, 0)].concat()
            },
            "unsupported_variant",
            "tga.extension.postage_stamp_offset",
        ),
        (
            {
                let mut area = extension(0);
                area[482] = 1;
                [area, footer(end, 0)].concat()
            },
            "unsupported_variant",
            "tga.extension.color_correction_offset",
        ),
        (
            // An extension area cut short by the footer.
            [extension(0)[..100].to_vec(), footer(end, 0)].concat(),
            "header_field",
            "tga.extension.size",
        ),
    ];
    for (trailer, code, field) in cases {
        let mut tga = asymmetric(10, RowOrder::BottomUp);
        tga.trailer = trailer;
        let error = read(&tga.bytes()).unwrap_err();
        assert_eq!(error.code(), code, "{field}");
        assert_eq!(error.field(), Some(field));
    }

    // A footer pointing at an extension area that has no bytes at all: the
    // failed read names the absolute offset directly after the pixels.
    let mut tga = asymmetric(10, RowOrder::BottomUp);
    tga.trailer = footer(end, 0);
    let error = read(&tga.bytes()).unwrap_err();
    let TgaError::Parse(parse) = &error else {
        panic!("expected a parse error, got {error:?}");
    };
    assert_eq!(parse.kind, ParseErrorKind::UnexpectedEof);
    assert_eq!(parse.field, "tga.extension.size");
    assert_eq!(parse.offset, u64::from(end));
}

#[test]
fn accept_f08_b_04_other_tga_variants_are_explicitly_unsupported() {
    let base = asymmetric(2, RowOrder::BottomUp).bytes();
    let cases: [(usize, u8, &str); 12] = [
        (1, 1, "tga.color_map_type"),
        (2, 0, "tga.image_type"),
        (2, 1, "tga.image_type"),
        (2, 3, "tga.image_type"),
        (2, 9, "tga.image_type"),
        (2, 11, "tga.image_type"),
        (16, 8, "tga.pixel_depth"),
        (16, 16, "tga.pixel_depth"),
        (17, 0x18, "tga.image_descriptor"),
        (17, 0x48, "tga.image_descriptor"),
        (17, 0x88, "tga.image_descriptor"),
        (17, 0x04, "tga.image_descriptor"),
    ];
    for (at, value, field) in cases {
        let mut bytes = base.clone();
        bytes[at] = value;
        let error = read(&bytes).unwrap_err();
        assert_eq!(error.code(), "unsupported_variant", "{field} = {value}");
        assert_eq!(error.field(), Some(field));
    }
}

#[test]
fn accept_f08_b_04_header_fields_image_id_and_dimensions_are_checked() {
    let base = asymmetric(2, RowOrder::BottomUp).bytes();
    for (at, field) in [
        (3, "tga.color_map_first"),
        (5, "tga.color_map_length"),
        (7, "tga.color_map_entry_bits"),
    ] {
        let mut bytes = base.clone();
        bytes[at] = 1;
        let error = read(&bytes).unwrap_err();
        assert_eq!(error.code(), "header_field", "{field}");
        assert_eq!(error.field(), Some(field));
    }

    // 24 bpp cannot carry alpha bits.
    let mut tga = asymmetric(2, RowOrder::BottomUp);
    tga.pixel_depth = 24;
    tga.pixels.truncate(18);
    let error = read(&tga.bytes()).unwrap_err();
    assert_eq!(error.code(), "header_field");
    assert_eq!(error.field(), Some("tga.image_descriptor"));

    // The image id is skipped with bounds and kept raw.
    let mut tga = asymmetric(10, RowOrder::TopDown);
    tga.image_id = b"id\0\xFFx".to_vec();
    let image = read(&tga.bytes()).unwrap();
    assert_eq!(image.image_id(), b"id\0\xFFx");
    assert_asymmetric(&decode(&image));
    let mut bytes = asymmetric(2, RowOrder::BottomUp).bytes();
    bytes[0] = 200;
    bytes.truncate(18 + 50);
    let error = read(&bytes).unwrap_err();
    assert_eq!(error.code(), "unexpected_eof");
    assert_eq!(error.field(), Some("tga.image_id"));

    // Zero and oversized dimensions are refused before any pixel is read.
    for (width, height) in [(0, 2), (3, 0)] {
        let mut tga = asymmetric(2, RowOrder::BottomUp);
        (tga.width, tga.height) = (width, height);
        let error = read(&tga.bytes()).unwrap_err();
        assert!(
            matches!(
                error,
                TgaError::Descriptor(DescriptorError::ZeroDimension { .. })
            ),
            "{error:?}"
        );
    }
    for (width, height) in [(MAX_DIMENSION as u16 + 1, 2), (3, u16::MAX)] {
        let mut tga = asymmetric(10, RowOrder::BottomUp);
        (tga.width, tga.height) = (width, height);
        assert_eq!(
            read(&tga.bytes()).unwrap_err().code(),
            "dimension_too_large"
        );
    }

    let error = read(&base[..17]).unwrap_err();
    assert!(matches!(&error, TgaError::Parse(p) if p.kind == ParseErrorKind::UnexpectedEof));
}

#[test]
fn accept_f08_b_04_image_id_and_texels_are_charged_to_the_budget() {
    for image_type in [2, 10] {
        let mut tga = asymmetric(image_type, RowOrder::BottomUp);
        tga.image_id = vec![1, 2, 3];
        let bytes = tga.bytes();
        // 3 id bytes, then 6 texels of 4 bytes.
        let needed = 3 + 6 * 4;
        let mut exact = AllocationBudget::new(CONTAINER, needed);
        read_tga(CONTAINER, &bytes, &mut exact).unwrap();
        assert_eq!(exact.used(), needed);

        let mut short = AllocationBudget::new(CONTAINER, needed - 1);
        let error = read_tga(CONTAINER, &bytes, &mut short).unwrap_err();
        assert!(
            matches!(&error, TgaError::Parse(p) if p.kind == ParseErrorKind::AllocationBudgetExceeded),
            "{error:?}"
        );
    }
}
