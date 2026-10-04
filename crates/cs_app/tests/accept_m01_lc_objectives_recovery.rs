//! M01-LC-OBJECTIVES: the objective producer reads every field of an original
//! `objectives.zrd` and names each one it cannot recover; it never drops one and
//! never builds an `Original` program from guessed semantics.

use std::path::PathBuf;

use cs_app::objectives::{
    ObjectiveRecovery, RecordFieldFamily, recover_retail_objectives,
    survey_retail_objective_records,
};
use cs_content::stunts::ZrdValue;

fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

fn block(fields: Vec<(&str, ZrdValue)>) -> ZrdValue {
    ZrdValue::List(
        fields
            .into_iter()
            .flat_map(|(key, value)| [text(key), value])
            .collect(),
    )
}

#[test]
fn accept_m01_lc_objectives_names_every_field_it_cannot_recover() {
    let record = block(vec![
        ("OBJECTIVE_DELAY", ZrdValue::Int(3)),
        (
            "OBJECTIVE1",
            block(vec![
                ("BEGIN_DORMANT", ZrdValue::Int(1)),
                ("KILL_OBJECTIVE_WHEN_I_COMPLETE", ZrdValue::Int(2)),
                ("SOME_NEW_KEY", ZrdValue::Int(0)),
            ]),
        ),
        ("OBJECTIVE2", block(vec![("INSTANTWIN", ZrdValue::Int(1))])),
    ]);
    let recovery = ObjectiveRecovery::read("zbd/test/m00", &ZrdValue::List(vec![record]));

    assert_eq!(recovery.blocks(), &[1, 2]);
    assert_eq!(recovery.fields_read(), 5);
    assert_eq!(recovery.fields_recovered(), 0);
    assert_eq!(recovery.unrecovered().len(), 5);
    let families = recovery.unrecovered_by_family();
    assert_eq!(families[&RecordFieldFamily::Dormancy], 1);
    assert_eq!(families[&RecordFieldFamily::CompletionEffect], 1);
    assert_eq!(families[&RecordFieldFamily::Outcome], 1);
    assert_eq!(families[&RecordFieldFamily::Unmeasured], 2);
    assert!(recovery.unrecovered().iter().any(|f| f.block.is_none()));

    let refusal = recovery.program().expect_err("nothing is recoverable");
    assert!(refusal.to_string().contains("CompletionEffect x1"));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_objectives_retail_m01_drops_no_record() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let recovery = recover_retail_objectives(&root, "zbd/c1c/m01").expect("M01 reads");

    // Independent denominator: the F39-D census's block count for the mission.
    let census = survey_retail_objective_records(&root).expect("census");
    let row = census.row("zbd/c1c/m01").expect("M01 census row");
    assert_eq!(recovery.blocks().len(), row.blocks as usize);
    assert_eq!(recovery.blocks().len(), 58);

    // Every field is accounted for, none silently dropped.
    assert_eq!(recovery.fields_read(), 356);
    assert_eq!(
        recovery.fields_recovered() + recovery.unrecovered().len(),
        recovery.fields_read()
    );
    assert_eq!(
        recovery.unrecovered_by_family().values().sum::<usize>(),
        recovery.fields_read()
    );
    // Block-level fields match the census's own per-key site totals.
    let census_sites: u32 = row.keys.iter().map(|(_, count)| count).sum();
    let block_fields = recovery
        .unrecovered()
        .iter()
        .filter(|f| f.block.is_some())
        .count();
    assert_eq!(block_fields, census_sites as usize);

    // Semantics are unmeasured, so no program is emitted.
    recovery
        .program()
        .expect_err("M01 semantics are unrecovered");
}
