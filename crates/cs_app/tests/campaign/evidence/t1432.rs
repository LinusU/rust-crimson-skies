//! Evidence-report harness for task T1432 (`OWNER-FACTS-TABLE`, Rally
//! #1432): the task's own test lists and its `evidence_report_*` harness.
//! Shared helpers, types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where the
//! module doc says where a new task's evidence goes.
//!
//! Run it from the workspace root, exactly as:
//!
//! 1. ```sh
//!    cargo test --workspace --locked -- accept_owner_facts_table_ --include-ignored \
//!      2>&1 | tee private/evidence/T1432/cargo-test.log
//!    ```
//!    (record the pipeline's exit status — it is `CS_EVIDENCE_EXIT_CODE`.
//!    The run needs `CS_GAME_DIR` and `CS_ENGINE_IMAGE` set: the retail rows
//!    bind against the installation and the image row re-reads the cited
//!    bytes, so a run that skips either half is not the acceptance run.)
//! 2. ```sh
//!    CS_EVIDENCE_DIR=private/evidence/T1432 \
//!    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
//!    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_owner_facts_table_ --include-ignored" \
//!    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
//!    CS_EVIDENCE_REVIEWER="<implementer and reviewer, and whether the reviewer's context was fresh>" \
//!      cargo test --locked --test campaign evidence_report_t1432 -- --ignored
//!    ```
//! 3. ```sh
//!    python3 tools/validate_evidence.py private/evidence/T1432/acceptance.json \
//!      --artifact-root private/evidence/T1432 --require-pass
//!    ```
//! 4. Commit a copy of `acceptance.json` as `docs/findings/evidence/T1432.json`.

use super::*;

/// The unignored tests: the table's own schema and the committed-table
/// binding leg, which run in CI without original data and carry the refusal
/// arms.
const SYNTHETIC_TESTS_T1432: &[&str] = &[
    "accept_owner_facts_table_parses_and_spells_the_measured_facts",
    "accept_owner_facts_table_binds_m01_like_the_image_does",
    "accept_owner_facts_table_unknown_missions_refuse_with_both_sources_named",
];

/// The `requires CS_GAME_DIR` tests: the committed-table leg bound against
/// the real installation, M01's whole launch plan, and the campaign-wide
/// coverage row count.
const RETAIL_TESTS_T1432: &[&str] = &[
    "accept_owner_facts_table_retail_m01_binds_through_the_committed_table",
    "accept_owner_facts_table_retail_m01_plans_and_prepares",
    "accept_owner_facts_table_retail_inventory_covers_every_campaign_dir",
];

/// The `requires CS_ENGINE_IMAGE` test: every byte the table cites is read
/// back out of the owner-supplied image, so the committed table cannot
/// silently drift.
const IMAGE_TESTS_T1432: &[&str] = &["accept_owner_facts_table_image_rows_match_the_image"];

/// Evidence-report harness for task T1432: the committed owner-static
/// engine-facts table. Records the `accept_owner_facts_table_` tests, the
/// committed table itself and the two source fingerprints, and claims
/// `implemented` only.
///
/// `CS_EVIDENCE_REVIEWER` fills `review.identity` whole, so whoever runs the
/// harness — the implementing agent at hand-over or the reviewing agent on
/// the rebased commit — writes its own identities and says whether the run
/// is a review. A placeholder identity is a failure.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR, CS_ENGINE_IMAGE"]
fn evidence_report_t1432_writes_the_acceptance_report() {
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
    let suite = parse_suite_prefixed(&log, "accept_owner_facts_table_");
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_owner_facts_table_` tests were recorded in {}",
        log_path.display()
    );
    let required = |test: &&str, needs: &str| {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{test} did not run: T1432 requires {needs}, run step 1 with \
                     `--include-ignored`"
                )
            });
        assert_eq!(status, "pass", "{test} must pass; got status {status}");
    };
    for test in SYNTHETIC_TESTS_T1432 {
        required(test, "the synthetic leg");
    }
    for test in RETAIL_TESTS_T1432 {
        required(test, "capability `retail` (CS_GAME_DIR)");
    }
    for test in IMAGE_TESTS_T1432 {
        required(test, "the image (CS_ENGINE_IMAGE)");
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The committed table, re-read through its production parse: the
    // observation records what the acceptance run's bound values were, from
    // the table itself rather than from this file's memory.
    let facts = cs_app::engine_facts::engine_facts().expect("the committed table parses");
    let table_path = Path::new(&git(&["rev-parse", "--show-toplevel"]))
        .join(cs_app::engine_facts::ENGINE_FACTS_PATH);
    let table_sha256 = sha256(&fs::read(&table_path).expect("the committed table reads")).to_hex();
    let observation_path = evidence_dir.join("engine-facts-observation.json");
    let known: Vec<String> = facts
        .missions()
        .iter()
        .filter_map(|row| match &row.airframe {
            cs_app::engine_facts::MissionAirframe::Known { index, record, va } => Some(format!(
                "{{\"dir\": {}, \"airframe_index\": {index}, \"record\": {}, \"va\": \"0x{va:x}\"}}",
                jstr(&row.dir),
                jstr(record)
            )),
            cs_app::engine_facts::MissionAirframe::Unknown { .. } => None,
        })
        .collect();
    let observation = format!(
        "{{\n\
         \x20\"table\": {},\n\
         \x20\"table_sha256\": {},\n\
         \x20\"image\": {},\n\
         \x20\"image_sha256\": {},\n\
         \x20\"airframe_rows\": {},\n\
         \x20\"mission_rows\": {},\n\
         \x20\"known_missions\": [{}],\n\
         \x20\"unknown_missions\": {}\n\
         }}\n",
        jstr(cs_app::engine_facts::ENGINE_FACTS_PATH),
        jstr(&table_sha256),
        jstr(facts.image()),
        jstr(facts.image_sha256()),
        facts.airframes().len(),
        facts.missions().len(),
        known.join(", "),
        facts.missions().len() - known.len(),
    );
    fs::write(&observation_path, &observation)
        .unwrap_or_else(|error| panic!("write {}: {error}", observation_path.display()));

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&observation_path, "json", &evidence_dir),
        artifact(&table_path, "toml", &evidence_dir),
    ];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"T1432\",\n\
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
             decrypted engine image; every field is derived from the recorded log, production \
             discovery of $CS_GAME_DIR and the production parse of the committed \
             data/owner-static/engine-facts.toml. The suite pins the table's strict-subset parse \
             and self-consistency (eleven airframe rows at the measured stride, M01's record is \
             its airframe row's own `p` field), the committed-table binding producing the same \
             claims and image spans the engine-state binding produces for M01 while every other \
             campaign mission's airframe stays an explicit unknown naming both sources, and — \
             against the installation — M01's full launch plan closing and its content preparing \
             on the table alone, so `cs --cs-path <retail> --mission M01` no longer requires \
             CS_ENGINE_IMAGE. The ignored image row re-reads every cited byte — all seven \
             pointers of each airframe row, the roster record's +0x2c, the π/180 double and the \
             conversion sequence's operand — so the committed table cannot silently drift. The \
             one development value the launch still needs, the player spawn speed, is committed \
             as `designed`, not measured. Claim is implemented only; no original executable ran \
             and nothing is verified_original. Validated with tools/validate_evidence.py \
             --require-pass"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"T1432\"",
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
