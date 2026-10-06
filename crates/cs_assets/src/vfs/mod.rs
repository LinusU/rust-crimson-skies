//! The context-aware virtual filesystem and its precedence contract (F04).
//!
//! Spec F04: "AssetKey is (mount namespace, logical path, variant).
//! ResolveContext includes installation fingerprint, world group, locale,
//! mission and mod stack. The VFS returns immutable `SourceSpan` plus a
//! resolution trace. Files with equal basenames from different archives
//! must remain distinct."
//!
//! The key and context types live in `cs_types::asset_id` (contracts cross
//! crate boundaries there); this module owns the mounts and the decision:
//!
//! * [`mount`] declares what a source is — [`MountBuilder`] validates
//!   member spellings and byte ranges, refuses a case-only duplicate with
//!   both spellings, and freezes an immutable [`Mount`];
//! * [`resolve`] answers `resolve(context, key)` — [`Vfs`] ranks every
//!   eligible mount, returns one [`ResolvedAsset`] (immutable
//!   [`SourceSpan`](cs_types::asset_id::SourceSpan) plus
//!   [`ResolutionTrace`]), and fails with both origins when equal
//!   priorities tie or with the attempts when nothing holds the key.
//!
//! * [`source`] mounts a host directory tree ([`mount_directory`]: safe
//!   walk, symbolic links rejected, escaping or colliding host names refused
//!   with their spellings) and reads a resolution's bytes read-only
//!   ([`Vfs::read_range`], [`Vfs::read_all`]).
//!
//! * [`session`] owns the mounts for the lifetime of one content session
//!   ([`SessionBuilder`], [`ContentSession`]): resolutions and reads are
//!   stamped with a [`SessionGeneration`] and refused by any other session,
//!   a [`PendingRead`] survives the session closing by holding its mount
//!   description instead of a file handle, and [`ContentSession::close`]
//!   is the teardown;
//! * [`export`] is the explicit private research export
//!   ([`ExportDirectory`], [`export_asset`]): hostile member names cannot
//!   leave the export directory, and the directory cannot lie inside a
//!   mounted source.
//!
//! * [`collision`] lists every file-name collision among the mounts and
//!   looks each member up under each context ([`compare_collisions`]),
//!   classifying it as distinct by path, shadowed by identical bytes or
//!   conflicting (F04-D).
//!
//! * [`binding`] records which archives one world/mission load opens and at
//!   which level — `gamez.zbd` per world group, `planes.zbd` from the ZBD
//!   root, the world's `cam_anim.zbd` then the mission's `mis_anim.zbd` with no
//!   fallback, and the world's one texture archive
//!   ([`WorldLayout`], [`TextureBinding`]). That is the **mission level** the
//!   designed file layout had no mount for. Which texture *file* a world opens
//!   is task #352's measured rule in `cs_content::textures`, so a binding
//!   records where the search happens and the file that rule selected;
//!   [`binding::BINDING_ORDER_STATUS`] says the names are code-derived.
//!
//! * [`reader`] is the one lookup whose order is **not** the designed one:
//!   a reader archive (`zrdr.zbd`) member is reached by its **basename** from
//!   the archives mounted as [root, mission, world], the first archive that
//!   holds the name serving it ([`ReaderMounts::resolve`]). That order comes
//!   from the original executable's code, not from a runtime capture, and
//!   [`reader::READER_LOOKUP_ORDER_STATUS`] says so; the designed
//!   [`PRECEDENCE_ORDER_STATUS`] is untouched and a reader lookup never
//!   consults it.
//!
//! Mounting archive members needs the archive readers of the format tasks.
//! Original lookup behavior is unmeasured (F04-D findings), so the
//! precedence order stays `designed` and content sessions refuse a retail
//! answer that order alone decided. Nothing here writes to the original
//! installation.

pub mod binding;
pub mod collision;
pub mod export;
pub mod mount;
pub mod reader;
pub mod resolve;
pub mod session;
pub mod source;

pub use binding::{
    ArchiveFamily, BINDING_ORDER_STATUS, BindingError, BindingLevel, BindingRole, BoundArchive,
    MissionDirectory, TextureBinding, WorldLayout, mission_directories,
};
pub use collision::{
    Collision, CollisionComparison, CollisionMember, CollisionReport, CollisionVerdict,
    LookupOutcome, MemberLookup, compare_collisions, observe_collisions,
};
pub use export::{
    ExportDirectory, ExportError, ExportedFile, UnsafeName, export_asset, export_components,
};
pub use mount::{MemberRecord, Mount, MountBuilder, MountError, MountScope, SkipReason};
pub use reader::{
    READER_LOOKUP_ORDER_STATUS, READER_NAMESPACE, ReaderArchive, ReaderAttempt,
    ReaderAttemptOutcome, ReaderLevel, ReaderLookupError, ReaderMember, ReaderMountError,
    ReaderMounts, ReaderReadError, ReaderResolution, ReaderTrace, Unreachable,
    mount_reader_archive,
};
pub use resolve::{
    AttemptOutcome, ConflictOrigin, ResolutionAttempt, ResolutionTrace, ResolveError,
    ResolvedAsset, Vfs,
};
pub use session::{
    CompletedRead, ContentSession, INSTALL_NAMESPACE, MISSION_NAMESPACE, PENDING_READ_CHUNK,
    PendingRead, ReadCancel, ReadProgress, SessionAsset, SessionBuilder, SessionError,
    SessionGeneration, SessionRejection, SessionTeardown, WORLD_NAMESPACE,
};
pub use source::{
    MountedDirectory, ReadError, RejectReason, RejectedEntry, SourceError, mount_directory,
};
