//! `M02-B-FU4` acceptance: the wake/sleep/kill/nap directive family's executor
//! applies the cross-objective address rule (synthetic — runs in CI).
//!
//! Task: Rally #807. Rule: `M02-B-FU3`
//! (`docs/findings/2026-10-09-m02-b-fu3-out-of-range-wake-address.md`). Ordering:
//! `docs/contracts/SCRIPT-MISSION.md` ("Objective event ordering") and the
//! measured completion pipeline `WAKE → NAP → KILL → SLEEP`.

use cs_script::ir::{DirectiveOperation, SymbolId, Value};
use cs_script::runtime::{DirectiveEmission, EventKey, SessionGeneration};
use cs_sim::objectives::address::AddressRefusal;
use cs_sim::objectives::runtime::{
    ObjectiveCompletion, ObjectiveEventKind, ObjectiveRuntime, ObjectiveSpec, ObjectiveTick,
    RevealRule, RuntimeError, RuntimeLimits, TickInput,
};
use cs_sim::objectives::state::ObjectiveState;
use cs_sim::objectives::terminal::TerminalPrecedence;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

const SESSION: SessionGeneration = SessionGeneration(3);
/// The record's block count; addresses `1..=BLOCKS` name `SymbolId(0..BLOCKS)`.
/// The runtime reserves `SymbolId(0)` for actor events, so the first block
/// (address 1) cannot be registered; the tests register blocks `1..BLOCKS`.
const BLOCKS: u32 = 5;
/// The objective whose block spells the directives (address 2).
const SOURCE: SymbolId = SymbolId(1);

fn record(initial: ObjectiveState) -> ObjectiveRuntime {
    let mut runtime = ObjectiveRuntime::new(
        SESSION,
        TerminalPrecedence::SyntheticConservative,
        RuntimeLimits::default(),
    );
    for index in 1..BLOCKS {
        runtime
            .add_objective(ObjectiveSpec {
                id: SymbolId(index),
                content: ContentId::from_source(ContentKind::Objective, &format!("block-{index}"))
                    .unwrap(),
                initial: if index == 1 {
                    ObjectiveState::Active
                } else {
                    initial
                },
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

fn emission(sequence: u32, operation: DirectiveOperation, args: Vec<Value>) -> DirectiveEmission {
    DirectiveEmission {
        key: EventKey {
            session: SESSION,
            tick: Tick(1),
            source: SOURCE,
            sequence,
        },
        operation,
        args,
    }
}

fn run(runtime: &mut ObjectiveRuntime, emissions: &[DirectiveEmission]) -> ObjectiveTick {
    runtime.submit_directives(BLOCKS, emissions).unwrap();
    runtime.step(&TickInput::at(Tick(1))).unwrap()
}

fn states(runtime: &ObjectiveRuntime) -> Vec<ObjectiveState> {
    (1..BLOCKS)
        .map(|i| runtime.objective_state(SymbolId(i)).unwrap())
        .collect()
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

/// An in-range address applies its effect on the record it names (one-based).
#[test]
fn accept_m02_b_fu4_an_in_range_address_applies_its_effect() {
    let mut runtime = record(ObjectiveState::Asleep);
    let tick = run(
        &mut runtime,
        &[emission(
            1,
            DirectiveOperation::WakeObjectives,
            vec![list(&[3, 5])],
        )],
    );
    assert_eq!(
        states(&runtime),
        [
            ObjectiveState::Active,
            ObjectiveState::Active,
            ObjectiveState::Asleep,
            ObjectiveState::Active,
        ],
        "address 3 names symbol 2 and address 5 (the last) symbol 4"
    );
    assert!(refusals(&tick).is_empty());
}

/// An out-of-range address is refused by name and applies nothing — through
/// every operation of the family that has a consumer.
#[test]
fn accept_m02_b_fu4_an_out_of_range_address_refuses_by_name_and_applies_nothing() {
    for (operation, args) in [
        (DirectiveOperation::WakeObjectives, vec![list(&[6])]),
        (DirectiveOperation::SleepObjectives, vec![list(&[6])]),
        (DirectiveOperation::KillObjectives, vec![list(&[6])]),
        (
            DirectiveOperation::NapObjective,
            vec![Value::Int(6), Value::Float(2.0)],
        ),
        // A negative address decodes below the terminator: also outside.
        (DirectiveOperation::WakeObjectives, vec![list(&[-1])]),
    ] {
        let mut runtime = record(ObjectiveState::Asleep);
        let before = states(&runtime);
        let tick = run(&mut runtime, &[emission(1, operation, args.clone())]);
        let address = match &args[..] {
            [Value::List(items)] => match items[0] {
                Value::Int(a) => i64::from(a),
                _ => unreachable!(),
            },
            [Value::Int(a), _] => i64::from(*a),
            _ => unreachable!(),
        };
        assert_eq!(
            refusals(&tick),
            [&RuntimeError::Address(AddressRefusal {
                address,
                objectives: BLOCKS
            })],
            "{operation:?} {args:?}"
        );
        assert_eq!(states(&runtime), before, "{operation:?} applied something");
        for record in 1..BLOCKS {
            assert_eq!(runtime.is_present(SymbolId(record)), Some(true));
        }
    }
}

/// The parse's `-1` terminator: an authored `0` ends the walk, the children
/// after it are never reached, and that is not a refusal.
#[test]
fn accept_m02_b_fu4_an_authored_zero_truncates_the_list_without_a_refusal() {
    let mut runtime = record(ObjectiveState::Asleep);
    let tick = run(
        &mut runtime,
        &[emission(
            1,
            DirectiveOperation::WakeObjectives,
            vec![list(&[3, 0, 4, 99])],
        )],
    );
    assert_eq!(
        states(&runtime),
        [
            ObjectiveState::Active,
            ObjectiveState::Active,
            ObjectiveState::Asleep,
            ObjectiveState::Asleep,
        ]
    );
    assert!(
        refusals(&tick).is_empty(),
        "99 follows the terminator and is never reached"
    );
}

/// One source's emissions apply in the measured pipeline order whatever order
/// they were emitted in, and an emission queues exactly once.
#[test]
fn accept_m02_b_fu4_emissions_apply_in_pipeline_order_exactly_once() {
    let mut runtime = record(ObjectiveState::Asleep);
    let emissions = [
        emission(1, DirectiveOperation::SleepObjectives, vec![list(&[3])]),
        emission(2, DirectiveOperation::KillObjectives, vec![list(&[4])]),
        emission(
            3,
            DirectiveOperation::NapObjective,
            vec![Value::Int(5), Value::Int(2)],
        ),
        emission(4, DirectiveOperation::WakeObjectives, vec![list(&[3])]),
    ];
    assert_eq!(runtime.submit_directives(BLOCKS, &emissions), Ok(4));
    assert_eq!(runtime.submit_directives(BLOCKS, &emissions), Ok(0));
    let tick = runtime.step(&TickInput::at(Tick(1))).unwrap();
    // Wake then sleep on symbol 2 leaves it asleep; the nap and kill landed.
    assert_eq!(
        runtime.objective_state(SymbolId(2)),
        Some(ObjectiveState::Asleep)
    );
    assert_eq!(
        runtime.objective_state(SymbolId(4)),
        Some(ObjectiveState::Napping)
    );
    assert_eq!(runtime.is_present(SymbolId(3)), Some(false));
    let touched: Vec<SymbolId> = tick
        .events
        .iter()
        .filter_map(|event| match event.kind {
            ObjectiveEventKind::ObjectiveChanged { objective, .. }
            | ObjectiveEventKind::ObjectiveRemoved { objective, .. } => Some(objective),
            _ => None,
        })
        .collect();
    assert_eq!(
        touched,
        [SymbolId(2), SymbolId(4), SymbolId(3), SymbolId(2)],
        "WAKE, NAP, KILL, SLEEP"
    );
}

/// Address 1 names `SymbolId(0)`, which the runtime reserves for actor events,
/// so no objective can be registered there: the wake is reported as an unknown
/// objective rather than dropped (follow-up filed for the clash).
#[test]
fn accept_m02_b_fu4_the_first_address_names_the_reserved_symbol_and_is_reported() {
    let mut runtime = record(ObjectiveState::Asleep);
    let tick = run(
        &mut runtime,
        &[emission(
            1,
            DirectiveOperation::WakeObjectives,
            vec![list(&[1])],
        )],
    );
    assert_eq!(
        refusals(&tick),
        [&RuntimeError::UnknownObjective {
            objective: SymbolId(0)
        }]
    );
}

/// The three operations with no lifecycle consumer check their addresses and
/// then say so, instead of dropping the emission; a stale session applies
/// nothing.
#[test]
fn accept_m02_b_fu4_operations_without_a_consumer_are_reported_not_dropped() {
    let mut runtime = record(ObjectiveState::Asleep);
    let tick = run(
        &mut runtime,
        &[
            emission(1, DirectiveOperation::DependencyGate, vec![Value::Int(2)]),
            emission(2, DirectiveOperation::HideObjective, vec![Value::Int(9)]),
        ],
    );
    assert_eq!(
        refusals(&tick),
        [
            &RuntimeError::UnexecutedDirective {
                operation: DirectiveOperation::DependencyGate
            },
            &RuntimeError::Address(AddressRefusal {
                address: 9,
                objectives: BLOCKS
            }),
            &RuntimeError::UnexecutedDirective {
                operation: DirectiveOperation::HideObjective
            },
        ]
    );
    let mut stale = emission(1, DirectiveOperation::WakeObjectives, vec![list(&[3])]);
    stale.key.session = SessionGeneration(4);
    let mut fresh = record(ObjectiveState::Asleep);
    assert_eq!(
        fresh.submit_directives(BLOCKS, &[stale]),
        Err(RuntimeError::StaleDirective {
            expected: SESSION,
            given: SessionGeneration(4)
        })
    );
}
