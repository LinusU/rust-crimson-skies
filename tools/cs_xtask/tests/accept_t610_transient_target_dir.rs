//! Acceptance tests for task #610 (`CS-XTASK-FLAKE`): the cs_xtask checks
//! must stay deterministic while a *second* cargo process writes this target
//! directory.
//!
//! Two mechanisms produced the reported `No such file or directory (os error
//! 2)` failures:
//!
//! - Cargo scheduled empty unit-test harness binaries for `src/lib.rs` and
//!   `src/main.rs`; a concurrent rebuild replaced them between cargo's
//!   discovery and exec. `tools/cs_xtask/Cargo.toml` now sets `test = false`
//!   on both targets so workspace test runs never exec them.
//! - Every gate read (`fs::read_to_string`, `fs::read_dir`, `Path::exists`)
//!   ran once against paths a writer can transiently remove. Those reads now
//!   go through [`cs_xtask::transient`], which retries only `NotFound` and
//!   surfaces the last real error.
//!
//! The tests below exercise the production helpers against a real concurrent
//! rewriter and pin the manifest configuration that keeps the empty harnesses
//! out of the test plan. They intentionally do **not** weaken any T430
//! assertion — `accept_t430_ci_disk_budget.rs` is untouched by this task.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use cs_xtask::transient::{self, Policy};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Scratch space unique to this test process, matching the pid-keyed layout
/// the sibling suites adopted so overlapping `cargo test` runs cannot delete
/// each other's fixtures.
fn scratch(name: &str) -> PathBuf {
    let dir = workspace_root().join(format!(
        "target/t610-transient-fixtures/{}/{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("the scratch directory must be creatable");
    dir
}

/// A read racing a writer that repeatedly unlinks and atomically replaces a
/// file must never surface `NotFound` — the patient retry window spans the
/// remove-and-relink gap cargo creates under `target/debug/`.
#[test]
fn accept_t610_a_read_survives_a_concurrent_rewriter() {
    let dir = scratch("rewrite");
    let file = dir.join("under-write.txt");
    let staging = dir.join("under-write.staging");
    const PAYLOAD: &str = "payload written atomically\n";

    let done = Arc::new(AtomicBool::new(false));
    let writer = {
        let (file, staging, done) = (file.clone(), staging.clone(), Arc::clone(&done));
        std::thread::spawn(move || {
            for _ in 0..400 {
                fs::write(&staging, PAYLOAD).expect("the staging write must succeed");
                fs::rename(&staging, &file).expect("the atomic replace must succeed");
                let _ = fs::remove_file(&file);
            }
            // End with the file present: a writer whose last act deletes the
            // path has really deleted it, and a reader is owed an error.
            fs::write(&staging, PAYLOAD).expect("the staging write must succeed");
            fs::rename(&staging, &file).expect("the final replace must succeed");
            done.store(true, Ordering::SeqCst);
        })
    };

    while !done.load(Ordering::SeqCst) {
        let text = transient::read_to_string(&file, transient::PATIENT).unwrap_or_else(|err| {
            panic!("a transient remove-and-relink must be retried, not fail: {err}")
        });
        assert_eq!(text, PAYLOAD, "the rename must never produce a torn read");
    }
    writer.join().expect("the rewriter thread must not panic");
}

/// Retries cover transient races only: a path that stays absent must still
/// fail with `NotFound` once the policy is exhausted.
#[test]
fn accept_t610_a_missing_file_still_errors() {
    let dir = scratch("missing");
    let quick = Policy {
        attempts: 2,
        interval: Duration::from_millis(10),
    };
    let err = transient::read_to_string(&dir.join("never-created.txt"), quick)
        .expect_err("a file that never appears must surface its error");
    assert_eq!(
        err.kind(),
        io::ErrorKind::NotFound,
        "the surfaced error must be the NotFound the caller saw"
    );
}

/// `command_output` must run a stable binary on the first attempt and surface
/// a spawn `NotFound` once the retry window is spent — a missing binary is
/// reported missing, never retried forever.
#[test]
fn accept_t610_command_output_retries_only_transient_exec_races() {
    let output =
        transient::command_output(Command::new(env!("CARGO_BIN_EXE_cs_xtask")).arg("--help"))
            .expect("the cs_xtask binary must run");
    assert!(output.status.success(), "--help must exit zero");

    let err = transient::command_output(&mut Command::new("cs-xtask-definitely-not-a-binary"))
        .expect_err("exec of a missing binary must fail");
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
}

/// Directory scans see a mid-write tree, not a fatal error: the scan policy
/// retries an absent directory, then still reports `NotFound` once the window
/// closes — the `let Ok(..) else` idiom the dep-info walk uses turns that
/// into an empty listing, so a populated dir yields its entries either way.
#[test]
fn accept_t610_directory_scans_retry_then_report_absence() {
    let dir = scratch("scan");
    let missing = dir.join("not-there");
    let entry_count = transient::read_dir(&missing, transient::SCAN)
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(
        entry_count, 0,
        "a directory a writer has not created yet must walk as empty"
    );
    assert!(
        !transient::is_dir(&missing, transient::SCAN),
        "absence is not a directory"
    );
    assert!(
        !transient::is_file(&missing, transient::SCAN),
        "absence is not a file"
    );

    fs::write(dir.join("one.d"), "a").expect("a fixture entry must be writable");
    fs::write(dir.join("two.d"), "b").expect("a fixture entry must be writable");
    assert_eq!(
        transient::read_dir(&dir, transient::SCAN)
            .expect("the populated dir must list")
            .count(),
        2,
        "every entry the writer committed must be listed"
    );
    assert!(transient::is_dir(&dir, transient::PATIENT));
    assert!(transient::is_file(&dir.join("one.d"), transient::PATIENT));
}

/// The reported flake exec'd `cs_xtask --lib` and `--bin cs_xtask` unit-test
/// harnesses that ran zero tests. `test = false` on both targets removes them
/// from the workspace test plan so a concurrent cargo can never be mid-relink
/// on a binary this run is about to exec. This pins that configuration and
/// the invariant that makes it safe: no unit tests live in either target.
#[test]
fn accept_t610_cs_xtask_schedules_no_empty_unit_test_harnesses() {
    let manifest = transient::read_to_string(
        &workspace_root().join("tools/cs_xtask/Cargo.toml"),
        transient::PATIENT,
    )
    .expect("the cs_xtask manifest must be readable");

    for section in ["[lib]", "[[bin]]"] {
        let start = manifest
            .find(section)
            .unwrap_or_else(|| panic!("the manifest must declare a `{section}` target"));
        let body = &manifest[start..];
        let end = body[section.len()..]
            .find("\n[")
            .map_or(body.len(), |offset| section.len() + offset);
        assert!(
            body[..end]
                .lines()
                .any(|line| line.trim() == "test = false"),
            "the `{section}` target must opt out of unit-test compilation so no \
             empty harness binary is scheduled"
        );
    }

    // The opt-out is only sound while neither target carries `#[test]`s; pin
    // that so adding one is a deliberate revert, not a silent drop.
    for source in ["src/lib.rs", "src/main.rs"] {
        let text = transient::read_to_string(
            &workspace_root().join("tools/cs_xtask").join(source),
            transient::PATIENT,
        )
        .expect("the cs_xtask sources must be readable");
        assert!(
            !text.contains("#[test]"),
            "{source} must keep zero unit tests while `test = false` drops its harness"
        );
    }
}
