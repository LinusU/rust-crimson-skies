//! M01-B-OBJ-4: the 19 mission-language directive keys M01 spells are each
//! measured — the five record-level keys (`MISSION_TIMER`, `PLAYER_INIT` and
//! the three animation lists) by their parser writes in the engine image, the
//! fourteen block-level keys by the M01-LC-DIRECTIVE findings — and
//! `ObjectiveRecovery` names each a `RecordDirective` or a `ControlDirective`
//! rather than a generic `Unmeasured`. What the directives *do* is world-side
//! state the declared program does not represent, so the program still
//! refuses — naming the families, never guessing them.

use std::path::PathBuf;

use cs_app::objectives::{ObjectiveRecovery, RecordFieldFamily, recover_retail_objectives};
use cs_content::mission_control::{DirectiveOperation, measured_directive};
use cs_content::objectives::{
    AnimDirective, MeasuredRecordFields, MissionTimerField, RECORD_FIELD_KEY_VOCABULARY,
    RecordFieldError, RecordScalar,
};
use cs_content::stunts::ZrdValue;

fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

fn int(value: u32) -> ZrdValue {
    ZrdValue::Int(value)
}

fn real(value: f32) -> ZrdValue {
    ZrdValue::Float(value)
}

fn list(values: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(values)
}

fn block(fields: Vec<(&str, ZrdValue)>) -> ZrdValue {
    ZrdValue::List(
        fields
            .into_iter()
            .flat_map(|(key, value)| [text(key), value])
            .collect(),
    )
}

/// M01's own spellings for the five record-level keys.
fn m01_record_fields() -> Vec<(&'static str, ZrdValue)> {
    vec![
        ("MISSION_TIMER", list(vec![real(0.0)])),
        (
            "PLAYER_INIT",
            list(vec![
                int(1),
                list(vec![real(-3694.0), real(1318.0), real(-12482.0)]),
                list(vec![real(0.0), real(170.0), real(0.0)]),
                real(0.8),
                real(580.0),
            ]),
        ),
        ("RESTORE_ANIMS", list(vec![])),
        ("EXECUTE_ANIMS", list(vec![])),
        ("INVALIDATE_ANIMS", list(vec![])),
    ]
}

/// The fourteen block-level keys of the task, each paired with an argument
/// spelled exactly as one of its M01 sites spells it, and the
/// `DirectiveOperation` the measured table returns.
fn m01_block_directives() -> Vec<(&'static str, ZrdValue, DirectiveOperation)> {
    vec![
        // M01 block 2.
        (
            "ADD_OBJECTIVE_TARGET",
            list(vec![text("workersvoyagezep")]),
            DirectiveOperation::SetTargetFlag {
                objective: true,
                set: true,
            },
        ),
        // M01 block 11: a nested name chain.
        (
            "REMOVE_OBJECTIVE_TARGET",
            list(vec![list(vec![text("wv_tailhook"), text("peoplehook")])]),
            DirectiveOperation::SetTargetFlag {
                objective: true,
                set: false,
            },
        ),
        // M01 block 2.
        (
            "ADD_OTHER_TARGET",
            list(vec![list(vec![text("piratezep"), text("rock_zeppelin")])]),
            DirectiveOperation::SetTargetFlag {
                objective: false,
                set: true,
            },
        ),
        // M01 block 11.
        (
            "ANIM_STATE",
            list(vec![
                text("ANIM"),
                list(vec![
                    text("NAME"),
                    list(vec![text("wv_drop_copilot")]),
                    text("STATE"),
                    list(vec![text("RUNNING")]),
                ]),
            ]),
            DirectiveOperation::AnimationStates,
        ),
        // M01 block 13.
        (
            "COMPLETED_STOPPOINT",
            list(vec![list(vec![text("SwanZep1"), int(1), int(0)])]),
            DirectiveOperation::AdvanceStopPoint,
        ),
        // M01 block 2.
        (
            "DEDG",
            list(vec![int(1), int(0)]),
            DirectiveOperation::EnemyGroupDepletion,
        ),
        // M01 block 2: one field whose argument carries two pairs.
        (
            "SET_AI_NET",
            list(vec![
                list(vec![text("devastator_2"), text("Bravo2")]),
                list(vec![text("devastator_3"), text("Charlie2")]),
            ]),
            DirectiveOperation::AssignNet,
        ),
        // M01 block 2.
        (
            "SET_HELP_LABEL",
            list(vec![
                list(vec![text("piratezep"), text("rock_zeppelin")]),
                text(" "),
            ]),
            DirectiveOperation::SetHelpLabel,
        ),
        // M01 block 11.
        (
            "STOP_QUEUED_SOUNDS",
            list(vec![text("snd_c2-NW-m1_Jack_21")]),
            DirectiveOperation::StopQueuedSounds,
        ),
        // M01 block 3.
        (
            "TRAVELERS",
            list(vec![
                text("player"),
                text("APPROACHING"),
                text("workersvoyagezep"),
                real(700.0),
                int(1),
            ]),
            DirectiveOperation::Travelers,
        ),
        // M01 block 13.
        (
            "WAKEUP_ENEMIES",
            list(vec![text("bsfury_1"), text("blackswanzep")]),
            DirectiveOperation::WakeEnemies,
        ),
        // M01 block 9.
        (
            "WAKEUP_GENERATOR",
            list(vec![text("workersvoyagezep"), int(3)]),
            DirectiveOperation::FeedGenerator,
        ),
        // M01 block 1.
        (
            "WAKEUP_ZEP_TURRETS",
            list(vec![text("piratezep")]),
            DirectiveOperation::WakeZeppelinTurrets,
        ),
        // M01 block 11.
        (
            "WAKE_ANIM",
            list(vec![text("activate_dropoff_node")]),
            DirectiveOperation::WakeAnimation,
        ),
    ]
}

#[test]
fn accept_m01_b_obj_4_record_fields_parse_the_measured_grammar() {
    let record = block(
        [
            m01_record_fields(),
            vec![("OBJECTIVE1", block(vec![("BEGIN_DORMANT", int(1))]))],
        ]
        .concat(),
    );
    let recovery = ObjectiveRecovery::read("zbd/test/m01", &list(vec![record]));

    let fields = recovery
        .record_fields()
        .as_ref()
        .expect("M01's own record fields are inside the measured shapes");
    assert_eq!(
        fields.mission_timer,
        Some(MissionTimerField {
            seconds: 0.0,
            no_loss: false
        })
    );
    let init = fields.player_init.expect("PLAYER_INIT parses");
    assert_eq!(init.head, RecordScalar::Int(1));
    assert_eq!(
        init.position,
        [
            RecordScalar::Real(-3694.0),
            RecordScalar::Real(1318.0),
            RecordScalar::Real(-12482.0),
        ]
    );
    assert_eq!(init.attitude_degrees, [0.0, 170.0, 0.0]);
    assert_eq!(init.tail, RecordScalar::Real(0.8));
    assert_eq!(init.scaled_input, 580.0);
    for list in [
        &fields.restore_anims,
        &fields.execute_anims,
        &fields.invalidate_anims,
    ] {
        assert!(list.directives.is_empty());
        assert_eq!(list.skipped, 0);
    }

    // The five keys are record directives at the top level, never Unmeasured.
    for key in RECORD_FIELD_KEY_VOCABULARY {
        let field = recovery
            .unrecovered()
            .iter()
            .find(|field| field.key == key)
            .unwrap_or_else(|| panic!("{key} is read"));
        assert_eq!(field.family, RecordFieldFamily::RecordDirective, "{key}");
        assert!(field.block.is_none(), "{key} sits at the record level");
    }
    recovery
        .program()
        .expect_err("measured directives are still not a declared program");
}

#[test]
fn accept_m01_b_obj_4_record_field_shapes_outside_the_measured_grammar_refuse() {
    let read = |key: &str, value: ZrdValue| {
        let record = block(vec![(key, value)]);
        ObjectiveRecovery::read("zbd/test/m01", &list(vec![record]))
            .record_fields()
            .clone()
    };

    // `MISSION_TIMER`: element 0 must be a number the engine's real path
    // reads; element 1 is compared to "NOLOSS" as a char *.
    assert!(matches!(
        read("MISSION_TIMER", list(vec![text("soon")])),
        Err(RecordFieldError::MissionTimerShape { .. })
    ));
    assert!(matches!(
        read("MISSION_TIMER", list(vec![])),
        Err(RecordFieldError::MissionTimerShape { .. })
    ));
    assert!(matches!(
        read("MISSION_TIMER", list(vec![real(30.0), int(1)])),
        Err(RecordFieldError::MissionTimerShape { .. })
    ));
    assert_eq!(
        read("MISSION_TIMER", list(vec![real(30.0), text("NOLOSS")])),
        Ok(MeasuredRecordFields {
            mission_timer: Some(MissionTimerField {
                seconds: 30.0,
                no_loss: true
            }),
            ..Default::default()
        })
    );
    // An integer element 0 converts rather than erroring — signed like the
    // engine's `fild dword`, so a payload above `i32::MAX` reads negative —
    // and any other second element is silently ignored by the byte compare.
    assert_eq!(
        read("MISSION_TIMER", list(vec![int(30), text("anything-else")])),
        Ok(MeasuredRecordFields {
            mission_timer: Some(MissionTimerField {
                seconds: 30.0,
                no_loss: false
            }),
            ..Default::default()
        })
    );
    assert_eq!(
        read("MISSION_TIMER", list(vec![int(0xffff_ffff)])),
        Ok(MeasuredRecordFields {
            mission_timer: Some(MissionTimerField {
                seconds: -1.0,
                no_loss: false
            }),
            ..Default::default()
        })
    );

    // `PLAYER_INIT`: the parser reads five elements unconditionally.
    assert!(matches!(
        read("PLAYER_INIT", list(vec![int(1), int(2), int(3)])),
        Err(RecordFieldError::PlayerInitShape { .. })
    ));
    assert!(matches!(
        read(
            "PLAYER_INIT",
            list(vec![
                int(1),
                text("x"),
                list(vec![]),
                real(0.8),
                real(580.0)
            ])
        ),
        Err(RecordFieldError::PlayerInitShape { .. })
    ));

    // The animation lists keep the two measured element forms and skip what
    // the parser's shape rules skip — a skip is never an error.
    let fields = read(
        "EXECUTE_ANIMS",
        list(vec![
            text("hover"),
            list(vec![text("zep"), text("turret#"), text("fire")]),
            int(3),
            list(vec![text("a"), text("b")]),
            list(vec![text("a"), text("no-hash"), text("c")]),
        ]),
    )
    .expect("the list grammar keeps and skips, never refuses");
    assert_eq!(
        fields.execute_anims.directives,
        [
            AnimDirective::Named("hover".to_owned()),
            AnimDirective::Scoped {
                object: "zep".to_owned(),
                member: "turret".to_owned(),
                animation: "fire".to_owned(),
            },
        ]
    );
    assert_eq!(fields.execute_anims.skipped, 3);

    // The follower's element vector is read unconditionally, so a non-list
    // follower is off the measured shape — a wild read in the original, not
    // an empty list.
    for key in ["RESTORE_ANIMS", "EXECUTE_ANIMS", "INVALIDATE_ANIMS"] {
        assert!(
            matches!(
                read(key, text("hover")),
                Err(RecordFieldError::AnimListShape { key: seen, .. }) if seen == key
            ),
            "{key} with a non-list follower"
        );
        assert!(
            matches!(
                read(key, int(1)),
                Err(RecordFieldError::AnimListShape { key: seen, .. }) if seen == key
            ),
            "{key} with a scalar follower"
        );
    }

    // An absent key still leaves the (empty) list the parser allocates.
    let recovery = ObjectiveRecovery::read(
        "zbd/test/m01",
        &list(vec![block(vec![("OBJECTIVE1", block(vec![]))])]),
    );
    let fields = recovery
        .record_fields()
        .as_ref()
        .expect("absent keys parse");
    assert!(fields.mission_timer.is_none());
    assert!(fields.player_init.is_none());
    assert!(fields.execute_anims.directives.is_empty());
}

#[test]
fn accept_m01_b_obj_4_block_directives_classify_as_control_directives() {
    let directives = m01_block_directives();
    let fields: Vec<(&'static str, ZrdValue)> = directives
        .iter()
        .map(|(key, argument, _)| (*key, argument.clone()))
        .collect();
    let mut top = m01_record_fields();
    top.push(("OBJECTIVE1", block(fields)));
    let recovery = ObjectiveRecovery::read("zbd/test/m01", &list(vec![block(top)]));

    // Every task key reaches its measured family and its measured operation —
    // none is reported as a generic Unmeasured.
    for (key, _, operation) in &directives {
        let field = recovery
            .unrecovered()
            .iter()
            .find(|field| field.key == *key)
            .unwrap_or_else(|| panic!("{key} is read"));
        assert_eq!(field.family, RecordFieldFamily::ControlDirective, "{key}");
        assert_eq!(field.block, Some(1), "{key} sits inside a block");
        assert_eq!(
            measured_directive(key).map(|directive| directive.operation),
            Some(*operation),
            "{key} keeps its measured operation"
        );
    }

    // The refusal names the families it keeps unrecovered.
    let refusal = recovery.program().expect_err("unrecovered families");
    let text = refusal.to_string();
    assert!(text.contains("ControlDirective x14"), "{text}");
    assert!(text.contains("RecordDirective x5"), "{text}");
    assert!(!text.contains("Unmeasured"), "{text}");
}

#[test]
fn accept_m01_b_obj_4_unmeasured_keys_stay_unmeasured() {
    // A key no measured stage covers — inside a block or at the record's top
    // level — still classifies `Unmeasured`; the new families only claim the
    // measured vocabulary.
    let record = block(vec![
        ("SOME_NEW_KEY", int(0)),
        ("OBJECTIVE1", block(vec![("ANOTHER_NEW_KEY", int(0))])),
    ]);
    let recovery = ObjectiveRecovery::read("zbd/test/m01", &list(vec![record]));
    assert_eq!(
        recovery.unrecovered_by_family()[&RecordFieldFamily::Unmeasured],
        2
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_b_obj_4_retail_m01_accounts_for_every_spelled_directive() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let recovery = recover_retail_objectives(&root, "zbd/c1c/m01").expect("M01 reads");

    // M01's record preamble parses exactly as spelled.
    let fields = recovery
        .record_fields()
        .as_ref()
        .expect("M01's record fields are inside the measured shapes");
    let init = fields.player_init.expect("M01 spells PLAYER_INIT");
    assert_eq!(init.head, RecordScalar::Int(1));
    assert_eq!(
        init.position,
        [
            RecordScalar::Real(-3694.0),
            RecordScalar::Real(1318.0),
            RecordScalar::Real(-12482.0),
        ]
    );
    assert_eq!(init.attitude_degrees, [0.0, 170.0, 0.0]);
    assert_eq!(init.tail, RecordScalar::Real(0.8));
    assert_eq!(init.scaled_input, 580.0);
    assert_eq!(
        fields.mission_timer,
        Some(MissionTimerField {
            seconds: 0.0,
            no_loss: false
        })
    );
    for list in [
        &fields.restore_anims,
        &fields.execute_anims,
        &fields.invalidate_anims,
    ] {
        assert!(list.directives.is_empty());
        assert_eq!(list.skipped, 0);
    }

    // The 19 keys, with the site counts the production census reads in M01 —
    // five record-level fields plus 45 block-level fields, 50 in all.
    let expected: &[(&str, usize, RecordFieldFamily)] = &[
        ("MISSION_TIMER", 1, RecordFieldFamily::RecordDirective),
        ("PLAYER_INIT", 1, RecordFieldFamily::RecordDirective),
        ("RESTORE_ANIMS", 1, RecordFieldFamily::RecordDirective),
        ("EXECUTE_ANIMS", 1, RecordFieldFamily::RecordDirective),
        ("INVALIDATE_ANIMS", 1, RecordFieldFamily::RecordDirective),
        (
            "ADD_OBJECTIVE_TARGET",
            4,
            RecordFieldFamily::ControlDirective,
        ),
        (
            "REMOVE_OBJECTIVE_TARGET",
            4,
            RecordFieldFamily::ControlDirective,
        ),
        ("ADD_OTHER_TARGET", 3, RecordFieldFamily::ControlDirective),
        ("ANIM_STATE", 3, RecordFieldFamily::ControlDirective),
        (
            "COMPLETED_STOPPOINT",
            1,
            RecordFieldFamily::ControlDirective,
        ),
        ("DEDG", 8, RecordFieldFamily::ControlDirective),
        ("SET_AI_NET", 3, RecordFieldFamily::ControlDirective),
        ("SET_HELP_LABEL", 2, RecordFieldFamily::ControlDirective),
        ("STOP_QUEUED_SOUNDS", 3, RecordFieldFamily::ControlDirective),
        ("TRAVELERS", 1, RecordFieldFamily::ControlDirective),
        ("WAKEUP_ENEMIES", 1, RecordFieldFamily::ControlDirective),
        ("WAKEUP_GENERATOR", 4, RecordFieldFamily::ControlDirective),
        ("WAKEUP_ZEP_TURRETS", 3, RecordFieldFamily::ControlDirective),
        ("WAKE_ANIM", 5, RecordFieldFamily::ControlDirective),
    ];
    let mut directive_sites = 0;
    for (key, count, family) in expected {
        let sites: Vec<_> = recovery
            .unrecovered()
            .iter()
            .filter(|field| field.key == *key)
            .collect();
        assert_eq!(sites.len(), *count, "{key} site count");
        assert!(
            sites.iter().all(|field| field.family == *family),
            "{key} family"
        );
        let top_level = *family == RecordFieldFamily::RecordDirective;
        assert!(
            sites.iter().all(|field| field.block.is_none() == top_level),
            "{key} position"
        );
        directive_sites += count;
    }
    assert_eq!(directive_sites, 50);

    // No key a measured stage covers is left generically Unmeasured anywhere
    // in the record — and no `Unmeasured` field names a covered key.
    for field in recovery.unrecovered() {
        if field.family == RecordFieldFamily::Unmeasured {
            assert!(
                measured_directive(&field.key).is_none()
                    && !RECORD_FIELD_KEY_VOCABULARY.contains(&field.key.as_str()),
                "{} is covered by a measured stage",
                field.key
            );
        }
    }
    let families = recovery.unrecovered_by_family();
    assert_eq!(families[&RecordFieldFamily::RecordDirective], 5);
    assert_eq!(families[&RecordFieldFamily::ControlDirective], 45);
    assert_eq!(
        families.values().sum::<usize>() + recovery.fields_recovered(),
        recovery.fields_read()
    );

    // The measured semantics stay unlowered: the program still refuses, now
    // naming the directive families instead of a generic Unmeasured.
    let refusal = recovery.program().expect_err("M01 stays unrecovered");
    assert!(refusal.to_string().contains("ControlDirective x45"));
    assert!(refusal.to_string().contains("RecordDirective x5"));
}
