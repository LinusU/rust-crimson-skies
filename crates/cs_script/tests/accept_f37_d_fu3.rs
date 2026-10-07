//! F37-D-FU3 acceptance (Rally #729): `MissionState::step` completes **at
//! most one objective per tick** — the lowest declaration index whose
//! condition holds — the rule the owner measured in the original's completion
//! scan (`f37.rule.terminal_precedence.one_completion_per_tick`, source
//! `measured-from-original`: static code evidence from the owner-supplied
//! decrypted executable, never a run of the original and never
//! `verified_original`).
//!
//! What is pinned here, on the production evaluator:
//!
//! * `accept_f37_d_fu3_one_objective_completes_per_tick_lowest_declaration_index_first`
//!   — one completion per tick, in declaration (index) order, every reward
//!   exactly once.
//! * `accept_f37_d_fu3_a_satisfied_objective_that_waits_is_re_evaluated_not_latched`
//!   — the objective that lost the scan stays *unfired*: its condition is
//!   evaluated again on every later tick and it completes only on a tick where
//!   it still holds, so nothing is latched early.
//! * `accept_f37_d_fu3_objectives_cannot_request_both_outcomes_on_one_tick`
//!   — the measured consequence: through objectives, success and failure can
//!   never be requested on the same tick.
//!
//! Every test here fails if a second objective can complete on the same tick.
//! They close `f37.d.limit.one_completion_per_tick` together with the entry's
//! removal from `RULE_LIMITATIONS` — the divergence that entry recorded is
//! gone, and neither half can move without the other.

use cs_script::ir::*;
use cs_script::runtime::*;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

/// Fixed session generation: the event keys are part of what the probes read.
const SESSION: SessionGeneration = SessionGeneration(729);

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

fn reward(key: &str) -> Action {
    Action::GrantReward {
        reward: cid(ContentKind::Blueprint, key),
    }
}

fn objective(id: u32, condition: Condition, actions: Vec<Action>) -> Objective {
    Objective {
        id: SymbolId(id),
        content: cid(ContentKind::Objective, &format!("synthetic-fu3-obj-{id}")),
        condition,
        actions,
        span: None,
    }
}

fn program(objectives: Vec<Objective>) -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-f37-d-fu3"),
        variables: vec![],
        objectives,
    }
}

fn constant_true(id: u32, actions: Vec<Action>) -> Objective {
    objective(id, Condition::Const(true), actions)
}

/// The ids of the objectives that completed on `tick`'s result, in emission
/// order (one entry at most).
fn completed_on(result: &TickResult) -> Vec<u32> {
    result
        .events
        .iter()
        .filter(|event| matches!(event.kind, EventKind::ObjectiveCompleted))
        .map(|event| event.key.source.0)
        .collect()
}

/// Every reward a slice of events granted, in emission order.
fn rewards_granted(events: &[MissionEvent]) -> Vec<ContentId> {
    events
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::RewardGranted(reward) => Some(reward.clone()),
            _ => None,
        })
        .collect()
}

fn dead(actors: &[u32]) -> MissionFacts {
    MissionFacts {
        actors: actors
            .iter()
            .map(|actor| (ActorId(*actor), ActorState::Dead))
            .collect(),
        ..MissionFacts::default()
    }
}

/// **One completion per tick, lowest declaration index first.**
///
/// Four objectives whose conditions all hold from the first tick, declared
/// out of symbol order so a scan keyed by symbol would answer differently.
/// Each tick completes exactly one of them — the first not yet completed in
/// declaration order — and the fifth tick completes nothing, because the rule
/// admits an objective, it never invents one.
#[test]
fn accept_f37_d_fu3_one_objective_completes_per_tick_lowest_declaration_index_first() {
    let p = program(vec![
        constant_true(9, vec![reward("r-9")]),
        constant_true(3, vec![reward("r-3")]),
        constant_true(7, vec![reward("r-7")]),
        constant_true(1, vec![reward("r-1")]),
    ])
    .validate()
    .unwrap();

    let mut state = MissionState::new(&p, SESSION);
    let mut completions = Vec::new();
    let mut granted: Vec<ContentId> = Vec::new();
    for tick in 1..=5u64 {
        let result = state
            .step(&p, &MissionFacts::default(), Tick(tick))
            .unwrap();
        let completed = completed_on(&result);
        assert!(
            completed.len() <= 1,
            "tick {tick} completed more than one objective: {completed:?}"
        );
        granted.extend(rewards_granted(&result.events));
        completions.push(completed);
    }
    assert_eq!(
        completions,
        vec![vec![9], vec![3], vec![7], vec![1], Vec::<u32>::new()],
        "the scan completes the lowest declaration index whose condition holds, \
         one objective per tick"
    );
    // Nothing repeated and nothing skipped: each objective's own reward, once.
    let mut expected: Vec<ContentId> = ["r-9", "r-3", "r-7", "r-1"]
        .iter()
        .map(|key| cid(ContentKind::Blueprint, key))
        .collect();
    expected.sort();
    let mut unique = granted.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), granted.len(), "a reward was granted twice");
    assert_eq!(unique, expected, "the reward set changed with the scan");
}

/// **A satisfied objective that lost the scan waits, and is re-evaluated.**
///
/// Objective 3's condition holds on tick 1 — together with objective 1's, and
/// declared after it — so the scan admits objective 1 and objective 3 stays
/// unfired. That is not a latch: on tick 2 its actor is alive, so it does not
/// complete, and on tick 3 the actor is dead again and it does. Objective 2's
/// condition never holds and it never completes. With the old behaviour —
/// every satisfied objective completing on the same tick — objective 3 would
/// complete on tick 1 and the first assertion below would fail.
#[test]
fn accept_f37_d_fu3_a_satisfied_objective_that_waits_is_re_evaluated_not_latched() {
    let p = program(vec![
        constant_true(1, vec![reward("r-first")]),
        objective(2, Condition::Const(false), vec![reward("r-never")]),
        objective(
            3,
            Condition::ActorIs {
                actor: ActorId(70),
                state: ActorState::Dead,
            },
            vec![reward("r-late")],
        ),
    ])
    .validate()
    .unwrap();

    let mut state = MissionState::new(&p, SESSION);

    // Both conditions hold; only the lower index completes.
    let first = state.step(&p, &dead(&[70]), Tick(1)).unwrap();
    assert_eq!(completed_on(&first), vec![1]);
    assert!(state.is_completed(SymbolId(1)));
    assert!(
        !state.is_completed(SymbolId(3)),
        "objective 3 satisfied on tick 1 but not admitted: it must not be latched"
    );

    // The actor is alive: the waiting objective's condition is false now, so
    // it does not complete — nothing carried over from tick 1.
    let second = state.step(&p, &MissionFacts::default(), Tick(2)).unwrap();
    assert!(
        completed_on(&second).is_empty(),
        "a false condition must not complete an objective that waited"
    );
    assert!(!state.is_completed(SymbolId(3)));

    // Dead again: it completes on this tick, on its own condition, not on a
    // queue it was pushed onto earlier.
    let third = state.step(&p, &dead(&[70]), Tick(3)).unwrap();
    assert_eq!(completed_on(&third), vec![3]);
    assert!(state.is_completed(SymbolId(3)));
    assert!(
        !state.is_completed(SymbolId(2)),
        "a false condition never fires"
    );

    // And the waiting objective's reward was granted exactly once, on the tick
    // it actually completed.
    assert_eq!(
        rewards_granted(&first.events),
        vec![cid(ContentKind::Blueprint, "r-first")]
    );
    assert!(rewards_granted(&second.events).is_empty());
    assert_eq!(
        rewards_granted(&third.events),
        vec![cid(ContentKind::Blueprint, "r-late")]
    );
}

/// **Through objectives, success and failure cannot be requested on one
/// tick** — the measured consequence of the rule (`f37.rule.terminal_precedence`,
/// "so through objectives success and failure can never be requested on one
/// tick").
///
/// Two objectives ask for opposite outcomes and both conditions hold from the
/// first tick on. The scan admits the first, so one request reaches the
/// policy, the measured precedence records it (success iff WON), and the
/// mission latches — the second objective never fires at all, on this tick or
/// any later one.
#[test]
fn accept_f37_d_fu3_objectives_cannot_request_both_outcomes_on_one_tick() {
    let p = program(vec![
        constant_true(1, vec![Action::Finish(Outcome::Succeeded)]),
        constant_true(2, vec![Action::Finish(Outcome::Failed)]),
    ])
    .validate()
    .unwrap();

    let mut state = MissionState::new(&p, SESSION);
    let first = state.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    let requests: Vec<Outcome> = first
        .events
        .iter()
        .filter_map(|event| match event.kind {
            EventKind::TerminalRequested(outcome) => Some(outcome),
            _ => None,
        })
        .collect();
    assert_eq!(
        requests,
        vec![Outcome::Succeeded],
        "only one objective can ask for an outcome on one tick"
    );
    assert_eq!(first.terminal, TerminalState::Succeeded);
    assert!(state.is_completed(SymbolId(1)));
    assert!(
        !state.is_completed(SymbolId(2)),
        "the second objective must never have fired"
    );

    // Latched: the mission is over, so later ticks neither complete it nor
    // request its outcome.
    for tick in 2..=4u64 {
        let later = state
            .step(&p, &MissionFacts::default(), Tick(tick))
            .unwrap();
        assert!(
            later.events.is_empty(),
            "tick {tick} emitted after the mission ended: {:?}",
            later.events
        );
        assert_eq!(later.terminal, TerminalState::Succeeded);
        assert!(!state.is_completed(SymbolId(2)));
    }
}
