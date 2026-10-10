//! Evidence-report harness for task M06-B-FU2: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M06-B-FU2's `retail` capability is judged on:
/// M06's only passenger-named string is the shared `location.zrd` place list,
/// and no shipped record — startup animation, world node or message id — binds
/// a passenger entity to the mission.
const RETAIL_TESTS_M06_B_FU2: &[&str] = &[
    "accept_m06_b_fu2_the_only_passenger_string_in_m06s_own_data_is_a_map_location",
    "accept_m06_b_fu2_no_shipped_record_binds_a_passenger_entity_for_m06",
];

/// The engine-image acceptance test M06-B-FU2's static reading is judged on:
/// the member's consumer is the original's Teleport feature, measured as the
/// strings that run spells. The image is the owner-supplied decrypted
/// executable (SHA-256 `43540fc9…`), read-only and never committed; the test is
/// `#[ignore = "requires CS_ENGINE_IMAGE"]` like the retail members are
/// `#[ignore = "requires CS_GAME_DIR"]`, so step 1 needs both set.
const IMAGE_TESTS_M06_B_FU2: &[&str] =
    &["accept_m06_b_fu2_the_image_spells_location_zrd_as_teleport_data"];

/// The synthetic predicate test M06-B-FU2's report must also record: the
/// location record's shape — a name plus two float triples, carrying nothing
/// that could be an actor — on an authored document, in CI, where there is no
/// original data.
const SYNTHETIC_TESTS_M06_B_FU2: &[&str] =
    &["accept_m06_b_fu2_a_location_entry_is_a_name_plus_two_float_triples"];

/// Evidence-report harness for task M06-B-FU2 (Rally #818), the follow-up
/// that answers where — if anywhere — M06's passenger/extraction entity
/// lives. It follows the sequence in this module's doc with `M06-B-FU2` and
/// `accept_m06_b_fu2_` in place of `M01-A` and `accept_m01_a_`:
///
/// * the acceptance-log parser selects `accept_m06_b_fu2_` tests, so the
///   recorded assertions are this follow-up's own — the selection must be the
///   prefix's own run (step 1 with that prefix), or the counts would describe
///   a wider run than the assertions;
/// * the only artifact is that log: the task is a measurement over the
///   installation, so the report cites the run that proves it rather than a
///   derived binding, and `missions/bindings/M06.json` is unchanged (its
///   `unknowns` are production-derived from one global table, so carrying the
///   verdict there is the filed follow-up M06-B-FU4, Rally #1184);
/// * the review identity is written by whoever runs the harness, as the
///   module doc requires: this report is the implementer's hand-over run, and
///   the reviewing agent regenerates it on the reviewed commit.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR, CS_ENGINE_IMAGE"]
fn evidence_report_m06_b_fu2_writes_the_acceptance_report() {
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
    let suite = parse_m06_b_fu2_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m06_b_fu2_` tests were recorded in {}",
        log_path.display()
    );
    for (tests, capability) in [
        (RETAIL_TESTS_M06_B_FU2, "retail"),
        (IMAGE_TESTS_M06_B_FU2, "the owner-supplied engine image"),
        (SYNTHETIC_TESTS_M06_B_FU2, "synthetic"),
    ] {
        for task_test in tests {
            let status = suite
                .assertions
                .iter()
                .find(|(name, _)| name == task_test)
                .map(|(_, status)| *status)
                .unwrap_or_else(|| {
                    panic!(
                        "{task_test} did not run: M06-B-FU2 needs {capability}, run step 1 with \
                         `--include-ignored` and CS_GAME_DIR / CS_ENGINE_IMAGE set"
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
         \x20\"task_id\": \"M06-B-FU2\",\n\
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
             engine image; the fields are derived from the recorded log and production discovery \
             of $CS_GAME_DIR. The suite pins this task's verdict: the shipped data binds no \
             passenger or extraction entity to M06. The archive's one passenger-named string is \
             `Passenger_hangar`, an entry of `location.zrd` — a name plus a position and a \
             heading, byte-identical to the sibling mission's copy, whose consumer the decrypted \
             image spells as a Teleport feature; the installation's other passenger vocabulary is \
             the library's seventeen ON_CALL crew animations over the world node `apassengers` \
             (M06 fires none), the chapter world's `apassengers`/`passall` nodes (addressed by \
             nothing in M06), and the instant-action record's `passenger_zeppelin` class; \
             `MSG_OBJ_PASSENGERHANGER` is carried by chapter one's instant action only. NOT \
             CLAIMED: what the teleport feature does with an entry, whether M06's objective texts \
             mention passengers (their name-to-string-id join is unmeasured) and every runtime \
             half stay open — a predicate needs an original reference run (M06-C) or the owner's \
             ruling; nothing is verified_original and no original executable was run. Claim is \
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
        "\"task_id\": \"M06-B-FU2\"",
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
fn parse_m06_b_fu2_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m06_b_fu2_")
}
