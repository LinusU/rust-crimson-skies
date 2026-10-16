//! Evidence-report harness for task M01-PLAY-TRIGGERS-TURRET-WIRE: the task's
//! own test list and its `evidence_report_*` harness. Shared helpers, types
//! and imports live in `super` — see `crates/cs_app/tests/campaign/evidence.rs`,
//! which is also where the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests this task's capabilities are judged on: the one
/// leg that composes M01 headlessly and reads the class back out of the live
/// damage graph.
const RETAIL_TESTS: &[&str] =
    &["accept_m01_play_triggers_turret_wire_the_live_graph_classes_the_nine_mounts"];

/// The synthetic legs that must run alongside it, so the report can never be
/// written from the retail leg alone.
const SYNTHETIC_TESTS: &[&str] = &[
    "accept_m01_play_triggers_turret_wire_the_synthetic_stage_classes_exactly_its_declared_part",
    "accept_m01_play_triggers_turret_wire_an_unread_table_is_named_on_the_parts_report",
    "accept_m01_play_triggers_turret_wire_an_uncheckable_installation_is_a_refusal",
    "accept_m01_play_triggers_turret_wire_carriers_without_the_member_are_a_measured_absence",
];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m01_play_triggers_turret_wire_writes_the_acceptance_report() {
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
    let suite = parse_suite_prefixed(&log, "accept_m01_play_triggers_turret_wire_");
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m01_play_triggers_turret_wire_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed: `retail` is declared only
    // because the retail acceptance test above is in this log and passed, and
    // `retail` is this task's required capability.
    for retail_test in RETAIL_TESTS {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M01-PLAY-TRIGGERS-TURRET-WIRE requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{synthetic_test} did not run: it is a CI leg of this task and must be in the \
                     acceptance log beside the retail one"
                )
            });
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    // `source` hashes describe the real installation, measured by the very
    // production discovery this task's scope read is built on.
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
         \x20\"task_id\": \"M01-PLAY-TRIGGERS-TURRET-WIRE\",\n\
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
            "implementer: mimo-1/mimo-1 (Rally #1499); this report was generated by the \
             implementer's own session and is not independent evidence. The reviewing agent's \
             identity and whether its context was fresh are recorded in the Rally \
             complete_review notes. No agent review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives \
             every field from the recorded log and production discovery of $CS_GAME_DIR. The \
             retail leg builds M01's stage through plan_mission_launch + stage_for — which \
             reads the scope's ai.zrd in the mission/group/shared mount order through \
             cs_assets::install::discover + cs_formats discovery + cs_content::stunts::decode_zrd \
             — composes it headlessly with build_headless, and reads each part's class back out \
             of the registered graph in SessionDamageResolver. MEASURED: the carrier the member \
             comes from, the 26 NODES-bearing records, and which nine of M01's 39 chains are \
             declared by them (#1491). DESIGNED: the single-unit pool every part node carries \
             and the Weapon system a mount's destruction disables (F29 AC03). The synthetic legs \
             prove the same wiring without retail: one table that declares one of two parts, and \
             a refused table that is reported by name. This report carries three product-level \
             unresolved issues (the in-play-bit writers, the HEALTH unit and what the original \
             does with a destroyed mount), so it is checked with the plain structural form of \
             tools/validate_evidence.py, which exits 0; the --require-pass form refuses any \
             report whose `unknowns` list is nonempty by design, and those issues are kept here \
             rather than moved out of the machine-readable field to pass it (owner directive of \
             2026-10-01), the same split docs/findings/2026-10-15-m01-play-triggers-anims-arm.md \
             records. Claim is implemented only; no original executable ran and no engine-image \
             trace was made"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M01-PLAY-TRIGGERS-TURRET-WIRE\"",
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
         cs_script::ir::IN_PLAY_BIT_WRITERS_UNTRACED and no engine-image trace was made for this \
         task. Affected content: OBJECTIVE7 and OBJECTIVE28 (block indexes 6 and 27) over the \
         nine mount chains. Resolving: #1360's open item (a trace through $CS_ENGINE_IMAGE) or an \
         original run (#358)."
            .to_owned(),
        "The unit of the TURRET `HEALTH` value (10.0 for the ctur and utur mounts, absent for \
         maingun) is unmeasured, so the part nodes keep the designed single-unit pool and the \
         table's health field is carried but not consumed. Affected content: how many hits any \
         of the nine mounts survives. Resolving: #668 / F27-E."
            .to_owned(),
        "What the **original** does when a mount's node is destroyed — whether its gun really \
         stops, and whether the `WEAPON`/`FIRE_RATE`/`DETECTION_RANGE`/`PITCH`/`YAW` fields the \
         records state drive any firing at all — is unmeasured: this task binds F29's designed \
         AC03 edge so a destroyed mount silences its own firing in this build, and nothing yet \
         fires from a zeppelin mount. Affected content: M01's nine mounts as weapons. Resolving: \
         an engine-image trace or an original run (#358), then the F35/F29 consumer that arms \
         them."
            .to_owned(),
    ]
}
