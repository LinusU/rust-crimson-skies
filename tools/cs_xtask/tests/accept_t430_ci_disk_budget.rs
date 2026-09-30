//! Task #430: keep the CI job's build footprint inside the runner's disk.
//!
//! The `rust` CI job died on different runners at the same step: linking the
//! merged `cs_app` doctest failed with `collect2: fatal error: ld terminated
//! with signal 7 [Bus error]` inside rust-lld's writer threads. The
//! measurement behind it (run 36736183776) is that the root filesystem which
//! also holds `/tmp` — where rustdoc links the doctest — had 1.91 GiB free of
//! 144 GiB, 99% used, while `cargo test` ran. A memory-mapped output file the
//! filesystem cannot extend faults with `SIGBUS`; the same tree passed on
//! runners that had more headroom with the identical restored cache.
//!
//! `.github/` is owner-maintained, so the part this workspace owns is the
//! profile CI builds under: rustc's default `debug = 2` puts type, variable
//! and name DWARF in every dependency object (`libbevy_pbr.rlib` is 169.5 MB
//! of which 102.7 MB is `__debug_*`), and that is the footprint that spends
//! the budget. The fix is one line in the workspace manifest,
//! `[profile.dev] debug = "line-tables-only"`, and [`cs_xtask::budget`] is the
//! gate that keeps it there.
//!
//! Evidence, measurements and the `.github/` options left to the owner are in
//! `docs/findings/2026-09-30-t430-rust-lld-sigbus-in-ci.md`.

use std::path::{Path, PathBuf};

use cs_xtask::budget::{self, BudgetError};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root must exist")
}

/// The workspace this task changed: reverting `[profile.dev] debug` to
/// rustc's default — deleting the line or setting it to `2` — must fail this
/// gate, so the CI job cannot silently go back to the footprint that ran the
/// runner out of disk.
#[test]
fn accept_t430_this_workspace_keeps_full_dwarf_out_of_the_ci_profiles() {
    if let Err(error) = budget::verify_workspace(&workspace_root()) {
        panic!(
            "{error}\n\
             (gate: cs_xtask verify-ci-budget)"
        );
    }
}

/// The manifest as CI crashed with it: no `[profile.dev] debug` at all, so
/// rustc's default applies to every dependency object. This is the exact
/// revert, and the gate has to name it rather than pass it.
#[test]
fn accept_t430_the_unreduced_profile_ci_crashed_with_is_rejected() {
    let manifest = "\
[workspace]
members = []

[profile.dev.package.\"*\"]
opt-level = 3
";
    assert_eq!(
        budget::profile_debug(manifest, "dev"),
        None,
        "the recorded pre-fix manifest states no [profile.dev] debug level"
    );
    assert_eq!(
        budget::verify_manifest("Cargo.toml", manifest),
        Err(BudgetError::UnstatedDebug {
            file: "Cargo.toml".to_string(),
        })
    );
}

/// A full-DWARF level is rejected in every table CI links under, including
/// the dependency tables where the bytes actually are.
#[test]
fn accept_t430_full_dwarf_is_rejected_wherever_ci_states_it() {
    for value in ["2", "\"full\"", " 2 "] {
        let manifest = format!("[profile.dev]\ndebug = {value}\n");
        assert_eq!(
            budget::verify_manifest("Cargo.toml", &manifest),
            Err(BudgetError::FullDwarf {
                file: "Cargo.toml".to_string(),
                profile: "profile.dev".to_string(),
                value: value.trim().to_string(),
            }),
            "debug = {value} must not pass: it is full DWARF"
        );
    }

    // The dependency tables are part of the same profile and hold the bulk.
    let dependency_override = "\
[profile.dev]
debug = \"line-tables-only\"

[profile.dev.package.\"*\"]
debug = 2
opt-level = 3
";
    assert_eq!(
        budget::verify_manifest("Cargo.toml", dependency_override),
        Err(BudgetError::FullDwarf {
            file: "Cargo.toml".to_string(),
            profile: "profile.dev.package.\"*\"".to_string(),
            value: "2".to_string(),
        })
    );

    // And so is a profile that inherits from `dev` and overrides it back.
    let inherited = "\
[profile.dev]
debug = \"line-tables-only\"

[profile.test]
debug = 2
";
    assert_eq!(
        budget::verify_manifest("Cargo.toml", inherited),
        Err(BudgetError::FullDwarf {
            file: "Cargo.toml".to_string(),
            profile: "profile.test".to_string(),
            value: "2".to_string(),
        })
    );
}

/// Every reduced level cargo accepts passes, so the gate does not force one
/// spelling: line numbers in a backtrace are what has to survive, not the
/// particular string.
#[test]
fn accept_t430_reduced_debug_levels_pass_the_budget() {
    for value in [
        "0",
        "\"none\"",
        "1",
        "\"limited\"",
        "\"line-tables-only\"",
        "\"line-directives-only\"",
    ] {
        let manifest = format!(
            "[profile.dev]\ndebug = {value}\n\n[profile.dev.package.\"*\"]\nopt-level = 3\n"
        );
        assert_eq!(
            budget::verify_manifest("Cargo.toml", &manifest),
            Ok(()),
            "debug = {value} must stay inside the CI disk budget"
        );
    }
}

/// The committed manifest keeps `file:line` for a panic backtrace: the
/// setting this task relies on is the one that still emits line tables, not
/// one that trades the whole debug section away for a few more megabytes.
#[test]
fn accept_t430_the_workspace_keeps_backtrace_line_numbers() {
    let manifest = std::fs::read_to_string(workspace_root().join(budget::MANIFEST_PATH))
        .expect("the workspace manifest must be readable");
    assert_eq!(
        budget::profile_debug(&manifest, "dev"),
        Some("\"line-tables-only\""),
        "[profile.dev] debug must be \"line-tables-only\": a panic backtrace \
still needs file:line, and full DWARF is what filled the CI runner's disk"
    );
}
