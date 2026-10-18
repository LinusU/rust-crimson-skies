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
    ValidatedProgram, Value,
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
