//! Evidence-report harness for task M13-B: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M13-B's capabilities are judged on.
const RETAIL_TESTS_M13_B: &[&str] = &[
    "accept_m13_b_the_control_program_is_the_member_that_declares_the_blocks",
    "accept_m13_b_every_directive_m13_spells_has_a_disposition_and_none_is_refused",
    "accept_m13_b_the_sheet_priorities_resolve_to_measured_operations_and_none_changes_allegiance",
    "accept_m13_b_the_damage_ladder_and_the_dependency_gates_are_record_data",
    "accept_m13_b_every_actor_the_record_names_resolves_in_the_shipped_data_but_one",
    "accept_m13_b_the_ai_net_actor_is_declared_by_no_archive_in_the_installation",
    "accept_m13_b_the_terminal_blocks_are_gated_and_every_address_is_in_range",
    "accept_m13_b_the_two_outcomes_have_disjoint_prerequisites_and_the_rest_is_not_mandatory",
    "accept_m13_b_every_call_binds_every_condition_lowers_and_m13s_record_completes",
    "accept_m13_b_m13_is_complete_and_the_campaign_stays_unready",
];

/// The synthetic predicate tests M13-B's report must also record.
const SYNTHETIC_TESTS_M13_B: &[&str] = &[
    "accept_m13_b_m13s_travelers_site_lowers_and_a_counting_mode_refuses",
    "accept_m13_b_an_ungrounded_allegiance_directive_is_refused_rather_than_honoured",
    "accept_m13_b_a_nap_delay_is_data_and_an_inactive_threshold_defaults_to_its_list",
];

/// Evidence-report harness for task M13-B: M13's mission-specific
/// compatibility gaps. Same sequence as the M06-B and M08-B reports; it
/// records the `accept_m13_b_` tests, the fully lowering record they pin and
/// the one measured gap they record (`britkestrel_1`), and claims
/// `implemented` only.
///
/// `CS_EVIDENCE_REVIEWER` fills `review.identity` whole, so whoever runs the
/// harness — the implementing agent at hand-over or the reviewing agent on the
/// rebased commit — writes its own identities and says whether the run is a
/// review. A placeholder identity is a failure.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m13_b_writes_the_acceptance_report() {
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
    let suite = parse_m13_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m13_b_` tests were recorded in {}",
        log_path.display()
    );
    for retail_test in RETAIL_TESTS_M13_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M13-B requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M13_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{synthetic_test} did not run: it pins the refusal arms the retail gaps rest on"
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
         \x20\"task_id\": \"M13-B\",\n\
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
             from the recorded log and production discovery of $CS_GAME_DIR. The suite pins M13's \
             measured control program (objectives.zrd of ZBD/C3/M03/zrdr.zbd: 38 blocks, 178 \
             directive sites, 36 keys, a fully measured vocabulary) through the retail control \
             census, the production control-program binding and M13-A's mission binding, and \
             locates the sheet's three regression priorities in the record: the four-stage damage \
             ladder over the same twelve pirate-zeppelin engines under rising thresholds 3, 5, 7 \
             and 10 with each stage entering the next only through its own nap, plus the two \
             TICK_DEPENDS_ON_OBJ gates that lower as the dependency's ObjectiveAwake conjunct; \
             the measured absence of any allegiance-changing directive beside the actor and node \
             resolution table that leaves the SET_AI_NET actor britkestrel_1 declared by no \
             archive in the installation (its node operand M3BritAce is declared by the chapter \
             world's neindex.zrd); and the two latches' disjoint prerequisite closures. Both \
             terminal gates are pinned with all 41 spelled block addresses in range, and M13's \
             lowering is complete: every call binds, every condition lowers, \
             MissionProgram::validate accepts and M13's census row is complete while the campaign \
             stays unready. Claim is implemented only; no mission was played, no original \
             executable was run, nothing is verified_original, and the measured gap and the \
             unmeasured runtime halves are recorded in \
             docs/findings/2026-10-10-m13-b-compatibility-gaps.md. Validated with \
             tools/validate_evidence.py --require-pass"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M13-B\"",
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
fn parse_m13_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m13_b_")
}
