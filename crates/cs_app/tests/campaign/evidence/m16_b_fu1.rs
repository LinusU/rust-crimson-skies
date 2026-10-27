//! Evidence-report harness for task M16-B-FU1: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M16-B-FU1's `retail` capability is judged
/// on: M16's seven sound-cleanup sites bind and no census row anywhere
/// still refuses `STOP_QUEUED_SOUNDS`.
const RETAIL_TESTS_M16_B_FU1: &[&str] = &[
    "accept_m16_b_fu1_m16s_record_lowers_and_the_four_comment_words_are_ignored",
    "accept_m16_b_fu1_no_census_row_refuses_a_sound_cleanup_site",
];

/// The engine-image acceptance test M16-B-FU1's static code reading is
/// judged on: the ten-name bound `stop_queued_sounds_applied` applies must
/// be the `cmp esi, 0xa; jg` the M01-LC-D finding measured in the original
/// parser's name copy, read back out of `$CS_ENGINE_IMAGE`.
const IMAGE_TESTS_M16_B_FU1: &[&str] =
    &["accept_m16_b_fu1_the_ten_name_cap_the_copy_loop_applies_is_in_the_image"];

/// The synthetic predicate test M16-B-FU1's report must also record: the
/// spelled name list carried as one `Value::List` argument and the
/// original's ten-name cap applied by the consumer helper, into CI where
/// there is no original data.
const SYNTHETIC_TESTS_M16_B_FU1: &[&str] = &[
    "accept_m16_b_fu1_sound_name_lists_bind_as_one_list_argument_and_the_original_cap_is_applied",
];

/// Evidence-report harness for task M16-B-FU1 (Rally #1252), the follow-up
/// that carries `STOP_QUEUED_SOUNDS`'s spelled name list as one
/// `Value::List` argument. It follows the sequence in this module's doc
/// with `M16-B-FU1` and `accept_m16_b_fu1_` in place of `M01-A` and
/// `accept_m01_a_`:
///
/// * the acceptance-log parser selects `accept_m16_b_fu1_` tests, so the
///   recorded assertions are this follow-up's own — the selection must be
///   the prefix's own run (step 1 with that prefix), or the counts would
///   describe a wider run than the assertions;
/// * the only artifact is that log: the change is a lowering-adapter shape
///   decision plus a consumer-boundary helper, so the report cites the run
///   that proves both rather than a derived binding;
/// * `review.identity` is read at run time from `CS_EVIDENCE_REVIEWER` (the
///   M02-B reading), so the implementer and the reviewer that really ran
///   write their own; a literal is never typed in here.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M16-B report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR, CS_ENGINE_IMAGE"]
fn evidence_report_m16_b_fu1_writes_the_acceptance_report() {
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
    assert!(
        !reviewer.trim().is_empty(),
        "CS_EVIDENCE_REVIEWER must name the reviewer that really ran"
    );
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
    let suite = parse_m16_b_fu1_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m16_b_fu1_` tests were recorded in {}",
        log_path.display()
    );

    for (tests, capability) in [
        (RETAIL_TESTS_M16_B_FU1, "retail"),
        (IMAGE_TESTS_M16_B_FU1, "the owner-supplied engine image"),
        (SYNTHETIC_TESTS_M16_B_FU1, "synthetic"),
    ] {
        for task_test in tests {
            let status = suite
                .assertions
                .iter()
                .find(|(name, _)| name == task_test)
                .map(|(_, status)| *status)
                .unwrap_or_else(|| {
                    panic!(
                        "{task_test} did not run: M16-B-FU1 needs {capability}, run step 1 with \
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

    // The census-wide outcome the task pins — every spelled
    // STOP_QUEUED_SOUNDS site and no refusal naming the key — written
    // beside the report and referenced by digest: mission labels, site
    // counts and lowering verdicts, never original bytes or display text.
    let census = cs_app::mission_control::survey_mission_control_programs(&game_dir)
        .expect("the census measures the installation");
    let mut sites: Vec<String> = Vec::new();
    let mut sound_refusals = 0usize;
    for row in census.measured_rows() {
        let Some(record) = row.record() else {
            continue;
        };
        let Some(key) = record.key("STOP_QUEUED_SOUNDS") else {
            continue;
        };
        sites.push(format!(
            "{{\"row\": {}, \"sites\": {}}}",
            jstr(row.mission()),
            key.sites
        ));
        if let Some(lowered) = row.lowering_attempt() {
            let attempt = lowered.attempt();
            sound_refusals += attempt
                .unbound_keys
                .iter()
                .filter(|text| text.contains("STOP_QUEUED_SOUNDS"))
                .count();
            sound_refusals += attempt
                .calls
                .iter()
                .filter(|outcome| match outcome {
                    cs_content::mission_control::CallOutcome::Refused(text) => {
                        text.contains("STOP_QUEUED_SOUNDS")
                    }
                    cs_content::mission_control::CallOutcome::Bound => false,
                })
                .count();
        }
    }
    let census_path = evidence_dir.join("sound-cleanup-census.json");
    let census_json = format!(
        "{{\n\
         \x20\"task_id\": \"M16-B-FU1\",\n\
         \x20\"install_sha256\": {},\n\
         \x20\"stop_queued_sounds_rows\": [{}],\n\
         \x20\"refusals_naming_the_key\": {}\n\
         }}\n",
        jstr(install_sha256.as_str()),
        sites.join(", "),
        sound_refusals,
    );
    fs::write(&census_path, census_json)
        .unwrap_or_else(|error| panic!("write {}: {error}", census_path.display()));

    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&census_path, "json", &evidence_dir),
    ];

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M16-B-FU1\",\n\
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
             engine image; every field is derived from the recorded log, production discovery \
             of $CS_GAME_DIR and the mission-control census the suite measures. The suite pins \
             this task's change: `STOP_QUEUED_SOUNDS`'s spelled name list is carried as ONE \
             `Value::List` argument — the shape the other list-taking keys already use — so the \
             9- and 14-name signatures stay inside `MAX_CALL_ARGS`, all seven of M16's sites \
             bind, `unbound_keys` no longer names the key, and no measured record census-wide \
             refuses it; `DirectiveOperation::stop_queued_sounds_applied` hands the eventual \
             consumer the first ten names, the bound the original's `cmp esi, 0xa; jg` name \
             copy at VA 0x4688bc applies, verified in the image — the spelled fourteen stay \
             faithful data in the call. NOT CLAIMED: no consumer runs the operation yet, so no \
             runtime behavior is pinned; M16's record lowers now that OBJECTIVE24's four \
             bare comment words are measured ignored by the original (#2134), and M24's \
             truncated `SET_AI_` site is likewise ignored (#2135), so both census rows are \
             complete; no mission was played, no original executable was run and nothing is \
             verified_original. Claim is implemented only; validated with \
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
        "\"task_id\": \"M16-B-FU1\"",
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

/// [`parse_suite_prefixed`] with this task's own test prefix.
fn parse_m16_b_fu1_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m16_b_fu1_")
}
