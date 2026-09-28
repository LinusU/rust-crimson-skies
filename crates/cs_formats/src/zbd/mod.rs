//! ZBD family inventory, dispatch and the reader/sound container subset
//! (`specs/F06-zbd-families-reader-archives-and-sound-containers.md`, stages
//! `### F06-A` and `### F06-B`).
//!
//! "ZBD" is a family label, not one file layout: the sound, reader,
//! texture, interp, GameZ and animation containers are distinct families
//! with distinct readers. No installation bytes were read for these stages
//! (ordinary build/test capability only):
//!
//! * [`family`] is the inventory: one row per family with its reader slot,
//!   its header rule and the observed archive names that identify it.
//! * [`header`] holds the documented header rules — today exactly the
//!   INTERP signature/version — and an explicit "undocumented" rule for
//!   every family whose layout is unknown.
//! * [`role`] resolves the installation role of a path from the observed
//!   naming conventions under the `zbd/` content root.
//! * [`dispatch`] combines the two keys into a [`dispatch::ZbdDispatch`]
//!   or a machine-matchable [`dispatch::ZbdDispatchError`], never a silent
//!   fallback to another parser.
//! * [`archive`] is the bounded container layer the family readers share:
//!   declared members, checked extents, consumed and uncovered ranges, the
//!   strict status, the [`archive::RoutableFamily`] refusal that keeps a
//!   dispatched family from being named by a caller, and the
//!   [`archive::FamilyMismatch`] gate a reader puts in front of bytes of
//!   another family (stage F06-B).
//! * [`reader_archive`] and [`sound_archive`] are the two readers this
//!   stage implements (F06-B "reader and sound container subset with
//!   bounds"). They hand out each member's verbatim bytes with its source
//!   span and its evidence, and they decode nothing: no reader or sound
//!   header layout is documented, so every entry reports the recorded
//!   unknown instead (stage F06-B; task #340 owns reading those layouts).
//!
//! The evidence behind every rule, the design decisions and the recorded
//! unknowns are written down in
//! `docs/findings/2026-09-28-f06-a-zbd-family-inventory-and-dispatch.md` and
//! `docs/findings/2026-09-28-f06-b-reader-and-sound-container-bounds.md`.
//! The fixtures exercised by `crates/cs_formats/tests/zbd/` are newly
//! authored synthetic bytes; nothing here is derived from original game
//! data.

pub mod archive;
pub mod dispatch;
pub mod family;
pub mod header;
pub mod reader_archive;
pub mod role;
pub mod sound_archive;

pub use archive::{
    ArchiveListing, CONTAINER_ENTRYPOINT, ContainerError, ContainerStatus, FamilyMismatch,
    FamilyOrigin, MEMBER_ROW_BYTES, MemberError, MemberExtent, MemberRow, MemberStatus,
    MemberTable, RoutableFamily, SOURCE_SPAN_BYTES, UnsupportedRecord, list_members,
    require_family, undocumented_reason,
};
pub use dispatch::{
    DispatchBasis, HeaderStatus, RoleStatus, ZbdDispatch, ZbdDispatchError, ZbdProbe, dispatch,
};
pub use family::{ZBD_FAMILY_INVENTORY, ZbdFamily, ZbdFamilyRecord, ZbdReaderId, family_record};
pub use header::{
    HeaderProbe, HeaderRule, INTERP_SIGNATURE, INTERP_SIGNATURE_OFFSET, INTERP_VERSION,
    INTERP_VERSION_OFFSET, SignatureRule,
};
pub use reader_archive::{
    EncodingEvidence, ReaderArchive, ReaderEntry, ReaderError, read_reader_archive,
};
pub use role::{
    CONTENT_ROOT, OUTSIDE_CONTENT_ROOT, RoleLevel, RolePattern, RoleRule, UNOBSERVED_NAME, ZbdRole,
    role_for_path,
};
pub use sound_archive::{
    SoundArchive, SoundDescriptor, SoundEntry, SoundError, SoundField, read_sound_archive,
};
