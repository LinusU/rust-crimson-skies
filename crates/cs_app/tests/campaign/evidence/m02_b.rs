//! Evidence-report harness for task M02-B: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M02-B's capabilities are judged on: the
/// control-program binding held to the mission binding's identities, the
/// measured vocabulary partition, the sheet priorities located in the
/// measured graph and the named lowering gap.
const RETAIL_TESTS_M02_B: &[&str] = &[
    "accept_m02_b_m02s_control_program_is_bound_to_the_same_identities_as_its_mission_binding",
    "accept_m02_b_the_measured_vocabulary_partitions_and_refuses_no_m02_key",
    "accept_m02_b_the_objective_graph_the_sheet_priorities_need_is_measured_not_invented",
    "accept_m02_b_fu1_the_kill_sites_lower_through_one_list_argument_and_m02_lowers",
];

/// The synthetic predicate tests M02-B's report must also record: they run in
/// CI without original data and carry the refusal arms the retail
/// installation reaches only through the kill key.
const SYNTHETIC_TESTS_M02_B: &[&str] = &[
    "accept_m02_b_the_control_rule_refuses_an_archive_without_or_with_two_control_members",
    "accept_m02_b_the_vocabulary_partition_is_exact_on_an_authored_record",
    "accept_m02_b_a_site_over_the_host_call_bound_refuses_and_one_at_the_bound_binds",
    "accept_m02_b_fu1_a_long_index_list_binds_as_one_list_argument",
    "accept_m02_b_a_disagreeing_key_keeps_every_shape_and_a_text_follower_is_the_next_key",
];

/// Evidence-report harness for task M02-B, the mission-specific
/// compatibility stage of *The Bomber Heist* (Rally #262). It follows the
/// sequence in this module's doc with `M02-B` and `accept_m02_b_` in place
/// of `M01-A` and `accept_m01_a_`, and differs in three ways from the M02-A
/// report above:
///
/// * the acceptance-log parser selects `accept_m02_b_` tests, so the
///   recorded assertions are this task's own — and the selection also
///   includes `m02_t3.rs`'s suites, which share the prefix (Rally #450);
/// * the artifact beside the report is the **control-program binding**
///   `SourceContext::control_program` derives from the installation — the
///   mission and program identities, the archive and control-member spans
///   and digests, the members the measured rule judged and the directive
///   accounting — ids, ranges, counts and hashes only, never original
///   content;
/// * `CS_EVIDENCE_REVIEWER` fills `review.identity` whole (the runtime
///   identity shape `jstr(&reviewer)` reads), so the report can never carry
///   a hand-over placeholder: the runner supplies the full identity text —
///   the implementing agent's own run names the implementer and says the run
///   is the implementer's evidence and not a review, and the reviewing
///   agent's run names itself.
///
/// The report records what M02-B does *not* claim: no directive is
/// implemented by a measured effect, M02's control record lowers completely
/// only since M02-B-FU1 (#800) — a lowering result, not an implemented
/// effect — no mission was played, no original executable was run and
/// nothing is `verified_original`.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m02_b_writes_the_acceptance_report() {
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
    let suite = parse_m02_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m02_b_` tests were recorded in {}",
        log_path.display()
    );
    for retail_test in RETAIL_TESTS_M02_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M02-B requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M02_B {
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

    // The control-program binding itself, written beside the report and
    // referenced by digest: identities, spans, digests, member accounting and
    // directive counts, never original bytes or display text.
    let toplevel = git(&["rev-parse", "--show-toplevel"]);
    let inventory = CampaignInventory::load(
        &Path::new(&toplevel).join("missions/bindings/campaign-inventory.tsv"),
    )
    .unwrap_or_else(|error| {
        panic!("read missions/bindings/campaign-inventory.tsv from {toplevel}: {error}")
    });
    let title = inventory
        .iter()
        .find(|(work_order, _)| work_order.as_str() == "M02")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M02 work order");
    let context =
        SourceContext::read(&game_dir).expect("production source context reads the installation");
    let control = context
        .control_program(
            MissionLabel::new("M02").expect("M02 is a valid label"),
            &title,
        )
        .expect("M02's control program binds through the measured rule");
    assert_eq!(
        context.install_sha256(),
        install_sha256,
        "the control binding was derived under a different installation fingerprint than discovery reports"
    );
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
    let control_path = evidence_dir.join("m02-control-program.json");
    let control_json = format!(
        "{{\n\
         \x20\"task_id\": \"M02-B\",\n\
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
         \"unclassified_record_keys\": {}, \"refusals\": {}}}\n\
         }}\n",
        jstr(context.install_sha256()),
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
         \x20\"task_id\": \"M02-B\",\n\
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
            "acceptance suite re-run locally with the retail capability; this harness derives \
             every field from the recorded log, production discovery of $CS_GAME_DIR and the \
             control-program binding `SourceContext::control_program` derives from it (mission \
             and program identities equal to the M02-A binding, the control member chosen by the \
             measured rule over the archive's whole member set, the directive accounting of the \
             member it spells). NOT CLAIMED: no directive is implemented by a measured effect; \
             M02's control record lowers completely (its KILL_OBJECTIVE_WHEN_I_COMPLETE index \
             lists are carried as one list argument each, task M02-B-FU1), which is lowering \
             evidence only, not an implemented effect; the record-level \
             sound keys are outside CONTROL_RECORD_KEY_VOCABULARY — M02-B-FU2 (#801) and \
             RECORD-OBJECTIVES-SOUND (#808) measured and admitted all seven to \
             CONTROL_RECORD_SOUND_KEY_VOCABULARY with their consumers; no mission was played, no \
             original \
             executable was run and nothing is verified_original. Claim is implemented only; \
             validated with tools/validate_evidence.py --require-pass. `candidate_tree` is the \
             tree of the commit the suite and this harness ran on; the only later delta is this \
             report's own copy under docs/findings/evidence/ and the findings document that \
             discusses it, neither of which the acceptance suite reads"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M02-B\"",
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

/// [`parse_suite_prefixed`] with this task's test prefix. The prefix is
/// shared with `m02_t3.rs` (Rally #450), so the selection legitimately
/// records both suites; the constants above name this task's own tests.
fn parse_m02_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m02_b_")
}
