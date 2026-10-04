//! Task-specific positive-test discovery (F00-C).
//!
//! `docs/contracts/CLI-EVIDENCE.md` requires that a task's unique test prefix
//! really resolves to tests: `cargo test --workspace --locked -- <prefix>
//! --include-ignored` must execute and pass at least one test, and every
//! discovered test must pass again when re-run alone with `--exact`. This
//! module runs exactly that command and refuses to report success when
//! nothing was selected ([`SelectError::Empty`]) or when anything failed
//! ([`SelectError::SelectionFailed`]), which is what turns a typo'd or
//! missing prefix into a loud failure instead of a green run with zero tests.
//!
//! It also keeps cargo's "the test harness was not executed" diagnostic out of
//! the generic [`SelectError::CargoFailed`] bucket: such a run executed no test
//! at all, so it is reported on its own ([`SelectError::HarnessMissing`], task
//! #608) with the rule that only a rerun of the identical command says
//! anything, and that a green rerun never makes the failed run green. The cause
//! is quoted from cargo's own OS error ([`HarnessFault`]) rather than assumed:
//! only `ENOENT` is called a vanished harness.
//!
//! The owner's note on F00-C puts discovery in the agents' hands rather than
//! in CI: CI keeps running fmt, clippy and the whole workspace suite (see
//! [`crate::ci`]), while the implementing and reviewing agents run this gate
//! locally for their own task prefix.

use std::fmt;
use std::io;
use std::path::Path;
use std::process::Command;

/// What one captured `cargo test` log says.
///
/// Names come from the harness's own `test <name> ... ok`/`... FAILED` lines
/// and counts from its `test result:` summaries, so this is a record of tests
/// that really ran, not of what the caller hoped would run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedLog {
    /// Names of the tests that passed, in harness order.
    pub tests: Vec<String>,
    /// Names of the tests that failed.
    pub failing: Vec<String>,
    /// Summed `passed` over every test target's summary.
    pub passed: u32,
    /// Summed `failed` over every test target's summary.
    pub failed: u32,
    /// Summed `ignored` over every test target's summary.
    pub ignored: u32,
}

/// Result of a successful prefix selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    /// The prefix that was selected with.
    pub prefix: String,
    /// Discovered test names, deduplicated, in first-seen order.
    pub tests: Vec<String>,
    /// Tests that passed across the workspace run.
    pub passed: u32,
    /// Always zero: a selection with a failure is an error, not a result.
    pub failed: u32,
    /// Tests that stayed ignored despite `--include-ignored`.
    pub ignored: u32,
    /// The combined cargo output, kept as evidence of what ran.
    pub log: String,
}

/// Result of the full gate: prefix selection plus `--exact` re-runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateReport {
    /// The prefix selection the gate started from.
    pub selection: Selection,
    /// How many discovered tests were re-run alone with `--exact`.
    pub exact_verified: u32,
}

/// Why the gate refuses to call the selection a success.
#[derive(Debug)]
pub enum SelectError {
    /// An empty prefix would select nothing by construction.
    EmptyPrefix,
    /// Cargo could not be started at all.
    Launch { program: String, source: io::Error },
    /// Cargo failed for a reason other than a failing test (compile error,
    /// missing workspace, interrupted run): nothing may be claimed.
    CargoFailed {
        context: String,
        status: String,
        tail: String,
    },
    /// The prefix selected tests, and some of them failed.
    SelectionFailed {
        prefix: String,
        failed: u32,
        failing: Vec<String>,
    },
    /// The prefix selected no test at all — the empty-selection failure the
    /// contract explicitly forbids.
    Empty { prefix: String },
    /// Cargo could not execute a test harness
    /// (`could not execute process … (never executed)`).
    ///
    /// This is its own failure, not a [`Self::CargoFailed`] fallback: no test
    /// in that unit ran, so the run says nothing at all about the code under
    /// test, and a rerun that passes does not turn the failed run green.
    HarnessMissing {
        context: String,
        /// Exactly what cargo printed as unexecutable: the harness path,
        /// followed by whatever test arguments cargo appended to it.
        command: String,
        /// What the `exec` itself failed with, read from the OS error cargo
        /// printed. The message says "not on disk" only for
        /// [`HarnessFault::Absent`].
        fault: HarnessFault,
        tail: String,
    },
    /// A test failed when it was re-run alone with `--exact`.
    ExactFailed { name: String, failed: u32 },
    /// A discovered test selected nothing when re-run alone with `--exact`,
    /// so the prefix matched a name no harness would run in isolation.
    ExactEmpty { name: String },
}

/// Why an `exec` of a test harness failed, as far as cargo's own output says.
///
/// `could not execute process … (never executed)` is printed for *every* failed
/// `exec`, and only the OS error on the next line says which kind of failure it
/// was. Naming the wrong one would be worse than naming none, so the cases are
/// kept apart instead of collapsed into "the file was deleted" (task #608).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HarnessFault {
    /// Cargo's OS error was `ENOENT` (`No such file or directory (os error 2)`):
    /// the executable was gone when cargo tried to start it. This is the case
    /// `docs/findings/2026-10-04-f54-x7-missing-test-harness-binary.md`
    /// documents.
    Absent,
    /// Cargo printed a different OS error. The file may be present and simply
    /// not runnable — wrong mode, truncated, still open for writing — so this
    /// is *not* the vanished-harness class and a repeat is a real fault to
    /// investigate as one.
    OsError(String),
    /// Cargo printed the exec message but no OS error this parser recognises,
    /// so nothing is claimed about the cause.
    Unattributed,
}

impl fmt::Display for HarnessFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent => write!(
                f,
                "the executable was not on disk, the case documented in the finding"
            ),
            Self::OsError(error) => write!(
                f,
                "the exec failed with `{error}`, which is not the vanished-harness \
case: the file may be present and unrunnable, so a repeat of this is a real \
fault to investigate as one"
            ),
            Self::Unattributed => write!(
                f,
                "cargo printed no OS error for the exec, so the cause is not \
established here"
            ),
        }
    }
}

impl fmt::Display for SelectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPrefix => write!(f, "no test prefix was given"),
            Self::Launch { program, source } => {
                write!(f, "cannot run {program}: {source}")
            }
            Self::CargoFailed {
                context,
                status,
                tail,
            } => write!(
                f,
                "cargo failed during {context} ({status}); last output:\n{tail}"
            ),
            Self::SelectionFailed {
                prefix,
                failed,
                failing,
            } => write!(
                f,
                "prefix {prefix:?} selected {failed} failing test(s): {failing:?}"
            ),
            Self::Empty { prefix } => write!(
                f,
                "prefix {prefix:?} selected no test; a task test prefix must \
 resolve to at least one test that runs"
            ),
            Self::HarnessMissing {
                context,
                command,
                fault,
                tail,
            } => write!(
                f,
                "cargo could not execute the test harness {command} during {context}: {fault}. \
No test in that unit reported a result, so this is not a test failure and says \
nothing about the code under test. \
Rerun the identical command once; if that rerun passes, report both runs — \
never the rerun alone — and see \
docs/findings/2026-10-04-f54-x7-missing-test-harness-binary.md.\n\
last output:\n{tail}"
            ),
            Self::ExactFailed { name, failed } => write!(
                f,
                "test {name:?} failed when re-run alone with --exact ({failed} failure(s))"
            ),
            Self::ExactEmpty { name } => write!(
                f,
                "test {name:?} selected nothing when re-run alone with --exact"
            ),
        }
    }
}

impl std::error::Error for SelectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Launch { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Parses one `test result:` summary line of the libtest harness.
///
/// Returns the summed counts plus whether the summary says `ok` (a `FAILED.`
/// summary is a failure even if some tests passed).
fn parse_result_line(line: &str) -> Option<(u32, u32, u32, bool)> {
    let rest = line.trim().strip_prefix("test result:")?.trim_start();
    let ok = if rest.starts_with("ok.") {
        true
    } else if rest.starts_with("FAILED.") {
        false
    } else {
        return None;
    };

    let (mut passed, mut failed, mut ignored) = (0, 0, 0);
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    for (index, token) in tokens.iter().enumerate() {
        let Ok(count) = token.trim_end_matches(';').parse::<u32>() else {
            continue;
        };
        let Some(word) = tokens.get(index + 1) else {
            continue;
        };
        match word.trim_end_matches(';') {
            "passed" => passed = count,
            "failed" => failed = count,
            "ignored" => ignored = count,
            _ => {}
        }
    }
    Some((passed, failed, ignored, ok))
}

/// Parses one `test <name> ... ok|FAILED` line of the libtest harness.
fn parse_harness_line(line: &str) -> Option<(&str, bool)> {
    let rest = line.strip_prefix("test ")?;
    if let Some(name) = rest.strip_suffix(" ... ok") {
        return Some((name, true));
    }
    rest.strip_suffix(" ... FAILED").map(|name| (name, false))
}

/// Reads names and counts out of a captured `cargo test` log.
pub fn parse_run_log(log: &str) -> ParsedLog {
    let mut parsed = ParsedLog::default();
    for line in log.lines() {
        if let Some((passed, failed, ignored, _ok)) = parse_result_line(line) {
            parsed.passed += passed;
            parsed.failed += failed;
            parsed.ignored += ignored;
        } else if let Some((name, ok)) = parse_harness_line(line) {
            let name = name.to_string();
            if ok {
                parsed.tests.push(name);
            } else {
                parsed.failing.push(name);
            }
        }
    }
    parsed
}

/// Last `lines` lines of a log, for an error message.
fn tail(log: &str, lines: usize) -> String {
    let all: Vec<&str> = log.lines().collect();
    let start = all.len().saturating_sub(lines);
    all[start..].join("\n")
}

/// The OS error cargo printed for a failed `exec`, if it printed one.
///
/// Cargo follows the `could not execute process … (never executed)` line with
/// the error the `exec` itself returned, e.g. `No such file or directory (os
/// error 2)`. Only a handful of lines after the message are scanned, so a
/// different crate's compile error later in the same log cannot be mistaken
/// for this run's `exec` failure.
fn exec_os_error(log: &str) -> Option<String> {
    let lines: Vec<&str> = log.lines().collect();
    let message = lines
        .iter()
        .position(|line| is_missing_harness_line(line))?;
    lines
        .iter()
        .skip(message + 1)
        .take(5)
        .map(|line| line.trim())
        .find(|line| line.contains("(os error "))
        .map(str::to_string)
}

/// Whether one line is cargo's "the harness was never executed" message.
///
/// Both halves are required: `could not execute process` alone also appears in
/// other cargo diagnostics, and `(never executed)` alone says nothing about
/// what failed.
fn is_missing_harness_line(line: &str) -> bool {
    let line = line.trim();
    line.contains("could not execute process") && line.contains("(never executed)")
}

/// The command cargo could not execute, if that is what happened.
///
/// Cargo prints `Running <unit> (<path>)`, then, when `exec` of that path
/// fails, `could not execute process <path> (never executed)` and the OS
/// error below it. `(never executed)` is the load-bearing part: it means the
/// harness never started, so no test in that unit ran and no assertion was
/// evaluated. Returns what cargo printed between the two markers, which is the
/// harness path followed by any test arguments cargo appended to it.
///
/// Only that one message is matched, and only together with
/// `could not execute process`, so a compile error, a failing test or any
/// other cargo diagnostic is not mistaken for it.
pub fn missing_harness_command(log: &str) -> Option<String> {
    for line in log.lines() {
        if !is_missing_harness_line(line) {
            continue;
        }
        let after = line.trim().split_once("could not execute process")?.1;
        let command = after
            .split("(never executed)")
            .next()
            .unwrap_or_default()
            .trim()
            .trim_matches('`')
            .trim();
        if command.is_empty() {
            continue;
        }
        return Some(command.to_string());
    }
    None
}

/// Reads the cause of a failed harness `exec` out of cargo's own output.
///
/// Only `ENOENT` is turned into [`HarnessFault::Absent`], because that is the
/// only OS error that supports "the file was not on disk": `Permission denied`
/// means the file is there and unreadable as an executable, `Exec format error`
/// means it is there and not a runnable image, and `Text file busy` means
/// something else is writing it. Collapsing those into a deletion would send an
/// agent looking for a prune job that never touched anything (rule 4: unknown
/// means unknown).
pub fn harness_fault(log: &str) -> HarnessFault {
    match exec_os_error(log) {
        Some(error) if error.contains("os error 2") => HarnessFault::Absent,
        Some(error) => HarnessFault::OsError(error),
        None => HarnessFault::Unattributed,
    }
}

/// Decides whether a finished run must be reported as an unexecutable test
/// harness instead of a generic cargo failure.
///
/// All three conditions are required, and each one matters:
///
/// * cargo failed — a green run never lost a harness;
/// * no test reported a failure — a real failing test is the specific
///   problem and keeps its own, more precise error;
/// * the log names an executable cargo never executed.
///
/// Returns the error to report, or `None` when this was an ordinary cargo
/// failure that the caller must classify as before.
pub fn classify_missing_harness(
    context: &str,
    cargo_failure: Option<&str>,
    parsed: &ParsedLog,
    log: &str,
) -> Option<SelectError> {
    if cargo_failure.is_none() || parsed.failed > 0 {
        return None;
    }
    let command = missing_harness_command(log)?;
    Some(SelectError::HarnessMissing {
        context: context.to_string(),
        command,
        fault: harness_fault(log),
        tail: tail(log, 12),
    })
}

/// Decides what a finished prefix selection means.
///
/// `cargo_failure` is the cargo status when cargo itself failed (compile
/// error, missing workspace); `log_tail` is quoted in that error. A failing
/// test outranks a nonzero cargo status, because it is the specific failure;
/// an empty selection is never a success.
pub fn classify_selection(
    prefix: &str,
    cargo_failure: Option<String>,
    parsed: &ParsedLog,
    log_tail: &str,
) -> Result<(), SelectError> {
    if cargo_failure.is_some() && parsed.failed == 0 {
        return Err(SelectError::CargoFailed {
            context: format!("the selection run for prefix {prefix:?}"),
            status: cargo_failure.unwrap_or_default(),
            tail: log_tail.to_string(),
        });
    }
    if parsed.failed > 0 {
        return Err(SelectError::SelectionFailed {
            prefix: prefix.to_string(),
            failed: parsed.failed,
            failing: parsed.failing.clone(),
        });
    }
    if parsed.passed == 0 {
        return Err(SelectError::Empty {
            prefix: prefix.to_string(),
        });
    }
    Ok(())
}

/// Decides what a finished `--exact` re-run of one test means.
///
/// The named test must have been selected somewhere (otherwise the prefix
/// matched a name no harness runs alone) and must not have failed.
pub fn classify_exact(
    name: &str,
    cargo_failure: Option<String>,
    parsed: &ParsedLog,
    log_tail: &str,
) -> Result<(), SelectError> {
    if cargo_failure.is_some() && parsed.failed == 0 {
        return Err(SelectError::CargoFailed {
            context: format!("the --exact re-run of {name:?}"),
            status: cargo_failure.unwrap_or_default(),
            tail: log_tail.to_string(),
        });
    }
    if parsed.failed > 0 {
        return Err(SelectError::ExactFailed {
            name: name.to_string(),
            failed: parsed.failed,
        });
    }
    if parsed.passed == 0 {
        return Err(SelectError::ExactEmpty {
            name: name.to_string(),
        });
    }
    Ok(())
}

/// One captured `cargo test` invocation.
struct CargoRun {
    success: bool,
    status: String,
    log: String,
}

/// Runs `cargo test --workspace --locked -- <filter...> --include-ignored`
/// in `workspace_root` with colored output switched off, so the log parses the
/// same way locally and in CI.
fn run_cargo(workspace_root: &Path, filter: &[&str]) -> Result<CargoRun, SelectError> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(&cargo)
        .current_dir(workspace_root)
        .args(["test", "--workspace", "--locked", "--"])
        .args(filter)
        .args(["--include-ignored", "--color", "never"])
        .env("NO_COLOR", "1")
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .map_err(|source| SelectError::Launch {
            program: cargo.clone(),
            source,
        })?;

    let mut log = String::from_utf8_lossy(&output.stdout).into_owned();
    log.push('\n');
    log.push_str(&String::from_utf8_lossy(&output.stderr));

    Ok(CargoRun {
        success: output.status.success(),
        status: output.status.to_string(),
        log,
    })
}

/// Removes repeated names while keeping first-seen order (the same function
/// name can exist in two test targets; it is one test to re-run).
fn dedupe(names: &mut Vec<String>) {
    let mut unique: Vec<String> = Vec::with_capacity(names.len());
    for name in names.iter() {
        if !unique.iter().any(|existing| existing == name) {
            unique.push(name.clone());
        }
    }
    *names = unique;
}

/// Runs the prefix selection and returns the tests it really executed.
///
/// Fails on an empty prefix, on cargo itself failing, on any failing test and
/// — the point of the gate — on a prefix that selects nothing.
pub fn select_tests(workspace_root: &Path, prefix: &str) -> Result<Selection, SelectError> {
    if prefix.is_empty() {
        return Err(SelectError::EmptyPrefix);
    }

    let run = run_cargo(workspace_root, &[prefix])?;
    let parsed = parse_run_log(&run.log);
    let log_tail = tail(&run.log, 30);
    let cargo_failure = (!run.success).then(|| run.status.clone());
    if let Some(error) = classify_missing_harness(
        &format!("the selection run for prefix {prefix:?}"),
        cargo_failure.as_deref(),
        &parsed,
        &run.log,
    ) {
        return Err(error);
    }
    classify_selection(prefix, cargo_failure, &parsed, &log_tail)?;

    let mut tests = parsed.tests;
    dedupe(&mut tests);
    Ok(Selection {
        prefix: prefix.to_string(),
        tests,
        passed: parsed.passed,
        failed: parsed.failed,
        ignored: parsed.ignored,
        log: run.log,
    })
}

/// Re-runs every discovered test alone, with `--exact`, and requires each to
/// run and pass: a prefix must not be rescued by an unrelated test that
/// merely embeds it.
pub fn verify_exact(workspace_root: &Path, names: &[String]) -> Result<(), SelectError> {
    for name in names {
        let run = run_cargo(workspace_root, &[name, "--exact"])?;
        let parsed = parse_run_log(&run.log);
        let log_tail = tail(&run.log, 30);
        let cargo_failure = (!run.success).then(|| run.status.clone());
        if let Some(error) = classify_missing_harness(
            &format!("the --exact re-run of {name:?}"),
            cargo_failure.as_deref(),
            &parsed,
            &run.log,
        ) {
            return Err(error);
        }
        classify_exact(name, cargo_failure, &parsed, &log_tail)?;
    }
    Ok(())
}

/// The whole gate: prefix selection plus an `--exact` re-run of every name it
/// discovered.
pub fn run_gate(workspace_root: &Path, prefix: &str) -> Result<GateReport, SelectError> {
    let selection = select_tests(workspace_root, prefix)?;
    let names = selection.tests.clone();
    verify_exact(workspace_root, &names)?;
    Ok(GateReport {
        exact_verified: names.len() as u32,
        selection,
    })
}
