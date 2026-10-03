//! F37-C acceptance: the versioned save record of a live mission session.
//!
//! Synthetic fixture only; no original data and no original semantics claimed.
//! The minimum scenario is AC03: save/restore at a pending timer preserves the
//! exact remaining ticks. The failure cases are the record refusals — a save
//! that silently half-applied would change which events fire, which is the one
//! thing an execution key exists to prevent.

use cs_script::ir::*;
use cs_script::runtime::*;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

/// A reward intent the deferred item carries; the identity of the emitted
/// event is what these tests observe.
fn reward(key: &str) -> Action {
    Action::GrantReward {
        reward: cid(ContentKind::Blueprint, key),
    }
}

/// A delay of `DELAY` ticks, long enough that the item is still queued when
/// the snapshot is taken.
const DELAY: u64 = 4;

fn program(objectives: Vec<Objective>) -> ValidatedProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-f37c"),
        variables: vec![
            Variable {
                id: SymbolId(100),
                name: "roll".into(),
                initial: Value::Int(0),
            },
            Variable {
                id: SymbolId(101),
                name: "marks".into(),
                initial: Value::Int(0),
            },
        ],
        objectives,
    }
    .validate()
    .unwrap()
}

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

fn objective(id: u32, condition: Condition, actions: Vec<Action>) -> Objective {
    Objective {
        id: SymbolId(id),
        content: cid(ContentKind::Objective, &format!("synthetic-obj-{id}")),
        condition,
        actions,
        span: None,
    }
}

/// One delayed reward item, the AC03 fixture.
fn delayed_reward_program() -> ValidatedProgram {
    program(vec![objective(
        1,
        Condition::Const(true),
        vec![Action::Schedule {
            delay_ticks: DELAY,
            actions: vec![reward("r-delayed")],
        }],
    )])
}

fn state(program: &ValidatedProgram) -> MissionState {
    MissionState::new(program, SessionGeneration(9))
}

/// Runs the fixture from tick 1 to `until` and returns every emitted reward
/// key, so a restored run can be compared against an uninterrupted one.
fn reward_keys_while_running(program: &ValidatedProgram, until: u64) -> Vec<(u64, u32, u32)> {
    let mut s = state(program);
    let mut keys = Vec::new();
    for tick in 1..=until {
        let result = s
            .step(program, &MissionFacts::default(), Tick(tick))
            .unwrap();
        keys.extend(result.events.iter().filter_map(|e| match &e.kind {
            EventKind::RewardGranted(_) => Some((tick, e.key.source.0, e.key.sequence)),
            _ => None,
        }));
    }
    keys
}

/// AC03: save/restore at a pending timer preserves the exact remaining ticks.
///
/// The restored session must not fire early, must not fire late, and must emit
/// the same execution keys the uninterrupted session emitted.
#[test]
fn accept_f37_c_save_restore_at_pending_timer_preserves_remaining_ticks() {
    let p = delayed_reward_program();
    let mut live = state(&p);
    live.step(&p, &MissionFacts::default(), Tick(1)).unwrap();

    // One item, due at tick 5, four ticks left after the tick evaluated last.
    let before = live.pending_timers();
    let [timer] = before.as_slice() else {
        panic!("one queued item expected, got {before:?}");
    };
    assert_eq!(timer.due, Tick(1 + DELAY));
    assert_eq!(
        timer.remaining, DELAY,
        "four ticks left, not a rounded value"
    );
    assert_eq!(timer.actions_remaining(), 1);

    // The record itself carries the remaining ticks, not just the queue.
    let record = live.snapshot(&p);
    assert_eq!(record.pending_timers(), before);
    assert_eq!(record.version, SNAPSHOT_VERSION);
    assert_eq!(record.mission, cid(ContentKind::Mission, "synthetic-f37c"));

    let mut restored = MissionState::restore(&p, record).unwrap();
    assert_eq!(restored.pending_timers(), before, "restore lost the queue");
    assert_eq!(restored.queued_items(), 1);

    // A retry of the snapshot tick is refused, so a restore cannot re-evaluate
    // the tick whose events the record already consumed.
    assert_eq!(
        restored.step(&p, &MissionFacts::default(), Tick(1)),
        Err(TickError::NotAdvancing {
            last: Tick(1),
            given: Tick(1)
        })
    );

    // The remaining ticks count down exactly and the item fires on tick 5.
    for tick in 2..=4 {
        let result = restored
            .step(&p, &MissionFacts::default(), Tick(tick))
            .unwrap();
        assert_eq!(result.events.len(), 0, "due is tick 5, not {tick}");
        assert_eq!(restored.pending_timers()[0].remaining, 5 - tick);
    }
    let t5 = restored
        .step(&p, &MissionFacts::default(), Tick(5))
        .unwrap();
    assert_eq!(
        t5.events
            .iter()
            .filter_map(|e| match &e.kind {
                EventKind::RewardGranted(_) => Some(e.key.sequence),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec![item_sequence_of_first_item()]
    );
    assert!(restored.pending_timers().is_empty());

    // Identical to a session that was never saved.
    assert_eq!(
        reward_keys_while_running(&p, 6),
        vec![(5, 1, item_sequence_of_first_item())]
    );
}

/// The first scheduled item of a session gets ordinal 0, so its first action
/// carries `sequence = (0 + 1) * 65 + 1`.
fn item_sequence_of_first_item() -> u32 {
    65 + 1
}

/// The restored RNG stream continues the same session's stream: a save must not
/// re-seed the evaluator, or every later draw would change.
#[test]
fn accept_f37_c_restore_continues_the_explicit_rng_stream() {
    // `marks` steps 1 -> 2 -> 3, so each objective fires on the tick after the
    // previous one's write became visible; exactly one draw happens per tick.
    let draw = Action::Draw {
        variable: SymbolId(100),
        min: -1_000_000,
        max: 1_000_000,
    };
    let mark = |n: i32| Action::SetVariable {
        variable: SymbolId(101),
        value: Value::Int(n),
    };
    let marks = |n: i32| Objective {
        id: SymbolId(200 + n as u32),
        content: cid(ContentKind::Objective, &format!("synthetic-mark-{n}")),
        condition: Condition::Compare {
            variable: SymbolId(101),
            op: CompareOp::Eq,
            value: Value::Int(n),
        },
        actions: vec![mark(n + 1), draw.clone()],
        span: None,
    };
    let p = program(vec![
        objective(1, Condition::Const(true), vec![mark(1), draw.clone()]),
        marks(1),
        marks(2),
    ]);

    // Uninterrupted reference run: the drawn value after each of three ticks.
    let mut reference = state(&p);
    let mut expected = Vec::new();
    for tick in 1..=3 {
        reference
            .step(&p, &MissionFacts::default(), Tick(tick))
            .unwrap();
        expected.push(reference.variable(SymbolId(100)).cloned().unwrap());
    }
    assert!(
        expected.windows(2).all(|w| w[0] != w[1]),
        "the fixture must draw three different values, got {expected:?}"
    );

    // The same run, saved after tick 2 and restored: tick 3 draws from the
    // restored stream and must reproduce the third reference value.
    let mut live = state(&p);
    let mut actual = Vec::new();
    for tick in 1..=2 {
        live.step(&p, &MissionFacts::default(), Tick(tick)).unwrap();
        actual.push(live.variable(SymbolId(100)).cloned().unwrap());
    }
    let record = live.snapshot(&p);
    assert_eq!(record.rng_draws, 2, "one draw per tick before the save");
    let mut restored = MissionState::restore(&p, record).unwrap();
    restored
        .step(&p, &MissionFacts::default(), Tick(3))
        .unwrap();
    actual.push(restored.variable(SymbolId(100)).cloned().unwrap());
    assert_eq!(actual, expected, "the restored stream diverged");
}

/// A work-budget stop leaves an item half executed. A save taken there must
/// resume at the same action: restarting the item would repeat the work already
/// committed (a second reward) and skipping it would lose the rest.
#[test]
fn accept_f37_c_restore_resumes_a_budget_stopped_item_at_its_cursor() {
    let p = program(vec![objective(
        1,
        Condition::Const(true),
        vec![Action::Schedule {
            delay_ticks: 0,
            actions: vec![reward("r-first"), reward("r-second")],
        }],
    )]);
    // fire 1 + schedule 1 + dequeue 1 + first action 1 = 4: the stop lands on
    // the item's second action.
    let mut live = state(&p);
    live.set_limits(WorkLimits {
        max_work_per_tick: 4,
        ..WorkLimits::default()
    });
    let t1 = live.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    assert!(matches!(
        t1.stop,
        Some(StopReason::WorkBudget { spent: 4, .. })
    ));
    let timers = live.pending_timers();
    let [timer] = timers.as_slice() else {
        panic!("one resumed item expected, got {timers:?}");
    };
    assert_eq!(timer.next, 1, "the first action already ran");
    assert_eq!(timer.actions_remaining(), 1);

    let mut restored = MissionState::restore(&p, live.snapshot(&p)).unwrap();
    assert_eq!(restored.pending_timers(), timers);
    let t2 = restored
        .step(&p, &MissionFacts::default(), Tick(2))
        .unwrap();
    assert!(t2.stop.is_none());
    let rewards: Vec<_> = t2
        .events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::RewardGranted(r) => Some(r.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        rewards,
        [cid(ContentKind::Blueprint, "r-second")],
        "the second action runs once and the first is not repeated"
    );
}

/// A record is data from outside the process: a foreign or older one is refused
/// whole, never partially applied.
#[test]
fn accept_f37_c_restore_refuses_a_foreign_or_incompatible_record() {
    let p = delayed_reward_program();
    let mut live = state(&p);
    live.step(&p, &MissionFacts::default(), Tick(1)).unwrap();

    let mut older = live.snapshot(&p);
    older.version = SNAPSHOT_VERSION + 1;
    assert_eq!(
        MissionState::restore(&p, older),
        Err(RestoreError::SnapshotVersion {
            found: SNAPSHOT_VERSION + 1
        })
    );

    let mut foreign = live.snapshot(&p);
    foreign.mission = cid(ContentKind::Mission, "synthetic-other-mission");
    assert_eq!(
        MissionState::restore(&p, foreign),
        Err(RestoreError::MissionMismatch {
            expected: cid(ContentKind::Mission, "synthetic-f37c"),
            found: cid(ContentKind::Mission, "synthetic-other-mission"),
        })
    );

    // A variable the program does not declare, and one whose stored type no
    // condition could compare against.
    let mut undeclared = live.snapshot(&p);
    undeclared.variables.push((SymbolId(999), Value::Int(1)));
    assert_eq!(
        MissionState::restore(&p, undeclared),
        Err(RestoreError::UnknownVariable {
            symbol: SymbolId(999)
        })
    );
    let mut mistyped = live.snapshot(&p);
    mistyped.variables = vec![(SymbolId(100), Value::Bool(true))];
    assert_eq!(
        MissionState::restore(&p, mistyped),
        Err(RestoreError::TypeMismatch {
            symbol: SymbolId(100),
            expected: ValueType::Int,
            found: ValueType::Bool,
        })
    );

    // An objective the program no longer declares.
    let mut unknown_latch = live.snapshot(&p);
    unknown_latch.completed.push(SymbolId(42));
    assert_eq!(
        MissionState::restore(&p, unknown_latch),
        Err(RestoreError::UnknownObjective {
            symbol: SymbolId(42)
        })
    );

    // A session that took more draws than the restore bound refuses rather than
    // replaying them.
    let mut long = live.snapshot(&p);
    long.rng_draws = MAX_RNG_REPLAY_DRAWS + 1;
    assert_eq!(
        MissionState::restore(&p, long),
        Err(RestoreError::RngReplayTooLong {
            draws: MAX_RNG_REPLAY_DRAWS + 1
        })
    );
}

/// A broken pending queue is refused with its defect. Each of these would
/// change which events fire if it were restored: colliding or unallocated
/// ordinals make the exactly-once guard swallow another item's events, an
/// out-of-range cursor skips or repeats actions, and an out-of-order queue
/// drains in an order the record does not claim.
#[test]
fn accept_f37_c_restore_refuses_a_corrupt_pending_queue() {
    let p = program(vec![objective(
        1,
        Condition::Const(true),
        vec![
            Action::Schedule {
                delay_ticks: 3,
                actions: vec![reward("r-a")],
            },
            Action::Schedule {
                delay_ticks: 5,
                actions: vec![reward("r-b")],
            },
        ],
    )]);
    let mut live = state(&p);
    live.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    let good = live.snapshot(&p);
    assert_eq!(good.pending.len(), 2);
    assert_eq!(good.next_item_ordinal, 2);

    // Ordinals shared by two items.
    let mut twin = good.clone();
    twin.pending[1].ordinal = twin.pending[0].ordinal;
    assert_eq!(
        MissionState::restore(&p, twin),
        Err(RestoreError::Corrupt {
            defect: RestoreDefect::DuplicateOrdinal { ordinal: 0 },
        })
    );

    // An ordinal this session never handed out.
    let mut unborn = good.clone();
    unborn.pending[1].ordinal = good.next_item_ordinal;
    assert_eq!(
        MissionState::restore(&p, unborn),
        Err(RestoreError::Corrupt {
            defect: RestoreDefect::OrdinalNotAllocated { ordinal: 2 },
        })
    );

    // A cursor outside the item's action list.
    let mut cursor = good.clone();
    cursor.pending[0].next = 2;
    assert_eq!(
        MissionState::restore(&p, cursor),
        Err(RestoreError::Corrupt {
            defect: RestoreDefect::ActionCursor {
                next: 2,
                actions: 1
            },
        })
    );

    // A queue whose due ticks run backwards.
    let mut unordered = good.clone();
    unordered.pending.swap(0, 1);
    assert_eq!(
        MissionState::restore(&p, unordered),
        Err(RestoreError::Corrupt {
            defect: RestoreDefect::PendingOrder {
                previous: Tick(6),
                given: Tick(4),
            },
        })
    );

    // A consumed key from another session would dedupe this session's events.
    let mut foreign_key = good.clone();
    foreign_key.consumed.push(ExecutionKey {
        session: SessionGeneration(10),
        source: SymbolId(1),
        sequence: 1,
    });
    assert_eq!(
        MissionState::restore(&p, foreign_key),
        Err(RestoreError::Corrupt {
            defect: RestoreDefect::ForeignExecutionKey {
                session: SessionGeneration(10)
            },
        })
    );

    // The intact record still restores, so the refusals above are the defects
    // and not a fixture that never restores.
    let mut restored = MissionState::restore(&p, good).unwrap();
    let t4 = restored
        .step(&p, &MissionFacts::default(), Tick(4))
        .unwrap();
    assert_eq!(
        t4.events
            .iter()
            .filter(|e| e.kind == EventKind::RewardGranted(cid(ContentKind::Blueprint, "r-a")))
            .count(),
        1
    );
    let t6 = restored
        .step(&p, &MissionFacts::default(), Tick(6))
        .unwrap();
    assert_eq!(
        t6.events
            .iter()
            .filter(|e| e.kind == EventKind::RewardGranted(cid(ContentKind::Blueprint, "r-b")))
            .count(),
        1
    );
}

/// A queued item's action list comes out of the record, not out of the program,
/// so it is the one piece of the record the evaluator would otherwise run on
/// trust. Each case below is an action pre-launch validation refuses; before
/// this was checked, restoring such a record either aborted the process
/// (`Unknown` hits the evaluator's unreachable, an empty `Draw` range divides
/// by zero) or wrote a variable the program never declared, which made the
/// *next* save of the restored session refusable.
#[test]
fn accept_f37_c_restore_refuses_a_record_whose_deferred_work_cannot_run() {
    let p = program(vec![objective(
        1,
        Condition::Const(true),
        vec![Action::Schedule {
            delay_ticks: DELAY,
            actions: vec![reward("r-delayed")],
        }],
    )]);
    let mut live = state(&p);
    live.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    let good = live.snapshot(&p);
    assert_eq!(good.pending.len(), 1);

    // The first queued item's actions, replaced by something validation would
    // have refused before launch.
    let with_deferred = |actions: Vec<Action>| {
        let mut record = good.clone();
        record.pending[0].actions = actions;
        MissionState::restore(&p, record)
    };

    // An undecodable instruction: the evaluator treats `Unknown` as impossible.
    assert!(matches!(
        with_deferred(vec![Action::Unknown {
            instruction: "op".into()
        }]),
        Err(RestoreError::DeferredActions {
            ordinal: 0,
            error: ValidationError::UnsupportedInstruction { .. }
        })
    ));

    // An empty draw range: `span` would be zero.
    assert!(matches!(
        with_deferred(vec![Action::Draw {
            variable: SymbolId(100),
            min: 1,
            max: 0,
        }]),
        Err(RestoreError::DeferredActions {
            ordinal: 0,
            error: ValidationError::InvalidRange { .. }
        })
    ));

    // A write to a variable this program does not declare.
    assert!(matches!(
        with_deferred(vec![Action::SetVariable {
            variable: SymbolId(999),
            value: Value::Int(1),
        }]),
        Err(RestoreError::DeferredActions {
            ordinal: 0,
            error: ValidationError::UnknownVariable { .. }
        })
    ));

    // Longer than any objective's list may be, so a record cannot smuggle in
    // work the launch-time cap exists to bound.
    let long = (0..=MAX_ACTIONS_PER_OBJECTIVE)
        .map(|_| reward("r-delayed"))
        .collect();
    assert!(matches!(
        with_deferred(long),
        Err(RestoreError::DeferredActions {
            ordinal: 0,
            error: ValidationError::TooManyActions { .. }
        })
    ));

    // A queue past the cap the live path enforces.
    let mut crowded = good.clone();
    let next = crowded.next_item_ordinal;
    crowded.pending = (0..=MAX_PENDING_ITEMS)
        .map(|i| cs_script::runtime::ScheduledWork {
            source: SymbolId(1),
            ordinal: next + i as u32,
            due: Tick(DELAY + 2 + i as u64),
            next: 0,
            actions: vec![reward("r-delayed")],
        })
        .collect();
    crowded.next_item_ordinal = next + MAX_PENDING_ITEMS as u32 + 1;
    assert_eq!(
        MissionState::restore(&p, crowded),
        Err(RestoreError::Corrupt {
            defect: RestoreDefect::PendingQueueTooLong {
                count: MAX_PENDING_ITEMS + 1,
                allowed: MAX_PENDING_ITEMS,
            },
        })
    );

    // A record that drops or duplicates a declared variable. Every state of this
    // program holds every declared variable, and a condition on an absent one
    // is false, so a dropped variable would disarm the mission silently.
    let mut dropped = good.clone();
    dropped
        .variables
        .retain(|(symbol, _)| *symbol != SymbolId(100));
    assert_eq!(
        MissionState::restore(&p, dropped),
        Err(RestoreError::Corrupt {
            defect: RestoreDefect::MissingVariable {
                symbol: SymbolId(100)
            },
        })
    );
    let mut doubled = good.clone();
    doubled.variables.push((SymbolId(100), Value::Int(7)));
    assert_eq!(
        MissionState::restore(&p, doubled),
        Err(RestoreError::Corrupt {
            defect: RestoreDefect::DuplicateVariable {
                symbol: SymbolId(100)
            },
        })
    );

    // The intact record still restores and its deferred reward still fires, so
    // the refusals above are the defects and not a fixture that never restores.
    let mut restored = MissionState::restore(&p, good).unwrap();
    let t5 = restored
        .step(&p, &MissionFacts::default(), Tick(1 + DELAY))
        .unwrap();
    assert_eq!(
        t5.events
            .iter()
            .filter(|e| e.kind == EventKind::RewardGranted(cid(ContentKind::Blueprint, "r-delayed")))
            .count(),
        1
    );
}

/// The work and queue bounds are engine policy, not gameplay state: a record
/// may keep the host's tighter budget across a save, but it may not install a
/// looser one than the engine's own maximum, and a queue within the cap still
/// runs.
#[test]
fn accept_f37_c_restore_keeps_the_bounds_within_the_engine_maximum() {
    let p = program(vec![objective(
        1,
        Condition::Const(true),
        vec![Action::Schedule {
            delay_ticks: DELAY,
            actions: vec![reward("r-delayed")],
        }],
    )]);
    let mut live = state(&p);
    live.set_limits(WorkLimits {
        max_work_per_tick: 8,
        max_pending_items: 4,
    });
    live.step(&p, &MissionFacts::default(), Tick(1)).unwrap();

    // A host-tightened budget survives the save, because it is the host's.
    let tight = live.snapshot(&p);
    assert_eq!(
        MissionState::restore(&p, tight.clone())
            .unwrap()
            .snapshot(&p)
            .limits,
        tight.limits
    );

    // A record claiming unbounded work and an unbounded queue is clamped to the
    // engine's own maxima instead of being obeyed.
    let mut greedy = tight.clone();
    greedy.limits = WorkLimits {
        max_work_per_tick: u64::MAX,
        max_pending_items: usize::MAX,
    };
    let mut restored = MissionState::restore(&p, greedy).unwrap();
    assert_eq!(
        restored.snapshot(&p).limits,
        WorkLimits {
            max_work_per_tick: MAX_WORK_PER_TICK,
            max_pending_items: MAX_PENDING_ITEMS,
        },
        "a save file may tighten the engine's bounds, never lift them"
    );
    let t5 = restored
        .step(&p, &MissionFacts::default(), Tick(1 + DELAY))
        .unwrap();
    assert_eq!(
        t5.events
            .iter()
            .filter(|e| e.kind == EventKind::RewardGranted(cid(ContentKind::Blueprint, "r-delayed")))
            .count(),
        1
    );
}

/// Teardown: once a mission is over, deferred work is dropped instead of being
/// left to fire on a later tick or to survive into the next save.
#[test]
fn accept_f37_c_teardown_drops_deferred_work() {
    let p = delayed_reward_program();
    let mut s = state(&p);
    s.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    assert_eq!(s.queued_items(), 1);
    assert_eq!(s.teardown(), 1, "one item was dropped");
    assert_eq!(s.queued_items(), 0);
    assert!(s.pending_timers().is_empty());
    // Teardown is idempotent.
    assert_eq!(s.teardown(), 0);
    let t2 = s.step(&p, &MissionFacts::default(), Tick(2)).unwrap();
    assert!(t2.events.is_empty(), "dropped work must not fire");
    // And it does not come back in the record.
    assert!(s.snapshot(&p).pending.is_empty());
}
