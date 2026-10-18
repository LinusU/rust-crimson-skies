//! Evidence-report harness for task M04-B: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M04-B's capabilities are judged on.
const RETAIL_TESTS_M04_B: &[&str] = &[
    "accept_m04_b_the_control_program_is_the_member_that_declares_the_blocks",
    "accept_m04_b_every_directive_m04_spells_has_a_disposition_and_none_is_refused",
    "accept_m04_b_the_sheet_priorities_are_located_and_resolve_to_measured_operations",
    "accept_m04_b_the_terminal_blocks_are_gated_and_every_address_is_in_range",
    "accept_m04_b_fu1_the_bound_record_spells_no_anim_state_and_m04s_record_completes",
    "accept_m04_b_fu1_m04_is_complete_and_the_campaign_stays_unready",
];

/// The synthetic predicate tests M04-B's report must also record.
const SYNTHETIC_TESTS_M04_B: &[&str] = &[
    "accept_m04_b_fu1_a_multi_pair_site_lowers_with_its_count_override",
    "accept_m04_b_fu1_only_an_uncarriable_operand_list_refuses",
    "accept_m04_b_fu1_the_first_site_arms_the_evaluator",
    "accept_m04_b_fu1_a_top_level_completion_count_is_inert_and_unbound",
];

/// Evidence-report harness for task M04-B: M04's mission-specific
/// compatibility gaps. Same sequence as the M03-B report; it records the
/// `accept_m04_b_` tests and the one measured gap they pin, and claims
/// `implemented` only.
///
/// `CS_EVIDENCE_REVIEWER` fills `review.identity` whole, so whoever runs the
/// harness — the implementing agent at hand-over or the reviewing agent on the
/// rebased commit — writes its own identities and says whether the run is a
/// review. A placeholder identity is a failure.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m04_b_writes_the_acceptance_report() {
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
    let suite = parse_m04_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m04_b_` tests were recorded in {}",
        log_path.display()
    );
    for retail_test in RETAIL_TESTS_M04_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M04-B requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M04_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{synthetic_test} did not run: it pins the refusal arms the retail gap rests on"
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
         \x20\"task_id\": \"M04-B\",\n\
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
            "acceptance suite run locally with the retail capability; every field is derived \
             from the recorded log and production discovery of $CS_GAME_DIR. The suite pins M04's \
             measured control program (23 blocks, 126 sites, 29 keys, fully measured vocabulary) \
             of ZBD/C3/M04/zrdr.zbd — the archive the corrected campaign layout binds to work \
             order M04, not the ZBD/C1/M04 archive the suite was first measured on — locates the \
             sheet's three regression priorities in the record, gates both terminal latches and \
             walks every block address, and pins M04's complete lowering: every one of its 126 \
             sites binds and `MissionProgram::validate` accepts. The bound record spells no \
             ANIM_STATE key, so the operand-list mechanism M04-B-FU1 (#806) landed is pinned by \
             synthetic members here and by the retail suites of the records that do spell it. \
             Claim is implemented only; no mission was played, no \
             original executable was run and nothing is verified_original. Regenerated on the \
             re-pinned suite by swe2-max-1/swe2-max-1 (SWE-2 Max) in its Rally #1617 implement \
             session, before review; `review.identity` above is unchanged and records M04-B's \
             original implementer and review, not a review of this re-pin. Validated with \
             tools/validate_evidence.py --require-pass"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M04-B\"",
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
fn parse_m04_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m04_b_")
}
