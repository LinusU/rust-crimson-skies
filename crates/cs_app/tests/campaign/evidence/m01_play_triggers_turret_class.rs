//! Evidence-report harness for task M01-PLAY-TRIGGERS-TURRET-CLASS: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests this task's capabilities are judged on.
const RETAIL_TESTS: &[&str] = &[
    "accept_m01_play_triggers_turret_class_the_nine_chains_are_declared_weapon_mounts",
    "accept_m01_play_triggers_turret_class_exactly_the_nine_of_the_39_chains_are_mounts",
];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m01_play_triggers_turret_class_writes_the_acceptance_report() {
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
    let suite = parse_suite_prefixed(&log, "accept_m01_play_triggers_turret_class_");
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m01_play_triggers_turret_class_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed: `retail` is declared
    // only because every retail acceptance test above is in this log and
    // passed, and `retail` is this task's required capability.
    for retail_test in RETAIL_TESTS {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M01-PLAY-TRIGGERS-TURRET-CLASS requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    assert!(
        suite
            .assertions
            .iter()
            .any(|(name, _)| name == "accept_m01_play_triggers_turret_class_a_synthetic_table_classes_only_what_it_declares"),
        "the synthetic predicate tests must be present alongside the retail ones"
    );

    // `source` hashes describe the real installation, measured by the very
    // production discovery this task's binding is built on.
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
         \x20\"task_id\": \"M01-PLAY-TRIGGERS-TURRET-CLASS\",\n\
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
         \x20\"unknowns\": {},\n\
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
        str_array(&unknowns()),
        jstr(
            "implementer: claude-2/claude-1 (Rally #1491, Sonnet 5.5); reviewer: \
             bunny-alpha-2/bunny-alpha-2 in a fresh session, a different agent instance from \
             the implementer, so the review's context was fresh and independent of the \
             implementer's session. No agent review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives \
             every field from the recorded log and production discovery of $CS_GAME_DIR. The \
             retail legs read ai.zrd's TURRET table through cs_formats discovery and \
             cs_content::stunts::decode_zrd, and M01's stage through plan_mission_launch + \
             stage_for. MEASURED: which TURRET records declare the nine nodes, their weapon, \
             PARTS and HEALTH. DESIGNED: the unit pool every part node carries and the \
             Weapon system a mount's destruction disables (F29 AC03). Claim is implemented \
             only; no original executable ran and no engine-image trace was made"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M01-PLAY-TRIGGERS-TURRET-CLASS\"",
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

/// The unresolved questions the report carries instead of deleting.
fn unknowns() -> Vec<String> {
    vec![
        "Whether destroying a TURRET mount clears the node's in-play bit (+0x24 bit 4) that the \
         INACTIVE ladders count, and whether the original counts a turret's `healthy` state the \
         same way as an engine's, is untraced: the writers of the bit are \
         cs_script::ir::IN_PLAY_BIT_WRITERS_UNTRACED and no engine-image trace was made. \
         Affected content: OBJECTIVE7 and OBJECTIVE28 (block indexes 6 and 27). Resolving: \
         #1360's open item or an original run (#358)."
            .to_owned(),
        "The unit of the TURRET `HEALTH` value (10.0 for the ctur and utur mounts, absent for \
         maingun) is unmeasured, so the part nodes keep the designed single-unit pool. \
         Affected content: how many hits a turret survives. Resolving: #668 / F27-E."
            .to_owned(),
        "The class is bound by `classed_part_node`, which nothing in the composition calls yet: \
         the spawn host (mission_actors.rs) still calls designed_part_node and the stage carries \
         no ai.zrd table. Affected content: every F29 consumer of the nine nodes. Resolving: the \
         runtime wiring follow-up filed by this task."
            .to_owned(),
    ]
}
