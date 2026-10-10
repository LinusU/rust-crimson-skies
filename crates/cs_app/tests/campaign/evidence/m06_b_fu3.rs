//! Evidence-report harness for task M06-B-FU3: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

use cs_app::mission_control::read_control_member;

/// The retail acceptance test M06-B-FU3's `retail` capability is judged on:
/// all four reconciled records re-read from the installation, every spelled
/// address resolved through the production rule, each record's own block
/// count spelled by one of its sites.
const RETAIL_TESTS_M06_B_FU3: &[&str] =
    &["accept_objaddr_every_reconciled_record_spells_its_own_block_count"];

/// The reconciled terminal-gate pins the same acceptance run carries: the
/// four suites' graph tests, whose assertions this task corrected (M02, M04)
/// or re-confirmed (M03, M06).
const RECONCILED_TESTS_M06_B_FU3: &[&str] = &[
    "accept_m02_b_the_objective_graph_the_sheet_priorities_need_is_measured_not_invented",
    "accept_m03_b_the_terminal_blocks_are_gated_and_every_address_is_in_range",
    "accept_m04_b_the_terminal_blocks_are_gated_and_every_address_is_in_range",
    "accept_m06_b_the_terminal_blocks_are_gated_and_every_address_is_in_range",
];

/// The synthetic predicate the rule itself is judged on: it runs in CI
/// without original data and carries both boundaries plus the refusals, so
/// the conversion cannot rot behind `#[ignore]`.
const SYNTHETIC_TESTS_M06_B_FU3: &[&str] =
    &["accept_objaddr_a_spelled_address_is_the_one_based_block_number"];

/// The four reconciled records, as `(census row, expected block count, the
/// site that spells the count)`.
const RECONCILED: &[(&str, u32, u32, &str)] = &[
    ("zbd/c1/m02", 50, 13, "WAKE_OBJECTIVE_WHEN_I_COMPLETE"),
    ("zbd/c1b/m03", 55, 21, "NAP_OBJECTIVE_WHEN_I_COMPLETE"),
    ("zbd/c1/m04", 52, 23, "WAKE_OBJECTIVE_WHEN_I_COMPLETE"),
    ("zbd/c2/m01", 82, 67, "WAKE_OBJECTIVE_WHEN_I_COMPLETE"),
];

/// Evidence-report harness for task M06-B-FU3, the follow-up that reconciled
/// the four campaign suites' readings of the same spelled integers onto the
/// one measured convention (Rally #819). It follows the sequence in this
/// module's doc with `M06-B-FU3` and `accept_objaddr_` in place of `M01-A`
/// and `accept_m01_a_`, and differs in two ways from the M02-B-FU3 report:
///
/// * the acceptance log holds **one** cargo invocation with five filters —
///   the `accept_objaddr_` prefix plus the four reconciled terminal-graph
///   tests — so the recorded assertions are the convention's own pin and the
///   four suites' terminal-gate pins it reconciles, in one run;
/// * the artifact beside the report is the **reconciliation record**
///   re-derived from the installation: for each of the four records the
///   block count, every spelled cross-objective address with its block and
///   key and the record index the production rule resolves it to, and the
///   boundary site that spells the count — ids, numbers and the rule's own
///   verdicts, never a byte of any document.
///
/// `review.identity` is read whole from `CS_EVIDENCE_REVIEWER` (the runtime
/// shape the reader above documents), so the report names whoever actually
/// ran the harness and can never carry a hand-over placeholder.
///
/// The report records what this task does *not* claim: the executable
/// evidence the convention rests on is #802's **static code reading** of the
/// owner's decrypted image, no original program was run, no mission was
/// played, and nothing is `verified_original`.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m06_b_fu3_writes_the_acceptance_report() {
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
    let suite = parse_m06_b_fu3_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_objaddr_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed: `retail` is declared only
    // because the retail member below is in this log and passed; `synthetic`
    // because the unignored arm of the rule did too. The four reconciled
    // terminal-graph tests must be in the same log and passing.
    for retail_test in RETAIL_TESTS_M06_B_FU3 {
        let status = recorded_status(&suite, retail_test);
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for reconciled_test in RECONCILED_TESTS_M06_B_FU3 {
        let status = recorded_status(&suite, reconciled_test);
        assert_eq!(
            status, "pass",
            "{reconciled_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M06_B_FU3 {
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

    // The reconciliation record itself, re-derived from the installation
    // beside the report and referenced by digest.
    let record_path = evidence_dir.join("m06-b-fu3-addresses.json");
    fs::write(
        &record_path,
        reconciliation_record(&game_dir, &install_sha256),
    )
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
         \x20\"task_id\": \"M06-B-FU3\",\n\
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
            "one acceptance invocation with five filters: the `accept_objaddr_` prefix (the \
             synthetic boundary/refusal pin over `resolve_objective_address` plus the retail \
             re-walk of all four reconciled records) and the four suites' terminal-graph tests \
             (`accept_m02_b_…objective_graph…`, `accept_m03_b_…terminal…`, \
             `accept_m04_b_…terminal…`, `accept_m06_b_…terminal…`), so the report records the \
             convention pin and every reconciled pin in one run; this harness derives every \
             field from the recorded log, production discovery of $CS_GAME_DIR and the decoded \
             control members that `objaddr.rs` re-reads through `read_control_member` \
             (m06-b-fu3-addresses.json: each record's block count, every spelled \
             cross-objective address with its block and key, and the record index the \
             production rule resolves it to). THE CONVENTION RESTS ON M02-B-FU3'S STATIC CODE \
             EVIDENCE FROM THE OWNER'S DECRYPTED EXECUTABLE (sha256 \
             43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75, no original \
             program run): the parse decrements every objective address into a zero-based \
             record index (0x468c40, 0x468cf0, 0x4679fc) while non-address integers are stored \
             unchanged (0x467a21), so a spelled address is the one-based block number and \
             resolves to `address - 1`. RECONCILED BY THIS TASK: M02's and M04's suites read \
             the integer as the index itself - the wrong-convention assertions were corrected, \
             never deleted (M02: OBJECTIVE13's spelled 50 is a real edge to OBJECTIVE50 and no \
             address is dangling; the INSTANTWIN latch is named by OBJECTIVE14's nap, the \
             INSTANTLOSS latches by block 3's kill plus block 19's nap and by block 7's nap. \
             M04: INSTANTWIN block 32 is named by block 31's nap, INSTANTLOSS block 41 by block \
             27's nap, and block 42's TICK_DEPENDS_ON_OBJ gates on block 40, not on the latch). \
             DISCRIMINATING RECORD FACTS: each of the four records spells an address equal to \
             its own block count (M02 OBJECTIVE13 -> 50, M03 OBJECTIVE21 -> 55, M04 OBJECTIVE23 \
             -> 52, M06 OBJECTIVE67 -> 82), in range only under the one-based reading, and in \
             M06 nothing spells 50 although block 50 naps 51. NOT CLAIMED: what the original \
             observes when a genuinely out-of-range stored index reaches its unchecked wake \
             walk is allocator-dependent and stays unknown (#802); WAKEUP_OBJECTIVE_WHEN_I_\
             COMPLETE's decrement is unmeasured and stays out of the address walk; no original \
             program was run, no mission was played and nothing is verified_original. Claim is \
             implemented only; validated with tools/validate_evidence.py --require-pass. \
             `candidate_tree` is the tree of the commit the suite ran on; the only later delta \
             is this report's own copy under docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M06-B-FU3\"",
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

/// Parses the recorded log: one `Suite` for the shared `test result:` counters
/// plus the assertions of every test this task's run selected — the
/// `accept_objaddr_` pair and the four reconciled terminal-graph tests.
fn parse_m06_b_fu3_suite(log: &str) -> Suite {
    let mut suite = parse_suite_prefixed(log, "accept_objaddr_");
    for prefix in [
        "accept_m02_b_the_objective_graph",
        "accept_m03_b_the_terminal",
        "accept_m04_b_the_terminal",
        "accept_m06_b_the_terminal",
    ] {
        let partial = parse_suite_prefixed(log, prefix);
        for (name, status) in partial.assertions {
            record(&mut suite, name, status);
        }
    }
    // `Suite`'s counters are global per summary line: every parse saw the same
    // `test result:` totals, so `suite`'s already cover all six tests.
    suite
}

/// The second production observation beside the report: each reconciled
/// record's cross-objective addresses, re-read from the installation through
/// `read_control_member` and `objaddr`'s walk — the same production code the
/// acceptance test uses — each resolved by the production rule.
///
/// Carries ids, spans, digests, counts, directive key names and the rule's
/// own verdicts — never a byte of any document and never display text.
fn reconciliation_record(game_dir: &Path, install_sha256: &str) -> String {
    let mut missions = Vec::new();
    for (mission, blocks, speller, key) in RECONCILED {
        let (document, member) = read_control_member(game_dir, mission)
            .unwrap_or_else(|| panic!("the measured rule finds {mission}'s control member"));
        assert_eq!(
            member.objective_blocks, *blocks,
            "{mission} declares {blocks} numbered blocks"
        );
        let sites = crate::objaddr::walk(&document);
        let mut addresses = Vec::new();
        for site in &sites {
            for address in &site.addresses {
                let resolved =
                    resolve_objective_address(i64::from(*address), member.objective_blocks);
                let (record_index, rule) = match &resolved {
                    Ok(symbol) => (symbol.0.to_string(), jstr("in_range")),
                    Err(_) => ("null".to_owned(), jstr(OUT_OF_RANGE_OBJECTIVE_ADDRESS)),
                };
                addresses.push(format!(
                    "{{\"block\": {}, \"key\": {}, \"address\": {}, \"record_index\": {}, \
                     \"rule\": {}}}",
                    site.block,
                    jstr(&site.key),
                    address,
                    record_index,
                    rule
                ));
            }
        }
        let boundary = sites
            .iter()
            .find(|site| {
                site.block == *speller && site.key == *key && site.addresses.contains(blocks)
            })
            .unwrap_or_else(|| {
                panic!("{mission} OBJECTIVE{speller} spells {blocks} through {key}")
            });
        missions.push(format!(
            "{{\n    \"mission\": {},\n    \"member\": {},\n    \"blocks\": {},\n    \
             \"sites\": {},\n    \"boundary_site\": {{\"block\": {}, \"key\": {}, \
             \"addresses\": [{}]}},\n    \"addresses\": [\n      {}\n    ]\n  }}",
            jstr(mission),
            jstr(&member.name),
            member.objective_blocks,
            sites.len(),
            boundary.block,
            jstr(&boundary.key),
            boundary
                .addresses
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            addresses.join(",\n      ")
        ));
    }
    format!(
        "{{\n\
         \x20\"task_id\": \"M06-B-FU3\",\n\
         \x20\"install_sha256\": {},\n\
         \x20\"missions\": [\n  {}\n ]\n\
         }}\n",
        jstr(install_sha256),
        missions.join(",\n  ")
    )
}
