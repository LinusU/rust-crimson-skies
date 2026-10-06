//! Acceptance for `M01-LC-DIRECTIVE-B` (#680): the findings document
//! `docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`
//! records the measured runtime semantics of M01's lifecycle and target
//! directives. These tests pin the **spellings the document interprets**: the
//! argument shapes the measured semantics apply to, and the exact set of
//! sites whose shapes the mission IR cannot carry. Both come from
//! `survey_mission_control_programs`, a real production read of the owner's
//! installation, so the tests need `CS_GAME_DIR`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cs_app::mission_control::survey_mission_control_programs;

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR: retail acceptance needs the install"),
    )
}

/// The keys the findings document gives runtime semantics for: the
/// lifecycle/target keys the task names and every related key M01 spells
/// alongside them, with the shapes those semantics apply to.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directive_b_each_measured_keys_argument_shapes_match_the_record() {
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
    // One row per key the document's semantics sections cover, in document
    // order. Lifecycle and target keys first, then the sibling world-effect
    // and outcome keys measured at their call sites.
    let documented: BTreeMap<&str, &[&str]> = BTreeMap::from([
        ("BEGIN_DORMANT", &["[float]x52"][..]),
        ("TICK_DEPENDS_ON_OBJ", &["[int]x3"][..]),
        ("IDENTITY", &["[text,int]x1", "[text,int,text]x4"][..]),
        (
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
            &[
                "[int]x16",
                "[int,int]x2",
                "[int,int,int]x1",
                "[int,int,int,int]x1",
            ][..],
        ),
        (
            "KILL_OBJECTIVE_WHEN_I_COMPLETE",
            &["[int]x5", "[int,int]x2", "[int,int,int]x2"][..],
        ),
        ("NAP_OBJECTIVE_WHEN_I_COMPLETE", &["[int,float]x27"][..]),
        ("INACTIVE1", &["[text]x2", "[text,text,text]x10"][..]),
        ("INACTIVE2", &["[text,text,text]x10"][..]),
        ("INACTIVE3", &["[text,text,text]x10"][..]),
        ("INACTIVE4", &["[text,text,text]x10"][..]),
        ("INACTIVE5", &["[text,text,text]x10"][..]),
        ("INACTIVE6", &["[text,text,text]x10"][..]),
        ("INACTIVE7", &["[text,text,text]x10"][..]),
        ("INACTIVE8", &["[text,text,text]x10"][..]),
        ("INACTIVE9", &["[text,text,text]x10"][..]),
        ("INACTIVE10", &["[text,text,text]x8"][..]),
        ("INACTIVE11", &["[text,text,text]x8"][..]),
        ("INACTIVE12", &["[text,text,text]x8"][..]),
        ("INACTIVE13", &["[text,text,text]x4"][..]),
        ("INACTIVE14", &["[text,text,text]x4"][..]),
        ("INACTIVE15", &["[text,text,text]x4"][..]),
        ("INACTIVE16", &["[text,text,text]x4"][..]),
        ("INACTIVE17", &["[text,text,text]x4"][..]),
        ("INACTIVE18", &["[text,text,text]x4"][..]),
        ("INACTIVE_COMPLETION_COUNT", &["[int]x9"][..]),
        ("DEDG", &["[int,int]x8"][..]),
        ("TRAVELERS", &["[text,text,text,float,int]x1"][..]),
        ("ANIM_STATE", &["[text,[text,[text],text,[text]]]x3"][..]),
        ("INSTANTWIN", &["barex1"][..]),
        ("INSTANTLOSS", &["barex1"][..]),
        ("ADD_OBJECTIVE_TARGET", &["[text]x2", "[[text,text]]x2"][..]),
        (
            "REMOVE_OBJECTIVE_TARGET",
            &["[text]x1", "[[text,text]]x3"][..],
        ),
        ("ADD_OTHER_TARGET", &["[text]x2", "[[text,text]]x1"][..]),
        ("COMPLETED_STOPPOINT", &["[[text,int,int]]x1"][..]),
        (
            "SET_AI_NET",
            &["[[text,text]]x2", "[[text,text],[text,text]]x1"][..],
        ),
        (
            "SET_HELP_LABEL",
            &["[text,text]x1", "[[text,text],text]x1"][..],
        ),
        ("STOP_QUEUED_SOUNDS", &["[text]x3"][..]),
        ("WAKEUP_ENEMIES", &["[text,text]x1"][..]),
        ("WAKEUP_GENERATOR", &["[text,int]x4"][..]),
        ("WAKEUP_SOUND_GROUP", &["[text]x18"][..]),
        ("WAKEUP_ZEP_TURRETS", &["[text]x3"][..]),
        ("WAKE_ANIM", &["[text]x5"][..]),
        ("COMPLETED_SOUND_GROUP", &["[text]x23"][..]),
    ]);
    for (key, shapes) in &documented {
        let expected: Vec<String> = shapes.iter().map(|shape| (*shape).to_owned()).collect();
        assert_eq!(
            measured.get(*key),
            Some(&expected),
            "measured argument shapes for {key} the document's semantics apply to"
        );
    }
    // The scope is bounded to M01's vocabulary: every documented key is one
    // of the 43, and nothing outside it is claimed.
    assert_eq!(
        measured.len(),
        43,
        "M01's whole vocabulary is what the document is bounded by"
    );
}

/// The document names every M01 spelling whose argument list nests a list —
/// the shapes `cs_script::ir::Value` cannot carry. Compute the same set from
/// the production census and require it to match the named table exactly: a
/// spelling that nests a list added to or removed from M01 changes the
/// documented set, and so does an `is_ir_carriable` change in production.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directive_b_the_named_non_carriable_shapes_match_the_record() {
    let census = survey_mission_control_programs(&game_dir()).expect("census runs on the install");
    let record = census
        .row("zbd/c1c/m01")
        .expect("M01 is present")
        .record()
        .expect("M01 declares a control program");
    let mut computed: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for key in record.keys() {
        let non_carriable: Vec<String> = key
            .shapes
            .iter()
            .filter(|(shape, _)| !shape.is_ir_carriable())
            .map(|(shape, count)| format!("{}x{}", shape.label(), count))
            .collect();
        if !non_carriable.is_empty() {
            computed.insert(key.key.clone(), non_carriable);
        }
    }
    // The document's "list-valued shapes the IR cannot carry" table, exactly.
    let documented: BTreeMap<&str, &[&str]> = BTreeMap::from([
        ("ADD_OBJECTIVE_TARGET", &["[[text,text]]x2"][..]),
        ("REMOVE_OBJECTIVE_TARGET", &["[[text,text]]x3"][..]),
        ("ADD_OTHER_TARGET", &["[[text,text]]x1"][..]),
        (
            "SET_AI_NET",
            &["[[text,text]]x2", "[[text,text],[text,text]]x1"][..],
        ),
        ("SET_HELP_LABEL", &["[[text,text],text]x1"][..]),
        ("ANIM_STATE", &["[text,[text,[text],text,[text]]]x3"][..]),
        ("COMPLETED_STOPPOINT", &["[[text,int,int]]x1"][..]),
    ]);
    let documented: BTreeMap<String, Vec<String>> = documented
        .into_iter()
        .map(|(key, shapes)| {
            (
                key.to_owned(),
                shapes.iter().map(|shape| (*shape).to_owned()).collect(),
            )
        })
        .collect();
    assert_eq!(
        computed, documented,
        "the non-carriable spellings named in the findings document"
    );
}
