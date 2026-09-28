//! Acceptance stage F08-B.03: conventional 4 and 8 bpp `BI_RGB` BMPs, read
//! by content rather than by name
//! (`specs/F08-texture-archives-and-conventional-image-decoding.md`, section
//! `### F08-B`, non-negotiables #1, #2 and #5).
//!
//! Every file here is built byte by byte in this file from the published
//! BMP layout recorded in `docs/findings/2026-09-28-f08-b-03-conventional-bmp.md`:
//! newly authored synthetic content, no original game data, no
//! `CS_GAME_DIR` access. Expected texels are written per coordinate,
//! independently of the builders, and the 4 bpp pixel rows are written out
//! as literal bytes so the nibble order is not taken from the reader.

use cs_formats::ParseErrorKind;
use cs_formats::io::AllocationBudget;
use cs_formats::texture::bmp::{BMP_FILE_HEADER_BYTES, BMP_INFO_HEADER_BYTES};
use cs_formats::texture::{
    AlphaSource, AlphaTest, BmpError, BmpImage, ColorSpace, DecodedFormat, DescriptorError, Extent,
    MAX_DIMENSION, Palette, PaletteEntry, PixelFormat, RowOrder, TextureError, looks_like_bmp,
    read_bmp,
};

const CONTAINER: &str = "synthetic/f08_b_03_asymmetric.bmp";

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

/// A palette in an order unrelated to the image, so no index equals its
/// texel position; entry 2 is never used by the image.
const PALETTE: [[u8; 3]; 7] = [MAGENTA, BLUE, GREY, YELLOW, RED, CYAN, GREEN];

/// Indices into [`PALETTE`] for [`EXPECTED_3X2`]: top row, bottom row.
const TOP: [u8; 3] = [4, 6, 1];
const BOTTOM: [u8; 3] = [3, 5, 0];

/// 8 bpp rows of the 3x2 image: three indices and one padding byte.
const ROW8_TOP: [u8; 4] = [4, 6, 1, 0];
const ROW8_BOTTOM: [u8; 4] = [3, 5, 0, 0];

/// 4 bpp rows of the 3x2 image, high nibble = left texel: `4 6 | 1 _`, then
/// two padding bytes (three nibbles round up to two bytes, then to four).
const ROW4_TOP: [u8; 4] = [0x46, 0x10, 0x00, 0x00];
const ROW4_BOTTOM: [u8; 4] = [0x35, 0x00, 0x00, 0x00];

/// One authored BMP.
struct Bmp {
    bits_per_pixel: u16,
    width: i32,
    height: i32,
    colors_used: u32,
    colors_important: u32,
    /// Color table entries as red, green, blue, reserved.
    table: Vec<[u8; 4]>,
    /// Stored rows, padding included, in stored order.
    rows: Vec<Vec<u8>>,
}

impl Bmp {
    fn new(bits_per_pixel: u16, width: i32, height: i32, rows: &[&[u8]]) -> Self {
        Self {
            bits_per_pixel,
            width,
            height,
            colors_used: PALETTE.len() as u32,
            colors_important: 0,
            table: PALETTE.iter().map(|&[r, g, b]| [r, g, b, 0]).collect(),
            rows: rows.iter().map(|row| row.to_vec()).collect(),
        }
    }

    fn pixel_offset(&self) -> u32 {
        BMP_FILE_HEADER_BYTES + BMP_INFO_HEADER_BYTES + 4 * self.table.len() as u32
    }

    fn bytes(&self) -> Vec<u8> {
        let pixels: Vec<u8> = self.rows.concat();
        let file_size = self.pixel_offset() + pixels.len() as u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&file_size.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&self.pixel_offset().to_le_bytes());
        out.extend_from_slice(&BMP_INFO_HEADER_BYTES.to_le_bytes());
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&self.height.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&self.bits_per_pixel.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(pixels.len() as u32).to_le_bytes());
        out.extend_from_slice(&2835i32.to_le_bytes());
        out.extend_from_slice(&(-7i32).to_le_bytes());
        out.extend_from_slice(&self.colors_used.to_le_bytes());
        out.extend_from_slice(&self.colors_important.to_le_bytes());
        for &[r, g, b, reserved] in &self.table {
            out.extend_from_slice(&[b, g, r, reserved]);
        }
        out.extend_from_slice(&pixels);
        out
    }
}

/// The 3x2 image at `bits_per_pixel`, stored in `order`.
fn asymmetric(bits_per_pixel: u16, order: RowOrder) -> Bmp {
    let (top, bottom): (&[u8], &[u8]) = match bits_per_pixel {
        8 => (&ROW8_TOP, &ROW8_BOTTOM),
        4 => (&ROW4_TOP, &ROW4_BOTTOM),
        _ => unreachable!(),
    };
    match order {
        RowOrder::BottomUp => Bmp::new(bits_per_pixel, 3, 2, &[bottom, top]),
        RowOrder::TopDown => Bmp::new(bits_per_pixel, 3, -2, &[top, bottom]),
    }
}

fn budget() -> AllocationBudget {
    AllocationBudget::with_defaults(CONTAINER)
}

fn read(bytes: &[u8]) -> Result<BmpImage, BmpError> {
    read_bmp(CONTAINER, bytes, &mut budget())
}

fn put_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn assert_asymmetric(image: &BmpImage) {
    let decoded = image.decode(&mut budget()).expect("decodes");
    assert_eq!(decoded.extent(), Extent::new(3, 2));
    assert_eq!(decoded.format(), DecodedFormat::Rgb8);
    for (x, y, color) in EXPECTED_3X2 {
        assert_eq!(decoded.texel(x, y), Some(&color[..]), "texel ({x}, {y})");
    }
    assert_eq!(decoded.indices(), Some(&[TOP, BOTTOM].concat()[..]));
    assert_eq!(decoded.alpha_source(), AlphaSource::Opaque);
}

#[test]
fn accept_f08_b_03_asymmetric_3x2_8bpp_decodes_every_texel_in_place_in_both_row_orders() {
    for order in [RowOrder::BottomUp, RowOrder::TopDown] {
        let bmp = asymmetric(8, order);
        let image = read(&bmp.bytes()).unwrap_or_else(|e| panic!("{order:?}: {e}"));
        assert_eq!(image.bits_per_pixel(), 8);
        let descriptor = image.descriptor();
        assert_eq!(descriptor.extent(), Extent::new(3, 2));
        assert_eq!(descriptor.format(), PixelFormat::Indexed8);
        assert_eq!(descriptor.row_order(), order);
        assert_eq!(descriptor.mips(), &[]);
        assert_eq!(descriptor.alpha_test(), AlphaTest::Unknown);
        assert_eq!(descriptor.color_space(), ColorSpace::Unknown);
        assert_eq!(image.pixel_offset(), bmp.pixel_offset());
        assert_eq!(image.pixels_per_meter(), (2835, -7));
        // Padding is dropped from the stored level, rows stay in stored order.
        let stored = match order {
            RowOrder::BottomUp => [BOTTOM, TOP].concat(),
            RowOrder::TopDown => [TOP, BOTTOM].concat(),
        };
        assert_eq!(image.stored_indices(), &stored[..]);
        assert_asymmetric(&image);
    }
}

#[test]
fn accept_f08_b_03_asymmetric_3x2_4bpp_odd_width_puts_the_high_nibble_on_the_left() {
    for order in [RowOrder::BottomUp, RowOrder::TopDown] {
        let image = read(&asymmetric(4, order).bytes()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(image.bits_per_pixel(), 4);
        assert_eq!(image.descriptor().row_order(), order);
        assert_eq!(image.descriptor().format(), PixelFormat::Indexed8);
        assert_asymmetric(&image);
    }
}

#[test]
fn accept_f08_b_03_non_square_2x3_and_even_4bpp_width_decode_in_place() {
    // 2 columns by 3 rows, 8 bpp bottom-up: rows of 2 indices + 2 padding.
    //   y=0: red green, y=1: blue yellow, y=2: cyan magenta
    let bmp = Bmp::new(8, 2, 3, &[&[5, 0, 0, 0], &[1, 3, 0, 0], &[4, 6, 0, 0]]);
    let decoded = read(&bmp.bytes()).unwrap().decode(&mut budget()).unwrap();
    let expected = [
        (0, 0, RED),
        (1, 0, GREEN),
        (0, 1, BLUE),
        (1, 1, YELLOW),
        (0, 2, CYAN),
        (1, 2, MAGENTA),
    ];
    for (x, y, color) in expected {
        assert_eq!(decoded.texel(x, y), Some(&color[..]), "texel ({x}, {y})");
    }

    // 4x1, 4 bpp: two full bytes then two padding bytes.
    let bmp = Bmp::new(4, 4, 1, &[&[0x46, 0x13, 0x00, 0x00]]);
    let decoded = read(&bmp.bytes()).unwrap().decode(&mut budget()).unwrap();
    assert_eq!(decoded.indices(), Some(&[4, 6, 1, 3][..]));
    assert_eq!(decoded.texel(3, 0), Some(&YELLOW[..]));
}

#[test]
fn accept_f08_b_03_color_table_is_bgrx_and_the_reserved_byte_is_not_alpha() {
    let mut bmp = asymmetric(8, RowOrder::BottomUp);
    // Reserved bytes 0x00, 0x80 and 0xFF: none of them changes anything.
    for (index, entry) in bmp.table.iter_mut().enumerate() {
        entry[3] = [0x00, 0x80, 0xFF][index % 3];
    }
    let image = read(&bmp.bytes()).unwrap();
    let Some(Palette::Rgb8(entries)) = image.descriptor().palette() else {
        panic!("expected an rgb8 palette");
    };
    let expected: Vec<PaletteEntry> = PALETTE
        .iter()
        .map(|&[r, g, b]| PaletteEntry::new(r, g, b))
        .collect();
    assert_eq!(entries, &expected);
    assert_eq!(image.descriptor().alpha_source(), AlphaSource::Opaque);
    assert_asymmetric(&image);
    assert_eq!(image.decode(&mut budget()).unwrap().alpha(), None);
}

#[test]
fn accept_f08_b_03_colors_used_zero_means_the_full_table() {
    for (bits, rows) in [(4u16, &ROW4_TOP), (8, &ROW8_TOP)] {
        let mut bmp = Bmp::new(bits, 3, 1, &[rows]);
        bmp.colors_used = 0;
        let full = 1usize << bits;
        bmp.table.resize(full, [0x11, 0x22, 0x33, 0]);
        let image = read(&bmp.bytes()).unwrap();
        assert_eq!(image.colors_used(), 0);
        assert_eq!(image.descriptor().palette().map(Palette::len), Some(full));
        let decoded = image.decode(&mut budget()).unwrap();
        assert_eq!(decoded.texel(0, 0), Some(&RED[..]));
        assert_eq!(decoded.texel(2, 0), Some(&BLUE[..]));
    }
}

#[test]
fn accept_f08_b_03_index_beyond_the_color_table_is_rejected_with_coordinates() {
    // 8 bpp, bottom-up: canonical (2, 0) is stored row 1, byte 2.
    let mut bmp = asymmetric(8, RowOrder::BottomUp);
    bmp.rows[1][2] = 7;
    let offset = u64::from(bmp.pixel_offset()) + 4 + 2;
    let error = read(&bmp.bytes()).unwrap_err();
    assert_eq!(error.code(), "palette_index_out_of_range");
    assert_eq!(
        error,
        BmpError::Pixels(TextureError::PaletteIndexOutOfRange {
            container: CONTAINER.to_owned(),
            offset,
            x: 2,
            y: 0,
            index: 7,
            entries: 7,
        })
    );

    // The last valid index is accepted.
    bmp.rows[1][2] = 6;
    assert!(read(&bmp.bytes()).is_ok());

    // 4 bpp, top-down: canonical (1, 1) is the low nibble of stored row 1,
    // byte 0.
    let mut bmp = asymmetric(4, RowOrder::TopDown);
    bmp.rows[1][0] = 0x3F;
    let offset = u64::from(bmp.pixel_offset()) + 4;
    let error = read(&bmp.bytes()).unwrap_err();
    assert_eq!(
        error,
        BmpError::Pixels(TextureError::PaletteIndexOutOfRange {
            container: CONTAINER.to_owned(),
            offset,
            x: 1,
            y: 1,
            index: 15,
            entries: 7,
        })
    );
}

#[test]
fn accept_f08_b_03_padding_bits_must_be_zero() {
    // The unused low nibble of an odd 4 bpp width.
    let mut bmp = asymmetric(4, RowOrder::BottomUp);
    bmp.rows[0][1] = 0x01;
    let error = read(&bmp.bytes()).unwrap_err();
    assert_eq!(error.code(), "nonzero_padding");
    assert_eq!(
        error,
        BmpError::Padding {
            container: CONTAINER.to_owned(),
            offset: u64::from(bmp.pixel_offset()) + 1,
            y: 1,
            value: 0x01,
        }
    );

    // A row padding byte, 8 bpp and 4 bpp.
    for bits in [8, 4] {
        let mut bmp = asymmetric(bits, RowOrder::TopDown);
        bmp.rows[1][3] = 0xAA;
        let error = read(&bmp.bytes()).unwrap_err();
        assert_eq!(
            error,
            BmpError::Padding {
                container: CONTAINER.to_owned(),
                offset: u64::from(bmp.pixel_offset()) + 4 + 3,
                y: 1,
                value: 0xAA,
            },
            "{bits} bpp"
        );
    }
}

#[test]
fn accept_f08_b_03_truncated_or_padded_pixel_data_is_rejected() {
    let bytes = asymmetric(8, RowOrder::BottomUp).bytes();

    // One byte short, file size left as stored: the header disagrees.
    let error = read(&bytes[..bytes.len() - 1]).unwrap_err();
    assert_eq!(error.code(), "header_field");
    assert_eq!(error.field(), Some("bmp.file_size"));

    // One byte short with a matching file size: the last row is missing.
    let mut short = bytes[..bytes.len() - 1].to_vec();
    let len = short.len() as u32;
    put_u32(&mut short, 2, len);
    put_u32(&mut short, 34, 0);
    let error = read(&short).unwrap_err();
    let BmpError::Parse(parse) = &error else {
        panic!("expected a parse error, got {error:?}");
    };
    assert_eq!(parse.kind, ParseErrorKind::UnexpectedEof);
    assert_eq!(parse.field, "bmp.pixels");

    // One byte after the last row, with a matching file size.
    let mut long = bytes.clone();
    long.push(0);
    let len = long.len() as u32;
    put_u32(&mut long, 2, len);
    let error = read(&long).unwrap_err();
    assert_eq!(
        error,
        BmpError::TrailingBytes {
            container: CONTAINER.to_owned(),
            expected_end: bytes.len() as u64,
            observed_len: long.len() as u64,
        }
    );

    // A stored image size other than 0 or the exact padded size.
    let mut sized = bytes.clone();
    put_u32(&mut sized, 34, 6);
    let error = read(&sized).unwrap_err();
    assert_eq!(error.field(), Some("bmp.info.image_size"));
    put_u32(&mut sized, 34, 0);
    assert!(read(&sized).is_ok());
}

#[test]
fn accept_f08_b_03_the_name_does_not_decide_the_format() {
    let bytes = asymmetric(4, RowOrder::BottomUp).bytes();
    assert!(looks_like_bmp(&bytes));
    let names = [
        "synthetic/picture.bmp",
        "synthetic/00000409.016",
        "synthetic/00000409.256",
    ];
    let decoded: Vec<_> = names
        .iter()
        .map(|name| {
            let image = read_bmp(name, &bytes, &mut budget()).unwrap();
            assert_eq!(image.container(), *name);
            (
                image.descriptor().clone(),
                image.decode(&mut budget()).unwrap(),
            )
        })
        .collect();
    assert!(decoded.windows(2).all(|pair| pair[0] == pair[1]));

    // Without the signature a `.bmp` name does not help.
    let mut other = bytes.clone();
    other[0] = b'Z';
    assert!(!looks_like_bmp(&other));
    let error = read_bmp("synthetic/picture.bmp", &other, &mut budget()).unwrap_err();
    assert_eq!(error.code(), "not_bmp");
}

#[test]
fn accept_f08_b_03_other_bmp_variants_are_explicitly_unsupported() {
    let base = asymmetric(8, RowOrder::BottomUp).bytes();
    let cases: [(usize, u32, &str, usize); 8] = [
        (14, 12, "bmp.info.header_size", 4),
        (14, 108, "bmp.info.header_size", 4),
        (30, 1, "bmp.info.compression", 4),
        (30, 3, "bmp.info.compression", 4),
        (28, 1, "bmp.info.bits_per_pixel", 2),
        (28, 16, "bmp.info.bits_per_pixel", 2),
        (28, 24, "bmp.info.bits_per_pixel", 2),
        (28, 32, "bmp.info.bits_per_pixel", 2),
    ];
    for (at, value, field, width) in cases {
        let mut bytes = base.clone();
        if width == 2 {
            put_u16(&mut bytes, at, value as u16);
        } else {
            put_u32(&mut bytes, at, value);
        }
        let error = read(&bytes).unwrap_err();
        assert_eq!(error.code(), "unsupported_variant", "{field} = {value}");
        assert_eq!(error.field(), Some(field));
    }

    // More colors than the bit depth can address.
    for (bits, colors) in [(4u16, 17u32), (8, 257)] {
        let mut bmp = asymmetric(bits, RowOrder::BottomUp);
        bmp.colors_used = colors;
        bmp.table.resize(colors as usize, [0, 0, 0, 0]);
        let error = read(&bmp.bytes()).unwrap_err();
        assert_eq!(error.code(), "unsupported_variant");
        assert_eq!(error.field(), Some("bmp.info.colors_used"));
    }
}

#[test]
fn accept_f08_b_03_header_fields_and_dimensions_are_checked() {
    let bmp = asymmetric(8, RowOrder::BottomUp);
    let base = bmp.bytes();
    let bad_u16 = [
        (6, "bmp.reserved1", 1),
        (8, "bmp.reserved2", 1),
        (26, "bmp.info.planes", 2),
    ];
    for (at, field, value) in bad_u16 {
        let mut bytes = base.clone();
        put_u16(&mut bytes, at, value);
        let error = read(&bytes).unwrap_err();
        assert_eq!(error.code(), "header_field", "{field}");
        assert_eq!(error.field(), Some(field));
    }
    for offset in [bmp.pixel_offset() - 1, bmp.pixel_offset() + 1] {
        let mut bytes = base.clone();
        put_u32(&mut bytes, 10, offset);
        assert_eq!(read(&bytes).unwrap_err().field(), Some("bmp.pixel_offset"));
    }
    let mut bytes = base.clone();
    put_u32(&mut bytes, 50, 8);
    assert_eq!(
        read(&bytes).unwrap_err().field(),
        Some("bmp.info.colors_important")
    );
    put_u32(&mut bytes, 50, 7);
    assert_eq!(read(&bytes).unwrap().colors_important(), 7);

    let mut bytes = base.clone();
    put_u32(&mut bytes, 18, (-3i32) as u32);
    assert_eq!(read(&bytes).unwrap_err().field(), Some("bmp.info.width"));

    // Zero and oversized dimensions are refused before any row is read.
    for (at, value) in [(18, 0i32), (22, 0)] {
        let mut bytes = base.clone();
        put_u32(&mut bytes, at, value as u32);
        let error = read(&bytes).unwrap_err();
        assert!(
            matches!(
                error,
                BmpError::Descriptor(DescriptorError::ZeroDimension { .. })
            ),
            "{error:?}"
        );
    }
    for (at, value) in [
        (18, MAX_DIMENSION as i32 + 1),
        (22, MAX_DIMENSION as i32 + 1),
        (22, -(MAX_DIMENSION as i32) - 1),
        (22, i32::MIN),
    ] {
        let mut bytes = base.clone();
        put_u32(&mut bytes, at, value as u32);
        let error = read(&bytes).unwrap_err();
        assert_eq!(error.code(), "dimension_too_large", "{value}");
    }

    let error = read(b"BM").unwrap_err();
    assert!(matches!(&error, BmpError::Parse(p) if p.kind == ParseErrorKind::UnexpectedEof));
}

#[test]
fn accept_f08_b_03_palette_and_indices_are_charged_to_the_budget() {
    let bytes = asymmetric(4, RowOrder::BottomUp).bytes();
    // 7 palette entries of 3 bytes, then 6 index bytes.
    let needed = 7 * 3 + 6;
    let mut exact = AllocationBudget::new(CONTAINER, needed);
    read_bmp(CONTAINER, &bytes, &mut exact).unwrap();
    assert_eq!(exact.used(), needed);

    let mut short = AllocationBudget::new(CONTAINER, needed - 1);
    let error = read_bmp(CONTAINER, &bytes, &mut short).unwrap_err();
    assert!(
        matches!(&error, BmpError::Parse(p) if p.kind == ParseErrorKind::AllocationBudgetExceeded),
        "{error:?}"
    );
}
