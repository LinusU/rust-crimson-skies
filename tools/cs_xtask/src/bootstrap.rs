//! Platform bootstrap gate (F00-D): required workspace members and frozen
//! toolchain.
//!
//! The F00 deliverable is a workspace with exactly ten members (`cs_types`,
//! `cs_formats`, `cs_assets`, `cs_content`, `cs_sim`, `cs_script`, `cs_net`,
//! `cs_app`, `cs_inspect`, `cs_xtask`). Losing one from `[workspace]
//! members` is silent in real cargo: `cargo metadata --no-deps --locked`
//! still exits 0 and `cargo test --workspace --locked` simply stops covering
//! the dropped crate, so CI stays green while coverage shrinks (measured in
//! `docs/findings/2026-09-23-f00-d-platform-bootstrap-evidence-and-toolchain-freeze.md`).
//! Members that are path dependencies of another member are even re-added
//! implicitly, so the `members` list itself can lie without any gate
//! noticing.
//!
//! This module is the gate that does notice: [`verify_workspace`] requires
//! every [`REQUIRED_MEMBERS`] entry to be listed explicitly (globs do not
//! count — an auditable list is the point), requires each listed member to
//! ship a `[package]` manifest, and then composes the two existing guards so
//! one command freezes the whole bootstrap:
//!
//! * [`crate::pins`] — `Cargo.lock` still pins the Bevy 0.19 / Avian3d 0.7
//!   pair and `rust-toolchain.toml` pins an exact patch at or above the
//!   declared MSRV (F00-B),
//! * [`crate::ci`] — the owner-maintained workflow still runs fmt, clippy
//!   with `-D warnings` and the workspace test suite (F00-C).
//!
//! Members beyond [`REQUIRED_MEMBERS`] are kept in [`BootstrapReport`] but
//! are not rejected: a later spec stage may add a crate, while *losing* one
//! of the ten is always a regression of the F00 deliverable.
//!
//! Every rejection names the member, file or key that failed; a green result
//! means the workspace shape, the pins and the CI gates were all actually
//! read from disk.

use std::fmt;
use std::fs;
use std::path::Path;

use crate::ci::{self, CiError};
use crate::pins::{INTENDED_BASELINE, PinError, Pins};

/// The members `specs/F00-workspace-toolchain-and-first-executable.md`
/// requires the workspace to have, in manifest order.
pub const REQUIRED_MEMBERS: [&str; 10] = [
    "crates/cs_app",
    "crates/cs_assets",
    "crates/cs_content",
    "crates/cs_formats",
    "crates/cs_net",
    "crates/cs_script",
    "crates/cs_sim",
    "crates/cs_types",
    "tools/cs_inspect",
    "tools/cs_xtask",
];

/// What a passing [`verify_workspace`] actually observed on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BootstrapReport {
    /// The `[workspace] members` entries, in the order the manifest lists
    /// them. A passing report contains every [`REQUIRED_MEMBERS`] entry.
    pub members: Vec<String>,
    /// The pins as read from `Cargo.lock`, `Cargo.toml` and
    /// `rust-toolchain.toml`, verified against [`INTENDED_BASELINE`].
    pub pins: Pins,
}

/// Why the workspace bootstrap cannot be trusted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BootstrapError {
    /// The workspace `Cargo.toml` could not be read.
    Io { path: String },
    /// `Cargo.toml` has no `[workspace] members = [...]` array.
    MissingMembers { file: String },
    /// A required member is not listed in `[workspace] members`.
    MissingMember { member: String },
    /// A listed member has no `Cargo.toml` at its path.
    MissingManifest { member: String, path: String },
    /// A member's manifest is not a package manifest.
    NotAPackage { member: String, path: String },
    /// A frozen pin drifted (or its file is gone): see [`PinError`].
    Pin(PinError),
    /// A CI gate disappeared from the owner-maintained workflow.
    Ci(CiError),
}

impl fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path } => write!(f, "cannot read {path}"),
            Self::MissingMembers { file } => {
                write!(f, "{file} has no [workspace] members = [...] array")
            }
            Self::MissingMember { member } => write!(
                f,
                "required workspace member {member:?} is not listed in [workspace] members"
            ),
            Self::MissingManifest { member, path } => {
                write!(f, "workspace member {member:?} has no manifest at {path}")
            }
            Self::NotAPackage { member, path } => write!(
                f,
                "workspace member {member:?} manifest {path} has no [package] table"
            ),
            Self::Pin(error) => write!(f, "frozen pin check failed: {error}"),
            Self::Ci(error) => write!(f, "CI workflow check failed: {error}"),
        }
    }
}

impl std::error::Error for BootstrapError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Pin(error) => Some(error),
            Self::Ci(error) => Some(error),
            _ => None,
        }
    }
}

/// Reads `[workspace] members` from `manifest`.
///
/// Both layouts cargo accepts are handled: the multi-line array this
/// workspace uses and an inline `members = ["a", "b"]`. Globs such as
/// `crates/*` are returned verbatim, so a manifest that replaced the
/// explicit list with a glob fails [`verify_workspace`] like any other
/// missing member.
pub fn workspace_members(manifest: &str) -> Result<Vec<String>, BootstrapError> {
    let lines: Vec<&str> = manifest.lines().collect();
    let mut index = lines
        .iter()
        .position(|line| line.trim() == "[workspace]")
        .ok_or_else(|| BootstrapError::MissingMembers {
            file: "Cargo.toml".to_string(),
        })?;
    index += 1;

    let mut array = String::new();
    let mut found = false;
    for line in &lines[index..] {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            // Next table: the members array has to live in [workspace].
            break;
        }
        if !found {
            let Some(rest) = trimmed.strip_prefix("members") else {
                continue;
            };
            let Some(rest) = rest.trim_start().strip_prefix('=') else {
                continue;
            };
            found = true;
            array.push_str(rest);
        } else {
            array.push('\n');
            array.push_str(trimmed);
        }
        if array_depth(&array) == 0 && found {
            break;
        }
    }

    if !found {
        return Err(BootstrapError::MissingMembers {
            file: "Cargo.toml".to_string(),
        });
    }
    Ok(quoted_strings(&array))
}

/// Verifies the whole platform bootstrap of the workspace at
/// `workspace_root`:
///
/// 1. every [`REQUIRED_MEMBERS`] entry is listed in `[workspace] members`,
/// 2. every listed required member has a `Cargo.toml` with a `[package]`
///    table,
/// 3. [`crate::pins`] verifies the frozen Bevy/Avian/toolchain pins,
/// 4. [`crate::ci`] verifies the owner-maintained workflow's gates.
///
/// The first failure is returned; nothing is skipped, so `Ok` means all four
/// checks really ran against the files on disk.
pub fn verify_workspace(workspace_root: &Path) -> Result<BootstrapReport, BootstrapError> {
    let manifest_path = workspace_root.join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path).map_err(|_| BootstrapError::Io {
        path: manifest_path.display().to_string(),
    })?;
    let members = workspace_members(&manifest)?;

    for required in REQUIRED_MEMBERS {
        if !members.iter().any(|member| member == required) {
            return Err(BootstrapError::MissingMember {
                member: required.to_string(),
            });
        }
    }

    for required in REQUIRED_MEMBERS {
        let member_manifest = workspace_root.join(required).join("Cargo.toml");
        let path = member_manifest.display().to_string();
        if !member_manifest.is_file() {
            return Err(BootstrapError::MissingManifest {
                member: required.to_string(),
                path,
            });
        }
        let text = fs::read_to_string(&member_manifest)
            .map_err(|_| BootstrapError::Io { path: path.clone() })?;
        if !text.lines().any(|line| line.trim() == "[package]") {
            return Err(BootstrapError::NotAPackage {
                member: required.to_string(),
                path,
            });
        }
    }

    let pins = Pins::read(workspace_root).map_err(BootstrapError::Pin)?;
    pins.verify(&INTENDED_BASELINE)
        .map_err(BootstrapError::Pin)?;

    ci::verify_workspace_workflow(workspace_root).map_err(BootstrapError::Ci)?;

    Ok(BootstrapReport { members, pins })
}

/// Nesting depth of `[` / `]` brackets in `text` (strings in this workspace
/// never contain brackets, so no quote tracking is needed).
fn array_depth(text: &str) -> i32 {
    text.chars()
        .map(|character| match character {
            '[' => 1,
            ']' => -1,
            _ => 0,
        })
        .sum()
}

/// Every `"…"` payload in `text`, in order.
fn quoted_strings(text: &str) -> Vec<String> {
    let mut strings = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('"') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('"') else {
            break;
        };
        strings.push(rest[..end].to_string());
        rest = &rest[end + 1..];
    }
    strings
}
