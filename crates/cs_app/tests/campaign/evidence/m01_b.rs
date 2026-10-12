//! Evidence-report harness for task M01-B (Rally #259): the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M01-B's `retail` capability is judged on: the
/// parent suite's binding pin (M01's derived and committed records carry the
/// measured statements; M02 keeps the shared checklist), the M01-B-WAKE legs
/// that drive both of M01's `Finish` blocks through the measured world facts,
/// and the M01-B-OBJ retail legs whose census the binding entries quote.
const RETAIL_TESTS_M01_B: &[&str] = &[
    "accept_m01_b_m01s_record_binds_the_measured_results_and_m02s_does_not",
    "accept_m01_b_wake_retail_m01_carries_the_recovered_lifecycle_model",
    "accept_m01_b_wake_a_driven_run_reaches_instantwin_through_measured_facts",
    "accept_m01_b_wake_a_driven_run_reaches_instantloss_through_measured_facts",
    "accept_m01_b_wake_an_unobserved_world_reaches_no_terminal",
    "accept_m01_b_wake_a_missing_member_stalls_the_chain_at_its_threshold",
    "accept_m01_b_wake_removing_the_lifecycle_model_blocks_the_effects",
    "accept_m01_b_obj_integration_retail_m01_every_family_is_measured_and_the_directives_are_named",
    "accept_m01_b_obj_1_retail_m01_recovers_its_effects_and_dependencies",
    "accept_m01_b_obj_2_retail_m01_recovers_every_lifecycle_field",
    "accept_m01_b_obj_3_retail_m01_recovers_every_condition_and_outcome_the_census_reads",
    "accept_m01_b_obj_4_retail_m01_accounts_for_every_spelled_directive",
];

/// The synthetic predicate tests M01-B's report must also record: the
/// checklist-override mechanism over the whole declared inventory, the
/// driven-wake mechanism's CI legs and the objective-recovery refusal legs —
/// all of which run in CI, where there is no original data.
const SYNTHETIC_TESTS_M01_B: &[&str] = &[
    "accept_m01_b_a_settled_checklist_entry_reaches_only_the_work_order_it_was_settled_for",
    "accept_m01_b_wake_synthetic_chain_reaches_terminal_through_lifecycle_effects",
    "accept_m01_b_wake_synthetic_chain_without_the_model_stays_dormant",
    "accept_m01_b_obj_integration_fully_recovered_record_refuses_by_the_schema_gap",
    "accept_m01_b_obj_integration_unmeasured_key_is_still_refused_by_name",
];

/// Evidence-report harness for task M01-B (Rally #259), the parent stage
/// that binds its subtasks' measured results into the M01 binding record. It
/// follows the sequence in this module's doc with `M01-B` and
/// `accept_m01_b_` in place of `M01-A` and `accept_m01_a_`:
///
/// * the acceptance-log parser selects `accept_m01_b_` tests, so the
///   recorded assertions are this task family's own — the selection must be
///   the prefix's own run (step 1 with that prefix), or the counts would
///   describe a wider run than the assertions;
/// * the artifacts beside the report are the **M01** and **M02** bindings,
///   derived from the committed inventory's declared titles through
///   production code — the derivation whose mission-scoped checklist
///   overrides this task added and regenerated `missions/bindings/M01.json`
///   from — so the report cites the records it is evidence for, including
///   the contrast record that keeps the shared checklist;
/// * the review identity is written by whoever runs the harness, as the
///   module doc requires: this report is the implementer's hand-over run, and
///   the reviewing agent regenerates it on the reviewed commit.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_b_writes_the_acceptance_report() {
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
    let reviewer = env_var("CS_EVIDENCE_REVIEWER");
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
    let suite = parse_m01_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m01_b_` tests were recorded in {}",
        log_path.display()
    );
    assert_eq!(
        suite.failed, 0,
        "the recorded acceptance log contains failures; fix the tests before writing the report"
    );
    for (tests, capability) in [
        (RETAIL_TESTS_M01_B, "retail"),
        (SYNTHETIC_TESTS_M01_B, "synthetic"),
    ] {
        for task_test in tests {
            let status = suite
                .assertions
                .iter()
                .find(|(name, _)| name == task_test)
                .map(|(_, status)| *status)
                .unwrap_or_else(|| {
                    panic!(
                        "{task_test} did not run: M01-B needs {capability}, run step 1 with \
                         `--include-ignored` and CS_GAME_DIR set"
                    )
                });
            assert_eq!(status, "pass", "{task_test} must pass; got status {status}");
        }
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let m01_binding = source_binding_for(&game_dir, "M01");
    assert!(
        m01_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    for settled in [
        "objective graph: measured for this mission",
        "script/native coverage: the mission opcode table is still unmeasured",
        "exact failure/success precedence: measured for this mission",
    ] {
        assert!(
            m01_binding
                .unknowns
                .iter()
                .any(|entry| entry.starts_with(settled)),
            "the derived M01 record no longer carries the measured statement {settled:?}; the \
             report must not be written"
        );
    }
    let m01_path = evidence_dir.join("m01-binding.json");
    fs::write(&m01_path, m01_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", m01_path.display()));

    let m02_binding = source_binding_for(&game_dir, "M02");
    assert!(
        m02_binding
            .unknowns
            .iter()
            .any(|entry| entry == "objective graph: not bound from original data at this stage"),
        "the derived M02 record must keep the shared checklist entries M01's measurements \
         settled for M01 alone; the report must not be written"
    );
    let m02_path = evidence_dir.join("m02-binding.json");
    fs::write(&m02_path, m02_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", m02_path.display()));

    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&m01_path, "json", &evidence_dir));
    artifacts.push(artifact(&m02_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M01-B\",\n\
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
        jstr(&reviewer),
        jstr(
            "acceptance suite run locally with the retail capability; the fields are derived \
             from the recorded log, production discovery of $CS_GAME_DIR and the bindings \
             `SourceContext::read` + `SourceContext::bind` derive from it (all five critical \
             dependencies resolved on both records). M01-B is the parent stage that binds its \
             subtasks' measured results into missions/bindings/M01.json: cs_content:: \
             campaign_bindings gained a mission-scoped checklist-override table \
             (mission_scoped_checklist_overrides) that replaces — on M01's record alone — the \
             three shared checklist entries M01-B-OBJ, M01-B-OPC and M01-B-WAKE measured (the \
             objective graph, script/native coverage and the failure/success precedence), each \
             replacement naming the census it rests on and the residuals it did not settle, \
             while M02 and every other mission keep the shared text. The committed M01.json was \
             regenerated through production code (regenerate_m01_b_writes_the_committed_record_ \
             through_production), so accept_m01_a's byte-equality pin stays the record's own. \
             NOT CLAIMED: nothing is verified_original, no original executable was run, the \
             record keeps verified false with every residual named, and ordinary-play success, \
             failure and continuity stay with M01-C. Claim is implemented only; validated with \
             tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the \
             commit the suite and this harness ran on; the only later delta is this report's \
             own copy under docs/findings/evidence/, which no acceptance test reads"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M01-B\"",
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

/// [`parse_suite_prefixed`] with this task's test prefix.
fn parse_m01_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m01_b_")
}
