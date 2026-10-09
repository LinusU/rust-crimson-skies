//! Evidence-report harness for task M02-B-FU2: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance test M02-B-FU2's `retail` capability is judged on:
/// M02's record-level sound keys measured through the production binding.
const RETAIL_TESTS_M02_B_FU2: &[&str] =
    &["accept_m02_b_fu2_m02s_five_record_sound_keys_are_measured_with_their_consumers"];

/// The engine-image acceptance test M02-B-FU2's static code reading is
/// judged on: production's parse sites, field offsets and consumer sites read
/// back out of `$CS_ENGINE_IMAGE`. The image is the owner-supplied decrypted
/// executable (SHA-256 `43540fc9…`), read-only and never committed; the test
/// is `#[ignore = "requires CS_ENGINE_IMAGE"]` like the retail member is
/// `#[ignore = "requires CS_GAME_DIR"]`, so step 1 needs both set.
const IMAGE_TESTS_M02_B_FU2: &[&str] =
    &["accept_m02_b_fu2_the_image_parses_and_consumes_each_sound_key_where_production_says"];

/// The synthetic predicate test M02-B-FU2's report must also record: it runs
/// in CI without any original data and holds the vocabulary to the
/// disposition table.
const SYNTHETIC_TESTS_M02_B_FU2: &[&str] =
    &["accept_m02_b_fu2_the_sound_vocabulary_is_entirely_measured_and_answers_for_nothing_else"];

/// Evidence-report harness for task M02-B-FU2, the follow-up that measures
/// the five record-level sound keys M02's control record spells and admits
/// them to `cs_content::mission_control`'s record vocabulary (Rally #801). It
/// follows the sequence in this module's doc with `M02-B-FU2` and
/// `accept_m02_b_fu2_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// four ways from the M02-A report above:
///
/// * the acceptance-log parser selects `accept_m02_b_fu2_` tests, so the
///   recorded assertions are this follow-up's own;
/// * the artifact beside the report is the **measured record-sound
///   vocabulary**: for each of the five keys its consumer, class, mission
///   field offset, parse site, consumer site, summary, evidence documents and
///   residual unknowns, plus the sites and value shape M02's record spells
///   beside it — dispositions, addresses and counts only, never original
///   bytes or sound names;
/// * the run must carry **three** members: the retail binding test, the
///   engine-image test that reads `$CS_ENGINE_IMAGE` at production's recorded
///   addresses (step 1 needs `CS_GAME_DIR` *and* `CS_ENGINE_IMAGE` set), and
///   the synthetic CI test;
/// * `CS_EVIDENCE_REVIEWER` fills `review.identity` whole (the runtime
///   identity shape `jstr(&reviewer)` reads), so the report can never carry a
///   hand-over placeholder: the runner supplies the full identity text — the
///   implementing agent's own run names the implementer and says the run is
///   the implementer's evidence and not a review, and the reviewing agent's
///   run names itself.
///
/// The report records what M02-B-FU2 does *not* claim: a measured disposition
/// is static code evidence, not a licence — no sound is played, the handle's
/// sound identity stays runtime state, M02's control record lowers only since
/// M02-B-FU1 (#800), a lowering result and not an implemented effect, no
/// mission was played, no original executable was run and nothing is
/// `verified_original`.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR, CS_ENGINE_IMAGE"]
fn evidence_report_m02_b_fu2_writes_the_acceptance_report() {
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
    let suite = parse_m02_b_fu2_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m02_b_fu2_` tests were recorded in {}",
        log_path.display()
    );
    for (tests, capability) in [
        (RETAIL_TESTS_M02_B_FU2, "retail"),
        (IMAGE_TESTS_M02_B_FU2, "the owner-supplied engine image"),
        (SYNTHETIC_TESTS_M02_B_FU2, "synthetic"),
    ] {
        for task_test in tests {
            let status = suite
                .assertions
                .iter()
                .find(|(name, _)| name == task_test)
                .map(|(_, status)| *status)
                .unwrap_or_else(|| {
                    panic!(
                        "{task_test} did not run: M02-B-FU2 needs {capability}, run step 1 with \
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

    // The measured record-sound vocabulary, written beside the report and
    // referenced by digest: dispositions, addresses and counts, never the
    // sound names the record spells.
    let toplevel = git(&["rev-parse", "--show-toplevel"]);
    let inventory = CampaignInventory::load(
        &Path::new(&toplevel).join("missions/bindings/campaign-inventory.tsv"),
    )
    .unwrap_or_else(|error| {
        panic!("read missions/bindings/campaign-inventory.tsv from {toplevel}: {error}")
    });
    let title = inventory
        .iter()
        .find(|(work_order, _)| work_order.as_str() == "M02")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M02 work order");
    let context =
        SourceContext::read(&game_dir).expect("production source context reads the installation");
    let control = context
        .control_program(
            MissionLabel::new("M02").expect("M02 is a valid label"),
            &title,
        )
        .expect("M02's control program binds through the measured rule");
    assert_eq!(
        context.install_sha256(),
        install_sha256,
        "the control binding was derived under a different installation fingerprint than discovery reports"
    );
    let sounds: Vec<String> = CONTROL_RECORD_SOUND_KEY_VOCABULARY
        .iter()
        .map(|key| {
            let Some(RecordSoundDisposition::Measured(measured)) = record_sound_disposition(key)
            else {
                panic!("{key} is a vocabulary key and is measured, not refused");
            };
            let sites = control
                .record
                .record_sounds()
                .iter()
                .find(|(field, _)| field.key() == *key)
                .map(|(_, sites)| *sites);
            let shape = control
                .record
                .record_sound_shapes()
                .iter()
                .find(|(field, _)| field == key)
                .map(|(_, shape)| shape.label());
            let evidence: Vec<String> = measured.evidence.iter().map(|item| jstr(item)).collect();
            let unknowns: Vec<String> = measured.unknowns.iter().map(|item| jstr(item)).collect();
            format!(
                "{{\"key\": {}, \"sites\": {}, \"shape\": {}, \"consumer\": {}, \"class\": {}, \
                 \"field_offset\": {}, \"parse_site\": {}, \"consumer_site\": {}, \"summary\": {}, \
                 \"evidence\": [{}], \"unknowns\": [{}]}}",
                jstr(key),
                sites.map_or_else(|| "null".to_owned(), |sites| sites.to_string()),
                shape.map_or_else(|| "null".to_owned(), |shape| jstr(&shape)),
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
    let sounds_path = evidence_dir.join("m02-b-fu2-record-sounds.json");
    let sounds_json = format!(
        "{{\n\
         \x20\"task_id\": \"M02-B-FU2\",\n\
         \x20\"install_sha256\": {},\n\
         \x20\"mission\": {},\n\
         \x20\"program_id\": {},\n\
         \x20\"control_member\": {},\n\
         \x20\"control_sha256\": {},\n\
         \x20\"record\": {{\"blocks\": {}, \"sites\": {}, \"vocabulary\": {}, \
         \"unclassified_record_keys\": {}}},\n\
         \x20\"sounds\": [{}]\n\
         }}\n",
        jstr(context.install_sha256()),
        jstr(control.mission.as_str()),
        jstr(control.program_id.as_str()),
        jstr(&control.control_member),
        jstr(&control.control_sha256),
        control.record.blocks(),
        control.record.sites(),
        control.record.vocabulary(),
        str_array(control.unclassified_record_keys()),
        sounds.join(", "),
    );
    fs::write(&sounds_path, sounds_json)
        .unwrap_or_else(|error| panic!("write {}: {error}", sounds_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&sounds_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M02-B-FU2\",\n\
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
             discovery of $CS_GAME_DIR and the control-program binding \
             `SourceContext::control_program` derives from it, and the dispositions come from \
             `cs_content::mission_control::record_sound_disposition` — five keys, each measured \
             with its consumer, its mission-object field, its parse site and its consumer site. \
             The image member re-reads $CS_ENGINE_IMAGE (the owner-supplied decrypted executable, \
             sha256 43540fc9…) at those addresses and re-derives both selectors — the \
             objective-class dec/je chain and the mission-end won flag — from the instruction \
             bytes: static code reading only, no original run. NOT CLAIMED: no sound is played \
             here and the handle's sound identity stays runtime state; M02's control record \
             lowers completely since M02-B-FU1 (#800), which is lowering evidence only, not an \
             implemented effect — the campaign gate still needs every row; the two original \
             record keys this task did not admit \
             (OBJECTIVES_WON/LOST_SOUND) were since measured and admitted by \
             RECORD-OBJECTIVES-SOUND (#808) — no retail record spells either; no mission was \
             played, no original \
             executable was run and nothing is verified_original. Claim is implemented only; \
             validated with tools/validate_evidence.py --require-pass. `candidate_tree` is the \
             tree of the commit the suite and this harness ran on; the only later delta is this \
             report's own copy under docs/findings/evidence/ and the findings document that \
             discusses it, neither of which the acceptance suite reads"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M02-B-FU2\"",
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
/// assertions are M02-B-FU2's own and not the `accept_m02_b_` suites that
/// share the parent selection.
fn parse_m02_b_fu2_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m02_b_fu2_")
}
