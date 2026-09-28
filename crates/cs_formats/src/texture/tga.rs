//! Conventional Truevision TGA images, uncompressed (type 2) and run-length
//! encoded (type 10) true color, 24 and 32 bits per pixel, stage F08-B.04.
//!
//! Layout source: the published Truevision TGA File Format Specification,
//! version 2.0 (header, image/color map data, extension area, footer). A
//! read-only survey of the installation found exactly two TGAs,
//! `GOSDATA/ASSETS/GRAPHICS/font.tga` (type 2, 128x128, 32 bpp, 8 alpha
//! bits) and `GOSDATA/ASSETS/GRAPHICS/arial8.tga` (type 10, 256x256, 32 bpp,
//! 0 alpha bits), both bottom-left origin, both ending in a TGA 2.0 footer.
//! Details and recorded unknowns:
//! `docs/findings/2026-09-28-f08-b-04-conventional-tga.md`.
//!
//! All words are little-endian.
//!
//! ```text
//! header (18)      u8 id_length, u8 color_map_type (0), u8 image_type (2 | 10),
//!                  u16 color_map_first (0), u16 color_map_length (0),
//!                  u8 color_map_entry_bits (0), u16 x_origin, u16 y_origin,
//!                  u16 width, u16 height, u8 pixel_depth (24 | 32),
//!                  u8 image_descriptor: bits 3-0 alpha bits, bit 4
//!                  right-to-left (0), bit 5 top-to-bottom, bits 7-6 (0)
//! image id         id_length bytes, kept raw
//! pixels           type 2:  width * height texels of blue, green, red
//!                           (, alpha)
//!                  type 10: packets until exactly width * height texels:
//!                           u8 header; bit 7 set: one texel repeated
//!                           (header & 0x7F) + 1 times; clear: (header & 0x7F)
//!                           + 1 literal texels
//! extension (495)  optional, directly after the pixels: u16 size (495),
//!                  ..., u32 color_correction_offset (0),
//!                  u32 postage_stamp_offset (0), u32 scan_line_offset (0),
//!                  u8 attributes_type
//! footer (26)      optional, at the very end: u32 extension_offset (0 or
//!                  the end of the pixels), u32 developer_offset (0),
//!                  "TRUEVISION-XFILE." NUL
//! end              the file ends after the pixels, extension and footer
//! ```
//!
//! Only this variant is read. Color-mapped, grayscale and other depths,
//! right-to-left columns, interleaving, a developer area and extension
//! tables fail with [`TgaError::Unsupported`] rather than being guessed.
//!
//! **Alpha.** The alpha-bit count of the image descriptor decides the
//! [`AlphaSource`]; the extension area's attributes type is kept raw and
//! decides nothing. 24 bpp with 0 alpha bits is [`AlphaSource::Opaque`];
//! 32 bpp with 8 alpha bits is [`AlphaSource::Channel`]; 32 bpp with 0 alpha
//! bits is [`AlphaSource::Unknown`]: the fourth byte is reported as stored
//! and nobody here decides whether it is coverage.
//!
//! [`read_tga`] validates everything, expands the RLE packets and reorders
//! each texel from blue, green, red (, alpha) to red, green, blue (, alpha),
//! values unchanged, in stored row order; [`TgaImage::decode`] hands them to
//! [`decode_base_level`], which is the one place the row order is applied.
//! An RLE packet may span rows; it may not run past the last texel.

use std::fmt;

use crate::error::ParseError;
use crate::io::{AllocationBudget, Reader};

use super::decode::{DecodedImage, TextureError, decode_base_level};
use super::descriptor::{
    AlphaSource, AlphaTest, ColorSpace, DescriptorError, DescriptorParts, Extent, ImageDescriptor,
    PixelFormat, RowOrder,
};

/// Bytes of the fixed TGA header.
pub const TGA_HEADER_BYTES: u32 = 18;
/// Image type 2: uncompressed true color.
pub const TGA_TYPE_TRUE_COLOR: u8 = 2;
/// Image type 10: run-length encoded true color.
pub const TGA_TYPE_RLE_TRUE_COLOR: u8 = 10;
/// Bytes of the TGA 2.0 footer.
pub const TGA_FOOTER_BYTES: u32 = 26;
/// The signature a TGA 2.0 footer ends with, terminator included.
pub const TGA_FOOTER_SIGNATURE: [u8; 18] = *b"TRUEVISION-XFILE.\0";
/// Bytes of the TGA 2.0 extension area, the only size read here.
pub const TGA_EXTENSION_BYTES: u16 = 495;

/// Image descriptor bits 3-0: attribute (alpha) bits per texel.
const DESCRIPTOR_ALPHA_BITS: u8 = 0x0F;
/// Image descriptor bit 4: columns stored right to left.
const DESCRIPTOR_RIGHT_TO_LEFT: u8 = 0x10;
/// Image descriptor bit 5: rows stored top to bottom.
const DESCRIPTOR_TOP_TO_BOTTOM: u8 = 0x20;
/// Image descriptor bits 7-6: interleaving (TGA 1.0) / reserved (TGA 2.0).
const DESCRIPTOR_INTERLEAVE: u8 = 0xC0;

/// Offsets inside the extension area of the three table offsets that must
/// be zero, and of the attributes type byte.
const EXTENSION_TABLES: [(u64, &str); 3] = [
    (482, "tga.extension.color_correction_offset"),
    (486, "tga.extension.postage_stamp_offset"),
    (490, "tga.extension.scan_line_offset"),
];
const EXTENSION_ATTRIBUTES_TYPE: usize = 494;

/// A TGA 2.0 footer found at the end of the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TgaFooter {
    /// Absolute offset of the footer.
    pub offset: u64,
    /// The stored extension area offset (0 when there is none).
    pub extension_offset: u32,
    /// The stored developer area offset (always 0 here).
    pub developer_offset: u32,
}

/// The TGA 2.0 extension area, as far as it is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TgaExtension {
    /// Absolute offset of the area.
    pub offset: u64,
    /// The stored attributes type byte, kept raw. It does not decide the
    /// [`AlphaSource`].
    pub attributes_type: u8,
}

/// How the run-length encoded pixels of a type 10 image were packed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TgaRleStats {
    /// Packets read.
    pub packets: u32,
    /// Packets whose texels span more than one stored row.
    pub row_crossing_packets: u32,
}

/// A read TGA: its header facts, its validated descriptor and its texels as
/// red, green, blue (, alpha), in stored row order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TgaImage {
    container: String,
    image_type: u8,
    pixel_depth: u8,
    alpha_bits: u8,
    origin: (u16, u16),
    image_id: Vec<u8>,
    rle: Option<TgaRleStats>,
    extension: Option<TgaExtension>,
    footer: Option<TgaFooter>,
    descriptor: ImageDescriptor,
    texels: Vec<u8>,
}

impl TgaImage {
    /// Provenance label used in errors.
    pub fn container(&self) -> &str {
        &self.container
    }

    /// Stored image type: 2 or 10.
    pub fn image_type(&self) -> u8 {
        self.image_type
    }

    /// Stored bits per pixel: 24 or 32.
    pub fn pixel_depth(&self) -> u8 {
        self.pixel_depth
    }

    /// The alpha-bit count of the image descriptor (bits 3-0).
    pub fn alpha_bits(&self) -> u8 {
        self.alpha_bits
    }

    /// The stored x and y origin words, kept raw.
    pub fn origin(&self) -> (u16, u16) {
        self.origin
    }

    /// The image id field, kept raw (empty when `id_length` is 0).
    pub fn image_id(&self) -> &[u8] {
        &self.image_id
    }

    /// Packet counts of a type 10 image; `None` for type 2.
    pub fn rle(&self) -> Option<TgaRleStats> {
        self.rle
    }

    /// The extension area, if the footer points at one.
    pub fn extension(&self) -> Option<TgaExtension> {
        self.extension
    }

    /// The TGA 2.0 footer, if the file ends in one.
    pub fn footer(&self) -> Option<TgaFooter> {
        self.footer
    }

    /// The validated description: [`PixelFormat::Rgb8`] or
    /// [`PixelFormat::Rgba8`], the stored row order, no mips, the
    /// [`AlphaSource`] the alpha bits establish.
    pub fn descriptor(&self) -> &ImageDescriptor {
        &self.descriptor
    }

    /// Texels as red, green, blue (, alpha), rows in stored order, columns
    /// left to right, values as stored.
    pub fn stored_texels(&self) -> &[u8] {
        &self.texels
    }

    /// Decodes the image, charging `budget` for the decoded buffers.
    pub fn decode(&self, budget: &mut AllocationBudget) -> Result<DecodedImage, TextureError> {
        decode_base_level(&self.container, &self.descriptor, &self.texels, budget)
    }
}

/// Why a TGA was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TgaError {
    /// A checked read or the allocation budget failed: a truncated header,
    /// image id, texel or RLE packet, or a decoded size beyond the budget.
    Parse(ParseError),
    /// A header, extension or footer word differs from what the format
    /// requires.
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
    /// A valid TGA feature this reader does not implement.
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
    /// The size or row order is not a valid descriptor (zero or oversized
    /// dimension).
    Descriptor(DescriptorError),
    /// An RLE packet declares more texels than the image has left.
    RunPastImage {
        /// Provenance label.
        container: String,
        /// Absolute offset of the packet header.
        offset: u64,
        /// Texels the packet declares.
        count: u32,
        /// Texels the image still needed.
        remaining: u64,
    },
    /// Bytes after the pixels that are neither a TGA 2.0 footer nor the
    /// extension area it points at.
    TrailingBytes {
        /// Provenance label.
        container: String,
        /// Absolute offset of the first unaccounted byte.
        offset: u64,
        /// Unaccounted bytes.
        len: u64,
    },
}

impl TgaError {
    /// Stable machine-matchable identifier.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Parse(error) => error.kind.as_str(),
            Self::HeaderField { .. } => "header_field",
            Self::Unsupported { .. } => "unsupported_variant",
            Self::Descriptor(error) => error.code(),
            Self::RunPastImage { .. } => "rle_run_past_image",
            Self::TrailingBytes { .. } => "trailing_bytes",
        }
    }

    /// The field an error names, if it names one.
    pub fn field(&self) -> Option<&str> {
        match self {
            Self::Parse(error) => Some(&error.field),
            Self::HeaderField { field, .. } | Self::Unsupported { field, .. } => Some(field),
            _ => None,
        }
    }
}

impl From<ParseError> for TgaError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl fmt::Display for TgaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(f),
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
            Self::RunPastImage {
                container,
                offset,
                count,
                remaining,
            } => write!(
                f,
                "{container}: the RLE packet at offset {offset} declares {count} texels, the \
                 image has {remaining} left"
            ),
            Self::TrailingBytes {
                container,
                offset,
                len,
            } => write!(
                f,
                "{container}: {len} bytes at offset {offset} are neither pixels nor a TGA 2.0 \
                 footer or extension area"
            ),
        }
    }
}

impl std::error::Error for TgaError {}

/// Reads and validates a whole type 2 or type 10, 24 or 32 bpp TGA.
///
/// `container` labels errors and is the only name involved: the result
/// depends on `bytes` alone. `budget` is charged for the image id and the
/// reordered texels before they are allocated.
pub fn read_tga(
    container: &str,
    bytes: &[u8],
    budget: &mut AllocationBudget,
) -> Result<TgaImage, TgaError> {
    let mut reader = Reader::new(container, bytes);
    let header_field =
        |offset: u64, field, expected: String, observed: i64| TgaError::HeaderField {
            container: container.to_owned(),
            offset,
            field,
            expected,
            observed,
        };
    let unsupported = |offset: u64, field, supported, observed: i64| TgaError::Unsupported {
        container: container.to_owned(),
        offset,
        field,
        supported,
        observed,
    };

    let id_length = reader.read_u8("tga.id_length")?;
    let color_map_type = reader.read_u8("tga.color_map_type")?;
    let image_type = reader.read_u8("tga.image_type")?;
    let color_map_first = reader.read_u16("tga.color_map_first")?;
    let color_map_length = reader.read_u16("tga.color_map_length")?;
    let color_map_entry_bits = reader.read_u8("tga.color_map_entry_bits")?;
    let x_origin = reader.read_u16("tga.x_origin")?;
    let y_origin = reader.read_u16("tga.y_origin")?;
    let width = reader.read_u16("tga.width")?;
    let height = reader.read_u16("tga.height")?;
    let pixel_depth = reader.read_u8("tga.pixel_depth")?;
    let image_descriptor = reader.read_u8("tga.image_descriptor")?;

    if color_map_type != 0 {
        return Err(unsupported(
            1,
            "tga.color_map_type",
            "0 (no color map)",
            color_map_type.into(),
        ));
    }
    if image_type != TGA_TYPE_TRUE_COLOR && image_type != TGA_TYPE_RLE_TRUE_COLOR {
        return Err(unsupported(
            2,
            "tga.image_type",
            "2 or 10 (true color)",
            image_type.into(),
        ));
    }
    // Without a color map the specification leaves the color map fields
    // zero; anything else is a contradiction, not a map to skip.
    for (offset, field, value) in [
        (3, "tga.color_map_first", color_map_first),
        (5, "tga.color_map_length", color_map_length),
        (7, "tga.color_map_entry_bits", color_map_entry_bits.into()),
    ] {
        if value != 0 {
            return Err(header_field(offset, field, "0".to_owned(), value.into()));
        }
    }
    if pixel_depth != 24 && pixel_depth != 32 {
        return Err(unsupported(
            16,
            "tga.pixel_depth",
            "24 or 32",
            pixel_depth.into(),
        ));
    }
    if image_descriptor & DESCRIPTOR_INTERLEAVE != 0 {
        return Err(unsupported(
            17,
            "tga.image_descriptor",
            "bits 7-6 clear (no interleaving)",
            image_descriptor.into(),
        ));
    }
    if image_descriptor & DESCRIPTOR_RIGHT_TO_LEFT != 0 {
        return Err(unsupported(
            17,
            "tga.image_descriptor",
            "bit 4 clear (columns left to right)",
            image_descriptor.into(),
        ));
    }
    let alpha_bits = image_descriptor & DESCRIPTOR_ALPHA_BITS;
    let (format, alpha_source) = match (pixel_depth, alpha_bits) {
        (24, 0) => (PixelFormat::Rgb8, AlphaSource::Opaque),
        (32, 8) => (PixelFormat::Rgba8, AlphaSource::Channel),
        // A fourth byte the header does not call alpha: reported as stored,
        // its meaning is not established.
        (32, 0) => (PixelFormat::Rgba8, AlphaSource::Unknown),
        (24, _) => {
            return Err(header_field(
                17,
                "tga.image_descriptor",
                "0 alpha bits at 24 bpp".to_owned(),
                image_descriptor.into(),
            ));
        }
        _ => {
            return Err(unsupported(
                17,
                "tga.image_descriptor",
                "0 or 8 alpha bits at 32 bpp",
                image_descriptor.into(),
            ));
        }
    };
    let row_order = if image_descriptor & DESCRIPTOR_TOP_TO_BOTTOM != 0 {
        RowOrder::TopDown
    } else {
        RowOrder::BottomUp
    };
    let extent = Extent::new(width.into(), height.into());
    let descriptor = ImageDescriptor::new(DescriptorParts {
        extent,
        format,
        row_order,
        palette: None,
        mips: Vec::new(),
        alpha_source,
        alpha_test: AlphaTest::Unknown,
        color_space: ColorSpace::Unknown,
    })
    .map_err(TgaError::Descriptor)?;

    let id_at = reader.position();
    let image_id = reader.read_bytes("tga.image_id", id_length.into())?;
    budget.reserve("tga.image_id", id_at, id_length.into(), 1)?;
    let image_id = image_id.to_vec();

    let texel_bytes = usize::from(pixel_depth / 8);
    let pixels_at = reader.position();
    let texel_len = budget.reserve(
        "tga.texels",
        pixels_at,
        extent.texel_count(),
        texel_bytes as u64,
    )?;
    let mut texels = Vec::with_capacity(texel_len);
    let rle = if image_type == TGA_TYPE_TRUE_COLOR {
        let stored = reader.read_bytes("tga.pixels", texel_len)?;
        push_reordered(&mut texels, stored, texel_bytes);
        None
    } else {
        Some(read_rle(
            container,
            &mut reader,
            &mut texels,
            texel_bytes,
            extent,
        )?)
    };

    let pixels_end = reader.position();
    let (extension, footer) = read_trailer(container, bytes, pixels_end)?;

    Ok(TgaImage {
        container: container.to_owned(),
        image_type,
        pixel_depth,
        alpha_bits,
        origin: (x_origin, y_origin),
        image_id,
        rle,
        extension,
        footer,
        descriptor,
        texels,
    })
}

/// Appends `stored` texels of `texel_bytes` each to `texels`, blue, green,
/// red (, alpha) reordered to red, green, blue (, alpha).
fn push_reordered(texels: &mut Vec<u8>, stored: &[u8], texel_bytes: usize) {
    for texel in stored.chunks_exact(texel_bytes) {
        texels.extend_from_slice(&[texel[2], texel[1], texel[0]]);
        texels.extend_from_slice(&texel[3..]);
    }
}

/// Expands RLE packets until exactly `extent.texel_count()` texels are in
/// `texels`. A packet that would run past the last texel is refused before
/// its texels are read.
fn read_rle(
    container: &str,
    reader: &mut Reader<'_>,
    texels: &mut Vec<u8>,
    texel_bytes: usize,
    extent: Extent,
) -> Result<TgaRleStats, TgaError> {
    let total = extent.texel_count();
    let width = u64::from(extent.width);
    let mut produced = 0u64;
    let mut stats = TgaRleStats {
        packets: 0,
        row_crossing_packets: 0,
    };
    while produced < total {
        let packet_at = reader.position();
        let header = reader.read_u8("tga.rle.packet_header")?;
        let count = u32::from(header & 0x7F) + 1;
        let remaining = total - produced;
        if u64::from(count) > remaining {
            return Err(TgaError::RunPastImage {
                container: container.to_owned(),
                offset: packet_at,
                count,
                remaining,
            });
        }
        if header & 0x80 != 0 {
            let texel = reader.read_bytes("tga.rle.run_texel", texel_bytes)?;
            for _ in 0..count {
                push_reordered(texels, texel, texel_bytes);
            }
        } else {
            let len =
                reader.checked_byte_len("tga.rle.raw_texels", count.into(), texel_bytes as u64)?;
            push_reordered(
                texels,
                reader.read_bytes("tga.rle.raw_texels", len)?,
                texel_bytes,
            );
        }
        let first_row = produced / width;
        produced += u64::from(count);
        stats.packets += 1;
        if (produced - 1) / width != first_row {
            stats.row_crossing_packets += 1;
        }
    }
    Ok(stats)
}

/// Accounts for every byte after the pixels: nothing, or a TGA 2.0 footer
/// at the very end, optionally pointing at an extension area directly after
/// the pixels. Anything else is [`TgaError::TrailingBytes`].
fn read_trailer(
    container: &str,
    bytes: &[u8],
    pixels_end: u64,
) -> Result<(Option<TgaExtension>, Option<TgaFooter>), TgaError> {
    let len = bytes.len() as u64;
    let trailing = |offset: u64| TgaError::TrailingBytes {
        container: container.to_owned(),
        offset,
        len: len - offset,
    };
    if pixels_end == len {
        return Ok((None, None));
    }
    let footer_bytes = u64::from(TGA_FOOTER_BYTES);
    if len - pixels_end < footer_bytes || !bytes.ends_with(&TGA_FOOTER_SIGNATURE) {
        return Err(trailing(pixels_end));
    }
    let footer_at = len - footer_bytes;
    let mut reader = Reader::new(container, bytes);
    reader.skip("tga.footer", footer_at as usize)?;
    let extension_offset = reader.read_u32("tga.footer.extension_offset")?;
    let developer_offset = reader.read_u32("tga.footer.developer_offset")?;
    let footer = TgaFooter {
        offset: footer_at,
        extension_offset,
        developer_offset,
    };
    if developer_offset != 0 {
        return Err(TgaError::Unsupported {
            container: container.to_owned(),
            offset: footer_at + 4,
            field: "tga.footer.developer_offset",
            supported: "0 (no developer area)",
            observed: developer_offset.into(),
        });
    }
    if extension_offset == 0 {
        if footer_at != pixels_end {
            return Err(trailing(pixels_end));
        }
        return Ok((None, Some(footer)));
    }
    if u64::from(extension_offset) != pixels_end {
        return Err(TgaError::HeaderField {
            container: container.to_owned(),
            offset: footer_at,
            field: "tga.footer.extension_offset",
            expected: format!("0 or {pixels_end} (directly after the pixels)"),
            observed: extension_offset.into(),
        });
    }

    let area = &bytes[pixels_end as usize..footer_at as usize];
    let mut reader = Reader::new(container, area);
    let size = reader.read_u16("tga.extension.size")?;
    if size != TGA_EXTENSION_BYTES {
        return Err(TgaError::Unsupported {
            container: container.to_owned(),
            offset: pixels_end,
            field: "tga.extension.size",
            supported: "495 (TGA 2.0)",
            observed: size.into(),
        });
    }
    let area_end = pixels_end + u64::from(TGA_EXTENSION_BYTES);
    if area.len() < usize::from(TGA_EXTENSION_BYTES) {
        return Err(TgaError::HeaderField {
            container: container.to_owned(),
            offset: pixels_end,
            field: "tga.extension.size",
            expected: format!("at most {} (the footer follows)", area.len()),
            observed: size.into(),
        });
    }
    if area_end != footer_at {
        return Err(trailing(area_end));
    }
    for (at, field) in EXTENSION_TABLES {
        let word = u32::from_le_bytes([
            area[at as usize],
            area[at as usize + 1],
            area[at as usize + 2],
            area[at as usize + 3],
        ]);
        if word != 0 {
            return Err(TgaError::Unsupported {
                container: container.to_owned(),
                offset: pixels_end + at,
                field,
                supported: "0 (no table)",
                observed: word.into(),
            });
        }
    }
    let extension = TgaExtension {
        offset: pixels_end,
        attributes_type: area[EXTENSION_ATTRIBUTES_TYPE],
    };
    Ok((Some(extension), Some(footer)))
}
