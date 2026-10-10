//! Evidence-report harness for task M06-B-FU4: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance test M06-B-FU4's `retail` capability is judged on:
/// M06's derived record names the passenger-identity limitation, and M01's —
/// whose startup fires the library crew animation `call_add_jack` — does not.
const RETAIL_TESTS_M06_B_FU4: &[&str] =
    &["accept_m06_b_fu4_m06s_record_names_the_passenger_identity_limitation_and_m01s_does_not"];

/// The synthetic predicate test M06-B-FU4's report must also record: the
/// mission-scoped unknown reaches only the work order it was measured for,
/// over the whole declared inventory, in CI, where there is no original data.
const SYNTHETIC_TESTS_M06_B_FU4: &[&str] =
    &["accept_m06_b_fu4_a_mission_scoped_unknown_reaches_only_the_work_order_it_was_measured_for"];

/// Evidence-report harness for task M06-B-FU4 (Rally #1184), the follow-up
/// that records M06's passenger-identity verdict inside the binding record's
/// own unknowns. It follows the sequence in this module's doc with
/// `M06-B-FU4` and `accept_m06_b_fu4_` in place of `M01-A` and
/// `accept_m01_a_`:
///
/// * the acceptance-log parser selects `accept_m06_b_fu4_` tests, so the
///   recorded assertions are this follow-up's own — the selection must be the
///   prefix's own run (step 1 with that prefix), or the counts would describe
///   a wider run than the assertions;
/// * the artifact beside the report is the **M06** binding, derived from the
///   committed inventory's declared title through production code — the
///   derivation whose new mission-scoped entry this task regenerated
///   `missions/bindings/M06.json` from — so the report cites the record it is
///   evidence for;
/// * the review identity is written by whoever runs the harness, as the
///   module doc requires: this report is the implementer's hand-over run, and
///   the reviewing agent regenerates it on the reviewed commit.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m06_b_fu4_writes_the_acceptance_report() {
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
    let suite = parse_m06_b_fu4_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m06_b_fu4_` tests were recorded in {}",
        log_path.display()
    );
    for (tests, capability) in [
        (RETAIL_TESTS_M06_B_FU4, "retail"),
        (SYNTHETIC_TESTS_M06_B_FU4, "synthetic"),
    ] {
        for task_test in tests {
            let status = suite
                .assertions
                .iter()
                .find(|(name, _)| name == task_test)
                .map(|(_, status)| *status)
                .unwrap_or_else(|| {
                    panic!(
                        "{task_test} did not run: M06-B-FU4 needs {capability}, run step 1 with \
                         `--include-ignored` and CS_GAME_DIR set"
                    )
                });
            assert_eq!(status, "pass", "{task_test} must pass; got status {status}");
        }
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M06");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    assert!(
        source_binding
            .unknowns
            .iter()
            .any(|entry| entry.contains("passenger identity")),
        "the derived M06 record no longer carries the mission-scoped passenger-identity \
         limitation; the report must not be written"
    );
    let binding_path = evidence_dir.join("m06-binding.json");
    fs::write(&binding_path, source_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M06-B-FU4\",\n\
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
            "acceptance suite run locally with the retail capability; the fields are derived \
             from the recorded log, production discovery of $CS_GAME_DIR and the binding \
             `SourceContext::read` + `SourceContext::bind` derive from it (all five critical \
             dependencies resolved; the new mission-scoped entry is carried in \
             missions/bindings/M06.json, regenerated through production code). This task adds \
             the mission-scoped unknown mechanism to cs_content::campaign_bindings: the \
             shared SOURCE_BINDING_UNKNOWNS stays global, and M06's passenger-identity verdict \
             — the shipped data binds no passenger or extraction entity to the mission, \
             measured by M06-B-FU2 — is keyed by work order, so it reaches M06's record alone \
             while M01 (whose startup fires the crew animation call_add_jack, declared by the \
             library's passengers.zrd driving the world node apassengers) receives none. The \
             entry names the limitation, the reason and what settles it: an original reference \
             run under M06-C or the owner's ruling. NOT CLAIMED: nothing is verified_original, \
             no original executable was run, and the global checklist is unchanged for every \
             other mission. Claim is implemented only; validated with \
             tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the \
             commit the suite and this harness ran on; the only later delta is this report's \
             own copy under docs/findings/evidence/, which no acceptance test reads"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M06-B-FU4\"",
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
fn parse_m06_b_fu4_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m06_b_fu4_")
}
