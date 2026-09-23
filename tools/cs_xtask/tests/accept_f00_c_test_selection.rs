//! F00-C: task-specific positive-test discovery.
//!
//! `docs/contracts/CLI-EVIDENCE.md` demands that a task prefix really
//! resolves to tests: the selection must run at least one test, none may
//! fail, and every discovered test must pass again when re-run alone with
//! `--exact`. These tests drive the production gate
//! (`cs_xtask::test_select`) against the real workspace for the positive
//! cases, and against real harness output for the failure cases a green
//! workspace cannot produce on its own (a failing test, an empty selection, a
//! build that failed). Removing the gate — or making it return success
//! unconditionally — fails them.

use std::path::{Path, PathBuf};
use std::process::Command;

use cs_xtask::test_select::{
    self, ParsedLog, SelectError, classify_exact, classify_selection, parse_run_log,
};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A prefix that exists in this workspace and passes: the gate must really
/// run it and count what ran.
///
/// Observable failure if the gate is stubbed: a stub that reports success
/// without running cargo never discovers
/// `accept_f00_b_workspace_pins_the_intended_baseline`, so the name
/// assertions fail; a stub that reports zero passes fails the count.
#[test]
fn accept_f00_c_prefix_selection_runs_and_counts_real_tests() {
    let selection = test_select::select_tests(&workspace_root(), "accept_f00_b_")
        .expect("the F00-B prefix must select and pass real tests");

    assert_eq!(
        selection.prefix, "accept_f00_b_",
        "the prefix must be echoed"
    );
    assert_eq!(selection.failed, 0, "a passing selection has no failures");
    assert!(
        selection.passed >= 1,
        "the selection must execute at least one test, got {}",
        selection.passed
    );
    assert!(
        selection.passed as usize >= selection.tests.len(),
        "every discovered test must be accounted for by a pass: {} tests, {} passed",
        selection.tests.len(),
        selection.passed
    );
    assert!(
        selection
            .tests
            .iter()
            .any(|name| name == "accept_f00_b_workspace_pins_the_intended_baseline"),
        "the selection must discover the known F00-B test, got {:?}",
        selection.tests
    );
    assert!(
        selection
            .tests
            .iter()
            .all(|name| name.contains("accept_f00_b_")),
        "only tests of the prefix may be reported, got {:?}",
        selection.tests
    );
    assert!(
        selection.log.contains("test result:"),
        "the gate must keep the real harness log as evidence"
    );
}

/// The empty-selection failure the contract forbids: a prefix that matches
/// nothing must be an error, never a green run with zero tests.
#[test]
fn accept_f00_c_a_prefix_that_selects_nothing_is_an_error() {
    let error = test_select::select_tests(&workspace_root(), "accept_f00_c_zz_no_such_test")
        .expect_err("a prefix that selects no test must not be reported as success");

    assert!(
        matches!(
            &error,
            SelectError::Empty { prefix } if prefix == "accept_f00_c_zz_no_such_test"
        ),
        "the rejection must name the empty prefix, got {error:?}"
    );
    assert!(
        error.to_string().contains("accept_f00_c_zz_no_such_test"),
        "the message must name the prefix, got {error}"
    );
}

/// An empty prefix is rejected before cargo is even started.
#[test]
fn accept_f00_c_an_empty_prefix_is_rejected_before_running_cargo() {
    let error = test_select::select_tests(&workspace_root(), "")
        .expect_err("an empty prefix must be rejected instead of selecting everything");
    assert!(matches!(error, SelectError::EmptyPrefix), "got {error:?}");
}

/// Failure classification against real harness output: a failing selection,
/// an empty selection, a cargo failure and an all-green run each mean
/// something different, and only the green one passes.
#[test]
fn accept_f00_c_failing_and_empty_selections_are_classified_as_failures() {
    let failing = "\
     Running tests/accept_f00_b_cli_help.rs (target/debug/deps/accept_f00_b_cli_help-ab68fd102e3aa028)
running 2 tests
test accept_f00_b_one ... ok
test accept_f00_b_broken ... FAILED

failures:
    accept_f00_b_broken

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.02s";

    let parsed = parse_run_log(failing);
    assert_eq!(parsed.passed, 1, "one test passed in the captured run");
    assert_eq!(parsed.failed, 1, "one test failed in the captured run");
    assert_eq!(parsed.tests, vec!["accept_f00_b_one".to_string()]);
    assert_eq!(parsed.failing, vec!["accept_f00_b_broken".to_string()]);

    let error = classify_selection("accept_f00_b_", None, &parsed, failing)
        .expect_err("a failing selection must not pass");
    assert!(
        matches!(
            &error,
            SelectError::SelectionFailed { prefix, failed, failing }
                if prefix == "accept_f00_b_" && *failed == 1
                    && failing == &vec!["accept_f00_b_broken".to_string()]
        ),
        "the rejection must name the prefix, the count and the failing test, got {error:?}"
    );
    // A failing test is the specific failure, even when cargo also exited
    // nonzero; claiming "cargo failed" would hide which test broke.
    let error = classify_selection(
        "accept_f00_b_",
        Some("exit status: 101".to_string()),
        &parsed,
        failing,
    )
    .expect_err("a failing selection must not pass");
    assert!(
        matches!(error, SelectError::SelectionFailed { .. }),
        "got {error:?}"
    );

    let empty = "\
running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 14 filtered out; finished in 0.00s";
    let parsed = parse_run_log(empty);
    assert_eq!(parsed.passed, 0, "nothing ran");
    assert!(
        classify_selection("accept_f00_c_zz", None, &parsed, empty).is_err(),
        "an empty selection must never be a success"
    );
    let error = classify_selection("accept_f00_c_zz", None, &parsed, empty)
        .expect_err("an empty selection must be an error");
    assert!(matches!(error, SelectError::Empty { .. }), "got {error:?}");
    let error = classify_selection(
        "accept_f00_c_zz",
        Some("exit status: 101".to_string()),
        &parsed,
        "error: could not compile `cs_app`",
    )
    .expect_err("a cargo failure must not be a success");
    assert!(
        matches!(&error, SelectError::CargoFailed { context, tail, .. }
            if context.contains("accept_f00_c_zz") && tail.contains("could not compile")),
        "the rejection must name the prefix and quote the cargo output, got {error:?}"
    );

    let green = "\
running 1 test
test accept_f00_c_thing ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s";
    let parsed = parse_run_log(green);
    assert_eq!(parsed.passed, 1, "summaries of every target must be summed");
    assert!(
        classify_selection("accept_f00_c_", None, &parsed, green).is_ok(),
        "a green selection must pass"
    );

    // Doc-test names contain spaces and dashes; they are tests like any
    // other and must survive parsing.
    let doc_tests = "\
test crates/cs_types/src/lib.rs - cs_types::Tick (line 5) ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s";
    assert_eq!(
        parse_run_log(doc_tests).tests,
        vec!["crates/cs_types/src/lib.rs - cs_types::Tick (line 5)".to_string()],
        "doc-test names must be discovered too"
    );
}

/// The `--exact` half of the contract: a discovered test must run when it is
/// re-run alone, and a name that selects nothing there is an error — a
/// prefix cannot be rescued by an unrelated test that merely embeds it.
#[test]
fn accept_f00_c_exact_re_run_selects_each_discovered_test_alone() {
    let known = "accept_f00_b_workspace_pins_the_intended_baseline".to_string();
    test_select::verify_exact(&workspace_root(), std::slice::from_ref(&known))
        .expect("a discovered test must pass when re-run alone with --exact");

    let unknown = "accept_f00_c_zz_no_such_test_exact".to_string();
    let error = test_select::verify_exact(&workspace_root(), std::slice::from_ref(&unknown))
        .expect_err("an --exact run that selects nothing must fail");
    assert!(
        matches!(&error, SelectError::ExactEmpty { name } if name == &unknown),
        "the rejection must name the test, got {error:?}"
    );

    // Classification of the cases a green workspace cannot produce: a test
    // failing alone, and a cargo failure during the re-run.
    let failing = "\
running 1 test
test accept_f00_c_thing ... FAILED

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s";
    let parsed = parse_run_log(failing);
    let error = classify_exact(
        "accept_f00_c_thing",
        Some("exit status: 101".to_string()),
        &parsed,
        failing,
    )
    .expect_err("a failing --exact run must fail");
    assert!(
        matches!(&error, SelectError::ExactFailed { name, failed }
            if name == "accept_f00_c_thing" && *failed == 1),
        "the rejection must name the failing test, got {error:?}"
    );

    let error = classify_exact(
        "accept_f00_c_thing",
        Some("exit status: 101".to_string()),
        &ParsedLog::default(),
        "error: could not compile `cs_app`",
    )
    .expect_err("a cargo failure during the re-run must fail");
    assert!(
        matches!(error, SelectError::CargoFailed { .. }),
        "got {error:?}"
    );
}

/// The command agents actually run, end to end: `cs_xtask test-select` must
/// execute the gate (select, then re-run each name with `--exact`) and report
/// what it did, while a bad invocation is a usage error (exit 2) and an
/// unusable workspace is a gate failure (exit 1) — never a silent success.
#[test]
fn accept_f00_c_test_select_command_runs_the_gate() {
    let root = workspace_root();
    let bin = env!("CARGO_BIN_EXE_cs_xtask");

    let output = Command::new(bin)
        .args([
            "test-select",
            "--prefix",
            "accept_f00_c_ci_workflow_runs_fmt",
            "--workspace-root",
        ])
        .arg(&root)
        .output()
        .expect("the cs_xtask binary must run");
    assert_eq!(
        output.status.code(),
        Some(0),
        "the gate must pass for a real, green prefix; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("selected accept_f00_c_ci_workflow_runs_fmt_clippy_and_workspace_tests"),
        "the command must print what it discovered, got: {stdout:?}"
    );
    assert!(
        stdout.contains("re-ran alone with --exact and passed"),
        "the command must report the --exact re-runs, got: {stdout:?}"
    );

    let unknown_command = Command::new(bin)
        .arg("no-such-command")
        .output()
        .expect("the cs_xtask binary must run");
    assert_eq!(
        unknown_command.status.code(),
        Some(2),
        "an unknown command must be a usage error"
    );
    assert!(
        String::from_utf8_lossy(&unknown_command.stderr).contains("unknown command"),
        "the usage error must name the command"
    );

    let missing_prefix = Command::new(bin)
        .arg("test-select")
        .output()
        .expect("the cs_xtask binary must run");
    assert_eq!(
        missing_prefix.status.code(),
        Some(2),
        "test-select without --prefix must be a usage error"
    );
    assert!(
        String::from_utf8_lossy(&missing_prefix.stderr).contains("--prefix"),
        "the usage error must name the missing option"
    );

    let bad_root = Command::new(bin)
        .args(["verify-ci", "--workspace-root"])
        .arg(root.join("target/not-a-workspace"))
        .output()
        .expect("the cs_xtask binary must run");
    assert_eq!(
        bad_root.status.code(),
        Some(1),
        "an unusable workspace root must fail the gate, not the usage check"
    );
    assert!(
        String::from_utf8_lossy(&bad_root.stderr).contains("not a workspace root"),
        "the failure must explain the root, got: {}",
        String::from_utf8_lossy(&bad_root.stderr)
    );

    let verify_ci = Command::new(bin)
        .args(["verify-ci", "--workspace-root"])
        .arg(&root)
        .output()
        .expect("the cs_xtask binary must run");
    assert_eq!(
        verify_ci.status.code(),
        Some(0),
        "verify-ci must pass on this workspace; stderr: {}",
        String::from_utf8_lossy(&verify_ci.stderr)
    );
    assert!(
        String::from_utf8_lossy(&verify_ci.stdout).contains("runs cargo fmt"),
        "verify-ci must say what it checked"
    );

    let help = Command::new(bin)
        .arg("--help")
        .output()
        .expect("the cs_xtask binary must run");
    assert_eq!(help.status.code(), Some(0), "--help must exit zero");
    assert!(
        String::from_utf8_lossy(&help.stdout).contains("test-select"),
        "--help must describe the gate"
    );
}
