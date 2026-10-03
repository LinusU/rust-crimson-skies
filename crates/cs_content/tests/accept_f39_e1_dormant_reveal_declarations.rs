//! F39-E1 acceptance: what an objective block's dormant/reveal declarations
//! are, and what this stage refuses to call.
//!
//! Tests the production reader
//! `cs_content::objectives::{measure_dormant_block, measure_dormant_declarations}`
//! over hand-authored `.zrd` documents, so every assertion is about what the
//! reader reads out of the fields it is given — never about a constant the
//! reader and the test share.
//!
//! Spec: `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`
//! non-negotiable behavior 5 ("show objectives only when the original reveal
//! rules allow"). Contract: `docs/contracts/SCRIPT-MISSION.md` ("for dynamic
//! native behavior not directly visible in text, isolate one controlled
//! condition and record the inference, contrary hypotheses and subsequent
//! verification").
//!
//! The retail half of the same measurement is
//! `crates/cs_app/tests/accept_f39_e1_retail_dormant_reveal.rs`.

use cs_content::objectives::{
    DORMANT_NO_ELAPSED_TIME, DormantReadError, DormantReading, InactiveCondition,
    MeasuredDormantBlock, MeasuredIdentity, OBJECTIVE_DORMANT_KEY, OBJECTIVE_IDENTITY_KEY,
    OBJECTIVE_INACTIVE_COUNT_KEY, measure_dormant_block, measure_dormant_declarations,
};
use cs_content::stunts::{ZrdValue, zrd_flat_fields};

fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

fn int(value: u32) -> ZrdValue {
    ZrdValue::Int(value)
}

fn float(value: f32) -> ZrdValue {
    ZrdValue::Float(value)
}

fn list(children: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(children)
}

/// A flat `.zrd` record of the given `key, value` fields, as the production
/// reader sees one.
fn record(entries: Vec<(String, ZrdValue)>) -> ZrdValue {
    let mut flat: Vec<ZrdValue> = Vec::new();
    for (key, value) in entries {
        flat.push(text(&key));
        flat.push(value);
    }
    list(flat)
}

/// Measures one block from a flat field list, through the production pair
/// reader.
fn measure(entries: Vec<(&str, ZrdValue)>) -> Result<MeasuredDormantBlock, DormantReadError> {
    measure_named("OBJECTIVE1", entries)
}

fn measure_named(
    block: &str,
    entries: Vec<(&str, ZrdValue)>,
) -> Result<MeasuredDormantBlock, DormantReadError> {
    let owned: Vec<(String, ZrdValue)> = entries
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect();
    let document = record(owned);
    let pairs = zrd_flat_fields(&document);
    measure_dormant_block(block, &pairs)
}

/// A dormant declaration as the installation spells it: a one-element list.
fn dormant_argument(value: f32) -> ZrdValue {
    list(vec![float(value)])
}

/// A stage declaration as the installation spells it: a list of texts.
fn condition(elements: &[&str]) -> ZrdValue {
    list(elements.iter().map(|value| text(value)).collect())
}

/// The measured sentinel and a positive elapsed-time argument.
#[test]
fn accept_f39_e1_the_dormant_argument_is_read_as_measured() {
    let sentinel = measure(vec![(OBJECTIVE_DORMANT_KEY, list(vec![float(-1.0)]))])
        .expect("the measured sentinel reads");
    assert_eq!(sentinel.dormant, Some(DormantReading::Sentinel));
    assert!(sentinel.dormant.is_some_and(DormantReading::is_sentinel));
    assert_eq!(
        sentinel.dormant.expect("read").argument(),
        DORMANT_NO_ELAPSED_TIME
    );
    assert!(sentinel.begins_dormant());

    // A positive argument, including the fractional one the installation ships,
    // reads as an elapsed-time quantity and keeps the value exactly.
    for argument in [1.0_f32, 13.5, 300.0] {
        let dated = measure(vec![(OBJECTIVE_DORMANT_KEY, list(vec![float(argument)]))])
            .expect("a positive argument reads");
        assert_eq!(dated.dormant, Some(DormantReading::ElapsedTime(argument)));
        assert_eq!(dated.dormant.expect("read").argument(), argument);
    }

    // An integer node reads as the argument the float spelling would carry.
    let as_int = measure(vec![(OBJECTIVE_DORMANT_KEY, list(vec![int(15)]))])
        .expect("an integer argument reads");
    assert_eq!(as_int.dormant, Some(DormantReading::ElapsedTime(15.0)));

    // A block that declares no dormant argument declares none.
    let plain = measure(vec![("COMPLETED_SOUND_GROUP", list(vec![text("snd_x")]))])
        .expect("a block without the key reads");
    assert_eq!(plain.dormant, None);
    assert!(!plain.begins_dormant());
}

/// Every measured condition arity reads, and nothing is invented for it.
#[test]
fn accept_f39_e1_a_condition_keeps_its_measured_elements() {
    let block = measure(vec![
        ("INACTIVE1", list(vec![text("pickup_objective")])),
        ("INACTIVE2", list(vec![text("fuel_truck01"), text("tank")])),
        (
            "INACTIVE3",
            list(vec![text("geminizep"), text("reng11"), text("healthy")]),
        ),
    ])
    .expect("the three measured arities read");

    assert_eq!(block.condition_count(), 3);
    assert_eq!(
        block.conditions,
        vec![
            InactiveCondition {
                stage: 1,
                subject: "pickup_objective".to_owned(),
                part: None,
                attribute: None,
                arity: 1,
            },
            InactiveCondition {
                stage: 2,
                subject: "fuel_truck01".to_owned(),
                part: Some("tank".to_owned()),
                attribute: None,
                arity: 2,
            },
            InactiveCondition {
                stage: 3,
                subject: "geminizep".to_owned(),
                part: Some("reng11".to_owned()),
                attribute: Some("healthy".to_owned()),
                arity: 3,
            },
        ]
    );
    assert_eq!(
        block.condition_signature(),
        vec![
            ("pickup_objective".to_owned(), None, None),
            ("fuel_truck01".to_owned(), Some("tank".to_owned()), None),
            (
                "geminizep".to_owned(),
                Some("reng11".to_owned()),
                Some("healthy".to_owned())
            ),
        ]
    );
}

/// The completion count is read beside the conditions it thresholds, and the
/// degenerate shapes are reported questions rather than defaults.
#[test]
fn accept_f39_e1_a_completion_count_is_read_beside_its_conditions() {
    let ladder = measure(vec![
        (OBJECTIVE_INACTIVE_COUNT_KEY, list(vec![int(2)])),
        ("INACTIVE1", condition(&["geminizep", "reng11", "healthy"])),
        ("INACTIVE2", condition(&["geminizep", "reng12", "healthy"])),
    ])
    .expect("a count beside conditions reads");
    assert_eq!(ladder.completion_count, Some(2));
    assert_eq!(ladder.condition_count(), 2);
    assert!(!ladder.count_exceeds_conditions());
    assert!(!ladder.count_without_conditions());

    // A count with no condition is reported, never silently defaulted to zero.
    let orphan = measure(vec![(OBJECTIVE_INACTIVE_COUNT_KEY, list(vec![int(2)]))])
        .expect("a count without conditions reads");
    assert_eq!(orphan.completion_count, Some(2));
    assert!(orphan.count_without_conditions());
    // Both facts hold at once for the one shape the installation ships: a count
    // over no condition is also a count no condition list can satisfy.
    assert!(orphan.count_exceeds_conditions());

    // A count larger than the condition list is the shape a lowering must
    // refuse, and the reader says so instead of clamping it.
    let unsatisfiable = measure(vec![
        (OBJECTIVE_INACTIVE_COUNT_KEY, list(vec![int(5)])),
        ("INACTIVE1", list(vec![text("geminizep"), text("reng11")])),
    ])
    .expect("an unsatisfiable count still reads");
    assert!(unsatisfiable.count_exceeds_conditions());

    // Conditions without a count carry no threshold of their own.
    let uncounted = measure(vec![("INACTIVE1", list(vec![text("geminizep")]))])
        .expect("conditions without a count read");
    assert_eq!(uncounted.completion_count, None);
    assert!(!uncounted.count_without_conditions());
    assert!(!uncounted.count_exceeds_conditions());
}

/// The display identity reads as role, ordinal and message — and nothing about
/// when the objective may be shown.
#[test]
fn accept_f39_e1_an_identity_reads_as_role_ordinal_and_message() {
    let with_message = measure(vec![
        (OBJECTIVE_DORMANT_KEY, list(vec![float(-1.0)])),
        (
            OBJECTIVE_IDENTITY_KEY,
            list(vec![text("PRIMARY"), int(2), text("MSG_BRF_HWM4_OBJ2")]),
        ),
    ])
    .expect("an identity with a message reads");
    assert_eq!(
        with_message.identities,
        vec![MeasuredIdentity {
            role: "PRIMARY".to_owned(),
            ordinal: 2,
            message: Some("MSG_BRF_HWM4_OBJ2".to_owned()),
        }]
    );
    // A dormant block may still be the one whose text the player is shown, and
    // the reader keeps the two facts apart rather than collapsing them.
    assert!(with_message.begins_dormant());

    let without_message = measure(vec![(
        OBJECTIVE_IDENTITY_KEY,
        list(vec![text("SECONDARY"), int(11)]),
    )])
    .expect("an identity without a message reads");
    assert_eq!(
        without_message.identities,
        vec![MeasuredIdentity {
            role: "SECONDARY".to_owned(),
            ordinal: 11,
            message: None,
        }]
    );

    // A block that declares two identities keeps both: the installation has
    // exactly one such block, and which one the original honours is unmeasured,
    // so dropping either would be a silent choice.
    let twice = measure(vec![
        (
            OBJECTIVE_IDENTITY_KEY,
            list(vec![text("PRIMARY"), int(3), text("MSG_BRF_RMM5_OBJ3")]),
        ),
        (
            OBJECTIVE_IDENTITY_KEY,
            list(vec![text("SECONDARY"), int(11)]),
        ),
    ])
    .expect("a block with two identities reads");
    assert_eq!(twice.identities.len(), 2);
    assert_eq!(twice.identities[0].role, "PRIMARY");
    assert_eq!(
        twice.identities[0].message.as_deref(),
        Some("MSG_BRF_RMM5_OBJ3")
    );
    assert_eq!(twice.identities[1].role, "SECONDARY");
    assert_eq!(twice.identities[1].message, None);
}

/// Only `INACTIVE` plus digits is a stage: the count key, a word, an empty
/// number and a zero index are not stages, and none becomes a condition.
#[test]
fn accept_f39_e1_only_inactive_with_digits_is_a_stage() {
    let block = measure(vec![
        (OBJECTIVE_INACTIVE_COUNT_KEY, list(vec![int(1)])),
        ("INACTIVE1", list(vec![text("geminizep")])),
        ("INACTIVATED", list(vec![text("geminizep")])),
        ("INACTIVE_A", list(vec![text("geminizep")])),
        ("INACTIVE", list(vec![text("geminizep")])),
    ])
    .expect("the non-stage spellings are ignored");
    assert_eq!(block.condition_count(), 1);
    assert_eq!(block.completion_count, Some(1));
    assert_eq!(block.conditions[0].stage, 1);
}

/// Stage numbering is read as declared, and a shape this stage never measured
/// is refused by name instead of being sorted over.
#[test]
fn accept_f39_e1_stage_numbering_is_either_measured_or_refused() {
    // Out of order but complete: the reader sorts the stages by number.
    let out_of_order = measure(vec![
        ("INACTIVE2", list(vec![text("a")])),
        ("INACTIVE1", list(vec![text("b")])),
    ])
    .expect("a complete set in any order reads");
    assert_eq!(
        out_of_order
            .conditions
            .iter()
            .map(|condition| condition.stage)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(
        out_of_order
            .conditions
            .iter()
            .map(|condition| condition.subject.as_str())
            .collect::<Vec<_>>(),
        vec!["b", "a"]
    );

    // `INACTIVE0` is a stage-shaped key with an unusable number, so it is
    // refused with the rest rather than dropped and read as a shorter ladder.
    for declared in [vec![1_u32, 3], vec![2], vec![1, 1], vec![0], vec![0, 1]] {
        let entries: Vec<(String, ZrdValue)> = declared
            .iter()
            .map(|stage| (format!("INACTIVE{stage}"), list(vec![text("geminizep")])))
            .collect();
        let document = record(entries);
        let pairs = zrd_flat_fields(&document);
        assert_eq!(
            measure_dormant_block("OBJECTIVE9", &pairs).unwrap_err(),
            DormantReadError::StageNumbering {
                block: "OBJECTIVE9".to_owned(),
                declared,
            },
            "a stage numbering this stage never measured must be refused"
        );
    }
}

/// Every unmeasured declaration shape is refused by name, and each refusal says
/// which field it is about.
#[test]
fn accept_f39_e1_every_unmeasured_shape_is_refused_by_name() {
    assert_eq!(
        measure(vec![(OBJECTIVE_DORMANT_KEY, list(vec![]))]).unwrap_err(),
        DormantReadError::DormantArgument {
            block: "OBJECTIVE1".to_owned(),
            arity: 0,
        }
    );
    assert_eq!(
        measure(vec![(
            OBJECTIVE_DORMANT_KEY,
            list(vec![float(1.0), float(2.0)])
        )])
        .unwrap_err(),
        DormantReadError::DormantArgument {
            block: "OBJECTIVE1".to_owned(),
            arity: 2,
        }
    );
    assert_eq!(
        measure(vec![(OBJECTIVE_DORMANT_KEY, list(vec![text("-1")]))]).unwrap_err(),
        DormantReadError::DormantArgument {
            block: "OBJECTIVE1".to_owned(),
            arity: 1,
        }
    );
    assert!(matches!(
        measure(vec![(OBJECTIVE_DORMANT_KEY, list(vec![float(f32::NAN)]))]).unwrap_err(),
        DormantReadError::NonFiniteDormant { block, value } if block == "OBJECTIVE1" && value.is_nan()
    ));
    // A negative argument other than the measured sentinel is not read as a
    // duration in the other direction either, and neither is a zero.
    for value in [-2.0_f32, 0.0] {
        assert_eq!(
            measure(vec![(OBJECTIVE_DORMANT_KEY, list(vec![float(value)]))]).unwrap_err(),
            DormantReadError::UnmeasuredDormantArgument {
                block: "OBJECTIVE1".to_owned(),
                value,
            }
        );
    }
    assert_eq!(
        measure(vec![(OBJECTIVE_INACTIVE_COUNT_KEY, list(vec![float(4.0)]))]).unwrap_err(),
        DormantReadError::CompletionCount {
            block: "OBJECTIVE1".to_owned(),
            arity: 1,
        }
    );
    assert_eq!(
        measure(vec![("INACTIVE1", list(vec![]))]).unwrap_err(),
        DormantReadError::ConditionShape {
            block: "OBJECTIVE1".to_owned(),
            stage: 1,
            arity: 0,
        }
    );
    assert_eq!(
        measure(vec![(
            "INACTIVE1",
            list(vec![text("a"), text("b"), text("c"), text("d")])
        )])
        .unwrap_err(),
        DormantReadError::ConditionShape {
            block: "OBJECTIVE1".to_owned(),
            stage: 1,
            arity: 4,
        }
    );
    assert_eq!(
        measure(vec![("INACTIVE1", list(vec![text(""), text("b")]))]).unwrap_err(),
        DormantReadError::ConditionShape {
            block: "OBJECTIVE1".to_owned(),
            stage: 1,
            arity: 2,
        }
    );
    assert_eq!(
        measure(vec![("INACTIVE1", list(vec![int(7), text("b")]))]).unwrap_err(),
        DormantReadError::NonTextConditionElement {
            block: "OBJECTIVE1".to_owned(),
            stage: 1,
            index: 0,
        }
    );
    assert_eq!(
        measure(vec![(OBJECTIVE_IDENTITY_KEY, list(vec![text("PRIMARY")]))]).unwrap_err(),
        DormantReadError::IdentityShape {
            block: "OBJECTIVE1".to_owned(),
            arity: 1,
        }
    );
    assert_eq!(
        measure(vec![(
            OBJECTIVE_IDENTITY_KEY,
            list(vec![text("PRIMARY"), text("1")])
        )])
        .unwrap_err(),
        DormantReadError::IdentityShape {
            block: "OBJECTIVE1".to_owned(),
            arity: 2,
        }
    );
}

/// A whole objective record is measured block by block, in declaration order,
/// and a record with one unmeasurable block measures nothing at all.
#[test]
fn accept_f39_e1_a_whole_record_is_measured_block_by_block() {
    let block_one = list(vec![
        text(OBJECTIVE_DORMANT_KEY),
        dormant_argument(-1.0),
        text(OBJECTIVE_IDENTITY_KEY),
        list(vec![text("PRIMARY"), int(1)]),
    ]);
    let block_two = list(vec![
        text(OBJECTIVE_INACTIVE_COUNT_KEY),
        list(vec![int(1)]),
        text("INACTIVE1"),
        condition(&["geminizep"]),
    ]);
    let document = list(vec![list(vec![
        text("OBJECTIVE1"),
        block_one,
        text("OBJECTIVE2"),
        block_two,
        text("MISSIONTYPE"),
        int(3),
    ])]);
    let blocks = measure_dormant_declarations(&document).expect("the record measures");
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].block, "OBJECTIVE1");
    assert!(blocks[0].begins_dormant());
    assert_eq!(blocks[0].identities[0].ordinal, 1);
    assert_eq!(blocks[1].block, "OBJECTIVE2");
    assert!(!blocks[1].begins_dormant());
    assert_eq!(blocks[1].completion_count, Some(1));
    assert_eq!(blocks[1].condition_count(), 1);

    // A record holding one unmeasurable block measures nothing at all.
    let broken = list(vec![list(vec![
        text("OBJECTIVE1"),
        list(vec![text(OBJECTIVE_DORMANT_KEY), list(vec![text("soon")])]),
    ])]);
    assert_eq!(
        measure_dormant_declarations(&broken).unwrap_err(),
        DormantReadError::DormantArgument {
            block: "OBJECTIVE1".to_owned(),
            arity: 1,
        }
    );
}

/// Keys this stage does not measure are ignored, so a block may declare the
/// rest of the original vocabulary without the reader inventing a field for it —
/// and the fields it *does* measure are still read.
#[test]
fn accept_f39_e1_unmeasured_keys_are_ignored_and_named_fields_still_read() {
    let block = measure_named(
        "OBJECTIVE77",
        vec![
            (OBJECTIVE_DORMANT_KEY, list(vec![float(-1.0)])),
            (
                "NAP_OBJECTIVE_WHEN_I_COMPLETE",
                list(vec![int(24), float(15.0)]),
            ),
            (
                "ANIM_STATE",
                list(vec![text("ANIM"), list(vec![text("STATE")])]),
            ),
            (OBJECTIVE_IDENTITY_KEY, list(vec![text("PRIMARY"), int(3)])),
        ],
    )
    .expect("the block measures");
    assert_eq!(block.block, "OBJECTIVE77");
    assert_eq!(block.dormant, Some(DormantReading::Sentinel));
    assert_eq!(block.identities[0].ordinal, 3);
    assert_eq!(block.completion_count, None);
    assert!(block.conditions.is_empty());
}
