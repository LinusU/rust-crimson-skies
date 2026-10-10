//! Evidence-report harness for task M24-B: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M24-B's `retail` capability is judged on.
const RETAIL_TESTS_M24_B: &[&str] = &[
    "accept_m24_b_m24s_control_program_is_bound_to_the_same_identities_as_its_mission_binding",
    "accept_m24_b_the_measured_vocabulary_partitions_and_refuses_no_m24_key",
    "accept_m24_b_the_block_graph_is_closed_under_the_records_own_numbering",
    "accept_m24_b_the_capital_battle_transition_is_the_zeppelin_chain_the_record_spells",
    "accept_m24_b_the_target_eligibility_changes_are_five_writes_and_the_record_never_adds_a_target",
    "accept_m24_b_the_campaign_ending_is_one_win_and_one_loss_latch_with_measured_arming_chains",
    "accept_m24_b_m24s_record_does_not_lower_and_its_calls_refuse_by_two_named_gaps",
    "accept_m24_b_m24_stays_unready_while_its_sound_cleanup_and_truncated_key_refuse",
];

/// The engine-image acceptance test M24-B's static code reading is judged
/// on: the truncated `SET_AI_` spelling must be absent from the
/// directive-key string table the original parser looks names up in, read
/// back out of `$CS_ENGINE_IMAGE`. The image is the owner-supplied decrypted
/// executable, read-only and never committed; the test is
/// `#[ignore = "requires CS_ENGINE_IMAGE"]` like the retail members are
/// `#[ignore = "requires CS_GAME_DIR"]`, so step 1 needs both set.
const IMAGE_TESTS_M24_B: &[&str] =
    &["accept_m24_b_the_truncated_set_ai_key_is_not_in_the_measured_directive_table"];

/// The synthetic predicate tests M24-B's report must also record: they carry
/// the two refusal mechanisms the retail record leans on — one oversized
/// `STOP_QUEUED_SOUNDS` signature unregistering the whole key, and the
/// truncated net key refusing as an unknown call — into CI, where there is
/// no original data.
const SYNTHETIC_TESTS_M24_B: &[&str] = &[
    "accept_m24_b_one_oversized_sound_site_poisons_the_whole_sound_key",
    "accept_m24_b_the_truncated_set_ai_site_refuses_but_the_measured_net_site_binds",
];

/// Evidence-report harness for task M24-B, the campaign-ending mission's
/// mission-specific compatibility surface. Same sequence as the M16-B
/// report: it records the `accept_m24_b_` tests, writes M24's
/// control-program binding and its lowering gaps beside the report as a
/// second production observation, and claims `implemented` only.
///
/// The review identity is read at run time from `CS_EVIDENCE_REVIEWER` (the
/// M02-B reading), so the implementer and the reviewer that really ran write
/// their own; a literal is never typed in here.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR, CS_ENGINE_IMAGE"]
fn evidence_report_m24_b_writes_the_acceptance_report() {
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
    assert!(
        !reviewer.trim().is_empty(),
        "CS_EVIDENCE_REVIEWER must name the reviewer that really ran"
    );
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
    let suite = parse_m24_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m24_b_` tests were recorded in {}",
        log_path.display()
    );
    for (tests, capability) in [
        (RETAIL_TESTS_M24_B, "retail"),
        (IMAGE_TESTS_M24_B, "the owner-supplied engine image"),
        (SYNTHETIC_TESTS_M24_B, "synthetic"),
    ] {
        for task_test in tests {
            let status = suite
                .assertions
                .iter()
                .find(|(name, _)| name == task_test)
                .map(|(_, status)| *status)
                .unwrap_or_else(|| {
                    panic!(
                        "{task_test} did not run: M24-B needs {capability}, run step 1 with \
                         `--include-ignored` and CS_GAME_DIR / CS_ENGINE_IMAGE set"
                    )
                });
            assert_eq!(status, "pass", "{task_test} must pass; got status {status}");
        }
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The control-program binding and its lowering, written beside the report
    // and referenced by digest: identities, spans, digests, member accounting,
    // directive counts and the refused calls — never original bytes or
    // display text.
    let toplevel = git(&["rev-parse", "--show-toplevel"]);
    let inventory = CampaignInventory::load(
        &Path::new(&toplevel).join("missions/bindings/campaign-inventory.tsv"),
    )
    .unwrap_or_else(|error| {
        panic!("read missions/bindings/campaign-inventory.tsv from {toplevel}: {error}")
    });
    let title = inventory
        .iter()
        .find(|(work_order, _)| work_order.as_str() == "M24")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M24 work order");
    let context =
        SourceContext::read(&game_dir).expect("production source context reads the installation");
    let control = context
        .control_program(
            MissionLabel::new("M24").expect("M24 is a valid label"),
            &title,
        )
        .expect("M24's control program binds through the measured rule");
    assert_eq!(
        context.install_sha256(),
        install_sha256,
        "the control binding was derived under a different installation fingerprint than discovery reports"
    );

    let census = cs_app::mission_control::survey_mission_control_programs(&game_dir)
        .expect("the census measures the installation");
    let row = census
        .row("zbd/c5/m04")
        .expect("M24's reader archive is measured by the census");
    let lowered = row
        .lowering_attempt()
        .expect("the census lowers M24's measured record");
    let attempt = lowered.attempt();
    let refused_calls = attempt
        .calls
        .iter()
        .filter(|outcome| {
            matches!(
                outcome,
                cs_content::mission_control::CallOutcome::Refused(_)
            )
        })
        .count();
    let refused_conditions = attempt
        .conditions
        .iter()
        .filter(|outcome| {
            matches!(
                outcome,
                cs_content::mission_control::ConditionOutcome::Refused(_)
            )
        })
        .count();
    let lowering = control.record.lowering(attempt);
    let unmet: Vec<String> = lowering
        .unmet()
        .map(|row| row.kind.code().to_owned())
        .collect();

    let members: Vec<String> = control
        .members
        .iter()
        .map(|row| {
            format!(
                "{{\"name\": {}, \"offset\": {}, \"length\": {}, \"objective_blocks\": {}}}",
                jstr(&row.name),
                row.offset,
                row.len,
                row.objective_blocks
            )
        })
        .collect();
    let implemented: Vec<String> = control
        .record
        .implemented()
        .into_iter()
        .map(|(key, outcome)| {
            format!(
                "{{\"key\": {}, \"outcome\": {}}}",
                jstr(&key.key),
                jstr(outcome.label())
            )
        })
        .collect();
    let control_path = evidence_dir.join("m24-control-program.json");
    let control_json = format!(
        "{{\n\
         \x20\"task_id\": \"M24-B\",\n\
         \x20\"install_sha256\": {},\n\
         \x20\"mission\": {},\n\
         \x20\"program_id\": {},\n\
         \x20\"program_asset\": {},\n\
         \x20\"program_length\": {},\n\
         \x20\"program_sha256\": {},\n\
         \x20\"control_member\": {},\n\
         \x20\"control_offset\": {},\n\
         \x20\"control_length\": {},\n\
         \x20\"control_sha256\": {},\n\
         \x20\"members\": [{}],\n\
         \x20\"record\": {{\"blocks\": {}, \"sites\": {}, \"vocabulary\": {}, \
         \"implemented\": [{}], \"measured\": {}, \"unmeasured\": [{}], \
         \"unclassified_record_keys\": [{}], \"refusals\": {}}},\n\
         \x20\"lowering\": {{\"mission\": {}, \"objectives\": {}, \"calls\": {}, \
         \"refused_calls\": {}, \"refused_conditions\": {}, \"unbound_keys\": [{}], \
         \"unmet\": [{}], \"validation_present\": {}, \"program_present\": {}}}\n\
         }}\n",
        jstr(install_sha256.as_str()),
        jstr(control.mission.as_str()),
        jstr(control.program_id.as_str()),
        jstr(&control.program_asset),
        control.program_length,
        jstr(&control.program_sha256),
        jstr(&control.control_member),
        control.control_offset,
        control.control_length,
        jstr(&control.control_sha256),
        members.join(", "),
        control.record.blocks(),
        control.record.sites(),
        control.record.vocabulary(),
        implemented.join(", "),
        control.record.measured().len(),
        str_array(&control.unmeasured_keys()),
        str_array(control.unclassified_record_keys()),
        control.record.refusals().len(),
        match &attempt.mission {
            Ok(mission) => jstr(mission),
            Err(reason) => jstr(reason),
        },
        attempt.objectives,
        attempt.calls.len(),
        refused_calls,
        refused_conditions,
        str_array(&attempt.unbound_keys),
        str_array(&unmet),
        attempt.validation.is_some(),
        lowered.program().is_some(),
    );
    fs::write(&control_path, control_json)
        .unwrap_or_else(|error| panic!("write {}: {error}", control_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&control_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M24-B\",\n\
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
            "acceptance suite run locally with the retail capability and the owner-supplied \
             engine image; every field is derived from the recorded log, production discovery \
             of $CS_GAME_DIR and the control-program binding `SourceContext::control_program` \
             derives from it (mission and program identities equal to the M24-A binding, the \
             control member chosen by the measured rule over the archive's whole member set, \
             the directive accounting of the member it spells). NOT CLAIMED: M24's control \
             record does not lower — every one of its 63 conditions lowers and 260 of its 267 \
             calls bind, but `STOP_QUEUED_SOUNDS` never registers (its measured shapes include \
             a 9-name list, and a signature past `MAX_CALL_ARGS` makes the whole spec unfit, \
             so all six sites refuse `unknown host call`) and OBJECTIVE19's truncated `SET_AI_` \
             site is an unmeasured key that registers nothing — the engine-image member shows \
             the spelling absent from the directive-key table the original parser looks names \
             up in — so no program assembles, `call_arguments` is unmet, M24 stays Unsupported \
             and the campaign gate stays closed; carrying the name list as one list argument \
             is the follow-up the findings name; the wrong-actor, wrong-session and \
             repeated-event halves of the sheet's three priorities (capital-battle \
             transition, target-eligibility change, campaign ending) are runtime observations \
             and stay unmeasured (M24-C); no mission was played, no original executable was \
             run and nothing is verified_original. Claim is implemented only; validated with \
             tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the \
             commit the suite and this harness ran on; the only later delta is this report's \
             own copy under docs/findings/evidence/ and the findings document that discusses \
             it, neither of which the acceptance suite reads"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M24-B\"",
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

/// [`parse_suite_prefixed`] with this task's own test prefix.
fn parse_m24_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m24_b_")
}
