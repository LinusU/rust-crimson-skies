//! Acceptance stage M07-B: the mission-specific compatibility gaps of the
//! seventh mission (`missions/M07.md`, work order `M07-B`).
//!
//! M07-A bound M07's identities and left the mission program unmeasured. The
//! machinery that measures a control program (the `.zrd` reader, the census,
//! the directive dispositions and the record → `RawProgram` adapter) is shared
//! and was built for M01; this stage runs it over M07's own reader archive and
//! pins what is **different** at M07, so the gaps are recorded as measurements
//! and not discovered later as a silent failure:
//!
//! * M07's control program is `objectives.zrd` with 61 numbered blocks and 231
//!   directive sites, selected by the content rule and not by its name;
//! * its 29-key vocabulary is **fully disposed** — two terminal outcomes, 27
//!   measured keys, none refused and no unclassified record key — so every gap
//!   below is a lowering gap, not a measurement gap;
//! * the record lowers **completely** and both requirements are met: the
//!   `ANIM_STATE` half closed with M04-B-FU1 (#806), which generalized the
//!   list-argument mechanism to the operand list an `ANIM_STATE` evaluator
//!   reads — all nine sites bind and their two-to-five-record operand lists
//!   append into each block's one evaluator — and the `DANGER_ZONES_`
//!   `COMPLETED` half closed with M07-B-FU1 (#813), which measured the flag
//!   bytes' writer in the engine image and lowers the three sites (blocks 37,
//!   39 and 41) to the counted-flag predicate, so no condition refuses and
//!   `MissionProgram::validate` accepts the program
//!   (`docs/findings/2026-10-09-m07-b-fu1-danger-zones-flag-writer.md`);
//! * the sheet's three regression priorities are located in the measured
//!   record: the moving-subject proximity evaluator (`TRAVELERS`), the
//!   animation gates that name the trailer segments, `got_pickford` and
//!   `hooked_to_klondike`, and the `PLAYER_INIT` record field — while the
//!   record spells **no** player-vehicle change and **no** input-ownership
//!   transfer, which stay in the mission-program members this stage does not
//!   decode;
//! * the objective graph is closed: all 102 cross-objective addresses lie in
//!   `1..=61` (the parser `dec`s them into zero-based indices), both terminal
//!   blocks start dormant, and their incoming wake/kill/nap edges are pinned.
//!
//! No behaviour is invented here: the runtime halves of the sheet's priorities
//! need ordinary-play observation (M07-C), and the campaign gate stays closed
//! until every measured row lowers — M07's own row does, while the rows whose
//! gaps are still open keep `campaign_ready()` false
//! (`docs/findings/2026-10-09-m07-b-compatibility-gaps.md`, superseded on the
//! danger-zones gap by
//! `docs/findings/2026-10-09-m07-b-fu1-danger-zones-flag-writer.md`).
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`; the two synthetic
//! tests run in CI.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_app::control_lowering::lower_control_record;
use cs_app::mission_control::{
    RetailControlCensus, RetailControlRow, survey_mission_control_programs,
};
use cs_assets::install::sha256;
use cs_content::campaign_bindings::{MissionControlBinding, SourceContext};
use cs_content::mission_control::{
    AnimList, CallOutcome, ConditionOutcome, ControlRecordField, DirectiveDisposition,
    DirectiveOperation, TerminalOutcome, measure_control_record,
};
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{ZrdValue, objective_record, zrd_flat_fields};
use cs_formats::script_raw::discover_container;
use cs_script::ir::{
    AnimationState, Condition, DANGER_ZONE_CROSSING_TEST_UNTRACED, MAX_VALUE_ITEMS,
};
use cs_script::runtime::{MissionFacts, MissionState, ObjectiveLifecycle, SessionGeneration};
use cs_types::content::{ContentId, ContentKind};
use cs_types::install::RelativePath;

use crate::common::{label, load_inventory};

/// The census row label of the mission (`ZBD/C2/M02/zrdr.zbd`, F13-B's
/// mission-scope rule).
const MISSION: &str = "zbd/c2/m02";

/// The numbered blocks of the control member.
const BLOCKS: u32 = 61;
/// The directive sites of the control member.
const SITES: u32 = 231;
/// The distinct directive keys of the control member.
const KEYS: usize = 29;

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M07-B needs the retail capability; run this suite with \
             `--include-ignored` and CS_GAME_DIR pointing at the read-only installation"
        )
    }))
}

/// The census, built once for the whole suite (it decodes every reader archive).
fn census() -> &'static RetailControlCensus {
    static CENSUS: OnceLock<RetailControlCensus> = OnceLock::new();
    CENSUS.get_or_init(|| {
        survey_mission_control_programs(&game_dir()).expect("the census runs on the installation")
    })
}

/// The source context, read once for the whole suite.
fn context() -> &'static SourceContext {
    static CONTEXT: OnceLock<SourceContext> = OnceLock::new();
    CONTEXT.get_or_init(|| {
        SourceContext::read(&game_dir()).expect("the installation yields a source context")
    })
}

/// M07's declared discovery title, from the committed inventory.
fn m07_title() -> String {
    load_inventory()
        .iter()
        .find(|(work_order, _)| work_order.as_str() == "M07")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M07 work order")
}

/// M07's control binding, derived fresh through production code.
fn control_binding() -> MissionControlBinding {
    context()
        .control_program(label("M07"), &m07_title())
        .expect("M07's control program binds through the measured rule")
}

/// M07's row in the retail control census: the same installation measured a
/// second time through `cs_app::mission_control`.
fn census_row() -> &'static RetailControlRow {
    static ROW: OnceLock<RetailControlRow> = OnceLock::new();
    ROW.get_or_init(|| {
        census()
            .row(MISSION)
            .expect("M07's reader archive is measured by the census")
            .clone()
    })
}

/// The control member's decoded document, re-read from the archive through
/// production discovery — an independent walk from the binding's and the
/// census's, so the graph assertions below cannot be satisfied by either
/// one's own output. Returns the document and `(name, offset, len, blocks)`
/// for every member the archive offers.
fn control_document() -> (ZrdValue, Vec<(String, u64, u64, u32)>) {
    let binding = control_binding();
    let bytes = std::fs::read(game_dir().join(&binding.program_asset))
        .expect("M07's reader archive reads from disk");
    let logical = binding.program_asset.to_lowercase();
    let relative = RelativePath::new(&logical).expect("the archive path is relative");
    let container_key = relative.logical_key();
    let discovery = discover_container(&container_key, &relative, &bytes);
    assert!(
        discovery.findings().is_empty(),
        "the archive locates without findings: {:?}",
        discovery.findings()
    );
    let mut document = None;
    let mut members = Vec::new();
    for program in discovery.programs() {
        let Some(name) = program.locator().member() else {
            continue;
        };
        let decoded = cs_content::stunts::decode_zrd(program.bytes())
            .unwrap_or_else(|error| panic!("member {name} decodes: {error}"));
        let span = program.locator().span();
        members.push((
            name.to_owned(),
            span.offset,
            span.len,
            cs_content::mission_control::objective_blocks_of(
                &cs_content::mission_control::DecodedMember::new(name, decoded.clone()),
            ),
        ));
        if name == binding.control_member {
            document = Some(decoded);
        }
    }
    (
        document.expect("the member the rule chose is in the archive"),
        members,
    )
}

/// One directive of a block as authored: key and, unless bare, its argument list.
struct Directive {
    key: String,
    args: Option<Vec<ZrdValue>>,
}

/// Walks a decoded control record into `(block number, directives)`, reading
/// the grammar independently of the census: a text key, then its argument list
/// if the next child is a list.
fn blocks_of(document: &ZrdValue) -> Vec<(u32, Vec<Directive>)> {
    let mut blocks = Vec::new();
    for (key, value) in zrd_flat_fields(objective_record(document)) {
        let Some(number) = objective_block_number(key) else {
            continue;
        };
        let children = value.as_list().expect("every M07 block is a list");
        let mut directives = Vec::new();
        let mut index = 0;
        while index < children.len() {
            let ZrdValue::Text(name) = &children[index] else {
                panic!("OBJECTIVE{number} child {index} is not a directive key");
            };
            index += 1;
            let args = children
                .get(index)
                .and_then(ZrdValue::as_list)
                .map(<[ZrdValue]>::to_vec);
            if args.is_some() {
                index += 1;
            }
            directives.push(Directive {
                key: name.clone(),
                args,
            });
        }
        blocks.push((number, directives));
    }
    blocks
}

/// The blocks whose directives spell `key`, with the site's argument list.
fn sites<'a>(blocks: &'a [(u32, Vec<Directive>)], key: &str) -> Vec<(u32, &'a [ZrdValue])> {
    blocks
        .iter()
        .flat_map(|(number, directives)| {
            directives
                .iter()
                .filter(move |directive| directive.key == key)
                .map(move |directive| (*number, directive.args.as_deref().unwrap_or(&[])))
        })
        .collect()
}

/// The integer addresses a directive spells in its argument list.
fn addresses(args: &[ZrdValue]) -> Vec<i64> {
    args.iter()
        .filter_map(|value| match value {
            ZrdValue::Int(int) => Some(i64::from(*int)),
            _ => None,
        })
        .collect()
}

/// The numbered block a condition refusal names: `OBJECTIVE13` is the block
/// the record spells it as.
fn refused_block(text: &str) -> u32 {
    let start = text
        .find("`OBJECTIVE")
        .expect("a condition refusal names its block")
        + "`OBJECTIVE".len();
    let digits: String = text[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().expect("the block number is a number")
}

// ---------------------------------------------------------------------------
// Retail: what M07's control program is
// ---------------------------------------------------------------------------

/// **The control program is the member that declares the numbered blocks, and
/// it is the program M07-A bound.**
///
/// The work-order join (`SourceContext::control_program`) and the census
/// (`zbd/c2/m02`) name one archive — the span and digest
/// `missions/bindings/M07.json` cites — and one member: of the 19 members of
/// `ZBD/C2/M02/zrdr.zbd` exactly one declares numbered `OBJECTIVE<N>` blocks.
/// A longer member exists and the control member is the archive's eighth, so
/// neither size nor position is the rule. The member's span lies inside the
/// archive and its own digest re-derives from its own bytes.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m07_b_the_control_program_is_the_member_that_declares_the_blocks() {
    let binding = control_binding();
    assert_eq!(binding.mission.as_str(), "mission/ch2-m02");
    assert_eq!(binding.program_id.as_str(), "script/c2-m02-zrdr");
    assert_eq!(binding.program_asset, "ZBD/C2/M02/zrdr.zbd");
    assert_eq!(
        binding.program_sha256, "6154e4ea47b0a5261ef047e4a6dd2732973e868858fbba7761ebb5f83f580a5d",
        "the reader archive is the program M07-A bound"
    );

    let row = census_row();
    assert_eq!(row.container, binding.program_asset);
    assert_eq!(
        row.container_sha256, binding.program_sha256,
        "the census and the work-order join read one archive"
    );
    assert_eq!(row.members.len(), 19);
    let with_blocks: Vec<&str> = row
        .members
        .iter()
        .filter(|member| member.objective_blocks > 0)
        .map(|member| member.name.as_str())
        .collect();
    assert_eq!(with_blocks, ["objectives.zrd"]);
    let control: Vec<&str> = row
        .members
        .iter()
        .filter(|member| member.is_control)
        .map(|member| member.name.as_str())
        .collect();
    assert_eq!(control, ["objectives.zrd"]);
    assert_eq!(binding.control_member, "objectives.zrd");
    assert_eq!(
        (binding.control_offset, binding.control_length),
        (18_437, 16_463)
    );

    let longest = row
        .members
        .iter()
        .max_by_key(|member| member.len)
        .expect("the archive has members");
    assert!(
        longest.len > binding.control_length,
        "a longer member ({}) exists, and it carries no blocks",
        longest.name
    );
    assert!(
        row.members[0].name != binding.control_member,
        "the control member is not the archive's first member"
    );

    let bytes = std::fs::read(game_dir().join(&binding.program_asset))
        .expect("M07's reader archive reads from disk");
    assert_eq!(
        sha256(&bytes).to_hex(),
        binding.program_sha256,
        "the archive digest re-derives from the archive's own bytes"
    );
    assert_eq!(bytes.len() as u64, binding.program_length);
    let start = binding.control_offset as usize;
    let end = start + binding.control_length as usize;
    let member_bytes = &bytes[start..end];
    assert_eq!(
        sha256(member_bytes).to_hex(),
        binding.control_sha256,
        "the control member's digest re-derives from the member's own bytes"
    );

    let record = binding.record;
    assert_eq!((record.blocks(), record.sites()), (BLOCKS, SITES));
    assert_eq!(record.keys().len(), KEYS);
    assert_eq!(
        (record.blocks(), record.sites(), record.keys().len()),
        (
            row.record().expect("the census measured M07").blocks(),
            row.record().expect("the census measured M07").sites(),
            row.record().expect("the census measured M07").keys().len()
        ),
        "the census and the work-order join measure one record"
    );

    let (document, members) = control_document();
    assert_eq!(
        members,
        row.members
            .iter()
            .map(|member| (
                member.name.clone(),
                member.offset,
                member.len,
                member.objective_blocks
            ))
            .collect::<Vec<_>>(),
        "the independent walk enumerates the same members at the same spans"
    );
    let walked = blocks_of(&document);
    assert_eq!(walked.len() as u32, BLOCKS);
    let sites_walked: usize = walked.iter().map(|(_, d)| d.len()).sum();
    assert_eq!(sites_walked as u32, SITES, "the independent walk agrees");
    assert_eq!(
        measure_control_record(&document),
        record,
        "an independent measure of the same document reproduces the record"
    );
}

/// **Every directive key M07 spells has exactly one disposition and none is
/// refused.**
///
/// Two keys are terminal outcomes (`INSTANTWIN`, `INSTANTLOSS`, one bare site
/// each), the other 27 have a measured effect, and the sites are accounted
/// for: the keys' sites sum to the record's. Unlike M02, M07 spells no
/// record-level key outside the measured vocabulary, and no block is
/// unreadable.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m07_b_the_vocabulary_is_fully_disposed_and_no_m07_key_is_refused() {
    let record = census_row().record().expect("M07 has a control program");
    let mut outcomes = BTreeMap::new();
    let mut refused = Vec::new();
    let mut measured = 0;
    for key in record.keys() {
        match key.disposition() {
            DirectiveDisposition::TerminalOutcome { outcome } => {
                outcomes.insert(key.key.clone(), outcome);
            }
            DirectiveDisposition::Measured(_) => measured += 1,
            DirectiveDisposition::Unmeasured { reason } => {
                refused.push((key.key.clone(), reason, key.sites));
            }
        }
    }
    assert_eq!(
        outcomes,
        BTreeMap::from([
            ("INSTANTLOSS".to_owned(), TerminalOutcome::Failed),
            ("INSTANTWIN".to_owned(), TerminalOutcome::Succeeded),
        ])
    );
    assert!(
        refused.is_empty(),
        "M07's whole vocabulary is measured: {refused:?}"
    );
    assert_eq!(measured, 27);
    let sites: u32 = record.keys().iter().map(|key| key.sites).sum();
    assert_eq!(sites, SITES, "no site is dropped from the accounting");
    assert!(
        record.refusals().is_empty(),
        "every block is a readable list"
    );
    assert!(
        record.unclassified_record_keys().is_empty(),
        "M07 spells no record key outside the measured vocabulary"
    );
    assert_eq!(
        record.record_fields(),
        [
            (ControlRecordField::MissionTimer, 1),
            (ControlRecordField::PlayerInit, 1),
            (ControlRecordField::AnimList(AnimList::Restore), 1),
            (ControlRecordField::AnimList(AnimList::Execute), 1),
            (ControlRecordField::AnimList(AnimList::Invalidate), 1),
        ]
    );
}

/// **Every block lowers: the nine `ANIM_STATE` sites bind and the three
/// `DANGER_ZONES_COMPLETED` sites carry their counted-flag predicate.**
///
/// All 231 sites bind — every `ANIM_STATE` site carries its operand list as
/// one list argument, so the ten-argument shape (five spec records at block
/// 60) registers like every other measured shape and the whole key is in the
/// registry. 61 of 61 conditions lower: the five multi-record `ANIM_STATE`
/// sites (blocks 13, 16, 20, 30 and 60) append every spelled pair with
/// `required` counting them, and the three `DANGER_ZONES_COMPLETED` sites
/// (blocks 37, 39 and 41) lower to the flag-count predicate M07-B-FU1 (#813)
/// measured in the engine image. A program stands, `MissionProgram::validate`
/// accepts it, `objective_condition` and `call_arguments` are met, and the row
/// is complete — while the campaign gate stays closed on the rows whose gaps
/// are still open.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m07_b_every_block_lowers_including_the_three_danger_zones_sites() {
    let row = census_row();
    let attempt = row.lowering_attempt().expect("M07 has a lowering attempt");
    let lowered = attempt.attempt();
    assert_eq!(lowered.mission.as_deref(), Ok("mission/ch2-m02"));
    assert_eq!(lowered.objectives, BLOCKS);
    assert_eq!(lowered.calls.len() as u32, SITES);

    let (document, _) = control_document();
    let blocks = blocks_of(&document);

    // Calls: no site refuses — the nine ANIM_STATE sites bind like every
    // other site, each carrying its operand list as the one argument.
    let refused_calls: Vec<&str> = lowered
        .calls
        .iter()
        .filter_map(|call| match call {
            CallOutcome::Refused(text) => Some(text.as_str()),
            CallOutcome::Bound => None,
        })
        .collect();
    assert!(
        refused_calls.is_empty(),
        "every site binds: {refused_calls:?}"
    );
    assert!(
        lowered.unbound_keys.is_empty(),
        "no key fails registration — block 60's ten-operand list is one list \
         argument, inside the bound: {:?}",
        lowered.unbound_keys
    );
    let anim_blocks: Vec<u32> = sites(&blocks, "ANIM_STATE")
        .iter()
        .map(|(number, _)| *number)
        .collect();
    assert_eq!(anim_blocks.len(), 9, "M07 spells nine ANIM_STATE sites");
    let raw = attempt.raw_program().expect("the program assembled");
    let anim_calls: Vec<_> = raw
        .objectives
        .iter()
        .flat_map(|objective| objective.calls.iter())
        .filter(|call| call.name == "ANIM_STATE")
        .collect();
    assert_eq!(anim_calls.len(), 9);
    assert!(
        anim_calls
            .iter()
            .all(|call| matches!(call.args.as_slice(), [cs_script::ir::Value::List(_)])),
        "each carries its operand list as the one argument"
    );

    // Conditions: no block refuses. The three danger-zones sites lower to
    // the counted-flag evaluator, and every ANIM_STATE block still lowers its
    // appended pairs counted into `required` (block 60's five below).
    let refused_conditions: Vec<&str> = lowered
        .conditions
        .iter()
        .filter_map(|outcome| match outcome {
            ConditionOutcome::Refused(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        refused_conditions.is_empty(),
        "every block lowers, the three danger-zones sites included: {refused_conditions:?}"
    );
    assert_eq!(
        lowered.conditions.len() as u32,
        BLOCKS,
        "every block was walked"
    );

    let (required, pairs) = animation_evaluator(&raw.objectives[59].condition);
    assert_eq!(pairs.len(), 5, "block 60 appends all five descriptors");
    assert_eq!(
        required, 5,
        "no COMPLETION_COUNT: required counts the pairs"
    );

    let validation = lowered
        .validation
        .as_ref()
        .expect("a program stood, so validate ran");
    assert!(
        validation.is_empty(),
        "and validate accepts every lowered condition: {validation:?}"
    );
    let lowering = row.lowering().expect("the accounting exists");
    let unmet: Vec<String> = lowering.unmet().map(|r| r.kind.code().to_owned()).collect();
    assert!(
        unmet.is_empty(),
        "the conditions and the calls both meet their requirements: {unmet:?}"
    );
    assert!(row.is_complete());
    assert!(lowering.complete());
}

/// **The sheet's three regression priorities, located in the measured record.**
///
/// *Moving pickup / moving subject:* the record's proximity evaluator is
/// `TRAVELERS` (blocks 44 and 45): two sites that spell the same shape — a
/// named subject, the `APPROACHING` polarity, a three-float anchor, a float
/// radius, a required count of one and `DELETE_ON_SUCCESS` — with distinct
/// subjects and every other operand equal. The animation gates that surround
/// the pickup name the trailer segments (`trailer_seg1`…`trailer_seg5`) and
/// `got_pickford` / `hooked_to_klondike`, all in the `EXECUTED` state, and the
/// record's per-block spec counts are pinned. The initial player
/// configuration exists as the `PLAYER_INIT` record field, once.
///
/// *Forced plane swap and persistent input ownership:* the 29-key vocabulary
/// is pinned below in full, and it contains no player-vehicle change and no
/// input-ownership directive — those live, if anywhere, in the mission-program
/// members (`pickford_pickup.zrd`, `car_truck_pickford.zrd`) that declare no
/// blocks and that this stage does not decode. Their absence from the control
/// record is a measured fact, not an assumption.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m07_b_the_sheet_priorities_resolve_to_measured_operations() {
    let record = census_row().record().expect("M07 has a control program");
    let operation = |key: &str| -> DirectiveOperation {
        match record
            .key(key)
            .unwrap_or_else(|| panic!("{key} is missing"))
            .disposition()
        {
            DirectiveDisposition::Measured(measured) => measured.operation,
            other => panic!("{key} is not measured: {other:?}"),
        }
    };
    assert_eq!(operation("TRAVELERS"), DirectiveOperation::Travelers);
    assert_eq!(operation("ANIM_STATE"), DirectiveOperation::AnimationStates);
    assert_eq!(
        operation("DANGER_ZONES_COMPLETED"),
        DirectiveOperation::DangerZoneFlags
    );
    assert_eq!(operation("DEDG"), DirectiveOperation::EnemyGroupDepletion);
    assert_eq!(operation("INACTIVE1"), DirectiveOperation::InactiveMembers);
    assert_eq!(
        operation("INACTIVE_COMPLETION_COUNT"),
        DirectiveOperation::InactiveThreshold
    );
    assert_eq!(
        operation("IDENTITY"),
        DirectiveOperation::PresentationIdentity
    );
    assert_eq!(operation("BEGIN_DORMANT"), DirectiveOperation::DormantStart);
    assert_eq!(operation("START_TAXI"), DirectiveOperation::ReleaseTaxi);
    assert_eq!(operation("SET_AI_TEAM"), DirectiveOperation::AssignTeam);
    assert_eq!(operation("WAKE_ANIM"), DirectiveOperation::WakeAnimation);
    assert_eq!(operation("WAKEUP_ENEMIES"), DirectiveOperation::WakeEnemies);
    assert_eq!(
        operation("COMPLETED_STOPPOINT"),
        DirectiveOperation::AdvanceStopPoint
    );
    assert_eq!(
        operation("SET_HELP_LABEL"),
        DirectiveOperation::SetHelpLabel
    );
    assert_eq!(
        operation("WAKEUP_SOUND_GROUP"),
        DirectiveOperation::WakeSoundGroup
    );
    assert_eq!(
        operation("COMPLETED_SOUND_GROUP"),
        DirectiveOperation::CompletedSoundGroup
    );
    assert_eq!(
        operation("WAKE_OBJECTIVE_WHEN_I_COMPLETE"),
        DirectiveOperation::WakeObjectives
    );
    assert_eq!(
        operation("KILL_OBJECTIVE_WHEN_I_COMPLETE"),
        DirectiveOperation::KillObjectives
    );
    assert_eq!(
        operation("NAP_OBJECTIVE_WHEN_I_COMPLETE"),
        DirectiveOperation::NapObjective
    );
    assert_eq!(
        operation("ADD_OBJECTIVE_TARGET"),
        DirectiveOperation::SetTargetFlag {
            objective: true,
            set: true
        }
    );
    assert_eq!(
        operation("REMOVE_OBJECTIVE_TARGET"),
        DirectiveOperation::SetTargetFlag {
            objective: true,
            set: false
        }
    );
    assert_eq!(
        operation("ADD_OTHER_TARGET"),
        DirectiveOperation::SetTargetFlag {
            objective: false,
            set: true
        }
    );
    assert_eq!(
        operation("REMOVE_OTHER_TARGET"),
        DirectiveOperation::SetTargetFlag {
            objective: false,
            set: false
        }
    );

    // The complete vocabulary: what the record spells, in full, so a key that
    // appears, disappears or is renamed fails here.
    let mut keys: Vec<String> = record.keys().iter().map(|key| key.key.clone()).collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "ADD_OBJECTIVE_TARGET",
            "ADD_OTHER_TARGET",
            "ANIM_STATE",
            "BEGIN_DORMANT",
            "COMPLETED_SOUND_GROUP",
            "COMPLETED_STOPPOINT",
            "DANGER_ZONES_COMPLETED",
            "DEDG",
            "IDENTITY",
            "INACTIVE1",
            "INACTIVE2",
            "INACTIVE3",
            "INACTIVE4",
            "INACTIVE5",
            "INACTIVE_COMPLETION_COUNT",
            "INSTANTLOSS",
            "INSTANTWIN",
            "KILL_OBJECTIVE_WHEN_I_COMPLETE",
            "NAP_OBJECTIVE_WHEN_I_COMPLETE",
            "REMOVE_OBJECTIVE_TARGET",
            "REMOVE_OTHER_TARGET",
            "SET_AI_TEAM",
            "SET_HELP_LABEL",
            "START_TAXI",
            "TRAVELERS",
            "WAKEUP_ENEMIES",
            "WAKEUP_SOUND_GROUP",
            "WAKE_ANIM",
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
        ]
    );

    let (document, members) = control_document();
    let blocks = blocks_of(&document);

    // The proximity sites: two, distinct subjects, identical shape and
    // operands beside the subject. The coordinates are not quoted here — the
    // equality of the two sites is the assertion.
    let travelers = sites(&blocks, "TRAVELERS");
    assert_eq!(travelers.len(), 2);
    let (first_block, first) = travelers[0];
    let (second_block, second) = travelers[1];
    assert_eq!((first_block, second_block), (44, 45));
    assert_eq!(first.len(), 6);
    assert_eq!(second.len(), 6);
    let subject = |args: &[ZrdValue]| match &args[0] {
        ZrdValue::Text(text) => text.clone(),
        other => panic!("the subject is text, not {other:?}"),
    };
    assert_eq!(subject(first), "secfury_5");
    assert_eq!(subject(second), "secfury_6");
    assert_eq!(first[1], ZrdValue::Text("APPROACHING".to_owned()));
    assert_eq!(second[1], ZrdValue::Text("APPROACHING".to_owned()));
    assert_eq!(first[2], second[2], "one anchor");
    assert_eq!(first[3], second[3], "one radius");
    assert_eq!(first[4], ZrdValue::Int(1));
    assert_eq!(second[4], ZrdValue::Int(1));
    assert_eq!(first[5], ZrdValue::Text("DELETE_ON_SUCCESS".to_owned()));
    assert_eq!(second[5], ZrdValue::Text("DELETE_ON_SUCCESS".to_owned()));
    assert!(
        matches!(first[2], ZrdValue::List(ref anchor) if anchor.len() == 3
            && anchor.iter().all(|v| matches!(v, ZrdValue::Float(_)))),
        "the anchor is a three-float point"
    );
    assert!(
        matches!(first[3], ZrdValue::Float(_)),
        "the radius is a float"
    );

    // The animation gates: which blocks, how many spec records each, and the
    // names and states the record spells.
    let anim: Vec<(u32, usize)> = sites(&blocks, "ANIM_STATE")
        .iter()
        .map(|(number, args)| (*number, args.len() / 2))
        .collect();
    assert_eq!(
        anim,
        [
            (9, 1),
            (13, 2),
            (16, 3),
            (20, 4),
            (26, 1),
            (30, 3),
            (31, 1),
            (60, 5),
            (61, 1)
        ],
        "M07 is the first measured mission whose ANIM_STATE sites spell up to five records"
    );
    let mut names: BTreeMap<String, usize> = BTreeMap::new();
    for (_, args) in sites(&blocks, "ANIM_STATE") {
        for spec in args.chunks(2) {
            assert_eq!(spec[0], ZrdValue::Text("ANIM".to_owned()));
            let ZrdValue::List(fields) = &spec[1] else {
                panic!("the spec record is a list");
            };
            assert_eq!(fields.len(), 4);
            assert_eq!(fields[0], ZrdValue::Text("NAME".to_owned()));
            let ZrdValue::List(name) = &fields[1] else {
                panic!("the NAME value is a list");
            };
            let ZrdValue::Text(text) = &name[0] else {
                panic!("the NAME value holds text");
            };
            *names.entry(text.clone()).or_insert(0) += 1;
            assert_eq!(fields[2], ZrdValue::Text("STATE".to_owned()));
            let ZrdValue::List(state) = &fields[3] else {
                panic!("the STATE value is a list");
            };
            assert_eq!(
                state[0],
                ZrdValue::Text("EXECUTED".to_owned()),
                "every gate waits for the EXECUTED state"
            );
        }
    }
    assert_eq!(
        names,
        BTreeMap::from([
            ("got_pickford".to_owned(), 1),
            ("hooked_to_klondike".to_owned(), 1),
            ("trailer_seg1".to_owned(), 7),
            ("trailer_seg2".to_owned(), 5),
            ("trailer_seg3".to_owned(), 4),
            ("trailer_seg4".to_owned(), 2),
            ("trailer_seg5".to_owned(), 1),
        ]),
        "the gates name five trailer segments (used by up to five sites) plus `got_pickford` \
         and `hooked_to_klondike` once each"
    );

    // The members that would carry the pickup and vehicle-swap program
    // declare no blocks: named here so a later stage knows where to look.
    let no_blocks = |name: &str| {
        members
            .iter()
            .find(|(member, _, _, _)| member == name)
            .map(|(_, _, _, blocks)| *blocks)
            .unwrap_or_else(|| panic!("{name} is in the archive"))
    };
    assert_eq!(no_blocks("pickford_pickup.zrd"), 0);
    assert_eq!(no_blocks("car_truck_pickford.zrd"), 0);
    assert_eq!(no_blocks("pickups.zrd"), 0);
}

/// **The objective graph is closed and the two terminal blocks are gated.**
///
/// All 102 cross-objective addresses (50 wake, 39 kill, 13 nap) lie in
/// `1..=61`; the original `dec`s them into zero-based indices at parse
/// (`docs/findings/2026-10-06-m01-lc-directive-b-…`), so every address names a
/// block of this record and none points past it. Both terminal blocks start
/// dormant with no timed wake (`BEGIN_DORMANT -1`), so neither can fire on its
/// own clock: block 25 (`INSTANTLOSS`) is reached only through block 24's nap,
/// and block 27 (`INSTANTWIN`) is named by block 24's kill list and block 61's
/// wake list. The three PRIMARY objectives carry distinct HUD slots and one
/// SECONDARY objective carries slot 11.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m07_b_the_objective_graph_is_closed_and_the_terminal_blocks_are_gated() {
    let (document, _) = control_document();
    let blocks = blocks_of(&document);

    let chain_keys = [
        "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
        "KILL_OBJECTIVE_WHEN_I_COMPLETE",
        "NAP_OBJECTIVE_WHEN_I_COMPLETE",
    ];
    let mut per_kind: BTreeMap<&str, u32> = BTreeMap::new();
    let mut edges = 0;
    for key in chain_keys {
        let mut count = 0;
        for (_, args) in sites(&blocks, key) {
            for address in addresses(args) {
                count += 1;
                edges += 1;
                assert!(
                    (1..=i64::from(BLOCKS)).contains(&address),
                    "{key} spells address {address}, past the record's own blocks"
                );
            }
        }
        per_kind.insert(key, count);
    }
    assert_eq!(
        per_kind,
        BTreeMap::from([
            ("WAKE_OBJECTIVE_WHEN_I_COMPLETE", 50),
            ("KILL_OBJECTIVE_WHEN_I_COMPLETE", 39),
            ("NAP_OBJECTIVE_WHEN_I_COMPLETE", 13),
        ])
    );
    assert_eq!(edges, 102);
    assert!(
        sites(&blocks, "TICK_DEPENDS_ON_OBJ").is_empty(),
        "unlike M03, M07 spells no dependency gate"
    );

    // The terminal blocks.
    let mut terminal: Vec<(u32, &str)> = Vec::new();
    for (number, directives) in &blocks {
        for directive in directives {
            if let Some(outcome) = cs_content::mission_control::terminal_outcome_of(&directive.key)
            {
                assert!(directive.args.is_none(), "the outcome site is bare");
                terminal.push((
                    *number,
                    match outcome {
                        TerminalOutcome::Succeeded => "INSTANTWIN",
                        TerminalOutcome::Failed => "INSTANTLOSS",
                    },
                ));
            }
        }
    }
    assert_eq!(terminal, [(25, "INSTANTLOSS"), (27, "INSTANTWIN")]);
    for (number, _) in &terminal {
        let dormant = sites(&blocks, "BEGIN_DORMANT")
            .into_iter()
            .find(|(block, _)| block == number)
            .expect("the terminal block states its start");
        assert_eq!(
            dormant.1,
            &[ZrdValue::Float(-1.0)],
            "block {number} never wakes on its own clock"
        );
    }

    // Incoming edges to the terminal blocks, derived by walking every chain
    // site: (source block, key, spelled integers).
    let incoming = |target: u32| -> Vec<(u32, String, Vec<i64>)> {
        chain_keys
            .iter()
            .flat_map(|key| {
                sites(&blocks, key)
                    .into_iter()
                    .filter(move |(_, args)| addresses(args).contains(&i64::from(target)))
                    .map(move |(number, args)| (number, (*key).to_owned(), addresses(args)))
            })
            .collect()
    };
    assert_eq!(
        incoming(25),
        [(24, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![25])],
        "the loss block is reached only through one nap"
    );
    assert_eq!(
        incoming(27),
        [
            (
                61,
                "WAKE_OBJECTIVE_WHEN_I_COMPLETE".to_owned(),
                vec![24, 27]
            ),
            (
                24,
                "KILL_OBJECTIVE_WHEN_I_COMPLETE".to_owned(),
                vec![23, 26, 27]
            ),
        ],
        "the win block is named by one wake list and one kill list"
    );

    // The objective classes the HUD reads: three PRIMARY objectives with
    // distinct slots and one SECONDARY.
    let identity: Vec<(u32, String, i64)> = sites(&blocks, "IDENTITY")
        .into_iter()
        .map(|(number, args)| {
            let ZrdValue::Text(class) = &args[0] else {
                panic!("IDENTITY child0 is the class spelling");
            };
            let slot = match &args[1] {
                ZrdValue::Int(int) => i64::from(*int),
                other => panic!("IDENTITY child1 is the slot ordinal, not {other:?}"),
            };
            (number, class.clone(), slot)
        })
        .collect();
    assert_eq!(
        identity,
        [
            (1, "PRIMARY".to_owned(), 1),
            (26, "PRIMARY".to_owned(), 3),
            (34, "PRIMARY".to_owned(), 2),
            (41, "SECONDARY".to_owned(), 11),
        ]
    );

    // The enemy-group depletion sites: seven, each a group id and a remaining
    // count the record spells.
    let dedg: Vec<(u32, Vec<i64>)> = sites(&blocks, "DEDG")
        .into_iter()
        .map(|(number, args)| (number, addresses(args)))
        .collect();
    assert_eq!(
        dedg,
        [
            (35, vec![1, 3]),
            (51, vec![1, 0]),
            (52, vec![2, 0]),
            (53, vec![3, 0]),
            (54, vec![5, 1]),
            (55, vec![1, 0]),
            (56, vec![2, 0]),
        ]
    );
}

/// **M07's row is complete, and the census does not call that campaign-ready.**
///
/// M07-B-FU1 (#813) closed the last gap in M07's own record, so the row's
/// accounting meets every requirement — while `campaign_ready()` still needs
/// **every** measured row complete, and rows whose gaps are still open keep
/// the gate closed. A complete row is a lowering claim, never a launchable
/// mission: M07's runtime halves stay open for M07-C.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m07_b_m07s_row_is_complete_and_the_campaign_gate_stays_closed() {
    let census = census();
    assert!(census.complete_missions().contains(&MISSION));
    assert!(!census.campaign_ready());
    assert!(census.measured_rows().any(|row| row.mission() == MISSION));
    let row = census_row();
    assert!(row.is_complete());
    assert!(
        row.lowering()
            .expect("the accounting exists")
            .unmet()
            .next()
            .is_none()
    );
    // The gate is closed by rows other than M07: some other measured row is
    // still incomplete, so `campaign_ready()` cannot be resting on M07.
    assert!(
        census
            .measured_rows()
            .filter(|row| row.mission() != MISSION)
            .any(|row| !row.is_complete()),
        "another measured row still carries a gap, which is what keeps the gate closed"
    );
}

// ---------------------------------------------------------------------------
// Synthetic: the predicates the retail gaps lean on
// ---------------------------------------------------------------------------

fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

fn int(value: u32) -> ZrdValue {
    ZrdValue::Int(value)
}

fn record_of(blocks: Vec<(u32, Vec<ZrdValue>)>) -> ZrdValue {
    let mut children = Vec::new();
    for (number, directives) in blocks {
        children.push(text(&format!("OBJECTIVE{number}")));
        children.push(ZrdValue::List(directives));
    }
    ZrdValue::List(vec![ZrdValue::List(children)])
}

fn lower(document: &ZrdValue) -> cs_app::control_lowering::LoweredControlRecord {
    let record = measure_control_record(document);
    lower_control_record(
        ContentId::from_source(ContentKind::Mission, "accept-m07-b").map_err(|e| e.to_string()),
        "accept-m07-b",
        document,
        &record,
    )
}

/// One `ANIM_STATE` spec record: `ANIM [NAME [name], STATE [state]]`.
fn anim(name: &str) -> Vec<ZrdValue> {
    vec![
        text("ANIM"),
        ZrdValue::List(vec![
            text("NAME"),
            ZrdValue::List(vec![text(name)]),
            text("STATE"),
            ZrdValue::List(vec![text("EXECUTED")]),
        ]),
    ]
}

/// The block's one animation evaluator out of a lowered condition, wherever
/// it sits inside the gate — `All` when it is the only evaluator, `Any`
/// beside another kind otherwise.
fn animation_evaluator(condition: &Condition) -> (u32, &Vec<(String, AnimationState)>) {
    fn find(condition: &Condition) -> Option<(u32, &Vec<(String, AnimationState)>)> {
        match condition {
            Condition::AnimationStates {
                required,
                animations,
            } => Some((*required, animations)),
            Condition::All(items) | Condition::Any(items) => items.iter().find_map(find),
            _ => None,
        }
    }
    find(condition)
        .unwrap_or_else(|| panic!("the block carries no animation evaluator: {condition:?}"))
}

/// The block's one danger-zones evaluator out of a lowered condition,
/// wherever it sits inside the gate — the threshold and the zone names as the
/// lowering wrote them.
fn danger_evaluator(condition: &Condition) -> (u32, &Vec<String>) {
    fn find(condition: &Condition) -> Option<(u32, &Vec<String>)> {
        match condition {
            Condition::DangerZoneFlags { zones, required } => Some((*required, zones)),
            Condition::All(items) | Condition::Any(items) => items.iter().find_map(find),
            _ => None,
        }
    }
    find(condition)
        .unwrap_or_else(|| panic!("the block carries no danger-zones evaluator: {condition:?}"))
}

/// **Every spelled record lowers into one evaluator; only an operand list
/// the call cannot carry still refuses.**
///
/// M07's multi-record `ANIM_STATE` sites were the class this stage filed.
/// The measured walk appends every `{ANIM, spec}` pair, so two records lower
/// to two pairs with `required` counting them; five records — ten operands,
/// past the registry's positional bound — lower and bind just the same,
/// because the operand list is carried as one list argument. The arm that
/// still refuses is an operand list wider than the bound on carried items:
/// `MAX_VALUE_ITEMS` and beyond cannot fit the call's one argument, so the
/// key's only signature is uncarriable and it refuses registration rather
/// than truncating the spelled list.
#[test]
fn accept_m07_b_every_spelled_record_lowers_and_an_uncarriable_list_refuses() {
    // One record: condition lowers, call binds, program assembles.
    let single = record_of(vec![(1, {
        let mut site = vec![text("ANIM_STATE")];
        site.push(ZrdValue::List(anim("trailer_seg1")));
        site
    })]);
    let lowered = lower(&single);
    assert!(
        lowered
            .attempt()
            .conditions
            .iter()
            .all(|condition| *condition == ConditionOutcome::Lowered)
    );
    assert_eq!(lowered.attempt().unbound_keys, Vec::<String>::new());
    assert!(lowered.program().is_some(), "the record assembles");
    assert!(
        lowered
            .attempt()
            .validation
            .as_ref()
            .is_some_and(Vec::is_empty),
        "and validates clean"
    );

    // Two and five records: every spelled pair appends in order, `required`
    // counts them, and the wide site's ten operands are the call's one list
    // argument — the bound is not read as an arity.
    for count in [2u32, 5] {
        let record = record_of(vec![(1, {
            let mut site = vec![text("ANIM_STATE")];
            let mut args = Vec::new();
            for index in 0..count {
                args.extend(anim(&format!("trailer_seg{index}")));
            }
            site.push(ZrdValue::List(args));
            site
        })]);
        let lowered = lower(&record);
        assert_eq!(lowered.attempt().unbound_keys, Vec::<String>::new());
        assert!(
            lowered
                .attempt()
                .conditions
                .iter()
                .all(|condition| *condition == ConditionOutcome::Lowered),
            "{count} records lower: {:?}",
            lowered.attempt().conditions
        );
        let raw = lowered.raw_program().expect("the program assembled");
        let (required, pairs) = animation_evaluator(&raw.objectives[0].condition);
        assert_eq!(pairs.len(), count as usize);
        assert_eq!(required, count);
        let names: Vec<&str> = pairs.iter().map(|(name, _)| name.as_str()).collect();
        let expected: Vec<String> = (0..count)
            .map(|index| format!("trailer_seg{index}"))
            .collect();
        assert_eq!(names, expected, "declaration order is preserved");
        assert!(
            pairs
                .iter()
                .all(|(_, state)| *state == AnimationState::Executed)
        );
    }

    // Past MAX_VALUE_ITEMS the operand list cannot fit the call's one list
    // argument, so the site refuses rather than truncating — the one
    // signature is uncarriable, the key never registers and the damaged
    // block's condition refuses beside it.
    let over = record_of(vec![(1, {
        let mut site = vec![text("ANIM_STATE")];
        let mut args = Vec::new();
        for index in 0..(MAX_VALUE_ITEMS / 2 + 1) {
            args.extend(anim(&format!("trailer_seg{index}")));
        }
        site.push(ZrdValue::List(args));
        site
    })]);
    let lowered = lower(&over);
    assert_eq!(
        lowered.attempt().unbound_keys.len(),
        1,
        "the uncarriable signature refuses the key's registration: {:?}",
        lowered.attempt().unbound_keys
    );
    assert!(
        lowered.attempt().unbound_keys[0].contains("ANIM_STATE"),
        "{}",
        lowered.attempt().unbound_keys[0]
    );
    let refusals: Vec<&str> = lowered
        .attempt()
        .calls
        .iter()
        .filter_map(|call| match call {
            CallOutcome::Refused(text) => Some(text.as_str()),
            CallOutcome::Bound => None,
        })
        .collect();
    assert_eq!(
        refusals.len(),
        1,
        "the one site refuses rather than truncating: {refusals:?}"
    );
    // The refused conversion carries no call into the program, so the rest
    // still assembles and binds; `validate` then refuses the damaged block's
    // condition.
    assert!(
        lowered.program().is_some(),
        "no RawCall reaches the registry, so the program stands empty-handed"
    );
    assert!(
        lowered
            .attempt()
            .validation
            .as_ref()
            .is_some_and(|errors| !errors.is_empty()),
        "and validate refuses the record: {:?}",
        lowered.attempt().validation
    );
}

/// **A danger-zones site lowers the measured flag predicate, and a site the
/// parse cannot read refuses.**
///
/// The retail record's three `DANGER_ZONES_COMPLETED` blocks refused until
/// M07-B-FU1 (#813) measured the flag bytes' writer in the engine image.
/// Authored the measured way — a list of zone names and the optional
/// `DANGER_ZONES_COMPLETION_COUNT` beside it — the site lowers to
/// `Condition::DangerZoneFlags`, both keys bind and the program assembles and
/// validates. A site whose record after the key is **not** the zone-name list
/// still refuses by block and key, carrying the named reason: the original
/// would read whatever follows as the flag array's names, and this build
/// offers no predicate for a record it cannot read. The same block carrying
/// the measured inactive-members evaluator instead lowers its condition, so
/// an unreadable site is never mistaken for a broken record.
#[test]
fn accept_m07_b_a_danger_zones_site_lowers_its_predicate_and_an_unreadable_one_refuses() {
    // The measured shape: the zone names and the flag count they need.
    let danger = record_of(vec![(
        1,
        vec![
            text("DANGER_ZONES_COMPLETED"),
            ZrdValue::List(vec![text("dzpath8"), text("dzpath2")]),
            text("DANGER_ZONES_COMPLETION_COUNT"),
            ZrdValue::List(vec![int(1)]),
        ],
    )]);
    let lowered = lower(&danger);
    assert_eq!(
        lowered.attempt().conditions,
        [ConditionOutcome::Lowered],
        "the measured shape lowers: {:?}",
        lowered.attempt().conditions
    );
    assert!(
        lowered
            .attempt()
            .calls
            .iter()
            .all(|call| *call == CallOutcome::Bound),
        "both keys bind: they are measured and registered: {:?}",
        lowered.attempt().calls
    );
    assert_eq!(lowered.attempt().calls.len(), 2, "one call per site");
    let program = lowered.program().expect("the record assembles");
    assert!(
        lowered
            .attempt()
            .validation
            .as_ref()
            .is_some_and(Vec::is_empty),
        "and validates clean"
    );
    let (required, zones) = danger_evaluator(&program.objectives[0].condition);
    assert_eq!(
        zones,
        &["dzpath8".to_owned(), "dzpath2".to_owned()],
        "the predicate carries the zone names as spelled, in order"
    );
    assert_eq!(required, 1, "the spelled flag count is the threshold");

    // A site with no list after the key: the parse reads the record that
    // follows as the zone vector, which this build cannot state.
    let bare = record_of(vec![(1, vec![text("DANGER_ZONES_COMPLETED")])]);
    let lowered = lower(&bare);
    let refused: Vec<&str> = lowered
        .attempt()
        .conditions
        .iter()
        .filter_map(|condition| match condition {
            ConditionOutcome::Refused(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(refused.len(), 1, "{:?}", lowered.attempt().conditions);
    assert!(
        refused[0].contains("`DANGER_ZONES_COMPLETED`"),
        "the refusal names the key: {}",
        refused[0]
    );
    assert_eq!(
        refused_block(refused[0]),
        1,
        "the refusal names the block: {}",
        refused[0]
    );
    assert!(
        refused[0].contains("would be a guess at its predicate"),
        "the refusal says why: {}",
        refused[0]
    );
    assert!(
        !lowered
            .attempt()
            .conditions
            .contains(&ConditionOutcome::Lowered),
        "an unreadable site never lowers"
    );

    let inactive = record_of(vec![(
        1,
        vec![
            text("INACTIVE1"),
            ZrdValue::List(vec![text("actor_a"), text("actor_b")]),
            text("INACTIVE_COMPLETION_COUNT"),
            ZrdValue::List(vec![int(1)]),
        ],
    )]);
    let lowered = lower(&inactive);
    assert!(
        lowered
            .attempt()
            .conditions
            .iter()
            .all(|condition| *condition == ConditionOutcome::Lowered),
        "{:?}",
        lowered.attempt().conditions
    );
    assert!(lowered.program().is_some());
}

// ---------------------------------------------------------------------------
// M07-B-FU1: the danger-zones predicate, lowered from the measured flag
// ---------------------------------------------------------------------------

/// The block's own lifecycle gate, wherever it sits inside the lowered
/// condition — the pass-2 admission the original tests before any evaluator
/// runs (finding B).
fn carries_awake_gate(condition: &Condition, index: u32) -> bool {
    match condition {
        Condition::ObjectiveAwake { index: found } => *found == index,
        Condition::All(items) | Condition::Any(items) => {
            items.iter().any(|item| carries_awake_gate(item, index))
        }
        _ => false,
    }
}

/// **The three danger-zones blocks carry the measured flag-count predicate and
/// evaluate it fail-closed.**
///
/// `docs/findings/2026-10-09-m07-b-fu1-danger-zones-flag-writer.md` measures
/// the whole mechanism in the engine image: the once-per-block parse (`0x465ec0`)
/// strdups each listed zone name and zeroes one flag byte per name, the
/// pass-2 evaluator (`0x469ab0`) counts the nonzero bytes and fires at
/// `count >= threshold` with the threshold defaulting to the listed count,
/// and the single runtime writer (`0x446990`) stores `1` and never `0`.
/// Everything below is derived from the record itself, never repeated from it:
/// the zone names each block carries, the default threshold, the block's own
/// lifecycle gate, the named residual unknown of the producer's world test,
/// and the evaluation — no flags recorded, no completion; the block's own
/// zones recorded, one per site with a threshold equal to their count, a
/// completion; a zone the record does not list, never a completion.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m07_b_fu1_the_danger_zones_predicate_is_the_measured_flag_count() {
    let row = census_row();
    let lowered = row.lowering_attempt().expect("M07 has a lowering attempt");
    let raw = lowered.raw_program().expect("the program assembled");
    let program = lowered.program().expect("the program stands");

    let (document, _) = control_document();
    let blocks = blocks_of(&document);
    let danger_sites = sites(&blocks, "DANGER_ZONES_COMPLETED");
    assert_eq!(danger_sites.len(), 3, "M07 spells three danger-zones sites");
    assert!(
        sites(&blocks, "DANGER_ZONES_COMPLETION_COUNT").is_empty(),
        "M07 spells no flag count, so every threshold defaults to the listed zone count"
    );

    let mut carried: Vec<(usize, Vec<String>)> = Vec::new();
    for (number, args) in &danger_sites {
        let zones: Vec<String> = args
            .iter()
            .map(|value| match value {
                ZrdValue::Text(name) => name.clone(),
                other => panic!("block {number} spells {other:?} where a zone name is measured"),
            })
            .collect();
        assert!(!zones.is_empty(), "block {number} lists at least one zone");
        let index = usize::try_from(*number).expect("a block number fits") - 1;
        let (required, lowered_zones) = danger_evaluator(&raw.objectives[index].condition);
        assert_eq!(
            lowered_zones, &zones,
            "block {number} carries the zone names exactly as the record spells them, in order"
        );
        assert_eq!(
            required,
            zones.len() as u32,
            "block {number}: no spelled count, so the threshold is the zone count"
        );
        assert!(
            carries_awake_gate(&raw.objectives[index].condition, index as u32),
            "block {number} still carries its own lifecycle gate"
        );
        carried.push((index, zones));
    }

    // What sets a flag is the producer's world test, which is untraced: the
    // condition carries it as a named residual unknown and never inside the
    // predicate.
    let residuals = raw.objectives[carried[0].0].condition.residual_unknowns();
    assert_eq!(
        residuals,
        [DANGER_ZONE_CROSSING_TEST_UNTRACED],
        "the danger-zones condition carries exactly the named residual unknown"
    );

    // The predicate itself: fail-closed without flags, complete with the
    // block's own zones, and blind to a zone the record did not list. The
    // block's own lifecycle gate is admitted in every fact table, so what
    // separates the four cases below is the flags alone.
    fn facts_for(index: usize, zones: &[String]) -> MissionFacts {
        let mut facts = MissionFacts::default();
        facts
            .objectives
            .insert(index as u32, ObjectiveLifecycle::Awake);
        facts.danger_zones.extend(zones.iter().cloned());
        facts
    }
    let validated = program
        .clone()
        .validate()
        .expect("M07's program validates clean");
    let state = MissionState::new(&validated, SessionGeneration(1));
    for (index, zones) in &carried {
        let condition = &program.objectives[*index].condition;
        assert!(
            !state.holds(condition, &facts_for(*index, &[])),
            "block {} completes on no recorded flag",
            index + 1
        );
        assert!(
            !state.holds(condition, &facts_for(*index, &["dzpath9".to_owned()])),
            "block {} completes on a zone it never listed",
            index + 1
        );
        assert!(
            state.holds(condition, &facts_for(*index, zones)),
            "block {} completes once every listed zone carries its flag",
            index + 1
        );
        assert!(
            !state.holds(condition, &facts_for(*index, &zones[..zones.len() - 1])),
            "block {} needs its whole threshold, not one zone short",
            index + 1
        );
    }
}

/// **The flag-count threshold defaults to the zone count, and the first site
/// of each key is the one the parse reads.**
///
/// Both rules are the parse's own: `0x465ec0` runs once per block, so each of
/// its `0x57a090` lookups takes the first occurrence of its key in the block's
/// depth-first order — a later spelling of either key is inert, exactly as a
/// second `ANIM_STATE` site is — and an absent `DANGER_ZONES_COMPLETION_COUNT`
/// leaves `+0x568` at the listed count. The three shapes lower, bind and
/// validate; a threshold nobody spelled is a default, never a refusal.
#[test]
fn accept_m07_b_fu1_the_threshold_defaults_to_the_zone_count_and_the_first_site_wins() {
    /// Lowers one authored record and hands back its one evaluator.
    fn single(record: &ZrdValue) -> (u32, Vec<String>) {
        let lowered = lower(record);
        assert_eq!(
            lowered.attempt().conditions,
            [ConditionOutcome::Lowered],
            "the measured shape lowers: {:?}",
            lowered.attempt().conditions
        );
        assert!(
            lowered
                .attempt()
                .calls
                .iter()
                .all(|call| *call == CallOutcome::Bound),
            "every site binds: {:?}",
            lowered.attempt().calls
        );
        let program = lowered.program().expect("the record assembles");
        assert!(
            lowered
                .attempt()
                .validation
                .as_ref()
                .is_some_and(Vec::is_empty),
            "and validates clean"
        );
        let (required, zones) = danger_evaluator(&program.objectives[0].condition);
        (required, zones.clone())
    }

    // No count spelled: the threshold is the listed zone count.
    let (required, zones) = single(&record_of(vec![(1, {
        let mut site = vec![text("DANGER_ZONES_COMPLETED")];
        site.push(ZrdValue::List(vec![
            text("dzpath1"),
            text("dzpath2"),
            text("dzpath3"),
        ]));
        site
    })]));
    assert_eq!(zones, ["dzpath1", "dzpath2", "dzpath3"]);
    assert_eq!(required, 3, "absent count: the threshold is the zone count");

    // A spelled count is the threshold it spells.
    let (required, zones) = single(&record_of(vec![(
        1,
        vec![
            text("DANGER_ZONES_COMPLETED"),
            ZrdValue::List(vec![text("dzpath1"), text("dzpath2")]),
            text("DANGER_ZONES_COMPLETION_COUNT"),
            ZrdValue::List(vec![int(1)]),
        ],
    )]));
    assert_eq!(zones.len(), 2, "both zones are carried");
    assert_eq!(required, 1, "the spelled count is the threshold");

    // A second spelling of either key is never reached by the once-per-block
    // lookup: the first site's zones and the first count stand.
    let (required, zones) = single(&record_of(vec![(
        1,
        vec![
            text("DANGER_ZONES_COMPLETED"),
            ZrdValue::List(vec![text("dzpath1")]),
            text("DANGER_ZONES_COMPLETED"),
            ZrdValue::List(vec![text("dzpath2"), text("dzpath3")]),
            text("DANGER_ZONES_COMPLETION_COUNT"),
            ZrdValue::List(vec![int(2)]),
            text("DANGER_ZONES_COMPLETION_COUNT"),
            ZrdValue::List(vec![int(3)]),
        ],
    )]));
    assert_eq!(
        zones,
        ["dzpath1"],
        "the first zone-name site is the one the parse reads"
    );
    assert_eq!(
        required, 2,
        "the first flag-count site is the one the parse reads"
    );
}
