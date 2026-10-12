//! M01-B-OBJ (#1355): the integration verdict of the four objective-semantics
//! stages over one record.
//!
//! M01-B-OBJ-1/2/3/4 measured every field family M01's `objectives.zrd` uses:
//! the eight families the engine's loader reads are recovered into the engine
//! model (308 of 358 fields), and the two mission-language directive families
//! are measured as world-side operations the declared program does not
//! represent (the remaining 50). No field M01 spells is left in the
//! `Unmeasured` family. `ObjectiveRecovery::program()` therefore refuses
//! **naming** the directive families and nothing else — a refusal computed
//! from the record's census, never the blanket one, and never a program built
//! from guessed or dropped semantics (AGENTS.md rules 4 and 5).
//!
//! Evidence: `docs/findings/2026-10-12-m01-b-obj-1..4-*.md` hold the
//! per-family measurements; the integration finding
//! `docs/findings/2026-10-12-m01-b-obj-integration-family-verdict.md` joins
//! them into this verdict.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cs_app::objectives::{ObjectiveRecovery, RecordFieldFamily, recover_retail_objectives};
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

/// A record whose every field is a measured, well-formed engine field, so the
/// census recovers all of them and none is left unrecovered.
#[test]
fn accept_m01_b_obj_integration_fully_recovered_record_refuses_by_the_schema_gap() {
    let record = ZrdValue::List(vec![block(vec![
        (
            "OBJECTIVE1",
            block(vec![
                ("WAKE_OBJECTIVE_WHEN_I_COMPLETE", ints(&[2])),
                ("BEGIN_DORMANT", ZrdValue::Int(2)),
            ]),
        ),
        (
            "OBJECTIVE2",
            block(vec![("INSTANTWIN", ZrdValue::List(vec![]))]),
        ),
    ])]);
    let recovery = ObjectiveRecovery::read("zbd/test/m00", &record);

    // Every field read is recovered; the family maps partition the record.
    assert_eq!(recovery.fields_read(), 3);
    assert_eq!(recovery.fields_recovered(), 3);
    assert!(recovery.unrecovered().is_empty());
    assert!(recovery.unrecovered_by_family().is_empty());
    let recovered = recovery.recovered_by_family();
    assert_eq!(recovered[&RecordFieldFamily::CompletionEffect], 1);
    assert_eq!(recovered[&RecordFieldFamily::Dormancy], 1);
    assert_eq!(recovered[&RecordFieldFamily::Outcome], 1);
    assert_eq!(
        recovered.values().sum::<usize>(),
        recovery.fields_read(),
        "the recovered map accounts for every field"
    );

    // And still no declared program: with no field gap left, the refusal must
    // name the schema gap instead of pretending a family is unrecovered.
    let refusal = recovery
        .program()
        .expect_err("the declared schema has no measured carrier for the engine model");
    assert!(refusal.families().is_empty());
    let message = refusal.to_string();
    assert!(
        message.contains("no measured carrier"),
        "the refusal must name the schema gap: {message}"
    );
    for family in [
        "CompletionEffect",
        "Dormancy",
        "Outcome",
        "Unmeasured",
        "ControlDirective",
    ] {
        assert!(
            !message.contains(family),
            "no family may be blamed when none is open: {message}"
        );
    }
}

/// The inverse guard: a key no measured stage covers still refuses **by name**
/// (AGENTS.md rule 4), so the computed refusal can never silently pass a
/// record with unknown semantics.
#[test]
fn accept_m01_b_obj_integration_unmeasured_key_is_still_refused_by_name() {
    let record = ZrdValue::List(vec![block(vec![
        (
            "OBJECTIVE1",
            block(vec![
                ("WAKE_OBJECTIVE_WHEN_I_COMPLETE", ints(&[2])),
                ("SOME_FUTURE_KEY", ZrdValue::Int(0)),
            ]),
        ),
        (
            "OBJECTIVE2",
            block(vec![("INSTANTWIN", ZrdValue::List(vec![]))]),
        ),
    ])]);
    let recovery = ObjectiveRecovery::read("zbd/test/m00", &record);

    assert_eq!(recovery.fields_read(), 3);
    assert_eq!(recovery.fields_recovered(), 2);
    let families = recovery.unrecovered_by_family();
    assert_eq!(families[&RecordFieldFamily::Unmeasured], 1);
    let refusal = recovery
        .program()
        .expect_err("an unmeasured key keeps the refusal");
    let message = refusal.to_string();
    assert!(message.contains("Unmeasured x1"), "{message}");
    assert!(!message.contains("no measured carrier"), "{message}");
}

/// The retail integration verdict for M01: every family it uses is measured
/// (nothing is left `Unmeasured`), the eight engine-read families are fully
/// recovered at the four stages' census, and `program()` refuses naming
/// exactly the two directive families — measured, world-side, never lowered.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_b_obj_integration_retail_m01_every_family_is_measured_and_the_directives_are_named() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let recovery = recover_retail_objectives(&root, "zbd/c1c/m01").expect("M01 reads");

    // The record itself: 58 blocks, 358 fields, none dropped.
    assert_eq!(recovery.blocks().len(), 58);
    assert_eq!(recovery.fields_read(), 358);
    assert_eq!(
        recovery.fields_recovered() + recovery.unrecovered().len(),
        recovery.fields_read()
    );

    // The eight engine-read families, recovered at the measured census of
    // M01-B-OBJ-1 (56 + 3), M01-B-OBJ-2 (52 + 9 + 5 + 41), M01-B-OBJ-3
    // (140 + 2).
    let mut expected = BTreeMap::new();
    expected.insert(RecordFieldFamily::CompletionEffect, 56);
    expected.insert(RecordFieldFamily::OrderDependency, 3);
    expected.insert(RecordFieldFamily::Dormancy, 52);
    expected.insert(RecordFieldFamily::CompletionCount, 9);
    expected.insert(RecordFieldFamily::Identity, 5);
    expected.insert(RecordFieldFamily::SoundCue, 41);
    expected.insert(RecordFieldFamily::InactiveStage, 140);
    expected.insert(RecordFieldFamily::Outcome, 2);
    let recovered = recovery.recovered_by_family();
    assert_eq!(
        recovered, expected,
        "the recovered census must match the four stages' findings"
    );
    assert_eq!(recovered.values().sum::<usize>(), 308);

    // Nothing M01 spells is unmeasured; the unrecovered remainder is exactly
    // the directive families of M01-B-OBJ-4: 45 block-level sites and the 5
    // record-level fields.
    let families = recovery.unrecovered_by_family();
    assert!(
        !families.contains_key(&RecordFieldFamily::Unmeasured),
        "every family M01 uses is measured"
    );
    let mut expected_unrecovered = BTreeMap::new();
    expected_unrecovered.insert(RecordFieldFamily::ControlDirective, 45);
    expected_unrecovered.insert(RecordFieldFamily::RecordDirective, 5);
    assert_eq!(families, expected_unrecovered);
    assert_eq!(families.values().sum::<usize>(), 50);

    // The refusal is computed from that census: it names the two directive
    // families and blames no family that is fully recovered.
    let refusal = recovery
        .program()
        .expect_err("the directives are world-side operations, not declared fields");
    assert_eq!(refusal.families(), &expected_unrecovered);
    let message = refusal.to_string();
    assert!(message.contains("ControlDirective x45"), "{message}");
    assert!(message.contains("RecordDirective x5"), "{message}");
    for family in [
        "CompletionEffect",
        "OrderDependency",
        "Dormancy",
        "CompletionCount",
        "Identity",
        "SoundCue",
        "InactiveStage",
        "Outcome",
        "Unmeasured",
    ] {
        assert!(
            !message.contains(&format!("{family} x")),
            "a recovered family must not be named: {message}"
        );
    }
}
