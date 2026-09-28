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
//! Mounting archive members needs the archive readers of the format tasks;
//! tracing exports and the session mount lifecycle are F04-C; measuring
//! original lookup behavior for every observed collision is F04-D. Nothing
//! here writes to the original installation.

pub mod mount;
pub mod resolve;
pub mod source;

pub use mount::{MemberRecord, Mount, MountBuilder, MountError, MountScope, SkipReason};
pub use resolve::{
    AttemptOutcome, ConflictOrigin, ResolutionAttempt, ResolutionTrace, ResolveError,
    ResolvedAsset, Vfs,
};
pub use source::{
    MountedDirectory, ReadError, RejectReason, RejectedEntry, SourceError, mount_directory,
};
