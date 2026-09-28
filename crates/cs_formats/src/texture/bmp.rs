//! Conventional Windows BMP images, 4 and 8 bits per pixel, stage F08-B.03.
//!
//! Layout source: the published Windows bitmap format (`BITMAPFILEHEADER`,
//! `BITMAPINFOHEADER`, `RGBQUAD`). A read-only survey of the installation
//! found exactly two BMPs, `00000409.016` (4 bpp) and `00000409.256`
//! (8 bpp), both 640x480, bottom-up, `BI_RGB`, with the default color
//! count. Their extensions say nothing about the format, so a BMP is
//! recognised by its `BM` signature and header ([`looks_like_bmp`]), never
//! by its name. Details and recorded unknowns:
//! `docs/findings/2026-09-28-f08-b-03-conventional-bmp.md`.
//!
//! All words are little-endian.
//!
//! ```text
//! file header (14)  "BM", u32 file_size, u16 0, u16 0, u32 pixel_offset
//! info header (40)  u32 40, i32 width, i32 height, u16 planes (1),
//!                   u16 bits_per_pixel (4 | 8), u32 compression (0 = BI_RGB),
//!                   u32 image_size (0 or exact), i32 x_pixels_per_meter,
//!                   i32 y_pixels_per_meter, u32 colors_used,
//!                   u32 colors_important
//! color table       colors (colors_used, or 2^bits_per_pixel when 0) x 4:
//!                   blue, green, red, reserved
//! pixels            at pixel_offset, |height| rows of
//!                   ceil(width * bits_per_pixel / 32) * 4 bytes;
//!                   positive height: bottom row first,
//!                   negative height: top row first
//! end               the file ends after the last row
//! ```
//!
//! Only this variant is read. Other header sizes, compression, bit depths
//! and color counts beyond `2^bits_per_pixel` fail with
//! [`BmpError::Unsupported`] rather than being guessed. The reserved byte of
//! a color table entry is not alpha: the image is described as
//! [`AlphaSource::Opaque`] and the byte is ignored.
//!
//! [`read_bmp`] validates everything, including every palette index and
//! every padding bit, and unpacks the stored rows into one index byte per
//! texel in stored row order; [`BmpImage::decode`] hands them to
//! [`decode_base_level`], which is the one place the row order is applied.
//! A 4 bpp byte holds two texels, the high nibble being the left one.

use std::fmt;

use crate::error::ParseError;
use crate::io::{AllocationBudget, Reader};

use super::decode::{DecodedImage, TextureError, decode_base_level};
use super::descriptor::{
    AlphaSource, AlphaTest, ColorSpace, DescriptorError, DescriptorParts, Extent, ImageDescriptor,
    Palette, PaletteEntry, PixelFormat, RowOrder,
};

/// The two signature bytes every BMP starts with.
pub const BMP_SIGNATURE: [u8; 2] = *b"BM";
/// Bytes of `BITMAPFILEHEADER`.
pub const BMP_FILE_HEADER_BYTES: u32 = 14;
/// Bytes of `BITMAPINFOHEADER`, the only info header size read here.
pub const BMP_INFO_HEADER_BYTES: u32 = 40;
/// Bytes of one color table entry (`RGBQUAD`).
pub const BMP_COLOR_ENTRY_BYTES: u32 = 4;
/// `BI_RGB`: uncompressed, the only compression read here.
pub const BMP_BI_RGB: u32 = 0;

/// Whether `bytes` start with the BMP signature. A name or extension is
/// never consulted; [`read_bmp`] still validates the whole header.
pub fn looks_like_bmp(bytes: &[u8]) -> bool {
    bytes.starts_with(&BMP_SIGNATURE)
}

/// A read BMP: its header facts, its validated descriptor and its palette
/// indices unpacked to one byte per texel, in stored row order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BmpImage {
    container: String,
    bits_per_pixel: u16,
    colors_used: u32,
    colors_important: u32,
    pixels_per_meter: (i32, i32),
    pixel_offset: u32,
    descriptor: ImageDescriptor,
    indices: Vec<u8>,
}

impl BmpImage {
    /// Provenance label used in errors.
    pub fn container(&self) -> &str {
        &self.container
    }

    /// Stored bits per pixel: 4 or 8.
    pub fn bits_per_pixel(&self) -> u16 {
        self.bits_per_pixel
    }

    /// The stored `colors_used` word (0 means `2^bits_per_pixel`).
    pub fn colors_used(&self) -> u32 {
        self.colors_used
    }

    /// The stored `colors_important` word, kept raw.
    pub fn colors_important(&self) -> u32 {
        self.colors_important
    }

    /// The stored horizontal and vertical resolution, kept raw.
    pub fn pixels_per_meter(&self) -> (i32, i32) {
        self.pixels_per_meter
    }

    /// Byte offset of the first stored row.
    pub fn pixel_offset(&self) -> u32 {
        self.pixel_offset
    }

    /// The validated description: [`PixelFormat::Indexed8`] with the color
    /// table as a [`Palette::Rgb8`], the stored row order, no mips.
    pub fn descriptor(&self) -> &ImageDescriptor {
        &self.descriptor
    }

    /// One palette index per texel, rows in stored order without padding,
    /// columns left to right.
    pub fn stored_indices(&self) -> &[u8] {
        &self.indices
    }

    /// Decodes the image, charging `budget` for the decoded buffers.
    pub fn decode(&self, budget: &mut AllocationBudget) -> Result<DecodedImage, TextureError> {
        decode_base_level(&self.container, &self.descriptor, &self.indices, budget)
    }
}

/// Why a BMP was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BmpError {
    /// A checked read or the allocation budget failed: a truncated header,
    /// color table or row, or a decoded size beyond the budget.
    Parse(ParseError),
    /// The input does not start with `BM`.
    NotBmp {
        /// Provenance label.
        container: String,
        /// The first two bytes.
        signature: [u8; 2],
    },
    /// A header word differs from what the format requires.
    HeaderField {
        /// Provenance label.
        container: String,
        /// Absolute offset of the word.
        offset: u64,
        /// The field.
        field: &'static str,
        /// The required value or range.
        expected: String,
        /// The stored value.
        observed: i64,
    },
    /// A valid BMP feature this reader does not implement.
    Unsupported {
        /// Provenance label.
        container: String,
        /// Absolute offset of the word.
        offset: u64,
        /// The field.
        field: &'static str,
        /// What is supported.
        supported: &'static str,
        /// The stored value.
        observed: i64,
    },
    /// The size, row order or palette is not a valid descriptor (zero or
    /// oversized dimension, ...).
    Descriptor(DescriptorError),
    /// A non-zero bit after the last texel of a row: the unused low nibble
    /// of an odd 4 bpp width or a row padding byte.
    Padding {
        /// Provenance label.
        container: String,
        /// Absolute offset of the byte.
        offset: u64,
        /// Canonical row (from the top) it belongs to.
        y: u32,
        /// The stored byte.
        value: u8,
    },
    /// Bytes after the last row.
    TrailingBytes {
        /// Provenance label.
        container: String,
        /// Where the last row ends.
        expected_end: u64,
        /// The input length.
        observed_len: u64,
    },
    /// A stored palette index beyond the color table, with the absolute
    /// offset of its byte and its canonical texel.
    Pixels(TextureError),
}

impl BmpError {
    /// Stable machine-matchable identifier.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Parse(error) => error.kind.as_str(),
            Self::NotBmp { .. } => "not_bmp",
            Self::HeaderField { .. } => "header_field",
            Self::Unsupported { .. } => "unsupported_variant",
            Self::Descriptor(error) => error.code(),
            Self::Padding { .. } => "nonzero_padding",
            Self::TrailingBytes { .. } => "trailing_bytes",
            Self::Pixels(error) => error.code(),
        }
    }

    /// The header field an error names, if it names one.
    pub fn field(&self) -> Option<&str> {
        match self {
            Self::Parse(error) => Some(&error.field),
            Self::HeaderField { field, .. } | Self::Unsupported { field, .. } => Some(field),
            _ => None,
        }
    }
}

impl From<ParseError> for BmpError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl fmt::Display for BmpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(f),
            Self::NotBmp {
                container,
                signature,
            } => write!(f, "{container}: signature {signature:02x?} is not \"BM\""),
            Self::HeaderField {
                container,
                offset,
                field,
                expected,
                observed,
            } => write!(
                f,
                "{container}: {field} at offset {offset} is {observed}, expected {expected}"
            ),
            Self::Unsupported {
                container,
                offset,
                field,
                supported,
                observed,
            } => write!(
                f,
                "{container}: {field} at offset {offset} is {observed}; only {supported} is \
                 supported"
            ),
            Self::Descriptor(error) => error.fmt(f),
            Self::Padding {
                container,
                offset,
                y,
                value,
            } => write!(
                f,
                "{container}: padding byte {value:#04x} at offset {offset} (row {y}) is not zero"
            ),
            Self::TrailingBytes {
                container,
                expected_end,
                observed_len,
            } => write!(
                f,
                "{container}: the last row ends at {expected_end}, the file is {observed_len} \
                 bytes"
            ),
            Self::Pixels(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for BmpError {}

/// Reads and validates a whole 4 or 8 bpp `BI_RGB` BMP.
///
/// `container` labels errors and is the only name involved: the result
/// depends on `bytes` alone. `budget` is charged for the palette and the
/// unpacked indices before they are allocated.
pub fn read_bmp(
    container: &str,
    bytes: &[u8],
    budget: &mut AllocationBudget,
) -> Result<BmpImage, BmpError> {
    let mut reader = Reader::new(container, bytes);
    let header_field =
        |offset: u64, field, expected: String, observed: i64| BmpError::HeaderField {
            container: container.to_owned(),
            offset,
            field,
            expected,
            observed,
        };
    let unsupported = |offset: u64, field, supported, observed: i64| BmpError::Unsupported {
        container: container.to_owned(),
        offset,
        field,
        supported,
        observed,
    };

    let signature = reader.read_bytes("bmp.signature", 2)?;
    if signature != BMP_SIGNATURE {
        return Err(BmpError::NotBmp {
            container: container.to_owned(),
            signature: [signature[0], signature[1]],
        });
    }
    let file_size = reader.read_u32("bmp.file_size")?;
    if u64::from(file_size) != bytes.len() as u64 {
        return Err(header_field(
            2,
            "bmp.file_size",
            format!("the input length {}", bytes.len()),
            file_size.into(),
        ));
    }
    for (offset, field) in [(6, "bmp.reserved1"), (8, "bmp.reserved2")] {
        let word = reader.read_u16(field)?;
        if word != 0 {
            return Err(header_field(offset, field, "0".to_owned(), word.into()));
        }
    }
    let pixel_offset = reader.read_u32("bmp.pixel_offset")?;

    let header_size = reader.read_u32("bmp.info.header_size")?;
    if header_size != BMP_INFO_HEADER_BYTES {
        return Err(unsupported(
            14,
            "bmp.info.header_size",
            "40 (BITMAPINFOHEADER)",
            header_size.into(),
        ));
    }
    let width = reader.read_i32("bmp.info.width")?;
    let height = reader.read_i32("bmp.info.height")?;
    let planes = reader.read_u16("bmp.info.planes")?;
    let bits_per_pixel = reader.read_u16("bmp.info.bits_per_pixel")?;
    let compression = reader.read_u32("bmp.info.compression")?;
    let image_size = reader.read_u32("bmp.info.image_size")?;
    let x_pixels_per_meter = reader.read_i32("bmp.info.x_pixels_per_meter")?;
    let y_pixels_per_meter = reader.read_i32("bmp.info.y_pixels_per_meter")?;
    let colors_used = reader.read_u32("bmp.info.colors_used")?;
    let colors_important = reader.read_u32("bmp.info.colors_important")?;

    if width < 0 {
        return Err(header_field(
            18,
            "bmp.info.width",
            ">= 0".to_owned(),
            width.into(),
        ));
    }
    if planes != 1 {
        return Err(header_field(
            26,
            "bmp.info.planes",
            "1".to_owned(),
            planes.into(),
        ));
    }
    if bits_per_pixel != 4 && bits_per_pixel != 8 {
        return Err(unsupported(
            28,
            "bmp.info.bits_per_pixel",
            "4 or 8",
            bits_per_pixel.into(),
        ));
    }
    if compression != BMP_BI_RGB {
        return Err(unsupported(
            30,
            "bmp.info.compression",
            "0 (BI_RGB)",
            compression.into(),
        ));
    }
    let max_colors = 1u32 << bits_per_pixel;
    if colors_used > max_colors {
        return Err(unsupported(
            46,
            "bmp.info.colors_used",
            "at most 2^bits_per_pixel",
            colors_used.into(),
        ));
    }
    let colors = if colors_used == 0 {
        max_colors
    } else {
        colors_used
    };
    if colors_important > colors {
        return Err(header_field(
            50,
            "bmp.info.colors_important",
            format!("0 to {colors}"),
            colors_important.into(),
        ));
    }

    // A positive height is bottom-up, a negative one top-down. Both
    // magnitudes fit in u32; the descriptor refuses zero and anything
    // beyond MAX_DIMENSION before a row is read.
    let row_order = if height < 0 {
        RowOrder::TopDown
    } else {
        RowOrder::BottomUp
    };
    let extent = Extent::new(width.unsigned_abs(), height.unsigned_abs());

    let table_at = reader.position();
    let table_len = reader.checked_byte_len(
        "bmp.color_table",
        colors.into(),
        BMP_COLOR_ENTRY_BYTES.into(),
    )?;
    let table = reader.read_bytes("bmp.color_table", table_len)?;
    budget.reserve(
        "bmp.color_table",
        table_at,
        colors.into(),
        std::mem::size_of::<PaletteEntry>() as u64,
    )?;
    let palette: Vec<PaletteEntry> = table
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&[b, g, r, _reserved]| PaletteEntry::new(r, g, b))
        .collect();

    let descriptor = ImageDescriptor::new(DescriptorParts {
        extent,
        format: PixelFormat::Indexed8,
        row_order,
        palette: Some(Palette::Rgb8(palette)),
        mips: Vec::new(),
        alpha_source: AlphaSource::Opaque,
        alpha_test: AlphaTest::Unknown,
        color_space: ColorSpace::Unknown,
    })
    .map_err(BmpError::Descriptor)?;

    let expected_offset = reader.position();
    if u64::from(pixel_offset) != expected_offset {
        return Err(header_field(
            10,
            "bmp.pixel_offset",
            format!("{expected_offset} (directly after the color table)"),
            pixel_offset.into(),
        ));
    }

    // Both dimensions are at most MAX_DIMENSION now, so none of this
    // overflows.
    let width = extent.width as usize;
    let height = extent.height as usize;
    let bits = usize::from(bits_per_pixel);
    let stride = (width * bits).div_ceil(32) * 4;
    let pixels_len = stride as u64 * height as u64;
    if image_size != 0 && u64::from(image_size) != pixels_len {
        return Err(header_field(
            34,
            "bmp.info.image_size",
            format!("0 or {pixels_len}"),
            image_size.into(),
        ));
    }

    let pixels_at = reader.position();
    let pixels = reader.read_bytes(
        "bmp.pixels",
        reader.checked_byte_len("bmp.pixels", height as u64, stride as u64)?,
    )?;
    if !reader.is_empty() {
        return Err(BmpError::TrailingBytes {
            container: container.to_owned(),
            expected_end: reader.position(),
            observed_len: bytes.len() as u64,
        });
    }

    let index_len = budget.reserve("bmp.indices", pixels_at, extent.texel_count(), 1)?;
    let mut indices = Vec::with_capacity(index_len);
    let entries = colors as usize;
    let used = (width * bits).div_ceil(8);
    for (stored_row, row) in pixels.chunks_exact(stride).enumerate() {
        let y = match row_order {
            RowOrder::TopDown => stored_row,
            RowOrder::BottomUp => height - 1 - stored_row,
        };
        let row_at = pixels_at + (stored_row * stride) as u64;
        for x in 0..width {
            let (byte, index) = if bits == 8 {
                (x, row[x])
            } else if x % 2 == 0 {
                (x / 2, row[x / 2] >> 4)
            } else {
                (x / 2, row[x / 2] & 0x0F)
            };
            if usize::from(index) >= entries {
                return Err(BmpError::Pixels(TextureError::PaletteIndexOutOfRange {
                    container: container.to_owned(),
                    offset: row_at + byte as u64,
                    // Both are below MAX_DIMENSION, so they fit in u32.
                    x: x as u32,
                    y: y as u32,
                    index,
                    entries,
                }));
            }
            indices.push(index);
        }
        let padding = |offset: usize, value: u8| BmpError::Padding {
            container: container.to_owned(),
            offset: row_at + offset as u64,
            y: y as u32,
            value,
        };
        if bits == 4 && width % 2 == 1 && row[used - 1] & 0x0F != 0 {
            return Err(padding(used - 1, row[used - 1]));
        }
        if let Some(bad) = row[used..].iter().position(|&b| b != 0) {
            return Err(padding(used + bad, row[used + bad]));
        }
    }

    Ok(BmpImage {
        container: container.to_owned(),
        bits_per_pixel,
        colors_used,
        colors_important,
        pixels_per_meter: (x_pixels_per_meter, y_pixels_per_meter),
        pixel_offset,
        descriptor,
        indices,
    })
}
