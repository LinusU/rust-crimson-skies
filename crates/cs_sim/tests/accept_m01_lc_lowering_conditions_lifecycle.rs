//! M01-LC-DIRECTIVE-LOWERING.02 acceptance (`cs_sim` half): the writer for
//! `MissionFacts::objectives` — the block-lifecycle table seeded from the
//! record's own `BEGIN_DORMANT` spelling — and the lifecycle gates
//! `MissionState::step` then honours.
//!
//! Task key `M01-LC-DIRECTIVE-LOWERING.02`; test prefix
//! `accept_m01_lc_lowering_conditions_`. Measured source:
//! `docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`
//! (the record defaults, pass 1's timed self-wake and the pass-2 gate).
//!
//! The conditions below are produced by `cs_script::conditions` from the
//! same directive spellings the record writes, so this suite exercises the
//! production lowering and the production evaluator together — no parallel
//! predicate is written here.

use cs_script::conditions::{BlockDirective, lower_block_condition};
use cs_script::ir::{Condition, IR_VERSION, MissionProgram, Objective, SymbolId, Value};
use cs_script::runtime::{
    EventKind, MissionFacts, MissionState, ObjectiveLifecycle, SessionGeneration,
};
use cs_sim::mission::{BlockLifecycleTable, LifecycleDecl, LifecycleError};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

fn lower(block: &str, index: u32, directives: Vec<BlockDirective>) -> Condition {
    lower_block_condition(block, index, &directives)
        .unwrap_or_else(|refusal| panic!("{block} must lower, refused: {refusal}"))
}

/// One declared block, lowered from the record's own spelling and wired into
/// a one-objective program.
fn objective(index: u32, block: &str, directives: Vec<BlockDirective>) -> Objective {
    Objective {
        id: SymbolId(index),
        content: cid(ContentKind::Objective, block),
        condition: lower(block, index, directives),
        actions: vec![],
        span: None,
    }
}

fn dormant(wake_at: f64) -> BlockDirective {
    BlockDirective::new("BEGIN_DORMANT", vec![Value::Float(wake_at)])
}

fn program(objectives: Vec<Objective>) -> cs_script::ir::ValidatedProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "m01"),
        variables: vec![],
        objectives,
    }
    .validate()
    .expect("every lowered condition validates")
}

/// AC3: a block that starts dormant does not latch at tick 0, an awake block
/// completes on the first tick it is awake, and `TICK_DEPENDS_ON_OBJ`'s gate
/// holds a dependent until its dependency is awake — through
/// `MissionState::step`, on facts `cs_sim`'s block-lifecycle table produces.
#[test]
fn accept_m01_lc_lowering_conditions_lifecycle_gates_are_represented() {
    // Three blocks: 0 starts dormant with a timed self-wake at 2 s, 1 is
    // awake from parse, 2 starts dormant and depends on block 0.
    let program = program(vec![
        objective(0, "OBJECTIVE1", vec![dormant(2.0)]),
        objective(1, "OBJECTIVE2", vec![]),
        objective(
            2,
            "OBJECTIVE3",
            vec![
                dormant(-1.0),
                BlockDirective::new("TICK_DEPENDS_ON_OBJ", vec![Value::Int(1)]),
            ],
        ),
    ]);
    assert!(
        matches!(
            &program.program().objectives[2].condition,
            Condition::All(items)
                if items.len() == 2
                    && matches!(items[0], Condition::ObjectiveAwake { index: 2 })
                    && matches!(items[1], Condition::ObjectiveAwake { index: 0 })
        ),
        "the dependent gates on itself and on the block the record names"
    );

    let mut table = BlockLifecycleTable::new();
    assert_eq!(
        table
            .declare(0, LifecycleDecl::dormant(Some(2.0)))
            .expect("block 0 is declared once"),
        ObjectiveLifecycle::Dormant,
        "a block that starts dormant is recorded dormant"
    );
    assert_eq!(
        table
            .declare(1, LifecycleDecl::awake())
            .expect("block 1 is declared once"),
        ObjectiveLifecycle::Awake,
        "a block without BEGIN_DORMANT is recorded awake"
    );
    assert_eq!(
        table
            .declare(2, LifecycleDecl::dormant(Some(-1.0)))
            .expect("block 2 is declared once"),
        ObjectiveLifecycle::Dormant
    );
    assert_eq!(
        table.declare(0, LifecycleDecl::awake()),
        Err(LifecycleError::Duplicate { index: 0 }),
        "a block is declared once"
    );

    // The caller folds the lifecycle facts into whatever else it observed —
    // the documented composition surface.
    let mut facts = MissionFacts::default();
    facts.absorb(table.facts());
    assert_eq!(
        facts.objectives,
        [
            (0, ObjectiveLifecycle::Dormant),
            (1, ObjectiveLifecycle::Awake),
            (2, ObjectiveLifecycle::Dormant)
        ]
        .into_iter()
        .collect()
    );

    let mut state = MissionState::new(&program, SessionGeneration(3));
    let first = state.step(&program, &facts, Tick(1)).unwrap();
    assert!(
        !state.is_completed(SymbolId(0)),
        "the dormant block must not latch at tick 0"
    );
    assert!(
        state.is_completed(SymbolId(1)),
        "the block the record leaves awake completes on the first tick it is awake"
    );
    assert!(
        !state.is_completed(SymbolId(2)),
        "the dependent starts dormant too, so it holds"
    );
    assert_eq!(
        first
            .events
            .iter()
            .map(|event| event.key.source)
            .collect::<Vec<_>>(),
        vec![SymbolId(1)],
        "exactly the awake block completed"
    );

    // The host wakes the dependent (the `WAKE_OBJECTIVE*` effect): its own
    // gate opens, but its dependency is still dormant.
    assert!(table.wake(2), "a dormant block wakes");
    assert!(!table.wake(2), "an awake block is left alone");
    state.step(&program, &table.facts(), Tick(2)).unwrap();
    assert!(
        !state.is_completed(SymbolId(2)),
        "the dependent is held while its dependency is not awake"
    );

    // The dependency's own timed self-wake fires at the spelled second, and
    // the dependent's gate opens with it.
    assert_eq!(table.tick(2.0), vec![0]);
    assert_eq!(table.tick(3.0), vec![], "a woken block does not re-wake");
    state.step(&program, &table.facts(), Tick(3)).unwrap();
    assert!(
        state.is_completed(SymbolId(0)),
        "the block wakes itself at its measured second and completes"
    );
    assert!(
        state.is_completed(SymbolId(2)),
        "the dependent completes once its dependency is awake"
    );
}

/// The completion transition and the freeze it causes: a dependent whose
/// evaluator is still false does not fire, and stays un-fired once its
/// dependency is `Done` — because `Done` is not `Awake`.
#[test]
fn accept_m01_lc_lowering_conditions_a_completed_dependency_freezes_its_dependent() {
    let program = program(vec![objective(
        1,
        "OBJECTIVE2",
        vec![
            BlockDirective::new("TICK_DEPENDS_ON_OBJ", vec![Value::Int(1)]),
            BlockDirective::new("DEDG", vec![Value::Int(9), Value::Int(0)]),
        ],
    )]);
    assert!(
        matches!(
            &program.program().objectives[0].condition,
            Condition::All(items)
                if items.len() == 3
                    && matches!(items[0], Condition::ObjectiveAwake { index: 1 })
                    && matches!(items[1], Condition::ObjectiveAwake { index: 0 })
                    && matches!(items[2], Condition::EnemyGroupDepletion { .. })
        ),
        "the gate carries the block itself and its dependency, then the evaluator"
    );

    let mut table = BlockLifecycleTable::new();
    table
        .declare(0, LifecycleDecl::awake())
        .expect("declared once");
    table
        .declare(1, LifecycleDecl::awake())
        .expect("declared once");

    let mut state = MissionState::new(&program, SessionGeneration(4));
    let mut facts = table.facts();

    // The gate is open but the group still has a member: no completion.
    facts.groups.insert(9, 1);
    state.step(&program, &facts, Tick(1)).unwrap();
    assert!(!state.is_completed(SymbolId(1)));

    // The dependency completes: the dependent freezes even though its own
    // evaluator would now be true.
    assert!(table.complete(0), "completion records the done state");
    assert!(!table.complete(0), "completing twice changes nothing");
    assert_eq!(table.state(0), Some(ObjectiveLifecycle::Done));
    facts = table.facts();
    facts.groups.insert(9, 0);
    state.step(&program, &facts, Tick(2)).unwrap();
    assert!(
        !state.is_completed(SymbolId(1)),
        "a done dependency is not awake, so the dependent never runs again"
    );
    assert_eq!(state.terminal(), cs_script::runtime::TerminalState::Running);
}

/// The table's own measured transitions: the timed self-wake arm test, the
/// sentinel that never arms, and the fail-closed read of an undeclared block.
#[test]
fn accept_m01_lc_lowering_conditions_the_lifecycle_table_drives_the_measured_wake() {
    let mut table = BlockLifecycleTable::new();
    table
        .declare(0, LifecycleDecl::dormant(Some(2.0)))
        .expect("declared once");
    table
        .declare(1, LifecycleDecl::dormant(Some(-1.0)))
        .expect("declared once");
    table
        .declare(2, LifecycleDecl::awake())
        .expect("declared once");
    assert_eq!(table.len(), 3);
    assert!(!table.is_empty());

    // Nothing wakes before the spelled second, and the `-1` sentinel never
    // arms (measured: `+0x5d0 < 0` skips the wake branch).
    assert_eq!(table.tick(0.0), vec![]);
    assert_eq!(table.tick(1.999), vec![]);
    assert_eq!(table.tick(2.0), vec![0]);
    assert_eq!(table.tick(100.0), vec![], "a woken block does not re-wake");
    assert_eq!(table.state(1), Some(ObjectiveLifecycle::Dormant));
    assert_eq!(table.state(2), Some(ObjectiveLifecycle::Awake));

    // An undeclared block is absent, and absence is never "awake".
    assert_eq!(table.state(7), None);
    let facts = table.facts();
    assert!(!facts.objectives.contains_key(&7));
    assert!(
        !MissionState::new(
            &program(vec![objective(7, "OBJECTIVE8", vec![])]),
            SessionGeneration(5)
        )
        .holds(&Condition::ObjectiveAwake { index: 7 }, &facts)
    );

    // A wake time no clock comparison can decide is refused, not folded into
    // "dormant, never waking" — and the refusal changes nothing.
    let mut fresh = BlockLifecycleTable::new();
    assert!(matches!(
        fresh.declare(3, LifecycleDecl::dormant(Some(f64::NAN))),
        Err(LifecycleError::NonFiniteWake { index: 3, .. })
    ));
    assert!(
        fresh.state(3).is_none(),
        "a refused declaration changes nothing"
    );
}

/// The session's own event stream carries the completion the lifecycle table
/// records — one `ObjectiveCompleted` per block that fires, in `EventKey`
/// order, so a host can fold events into `complete()`.
#[test]
fn accept_m01_lc_lowering_conditions_a_firing_block_emits_one_completion() {
    let program = program(vec![
        objective(0, "OBJECTIVE1", vec![dormant(1.0)]),
        objective(1, "OBJECTIVE2", vec![]),
    ]);
    let mut table = BlockLifecycleTable::new();
    table
        .declare(0, LifecycleDecl::dormant(Some(1.0)))
        .expect("declared once");
    table
        .declare(1, LifecycleDecl::awake())
        .expect("declared once");

    let mut state = MissionState::new(&program, SessionGeneration(6));
    let first = state.step(&program, &table.facts(), Tick(1)).unwrap();
    assert_eq!(first.events.len(), 1);
    assert_eq!(first.events[0].key.source, SymbolId(1));
    assert_eq!(first.events[0].kind, EventKind::ObjectiveCompleted);

    // The host folds the completion into the lifecycle table, which is what
    // freezes the dependent side of the world: the block is done, not awake.
    assert!(table.complete(1));
    let second = state.step(&program, &table.facts(), Tick(2)).unwrap();
    assert!(
        second.events.is_empty(),
        "a completed block completes exactly once"
    );
    assert_eq!(
        table.facts().objectives.get(&1),
        Some(&ObjectiveLifecycle::Done)
    );
}
