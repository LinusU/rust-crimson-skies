//! Evidence-report harness for task M02-T3: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M02-T3's capabilities are judged on.
const RETAIL_TESTS_M02_T3: &[&str] = &[
    "accept_m02_b_the_two_campaign_length_blocks_correspond_row_to_row",
    "accept_m02_b_a_plain_normalized_comparison_would_not_correspond",
    "accept_m02_b_the_join_agreement_folds_in_the_correspondence",
];

/// The synthetic predicate tests M02-T3's report must also record.
const SYNTHETIC_TESTS_M02_T3: &[&str] = &[
    "accept_m02_b_a_block_pair_corresponds_only_through_its_own_rows",
    "accept_m02_b_the_rule_ignores_case_and_articles",
    "accept_m02_b_a_corroboration_disagreement_is_a_refusal",
];

/// Evidence-report harness for task M02-T3, the follow-up that corroborates the
/// title-to-campaign join with the row-to-row correspondence of the two
/// campaign-length localized blocks (Rally #450). It follows the sequence in
/// this module's doc with `M02-T3` and `accept_m02_b_` in place of `M01-A` and
/// `accept_m01_a_`, and differs in two ways from the M02-A report above:
///
/// * the acceptance-log parser selects `accept_m02_b_` tests, so the recorded
///   assertions are this follow-up's own — the three retail measurements of the
///   correspondence and the three synthetic predicate tests of the rule and of
///   the refusal it feeds;
/// * the artifact beside the report is a **correspondence record** derived from
///   the production `SourceContext::join_agreement` and `blocks_correspond`:
///   the two block boundaries, the chapter sizes, the region groups, the
///   measured `state` and the measured pair. It carries ids, counts and
///   booleans only, never original text.
///
/// Everything else — the toolchain versions, the installation hashes, the
/// candidate tree, the counts, the digests and the timestamps — is derived
/// from the same real inputs as the M01-A report.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
fn evidence_report_m02_t3_writes_the_acceptance_report() {
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
    let suite = parse_m02_t3_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m02_b_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed: `retail` is declared only
    // because every retail acceptance test below is in this log and passed;
    // `synthetic` because the three unignored predicate tests did too.
    for retail_test in RETAIL_TESTS_M02_T3 {
        let status = recorded_status(&suite, retail_test);
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M02_T3 {
        let status = recorded_status(&suite, synthetic_test);
        assert_eq!(
            status, "pass",
            "{synthetic_test} must pass; got status {status}"
        );
    }

    let found = discover(&game_dir)
        .expect("production discovery must read the original installation for the evidence record");
    let install_sha256 = fingerprint(&found.manifest).to_hex();
    let content_sha256 = content_fingerprint(&found.manifest).to_hex();

    // The production join agreement and the measured correspondence, written
    // beside the report and referenced by digest.
    let context = SourceContext::read(&game_dir).expect(
        "production source context reads the original installation for the evidence record",
    );
    let agreement = context.join_agreement();
    assert_eq!(
        agreement.state,
        JoinCorroboration::Agreed,
        "the acceptance run passed while the localized table disagrees with the layout; the \
         report must not be written"
    );
    let record_path = evidence_dir.join("m02-t3-correspondence.json");
    fs::write(&record_path, correspondence_record(&context, &agreement))
        .unwrap_or_else(|error| panic!("write {}: {error}", record_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&record_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M02-T3\",\n\
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
            "implementer: deepseek-1/deepseek-1 (Rally #450, DeepSeek V4.1 Flash, session of \
             2026-10-02T14:33Z); reviewer: deepseek-1/deepseek-1 again, as the Rally reviewing \
             agent on the review claim (2026-10-02T15:07Z). Same agent instance and model, so \
             this is NOT independent review and is not independent original-reference evidence; \
             the reviewer's context was fresh (a new session that re-read the tree, the task \
             history and the installation) but a fresh context does not make a reviewer \
             independent. No agent review replaces the owner's human approval"
        ),
        jstr(
            "acceptance suite re-run locally with the retail and synthetic capabilities by the \
             reviewer on the reviewed commit, and the reviewer regenerated this report from that \
             run; this harness derives every field from the recorded log, production \
             discovery of $CS_GAME_DIR and the join `SourceContext::read` + \
             `SourceContext::join_agreement` + `blocks_correspond` derive from it. Production \
             code changed: the join is now checked a third time, by the row-to-row \
             correspondence of the two campaign-length localized blocks (a mutual strict argmax \
             of shared content tokens), and a pair that does not correspond is a \
             JoinCorroboration::Disagreed rather than a warning. The three retail tests measure \
             the correspondence on the installation, the 16 rows a plain normalized equality \
             would reject and the 23 rotations the rule refuses; the three synthetic tests \
             prove every arm of the rule, of the classifier and of the refusal. The companion \
             artifact is the agreement's own account (block boundaries, chapter sizes, region \
             groups and the measured pair), with no original text. The join stays an inference: \
             no region name is bound to a chapter and nothing is verified_original. Claim is \
             implemented only; validated with tools/validate_evidence.py --require-pass. \
             `candidate_tree` is the tree of the commit the suite ran on: the only later delta \
             is this report's own copy under docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M02-T3\"",
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
fn parse_m02_t3_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m02_b_")
}

/// `Agreed`, `Disagreed` or `Unavailable` as the report spells it.
fn corroboration_name(state: JoinCorroboration) -> &'static str {
    match state {
        JoinCorroboration::Unavailable => "Unavailable",
        JoinCorroboration::Agreed => "Agreed",
        JoinCorroboration::Disagreed => "Disagreed",
    }
}

/// The display text of every row of `block`, with a leading `[tag]` dropped.
/// The harness re-reads the text so the record it writes is the agreement's own
/// account, not a constant typed into this file.
fn block_display_texts(context: &SourceContext, block: TitleBlock) -> Vec<String> {
    (block.first_id()..=block.last_id())
        .map(|id| {
            let row = context
                .string_rows()
                .iter()
                .find(|row| row.id == id)
                .unwrap_or_else(|| panic!("row {id} of block {block} is missing"));
            let text = row
                .text
                .as_deref()
                .unwrap_or_else(|| panic!("row {id} does not decode"));
            let display = match text.strip_prefix('[') {
                Some(rest) => match rest.find(']') {
                    Some(end) => &text[end + 2..],
                    None => text,
                },
                None => text,
            };
            assert!(!display.is_empty(), "row {id} carries no display text");
            display.to_owned()
        })
        .collect()
}

/// The join agreement as a JSON record: block boundaries, chapter sizes, region
/// groups, the measured state and the measured pair, never original text.
fn correspondence_record(context: &SourceContext, agreement: &JoinAgreement) -> String {
    let blocks: Vec<String> = agreement
        .blocks
        .iter()
        .map(|block| {
            format!(
                "{{\"first_id\": {}, \"last_id\": {}, \"len\": {}}}",
                block.first_id(),
                block.last_id(),
                block.len()
            )
        })
        .collect();
    let grouped: Vec<String> = agreement
        .grouped
        .iter()
        .map(|entry| {
            format!(
                "{{\"block\": \"{}\", \"groups\": [{}]}}",
                entry.block,
                numbers(&entry.groups)
            )
        })
        .collect();
    let texts: Vec<Vec<String>> = agreement
        .blocks
        .iter()
        .map(|block| block_display_texts(context, *block))
        .collect();
    let mut correspondences = Vec::new();
    for (index, left) in texts.iter().enumerate() {
        for right in texts.iter().skip(index + 1) {
            let left: Vec<&str> = left.iter().map(String::as_str).collect();
            let right: Vec<&str> = right.iter().map(String::as_str).collect();
            correspondences.push(blocks_correspond(&left, &right).to_string());
        }
    }
    format!(
        "{{\n\
         \x20\"task_id\": \"M02-T3\",\n\
         \x20\"install_sha256\": {},\n\
         \x20\"state\": \"{}\",\n\
         \x20\"layout_chapters\": [{}],\n\
         \x20\"blocks\": [{}],\n\
         \x20\"grouped\": [{}],\n\
         \x20\"correspondences\": [{}]\n\
         }}\n",
        jstr(context.install_sha256()),
        corroboration_name(agreement.state),
        numbers(&agreement.layout_chapters),
        blocks.join(", "),
        grouped.join(", "),
        correspondences.join(", "),
    )
}
