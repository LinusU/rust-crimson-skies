//! Explicit private export of resolved members (F04-C).
//!
//! Spec F04 non-negotiable behavior 5: "Extracting is an explicit private
//! research command, never an implicit runtime requirement." Nothing in
//! the runtime path calls this module; `cs-inspect resolve --export-dir`
//! does, on request.
//!
//! Spec F04 AC03: "Malicious ZIP/ROF names cannot leave a private export
//! directory." An archive member name is untrusted input, so the name is
//! validated again here — independently of `MountBuilder`, which already
//! refused it once — before it is turned into a host path:
//!
//! * it must be a `RelativePath`: no `..`, absolute spelling (`/x`, `\x`,
//!   `\\server\share`), drive prefix (`C:\x`, `C:x`), empty or `.`
//!   component, or NUL;
//! * no component may contain `:` (a drive-relative or alternate-stream
//!   spelling on Windows), a control character, or end in `.` or a space
//!   (Windows strips those, so `.. ` would become `..`);
//! * no component may be a Windows device name (`CON`, `NUL`, `AUX`,
//!   `PRN`, `COM1`–`COM9`, `LPT1`–`LPT9`, with or without an extension),
//!   which would write to a device instead of a file.
//!
//! The export root must be an existing directory that is not a symbolic
//! link and does not lie inside any mount of the session, so an export
//! never writes into the original installation. Directories below the
//! root are created one component at a time and an existing symbolic link
//! is refused, never followed; the final file is written to a fresh
//! temporary sibling and renamed into place, and an existing target is
//! refused rather than overwritten.
//!
//! The components are checked with `symlink_metadata` before the write;
//! the standard library offers no `openat`, so a link swapped into the
//! export tree *between* that check and the write is a known race window
//! of this research command (recorded in `docs/findings/`). The export
//! directory is owner-controlled and private, not shared with untrusted
//! writers.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_types::evidence::ContentHash;
use cs_types::install::{RelativePath, RelativePathError};

use crate::vfs::session::{ContentSession, SessionAsset};
use crate::vfs::source::ReadError;

/// Serial for temporary file names, so two exports never share one.
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// Windows device names that must never become an export file name.
const DEVICE_NAMES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Why a member name cannot become an export path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnsafeName {
    /// The name is not a safe relative spelling.
    Path(RelativePathError),
    /// A component contains `:`.
    Colon,
    /// A component contains a control character.
    ControlCharacter,
    /// A component ends in `.` or a space.
    TrailingDotOrSpace,
    /// A component is a Windows device name.
    DeviceName,
}

impl fmt::Display for UnsafeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path(error) => write!(f, "{error}"),
            Self::Colon => write!(f, "a component contains `:`"),
            Self::ControlCharacter => write!(f, "a component contains a control character"),
            Self::TrailingDotOrSpace => write!(f, "a component ends in `.` or a space"),
            Self::DeviceName => write!(f, "a component is a Windows device name"),
        }
    }
}

/// Why an export was refused or failed.
#[derive(Debug)]
pub enum ExportError {
    /// The member name could leave the export directory.
    UnsafeName {
        /// The name exactly as the archive or mount spelled it.
        name: String,
        /// Which rule refused it.
        reason: UnsafeName,
    },
    /// The export root is missing, not a directory, or a symbolic link.
    RootUnavailable {
        /// The requested root.
        path: PathBuf,
        /// Why it cannot be used.
        source: io::Error,
    },
    /// The export root lies inside a mounted source; exporting there
    /// would write into the installation.
    RootInsideMount {
        /// The requested root.
        path: PathBuf,
        /// The mount whose host root contains it.
        mount: String,
    },
    /// A path inside the export tree is a symbolic link or not a
    /// directory where one is needed. It is not followed.
    UnsafeExportTree {
        /// The offending path.
        path: PathBuf,
    },
    /// The target file already exists; exports never overwrite.
    TargetExists {
        /// The target.
        path: PathBuf,
    },
    /// The member's bytes could not be read.
    Read(ReadError),
    /// Writing the export failed.
    Io {
        /// The path being written.
        path: PathBuf,
        /// The failure.
        source: io::Error,
    },
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsafeName { name, reason } => {
                write!(f, "member name {name:?} cannot be exported: {reason}")
            }
            Self::RootUnavailable { path, source } => {
                write!(f, "export root {} is unusable: {source}", path.display())
            }
            Self::RootInsideMount { path, mount } => write!(
                f,
                "export root {} lies inside mount {mount}; exports never write into a \
                 mounted source",
                path.display()
            ),
            Self::UnsafeExportTree { path } => write!(
                f,
                "{} is a symbolic link or not a directory; not followed",
                path.display()
            ),
            Self::TargetExists { path } => {
                write!(
                    f,
                    "{} already exists; exports never overwrite",
                    path.display()
                )
            }
            Self::Read(error) => write!(f, "{error}"),
            Self::Io { path, source } => write!(f, "cannot write {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for ExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RootUnavailable { source, .. } | Self::Io { source, .. } => Some(source),
            Self::Read(error) => Some(error),
            _ => None,
        }
    }
}

/// The validated components of `name`, or why it cannot be exported.
///
/// Pure: no IO, so a hostile name is refused before anything touches the
/// filesystem.
pub fn export_components(name: &str) -> Result<Vec<&str>, ExportError> {
    let refuse = |reason| ExportError::UnsafeName {
        name: name.to_owned(),
        reason,
    };
    RelativePath::new(name).map_err(|error| refuse(UnsafeName::Path(error)))?;
    let components: Vec<&str> = name.split(['/', '\\']).collect();
    for component in &components {
        if component.contains(':') {
            return Err(refuse(UnsafeName::Colon));
        }
        if component.chars().any(char::is_control) {
            return Err(refuse(UnsafeName::ControlCharacter));
        }
        if component.ends_with('.') || component.ends_with(' ') {
            return Err(refuse(UnsafeName::TrailingDotOrSpace));
        }
        let stem = component
            .split('.')
            .next()
            .unwrap_or_default()
            .trim_end()
            .to_ascii_lowercase();
        if DEVICE_NAMES.contains(&stem.as_str()) {
            return Err(refuse(UnsafeName::DeviceName));
        }
    }
    Ok(components)
}

/// An owner-selected private directory that exports are written below.
#[derive(Clone, Debug)]
pub struct ExportDirectory {
    /// The canonical root.
    root: PathBuf,
}

/// One exported member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportedFile {
    /// Where the bytes were written.
    pub path: PathBuf,
    /// How many bytes were written.
    pub size_bytes: u64,
    /// The digest of the written bytes.
    pub sha256: ContentHash,
}

impl ExportDirectory {
    /// Opens `root` as the export directory of `session`.
    ///
    /// `root` must exist, be a directory and not a symbolic link, and must
    /// not lie inside the host root of any of the session's mounts.
    pub fn open(root: &Path, session: &ContentSession) -> Result<Self, ExportError> {
        let unavailable = |source| ExportError::RootUnavailable {
            path: root.to_path_buf(),
            source,
        };
        let metadata = fs::symlink_metadata(root).map_err(unavailable)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(unavailable(io::Error::new(
                io::ErrorKind::NotADirectory,
                "the export root must be a directory, not a symbolic link",
            )));
        }
        let canonical = fs::canonicalize(root).map_err(unavailable)?;
        for mount in session.mounts() {
            let Some(host_root) = mount.host_root() else {
                continue;
            };
            let Ok(mount_root) = fs::canonicalize(host_root) else {
                continue;
            };
            if canonical.starts_with(&mount_root) {
                return Err(ExportError::RootInsideMount {
                    path: root.to_path_buf(),
                    mount: mount.id().to_string(),
                });
            }
        }
        Ok(Self { root: canonical })
    }

    /// The canonical export root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Writes `bytes` as `name` below the export root and returns the
    /// written file's path, length and digest.
    pub fn write(&self, name: &str, bytes: &[u8]) -> Result<ExportedFile, ExportError> {
        let components = export_components(name)?;
        let (file_name, directories) = components
            .split_last()
            .expect("a validated relative spelling has at least one component");

        let mut directory = self.root.clone();
        for component in directories {
            directory.push(component);
            match fs::symlink_metadata(&directory) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
                Ok(_) => {
                    return Err(ExportError::UnsafeExportTree { path: directory });
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    fs::create_dir(&directory).map_err(|source| ExportError::Io {
                        path: directory.clone(),
                        source,
                    })?;
                }
                Err(source) => {
                    return Err(ExportError::Io {
                        path: directory,
                        source,
                    });
                }
            }
        }
        // Defense in depth: the directory the file lands in must still be
        // below the root once every link is resolved.
        let resolved = fs::canonicalize(&directory).map_err(|source| ExportError::Io {
            path: directory.clone(),
            source,
        })?;
        if !resolved.starts_with(&self.root) {
            return Err(ExportError::UnsafeExportTree { path: directory });
        }

        let target = resolved.join(file_name);
        match fs::symlink_metadata(&target) {
            Ok(_) => return Err(ExportError::TargetExists { path: target }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(ExportError::Io {
                    path: target,
                    source,
                });
            }
        }

        let temp = resolved.join(format!(
            ".cs-export-{}-{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let io_error = |source| ExportError::Io {
            path: target.clone(),
            source,
        };
        let written = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .and_then(|mut file| {
                file.write_all(bytes)?;
                file.sync_all()
            })
            .and_then(|()| {
                // `rename` would silently replace a target created since
                // the check above; `hard_link` refuses an existing one.
                fs::hard_link(&temp, &target)
            });
        let _ = fs::remove_file(&temp);
        written.map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                ExportError::TargetExists {
                    path: target.clone(),
                }
            } else {
                io_error(error)
            }
        })?;

        Ok(ExportedFile {
            size_bytes: bytes.len() as u64,
            sha256: crate::install::sha256(bytes),
            path: target,
        })
    }
}

/// Reads the member `asset` names through `session` (digest-checked) and
/// exports it below `directory` under the member's own spelling.
pub fn export_asset(
    session: &ContentSession,
    asset: &SessionAsset,
    directory: &ExportDirectory,
) -> Result<ExportedFile, ExportError> {
    let name = asset
        .resolved()
        .span
        .member_key()
        .unwrap_or(asset.resolved().span.container_path());
    // Validate before reading, so a hostile name costs no IO at all.
    export_components(name)?;
    let bytes = session.read_all(asset).map_err(ExportError::Read)?;
    directory.write(name, &bytes)
}
