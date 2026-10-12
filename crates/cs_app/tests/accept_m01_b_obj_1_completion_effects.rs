//! M01-B-OBJ-1: what `WAKE`, `NAP`, `KILL` and `TICK_DEPENDS_ON_OBJ` do, as
//! measured from the engine image, and their recovery in `ObjectiveRecovery`.
//!
//! The behavior is `cs_content::objectives_engine`; these tests drive it through
//! the production recovery of an `objectives.zrd` document.

use std::path::PathBuf;

use cs_app::objectives::{
    ObjectiveRecovery, RecordFieldFamily, recover_all_retail_objectives, recover_retail_objectives,
};
use cs_content::objectives_engine::{
    ENGINE_DEFAULT_NAP_TIME, EngineObjective, EngineObjectives, EngineState,
};
use cs_content::stunts::ZrdValue;

fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

fn ints(numbers: &[u32]) -> ZrdValue {
    ZrdValue::List(
        numbers
            .iter()
            .map(|number| ZrdValue::Int(*number))
            .collect(),
    )
}

fn block(fields: Vec<(&str, ZrdValue)>) -> ZrdValue {
    ZrdValue::List(
        fields
            .into_iter()
            .flat_map(|(key, value)| [text(key), value])
            .collect(),
    )
}

fn record(blocks: Vec<(&str, ZrdValue)>) -> ZrdValue {
    ZrdValue::List(vec![block(blocks)])
}

fn recovery(blocks: Vec<(&str, ZrdValue)>) -> ObjectiveRecovery {
    ObjectiveRecovery::read("zbd/test/m00", &record(blocks))
}

fn engine(recovery: &ObjectiveRecovery, state: EngineState) -> EngineObjectives {
    let objectives = (1..=u32::try_from(recovery.engine_objective_count()).expect("small"))
        .map(|number| {
            let declaration = recovery
                .engine_declarations()
                .get(&number)
                .cloned()
                .unwrap_or_default();
            EngineObjective::new(state, declaration)
        })
        .collect();
    EngineObjectives::new(objectives).expect("every target is in range")
}

#[test]
fn accept_m01_b_obj_1_recovers_the_four_measured_keys() {
    let recovery = recovery(vec![
        (
            "OBJECTIVE1",
            block(vec![
                ("WAKE_OBJECTIVE_WHEN_I_COMPLETE", ints(&[2, 3])),
                ("NAP_OBJECTIVE_WHEN_I_COMPLETE", {
                    ZrdValue::List(vec![ZrdValue::Int(3), ZrdValue::Float(2.5)])
                }),
                ("KILL_OBJECTIVE_WHEN_I_COMPLETE", ints(&[3])),
            ]),
        ),
        (
            "OBJECTIVE2",
            block(vec![("TICK_DEPENDS_ON_OBJ", ZrdValue::Int(1))]),
        ),
        (
            "OBJECTIVE3",
            block(vec![("BEGIN_DORMANT", ZrdValue::Int(1))]),
        ),
    ]);
    assert_eq!(recovery.fields_read(), 5);
    assert_eq!(recovery.fields_recovered(), 4);
    assert_eq!(recovery.engine_objective_count(), 3);
    let first = &recovery.engine_declarations()[&1];
    assert_eq!(first.wake.targets(), &[1, 2]);
    assert_eq!(first.kill.targets(), &[2]);
    let nap = first.nap.expect("nap");
    assert_eq!(nap.target, Some(2));
    assert!((nap.time - 2.5).abs() < f32::EPSILON);
    assert_eq!(recovery.engine_declarations()[&2].tick_depends_on, Some(0));
    assert!(!recovery.engine_declarations().contains_key(&3));
    // Only the dormancy field is left, and the program still refuses naming it.
    let families = recovery.unrecovered_by_family();
    assert_eq!(families.len(), 1);
    assert_eq!(families[&RecordFieldFamily::Dormancy], 1);
    let refusal = recovery.program().expect_err("dormancy is unrecovered");
    assert!(refusal.to_string().contains("Dormancy x1"));
}

#[test]
fn accept_m01_b_obj_1_effects_apply_in_the_engines_order_not_the_records() {
    // The one retail collision (`zbd/c3/m05` OBJECTIVE8): WAKE and NAP both name
    // one objective. Spelled either way round, the engine applies WAKE first, so
    // the target ends napping.
    let wake = ("WAKE_OBJECTIVE_WHEN_I_COMPLETE", ints(&[2]));
    let nap = (
        "NAP_OBJECTIVE_WHEN_I_COMPLETE",
        ZrdValue::List(vec![ZrdValue::Int(2), ZrdValue::Float(2.0)]),
    );
    for fields in [vec![wake.clone(), nap.clone()], vec![nap, wake]] {
        let recovery = recovery(vec![
            ("OBJECTIVE1", block(fields)),
            (
                "OBJECTIVE2",
                block(vec![("BEGIN_DORMANT", ZrdValue::Int(1))]),
            ),
        ]);
        assert_eq!(recovery.fields_recovered(), 2);
        let mut set = engine(&recovery, EngineState::Dormant);
        set.complete(0);
        let target = set.get(1).expect("target");
        assert_eq!(target.state, EngineState::Napping);
        assert!(!target.active);
    }
}

#[test]
fn accept_m01_b_obj_1_a_nap_without_a_time_takes_the_engines_default() {
    let recovery = recovery(vec![
        (
            "OBJECTIVE1",
            block(vec![("NAP_OBJECTIVE_WHEN_I_COMPLETE", ZrdValue::Int(2))]),
        ),
        ("OBJECTIVE2", block(vec![])),
    ]);
    assert_eq!(recovery.fields_recovered(), 1);
    let nap = recovery.engine_declarations()[&1].nap.expect("nap");
    assert!((nap.time - ENGINE_DEFAULT_NAP_TIME).abs() < f32::EPSILON);
    assert!((ENGINE_DEFAULT_NAP_TIME - 0.3).abs() < 1e-6);
}

#[test]
fn accept_m01_b_obj_1_unmeasured_shapes_stay_unrecovered() {
    let recovery = recovery(vec![
        (
            "OBJECTIVE1",
            block(vec![
                // A spelling the engine's loader never reads.
                ("WAKEUP_OBJECTIVE_WHEN_I_COMPLETE", ints(&[2])),
                // A target past the objectives the engine loads.
                ("KILL_OBJECTIVE_WHEN_I_COMPLETE", ints(&[9])),
                // A string where the engine reads an integer.
                (
                    "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
                    ZrdValue::List(vec![text("x")]),
                ),
                // Two numbers where the dependency is one.
                ("TICK_DEPENDS_ON_OBJ", ints(&[1, 2])),
            ]),
        ),
        ("OBJECTIVE2", block(vec![])),
    ]);
    assert_eq!(recovery.fields_recovered(), 0);
    assert_eq!(recovery.unrecovered().len(), 4);
    assert!(recovery.engine_declarations().is_empty());
}

#[test]
fn accept_m01_b_obj_1_only_the_first_occurrence_of_a_key_is_the_engines() {
    let recovery = recovery(vec![
        (
            "OBJECTIVE1",
            block(vec![
                ("KILL_OBJECTIVE_WHEN_I_COMPLETE", ints(&[2])),
                ("KILL_OBJECTIVE_WHEN_I_COMPLETE", ints(&[3])),
            ]),
        ),
        ("OBJECTIVE2", block(vec![])),
        ("OBJECTIVE3", block(vec![])),
    ]);
    assert_eq!(recovery.fields_recovered(), 1);
    assert_eq!(recovery.unrecovered().len(), 1);
    assert_eq!(recovery.engine_declarations()[&1].kill.targets(), &[1]);
}

#[test]
fn accept_m01_b_obj_1_the_engine_loads_objectives_only_up_to_the_first_gap() {
    let recovery = recovery(vec![
        ("OBJECTIVE1", block(vec![])),
        ("OBJECTIVE2", block(vec![])),
        (
            "OBJECTIVE4",
            block(vec![("KILL_OBJECTIVE_WHEN_I_COMPLETE", ints(&[1]))]),
        ),
    ]);
    assert_eq!(recovery.engine_objective_count(), 2);
    // OBJECTIVE4 is never loaded, so nothing of it is recovered.
    assert_eq!(recovery.fields_recovered(), 0);
}

#[test]
fn accept_m01_b_obj_1_a_dependent_is_evaluated_only_while_its_dependency_is_awake() {
    let recovery = recovery(vec![
        ("OBJECTIVE1", block(vec![])),
        (
            "OBJECTIVE2",
            block(vec![("TICK_DEPENDS_ON_OBJ", ZrdValue::Int(1))]),
        ),
    ]);
    let mut set = engine(&recovery, EngineState::Awake);
    assert!(set.evaluates_completion(1));
    set.complete(0);
    assert!(!set.is_ticked(1));
}

fn retail_root() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_b_obj_1_retail_m01_recovers_its_effects_and_dependencies() {
    let recovery = recover_retail_objectives(&retail_root(), "zbd/c1c/m01").expect("M01 reads");
    // The loader reads OBJECTIVE1.. to the first gap: M01 declares 58 blocks.
    assert_eq!(recovery.blocks().len(), 58);
    assert_eq!(recovery.engine_objective_count(), 58);

    let by_family = |family| {
        recovery
            .recovered()
            .iter()
            .filter(|field| field.family == family)
            .count()
    };
    assert_eq!(by_family(RecordFieldFamily::CompletionEffect), 56);
    assert_eq!(by_family(RecordFieldFamily::OrderDependency), 3);
    assert_eq!(recovery.fields_recovered(), 59);
    assert_eq!(
        recovery.fields_recovered() + recovery.unrecovered().len(),
        recovery.fields_read()
    );
    // None of M01's effect or dependency fields is left behind.
    let leftover = recovery.unrecovered_by_family();
    assert!(!leftover.contains_key(&RecordFieldFamily::CompletionEffect));
    assert!(!leftover.contains_key(&RecordFieldFamily::OrderDependency));

    // The recovered model builds: every target names a loaded objective.
    let mut set = engine(&recovery, EngineState::Awake);
    for number in recovery.engine_declarations().keys() {
        set.complete(*number as usize - 1);
    }
    // And the program still refuses, naming what is left.
    let refusal = recovery
        .program()
        .expect_err("other families are unmeasured");
    assert!(!refusal.to_string().contains("CompletionEffect"));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_b_obj_1_retail_corpus_recovers_every_measured_key() {
    let all = recover_all_retail_objectives(&retail_root()).expect("every mission reads");
    assert_eq!(all.len(), 53);
    let (mut recovered, mut left_effects) = (0usize, 0usize);
    for recovery in &all {
        // Every record's blocks are contiguous from 1: the engine loads them all.
        assert_eq!(
            recovery.engine_objective_count(),
            recovery.blocks().len(),
            "{} has a gap in its objective numbers",
            recovery.mission()
        );
        engine_builds(recovery);
        recovered += recovery.fields_recovered();
        left_effects += recovery
            .unrecovered()
            .iter()
            .filter(|field| field.family == RecordFieldFamily::CompletionEffect)
            .count();
    }
    // 412 WAKE + 417 NAP + 225 KILL + 35 TICK_DEPENDS (F39-E2); the two `WAKEUP`
    // sites are a spelling the loader never reads.
    assert_eq!(recovered, 412 + 417 + 225 + 35);
    assert_eq!(left_effects, 2);
}

fn engine_builds(recovery: &ObjectiveRecovery) {
    let mut set = engine(recovery, EngineState::Awake);
    for number in recovery.engine_declarations().keys() {
        set.complete(*number as usize - 1);
    }
}
