//! `M02-B-FU5` acceptance: a running mission drives the wake/sleep/kill/nap
//! directive family into [`ObjectiveRuntime`] (synthetic — runs in CI).
//!
//! Task: Rally #1586. The driver is [`MissionSession::submit_directives`]:
//! the session's evaluator logs every `DirectiveEmission` the record's
//! blocks spell, the session hands the log plus the record's block count to
//! the family's executor, and `resolve_objective_address`'s `SymbolId(0)` —
//! the record's first block — registers like any other objective because
//! the counted-actor source lives at `SymbolId(u32::MAX)` (the clash the
//! FU4 suite documented and this task resolved). The measurement is
//! `docs/findings/2026-10-09-m02-b-fu3-out-of-range-wake-address.md` —
//! including the wake walk's 15-entry window at `0x469af0`, which a spelled
//! list can now overflow only through `RuntimeError::DirectiveListTooLong`.

use cs_script::ir::{
    Action, Condition, DirectiveOperation, IR_VERSION, MissionProgram, Objective, SymbolId, Value,
};
use cs_script::runtime::{MissionFacts, ObjectiveLifecycle, SessionGeneration};
use cs_sim::mission::MissionSession;
use cs_sim::objectives::address::resolve_objective_address;
use cs_sim::objectives::runtime::{
    ACTOR_EVENT_SOURCE, ObjectiveCompletion, ObjectiveEventKind, ObjectiveRuntime, ObjectiveSpec,
    ObjectiveTick, RevealRule, RuntimeError, RuntimeLimits, TickInput,
};
use cs_sim::objectives::state::ObjectiveState;
use cs_sim::objectives::terminal::TerminalPrecedence;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

const SESSION: SessionGeneration = SessionGeneration(7);

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

fn block(index: u32, condition: Condition, actions: Vec<Action>) -> Objective {
    Objective {
        id: SymbolId(index),
        content: cid(ContentKind::Objective, &format!("block-{index}")),
        condition,
        actions,
        span: None,
    }
}

/// A control program whose `objectives` are the record's blocks: `id` is the
/// zero-based record index — the same space `resolve_objective_address`
/// mints (`address − 1`).
fn program(objectives: Vec<Objective>) -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "fu5"),
        variables: vec![],
        objectives,
    }
}

/// The declared-objective session matching `program`'s blocks: `SymbolId(i)`
/// is block `i`, every one of them registerable — including `SymbolId(0)`.
fn runtime(blocks: u32, initial: ObjectiveState) -> ObjectiveRuntime {
    let mut runtime = ObjectiveRuntime::new(
        SESSION,
        TerminalPrecedence::SyntheticConservative,
        RuntimeLimits::default(),
    );
    for index in 0..blocks {
        runtime
            .add_objective(ObjectiveSpec {
                id: SymbolId(index),
                content: cid(ContentKind::Objective, &format!("block-{index}")),
                initial,
                reveal: RevealRule::Immediate,
                on_complete: ObjectiveCompletion::Continue,
                completion_effects: Vec::new(),
            })
            .unwrap();
    }
    runtime
}

fn list(addresses: &[i32]) -> Value {
    Value::List(addresses.iter().map(|a| Value::Int(*a)).collect())
}

fn refusals(tick: &ObjectiveTick) -> Vec<&RuntimeError> {
    tick.events
        .iter()
        .filter_map(|event| match &event.kind {
            ObjectiveEventKind::RequestRefused { reason, .. } => Some(reason),
            _ => None,
        })
        .collect()
}

/// A running mission: the evaluator logs a block's `WAKE_OBJECTIVE`
/// emission on completion, `MissionSession::submit_directives` hands the
/// record's block count and the log to the objective runtime, and the
/// next runtime tick applies the wake — address 1 included, because
/// `SymbolId(0)` is the record's first block and not a reserved marker.
#[test]
fn accept_m02_b_fu5_a_running_mission_drives_its_directives_into_the_objective_runtime() {
    let mut session = MissionSession::launch(
        program(vec![
            // Block 0 never fires; block 1 completes on tick 1 and spells
            // `WAKE_OBJECTIVE [1]` — the record's first block.
            block(0, Condition::Const(false), vec![]),
            block(
                1,
                Condition::Const(true),
                vec![Action::Directive {
                    operation: DirectiveOperation::WakeObjectives,
                    args: vec![list(&[1])],
                }],
            ),
        ]),
        SESSION,
        Vec::new(),
    )
    .unwrap();
    let mut objectives = runtime(2, ObjectiveState::Pending);

    session.advance(&MissionFacts::default(), Tick(1)).unwrap();
    assert_eq!(session.submit_directives(&mut objectives), Ok(1));
    let tick = objectives.step(&TickInput::at(Tick(1))).unwrap();
    assert_eq!(
        objectives.objective_state(SymbolId(0)),
        Some(ObjectiveState::Active),
        "address 1 resolved to the record's first block and the wake applied"
    );
    assert!(refusals(&tick).is_empty());

    // The executor consumes each emission's execution key once: the same
    // log re-submitted queues nothing again.
    assert_eq!(session.submit_directives(&mut objectives), Ok(0));
}

/// The same driver across ticks: each new emission is queued exactly once,
/// in the order the mission produced it.
#[test]
fn accept_m02_b_fu5_the_driver_queues_each_new_emission_once_across_ticks() {
    let mut session = MissionSession::launch(
        program(vec![
            block(0, Condition::Const(false), vec![]),
            block(
                1,
                Condition::Const(true),
                vec![Action::Directive {
                    operation: DirectiveOperation::WakeObjectives,
                    args: vec![list(&[1])],
                }],
            ),
            block(
                2,
                Condition::Const(true),
                vec![Action::Directive {
                    operation: DirectiveOperation::SleepObjectives,
                    args: vec![list(&[1])],
                }],
            ),
        ]),
        SESSION,
        Vec::new(),
    )
    .unwrap();
    let mut objectives = runtime(3, ObjectiveState::Pending);

    // One completion per tick, lowest index first: block 1's wake on tick
    // 1, block 2's sleep on tick 2.
    session.advance(&MissionFacts::default(), Tick(1)).unwrap();
    assert_eq!(session.submit_directives(&mut objectives), Ok(1));
    objectives.step(&TickInput::at(Tick(1))).unwrap();
    assert_eq!(
        objectives.objective_state(SymbolId(0)),
        Some(ObjectiveState::Active)
    );

    session.advance(&MissionFacts::default(), Tick(2)).unwrap();
    assert_eq!(session.submit_directives(&mut objectives), Ok(1));
    objectives.step(&TickInput::at(Tick(2))).unwrap();
    assert_eq!(
        objectives.objective_state(SymbolId(0)),
        Some(ObjectiveState::Asleep)
    );
}

/// The reserved source moved off the record's space: `SymbolId(0)` is an
/// ordinary registerable objective, the counted-actor source is
/// `SymbolId(u32::MAX)` — still refused — and no record index or resolved
/// address can reach it.
#[test]
fn accept_m02_b_fu5_the_counted_actor_source_stays_reserved_and_unreachable() {
    let mut objectives = runtime(2, ObjectiveState::Pending);
    assert_eq!(
        objectives.add_objective(ObjectiveSpec {
            id: ACTOR_EVENT_SOURCE,
            content: cid(ContentKind::Objective, "actor-source"),
            initial: ObjectiveState::Pending,
            reveal: RevealRule::Immediate,
            on_complete: ObjectiveCompletion::Continue,
            completion_effects: Vec::new(),
        }),
        Err(RuntimeError::ReservedSymbol {
            symbol: SymbolId(u32::MAX)
        })
    );
    // The largest symbol the address rule can mint is `u32::MAX - 1` —
    // a record's block count is a `u32`, so `u32::MAX` stays the counted
    // actors' alone.
    assert_eq!(
        resolve_objective_address(i64::from(u32::MAX), u32::MAX),
        Ok(SymbolId(u32::MAX - 1))
    );
}

/// The engine's 15-entry list window (`0x469af0`'s `cmpl $0xf`): a spelled
/// list of 14 addresses fills it beside the terminator and applies; 15
/// leaves the terminator nowhere and the whole list is refused by name —
/// never walked truncated.
#[test]
fn accept_m02_b_fu5_a_list_filling_the_engines_window_applies_and_one_past_is_refused() {
    // Fourteen targets fit: `WAKE [2..=15]` wakes blocks 1..14.
    let mut session = MissionSession::launch(
        program({
            let mut objectives: Vec<Objective> = (0..15)
                .map(|index| block(index, Condition::Const(false), vec![]))
                .collect();
            objectives[0].condition = Condition::Const(true);
            objectives[0].actions = vec![Action::Directive {
                operation: DirectiveOperation::WakeObjectives,
                args: vec![list(&(2..=15).collect::<Vec<_>>())],
            }];
            objectives
        }),
        SESSION,
        Vec::new(),
    )
    .unwrap();
    let mut objectives = runtime(15, ObjectiveState::Pending);
    session.advance(&MissionFacts::default(), Tick(1)).unwrap();
    assert_eq!(session.submit_directives(&mut objectives), Ok(1));
    let tick = objectives.step(&TickInput::at(Tick(1))).unwrap();
    assert!(refusals(&tick).is_empty());
    for symbol in 1..15 {
        assert_eq!(
            objectives.objective_state(SymbolId(symbol)),
            Some(ObjectiveState::Active),
            "address {} (symbol {}) should be woken",
            symbol + 1,
            symbol
        );
    }

    // Fifteen targets overrun the window: nothing applies and the refusal
    // is named.
    let mut session = MissionSession::launch(
        program({
            let mut objectives: Vec<Objective> = (0..16)
                .map(|index| block(index, Condition::Const(false), vec![]))
                .collect();
            objectives[0].condition = Condition::Const(true);
            objectives[0].actions = vec![Action::Directive {
                operation: DirectiveOperation::WakeObjectives,
                args: vec![list(&(1..=15).collect::<Vec<_>>())],
            }];
            objectives
        }),
        SESSION,
        Vec::new(),
    )
    .unwrap();
    let mut objectives = runtime(16, ObjectiveState::Pending);
    session.advance(&MissionFacts::default(), Tick(1)).unwrap();
    assert_eq!(session.submit_directives(&mut objectives), Ok(1));
    let tick = objectives.step(&TickInput::at(Tick(1))).unwrap();
    assert_eq!(
        refusals(&tick),
        [&RuntimeError::DirectiveListTooLong {
            operation: DirectiveOperation::WakeObjectives,
            len: 15
        }]
    );
    for symbol in 0..16 {
        assert_eq!(
            objectives.objective_state(SymbolId(symbol)),
            Some(ObjectiveState::Pending),
            "symbol {symbol} moved though the whole list was refused"
        );
    }
}

/// `WakeObjectivesOnTransition`, `DependencyGate` and `HideObjective` keep
/// their named refusal through the running-mission path — the first arrives
/// as a completion directive, the second likewise, the third as the
/// wake-role emission a block's fresh `Awake` row produces — and the
/// addresses they spell are still checked first.
#[test]
fn accept_m02_b_fu5_the_operations_without_a_consumer_stay_named_refusals() {
    let mut session = MissionSession::launch(
        program(vec![
            // Block 0's facts row reads `Awake` on tick 1, so its wake-role
            // `HIDE_OBJ` fires then (the gate is dormant-only in the
            // engine, and dormancy is what this runtime has no model of).
            block(
                0,
                Condition::Const(false),
                vec![Action::Directive {
                    operation: DirectiveOperation::HideObjective,
                    args: vec![Value::Int(2)],
                }],
            ),
            block(
                1,
                Condition::Const(true),
                vec![Action::Directive {
                    operation: DirectiveOperation::DependencyGate,
                    args: vec![Value::Int(2)],
                }],
            ),
            block(
                2,
                Condition::Const(true),
                vec![Action::Directive {
                    operation: DirectiveOperation::WakeObjectivesOnTransition,
                    args: vec![list(&[2])],
                }],
            ),
        ]),
        SESSION,
        Vec::new(),
    )
    .unwrap();
    let mut objectives = runtime(3, ObjectiveState::Pending);
    let mut facts = MissionFacts::default();
    facts.objectives.insert(0, ObjectiveLifecycle::Awake);

    // Tick 1: block 0's wake-role emission and block 1's completion.
    session.advance(&facts, Tick(1)).unwrap();
    assert_eq!(session.submit_directives(&mut objectives), Ok(2));
    let tick = objectives.step(&TickInput::at(Tick(1))).unwrap();
    assert_eq!(
        refusals(&tick),
        [
            &RuntimeError::UnexecutedDirective {
                operation: DirectiveOperation::HideObjective
            },
            &RuntimeError::UnexecutedDirective {
                operation: DirectiveOperation::DependencyGate
            },
        ]
    );
    assert_eq!(
        objectives.objective_state(SymbolId(1)),
        Some(ObjectiveState::Pending),
        "nothing moved — the named refusal applied no effect"
    );

    // Tick 2: block 2's completion, one completion per tick.
    session.advance(&facts, Tick(2)).unwrap();
    assert_eq!(session.submit_directives(&mut objectives), Ok(1));
    let tick = objectives.step(&TickInput::at(Tick(2))).unwrap();
    assert_eq!(
        refusals(&tick),
        [&RuntimeError::UnexecutedDirective {
            operation: DirectiveOperation::WakeObjectivesOnTransition
        }]
    );
}

/// The sessions must serve one generation: a runtime launched for another
/// refuses the whole submission and queues nothing.
#[test]
fn accept_m02_b_fu5_a_foreign_generations_directives_are_refused() {
    let mut session = MissionSession::launch(
        program(vec![
            block(0, Condition::Const(false), vec![]),
            block(
                1,
                Condition::Const(true),
                vec![Action::Directive {
                    operation: DirectiveOperation::WakeObjectives,
                    args: vec![list(&[1])],
                }],
            ),
        ]),
        SESSION,
        Vec::new(),
    )
    .unwrap();
    session.advance(&MissionFacts::default(), Tick(1)).unwrap();

    let mut foreign = ObjectiveRuntime::new(
        SessionGeneration(8),
        TerminalPrecedence::SyntheticConservative,
        RuntimeLimits::default(),
    );
    assert_eq!(
        session.submit_directives(&mut foreign),
        Err(RuntimeError::StaleDirective {
            expected: SessionGeneration(8),
            given: SESSION,
        })
    );
}
