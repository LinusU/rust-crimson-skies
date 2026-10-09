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
//! * the record does **not** lower, and both requirements are unmet:
//!   `objective_condition` (eight blocks: five `ANIM_STATE` sites that spell
//!   two to five spec records where the condition lowering accepts one, and
//!   three `DANGER_ZONES_COMPLETED` sites whose condition this build declines
//!   to offer) and `call_arguments` (all nine `ANIM_STATE` call sites refuse,
//!   because the key's ten-argument shape exceeds the registry's per-signature
//!   bound and one over-bound signature refuses the whole key — the mechanism
//!   M02-B-FU1/Rally #800 filed and M04-B-FU1/#806 tracks for M04);
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
//! need ordinary-play observation (M07-C), the mission stays unready and the
//! campaign gate stays closed until the named gaps are measured
//! (`docs/findings/2026-10-09-m07-b-compatibility-gaps.md`).
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

/// The zero-based objective index a refusal text names: `objective#8` is the
/// ninth numbered block, because the census renders the record's zero-based
/// block index.
fn refused_objective(text: &str) -> u32 {
    let start = text
        .find("objective#")
        .expect("a call refusal names its objective")
        + "objective#".len();
    let digits: String = text[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().expect("the objective index is a number")
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

/// **The record does not lower, and the refusals are exactly the M07-specific
/// gaps.**
///
/// Eight conditions refuse — the five `ANIM_STATE` sites that spell two to
/// five `{ANIM, spec}` records where `cs_script::conditions::anim_state`
/// accepts one (blocks 13, 16, 20, 30 and 60), and the three
/// `DANGER_ZONES_COMPLETED` sites (blocks 37, 39 and 41) whose condition this
/// build declines to offer. Nine calls refuse, all `ANIM_STATE`: the key's
/// ten-argument shape (five spec records at block 60) exceeds the registry's
/// per-signature bound, so the whole key fails registration and even the
/// two-argument sites refuse. Both `objective_condition` and `call_arguments`
/// are unmet, no program stands, and the row is not complete.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m07_b_the_record_does_not_lower_and_the_gaps_are_the_named_ones() {
    let row = census_row();
    let attempt = row.lowering_attempt().expect("M07 has a lowering attempt");
    let lowered = attempt.attempt();
    assert_eq!(lowered.mission.as_deref(), Ok("mission/ch2-m02"));
    assert_eq!(lowered.objectives, BLOCKS);
    assert_eq!(lowered.calls.len() as u32, SITES);

    let (document, _) = control_document();
    let blocks = blocks_of(&document);

    // Calls: every refusal is an `ANIM_STATE` site, and the refused objective
    // indices are exactly the blocks an independent walk finds spelling the
    // key.
    let refused_calls: Vec<u32> = lowered
        .calls
        .iter()
        .filter_map(|call| match call {
            CallOutcome::Refused(text) => {
                assert!(
                    text.contains("unknown host call `ANIM_STATE`"),
                    "M07's only refused calls are ANIM_STATE sites: {text}"
                );
                Some(refused_objective(text))
            }
            CallOutcome::Bound => None,
        })
        .collect();
    let anim_blocks: Vec<u32> = sites(&blocks, "ANIM_STATE")
        .iter()
        .map(|(number, _)| number - 1)
        .collect();
    assert_eq!(
        refused_calls, anim_blocks,
        "every ANIM_STATE site refuses and no other site does"
    );
    assert_eq!(refused_calls.len(), 9);
    assert_eq!(
        lowered.unbound_keys,
        ["`ANIM_STATE`: binding `ANIM_STATE`: too many arguments"],
        "the registry refused ANIM_STATE at registration, on the host-call bound"
    );

    // Conditions: the refused blocks are the multi-record ANIM_STATE sites
    // plus the DANGER_ZONES_COMPLETED sites, each named with its own key.
    let mut refused_conditions = BTreeMap::new();
    for outcome in &lowered.conditions {
        if let ConditionOutcome::Refused(text) = outcome {
            let key = if text.contains("`ANIM_STATE`") {
                "ANIM_STATE"
            } else if text.contains("`DANGER_ZONES_COMPLETED`") {
                "DANGER_ZONES_COMPLETED"
            } else {
                panic!("M07's refused conditions name the measured keys: {text}");
            };
            refused_conditions.insert(refused_block(text), key);
        }
    }
    let mut expected = BTreeMap::new();
    for (number, args) in sites(&blocks, "ANIM_STATE") {
        if args.len() > 2 {
            expected.insert(number, "ANIM_STATE");
        }
    }
    for (number, _) in sites(&blocks, "DANGER_ZONES_COMPLETED") {
        expected.insert(number, "DANGER_ZONES_COMPLETED");
    }
    assert_eq!(
        refused_conditions, expected,
        "the refused conditions are exactly the multi-record ANIM_STATE sites and the \
         danger-zones sites"
    );
    assert_eq!(refused_conditions.len(), 8);
    assert_eq!(
        lowered.conditions.len() as u32,
        BLOCKS,
        "every block was walked"
    );

    assert!(
        lowered.validation.is_none(),
        "no program stood to be validated"
    );
    assert!(
        attempt.program().is_none(),
        "and none is handed to the runtime"
    );
    let lowering = row.lowering().expect("the accounting exists");
    let unmet: Vec<String> = lowering.unmet().map(|r| r.kind.code().to_owned()).collect();
    assert_eq!(unmet, ["objective_condition", "call_arguments"]);
    assert!(!row.is_complete());
    assert!(!lowering.complete());
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

/// **M07 is not campaign-ready and the census does not hide it.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m07_b_the_mission_stays_unready_and_the_campaign_gate_stays_closed() {
    let census = census();
    assert!(!census.complete_missions().contains(&MISSION));
    assert!(!census.campaign_ready());
    assert!(census.measured_rows().any(|row| row.mission() == MISSION));
    let row = census_row();
    assert!(!row.is_complete());
    assert!(
        row.lowering()
            .expect("the accounting exists")
            .unmet()
            .next()
            .is_some()
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

/// **Two refusal arms of one key: the multi-record condition and the
/// registration bound.**
///
/// The retail record refuses `ANIM_STATE` at M07 on two independent arms. Each
/// is reproduced on authored records: two spec records (four arguments) keep
/// the key inside the host-call bound — the calls bind and the program
/// assembles — but the condition refuses with the one-record-shape message,
/// and the objective's condition lowers to `Unknown`. Five spec records (ten
/// arguments) exceed the bound, so the key fails registration and every site
/// refuses as an unknown host call, even a single-record one elsewhere in the
/// record, and no program stands.
#[test]
fn accept_m07_b_a_multi_record_anim_state_refuses_its_condition_and_a_bound_shape_refuses_the_key()
{
    // One record: the shape the engine implements today — condition lowers,
    // call binds, program assembles.
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

    // Two records: inside the bound, so the calls bind — but the condition
    // refuses, naming the one-record shape.
    let double = record_of(vec![(1, {
        let mut site = vec![text("ANIM_STATE")];
        let mut args = anim("trailer_seg1");
        args.extend(anim("trailer_seg2"));
        site.push(ZrdValue::List(args));
        site
    })]);
    let lowered = lower(&double);
    assert_eq!(lowered.attempt().unbound_keys, Vec::<String>::new());
    let refused: Vec<&str> = lowered
        .attempt()
        .conditions
        .iter()
        .filter_map(|condition| match condition {
            ConditionOutcome::Refused(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(refused.len(), 1);
    assert!(refused[0].contains("`ANIM_STATE`"), "{}", refused[0]);
    assert!(
        refused[0].contains("the measured shape is the tag `ANIM` and one spec record"),
        "{}",
        refused[0]
    );
    assert!(
        lowered.program().is_some(),
        "the calls all bind, so the program assembles — the refusal is the condition alone"
    );

    // Five records: ten arguments, past the host-call bound, so the key
    // refuses registration and every site refuses with it.
    let five = record_of(vec![(1, {
        let mut site = vec![text("ANIM_STATE")];
        let mut args = Vec::new();
        for index in 0..5 {
            args.extend(anim(&format!("trailer_seg{index}")));
        }
        site.push(ZrdValue::List(args));
        site
    })]);
    let lowered = lower(&five);
    assert_eq!(
        lowered.attempt().unbound_keys,
        ["`ANIM_STATE`: binding `ANIM_STATE`: too many arguments"]
    );
    assert!(
        lowered
            .attempt()
            .calls
            .iter()
            .all(|call| matches!(call, CallOutcome::Refused(_)))
    );
    assert!(lowered.program().is_none());
}

/// **A danger-zones site refuses its condition, and the same block without it
/// lowers.**
///
/// The retail record refuses its three `DANGER_ZONES_COMPLETED` blocks because
/// this build lowers no condition for the flag evaluator. Authored the same
/// way, the refusal names that reason and the call still binds; the same block
/// carrying the measured inactive-members evaluator instead lowers its
/// condition. So the retail refusal is the missing predicate, not a broken
/// record.
#[test]
fn accept_m07_b_a_danger_zones_site_refuses_its_condition_and_the_block_without_it_lowers() {
    let danger = record_of(vec![(
        1,
        vec![
            text("DANGER_ZONES_COMPLETED"),
            ZrdValue::List(vec![text("dzpath8")]),
        ],
    )]);
    let lowered = lower(&danger);
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
        refused[0].contains(
            "the danger-zones flag evaluator is measured but this build lowers no condition \
             for it"
        ),
        "{}",
        refused[0]
    );
    assert_eq!(
        lowered.attempt().calls,
        [CallOutcome::Bound],
        "the call binds: the key is measured and registered"
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
