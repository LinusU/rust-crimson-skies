//! F37-D-FU2 acceptance (Rally #589): the mission-terminal precedence and
//! tick-ordering rules, labelled with their source and pinned against the
//! owner's static-code-evidence observation.
//!
//! The evidence is *static analysis of the owner-supplied decrypted
//! executable* (owner note on Rally #589, 2026-10-05), never a run of the
//! original: the claim it supports is `inferred`, never `verified_original`.
//! Addresses and behaviour are cited; no executable byte, disassembly listing
//! or decompiled code appears anywhere in this tree.
//!
//! What the production code is pinned on here:
//!
//! * `accept_f37_d_fu2_measured_precedence_records_success_iff_won` — the
//!   default policy records success when both outcomes are requested on one
//!   tick (the original's "success iff WON"), the designed policy still says
//!   the opposite when a session selects it, and an abort request keeps the
//!   designed ordering the limitation records.
//! * `accept_f37_d_fu2_completion_scan_runs_in_declaration_index_order` — the
//!   scan reaches objectives in declaration (index) order, not symbol order.
//! * `accept_f37_d_fu2_both_rules_carry_a_source_label_and_their_evidence` and
//!   `accept_f37_d_fu2_every_limitation_names_affected_content_and_a_resolving_task`
//!   — the two rules' labels, addresses, image sha256, findings entry, fact
//!   table and machine-readable limitations, cross-referenced so neither side
//!   can drift from the other.
//! * `accept_f37_d_fu2_recorded_divergences_match_what_the_runtime_does` — the
//!   runtime really does diverge where a limitation says it does, so closing a
//!   limitation and changing the behaviour cannot happen apart.

use std::collections::BTreeSet;

use cs_script::ir::*;
use cs_script::runtime::*;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ClaimStatus;

/// Fixed session generation: the event keys are part of what the probes read.
const SESSION: SessionGeneration = SessionGeneration(11);

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

fn reward(key: &str) -> Action {
    Action::GrantReward {
        reward: cid(ContentKind::Blueprint, key),
    }
}

/// A `Condition::Const(true)` objective: it completes as soon as the scan
/// reaches it, so the scan itself is what the probe reads.
fn objective(id: u32, actions: Vec<Action>) -> Objective {
    Objective {
        id: SymbolId(id),
        content: cid(ContentKind::Objective, &format!("synthetic-fu2-obj-{id}")),
        condition: Condition::Const(true),
        actions,
        span: None,
    }
}

fn program(objectives: Vec<Objective>) -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-f37-d-fu2"),
        variables: vec![],
        objectives,
    }
}

fn rewards(events: &[MissionEvent]) -> Vec<ContentId> {
    events
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::RewardGranted(reward) => Some(reward.clone()),
            _ => None,
        })
        .collect()
}

fn terminal_requests(events: &[MissionEvent]) -> Vec<Outcome> {
    events
        .iter()
        .filter_map(|event| match event.kind {
            EventKind::TerminalRequested(outcome) => Some(outcome),
            _ => None,
        })
        .collect()
}

fn completions(events: &[MissionEvent]) -> Vec<u32> {
    events
        .iter()
        .filter(|event| matches!(event.kind, EventKind::ObjectiveCompleted))
        .map(|event| event.key.source.0)
        .collect()
}

/// **The precedence change, pinned on the production path.**
///
/// Both outcomes are requested on one tick and the measured policy records
/// the success, because the original's result is success iff its WON flag is
/// set (0x4194e0) even though the loss branch is the branch that plays. The
/// designed conservative policy, which F37-A wrote before anything was
/// measured, is still reachable — a session selects it through its save
/// record, which is why the policy travels in the record — and still gives
/// the opposite answer, so the two rules are distinguishable by behaviour and
/// not only by their labels.
#[test]
fn accept_f37_d_fu2_measured_precedence_records_success_iff_won() {
    let p = program(vec![
        // Declared out of symbol order on purpose: the requests are merged
        // into one set, so which one wins may not depend on the scan.
        objective(10, vec![Action::Finish(Outcome::Succeeded)]),
        objective(3, vec![Action::Finish(Outcome::Failed)]),
    ])
    .validate()
    .unwrap();

    let mut measured = MissionState::new(&p, SESSION);
    // The default policy is the measured one; it is observable through the
    // save record, which carries the rule a restore must not swap.
    assert_eq!(
        measured.snapshot(&p).policy,
        PrecedencePolicy::MeasuredOriginal
    );
    let tick = measured
        .step(&p, &MissionFacts::default(), Tick(1))
        .unwrap();
    // Both requests were made — the runtime does not hide the one that lost.
    assert_eq!(
        terminal_requests(&tick.events),
        vec![Outcome::Failed, Outcome::Succeeded],
        "requests are reported in key order, both of them"
    );
    // ... and the success is what is recorded: success iff WON.
    assert_eq!(tick.terminal, TerminalState::Succeeded);
    // Latched: later ticks cannot flip it and emit nothing.
    let later = measured
        .step(&p, &MissionFacts::default(), Tick(2))
        .unwrap();
    assert!(later.events.is_empty());
    assert_eq!(later.terminal, TerminalState::Succeeded);

    // The designed policy, selected through the save record, still answers
    // `Failed`: the policies are different rules, not two names for one.
    let mut record = MissionState::new(&p, SESSION).snapshot(&p);
    record.policy = PrecedencePolicy::SyntheticConservative;
    let mut designed = MissionState::restore(&p, record).unwrap();
    let tick = designed
        .step(&p, &MissionFacts::default(), Tick(1))
        .unwrap();
    assert_eq!(tick.terminal, TerminalState::Failed);

    // An abort request is ordered above the measured results — the designed
    // answer, because the original has no Aborted outcome to measure
    // (`f37.d.limit.aborted_outcome`).
    let p = program(vec![
        objective(1, vec![Action::Finish(Outcome::Succeeded)]),
        objective(2, vec![Action::Finish(Outcome::Aborted)]),
    ])
    .validate()
    .unwrap();
    let mut state = MissionState::new(&p, SESSION);
    let tick = state.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    assert_eq!(
        tick.terminal,
        TerminalState::Aborted,
        "the abort ordering is designed, and recorded as such"
    );
    assert!(
        RULE_LIMITATIONS
            .iter()
            .any(|limitation| limitation.id == "f37.d.limit.aborted_outcome"),
        "the designed half must stay recorded as a limitation"
    );
}

/// **The index-order scan, pinned on the production path.**
///
/// The objectives are declared out of symbol order (9 first, 3 second) and
/// the work budget is at its floor, so only one objective can be *admitted*
/// per tick: the scan order decides which one. A scan in symbol order would
/// run objective 3 first and defer objective 9; the measured order — lowest
/// index first — runs the objective declared first, and the other one's own
/// reward follows on the next tick.
#[test]
fn accept_f37_d_fu2_completion_scan_runs_in_declaration_index_order() {
    let r9 = cid(ContentKind::Blueprint, "r-nine");
    let r3 = cid(ContentKind::Blueprint, "r-three");
    let p = program(vec![
        objective(9, vec![reward("r-nine")]),
        objective(3, vec![reward("r-three")]),
    ])
    .validate()
    .unwrap();

    let mut state = MissionState::new(&p, SESSION);
    state.set_limits(WorkLimits {
        max_work_per_tick: MIN_WORK_PER_TICK,
        ..WorkLimits::default()
    });

    let first = state.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    assert_eq!(
        rewards(&first.events),
        vec![r9.clone()],
        "objective 9 is declared first, so the scan admits it first"
    );
    assert!(state.is_completed(SymbolId(9)));
    // The scan reached the second objective too — it is latched by the budget
    // stop — but its own action had no budget left and is retried, never
    // skipped.
    assert!(state.is_completed(SymbolId(3)));

    let second = state.step(&p, &MissionFacts::default(), Tick(2)).unwrap();
    assert_eq!(
        rewards(&second.events),
        vec![r3],
        "the deferred action of the later-declared objective runs next tick"
    );
}

/// **Both rules carry a source label, with the evidence behind it.**
///
/// The two rules the task decides are `measured-from-original` (static code
/// evidence, `inferred`, never `verified_original`) and every fact cites the
/// addresses that settle it, the findings entry and the image sha256. The
/// observation order of the recreated event stream is the one labelled
/// `designed-and-unmeasured`, because nothing about it was measured from the
/// original — the original emits no event stream at all.
#[test]
fn accept_f37_d_fu2_both_rules_carry_a_source_label_and_their_evidence() {
    for rule in [TERMINAL_PRECEDENCE_RULE, TICK_ORDERING_RULE] {
        assert_eq!(
            rule.source,
            RuleSource::MeasuredFromOriginalStatic,
            "{} must be labelled measured-from-original",
            rule.id
        );
        assert_eq!(rule.source.label(), "measured-from-original", "{}", rule.id);
        assert_eq!(rule.source.claim(), ClaimStatus::Inferred, "{}", rule.id);
        assert_ne!(
            rule.source.claim(),
            ClaimStatus::VerifiedOriginal,
            "{}: static code evidence never verifies the original",
            rule.id
        );
        assert!(!rule.statement.is_empty(), "{}", rule.id);
        assert!(
            rule.evidence.contains(TERMINAL_RULE_FINDINGS),
            "{} must cite its findings entry",
            rule.id
        );
        assert!(
            rule.evidence.contains(ORIGINAL_IMAGE_SHA256),
            "{} must cite the image sha256",
            rule.id
        );
        assert!(!rule.facts.is_empty(), "{} has no facts", rule.id);
        for fact in rule.facts {
            let prefix = format!("{}.", rule.id);
            assert!(
                fact.id.starts_with(&prefix),
                "{} does not belong under {}",
                fact.id,
                rule.id
            );
            assert!(!fact.statement.is_empty(), "{}", fact.id);
            assert!(
                !fact.addresses.is_empty(),
                "a measured fact must cite the addresses that settle it: {}",
                fact.id
            );
            for id in fact.limitations {
                assert!(
                    RULE_LIMITATIONS
                        .iter()
                        .any(|limitation| limitation.id == *id),
                    "fact {} names unknown limitation {}",
                    fact.id,
                    id
                );
            }
        }
    }

    let addresses = |rule: RuleLabel| {
        rule.facts
            .iter()
            .map(|fact| fact.addresses)
            .collect::<Vec<_>>()
            .join(" ")
    };
    let precedence = addresses(TERMINAL_PRECEDENCE_RULE);
    for address in [
        "0x46a490", "0x46c640", "0x463c30", "0x46af7a", "0x46afad", "0x4194e0",
    ] {
        assert!(
            precedence.contains(address),
            "the precedence rule does not cite {address}"
        );
    }
    let ordering = addresses(TICK_ORDERING_RULE);
    for address in ["0x4a0220", "0x4d0010", "0x4a09c3", "0x46a490", "0x71c128"] {
        assert!(
            ordering.contains(address),
            "the tick-ordering rule does not cite {address}"
        );
    }

    // The observation order is the designed-and-unmeasured one.
    assert_eq!(
        EVENT_OBSERVATION_ORDER_RULE.source,
        RuleSource::DesignedAndUnmeasured
    );
    assert_eq!(
        EVENT_OBSERVATION_ORDER_RULE.source.label(),
        "designed-and-unmeasured"
    );
    assert_eq!(
        EVENT_OBSERVATION_ORDER_RULE.source.claim(),
        ClaimStatus::Designed
    );
    assert!(
        EVENT_OBSERVATION_ORDER_RULE
            .evidence
            .contains("no addresses"),
        "a designed rule must not borrow measured addresses"
    );
    assert!(
        EVENT_OBSERVATION_ORDER_RULE.facts.is_empty(),
        "nothing about the observation order was measured"
    );
}

/// **The limitations are machine-readable, complete and cross-referenced.**
///
/// Every `f37.d.limit.*` entry names what is open, the content it affects and
/// what resolves it; every fact that the runtime does not follow points at
/// one; and the facts it *does* follow carry none, so a fidelity claim gated
/// by an open entry cannot be dropped silently. The findings entry the labels
/// cite must exist in the tree and quote the same image sha256.
#[test]
fn accept_f37_d_fu2_every_limitation_names_affected_content_and_a_resolving_task() {
    assert!(!RULE_LIMITATIONS.is_empty());
    let mut ids = BTreeSet::new();
    for limitation in RULE_LIMITATIONS {
        assert!(
            limitation.id.starts_with("f37.d.limit."),
            "{}",
            limitation.id
        );
        assert!(
            ids.insert(limitation.id),
            "duplicate limitation id {}",
            limitation.id
        );
        assert!(!limitation.open.is_empty(), "{}", limitation.id);
        assert!(
            !limitation.affected_content.is_empty(),
            "{} must name the content it affects",
            limitation.id
        );
        assert!(
            !limitation.resolving_task.is_empty(),
            "{} must name what resolves it",
            limitation.id
        );
    }

    // The facts and the limitations reference each other exactly: no orphan
    // limitation, no fact pointing at one that does not exist.
    let mut referenced = BTreeSet::new();
    let mut facts = BTreeSet::new();
    for rule in [TERMINAL_PRECEDENCE_RULE, TICK_ORDERING_RULE] {
        for fact in rule.facts {
            assert!(facts.insert(fact.id), "duplicate fact id {}", fact.id);
            referenced.extend(fact.limitations.iter().copied());
        }
    }
    assert_eq!(
        referenced, ids,
        "every limitation is referenced by exactly the facts it gates"
    );

    // The four answers the owner note settles are all recorded, with and
    // without their divergence.
    for id in [
        "f37.rule.terminal_precedence.countdown_preempts",
        "f37.rule.terminal_precedence.one_completion_per_tick",
        "f37.rule.terminal_precedence.loss_branch_before_win",
        "f37.rule.terminal_precedence.result_iff_won",
        "f37.rule.terminal_precedence.no_aborted",
        "f37.rule.tick_ordering.mission_update_position",
        "f37.rule.tick_ordering.per_tick_order",
    ] {
        assert!(facts.contains(id), "the measured fact {id} is not recorded");
    }
    let fact = TERMINAL_PRECEDENCE_RULE
        .facts
        .iter()
        .find(|fact| fact.id == "f37.rule.terminal_precedence.result_iff_won")
        .expect("result_iff_won is recorded");
    assert!(
        fact.limitations.is_empty(),
        "the implemented half carries no divergence; `accept_f37_d_fu2_measured_precedence_records_success_iff_won` pins it"
    );

    let findings = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(TERMINAL_RULE_FINDINGS);
    let text = std::fs::read_to_string(&findings).unwrap_or_else(|error| {
        panic!("the findings entry {TERMINAL_RULE_FINDINGS} must exist: {error}")
    });
    assert!(
        text.contains(ORIGINAL_IMAGE_SHA256),
        "the findings entry must quote the image sha256 it was measured from"
    );
}

/// **The record tells the truth about what this runtime does.**
///
/// A limitation that claims a divergence must describe the running code: this
/// test demonstrates the divergence itself, so the entry cannot survive a
/// silent behaviour change, and a change of behaviour cannot land without the
/// entry being closed in the same commit (F37-D-FU3 closes
/// `f37.d.limit.one_completion_per_tick` together with these assertions, and
/// F37-D-FU4 closed `f37.d.limit.mission_countdown_preemption` together with
/// the `accept_f37_d_fu4_*` tests that implement the pre-emption).
#[test]
fn accept_f37_d_fu2_recorded_divergences_match_what_the_runtime_does() {
    // `f37.d.limit.one_completion_per_tick`: two satisfied objectives both
    // complete on this tick, which the original never does.
    let p = program(vec![
        objective(1, vec![reward("r-a")]),
        objective(2, vec![reward("r-b")]),
    ])
    .validate()
    .unwrap();
    let mut state = MissionState::new(&p, SESSION);
    let tick = state.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    assert_eq!(
        completions(&tick.events),
        vec![1, 2],
        "two objectives completed on one tick: f37.d.limit.one_completion_per_tick \
         still describes this runtime (close it and this assertion together, in \
         F37-D-FU3)"
    );
    assert!(
        RULE_LIMITATIONS
            .iter()
            .any(|limitation| limitation.id == "f37.d.limit.one_completion_per_tick"),
        "the divergence this test just demonstrated must stay recorded"
    );

    // F37-D-FU4 (#730) closed `f37.d.limit.mission_countdown_preemption`: the
    // pre-emption the entry denied ("a tick carries no timeout input") exists
    // now — `MissionState::step_with_countdown` ends the mission as a failure
    // before that tick's objectives, pinned by `accept_f37_d_fu4_*`. A closed
    // entry must not come back while that behaviour stands.
    assert!(
        !RULE_LIMITATIONS
            .iter()
            .any(|limitation| limitation.id == "f37.d.limit.mission_countdown_preemption"),
        "the closed countdown entry must not come back: the pre-emption is implemented"
    );
    // What still diverges is reachability: nothing in this tree feeds the
    // input, so that entry stays recorded.
    assert!(
        RULE_LIMITATIONS
            .iter()
            .any(|limitation| limitation.id == "f37.d.limit.mission_countdown_producer"),
        "the countdown pre-emption's remaining divergence — no producer feeds it — \
         stays recorded"
    );
    let p = program(vec![objective(1, vec![reward("r-only")])])
        .validate()
        .unwrap();
    let mut state = MissionState::new(&p, SESSION);
    // Ten ordinary ticks on the path every current caller uses
    // (`MissionState::step`, which passes `MissionCountdown::NONE`): the
    // mission runs on, granting exactly its one reward, with no path that
    // could have failed it on the way — the divergence the entry describes.
    for tick in 1..=10u64 {
        let result = state
            .step(&p, &MissionFacts::default(), Tick(tick))
            .unwrap();
        assert_eq!(result.terminal, TerminalState::Running, "tick {tick}");
    }
    assert_eq!(
        state.terminal(),
        TerminalState::Running,
        "nothing but a program request can end this mission on the default path"
    );
}
