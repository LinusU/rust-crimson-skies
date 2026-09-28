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
    /// [`ZbdError::Sound`] when the family gate or the listing refuses the
    /// container, and [`ZbdError::Dispatch`] when the caller passes an index
    /// that read a different container's dispatch.
    pub fn sound_archive<'c>(
        &'c self,
        context: &mut ParseContext,
        index: &'c VersionOneIndex<'c>,
        table: &'c MemberTable<'c>,
    ) -> Result<SoundArchive<'c>, ZbdError> {
        Ok(read_sound_archive(context, table, index.data())?)
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
    /// # Errors
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
