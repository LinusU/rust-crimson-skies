//! Acceptance for `M01-LC-DIRECTIVE-E` (#683): the measured directive
//! dispositions in `cs_content::mission_control`.
//!
//! Stages A–D measured the parser map and the per-family directive semantics
//! (findings `docs/findings/2026-10-06-m01-lc-directive-{b,c,d}-*.md`, with
//! `2026-10-04-m01-lc-mission-program.md` for the parser table). This suite pins
//! the **disposition** layer those measurements now feed:
//!
//! * every key a finding covers reports `DirectiveDisposition::Measured` — the
//!   measured operation, the evidence and the residual unknowns;
//! * a measured key is **not** implemented — measured is a statement about the
//!   original, not a host binding — and `Supported`/`complete` still requires
//!   the lowering requirements, which stay unmet;
//! * a key no finding covers stays `DirectiveDisposition::Unmeasured` with its
//!   named reason — `SET_AI_`, `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE`, the
//!   `OBJECTIVE_HD_*` family, `TEST_COMPLETE`, `WIN_ANIM`, `DELETE_ON_SUCCESS`,
//!   the record-level fields and the stray English words the corpus spells all
//!   keep their refusals;
//! * and the shape defects a per-key refusal used to carry — `INACTIVE1`'s
//!   disagreeing sites, `ANIM_STATE`'s nested list — are named by the lowering
//!   accounting on the row that owns them rather than laundered by the measured
//!   disposition.
//!
//! The retail tests need `CS_GAME_DIR`; the synthetic ones build `.zrd` values
//! tag by tag and exercise the same production walk.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cs_app::mission_control::survey_mission_control_programs;
use cs_content::mission_control::{
    DirectiveDisposition, DirectiveOperation, DirectiveRole, LoweringRequirementKind,
    MeasuredDirective, measure_control_record, measured_directive,
};
use cs_content::stunts::ZrdValue;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

const M01: &str = "zbd/c1c/m01";

// ------------------------------------------------- the .zrd authoring helpers ---

fn zrd_int(value: u32) -> ZrdValue {
    ZrdValue::Int(value)
}

fn zrd_float(value: f32) -> ZrdValue {
    ZrdValue::Float(value)
}

fn zrd_text(text: &str) -> ZrdValue {
    ZrdValue::Text(text.to_owned())
}

fn zrd_list(children: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(children)
}

/// One authored directive of a block: the key, and — unless authored bare — its
/// argument list beside it, flattened into the block's children the way the
/// measured grammar reads them.
fn directive(key: &str, args: Vec<ZrdValue>) -> Vec<ZrdValue> {
    let mut children = vec![zrd_text(key)];
    if !args.is_empty() {
        children.push(zrd_list(args));
    }
    children
}

fn block(number: u32, directives: Vec<Vec<ZrdValue>>) -> (String, ZrdValue) {
    let mut children = Vec::new();
    for children_of_directive in directives {
        children.extend(children_of_directive);
    }
    (format!("OBJECTIVE{number}"), zrd_list(children))
}

fn control_record(fields: Vec<(String, ZrdValue)>) -> ZrdValue {
    let mut children = Vec::new();
    for (key, value) in fields {
        children.push(zrd_text(&key));
        children.push(value);
    }
    zrd_list(vec![zrd_list(children)])
}

// ---------------------------------------------------------------------------
// The measured table
// ---------------------------------------------------------------------------

/// Every key `measured_directive` answers for, each with the operation the
/// findings measured. A key absent from this table — or the table answering a
/// different operation — fails the test: the disposition must follow the
/// evidence, and only the evidence.
#[test]
fn accept_m01_lc_directive_e_the_measured_table_is_exactly_the_findings_vocabulary() {
    let expected: &[(&str, DirectiveOperation)] = &[
        // Objective-record fields.
        ("BEGIN_DORMANT", DirectiveOperation::DormantStart),
        ("TICK_DEPENDS_ON_OBJ", DirectiveOperation::DependencyGate),
        ("IDENTITY", DirectiveOperation::PresentationIdentity),
        // Completion-condition evaluators and their thresholds.
        (
            "INACTIVE_COMPLETION_COUNT",
            DirectiveOperation::InactiveThreshold,
        ),
        (
            "DANGER_ZONES_COMPLETED",
            DirectiveOperation::DangerZoneFlags,
        ),
        (
            "DANGER_ZONES_COMPLETION_COUNT",
            DirectiveOperation::DangerZoneThreshold,
        ),
        ("DEDG", DirectiveOperation::EnemyGroupDepletion),
        ("TRAVELERS", DirectiveOperation::Travelers),
        ("ANIM_STATE", DirectiveOperation::AnimationStates),
        ("COUNTER", DirectiveOperation::NamedCounters),
        // Completion effects.
        ("WAKE_OBJECTIVE", DirectiveOperation::WakeObjectives),
        (
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
            DirectiveOperation::WakeObjectives,
        ),
        (
            "SLEEP_OBJECTIVE_WHEN_I_COMPLETE",
            DirectiveOperation::SleepObjectives,
        ),
        (
            "KILL_OBJECTIVE_WHEN_I_COMPLETE",
            DirectiveOperation::KillObjectives,
        ),
        (
            "NAP_OBJECTIVE_WHEN_I_COMPLETE",
            DirectiveOperation::NapObjective,
        ),
        (
            "ADD_OBJECTIVE_TARGET",
            DirectiveOperation::SetTargetFlag {
                objective: true,
                set: true,
            },
        ),
        (
            "REMOVE_OBJECTIVE_TARGET",
            DirectiveOperation::SetTargetFlag {
                objective: true,
                set: false,
            },
        ),
        (
            "ADD_OTHER_TARGET",
            DirectiveOperation::SetTargetFlag {
                objective: false,
                set: true,
            },
        ),
        (
            "REMOVE_OTHER_TARGET",
            DirectiveOperation::SetTargetFlag {
                objective: false,
                set: false,
            },
        ),
        ("COMPLETED_ZEPCANNONS", DirectiveOperation::ZeppelinCannons),
        ("COMPLETED_STOPPOINT", DirectiveOperation::AdvanceStopPoint),
        ("SET_AI_NET", DirectiveOperation::AssignNet),
        ("SET_AI_TEAM", DirectiveOperation::AssignTeam),
        ("SET_AI_ATTACK_RADIUS", DirectiveOperation::SetAttackRadius),
        ("START_TAXI", DirectiveOperation::ReleaseTaxi),
        ("SET_HELP_LABEL", DirectiveOperation::SetHelpLabel),
        ("STOP_QUEUED_SOUNDS", DirectiveOperation::StopQueuedSounds),
        (
            "COMPLETED_SOUND_GROUP",
            DirectiveOperation::CompletedSoundGroup,
        ),
        ("TIMER_ADJUST", DirectiveOperation::AdjustMissionTimer),
        (
            "ADJUST_TIMER_WHEN_I_COMPLETE",
            DirectiveOperation::AdjustMissionTimer,
        ),
        ("END_TIMER", DirectiveOperation::EndMissionTimer),
        ("WARP_VEHICLE", DirectiveOperation::WarpVehicle),
        ("HIDE_OBJ", DirectiveOperation::HideObjective),
        // Wake and transition effects.
        ("WAKEUP_ENEMIES", DirectiveOperation::WakeEnemies),
        ("WAKEUP_TURRETS", DirectiveOperation::WakeTurrets),
        (
            "WAKEUP_ZEP_TURRETS",
            DirectiveOperation::WakeZeppelinTurrets,
        ),
        ("WAKEUP_GENERATOR", DirectiveOperation::FeedGenerator),
        ("WAKE_ANIM", DirectiveOperation::WakeAnimation),
        ("WAKEUP_SOUND_GROUP", DirectiveOperation::WakeSoundGroup),
        ("RESET_TIMER", DirectiveOperation::ResetMissionTimer),
        ("SLEEP_ANIM", DirectiveOperation::TransitionAnimation),
        (
            "WAKE_OBJECTIVE_WHEN_I_SLEEP",
            DirectiveOperation::WakeObjectivesOnTransition,
        ),
        // The outcome classes — measured, and *not* the terminal keys.
        ("WON", DirectiveOperation::OutcomeClass { won: true }),
        ("LOST", DirectiveOperation::OutcomeClass { won: false }),
    ];
    for (key, operation) in expected {
        assert_eq!(
            measured_directive(key).map(|directive| directive.operation),
            Some(*operation),
            "{key}: the measured disposition the finding supplies"
        );
        let MeasuredDirective {
            summary,
            evidence,
            unknowns,
            ..
        } = measured_directive(key).expect("measured");
        assert!(
            !summary.is_empty() && !evidence.is_empty(),
            "{key}: a measured disposition names its effect and its evidence"
        );
        for unknown in unknowns {
            assert!(
                !unknown.is_empty(),
                "{key}: a residual unknown is a statement, not a placeholder"
            );
        }
    }

    // The hundred INACTIVE<n> spellings are the one measured mechanism, and the
    // bound is the parser's: INACTIVE1..=INACTIVE100 exactly — canonical
    // formatting, no leading zeros, no INACTIVE0, no INACTIVE101.
    for stage in [1u32, 2, 9, 18, 42, 99, 100] {
        assert_eq!(
            measured_directive(&format!("INACTIVE{stage}")).map(|directive| directive.operation),
            Some(DirectiveOperation::InactiveMembers),
            "INACTIVE{stage}: the measured member-count mechanism"
        );
    }
    for key in [
        "INACTIVE0",
        "INACTIVE101",
        "INACTIVE01",
        "INACTIVE",
        "INACTIVEX",
    ] {
        assert_eq!(
            measured_directive(key),
            None,
            "{key}: outside the measured `INACTIVE%d` loop"
        );
    }

    // Everything the findings do not cover stays unmeasured — the record-level
    // keys, the corpus-only directive keys, the parser keys whose handlers were
    // never located, the argument tokens and the stray words.
    for key in [
        "MISSION_TIMER",
        "PLAYER_INIT",
        "RESTORE_ANIMS",
        "EXECUTE_ANIMS",
        "INVALIDATE_ANIMS",
        "MISSION_LOST_SOUND",
        "PRIMARY_COMPLETE_SOUND",
        "WAKEUP_OBJECTIVE_WHEN_I_COMPLETE",
        "SET_AI_",
        "OBJECTIVE_HD_a",
        "OBJECTIVE_HD_b",
        "TEST_COMPLETE",
        "COMPLETION_COUNT",
        "WIN_ANIM",
        "LOSS_ANIM",
        "DELETE_ON_SUCCESS",
        "NOLOSS",
        "Change",
        "to",
        "mobile",
        "net",
        "INSTANTWIN",
        "INSTANTLOSS",
    ] {
        assert_eq!(
            measured_directive(key),
            None,
            "{key}: no finding measures it, so no disposition table entry"
        );
    }
}

/// `WON` and `LOST` are measured *outcome classes* — the mission resolves when
/// every block of a class completes — not the terminal outcome requests
/// `INSTANTWIN`/`INSTANTLOSS` spell. The two pairs must never be conflated.
#[test]
fn accept_m01_lc_directive_e_won_and_lost_are_measured_classes_not_terminal_requests() {
    let document = control_record(vec![block(
        1,
        vec![
            directive("WON", vec![zrd_int(0)]),
            directive("LOST", vec![zrd_int(0)]),
            directive("INSTANTWIN", Vec::new()),
            directive("INSTANTLOSS", Vec::new()),
        ],
    )]);
    let record = measure_control_record(&document);

    for (key, won) in [("WON", true), ("LOST", false)] {
        match record.key(key).expect("spelled").disposition() {
            DirectiveDisposition::Measured(directive) => {
                assert_eq!(
                    directive.operation,
                    DirectiveOperation::OutcomeClass { won },
                    "{key} is the measured outcome class"
                );
                assert_eq!(
                    directive.operation.role(),
                    DirectiveRole::OutcomeAggregation,
                    "{key} feeds the aggregation, not a call"
                );
            }
            other => panic!("{key} is measured, got {other:?}"),
        }
        assert!(
            cs_content::mission_control::terminal_outcome_of(key).is_none(),
            "{key} is not a terminal-outcome spelling"
        );
    }
    let implemented: Vec<&str> = record
        .implemented()
        .iter()
        .map(|(key, _)| key.key.as_str())
        .collect();
    assert_eq!(
        implemented,
        ["INSTANTLOSS", "INSTANTWIN"],
        "the terminal requests are still the only implemented directives"
    );
}

/// A measured disposition is not support: the three sets — implemented,
/// measured, unmeasured — partition the vocabulary and the lowering accounting
/// still names what the record lacks.
#[test]
fn accept_m01_lc_directive_e_measured_is_not_implemented_and_the_sets_partition() {
    let document = control_record(vec![
        block(
            1,
            vec![
                directive("IDENTITY", vec![zrd_text("PRIMARY"), zrd_int(1)]),
                directive("BEGIN_DORMANT", vec![zrd_float(-1.0)]),
                directive("INACTIVE1", vec![zrd_text("workersvoyagezep")]),
                directive("INACTIVE_COMPLETION_COUNT", vec![zrd_int(4)]),
                directive("WAKE_ANIM", vec![zrd_text("wv_hookup")]),
                directive("WAKEUP_OBJECTIVE_WHEN_I_COMPLETE", vec![zrd_int(3)]),
                directive("INSTANTWIN", Vec::new()),
            ],
        ),
        block(
            2,
            vec![
                directive(
                    "SET_HELP_LABEL",
                    vec![zrd_list(vec![zrd_text("a"), zrd_text("b")]), zrd_text(" ")],
                ),
                directive("INSTANTLOSS", Vec::new()),
            ],
        ),
    ]);
    let record = measure_control_record(&document);

    let implemented: Vec<&str> = record
        .implemented()
        .iter()
        .map(|(key, _)| key.key.as_str())
        .collect();
    let measured: Vec<&str> = record
        .measured()
        .iter()
        .map(|(key, _)| key.key.as_str())
        .collect();
    let unmeasured: Vec<&str> = record
        .unmeasured()
        .iter()
        .map(|(key, _)| key.key.as_str())
        .collect();
    assert_eq!(implemented, ["INSTANTLOSS", "INSTANTWIN"]);
    assert_eq!(
        unmeasured,
        ["WAKEUP_OBJECTIVE_WHEN_I_COMPLETE"],
        "the key no finding covers is the one refusal"
    );
    assert_eq!(
        measured.len() + implemented.len() + unmeasured.len(),
        record.vocabulary() as usize,
        "the three sets partition: {measured:?} + {implemented:?} + {unmeasured:?}"
    );

    // The measured keys keep their shape gaps named — SET_HELP_LABEL's nested
    // site lands on the calls row, never flattened into a positional call.
    let lowering = record.lowering();
    let calls = lowering
        .requirements()
        .iter()
        .find(|row| row.kind == LoweringRequirementKind::CallArguments)
        .expect("the calls row exists");
    assert!(!calls.met);
    assert!(
        calls
            .unmeasured_fields
            .iter()
            .any(|field| field.contains("SET_HELP_LABEL") && field.contains("cannot carry")),
        "the calls row names the nested shape it cannot carry: {:?}",
        calls.unmeasured_fields
    );
    assert!(
        calls
            .unmeasured_fields
            .iter()
            .any(|field| field.contains("WAKEUP_OBJECTIVE_WHEN_I_COMPLETE")),
        "the calls row names the unmeasured key: {:?}",
        calls.unmeasured_fields
    );
    assert!(!record.is_complete());
    assert!(!record.lowering().complete());
}

/// `Supported` is a bar the accounting still fails: mission and objective
/// identity are measured, the condition and the calls are not — and an empty
/// record fails closed rather than vacuously reporting ready.
#[test]
fn accept_m01_lc_directive_e_the_lowering_rows_fail_closed() {
    // A record of only measured directives: identity met, condition and calls
    // still unmet.
    let record = measure_control_record(&control_record(vec![block(
        1,
        vec![
            directive("BEGIN_DORMANT", vec![zrd_float(-1.0)]),
            directive("INSTANTWIN", Vec::new()),
        ],
    )]));
    let lowering = record.lowering();
    let rows: BTreeMap<LoweringRequirementKind, bool> = lowering
        .requirements()
        .iter()
        .map(|row| (row.kind, row.met))
        .collect();
    assert_eq!(
        rows,
        BTreeMap::from([
            (LoweringRequirementKind::MissionIdentity, true),
            (LoweringRequirementKind::ObjectiveIdentity, true),
            (LoweringRequirementKind::ObjectiveCondition, false),
            (LoweringRequirementKind::CallArguments, false),
        ]),
        "the measured block's rows: identity met, condition and calls unmet"
    );
    for row in lowering.unmet() {
        assert!(
            !row.unmeasured_fields.is_empty(),
            "{}: an unmet row names what it lacks",
            row.label()
        );
    }

    // And the empty record: no blocks, so even identity is unmet — never a
    // vacuous pass.
    let empty = measure_control_record(&control_record(Vec::new()));
    let empty_lowering = empty.lowering();
    assert_eq!(empty_lowering.requirements().len(), 4);
    assert_eq!(
        empty_lowering.unmet().count(),
        3,
        "an empty record fails identity, condition and calls"
    );
    assert!(!empty_lowering.complete());
    assert!(!empty.is_complete());
}

// ---------------------------------------------------------------------------
// The retail record
// ---------------------------------------------------------------------------

/// M01's whole vocabulary carries a measured disposition or an outcome: 41
/// measured keys — each with the operation its finding supplies — and the two
/// terminal spellings, with no key left unmeasured.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directive_e_every_m01_key_reports_the_disposition_its_evidence_supplies() {
    let census =
        survey_mission_control_programs(&game_dir()).expect("the census runs on the installation");
    let record = census
        .row(M01)
        .expect("M01 is measured")
        .record()
        .expect("M01 declares a control program");

    // The 41 non-outcome keys M01 spells, each with the operation the findings
    // measure for it.
    let mut expected: BTreeMap<String, DirectiveOperation> = BTreeMap::from([
        (
            "ADD_OBJECTIVE_TARGET",
            DirectiveOperation::SetTargetFlag {
                objective: true,
                set: true,
            },
        ),
        (
            "ADD_OTHER_TARGET",
            DirectiveOperation::SetTargetFlag {
                objective: false,
                set: true,
            },
        ),
        ("ANIM_STATE", DirectiveOperation::AnimationStates),
        ("BEGIN_DORMANT", DirectiveOperation::DormantStart),
        (
            "COMPLETED_SOUND_GROUP",
            DirectiveOperation::CompletedSoundGroup,
        ),
        ("COMPLETED_STOPPOINT", DirectiveOperation::AdvanceStopPoint),
        ("DEDG", DirectiveOperation::EnemyGroupDepletion),
        ("IDENTITY", DirectiveOperation::PresentationIdentity),
        (
            "INACTIVE_COMPLETION_COUNT",
            DirectiveOperation::InactiveThreshold,
        ),
        (
            "KILL_OBJECTIVE_WHEN_I_COMPLETE",
            DirectiveOperation::KillObjectives,
        ),
        (
            "NAP_OBJECTIVE_WHEN_I_COMPLETE",
            DirectiveOperation::NapObjective,
        ),
        (
            "REMOVE_OBJECTIVE_TARGET",
            DirectiveOperation::SetTargetFlag {
                objective: true,
                set: false,
            },
        ),
        ("SET_AI_NET", DirectiveOperation::AssignNet),
        ("SET_HELP_LABEL", DirectiveOperation::SetHelpLabel),
        ("STOP_QUEUED_SOUNDS", DirectiveOperation::StopQueuedSounds),
        ("TICK_DEPENDS_ON_OBJ", DirectiveOperation::DependencyGate),
        ("TRAVELERS", DirectiveOperation::Travelers),
        ("WAKEUP_ENEMIES", DirectiveOperation::WakeEnemies),
        ("WAKEUP_GENERATOR", DirectiveOperation::FeedGenerator),
        ("WAKEUP_SOUND_GROUP", DirectiveOperation::WakeSoundGroup),
        (
            "WAKEUP_ZEP_TURRETS",
            DirectiveOperation::WakeZeppelinTurrets,
        ),
        ("WAKE_ANIM", DirectiveOperation::WakeAnimation),
        (
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
            DirectiveOperation::WakeObjectives,
        ),
    ])
    .into_iter()
    .map(|(key, operation)| (key.to_owned(), operation))
    .collect();
    for stage in 1..=18u32 {
        expected.insert(
            format!("INACTIVE{stage}"),
            DirectiveOperation::InactiveMembers,
        );
    }

    for key in record.keys() {
        match key.disposition() {
            DirectiveDisposition::Measured(directive) => {
                let want = expected.remove(&key.key).unwrap_or_else(|| {
                    panic!(
                        "{}: M01 spells a measured key the table does not expect",
                        key.key
                    )
                });
                assert_eq!(
                    directive.operation, want,
                    "{}: the operation its finding supplies",
                    key.key
                );
            }
            DirectiveDisposition::TerminalOutcome { .. } => {
                assert!(
                    matches!(key.key.as_str(), "INSTANTWIN" | "INSTANTLOSS"),
                    "{}: the only terminal keys are the outcome spellings",
                    key.key
                );
            }
            DirectiveDisposition::Unmeasured { reason } => {
                panic!(
                    "{}: M01 spells no unmeasured key — every directive is \
                     covered by the findings (got {reason})",
                    key.key
                )
            }
        }
    }
    assert!(
        expected.is_empty(),
        "the table named keys M01 does not spell: {expected:?}"
    );
}

/// M01's lowering: the two identity requirements are met and the condition and
/// the calls stay unmet, each naming what it lacks — the evaluators are not
/// side-effect-free `Condition`s and no `Lowering` variant carries the measured
/// operations. Supported is still refused, honestly.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directive_e_m01_is_measured_but_never_supported() {
    let census =
        survey_mission_control_programs(&game_dir()).expect("the census runs on the installation");
    let row = census.row(M01).expect("M01 is measured");
    let lowering = row.lowering().expect("a measured row lowers");

    let unmet: Vec<LoweringRequirementKind> = lowering.unmet().map(|row| row.kind).collect();
    assert_eq!(
        unmet,
        [
            LoweringRequirementKind::ObjectiveCondition,
            LoweringRequirementKind::CallArguments,
        ],
        "the two rows the findings cannot discharge stay unmet"
    );
    assert!(!lowering.complete());
    assert!(!row.is_complete());

    let fields = lowering.unmeasured_fields();
    // The residual unknowns are named, per key — the measurements' own limits,
    // carried rather than dropped.
    for name in ["IDENTITY", "TRAVELERS", "DEDG"] {
        assert!(
            fields.iter().any(|field| field.contains(name)),
            "{name}: its residual unknown is named: {fields:?}"
        );
    }
    // And the unmeasured-key names that are known corpus-wide stay absent here:
    // M01 spells none of them.
    for key in ["SET_AI_", "WAKEUP_OBJECTIVE_WHEN_I_COMPLETE"] {
        assert!(
            !fields
                .iter()
                .any(|field| field.contains(&format!("`{key}`:"))),
            "{key}: M01 does not spell it, so it cannot be named unmeasured here"
        );
    }

    // The corpus gate follows: no mission is complete while any row spells an
    // unmeasured key or an unmet lowering row.
    assert!(
        census.complete_missions().is_empty(),
        "no mission reports complete while measured is not implemented"
    );
    assert!(!census.campaign_ready());
}

/// Corpus-wide, the boundary is the findings: every spelled key either carries
/// the measured disposition its evidence supplies, is a terminal outcome, or is
/// refused `Unmeasured` — and the corpus-only keys that prove the boundary are
/// named.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directive_e_corpus_keys_no_finding_covers_stay_refused() {
    let census =
        survey_mission_control_programs(&game_dir()).expect("the census runs on the installation");
    let mut unmeasured_corpus: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for row in census.measured_rows() {
        let record = row.record().expect("a measured row carries a record");
        for key in record.keys() {
            match key.disposition() {
                DirectiveDisposition::Measured(_) => {}
                DirectiveDisposition::TerminalOutcome { .. } => {}
                DirectiveDisposition::Unmeasured { .. } => {
                    unmeasured_corpus
                        .entry(key.key.clone())
                        .or_default()
                        .push(row.mission());
                }
            }
        }
    }
    // The keys the corpus spells and no finding covers — the parser table's
    // residue and the stray English. Each is a live refusal, not a silent drop.
    for key in [
        "WAKEUP_OBJECTIVE_WHEN_I_COMPLETE",
        "SET_AI_",
        "Change",
        "to",
        "mobile",
        "net",
    ] {
        assert!(
            unmeasured_corpus.contains_key(key),
            "{key}: spelled in the corpus, covered by no finding, so refused — \
             the measured table must not absorb it"
        );
    }
    // And no unmeasured key is ever implemented or reported measured.
    for (key, missions) in &unmeasured_corpus {
        assert_eq!(
            measured_directive(key),
            None,
            "{key} ({missions:?}): an unmeasured disposition means no finding entry"
        );
    }
}
