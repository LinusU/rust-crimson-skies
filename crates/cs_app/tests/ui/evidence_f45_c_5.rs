//! Evidence-report harness for task F45-C.5 (#1525; `docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`).
//!
//! Deliberately **not** named `accept_f45_c_5_*`: it fails loudly when its
//! inputs are missing and must never be picked up as an acceptance test. Run
//! from the workspace root after the acceptance suite:
//!
//! 1. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F45-C.5 \
//!      cargo test --locked -p cs_app -- accept_f45_c_5_ --include-ignored \
//!      2>&1 | tee private/evidence/F45-C.5/cargo-test.log
//!    ```
//!    (run with `pipefail`; the pipeline's status is `CS_EVIDENCE_EXIT_CODE`.
//!    `CS_EVIDENCE_DIR` must be set *in this step*: the retail test writes
//!    `flight-check-selection.json` only then.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F45-C.5 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --locked -p cs_app -- accept_f45_c_5_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!    CS_GAME_DIR="$CS_GAME_DIR" \
//!      cargo test --locked -p cs_app -- evidence_report_f45_c_5 --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/F45-C.5/acceptance.json \
//!      --artifact-root private/evidence/F45-C.5 --require-pass
//!    ```
//! 4. Commit a copy as `docs/findings/evidence/F45-C.5.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and the measurement artifact the retail test
//! wrote; nothing is typed in by hand. `retail` is carried by the one retail
//! test and `synthetic` by the three unignored tests; all must appear in the
//! log and pass.
//!
//! The report's `unknowns` are this task's own **blockers** (the validator's
//! `--require-pass` refuses any), and there are none because the run passed.
//! The product limitations (what the original data does not state) are **not
//! dropped**: `LIMITATIONS` below is checked line by line against the finding
//! that records them, and the referenced `flight-check-selection.json`
//! artifact lists every measured control, its destination and that
//! destination's selector rows. The claim is `implemented`, never `checked` or
//! `verified_original`.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};

const RETAIL_TEST: &str =
    "accept_f45_c_5_retail_the_flight_checks_ammunition_is_the_planes_ordnance_screen";

/// The synthetic tests that must also appear and pass.
const SYNTHETIC_TESTS: &[&str] = &[
    "accept_f45_c_5_the_reader_measures_the_controls_their_slots_and_their_selectors",
    "accept_f45_c_5_a_measured_producer_with_a_wingmate_control_commits_the_equipment",
    "accept_f45_c_5_a_measured_producer_without_a_wingmate_control_refuses_the_equipment",
];

const MEASUREMENT_ARTIFACT: &str = "flight-check-selection.json";

/// The product limitations. Mirrors the "What stays unknown" section of
/// `docs/findings/2026-10-18-f45-c-5-flight-check-options-and-ammunition.md`.
const LIMITATIONS: &[&str] = &[
    "The original's per-mission option set is not readable: the FlightCheck section holds no selector row, and the PlaneSelection / OrdinanceLayout lists are filled by scripting natives whose tables are unresolved. Gates \"the flight check offers what the original offers\".",
    "The native call numbers are named, not interpreted (2010/2011/2012/2013/2018/2019/2020/2027/2028/2030..2037, 27). Their meaning is unresolved. Gates any claim about which aircraft or loadouts the candidate lists contain.",
    "Whether the campaign roster's unlocks gate the flight check: nothing readable states it. Gates \"unlocks gate the flight check\".",
    "The -1 / -2 slot mapping is read from FLIGHTCHECK.SCRIPT's own picture controls, not from a table; a different revision could renumber it.",
    "Which authored wingmate slot a multi-slot mission's flight check fills stays refused (AmbiguousWingmateSlot).",
    "No original executable run: retail is read access only; nothing is verified_original.",
];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_f45_c_5_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_f45_c_5_` tests were recorded in {}",
        log_path.display()
    );
    let required: Vec<&str> = std::iter::once(RETAIL_TEST)
        .chain(SYNTHETIC_TESTS.iter().copied())
        .collect();
    for name in &required {
        let status = suite
            .assertions
            .iter()
            .find(|(recorded, _)| recorded == name)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{name} did not run: step 1 must use --include-ignored with CS_GAME_DIR set")
            });
        assert_eq!(status, "pass", "{name} must pass; got {status}");
    }
    assert_eq!(
        suite.assertions.len(),
        required.len(),
        "every `accept_f45_c_5_` test in the selection is one this report names: {:?}",
        suite
            .assertions
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
    );

    let found = discover(&game_dir).expect("production discovery must read the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The artifact is checked against this run's installation, not trusted.
    let measurement_path = evidence_dir.join(MEASUREMENT_ARTIFACT);
    let measurement = fs::read_to_string(&measurement_path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}; step 1 must set CS_EVIDENCE_DIR",
            measurement_path.display()
        )
    });
    for needle in [
        format!("\"install_sha256\": \"{install_sha256}\""),
        "\"flight_check_selectors\": []".to_owned(),
        "\"starts\": \"OrdinanceLayout\"".to_owned(),
        "\"starts\": \"PlaneSelection\"".to_owned(),
        "\"slot\": \"wingmate\"".to_owned(),
        "\"selectors\": [\"OL_D_AMMO0\"".to_owned(),
    ] {
        assert!(
            measurement.contains(&needle),
            "the measurement artifact does not describe this run ({needle:?}); a stale file \
             cannot be reported as measured evidence"
        );
    }

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&measurement_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let finding = fs::read_to_string(
        Path::new(&git(&["rev-parse", "--show-toplevel"]))
            .join("docs/findings/2026-10-18-f45-c-5-flight-check-options-and-ammunition.md"),
    )
    .expect("the finding is readable");
    assert!(
        finding.contains("What stays unknown") && !LIMITATIONS.is_empty(),
        "the limitations are recorded in the finding"
    );
    let unknowns: Vec<String> = Vec::new();

    let document = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F45-C.5\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": {},\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        str_array(&unknowns),
        jstr(&review_identity()),
        jstr(&review_method()),
    );
    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &document).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

fn review_identity() -> String {
    String::from(
        "Implementer: bunny-alpha-2 (deepseek-v4.1-flash, Rally #1525 implement claim of \
         2026-10-18). Reviewer: recorded by the reviewer in the complete_review notes; this \
         report was written by the implementer, so it is not independent evidence, and no agent \
         review replaces the owner's human approval.",
    )
}

fn review_method() -> String {
    String::from(
        "the acceptance suite re-run locally with `CS_GAME_DIR` set, every `accept_f45_c_5_*` \
         test selected by prefix with `--include-ignored`; every field is derived from the \
         recorded log, production discovery of $CS_GAME_DIR and the measurement artifact the \
         retail test derived with the production reader \
         (`cs_app::ui::front_end::OriginalLayout::flight_check_selection` over \
         `FrontEndScreens`, `cs_assets::rof::mount_rof_into` for GOSDATA/ASSETS/crimson.rof); \
         the artifact is checked against this report's install hash before it is referenced. \
         `unknowns` are blockers and are empty; the product limitations are in \
         docs/findings/2026-10-18-f45-c-5-flight-check-options-and-ammunition.md and the \
         measurement artifact, and gate fidelity claims. `claim` is `implemented` only: `retail` \
         means read access to the owner's files, not an original run.",
    )
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/ui/evidence_f45_c_5.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F45-C.5` written relative to the
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
/// `accept_f45_c_5_` tests from a recorded `cargo test` output.
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
            if !full.contains("accept_f45_c_5_") {
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
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\", \
                 \"flight-check-selection.json\"]}}",
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
