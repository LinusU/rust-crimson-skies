//! Evidence-report harness for task RECORD-OBJECTIVES-SOUND: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance test RECORD-OBJECTIVES-SOUND's `retail` capability
/// is judged on: the production census measures every mission-scoped reader
/// and no control member spells either admitted key.
const RETAIL_TESTS_RECORD_OBJECTIVES_SOUND: &[&str] =
    &["accept_record_objectives_sound_no_retail_control_member_spells_either_key"];

/// The engine-image acceptance test RECORD-OBJECTIVES-SOUND's static code
/// reading is judged on: the two parse blocks, the flag accessors and the
/// end-of-tick outcome block read back out of `$CS_ENGINE_IMAGE`. The image
/// is the owner-supplied decrypted executable (SHA-256 `43540fc9…`),
/// read-only and never committed; the test is
/// `#[ignore = "requires CS_ENGINE_IMAGE"]` like the retail member is
/// `#[ignore = "requires CS_GAME_DIR"]`, so step 1 needs both set.
const IMAGE_TESTS_RECORD_OBJECTIVES_SOUND: &[&str] =
    &["accept_record_objectives_sound_the_image_gates_and_consumes_both_keys_at_end_of_tick"];

/// The synthetic predicate test RECORD-OBJECTIVES-SOUND's report must also
/// record: it runs in CI without any original data and holds the vocabulary,
/// the enum surface and the disposition table to each other.
const SYNTHETIC_TESTS_RECORD_OBJECTIVES_SOUND: &[&str] =
    &["accept_record_objectives_sound_the_two_keys_are_measured_with_their_outcome_consumer"];

/// Evidence-report harness for task RECORD-OBJECTIVES-SOUND, the follow-up
/// that measures the two `OBJECTIVES_*_SOUND` record keys the original's
/// parser spells beside M02-B-FU2's five and admits them to
/// `cs_content::mission_control`'s record sound vocabulary (Rally #808). It
/// follows the sequence in this module's doc with `RECORD-OBJECTIVES-SOUND`
/// and `accept_record_objectives_sound_` in place of `M01-A` and
/// `accept_m01_a_`, and differs in two ways from the M02-B-FU2 report above:
///
/// * the artifact beside the report is the **census result plus the two
///   measured dispositions**: for each admitted key its consumer, mission
///   field offset, parse site, consumer site, summary, evidence documents
///   and residual unknowns, and the missions spelling it — both measured
///   `[]` — as `cs_app::mission_control::survey_mission_control_programs`
///   reports them. Dispositions, addresses and counts only, never original
///   bytes or sound names;
/// * the run must carry **three** members: the retail census test, the
///   engine-image test that reads `$CS_ENGINE_IMAGE` at production's
///   recorded addresses (step 1 needs `CS_GAME_DIR` *and* `CS_ENGINE_IMAGE`
///   set), and the synthetic CI test.
///
/// The report records what RECORD-OBJECTIVES-SOUND does *not* claim: a
/// measured disposition is static code evidence, not a licence — no sound is
/// played, the handle's sound identity stays runtime state, which branch ran
/// is not the recorded outcome (F37-D-FU2), no retail mission's classified
/// content changes (no member spells either key), no mission was played, no
/// original executable was run and nothing is `verified_original`.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR, CS_ENGINE_IMAGE"]
fn evidence_report_record_objectives_sound_writes_the_acceptance_report() {
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
    let suite = parse_record_objectives_sound_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_record_objectives_sound_` tests were recorded in {}",
        log_path.display()
    );
    for (tests, capability) in [
        (RETAIL_TESTS_RECORD_OBJECTIVES_SOUND, "retail"),
        (
            IMAGE_TESTS_RECORD_OBJECTIVES_SOUND,
            "the owner-supplied engine image",
        ),
        (SYNTHETIC_TESTS_RECORD_OBJECTIVES_SOUND, "synthetic"),
    ] {
        for task_test in tests {
            let status = suite
                .assertions
                .iter()
                .find(|(name, _)| name == task_test)
                .map(|(_, status)| *status)
                .unwrap_or_else(|| {
                    panic!(
                        "{task_test} did not run: RECORD-OBJECTIVES-SOUND needs {capability}, run \
                         step 1 with `--include-ignored` and CS_GAME_DIR / CS_ENGINE_IMAGE set"
                    )
                });
            assert_eq!(status, "pass", "{task_test} must pass; got status {status}");
        }
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The production census, run a second time for the report: every measured
    // row's record sound keys, so the "no member spells either admitted key"
    // claim is derived rather than typed in.
    let census = cs_app::mission_control::survey_mission_control_programs(&game_dir)
        .expect("the installation measures a control census");
    let mut spelling: Vec<String> = Vec::new();
    for key in ["OBJECTIVES_WON_SOUND", "OBJECTIVES_LOST_SOUND"] {
        let missions: Vec<String> = census
            .measured_rows()
            .filter(|row| {
                row.record().is_some_and(|record| {
                    record
                        .record_sounds()
                        .iter()
                        .any(|(sound, _)| sound.key() == key)
                })
            })
            .map(|row| row.mission.clone())
            .collect();
        assert!(
            missions.is_empty(),
            "the evidence run itself must measure zero spellers; {key} is spelled by {missions:?}"
        );
        spelling.push(format!(
            "{{\"key\": {}, \"missions\": {}}}",
            jstr(key),
            str_array(&missions)
        ));
    }

    // The two admitted keys' measured dispositions, written beside the report
    // and referenced by digest: consumers, addresses and counts, never the
    // sound names a record might spell.
    let sounds: Vec<String> = ["OBJECTIVES_WON_SOUND", "OBJECTIVES_LOST_SOUND"]
        .iter()
        .map(|key| {
            let Some(RecordSoundDisposition::Measured(measured)) = record_sound_disposition(key)
            else {
                panic!("{key} is a vocabulary key and is measured, not refused");
            };
            let evidence: Vec<String> = measured.evidence.iter().map(|item| jstr(item)).collect();
            let unknowns: Vec<String> = measured.unknowns.iter().map(|item| jstr(item)).collect();
            format!(
                "{{\"key\": {}, \"consumer\": {}, \"class\": {}, \"field_offset\": {}, \
                 \"parse_site\": {}, \"consumer_site\": {}, \"summary\": {}, \
                 \"evidence\": [{}], \"unknowns\": [{}]}}",
                jstr(key),
                jstr(measured.consumer.code()),
                measured
                    .consumer
                    .class()
                    .map_or_else(|| "null".to_owned(), |class| class.to_string()),
                measured.field_offset,
                measured.parse_site,
                measured.consumer_site,
                jstr(measured.summary),
                evidence.join(", "),
                unknowns.join(", "),
            )
        })
        .collect();
    let census_path = evidence_dir.join("record-objectives-sound-census.json");
    let census_json = format!(
        "{{\n\
         \x20\"task_id\": \"RECORD-OBJECTIVES-SOUND\",\n\
         \x20\"install_sha256\": {},\n\
         \x20\"census\": {{\"rows\": {}, \"measured\": {}, \"absent\": {}}},\n\
         \x20\"missions_spelling\": [{}],\n\
         \x20\"sounds\": [{}]\n\
         }}\n",
        jstr(&install_sha256),
        census.rows().len(),
        census.measured_len(),
        str_array(
            &census
                .archives_without_control_program()
                .iter()
                .map(|mission| mission.to_string())
                .collect::<Vec<_>>()
        ),
        spelling.join(", "),
        sounds.join(", "),
    );
    fs::write(&census_path, census_json)
        .unwrap_or_else(|error| panic!("write {}: {error}", census_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&census_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"RECORD-OBJECTIVES-SOUND\",\n\
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
            "acceptance suite re-run locally with the retail capability and the owner-supplied \
             engine image; this harness derives every field from the recorded log, production \
             discovery of $CS_GAME_DIR and the production census \
             `cs_app::mission_control::survey_mission_control_programs` derives from it, and the \
             dispositions come from `cs_content::mission_control::record_sound_disposition` — two \
             keys, each measured with its end-of-tick outcome consumer, its mission-object field, \
             its parse site and its consumer site. The image member re-reads $CS_ENGINE_IMAGE \
             (the owner-supplied decrypted executable, sha256 43540fc9…) at those addresses and \
             re-derives the selector — the lost-flag-first branch order, the null-handle skips \
             and the shared play call — from the instruction bytes: static code reading only, no \
             original run. The retail member re-measures the whole installation through the \
             production census and finds zero control members spelling either key, so the \
             admission completes the parser's known vocabulary without changing any retail \
             mission's classified content. NOT CLAIMED: no sound is played here and the handle's \
             sound identity stays runtime state; playing an objectives handle is not the recorded \
             mission outcome (F37-D-FU2); no mission was played, no original executable was run \
             and nothing is verified_original. Claim is implemented only; validated with \
             tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the \
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
        "\"task_id\": \"RECORD-OBJECTIVES-SOUND\"",
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

/// [`parse_suite_prefixed`] with this task's test prefix, so the recorded
/// assertions are RECORD-OBJECTIVES-SOUND's own.
fn parse_record_objectives_sound_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_record_objectives_sound_")
}
