//! F-SCRIPT-WAKE-REPEAT (#1524): a block's wake-role effects run on **every**
//! wake, not once per session.
//!
//! The measurement is `2026-10-06-m01-lc-directive-b-…` (the wake call
//! `0x469af0`): a wake from nap or done performs every wake effect except the
//! two dormant-only ones. `docs/findings/2026-10-18-f-script-wake-repeat.md`
//! names the affected content and the measured limits. Nothing here is an
//! original run.

use cs_script::ir::{
    Action, Condition, DirectiveOperation, IR_VERSION, MissionProgram, Objective, SymbolId,
    ValidatedProgram, Value, Variable,
};
use cs_script::runtime::{
    MissionFacts, MissionState, ObjectiveLifecycle, RestoreDefect, RestoreError, SessionGeneration,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

fn program() -> ValidatedProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: ContentId::from_source(ContentKind::Mission, "synthetic").unwrap(),
        variables: vec![],
        objectives: vec![Objective {
            id: SymbolId(1),
            content: ContentId::from_source(ContentKind::Objective, "o1").unwrap(),
            // Never completes: the wake emission is the only path to an effect.
            condition: Condition::Const(false),
            actions: vec![Action::Directive {
                operation: DirectiveOperation::WakeAnimation,
                args: vec![Value::Str("pzhomebase".into())],
            }],
            span: None,
        }],
    }
    .validate()
    .expect("the synthetic wake program validates")
}

fn facts(lifecycle: ObjectiveLifecycle) -> MissionFacts {
    let mut facts = MissionFacts::default();
    facts.objectives.insert(0, lifecycle);
    facts
}

fn step(state: &mut MissionState, p: &ValidatedProgram, tick: u64, l: ObjectiveLifecycle) {
    state.step(p, &facts(l), Tick(tick)).expect("tick steps");
}

use ObjectiveLifecycle::{Awake, Dormant, Napping};

/// **A wake from nap emits the block's wake effects again.**
#[test]
fn accept_wake_repeat_a_wake_from_nap_emits_the_wake_effect_again() {
    let p = program();
    let mut state = MissionState::new(&p, SessionGeneration(1), 0);
    for (tick, l) in [
        (1, Dormant),
        (2, Awake),
        (3, Awake),
        (4, Napping),
        (5, Napping),
    ] {
        step(&mut state, &p, tick, l);
    }
    assert_eq!(state.directives().len(), 1, "the first wake emitted once");
    step(&mut state, &p, 6, Awake);
    step(&mut state, &p, 7, Awake);
    assert_eq!(state.directives().len(), 2, "the re-wake emitted once more");
    let keys: Vec<_> = state
        .directives()
        .iter()
        .map(|e| e.key.execution_key())
        .collect();
    assert_ne!(keys[0], keys[1], "each wake has its own execution key");
    assert_eq!(state.directives()[0].key.tick, Tick(2));
    assert_eq!(state.directives()[1].key.tick, Tick(6));
}

/// **A wake that stays awake emits once**, and the first wake keeps the key
/// the completion path uses (so a completion after it emits nothing twice).
#[test]
fn accept_wake_repeat_staying_awake_does_not_repeat() {
    let p = program();
    let mut state = MissionState::new(&p, SessionGeneration(1), 0);
    for tick in 1..=5 {
        step(&mut state, &p, tick, Awake);
    }
    assert_eq!(state.directives().len(), 1);
    assert_eq!(state.directives()[0].key.sequence, 1);
}

/// **A save taken while awake neither repeats that wake after the restore nor
/// loses the next one**: the wake ledger is in the record.
#[test]
fn accept_wake_repeat_the_wake_ledger_survives_a_save_record() {
    let p = program();
    let mut state = MissionState::new(&p, SessionGeneration(1), 0);
    for (tick, l) in [(1, Awake), (2, Napping), (3, Awake)] {
        step(&mut state, &p, tick, l);
    }
    assert_eq!(state.directives().len(), 2);
    let mut restored = MissionState::restore(&p, state.snapshot(&p)).expect("restores");
    step(&mut restored, &p, 4, Awake);
    assert_eq!(
        restored.directives().len(),
        2,
        "the wake it was on is not repeated"
    );
    step(&mut restored, &p, 5, Napping);
    step(&mut restored, &p, 6, Awake);
    assert_eq!(restored.directives().len(), 3, "the next wake still fires");
    // A session that never stopped agrees.
    for (tick, l) in [(4, Awake), (5, Napping), (6, Awake)] {
        step(&mut state, &p, tick, l);
    }
    assert_eq!(state.directives(), restored.directives());
}

/// **A re-wake's key never aliases a scheduled item's.** Item sequences pack
/// as `(ordinal + 1) * SEQS_PER_ITEM + (action + 1)` — never a multiple of
/// `SEQS_PER_ITEM` — while the completion path holds `0..=64`; a re-wake's
/// occurrence key is a multiple of `SEQS_PER_ITEM` above both, so a session
/// that keeps rescheduling one block emits the item's effect and the
/// re-wake's on the same tick. A packing into the same space (the retired
/// `(occurrence << 16) | (action + 1)`) lets the two meet — the program
/// below reaches it exactly: the item's 1008th ordinal emits `1008 * 65 +
/// 17` = 65537 at its action 16, which the second wake's `(1 << 16) | 1`
/// would have been.
#[test]
fn accept_wake_repeat_a_rewake_never_aliases_a_scheduled_items_key() {
    // The item: sixteen writes, its directive at action 16, then the
    // `Reschedule` that re-queues the whole list one ordinal further each
    // tick. The block completes on its first tick and never sleeps.
    let mut item_actions: Vec<Action> = (0..16)
        .map(|n| Action::SetVariable {
            variable: SymbolId(100),
            value: Value::Int(n),
        })
        .collect();
    item_actions.push(Action::Directive {
        operation: DirectiveOperation::CompletedSoundGroup,
        args: vec![],
    });
    item_actions.push(Action::Reschedule { delay_ticks: 1 });
    let p = MissionProgram {
        version: IR_VERSION,
        mission: ContentId::from_source(ContentKind::Mission, "synthetic").unwrap(),
        variables: vec![Variable {
            id: SymbolId(100),
            name: "v".into(),
            initial: Value::Int(0),
        }],
        objectives: vec![Objective {
            id: SymbolId(1),
            content: ContentId::from_source(ContentKind::Objective, "o1").unwrap(),
            condition: Condition::Const(true),
            actions: vec![
                Action::Directive {
                    operation: DirectiveOperation::WakeAnimation,
                    args: vec![Value::Str("anim".into())],
                },
                Action::Schedule {
                    delay_ticks: 1,
                    actions: item_actions,
                },
            ],
            span: None,
        }],
    }
    .validate()
    .expect("the synthetic reschedule program validates");

    let mut state = MissionState::new(&p, SessionGeneration(1), 0);
    // Tick 1 wakes and completes the block — the wake site emits under the
    // completion path's own key — and arms the item for tick 2.
    step(&mut state, &p, 1, Awake);
    // Napping ticks carry the item to its 1008th ordinal.
    for tick in 2..=1008 {
        step(&mut state, &p, tick, Napping);
    }
    // Tick 1009: the second wake and the item's `1008 * 65 + 17` emission
    // share the tick; they must not share a key.
    step(&mut state, &p, 1009, Awake);
    let emitted_at_1009 = |operation| {
        state
            .directives()
            .iter()
            .any(|e| e.key.tick == Tick(1009) && e.operation == operation)
    };
    assert!(
        emitted_at_1009(DirectiveOperation::WakeAnimation),
        "the re-wake emitted"
    );
    assert!(
        emitted_at_1009(DirectiveOperation::CompletedSoundGroup),
        "the item's own emission was not aliased away"
    );
}

/// **A facts row for a block the program does not carry leaves the save
/// record restorable** — it can wake nothing, so it is not a wake the ledger
/// owes a count.
#[test]
fn accept_wake_repeat_an_unknown_facts_row_still_snapshots_clean() {
    let p = program();
    let mut state = MissionState::new(&p, SessionGeneration(1), 0);
    let mut facts = MissionFacts::default();
    facts.objectives.insert(0, Awake);
    facts.objectives.insert(9, Awake); // the program has one block
    state.step(&p, &facts, Tick(1)).expect("tick steps");
    let mut restored = MissionState::restore(&p, state.snapshot(&p)).expect("restores");
    step(&mut restored, &p, 2, Awake);
    assert_eq!(restored.directives().len(), 1, "no phantom re-wake");
}

/// **A forged wake ledger is refused**, not trusted.
#[test]
fn accept_wake_repeat_a_forged_wake_ledger_is_refused() {
    let p = program();
    let mut state = MissionState::new(&p, SessionGeneration(1), 0);
    step(&mut state, &p, 1, Awake);
    let intact = state.snapshot(&p);
    for forge in [
        |s: &mut cs_script::runtime::MissionStateSnapshot| s.wakes = vec![(7, 1)],
        |s: &mut cs_script::runtime::MissionStateSnapshot| s.wakes = vec![(0, 0)],
        |s: &mut cs_script::runtime::MissionStateSnapshot| s.wakes.clear(),
    ] {
        let mut record = intact.clone();
        forge(&mut record);
        assert!(matches!(
            MissionState::restore(&p, record),
            Err(RestoreError::Corrupt {
                defect: RestoreDefect::WakeLedger { .. }
            })
        ));
    }
    assert!(MissionState::restore(&p, intact).is_ok());
}
