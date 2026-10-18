//! Evidence-report harness for task M13-B-FU2: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M13-B-FU2's `retail` capability is judged on:
/// the archive's startup table is the two names, and production binds both of
/// them to one declaring member and one carrier record each.
const RETAIL_TESTS_M13_B_FU2: &[&str] = &[
    "accept_m13_b_fu2_retail_startup_table_and_empty_mis_anim_member",
    "accept_m13_b_fu2_retail_each_startup_name_resolves_or_is_refused_by_name",
];

/// The synthetic tests M13-B-FU2's report must also record: the join keeps
/// single, ambiguous and unresolved apart, and the limitation measured under
/// the M13 label reaches the archive it was measured on.
const SYNTHETIC_TESTS_M13_B_FU2: &[&str] = &[
    "accept_m13_b_fu2_synthetic_binding_keeps_single_ambiguous_and_unresolved_apart",
    "accept_m13_b_fu2_the_ambiguous_startup_name_is_an_unknown_of_m03_alone",
];

/// Evidence-report harness for task M13-B-FU2 (Rally #1244): where each of
/// M13's startup animation names resolves. It follows the sequence in this
/// module's doc with `M13-B-FU2` and `accept_m13_b_fu2_` in place of `M01-A`
/// and `accept_m01_a_`; the only artifact is the acceptance log, and M13's
/// corrected archive leaves no unknown. The review identity is written by
/// whoever runs the harness.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m13_b_fu2_writes_the_acceptance_report() {
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
    let suite = parse_m13_b_fu2_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m13_b_fu2_` tests were recorded in {}",
        log_path.display()
    );
    for (tests, capability) in [
        (RETAIL_TESTS_M13_B_FU2, "retail"),
        (SYNTHETIC_TESTS_M13_B_FU2, "synthetic"),
    ] {
        for task_test in tests {
            let status = suite
                .assertions
                .iter()
                .find(|(name, _)| name == task_test)
                .map(|(_, status)| *status)
                .unwrap_or_else(|| {
                    panic!(
                        "{task_test} did not run: M13-B-FU2 needs {capability}, run step 1 with \
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

    let artifacts = vec![artifact(&log_path, "log", &evidence_dir)];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M13-B-FU2\",\n\
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
            "acceptance suite run locally with the retail capability; the fields are derived from \
             the recorded log and production discovery of $CS_GAME_DIR. The suite pins this \
             task's verdict on M13's corrected archive: `bind_mission_animation` over \
             `zbd/c2/m03` binds all three of M13's startup rows (`player_setup` and \
             `pzep_engines_start` on NEW_GAME_START, `player_setup` again on LOAD_GAME_START) to \
             exactly one declaring member and one carrier record each, and refuses none. The \
             archive's own `mis_anim.zrd` declares no definition and only names authoring files. \
             The ambiguous `cg1zep_engines_stop` this task first measured belongs to \
             `ZBD/C3/M03/zrdr.zbd`, which the corrected campaign layout binds to work order M03; \
             its mission-scoped unknown now sits in M03's record (M01-ARCHIVE-DECISION) and is \
             pinned there, so this report records no unknown for M13. NOT CLAIMED: any runtime \
             half; nothing is verified_original and no original executable was run. Claim is \
             implemented only; validated with tools/validate_evidence.py --require-pass. \
             `candidate_tree` is the tree of the commit the suite and this harness ran on; the \
             only later delta is this report's own copy under docs/findings/evidence/, which no \
             acceptance test reads"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M13-B-FU2\"",
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
fn parse_m13_b_fu2_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m13_b_fu2_")
}
