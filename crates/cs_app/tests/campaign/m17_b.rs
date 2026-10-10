//! Acceptance stage M17-B: the mission-specific compatibility gaps of the
//! seventeenth mission (`missions/M17.md`, work order `M17-B`).
//!
//! M17-A bound M17's identities and left the mission program unmeasured. The
//! machinery that measures a control program (the `.zrd` reader, the census,
//! the directive dispositions and the record → `RawProgram` adapter) is shared
//! and was built for M01; this stage runs it over M17's own reader archive and
//! pins what is **different** at M17, together with the three regression
//! priorities the sheet names:
//!
//! * M17's control program is `objectives.zrd` with 38 numbered blocks and 135
//!   directive sites, the eighth of the archive's 12 members and selected by
//!   the content rule — the one member longer than it is `aiv.zrd`, so neither
//!   size nor position picks it;
//! * its 22-key vocabulary is **fully disposed** — 21 measured keys and exactly
//!   one terminal outcome (`INSTANTWIN`, block 14), none refused, no
//!   unclassified record key — so every gap below is a lowering gap or a
//!   world-side unknown, never an unmeasured key;
//! * the record **lowers completely**: 38 `RawObjective`s, all 38 conditions
//!   lowered, all 135 calls bound, `MissionProgram::validate` clean, and the
//!   census row for `zbd/c4/m02` is complete while the campaign gate stays
//!   shut;
//! * **forced-airframe control** — the record writes *nothing* on the player:
//!   the only four sites that spell `player` are the four `TRAVELERS`
//!   subjects, the one vehicle-teleport site names the ace's gyro, and the
//!   airframe the mission launches in is the measured campaign chain's own row
//!   (`airframe/player_pfighter`) read through `cs_app::mission_start`, not
//!   anything M17's records assign. The sheet's discovery cue (`forced
//!   autogyro`) is a research label; nothing measured here implements it;
//! * **search triggers** — four blocks (2, 3, 4, 6) start *awake* and spell the
//!   `DANGER_ZONES_COMPLETED` evaluator over seven named `dzpath` zones at
//!   threshold 1, and the four `TRAVELERS` proximity sites (31–34) fire on the
//!   player closing to 500 about `bhatgyro_1`. The zone names are cross-checked
//!   against the chapter-4 world container's own `dzpath1…15` nodes through the
//!   production trigger-volume survey, which reports **no** declaration gap for
//!   M17;
//! * **ace encounter lifecycle** — `bhatgyro_1` is netted (block 15), woken
//!   (17), warped to one of four spelled points five seconds in (23) and
//!   approached four times (31–34); the whole record spells 128 distinct texts,
//!   all of them declared somewhere outside M17's control member except nine
//!   measured ones — among them `WARP_VEHICLE`, the one directive key no other
//!   archive in the installation spells, and two actors, `bhatbrigand_13` and
//!   `bhatbrigand_14`, that **no file in the installation declares**.
//!
//! A lowered program is **not** a played mission: no playthrough, difficulty,
//! media or presentation row is covered (that is M17-C, which requires
//! `human_play`), and the wrong-actor / wrong-session / repeated-event halves of
//! the sheet's priorities are runtime observations that stay unmeasured here.
//! The measured unknowns are written up in
//! `docs/findings/2026-10-10-m17-b-compatibility-gaps.md`.
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`; the two synthetic
//! tests run in CI.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_app::control_lowering::lower_control_record;
use cs_app::mission_control::{
    RetailControlCensus, read_control_member, survey_mission_control_programs,
};
use cs_app::mission_start::{
    AIRFRAME_TABLE, CAMPAIGN_AIRFRAME_ROW, recover_retail_start_configuration,
};
use cs_app::world::triggers::survey_retail_trigger_volumes;
use cs_assets::install::{discover, sha256};
use cs_content::campaign_bindings::{
    MissionControlBinding, MissionLabel, SourceBinding, SourceContext,
};
use cs_content::mission_control::{
    AnimList, CallOutcome, ConditionOutcome, ControlRecordField, DirectiveDisposition,
    DirectiveOperation, TerminalOutcome, measure_control_record, terminal_outcome_of,
};
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{ZrdValue, decode_zrd, objective_record, zrd_flat_fields};
use cs_content::world::{RetailTriggerVolumeSurvey, WorldId, ZoneDeclarationKey};
use cs_formats::script_raw::discover_container;
use cs_types::content::{ContentId, ContentKind, Resolved};
use cs_types::install::RelativePath;

use crate::common::load_inventory;

/// The census row label of the mission (F13-B's mission-scope rule).
const MISSION: &str = "zbd/c4/m02";
/// The reader archive the installation ships for M17 — the program span
/// `missions/bindings/M17.json` cites.
const CONTAINER: &str = "ZBD/C4/M02/zrdr.zbd";
/// SHA-256 of that whole archive, from production discovery.
const CONTAINER_SHA256: &str = "f21d5b6a2a734c983ee2775aad645acae436e228d7cf71c0a8e3ff8d5914786d";
/// The archive's length in bytes — M17-A's own source span.
const CONTAINER_LENGTH: u64 = 31_647;
/// The member the measured rule chose.
const CONTROL_MEMBER: &str = "objectives.zrd";
/// The control member's first byte inside the archive.
const CONTROL_OFFSET: u64 = 13_070;
/// The control member's length in bytes.
const CONTROL_LENGTH: u64 = 9_552;
/// SHA-256 of the control member's own bytes.
const CONTROL_SHA256: &str = "a343b470b85b6f3099bf8f44bbe87557dbcce18ff867ca4aa327d9042ad05e41";
/// The detection-zone member the chapter survey decodes for this mission.
const DZONES_MEMBER: &str = "dzones.zrd";
/// Its first byte inside the archive (census row and survey span must agree).
const DZONES_OFFSET: u64 = 10_843;
/// Its length in bytes.
const DZONES_LENGTH: u64 = 529;
/// SHA-256 of the detection-zone member's own bytes.
const DZONES_SHA256: &str = "517a6b6fd63cb2ffe6f8bb51fba507d1104c67e17c1a89f4acce99a2085e99f6";
/// The aircraft member `cs_app::mission_start` reads the player record from.
const AIRCRAFT_MEMBER: &str = "aiv.zrd";
/// The numbered blocks of the control member.
const BLOCKS: u32 = 38;
/// The directive sites of the control member.
const SITES: u32 = 135;
/// The distinct directive keys of the control member.
const KEYS: usize = 22;
/// The spelled wake/kill/nap/gate addresses: 21 wake, 12 kill, 14 nap, 1 gate.
const EDGES: usize = 48;
/// The distinct text nodes the control document spells (whole-document walk).
const NAMED_TEXTS: usize = 128;
/// The texts of that walk which **no** `.zrd` member outside M17's control
/// member declares, and which are therefore recorded rather than resolved.
/// Sorted the way the walk sorts them.
const RECORD_ONLY_TEXTS: [&str; 9] = [
    "MSG_BRF_RMM2_OBJ1",
    "MSG_BRF_RMM2_OBJ2",
    "WARP_VEHICLE",
    "bhatbrigand_13",
    "bhatbrigand_14",
    "snd_c4-RM-m2_Blacke_14",
    "snd_c4-RM-m2_Blacke_16",
    "snd_c4-RM-m2_Zachary_13",
    "snd_c4-RM-m2_Zachary_15",
];

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M17-B needs the retail capability; run this suite with \
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

/// The declared discovery title of `M17`, read from the committed inventory.
fn declared_title() -> String {
    load_inventory()
        .iter()
        .find(|(label, _)| label.as_str() == "M17")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M17 work order")
}

/// The production control-program binding, built once for the whole suite.
fn control_binding() -> &'static MissionControlBinding {
    static BINDING: OnceLock<MissionControlBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        let context = SourceContext::read(&game_dir())
            .expect("production source context reads the installation");
        context
            .control_program(
                MissionLabel::new("M17").expect("M17 is a valid label"),
                &declared_title(),
            )
            .expect("M17's control program binds through the measured rule")
    })
}

/// The M17 mission binding M17-A derives, built once — this stage consumes its
/// identities and adds no second evidence for the join itself.
fn mission_binding() -> &'static SourceBinding {
    static BINDING: OnceLock<SourceBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        let context = SourceContext::read(&game_dir())
            .expect("production source context reads the installation");
        context
            .bind(
                MissionLabel::new("M17").expect("M17 is a valid label"),
                &declared_title(),
            )
            .expect("M17 binds to the original data")
    })
}

/// The trigger-volume survey with every campaign mission's `dzones.zrd`
/// declaration attached, built once for the whole suite.
fn zone_survey() -> &'static RetailTriggerVolumeSurvey {
    static SURVEY: OnceLock<RetailTriggerVolumeSurvey> = OnceLock::new();
    SURVEY.get_or_init(|| {
        survey_retail_trigger_volumes(&game_dir())
            .expect("the production trigger-volume survey runs on the installation")
    })
}

/// One directive of a block as authored: key and, unless bare, its argument list.
struct Directive {
    key: String,
    args: Option<Vec<ZrdValue>>,
}

/// Walks a decoded control record into `(block number, directives)`, reading the
/// grammar independently of the census: a text key, then its argument list if
/// the next child is a list.
fn blocks_of(document: &ZrdValue) -> Vec<(u32, Vec<Directive>)> {
    let mut blocks = Vec::new();
    for (key, value) in zrd_flat_fields(objective_record(document)) {
        let Some(number) = objective_block_number(key) else {
            continue;
        };
        let children = value.as_list().expect("every M17 block is a list");
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
    blocks.sort_by_key(|(number, _)| *number);
    blocks
}

/// The control member's decoded document, re-read from the archive through
/// production discovery — an independent walk from the binding's, so the graph
/// assertions below cannot be satisfied by the binding's own output.
fn control_document() -> ZrdValue {
    read_control_member(&game_dir(), MISSION)
        .expect("the rule finds M17's control member")
        .0
}

/// The argument list a directive spells, or an empty list when it is bare.
fn arguments(directive: &Directive) -> Vec<ZrdValue> {
    directive.args.clone().unwrap_or_default()
}

/// The integer arguments of one argument list.
fn integers_of(args: &[ZrdValue]) -> Vec<i64> {
    args.iter()
        .filter_map(|value| match value {
            ZrdValue::Int(int) => Some(i64::from(*int)),
            _ => None,
        })
        .collect()
}

/// The integer arguments a directive spells.
fn integers(directive: &Directive) -> Vec<i64> {
    integers_of(&arguments(directive))
}

/// The text arguments a directive spells, in order.
fn texts(directive: &Directive) -> Vec<&str> {
    directive
        .args
        .iter()
        .flatten()
        .filter_map(ZrdValue::as_text)
        .collect()
}

/// Whether a directive's arguments spell `name` **at any depth** — the
/// `SET_AI_NET` pairs nest their actor inside a list, so a top-level scan
/// would report an actor the record in fact names.
fn spells(directive: &Directive, name: &str) -> bool {
    let mut found = Vec::new();
    for argument in arguments(directive) {
        walk_texts(&argument, &mut found);
    }
    found.iter().any(|text| text == name)
}

/// The cross-objective **block addresses** a directive spells.
///
/// Measured (M02-B-FU3 #802, re-derived by M06-B-FU3 #819): a spelled address is
/// the one-based number of a numbered block and the original's parse decrements
/// it to the record index `address − 1`. A nap contributes only child0 — child1
/// is the delay in seconds after which the target re-wakes, never an address.
fn addresses(directive: &Directive) -> Vec<i64> {
    let mut ints = integers(directive);
    if directive.key == "NAP_OBJECTIVE_WHEN_I_COMPLETE" {
        ints.truncate(1);
    }
    ints
}

/// Whether a directive addresses other blocks at all.
fn addresses_blocks(directive: &Directive) -> bool {
    directive.key.ends_with("_OBJECTIVE_WHEN_I_COMPLETE") || directive.key == "TICK_DEPENDS_ON_OBJ"
}

/// The argument lists one key is spelled with, in block order.
fn spelled(blocks: &[(u32, Vec<Directive>)], key: &str) -> Vec<Vec<ZrdValue>> {
    blocks
        .iter()
        .flat_map(|(_, directives)| directives.iter())
        .filter(|directive| directive.key == key)
        .map(arguments)
        .collect()
}

/// The directives of one numbered block, in spelling order.
fn block(blocks: &[(u32, Vec<Directive>)], number: u32) -> &[Directive] {
    &blocks
        .iter()
        .find(|(n, _)| *n == number)
        .unwrap_or_else(|| panic!("OBJECTIVE{number} exists"))
        .1
}

/// The argument lists one key is spelled with inside one block.
fn sites(blocks: &[(u32, Vec<Directive>)], number: u32, key: &str) -> Vec<Vec<ZrdValue>> {
    block(blocks, number)
        .iter()
        .filter(|directive| directive.key == key)
        .map(arguments)
        .collect()
}

/// One always-awake search block: `(block, zones, retired target, wakes, marker)`.
type SearchBlock<'a> = (u32, Vec<&'a str>, &'a str, [u32; 2], u32);

/// The blocks of the record that spell a key.
fn with(blocks: &[(u32, Vec<Directive>)], key: &str) -> Vec<u32> {
    blocks
        .iter()
        .filter(|(_, directives)| directives.iter().any(|directive| directive.key == key))
        .map(|(number, _)| *number)
        .collect()
}

/// `(predecessor, target)` of every completion edge — a wake or a nap — plus
/// `(gate target, dependency)` for every `TICK_DEPENDS_ON_OBJ`.
///
/// A **kill** is deliberately not an edge here: killing a block prevents it, it
/// does not enter it, and reading kills as prerequisites would invent a path to
/// the success latch the record never spells.
fn prerequisites(blocks: &[(u32, Vec<Directive>)]) -> BTreeMap<u32, Vec<u32>> {
    let mut map: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (number, directives) in blocks {
        for directive in directives {
            match directive.key.as_str() {
                "WAKE_OBJECTIVE_WHEN_I_COMPLETE" | "NAP_OBJECTIVE_WHEN_I_COMPLETE" => {
                    for address in addresses(directive) {
                        map.entry(address as u32).or_default().push(*number);
                    }
                }
                "TICK_DEPENDS_ON_OBJ" => {
                    for address in addresses(directive) {
                        map.entry(*number).or_default().push(address as u32);
                    }
                }
                _ => {}
            }
        }
    }
    for targets in map.values_mut() {
        targets.sort_unstable();
        targets.dedup();
    }
    map
}

/// Every block reachable from `target` by following prerequisites backwards —
/// the blocks that must be entered before `target` can be entered.
fn closure(map: &BTreeMap<u32, Vec<u32>>, target: u32) -> Vec<u32> {
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    let mut stack = vec![target];
    while let Some(node) = stack.pop() {
        if !seen.insert(node) {
            continue;
        }
        if let Some(predecessors) = map.get(&node) {
            stack.extend(predecessors.iter().copied());
        }
    }
    seen.into_iter().collect()
}

/// Collects every text node of a decoded `.zrd` document.
fn walk_texts(value: &ZrdValue, texts: &mut Vec<String>) {
    match value {
        ZrdValue::Text(text) => texts.push(text.clone()),
        ZrdValue::List(children) => {
            for child in children {
                walk_texts(child, texts);
            }
        }
        _ => {}
    }
}

/// Every `.zrd` text the whole installation declares, indexed once through
/// production discovery and the production `.zrd` decoder: the text to the set
/// of `(container, member)` pairs that spell it.
fn text_index() -> &'static BTreeMap<String, BTreeSet<(String, String)>> {
    static INDEX: OnceLock<BTreeMap<String, BTreeSet<(String, String)>>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let found = discover(&game_dir()).expect("production discovery reads the installation");
        let mut index: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
        for record in &found.manifest.files {
            let spelling = record.relative_spelling.as_str();
            if !spelling.to_lowercase().ends_with(".zbd") {
                continue;
            }
            let Ok(relative) = RelativePath::new(&spelling.to_lowercase()) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(found.manifest.host_root.join(spelling)) else {
                continue;
            };
            let discovery = discover_container(&relative.logical_key(), &relative, &bytes);
            for program in discovery.programs() {
                let Some(member) = program.locator().member() else {
                    continue;
                };
                let Ok(document) = decode_zrd(program.bytes()) else {
                    continue;
                };
                let mut texts = Vec::new();
                walk_texts(&document, &mut texts);
                for text in texts {
                    index
                        .entry(text)
                        .or_default()
                        .insert((spelling.to_owned(), member.to_owned()));
                }
            }
        }
        index
    })
}

/// Every `(container, member)` whose decoded texts spell `name` exactly,
/// **outside** M17's control member — the declarations a directive's operand
/// can resolve against, sorted.
fn declarations_of(name: &str) -> Vec<(String, String)> {
    text_index()
        .get(name)
        .map(|set| {
            set.iter()
                .filter(|(container, member)| {
                    !(container.eq_ignore_ascii_case(CONTAINER) && member == CONTROL_MEMBER)
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Every distinct `.zrd` text declared **outside** M17's control member.
fn texts_declared_outside_control() -> BTreeSet<String> {
    text_index()
        .iter()
        .filter(|(_, hits)| {
            hits.iter().any(|(container, member)| {
                !(container.eq_ignore_ascii_case(CONTAINER) && member == CONTROL_MEMBER)
            })
        })
        .map(|(text, _)| text.clone())
        .collect()
}

/// Every file of the installation whose **raw bytes** contain `needle` — the
/// scan that backs "declared by no file", independent of any decoder.
fn raw_files_containing(needle: &str) -> Vec<String> {
    let found = discover(&game_dir()).expect("production discovery reads the installation");
    let mut hits = Vec::new();
    for record in &found.manifest.files {
        let spelling = record.relative_spelling.as_str();
        let Ok(bytes) = std::fs::read(found.manifest.host_root.join(spelling)) else {
            continue;
        };
        if bytes
            .windows(needle.len())
            .any(|window| window == needle.as_bytes())
        {
            hits.push(spelling.to_owned());
        }
    }
    hits.sort();
    hits
}

// ---------------------------------------------------------------------------
// Retail: what M17's control program is
// ---------------------------------------------------------------------------

/// **The control program is the member that declares the numbered blocks.**
///
/// Of the 12 members of M17's reader archive exactly one declares numbered
/// `OBJECTIVE<N>` blocks: the eighth, `objectives.zrd`. The census's blocks and
/// sites equal an independent walk of the same document, the archive is the
/// program span M17-A bound, and both the production control binding and M17-A's
/// mission binding reach the same member, span and two digests that re-derive
/// from the archive's bytes. Size is not the rule: the one member longer than
/// the control member (`aiv.zrd`, 10 843 bytes) declares no block.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m17_b_the_control_program_is_the_member_that_declares_the_blocks() {
    let row = census().row(MISSION).expect("M17 is in the census");
    assert_eq!(row.container, CONTAINER);
    assert_eq!(
        row.container_sha256, CONTAINER_SHA256,
        "the reader archive is the program M17-A bound"
    );
    assert_eq!(row.members.len(), 12);

    let with_blocks: Vec<&str> = row
        .members
        .iter()
        .filter(|member| member.objective_blocks > 0)
        .map(|member| member.name.as_str())
        .collect();
    assert_eq!(with_blocks, [CONTROL_MEMBER]);
    let control: Vec<&str> = row
        .members
        .iter()
        .filter(|member| member.is_control)
        .map(|member| member.name.as_str())
        .collect();
    assert_eq!(control, [CONTROL_MEMBER]);

    let index = row
        .members
        .iter()
        .position(|member| member.is_control)
        .expect("one member is the control program");
    assert_eq!(index, 7, "the eighth member, not the first");
    let chosen = &row.members[index];
    assert_eq!(
        (chosen.offset, chosen.len),
        (CONTROL_OFFSET, CONTROL_LENGTH)
    );
    let longer: Vec<&str> = row
        .members
        .iter()
        .filter(|member| member.len > chosen.len)
        .map(|member| member.name.as_str())
        .collect();
    assert_eq!(
        longer,
        [AIRCRAFT_MEMBER],
        "size is not the rule: the one member longer than the control member declares no block"
    );

    let record = row.record().expect("M17 has a measured control program");
    assert_eq!((record.blocks(), record.sites()), (BLOCKS, SITES));
    assert_eq!(record.keys().len(), KEYS);

    let (document, member) = read_control_member(&game_dir(), MISSION)
        .expect("the rule finds M17's control member again");
    assert_eq!(member.name, CONTROL_MEMBER);
    assert_eq!(member.objective_blocks, BLOCKS);
    assert!(member.is_control);
    assert_eq!(
        (member.offset, member.len),
        (CONTROL_OFFSET, CONTROL_LENGTH)
    );

    let blocks = blocks_of(&document);
    let numbers: Vec<u32> = blocks.iter().map(|(number, _)| *number).collect();
    assert_eq!(
        numbers,
        (1..=BLOCKS).collect::<Vec<_>>(),
        "numbered 1..=38, no gaps"
    );
    let walked: u32 = blocks
        .iter()
        .map(|(_, directives)| directives.len() as u32)
        .sum();
    assert_eq!(walked, record.sites(), "the independent walk agrees");
    assert!(
        record.refusals().is_empty(),
        "every block is a readable list"
    );
    assert!(
        record.unclassified_record_keys().is_empty(),
        "M17 spells no record-level key outside the measured vocabulary"
    );

    // The production binding and the census must not disagree about the
    // mission, the program, the member or the record.
    let bound = control_binding();
    assert_eq!(bound.mission.as_str(), "mission/ch4-m02");
    assert_eq!(bound.program_id.as_str(), "script/c4-m02-zrdr");
    assert_eq!(bound.program_asset, CONTAINER);
    assert_eq!(bound.program_length, CONTAINER_LENGTH);
    assert_eq!(bound.program_sha256, row.container_sha256);
    assert_eq!(bound.control_member, CONTROL_MEMBER);
    assert_eq!(
        (bound.control_offset, bound.control_length),
        (CONTROL_OFFSET, CONTROL_LENGTH)
    );
    assert_eq!(
        (bound.record.blocks(), bound.record.sites()),
        (BLOCKS, SITES)
    );
    assert_eq!(bound.record.vocabulary(), KEYS as u32);

    // …and both agree with the mission binding M17-A committed: one mission id,
    // one program id, one span, one digest.
    let mission = mission_binding();
    assert_eq!(
        mission.catalog_id.as_ref(),
        Some(&bound.mission),
        "the mission binding and the control binding name one mission"
    );
    assert_eq!(
        mission.program_id.as_ref(),
        Some(&bound.program_id),
        "the mission binding and the control binding name one program"
    );
    let span = mission
        .source_spans
        .iter()
        .find(|span| span.asset_id == CONTAINER)
        .expect("the mission binding cites the program archive");
    assert_eq!((span.offset, span.length), (0, CONTAINER_LENGTH));
    assert_eq!(span.sha256, CONTAINER_SHA256);

    // The spans and digests re-derive from the archive's own bytes.
    let bytes = std::fs::read(game_dir().join(CONTAINER)).expect("the archive reads");
    assert_eq!(bytes.len() as u64, CONTAINER_LENGTH);
    assert_eq!(sha256(&bytes).to_hex(), CONTAINER_SHA256);
    let start = CONTROL_OFFSET as usize;
    let end = start + CONTROL_LENGTH as usize;
    assert_eq!(
        sha256(&bytes[start..end]).to_hex(),
        CONTROL_SHA256,
        "the control member's digest re-derives from the member's own bytes"
    );
    assert_eq!(bound.control_sha256, CONTROL_SHA256);

    // The detection-zone member this stage leans on is the same archive's own
    // member, at the span the census reports.
    let dzones = row
        .members
        .iter()
        .find(|member| member.name == DZONES_MEMBER)
        .expect("the archive ships its detection-zone member");
    assert_eq!((dzones.offset, dzones.len), (DZONES_OFFSET, DZONES_LENGTH));
    assert_eq!(
        sha256(&bytes[DZONES_OFFSET as usize..(DZONES_OFFSET + DZONES_LENGTH) as usize]).to_hex(),
        DZONES_SHA256,
        "the detection-zone member's digest re-derives from its own bytes"
    );
}

/// **Every directive key M17 spells has exactly one disposition, and none is
/// refused.**
///
/// One key is a terminal outcome (`INSTANTWIN`, one bare site at block 14), the
/// other 21 have a measured effect, no key is `Unmeasured`, and the sites add
/// up to the census's own total. The record-level vocabulary is measured beside
/// it: five fields, no record sound, no key outside the vocabulary.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m17_b_every_directive_m17_spells_has_a_disposition_and_none_is_refused() {
    let record = census()
        .row(MISSION)
        .expect("M17 is in the census")
        .record()
        .expect("M17 has a measured control program");

    let measured: BTreeMap<&str, u32> = record
        .keys()
        .iter()
        .map(|key| (key.key.as_str(), key.sites))
        .collect();
    let expected: BTreeMap<&str, u32> = [
        ("ADD_OBJECTIVE_TARGET", 1),
        ("ANIM_STATE", 1),
        ("BEGIN_DORMANT", 34),
        ("COMPLETED_SOUND_GROUP", 18),
        ("DANGER_ZONES_COMPLETED", 4),
        ("DANGER_ZONES_COMPLETION_COUNT", 4),
        ("DEDG", 2),
        ("IDENTITY", 3),
        ("INACTIVE1", 1),
        ("INSTANTWIN", 1),
        ("KILL_OBJECTIVE_WHEN_I_COMPLETE", 9),
        ("NAP_OBJECTIVE_WHEN_I_COMPLETE", 14),
        ("REMOVE_OBJECTIVE_TARGET", 8),
        ("SET_AI_NET", 6),
        ("STOP_QUEUED_SOUNDS", 1),
        ("TICK_DEPENDS_ON_OBJ", 1),
        ("TRAVELERS", 4),
        ("WAKEUP_ENEMIES", 6),
        ("WAKEUP_ZEP_TURRETS", 1),
        ("WAKE_ANIM", 1),
        ("WAKE_OBJECTIVE_WHEN_I_COMPLETE", 14),
        ("WARP_VEHICLE", 1),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        measured, expected,
        "M17's exact 22-key vocabulary and its site counts changed"
    );
    assert_eq!(measured.len(), KEYS);
    let sites: u32 = measured.values().sum();
    assert_eq!(sites, SITES, "the keys' own sites do not add up");

    // Dispositions: one terminal outcome, 21 measured keys, nothing unmeasured
    // and nothing refused.
    let terminal: Vec<&str> = record
        .keys()
        .iter()
        .filter(|key| {
            matches!(
                key.disposition(),
                DirectiveDisposition::TerminalOutcome { .. }
            )
        })
        .map(|key| key.key.as_str())
        .collect();
    assert_eq!(terminal, ["INSTANTWIN"]);
    assert_eq!(
        record.key("INSTANTWIN").map(|key| key.disposition()),
        Some(DirectiveDisposition::TerminalOutcome {
            outcome: TerminalOutcome::Succeeded
        })
    );
    for key in record.keys() {
        if key.key == "INSTANTWIN" {
            continue;
        }
        assert!(
            matches!(key.disposition(), DirectiveDisposition::Measured(_)),
            "{} is not measured: {:?}",
            key.key,
            key.disposition()
        );
    }
    assert!(
        record.unmeasured().is_empty(),
        "an unmeasured key exists: {:?}",
        record.unmeasured()
    );
    assert!(
        record.refusals().is_empty(),
        "a block refusal exists: {:?}",
        record.refusals()
    );
    assert!(
        record.unclassified_record_keys().is_empty(),
        "an unclassified record key exists"
    );

    // The record-level fields, as stored: the mission timer, the authored
    // player start and the three animation lists (all three empty).
    let fields = [
        (ControlRecordField::MissionTimer, 1),
        (ControlRecordField::PlayerInit, 1),
        (ControlRecordField::AnimList(AnimList::Restore), 1),
        (ControlRecordField::AnimList(AnimList::Execute), 1),
        (ControlRecordField::AnimList(AnimList::Invalidate), 1),
    ];
    assert_eq!(
        record.record_fields(),
        &fields[..],
        "the record-level fields changed"
    );
    assert!(
        record.record_sounds().is_empty(),
        "M17 spells a record-level sound key: {:?}",
        record.record_sounds()
    );
}

/// **The sheet's forced-airframe priority: M17's record writes nothing on the
/// player, and the airframe it launches in comes from the measured campaign
/// chain rather than from this mission's data.**
///
/// Three independent facts pin the real bindings the sheet asks for first:
/// no site other than the four `TRAVELERS` subjects spells `player`, the one
/// vehicle-teleport site names the ace's gyro, and
/// `cs_app::mission_start::recover_retail_start_configuration` reads M17's own
/// `aiv.zrd` (whose player record carries the none value in field 0 and no
/// airframe field at all) and binds `airframe/player_pfighter` from the engine
/// state, not an autogyro. The archive carries no `ia.zrd`, so the instant-action
/// `player_plane` assignment cannot apply either. What the original *does* at a
/// forced-airframe transition is runtime behaviour and stays unmeasured (M17-C).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m17_b_no_directive_writes_the_player_and_the_airframe_is_the_campaign_chains() {
    let document = control_document();
    let blocks = blocks_of(&document);

    // Every site that spells `player` at all — at any depth, so a nested
    // `{player, …}` operand could not hide — four rows, all of them the subject
    // operand of the proximity evaluator. Nothing writes the player.
    let mut player_sites = Vec::new();
    for (number, directives) in &blocks {
        for directive in directives {
            if spells(directive, "player") {
                player_sites.push((*number, directive.key.clone()));
            }
        }
    }
    assert_eq!(
        player_sites,
        vec![
            (31, "TRAVELERS".to_owned()),
            (32, "TRAVELERS".to_owned()),
            (33, "TRAVELERS".to_owned()),
            (34, "TRAVELERS".to_owned()),
        ],
        "the record spells `player` somewhere other than the four proximity subjects"
    );
    // …and at those four it is the first operand, the subject the evaluator
    // reads rather than a target any effect names.
    for number in [31, 32, 33, 34] {
        assert_eq!(
            texts(
                block(&blocks, number)
                    .iter()
                    .find(|directive| directive.key == "TRAVELERS")
                    .expect("the proximity site exists")
            ),
            ["player", "APPROACHING", "bhatgyro_1"],
            "OBJECTIVE{number}'s subject is not the player closing on the ace"
        );
    }

    // The record-level fields spell no text either: the player's start is
    // numbers, and the three animation lists are empty.
    let mut field_texts = Vec::new();
    for (key, value) in zrd_flat_fields(objective_record(&document)) {
        if objective_block_number(key).is_none() {
            walk_texts(value, &mut field_texts);
        }
    }
    assert!(
        field_texts.is_empty(),
        "a record-level field spells a name: {field_texts:?}"
    );

    // The one directive that moves an object moves the ace's gyro: one site,
    // four points, and the last point carries a label.
    assert_eq!(
        spelled(&blocks, "WARP_VEHICLE"),
        [vec![
            ZrdValue::Text("bhatgyro_1".to_owned()),
            ZrdValue::List(vec![
                ZrdValue::Float(-8778.0),
                ZrdValue::Float(899.0),
                ZrdValue::Float(-6330.0),
                ZrdValue::Float(102.0),
            ]),
            ZrdValue::List(vec![
                ZrdValue::Float(-8031.0),
                ZrdValue::Float(569.0),
                ZrdValue::Float(-3530.0),
                ZrdValue::Float(-144.0),
            ]),
            ZrdValue::List(vec![
                ZrdValue::Float(-3600.0),
                ZrdValue::Float(741.0),
                ZrdValue::Float(-3546.0),
                ZrdValue::Float(81.0),
            ]),
            ZrdValue::List(vec![
                ZrdValue::Float(-2758.0),
                ZrdValue::Float(544.0),
                ZrdValue::Float(-6619.0),
                ZrdValue::Float(88.0),
                ZrdValue::Text("pp5".to_owned()),
            ]),
        ]],
        "the record's only teleport site changed"
    );
    let record = census()
        .row(MISSION)
        .expect("M17 is in the census")
        .record()
        .expect("M17 has a measured control program");
    let operations: BTreeSet<DirectiveOperation> = record
        .measured()
        .into_iter()
        .map(|(_, directive)| directive.operation)
        .collect();
    assert_eq!(
        operations,
        BTreeSet::from([
            DirectiveOperation::AnimationStates,
            DirectiveOperation::AssignNet,
            DirectiveOperation::CompletedSoundGroup,
            DirectiveOperation::DangerZoneFlags,
            DirectiveOperation::DangerZoneThreshold,
            DirectiveOperation::DependencyGate,
            DirectiveOperation::DormantStart,
            DirectiveOperation::EnemyGroupDepletion,
            DirectiveOperation::InactiveMembers,
            DirectiveOperation::KillObjectives,
            DirectiveOperation::NapObjective,
            DirectiveOperation::PresentationIdentity,
            DirectiveOperation::SetTargetFlag {
                objective: true,
                set: true
            },
            DirectiveOperation::SetTargetFlag {
                objective: true,
                set: false
            },
            DirectiveOperation::StopQueuedSounds,
            DirectiveOperation::Travelers,
            DirectiveOperation::WakeAnimation,
            DirectiveOperation::WakeEnemies,
            DirectiveOperation::WakeObjectives,
            DirectiveOperation::WakeZeppelinTurrets,
            DirectiveOperation::WarpVehicle,
        ]),
        "the measured operation set of M17's vocabulary changed"
    );

    // …and the archive has no instant-action scenario whose `player_plane`
    // would decide the airframe instead of the campaign chain.
    let row = census().row(MISSION).expect("M17 is in the census");
    assert!(
        !row.members.iter().any(|member| member.name == "ia.zrd"),
        "M17's archive carries an instant-action scenario, so the campaign chain would not decide \
         the airframe"
    );

    // The production start configuration reads this mission's own aircraft
    // table: one player record (no wingmates), the measured pose the control
    // record's PLAYER_INIT repeats, and the campaign chain's airframe.
    let configuration = recover_retail_start_configuration(&game_dir(), MISSION)
        .expect("M17's start configuration reads through production code");
    assert_eq!(configuration.mission(), MISSION);
    assert!(
        configuration.wingmates().is_empty(),
        "M17 launches no `wingman_<n>` record: {:?}",
        configuration.wingmates()
    );
    let Resolved::Known(player) = configuration.player() else {
        panic!("M17's aircraft table does not hold exactly one player record")
    };
    assert_eq!(player.value.name, "player");
    assert_eq!(
        player.value.field_zero,
        Some(u32::MAX),
        "the player's field 0 is not the none value"
    );
    let pose = player
        .value
        .stored_pose
        .expect("M17's player record has the measured pose shape");
    assert_eq!(pose.position, [-5629.0, 595.0, -6860.0]);
    assert_eq!(pose.heading, 85.0);

    // The airframe: the measured engine-state binding when the owner's image is
    // available, and an explicit refusal naming that input when it is not —
    // both are production behaviour, and neither is the autogyro the sheet's
    // discovery cue names.
    let autogyro = ContentId::from_source(ContentKind::Airframe, AIRFRAME_TABLE[0].scene_root)
        .expect("the autogyro row builds an id");
    match configuration.airframe() {
        Resolved::Known(known) => {
            assert_eq!(
                known.value.as_str(),
                "airframe/player_pfighter",
                "the campaign chain's own row decides M17's airframe"
            );
            assert_ne!(
                known.value, autogyro,
                "the measured chain gave M17 the autogyro the discovery cue names"
            );
            assert_eq!(
                AIRFRAME_TABLE[CAMPAIGN_AIRFRAME_ROW].scene_root, "player_pfighter",
                "the chain's row moved"
            );
        }
        Resolved::Unknown { reason, .. } => {
            assert!(
                reason.contains("CS_ENGINE_IMAGE"),
                "without the owner's image the airframe must refuse by naming that input: {reason}"
            );
        }
    }

    // The record's own PLAYER_INIT repeats the aircraft table's pose: same
    // position vector, and the heading the stored pose carries.
    let init = record_field(&document, "PLAYER_INIT");
    let expected_init = [
        ZrdValue::Int(1),
        ZrdValue::List(vec![
            ZrdValue::Float(-5629.0),
            ZrdValue::Float(595.0),
            ZrdValue::Float(-6860.0),
        ]),
        ZrdValue::List(vec![
            ZrdValue::Float(0.0),
            ZrdValue::Float(85.0),
            ZrdValue::Float(0.0),
        ]),
        ZrdValue::Float(0.8),
        ZrdValue::Float(180.0),
    ];
    assert_eq!(
        init, expected_init,
        "the authored player start, as the record spells it"
    );
    let init_position = floats(init[1].as_list().expect("a three-number vector"));
    assert_eq!(
        init_position,
        pose.position.to_vec(),
        "PLAYER_INIT's position and the aircraft table's stored position disagree"
    );
    let init_middle = floats(init[2].as_list().expect("a three-number vector"));
    assert_eq!(
        init_middle,
        [0.0, pose.heading, 0.0],
        "PLAYER_INIT's middle vector does not carry the stored heading"
    );
}

/// The stored value of one record-level field.
fn record_field(document: &ZrdValue, name: &str) -> Vec<ZrdValue> {
    zrd_flat_fields(objective_record(document))
        .into_iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.as_list().expect("a record field is a list").to_vec())
        .unwrap_or_else(|| panic!("the record carries the field {name}"))
}

/// The float members of a stored list, in order.
fn floats(values: &[ZrdValue]) -> Vec<f32> {
    values
        .iter()
        .filter_map(|value| match value {
            ZrdValue::Float(number) => Some(*number),
            _ => None,
        })
        .collect()
}

/// **The sheet's search-trigger priority: four blocks start awake over seven
/// named detection zones, and every name resolves to a node in M17's own world
/// container through the production trigger-volume survey.**
///
/// The four `DANGER_ZONES_COMPLETED` blocks (2, 3, 4, 6) are exactly the blocks
/// with no `BEGIN_DORMANT`, so they evaluate from the mission's first tick;
/// each carries threshold 1, retires one objective target, wakes two more
/// blocks and naps the matching "found" marker for two seconds. The four
/// `TRAVELERS` sites are the other half of the priority: the player closing to
/// 500 about `bhatgyro_1` wakes block 24 and kills that marker. The zone names
/// are cross-checked against `ZBD/C4/zrdr.zbd`'s `dzpath1…15` nodes with no
/// declaration gap, and M17's own `dzones.zrd` declaration — its three keys, its
/// stored order and its 13 `objective_numbers` pairs — is carried as data whose
/// meaning stays unmeasured.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m17_b_the_search_triggers_are_the_four_always_awake_danger_zone_blocks() {
    let document = control_document();
    let blocks = blocks_of(&document);

    // Exactly four blocks start awake: the ones with no dormant marker.
    let dormant = with(&blocks, "BEGIN_DORMANT");
    assert_eq!(dormant.len(), 34);
    let awake: Vec<u32> = (1..=BLOCKS)
        .filter(|number| !dormant.contains(number))
        .collect();
    assert_eq!(
        awake,
        [2, 3, 4, 6],
        "the always-awake set is not the four danger-zone blocks"
    );
    // Two of the 34 dormant blocks arm a clock; the rest are woken by edges.
    let timed: Vec<(u32, f32)> = blocks
        .iter()
        .filter(|(_, directives)| directives.iter().any(|d| d.key == "BEGIN_DORMANT"))
        .map(|(number, directives)| {
            let directive = directives
                .iter()
                .find(|d| d.key == "BEGIN_DORMANT")
                .expect("the block spells the marker");
            match arguments(directive).first() {
                Some(ZrdValue::Float(wake)) => (*number, *wake),
                other => panic!("OBJECTIVE{number} spells a float wake, not {other:?}"),
            }
        })
        .filter(|(_, wake)| *wake >= 0.0)
        .collect();
    assert_eq!(
        timed,
        [(1, 2.0), (23, 5.0)],
        "the record's timed self-wakes changed"
    );

    // Each of the four spells threshold 1 over its own zones, retires one
    // target, wakes two blocks and naps one marker for two seconds — the
    // expected site-for-site spelling.
    let expected: [SearchBlock<'_>; 4] = [
        (
            2,
            vec!["dzpath7", "dzpath8", "dzpath9"],
            "dz8",
            [18, 31],
            35,
        ),
        (3, vec!["dzpath3", "dzpath4"], "dz3", [19, 32], 36),
        (4, vec!["dzpath11"], "dz11", [5, 33], 37),
        (6, vec!["dzpath13"], "dz13", [29, 34], 38),
    ];
    for (number, zones, target, wakes, marker) in expected {
        assert_eq!(
            sites(&blocks, number, "DANGER_ZONES_COMPLETION_COUNT"),
            [vec![ZrdValue::Int(1)]],
            "OBJECTIVE{number} does not spell threshold 1"
        );
        assert_eq!(
            sites(&blocks, number, "DANGER_ZONES_COMPLETED"),
            [zones
                .iter()
                .map(|zone| ZrdValue::Text((*zone).to_owned()))
                .collect::<Vec<_>>()],
            "OBJECTIVE{number}'s zone list changed"
        );
        assert_eq!(
            sites(&blocks, number, "REMOVE_OBJECTIVE_TARGET"),
            [vec![ZrdValue::Text(target.to_owned())]],
            "OBJECTIVE{number} does not retire {target}"
        );
        assert_eq!(
            integers_of(
                &sites(&blocks, number, "WAKE_OBJECTIVE_WHEN_I_COMPLETE")
                    .pop()
                    .expect("OBJECTIVE{number} wakes two blocks")
            ),
            wakes.map(i64::from),
            "OBJECTIVE{number}'s wake list changed"
        );
        assert_eq!(
            sites(&blocks, number, "NAP_OBJECTIVE_WHEN_I_COMPLETE"),
            [vec![ZrdValue::Int(marker), ZrdValue::Float(2.0),]],
            "OBJECTIVE{number} does not nap its marker for two seconds"
        );
        assert_eq!(
            sites(&blocks, number, "BEGIN_DORMANT"),
            Vec::<Vec<ZrdValue>>::new(),
            "OBJECTIVE{number} is not one of the always-awake blocks"
        );
    }

    // The four proximity sites: identical shape, radius 500, one kill each.
    let approaches = spelled(&blocks, "TRAVELERS");
    assert_eq!(approaches.len(), 4);
    assert!(
        approaches.iter().all(|args| args == &approaches[0]),
        "the four proximity sites are not the same site: {approaches:?}"
    );
    for (number, marker) in [(31, 35), (32, 36), (33, 37), (34, 38)] {
        assert_eq!(
            sites(&blocks, number, "TRAVELERS"),
            [vec![
                ZrdValue::Text("player".to_owned()),
                ZrdValue::Text("APPROACHING".to_owned()),
                ZrdValue::Text("bhatgyro_1".to_owned()),
                ZrdValue::Float(500.0),
                ZrdValue::Int(1),
            ]],
            "OBJECTIVE{number}'s proximity trigger changed"
        );
        assert_eq!(
            integers_of(
                &sites(&blocks, number, "WAKE_OBJECTIVE_WHEN_I_COMPLETE")
                    .pop()
                    .expect("the approach wakes the primary")
            ),
            [24],
            "OBJECTIVE{number} does not wake OBJECTIVE24"
        );
        assert_eq!(
            integers_of(
                &sites(&blocks, number, "KILL_OBJECTIVE_WHEN_I_COMPLETE")
                    .pop()
                    .expect("the approach kills the marker")
            ),
            [i64::from(marker)],
            "OBJECTIVE{number} does not kill OBJECTIVE{marker}"
        );
    }
    // …and the four markers carry their own sound groups, two of them the same
    // name, so a test that read the marker list by position could not pass on a
    // duplicated pair by accident.
    for (marker, sound) in [
        (35, "snd_RM2Jimmys"),
        (36, "snd_RM2McCoys"),
        (37, "snd_RM2ShangriLa"),
        (38, "snd_RM2Jimmys"),
    ] {
        assert_eq!(
            sites(&blocks, marker, "COMPLETED_SOUND_GROUP"),
            [vec![ZrdValue::Text(sound.to_owned())]],
            "OBJECTIVE{marker}'s completion sound changed"
        );
    }

    // The zone names resolve through the production survey: M17's declaration
    // is this archive's own member, at the span the census reports, under the
    // three keys in stored order, with no gap against the chapter container.
    let survey = zone_survey();
    assert!(survey.zone_declarations_are_decoded());
    let declaration = survey
        .declarations()
        .iter()
        .find(|declaration| declaration.mission() == MISSION)
        .expect("M17's dzones.zrd decoded into a declaration");
    assert_eq!(declaration.member_container(), "zbd/c4/m02/zrdr.zbd");
    assert_eq!(declaration.member_container_sha256(), CONTAINER_SHA256);
    assert_eq!(
        declaration.member_span(),
        (DZONES_OFFSET, DZONES_LENGTH),
        "the survey's member span is not the census's dzones.zrd span"
    );
    let keys = [
        ZoneDeclarationKey::Disable,
        ZoneDeclarationKey::NoSnapshot,
        ZoneDeclarationKey::ObjectiveNumbers,
    ];
    assert_eq!(
        declaration.keys(),
        &keys[..],
        "the declaration's stored key order changed"
    );
    let disabled = ["dzpath15".to_owned()];
    assert_eq!(
        declaration.disable(),
        &disabled[..],
        "the disabled zone changed"
    );
    let excluded = ["dzpath12".to_owned()];
    assert_eq!(
        declaration.no_snapshot(),
        &excluded[..],
        "the snapshot-excluded zone changed"
    );
    let numbers = [
        ("dzpath1".to_owned(), 18),
        ("dzpath2".to_owned(), 19),
        ("dzpath3".to_owned(), 20),
        ("dzpath4".to_owned(), 21),
        ("dzpath5".to_owned(), 22),
        ("dzpath6".to_owned(), 23),
        ("dzpath7".to_owned(), 24),
        ("dzpath8".to_owned(), 25),
        ("dzpath9".to_owned(), 26),
        ("dzpath10".to_owned(), 27),
        ("dzpath11".to_owned(), 28),
        ("dzpath13".to_owned(), 29),
        ("dzpath14".to_owned(), 30),
    ];
    assert_eq!(
        declaration.objective_numbers(),
        &numbers[..],
        "the zone → objective-number pairs changed (their meaning is unmeasured)"
    );
    assert_eq!(declaration.named_zones().len(), 15);

    let gaps: Vec<_> = survey
        .declaration_gaps()
        .into_iter()
        .filter(|gap| gap.mission == MISSION)
        .collect();
    assert!(
        gaps.is_empty(),
        "M17 names a zone its world container has no node for: {gaps:?}"
    );

    // Every zone the control record spells has a node in the chapter-4
    // container, and that container carries exactly the 15 numbered zones the
    // declaration accounts for.
    let c4 = WorldId::from_key("c4").expect("c4 is a world key");
    let nodes: BTreeSet<String> = survey
        .volumes_in(&c4)
        .iter()
        .map(|volume| volume.zone().to_owned())
        .collect();
    let expected_nodes: BTreeSet<String> =
        (1..=15).map(|number| format!("dzpath{number}")).collect();
    assert_eq!(
        nodes, expected_nodes,
        "the chapter-4 container's numbered zones changed"
    );
    let spelled_zones: BTreeSet<&str> = blocks
        .iter()
        .flat_map(|(_, directives)| directives.iter())
        .filter(|directive| directive.key == "DANGER_ZONES_COMPLETED")
        .flat_map(|directive| texts(directive))
        .collect();
    assert_eq!(
        spelled_zones,
        BTreeSet::from([
            "dzpath3", "dzpath4", "dzpath7", "dzpath8", "dzpath9", "dzpath11", "dzpath13",
        ]),
        "the zones the search-trigger blocks spell changed"
    );
    for zone in &spelled_zones {
        assert!(
            nodes.contains(*zone),
            "the control record spells {zone}, which the chapter container carries no node for"
        );
        assert!(
            numbers.iter().any(|(name, _)| name == zone),
            "{zone} is spelled by the control record but not declared by M17's dzones.zrd"
        );
    }
    // …and each of them is declared by this archive's own member rather than
    // only by a sibling's copy of the file.
    for zone in &spelled_zones {
        assert!(
            declarations_of(zone)
                .iter()
                .any(
                    |(container, member)| container.eq_ignore_ascii_case(CONTAINER)
                        && member == DZONES_MEMBER
                ),
            "{zone} is not declared by M17's own detection-zone member"
        );
    }
}

/// **The sheet's ace-encounter priority: `bhatgyro_1`'s net, wake, warp and
/// four approach triggers are record data, and two of the actors the record
/// directs are declared by no file in the installation.**
///
/// The lifecycle is spelled block by block below. The declaration half is the
/// measurement the sheet asks for first: of the actors the record names, the
/// aircraft table of this archive declares most of them, `bhatbrigand_4` is
/// declared only by sibling missions' tables, and `bhatbrigand_13` /
/// `bhatbrigand_14` — the two `SET_AI_NET` operands of blocks 20 and 30 — are
/// declared by **no** `.zrd` member anywhere and appear in **no** file's raw
/// bytes except the directive site that spells them. That gap is recorded, not
/// worked around.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m17_b_the_ace_lifecycle_is_record_data_and_two_actors_are_declared_nowhere() {
    let blocks = blocks_of(&control_document());

    // The net assignments: six sites, all pointing at one chapter node.
    assert_eq!(
        spelled(&blocks, "SET_AI_NET"),
        [
            vec![pair("bhatgyro_1", "M2Blacke")],
            vec![
                pair("bhatbrigand_1", "M2Blacke"),
                pair("bhatbrigand_2", "M2Blacke"),
                pair("bhatbrigand_3", "M2Blacke"),
                pair("bhatbrigand_4", "M2Blacke"),
            ],
            vec![
                pair("bhatbrigand_5", "M2Blacke"),
                pair("bhatbrigand_6", "M2Blacke"),
                pair("bhatbrigand_13", "M2Blacke"),
            ],
            vec![
                pair("bhatbrigand_7", "M2Blacke"),
                pair("bhatbrigand_8", "M2Blacke")
            ],
            vec![
                pair("bhatbrigand_9", "M2Blacke"),
                pair("bhatbrigand_10", "M2Blacke")
            ],
            vec![
                pair("bhatbrigand_11", "M2Blacke"),
                pair("bhatbrigand_12", "M2Blacke"),
                pair("bhatbrigand_14", "M2Blacke"),
            ],
        ],
        "the six net sites changed"
    );
    assert_eq!(
        declarations_of("M2Blacke"),
        [("ZBD/C4/zrdr.zbd".to_owned(), "neindex.zrd".to_owned())],
        "the chapter node the ace is pointed at is declared where the chapter world index spells it"
    );

    // The wake: the ace's gyro comes up at block 17, which also wakes the
    // Blacke-down primary and naps its own net site five seconds later.
    assert_eq!(
        sites(&blocks, 17, "WAKEUP_ENEMIES"),
        [vec![ZrdValue::Text("bhatgyro_1".to_owned())]],
        "the ace's wake changed"
    );
    assert_eq!(
        integers_of(
            &sites(&blocks, 17, "WAKE_OBJECTIVE_WHEN_I_COMPLETE")
                .pop()
                .expect("OBJECTIVE17 wakes")
        ),
        [8, 25],
        "OBJECTIVE17's wake list changed"
    );
    assert_eq!(
        sites(&blocks, 17, "NAP_OBJECTIVE_WHEN_I_COMPLETE"),
        [vec![ZrdValue::Int(15), ZrdValue::Float(5.0)]],
        "the net site's nap changed"
    );

    // The warp: block 23 arms a five-second clock and teleports the gyro to one
    // of four spelled points, the last of which carries a name the installation
    // declares only inside another mission's aircraft table.
    assert_eq!(
        sites(&blocks, 23, "BEGIN_DORMANT"),
        [vec![ZrdValue::Float(5.0)]],
        "the warp block's clock changed"
    );
    let warp = sites(&blocks, 23, "WARP_VEHICLE")
        .pop()
        .expect("the ace is warped");
    assert_eq!(warp.len(), 5, "the warp site is a name and four points");
    assert_eq!(warp[0], ZrdValue::Text("bhatgyro_1".to_owned()));
    let point_sizes: Vec<usize> = warp[1..]
        .iter()
        .map(|point| {
            point
                .as_list()
                .unwrap_or_else(|| panic!("a warp point is a list, not {point:?}"))
                .len()
        })
        .collect();
    assert_eq!(
        point_sizes,
        [4, 4, 4, 5],
        "the four warp points changed shape"
    );
    assert_eq!(
        declarations_of("pp5"),
        [("ZBD/C2/M02/zrdr.zbd".to_owned(), "aiv.zrd".to_owned())],
        "the named warp point is declared where the measurement found it"
    );

    // The approach: four identical proximity sites, then the primary that
    // retires the danger-zone blocks.
    assert_eq!(with(&blocks, "TRAVELERS"), [31, 32, 33, 34]);
    assert_eq!(
        sites(&blocks, 24, "IDENTITY"),
        [vec![
            ZrdValue::Text("PRIMARY".to_owned()),
            ZrdValue::Int(1),
            ZrdValue::Text("MSG_BRF_RMM2_OBJ1".to_owned()),
        ]],
        "the primary the approach wakes changed"
    );
    assert_eq!(
        integers_of(
            &sites(&blocks, 24, "KILL_OBJECTIVE_WHEN_I_COMPLETE")
                .pop()
                .expect("the primary retires the zones")
        ),
        [2, 3, 4, 6],
        "the primary does not retire the four search-trigger blocks"
    );

    // The declaration table of every actor the record names: where each one is
    // declared, as measured over the whole installation.
    let table: [(&str, Declaration); 17] = [
        ("player", Declaration::OwnAircraftTable),
        ("bhatgyro_1", Declaration::OwnAircraftTable),
        ("bswingman_1", Declaration::OwnAircraftTable),
        ("bhatbrigand_1", Declaration::OwnAircraftTable),
        ("bhatbrigand_2", Declaration::OwnAircraftTable),
        ("bhatbrigand_3", Declaration::OwnAircraftTable),
        ("bhatbrigand_5", Declaration::OwnAircraftTable),
        ("bhatbrigand_6", Declaration::OwnAircraftTable),
        ("bhatbrigand_7", Declaration::OwnAircraftTable),
        ("bhatbrigand_8", Declaration::OwnAircraftTable),
        ("bhatbrigand_9", Declaration::OwnAircraftTable),
        ("bhatbrigand_10", Declaration::OwnAircraftTable),
        ("bhatbrigand_11", Declaration::OwnAircraftTable),
        ("bhatbrigand_12", Declaration::OwnAircraftTable),
        ("bhatbrigand_4", Declaration::ElsewhereOnly),
        ("bhatbrigand_13", Declaration::Nowhere),
        ("bhatbrigand_14", Declaration::Nowhere),
    ];
    for (name, expected) in table {
        // The record really does spell every name in the table, so a row the
        // control member stopped naming cannot pass unnoticed.
        assert!(
            blocks
                .iter()
                .flat_map(|(_, directives)| directives.iter())
                .any(|directive| spells(directive, name)),
            "the record no longer spells {name}"
        );
        let hits = declarations_of(name);
        let in_own_archive = hits.iter().any(|(container, member)| {
            container.eq_ignore_ascii_case(CONTAINER) && member != CONTROL_MEMBER
        });
        let in_own_aircraft_table = hits.iter().any(|(container, member)| {
            container.eq_ignore_ascii_case(CONTAINER) && member == AIRCRAFT_MEMBER
        });
        let elsewhere = hits
            .iter()
            .any(|(container, _)| !container.eq_ignore_ascii_case(CONTAINER));
        match expected {
            Declaration::OwnAircraftTable => assert!(
                in_own_archive && in_own_aircraft_table,
                "{name} is not declared by M17's own aircraft table: {hits:?}"
            ),
            Declaration::ElsewhereOnly => assert!(
                !in_own_archive && elsewhere,
                "{name} is declared by M17's own archive after all: {hits:?}"
            ),
            Declaration::Nowhere => {
                assert!(hits.is_empty(), "{name} is declared somewhere: {hits:?}")
            }
        }
    }

    // …and for the two undeclared actors, the raw bytes agree with the
    // decoders: each name occurs in exactly one file of the installation, the
    // directive site that spells it.
    for name in ["bhatbrigand_13", "bhatbrigand_14"] {
        assert_eq!(
            raw_files_containing(name),
            [CONTAINER.to_owned()],
            "{name} occurs in a file other than the control archive"
        );
    }
    // The counter-example that keeps the two rows above honest: a name the same
    // scan finds in over a hundred files.
    let declared_everywhere = raw_files_containing("piratezep");
    assert!(
        declared_everywhere.len() > 100,
        "the raw scan no longer distinguishes a declared name from an undeclared one: {} files",
        declared_everywhere.len()
    );
}

/// Where a named actor of the control record is declared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Declaration {
    /// Declared by M17's own `aiv.zrd`.
    OwnAircraftTable,
    /// Declared by some other archive, but not by M17's.
    ElsewhereOnly,
    /// Declared by no file in the installation.
    Nowhere,
}

/// One `{actor, node}` pair as the `SET_AI_NET` sites spell it.
fn pair(actor: &str, node: &str) -> ZrdValue {
    ZrdValue::List(vec![
        ZrdValue::Text(actor.to_owned()),
        ZrdValue::Text(node.to_owned()),
    ])
}

/// **Every text the record spells is declared somewhere outside the control
/// member — except nine, which are recorded as the gaps they are.**
///
/// The record spells 128 distinct text nodes. All but nine are declared by
/// another `.zrd` member of some archive in the installation (this mission's
/// own `aiv.zrd`, `dzones.zrd`, `targets.zrd` and `zeppelins.zrd` included).
/// The nine are two HUD message ids, four queued-sound names, the two
/// undeclared actors, and `WARP_VEHICLE` itself — the one **directive key** no
/// other archive spells, which is why it is measured from the executable rather
/// than from a sibling record.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m17_b_every_text_the_record_spells_is_declared_outside_it_or_recorded_as_a_gap() {
    let mut texts = Vec::new();
    walk_texts(&control_document(), &mut texts);
    texts.sort();
    texts.dedup();
    assert_eq!(
        texts.len(),
        NAMED_TEXTS,
        "the control document's distinct text count changed"
    );

    let outside = texts_declared_outside_control();
    let exceptions: Vec<&str> = texts
        .iter()
        .map(String::as_str)
        .filter(|text| !outside.contains(*text))
        .collect();
    assert_eq!(
        exceptions, RECORD_ONLY_TEXTS,
        "the set of texts no other member declares changed"
    );
    for name in RECORD_ONLY_TEXTS {
        assert!(
            texts.contains(&name.to_owned()),
            "the record no longer spells {name}"
        );
    }
    // Each exception really is absent from every other member, and present in
    // the control member itself — the two halves of "record-only".
    for name in [
        "bhatbrigand_13",
        "bhatbrigand_14",
        "WARP_VEHICLE",
        "MSG_BRF_RMM2_OBJ1",
        "MSG_BRF_RMM2_OBJ2",
        "snd_c4-RM-m2_Zachary_15",
    ] {
        assert!(
            declarations_of(name).is_empty(),
            "{name} is declared by another member after all"
        );
    }
    // A name that *is* declared elsewhere is not an exception even when it
    // looks like one: the `dzpath` zones the record spells are declared by this
    // archive's own detection-zone member.
    for name in ["dzpath7", "dzpath13"] {
        assert!(
            outside.contains(name),
            "{name} fell out of the declared set"
        );
        assert!(
            declarations_of(name)
                .iter()
                .any(
                    |(container, member)| container.eq_ignore_ascii_case(CONTAINER)
                        && member == DZONES_MEMBER
                ),
            "{name} is no longer declared by M17's own detection-zone member"
        );
    }
}

/// **The record has exactly one terminal outcome, its latch's prerequisite
/// closure is spelled, and every address is in range.**
///
/// `INSTANTLOSS` is a key the original knows — `terminal_outcome_of` answers it
/// — but M17's record spells it **nowhere**, so the mission's control program
/// contains no failure cause at all: the failure half of M17-FAILURE is outside
/// this member and stays unknown here. All 48 spelled addresses lie in
/// `1..=38` under the measured one-based convention, the success latch's
/// closure is the 17 blocks listed, and killing a block is not a path into it.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m17_b_the_single_terminal_latch_is_gated_and_every_address_is_in_range() {
    let blocks = blocks_of(&control_document());
    let record = census()
        .row(MISSION)
        .expect("M17 is in the census")
        .record()
        .expect("M17 has a measured control program");

    assert_eq!(with(&blocks, "INSTANTWIN"), [14], "the success latch moved");
    assert_eq!(
        with(&blocks, "INSTANTLOSS"),
        Vec::<u32>::new(),
        "M17's control program spells a failure cause after all"
    );
    assert!(
        record.key("INSTANTLOSS").is_none(),
        "the vocabulary carries INSTANTLOSS, which the record does not spell"
    );
    assert_eq!(
        terminal_outcome_of("INSTANTLOSS"),
        Some(TerminalOutcome::Failed),
        "the original's failure key is still a terminal outcome"
    );
    assert_eq!(
        record.implemented().len(),
        1,
        "exactly one terminal site exists"
    );

    // Every address is a one-based block number in range, none is zero, and
    // the four address-carrying keys account for all of them.
    let mut all: Vec<i64> = Vec::new();
    let mut per_key: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, directives) in &blocks {
        for directive in directives {
            if !addresses_blocks(directive) {
                continue;
            }
            let spelled_addresses = addresses(directive);
            *per_key.entry(directive.key.as_str()).or_default() += spelled_addresses.len();
            all.extend(spelled_addresses);
        }
    }
    assert_eq!(
        per_key,
        [
            ("WAKE_OBJECTIVE_WHEN_I_COMPLETE", 21),
            ("KILL_OBJECTIVE_WHEN_I_COMPLETE", 12),
            ("NAP_OBJECTIVE_WHEN_I_COMPLETE", 14),
            ("TICK_DEPENDS_ON_OBJ", 1),
        ]
        .into_iter()
        .collect::<BTreeMap<_, _>>(),
        "the address budget changed"
    );
    assert_eq!(all.len(), EDGES);
    assert!(
        all.iter()
            .all(|address| (1..=i64::from(BLOCKS)).contains(address)),
        "an address is out of range: {all:?}"
    );
    assert!(!all.contains(&0), "a spelled address is 0");
    assert_eq!(with(&blocks, "TICK_DEPENDS_ON_OBJ"), [13]);
    assert_eq!(
        sites(&blocks, 13, "TICK_DEPENDS_ON_OBJ"),
        [vec![ZrdValue::Int(12)]],
        "the one dependency gate changed"
    );

    // The latch's prerequisite closure, over wake/nap edges and the gate.
    let map = prerequisites(&blocks);
    let latch_closure = closure(&map, 14);
    assert_eq!(
        latch_closure,
        [2, 3, 4, 6, 8, 9, 10, 11, 12, 13, 14, 17, 24, 31, 32, 33, 34],
        "the success latch's prerequisite closure changed"
    );
    assert_eq!(
        (1..=BLOCKS)
            .filter(|number| !latch_closure.contains(number))
            .count(),
        21,
        "the number of blocks outside the latch's closure changed"
    );
    // Killing block 2 does not enter it: its only way in would be a wake or a
    // nap, and no block spells either.
    assert!(
        !map.contains_key(&2),
        "the always-awake search block has a prerequisite: {:?}",
        map.get(&2)
    );
    // The three presentation identities the record spells, and nothing more.
    let identities: Vec<(u32, Vec<ZrdValue>)> = [8, 11, 24]
        .into_iter()
        .map(|number| {
            let mut site = sites(&blocks, number, "IDENTITY");
            let args = site
                .pop()
                .unwrap_or_else(|| panic!("OBJECTIVE{number} presents itself"));
            (number, args)
        })
        .collect();
    let expected_identities = [
        (
            8u32,
            vec![
                ZrdValue::Text("PRIMARY".to_owned()),
                ZrdValue::Int(2),
                ZrdValue::Text("MSG_BRF_RMM2_OBJ2".to_owned()),
            ],
        ),
        (
            11,
            vec![ZrdValue::Text("SECONDARY".to_owned()), ZrdValue::Int(11)],
        ),
        (
            24,
            vec![
                ZrdValue::Text("PRIMARY".to_owned()),
                ZrdValue::Int(1),
                ZrdValue::Text("MSG_BRF_RMM2_OBJ1".to_owned()),
            ],
        ),
    ];
    assert_eq!(
        identities, expected_identities,
        "the identity sites changed"
    );
}

/// **M17's record lowers completely: every condition lowers, every call binds
/// and `MissionProgram::validate` accepts the program.**
///
/// This is what "all discovered mission-specific behavior uses production
/// engine systems" buys: the record the census measured is the record
/// `cs_app::control_lowering` adapts, and the accounting rows are derived from
/// that attempt rather than stored beside it.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m17_b_every_call_binds_every_condition_lowers_and_m17s_record_completes() {
    let row = census().row(MISSION).expect("M17 is in the census");
    let attempt = row
        .lowering_attempt()
        .expect("M17's record lowers")
        .attempt();

    assert_eq!(
        attempt.mission.as_ref().map(String::as_str),
        Ok("mission/ch4-m02"),
        "the lowered program carries a different mission id"
    );
    assert_eq!(
        attempt.objectives, BLOCKS,
        "one objective per numbered block"
    );
    assert_eq!(attempt.conditions.len(), BLOCKS as usize);
    assert!(
        attempt
            .conditions
            .iter()
            .all(|outcome| matches!(outcome, ConditionOutcome::Lowered)),
        "a condition refused: {:?}",
        attempt.conditions
    );
    assert_eq!(attempt.calls.len(), SITES as usize, "one call per site");
    assert!(
        attempt
            .calls
            .iter()
            .all(|call| matches!(call, CallOutcome::Bound)),
        "a call refused: {:?}",
        attempt.calls
    );
    assert!(
        attempt.unbound_keys.is_empty(),
        "a key refused registration: {:?}",
        attempt.unbound_keys
    );
    assert_eq!(
        attempt.validation.as_ref().map(Vec::len),
        Some(0),
        "MissionProgram::validate reported errors: {:?}",
        attempt.validation
    );

    let lowering = row.lowering().expect("the accounting is derived");
    assert!(
        lowering.complete(),
        "a requirement is unmet: {:?}",
        lowering.unmet().map(|row| row.label()).collect::<Vec<_>>()
    );
    assert_eq!(
        lowering.unmet().count(),
        0,
        "unmet rows: {:?}",
        lowering.unmet().map(|row| row.label()).collect::<Vec<_>>()
    );
    assert_eq!(
        lowering.unmeasured_fields(),
        Vec::<String>::new(),
        "fields stay unmeasured"
    );
    assert!(row.is_measured());
    assert!(row.is_complete(), "M17's census row is not complete");
}

/// **M17's row is complete while the campaign gate stays shut.**
///
/// A complete row is a lowering claim about one mission; the campaign gate
/// closes over every row and every runtime observation, none of which this
/// stage supplies. Nothing here plays the mission.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m17_b_m17_is_complete_and_the_campaign_stays_unready() {
    let census = census();
    assert!(
        census.complete_missions().contains(&MISSION),
        "M17's row is not among the complete ones: {:?}",
        census.complete_missions()
    );
    assert!(
        census.measured_rows().any(|row| row.mission() == MISSION),
        "M17 is not a measured row"
    );
    assert!(
        census.rows().iter().any(|row| !row.is_complete()),
        "every row is complete, so the campaign gate would open on data this stage does not have"
    );
    assert!(
        !census.campaign_ready(),
        "the campaign gate opened without any runtime evidence"
    );
}

// ---------------------------------------------------------------------------
// Synthetic: the predicates the retail record leans on
// ---------------------------------------------------------------------------

fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

fn int(value: u32) -> ZrdValue {
    ZrdValue::Int(value)
}

fn float(value: f32) -> ZrdValue {
    ZrdValue::Float(value)
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
        ContentId::from_source(ContentKind::Mission, "accept-m17-b").map_err(|e| e.to_string()),
        "accept-m17-b",
        document,
        &record,
    )
}

/// The measured `WARP_VEHICLE` argument list, authored from the retail site's
/// own shape: a name and four points, the last carrying the point's label.
fn warp_arguments() -> Vec<ZrdValue> {
    vec![
        text("bhatgyro_1"),
        ZrdValue::List(vec![
            float(-8778.0),
            float(899.0),
            float(-6330.0),
            float(102.0),
        ]),
        ZrdValue::List(vec![
            float(-8031.0),
            float(569.0),
            float(-3530.0),
            float(-144.0),
        ]),
        ZrdValue::List(vec![
            float(-3600.0),
            float(741.0),
            float(-3546.0),
            float(81.0),
        ]),
        ZrdValue::List(vec![
            float(-2758.0),
            float(544.0),
            float(-6619.0),
            float(88.0),
            text("pp5"),
        ]),
    ]
}

/// **M17's one-of-a-kind directive binds at its measured shape and is refused
/// when an argument the IR cannot carry sits inside it.**
///
/// `WARP_VEHICLE` is spelled by no other archive in the installation, so this
/// synthetic member is where its arms run in CI. The signature the registry
/// registers comes from the record's own measured shapes, so a truncated site
/// would simply measure a shorter shape and bind — the honest refusal arm is a
/// value the adapter refuses rather than a count: the same site with a
/// non-finite coordinate inside one point produces no `RawCall`, reports the
/// refusal by block and key, damages the block's condition and leaves
/// `MissionProgram::validate` with an error instead of a flyable program.
#[test]
fn accept_m17_b_a_warp_site_binds_at_its_measured_shape_and_refuses_a_value_the_ir_cannot_carry() {
    let well_formed = record_of(vec![(
        1,
        vec![text("WARP_VEHICLE"), ZrdValue::List(warp_arguments())],
    )]);
    let record = measure_control_record(&well_formed);
    assert!(
        matches!(
            record.key("WARP_VEHICLE").map(|key| key.disposition()),
            Some(DirectiveDisposition::Measured(_))
        ),
        "the measured warp key lost its disposition"
    );
    assert_eq!(
        record
            .key("WARP_VEHICLE")
            .and_then(|key| key.agreed_shape())
            .map(|shape| shape.label()),
        Some(
            "[text,[float,float,float,float],[float,float,float,float],[float,float,float,float],\
             [float,float,float,float,text]]"
                .to_owned()
        )
    );
    let lowered = lower(&well_formed);
    assert!(
        lowered
            .attempt()
            .calls
            .iter()
            .all(|call| matches!(call, CallOutcome::Bound)),
        "the measured warp site did not bind: {:?}",
        lowered.attempt().calls
    );
    assert!(lowered.program().is_some(), "no program stood");
    assert_eq!(
        lowered.attempt().validation.as_ref().map(Vec::len),
        Some(0),
        "the program did not validate: {:?}",
        lowered.attempt().validation
    );

    // The same key, one coordinate not a number: the adapter refuses the value
    // rather than coercing it — the site reports the refusal by block, key and
    // child, the block's own condition verdict is damaged to a refusal instead
    // of a predicate, and `MissionProgram::validate` refuses the program the
    // damaged condition stands in.
    let non_finite = {
        let mut arguments = warp_arguments();
        let mut point = match &arguments[1] {
            ZrdValue::List(point) => point.clone(),
            other => panic!("a warp point is a list, not {other:?}"),
        };
        point[0] = ZrdValue::Float(f32::NAN);
        arguments[1] = ZrdValue::List(point);
        record_of(vec![(
            1,
            vec![text("WARP_VEHICLE"), ZrdValue::List(arguments)],
        )])
    };
    let lowered = lower(&non_finite);
    let refusals: Vec<&String> = lowered
        .attempt()
        .calls
        .iter()
        .filter_map(|call| match call {
            CallOutcome::Refused(reason) => Some(reason),
            CallOutcome::Bound => None,
        })
        .collect();
    assert_eq!(
        refusals.len(),
        1,
        "the non-finite coordinate was not refused exactly once: {:?}",
        lowered.attempt().calls
    );
    assert!(
        refusals[0].contains("not finite"),
        "the refusal does not name the value it refused: {}",
        refusals[0]
    );
    assert!(
        !matches!(lowered.attempt().conditions[0], ConditionOutcome::Lowered),
        "the block's condition still lowered over a refused site: {:?}",
        lowered.attempt().conditions[0]
    );
    assert!(
        lowered
            .attempt()
            .validation
            .as_ref()
            .is_some_and(|errors| !errors.is_empty()),
        "MissionProgram::validate accepted a program whose condition was refused: {:?}",
        lowered.attempt().validation
    );
}

/// **The search-trigger evaluator needs the keys the retail record pairs, and
/// an unknown key refuses the whole record rather than being ignored.**
///
/// M17's four zone blocks always spell the threshold beside the evaluator, so
/// the pair is the shape production binds; the threshold alone still stands,
/// because the count key is itself measured. A key nobody measured refuses the
/// record outright — the contract's "no binding without a measured meaning",
/// carried into CI without original data.
#[test]
fn accept_m17_b_the_zone_evaluator_pairs_with_its_threshold_and_an_unknown_key_refuses() {
    let paired = record_of(vec![
        (
            1,
            vec![
                text("DANGER_ZONES_COMPLETION_COUNT"),
                ZrdValue::List(vec![int(1)]),
                text("DANGER_ZONES_COMPLETED"),
                ZrdValue::List(vec![text("dzpath7"), text("dzpath8")]),
            ],
        ),
        (
            2,
            vec![
                text("BEGIN_DORMANT"),
                ZrdValue::List(vec![float(-1.0)]),
                text("INSTANTWIN"),
            ],
        ),
    ]);
    let lowered = lower(&paired);
    assert_eq!(
        lowered.attempt().conditions,
        [ConditionOutcome::Lowered, ConditionOutcome::Lowered],
        "the paired zone evaluator did not lower: {:?}",
        lowered.attempt().conditions
    );
    assert!(
        lowered
            .attempt()
            .calls
            .iter()
            .all(|call| matches!(call, CallOutcome::Bound)),
        "a paired site did not bind: {:?}",
        lowered.attempt().calls
    );
    assert!(lowered.program().is_some());

    // …the threshold alone: the count key is measured on its own, so the
    // record still stands while no zone evaluator exists to pair with.
    let threshold_only = record_of(vec![(
        1,
        vec![
            text("DANGER_ZONES_COMPLETION_COUNT"),
            ZrdValue::List(vec![int(1)]),
            text("INSTANTWIN"),
        ],
    )]);
    assert!(
        lower(&threshold_only).program().is_some(),
        "a lone threshold key refuses the record"
    );

    // An unknown key refuses its own site by name and the record with it: no
    // program stands, the measured sites beside it still bind, and nothing is
    // silently dropped or replaced by a convenient operation.
    let with_unknown = record_of(vec![
        (
            1,
            vec![
                text("DANGER_ZONES_COMPLETION_COUNT"),
                ZrdValue::List(vec![int(1)]),
                text("NOT_A_MEASURED_KEY"),
                ZrdValue::List(vec![int(1)]),
            ],
        ),
        (
            2,
            vec![
                text("BEGIN_DORMANT"),
                ZrdValue::List(vec![float(-1.0)]),
                text("INSTANTWIN"),
            ],
        ),
    ]);
    let lowered = lower(&with_unknown);
    assert!(
        lowered.program().is_none(),
        "an unknown key produced a program"
    );
    let refusals: Vec<&String> = lowered
        .attempt()
        .calls
        .iter()
        .filter_map(|call| match call {
            CallOutcome::Refused(reason) => Some(reason),
            CallOutcome::Bound => None,
        })
        .collect();
    assert_eq!(
        refusals.len(),
        1,
        "the unknown key did not refuse exactly its own site: {:?}",
        lowered.attempt().calls
    );
    assert!(
        refusals[0].contains("NOT_A_MEASURED_KEY"),
        "the refusal does not name the unknown key: {}",
        refusals[0]
    );
    assert_eq!(
        lowered
            .attempt()
            .calls
            .iter()
            .filter(|call| matches!(call, CallOutcome::Bound))
            .count(),
        3,
        "the measured sites beside the unknown key did not bind: {:?}",
        lowered.attempt().calls
    );
    let record = measure_control_record(&with_unknown);
    assert_eq!(
        record.unmeasured().len(),
        1,
        "the unknown key is not reported as unmeasured: {:?}",
        record.unmeasured()
    );
}
