//! Mounting a ROF container into the VFS and reading its members back
//! (F05-C).
//!
//! This is the integration stage of spec F05: the producer is the ROF
//! reader of [`cs_formats`] (`read_tree` walks the directory tree with
//! cycle detection, bounded depth, extent checks, explainable flags and
//! the overlap check; `read_member` reads one member through the bounded
//! zlib decoder), and the consumers are the content session of
//! [`crate::vfs`] — keys, spans and traces through a normal [`Mount`] —
//! and the explicit inspection/export path below.
//!
//! ```text
//! let mounted = mount_rof(builder, container_path)?;   // or mount_rof_into
//! session.mount(mounted.mount)?;                        // the VFS holds it
//! let asset  = session.resolve(&key)?;                  // span + trace
//! let bytes  = mounted.source.read(asset.resolved().key)?;  // decoded
//! export_rof_member(&session, &asset, &mounted.source, &dir)?;
//! ```
//!
//! **What the mount records.** One member per file entry, spelled as its
//! root-relative path joined with `/`. The [`SourceSpan`] a resolution
//! returns is the member's *stored* extent in the container — offset
//! `start`, length `raw_length`, digest of exactly those bytes — because a
//! span is provenance inside a file (IDENTITY-CONTENT: "treat offsets as
//! unsigned checked ranges") and `raw_length` is the field the reference
//! extractor reads ([S05]). Which of the two on-disk length words is
//! stored and which decoded is the F05-D retail question; nothing here
//! decides it from a field name.
//!
//! **Why reads do not go through `Vfs::read_all`.** The F04 read path
//! ([`crate::vfs::source`]) reads a *host file* range: it refuses any
//! member whose length differs from the file on disk
//! ([`crate::vfs::ReadError::ChangedOnDisk`]) and has no notion of a
//! compressed member, so it can only serve plain directory mounts. A ROF
//! member lives *inside* a container and may be zlib-compressed, so its
//! decoded bytes come from [`RofSource::read`], which holds the container
//! bytes the mount was built from and calls the production
//! `cs_formats::read_member`. The mount answers resolution (keys, spans,
//! traces, collisions); the source answers bytes. A ROF mount therefore
//! has no directory backing: `Vfs::read_all` on one of its members reports
//! `no backing` rather than returning compressed bytes as if they were
//! content.
//!
//! **Nothing here writes** except [`export_rof_member`], the explicit
//! private research export of spec F04 non-negotiable behavior 5, which
//! validates the name, reads the member (so an expansion bomb or a refused
//! layout fails *before* any byte is written) and only then hands the bytes
//! to [`ExportDirectory::write`]. Mounting a container that the reader
//! refuses — a cycle, a name table its records disagree with, an extent
//! past the end of the file — fails before a [`Mount`] exists, so the
//! session it was meant to join is untouched and can be retried or dropped
//! (F04-C's builder contract).
//!
//! Original data is read-only: this module only ever reads the container.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cs_formats::{
    ParseContext, RofError, RofLimits, RofMember, RofMemberRead, RofRawRecord, read_member,
    read_tree,
};
use cs_types::asset_id::{AssetKey, AssetVariant, MountId, MountNamespace};
use cs_types::evidence::ContentHash;
use cs_types::install::{RelativePath, RelativePathError};

use crate::install::sha256;
use crate::vfs::export::{ExportDirectory, ExportError, ExportedFile, export_components};
use crate::vfs::mount::{Mount, MountBuilder, MountError};
use crate::vfs::session::{ContentSession, SessionAsset, SessionBuilder, SessionError};

/// Why a ROF container could not be mounted.
///
/// Every variant is raised *before* a [`Mount`] exists, so a refused
/// container never leaves a partial mount behind: the session builder it
/// was handed still holds exactly the mounts it held before, and the same
/// container can be retried or simply dropped.
#[derive(Debug)]
pub enum RofMountError {
    /// The container file could not be read at all.
    UnreadableContainer {
        /// The mount's container label (the provenance the caller gave).
        container: String,
        /// The host path that was read.
        path: PathBuf,
        /// Why the read failed.
        source: io::Error,
    },
    /// [`read_tree`] refused the container: a cycle, a name table its
    /// records disagree with, an extent outside the file, unexplained
    /// flags, an overlap, or a structural failure. Propagated unchanged so
    /// its code, container and absolute offset survive to the diagnostic.
    Format {
        /// The reader's refusal.
        source: RofError,
    },
    /// A member name is not valid UTF-8, so it has no key spelling a
    /// lookup could match (the same refusal as a non-UTF-8 host name in
    /// [`crate::vfs::mount_directory`]).
    NonUtf8Name {
        /// The mount's container label.
        container: String,
        /// Offset of the member's stored extent inside the container.
        offset: u64,
    },
    /// A member spelling cannot become a key: `..`, an absolute spelling,
    /// a drive prefix, an empty or `.` component, or a NUL byte. Carries
    /// the reason but never the raw name out of the container.
    InvalidMemberPath {
        /// The mount's container label.
        container: String,
        /// Offset of the member's stored extent inside the container.
        offset: u64,
        /// Which path rule refused it.
        reason: RelativePathError,
    },
    /// The member was refused by the mount index — typically a second
    /// spelling of a key the mount already holds (spec F04
    /// non-negotiable behavior 3: both spellings are reported, never
    /// flattened to the first one found).
    Member {
        /// The mount's container label.
        container: String,
        /// The refusal.
        source: MountError,
    },
    /// The mount as a whole was refused (an empty container label, a
    /// precedence/scope mismatch).
    Mount(MountError),
    /// The session refused the mount that was already built — a repeated
    /// mount id. Raised by [`mount_rof_into`] after the container itself
    /// was accepted.
    Session(SessionError),
}

impl RofMountError {
    /// Stable lowercase identifier for reports and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnreadableContainer { .. } => "unreadable_container",
            Self::Format { source } => source.code(),
            Self::NonUtf8Name { .. } => "non_utf8_name",
            Self::InvalidMemberPath { .. } => "invalid_member_path",
            Self::Member { .. } => "member",
            Self::Mount(_) => "mount",
            Self::Session(_) => "session",
        }
    }

    /// The mount's container label, or the empty string when the refusal
    /// is about the mount itself rather than a container's content.
    pub fn container(&self) -> &str {
        match self {
            Self::UnreadableContainer { container, .. } | Self::NonUtf8Name { container, .. } => {
                container
            }
            Self::InvalidMemberPath { container, .. } | Self::Member { container, .. } => container,
            Self::Format { source } => source.container(),
            Self::Mount(error) => mount_error_container(error),
            Self::Session(error) => match error {
                SessionError::Source { mount, .. } => mount.as_str(),
                SessionError::Mount(error) => mount_error_container(error),
            },
        }
    }

    /// The absolute container offset the refusal points at, when there is
    /// one (an IO failure or a whole-mount refusal has none).
    pub fn offset(&self) -> Option<u64> {
        match self {
            Self::Format { source } => Some(source.offset()),
            Self::NonUtf8Name { offset, .. } | Self::InvalidMemberPath { offset, .. } => {
                Some(*offset)
            }
            Self::UnreadableContainer { .. }
            | Self::Member { .. }
            | Self::Mount(_)
            | Self::Session(_) => None,
        }
    }
}

impl fmt::Display for RofMountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnreadableContainer {
                container,
                path,
                source,
            } => write!(
                f,
                "cannot read ROF container {path} ({container}): {source}",
                path = path.display()
            ),
            Self::Format { source } => write!(f, "{source}"),
            Self::NonUtf8Name { container, offset } => write!(
                f,
                "the member at offset {offset} in {container} has a name that is not UTF-8; \
                 no key spelling could match it"
            ),
            Self::InvalidMemberPath {
                container,
                offset,
                reason,
            } => write!(
                f,
                "the member at offset {offset} in {container} cannot be spelled as a key: \
                 {reason}"
            ),
            Self::Member { container, source } => write!(f, "in {container}: {source}"),
            Self::Mount(source) => write!(f, "{source}"),
            Self::Session(source) => write!(f, "{source}"),
        }
    }
}

impl std::error::Error for RofMountError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::UnreadableContainer { source, .. } => Some(source),
            Self::Format { source } => Some(source),
            Self::InvalidMemberPath { reason, .. } => Some(reason),
            Self::Member { source, .. } | Self::Mount(source) => Some(source),
            Self::Session(source) => Some(source),
            Self::NonUtf8Name { .. } => None,
        }
    }
}

/// The mount id a whole-mount refusal names, or the empty string when the
/// refusal says nothing about which mount it was.
fn mount_error_container(error: &MountError) -> &str {
    match error {
        MountError::ModClassWithoutBinding { id }
        | MountError::BindingWithoutModClass { id, .. }
        | MountError::DuplicateMountId { id } => id.as_str(),
        MountError::InvalidMemberPath { .. }
        | MountError::SpanOverflow { .. }
        | MountError::DuplicateMember { .. }
        | MountError::EmptyContainer
        | MountError::ContainerNul => "",
    }
}

/// Why a mounted member's bytes could not be read.
#[derive(Debug)]
pub enum RofReadError {
    /// The key is not a member of this source: another namespace, another
    /// variant, or a spelling the container does not hold.
    UnknownMember {
        /// The mount's container label.
        container: String,
        /// The canonical key that was asked for (caller input, never
        /// content out of the container).
        key: String,
    },
    /// The reader refused the member: expansion bomb, decode failure,
    /// unsupported flags or an extent outside the container.
    Format {
        /// The reader's refusal.
        source: RofError,
    },
}

impl RofReadError {
    /// Stable lowercase identifier for reports and structured diagnostics.
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownMember { .. } => "unknown_member",
            Self::Format { source } => source.code(),
        }
    }

    /// The absolute container offset the refusal points at, when there is
    /// one.
    pub fn offset(&self) -> Option<u64> {
        match self {
            Self::UnknownMember { .. } => None,
            Self::Format { source } => Some(source.offset()),
        }
    }
}

impl fmt::Display for RofReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownMember { container, key } => {
                write!(f, "{container} does not hold the member {key}")
            }
            Self::Format { source } => write!(f, "{source}"),
        }
    }
}

impl std::error::Error for RofReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Format { source } => Some(source),
            Self::UnknownMember { .. } => None,
        }
    }
}

/// Why an explicit ROF export was refused.
#[derive(Debug)]
pub enum RofExportError {
    /// The asset was stamped by a session that is not the one exporting it
    /// (a world switch replaced it); its bytes are never reused.
    ForeignSession {
        /// The session asked to export.
        session: u64,
        /// The session that stamped the asset.
        issued_by: u64,
    },
    /// The asset resolves to another mount than the source it was handed,
    /// so the bytes would not be this container's.
    ForeignMount {
        /// The mount the asset names.
        mount: String,
        /// The mount the source was built from.
        expected: String,
    },
    /// The member's bytes could not be read (including an expansion bomb,
    /// which fails here and therefore before anything is written).
    Read(RofReadError),
    /// The name was unsafe or the write failed.
    Export(ExportError),
}

impl fmt::Display for RofExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignSession { session, issued_by } => write!(
                f,
                "session#{session} refuses an asset stamped by session#{issued_by}"
            ),
            Self::ForeignMount { mount, expected } => write!(
                f,
                "the asset resolves to mount {mount}, but these bytes belong to mount {expected}"
            ),
            Self::Read(error) => write!(f, "{error}"),
            Self::Export(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for RofExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read(error) => Some(error),
            Self::Export(error) => Some(error),
            Self::ForeignSession { .. } | Self::ForeignMount { .. } => None,
        }
    }
}

/// One mounted member as a consumer sees it: where its stored extent is
/// and what the mount recorded about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RofMemberInfo {
    /// The member's root-relative spelling inside the container, joined
    /// with `/`.
    pub spelling: String,
    /// The record's id, verbatim (spec F05 non-negotiable #1; opaque
    /// identity, never renumbered).
    pub id: u32,
    /// First byte of the stored extent inside the container.
    pub offset: u64,
    /// Stored length: the record's `raw_length`, the field the reference
    /// extractor reads ([S05]).
    pub stored_len: u64,
    /// Whether the record carries the observed compression bit (flag 2).
    pub compressed: bool,
    /// SHA-256 of the stored extent — exactly the bytes the resolution's
    /// [`SourceSpan`](cs_types::asset_id::SourceSpan) describes.
    pub sha256: ContentHash,
}

/// What one member adds to the index: the record the reader needs to read
/// it again, next to the info above.
struct RofMemberData {
    info: RofMemberInfo,
    record: RofRawRecord,
}

/// One mounted ROF container: the bytes the mount was built from, indexed
/// for reads.
///
/// A source is immutable and holds its container in memory (the mount was
/// built from those same bytes, so the digest recorded at mount time is
/// the digest of what a read returns). It is deliberately not `Clone`:
/// cloning would duplicate the whole container; share it with a reference
/// instead. Dropping it — the teardown of the session or command that
/// owned it — releases those bytes.
pub struct RofSource {
    /// The mount's container label, also the provenance every error and
    /// [`ParseContext`] of this source carries.
    container: String,
    /// The host path the bytes were read from (reported, never written).
    path: PathBuf,
    /// The mount these members belong to, so an asset resolved elsewhere
    /// is refused rather than read out of the wrong container.
    mount_id: MountId,
    /// The key space this source answers.
    namespace: MountNamespace,
    /// The container bytes the mount was built from.
    bytes: Vec<u8>,
    /// The decode ceiling every read is handed.
    limits: RofLimits,
    /// Every file member, in the container's depth-first walk order.
    members: Vec<RofMemberData>,
    /// `logical_key()` -> index into [`Self::members`], mirroring the
    /// mount's member index exactly.
    index: BTreeMap<String, usize>,
}

impl fmt::Debug for RofSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The container bytes are deliberately not printed.
        f.debug_struct("RofSource")
            .field("container", &self.container)
            .field("path", &self.path)
            .field("mount_id", &self.mount_id)
            .field("namespace", &self.namespace)
            .field("bytes", &self.bytes.len())
            .field("limits", &self.limits)
            .field("members", &self.members.len())
            .finish()
    }
}

impl RofSource {
    /// The mount's container label (every span and error records it).
    pub fn container(&self) -> &str {
        &self.container
    }

    /// The host path the container was read from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The mount these members belong to.
    pub fn mount_id(&self) -> &MountId {
        &self.mount_id
    }

    /// The key space this source answers.
    pub fn namespace(&self) -> &MountNamespace {
        &self.namespace
    }

    /// The decode ceiling every read of this source is handed.
    pub fn limits(&self) -> RofLimits {
        self.limits
    }

    /// How many file members the container holds.
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// Every member, in the container's depth-first walk order.
    pub fn members(&self) -> impl Iterator<Item = &RofMemberInfo> {
        self.members.iter().map(|data| &data.info)
    }

    /// What the mount records about `key`'s member, if this source holds
    /// it. The namespace and variant are checked exactly as the mount
    /// checks them, so a key of another key space is never answered.
    pub fn member(&self, key: &AssetKey) -> Option<&RofMemberInfo> {
        self.lookup(key).ok().map(|index| &self.members[index].info)
    }

    /// Reads `key`'s member through the production reader: verbatim bytes
    /// for an uncompressed member, the bounded zlib decoder for a
    /// compressed one.
    ///
    /// The extent is established and the flags explained *inside*
    /// `cs_formats::read_member`, so an expansion bomb stops at
    /// [`RofLimits::max_decoded_bytes`] and no failure returns partial
    /// output (spec F05 non-negotiable #3).
    pub fn read(&self, key: &AssetKey) -> Result<RofMemberRead, RofReadError> {
        let data = &self.members[self.lookup(key)?];
        let context = ParseContext::with_defaults(self.container.as_str());
        let start = data.info.offset;
        let stored = u64::from(data.record.raw_length);
        let on_disk = u64::from(data.record.raw_length_on_disk);
        let member = RofMember {
            path: self.segments(&data.info.spelling),
            record: data.record,
            start,
            length_end: start + stored,
            length_on_disk_end: start + on_disk,
        };
        read_member(&context, &self.bytes, &member, &self.limits)
            .map_err(|source| RofReadError::Format { source })
    }

    /// The index position of `key`, or the refusal that explains why this
    /// source does not answer it.
    fn lookup(&self, key: &AssetKey) -> Result<usize, RofReadError> {
        if *key.namespace() != self.namespace || *key.variant() != AssetVariant::default() {
            return Err(self.unknown(key));
        }
        self.index
            .get(key.path_key())
            .copied()
            .ok_or_else(|| self.unknown(key))
    }

    fn unknown(&self, key: &AssetKey) -> RofReadError {
        RofReadError::UnknownMember {
            container: self.container.clone(),
            key: key.logical_key(),
        }
    }

    /// The spelling's path segments, as the reader's record type wants
    /// them: the mount joined them with `/`, so splitting on `/` gives the
    /// segments back (a name that itself contains a separator is nested,
    /// which is how its key was validated).
    fn segments<'a>(&self, spelling: &'a str) -> Vec<&'a [u8]> {
        spelling.split('/').map(str::as_bytes).collect()
    }
}

/// A mounted container: the [`Mount`] the VFS holds and the [`RofSource`]
/// its members are read from.
///
/// Both are built from one read of the container, or neither exists.
#[derive(Debug)]
pub struct MountedRof {
    /// The mount, ready for [`SessionBuilder::mount`].
    pub mount: Mount,
    /// The source its members are read and exported through.
    pub source: RofSource,
}

/// Mounts `container_path` as `builder` describes it, with the default
/// decode ceiling ([`RofLimits::default`]).
///
/// See [`mount_rof_with_limits`] for the behaviour; this is the surface a
/// caller that has no reason to configure the ceiling uses.
pub fn mount_rof(
    builder: MountBuilder,
    container_path: &Path,
) -> Result<MountedRof, RofMountError> {
    mount_rof_with_limits(builder, container_path, RofLimits::default())
}

/// Mounts `container_path` as `builder` describes it, decoding members no
/// further than `limits` ever allows.
///
/// The container is read once, walked by the production `read_tree`, and
/// every file member becomes one mount member:
///
/// * its spelling is the member's root-relative path joined with `/`,
///   validated as a key (`RofMountError::InvalidMemberPath` otherwise, and
///   `RofMountError::NonUtf8Name` when it is not UTF-8);
/// * its extent is `start .. start + raw_length`, already proven inside
///   the container by the walk;
/// * its digest is the SHA-256 of exactly those stored bytes, so the
///   [`SourceSpan`](cs_types::asset_id::SourceSpan) a resolution returns
///   describes bytes that exist and hash to what the span says.
///
/// **Nothing is written anywhere, and a refusal leaves no trace**: every
/// error above is returned before the [`Mount`] is built, so a caller
/// that then joins a session finds it exactly as it was (and can retry the
/// same container — a refusal is deterministic) — spec F05 AC03, "fail
/// before writes". Member *contents* are not decoded here: an expansion
/// bomb or a corrupt stream fails when that member is read
/// ([`RofSource::read`]), not while the container is enumerated.
pub fn mount_rof_with_limits(
    mut builder: MountBuilder,
    container_path: &Path,
    limits: RofLimits,
) -> Result<MountedRof, RofMountError> {
    let container = builder.container().to_owned();
    let bytes = fs::read(container_path).map_err(|source| RofMountError::UnreadableContainer {
        container: container.clone(),
        path: container_path.to_path_buf(),
        source,
    })?;

    let mut context = ParseContext::with_defaults(container.as_str());
    let tree =
        read_tree(&mut context, &bytes).map_err(|source| RofMountError::Format { source })?;

    let mut members = Vec::with_capacity(tree.members().len());
    let mut index = BTreeMap::new();
    for member in tree.members() {
        let offset = member.start;
        let spelling = join_spelling(&member.path, &container, offset)?;
        let relative =
            RelativePath::new(&spelling).map_err(|reason| RofMountError::InvalidMemberPath {
                container: container.clone(),
                offset,
                reason,
            })?;

        // `read_tree` proved both ends of this extent lie inside the
        // container; the `get` is that same claim checked once more before
        // a slice exists, so a producer that ever broke the contract would
        // be refused here instead of panicking.
        let stored_len = u64::from(member.record.raw_length);
        let end = offset + stored_len;
        let start_index = usize::try_from(offset).expect("offset <= container length");
        let end_index = usize::try_from(end).expect("end <= container length");
        let extent = bytes
            .get(start_index..end_index)
            .ok_or_else(|| RofError::ExtentOutOfBounds {
                container: container.clone(),
                offset,
                start: offset,
                length: stored_len,
                file_len: bytes.len() as u64,
            })
            .map_err(|source| RofMountError::Format { source })?;
        let digest = sha256(extent);

        builder
            .add_member(&spelling, stored_len, offset, Some(digest))
            .map_err(|source| RofMountError::Member {
                container: container.clone(),
                source,
            })?;
        index.insert(relative.logical_key(), members.len());
        members.push(RofMemberData {
            info: RofMemberInfo {
                spelling,
                id: member.record.id,
                offset,
                stored_len,
                compressed: member.record.flags.is_compressed(),
                sha256: digest,
            },
            record: member.record,
        });
    }

    let mount = builder.build().map_err(RofMountError::Mount)?;
    let mount_id = mount.id().clone();
    let namespace = mount.namespace().clone();
    Ok(MountedRof {
        mount,
        source: RofSource {
            container,
            path: container_path.to_path_buf(),
            mount_id,
            namespace,
            bytes,
            limits,
            members,
            index,
        },
    })
}

/// Mounts `container_path` into `session` and returns the source its
/// members are read from.
///
/// This is the wiring a content session wants: on any refusal the session
/// is untouched (nothing was added), so the caller can retry this
/// container, mount another instead, or drop the builder — F04-C's
/// mount-failure contract, applied to archive members.
pub fn mount_rof_into(
    session: &mut SessionBuilder,
    builder: MountBuilder,
    container_path: &Path,
) -> Result<RofSource, RofMountError> {
    let mounted = mount_rof(builder, container_path)?;
    let source = mounted.source;
    session
        .mount(mounted.mount)
        .map_err(RofMountError::Session)?;
    Ok(source)
}

/// Reads `asset` through `source` (digest-checked extent first, bounded
/// decode second) and writes it below `directory` under the member's own
/// spelling.
///
/// The explicit private research export of spec F04 non-negotiable
/// behavior 5, the ROF counterpart of [`crate::vfs::export_asset`]: the
/// name is validated before a byte is read, the member is read before a
/// byte is written — so a refused layout, an unknown key or an expansion
/// bomb fails **before any write** — and the bytes handed to
/// [`ExportDirectory::write`] are the decoded ones, not the compressed
/// extent.
pub fn export_rof_member(
    session: &ContentSession,
    asset: &SessionAsset,
    source: &RofSource,
    directory: &ExportDirectory,
) -> Result<ExportedFile, RofExportError> {
    if asset.generation() != session.generation() {
        return Err(RofExportError::ForeignSession {
            session: session.generation().get(),
            issued_by: asset.generation().get(),
        });
    }
    if asset.resolved().mount != source.mount_id {
        return Err(RofExportError::ForeignMount {
            mount: asset.resolved().mount.to_string(),
            expected: source.mount_id.to_string(),
        });
    }
    let name = asset
        .resolved()
        .span
        .member_key()
        .unwrap_or(asset.resolved().span.container_path());
    // Validate before reading, so a hostile name costs no IO at all.
    export_components(name).map_err(RofExportError::Export)?;
    let read = source
        .read(&asset.resolved().key)
        .map_err(RofExportError::Read)?;
    directory
        .write(name, &read.data)
        .map_err(RofExportError::Export)
}

/// Joins a member's path segments into one `/`-separated spelling.
///
/// Names are bytes (spec F05 non-negotiable #2): a segment that is not
/// UTF-8 has no key spelling a lookup could match, so the mount is refused
/// rather than lossily renamed. The refusal names the container and the
/// member's extent offset, never the bytes of the name itself.
fn join_spelling(path: &[&[u8]], container: &str, offset: u64) -> Result<String, RofMountError> {
    let mut spelling = String::new();
    for (position, segment) in path.iter().enumerate() {
        if position > 0 {
            spelling.push('/');
        }
        let text = std::str::from_utf8(segment).map_err(|_| RofMountError::NonUtf8Name {
            container: container.to_owned(),
            offset,
        })?;
        spelling.push_str(text);
    }
    Ok(spelling)
}

#[cfg(test)]
mod tests {
    //! F05-C acceptance tests (prefix `accept_f05_c_`): a container mounted
    //! into a real content session, its members read back through the
    //! production decoder, and the AC03 minimum scenario — cycle, invalid
    //! name table, outside-file pointer and expansion bomb — failing
    //! *before any write*.
    //!
    //! Every byte below is authored here (the zlib streams carry their
    //! generator command, as in `crates/cs_formats/tests/rof.rs`): newly
    //! authored synthetic content, no original game data, no
    //! `CS_GAME_DIR` access. Containers are written under the system
    //! temporary directory and removed when the test finishes.

    use std::sync::atomic::{AtomicU64, Ordering};

    use cs_formats::{DIRECTORY_HEADER_BYTES, FLAG_COMPRESSED, FLAG_DIRECTORY, RECORD_BYTES};
    use cs_types::asset_id::{MountId, MountNamespace, PrecedenceClass, ResolveContext};

    use super::*;
    use crate::vfs::{MountError, ReadError};

    /// Provenance label every container of these tests mounts under.
    const GOOD: &str = "good.rof";
    const CYCLE: &str = "cycle.rof";
    const NAMES: &str = "names.rof";
    const OUTSIDE: &str = "outside.rof";
    const BOMB: &str = "bomb.rof";
    const UTF8: &str = "utf8.rof";
    const CASE: &str = "case.rof";
    const ABSENT: &str = "absent.rof";
    const OTHER: &str = "other.rof";

    /// The payload of the compressed fixtures: 204 bytes, four repetitions
    /// of one authored line.
    const COMPRESSED_PAYLOAD: &[u8] = &[
        0x43, 0x72, 0x69, 0x6d, 0x73, 0x6f, 0x6e, 0x20, 0x53, 0x6b, 0x69, 0x65, 0x73, 0x20, 0x73,
        0x79, 0x6e, 0x74, 0x68, 0x65, 0x74, 0x69, 0x63, 0x20, 0x63, 0x6f, 0x6d, 0x70, 0x72, 0x65,
        0x73, 0x73, 0x65, 0x64, 0x20, 0x6d, 0x65, 0x6d, 0x62, 0x65, 0x72, 0x20, 0x70, 0x61, 0x79,
        0x6c, 0x6f, 0x61, 0x64, 0x2e, 0x0a, 0x43, 0x72, 0x69, 0x6d, 0x73, 0x6f, 0x6e, 0x20, 0x53,
        0x6b, 0x69, 0x65, 0x73, 0x20, 0x73, 0x79, 0x6e, 0x74, 0x68, 0x65, 0x74, 0x69, 0x63, 0x20,
        0x63, 0x6f, 0x6d, 0x70, 0x72, 0x65, 0x73, 0x73, 0x65, 0x64, 0x20, 0x6d, 0x65, 0x6d, 0x62,
        0x65, 0x72, 0x20, 0x70, 0x61, 0x79, 0x6c, 0x6f, 0x61, 0x64, 0x2e, 0x0a, 0x43, 0x72, 0x69,
        0x6d, 0x73, 0x6f, 0x6e, 0x20, 0x53, 0x6b, 0x69, 0x65, 0x73, 0x20, 0x73, 0x79, 0x6e, 0x74,
        0x68, 0x65, 0x74, 0x69, 0x63, 0x20, 0x63, 0x6f, 0x6d, 0x70, 0x72, 0x65, 0x73, 0x73, 0x65,
        0x64, 0x20, 0x6d, 0x65, 0x6d, 0x62, 0x65, 0x72, 0x20, 0x70, 0x61, 0x79, 0x6c, 0x6f, 0x61,
        0x64, 0x2e, 0x0a, 0x43, 0x72, 0x69, 0x6d, 0x73, 0x6f, 0x6e, 0x20, 0x53, 0x6b, 0x69, 0x65,
        0x73, 0x20, 0x73, 0x79, 0x6e, 0x74, 0x68, 0x65, 0x74, 0x69, 0x63, 0x20, 0x63, 0x6f, 0x6d,
        0x70, 0x72, 0x65, 0x73, 0x73, 0x65, 0x64, 0x20, 0x6d, 0x65, 0x6d, 0x62, 0x65, 0x72, 0x20,
        0x70, 0x61, 0x79, 0x6c, 0x6f, 0x61, 0x64, 0x2e, 0x0a,
    ];

    /// [`COMPRESSED_PAYLOAD`] as a zlib stream (62 bytes), produced by
    /// `python3 -c "import zlib; ..."` (CPython 3.14, zlib 1.2.12) — an
    /// implementation that shares no code with the decoder under test
    /// (`docs/research/FORMAT-NOTES.md`).
    const COMPRESSED_STREAM: &[u8] = &[
        0x78, 0x9c, 0x73, 0x2e, 0xca, 0xcc, 0x2d, 0xce, 0xcf, 0x53, 0x08, 0xce, 0xce, 0x4c, 0x2d,
        0x56, 0x28, 0xae, 0xcc, 0x2b, 0xc9, 0x48, 0x2d, 0xc9, 0x4c, 0x56, 0x48, 0xce, 0xcf, 0x2d,
        0x28, 0x4a, 0x2d, 0x2e, 0x4e, 0x4d, 0x51, 0xc8, 0x4d, 0xcd, 0x4d, 0x4a, 0x2d, 0x52, 0x28,
        0x48, 0xac, 0xcc, 0xc9, 0x4f, 0x4c, 0xd1, 0xe3, 0x72, 0x1e, 0xac, 0x5a, 0x00, 0xd7, 0xca,
        0x4c, 0x91,
    ];

    /// 128 KiB of zero bytes as a zlib stream (149 bytes): a stored extent
    /// far smaller than what it decodes to, i.e. an expansion bomb. Same
    /// provenance as [`COMPRESSED_STREAM`].
    const BOMB_STREAM: &[u8] = &[
        0x78, 0xda, 0xed, 0xc1, 0x31, 0x01, 0x00, 0x00, 0x00, 0xc2, 0xa0, 0xf5, 0x4f, 0xed, 0x61,
        0x0d, 0xa0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x6e, 0x00, 0x1e, 0x00, 0x01,
    ];

    /// The payloads the good container holds.
    const INDEX: &[u8] = b"root index\n";
    const README: &[u8] = b"shared readme (MIS)\n";

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// A disposable directory, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "cs-f05-c-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("temp dir is created");
            Self(root)
        }

        /// Writes one container and returns its host path.
        fn container(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, bytes).expect("fixture bytes are written");
            path
        }

        /// Creates the private export directory and returns it.
        fn export(&self) -> PathBuf {
            let path = self.0.join("export");
            fs::create_dir_all(&path).expect("export dir is created");
            path
        }

        /// Every entry below `path`, sorted — an empty vec means *nothing*
        /// was written, not even a temporary file.
        fn entries(&self, path: &Path) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(path)
                .expect("directory is readable")
                .map(|entry| {
                    entry
                        .expect("entry")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// One record exactly as authored: the six on-disk fields, verbatim.
    #[derive(Clone, Copy)]
    struct RawRecord {
        start: u32,
        raw_length: u32,
        raw_length_on_disk: u32,
        flags: u32,
        name_length: u32,
        id: u32,
    }

    impl RawRecord {
        fn file(name: &str, id: u32) -> Self {
            Self {
                start: 0,
                raw_length: 0,
                raw_length_on_disk: 0,
                flags: 0,
                name_length: name.len() as u32 + 1,
                id,
            }
        }

        fn directory(name: &str, id: u32, start: u32) -> Self {
            Self {
                flags: FLAG_DIRECTORY,
                start,
                ..Self::file(name, id)
            }
        }

        fn at(mut self, start: u32, length: u32) -> Self {
            self.start = start;
            self.raw_length = length;
            self.raw_length_on_disk = length;
            self
        }

        fn compressed(mut self) -> Self {
            self.flags = FLAG_COMPRESSED;
            self
        }

        /// Authors the two length words differently: the compressed
        /// fixtures are exactly the case where they must not be collapsed
        /// (spec F05 non-negotiable #4).
        fn on_disk(mut self, length: u32) -> Self {
            self.raw_length_on_disk = length;
            self
        }
    }

    fn name_table(names: &[&str]) -> Vec<u8> {
        let mut table = Vec::new();
        for name in names {
            table.extend_from_slice(name.as_bytes());
            table.push(0);
        }
        table
    }

    /// Encodes one directory block; the header is a parameter so a test
    /// can author one that disagrees with the tables it points at.
    fn block(entry_count: u32, names_length: u32, records: &[RawRecord], names: &[u8]) -> Vec<u8> {
        let mut bytes =
            Vec::with_capacity(DIRECTORY_HEADER_BYTES + records.len() * RECORD_BYTES + names.len());
        bytes.extend_from_slice(&entry_count.to_le_bytes());
        bytes.extend_from_slice(&names_length.to_le_bytes());
        for record in records {
            bytes.extend_from_slice(&record.start.to_le_bytes());
            bytes.extend_from_slice(&record.raw_length.to_le_bytes());
            bytes.extend_from_slice(&record.raw_length_on_disk.to_le_bytes());
            bytes.extend_from_slice(&record.flags.to_le_bytes());
            bytes.extend_from_slice(&record.name_length.to_le_bytes());
            bytes.extend_from_slice(&record.id.to_le_bytes());
        }
        bytes.extend_from_slice(names);
        bytes
    }

    fn valid_block(records: &[RawRecord], names: &[u8]) -> Vec<u8> {
        block(records.len() as u32, names.len() as u32, records, names)
    }

    /// Places one payload at `cursor` and advances past it.
    fn place(cursor: &mut u32, payload: &[u8]) -> (u32, u32) {
        let start = *cursor;
        *cursor += payload.len() as u32;
        (start, payload.len() as u32)
    }

    /// A root block with one directory record pointing at `start`.
    fn root_pointing_at(start: u32) -> Vec<u8> {
        let names = name_table(&["LOOP"]);
        valid_block(&[RawRecord::directory("LOOP", 1, start)], &names)
    }

    /// A single file entry claiming `length` bytes the container does not
    /// hold.
    fn outside_pointer() -> Vec<u8> {
        let names = name_table(&["FAR.DAT"]);
        let block_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + names.len();
        let records = [RawRecord::file("FAR.DAT", 5).at(block_len as u32, 1000)];
        valid_block(&records, &names)
    }

    /// One file member holding `payload`: a second container beside
    /// [`good_container`] for a session that mounts two of them.
    fn single_member_container(name: &str, payload: &[u8]) -> Vec<u8> {
        let names = name_table(&[name]);
        let block_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + names.len();
        let records = [RawRecord::file(name, 4).at(block_len as u32, payload.len() as u32)];
        let mut bytes = valid_block(&records, &names);
        bytes.extend_from_slice(payload);
        bytes
    }

    /// `[root][MIS][INDEX][README][PACK]`: a directory, an uncompressed
    /// file, a compressed member whose two length words differ (62 stored,
    /// 32 "on disk"), and a duplicate basename under the directory.
    fn good_container() -> Vec<u8> {
        let root_names = name_table(&["MIS", "INDEX.TXT", "PACK.DAT"]);
        let mis_names = name_table(&["readme.txt"]);
        let root_len = DIRECTORY_HEADER_BYTES + 3 * RECORD_BYTES + root_names.len();
        let mis_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + mis_names.len();
        let mut cursor = (root_len + mis_len) as u32;
        let (index_start, index_len) = place(&mut cursor, INDEX);
        let (readme_start, readme_len) = place(&mut cursor, README);
        let (pack_start, pack_len) = place(&mut cursor, COMPRESSED_STREAM);

        let root_records = [
            RawRecord::directory("MIS", 1, root_len as u32),
            RawRecord::file("INDEX.TXT", 2).at(index_start, index_len),
            RawRecord::file("PACK.DAT", 3)
                .at(pack_start, pack_len)
                .compressed()
                .on_disk(32),
        ];
        let mis_records = [RawRecord::file("readme.txt", 11).at(readme_start, readme_len)];

        let mut bytes = valid_block(&root_records, &root_names);
        bytes.extend_from_slice(&valid_block(&mis_records, &mis_names));
        bytes.extend_from_slice(INDEX);
        bytes.extend_from_slice(README);
        bytes.extend_from_slice(COMPRESSED_STREAM);
        assert_eq!(
            bytes.len(),
            cursor as usize,
            "every payload was placed once"
        );
        bytes
    }

    /// A root block whose only record is a directory pointing at offset
    /// zero — itself.
    fn cycle_container() -> Vec<u8> {
        root_pointing_at(0)
    }

    /// A root block whose header declares fewer name bytes than its
    /// records describe, so the two tables disagree about where names end.
    fn bad_name_table_container() -> Vec<u8> {
        let names = name_table(&["LOOP"]);
        let records = [RawRecord::directory("LOOP", 1, 0)];
        block(1, names.len() as u32 - 4, &records, &names)
    }

    /// A single compressed member holding the 149-byte bomb stream.
    fn bomb_container() -> Vec<u8> {
        let names = name_table(&["BOMB.DAT"]);
        let block_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + names.len();
        let records = [RawRecord::file("BOMB.DAT", 7)
            .at(block_len as u32, BOMB_STREAM.len() as u32)
            .compressed()];
        let mut bytes = valid_block(&records, &names);
        bytes.extend_from_slice(BOMB_STREAM);
        bytes
    }

    /// A single member whose name is not valid UTF-8.
    fn non_utf8_container() -> Vec<u8> {
        let names = [b'D', 0xFF, b'T', 0];
        let block_len = DIRECTORY_HEADER_BYTES + RECORD_BYTES + names.len();
        let records = [RawRecord {
            start: block_len as u32,
            raw_length: 0,
            raw_length_on_disk: 0,
            flags: 0,
            name_length: names.len() as u32,
            id: 1,
        }];
        valid_block(&records, &names)
    }

    /// Two members whose names differ only in case: one logical key.
    fn case_collision_container() -> Vec<u8> {
        let names = name_table(&["ALERT.DDS", "alert.dds"]);
        let block_len = DIRECTORY_HEADER_BYTES + 2 * RECORD_BYTES + names.len();
        let records = [
            RawRecord::file("ALERT.DDS", 1).at(block_len as u32, 0),
            RawRecord::file("alert.dds", 2).at(block_len as u32, 0),
        ];
        valid_block(&records, &names)
    }

    /// The mount builder a caller hands to [`mount_rof`]: one install-space
    /// key space, the container label every span and error records.
    fn mount_builder(label: &str) -> MountBuilder {
        let id = format!("rof-{}", label.replace('.', "-"));
        MountBuilder::new(
            MountId::new(&id).expect("a valid mount id"),
            MountNamespace::new("install").expect("a valid namespace"),
            PrecedenceClass::Shared,
            label,
        )
    }

    fn context() -> ResolveContext {
        ResolveContext::new(sha256(b"f05-c synthetic fixture"))
    }

    fn key(path: &str) -> AssetKey {
        AssetKey::from_spelling("install", path, "default").expect("a valid fixture key")
    }

    /// **The positive path:** a container mounted into one content session
    /// resolves to spans that describe the stored extents, reads back
    /// through the production decoder, and exports its decoded bytes.
    #[test]
    fn accept_f05_c_mounted_members_resolve_read_and_export() {
        let temp = Temp::new("mount");
        let path = temp.container(GOOD, &good_container());
        let bytes = fs::read(&path).expect("fixture bytes");

        let mut builder = SessionBuilder::new(context());
        let source = mount_rof_into(&mut builder, mount_builder(GOOD), &path)
            .expect("the authored container mounts");

        // Depth-first walk order, verbatim ids, both length words kept
        // apart: the mount records the stored extent of every member.
        assert_eq!(source.member_count(), 3);
        let members: Vec<RofMemberInfo> = source.members().cloned().collect();
        assert_eq!(
            members
                .iter()
                .map(|member| member.spelling.as_str())
                .collect::<Vec<_>>(),
            vec!["MIS/readme.txt", "INDEX.TXT", "PACK.DAT"]
        );
        assert_eq!(
            members.iter().map(|member| member.id).collect::<Vec<_>>(),
            vec![11, 2, 3],
            "ids are preserved verbatim"
        );
        assert!(!members[0].compressed && !members[1].compressed);
        assert!(members[2].compressed, "flag 2 is the compression bit");
        assert_eq!(
            members[2].stored_len, 62,
            "the record's `raw_length` — the other word authors 32 for this member"
        );

        let session = builder.open();
        let index_asset = session.resolve(&key("INDEX.TXT")).expect("resolved");
        let index = &members[1];
        assert_eq!(index_asset.resolved().span.offset(), index.offset);
        assert_eq!(index_asset.resolved().span.length(), index.stored_len);
        assert_eq!(
            index_asset.resolved().span.member_sha256(),
            Some(index.sha256),
            "the span's digest is the digest of its own extent"
        );
        assert_eq!(
            index_asset.resolved().span.member_sha256(),
            Some(sha256(
                &bytes[index.offset as usize..(index.offset + index.stored_len) as usize]
            )),
            "the digest covers exactly the stored bytes in the container"
        );
        assert_eq!(source.read(&key("INDEX.TXT")).expect("reads").data, INDEX);

        // The compressed member: the span describes 62 stored bytes, the
        // read produces 204 decoded ones, and the recorded digest is of
        // the stored extent — not of the decoded payload.
        let pack_asset = session.resolve(&key("PACK.DAT")).expect("resolved");
        assert_eq!(pack_asset.resolved().span.length(), 62);
        assert_ne!(
            pack_asset.resolved().span.member_sha256(),
            Some(sha256(COMPRESSED_PAYLOAD)),
            "the span never claims to describe the decoded bytes"
        );
        let pack = source.read(&key("PACK.DAT")).expect("decodes");
        assert_eq!(pack.data, COMPRESSED_PAYLOAD);
        assert_eq!(pack.stored_len, 62);
        assert_eq!(pack.decoded_len, 204);
        assert_eq!(pack.trailing_len, 0);

        // The VFS-level read path is host-file backed (F04-B) and says so
        // instead of returning compressed bytes as if they were content.
        let error = session
            .read_all(&pack_asset)
            .expect_err("a container member has no host file range");
        assert!(
            matches!(error, ReadError::NoBacking { .. }),
            "unexpected {error:?}"
        );

        // A key this source does not hold is refused with its stable code
        // and the container label, while the keys it does hold keep
        // reading.
        let missing = source.read(&key("MISSING.DAT")).expect_err("not a member");
        assert_eq!(missing.code(), "unknown_member", "{missing}");
        assert!(missing.to_string().contains(GOOD), "{missing}");
        let wrong_space = source
            .read(&AssetKey::from_spelling("world", "INDEX.TXT", "default").expect("a key"))
            .expect_err("another namespace is not answered");
        assert_eq!(wrong_space.code(), "unknown_member", "{wrong_space}");
        assert_eq!(source.read(&key("INDEX.TXT")).expect("reads").data, INDEX);

        // The explicit export writes the *decoded* bytes below the private
        // directory: a subdirectory for a nested spelling, and the
        // compressed member as its 204-byte payload — never the 62-byte
        // stored stream that sits in the container.
        let export = temp.export();
        let directory = ExportDirectory::open(&export, &session).expect("export dir opens");
        let readme_asset = session.resolve(&key("MIS/readme.txt")).expect("resolved");
        let readme_file =
            export_rof_member(&session, &readme_asset, &source, &directory).expect("exports");
        assert_eq!(readme_file.size_bytes, README.len() as u64);
        assert_eq!(
            fs::read(export.join("MIS/readme.txt")).expect("the export is written"),
            README
        );
        let pack_file =
            export_rof_member(&session, &pack_asset, &source, &directory).expect("exports");
        assert_eq!(pack_file.size_bytes, COMPRESSED_PAYLOAD.len() as u64);
        assert_eq!(
            fs::read(export.join("PACK.DAT")).expect("the export is written"),
            COMPRESSED_PAYLOAD,
            "the export hands over decoded bytes, not the stored stream"
        );

        // Teardown: closing the session releases exactly this mount.
        let teardown = session.close();
        assert_eq!(teardown.released.len(), 1);
        assert_eq!(teardown.released[0].as_str(), "rof-good-rof");
        assert_eq!(
            temp.entries(&export),
            vec!["MIS".to_owned(), "PACK.DAT".to_owned()]
        );
    }

    /// **AC03 (mount half of the minimum scenario):** a cycle, an invalid
    /// name table and an outside-file pointer fail *before any write* —
    /// no mount exists afterwards, the session keeps the mounts it
    /// already had, and a retry refuses them again at the same offset.
    #[test]
    fn accept_f05_c_mount_refuses_cycles_bad_name_tables_and_outside_pointers() {
        let temp = Temp::new("refuse");
        let good = temp.container(GOOD, &good_container());
        let cycle = temp.container(CYCLE, &cycle_container());
        let names = temp.container(NAMES, &bad_name_table_container());
        let outside = temp.container(OUTSIDE, &outside_pointer());
        let export = temp.export();

        let mut builder = SessionBuilder::new(context());
        let good_source = mount_rof_into(&mut builder, mount_builder(GOOD), &good)
            .expect("the authored container mounts");
        assert_eq!(builder.len(), 1);

        let refused = [
            (CYCLE, &cycle, "cycle"),
            (NAMES, &names, "name_table_length"),
            (OUTSIDE, &outside, "extent_out_of_bounds"),
        ];
        for (label, path, code) in refused {
            let error = mount_rof_into(&mut builder, mount_builder(label), path)
                .expect_err("the refusal must not mount");
            assert_eq!(error.code(), code, "{error}");
            assert_eq!(error.container(), label);
            assert!(error.offset().is_some(), "{error} names an offset");
            assert_eq!(
                builder.len(),
                1,
                "{label} left no partial mount behind: the builder still holds exactly the \
                 mount it held before"
            );
        }

        // The refusal is deterministic, so a retry says exactly the same.
        let first = mount_rof(mount_builder(CYCLE), &cycle).expect_err("cycle is refused");
        let second = mount_rof(mount_builder(CYCLE), &cycle).expect_err("cycle is refused");
        assert_eq!(first.code(), second.code());
        assert_eq!(first.offset(), second.offset());
        assert_eq!(first.to_string(), second.to_string());

        // The session that refused them still serves the mount it has,
        // and teardown reports exactly that one.
        let session = builder.open();
        let asset = session.resolve(&key("INDEX.TXT")).expect("still resolves");
        assert_eq!(asset.resolved().span.length(), INDEX.len() as u64);
        assert_eq!(
            good_source.read(&key("INDEX.TXT")).expect("reads").data,
            INDEX
        );
        let teardown = session.close();
        assert_eq!(teardown.released.len(), 1);

        // And nothing anywhere was written: the export directory is
        // still empty, not even a temporary file.
        assert!(
            temp.entries(&export).is_empty(),
            "a refused container wrote {:?}",
            temp.entries(&export)
        );
    }

    /// **AC03 (the expansion bomb):** the container mounts — enumeration
    /// decodes nothing — and the bomb fails when that member is read,
    /// *before* the export writes a byte.
    #[test]
    fn accept_f05_c_expansion_bomb_fails_before_any_write() {
        let temp = Temp::new("bomb");
        let path = temp.container(BOMB, &bomb_container());
        let export = temp.export();

        // Under a 4 KiB ceiling the 149-byte stream is a bomb.
        let mounted = mount_rof_with_limits(mount_builder(BOMB), &path, RofLimits::new(4096))
            .expect("the container itself enumerates: reading no member decodes nothing");
        assert_eq!(mounted.source.member_count(), 1);
        let info = mounted.source.members().next().expect("one member").clone();
        assert_eq!(info.stored_len, BOMB_STREAM.len() as u64);
        assert!(info.compressed);

        let mut builder = SessionBuilder::new(context());
        builder
            .mount(mounted.mount)
            .expect("the mount joins the session");
        let session = builder.open();
        let asset = session.resolve(&key("BOMB.DAT")).expect("resolved");
        let directory = ExportDirectory::open(&export, &session).expect("export dir opens");

        let error = export_rof_member(&session, &asset, &mounted.source, &directory)
            .expect_err("the bomb must be refused");
        match &error {
            RofExportError::Read(read) => {
                assert_eq!(read.code(), "expansion_bomb", "{read}");
                assert_eq!(read.offset(), Some(info.offset));
            }
            other => panic!("expected a read refusal, got {other:?}"),
        }
        assert!(
            temp.entries(&export).is_empty(),
            "the refusal happened before any write, found {:?}",
            temp.entries(&export)
        );

        // The same member under the default ceiling decodes in full: the
        // bound is the configured limit, not a hard failure.
        let unbounded = mount_rof(mount_builder(BOMB), &path).expect("mounts");
        let read = unbounded.source.read(&key("BOMB.DAT")).expect("decodes");
        assert_eq!(read.stored_len, BOMB_STREAM.len() as u64);
        assert_eq!(read.decoded_len, 128 * 1024);
        assert_eq!(read.data.len(), 128 * 1024);
        session.close();
    }

    /// A member name that is not UTF-8 has no key spelling, and two names
    /// that fold onto one key are refused with both spellings — the mount
    /// never flattens or lossily renames content (spec F04 non-negotiable
    /// behavior 1 and 3; spec F05 non-negotiable #2).
    #[test]
    fn accept_f05_c_unspellable_members_refuse_the_mount() {
        let temp = Temp::new("spellings");

        let path = temp.container(UTF8, &non_utf8_container());
        let error = mount_rof(mount_builder(UTF8), &path).expect_err("no key spelling exists");
        assert_eq!(error.code(), "non_utf8_name");
        assert_eq!(error.container(), UTF8);
        // The member's stored extent starts right after the block.
        assert_eq!(error.offset(), Some(36));

        let path = temp.container(CASE, &case_collision_container());
        let error = mount_rof(mount_builder(CASE), &path).expect_err("one key, two spellings");
        assert_eq!(error.code(), "member");
        match &error {
            RofMountError::Member {
                source: MountError::DuplicateMember { .. },
                ..
            } => {}
            other => panic!("expected a duplicate member, got {other:?}"),
        }
        let message = error.to_string();
        assert!(message.contains("ALERT.DDS"), "{message}");
        assert!(message.contains("alert.dds"), "{message}");

        // Both refusals happened before a mount existed.
        let mut builder = SessionBuilder::new(context());
        assert_eq!(builder.len(), 0);
        assert!(mount_rof_into(&mut builder, mount_builder(UTF8), &path).is_err());
        assert_eq!(builder.len(), 0);
    }

    /// **Error propagation (IO):** a container that cannot be read at all
    /// is refused before a mount exists — the builder is never touched,
    /// the diagnostic names the label and the host path, and a retry
    /// refuses identically (F04-C's mount-failure contract, teardown and
    /// retry).
    #[test]
    fn accept_f05_c_unreadable_container_refuses_before_a_mount() {
        let temp = Temp::new("unreadable");
        let missing = temp.0.join(ABSENT);

        let mut builder = SessionBuilder::new(context());
        let error = mount_rof_into(&mut builder, mount_builder(ABSENT), &missing)
            .expect_err("there is no container to read");
        assert_eq!(error.code(), "unreadable_container");
        assert_eq!(error.container(), ABSENT);
        assert_eq!(
            error.offset(),
            None,
            "an IO refusal has no container offset"
        );
        assert!(
            error.to_string().contains(&missing.display().to_string()),
            "{error}"
        );
        assert_eq!(builder.len(), 0, "the refusal left no partial mount behind");

        let first = mount_rof(mount_builder(ABSENT), &missing).expect_err("still absent");
        let second = mount_rof(mount_builder(ABSENT), &missing).expect_err("still absent");
        assert_eq!(first.code(), second.code());
        assert_eq!(first.to_string(), second.to_string());
    }

    /// **Error propagation (session):** the session refuses a repeated
    /// mount id after the container itself was accepted. Nothing of the
    /// duplicate survives — the builder keeps exactly the mount it held,
    /// and that mount's source still resolves and reads.
    #[test]
    fn accept_f05_c_session_refuses_a_repeated_mount() {
        let temp = Temp::new("repeat");
        let path = temp.container(GOOD, &good_container());

        let mut builder = SessionBuilder::new(context());
        let first = mount_rof_into(&mut builder, mount_builder(GOOD), &path)
            .expect("the authored container mounts");
        assert_eq!(builder.len(), 1);

        let error = mount_rof_into(&mut builder, mount_builder(GOOD), &path)
            .expect_err("the second mount repeats the id");
        assert_eq!(error.code(), "session");
        assert_eq!(error.container(), "rof-good-rof", "the id it collided with");
        assert_eq!(builder.len(), 1, "the refused mount left nothing behind");

        let session = builder.open();
        let asset = session.resolve(&key("INDEX.TXT")).expect("still resolves");
        assert_eq!(asset.resolved().span.length(), INDEX.len() as u64);
        assert_eq!(
            first.read(&key("INDEX.TXT")).expect("reads").data,
            INDEX,
            "the surviving source is untouched"
        );
        let teardown = session.close();
        assert_eq!(teardown.released.len(), 1);
        assert_eq!(teardown.released[0].as_str(), "rof-good-rof");
    }

    /// **Error propagation (export):** an asset stamped by another
    /// session, or resolved from another mount, is refused before a byte
    /// is written — while the right pairing still exports, so the
    /// refusals are about identity and not a broken export path.
    #[test]
    fn accept_f05_c_export_refuses_foreign_sessions_and_mounts() {
        let temp = Temp::new("foreign");
        let good = temp.container(GOOD, &good_container());
        let payload = b"second container payload\n";
        let other = temp.container(OTHER, &single_member_container("OTHER.DAT", payload));
        let export = temp.export();

        let mut builder_a = SessionBuilder::new(context());
        let source_a = mount_rof_into(&mut builder_a, mount_builder(GOOD), &good)
            .expect("the first container mounts");
        let session_a = builder_a.open();
        let foreign_asset = session_a.resolve(&key("INDEX.TXT")).expect("resolved");

        let mut builder_b = SessionBuilder::new(context());
        let source_b = mount_rof_into(&mut builder_b, mount_builder(OTHER), &other)
            .expect("the second container mounts");
        let session_b = builder_b.open();
        let directory = ExportDirectory::open(&export, &session_b).expect("export dir opens");

        let error = export_rof_member(&session_b, &foreign_asset, &source_a, &directory)
            .expect_err("an asset stamped by another session is never exported");
        assert!(
            matches!(error, RofExportError::ForeignSession { .. }),
            "unexpected {error:?}"
        );
        assert!(
            temp.entries(&export).is_empty(),
            "nothing was written, found {:?}",
            temp.entries(&export)
        );

        let own_asset = session_b.resolve(&key("OTHER.DAT")).expect("resolved");
        let error = export_rof_member(&session_b, &own_asset, &source_a, &directory)
            .expect_err("these bytes belong to another mount");
        assert!(
            matches!(error, RofExportError::ForeignMount { .. }),
            "unexpected {error:?}"
        );
        assert!(
            temp.entries(&export).is_empty(),
            "nothing was written, found {:?}",
            temp.entries(&export)
        );

        let file = export_rof_member(&session_b, &own_asset, &source_b, &directory)
            .expect("the matching session and mount export");
        assert_eq!(file.size_bytes, payload.len() as u64);
        assert_eq!(
            fs::read(export.join("OTHER.DAT")).expect("the export is written"),
            payload
        );

        let released_a = session_a.close();
        assert_eq!(released_a.released.len(), 1);
        let released_b = session_b.close();
        assert_eq!(released_b.released.len(), 1);
    }
}
