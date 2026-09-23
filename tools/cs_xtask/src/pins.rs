//! Guards for the pinned dependency baseline and Rust toolchain.
//!
//! The intended baseline is Bevy 0.19 + Avian3d 0.7, matching the inspected
//! MM2 repository (`specs/F00-workspace-toolchain-and-first-executable.md`,
//! references S01/S11). `Cargo.toml` uses caret ranges that cannot cross a
//! minor boundary; the exact patches are pinned by the committed
//! `Cargo.lock`, and the toolchain is pinned by `rust-toolchain.toml`. This
//! module reads those three files and reports a drift instead of letting a
//! later `cargo update` silently change the pair.

use std::fmt;
use std::fs;
use std::path::Path;

/// The baseline the workspace promises to hold (major.minor of the pair, the
/// declared MSRV of the workspace).
pub const INTENDED_BASELINE: Baseline = Baseline {
    bevy: "0.19",
    avian3d: "0.7",
    rust_version: "1.98",
};

/// Intended dependency/toolchain series.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Baseline {
    pub bevy: &'static str,
    pub avian3d: &'static str,
    pub rust_version: &'static str,
}

/// What the workspace actually pins right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pins {
    /// Exact `bevy` version resolved into `Cargo.lock`.
    pub bevy: String,
    /// Exact `avian3d` version resolved into `Cargo.lock`.
    pub avian3d: String,
    /// `workspace.package.rust-version` from `Cargo.toml`.
    pub rust_version: String,
    /// `toolchain.channel` from `rust-toolchain.toml`.
    pub toolchain_channel: String,
}

/// Why the pins cannot be trusted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PinError {
    /// A required file could not be read.
    Io { path: String },
    /// A required package is absent from `Cargo.lock`.
    MissingPackage { name: String },
    /// A required key is absent from a manifest.
    MissingKey {
        file: &'static str,
        key: &'static str,
    },
    /// A pinned value left its intended series.
    Mismatch {
        what: String,
        expected: String,
        found: String,
    },
    /// The toolchain channel names a rolling channel or a bare major.minor
    /// instead of an exact `major.minor.patch`.
    UnpinnedToolchain { channel: String },
}

impl fmt::Display for PinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path } => write!(f, "cannot read {path}"),
            Self::MissingPackage { name } => {
                write!(f, "package {name} is missing from Cargo.lock")
            }
            Self::MissingKey { file, key } => write!(f, "{file} has no {key} key"),
            Self::Mismatch {
                what,
                expected,
                found,
            } => write!(f, "{what} must be {expected}, found {found}"),
            Self::UnpinnedToolchain { channel } => write!(
                f,
                "rust-toolchain channel {channel:?} must be an exact major.minor.patch pin"
            ),
        }
    }
}

impl std::error::Error for PinError {}

impl Pins {
    /// Reads the pins from a workspace root (`Cargo.lock`, `Cargo.toml`,
    /// `rust-toolchain.toml`).
    pub fn read(workspace_root: &Path) -> Result<Self, PinError> {
        let lockfile = read_file(&workspace_root.join("Cargo.lock"))?;
        let cargo_toml = read_file(&workspace_root.join("Cargo.toml"))?;
        let toolchain_toml = read_file(&workspace_root.join("rust-toolchain.toml"))?;

        Ok(Self {
            bevy: lockfile_package_version(&lockfile, "bevy")?,
            avian3d: lockfile_package_version(&lockfile, "avian3d")?,
            rust_version: workspace_rust_version(&cargo_toml)?,
            toolchain_channel: toolchain_channel(&toolchain_toml)?,
        })
    }

    /// Checks the observed pins against a [`Baseline`].
    ///
    /// Every rejection names the key and both values, so a drifted pin is
    /// reported instead of quietly accepted.
    pub fn verify(&self, baseline: &Baseline) -> Result<(), PinError> {
        require_series("bevy", baseline.bevy, &self.bevy)?;
        require_series("avian3d", baseline.avian3d, &self.avian3d)?;
        require_series(
            "workspace package rust-version",
            baseline.rust_version,
            &self.rust_version,
        )?;

        if !is_exact_patch(&self.toolchain_channel) {
            return Err(PinError::UnpinnedToolchain {
                channel: self.toolchain_channel.clone(),
            });
        }
        if !satisfies(&self.toolchain_channel, &self.rust_version) {
            return Err(PinError::Mismatch {
                what: "rust-toolchain channel".to_string(),
                expected: format!("at least {}", self.rust_version),
                found: self.toolchain_channel.clone(),
            });
        }
        Ok(())
    }
}

fn read_file(path: &Path) -> Result<String, PinError> {
    fs::read_to_string(path).map_err(|_| PinError::Io {
        path: path.display().to_string(),
    })
}

/// The version of one `[[package]]` block in a `Cargo.lock` file.
pub fn lockfile_package_version(lockfile: &str, package: &str) -> Result<String, PinError> {
    let mut name: Option<&str> = None;
    let mut version: Option<&str> = None;

    let finish = |name: Option<&str>, version: Option<&str>| match (name, version) {
        (Some(n), Some(v)) if n == package => Some(v.to_string()),
        _ => None,
    };

    for line in lockfile.lines().map(str::trim) {
        if line == "[[package]]" {
            if let Some(found) = finish(name, version) {
                return Ok(found);
            }
            name = None;
            version = None;
        } else if let Some(value) = quoted(line, "name") {
            name = Some(value);
        } else if let Some(value) = quoted(line, "version") {
            version = Some(value);
        }
    }

    finish(name, version).ok_or(PinError::MissingPackage {
        name: package.to_string(),
    })
}

/// `workspace.package.rust-version` from a workspace `Cargo.toml`.
pub fn workspace_rust_version(cargo_toml: &str) -> Result<String, PinError> {
    let mut in_workspace_package = false;

    for line in cargo_toml.lines().map(str::trim) {
        if line.starts_with('[') {
            in_workspace_package = line == "[workspace.package]";
        } else if in_workspace_package && let Some(value) = quoted(line, "rust-version") {
            return Ok(value.to_string());
        }
    }

    Err(PinError::MissingKey {
        file: "Cargo.toml [workspace.package]",
        key: "rust-version",
    })
}

/// `toolchain.channel` from `rust-toolchain.toml`.
pub fn toolchain_channel(toolchain_toml: &str) -> Result<String, PinError> {
    let mut in_toolchain = false;

    for line in toolchain_toml.lines().map(str::trim) {
        if line.starts_with('[') {
            in_toolchain = line == "[toolchain]";
        } else if in_toolchain && let Some(value) = quoted(line, "channel") {
            return Ok(value.to_string());
        }
    }

    Err(PinError::MissingKey {
        file: "rust-toolchain.toml [toolchain]",
        key: "channel",
    })
}

/// `key = "value"` for a single line, if it matches.
fn quoted<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (found, value) = line.split_once('=')?;
    if found.trim() != key {
        return None;
    }
    let value = value.trim();
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
}

/// `found` is in the `series` major.minor series.
fn require_series(what: &str, series: &str, found: &str) -> Result<(), PinError> {
    if found == series || found.starts_with(&format!("{series}.")) {
        Ok(())
    } else {
        Err(PinError::Mismatch {
            what: what.to_string(),
            expected: format!("{series}.x"),
            found: found.to_string(),
        })
    }
}

/// Exact `major.minor.patch`, not a rolling channel such as `stable` or a
/// bare `1.98`.
fn is_exact_patch(channel: &str) -> bool {
    let mut parts = channel.split('.');
    let numeric = |part: Option<&str>| {
        part.map(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())) == Some(true)
    };
    numeric(parts.next())
        && numeric(parts.next())
        && numeric(parts.next())
        && parts.next().is_none()
}

/// `channel` is at least the `series` version (the toolchain satisfies the
/// workspace's declared MSRV).
fn satisfies(channel: &str, series: &str) -> bool {
    match (version_tuple(channel), version_tuple(series)) {
        (Some(channel), Some(msrv)) => {
            let width = channel.len().max(msrv.len());
            let mut channel = channel;
            let mut msrv = msrv;
            channel.resize(width, 0);
            msrv.resize(width, 0);
            channel >= msrv
        }
        _ => false,
    }
}

/// Numeric `major[.minor[.patch]]` components of a version-ish string.
fn version_tuple(value: &str) -> Option<Vec<u32>> {
    if value.is_empty() {
        return None;
    }
    value
        .split('.')
        .map(|part| {
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                None
            } else {
                part.parse().ok()
            }
        })
        .collect()
}
