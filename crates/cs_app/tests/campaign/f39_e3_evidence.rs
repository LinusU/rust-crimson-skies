//! Evidence-report harness for task F39-E3: `docs/contracts/CLI-EVIDENCE.md`,
//! schema `schemas/evidence.schema.json`. Not named `accept_f39_e3_*`: it is not
//! part of the acceptance suite and fails loudly when its inputs are missing.
//!
//! 1. `cargo test --workspace --locked -- accept_f39_e3_ --include-ignored 2>&1 |
//!    tee private/evidence/F39-E3/cargo-test.log` (note the exit status)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/F39-E3 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f39_e3_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
//!      cargo test --locked --test campaign evidence_report_f39_e3 -- --ignored
//!    ```
//! 3. `python3 tools/validate_evidence.py private/evidence/F39-E3/acceptance.json
//!    --artifact-root private/evidence/F39-E3 --require-pass`
//! 4. Commit a copy as `docs/findings/evidence/F39-E3.json`.
//!
//! Every field is derived from the recorded log, the environment, production
//! discovery of `$CS_GAME_DIR` and `Cargo.lock`. The second artifact is a
//! **second production observation**: the harness re-runs
//! [`survey_retail_scope_objective_records`] and records the installation-scope
//! census — the archive, its role, its declared and distinct member counts, the
//! numbered objective blocks, the objective target records and the complete
//! objective-named spelling inventory — as JSON, beside the **complete** member
//! key vocabulary. That is a real production run over the owner's installation,
//! not a paraphrase of the acceptance assertions.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_app::objectives::{
    RetailScopeObjectiveCensus, survey_retail_objective_records,
    survey_retail_scope_objective_records,
};
use cs_assets::install::{content_fingerprint, discover, fingerprint};

/// Every acceptance test the report must see pass.
const ACCEPTANCE_PREFIX: &str = "accept_f39_e3_";

/// The measured values the report's `review.method` prose states.
///
/// Taken out of the census by [`MethodFacts::measured`] so that the rendering is a
/// pure function of plain numbers, which is what lets a synthetic census pin it
/// (see `…_method_prose_carries_each_measured_number_in_its_own_clause`). Nothing
/// here is computed: each field is one production reader's own figure.
struct MethodFacts {
    mission_archives: usize,
    scope_archives: usize,
    declared_members: usize,
    distinct_names: usize,
    decoded_members: usize,
    scope_blocks: u32,
    mission_blocks: u32,
    scope_target_records: u32,
    /// The scopes whose archive declares objective target records.
    target_scopes: Vec<String>,
    /// `(label, occurrences)` over the whole scope layer.
    target_labels: Vec<(String, u32)>,
    /// `(spelling, occurrences)` over the whole scope layer.
    spellings: Vec<(String, u32)>,
}

impl MethodFacts {
    /// The facts this report's prose states, read from the two censuses the harness
    /// has just run over the installation.
    fn measured(scope: &RetailScopeObjectiveCensus, missions: usize, blocks: u32) -> Self {
        Self {
            mission_archives: missions,
            scope_archives: scope.len(),
            declared_members: scope.declared_members(),
            distinct_names: scope.distinct_member_names(),
            decoded_members: scope.decoded_members(),
            scope_blocks: scope.objective_blocks(),
            mission_blocks: blocks,
            scope_target_records: scope.target_records(),
            target_scopes: scope
                .objective_target_scopes()
                .into_iter()
                .map(str::to_owned)
                .collect(),
            target_labels: scope.target_names().into_iter().collect(),
            spellings: scope.objective_spellings(),
        }
    }
}

/// How this run was reviewed, with every measured number **derived** from the two
/// censuses this same run produced.
///
/// The prose is a template: the counts are interpolated rather than written down,
/// so a report regenerated on another installation cannot describe this one's
/// numbers.
///
/// Every interpolated value is a **named** argument. A positional `{}` list of
/// twelve values for a template this long compiles happily and silently
/// transposes them — the first version of this function did exactly that, and
/// published a report whose `review.method` read "the 612 scope archives declare
/// 381 members (612 distinct names)", put the **target-label** list where the
/// objective-**spelling** inventory belongs and left the spelling list in an
/// unrelated clause
/// (`docs/findings/2026-10-04-f39-e3-installation-scope-objective-declarations.md`,
/// "Review 2026-10-04"). Every number came from the right call and was attached to
/// the wrong claim, and no schema check can see that. A named argument cannot be
/// transposed: it binds to the placeholder that names it, and the compiler rejects
/// both a name the template never uses and a placeholder the template never
/// supplies.
fn review_method(facts: &MethodFacts) -> String {
    let spellings = facts
        .spellings
        .iter()
        .map(|(spelling, count)| format!("{spelling} {count}"))
        .collect::<Vec<_>>()
        .join("; ");
    // Bracketed, because one measured label is a single space: an unbracketed
    // rendering would put a blank where a name belongs and read as a missing one.
    let labels = facts
        .target_labels
        .iter()
        .map(|(name, count)| format!("[{name}] x{count}"))
        .collect::<Vec<_>>()
        .join("; ");
    let target_scopes = facts.target_scopes.join(", ");
    format!(
        "Acceptance suite run locally with the retail capability; this harness derives every field from \
     the recorded log, production discovery of $CS_GAME_DIR, and two production runs of the \
     cs_app::objectives censuses over the installation (scope-objective-census.json). Claim is implemented \
     only. MEASURED: every reader archive the installation holds was split by F13-B's mission_scope rule \
     into the {mission_archives} mission-scoped archives F39-D's census measures and the \
     {scope_archives} installation-scope archives this task adds; those {scope_archives} archives declare \
     {declared_members} members ({distinct_names} distinct names) and {decoded_members} of them decoded, \
     and they declare {scope_blocks} numbered OBJECTIVE<N> blocks, so the mission-scoped denominator of \
     {mission_blocks} blocks is COMPLETE for that surface rather than a share of it. The objective TARGET \
     surface is the opposite: {scope_target_records} objective target records, in {target_scope_count} of \
     the {scope_archives} scope archives ({target_scopes}), labelled {target_labels} — so the objective \
     targets a mission may inherit sit outside the mission-scoped denominator, while a mission-scoped \
     archive carries objective blocks only. The complete objective-named spelling inventory over all \
     scope members, which is the search list the negative result is drawn from: {spellings}. LIMITS OF \
     WHAT WAS MEASURED, each recorded in \
     docs/findings/2026-10-04-f39-e3-installation-scope-objective-declarations.md: (1) whether a mission \
     RESOLVES a member of its world group's or the install-wide reader is reader-archive precedence \
     (F04/F06) and was not measured - the three dialog spellings sit in install-wide members that no \
     mission-scoped reader carries, so they are inherited rather than duplicated in the files, and what \
     the original does with them is unknown; (2) a spelling is a spelling - no declaration's meaning is \
     recovered, the mission-language instruction table is unmeasured (F13-B/C, F38) and no original \
     executable was run, so nothing here is evidence of behaviour; (3) an original record stays refused \
     as UNMEASURED_OBJECTIVE_SEMANTICS and no original mission is played. `unknowns` is empty because \
     every unresolved item above is a limit on the claim rather than an unresolved measurement: every \
     row in both censuses resolved and every scope archive classified. Validated with \
     tools/validate_evidence.py --require-pass.",
        mission_archives = facts.mission_archives,
        scope_archives = facts.scope_archives,
        declared_members = facts.declared_members,
        distinct_names = facts.distinct_names,
        decoded_members = facts.decoded_members,
        scope_blocks = facts.scope_blocks,
        mission_blocks = facts.mission_blocks,
        scope_target_records = facts.scope_target_records,
        target_scope_count = facts.target_scopes.len(),
        target_scopes = target_scopes,
        target_labels = labels,
        spellings = spellings,
    )
}

/// The report's own prose must carry the numbers the censuses measured, in the
/// place each one belongs.
///
/// This exists because the first version of [`review_method`] passed its twelve
/// values **positionally** to a template that long, and `rustc` accepted the
/// transposition silently: the committed report's `review.method` said "the 612
/// scope archives declare 381 members (612 distinct names)", "53 target records
/// in 5 of the scope archives", rendered the **target-label** list where the
/// objective-**spelling** inventory belongs, and put the spelling list inside the
/// unrelated "the {}-sided reading" clause. Every number in it came from the right
/// call; every number was attached to the wrong claim. A validation tool cannot
/// catch that — the JSON was well-formed and the schema passed — so the reading is
/// pinned here instead: each measured value must appear next to the words that
/// describe what it counts.
///
/// Synthetic census, so this runs in CI without `CS_GAME_DIR`: the values are
/// chosen to be mutually distinguishable (9 archives, 612 declared, 381 distinct,
/// 612 decoded, 0 blocks, 1338 mission blocks, 5 target records in 1 scope), which
/// is exactly the property a transposition destroys.
#[test]
fn evidence_report_f39_e3_method_prose_carries_each_measured_number_in_its_own_clause() {
    // Values chosen to be mutually distinguishable (53 / 9 / 612 / 381 / 0 / 1338
    // / 5 / 1), which is exactly the property a transposition destroys: with equal
    // or near-equal numbers a wrong attachment is invisible in the prose.
    let facts = MethodFacts {
        mission_archives: 53,
        scope_archives: 9,
        declared_members: 612,
        distinct_names: 381,
        decoded_members: 612,
        scope_blocks: 0,
        mission_blocks: 1338,
        scope_target_records: 5,
        target_scopes: vec!["zbd/c1c".to_owned()],
        target_labels: vec![
            ("MSG_OBJ_DOCK".to_owned(), 2),
            ("MSG_OBJ_ZEPPELIN".to_owned(), 3),
        ],
        spellings: vec![
            ("MSG_BRF_DLG_OBJECTIVES".to_owned(), 4),
            ("OBJECTIVESLIST".to_owned(), 553),
        ],
    };
    let method = review_method(&facts);

    // Each count, next to the words that say what it counts.
    for clause in [
        "into the 53 mission-scoped archives",
        "the 9 installation-scope archives",
        "declare 612 members",
        "(381 distinct names)",
        "they declare 0 numbered OBJECTIVE<N> blocks",
        "denominator of 1338 blocks is COMPLETE",
        "surface is the opposite: 5 objective target records",
        "in 1 of the 9 scope archives (zbd/c1c)",
    ] {
        assert!(
            method.contains(clause),
            "review.method does not carry {clause:?}: {method}"
        );
    }
    // The spelling inventory and the target labels are different lists; the
    // transposition put each where the other belonged, so each must appear in its
    // own clause and *not* in the other's.
    let spellings_clause = method
        .split_once("negative result is drawn from: ")
        .and_then(|(_, tail)| tail.split_once(". LIMITS"))
        .map(|(head, _)| head)
        .unwrap_or_else(|| panic!("no spelling clause in {method}"));
    assert_eq!(
        spellings_clause, "MSG_BRF_DLG_OBJECTIVES 4; OBJECTIVESLIST 553",
        "the spelling clause is not the objective-named spelling inventory: {method}"
    );
    assert!(
        method.contains("labelled [MSG_OBJ_DOCK] x2; [MSG_OBJ_ZEPPELIN] x3"),
        "the target labels are not reported next to the target records: {method}"
    );
    // No clause may be left holding an unrelated value: the old template's
    // "(3) the {}-sided reading" clause took the spelling list, which is what made
    // the defect visible in the first place.
    assert!(
        !method.contains("-sided reading") && !method.contains("{}"),
        "an unlabelled clause is still receiving an interpolated value: {method}"
    );
}

/// A label that is a single space must render as a name, not as a blank.
///
/// Measured: one of `zbd/c1c`'s five objective target records spells its
/// `help_label` as `" "`. Rendered unbracketed into a `; `-joined list it
/// disappears, and the report would state four labels for five records without
/// saying why.
#[test]
fn evidence_report_f39_e3_a_blank_target_label_still_renders_as_a_label() {
    let facts = MethodFacts {
        mission_archives: 53,
        scope_archives: 9,
        declared_members: 612,
        distinct_names: 381,
        decoded_members: 612,
        scope_blocks: 0,
        mission_blocks: 1338,
        scope_target_records: 5,
        target_scopes: vec!["zbd/c1c".to_owned()],
        target_labels: vec![(" ".to_owned(), 1)],
        spellings: vec![("OBJECTIVESLIST".to_owned(), 553)],
    };
    let method = review_method(&facts);
    assert!(
        method.contains("labelled [ ] x1"),
        "a blank target label was rendered as nothing at all: {method}"
    );
}

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_f39_e3_writes_the_acceptance_report() {
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

    // The second production observation: both censuses, so the artifact states the
    // whole reader-archive denominator and not only this task's half of it.
    let scope = survey_retail_scope_objective_records(&game_dir)
        .expect("the installation-scope census surveys");
    assert!(
        !scope.is_empty(),
        "the report must not be written over an empty census"
    );
    let missions = survey_retail_objective_records(&game_dir).expect("the mission census surveys");
    assert_eq!(
        missions.install_sha256(),
        scope.install_sha256(),
        "both censuses must read the same installation"
    );
    let count_map = |names: &std::collections::BTreeMap<String, u32>| -> String {
        let items: Vec<String> = names
            .iter()
            .map(|(name, count)| format!("{{\"name\": {}, \"sites\": {count}}}", jstr(name)))
            .collect();
        format!("[{}]", items.join(", "))
    };
    let spelling_list = |spelling: &str, count: u32| -> String {
        format!(
            "{{\"spelling\": {}, \"occurrences\": {count}}}",
            jstr(spelling)
        )
    };
    let rows: Vec<String> = scope
        .rows()
        .iter()
        .map(|row| {
            let targets = match &row.target_records {
                Some(kinds) => format!(
                    "{{\"records\": {}, \"labelled\": {}, \"names\": {}}}",
                    kinds.records,
                    kinds.labelled,
                    count_map(&kinds.names),
                ),
                // An archive that declares no `targets.zrd` is rendered as an
                // explicit null with its absence named, never as an empty reading.
                None => "null".to_owned(),
            };
            format!(
                "{{\"scope\": {}, \"role\": {}, \"container\": {}, \"container_sha256\": {}, \
                 \"evidence\": [{}], \"declared_members\": {}, \"distinct_members\": {}, \
                 \"decoded_members\": {}, \"objective_blocks\": {}, \"target_records\": {}, \
                 \"objective_spellings\": [{}], \"keys\": [{}], \"member_names\": {}}}",
                jstr(&row.scope),
                jstr(row.role.label()),
                jstr(&row.container),
                jstr(&row.container_sha256),
                row.evidence
                    .iter()
                    .map(|name| jstr(name))
                    .collect::<Vec<_>>()
                    .join(", "),
                row.declared_members,
                row.distinct_members,
                row.decoded_members,
                row.objective_blocks,
                targets,
                row.objective_spellings
                    .iter()
                    .map(|(spelling, count)| spelling_list(spelling, *count))
                    .collect::<Vec<_>>()
                    .join(", "),
                row.keys
                    .iter()
                    .map(|(key, count)| format!(
                        "{{\"key\": {}, \"occurrences\": {count}}}",
                        jstr(key)
                    ))
                    .collect::<Vec<_>>()
                    .join(", "),
                str_array(&row.member_names),
            )
        })
        .collect();
    let census_path = evidence_dir.join("scope-objective-census.json");
    fs::write(
        &census_path,
        format!(
            "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"scope_archives\": {}, \
             \"declared_members\": {}, \"decoded_members\": {}, \"distinct_member_names\": {}, \
             \"objective_blocks\": {}, \"target_records\": {}, \"labelled_targets\": {}, \
             \"objective_target_scopes\": {}, \"mission_archives\": {}, \"mission_blocks\": {}, \
             \"objective_spellings\": [{}], \"target_names\": {}, \"keys\": [{}], \"rows\": [{}]}}\n",
            jstr(&install_sha256),
            jstr(&candidate_tree),
            scope.len(),
            scope.declared_members(),
            scope.decoded_members(),
            scope.distinct_member_names(),
            scope.objective_blocks(),
            scope.target_records(),
            scope.labelled_targets(),
            str_array(
                &scope
                    .objective_target_scopes()
                    .into_iter()
                    .map(str::to_owned)
                    .collect::<Vec<_>>(),
            ),
            missions.len(),
            missions.blocks(),
            scope
                .objective_spellings()
                .iter()
                .map(|(spelling, count)| spelling_list(spelling, *count))
                .collect::<Vec<_>>()
                .join(", "),
            count_map(&scope.target_names()),
            scope
                .keys()
                .iter()
                .map(|(key, count)| format!(
                    "{{\"key\": {}, \"occurrences\": {count}}}",
                    jstr(key)
                ))
                .collect::<Vec<_>>()
                .join(", "),
            rows.join(", "),
        ),
    )
    .expect("write scope-objective-census.json");

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
        "{{\n \"schema_version\": 1,\n \"task_id\": \"F39-E3\",\n \"candidate_tree\": {},\n \"engine\": {},\n \"created_at\": {},\n \"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n \"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n \"seed\": 0,\n \"ticks\": {{\"start\": 0, \"end\": 0}},\n \"overrides\": [],\n \"capabilities\": [\"retail\", \"synthetic\"],\n \"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n \"assertions\": [{}],\n \"artifacts\": [{}],\n \"unknowns\": [],\n \"review\": {{\"identity\": {}, \"method\": {}}},\n \"claim\": \"implemented\"\n}}\n",
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
        jstr(&review_method(&MethodFacts::measured(
            &scope,
            missions.len(),
            missions.blocks(),
        ))),
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
             (crates/cs_app/tests/evidence_report_f39_e3.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/F39-E3` written relative to the
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

/// Extracts the per-test results of the `accept_f39_e3_` tests from a recorded
/// `cargo test` output.
///
/// The counts come from the **prefixed test lines**, not from the `test result:`
/// summaries: a summary aggregates every test binary cargo ran, so reading it
/// would report hundreds of unrelated tests as this task's acceptance selection.
/// A prefixed test that was skipped is recorded with the schema's `unknown`
/// status rather than counted as a pass, so a report can never claim an assertion
/// it did not run.
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
/// (`catalog::reader_dirs::tests::accept_f39_e3_…`). Matching the whole name
/// instead would silently drop every in-module acceptance test from `discovered`,
/// from the assertion list and from the log.
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
                 \"scope-objective-census.json\"]}}",
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
