//! Typed image descriptors: what a texture variant reader must state about
//! its pixels before a single byte is decoded.
//!
//! Spec F08's deliverable names the facts a descriptor carries: "size,
//! layout, row order, channels, palette, mip levels, alpha interpretation
//! and color-space provenance". Every one of them is an explicit field here,
//! and none has a silent default: a variant reader that does not know a
//! fact says so ([`AlphaSource::Unknown`], [`AlphaTest::Unknown`],
//! [`ColorSpace::Unknown`]) instead of letting the decoder assume one.
//!
//! No Crimson Skies texture variant is described here. The research
//! boundary of spec F08 forbids assuming DXTC, 565, indexed color or any
//! other encoding until an archive variant establishes it; the
//! [`PixelFormat`]s below are the conventional, self-describing layouts a
//! variant reader (F08-B) or a conventional TGA/BMP/TIFF reader can map
//! established bytes onto. Adding a variant means adding a format here with
//! its evidence, not reinterpreting an existing one.

use std::fmt;

/// Largest width or height a descriptor accepts, in texels.
///
/// A new-engine design limit (spec F08 non-negotiable #2, "limit
/// dimensions"), not an observed property of the original data: it bounds a
/// hostile or corrupted header before any allocation is attempted. If a
/// retail texture turns out to be larger, the limit is raised with that
/// evidence; the decoder never resizes to fit it.
pub const MAX_DIMENSION: u32 = 4096;

/// Largest palette a descriptor accepts: an 8-bit index addresses at most
/// 256 entries.
pub const MAX_PALETTE_ENTRIES: usize = 256;

/// Largest number of mip levels below the base level a descriptor accepts.
///
/// A chain that at least halves one axis per level reaches 1x1 after
/// `log2(MAX_DIMENSION) = 12` levels; the descriptor does not assume the
/// halving rule (see [`ImageDescriptor::new`]), it only caps the count.
pub const MAX_MIP_LEVELS: usize = 12;

/// Width and height of one image level, in texels. Both are non-zero and at
/// most [`MAX_DIMENSION`] once they belong to a validated descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Extent {
    /// Texels per row (the column count).
    pub width: u32,
    /// Rows (the row count).
    pub height: u32,
}

impl Extent {
    /// An extent of `width` columns by `height` rows.
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// `width * height`, widened so it cannot overflow.
    pub const fn texel_count(self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

impl fmt::Display for Extent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

/// Byte layout of one stored texel.
///
/// Channels are stored in the order the name spells, one byte each, with no
/// padding between texels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    /// Red, green, blue.
    Rgb8,
    /// Red, green, blue, alpha. The alpha byte is reported as stored; what
    /// it means is [`ImageDescriptor::alpha_source`] and
    /// [`ImageDescriptor::alpha_test`], not the decoder's business.
    Rgba8,
    /// One byte per texel indexing [`ImageDescriptor::palette`].
    Indexed8,
}

impl PixelFormat {
    /// Stored bytes per texel.
    pub const fn bytes_per_texel(self) -> u32 {
        match self {
            Self::Rgb8 => 3,
            Self::Rgba8 => 4,
            Self::Indexed8 => 1,
        }
    }

    /// Stable lowercase identifier, for diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rgb8 => "rgb8",
            Self::Rgba8 => "rgba8",
            Self::Indexed8 => "indexed8",
        }
    }
}

/// Order in which the stored rows appear.
///
/// Columns are always stored left to right; decoded images are always top
/// row first ([`crate::texture::DecodedImage`]), so this is the one place a
/// vertical flip happens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RowOrder {
    /// The first stored row is the top row of the image.
    TopDown,
    /// The first stored row is the bottom row of the image (the BMP
    /// convention for a positive height).
    BottomUp,
}

/// One palette entry: red, green, blue.
///
/// Palette transparency is not a fourth channel here; it is
/// [`AlphaSource::PaletteKey`], so a black entry is never transparent by
/// accident (spec F08 non-negotiable #1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PaletteEntry {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
}

impl PaletteEntry {
    /// An entry with the given channels.
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// Where an image's coverage (alpha) comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AlphaSource {
    /// The variant establishes that the image carries no coverage: every
    /// texel is opaque.
    Opaque,
    /// The stored alpha channel of [`PixelFormat::Rgba8`] is coverage.
    Channel,
    /// Texels whose palette index equals `index` are transparent. The
    /// palette entry keeps its color; the key is metadata, not a baked alpha.
    PaletteKey {
        /// The transparent palette index.
        index: u8,
    },
    /// The variant does not establish how coverage is encoded. The decoder
    /// reports the stored bytes and makes no transparency decision.
    Unknown,
}

/// Alpha-test threshold, kept separate from where alpha comes from (spec F08
/// non-negotiable #1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AlphaTest {
    /// The material/variant establishes that no alpha test applies.
    Disabled,
    /// Texels with coverage below `threshold` are discarded at presentation.
    Threshold(u8),
    /// Not established.
    Unknown,
}

/// Color space the stored channel values are encoded in.
///
/// Recorded for the presentation boundary only: raw decoding never converts
/// (spec F08 non-negotiable #3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorSpace {
    /// sRGB-encoded channel values.
    Srgb,
    /// Linear channel values.
    Linear,
    /// Not established by any evidence yet.
    Unknown,
}

/// Everything a variant reader states when it builds a descriptor.
///
/// Plain data so a reader can fill it field by field; [`ImageDescriptor::new`]
/// validates the combination.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DescriptorParts {
    /// Base level extent.
    pub extent: Extent,
    /// Stored texel layout.
    pub format: PixelFormat,
    /// Stored row order.
    pub row_order: RowOrder,
    /// The palette; required for [`PixelFormat::Indexed8`] and forbidden
    /// otherwise.
    pub palette: Option<Vec<PaletteEntry>>,
    /// Extents of the stored mip levels below the base level, largest
    /// first; empty when the image has only its base level.
    pub mips: Vec<Extent>,
    /// Where coverage comes from.
    pub alpha_source: AlphaSource,
    /// Alpha-test threshold.
    pub alpha_test: AlphaTest,
    /// Color space of the stored values.
    pub color_space: ColorSpace,
}

/// A validated description of one stored image: every fact spec F08 lists,
/// checked for internal consistency before any pixel byte is touched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageDescriptor {
    parts: DescriptorParts,
}

impl ImageDescriptor {
    /// Validates `parts`.
    ///
    /// Checks, in order: the base extent and every mip extent are non-zero
    /// and at most [`MAX_DIMENSION`]; no more than [`MAX_MIP_LEVELS`] mips;
    /// every mip is no larger than the level above it on either axis and
    /// smaller on at least one (the exact reduction rule is a variant fact
    /// F08-B establishes, so it is not assumed here); a palette is present
    /// exactly for [`PixelFormat::Indexed8`], non-empty and at most
    /// [`MAX_PALETTE_ENTRIES`]; [`AlphaSource::Channel`] only with a stored
    /// alpha channel; [`AlphaSource::PaletteKey`] only with a palette that
    /// has the keyed entry.
    pub fn new(parts: DescriptorParts) -> Result<Self, DescriptorError> {
        check_extent(None, parts.extent)?;
        if parts.mips.len() > MAX_MIP_LEVELS {
            return Err(DescriptorError::TooManyMipLevels {
                count: parts.mips.len(),
                max: MAX_MIP_LEVELS,
            });
        }
        let mut above = parts.extent;
        for (index, &mip) in parts.mips.iter().enumerate() {
            let level = index + 1;
            check_extent(Some(level), mip)?;
            let shrinks = mip.width <= above.width
                && mip.height <= above.height
                && (mip.width < above.width || mip.height < above.height);
            if !shrinks {
                return Err(DescriptorError::MipNotSmaller {
                    level,
                    extent: mip,
                    above,
                });
            }
            above = mip;
        }

        match (parts.format, &parts.palette) {
            (PixelFormat::Indexed8, None) => {
                return Err(DescriptorError::PaletteMissing {
                    format: parts.format,
                });
            }
            (PixelFormat::Indexed8, Some(palette)) => {
                if palette.is_empty() || palette.len() > MAX_PALETTE_ENTRIES {
                    return Err(DescriptorError::PaletteSize {
                        entries: palette.len(),
                        max: MAX_PALETTE_ENTRIES,
                    });
                }
            }
            (_, Some(_)) => {
                return Err(DescriptorError::PaletteNotAllowed {
                    format: parts.format,
                });
            }
            (_, None) => {}
        }

        match parts.alpha_source {
            AlphaSource::Channel if parts.format != PixelFormat::Rgba8 => {
                return Err(DescriptorError::AlphaChannelMissing {
                    format: parts.format,
                });
            }
            AlphaSource::PaletteKey { index } => {
                let entries = parts.palette.as_ref().map_or(0, Vec::len);
                if usize::from(index) >= entries {
                    return Err(DescriptorError::PaletteKeyOutOfRange { index, entries });
                }
            }
            _ => {}
        }

        Ok(Self { parts })
    }

    /// Base level extent.
    pub fn extent(&self) -> Extent {
        self.parts.extent
    }

    /// Stored texel layout.
    pub fn format(&self) -> PixelFormat {
        self.parts.format
    }

    /// Stored row order.
    pub fn row_order(&self) -> RowOrder {
        self.parts.row_order
    }

    /// The palette of an indexed image.
    pub fn palette(&self) -> Option<&[PaletteEntry]> {
        self.parts.palette.as_deref()
    }

    /// Extents of the stored mip levels below the base level, largest first.
    pub fn mips(&self) -> &[Extent] {
        &self.parts.mips
    }

    /// Where coverage comes from.
    pub fn alpha_source(&self) -> AlphaSource {
        self.parts.alpha_source
    }

    /// Alpha-test threshold.
    pub fn alpha_test(&self) -> AlphaTest {
        self.parts.alpha_test
    }

    /// Color space of the stored values.
    pub fn color_space(&self) -> ColorSpace {
        self.parts.color_space
    }

    /// Exact stored byte length of the base level: rows of
    /// `width * bytes_per_texel` with no row padding.
    pub fn base_level_bytes(&self) -> u64 {
        self.parts.extent.texel_count() * u64::from(self.parts.format.bytes_per_texel())
    }
}

fn check_extent(level: Option<usize>, extent: Extent) -> Result<(), DescriptorError> {
    if extent.width == 0 || extent.height == 0 {
        return Err(DescriptorError::ZeroDimension { level, extent });
    }
    if extent.width > MAX_DIMENSION || extent.height > MAX_DIMENSION {
        return Err(DescriptorError::DimensionTooLarge {
            level,
            extent,
            max: MAX_DIMENSION,
        });
    }
    Ok(())
}

/// Why a [`DescriptorParts`] combination was rejected.
///
/// `level` is `None` for the base level and `Some(n)` for the `n`-th mip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DescriptorError {
    /// A width or height of zero.
    ZeroDimension {
        /// Offending level.
        level: Option<usize>,
        /// Its extent.
        extent: Extent,
    },
    /// A width or height beyond [`MAX_DIMENSION`].
    DimensionTooLarge {
        /// Offending level.
        level: Option<usize>,
        /// Its extent.
        extent: Extent,
        /// The limit.
        max: u32,
    },
    /// More mip levels than [`MAX_MIP_LEVELS`].
    TooManyMipLevels {
        /// Declared count.
        count: usize,
        /// The limit.
        max: usize,
    },
    /// A mip level that is not smaller than the level above it.
    MipNotSmaller {
        /// Offending mip level (1 is the first below the base).
        level: usize,
        /// Its extent.
        extent: Extent,
        /// Extent of the level above.
        above: Extent,
    },
    /// An indexed format without a palette.
    PaletteMissing {
        /// The format.
        format: PixelFormat,
    },
    /// A palette on a direct-color format.
    PaletteNotAllowed {
        /// The format.
        format: PixelFormat,
    },
    /// An empty palette or one with more than [`MAX_PALETTE_ENTRIES`].
    PaletteSize {
        /// Declared entries.
        entries: usize,
        /// The limit.
        max: usize,
    },
    /// [`AlphaSource::Channel`] on a format without a stored alpha channel.
    AlphaChannelMissing {
        /// The format.
        format: PixelFormat,
    },
    /// [`AlphaSource::PaletteKey`] naming an entry the palette lacks.
    PaletteKeyOutOfRange {
        /// The key.
        index: u8,
        /// Palette entries (0 without a palette).
        entries: usize,
    },
}

impl DescriptorError {
    /// Stable machine-matchable identifier.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ZeroDimension { .. } => "zero_dimension",
            Self::DimensionTooLarge { .. } => "dimension_too_large",
            Self::TooManyMipLevels { .. } => "too_many_mip_levels",
            Self::MipNotSmaller { .. } => "mip_not_smaller",
            Self::PaletteMissing { .. } => "palette_missing",
            Self::PaletteNotAllowed { .. } => "palette_not_allowed",
            Self::PaletteSize { .. } => "palette_size",
            Self::AlphaChannelMissing { .. } => "alpha_channel_missing",
            Self::PaletteKeyOutOfRange { .. } => "palette_key_out_of_range",
        }
    }
}

struct LevelName(Option<usize>);

impl fmt::Display for LevelName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            None => f.write_str("base level"),
            Some(level) => write!(f, "mip level {level}"),
        }
    }
}

impl fmt::Display for DescriptorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroDimension { level, extent } => {
                write!(f, "{} has a zero dimension ({extent})", LevelName(*level))
            }
            Self::DimensionTooLarge { level, extent, max } => write!(
                f,
                "{} is {extent}, beyond the {max} texel limit",
                LevelName(*level)
            ),
            Self::TooManyMipLevels { count, max } => {
                write!(f, "{count} mip levels declared, at most {max} accepted")
            }
            Self::MipNotSmaller {
                level,
                extent,
                above,
            } => write!(
                f,
                "mip level {level} is {extent}, not smaller than the {above} level above it"
            ),
            Self::PaletteMissing { format } => {
                write!(f, "format {} needs a palette", format.as_str())
            }
            Self::PaletteNotAllowed { format } => {
                write!(f, "format {} does not take a palette", format.as_str())
            }
            Self::PaletteSize { entries, max } => {
                write!(f, "palette has {entries} entries, expected 1 to {max}")
            }
            Self::AlphaChannelMissing { format } => write!(
                f,
                "alpha source is the alpha channel, but format {} stores none",
                format.as_str()
            ),
            Self::PaletteKeyOutOfRange { index, entries } => write!(
                f,
                "transparent palette key {index} is outside the {entries}-entry palette"
            ),
        }
    }
}

impl std::error::Error for DescriptorError {}
