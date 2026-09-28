//! Acceptance stage F08-B: the variant readers together
//! (`specs/F08-texture-archives-and-conventional-image-decoding.md`, section
//! `### F08-B`, AC02 and non-negotiables #1, #2 and #5).
//!
//! Every file here is built byte by byte in this file from the layouts
//! recorded in the F08-B.02–B.04 findings: newly authored synthetic
//! content, no original game data, no `CS_GAME_DIR` access. Expected texels
//! are written per coordinate, independently of the builders.
//!
//! The per-variant cases (palette index out of range, alpha edges, the
//! non-square mip chains of [`decode_levels`]) are in the stage modules;
//! these tests check what holds across readers: no reader invents mip
//! levels, the multi-level decoder agrees with each reader on the one
//! stored level, and two conventional formats storing the same image
//! decode to the same texels in the same orientation.

use cs_formats::io::AllocationBudget;
use cs_formats::texture::zbd::{
    FLAG_BYTES_PER_PIXEL2, FLAG_FULL_ALPHA, FLAG_HAS_ALPHA, ZBD_TEXTURE_ENTRY_BYTES,
    ZBD_TEXTURE_HEADER_BYTES,
};
use cs_formats::texture::{
    AlphaSource, DecodedFormat, DecodedImage, Extent, ImageDescriptor, RowOrder, TextureError,
    decode_levels, read_bmp, read_tga, read_zbd_textures,
};

const CONTAINER: &str = "synthetic/f08_b_variants";

const RED: [u8; 3] = [0xFF, 0x00, 0x00];
const GREEN: [u8; 3] = [0x00, 0xFF, 0x00];
const BLUE: [u8; 3] = [0x00, 0x00, 0xFF];
const YELLOW: [u8; 3] = [0xFF, 0xFF, 0x00];
const CYAN: [u8; 3] = [0x00, 0xFF, 0xFF];
const MAGENTA: [u8; 3] = [0xFF, 0x00, 0xFF];
const GREY: [u8; 3] = [0x80, 0x40, 0x20];

/// The intended 3x2 image, as `(x from the left, y from the top, color)`.
///
/// ```text
///        x=0     x=1    x=2
/// y=0    red     green  blue
/// y=1    yellow  cyan   magenta
/// ```
const EXPECTED_3X2: [(u32, u32, [u8; 3]); 6] = [
    (0, 0, RED),
    (1, 0, GREEN),
    (2, 0, BLUE),
    (0, 1, YELLOW),
    (1, 1, CYAN),
    (2, 1, MAGENTA),
];

fn budget() -> AllocationBudget {
    AllocationBudget::with_defaults(CONTAINER)
}

/// A one-texture ZBD package: the 3x2 image as indices into a 565
/// palette, with a full alpha plane whose corners are 0 and 255.
fn zbd_package() -> Vec<u8> {
    let mut out = Vec::new();
    for word in [0u32, 1, 0, 1, 0, 0] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    let mut name = [0u8; 32];
    name[..4].copy_from_slice(b"tex1");
    out.extend_from_slice(&name);
    let start = (ZBD_TEXTURE_HEADER_BYTES + ZBD_TEXTURE_ENTRY_BYTES) as u32;
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&(-1i32).to_le_bytes());
    let flags = FLAG_BYTES_PER_PIXEL2 | FLAG_HAS_ALPHA | FLAG_FULL_ALPHA;
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&3u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    let palette: [u16; 3] = [0x001F, 0x07E0, 0xF800];
    out.extend_from_slice(&(palette.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&[2, 1, 0, 0, 1, 2]);
    out.extend_from_slice(&[0, 0x11, 0xFF, 0xFF, 0x80, 0]);
    for word in palette {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out
}

/// A palette in an order unrelated to the image, so no index equals its
/// texel position; entry 2 is never used by the image.
const BMP_PALETTE: [[u8; 3]; 7] = [MAGENTA, BLUE, GREY, YELLOW, RED, CYAN, GREEN];

/// A `BI_RGB` BMP of the 3x2 image. `height` is 2 (bottom-up) or -2
/// (top-down); `rows` are the stored rows, padding included.
fn bmp(bits_per_pixel: u16, height: i32, rows: [[u8; 4]; 2]) -> Vec<u8> {
    let pixels = rows.concat();
    let pixel_offset = 14 + 40 + 4 * BMP_PALETTE.len() as u32;
    let mut out = b"BM".to_vec();
    out.extend_from_slice(&(pixel_offset + pixels.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&pixel_offset.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&3i32.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&bits_per_pixel.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(pixels.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&(BMP_PALETTE.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for [r, g, b] in BMP_PALETTE {
        out.extend_from_slice(&[b, g, r, 0]);
    }
    out.extend_from_slice(&pixels);
    out
}

/// 8 bpp, bottom-up: indices `3 5 0` (bottom row) stored first.
fn bmp_8bpp_bottom_up() -> Vec<u8> {
    bmp(8, 2, [[3, 5, 0, 0], [4, 6, 1, 0]])
}

/// 4 bpp, top-down: high nibble is the left texel, `4 6 | 1 _` then
/// `3 5 | 0 _`.
fn bmp_4bpp_top_down() -> Vec<u8> {
    bmp(4, -2, [[0x46, 0x10, 0, 0], [0x35, 0x00, 0, 0]])
}

/// A 24 bpp TGA of the 3x2 image with no alpha bits. `pixels` are the
/// stored texels (type 2) or packets (type 10).
fn tga(image_type: u8, image_descriptor: u8, pixels: &[u8]) -> Vec<u8> {
    let mut out = vec![0, 0, image_type, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    out.extend_from_slice(&3u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&[24, image_descriptor]);
    out.extend_from_slice(pixels);
    out
}

/// Stored blue, green, red texels of `colors`.
fn bgr(colors: &[[u8; 3]]) -> Vec<u8> {
    colors.iter().flat_map(|&[r, g, b]| [b, g, r]).collect()
}

/// Type 2, top-left origin (descriptor bit 5): top row stored first.
fn tga_uncompressed_top_down() -> Vec<u8> {
    tga(2, 0x20, &bgr(&[RED, GREEN, BLUE, YELLOW, CYAN, MAGENTA]))
}

/// Type 10, bottom-left origin: one raw packet of six texels, bottom row
/// stored first.
fn tga_rle_bottom_up() -> Vec<u8> {
    let mut pixels = vec![0x05];
    pixels.extend(bgr(&[YELLOW, CYAN, MAGENTA, RED, GREEN, BLUE]));
    tga(10, 0x00, &pixels)
}

/// Decodes the one stored level through [`decode_levels`], checks it
/// equals the reader's own decode, and checks that a second level is
/// refused because none is declared.
fn assert_single_level(
    descriptor: &ImageDescriptor,
    stored: &[u8],
    from_reader: &DecodedImage,
) -> DecodedImage {
    assert!(descriptor.mips().is_empty(), "no reader declares mips");
    let levels = decode_levels(CONTAINER, descriptor, &[stored], &mut budget())
        .expect("the stored level decodes");
    assert_eq!(levels.levels().len(), 1);
    assert!(levels.mips().is_empty());
    assert_eq!(levels.base(), from_reader);

    let error = decode_levels(CONTAINER, descriptor, &[stored, stored], &mut budget())
        .expect_err("an undeclared mip level");
    assert_eq!(
        error,
        TextureError::LevelCountMismatch {
            container: CONTAINER.to_owned(),
            expected: 1,
            observed: 2,
        }
    );
    levels.base().clone()
}

#[test]
fn accept_f08_b_readers_declare_only_the_stored_level_and_decode_levels_agrees() {
    let bytes = zbd_package();
    let package = read_zbd_textures(CONTAINER, &bytes, &mut budget()).expect("a valid package");
    let texture = &package.textures()[0];
    let image = texture.decode(&mut budget()).expect("decodes");
    let image = assert_single_level(texture.descriptor(), texture.stored(), &image);
    assert_eq!(image.extent(), Extent::new(3, 2));
    assert_eq!(image.texel565(0, 0), Some(0xF800));
    assert_eq!(image.texel565(2, 1), Some(0xF800));
    assert_eq!(image.alpha_source(), AlphaSource::Plane);
    assert_eq!(image.alpha_at(0, 0), Some(0));
    assert_eq!(image.alpha_at(2, 0), Some(0xFF));
    assert_eq!(image.alpha_at(2, 1), Some(0));

    let bytes = bmp_8bpp_bottom_up();
    let bmp = read_bmp(CONTAINER, &bytes, &mut budget()).expect("a valid BMP");
    let image = bmp.decode(&mut budget()).expect("decodes");
    let image = assert_single_level(bmp.descriptor(), bmp.stored_indices(), &image);
    assert_eq!(image.texel(0, 0), Some(&RED[..]));

    let bytes = tga_rle_bottom_up();
    let tga = read_tga(CONTAINER, &bytes, &mut budget()).expect("a valid TGA");
    let image = tga.decode(&mut budget()).expect("decodes");
    let image = assert_single_level(tga.descriptor(), tga.stored_texels(), &image);
    assert_eq!(image.texel(0, 0), Some(&RED[..]));
}

#[test]
fn accept_f08_b_bmp_and_tga_of_the_same_image_decode_to_identical_texels() {
    let mut decoded = Vec::new();
    for (name, bytes) in [
        ("bmp 8 bpp bottom-up", bmp_8bpp_bottom_up()),
        ("bmp 4 bpp top-down", bmp_4bpp_top_down()),
    ] {
        let image = read_bmp(CONTAINER, &bytes, &mut budget()).expect(name);
        let expected_order = if name.ends_with("top-down") {
            RowOrder::TopDown
        } else {
            RowOrder::BottomUp
        };
        assert_eq!(image.descriptor().row_order(), expected_order, "{name}");
        decoded.push((name, image.decode(&mut budget()).expect(name)));
    }
    for (name, bytes) in [
        ("tga type 2 top-down", tga_uncompressed_top_down()),
        ("tga type 10 bottom-up", tga_rle_bottom_up()),
    ] {
        let image = read_tga(CONTAINER, &bytes, &mut budget()).expect(name);
        let expected_order = if name.ends_with("top-down") {
            RowOrder::TopDown
        } else {
            RowOrder::BottomUp
        };
        assert_eq!(image.descriptor().row_order(), expected_order, "{name}");
        decoded.push((name, image.decode(&mut budget()).expect(name)));
    }

    for (name, image) in &decoded {
        assert_eq!(image.extent(), Extent::new(3, 2), "{name}");
        assert_eq!(image.format(), DecodedFormat::Rgb8, "{name}");
        assert_eq!(image.alpha_source(), AlphaSource::Opaque, "{name}");
        for (x, y, color) in EXPECTED_3X2 {
            assert_eq!(image.texel(x, y), Some(&color[..]), "{name}: ({x}, {y})");
        }
    }
    // Channel bytes compared independently of how each file stores them.
    let (first, rest) = decoded.split_first().unwrap();
    for (name, image) in rest {
        assert_eq!(image.texels(), first.1.texels(), "{name} vs {}", first.0);
    }
}
