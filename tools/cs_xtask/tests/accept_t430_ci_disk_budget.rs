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
use cs_xtask::transient;

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
    // `true` is cargo's boolean spelling of `2`, not a reduced level: the same
    // scratch crate built with `debug = true` and with `debug = 2` produced a
    // byte-identical binary. A trailing comment must not smuggle a level past
    // the gate either.
    for (value, recorded) in [
        ("2", "2"),
        ("\"2\"", "\"2\""),
        ("true", "true"),
        ("\"full\"", "\"full\""),
        (" 2 ", "2"),
        ("2 # full type information, needed for the debugger", "2"),
    ] {
        let manifest = format!("[profile.dev]\ndebug = {value}\n");
        assert_eq!(
            budget::verify_manifest("Cargo.toml", &manifest),
            Err(BudgetError::FullDwarf {
                file: "Cargo.toml".to_string(),
                profile: "profile.dev".to_string(),
                value: recorded.to_string(),
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
        "false",
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

/// Name of the helper process that panics on purpose, for
/// [`accept_t430_a_panic_backtrace_names_the_file_and_line`]. It is not an
/// acceptance test itself: it returns immediately unless the parent asks for
/// it, so it carries no `accept_t430_` prefix and nothing selects it by name.
const PANIC_HELPER: &str = "cs_xtask_t430_panic_helper";

/// Environment variable that turns this test binary into the panicking child.
const PANIC_HELPER_ENV: &str = "CS_XTASK_T430_PANIC_HELPER";

/// The process that panics when [`PANIC_HELPER_ENV`] is set, and does nothing
/// at all otherwise. It is the only test in the binary that does.
#[test]
fn cs_xtask_t430_panic_helper() {
    if std::env::var_os(PANIC_HELPER_ENV).is_none() {
        return;
    }
    panic!("cs_xtask task #430: this panic exists to be located in a backtrace");
}

/// The setting above is only worth its megabytes if the binary really keeps
/// `file:line`, so this checks the artifact instead of the manifest string: it
/// runs [`PANIC_HELPER`] in a child process with `RUST_BACKTRACE=1` and
/// requires a backtrace *frame* to name a source file and a line.
///
/// Only the frames are read, never the header the panic hook prints for the
/// panic's own location: that location is a string the compiler wrote into the
/// binary, so it would pass even with the line program thrown away. A profile
/// without line tables prints `0x...` per frame instead, and this fails.
#[test]
fn accept_t430_a_panic_backtrace_names_the_file_and_line() {
    let child = transient::command_output(
        std::process::Command::new(
            std::env::current_exe().expect("this test binary must have a path to re-run"),
        )
        .args(["--exact", PANIC_HELPER, "--nocapture"])
        .env(PANIC_HELPER_ENV, "1")
        .env("RUST_BACKTRACE", "1"),
    )
    .expect("the panic helper must be runnable");
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(
        !child.status.success(),
        "the helper process must have panicked, so there is a backtrace to read: {output}"
    );
    let frames = output
        .split_once("stack backtrace:")
        .map_or("", |(_, frames)| frames);
    assert!(
        names_source_location(frames),
        "no backtrace frame named a source file and line, so the test binaries \
CI builds cannot locate a failing test: RUST_BACKTRACE=1 printed only \
addresses. The committed setting is [profile.dev] debug = \
\"line-tables-only\" (cs_xtask verify-ci-budget); the helper's output was: \
{output}"
    );
}

/// Whether any whitespace-separated token in `text` is a `something.rs:12` style
/// location, which is what a backtrace frame looks like when the line program
/// survived. The column that may follow the line number is ignored.
fn names_source_location(text: &str) -> bool {
    text.split_ascii_whitespace().any(|token| {
        token
            .rsplit_once(".rs:")
            .is_some_and(|(_, position)| position.starts_with(|c: char| c.is_ascii_digit()))
    })
}
