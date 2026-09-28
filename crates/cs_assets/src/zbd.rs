//! ZBD containers on a content session, and the sound samples they yield
//! (`specs/F06-zbd-families-reader-archives-and-sound-containers.md`, stage
//! `### F06-C`).
//!
//! This module is the wiring stage of F06. Tasks #340, #343 and #344 already
//! established what a ZBD container *is* for the sound family: `ZBD/sounds*.zbd`
//! archives carry a version-one member index in their trailer and each member
//! is a RIFF/WAVE file whose `fmt ` header declares the sample layout
//! (`docs/findings/2026-09-28-t3{40,43,44}-*.md`). This module connects that
//! to the places the bytes actually live and the places the samples are
//! actually used:
//!
//! * **the producer — the VFS.** [`ZbdContainer::open`] resolves a ZBD
//!   container key in a [`ContentSession`], reads its bytes, rebuilds the
//!   **installation-relative** [`RelativePath`] from the resolution's
//!   immutable [`SourceSpan`], and runs the F06-A two-key dispatch.
//!   [`ZbdContainer::index`] then reads the container's **own** trailer, so
//!   the member index is what the archive declares rather than anything a
//!   caller supplies.
//! * **the consumer — audio assets.** [`ZbdContainer::sound_assets`] is the
//!   F06-B sound reader plus the F06-C decode: every readable entry becomes
//!   a [`SoundAsset`] carrying its identity, its span, the WAVE header its
//!   member declares, and its [`SoundReadiness`].
//!
//! # What this stage does not do
//!
//! * It does not decode ADPCM. Task #344 measured that every retail member is
//!   IMA ADPCM, MS ADPCM or 8-bit PCM; an ADPCM member is an
//!   [`SoundReadiness::UnsupportedFormat`] row carrying its **declared** tag,
//!   never an approximation and never a silent pass-through.
//! * It does not mix, resample, filter or play anything. The values are the
//!   stored samples widened to `i32`; what an audio consumer does with them is
//!   F41's work.
//! * It does not mount anything. F04's `Mount` is the source of these bytes
//!   and is untouched.
//!
//! # Teardown, retry and stale state
//!
//! * A [`ZbdContainer`] **owns** its bytes, so closing the session that
//!   produced it does not invalidate it: its archives and its sound assets
//!   stay readable (spec F04 non-negotiable behavior 4, "in-flight reads
//!   survive unloading by owned backing storage, not dangling file handles").
//! * It is stamped with the [`SessionGeneration`] that read it, and
//!   [`ZbdContainer::require_session`] refuses a container a *different*
//!   session is now serving, so a sound archive resolved for one world can
//!   never be reused after a world switch.
//! * A listing refused by a starved parse budget can be retried on the same
//!   container with a context that has budget: the refused attempt rolled its
//!   own charges back (spec F03-C), so the retry is honest.
//!
//! Every failure arrives as one [`ZbdError`] with a stable [`ZbdError::code`]
//! and a `source()` chain back to the resolve, read, dispatch, index, listing
//! or decode failure that caused it.

use std::fmt;

use cs_formats::ParseContext;
use cs_formats::zbd::{
    ArchiveListing, ContainerStatus, DispatchBasis, HeaderStatus, IndexError, MemberStatus,
    MemberTable, ReaderError, RoleStatus, RoutableFamily, SampleError, SoundArchive, SoundEntry,
    SoundError, UnsupportedRecord, VersionOneIndex, WaveError, WaveHeader, ZbdDispatch,
    ZbdDispatchError, ZbdFamily, ZbdProbe, ZbdReaderId, decode_sound_sample, dispatch,
    read_sound_archive, read_version_one_index,
};
use cs_types::asset_id::{AssetKey, MountId, SourceSpan};
use cs_types::evidence::SourceSpan as ByteSpan;
use cs_types::install::{RelativePath, RelativePathError};

use crate::vfs::resolve::ResolveError;
use crate::vfs::session::{ContentSession, SessionAsset, SessionGeneration};
use crate::vfs::source::ReadError;

/// Why a ZBD container could not be opened, routed, read or decoded.
///
/// Every variant carries the decision-relevant values and never member bytes
/// (F03: errors carry metadata only, so a diagnostic cannot echo private
/// installation data). Match on [`Self::code`] rather than the
/// [`fmt::Display`] text.
#[derive(Debug)]
pub enum ZbdError {
    /// The key did not resolve to exactly one origin.
    Resolve(ResolveError),
    /// The resolved member's bytes could not be read, or failed the
    /// mount-time digest check.
    Read(ReadError),
    /// The resolution's container label and member spelling do not compose
    /// into a valid [`RelativePath`], so no role rule can be matched against
    /// it. The rejected spelling is carried verbatim.
    ContainerPath {
        /// The composition that was refused.
        spelling: String,
        /// Which path rule refused it.
        reason: RelativePathError,
    },
    /// The container's two keys **disagreed** with each other, or no key named
    /// a family at all. Refused, never re-read by another parser (spec F06
    /// AC02).
    Dispatch(ZbdDispatchError),
    /// The container's own member index could not be read (task #343).
    Index(IndexError),
    /// The reader-family reader refused the container.
    Reader(ReaderError),
    /// The sound-family reader refused the container.
    Sound(SoundError),
    /// One member's samples could not be decoded. Only a **whole-container**
    /// decode reports this; a per-member failure is that member's
    /// [`SoundReadiness`], so one bad member cannot hide its siblings.
    Sample(SampleError),
    /// The named family is one a dispatch already routes, so a caller
    /// asserted something the two keys own (F06-B's `RoutableFamily`).
    Routable(RoutableFamily),
    /// The member index or member table handed to a container was read from
    /// another container's bytes, so its extents describe someone else's
    /// members. Refused before any member is read.
    ForeignIndex {
        /// The provenance label of the container asked to read.
        container: String,
        /// The provenance label the index or table carries.
        index_container: String,
    },
}

impl ZbdError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Resolve(_) => "resolve",
            Self::Read(_) => "read",
            Self::ContainerPath { .. } => "container_path",
            Self::Dispatch(_) => "dispatch",
            Self::Index(error) => error.code(),
            Self::Reader(error) => error.code(),
            Self::Sound(error) => error.code(),
            Self::Sample(error) => error.code(),
            Self::Routable(error) => error.code(),
            Self::ForeignIndex { .. } => "foreign_index",
        }
    }
}

impl fmt::Display for ZbdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resolve(error) => write!(f, "{error}"),
            Self::Read(error) => write!(f, "{error}"),
            Self::ContainerPath { spelling, reason } => {
                write!(
                    f,
                    "the resolved container path {spelling:?} is refused: {reason}"
                )
            }
            Self::Dispatch(error) => write!(f, "{error}"),
            Self::Index(error) => write!(f, "{error}"),
            Self::Reader(error) => write!(f, "{error}"),
            Self::Sound(error) => write!(f, "{error}"),
            Self::Sample(error) => write!(f, "{error}"),
            Self::Routable(error) => write!(f, "{error}"),
            Self::ForeignIndex {
                container,
                index_container,
            } => write!(
                f,
                "{container}: the member index handed in was read from {index_container}, not \
                 from this container's bytes"
            ),
        }
    }
}

impl std::error::Error for ZbdError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Resolve(error) => Some(error),
            Self::Read(error) => Some(error),
            Self::ContainerPath { reason, .. } => Some(reason),
            Self::Dispatch(error) => Some(error),
            Self::Index(error) => Some(error),
            Self::Reader(error) => Some(error),
            Self::Sound(error) => Some(error),
            Self::Sample(error) => Some(error),
            Self::Routable(error) => Some(error),
            Self::ForeignIndex { .. } => None,
        }
    }
}

impl From<ResolveError> for ZbdError {
    fn from(error: ResolveError) -> Self {
        Self::Resolve(error)
    }
}

impl From<ReadError> for ZbdError {
    fn from(error: ReadError) -> Self {
        Self::Read(error)
    }
}

impl From<ZbdDispatchError> for ZbdError {
    fn from(error: ZbdDispatchError) -> Self {
        Self::Dispatch(error)
    }
}

impl From<IndexError> for ZbdError {
    fn from(error: IndexError) -> Self {
        Self::Index(error)
    }
}

impl From<ReaderError> for ZbdError {
    fn from(error: ReaderError) -> Self {
        Self::Reader(error)
    }
}

impl From<SoundError> for ZbdError {
    fn from(error: SoundError) -> Self {
        Self::Sound(error)
    }
}

impl From<SampleError> for ZbdError {
    fn from(error: SampleError) -> Self {
        Self::Sample(error)
    }
}

impl From<RoutableFamily> for ZbdError {
    fn from(error: RoutableFamily) -> Self {
        Self::Routable(error)
    }
}

/// Which family a container's two keys decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZbdRouting {
    /// The two keys agreed, or one key alone identified the family.
    Routed {
        /// The family they named.
        family: ZbdFamily,
        /// The reader slot it routes to.
        reader: ZbdReaderId,
        /// Which key identified it ([`DispatchBasis`]).
        basis: DispatchBasis,
        /// What the header bytes could establish ([`HeaderStatus`]).
        header_status: HeaderStatus,
        /// What the observed installation role contributed ([`RoleStatus`]).
        role_status: RoleStatus,
    },
    /// No key named a family, so the container is not routed and cannot be
    /// read. `reason` is the inventory's own recorded explanation.
    Unrouted {
        /// Why no key named a family.
        reason: &'static str,
    },
}

impl ZbdRouting {
    /// The family the two keys named, when they named one.
    pub const fn family(&self) -> Option<ZbdFamily> {
        match self {
            Self::Routed { family, .. } => Some(*family),
            Self::Unrouted { .. } => None,
        }
    }

    /// The reader slot the container routes to, when it routes at all.
    pub const fn reader(&self) -> Option<ZbdReaderId> {
        match self {
            Self::Routed { reader, .. } => Some(*reader),
            Self::Unrouted { .. } => None,
        }
    }

    /// Whether the two keys named a family.
    pub const fn is_routed(&self) -> bool {
        matches!(self, Self::Routed { .. })
    }
}

/// One ZBD container, read through a content session.
///
/// The container **owns** the bytes it was read from, so it stays usable
/// after the session that produced it is closed, and it carries the
/// [`SessionGeneration`] that read it so a later session can refuse it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZbdContainer {
    key: AssetKey,
    mount: MountId,
    span: SourceSpan,
    generation: SessionGeneration,
    label: String,
    path: RelativePath,
    header: Vec<u8>,
    bytes: Vec<u8>,
    routing: ZbdRouting,
}

impl ZbdContainer {
    /// Resolves `key` in `session` and reads the container it names.
    ///
    /// The installation-relative path dispatch is matched against is rebuilt
    /// from the resolution's immutable [`SourceSpan`]: the member key alone
    /// when the mount's container label is the installation root `.`, and
    /// `container/member` otherwise. The result is validated by
    /// [`RelativePath::new`], so a container label or member spelling that
    /// would escape or contain `..` is refused with
    /// [`ZbdError::ContainerPath`] instead of being dispatched.
    ///
    /// Only the leading bytes any documented header rule can look at reach
    /// the dispatch (task #340 added the animation and GameZ signature
    /// rules); the whole container is kept for the index and the readers.
    ///
    /// # Errors
    ///
    /// [`ZbdError::Resolve`] when the key does not resolve to exactly one
    /// origin, [`ZbdError::Read`] when the bytes cannot be read or fail the
    /// mount-time digest check, [`ZbdError::ContainerPath`] when the resolved
    /// path does not survive validation, and [`ZbdError::Dispatch`] when the
    /// two keys disagree **or name no family at all** — a container nothing
    /// routes is not a ZBD container this stage may read.
    pub fn open(session: &ContentSession, key: &AssetKey) -> Result<Self, ZbdError> {
        let asset = session.resolve(key)?;
        let bytes = session.read_all(&asset)?;
        Self::from_resolution(&asset, bytes)
    }

    /// The body [`Self::open`] and any future pending-read path share.
    fn from_resolution(asset: &SessionAsset, bytes: Vec<u8>) -> Result<Self, ZbdError> {
        let resolved = asset.resolved();
        let path = installation_path(&resolved.span)?;
        let label = format!("{} at {}", resolved.mount, resolved.key);
        let header = bytes[..bytes.len().min(header_probe_bytes())].to_vec();
        // The decision is taken from the three owned fields below and
        // re-derived on demand by `Self::dispatch`, so the two can never
        // disagree.
        let routing = match dispatch(ZbdProbe::new(&label, &path, &header)) {
            Ok(decided) => ZbdRouting::Routed {
                family: decided.family(),
                reader: decided.reader(),
                basis: decided.basis(),
                header_status: decided.header_status(),
                role_status: decided.role_status(),
            },
            // No key names a family: the container is not routed, and this
            // stage reads nothing it cannot name.
            Err(ZbdDispatchError::UnknownFamily { reason, .. }) => ZbdRouting::Unrouted { reason },
            Err(error) => return Err(ZbdError::Dispatch(error)),
        };
        if let ZbdRouting::Unrouted { reason } = routing {
            return Err(ZbdError::Dispatch(ZbdDispatchError::UnknownFamily {
                container: label.clone(),
                reason,
            }));
        }
        Ok(Self {
            key: resolved.key.clone(),
            mount: resolved.mount.clone(),
            span: resolved.span.clone(),
            generation: asset.generation(),
            label,
            path,
            header,
            bytes,
            routing,
        })
    }

    /// The key the container was resolved with.
    pub const fn key(&self) -> &AssetKey {
        &self.key
    }

    /// The mount that served the key.
    pub const fn mount(&self) -> &MountId {
        &self.mount
    }

    /// The immutable origin of the container's bytes.
    pub const fn span(&self) -> &SourceSpan {
        &self.span
    }

    /// The session generation that read the container.
    pub const fn generation(&self) -> SessionGeneration {
        self.generation
    }

    /// The provenance label every result and error of this container carries.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The installation-relative path the two keys were matched against.
    pub const fn path(&self) -> &RelativePath {
        &self.path
    }

    /// The leading bytes the dispatch was given.
    pub fn header(&self) -> &[u8] {
        &self.header
    }

    /// The whole container, as the mount read it.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The routing the two keys decided when the container was opened.
    pub const fn routing(&self) -> &ZbdRouting {
        &self.routing
    }

    /// The family the two keys named.
    pub const fn family(&self) -> ZbdFamily {
        self.routing
            .family()
            .expect("an unrouted container is refused by `open`")
    }

    /// The reader slot the container routes to.
    pub const fn reader(&self) -> ZbdReaderId {
        self.routing
            .reader()
            .expect("an unrouted container is refused by `open`")
    }

    /// Refuses a container this session did not read.
    ///
    /// A world switch replaces the session, and a container resolved for the
    /// old world must not be served to the new one (spec F04 non-negotiable
    /// behavior 4). The container stays usable after its own session is
    /// **closed** — it owns its bytes — but it is not a member of any other
    /// session.
    ///
    /// # Errors
    ///
    /// [`ZbdError::Read`] with [`ReadError::ForeignSession`] when the
    /// generations differ.
    pub fn require_session(&self, session: &ContentSession) -> Result<(), ZbdError> {
        if session.generation() == self.generation {
            Ok(())
        } else {
            Err(ZbdError::Read(ReadError::ForeignSession {
                session: session.generation().get(),
                issued_by: self.generation.get(),
            }))
        }
    }

    /// The same decision [`Self::routing`] records, re-derived from this
    /// container's own label, path and header prefix.
    ///
    /// # Errors
    ///
    /// The [`ZbdDispatchError`] of an unrouted container, and of any
    /// container whose stored fields somehow no longer agree — impossible for
    /// a container built by [`Self::open`], which stored exactly these fields.
    pub fn dispatch(&self) -> Result<ZbdDispatch<'_>, ZbdDispatchError> {
        dispatch(ZbdProbe::new(&self.label, &self.path, &self.header))
    }

    /// The member index this container declares in its own trailer
    /// (task #343).
    ///
    /// This is the **member producer**: the index comes out of the container
    /// the two keys routed here, not out of a caller's table. Only the sound
    /// and reader families carry a version-one trailer, and the reader
    /// refuses any other family before a byte is read.
    ///
    /// # Errors
    ///
    /// [`ZbdError::Index`] when the trailer is absent, truncated, of another
    /// version, or does not fit in front of itself.
    pub fn index<'c>(
        &'c self,
        context: &mut ParseContext,
    ) -> Result<VersionOneIndex<'c>, ZbdError> {
        let decided = self.dispatch().map_err(ZbdError::Dispatch)?;
        Ok(read_version_one_index(context, decided, &self.bytes)?)
    }

    /// Reads the sound archive of a container the two keys routed to the
    /// sound family, from the member index its own trailer declares.
    ///
    /// `index` and `table` are the [`VersionOneIndex`] [`Self::index`]
    /// returned and the [`MemberTable`] its
    /// [`VersionOneIndex::member_table`] built. The caller keeps both: the
    /// index owns the entry and extent tables, the table borrows them, and the
    /// archive borrows both — so nothing here can hand out an archive that
    /// outlives its own index.
    ///
    /// ```ignore
    /// let index = container.index(&mut context)?;
    /// let table = index.member_table();
    /// let assets = container.sound_assets(&mut context, &index, &table)?;
    /// ```
    ///
    /// # Errors
    ///
    /// [`ZbdError::ForeignIndex`] when `index` was not read from this
    /// container's own bytes or `table` carries another container's label,
    /// and [`ZbdError::Sound`] when the family gate or the listing refuses the
    /// container.
    pub fn sound_archive<'c>(
        &'c self,
        context: &mut ParseContext,
        index: &'c VersionOneIndex<'c>,
        table: &'c MemberTable<'c>,
    ) -> Result<SoundArchive<'c>, ZbdError> {
        self.require_own_index(index, table)?;
        Ok(read_sound_archive(context, table, index.data())?)
    }

    /// Refuses an index or table that describes another container's members.
    ///
    /// The index must slice exactly this container's bytes (its data starts
    /// at this container's first byte), and the table must carry this
    /// container's label, so extents read from one archive are never applied
    /// to another archive's bytes.
    fn require_own_index(
        &self,
        index: &VersionOneIndex<'_>,
        table: &MemberTable<'_>,
    ) -> Result<(), ZbdError> {
        let data = index.data();
        let own_bytes = std::ptr::eq(data.as_ptr(), self.bytes.as_ptr())
            && data.len() <= self.bytes.len()
            && index.container() == self.label;
        if !own_bytes {
            return Err(ZbdError::ForeignIndex {
                container: self.label.clone(),
                index_container: index.container().to_owned(),
            });
        }
        if table.container() != self.label {
            return Err(ZbdError::ForeignIndex {
                container: self.label.clone(),
                index_container: table.container().to_owned(),
            });
        }
        Ok(())
    }

    /// The sound assets of this container's sound archive.
    ///
    /// Every **readable** entry becomes a [`SoundAsset`] — identity, span,
    /// the WAVE header its member declares, and its [`SoundReadiness`]. An
    /// entry is decoded under **its own** header, so this stage never applies
    /// one entry's declaration to another's bytes.
    ///
    /// A member whose extent failed its bounds check is not a sound asset — it
    /// has no bytes at all — and is reported by [`SoundAssets::status`] and
    /// [`SoundAssets::failures`] instead, so nothing is silently dropped
    /// (IDENTITY-CONTENT: "collections cannot exclude failed entries"; spec F06
    /// non-negotiable #4).
    ///
    /// `index` is the [`VersionOneIndex`] [`Self::index`] returned; see
    /// [`Self::sound_archive`].
    ///
    /// # Errors
    ///
    /// Everything [`Self::sound_archive`] reports. A per-member decode
    /// failure is **not** an error here: it is that member's
    /// [`SoundReadiness`], so one bad member cannot hide its valid siblings.
    pub fn sound_assets<'c>(
        &'c self,
        context: &mut ParseContext,
        index: &'c VersionOneIndex<'c>,
        table: &'c MemberTable<'c>,
    ) -> Result<SoundAssets<'c>, ZbdError> {
        let archive = self.sound_archive(context, index, table)?;
        Ok(SoundAssets::new(context, archive))
    }
}

/// How many leading bytes of a container the documented header rules can look
/// at.
///
/// Task #340 added signature rules for the GameZ and animation families, so
/// this is the largest of the three `required_bytes()` values the inventory
/// holds. A family that documents no header is never handed more bytes to
/// fail on.
fn header_probe_bytes() -> usize {
    cs_formats::zbd::ZBD_FAMILY_INVENTORY
        .iter()
        .filter_map(|record| record.header_rule().signature())
        .map(|rule| rule.required_bytes())
        .max()
        .unwrap_or(0)
}

/// The installation-relative path a resolution describes.
///
/// The shared installation mount's container label is `.` (F04's designed
/// layout), in which case the member spelling already *is* the
/// installation-relative path; every other mount names a subdirectory, so the
/// two compose. Either way the composition is validated, never concatenated
/// unchecked.
fn installation_path(span: &SourceSpan) -> Result<RelativePath, ZbdError> {
    let container = span.container_path();
    let spelling = match span.member_key() {
        Some(member) if container == "." || container.is_empty() => member.to_owned(),
        Some(member) => format!("{container}/{member}"),
        None => container.to_owned(),
    };
    RelativePath::new(&spelling).map_err(|reason| ZbdError::ContainerPath {
        spelling: spelling.clone(),
        reason,
    })
}

/// What a sound member is ready for, from this stage's point of view.
///
/// Deliberately never "playable": spec F06 non-negotiable #4 forbids
/// advertising playability from a listing, and this stage has no audio device,
/// no mix and no loop playback (that is F41's job).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SoundReadiness {
    /// The member's own WAVE header declares a format this stage decodes, and
    /// its `data` payload is a whole number of declared frames: the member
    /// holds `frames` frames of `samples_per_frame` samples, accounting for
    /// every byte of the payload.
    Decoded {
        /// Whole declared frames the payload holds.
        frames: u64,
        /// Sample values one frame holds (one per declared channel).
        samples_per_frame: u64,
    },
    /// The member's WAVE header declares a format tag this stage does not
    /// decode. `tag` and `name` are the member's own, so a diagnostic can
    /// say what the member is; the row is never silently passed through.
    UnsupportedFormat {
        /// The `wFormatTag` the member declares.
        tag: u16,
        /// The name RFC 2361 gives the tag, when it gives one.
        name: Option<&'static str>,
    },
    /// The member's WAVE header did not read, so nothing about its samples is
    /// known. `reason` is the header reader's own explanation.
    UnreadableHeader {
        /// Why the member's header did not read.
        reason: &'static str,
    },
    /// The member's header declares a format this stage could plan for, but
    /// its `data` payload does not match it. `code` is the decode failure's
    /// stable code.
    Undecodable {
        /// The decode failure's stable code ([`SampleError::code`]).
        code: &'static str,
    },
}

impl SoundReadiness {
    /// Whether the member's own declared format was decoded.
    pub const fn is_decoded(&self) -> bool {
        matches!(self, Self::Decoded { .. })
    }
}

/// One sound member as the audio consumer sees it.
///
/// Carries the catalog fields this stage can state honestly from the member
/// itself — `id` (the member's numeric id, which the version-one index does
/// not declare, so `None` today), `kind` (always
/// [`cs_types::install::FileFamily::zbd_sound`], since the sound family is
/// what this route is for), `origin` (the member's
/// [`cs_types::evidence::SourceSpan`]) and `parse_state` — plus the WAVE
/// header its own bytes declare and its [`SoundReadiness`].
///
/// It deliberately does **not** carry the remaining IDENTITY-CONTENT catalog
/// fields: `dependencies` and `normalize_state` are not known here (a dynamic
/// lookup that cannot be bounded is an unresolved dependency, not proof of
/// none, and this stage normalizes nothing), and `runtime_consumers` and
/// `fingerprint` belong to the content catalog (F14). A field that cannot be
/// filled honestly is left out rather than filled with a zero that reads as a
/// measurement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundAsset<'a> {
    entry: SoundEntry<'a>,
    span: ByteSpan,
    parse_state: MemberStatus,
    readiness: SoundReadiness,
}

impl<'a> SoundAsset<'a> {
    /// The sound entry this asset was built from, in full.
    pub const fn entry(&self) -> &SoundEntry<'a> {
        &self.entry
    }

    /// Position of the member in the container's declared index.
    pub const fn index(&self) -> usize {
        self.entry.index()
    }

    /// The member's name bytes exactly as the index spells them.
    pub const fn name(&self) -> &'a [u8] {
        self.entry.name()
    }

    /// The member's numeric id, when its index declares one. The version-one
    /// index declares none, so this is `None` for every real sound archive.
    pub const fn id(&self) -> Option<u32> {
        self.entry.id()
    }

    /// Where the member lives inside the container.
    pub const fn span(&self) -> ByteSpan {
        self.span
    }

    /// The member's length in bytes.
    pub const fn byte_len(&self) -> u64 {
        self.entry.content().len() as u64
    }

    /// The member's bytes, exactly as the container stores them.
    pub const fn content(&self) -> &'a [u8] {
        self.entry.content()
    }

    /// The RIFF/WAVE header of this member, or the error that stopped it
    /// reading (task #344).
    pub fn wave(&self) -> Result<WaveHeader, WaveError> {
        self.entry.wave()
    }

    /// What the bounds check established for this member.
    pub const fn parse_state(&self) -> MemberStatus {
        self.parse_state
    }

    /// Whether this member is [`SoundReadiness::Decoded`].
    pub const fn readiness(&self) -> &SoundReadiness {
        &self.readiness
    }

    /// Decodes this member's samples under the format **its own** WAVE
    /// header declares.
    ///
    /// # Errors
    ///
    /// [`SampleError`] for a header that did not read, a format this stage
    /// does not decode, or a `data` payload that is not a whole number of
    /// declared frames.
    pub fn decode(
        &self,
        context: &mut ParseContext,
    ) -> Result<cs_formats::zbd::DecodedSound, SampleError> {
        let header = self.entry.wave().map_err(SampleError::from_wave_header)?;
        let format = cs_formats::zbd::SampleFormat::from_header(&header)?;
        decode_sound_sample(context, self.entry.content(), &format)
    }
}

/// The sound assets of one container, plus the strict status of the listing
/// they came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundAssets<'a> {
    archive: SoundArchive<'a>,
    entries: Vec<SoundAsset<'a>>,
    status: ContainerStatus,
    failures: usize,
    wave_failures: usize,
    unsupported: Vec<UnsupportedRecord<'a>>,
}

impl<'a> SoundAssets<'a> {
    /// Builds the assets of `archive`, decoding each member under its own
    /// WAVE header.
    ///
    /// Each entry is decoded on its own with one shared [`ParseContext`], so
    /// they share its budget and a starved one refuses them all without
    /// leaving a charge behind. One entry whose bytes contradict its header
    /// becomes [`SoundReadiness::Undecodable`] while its siblings stay
    /// decoded (spec F06 non-negotiable #4).
    fn new(context: &mut ParseContext, archive: SoundArchive<'a>) -> Self {
        let listing = archive.listing();
        let mut entries = Vec::with_capacity(listing.len());
        for index in 0..listing.len() {
            // A member that failed its bounds check has no bytes: it is not a
            // sound asset, and is reported as a listing failure instead.
            let Some(entry) = archive.entry(index) else {
                continue;
            };
            let row = listing
                .row(index)
                .expect("a listed index always has its own row");
            let readiness = match entry.wave() {
                Err(error) => SoundReadiness::UnreadableHeader {
                    reason: error.reason(),
                },
                Ok(header) => match cs_formats::zbd::SampleFormat::from_header(&header) {
                    Err(error) => match error {
                        cs_formats::zbd::SampleFormatError::UnsupportedFormat { tag, name } => {
                            SoundReadiness::UnsupportedFormat { tag, name }
                        }
                        other => SoundReadiness::Undecodable { code: other.code() },
                    },
                    Ok(format) => match decode_sound_sample(context, entry.content(), &format) {
                        Ok(sample) => SoundReadiness::Decoded {
                            frames: sample.frames(),
                            samples_per_frame: sample.samples_per_frame(),
                        },
                        Err(error) => SoundReadiness::Undecodable { code: error.code() },
                    },
                },
            };
            entries.push(SoundAsset {
                span: entry.span(),
                entry,
                parse_state: row.status(),
                readiness,
            });
        }
        Self {
            status: listing.status(),
            failures: listing.failures(),
            wave_failures: archive.wave_failures().count(),
            unsupported: archive.unsupported_records(),
            entries,
            archive,
        }
    }

    /// Provenance label of the container these assets came from.
    pub fn container(&self) -> &str {
        self.archive.container()
    }

    /// The sound archive behind these assets, for a consumer that wants the
    /// listing rather than the decoded rows.
    pub const fn archive(&self) -> &SoundArchive<'a> {
        &self.archive
    }

    /// How many sound assets the listing produced.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the container's index declared no readable entry.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every asset, in declared order.
    pub fn entries(&self) -> &[SoundAsset<'a>] {
        &self.entries
    }

    /// The asset at `index`, or `None` when the index is out of range or that
    /// member failed its bounds check.
    pub fn entry(&self, index: usize) -> Option<&SoundAsset<'a>> {
        self.entries.get(index)
    }

    /// The strict status of the listing behind these assets: the bounds of
    /// its members, as [`SoundArchive::status`] documents.
    pub const fn status(&self) -> ContainerStatus {
        self.status
    }

    /// How many declared members failed their bounds check. A failed member
    /// has no bytes and so is not an asset, but it is counted here so the
    /// collection cannot hide it.
    pub const fn failures(&self) -> usize {
        self.failures
    }

    /// How many members' WAVE headers did not read, so nothing about their
    /// samples is known. A strict audit (F06-D) must count this **and**
    /// [`Self::failures`], because neither is in [`Self::status`].
    pub const fn wave_failures(&self) -> usize {
        self.wave_failures
    }

    /// The bounded listing behind these assets: rows, consumed and uncovered
    /// ranges.
    pub fn listing(&self) -> &ArchiveListing<'a> {
        self.archive.listing()
    }

    /// Every entry the sound reader cannot interpret, with its span.
    pub fn unsupported_records(&self) -> &[UnsupportedRecord<'a>] {
        &self.unsupported
    }

    /// The members whose own declared format this stage decoded.
    pub fn decoded(&self) -> impl Iterator<Item = &SoundAsset<'a>> {
        self.entries
            .iter()
            .filter(|entry| entry.readiness.is_decoded())
    }
}

#[cfg(test)]
mod tests {
    //! Acceptance stage F06-C: the VFS producer and the sound assets it
    //! yields (`specs/F06-zbd-families-reader-archives-and-sound-containers.md`,
    //! section `### F06-C`).
    //!
    //! Every tree here is newly authored fixture data written under the system
    //! temporary directory: it proves nothing about a retail installation, it
    //! never touches `$CS_GAME_DIR`, and it is removed again when the test
    //! finishes (including on panic). The tests call production code only:
    //! `cs_assets::install::discover`, `crate::vfs::SessionBuilder`,
    //! `ContentSession`, the ZBD family readers and this module's
    //! `ZbdContainer` / `SoundAssets`.
    //!
    //! The inline shape is deliberate: `crates/cs_assets/tests/` is not an
    //! owner path of this task, and the same inline-test shape F04-C used in
    //! `tools/cs_inspect/src/resolve.rs` keeps the tests next to the code they
    //! exercise.

    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{SoundReadiness, ZbdContainer, ZbdError};
    use cs_formats::ParseContext;
    use cs_formats::zbd::{
        ContainerStatus, INDEX_ENTRY_BYTES, INDEX_NAME_BYTES, INDEX_UNEXPLAINED_BYTES,
        TRAILER_VERSION_ONE, WAVE_FORMAT_IMA_ADPCM, WAVE_FORMAT_MS_ADPCM, WAVE_FORMAT_PCM,
    };
    use cs_types::asset_id::{AssetKey, WorldGroup};

    /// A serial so parallel test binaries cannot collide on one name.
    static NEXT_TREE: AtomicU64 = AtomicU64::new(0);

    /// A disposable fixture directory, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "cs-f06-c-{label}-{}-{}",
                std::process::id(),
                NEXT_TREE.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("the fixture root is created");
            Self(root)
        }

        fn write(&self, spelling: &str, bytes: &[u8]) {
            let path = self.0.join(spelling);
            fs::create_dir_all(path.parent().expect("has a parent")).expect("dirs are created");
            fs::write(path, bytes).expect("fixture bytes are written");
        }

        fn edit_byte(&self, spelling: &str, index: usize, xor_mask: u8) {
            let path = self.0.join(spelling);
            let mut bytes = fs::read(&path).expect("fixture bytes are readable");
            bytes[index] ^= xor_mask;
            fs::write(path, bytes).expect("fixture bytes are written back");
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn key(namespace: &str, path: &str) -> AssetKey {
        AssetKey::from_spelling(namespace, path, "default").expect("fixture keys are valid")
    }

    /// Mounts `root` as one installation-wide content session.
    fn session(root: &Path) -> crate::vfs::ContentSession {
        let found = crate::install::discover(root).expect("the fixture tree is discoverable");
        let context =
            cs_types::asset_id::ResolveContext::new(crate::install::fingerprint(&found.manifest));
        let mut builder = crate::vfs::SessionBuilder::new(context);
        builder
            .mount_installation(root, &found.diagnosis)
            .expect("the fixture tree mounts");
        builder.open()
    }

    /// A session bound to one world group, as `world:` keys need.
    fn world_session(root: &Path, group: &str) -> crate::vfs::ContentSession {
        let found = crate::install::discover(root).expect("the fixture tree is discoverable");
        let context =
            cs_types::asset_id::ResolveContext::new(crate::install::fingerprint(&found.manifest))
                .with_world_group(WorldGroup::new(group).expect("a valid world group spelling"));
        let mut builder = crate::vfs::SessionBuilder::new(context);
        builder
            .mount_installation(root, &found.diagnosis)
            .expect("the fixture tree mounts");
        builder.open()
    }

    // --- authored RIFF/WAVE members ----------------------------------------

    /// One member's `fmt ` fields, as the Microsoft/IBM RIFF spec states them.
    struct Fmt {
        tag: u16,
        channels: u16,
        rate_hz: u32,
        bits_per_sample: u16,
    }

    impl Fmt {
        /// 16-bit mono PCM, the shape these tests decode in full.
        const fn pcm16() -> Self {
            Self {
                tag: WAVE_FORMAT_PCM,
                channels: 1,
                rate_hz: 22_050,
                bits_per_sample: 16,
            }
        }

        /// The IMA ADPCM shape task #344 measured most often in retail.
        const fn ima() -> Self {
            Self {
                tag: WAVE_FORMAT_IMA_ADPCM,
                channels: 1,
                rate_hz: 11_025,
                bits_per_sample: 4,
            }
        }

        /// The Microsoft ADPCM shape task #344 measured in retail.
        const fn ms() -> Self {
            Self {
                tag: WAVE_FORMAT_MS_ADPCM,
                channels: 1,
                rate_hz: 22_050,
                bits_per_sample: 4,
            }
        }

        /// `nBlockAlign` the fields imply.
        const fn block_align(&self) -> u16 {
            self.channels * (self.bits_per_sample / 8)
        }

        /// A format-specific `fmt ` tail, as ADPCM carries its coefficients.
        fn fmt_tail(&self) -> Vec<u8> {
            if self.tag == WAVE_FORMAT_PCM {
                Vec::new()
            } else {
                let mut tail = Vec::with_capacity(4);
                tail.extend_from_slice(&2u16.to_le_bytes());
                tail.extend_from_slice(&1u16.to_le_bytes());
                tail
            }
        }
    }

    /// Assembles a complete RIFF/WAVE member around `fmt` and a `data` payload.
    fn wave_member(fmt: &Fmt, data: &[u8]) -> Vec<u8> {
        let mut fmt_payload = Vec::new();
        fmt_payload.extend_from_slice(&fmt.tag.to_le_bytes());
        fmt_payload.extend_from_slice(&fmt.channels.to_le_bytes());
        fmt_payload.extend_from_slice(&fmt.rate_hz.to_le_bytes());
        fmt_payload.extend_from_slice(&(fmt.rate_hz * u32::from(fmt.block_align())).to_le_bytes());
        fmt_payload.extend_from_slice(&fmt.block_align().to_le_bytes());
        fmt_payload.extend_from_slice(&fmt.bits_per_sample.to_le_bytes());
        fmt_payload.extend_from_slice(&fmt.fmt_tail());

        let mut chunks = Vec::new();
        chunks.extend_from_slice(b"fmt ");
        chunks.extend_from_slice(&(fmt_payload.len() as u32).to_le_bytes());
        chunks.extend_from_slice(&fmt_payload);
        if fmt_payload.len() % 2 == 1 {
            chunks.push(0);
        }
        chunks.extend_from_slice(b"data");
        chunks.extend_from_slice(&(data.len() as u32).to_le_bytes());
        chunks.extend_from_slice(data);
        if data.len() % 2 == 1 {
            chunks.push(0);
        }

        let mut member = Vec::with_capacity(12 + chunks.len());
        member.extend_from_slice(b"RIFF");
        member.extend_from_slice(&((chunks.len() + 4) as u32).to_le_bytes());
        member.extend_from_slice(b"WAVE");
        member.extend_from_slice(&chunks);
        member
    }

    /// A 16-bit mono member holding `RAMP`.
    const RAMP: [i16; 8] = [0, 1, 2, 3, -1, -2, -3, -4];

    fn pcm16_member() -> Vec<u8> {
        let mut data = Vec::with_capacity(RAMP.len() * 2);
        for sample in RAMP {
            data.extend_from_slice(&sample.to_le_bytes());
        }
        wave_member(&Fmt::pcm16(), &data)
    }

    // --- authored version-one archives -------------------------------------

    /// One index entry: u32 start, u32 length, a 64-byte NUL-padded name and
    /// 76 bytes the pinned source reads without explaining (task #343).
    fn index_entry(start: u32, length: u32, name: &[u8]) -> Vec<u8> {
        assert!(
            name.len() < INDEX_NAME_BYTES,
            "a fixture name fits its field"
        );
        let mut entry = Vec::with_capacity(INDEX_ENTRY_BYTES as usize);
        entry.extend_from_slice(&start.to_le_bytes());
        entry.extend_from_slice(&length.to_le_bytes());
        let mut name_field = vec![0u8; INDEX_NAME_BYTES];
        name_field[..name.len()].copy_from_slice(name);
        entry.extend_from_slice(&name_field);
        entry.extend_from_slice(&[0xA5; INDEX_UNEXPLAINED_BYTES]);
        entry
    }

    /// Wraps member bodies in a version-one trailer index, exactly as task
    /// #343's reader expects.
    fn archive(members: &[(&[u8], Vec<u8>)]) -> Vec<u8> {
        let mut data = Vec::new();
        let mut entries = Vec::new();
        for (name, body) in members {
            let start = u32::try_from(data.len()).expect("a fixture body fits u32");
            let length = u32::try_from(body.len()).expect("a fixture body fits u32");
            entries.extend_from_slice(&index_entry(start, length, name));
            data.extend_from_slice(body);
        }
        let mut bytes = data;
        bytes.extend_from_slice(&entries);
        bytes.extend_from_slice(&TRAILER_VERSION_ONE.to_le_bytes());
        bytes.extend_from_slice(
            &u32::try_from(members.len())
                .expect("count fits")
                .to_le_bytes(),
        );
        bytes
    }

    /// The sound archive the fixtures mount, and a reader archive beside it.
    fn installation() -> Temp {
        let tree = Temp::new("install");
        // An observed sound archive name (task #340): three members, a
        // decodable PCM one and two compressed ones this stage refuses.
        tree.write(
            "ZBD/soundsl.zbd",
            &archive(&[
                (b"ramp.wav".as_slice(), pcm16_member()),
                (b"gun.wav".as_slice(), wave_member(&Fmt::ima(), &[0u8; 256])),
                (b"loop.wav".as_slice(), wave_member(&Fmt::ms(), &[0u8; 512])),
            ]),
        );
        // An observed reader archive name, whose body is not WAVE at all: it
        // is never read as sound, and never decoded.
        tree.write(
            "ZBD/c1/zrdr.zbd",
            &archive(&[(b"chapter_one".as_slice(), b"reader bytes here".to_vec())]),
        );
        // Not a ZBD archive at all.
        tree.write("README.txt", b"not a zbd container");
        tree
    }

    // --- the producer ------------------------------------------------------

    #[test]
    fn accept_f06_c_an_observed_sound_container_routes_to_the_sound_family_and_reads_its_own_index()
    {
        // The stage's observable failure, positive half: an observed sound
        // archive name is routed to the **sound** family by the inventory's
        // role rule, and its member index comes out of its **own** trailer.
        let tree = installation();
        let session = session(tree.0.as_path());
        let sounds = key("install", "ZBD/soundsl.zbd");

        let container =
            ZbdContainer::open(&session, &sounds).expect("the sound container resolves and reads");

        // The provenance is the session's, not a re-derived one.
        assert_eq!(container.key(), &sounds);
        assert_eq!(container.mount().as_str(), "install");
        assert_eq!(container.span().container_path(), ".");
        assert_eq!(container.span().member_key(), Some("ZBD/soundsl.zbd"));
        assert_eq!(container.generation(), session.generation());
        container
            .require_session(&session)
            .expect("the reading session is its own");

        // The installation-relative path is rebuilt from the resolution: the
        // shared install mount's container label is `.`, so the member spelling
        // already is the installation-relative path, and the role rules match
        // against *that* rather than the bare `soundsl.zbd`.
        assert_eq!(container.path().as_str(), "ZBD/soundsl.zbd");

        // Task #340's observed name routed it to the sound family.
        assert_eq!(container.family(), cs_formats::zbd::ZbdFamily::Sound);
        assert_eq!(container.reader(), cs_formats::zbd::ZbdReaderId::Sound);
        let routing = *container.routing();
        assert!(routing.is_routed());
        assert_eq!(
            routing,
            super::ZbdRouting::Routed {
                family: cs_formats::zbd::ZbdFamily::Sound,
                reader: cs_formats::zbd::ZbdReaderId::Sound,
                basis: cs_formats::zbd::DispatchBasis::RoleOnly,
                header_status: match routing {
                    super::ZbdRouting::Routed { header_status, .. } => header_status,
                    _ => unreachable!("the routing is routed"),
                },
                role_status: match routing {
                    super::ZbdRouting::Routed { role_status, .. } => role_status,
                    _ => unreachable!("the routing is routed"),
                },
            }
        );
        // No sound header rule is documented, so the bytes stay unvalidated.
        assert!(matches!(
            routing,
            super::ZbdRouting::Routed {
                header_status: cs_formats::zbd::HeaderStatus::Unvalidated { .. },
                ..
            }
        ));

        // The member index is the archive's **own** trailer: three members,
        // named as the index spells them, with the 76 unexplained bytes kept
        // but unread.
        let mut context = ParseContext::with_defaults(container.label());
        let index = container.index(&mut context).expect("the trailer reads");
        assert_eq!(index.family(), cs_formats::zbd::ZbdFamily::Sound);
        assert_eq!(index.version(), TRAILER_VERSION_ONE);
        assert_eq!(index.len(), 3);
        assert_eq!(index.entry(0).expect("entry 0").name(), b"ramp.wav");
        assert_eq!(index.entry(1).expect("entry 1").name(), b"gun.wav");
        assert_eq!(index.entry(2).expect("entry 2").name(), b"loop.wav");
        assert_eq!(index.extents().len(), 3);
        for entry in index.entries() {
            assert_eq!(
                entry.unexplained().bytes().len(),
                INDEX_UNEXPLAINED_BYTES,
                "the unexplained bytes are retained, not dropped"
            );
            assert_eq!(
                entry.unexplained().reason(),
                cs_formats::zbd::UNEXPLAINED_REASON,
                "the 76 bytes are labelled with the trailer reader's own reason"
            );
        }
    }

    #[test]
    fn accept_f06_c_a_container_no_key_names_is_refused_rather_than_read() {
        // "No key names a family" must mean "this is not a ZBD container this
        // stage may read", never "read it as something else".
        let tree = installation();
        let session = session(tree.0.as_path());
        let error = ZbdContainer::open(&session, &key("install", "README.txt"))
            .expect_err("a file outside `zbd/` names no family");
        assert_eq!(error.code(), "dispatch");
        let ZbdError::Dispatch(dispatch) = &error else {
            panic!("expected a dispatch refusal, got {error:?}")
        };
        assert_eq!(dispatch.code(), "unknown_family");
        assert!(std::error::Error::source(&error).is_some());
    }

    #[test]
    fn accept_f06_c_a_reader_archive_is_never_read_as_sound() {
        // The mirror of the stage's observable failure: `zrdr.zbd` routes to
        // the **reader** family, and its bytes are not WAVE, so reading it as
        // sound would produce assets for a family the dispatch did not name.
        let tree = installation();
        let session = world_session(tree.0.as_path(), "zbd/c1");
        let container =
            ZbdContainer::open(&session, &key("world", "zrdr.zbd")).expect("the container opens");
        assert_eq!(container.family(), cs_formats::zbd::ZbdFamily::Reader);
        assert_eq!(container.reader(), cs_formats::zbd::ZbdReaderId::Reader);
        // The world mount's container label keeps the installation's own
        // spelling, so the composed path does too.
        assert_eq!(container.path().as_str(), "ZBD/c1/zrdr.zbd");

        let mut context = ParseContext::with_defaults(container.label());
        let index = container
            .index(&mut context)
            .expect("its own trailer reads");
        let table = index.member_table();
        let error = container
            .sound_archive(&mut context, &index, &table)
            .expect_err("a reader archive is not a sound archive");
        assert_eq!(error.code(), "family_mismatch");
        let text = error.to_string();
        assert!(text.contains("reader"), "{text}");
        assert!(text.contains("sound"), "{text}");

        // And the sound asset path refuses the same way.
        let error = container
            .sound_assets(&mut context, &index, &table)
            .expect_err("and produces no sound assets");
        assert_eq!(error.code(), "family_mismatch");
    }

    #[test]
    fn accept_f06_c_an_unresolvable_key_and_a_changed_member_both_refuse_with_their_own_code() {
        let tree = installation();
        let session = session(tree.0.as_path());

        // A key no mount holds: the resolve failure propagates.
        let missing = key("install", "ZBD/absent.zbd");
        let error = ZbdContainer::open(&session, &missing).expect_err("an absent key is refused");
        assert_eq!(error.code(), "resolve");
        assert!(matches!(error, ZbdError::Resolve(_)));
        assert!(std::error::Error::source(&error).is_some());

        // A member whose bytes changed after the mount hashed them: the
        // digest check refuses, so an archive is never read from bytes the
        // mount did not vouch for.
        tree.edit_byte("ZBD/soundsl.zbd", 0, 0x20);
        let error = ZbdContainer::open(&session, &key("install", "ZBD/soundsl.zbd"))
            .expect_err("changed bytes are refused");
        assert_eq!(error.code(), "read");
        let text = error.to_string();
        assert!(text.contains("hashed"), "{text}");
    }

    #[test]
    fn accept_f06_c_a_resolution_whose_path_would_escape_is_refused_before_dispatch() {
        // The installation-relative path is a *composition* of a mount's
        // container label and a member spelling, so it is validated like any
        // other untrusted spelling (IDENTITY-CONTENT: "no unchecked path
        // join"). A mount labelled with `..` composes to an escaping path.
        let tree = installation();
        let found = crate::install::discover(tree.0.as_path()).expect("the tree is discoverable");
        let context =
            cs_types::asset_id::ResolveContext::new(crate::install::fingerprint(&found.manifest));
        let mut builder = crate::vfs::SessionBuilder::new(context);
        // The container label is provenance, not a path to read from, so the
        // mount itself is accepted — which is exactly why the composition has
        // to be re-checked on the way out.
        builder
            .mount(hostile_label_mount(tree.0.as_path()))
            .expect("the mount itself is accepted");
        let session = builder.open();

        let error = ZbdContainer::open(&session, &key("hostile", "ZBD/soundsl.zbd"))
            .expect_err("an escaping composed path is refused");
        assert_eq!(error.code(), "container_path");
        let ZbdError::ContainerPath { spelling, reason } = &error else {
            panic!("expected a container-path refusal, got {error:?}")
        };
        assert_eq!(spelling, "../escape/ZBD/soundsl.zbd");
        assert_eq!(
            *reason,
            cs_types::install::RelativePathError::ParentComponent
        );
        let text = error.to_string();
        assert!(text.contains(".."), "{text}");
    }

    /// A directory mount whose container label is `../escape`, so a hostile
    /// *label* reaches a real session.
    fn hostile_label_mount(root: &Path) -> crate::vfs::Mount {
        use crate::vfs::MountBuilder;
        use cs_types::asset_id::{MountId, MountNamespace, PrecedenceClass};
        let builder = MountBuilder::new(
            MountId::new("hostile").expect("a valid mount id"),
            MountNamespace::new("hostile").expect("a valid namespace"),
            PrecedenceClass::Shared,
            "../escape",
        );
        match crate::vfs::source::mount_directory(builder, root) {
            Ok(mounted) => mounted.mount,
            Err(error) => panic!("the fixture tree mounts: {error}"),
        }
    }

    #[test]
    fn accept_f06_c_an_index_read_from_another_container_is_refused() {
        // The caller holds the index and the table, so the container must
        // refuse extents that were read from someone else's bytes rather
        // than apply them to its own.
        let tree = installation();
        tree.write(
            "ZBD/soundsh.zbd",
            &archive(&[(b"other.wav".as_slice(), pcm16_member())]),
        );
        let session = session(tree.0.as_path());
        let low =
            ZbdContainer::open(&session, &key("install", "ZBD/soundsl.zbd")).expect("it opens");
        let high =
            ZbdContainer::open(&session, &key("install", "ZBD/soundsh.zbd")).expect("it opens");

        let mut context = ParseContext::with_defaults(low.label());
        let low_index = low.index(&mut context).expect("its trailer reads");
        let low_table = low_index.member_table();
        let high_index = high.index(&mut context).expect("its trailer reads");
        let high_table = high_index.member_table();

        // Another container's index, with its own table.
        let error = high
            .sound_assets(&mut context, &low_index, &low_table)
            .expect_err("a foreign index is refused");
        assert_eq!(error.code(), "foreign_index");
        let text = error.to_string();
        assert!(text.contains("soundsl.zbd"), "{text}");

        // This container's index, with another container's table.
        let error = high
            .sound_archive(&mut context, &high_index, &low_table)
            .expect_err("a foreign table is refused");
        assert_eq!(error.code(), "foreign_index");

        // Its own index and table read.
        let assets = high
            .sound_assets(&mut context, &high_index, &high_table)
            .expect("its own index reads");
        assert_eq!(assets.len(), 1);
        assert_eq!(assets.entry(0).expect("row 0").name(), b"other.wav");
    }

    // --- the consumer: sound assets ----------------------------------------

    #[test]
    fn accept_f06_c_a_sound_container_becomes_audio_assets_with_the_samples_it_declares() {
        // The stage's minimum scenario, end to end through a mounted content
        // session: an observed sound archive is dispatched, indexed from its
        // own trailer, read, and each member's samples are decoded under the
        // format **its own** WAVE header declares.
        let tree = installation();
        let session = session(tree.0.as_path());
        let container =
            ZbdContainer::open(&session, &key("install", "ZBD/soundsl.zbd")).expect("it opens");

        let mut context = ParseContext::with_defaults(container.label());
        let index = container.index(&mut context).expect("its trailer reads");
        let table = index.member_table();
        let assets = container
            .sound_assets(&mut context, &index, &table)
            .expect("the sound archive is read");

        assert_eq!(assets.len(), 3);
        assert_eq!(assets.status(), ContainerStatus::Clean);
        assert_eq!(assets.failures(), 0);
        assert_eq!(
            assets.wave_failures(),
            0,
            "every fixture member is a readable WAVE file"
        );

        // The decoded member: the byte and sample counts are the ones its own
        // header implies, and the values are its stored samples.
        let ramp = assets.entry(0).expect("row 0 is an asset");
        assert_eq!(ramp.name(), b"ramp.wav");
        assert_eq!(ramp.byte_len(), pcm16_member().len() as u64);
        assert_eq!(
            ramp.readiness(),
            &SoundReadiness::Decoded {
                frames: 8,
                samples_per_frame: 1
            }
        );
        let header = ramp.wave().expect("its header reads");
        assert_eq!(header.format_tag(), WAVE_FORMAT_PCM);
        let mut context = ParseContext::with_defaults(container.label());
        let decoded = ramp.decode(&mut context).expect("its declared PCM decodes");
        assert_eq!(decoded.frames(), 8);
        assert_eq!(decoded.sample_count(), 8);
        assert_eq!(decoded.byte_len(), header.data_span().length);
        assert_eq!(
            decoded.byte_len(),
            decoded.sample_count() * 2,
            "byte count = sample count * the header's bytes per sample"
        );
        for (index, expected) in RAMP.iter().enumerate() {
            assert_eq!(
                decoded.samples()[index],
                i32::from(*expected),
                "sample {index}"
            );
        }

        // The two compressed members are refused with the tag **they**
        // declare, so the rows are visible and honest rather than passed
        // through as if they were PCM.
        let ima = assets.entry(1).expect("row 1 is an asset");
        assert_eq!(ima.name(), b"gun.wav");
        assert_eq!(
            ima.readiness(),
            &SoundReadiness::UnsupportedFormat {
                tag: WAVE_FORMAT_IMA_ADPCM,
                name: Some("ima_adpcm")
            }
        );
        let ms = assets.entry(2).expect("row 2 is an asset");
        assert_eq!(ms.name(), b"loop.wav");
        assert_eq!(
            ms.readiness(),
            &SoundReadiness::UnsupportedFormat {
                tag: WAVE_FORMAT_MS_ADPCM,
                name: Some("ms_adpcm")
            }
        );
        // And decoding one directly refuses with the same typed error.
        let error = ima
            .decode(&mut context)
            .expect_err("a compressed member is not decoded by this stage");
        assert_eq!(error.code(), "unsupported_format");

        assert_eq!(
            assets.decoded().count(),
            1,
            "only the PCM member is decoded"
        );
    }

    #[test]
    fn accept_f06_c_a_member_whose_header_does_not_read_is_a_row_with_its_own_reason() {
        // A member that is not RIFF at all: nothing about its samples is
        // known, the row says so with the header reader's own reason, and its
        // siblings stay decoded (spec F06 non-negotiable #4).
        let tree = Temp::new("install");
        tree.write(
            "ZBD/soundsl.zbd",
            &archive(&[
                (b"ramp.wav".as_slice(), pcm16_member()),
                (
                    b"broken.dat".as_slice(),
                    b"NOTRIFFxx not a wave file at all".to_vec(),
                ),
                (b"short.dat".as_slice(), b"RI".to_vec()),
            ]),
        );
        let session = session(tree.0.as_path());
        let container =
            ZbdContainer::open(&session, &key("install", "ZBD/soundsl.zbd")).expect("it opens");

        let mut context = ParseContext::with_defaults(container.label());
        let index = container.index(&mut context).expect("its trailer reads");
        let table = index.member_table();
        let assets = container
            .sound_assets(&mut context, &index, &table)
            .expect("one unreadable header does not fail the listing");

        assert_eq!(assets.len(), 3);
        assert_eq!(assets.wave_failures(), 2, "both broken members are counted");
        assert_eq!(
            assets.decoded().count(),
            1,
            "the readable sibling still decodes"
        );
        // The reasons are the header reader's own, quoted per member.
        let not_riff = cs_formats::zbd::read_wave_header(b"NOTRIFFxx not a wave file at all")
            .expect_err("the body is not a RIFF file");
        let too_short = cs_formats::zbd::read_wave_header(b"RI")
            .expect_err("the body is shorter than a header");
        assert_eq!(
            assets.entry(1).expect("row 1").readiness(),
            &SoundReadiness::UnreadableHeader {
                reason: not_riff.reason()
            }
        );
        assert_eq!(
            assets.entry(2).expect("row 2").readiness(),
            &SoundReadiness::UnreadableHeader {
                reason: too_short.reason()
            }
        );
    }

    #[test]
    fn accept_f06_c_a_member_reaching_into_the_index_fails_its_own_row_only() {
        // The readers are handed the bytes *before* the index, so a member
        // whose extent reaches into the index fails its own bounds check and
        // nothing else (task #343, through the F06-C wiring). The asset
        // collection must count the failure rather than hide it, and its valid
        // siblings stay decoded (spec F06 non-negotiable #4).
        let tree = Temp::new("install");
        let good = pcm16_member();
        // The lying extent runs past the end of the member data (where the
        // index begins) but not past the whole archive, so handing the
        // readers the *whole* file instead of `VersionOneIndex::data()` would
        // silently accept it.
        let lying = index_entry(
            0,
            u32::try_from(good.len()).expect("a fixture member fits u32") + 100,
            b"lying.wav",
        );
        let honest = index_entry(
            0,
            u32::try_from(good.len()).expect("a fixture member fits u32"),
            b"honest.wav",
        );
        let mut bytes = good.clone();
        bytes.extend_from_slice(&lying);
        bytes.extend_from_slice(&honest);
        bytes.extend_from_slice(&TRAILER_VERSION_ONE.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        tree.write("ZBD/soundsl.zbd", &bytes);

        let session = session(tree.0.as_path());
        let container =
            ZbdContainer::open(&session, &key("install", "ZBD/soundsl.zbd")).expect("it opens");
        let mut context = ParseContext::with_defaults(container.label());
        let index = container.index(&mut context).expect("its trailer reads");
        let table = index.member_table();
        let assets = container
            .sound_assets(&mut context, &index, &table)
            .expect("one member reaching into the index does not fail the listing");

        // The lying member has no bytes, so it is not an asset, but it is
        // counted.
        assert_eq!(index.len(), 2, "the index declares both members");
        assert_eq!(
            assets.len(),
            1,
            "only the member with real bytes is an asset"
        );
        assert_eq!(assets.failures(), 1);
        assert_eq!(assets.status(), ContainerStatus::Failed { failures: 1 });
        assert_eq!(assets.entry(0).expect("row 0").name(), b"honest.wav");
        assert!(assets.entry(1).is_none(), "the lying member has no bytes");
        // Row 0 is the lying member and keeps the bounds failure with its own
        // code; row 1 is the honest one and is readable.
        let rows = assets.listing().rows();
        assert_eq!(rows[0].name(), b"lying.wav");
        assert_eq!(
            rows[0].error().map(|error| error.code()),
            Some("member_out_of_bounds"),
            "its row keeps the bounds failure with its own code"
        );
        assert!(rows[1].is_readable());
        assert_eq!(rows[1].name(), b"honest.wav");
        // And the honest sibling is untouched.
        assert_eq!(assets.decoded().count(), 1);
    }

    // --- teardown, retry and stale state -----------------------------------

    #[test]
    fn accept_f06_c_a_container_outlives_its_session_and_is_refused_by_another() {
        // Teardown: the container owns its bytes, so closing the session that
        // read it leaves it and its assets readable (spec F04 non-negotiable
        // behavior 4, "owned backing storage, not dangling file handles").
        let tree = installation();
        let session = session(tree.0.as_path());
        let generation = session.generation();
        let container =
            ZbdContainer::open(&session, &key("install", "ZBD/soundsl.zbd")).expect("it opens");

        let mut context = ParseContext::with_defaults(container.label());
        let index = container.index(&mut context).expect("its trailer reads");
        let table = index.member_table();
        let assets = container
            .sound_assets(&mut context, &index, &table)
            .expect("the sound archive is read");
        assert_eq!(assets.decoded().count(), 1);

        // The teardown.
        let teardown = session.close();
        assert_eq!(teardown.generation, generation);

        // After it, the container and its assets are still usable: the bytes
        // are the container's own, not a file handle into a closed session.
        assert_eq!(container.generation(), generation);
        assert_eq!(assets.len(), 3);
        let mut context = ParseContext::with_defaults(container.label());
        let decoded = assets
            .entry(0)
            .expect("the asset survives teardown")
            .decode(&mut context)
            .expect("and still decodes");
        assert_eq!(decoded.sample_count(), 8);

        // But a *replacement* session refuses it: the world switched, so an
        // archive resolved for the old one is not a member of the new one.
        let next = session_again(tree.0.as_path());
        assert_ne!(next.generation(), generation);
        let error = container
            .require_session(&next)
            .expect_err("a replaced session refuses the previous generation");
        assert_eq!(error.code(), "read");
        let text = error.to_string();
        assert!(text.contains("session#"), "{text}");
    }

    /// Opens a second session over the same tree, so its generation differs.
    fn session_again(root: &Path) -> crate::vfs::ContentSession {
        session(root)
    }

    #[test]
    fn accept_f06_c_a_listing_refused_by_a_starved_budget_can_be_retried() {
        // Retry: a refused listing leaves the starved ledger untouched, so the
        // same container and the same index read on a funded context (spec
        // F03-C's rollback, through the F06-C wiring).
        let tree = installation();
        let session = session(tree.0.as_path());
        let container =
            ZbdContainer::open(&session, &key("install", "ZBD/soundsl.zbd")).expect("it opens");

        // A context with no budget at all refuses the index.
        let mut starved = ParseContext::new(container.label(), 0, 32);
        let error = container
            .index(&mut starved)
            .expect_err("a starved budget refuses the index");
        // The index reports the parse's own failure code; the budget kind it
        // carries is the allocation budget.
        assert_eq!(error.code(), "parse");
        let ZbdError::Index(index) = &error else {
            panic!("expected an index failure, got {error:?}")
        };
        assert!(matches!(
            index,
            cs_formats::zbd::IndexError::Parse(parse)
                if parse.kind == cs_formats::ParseErrorKind::AllocationBudgetExceeded
        ));
        assert!(std::error::Error::source(&error).is_some());
        assert_eq!(starved.allocation().used(), 0, "the refusal is rolled back");
        assert_eq!(starved.recursion().depth(), 0);

        // The same container, a funded context: it reads.
        let mut funded = ParseContext::with_defaults(container.label());
        let index = container
            .index(&mut funded)
            .expect("a funded context reads the trailer");
        assert_eq!(index.len(), 3);
        assert!(
            funded.allocation().used() > 0,
            "a successful index is charged"
        );
    }
}
