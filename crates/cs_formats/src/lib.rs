//! Byte-level parsers, raw records and parse diagnostics.
//!
//! This stage provides the checked reader, the structured errors, the
//! bounded-allocation / recursion utilities and the contextual-error
//! entrypoint every later parser builds on (`specs/F03-bounded-binary-
//! parsing-primitives.md`, stages F03-A, F03-B and F03-C), plus the first
//! format built on them: the raw ROF directory block of `specs/F05-rof-
//! directory-trees-and-compressed-members.md` (stage F05-A, [`rof`]).
//! Allowed
//! dependency: [`cs_types`]. This crate must never depend on Bevy or Avian,
//! and parsing must stay independent of renderer, window, network, game state
//! and asset-directory enumeration.
//!
//! On top of those primitives sits the ZBD family inventory and its
//! two-key dispatch (`specs/F06-zbd-families-reader-archives-and-
//! sound-containers.md`, stage F06-A, [`zbd`]), plus the bounded reader- and
//! sound-container subset those two keys route to (stage F06-B, [`zbd`]):
//! checked member extents, consumed and uncovered ranges, a strict status,
//! the records the stage cannot interpret, and the family gate that refuses
//! one family's bytes to another family's reader. The member index is an input
//! ([`zbd::MemberTable`]); for sound and reader archives task #343 reads it
//! from the version-one trailer task #340 recorded ([`zbd::trailer`]). Every
//! entry still reports the recorded unknown for its content.
//!
//! The raw INTERP loading-script records and their validated, lossless token
//! decoding follow:
//! `specs/F07-interp-loading-script-container.md` (stage F07-A,
//! [`interp::read_interp`], and stage F07-B, [`interp::decode_interp`]).
//!
//! Texture image descriptors, level decoding, the ZBD texture package and
//! conventional BMPs and TGAs:
//! `specs/F08-texture-archives-and-conventional-image-decoding.md` (stages
//! F08-A and F08-B, [`texture`]).
//!
//! The BM multilayer livery layout: `specs/F09-bm-multilayer-liveries-and-
//! paint-composition.md` (stage F09-A, [`bm`]).
//!
//! The lossless GameZ mesh IR and triangle-strip decoding:
//! `specs/F10-gamez-mesh-topology-and-material-records.md` (stage F10-A,
//! [`gamez`]).
//!
//! The ROF reader spans both F05 stages of `specs/F05-rof-directory-trees-
//! and-compressed-members.md`: [`rof`] defines the raw block (F05-A,
//! [`read_directory`]) and follows the tree and reads members through a
//! bounded zlib decoder (F05-B, [`read_tree`] and [`read_member`]).
//!
//! The fixtures exercised below are newly authored synthetic bytes; nothing
//! here is derived from original game data.

pub mod bm;
pub mod error;
pub mod gamez;
pub mod interp;
pub mod io;
pub mod rof;
pub mod texture;
pub mod zbd;

pub use bm::{
    BM_BYTES_PER_PIXEL, BM_ENTRYPOINT, BM_HEADER_BYTES, BM_STORED_ROW_ORDER, BmError, BmFile,
    BmPlane, BmRawHeader, BmUnsupportedTail, read_bm,
};
pub use error::{ParseError, ParseErrorKind};
pub use interp::{
    DecodedInterp, INDEX_ENTRY_BYTES, INTERP_ENTRYPOINT, INTERP_HEADER_BYTES, InterpError,
    InterpFile, InterpFinding, InterpLine, InterpRawHeader, InterpRawIndexEntry, InterpRawLine,
    InterpRawScript, InterpScript, InterpToken, LINE_HEADER_BYTES, NAME_FIELD_BYTES, RawArgument,
    RawArguments, TERMINATOR_BYTES, decode_interp, read_interp,
};
pub use io::{AllocationBudget, ParseContext, Reader, RecursionBudget, RecursionGuard};
pub use rof::{
    DIRECTORY_ENTRYPOINT, DIRECTORY_HEADER_BYTES, FLAG_COMPRESSED, FLAG_DIRECTORY, KNOWN_FLAG_MASK,
    RECORD_BYTES, RofDirectory, RofEntries, RofEntry, RofError, RofFlags, RofLimits, RofMember,
    RofMemberRead, RofRawHeader, RofRawRecord, RofTree, RofTreeDirectory, TREE_ENTRYPOINT,
    read_directory, read_member, read_tree,
};
pub use texture::{
    BmpError, BmpImage, DecodedImage, DecodedLevels, DescriptorError, ImageDescriptor,
    TextureError, TgaError, TgaImage, ZbdTexture, ZbdTextureError, ZbdTexturePackage,
    decode_base_level, decode_levels, read_bmp, read_tga, read_zbd_textures,
};
pub use zbd::{
    DispatchBasis, HeaderStatus, RoleStatus, ZbdDispatch, ZbdDispatchError, ZbdFamily, ZbdProbe,
    ZbdReaderId, dispatch,
};
