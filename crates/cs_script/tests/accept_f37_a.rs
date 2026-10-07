//! F37-A acceptance: typed mission IR, validation and stable objective order.
//! Synthetic fixture only; no original data and no original semantics claimed.

use cs_script::ir::*;
use cs_script::runtime::*;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

/// Launch helper: validate, then drive the production [`MissionState`].
#[derive(Debug)]
struct MissionSession {
    program: ValidatedProgram,
    state: MissionState,
}

#[derive(Debug)]
struct Refused {
    terminal: TerminalState,
    error: ValidationError,
}

impl MissionSession {
    fn launch(p: MissionProgram, g: SessionGeneration) -> Result<Self, Refused> {
        let program = p.validate().map_err(|error| Refused {
            terminal: TerminalState::Unsupported,
            error,
        })?;
        let state = MissionState::new(&program, g);
        Ok(Self { program, state })
    }
    fn step(&mut self, f: &MissionFacts, t: Tick) -> Result<TickResult, TickError> {
        self.state.step(&self.program, f, t)
    }
    fn state(&self) -> &MissionState {
        &self.state
    }
}

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

fn objective(id: u32, actor: u32, actions: Vec<Action>) -> Objective {
    Objective {
        id: SymbolId(id),
        content: cid(ContentKind::Objective, &format!("synthetic-obj-{id}")),
        condition: Condition::ActorIs {
            actor: ActorId(actor),
            state: ActorState::Dead,
        },
        actions,
        span: Some(SourceSpan {
            start: id,
            end: id + 1,
        }),
    }
}

fn program(objectives: Vec<Objective>) -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-f37"),
        variables: vec![Variable {
            id: SymbolId(100),
            name: "kills".into(),
            initial: Value::Int(0),
        }],
        objectives,
    }
}

fn dead(actors: &[u32]) -> MissionFacts {
    MissionFacts {
        actors: actors
            .iter()
            .map(|a| (ActorId(*a), ActorState::Dead))
            .collect(),
        ..MissionFacts::default()
    }
}

fn two_objectives() -> MissionProgram {
    program(vec![
        objective(
            2,
            20,
            vec![Action::GrantReward {
                reward: cid(ContentKind::Blueprint, "synthetic-reward-b"),
            }],
        ),
        objective(
            1,
            10,
            vec![Action::GrantReward {
                reward: cid(ContentKind::Blueprint, "synthetic-reward-a"),
            }],
        ),
    ])
}

#[test]
fn accept_f37_a_two_objectives_one_tick_have_stable_order() {
    // Both conditions become true on the same tick. Facts inserted in
    // opposite orders must give the identical result, and only the lowest
    // declaration index completes on that tick: objective 2 is declared
    // first, so it fires on tick 5 while objective 1 — satisfied all along —
    // waits for tick 6, its condition re-evaluated rather than latched
    // (`f37.rule.terminal_precedence.one_completion_per_tick`).
    let run = |actors: &[u32]| {
        let mut s = MissionSession::launch(two_objectives(), SessionGeneration(1)).unwrap();
        let first = s.step(&dead(actors), Tick(5)).unwrap();
        let second = s.step(&dead(actors), Tick(6)).unwrap();
        // Exactly one objective completes per tick, and it is the one the
        // index-order scan reaches first.
        let sources = |events: &[MissionEvent]| -> Vec<_> {
            events
                .iter()
                .filter(|e| matches!(e.kind, EventKind::ObjectiveCompleted))
                .map(|e| e.key.source)
                .collect()
        };
        assert_eq!(sources(&first.events), [SymbolId(2)], "tick 5");
        assert_eq!(sources(&second.events), [SymbolId(1)], "tick 6");
        let mut all = first.events;
        all.extend(second.events);
        all
    };
    let a = run(&[10, 20]);
    let b = run(&[20, 10]);
    assert_eq!(a, b);
    let order: Vec<_> = a.iter().map(|e| (e.key.source.0, e.key.sequence)).collect();
    // Execution follows declaration order (objective 2 is declared first),
    // completion before its own actions, never source-symbol order — and the
    // whole two-tick sequence is key-ordered, tick by tick.
    assert_eq!(order, vec![(2, 0), (2, 1), (1, 0), (1, 1)]);
    assert!(a.windows(2).all(|w| w[0].key < w[1].key));
}

#[test]
fn accept_f37_a_objective_fires_once_and_replay_is_refused() {
    let mut s = MissionSession::launch(two_objectives(), SessionGeneration(1)).unwrap();
    let first = s.step(&dead(&[10]), Tick(1)).unwrap();
    assert_eq!(first.events.len(), 2);
    let again = s.step(&dead(&[10]), Tick(2)).unwrap();
    assert!(again.events.is_empty(), "reward must not repeat");
    assert!(matches!(
        s.step(&dead(&[10]), Tick(2)),
        Err(TickError::NotAdvancing { .. })
    ));
}

#[test]
fn accept_f37_a_conditions_see_start_of_tick_state_only() {
    let mut p = program(vec![
        Objective {
            id: SymbolId(1),
            content: cid(ContentKind::Objective, "a"),
            condition: Condition::Const(true),
            actions: vec![Action::SetVariable {
                variable: SymbolId(100),
                value: Value::Int(1),
            }],
            span: None,
        },
        Objective {
            id: SymbolId(2),
            content: cid(ContentKind::Objective, "b"),
            condition: Condition::Compare {
                variable: SymbolId(100),
                op: CompareOp::Ge,
                value: Value::Int(1),
            },
            actions: vec![],
            span: None,
        },
    ]);
    p.objectives[0].actions.push(Action::GrantReward {
        reward: cid(ContentKind::Blueprint, "r"),
    });
    let mut s = MissionSession::launch(p, SessionGeneration(1)).unwrap();
    let t1 = s.step(&MissionFacts::default(), Tick(1)).unwrap();
    assert!(t1.events.iter().all(|e| e.key.source != SymbolId(2)));
    assert!(!s.state().is_completed(SymbolId(2)));
    assert_eq!(s.state().variable(SymbolId(100)), Some(&Value::Int(1)));
    let t2 = s.step(&MissionFacts::default(), Tick(2)).unwrap();
    assert!(t2.events.iter().any(|e| e.key.source == SymbolId(2)));
}

#[test]
fn accept_f37_a_simultaneous_success_and_failure_never_coexist() {
    let p = program(vec![
        objective(1, 10, vec![Action::Finish(Outcome::Succeeded)]),
        objective(2, 20, vec![Action::Finish(Outcome::Failed)]),
    ]);
    let mut s = MissionSession::launch(p, SessionGeneration(1)).unwrap();
    let r = s.step(&dead(&[10, 20]), Tick(1)).unwrap();
    // Exactly one answer, and the measured precedence decides which one: the
    // original records success iff its WON flag is set, so the success stands
    // when both were requested on one tick (`TERMINAL_PRECEDENCE_RULE`,
    // `f37.rule.terminal_precedence.result_iff_won`). The designed
    // `SyntheticConservative` answer is pinned separately by
    // `accept_f37_d_fu2_*`, which selects it explicitly.
    assert_eq!(r.terminal, TerminalState::Succeeded);
    // Latched: later ticks emit nothing and cannot flip it.
    let later = s.step(&dead(&[10, 20]), Tick(2)).unwrap();
    assert!(later.events.is_empty());
    assert_eq!(later.terminal, TerminalState::Succeeded);
}

#[test]
fn accept_f37_a_unknown_instruction_is_unsupported_with_trace() {
    let mut p = two_objectives();
    p.objectives[1].actions.push(Action::Unknown {
        instruction: "op_0x7f".into(),
    });
    let err = MissionSession::launch(p, SessionGeneration(1)).unwrap_err();
    assert_eq!(err.terminal, TerminalState::Unsupported);
    let ValidationError::UnsupportedInstruction { at, instruction } = err.error else {
        panic!("wrong error");
    };
    assert_eq!(instruction, "op_0x7f");
    assert_eq!(at.objective, Some(SymbolId(1)));
    assert_eq!(at.mission, "mission/synthetic-f37");

    let mut p = two_objectives();
    p.objectives[0].condition = Condition::Unknown {
        instruction: "native_x".into(),
    };
    assert!(matches!(
        MissionSession::launch(p, SessionGeneration(1))
            .unwrap_err()
            .error,
        ValidationError::UnsupportedInstruction { .. }
    ));
}

#[test]
fn accept_f37_a_validation_rejects_bad_references_types_and_bounds() {
    let check = |edit: fn(&mut MissionProgram)| {
        let mut p = two_objectives();
        edit(&mut p);
        p.validate().unwrap_err()
    };
    assert!(matches!(
        check(|p| p.version = 9),
        ValidationError::UnsupportedVersion { found: 9 }
    ));
    assert!(matches!(
        check(|p| p.objectives[0].id = SymbolId(1)),
        ValidationError::DuplicateSymbol { .. }
    ));
    assert!(matches!(
        check(|p| p.objectives[0].actions = vec![Action::SetVariable {
            variable: SymbolId(999),
            value: Value::Int(1)
        }]),
        ValidationError::UnknownVariable { .. }
    ));
    assert!(matches!(
        check(|p| p.objectives[0].actions = vec![Action::SetVariable {
            variable: SymbolId(100),
            value: Value::Bool(true)
        }]),
        ValidationError::TypeMismatch { .. }
    ));
    assert!(matches!(
        check(|p| p.objectives[0].actions = vec![Action::SetVariable {
            variable: SymbolId(100),
            value: Value::Float(f64::NAN)
        }]),
        ValidationError::TypeMismatch { .. } | ValidationError::NonFiniteValue { .. }
    ));
    assert!(matches!(
        check(|p| p.objectives[0].actions = vec![Action::Finish(Outcome::Failed); 65]),
        ValidationError::TooManyActions { count: 65, .. }
    ));
    assert!(matches!(
        check(|p| p.mission = cid(ContentKind::Objective, "x")),
        ValidationError::WrongContentKind { .. }
    ));
    assert!(matches!(
        check(|p| {
            let mut c = Condition::Const(true);
            for _ in 0..20 {
                c = Condition::Not(Box::new(c));
            }
            p.objectives[0].condition = c;
        }),
        ValidationError::ConditionTooDeep { .. }
    ));
    assert!(matches!(
        check(|p| p.objectives[0].condition = Condition::Compare {
            variable: SymbolId(100),
            op: CompareOp::Lt,
            value: Value::Bool(true),
        }),
        ValidationError::TypeMismatch { .. }
    ));
}

#[test]
fn accept_f37_a_phase_of_each_action_is_documented_order() {
    assert!(Phase::State < Phase::Terminal && Phase::Terminal < Phase::Host);
    assert_eq!(Action::Finish(Outcome::Succeeded).phase(), Phase::Terminal);
}
