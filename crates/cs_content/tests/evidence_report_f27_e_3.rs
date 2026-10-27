//! Evidence-report harness for task F27-E.3 (#550), `docs/contracts/CLI-EVIDENCE.md`
//! and `schemas/evidence.schema.json`. Not named `accept_f27_e_3_*`: it is not
//! part of the acceptance suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --locked -p cs_content -- accept_f27_e_3_ --include-ignored 2>&1 |
//!    tee private/evidence/F27-E.3/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F27-E.3 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --locked -p cs_content -- accept_f27_e_3_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_content --test cs_content_tests evidence_report_f27_e_3 -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/F27-E.3/acceptance.json
//!    --artifact-root private/evidence/F27-E.3 --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/F27-E.3.json`.
//!
//! `armor-vocabulary.json` is a second production observation of the armor
//! screen: the four positions with their title string ids, layout rows and
//! dropdown keys, the point count, the purchase columns and the engine callback
//! ids, plus the `f27.d.limit.gun_group_assignment` standing, which this stage
//! leaves deferred. Numbers, identifiers and spans only.
//!
//! **On `unknowns`.** Empty: no assertion in the report is unresolved. What the
//! stage does *not* resolve (which gun group sits at which position, any armor
//! amount, any zone) is named in `REVIEW_METHOD` and kept deferred in `claims`.
//! Claim: `implemented`, never `verified_original`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::damage::read_original_armor_vocabulary;
use cs_content::weapons::{LimitEvidence, LimitOutcome, OriginalLimitClaim, OriginalLimitReport};

use std::collections::VecDeque;

const RETAIL_TESTS: [&str; 1] = [
    "accept_f27_e_3_armor_positions::accept_f27_e_3_retail_armor_vocabulary_is_four_positions_four_points",
];

const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability (CS_GAME_DIR set, CS_CAPABILITIES includes retail), plus a second production pass recorded in armor-vocabulary.json. MEASURED: ASSETS/SCRIPTS/ARMOR.SCRIPT in GOSDATA/ASSETS/crimson.rof builds four armor titles (ar_t_nosetitle, ar_t_tailtitle, ar_t_lefttitle, ar_t_righttitle, in that order), declares object X[4] and builds four ar_d_point dropdowns asking the engine through callback 2247, and asks for the armor name and description through callbacks 2220 and 2225; ASSETS/LAYOUT.CSV places each title (IDS_AR_*_TITLE) and each dropdown below its own title and above the next, and declares PUR_T_ARMORLIST, PUR_T_ARMORWEIGHT and PUR_T_ARMORCOST in [@Purchase@]; RESOURCE.H declares IDS_PX_ARMORUNITS. All read through cs_content::damage::read_original_armor_vocabulary with observed_tool provenance naming the member's stored extent and the decoded line. NOT CLAIMED: this is an armor vocabulary. No shipped file maps a gun group onto a position, so f27.d.limit.gun_group_assignment stays deferred, DeclaredGunMountKind gains no side, and OriginalArmorVocabulary::gun_group_for answers Resolved::Unknown. No armor amount, zone or damage rule is recorded (F29 owns that model). Nothing is verified_original and no agent review replaces the owner's human approval.";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_f27_e_3_writes_the_acceptance_report() {
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

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", log_path.display()));
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && suite.assertions.len() as u64 >= suite.passed,
        "the acceptance log was not understood: {suite:?}"
    );
    for retail_test in RETAIL_TESTS {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: run step 1 with --include-ignored and CS_GAME_DIR"
                )
            });
        assert_eq!(status, "pass", "{retail_test}");
    }

    let manifest = discover(&game_dir)
        .expect("discover the installation")
        .manifest;
    let install_sha256 = fingerprint(&manifest).to_hex();
    let content_sha256 = content_fingerprint(&manifest).to_hex();

    // Second observation: the production reader again.
    let vocabulary = read_original_armor_vocabulary(&game_dir).expect("the vocabulary reads");
    assert_eq!(vocabulary.positions().len(), 4);
    assert_eq!(vocabulary.point_count(), 4);
    let mut report = OriginalLimitReport::new();
    report
        .record(
            OriginalLimitClaim::GunGroupAssignment,
            LimitEvidence::Unmeasurable {
                reason: OriginalLimitClaim::GunGroupAssignment
                    .deferral_reason()
                    .to_owned(),
            },
        )
        .expect("deferred");
    report
        .refile(
            OriginalLimitClaim::GunGroupAssignment,
            "unchanged by #550 F27-E.3: the armor positions are a measured lead, not a resolution; still re-filed to #358 REF-OWNER-FIRST-CAPTURE",
        )
        .expect("re-filed");
    assert!(report.bound().is_empty());

    let positions = vocabulary
        .positions()
        .iter()
        .map(|record| {
            format!(
                "{{\"position\": {}, \"title_string_id\": {}, \"title_row_y\": {}, \"point_slot\": {}, \"point_key\": {}, \"point_row_y\": {}, \"script_line\": {}, \"layout_title_line\": {}, \"layout_point_line\": {}}}",
                jstr(&record.position().to_string()),
                record.title_string_id(),
                record.title_row_y(),
                record.point_slot(),
                jstr(record.point_control_key()),
                record.point_row_y(),
                record.script_title().line(),
                record.layout_title().line(),
                record.layout_point().line(),
            )
        })
        .collect::<Vec<String>>()
        .join(", ");
    let columns = vocabulary
        .purchase_columns()
        .iter()
        .map(|column| {
            format!(
                "{{\"key\": {}, \"layout_line\": {}}}",
                jstr(column.column().layout_key()),
                column.layout().line()
            )
        })
        .collect::<Vec<String>>()
        .join(", ");
    let claims = report
        .rows()
        .iter()
        .map(|row| {
            let (verdict, refiled) = match row.outcome() {
                LimitOutcome::Bound { .. } => ("bound", None),
                LimitOutcome::Deferred { refiled_to, .. } => ("deferred", refiled_to.clone()),
            };
            format!(
                "{{\"claim\": {}, \"verdict\": {}, \"refiled_to\": {}}}",
                jstr(row.claim_id()),
                jstr(verdict),
                refiled.map_or_else(|| "null".to_owned(), |target| jstr(&target))
            )
        })
        .collect::<Vec<String>>()
        .join(", ");
    let surface_path = evidence_dir.join("armor-vocabulary.json");
    fs::write(
        &surface_path,
        format!(
            "{{\"install_sha256\": {}, \"content_sha256\": {}, \"candidate_tree\": {}, \"container\": \"GOSDATA/ASSETS/crimson.rof\", \"point_count\": {}, \"point_callback\": {}, \"name_callback\": {}, \"description_callback\": {}, \"units_string_id\": {}, \"positions\": [{positions}], \"purchase_columns\": [{columns}], \"claims\": [{claims}]}}\n",
            jstr(&install_sha256),
            jstr(&content_sha256),
            jstr(&candidate_tree),
            vocabulary.point_count(),
            vocabulary.point_callback().id(),
            vocabulary.name_callback().id(),
            vocabulary.description_callback().id(),
            vocabulary.units_string_id(),
        ),
    )
    .expect("write armor-vocabulary.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&surface_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report_json = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"F27-E.3\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        jstr(&reviewer),
        jstr(REVIEW_METHOD),
    );
    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report_json).expect("write acceptance.json");
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report must NOT validate",
        suite.failed
    );
    println!("wrote {}", out.display());
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_content/tests/evidence_report_f27_e_3.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F27-E.1` written relative to the
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

// ------------------------------------------------------------ log parsing ---

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
/// `accept_f27_e_3_` tests from a recorded `cargo test` output.
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
            if !name.starts_with("accept_f27_e_3_") {
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

/// One referenced artifact: hashed here with the production SHA-256 the task
/// implements (the validator re-hashes it with `hashlib` independently).
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
                 \"armor-vocabulary.json\"]}}",
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
/// accepts after the validator's `Z` -> `+00:00` replacement.
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
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if m <= 2 { y + 1 } else { y },
        m as u32,
        d as u32,
        (rest / 3600) as u32,
        ((rest % 3600) / 60) as u32,
        (rest % 60) as u32,
    )
}
