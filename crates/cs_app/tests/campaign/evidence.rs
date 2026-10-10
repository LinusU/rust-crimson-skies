//! Evidence-report harnesses for the per-mission binding stages M01-A,
//! M02-A, M03-A, M04-A, M05-A, M06-A, M07-A, M08-A, M10-A, M12-A, M13-A, M16-A,
//! M17-A, M18-A, M19-A, M21-A, M24-A, for the mission-compatibility stages
//! M02-B, M03-B, M04-B, M06-B, M08-B, M10-B, M12-B, M13-B, M16-B, M17-B and
//! M19-B, for the whole-campaign binding stage
//! F50-B and for the per-mission probe-route stage F50-C
//! (`docs/contracts/CLI-EVIDENCE.md`, schema
//! `schemas/evidence.schema.json`), and — as the *reissue* harnesses of Rally
//! #1174 `EVIDENCE-CONTENT-DIGEST-OUTLIERS-REPAIR` — for F05-D, F12-J and
//! T351, the three committed reports that recorded a task-scoped digest in
//! `source.content_sha256`, a field whose contract meaning is the canonical
//! whole-installation fingerprint.
//!
//! These tests are deliberately **not** named `accept_m01_a_*` …
//! `accept_m08_a_*` … `accept_m10_a_*` … `accept_m16_a_*` … `accept_m17_a_*` …
//! `accept_m18_a_*` … `accept_m19_a_*` … `accept_m21_a_*` … `accept_f50_b_*` …
//! `accept_f50_c_*`: they are not part
//! of the acceptance
//! suites, they fail
//! loudly when their inputs are missing instead of passing vacuously, and a
//! task's test selection must never pick them up as acceptance tests. Run
//! from the workspace root, after the acceptance suite, exactly as (with
//! `M01-A` / `accept_m01_a_` substituted for `M02-A` / `accept_m02_a_`):
//!
//! 1. ```sh
//!    cargo test --workspace --locked -- accept_m02_a_ --include-ignored \
//!      2>&1 | tee private/evidence/M02-A/cargo-test.log
//!    ```
//!    (record the pipeline's exit status — it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M02-A \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m02_a_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!      cargo test --locked --test campaign evidence_report_m02_a -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/M02-A/acceptance.json \
//!      --artifact-root private/evidence/M02-A --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/M02-A.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR` and the binding
//! production code derives from it, `rustc --version` and `Cargo.lock`.
//! Nothing is typed in by hand.
//!
//! The reports' `unknowns` are *those tasks'* blockers and are empty because
//! the acceptance run passed. The bindings' own unbound checklist entries are
//! **not** dropped anywhere: they are carried in
//! `missions/bindings/M01.json` … `missions/bindings/M08.json`, `M10.json`,
//! `M12.json`, `M13.json`, `M16.json`, `M17.json`, `M18.json`, `M19.json`,
//! `M21.json`, `M24.json` and in
//! `docs/findings/`, which is where the product-incompleteness state lives
//! (`AUDIT-PLAN-SYNC`: keep the states separate). The claim is `implemented`,
//! never `checked` or `verified_original`.
//!
//! Every `review.identity` literal below names the implementer and the reviewer
//! as the two Rally actor strings (`<agent instance>/<session label>`) that
//! really ran, and says whether the reviewer's context was fresh. A stage added
//! to this file should add its own Rally implementer and reviewer to
//! `docs/findings/2026-10-02-m16-a-fu2-rally-review-snapshot.json` in the same
//! change: `tools/tests/test_evidence_review_identity.py` resolves every literal
//! in this file against that snapshot, and a stage the snapshot has not caught
//! up with is reported as an advisory note, not a failure.  A literal carrying a
//! hand-over placeholder such as `reviewer: none yet` is a failure whether or not
//! the snapshot knows the stage, so a new stage must not paste one.
//!
//! # Where a new task's evidence goes
//!
//! This file holds only what every harness shares: the imports, the `Suite`
//! log parser and the helpers that read the environment, `git`, the
//! installation, the artifacts and the report renderer. A task's own test
//! lists (`RETAIL_TESTS_*`, `SYNTHETIC_TESTS_*`, `IMAGE_TESTS_*`), its
//! `evidence_report_*` harness and the parser of its own run live in one child
//! module per task: `evidence/<task>.rs` (for example `evidence/m02_b.rs`,
//! `evidence/m02_b_fu2.rs`), which starts with `use super::*;` and so sees
//! everything declared here.
//!
//! A new task therefore extends this directory in exactly one way: it adds its
//! own `evidence/<task>.rs` and one `mod <task>;` line to the sorted module
//! list above. There is no registration table to append to — libtest collects
//! a `#[test]` wherever it is declared — and each `mod` line is alone on its
//! own line, so two branches adding two tasks do not collide here. A new
//! acceptance suite adds its own `<task>.rs` next to this file and one sorted
//! `mod` line in `main.rs`, the same way.
//!
//! # The #1174 reissue harnesses
//!
//! `evidence/f05_d.rs`, `evidence/f12_j.rs` and `evidence/t351.rs` reissue
//! three reports that already exist, so they share the helpers this file
//! holds alongside the ones above: [`tabled_assertions`] and
//! [`full_test_status`] read each row's status out of the recorded log by its
//! full test id, and [`cs_inspect_binary`] / [`run_cs_inspect`] run the
//! production `cs-inspect` commands that write their artifacts — one process
//! per report, because a second `rof` command in this process would carry the
//! process-wide session generation and could not reproduce the bytes the CLI
//! writes standalone. Unlike the mission stages, whose `assertions` are the
//! leaf test names the log parser records, these three reports keep the
//! **full** test ids their original reports spelled — two of T351's five tests
//! share a leaf name, so a leaf-keyed table could not tell them apart — and
//! each row carries the evidence entries the original record cited. The status
//! of every row is read from the recorded log by its full id, the counts stay
//! the log parser's, and a row the log does not contain is a loud failure
//! rather than a report that quietly records a test that did not run.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::campaign_bindings::{
    CampaignInventory, JoinAgreement, JoinCorroboration, MissionLabel, ProbeInterruption,
    SourceBinding, SourceContext, TitleBlock, blocks_correspond, probe_routes,
};
use cs_content::mission_control::{
    CONTROL_RECORD_SOUND_KEY_VOCABULARY, RecordSoundDisposition, record_sound_disposition,
};
use cs_sim::objectives::address::{OUT_OF_RANGE_OBJECTIVE_ADDRESS, resolve_objective_address};
use cs_types::content::ContentId;

// One module per task's evidence, in sorted order: a new task
// adds one `mod` line here and its own `evidence/<task>.rs`.
mod f05_d;
mod f12_j;
mod f50_b;
mod f50_c;
mod f50_e4;
mod m01_a;
mod m02_a;
mod m02_b;
mod m02_b_fu1;
mod m02_b_fu2;
mod m02_b_fu3;
mod m02_t3;
mod m03_a;
mod m03_b;
mod m04_a;
mod m04_b;
mod m04_b_fu1;
mod m05_a;
mod m05_b;
mod m06_a;
mod m06_b;
mod m06_b_fu1;
mod m06_b_fu2;
mod m06_b_fu3;
mod m06_b_fu4;
mod m07_a;
mod m07_b;
mod m07_b_fu1;
mod m08_a;
mod m08_b;
mod m10_a;
mod m10_b;
mod m12_a;
mod m12_b;
mod m13_a;
mod m13_b;
mod m16_a;
mod m16_a_fu1;
mod m16_b;
mod m17_a;
mod m17_b;
mod m18_a;
mod m18_b;
mod m19_a;
mod m19_b;
mod m21_a;
mod m21_b;
mod m24_a;
mod record_objectives_sound;
mod t351;

/// The recorded status of one test, or a loud failure naming the missing run.
fn recorded_status(suite: &Suite, name: &str) -> &'static str {
    suite
        .assertions
        .iter()
        .find(|(seen, _)| seen == name)
        .map(|(_, status)| *status)
        .unwrap_or_else(|| {
            panic!(
                "{name} did not run: M02-T3 requires capability `retail`, run step 1 with \
                 `--include-ignored` and CS_GAME_DIR set"
            )
        })
}

/// A comma-separated list of numbers, for a JSON array.
fn numbers(values: &[usize]) -> String {
    values
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/campaign/evidence.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-A` written relative to the
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
    panic!("package {package:?} is not in {}", lock_path.display());
}

/// The M01 binding derived from the installation, built once.
fn source_binding(game_dir: &Path) -> &'static SourceBinding {
    static BINDING: OnceLock<SourceBinding> = OnceLock::new();
    BINDING.get_or_init(|| source_binding_for(game_dir, "M01"))
}

/// One work order's binding derived from the installation, through the same
/// production path `SourceContext::read` + `SourceContext::bind` the
/// acceptance suite uses. The declared discovery title comes from the
/// committed inventory, never from this file.
fn source_binding_for(game_dir: &Path, work_order: &str) -> SourceBinding {
    let context = SourceContext::read(game_dir)
        .expect("production source context reads the original installation");
    let title = declared_title(work_order);
    context
        .bind(
            cs_content::campaign_bindings::MissionLabel::new(work_order)
                .unwrap_or_else(|error| panic!("{work_order} is not a valid label: {error}")),
            &title,
        )
        .unwrap_or_else(|error| panic!("{work_order} binds to the original data: {error}"))
}

/// The declared discovery title of one work order, read from the committed
/// inventory.
fn declared_title(work_order: &str) -> String {
    let inventory = fs::read_to_string(
        Path::new(&git(&["rev-parse", "--show-toplevel"]))
            .join("missions/bindings/campaign-inventory.tsv"),
    )
    .expect("the declared campaign inventory reads");
    inventory
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| line.split_once('\t'))
        .find(|(label, _)| *label == work_order)
        .map(|(_, title)| title.trim().to_owned())
        .unwrap_or_else(|| panic!("the declared inventory has no {work_order} work order"))
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
/// `accept_m01_a_` tests from a recorded `cargo test` output.
fn parse_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m01_a_")
}

/// [`parse_suite`] with a task's own test prefix, so one parser serves every
/// mission binding stage and no report can record another task's assertions.
fn parse_suite_prefixed(log: &str, prefix: &str) -> Suite {
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
        // Test names carry their module path (`m01_a::accept_m01_a_…`);
        // the report records the leaf, which is what the prefix selects.
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let full = &after[..separator];
            if !full.contains(prefix) {
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

/// The production `cs-inspect` binary this sequence builds in its step 0,
/// located next to this test binary (`target/debug/deps/<test>` →
/// `target/debug/cs-inspect`).
///
/// The artifacts these harnesses record are the CLI's own reports, so the
/// CLI has to be the thing that writes them: a second `rof` command inside
/// the test process would carry the process-wide session generation and could
/// not reproduce the bytes a real `cs-inspect` run writes.
fn cs_inspect_binary() -> PathBuf {
    let mut binary = std::env::current_exe().expect("this test binary has a path");
    binary.pop(); // `deps/`
    binary.pop(); // the target profile directory
    let binary = binary.join("cs-inspect");
    assert!(
        binary.is_file(),
        "{} is missing: run `cargo build -p cs_inspect --locked` first (step 0 of the \
         evidence sequence)",
        binary.display()
    );
    binary
}

/// One `cs-inspect` run, asserted to have written `out` with exit code 0.
fn run_cs_inspect(binary: &Path, args: &[&str], out: &Path, what: &str) {
    let run = Command::new(binary)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("{} runs: {error}", binary.display()));
    assert!(
        run.status.success(),
        "cs-inspect {what} failed with {:?}: {}",
        run.status.code(),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        out.is_file(),
        "cs-inspect {what} did not write {}: {}",
        out.display(),
        String::from_utf8_lossy(&run.stderr)
    );
}

/// The status of one **full** test id (`module::path::leaf`) in the recorded
/// log, `None` when the log does not name it.
///
/// [`parse_suite_prefixed`] keys its assertions by the leaf name, because that
/// is what a task's selection selects; the #1174 reissue reports keep the full
/// ids their original reports spelled, and two of T351's tests share a leaf,
/// so those rows are looked up by the id the log actually prints. The status
/// may sit on the same line as the test or, when the test printed output, on
/// the line that completes it — the two shapes [`parse_suite_prefixed`]
/// already understands.
fn full_test_status(log: &str, full_id: &str) -> Option<&'static str> {
    let needle = format!("test {full_id} ... ");
    let start = log.find(&needle)?;
    let tail = &log[start + needle.len()..];
    let line_end = tail.find('\n').unwrap_or(tail.len());
    let head = tail[..line_end].trim_end();
    if head.starts_with("ok") {
        return Some("pass");
    }
    if head.starts_with("FAILED") {
        return Some("fail");
    }
    if head.starts_with("ignored") {
        return Some("ignored");
    }
    let complete = tail[line_end..]
        .lines()
        .map(str::trim_start)
        .find(|line| matches!(*line, "ok" | "FAILED" | "ignored"));
    complete.map(|line| match line {
        "FAILED" => "fail",
        "ignored" => "ignored",
        _ => "pass",
    })
}

/// The report's `assertions` array from a task's own table: every row's full
/// test id, its status read from the recorded log and the evidence entries
/// that row cites (always including the log itself).
///
/// A row the log does not name, or names as ignored, fails here rather than
/// being written with a status nothing recorded.
fn tabled_assertions(log: &str, table: &[(&str, &[&str])]) -> String {
    let items: Vec<String> = table
        .iter()
        .map(|&(full_id, evidence)| {
            let status = match full_test_status(log, full_id) {
                None => panic!(
                    "{full_id} is not in the recorded acceptance log: step 1 must record the \
                     task's own selection with `--include-ignored`"
                ),
                Some("ignored") => panic!(
                    "{full_id} is ignored in the recorded log: run step 1 with \
                     `--include-ignored` and CS_GAME_DIR set"
                ),
                Some(status) => status,
            };
            let mut cited: Vec<String> = vec!["cargo-test.log".to_owned()];
            for entry in evidence {
                if !cited.iter().any(|seen| seen.as_str() == *entry) {
                    cited.push((*entry).to_owned());
                }
            }
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": {}}}",
                jstr(full_id),
                str_array(&cited)
            )
        })
        .collect();
    items.join(", ")
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
/// calendar date, because `std` has no date formatting.
fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year_of_day = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = (if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    }) as u32;
    let year = if month <= 2 {
        year_of_day + 1
    } else {
        year_of_day
    };
    (
        year,
        month,
        day,
        (rest / 3_600) as u32,
        ((rest % 3_600) / 60) as u32,
        (rest % 60) as u32,
    )
}
