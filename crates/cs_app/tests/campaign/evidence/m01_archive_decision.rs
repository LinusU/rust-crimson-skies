//! Evidence-report harness for task M01-ARCHIVE-DECISION (Rally #1565): the
//! task's own test list and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where the module
//! doc says where a new task's evidence goes.
//!
//! The verdict this report records is the measured title → campaign-position →
//! archive join: `missions/bindings/M01.json` now binds `ZBD/C3/M01/zrdr.zbd`
//! (sound-group code `snd_c1-HA-m1`, briefing ids `MSG_BRF_HAM1_*`), and the
//! archive directory naming (`C1C`) that bound M01 to `ZBD/C1C/M01/zrdr.zbd`
//! is shown to be the storage location of campaign position `ch2-m01` instead.

use super::*;

/// The retail acceptance tests this task's capabilities are judged on: the
/// corrected derivation pinned at both ends of the moved join — M01's record
/// derived from `ZBD/C3/M01`, M06's from `ZBD/C1C/M01`, M12's from
/// `ZBD/C2/M01` — plus the scoped-knowledge tests that follow the archive the
/// measurements were actually taken on.
const RETAIL_TESTS: &[&str] = &[
    "accept_m01_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m01_a_the_committed_record_is_what_the_installation_derives",
    "accept_m01_a_a_title_the_local_strings_do_not_carry_is_unresolved",
    "accept_m01_a_the_campaign_keeps_everything_else_unresolved_and_unready",
    "accept_m01_a_the_retail_title_block_binds_every_campaign_position",
    "accept_m01_a_a_title_outside_the_retail_title_block_resolves_no_position",
    "accept_m06_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m06_a_the_committed_record_is_what_the_installation_derives",
    "accept_m06_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m12_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m12_a_the_committed_record_is_what_the_installation_derives",
    "accept_m12_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m01_b_m06s_record_binds_the_measured_results_and_m01s_does_not",
    "accept_m06_b_fu4_m12s_record_names_the_passenger_identity_limitation_and_m06s_does_not",
];

#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m01_archive_decision_writes_the_acceptance_report() {
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
    // The task's acceptance selection spans several `accept_*` prefixes
    // (m01_a/m06_a/m12_a/m01_b/m06_b_fu4/f50_c), so the parser keeps every
    // acceptance test the recorded run executed rather than one stage's.
    let suite = parse_suite_prefixed(&log, "accept_");
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no acceptance tests were recorded in {}",
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
                    "{retail_test} did not run: M01-ARCHIVE-DECISION requires capability \
                     `retail`, run step 1 with `--include-ignored` and CS_GAME_DIR set"
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
            .any(|(name, _)| name
                == "accept_m01_b_a_settled_checklist_entry_reaches_only_the_work_order_it_was_settled_for"),
        "the synthetic predicate tests must be present alongside the retail ones"
    );

    // `source` hashes describe the real installation, measured by the very
    // production discovery this task's binding is built on.
    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The verdict, re-derived through production: M01's binding must cite the
    // `ZBD/C3/M01` archive and M06's the `ZBD/C1C/M01` archive, and no critical
    // dependency may be left unresolved by the re-derivation.
    let m01_binding = source_binding_for(&game_dir, "M01");
    let m06_binding = source_binding_for(&game_dir, "M06");
    for (work_order, binding) in [("M01", &m01_binding), ("M06", &m06_binding)] {
        assert!(
            binding.unresolved_critical().is_empty(),
            "{work_order}: the acceptance run passed while a critical dependency is \
             unresolved; the report must not be written"
        );
    }
    let binding_path = evidence_dir.join("m01-binding.json");
    fs::write(&binding_path, m01_binding.to_json())
        .unwrap_or_else(|error| panic!("write {}: {error}", binding_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&binding_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M01-ARCHIVE-DECISION\",\n\
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
        jstr(
            "implementer: claude-2/claude-2 (Rally #1565 M01-ARCHIVE-DECISION, resumed \
             2026-10-17T22:25Z after the implement lease of Devin SWE-2/swe2-max-1, the \
             original implementer, expired); reviewer: claude-2/claude-2 (Rally review \
             claim of 2026-10-17T23:02Z) - the same agent identity as the resuming \
             implementer, context not fresh, so this review is not independent; the \
             committed report was generated by the original implementer before the rebase \
             and the reviewer did not regenerate it, an agent review of the code and tests \
             only, not original-run or original-reference evidence; no agent review \
             replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability; this harness derives \
             every field from the recorded log, production discovery of $CS_GAME_DIR and the \
             bindings `SourceContext::read` + `SourceContext::bind` derive from it. The \
             measured join (per-archive sound-group codes, the langui.dll title blocks, the \
             scrapbook ordering and the MSG_BRF_* briefing ids) binds campaign position \
             ch1-m01 to ZBD/C3/M01 and ch2-m01 to ZBD/C1C/M01; directory digits were never \
             the chapter. The affected-findings census and remaining unknowns live in \
             docs/findings/2026-10-17-m01-archive-decision.md; claim is implemented only; \
             validated with tools/validate_evidence.py --require-pass"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M01-ARCHIVE-DECISION\"",
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
