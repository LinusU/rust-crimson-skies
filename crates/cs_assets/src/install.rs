//! The typed installation-inventory path (F02-A).
//!
//! [`inventory`] turns discovered host paths plus their per-file facts into
//! the validated typed output, an [`InstallManifest`]: it derives each
//! relative spelling by stripping the host root (matching components
//! case-insensitively, so a root recorded with different letter case still
//! matches — spec F02 non-negotiable behavior 2), preserves the original
//! spelling, and refuses anything that is not under the root instead of
//! silently omitting it.
//!
//! Walking a real installation, reading bytes and computing the SHA-256
//! values carried by [`DiscoveredFile`] are F02-B work; this module is the
//! schema-level path between discovery input and manifest output. Nothing
//! here is derived from original game data.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use cs_types::evidence::ContentHash;
use cs_types::install::{
    FileFamily, FileRole, InstallFileRecord, InstallManifest, ManifestError, ParseState,
    RelativePath,
};

/// Typed input: one regular source file discovered under a host root.
///
/// Discovery supplies the host path as found on this machine plus the facts
/// it measured (size, SHA-256) and its current analysis result
/// (family/role/parse state; all of them may be the explicit unknowns).
/// The host path is input only: [`inventory`] reduces it to a relative
/// spelling, and it never reaches the manifest's logical identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredFile {
    /// The path as discovered on this host (root-joined).
    pub host_path: PathBuf,
    /// The measured size in bytes.
    pub size_bytes: u64,
    /// The measured SHA-256 of the file's bytes.
    pub sha256: ContentHash,
    /// The detected family, `None` when undetected.
    pub family: Option<FileFamily>,
    /// The classification recorded so far.
    pub role: FileRole,
    /// The parse state recorded so far.
    pub parse_state: ParseState,
}

/// Builds the typed output: a validated [`InstallManifest`] over `host_root`.
///
/// Every discovered file either becomes a manifest row or the whole
/// inventory fails with a named error — no entry is ever dropped, because
/// the inventory must contain every regular source file (spec F02
/// non-negotiable behavior 4). Two files that differ only in letter case
/// collide on one logical key and are rejected as ambiguous rather than
/// silently merged.
pub fn inventory(
    host_root: &Path,
    discovered: Vec<DiscoveredFile>,
) -> Result<InstallManifest, ManifestError> {
    if host_root.as_os_str().is_empty() {
        return Err(ManifestError::EmptyRoot);
    }
    let mut files = Vec::with_capacity(discovered.len());
    for file in discovered {
        let relative_spelling = relative_spelling(host_root, &file.host_path)?;
        files.push(InstallFileRecord {
            relative_spelling,
            size_bytes: file.size_bytes,
            sha256: file.sha256,
            family: file.family,
            role: file.role,
            parse_state: file.parse_state,
        });
    }
    InstallManifest::new(host_root.to_path_buf(), files)
}

/// Strips `host_root` from `host_path` into a validated relative spelling.
///
/// Root components are matched case-insensitively (ASCII), which is what
/// lets a `CS_GAME_DIR` whose letter case disagrees with the discovered
/// paths still inventory the same data under one logical identity. The
/// remainder keeps each component's original case and is joined with `/`,
/// the portable spelling of a relative path.
fn relative_spelling(host_root: &Path, host_path: &Path) -> Result<RelativePath, ManifestError> {
    let root: Vec<&OsStr> = host_root
        .components()
        .filter(|component| !matches!(component, Component::CurDir))
        .map(|component| component.as_os_str())
        .collect();
    let path: Vec<&OsStr> = host_path
        .components()
        .filter(|component| !matches!(component, Component::CurDir))
        .map(|component| component.as_os_str())
        .collect();

    if path.len() < root.len()
        || !path.iter().zip(root.iter()).all(|(part, prefix)| {
            part.as_encoded_bytes()
                .eq_ignore_ascii_case(prefix.as_encoded_bytes())
        })
    {
        return Err(ManifestError::NotUnderRoot {
            path: host_path.to_path_buf(),
        });
    }
    if path.len() == root.len() {
        return Err(ManifestError::EmptyRelative {
            path: host_path.to_path_buf(),
        });
    }

    let mut components = Vec::with_capacity(path.len() - root.len());
    for component in &path[root.len()..] {
        let text = component
            .to_str()
            .ok_or_else(|| ManifestError::NonUtf8Path {
                path: host_path.to_path_buf(),
            })?;
        components.push(text);
    }
    RelativePath::new(&components.join("/")).map_err(|error| ManifestError::RelativePath {
        path: host_path.to_path_buf(),
        error,
    })
}
