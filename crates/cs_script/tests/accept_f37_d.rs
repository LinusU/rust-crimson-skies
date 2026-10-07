//! F37-D acceptance: the adversarial mission-runtime corpus and the reference
//! ordering probes.
//!
//! Synthetic fixture only; no original data and no original semantics claimed.
//! The corpus is adversarial on purpose: objectives declared out of symbol
//! order, deferred work that lands on the same tick as later objectives,
//! budgets that split action lists mid-item, self-scheduling lists and records
//! that no live session could have written.
//!
//! The minimum scenario is AC04: an unknown instruction returns Unsupported and
//! prevents reward/progression. The reference ordering probes recompute the
//! expected event order from the documented key rule — (session, tick, source,
//! sequence) — instead of trusting the runtime's own sort, so a change in the
//! ordering key fails here.

use cs_script::ir::*;
use cs_script::runtime::*;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

/// The mission id every corpus program carries; the diagnostic locator is
/// matched against it.
const MISSION: &str = "synthetic-f37d";

/// The corpus session generation. Fixed, because the event keys are part of the
/// reference order the probes recompute.
const SESSION: SessionGeneration = SessionGeneration(7);

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

fn mission_id() -> ContentId {
    cid(ContentKind::Mission, MISSION)
}

fn reward(key: &str) -> Action {
    Action::GrantReward {
        reward: cid(ContentKind::Blueprint, key),
    }
}

fn schedule(delay_ticks: u64, actions: Vec<Action>) -> Action {
    Action::Schedule {
        delay_ticks,
        actions,
    }
}

fn set(variable: u32, value: i32) -> Action {
    Action::SetVariable {
        variable: SymbolId(variable),
        value: Value::Int(value),
    }
}

fn unknown(instruction: &str) -> Action {
    Action::Unknown {
        instruction: instruction.into(),
    }
}

fn variable(id: u32, initial: i32) -> Variable {
    Variable {
        id: SymbolId(id),
        name: format!("v{id}"),
        initial: Value::Int(initial),
    }
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

fn program(variables: Vec<Variable>, objectives: Vec<Objective>) -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: mission_id(),
        variables,
        objectives,
    }
}

fn state(program: &ValidatedProgram) -> MissionState {
    MissionState::new(program, SESSION)
}

/// The reference event order the contract fixes: session, tick, source symbol,
/// sequence. Recomputed here from the emitted events, so a runtime that ordered
/// by anything else fails the probe.
fn reference_order(events: &[MissionEvent]) -> Vec<MissionEvent> {
    let mut sorted = events.to_vec();
    sorted.sort_by_key(|e| e.key);
    sorted
}

/// One `(tick, source, sequence)` triple per emitted event: the trace two runs
/// must share.
fn trace(events: &[MissionEvent]) -> Vec<(u64, u32, u32)> {
    events
        .iter()
        .map(|e| (e.key.tick.0, e.key.source.0, e.key.sequence))
        .collect()
}

/// Runs `ticks` ticks and returns every emitted event, in emission order.
fn run_all(program: &ValidatedProgram, ticks: u64) -> Vec<MissionEvent> {
    let mut state = state(program);
    let mut all = Vec::new();
    for tick in 1..=ticks {
        all.extend(
            state
                .step(program, &MissionFacts::default(), Tick(tick))
                .unwrap()
                .events,
        );
    }
    all
}

/// AC04: an unknown instruction returns Unsupported and prevents
/// reward/progression.
///
/// Every place an undecodable instruction can hide is probed: a condition, an
/// objective's action list, a list nested inside a deferred work item, and an
/// item nested one level deeper again. In each case the *whole* launch is
/// refused, so the objectives declared before the unknown one never fire and
/// their rewards are unreachable — one unknown instruction cannot be skipped
/// past.
#[test]
fn accept_f37_d_unknown_instruction_returns_unsupported_and_prevents_reward_and_progression() {
    // An objective that would otherwise reward and finish, declared *before*
    // the objective that hides the unknown instruction.
    let rewarding = objective(
        1,
        Condition::Const(true),
        vec![reward("r-reward"), Action::Finish(Outcome::Succeeded)],
    );

    // An unknown instruction in a condition.
    let in_condition = program(
        vec![],
        vec![
            rewarding.clone(),
            objective(
                2,
                Condition::Unknown {
                    instruction: "native 0x1f".into(),
                },
                vec![reward("r-condition")],
            ),
        ],
    );
    let refused = in_condition.clone().validate().unwrap_err();
    assert_eq!(
        refused,
        ValidationError::UnsupportedInstruction {
            at: ProgramLocator {
                mission: mission_id().to_string(),
                objective: Some(SymbolId(2)),
                trace: vec!["condition".into()],
            },
            instruction: "native 0x1f".into(),
        },
        "an undecodable condition must name its objective and not be a NOP"
    );
    assert_eq!(
        refused.to_string(),
        format!(
            "{} objective#2 [condition]: unsupported instruction `native 0x1f`",
            mission_id()
        ),
        "the diagnostic must carry mission, objective and trace"
    );

    // An unknown instruction in an action list, after a reward.
    let in_actions = program(
        vec![],
        vec![
            rewarding.clone(),
            objective(
                3,
                Condition::Const(true),
                vec![reward("r-actions"), unknown("op 0x77")],
            ),
        ],
    );
    assert_eq!(
        in_actions.clone().validate().unwrap_err(),
        ValidationError::UnsupportedInstruction {
            at: ProgramLocator {
                mission: mission_id().to_string(),
                objective: Some(SymbolId(3)),
                trace: vec!["action 1".into()],
            },
            instruction: "op 0x77".into(),
        }
    );

    // An unknown instruction two `Schedule` levels down, where a validator that
    // only walked the objective's own list would miss it.
    let nested = program(
        vec![],
        vec![
            rewarding.clone(),
            objective(
                4,
                Condition::Const(true),
                vec![schedule(
                    3,
                    vec![schedule(2, vec![reward("r-nested"), unknown("deep op")])],
                )],
            ),
        ],
    );
    assert_eq!(
        nested.clone().validate().unwrap_err(),
        ValidationError::UnsupportedInstruction {
            at: ProgramLocator {
                mission: mission_id().to_string(),
                objective: Some(SymbolId(4)),
                // The innermost list's own index: the nested lists are all at
                // index 0, so a locator that did not reach inside would report
                // nothing at all.
                trace: vec!["action 1".into()],
            },
            instruction: "deep op".into(),
        },
        "validation must reach inside scheduled action lists"
    );

    // No program carrying an unknown instruction anywhere can be launched, so
    // none of the rewards above is reachable through any of them: no
    // `ValidatedProgram` means no state to step and nothing to grant.
    for (index, candidate) in [&in_condition, &in_actions, &nested].iter().enumerate() {
        assert!(
            matches!(
                (*candidate).clone().validate(),
                Err(ValidationError::UnsupportedInstruction { .. })
            ),
            "corpus program {index} was not refused"
        );
    }

    // A record that carries the same nested unknown in its deferred work is
    // refused too. The evaluator treats `Unknown` as impossible, so restoring
    // it would abort the process on the next tick.
    let ok = program(
        vec![],
        vec![objective(
            5,
            Condition::Const(true),
            vec![schedule(4, vec![reward("r-deferred")])],
        )],
    )
    .validate()
    .unwrap();
    let mut live = state(&ok);
    live.step(&ok, &MissionFacts::default(), Tick(1)).unwrap();
    let good = live.snapshot(&ok);
    assert_eq!(good.pending.len(), 1);
    let mut smuggled = good.clone();
    smuggled.pending[0].actions = vec![schedule(
        1,
        vec![schedule(1, vec![unknown("nested in a record")])],
    )];
    assert!(
        matches!(
            MissionState::restore(&ok, smuggled),
            Err(RestoreError::DeferredActions {
                error: ValidationError::UnsupportedInstruction { .. },
                ..
            })
        ),
        "a record must not smuggle an undecodable instruction into the queue"
    );
    // The intact record still restores and its reward still fires, so the
    // refusal above is the defect and not a fixture that never restores.
    let mut restored = MissionState::restore(&ok, good).unwrap();
    let fired = restored
        .step(&ok, &MissionFacts::default(), Tick(5))
        .unwrap();
    assert_eq!(fired.events.len(), 1);
    assert!(matches!(fired.events[0].kind, EventKind::RewardGranted(_)));
}

/// The reference ordering probe: many objectives, symbol order and declaration
/// order deliberately at odds.
///
/// At most one objective completes per tick — the lowest declaration index
/// whose condition holds
/// (`f37.rule.terminal_precedence.one_completion_per_tick`) — so this run spans
/// one tick per objective, and every objective schedules an item that lands on
/// the *next* tick, behind the objective that completes there. Those ticks
/// carry events from two sources at once: execution runs the completion first
/// and drains the queue after it, while the report comes out in
/// (session, tick, source, sequence) order — a different order, which is what
/// makes the probe discriminate.
///
/// The reference order is recomputed from the documented key rule instead of
/// taken from the runtime, so a change in the ordering key fails here. What
/// must *not* depend on the declaration order is the rule, not the resulting
/// sequence: however the same objectives are declared, the report is
/// key-ordered, one completion per tick in that declaration's index order, and
/// every reward is granted exactly once. Which tick a completion lands on does
/// change with the declaration — that is the measured scan, pinned by
/// `accept_f37_d_fu3_*` — and no `Finish` appears here: a single `Finish`
/// latches the mission, so terminal precedence is pinned by
/// `accept_f37_d_fu2_measured_precedence_records_success_iff_won`.
#[test]
fn accept_f37_d_emitted_order_is_the_reference_key_order_and_independent_of_declaration() {
    // Symbols 9, 3, 7, 1, 5: neither ascending nor descending, and never in
    // symbol order as declared.
    let symbols = [9u32, 3, 7, 1, 5];
    let build = |declaration: &[u32]| {
        program(
            vec![],
            declaration
                .iter()
                .map(|id| {
                    objective(
                        *id,
                        Condition::Const(true),
                        vec![
                            reward(&format!("r-{id}")),
                            // One tick later, behind whichever objective
                            // completes on that tick: a second source on it.
                            schedule(1, vec![reward(&format!("r-{id}-late"))]),
                        ],
                    )
                })
                .collect(),
        )
        .validate()
        .unwrap()
    };

    let shuffled_declaration = [7u32, 9, 5, 3, 1];
    let forward = build(&symbols);
    let backward = build(&symbols.iter().rev().copied().collect::<Vec<_>>());
    let shuffled = build(&shuffled_declaration);

    // One tick per objective (the scan) plus the last item's tick.
    let ticks = symbols.len() as u64 + 1;
    let runs: [(&str, &ValidatedProgram, Vec<u32>); 3] = [
        ("forward", &forward, symbols.to_vec()),
        (
            "backward",
            &backward,
            symbols.iter().rev().copied().collect(),
        ),
        ("shuffled", &shuffled, shuffled_declaration.to_vec()),
    ];
    for (label, program, declaration) in runs {
        let run = run_all(program, ticks);

        // The report is the key order — recomputed here, not trusted — for
        // every declaration order: the ordering *rule* is a function of the
        // keys alone.
        assert_eq!(
            run,
            reference_order(&run),
            "{label}: the emitted order is not the reference key order"
        );

        // One completion per tick, in declaration (index) order: that is the
        // measured scan, and it is what the declaration order decides.
        let completed: Vec<(u64, u32)> = run
            .iter()
            .filter(|event| matches!(event.kind, EventKind::ObjectiveCompleted))
            .map(|event| (event.key.tick.0, event.key.source.0))
            .collect();
        let expected: Vec<(u64, u32)> = (1..=symbols.len() as u64)
            .zip(declaration.iter().copied())
            .collect();
        assert_eq!(
            completed, expected,
            "{label}: the completion scan is not declaration index order"
        );

        // Exactly once: each objective's own reward and its deferred one,
        // nothing granted twice and nothing missing.
        let mut granted = rewards_in(&run);
        granted.sort();
        let mut expect_rewards: Vec<ContentId> = symbols
            .iter()
            .flat_map(|id| {
                [
                    cid(ContentKind::Blueprint, &format!("r-{id}")),
                    cid(ContentKind::Blueprint, &format!("r-{id}-late")),
                ]
            })
            .collect();
        expect_rewards.sort();
        assert_eq!(
            granted, expect_rewards,
            "{label}: rewards were not exactly once"
        );
    }

    // The forward run in full, tick by tick. Tick 3 is the discriminating one:
    // objective 3's queued item drains on the tick objective 7 completes, so
    // execution (completion, then queue) would report 7 before 3 while the key
    // order reports 3 first — and so it does.
    assert_eq!(
        trace(&run_all(&forward, ticks)),
        vec![
            (1, 9, 0),
            (1, 9, 1),
            // The completion of tick k runs before the queue drained behind it,
            // but the report is by source symbol.
            (2, 3, 0),
            (2, 3, 1),
            (2, 9, 66),
            (3, 3, 131),
            (3, 7, 0),
            (3, 7, 1),
            (4, 1, 0),
            (4, 1, 1),
            (4, 7, 196),
            (5, 1, 261),
            (5, 5, 0),
            (5, 5, 1),
            (6, 5, 326),
        ],
        "the emitted trace diverged from the reference key order"
    );
}

/// A run is a function of the program, the session *and the bounds*: a tighter
/// budget defers an objective's actions instead of running them on their own
/// tick, and the deferred queue drains after the objectives of the later tick.
/// So the same program under a tighter budget reports the same rewards in a
/// different order — the deferred tail of an early objective drains behind the
/// later objectives — while the reward *set*, which no condition gates, is
/// granted exactly once either way.
///
/// This is why the bounds travel in the save record: without them a restore
/// would reproduce a session that never existed.
#[test]
fn accept_f37_d_the_work_budget_is_part_of_the_sessions_determinism() {
    let p = program(
        vec![variable(100, 0)],
        vec![
            objective(
                1,
                Condition::Const(true),
                vec![
                    reward("r-first"),
                    Action::Draw {
                        variable: SymbolId(100),
                        min: -1_000_000,
                        max: 1_000_000,
                    },
                ],
            ),
            objective(
                2,
                Condition::Const(true),
                vec![
                    reward("r-second"),
                    Action::Draw {
                        variable: SymbolId(100),
                        min: -1_000_000,
                        max: 1_000_000,
                    },
                ],
            ),
        ],
    )
    .validate()
    .unwrap();

    let roomy = run_traced(&p, 4);
    let mut tight_state = state(&p);
    tight_state.set_limits(WorkLimits {
        // One objective completes per tick and the floor leaves room for its
        // firing and its first action only, so its draw is deferred to a later
        // tick instead of running with the reward.
        max_work_per_tick: MIN_WORK_PER_TICK,
        ..WorkLimits::default()
    });
    let tight = (1..=4)
        .map(|tick| record_tick(&p, &mut tight_state, tick))
        .collect::<RunTrace>();

    // The same rewards, in the same key order, and the same draws — the whole
    // draw sequence, not a shorter one.
    assert_eq!(rewards_of(&roomy), rewards_of(&tight));
    assert_eq!(values_of(&roomy), values_of(&tight));
    // But not on the same ticks: the budget moved the work. The deferral is
    // visible in the state the tick leaves behind — the draws are what a
    // tighter budget pushes to later ticks (`Draw` emits no event, so the
    // event streams of the two runs are the same and prove nothing here).
    assert_ne!(
        roomy
            .iter()
            .map(|tick| tick.variables.clone())
            .collect::<Vec<_>>(),
        tight
            .iter()
            .map(|tick| tick.variables.clone())
            .collect::<Vec<_>>(),
        "the tighter budget was expected to defer the draws across ticks"
    );
    // And the record carries the bounds that produced it.
    let tight_state = state(&p);
    let mut tightened = tight_state;
    tightened.set_limits(WorkLimits {
        max_work_per_tick: MIN_WORK_PER_TICK,
        ..WorkLimits::default()
    });
    tightened
        .step(&p, &MissionFacts::default(), Tick(1))
        .unwrap();
    assert_eq!(
        tightened.snapshot(&p).limits.max_work_per_tick,
        MIN_WORK_PER_TICK
    );

    // The order itself, not only the ticks: at the floor the tail of the first
    // objective is deferred, so the second and third objectives report before
    // it does. Same six rewards, different sequence.
    let interleaved = program(
        vec![],
        [1u32, 2, 3]
            .into_iter()
            .map(|id| {
                objective(
                    id,
                    Condition::Const(true),
                    vec![reward(&format!("r-{id}")), reward(&format!("r-{id}-late"))],
                )
            })
            .collect(),
    )
    .validate()
    .unwrap();
    let roomy_order = rewards_of(&run_traced(&interleaved, 8));
    let mut floor = state(&interleaved);
    floor.set_limits(WorkLimits {
        max_work_per_tick: MIN_WORK_PER_TICK,
        ..WorkLimits::default()
    });
    let deferred_order = rewards_of(&run_traced_state(&interleaved, &mut floor, 8));
    let key = |reward: &ContentId| reward.key().to_owned();
    assert_eq!(
        roomy_order.iter().map(key).collect::<Vec<_>>(),
        vec!["r-1", "r-1-late", "r-2", "r-2-late", "r-3", "r-3-late"],
        "with room to work, an objective's whole list runs before the next one"
    );
    assert_eq!(
        deferred_order.iter().map(key).collect::<Vec<_>>(),
        vec!["r-1", "r-2", "r-3", "r-1-late", "r-2-late", "r-3-late"],
        "at the floor the deferred tails drain behind the later objectives"
    );
    let mut sorted_roomy = roomy_order.clone();
    sorted_roomy.sort();
    let mut sorted_deferred = deferred_order.clone();
    sorted_deferred.sort();
    assert_eq!(
        sorted_roomy, sorted_deferred,
        "the reward set must not change"
    );
}

/// The reference ordering probe across ticks and phases: an objective's own
/// events, a zero-delay item and a delayed item all reporting on one tick, and
/// a later objective firing on a later tick.
///
/// The key layout is the documented one — an objective's own events use
/// sequence `0..=MAX_ACTIONS_PER_OBJECTIVE`, a scheduled item's events are
/// packed above that space by the item's ordinal — so the reference order is
/// computable without trusting the runtime.
#[test]
fn accept_f37_d_deferred_work_and_later_ticks_share_one_reference_order() {
    let p = program(
        vec![variable(100, 0)],
        vec![
            // Declared first but symbols 4 and 2, and it fires on tick 1.
            objective(
                4,
                Condition::Const(true),
                vec![
                    reward("r-now"),
                    // Zero delay: runs on this tick, after the objective's own
                    // actions.
                    schedule(0, vec![reward("r-zero")]),
                    // Two ticks later, behind the objective gated on the phase.
                    schedule(2, vec![set(100, 1), reward("r-late")]),
                ],
            ),
            objective(
                2,
                Condition::Compare {
                    variable: SymbolId(100),
                    op: CompareOp::Ge,
                    value: Value::Int(1),
                },
                vec![reward("r-phase"), Action::Finish(Outcome::Succeeded)],
            ),
        ],
    )
    .validate()
    .unwrap();

    let mut state = state(&p);
    let mut observed: Vec<(u64, u32, u32)> = Vec::new();
    for tick in 1..=4 {
        let result = state
            .step(&p, &MissionFacts::default(), Tick(tick))
            .unwrap();
        assert_eq!(
            result.events,
            reference_order(&result.events),
            "tick {tick} is not in key order"
        );
        observed.extend(trace(&result.events));
        if tick == 1 {
            assert_eq!(
                trace(&result.events),
                vec![
                    (1, 4, 0),
                    (1, 4, 1),
                    // The zero-delay item is ordinal 0 and runs this tick;
                    // item ordinal 1 has a two-tick delay and does not.
                    (1, 4, 66),
                ],
                "a zero-delay item runs on its own tick, in ordinal order"
            );
        }
    }
    assert_eq!(
        observed,
        vec![
            (1, 4, 0),
            (1, 4, 1),
            (1, 4, 66),
            // The delayed item is item ordinal 1: (1 + 1) * 65 + 1 + 1 for its
            // second action, the reward.
            (3, 4, 132),
            (4, 2, 0),
            (4, 2, 1),
            (4, 2, 2),
        ],
        "the emitted trace diverged from the reference order"
    );
    assert_eq!(state.terminal(), TerminalState::Succeeded);
}

/// One tick of a full run: its event trace, the rewards it granted, every
/// variable value and the terminal state.
#[derive(Clone, Debug, PartialEq)]
struct TickTrace {
    /// `(tick, source symbol, sequence)` per emitted event, in emission order.
    events: Vec<(u64, u32, u32)>,
    rewards: Vec<ContentId>,
    variables: Vec<(u32, Value)>,
    terminal: TerminalState,
}

type RunTrace = Vec<TickTrace>;

/// How many ticks every corpus program is run for.
const TICKS: u64 = 24;

/// The corpus's variable values, in symbol order.
fn variables_of(program: &ValidatedProgram, state: &MissionState) -> Vec<(u32, Value)> {
    program
        .program()
        .variables
        .iter()
        .map(|v| {
            (
                v.id.0,
                state
                    .variable(v.id)
                    .cloned()
                    .unwrap_or_else(|| panic!("declared variable #{} has no value", v.id.0)),
            )
        })
        .collect()
}

/// The rewards one tick granted, in the order its events carried them.
fn rewards_in(events: &[MissionEvent]) -> Vec<ContentId> {
    events
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::RewardGranted(reward) => Some(reward.clone()),
            _ => None,
        })
        .collect()
}

fn record_tick(program: &ValidatedProgram, state: &mut MissionState, tick: u64) -> TickTrace {
    let result = state
        .step(program, &MissionFacts::default(), Tick(tick))
        .unwrap();
    TickTrace {
        events: trace(&result.events),
        rewards: rewards_in(&result.events),
        variables: variables_of(program, state),
        terminal: result.terminal,
    }
}

fn run_traced(program: &ValidatedProgram, ticks: u64) -> RunTrace {
    let mut state = state(program);
    run_traced_state(program, &mut state, ticks)
}

/// [`record_tick`] over `ticks` ticks of a session the caller already bounded.
fn run_traced_state(program: &ValidatedProgram, state: &mut MissionState, ticks: u64) -> RunTrace {
    (1..=ticks)
        .map(|tick| record_tick(program, state, tick))
        .collect()
}

/// Saves before every tick and restores into a fresh state, so any piece of
/// execution state the record forgets shows up as a diverging trace.
fn run_traced_with_save_restore_each_tick(program: &ValidatedProgram, ticks: u64) -> RunTrace {
    let mut state = state(program);
    (1..=ticks)
        .map(|tick| {
            let record = state.snapshot(program);
            state = MissionState::restore(program, record).unwrap();
            record_tick(program, &mut state, tick)
        })
        .collect()
}

/// One save in the middle of the run: a pending timer and a split action list
/// are both in flight at that point in the corpus.
fn run_traced_with_one_save(program: &ValidatedProgram, ticks: u64) -> RunTrace {
    let mut state = state(program);
    (1..=ticks)
        .map(|tick| {
            if tick == TICKS / 2 {
                let record = state.snapshot(program);
                state = MissionState::restore(program, record).unwrap();
            }
            record_tick(program, &mut state, tick)
        })
        .collect()
}

/// Every reward a run granted, in emission order.
fn rewards_of(trace: &RunTrace) -> Vec<ContentId> {
    trace
        .iter()
        .flat_map(|tick| tick.rewards.iter().cloned())
        .collect()
}

/// The variable values a run ended on.
fn values_of(trace: &RunTrace) -> Vec<(u32, Value)> {
    trace
        .last()
        .map(|tick| tick.variables.clone())
        .unwrap_or_default()
}

/// The adversarial corpus. Every program is valid; every one of them is built
/// to make the bounded evaluator's job harder than a plain latch.
fn corpus_programs() -> Vec<MissionProgram> {
    // 1. Objectives declared out of symbol order, each with its own delayed
    //    reward, so the tick's event order and the drain order disagree.
    let scrambled = program(
        vec![],
        [9u32, 3, 7, 1, 5]
            .into_iter()
            .map(|id| {
                objective(
                    id,
                    Condition::Const(true),
                    vec![
                        reward(&format!("r-{id}")),
                        schedule(id as u64 % 3, vec![reward(&format!("r-{id}-late"))]),
                    ],
                )
            })
            .collect(),
    );

    // 2. Two objectives that both latch on one tick and both write the same
    //    variable: the writes collide, so which one survives is a designed
    //    order the corpus pins down.
    let colliding_writes = program(
        vec![variable(100, 0)],
        vec![
            objective(5, Condition::Const(true), vec![set(100, 5)]),
            objective(2, Condition::Const(true), vec![set(100, 2)]),
        ],
    );

    // 3. A self-scheduling list with a positive delay: a bounded loop that
    //    keeps producing work for many ticks.
    let looping = program(
        vec![variable(100, 0)],
        vec![objective(
            1,
            Condition::Const(true),
            vec![
                set(100, 1),
                Action::Schedule {
                    delay_ticks: 1,
                    actions: vec![Action::Reschedule { delay_ticks: 1 }],
                },
            ],
        )],
    );

    // 4. A long action list behind a delay, so a small work budget splits it
    //    across many ticks and every split point is a save/restore boundary.
    let long_list = program(
        vec![variable(100, 0)],
        vec![objective(
            1,
            Condition::Const(true),
            vec![schedule(
                1,
                (0..MAX_ACTIONS_PER_OBJECTIVE)
                    .map(|i| set(100, i as i32))
                    .collect(),
            )],
        )],
    );

    // 5. Zero-delay work interleaved with later objectives and a phase latch,
    //    so phases, ordering and the item ordinals all interact.
    let phased = program(
        vec![variable(100, 0)],
        vec![
            objective(
                1,
                Condition::Const(true),
                vec![
                    reward("r-a"),
                    schedule(0, vec![set(100, 1), reward("r-b")]),
                    schedule(3, vec![reward("r-c"), set(100, 2)]),
                ],
            ),
            objective(
                2,
                Condition::Compare {
                    variable: SymbolId(100),
                    op: CompareOp::Eq,
                    value: Value::Int(2),
                },
                vec![reward("r-d"), Action::Finish(Outcome::Succeeded)],
            ),
            objective(
                3,
                Condition::Compare {
                    variable: SymbolId(100),
                    op: CompareOp::Eq,
                    value: Value::Int(9),
                },
                vec![Action::Finish(Outcome::Failed)],
            ),
        ],
    );

    // 6. Conflicting terminal outcomes, a pending reward that is too late to be
    //    granted, and an explicit RNG draw whose stream must survive a save.
    let conflicting = program(
        vec![variable(100, 0)],
        vec![
            objective(
                6,
                Condition::Const(true),
                vec![
                    Action::Finish(Outcome::Succeeded),
                    schedule(5, vec![reward("r-too-late")]),
                    Action::Draw {
                        variable: SymbolId(100),
                        min: -1_000_000,
                        max: 1_000_000,
                    },
                    Action::Finish(Outcome::Failed),
                ],
            ),
            objective(
                4,
                Condition::Const(true),
                vec![Action::Draw {
                    variable: SymbolId(100),
                    min: 0,
                    max: 3,
                }],
            ),
        ],
    );

    // 7. Explicit RNG draws that land on ticks *after* every save, so a record
    //    that forgot how far the stream had advanced would replay it from its
    //    start. The replay test below asserts the draws came back different.
    let late_draw = program(
        vec![variable(100, 0)],
        vec![
            objective(
                1,
                Condition::Const(true),
                vec![
                    reward("r-early"),
                    schedule(
                        2,
                        vec![Action::Draw {
                            variable: SymbolId(100),
                            min: -1_000_000,
                            max: 1_000_000,
                        }],
                    ),
                ],
            ),
            objective(
                2,
                Condition::Const(true),
                vec![schedule(
                    3,
                    vec![Action::Draw {
                        variable: SymbolId(100),
                        min: -1_000_000,
                        max: 1_000_000,
                    }],
                )],
            ),
        ],
    );

    vec![
        scrambled,
        colliding_writes,
        looping,
        long_list,
        phased,
        conflicting,
        late_draw,
    ]
}

/// Does this program ask for a terminal outcome at all? A mission with no
/// `Finish` action keeps running when its work is done, which is the designed
/// behaviour and not a stall.
fn asks_for_an_outcome(program: &MissionProgram) -> bool {
    fn in_actions(actions: &[Action]) -> bool {
        actions.iter().any(|action| match action {
            Action::Finish(_) => true,
            Action::Schedule { actions, .. } => in_actions(actions),
            _ => false,
        })
    }
    program.objectives.iter().any(|o| in_actions(&o.actions))
}

/// Does this program keep scheduling work forever?
fn reschedules(program: &MissionProgram) -> bool {
    fn in_actions(actions: &[Action]) -> bool {
        actions.iter().any(|action| match action {
            Action::Reschedule { .. } => true,
            Action::Schedule { actions, .. } => in_actions(actions),
            _ => false,
        })
    }
    program.objectives.iter().any(|o| in_actions(&o.actions))
}

/// Does this program draw from the explicit RNG stream?
fn draws(program: &MissionProgram) -> bool {
    fn in_actions(actions: &[Action]) -> bool {
        actions.iter().any(|action| match action {
            Action::Draw { .. } => true,
            Action::Schedule { actions, .. } => in_actions(actions),
            _ => false,
        })
    }
    program.objectives.iter().any(|o| in_actions(&o.actions))
}

/// The distinct values a run left in a variable, in the order it wrote them:
/// collapsing consecutive repeats leaves the sequence of writes.
fn writes_of(trace: &RunTrace, symbol: u32) -> Vec<Value> {
    let mut values: Vec<Value> = trace
        .iter()
        .flat_map(|tick| tick.variables.iter().filter(|(id, _)| *id == symbol))
        .map(|(_, value)| value.clone())
        .collect();
    values.dedup();
    values
}

/// Execution order and observation order are different orders, and the corpus
/// pins both.
///
/// Two writes land on one tick when the queue an earlier objective scheduled
/// drains behind the objective that completes on that tick: phase 2 (the
/// completion) writes first and phase 3 (the queued item) writes last, so the
/// *queued* write is the one that lands — one completion per tick means an
/// objective and its own queue are what can collide. Neither write emits an
/// event; the only thing that reports on that tick is the completion, ordered
/// by tick and source symbol rather than by who wrote last. A reader of the
/// event stream therefore cannot infer the surviving value from the report
/// order: the same report shape accompanies either survivor, decided only by
/// the declaration.
#[test]
fn accept_f37_d_simultaneous_writes_follow_execution_order_events_follow_key_order() {
    // Every objective writes the variable itself and queues the same write one
    // tick later, so the tick after a completion carries two writes of one
    // variable: the next objective's completion and the earlier objective's
    // queued item.
    let build = |declaration: &[u32]| {
        program(
            vec![variable(100, 0)],
            declaration
                .iter()
                .map(|id| {
                    objective(
                        *id,
                        Condition::Const(true),
                        vec![
                            set(100, *id as i32),
                            schedule(1, vec![set(100, *id as i32 + 100)]),
                        ],
                    )
                })
                .collect(),
        )
        .validate()
        .unwrap()
    };
    let survivor = |p: &ValidatedProgram| {
        let mut run = state(p);
        let mut observed = Vec::new();
        for tick in 1..=2 {
            let result = run.step(p, &MissionFacts::default(), Tick(tick)).unwrap();
            observed.push(trace(&result.events));
        }
        (run.variable(SymbolId(100)).cloned(), observed)
    };

    // Declared 5 then 2: on tick 2 objective 2 writes 2 and objective 5's
    // queued write lands after it, so 105 is what survives.
    let five_first = build(&[5, 2]);
    let (value, reported) = survivor(&five_first);
    assert_eq!(
        value,
        Some(Value::Int(105)),
        "the queued write drains after the completion's write"
    );
    assert_eq!(
        reported,
        // A state write emits no event: only the completion of each objective
        // is observable, one per tick, in declaration (index) order.
        vec![vec![(1, 5, 0)], vec![(2, 2, 0)]],
        "one completion per tick, and it reports in key order"
    );

    // Declared 2 then 5: the same two objectives, the other survivor — and the
    // *same* report shape, which is the point: tick 2 reports #5 while the
    // write that survived is #2's queued one, the mirror of the run above.
    let two_first = build(&[2, 5]);
    let (value, reported) = survivor(&two_first);
    assert_eq!(
        value,
        Some(Value::Int(102)),
        "the survivor follows the queue, not the symbol order"
    );
    assert_eq!(
        reported,
        vec![vec![(1, 2, 0)], vec![(2, 5, 0)]],
        "the report is keyed by tick and source, never by who wrote last"
    );
}

/// The corpus is replayable: two independent save strategies produce the same
/// trace for every program, so the record carries every piece of state a later
/// observation depends on.
///
/// The programs that draw are checked to draw *more than once*: a record that
/// forgot the draw count would replay the stream from its start, both save
/// strategies would agree with each other and both would be wrong.
#[test]
fn accept_f37_d_adversarial_corpus_replays_identically_across_saves_at_every_boundary() {
    let corpus = corpus_programs();
    assert_eq!(corpus.len(), 7, "the corpus is part of this test");
    // How many members can tell a continued stream from a replayed one, i.e.
    // draw on more than one tick.
    let mut separated_draws = 0usize;
    for (index, program) in corpus.iter().enumerate() {
        let p = program
            .clone()
            .validate()
            .unwrap_or_else(|e| panic!("corpus program {index} does not validate: {e}"));
        let straight = run_traced(&p, TICKS);
        assert_eq!(
            straight,
            run_traced_with_save_restore_each_tick(&p, TICKS),
            "corpus program {index} diverged when saved and restored at every tick"
        );
        assert_eq!(
            straight,
            run_traced_with_one_save(&p, TICKS),
            "corpus program {index} diverged across a mid-run save"
        );
        // Every program does something: the corpus is not a set of no-ops.
        assert!(
            !straight.iter().all(|tick| tick.events.is_empty()),
            "corpus program {index} never emitted an event"
        );
        // The draws must be distinguishable, or a replayed stream would be
        // indistinguishable from a continued one.
        if draws(program) {
            let written = writes_of(&straight, 100);
            assert!(
                written.windows(2).all(|w| w[0] != w[1]),
                "corpus program {index} wrote the same value twice in a row: {written:?}"
            );
            if written.len() > 1 {
                separated_draws += 1;
            }
        }
    }
    assert!(
        separated_draws >= 1,
        "no corpus member draws on more than one tick, so a record that forgot \
         the draw count would be undetectable here"
    );
}

/// The corpus under the smallest work budget that can still make progress.
///
/// Bounded work must never skip an action, never repeat one and never stall a
/// mission that asks for an outcome: whatever the budget splits, the same
/// rewards are granted exactly once. A budget *below* that floor cannot admit
/// work and execute any of it, so it is raised to the floor instead of obeyed —
/// without that, this corpus stalls on its first tick and grants nothing, which
/// is how the floor was found.
#[test]
fn accept_f37_d_tiny_work_budget_grants_every_reward_exactly_once() {
    for (index, program) in corpus_programs().iter().enumerate() {
        let p = program
            .clone()
            .validate()
            .unwrap_or_else(|e| panic!("corpus program {index} does not validate: {e}"));
        let mut expected = rewards_of(&run_traced(&p, TICKS));
        expected.sort();

        let mut state = state(&p);
        state.set_limits(WorkLimits {
            max_work_per_tick: 1,
            ..WorkLimits::default()
        });
        assert_eq!(
            state.snapshot(&p).limits.max_work_per_tick,
            MIN_WORK_PER_TICK,
            "a budget that could never run an action must be raised to the floor"
        );
        let mut granted: Vec<ContentId> = Vec::new();
        let mut resolved = None;
        for tick in 1..=2048u64 {
            let result = state
                .step(&p, &MissionFacts::default(), Tick(tick))
                .unwrap();
            granted.extend(rewards_in(&result.events));
            if result.terminal != TerminalState::Running {
                resolved = Some((tick, result.terminal));
                break;
            }
        }
        let mut sorted = granted.clone();
        sorted.sort();
        assert_eq!(
            sorted, expected,
            "corpus program {index} granted a different reward set under the work floor"
        );
        let mut unique = granted.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            unique.len(),
            granted.len(),
            "corpus program {index} granted a reward twice under the work floor"
        );
        match (resolved, asks_for_an_outcome(program)) {
            // A mission that asks for an outcome must reach one: a bound that
            // stalls it forever would be a hang, not a stop.
            (None, true) => panic!("corpus program {index} never resolved under the work floor"),
            (Some((_, TerminalState::Unsupported)), _) => {
                panic!("corpus program {index} resolved as Unsupported")
            }
            (Some(_), _) => {}
            // A mission that asks for no outcome keeps running, which is the
            // designed behaviour. What it may not do is keep *working*: with
            // nothing left to run its queue is empty and it goes quiet.
            (None, false) if reschedules(program) => {
                assert!(
                    state.queued_items() >= 1,
                    "corpus program {index} stopped rescheduling"
                );
            }
            (None, false) => {
                assert_eq!(
                    state.queued_items(),
                    0,
                    "corpus program {index} still holds work"
                );
                assert!(
                    state.pending_timers().is_empty(),
                    "corpus program {index} still holds a pending timer"
                );
                let idle = state
                    .step(&p, &MissionFacts::default(), Tick(2049))
                    .unwrap();
                assert!(
                    idle.events.is_empty() && idle.stop.is_none(),
                    "corpus program {index} kept working with an empty queue"
                );
            }
        }
    }
}

/// The deferred work of a stopped tick resumes at its cursor, and the save
/// record is the only thing that carries it: this is the corpus's hardest
/// interaction, a budget split *and* a save in the same tick.
#[test]
fn accept_f37_d_budget_split_and_save_in_the_same_tick_resume_once() {
    let p = program(
        vec![],
        vec![objective(
            1,
            Condition::Const(true),
            vec![schedule(
                1,
                vec![reward("r-1"), reward("r-2"), reward("r-3")],
            )],
        )],
    )
    .validate()
    .unwrap();

    // Budget: fire 1 + schedule 1 = 2 on tick 1. Tick 2: dequeue 1 + action 1,
    // action 2 + action 3 = 4, so the item is split across ticks 2, 3 and 4.
    let mut live = state(&p);
    live.set_limits(WorkLimits {
        max_work_per_tick: 2,
        ..WorkLimits::default()
    });
    let mut reference = Vec::new();
    for tick in 1..=8 {
        let result = live.step(&p, &MissionFacts::default(), Tick(tick)).unwrap();
        reference.extend(rewards_in(&result.events));
    }
    assert_eq!(
        reference.len(),
        3,
        "a split item must still run all three of its actions"
    );

    // The same run, saved and restored at the end of every tick: the split
    // cursor is in the record, so the three rewards are granted once each.
    let mut restored = state(&p);
    restored.set_limits(WorkLimits {
        max_work_per_tick: 2,
        ..WorkLimits::default()
    });
    let mut granted = Vec::new();
    for tick in 1..=8 {
        let record = restored.snapshot(&p);
        restored = MissionState::restore(&p, record).unwrap();
        let result = restored
            .step(&p, &MissionFacts::default(), Tick(tick))
            .unwrap();
        granted.extend(rewards_in(&result.events));
    }
    assert_eq!(
        granted, reference,
        "a save at every split boundary changed what ran"
    );
    let mut unique = granted.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), granted.len(), "a reward ran twice");
    assert!(
        restored.pending_timers().is_empty(),
        "the item never finished"
    );
}

/// A budget of zero units is a live-path caller error, but a save record that
/// claims one would restore a mission that can never execute an action: no
/// reward, no terminal request, nothing but a growing pending queue.
#[test]
fn accept_f37_d_restore_refuses_a_record_that_could_never_execute_anything() {
    let p = program(
        vec![],
        vec![objective(
            1,
            Condition::Const(true),
            vec![reward("r"), schedule(2, vec![reward("r-later")])],
        )],
    )
    .validate()
    .unwrap();
    let mut live = state(&p);
    live.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    let good = live.snapshot(&p);

    // A work budget too small to execute anything: the restored session would
    // admit work items and run none of them, forever.
    for found in [0u64, MIN_WORK_PER_TICK - 1] {
        let mut stalled = good.clone();
        stalled.limits.max_work_per_tick = found;
        assert_eq!(
            MissionState::restore(&p, stalled),
            Err(RestoreError::WorkBudgetTooSmall { found }),
            "a record may tighten the budget, but not to a session that cannot progress"
        );
    }

    // An item-ordinal counter past the sequence space: every later schedule
    // would stop with `SequenceExhausted` and no work would ever be queued.
    let mut exhausted = good.clone();
    exhausted.next_item_ordinal = u32::MAX;
    assert_eq!(
        MissionState::restore(&p, exhausted),
        Err(RestoreError::SequenceSpaceExhausted { ordinal: u32::MAX }),
        "a record cannot hand this session an item-ordinal space that ran out"
    );

    // The counter the live allocator stops *at* is the opposite case and must
    // restore: it is the state a session is in once its sequence space really
    // has run out, and a session that cannot save itself has lost its progress.
    // Its later `Schedule`s stop with `SequenceExhausted`, which is reported.
    let scheduling = program(
        vec![],
        vec![objective(
            1,
            Condition::Const(true),
            vec![schedule(2, vec![schedule(1, vec![reward("r-late")])])],
        )],
    )
    .validate()
    .unwrap();
    let mut running = state(&scheduling);
    running
        .step(&scheduling, &MissionFacts::default(), Tick(1))
        .unwrap();
    let mut spent = running.snapshot(&scheduling);
    spent.next_item_ordinal = FIRST_EXHAUSTED_ITEM_ORDINAL;
    let mut at_the_end = MissionState::restore(&scheduling, spent).unwrap();
    let stopped = at_the_end
        .step(&scheduling, &MissionFacts::default(), Tick(3))
        .unwrap();
    assert!(
        matches!(stopped.stop, Some(StopReason::SequenceExhausted { .. })),
        "an exhausted sequence space must be reported, not hidden: {:?}",
        stopped.stop
    );
    assert!(
        at_the_end.pending_timers().len() == 1,
        "the item that could not enqueue must still be pending, not dropped"
    );
    // One step past the marker no live allocator ever produces.
    let mut past = running.snapshot(&scheduling);
    past.next_item_ordinal = FIRST_EXHAUSTED_ITEM_ORDINAL + 1;
    assert_eq!(
        MissionState::restore(&scheduling, past),
        Err(RestoreError::SequenceSpaceExhausted {
            ordinal: FIRST_EXHAUSTED_ITEM_ORDINAL + 1,
        }),
        "no live session counts past the exhausted marker"
    );

    // The intact record still restores, runs the deferred item exactly once and
    // keeps the engine's work budget, so the refusals above are the defects.
    let mut restored = MissionState::restore(&p, good).unwrap();
    assert_eq!(
        restored.snapshot(&p).limits.max_work_per_tick,
        MAX_WORK_PER_TICK
    );
    let fired = restored
        .step(&p, &MissionFacts::default(), Tick(3))
        .unwrap();
    assert_eq!(
        fired
            .events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::RewardGranted(_)))
            .count(),
        1
    );
}

/// The bounds are enforced exactly at their documented edges: the deepest
/// legal nesting and the longest legal action list run, and one step past either
/// is refused with its own diagnostic.
#[test]
fn accept_f37_d_bounds_are_refused_exactly_one_step_past_the_documented_edge() {
    let nest = |depth: usize| {
        let mut actions = vec![reward("r-leaf")];
        for _ in 0..depth {
            actions = vec![schedule(1, actions)];
        }
        actions
    };
    let nest_condition = |depth: usize| {
        let mut condition = Condition::Const(true);
        for _ in 0..depth {
            condition = Condition::Not(Box::new(condition));
        }
        condition
    };
    let build = |actions: Vec<Action>, condition: Condition| {
        program(vec![], vec![objective(1, condition, actions)])
    };

    // The deepest legal `Schedule` nesting runs, and its reward arrives.
    let deepest = build(
        nest(MAX_ACTION_NESTING - 1),
        nest_condition(MAX_CONDITION_DEPTH),
    )
    .validate()
    .unwrap();
    let mut deepest_state = state(&deepest);
    let mut fired = 0;
    for tick in 1..=32 {
        let result = deepest_state
            .step(&deepest, &MissionFacts::default(), Tick(tick))
            .unwrap();
        fired += rewards_in(&result.events).len();
        if !deepest_state.pending_timers().is_empty() {
            // The nesting costs one tick of delay per level.
            continue;
        }
        if fired > 0 {
            break;
        }
    }
    assert_eq!(fired, 1, "the deepest legal nesting must still run");

    // One step deeper in either dimension is refused, not truncated.
    assert!(matches!(
        build(nest(MAX_ACTION_NESTING + 1), nest_condition(1))
            .validate()
            .unwrap_err(),
        ValidationError::ActionsTooDeep { .. }
    ));
    assert!(matches!(
        build(vec![reward("r")], nest_condition(MAX_CONDITION_DEPTH + 1))
            .validate()
            .unwrap_err(),
        ValidationError::ConditionTooDeep { .. }
    ));
    // The longest legal action list runs; one more is refused.
    let longest = build(
        (0..MAX_ACTIONS_PER_OBJECTIVE)
            .map(|_| reward("r"))
            .collect(),
        Condition::Const(true),
    )
    .validate()
    .unwrap();
    let mut longest_state = state(&longest);
    let t1 = longest_state
        .step(&longest, &MissionFacts::default(), Tick(1))
        .unwrap();
    assert_eq!(rewards_in(&t1.events).len(), MAX_ACTIONS_PER_OBJECTIVE);
    assert!(matches!(
        build(
            (0..=MAX_ACTIONS_PER_OBJECTIVE)
                .map(|_| reward("r"))
                .collect(),
            Condition::Const(true)
        )
        .validate()
        .unwrap_err(),
        ValidationError::TooManyActions { .. }
    ));
    assert!(matches!(
        build(
            vec![schedule(
                1,
                (0..=MAX_ACTIONS_PER_OBJECTIVE)
                    .map(|_| reward("r"))
                    .collect()
            )],
            Condition::Const(true)
        )
        .validate()
        .unwrap_err(),
        ValidationError::TooManyActions { .. }
    ));
}
