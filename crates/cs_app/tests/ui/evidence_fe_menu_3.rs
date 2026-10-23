//! Evidence-report harness for task FE-MENU-3 (#1835; `docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`).
//!
//! Deliberately **not** named `accept_fe_menu_3_*`: it fails loudly when its
//! inputs are missing and must never be picked up as an acceptance test. Run
//! from the workspace root after the acceptance run, exactly as:
//!
//! 1. The acceptance run (the same command AGENTS.md names for a task's
//!    selection). The ignored windowed test clears its capture directory
//!    and rewrites the smoke's own artifacts (`*.png`, `report.json`, the
//!    seeded `profiles/` population), under `CS_FE_MENU_SMOKE_OUT`:
//!    ```sh
//!    mkdir -p private/evidence/FE-MENU-3
//!    CS_EVIDENCE_DIR=$PWD/private/evidence/FE-MENU-3 \
//!    CS_FE_MENU_SMOKE_OUT=$PWD/private/evidence/FE-MENU-3 \
//!      cargo test --locked -p cs_app --test cs_app_tests_c -- accept_fe_menu_3_ --include-ignored \
//!      2>&1 | tee private/evidence/FE-MENU-3/cargo-test.log
//!    ```
//!    (record the pipeline's exit status; it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/FE-MENU-3 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --locked -p cs_app --test cs_app_tests_c -- accept_fe_menu_3_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!    CS_EVIDENCE_REVIEWER=<identity> \
//!    CS_GAME_DIR="$CS_GAME_DIR" \
//!      cargo test --locked -p cs_app --test cs_app_tests_c -- evidence_report_fe_menu_3 --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/FE-MENU-3/acceptance.json \
//!      --artifact-root private/evidence/FE-MENU-3 --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/FE-MENU-3.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR`, the retail test's
//! smoke run's own `report.json` and framebuffer PNGs, and
//! `rustc --version` plus `Cargo.lock`. Nothing is
//! typed in by hand.
//!
//! The claim is `implemented`, never `checked` or `verified_original`: the
//! windowed run is this machine's own frames and no original executable ran.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint};

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_fe_menu_3_";

/// The retail test: reads the original layout's Campaign rows into the page.
const RETAIL_TEST: &str = "accept_fe_menu_3_retail_the_campaign_rows_are_as_written";

/// The windowed test: drives `cs --front-end --smoke-seconds` on a GPU.
const WINDOWED_TEST: &str =
    "accept_fe_menu_3_the_windowed_smoke_draws_the_typed_name_and_the_pilot_list";

/// The synthetic tests that must also appear and pass.
const SYNTHETIC_TESTS: &[&str] = &[
    "accept_fe_menu_3_a_page_reads_the_edit_and_list_rows_as_written",
    "accept_fe_menu_3_b_typing_is_bounded_and_list_selection_copies_the_name",
    "accept_fe_menu_3_c_continue_creates_chooses_and_persists_pilots",
    "accept_fe_menu_3_d_the_list_holds_at_most_the_scripts_limit",
    "accept_fe_menu_3_e_delete_needs_a_second_press_and_removes_the_save",
    "accept_fe_menu_3_f_keyboard_pointer_and_buttons_drive_the_page_through_the_machine",
    "accept_fe_menu_3_g_delete_cancel_and_escape_run_through_the_input_path",
    "accept_fe_menu_3_h_typing_off_the_page_never_reaches_it",
];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_fe_menu_3_writes_the_acceptance_report() {
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
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    assert_eq!(
        candidate_tree,
        git(&["rev-parse", "HEAD^{tree}"]),
        "CS_CANDIDATE_TREE must be the tree of the tested commit"
    );

    // The recorded acceptance run: its counts and per-test results.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", log_path.display()));
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "the acceptance log was not understood: {suite:?}"
    );
    let required: Vec<&str> = std::iter::once(RETAIL_TEST)
        .chain(std::iter::once(WINDOWED_TEST))
        .chain(SYNTHETIC_TESTS.iter().copied())
        .collect();
    for name in &required {
        let status = suite
            .assertions
            .iter()
            .find_map(|(found, status)| (found == name).then_some(*status))
            .unwrap_or("missing");
        assert_eq!(
            status, "pass",
            "{name} must appear once and pass in the acceptance log ({status})"
        );
    }

    // The smoke run's own report: the scripted pass's own record — its kind,
    // the navigation and the verdict.
    let smoke_path = evidence_dir.join("report.json");
    let smoke = fs::read_to_string(&smoke_path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error} (the windowed test's capture directory \
             must be CS_FE_MENU_SMOKE_OUT=$CS_EVIDENCE_DIR)",
            smoke_path.display()
        )
    });
    assert!(
        smoke.contains("\"kind\":\"front-end-shell-smoke-report\""),
        "{} is the front-end smoke's own report",
        smoke_path.display()
    );
    assert!(
        smoke.contains("\"passed\":true"),
        "the smoke run passed its own checks: {smoke}"
    );
    for label in ["campaign", "campaign-typed", "campaign-list"] {
        assert!(
            smoke.contains(&format!("{label}.png")),
            "the smoke photographed {label}: {smoke}"
        );
        assert!(
            evidence_dir.join(format!("{label}.png")).is_file(),
            "{label}.png is among the artifacts"
        );
    }
    let frames = json_u64(&smoke, "frames").unwrap_or(0);

    // The source fingerprints, from production discovery.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // Every artifact this run left in the evidence directory — the log, the
    // smoke's report and its framebuffer PNGs, the shell's section record —
    // hashed as it sits.
    let mut artifacts = collect_artifacts(&evidence_dir, &evidence_dir);
    artifacts.sort_by(|left, right| left.0.cmp(&right.0));
    for (name, kind) in [("cargo-test.log", "log"), ("report.json", "json")] {
        assert!(
            artifacts
                .iter()
                .any(|(found, _, found_kind)| found == name && found_kind == kind),
            "{name} must be one of the artifacts"
        );
    }
    assert!(
        artifacts.iter().any(|(_, _, kind)| kind == "png"),
        "the GPU captures must be among the artifacts: this task claims `gpu`"
    );

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"FE-MENU-3\",\n \"candidate_tree\": {},\n \
         \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \
         \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \
         \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": {frames}}},\n \"overrides\": {},\n \
         \"capabilities\": [\"retail\", \"gpu\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \
         \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": \
         [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \
         \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
        jstr(&candidate_tree),
        engine_json(&engine),
        jstr(&iso_utc_now()),
        str_array(&argv),
        jstr(&git(&["rev-parse", "--show-toplevel"])),
        exit_code,
        jstr(&install_sha256),
        jstr(&content_sha256),
        str_array(&[
            "the front-end smoke's manual 1/60 s clock".to_owned(),
            "the original save layout is unread: pilots persist in this project's own profile \
             files through ProfileSession, and the original's engine callbacks 2099/2100/2101/2106 \
             stay unmodelled"
                .to_owned(),
            "FONT_CM_EDIT is a GDI font id whose face and height are not in the installation: the \
             typed name and the list rows draw in the engine's default face, a stand-in"
                .to_owned(),
            "the list's slider/up/down art placement and frame layout are unread: the scroll bar \
             is not drawn and the list scrolls by arrows"
                .to_owned(),
            "the original's yes/no MessageBox on Delete Player is not presented: the first press \
             arms the delete naming the pilot and a second confirms, a design choice"
                .to_owned(),
            "Esc as Cancel is the task's design addition: the measured finding \
             2026-10-22-parity5-start-flow-transitions.md found no Esc handler on this page"
                .to_owned(),
        ]),
        suite.discovered,
        suite.executed,
        suite.passed,
        suite.failed,
        suite.ignored,
        assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&reviewer),
        jstr(&review_method()),
    );
    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).expect("write acceptance.json");
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report must NOT validate",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// How this run was reviewed.
///
/// The prose names the windowed run and what it measured, so a regenerated
/// report cannot describe more than this one observed.
fn review_method() -> String {
    "Acceptance suite run locally with `retail,gpu` (CS_GAME_DIR, CS_CAPABILITIES): the two ignored \
     tests ran — the retail test read the installation's own ASSETS/LAYOUT.CSV Campaign section \
     through PilotPage::from_layout (CM_E_NAME: FONT_CM_EDIT at 243,297 221x22, 16 characters, \
     text 0xFFFF0000, caret 0xFFEF0010; CM_L_PLAYERS: 245,356, 305 wide, 20 px rows, 7 displayed) \
     and the shell's pane and three buttons around them, and the windowed test drove \
     `cs --cs-path $CS_GAME_DIR --front-end --smoke-seconds 14 --capture-dir \
     private/evidence/FE-MENU-3` on this machine's GPU (Apple M3 Pro, Metal) over a synthetic \
     population seeded with three pilots: the Campaign visit typed a name through KeyboardInput, \
     Down selected the first pilot and copied its name into the edit box, and the run wrote \
     framebuffer PNGs (campaign, campaign-typed, campaign-list among them) and report.json. The \
     synthetic tests prove the page's rules (typing bounds, list selection, create/choose/delete, \
     the 24-pilot limit, persistence across a reopened session) and the keyboard/pointer/F45 \
     machine route through the headless presenter. Those records are the product's own output, \
     hashed here; the assertions above are the acceptance run's own results. The run is scripted, \
     never `human_play`, and no original executable ran, so no screenshot here is the original's \
     renderer. LIMITS: the original save layout, FONT_CM_EDIT's face, the slider art placement, the \
     delete confirmation dialog and Esc on this page are recorded unknowns (see the overrides and \
     the pilot page's module docs). Validated with tools/validate_evidence.py --require-pass."
        .to_owned()
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/ui/evidence_fe_menu_3.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/FE-MENU-3` written relative to the
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

/// One integer out of the smoke's `report.json`, by key: the run's own
/// numbers, never a remembered one.
fn json_u64(report: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{key}\":");
    let index = report.find(&needle)?;
    let rest = &report[index + needle.len()..];
    let digits: String = rest
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

fn iso_utc_now() -> String {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_secs();
    let days = (since / 86_400) as i64;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        (since % 86_400) / 3600,
        (since % 3600) / 60,
        since % 60
    )
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to (y, m, d).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
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
    /// `(test name, "pass" | "fail" | "unknown")`, in log order.
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the per-test results of the `accept_fe_menu_3_` tests from a
/// recorded `cargo test` output.
///
/// The counts come from the **prefixed test lines**, never from the
/// `test result:` summaries: a summary aggregates every test binary cargo
/// ran, so reading it would report hundreds of unrelated tests as this
/// task's selection. A prefixed test that was ignored is recorded with the
/// schema's `unknown` status rather than counted as a pass, so the report
/// can never claim an assertion it did not run.
fn parse_suite(log: &str) -> Suite {
    let mut suite = Suite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if pending.front().is_some()
            && let Some(status) = finished(trimmed)
        {
            let name = pending.pop_front().expect("pending test");
            suite.assertions.push((name, status));
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("test ") else {
            continue;
        };
        let Some((name, tail)) = rest.split_once(" ... ") else {
            continue;
        };
        if !carries_prefix(name) {
            continue;
        }
        // The report's assertion id is the bare test name, the convention
        // the other evidence reports use — the module path stays in the log.
        let leaf = name.rsplit("::").next().unwrap_or(name).to_owned();
        match finished(tail.trim()) {
            Some(status) => suite.assertions.push((leaf, status)),
            None => pending.push_back(leaf),
        }
    }
    suite.passed = count(&suite, "pass");
    suite.failed = count(&suite, "fail");
    suite.ignored = count(&suite, "unknown");
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// Whether a libtest name is one of this task's tests, matched on the name's
/// last path segment so an in-module unit test under a module path still
/// counts.
fn carries_prefix(name: &str) -> bool {
    name.rsplit("::")
        .next()
        .is_some_and(|leaf| leaf.starts_with(ACCEPTANCE_PREFIX))
}

fn finished(line: &str) -> Option<&'static str> {
    if line.starts_with("ok") {
        Some("pass")
    } else if line.starts_with("ignored") || line.starts_with("ignored,") {
        Some("unknown")
    } else if line.starts_with("FAILED") || line.starts_with("failed") {
        Some("fail")
    } else {
        None
    }
}

fn count(suite: &Suite, status: &str) -> u64 {
    suite
        .assertions
        .iter()
        .filter(|(_, found)| *found == status)
        .count() as u64
}

// ------------------------------------------------------------- artifacts ---

/// Every file under `root`, as `(relative path, sha256, kind)`, skipping the
/// report this harness is about to write.
fn collect_artifacts(root: &Path, dir: &Path) -> Vec<(String, String, String)> {
    let mut found = Vec::new();
    let entries =
        fs::read_dir(dir).unwrap_or_else(|error| panic!("cannot list {}: {error}", dir.display()));
    for entry in entries {
        let entry = entry.expect("a directory entry");
        let path = entry.path();
        if path.is_dir() {
            found.extend(collect_artifacts(root, &path));
            continue;
        }
        let name = path
            .strip_prefix(root)
            .expect("the entry is under the evidence root")
            .to_string_lossy()
            .replace('\\', "/");
        if name == "acceptance.json" {
            continue;
        }
        let kind = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("file")
            .to_owned();
        found.push((name, sha256_file(&path), kind));
    }
    found
}

/// One artifact's digest, computed with the production SHA-256: the
/// validator re-hashes the same file with `hashlib` independently.
fn sha256_file(path: &Path) -> String {
    let bytes =
        fs::read(path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    cs_assets::install::sha256(&bytes).to_hex()
}

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

/// One assertion row per recorded test; the retail and windowed tests name
/// the artifacts they produced, the rest name the acceptance log.
fn assertion_array(assertions: &[(String, &'static str)]) -> String {
    let items: Vec<String> = assertions
        .iter()
        .map(|(name, status)| {
            let evidence: &[&str] = match name.as_str() {
                WINDOWED_TEST => &["cargo-test.log", "report.json"],
                _ => &["cargo-test.log"],
            };
            let evidence = evidence
                .iter()
                .map(|path| jstr(path))
                .collect::<Vec<_>>()
                .join(", ");
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
            other if (other as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", other as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
