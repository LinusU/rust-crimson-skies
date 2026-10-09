//! Evidence-report harness for task M08-B: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M08-B's capabilities are judged on.
const RETAIL_TESTS_M08_B: &[&str] = &[
    "accept_m08_b_m08s_control_program_is_bound_to_the_same_identities_as_its_mission_binding",
    "accept_m08_b_the_measured_vocabulary_partitions_and_refuses_no_m08_key",
    "accept_m08_b_the_block_graph_is_closed_under_the_records_own_numbering",
    "accept_m08_b_the_danger_zones_condition_is_the_gap_that_keeps_m08_unlowered",
    "accept_m08_b_the_three_sheet_priorities_locate_in_the_measured_record",
];

/// The synthetic predicate tests M08-B's report must also record: they carry
/// the two mechanisms the retail record leans on — the list-argument
/// lowering of a long kill index list, and the refused danger-zones
/// condition — into CI, where there is no original data.
const SYNTHETIC_TESTS_M08_B: &[&str] = &[
    "accept_m08_b_m08s_kill_shapes_bind_as_one_list_argument_and_an_over_wide_one_refuses",
    "accept_m08_b_the_danger_zones_condition_refuses_while_a_measured_condition_lowers",
];

/// Evidence-report harness for task M08-B, *The Petrol Plot*'s
/// mission-specific compatibility surface. Same sequence as the M02-B
/// report: it records the `accept_m08_b_` tests, writes M08's control-program
/// binding and both lowering gaps beside the report as a second production
/// observation, and claims `implemented` only.
///
/// The review identity is read at run time from `CS_EVIDENCE_REVIEWER` (the
/// M02-B reading), so the implementer and the reviewer that really ran write
/// their own; a literal is never typed in here.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m08_b_writes_the_acceptance_report() {
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
    let suite = parse_m08_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m08_b_` tests were recorded in {}",
        log_path.display()
    );
    for retail_test in RETAIL_TESTS_M08_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M08-B requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M08_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins a gap arm the retail record rests on")
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

    // The control-program binding and its lowering, written beside the report
    // and referenced by digest: identities, spans, digests, member accounting,
    // directive counts and the two gap armors — never original bytes or
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
        .find(|(work_order, _)| work_order.as_str() == "M08")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M08 work order");
    let context =
        SourceContext::read(&game_dir).expect("production source context reads the installation");
    let control = context
        .control_program(
            MissionLabel::new("M08").expect("M08 is a valid label"),
            &title,
        )
        .expect("M08's control program binds through the measured rule");
    assert_eq!(
        context.install_sha256(),
        install_sha256,
        "the control binding was derived under a different installation fingerprint than discovery reports"
    );

    let census = cs_app::mission_control::survey_mission_control_programs(&game_dir)
        .expect("the census measures the installation");
    let row = census
        .row("zbd/c2/m03")
        .expect("M08's reader archive is measured by the census");
    let lowered = row
        .lowering_attempt()
        .expect("the census lowers M08's measured record");
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
    let control_path = evidence_dir.join("m08-control-program.json");
    let control_json = format!(
        "{{\n\
         \x20\"task_id\": \"M08-B\",\n\
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
         \x20\"task_id\": \"M08-B\",\n\
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
             from the recorded log, production discovery of $CS_GAME_DIR and the control-program \
             binding `SourceContext::control_program` derives from it (mission and program \
             identities equal to the M08-A binding, the control member chosen by the measured \
             rule over the archive's whole member set, the directive accounting of the member it \
             spells). NOT CLAIMED: M08's control record does not lower completely — every one of \
             its 209 host calls binds (the list-argument lowering #800 landed while this stage \
             was in flight), but eight DANGER_ZONES_COMPLETED completion conditions are refused \
             because this build lowers no predicate for them (#813) — so M08 stays Unsupported \
             and the campaign gate stays closed; \
             the wrong-actor, wrong-session and repeated-event halves of the sheet's three \
             priorities are runtime observations and stay unmeasured (M08-C); no mission was \
             played, no original executable was run and nothing is verified_original. Claim is \
             implemented only; validated with tools/validate_evidence.py --require-pass. \
             `candidate_tree` is the tree of the commit the suite and this harness ran on; the \
             only later delta is this report's own copy under docs/findings/evidence/ and the \
             findings document that discusses it, neither of which the acceptance suite reads"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M08-B\"",
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
fn parse_m08_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m08_b_")
}
