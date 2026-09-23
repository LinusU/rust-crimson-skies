//! Acceptance scenario F00-C / AC03: run a fixed-tick synthetic smoke twice;
//! both end at the requested tick count.
//!
//! The scenario is checked from both ends of the production path: the typed
//! runner (`cs_app::run::run_synthetic`) is called twice in one process, and
//! the real `cs` binary is executed twice with `--synthetic --headless
//! --ticks … --trace …`. Removing the run mode, mis-counting ticks, losing
//! the world's own tick counter or swallowing a trace failure all fail these
//! tests; so does an implementation that always exits zero, because the
//! failure cases below expect exit 2 (invalid input) and exit 1 (runtime
//! failure) with no trace left behind.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use cs_app::cli::{self, CliRequest};
use cs_app::run::{self, SyntheticRequest};
use cs_types::{SceneProvenance, Tick};

/// A fresh, empty directory inside the workspace `target/`, so each test owns
/// the files it asserts about.
fn empty_scratch_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("{name}_{}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("the scratch dir must be reusable");
    }
    fs::create_dir_all(&dir).expect("the scratch dir must be creatable");
    dir
}

fn cs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cs"))
}

fn args(raw: &[&str]) -> Vec<String> {
    raw.iter().map(|arg| (*arg).to_string()).collect()
}

/// AC03 core: two independent runs of the production runner, both ending at
/// the requested tick count.
///
/// Observable failure if the implementation is removed: there is no
/// `run_synthetic` to call; if tick counting is broken, `ticks_completed` or
/// the world's own `final_sample.tick` stops short of 600 on at least one of
/// the two runs.
#[test]
fn accept_f00_c_fixed_tick_smoke_twice_ends_at_the_requested_tick_count() {
    let first = run::run_synthetic(&SyntheticRequest {
        ticks: 600,
        trace: None,
    })
    .expect("the first synthetic smoke run must succeed");
    let second = run::run_synthetic(&SyntheticRequest {
        ticks: 600,
        trace: None,
    })
    .expect("the second synthetic smoke run must succeed");

    for (index, report) in [&first, &second].into_iter().enumerate() {
        assert_eq!(
            report.requested_ticks, 600,
            "run {index} must record what it was asked for"
        );
        assert_eq!(
            report.ticks_completed, 600,
            "run {index} must end at the requested tick count"
        );
        assert_eq!(
            report.final_sample.tick,
            Tick(600),
            "run {index}: the world's own tick counter must reach 600"
        );
        assert_eq!(
            report.provenance,
            SceneProvenance::Synthetic,
            "run {index}: the scene must stay marked SYNTHETIC"
        );
        assert!(
            report.final_sample.position_m[1] < 9.0,
            "run {index}: the body must really have integrated for 600 ticks, y = {}",
            report.final_sample.position_m[1]
        );
        assert!(
            report.final_sample.linear_velocity_m_s[1] < -1.0,
            "run {index}: a falling body must accumulate velocity, got {}",
            report.final_sample.linear_velocity_m_s[1]
        );
        assert!(
            report.trace.is_none(),
            "run {index}: no trace was requested, none may be reported"
        );
    }
}

/// AC03 end to end: the `cs` binary runs the headless smoke twice, each run
/// ending at tick 600 and writing one trace record per tick plus a header.
#[test]
fn accept_f00_c_headless_smoke_runs_twice_and_traces_every_tick() {
    let workdir = empty_scratch_dir("accept_f00_c_smoke_twice");

    for run_index in 0..2 {
        let trace = workdir.join(format!("trace-{run_index}.jsonl"));
        let output = cs()
            .args(["--synthetic", "--headless", "--ticks", "600", "--trace"])
            .arg(&trace)
            .env_remove("CS_GAME_DIR")
            .current_dir(&workdir)
            .output()
            .expect("the cs binary must run without a GPU or retail installation");

        assert_eq!(
            output.status.code(),
            Some(0),
            "run {run_index} must exit zero; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("600/600") && stdout.contains("SYNTHETIC"),
            "run {run_index} must report the completed tick count and the \
 provenance, got: {stdout:?}"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).is_empty(),
            "a successful run must stay silent on stderr, got: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let text = fs::read_to_string(&trace)
            .unwrap_or_else(|error| panic!("run {run_index} must write its trace: {error}"));
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines.len(),
            602,
            "run {run_index}: header plus one record per tick 0..=600, got {} lines",
            lines.len()
        );
        assert!(
            lines[0].contains("\"provenance\":\"SYNTHETIC\"")
                && lines[0].contains("\"requested_ticks\":600")
                && lines[0].contains("\"kind\":\"synthetic-headless\""),
            "run {run_index}: the header must declare the trace, got: {}",
            lines[0]
        );
        for (tick, line) in lines[1..].iter().enumerate() {
            assert!(
                line.starts_with(&format!("{{\"tick\":{tick},")),
                "run {run_index}: record {tick} must be tick {tick}, got: {line}"
            );
        }
        let last = lines[601];
        assert!(
            last.starts_with("{\"tick\":600,") && last.contains("\"position_m\":["),
            "run {run_index}: the last record must be the final tick with its \
 body sample, got: {last}"
        );
    }
}

/// Failure cases of the same contract: unusable requests must exit 2 with a
/// diagnostic naming the offending flag, and must leave no trace behind — an
/// implementation that always exited zero (or started writing before
/// validating) fails here.
#[test]
fn accept_f00_c_invalid_synthetic_requests_exit_two_without_writing_a_trace() {
    let workdir = empty_scratch_dir("accept_f00_c_invalid_requests");

    let cases: [(&[&str], &str); 5] = [
        // non-numeric tick count
        (
            &[
                "--trace",
                "T",
                "--synthetic",
                "--headless",
                "--ticks",
                "abc",
            ],
            "--ticks",
        ),
        // no tick count at all
        (&["--trace", "T", "--synthetic", "--headless"], "--ticks"),
        // windowed run: no renderer exists to show it
        (
            &["--trace", "T", "--synthetic", "--ticks", "600"],
            "--headless",
        ),
        // incomplete vector: `--ticks` is the last thing typed
        (
            &["--trace", "T", "--synthetic", "--headless", "--ticks"],
            "--ticks",
        ),
        // `--headless`/`--ticks` without `--synthetic`
        (
            &["--trace", "T", "--headless", "--ticks", "600"],
            "--synthetic",
        ),
    ];

    for (index, (raw, needle)) in cases.into_iter().enumerate() {
        let trace = workdir.join(format!("invalid-{index}.jsonl"));
        let mut request = Vec::new();
        for arg in raw {
            request.push(if arg == &"T" {
                trace.display().to_string()
            } else {
                (*arg).to_string()
            });
        }

        let output = cs()
            .args(&request)
            .env_remove("CS_GAME_DIR")
            .current_dir(&workdir)
            .output()
            .expect("the cs binary must run without a GPU or retail installation");

        assert_eq!(
            output.status.code(),
            Some(i32::from(cli::EXIT_INVALID_INPUT)),
            "case {index} must exit 2, got {:?}",
            output.status.code()
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("cs:") && stderr.contains(needle),
            "case {index}: the diagnostic must name the binary and {needle}, got: {stderr:?}"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            !stdout.contains("ended at tick"),
            "case {index}: no run may be reported as finished, got: {stdout:?}"
        );
        assert!(
            !trace.exists(),
            "case {index}: an invalid request must not create {}: {request:?}",
            trace.display()
        );
    }
}

/// Failure case for the runtime path: a trace that cannot be opened is a
/// runtime failure (exit 1), reported with the path and never as success.
#[test]
fn accept_f00_c_an_unwritable_trace_is_a_runtime_failure() {
    let workdir = empty_scratch_dir("accept_f00_c_bad_trace");
    let trace = workdir.join("no-such-directory").join("trace.jsonl");

    let output = cs()
        .args(["--synthetic", "--headless", "--ticks", "10", "--trace"])
        .arg(&trace)
        .env_remove("CS_GAME_DIR")
        .current_dir(&workdir)
        .output()
        .expect("the cs binary must run without a GPU or retail installation");

    assert_ne!(
        output.status.code(),
        Some(0),
        "a run that could not write its trace must not exit zero"
    );
    assert_eq!(
        output.status.code(),
        Some(i32::from(cli::EXIT_RUNTIME_FAILURE)),
        "a runtime failure uses the contract's runtime exit code"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cs:") && stderr.contains("no-such-directory"),
        "the failure must name the binary and the trace path, got: {stderr:?}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("ended at tick"),
        "the run must not report a finished run, got: {stdout:?}"
    );
    assert!(
        !trace.exists(),
        "nothing may be left at {}",
        trace.display()
    );
}

/// The production parser: a complete request becomes a typed
/// [`SyntheticRequest`], flags may appear in any order, `--help` still wins,
/// unknown input stays unsupported and a recognizable-but-unusable request
/// names the flag that broke it.
#[test]
fn accept_f00_c_parser_builds_the_typed_synthetic_request() {
    match cli::parse(args(&[
        "--synthetic",
        "--headless",
        "--ticks",
        "600",
        "--trace",
        "private/trace.jsonl",
    ])) {
        CliRequest::Synthetic(request) => {
            assert_eq!(request.ticks, 600, "the tick count must be parsed");
            assert_eq!(
                request.trace,
                Some(PathBuf::from("private/trace.jsonl")),
                "the trace path must be parsed"
            );
        }
        other => panic!("expected a synthetic request, got {other:?}"),
    }

    match cli::parse(args(&[
        "--trace",
        "t.jsonl",
        "--ticks",
        "12",
        "--headless",
        "--synthetic",
    ])) {
        CliRequest::Synthetic(request) => {
            assert_eq!(request.ticks, 12, "flag order must not matter");
            assert_eq!(request.trace, Some(PathBuf::from("t.jsonl")));
        }
        other => panic!("expected a synthetic request, got {other:?}"),
    }

    assert_eq!(
        cli::parse(args(&[
            "--synthetic",
            "--headless",
            "--ticks",
            "10",
            "--help"
        ])),
        CliRequest::Help,
        "--help must keep winning over any run mode"
    );

    // The F00-B contract: an incomplete vector is unsupported input, still
    // reported by name rather than accepted as a run.
    let unknown = vec!["--synthetic".to_string(), "--ticks".to_string()];
    assert_eq!(
        cli::parse(unknown.clone()),
        CliRequest::Unsupported { args: unknown }
    );

    match cli::parse(args(&["--synthetic", "--ticks", "600"])) {
        CliRequest::Invalid { reason } => assert!(
            reason.contains("--headless"),
            "the reason must name the missing flag, got: {reason}"
        ),
        other => panic!("a windowed run must be rejected, got {other:?}"),
    }

    match cli::parse(args(&["--synthetic", "--headless", "--ticks", "-5"])) {
        CliRequest::Invalid { reason } => assert!(
            reason.contains("--ticks") && reason.contains("-5"),
            "the reason must name the flag and the value, got: {reason}"
        ),
        other => panic!("a negative tick count must be rejected, got {other:?}"),
    }
}

/// `--help` documents the run mode that really exists, and keeps the
/// guarantees F00-B relies on (usage on stdout, no diagnostic text).
#[test]
fn accept_f00_c_help_documents_the_synthetic_run_mode() {
    let output = cs()
        .arg("--help")
        .env_remove("CS_GAME_DIR")
        .output()
        .expect("the cs binary must run without a GPU or retail installation");

    assert_eq!(
        output.status.code(),
        Some(0),
        "--help must exit zero, got {:?}",
        output.status.code()
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("USAGE")
            && stdout.contains("--synthetic")
            && stdout.contains("--headless")
            && stdout.contains("--ticks")
            && stdout.contains("--trace"),
        "--help must document the synthetic smoke, got: {stdout:?}"
    );
    assert!(
        !stdout.contains("cs:") && !stdout.contains("no run modes yet"),
        "--help must not print a diagnostic, got: {stdout:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).is_empty(),
        "--help must stay silent on stderr"
    );
}
