//! Evidence-report harness for task F50-E4: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests F50-E4's capabilities are judged on.
///
/// Like M16-A-FU1's, this follow-up has no synthetic predicate of its own: its
/// whole point is a second reading of the installation, so every test it adds
/// needs `retail`.
const RETAIL_TESTS_F50_E4: &[&str] = &[
    "accept_f50_e4_the_row_runs_are_exactly_the_ones_an_independent_reading_finds",
    "accept_f50_e4_the_confirmed_rows_are_exactly_the_exact_byte_matches",
    "accept_f50_e4_a_fuzzy_matcher_would_confirm_a_near_miss_this_table_refuses",
];

/// Evidence-report harness for task F50-E4, the retail comparison test for the
/// row-geometry and title-exactness rules of `cs_content::campaign_bindings`
/// (work order `F50-E4`, raised by the M18-A review of Rally #309). It follows
/// the sequence in this module's doc with `F50-E4` and `accept_f50_e4_` in place
/// of `M01-A` and `accept_m01_a_`, and differs from the M01-A report in four
/// ways:
///
/// * the acceptance-log parser selects `accept_f50_e4_` tests, so the recorded
///   assertions are this task's own;
/// * `CS_EVIDENCE_REVIEWER` names the agent running the harness, so the report can
///   never carry a hand-over placeholder for the reviewer: the identity is read
///   at run time and the implementing and reviewing agents each write their own;
/// * the artifact beside the report is a **second production observation** over
///   the installation — `row-geometry.json` records what
///   `SourceContext::read`, `SourceContext::campaign_title_blocks` and
///   `SourceContext::confirm_title` actually return for this installation, so the
///   report cites measurements rather than a paraphrase of the assertions;
/// * the report records what F50-E4 measures and what it does not: the campaign
///   length, the chapter sizes, both campaign-length row blocks and the
///   confirmation of every declared title are re-derived from `$CS_GAME_DIR`
///   here, while the *rules* those readings are held to remain unverified
///   original semantics — the title-to-directory join is still an inference and
///   no original executable was run.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived from
/// the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_f50_e4_writes_the_acceptance_report() {
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
    let suite = parse_f50_e4_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_f50_e4_` tests were recorded in {}",
        log_path.display()
    );
    for retail_test in RETAIL_TESTS_F50_E4 {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: F50-E4 requires capability `retail`, run step 1 \
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

    // The second production observation: what the row-geometry and
    // title-confirmation rules actually return for this installation, recorded
    // rather than summarized.
    let context = SourceContext::read(&game_dir)
        .expect("production source context reads the original installation");
    let blocks: Vec<String> = context
        .campaign_title_blocks()
        .into_iter()
        .map(|block| {
            format!(
                "{{\"first_id\": {}, \"last_id\": {}, \"rows\": {}}}",
                block.first_id(),
                block.last_id(),
                block.len()
            )
        })
        .collect();
    let inventory = fs::read_to_string(
        Path::new(&git(&["rev-parse", "--show-toplevel"]))
            .join("missions/bindings/campaign-inventory.tsv"),
    )
    .expect("the declared campaign inventory reads");
    let confirmations: Vec<String> = inventory
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| line.split_once('\t'))
        .map(|(label, title)| {
            let confirmation = context.confirm_title(title.trim());
            let recorded = match confirmation.confirmed() {
                Some((row_id, form)) => format!(
                    "{{\"row_id\": {row_id}, \"form\": {}, \"refusal\": null}}",
                    jstr(match form {
                        cs_content::campaign_bindings::TitleForm::Verbatim => "verbatim",
                        cs_content::campaign_bindings::TitleForm::RegionPrefixedLongName => {
                            "region-prefixed long name"
                        }
                    })
                ),
                None => format!(
                    "{{\"row_id\": null, \"form\": null, \"refusal\": {}}}",
                    jstr(confirmation.refusal().unwrap_or("no reason recorded"))
                ),
            };
            format!(
                "{{\"work_order\": {}, \"title\": {}, \"confirmation\": {recorded}}}",
                jstr(label),
                jstr(title.trim())
            )
        })
        .collect();
    let chapters: Vec<String> = context
        .chapter_sizes()
        .iter()
        .map(|size| size.to_string())
        .collect();
    let geometry = format!(
        "{{\"install_sha256\": {}, \"candidate_tree\": {}, \"string_asset\": {}, \"campaign_length\": {}, \
         \"chapter_sizes\": [{}], \"campaign_length_blocks\": [{}], \"declared_title_confirmations\": [{}]}}\n",
        jstr(&install_sha256),
        jstr(&candidate_tree),
        jstr("GOSDATA/ASSETS/BINARIES/langui.dll"),
        context.campaign().len(),
        chapters.join(", "),
        blocks.join(", "),
        confirmations.join(", "),
    );
    let geometry_path = evidence_dir.join("row-geometry.json");
    fs::write(&geometry_path, &geometry)
        .unwrap_or_else(|error| panic!("write {}: {error}", geometry_path.display()));
    assert_eq!(
        context.campaign_title_blocks().len(),
        2,
        "the installation no longer offers exactly two campaign-length row blocks, so this \
         task's measurements no longer describe it"
    );
    let artifacts = vec![
        artifact(&log_path, "log", &evidence_dir),
        artifact(&geometry_path, "json", &evidence_dir),
    ];
    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"F50-E4\",\n\
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
        f50_e4_assertion_array(&suite.assertions),
        artifact_array(&artifacts),
        jstr(&reviewer),
        jstr(
            "acceptance suite run locally with the retail capability and regenerated by the agent \
             named above on the tree it reviewed; this harness derives every field from the \
             recorded log, production discovery of $CS_GAME_DIR, and a second production run of \
             SourceContext::read + campaign_title_blocks + confirm_title over the installation \
             (row-geometry.json). No production code changed at F50-E4: it adds the retail \
             comparison the M18-A review found missing (docs/findings/2026-10-02-m18-a-source-binding.md), \
             in which the row-geometry and title-exactness rules were proved only on authored values \
             while mutating title_form to accept a tail that merely starts with the title left all \
             eight accept_m18_a_* retail tests green. What is measured here: the campaign length \
             (24) and its chapter sizes ([5,5,5,5,4]) re-derived from the ZBD directory layout, the \
             76 maximal runs of rows that carry display text, the two campaign-length runs \
             (3450..3473 and 3480..3503), the 48 rows' own byte ranges in langui.dll, and the \
             confirmation of 219 authored near-miss titles against an independent per-row reading of \
             the same table - 27 confirmed verbatim, 19 through a long name, 2 ambiguous, 171 \
             uncarried, of which 27 a prefix matcher, 72 a substring matcher and 48 a \
             case-insensitive matcher would each have confirmed. Twelve mutations of \
             campaign_bindings.rs (title_form starts-with/case/trim/removed, confirm_title's arm \
             order, campaign_title_blocks' length filter and first-block truncation, \
             present_string_ids' emptiness test in both directions, title_blocks across gaps) each \
             fail at least one of the three tests; the table and the numbers are in \
             docs/findings/2026-10-03-f50-e4-row-geometry-and-title-exactness.md. NOT CLAIMED: the \
             row geometry and the title comparison are measured facts about this installation's \
             string table, not original-game rules; the title-to-directory join stays an inference, \
             no original executable was run, and nothing here is verified_original. Claim is \
             implemented only; validated with tools/validate_evidence.py --require-pass. \
             `candidate_tree` is the tree of the commit the suite and this harness ran on; the only \
             later delta is this report's own copy under docs/findings/evidence/ (whose bytes are \
             this file) and the findings document that discusses it, neither of which the \
             acceptance suite reads"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"F50-E4\"",
        "\"claim\": \"implemented\"",
        "\"capabilities\": [\"retail\"]",
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

/// The assertion list of this task's report. Every recorded test is cited
/// against both artifacts: the acceptance log says the test ran and passed, and
/// `row-geometry.json` is the production reading of the same installation the
/// tests compared against.
fn f50_e4_assertion_array(assertions: &[(String, &'static str)]) -> String {
    let items: Vec<String> = assertions
        .iter()
        .map(|(name, status)| {
            format!(
                "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\", \
                 \"row-geometry.json\"]}}",
                jstr(name)
            )
        })
        .collect();
    // The report's own `"assertions": [{}]` supplies the brackets, exactly as
    // `assertion_array` leaves them to be supplied.
    items.join(", ")
}

/// [`parse_suite_prefixed`] with this task's test prefix.
fn parse_f50_e4_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_f50_e4_")
}
