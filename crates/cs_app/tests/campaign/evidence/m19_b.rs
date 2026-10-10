//! Evidence-report harness for task M19-B: the task's own test
//! lists and its `evidence_report_*` harness. Shared helpers,
//! types and imports live in `super` — see
//! `crates/cs_app/tests/campaign/evidence.rs`, which is also where
//! the module doc says where a new task's evidence goes.

use super::*;

/// The retail acceptance tests M19-B's `retail` capability is judged on.
const RETAIL_TESTS_M19_B: &[&str] = &[
    "accept_m19_b_the_control_program_is_the_member_that_declares_the_blocks",
    "accept_m19_b_every_directive_m19_spells_has_a_disposition_and_none_is_refused",
    "accept_m19_b_the_sheet_priorities_resolve_to_measured_operations_and_none_transfers",
    "accept_m19_b_the_hookup_transfer_and_the_extraction_are_record_data",
    "accept_m19_b_the_unlock_gates_and_the_watchers_are_record_data",
    "accept_m19_b_the_failure_watchers_share_one_latch_and_the_kill_lists_match",
    "accept_m19_b_every_actor_the_record_names_resolves_in_the_shipped_data",
    "accept_m19_b_the_terminal_blocks_are_gated_and_every_address_is_in_range",
    "accept_m19_b_the_two_outcomes_have_disjoint_prerequisites_and_the_rest_is_not_mandatory",
    "accept_m19_b_every_call_binds_every_condition_lowers_and_m19s_record_is_the_largest_complete",
    "accept_m19_b_m19_is_complete_and_the_campaign_stays_unready",
];

/// The synthetic predicate tests M19-B's report must also record: they carry
/// the boundary and refusal arms the retail record leans on — the
/// first-`ANIM_STATE`-wins parse, the spelled-`0` dependency sentinel and the
/// unmeasured transfer key refusing — into CI, where there is no original
/// data.
const SYNTHETIC_TESTS_M19_B: &[&str] = &[
    "accept_m19_b_the_hookup_anim_state_lowers_and_a_second_site_is_inert",
    "accept_m19_b_a_zero_dependency_arms_no_gate_and_the_wildcards_arrive_as_data",
    "accept_m19_b_an_ungrounded_transfer_directive_is_refused_rather_than_honoured",
];

/// Evidence-report harness for task M19-B: *Rescue the Black Swan*'s
/// mission-specific compatibility surface. Same sequence as the M13-B and
/// M16-B reports: it records the `accept_m19_b_` tests, writes M19's
/// control-program binding, its complete lowering and the measured latch and
/// closure graph beside the report as a second production observation, and
/// claims `implemented` only.
///
/// `CS_EVIDENCE_REVIEWER` fills `review.identity` whole, so whoever runs the
/// harness — the implementing agent at hand-over or the reviewing agent on the
/// rebased commit — writes its own identities and says whether the run is a
/// review. A placeholder identity is a failure.
#[test]
#[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
fn evidence_report_m19_b_writes_the_acceptance_report() {
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
    let suite = parse_m19_b_suite(&log);
    assert!(
        suite.passed > 0 && !suite.assertions.is_empty(),
        "no `accept_m19_b_` tests were recorded in {}",
        log_path.display()
    );
    for retail_test in RETAIL_TESTS_M19_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == retail_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{retail_test} did not run: M19-B requires capability `retail`, run step 1 \
                     with `--include-ignored` and CS_GAME_DIR set"
                )
            });
        assert_eq!(
            status, "pass",
            "{retail_test} must pass; got status {status}"
        );
    }
    for synthetic_test in SYNTHETIC_TESTS_M19_B {
        let status = suite
            .assertions
            .iter()
            .find(|(name, _)| name == synthetic_test)
            .map(|(_, status)| *status)
            .unwrap_or_else(|| {
                panic!(
                    "{synthetic_test} did not run: it pins the refusal arms the retail record \
                     leans on"
                )
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

    // The control-program binding, its lowering and the measured graph,
    // written beside the report and referenced by digest: identities, spans,
    // digests, member accounting, directive counts and the latch/closure
    // structure — never original bytes or display text.
    let title = declared_title("M19");
    let context =
        SourceContext::read(&game_dir).expect("production source context reads the installation");
    let control = context
        .control_program(
            MissionLabel::new("M19").expect("M19 is a valid label"),
            &title,
        )
        .expect("M19's control program binds through the measured rule");
    assert_eq!(
        context.install_sha256(),
        install_sha256,
        "the control binding was derived under a different installation fingerprint than discovery reports"
    );

    let census = cs_app::mission_control::survey_mission_control_programs(&game_dir)
        .expect("the census measures the installation");
    let row = census
        .row("zbd/c4/m04")
        .expect("M19's reader archive is measured by the census");
    let lowered = row
        .lowering_attempt()
        .expect("the census lowers M19's measured record");
    let attempt = lowered.attempt();
    let refused_calls = attempt
        .calls
        .iter()
        .filter(|outcome| {
            matches!(
                outcome,
                cs_content::mission_control::CallOutcome::Refused(_)
            )
        })
        .count();
    let refused_conditions = attempt
        .conditions
        .iter()
        .filter(|outcome| {
            matches!(
                outcome,
                cs_content::mission_control::ConditionOutcome::Refused(_)
            )
        })
        .count();
    let lowering = control.record.lowering(attempt);
    let unmet: Vec<String> = lowering
        .unmet()
        .map(|row| row.kind.code().to_owned())
        .collect();

    // The measured graph, re-derived from the control member the census
    // chose: the two latches, their completion-edge predecessors and the
    // prerequisite closures the acceptance suite asserts.
    let (document, _) = cs_app::mission_control::read_control_member(&game_dir, "zbd/c4/m04")
        .expect("the rule finds M19's control member again");
    let graph = M19Graph::measure(&document);

    let members: Vec<String> = row
        .members
        .iter()
        .map(|row| {
            format!(
                "{{\"name\": {}, \"offset\": {}, \"length\": {}, \"objective_blocks\": {}, \"is_control\": {}}}",
                jstr(&row.name),
                row.offset,
                row.len,
                row.objective_blocks,
                row.is_control
            )
        })
        .collect();
    let implemented: Vec<String> = control
        .record
        .implemented()
        .into_iter()
        .map(|(key, outcome)| {
            format!(
                "{{\"key\": {}, \"outcome\": {}}}",
                jstr(&key.key),
                jstr(outcome.label())
            )
        })
        .collect();
    let control_path = evidence_dir.join("m19-control-program.json");
    let control_json = format!(
        "{{\n\
         \x20\"task_id\": \"M19-B\",\n\
         \x20\"install_sha256\": {},\n\
         \x20\"mission\": {},\n\
         \x20\"program_id\": {},\n\
         \x20\"program_asset\": {},\n\
         \x20\"program_length\": {},\n\
         \x20\"program_sha256\": {},\n\
         \x20\"control_member\": {},\n\
         \x20\"control_offset\": {},\n\
         \x20\"control_length\": {},\n\
         \x20\"control_sha256\": {},\n\
         \x20\"members\": [{}],\n\
         \x20\"record\": {{\"blocks\": {}, \"sites\": {}, \"vocabulary\": {}, \
         \"implemented\": [{}], \"measured\": {}, \"unmeasured\": [{}], \
         \"unclassified_record_keys\": [{}], \"refusals\": {}}},\n\
         \x20\"lowering\": {{\"mission\": {}, \"objectives\": {}, \"calls\": {}, \
         \"refused_calls\": {}, \"refused_conditions\": {}, \"unbound_keys\": [{}], \
         \"unmet\": [{}], \"validation_present\": {}, \"program_present\": {}}},\n\
         \x20\"graph\": {}\n\
         }}\n",
        jstr(install_sha256.as_str()),
        jstr(control.mission.as_str()),
        jstr(control.program_id.as_str()),
        jstr(&control.program_asset),
        control.program_length,
        jstr(&control.program_sha256),
        jstr(&control.control_member),
        control.control_offset,
        control.control_length,
        jstr(&control.control_sha256),
        members.join(", "),
        control.record.blocks(),
        control.record.sites(),
        control.record.vocabulary(),
        implemented.join(", "),
        control.record.measured().len(),
        str_array(&control.unmeasured_keys()),
        str_array(control.unclassified_record_keys()),
        control.record.refusals().len(),
        match &attempt.mission {
            Ok(mission) => jstr(mission),
            Err(reason) => jstr(reason),
        },
        attempt.objectives,
        attempt.calls.len(),
        refused_calls,
        refused_conditions,
        str_array(&attempt.unbound_keys),
        str_array(&unmet),
        attempt.validation.is_some(),
        lowered.program().is_some(),
        graph.json(),
    );
    fs::write(&control_path, control_json)
        .unwrap_or_else(|error| panic!("write {}: {error}", control_path.display()));
    let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
    artifacts.push(artifact(&control_path, "json", &evidence_dir));

    let engine = Engine {
        rust: rustc_version(),
        bevy: locked_version("bevy"),
        avian: locked_version("avian3d"),
    };

    let report = format!(
        "{{\n\
         \x20\"schema_version\": 1,\n\
         \x20\"task_id\": \"M19-B\",\n\
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
            "acceptance suite run locally with the retail capability; every field is derived \
             from the recorded log, production discovery of $CS_GAME_DIR and the control-program \
             binding `SourceContext::control_program` derives from it. The suite pins M19's \
             measured control program (objectives.zrd of ZBD/C4/M04/zrdr.zbd: 108 blocks, 440 \
             directive sites, 36 keys — the census's largest measured record — a fully measured \
             vocabulary) through the retail control census, the production control-program \
             binding and M19-A's mission binding, and locates the sheet's three regression \
             priorities in the record: world unlock as ten undormant watchers, the one 2-second \
             clock and the three TICK_DEPENDS_ON_OBJ gates (23 gated on the piratezep watcher \
             24, the launch waves 65 and 91 gated on watchers 106 and 103); the player-aircraft \
             transfer spelled as animation state — no key transfers — with block 19 arming \
             activate_bmhookup_node and completing on ANIM_STATE bm_hookup_player EXECUTED while \
             blocks 18/19/27/46 move the player_bmhook/bm_hook target pair; and ally extraction \
             as the win predicate itself, block 30's INSTANTWIN gated on ANIM_STATE \
             hooked_to_klondike EXECUTED and entered only through block 49's nap. The failure \
             side is measured: gasbag panels 9-12 at thresholds 1-4, engines 14-16 at 3/5/7, \
             hangar supports 20 (warn at 1) and 21 (fail at all four) and helium tank 39, each \
             worst rung napping the one INSTANTLOSS latch 13 after the spelled 20 s and killing \
             the same nine primary blocks; the goods block 17 kills watcher 39 and wakes block \
             26 on the same helium-tank chain, so one world event reads as failure before the \
             goods stage and release after it. All 147 spelled block addresses are in range, \
             the two latches' prerequisite closures are disjoint (13 and 10 blocks, 85 outside), \
             every actor the record names resolves in shipped data across three scopes — the \
             b_turret1/b_turret2 balloon turrets only through the b_turret* wildcard, and \
             klondike by no .zrd text anywhere — and M19's lowering is complete while the \
             campaign stays unready. Claim is implemented only; no mission was played, no \
             original executable was run, nothing is verified_original, the wrong-actor, \
             wrong-session and repeated-event halves of the sheet's priorities are runtime \
             observations that stay unmeasured (M19-C), and the measured state is recorded in \
             docs/findings/2026-10-10-m19-b-compatibility-gaps.md. Validated with \
             tools/validate_evidence.py --require-pass"
        ),
    );

    let out = evidence_dir.join("acceptance.json");
    fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));

    let written = fs::read_to_string(&out).expect("the report reads back");
    for needle in [
        "\"schema_version\": 1",
        "\"task_id\": \"M19-B\"",
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

/// [`parse_suite_prefixed`] with this task's test prefix.
fn parse_m19_b_suite(log: &str) -> Suite {
    parse_suite_prefixed(log, "accept_m19_b_")
}

// ---------------------------------------------------------- the graph ---

/// M19's control-record graph, measured from the decoded member: the two
/// latches, who may enter them and the blocks that lead to them. Same grammar
/// the acceptance suite walks — a text key, then its argument list — so the
/// artifact cannot silently disagree with what the tests asserted.
struct M19Graph {
    /// The completion edges (wakes, naps) into the failure latch 13.
    loss_entries: Vec<u32>,
    /// The completion edges into the success latch 30.
    win_entries: Vec<u32>,
    /// The prerequisite closure of the failure latch, itself included.
    loss_closure: Vec<u32>,
    /// The prerequisite closure of the success latch, itself included.
    win_closure: Vec<u32>,
    /// `(block, dependency)` of every TICK_DEPENDS_ON_OBJ gate.
    gates: Vec<(u32, u32)>,
    /// The blocks that spell no BEGIN_DORMANT.
    undormant: Vec<u32>,
}

/// `(key, args)` of one directive site in a walked block.
type Site = (String, Vec<cs_content::stunts::ZrdValue>);

/// `(block number, directives)` in spelling order.
type SpelledBlock = (u32, Vec<Site>);

impl M19Graph {
    fn measure(document: &cs_content::stunts::ZrdValue) -> Self {
        use cs_content::stunts::ZrdValue;
        let mut blocks: Vec<SpelledBlock> = Vec::new();
        for (key, value) in
            cs_content::stunts::zrd_flat_fields(cs_content::stunts::objective_record(document))
        {
            let Some(number) = cs_content::objectives::objective_block_number(key) else {
                continue;
            };
            let children = value.as_list().expect("every M19 block is a list");
            let mut directives = Vec::new();
            let mut index = 0;
            while index < children.len() {
                let ZrdValue::Text(name) = &children[index] else {
                    panic!("OBJECTIVE{number} child {index} is not a directive key");
                };
                index += 1;
                let args = children
                    .get(index)
                    .and_then(ZrdValue::as_list)
                    .map(<[ZrdValue]>::to_vec)
                    .unwrap_or_default();
                if children
                    .get(index)
                    .is_some_and(|value| value.as_list().is_some())
                {
                    index += 1;
                }
                directives.push((name.clone(), args));
            }
            blocks.push((number, directives));
        }
        blocks.sort_by_key(|(number, _)| *number);

        // The prerequisite map: wakes and naps edge predecessor → target,
        // gates edge block → dependency. Kills are not entries.
        let mut prerequisites: std::collections::BTreeMap<u32, Vec<u32>> =
            std::collections::BTreeMap::new();
        let mut gates = Vec::new();
        let mut undormant = Vec::new();
        for (number, directives) in &blocks {
            if !directives.iter().any(|(key, _)| key == "BEGIN_DORMANT") {
                undormant.push(*number);
            }
            for (key, args) in directives {
                let ints: Vec<i64> = args
                    .iter()
                    .filter_map(|value| match value {
                        ZrdValue::Int(int) => Some(i64::from(*int)),
                        _ => None,
                    })
                    .collect();
                match key.as_str() {
                    "WAKE_OBJECTIVE_WHEN_I_COMPLETE" => {
                        for address in ints {
                            prerequisites
                                .entry(address as u32)
                                .or_default()
                                .push(*number);
                        }
                    }
                    "NAP_OBJECTIVE_WHEN_I_COMPLETE" => {
                        if let Some(address) = ints.first() {
                            prerequisites
                                .entry(*address as u32)
                                .or_default()
                                .push(*number);
                        }
                    }
                    "TICK_DEPENDS_ON_OBJ" => {
                        for dependency in ints {
                            prerequisites
                                .entry(*number)
                                .or_default()
                                .push(dependency as u32);
                            gates.push((*number, dependency as u32));
                        }
                    }
                    _ => {}
                }
            }
        }
        for entries in prerequisites.values_mut() {
            entries.sort_unstable();
            entries.dedup();
        }
        let closure = |target: u32| {
            let mut seen: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
            let mut stack = vec![target];
            while let Some(node) = stack.pop() {
                if !seen.insert(node) {
                    continue;
                }
                if let Some(entries) = prerequisites.get(&node) {
                    stack.extend(entries.iter().copied());
                }
            }
            seen.into_iter().collect::<Vec<_>>()
        };
        Self {
            loss_entries: prerequisites.get(&13).cloned().unwrap_or_default(),
            win_entries: prerequisites.get(&30).cloned().unwrap_or_default(),
            loss_closure: closure(13),
            win_closure: closure(30),
            gates,
            undormant,
        }
    }

    fn json(&self) -> String {
        let numbers = |values: &[u32]| {
            values
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        };
        let gates = self
            .gates
            .iter()
            .map(|(block, dependency)| {
                format!("{{\"block\": {block}, \"depends_on\": {dependency}}}")
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{{\"latches\": {{\"win\": {{\"block\": 30, \"entries\": [{}], \"closure\": [{}]}}, \
             \"loss\": {{\"block\": 13, \"entries\": [{}], \"closure\": [{}]}}}}, \
             \"gates\": [{}], \"undormant\": [{}]}}",
            numbers(&self.win_entries),
            numbers(&self.win_closure),
            numbers(&self.loss_entries),
            numbers(&self.loss_closure),
            gates,
            numbers(&self.undormant),
        )
    }
}
