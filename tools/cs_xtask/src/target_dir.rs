//! Per-worktree `CARGO_TARGET_DIR` guard (task #383).
//!
//! Cargo keys build artifacts by package id and metadata fingerprint, not
//! by source path, so two checkouts of this workspace that share one
//! `CARGO_TARGET_DIR` write identically named artifacts — and Cargo happily
//! reuses the foreign one. Reproduced for this task with two `probe`
//! packages: after `wt-a` built, a `cargo run` in `wt-b` printed `wt-a`'s
//! marker without compiling anything. A green or red `cargo test` from such
//! an environment is not evidence about the tree it ran in.
//!
//! Cargo's own resolution is the oracle: `cargo metadata --no-deps` reports
//! `target_directory` after applying `CARGO_TARGET_DIR`, every
//! `.cargo/config.toml` on the way and the worktree-local default, so this
//! module never re-implements precedence rules. [`is_per_worktree`] then
//! applies the contract: an effective target directory is private to a
//! worktree when it sits inside the workspace root, or when one of its path
//! components *is* the workspace directory's own name — the
//! `<shared-root>/<worktree>` layout the recommended per-agent environment
//! derives. Anything else is a directory every worktree can compute
//! identically, which is exactly the reported defect.
//!
//! [`verify_workspace`] is the gate: the `accept_t383_` tests run it
//! against this very checkout as part of `cargo test --workspace`, so an
//! agent whose environment still exports a shared directory gets a loud
//! failure naming the fix instead of silently trusting foreign binaries.

use std::env;
use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Why the effective target directory cannot be trusted for this worktree.
#[derive(Debug)]
pub enum TargetDirError {
    /// Cargo could not be started at all.
    Launch { program: String, source: io::Error },
    /// `cargo metadata` failed before reporting a target directory.
    Metadata { status: String, tail: String },
    /// The metadata output carried no usable `target_directory`.
    MetadataShape { tail: String },
    /// The effective directory is not private to this worktree.
    Shared {
        workspace_root: PathBuf,
        target_dir: PathBuf,
    },
}

impl fmt::Display for TargetDirError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Launch { program, source } => {
                write!(f, "cannot run {program}: {source}")
            }
            Self::Metadata { status, tail } => {
                write!(f, "cargo metadata failed ({status}); last output:\n{tail}")
            }
            Self::MetadataShape { tail } => write!(
                f,
                "cargo metadata reported no usable target_directory; output tail:\n{tail}"
            ),
            Self::Shared {
                workspace_root,
                target_dir,
            } => write!(
                f,
                "the effective cargo target directory {} is not private to \
this worktree {}: a CARGO_TARGET_DIR shared between checkouts lets \
concurrent builds reuse each other's artifacts, so `cargo test` can run \
another tree's binary while reporting Finished (task #383). Unset \
CARGO_TARGET_DIR, or point it at a directory that names this worktree, \
e.g. export CARGO_TARGET_DIR=\"$PWD/target\".",
                target_dir.display(),
                workspace_root.display()
            ),
        }
    }
}

impl std::error::Error for TargetDirError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Launch { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// How the spawned `cargo metadata` sees `CARGO_TARGET_DIR`.
enum EnvMode {
    /// Whatever the calling process exports.
    Inherit,
    /// Removed, so Cargo applies config files and its default.
    Unset,
    /// Forced to one value.
    Set(std::ffi::OsString),
}

/// The directory cargo in `workspace_root` will really write artifacts to.
///
/// `cargo metadata --no-deps` answers after applying `CARGO_TARGET_DIR`,
/// every applicable `.cargo/config.toml` and the default
/// `<workspace root>/target`, so the gate observes Cargo's own verdict
/// instead of re-implementing its precedence rules.
pub fn effective_target_dir(workspace_root: &Path) -> Result<PathBuf, TargetDirError> {
    metadata_target_dir(workspace_root, EnvMode::Inherit)
}

/// [`effective_target_dir`] with `CARGO_TARGET_DIR` forced in the spawned
/// cargo: `Some` sets it to that value, `None` removes it entirely. This is
/// the seam the acceptance tests use to prove the reported directory really
/// tracks the environment rather than a guess.
pub fn effective_target_dir_with_env(
    workspace_root: &Path,
    cargo_target_dir: Option<&OsStr>,
) -> Result<PathBuf, TargetDirError> {
    let mode = match cargo_target_dir {
        Some(value) => EnvMode::Set(value.to_os_string()),
        None => EnvMode::Unset,
    };
    metadata_target_dir(workspace_root, mode)
}

/// Decides whether `target_dir` can only be written by `workspace_root`'s
/// checkout.
///
/// True when the directory is inside the workspace root (the default
/// `target/`, or any explicit path below it), or when one of its
/// components equals the workspace directory's file name — the
/// `…/<name>` or `…/target/<name>` layout a per-worktree
/// `CARGO_TARGET_DIR` is derived with. A path that is neither inside the
/// worktree nor names it (`…/rust-crimson-skies/target` on the owner's
/// fleet) is shared by construction.
pub fn is_per_worktree(workspace_root: &Path, target_dir: &Path) -> bool {
    let root = canonicalize_lenient(workspace_root);
    let candidate = if target_dir.is_absolute() {
        target_dir.to_path_buf()
    } else {
        root.join(target_dir)
    };
    let candidate = canonicalize_lenient(&candidate);

    if candidate.starts_with(&root) {
        return true;
    }
    match root.file_name() {
        Some(name) => candidate
            .components()
            .any(|component| component.as_os_str() == name),
        None => false,
    }
}

/// The whole gate: resolve the effective directory through Cargo, then
/// require it to be private to `workspace_root`. The `Ok` payload is the
/// directory itself, so callers can print what was verified.
pub fn verify_workspace(workspace_root: &Path) -> Result<PathBuf, TargetDirError> {
    let target_dir = effective_target_dir(workspace_root)?;
    require_per_worktree(workspace_root, target_dir)
}

/// [`verify_workspace`] with `CARGO_TARGET_DIR` forced in the spawned
/// cargo, same meaning as [`effective_target_dir_with_env`]: `Some` sets it,
/// `None` removes it. The acceptance tests use it to drive the shared-dir
/// rejection through the real `cargo metadata` path.
pub fn verify_workspace_with_env(
    workspace_root: &Path,
    cargo_target_dir: Option<&OsStr>,
) -> Result<PathBuf, TargetDirError> {
    let target_dir = effective_target_dir_with_env(workspace_root, cargo_target_dir)?;
    require_per_worktree(workspace_root, target_dir)
}

/// [`is_per_worktree`] as a verdict: `Err(Shared)` when the effective
/// directory could be written by other checkouts too.
fn require_per_worktree(
    workspace_root: &Path,
    target_dir: PathBuf,
) -> Result<PathBuf, TargetDirError> {
    if is_per_worktree(workspace_root, &target_dir) {
        Ok(target_dir)
    } else {
        Err(TargetDirError::Shared {
            workspace_root: workspace_root.to_path_buf(),
            target_dir,
        })
    }
}

/// Extracts the `target_directory` string from `cargo metadata` JSON.
///
/// `cargo metadata` emits one JSON object; this reads the single field the
/// gate needs without pulling in a JSON crate, matching the hand-rolled
/// parsing elsewhere in `cs_xtask`.
pub fn target_dir_from_metadata(json: &str) -> Option<PathBuf> {
    json_unescape(json_field_raw(json, "target_directory")?).map(PathBuf::from)
}

/// Runs `cargo metadata --no-deps` in `workspace_root` and reads its
/// `target_directory`.
fn metadata_target_dir(
    workspace_root: &Path,
    env_mode: EnvMode,
) -> Result<PathBuf, TargetDirError> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut command = Command::new(&cargo);
    command
        .current_dir(workspace_root)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .env("NO_COLOR", "1")
        .env("CARGO_TERM_COLOR", "never");
    match env_mode {
        EnvMode::Inherit => {}
        EnvMode::Unset => {
            command.env_remove("CARGO_TARGET_DIR");
        }
        EnvMode::Set(value) => {
            command.env("CARGO_TARGET_DIR", value);
        }
    }

    let output = command.output().map_err(|source| TargetDirError::Launch {
        program: cargo.clone(),
        source,
    })?;
    if !output.status.success() {
        return Err(TargetDirError::Metadata {
            status: output.status.to_string(),
            tail: tail(&String::from_utf8_lossy(&output.stderr), 20),
        });
    }
    let json = String::from_utf8_lossy(&output.stdout);
    target_dir_from_metadata(&json).ok_or_else(|| TargetDirError::MetadataShape {
        tail: tail(&json, 20),
    })
}

/// Canonicalizes `path` when it exists; otherwise canonicalizes its deepest
/// existing ancestor and re-appends the missing tail, so a not-yet-created
/// `target/` still compares equal across symlinked parents (macOS maps
/// `/tmp` to `/private/tmp`). Relative paths are made absolute first.
fn canonicalize_lenient(path: &Path) -> PathBuf {
    let mut head = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .map(|dir| dir.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let mut tail_components: Vec<std::ffi::OsString> = Vec::new();
    while !head.exists() {
        match head.file_name() {
            Some(name) => {
                tail_components.push(name.to_os_string());
                head.pop();
            }
            None => break,
        }
    }
    let mut resolved = head.canonicalize().unwrap_or(head);
    for component in tail_components.iter().rev() {
        resolved.push(component);
    }
    resolved
}

/// The raw — still escaped — contents of the string field `"key"` in a JSON
/// document, or `None` when it is absent or not a string.
fn json_field_raw<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let key_end = json.find(&format!("\"{key}\""))? + key.len() + 2;
    let rest = json[key_end..].trim_start().strip_prefix(':')?;
    let rest = rest.trim_start().strip_prefix('"')?;
    // A quote terminates the string only when preceded by an even number of
    // consecutive backslashes; odd means it is escaped content.
    let mut backslashes: u32 = 0;
    for (index, character) in rest.char_indices() {
        match character {
            '\\' => backslashes += 1,
            '"' if backslashes % 2 == 1 => backslashes = 0,
            '"' => return Some(&rest[..index]),
            _ => backslashes = 0,
        }
    }
    None
}

/// Decodes JSON string escapes; `None` on a malformed sequence.
fn json_unescape(raw: &str) -> Option<String> {
    if !raw.contains('\\') {
        return Some(raw.to_string());
    }
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match chars.next()? {
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            '/' => out.push('/'),
            'b' => out.push('\u{0008}'),
            'f' => out.push('\u{000C}'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'u' => {
                let high = hex_quad(&mut chars)?;
                let code = if (0xD800..0xDC00).contains(&high) {
                    if chars.next() != Some('\\') || chars.next() != Some('u') {
                        return None;
                    }
                    let low = hex_quad(&mut chars)?;
                    if !(0xDC00..0xE000).contains(&low) {
                        return None;
                    }
                    0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)
                } else {
                    high
                };
                out.push(char::from_u32(code)?);
            }
            _ => return None,
        }
    }
    Some(out)
}

/// Four hexadecimal digits of a `\uXXXX` escape.
fn hex_quad(chars: &mut impl Iterator<Item = char>) -> Option<u32> {
    let mut value = 0;
    for _ in 0..4 {
        value = value * 16 + chars.next()?.to_digit(16)?;
    }
    Some(value)
}

/// Last `lines` lines of a log, for an error message.
fn tail(log: &str, lines: usize) -> String {
    let all: Vec<&str> = log.lines().collect();
    let start = all.len().saturating_sub(lines);
    all[start..].join("\n")
}
