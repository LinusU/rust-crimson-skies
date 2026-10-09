//! Evidence-report harness for task M06-B-FU1: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M06-B-FU1's capability is judged on: M06's
/// three `COMPLETION_COUNT` `ANIM_STATE` sites lower through the measured
/// operand-list walk, and the row is complete while the campaign gate stays
/// closed.
const RETAIL_TESTS_M06_B_FU1: &[&str] = &[
    "accept_m06_b_fu1_the_completion_count_sites_lower_and_m06s_record_completes",
    "accept_m06_b_fu1_m06_is_complete_and_the_campaign_stays_unready",
];

/// The synthetic predicate test M06-B-FU1's report must also record: M06's
/// own three-site spelling lowered on an authored record, into CI, where
/// there is no original data.
const SYNTHETIC_TESTS_M06_B_FU1: &[&str] =
    &["accept_m06_b_fu1_m06s_spelling_lowers_all_three_sites"];

/// Evidence-report harness for task M06-B-FU1 (Rally #817), the follow-up
/// that pins M06's three `COMPLETION_COUNT` `ANIM_STATE` condition sites
/// lowering through the shared operand-list walk M04-B-FU1 (#806) landed.
/// It follows the sequence in this module's doc with `M06-B-FU1` and
/// `accept_m06_b_fu1_` in place of `M01-A` and `accept_m01_a_`:
///
/// * the acceptance-log parser selects `accept_m06_b_fu1_` tests, so the
///   recorded assertions are this follow-up's own — the selection must be
///   the prefix's own run (step 1 with that prefix), or the counts would
///   describe a wider run than the assertions;
/// * the only artifact is that log: the change is acceptance pinning over a
///   landed mechanism, so the report cites the run that proves it rather
///   than a derived binding;
/// * `review.identity` is a literal naming the real Rally actors and the
///   reviewer's context, as the module doc requires.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M06-B report above.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m06_b_fu1_writes_the_acceptance_report() {
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
    let suite = parse_m06_b_fu1_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m06_b_fu1_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M06_B_FU1 {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M06-B-FU1 requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M06_B_FU1 {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{synthetic_test} did not run: it pins the M06 spelling the retail test assumes"
                )
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
         \x20\"task_id\": \"M06-B-FU1\",\n\
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
            "acceptance suite run locally with the retail capability; the \
             fields are derived from the recorded log and production discovery of $CS_GAME_DIR. \
             The suite pins this task's verdict: M06's three six-operand ANIM_STATE sites \
             (blocks 9, 11 and 41 — COMPLETION_COUNT [1] plus the path3/path4/path5 \
             continue/accelerate RUNNING descriptors, each its block's first directive) lower \
             the measured way through M04-B-FU1 #806's operand-list walk — both pairs append, \
             the in-list count overwrites `required`, each operand list binds as one \
             Value::List argument — so all 265 calls bind, all 82 conditions lower, \
             MissionProgram::validate accepts and M06's census row is complete while the \
             campaign gate stays closed. NOT CLAIMED: no mission was played, no original \
             executable was run and nothing is verified_original; the runtime half of the \
             evaluator (which world writes put an animation in its wanted state) stays with \
             M06-C. Claim is implemented only; validated with \
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
        "\"task_id\": \"M06-B-FU1\"",
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
fn parse_m06_b_fu1_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m06_b_fu1_")
}
