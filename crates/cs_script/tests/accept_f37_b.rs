//! F37-B acceptance: bounded evaluator and the deferred-work event queue.
//! Synthetic fixture only; no original data and no original semantics claimed.

use cs_script::ir::*;
use cs_script::runtime::*;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

fn objective(id: u32, actions: Vec<Action>) -> Objective {
    Objective {
        id: SymbolId(id),
        content: cid(ContentKind::Objective, &format!("synthetic-obj-{id}")),
        condition: Condition::Const(true),
        actions,
        span: None,
    }
}

fn program(objectives: Vec<Objective>) -> ValidatedProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-f37b"),
        variables: vec![Variable {
            id: SymbolId(100),
            name: "acc".into(),
            initial: Value::Int(0),
        }],
        objectives,
    }
    .validate()
    .unwrap()
}

fn reward(key: &str) -> Action {
    Action::GrantReward {
        reward: cid(ContentKind::Blueprint, key),
    }
}

fn state(program: &ValidatedProgram, work: u64, pending: usize) -> MissionState {
    let mut state = MissionState::new(program, SessionGeneration(1));
    state.set_limits(WorkLimits {
        max_work_per_tick: work,
        max_pending_items: pending,
    });
    state
}

fn event_seqs(result: &TickResult) -> Vec<u32> {
    result.events.iter().map(|e| e.key.sequence).collect()
}

fn reward_events(result: &TickResult) -> Vec<&ContentId> {
    result
        .events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::RewardGranted(r) => Some(r),
            _ => None,
        })
        .collect()
}

/// AC02: a zero-delay action that schedules itself hits the work budget
/// instead of hanging. Without the budget this `step` never returns.
#[test]
fn accept_f37_b_zero_delay_self_schedule_hits_work_budget() {
    // `Reschedule` re-queues the list it lives in; delay 0 means this tick.
    let p = program(vec![objective(
        1,
        vec![Action::Reschedule { delay_ticks: 0 }],
    )]);
    let mut s = state(&p, 8, MAX_PENDING_ITEMS);
    let r = s.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    let Some(StopReason::WorkBudget { at, spent }) = r.stop else {
        panic!("expected a work-budget stop, got {r:?}");
    };
    assert_eq!(spent, 8);
    // The diagnostic names the mission and the owning symbol.
    assert_eq!(at.mission, "mission/synthetic-f37b");
    assert_eq!(at.objective, Some(SymbolId(1)));
    assert!(!at.trace.is_empty());
    // The objective fired once; the self-scheduled item emits nothing.
    assert_eq!(r.events.len(), 1);
    assert_eq!(r.events[0].kind, EventKind::ObjectiveCompleted);
    assert_eq!(r.terminal, TerminalState::Running);
    // One item is always outstanding: the mission stays bounded forever.
    assert_eq!(s.queued_items(), 1);
    let again = s.step(&p, &MissionFacts::default(), Tick(2)).unwrap();
    assert!(matches!(again.stop, Some(StopReason::WorkBudget { .. })));
    assert!(again.events.is_empty(), "rescheduling emits no new events");
}

/// A delayed work item runs on its due tick — and not before.
#[test]
fn accept_f37_b_delayed_item_runs_on_due_tick_and_not_before() {
    let p = program(vec![objective(
        1,
        vec![Action::Schedule {
            delay_ticks: 3,
            actions: vec![reward("r-delayed")],
        }],
    )]);
    let mut s = state(&p, MAX_WORK_PER_TICK, MAX_PENDING_ITEMS);
    let t1 = s.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    assert!(t1.stop.is_none() && t1.events.len() == 1);
    assert_eq!(s.queued_items(), 1);
    for tick in 2..4 {
        let r = s.step(&p, &MissionFacts::default(), Tick(tick)).unwrap();
        assert!(r.events.is_empty(), "due is tick 4, not {tick}");
    }
    let t4 = s.step(&p, &MissionFacts::default(), Tick(4)).unwrap();
    assert!(t4.stop.is_none());
    assert_eq!(s.queued_items(), 0);
    // The item's event: tick 4, source = the scheduling objective, sequence
    // lives above the objective's own 0..=64 space.
    let [event] = t4.events.as_slice() else {
        panic!("one reward event expected");
    };
    assert_eq!(
        event.kind,
        EventKind::RewardGranted(cid(ContentKind::Blueprint, "r-delayed"))
    );
    assert_eq!((event.key.tick, event.key.source), (Tick(4), SymbolId(1)));
    assert!(event.key.sequence > MAX_ACTIONS_PER_OBJECTIVE as u32);
}

/// A zero-delay item still runs this tick — after the work already queued.
#[test]
fn accept_f37_b_zero_delay_item_runs_same_tick_after_queued_work() {
    let p = program(vec![objective(
        1,
        vec![Action::Schedule {
            delay_ticks: 0,
            actions: vec![reward("r-zero")],
        }],
    )]);
    let mut s = state(&p, MAX_WORK_PER_TICK, MAX_PENDING_ITEMS);
    let r = s.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    assert!(r.stop.is_none());
    assert_eq!(reward_events(&r), [&cid(ContentKind::Blueprint, "r-zero")]);
    // Completion first, then the item's event: sorted by key.
    assert!(r.events.windows(2).all(|w| w[0].key < w[1].key));
    assert_eq!(s.queued_items(), 0);
}

/// `State` writes from a scheduled item apply at end of tick: conditions see
/// them starting next tick, never in the tick that produced them.
#[test]
fn accept_f37_b_scheduled_write_is_visible_next_tick() {
    let p = program(vec![
        objective(
            1,
            vec![Action::Schedule {
                delay_ticks: 0,
                actions: vec![Action::SetVariable {
                    variable: SymbolId(100),
                    value: Value::Int(1),
                }],
            }],
        ),
        Objective {
            id: SymbolId(2),
            content: cid(ContentKind::Objective, "synthetic-obj-2"),
            condition: Condition::Compare {
                variable: SymbolId(100),
                op: CompareOp::Ge,
                value: Value::Int(1),
            },
            actions: vec![reward("r-woke")],
            span: None,
        },
    ]);
    let mut s = state(&p, MAX_WORK_PER_TICK, MAX_PENDING_ITEMS);
    let t1 = s.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    assert!(!s.is_completed(SymbolId(2)), "the write lands end-of-tick");
    assert!(t1.events.iter().all(|e| e.key.source != SymbolId(2)));
    assert_eq!(s.variable(SymbolId(100)), Some(&Value::Int(1)));
    let t2 = s.step(&p, &MissionFacts::default(), Tick(2)).unwrap();
    assert_eq!(reward_events(&t2), [&cid(ContentKind::Blueprint, "r-woke")]);
}

/// A budget stop mid-item re-queues the unexecuted rest: next tick resumes
/// at the next action. Nothing is skipped and nothing is repeated.
#[test]
fn accept_f37_b_budget_stop_resumes_mid_item_without_skip_or_repeat() {
    let p = program(vec![objective(
        1,
        vec![Action::Schedule {
            delay_ticks: 0,
            actions: vec![reward("r-first"), reward("r-second")],
        }],
    )]);
    // fire 1 + schedule 1 + dequeue 1 + first action 1 = 4: stops on the
    // item's second action.
    let mut s = state(&p, 4, MAX_PENDING_ITEMS);
    let t1 = s.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    assert!(matches!(
        t1.stop,
        Some(StopReason::WorkBudget { spent: 4, .. })
    ));
    assert_eq!(
        reward_events(&t1),
        [&cid(ContentKind::Blueprint, "r-first")]
    );
    assert_eq!(s.queued_items(), 1, "the remainder is still queued");
    let t2 = s.step(&p, &MissionFacts::default(), Tick(2)).unwrap();
    assert!(t2.stop.is_none());
    // The second reward emits exactly once; the first does not repeat.
    assert_eq!(
        reward_events(&t2),
        [&cid(ContentKind::Blueprint, "r-second")]
    );
    assert_eq!(s.queued_items(), 0);
}

/// Resume must not restart an item at action 0: an already-executed
/// `Schedule` would enqueue a *second* copy of its work item — a duplicate
/// the event-key dedupe cannot mask, since the copy gets a fresh ordinal.
#[test]
fn accept_f37_b_resumed_item_does_not_rerun_completed_schedule() {
    let p = program(vec![objective(
        1,
        vec![Action::Schedule {
            delay_ticks: 0,
            actions: vec![
                Action::Schedule {
                    delay_ticks: 2,
                    actions: vec![reward("r-inner")],
                },
                reward("r-outer"),
            ],
        }],
    )]);
    // fire 1 + schedule 1 + dequeue 1 + inner schedule 1 = 4: stops before
    // the item's second action, with the inner item already stored.
    let mut s = state(&p, 4, MAX_PENDING_ITEMS);
    let t1 = s.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    assert!(matches!(
        t1.stop,
        Some(StopReason::WorkBudget { spent: 4, .. })
    ));
    assert_eq!(s.queued_items(), 2, "inner item plus the remainder");
    let t2 = s.step(&p, &MissionFacts::default(), Tick(2)).unwrap();
    assert_eq!(
        reward_events(&t2),
        [&cid(ContentKind::Blueprint, "r-outer")],
        "the remainder resumes at action 1, it does not re-schedule"
    );
    assert_eq!(s.queued_items(), 1);
    let t3 = s.step(&p, &MissionFacts::default(), Tick(3)).unwrap();
    assert_eq!(
        reward_events(&t3),
        [&cid(ContentKind::Blueprint, "r-inner")]
    );
    let t4 = s.step(&p, &MissionFacts::default(), Tick(4)).unwrap();
    assert!(
        reward_events(&t4).is_empty(),
        "a re-run `Schedule` would have queued a second r-inner for tick 4"
    );
    assert_eq!(s.queued_items(), 0);
}

/// The pending cap is a memory bound: a schedule that cannot enqueue stops
/// the tick with `PendingLimit`, is retried once space frees and is never
/// silently dropped.
#[test]
fn accept_f37_b_pending_limit_retries_until_space_frees() {
    let delayed = || Action::Schedule {
        delay_ticks: 5,
        actions: vec![reward("r-queued")],
    };
    let p = program(vec![objective(
        1,
        vec![delayed(), delayed(), delayed(), reward("r-tail")],
    )]);
    let mut s = state(&p, MAX_WORK_PER_TICK, 2);
    let t1 = s.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    let Some(StopReason::PendingLimit { queued: 2, .. }) = t1.stop else {
        panic!("expected PendingLimit, got {t1:?}");
    };
    // Two stored items plus the exempt remainder item.
    assert_eq!(s.queued_items(), 3);
    // Ticks 2..=5 cannot enqueue: the two due-tick-6 items fill the cap.
    for tick in 2..6 {
        let r = s.step(&p, &MissionFacts::default(), Tick(tick)).unwrap();
        assert!(
            matches!(r.stop, Some(StopReason::PendingLimit { .. })),
            "tick {tick} must still stop on the full queue"
        );
        assert!(reward_events(&r).is_empty());
        assert_eq!(s.queued_items(), 3);
    }
    // Tick 6: all due items leave `pending` for the drain queue, freeing the
    // cap; the deferred remainder retries and runs its whole tail — the two
    // drained rewards plus the never-dropped tail reward.
    let t6 = s.step(&p, &MissionFacts::default(), Tick(6)).unwrap();
    assert!(t6.stop.is_none(), "{:?}", t6.stop);
    assert_eq!(
        reward_events(&t6).len(),
        3,
        "two drained rewards plus the deferred tail"
    );
    // The remainder's own `Schedule` succeeded: one item is due at tick 11.
    assert_eq!(s.queued_items(), 1);
}

/// Two scheduled items from the same source on the same tick get distinct
/// event keys — the per-item ordinal keeps them exactly-once-addressable.
#[test]
fn accept_f37_b_same_source_items_have_distinct_event_keys() {
    let p = program(vec![objective(
        1,
        vec![
            Action::Schedule {
                delay_ticks: 2,
                actions: vec![reward("r-a")],
            },
            Action::Schedule {
                delay_ticks: 2,
                actions: vec![reward("r-b")],
            },
        ],
    )]);
    let mut s = state(&p, MAX_WORK_PER_TICK, MAX_PENDING_ITEMS);
    s.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    let t3 = s.step(&p, &MissionFacts::default(), Tick(3)).unwrap();
    assert!(t3.stop.is_none());
    assert_eq!(
        reward_events(&t3),
        [
            &cid(ContentKind::Blueprint, "r-a"),
            &cid(ContentKind::Blueprint, "r-b")
        ]
    );
    let seqs = event_seqs(&t3);
    assert_eq!(seqs.len(), 2);
    assert_ne!(seqs[0], seqs[1], "per-item ordinals must differ");
    assert!(seqs.iter().all(|s| *s > MAX_ACTIONS_PER_OBJECTIVE as u32));
}

/// The explicit RNG stream is part of state: same session + same program =
/// bit-identical draws; a different session draws from a different stream.
#[test]
fn accept_f37_b_draw_is_seeded_deterministic_per_session() {
    let draws = |session: u32| -> Vec<i32> {
        let p = program(vec![objective(
            1,
            vec![
                Action::Draw {
                    variable: SymbolId(100),
                    min: 0,
                    max: 1_000_000,
                },
                Action::Schedule {
                    delay_ticks: 1,
                    actions: vec![
                        Action::Draw {
                            variable: SymbolId(100),
                            min: 0,
                            max: 1_000_000,
                        },
                        Action::Draw {
                            variable: SymbolId(100),
                            min: 0,
                            max: 1_000_000,
                        },
                    ],
                },
            ],
        )]);
        let mut s = MissionState::new(&p, SessionGeneration(session));
        s.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
        let first = s.variable(SymbolId(100)).cloned();
        s.step(&p, &MissionFacts::default(), Tick(2)).unwrap();
        let second = s.variable(SymbolId(100)).cloned();
        match (first, second) {
            (Some(Value::Int(a)), Some(Value::Int(b))) => vec![a, b],
            other => panic!("expected two int draws, got {other:?}"),
        }
    };
    assert_eq!(draws(7), draws(7), "one session is deterministic");
    assert_ne!(
        draws(7),
        draws(8),
        "a different session seeds a different stream"
    );
    assert!(draws(7).iter().all(|d| (0..=1_000_000).contains(d)));
}

/// Validation reaches inside scheduled lists: unknown instructions and
/// out-of-range draws are launch refusals, not runtime surprises.
#[test]
fn accept_f37_b_validation_reaches_inside_scheduled_actions() {
    let check = |actions: Vec<Action>| {
        MissionProgram {
            version: IR_VERSION,
            mission: cid(ContentKind::Mission, "synthetic-f37b"),
            variables: vec![Variable {
                id: SymbolId(100),
                name: "acc".into(),
                initial: Value::Int(0),
            }],
            objectives: vec![objective(1, actions)],
        }
        .validate()
        .unwrap_err()
    };
    assert!(matches!(
        check(vec![Action::Schedule {
            delay_ticks: 0,
            actions: vec![Action::Unknown {
                instruction: "op_0x55".into()
            }],
        }]),
        ValidationError::UnsupportedInstruction { .. }
    ));
    assert!(matches!(
        check(vec![Action::Draw {
            variable: SymbolId(100),
            min: 5,
            max: 4,
        }]),
        ValidationError::InvalidRange { .. }
    ));
    // Nesting is bounded like the stack limit the contract requires.
    let mut deep = Action::Reschedule { delay_ticks: 0 };
    for _ in 0..MAX_ACTION_NESTING + 2 {
        deep = Action::Schedule {
            delay_ticks: 0,
            actions: vec![deep],
        };
    }
    assert!(matches!(
        check(vec![deep]),
        ValidationError::ActionsTooDeep { .. }
    ));
    // A scheduled item's list obeys the same size bound as an objective's.
    assert!(matches!(
        check(vec![Action::Schedule {
            delay_ticks: 0,
            actions: vec![Action::Finish(Outcome::Succeeded); MAX_ACTIONS_PER_OBJECTIVE + 1],
        }]),
        ValidationError::TooManyActions { .. }
    ));
}
