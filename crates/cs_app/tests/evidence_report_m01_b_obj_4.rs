//! Evidence-report harness for task `M01-B-OBJ-4` (#1361):
//! `docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`.
//! Not named `accept_m01_b_obj_4_*`: it is not part of the acceptance suite
//! and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_m01_b_obj_4_ --include-ignored 2>&1 |
//!    tee private/evidence/M01-B-OBJ-4/cargo-test.log` (note the exit
//!    status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M01-B-OBJ-4 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_b_obj_4_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!    CS_GAME_DIR=<installation> \
//!      cargo test --locked -p cs_app --test evidence_report_m01_b_obj_4 -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py
//!    private/evidence/M01-B-OBJ-4/acceptance.json
//!    --artifact-root private/evidence/M01-B-OBJ-4 --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/M01-B-OBJ-4.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness re-runs
//! [`recover_retail_objectives`] on `zbd/c1c/m01` and records the per-key
//! directive census — every spelled site of the 19 measured keys with its
//! family — plus the parsed record-level fields, as JSON. That is a real
//! production run over the owner's installation, not a paraphrase of the
//! acceptance assertions, and it carries no original bytes: keys, positions,
//! shapes and values only.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::objectives::{ObjectiveRecovery, RecordFieldFamily, recover_retail_objectives};
use cs_assets::install::{content_fingerprint, discover, fingerprint};
use cs_content::mission_control::measured_directive;
use cs_content::objectives::{AnimDirective, MeasuredRecordFields, RecordFieldError, RecordScalar};

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_m01_b_obj_4_";

/// The 19 keys the task measures, with M01's spelled site counts — asserted
/// verbatim by `accept_m01_b_obj_4_` tests, so a report regenerated on an
/// installation whose census disagrees can never be written.
const TASK_KEYS: &[(&str, usize)] = &[
    ("MISSION_TIMER", 1),
    ("PLAYER_INIT", 1),
    ("RESTORE_ANIMS", 1),
    ("EXECUTE_ANIMS", 1),
    ("INVALIDATE_ANIMS", 1),
    ("ADD_OBJECTIVE_TARGET", 4),
    ("REMOVE_OBJECTIVE_TARGET", 4),
    ("ADD_OTHER_TARGET", 3),
    ("ANIM_STATE", 3),
    ("COMPLETED_STOPPOINT", 1),
    ("DEDG", 8),
    ("SET_AI_NET", 3),
    ("SET_HELP_LABEL", 2),
    ("STOP_QUEUED_SOUNDS", 3),
    ("TRAVELERS", 1),
    ("WAKEUP_ENEMIES", 1),
    ("WAKEUP_GENERATOR", 4),
    ("WAKEUP_ZEP_TURRETS", 3),
    ("WAKE_ANIM", 5),
];

/// How this run was reviewed, with every measured number **derived** from the
/// recovery this same run produced rather than written down.
fn review_method(recovery: &ObjectiveRecovery, own_tests: usize) -> String {
    let families = recovery.unrecovered_by_family();
    let record_directives = families
        .get(&RecordFieldFamily::RecordDirective)
        .copied()
        .unwrap_or_default();
    let control_directives = families
        .get(&RecordFieldFamily::ControlDirective)
        .copied()
        .unwrap_or_default();
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field \
         from the recorded log, production discovery of $CS_GAME_DIR, and a second production run \
         of cs_app::objectives::recover_retail_objectives on zbd/c1c/m01 (directive-census.json). \
         Claim is implemented only. MEASURED: the findings document \
         docs/findings/2026-10-12-m01-b-obj-4-mission-language-directive-keys.md records the \
         engine-image parse sites of the five record-level keys in $CS_ENGINE_IMAGE (sha256 \
         43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75) and joins them with the \
         M01-LC-DIRECTIVE findings' measured block-level directive table; the production census \
         reports the 19 task keys as {record_directives} record-level fields and \
         {control_directives} block-level directive sites in M01, each classified by its measured \
         family rather than a generic Unmeasured, and the acceptance suite asserts the shapes, \
         counts, values and families key for key against that census, so a drift between the \
         document and the data fails the run. The native-side evidence is STATIC CODE EVIDENCE \
         ONLY: parse sites, field offsets, argument arity and the transform constants were read \
         out of the executable with r2 and no original program was run, so nothing here is \
         verified_original runtime behaviour. LIMITS OF WHAT WAS MEASURED, each recorded in the \
         findings document's Unknowns section: (1) PLAYER_INIT elements 3 and 4 have no measured \
         consumer and element 0's selection is a measured correlation, not an observed use; \
         (2) the three animation-list stores are measured but their consumers are untraced and \
         the lists' semantic differences are a name reading only; (3) DEDG's evaluator rewrites \
         three member fields whose consumers are untraced world state; (4) TRAVELERS' \
         opposite-polarity spelling, the counting mode's group id 0 and WAKE_ANIM's three \
         trailing call arguments stay unknown; (5) a record key spelled only inside a block \
         would be found by the engine's descending first-match lookup but not by this reader's \
         top-level scan — no retail record spells one. `unknowns` is empty because every spelled \
         site of the 19 keys reached a measured family. TEST-SELECTION NOTE: the prefix \
         accept_m01_b_obj_4_ is unique to this task, so the {own_tests} discovered assertions are \
         exactly this task's tests. Validated with tools/validate_evidence.py --require-pass.",
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_b_obj_4_writes_the_acceptance_report() {
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

    // The source fingerprints, from production discovery.
    let found = discover(&game_dir).expect("production discovery reads the installation");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The second production observation: M01's recovery — the 19 task keys'
    // spelled sites with their measured families, and the parsed record-level
    // fields.
    let recovery = recover_retail_objectives(&game_dir, "zbd/c1c/m01").expect("M01 reads");
    for (key, sites) in TASK_KEYS {
        let seen = recovery
            .unrecovered()
            .iter()
            .filter(|field| field.key == *key)
            .count();
        assert_eq!(
            seen, *sites,
            "the census no longer reports the sites the document maps for {key}"
        );
    }
    let census_path = evidence_dir.join("directive-census.json");
    fs::write(
        &census_path,
        format!(
            "{}\n",
            render_directive_census(&recovery, &install_sha256, &candidate_tree)
        ),
    )
    .expect("write directive-census.json");

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
        "{{\n \"schema_version\": 1,\n \"task_id\": \"M01-B-OBJ-4\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        jstr(&review_method(&recovery, suite.discovered as usize)),
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

/// M01's census of the 19 task keys as JSON: every spelled site with its block
/// position and measured family, the record-level parse and the measured
/// operations the block-level keys carry. No original bytes.
fn render_directive_census(
    recovery: &ObjectiveRecovery,
    install_sha256: &str,
    candidate_tree: &str,
) -> String {
    let keys: Vec<String> = TASK_KEYS
        .iter()
        .map(|(key, sites)| {
            let positions: Vec<String> = recovery
                .unrecovered()
                .iter()
                .filter(|field| field.key == *key)
                .map(|field| {
                    format!(
                        "{{\"block\": {}, \"family\": {}}}",
                        field
                            .block
                            .map_or_else(|| "null".to_owned(), |block| block.to_string()),
                        jstr(&format!("{:?}", field.family)),
                    )
                })
                .collect();
            let measured = measured_directive(key).map_or_else(
                || "null".to_owned(),
                |directive| {
                    let unknowns: Vec<String> =
                        directive.unknowns.iter().map(|u| jstr(u)).collect();
                    format!(
                        "{{\"operation\": {}, \"summary\": {}, \"unknowns\": [{}]}}",
                        jstr(directive.operation.code()),
                        jstr(directive.summary),
                        unknowns.join(", "),
                    )
                },
            );
            format!(
                "{{\"key\": {}, \"sites\": {sites}, \"fields\": [{}], \"measured\": {measured}}}",
                jstr(key),
                positions.join(", "),
            )
        })
        .collect();
    let families: Vec<String> = recovery
        .unrecovered_by_family()
        .iter()
        .map(|(family, count)| {
            format!(
                "{{\"family\": {}, \"unrecovered\": {count}}}",
                jstr(&format!("{family:?}"))
            )
        })
        .collect();
    format!(
        "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"member\": \"zbd/c1c/m01 \
         objectives.zrd\", \"fields_read\": {}, \"fields_recovered\": {}, \
         \"unrecovered_families\": [{}], \"record_fields\": {}, \"keys\": [{}]}}",
        jstr(install_sha256),
        jstr(candidate_tree),
        recovery.fields_read(),
        recovery.fields_recovered(),
        families.join(", "),
        render_record_fields(recovery.record_fields()),
        keys.join(", "),
    )
}

/// The record-level parse as JSON: the timer's stored seconds and `NOLOSS`
/// flag, the player init's spelled values, and each animation list's kept and
/// skipped elements.
fn render_record_fields(fields: &Result<MeasuredRecordFields, RecordFieldError>) -> String {
    let Ok(fields) = fields else {
        return format!(
            "{{\"error\": {}}}",
            jstr(&fields.as_ref().unwrap_err().to_string())
        );
    };
    let scalar = |value: &RecordScalar| match value {
        RecordScalar::Int(v) => format!("{{\"int\": {v}}}"),
        RecordScalar::Real(v) => format!("{{\"real\": {v:?}}}"),
    };
    let timer = fields.mission_timer.map_or_else(
        || "null".to_owned(),
        |timer| {
            format!(
                "{{\"seconds\": {:?}, \"no_loss\": {}}}",
                timer.seconds, timer.no_loss
            )
        },
    );
    let init = fields.player_init.as_ref().map_or_else(
        || "null".to_owned(),
        |init| {
            let position: Vec<String> = init.position.iter().map(scalar).collect();
            format!(
                "{{\"head\": {}, \"position\": [{}], \"attitude_degrees\": {:?}, \"tail\": {}, \
                 \"scaled_input\": {:?}}}",
                scalar(&init.head),
                position.join(", "),
                init.attitude_degrees,
                scalar(&init.tail),
                init.scaled_input,
            )
        },
    );
    let anims = |list: &cs_content::objectives::AnimListField| {
        let directives: Vec<String> = list
            .directives
            .iter()
            .map(|directive| match directive {
                AnimDirective::Named(name) => {
                    format!("{{\"named\": {}}}", jstr(name))
                }
                AnimDirective::Scoped {
                    object,
                    member,
                    animation,
                } => format!(
                    "{{\"object\": {}, \"member\": {}, \"animation\": {}}}",
                    jstr(object),
                    jstr(member),
                    jstr(animation),
                ),
            })
            .collect();
        format!(
            "{{\"directives\": [{}], \"skipped\": {}}}",
            directives.join(", "),
            list.skipped,
        )
    };
    format!(
        "{{\"mission_timer\": {timer}, \"player_init\": {init}, \"restore_anims\": {}, \
         \"execute_anims\": {}, \"invalidate_anims\": {}}}",
        anims(&fields.restore_anims),
        anims(&fields.execute_anims),
        anims(&fields.invalidate_anims),
    )
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_m01_b_obj_4.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-B-OBJ-4` written relative
/// to the workspace root in the module doc must be re-anchored here.
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

fn iso_utc_now() -> String {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_secs();
    // A Gregorian date rendered from the epoch second, so the report needs no
    // calendar dependency.
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
    /// `(test name, "pass" | "fail" | "unknown")`, in log order, deduplicated.
    assertions: Vec<(String, &'static str)>,
}

/// Extracts the per-test results of the `accept_m01_b_obj_4_` tests from
/// a recorded `cargo test` output.
///
/// The counts come from the **prefixed test lines**, not from the `test result:`
/// summaries: a summary aggregates every test binary cargo ran, so reading it
/// would report hundreds of unrelated tests as this task's acceptance selection.
/// A prefixed test that was skipped is recorded with the schema's `unknown`
/// status rather than counted as a pass, so a report can never claim an
/// assertion it did not run.
fn parse_suite(log: &str) -> Suite {
    let mut suite = Suite::default();
    let mut pending: VecDeque<String> = VecDeque::new();
    for line in log.lines() {
        let trimmed = line.trim_start();
        if pending.front().is_some()
            && let Some(status) = finished(trimmed)
        {
            let name = pending.pop_front().expect("pending test");
            record(&mut suite, name, status);
            continue;
        }
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let name = after[..separator].to_owned();
            let tail = after[separator + 5..].trim();
            cursor = &after[separator + 5..];
            if !carries_prefix(&name) {
                continue;
            }
            match finished(tail) {
                Some(status) => record(&mut suite, name, status),
                None => pending.push_back(name),
            }
        }
    }
    suite.assertions.dedup_by(|left, right| left.0 == right.0);
    suite.passed = count(&suite, "pass");
    suite.failed = count(&suite, "fail");
    suite.ignored = count(&suite, "unknown");
    suite.executed = suite.passed + suite.failed;
    suite.discovered = suite.passed + suite.failed + suite.ignored;
    suite
}

/// Whether a libtest name is one of this task's tests.
///
/// The prefix is matched on the name's **last** path segment, because libtest
/// prints an in-module unit test under its module path. Matching the whole
/// name instead would silently drop every in-module acceptance test from
/// `discovered`, from the assertion list and from the log.
fn carries_prefix(name: &str) -> bool {
    name.rsplit("::")
        .next()
        .is_some_and(|segment| segment.starts_with(ACCEPTANCE_PREFIX))
}

/// The libtest result word at the head of a test's tail line.
fn finished(tail: &str) -> Option<&'static str> {
    match tail.split_whitespace().next() {
        Some("ok") => Some("pass"),
        Some("FAILED") => Some("fail"),
        Some("ignored") => Some("unknown"),
        _ => None,
    }
}

fn count(suite: &Suite, status: &str) -> u64 {
    suite
        .assertions
        .iter()
        .filter(|(_, seen)| *seen == status)
        .count() as u64
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
    (
        name,
        cs_assets::install::sha256(&bytes).to_hex(),
        kind.to_owned(),
    )
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
                 \"directive-census.json\"]}}",
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
            other if (other as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", other as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
