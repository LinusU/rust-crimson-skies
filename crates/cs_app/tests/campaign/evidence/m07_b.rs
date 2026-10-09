//! Evidence-report harness for task M07-B: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M07-B's capabilities are judged on.
const RETAIL_TESTS_M07_B: &[&str] = &[
    "accept_m07_b_the_control_program_is_the_member_that_declares_the_blocks",
    "accept_m07_b_the_vocabulary_is_fully_disposed_and_no_m07_key_is_refused",
    "accept_m07_b_the_record_does_not_lower_and_the_gaps_are_the_named_ones",
    "accept_m07_b_the_sheet_priorities_resolve_to_measured_operations",
    "accept_m07_b_the_objective_graph_is_closed_and_the_terminal_blocks_are_gated",
    "accept_m07_b_the_mission_stays_unready_and_the_campaign_gate_stays_closed",
];

/// The synthetic predicate tests M07-B's report must also record.
const SYNTHETIC_TESTS_M07_B: &[&str] = &[
    "accept_m07_b_a_multi_record_anim_state_refuses_its_condition_and_a_bound_shape_refuses_the_key",
    "accept_m07_b_a_danger_zones_site_refuses_its_condition_and_the_block_without_it_lowers",
];

/// Evidence-report harness for task M07-B: M07's mission-specific
/// compatibility gaps. Same sequence as the M02-B report: the acceptance log
/// is parsed for this task's own tests, the control-program binding is
/// re-derived through production code and written beside the report, and the
/// review identity is read at run time from `CS_EVIDENCE_REVIEWER`, so the
/// implementing agent writes its own and the reviewing agent regenerates the
/// report with theirs on the rebased commit.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m07_b_writes_the_acceptance_report() {
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
    let suite = parse_m07_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m07_b_` tests were recorded in {}",
        log_path.display()
    );
    for retail_test in RETAIL_TESTS_M07_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M07-B requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M07_B {
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
        .find(|(work_order, _)| work_order.as_str() == "M07")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M07 work order");
    let context =
        SourceContext::read(&game_dir).expect("production source context reads the installation");
    let control = context
        .control_program(
            MissionLabel::new("M07").expect("M07 is a valid label"),
            &title,
        )
        .expect("M07's control program binds through the measured rule");
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
    let control_path = evidence_dir.join("m07-control-program.json");
    let control_json = format!(
        "{{\n\
         \x20\"task_id\": \"M07-B\",\n\
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
         \"unclassified_record_keys\": [{}], \"refusals\": {}}}\n\
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
         \x20\"task_id\": \"M07-B\",\n\
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
            "acceptance suite run locally with the retail capability; this harness derives \
             every field from the recorded log, production discovery of $CS_GAME_DIR and the \
             control-program binding `SourceContext::control_program` derives from it (mission \
             and program identities equal to the M07-A binding, the control member chosen by \
             the measured rule over the archive's whole member set, the directive accounting \
             of the member it spells). NOT CLAIMED: no directive is implemented by a measured \
             effect; M07's control record does not lower — eight conditions (five multi-record \
             ANIM_STATE sites and three DANGER_ZONES_COMPLETED sites) and all nine ANIM_STATE \
             call sites refuse, which the suite pins — so M07 stays Unsupported and the campaign \
             gate stays closed; the runtime halves of the sheet's priorities (moving pickup, \
             forced plane swap, persistent input ownership) need ordinary-play observation \
             (M07-C); no mission was played, no original executable was run and nothing is \
             verified_original. Claim is implemented only; validated with \
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
        "\"task_id\": \"M07-B\"",
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
fn parse_m07_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m07_b_")
}
