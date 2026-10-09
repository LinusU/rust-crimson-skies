//! Evidence-report harness for task M03-B: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M03-B's capabilities are judged on.
const RETAIL_TESTS_M03_B: &[&str] = &[
    "accept_m03_b_the_control_program_is_the_member_that_declares_the_blocks",
    "accept_m03_b_every_directive_m03_spells_has_a_disposition_and_one_is_refused",
    "accept_m03_b_the_record_does_not_lower_and_exactly_three_sites_refuse",
    "accept_m03_b_the_sheet_priorities_resolve_to_measured_operations",
    "accept_m03_b_the_terminal_blocks_are_gated_and_every_address_is_in_range",
    "accept_m03_b_the_mission_stays_unready_until_the_refused_sites_are_measured",
];

/// The synthetic predicate tests M03-B's report must also record.
const SYNTHETIC_TESTS_M03_B: &[&str] = &[
    "accept_m03_b_a_key_with_two_shapes_is_refused_and_no_program_assembles",
    "accept_m03_b_a_net_assignment_past_the_host_call_bound_refuses_and_a_small_one_binds",
];

/// Evidence-report harness for task M03-B: M03's mission-specific
/// compatibility gaps. Same sequence as the M03-A report; it records the
/// `accept_m03_b_` tests and the refusals they pin, and claims `implemented`
/// only.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m03_b_writes_the_acceptance_report() {
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
    let suite = parse_m03_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m03_b_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M03_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M03-B requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M03_B {
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

    let artifacts = vec![artifact(&log_path, "log", &evidence_dir)];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M03-B\",\n\
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
            "implementer: claude-2/claude-2 (Rally #265, Sonnet 5.5, session of 2026-10-08); \
             reviewer: bunny-2/bunny-2 (Rally review claim of 2026-10-08T21:46Z) — a different \
             agent instance and model with a fresh context, so this review is independent of the \
             implementation, but it is an agent review of the code and tests, not independent \
             original-reference evidence and not original-run evidence; no agent review replaces \
             the owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; the fields are derived \
             from the recorded log and production discovery of $CS_GAME_DIR. The suite pins \
             M03's measured control program and its refused lowering (three sites, two follow-ups); \
             claim is implemented only; the mission is NOT ready: the open gaps (WAKEUP_OBJECTIVE_WHEN_I_COMPLETE, task M03-B-FU1; SET_AI_NET host-call bound, task M02-B-FU1) are recorded in docs/findings/2026-10-08-m03-b-control-program-gaps.md. \
             The reviewer re-ran the whole acceptance suite with CS_GAME_DIR on the rebased \
             commit, regenerated this report from that run and validated it with \
             tools/validate_evidence.py --require-pass"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M03-B\"",
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
fn parse_m03_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m03_b_")
}
