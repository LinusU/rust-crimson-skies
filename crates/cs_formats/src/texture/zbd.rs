//! The Crimson Skies ZBD texture package variant (`texture.zbd`,
//! `rtexture*.zbd`, `rimage.zbd`), stage F08-B.02.
//!
//! Layout source: the pinned reference extractor (mech3ax v0.6.0, commit
//! `d3521a9721be731d365504568ddcd78e3f9846bb`, `docs/research/SOURCES.md`
//! S02), `crates/mech3ax-image/src/textures.rs`, read only; a read-only
//! survey of the 49 retail texture archives matched it for every one of
//! their 37,004 textures. Claim class *observed tool* throughout
//! ([`ZBD_TEXTURE_LAYOUT_BASIS`]): nothing here is original-verified yet
//! (the whole-corpus audit is F08-D). Details and recorded unknowns:
//! `docs/findings/2026-09-28-f08-b-02-zbd-texture-package.md`.
//!
//! All words are little-endian.
//!
//! ```text
//! header (24)   u32 0, u32 1, i32 global_palette_count, u32 texture_count,
//!               u32 0, u32 0
//! entries       texture_count x 40: 32-byte NUL-padded ASCII name,
//!               u32 start_offset, i32 palette_index (-1 = none)
//! palettes      global_palette_count x 512 bytes
//! textures      per entry, starting exactly at its start_offset:
//!               u32 flags, u16 width, u16 height, u32 0,
//!               u16 palette_count, u16 stretch;
//!               palette_count == 0: width*height u16 RGB565 texels,
//!               otherwise width*height u8 palette indices;
//!               FULL_ALPHA: width*height u8 alpha plane;
//!               local palette: palette_count u16 RGB565 entries
//! end           the file ends after the last texture
//! ```
//!
//! [`read_zbd_textures`] validates all of it, including every palette index,
//! and keeps each texture's stored level as a borrowed slice described by an
//! [`ImageDescriptor`]; [`ZbdTexture::decode`] turns it into a
//! [`DecodedImage`]. RGB565 words stay raw (the expansion to 8-bit channels
//! is a presentation fact), "simple alpha" stays metadata, and a texture
//! that uses a global palette is refused as unsupported rather than guessed:
//! no retail archive carries a global palette.

use std::fmt;

use cs_types::evidence::ClaimStatus;

use crate::error::ParseError;
use crate::io::{AllocationBudget, Reader};

use super::decode::{DecodedImage, TextureError, check_level, decode_base_level};
use super::descriptor::{
    AlphaSource, AlphaTest, ColorSpace, DescriptorError, DescriptorParts, Extent, ImageDescriptor,
    Palette, PixelFormat, RowOrder,
};

/// Bytes of the package header.
pub const ZBD_TEXTURE_HEADER_BYTES: usize = 24;
/// Bytes of one texture table entry.
pub const ZBD_TEXTURE_ENTRY_BYTES: usize = 40;
/// Bytes of the NUL-padded name field of an entry.
pub const ZBD_TEXTURE_NAME_BYTES: usize = 32;
/// Bytes of one global palette.
pub const ZBD_GLOBAL_PALETTE_BYTES: usize = 512;
/// Bytes of the per-texture info block.
pub const ZBD_TEXTURE_INFO_BYTES: usize = 16;

/// Flag bit 0; the pinned source reads it as "two bytes per pixel". Every
/// retail texture sets it, including palette textures whose indices are one
/// byte, so its meaning is not what decides the texel layout here (the
/// palette count does); a texture without it is refused as unsupported.
pub const FLAG_BYTES_PER_PIXEL2: u32 = 1 << 0;
/// Flag bit 1: the texture has alpha (simple, or full with bit 3).
pub const FLAG_HAS_ALPHA: u32 = 1 << 1;
/// Flag bit 2: the texture has no alpha.
pub const FLAG_NO_ALPHA: u32 = 1 << 2;
/// Flag bit 3: a separate alpha plane follows the texels.
pub const FLAG_FULL_ALPHA: u32 = 1 << 3;
/// Flag bit 4: the texture indexes a global palette.
pub const FLAG_GLOBAL_PALETTE: u32 = 1 << 4;
/// Flag bits 5–7: in the pinned source, the original's runtime tracking of
/// loaded image, alpha and palette buffers. Kept raw, not interpreted.
pub const FLAG_RUNTIME_MASK: u32 = 0b1110_0000;
/// Every flag bit the pinned source names.
pub const FLAG_KNOWN_MASK: u32 = 0xFF;

/// Claim class of this layout: read in the pinned reference extractor and
/// matched by a tool survey of the installation, not original-verified.
pub const ZBD_TEXTURE_LAYOUT_BASIS: ClaimStatus = ClaimStatus::ObservedTool;

/// Stored row order of every package texture.
///
/// Claim class *observed tool* ([`ZBD_TEXTURE_LAYOUT_BASIS`]): the pinned
/// extractor hands the stored texels unflipped to an image buffer whose first
/// row is the top row. Not yet matched against the original renderer.
pub const ZBD_TEXTURE_ROW_ORDER: RowOrder = RowOrder::TopDown;

/// How a package texture declares its coverage, from its flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ZbdAlpha {
    /// `NO_ALPHA`: described as [`AlphaSource::Opaque`].
    None,
    /// `HAS_ALPHA` without `FULL_ALPHA`. The pinned source treats a direct
    /// color texel stored as `0x0000` as transparent (described as
    /// [`AlphaSource::StoredValueKey`]) and skips the flag for palette
    /// textures, so for those it is [`AlphaSource::Unknown`]. Never baked:
    /// the texel keeps its stored value.
    Simple,
    /// `HAS_ALPHA | FULL_ALPHA`: a stored plane of one coverage byte per
    /// texel follows the texels ([`AlphaSource::Plane`]).
    Full,
}

impl ZbdAlpha {
    /// Stable lowercase identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Simple => "simple",
            Self::Full => "full",
        }
    }
}

/// The stored `stretch` word, named as the pinned source names it.
///
/// What each value does in the original renderer is not established; the
/// names 0–3 are the source's, and 4, 7 and 8 (which it lists as "Crimson
/// Skies only" without a meaning) are [`ZbdStretch::Unexplained`]. Any other
/// value is refused ([`ZbdTextureEntryError::UnknownStretch`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ZbdStretch {
    /// 0.
    None,
    /// 1.
    Vertical,
    /// 2.
    Horizontal,
    /// 3.
    Both,
    /// 4, 7 or 8, kept raw: meaning unknown.
    Unexplained(u16),
}

impl ZbdStretch {
    fn from_stored(stretch: u16) -> Option<Self> {
        match stretch {
            0 => Some(Self::None),
            1 => Some(Self::Vertical),
            2 => Some(Self::Horizontal),
            3 => Some(Self::Both),
            4 | 7 | 8 => Some(Self::Unexplained(stretch)),
            _ => None,
        }
    }
}

/// One texture of a package, with its stored bytes borrowed from the input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZbdTexture<'a> {
    label: String,
    entry_index: usize,
    name: String,
    start_offset: u32,
    flags: u32,
    stretch: u16,
    alpha: ZbdAlpha,
    descriptor: ImageDescriptor,
    stored: &'a [u8],
}

impl<'a> ZbdTexture<'a> {
    /// Position of the entry in the package table. Names are not unique
    /// inside a package; `(entry_index, name)` is.
    pub fn entry_index(&self) -> usize {
        self.entry_index
    }

    /// The stored name, without its NUL padding.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Provenance label used in decode errors:
    /// `<container>#<entry_index>:<name>`.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Byte offset of the texture's info block.
    pub fn start_offset(&self) -> u32 {
        self.start_offset
    }

    /// The stored flag word, runtime bits included.
    pub fn flags(&self) -> u32 {
        self.flags
    }

    /// Flag bits 5–7 as stored ([`FLAG_RUNTIME_MASK`]).
    pub fn runtime_flags(&self) -> u32 {
        self.flags & FLAG_RUNTIME_MASK
    }

    /// The declared coverage kind.
    pub fn alpha(&self) -> ZbdAlpha {
        self.alpha
    }

    /// Claim class of [`Self::alpha`] and of the [`AlphaSource`] it is
    /// described with: the pinned source's reading, not original-verified.
    pub fn alpha_basis(&self) -> ClaimStatus {
        ZBD_TEXTURE_LAYOUT_BASIS
    }

    /// The stored stretch word.
    pub fn stretch_raw(&self) -> u16 {
        self.stretch
    }

    /// The stretch word as the pinned source names it.
    pub fn stretch(&self) -> ZbdStretch {
        // Validated while reading.
        ZbdStretch::from_stored(self.stretch).unwrap_or(ZbdStretch::Unexplained(self.stretch))
    }

    /// The validated description of the stored level.
    pub fn descriptor(&self) -> &ImageDescriptor {
        &self.descriptor
    }

    /// The stored level: texels or palette indices, then the alpha plane
    /// for [`ZbdAlpha::Full`]. The local palette is in the descriptor.
    pub fn stored(&self) -> &'a [u8] {
        self.stored
    }

    /// Decodes the texture, charging `budget` for the decoded buffers.
    pub fn decode(&self, budget: &mut AllocationBudget) -> Result<DecodedImage, TextureError> {
        decode_base_level(&self.label, &self.descriptor, self.stored, budget)
    }
}

/// A read ZBD texture package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZbdTexturePackage<'a> {
    global_palette_count: u32,
    textures: Vec<ZbdTexture<'a>>,
}

impl<'a> ZbdTexturePackage<'a> {
    /// Declared global palettes (every one is skipped; no texture may use
    /// one, see [`ZbdTextureEntryError::GlobalPaletteUnsupported`]).
    pub fn global_palette_count(&self) -> u32 {
        self.global_palette_count
    }

    /// Every texture, in table order.
    pub fn textures(&self) -> &[ZbdTexture<'a>] {
        &self.textures
    }

    /// Every texture stored under `name`, in table order. Duplicates are
    /// kept as they are, never renamed.
    pub fn named<'s>(&'s self, name: &'s str) -> impl Iterator<Item = &'s ZbdTexture<'a>> + 's {
        self.textures
            .iter()
            .filter(move |texture| texture.name == name)
    }
}

/// Why a texture package was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZbdTextureError {
    /// A checked read or the allocation budget failed outside any texture
    /// (header, entry table, global palettes).
    Parse(ParseError),
    /// A header word differs from what the layout requires.
    HeaderField {
        /// Provenance label.
        container: String,
        /// Absolute offset of the word.
        offset: u64,
        /// The field.
        field: &'static str,
        /// The required value or range.
        expected: &'static str,
        /// The stored value.
        observed: i64,
    },
    /// Bytes after the last texture.
    TrailingBytes {
        /// Provenance label.
        container: String,
        /// Where the last texture ends.
        expected_end: u64,
        /// The input length.
        observed_len: u64,
    },
    /// Texture table entry `index` (named `name`) is invalid.
    Texture {
        /// Provenance label.
        container: String,
        /// The entry index.
        index: usize,
        /// Its name, or the readable prefix of a bad name field.
        name: String,
        /// What is wrong with it.
        error: Box<ZbdTextureEntryError>,
    },
}

/// What is wrong with one texture of a package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZbdTextureEntryError {
    /// A checked read inside the texture failed, e.g. a truncated alpha
    /// plane (`field` names which part).
    Parse(ParseError),
    /// A non-zero byte after the name's terminator.
    NamePadding {
        /// Absolute offset of the byte.
        offset: u64,
    },
    /// A name byte outside ASCII.
    NameNotAscii {
        /// Absolute offset of the byte.
        offset: u64,
    },
    /// An entry palette index outside `-1..global_palette_count`.
    PaletteIndexRange {
        /// Absolute offset of the field.
        offset: u64,
        /// The stored index.
        palette_index: i32,
        /// Declared global palettes.
        global_palette_count: u32,
    },
    /// The texture does not start where its entry says.
    StartOffset {
        /// The entry's start offset.
        declared: u32,
        /// Where the previous data ends.
        expected: u64,
    },
    /// Flag bits beyond the eight the pinned source names.
    UnknownFlagBits {
        /// Absolute offset of the flag word.
        offset: u64,
        /// The flag word.
        flags: u32,
        /// The unknown bits.
        unknown: u32,
    },
    /// Flag bit 0 clear; the pinned source supports only textures with it
    /// set and no retail texture lacks it.
    BytesPerPixelFlagClear {
        /// Absolute offset of the flag word.
        offset: u64,
        /// The flag word.
        flags: u32,
    },
    /// An alpha flag combination other than `NO_ALPHA`, `HAS_ALPHA` or
    /// `HAS_ALPHA | FULL_ALPHA`.
    AlphaFlags {
        /// Absolute offset of the flag word.
        offset: u64,
        /// The flag word.
        flags: u32,
    },
    /// The texture uses a global palette (flag bit 4 or an entry palette
    /// index). No retail archive has one, so how the palette is applied is
    /// not established: refused, not guessed.
    GlobalPaletteUnsupported {
        /// The flag word.
        flags: u32,
        /// The entry palette index.
        palette_index: i32,
        /// The stored palette count.
        palette_count: u16,
    },
    /// The info word at offset 8 is not zero.
    InfoField {
        /// Absolute offset of the word.
        offset: u64,
        /// The stored word.
        observed: u32,
    },
    /// A stretch word the pinned source does not list.
    UnknownStretch {
        /// Absolute offset of the word.
        offset: u64,
        /// The stored word.
        stretch: u16,
    },
    /// The size, palette or alpha combination is not a valid descriptor
    /// (zero or oversized dimension, palette beyond 256 entries, ...).
    Descriptor(DescriptorError),
    /// A stored palette index beyond the local palette; offsets and texel
    /// coordinates are relative to the texture's stored texels.
    Pixels(TextureError),
}

impl ZbdTextureError {
    /// Stable machine-matchable identifier.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Parse(error) => error.kind.as_str(),
            Self::HeaderField { .. } => "header_field",
            Self::TrailingBytes { .. } => "trailing_bytes",
            Self::Texture { error, .. } => error.code(),
        }
    }

    /// The failing texture's entry index, if a texture failed.
    pub fn entry_index(&self) -> Option<usize> {
        match self {
            Self::Texture { index, .. } => Some(*index),
            _ => None,
        }
    }

    /// The failing texture's error, if a texture failed.
    pub fn entry_error(&self) -> Option<&ZbdTextureEntryError> {
        match self {
            Self::Texture { error, .. } => Some(error.as_ref()),
            _ => None,
        }
    }
}

impl ZbdTextureEntryError {
    /// Stable machine-matchable identifier.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Parse(error) => error.kind.as_str(),
            Self::NamePadding { .. } => "name_padding",
            Self::NameNotAscii { .. } => "name_not_ascii",
            Self::PaletteIndexRange { .. } => "entry_palette_index_range",
            Self::StartOffset { .. } => "start_offset_mismatch",
            Self::UnknownFlagBits { .. } => "unknown_flag_bits",
            Self::BytesPerPixelFlagClear { .. } => "bytes_per_pixel_flag_clear",
            Self::AlphaFlags { .. } => "alpha_flags",
            Self::GlobalPaletteUnsupported { .. } => "global_palette_unsupported",
            Self::InfoField { .. } => "info_field",
            Self::UnknownStretch { .. } => "unknown_stretch",
            Self::Descriptor(error) => error.code(),
            Self::Pixels(error) => error.code(),
        }
    }
}

impl From<ParseError> for ZbdTextureError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl From<ParseError> for ZbdTextureEntryError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl fmt::Display for ZbdTextureError {
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
            Self::TrailingBytes {
                container,
                expected_end,
                observed_len,
            } => write!(
                f,
                "{container}: the last texture ends at {expected_end}, the file is \
                 {observed_len} bytes"
            ),
            Self::Texture {
                container,
                index,
                name,
                error,
            } => write!(f, "{container}: texture {index} ({name:?}): {error}"),
        }
    }
}

impl fmt::Display for ZbdTextureEntryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(f),
            Self::NamePadding { offset } => {
                write!(f, "non-zero name padding at offset {offset}")
            }
            Self::NameNotAscii { offset } => write!(f, "non-ASCII name byte at offset {offset}"),
            Self::PaletteIndexRange {
                offset,
                palette_index,
                global_palette_count,
            } => write!(
                f,
                "palette index {palette_index} at offset {offset} is outside -1..\
                 {global_palette_count}"
            ),
            Self::StartOffset { declared, expected } => write!(
                f,
                "declared start offset {declared}, but the previous data ends at {expected}"
            ),
            Self::UnknownFlagBits {
                offset,
                flags,
                unknown,
            } => write!(
                f,
                "flags {flags:#010x} at offset {offset} have unknown bits {unknown:#010x}"
            ),
            Self::BytesPerPixelFlagClear { offset, flags } => write!(
                f,
                "flags {flags:#010x} at offset {offset} lack bit 0, which no supported \
                 texture lacks"
            ),
            Self::AlphaFlags { offset, flags } => write!(
                f,
                "flags {flags:#010x} at offset {offset} combine the alpha bits inconsistently"
            ),
            Self::GlobalPaletteUnsupported {
                flags,
                palette_index,
                palette_count,
            } => write!(
                f,
                "uses a global palette (flags {flags:#010x}, palette index {palette_index}, \
                 {palette_count} entries), which is not supported"
            ),
            Self::InfoField { offset, observed } => {
                write!(f, "info word at offset {offset} is {observed}, expected 0")
            }
            Self::UnknownStretch { offset, stretch } => {
                write!(f, "unknown stretch {stretch} at offset {offset}")
            }
            Self::Descriptor(error) => error.fmt(f),
            Self::Pixels(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ZbdTextureError {}
impl std::error::Error for ZbdTextureEntryError {}

/// Reads and validates a whole ZBD texture package.
///
/// `container` labels errors; `budget` is charged for the texture table and
/// the local palettes before they are allocated. The texels themselves stay
/// borrowed from `bytes` until [`ZbdTexture::decode`].
pub fn read_zbd_textures<'a>(
    container: &str,
    bytes: &'a [u8],
    budget: &mut AllocationBudget,
) -> Result<ZbdTexturePackage<'a>, ZbdTextureError> {
    let mut reader = Reader::new(container, bytes);
    let header_field = |offset: u64, field, expected, observed: i64| ZbdTextureError::HeaderField {
        container: container.to_owned(),
        offset,
        field,
        expected,
        observed,
    };

    let zero00 = reader.read_u32("textures.header.zero00")?;
    if zero00 != 0 {
        return Err(header_field(0, "header.zero00", "0", zero00.into()));
    }
    let has_entries = reader.read_u32("textures.header.has_entries")?;
    if has_entries != 1 {
        return Err(header_field(
            4,
            "header.has_entries",
            "1",
            has_entries.into(),
        ));
    }
    let global_palette_count = reader.read_i32("textures.header.global_palette_count")?;
    let global_palette_count = u32::try_from(global_palette_count).map_err(|_| {
        header_field(
            8,
            "header.global_palette_count",
            ">= 0",
            global_palette_count.into(),
        )
    })?;
    let texture_count = reader.read_u32("textures.header.texture_count")?;
    if texture_count == 0 {
        return Err(header_field(12, "header.texture_count", "> 0", 0));
    }
    for (offset, field) in [(16, "header.zero16"), (20, "header.zero20")] {
        let word = reader.read_u32(field)?;
        if word != 0 {
            return Err(header_field(offset, field, "0", word.into()));
        }
    }

    // The whole table must be present before anything is allocated for it.
    let table_len = reader.checked_byte_len(
        "textures.entries",
        texture_count.into(),
        ZBD_TEXTURE_ENTRY_BYTES as u64,
    )?;
    let table_start = reader.position();
    let mut table = Reader::new(container, reader.read_bytes("textures.entries", table_len)?);
    let entry_count = budget.reserve(
        "textures.entries",
        table_start,
        texture_count.into(),
        std::mem::size_of::<Entry>() as u64,
    )? / std::mem::size_of::<Entry>();
    let mut entries = Vec::with_capacity(entry_count);
    for index in 0..entry_count {
        let at = table_start + (index * ZBD_TEXTURE_ENTRY_BYTES) as u64;
        let entry = read_entry(&mut table, at, global_palette_count)
            .map_err(|(name, error)| texture_error(container, index, name, error))?;
        entries.push(entry);
    }

    let palettes_len = reader.checked_byte_len(
        "textures.global_palettes",
        global_palette_count.into(),
        ZBD_GLOBAL_PALETTE_BYTES as u64,
    )?;
    reader.skip("textures.global_palettes", palettes_len)?;

    let mut textures = Vec::with_capacity(entry_count);
    for (index, entry) in entries.into_iter().enumerate() {
        let texture = read_texture(container, &mut reader, budget, index, &entry)
            .map_err(|error| texture_error(container, index, entry.name.clone(), error))?;
        textures.push(texture);
    }

    if !reader.is_empty() {
        return Err(ZbdTextureError::TrailingBytes {
            container: container.to_owned(),
            expected_end: reader.position(),
            observed_len: bytes.len() as u64,
        });
    }
    Ok(ZbdTexturePackage {
        global_palette_count,
        textures,
    })
}

fn texture_error(
    container: &str,
    index: usize,
    name: String,
    error: impl Into<Box<ZbdTextureEntryError>>,
) -> ZbdTextureError {
    ZbdTextureError::Texture {
        container: container.to_owned(),
        index,
        name,
        error: error.into(),
    }
}

struct Entry {
    name: String,
    start_offset: u32,
    palette_index: i32,
}

fn read_entry(
    table: &mut Reader<'_>,
    at: u64,
    global_palette_count: u32,
) -> Result<Entry, (String, Box<ZbdTextureEntryError>)> {
    let field = table
        .read_bytes("textures.entry.name", ZBD_TEXTURE_NAME_BYTES)
        .map_err(|error| (String::new(), Box::new(error.into())))?;
    let end = field.iter().position(|&b| b == 0);
    let text_end = end.unwrap_or(field.len());
    let prefix = String::from_utf8_lossy(&field[..text_end]).into_owned();
    if let Some(bad) = field[..text_end].iter().position(|b| !b.is_ascii()) {
        let offset = at + bad as u64;
        return Err((
            prefix,
            Box::new(ZbdTextureEntryError::NameNotAscii { offset }),
        ));
    }
    let Some(end) = end else {
        let error = ParseError::missing_terminator(
            table.container().to_owned(),
            at,
            "textures.entry.name",
            ZBD_TEXTURE_NAME_BYTES as u64,
        );
        return Err((prefix, Box::new(error.into())));
    };
    if let Some(bad) = field[end..].iter().position(|&b| b != 0) {
        let offset = at + (end + bad) as u64;
        return Err((
            prefix,
            Box::new(ZbdTextureEntryError::NamePadding { offset }),
        ));
    }

    let start_offset = table
        .read_u32("textures.entry.start_offset")
        .map_err(|error| (prefix.clone(), Box::new(error.into())))?;
    let palette_index = table
        .read_i32("textures.entry.palette_index")
        .map_err(|error| (prefix.clone(), Box::new(error.into())))?;
    if palette_index < -1 || i64::from(palette_index) >= i64::from(global_palette_count) {
        let error = ZbdTextureEntryError::PaletteIndexRange {
            offset: at + 36,
            palette_index,
            global_palette_count,
        };
        return Err((prefix, Box::new(error)));
    }
    Ok(Entry {
        name: prefix,
        start_offset,
        palette_index,
    })
}

fn read_texture<'a>(
    container: &str,
    reader: &mut Reader<'a>,
    budget: &mut AllocationBudget,
    index: usize,
    entry: &Entry,
) -> Result<ZbdTexture<'a>, ZbdTextureEntryError> {
    let start = reader.position();
    if u64::from(entry.start_offset) != start {
        return Err(ZbdTextureEntryError::StartOffset {
            declared: entry.start_offset,
            expected: start,
        });
    }

    let flags = reader.read_u32("texture.info.flags")?;
    let width = reader.read_u16("texture.info.width")?;
    let height = reader.read_u16("texture.info.height")?;
    let zero08 = reader.read_u32("texture.info.zero08")?;
    let palette_count = reader.read_u16("texture.info.palette_count")?;
    let stretch = reader.read_u16("texture.info.stretch")?;

    let unknown = flags & !FLAG_KNOWN_MASK;
    if unknown != 0 {
        return Err(ZbdTextureEntryError::UnknownFlagBits {
            offset: start,
            flags,
            unknown,
        });
    }
    if flags & FLAG_BYTES_PER_PIXEL2 == 0 {
        return Err(ZbdTextureEntryError::BytesPerPixelFlagClear {
            offset: start,
            flags,
        });
    }
    if flags & FLAG_GLOBAL_PALETTE != 0 || entry.palette_index != -1 {
        return Err(ZbdTextureEntryError::GlobalPaletteUnsupported {
            flags,
            palette_index: entry.palette_index,
            palette_count,
        });
    }
    let alpha = match (
        flags & FLAG_NO_ALPHA != 0,
        flags & FLAG_HAS_ALPHA != 0,
        flags & FLAG_FULL_ALPHA != 0,
    ) {
        (true, false, false) => ZbdAlpha::None,
        (false, true, false) => ZbdAlpha::Simple,
        (false, true, true) => ZbdAlpha::Full,
        _ => {
            return Err(ZbdTextureEntryError::AlphaFlags {
                offset: start,
                flags,
            });
        }
    };
    if zero08 != 0 {
        return Err(ZbdTextureEntryError::InfoField {
            offset: start + 8,
            observed: zero08,
        });
    }
    if ZbdStretch::from_stored(stretch).is_none() {
        return Err(ZbdTextureEntryError::UnknownStretch {
            offset: start + 14,
            stretch,
        });
    }

    let extent = Extent::new(width.into(), height.into());
    let (format, texel_field) = if palette_count == 0 {
        (PixelFormat::Rgb565, "texture.texels")
    } else {
        (PixelFormat::Indexed8, "texture.indices")
    };
    let level_start = reader.position();
    let texels_len = reader.checked_byte_len(
        texel_field,
        extent.texel_count(),
        format.bytes_per_texel().into(),
    )?;
    reader.skip(texel_field, texels_len)?;
    if alpha == ZbdAlpha::Full {
        let plane_len = reader.checked_byte_len("texture.alpha_plane", extent.texel_count(), 1)?;
        reader.skip("texture.alpha_plane", plane_len)?;
    }
    let level_end = reader.position();

    let palette = if palette_count == 0 {
        None
    } else {
        let palette_at = reader.position();
        let count = u64::from(palette_count);
        let stored = reader.read_bytes(
            "texture.palette",
            reader.checked_byte_len("texture.palette", count, 2)?,
        )?;
        budget.reserve("texture.palette", palette_at, count, 2)?;
        let entries = stored
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&word| u16::from_le_bytes(word))
            .collect();
        Some(Palette::Rgb565(entries))
    };

    let alpha_source = match (alpha, format) {
        (ZbdAlpha::None, _) => AlphaSource::Opaque,
        (ZbdAlpha::Full, _) => AlphaSource::Plane,
        (ZbdAlpha::Simple, PixelFormat::Rgb565) => AlphaSource::StoredValueKey { value: 0x0000 },
        (ZbdAlpha::Simple, _) => AlphaSource::Unknown,
    };
    let descriptor = ImageDescriptor::new(DescriptorParts {
        extent,
        format,
        row_order: ZBD_TEXTURE_ROW_ORDER,
        palette,
        mips: Vec::new(),
        alpha_source,
        alpha_test: AlphaTest::Unknown,
        color_space: ColorSpace::Unknown,
    })
    .map_err(ZbdTextureEntryError::Descriptor)?;

    // The level is the absolute window between the two positions the checked
    // reads above reached, so it is inside the container; asking the reader
    // for it keeps the bound in the one shared check instead of a slice
    // re-deriving it with a cast.
    let stored = reader.window_bytes(
        level_start,
        level_end - level_start,
        "texture.level",
    )?;
    let label = format!("{container}#{index}:{}", entry.name);
    check_level(&label, &descriptor, extent, stored).map_err(ZbdTextureEntryError::Pixels)?;

    Ok(ZbdTexture {
        label,
        entry_index: index,
        name: entry.name.clone(),
        start_offset: entry.start_offset,
        flags,
        stretch,
        alpha,
        descriptor,
        stored,
    })
}
