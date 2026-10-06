//! Acceptance for `M01-LC-DIRECTIVE-A` (#679): the findings document
//! `docs/findings/2026-10-06-m01-lc-directive-a-objective-directive-parser.md`
//! records the native `CZMission` objective-directive parse for every key M01's
//! control program spells. These tests pin the **key census** the document maps:
//! the exact 43-key vocabulary the production control-program census reports
//! for `zbd/c1c/m01`, and the argument shapes the map claims each key carries.
//! The vocabulary comes from `survey_mission_control_programs`, a real
//! production read of the owner's installation, so the tests need `CS_GAME_DIR`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cs_app::mission_control::survey_mission_control_programs;

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR: retail acceptance needs the install"),
    )
}

/// The key vocabulary `M01-LC-DIRECTIVE-A` maps in the findings document: the
/// native parse sites listed there cover exactly these 43 spellings.
const M01_KEY_VOCABULARY: &[&str] = &[
    "ADD_OBJECTIVE_TARGET",
    "ADD_OTHER_TARGET",
    "ANIM_STATE",
    "BEGIN_DORMANT",
    "COMPLETED_SOUND_GROUP",
    "COMPLETED_STOPPOINT",
    "DEDG",
    "IDENTITY",
    "INACTIVE1",
    "INACTIVE10",
    "INACTIVE11",
    "INACTIVE12",
    "INACTIVE13",
    "INACTIVE14",
    "INACTIVE15",
    "INACTIVE16",
    "INACTIVE17",
    "INACTIVE18",
    "INACTIVE2",
    "INACTIVE3",
    "INACTIVE4",
    "INACTIVE5",
    "INACTIVE6",
    "INACTIVE7",
    "INACTIVE8",
    "INACTIVE9",
    "INACTIVE_COMPLETION_COUNT",
    "INSTANTLOSS",
    "INSTANTWIN",
    "KILL_OBJECTIVE_WHEN_I_COMPLETE",
    "NAP_OBJECTIVE_WHEN_I_COMPLETE",
    "REMOVE_OBJECTIVE_TARGET",
    "SET_AI_NET",
    "SET_HELP_LABEL",
    "STOP_QUEUED_SOUNDS",
    "TICK_DEPENDS_ON_OBJ",
    "TRAVELERS",
    "WAKEUP_ENEMIES",
    "WAKEUP_GENERATOR",
    "WAKEUP_SOUND_GROUP",
    "WAKEUP_ZEP_TURRETS",
    "WAKE_ANIM",
    "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
];

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directive_a_the_mapped_vocabulary_is_exactly_the_43_keys_m01_spells() {
    let census = survey_mission_control_programs(&game_dir()).expect("census runs on the install");
    let record = census
        .row("zbd/c1c/m01")
        .expect("M01 is present")
        .record()
        .expect("M01 declares a control program");
    let spelled: Vec<&str> = record.keys().iter().map(|key| key.key.as_str()).collect();
    assert_eq!(
        spelled,
        M01_KEY_VOCABULARY.to_vec(),
        "the 43-key vocabulary the directive map records is exactly what M01 spells"
    );
}

/// The findings document claims an argument parse per key; the census's
/// measured shapes are the only arity evidence the data itself offers, so the
/// map's recorded spellings must match them key for key. Shapes below are the
/// production census's own labels (`[args]` per site, `xN` sites), measured
/// from `zbd/c1c/m01/objectives.zrd`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directive_a_each_mapped_keys_argument_shapes_match_the_record() {
    let census = survey_mission_control_programs(&game_dir()).expect("census runs on the install");
    let record = census
        .row("zbd/c1c/m01")
        .expect("M01 is present")
        .record()
        .expect("M01 declares a control program");
    let measured: BTreeMap<String, Vec<String>> = record
        .keys()
        .iter()
        .map(|key| {
            (
                key.key.clone(),
                key.shapes
                    .iter()
                    .map(|(shape, count)| format!("{}x{}", shape.label(), count))
                    .collect(),
            )
        })
        .collect();
    let documented: BTreeMap<&str, &[&str]> = BTreeMap::from([
        ("ADD_OBJECTIVE_TARGET", &["[text]x2", "[[text,text]]x2"][..]),
        ("ADD_OTHER_TARGET", &["[text]x2", "[[text,text]]x1"][..]),
        ("ANIM_STATE", &["[text,[text,[text],text,[text]]]x3"][..]),
        ("BEGIN_DORMANT", &["[float]x52"][..]),
        ("COMPLETED_SOUND_GROUP", &["[text]x23"][..]),
        ("COMPLETED_STOPPOINT", &["[[text,int,int]]x1"][..]),
        ("DEDG", &["[int,int]x8"][..]),
        ("IDENTITY", &["[text,int]x1", "[text,int,text]x4"][..]),
        ("INACTIVE1", &["[text]x2", "[text,text,text]x10"][..]),
        ("INACTIVE10", &["[text,text,text]x8"][..]),
        ("INACTIVE11", &["[text,text,text]x8"][..]),
        ("INACTIVE12", &["[text,text,text]x8"][..]),
        ("INACTIVE13", &["[text,text,text]x4"][..]),
        ("INACTIVE14", &["[text,text,text]x4"][..]),
        ("INACTIVE15", &["[text,text,text]x4"][..]),
        ("INACTIVE16", &["[text,text,text]x4"][..]),
        ("INACTIVE17", &["[text,text,text]x4"][..]),
        ("INACTIVE18", &["[text,text,text]x4"][..]),
        ("INACTIVE2", &["[text,text,text]x10"][..]),
        ("INACTIVE3", &["[text,text,text]x10"][..]),
        ("INACTIVE4", &["[text,text,text]x10"][..]),
        ("INACTIVE5", &["[text,text,text]x10"][..]),
        ("INACTIVE6", &["[text,text,text]x10"][..]),
        ("INACTIVE7", &["[text,text,text]x10"][..]),
        ("INACTIVE8", &["[text,text,text]x10"][..]),
        ("INACTIVE9", &["[text,text,text]x10"][..]),
        ("INACTIVE_COMPLETION_COUNT", &["[int]x9"][..]),
        ("INSTANTLOSS", &["barex1"][..]),
        ("INSTANTWIN", &["barex1"][..]),
        (
            "KILL_OBJECTIVE_WHEN_I_COMPLETE",
            &["[int]x5", "[int,int]x2", "[int,int,int]x2"][..],
        ),
        ("NAP_OBJECTIVE_WHEN_I_COMPLETE", &["[int,float]x27"][..]),
        (
            "REMOVE_OBJECTIVE_TARGET",
            &["[text]x1", "[[text,text]]x3"][..],
        ),
        (
            "SET_AI_NET",
            &["[[text,text]]x2", "[[text,text],[text,text]]x1"][..],
        ),
        (
            "SET_HELP_LABEL",
            &["[text,text]x1", "[[text,text],text]x1"][..],
        ),
        ("STOP_QUEUED_SOUNDS", &["[text]x3"][..]),
        ("TICK_DEPENDS_ON_OBJ", &["[int]x3"][..]),
        ("TRAVELERS", &["[text,text,text,float,int]x1"][..]),
        ("WAKEUP_ENEMIES", &["[text,text]x1"][..]),
        ("WAKEUP_GENERATOR", &["[text,int]x4"][..]),
        ("WAKEUP_SOUND_GROUP", &["[text]x18"][..]),
        ("WAKEUP_ZEP_TURRETS", &["[text]x3"][..]),
        ("WAKE_ANIM", &["[text]x5"][..]),
        (
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
            &[
                "[int]x16",
                "[int,int]x2",
                "[int,int,int]x1",
                "[int,int,int,int]x1",
            ][..],
        ),
    ]);
    assert_eq!(
        documented.len(),
        M01_KEY_VOCABULARY.len(),
        "the documented shape table covers every vocabulary key"
    );
    for (key, shapes) in &documented {
        let expected: Vec<String> = shapes.iter().map(|shape| (*shape).to_owned()).collect();
        assert_eq!(
            measured.get(*key),
            Some(&expected),
            "measured argument shapes for {key}"
        );
    }
}
