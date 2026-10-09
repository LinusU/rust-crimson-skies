//! Evidence-report harness for task #1134 (FLIGHT-ORIGINAL-LEVELOFF-INPUT):
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//!
//! This test is deliberately **not** named `accept_flight_original_levelop_*`:
//! it is not part of the acceptance suite and fails loudly when its inputs are
//! missing instead of passing vacuously. Run from the workspace root, after the
//! acceptance suite, exactly as:
//!
//! 1. ```sh
//!    cargo test --workspace --locked -- accept_flight_original_levelop_ \
//!      --include-ignored --nocapture 2>&1 \
//!      | tee private/evidence/T1134/cargo-test.log
//!    ```
//!    (the `--nocapture` is what puts the `PLAYTEST-LEVELOFF` measured values
//!    in the log; pass the pipeline's cargo status to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T1134 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_flight_original_levelop_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!      cargo test --locked -p cs_app --test evidence_report_flight_original_levelop -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/T1134/acceptance.json \
//!      --artifact-root private/evidence/T1134
//!    ```
//! 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/T1134.json`.
//!
//! Every field is derived from real inputs: the recorded test log, production
//! discovery of `$CS_GAME_DIR`, a fresh run of the playtest's own production
//! flight import over the installation, `rustc --version` and `Cargo.lock`.
//! Nothing is typed in by hand except the review/unknowns text, which states
//! what it is.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::playtest::retail::{RetailRequest, read_flight};
use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};

const TASK_ID: &str = "T1134";
const PREFIX: &str = "accept_flight_original_levelop_";
/// The one prefixed test that can only pass over the installation, so the
/// `retail` capability in the report is backed by a run and not by a name.
const RETAIL_PREFIX: &str = "accept_flight_original_levelop_bound_key_toggles";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_flight_original_levelop_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    // The candidate tree must be the tree that was actually tested.
    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    // The acceptance suite is the evidence: parse its recorded output.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `{PREFIX}` tests were recorded in {}",
        log_path.display()
    );
    assert!(
        suite
            .assertions
            .iter()
            .any(|(name, status)| { name.starts_with(RETAIL_PREFIX) && *status == "pass" }),
        "capability `retail` was not exercised: no {RETAIL_PREFIX} test passed in the log; \
         run step 1 with --include-ignored and CS_GAME_DIR set"
    );
    assert!(
        suite
            .assertions
            .iter()
            .any(|(name, status)| name.starts_with(PREFIX)
                && !name.starts_with(RETAIL_PREFIX)
                && *status == "pass"),
        "the synthetic (no-installation) tests must be present alongside the retail ones"
    );

    // Production discovery, by the very code the task's importer calls.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The consumer trace: the playtest's own production flight import
    // (`cs_app::playtest::retail::read_flight`) run over the installation and
    // written into the evidence directory. Its `playtest flight` statement now
    // carries `"level_off_toggle_wired":true` and the `key.l` binding — the
    // production JSON of the very wiring this task added.
    let request = RetailRequest::new(game_dir.clone(), None, None)
        .expect("the documented playtest selectors are valid");
    let flight = read_flight(&request)
        .expect("the playtest's production flight import reads the installation");
    let flight_json = flight.json();
    assert!(
        flight_json.contains("\"level_off_toggle_wired\":true"),
        "the flight statement must report the Level-Off toggle as wired: {flight_json}"
    );
    let flight_trace = format!(
        "{{\n \"task_id\": {},\n \"installation\": {},\n \"flight\": {}\n}}\n",
        jstr(TASK_ID),
        jstr(&game_dir.display().to_string()),
        flight_json
    );
    let flight_path = evidence_dir.join("playtest-flight.json");
    fs::write(&flight_path, &flight_trace)
        .unwrap_or_else(|error| panic!("write {}: {error}", flight_path.display()));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&flight_path, "json", &evidence_dir),
    ];

    let unknowns = [
        "level_off_binding_chord: the original toggles command 47 with the Shift+L chord; the \
         F22 action map has no chord sources and Left Shift is already the playtest's throttle \
         step-up, so the playtest binds the bare letter `L` — a designed slot for the original \
         command, not a reproduction of the original's chord, and the original's own default \
         binding remains unmeasured (native data, F22-H unknown #1). Affected content: the \
         retail playtest's Level-Off key. Resolving task: none needed for the toggle itself; a \
         chord-capable action map or an owner run (#358) would be needed to bind Shift+L as the \
         original does. Gates: any claim that the playtest's Level-Off key matches the original's \
         chord.",
        "level_off_calibration: the assist's torque path is part of the statically recovered law \
         (OWNER-STATIC-2026-10-08) and is still uncalibrated against an original run (#358); \
         what was measured here is that the wired toggle drives that law, not that the original \
         levels off at the same rate. Affected content: any handling claim about the Level-Off \
         assist. Resolving task: an owner original run compared against this law. Gates: any \
         verified_original claim about Level-Off behaviour.",
    ];

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": {task_id},\n\
         \x20\"candidate_tree\": {candidate_tree},\n\
         \x20\"engine\": {engine},\n\
         \x20\"created_at\": {created_at},\n\
         \x20\"command\": {{\"argv\": {argv}, \"cwd\": {cwd}, \"exit_code\": {exit_code}}},\n\
         \x20\"source\": {{\"install_sha256\": {install}, \"content_sha256\": {content}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {discovered}, \"executed\": {executed}, \"passed\": {passed}, \"failed\": {failed}, \"ignored\": {ignored}}},\n\
         \x20\"assertions\": [{assertions}],\n\
         \x20\"artifacts\": [{artifacts}],\n\
         \x20\"unknowns\": [{unknowns}],\n\
         \x20\"review\": {{\"identity\": {identity}, \"method\": {method}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        task_id = jstr(TASK_ID),
        candidate_tree = jstr(&candidate_tree),
        engine = engine_json(&engine),
        created_at = jstr(&iso_utc_now()),
        argv = str_array(&argv),
        cwd = jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code = exit_code,
        install = jstr(&install_sha256),
        content = jstr(&content_sha256),
        discovered = suite.discovered,
        executed = suite.executed,
        passed = suite.passed,
        failed = suite.failed,
        ignored = suite.ignored,
        assertions = assertion_array(&suite.assertions),
        artifacts = artifact_array(&artifacts),
        unknowns = unknowns
            .iter()
            .map(|entry| jstr(entry))
            .collect::<Vec<_>>()
            .join(", "),
        identity = jstr(
            "implementer: bunny-2 (Rally #1134 implement claim of 2026-10-09); reviewer: \
             recorded by the Rally review claim that follows — per AGENTS.md an agent review is \
             `checked` at best, is never independent original-reference evidence and never \
             replaces the owner's human approval. The activity log records both claims.",
        ),
        method = jstr(
            "acceptance suite run locally with the retail capability and recorded verbatim in \
             cargo-test.log (its PLAYTEST-LEVELOFF lines carry the measured bank angles); this \
             harness derives every field from that log, from production discovery of \
             $CS_GAME_DIR, from a fresh run of the playtest's own production flight import \
             (playtest-flight.json, `cs_app::playtest::retail::read_flight`, whose flight \
             statement now reports level_off_toggle_wired true), from rustc and from Cargo.lock. \
             Structure checked with tools/validate_evidence.py; the two unknowns it carries are \
             the unmeasured original chord and the uncalibrated assist rate, each stated with \
             what it gates.",
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        &format!("\"task_id\": \"{TASK_ID}\""),
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_flight_original_levelop.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/T1134` written relative to the
/// workspace root in the module doc must be re-anchored here.
fn workspace_path(as_described: &str) -> PathBuf {
    let path = PathBuf::from(as_described);
    if path.is_absolute() {
        return path;
    }
    Path::new(&git(&["rev-parse", "--show-toplevel"])).join(path)
}

fn git(args: &[&str]) -> String {
    let output = Command::new("git").args(args).output().expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn rustc_version() -> String {
    let output = Command::new("rustc")
        .arg("--version")
        .output()
        .expect("rustc runs");
    assert!(output.status.success(), "rustc --version failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// The locked version of one `Cargo.lock` package: read, never asserted from
/// memory.
fn locked_version(package: &str) -> String {
    let lock_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .join("Cargo.lock");
    let lock = fs::read_to_string(&lock_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", lock_path.display()));
    let mut wanted = false;
    for line in lock.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            wanted = false;
        } else if let Some(name) = line.strip_prefix("name = \"") {
            wanted = name.trim_end_matches('"') == package;
        } else if let Some(version) = line.strip_prefix("version = \"")
            && wanted
        {
            return version.trim_end_matches('"').to_owned();
        }
    }
    panic!("package {package:?} is not in {}", lock_path.display());
}

// ---------------------------------------------------------- log parsing ---

/// What the recorded `cargo test` output says actually happened.
#[derive(Debug, Default)]
struct Suite {
    discovered: u64,
    executed: u64,
    passed: u64,
    failed: u64,
    ignored: u64,
    /// `(test name, "pass" | "fail" | "unknown")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the libtest summaries and the per-test results of the
/// `accept_flight_original_levelop_` tests from a recorded `cargo test` output.
fn parse_suite(log: &str) -> Suite {
    let mut suite = Suite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("test result:") {
            for (count, kind) in summary_fields(trimmed) {
                match kind {
                    "passed" => suite.passed += count,
                    "failed" => suite.failed += count,
                    "ignored" => suite.ignored += count,
                    _ => {}
                }
            }
            continue;
        }
        if pending.front().is_some() {
            if trimmed == "ok" {
                let name = pending.pop_front().expect("pending test");
                record(&mut suite, name, "pass");
                continue;
            }
            if trimmed == "FAILED" {
                let name = pending.pop_front().expect("pending test");
                record(&mut suite, name, "fail");
                continue;
            }
        }
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let name = after[..separator].to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
            if !name.starts_with(PREFIX) {
                continue;
            }
            match tail.split_whitespace().next() {
                Some("ok") => record(&mut suite, name, "pass"),
                Some("FAILED") => record(&mut suite, name, "fail"),
                _ => pending.push_back(name),
            }
        }
    }
    suite.assertions.dedup_by(|left, right| left.0 == right.0);
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// `(count, kind)` pairs of one `test result:` summary line.
fn summary_fields(line: &str) -> Vec<(u64, &'static str)> {
    let mut fields = Vec::new();
    for segment in line["test result:".len()..].split(';') {
        let words: Vec<&str> = segment.split_whitespace().collect();
        for pair in words.windows(2) {
            let Ok(count) = pair[0].parse::<u64>() else {
                continue;
            };
            let kind = match pair[1] {
                "passed" => "passed",
                "failed" => "failed",
                "ignored" => "ignored",
                _ => continue,
            };
            fields.push((count, kind));
            break;
        }
    }
    fields
}

fn record(suite: &mut Suite, name: String, status: &'static str) {
    if suite.assertions.iter().any(|(seen, _)| *seen == name) {
        return;
    }
    suite.assertions.push((name, status));
}

// ------------------------------------------------------------- artifacts ---

/// One referenced artifact: hashed here with the production SHA-256 the
/// workspace implements (the validator re-hashes it with `hashlib`
/// independently).
fn artifact(source: &Path, kind: &str, evidence_dir: &Path) -> (String, String, String) {
    let name = source
        .file_name()
        .expect("artifact has a file name")
        .to_string_lossy()
        .into_owned();
    let target = evidence_dir.join(&name);
    if source != target {
        fs::copy(source, &target).unwrap_or_else(|error| {
            panic!("copy {} -> {}: {error}", source.display(), target.display())
        });
    }
    let bytes =
        fs::read(&target).unwrap_or_else(|error| panic!("read {}: {error}", target.display()));
    (name, sha256(&bytes).to_hex(), kind.to_owned())
}

// ------------------------------------------------------------- rendering ---

struct Engine {
    rust: String,
    bevy: String,
    avian: String,
}

fn engine_json(engine: &Engine) -> String {
    format!(
        "{{\"rust\": {}, \"bevy\": {}, \"avian\": {}}}",
        jstr(&engine.rust),
        jstr(&engine.bevy),
        jstr(&engine.avian)
    )
}

fn assertion_array(assertions: &[(String, &'static str)]) -> String {
    let items: Vec<String> = assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\"]}}",
                jstr(name)
            )
        })
        .collect();
    items.join(", ")
}

fn artifact_array(artifacts: &[(String, String, String)]) -> String {
    let items: Vec<String> = artifacts
        .iter()
        .map(|(name, digest, kind)| {
            format!(
                "{{\"path\": {}, \"sha256\": {digest:?}, \"kind\": {kind:?}}}",
                jstr(name)
            )
        })
        .collect();
    items.join(", ")
}

fn str_array(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| jstr(item)).collect();
    format!("[{}]", quoted.join(", "))
}

/// A JSON string literal: quoted and escaped, so no report field can break
/// out of its string.
fn jstr(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// RFC 3339 with whole seconds and `Z`, which `datetime.fromisoformat`
/// accepts after the validator's `Z` → `+00:00` replacement.
fn iso_utc_now() -> String {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after 1970")
        .as_secs() as i64;
    let (year, month, day, hour, minute, second) = civil_from_unix(epoch);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a UTC
/// calendar day, because `std` has no date formatting.
fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year_of_day = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = (if month_prime < 10 {
        month_prime + 3
    } else {
        (month_prime - 10) / 12 + 1
    }) as u32;
    let year = if month <= 2 {
        year_of_day - 1
    } else {
        year_of_day
    };
    let second = (rest % 60) as u32;
    let minute = ((rest / 60) % 60) as u32;
    let hour = (rest / 3600) as u32;
    (year, month, day, hour, minute, second)
}
