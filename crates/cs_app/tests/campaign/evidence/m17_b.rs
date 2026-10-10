//! Evidence-report harness for task M17-B: the task's own test lists and its
//! `evidence_report_*` harness. Shared helpers, types and imports live in
//! `super` — see `crates/cs_app/tests/campaign/evidence.rs`, which is also
//! where the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M17-B's capabilities are judged on.
const RETAIL_TESTS_M17_B: &[&str] = &[
    "accept_m17_b_the_control_program_is_the_member_that_declares_the_blocks",
    "accept_m17_b_every_directive_m17_spells_has_a_disposition_and_none_is_refused",
    "accept_m17_b_no_directive_writes_the_player_and_the_airframe_is_the_campaign_chains",
    "accept_m17_b_the_search_triggers_are_the_four_always_awake_danger_zone_blocks",
    "accept_m17_b_the_ace_lifecycle_is_record_data_and_two_actors_are_declared_nowhere",
    "accept_m17_b_every_text_the_record_spells_is_declared_outside_it_or_recorded_as_a_gap",
    "accept_m17_b_the_single_terminal_latch_is_gated_and_every_address_is_in_range",
    "accept_m17_b_every_call_binds_every_condition_lowers_and_m17s_record_completes",
    "accept_m17_b_m17_is_complete_and_the_campaign_stays_unready",
];

/// The synthetic predicate tests M17-B's report must also record: they run in
/// CI without original data and carry the refusal arms.
const SYNTHETIC_TESTS_M17_B: &[&str] = &[
    "accept_m17_b_a_warp_site_binds_at_its_measured_shape_and_refuses_a_value_the_ir_cannot_carry",
    "accept_m17_b_the_zone_evaluator_pairs_with_its_threshold_and_an_unknown_key_refuses",
];

/// Evidence-report harness for task M17-B: M17's mission-specific
/// compatibility gaps. Same sequence as the M13-B report; it records the
/// `accept_m17_b_` tests, the fully lowering record they pin and the measured
/// gaps they record, and claims `implemented` only.
///
/// `CS_EVIDENCE_REVIEWER` fills `review.identity` whole, so whoever runs the
/// harness — the implementing agent at hand-over or the reviewing agent on the
/// rebased commit — writes its own identities and says whether the run is a
/// review. A placeholder identity is a failure.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m17_b_writes_the_acceptance_report() {
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
    let suite = parse_m17_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m17_b_` tests were recorded in {}",
        log_path.display()
    );
    for retail_test in RETAIL_TESTS_M17_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M17-B requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M17_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{synthetic_test} did not run: it pins the refusal arms the retail \
                     measurements rest on"
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
         \x20\"task_id\": \"M17-B\",\n\
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
             from the recorded log and production discovery of $CS_GAME_DIR. The suite pins M17's \
             measured control program (objectives.zrd of ZBD/C4/M02/zrdr.zbd: 38 blocks, 135 \
             directive sites, 22 keys, one INSTANTWIN latch, a fully measured vocabulary) through \
             the retail control census, the production control-program binding and M17-A's mission \
             binding, and locates the sheet's three regression priorities in the record: the \
             forced-airframe arm, where no site but the four TRAVELERS subjects spells `player`, \
             the one warp site names the ace's gyro and cs_app::mission_start reads M17's own \
             aiv.zrd for the campaign chain's airframe/player_pfighter; the search-trigger arm, \
             where four always-awake danger-zone blocks pair seven dzpath zones with threshold 1 \
             and the production trigger-volume survey reports no declaration gap against the \
             chapter-4 container's fifteen zone nodes; and the ace lifecycle, where bhatgyro_1 is \
             netted, woken, warped and approached, and nine of the record's 128 texts are \
             declared nowhere else — including WARP_VEHICLE and the two actors bhatbrigand_13 and \
             bhatbrigand_14, which no file in the installation declares. M17's record lowers \
             completely (38 conditions, 135 calls, MissionProgram::validate clean) while the \
             campaign gate stays shut. Claim is implemented only; no mission was played, no \
             original executable was run, nothing is verified_original, and the measured gaps and \
             the unmeasured runtime halves are recorded in \
             docs/findings/2026-10-10-m17-b-compatibility-gaps.md. Validated with \
             tools/validate_evidence.py --require-pass"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M17-B\"",
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
fn parse_m17_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m17_b_")
}
