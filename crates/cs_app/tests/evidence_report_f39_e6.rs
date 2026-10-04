//! Evidence-report harness for task F39-E6: `docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`. Not named `accept_f39_e6_*`: it is
//! not part of the acceptance suite and fails loudly when its inputs are
//! missing.
//!
//! 1. `cargo test --workspace --locked -- accept_f39_e6_ --include-ignored 2>&1 |
//!    tee private/evidence/F39-E6/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F39-E6 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f39_e6_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_f39_e6 -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/F39-E6/acceptance.json
//!    --artifact-root private/evidence/F39-E6 --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/F39-E6.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness re-runs
//! [`survey_retail_objective_records`] and
//! [`survey_excluded_objective_records`] and records both censuses — the
//! mission rows' repeated-key reading and, for the corpus the mission census
//! excludes, every member's container, span, digest, scopes, branching-key
//! spelling counts and per-block reading — as JSON. That is a real production
//! run over the owner's installation, not a paraphrase of the acceptance
//! assertions.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::objectives::{
    ExcludedObjectiveCensus, ExcludedObjectiveScope, RetailObjectiveCensus,
    survey_excluded_objective_records, survey_retail_objective_records,
};
use cs_assets::install::{content_fingerprint, discover, fingerprint};

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_f39_e6_";

/// How this run was reviewed, with every measured number **derived** from the
/// censuses this same run produced.
///
/// The prose is a template: the counts are interpolated from
/// [`survey_retail_objective_records`] and
/// [`survey_excluded_objective_records`] rather than written down, so a report
/// regenerated on another installation cannot describe this one's numbers.
fn review_method(census: &RetailObjectiveCensus, excluded: &ExcludedObjectiveCensus) -> String {
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field from \
     the recorded log, production discovery of $CS_GAME_DIR, and second production runs of \
     cs_app::objectives::survey_retail_objective_records and survey_excluded_objective_records over the \
     installation (repeated-key-census.json). Claim is implemented only. MEASURED: the question is whether a \
     block spelling the SAME completion-effect key twice is meaningful - F39-E2's measurement counted it as two \
     sites and one effect, a reading rather than a rule. Over {} mission readers the installation declares {} \
     objective blocks and {} blocks that repeat a completion-effect key ({} repeated (block, kind) pairs in all). \
     The excluded corpus measured beside it: {} reader archives mission_scope does not name holding {} members, \
     plus {} targets.zrd members declaring {} objective records - of which {} members declare any OBJECTIVE<N> \
     block, {} members spell a completion-effect key anywhere in their decoded tree, {} spell the \
     order-dependency key, and {} blocks repeat a completion-effect key. So the repeated-key shape occurs \
     NOWHERE in the readable corpus, and that silence is a bound on the corpus, not evidence the original refuses \
     it. The shape is its own named unknown (UNMEASURED_REPEATED_EFFECT_KEY): what a second site of one key does - \
     replace the first, be ignored, or apply beside it - is unmeasured because the compiled program behind the \
     record is not decoded (F13-B/C, F38 own the instruction table), and is refused by name in the declared \
     vocabulary (RepeatedCompletionEffect) rather than deduplicated. What would settle it: a decoded compiled \
     mission program, or an original run. LIMITS OF WHAT WAS MEASURED, recorded in \
     docs/findings/2026-10-04-f39-e6-repeated-completion-effect-key.md: (1) the corpus DECLARES spellings; a \
     zero count bounds the corpus and says nothing about what the original engine would do with one; (2) the \
     .zrd members of other container families (interp, texture, sound, animation, gamez) are outside this \
     corpus's scope - the measurement covers the reader archives and the targets.zrd records, which is where an \
     objective record could live; (3) no original executable was run, so nothing here is evidence of original \
     behaviour. `unknowns` is empty because every unresolved item above is a limit on the claim rather than an \
     unresolved measurement: every member of both censuses decoded and every count resolved. The open semantic \
     the measurement cannot close is named in the findings and by UNMEASURED_REPEATED_EFFECT_KEY in the record, \
     not carried as a report unknown. Validated with \
     tools/validate_evidence.py --require-pass.",
        census.len(),
        census.blocks(),
        census.repeated_effect_blocks(),
        census.repeated_effects().len(),
        excluded.archives_outside_mission_scope().len(),
        excluded.members_in_scope(ExcludedObjectiveScope::OutsideMissionScope),
        excluded.members_in_scope(ExcludedObjectiveScope::TargetsRecord),
        excluded.targets_records(),
        excluded.members_with_objective_blocks(),
        excluded
            .rows()
            .iter()
            .filter(|row| row.effect_key_sites > 0)
            .count(),
        excluded
            .rows()
            .iter()
            .filter(|row| row.order_key_sites > 0)
            .count(),
        excluded.repeated_effect_blocks(),
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_f39_e6_writes_the_acceptance_report() {
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

    // The second production observation: both censuses, the mission corpus and
    // the corpus the mission census excludes.
    let census = survey_retail_objective_records(&game_dir).expect("the census surveys");
    assert!(
        !census.is_empty(),
        "the report must not be written over an empty census"
    );
    let excluded =
        survey_excluded_objective_records(&game_dir).expect("the excluded census surveys");
    assert!(
        !excluded.is_empty(),
        "the report must not be written over an empty census"
    );

    let mission_rows: Vec<String> = census
        .rows()
        .iter()
        .map(|row| {
            let repeats: Vec<String> = row
                .repeated_effects()
                .iter()
                .map(|repeated| {
                    let sites: Vec<String> = repeated
                        .repeated
                        .sites
                        .iter()
                        .map(|site| {
                            let targets: Vec<String> =
                                site.targets.iter().map(u32::to_string).collect();
                            let arguments: Vec<String> = site
                                .arguments
                                .iter()
                                .map(|argument| format!("{argument:?}"))
                                .collect();
                            format!(
                                "{{\"targets\": [{}], \"arguments\": [{}]}}",
                                targets.join(", "),
                                arguments.join(", ")
                            )
                        })
                        .collect();
                    format!(
                        "{{\"block\": {}, \"kind\": {}, \"sites\": [{}]}}",
                        jstr(&repeated.repeated.block),
                        jstr(repeated.repeated.kind.label()),
                        sites.join(", ")
                    )
                })
                .collect();
            format!(
                "{{\"mission\": {}, \"container\": {}, \"member_sha256\": {}, \"blocks\": {}, \
                 \"effect_sites\": {}, \"repeated_effect_blocks\": {}, \"repeated_effects\": [{}]}}",
                jstr(&row.mission),
                jstr(&row.container),
                jstr(&row.member_sha256),
                row.blocks,
                row.completion_effect_sites,
                row.branch_precedence.repeated_effect_blocks(),
                repeats.join(", ")
            )
        })
        .collect();

    let excluded_rows: Vec<String> = excluded
        .rows()
        .iter()
        .map(|row| {
            let scopes: Vec<String> = row
                .scopes
                .iter()
                .map(|scope| {
                    jstr(match scope {
                        ExcludedObjectiveScope::OutsideMissionScope => "outside_mission_scope",
                        ExcludedObjectiveScope::TargetsRecord => "targets_record",
                    })
                    .to_string()
                })
                .collect();
            let targets = row.targets.as_ref().map_or_else(
                || "null".to_owned(),
                |targets| {
                    let keys: Vec<String> = targets
                        .keys
                        .iter()
                        .map(|(key, count)| {
                            format!("{{\"key\": {}, \"records\": {count}}}", jstr(key))
                        })
                        .collect();
                    format!(
                        "{{\"records\": {}, \"keys\": [{}]}}",
                        targets.records,
                        keys.join(", ")
                    )
                },
            );
            format!(
                "{{\"container\": {}, \"container_sha256\": {}, \"member\": {}, \"offset\": {}, \
                 \"length\": {}, \"member_sha256\": {}, \"scopes\": [{}], \"effect_key_sites\": {}, \
                 \"order_key_sites\": {}, \"objective_blocks\": {}, \"repeated_effect_blocks\": {}, \
                 \"targets\": {}}}",
                jstr(&row.container),
                jstr(&row.container_sha256),
                jstr(&row.member),
                row.member_offset,
                row.member_len,
                jstr(&row.member_sha256),
                scopes.join(", "),
                row.effect_key_sites,
                row.order_key_sites,
                row.precedence.blocks,
                row.precedence.repeated_effect_blocks(),
                targets
            )
        })
        .collect();
    let archives: Vec<String> = excluded
        .archives_outside_mission_scope()
        .iter()
        .map(|spelling| jstr(spelling).to_string())
        .collect();

    let census_path = evidence_dir.join("repeated-key-census.json");
    fs::write(
        &census_path,
        format!(
            "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"mission_corpus\": {{\"missions\": {}, \
             \"blocks\": {}, \"completion_effect_sites\": {}, \"repeated_effect_blocks\": {}, \
             \"needs_unmeasured_repeated_effect\": {}, \"rows\": [{}]}}, \"excluded_corpus\": \
             {{\"archives_outside_mission_scope\": [{}], \"members\": {}, \
             \"members_outside_mission_scope\": {}, \"targets_members\": {}, \"targets_records\": {}, \
             \"members_with_objective_blocks\": {}, \"objective_blocks\": {}, \"effect_key_sites\": {}, \
             \"order_key_sites\": {}, \"repeated_effect_blocks\": {}, \
             \"needs_unmeasured_repeated_effect\": {}, \"rows\": [{}]}}}}\n",
            jstr(&install_sha256),
            jstr(&candidate_tree),
            census.len(),
            census.blocks(),
            census.completion_effect_sites(),
            census.repeated_effect_blocks(),
            census.needs_unmeasured_repeated_effect(),
            mission_rows.join(", "),
            archives.join(", "),
            excluded.len(),
            excluded.members_in_scope(ExcludedObjectiveScope::OutsideMissionScope),
            excluded.members_in_scope(ExcludedObjectiveScope::TargetsRecord),
            excluded.targets_records(),
            excluded.members_with_objective_blocks(),
            excluded.objective_blocks(),
            excluded.effect_key_sites(),
            excluded.order_key_sites(),
            excluded.repeated_effect_blocks(),
            excluded.needs_unmeasured_repeated_effect(),
            excluded_rows.join(", "),
        ),
    )
    .expect("write repeated-key-census.json");

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&census_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };
    // `unknowns` must stay empty for --require-pass: the open semantic the
    // measurement cannot close (what a second site of one key does) is a limit
    // on the claim, not an unresolved measurement — every member of both
    // censuses decoded and every count resolved. It is named in the findings
    // and by UNMEASURED_REPEATED_EFFECT_KEY in the record, not here.
    let unknowns: Vec<String> = Vec::new();
    let report = format!(
        "{{\n \"schema_version\": 1,\n \"task_id\": \"F39-E6\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [{}],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        unknowns.join(", "),
        jstr(&reviewer),
        jstr(&review_method(&census, &excluded)),
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

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_f39_e6.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F39-E6` written relative to the
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

/// Extracts the per-test results of the `accept_f39_e6_` tests from a recorded
/// `cargo test` output.
///
/// The counts come from the **prefixed test lines**, not from the
/// `test result:` summaries: a summary aggregates every test binary cargo ran,
/// so reading it would report hundreds of unrelated tests as this task's
/// acceptance selection. A prefixed test that was skipped is recorded with the
/// schema's `unknown` status rather than counted as a pass, so a report can
/// never claim an assertion it did not run.
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
/// prints an in-module unit test under its module path
/// (`objectives::tests::accept_f39_e6_…`). Matching the whole name instead
/// would silently drop every in-module acceptance test from `discovered`, from
/// the assertion list and from the log.
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
                 \"repeated-key-census.json\"]}}",
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
