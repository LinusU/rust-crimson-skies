//! Evidence-report harness for task M16-A-FU1: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M16-A-FU1's capabilities are judged on.
///
/// This follow-up has no synthetic predicate of its own: the whole point is a
/// byte-range measurement against the installation, so every test it adds
/// needs `retail`.
const RETAIL_TESTS_M16_A_FU1: &[&str] = &[
    "accept_m16_a_fu1_the_title_span_is_the_matched_rows_own_bytes",
    "accept_m16_a_fu1_the_enclosure_is_kept_distinct_from_the_cited_span",
];

/// Evidence-report harness for task M16-A-FU1, the follow-up that makes the
/// mission-binding title source span the matched row's own bytes rather than
/// the `RT_STRING` block. It follows the sequence in this module's doc with
/// `M16-A-FU1` and `accept_m16_a_fu1_` in place of `M01-A` and
/// `accept_m01_a_`:
///
/// * the acceptance-log parser selects `accept_m16_a_fu1_` tests, so the
///   recorded assertions are this follow-up's own;
/// * the artifact beside the report is the **M16** binding, derived from the
///   committed inventory's declared title, whose first langui span is now the
///   confirmed row's own bytes;
/// * the report records what this task changes: the span is measured from the
///   decoded `RT_STRING` units, the enclosing block is kept as
///   `title_enclosure`, and the corrected measurement of the row the join
///   actually matched is recorded in
///   `docs/findings/2026-10-02-m16-a-fu1-title-row-span.md`.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m16_a_fu1_writes_the_acceptance_report() {
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
    let suite = parse_m16_a_fu1_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m16_a_fu1_` tests were recorded in {}",
        log_path.display()
    );

    for retail_test in RETAIL_TESTS_M16_A_FU1 {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M16-A-FU1 requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    let source_binding = source_binding_for(&game_dir, "M16");
    assert!(
        source_binding.unresolved_critical().is_empty(),
        "the acceptance run passed while a critical dependency is unresolved; the report must \
         not be written"
    );
    let binding_path = evidence_dir.join("m16-a-fu1-binding.json");
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
         \x20\"task_id\": \"M16-A-FU1\",\n\
         \x20\"candidate_tree\": {},\n\
         \x20\"engine\": {},\n\
         \x20\"created_at\": {},\n\
         \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
         \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
         \x20\"seed\": 0,\n\
         \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
         \x20\"overrides\": [],\n\
         \x20\"capabilities\": [\"retail\"],\n\
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
            "implementer: deepseek-1 (Rally #478, DeepSeek V4.1 Flash, session of \
             2026-10-02T01:12Z); reviewer: deepseek-1 (same agent and model, fresh session, \
             reviewed the rebased branch per Rally #478) — same-agent review is NOT independent \
             evidence and no agent review replaces the owner's human approval; review notes and \
             the rebase follow-up are recorded in \
             docs/findings/2026-10-02-m16-a-fu1-title-row-span.md"
        ),
        jstr(
            "acceptance suite run locally with the retail capability by the implementer and \
             regenerated by the reviewer on the rebased tree; this \
             harness derives every field from the recorded log, production discovery of \
             $CS_GAME_DIR and the M16 binding `SourceContext::read` + `SourceContext::bind` \
             derive from it (all five critical dependencies resolved; checklist entries still \
             unknown are recorded in missions/bindings/M16.json, not dropped). Production code \
             changed: the title's source span is measured from the decoded RT_STRING units \
             (2-byte length prefix plus 2 bytes per code unit, in block order) instead of the \
             whole block, the block is kept as the named `title_enclosure`, and the M16 span is \
             now 95676 + 66 bytes (row 3495, `Raid on the Rocky Express`) rather than 95304 + 876 \
             (block 219, the short names of M09..M24). The new tests re-measure the row from the \
             decoded units, decode the cited bytes back to the row's code units, and prove no \
             other campaign mission title overlaps the cited range; they fail when the span fix \
             is reverted. The task description's claim that the join matched the region-prefixed \
             long name `Rocky Mountains - Raid on the Rocky Express` is corrected by measurement \
             — the verbatim short row 3495 wins the confirmation, and the long-name sibling 3465 \
             is disjoint from the cited span; see \
             docs/findings/2026-10-02-m16-a-fu1-title-row-span.md. Claim is implemented only; \
             validated with tools/validate_evidence.py --require-pass. `candidate_tree` is the \
             tree of the commit the suite ran on: the only later delta is this report's own copy \
             under docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M16-A-FU1\"",
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
fn parse_m16_a_fu1_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m16_a_fu1_")
}
