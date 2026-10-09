//! Evidence-report harness for task M18-A: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M18-A's capabilities are judged on.
const RETAIL_TESTS_M18_A: &[&str] = &[
    "accept_m18_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m18_a_the_committed_record_is_what_the_installation_derives",
    "accept_m18_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m18_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m18_a_the_position_is_interior_to_the_fourth_chapter_and_its_region_group",
    "accept_m18_a_the_world_group_is_the_whole_chapter_and_neither_it_nor_the_mission_number_identifies_the_mission",
    "accept_m18_a_m18s_own_region_prefix_is_a_confirmed_row_that_selects_no_position",
    "accept_m18_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M18-A's report must also record.
const SYNTHETIC_TESTS_M18_A: &[&str] = &[
    "accept_m18_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m18_a_a_confirmed_row_outside_every_campaign_block_selects_no_position",
    "accept_m18_a_a_near_miss_title_is_never_confirmed",
    "accept_m18_a_a_contradicted_corroboration_establishes_no_position",
    "accept_m18_a_a_verified_needs_every_condition_and_not_only_the_dependencies",
];

/// Evidence-report harness for task M18-A, the eighteenth mission's source
/// binding. It follows the sequence in this module's doc with `M18-A` and
/// `accept_m18_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m18_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M18** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records what M18-A adds: the eighteenth campaign position is
///   the *third* mission of chapter 4 — strictly interior to the layout's
///   chapter group and the localized long names' fourth region group — its
///   world group `c4` is the whole chapter rather than the mission, the
///   mission number `3` names a mission in every chapter, and the stage
///   exercises on real data the refusal arm no earlier stage reached: a
///   *confirmed* localized row that names no campaign position, because it
///   sits outside every campaign-length row block. M18's own region prefix is
///   such a row. The reviewing agent added a fifth synthetic entry: M18's own
///   record is unverified for four separate reasons, so no `is_verified`
///   assertion in the suite could tell the conditions apart, and dropping one
///   of them survived the suite.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m18_a_writes_the_acceptance_report() {
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
    let suite = parse_m18_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m18_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M18_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M18-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M18_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!("{synthetic_test} did not run: it pins predicates the retail tests assume")
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

    let source_binding = source_binding_for(&game_dir, "M18");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m18-binding.json");
    fs::write(&binding_path, source_binding.to_json())
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
         \x20\"task_id\": \"M18-A\",\n\
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
            "implementer: bunny-alpha-2 (OpenCode, Space Bunny Alpha, Rally #309, session of \
             2026-10-02T01:27Z); reviewing agent: bunny-alpha-1 (OpenCode, Space Bunny Alpha, \
             Rally #309 review claim, session of 2026-10-02T03:05Z, fresh context that did not \
             write the implementation and re-derived M18's retail facts from the installation \
             independently of the binding code). A different agent instance from the implementer, \
             so this is agent review, not independent original-reference evidence; no agent review \
             replaces the owner's human approval, and the claim stays `implemented`"
        ),
        jstr(
            "acceptance suite run locally with the retail capability, each test also executed \
             alone with `--exact --include-ignored`; this harness derives every field from the \
             recorded log, production discovery of $CS_GAME_DIR and the binding `SourceContext::\
             read` + `SourceContext::bind` derive from it (all five critical dependencies \
             resolved; checklist entries still unknown are recorded in missions/bindings/M18.json, \
             not dropped). No production code changed at M18-A: the join, its corroboration, the \
             two display-form confirmation and the guard are M02-A's and M05-A's, and this stage \
             exercises them at the eighteenth campaign position — the third mission of chapter 4, \
             strictly interior to the layout's chapter group and to the localized long names' \
             fourth region group, whose world group c4 is the whole chapter and whose mission \
             number 3 names a mission in every chapter. What M18-A adds is the refusal arm no \
             earlier stage reached on real data: a *confirmed* localized row that names no \
             campaign position, exercised on M18's own region prefix, with the same production \
             predicate (`campaign_position_for`, `JoinAgreement::establishes`) proved on authored \
             values in the synthetic tests so CI runs it. The implementer applied seven mutations \
             to `crates/cs_content/src/campaign_bindings.rs` — dropping the short-row-block \
             refusal, suppressing the verbatim confirmation form, neutering the `establishes()` \
             guard, removing the campaign-length block filter, relaxing `is_verified`, shifting \
             the campaign position by one, and making `title_form`'s tail comparison a prefix \
             match — and every one was caught; the last was initially missed and closed by adding \
             `accept_m18_a_a_near_miss_title_is_never_confirmed`. The reviewing agent re-ran the \
             full check set on the rebased commit and repeated the mutation work independently: \
             `title_form`'s tail comparison made a prefix match, `campaign_position_for` made to \
             refuse nothing, `campaign_title_blocks` made to drop the campaign-length filter, \
             `JoinAgreement::establishes` made always true, and `is_verified` reduced to the \
             critical dependencies alone were each caught by the suite. A sixth mutation, dropping \
             only the `unknowns.is_empty()` condition from `is_verified`, was NOT caught — M18's \
             own record has no closure hash and no evidence claims either, so the three conditions \
             hid each other — and the reviewing agent closed it with \
             `accept_m18_a_a_verified_needs_every_condition_and_not_only_the_dependencies`, which \
             drops each of the four conditions on its own. The reviewer also re-derived M18's \
             retail facts from the installation without the binding code (title row 3497, long-name \
             row 3467, region-prefix row 1223, blocks 3450..3473 and 3480..3503, chapter sizes \
             [5,5,5,5,4], `ZBD/C4/M03/zrdr.zbd` 0eff1e94…) and confirmed them, repaired a stray \
             blank line inside this module's doc comment and the three rebase conflicts against \
             main's M17-A, and re-ran the suite after the rebase, which touched three of this \
             stage's files, so the full check set was re-run rather than the lighter one the owner \
             directive of 2026-10-01 allows when it does not. A second rebase brought in \
             M16-A-FU1's title-span fix, which had re-pinned every binding that landed after that \
             branch was cut and so made missions/bindings/M18.json stale (it still cited the \
             enclosing RT_STRING block); the record-pinning test failed on the rebased tree, the \
             reviewing agent re-derived it from SourceBinding::to_json (95792 + 60, the \
             confirmed row's own 29 UTF-16 code units) and added an assertion that the cited \
             bytes carry M18's row and no other row of either campaign-length block — \
             containment alone proves nothing, because the block carries this row's text too. \
             Claim is implemented only; validated with tools/validate_evidence.py --require-pass. \
             `candidate_tree` is the tree of the commit the suite ran on: the only later delta is \
             this report's own copy under docs/findings/evidence/ and the Checks, Rebase and \
             Review sections of docs/findings/2026-10-02-m18-a-source-binding.md, which record the \
             run; no production code, test or binding record changed after it"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M18-A\"",
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

/// [`parse_suite`] with this task's test prefix.
fn parse_m18_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m18_a_")
}
