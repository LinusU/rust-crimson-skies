//! Texture image descriptors and base-level decoding
//! (`specs/F08-texture-archives-and-conventional-image-decoding.md`, stage
//! `### F08-A`).
//!
//! This stage defines the typed input and output every texture reader
//! shares and nothing else — no Crimson Skies texture archive variant is
//! parsed yet (that is F08-B), and no installation bytes were read for this
//! stage (ordinary build/test capability only):
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
//!
//! Design decisions and recorded unknowns are in
//! `docs/findings/2026-09-28-f08-a-image-descriptors-and-asymmetric-fixture.md`
//! and `docs/findings/2026-09-28-f08-b-01-mip-level-decoding.md`.
//! The fixtures exercised by `crates/cs_formats/tests/texture/` are newly
//! authored synthetic bytes; nothing here is derived from original game
//! data.

pub mod decode;
pub mod descriptor;

pub use decode::{
    DecodedFormat, DecodedImage, DecodedLevels, TextureError, decode_base_level, decode_levels,
};
pub use descriptor::{
    AlphaSource, AlphaTest, ColorSpace, DescriptorError, DescriptorParts, Extent, ImageDescriptor,
    MAX_DIMENSION, MAX_MIP_LEVELS, MAX_PALETTE_ENTRIES, PaletteEntry, PixelFormat, RowOrder,
};
