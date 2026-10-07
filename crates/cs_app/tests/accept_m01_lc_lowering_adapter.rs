//! Acceptance for `M01-LC-DIRECTIVE-LOWERING.03` (#726): the adapter that
//! lowers a measured control record into a `cs_script::bindings::RawProgram`,
//! and the `ControlLowering` rows derived from that attempt.
//!
//! Task key `M01-LC-DIRECTIVE-LOWERING.03`; test prefix
//! `accept_m01_lc_lowering_adapter_`. Shared contract:
//! `docs/contracts/SCRIPT-MISSION.md` ("Source adapter acceptance"). Stages .01
//! and .02 landed the `cs_script` halves (binding variants and the
//! side-effect-free block-condition lowering); this stage is the cs_app adapter
//! that joins them: [`cs_app::control_lowering::lower_control_record`] walks a
//! record's own document into `RawBlock`s and `RawCall`s, binds every site
//! through a registry built from the record's dispositions, and reports the
//! attempt as a [`LoweringAttempt`], which is the only thing the lowering rows
//! may read.
//!
//! # What "lowered" means here — and what it does not
//!
//! A bound call emits `Action::Directive { operation, args }`: the measured
//! operation names what the original is measured to do, and carrying it to the
//! host is the action's whole effect. It is **not** evidence the world-side
//! handler exists — `Measured` is not implemented — and the two terminal keys
//! still bind `Lowering::Finish` only as a reading of their names. M01
//! reporting `is_complete` means the record lowered into a validated
//! `MissionProgram` with no refusals; it does not mean the mission is
//! playable.
//!
//! The retail test needs `CS_GAME_DIR`; the synthetic ones author `.zrd` values
//! tag by tag and exercise the same production adapter the census runs.

use std::path::PathBuf;

use cs_app::control_lowering::{LoweredControlRecord, lower_control_record};
use cs_app::mission_control::survey_mission_control_programs;
use cs_content::mission_control::{
    CallOutcome, ConditionOutcome, DirectiveDisposition, LoweringAttempt, LoweringRequirementKind,
    MeasuredControlRecord, measure_control_record,
};
use cs_content::stunts::ZrdValue;
use cs_script::bindings::Lowering;
use cs_script::ir::{Action, Condition, DirectiveOperation, Outcome, Value};
use cs_types::content::{ContentId, ContentKind};

const M01: &str = "zbd/c1c/m01";

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

// ------------------------------------------------- the .zrd authoring helpers ---

fn zrd_int(value: u32) -> ZrdValue {
    ZrdValue::Int(value)
}

fn zrd_float(value: f32) -> ZrdValue {
    ZrdValue::Float(value)
}

fn zrd_text(text: &str) -> ZrdValue {
    ZrdValue::Text(text.to_owned())
}

fn zrd_list(children: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(children)
}

/// One authored directive of a block: the key, and — unless authored bare — its
/// argument list beside it, flattened into the block's children the way the
/// measured grammar reads them.
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

/// Runs the production adapter over an authored record: a fixed synthetic
/// mission id stands in for the campaign-layout derivation the census performs,
/// so these tests pin what the attempt produced rather than which mission the
/// record was filed under.
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

/// The row for one requirement kind, or a test failure naming the row that is
/// missing.
fn row_of(
    lowering: &cs_content::mission_control::ControlLowering,
    kind: LoweringRequirementKind,
) -> &cs_content::mission_control::LoweringRequirement {
    lowering
        .requirements()
        .iter()
        .find(|row| row.kind == kind)
        .unwrap_or_else(|| panic!("the {} row exists", kind.code()))
}

// ---------------------------------------------------------------------------
// The retail acceptance
// ---------------------------------------------------------------------------

/// **M01 lowers completely.** The task's acceptance figure: 58 blocks, 353
/// directive sites, every call bound and every condition lowered, the program
/// validating — so `objective_condition` and `call_arguments` report `met`
/// because the attempt did the work, not because the vocabulary was known.
///
/// The mission id is the campaign layout's derivation (`mission/ch1-m01`), and
/// every `RawObjective` carries the content id of its own block's authored
/// number while its `SymbolId` is the zero-based record index — the index
/// `ObjectiveAwake` and `TICK_DEPENDS_ON_OBJ` share.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_lowering_adapter_m01_lowers_into_a_validated_raw_program() {
    let census =
        survey_mission_control_programs(&game_dir()).expect("the census runs on the installation");
    let row = census.row(M01).expect("M01 is measured");
    let record = row.record().expect("M01 declares a control program");
    let lowered = row
        .lowering_attempt()
        .expect("a measured row carries the lowering attempt");
    let attempt = lowered.attempt();

    assert_eq!(
        attempt.mission.as_deref(),
        Ok("mission/ch1-m01"),
        "the campaign-layout derivation entered the `RawProgram`"
    );
    assert_eq!(
        attempt.objectives,
        record.blocks(),
        "one `RawObjective` per numbered block"
    );
    assert_eq!(record.blocks(), 58);
    assert_eq!(record.sites(), 353);
    assert_eq!(attempt.conditions.len(), 58);
    assert!(
        attempt
            .conditions
            .iter()
            .all(|outcome| matches!(outcome, ConditionOutcome::Lowered)),
        "every block's condition lowered: {:?}",
        attempt.conditions
    );
    assert_eq!(attempt.calls.len(), 353);
    assert!(
        attempt
            .calls
            .iter()
            .all(|outcome| matches!(outcome, CallOutcome::Bound)),
        "every directive site bound a call: {:?}",
        attempt.calls
    );
    assert!(
        attempt.unbound_keys.is_empty(),
        "every measured key registered: {:?}",
        attempt.unbound_keys
    );
    assert_eq!(
        attempt.validation.as_deref(),
        Some(&[][..]),
        "the lowered program passes `MissionProgram::validate`: {:?}",
        attempt.validation
    );
    assert!(
        lowered.binding_errors().is_empty(),
        "no per-site binding error: {:?}",
        lowered.binding_errors()
    );

    let raw = lowered.raw_program().expect("the program assembled");
    assert_eq!(raw.objectives.len(), 58);
    assert_eq!(raw.mission.to_string(), "mission/ch1-m01");
    let calls: usize = raw.objectives.iter().map(|o| o.calls.len()).sum();
    assert_eq!(calls, 353, "one `RawCall` per directive site");
    for (index, objective) in raw.objectives.iter().enumerate() {
        assert_eq!(
            objective.id.0, index as u32,
            "the symbol id is the zero-based record index"
        );
        assert!(
            objective
                .content
                .to_string()
                .starts_with("objective/ch1-m01.objective"),
            "the content id carries the block's own number: {}",
            objective.content
        );
    }
    // Nested lists arrive nested: a measured `SET_AI_NET`/`ANIM_STATE`-shape
    // site carries a `Value::List` argument, never a flattened positional row.
    let nested: Vec<&[Value]> = raw
        .objectives
        .iter()
        .flat_map(|objective| objective.calls.iter())
        .filter(|call| call.args.iter().any(|arg| matches!(arg, Value::List(_))))
        .map(|call| call.args.as_slice())
        .collect();
    assert_eq!(
        nested.len(),
        14,
        "the sites M01 spells with a nested list argument: {:?}",
        nested
    );

    let program = lowered.program().expect("every call bound");
    assert_eq!(program.objectives.len(), 58);
    assert_eq!(
        program
            .objectives
            .iter()
            .map(|objective| objective.actions.len())
            .sum::<usize>(),
        353,
        "one action per bound call"
    );
    let (finishes, directives, other): (usize, usize, usize) = program
        .objectives
        .iter()
        .flat_map(|objective| objective.actions.iter())
        .fold((0, 0, 0), |(f, d, o), action| match action {
            Action::Finish(_) => (f + 1, d, o),
            Action::Directive { .. } => (f, d + 1, o),
            _ => (f, d, o + 1),
        });
    assert_eq!(
        (finishes, directives, other),
        (2, 351, 0),
        "INSTANTWIN/INSTANTLOSS bind `Finish`; every other site emits its measured \
         `Directive` operation — an emission on the host log, not a claim the \
         world-side handler exists"
    );
    assert!(
        program
            .objectives
            .iter()
            .flat_map(|objective| objective.actions.iter())
            .any(|action| matches!(
                action,
                Action::Directive {
                    operation: DirectiveOperation::AssignNet,
                    args,
                } if args.iter().any(|arg| matches!(arg, Value::List(_)))
            )),
        "a nested `SET_AI_NET` site arrives as a directive action with its list intact"
    );
    assert!(
        row.is_complete() && record.is_complete(attempt),
        "M01's record is complete: the attempt produced a validated program"
    );
    let lowering = row.lowering().expect("a measured row lowers");
    assert!(
        lowering.complete() && lowering.unmet().count() == 0,
        "every requirement row is met"
    );
}

/// The census-side view: `complete_missions` reports exactly the measured rows
/// whose attempts validated, and the campaign gate stays closed while any row
/// still spells an unmeasured key.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_lowering_adapter_the_census_reports_complete_rows_and_keeps_the_gate() {
    let census =
        survey_mission_control_programs(&game_dir()).expect("the census runs on the installation");

    // The rows whose record lowered completely: every one is a measured record
    // with no unmeasured key, no refusal and no unmet row — a stale membership
    // fails here rather than drifting with the data.
    let complete = census.complete_missions();
    for mission in &complete {
        let row = census.row(mission).expect("a complete mission is a row");
        let record = row.record().expect("a complete row is measured");
        assert!(row.is_complete(), "{mission}: the row reports complete");
        assert!(
            record.unmeasured().is_empty(),
            "{mission}: no unmeasured key"
        );
        assert!(record.refusals().is_empty(), "{mission}: every block read");
        assert_eq!(
            row.lowering().expect("measured").unmet().count(),
            0,
            "{mission}: no unmet lowering row"
        );
    }
    assert!(complete.contains(&M01), "M01 is among them: {complete:?}");

    for row in census.measured_rows() {
        let lowering = row.lowering().expect("a measured row lowers");
        if row.is_complete() {
            continue;
        }
        // An incomplete measured row fails closed with named fields: an
        // unmet row that named nothing would be the failure this accounting
        // exists to prevent.
        assert!(
            lowering.unmet().count() > 0,
            "{}: incomplete means an unmet row",
            row.mission()
        );
        assert!(
            !lowering.unmeasured_fields().is_empty(),
            "{}: every unmet row names its fields",
            row.mission()
        );
    }
    assert!(
        !census.campaign_ready(),
        "the campaign gate stays closed while any row is incomplete"
    );

    // **Only where they should.** The launch surface flips for M01 and for no
    // row that still spells a directive no finding covers: the corpus must
    // hold such a row, or this assertion proves nothing about the gate.
    assert!(row_is_complete(&census, M01), "M01's surface flips to true");
    let unmeasured_rows: Vec<&str> = census
        .measured_rows()
        .filter(|row| {
            row.record()
                .is_some_and(|record| !record.unmeasured().is_empty())
        })
        .map(|row| row.mission())
        .collect();
    assert!(
        !unmeasured_rows.is_empty(),
        "the corpus spells at least one key no finding covers, so the gate has \
         something to hold closed"
    );
    for mission in &unmeasured_rows {
        assert!(
            !census.row(mission).expect("a measured row is a row").is_complete(),
            "{mission}: a row whose directives include an unmeasured key stays \
             incomplete"
        );
    }

    // The two populations are named by the census itself: the readers that
    // declare no control program at all are part of the denominator the gate
    // fails on, not rows that quietly disappeared.
    let absent = census.archives_without_control_program();
    assert_eq!(
        absent.len(),
        13,
        "13 of the mission-scoped readers declare no control program at all"
    );
    assert_eq!(
        census.len(),
        53,
        "the installation has 53 mission-scoped readers"
    );
    assert_eq!(
        absent.len() + census.measured_len(),
        census.len(),
        "every reader is either measured or explicitly absent"
    );
}

/// Whether one row reports complete, by label — the launch surface a caller
/// reads.
fn row_is_complete(census: &cs_app::mission_control::RetailControlCensus, mission: &str) -> bool {
    census
        .row(mission)
        .unwrap_or_else(|| panic!("{mission} is a row"))
        .is_complete()
}

/// **Both vocabulary tables agree, key for key.** The record's dispositions
/// (`.01`'s measured table) and the adapter's binding registry (`.01`'s
/// `Lowering` vocabulary) are compared against each other rather than trusted
/// separately: every one of M01's 43 keys is bound — 41 measured keys to the
/// `DirectiveOperation` whose code both crates publish, the 2 terminal keys to
/// `Lowering::Finish` — with 0 keys dropped and no invented name in the
/// registry.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_lowering_adapter_every_key_is_bound_and_both_vocabulary_tables_agree() {
    let census =
        survey_mission_control_programs(&game_dir()).expect("the census runs on the installation");
    let row = census.row(M01).expect("M01 is measured");
    let record = row.record().expect("M01 declares a control program");
    let lowered = row
        .lowering_attempt()
        .expect("a measured row carries the lowering attempt");
    let attempt = lowered.attempt();

    // The disposition table: 41 measured + 2 terminal = 43, none unmeasured.
    let measured = record.measured();
    let terminal = record.implemented();
    assert_eq!(
        (measured.len(), terminal.len(), record.unmeasured().len()),
        (41, 2, 0),
        "M01's vocabulary partitions into 41 measured keys and 2 outcome keys: {:?}",
        record.unmeasured()
    );
    assert_eq!(
        measured.len() + terminal.len(),
        record.vocabulary() as usize,
        "the two sets partition the vocabulary — nothing sits in neither"
    );
    assert_eq!(record.vocabulary(), 43, "M01 spells 43 distinct keys");

    // The registry table: one binding per key, so the two tables are the same
    // size and every record key is in it — an adapter that dropped a key would
    // fail here rather than in a site count somewhere else.
    assert_eq!(
        lowered.bindings(),
        record.vocabulary() as usize,
        "one `BindingSpec` per dispositioned key"
    );
    assert!(
        attempt.unbound_keys.is_empty(),
        "no key refused registration: {:?}",
        attempt.unbound_keys
    );

    for key in record.keys() {
        let spec = lowered.registry().get(&key.key).unwrap_or_else(|| {
            panic!(
                "the adapter bound `{}`, a key its own disposition table answers",
                key.key
            )
        });
        match key.disposition() {
            DirectiveDisposition::Measured(directive) => {
                let code = directive.operation.code();
                // The cross-crate check: `cs_script`'s operation table must
                // answer the exact code `cs_content` published for this key,
                // and the binding must carry that same operation — neither
                // table is trusted on its own.
                let operation = DirectiveOperation::from_code(code).unwrap_or_else(|| {
                    panic!(
                        "`.01`'s `Lowering` vocabulary answers `{code}`, the code \
                         `{}` measured for `{}`",
                        key.key, code
                    )
                });
                assert_eq!(
                    operation.code(),
                    code,
                    "{}: both crates publish the same code for one operation",
                    key.key
                );
                assert!(
                    matches!(spec.lowering, Lowering::Directive(bound) if bound.code() == code),
                    "{}: the spec binds the measured operation `{code}`, got {:?}",
                    key.key,
                    spec.lowering
                );
            }
            DirectiveDisposition::TerminalOutcome { .. } => {
                assert!(
                    matches!(spec.lowering, Lowering::Finish(_)),
                    "{}: the outcome key binds `Lowering::Finish`, got {:?}",
                    key.key,
                    spec.lowering
                );
            }
            DirectiveDisposition::Unmeasured { reason } => {
                panic!(
                    "{}: M01 spells no unmeasured key, yet the disposition refused it: {}",
                    key.key,
                    reason.detail()
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The synthetic records
// ---------------------------------------------------------------------------

/// A measured-only record lowers completely through the synthetic mission id:
/// both identity rows met, the condition row met (the block lowers to its
/// measured gate) and the calls row met (each site bound) — and the bound call
/// is a `Directive` action carrying the measured operation, while the outcome
/// key is `Finish`.
#[test]
fn accept_m01_lc_lowering_adapter_a_measured_record_lowers_and_reports_complete() {
    let document = control_record(vec![block(
        1,
        vec![
            directive("BEGIN_DORMANT", vec![zrd_float(-1.0)]),
            directive("WAKE_ANIM", vec![zrd_text("wv_hookup")]),
            directive("INSTANTWIN", Vec::new()),
        ],
    )]);
    let (record, lowered) = lower(&document);
    let attempt = lowered.attempt();

    assert_eq!(attempt.mission.as_deref(), Ok("mission/accept-mission"));
    assert_eq!(attempt.objectives, 1);
    assert_eq!(attempt.conditions, [ConditionOutcome::Lowered]);
    assert_eq!(
        attempt.calls,
        [CallOutcome::Bound, CallOutcome::Bound, CallOutcome::Bound],
        "one bound call per site, in record order"
    );
    assert_eq!(attempt.validation.as_deref(), Some(&[][..]));

    let program = lowered.program().expect("every call bound");
    let actions = &program.objectives[0].actions;
    assert!(
        matches!(
            actions[0],
            Action::Directive {
                operation: DirectiveOperation::DormantStart,
                ..
            }
        ) && matches!(
            actions[1],
            Action::Directive {
                operation: DirectiveOperation::WakeAnimation,
                ..
            }
        ) && matches!(actions[2], Action::Finish(Outcome::Succeeded)),
        "the directive sites emit their measured operations and the outcome key \
         emits `Finish`: {actions:?}"
    );
    // The dormant block's condition is the measured gate — awake only, never a
    // constant — not a predicate invented for the fixture.
    assert!(
        matches!(
            program.objectives[0].condition,
            Condition::ObjectiveAwake { .. }
        ),
        "a dormant block lowers to the measured lifecycle gate: {:?}",
        program.objectives[0].condition
    );

    assert!(record.is_complete(attempt));
    assert!(record.lowering(attempt).complete());
}

/// A nested list argument stays a `Value::List` end to end: the `RawCall`
/// carries it, the registry binds it, and the lowered action hands it to the
/// host still nested — the IR has carried lists since stage .01, so no shape is
/// flattened or refused for nesting alone.
#[test]
fn accept_m01_lc_lowering_adapter_a_nested_argument_stays_nested_through_the_bound_call() {
    let document = control_record(vec![block(
        1,
        vec![
            directive(
                "SET_AI_NET",
                vec![zrd_list(vec![zrd_text("alpha"), zrd_text("bravo")])],
            ),
            directive(
                "SET_AI_NET",
                vec![
                    zrd_list(vec![zrd_text("alpha"), zrd_text("bravo")]),
                    zrd_list(vec![zrd_text("delta"), zrd_text("echo")]),
                ],
            ),
        ],
    )]);
    let (record, lowered) = lower(&document);
    let attempt = lowered.attempt();

    let raw = lowered.raw_program().expect("the program assembled");
    let calls = &raw.objectives[0].calls;
    assert_eq!(
        calls[0].args,
        [Value::List(vec![
            Value::Str("alpha".to_owned()),
            Value::Str("bravo".to_owned()),
        ])],
        "the nested argument arrives nested, not flattened"
    );
    assert_eq!(
        calls[1].args.len(),
        2,
        "two nested arguments stay two arguments"
    );
    assert_eq!(
        attempt.calls,
        [CallOutcome::Bound, CallOutcome::Bound],
        "both nested shapes bound — a key whose sites disagree registers one \
         signature per measured shape and picks neither"
    );
    assert_eq!(
        record.key("SET_AI_NET").expect("measured").shapes.len(),
        2,
        "the record still reports both spelled shapes"
    );
    assert!(record.is_complete(attempt));
}

/// An unmeasured key registers **nothing**: its sites refuse the bind as an
/// unknown host call, the calls row names the key and the site, and the record
/// is never complete — no convenient operation is invented for it.
#[test]
fn accept_m01_lc_lowering_adapter_an_unmeasured_key_refuses_its_sites_by_name() {
    // `SET_AI_` is spelled corpus-wide with a handler nobody located — the
    // corpus's live `MeaningNotMeasured` refusal.
    let document = control_record(vec![block(
        1,
        vec![
            directive("WAKE_ANIM", vec![zrd_text("x")]),
            directive("SET_AI_", vec![zrd_text("trouble")]),
            directive("INSTANTWIN", Vec::new()),
        ],
    )]);
    let (record, lowered) = lower(&document);
    let attempt = lowered.attempt();

    assert_eq!(attempt.calls.len(), 3, "every site got a verdict");
    assert!(
        matches!(&attempt.calls[0], CallOutcome::Bound)
            && matches!(&attempt.calls[2], CallOutcome::Bound),
        "the measured and outcome sites still bind: {:?}",
        attempt.calls
    );
    let CallOutcome::Refused(reason) = &attempt.calls[1] else {
        panic!("the unmeasured key's site refuses: {:?}", attempt.calls);
    };
    assert!(
        reason.contains("SET_AI_"),
        "the refusal names the unmeasured key: {reason}"
    );
    assert!(
        lowered.program().is_none() && attempt.validation.is_none(),
        "no program stands while a call cannot bind"
    );

    let lowering = record.lowering(attempt);
    let unmet: Vec<LoweringRequirementKind> = lowering.unmet().map(|row| row.kind).collect();
    assert_eq!(
        unmet,
        [LoweringRequirementKind::CallArguments],
        "only the calls row is unmet — the block's condition still lowered"
    );
    let calls = row_of(&lowering, LoweringRequirementKind::CallArguments);
    assert!(
        calls
            .unmeasured_fields
            .iter()
            .any(|field| field.contains("SET_AI_") && field.contains("meaning_not_measured")),
        "the calls row names the unmeasured key and its reason: {:?}",
        calls.unmeasured_fields
    );
    assert!(
        calls
            .unmeasured_fields
            .iter()
            .any(|field| field.contains("unknown host call")),
        "the calls row names the refused site by its registry reason: {:?}",
        calls.unmeasured_fields
    );
    assert!(!record.is_complete(attempt));
}

/// A site that spells a **scalar** beside its key is `not_a_list`: no `RawCall`
/// can carry it, so the site is refused by name, the block's condition verdict
/// is overridden to a refusal (a predicate lowered over a partially represented
/// directive list is not the record's predicate), and the program fails
/// validation on the `Condition::Unknown` it carries.
#[test]
fn accept_m01_lc_lowering_adapter_a_scalar_site_is_refused_and_damages_its_block() {
    let document = control_record(vec![(
        "OBJECTIVE1".to_owned(),
        // A text key with a scalar beside it — the one grammar shape that is
        // neither a bare directive nor an argument list.
        zrd_list(vec![zrd_text("WAKE_ANIM"), zrd_int(7)]),
    )]);
    let (record, lowered) = lower(&document);
    let attempt = lowered.attempt();

    assert_eq!(attempt.calls.len(), 1);
    let CallOutcome::Refused(reason) = &attempt.calls[0] else {
        panic!("a scalar follower refuses the call: {:?}", attempt.calls);
    };
    assert!(
        reason.contains("WAKE_ANIM") && reason.contains("not_a_list"),
        "the refused site names the key and the defect: {reason}"
    );
    let ConditionOutcome::Refused(field) = &attempt.conditions[0] else {
        panic!(
            "a block whose site is not representable keeps no lowered condition: {:?}",
            attempt.conditions
        );
    };
    assert!(
        field.contains("OBJECTIVE1") && field.contains("WAKE_ANIM"),
        "the damaged block's refusal names the block and the key: {field}"
    );
    assert!(
        attempt
            .validation
            .as_ref()
            .is_some_and(|errors| !errors.is_empty()),
        "the `Condition::Unknown` the block carries fails validation: {:?}",
        attempt.validation
    );
    let unmet: Vec<LoweringRequirementKind> = record
        .lowering(attempt)
        .unmet()
        .map(|row| row.kind)
        .collect();
    assert!(
        unmet.contains(&LoweringRequirementKind::ObjectiveCondition)
            && unmet.contains(&LoweringRequirementKind::CallArguments),
        "the condition and the calls rows are unmet: {unmet:?}"
    );
    assert!(!record.is_complete(attempt));
}

/// A block the walk cannot read stays `RawBlock::Unreadable`: the condition
/// outcome names the block, the `RawObjective` carries `Condition::Unknown`,
/// validation refuses it and the record is never complete — a `.zrd` block
/// nobody read is never flattened into an empty objective.
#[test]
fn accept_m01_lc_lowering_adapter_an_unreadable_block_is_carried_as_a_refusal() {
    let document = control_record(vec![(
        "OBJECTIVE4".to_owned(),
        zrd_text("this is not a directive list"),
    )]);
    let (record, lowered) = lower(&document);
    let attempt = lowered.attempt();

    let ConditionOutcome::Unreadable(detail) = &attempt.conditions[0] else {
        panic!(
            "an unreadable block stays unreadable: {:?}",
            attempt.conditions
        );
    };
    assert!(
        detail.contains("OBJECTIVE4"),
        "the unreadable block is named: {detail}"
    );
    let raw = lowered.raw_program().expect("the program still assembled");
    assert!(
        matches!(raw.objectives[0].condition, Condition::Unknown { .. }),
        "the unreadable block's objective carries the honest `Unknown`"
    );
    assert!(
        attempt
            .validation
            .as_ref()
            .is_some_and(|errors| !errors.is_empty()),
        "validation refuses the `Unknown` condition"
    );
    assert!(!record.is_complete(attempt));
    assert!(
        record.to_lowering_refusal(attempt).contains("OBJECTIVE4"),
        "the lowering refusal names the block: {}",
        record.to_lowering_refusal(attempt)
    );
}

/// A block whose evaluator **stage `.02` refused** still refuses here: the
/// attempt reports `ConditionOutcome::Refused` by block and key, the
/// `objective_condition` row goes unmet naming that field, and the record is
/// never complete. A refusal downstream of the walk is a refusal of the whole
/// lowering, not a condition that quietly defaults.
#[test]
fn accept_m01_lc_lowering_adapter_a_block_the_condition_lowering_refuses_stays_unmet() {
    // Two `DEDG` evaluators spell one block: the record holds one slot per
    // evaluator kind, so `cs_script::conditions` refuses by block and key
    // rather than inventing a disjunction the original cannot produce.
    let document = control_record(vec![block(
        9,
        vec![
            directive("DEDG", vec![zrd_int(4), zrd_int(1)]),
            directive("DEDG", vec![zrd_int(4), zrd_int(1)]),
            directive("INSTANTWIN", Vec::new()),
        ],
    )]);
    let (record, lowered) = lower(&document);
    let attempt = lowered.attempt();

    let ConditionOutcome::Refused(field) = &attempt.conditions[0] else {
        panic!(
            "the refused evaluator keeps no lowered condition: {:?}",
            attempt.conditions
        );
    };
    assert!(
        field.contains("OBJECTIVE9") && field.contains("DEDG"),
        "the refusal names the block and the key: {field}"
    );
    assert_eq!(
        attempt.calls,
        [CallOutcome::Bound, CallOutcome::Bound, CallOutcome::Bound],
        "the sites themselves still bound — the condition row fails, not the calls row"
    );

    let lowering = record.lowering(attempt);
    let unmet: Vec<LoweringRequirementKind> = lowering.unmet().map(|row| row.kind).collect();
    assert_eq!(
        unmet,
        [
            LoweringRequirementKind::ObjectiveCondition,
            LoweringRequirementKind::CallArguments,
        ],
        "the condition row is unmet, and the calls row with it: a program whose \
        objective carries `Condition::Unknown` does not pass `MissionProgram::validate`"
    );
    let condition = row_of(&lowering, LoweringRequirementKind::ObjectiveCondition);
    assert!(
        condition
            .unmeasured_fields
            .iter()
            .any(|row| row.contains("OBJECTIVE9") && row.contains("DEDG")),
        "the condition row names the refused field: {:?}",
        condition.unmeasured_fields
    );
    assert!(
        !record.is_complete(attempt),
        "a record whose block refused is never complete"
    );
}

/// An empty record stays unmet: with no numbered block the attempt emits no
/// `RawObjective`, so every row the program needs goes unmet with a named
/// field — a record nobody read can never report itself ready.
#[test]
fn accept_m01_lc_lowering_adapter_an_empty_record_stays_unmet() {
    let document = control_record(Vec::new());
    let (record, lowered) = lower(&document);
    let attempt = lowered.attempt();

    assert_eq!(record.blocks(), 0, "the record declares no block");
    let lowering = record.lowering(attempt);
    assert_eq!(
        lowering.unmet().count(),
        3,
        "an empty record fails identity, condition and calls: {:?}",
        lowering
            .unmet()
            .map(|row| row.kind)
            .collect::<Vec<LoweringRequirementKind>>()
    );
    for row in lowering.unmet() {
        assert!(
            !row.unmeasured_fields.is_empty(),
            "{}: an unmet row names what it lacks",
            row.label()
        );
    }
    assert!(!lowering.complete());
    assert!(!record.is_complete(attempt));
}

/// A record the campaign layout cannot name still walks and lowers its blocks:
/// the conditions and the per-site verdicts are produced, but no `RawProgram`
/// is assembled, so `mission_identity` and `objective_identity` are unmet with
/// the reason named and no content ids are invented.
#[test]
fn accept_m01_lc_lowering_adapter_a_missing_mission_id_produces_no_program() {
    let document = control_record(vec![block(1, vec![directive("INSTANTWIN", Vec::new())])]);
    let record = measure_control_record(&document);
    let lowered = lower_control_record(
        Err("the campaign layout names no mission for `zbd/c9/m99/zrdr.zbd`".to_owned()),
        "zbd/c9/m99",
        &document,
        &record,
    );
    let attempt = lowered.attempt();

    let Err(reason) = &attempt.mission else {
        panic!("no mission id resolved, so none was reported");
    };
    assert!(
        reason.contains("zbd/c9/m99"),
        "the attempt names the container nobody bound: {reason}"
    );
    assert!(
        lowered.raw_program().is_none() && lowered.program().is_none(),
        "no `RawProgram` is assembled without a mission id"
    );
    assert_eq!(attempt.objectives, 0);
    assert_eq!(
        attempt.conditions,
        [ConditionOutcome::Lowered],
        "the block's condition still lowered — the verdicts are produced even \
         when no program can carry them"
    );
    assert_eq!(attempt.calls, [CallOutcome::Bound]);

    let unmet: Vec<LoweringRequirementKind> = record
        .lowering(attempt)
        .unmet()
        .map(|row| row.kind)
        .collect();
    assert_eq!(
        unmet,
        [
            LoweringRequirementKind::MissionIdentity,
            LoweringRequirementKind::ObjectiveIdentity,
            LoweringRequirementKind::CallArguments,
        ],
        "identity and calls are unmet without a program; the condition row \
         stands on the lowered blocks"
    );
    assert!(!record.is_complete(attempt));
}

/// The rows derive from the attempt, not from the vocabulary: the same record
/// lowers complete when the attempt did, and every row is unmet with a named
/// field when the attempt refused — an unmet row that named nothing is the
/// failure this accounting exists to prevent.
#[test]
fn accept_m01_lc_lowering_adapter_the_rows_report_the_attempt_not_the_vocabulary() {
    let document = control_record(vec![block(
        1,
        vec![
            directive("INACTIVE1", vec![zrd_text("workersvoyagezep")]),
            directive("WAKE_ANIM", vec![zrd_text("wv_hookup")]),
            directive("INSTANTWIN", Vec::new()),
        ],
    )]);
    let (record, lowered) = lower(&document);
    assert!(
        record.lowering(lowered.attempt()).complete(),
        "the real attempt lowered the record completely"
    );

    let refused =
        LoweringAttempt::refused("the adapter never ran: the control member did not decode");
    let lowering = record.lowering(&refused);
    assert_eq!(
        lowering.unmet().count(),
        4,
        "with a refused attempt every row is unmet — the record's known \
         vocabulary cannot lift a row the attempt did not satisfy"
    );
    for row in lowering.unmet() {
        assert!(
            !row.unmeasured_fields.is_empty(),
            "{}: an unmet row names what it lacks",
            row.label()
        );
    }
    assert!(
        !record.is_complete(&refused),
        "a record is complete only while its attempt says so"
    );
}
