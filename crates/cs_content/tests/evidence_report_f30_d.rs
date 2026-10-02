//! Evidence-report harness for task F30-D (#124), `docs/contracts/CLI-EVIDENCE.md`
//! and `schemas/evidence.schema.json`. Not named `accept_f30_d_*`: it is not
//! part of the acceptance suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_f30_d_ --include-ignored 2>&1 |
//!    tee private/evidence/F30-D/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F30-D \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f30_d_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_content --test evidence_report_f30_d -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/F30-D/acceptance.json
//!    --artifact-root private/evidence/F30-D --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/F30-D.json`.
//!
//! `targeting-vocabulary.json` is a second production observation of the
//! original installation: the `StringCatalog` accounting, the `.data`
//! `{name, id}` table size and id range, the measured target-command /
//! padlock / font label names with their ids and code-unit lengths, and the
//! action classification — ids, names, counts and digests only, never original
//! display text.
//!
//! **On `unknowns`.** The validator's `--require-pass` rejects a nonempty
//! `unknowns` list. This stage has *no unresolved issue inside its own
//! assertions*: every assertion below is a measured fact about shipped files or
//! a production behavior that the suite exercised, and all of them pass. The
//! original's target **order**, its **reveal rules** and its **assistance
//! behavior** are not such issues — they are *unmeasured original behavior*
//! that this stage recorded as fidelity limitations rather than as failures of
//! its own claims. They are named, with their claim ids, in `REVIEW_METHOD`
//! below (inside the report itself), in `targeting-vocabulary.json` (a hashed
//! artifact), in the committed finding
//! `docs/findings/2026-10-02-f30-d-target-order-reveal-and-assistance.md` and
//! in the follow-up tasks filed with Rally, so no limitation is removed from
//! machine-readable evidence to turn a validator green. The report's `claim` is
//! `implemented`: this stage awards nothing above that, and never
//! `verified_original`.

#[path = "f30_d_support/mod.rs"]
mod support;

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::config::{StringCatalog, StringLookup};
use cs_formats::ParseContext;
use cs_types::asset_id::SourceSpan;
use support::*;

/// The two retail tests this report's capabilities rest on: without both of
/// them passing, the report is not an observation of the installation.
const RETAIL_TESTS: [&str; 2] = [
    "accept_f30_d_retail_names_the_original_target_action_vocabulary",
    "accept_f30_d_retail_names_no_reveal_or_assistance_option",
];

const REVIEW_METHOD: &str = "Acceptance suite run locally with the retail capability; this report is derived from the recorded log plus a second, independent production pass of the same readers, recorded in targeting-vocabulary.json. MEASURED (ids, names, counts, digests only, no original display text): the installation and canonical-content fingerprints; strings.dll's RT_STRING accounting (112 blocks / 1792 units / language 1033); its PE .data {name, id} table of 1023 named entries with ids 100..17142; eleven MSG_CMD_TARGET_* command labels (a clear at 10008, an under-reticule pick at 10009, and a next/previous/nearest triple for each of the named classes enemy, ally and ground at 10015..10023) plus the MSG_TARGETING_CONTROLS category at 3009; a twelve-label padlock assist family (three mode commands and nine directions at 11025..11036); four target-display text fonts including an aim-point font; and two absences over the whole named table - no name contains REVEAL, VISIB, SENSOR, DETECT, HIDDEN, STEALTH, LEAD or ASSIST, so the shipped string image names no reveal or visibility concept and no lead-indicator or aim-assistance option. The project's DeclaredAction set is classified against that vocabulary exactly once per action; nearest_objective and nearest_attacker are the two rows with no name in the shipped observation, and absent means absent-from-the-observation, never that the original cannot do it. The two production tests are the AC04 half: crosshair selection needs eligibility and free sight together, and the reveal rule gates the cycle and the crosshair in one boundary. FIDELITY LIMITATIONS (unmeasured original behavior, recorded in targeting-vocabulary.json, in the committed finding and in the filed follow-up tasks; none of them is claimed by this report): claim f30.d.limit.target_order - the order the original's next/previous cycle walks is native data in the packed executable and is not in any shipped readable file; claim f30.d.limit.reveal_rules - the original's reveal/visibility rules are not named in the string vocabulary and not readable from any file; claim f30.d.limit.assistance_behavior - what the three padlock modes and the nine directions do is native, so no automatic hit correction is claimed anywhere (F30 non-negotiable 3); claim f30.d.limit.bindings - which key fires which target command is native (F22-H, task #505). The claim is implemented: a code and test pass awards nothing above that, and no agent review replaces the owner's human approval. Validated with tools/validate_evidence.py --require-pass.";

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_f30_d_writes_the_acceptance_report() {
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

    // Second observation: the same production readers, rendered without text.
    let bytes = fs::read(game_dir.join("strings.dll")).expect("strings.dll");
    let strings_sha256 = sha256(&bytes).to_hex();
    let span = SourceSpan::new(install, "strings.dll", None, 0, bytes.len() as u64, None)
        .expect("a valid span");
    let mut context = ParseContext::with_defaults("strings.dll");
    let catalog = StringCatalog::read(&mut context, span, &bytes).expect("strings.dll reads");
    let accounting = catalog.accounting();
    let table = parse_name_id_table(&bytes);
    assert_eq!(table.len(), NAME_ID_ENTRIES);

    let label = |name: &str, id: u32| -> String {
        assert_eq!(table.get(name).copied(), Some(id), "{name} id");
        let units = match catalog.resolve(id, Some(STRINGS_LANGUAGE)) {
            StringLookup::Found(row) => row.code_units.len(),
            other => panic!("{name} must resolve, got {other:?}"),
        };
        format!(
            "{{\"name\": {}, \"id\": {id}, \"code_units\": {units}}}",
            jstr(name)
        )
    };
    let targets: Vec<String> = TARGET_COMMAND_LABELS
        .iter()
        .map(|(name, id)| label(name, *id))
        .collect();
    let padlock: Vec<String> = PADLOCK_COMMAND_LABELS
        .iter()
        .map(|(name, id)| label(name, *id))
        .collect();
    let fonts: Vec<String> = TARGET_FONTS
        .iter()
        .map(|(name, id)| label(name, *id))
        .collect();
    let (category, category_id) = TARGETING_CATEGORY;
    let category_json = label(category, category_id);
    let classification: Vec<String> = ACTION_CLASSIFICATION
        .iter()
        .map(|(label, status, evidence)| {
            format!(
                "{{\"action\": {}, \"status\": {}, \"original\": {}}}",
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
    let absent_hits: Vec<String> = ABSENT_NAME_FRAGMENTS
        .iter()
        .map(|fragment| {
            let hits: Vec<String> = table
                .keys()
                .filter(|name| name.contains(fragment))
                .cloned()
                .collect();
            format!(
                "{{\"fragment\": {}, \"matches\": {}}}",
                jstr(fragment),
                str_array(&hits)
            )
        })
        .collect();

    let vocabulary_path = evidence_dir.join("targeting-vocabulary.json");
    fs::write(
        &vocabulary_path,
        format!(
            "{{\"install_sha256\": {}, \"content_sha256\": {}, \"candidate_tree\": {}, \"strings_dll\": {{\"sha256\": {}, \"len\": {}, \"accounting\": {{\"strings\": {}, \"blocks\": {}, \"undecodable\": {}, \"other_leaves\": {}, \"duplicate_ids\": {}}}, \"name_id_entries\": {}, \"name_id_min\": {}, \"name_id_max\": {}, \"targeting_category\": {category_json}, \"target_commands\": [{}], \"padlock_commands\": [{}], \"target_fonts\": [{}], \"absent_name_fragments\": [{}]}}, \"action_classification\": [{}], \"limitations\": [{}]}}\n",
            jstr(&install_sha256),
            jstr(&content_sha256),
            jstr(&candidate_tree),
            jstr(&strings_sha256),
            bytes.len(),
            accounting.strings,
            catalog.resources().strings().len(),
            accounting.undecodable,
            accounting.other_leaves,
            accounting.duplicate_ids,
            table.len(),
            NAME_ID_MIN,
            NAME_ID_MAX,
            targets.join(", "),
            padlock.join(", "),
            fonts.join(", "),
            absent_hits.join(", "),
            classification.join(", "),
            limitations_json(),
        ),
    )
    .expect("write targeting-vocabulary.json");

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
        "{{\n \"schema_version\": 1,\n \"task_id\": \"F30-D\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report must NOT validate",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// The fidelity limitations this stage records rather than resolves. Each names
/// a claim id, the original behavior that stays unmeasured, the content it
/// gates and the task that would resolve it. They are unmeasured *original
/// behavior*, not failures of this stage's assertions, and they are written
/// into the hashed artifact so they cannot be lost with the report.
fn limitations_json() -> String {
    const LIMITATIONS: [(&str, &str, &str, &str); 4] = [
        (
            "f30.d.limit.target_order",
            "the order the original's target next/previous cycle walks over its enemy, ally and ground classes",
            "F30 AC01 / non-negotiable 2 cycle order; the engine orders by distance with an actor-id tie-break",
            "#505 (original default bindings, same native-data limit) and #358 (owner original run)",
        ),
        (
            "f30.d.limit.reveal_rules",
            "the original's reveal and visibility rules",
            "F30 non-negotiable 1 reveal axis; the engine's reveal flag is designed vocabulary",
            "#358 (owner original run): no shipped file names a reveal or visibility concept",
        ),
        (
            "f30.d.limit.assistance_behavior",
            "what the original's three padlock modes and nine padlock directions do",
            "F30 non-negotiable 3 lead indicator and aim assistance; the engine offers neither as measured behavior",
            "#358 (owner original run): the padlock family is named but native",
        ),
        (
            "f30.d.limit.bindings",
            "which physical key fires which target command",
            "F30-B declared selection actions bound to FlightCommand edges",
            "#505 F22-J (original default control bindings)",
        ),
    ];
    LIMITATIONS
        .iter()
        .map(|(claim, behavior, gates, resolving)| {
            format!(
                "{{\"claim\": {}, \"unmeasured\": {}, \"gates\": {}, \"resolving_task\": {}}}",
                jstr(claim),
                jstr(behavior),
                jstr(gates),
                jstr(resolving)
            )
        })
        .collect::<Vec<String>>()
        .join(", ")
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_content/tests/evidence_report_f30_d.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F30-D` written relative to the
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
/// `accept_f30_d_` tests from a recorded `cargo test` output.
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
            if !name.starts_with("accept_f30_d_") {
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
                 \"targeting-vocabulary.json\"]}}",
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
