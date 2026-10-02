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
//!   member declares, and its [`SoundReadiness`]. The samples a member yields
//!   come from its **own** declaration, the two block codecs the retail
//!   archives use included (task #444).
//!
//! * **the corpus audit (stage F06-D).** [`audit_container`] and
//!   [`audit_containers`] run the producer over every ZBD container of a
//!   session and give each container and each member of the sound and reader
//!   families a row: `decoded`, `readable` (sound, not interpreted, with the
//!   reason) or `failed` (with a stable code). A corrupt container or member
//!   is a row beside its valid siblings, never a reason to stop (spec F06
//!   non-negotiable #4, AC04); [`ZbdAudit::passes`] is the strict status the
//!   `cs-inspect zbd-audit` command exits with.
//!
//! # What this stage does not do
//!
//! * It does not decode anything `cs_formats` does not read, and it never
//!   approximates. Task #344 measured that every retail member is IMA ADPCM, MS
//!   ADPCM or PCM; task #444 added the two block layouts, and this stage plans
//!   a member with [`SampleFormat::from_member`] /
//!   [`SampleFormat::from_header_with_blocks`], so a compressed member is
//!   decoded under the block geometry its own `fmt ` payload declares. A member
//!   that declares a tag nothing here reads, or a declaration the block
//!   geometry contradicts, is refused with that refusal's own stable code —
//!   never passed through as if it were PCM.
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
    MemberTable, ReaderArchive, ReaderError, RoleStatus, RoutableFamily, SampleError, SampleFormat,
    SampleFormatError, SoundArchive, SoundEntry, SoundError, UnsupportedRecord, VersionOneIndex,
    WaveError, WaveHeader, ZbdDispatch, ZbdDispatchError, ZbdFamily, ZbdProbe, ZbdReaderId,
    decode_sound_sample, dispatch, read_reader_archive, read_sound_archive, read_version_one_index,
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

    /// Reads the reader archive of a container the two keys routed to the
    /// reader family, from the member index its own trailer declares
    /// (stage F06-D: the corpus audit lists reader members through the same
    /// producer as sound members).
    ///
    /// `index` and `table` are as in [`Self::sound_archive`].
    ///
    /// # Errors
    ///
    /// [`ZbdError::ForeignIndex`] when `index` was not read from this
    /// container's own bytes or `table` carries another container's label,
    /// and [`ZbdError::Reader`] when the family gate or the listing refuses the
    /// container.
    pub fn reader_archive<'c>(
        &'c self,
        context: &mut ParseContext,
        index: &'c VersionOneIndex<'c>,
        table: &'c MemberTable<'c>,
    ) -> Result<ReaderArchive<'c>, ZbdError> {
        self.require_own_index(index, table)?;
        Ok(read_reader_archive(context, table, index.data())?)
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
    /// holds `frames` frames of `samples_per_frame` samples.
    ///
    /// For an uncompressed member a frame is one sample per declared channel.
    /// For a **block-coded** member — what task #344 measured for almost every
    /// retail member — a frame is one whole block: `frames` counts the blocks
    /// the payload holds and `samples_per_frame` is that block's own
    /// `wSamplesPerBlock` for each of its channels. Both numbers are the
    /// declaration's, never this stage's.
    Decoded {
        /// Whole declared frames the payload holds.
        frames: u64,
        /// Sample values one frame holds (one per declared channel).
        samples_per_frame: u64,
    },
    /// Nothing this stage can read stands behind the member's own `wFormatTag`,
    /// so no plan was built for it. `tag` and `name` are the member's own, so a
    /// diagnostic can say what the member is; the row is never silently passed
    /// through.
    ///
    /// A member lands here in either of two ways, and the variant does not
    /// pretend to tell them apart: a tag nothing in this stage decodes at all
    /// (`cs_formats`' `SampleFormat::UnsupportedFormat`), and a block codec this
    /// stage **does** decode whose `fmt ` payload carries no extension bytes, so
    /// no `wSamplesPerBlock` can be read and no block layout can be planned
    /// (`cs_formats` reports an absent extension for a tag it does not read as
    /// `UnsupportedFormat` too). Both keep the member's tag; a consumer that
    /// needs to tell an unknown tag from a codec with no geometry must re-read
    /// the member's `fmt ` payload, not this row alone.
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
    /// its `data` payload does not match it. `code` is the refusal's stable
    /// code: a declaration the block geometry contradicts, a block that cannot
    /// be read, or a payload that is not a whole number of declared frames.
    Undecodable {
        /// The refusal's stable code ([`SampleFormatError::code`] for a
        /// declaration, [`SampleError::code`] for a payload).
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

    /// Decodes this member's samples under the format **its own** WAVE header
    /// declares, the `fmt ` extension a block-coded member carries included.
    ///
    /// # Errors
    ///
    /// [`SampleError`] for a header that did not read, a format nothing in this
    /// stage reads, a declaration the block geometry contradicts, or a `data`
    /// payload the declaration does not account for.
    pub fn decode(
        &self,
        context: &mut ParseContext,
    ) -> Result<cs_formats::zbd::DecodedSound, SampleError> {
        let header = self.entry.wave().map_err(SampleError::from_wave_header)?;
        let format = sample_plan(&header, self.entry.content())?;
        decode_sound_sample(context, self.entry.content(), &format)
    }
}

/// The decode plan `header` and the `fmt ` payload it located declare.
///
/// Task #444 taught `cs_formats` the two block layouts the retail archives
/// declare, and their per-member values (`wSamplesPerBlock`, and the Microsoft
/// coefficient table) live in the `fmt ` extension rather than in the header's
/// sixteen common fields. This is where a member of this stage gets its plan:
/// the header is the one its own bytes were read into, and the payload that
/// header located is sliced out of those same bytes, so no value and no extent
/// comes from a caller.
fn sample_plan(header: &WaveHeader, member: &[u8]) -> Result<SampleFormat, SampleFormatError> {
    match fmt_payload(member, header) {
        Some(fmt) => SampleFormat::from_header_with_blocks(header, fmt),
        // A header whose `fmt ` span no slice of the member holds: the member's
        // own entry point reports that with its own typed reason, so the row
        // keeps it instead of guessing at one.
        None => SampleFormat::from_member(member),
    }
}

/// The `fmt ` payload `header` located, in the member's own bytes.
///
/// `None` when the header did not read a span that slice can hold, which is the
/// same condition [`SampleFormat::from_member`] refuses on.
fn fmt_payload<'m>(member: &'m [u8], header: &WaveHeader) -> Option<&'m [u8]> {
    let span = header.fmt_span();
    let start = usize::try_from(span.offset).ok()?;
    let length = usize::try_from(span.length).ok()?;
    member.get(start..start.checked_add(length)?)
}

/// Whether the member's own declaration is one of the block codecs.
///
/// A member only reaches this with a plan that built, so the `false` arm is
/// unreachable for a decoded row; it exists so the caller keeps a total match.
fn is_block_coded(header: &WaveHeader, member: &[u8]) -> bool {
    sample_plan(header, member)
        .map(|format| format.layout().is_block_coded())
        .unwrap_or(false)
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
    /// WAVE header and its own `fmt ` extension.
    ///
    /// Each entry is decoded on its own under a [`ParseContext`] that carries
    /// the caller's own limits, and one entry whose bytes contradict its header
    /// becomes [`SoundReadiness::Undecodable`] while its siblings stay decoded
    /// (spec F06 non-negotiable #4).
    ///
    /// # Why one context per member and not one for the listing
    ///
    /// A listing only keeps each member's **counts**; the decoded values are
    /// dropped as soon as they have been accounted for, while a charge on a
    /// shared ledger is only ever released by a failed attempt. Charging every
    /// member of a container to one ledger therefore makes a listing cost the
    /// sum of every member's decoded samples, which no per-parse budget is meant
    /// to cover. Measured on the installation task #344 fingerprinted: the two
    /// retail sound archives hold 5,041 members whose decoded values come to
    /// 1,418 MiB, 22x the 64 MiB per-parse default, while the **largest single
    /// member** decodes to 22 MiB and fits. So each member is bounded by the
    /// caller's own per-parse ceiling, exactly as before, and the listing's own
    /// cost stays proportional to the archive's stored bytes rather than to a
    /// budget a caller has to keep enlarging.
    ///
    /// The limits are inherited, never widened: a caller that starves its
    /// context still starves every member, and a refused member still leaves no
    /// charge behind.
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
            let mut member_context = ParseContext::new(
                context.container(),
                context.allocation().limit(),
                context.recursion().max_depth(),
            );
            let readiness = match entry.wave() {
                Err(error) => SoundReadiness::UnreadableHeader {
                    reason: error.reason(),
                },
                Ok(header) => match sample_plan(&header, entry.content()) {
                    // A tag nothing in this stage reads keeps the member's own
                    // tag and RFC 2361 name, so a diagnostic can say what it is.
                    Err(SampleFormatError::UnsupportedFormat { tag, name }) => {
                        SoundReadiness::UnsupportedFormat { tag, name }
                    }
                    // Every other refusal is a declaration this stage cannot
                    // honour: a `fmt ` extension too short for the tag it names,
                    // a block size no block fits, a `wSamplesPerBlock` the
                    // geometry contradicts. The code is that refusal's own.
                    Err(other) => SoundReadiness::Undecodable { code: other.code() },
                    Ok(format) => {
                        match decode_sound_sample(&mut member_context, entry.content(), &format) {
                            Ok(sample) => SoundReadiness::Decoded {
                                frames: sample.frames(),
                                samples_per_frame: sample.samples_per_frame(),
                            },
                            Err(error) => SoundReadiness::Undecodable { code: error.code() },
                        }
                    }
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

    /// The asset at position `index` of the container's declared index, or
    /// `None` when the index is out of range or that member failed its bounds
    /// check. A failed member has no asset, so positions in
    /// [`Self::entries`] shift past it; this looks the member up by its
    /// declared position instead.
    pub fn entry(&self, index: usize) -> Option<&SoundAsset<'a>> {
        self.entries.iter().find(|asset| asset.index() == index)
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

// --- Corpus audit (stage F06-D) ---------------------------------------------

/// What the audit established about one member of a member-listed container.
///
/// Three outcomes, never "playable": spec F06 non-negotiable #4 lets a
/// diagnostic listing continue past an invalid member and show every error, but
/// forbids it to advertise playability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemberVerdict {
    /// The member's content was interpreted under its own declaration.
    Decoded {
        /// What was decoded, e.g. the declared format and the frame count.
        detail: String,
    },
    /// The member is structurally sound (in bounds, and for a sound member a
    /// readable WAVE header) but its content is not interpreted by any
    /// reader this stage has. `reason` says why, quoting the member's own
    /// declaration where there is one.
    Readable {
        /// Why the content is not interpreted.
        reason: String,
    },
    /// The member is corrupt: out of bounds, an unreadable WAVE header, or a
    /// payload that contradicts its own declared format.
    Failed {
        /// Stable code of the failure.
        code: &'static str,
        /// Human explanation, carrying values and never member bytes.
        reason: String,
    },
}

impl MemberVerdict {
    /// Stable lowercase label for reports.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Decoded { .. } => "decoded",
            Self::Readable { .. } => "readable",
            Self::Failed { .. } => "failed",
        }
    }

    /// Whether the member is corrupt.
    pub const fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

/// One member row of the audit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberAudit {
    /// Position in the container's declared index.
    pub index: usize,
    /// The name bytes exactly as the index spells them (duplicates stay
    /// separate rows).
    pub name: Vec<u8>,
    /// Where the index says the member lives inside the container.
    pub span: ByteSpan,
    /// The index entry's recorded deviations from the pinned source's
    /// assertions (task #343's [`cs_formats::zbd::EntryAnomaly`] codes).
    pub anomalies: Vec<&'static str>,
    /// What the audit established.
    pub verdict: MemberVerdict,
}

/// What the audit established about one container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContainerVerdict {
    /// The container's own member index was read and every member has a row.
    Listed,
    /// The container was opened and routed, but its family's members are not
    /// listed by any F06 reader; `reason` names the feature that reads it.
    NotListed {
        /// Why no member rows exist.
        reason: &'static str,
    },
    /// The container could not be opened, routed, indexed or listed.
    Failed {
        /// The stable [`ZbdError::code`].
        code: &'static str,
        /// The error's explanation.
        reason: String,
    },
}

impl ContainerVerdict {
    /// Stable lowercase label for reports.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Listed => "listed",
            Self::NotListed { .. } => "not_listed",
            Self::Failed { .. } => "failed",
        }
    }
}

/// One container row of the audit, with the trace of how it was produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContainerAudit {
    /// The key the container was resolved with.
    pub key: AssetKey,
    /// The mount that served it, when it resolved.
    pub mount: Option<String>,
    /// The session generation that read it, when it was read.
    pub generation: Option<u64>,
    /// The installation-relative path dispatch was matched against, when the
    /// container opened.
    pub path: Option<String>,
    /// The container's length in bytes, when it was read.
    pub container_len: Option<u64>,
    /// The family the two keys named, when they named one.
    pub family: Option<ZbdFamily>,
    /// Which key identified the family ([`DispatchBasis::label`]).
    pub basis: Option<&'static str>,
    /// What the header bytes established: `validated` or `unvalidated`.
    pub header: Option<&'static str>,
    /// One row per declared member, for a listed container.
    pub members: Vec<MemberAudit>,
    /// Byte ranges of the data region no member claims.
    pub uncovered: Vec<ByteSpan>,
    /// The container's own outcome.
    pub verdict: ContainerVerdict,
}

impl ContainerAudit {
    /// Rows whose member is corrupt.
    pub fn failed_members(&self) -> impl Iterator<Item = &MemberAudit> + '_ {
        self.members.iter().filter(|row| row.verdict.is_failed())
    }

    /// How many members were decoded.
    pub fn decoded_members(&self) -> usize {
        self.count(|verdict| matches!(verdict, MemberVerdict::Decoded { .. }))
    }

    /// How many members are sound but not interpreted.
    pub fn readable_members(&self) -> usize {
        self.count(|verdict| matches!(verdict, MemberVerdict::Readable { .. }))
    }

    fn count(&self, wanted: impl Fn(&MemberVerdict) -> bool) -> usize {
        self.members
            .iter()
            .filter(|row| wanted(&row.verdict))
            .count()
    }

    /// Corruption in this container: the container itself failing counts
    /// once, and every failed member counts once.
    pub fn failures(&self) -> usize {
        usize::from(matches!(self.verdict, ContainerVerdict::Failed { .. }))
            + self.failed_members().count()
    }

    /// Content this container holds that no reader interpreted: a container
    /// that is not member-listed counts once, and every readable member,
    /// index anomaly and uncovered range counts once.
    pub fn uninterpreted(&self) -> usize {
        usize::from(matches!(self.verdict, ContainerVerdict::NotListed { .. }))
            + self.readable_members()
            + self
                .members
                .iter()
                .map(|row| row.anomalies.len())
                .sum::<usize>()
            + self.uncovered.len()
    }
}

/// The audit of every ZBD container of a session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ZbdAudit {
    /// One row per audited container, in the order they were audited.
    pub containers: Vec<ContainerAudit>,
}

impl ZbdAudit {
    /// Corrupt containers and members, summed over every container.
    pub fn failures(&self) -> usize {
        self.containers.iter().map(ContainerAudit::failures).sum()
    }

    /// Uninterpreted content, summed over every container.
    pub fn uninterpreted(&self) -> usize {
        self.containers
            .iter()
            .map(ContainerAudit::uninterpreted)
            .sum()
    }

    /// Whether the audit passes: nothing is corrupt, and under `strict`
    /// nothing is left uninterpreted either.
    pub fn passes(&self, strict: bool) -> bool {
        self.failures() == 0 && (!strict || self.uninterpreted() == 0)
    }
}

/// The feature that reads a family F06 does not member-list.
const fn not_listed_reason(family: ZbdFamily) -> &'static str {
    match family {
        ZbdFamily::Texture => {
            "texture packages are not member-listed by F06; their reader is F08 (texture archives)"
        }
        ZbdFamily::Interp => {
            "interp containers are not member-listed by F06; their reader is F07 (interp loading)"
        }
        ZbdFamily::GameZ => {
            "GameZ containers are not member-listed by F06; their reader is F10 (GameZ mesh \
             topology)"
        }
        ZbdFamily::Animation => {
            "animation containers are not member-listed by F06; the Crimson Skies animation body \
             is undocumented (task #340) and belongs to F20 (object animation)"
        }
        ZbdFamily::Sound | ZbdFamily::Reader => "sound and reader containers are member-listed",
    }
}

/// Audits one ZBD container of `session`: opens it through the VFS producer,
/// reads its own member index and gives every member a row.
///
/// Never fails: every failure is the row's [`ContainerVerdict::Failed`] or a
/// member's [`MemberVerdict::Failed`], so one corrupt container or member
/// cannot hide its siblings (spec F06 non-negotiable #4, AC04).
pub fn audit_container(session: &ContentSession, key: &AssetKey) -> ContainerAudit {
    let mut row = ContainerAudit {
        key: key.clone(),
        mount: None,
        generation: None,
        path: None,
        container_len: None,
        family: None,
        basis: None,
        header: None,
        members: Vec::new(),
        uncovered: Vec::new(),
        verdict: ContainerVerdict::Listed,
    };
    let container = match ZbdContainer::open(session, key) {
        Ok(container) => container,
        Err(error) => {
            if let Ok(asset) = session.resolve(key) {
                row.mount = Some(asset.resolved().mount.as_str().to_owned());
                row.generation = Some(asset.generation().get());
            }
            row.verdict = ContainerVerdict::Failed {
                code: error.code(),
                reason: error.to_string(),
            };
            return row;
        }
    };
    row.mount = Some(container.mount().as_str().to_owned());
    row.generation = Some(container.generation().get());
    row.path = Some(container.path().as_str().to_owned());
    row.container_len = Some(container.bytes().len() as u64);
    if let ZbdRouting::Routed {
        family,
        basis,
        header_status,
        ..
    } = *container.routing()
    {
        row.family = Some(family);
        row.basis = Some(basis.label());
        row.header = Some(match header_status {
            HeaderStatus::Validated { .. } => "validated",
            HeaderStatus::Unvalidated { .. } => "unvalidated",
        });
    }
    let family = container.family();
    if !cs_formats::zbd::indexed_by_trailer(family) {
        row.verdict = ContainerVerdict::NotListed {
            reason: not_listed_reason(family),
        };
        return row;
    }
    if let Err(error) = list_container(&container, &mut row) {
        row.members.clear();
        row.uncovered.clear();
        row.verdict = ContainerVerdict::Failed {
            code: error.code(),
            reason: error.to_string(),
        };
    }
    row
}

/// Reads `container`'s own index and fills `row` with one member row each.
fn list_container(container: &ZbdContainer, row: &mut ContainerAudit) -> Result<(), ZbdError> {
    let mut context = ParseContext::with_defaults(container.label());
    let index = container.index(&mut context)?;
    let table = index.member_table();
    let anomalies = |position: usize| -> Vec<&'static str> {
        index
            .entry(position)
            .map(|entry| entry.anomalies().map(|anomaly| anomaly.code()).collect())
            .unwrap_or_default()
    };
    let bounds_failure = |listing: &ArchiveListing<'_>, position: usize| {
        listing
            .row(position)
            .and_then(|member| member.error())
            .map(|error| MemberVerdict::Failed {
                code: error.code(),
                reason: error.to_string(),
            })
    };
    match container.family() {
        ZbdFamily::Sound => {
            let assets = container.sound_assets(&mut context, &index, &table)?;
            let listing = assets.listing();
            for (position, member) in listing.rows().iter().enumerate() {
                let verdict = match bounds_failure(listing, position) {
                    Some(failed) => failed,
                    None => {
                        let asset = assets
                            .entry(position)
                            .expect("a readable member is a sound asset");
                        sound_verdict(&mut context, asset)
                    }
                };
                row.members.push(MemberAudit {
                    index: position,
                    name: member.name().to_vec(),
                    span: member.span(),
                    anomalies: anomalies(position),
                    verdict,
                });
            }
            row.uncovered = listing.uncovered_ranges();
        }
        ZbdFamily::Reader => {
            let archive = container.reader_archive(&mut context, &index, &table)?;
            let listing = archive.listing();
            for (position, member) in listing.rows().iter().enumerate() {
                let verdict = bounds_failure(listing, position).unwrap_or_else(|| {
                    let entry = archive.entry(position).expect("a readable member");
                    MemberVerdict::Readable {
                        reason: entry.encoding().reason().to_owned(),
                    }
                });
                row.members.push(MemberAudit {
                    index: position,
                    name: member.name().to_vec(),
                    span: member.span(),
                    anomalies: anomalies(position),
                    verdict,
                });
            }
            row.uncovered = listing.uncovered_ranges();
        }
        other => {
            row.verdict = ContainerVerdict::NotListed {
                reason: not_listed_reason(other),
            };
        }
    }
    Ok(())
}

/// The verdict of one in-bounds sound member, from its [`SoundReadiness`].
fn sound_verdict(context: &mut ParseContext, asset: &SoundAsset<'_>) -> MemberVerdict {
    match asset.readiness() {
        SoundReadiness::Decoded {
            frames,
            samples_per_frame,
        } => {
            let detail = match asset.wave() {
                // A block-coded member's unit is the block, so the row says how
                // many blocks it holds and how many samples each of them holds,
                // never "N channel(s)" beside a block count.
                Ok(header) if is_block_coded(&header, asset.content()) => format!(
                    "{} tag {:#06x}, {} Hz, {} bits, {samples_per_frame} sample(s) per block, \
                     {frames} block(s)",
                    header.format_name().unwrap_or("unnamed"),
                    header.format_tag(),
                    header.rate_hz(),
                    header.bits_per_sample(),
                ),
                Ok(header) => format!(
                    "{} tag {:#06x}, {} Hz, {} bits, {samples_per_frame} channel(s), {frames} \
                     frames",
                    header.format_name().unwrap_or("unnamed"),
                    header.format_tag(),
                    header.rate_hz(),
                    header.bits_per_sample(),
                ),
                Err(_) => format!("{frames} frames of {samples_per_frame} samples"),
            };
            MemberVerdict::Decoded { detail }
        }
        SoundReadiness::UnsupportedFormat { tag, name } => MemberVerdict::Readable {
            reason: format!(
                "the member declares format tag {tag:#06x} ({}), which this stage cannot decode \
                 from the declaration it carries",
                name.unwrap_or("unnamed")
            ),
        },
        SoundReadiness::UnreadableHeader { reason } => match asset.wave() {
            Err(error) => MemberVerdict::Failed {
                code: error.code(),
                reason: error.to_string(),
            },
            // The readiness was taken from this same header read, so this arm
            // only keeps the verdict total.
            Ok(_) => MemberVerdict::Failed {
                code: "unreadable_header",
                reason: (*reason).to_owned(),
            },
        },
        SoundReadiness::Undecodable { code } => MemberVerdict::Failed {
            code,
            reason: asset
                .decode(context)
                .err()
                .map_or_else(|| (*code).to_owned(), |error| error.to_string()),
        },
    }
}

/// Audits every container in `keys`, in order.
pub fn audit_containers<'k>(
    session: &ContentSession,
    keys: impl IntoIterator<Item = &'k AssetKey>,
) -> ZbdAudit {
    ZbdAudit {
        containers: keys
            .into_iter()
            .map(|key| audit_container(session, key))
            .collect(),
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

    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{MemberVerdict, SoundReadiness, ZbdContainer, ZbdError, audit_container};
    use cs_formats::ParseContext;
    use cs_formats::zbd::{
        ContainerStatus, INDEX_ENTRY_BYTES, INDEX_NAME_BYTES, INDEX_UNEXPLAINED_BYTES,
        TRAILER_VERSION_ONE, WAVE_FORMAT_IMA_ADPCM, WAVE_FORMAT_MS_ADPCM, WAVE_FORMAT_PCM,
        ZbdFamily,
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
    ///
    /// `extension` is the tag's own `fmt ` tail, kept beside the sixteen common
    /// fields exactly as the specification orders it: `cbSize` and, for the two
    /// block codecs, the values that locate and read a block.
    struct Fmt {
        tag: u16,
        channels: u16,
        rate_hz: u32,
        bits_per_sample: u16,
        block_align: u16,
        extension: Vec<u8>,
    }

    impl Fmt {
        /// 16-bit mono PCM, the shape these tests decode in full.
        const fn pcm16() -> Self {
            Self {
                tag: WAVE_FORMAT_PCM,
                channels: 1,
                rate_hz: 22_050,
                bits_per_sample: 16,
                block_align: 2,
                extension: Vec::new(),
            }
        }

        /// A block-coded member: four bits per sample, the block size the retail
        /// archives use, and the `fmt ` extension that declares its geometry.
        fn adpcm(tag: u16, rate_hz: u32, block_align: u16, extension: Vec<u8>) -> Self {
            Self {
                tag,
                channels: 1,
                rate_hz,
                bits_per_sample: 4,
                block_align,
                extension,
            }
        }

        /// The IMA ADPCM shape task #344 measured most often in retail: mono at
        /// 11_025 Hz with `nBlockAlign` 256. A block of that size holds its four
        /// header bytes and two nibbles per remaining byte, and starts from the
        /// predictor it declares, so the `wSamplesPerBlock` such a member
        /// declares is `1 + 2 * (256 - 4)` = 505.
        fn ima() -> Self {
            let block_align = 256;
            Self::adpcm(
                WAVE_FORMAT_IMA_ADPCM,
                11_025,
                block_align,
                ima_extension(1 + 2 * (block_align - 4)),
            )
        }

        /// The Microsoft ADPCM shape task #344 measured: mono at 22_050 Hz with
        /// `nBlockAlign` 256. A block holds seven header bytes per channel and
        /// two nibbles per remaining byte, and starts from the two history
        /// samples it declares, so the `wSamplesPerBlock` it declares is
        /// `2 + 2 * (256 - 7)` = 500.
        fn ms() -> Self {
            let block_align = 256;
            Self::adpcm(
                WAVE_FORMAT_MS_ADPCM,
                22_050,
                block_align,
                ms_extension(2 + 2 * (block_align - 7)),
            )
        }

        /// An IMA ADPCM member that declares no `fmt ` extension at all, so no
        /// `wSamplesPerBlock` can be read out of it.
        fn ima_without_extension() -> Self {
            Self::adpcm(WAVE_FORMAT_IMA_ADPCM, 11_025, 256, Vec::new())
        }

        /// A Microsoft ADPCM member whose `fmt ` payload stops after `cbSize`,
        /// four bytes short of the `wSamplesPerBlock` the tag documents.
        fn ms_with_short_extension() -> Self {
            Self::adpcm(
                WAVE_FORMAT_MS_ADPCM,
                22_050,
                256,
                32u16.to_le_bytes().to_vec(),
            )
        }

        /// An IMA ADPCM member whose declared `wSamplesPerBlock` is not what a
        /// full block of its own `nBlockAlign` holds, so the header contradicts
        /// itself.
        fn ima_with_wrong_samples_per_block() -> Self {
            Self::adpcm(WAVE_FORMAT_IMA_ADPCM, 11_025, 256, ima_extension(7))
        }
    }

    /// The IMA ADPCM `fmt ` extension: `cbSize` 2 and `wSamplesPerBlock`.
    fn ima_extension(samples_per_block: u16) -> Vec<u8> {
        let mut bytes = 2u16.to_le_bytes().to_vec();
        bytes.extend_from_slice(&samples_per_block.to_le_bytes());
        bytes
    }

    /// The Microsoft ADPCM `fmt ` extension: `cbSize` 32, `wSamplesPerBlock`,
    /// `wNumCoefs` and that many coefficient pairs.
    ///
    /// The table is the one every retail member declares
    /// (`docs/findings/2026-09-28-t344-zbd-sound-member-wave-headers.md`), so
    /// the fixture members carry the values the original data carries rather
    /// than a table invented here.
    fn ms_extension(samples_per_block: u16) -> Vec<u8> {
        let mut bytes = 32u16.to_le_bytes().to_vec();
        bytes.extend_from_slice(&samples_per_block.to_le_bytes());
        bytes.extend_from_slice(&(RETAIL_MS_COEFFICIENTS.len() as u16).to_le_bytes());
        for (predictor, difference) in RETAIL_MS_COEFFICIENTS {
            bytes.extend_from_slice(&predictor.to_le_bytes());
            bytes.extend_from_slice(&difference.to_le_bytes());
        }
        bytes
    }

    /// The Microsoft ADPCM coefficient table every retail member declares, in
    /// the order `aCoefs` lists it.
    const RETAIL_MS_COEFFICIENTS: [(i16, i16); 7] = [
        (256, 0),
        (512, -256),
        (0, 0),
        (192, 64),
        (240, 0),
        (460, -208),
        (392, -232),
    ];

    /// Assembles a complete RIFF/WAVE member around `fmt` and a `data` payload.
    fn wave_member(fmt: &Fmt, data: &[u8]) -> Vec<u8> {
        let mut fmt_payload = Vec::new();
        fmt_payload.extend_from_slice(&fmt.tag.to_le_bytes());
        fmt_payload.extend_from_slice(&fmt.channels.to_le_bytes());
        fmt_payload.extend_from_slice(&fmt.rate_hz.to_le_bytes());
        fmt_payload.extend_from_slice(&(fmt.rate_hz * u32::from(fmt.block_align)).to_le_bytes());
        fmt_payload.extend_from_slice(&fmt.block_align.to_le_bytes());
        fmt_payload.extend_from_slice(&fmt.bits_per_sample.to_le_bytes());
        fmt_payload.extend_from_slice(&fmt.extension);

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

    /// One IMA ADPCM block: the documented four header bytes — an `i16`
    /// predictor, a `u8` step index and one reserved byte — and then the
    /// nibbles, low one first.
    fn ima_block(predictor: i16, step_index: u8, nibble_bytes: &[u8]) -> Vec<u8> {
        let mut bytes = predictor.to_le_bytes().to_vec();
        bytes.push(step_index);
        bytes.push(0);
        bytes.extend_from_slice(nibble_bytes);
        bytes
    }

    /// One Microsoft ADPCM block: the seven header bytes of a mono channel —
    /// a `u8` coefficient index, an `i16` delta, an `i16` sample1 and an `i16`
    /// sample2 — and then the nibbles, already packed two to a byte with the
    /// high one first, as the layout stores them.
    fn ms_block(
        coefficient: u8,
        delta: i16,
        sample1: i16,
        sample2: i16,
        nibble_bytes: &[u8],
    ) -> Vec<u8> {
        let mut bytes = vec![coefficient];
        bytes.extend_from_slice(&delta.to_le_bytes());
        bytes.extend_from_slice(&sample1.to_le_bytes());
        bytes.extend_from_slice(&sample2.to_le_bytes());
        bytes.extend_from_slice(nibble_bytes);
        bytes
    }

    /// `count` bytes of IMA nibbles walking the small magnitudes 1 to 8, low one
    /// first, so a decoded block moves instead of sitting at its predictor.
    fn ima_nibbles(count: usize) -> Vec<u8> {
        (0..count)
            .map(|index| ((index % 8 + 1) as u8) | (((index % 8 + 2) as u8) << 4))
            .collect()
    }

    /// `count` bytes of Microsoft ADPCM nibbles walking the small magnitudes 1
    /// to 8, high one first, as that layout stores them.
    fn ms_nibbles(count: usize) -> Vec<u8> {
        (0..count)
            .map(|index| (((index % 8 + 1) as u8) << 4) | ((index % 8 + 2) as u8))
            .collect()
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

    /// A mono IMA ADPCM member of two full 256-byte blocks.
    ///
    /// The blocks declare different predictors, so a decode that carried state
    /// from one block into the next — or that read the block header from the
    /// wrong offset — cannot produce both.
    fn ima_member() -> Vec<u8> {
        let block_align = 256;
        let mut data = ima_block(1000, 0, &ima_nibbles(block_align - 4));
        data.extend_from_slice(&ima_block(-2000, 3, &ima_nibbles(block_align - 4)));
        wave_member(&Fmt::ima(), &data)
    }

    /// A mono Microsoft ADPCM member of two full 256-byte blocks, with
    /// different history samples in each.
    fn ms_member() -> Vec<u8> {
        let block_align = 256;
        let mut data = ms_block(0, 100, 300, 200, &ms_nibbles(block_align - 7));
        data.extend_from_slice(&ms_block(0, 50, 100, 60, &ms_nibbles(block_align - 7)));
        wave_member(&Fmt::ms(), &data)
    }

    /// A block-coded member whose payload is one full block of bytes, for the
    /// refusal cases: the member is a real RIFF/WAVE file, only its declaration
    /// is unusable.
    fn declared_member(fmt: &Fmt) -> Vec<u8> {
        wave_member(fmt, &ima_nibbles(256))
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
        // An observed sound archive name (task #340): three members, an
        // uncompressed PCM one and two block-coded ones, each declaring the
        // block geometry its own payload has.
        tree.write(
            "ZBD/soundsl.zbd",
            &archive(&[
                (b"ramp.wav".as_slice(), pcm16_member()),
                (b"gun.wav".as_slice(), ima_member()),
                (b"loop.wav".as_slice(), ms_member()),
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

        // The two block-coded members decode under the block geometry their own
        // `fmt ` payload declares: a frame is a whole block, so the row carries
        // the block count and the samples one block holds.
        let ima = assets.entry(1).expect("row 1 is an asset");
        assert_eq!(ima.name(), b"gun.wav");
        assert_eq!(
            ima.readiness(),
            &SoundReadiness::Decoded {
                frames: 2,
                samples_per_frame: 505
            }
        );
        let ms = assets.entry(2).expect("row 2 is an asset");
        assert_eq!(ms.name(), b"loop.wav");
        assert_eq!(
            ms.readiness(),
            &SoundReadiness::Decoded {
                frames: 2,
                samples_per_frame: 500
            }
        );
        assert_eq!(
            assets.decoded().count(),
            3,
            "every fixture member is decoded under its own declaration"
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
        assert!(assets.entry(0).is_none(), "the lying member has no bytes");
        assert_eq!(assets.entry(1).expect("row 1").name(), b"honest.wav");
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
        assert_eq!(assets.decoded().count(), 3, "every member is decoded");

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
        // The block-coded members decode after teardown too: their blocks are
        // in the container's own bytes, not behind a live file handle.
        let decoded = assets
            .entry(1)
            .expect("the block-coded asset survives teardown")
            .decode(&mut context)
            .expect("and still decodes its blocks");
        assert_eq!(decoded.frames(), 2);
        assert_eq!(decoded.sample_count(), 1010);

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

    // --- task #524: the block-aware decode plan reaches the runtime ----------

    #[test]
    fn accept_t524_a_block_coded_member_decodes_the_blocks_its_own_header_declares() {
        // The switch this task makes: a compressed member is planned from the
        // `fmt ` extension **its own bytes** carry and decoded block by block,
        // so the row's counts are its declaration's, not PCM's.
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

        // The IMA member: two 256-byte blocks, so two frames, and the 505
        // samples one such block holds.
        let ima = assets.entry(1).expect("row 1 is an asset");
        assert_eq!(ima.name(), b"gun.wav");
        assert_eq!(
            ima.readiness(),
            &SoundReadiness::Decoded {
                frames: 2,
                samples_per_frame: 505
            }
        );
        let ima_header = ima.wave().expect("its header reads");
        assert_eq!(ima_header.format_tag(), WAVE_FORMAT_IMA_ADPCM);
        assert_eq!(ima_header.block_align(), 256);
        let mut context = ParseContext::with_defaults(container.label());
        let decoded = ima.decode(&mut context).expect("its blocks decode");
        assert_eq!(decoded.frames(), 2, "a frame is a whole block");
        assert_eq!(decoded.samples_per_frame(), 505);
        assert_eq!(decoded.sample_count(), 2 * 505);
        assert_eq!(
            decoded.byte_len(),
            512,
            "the decode accounts for the whole payload"
        );
        assert_eq!(decoded.byte_len(), ima_header.data_span().length);
        // Every block starts from the predictor **it** declares, so the second
        // block's value is not a continuation of the first one's.
        assert_eq!(
            decoded.samples()[0],
            1000,
            "the first block's own predictor is its first sample"
        );
        assert_eq!(
            decoded.samples()[505],
            -2000,
            "the second block restates its own predictor"
        );
        assert_eq!(decoded.frame(1), Some(&decoded.samples()[505..1010]));

        // The Microsoft member: two 256-byte blocks of 500 samples each, and
        // the two history values each block starts from.
        let ms = assets.entry(2).expect("row 2 is an asset");
        assert_eq!(ms.name(), b"loop.wav");
        assert_eq!(
            ms.readiness(),
            &SoundReadiness::Decoded {
                frames: 2,
                samples_per_frame: 500
            }
        );
        let mut context = ParseContext::with_defaults(container.label());
        let decoded = ms.decode(&mut context).expect("its blocks decode");
        assert_eq!(decoded.frames(), 2);
        assert_eq!(decoded.samples_per_frame(), 500);
        assert_eq!(decoded.sample_count(), 1000);
        assert_eq!(decoded.byte_len(), 512);
        assert_eq!(decoded.samples()[0], 200, "the older history sample");
        assert_eq!(decoded.samples()[1], 300, "then the newer one");
        assert_eq!(decoded.samples()[500], 60);
        assert_eq!(decoded.samples()[501], 100);

        // The PCM sibling still reports what it reported before: a frame is one
        // sample per channel, and the values are its stored samples.
        let ramp = assets.entry(0).expect("row 0 is an asset");
        assert_eq!(
            ramp.readiness(),
            &SoundReadiness::Decoded {
                frames: 8,
                samples_per_frame: 1
            }
        );
        let mut context = ParseContext::with_defaults(container.label());
        let decoded = ramp.decode(&mut context).expect("its declared PCM decodes");
        for (index, expected) in RAMP.iter().enumerate() {
            assert_eq!(
                decoded.samples()[index],
                i32::from(*expected),
                "sample {index}"
            );
        }
    }

    #[test]
    fn accept_t524_an_audit_row_counts_a_block_coded_member_in_blocks_not_frames() {
        // The corpus audit's detail string is the one place a human reads a
        // decoded row, so it has to count a block-coded member in the unit its
        // own declaration uses. Calling a 256-byte block a "frame" and putting
        // a channel count beside it would describe a frame this format does
        // not have: `wSamplesPerBlock` is samples **per block**, and the block
        // count is what the payload holds.
        let tree = installation();
        let session = session(tree.0.as_path());
        let audit = audit_container(&session, &key("install", "ZBD/soundsl.zbd"));
        assert_eq!(audit.family, Some(ZbdFamily::Sound));
        assert_eq!(audit.decoded_members(), 3);
        assert_eq!(audit.uninterpreted(), 0, "nothing is left unread");
        let details: Vec<&str> = audit
            .members
            .iter()
            .map(|row| match &row.verdict {
                MemberVerdict::Decoded { detail } => detail.as_str(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            details,
            [
                // The uncompressed member keeps the wording it always had.
                "pcm tag 0x0001, 22050 Hz, 16 bits, 1 channel(s), 8 frames",
                // The IMA member: two blocks of 505 samples each, at the rate
                // and tag its own `fmt ` declares.
                "ima_adpcm tag 0x0011, 11025 Hz, 4 bits, 505 sample(s) per block, 2 block(s)",
                // And the Microsoft one, whose table and block size differ.
                "ms_adpcm tag 0x0002, 22050 Hz, 4 bits, 500 sample(s) per block, 2 block(s)",
            ]
        );
        // No block-coded row says "frames" or "channel(s)", which is what the
        // wording above is for.
        for detail in &details[1..] {
            assert!(!detail.contains("frames"), "{detail}");
            assert!(!detail.contains("channel(s)"), "{detail}");
        }
    }

    #[test]
    fn accept_t524_a_block_coded_member_without_a_readable_fmt_extension_is_refused() {
        // Spec F06 non-negotiable #4: a member that declares a block codec but
        // cannot say how its blocks are laid out is refused with **that**
        // refusal's own code, and its siblings stay decoded.
        let tree = Temp::new("install");
        tree.write(
            "ZBD/soundsl.zbd",
            &archive(&[
                (b"ramp.wav".as_slice(), pcm16_member()),
                (
                    b"noext.wav".as_slice(),
                    declared_member(&Fmt::ima_without_extension()),
                ),
                (
                    b"short.wav".as_slice(),
                    declared_member(&Fmt::ms_with_short_extension()),
                ),
                (
                    b"lying.wav".as_slice(),
                    declared_member(&Fmt::ima_with_wrong_samples_per_block()),
                ),
                (b"gun.wav".as_slice(), ima_member()),
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
            .expect("three unusable declarations do not fail the listing");

        assert_eq!(assets.len(), 5);
        assert_eq!(
            assets.wave_failures(),
            0,
            "every member is a readable WAVE file"
        );

        // An IMA member with no `fmt ` extension has no `wSamplesPerBlock` to
        // read, so it keeps its own tag rather than a row that says nothing.
        let no_extension = assets.entry(1).expect("row 1 is an asset");
        assert_eq!(no_extension.name(), b"noext.wav");
        assert_eq!(
            no_extension.readiness(),
            &SoundReadiness::UnsupportedFormat {
                tag: WAVE_FORMAT_IMA_ADPCM,
                name: Some("ima_adpcm")
            }
        );

        // The other two are declarations this stage read and cannot honour.
        for (position, name, code) in [
            (2usize, &b"short.wav"[..], "adpcm_extension_short"),
            (3, &b"lying.wav"[..], "samples_per_block_mismatch"),
        ] {
            let asset = assets.entry(position).expect("the member has bytes");
            assert_eq!(asset.name(), name);
            assert_eq!(asset.readiness(), &SoundReadiness::Undecodable { code });
            // And decoding it directly reports the same typed refusal.
            let mut context = ParseContext::with_defaults(container.label());
            let error = asset
                .decode(&mut context)
                .expect_err("the declaration cannot be honoured");
            assert_eq!(error.code(), code);
        }

        // Every sibling is untouched: the PCM member and the well-declared
        // block-coded one both decode.
        assert_eq!(assets.decoded().count(), 2);
        assert_eq!(
            assets.entry(0).expect("row 0").readiness(),
            &SoundReadiness::Decoded {
                frames: 8,
                samples_per_frame: 1
            }
        );
        assert_eq!(
            assets.entry(4).expect("row 4").readiness(),
            &SoundReadiness::Decoded {
                frames: 2,
                samples_per_frame: 505
            }
        );
    }

    /// 16-bit mono PCM with `count` samples, a ramp that stays inside `i16`.
    fn pcm16_samples(count: usize) -> Vec<u8> {
        let mut data = Vec::with_capacity(count * 2);
        for index in 0..count {
            let sample = i16::try_from(index % 512).expect("a ramp inside i16");
            data.extend_from_slice(&sample.to_le_bytes());
        }
        wave_member(&Fmt::pcm16(), &data)
    }

    #[test]
    fn accept_t524_a_listing_bounds_each_member_by_the_callers_own_budget() {
        // A listing keeps counts, not samples: one member's decode must not
        // spend the next member's budget. Two members that each fit the
        // caller's ceiling decode even when their sum does not, which is what
        // makes a real archive readable at all — the 5,041 members of the two
        // retail sound archives decode to 1,418 MiB against a 64 MiB per-parse
        // ceiling, while the largest single member needs 22 MiB.
        const SAMPLES: usize = 12_000;
        // 12,000 samples of `i32` is 48,000 bytes: each member fits the 64 KiB
        // ceiling, and the pair does not.
        const MEMBER_BYTES: u64 = 48_000;
        const CEILING: u64 = 64 * 1024;
        let member = pcm16_samples(SAMPLES);
        let tree = Temp::new("install");
        tree.write(
            "ZBD/soundsl.zbd",
            &archive(&[
                (b"first.wav".as_slice(), member.clone()),
                (b"second.wav".as_slice(), member),
            ]),
        );
        let session = session(tree.0.as_path());
        let container =
            ZbdContainer::open(&session, &key("install", "ZBD/soundsl.zbd")).expect("it opens");

        let mut context = ParseContext::new(container.label(), CEILING, 32);
        let index = container.index(&mut context).expect("its trailer reads");
        let table = index.member_table();
        let assets = container
            .sound_assets(&mut context, &index, &table)
            .expect("both members fit the caller's ceiling");
        assert_eq!(assets.len(), 2);
        for position in 0..2 {
            assert_eq!(
                assets.entry(position).expect("row").readiness(),
                &SoundReadiness::Decoded {
                    frames: SAMPLES as u64,
                    samples_per_frame: 1
                }
            );
        }

        // The limits are inherited, never widened: a ceiling no member fits
        // refuses every member, each with the budget's own code, and its
        // siblings are unaffected.
        let mut starved = ParseContext::new(container.label(), 1024, 32);
        let starved_index = container
            .index(&mut starved)
            .expect("a ceiling that fits the index still reads it");
        let starved_table = starved_index.member_table();
        let refused = container
            .sound_assets(&mut starved, &starved_index, &starved_table)
            .expect("a refused decode is a row, not a failed listing");
        assert_eq!(refused.len(), 2);
        for position in 0..2 {
            let asset = refused.entry(position).expect("row");
            assert_eq!(
                asset.readiness(),
                &SoundReadiness::Undecodable {
                    code: "allocation_budget_exceeded"
                }
            );
            let mut context = ParseContext::new(container.label(), 1024, 32);
            assert_eq!(
                asset.decode(&mut context).expect_err("refused").code(),
                "allocation_budget_exceeded"
            );
        }
        assert!(
            starved.allocation().used() < MEMBER_BYTES,
            "a refused member's buffer is never charged to the caller's ledger: {} bytes booked",
            starved.allocation().used()
        );
    }

    /// The read-only original installation, or a loud failure naming what a
    /// retail test needs.
    fn game_dir() -> PathBuf {
        let value = std::env::var_os("CS_GAME_DIR").expect(
            "CS_GAME_DIR must point at the read-only original installation (capability `retail`)",
        );
        assert!(!value.is_empty(), "CS_GAME_DIR must not be empty");
        PathBuf::from(value)
    }

    /// A member's declared `wFormatTag`, `nChannels`, `nBlockAlign` and
    /// `wSamplesPerBlock`, read from the bytes at the offsets the RIFF
    /// specification fixes (task #344 measured every retail member's `fmt `
    /// payload there). Independent of the production readers.
    fn declared_fields(member: &[u8]) -> (u16, u16, u16, Option<u16>) {
        let u16_at = |at: usize| -> u16 { u16::from_le_bytes([member[at], member[at + 1]]) };
        let u32_at = |at: usize| -> u32 {
            u32::from_le_bytes([member[at], member[at + 1], member[at + 2], member[at + 3]])
        };
        let fmt_length = u32_at(16) as usize;
        let fmt = 20..20 + fmt_length;
        let tag = u16_at(fmt.start);
        let channels = u16_at(fmt.start + 2);
        let block_align = u16_at(fmt.start + 12);
        let samples_per_block = match tag {
            WAVE_FORMAT_PCM => None,
            WAVE_FORMAT_IMA_ADPCM | WAVE_FORMAT_MS_ADPCM => Some(u16_at(fmt.start + 18)),
            _ => None,
        };
        (tag, channels, block_align, samples_per_block)
    }

    /// A member's `data` chunk length, found by walking the chunks from the
    /// `WAVE` form type. Independent of the production reader.
    fn data_chunk_len(member: &[u8]) -> u64 {
        let u32_at = |at: usize| -> u64 {
            u64::from(u32::from_le_bytes([
                member[at],
                member[at + 1],
                member[at + 2],
                member[at + 3],
            ]))
        };
        let mut at = 12usize;
        while at + 8 <= member.len() {
            let id = &member[at..at + 4];
            let length = u32_at(at + 4) as usize;
            if id == b"data" {
                return length as u64;
            }
            // Chunks are padded to an even length.
            at += 8 + length + (length % 2);
        }
        panic!("the member has no `data` chunk")
    }

    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_t524_retail_every_compressed_sound_member_decodes_to_its_declared_counts() {
        // The criterion this task exists for: through the production path —
        // `ZbdContainer::open`, `sound_assets` — every member of both retail
        // sound archives is a decoded row, and its counts are the ones its own
        // `fmt ` payload implies. The counts are recomputed here from the
        // member's raw bytes at the offsets the RIFF specification fixes, so
        // this is an independent read and not a restatement of the verdict.
        let root = game_dir();
        let found = crate::install::discover(&root).expect("the installation is discoverable");
        let resolve_context =
            cs_types::asset_id::ResolveContext::new(crate::install::fingerprint(&found.manifest));
        let mut builder = crate::vfs::SessionBuilder::new(resolve_context);
        builder
            .mount_installation(&root, &found.diagnosis)
            .expect("the installation mounts");
        let session = builder.open();

        let mut per_tag: BTreeMap<u16, usize> = BTreeMap::new();
        let mut decoded_per_tag: BTreeSet<u16> = BTreeSet::new();
        let mut members = 0usize;
        for spelling in ["ZBD/soundsl.zbd", "ZBD/soundsh.zbd"] {
            let container =
                ZbdContainer::open(&session, &key("install", spelling)).expect("it opens");
            assert_eq!(container.family(), cs_formats::zbd::ZbdFamily::Sound);
            let mut context = ParseContext::with_defaults(container.label());
            let index = container.index(&mut context).expect("its trailer reads");
            let table = index.member_table();
            let assets = container
                .sound_assets(&mut context, &index, &table)
                .expect("the sound archive is read");
            assert_eq!(assets.status(), ContainerStatus::Clean, "{spelling}");
            assert_eq!(assets.failures(), 0, "{spelling}");

            for position in 0..assets.len() {
                let asset = assets.entry(position).expect("every member has bytes");
                let name = String::from_utf8_lossy(asset.name()).into_owned();
                let (tag, channels, block_align, samples_per_block) =
                    declared_fields(asset.content());
                let data_len = data_chunk_len(asset.content());
                let block_align = u64::from(block_align);
                let (frames, samples_per_frame) = match samples_per_block {
                    // A block-coded member's frame is a whole block, and its
                    // final block may be shorter than `nBlockAlign`.
                    Some(samples_per_block) => (
                        data_len.div_ceil(block_align),
                        u64::from(samples_per_block) * u64::from(channels),
                    ),
                    // An uncompressed one must be a whole number of frames.
                    None => (data_len / block_align, u64::from(channels)),
                };
                let SoundReadiness::Decoded {
                    frames: decoded_frames,
                    samples_per_frame: decoded_samples_per_frame,
                } = asset.readiness()
                else {
                    panic!(
                        "{spelling} member {position} ({name}) is {:?}, not decoded",
                        asset.readiness()
                    )
                };
                assert_eq!(
                    (*decoded_frames, *decoded_samples_per_frame),
                    (frames, samples_per_frame),
                    "{spelling} member {position} ({name})"
                );
                *per_tag.entry(tag).or_default() += 1;
                members += 1;

                // One member of each declared format is also decoded through
                // the public entry point, which plans it from the member's own
                // bytes rather than from the row the listing produced.
                if decoded_per_tag.insert(tag) {
                    let mut context = ParseContext::with_defaults(container.label());
                    let decoded = asset.decode(&mut context).expect("it decodes");
                    assert_eq!(decoded.frames(), frames, "{spelling} {name}");
                    assert_eq!(decoded.samples_per_frame(), samples_per_frame);
                    assert_eq!(decoded.byte_len(), data_len, "{spelling} {name}");
                    assert!(decoded.sample_count() > 0, "{spelling} {name}");
                }
            }
        }
        assert_eq!(
            decoded_per_tag,
            BTreeSet::from([WAVE_FORMAT_PCM, WAVE_FORMAT_IMA_ADPCM, WAVE_FORMAT_MS_ADPCM]),
            "every declared format was decoded through SoundAsset::decode"
        );

        // The census tasks #344 and #444 measured on this installation:
        // 5,019 compressed members (555 IMA, 4,464 Microsoft) and 22 PCM ones.
        assert_eq!(
            per_tag,
            BTreeMap::from([
                (WAVE_FORMAT_PCM, 22),
                (WAVE_FORMAT_IMA_ADPCM, 555),
                (WAVE_FORMAT_MS_ADPCM, 4_464),
            ]),
            "every retail sound member declares one of the three measured formats"
        );
        assert_eq!(members, 5_041);
    }
}
