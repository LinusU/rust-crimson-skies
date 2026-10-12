//! M01-B-OBJ-3: what an `INACTIVE<n>` stage condition, `INACTIVE_COMPLETION_COUNT`
//! and `INSTANTWIN`/`INSTANTLOSS` mean, as measured from the engine image, and
//! their recovery in `ObjectiveRecovery`.
//!
//! The behavior is `cs_content::objectives_conditions`; these tests drive it
//! through the production recovery of an `objectives.zrd` document, and the
//! retail test compares it with the F39-E1 census, which reads the same record
//! with a different, stricter parser.

use std::path::PathBuf;

use cs_app::objectives::{
    ObjectiveRecovery, RecordFieldFamily, recover_retail_objectives, survey_retail_dormant_reveal,
};
use cs_content::objectives_conditions::{
    ENGINE_MAX_STAGE, INSTANT_END_DELAY, MissionEnd, MissionEndFlags, NodeState,
    ORDINARY_END_DELAY, ObjectiveOutcome, StageConditions, StagePath,
};
use cs_content::stunts::ZrdValue;

fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

fn path(names: &[&str]) -> ZrdValue {
    ZrdValue::List(names.iter().map(|name| text(name)).collect())
}

fn count(value: u32) -> ZrdValue {
    ZrdValue::List(vec![ZrdValue::Int(value)])
}

fn block(fields: Vec<(&str, ZrdValue)>) -> ZrdValue {
    ZrdValue::List(
        fields
            .into_iter()
            .flat_map(|(key, value)| [text(key), value])
            .collect(),
    )
}

fn recovery(blocks: Vec<(&str, ZrdValue)>) -> ObjectiveRecovery {
    ObjectiveRecovery::read("zbd/test/m00", &ZrdValue::List(vec![block(blocks)]))
}

fn read(fields: &[(&str, ZrdValue)]) -> cs_content::objectives_conditions::StageConditionRead {
    let borrowed: Vec<(&str, &ZrdValue)> =
        fields.iter().map(|(key, value)| (*key, value)).collect();
    StageConditions::read(&borrowed)
}

#[test]
fn accept_m01_b_obj_3_the_loader_reads_canonical_stage_keys_one_to_a_hundred() {
    let fields = [
        ("INACTIVE3", path(&["zep", "reng3", "healthy"])),
        ("INACTIVE1", path(&["zep", "reng1", "healthy"])),
        // A repeat of a spelling is not the one the loader reads.
        ("INACTIVE1", path(&["zep", "other"])),
        // Never tried: the loader formats `INACTIVE%d` for 1..=100.
        ("INACTIVE0", path(&["zep"])),
        ("INACTIVE101", path(&["zep"])),
        ("INACTIVE01", path(&["zep"])),
        ("INACTIVE100", path(&["gun"])),
        // Read, but not as a list of names.
        ("INACTIVE5", ZrdValue::Int(4)),
        ("INACTIVE6", path(&[])),
        (
            "INACTIVE7",
            ZrdValue::List(vec![text("zep"), ZrdValue::Int(1)]),
        ),
        ("INACTIVE_COMPLETION_COUNT", count(2)),
        ("INACTIVE_COMPLETION_COUNT", count(9)),
    ];
    let read = read(&fields);
    assert_eq!(
        read.recovered,
        [
            true, true, false, false, false, false, true, false, false, false, true, false
        ]
    );
    assert_eq!(ENGINE_MAX_STAGE, 100);
    // Stage order, not record order.
    let numbers: Vec<u32> = read.conditions.stages().iter().map(|(n, _)| *n).collect();
    assert_eq!(numbers, [1, 3, 100]);
    assert_eq!(read.conditions.stages()[0].1.subject(), "zep");
    assert_eq!(read.conditions.stages()[0].1.elements().len(), 3);
    // The first count wins.
    assert_eq!(read.conditions.threshold(), Some(2));
}

#[test]
fn accept_m01_b_obj_3_a_count_the_engine_cannot_hold_is_not_recovered() {
    for value in [
        ZrdValue::Int(3),
        ZrdValue::List(vec![]),
        ZrdValue::List(vec![ZrdValue::Int(1), ZrdValue::Int(2)]),
        ZrdValue::List(vec![ZrdValue::Float(1.0)]),
        // Beyond the signed word the engine stores it in.
        count(u32::MAX),
    ] {
        let read = read(&[("INACTIVE_COMPLETION_COUNT", value)]);
        assert_eq!(read.recovered, [false]);
        assert_eq!(read.conditions.threshold(), None);
    }
    assert_eq!(
        read(&[("INACTIVE_COMPLETION_COUNT", count(0))])
            .conditions
            .threshold(),
        Some(0)
    );
}

/// A scripted node table: names that resolve, and which of those are inactive.
fn state_of(
    inactive: &'static [&'static str],
    unresolved: &'static [&'static str],
) -> impl Fn(u32, &StagePath) -> NodeState {
    move |_, path| {
        let last = path.elements().last().expect("never empty").as_str();
        if unresolved.contains(&last) {
            NodeState::Unresolved
        } else if inactive.contains(&last) {
            NodeState::Inactive
        } else {
            NodeState::Active
        }
    }
}

fn conditions(names: &[&str], threshold: Option<u32>) -> StageConditions {
    let mut fields: Vec<(String, ZrdValue)> = names
        .iter()
        .enumerate()
        .map(|(index, name)| (format!("INACTIVE{}", index + 1), path(&["zep", name])))
        .collect();
    if let Some(threshold) = threshold {
        fields.push(("INACTIVE_COMPLETION_COUNT".to_owned(), count(threshold)));
    }
    let borrowed: Vec<(&str, &ZrdValue)> = fields
        .iter()
        .map(|(key, value)| (key.as_str(), value))
        .collect();
    StageConditions::read(&borrowed).conditions
}

#[test]
fn accept_m01_b_obj_3_the_set_is_met_when_enough_resolved_nodes_are_inactive() {
    // No count: every condition that resolved must be inactive.
    let all = conditions(&["a", "b", "c"], None);
    assert!(!all.is_satisfied(state_of(&[], &[])));
    assert!(!all.is_satisfied(state_of(&["a", "b"], &[])));
    assert!(all.is_satisfied(state_of(&["a", "b", "c"], &[])));
    // A condition that does not resolve is dropped, not counted against the set.
    assert!(all.is_satisfied(state_of(&["a", "b"], &["c"])));
    assert_eq!(all.required(2), 2);

    // A count is a threshold over the inactive nodes.
    let two_of_three = conditions(&["a", "b", "c"], Some(2));
    assert!(!two_of_three.is_satisfied(state_of(&["a"], &[])));
    assert!(two_of_three.is_satisfied(state_of(&["a", "c"], &[])));
    assert!(two_of_three.is_satisfied(state_of(&["a", "b", "c"], &[])));

    // Count zero is met as soon as one condition resolved, nothing needs to fall.
    let zero = conditions(&["a"], Some(0));
    assert!(zero.is_satisfied(state_of(&[], &[])));

    // A count above what resolved can never be met.
    let too_many = conditions(&["a", "b"], Some(3));
    assert!(!too_many.is_satisfied(state_of(&["a", "b"], &[])));
}

#[test]
fn accept_m01_b_obj_3_no_resolved_condition_is_never_satisfied() {
    // Nothing declared, a count alone, or every condition dropped: not met,
    // whatever the count says.
    assert!(!StageConditions::default().is_satisfied(state_of(&[], &[])));
    assert!(!conditions(&[], Some(0)).is_satisfied(state_of(&[], &[])));
    let dropped = conditions(&["a", "b"], Some(0));
    assert!(!dropped.is_satisfied(state_of(&[], &["a", "b"])));
}

#[test]
fn accept_m01_b_obj_3_later_outcome_spellings_override_earlier_ones() {
    use ObjectiveOutcome::{InstantLoss, InstantWin, Lost, Won};
    assert_eq!(ObjectiveOutcome::of_keys(["WAKE"]), None);
    assert_eq!(ObjectiveOutcome::of_keys(["INSTANTWIN"]), Some(InstantWin));
    assert_eq!(
        ObjectiveOutcome::of_keys(["INSTANTWIN", "INSTANTLOSS"]),
        Some(InstantLoss)
    );
    // The order the block spells them in does not matter, the loader's does.
    assert_eq!(
        ObjectiveOutcome::of_keys(["INSTANTLOSS", "INSTANTWIN"]),
        Some(InstantLoss)
    );
    assert_eq!(
        ObjectiveOutcome::of_keys(["INSTANTLOSS", "WON", "INSTANTWIN"]),
        Some(Won)
    );
    assert_eq!(
        ObjectiveOutcome::of_keys(["LOST", "WON", "INSTANTLOSS"]),
        Some(Lost)
    );
    let values: Vec<u8> = [Lost, Won, InstantWin, InstantLoss]
        .into_iter()
        .map(ObjectiveOutcome::engine_value)
        .collect();
    assert_eq!(values, [1, 2, 3, 4]);
}

#[test]
fn accept_m01_b_obj_3_the_lost_flag_is_tested_before_the_won_flag() {
    let mut flags = MissionEndFlags::default();
    assert_eq!(flags.end(), None);

    flags.begin_pass();
    flags.completed(ObjectiveOutcome::InstantWin);
    assert_eq!(flags.end(), Some((MissionEnd::Win, INSTANT_END_DELAY)));

    // Both flags set: the loss ends the mission, and the win is not reported.
    flags.completed(ObjectiveOutcome::InstantLoss);
    assert_eq!(flags.end(), Some((MissionEnd::Loss, INSTANT_END_DELAY)));

    // The instant markers are per pass, the flags persist.
    flags.begin_pass();
    assert_eq!(flags.end(), Some((MissionEnd::Loss, ORDINARY_END_DELAY)));
}

#[test]
fn accept_m01_b_obj_3_won_and_lost_categories_end_the_mission_when_all_complete() {
    let mut flags = MissionEndFlags::default();
    flags.tally(ObjectiveOutcome::Won, 0, 0);
    flags.tally(ObjectiveOutcome::Won, 3, 2);
    assert_eq!(flags.end(), None);
    flags.tally(ObjectiveOutcome::Won, 3, 3);
    assert_eq!(flags.end(), Some((MissionEnd::Win, ORDINARY_END_DELAY)));
    flags.tally(ObjectiveOutcome::Lost, 1, 1);
    assert_eq!(flags.end(), Some((MissionEnd::Loss, ORDINARY_END_DELAY)));
}

#[test]
fn accept_m01_b_obj_3_recovery_reads_what_the_loader_reads_and_names_the_rest() {
    let recovery = recovery(vec![
        (
            "OBJECTIVE1",
            block(vec![
                ("INACTIVE1", path(&["zep", "reng1", "healthy"])),
                ("INACTIVE2", path(&["zep", "reng2", "healthy"])),
                ("INACTIVE_COMPLETION_COUNT", count(1)),
                ("INSTANTWIN", ZrdValue::List(vec![])),
                // Not a shape the loader reads.
                ("INACTIVE3", ZrdValue::Int(7)),
                // Not a shape the loader reads either (M01-B-OBJ-2 recovers the
                // well-formed ones).
                (
                    "BEGIN_DORMANT",
                    ZrdValue::List(vec![ZrdValue::List(vec![])]),
                ),
            ]),
        ),
        (
            "OBJECTIVE2",
            block(vec![
                ("INACTIVE1", path(&["gun"])),
                ("INSTANTLOSS", ZrdValue::List(vec![])),
            ]),
        ),
        // OBJECTIVE3 is absent: the loader stops at the first missing number, so
        // OBJECTIVE4 is never loaded and none of its fields is recovered.
        (
            "OBJECTIVE4",
            block(vec![
                ("INACTIVE1", path(&["zep"])),
                ("INSTANTWIN", ZrdValue::List(vec![])),
            ]),
        ),
    ]);

    assert_eq!(recovery.fields_read(), 10);
    assert_eq!(recovery.fields_recovered(), 6);
    let families = recovery.unrecovered_by_family();
    assert_eq!(families[&RecordFieldFamily::InactiveStage], 2);
    assert_eq!(families[&RecordFieldFamily::Outcome], 1);
    assert_eq!(families[&RecordFieldFamily::Dormancy], 1);

    let conditions = recovery.stage_conditions();
    assert_eq!(conditions.keys().copied().collect::<Vec<_>>(), [1, 2]);
    assert_eq!(conditions[&1].stages().len(), 2);
    assert_eq!(conditions[&1].threshold(), Some(1));
    assert_eq!(conditions[&2].threshold(), None);
    assert_eq!(recovery.outcomes().len(), 2);
    assert_eq!(recovery.outcomes()[&2], ObjectiveOutcome::InstantLoss);

    // The program still refuses, now without naming the recovered families.
    let refusal = recovery
        .program()
        .expect_err("a malformed dormancy is unrecovered");
    assert!(refusal.to_string().contains("Dormancy x1"));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_b_obj_3_retail_m01_recovers_every_condition_and_outcome_the_census_reads() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let recovery = recover_retail_objectives(&root, "zbd/c1c/m01").expect("M01 reads");

    // Independent denominator: F39-E1's strict census of the same record.
    let census = survey_retail_dormant_reveal(&root).expect("census");
    let row = census
        .rows()
        .iter()
        .find(|row| row.mission.eq_ignore_ascii_case("zbd/c1c/m01"))
        .expect("M01 census row");
    assert_eq!(row.block_count(), recovery.blocks().len());

    let mut census_stages = 0;
    let mut census_counts = 0;
    for (index, measured) in row.blocks.iter().enumerate() {
        let number = u32::try_from(index + 1).expect("small");
        let recovered = recovery.stage_conditions().get(&number);
        assert_eq!(
            recovered.map_or(0, |c| c.stages().len()),
            measured.condition_count(),
            "{} stages",
            measured.block
        );
        census_stages += measured.condition_count();
        if let Some(recovered) = recovered {
            for ((stage, path), measured) in recovered.stages().iter().zip(&measured.conditions) {
                assert_eq!(*stage, measured.stage);
                assert_eq!(path.subject(), measured.subject);
                assert_eq!(path.elements().get(1), measured.part.as_ref());
                assert_eq!(path.elements().get(2), measured.attribute.as_ref());
            }
            assert_eq!(
                recovered.threshold(),
                measured.completion_count.map(|c| i32::try_from(c).unwrap())
            );
        }
        if measured.completion_count.is_some() {
            census_counts += 1;
        }
    }
    assert_eq!(
        recovery
            .stage_conditions()
            .values()
            .filter(|c| c.threshold().is_some())
            .count(),
        census_counts
    );

    // Every stage, count and outcome field is recovered; none is left unrecovered.
    let families = recovery.unrecovered_by_family();
    for family in [
        RecordFieldFamily::InactiveStage,
        RecordFieldFamily::CompletionCount,
        RecordFieldFamily::Outcome,
    ] {
        assert!(
            !families.contains_key(&family),
            "{family:?} left unrecovered"
        );
    }
    // Other tasks' families (effects, dependencies, lifecycle) share the
    // recovered list; count only this task's.
    let own_recovered = recovery
        .recovered()
        .iter()
        .filter(|field| {
            matches!(
                field.family,
                RecordFieldFamily::InactiveStage
                    | RecordFieldFamily::CompletionCount
                    | RecordFieldFamily::Outcome
            )
        })
        .count();
    assert_eq!(
        own_recovered,
        census_stages + census_counts + recovery.outcomes().len()
    );

    // M01 declares exactly one instant win and one instant loss.
    assert_eq!(
        recovery.outcomes().values().copied().collect::<Vec<_>>(),
        [ObjectiveOutcome::InstantWin, ObjectiveOutcome::InstantLoss]
    );

    // Other families are unmeasured, so no program is emitted.
    recovery.program().expect_err("other families are open");
}
