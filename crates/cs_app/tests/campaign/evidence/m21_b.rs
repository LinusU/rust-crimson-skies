//! Evidence-report harness for task M21-B: the task's own test lists and its
//! `evidence_report_*` harness. Shared helpers, types and imports live in
//! `super` — see `crates/cs_app/tests/campaign/evidence.rs`, which is also
//! where the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M21-B's capabilities are judged on.
const RETAIL_TESTS_M21_B: &[&str] = &[
    "accept_m21_b_the_control_program_is_the_member_that_declares_the_blocks",
    "accept_m21_b_every_directive_m21_spells_has_a_disposition_and_none_is_refused",
    "accept_m21_b_the_moving_guide_is_the_actor_the_program_starts_and_then_measures",
    "accept_m21_b_the_structural_destruction_is_the_support_beams_the_freighter_and_the_zeppelin_panels",
    "accept_m21_b_the_optional_route_is_the_danger_zone_threshold_and_its_paired_gates",
    "accept_m21_b_every_text_the_record_spells_is_declared_outside_it_or_recorded_as_a_gap",
    "accept_m21_b_the_two_terminal_latches_are_gated_and_every_address_is_in_range",
    "accept_m21_b_the_player_start_is_the_aircraft_tables_own_record",
    "accept_m21_b_every_call_binds_every_condition_lowers_and_m21s_record_completes",
    "accept_m21_b_m21s_row_is_complete_and_the_campaign_stays_unready",
];

/// The synthetic predicate tests M21-B's report must also record: they run in
/// CI without original data and carry the refusal arms.
const SYNTHETIC_TESTS_M21_B: &[&str] = &[
    "accept_m21_b_a_guide_approach_site_binds_at_its_measured_shape_and_refuses_a_radius_the_ir_cannot_carry",
    "accept_m21_b_the_zone_threshold_pairs_with_its_evaluator_and_an_unknown_key_refuses",
];

/// Evidence-report harness for task M21-B: M21's mission-specific
/// compatibility gaps. Same sequence as the M17-B report; it records the
/// `accept_m21_b_` tests, the fully lowering record they pin and the measured
/// gaps they record, and claims `implemented` only.
///
/// `CS_EVIDENCE_REVIEWER` fills `review.identity` whole, so whoever runs the
/// harness — the implementing agent at hand-over or the reviewing agent on the
/// rebased commit — writes its own identities and says whether the run is a
/// review. A placeholder identity is a failure.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m21_b_writes_the_acceptance_report() {
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
    let suite = parse_m21_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m21_b_` tests were recorded in {}",
        log_path.display()
    );
    for retail_test in RETAIL_TESTS_M21_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M21-B requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M21_B {
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
         \x20\"task_id\": \"M21-B\",\n\
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
             from the recorded log and production discovery of $CS_GAME_DIR. The suite pins M21's \
             measured control program (objectives.zrd of ZBD/C5/M01/zrdr.zbd: 63 blocks, 237 \
             directive sites, 28 keys, one INSTANTWIN and one INSTANTLOSS latch, a fully measured \
             vocabulary) through the retail control census, the production control-program \
             binding and M21-A's mission binding, and locates the sheet's three regression \
             priorities in the record: the moving-guide arm, where the six TRAVELERS sites give \
             exactly one non-player subject (autogyro_1, measured against a fixed coordinate with \
             DELETE_ON_SUCCESS) and the program itself starts that actor moving with START_TAXI at \
             block 28; the structural-destruction arm, where six warehouse support beams \
             (MSG_TRGT_WH_SUPPORTBEAM) watch four thresholds, the steinmann freighter's \
             steinmann_sink animation is block 17's predicate, and block 63's sixth chained lookup \
             names gasbag6, which this archive's zeppelins.zrd does not spell (it spells gasbag1 \
             through gasbag5 with gasbag5 twice); and the optional-route arm, where the always-awake \
             block 37's DANGER_ZONES_COMPLETION_COUNT 4 over dzpath22..27 wakes the SECONDARY \
             objective 5 and kills block 6, with every zone name resolved by the production \
             trigger-volume survey against the chapter-5 container's 34 nodes and no declaration \
             gap. The record's 186 texts split 174 declared outside the control member and 12 \
             recorded, of which a one-pass raw scan carries only three (fbgun01, fbgun02, \
             maagun0*) nowhere else. M21's record lowers completely (63 conditions, 237 calls, \
             MissionProgram::validate clean) while the campaign gate stays shut. Claim is \
             implemented only; no mission was played, no original executable was run, nothing is \
             verified_original, and the measured gaps and the unmeasured runtime halves are \
             recorded in docs/findings/2026-10-10-m21-b-compatibility-gaps.md. Validated with \
             tools/validate_evidence.py --require-pass"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M21-B\"",
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
fn parse_m21_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m21_b_")
}
