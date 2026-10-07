//! M01-LC-DIRECTIVE-LOWERING.02 acceptance: every one of M01's 58 numbered
//! blocks lowers to a side-effect-free `Condition` `MissionProgram::validate`
//! accepts, evaluation writes nothing, refusals name the block and the key,
//! and the fail-closed cases stay closed.
//!
//! Task key `M01-LC-DIRECTIVE-LOWERING.02`; test prefix
//! `accept_m01_lc_lowering_conditions_`. Shared contract
//! `docs/contracts/SCRIPT-MISSION.md` ("IR requirements", "Objective event
//! ordering"); specs `specs/F37-mission-ir-and-deterministic-runtime-core.md`
//! and `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`;
//! findings `docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`
//! and `…-c-ai-world-and-animation-directives.md`.
//!
//! # What is re-derived and what is synthetic
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]` and re-read
//! M01's control member from the installation on **every** run through
//! production discovery (`cs_assets::install::discover`,
//! `cs_formats::script_raw::discover_container`, `cs_content::stunts::decode_zrd`,
//! `mission_control::control_member`) — so a stale constant fails instead of
//! passing, and CI (which has no original data) runs only the synthetic half.
//!
//! The synthetic tests exercise production code only: the lowering module,
//! `MissionState::holds` and `cs_sim`'s block-lifecycle table. Nothing here
//! reimplements the directive grammar — per-block directives come from
//! `cs_content::stunts::zrd_directive_fields`, the production walk, and the
//! census cross-check pins the shapes it yields to
//! `mission_control::measure_control_record`'s own measurement.
//!
//! # Which M01 blocks lower, and which refuse
//!
//! Stated in the suite rather than left implicit (AC4): **all 58 lower, and
//! none refuses.** The per-block classification — 12 `INACTIVE` ladders,
//! 8 `DEDG`, 1 `TRAVELERS`, 3 `ANIM_STATE`, 34 lifecycle-only, 3 of them
//! gated by `TICK_DEPENDS_ON_OBJ` — is asserted from the lowered conditions
//! themselves in the retail test below, so it can never drift from what the
//! lowering actually produced.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cs_content::mission_control::{self, BlockRefusal as MeasuredBlockRefusal, DecodedMember};
use cs_content::objectives::{
    inactive_stage_number, measure_dormant_declarations, objective_block_number,
};
use cs_content::stunts::{
    ZrdValue, decode_zrd, objective_record, zrd_directive_fields, zrd_flat_fields,
    zrd_is_bare_argument,
};
use cs_formats::script_raw::{discover_container, mission_scope};
use cs_script::conditions::{
    BlockCondition, BlockDirective, BlockRefusal, ConditionRefusal, DirectiveArguments, RawBlock,
    TRAVELERS_APPROACHING, lower_block_condition, lower_record,
};
use cs_script::ir::{
    AnimationState, Condition, DEDG_MEMBER_FIELD_REWRITES, IN_PLAY_BIT_WRITERS_UNTRACED,
    IR_VERSION, MissionProgram, Objective, Value,
};
use cs_script::runtime::{
    MemberFact, MemberPresence, MissionFacts, MissionState, ObjectiveLifecycle, SessionGeneration,
};
use cs_types::content::{ContentId, ContentKind};
use cs_types::install::RelativePath;

// --------------------------------------------------------------- discovery ---

/// M01's mission label under F13-B's scope rule (chapter-1 group `c1c`), as
/// `missions/bindings/M01.json` records it.
const M01: &str = "zbd/c1c/m01";

/// The reader archive every mission's control program lives in.
const MISSION_READER_ARCHIVE: &str = "zrdr.zbd";

/// How many numbered blocks M01 declares — the measured figure the census
/// reports, re-derived here rather than read from a constant.
const M01_BLOCKS: usize = 58;

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the original installation"),
    )
}

/// M01's decoded control member, chosen by the production rule (the member
/// that declares numbered `OBJECTIVE<N>` blocks).
fn control_document() -> ZrdValue {
    let root = game_dir();
    let found = cs_assets::install::discover(&root).expect("the installation is discoverable");
    for record in &found.manifest.files {
        let container_key = record.relative_spelling.logical_key();
        if !container_key.ends_with(MISSION_READER_ARCHIVE) {
            continue;
        }
        let spelling = record.relative_spelling.as_str();
        let Ok(path) = RelativePath::new(&spelling.to_lowercase()) else {
            continue;
        };
        if mission_scope(&path).as_deref() != Some(M01) {
            continue;
        }
        let bytes = std::fs::read(found.manifest.host_root.join(spelling))
            .expect("the archive is readable");
        let discovery = discover_container(&container_key, &path, &bytes);
        let mut decoded = Vec::new();
        for program in discovery.programs() {
            let Some(name) = program.locator().member() else {
                continue;
            };
            let Ok(document) = decode_zrd(program.bytes()) else {
                continue;
            };
            decoded.push(DecodedMember::new(name.to_owned(), document));
        }
        let control = mission_control::control_member(&container_key, &decoded)
            .expect("M01 has exactly one control member");
        return control.document.clone();
    }
    panic!("M01's reader archive is not in {root:?}");
}

/// The mechanical crossing a record adapter performs
/// (`cs_content::stunts::ZrdValue` → `cs_script::ir::Value`), spelled out
/// here because the row lives outside this crate — the same edge
/// `accept_m01_lc_lowering_signatures` writes for measured argument shapes.
fn zrd_to_value(value: &ZrdValue) -> Value {
    match value {
        ZrdValue::Int(number) => Value::Int(
            i32::try_from(*number).expect("a record int fits the IR's checked 32-bit integer"),
        ),
        ZrdValue::Float(number) => Value::Float(f64::from(*number)),
        ZrdValue::Text(text) => Value::Str(text.clone()),
        ZrdValue::List(children) => Value::List(children.iter().map(zrd_to_value).collect()),
    }
}

/// The measurement side's `BlockRefusal` as this crate carries it — the two
/// are pinned to the same codes by the fail-closed test below.
fn mirror(refusal: &MeasuredBlockRefusal) -> BlockRefusal {
    match refusal {
        MeasuredBlockRefusal::BlockNotAList { block } => BlockRefusal::BlockNotAList {
            block: block.clone(),
        },
        MeasuredBlockRefusal::KeyNotText { block, index } => BlockRefusal::KeyNotText {
            block: block.clone(),
            index: *index,
        },
    }
}

/// Every numbered block of `document`, in record order, as the lowering
/// consumes it: the production directive walk, with any block the census
/// refused carried as a refusal.
fn raw_blocks(document: &ZrdValue) -> Vec<RawBlock> {
    let census = mission_control::measure_control_record(document);
    let refused: Vec<&MeasuredBlockRefusal> = census.refusals().iter().collect();
    let mut blocks = Vec::new();
    for (key, value) in zrd_flat_fields(objective_record(document)) {
        if objective_block_number(key).is_none() {
            continue;
        }
        let index = blocks.len() as u32;
        if let Some(refusal) = refused.iter().find(|entry| entry.block() == key) {
            blocks.push(RawBlock::Unreadable(mirror(refusal)));
            continue;
        }
        let directives = zrd_directive_fields(value)
            .into_iter()
            .map(|(name, argument)| BlockDirective {
                key: name.to_owned(),
                args: if zrd_is_bare_argument(argument) {
                    DirectiveArguments::Bare
                } else if let Some(children) = argument.as_list() {
                    DirectiveArguments::List(children.iter().map(zrd_to_value).collect())
                } else {
                    DirectiveArguments::NotAList(zrd_to_value(argument))
                },
            })
            .collect();
        blocks.push(RawBlock::Read {
            block: key.to_owned(),
            index,
            directives,
        });
    }
    blocks
}

/// The measured label of one site's arguments, in the census's own vocabulary
/// (`cs_content::mission_control::MeasuredArg::label`) — used only to pin
/// this walk's view of the record to the census's.
fn measured_label(value: &Value) -> String {
    match value {
        Value::Int(_) => "int".to_owned(),
        Value::Float(_) => "float".to_owned(),
        Value::Str(_) => "text".to_owned(),
        Value::List(items) => {
            if items.is_empty() {
                "[]".to_owned()
            } else {
                format!(
                    "[{}]",
                    items
                        .iter()
                        .map(measured_label)
                        .collect::<Vec<_>>()
                        .join(",")
                )
            }
        }
        other => format!("unexpected-{:?}", other.value_type()),
    }
}

fn shape_label(args: &DirectiveArguments) -> String {
    match args {
        DirectiveArguments::Bare => "bare".to_owned(),
        DirectiveArguments::NotAList(value) => format!("not_a_list({})", measured_label(value)),
        DirectiveArguments::List(values) => {
            if values.is_empty() {
                "[]".to_owned()
            } else {
                format!(
                    "[{}]",
                    values
                        .iter()
                        .map(measured_label)
                        .collect::<Vec<_>>()
                        .join(",")
                )
            }
        }
    }
}

// ------------------------------------------------------------------ helpers ---

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

fn s(text: &str) -> Value {
    Value::Str(text.to_owned())
}

fn ls(items: Vec<Value>) -> Value {
    Value::List(items)
}

fn chain(names: &[&str]) -> Vec<Value> {
    names.iter().map(|name| s(name)).collect()
}

/// Lowers one synthetic block, refusing to let a test pass on a refusal.
fn lower(block: &str, index: u32, directives: Vec<BlockDirective>) -> Condition {
    lower_block_condition(block, index, &directives)
        .unwrap_or_else(|refusal| panic!("{block} must lower, refused: {refusal}"))
}

/// A session whose program declares nothing: `holds` is asked about
/// conditions directly, so no objective of its own can fire.
fn evaluator() -> MissionState {
    let program = MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic"),
        variables: vec![],
        objectives: vec![],
    }
    .validate()
    .expect("an empty program validates");
    MissionState::new(&program, SessionGeneration(1))
}

/// Every world-shaped leaf inside a condition, in document order — so a test
/// can say what a lowered predicate is made of without matching its shape.
fn leaves(condition: &Condition) -> Vec<&'static str> {
    match condition {
        Condition::ObjectiveAwake { .. } => vec!["objective_awake"],
        Condition::InactiveMembers { .. } => vec!["inactive_members"],
        Condition::EnemyGroupDepletion { .. } => vec!["enemy_group_depletion"],
        Condition::Travelers { .. } => vec!["travelers"],
        Condition::AnimationStates { .. } => vec!["animation_states"],
        Condition::Not(inner) => leaves(inner),
        Condition::All(items) | Condition::Any(items) => {
            let mut collected = Vec::new();
            for item in items {
                collected.extend(leaves(item));
            }
            collected
        }
        Condition::Const(_) => vec!["const"],
        Condition::Compare { .. } => vec!["compare"],
        Condition::ActorIs { .. } => vec!["actor_is"],
        Condition::Unknown { .. } => vec!["unknown"],
    }
}

// ------------------------------------------------------------- the census ----

/// AC1/AC4 (retail): every one of M01's 58 blocks yields a `Condition`
/// `MissionProgram::validate` accepts — never `Unknown`, never a `Const`
/// placeholder — and the suite states which blocks lower and which refuse.
///
/// Also pins this walk to the census (same keys, same measured shapes) and
/// asserts the residual unknowns the two evaluators carry are still named.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_lowering_conditions_every_m01_block_lowers_to_a_validated_condition() {
    let document = control_document();
    let census = mission_control::measure_control_record(&document);
    let blocks = raw_blocks(&document);
    assert_eq!(
        blocks.len(),
        M01_BLOCKS,
        "M01 declares {} numbered blocks, not {M01_BLOCKS}",
        census.blocks()
    );
    assert_eq!(census.blocks() as usize, M01_BLOCKS);

    // This walk and the census read the same record: every (key, shape)
    // site-count pair agrees, so the lowering sees what was measured.
    let mut from_census: BTreeMap<(String, String), u32> = BTreeMap::new();
    for key in census.keys() {
        for (shape, sites) in &key.shapes {
            *from_census
                .entry((key.key.clone(), shape.label()))
                .or_default() += sites;
        }
    }
    let mut from_walk: BTreeMap<(String, String), u32> = BTreeMap::new();
    for block in &blocks {
        let RawBlock::Read { directives, .. } = block else {
            continue;
        };
        for directive in directives {
            *from_walk
                .entry((directive.key.clone(), shape_label(&directive.args)))
                .or_default() += 1;
        }
    }
    assert_eq!(
        from_walk, from_census,
        "the production directive walk and the census see the same sites"
    );

    let lowered = lower_record(&blocks);
    assert_eq!(lowered.len(), M01_BLOCKS);

    // The explicit statement AC4 asks for: which blocks lower, which refuse.
    let mut refused: Vec<String> = Vec::new();
    let mut classified: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    let mut dependencies: BTreeMap<String, u32> = BTreeMap::new();
    let mut no_const_no_unknown = true;
    let evaluator_state = evaluator();
    let facts = MissionFacts::default();

    for (raw, answer) in blocks.iter().zip(&lowered) {
        let RawBlock::Read { block, index, .. } = raw else {
            refused.push(format!("{raw:?} has no condition"));
            continue;
        };
        let Some(condition) = answer.condition() else {
            refused.push(format!("{block}: {answer}"));
            continue;
        };
        if matches!(condition, Condition::Const(_) | Condition::Unknown { .. }) {
            no_const_no_unknown = false;
        }
        let present = leaves(condition);
        let kind = if present.contains(&"inactive_members") {
            "INACTIVE"
        } else if present.contains(&"enemy_group_depletion") {
            "DEDG"
        } else if present.contains(&"travelers") {
            "TRAVELERS"
        } else if present.contains(&"animation_states") {
            "ANIM_STATE"
        } else {
            "lifecycle only"
        };
        classified.entry(kind).or_default().push(block.clone());

        // The lifecycle gate: every block gates on itself, and a block the
        // record names a dependency for gates on that one too.
        let gates: Vec<u32> = match condition {
            Condition::ObjectiveAwake { index: target } => vec![*target],
            Condition::All(items) => items
                .iter()
                .filter_map(|item| match item {
                    Condition::ObjectiveAwake { index: target } => Some(*target),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        assert!(
            gates.contains(index),
            "{block} must gate on its own lifecycle state"
        );
        if let Some(second) = gates.get(1) {
            dependencies.insert(block.clone(), *second);
            assert!(
                *second < M01_BLOCKS as u32,
                "{block} depends on block index {second}, past the record"
            );
        }

        // Evaluation is side-effect-free: both receivers are shared
        // references, and neither the state nor the facts change.
        let state_before = evaluator_state.clone();
        let facts_before = facts.clone();
        evaluator_state.holds(condition, &facts);
        assert_eq!(
            evaluator_state, state_before,
            "{block} evaluation wrote state"
        );
        assert_eq!(facts, facts_before, "{block} evaluation wrote facts");

        // A residual unknown that does not change the predicate is carried,
        // never dropped: the INACTIVE ladder names its bit's untraced
        // writers and the DEDG predicate names the original's rewrites.
        if present.contains(&"inactive_members") {
            assert!(
                condition
                    .residual_unknowns()
                    .contains(&IN_PLAY_BIT_WRITERS_UNTRACED),
                "{block} carries the in-play-bit residual unknown"
            );
        }
        if present.contains(&"enemy_group_depletion") {
            assert!(
                condition
                    .residual_unknowns()
                    .contains(&DEDG_MEMBER_FIELD_REWRITES),
                "{block} carries the DEDG rewrite residual unknown"
            );
        }
    }

    assert!(
        refused.is_empty(),
        "no M01 block may refuse; refused: {refused:?}"
    );
    assert!(
        no_const_no_unknown,
        "every M01 condition is a real predicate, not Const or Unknown"
    );

    // The measured classification: 12 INACTIVE ladders, 8 DEDG, 1 TRAVELERS,
    // 3 ANIM_STATE and 34 lifecycle-only — 58 in total.
    let count = |kind: &'static str| {
        classified
            .get(kind)
            .map(Vec::len)
            .unwrap_or_else(|| panic!("M01 has no {kind} block"))
    };
    assert_eq!(count("INACTIVE"), 12);
    assert_eq!(count("DEDG"), 8);
    assert_eq!(count("TRAVELERS"), 1);
    assert_eq!(count("ANIM_STATE"), 3);
    assert_eq!(count("lifecycle only"), 34);
    assert_eq!(
        classified.values().map(Vec::len).sum::<usize>(),
        M01_BLOCKS,
        "every M01 block is classified exactly once"
    );

    // `TICK_DEPENDS_ON_OBJ` gates three blocks: OBJECTIVE42 and OBJECTIVE43
    // on block 54, OBJECTIVE57 on block 56 (the record's 1-based child0,
    // stored decremented).
    assert_eq!(
        dependencies,
        BTreeMap::from([
            ("OBJECTIVE42".to_owned(), 53),
            ("OBJECTIVE43".to_owned(), 53),
            ("OBJECTIVE57".to_owned(), 55),
        ]),
        "the dependency gate is represented rather than dropped"
    );

    // 52 of the 58 blocks start dormant (finding B's measured site count),
    // read from the record's own lifecycle declarations.
    let dormant = measure_dormant_declarations(&document)
        .expect("M01's dormant declarations are measured shapes")
        .iter()
        .filter(|declared| declared.begins_dormant())
        .count();
    assert_eq!(dormant, 52, "M01 spells BEGIN_DORMANT in 52 blocks");

    // Every condition validates as one program — AC1's "that
    // MissionProgram::validate accepts", over all 58 at once.
    let objectives: Vec<Objective> = lowered
        .iter()
        .enumerate()
        .map(|(index, answer)| Objective {
            id: cs_script::ir::SymbolId(index as u32),
            content: cid(ContentKind::Objective, &format!("M01_BLOCK_{index}")),
            condition: answer.condition().expect("M01 lowers every block").clone(),
            actions: vec![],
            span: None,
        })
        .collect();
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "m01"),
        variables: vec![],
        objectives,
    }
    .validate()
    .expect("all 58 lowered conditions validate as one program");
}

/// AC1 (retail): the measured polarity token of M01's single `TRAVELERS`
/// site is the measured inside pole, and every `ANIM_STATE` token M01 spells
/// maps to a state the engine's name table holds — the two residual unknowns
/// that would otherwise change those predicates.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_lowering_conditions_m01s_tokens_are_the_measured_ones() {
    let document = control_document();
    let blocks = raw_blocks(&document);

    let mut travelers_sites = 0;
    let mut anim_sites = 0;
    for block in &blocks {
        let RawBlock::Read { directives, .. } = block else {
            continue;
        };
        for directive in directives {
            match directive.key.as_str() {
                "TRAVELERS" => {
                    let DirectiveArguments::List(args) = &directive.args else {
                        panic!("M01's TRAVELERS site spells an argument list");
                    };
                    assert_eq!(
                        args.get(1),
                        Some(&s(TRAVELERS_APPROACHING)),
                        "M01's only TRAVELERS site spells the measured inside pole; any other \
                         token is an unmeasured direction and must refuse"
                    );
                    assert!(
                        matches!(args.first(), Some(Value::Str(_))),
                        "M01's TRAVELERS site spells a subject name, so it takes subject mode"
                    );
                    travelers_sites += 1;
                }
                "ANIM_STATE" => {
                    let DirectiveArguments::List(args) = &directive.args else {
                        panic!("M01's ANIM_STATE site spells an argument list");
                    };
                    let Value::List(spec) = args.get(1).expect("the descriptor follows the tag")
                    else {
                        panic!("M01's ANIM_STATE descriptor is a list");
                    };
                    for pair in spec.chunks(2) {
                        if let (Some(Value::Str(key)), Some(Value::List(wrapped))) =
                            (pair.first(), pair.get(1))
                            && key == "STATE"
                            && let Some(Value::Str(token)) = wrapped.first()
                        {
                            let state = AnimationState::from_token(token)
                                .unwrap_or_else(|| panic!("{token} is not a measured token"));
                            assert!(
                                state.code() <= 6,
                                "the state enum above 6 is unexercised by M01"
                            );
                            anim_sites += 1;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    assert_eq!(
        travelers_sites, 1,
        "M01 spells TRAVELERS once, in OBJECTIVE3"
    );
    assert_eq!(
        anim_sites, 3,
        "M01 spells ANIM_STATE three times: OBJECTIVE11, OBJECTIVE15, OBJECTIVE18"
    );

    // The stage-key rule this lowering mirrors is the measurement side's own:
    // `INACTIVE<n>` is a stage and the threshold key is not, so the two
    // cannot disagree about which directive is an evaluator.
    let mut stage_sites = 0;
    for block in &blocks {
        let RawBlock::Read { directives, .. } = block else {
            continue;
        };
        for directive in directives {
            if directive.key.starts_with("INACTIVE") {
                if directive.key == "INACTIVE_COMPLETION_COUNT" {
                    assert!(
                        inactive_stage_number(&directive.key).is_none(),
                        "the threshold key is not a stage"
                    );
                } else {
                    assert!(
                        inactive_stage_number(&directive.key).is_some(),
                        "{} must be a stage key",
                        directive.key
                    );
                    stage_sites += 1;
                }
            }
        }
    }
    assert_eq!(stage_sites, 140, "M01 spells 140 INACTIVE<n> member rows");
}

// ------------------------------------------------------------- evaluation ----

/// AC2: the measured predicates read the facts and nothing else — the
/// threshold ladder, the group count, the radius with its strict boundary,
/// the animation states and the lifecycle gates.
#[test]
fn accept_m01_lc_lowering_conditions_measured_predicates_evaluate_from_facts() {
    let state = evaluator();
    let mut facts = MissionFacts::default();
    // Blocks 0..=5 are awake; block 6 is left undeclared so the lifecycle
    // gate below can be asked both ways.
    for index in 0..6 {
        facts.objectives.insert(index, ObjectiveLifecycle::Awake);
    }

    // --- INACTIVE: threshold over the named member chains -----------------
    let inactive = lower(
        "OBJECTIVE1",
        0,
        vec![
            BlockDirective::new("INACTIVE_COMPLETION_COUNT", vec![Value::Int(2)]),
            BlockDirective::new("INACTIVE1", chain(&["zeppelin", "reng1", "healthy"])),
            BlockDirective::new("INACTIVE2", chain(&["zeppelin", "reng2", "healthy"])),
            BlockDirective::new("INACTIVE3", chain(&["zeppelin", "reng3", "healthy"])),
        ],
    );
    assert!(!state.holds(&inactive, &facts), "nothing observed yet");

    let row = |presence| MemberFact {
        presence,
        position: [0.0; 3],
    };
    facts.members.insert(
        vec!["zeppelin".into(), "reng1".into(), "healthy".into()],
        row(MemberPresence::OutOfPlay),
    );
    assert!(
        !state.holds(&inactive, &facts),
        "one cleared member of a threshold of two"
    );
    facts.members.insert(
        vec!["zeppelin".into(), "reng2".into(), "healthy".into()],
        row(MemberPresence::InPlay),
    );
    assert!(
        !state.holds(&inactive, &facts),
        "a member still in play is not counted"
    );
    facts.members.insert(
        vec!["zeppelin".into(), "reng3".into(), "healthy".into()],
        row(MemberPresence::Missing),
    );
    assert!(
        !state.holds(&inactive, &facts),
        "a name that did not resolve is not counted"
    );
    facts.members.insert(
        vec!["zeppelin".into(), "reng2".into(), "healthy".into()],
        row(MemberPresence::OutOfPlay),
    );
    assert!(
        state.holds(&inactive, &facts),
        "two cleared members satisfy the threshold of two"
    );

    // The threshold the record does not spell defaults to the member count:
    // of these two members only `reng1` is cleared, so one of two is not
    // enough — and clearing `reng3` completes the block.
    let default_threshold = lower(
        "OBJECTIVE2",
        1,
        vec![
            BlockDirective::new("INACTIVE1", chain(&["zeppelin", "reng3", "healthy"])),
            BlockDirective::new("INACTIVE2", chain(&["zeppelin", "reng1", "healthy"])),
        ],
    );
    assert!(
        !state.holds(&default_threshold, &facts),
        "the default threshold is both listed members, and only one is cleared"
    );
    facts.members.insert(
        vec!["zeppelin".into(), "reng3".into(), "healthy".into()],
        row(MemberPresence::OutOfPlay),
    );
    assert!(
        state.holds(&default_threshold, &facts),
        "the default threshold counts every listed member"
    );

    // --- DEDG: living group members plus what the generator owes ----------
    let dedg = lower(
        "OBJECTIVE3",
        2,
        vec![
            BlockDirective::new("DEDG", vec![Value::Int(4), Value::Int(1)]),
            BlockDirective::bare("WAKEUP_SOUND_GROUP".to_owned()),
        ],
    );
    assert!(
        !state.holds(&dedg, &facts),
        "an unrecorded group is unknown, not empty"
    );
    facts.groups.insert(4, 2);
    assert!(
        !state.holds(&dedg, &facts),
        "two living over a limit of one"
    );
    facts.groups.insert(4, 1);
    assert!(state.holds(&dedg, &facts), "one living meets the limit");
    let owed = lower(
        "OBJECTIVE4",
        3,
        vec![BlockDirective::new(
            "DEDG",
            vec![Value::Int(4), Value::Int(1), s("generator")],
        )],
    );
    facts.generators.insert("generator".to_owned(), 1);
    assert!(
        !state.holds(&owed, &facts),
        "the generator's pending spawn still counts against the limit"
    );
    facts.generators.insert("generator".to_owned(), 0);
    assert!(state.holds(&owed, &facts));

    // --- TRAVELERS: the subject inside the radius, strictly ---------------
    let travelers = lower(
        "OBJECTIVE5",
        4,
        vec![BlockDirective::new(
            "TRAVELERS",
            vec![
                s("player"),
                s(TRAVELERS_APPROACHING),
                s("anchor"),
                Value::Float(700.0),
                Value::Int(1),
            ],
        )],
    );
    facts
        .members
        .insert(vec!["player".to_owned()], row(MemberPresence::InPlay));
    assert!(
        !state.holds(&travelers, &facts),
        "the anchor the facts do not carry makes no distance"
    );
    facts.members.insert(
        vec!["anchor".to_owned()],
        MemberFact {
            presence: MemberPresence::InPlay,
            position: [0.0, 0.0, 700.0],
        },
    );
    assert!(
        !state.holds(&travelers, &facts),
        "equality at the radius never fires"
    );
    facts.members.insert(
        vec!["anchor".to_owned()],
        MemberFact {
            presence: MemberPresence::InPlay,
            position: [0.0, 0.0, 699.0],
        },
    );
    assert!(state.holds(&travelers, &facts), "inside the radius");
    facts
        .members
        .insert(vec!["player".to_owned()], row(MemberPresence::OutOfPlay));
    assert!(
        !state.holds(&travelers, &facts),
        "a subject no longer in play falls to the counting path, which never fires here"
    );

    // --- ANIM_STATE: at least `required` animations in the wanted state ---
    let anim = lower(
        "OBJECTIVE6",
        5,
        vec![BlockDirective::new(
            "ANIM_STATE",
            vec![
                s("ANIM"),
                ls(vec![
                    s("NAME"),
                    ls(vec![s("wv_drop_copilot")]),
                    s("STATE"),
                    ls(vec![s("RUNNING")]),
                ]),
            ],
        )],
    );
    assert!(!state.holds(&anim, &facts), "no animation observed");
    facts.animations.insert(
        "wv_drop_copilot".to_owned(),
        AnimationState::Executed.code(),
    );
    assert!(!state.holds(&anim, &facts), "EXECUTED is not RUNNING");
    facts
        .animations
        .insert("wv_drop_copilot".to_owned(), AnimationState::Running.code());
    assert!(state.holds(&anim, &facts));

    // --- the lifecycle gate, and the residual unknowns still named --------
    let dormant_only = lower(
        "OBJECTIVE7",
        6,
        vec![BlockDirective::new(
            "BEGIN_DORMANT",
            vec![Value::Float(-1.0)],
        )],
    );
    assert_eq!(dormant_only, Condition::ObjectiveAwake { index: 6 });
    assert!(
        !state.holds(&dormant_only, &facts),
        "no lifecycle fact for 6"
    );
    facts.objectives.insert(6, ObjectiveLifecycle::Dormant);
    assert!(!state.holds(&dormant_only, &facts), "a dormant block");
    facts.objectives.insert(6, ObjectiveLifecycle::Awake);
    assert!(state.holds(&dormant_only, &facts));

    assert!(
        Condition::InactiveMembers {
            members: vec![vec!["a".to_owned()]],
            threshold: 1,
        }
        .residual_unknowns()
        .contains(&IN_PLAY_BIT_WRITERS_UNTRACED),
        "the in-play bit's untraced writers stay a named residual unknown"
    );
    assert!(
        dedg.residual_unknowns()
            .contains(&DEDG_MEMBER_FIELD_REWRITES),
        "the original's evaluation-time member-field rewrites stay a named residual unknown and \
         never enter the predicate"
    );
    assert!(
        travelers.residual_unknowns().is_empty() && anim.residual_unknowns().is_empty(),
        "a resolved token carries no residual unknown"
    );
}

/// AC2: evaluation performs no write — the state and the facts are compared
/// before and after the predicates that the original's evaluators produce,
/// including `DEDG` (whose original rewrites three member fields while
/// evaluating) and `INACTIVE` (whose bit's writers are untraced).
#[test]
fn accept_m01_lc_lowering_conditions_evaluation_writes_nothing() {
    let state = evaluator();
    let mut facts = MissionFacts::default();
    for index in 0..4 {
        facts.objectives.insert(index, ObjectiveLifecycle::Awake);
    }
    facts.groups.insert(7, 0);
    facts.members.insert(
        vec!["zeppelin".to_owned(), "reng1".into(), "healthy".into()],
        MemberFact {
            presence: MemberPresence::OutOfPlay,
            position: [1.0, 2.0, 3.0],
        },
    );
    facts.members.insert(
        vec!["player".to_owned()],
        MemberFact {
            presence: MemberPresence::InPlay,
            position: [9.0, 9.0, 9.0],
        },
    );
    facts.animations.insert("anim".to_owned(), 3);

    let conditions = vec![
        lower(
            "OBJECTIVE1",
            0,
            vec![
                BlockDirective::new("INACTIVE_COMPLETION_COUNT", vec![Value::Int(1)]),
                BlockDirective::new("INACTIVE1", chain(&["zeppelin", "reng1", "healthy"])),
            ],
        ),
        lower(
            "OBJECTIVE2",
            1,
            vec![BlockDirective::new(
                "DEDG",
                vec![Value::Int(7), Value::Int(0)],
            )],
        ),
        lower(
            "OBJECTIVE3",
            2,
            vec![BlockDirective::new(
                "TRAVELERS",
                vec![
                    s("player"),
                    s(TRAVELERS_APPROACHING),
                    ls(vec![
                        Value::Float(0.0),
                        Value::Float(0.0),
                        Value::Float(0.0),
                    ]),
                    Value::Float(5.0),
                ],
            )],
        ),
        lower(
            "OBJECTIVE4",
            3,
            vec![BlockDirective::new(
                "ANIM_STATE",
                vec![
                    s("ANIM"),
                    ls(vec![
                        s("NAME"),
                        ls(vec![s("anim")]),
                        s("STATE"),
                        ls(vec![s("EXECUTED")]),
                    ]),
                ],
            )],
        ),
    ];

    for condition in &conditions {
        let state_before = state.clone();
        let facts_before = facts.clone();
        // Repeated evaluation is also a no-op: a predicate that latched
        // anything into the session would show up here.
        for _ in 0..3 {
            state.holds(condition, &facts);
        }
        assert_eq!(state, state_before, "evaluation wrote mission state");
        assert_eq!(facts, facts_before, "evaluation wrote facts");
    }

    // The DEDG predicate is a condition, not an effect: the original's
    // member-field normalization has no home inside it.
    assert!(matches!(
        conditions[1],
        Condition::All(ref items)
        if items.iter().any(|item| matches!(item, Condition::EnemyGroupDepletion { .. }))
    ));
    assert!(
        conditions[1]
            .residual_unknowns()
            .contains(&DEDG_MEMBER_FIELD_REWRITES)
    );
}

// --------------------------------------------------------------- refusals ----

/// AC4: a residual unknown that changes the predicate refuses the block
/// **by block and key**, and the refusal is visible to a caller as the field
/// it reports under `objective_condition`.
#[test]
fn accept_m01_lc_lowering_conditions_refusals_name_the_block_and_the_key() {
    use cs_content::mission_control::LoweringRequirementKind;

    let cases: Vec<(Vec<BlockDirective>, &str)> = vec![
        (
            vec![BlockDirective::bare("DANGER_ZONES_COMPLETED")],
            "DANGER_ZONES_COMPLETED",
        ),
        (vec![BlockDirective::bare("COUNTER")], "COUNTER"),
        (
            vec![BlockDirective::bare("COMPLETION_COUNT")],
            "COMPLETION_COUNT",
        ),
        (
            vec![BlockDirective::new(
                "TRAVELERS",
                vec![s("player"), s("DEPARTING"), s("anchor"), Value::Float(10.0)],
            )],
            "TRAVELERS",
        ),
        (
            vec![BlockDirective::new(
                "TRAVELERS",
                vec![
                    Value::Int(3),
                    s(TRAVELERS_APPROACHING),
                    s("anchor"),
                    Value::Float(10.0),
                ],
            )],
            "TRAVELERS",
        ),
        (
            vec![BlockDirective::new(
                "ANIM_STATE",
                vec![
                    s("ANIM"),
                    ls(vec![
                        s("NAME"),
                        ls(vec![s("anim")]),
                        s("STATE"),
                        ls(vec![s("CORRUPT")]),
                    ]),
                ],
            )],
            "ANIM_STATE",
        ),
    ];

    let requirement = LoweringRequirementKind::ObjectiveCondition.code();
    for (directives, key) in cases {
        let refusal: ConditionRefusal = lower_block_condition("OBJECTIVE9", 8, &directives)
            .expect_err(&format!("{key} must refuse rather than guess"));
        assert_eq!(refusal.block(), "OBJECTIVE9", "the refusal names the block");
        assert_eq!(refusal.key(), key, "the refusal names the key");
        assert!(!refusal.detail().is_empty(), "the refusal says why");

        // The field a caller reports: the requirement, the block and the key
        // in one string, so `.03`'s unmet `objective_condition` row can name
        // the field instead of only the requirement.
        let field = refusal.field();
        assert!(
            field.starts_with(&format!("{requirement}: ")),
            "{field} must start with the requirement code {requirement}"
        );
        assert!(field.contains("OBJECTIVE9"), "{field} names the block");
        assert!(field.contains(key), "{field} names the key");
        // A caller reports it in the unmet row's `unmeasured_fields`.
        let row: Vec<String> = vec![field];
        assert_eq!(row.len(), 1);
    }

    // A refusal is never lowered, and the record-level entry keeps it out of
    // the conditions a program would be built from.
    let answers = lower_record(&[RawBlock::Read {
        block: "OBJECTIVE9".to_owned(),
        index: 8,
        directives: vec![BlockDirective::bare("DANGER_ZONES_COMPLETED")],
    }]);
    assert!(matches!(&answers[0], BlockCondition::Refused(_)));
    assert!(answers[0].condition().is_none());
    let rendered = answers[0].to_string();
    assert!(
        rendered
            .starts_with("refused: objective_condition: `OBJECTIVE9` `DANGER_ZONES_COMPLETED`: "),
        "the rendered refusal names the requirement, the block and the key: {rendered}"
    );
    assert!(
        rendered.contains("would be a guess at its predicate"),
        "the rendered refusal says why: {rendered}"
    );

    // An unarmed DEDG is not a refusal: it is the record's own "no armed
    // evaluator" spelling, and the block completes when awake.
    let unarmed = lower(
        "OBJECTIVE10",
        9,
        vec![BlockDirective::new(
            "DEDG",
            vec![Value::Int(-1), Value::Int(-1)],
        )],
    );
    assert_eq!(unarmed, Condition::ObjectiveAwake { index: 9 });
}

// ------------------------------------------------------------- fail-closed ---

/// AC5: an empty record produces no conditions, and a block the directive
/// walk cannot read stays a `BlockRefusal` — with the same code the
/// measurement side publishes.
#[test]
fn accept_m01_lc_lowering_conditions_fail_closed_on_empty_and_unreadable_blocks() {
    // The empty record: no block, no condition, nothing invented.
    let empty = control_record(vec![]);
    assert_eq!(mission_control::measure_control_record(&empty).blocks(), 0);
    assert!(raw_blocks(&empty).is_empty());
    assert!(lower_record(&raw_blocks(&empty)).is_empty());
    assert!(lower_record(&[]).is_empty());

    // One block that is not a list, one whose first child is not a text key,
    // and one that reads.
    let document = control_record(vec![
        ("OBJECTIVE1".to_owned(), ZrdValue::Int(5)),
        (
            "OBJECTIVE2".to_owned(),
            zrd_list(vec![ZrdValue::Int(9), zrd_text("BEGIN_DORMANT")]),
        ),
        (
            "OBJECTIVE3".to_owned(),
            zrd_list(vec![
                zrd_text("BEGIN_DORMANT"),
                zrd_list(vec![ZrdValue::Float(-1.0)]),
            ]),
        ),
    ]);
    let record = mission_control::measure_control_record(&document);
    assert_eq!(record.blocks(), 3);
    let measured: Vec<&MeasuredBlockRefusal> = record.refusals().iter().collect();
    assert_eq!(
        measured
            .iter()
            .map(|refusal| (refusal.block().to_owned(), refusal.code()))
            .collect::<Vec<_>>(),
        vec![
            ("OBJECTIVE1".to_owned(), "block_not_a_list"),
            ("OBJECTIVE2".to_owned(), "key_not_text"),
        ],
        "the census refuses exactly the two unreadable blocks"
    );

    let blocks = raw_blocks(&document);
    assert_eq!(blocks.len(), 3);
    let answers = lower_record(&blocks);
    assert_eq!(answers.len(), 3);

    // The refusals survive lowering, with the measurement side's own codes.
    for (answer, refusal) in answers.iter().zip(&measured) {
        let BlockCondition::Unreadable(mine) = answer else {
            panic!("{answer} must stay a refusal, not become a condition");
        };
        assert_eq!(mine.block(), refusal.block());
        assert_eq!(mine.code(), refusal.code(), "the codes must not drift");
        assert!(answer.condition().is_none());
    }
    // The third block reads and lowers — the refusals did not swallow it.
    assert!(answers[2].condition().is_some());

    // And the mirror is honest for both variants, not only the ones this
    // record happens to produce.
    assert_eq!(
        BlockRefusal::KeyNotText {
            block: "OBJECTIVE7".to_owned(),
            index: 3,
        }
        .code(),
        "key_not_text"
    );
    assert_eq!(
        BlockRefusal::BlockNotAList {
            block: "OBJECTIVE7".to_owned(),
        }
        .to_string(),
        "OBJECTIVE7: block_not_a_list: the block is not a directive list"
    );
}

// ------------------------------------------------------------------ helpers ---

/// One authored control record: the root one-element list holding the flat
/// record, exactly the shape the production reader unwraps.
fn control_record(fields: Vec<(String, ZrdValue)>) -> ZrdValue {
    let mut children = Vec::new();
    for (key, value) in fields {
        children.push(zrd_text(&key));
        children.push(value);
    }
    zrd_list(vec![zrd_list(children)])
}

fn zrd_text(text: &str) -> ZrdValue {
    ZrdValue::Text(text.to_owned())
}

fn zrd_list(children: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(children)
}
