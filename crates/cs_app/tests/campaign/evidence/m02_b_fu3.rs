//! Evidence-report harness for task M02-B-FU3: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance test M02-B-FU3's `retail` capability is judged on:
/// M02's own cross-objective addresses, re-read from the installation,
/// resolved through the production rule.
const RETAIL_TESTS_M02_B_FU3: &[&str] =
    &["accept_m02_b_fu3_m02s_wake_addresses_resolve_inside_its_block_count"];

/// The synthetic predicate tests the rule itself is judged on: they run in CI
/// without original data and carry the in-range boundary and the out-of-range
/// refusal, so the refusal arm cannot rot behind `#[ignore]`.
const SYNTHETIC_TESTS_M02_B_FU3: &[&str] = &[
    "accept_m02_b_fu3_an_address_inside_the_record_resolves_to_its_one_based_record_index",
    "accept_m02_b_fu3_an_address_past_the_block_count_is_refused_never_clamped",
];

/// Evidence-report harness for task M02-B-FU3, the follow-up that measured
/// what the original does with a cross-objective address past the record's
/// block count and decided the engine rule for it (Rally #802). It follows the
/// sequence in this module's doc with `M02-B-FU3` and `accept_m02_b_fu3_` in
/// place of `M01-A` and `accept_m01_a_`, and differs in two ways from the
/// M02-B report above:
///
/// * the acceptance-log parser selects `accept_m02_b_fu3_` — a prefix no other
///   suite shares, so the recorded assertions are exactly this task's three
///   tests: the retail measurement over M02's record and the two synthetic
///   arms of the rule (`cs_sim`'s suite and `campaign`'s retail member are
///   separate binaries, and one log carries both);
/// * the artifact beside the report is the **address record** re-derived from
///   the installation: the control binding's identities, spans and digests
///   plus every cross-objective address M02 spells with the block, the key,
///   the record index the rule resolves it to and the rule's verdict — ids,
///   numbers and the rule's own name, never original text and never a byte of
///   the document.
///
/// `review.identity` is read whole from `CS_EVIDENCE_REVIEWER` (the runtime
/// shape the reader above documents), so the report names whoever actually
/// ran the harness and can never carry a hand-over placeholder.
///
/// The report records what this task does *not* claim: the original's own
/// behaviour for an address that reaches its wake walk past the count is a
/// **static code reading** with an allocator-dependent outcome left unknown,
/// no original program was run, no mission was played, the wake/sleep/kill
/// directive family still has no consumer in this build, and nothing is
/// `verified_original`.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m02_b_fu3_writes_the_acceptance_report() {
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
    let suite = parse_m02_b_fu3_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m02_b_fu3_` tests were recorded in {}",
        log_path.display()
    );

    // Capability coverage is checked, never assumed: `retail` is declared only
    // because the retail member below is in this log and passed; `synthetic`
    // because the two unignored arms of the rule did too.
    for retail_test in RETAIL_TESTS_M02_B_FU3 {
        let status = recorded_status(&suite, retail_test);
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M02_B_FU3 {
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

    // The address record itself, re-derived from the installation beside the
    // report and referenced by digest.
    let record_path = evidence_dir.join("m02-b-fu3-addresses.json");
    fs::write(&record_path, address_record(&install_sha256))
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
         \x20\"task_id\": \"M02-B-FU3\",\n\
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
            "acceptance suite run locally with the retail and synthetic capabilities; this \
             harness derives every field from the recorded log, production discovery of \
             $CS_GAME_DIR and the control binding plus the decoded control member that \
             `m02_b_fu3.rs` re-reads from the archive (m02-b-fu3-addresses.json: every \
             cross-objective address M02 spells, the record index the production rule resolves \
             it to and the rule's verdict). MEASURED FROM THE OWNER'S DECRYPTED EXECUTABLE \
             (static code evidence, sha256 43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75, \
             no original program run): the parse decrements every objective address into a \
             zero-based record index (0x468c40, 0x468cf0, 0x4679fc) while non-address integers \
             are stored unchanged (0x467a21), the record holds one 0x5e4-byte record per numbered \
             block with the count at +0xc48 (0x467956, 0x469043), and the wake walk 0x469af0 \
             checks no address at all - no clamp, no ignore, no log. NOT CLAIMED: what the \
             original *observes* when an address past the count reaches that walk depends on the \
             memory after the array and stays unknown; M02's own address 50 resolves inside its \
             50 blocks as record 49; the wake/sleep/kill directive family still has no consumer \
             in this build, so the rule is the objective lifecycle's resolver and refusal for \
             that executor to use; no mission was played and nothing is verified_original. Claim \
             is implemented only; validated with tools/validate_evidence.py --require-pass. \
             `candidate_tree` is the tree of the commit the suite ran on; the only later delta \
             is this report's own copy under docs/findings/evidence/, whose bytes are this file"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M02-B-FU3\"",
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

/// [`parse_suite_prefixed`] with this task's own prefix. The prefix is unique
/// to M02-B-FU3: `cs_sim`'s two synthetic arms and `campaign`'s one retail
/// member both carry it, and no other suite's test names do.
fn parse_m02_b_fu3_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m02_b_fu3_")
}

/// The second production observation beside the report: M02's cross-objective
/// addresses, re-read from the installation through the same production
/// binding and decode the acceptance test uses, each resolved by the
/// production rule.
///
/// Carries ids, byte spans, digests, counts, directive key names and the
/// rule's own verdict — never a byte of the document and never display text.
fn address_record(install_sha256: &str) -> String {
    let binding = crate::m02_b_fu3::control_binding();
    let document = crate::m02_b_fu3::control_document();
    let blocks = crate::m02_b_fu3::blocks_with_addresses(&document);
    let count = blocks.len() as u32;
    assert_eq!(
        count,
        binding.record.blocks(),
        "the address record and the measured record see the same block count"
    );
    let mut entries = Vec::new();
    for block in &blocks {
        for (key, args) in &block.addresses {
            for address in args {
                let resolved = resolve_objective_address(i64::from(*address), count);
                let (record_index, rule) = match &resolved {
                    Ok(symbol) => (symbol.0.to_string(), jstr("in_range")),
                    Err(_) => ("null".to_owned(), jstr(OUT_OF_RANGE_OBJECTIVE_ADDRESS)),
                };
                entries.push(format!(
                    "{{\"block\": {}, \"key\": {}, \"address\": {}, \"record_index\": {}, \
                     \"rule\": {}}}",
                    block.number,
                    jstr(key),
                    address,
                    record_index,
                    rule
                ));
            }
        }
    }
    format!(
        "{{\n\
         \x20\"task_id\": \"M02-B-FU3\",\n\
         \x20\"install_sha256\": {},\n\
         \x20\"mission\": {},\n\
         \x20\"program\": {},\n\
         \x20\"container\": {},\n\
         \x20\"container_length\": {},\n\
         \x20\"container_sha256\": {},\n\
         \x20\"member\": {},\n\
         \x20\"member_offset\": {},\n\
         \x20\"member_length\": {},\n\
         \x20\"member_sha256\": {},\n\
         \x20\"blocks\": {},\n\
         \x20\"addresses\": [{}]\n\
         }}\n",
        jstr(install_sha256),
        jstr(binding.mission.as_str()),
        jstr(binding.program_id.as_str()),
        jstr(&binding.program_asset),
        binding.program_length,
        jstr(&binding.program_sha256),
        jstr(&binding.control_member),
        binding.control_offset,
        binding.control_length,
        jstr(&binding.control_sha256),
        count,
        entries.join(",\n    ")
    )
}
