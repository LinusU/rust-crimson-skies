//! Evidence-report harness for task F22-H (#411), `docs/contracts/CLI-EVIDENCE.md`
//! and `schemas/evidence.schema.json`. Not named `accept_f22_h_*`: it is not
//! part of the acceptance suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_f22_h_ --include-ignored 2>&1 |
//!    tee private/evidence/F22-H/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F22-H \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f22_h_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_content --test evidence_report_f22_h -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/F22-H/acceptance.json
//!    --artifact-root private/evidence/F22-H
//!    --require-pass` (expected to exit 3 with `Unresolved issues`: the
//!    default bindings and the runtime questions are the deliverable's
//!    unknowns and must not be deleted to turn the flag green).
//! 4. Commit a copy as `docs/findings/evidence/F22-H.json`.
//!
//! `vocabulary.json` is the production `StringCatalog` accounting, the `.data`
//! `{name, id}` command table, the `crimson.icd` key pool summary and the
//! control-script digests — ids, names, counts and digests only, never
//! original display text.

#[path = "f22_h_support/mod.rs"]
mod support;

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::config::{StringCatalog, StringLookup};
use cs_formats::{ParseContext, RofLimits, read_member, read_tree};
use cs_types::asset_id::SourceSpan;
use support::*;

const RETAIL_TESTS: [&str; 2] = [
    "accept_f22_h_retail_strings_dll_names_the_original_command_vocabulary",
    "accept_f22_h_retail_the_game_executable_and_control_scripts_expose_the_key_vocabulary",
];

const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability; the report is derived from the recorded log, production discovery of $CS_GAME_DIR and a second production run of the readers recorded in vocabulary.json. Claim is implemented only: the observable original control vocabulary (65 named command labels across the two control ranges, their 15/50 label counts, the control categories, the two device-family modes, the key/button labels, the 120-name executable key pool and the control scripts) is measured; the default bindings and the runtime questions are NOT observable from files and are kept in `unknowns`. LIMITS (all outside F22-H's file-readable scope and listed in docs/findings/2026-10-02-f22-h-original-control-vocabulary.md): (1) the original's default bindings are native data in the packed executable, not shipped as a readable file (task #358 REF-OWNER-FIRST-CAPTURE); (2) the ordered command list and the mapping of each label id to a command slot are produced by native callbacks 2115/2120/2139/2140; (3) which key fires in the running game, text-entry suppression and focus-loss behavior need an owner-supplied original run. The report is validated with tools/validate_evidence.py; --require-pass exits 3 on the preserved unknowns.";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_f22_h_writes_the_acceptance_report() {
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
            .unwrap_or_else(|| panic!("{retail_test} did not run: run with --include-ignored"));
        assert_eq!(status, "pass", "{retail_test}");
    }

    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install = fingerprint(&found.manifest);
    let install_sha256 = install.to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // Second observation: the production readers, rendered without text.
    let strings_path = game_dir.join("strings.dll");
    let strings = fs::read(&strings_path).expect("strings.dll");
    let strings_sha256 = sha256(&strings).to_hex();
    let span = SourceSpan::new(install, "strings.dll", None, 0, strings.len() as u64, None)
        .expect("a valid span");
    let mut context = ParseContext::with_defaults("strings.dll");
    let catalog = StringCatalog::read(&mut context, span, &strings).expect("strings.dll reads");
    let accounting = catalog.accounting();
    let table = parse_name_id_table(&strings);
    assert_eq!(table.len(), NAME_ID_ENTRIES);

    let icd = fs::read(game_dir.join("crimson.icd")).expect("crimson.icd");
    let icd_sha256 = sha256(&icd).to_hex();
    let key_names = parse_icd_key_names(&icd);
    assert_eq!(key_names.len(), ICD_KEY_NAMES);

    let rof = fs::read(game_dir.join(CRIMSON_ROF)).expect(CRIMSON_ROF);
    let mut rof_context = ParseContext::with_defaults(CRIMSON_ROF);
    let tree = read_tree(&mut rof_context, &rof).expect("the container's tree walks");
    assert_eq!(tree.members().len(), CRIMSON_ROF_MEMBERS);
    let mut scripts = Vec::new();
    for (spelling, length, digest) in CONTROL_SCRIPTS {
        let wanted: Vec<&str> = spelling.split('/').collect();
        let member = tree
            .members()
            .iter()
            .find(|entry| {
                entry.path.len() == wanted.len()
                    && entry
                        .path
                        .iter()
                        .zip(&wanted)
                        .all(|(segment, name)| segment.eq_ignore_ascii_case(name.as_bytes()))
            })
            .unwrap_or_else(|| panic!("{spelling}: no such member"));
        let read = read_member(&rof_context, &rof, member, &RofLimits::default())
            .expect("the control script decodes");
        assert_eq!(read.data.len() as u64, length);
        assert_eq!(sha256(&read.data).to_hex(), digest);
        scripts.push(format!(
            "{{\"member\": {}, \"decoded_len\": {}, \"sha256\": {}}}",
            jstr(spelling),
            length,
            jstr(digest)
        ));
    }

    let command_items: Vec<String> = COMMAND_LABELS
        .iter()
        .map(|(name, id)| {
            let row = match catalog.resolve(*id, Some(STRINGS_LANGUAGE)) {
                StringLookup::Found(row) => row,
                other => panic!("{name} must resolve, got {other:?}"),
            };
            format!(
                "{{\"name\": {}, \"id\": {id}, \"code_units\": {}}}",
                jstr(name),
                row.code_units.len()
            )
        })
        .collect();
    let extra_labels: Vec<u32> = (11_000..=11_100)
        .filter(|id| match catalog.resolve(*id, Some(STRINGS_LANGUAGE)) {
            StringLookup::Found(row) => !row.code_units.is_empty(),
            StringLookup::Missing => false,
            StringLookup::Ambiguous(_) => false,
        })
        .collect();
    let comparison_items: Vec<String> = COMPARISON
        .iter()
        .map(|(label, status, evidence)| {
            format!(
                "{{\"command\": {}, \"status\": {}, \"original\": {}}}",
                jstr(label),
                jstr(status),
                str_array(
                    &evidence
                        .iter()
                        .map(|name| (*name).to_owned())
                        .collect::<Vec<String>>()
                )
            )
        })
        .collect();
    let vocabulary_path = evidence_dir.join("vocabulary.json");
    fs::write(
        &vocabulary_path,
        format!(
            "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"strings_dll\": {{\"sha256\": {}, \"len\": {}, \"accounting\": {{\"strings\": {}, \"blocks\": {}, \"undecodable\": {}, \"other_leaves\": {}, \"duplicate_ids\": {}}}, \"name_id_entries\": {}, \"command_label_count\": {}, \"command_labels\": [{}], \"original_only_commands\": [{}], \"control_label_ids\": [{}]}}, \"crimson_icd\": {{\"sha256\": {}, \"len\": {}, \"key_names\": {}}}, \"crimson_rof\": {{\"member_count\": {}, \"control_scripts\": [{}]}}, \"comparison\": [{}]}}\n",
            jstr(&install_sha256),
            jstr(&candidate_tree),
            jstr(&strings_sha256),
            strings.len(),
            accounting.strings,
            catalog.resources().strings().len(),
            accounting.undecodable,
            accounting.other_leaves,
            accounting.duplicate_ids,
            table.len(),
            COMMAND_LABELS.len(),
            command_items.join(", "),
            str_array(
                &ORIGINAL_ONLY_COMMANDS
                    .iter()
                    .map(|name| (*name).to_owned())
                    .collect::<Vec<String>>()
            ),
            id_array(&extra_labels),
            jstr(&icd_sha256),
            icd.len(),
            key_names.len(),
            tree.members().len(),
            scripts.join(", "),
            comparison_items.join(", ")
        ),
    )
    .expect("write vocabulary.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&vocabulary_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"F22-H\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": {},\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        str_array(&unknowns()),
        jstr(&reviewer),
        jstr(REVIEW_METHOD),
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

/// The unresolved questions this task must not delete to satisfy a validator:
/// the default bindings and the runtime behavior are not file-observable.
fn unknowns() -> Vec<String> {
    vec![
        "The original's default bindings (which key, mouse button/axis, gamepad or joystick control each command binds to) are not observable from the shipped files: the control UI fetches every row from native callbacks in the packed executable, and no control-profile file exists in the installation.".to_owned(),
        "The ordered command list the control UI enumerates, and the mapping from each measured label id to a command slot, are produced by native callbacks 2115/2120/2139/2140 and are not in any readable file.".to_owned(),
        "Which key actually fires which command in the running game, what the original does while text is entered, and what it does on focus loss, are runtime questions; they need an owner-supplied original run (task #358 REF-OWNER-FIRST-CAPTURE).".to_owned(),
    ]
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_content/tests/evidence_report_f22_h.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F22-H` written relative to the
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
/// `accept_f22_h_` tests from a recorded `cargo test` output.
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
            if !name.starts_with("accept_f22_h_") {
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
                 \"vocabulary.json\"]}}",
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

fn id_array(items: &[u32]) -> String {
    let quoted: Vec<String> = items.iter().map(u32::to_string).collect();
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

// Keep the shared-module import used even when a build reads only part of it.
#[allow(unused_imports)]
use cs_types::input::FlightCommand as _F22HFlightCommand;
#[allow(unused_imports)]
use cs_types::input::UiAction as _F22HUiAction;
#[allow(unused_imports)]
use std::collections::BTreeMap as _F22HBTreeMap;
