//! Texture image descriptors and base-level decoding
//! (`specs/F08-texture-archives-and-conventional-image-decoding.md`, stage
//! `### F08-A`).
//!
//! Stage F08-A defined the typed input and output every texture reader
//! shares; stage F08-B adds the variant readers:
//!
//! * [`descriptor`] is the input: an [`ImageDescriptor`] states the size,
//!   stored layout, row order, palette, mip levels, alpha interpretation and
//!   color space of one stored image, validated before any pixel is read.
//! * [`decode`] is the output: [`decode_base_level`] turns the stored base
//!   level into a [`DecodedImage`] in one fixed orientation (top row first,
//!   columns left to right) with channel values unchanged.
//!   [`decode_levels`] (stage F08-B.01) does the same for the base level
//!   and every declared mip level, from per-level byte slices the variant
//!   reader supplies.
//! * [`zbd`] (stage F08-B.02) reads the Crimson Skies ZBD texture package
//!   (`texture.zbd`, `rtexture*.zbd`, `rimage.zbd`) into one descriptor and
//!   one borrowed stored level per texture.
//! * [`bmp`] (stage F08-B.03) reads conventional 4 and 8 bpp `BI_RGB` BMPs,
//!   recognised by content rather than by name, into one descriptor and
//!   one unpacked index level.
//! * [`tga`] (stage F08-B.04) reads conventional uncompressed and RLE true
//!   color TGAs (types 2 and 10, 24 and 32 bpp), including a TGA 2.0 footer
//!   and extension area, into one descriptor and one reordered texel level.
//!
//! Design decisions and recorded unknowns are in
//! `docs/findings/2026-09-28-f08-a-image-descriptors-and-asymmetric-fixture.md`,
//! `docs/findings/2026-09-28-f08-b-01-mip-level-decoding.md`,
//! `docs/findings/2026-09-28-f08-b-02-zbd-texture-package.md`,
//! `docs/findings/2026-09-28-f08-b-03-conventional-bmp.md` and
//! `docs/findings/2026-09-28-f08-b-04-conventional-tga.md`.
//! The fixtures exercised by `crates/cs_formats/tests/texture/` are newly
//! authored synthetic bytes; nothing here is derived from original game
//! data.

pub mod bmp;
pub mod decode;
pub mod descriptor;
pub mod tga;
pub mod zbd;

pub use bmp::{BmpError, BmpImage, looks_like_bmp, read_bmp};
pub use decode::{
    DecodedFormat, DecodedImage, DecodedLevels, TextureError, decode_base_level, decode_levels,
};
pub use descriptor::{
    AlphaSource, AlphaTest, ColorSpace, DescriptorError, DescriptorParts, Extent, ImageDescriptor,
    MAX_DIMENSION, MAX_MIP_LEVELS, MAX_PALETTE_ENTRIES, Palette, PaletteEntry, PixelFormat,
    RowOrder,
};
pub use tga::{TgaError, TgaExtension, TgaFooter, TgaImage, TgaRleStats, read_tga};
pub use zbd::{
    ZbdAlpha, ZbdStretch, ZbdTexture, ZbdTextureEntryError, ZbdTextureError, ZbdTexturePackage,
    read_zbd_textures,
};
