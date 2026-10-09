//! Evidence-report harness for task M07-B-FU1: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M07-B-FU1's capability is judged on: M07's
/// three `DANGER_ZONES_COMPLETED` blocks lower the measured flag-count
/// predicate and evaluate it fail-closed, and M07's census row completes
/// while the campaign gate stays closed on the rows whose gaps remain.
const RETAIL_TESTS_M07_B_FU1: &[&str] =
    &["accept_m07_b_fu1_the_danger_zones_predicate_is_the_measured_flag_count"];

/// The synthetic predicate tests M07-B-FU1's report must also record: the
/// parse's own selection rules — the threshold defaulting to the zone count,
/// a spelled count standing as the threshold, and the first site of each key
/// being the one the once-per-block lookup reads — into CI, where there is no
/// original data.
const SYNTHETIC_TESTS_M07_B_FU1: &[&str] =
    &["accept_m07_b_fu1_the_threshold_defaults_to_the_zone_count_and_the_first_site_wins"];

/// Evidence-report harness for task M07-B-FU1 (Rally #813), the follow-up
/// that measures who writes the danger-zone flag bytes in the engine image
/// and lowers `DANGER_ZONES_COMPLETED` from that measurement. It follows the
/// sequence in this module's doc with `M07-B-FU1` and `accept_m07_b_fu1_` in
/// place of `M01-A` and `accept_m01_a_`:
///
/// * the acceptance-log parser selects `accept_m07_b_fu1_` tests, so the
///   recorded assertions are this follow-up's own — the selection must be
///   the prefix's own run (step 1 with that prefix), or the counts would
///   describe a wider run than the assertions;
/// * the only artifact is that log: the change is a lowering predicate
///   derived from a static reading of the image, so the report cites the run
///   that proves it rather than a derived binding;
/// * `review.identity` is a literal naming the real Rally actors and the
///   reviewer's context, as the module doc requires.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M07-B report above.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m07_b_fu1_writes_the_acceptance_report() {
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
    let suite = parse_m07_b_fu1_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m07_b_fu1_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M07_B_FU1 {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M07-B-FU1 requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M07_B_FU1 {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins an arm the retail test assumes")
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

    let artifacts = vec![artifact(&log_path, "log", &evidence_dir)];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M07-B-FU1\",\n\
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
            "acceptance suite run locally with the retail capability; the \
             fields are derived from the recorded log and production discovery of $CS_GAME_DIR. \
             The suite pins this task's change: the `DANGER_ZONES_COMPLETED` predicate is the \
             engine image's own flag evaluator — the once-per-block parse (0x465ec0) strdups each \
             listed zone name and zeroes one flag byte per name, the pass-2 evaluator (0x469ab0) \
             counts the nonzero bytes and fires at count >= threshold with the threshold \
             defaulting to the listed count, and the single runtime writer (0x446990) stores 1 \
             and never 0 — so M07's three sites (blocks 37, 39, 41) lower to a sticky set-of-zones \
             count that answers false without recorded flags, true once the block's own zones are \
             recorded and false for a zone the record never listed, M07's census row completes \
             and the campaign gate stays closed on the rows whose gaps remain; the M08-B suite's \
             pins were updated in the same change. NOT CLAIMED: the producer's world test \
             (0x446930 -> 0x55d6c0 and its two-flagged-entry gate) is untraced and carried as a \
             named residual unknown, no directive is implemented by a measured effect, no \
             mission was played, no original executable was run and nothing is verified_original. \
             Claim is implemented only; validated with \
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
        "\"task_id\": \"M07-B-FU1\"",
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
fn parse_m07_b_fu1_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m07_b_fu1_")
}
