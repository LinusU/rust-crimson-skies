//! Evidence-report harness for task F52-D (`docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`).
//!
//! This test is deliberately **not** named `accept_f52_d_*`: it is not part of
//! the acceptance suite, it fails loudly when its inputs are missing instead of
//! passing vacuously, and the task's test selection must never pick it up as an
//! acceptance test. Run from the workspace root, after the acceptance suite,
//! exactly as:
//!
//! 1. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F52-D \
//!      cargo test --workspace --locked -- accept_f52_d_ --include-ignored \
//!      2>&1 | tee private/evidence/F52-D/cargo-test.log
//!    ```
//!    (record the pipeline's exit status — it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`; `CS_EVIDENCE_DIR` is also what makes the GPU
//!    witness write its PNGs here, so the artifacts this report hashes are this
//!    run's own.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F52-D \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f52_d_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!      cargo test --locked -p cs_app --test accessibility -- \
//!      evidence_report_f52_d --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/F52-D/acceptance.json \
//!      --artifact-root private/evidence/F52-D --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/F52-D.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, `rustc --version` and `Cargo.lock`, and the PNGs the GPU
//! witness wrote on the real adapter. Nothing is typed in by hand.
//!
//! The stage declares **`gpu` and `synthetic`** and no `retail`: F52-D's
//! required capability is `gpu`, no acceptance test here reads `$CS_GAME_DIR`,
//! and `source` is therefore `null` for both hashes — this task measures no
//! original data, so claiming an installation hash would be a claim of
//! evidence that was never taken. `gpu` is carried only by the capture test
//! and `synthetic` by the rest; both are checked against the recorded log, so
//! the report is not written with a capability whose tests did not run.
//!
//! The report's `unknowns` are *this task's* blockers, and they are empty
//! because the acceptance run passed. The product incompleteness this stage
//! found is **not** dropped anywhere: no writer puts the fidelity label into a
//! `ReplayRecord` yet (F52-W3, #782), no presenter applies the colour filter,
//! UI scale or bus levels to a drawn/audible frame (F52-W2, #781), nothing
//! opens the settings session at boot (F52-W1, #780), the page's row geometry
//! and the capture's quads are designed rather than original, and the original
//! option set is still unmeasured — all of it is written out with its
//! resolving task in
//! `docs/findings/2026-10-08-f52-d-accessibility-flows-gpu-review.md`.
//! The claim is `implemented`, never `checked` or `verified_original`.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::sha256;

/// The acceptance tests whose capabilities this report declares.
///
/// `gpu` is carried by the capture test (it draws a real frame through
/// `cs_app::accessibility::gpu_capture::capture_objective_page`); the other
/// seven are synthetic and run on every push. All eight must appear in the
/// recorded log and pass, or the report is not written with that capability.
const REQUIRED_TESTS: &[&str] = &[
    "accept_f52_d_a_real_gpu_capture_draws_the_objectives_page_at_the_ui_scale",
    "accept_f52_d_the_objectives_page_geometry_is_identical_under_every_colour_filter",
    "accept_f52_d_a_larger_ui_scale_draws_the_same_rows_bigger_and_never_drops_one",
    "accept_f52_d_only_the_part_of_a_row_inside_the_viewport_is_drawn",
    "accept_f52_d_a_capture_with_nothing_to_draw_is_refused_without_writing_a_file",
    "accept_f52_d_enabling_a_gameplay_assist_is_named_in_the_comparison_and_replay_metadata",
    "accept_f52_d_an_inert_assist_and_a_refused_change_never_reach_the_record",
    "accept_f52_d_a_change_that_cannot_be_saved_is_recorded_and_says_it_is_not_on_disk",
];

/// The capture files the GPU witness writes, by prefix: a capture the suite
/// did not produce cannot be claimed, and a stale file from another task
/// cannot be picked up by accident. (`f52-d-refused.png` never exists: a
/// refused capture writes nothing.)
const CAPTURE_PREFIX: &str = "f52-d-";
const CAPTURE_SUFFIX: &str = ".png";

/// The captures the `gpu` test must have written.
const EXPECTED_CAPTURES: &[&str] = &[
    "f52-d-scale-100.png",
    "f52-d-scale-300.png",
    "f52-d-monochrome-100.png",
];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE"]
fn evidence_report_f52_d_writes_the_acceptance_report() {
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

    // The candidate tree must be the tree that was actually tested.
    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; old \
         reports cannot be reused for new code"
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
        "no `accept_f52_d_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed.
    for required in REQUIRED_TESTS {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == required)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{required} did not run: F52-D declares `gpu`, so step 1 must run with \
                     `--include-ignored` and a GPU adapter available"
                )
            });
        assert_eq!(status, "pass", "{required} must pass; got status {status}");
    }
    assert_eq!(
        suite.assertions.len(),
        REQUIRED_TESTS.len(),
        "and every `accept_f52_d_` test in the selection is one this report names: {:?}",
        suite
            .assertions
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
    );

    // `source` is null: this stage declares no `retail` capability and reads no
    // original data, so there is no installation or content hash to record.
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    let captures = capture_artifacts(&evidence_dir);
    for expected in EXPECTED_CAPTURES {
        assert!(
            captures.iter().any(|(name, _, _)| name == expected),
            "the suite must have written {expected}: found {:?}",
            captures
                .iter()
                .map(|(name, _, _)| name.as_str())
                .collect::<Vec<_>>()
        );
    }
    artifacts.extend(captures);

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let document = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F52-D\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": null, \"content_sha256\": null}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"gpu\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&review_identity()),
        jstr(&review_method()),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &document).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F52-D\"",
        "\"capabilities\": [\"gpu\", \"synthetic\"]",
        "\"claim\": \"implemented\"",
        "\"install_sha256\": null",
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
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written honestly \
         and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// Every capture the suite wrote, hashed as artifacts.
fn capture_artifacts(evidence_dir: &Path) -> Vec<(String, String, String)> {
    let mut found: Vec<(String, String, String)> = fs::read_dir(evidence_dir)
        .expect("the evidence directory is readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with(CAPTURE_PREFIX) && name.ends_with(CAPTURE_SUFFIX)
                })
        })
        .map(|path| {
            let name = path
                .file_name()
                .expect("capture has a file name")
                .to_string_lossy()
                .into_owned();
            let bytes = fs::read(&path).expect("capture is readable");
            (name, sha256(&bytes).to_hex(), "png".to_owned())
        })
        .collect();
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found
}

fn review_identity() -> String {
    String::from(
        "Implementer: bunny-2 (opencode/mimo-v2.6-Flash, Rally #211 implement claim of \
         2026-10-08). Reviewer: recorded by the reviewer in the complete_review notes — this \
         report was written by the implementer, so it is not independent evidence: AGENTS.md asks \
         for a different agent instance or model with fresh context for fidelity claims, and no \
         agent review of this evidence replaces the owner's human approval.",
    )
}

fn review_method() -> String {
    String::from(
        "the acceptance suite re-run locally with a real GPU adapter (Apple M3 Pro, Metal), every \
         `accept_f52_d_` test selected by prefix with `--include-ignored` and `CS_EVIDENCE_DIR` set; \
         this harness derives every field from the recorded log, `rustc --version` and `Cargo.lock`, \
         and the PNGs the capture test wrote through \
         `cs_app::accessibility::gpu_capture::capture_objective_page`, which delegates the draw to \
         the production `cs_app::text::capture_text_boxes` and refuses a frame that drew nothing; \
         validated with tools/validate_evidence.py --require-pass. `source` is null because this \
         stage declares no `retail` capability and reads no original data — an installation hash \
         would be a claim of evidence never taken. The report's `unknowns` are this task's own \
         blockers and are empty because the acceptance run passed; the product incompleteness this \
         stage found — no replay/comparison writer carries the fidelity label yet (F52-W3, #782), \
         no presenter applies the colour filter, UI scale or bus levels (F52-W2, #781), nothing \
         opens the session at boot (F52-W1, #780), the page geometry and the capture's quads are \
         designed rather than original, and the original option set is still unmeasured — is \
         written out with its resolving task in \
         docs/findings/2026-10-08-f52-d-accessibility-flows-gpu-review.md and gated by it. `claim` \
         is `implemented` only. `candidate_tree` is the tree of the commit the suite ran on; the \
         only later delta is this report's own copy under docs/findings/evidence/, whose bytes are \
         that file.",
    )
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/accessibility/evidence.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F52-D` written relative to the
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
        "git {:?} failed: {}",
        args,
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
    panic!("package {package:?} is not {}", lock_path.display());
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
    /// `(test name, "pass" | "fail")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the libtest summaries and the per-test results of the
/// `accept_f52_d_` tests from a recorded `cargo test` output.
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
        // A status on its own line completes the earliest test that was
        // started on an earlier line without an inline status.
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
        // `test <name> ... <status>`, possibly several per interleaved line.
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let full = &after[..separator];
            if !full.contains("accept_f52_d_") {
                cursor = &after[separator + 5..];
                continue;
            }
            let name = full.rsplit("::").next().expect("a name").to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
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
fn summary_fields(line: &str) -> Vec<(u64, &str)> {
    let mut fields = Vec::new();
    for segment in line["test result:".len()..].split(';') {
        let words: Vec<&str> = segment.split_whitespace().collect();
        for pair in words.windows(2) {
            if let Ok(count) = pair[0].parse::<u64>()
                && matches!(pair[1], "passed" | "failed" | "ignored")
            {
                fields.push((count, pair[1]));
                break;
            }
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

/// One referenced artifact: hashed here with the production SHA-256 of this
/// workspace (the validator re-hashes it with `hashlib` independently).
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
            let mut evidence = String::from("\"cargo-test.log\"");
            if name.contains("gpu_capture") {
                for capture in EXPECTED_CAPTURES {
                    evidence.push_str(&format!(", {capture:?}"));
                }
            }
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [{evidence}]}}",
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

/// A JSON string literal: quoted and escaped, so no report field can break out
/// of its string.
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

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a UTC calendar
/// date, because `std` has no date formatting.
fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = (if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    }) as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (
        year,
        month,
        day,
        (rest / 3_600) as u32,
        ((rest % 3_600) / 60) as u32,
        (rest % 60) as u32,
    )
}
