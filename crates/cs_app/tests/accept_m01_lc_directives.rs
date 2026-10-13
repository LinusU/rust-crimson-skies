//! Acceptance for `M01-LC-DIRECTIVES` (#641): the task's four measured items
//! and its hard constraints, read back through `RetailControlCensus` and the
//! production lowering chain — the work it asked for landed as stages
//! `M01-LC-DIRECTIVE-{A,B,C,D,E,LOWERING}` and `M01-B-OBJ-4`, and this suite
//! pins the verdict each item reached rather than re-measuring `#630`'s walk.
//!
//! The task asked, in order:
//!
//! 1. `IDENTITY` — what the bare integer indexes. Measured: it is the HUD
//!    slot ordinal and the class spelling selects the completion sound
//!    channel and HUD class; block *identity* is the zero-based record index
//!    every cross-objective directive spells, so `IDENTITY` is presentation
//!    data, not the `objective_identity` candidate it was suspected to be.
//! 2. The `INACTIVE<n>` stage keys and `INACTIVE_COMPLETION_COUNT` — what a
//!    stage asserts and what the threshold counts. Measured: a member list
//!    whose handles lose the in-play bit, counted against the threshold that
//!    defaults to the block's own member count — a real `Condition`, so the
//!    `objective_condition` requirement has a candidate that lowers.
//! 3. Nested argument shapes — the IR carries `Value::List`, and the sites
//!    stay nested: `ANIM_STATE`'s `[text,[text,[text],text,[text]]]` and
//!    `SET_HELP_LABEL`'s `[[text,text],text]` arrive structure-for-structure,
//!    never flattened into a positional `Vec<Value>`.
//! 4. Disagreeing shapes — never a plurality resolved: a key registers one
//!    binding signature per measured shape and each site binds against its
//!    own, and a key no finding measures stays counted, named and refused.
//!
//! The hard constraints the task set are pinned the same way: every key that
//! cannot be honoured stays counted and named (`Unmeasured` with its reason,
//! the calls row unmet), no field is flattened, defaulted or tolerated to
//! make a program launch (a `not_a_list` site refuses and damages its block),
//! and `campaign_ready()` stays `false` while any mission carries an
//! unhonoured key — M01's own record is complete, the corpus's is not.
//!
//! Retail cases need `CS_GAME_DIR` and are `#[ignore]`d so CI runs the
//! synthetic ones. Test prefix `accept_m01_lc_directives_`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use cs_app::control_lowering::{LoweredControlRecord, lower_control_record};
use cs_app::mission_control::survey_mission_control_programs;
use cs_content::mission_control::{
    CallOutcome, ConditionOutcome, DirectiveDisposition, DirectiveOperation, DirectiveRole,
    DirectiveShape, MeasuredArg, MeasuredControlRecord, measure_control_record, measured_directive,
};
use cs_content::objectives::is_objective_inactive_stage;
use cs_content::stunts::ZrdValue;
use cs_script::bindings::RawCall;
use cs_script::ir::{Condition, Value};
use cs_types::content::{ContentId, ContentKind};

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

/// The census row label of the mission this task is about.
const M01: &str = "zbd/c1c/m01";

// ------------------------------------------------- the .zrd authoring helpers ---

fn zrd_int(value: u32) -> ZrdValue {
    ZrdValue::Int(value)
}

fn zrd_text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

fn zrd_list(children: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(children)
}

/// One authored directive of a block: the key, and — unless authored bare —
/// its argument list beside it, in the asymmetric grammar the census reads.
fn directive(key: &str, args: Vec<ZrdValue>) -> Vec<ZrdValue> {
    let mut children = vec![zrd_text(key)];
    if !args.is_empty() {
        children.push(zrd_list(args));
    }
    children
}

fn block(number: u32, directives: Vec<Vec<ZrdValue>>) -> (String, ZrdValue) {
    let mut children = Vec::new();
    for children_of_directive in directives {
        children.extend(children_of_directive);
    }
    (format!("OBJECTIVE{number}"), zrd_list(children))
}

fn control_record(fields: Vec<(String, ZrdValue)>) -> ZrdValue {
    let mut children = Vec::new();
    for (key, value) in fields {
        children.push(zrd_text(&key));
        children.push(value);
    }
    zrd_list(vec![zrd_list(children)])
}

/// Runs the production adapter over an authored record: measured, lowered and
/// bound through the record's own key dispositions, exactly as the census runs
/// it for a retail row.
fn lower(document: &ZrdValue) -> (MeasuredControlRecord, LoweredControlRecord) {
    let record = measure_control_record(document);
    let lowered = lower_control_record(
        ContentId::from_source(ContentKind::Mission, "accept-mission")
            .map_err(|error| error.to_string()),
        "accept-mission",
        document,
        &record,
    );
    (record, lowered)
}

/// The measured shape of one emitted `Value` — the mirror of the adapter's
/// `zrd_to_value`, so a call's structure can be compared field for field
/// against the shapes the census measured its key spelling.
fn shape_of_value(value: &Value) -> MeasuredArg {
    match value {
        Value::Int(_) => MeasuredArg::Int,
        Value::Float(_) => MeasuredArg::Float,
        Value::Str(_) => MeasuredArg::Text,
        Value::List(children) if children.is_empty() => MeasuredArg::Empty,
        Value::List(children) => MeasuredArg::List(children.iter().map(shape_of_value).collect()),
        other => panic!("a converted .zrd argument produced {other:?}, which no site spells"),
    }
}

/// The shape an emitted `RawCall`'s argument row spells.
fn shape_of_call(call: &RawCall) -> DirectiveShape {
    if call.args.is_empty() {
        DirectiveShape::Bare
    } else {
        DirectiveShape::Arguments(call.args.iter().map(shape_of_value).collect())
    }
}

/// Whether an emitted call's argument shape is one the census measured its
/// key spelling. Three readings agree: the call's own positional shape; a
/// bare call against a site that spelled an explicit empty list; and — for
/// the operations the adapter binds with the spelled list as one argument —
/// the single `Value::List` it wraps the spelled list in.
fn emitted_shape_is_measured(spelled: &[(DirectiveShape, u32)], emitted: &DirectiveShape) -> bool {
    if spelled.iter().any(|(shape, _)| shape == emitted) {
        return true;
    }
    match emitted {
        DirectiveShape::Arguments(args) if args.len() == 1 => match &args[0] {
            MeasuredArg::List(children) => spelled.iter().any(|(shape, _)| {
                matches!(shape, DirectiveShape::Arguments(spelled_args) if spelled_args == children)
            }),
            MeasuredArg::Empty => spelled.iter().any(|(shape, _)| {
                matches!(shape, DirectiveShape::Arguments(spelled_args) if spelled_args.is_empty())
            }),
            _ => false,
        },
        DirectiveShape::Bare => spelled.iter().any(|(shape, _)| {
            matches!(shape, DirectiveShape::Arguments(spelled_args) if spelled_args.is_empty())
        }),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Item 1: `IDENTITY` — the integer indexes the HUD slot, not the objective
// ---------------------------------------------------------------------------

/// The `objective_identity` requirement's candidate measured to be the
/// **block's own position**: `IDENTITY`'s integer is the HUD slot ordinal,
/// the class spelling picks the completion sound channel and HUD class —
/// presentation data — while every cross-objective directive addresses a
/// block by its zero-based index. The lowered `RawObjective` carries the
/// position as its `SymbolId` and the authored `OBJECTIVE<N>` number as its
/// content id, so the two cannot be confused.
#[test]
fn accept_m01_lc_directives_identity_is_presentation_and_the_block_index_is_the_identity() {
    let measured = measured_directive("IDENTITY").expect("IDENTITY is measured");
    assert_eq!(measured.operation, DirectiveOperation::PresentationIdentity);
    assert_eq!(
        measured.operation.role(),
        DirectiveRole::ObjectiveRecord,
        "a record field, not an evaluator and not a call"
    );
    assert!(
        measured.summary.contains("HUD slot ordinal"),
        "the integer's measured role is the HUD slot ordinal, not an \
         objective index: {}",
        measured.summary
    );

    // Author two blocks out of numeric order: the content id must carry the
    // authored `OBJECTIVE<N>` number while the `SymbolId` — the index the
    // cross-objective directives spell — carries the position.
    let document = control_record(vec![
        block(
            7,
            vec![
                directive("IDENTITY", vec![zrd_text("PRIMARY"), zrd_int(2)]),
                directive("INSTANTWIN", Vec::new()),
            ],
        ),
        block(2, vec![directive("INSTANTLOSS", Vec::new())]),
    ]);
    let (_, lowered) = lower(&document);
    let attempt = lowered.attempt();
    assert_eq!(attempt.objectives, 2, "one RawObjective per numbered block");
    let raw = lowered.raw_program().expect("the program assembled");
    for (index, objective) in raw.objectives.iter().enumerate() {
        assert_eq!(
            objective.id.0, index as u32,
            "the block's identity in the program is its zero-based position"
        );
    }
    assert_eq!(
        raw.objectives[0].content.key(),
        "accept-mission.objective7",
        "the authored OBJECTIVE7 number names the content id"
    );
    assert_eq!(
        raw.objectives[1].content.key(),
        "accept-mission.objective2",
        "the authored OBJECTIVE2 number names the content id"
    );
}

// ---------------------------------------------------------------------------
// Item 2: the inactive stages and their completion-count threshold
// ---------------------------------------------------------------------------

/// A stage asserts that its listed member's in-play bit cleared, and the
/// threshold counts cleared members in units of members — the measured
/// `objective_condition` candidate, lowered by `lower_record` into the
/// side-effect-free `Condition` the pass-2 evaluator runs.
#[test]
fn accept_m01_lc_directives_inactive_stages_and_their_threshold_are_the_measured_evaluator() {
    for stage in [1u32, 2, 9, 18, 42, 99, 100] {
        let directive =
            measured_directive(&format!("INACTIVE{stage}")).expect("a stage key is measured");
        assert_eq!(directive.operation, DirectiveOperation::InactiveMembers);
        assert_eq!(
            directive.operation.role(),
            DirectiveRole::CompletionCondition,
            "INACTIVE{stage} feeds the block's completion predicate"
        );
    }
    // Outside the parser's measured `INACTIVE%d` loop: `d` in 1..=100 and the
    // digits spell `d` — `INACTIVE01` is not `INACTIVE1`.
    for key in ["INACTIVE0", "INACTIVE101", "INACTIVE01", "INACTIVE"] {
        assert_eq!(
            measured_directive(key),
            None,
            "{key}: outside the measured stage range, so unmeasured"
        );
    }
    let threshold =
        measured_directive("INACTIVE_COMPLETION_COUNT").expect("the threshold key is measured");
    assert_eq!(threshold.operation, DirectiveOperation::InactiveThreshold);
    assert!(
        threshold.summary.contains("defaults to all"),
        "the measured default is the block's own member count: {}",
        threshold.summary
    );

    // Block 1 spells two member lists beside an explicit threshold of two;
    // block 2 spells one member and no threshold, so the measured default —
    // the block's own member count — applies.
    let document = control_record(vec![
        block(
            1,
            vec![
                directive("INACTIVE1", vec![zrd_text("workersvoyagezep")]),
                directive("INACTIVE2", vec![zrd_text("piratezep"), zrd_text("turret")]),
                directive("INACTIVE_COMPLETION_COUNT", vec![zrd_int(2)]),
            ],
        ),
        block(
            2,
            vec![directive(
                "INACTIVE1",
                vec![zrd_text("a"), zrd_text("b"), zrd_text("c")],
            )],
        ),
    ]);
    let (_, lowered) = lower(&document);
    let attempt = lowered.attempt();
    assert_eq!(
        attempt.conditions.len(),
        2,
        "one condition verdict per block"
    );
    for outcome in &attempt.conditions {
        assert!(
            matches!(outcome, ConditionOutcome::Lowered),
            "the stage evaluator lowered: {outcome:?}"
        );
    }
    let raw = lowered.raw_program().expect("the program assembled");
    let members_clause = |index: usize, members_want: usize, threshold_want: u32| {
        let Condition::All(gate) = &raw.objectives[index].condition else {
            panic!(
                "a gated evaluator, not {:?}",
                raw.objectives[index].condition
            )
        };
        assert!(
            gate.iter().any(|condition| matches!(
                condition,
                Condition::InactiveMembers { members, threshold }
                    if members.len() == members_want && *threshold == threshold_want
            )),
            "block {index}: members {members_want} against threshold {threshold_want}: {gate:?}"
        );
    };
    members_clause(0, 2, 2);
    // One member chain of three name parts, threshold defaulted to the
    // member count — not the chain's own length.
    members_clause(1, 1, 1);
}

// ---------------------------------------------------------------------------
// Item 3: nested argument shapes stay nested inside `Value::List`
// ---------------------------------------------------------------------------

/// `ANIM_STATE` spells `[text,[text,[text],text,[text]]]` and `SET_HELP_LABEL`
/// spells `[[text,text],text]` — argument shapes `cs_script::ir::Value`
/// could not carry before the task's measured list variant. The emitted
/// calls keep every level: the operand-list operation's call carries the
/// spelled list as one `Value::List`, and each list inside it stays a nested
/// `Value::List`, so the structure the site spelled is the structure the
/// host sees — nothing is flattened into a positional row.
#[test]
fn accept_m01_lc_directives_nested_argument_shapes_stay_nested_inside_value_list() {
    // M01's own spellings: `ANIM_STATE` with its `[text,[text,[text],text,[text]]]`
    // operand and `SET_HELP_LABEL`'s `[[text,text],text]` name pair.
    let document = control_record(vec![block(
        1,
        vec![
            directive(
                "ANIM_STATE",
                vec![
                    zrd_text("ANIM"),
                    zrd_list(vec![
                        zrd_text("obj"),
                        zrd_list(vec![zrd_text("node")]),
                        zrd_text("EXECUTED"),
                        zrd_list(vec![zrd_text("anim")]),
                    ]),
                ],
            ),
            directive(
                "SET_HELP_LABEL",
                vec![
                    zrd_list(vec![zrd_text("node"), zrd_text("sub")]),
                    zrd_text(" "),
                ],
            ),
        ],
    )]);
    let (record, lowered) = lower(&document);
    for key in ["ANIM_STATE", "SET_HELP_LABEL"] {
        assert!(
            matches!(
                record.key(key).expect("spelled").disposition(),
                DirectiveDisposition::Measured(_)
            ),
            "{key}: the nested shape is measured, not a no-value refusal"
        );
    }
    let raw = lowered.raw_program().expect("the program assembled");
    let calls = &raw.objectives[0].calls;

    // `ANIM_STATE` is an operand-list operation: the call's one argument is
    // the spelled list as a `Value::List`, with the descriptor pair's own
    // two lists still nested inside it.
    let anim = calls
        .iter()
        .find(|call| call.name == "ANIM_STATE")
        .expect("the ANIM_STATE site reached a call");
    let [Value::List(operands)] = anim.args.as_slice() else {
        panic!(
            "ANIM_STATE carries its operand list as one argument: {:?}",
            anim.args
        )
    };
    assert_eq!(operands.len(), 2, "the operand list's own arity");
    assert_eq!(operands[0], Value::Str("ANIM".to_owned()));
    let Value::List(descriptor) = &operands[1] else {
        panic!("the descriptor pair stays a nested list: {:?}", operands[1])
    };
    assert_eq!(descriptor.len(), 4);
    assert!(
        matches!(descriptor[1], Value::List(_)) && matches!(descriptor[3], Value::List(_)),
        "the pair's own nested lists stay nested: {descriptor:?}"
    );

    // `SET_HELP_LABEL`'s `[[text,text],text]` is positional: its first
    // argument is still the nested pair — flattened, the row would be four.
    let label = calls
        .iter()
        .find(|call| call.name == "SET_HELP_LABEL")
        .expect("the SET_HELP_LABEL site reached a call");
    assert_eq!(
        label.args.len(),
        2,
        "the spelled arity, not a flattened row"
    );
    assert_eq!(
        label.args[0],
        Value::List(vec![
            Value::Str("node".to_owned()),
            Value::Str("sub".to_owned())
        ]),
        "the nested name pair arrives as one nested argument"
    );
    assert_eq!(label.args[1], Value::Str(" ".to_owned()));

    // Both emitted shapes are measured shapes of their keys — the structural
    // correspondence every call must satisfy.
    for call in calls {
        let emitted = shape_of_call(call);
        let spelled = &record
            .key(&call.name)
            .expect("the call's key is in the record")
            .shapes;
        assert!(
            emitted_shape_is_measured(spelled, &emitted),
            "`{}`: the emitted shape {emitted:?} is none of the measured \
             shapes {spelled:?}",
            call.name
        );
    }
    assert_eq!(
        lowered.attempt().validation.as_deref(),
        Some(&[][..]),
        "the nested record validates: {:?}",
        lowered.attempt().validation
    );
}

// ---------------------------------------------------------------------------
// Item 4: disagreeing shapes — every spelled form binds, no plurality resolves
// ---------------------------------------------------------------------------

/// `INACTIVE1` spells `[text]`, `[text,text]` and `[text,text,text]` across
/// the corpus — a member list of one, two or three names, every form inside
/// the measured semantics. The registry registers **one signature per
/// measured shape** and each site binds against its own; authoring two
/// shapes in one record leaves both standing, and nothing picks a plurality.
#[test]
fn accept_m01_lc_directives_disagreeing_sites_bind_their_own_signature_never_a_plurality() {
    let document = control_record(vec![
        block(
            1,
            vec![directive("INACTIVE1", vec![zrd_text("one_member")])],
        ),
        block(
            2,
            vec![directive(
                "INACTIVE1",
                vec![zrd_text("a"), zrd_text("b"), zrd_text("c")],
            )],
        ),
    ]);
    let (record, lowered) = lower(&document);
    let key = record.key("INACTIVE1").expect("spelled twice");
    assert_eq!(
        key.shapes.len(),
        2,
        "both spelled shapes are kept: {:?}",
        key.shapes
    );
    assert!(
        matches!(key.disposition(), DirectiveDisposition::Measured(_)),
        "a measured key stays measured while its own sites disagree"
    );
    let spec = lowered
        .registry()
        .get("INACTIVE1")
        .expect("a measured key registers");
    assert_eq!(
        spec.signatures.len(),
        2,
        "one signature per measured shape — no plurality resolved"
    );
    let bound = lowered
        .attempt()
        .calls
        .iter()
        .filter(|outcome| matches!(outcome, CallOutcome::Bound))
        .count();
    assert_eq!(bound, 2, "each site bound against its own signature");
    assert_eq!(
        lowered.attempt().validation.as_deref(),
        Some(&[][..]),
        "the disagreeing record validates"
    );
}

// ---------------------------------------------------------------------------
// The hard constraints
// ---------------------------------------------------------------------------

/// **Counted and named, never bound to a convenient operation.** A key no
/// finding covers keeps its `Unmeasured` disposition, its site refuses the
/// bind as an `unknown host call`, the calls row names it — and the corpus's
/// stray English words are counted beside it, exactly like a real key.
#[test]
fn accept_m01_lc_directives_a_key_no_finding_covers_stays_counted_and_named() {
    let document = control_record(vec![block(
        1,
        vec![
            directive("WAKEUP_OBJECTIVE_WHEN_I_COMPLETE", vec![zrd_int(3)]),
            vec![zrd_text("Change")],
            directive("INSTANTWIN", Vec::new()),
        ],
    )]);
    let (record, lowered) = lower(&document);
    assert_eq!(record.sites(), 3, "every spelled site is counted");
    for name in ["WAKEUP_OBJECTIVE_WHEN_I_COMPLETE", "Change"] {
        let key = record.key(name).expect("counted");
        assert_eq!(key.sites, 1);
        assert!(
            matches!(key.disposition(), DirectiveDisposition::Unmeasured { .. }),
            "{name}: counted, named and refused"
        );
    }
    let attempt = lowered.attempt();
    let refused: Vec<&str> = attempt
        .calls
        .iter()
        .filter_map(|outcome| match outcome {
            CallOutcome::Refused(reason) => Some(reason.as_str()),
            CallOutcome::Bound => None,
        })
        .collect();
    assert!(
        refused
            .iter()
            .any(|reason| reason.contains("WAKEUP_OBJECTIVE_WHEN_I_COMPLETE")),
        "the unmeasured key's site refuses by name: {refused:?}"
    );
    assert!(
        refused.iter().any(|reason| reason.contains("Change")),
        "the stray word's site refuses by name: {refused:?}"
    );
    // A refusal is per site, never a blanket on its block: the terminal
    // spelling still binds.
    assert!(
        attempt
            .calls
            .iter()
            .any(|outcome| matches!(outcome, CallOutcome::Bound)),
        "the outcome site still bound: {:?}",
        attempt.calls
    );
    let unmeasured_fields = record.lowering(attempt).unmeasured_fields();
    assert!(
        unmeasured_fields
            .iter()
            .any(|field| field.contains("WAKEUP_OBJECTIVE_WHEN_I_COMPLETE")),
        "the lowering rows name the unhonoured key: {unmeasured_fields:?}"
    );
    assert!(!record.is_complete(attempt));
}

/// **No field is flattened, defaulted or tolerated to make a program
/// launch.** A scalar where the grammar spells a list is the `not_a_list`
/// defect: the site emits no `RawCall` (nothing is defaulted into one), the
/// block's condition verdict is refused because a partially-represented
/// directive list is not the record's predicate, and the lowered objective
/// carries `Condition::Unknown`, which `MissionProgram::validate` refuses —
/// never a guessed predicate standing in.
#[test]
fn accept_m01_lc_directives_no_field_is_flattened_defaulted_or_tolerated() {
    let mut children = vec![zrd_text("WAKE_ANIM"), zrd_int(9)];
    children.extend(directive("INSTANTWIN", Vec::new()));
    let document = control_record(vec![("OBJECTIVE1".to_owned(), zrd_list(children))]);
    let (record, lowered) = lower(&document);
    let key = record.key("WAKE_ANIM").expect("counted");
    assert_eq!(
        key.shapes,
        vec![(DirectiveShape::NotAList, 1)],
        "the scalar follower is the measured not_a_list shape: {:?}",
        key.shapes
    );
    let attempt = lowered.attempt();
    let refused = attempt.calls.iter().find_map(|outcome| match outcome {
        CallOutcome::Refused(reason) if reason.contains("not_a_list") => Some(reason.clone()),
        _ => None,
    });
    assert!(
        refused.is_some(),
        "the scalar site is refused by name, not carried: {:?}",
        attempt.calls
    );
    assert!(
        attempt.conditions.iter().any(
            |outcome| matches!(outcome, ConditionOutcome::Refused(reason) if reason.contains("not_a_list"))
        ),
        "the damaged block's condition refuses: {:?}",
        attempt.conditions
    );
    // The defect reaches the program as `Condition::Unknown`, which
    // validation refuses — never a guessed predicate standing in.
    let raw = lowered.raw_program().expect("the program assembled");
    assert!(
        matches!(raw.objectives[0].condition, Condition::Unknown { .. }),
        "no guessed predicate stands in for the damaged block: {:?}",
        raw.objectives[0].condition
    );
}

// ---------------------------------------------------------------------------
// The retail record: all four items measured, every site honoured or named
// ---------------------------------------------------------------------------

/// M01's whole directive vocabulary carries a measured disposition or a
/// terminal outcome: the four items' keys are each measured — `IDENTITY`,
/// every `INACTIVE<n>` stage and its threshold, the nested-shape keys and
/// every disagreeing key — and no M01 key is `Unmeasured`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directives_m01s_four_items_are_all_measured() {
    let census =
        survey_mission_control_programs(&game_dir()).expect("the census runs on the installation");
    let record = census
        .row(M01)
        .expect("M01 is measured")
        .record()
        .expect("M01 declares a control program");

    // Item 1.
    let identity = record.key("IDENTITY").expect("M01 spells IDENTITY");
    assert!(identity.sites > 0);
    match identity.disposition() {
        DirectiveDisposition::Measured(directive) => assert_eq!(
            directive.operation,
            DirectiveOperation::PresentationIdentity
        ),
        other => panic!("IDENTITY is measured: {other:?}"),
    }

    // Item 2.
    let stages: Vec<&str> = record
        .keys()
        .iter()
        .filter(|key| is_objective_inactive_stage(&key.key))
        .map(|key| key.key.as_str())
        .collect();
    assert!(!stages.is_empty(), "M01 spells inactive stage keys");
    for stage in &stages {
        match record.key(stage).expect("spelled").disposition() {
            DirectiveDisposition::Measured(directive) => assert_eq!(
                directive.operation,
                DirectiveOperation::InactiveMembers,
                "{stage} is the measured member-count evaluator"
            ),
            other => panic!("{stage} is measured: {other:?}"),
        }
    }
    match record
        .key("INACTIVE_COMPLETION_COUNT")
        .expect("M01 spells the threshold")
        .disposition()
    {
        DirectiveDisposition::Measured(directive) => {
            assert_eq!(directive.operation, DirectiveOperation::InactiveThreshold)
        }
        other => panic!("the threshold is measured: {other:?}"),
    }

    // Item 3: every key whose measured shape nests a list is measured, and
    // the nested shapes are still the record's shapes.
    let nested: Vec<&str> = record
        .keys()
        .iter()
        .filter(|key| {
            key.shapes.iter().any(|(shape, _)| match shape {
                DirectiveShape::Arguments(args) => {
                    args.iter().any(|arg| matches!(arg, MeasuredArg::List(_)))
                }
                _ => false,
            })
        })
        .map(|key| key.key.as_str())
        .collect();
    assert!(
        nested.contains(&"ANIM_STATE") && nested.contains(&"SET_HELP_LABEL"),
        "M01's nested-shape keys: {nested:?}"
    );
    for key in &nested {
        assert!(
            matches!(
                record.key(key).expect("spelled").disposition(),
                DirectiveDisposition::Measured(_)
            ),
            "{key}: the nested shape is carried, not refused"
        );
    }

    // Item 4: every key whose M01 sites disagree is measured — the
    // disagreement is arity, and each form binds its own signature.
    let disagreeing: Vec<&str> = record
        .keys()
        .iter()
        .filter(|key| key.shapes.len() > 1)
        .map(|key| key.key.as_str())
        .collect();
    for key in &disagreeing {
        assert!(
            matches!(
                record.key(key).expect("spelled").disposition(),
                DirectiveDisposition::Measured(_)
            ),
            "{key}: disagreeing shapes stay measured in M01"
        );
    }
    assert!(
        record.unmeasured().is_empty(),
        "M01 spells no unmeasured key: {:?}",
        record
            .unmeasured()
            .iter()
            .map(|(key, _)| key.key.as_str())
            .collect::<Vec<_>>()
    );
}

/// M01's record lowers completely into the mission IR: every requirement row
/// met by the record's own attempt, one `RawObjective` per numbered block
/// keyed on the zero-based index, one lowered `Condition` per block, one
/// bound call per directive site and a program `MissionProgram::validate`
/// accepts — while `campaign_ready()` stays closed on the corpus.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directives_m01_lowers_completely_and_every_site_is_honoured() {
    let census =
        survey_mission_control_programs(&game_dir()).expect("the census runs on the installation");
    let row = census.row(M01).expect("M01 is measured");
    let record = row.record().expect("M01 declares a control program");
    let lowering = row.lowering().expect("a measured row lowers");
    assert!(
        lowering.complete() && lowering.unmet().count() == 0,
        "every requirement row is met by the attempt: {:?}",
        lowering.unmet().map(|row| row.label()).collect::<Vec<_>>()
    );
    assert!(row.is_complete());

    let lowered = row.lowering_attempt().expect("a measured row lowers");
    let attempt = lowered.attempt();
    assert_eq!(
        (attempt.objectives, attempt.conditions.len() as u32),
        (record.blocks(), record.blocks()),
        "one objective and one condition verdict per numbered block"
    );
    assert!(
        attempt
            .conditions
            .iter()
            .all(|outcome| matches!(outcome, ConditionOutcome::Lowered)),
        "every block lowered a side-effect-free predicate: {:?}",
        attempt.conditions
    );
    assert_eq!(
        attempt.calls.len() as u32,
        record.sites(),
        "one call verdict per directive site"
    );
    assert!(
        attempt
            .calls
            .iter()
            .all(|outcome| matches!(outcome, CallOutcome::Bound)),
        "every M01 site bound: {:?}",
        attempt
            .calls
            .iter()
            .filter(|outcome| !matches!(outcome, CallOutcome::Bound))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        attempt.validation.as_deref(),
        Some(&[][..]),
        "the lowered program validates"
    );

    // The measured identity rule, concrete: the `SymbolId` is the block's
    // zero-based position and the content id carries the authored number.
    let raw = lowered.raw_program().expect("every site bound");
    assert_eq!(raw.objectives.len() as u32, record.blocks());
    for (index, objective) in raw.objectives.iter().enumerate() {
        assert_eq!(objective.id.0, index as u32);
        assert!(
            objective.content.key().contains(".objective"),
            "the authored block number names the content id: {}",
            objective.content.key()
        );
    }

    assert!(
        census.complete_missions().contains(&M01),
        "M01's attempt validated: {:?}",
        census.complete_missions()
    );
    assert!(
        !census.campaign_ready(),
        "the corpus gate stays closed on the missions that still spell \
         unhonoured keys"
    );
}

/// Every call M01's lowered `RawProgram` carries is shaped exactly like one
/// of the shapes the census measured its key spelling: a `Value::List`
/// argument is the nested list the site spelled (the operand-list keys wrap
/// it as one argument), a positional row is the spelled row, and no emitted
/// shape is outside the key's own measured set — the never-flattened and
/// no-plurality constraints, read back off the program rather than asserted
/// from the walk.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directives_every_emitted_call_matches_a_measured_shape_of_its_key() {
    let census =
        survey_mission_control_programs(&game_dir()).expect("the census runs on the installation");
    let row = census.row(M01).expect("M01 is measured");
    let record = row.record().expect("M01 declares a control program");
    let lowered = row.lowering_attempt().expect("a measured row lowers");
    let raw = lowered.raw_program().expect("every site bound");

    // The keys whose measured operation takes the spelled list as the call's
    // one argument — `control_lowering`'s `takes_list_argument` set, mirrored
    // so the correspondence is checked against the same rule.
    let list_argument: BTreeSet<DirectiveOperation> = [
        DirectiveOperation::WakeObjectives,
        DirectiveOperation::SleepObjectives,
        DirectiveOperation::KillObjectives,
        DirectiveOperation::WakeObjectivesOnTransition,
        DirectiveOperation::AnimationStates,
        DirectiveOperation::AssignNet,
    ]
    .into_iter()
    .collect();

    let mut calls = 0usize;
    let mut list_args_seen: BTreeSet<String> = BTreeSet::new();
    let mut nested_seen: BTreeSet<String> = BTreeSet::new();
    for objective in &raw.objectives {
        for call in &objective.calls {
            calls += 1;
            let key = record
                .key(&call.name)
                .expect("the call's key is in the record");
            let emitted = shape_of_call(call);
            assert!(
                emitted_shape_is_measured(&key.shapes, &emitted),
                "`{}`: the emitted shape {emitted:?} is none of the measured \
                 shapes {:?} — a flattened or defaulted argument would land \
                 here",
                call.name,
                key.shapes
            );

            // The keys measured to nest lists really carry them in the
            // program: a `Value::List` argument where the spelled list sat
            // (`SET_HELP_LABEL`'s name pair), and — for `ANIM_STATE`'s
            // `[text,[text,[text],text,[text]]]` — a list inside the list.
            if call.args.iter().any(|arg| matches!(arg, Value::List(_))) {
                list_args_seen.insert(call.name.clone());
            }
            if call.args.iter().any(|arg| {
                matches!(arg, Value::List(children)
                    if children.iter().any(|child| matches!(child, Value::List(_))))
            }) {
                nested_seen.insert(call.name.clone());
            }
            let is_list_argument = matches!(
                key.disposition(),
                DirectiveDisposition::Measured(directive)
                    if list_argument.contains(&directive.operation)
            );
            if is_list_argument && !call.args.is_empty() {
                assert!(
                    call.args.len() == 1 && matches!(call.args[0], Value::List(_)),
                    "`{}`: the operand list is the call's one argument, not a \
                     positional row: {:?}",
                    call.name,
                    call.args
                );
            }
        }
    }
    assert_eq!(
        calls as u32,
        record.sites(),
        "every site reached the program: none was dropped"
    );
    for name in ["ANIM_STATE", "SET_HELP_LABEL", "ADD_OBJECTIVE_TARGET"] {
        assert!(
            list_args_seen.contains(name),
            "{name}: a spelled list reached the program as a `Value::List` \
             argument: {list_args_seen:?}"
        );
    }
    assert!(
        nested_seen.contains("ANIM_STATE"),
        "ANIM_STATE: the descriptor pair's own lists stayed nested: \
         {nested_seen:?}"
    );
}

/// The corpus gate is honest: `campaign_ready()` stays `false` while any
/// mission spells a key no finding covers, and every unhonoured key stays
/// **counted and named** — in its record's vocabulary and in the unmet
/// requirement rows the census's `unmet_by_requirement` index groups.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directives_the_corpus_gate_names_every_unhonoured_key() {
    let census =
        survey_mission_control_programs(&game_dir()).expect("the census runs on the installation");
    assert!(!census.campaign_ready());

    // Every unmeasured key of every row is counted (in `record.keys()`),
    // named and refused — never silently dropped and never bound.
    let mut unmeasured: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in census.measured_rows() {
        let record = row.record().expect("a measured row carries a record");
        for (key, disposition) in record.unmeasured() {
            let reason = disposition
                .refusal()
                .expect("an unmeasured disposition carries a reason");
            assert!(
                !reason.code().is_empty() && !reason.detail().is_empty(),
                "{}: the refusal names why",
                key.key
            );
            unmeasured
                .entry(key.key.clone())
                .or_default()
                .push(row.mission().to_owned());
        }
    }
    // The corpus-wide spellings the findings leave unhonoured — each still
    // counted and named, exactly as the hard constraint demands.
    for key in ["WAKEUP_OBJECTIVE_WHEN_I_COMPLETE", "SET_AI_"] {
        assert!(
            unmeasured.contains_key(key),
            "{key}: still spelled and still refused by name across the \
             corpus: {:?}",
            unmeasured.keys().collect::<Vec<_>>()
        );
    }

    // The gate's own accounting names the requirement rows the unhonoured
    // keys block.
    let unmet = census.unmet_by_requirement();
    assert!(
        unmet.contains_key("call_arguments"),
        "the corpus gate's unmet rows name the requirement: {unmet:?}"
    );
}
