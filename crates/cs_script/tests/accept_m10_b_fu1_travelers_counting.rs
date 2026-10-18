//! M10-B-FU1: the `TRAVELERS` counting mode as a runtime condition.
//!
//! M10 spells `TRAVELERS` twice with a numeric child0
//! (`[2, APPROACHING, cargozep2, 200, 1]` and
//! `[2, LEAVING, cargozep2, 2000, 1, DELETE_ON_SUCCESS]`). These tests lower
//! those exact spellings and run the lowered condition through the tick
//! pipeline: the objective's running total is state, accumulated per
//! evaluation and cleared when the objective fires. The behaviour under test
//! is the one finding `2026-10-06-m01-lc-directive-c-…` measured at
//! `0x465b40`/`0x4659b0`; nothing here is an original run.

use cs_script::conditions::{BlockDirective, lower_block_condition};
use cs_script::ir::{
    Condition, IR_VERSION, MemberName, MissionProgram, Objective, SymbolId,
    TRAVELERS_COUNT_DELETE_HOST_EFFECT, TravelersAnchor, Value,
};
use cs_script::runtime::{
    MemberFact, MemberPresence, MissionFacts, MissionState, SessionGeneration,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

fn s(text: &str) -> Value {
    Value::Str(text.to_owned())
}

/// M10 block 22, as the record spells it.
fn block_22() -> Vec<BlockDirective> {
    block_22_requiring(1)
}

/// Block 22 with another required total.
fn block_22_requiring(required: i32) -> Vec<BlockDirective> {
    vec![BlockDirective::new(
        "TRAVELERS",
        vec![
            Value::Int(2),
            s("APPROACHING"),
            s("cargozep2"),
            Value::Float(200.0),
            Value::Int(required),
        ],
    )]
}

/// M10 block 35, as the record spells it.
fn block_35() -> Vec<BlockDirective> {
    vec![BlockDirective::new(
        "TRAVELERS",
        vec![
            Value::Int(2),
            s("LEAVING"),
            s("cargozep2"),
            Value::Float(2000.0),
            Value::Int(1),
            s("DELETE_ON_SUCCESS"),
        ],
    )]
}

/// The lowered block: the awake gate and the counting leaf behind it.
fn lowered(spelled: Vec<BlockDirective>) -> Condition {
    lower_block_condition("OBJECTIVE22", 0, &spelled).expect("the counting site lowers")
}

fn counting(spelled: Vec<BlockDirective>) -> Condition {
    match lowered(spelled) {
        Condition::All(mut items) if items.len() == 2 => items.pop().unwrap(),
        other => panic!("the awake gate and one evaluator, not {other:?}"),
    }
}

/// The gated condition a program carries.
fn gated(spelled: Vec<BlockDirective>) -> Condition {
    lowered(spelled)
}

fn program(condition: Condition) -> cs_script::ir::ValidatedProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: ContentId::from_source(ContentKind::Mission, "synthetic").unwrap(),
        variables: vec![],
        objectives: vec![Objective {
            id: SymbolId(1),
            content: ContentId::from_source(ContentKind::Objective, "o1").unwrap(),
            condition,
            actions: vec![],
            span: None,
        }],
    }
    .validate()
    .expect("the lowered counting condition validates")
}

fn zeppelin_at_origin() -> (MemberName, MemberFact) {
    (
        MemberName::from(["cargozep2".to_owned()]),
        MemberFact {
            presence: MemberPresence::InPlay,
            position: [0.0; 3],
        },
    )
}

fn facts(members: &[[f64; 3]]) -> MissionFacts {
    let mut facts = MissionFacts::default();
    let (name, row) = zeppelin_at_origin();
    facts.members.insert(name, row);
    facts
        .objectives
        .insert(0, cs_script::runtime::ObjectiveLifecycle::Awake);
    facts.group_members.insert(2, members.to_vec());
    facts
}

fn completed(
    state: &mut MissionState,
    p: &cs_script::ir::ValidatedProgram,
    f: &MissionFacts,
    tick: u64,
) -> bool {
    let result = state.step(p, f, Tick(tick)).unwrap();
    !result.events.is_empty()
}

#[test]
fn accept_m10_b_fu1_both_m10_sites_lower_to_a_counting_condition() {
    let Condition::TravelersCount {
        group,
        anchor,
        radius,
        approaching,
        required,
        delete_matched,
    } = counting(block_22())
    else {
        panic!("block 22 lowers to the counting mode");
    };
    assert_eq!(group, 2);
    assert_eq!(
        anchor,
        TravelersAnchor::Object(vec!["cargozep2".to_owned()])
    );
    assert_eq!(radius, 200.0);
    assert!(approaching);
    assert_eq!(required, 1);
    assert!(!delete_matched);

    let leaving = counting(block_35());
    let Condition::TravelersCount {
        approaching,
        delete_matched,
        radius,
        ..
    } = &leaving
    else {
        panic!("block 35 lowers to the counting mode");
    };
    // `LEAVING` is not `APPROACHING`: the parse stores 0, the outside pole.
    assert!(!approaching);
    assert!(delete_matched);
    assert_eq!(*radius, 2000.0);
    assert_eq!(
        leaving.residual_unknowns(),
        vec![TRAVELERS_COUNT_DELETE_HOST_EFFECT],
        "the deletion is named, not dropped"
    );
}

#[test]
fn accept_m10_b_fu1_inside_the_radius_completes_and_outside_does_not() {
    let p = program(gated(block_22()));
    let mut outside = MissionState::new(&p, SessionGeneration(1), 1);
    assert!(!completed(
        &mut outside,
        &p,
        &facts(&[[300.0, 0.0, 0.0]]),
        1
    ));
    // Strict: exactly on the radius is neither inside nor outside.
    assert!(!completed(
        &mut outside,
        &p,
        &facts(&[[200.0, 0.0, 0.0]]),
        2
    ));
    assert!(!completed(&mut outside, &p, &facts(&[]), 3));
    let mut inside = MissionState::new(&p, SessionGeneration(1), 1);
    assert!(completed(&mut inside, &p, &facts(&[[100.0, 0.0, 0.0]]), 1));
}

#[test]
fn accept_m10_b_fu1_leaving_counts_the_members_outside_the_radius() {
    let p = program(gated(block_35()));
    let mut state = MissionState::new(&p, SessionGeneration(1), 1);
    assert!(!completed(&mut state, &p, &facts(&[[1999.0, 0.0, 0.0]]), 1));
    assert!(completed(
        &mut state,
        &p,
        &facts(&[[1999.0, 0.0, 0.0], [2001.0, 0.0, 0.0]]),
        2
    ));
}

#[test]
fn accept_m10_b_fu1_the_running_total_accumulates_across_ticks() {
    // Three required, two members inside every tick: 2, then 4 >= 3.
    let p = program(gated(block_22_requiring(3)));
    let mut state = MissionState::new(&p, SessionGeneration(1), 1);
    let two_inside = facts(&[[1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]);
    assert!(!completed(&mut state, &p, &two_inside, 1));
    assert!(completed(&mut state, &p, &two_inside, 2));
}

#[test]
fn accept_m10_b_fu1_the_running_total_survives_a_save_record() {
    let p = program(gated(block_22_requiring(3)));
    let mut state = MissionState::new(&p, SessionGeneration(1), 1);
    let two_inside = facts(&[[1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]);
    assert!(!completed(&mut state, &p, &two_inside, 1));
    let record = state.snapshot(&p);
    assert_eq!(record.travelers_totals, vec![(SymbolId(1), 2)]);
    let mut restored = MissionState::restore(&p, record).expect("the record restores");
    assert!(completed(&mut restored, &p, &two_inside, 2));
    // A fresh state has no total, so the same tick does not complete.
    let mut fresh = MissionState::new(&p, SessionGeneration(1), 1);
    assert!(!completed(&mut fresh, &p, &two_inside, 2));
}

#[test]
fn accept_m10_b_fu1_an_unobserved_group_counts_nothing() {
    let p = program(gated(block_22()));
    let mut state = MissionState::new(&p, SessionGeneration(1), 1);
    let mut f = facts(&[]);
    f.group_members.clear();
    assert!(!completed(&mut state, &p, &f, 1));
    // `holds` alone answers one evaluation from a total of zero.
    assert!(state.holds(&gated(block_22()), &facts(&[[1.0, 0.0, 0.0]])));
}
