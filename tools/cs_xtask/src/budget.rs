//! Build-footprint guard for the CI runner's disk (task #430).
//!
//! `.github/` is owner-maintained and protected (see [`crate::ci`]), so the
//! half of the CI job this workspace *can* own is its own build: the profile
//! that decides how many bytes `cargo test` writes onto the runner's disk.
//!
//! What went wrong, measured (run 36736183776, `rust` job, ubuntu24 image):
//! while `cargo test` was running, the root filesystem that also holds
//! `/tmp` had **1.91 GiB free of 144 GiB (99% used)**, and the job died a few
//! seconds into linking the merged `cs_app` doctest — the largest single
//! write of the run — with
//! `collect2: fatal error: ld terminated with signal 7 [Bus error]` raised
//! inside rust-lld's writer threads. A memory-mapped output file that the
//! filesystem cannot extend faults with `SIGBUS`, which is what that message
//! is; the same tree passed on other runners with the same restored cache,
//! because the headroom differed.
//!
//! The bytes that spend the budget are DWARF. rustc's default (`debug = 2`)
//! puts type, variable and name information in every object: measured on this
//! workspace, `libbevy_pbr.rlib` is 169.5 MB of which 102.7 MB is
//! `__debug_*` and 77.9 MB of that is `__debug_str`, while the line program
//! that keeps `file:line` in a backtrace is 4.9 MB of it. So the rule this
//! module enforces is narrow and mechanical:
//!
//! * `[profile.dev]` must state `debug` explicitly, because *not* stating it
//!   is `debug = 2`.
//! * `debug` in `profile.dev`, `profile.test` and `profile.bench` must not be
//!   a full-DWARF level. `test` and `bench` are listed separately because
//!   they inherit `dev` but can override it, and the profile CI's `cargo test`
//!   link runs under is the one that crashed.
//!
//! Everything else about the profile is deliberately not this gate's
//! business: `opt-level` is task #336's (`[profile.dev.package."*"]`), and
//! `incremental` is already `0` on the runner (`CARGO_INCREMENTAL=0` in the
//! same run), so asserting it here would only slow local iteration.
//!
//! The measured numbers, the runs that crashed and the `.github/` options
//! this leaves to the owner are in
//! `docs/findings/2026-09-30-t430-rust-lld-sigbus-in-ci.md`.

use std::fmt;
use std::fs;
use std::path::Path;

/// Workspace manifest, relative to the workspace root.
pub const MANIFEST_PATH: &str = "Cargo.toml";

/// Profile tables CI builds under. `test` and `bench` inherit `dev`, so the
/// rule is stated once for `dev` and the overrides are checked so a later
/// `[profile.test] debug = 2` cannot quietly undo it.
pub const CI_PROFILES: [&str; 3] = ["dev", "test", "bench"];

/// The `debug` levels that emit full type and variable DWARF.
///
/// `true` is cargo's boolean spelling of the same setting, not a reduced one:
/// a scratch crate built once with `debug = true` and once with `debug = 2`
/// produced a byte-identical binary (555,480 B each, macOS aarch64, rustc
/// 1.98.1), so a manifest that states `debug = true` is the full-DWARF state
/// this gate exists to reject. `"full-dwarf"` is not a level cargo accepts; it
/// is listed so a manifest carrying clang's spelling of `2` is still refused
/// rather than passed as something unknown.
pub const FULL_DWARF_LEVELS: [&str; 4] = ["2", "true", "\"full\"", "\"full-dwarf\""];

/// The workspace manifest must exist and keep CI's build footprint small.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BudgetError {
    /// The manifest could not be read.
    Io { path: String },
    /// `[profile.dev]` does not state a `debug` level, so rustc's default
    /// (full DWARF) applies to every dependency object CI links.
    UnstatedDebug { file: String },
    /// A CI profile — or one of its dependency tables — states a full-DWARF
    /// `debug` level.
    FullDwarf {
        file: String,
        profile: String,
        value: String,
    },
}

impl fmt::Display for BudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path } => write!(f, "cannot read {path}"),
            Self::UnstatedDebug { file } => write!(
                f,
                "{file} sets no [profile.dev] debug level, so rustc's default \
applies and the whole Bevy dependency graph is built with full DWARF \
(`debug = 2`): that is the footprint the CI runner ran out of disk \
linking the cs_app doctest (task #430). Set `debug = \"line-tables-only\"` \
to keep file:line in backtraces and drop the rest."
            ),
            Self::FullDwarf {
                file,
                profile,
                value,
            } => write!(
                f,
                "{file} sets [{profile}] debug = {value}, which emits \
full type and variable DWARF for every dependency object; the CI runner \
ran out of disk writing them and died linking the cs_app doctest with \
`ld terminated with signal 7 [Bus error]` (task #430). Use \
`debug = \"line-tables-only\"` for backtrace line numbers without the \
bulk."
            ),
        }
    }
}

impl std::error::Error for BudgetError {}

/// Every `debug` value stated by a table CI builds under, with the table it
/// was stated in.
///
/// A table counts when its first component is `dev`, `test` or `bench`, so
/// `[profile.dev.package."*"]` is included: the dependency tables are where
/// the bulk of the bytes is. Only exact `[profile.dev]` satisfies the
/// "must be stated" rule, because a per-package override leaves the
/// workspace's own crates on rustc's default.
pub fn ci_profile_debugs(manifest: &str) -> Vec<(&str, &str)> {
    let mut stated = Vec::new();
    let mut inside: Option<&str> = None;
    for raw in manifest.lines() {
        // A trailing comment must not decide the outcome: `[profile.dev] #
        // set in #430` is still `[profile.dev]`, and `debug = 2 # full type
        // info` is still `debug = 2`.
        let line = strip_comment(raw.trim());
        if let Some(rest) = line.strip_prefix('[') {
            let header = rest.trim_end_matches(']').trim();
            inside = header
                .strip_prefix("profile.")
                .filter(|rest| {
                    CI_PROFILES.iter().any(|profile| {
                        *rest == *profile || rest.starts_with(&format!("{profile}."))
                    })
                })
                .map(|_| header);
            continue;
        }
        let Some(header) = inside else { continue };
        if let Some((key, value)) = line.split_once('=')
            && key.trim() == "debug"
        {
            stated.push((header, value.trim()));
        }
    }
    stated
}

/// Drops a trailing TOML comment from an already-trimmed line, and the
/// whitespace that surrounded it. `debug` values are levels and paths, none of
/// which can contain a `#`, so the first one starts the comment.
fn strip_comment(line: &str) -> &str {
    line.split_once('#')
        .map_or(line, |(head, _)| head)
        .trim_end()
}

/// The `debug` value stated by exactly `[profile.<profile>]`, if it states one.
pub fn profile_debug<'a>(manifest: &'a str, profile: &str) -> Option<&'a str> {
    let header = format!("profile.{profile}");
    ci_profile_debugs(manifest)
        .into_iter()
        .find(|(table, _)| *table == header)
        .map(|(_, value)| value)
}

/// Whether `value` (as written in a manifest) is a full-DWARF `debug` level.
pub fn is_full_dwarf(value: &str) -> bool {
    let value = value.trim().trim_matches('"').trim();
    FULL_DWARF_LEVELS
        .iter()
        .any(|level| level.trim_matches('"') == value)
}

/// Verifies the profile tables in `manifest`'s text.
pub fn verify_manifest(file: &str, manifest: &str) -> Result<(), BudgetError> {
    if profile_debug(manifest, "dev").is_none() {
        return Err(BudgetError::UnstatedDebug {
            file: file.to_string(),
        });
    }
    for (table, value) in ci_profile_debugs(manifest) {
        if is_full_dwarf(value) {
            return Err(BudgetError::FullDwarf {
                file: file.to_string(),
                profile: table.to_string(),
                value: value.to_string(),
            });
        }
    }
    Ok(())
}

/// Verifies `<workspace_root>/Cargo.toml`.
pub fn verify_workspace(workspace_root: &Path) -> Result<(), BudgetError> {
    let path = workspace_root.join(MANIFEST_PATH);
    let text = fs::read_to_string(&path).map_err(|_| BudgetError::Io {
        path: path.display().to_string(),
    })?;
    verify_manifest(&path.display().to_string(), &text)
}
