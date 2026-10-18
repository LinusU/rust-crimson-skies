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
//! * M07's control program is `objectives.zrd` of `ZBD/C1/M02/zrdr.zbd` with
//!   50 numbered blocks and 190 directive sites, selected by the content rule
//!   and not by its name (the corrected archive layout binds this record to
//!   M07, Rally #1597; the record this stage first pinned belongs to another
//!   mission);
//! * its 36-key vocabulary is **fully disposed** — two terminal outcomes, 34
//!   measured keys, none refused and no unclassified record key — and the
//!   record spells all five record-level sound keys, so every gap below is a
//!   lowering gap, not a measurement gap;
//! * the record lowers **completely**: it spells no `ANIM_STATE`, no
//!   `TRAVELERS` and no `DANGER_ZONES_COMPLETED` site, so the list-argument
//!   mechanism of M04-B-FU1 (#806) and the counted-flag predicate of
//!   M07-B-FU1 (#813,
//!   `docs/findings/2026-10-09-m07-b-fu1-danger-zones-flag-writer.md`) are
//!   pinned by the synthetic tests here and no longer exercised by M07's own
//!   retail record; no condition refuses and `MissionProgram::validate`
//!   accepts the program;
//! * the sheet's three regression priorities are located only as far as the
//!   record goes: the `PLAYER_INIT` record field exists, while the record
//!   spells **no** moving-subject proximity evaluator, **no** animation gate,
//!   **no** player-vehicle change and **no** input-ownership transfer, which
//!   stay in the mission-program members this stage does not decode;
//! * the objective graph is closed: all 63 cross-objective addresses lie in
//!   `1..=50` (the parser `dec`s them into zero-based indices), all three
//!   terminal blocks start dormant, and their incoming wake/kill/nap edges are
//!   pinned.
//!
//! No behaviour is invented here: the runtime halves of the sheet's priorities
//! need ordinary-play observation (M07-C), and the campaign gate stays closed
//! until every measured row lowers — M07's own row does, while the rows whose
//! gaps are still open keep `campaign_ready()` false
//! (`docs/findings/2026-10-09-m07-b-compatibility-gaps.md`, with the re-pin on
//! the corrected layout recorded at its end).
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
    AnimList, CallOutcome, ConditionOutcome, ControlRecordField, ControlRecordSound,
    DirectiveDisposition, DirectiveOperation, TerminalOutcome, measure_control_record,
};
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{ZrdValue, objective_record, zrd_flat_fields};
use cs_formats::script_raw::discover_container;
use cs_script::ir::{AnimationState, Condition, MAX_VALUE_ITEMS};
use cs_types::content::{ContentId, ContentKind};
use cs_types::install::RelativePath;

use crate::campaign::common::{label, load_inventory};

/// The census row label of the mission (`ZBD/C1/M02/zrdr.zbd`, F13-B's
/// mission-scope rule).
const MISSION: &str = "zbd/c1/m02";

/// The numbered blocks of the control member.
const BLOCKS: u32 = 50;
/// The directive sites of the control member.
const SITES: u32 = 190;
/// The distinct directive keys of the control member.
const KEYS: usize = 36;

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
/// (`zbd/c1/m02`) name one archive — the span and digest
/// `missions/bindings/M07.json` cites — and one member: of the 20 members of
/// `ZBD/C1/M02/zrdr.zbd` exactly one declares numbered `OBJECTIVE<N>` blocks.
/// A longer member exists and the control member is the archive's eighth, so
/// neither size nor position is the rule. The member's span lies inside the
/// archive and its own digest re-derives from its own bytes.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m07_b_the_control_program_is_the_member_that_declares_the_blocks() {
    let binding = control_binding();
    assert_eq!(binding.mission.as_str(), "mission/ch2-m02");
    assert_eq!(binding.program_id.as_str(), "script/c1-m02-zrdr");
    assert_eq!(binding.program_asset, "ZBD/C1/M02/zrdr.zbd");
    assert_eq!(
        binding.program_sha256, "89f272191c20a0905197a2f9c77a492b0a7337068a1f4b1fa104a39b65423ad9",
        "the reader archive is the program M07-A bound"
    );

    let row = census_row();
    assert_eq!(row.container, binding.program_asset);
    assert_eq!(
        row.container_sha256, binding.program_sha256,
        "the census and the work-order join read one archive"
    );
    assert_eq!(row.members.len(), 20);
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
        (14_413, 12_410)
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
/// Two keys are terminal outcomes (`INSTANTWIN` once, `INSTANTLOSS` twice, all
/// bare), the other 34 have a measured effect, and the sites are accounted
/// for: the keys' sites sum to the record's. M07 spells no record-level key
/// outside the measured vocabulary, and no block is unreadable.
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
    assert_eq!(measured, 34);
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
            (
                ControlRecordField::Sound(ControlRecordSound::PrimaryComplete),
                1
            ),
            (
                ControlRecordField::Sound(ControlRecordSound::SecondaryComplete),
                1
            ),
            (
                ControlRecordField::Sound(ControlRecordSound::TertiaryComplete),
                1
            ),
            (ControlRecordField::Sound(ControlRecordSound::MissionWon), 1),
            (
                ControlRecordField::Sound(ControlRecordSound::MissionLost),
                1
            ),
        ],
        "the five shape fields and the five record-level sound keys, once each"
    );
    assert_eq!(
        record.record_sounds(),
        [
            (ControlRecordSound::PrimaryComplete, 1),
            (ControlRecordSound::SecondaryComplete, 1),
            (ControlRecordSound::TertiaryComplete, 1),
            (ControlRecordSound::MissionWon, 1),
            (ControlRecordSound::MissionLost, 1),
        ],
        "M07 is the record that spells all five sound keys, in the vocabulary's parse order"
    );
}

/// **Every block lowers, and the record spells none of the shapes M07-B's
/// first pins were about.**
///
/// All 190 sites bind and 50 of 50 conditions lower: the record spells no
/// `ANIM_STATE` site, no `TRAVELERS` site and no `DANGER_ZONES_COMPLETED` site,
/// so neither the list-argument mechanism of M04-B-FU1 (#806) nor the
/// counted-flag predicate of M07-B-FU1 (#813) is exercised by this record —
/// the synthetic tests below keep them pinned, and M08's record is where the
/// retail danger-zones blocks are. A program stands, `MissionProgram::validate`
/// accepts it, `objective_condition` and `call_arguments` are met, and the row
/// is complete — while the campaign gate stays closed on the rows whose gaps
/// are still open.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m07_b_every_block_lowers_and_the_record_spells_no_animation_or_danger_zones_site() {
    let row = census_row();
    let attempt = row.lowering_attempt().expect("M07 has a lowering attempt");
    let lowered = attempt.attempt();
    assert_eq!(lowered.mission.as_deref(), Ok("mission/ch2-m02"));
    assert_eq!(lowered.objectives, BLOCKS);
    assert_eq!(lowered.calls.len() as u32, SITES);

    let (document, _) = control_document();
    let blocks = blocks_of(&document);

    // Calls: no site refuses.
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
        "no key fails registration: {:?}",
        lowered.unbound_keys
    );

    // The shapes the earlier pins were about are not in this record.
    for key in [
        "ANIM_STATE",
        "TRAVELERS",
        "DANGER_ZONES_COMPLETED",
        "DANGER_ZONES_COMPLETION_COUNT",
    ] {
        assert!(
            sites(&blocks, key).is_empty(),
            "M07's record spells no {key} site"
        );
    }
    let raw = attempt.raw_program().expect("the program assembled");
    assert!(
        raw.objectives
            .iter()
            .all(|objective| objective.calls.iter().all(|call| call.name != "ANIM_STATE")),
        "no lowered call is an ANIM_STATE call"
    );

    // Conditions: no block refuses and every block was walked.
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
        "every block lowers: {refused_conditions:?}"
    );
    assert_eq!(
        lowered.conditions.len() as u32,
        BLOCKS,
        "every block was walked"
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

/// **The sheet's three regression priorities, located (or absent) in the
/// measured record.**
///
/// *Moving pickup / moving subject:* the record spells **no** proximity
/// evaluator (`TRAVELERS`) and no animation gate (`ANIM_STATE`) — the trailer
/// segments and the pickup the sheet names are not in the control record, so
/// where they live is unmeasured here (the members this stage does not
/// decode). The initial player configuration exists as the `PLAYER_INIT`
/// record field, once, and the record spells the five record-level sound keys.
///
/// *Forced plane swap and persistent input ownership:* the 36-key vocabulary
/// is pinned below in full, and it contains no player-vehicle change and no
/// input-ownership directive — those live, if anywhere, in the mission-program
/// members that declare no blocks and that this stage does not decode. Their
/// absence from the control record is a measured fact, not an assumption.
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
    assert_eq!(operation("SET_AI_NET"), DirectiveOperation::AssignNet);
    assert_eq!(
        operation("STOP_QUEUED_SOUNDS"),
        DirectiveOperation::StopQueuedSounds
    );
    assert_eq!(
        operation("WAKEUP_GENERATOR"),
        DirectiveOperation::FeedGenerator
    );
    assert_eq!(operation("WAKEUP_TURRETS"), DirectiveOperation::WakeTurrets);
    assert_eq!(operation("WAKEUP_ENEMIES"), DirectiveOperation::WakeEnemies);
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
            "BEGIN_DORMANT",
            "COMPLETED_SOUND_GROUP",
            "DEDG",
            "IDENTITY",
            "INACTIVE1",
            "INACTIVE10",
            "INACTIVE11",
            "INACTIVE12",
            "INACTIVE13",
            "INACTIVE14",
            "INACTIVE15",
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
            "REMOVE_OTHER_TARGET",
            "SET_AI_NET",
            "SET_HELP_LABEL",
            "STOP_QUEUED_SOUNDS",
            "WAKEUP_ENEMIES",
            "WAKEUP_GENERATOR",
            "WAKEUP_SOUND_GROUP",
            "WAKEUP_TURRETS",
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
        ]
    );

    let (document, _) = control_document();
    let blocks = blocks_of(&document);

    // The sheet's moving-subject and animation-gate evaluators are not
    // spelled by this record, and neither are the sheet's vehicle-swap or
    // input-ownership directives.
    for key in ["TRAVELERS", "ANIM_STATE", "DANGER_ZONES_COMPLETED"] {
        assert!(
            sites(&blocks, key).is_empty(),
            "M07's record spells no {key} site"
        );
    }
}

/// **The objective graph is closed and the terminal blocks are gated.**
///
/// All 63 cross-objective addresses (21 wake, 23 kill, 19 nap) lie in
/// `1..=50`; the original `dec`s them into zero-based indices at parse
/// (`docs/findings/2026-10-06-m01-lc-directive-b-…`), so every address names a
/// block of this record and none points past it. All three terminal blocks
/// start dormant with no timed wake (`BEGIN_DORMANT -1`), so none can fire on
/// its own clock: block 15 (`INSTANTWIN`) is reached only through block 14's
/// nap, block 24 (`INSTANTLOSS`) through block 3's kill list and block 19's
/// nap, and block 36 (`INSTANTLOSS`) through block 7's nap. The four PRIMARY
/// objectives carry distinct HUD slots and one SECONDARY objective carries
/// slot 11.
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
            ("WAKE_OBJECTIVE_WHEN_I_COMPLETE", 21),
            ("KILL_OBJECTIVE_WHEN_I_COMPLETE", 23),
            ("NAP_OBJECTIVE_WHEN_I_COMPLETE", 19),
        ])
    );
    assert_eq!(edges, 63);
    assert!(
        sites(&blocks, "TICK_DEPENDS_ON_OBJ").is_empty(),
        "M07 spells no dependency gate"
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
    assert_eq!(
        terminal,
        [(15, "INSTANTWIN"), (24, "INSTANTLOSS"), (36, "INSTANTLOSS")]
    );
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
        incoming(15),
        [(14, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![15])],
        "the win block is reached only through one nap"
    );
    assert_eq!(
        incoming(24),
        [
            (
                3,
                "KILL_OBJECTIVE_WHEN_I_COMPLETE".to_owned(),
                vec![19, 20, 21, 22, 23, 24, 32, 33, 34]
            ),
            (19, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![24]),
        ],
        "the first loss block is named by one nine-target kill list and one nap"
    );
    assert_eq!(
        incoming(36),
        [(7, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![36])],
        "the second loss block is reached only through one nap"
    );

    // The objective classes the HUD reads: four PRIMARY objectives with
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
            (3, "PRIMARY".to_owned(), 1),
            (10, "PRIMARY".to_owned(), 2),
            (11, "PRIMARY".to_owned(), 3),
            (14, "PRIMARY".to_owned(), 4),
            (16, "SECONDARY".to_owned(), 11),
        ]
    );

    // The enemy-group depletion sites: three, each a group id and a remaining
    // count the record spells.
    let dedg: Vec<(u32, Vec<i64>)> = sites(&blocks, "DEDG")
        .into_iter()
        .map(|(number, args)| (number, addresses(args)))
        .collect();
    assert_eq!(dedg, [(5, vec![1, 2]), (13, vec![2, 0]), (14, vec![1, 0])]);
}

/// **M07's row is complete, and the census does not call that campaign-ready.**
///
/// M07's own record lowers with every requirement met — while
/// `campaign_ready()` still needs **every** measured row complete, and rows
/// whose gaps are still open keep the gate closed. A complete row is a
/// lowering claim, never a launchable mission: M07's runtime halves stay open
/// for M07-C.
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

/// **M07's record spells no danger-zones site, so none of its blocks carries
/// the measured flag-count predicate.**
///
/// `docs/findings/2026-10-09-m07-b-fu1-danger-zones-flag-writer.md` measures
/// the predicate in the engine image (parse `0x465ec0`, evaluator `0x469ab0`,
/// single writer `0x446990`); the three retail blocks it was first pinned on
/// belong to a different mission's record on the corrected archive layout
/// (#1597). This pin derives from the record itself, through production
/// lowering, that M07 spells neither `DANGER_ZONES_COMPLETED` nor
/// `DANGER_ZONES_COMPLETION_COUNT` and that no lowered condition carries a
/// `DangerZoneFlags` evaluator. The predicate's evaluation stays pinned by the
/// synthetic tests in this file and by M08-B's retail danger-zones blocks.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m07_b_fu1_the_record_spells_no_danger_zones_site_so_no_block_carries_the_predicate() {
    fn carries_flags(condition: &Condition) -> bool {
        match condition {
            Condition::DangerZoneFlags { .. } => true,
            Condition::All(items) | Condition::Any(items) => items.iter().any(carries_flags),
            _ => false,
        }
    }

    let row = census_row();
    let lowered = row.lowering_attempt().expect("M07 has a lowering attempt");
    let raw = lowered.raw_program().expect("the program assembled");

    let (document, _) = control_document();
    let blocks = blocks_of(&document);
    for key in ["DANGER_ZONES_COMPLETED", "DANGER_ZONES_COMPLETION_COUNT"] {
        assert!(sites(&blocks, key).is_empty(), "M07 spells no {key} site");
    }
    assert!(
        control_binding()
            .record
            .key("DANGER_ZONES_COMPLETED")
            .is_none(),
        "and its measured vocabulary does not list the key"
    );
    assert_eq!(raw.objectives.len() as u32, BLOCKS);
    let carrying: Vec<usize> = raw
        .objectives
        .iter()
        .enumerate()
        .filter(|(_, objective)| carries_flags(&objective.condition))
        .map(|(index, _)| index + 1)
        .collect();
    assert!(
        carrying.is_empty(),
        "no lowered condition is a danger-zones evaluator, yet blocks {carrying:?} are"
    );
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
