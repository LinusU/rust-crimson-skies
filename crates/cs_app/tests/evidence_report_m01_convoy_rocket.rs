//! Evidence-report harness for task #1999 `BUG-M01-CONVOY-ROCKET-DAMAGE`:
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//!
//! Not named `accept_m01_convoy_rocket_*`: it is not part of the acceptance
//! suite and it fails loudly when its inputs are missing, so the task's own
//! test selection can never pick it up as an acceptance test. Run it from the
//! workspace root, after the acceptance run and the owner-command smoke,
//! exactly as:
//!
//! 1. The owner-command smoke, whose capture directory is this evidence
//!    directory:
//!    ```sh
//!    mkdir -p private/evidence/BUG-M01-CONVOY-ROCKET-DAMAGE
//!    cs --cs-path "$CS_GAME_DIR" --mission M01 --smoke-seconds 45 \
//!      --capture-dir "$PWD/private/evidence/BUG-M01-CONVOY-ROCKET-DAMAGE"
//!    ```
//! 2. The acceptance run (the same command AGENTS.md names for a task's
//!    selection):
//!    ```sh
//!    CS_GAME_DIR="$CS_GAME_DIR" \
//!      cargo test --locked -p cs_app -- accept_m01_convoy_rocket_ --include-ignored \
//!      2>&1 | tee private/evidence/BUG-M01-CONVOY-ROCKET-DAMAGE/cargo-test.log
//!    ```
//!    (record the pipeline's exit status; it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 3. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/BUG-M01-CONVOY-ROCKET-DAMAGE \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --locked -p cs_app -- accept_m01_convoy_rocket_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 2> \
//!    CS_EVIDENCE_REVIEWER=<identity> \
//!    CS_GAME_DIR="$CS_GAME_DIR" \
//!      cargo test --locked -p cs_app --test cs_app_tests_a -- evidence_report_m01_convoy_rocket --include-ignored
//!    ```
//! 4. ```sh
//!    python3 tools/validate_evidence.py \
//!      private/evidence/BUG-M01-CONVOY-ROCKET-DAMAGE/acceptance.json \
//!      --artifact-root private/evidence/BUG-M01-CONVOY-ROCKET-DAMAGE --require-pass
//!    ```
//! 5. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/BUG-M01-CONVOY-ROCKET-DAMAGE.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR`, the smoke run's own
//! `report.json` and framebuffer PNGs, and `rustc --version` plus
//! `Cargo.lock`. Nothing is typed in by hand.
//!
//! The claim is `implemented`, never `checked` or `verified_original`: the
//! windowed run is this machine's own frames and no original executable ran.
//! What the smoke measured — including `weapon_rocket_damages`, the check
//! this task added — lives in `report.json`, which this report hashes as an
//! artifact.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint};

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_m01_convoy_rocket_";
/// The prefixed tests that read the installation, so the `retail` capability
/// in the report is backed by runs and not by a name.
const RETAIL_PREFIXES: [&str; 2] = [
    "accept_m01_convoy_rocket_retail_trucks_take_routed_damage",
    "accept_m01_convoy_rocket_retail_no_selection_refuses",
];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_convoy_rocket_writes_the_acceptance_report() {
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
    for retail in RETAIL_PREFIXES {
        assert!(
            suite
                .assertions
                .iter()
                .any(|(name, status)| name.ends_with(retail) && *status == "pass"),
            "the retail test {retail} must appear in the log as passed"
        );
    }

    // The owner-command smoke's own report: the checks, their verdicts and
    // the tick count this run measured. This task's check must be there,
    // applied and passed.
    let smoke_path = evidence_dir.join("report.json");
    let smoke = fs::read_to_string(&smoke_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", smoke_path.display()));
    assert!(
        smoke.contains("\"kind\":\"mission-smoke-report\""),
        "{} is the smoke's own report",
        smoke_path.display()
    );
    assert!(
        smoke.contains("\"human_play\":false"),
        "a scripted run is never human_play"
    );
    assert!(
        smoke.contains("\"id\":\"weapon_rocket_damages\",\"applies\":true,\"passed\":true"),
        "the smoke must judge this task's `weapon_rocket_damages` check passed: {}",
        smoke_path.display()
    );
    assert!(
        smoke.contains("\"failures\":[]"),
        "the smoke report records no failing checks: {}",
        smoke_path.display()
    );
    let ticks_end = json_u64(&smoke, "ticks").unwrap_or(0);

    // The source fingerprints, from production discovery.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // Every artifact this run left in the evidence directory — the log, the
    // smoke's report and its framebuffer PNGs — hashed as it sits.
    let mut artifacts = collect_artifacts(&evidence_dir, &evidence_dir);
    artifacts.sort_by(|left, right| left.0.cmp(&right.0));
    assert!(
        artifacts
            .iter()
            .any(|(name, _, kind)| name == "cargo-test.log" && kind == "log"),
        "the acceptance log must be one of the artifacts"
    );
    assert!(
        artifacts
            .iter()
            .any(|(name, _, kind)| name == "report.json" && kind == "json"),
        "the smoke's report must be one of the artifacts"
    );
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
        "{{\n \"schema_version\": 1,\n \"task_id\": \"BUG-M01-CONVOY-ROCKET-DAMAGE\",\n \"candidate_tree\": {},\n \
         \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \
         \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \
         \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": {ticks_end}}},\n \"overrides\": {},\n \
         \"capabilities\": [\"retail\", \"gpu\", \"audio\"],\n \"tests\": {{\"discovered\": {}, \
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
            "the mission smoke's manual 1/60 s clock".to_owned(),
            "the mission smoke's test placements of the production player body (each one listed \
             in report.json under `placements`)"
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
/// The prose names the windowed run, what it measured and the limits, so a
/// regenerated report cannot describe more than this one observed.
fn review_method() -> String {
    "Acceptance suite run locally with `retail,gpu,audio` (CS_GAME_DIR, CS_ENGINE_IMAGE): the five \
     synthetic tests and the two retail tests of accept_m01_convoy_rocket_ drive the production \
     launch -> fuse -> resolver path against composed M01 — the retail half selects t_truck01, \
     t_truck02 and t_truck03 through the production targeting session and destroys each with one \
     routed rocket on its own targets.zrd node, and the no-selection refusal fires no rocket. The \
     owner-command smoke `cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 45 --capture-dir \
     private/evidence/BUG-M01-CONVOY-ROCKET-DAMAGE` then ran the same production path on this \
     machine's GPU (Apple M3 Pro, Metal): its Rocket phase places the player ahead of a declared \
     hostile ground target, selects it through the Ctrl+R edge and fires `X`; the run's own \
     report.json records the new `weapon_rocket_damages` check passed — the proximity detonation \
     routed 2 hit(s) to t_truck01 on actor 1:9 and destroyed it — inside 72 checks with no \
     failures, and the framebuffer PNGs (the rocket-burst frame shows the detonation over the \
     convoy on the bridge) are hashed here as artifacts. The run is automated, never \
     `human_play`, and no original executable ran. This harness derives every field from the \
     recorded log, production discovery of $CS_GAME_DIR, the smoke's report and the hashes of \
     those artifacts. LIMITS: the selection is the mission's declared scripted-select (the \
     original's lock gesture is unmeasured), the rocket's tuning is the designed \
     m01-play-weapons.designed-rocket record rather than the original's, and the stack holds six \
     units of two each so three rockets is all a launch carries. Validated with \
     tools/validate_evidence.py --require-pass."
        .to_owned()
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_m01_convoy_rocket.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/BUG-M01-CONVOY-ROCKET-DAMAGE`
/// written relative to the workspace root in the module doc must be
/// re-anchored here.
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

/// Extracts the per-test results of the `accept_m01_convoy_rocket_` tests
/// from a recorded `cargo test` output.
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
        match finished(tail.trim()) {
            Some(status) => suite.assertions.push((name.to_owned(), status)),
            None => pending.push_back(name.to_owned()),
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
            '"' | '\\' => {
                out.push('\\');
                out.push(character);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
