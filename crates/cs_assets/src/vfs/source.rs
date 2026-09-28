//! Mounted host sources and the read path behind them (F04-B).
//!
//! [`mount_directory`] turns a host directory tree into one immutable
//! [`Mount`]: every regular file below the root becomes a member whose
//! spelling is its path relative to that root, hashed while it is indexed.
//! [`read_member_range`] is the random-read path a resolution's bytes come
//! from.
//!
//! Path validation is spec F04 non-negotiable behavior 1, applied to host
//! names and not only to caller keys:
//!
//! * a member spelling must be a `RelativePath` — no `..`, absolute
//!   spelling, drive prefix, `.`/empty component or NUL. On a Unix host a
//!   file may be *named* `..\..\evil.dds`; that name is refused with the
//!   spelling that was found, and the whole mount fails;
//! * symbolic links are never followed. A link inside the tree is reported
//!   in [`MountedDirectory::rejected`] and never becomes a member, so no
//!   key can resolve through it to a file outside the root. A read checks
//!   again that no component of the member's host path became a link after
//!   mounting;
//! * two host names that fold onto one logical key — `Alert.DDS` next to
//!   `alert.dds` on a case-sensitive host, or `hud\x.dds` next to
//!   `hud/x.dds` — are refused with both spellings, never flattened to the
//!   first one found (non-negotiable behavior 3).
//!
//! Nothing here writes: files are opened read-only, nothing is extracted,
//! and a read returns owned bytes (non-negotiable behavior 5). A read opens
//! the file afresh and closes it before returning, so no file handle
//! outlives the call; the session mount lifecycle is F04-C.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use cs_types::evidence::ContentHash;

use crate::install::Sha256;
use crate::vfs::mount::{MemberRecord, Mount, MountBuilder, MountError};

/// Why an entry below a mounted directory did not become a member.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RejectReason {
    /// A symbolic link. It is never followed: its target could lie outside
    /// the mount root.
    SymbolicLink,
    /// A socket, FIFO, device node or anything else that is neither a
    /// regular file nor a directory.
    NonRegular,
}

impl RejectReason {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::SymbolicLink => "symbolic_link",
            Self::NonRegular => "non_regular",
        }
    }
}

impl fmt::Display for RejectReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One entry that was observed below a mounted directory and deliberately
/// not mounted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedEntry {
    /// The entry's host path relative to the mount root, as observed.
    pub host_relative: PathBuf,
    /// Why it was not mounted.
    pub reason: RejectReason,
}

/// A mounted host directory: the immutable mount plus what the walk
/// refused to mount.
#[derive(Clone, Debug)]
pub struct MountedDirectory {
    /// The mount, ready for `Vfs::mount`.
    pub mount: Mount,
    /// Entries that exist below the root but are not members, in walk
    /// order. Refusal is visible, never silent.
    pub rejected: Vec<RejectedEntry>,
}

/// Why a host directory could not be mounted.
#[derive(Debug)]
pub enum SourceError {
    /// The builder already held declared members; a directory mount holds
    /// exactly the files below its root.
    BuilderHasMembers,
    /// The root is missing, unreadable or not a directory.
    RootUnavailable {
        /// The root that was asked for.
        path: PathBuf,
        /// Why it could not be used.
        source: io::Error,
    },
    /// A directory below the root could not be listed.
    UnreadableDirectory {
        /// The directory.
        path: PathBuf,
        /// Why it could not be listed.
        source: io::Error,
    },
    /// A regular file could not be opened or read while it was hashed.
    UnreadableFile {
        /// The file.
        path: PathBuf,
        /// Why it could not be read.
        source: io::Error,
    },
    /// A file changed while it was hashed, so no coherent digest exists.
    FileChangedDuringMount {
        /// The file.
        path: PathBuf,
    },
    /// A host name is not UTF-8, so it has no spelling a key could match.
    NonUtf8Name {
        /// The entry.
        path: PathBuf,
    },
    /// A member was refused: an escaping spelling, or a second spelling of
    /// a key the mount already holds (both spellings are in the error).
    Member {
        /// The container label of the mount being built.
        container: String,
        /// The refusal.
        error: MountError,
    },
    /// The mount as a whole was refused.
    Mount(MountError),
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BuilderHasMembers => write!(
                f,
                "a directory mount holds exactly the files below its root; the builder \
                 already declared members"
            ),
            Self::RootUnavailable { path, source } => {
                write!(f, "mount root {} is unavailable: {source}", path.display())
            }
            Self::UnreadableDirectory { path, source } => {
                write!(f, "cannot list directory {}: {source}", path.display())
            }
            Self::UnreadableFile { path, source } => {
                write!(f, "cannot read file {}: {source}", path.display())
            }
            Self::FileChangedDuringMount { path } => write!(
                f,
                "{} changed while it was being mounted; refusing the torn digest",
                path.display()
            ),
            Self::NonUtf8Name { path } => {
                write!(f, "host name {} is not UTF-8", path.display())
            }
            Self::Member { container, error } => write!(f, "in {container}: {error}"),
            Self::Mount(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SourceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RootUnavailable { source, .. }
            | Self::UnreadableDirectory { source, .. }
            | Self::UnreadableFile { source, .. } => Some(source),
            Self::Member { error, .. } | Self::Mount(error) => Some(error),
            Self::BuilderHasMembers
            | Self::FileChangedDuringMount { .. }
            | Self::NonUtf8Name { .. } => None,
        }
    }
}

/// Mounts the host directory `root` with the id, namespace, precedence,
/// container label and scope already declared on `builder`.
///
/// The container label is what every [`SourceSpan`](cs_types::asset_id::SourceSpan)
/// of this mount records (for example the installation-relative spelling
/// of the directory); the host root itself never enters a span.
///
/// The walk is read-only and deterministic (entries in sorted byte order),
/// never follows a symbolic link, and fails by name on anything it cannot
/// read or index rather than dropping it.
pub fn mount_directory(
    mut builder: MountBuilder,
    root: &Path,
) -> Result<MountedDirectory, SourceError> {
    if builder.has_members() {
        return Err(SourceError::BuilderHasMembers);
    }
    let metadata = fs::symlink_metadata(root).map_err(|source| SourceError::RootUnavailable {
        path: root.to_path_buf(),
        source,
    })?;
    if !metadata.is_dir() {
        return Err(SourceError::RootUnavailable {
            path: root.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::NotADirectory,
                "mount root is not a directory (a symbolic link is not followed)",
            ),
        });
    }

    let mut walk = Walk::default();
    walk_directory(root, Path::new(""), "", &mut walk)?;

    let container = builder.container().to_owned();
    for (spelling, host_relative) in walk.files {
        let host_path = root.join(&host_relative);
        let (size_bytes, digest) = hash_file(&host_path)?;
        builder
            .add_host_file(&spelling, host_relative, size_bytes, digest)
            .map_err(|error| SourceError::Member {
                container: container.clone(),
                error,
            })?;
    }
    builder.set_directory_backing(root.to_path_buf());
    let mount = builder.build().map_err(SourceError::Mount)?;
    Ok(MountedDirectory {
        mount,
        rejected: walk.rejected,
    })
}

/// What one directory walk collected.
#[derive(Debug, Default)]
struct Walk {
    /// `(spelling, host path relative to the root)` of every regular file.
    files: Vec<(String, PathBuf)>,
    /// Entries that were observed and not mounted.
    rejected: Vec<RejectedEntry>,
}

/// Recursively collects the regular files below `directory` without
/// following links.
fn walk_directory(
    directory: &Path,
    host_relative: &Path,
    spelling: &str,
    walk: &mut Walk,
) -> Result<(), SourceError> {
    let entries = fs::read_dir(directory).map_err(|source| SourceError::UnreadableDirectory {
        path: directory.to_path_buf(),
        source,
    })?;
    let mut collected: Vec<(OsString, fs::FileType)> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| SourceError::UnreadableDirectory {
            path: directory.to_path_buf(),
            source,
        })?;
        let file_type = entry
            .file_type()
            .map_err(|source| SourceError::UnreadableDirectory {
                path: entry.path(),
                source,
            })?;
        collected.push((entry.file_name(), file_type));
    }
    collected.sort_by(|left, right| left.0.as_encoded_bytes().cmp(right.0.as_encoded_bytes()));

    for (name, file_type) in collected {
        let child_host = host_relative.join(&name);
        let component = name.to_str().ok_or_else(|| SourceError::NonUtf8Name {
            path: directory.join(&name),
        })?;
        let child_spelling = if spelling.is_empty() {
            component.to_owned()
        } else {
            format!("{spelling}/{component}")
        };
        if file_type.is_symlink() {
            walk.rejected.push(RejectedEntry {
                host_relative: child_host,
                reason: RejectReason::SymbolicLink,
            });
        } else if file_type.is_dir() {
            walk_directory(&directory.join(&name), &child_host, &child_spelling, walk)?;
        } else if file_type.is_file() {
            walk.files.push((child_spelling, child_host));
        } else {
            walk.rejected.push(RejectedEntry {
                host_relative: child_host,
                reason: RejectReason::NonRegular,
            });
        }
    }
    Ok(())
}

/// Hashes one regular file, returning the hashed byte count and digest,
/// and refuses a file whose length or modification time moved during the
/// read.
fn hash_file(path: &Path) -> Result<(u64, ContentHash), SourceError> {
    let unreadable = |source| SourceError::UnreadableFile {
        path: path.to_path_buf(),
        source,
    };
    let mut file = fs::File::open(path).map_err(unreadable)?;
    let before = file.metadata().map_err(unreadable)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let read = file.read(&mut buffer).map_err(unreadable)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
    let after = file.metadata().map_err(unreadable)?;
    let before_modified = before.modified().map_err(unreadable)?;
    let after_modified = after.modified().map_err(unreadable)?;
    if before.len() != total || after.len() != total || before_modified != after_modified {
        return Err(SourceError::FileChangedDuringMount {
            path: path.to_path_buf(),
        });
    }
    Ok((total, hasher.finalize()))
}

/// Why bytes could not be read for a resolution.
#[derive(Debug)]
pub enum ReadError {
    /// The resolution names a mount this VFS does not hold.
    UnknownMount {
        /// The mount id the resolution named.
        mount: String,
    },
    /// The resolution no longer matches the mount it names: the member is
    /// gone or its container, range or digest differ. A stale answer is
    /// refused rather than read from whatever now sits there.
    StaleResolution {
        /// The mount id the resolution named.
        mount: String,
    },
    /// The mount has no host bytes behind it (declared members only).
    NoBacking {
        /// The mount id.
        mount: String,
    },
    /// The requested range leaves the member.
    OutOfRange {
        /// First requested byte, relative to the member.
        start: u64,
        /// Requested length.
        length: u64,
        /// The member's length.
        member_length: u64,
    },
    /// A component of the member's host path is now a symbolic link, or
    /// the member is no longer a regular file. It is not followed.
    NotARegularFile {
        /// The host path.
        path: PathBuf,
    },
    /// The member's length on disk differs from the mounted length.
    ChangedOnDisk {
        /// The host path.
        path: PathBuf,
        /// The length recorded when it was mounted.
        mounted_length: u64,
        /// The length found now.
        found_length: u64,
    },
    /// A whole-member read produced bytes whose digest differs from the
    /// mounted digest.
    DigestMismatch {
        /// The host path.
        path: PathBuf,
        /// The digest recorded when it was mounted.
        mounted: ContentHash,
        /// The digest of the bytes read now.
        found: ContentHash,
    },
    /// The read itself failed.
    Io {
        /// The host path.
        path: PathBuf,
        /// The failure.
        source: io::Error,
    },
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownMount { mount } => write!(f, "mount {mount} is not mounted"),
            Self::StaleResolution { mount } => write!(
                f,
                "the resolution no longer matches mount {mount}; resolve the key again"
            ),
            Self::NoBacking { mount } => write!(f, "mount {mount} has no host bytes to read"),
            Self::OutOfRange {
                start,
                length,
                member_length,
            } => write!(
                f,
                "range {start}+{length} leaves the {member_length}-byte member"
            ),
            Self::NotARegularFile { path } => write!(
                f,
                "{} is no longer a regular file reached without symbolic links; not followed",
                path.display()
            ),
            Self::ChangedOnDisk {
                path,
                mounted_length,
                found_length,
            } => write!(
                f,
                "{} was {mounted_length} bytes when mounted and is {found_length} bytes now",
                path.display()
            ),
            Self::DigestMismatch {
                path,
                mounted,
                found,
            } => write!(
                f,
                "{} hashed {} when mounted and {} now",
                path.display(),
                mounted.to_hex(),
                found.to_hex()
            ),
            Self::Io { path, source } => write!(f, "cannot read {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for ReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Reads `length` bytes starting `start` bytes into `member` of `mount`.
///
/// The range is checked against the member before anything is opened.
/// The mount root and every component of the host path below it are
/// checked not to be symbolic links, the file is opened read-only, and on
/// Unix the opened file is checked to be the same inode the checked path names, so a link
/// swapped in between the check and the open is refused too. The file's
/// length must still equal the mounted length.
pub(crate) fn read_member_range(
    mount: &Mount,
    member: &MemberRecord,
    start: u64,
    length: u64,
) -> Result<Vec<u8>, ReadError> {
    let (Some(root), Some(host_relative)) = (mount.host_root(), member.host_relative()) else {
        return Err(ReadError::NoBacking {
            mount: mount.id().to_string(),
        });
    };
    let out_of_range = ReadError::OutOfRange {
        start,
        length,
        member_length: member.size_bytes(),
    };
    let Some(end) = start.checked_add(length) else {
        return Err(out_of_range);
    };
    if end > member.size_bytes() {
        return Err(out_of_range);
    }
    let Ok(buffer_length) = usize::try_from(length) else {
        return Err(out_of_range);
    };

    let path = root.join(host_relative);
    let io_error = |source| ReadError::Io {
        path: path.clone(),
        source,
    };
    // The root was refused as a link when it was mounted; one swapped in
    // since is refused too, so no read reaches through it.
    let root_metadata = fs::symlink_metadata(root).map_err(io_error)?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(ReadError::NotARegularFile { path: path.clone() });
    }
    let mut checked = root.to_path_buf();
    let mut last = None;
    for component in host_relative.components() {
        checked.push(component);
        let metadata = fs::symlink_metadata(&checked).map_err(io_error)?;
        if metadata.file_type().is_symlink() {
            return Err(ReadError::NotARegularFile { path: path.clone() });
        }
        last = Some(metadata);
    }
    let Some(checked_metadata) = last.filter(fs::Metadata::is_file) else {
        return Err(ReadError::NotARegularFile { path: path.clone() });
    };

    let mut file = fs::File::open(&path).map_err(io_error)?;
    let opened = file.metadata().map_err(io_error)?;
    if !opened.is_file() || !same_file(&checked_metadata, &opened) {
        return Err(ReadError::NotARegularFile { path: path.clone() });
    }
    if opened.len() != member.size_bytes() {
        return Err(ReadError::ChangedOnDisk {
            path: path.clone(),
            mounted_length: member.size_bytes(),
            found_length: opened.len(),
        });
    }
    let absolute = member
        .offset()
        .checked_add(start)
        .expect("MemberRecord validated offset + size, and start <= size");
    file.seek(SeekFrom::Start(absolute)).map_err(io_error)?;
    let mut bytes = vec![0u8; buffer_length];
    file.read_exact(&mut bytes).map_err(io_error)?;
    Ok(bytes)
}

/// Whether two metadata records describe the same file.
#[cfg(unix)]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

/// Whether two metadata records describe the same file. Without inode
/// numbers the component walk above is the only check.
#[cfg(not(unix))]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.len() == right.len()
}
