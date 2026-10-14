//! F37-D-FU3-DOC acceptance (Rally #738): the `MissionProgram` doc comment in
//! `crates/cs_script/src/ir.rs` must describe the execution order the runtime
//! has followed since F37-D-FU3 (#729) — the completion scan admits **at most
//! one** objective per tick, the lowest declaration index whose condition
//! holds — and must no longer cite the closed `f37.d.limit.one_completion_per_tick`
//! divergence, which no longer exists in `RULE_LIMITATIONS`. The two-orders
//! distinction the comment documents — execution order versus `EventKey`
//! observation order — stays.
//!
//! Task test prefix: `accept_f37_d_fu3_doc_`. This is a doc comment, so the
//! pin is source-level: the tests read the shipped `src/ir.rs` through
//! `include_str!`, so reverting the bullet to the closed divergence — or
//! dropping the distinction it draws — fails them. The same tests drive the
//! production items the bullet names (`RULE_LIMITATIONS`, the rule labels,
//! `EventKey` and `MissionState::step`), so the pinned text is the behaviour
//! the runtime actually has.

use cs_script::ir::*;
use cs_script::runtime::*;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

/// The file under review, read from the crate's own sources.
const IR_RS: &str = include_str!("../src/ir.rs");

/// Fixed session generation for the `EventKey` and `MissionState::step`
/// probes.
const SESSION: SessionGeneration = SessionGeneration(738);

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

fn program(objectives: Vec<Objective>) -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-f37-d-fu3-doc"),
        variables: vec![],
        objectives,
    }
}

fn constant_true(id: u32, reward: &str) -> Objective {
    Objective {
        id: SymbolId(id),
        content: cid(
            ContentKind::Objective,
            &format!("synthetic-fu3-doc-obj-{id}"),
        ),
        condition: Condition::Const(true),
        actions: vec![Action::GrantReward {
            reward: cid(ContentKind::Blueprint, reward),
        }],
        span: None,
    }
}

/// The ids of the objectives that completed on `tick`'s result, in emission
/// order.
fn completed_on(result: &TickResult) -> Vec<u32> {
    result
        .events
        .iter()
        .filter(|event| matches!(event.kind, EventKind::ObjectiveCompleted))
        .map(|event| event.key.source.0)
        .collect()
}

/// **The bullet states the measured rule the runtime now follows.**
///
/// The execution-order bullet must say the scan admits at most one objective
/// per tick — the lowest declaration index whose condition holds — and that a
/// satisfied objective the scan passes over stays unfired for a later tick.
/// The stale claims it replaces cannot be present anywhere in the file: the
/// `f37.d.limit.one_completion_per_tick` limitation the bullet cited was
/// removed with #729, and "this runtime completes every satisfied one" is the
/// divergence the same task closed.
#[test]
fn accept_f37_d_fu3_doc_execution_order_bullet_states_one_completion_per_tick() {
    for phrase in [
        "**Execution order**",
        "admits **at most one**",
        "declaration index whose condition holds",
        "stays unfired and is re-evaluated",
        "crate::runtime::TICK_ORDERING_RULE",
    ] {
        assert!(
            IR_RS.contains(phrase),
            "crates/cs_script/src/ir.rs must state the closed rule in the \
             `MissionProgram` execution-order bullet — missing {phrase:?}"
        );
    }
    for stale in [
        "f37.d.limit.one_completion_per_tick",
        "this runtime completes every",
        "that divergence is the machine-readable",
    ] {
        assert!(
            !IR_RS.contains(stale),
            "crates/cs_script/src/ir.rs still carries the closed divergence: \
             {stale:?}"
        );
    }
}

/// **The two-orders distinction stays.** Execution order is declaration
/// order; observation order is `EventKey` and does not depend on it. Closing
/// the divergence changes only the first bullet — the comment must still draw
/// both, with the observation order labelled `designed-and-unmeasured`.
#[test]
fn accept_f37_d_fu3_doc_keeps_the_execution_vs_observation_distinction() {
    let execution = IR_RS
        .find("**Execution order**")
        .expect("the execution-order bullet must still exist");
    let observation = IR_RS
        .find("**Observation order**")
        .expect("the observation-order bullet must still exist");
    assert!(
        execution < observation,
        "the `MissionProgram` comment must keep execution order and \
         observation order as separate bullets"
    );
    for phrase in [
        "crate::runtime::EventKey",
        "crate::runtime::EVENT_OBSERVATION_ORDER_RULE",
        "designed-and-unmeasured",
    ] {
        assert!(
            IR_RS.contains(phrase),
            "the observation-order bullet must still name {phrase:?}"
        );
    }
}

/// **The pinned text is the runtime's real behaviour.** Every production item
/// the bullets name is exercised here: `RULE_LIMITATIONS` holds no
/// `f37.d.limit.one_completion_per_tick` entry for the doc to cite, the rule
/// labels carry the source labels the comment claims, `EventKey` sorts the
/// way the observation bullet describes, and `MissionState::step` completes
/// at most one satisfied objective per tick — lowest declaration index
/// first — exactly what the corrected bullet says.
#[test]
fn accept_f37_d_fu3_doc_pinned_items_match_the_runtime() {
    // The limitation id the old bullet linked is gone; nothing in the open
    // list may carry it back.
    assert!(
        RULE_LIMITATIONS
            .iter()
            .all(|limitation| limitation.id != "f37.d.limit.one_completion_per_tick"),
        "RULE_LIMITATIONS must not regain the closed \
         f37.d.limit.one_completion_per_tick entry"
    );

    // The source labels the comment attributes: the tick's execution order is
    // measured from the original; the observation order is the project's
    // design.
    assert_eq!(
        TICK_ORDERING_RULE.source,
        RuleSource::MeasuredFromOriginalStatic,
        "the execution-order rule must stay measured-from-original"
    );
    assert_eq!(
        EVENT_OBSERVATION_ORDER_RULE.source,
        RuleSource::DesignedAndUnmeasured,
        "the observation-order rule must stay designed-and-unmeasured"
    );

    // EventKey is the observation order the bullet documents: session first,
    // then tick, source symbol, sequence — declaration order plays no part.
    let key = EventKey {
        session: SESSION,
        tick: Tick(1),
        source: SymbolId(2),
        sequence: 0,
    };
    assert!(
        EventKey {
            session: SessionGeneration(1),
            tick: Tick(9),
            source: SymbolId(9),
            sequence: 9,
        } < key,
        "session leads the EventKey order"
    );
    assert!(
        key < EventKey {
            tick: Tick(2),
            source: SymbolId(1),
            ..key
        },
        "tick orders before source symbol"
    );
    assert!(
        key < EventKey { sequence: 1, ..key },
        "sequence is the last component"
    );

    // And the behaviour the bullet states: two satisfied objectives, one
    // completion per tick in declaration order.
    let p = program(vec![
        constant_true(1, "r-first"),
        constant_true(2, "r-second"),
    ])
    .validate()
    .unwrap();
    let mut state = MissionState::new(&p, SESSION, 1);
    let first = state.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    let second = state.step(&p, &MissionFacts::default(), Tick(2)).unwrap();
    assert_eq!(
        completed_on(&first),
        vec![1],
        "at most one objective completes per tick — the lowest declaration \
         index whose condition holds"
    );
    assert_eq!(
        completed_on(&second),
        vec![2],
        "the objective that lost the scan completes on a later tick"
    );
}
