//! Evidence-report harness for task F50-C: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests F50-C's capabilities are judged on: the probe
/// routes over the bound campaign, the AC03 reentry pass and the human
/// playtest route document pinned to the plan.
const RETAIL_TESTS_F50_C: &[&str] = &[
    "accept_f50_c_the_whole_campaign_has_one_probe_route_per_declared_work_order",
    "accept_f50_c_every_ready_route_reenters_the_same_mission_after_each_ac03_interruption",
    "accept_f50_c_the_playtest_route_document_lists_every_work_order_as_planned",
];

/// The synthetic planning-rule tests F50-C's report must also record: they
/// run in CI without original data and carry the refusal arms the retail
/// installation never reaches.
const SYNTHETIC_TESTS_F50_C: &[&str] = &[
    "accept_f50_c_the_minimum_scenario_is_exactly_the_five_ac03_interruptions",
    "accept_f50_c_an_unresolved_identity_is_a_refused_route_that_stays_in_the_plan",
    "accept_f50_c_a_campaign_read_under_two_fingerprints_is_refused",
    "accept_f50_c_a_declared_work_order_with_no_source_is_refused",
    "accept_f50_c_a_source_for_an_undeclared_work_order_is_refused",
    "accept_f50_c_a_fingerprint_that_is_not_canonical_hex_is_refused",
];

/// Evidence-report harness for task F50-C, the per-mission probe-route and
/// human-playtest-route stage. It follows the sequence in this module's doc
/// with `F50-C` and `accept_f50_c_` in place of `M01-A` and `accept_m01_a_`,
/// and differs in three ways from the F50-B report above:
///
/// * the acceptance-log parser selects `accept_f50_c_` tests, so the recorded
///   assertions are this task's own — three retail ones over the installation
///   and six synthetic ones over the planning rule;
/// * the artifact beside the report is the **probe plan** derived from the
///   installation by `SourceContext::bind_campaign` +
///   `cs_content::campaign_bindings::probe_routes`: one entry per declared
///   work order with its campaign position, its ready/refused state, the
///   mission identity its five reentries land on (or its refusal), the one
///   fingerprint every route is anchored to and the digest of the pinned
///   human route document `missions/bindings/playtest-routes.md` — ids and
///   counts only, never original content;
/// * the report records what F50-C does *not* claim: no mission was played,
///   no runtime death, bailout or retry was observed (the mission launch path
///   is `VS-M01-RUNTIME`, the controlled runs are `VS-M01-CONTROLLED-RUNS`),
///   the campaign progression is unmeasured so `ready` stays false, and the
///   seven work orders whose title the installation carries in neither
///   display form are refused routes listed with their reason rather than
///   dropped.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_f50_c_writes_the_acceptance_report() {
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

    // The candidate tree must be the tree that was actually tested.
    let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        candidate_tree, head_tree,
        "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
         old reports cannot be reused for new code"
    );

    // The acceptance suite is the evidence: parse its recorded output.
    let log_path = evidence_dir.join("cargo-test.log");
    let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
        panic!(
            "cannot read the acceptance log {}: {error} (step 1 must tee its output there)",
            log_path.display()
        )
    });
    let suite = parse_f50_c_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_f50_c_` tests were recorded in {}",
        log_path.display()
    );
    for retail_test in RETAIL_TESTS_F50_C {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: F50-C requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_F50_C {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{synthetic_test} did not run; the synthetic planning rule is part of this task"
                )
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    // `source` hashes describe the real installation, measured by the very
    // production discovery this task's plan is built on.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The derived probe plan itself, written beside the report and referenced
    // by digest: ids, positions, states and refusals only, never original
    // content.
    let toplevel = git(&["rev-parse", "--show-toplevel"]);
    let toplevel = Path::new(&toplevel);
    let inventory =
        CampaignInventory::load(&toplevel.join("missions/bindings/campaign-inventory.tsv"))
            .unwrap_or_else(|error| {
                panic!(
                    "read {}: {error}",
                    toplevel
                        .join("missions/bindings/campaign-inventory.tsv")
                        .display()
                )
            });
    let context = SourceContext::read(&game_dir)
        .expect("production source context reads the original installation");
    let bound = context
        .bind_campaign(&inventory)
        .expect("the declared campaign binds to the original data");
    let plan = probe_routes(&bound).expect("the bound campaign plans its probe routes");
    assert_eq!(
        plan.len(),
        inventory.len(),
        "the plan cites one route per declared work order"
    );
    assert_eq!(
        context.install_sha256(),
        install_sha256,
        "the plan was anchored under a different installation fingerprint than discovery reports"
    );
    let doc_path = toplevel.join("missions/bindings/playtest-routes.md");
    let doc_sha256 = sha256(
        &fs::read(&doc_path).unwrap_or_else(|error| panic!("read {}: {error}", doc_path.display())),
    )
    .to_hex();

    let identity = |value: &Option<ContentId>| match value {
        Some(value) => jstr(value.as_str()),
        None => "null".to_owned(),
    };
    let routes: Vec<String> = plan
        .routes()
        .iter()
        .map(|route| {
            let reentry = route.reentry(ProbeInterruption::Death);
            format!(
                "{{\"work_order\": {}, \"campaign_position\": {}, \"state\": {}, \"mission\": {}, \
                 \"world\": {}, \"program\": {}, \"refusal\": {}}}",
                jstr(route.label.as_str()),
                match route.campaign_position {
                    Some(position) => position.to_string(),
                    None => "null".to_owned(),
                },
                jstr(if route.is_ready() { "ready" } else { "refused" }),
                identity(&reentry.map(|reentry| reentry.mission.clone())),
                identity(&reentry.map(|reentry| reentry.world.clone())),
                identity(&reentry.map(|reentry| reentry.program.clone())),
                match &route.refusal {
                    Some(refusal) => jstr(refusal),
                    None => "null".to_owned(),
                },
            )
        })
        .collect();
    let refused_labels: Vec<&str> = plan.refused().map(|route| route.label.as_str()).collect();
    let plan_json = format!(
        "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"declared\": {}, \"routes\": {}, \
         \"ready\": {}, \"refused\": {}, \"reentries\": {}, \"interruptions\": [{}], \
         \"playtest_routes_doc_sha256\": {}, \"work_orders\": [{}]}}\n",
        jstr(&install_sha256),
        jstr(&candidate_tree),
        inventory.len(),
        plan.len(),
        plan.ready_count(),
        plan.refused_count(),
        plan.ready_count() * ProbeInterruption::ALL.len(),
        ProbeInterruption::ALL
            .iter()
            .map(|interruption| jstr(interruption.label()))
            .collect::<Vec<_>>()
            .join(", "),
        jstr(&doc_sha256),
        routes.join(", "),
    );
    let plan_path = evidence_dir.join("probe-routes.json");
    fs::write(&plan_path, &plan_json)
        .unwrap_or_else(|error| panic!("write {}: {error}", plan_path.display()));
    assert!(
        !refused_labels.is_empty() && plan.ready_count() > 0,
        "the plan must be cited exactly as it stands: some routes ready, the uncarried titles \
         refused by name ({})",
        refused_labels.join(", ")
    );

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&plan_path, "json", &evidence_dir),
    ];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F50-C\",\n\
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
        f50_c_assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&reviewer),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR and a second \
             production run of `SourceContext::read` + `SourceContext::bind_campaign` + \
             `probe_routes` over the committed denominator (probe-routes.json): 24 declared work \
             orders, one probe route each, every route anchored to the one fingerprint discovery \
             reports, the five AC03 interruptions (death, bailout, skip-media, save/restart, \
             settings change) each planned with a reentry onto the same mission identity, and \
             `missions/bindings/playtest-routes.md` digested as the pinned human route document. \
             The report cites what that call actually produced, including the refused routes by \
             name, because F50-C plans the retry contract and never filters to the working \
             subset. NOT CLAIMED: no mission was played, no runtime death, bailout or retry was \
             observed (executing a route is VS-M01-RUNTIME and VS-M01-CONTROLLED-RUNS), the \
             campaign progression is unmeasured so readiness is false, the seven uncarried titles \
             are not resolved to retail missions here, no original executable was run and nothing \
             is verified_original; ordinary play is F50-D. Claim is implemented only; validated \
             with tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the \
             commit the suite and this harness ran on; the only later delta is this report's own \
             copy under docs/findings/evidence/ and the findings document that discusses it, \
             neither of which the acceptance suite reads"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F50-C\"",
        "\"claim\": \"implemented\"",
        "\"capabilities\": [\"retail\", \"synthetic\"]",
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

/// The assertion list of this task's report. Every recorded test is cited
/// against both artifacts: the acceptance log says the test ran and passed,
/// and `probe-routes.json` is the production reading of the same
/// installation those tests were held to.
fn f50_c_assertion_array(assertions: &[(String, &'static str)]) -> String {
    let items: Vec<String> = assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\", \
                 \"probe-routes.json\"]}}",
                jstr(name)
            )
        })
        .collect();
    items.join(", ")
}

/// [`parse_suite_prefixed`] with this task's test prefix.
fn parse_f50_c_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_f50_c_")
}
