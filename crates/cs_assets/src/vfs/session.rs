//! The content session: who owns the mounts, and for how long (F04-C).
//!
//! Spec F04 non-negotiable behavior 4: "Mount lifetime belongs to a content
//! session; in-flight reads survive unloading by owned backing storage, not
//! dangling file handles." This module is that lifecycle:
//!
//! * [`SessionBuilder`] collects the mounts of one session. A mount that
//!   fails (an escaping host name, a collision, an unreadable root) is
//!   returned as a [`SessionError`] naming the mount, and the builder keeps
//!   every mount that already succeeded, so the caller can retry that one
//!   mount or give up — dropping the builder releases everything.
//! * [`ContentSession`] is the frozen result: one [`ResolveContext`], one
//!   [`Vfs`] and a process-unique [`SessionGeneration`]. Every
//!   [`SessionAsset`] it resolves is stamped with that generation, and its
//!   reads refuse an asset stamped by any other session — a texture
//!   resolved for world `c1` cannot be read through the session of world
//!   `c2` after a world switch.
//! * [`PendingRead`] is a read that was issued but not yet performed. It
//!   owns a shared handle on its mount's description (never a file handle),
//!   so it can still complete after the session closed; the bytes it
//!   produces carry the issuing generation, and
//!   [`ContentSession::accept`] delivers them only to that same session.
//!   It reads in chunks and checks a [`ReadCancel`] before each, so a world
//!   switch can cancel it from another thread mid-read (spec F04 AC04).
//! * [`ContentSession::close`] is the teardown: the session is consumed,
//!   its mounts are released, and what was released is reported.
//!
//! No file handle is ever stored: every read opens the host file afresh
//! and closes it before returning (`crate::vfs::source`).

use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext, WorldGroup,
};

use crate::install::Diagnosis;
use crate::vfs::collision::{self, CollisionReport};
use crate::vfs::mount::{Mount, MountBuilder, MountError};
use crate::vfs::resolve::{self, ResolveError, ResolvedAsset, Vfs};
use crate::vfs::source::{self, ReadError, RejectedEntry, SourceError};

/// The namespace [`SessionBuilder::mount_installation`] mounts the whole
/// installation under: keys are installation-relative spellings.
pub const INSTALL_NAMESPACE: &str = "install";

/// The namespace [`SessionBuilder::mount_installation`] mounts each world
/// group directory under: keys are spellings relative to the group.
pub const WORLD_NAMESPACE: &str = "world";

/// The next generation handed out; generations start at 1.
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

/// The identity of one content session inside this process.
///
/// Generations are assigned by [`SessionBuilder::open`] from a process-wide
/// counter, never supplied by the caller, so two sessions — the one being
/// unloaded and the one replacing it — can never share one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionGeneration(u64);

impl SessionGeneration {
    /// The raw counter value, for reports.
    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for SessionGeneration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "session#{}", self.0)
    }
}

/// Why a mount could not join a session.
#[derive(Debug)]
pub enum SessionError {
    /// A host directory could not be mounted.
    Source {
        /// The mount that failed.
        mount: MountId,
        /// Why.
        error: SourceError,
    },
    /// A mount was refused by the VFS (a repeated mount id).
    Mount(MountError),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source { mount, error } => write!(f, "cannot mount {mount}: {error}"),
            Self::Mount(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source { error, .. } => Some(error),
            Self::Mount(error) => Some(error),
        }
    }
}

/// One entry a directory mount of this session refused to mount.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRejection {
    /// The mount whose walk observed it.
    pub mount: MountId,
    /// The entry and why it was not mounted.
    pub entry: RejectedEntry,
}

/// Collects the mounts of one content session.
#[derive(Debug)]
pub struct SessionBuilder {
    context: ResolveContext,
    vfs: Vfs,
    rejected: Vec<SessionRejection>,
}

impl SessionBuilder {
    /// Starts a session that will resolve against `context`.
    pub fn new(context: ResolveContext) -> Self {
        Self {
            context,
            vfs: Vfs::new(),
            rejected: Vec::new(),
        }
    }

    /// Adds an already built mount (for example a declared archive index).
    pub fn mount(&mut self, mount: Mount) -> Result<&mut Self, SessionError> {
        self.vfs.mount(mount).map_err(SessionError::Mount)?;
        Ok(self)
    }

    /// Mounts the host directory `root` as described by `builder`.
    ///
    /// On failure nothing of this mount is kept and the error names it;
    /// mounts added before stay, so the caller can retry this one.
    pub fn mount_directory(
        &mut self,
        builder: MountBuilder,
        root: &Path,
    ) -> Result<&mut Self, SessionError> {
        let id = builder.id().clone();
        let mounted = source::mount_directory(builder, root)
            .map_err(|error| SessionError::Source { mount: id, error })?;
        let mount_id = mounted.mount.id().clone();
        self.vfs.mount(mounted.mount).map_err(SessionError::Mount)?;
        self.rejected
            .extend(mounted.rejected.into_iter().map(|entry| SessionRejection {
                mount: mount_id.clone(),
                entry,
            }));
        Ok(self)
    }

    /// Mounts an installation with the **designed** baseline layout, both
    /// kinds as [`MountBuilder::retail`] sources:
    ///
    /// * `install` — the whole tree at `host_root`, [`PrecedenceClass::Shared`],
    ///   namespace [`INSTALL_NAMESPACE`], container label `.`;
    /// * `world-<n>` — each discovered world group directory (F02-B
    ///   [`Diagnosis::world_groups`], e.g. `zbd/c1`), in discovery order,
    ///   [`PrecedenceClass::MissionWorld`], namespace [`WORLD_NAMESPACE`],
    ///   bound to that world group, container label the group's spelling.
    ///
    /// So `world/default/texture.zbd` resolves to the selected world's own
    /// `texture.zbd`, and two worlds never share one. Which retail sources
    /// the original engine really binds to a world is unmeasured (F04-D);
    /// this layout is recorded as designed in `docs/findings/`.
    pub fn mount_installation(
        &mut self,
        host_root: &Path,
        diagnosis: &Diagnosis,
    ) -> Result<&mut Self, SessionError> {
        let install = MountBuilder::new(
            MountId::new("install").expect("a valid label"),
            MountNamespace::new(INSTALL_NAMESPACE).expect("a valid label"),
            PrecedenceClass::Shared,
            ".",
        )
        .retail();
        self.mount_directory(install, host_root)?;
        for (index, group) in diagnosis.world_groups.iter().enumerate() {
            let world = MountBuilder::new(
                MountId::new(&format!("world-{index}")).expect("a valid label"),
                MountNamespace::new(WORLD_NAMESPACE).expect("a valid label"),
                PrecedenceClass::MissionWorld,
                group.as_str(),
            )
            .with_world_group(WorldGroup::from_relative(group.clone()))
            .retail();
            let mut root = host_root.to_path_buf();
            root.extend(group.as_str().split(['/', '\\']));
            self.mount_directory(world, &root)?;
        }
        Ok(self)
    }

    /// How many mounts have joined so far.
    pub fn len(&self) -> usize {
        self.vfs.len()
    }

    /// Whether no mount has joined yet.
    pub fn is_empty(&self) -> bool {
        self.vfs.is_empty()
    }

    /// Freezes the session under a fresh generation.
    pub fn open(self) -> ContentSession {
        ContentSession {
            generation: SessionGeneration(NEXT_GENERATION.fetch_add(1, Ordering::Relaxed)),
            context: self.context,
            vfs: self.vfs,
            rejected: self.rejected,
        }
    }
}

/// One resolution, stamped with the session that produced it.
///
/// The fields are private so a consumer cannot re-stamp an asset for
/// another session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionAsset {
    generation: SessionGeneration,
    resolved: ResolvedAsset,
}

impl SessionAsset {
    /// The session that resolved it.
    pub fn generation(&self) -> SessionGeneration {
        self.generation
    }

    /// The resolution: key, winning mount, immutable span and trace.
    pub fn resolved(&self) -> &ResolvedAsset {
        &self.resolved
    }
}

/// Bytes a [`PendingRead`] produced, still stamped with the issuing
/// session. Only [`ContentSession::accept`] turns them into plain bytes.
#[derive(Debug, PartialEq, Eq)]
pub struct CompletedRead {
    generation: SessionGeneration,
    key: AssetKey,
    bytes: Vec<u8>,
}

impl CompletedRead {
    /// The session that issued the read.
    pub fn generation(&self) -> SessionGeneration {
        self.generation
    }

    /// The key the bytes were read for.
    pub fn key(&self) -> &AssetKey {
        &self.key
    }
}

/// How many bytes a [`PendingRead`] reads between two cancellation checks.
pub const PENDING_READ_CHUNK: u64 = 1 << 20;

/// A shareable cancel switch for one [`PendingRead`].
///
/// It can be handed to another thread than the one completing the read —
/// typically the thread that switches worlds. Cancelling is sticky and
/// observed before every chunk, so a read in flight stops at the next
/// chunk boundary with [`ReadError::Cancelled`] and delivers nothing.
#[derive(Clone, Debug, Default)]
pub struct ReadCancel(Arc<AtomicBool>);

impl ReadCancel {
    /// Asks the read to stop.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Whether [`ReadCancel::cancel`] was called.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// How far a [`PendingRead`] has come, reported after every chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadProgress {
    /// Bytes read so far.
    pub read: u64,
    /// The member's length.
    pub total: u64,
}

/// A read issued by a session and not yet performed.
///
/// It holds a shared handle on its mount's description and the resolution
/// it is for — never an open file — so it stays valid when the session is
/// closed: completing it re-checks the member against that description,
/// then reads it in [`PENDING_READ_CHUNK`]-sized ranges (each opening the
/// file read-only and closing it again), verifies the digest and delivers.
/// Dropping it before completing cancels the read; a [`ReadCancel`] from
/// [`PendingRead::cancel_handle`] cancels it while another thread is
/// completing it.
#[derive(Debug)]
pub struct PendingRead {
    generation: SessionGeneration,
    mount: Arc<Mount>,
    resolved: ResolvedAsset,
    cancel: ReadCancel,
}

impl PendingRead {
    /// The session that issued the read.
    pub fn generation(&self) -> SessionGeneration {
        self.generation
    }

    /// A switch that cancels this read from anywhere, even while it is
    /// being completed on another thread.
    pub fn cancel_handle(&self) -> ReadCancel {
        self.cancel.clone()
    }

    /// Performs the read: the whole member, digest-checked.
    pub fn complete(self) -> Result<CompletedRead, ReadError> {
        self.complete_with(|_| {})
    }

    /// Performs the read like [`PendingRead::complete`], reporting progress
    /// after every chunk. Cancellation is checked before every chunk; a
    /// cancelled read returns [`ReadError::Cancelled`] and its partial
    /// bytes are dropped.
    pub fn complete_with(
        self,
        mut progress: impl FnMut(ReadProgress),
    ) -> Result<CompletedRead, ReadError> {
        let member = resolve::member_matching(&self.mount, &self.resolved)?;
        let total = member.size_bytes();
        let mut bytes = Vec::new();
        let mut read = 0;
        loop {
            if self.cancel.is_cancelled() {
                return Err(ReadError::Cancelled {
                    mount: self.mount.id().to_string(),
                    read,
                    total,
                });
            }
            if read == total {
                break;
            }
            let length = PENDING_READ_CHUNK.min(total - read);
            bytes.extend(source::read_member_range(
                &self.mount,
                member,
                read,
                length,
            )?);
            read += length;
            progress(ReadProgress { read, total });
        }
        resolve::check_member_digest(&self.mount, member, &bytes)?;
        Ok(CompletedRead {
            generation: self.generation,
            key: self.resolved.key,
            bytes,
        })
    }
}

/// What [`ContentSession::close`] released.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionTeardown {
    /// The generation that ended.
    pub generation: SessionGeneration,
    /// The mounts released, in registration order.
    pub released: Vec<MountId>,
}

/// One content session: a context, its mounts and its generation.
#[derive(Debug)]
pub struct ContentSession {
    generation: SessionGeneration,
    context: ResolveContext,
    vfs: Vfs,
    rejected: Vec<SessionRejection>,
}

impl ContentSession {
    /// This session's generation.
    pub fn generation(&self) -> SessionGeneration {
        self.generation
    }

    /// The context every lookup of this session resolves against.
    pub fn context(&self) -> &ResolveContext {
        &self.context
    }

    /// The mounts of this session, in registration order.
    pub fn mounts(&self) -> impl Iterator<Item = &Mount> {
        self.vfs.mounts()
    }

    /// Entries the session's directory mounts observed and refused.
    pub fn rejected(&self) -> &[SessionRejection] {
        &self.rejected
    }

    /// Every file-name collision among this session's mounts, each member
    /// looked up under each of `contexts` exactly as this session would
    /// look it up ([`crate::vfs::collision::compare_collisions`]).
    pub fn collision_report(&self, contexts: &[ResolveContext]) -> CollisionReport {
        collision::compare_collisions(&self.vfs, contexts)
    }

    /// Resolves `key` against this session's context and mounts, stamping
    /// the answer with this session's generation. A failure carries the
    /// same trace as [`Vfs::resolve`]; an answer the unmeasured precedence
    /// order alone decided between different retail bytes is refused
    /// ([`Vfs::resolve_blocking_unmeasured`]).
    pub fn resolve(&self, key: &AssetKey) -> Result<SessionAsset, ResolveError> {
        let resolved = self.vfs.resolve_blocking_unmeasured(&self.context, key)?;
        Ok(SessionAsset {
            generation: self.generation,
            resolved,
        })
    }

    /// Reads the whole member `asset` names, digest-checked.
    pub fn read_all(&self, asset: &SessionAsset) -> Result<Vec<u8>, ReadError> {
        self.own(asset.generation)?;
        self.vfs.read_all(&asset.resolved)
    }

    /// Reads `length` bytes starting `start` bytes into the member `asset`
    /// names.
    pub fn read_range(
        &self,
        asset: &SessionAsset,
        start: u64,
        length: u64,
    ) -> Result<Vec<u8>, ReadError> {
        self.own(asset.generation)?;
        self.vfs.read_range(&asset.resolved, start, length)
    }

    /// Issues a read of the whole member `asset` names that can be
    /// completed later, even after this session closed.
    pub fn begin_read(&self, asset: &SessionAsset) -> Result<PendingRead, ReadError> {
        self.own(asset.generation)?;
        let mount = self.vfs.current_mount(&asset.resolved)?;
        Ok(PendingRead {
            generation: self.generation,
            mount,
            resolved: asset.resolved.clone(),
            cancel: ReadCancel::default(),
        })
    }

    /// Delivers the bytes of a completed read, but only to the session
    /// that issued it: bytes read for a session that has since been
    /// replaced are refused, never reused by its successor.
    pub fn accept(&self, completed: CompletedRead) -> Result<Vec<u8>, ReadError> {
        self.own(completed.generation)?;
        Ok(completed.bytes)
    }

    /// Ends the session, releasing its mounts. Pending reads it issued
    /// still hold their own mount descriptions and can complete; their
    /// bytes can no longer be accepted by any session.
    pub fn close(self) -> SessionTeardown {
        SessionTeardown {
            generation: self.generation,
            released: self.vfs.mounts().map(|mount| mount.id().clone()).collect(),
        }
    }

    /// Refuses anything stamped by another session.
    fn own(&self, generation: SessionGeneration) -> Result<(), ReadError> {
        if generation == self.generation {
            Ok(())
        } else {
            Err(ReadError::ForeignSession {
                session: self.generation.get(),
                issued_by: generation.get(),
            })
        }
    }
}
