//! Acceptance suite for `M01-LC-MISSION-PROGRAM` (#630): the mission control
//! program of M01, and what of it the engine may run.
//!
//! Task key `M01-LC-MISSION-PROGRAM`; test prefix `accept_m01_lc_`. Shared
//! contract: `docs/contracts/SCRIPT-MISSION.md` ("Source adapter acceptance",
//! "Host interface", "IR requirements", "Objective event ordering"). Finding:
//! `docs/findings/2026-10-04-m01-lc-mission-program.md`.
//!
//! # The question this suite answers
//!
//! The task was handed over with two readings attached and neither had been
//! checked: that `objectives.zrd` is M01's control program because it is an
//! `aiv`-shaped MISSION_CONTROL_MEMBER name, and that `wv_tailhook.zrd` is the
//! mission's driving program "by name shape only". Both are name readings. This
//! suite measures the control member **by a rule** instead — the member whose
//! decoded record declares numbered `OBJECTIVE<N>` blocks — and then measures
//! every directive that member spells, so the answer rests on the record rather
//! than on a filename.
//!
//! # What is measured and what is not
//!
//! Every figure the retail tests assert is re-derived from `$CS_GAME_DIR` on each
//! run, so a stale constant fails rather than passes. What a directive key *does*
//! is unmeasured: no original executable has been run, and `INSTANTWIN` naming a
//! win is a reading of a spelling. The two outcome keys are therefore the only
//! directives the engine may act on, and the campaign gate stays closed.
//!
//! Every value the synthetic tests use is newly authored `.zrd` bytes built here
//! tag by tag — no original game data is committed, and the synthetic records
//! exercise the same production reader and walk the retail census uses.

use std::path::PathBuf;

use cs_app::mission_control::{
    RetailControlCensus, read_control_member, survey_mission_control_programs,
};
use cs_content::mission_control::{
    CONTROL_MEMBER, CONTROL_RECORD_KEY_VOCABULARY, ControlLowering, ControlMemberError,
    ControlRecordField, DecodedMember, DirectiveDisposition, LoweringRequirementKind, MeasuredArg,
    TerminalOutcome, UnmeasuredReason, control_member, measure_control_record, objective_blocks_of,
    terminal_outcome_of,
};
use cs_content::stunts::{ZRD_TAG_FLOAT, ZRD_TAG_INT, ZRD_TAG_LIST, ZRD_TAG_TEXT, ZrdValue};

// ------------------------------------------------------------------ helpers ---

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

/// The installation's denominator: every mission-scoped reader measured, so the
/// figures below all refer to the same population.
fn census() -> RetailControlCensus {
    survey_mission_control_programs(&game_dir()).expect("the installation measures")
}

/// M01's mission label under F13-B's scope rule: chapter-1 group `c1c`, mission
/// `m01`, as M01-A's binding record (`missions/bindings/M01.json`) records it.
const M01: &str = "zbd/c1c/m01";

// ------------------------------------------------- the .zrd authoring helpers ---

/// A `.zrd` int node: tag `1` and the value.
fn zrd_int(value: u32) -> ZrdValue {
    ZrdValue::Int(value)
}

/// A `.zrd` float node: tag `2` and the bits.
fn zrd_float(value: f32) -> ZrdValue {
    ZrdValue::Float(value)
}

/// A `.zrd` text node: tag `3`, the byte length and the bytes.
fn zrd_text(text: &str) -> ZrdValue {
    ZrdValue::Text(text.to_owned())
}

/// A `.zrd` list node: tag `4`, then **`children.len() + 1`** as the count (the
/// measured convention, `cs_content::stunts::ZRD_TAG_LIST`), then the children.
fn zrd_list(children: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(children)
}

/// One authored directive of a block: the key, and — unless the directive is
/// authored bare — its argument list beside it.
///
/// Returns the **children** the directive contributes to its block, because a
/// block is the flat concatenation of its directives' children: that flatness is
/// exactly what the measured directive grammar reads, and a fixture that hid it
/// inside a nested list would not be exercising the same shape.
fn directive(key: &str, args: Vec<ZrdValue>) -> Vec<ZrdValue> {
    let mut children = vec![zrd_text(key)];
    if !args.is_empty() {
        children.push(zrd_list(args));
    }
    children
}

/// One authored objective block, as `objectives.zrd` spells it: the key and a
/// flat alternating list of its directives.
fn block(number: u32, directives: Vec<Vec<ZrdValue>>) -> (String, ZrdValue) {
    let mut children = Vec::new();
    for children_of_directive in directives {
        children.extend(children_of_directive);
    }
    (format!("OBJECTIVE{number}"), zrd_list(children))
}

/// A wrapped control record: the root one-element list holding the flat record,
/// exactly the shape the production reader unwraps.
fn control_record(fields: Vec<(String, ZrdValue)>) -> ZrdValue {
    let mut children = Vec::new();
    for (key, value) in fields {
        children.push(zrd_text(&key));
        children.push(value);
    }
    zrd_list(vec![zrd_list(children)])
}

/// An authored record with one block spelling `INSTANTWIN` bare beside an
/// `IDENTITY` site, so the suite has a record whose only implemented directive is
/// an outcome key.
fn authored_record() -> ZrdValue {
    control_record(vec![
        ("MISSION_TIMER".to_owned(), zrd_list(vec![zrd_float(0.0)])),
        (
            "OBJECTIVE1".to_owned(),
            zrd_list(vec![
                zrd_text("IDENTITY"),
                zrd_list(vec![zrd_text("PRIMARY"), zrd_int(1), zrd_text("MSG_X")]),
                zrd_text("INSTANTWIN"),
            ]),
        ),
    ])
}

// ---------------------------------------------------------------------------
// The control-member rule
// ---------------------------------------------------------------------------

/// **The observable failure of this stage.** A reader that picks a mission's
/// control program by member name, size or position reports a program it never
/// checked. The witness is built here so it needs no installation: an archive
/// whose longest member is an animation definition and whose *shortest* member is
/// the one carrying numbered blocks. With the rule reduced to "the biggest
/// member", `control_member` answers the animation definition and M01 would fly a
/// cinematic as its mission program.
#[test]
fn accept_m01_lc_the_control_member_is_chosen_by_its_blocks_not_its_size() {
    // A big animation member: many directives, none of them a numbered block.
    let mut animation = Vec::new();
    for index in 0..64 {
        animation.extend(directive(
            &format!("SEQUENCE_DEFINITION_{index}"),
            vec![zrd_text("whee")],
        ));
    }
    let animation = DecodedMember::new(
        "wv_tailhook.zrd",
        control_record(vec![(
            "ANIMATION_DEFINITIONS".to_owned(),
            zrd_list(animation),
        )]),
    );

    // The real control member: one block, far fewer bytes.
    let control = DecodedMember::new(
        CONTROL_MEMBER,
        control_record(vec![block(1, vec![directive("INSTANTWIN", Vec::new())])]),
    );

    let members = vec![animation, control];
    assert!(
        objective_blocks_of(&members[0]) == 0,
        "the animation member must carry no numbered block, or the rule has nothing to choose between"
    );
    assert_eq!(
        objective_blocks_of(&members[1]),
        1,
        "the control member carries exactly the block the fixture authored"
    );

    let chosen = control_member("zbd/c1/m01/zrdr.zbd", &members).expect("the rule finds one");
    assert_eq!(
        chosen.name, CONTROL_MEMBER,
        "the member with the numbered block is the control program, whatever its length"
    );

    // And the refusal: an archive with no qualifying member is refused rather
    // than answered with the largest member.
    let refused = control_member("zbd/c1/m01/zrdr.zbd", &[members[0].clone()])
        .expect_err("an archive with no numbered block has no control program");
    assert_eq!(
        refused,
        ControlMemberError::NoControlMember {
            container: "zbd/c1/m01/zrdr.zbd".to_owned(),
            members: 1,
        },
        "the refusal names the archive and how many members it offered"
    );
    assert!(
        refused.to_string().contains("1 member(s)"),
        "the refusal text says what was searched: {refused}"
    );
}

/// An archive with **two** qualifying members has no single control program, and
/// picking either would be a guess about which half drives the mission.
#[test]
fn accept_m01_lc_two_qualifying_members_are_a_refusal_not_a_choice() {
    let first = DecodedMember::new(
        "first.zrd",
        control_record(vec![block(1, vec![directive("INSTANTWIN", Vec::new())])]),
    );
    let second = DecodedMember::new(
        "second.zrd",
        control_record(vec![block(7, vec![directive("INSTANTLOSS", Vec::new())])]),
    );
    let refused = control_member("zbd/c1/m01/zrdr.zbd", &[first, second])
        .expect_err("two candidates cannot be resolved without evidence");
    match refused {
        ControlMemberError::AmbiguousControlMember { container, members } => {
            assert_eq!(container, "zbd/c1/m01/zrdr.zbd");
            assert_eq!(members, ["first.zrd", "second.zrd"]);
        }
        other => panic!("expected an ambiguity refusal, got {other:?}"),
    }
}

/// The rule is one rule, not a name list: a control member under a name the
/// constant has never seen is still found, and a member named
/// [`CONTROL_MEMBER`] that stops carrying blocks stops being accepted.
#[test]
fn accept_m01_lc_the_rule_follows_the_blocks_and_not_the_member_name() {
    let renamed = DecodedMember::new(
        "not_the_name_anyone_expected.zrd",
        control_record(vec![block(3, vec![directive("INSTANTLOSS", Vec::new())])]),
    );
    let chosen = control_member("zbd/c1/m01/zrdr.zbd", std::slice::from_ref(&renamed))
        .expect("a record with a block qualifies whatever the member is called");
    assert_eq!(chosen.name, renamed.name);

    // The mirror: the constant's own name, carrying no block, does not qualify.
    let empty = DecodedMember::new(
        CONTROL_MEMBER,
        control_record(vec![(
            "ANIMATION_DEFINITIONS".to_owned(),
            zrd_list(Vec::new()),
        )]),
    );
    assert!(
        control_member("zbd/c1/m01/zrdr.zbd", &[empty]).is_err(),
        "a member carrying no numbered block is not a control program, whatever it is called"
    );
}

// ---------------------------------------------------------------------------
// The directive walk
// ---------------------------------------------------------------------------

/// A key followed immediately by another key carries **no** argument list, and a
/// key followed by a list carries exactly that list.
///
/// The asymmetry is the measured grammar: reading the pair as `key, value` would
/// attribute the next key's name to the previous key's argument list, which is
/// how a naive walk turns `INSTANTWIN` into a directive with one string argument
/// and then reports an argument shape the original never wrote.
#[test]
fn accept_m01_lc_a_bare_directive_is_not_read_as_carrying_the_next_key() {
    let document = control_record(vec![block(
        1,
        vec![
            directive("INSTANTWIN", Vec::new()),
            directive("INSTANTLOSS", Vec::new()),
            directive("WAKEUP_SOUND_GROUP", vec![zrd_text("snd_x")]),
        ],
    )]);
    let record = measure_control_record(&document);

    let win = record.key("INSTANTWIN").expect("the bare key is measured");
    assert_eq!(
        win.agreed_shape(),
        Some(&cs_content::mission_control::DirectiveShape::Bare),
        "a key with no argument list beside it is measured as bare, not as carrying the next key"
    );
    assert_eq!(win.sites, 1);

    let loss = record
        .key("INSTANTLOSS")
        .expect("the second bare key is measured");
    assert_eq!(loss.agreed_shape().map(|shape| shape.arity()), Some(0));

    let sound = record
        .key("WAKEUP_SOUND_GROUP")
        .expect("the keyed directive is measured");
    assert_eq!(
        sound.agreed_shape(),
        Some(&cs_content::mission_control::DirectiveShape::Arguments(
            vec![MeasuredArg::Text]
        )),
        "the argument list beside the key is measured as its arguments, in order"
    );

    assert_eq!(
        record.sites(),
        3,
        "three directives, three sites: the walk counts each exactly once"
    );
    assert!(
        record.refusals().is_empty(),
        "a well-formed record raises no block refusal, got {:?}",
        record.refusals()
    );
}

/// A nested argument list is measured as nested, and it is refused by name
/// because `cs_script::ir::Value` has no list variant. Flattening it into a
/// positional `Vec<Value>` would be a format change presented as a binding.
#[test]
fn accept_m01_lc_a_nested_argument_shape_is_named_and_never_flattened() {
    // `ANIM_STATE`'s measured M01 shape: one name beside a nested descriptor.
    let document = control_record(vec![block(
        1,
        vec![directive(
            "ANIM_STATE",
            vec![zrd_text("wv_tailhook"), zrd_list(vec![zrd_text("x")])],
        )],
    )]);
    let record = measure_control_record(&document);
    let key = record.key("ANIM_STATE").expect("the directive is measured");

    assert_eq!(
        key.agreed_shape(),
        Some(&cs_content::mission_control::DirectiveShape::Arguments(
            vec![
                MeasuredArg::Text,
                MeasuredArg::List(vec![MeasuredArg::Text]),
            ]
        )),
        "the nested list is preserved as a nested shape rather than flattened into two arguments"
    );
    assert!(
        !key.agreed_shape()
            .is_some_and(|shape| shape.is_ir_carriable()),
        "a shape nesting a list is not IR-carriable"
    );

    match key.disposition() {
        DirectiveDisposition::Unmeasured {
            reason: UnmeasuredReason::ArgumentShapeHasNoValue { shape },
        } => {
            assert_eq!(
                shape.label(),
                "[text,[text]]",
                "the refusal names the shape"
            );
        }
        other => panic!("a nested shape must be refused by name, got {other:?}"),
    }
    assert!(!record.is_complete());
}

/// Sites that disagree about a key's argument shape are both kept, and the key is
/// refused as disagreeing rather than resolved to the majority shape.
///
/// The witness is the measured `INACTIVE1` disagreement: ten sites spell a node,
/// a part and a part-state; two spell a node alone. A reader that picked the
/// majority would be inventing a rule the original does not state.
#[test]
fn accept_m01_lc_disagreeing_argument_shapes_are_both_kept_and_refused() {
    let document = control_record(vec![
        block(
            1,
            vec![directive(
                "INACTIVE1",
                vec![
                    zrd_text("workersvoyagezep"),
                    zrd_text("reng1"),
                    zrd_text("healthy"),
                ],
            )],
        ),
        block(2, vec![directive("INACTIVE1", vec![zrd_text("piratezep")])]),
    ]);
    let record = measure_control_record(&document);
    let key = record.key("INACTIVE1").expect("the stage key is measured");

    assert_eq!(key.sites, 2, "both sites are counted");
    assert_eq!(key.blocks, 2, "both blocks are counted");
    assert_eq!(
        key.shapes.len(),
        2,
        "both shapes are kept: {:?}",
        key.shapes
            .iter()
            .map(|(shape, sites)| (shape.label(), *sites))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        key.shapes[0].1, 1,
        "the disagreeing shapes are counted per shape"
    );
    assert!(
        key.agreed_shape().is_none(),
        "disagreeing sites have no agreed shape"
    );
    match key.disposition() {
        DirectiveDisposition::Unmeasured {
            reason: UnmeasuredReason::DisagreeingArgumentShape { shapes },
        } => assert_eq!(shapes, 2),
        other => panic!("disagreeing sites must be refused, got {other:?}"),
    }
}

/// A block the walk cannot read is a refusal naming the block and the child, not a
/// silently shorter census.
#[test]
fn accept_m01_lc_an_unreadable_block_is_refused_with_its_block_and_child() {
    // A block whose value is not a list at all.
    let not_a_list = measure_control_record(&control_record(vec![(
        "OBJECTIVE4".to_owned(),
        zrd_text("this is not a directive list"),
    )]));
    assert_eq!(
        not_a_list.refusals(),
        &[cs_content::mission_control::BlockRefusal::BlockNotAList {
            block: "OBJECTIVE4".to_owned(),
        }],
        "a block that is not a list is refused by block"
    );
    assert_eq!(
        not_a_list.blocks(),
        1,
        "the block is still counted: a refusal is not a skip"
    );
    assert!(
        !not_a_list.is_complete(),
        "a record with an unreadable block is never complete"
    );

    // A block whose directive key is not text: the walk stops at that child and
    // says where, rather than reading an integer as a directive.
    let not_text = measure_control_record(&control_record(vec![(
        "OBJECTIVE5".to_owned(),
        zrd_list(vec![zrd_int(7), zrd_list(vec![zrd_text("x")])]),
    )]));
    assert_eq!(
        not_text.refusals(),
        &[cs_content::mission_control::BlockRefusal::KeyNotText {
            block: "OBJECTIVE5".to_owned(),
            index: 0,
        }],
        "a non-text directive key is refused with its child position"
    );
    assert!(
        not_text.to_lowering_refusal().contains("OBJECTIVE5"),
        "the lowering refusal names the unreadable block: {}",
        not_text.to_lowering_refusal()
    );
}

/// Every site the record spells appears in exactly one key's count, so a census
/// that dropped a directive would fail here rather than report a smaller total.
#[test]
fn accept_m01_lc_the_key_counts_reconcile_with_the_declared_site_total() {
    let document = control_record(vec![
        ("MISSION_TIMER".to_owned(), zrd_list(vec![zrd_float(0.0)])),
        ("RESTORE_ANIMS".to_owned(), zrd_list(Vec::new())),
        block(
            1,
            vec![
                directive("BEGIN_DORMANT", vec![zrd_float(-1.0)]),
                directive("WAKE_ANIM", vec![zrd_text("wv_hookup")]),
                directive("WAKE_ANIM", vec![zrd_text("wv_hookup_lights")]),
                directive("INSTANTWIN", Vec::new()),
            ],
        ),
        block(2, vec![directive("WAKE_ANIM", vec![zrd_text("letterbox")])]),
    ]);
    let record = measure_control_record(&document);

    let by_key: u32 = record.keys().iter().map(|key| key.sites).sum();
    assert_eq!(
        by_key,
        record.sites(),
        "every measured site belongs to exactly one key, so the two totals are equal"
    );
    assert_eq!(
        record.sites(),
        5,
        "block 1 spells 4 directives (BEGIN_DORMANT, WAKE_ANIM twice, INSTANTWIN) \
         and block 2 spells 1: the walk counts each exactly once"
    );
    assert_eq!(
        record.key("WAKE_ANIM").map(|key| (key.sites, key.blocks)),
        Some((3, 2)),
        "a key spelled twice in one block and once in another counts three sites \
         and two blocks"
    );
    assert_eq!(
        record.vocabulary(),
        3,
        "three distinct keys — BEGIN_DORMANT, WAKE_ANIM and INSTANTWIN — for five sites"
    );
}

/// The record fields outside the numbered blocks are counted and classified, and
/// a key outside the measured vocabulary is reported as **unclassified** rather
/// than absorbed into a documented field.
#[test]
fn accept_m01_lc_record_fields_are_classified_and_an_unknown_key_stays_unclassified() {
    let document = control_record(vec![
        ("MISSION_TIMER".to_owned(), zrd_list(vec![zrd_float(0.0)])),
        (
            "PLAYER_INIT".to_owned(),
            zrd_list(vec![
                zrd_int(1),
                zrd_list(vec![zrd_float(0.0), zrd_float(1.0), zrd_float(2.0)]),
                zrd_list(vec![zrd_float(0.0), zrd_float(90.0), zrd_float(0.0)]),
                zrd_float(0.5),
                zrd_float(600.0),
            ]),
        ),
        ("RESTORE_ANIMS".to_owned(), zrd_list(Vec::new())),
        ("EXECUTE_ANIMS".to_owned(), zrd_list(Vec::new())),
        ("INVALIDATE_ANIMS".to_owned(), zrd_list(Vec::new())),
        (
            "A_KEY_NOBODY_HAS_MEASURED".to_owned(),
            zrd_list(vec![zrd_int(1)]),
        ),
        block(1, vec![directive("INSTANTLOSS", Vec::new())]),
    ]);
    let record = measure_control_record(&document);

    let classified: Vec<&str> = record
        .record_fields()
        .iter()
        .map(|(field, _)| field.key())
        .collect();
    assert_eq!(
        classified,
        [
            "MISSION_TIMER",
            "PLAYER_INIT",
            "RESTORE_ANIMS",
            "EXECUTE_ANIMS",
            "INVALIDATE_ANIMS",
        ],
        "the five measured record fields are classified, in the constant's order"
    );
    assert_eq!(
        CONTROL_RECORD_KEY_VOCABULARY.as_slice(),
        classified.as_slice(),
        "the constant and the walk classify the same five fields"
    );
    assert_eq!(
        record.unclassified_record_keys(),
        ["A_KEY_NOBODY_HAS_MEASURED"],
        "a key outside the measured vocabulary is reported, not absorbed and not dropped"
    );
    assert!(
        record
            .record_field_shapes()
            .iter()
            .any(|(key, shape)| key == "PLAYER_INIT"
                && shape.label() == "[int,[float,float,float],[float,float,float],float,float]"),
        "the player-init shape is measured as five positions: {:?}",
        record.record_field_shapes()
    );

    // And the support level is the honest one: the shape is measured, the effect
    // is not, so no duration or coordinate may be read out of it.
    assert_eq!(
        ControlRecordField::PlayerInit.support().label(),
        "shape_measured"
    );
    assert!(
        ControlRecordField::MissionTimer
            .support()
            .refusal()
            .contains("no original observation states what the value does"),
        "the refusal says what is missing"
    );
}

/// **A bare key is not automatically an outcome key.** The measured corpus
/// contains bare keys that spell ordinary English — `Change`, `to`, `mobile`,
/// `net`, all bare — in `ZBD/C4/M01`'s `OBJECTIVE24`, where they sit between
/// `BEGIN_DORMANT` and `SET_AI_NET` and read as an author's stray note in the
/// directive stream.
///
/// A rule that read "bare ⇒ terminal outcome" would give those four words a
/// terminal operation. The disposition is a **name match against the measured
/// outcome vocabulary** instead, so an unrecognized bare key is refused like any
/// other unmeasured key — and it is still **counted**, so the stray note cannot
/// quietly disappear from a site's total.
#[test]
fn accept_m01_lc_a_bare_key_that_is_not_a_measured_outcome_key_is_refused() {
    let document = control_record(vec![block(
        1,
        vec![
            directive("Change", Vec::new()),
            directive("to", Vec::new()),
            directive("mobile", Vec::new()),
            directive("net", Vec::new()),
            directive("WAKE_ANIM", vec![zrd_text("x")]),
        ],
    )]);
    let record = measure_control_record(&document);

    assert_eq!(
        record.sites(),
        5,
        "the walk counts every bare key as a site, including the English ones"
    );
    assert!(
        record.implemented().is_empty(),
        "no bare key outside the measured outcome vocabulary reaches an engine \
         operation: {:?}",
        record.implemented()
    );
    for word in ["Change", "to", "mobile", "net"] {
        let key = record
            .key(word)
            .unwrap_or_else(|| panic!("the stray note {word} is still counted"));
        assert_eq!(
            key.agreed_shape(),
            Some(&cs_content::mission_control::DirectiveShape::Bare),
            "{word} is measured as a bare site, not dropped"
        );
        assert_eq!(
            key.disposition(),
            DirectiveDisposition::Unmeasured {
                reason: UnmeasuredReason::MeaningNotMeasured,
            },
            "{word} is refused with the same reason as any unmeasured key"
        );
    }
}

/// The two outcome keys are the only directives the engine may act on, and the
/// reading is named as a reading of a spelling rather than an observation.
#[test]
fn accept_m01_lc_only_an_outcome_key_reaches_an_engine_operation() {
    let document = control_record(vec![block(
        1,
        vec![
            directive("INSTANTWIN", Vec::new()),
            directive("WAKE_ANIM", vec![zrd_text("x")]),
        ],
    )]);
    let record = measure_control_record(&document);

    let implemented = record.implemented();
    assert_eq!(
        implemented
            .iter()
            .map(|(key, outcome)| (key.key.as_str(), *outcome))
            .collect::<Vec<_>>(),
        [("INSTANTWIN", TerminalOutcome::Succeeded)],
        "the bare outcome key is the one directive with an engine operation"
    );

    let unmeasured: Vec<(&str, UnmeasuredReason)> = record
        .unmeasured()
        .iter()
        .map(|(key, disposition)| {
            let reason = disposition
                .refusal()
                .expect("an unmeasured disposition carries a reason");
            (key.key.as_str(), reason.clone())
        })
        .collect();
    assert_eq!(unmeasured.len(), 1);
    assert_eq!(unmeasured[0].0, "WAKE_ANIM");
    assert_eq!(
        unmeasured[0].1,
        UnmeasuredReason::MeaningNotMeasured,
        "a keyed directive with an IR-carriable shape is still refused: what it \
         does is unmeasured, not its argument list"
    );

    // The outcome vocabulary cannot drift from the one F39-D measured.
    for key in cs_content::objectives::FAILURE_KEY_VOCABULARY {
        assert!(
            terminal_outcome_of(key).is_some(),
            "every measured outcome key has a reading: {key}"
        );
    }
    assert_eq!(
        terminal_outcome_of("WAKE_ANIM"),
        None,
        "a directive outside the measured outcome vocabulary has no reading"
    );
    assert!(
        record.to_lowering_refusal().contains("unmeasured"),
        "the refusal names what is unmeasured"
    );
}

// ---------------------------------------------------------------------------
// The lowering accounting
// ---------------------------------------------------------------------------

/// The accounting is requirement-by-requirement and fails closed: the mission id
/// is met (it comes from the path, not the member), and the other three name
/// their unmeasured fields. An unmet row with nothing named would be the failure
/// this accounting exists to prevent.
#[test]
fn accept_m01_lc_the_lowering_accounting_names_what_each_unmet_requirement_lacks() {
    let record = measure_control_record(&control_record(vec![block(
        1,
        vec![
            directive(
                "IDENTITY",
                vec![zrd_text("PRIMARY"), zrd_int(1), zrd_text("MSG_X")],
            ),
            directive("BEGIN_DORMANT", vec![zrd_float(-1.0)]),
            directive("INACTIVE1", vec![zrd_text("workersvoyagezep")]),
            directive("INACTIVE_COMPLETION_COUNT", vec![zrd_int(4)]),
            directive("INSTANTWIN", Vec::new()),
        ],
    )]));
    let lowering = record.lowering();

    let rows: Vec<(&LoweringRequirementKind, bool)> = lowering
        .requirements()
        .iter()
        .map(|row| (&row.kind, row.met))
        .collect();
    assert_eq!(
        rows,
        [
            (&LoweringRequirementKind::MissionIdentity, true),
            (&LoweringRequirementKind::ObjectiveIdentity, false),
            (&LoweringRequirementKind::ObjectiveCondition, false),
            (&LoweringRequirementKind::CallArguments, false),
        ],
        "one row per requirement of lower_program, in the order it needs them"
    );

    for requirement in lowering.unmet() {
        assert!(
            !requirement.unmeasured_fields.is_empty(),
            "{}: an unmet requirement must name what it lacks",
            requirement.label()
        );
    }
    let fields = lowering.unmeasured_fields();
    assert!(
        fields.iter().any(|field| field.contains("IDENTITY")),
        "the identity row names the integer it cannot read: {fields:?}"
    );
    assert!(
        fields
            .iter()
            .any(|field| field.contains("INACTIVE_COMPLETION_COUNT")),
        "the condition row names the threshold it cannot read: {fields:?}"
    );
    assert!(
        !lowering.complete(),
        "a record with unmeasured directives is never complete"
    );
    assert_eq!(
        lowering.unmet().count(),
        3,
        "three of the four requirements are unmet on a record that spells directives"
    );

    // The measurement text carries the counted numbers behind each verdict.
    let identity = lowering
        .requirements()
        .iter()
        .find(|row| row.kind == LoweringRequirementKind::ObjectiveIdentity)
        .expect("the identity row exists");
    assert!(
        identity.measurement.contains("1 numbered block(s)"),
        "the identity row counts the blocks it is about: {}",
        identity.measurement
    );
    let condition = lowering
        .requirements()
        .iter()
        .find(|row| row.kind == LoweringRequirementKind::ObjectiveCondition)
        .expect("the condition row exists");
    assert!(
        condition.measurement.contains("1 inactive-stage site(s)"),
        "the condition row counts the stage sites: {}",
        condition.measurement
    );
    assert!(
        condition
            .measurement
            .contains("1 completion-count threshold(s)"),
        "and the thresholds beside them: {}",
        condition.measurement
    );
}

/// An **empty** record does not report itself complete: a census that answered
/// "complete" for a member nobody read would fail open on the input most likely to
/// be wrong.
#[test]
fn accept_m01_lc_an_empty_record_is_not_complete() {
    let record = measure_control_record(&control_record(Vec::new()));
    assert_eq!(record.keys().len(), 0);
    assert_eq!(record.sites(), 0);
    assert!(
        !record.is_complete(),
        "a record with no measured keys is never complete"
    );
    assert!(
        ControlLowering::measure(&record).requirements().len() == 4,
        "the accounting still answers every requirement, so an empty record is \
         refused by rule rather than by accident"
    );
}

// ---------------------------------------------------------------------------
// The retail measurement
// ---------------------------------------------------------------------------

/// M01's own control program: which member carries it, and what it declares.
///
/// Re-derived from the installation on every run. The assertions that matter are
/// the ones a name reading would fail: the control member is the one with the
/// blocks, and the census measured **every** member so the rule had candidates.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_m01s_control_member_is_measured_and_not_assumed() {
    let census = census();
    let row = census.row(M01).unwrap_or_else(|| {
        panic!(
            "{M01} is measured; the census holds {} missions",
            census.len()
        )
    });

    let (member, offset, len, sha256) = match &row.program {
        cs_app::mission_control::ControlProgram::Measured {
            member,
            offset,
            len,
            sha256,
            ..
        } => (member.clone(), *offset, *len, sha256.clone()),
        other => panic!("{M01} declares a control program, got {}", other.label()),
    };
    assert_eq!(
        member.to_lowercase(),
        CONTROL_MEMBER,
        "the measured control member is the objectives record"
    );

    // The rule had candidates: every member of the archive was decoded, and
    // exactly one declares numbered blocks.
    assert!(
        row.members.len() >= 2,
        "the archive declares more than one member, so the control member was \
         chosen rather than found by being the only file: {:?}",
        row.members.iter().map(|m| &m.name).collect::<Vec<_>>()
    );
    let with_blocks: Vec<&str> = row
        .members
        .iter()
        .filter(|member| member.objective_blocks > 0)
        .map(|member| member.name.as_str())
        .collect();
    assert_eq!(
        with_blocks,
        [CONTROL_MEMBER],
        "exactly one member of the archive declares numbered objective blocks"
    );
    assert_eq!(
        row.members
            .iter()
            .filter(|member| member.is_control)
            .count(),
        1,
        "the census marks exactly one member as the control program"
    );

    // Every member's own extent was located inside the archive, so the digest and
    // the span describe bytes that exist.
    assert!(
        row.members
            .iter()
            .any(|member| member.offset == offset && member.len == len),
        "the control member's extent is one of the located member rows"
    );
    assert_eq!(
        sha256.len(),
        64,
        "the control member carries a digest of its own bytes"
    );
    assert!(len > 0 && offset + len > offset, "the extent is non-empty");

    // The record: blocks, sites and a vocabulary that reconciles.
    let record = row.record().expect("M01 declares a control program");
    assert_eq!(
        record.blocks() as usize,
        row.members
            .iter()
            .find(|member| member.is_control)
            .map(|member| member.objective_blocks as usize)
            .expect("the control member is in the list"),
        "the measured block count and the member row's block count are the same number"
    );
    assert!(
        record.blocks() >= 50,
        "M01 declares a substantial objective program, measured {} blocks",
        record.blocks()
    );
    assert_eq!(
        record.keys().iter().map(|key| key.sites).sum::<u32>(),
        record.sites(),
        "every measured site belongs to exactly one key"
    );
    assert!(
        record.vocabulary() >= 40,
        "M01's control program spells a wide directive vocabulary, measured {}",
        record.vocabulary()
    );
    assert!(
        record.refusals().is_empty(),
        "M01's control record parses under the measured directive grammar; refusals: {:?}",
        record.refusals()
    );
    assert!(
        record.unclassified_record_keys().is_empty(),
        "every record key outside the numbered blocks is one this crate names: {:?}",
        record.unclassified_record_keys()
    );
    assert_eq!(
        record
            .record_fields()
            .iter()
            .map(|(field, _)| field.key())
            .collect::<Vec<_>>(),
        CONTROL_RECORD_KEY_VOCABULARY,
        "M01 carries exactly the five measured record fields"
    );
}

/// The name reading the task started from is measurably the wrong rule: the
/// member a reader would guess from "the mission's driving program by name shape"
/// is 1.61x the length of the control member (measured: 38639 bytes beside 24012)
/// and declares no objective block at all.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_the_longest_member_is_not_the_control_program() {
    let (document, member) =
        read_control_member(&game_dir(), M01).expect("M01's control member is read by the rule");

    // The control member, reached through the rule and not through a filename.
    assert_eq!(
        objective_blocks_of(&DecodedMember::new("x", document.clone())),
        58
    );
    assert_eq!(
        (member.objective_blocks, member.is_control),
        (58, true),
        "the row the reader returns describes the member the rule chose: it carries \
         that member's own measured block count and marks it as the control program"
    );

    // Now measure every member of the same archive through production discovery
    // and compare: the largest member is a different file.
    let row = census().row(M01).expect("M01 is measured").clone();
    let control_len = match &row.program {
        cs_app::mission_control::ControlProgram::Measured { len, .. } => *len,
        other => panic!("M01 declares a control program, got {}", other.label()),
    };
    let largest = row
        .members
        .iter()
        .max_by_key(|member| member.len)
        .expect("the archive declares members");
    assert!(
        largest.len > control_len,
        "M01's largest member ({} bytes) is longer than its control member ({} \
         bytes), so size is not the rule",
        largest.len,
        control_len
    );
    assert!(
        !largest.is_control,
        "the largest member is not the control program: {}",
        largest.name
    );
    assert_eq!(
        largest.objective_blocks, 0,
        "the largest member declares no objective block: {}",
        largest.name
    );
    assert_eq!(
        member.name.to_lowercase(),
        CONTROL_MEMBER,
        "the rule and the census name the same member"
    );
}

/// What the engine may honour, measured: the two outcome keys and nothing else,
/// with every other key named by its refusal reason.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_every_directive_m01_spells_is_measured_and_only_outcomes_run() {
    let census = census();
    let row = census.row(M01).expect("M01 is measured");

    let record = row.record().expect("M01 declares a control program");
    let implemented: Vec<(&str, TerminalOutcome)> = record
        .implemented()
        .iter()
        .map(|(key, outcome)| (key.key.as_str(), *outcome))
        .collect();
    assert_eq!(
        implemented,
        [
            ("INSTANTLOSS", TerminalOutcome::Failed),
            ("INSTANTWIN", TerminalOutcome::Succeeded),
        ],
        "M01's only implemented directives are its two bare outcome keys, each with \
         the reading its spelling names"
    );

    // Every other key carries a named refusal, and the reasons partition the
    // vocabulary: a key cannot be in none of the three categories.
    let by_reason = record.unmeasured().iter().fold(
        std::collections::BTreeMap::<&str, Vec<&str>>::new(),
        |mut counts, (key, disposition)| {
            let reason = disposition
                .refusal()
                .expect("an unmeasured disposition carries a reason");
            counts
                .entry(reason.code())
                .or_default()
                .push(key.key.as_str());
            counts
        },
    );
    let refused: usize = by_reason.values().map(Vec::len).sum();
    assert_eq!(
        refused + implemented.len(),
        record.vocabulary() as usize,
        "every key is either implemented or refused by a named reason: {:?}",
        by_reason
    );
    for (code, keys) in &by_reason {
        assert!(
            matches!(
                *code,
                "meaning_not_measured"
                    | "disagreeing_argument_shape"
                    | "argument_shape_has_no_value"
            ),
            "the refusal codes are the three declared ones, got {code}"
        );
        assert!(!keys.is_empty(), "{code} has keys");
    }
    // The split the finding quotes, measured per record: a disposition is a
    // property of one archive's sites, so it is counted here and not corpus-wide.
    assert_eq!(
        (
            by_reason.get("meaning_not_measured").map_or(0, Vec::len),
            by_reason
                .get("disagreeing_argument_shape")
                .map_or(0, Vec::len),
            by_reason
                .get("argument_shape_has_no_value")
                .map_or(0, Vec::len),
            implemented.len()
        ),
        (30, 9, 2, 2),
        "M01's 43 keys split into 30 unmeasured-of-meaning, 9 disagreeing, 2 with a \
         nested agreed shape and the 2 outcome keys: {by_reason:?}"
    );
    assert!(
        by_reason.contains_key("meaning_not_measured"),
        "most keys are refused for want of a measured meaning, which is the \
         honest reading of a spelling: {by_reason:?}"
    );
    assert!(
        !record.is_complete(),
        "M01 is not complete: its record declares directives the engine cannot honour"
    );

    // A bare key in M01 is not automatically an outcome key. The measured corpus
    // contains bare keys that spell ordinary English (`Change`, `to`, `mobile`),
    // which is exactly why the disposition rule is a **name match against the
    // measured outcome vocabulary** and never "bare implies terminal".
    let bare: Vec<&str> = record
        .keys()
        .iter()
        .filter(|key| {
            key.agreed_shape()
                .is_some_and(|shape| shape.label() == "bare")
        })
        .map(|key| key.key.as_str())
        .collect();
    assert!(
        bare.contains(&"INSTANTWIN") && bare.contains(&"INSTANTLOSS"),
        "M01's outcome keys are bare: {bare:?}"
    );
    for key in &bare {
        assert!(
            terminal_outcome_of(key).is_some(),
            "in M01 every bare key spells an outcome: {key}"
        );
    }
}

/// The corpus-wide gate. Every mission is measured with the same rule, every
/// measured record is incomplete, and the census's positive name for a
/// releasable mission is empty.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_every_mission_is_measured_and_none_is_campaign_ready() {
    let census = census();
    assert!(
        census.len() >= 50,
        "the campaign's mission readers are all measured, got {}",
        census.len()
    );
    assert!(
        census.measured_len() > 0 && census.measured_len() < census.len(),
        "some readers carry a control program and some do not, so the census \
         measures two populations: {} of {}",
        census.measured_len(),
        census.len()
    );
    assert!(
        census.blocks() > census.measured_len() as u32,
        "each measured control program declares numbered blocks"
    );
    for row in census.rows() {
        assert!(
            !row.members.is_empty(),
            "{}: every archive declares at least one member, so the control rule \
             had candidates everywhere",
            row.mission()
        );
        assert!(
            !row.is_complete(),
            "{}: a mission with unmeasured directives — or none at all — is never \
             complete",
            row.mission()
        );
        let Some(record) = row.record() else {
            // A reader with no control program: its absence is the measurement,
            // and the row still carries the members it scanned.
            assert!(
                matches!(
                    row.program,
                    cs_app::mission_control::ControlProgram::Absent { .. }
                ),
                "{}: a row without a record is an absent program",
                row.mission()
            );
            assert!(
                row.members.iter().all(|member| !member.is_control),
                "{}: an archive with no control program marks none of its members",
                row.mission()
            );
            continue;
        };
        assert!(
            record.keys().iter().map(|key| key.sites).sum::<u32>() == record.sites(),
            "{}: every measured site belongs to exactly one key",
            row.mission()
        );
        assert!(
            record.blocks() > 0,
            "{}: the control member declares numbered blocks",
            row.mission()
        );
        assert!(
            record.refusals().is_empty(),
            "{}: the measured directive grammar parses every block: {:?}",
            row.mission(),
            record.refusals()
        );
        assert!(
            row.lowering().expect("measured").unmet().count() >= 3,
            "{}: at least three of lower_program's four requirements are unmet",
            row.mission()
        );
    }
    assert!(
        census.complete_missions().is_empty(),
        "no mission is complete, so the positive name for a releasable mission is empty"
    );
    assert!(
        !census.campaign_ready(),
        "the campaign gate stays closed while any reader declares a directive with \
         no measured effect, or carries no objective program at all"
    );

    // The readers without a control program are named, not filtered out: measured,
    // every one of them is an instant-action or multiplayer scenario.
    let absent = census.archives_without_control_program();
    assert!(
        !absent.is_empty(),
        "the installation has readers with no objective program, and the census \
         names them rather than hiding them"
    );
    for mission in &absent {
        let last = mission
            .rsplit('/')
            .next()
            .expect("a mission label has a segment");
        assert!(
            last.starts_with("ia") || last.starts_with("mp"),
            "{mission}: a mission-scoped reader with no numbered objective block is \
             measured to be an instant-action or multiplayer scenario"
        );
    }
    assert!(
        absent.len() + census.measured_len() == census.len(),
        "every mission-scoped reader is in exactly one population"
    );

    // The corpus-wide accounting: which requirement blocks which missions.
    let unmet = census.unmet_by_requirement();
    for kind in [
        LoweringRequirementKind::ObjectiveIdentity,
        LoweringRequirementKind::ObjectiveCondition,
        LoweringRequirementKind::CallArguments,
    ] {
        let missions = unmet
            .get(kind.code())
            .unwrap_or_else(|| panic!("{} is unmet somewhere", kind.code()));
        assert_eq!(
            missions.len(),
            census.measured_len(),
            "{} blocks every measured control program, so the requirement is not \
             mission-specific",
            kind.code()
        );
    }
    assert!(
        !unmet.contains_key(LoweringRequirementKind::MissionIdentity.code()),
        "the mission id is not an unmet requirement: it comes from the mission path, \
         not from the control member"
    );
    assert!(
        !census.vocabulary().is_empty()
            && census.vocabulary().len() >= census.directive_keys().len(),
        "the site totals and the distinct keys are both published"
    );
    assert!(
        census
            .unmeasured_fields()
            .iter()
            .any(|field| field.contains("IDENTITY")),
        "the corpus names the identity field it cannot read"
    );
}

/// The corpus-wide vocabulary is published beside the classification, and the
/// outcome keys are the only implemented directives anywhere in it.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_the_measured_vocabulary_is_wide_and_only_outcomes_are_implemented() {
    let census = census();
    let keys = census.directive_keys();
    assert!(
        keys.len() > 40,
        "the campaign's control programs spell a wide directive vocabulary, got {}",
        keys.len()
    );

    // Every key the census publishes has sites, and the totals reconcile.
    assert_eq!(
        census
            .vocabulary()
            .iter()
            .map(|(_, sites)| sites)
            .sum::<u32>(),
        census.sites(),
        "the per-key site totals reconcile with the corpus site total"
    );

    // The implemented set corpus-wide is exactly the two measured outcome keys.
    let mut implemented: Vec<&str> = census
        .measured_rows()
        .flat_map(|row| {
            row.record()
                .expect("a measured row carries a record")
                .implemented()
        })
        .map(|(key, _)| key.key.as_str())
        .collect();
    implemented.sort_unstable();
    implemented.dedup();
    assert_eq!(
        implemented,
        ["INSTANTLOSS", "INSTANTWIN"],
        "the only implemented directives in the whole campaign are the two outcome keys"
    );

    // And the mission-identity data the campaign supplies for every row: a
    // mission-scoped path, which is where the mission id comes from.
    for row in census.rows() {
        assert!(
            row.mission.starts_with("zbd/") && row.mission.matches('/').count() == 2,
            "{}: a mission label is exactly zbd/<group>/<mission>",
            row.mission()
        );
    }
}

/// The `.zrd` tag constants this measurement's grammar depends on are the ones the
/// production reader uses. If they ever diverged, every shape label in this stage
/// would describe a grammar the reader does not implement.
#[test]
fn accept_m01_lc_the_measured_shapes_describe_the_production_zrd_grammar() {
    assert_eq!(ZRD_TAG_INT, 1);
    assert_eq!(ZRD_TAG_FLOAT, 2);
    assert_eq!(ZRD_TAG_TEXT, 3);
    assert_eq!(ZRD_TAG_LIST, 4);

    // Round-trip the authored bytes through the production decoder, so the
    // synthetic fixtures above are read by the same reader the census uses.
    let document = authored_record();
    let bytes = encode(&document);
    let decoded = cs_content::stunts::decode_zrd(&bytes).expect("the authored record decodes");
    let record = measure_control_record(&decoded);
    assert_eq!(record.blocks(), 1);
    assert_eq!(record.sites(), 2);
    assert_eq!(record.vocabulary(), 2);
    assert_eq!(
        record.implemented().len(),
        1,
        "only INSTANTWIN is implemented in the authored record"
    );
    assert_eq!(
        record.record_fields()[0].0.key(),
        "MISSION_TIMER",
        "the authored record field is classified"
    );
}

// --------------------------------------------------------------- .zrd writer ---

/// Encodes a [`ZrdValue`] back to `.zrd` bytes, so a synthetic test exercises the
/// production decoder rather than only the walk that reads a decoded tree.
///
/// Written from the grammar `cs_content::stunts::decode_zrd` implements: a `u32`
/// tag then the payload, and a list's count is its child count **plus one**.
fn encode(value: &ZrdValue) -> Vec<u8> {
    let mut out = Vec::new();
    match value {
        ZrdValue::Int(v) => {
            out.extend_from_slice(&ZRD_TAG_INT.to_le_bytes());
            out.extend_from_slice(&v.to_le_bytes());
        }
        ZrdValue::Float(v) => {
            out.extend_from_slice(&ZRD_TAG_FLOAT.to_le_bytes());
            out.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        ZrdValue::Text(text) => {
            out.extend_from_slice(&ZRD_TAG_TEXT.to_le_bytes());
            out.extend_from_slice(&(text.len() as u32).to_le_bytes());
            out.extend_from_slice(text.as_bytes());
        }
        ZrdValue::List(children) => {
            out.extend_from_slice(&ZRD_TAG_LIST.to_le_bytes());
            out.extend_from_slice(&((children.len() + 1) as u32).to_le_bytes());
            for child in children {
                encode_into(child, &mut out);
            }
        }
    }
    out
}

/// The recursive half of [`encode`], so a nested list encodes without a second
/// allocation per level.
fn encode_into(value: &ZrdValue, out: &mut Vec<u8>) {
    match value {
        ZrdValue::List(children) => {
            out.extend_from_slice(&ZRD_TAG_LIST.to_le_bytes());
            out.extend_from_slice(&((children.len() + 1) as u32).to_le_bytes());
            for child in children {
                encode_into(child, out);
            }
        }
        other => out.extend_from_slice(&encode(other)),
    }
}
