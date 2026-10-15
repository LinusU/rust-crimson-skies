//! Evidence-report harness for task F45-B.1 (#742; `docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`).
//!
//! Deliberately **not** named `accept_f45_b_1_*`: it fails loudly when its
//! inputs are missing and must never be picked up as an acceptance test. Run
//! from the workspace root after the acceptance suite:
//!
//! 1. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F45-B.1 \
//!      cargo test --workspace --locked -- accept_f45_b_1_ --include-ignored \
//!      2>&1 | tee private/evidence/F45-B.1/cargo-test.log
//!    ```
//!    (run with `pipefail`; the pipeline's status is `CS_EVIDENCE_EXIT_CODE`.
//!    `CS_EVIDENCE_DIR` must be set *in this step*: the retail test writes
//!    `original-layout.json` only then.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F45-B.1 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f45_b_1_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!    CS_GAME_DIR="$CS_GAME_DIR" \
//!      cargo test --locked -p cs_app --test ui -- evidence_report_f45_b_1 --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/F45-B.1/acceptance.json \
//!      --artifact-root private/evidence/F45-B.1 --require-pass
//!    ```
//! 4. Commit a copy as `docs/findings/evidence/F45-B.1.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and the layout artifact the retail test wrote;
//! nothing is typed in by hand. `retail` is carried by the one retail test and
//! `synthetic` by the unignored format tests; all must appear in the log and
//! pass.
//!
//! The report's `unknowns` list the task's **product limitations** (what the
//! original layout does not state or the designed table cannot bind). They are
//! not blockers of the run, and they are never removed to satisfy a validator.
//! The claim is `implemented`, never `checked` or `verified_original`.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};

const RETAIL_TEST: &str =
    "accept_f45_b_1_retail_the_original_layout_is_read_and_each_screen_is_built_or_refused_by_name";

/// The synthetic format tests that must also appear and pass.
const SYNTHETIC_TESTS: &[&str] = &[
    "accept_f45_b_1_the_parser_reads_sections_variables_and_keeps_unresolved_placeholders",
    "accept_f45_b_1_a_button_is_bound_only_through_the_script_it_starts",
    "accept_f45_b_1_the_deck_refuses_a_screen_whose_original_buttons_the_table_cannot_all_bind",
    "accept_f45_b_1_a_screen_that_the_table_fully_covers_enters_the_deck",
    "accept_f45_b_1_art_that_is_missing_ambiguous_or_unreadable_is_refused_not_chosen",
];

const LAYOUT_ARTIFACT: &str = "original-layout.json";

/// The product limitations, machine-readable. Mirrors the "What stays unknown"
/// section of `docs/findings/2026-10-15-f45-b-1-original-layout.md`.
const UNKNOWNS: &[&str] = &[
    "original focus/tab order: no column of LAYOUT.CSV carries it (the header's TabOrder column is absent from every data row); focus stays the declared order. Gates any keyboard/controller-focus fidelity claim.",
    "handlers of the buttons that start no screen script (NoScript, e.g. quit, fly mission, return to briefing): they live in ASSETS/SCRIPTS/*.SCRIPT, not read here. Gates 'every visible original button works'.",
    "MainMenu and Settings have no screen-sized pane at the origin (their backdrop is a movie row / the menu behind): refused NoFullPane, no art bound.",
    "Cabin, FlightCheck and Construction are refused by the deck (HiddenAction / NoEscapeButton): the designed state table and the original screens do not match. Gates any claim that the designed front end reproduces the original screen set.",
    "original screens with no designed counterpart (Campaign, Instant Action, Multiplayer, Credits, Save/Load, Momento selection, Plane name/selection, Ordinance layout) are unpaired and unread.",
    "BUTTON_STATE_FRAMES=4 is read from the art; PC_B_PreviousMissions.png (height 129) has an unknown frame count and is refused.",
    "the pairing of original sections with designed screens (SCREEN_SECTIONS) is a name-based design choice awaiting owner review.",
    "no original executable run: retail is read access only; nothing is verified_original.",
];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_f45_b_1_writes_the_acceptance_report() {
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
        "no `accept_f45_b_1_` tests were recorded in {}",
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
        "every `accept_f45_b_1_` test in the selection is one this report names: {:?}",
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
    let layout_path = evidence_dir.join(LAYOUT_ARTIFACT);
    let layout = fs::read_to_string(&layout_path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}; step 1 must set CS_EVIDENCE_DIR",
            layout_path.display()
        )
    });
    for needle in [
        format!("\"install_sha256\": \"{install_sha256}\""),
        "\"sections\": 34, \"panes\": 106, \"buttons\": 119".to_owned(),
        "\"deck\": \"in-deck\"".to_owned(),
    ] {
        assert!(
            layout.contains(&needle),
            "the layout artifact does not describe this run ({needle:?}); a stale file cannot be \
             reported as measured evidence"
        );
    }

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&layout_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let unknowns: Vec<String> = UNKNOWNS.iter().map(|text| (*text).to_owned()).collect();

    let document = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F45-B.1\",\n\
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
        "Implementer: claude-2 (claude-sonnet-5-5, Rally #742 implement claim of 2026-10-15). \
         Reviewer: recorded by the reviewer in the complete_review notes; this report was \
         written by the implementer, so it is not independent evidence, and no agent review \
         replaces the owner's human approval.",
    )
}

fn review_method() -> String {
    String::from(
        "the acceptance suite re-run locally with `CS_GAME_DIR` set, every `accept_f45_b_1_*` \
         test selected by prefix with `--include-ignored`; every field is derived from the \
         recorded log, production discovery of $CS_GAME_DIR and the layout artifact the retail \
         test derived with the production reader (`cs_app::ui::front_end::OriginalLayout` over \
         `FrontEndScreens`, `cs_assets::rof::mount_rof_into` for GOSDATA/ASSETS/crimson.rof); the \
         artifact is checked against this report's install hash before it is referenced. \
         `unknowns` are the product limitations of docs/findings/2026-10-15-f45-b-1-original-layout.md \
         and gate fidelity claims. `claim` is `implemented` only: `retail` means read access to \
         the owner's files, not an original run.",
    )
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/ui/evidence.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F45-B.1` written relative to the
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
/// `accept_f45_b_1_` tests from a recorded `cargo test` output.
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
            if !full.contains("accept_f45_b_1_") {
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
                 \"front-end-screens.json\"]}}",
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
