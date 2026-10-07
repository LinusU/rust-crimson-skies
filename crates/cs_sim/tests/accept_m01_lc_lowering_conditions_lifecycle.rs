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
/// Two open gates on one tick complete one at a time, lowest declaration
/// index first, on consecutive ticks.
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
            .declare(2, LifecycleDecl::dormant(Some(-1.0)).depends_on(0),)
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
    // the dependent's gate opens with it. Both conditions now hold, but at
    // most one objective completes per tick — the lowest declared index
    // whose condition holds (`f37.rule.terminal_precedence.one_completion_per_tick`, #729).
    assert_eq!(table.tick(2.0), vec![0]);
    assert_eq!(table.tick(3.0), vec![], "a woken block does not re-wake");
    let third = state.step(&program, &table.facts(), Tick(3)).unwrap();
    assert!(
        state.is_completed(SymbolId(0)),
        "the block wakes itself at its measured second and completes"
    );
    assert_eq!(
        third
            .events
            .iter()
            .map(|event| event.key.source)
            .collect::<Vec<_>>(),
        vec![SymbolId(0)],
        "exactly one completion on this tick, the lower declared index"
    );
    assert!(
        !state.is_completed(SymbolId(2)),
        "the dependent's gate is open, but it waits for a later tick"
    );

    // Its condition is re-evaluated every tick, never latched early: on the
    // next tick the dependent is the lowest satisfied index and completes.
    let fourth = state.step(&program, &table.facts(), Tick(4)).unwrap();
    assert!(
        state.is_completed(SymbolId(2)),
        "the dependent completes once its dependency is awake"
    );
    assert_eq!(
        fourth
            .events
            .iter()
            .map(|event| event.key.source)
            .collect::<Vec<_>>(),
        vec![SymbolId(2)],
        "one completion per tick, in declaration-index order"
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

/// The measured pass-1 gate on the timed self-wake: a block that names a
/// `TICK_DEPENDS_ON_OBJ` target does not run its own wake timer while that
/// dependency is not awake — the dependency delays the dependent's whole
/// lifecycle, not only its completion (finding B, pass 1). An external wake
/// is not gated, because the original's wake call checks killed and completed
/// records only, and a dependency nobody declared reads as not awake.
#[test]
fn accept_m01_lc_lowering_conditions_the_timed_self_wake_waits_for_its_dependency() {
    // Block 1's wake time is due well before block 0's, but its dependency is
    // block 0 — so it stays dormant until the dependency wakes.
    let mut table = BlockLifecycleTable::new();
    table
        .declare(0, LifecycleDecl::dormant(Some(5.0)))
        .expect("block 0 is declared once");
    table
        .declare(1, LifecycleDecl::dormant(Some(1.0)).depends_on(0))
        .expect("block 1 is declared once");
    assert_eq!(
        table.tick(2.0),
        Vec::<u32>::new(),
        "block 1's wake time has passed, but its dependency is still dormant"
    );
    assert_eq!(table.state(1), Some(ObjectiveLifecycle::Dormant));

    // When the dependency wakes, the dependent's own (long overdue) timer
    // fires in the same call — record order, the same sequencing the
    // original's per-objective loop has.
    assert_eq!(table.tick(6.0), vec![0, 1]);
    assert_eq!(table.state(1), Some(ObjectiveLifecycle::Awake));
    assert_eq!(
        table.tick(7.0),
        Vec::<u32>::new(),
        "a woken block does not re-wake"
    );

    // A dependency nobody declared is never awake, so the dependent never
    // self-wakes: absence fails closed rather than opening the gate.
    let mut unknown = BlockLifecycleTable::new();
    unknown
        .declare(0, LifecycleDecl::dormant(Some(1.0)).depends_on(9))
        .expect("block 0 is declared once");
    assert_eq!(unknown.tick(10.0), Vec::<u32>::new());
    assert_eq!(unknown.state(0), Some(ObjectiveLifecycle::Dormant));
    assert_eq!(unknown.state(9), None);

    // A dependency that already completed is not awake either: the dependent
    // stays dormant for the rest of the mission, while an *external* wake
    // still works (the wake call has no dependency gate).
    let mut done = BlockLifecycleTable::new();
    done.declare(0, LifecycleDecl::awake())
        .expect("declared once");
    done.declare(1, LifecycleDecl::dormant(Some(1.0)).depends_on(0))
        .expect("block 1 is declared once");
    assert!(done.complete(0), "the dependency completes");
    assert_eq!(done.tick(10.0), Vec::<u32>::new());
    assert!(done.wake(1), "the external wake is not dependency-gated");
    assert_eq!(done.state(1), Some(ObjectiveLifecycle::Awake));
}
