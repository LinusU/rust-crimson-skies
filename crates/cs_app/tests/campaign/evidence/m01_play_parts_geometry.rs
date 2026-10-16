//! Evidence-report harness for task M01-PLAY-PARTS-GEOMETRY: the task's own
//! test lists and its `evidence_report_*` harness. Shared helpers, types and
//! imports live in `super` — see `crates/cs_app/tests/campaign/evidence.rs`,
//! which is also where the module doc says where a new task's evidence goes.
//!
//! The reviewer identity is **runtime**: it is read from
//! `CS_EVIDENCE_REVIEWER`, so the reviewer's own pass regenerates the report
//! with its real identity rather than a literal written before the review
//! happened.

use super::*;

/// The retail acceptance tests this task's capabilities are judged on.
const RETAIL_TESTS: &[&str] = &[
    "accept_m01_play_parts_geometry_m01_binds_the_measured_allegiance_and_a_labelled_box_on_every_part",
];

/// The synthetic (CI) acceptance tests, which must be present beside them.
const SYNTHETIC_TESTS: &[&str] = &[
    "accept_m01_play_parts_geometry_the_fire_key_destroys_a_bound_part_and_advances_its_ladder",
    "accept_m01_play_parts_geometry_a_world_actor_on_the_players_team_is_refused",
    "accept_m01_play_parts_geometry_an_unread_player_team_declares_no_side",
    "accept_m01_play_parts_geometry_the_allegiance_reading_is_the_measured_team_split",
];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m01_play_parts_geometry_writes_the_acceptance_report() {
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
    let suite = parse_suite_prefixed(&log, "accept_m01_play_parts_geometry_");
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m01_play_parts_geometry_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed: `retail` is declared only
    // because the retail acceptance test above is in this log and passed, and
    // `retail` is this task's required capability.
    for retail_test in RETAIL_TESTS {
        assert_eq!(
            recorded_status(&suite, retail_test),
            "pass",
            "{retail_test} must pass; run step 1 with `--include-ignored` and CS_GAME_DIR set"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS {
        assert_eq!(
            recorded_status(&suite, synthetic_test),
            "pass",
            "{synthetic_test} is this task's CI half and must be in the same run"
        );
    }

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
         \x20\"task_id\": \"M01-PLAY-PARTS-GEOMETRY\",\n\
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
        jstr(&format!(
            "implementer: bunny-alpha-2/bunny-alpha-2 (Rally #1492, DeepSeek V4.1 Flash); \
             reviewer: {reviewer} — the identity is read at run time from CS_EVIDENCE_REVIEWER, \
             so the reviewer's own regeneration names itself. No agent review replaces the \
             owner's human approval"
        )),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives every \
             field from the recorded log and production discovery of $CS_GAME_DIR. The retail leg \
             reads M01's stage through plan_mission_launch + stage_for and the composed mission \
             through build_headless. MEASURED: each part's bind pose (the session's canonical \
             actor pose), each world actor's loader-resolved faction (#1155/#1177) and the player \
             record's own team (#1453). DESIGNED: the part box's half extents, the unmeasured \
             container box it stands in for, and the reading that turns the two measured teams \
             into the player's relation. Claim is implemented only; no original executable ran \
             and no engine-image trace was made"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M01-PLAY-PARTS-GEOMETRY\"",
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
        "The original's per-part swept-box geometry (each part node's authored transform and \
         stored box) has no decoded source: a part node is an `object3d` record of the container \
         hierarchy the world import makes no object for, and no reader in this tree decodes that \
         record's transform or box, so the mission runs on a labelled designed box at the \
         owner's measured bind pose. Affected content: every geometrically destroyable part of \
         M01's two zeppelins; how close a shot must be. Resolving: the container node decode \
         (F18/F35) or a measured original hit / F27-E's measured damage table."
            .to_owned(),
        "The original's per-part integrity (how many hits a part survives) is still unmeasured — \
         the designed single-unit pool #1481 bound is unchanged. Affected content: the pool every \
         part damage node carries. Resolving: #668 (the measured damage table), F27-E."
            .to_owned(),
        "Which gameplay side `ally`/`enemy` place the player on is unmeasured: the loader's team \
         vocabulary is measured (#1155/#1177) and the player record's own team is measured \
         (#1453), but the reading that compares them is designed here — the same team split \
         #1436's role split uses — so it is labelled designed, never a reproduction. A scripted \
         mid-mission relation change (F39) is not modelled. Affected content: every world actor's \
         relation to the player, and in particular M01's `piratezep`, whose measured `ally` \
         faction makes the player's guns refuse it under this reading. Resolving: an original run \
         (#358) or the F30/F34 faction-consumption stages."
            .to_owned(),
        "The in-play bit's producers (+0x24 bit 4) stay untraced (#1360, \
         cs_script::ir::IN_PLAY_BIT_WRITERS_UNTRACED): a part destroyed geometrically is reported \
         out of play because the damage authority says so, not because this tree measured the \
         original clearing the bit. Affected content: every INACTIVE ladder over a part. \
         Resolving: #1360's open item or an original run (#358)."
            .to_owned(),
    ]
}
