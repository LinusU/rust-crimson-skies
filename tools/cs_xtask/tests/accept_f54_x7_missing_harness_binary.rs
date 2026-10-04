//! F54-X7 / task #608: `cargo test` reporting
//! `could not execute process … (never executed)`.
//!
//! That message is not a test failure and never was one: cargo printed
//! `Running <unit> (<path>)` and then `exec` of that path failed, because the
//! test-harness executable was no longer on disk. No test in that unit ran, so
//! the run carries no information about the code under test. Before #608 such
//! a run was reported as a generic cargo failure, which is indistinguishable
//! from a real regression unless whoever reads it already knows the message.
//!
//! These tests pin the classification the `cs_xtask test-select` gate uses, and
//! they pin that a real failing test keeps outranking it, so the fix cannot
//! quietly turn a genuine failure into "rerun once". The mechanism, the
//! reproduction and the residual limit are in
//! `docs/findings/2026-10-04-f54-x7-missing-test-harness-binary.md`.

use cs_xtask::test_select::{
    ParsedLog, SelectError, classify_missing_harness, missing_harness_command, parse_run_log,
};

/// The cargo output of an observed failure on this workspace, with the paths
/// made generic: the message is the point, not the machine it happened on.
///
/// Shape as cargo 1.98 printed it (three reported occurrences, three different
/// crates): `error: test failed, to rerun pass …`, then a `Caused by:` chain
/// naming the executable and the OS error.
const OBSERVED_LOG: &str = "\
   Compiling cs_inspect v0.0.0 (/workspace/tools/cs_inspect)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 8.48s
     Running unittests src/main.rs (target/debug/deps/cs_inspect-c0cbf818ae044ebc)

error: test failed, to rerun pass `-p cs_inspect --bin cs-inspect`

Caused by:
  could not execute process `/workspace/target/debug/deps/cs_inspect-c0cbf818ae044ebc` (never executed)

Caused by:
  No such file or directory (os error 2)
error: 1 target failed:
    `--bin cs-inspect`
";

/// A parsed log of a run in which tests really ran and passed.
fn green_run() -> ParsedLog {
    parse_run_log(
        "running 2 tests\n\
         test accept_f54_x7_one ... ok\n\
         test accept_f54_x7_two ... ok\n\
         \n\
         test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n",
    )
}

/// The classification the gate reports when the harness executable was gone.
///
/// Observable failure if the classification is removed: the error comes back as
/// `None`, the caller falls through to `classify_selection`, and the run is
/// reported as a bare `CargoFailed` with no statement about which harness was
/// lost or that no test ran — exactly the ambiguity this task removes.
#[test]
fn accept_f54_x7_a_run_whose_harness_vanished_is_reported_as_a_missing_harness() {
    let parsed = parse_run_log(OBSERVED_LOG);
    assert_eq!(parsed.passed, 0, "the observed log runs no test at all");
    assert_eq!(parsed.failed, 0, "the observed log fails no test either");

    let error = classify_missing_harness(
        "the selection run for prefix \"accept_f54_x7_\"",
        Some("exit status: 101"),
        &parsed,
        OBSERVED_LOG,
    )
    .expect("a vanished harness must be classified, not swallowed");

    match &error {
        SelectError::HarnessMissing {
            context, command, ..
        } => {
            assert_eq!(
                command, "/workspace/target/debug/deps/cs_inspect-c0cbf818ae044ebc",
                "the error must name the executable cargo could not run"
            );
            assert!(
                context.contains("accept_f54_x7_"),
                "the error must say which run lost the harness, got {context:?}"
            );
        }
        other => panic!("a vanished harness must not be reported as {other:?}"),
    }
}

/// The reproduction in this task's finding deleted 371 harnesses while a
/// filtered selection run was in flight. Cargo then printed the *command* it
/// could not execute, which for a filtered run is the harness path followed by
/// the test arguments — so all of it must survive rather than a guess at where
/// the path ends.
///
/// Observable failure if the command is truncated at the first space: the error
/// would name a harness that does not exist, which is the one thing this
/// classification must never do.
#[test]
fn accept_f54_x7_the_diagnosis_keeps_the_arguments_cargo_appended_to_the_path() {
    let log = "     Running unittests src/lib.rs (target/debug/deps/cs_app-d95888fab103ceeb)\n\
               error: test failed, to rerun pass `-p cs_app --lib`\n\
               \n\
               Caused by:\n\
                 \x20 could not execute process `/w/target/debug/deps/cs_app-d95888fab103ceeb accept_f54_x7_deterministic_probe_ --include-ignored --color never` (never executed)\n\
               \n\
               Caused by:\n\
                 \x20 No such file or directory (os error 2)\n";
    let parsed = parse_run_log(log);
    let error =
        classify_missing_harness("the selection run", Some("exit status: 101"), &parsed, log)
            .expect("a vanished harness must be classified");

    match &error {
        SelectError::HarnessMissing { command, .. } => assert_eq!(
            command,
            "/w/target/debug/deps/cs_app-d95888fab103ceeb \
             accept_f54_x7_deterministic_probe_ --include-ignored --color never",
            "the whole printed command must survive, not a truncated path"
        ),
        other => panic!("a vanished harness must not be reported as {other:?}"),
    }
    assert!(
        error.to_string().contains("cs_app-d95888fab103ceeb"),
        "the message must still name the harness"
    );
}

/// The guidance is the point of the fix: an agent reading only this error must
/// learn that no test ran, that a rerun is the only thing that says anything,
/// and that a green rerun does not make the failed run green.
///
/// Observable failure if the message is trimmed: each of the three sentences
/// below is asserted, so losing any of them fails this test.
#[test]
fn accept_f54_x7_the_diagnosis_says_rerun_once_and_never_call_the_failed_run_green() {
    let error = classify_missing_harness(
        "the selection run for prefix \"accept_f54_x7_\"",
        Some("exit status: 101"),
        &green_run(),
        OBSERVED_LOG,
    )
    .expect("the classification must apply");
    let message = error.to_string();

    for required in [
        "not a test failure",
        "Rerun the identical command once",
        "report both runs",
        "never the rerun alone",
        "docs/findings/2026-10-04-f54-x7-missing-test-harness-binary.md",
    ] {
        assert!(
            message.contains(required),
            "the diagnosis must carry {required:?}, got: {message}"
        );
    }
    assert!(
        message.contains("No such file or directory (os error 2)"),
        "the diagnosis must keep the cargo evidence, got: {message}"
    );
}

/// A real failing test is the specific problem and keeps its own error: the
/// gate must not answer "a harness vanished, rerun once" for a run that
/// actually failed a test.
///
/// Observable failure if the precedence is dropped: the returned value is
/// `Some(HarnessMissing)` and the failing test is never named.
#[test]
fn accept_f54_x7_a_failing_test_outranks_a_vanished_harness() {
    let log = format!(
        "{OBSERVED_LOG}\n\
         running 1 test\n\
         test accept_f54_x7_broken ... FAILED\n\
         \n\
         test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\n"
    );
    let parsed = parse_run_log(&log);
    assert_eq!(
        parsed.failed, 1,
        "the fixture must really contain a failure"
    );

    assert!(
        classify_missing_harness("the selection run", Some("exit status: 101"), &parsed, &log)
            .is_none(),
        "a failing test must keep its own, more precise error"
    );
}

/// A green run cannot have lost a harness, and an ordinary cargo failure (a
/// compile error, an interrupted build) must not be relabelled as one.
///
/// Observable failure if the guard is removed: both fixtures are classified as
/// a vanished harness and the original diagnosis is lost.
#[test]
fn accept_f54_x7_only_a_failed_run_with_the_cargo_message_is_a_missing_harness() {
    // Green cargo status: nothing was lost.
    assert!(
        classify_missing_harness("the selection run", None, &green_run(), OBSERVED_LOG).is_none(),
        "a green run must never be classified as a vanished harness"
    );

    // Cargo failed, but for another reason: a compile error.
    let compile_error = "error[E0425]: cannot find value `nope` in this scope\n\
                         --> crates/cs_types/src/lib.rs:1:1\n\
                         error: could not compile `cs_types` (lib) due to 1 previous error\n";
    let parsed = parse_run_log(compile_error);
    assert!(
        classify_missing_harness(
            "the selection run",
            Some("exit status: 101"),
            &parsed,
            compile_error,
        )
        .is_none(),
        "a compile error must keep the generic cargo diagnosis"
    );
}

/// The message is recognised only in the exact form cargo prints: both
/// `could not execute process` and `(never executed)`, and a non-empty path.
///
/// Observable failure if the match is loosened to either half alone: the
/// compile-error and failing-test fixtures above would be misread, and this
/// test fails on the half-message fixtures.
#[test]
fn accept_f54_x7_only_the_full_cargo_exec_message_names_a_harness() {
    assert_eq!(
        missing_harness_command(
            "error: test failed, to rerun pass `-p cs_inspect --bin cs-inspect`\n"
        ),
        None,
        "a failing unit alone must not name a harness"
    );
    assert_eq!(
        missing_harness_command("  No such file or directory (os error 2)\n"),
        None,
        "the OS error alone must not name a harness"
    );
    assert_eq!(
        missing_harness_command("  could not execute process (never executed)\n"),
        None,
        "the message without a path must not name a harness"
    );
    assert_eq!(
        missing_harness_command(OBSERVED_LOG),
        Some("/workspace/target/debug/deps/cs_inspect-c0cbf818ae044ebc".to_string()),
        "the real message must name the executable"
    );
}
