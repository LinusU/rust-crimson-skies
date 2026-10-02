//! Evidence-report harnesses for the per-mission binding stages M01-A,
//! M02-A, M03-A, M04-A, M05-A, M06-A, M08-A, M11-A, M12-A and M13-A
//! (`docs/contracts/CLI-EVIDENCE.md`, schema `schemas/evidence.schema.json`).
//!
//! These tests are deliberately **not** named `accept_m01_a_*` …
//! `accept_m06_a_*` … `accept_m08_a_*` … `accept_m11_a_*` … `accept_m12_a_*` …
//! `accept_m13_a_*`: they are not part of the acceptance suites, they fail
//! loudly when their inputs are missing instead of passing vacuously, and a
//! task's test selection must never pick them up as acceptance tests. Run
//! from the workspace root, after the acceptance suite, exactly as (with
//! `M01-A` / `accept_m01_a_` substituted for `M02-A` / `accept_m02_a_`):
//!
//! 1. ```sh
//!    cargo test --workspace --locked -- accept_m02_a_ --include-ignored \
//!      2>&1 | tee private/evidence/M02-A/cargo-test.log
//!    ```
//!    (record the pipeline's exit status — it is passed to this harness as
//!    `CS_EVIDENCE_EXIT_CODE`.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/M02-A \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m02_a_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!      cargo test --locked --test campaign evidence_report_m02_a -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/M02-A/acceptance.json \
//!      --artifact-root private/evidence/M02-A --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as
//!    `docs/findings/evidence/M02-A.json`.
//!
//! Every field is derived from real inputs: the recorded test log, the
//! environment, production discovery of `$CS_GAME_DIR` and the binding
//! production code derives from it, `rustc --version` and `Cargo.lock`.
//! Nothing is typed in by hand.
//!
//! The reports' `unknowns` are *those tasks'* blockers. They are empty for
//! M01-A … M06-A, M08-A, M12-A and M13-A because their acceptance runs passed
//! with every critical dependency resolved. **M11-A is the exception**: its
//! declared discovery title is carried by no localized row, so four of the five
//! critical dependencies are unresolved, its report lists them in `unknowns`
//! and is therefore *expected* not to validate under `--require-pass`. Dropping
//! them to make a validator pass is forbidden, so the honest report is the one
//! that keeps them.
//!
//! The bindings' own unbound checklist entries are **not** dropped anywhere:
//! they are carried in `missions/bindings/M01.json` … `missions/bindings/M06.json`,
//! `M08.json`, `M11.json`, `M12.json`, `M13.json` and in `docs/findings/`, which
//! is where the product-incompleteness state lives (`AUDIT-PLAN-SYNC`: keep the
//! states separate). The claim is `implemented`, never `checked` or
//! `verified_original`.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_content::campaign_bindings::{SourceBinding, SourceContext};

/// The retail acceptance tests this task's capabilities are judged on.
const RETAIL_TESTS: &[&str] = &[
    "accept_m01_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m01_a_the_committed_record_is_what_the_installation_derives",
    "accept_m01_a_a_title_the_local_strings_do_not_carry_is_unresolved",
    "accept_m01_a_the_campaign_keeps_everything_else_unresolved_and_unready",
    "accept_m01_a_the_retail_title_block_binds_every_campaign_position",
    "accept_m01_a_a_title_outside_the_retail_title_block_resolves_no_position",
];

/// The retail acceptance tests M02-A's capabilities are judged on, and the
/// synthetic ones that must run alongside them.
const RETAIL_TESTS_M02_A: &[&str] = &[
    "accept_m02_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m02_a_the_committed_record_is_what_the_installation_derives",
    "accept_m02_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m02_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m02_a_the_world_group_holds_several_missions",
    "accept_m02_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The retail acceptance tests M03-A's capabilities are judged on.
const RETAIL_TESTS_M03_A: &[&str] = &[
    "accept_m03_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m03_a_the_committed_record_is_what_the_installation_derives",
    "accept_m03_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m03_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m03_a_the_world_group_is_its_own_but_not_its_directory",
    "accept_m03_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M03-A's report must also record.
const SYNTHETIC_TESTS_M03_A: &[&str] = &[
    "accept_m03_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m03_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M04-A's capabilities are judged on.
const RETAIL_TESTS_M04_A: &[&str] = &[
    "accept_m04_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m04_a_the_committed_record_is_what_the_installation_derives",
    "accept_m04_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m04_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m04_a_neither_the_world_group_nor_the_mission_number_identifies_the_mission",
    "accept_m04_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M04-A's report must also record.
const SYNTHETIC_TESTS_M04_A: &[&str] = &[
    "accept_m04_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m04_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M05-A's capabilities are judged on.
const RETAIL_TESTS_M05_A: &[&str] = &[
    "accept_m05_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m05_a_the_committed_record_is_what_the_installation_derives",
    "accept_m05_a_the_title_is_confirmed_only_through_the_long_name_form",
    "accept_m05_a_the_verbatim_form_wins_where_both_display_forms_carry_the_title",
    "accept_m05_a_the_world_group_is_shared_and_the_program_singles_the_mission_out",
    "accept_m05_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M05-A's report must also record.
const SYNTHETIC_TESTS_M05_A: &[&str] = &[
    "accept_m05_a_only_an_exact_title_or_an_exact_long_name_tail_confirms",
    "accept_m05_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M06-A's capabilities are judged on.
const RETAIL_TESTS_M06_A: &[&str] = &[
    "accept_m06_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m06_a_the_committed_record_is_what_the_installation_derives",
    "accept_m06_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m06_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m06_a_the_position_is_the_first_row_of_the_second_chapter_and_region_group",
    "accept_m06_a_the_world_group_holds_several_missions_and_not_the_whole_chapter",
    "accept_m06_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M06-A's report must also record.
const SYNTHETIC_TESTS_M06_A: &[&str] = &[
    "accept_m06_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m06_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M08-A's capabilities are judged on.
const RETAIL_TESTS_M08_A: &[&str] = &[
    "accept_m08_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m08_a_the_committed_record_is_what_the_installation_derives",
    "accept_m08_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m08_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m08_a_the_position_is_interior_to_the_second_chapter_and_its_region_group",
    "accept_m08_a_the_world_group_holds_several_missions_and_not_the_whole_chapter",
    "accept_m08_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M08-A's report must also record.
const SYNTHETIC_TESTS_M08_A: &[&str] = &[
    "accept_m08_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m08_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M12-A's capabilities are judged on.
const RETAIL_TESTS_M12_A: &[&str] = &[
    "accept_m12_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m12_a_the_committed_record_is_what_the_installation_derives",
    "accept_m12_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m12_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m12_a_the_position_is_inside_the_third_chapter_and_its_region_group",
    "accept_m12_a_the_world_group_is_the_whole_chapter_and_not_the_mission",
    "accept_m12_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M12-A's report must also record.
const SYNTHETIC_TESTS_M12_A: &[&str] = &[
    "accept_m12_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m12_a_a_contradicted_corroboration_establishes_no_position",
];

/// The retail acceptance tests M13-A's capabilities are judged on.
const RETAIL_TESTS_M13_A: &[&str] = &[
    "accept_m13_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m13_a_the_committed_record_is_what_the_installation_derives",
    "accept_m13_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m13_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m13_a_the_position_is_interior_to_the_third_chapter_and_its_region_group",
    "accept_m13_a_the_world_group_is_the_whole_chapter_and_not_the_mission",
    "accept_m13_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M13-A's report must also record.
const SYNTHETIC_TESTS_M13_A: &[&str] = &[
    "accept_m13_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m13_a_a_contradicted_corroboration_establishes_no_position",
];

/// The synthetic predicate tests M02-A's report must also record.
const SYNTHETIC_TESTS_M02_A: &[&str] = &[
    "accept_m02_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m02_a_a_contradicted_corroboration_establishes_no_position",
];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m01_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    // The candidate tree must be the tree that was actually tested.
    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    // The acceptance suite is the evidence: parse its recorded output.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m01_a_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed: `retail` is declared
    // only because every retail acceptance test above is in this log and
    // passed, and `retail` is this task's required capability.
    for retail_test in RETAIL_TESTS {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M01-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    assert!(
        suite
            .assertions
            .iter()
            .any(|(name, _)| name == "accept_m01_a_verified_and_unresolved_are_two_distinct_states"),
        "the synthetic predicate tests must be present alongside the retail ones"
    );

    // `source` hashes describe the real installation, measured by the very
    // production discovery this task's binding is built on.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The derived binding itself, written beside the report and referenced
    // by digest: ids, hashes and spans only, never original content.
    let source_binding = source_binding(&game_dir);
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m01-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M01-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
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
        jstr(
            "implementer: opencode-1 (Rally #258, session of 2026-09-29T05:06Z); reviewer: \
             opencode-1 — a separate session with fresh context that did not take part in the \
             implementation — regenerating this report on the rebased commit. Same agent name, \
             different context: this review is not independent original-reference evidence and \
             no agent review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite re-run locally with the retail capability by the reviewer; this \
             harness derives every field from the recorded log, production discovery of \
             $CS_GAME_DIR and the binding `SourceContext::read` + `SourceContext::bind` derive \
             from it (all five critical dependencies resolved, checklist entries still unknown \
             recorded in missions/bindings/M01.json); claim is implemented only; validated with \
             tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the \
             commit the suite ran on: the only later delta is this report's own copy under \
             docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M01-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// Evidence-report harness for task M02-A, the second mission's source
/// binding. It follows the sequence in this module's doc with `M02-A` and
/// `accept_m02_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m02_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M02** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records the join corroboration the stage adds, because that is
///   what M02-A's production change is: the localized table's account of the
///   campaign is compared with the campaign directory layout's, and a
///   disagreement would yield no campaign position at all.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m02_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m02_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m02_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M02_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M02-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M02_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M02");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m02-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M02-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
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
        jstr(
            "implementer: bunny-alpha-2 (Rally #261, session of 2026-10-01T04:31Z); reviewer: \
             bunny-alpha-2 again, on the same Rally review claim (2026-10-01T05:25Z). Same agent \
             identity, so this is NOT independent review and is not independent original-reference \
             evidence; the reviewer's context was fresh (a new session that re-read the tree, the \
             installation and the task history) but a fresh context does not make a reviewer \
             independent. No agent review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite re-run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR and the binding \
             `SourceContext::read` + `SourceContext::bind` derive from it (all five critical \
             dependencies resolved; checklist entries still unknown are recorded in \
             missions/bindings/M02.json, not dropped). This stage adds the join corroboration \
             `SourceContext::join_agreement` + `campaign_position_for`, whose contradiction arm no \
             retail installation produces and which is therefore proved on authored values in the \
             synthetic test; claim is implemented only; validated with \
             tools/validate_evidence.py --require-pass. The reviewer regenerated this report on the \
             corrected, rebased commit: `campaign_position_for` now refuses through \
             `JoinAgreement::establishes()` instead of re-reading `agreement.state`, and \
             `accept_m02_a_the_join_is_corroborated_by_the_long_name_rows` additionally asserts \
             that every listed row block is exactly as long as the campaign, which it did not \
             before. The reviewer also re-applied all seven mutations; two rows of \
             docs/findings/2026-10-01-m02-a-source-binding.md were wrong and are corrected there. \
             `candidate_tree` is the tree of the commit this run tested: the only later delta is \
             this report's own copy under docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M02-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m02_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m02_a_")
}

/// Evidence-report harness for task M03-A, the third mission's source
/// binding. It follows the sequence in this module's doc with `M03-A` and
/// `accept_m03_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m03_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M03** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records the join corroboration the stage adds, because that is
///   what M03-A's production change is: the localized table's account of the
///   campaign is compared with the campaign directory layout's, and a
///   disagreement would yield no campaign position at all.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m03_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m03_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m03_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M03_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M03-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M03_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M03");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m03-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M03-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
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
        jstr(
            "implementer: claude-1 (Rally #264, Sonnet 5.5, session of 2026-10-01T19:25Z); reviewer: \
             not yet assigned at hand-over. The implementer's own run is not independent review and \
             is not independent original-reference evidence; no agent review replaces the owner's \
             human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR and the binding \
             `SourceContext::read` + `SourceContext::bind` derive from it (all five critical \
             dependencies resolved; checklist entries still unknown are recorded in \
             missions/bindings/M03.json, not dropped). No production code changed at M03-A: the \
             join, its corroboration and the guard are M02-A's, and this stage exercises them at the \
             third campaign position and pins the world-group case that differs (one campaign \
             mission in world/c1b, plus non-campaign subdirectories); claim is implemented only; \
             validated with tools/validate_evidence.py --require-pass"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M03-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m03_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m03_a_")
}

/// Evidence-report harness for task M04-A, the fourth mission's source
/// binding. It follows the sequence in this module's doc with `M04-A` and
/// `accept_m04_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m04_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M04** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records what this stage pins at the fourth position: the join
///   and its corroboration are M02-A's, but M04's world group `world/c1` is
///   shared by three campaign missions and its mission number `4` is reused by
///   one mission in every chapter, so neither the world id nor the mission
///   number identifies the mission — only the campaign position does.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m04_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m04_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m04_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M04_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M04-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M04_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M04");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m04-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M04-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
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
        jstr(
            "implementer: deepseek-v4.1-flash (Rally #267, session of 2026-10-01); reviewer: \
             not yet assigned at hand-over. The implementer's own run is not independent review \
             and is not independent original-reference evidence; no agent review replaces the \
             owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR and the binding \
             `SourceContext::read` + `SourceContext::bind` derive from it (all five critical \
             dependencies resolved; checklist entries still unknown are recorded in \
             missions/bindings/M04.json, not dropped). No production code changed at M04-A: the \
             join, its corroboration and the guard are M02-A's, and this stage exercises them at \
             the fourth campaign position and pins that neither the shared world group `world/c1` \
             nor the reused mission number `4` identifies the mission; claim is implemented only; \
             validated with tools/validate_evidence.py --require-pass"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M04-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m04_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m04_a_")
}

/// Evidence-report harness for task M06-A, the sixth mission's source
/// binding. It follows the sequence in this module's doc with `M06-A` and
/// `accept_m06_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m06_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M06** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records what M06-A adds: the sixth campaign position is the
///   first mission of chapter 2 and the first row of the localized long names'
///   second region group, and its world group `c2` is shared with four
///   campaign missions while chapter 2's fifth mission lives in `c2b`.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m06_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m06_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m06_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M06_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M06-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M06_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M06");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m06-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M06-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
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
        jstr(
            "implementer: deepseek-1 (Rally #273, DeepSeek V4.1 Flash, session of 2026-10-01T20:41Z); \
             reviewer: deepseek-1/deepseek-1 — a separate session with fresh context (the review \
             claim of 2026-10-01T21:12Z) that did not take part in the implementation — \
             regenerating this report on the rebased commit. Same agent name and model, different \
             context: this review is not independent original-reference evidence and no agent \
             review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite re-run locally with the retail capability by the reviewer; this \
             harness derives every field from the recorded log, production discovery of \
             $CS_GAME_DIR and the binding `SourceContext::read` + `SourceContext::bind` derive \
             from it (all five critical dependencies resolved; checklist entries still unknown are \
             recorded in missions/bindings/M06.json, not dropped). No production code changed at \
             M06-A: the join, its corroboration and the guard are M02-A's, and this stage exercises \
             them at the sixth campaign position, the first mission of chapter 2 and the first row \
             of the localized long names' second region group; claim is implemented only; validated \
             with tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the \
             commit the suite ran on: the only later delta is this report's own copy under \
             docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M06-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m06_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m06_a_")
}

/// Evidence-report harness for task M08-A, the eighth mission's source
/// binding. It follows the sequence in this module's doc with `M08-A` and
/// `accept_m08_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m08_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M08** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records what M08-A adds: the eighth campaign position is the
///   third mission of chapter 2, strictly inside both the layout's chapter
///   group and the localized long names' second region group, and its world
///   group `c2` is shared with four campaign missions while chapter 2's fifth
///   mission lives in `c2b`.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m08_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m08_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m08_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M08_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M08-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M08_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M08");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m08-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M08-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
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
        jstr(
            "implementer: claude-1 (Rally #279, Claude Sonnet 5.5, session of 2026-10-01T23:16Z); \
             reviewer: none yet. The implementer's own run is not independent review, is not \
             independent original-reference evidence, and no agent review replaces the owner's \
             human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability by the implementer; this \
             harness derives every field from the recorded log, production discovery of \
             $CS_GAME_DIR and the binding `SourceContext::read` + `SourceContext::bind` derive \
             from it (all five critical dependencies resolved; checklist entries still unknown are \
             recorded in missions/bindings/M08.json, not dropped). No production code changed at \
             M08-A: the join, its corroboration and the guard are M02-A's, and this stage exercises \
             them at the eighth campaign position, the third mission of chapter 2, strictly inside the \
             layout's chapter group and the localized long names' second region group; claim is implemented only; validated \
             with tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the \
             commit the suite ran on: the only later delta is this report's own copy under \
             docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M08-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m08_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m08_a_")
}

/// Evidence-report harness for task M12-A, the twelfth mission's source
/// binding. It follows the sequence in this module's doc with `M12-A` and
/// `accept_m12_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m12_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M12** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records what M12-A adds: the twelfth campaign position is the
///   second mission of chapter 3, inside both the layout's chapter group and
///   the localized long names' third region group, and its world group `c3`
///   is the whole chapter rather than the mission.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m12_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m12_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m12_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M12_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M12-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M12_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M12");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m12-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M12-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
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
        jstr(
            "implementer: claude-1 (Rally #291, Claude Sonnet 5.5, session of 2026-10-01T23:58Z); \
             reviewer: none yet. The implementer's own run is not independent review, is not \
             independent original-reference evidence, and no agent review replaces the owner's \
             human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability by the implementer; this \
             harness derives every field from the recorded log, production discovery of \
             $CS_GAME_DIR and the binding `SourceContext::read` + `SourceContext::bind` derive \
             from it (all five critical dependencies resolved; checklist entries still unknown are \
             recorded in missions/bindings/M12.json, not dropped). No production code changed at \
             M12-A: the join, its corroboration and the guard are M02-A's, and this stage exercises \
             them at the twelfth campaign position, the second mission of chapter 3, inside the \
             layout's chapter group and the localized long names' third region group, whose world group c3 is the whole chapter; claim is implemented only; validated \
             with tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the \
             commit the suite ran on: the only later delta is this report's own copy under \
             docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M12-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m12_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m12_a_")
}

/// Evidence-report harness for task M13-A, the thirteenth mission's source
/// binding. It follows the sequence in this module's doc with `M13-A` and
/// `accept_m13_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m13_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M13** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records what M13-A adds: the thirteenth campaign position is the
///   third mission of chapter 3, strictly inside both the layout's chapter group and
///   the localized long names' third region group, and its world group `c3`
///   is the whole chapter rather than the mission.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m13_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m13_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m13_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M13_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M13-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M13_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M13");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m13-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M13-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
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
        jstr(
            "implementer: claude-1 (Rally #294, Claude Sonnet 5.5, session of 2026-10-02T00:10Z); \
             reviewer: none yet. The implementer's own run is not independent review, is not \
             independent original-reference evidence, and no agent review replaces the owner's \
             human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability by the implementer; this \
             harness derives every field from the recorded log, production discovery of \
             $CS_GAME_DIR and the binding `SourceContext::read` + `SourceContext::bind` derive \
             from it (all five critical dependencies resolved; checklist entries still unknown are \
             recorded in missions/bindings/M13.json, not dropped). No production code changed at \
             M13-A: the join, its corroboration and the guard are M02-A's, and this stage exercises \
             them at the thirteenth campaign position, the third mission of chapter 3, strictly inside the \
             layout's chapter group and the localized long names' third region group, whose world group c3 is the whole chapter; claim is implemented only; validated \
             with tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the \
             commit the suite ran on: the only later delta is this report's own copy under \
             docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M13-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m13_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m13_a_")
}

/// Evidence-report harness for task M05-A, the fifth mission's source
/// binding. It follows the sequence in this module's doc with `M05-A` and
/// `accept_m05_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m05_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M05** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records the two-display-form title confirmation M05-A adds: the
///   declared title is carried only as the title part of a region-prefixed long
///   name, the bare short name of the same campaign position spells it
///   differently, and that difference is reported rather than reconciled — the
///   record stays `verified: false` and the report's own `claim` stays
///   `implemented`.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m05_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m05_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m05_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M05_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M05-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M05_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M05");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    // The record this stage is evidence for is not finished, and the report
    // must not read as if it were: an unreconciled title spelling and the
    // unbound checklist entries both keep it out of any verified claim.
    assert!(
        !source_binding.is_verified() && !source_binding.unknowns.is_empty(),
        "the M05 record reads as verified or complete; this stage binds source identity only"
    );
    let binding_path = evidence_dir.join("m05-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M05-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": [],\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
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
        jstr(
            "implementer: bunny-2/bunny-2 (Rally #270, session of 2026-10-01T20:17Z); reviewer: \
             bunny-2/bunny-2 again, as the Rally reviewing agent, with fresh context but not a \
             different agent instance, so this review is NOT independent and is not independent \
             original-reference evidence; no agent review replaces the owner's human approval. \
             The review re-derived the retail facts from cs-inspect config rather than the \
             implementer's tables, ran the full four checks on the rebased commit, corrected the \
             TitleConfirmation::Ambiguous doc comment and the uncarried-title table, and paired \
             each title-confirmation failure arm with the refusal it must produce"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR and the binding \
             `SourceContext::read` + `SourceContext::bind` derive from it (all five critical \
             dependencies resolved; checklist entries still unknown are recorded in \
             missions/bindings/M05.json, not dropped). This stage's production change is the \
             two-display-form title confirmation: M05's declared title is carried only as the \
             title part of a region-prefixed long name, the installation's bare short name for \
             the same campaign position spells it without the leading article, and the record \
             reports that difference instead of reconciling it, so verified stays false; claim \
             is implemented only; validated with tools/validate_evidence.py --require-pass"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M05-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!("wrote {}", out.display());
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m05_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m05_a_")
}

/// The retail acceptance tests M11-A's capabilities are judged on.
const RETAIL_TESTS_M11_A: &[&str] = &[
    "accept_m11_a_the_declared_title_is_carried_by_no_retail_row",
    "accept_m11_a_the_source_derived_binding_keeps_m11_unresolved_and_says_why",
    "accept_m11_a_the_committed_record_is_what_the_installation_derives",
    "accept_m11_a_the_declared_order_agrees_with_the_retail_campaign_order_where_it_is_carried",
    "accept_m11_a_the_position_the_declared_order_would_select_is_read_from_the_installation",
    "accept_m11_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M11-A's report must also record.
const SYNTHETIC_TESTS_M11_A: &[&str] = &[
    "accept_m11_a_only_an_exact_title_or_an_exact_long_name_tail_confirms",
    "accept_m11_a_an_unresolved_critical_dependency_is_named_in_the_records_unknowns",
];

/// Evidence-report harness for task M11-A, the eleventh mission's source
/// binding. It follows the sequence in this module's doc with `M11-A` and
/// `accept_m11_a_` in place of `M01-A` and `accept_m01_a_`, and differs from
/// the M01-A report above in one way that matters:
///
/// **This task does not meet its minimum acceptance scenario, and the report
/// says so.** The scenario is "Source-derived binding has no unresolved
/// critical dependencies"; M11's declared discovery title is carried by no
/// localized row of the installation, so four of the five critical dependencies
/// are unresolved. Every other harness above asserts
/// `unresolved_critical().is_empty()` before it writes anything; this one
/// asserts the *expected unresolved set* and the refusal each dependency
/// carries, writes those refusals into the report's `unknowns`, and prints
/// that the report is therefore **expected not to validate** under
/// `--require-pass`. Dropping an unresolved issue to make a validator pass is
/// forbidden (`docs/contracts/CLI-EVIDENCE.md`, AUDIT-PLAN-SYNC 2026-10-01), so
/// the honest report is the one that keeps them.
///
/// The acceptance suite itself passes: what it asserts is that the refusal is
/// real, exact and recorded — not that M11 is bound. Which retail string names
/// M11 is Rally #470 `M05-A-GUIDE-TITLES`, an owner decision recorded as
/// `blocked`.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived from
/// the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m11_a_writes_the_acceptance_report() {
    let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
    let candidate_tree = env_var("CS_CANDIDATE_TREE");
    let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert!(
        !argv.is_empty(),
        "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
    );
    let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
        .parse()
        .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
    let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_m11_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m11_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M11_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M11-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M11_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The stage's own state: M11 is not bound, and the report must not imply
    // otherwise. The unresolved set is checked against the installation's
    // measured refusal rather than written in by hand.
    let source_binding = source_binding_for(&game_dir, "M11");
    assert!(
        !source_binding.unresolved_critical().is_empty(),
        "M11 resolved every critical dependency, so this stage's refusal is stale; the report and \
         the finding must be rewritten before it is produced"
    );
    assert!(
        !source_binding.is_verified() && !source_binding.unknowns.is_empty(),
        "the M11 record reads as verified or complete; this stage binds nothing"
    );
    let binding_path = evidence_dir.join("m11-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    // The report's `unknowns` are this task's blockers, read from the record
    // production code derived from the installation.
    let unknowns: Vec<String> = source_binding
        .unresolved_critical()
        .iter()
        .map(|id| {
            let reason = match source_binding.dependency(*id) {
                Some(cs_content::campaign_bindings::DependencyState::Unresolved {
                    reason, ..
                }) => reason.clone(),
                _ => panic!("{id} is listed as unresolved but carries no refusal"),
            };
            format!("{}: unresolved — {reason}", id.label())
        })
        .collect();
    assert!(
        unknowns.contains(&format!(
            "{}: unresolved — {}",
            cs_content::campaign_bindings::CriticalDependency::TitleString.label(),
            cs_content::campaign_bindings::UNCARRIED_TITLE_REFUSAL
        )),
        "the report's unknowns do not name the uncarried title, so they are not this task's \
         real blockers: {unknowns:?}"
    );
    let unknown_array = {
        let items: Vec<String> = unknowns.iter().map(|entry| jstr(entry)).collect();
        format!("[{}]", items.join(", "))
    };

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M11-A\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
         \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
         \x20\"assertions\": [{}],\n\
         \x20\"artifacts\": [{}],\n\
         \x20\"unknowns\": {},\n\
         \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
         \x20\"claim\": \"implemented\"\n\
         }}\n",
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
        unknown_array,
        jstr(
            "implementer: bunny-alpha-1/bunny-alpha-1 (Rally #288, session of 2026-10-02T00:00Z); \
             reviewer: none yet — this report was written by the implementing session, so no \
             review has taken place. No agent review replaces the owner's human approval, and no \
             agent may self-award checked, verified_original or release_approved"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR and the binding \
             `SourceContext::read` + `SourceContext::bind` derive from it. The suite passes and \
             M11 is still UNBOUND: the declared discovery title is carried by no localized row, \
             so four of the five critical dependencies are unresolved and the mission, world and \
             program identities are null. This report is therefore expected NOT to validate under \
             tools/validate_evidence.py --require-pass, which rejects a report with unresolved \
             issues; removing them to make it pass is forbidden. Resolving them needs Rally #470 \
             M05-A-GUIDE-TITLES, an owner decision. M11-A's production change names every \
             unresolved critical dependency in the record's own unknowns; claim is implemented \
             only. `candidate_tree` is the tree of the commit the suite ran on"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M11-A\"",
        "\"claim\": \"implemented\"",
        "\"install_sha256\"",
        "\"assertions\": [",
        "\"artifacts\": [",
        "\"unknowns\": [\"mission_id: unresolved",
    ] {
        assert!(
            written.contains(needle),
            "the written report is missing {needle:?}:\n{written}"
        );
    }
    assert!(
        suite.failed == 0 && exit_code == 0,
        "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
         honestly and must NOT validate; fix the tests first",
        suite.failed
    );
    println!(
        "wrote {} — M11-A does not meet its minimum acceptance scenario, so this report is \
         expected NOT to validate under --require-pass; that is the honest state, not a defect \
         of the harness",
        out.display()
    );
}

/// [`parse_suite`] with this task's test prefix.
fn parse_m11_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m11_a_")
}

// ---------------------------------------------------------------- inputs ---

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set: this harness only runs through the sequence in its module doc \
             (crates/cs_app/tests/campaign/evidence.rs)"
        )
    })
}

/// Cargo runs a test binary with its working directory set to the *package*
/// root, so a path like `private/evidence/M01-A` written relative to the
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

/// The M01 binding derived from the installation, built once.
fn source_binding(game_dir: &Path) -> &'static SourceBinding {
    static BINDING: OnceLock<SourceBinding> = OnceLock::new();
    BINDING.get_or_init(|| source_binding_for(game_dir, "M01"))
}

/// One work order's binding derived from the installation, through the same
/// production path `SourceContext::read` + `SourceContext::bind` the
/// acceptance suite uses. The declared discovery title comes from the
/// committed inventory, never from this file.
fn source_binding_for(game_dir: &Path, work_order: &str) -> SourceBinding {
    let context = SourceContext::read(game_dir)
        .expect("production source context reads the original installation");
    let title = declared_title(work_order);
    context
        .bind(
            cs_content::campaign_bindings::MissionLabel::new(work_order)
                .unwrap_or_else(|error| panic!("{work_order} is not a valid label: {error}")),
            &title,
        )
        .unwrap_or_else(|error| panic!("{work_order} binds to the original data: {error}"))
}

/// The declared discovery title of one work order, read from the committed
/// inventory.
fn declared_title(work_order: &str) -> String {
    let inventory = fs::read_to_string(
        Path::new(&git(&["rev-parse", "--show-toplevel"]))
            .join("missions/bindings/campaign-inventory.tsv"),
    )
    .expect("the declared campaign inventory reads");
    inventory
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| line.split_once('\t'))
        .find(|(label, _)| *label == work_order)
        .map(|(_, title)| title.trim().to_owned())
        .unwrap_or_else(|| panic!("the declared inventory has no {work_order} work order"))
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
/// `accept_m01_a_` tests from a recorded `cargo test` output.
fn parse_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m01_a_")
}

/// [`parse_suite`] with a task's own test prefix, so one parser serves every
/// mission binding stage and no report can record another task's assertions.
fn parse_suite_prefixed(log: &str, prefix: &str) -> Suite {
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
        // A status on its own line completes the earliest test that was
        // started on an earlier line without an inline status.
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
        // `test <name> ... <status>`, possibly several per interleaved line.
        // Test names carry their module path (`m01_a::accept_m01_a_…`);
        // the report records the leaf, which is what the prefix selects.
        let mut cursor = trimmed;
        while let Some(position) = cursor.find("test ") {
            let after = &cursor[position + 5..];
            let Some(separator) = after.find(" ... ") else {
                break;
            };
            let full = &after[..separator];
            if !full.contains(prefix) {
                cursor = &after[separator + 5..];
                continue;
            }
            let name = full.rsplit("::").next().expect("a name").to_owned();
            let tail = &after[separator + 5..];
            cursor = tail;
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

/// One referenced artifact: hashed here with the production SHA-256 of this
/// workspace (the validator re-hashes it with `hashlib` independently).
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
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year_of_day = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = (if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    }) as u32;
    let year = if month <= 2 {
        year_of_day + 1
    } else {
        year_of_day
    };
    (
        year,
        month,
        day,
        (rest / 3_600) as u32,
        ((rest % 3_600) / 60) as u32,
        (rest % 60) as u32,
    )
}
