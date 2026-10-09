//! Evidence-report harness for task M17-A: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M17-A's capabilities are judged on.
const RETAIL_TESTS_M17_A: &[&str] = &[
    "accept_m17_a_source_derived_binding_has_no_unresolved_critical_dependencies",
    "accept_m17_a_the_committed_record_is_what_the_installation_derives",
    "accept_m17_a_the_original_name_is_confirmed_against_the_local_strings",
    "accept_m17_a_only_the_short_name_row_carries_the_declared_title",
    "accept_m17_a_the_join_is_corroborated_by_the_long_name_rows",
    "accept_m17_a_the_position_is_interior_to_the_fourth_chapter_and_its_region_group",
    "accept_m17_a_the_program_archive_alone_singles_this_mission_out_of_its_chapter",
    "accept_m17_a_the_campaign_keeps_everything_else_unresolved_and_unready",
];

/// The synthetic predicate tests M17-A's report must also record.
const SYNTHETIC_TESTS_M17_A: &[&str] = &[
    "accept_m17_a_a_title_block_must_be_exactly_the_campaign_length",
    "accept_m17_a_a_contradicted_corroboration_establishes_no_position",
];

/// Evidence-report harness for task M17-A, the seventeenth mission's source
/// binding. It follows the sequence in this module's doc with `M17-A` and
/// `accept_m17_a_` in place of `M01-A` and `accept_m01_a_`, and differs in
/// three ways from the M01-A report above:
///
/// * the acceptance-log parser selects `accept_m17_a_` tests, so the recorded
///   assertions are this task's own;
/// * the artifact beside the report is the **M17** binding, derived from the
///   committed inventory's declared title, so the report cites the mission it
///   is evidence for;
/// * the report records what M17-A pins, because none of it is new machinery:
///   the declared title is carried by the installation's bare short-name row
///   and by no long name (the long name inserts `Nathan Zachary &` between the
///   region prefix and the title), so this record carries no `title spelling`
///   unknown, and the seventeenth campaign position is the *second* mission of
///   chapter 4 — one row past the boundary M16 measured.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m17_a_writes_the_acceptance_report() {
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
    let suite = parse_m17_a_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m17_a_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M17_A {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M17-A requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M17_A {
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

    let source_binding = source_binding_for(&game_dir, "M17");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m17-binding.json");
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
         \x20\"task_id\": \"M17-A\",\n\
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
            "implementer: bunny-alpha-1 (Rally #306, OpenCode Space Bunny Alpha, session of \
             2026-10-02T01:25Z); reviewing agent: bunny-alpha-1 (Rally #306 review claim, session \
             of 2026-10-02T01:58Z, fresh context that did not do the implementation and \
             re-derived the retail facts from the installation independently of the binding code). \
             Same agent name as the implementer, so this is not an independent review and not \
             independent original-reference evidence; no agent review replaces the owner's human \
             approval"
        ),
        jstr(
            "acceptance suite run locally with the retail capability by the implementer and re-run \
             by the reviewing agent; this harness derives every field from the recorded log, \
             production discovery of $CS_GAME_DIR and the binding `SourceContext::read` + \
             `SourceContext::bind` derive from it (all five critical dependencies resolved; \
             checklist entries still unknown are recorded in missions/bindings/M17.json, not \
             dropped). No production code changed at M17-A: the join, its corroboration and the \
             guard are M02-A's, and this stage exercises them at the seventeenth campaign \
             position. What it pins is that the declared title is carried by the bare short-name \
             row only (the long name inserts 'Nathan Zachary &' between the region prefix and the \
             title, so this record carries no 'title spelling' unknown, the opposite of M05-A, \
             whose declared title only the region-prefixed long-name row carries; M12-A and M16-A \
             also carry none, but because their titles are carried verbatim *and* as a long-name \
             tail), and that position 16 is the second mission of chapter 4 — one row past the \
             boundary M16 measured — whose world group world/c4 holds all five chapter-4 missions \
             while the mission number 2 names one mission in every chapter, leaving the program \
             archive the only per-mission discriminator. See \
             docs/findings/2026-10-02-m17-a-source-binding.md, which also records the measured \
             fact that seven declared titles (M09, M11, M14, M15, M20, M22, M23) are carried by no \
             retail row in either display form. Claim is implemented only; validated with \
             tools/validate_evidence.py --require-pass. `candidate_tree` is the tree of the commit \
             the suite ran on: the only later delta is this report's own copy under \
             docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M17-A\"",
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
fn parse_m17_a_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m17_a_")
}
