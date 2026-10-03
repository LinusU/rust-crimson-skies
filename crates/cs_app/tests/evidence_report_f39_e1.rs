//! Evidence-report harness for task F39-E1: `docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`. Not named `accept_f39_e1_*`: it is not
//! part of the acceptance suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_f39_e1_ --include-ignored 2>&1 |
//!    tee private/evidence/F39-E1/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F39-E1 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f39_e1_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked -p cs_app --test evidence_report_f39_e1 -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/F39-E1/acceptance.json
//!    --artifact-root private/evidence/F39-E1 --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/F39-E1.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness re-runs
//! [`cs_app::objectives::survey_retail_dormant_reveal`] and records the
//! per-mission dormant/reveal census — the archive, the objective member's
//! digest, every block's measured dormant argument, condition set, completion
//! count and display identity, plus the two controlled-condition families — as
//! JSON. That is a real production run over the owner's installation, not a
//! paraphrase of the acceptance assertions.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::objectives::{DormantRevealCensus, survey_retail_dormant_reveal};
use cs_assets::install::{content_fingerprint, discover, fingerprint};

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_f39_e1_";

/// How this run was reviewed, with every measured number **derived** from the
/// census this same run produced.
///
/// The prose is a template: the counts are interpolated from
/// [`survey_retail_dormant_reveal`] rather than written down, so a report
/// regenerated on another installation cannot describe this one's numbers.
fn review_method(census: &DormantRevealCensus) -> String {
    let ladders = census.condition_ladders();
    let isolating = ladders
        .iter()
        .filter(|ladder| ladder.thresholds().len() > 1)
        .count();
    let cues = census.cue_ordered_dated_blocks();
    let agreeing = cues.iter().filter(|family| family.order_agrees).count();
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field from \
     the recorded log, production discovery of $CS_GAME_DIR, and a second production run of \
     cs_app::objectives::survey_retail_dormant_reveal over the installation (dormant-reveal-census.json). \
     Claim is implemented only. MEASURED: every mission-scoped reader archive was opened, its objectives.zrd \
     member decoded with the production .zrd reader and its numbered OBJECTIVE<N> blocks read through \
     cs_content::objectives::measure_dormant_declarations; over {} mission readers the installation declares {} \
     objective blocks, {} of which carry BEGIN_DORMANT ({} the measured -1 sentinel, {} a positive argument), \
     {} blocks carry {} INACTIVE<n> conditions, {} carry an INACTIVE_COMPLETION_COUNT, {} carry an IDENTITY \
     display declaration ({} declarations in all) and {} name a WAKEUP_SOUND_GROUP beside a dated argument. \
     CONTROLLED CONDITIONS isolated from the files, each recorded with its \
     contrary hypotheses in docs/findings/2026-10-03-f39-e1-objective-dormant-reveal-lifecycle.md: {} of {} \
     shared-condition families declare more than one threshold over the same condition set, and {} of {} \
     dated-block families whose cues are numbered in sequence are also in argument order. LIMITS OF WHAT WAS \
     MEASURED: (1) the unit of BEGIN_DORMANT's positive argument is unmeasured - it orders blocks in mission \
     time, but seconds, ticks and mission-relative events are not distinguished by any shipped file, and no \
     original executable was run; (2) what satisfying an INACTIVE<n> condition means is unmeasured - the \
     conditions name an actor, a part and an attribute spelling (healthy, panels), never a decoded damage or \
     state rule; (3) whether a satisfied condition is monotone, and therefore whether the thresholds are \
     reached, is unmeasured; (4) whether the original shows a dormant objective's text to the player is \
     unmeasured - the display identity is declared independently of the dormancy and the message ids are not \
     resolvable to text, because the installation's two shipped generated headers define no MSG_* id; (5) the \
     compiled mission program behind each record is not decoded at all, because the mission-language instruction \
     table is unmeasured (F13-B/C, F38 own it); (6) the census covers mission-scoped archives only, so shared \
     and world-group readers are outside the denominator. `unknowns` is empty because every unresolved item above \
     is a limit on the claim rather than an unresolved measurement: each row in the census resolved. Validated \
     with tools/validate_evidence.py --require-pass.",
        census.readers(),
        census.block_count(),
        census.dormant_blocks(),
        census.sentinel_blocks(),
        census.dated_blocks(),
        census.condition_blocks(),
        census.condition_count(),
        census.completion_count_blocks(),
        census.identity_blocks(),
        census.identity_declarations(),
        census.dated_wakeup_sound_group_blocks(),
        isolating,
        ladders.len(),
        agreeing,
        cues.len(),
    )
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_f39_e1_writes_the_acceptance_report() {
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

    // The second production observation: the dormant/reveal census.
    let census = survey_retail_dormant_reveal(&game_dir).expect("the census surveys");
    assert!(
        census.block_count() > 0,
        "the report must not be written over an empty census"
    );
    let rows: Vec<String> = census
        .rows()
        .iter()
        .map(|row| {
            let blocks: Vec<String> = row.blocks.iter().map(block_json).collect();
            format!(
                "{{\"mission\": {}, \"container\": {}, \"container_sha256\": {}, \"member\": {}, \
                 \"member_sha256\": {}, \"blocks\": [{}]}}",
                jstr(&row.mission),
                jstr(&row.container),
                jstr(&row.container_sha256),
                jstr(&row.member),
                jstr(&row.member_sha256),
                blocks.join(", "),
            )
        })
        .collect();
    let ladders: Vec<String> = census
        .condition_ladders()
        .iter()
        .map(|ladder| {
            let rungs: Vec<String> = ladder
                .rungs
                .iter()
                .map(|rung| {
                    format!(
                        "{{\"block\": {}, \"completion_count\": {}, \"begins_dormant\": {}, \
                         \"carries_identity\": {}}}",
                        jstr(&rung.block),
                        rung.completion_count
                            .map_or_else(|| "null".to_owned(), |count| count.to_string()),
                        rung.begins_dormant,
                        rung.carries_identity,
                    )
                })
                .collect();
            format!(
                "{{\"mission\": {}, \"condition_count\": {}, \"thresholds\": [{}], \"rungs\": [{}]}}",
                jstr(&ladder.mission),
                ladder.condition_count,
                ladder
                    .thresholds()
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
                rungs.join(", "),
            )
        })
        .collect();
    let cues: Vec<String> = census
        .cue_ordered_dated_blocks()
        .iter()
        .map(|family| {
            let entries: Vec<String> = family
                .entries
                .iter()
                .map(|entry| {
                    format!(
                        "{{\"index\": {}, \"argument\": {}, \"block\": {}, \"cue\": {}}}",
                        entry.index,
                        entry.argument,
                        jstr(&entry.block),
                        jstr(&entry.cue),
                    )
                })
                .collect();
            format!(
                "{{\"mission\": {}, \"prefix\": {}, \"order_agrees\": {}, \"entries\": [{}]}}",
                jstr(&family.mission),
                jstr(&family.prefix),
                family.order_agrees,
                entries.join(", "),
            )
        })
        .collect();
    let census_path = evidence_dir.join("dormant-reveal-census.json");
    let census_text = format!(
        "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"readers\": {}, \"blocks\": {}, \
             \"dormant_blocks\": {}, \"sentinel_blocks\": {}, \"dated_blocks\": {}, \"dated_arguments\": [{}], \
             \"condition_blocks\": {}, \"conditions\": {}, \"completion_count_blocks\": {}, \
             \"completion_counts\": [{}], \"counts_matching_conditions\": {}, \"counts_above_conditions\": {}, \
             \"counts_without_conditions\": {}, \"identity_blocks\": {}, \"identity_declarations\": {}, \
             \"identity_messages\": {}, \"dormant_identity_blocks\": {}, \"condition_arities\": [{}], \
             \"condition_attributes\": [{}], \"condition_subjects\": [{}], \"condition_parts\": [{}], \
             \"wakeup_sound_group_blocks\": {}, \"completed_sound_group_blocks\": {}, \
             \"dated_wakeup_sound_group_blocks\": {}, \"condition_ladders\": [{}], \
             \"cue_ordered_families\": [{}], \"rows\": [{}]}}\n",
        jstr(&install_sha256),
        jstr(&candidate_tree),
        census.readers(),
        census.block_count(),
        census.dormant_blocks(),
        census.sentinel_blocks(),
        census.dated_blocks(),
        census
            .dated_arguments()
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(", "),
        census.condition_blocks(),
        census.condition_count(),
        census.completion_count_blocks(),
        census
            .completion_counts()
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        census.counts_matching_conditions(),
        census.counts_above_conditions(),
        census.counts_without_conditions(),
        census.identity_blocks(),
        census.identity_declarations(),
        census.identity_messages(),
        census.dormant_identity_blocks(),
        counted(&census.condition_arities(), |(arity, count)| format!(
            "{{\"arity\": {arity}, \"declarations\": {count}}}"
        )),
        counted(&census.condition_attributes(), |(name, count)| format!(
            "{{\"name\": {}, \"declarations\": {count}}}",
            jstr(name)
        )),
        counted(&census.condition_subjects(), |(name, count)| format!(
            "{{\"subject\": {}, \"declarations\": {count}}}",
            jstr(name)
        )),
        counted(&census.condition_parts(), |(name, count)| format!(
            "{{\"part\": {}, \"declarations\": {count}}}",
            jstr(name)
        )),
        census.wakeup_sound_group_blocks(),
        census.completed_sound_group_blocks(),
        census.dated_wakeup_sound_group_blocks(),
        ladders.join(", "),
        cues.join(", "),
        rows.join(", "),
    );
    // Nothing outside this harness ever parses the census artifact — it is
    // hashed and committed — so it is checked here rather than committed as if it
    // were a measurement.
    assert_well_formed_json(&census_text, &census_path.display().to_string());
    fs::write(&census_path, census_text).expect("write dormant-reveal-census.json");

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
        "{{\n \"schema_version\": 1,\n \"task_id\": \"F39-E1\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        jstr(&review_method(&census)),
    );
    let out = evidence_dir.join("acceptance.json");
    assert_well_formed_json(&report, &out.display().to_string());
    fs::write(&out, &report).expect("write acceptance.json");
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report must NOT validate",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// Whether a rendered JSON document is well formed: balanced braces and
/// brackets outside strings, no empty value where one is required, no trailing
/// comma.
///
/// This is not a JSON parser. It is the small set of properties a hand-rolled
/// renderer of this shape breaks, checked in the harness that writes the
/// document, because a malformed evidence artifact would otherwise be committed
/// as if it were a measurement.
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
                "{what}: a trailing comma before byte {index} ({character:?})"
            );
        }
        previous = character;
    }
    assert!(!in_string, "{what}: an unterminated JSON string");
    assert_eq!(
        (braces, brackets),
        (0, 0),
        "{what}: unbalanced JSON at end of document"
    );
}

/// One measured block, as one JSON object.
fn block_json(block: &cs_content::objectives::MeasuredDormantBlock) -> String {
    let conditions: Vec<String> = block
        .conditions
        .iter()
        .map(|condition| {
            format!(
                "{{\"stage\": {}, \"subject\": {}, \"part\": {}, \"attribute\": {}, \"arity\": {}}}",
                condition.stage,
                jstr(&condition.subject),
                option_jstr(condition.part.as_deref()),
                option_jstr(condition.attribute.as_deref()),
                condition.arity,
            )
        })
        .collect();
    let identities: Vec<String> = block
        .identities
        .iter()
        .map(|identity| {
            format!(
                "{{\"role\": {}, \"ordinal\": {}, \"message\": {}}}",
                jstr(&identity.role),
                identity.ordinal,
                option_jstr(identity.message.as_deref()),
            )
        })
        .collect();
    format!(
        "{{\"block\": {}, \"dormant\": {}, \"completion_count\": {}, \"conditions\": [{}], \
         \"identities\": [{}], \"wakeup_sound_group\": {}, \"completed_sound_group\": {}}}",
        jstr(&block.block),
        match block.dormant {
            Some(reading) => jstr(&format!("{:?}", reading)),
            None => "null".to_owned(),
        },
        block
            .completion_count
            .map_or_else(|| "null".to_owned(), |count| count.to_string()),
        conditions.join(", "),
        identities.join(", "),
        option_jstr(block.wakeup_sound_group.as_deref()),
        option_jstr(block.completed_sound_group.as_deref()),
    )
}

/// A JSON array of the counted rows, each rendered by `render`.
fn counted<T>(rows: &[T], render: impl Fn(&T) -> String) -> String {
    rows.iter().map(render).collect::<Vec<_>>().join(", ")
}

/// A JSON string, or `null` for a measured absence.
fn option_jstr(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_owned(), jstr)
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/evidence_report_f39_e1.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F39-E1` written relative to the
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
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
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

/// Extracts the per-test results of the `accept_f39_e1_` tests from a recorded
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
/// (`objectives::tests::accept_f39_e1_…`). Matching the whole name instead
/// would silently drop every in-module acceptance test from `discovered`, from
/// the assertion list and from the log — the incomplete test accounting task
/// #353 exists to reject.
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
                 \"dormant-reveal-census.json\"]}}",
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
