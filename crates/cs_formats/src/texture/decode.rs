//! Decoding the base level of a described image into canonical texels.
//!
//! The canonical output orientation is fixed: texel `(x, y)` is column `x`
//! counted from the left and row `y` counted from the top. [`decode_base_level`]
//! is the only place a stored [`RowOrder`] is turned into that orientation,
//! so a row/column swap or a missing vertical flip shows up as a wrong texel
//! at a known coordinate, not as a plausible-looking picture (spec F08
//! non-negotiable #5).
//!
//! Decoding is value-preserving: no color-space conversion, no alpha
//! premultiplication, no palette-key baking and no resizing (non-negotiables
//! #1–#3). A stored image that is shorter or longer than its descriptor
//! states is rejected, never padded or cropped.

use std::fmt;

use crate::error::ParseError;
use crate::io::{AllocationBudget, Reader};

use super::descriptor::{
    AlphaSource, AlphaTest, ColorSpace, Extent, ImageDescriptor, PaletteEntry, PixelFormat,
    RowOrder,
};

/// Channel layout of [`DecodedImage::texels`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DecodedFormat {
    /// Red, green, blue. Direct RGB images and palette lookups.
    Rgb8,
    /// Red, green, blue, stored alpha.
    Rgba8,
}

impl DecodedFormat {
    /// Bytes per decoded texel.
    pub const fn channels(self) -> usize {
        match self {
            Self::Rgb8 => 3,
            Self::Rgba8 => 4,
        }
    }
}

/// The decoded base level of one image, top row first, columns left to
/// right, channel values exactly as stored (or as the palette states them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedImage {
    extent: Extent,
    format: DecodedFormat,
    texels: Vec<u8>,
    indices: Option<Vec<u8>>,
    alpha_source: AlphaSource,
    alpha_test: AlphaTest,
    color_space: ColorSpace,
}

impl DecodedImage {
    /// Width and height.
    pub fn extent(&self) -> Extent {
        self.extent
    }

    /// Channel layout of [`Self::texels`].
    pub fn format(&self) -> DecodedFormat {
        self.format
    }

    /// All texels, row-major from the top-left, `format().channels()` bytes
    /// each.
    pub fn texels(&self) -> &[u8] {
        &self.texels
    }

    /// The channels of texel `(x, y)` (`x` from the left, `y` from the top),
    /// or `None` outside the image.
    pub fn texel(&self, x: u32, y: u32) -> Option<&[u8]> {
        let at = self.position(x, y)?;
        let channels = self.format.channels();
        self.texels.get(at * channels..(at + 1) * channels)
    }

    /// Palette indices in the same order as [`Self::texels`], for an indexed
    /// source. Kept so palette-key transparency can be evaluated at
    /// presentation instead of being baked in here.
    pub fn indices(&self) -> Option<&[u8]> {
        self.indices.as_deref()
    }

    /// The palette index of texel `(x, y)` for an indexed source.
    pub fn index(&self, x: u32, y: u32) -> Option<u8> {
        let at = self.position(x, y)?;
        self.indices.as_ref()?.get(at).copied()
    }

    /// Where coverage comes from, carried unchanged from the descriptor.
    pub fn alpha_source(&self) -> AlphaSource {
        self.alpha_source
    }

    /// Alpha-test threshold, carried unchanged from the descriptor.
    pub fn alpha_test(&self) -> AlphaTest {
        self.alpha_test
    }

    /// Color space of the values, carried unchanged from the descriptor.
    pub fn color_space(&self) -> ColorSpace {
        self.color_space
    }

    fn position(&self, x: u32, y: u32) -> Option<usize> {
        if x >= self.extent.width || y >= self.extent.height {
            return None;
        }
        usize::try_from(u64::from(y) * u64::from(self.extent.width) + u64::from(x)).ok()
    }
}

/// Why stored base-level bytes did not decode against their descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureError {
    /// A checked read or the allocation budget failed: too few stored bytes
    /// ([`crate::ParseErrorKind::UnexpectedEof`]) or a decoded size beyond
    /// the budget ([`crate::ParseErrorKind::AllocationBudgetExceeded`]).
    Parse(ParseError),
    /// More bytes than the descriptor accounts for. A partial or padded
    /// image is rejected, not cropped.
    TrailingBytes {
        /// Provenance label.
        container: String,
        /// Bytes the descriptor accounts for.
        expected: u64,
        /// Bytes supplied.
        observed: u64,
    },
    /// A stored palette index beyond the palette.
    PaletteIndexOutOfRange {
        /// Provenance label.
        container: String,
        /// Stored byte offset of the index.
        offset: u64,
        /// Canonical column of the texel.
        x: u32,
        /// Canonical row (from the top) of the texel.
        y: u32,
        /// The index.
        index: u8,
        /// Palette entries.
        entries: usize,
    },
}

impl TextureError {
    /// Stable machine-matchable identifier.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Parse(error) => error.kind.as_str(),
            Self::TrailingBytes { .. } => "trailing_bytes",
            Self::PaletteIndexOutOfRange { .. } => "palette_index_out_of_range",
        }
    }

    /// Provenance label of the failing image.
    pub fn container(&self) -> &str {
        match self {
            Self::Parse(error) => &error.container,
            Self::TrailingBytes { container, .. }
            | Self::PaletteIndexOutOfRange { container, .. } => container,
        }
    }
}

impl From<ParseError> for TextureError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl fmt::Display for TextureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(f),
            Self::TrailingBytes {
                container,
                expected,
                observed,
            } => write!(
                f,
                "{container}: the descriptor accounts for {expected} bytes, {observed} were \
                 supplied"
            ),
            Self::PaletteIndexOutOfRange {
                container,
                offset,
                x,
                y,
                index,
                entries,
            } => write!(
                f,
                "{container}: palette index {index} at offset {offset} (texel {x},{y}) is \
                 outside the {entries}-entry palette"
            ),
        }
    }
}

impl std::error::Error for TextureError {}

/// Decodes the base level of `descriptor` from exactly `stored`.
///
/// `stored` holds the base level only, rows of `width * bytes_per_texel`
/// bytes without padding, in the descriptor's [`RowOrder`]. `container`
/// labels errors; `budget` is charged for the decoded buffers before they
/// are allocated.
pub fn decode_base_level(
    container: &str,
    descriptor: &ImageDescriptor,
    stored: &[u8],
    budget: &mut AllocationBudget,
) -> Result<DecodedImage, TextureError> {
    let extent = descriptor.extent();
    let format = descriptor.format();
    let expected = descriptor.base_level_bytes();
    let mut reader = Reader::new(container, stored);
    let stored_len = reader.checked_byte_len(
        "texture.base_level",
        extent.texel_count(),
        u64::from(format.bytes_per_texel()),
    )?;
    let base = reader.read_bytes("texture.base_level", stored_len)?;
    if !reader.is_empty() {
        return Err(TextureError::TrailingBytes {
            container: container.to_owned(),
            expected,
            observed: stored.len() as u64,
        });
    }

    let decoded_format = match format {
        PixelFormat::Rgba8 => DecodedFormat::Rgba8,
        PixelFormat::Rgb8 | PixelFormat::Indexed8 => DecodedFormat::Rgb8,
    };
    let texel_len = budget.reserve(
        "texture.decoded_texels",
        0,
        extent.texel_count(),
        decoded_format.channels() as u64,
    )?;
    let index_len = match format {
        PixelFormat::Indexed8 => {
            Some(budget.reserve("texture.decoded_indices", 0, extent.texel_count(), 1)?)
        }
        PixelFormat::Rgb8 | PixelFormat::Rgba8 => None,
    };

    let row_len = extent.width as usize * format.bytes_per_texel() as usize;
    let height = extent.height as usize;
    let mut texels = Vec::with_capacity(texel_len);
    let mut indices = index_len.map(Vec::with_capacity);
    for y in 0..height {
        let stored_row = match descriptor.row_order() {
            RowOrder::TopDown => y,
            RowOrder::BottomUp => height - 1 - y,
        };
        let row_start = stored_row * row_len;
        let row = &base[row_start..row_start + row_len];
        match (format, descriptor.palette()) {
            (PixelFormat::Indexed8, Some(palette)) => {
                for (x, &index) in row.iter().enumerate() {
                    let entry = lookup(container, palette, index, row_start + x, x, y)?;
                    texels.extend_from_slice(&[entry.r, entry.g, entry.b]);
                }
                if let Some(indices) = indices.as_mut() {
                    indices.extend_from_slice(row);
                }
            }
            _ => texels.extend_from_slice(row),
        }
    }

    Ok(DecodedImage {
        extent,
        format: decoded_format,
        texels,
        indices,
        alpha_source: descriptor.alpha_source(),
        alpha_test: descriptor.alpha_test(),
        color_space: descriptor.color_space(),
    })
}

fn lookup(
    container: &str,
    palette: &[PaletteEntry],
    index: u8,
    offset: usize,
    x: usize,
    y: usize,
) -> Result<PaletteEntry, TextureError> {
    palette
        .get(usize::from(index))
        .copied()
        .ok_or_else(|| TextureError::PaletteIndexOutOfRange {
            container: container.to_owned(),
            offset: offset as u64,
            // Both are below MAX_DIMENSION, so they fit in u32.
            x: x as u32,
            y: y as u32,
            index,
            entries: palette.len(),
        })
}
