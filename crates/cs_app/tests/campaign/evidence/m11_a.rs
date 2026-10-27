//! Evidence-report harness for task M11-A: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M11-A's capabilities are judged on.
const RETAIL_TESTS_M11_A: &[&str] = &[
    "accept_m11_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m11_a_the_committed_record_is_what_the_installation_derives",
    "accept_m11_a_the_declared_title_is_joined_by_the_ruling_and_by_nothing_else",
    "accept_m11_a_the_declared_order_agrees_with_the_retail_campaign_order_where_it_is_carried",
    "accept_m11_a_the_position_the_declared_order_would_select_is_read_from_the_installation",
    "accept_m11_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M11-A's report must also record.
const SYNTHETIC_TESTS_M11_A: &[&str] = &[
    "accept_m11_a_only_an_exact_title_or_an_exact_long_name_tail_confirms",
    "accept_m11_a_a_ruled_title_is_named_in_the_ruling_table_and_nowhere_else",
    "accept_m11_a_an_unresolved_critical_dependency_is_named_in_the_records_unknowns",
];

/// Evidence-report harness for task M11-A, the eleventh mission's source
/// binding. It follows the sequence in this module's doc with `M11-A` and
/// `accept_m11_a_` in place of `M01-A` and `accept_m01_a_`. M11 is one of the
/// seven work orders whose declared title the installation spells differently,
/// so the report records the title dependency as inferred through the owner
/// ruling of Rally #470 and requires every critical dependency resolved.
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

    let source_binding = source_binding_for(&game_dir, "M11");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    assert!(
        source_binding
            .unknowns
            .iter()
            .any(|entry| entry.starts_with("title ruling:")),
        "the M11 record is bound, so it must say its title join rests on the owner ruling"
    );
    let binding_path = evidence_dir.join("m11-binding.json");
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
            "implementer: deepseek-1 (Rally #477, DeepSeek V4.1 Flash, session of 2026-10-27); \
             reviewer: none yet. An agent review is not independent original-reference \
             evidence and no agent review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability by the implementer; \
             this harness derives every field from the recorded log, production discovery of \
             $CS_GAME_DIR and the binding `SourceContext::read` + `SourceContext::bind` derive \
             from it (all five critical dependencies resolved, the title one as Inferred through \
             the owner ruling of Rally #470; checklist entries still unknown are recorded in \
             missions/bindings/M11.json, not dropped). Production code changed at M11-A: \
             `unresolved_critical_entries` names an unresolved critical dependency in the \
             record's own unknowns, so a refused record explains itself; see \
             docs/findings/2026-10-27-m11-a-source-binding.md. Claim is implemented only; \
             validated with tools/validate_evidence.py --require-pass. `candidate_tree` is the \
             tree of the commit the suite ran on: the only later delta is this report's own copy \
             under docs/findings/evidence/, whose bytes are this file"
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
        "\"unknowns\": []",
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
fn parse_m11_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m11_a_")
}
