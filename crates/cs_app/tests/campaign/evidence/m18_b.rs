//! Evidence-report harness for task M18-B: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M18-B's capabilities are judged on.
const RETAIL_TESTS_M18_B: &[&str] = &[
    "accept_m18_b_the_control_program_is_the_member_that_declares_the_blocks",
    "accept_m18_b_every_directive_m18_spells_has_a_disposition_and_none_is_refused",
    "accept_m18_b_every_call_binds_every_condition_lowers_and_m18s_record_completes",
    "accept_m18_b_release_dependencies_are_two_gates_that_lower_as_wake_conjuncts",
    "accept_m18_b_alternative_action_order_is_two_observers_of_one_release_and_free_per_clamp_blocks",
    "accept_m18_b_rescue_interaction_is_the_approach_site_and_the_per_clamp_pairs",
    "accept_m18_b_the_terminal_blocks_are_gated_and_every_address_is_in_range",
    "accept_m18_b_the_two_outcomes_have_disjoint_prerequisites_and_the_rest_is_not_mandatory",
    "accept_m18_b_every_text_the_record_spells_is_declared_outside_the_control_member_but_six",
    "accept_m18_b_the_sound_group_no_shipped_record_declares_is_the_one_blocks_12_and_50_stop",
    "accept_m18_b_the_production_animation_binding_reads_m18s_own_scope",
    "accept_m18_b_m18_is_a_complete_census_row_and_the_campaign_stays_unready",
];

/// The synthetic predicate tests M18-B's report must also record.
const SYNTHETIC_TESTS_M18_B: &[&str] = &[
    "accept_m18_b_m18s_travelers_site_lowers_and_a_counting_mode_refuses",
    "accept_m18_b_a_dependency_gate_lowers_as_the_dependency_wake_conjunct",
    "accept_m18_b_an_ungrounded_allegiance_directive_is_refused_rather_than_honoured",
];

/// Evidence-report harness for task M18-B: M18's mission-specific
/// compatibility gaps. Same sequence as the M13-B report; it records the
/// `accept_m18_b_` tests, the fully lowering record they pin and the one
/// measured gap they record (`snd_c4-RM-m3_BlackSwan_27`), and claims
/// `implemented` only.
///
/// `CS_EVIDENCE_REVIEWER` fills `review.identity` whole, so whoever runs the
/// harness — the implementing agent at hand-over or the reviewing agent on the
/// rebased commit — writes its own identities and says whether the run is a
/// review. A placeholder identity is a failure.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m18_b_writes_the_acceptance_report() {
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
    let suite = parse_m18_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m18_b_` tests were recorded in {}",
        log_path.display()
    );
    for retail_test in RETAIL_TESTS_M18_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M18-B requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M18_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{synthetic_test} did not run: it pins the refusal arms the retail record \
                     leans on"
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
         \x20\"task_id\": \"M18-B\",\n\
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
             from the recorded log and production discovery of $CS_GAME_DIR. The suite pins M18's \
             measured control program (objectives.zrd, the eighth of seventeen members of \
             ZBD/C4/M03/zrdr.zbd: 52 blocks, 238 directive sites, 29 keys, a fully measured \
             vocabulary) through the retail control census, the production control-program \
             binding and M18-A's mission binding, and locates the sheet's three regression \
             priorities in the record: alternative action order as the seven blocks awake at \
             mission start and the two observers (blocks 4 and 46) of one five-clamp release \
             that lower to the same InactiveMembers condition beside the five free per-clamp \
             blocks 26-30; release dependencies as the two TICK_DEPENDS_ON_OBJ gates (24 on 23, \
             45 on 44) that lower into the gated block's ObjectiveAwake conjunct carrying the \
             dependency's zero-based index; rescue interaction as the single TRAVELERS approach \
             site (player approaching cargozep1 at 1500) with the per-clamp INACTIVE/REMOVE \
             pairs and the measured absence of any docking, pickup, boarding or transfer key. \
             Both terminal blocks (11 INSTANTLOSS, 25 INSTANTWIN) are dormant with no timed \
             wake, all 65 spelled addresses lie in 1..=52, and the two prerequisite closures \
             are disjoint with 24 blocks named by neither. M18's lowering is complete: every \
             call binds, every condition lowers, MissionProgram::validate accepts and M18's \
             census row is complete while the campaign stays unready; the production animation \
             binding reads the scope's ten startup rows (7 playable, 3 refused with reasons). \
             One measured gap is recorded and not worked around: the sound group \
             snd_c4-RM-m3_BlackSwan_27 that blocks 12 and 50 stop is declared by no shipped \
             record and its bytes occur in exactly one file of the installation, while the \
             five MSG_BRF_RMM3_* operands are carried by strings.dll. Claim is implemented \
             only; no mission was played, no original executable was run, nothing is \
             verified_original, and the measured gap and the unmeasured runtime halves are \
             recorded in docs/findings/2026-10-10-m18-b-compatibility-gaps.md. Validated with \
             tools/validate_evidence.py --require-pass"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M18-B\"",
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
fn parse_m18_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m18_b_")
}
