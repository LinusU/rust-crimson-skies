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
//! components *below the point where it diverges from the workspace's own
//! path* is the workspace directory's name — the `<shared-root>/<worktree>`
//! layout the recommended per-agent environment derives. A component that
//! only names the worktree's parent does not count: on a `…/<name>/<name>`
//! checkout (GitHub Actions' `<repo>/<repo>`) the sibling-level
//! `…/<name>/target` would match through the parent and still be shared by
//! every checkout under it. Anything else is a directory every worktree can
//! compute identically, which is exactly the reported defect.
//!
//! Being private is necessary but not sufficient (task #433). Cargo keys an
//! artifact by package id and metadata fingerprint, *not* by source path, so
//! a directory that is private by name can still serve a checkout that no
//! longer exists: build `wt-a` and an identical `survivor` from one tree,
//! point both at `survivor/target`, then delete `wt-a`. Cargo calls the
//! artifacts fresh — same content, same fingerprint — so `survivor` keeps
//! running `wt-a`'s binary, with `wt-a`'s `env!("CARGO_MANIFEST_DIR")`
//! compiled into it and now naming a path that is gone. That is how
//! `cs_formats`' own `pe_resources` test came to read
//! `…/bunny-alpha-1-rev98/crates/cs_formats/src/pe_resources.rs` after
//! `bunny-alpha-1-rev98` was removed.
//!
//! Cargo leaves the evidence behind: for every unit whose crate read
//! `CARGO_MANIFEST_DIR`, its `.d` dep-info file carries an
//! `# env-dep:CARGO_MANIFEST_DIR=<absolute path>` line naming the checkout
//! that produced it. [`recorded_manifest_dirs`] reads those lines and
//! [`removed_manifest_dirs`] keeps the ones that are no longer a directory on
//! disk, which a crate's manifest directory can only be if its checkout is
//! gone. That is positive evidence, so it needs no guesswork about cargo's
//! hashing: a target directory with no such record — a fresh one, or one
//! built only from this checkout — passes.
//!
//! A *live* foreign checkout is the third case (task #440). A directory can be
//! private by name and hold a record that still exists — a sibling worktree
//! that built here and was never deleted — which the removed rule cannot see,
//! because it is not gone. The recorded path's existence cannot separate
//! "another checkout" from "cargo's own cache", but its location can: a
//! recorded manifest directory that is neither inside this workspace root nor
//! under cargo's home (`$CARGO_HOME`, else `~/.cargo`) belongs to a foreign
//! checkout, alive or not. The cargo-home exclusion is load-bearing: any
//! registry crate that reads `CARGO_MANIFEST_DIR` records
//! `$CARGO_HOME/registry/src/…`, so without it the rule would fail on ordinary
//! dependencies. [`foreign_manifest_dirs`] applies that, and
//! [`verify_workspace`] reports it as [`TargetDirError::Foreign`].
//!
//! [`verify_workspace`] is the gate: the `accept_t383_`, `accept_t433_`,
//! `accept_t437_` and `accept_t440_` tests run it against this very checkout as
//! part of `cargo test --workspace`, so an agent whose environment still exports
//! a shared directory, whose private directory still serves a removed worktree,
//! or whose private directory holds a live foreign checkout's artifacts, gets a
//! loud failure naming the fix instead of silently trusting foreign binaries.
//!
//! A gate has to name the checkout it judges, and a compiled test binary can
//! outlive the tree that built it (task #437).
//! [`running_workspace_root`] therefore derives the root from the process's
//! working directory, which cargo sets inside the checkout under test, rather
//! than from `env!("CARGO_MANIFEST_DIR")`, which is baked into whichever
//! checkout compiled the binary. Without it a reused artifact judges the wrong
//! tree and reports a private target directory as shared.

use std::collections::BTreeSet;
use std::env;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::transient;

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
    /// The effective directory is private to this worktree, but it still
    /// holds artifacts a checkout that no longer exists produced.
    Stale {
        workspace_root: PathBuf,
        target_dir: PathBuf,
        removed: Vec<PathBuf>,
    },
    /// The effective directory is private to this worktree, but it still
    /// holds artifacts a *different, still-present* checkout produced: the
    /// recorded manifest directories exist, yet none of them is this
    /// worktree or cargo's own cache (task #440).
    Foreign {
        workspace_root: PathBuf,
        target_dir: PathBuf,
        foreign: Vec<PathBuf>,
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
            Self::Stale {
                workspace_root,
                target_dir,
                removed,
            } => write!(
                f,
                "the cargo target directory {} is private to worktree {}, but it \
still holds artifacts whose recorded CARGO_MANIFEST_DIR names {}: that \
checkout is gone, and cargo still calls those artifacts fresh because it \
keys them by fingerprint rather than by source path, so `cargo test` can run \
a binary that reads a path outside this worktree (task #433). The directory \
name cannot express this, so delete {} and let the next build recreate it, \
e.g. cargo clean --target-dir {}.",
                target_dir.display(),
                workspace_root.display(),
                paths(removed),
                target_dir.display(),
                target_dir.display()
            ),
            Self::Foreign {
                workspace_root,
                target_dir,
                foreign,
            } => write!(
                f,
                "the cargo target directory {} is private to worktree {}, but it \
still holds artifacts a different, still-present checkout built: their \
recorded CARGO_MANIFEST_DIR names {}, which is neither inside this worktree \
nor cargo's own cache under CARGO_HOME, so `cargo test` can run another \
worktree's binary with its source paths and fingerprints (task #440). The \
directory name cannot express this, so give each checkout its own target \
directory: delete {} and let the next build recreate it, e.g. cargo clean \
--target-dir {}.",
                target_dir.display(),
                workspace_root.display(),
                paths(foreign),
                target_dir.display(),
                target_dir.display()
            ),
        }
    }
}

/// How many paths a [`TargetDirError::Stale`] or [`TargetDirError::Foreign`]
/// message names before it counts the rest.
const PATHS_SHOWN: usize = 5;

/// Renders up to [`PATHS_SHOWN`] paths, then counts the rest, so a message
/// about a hundred dead worktrees stays readable.
fn paths(values: &[PathBuf]) -> String {
    let shown = values
        .iter()
        .take(PATHS_SHOWN)
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    if values.len() > PATHS_SHOWN {
        format!("{shown} and {} more", values.len() - PATHS_SHOWN)
    } else {
        shown
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
/// components *below the point where it diverges from the workspace root*
/// equals the workspace directory's file name — the `…/<name>` or
/// `…/target/<name>` layout a per-worktree `CARGO_TARGET_DIR` is derived
/// with. The divergence bound matters: a component that is part of the
/// workspace's own ancestry names the fleet, not this checkout — on a
/// `…/<name>/<name>` checkout (GitHub Actions' `<repo>/<repo>`) the
/// sibling-level `…/<name>/target` must still read as shared. A path that
/// is neither inside the worktree nor names it
/// (`…/rust-crimson-skies/target` on the owner's fleet) is shared by
/// construction.
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
    let Some(name) = root.file_name() else {
        return false;
    };
    let root_components: Vec<_> = root.components().collect();
    let candidate_components: Vec<_> = candidate.components().collect();
    let shared_ancestry = candidate_components
        .iter()
        .zip(&root_components)
        .take_while(|(candidate, root)| candidate == root)
        .count();
    candidate_components[shared_ancestry..]
        .iter()
        .any(|component| component.as_os_str() == name)
}

/// The manifest table that marks a `Cargo.toml` as the root of a workspace.
const WORKSPACE_TABLE: &str = "[workspace]";

/// The workspace root of the checkout that contains `start`, found at *run*
/// time by walking up to the nearest manifest carrying a `[workspace]` table,
/// or `None` when no ancestor of `start` has one.
///
/// This is the same nearest-ancestor rule cargo itself applies to resolve a
/// package's workspace, so it cannot name a different root than the one cargo
/// built against.
///
/// The reason it is not `env!("CARGO_MANIFEST_DIR")` is task #437: a compiled
/// test binary can outlive the tree that built it. A `CARGO_TARGET_DIR` shared
/// between checkouts — or one naming a checkout that was later replaced — lets
/// `cargo test` reuse a foreign artifact, and the manifest directory baked into
/// that artifact names a root this run never touches. A gate built on the baked
/// root then judges the wrong checkout: it reports a private `…/f18b/target` as
/// shared because the baked root is `…/devin-1`, which is a failure with no
/// cause in the environment under test. Cargo runs test binaries with the
/// package root as the working directory, so a walk from
/// [`running_workspace_root`] lands inside the checkout that is *running*,
/// whichever checkout compiled the binary.
///
/// Only the table header counts: `[workspace.dependencies]` is a different
/// table, and `# [workspace]` is a comment, so neither makes a manifest a
/// workspace root.
pub fn workspace_root_from(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| {
            transient::read_to_string(&dir.join("Cargo.toml"), transient::SCAN).is_ok_and(
                |manifest| {
                    manifest
                        .lines()
                        .any(|line| line.trim_start().starts_with(WORKSPACE_TABLE))
                },
            )
        })
        .map(Path::to_path_buf)
}

/// [`workspace_root_from`] at the process's own working directory: the
/// workspace root of the checkout this process is *running* in.
///
/// `None` when the working directory cannot be read, or when no ancestor of it
/// holds a workspace manifest — which is also what the `verify-target-dir`
/// subcommand's own default (`--workspace-root .`) resolves to, so the CLI and
/// the live gates agree on what "this worktree" means.
pub fn running_workspace_root() -> Option<PathBuf> {
    workspace_root_from(&env::current_dir().ok()?)
}

/// The whole gate: resolve the effective directory through Cargo, then
/// require it to be private to `workspace_root` and to hold only artifacts
/// this checkout could have built — none from a checkout that is gone (task
/// #433), none from a different checkout that is still there (task #440). The
/// `Ok` payload is the directory itself, so callers can print what was
/// verified.
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
/// directory could be written by other checkouts too, then `Err(Stale)` or
/// `Err(Foreign)` when it is private but still serves a checkout that is gone
/// (task #433) or one that is still there (task #440). Order matters: a
/// directory another live worktree could write to is the #383 defect whatever
/// it contains, and that is the message an agent in the shared layout needs
/// first.
fn require_per_worktree(
    workspace_root: &Path,
    target_dir: PathBuf,
) -> Result<PathBuf, TargetDirError> {
    if !is_per_worktree(workspace_root, &target_dir) {
        return Err(TargetDirError::Shared {
            workspace_root: workspace_root.to_path_buf(),
            target_dir,
        });
    }
    require_live(workspace_root, target_dir)
}

/// [`require_per_worktree`]'s second question: does the directory hold only
/// artifacts this checkout could have built?
///
/// Two positive kinds of evidence say no, and they are reported in the order
/// cargo's own evidence appears: a recorded checkout that is *gone*
/// ([`TargetDirError::Stale`], task #433) comes before one that is *present but
/// foreign* ([`TargetDirError::Foreign`], task #440). A removed directory is the
/// stronger evidence — it is a path that cannot be checked out again — so it
/// wins when a directory carries both, keeping #433's verdict unchanged.
fn require_live(workspace_root: &Path, target_dir: PathBuf) -> Result<PathBuf, TargetDirError> {
    let removed: Vec<PathBuf> = removed_manifest_dirs(&target_dir).into_iter().collect();
    if !removed.is_empty() {
        return Err(TargetDirError::Stale {
            workspace_root: workspace_root.to_path_buf(),
            target_dir,
            removed,
        });
    }
    let foreign: Vec<PathBuf> = foreign_manifest_dirs(workspace_root, &target_dir)
        .into_iter()
        .collect();
    if !foreign.is_empty() {
        return Err(TargetDirError::Foreign {
            workspace_root: workspace_root.to_path_buf(),
            target_dir,
            foreign,
        });
    }
    Ok(target_dir)
}

/// Cargo's home: `$CARGO_HOME` when it is set and non-empty, else `~/.cargo`.
///
/// The registry cache cargo keeps there is not a foreign checkout — any
/// registry crate that reads `CARGO_MANIFEST_DIR` records
/// `$CARGO_HOME/registry/src/…` — so [`foreign_manifest_dirs`] must exclude it.
/// `None` only when neither `CARGO_HOME` nor a home directory can be read; the
/// exclusion is then impossible, which is reported by treating nothing as
/// cargo's cache rather than by guessing one.
pub fn cargo_home() -> Option<PathBuf> {
    if let Some(value) = env::var_os("CARGO_HOME")
        && !value.is_empty()
    {
        return Some(PathBuf::from(value));
    }
    home_dir().map(|home| home.join(".cargo"))
}

/// The platform's home directory, matching what cargo falls back to for
/// `CARGO_HOME`: `$HOME` on Unix, `$USERPROFILE` (or `$HOMEDRIVE$HOMEPATH`) on
/// Windows.
#[cfg(not(windows))]
fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

/// [`home_dir`] for Windows.
#[cfg(windows)]
fn home_dir() -> Option<PathBuf> {
    if let Some(profile) = env::var_os("USERPROFILE") {
        return Some(PathBuf::from(profile));
    }
    let drive = env::var_os("HOMEDRIVE")?;
    let path = env::var_os("HOMEPATH")?;
    let mut home = PathBuf::from(drive);
    home.push(path);
    Some(home)
}

/// The [`recorded_manifest_dirs`] that still exist but are a *different*
/// checkout: neither inside `workspace_root` nor under cargo's home
/// ([`cargo_home`]).
///
/// This is task #440's rule. The name alone cannot tell the two apart — a
/// private directory can hold a live sibling worktree's record — but the
/// location can: anything outside this workspace and outside cargo's own cache
/// belongs to another checkout, whether or not that checkout is still on disk.
/// A record that is gone is [`removed_manifest_dirs`]'s, not this function's.
pub fn foreign_manifest_dirs(workspace_root: &Path, target_dir: &Path) -> BTreeSet<PathBuf> {
    let home = cargo_home();
    foreign_manifest_dirs_with_home(workspace_root, target_dir, home.as_deref())
}

/// [`foreign_manifest_dirs`] with cargo's home forced, the seam the acceptance
/// tests use to point the exclusion at a fixture `CARGO_HOME` instead of the
/// machine's real one.
pub fn foreign_manifest_dirs_with_home(
    workspace_root: &Path,
    target_dir: &Path,
    cargo_home: Option<&Path>,
) -> BTreeSet<PathBuf> {
    let root = canonicalize_lenient(workspace_root);
    let home = cargo_home.map(canonicalize_lenient);
    recorded_manifest_dirs(target_dir)
        .into_iter()
        .filter(|dir| transient::is_dir(dir, transient::SCAN))
        .filter(|dir| {
            let candidate = canonicalize_lenient(dir);
            if candidate.starts_with(&root) {
                return false;
            }
            match &home {
                Some(home) => !candidate.starts_with(home),
                None => true,
            }
        })
        .collect()
}

/// Cargo's marker for an environment variable a unit read while compiling,
/// as it appears in a `.d` dep-info file.
const ENV_DEP_MARKER: &str = "# env-dep:";
/// The environment variable whose value is an absolute path into the checkout
/// that produced the artifact — the one record of another worktree, removed or
/// still present, that survives in a target directory.
pub const MANIFEST_DIR_VAR: &str = "CARGO_MANIFEST_DIR";

/// Every `CARGO_MANIFEST_DIR` cargo recorded in `target_dir`'s dep-info files.
///
/// Cargo writes the value verbatim, unescaped, so a path with spaces in it
/// survives intact. Files that cannot be read are skipped: the gate reports
/// only directories it actually saw recorded, never a guess about one it
/// could not open.
pub fn recorded_manifest_dirs(target_dir: &Path) -> BTreeSet<PathBuf> {
    let mut recorded = BTreeSet::new();
    for dep_info in dep_info_files(target_dir) {
        let Ok(bytes) = transient::read(&dep_info, transient::SCAN) else {
            continue;
        };
        if let Some(value) = env_dep_value(&String::from_utf8_lossy(&bytes), MANIFEST_DIR_VAR) {
            recorded.insert(PathBuf::from(value));
        }
    }
    recorded
}

/// The [`recorded_manifest_dirs`] that are no longer a directory on disk.
///
/// A crate's manifest directory is a directory for as long as its checkout
/// is there, so a recorded value that is not one is positive evidence that
/// the worktree this target directory once served has been removed — the
/// case cargo cannot see, because its artifact filenames and fingerprints
/// are a function of content, not of where the content lives.
pub fn removed_manifest_dirs(target_dir: &Path) -> BTreeSet<PathBuf> {
    recorded_manifest_dirs(target_dir)
        .into_iter()
        .filter(|dir| !transient::is_dir(dir, transient::SCAN))
        .collect()
}

/// The value of `# env-dep:<name>=<value>` in one dep-info file, or `None`
/// when the file records nothing about `name`.
///
/// The marker has to start the line: a dependency called `env-dep` appears
/// later on the same line as its artifact, and must not be mistaken for one.
pub fn env_dep_value<'a>(dep_info: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("{ENV_DEP_MARKER}{name}=");
    dep_info.lines().find_map(|line| {
        let value = line.trim_start().strip_prefix(prefix.as_str())?;
        Some(value.strip_suffix('\r').unwrap_or(value))
    })
}

/// The dep-info files in `target_dir`: `<profile>/deps/*.d`, where cargo
/// writes one per compiled unit, plus the `<profile>/*.d` copies it uplifts
/// next to the final artifacts. A target directory that does not exist yet,
/// or one whose layout this cargo version does not use, simply has none.
fn dep_info_files(target_dir: &Path) -> Vec<PathBuf> {
    let Ok(profiles) = transient::read_dir(target_dir, transient::SCAN) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for profile in profiles.flatten() {
        if !is_directory(&profile) {
            continue;
        }
        let profile_path = profile.path();
        for directory in [profile_path.join("deps"), profile_path] {
            let Ok(entries) = transient::read_dir(&directory, transient::SCAN) else {
                continue;
            };
            for entry in entries.flatten() {
                let is_dep_info = entry.file_name().to_string_lossy().ends_with(".d");
                if is_dep_info && !is_directory(&entry) {
                    files.push(entry.path());
                }
            }
        }
    }
    files
}

/// Whether `entry` is a directory that can be listed.
///
/// A listing entry whose file vanished mid-walk still classifies as a
/// directory under [`transient::SCAN`]: skipping a profile directory because
/// its own `file_type` call lost a race would read none of its dep-info and
/// pass a stale target directory as clean.
fn is_directory(entry: &fs::DirEntry) -> bool {
    transient::file_type(entry, transient::SCAN).is_ok_and(|kind| kind.is_dir())
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
