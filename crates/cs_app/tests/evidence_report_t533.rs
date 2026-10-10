//! Evidence-report harness for task #533: `docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`. Not named `accept_t533_*`: it is not
//! part of the acceptance suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_t533_ --include-ignored 2>&1 |
//!    tee private/evidence/T533/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T533 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_t533_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_t533 -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/T533/acceptance.json
//!    --artifact-root private/evidence/T533 --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/T533.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR`, `rustc` and `Cargo.lock`. The second artifact
//! is a **second production observation**: the harness re-runs
//! [`survey_retail_stunt_authority`] and records, per reader archive, the
//! objective-record count, the count task #533's label rule selects and the
//! count the label-only reading selects — so the decision this task made (the
//! union of the two measured labels) is a production measurement over the
//! installation, not a paraphrase of the acceptance assertions. The three
//! help-only campaign records are listed by container, description and node.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::stunts::survey_retail_stunt_authority;
use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::stunts::{
    FLY_THROUGH_CATEGORY_LABEL, FLY_THROUGH_HELP_LABEL, SCENARIO_TARGETS_MEMBER, decode_zrd,
    scenario_fly_through_targets,
};
use cs_formats::script_raw::discover_container;
use cs_types::install::RelativePath;

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_t533_";

const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability; this harness derives every field \
     from the recorded log, production discovery of $CS_GAME_DIR, and a second production run of \
     cs_app::stunts::survey_retail_stunt_authority over every reader archive of the installation \
     (label-rule-census.json): per reader, the objective-record count, the count task #533's label rule \
     (either measured label) selects and the count the label-only reading selects, plus the three \
     help-only campaign records named by container, description and node. Claim is implemented only: \
     the label RULE is decided and measured, but no original run happened, so nothing here is evidence \
     of the original's runtime behaviour — whether the original's engine awarded a stunt for these \
     three campaign objectives is unrecorded, and RETAIL here is read access to files. LIMITS OF WHAT \
     WAS MEASURED, each outside this task's scope and recorded in \
     docs/findings/2026-10-10-t533-fly-through-label-rule.md: (1) the campaign dzones.zrd bindings \
     (objective_numbers, disable, nosnapshot) remain unmeasured — task #513 owns that member's framing; \
     (2) the zone-crossing predicate, direction and clearance rules, the payout, the repeat policy and \
     the linked photo remain unmeasured (F42-A/#464); (3) which aircraft a zone crossing credits \
     remains unmeasured (#465); (4) the three help-only records are wired into their missions' \
     objective machines (ADD/REMOVE_OBJECTIVE_TARGET, and TRAVELERS on C5/M02's dz1) — that is a \
     statement about the data, not about runtime behaviour. Validated with \
     tools/validate_evidence.py --require-pass.";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_t533_writes_the_acceptance_report() {
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
        suite.passed > 0 && suite.assertions.len() as u64 >= suite.passed,
        "the acceptance log was not understood: {suite:?}"
    );

    // The source fingerprints, from production discovery.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The second production observation: the label rule, per reader, over the
    // whole installation.
    let survey = survey_retail_stunt_authority(&game_dir).expect("the retail corpus surveys");
    assert_eq!(
        (
            survey.fly_through_objectives(),
            survey.fly_through_labelled_objectives()
        ),
        (67, 67),
        "task #533's rule selects every labelled record over this installation"
    );
    let mut rows = Vec::new();
    let mut help_only = Vec::new();
    for row in survey.rows() {
        let Some(corpus) = row.objectives() else {
            rows.push(format!(
                "{{\"container\": {}, \"objective_records\": null, \"fly_through\": null, \
                 \"fly_through_labelled\": null}}",
                jstr(row.container())
            ));
            continue;
        };
        rows.push(format!(
            "{{\"container\": {}, \"objective_records\": {}, \"fly_through\": {}, \
             \"fly_through_labelled\": {}}}",
            jstr(row.container()),
            corpus.count(),
            corpus.fly_through(),
            corpus.fly_through_labelled()
        ));
        if corpus.fly_through() > 0 {
            // The record itself is read through the production decoder and
            // selector, and help-only records are listed by name.
            let Some(record) = found
                .manifest
                .files
                .iter()
                .find(|record| record.relative_spelling.logical_key() == row.container())
            else {
                continue;
            };
            let spelling = record.relative_spelling.as_str();
            let bytes = fs::read(found.manifest.host_root.join(spelling))
                .unwrap_or_else(|error| panic!("read {spelling}: {error}"));
            let Ok(path) = RelativePath::new(spelling) else {
                continue;
            };
            let discovery = discover_container(row.container(), &path, &bytes);
            let Some(targets) = discovery
                .programs()
                .iter()
                .find(|program| program.locator().member() == Some(SCENARIO_TARGETS_MEMBER))
            else {
                continue;
            };
            let Ok(root) = decode_zrd(targets.bytes()) else {
                continue;
            };
            for target in scenario_fly_through_targets(&root) {
                if target.category_label.is_none() {
                    help_only.push(format!(
                        "{{\"container\": {}, \"description\": {}, \"zone_label\": {}, \
                         \"category_label\": null, \"help_label\": {}}}",
                        jstr(row.container()),
                        jstr(&target.description),
                        jstr(&target.zone_label),
                        option_str(target.help_label.as_deref())
                    ));
                }
            }
        }
    }
    let census_path = evidence_dir.join("label-rule-census.json");
    fs::write(
        &census_path,
        format!(
            "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"readers\": {}, \
             \"fly_through\": {}, \"fly_through_labelled\": {}, \"help_only_records\": {}, \
             \"labels\": {{\"category\": {}, \"help\": {}}}, \"rows\": {}}}\n",
            jstr(&install_sha256),
            jstr(&candidate_tree),
            survey.len(),
            survey.fly_through_objectives(),
            survey.fly_through_labelled_objectives(),
            object_array(&help_only),
            jstr(FLY_THROUGH_CATEGORY_LABEL),
            jstr(FLY_THROUGH_HELP_LABEL),
            object_array(&rows),
        ),
    )
    .expect("write label-rule-census.json");
    // The census is a committed evidence artifact, so it has to be valid JSON.
    let census_text = fs::read_to_string(&census_path).expect("re-read label-rule-census.json");
    assert_well_formed_json(&census_text, &census_path.display().to_string());
    assert_eq!(
        help_only.len(),
        3,
        "the installation carries exactly the three help-only records this task named"
    );

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&census_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"T533\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": {},\n \"artifacts\": {},\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
    fs::write(&out, &report).expect("write acceptance.json");
    assert_well_formed_json(&report, &out.display().to_string());
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
             (crates/cs_app/tests/evidence_report_t533.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/T533` written relative to the
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
    panic!("package {package:?} is in {}", lock_path.display());
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
/// `accept_t533_` tests from a recorded `cargo test` output.
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
            if !name.starts_with(ACCEPTANCE_PREFIX) {
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

/// Whether a rendered JSON document is well formed: balanced braces and
/// brackets outside strings, no empty value where one is required, no trailing
/// comma.
///
/// This is not a JSON parser. It is the small set of properties a hand-rolled
/// renderer of this shape breaks, and it is checked in the harness that writes
/// the document, because a malformed evidence artifact would otherwise be
/// committed as if it were a measurement.
fn assert_well_formed_json(text: &str, what: &str) {
    let mut braces = 0_i64;
    let mut brackets = 0_i64;
    let mut in_string = false;
    let mut escaped = false;
    let mut previous = '\0';
    let bytes = text.as_bytes();
    for (index, character) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            previous = character;
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' => braces += 1,
            '}' => braces -= 1,
            '[' => brackets += 1,
            ']' => brackets -= 1,
            // `": ,`, `": }` and `": ]`: an empty value where the grammar
            // requires one, which is what an unbracketed empty list produces.
            ':' => {
                let next = bytes.get(index + 1).map(|byte| char::from(*byte));
                assert!(
                    !matches!(next, Some(',' | '}' | ']')),
                    "{what}: a JSON key with an empty value at byte {index}"
                );
            }
            _ => {}
        }
        assert!(
            braces >= 0 && brackets >= 0,
            "{what}: unbalanced JSON at byte {index} ({character:?})"
        );
        if previous == ',' {
            assert!(
                !matches!(character, '}' | ']'),
                "{what}: a trailing comma before byte {index}"
            );
        }
        previous = character;
    }
    assert!(
        !in_string && braces == 0 && brackets == 0,
        "{what}: unbalanced JSON (braces {braces}, brackets {brackets}, in string {in_string})"
    );
}

/// Already-rendered JSON objects as a JSON array, brackets included.
///
/// The brackets must come from here, not from a bare `join(", ")`: an empty
/// collection joined without them writes `"key": ,`, which is **not valid
/// JSON**, and the census is a committed evidence artifact that has to parse.
fn object_array(items: &[String]) -> String {
    // Every element must be a rendered **object**. An element that is itself an
    // array means a caller passed something already bracketed — a double wrap,
    // which is still valid JSON and therefore slips past a grammar check.
    assert!(
        items
            .iter()
            .all(|item| item.starts_with('{') && item.ends_with('}')),
        "object_array takes rendered objects, got {:?}",
        items
            .iter()
            .find(|item| !(item.starts_with('{') && item.ends_with('}')))
    );
    format!("[{}]", items.join(", "))
}

/// The assertion list of the report.
fn assertion_array(assertions: &[(String, &'static str)]) -> String {
    let items: Vec<String> = assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\", \
                 \"label-rule-census.json\"]}}",
                jstr(name)
            )
        })
        .collect();
    object_array(&items)
}

/// The artifact list of the report.
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
    object_array(&items)
}

/// A list of strings as a JSON array.
fn str_array(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| jstr(item)).collect();
    format!("[{}]", quoted.join(", "))
}

fn option_str(value: Option<&str>) -> String {
    match value {
        Some(value) => jstr(value),
        None => "null".to_owned(),
    }
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
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
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
