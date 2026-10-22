//! Evidence-report harness for task FE-MENU-2 (#1834; `docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`).
//!
//! Deliberately **not** named `accept_fe_menu_2_*`: it fails loudly when its
//! inputs are missing and must never be picked up as an acceptance test. Run
//! from the workspace root after the acceptance run, exactly as:
//!
//! 1. The acceptance run (the same command AGENTS.md names for a task's
//!    selection). The ignored retail test writes `front-end-mainmenu.json`
//!    and the ignored windowed test clears the smoke's own artifacts
//!    (`*.png`, `report.json`) and rewrites them, all under
//!    `CS_EVIDENCE_DIR`/`CS_FE_MENU_SMOKE_OUT`:
//!    ```sh
//!    mkdir -p private/evidence/FE-MENU-2
//!    CS_EVIDENCE_DIR=$PWD/private/evidence/FE-MENU-2 \
//!    CS_FE_MENU_SMOKE_OUT=$PWD/private/evidence/FE-MENU-2 \
//!      cargo test --locked -p cs_app --test cs_app_tests_c -- accept_fe_menu_2_ --include-ignored \
//!      2>&1 | tee private/evidence/FE-MENU-2/cargo-test.log
//!    ```
//!    (record the pipeline's exit status; it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/FE-MENU-2 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --locked -p cs_app --test cs_app_tests_c -- accept_fe_menu_2_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!    CS_EVIDENCE_REVIEWER=<identity> \
//!    CS_GAME_DIR="$CS_GAME_DIR" \
//!      cargo test --locked -p cs_app --test cs_app_tests_c -- evidence_report_fe_menu_2 --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/FE-MENU-2/acceptance.json \
//!      --artifact-root private/evidence/FE-MENU-2 --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/FE-MENU-2.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR`, the retail test's
//! own `front-end-mainmenu.json`, the smoke run's own `report.json` and
//! framebuffer PNGs, and `rustc --version` plus `Cargo.lock`. Nothing is
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
const ACCEPTANCE_PREFIX: &str = "accept_fe_menu_2_";

/// The retail test: reads the original layout, decodes the flag movie and
/// writes the MainMenu record.
const RETAIL_TEST: &str =
    "accept_fe_menu_2_retail_the_flag_decodes_and_the_six_buttons_are_as_written";

/// The windowed test: drives `cs --front-end --smoke-seconds` on a GPU.
const WINDOWED_TEST: &str = "accept_fe_menu_2_the_windowed_smoke_plays_the_flag_and_walks_the_six";

/// The synthetic tests that must also appear and pass.
const SYNTHETIC_TESTS: &[&str] = &[
    "accept_fe_menu_2_a_movie_row_presents_its_fields_and_drawn_rect",
    "accept_fe_menu_2_a_button_keeps_its_res_id_for_the_label_record",
    "accept_fe_menu_2_the_backdrop_plays_each_frame_at_its_time_and_loops",
    "accept_fe_menu_2_the_backdrop_holds_the_last_frame_when_loops_are_spent",
    "accept_fe_menu_2_a_failed_reopen_is_recorded_and_never_faked",
    "accept_fe_menu_2_the_six_buttons_reach_their_machine_states_through_the_real_path",
];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_fe_menu_2_writes_the_acceptance_report() {
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
    assert!(
        smoke.contains("MOVIE MOVIE on MainMenu: CrimFlag.MPG 320x240"),
        "the flag movie opened with its decoded extent: {smoke}"
    );
    assert!(
        smoke.contains("\"quit_seen\":true"),
        "Quit pressed terminate: {smoke}"
    );
    let frames = json_u64(&smoke, "frames").unwrap_or(0);
    assert!(
        json_u64(&smoke, "movie_frames").unwrap_or(0) > 0,
        "decoded movie frames reached a texture: {smoke}"
    );

    // The retail test's own record: the MainMenu section as the installation
    // writes it — the logo/frame panes, the flag movie row and the six
    // buttons' `ResID=0` label record.
    let shell_path = evidence_dir.join("front-end-mainmenu.json");
    let shell = fs::read_to_string(&shell_path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error} (the retail test writes it when \
             CS_EVIDENCE_DIR is set)",
            shell_path.display()
        )
    });
    assert!(
        shell.contains("\"section\": \"MainMenu\"")
            && shell.contains("CrimFlag.MPG")
            && shell.contains("MM_B_QUIT")
            && shell.contains("MM_LOGO")
            && shell.contains("BFRAME"),
        "{} is the shell's MainMenu record",
        shell_path.display()
    );

    // The source fingerprints, from production discovery.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // Every artifact this run left in the evidence directory — the log, the
    // smoke's report and its framebuffer PNGs, the shell's section record —
    // hashed as it sits.
    let mut artifacts = collect_artifacts(&evidence_dir, &evidence_dir);
    artifacts.sort_by(|left, right| left.0.cmp(&right.0));
    for (name, kind) in [
        ("cargo-test.log", "log"),
        ("report.json", "json"),
        ("front-end-mainmenu.json", "json"),
    ] {
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
        "{{\n \"schema_version\": 1,\n \"task_id\": \"FE-MENU-2\",\n \"candidate_tree\": {},\n \
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
            "the original layout's focus/tab order is unread: Up/Down walks the declaration \
             order, and the cursor art is the first named pointer member that decodes \
             (PASSIVEPOINTERZ.PNG here), never claimed as the original's choice"
                .to_owned(),
            "the M row's `# Loops=0` reads as continuous looping and the loop restarts on the \
             stream's own period (last frame's pts plus one frame interval): the original's \
             restart timing is unread, a stated design choice"
                .to_owned(),
            "the six MainMenu buttons' `ResID=0` is kept verbatim and no label text is drawn: \
             which string table answers an `IDS_*` name and how a label composes over the art \
             are unread"
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
     tests ran — the retail test read the installation's own ASSETS/LAYOUT.CSV MainMenu section \
     and MAINMENU.SCRIPT into the shell, decoded the real CrimFlag.MPG through the production \
     MpegVideoSource (320x240 @30.00 fps, its first frame's pixels non-uniform) and wrote \
     front-end-mainmenu.json (the MM_LOGO/BFRAME panes, the `MOVIE` row's fields verbatim, the six \
     MM_B_* buttons' `ResID=0` and stated effects), and the windowed test drove \
     `cs --cs-path $CS_GAME_DIR --front-end --smoke-seconds 14 --capture-dir \
     private/evidence/FE-MENU-2` on this machine's GPU (Apple M3 Pro, Metal), which wrote 12 real \
     framebuffer PNGs and report.json: the flag movie opened and delivered 302 decoded frames to \
     its texture under the panes at the row's 250%x250% (the authored 800x600), the pointer walk \
     visited MainMenu → Campaign → MainMenu → InstantAction → MainMenu → Preferences (left by Esc, \
     its return button's handler guarded) → MainMenu → Credits → MainMenu, the Multiplayer press \
     was the one recorded unhandled press (`MultiPlayerMain` is no section), five Down presses \
     walked the declaration order to Quit and Enter drove its `terminate`. Those records are the \
     product's own output, hashed here; the assertions above are the acceptance run's own \
     results. The run is scripted, never `human_play`, and no original executable ran. This \
     harness derives every field from the recorded log, production discovery of $CS_GAME_DIR, the \
     MainMenu record, the smoke's report and the hashes of those artifacts. LIMITS: the original \
     focus order, the loop's exact restart timing, the ResID label's string table and draw, the \
     movie's own audio (FE-MENU-4's work), the movie Z-order beyond declaration order and which \
     member the original maps onto the pointer are recorded unknowns in the shell's module docs \
     and the smoke's report. Validated with tools/validate_evidence.py --require-pass."
        .to_owned()
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/ui/evidence_fe_menu_2.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/FE-MENU-2` written relative to the
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

/// Extracts the per-test results of the `accept_fe_menu_2_` tests from a
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
                RETAIL_TEST => &["cargo-test.log", "front-end-mainmenu.json"],
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
