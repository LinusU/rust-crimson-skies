//! Acceptance tests for F00-SEED: `--seed <u64>` as the root seed of a
//! headless synthetic run (contract `docs/contracts/CLI-EVIDENCE.md`,
//! section "`--seed`").
//!
//! Every claim of the contract is checked from the production side: the real
//! `cs` binary and the production parser `cs_app::cli::parse`, plus the
//! production runner `cs_app::run::run_synthetic`. The failure cases are what
//! make this a test of an implementation rather than of a stub — an
//! implementation that parses `--seed` and then ignores it produces one
//! identical trace for every seed and fails the divergence cases below, and
//! one that accepts unusable seed input fails the rejection cases.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use cs_app::cli::{self, CliRequest};
use cs_app::run::{self, SyntheticRequest};

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

/// Runs the real binary once and returns its exit code, stdout and stderr.
fn run_binary<I, S>(workdir: &std::path::Path, raw: I) -> (Option<i32>, String, String)
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = cs()
        .args(raw)
        .env_remove("CS_GAME_DIR")
        .current_dir(workdir)
        .output()
        .expect("the cs binary must run without a GPU or retail installation");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// The typed request the production parser must build for a seeded run.
fn seeded_request(raw: &[&str]) -> SyntheticRequest {
    match cli::parse(args(raw)) {
        CliRequest::Synthetic(request) => request,
        other => panic!("expected a synthetic request for {raw:?}, got {other:?}"),
    }
}

/// The CLI contract of `--seed`: it parses into `SyntheticRequest::seed`, the
/// later duplicate wins, a non-`u64` value and `--seed` without `--synthetic`
/// are `Invalid` naming the flag, a missing value is `Unsupported`, and
/// `--help` documents the flag.
///
/// Observable failure if the implementation is removed: `--seed` is then an
/// unknown argument and every one of these vectors becomes `Unsupported`.
#[test]
fn accept_t335_cli_parses_the_seed_and_rejects_unusable_forms() {
    let request = seeded_request(&["--synthetic", "--headless", "--ticks", "600", "--seed", "1"]);
    assert_eq!(
        request.seed,
        Some(1),
        "--seed 1 must parse into the request"
    );
    assert_eq!(request.ticks, 600, "the rest of the request must survive");

    let unseeded = seeded_request(&["--synthetic", "--headless", "--ticks", "600"]);
    assert_eq!(
        unseeded.seed, None,
        "an omitted --seed must stay None so the canonical fixture runs"
    );

    let reseeded = seeded_request(&[
        "--seed",
        "1",
        "--synthetic",
        "--headless",
        "--ticks",
        "10",
        "--seed",
        "7",
    ]);
    assert_eq!(
        reseeded.seed,
        Some(7),
        "of duplicates the later --seed must win, and flag order must not matter"
    );

    // A value that is not a u64 is invalid input naming --seed.
    for value in ["abc", "-1", "1.5", ""] {
        match cli::parse(args(&[
            "--synthetic",
            "--headless",
            "--ticks",
            "10",
            "--seed",
            value,
        ])) {
            CliRequest::Invalid { reason } => assert!(
                reason.contains("--seed"),
                "the reason for --seed {value:?} must name the flag, got: {reason}"
            ),
            other => panic!("--seed {value:?} must be Invalid, got {other:?}"),
        }
    }

    // `--seed` without `--synthetic` has no consumer yet.
    match cli::parse(args(&["--headless", "--ticks", "10", "--seed", "1"])) {
        CliRequest::Invalid { reason } => assert!(
            reason.contains("--seed"),
            "the reason must name --seed, got: {reason}"
        ),
        other => panic!("--seed without --synthetic must be Invalid, got {other:?}"),
    }

    // A flag whose value is missing names no request at all.
    let missing_value = args(&["--synthetic", "--headless", "--ticks", "10", "--seed"]);
    assert_eq!(
        cli::parse(missing_value.clone()),
        CliRequest::Unsupported {
            args: missing_value
        },
        "--seed without a value must be Unsupported, mirroring --ticks"
    );
}

/// The same contract observed on the real binary: unusable seed input exits 2
/// with a `cs:` diagnostic naming `--seed`, writes no trace and reports no
/// finished run, while `--help` documents the flag.
#[test]
fn accept_t335_binary_rejects_bad_seed_requests_and_help_documents_the_flag() {
    let workdir = empty_scratch_dir("accept_t335_seed_rejections");

    let cases: [(&[&str], &str); 3] = [
        // not a u64
        (
            &[
                "--trace",
                "T",
                "--synthetic",
                "--headless",
                "--ticks",
                "10",
                "--seed",
                "abc",
            ],
            "--seed",
        ),
        // no consumer for the seed
        (
            &["--trace", "T", "--headless", "--ticks", "10", "--seed", "1"],
            "--seed",
        ),
        // value missing entirely
        (
            &[
                "--trace",
                "T",
                "--synthetic",
                "--headless",
                "--ticks",
                "10",
                "--seed",
            ],
            "--seed",
        ),
    ];

    for (index, (raw, needle)) in cases.into_iter().enumerate() {
        let trace = workdir.join(format!("invalid-{index}.jsonl"));
        let request: Vec<String> = raw
            .iter()
            .map(|arg| {
                if *arg == "T" {
                    trace.display().to_string()
                } else {
                    (*arg).to_string()
                }
            })
            .collect();

        let (code, stdout, stderr) = run_binary(&workdir, &request);
        assert_eq!(
            code,
            Some(i32::from(cli::EXIT_INVALID_INPUT)),
            "case {index} must exit 2, got {code:?}; stderr: {stderr}"
        );
        assert!(
            stderr.contains("cs:") && stderr.contains(needle),
            "case {index}: the diagnostic must name the binary and {needle}, got: {stderr:?}"
        );
        assert!(
            !stdout.contains("ended at tick"),
            "case {index}: no run may be reported as finished, got: {stdout:?}"
        );
        assert!(
            !trace.exists(),
            "case {index}: an invalid request must not create {}",
            trace.display()
        );
    }

    let (code, stdout, stderr) = run_binary(&workdir, ["--help"]);
    assert_eq!(code, Some(0), "--help must exit zero; stderr: {stderr}");
    assert!(
        stdout.contains("--seed"),
        "--help must document --seed, got: {stdout:?}"
    );
    assert!(
        stderr.is_empty(),
        "--help must stay silent on stderr, got: {stderr:?}"
    );
}

/// The core determinism contract: one seed reproduces its trace byte for
/// byte, another seed does not, and the difference is already visible in the
/// tick-0 sample — so a parser that drops the seed on the floor fails here.
#[test]
fn accept_t335_same_seed_reproduces_the_trace_and_other_seeds_diverge() {
    let workdir = empty_scratch_dir("accept_t335_seed_divergence");

    let first = workdir.join("first.jsonl");
    let again = workdir.join("again.jsonl");
    let other = workdir.join("other.jsonl");

    for (trace, seed) in [(&first, "12345"), (&again, "12345"), (&other, "999")] {
        let trace_arg = trace.display().to_string();
        let (code, _stdout, stderr) = run_binary(
            &workdir,
            [
                "--synthetic",
                "--headless",
                "--ticks",
                "120",
                "--seed",
                seed,
                "--trace",
                &trace_arg,
            ],
        );
        assert_eq!(code, Some(0), "a seeded run must succeed; stderr: {stderr}");
    }

    let first_bytes = fs::read(&first).expect("the first seeded trace must exist");
    let again_bytes = fs::read(&again).expect("the second seeded trace must exist");
    let other_bytes = fs::read(&other).expect("the other seeded trace must exist");

    assert_eq!(
        first_bytes, again_bytes,
        "the same seed must give identical trace bytes"
    );
    assert_ne!(
        first_bytes, other_bytes,
        "different seeds must give different traces; an ignored --seed fails here"
    );

    let first_text = String::from_utf8_lossy(&first_bytes).into_owned();
    let other_text = String::from_utf8_lossy(&other_bytes).into_owned();
    let first_lines: Vec<&str> = first_text.lines().collect();
    let other_lines: Vec<&str> = other_text.lines().collect();
    assert_ne!(
        first_lines[1],
        other_lines[1],
        "different seeds must already differ in the tick-0 sample, got the \
         same {sample:?}",
        sample = first_lines[1]
    );
    assert!(
        first_lines[0].contains("\"seed\":12345"),
        "the header must state the seed, got: {}",
        first_lines[0]
    );
    assert!(
        other_lines[0].contains("\"seed\":999"),
        "the header must state the seed, got: {}",
        other_lines[0]
    );
}

/// The typed runner, where a seed that is parsed but ignored would make all
/// three seeded runs identical to the canonical fixture.
#[test]
fn accept_t335_different_seeds_change_the_tick_zero_sample() {
    let sample = |seed| {
        run::run_synthetic(&SyntheticRequest {
            ticks: 0,
            trace: None,
            seed,
        })
        .expect("a seeded run of a valid fixture must succeed")
        .final_sample
    };

    let canonical = sample(None);
    assert_eq!(
        canonical.position_m,
        [0.0, 10.0, 0.0],
        "without a seed the canonical fixture must start untouched"
    );
    assert_eq!(
        canonical.linear_velocity_m_s,
        [0.0, 0.0, 0.0],
        "without a seed the canonical fixture starts at rest"
    );

    let mut lateral_positions = Vec::new();
    for root_seed in [0_u64, 1, 2, 12_345] {
        let seeded = sample(Some(root_seed));
        assert_eq!(
            seeded.tick, canonical.tick,
            "seed {root_seed}: the run must still start at tick 0"
        );
        assert_eq!(
            seeded.position_m[1], canonical.position_m[1],
            "seed {root_seed}: the drop height must not move"
        );
        assert_eq!(
            seeded.linear_velocity_m_s[1], canonical.linear_velocity_m_s[1],
            "seed {root_seed}: the vertical velocity must not move"
        );
        assert_ne!(
            seeded.position_m, canonical.position_m,
            "seed {root_seed}: a parsed seed must move the body, otherwise it \
             is being ignored"
        );
        assert!(
            seeded.position_m[0].abs() <= 2.0 && seeded.position_m[2].abs() <= 2.0,
            "seed {root_seed}: the lateral position must stay within +/- 2 m, \
             got {:?}",
            seeded.position_m
        );
        assert!(
            seeded.linear_velocity_m_s[0].abs() <= 4.0
                && seeded.linear_velocity_m_s[2].abs() <= 4.0,
            "seed {root_seed}: the lateral velocity must stay within +/- 4 m/s, \
             got {:?}",
            seeded.linear_velocity_m_s
        );
        assert_ne!(
            seeded, canonical,
            "seed {root_seed}: the seeded sample must differ from the canonical one"
        );
        lateral_positions.push(seeded.position_m);
    }
    let unique: Vec<_> = lateral_positions.iter().collect();
    for (index, left) in unique.iter().enumerate() {
        for right in &unique[index + 1..] {
            assert_ne!(
                left, right,
                "different root seeds must land the body in different places"
            );
        }
    }
}

/// No seed means the unchanged canonical trace: the F00-C smoke expectations
/// still hold (header, one record per tick, the fixture's untouched tick-0
/// sample, a body that really integrated) and the header now says
/// `"seed": null`. Two unseeded runs stay byte-identical.
#[test]
fn accept_t335_without_a_seed_the_trace_stays_the_canonical_fixture() {
    let workdir = empty_scratch_dir("accept_t335_canonical_trace");

    let mut traces = Vec::new();
    for run_index in 0..2 {
        let trace = workdir.join(format!("canonical-{run_index}.jsonl"));
        let (code, stdout, stderr) = run_binary(
            &workdir,
            [
                "--synthetic",
                "--headless",
                "--ticks",
                "120",
                "--trace",
                &trace.display().to_string(),
            ],
        );
        assert_eq!(
            code,
            Some(0),
            "run {run_index} must succeed; stderr: {stderr}"
        );
        assert!(
            stdout.contains("120/120") && stdout.contains("SYNTHETIC"),
            "run {run_index} must report the completed run, got: {stdout:?}"
        );
        traces.push(
            fs::read(&trace)
                .unwrap_or_else(|error| panic!("run {run_index} must write its trace: {error}")),
        );
    }
    assert_eq!(
        traces[0], traces[1],
        "the canonical trace must be reproducible byte for byte"
    );

    let text = String::from_utf8_lossy(&traces[0]).into_owned();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines.len(),
        122,
        "header plus one record per tick 0..=120, got {} lines",
        lines.len()
    );
    assert!(
        lines[0].contains("\"kind\":\"synthetic-headless\"")
            && lines[0].contains("\"provenance\":\"SYNTHETIC\"")
            && lines[0].contains("\"requested_ticks\":120"),
        "the header must still declare the trace, got: {}",
        lines[0]
    );
    assert!(
        lines[0].contains("\"seed\":null"),
        "an unseeded trace must state that it has no seed, got: {}",
        lines[0]
    );
    assert_eq!(
        lines[1], "{\"tick\":0,\"position_m\":[0,10,0],\"linear_velocity_m_s\":[0,0,0]}",
        "the canonical fixture must keep its untouched tick-0 sample"
    );
    for (tick, line) in lines[1..].iter().enumerate() {
        assert!(
            line.starts_with(&format!("{{\"tick\":{tick},")),
            "record {tick} must be tick {tick}, got: {line}"
        );
    }
    let last = lines[121];
    assert!(last.starts_with("{\"tick\":120,"), "got: {last}");
    assert!(
        last.contains("\"position_m\":["),
        "the last record must carry a body sample, got: {last}"
    );
    // F00-C's own smoke expectation: after 120 ticks the body really fell.
    let y_after = last
        .split("\"position_m\":[")
        .nth(1)
        .and_then(|rest| rest.split(',').next())
        .expect("the final position must be readable")
        .parse::<f32>()
        .expect("the final y must be a number");
    assert!(
        y_after < 9.0,
        "the canonical body must really have integrated for 120 ticks, y = {y_after}"
    );
}

/// The trace header is the proof that the flag was honoured: it carries the
/// seed the run started from, or `null` when there was none.
#[test]
fn accept_t335_the_trace_header_records_the_seed() {
    let workdir = empty_scratch_dir("accept_t335_trace_header");

    let header_of = |raw: &[&str]| -> String {
        let trace = workdir.join("header.jsonl");
        let mut request: Vec<String> = raw.iter().map(|arg| (*arg).to_string()).collect();
        request.push("--trace".to_string());
        request.push(trace.display().to_string());
        let (code, _stdout, stderr) = run_binary(&workdir, &request);
        assert_eq!(code, Some(0), "the run must succeed; stderr: {stderr}");
        fs::read_to_string(&trace)
            .expect("the run must write its trace")
            .lines()
            .next()
            .expect("the trace must start with a header")
            .to_string()
    };

    let seeded = header_of(&[
        "--synthetic",
        "--headless",
        "--ticks",
        "2",
        "--seed",
        "424242",
    ]);
    assert!(
        seeded.contains("\"seed\":424242"),
        "the header must carry the seed, got: {seeded}"
    );

    let unseeded = header_of(&["--synthetic", "--headless", "--ticks", "2"]);
    assert!(
        unseeded.contains("\"seed\":null"),
        "the header must carry null when no seed was given, got: {unseeded}"
    );
    assert_ne!(
        seeded, unseeded,
        "a seeded and an unseeded trace must be distinguishable by their headers"
    );
}
