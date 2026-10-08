//! F00-C: CI is installed and keeps running the workspace gates.
//!
//! `.github/workflows/ci.yml` is owner-maintained, so this test does not
//! change it — it proves the workflow still runs fmt, clippy with
//! `-D warnings` and the workspace test suite, and that a workflow which
//! drops a gate is rejected instead of silently passing. The negative cases
//! are built from the real file, so they fail if the guard starts accepting
//! anything.

use std::fs;
use std::path::{Path, PathBuf};

use cs_xtask::ci::{self, CiError, WORKFLOW_PATH};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Package names of the real workspace members.
fn members() -> Vec<String> {
    let root = workspace_root();
    let manifest = fs::read_to_string(root.join("Cargo.toml")).expect("the root manifest exists");
    cs_xtask::bootstrap::workspace_members(&manifest)
        .expect("the root manifest lists members")
        .iter()
        .map(|dir| dir.rsplit('/').next().unwrap().to_string())
        .collect()
}

/// The real workflow with its single workspace test command replaced by the
/// split the owner uses: the app on one runner, the rest on another.
fn split_workflow() -> String {
    let full = workflow_text();
    let single = "cargo test --workspace --locked";
    if !full.contains("cargo test -p cs_app --locked") {
        assert!(
            full.contains(single),
            "the real workflow must test the workspace"
        );
        return full.replace(
            single,
            "cargo test -p cs_app --locked\n        run: cargo test --workspace --locked --exclude cs_app",
        );
    }
    full
}

fn workflow_text() -> String {
    fs::read_to_string(workspace_root().join(WORKFLOW_PATH))
        .expect("the owner-maintained CI workflow must exist and be readable")
}

/// The installed CI really gates the workspace: fmt check, clippy with
/// `-D warnings`, and the locked workspace test suite.
///
/// Observable failure if the gate is removed: `verify_workflow_file` returns
/// `Err(MissingGate)` naming the gate that disappeared.
#[test]
fn accept_f00_c_ci_workflow_runs_fmt_clippy_and_workspace_tests() {
    ci::verify_workspace_workflow(&workspace_root())
        .expect("CI must keep running fmt, clippy with -D warnings and the workspace tests");
}

/// The counter-example the guard exists for: a workflow that loses one of the
/// three gates must be rejected, naming the gate and the missing command.
#[test]
fn accept_f00_c_ci_workflow_without_a_gate_is_rejected() {
    let full = workflow_text();

    let without_fmt: String = full
        .lines()
        .filter(|line| !line.contains("cargo fmt"))
        .collect::<Vec<&str>>()
        .join("\n");
    let error = ci::verify_workflow(WORKFLOW_PATH, &without_fmt, &members())
        .expect_err("a workflow without the fmt gate must be rejected");
    assert!(
        matches!(
            &error,
            CiError::MissingGate { gate, .. } if *gate == "cargo fmt --all -- --check"
        ),
        "the rejection must name the fmt gate, got {error:?}"
    );

    let without_clippy: String = full
        .lines()
        .filter(|line| !line.contains("cargo clippy"))
        .collect::<Vec<&str>>()
        .join("\n");
    let error = ci::verify_workflow(WORKFLOW_PATH, &without_clippy, &members())
        .expect_err("a workflow without the clippy gate must be rejected");
    assert!(
        matches!(
            &error,
            CiError::MissingGate { gate, .. } if *gate == "cargo clippy with -D warnings"
        ),
        "the rejection must name the clippy gate, got {error:?}"
    );
    // `-D warnings` is part of that gate: keeping the command without it is
    // the exact shortcut the spec forbids.
    let clippy_without_denials = full.replace(" -- -D warnings", " --");
    let error = ci::verify_workflow(WORKFLOW_PATH, &clippy_without_denials, &members())
        .expect_err("clippy without -D warnings must be rejected");
    assert!(
        matches!(
            &error,
            CiError::MissingGate { gate, missing, .. }
                if *gate == "cargo clippy with -D warnings" && missing == "-D warnings"
        ),
        "the rejection must blame the missing -D warnings, got {error:?}"
    );

    let without_tests: String = full
        .lines()
        .filter(|line| !line.contains("cargo test"))
        .collect::<Vec<&str>>()
        .join("\n");
    let error = ci::verify_workflow(WORKFLOW_PATH, &without_tests, &members())
        .expect_err("a workflow without the test gate must be rejected");
    assert!(
        matches!(
            &error,
            CiError::MissingGate { gate, .. } if *gate == "cargo test --workspace --locked"
        ),
        "the rejection must name the test gate, got {error:?}"
    );
}

/// A missing workflow is a failure, not an implied pass.
#[test]
fn accept_f00_c_a_missing_ci_workflow_is_reported() {
    let error = ci::verify_workspace_workflow(&workspace_root().join("target/no-workspace-here"))
        .expect_err("a workspace without the workflow must be rejected");
    assert!(
        matches!(&error, CiError::Io { path } if path.ends_with(WORKFLOW_PATH)),
        "the rejection must name the unreadable workflow, got {error:?}"
    );
    assert!(
        error.to_string().contains("cannot read"),
        "the error must say what failed, got {error}"
    );
}

/// A workflow that splits the suite across commands is accepted while the
/// commands together cover every member.
#[test]
fn accept_f00_c_split_test_commands_covering_every_member_are_accepted() {
    ci::verify_workflow(WORKFLOW_PATH, &split_workflow(), &members())
        .expect("a complete split must pass");
}

/// Dropping the step that tests `cs_app` leaves it untested, and the
/// rejection names it.
#[test]
fn accept_f00_c_dropping_a_split_step_names_the_untested_member() {
    let without_app: String = split_workflow()
        .lines()
        .filter(|line| !line.contains("cargo test -p cs_app"))
        .collect::<Vec<&str>>()
        .join("\n");
    let error = ci::verify_workflow(WORKFLOW_PATH, &without_app, &members())
        .expect_err("cs_app is no longer tested");
    assert!(
        matches!(&error, CiError::UntestedMember { member, .. } if member == "cs_app"),
        "got {error:?}"
    );
    assert!(error.to_string().contains("cs_app"));
}

/// An extra `--exclude cs_sim` leaves `cs_sim` untested, in both the split
/// and the single-command workflow.
#[test]
fn accept_f00_c_an_extra_exclude_names_the_untested_member() {
    for text in [split_workflow(), workflow_text()] {
        let excluded = text.replace(
            "cargo test --workspace --locked",
            "cargo test --workspace --locked --exclude cs_sim",
        );
        let error = ci::verify_workflow(WORKFLOW_PATH, &excluded, &members())
            .expect_err("cs_sim is excluded and tested nowhere");
        assert!(
            matches!(&error, CiError::UntestedMember { member, .. } if member == "cs_sim"),
            "got {error:?}"
        );
    }
}

/// A test step without `--locked` is rejected, naming the command.
#[test]
fn accept_f00_c_a_test_step_without_locked_is_rejected() {
    let unlocked =
        split_workflow().replace("cargo test -p cs_app --locked", "cargo test -p cs_app");
    let unlocked = if unlocked == split_workflow() {
        workflow_text().replace("cargo test --workspace --locked", "cargo test --workspace")
    } else {
        unlocked
    };
    let error = ci::verify_workflow(WORKFLOW_PATH, &unlocked, &members())
        .expect_err("an unlocked cargo test must be rejected");
    assert!(
        matches!(&error, CiError::TestNotLocked { command, .. } if command.starts_with("cargo test")),
        "got {error:?}"
    );
}
