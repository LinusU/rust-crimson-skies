//! Acceptance stage M21-B: the mission-specific compatibility gaps of the
//! twenty-first mission (`missions/M21.md`, work order `M21-B`).
//!
//! M21-A bound M21's identities and left the mission program unmeasured. The
//! machinery that measures a control program (the `.zrd` reader, the census,
//! the directive dispositions and the record → `RawProgram` adapter) is shared
//! and was built for M01; this stage runs it over M21's own reader archive and
//! pins what is **different** at M21, together with the three regression
//! priorities the sheet names:
//!
//! * M21's control program is `objectives.zrd` with 63 numbered blocks and 237
//!   directive sites — the eighth of the archive's 12 members, and the only one
//!   that declares numbered blocks at all;
//! * its 28-key vocabulary is **fully disposed** — 26 measured keys and exactly
//!   two terminal outcomes (`INSTANTWIN`, block 19; `INSTANTLOSS`, block 57),
//!   none refused, no unclassified record key — so every gap below is a
//!   lowering gap or a world-side unknown, never an unmeasured key;
//! * the record **lowers completely**: 63 `RawObjective`s, all 63 conditions
//!   lowered, all 237 calls bound, `MissionProgram::validate` clean, and the
//!   census row for `zbd/c5/m01` is complete while the campaign gate stays
//!   shut;
//! * **moving guide** — the sheet's first priority resolves to one actor: the
//!   record spells six `TRAVELERS` sites, five with `player` as the subject and
//!   exactly one with a second actor, `autogyro_1`, measured against a fixed
//!   coordinate with `DELETE_ON_SUCCESS` — and `autogyro_1` is the only actor
//!   the control program itself sets in motion (`START_TAXI` at block 28, woken
//!   by the always-awake block 2) before measuring it. The one other
//!   non-static geometry is block 51's anchor, the moving `piratezep`;
//! * **structural destruction** — the six warehouse support beams
//!   (`MSG_TRGT_WH_SUPPORTBEAM` / `MSG_OBJ_DESTROY`, declared by M21's own
//!   `targets.zrd`), the `steinmann` freighter whose `steinmann_sink` animation
//!   is block 17's whole predicate, and the `piratezep gasbagN panels` chained
//!   lookups of block 63 — whose sixth name, `gasbag6`, **no** `.zrd` member of
//!   this archive declares (`zeppelins.zrd` spells `gasbag1…5` and repeats
//!   `gasbag5`);
//! * **optional route reward** — the always-awake block 37's
//!   `DANGER_ZONES_COMPLETION_COUNT 4` over `dzpath22…27`, which wakes the
//!   `SECONDARY` objective 5 (and its `music_secondaryobj_sg` marker 49) and
//!   kills block 6, beside the record's six paired zone gates that kill each
//!   other and nap the next one.
//!
//! A lowered program is **not** a played mission: no playthrough, difficulty,
//! media or presentation row is covered (that is M21-C, which requires
//! `human_play`), and the wrong-actor / wrong-session / repeated-event halves of
//! the sheet's priorities are runtime observations that stay unmeasured here.
//! The measured unknowns are written up in
//! `docs/findings/2026-10-10-m21-b-compatibility-gaps.md`.
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`; the two synthetic
//! tests run in CI.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_app::control_lowering::{LoweredControlRecord, lower_control_record};
use cs_app::mission_control::{
    ControlProgram, RetailControlCensus, read_control_member, survey_mission_control_programs,
};
use cs_app::mission_start::recover_retail_start_configuration;
use cs_app::world::triggers::survey_retail_trigger_volumes;
use cs_assets::install::{discover, sha256};
use cs_content::campaign_bindings::{
    MissionControlBinding, MissionLabel, SourceBinding, SourceContext,
};
use cs_content::mission_control::{
    CallOutcome, ConditionOutcome, DirectiveDisposition, DirectiveOperation, TerminalOutcome,
    measure_control_record,
};
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{ZrdValue, decode_zrd, objective_record, zrd_flat_fields};
use cs_content::world::{RetailTriggerVolumeSurvey, WorldId, ZoneDeclarationKey};
use cs_formats::script_raw::discover_container;
use cs_types::content::{ContentId, ContentKind, Resolved};
use cs_types::install::RelativePath;

use crate::common::load_inventory;

/// The one work order this stage regresses.
const WORK_ORDER: &str = "M21";
/// The census row label of the mission (F13-B's mission-scope rule).
const MISSION: &str = "zbd/c5/m01";
/// The reader archive the installation ships for M21 — the program span
/// `missions/bindings/M21.json` cites.
const CONTAINER: &str = "ZBD/C5/M01/zrdr.zbd";
/// SHA-256 of that whole archive, from production discovery.
const CONTAINER_SHA256: &str = "263c991468cf93abf532b508778d192402a099d96c8321e551af3c8b5bfa4874";
/// The archive's length in bytes — M21-A's own source span.
const CONTAINER_LENGTH: u64 = 39_369;
/// The member the measured rule chose.
const CONTROL_MEMBER: &str = "objectives.zrd";
/// The control member's first byte inside the archive.
const CONTROL_OFFSET: u64 = 14_111;
/// The control member's length in bytes.
const CONTROL_LENGTH: u64 = 15_956;
/// SHA-256 of the control member's own bytes.
const CONTROL_SHA256: &str = "15d979d806f23e9c4c472451b94975bd109f8223fe0aeee5f290671fa02db61c";
/// The detection-zone member the chapter survey decodes for this mission.
const DZONES_MEMBER: &str = "dzones.zrd";
/// Its first byte inside the archive (census row and survey span must agree).
const DZONES_OFFSET: u64 = 10_498;
/// Its length in bytes.
const DZONES_LENGTH: u64 = 826;
/// SHA-256 of the detection-zone member's own bytes.
const DZONES_SHA256: &str = "5518dda740e0873de910cdda0717f9838f331c7e8981f35459e68af9d5309da0";
/// The aircraft member `cs_app::mission_start` reads the player record from.
const AIRCRAFT_MEMBER: &str = "aiv.zrd";
/// The numbered blocks of the control member.
const BLOCKS: u32 = 63;
/// The directive sites of the control member.
const SITES: u32 = 237;
/// The distinct directive keys of the control member.
const KEYS: usize = 28;
/// The distinct text nodes the control document spells (whole-document walk)
/// which **no** `.zrd` member outside M21's control member declares, and which
/// are therefore recorded rather than resolved. Sorted the way the walk sorts
/// them.
const RECORD_ONLY_TEXTS: [&str; 12] = [
    "MSG_BRF_NYM1_OBJ1",
    "MSG_BRF_NYM1_OBJ2",
    "MSG_BRF_NYM1_OBJ3",
    "MSG_BRF_NYM1_OBJ4",
    "MSG_BRF_NYM1_OBJ5",
    "fbgun01",
    "fbgun02",
    "maagun0*",
    "w_win01",
    "w_win02",
    "w_win03",
    "w_win04",
];

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M21-B needs the retail capability; run this suite with \
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

/// The declared discovery title of `M21`, read from the committed inventory.
fn declared_title() -> String {
    load_inventory()
        .iter()
        .find(|(label, _)| label.as_str() == WORK_ORDER)
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M21 work order")
}

/// The production control-program binding, built once for the whole suite.
fn control_binding() -> &'static MissionControlBinding {
    static BINDING: OnceLock<MissionControlBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        let context = SourceContext::read(&game_dir())
            .expect("production source context reads the installation");
        context
            .control_program(
                MissionLabel::new(WORK_ORDER).expect("M21 is a valid label"),
                &declared_title(),
            )
            .expect("M21's control program binds through the measured rule")
    })
}

/// The M21 mission binding M21-A derives, built once — this stage consumes its
/// identities and adds no second evidence for the join itself.
fn mission_binding() -> &'static SourceBinding {
    static BINDING: OnceLock<SourceBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        let context = SourceContext::read(&game_dir())
            .expect("production source context reads the installation");
        context
            .bind(
                MissionLabel::new(WORK_ORDER).expect("M21 is a valid label"),
                &declared_title(),
            )
            .expect("M21 binds to the original data")
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
        let children = value.as_list().expect("every M21 block is a list");
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
        .expect("the rule finds M21's control member")
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
    let mut ordered: Vec<u32> = seen.into_iter().collect();
    ordered.sort_unstable();
    ordered
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
/// **outside** M21's control member — the declarations a directive's operand
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

/// Every distinct `.zrd` text declared **outside** M21's control member.
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

/// Every file of the installation whose **raw bytes** contain each `needle`,
/// read in one pass — the scan that backs "carried by no other file",
/// independent of any decoder.
fn raw_files_containing<'a>(needles: &[&'a str]) -> BTreeMap<&'a str, Vec<String>> {
    let found = discover(&game_dir()).expect("production discovery reads the installation");
    let mut hits: BTreeMap<&str, Vec<String>> =
        needles.iter().map(|needle| (*needle, Vec::new())).collect();
    for record in &found.manifest.files {
        let spelling = record.relative_spelling.as_str();
        let Ok(bytes) = std::fs::read(found.manifest.host_root.join(spelling)) else {
            continue;
        };
        for needle in needles {
            if bytes
                .windows(needle.len())
                .any(|window| window == needle.as_bytes())
            {
                hits.entry(needle).or_default().push(spelling.to_owned());
            }
        }
    }
    for files in hits.values_mut() {
        files.sort();
    }
    hits
}

/// The first number of a stored list, or `None` when it holds no number.
fn first_float(value: Option<&ZrdValue>) -> Option<f32> {
    value?.as_list()?.iter().find_map(|entry| match entry {
        ZrdValue::Float(number) => Some(*number),
        _ => None,
    })
}

/// The first integer of a stored list, or `None` when it holds none.
fn first_int(value: Option<&ZrdValue>) -> Option<u32> {
    value?.as_list()?.iter().find_map(|entry| match entry {
        ZrdValue::Int(number) => Some(*number),
        _ => None,
    })
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

/// One decoded `.zrd` member of M21's reader archive, read from the archive's
/// own bytes through production discovery.
fn member(name: &str) -> ZrdValue {
    let bytes = std::fs::read(game_dir().join(CONTAINER)).expect("the archive reads");
    let path =
        RelativePath::new(&CONTAINER.to_lowercase()).expect("the archive is a relative path");
    let discovery = discover_container(CONTAINER, &path, &bytes);
    for program in discovery.programs() {
        if program.locator().member() == Some(name) {
            return decode_zrd(program.bytes()).unwrap_or_else(|error| {
                panic!(
                    "{name} did not decode: {} at {}",
                    error.code(),
                    error.offset()
                )
            });
        }
    }
    panic!("M21's archive carries no member {name}")
}

/// The `[name, fields]` record names an `aiv.zrd` document declares, in stored
/// order — the same shape `cs_app::mission_start` reads.
fn aircraft_names(document: &ZrdValue) -> Vec<String> {
    document
        .as_list()
        .expect("aiv.zrd is a list")
        .iter()
        .filter_map(|entry| {
            let parts = entry.as_list()?;
            if parts.len() != 2 {
                return None;
            }
            Some(parts[0].as_text()?.to_owned())
        })
        .collect()
}

/// M21's one zeppelin record: `zeppelins.zrd` wraps its flat alternating field
/// list in two single-element lists, measured from the document's own shape.
fn zeppelin_record(document: &ZrdValue) -> &[ZrdValue] {
    let entries = document
        .as_list()
        .expect("zeppelins.zrd is a list of entries");
    assert_eq!(entries.len(), 1, "M21 declares exactly one zeppelin");
    let wrapped = entries[0].as_list().expect("the zeppelin entry is a list");
    assert_eq!(
        wrapped.len(),
        1,
        "the zeppelin entry wraps exactly one record"
    );
    wrapped[0]
        .as_list()
        .expect("the zeppelin record is a flat alternating field list")
}

/// The value stored under `key` in a record written as a list of `[key, value]`
/// (or bare `[key]`) pairs — the shape `targets.zrd` uses.
fn pair_field<'a>(fields: &'a [ZrdValue], key: &str) -> Option<&'a ZrdValue> {
    fields.iter().find_map(|entry| {
        let parts = entry.as_list()?;
        if parts.first()?.as_text()? != key {
            return None;
        }
        parts.get(1)
    })
}

/// The value stored under `key` in a flat alternating `[key, value, …]` list.
fn flat_field<'a>(fields: &'a [ZrdValue], key: &str) -> Option<&'a ZrdValue> {
    let mut index = 0;
    while index + 1 < fields.len() {
        if fields[index].as_text() == Some(key) {
            return Some(&fields[index + 1]);
        }
        index += 2;
    }
    None
}

// ---------------------------------------------------------------------------
// Retail: what M21's control program is
// ---------------------------------------------------------------------------

/// **The control program is the member that declares the numbered blocks.**
///
/// Of the 12 members of M21's reader archive exactly one declares numbered
/// `OBJECTIVE<N>` blocks: the eighth, `objectives.zrd`. The census's blocks and
/// sites equal an independent walk of the same document, the archive is the
/// program span M21-A bound, and both the production control binding and M21-A's
/// mission binding reach the same member, span and two digests that re-derive
/// from the archive's bytes. Size is not the rule in either direction here: the
/// longest *other* member (`aiv.zrd`, 10 498 bytes) declares no block, and the
/// control member is 1.5× its length.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m21_b_the_control_program_is_the_member_that_declares_the_blocks() {
    let row = census().row(MISSION).expect("M21 is in the census");
    assert_eq!(row.container, CONTAINER);
    assert_eq!(
        row.container_sha256, CONTAINER_SHA256,
        "the reader archive is the program M21-A bound"
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
    assert!(
        longer.is_empty(),
        "the rule is not 'the longest member': {longer:?} out-ranks the control member"
    );
    let aircraft = row
        .members
        .iter()
        .find(|member| member.name == AIRCRAFT_MEMBER)
        .expect("the archive ships its aircraft member");
    let longest_other = row
        .members
        .iter()
        .filter(|member| !member.is_control)
        .map(|member| member.len)
        .max()
        .expect("the archive has other members");
    assert_eq!(
        (aircraft.len, aircraft.objective_blocks),
        (longest_other, 0),
        "the longest other member is the aircraft table and it declares no block"
    );

    let record = row.record().expect("M21 has a measured control program");
    assert_eq!((record.blocks(), record.sites()), (BLOCKS, SITES));
    assert_eq!(record.keys().len(), KEYS);
    let ControlProgram::Measured {
        sha256: member_sha, ..
    } = &row.program
    else {
        panic!("the census reports M21's program as measured")
    };
    assert_eq!(member_sha, CONTROL_SHA256);

    let (document, member_row) = read_control_member(&game_dir(), MISSION)
        .expect("the rule finds M21's control member again");
    assert_eq!(member_row.name, CONTROL_MEMBER);
    assert_eq!(member_row.objective_blocks, BLOCKS);
    assert!(member_row.is_control);
    assert_eq!(
        (member_row.offset, member_row.len),
        (CONTROL_OFFSET, CONTROL_LENGTH)
    );

    let blocks = blocks_of(&document);
    let numbers: Vec<u32> = blocks.iter().map(|(number, _)| *number).collect();
    assert_eq!(
        numbers,
        (1..=BLOCKS).collect::<Vec<_>>(),
        "numbered 1..=63, no gaps"
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
        "M21 spells no record-level key outside the measured vocabulary"
    );

    // The production binding and the census must not disagree about the
    // mission, the program, the member or the record.
    let bound = control_binding();
    assert_eq!(bound.mission.as_str(), "mission/ch5-m01");
    assert_eq!(bound.program_id.as_str(), "script/c5-m01-zrdr");
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

    // …and both agree with the mission binding M21-A committed: one mission id,
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

/// **Every directive key M21 spells has exactly one disposition, and none is
/// refused.**
///
/// Two keys are terminal outcomes (`INSTANTWIN` at block 19, `INSTANTLOSS` at
/// block 57), the other 26 have a measured effect, no key is `Unmeasured`, and
/// the sites add up to the census's own total. The record-level vocabulary is
/// measured beside it: five fields, no record sound, no key outside the
/// vocabulary, and the measured operation set is pinned as a set so a new
/// mechanism cannot arrive silently.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m21_b_every_directive_m21_spells_has_a_disposition_and_none_is_refused() {
    let record = census()
        .row(MISSION)
        .expect("M21 is in the census")
        .record()
        .expect("M21 has a measured control program");

    let measured: BTreeMap<&str, u32> = record
        .keys()
        .iter()
        .map(|key| (key.key.as_str(), key.sites))
        .collect();
    let expected: BTreeMap<&str, u32> = [
        ("ADD_OBJECTIVE_TARGET", 2),
        ("ANIM_STATE", 2),
        ("BEGIN_DORMANT", 43),
        ("COMPLETED_SOUND_GROUP", 33),
        ("DANGER_ZONES_COMPLETED", 7),
        ("DANGER_ZONES_COMPLETION_COUNT", 1),
        ("DEDG", 3),
        ("IDENTITY", 6),
        ("INACTIVE1", 18),
        ("INACTIVE2", 6),
        ("INACTIVE3", 6),
        ("INACTIVE4", 6),
        ("INACTIVE5", 5),
        ("INACTIVE6", 5),
        ("INACTIVE_COMPLETION_COUNT", 6),
        ("INSTANTLOSS", 1),
        ("INSTANTWIN", 1),
        ("KILL_OBJECTIVE_WHEN_I_COMPLETE", 19),
        ("NAP_OBJECTIVE_WHEN_I_COMPLETE", 15),
        ("REMOVE_OBJECTIVE_TARGET", 9),
        ("START_TAXI", 1),
        ("TICK_DEPENDS_ON_OBJ", 3),
        ("TRAVELERS", 6),
        ("WAKEUP_ENEMIES", 7),
        ("WAKEUP_TURRETS", 1),
        ("WAKEUP_ZEP_TURRETS", 1),
        ("WAKE_ANIM", 1),
        ("WAKE_OBJECTIVE_WHEN_I_COMPLETE", 23),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        measured, expected,
        "M21's 28-key vocabulary or its site counts changed"
    );
    let total: u32 = measured.values().sum();
    assert_eq!(total, SITES, "the site counts do not add up");

    assert!(record.unmeasured().is_empty(), "a key was left unmeasured");
    assert!(
        record.refusals().is_empty(),
        "a block was refused: {:?}",
        record.refusals()
    );
    assert!(
        record.unclassified_record_keys().is_empty(),
        "a record-level key fell outside the vocabulary"
    );
    assert!(
        record.record_sounds().is_empty(),
        "M21 spells a record-level sound key: {:?}",
        record.record_sounds()
    );

    // Exactly two terminal outcomes, each on its own block, and the spelling
    // decides the outcome — not the block number.
    let terminals: BTreeMap<&str, TerminalOutcome> = record
        .implemented()
        .into_iter()
        .map(|(key, outcome)| (key.key.as_str(), outcome))
        .collect();
    assert_eq!(
        terminals,
        BTreeMap::from([
            ("INSTANTWIN", TerminalOutcome::Succeeded),
            ("INSTANTLOSS", TerminalOutcome::Failed),
        ])
    );
    for key in ["INSTANTWIN", "INSTANTLOSS"] {
        let measured_key = record.key(key).expect("the terminal key is measured");
        assert_eq!(
            (measured_key.blocks, measured_key.sites),
            (1, 1),
            "{key} is no longer spelled once"
        );
        assert!(
            matches!(
                measured_key.disposition(),
                DirectiveDisposition::TerminalOutcome { outcome }
                    if outcome == terminals[key]
            ),
            "{key} is no longer a terminal outcome for {}",
            terminals[key]
        );
    }

    // The record-level fields M21's own record carries, and their shapes as the
    // measurement reads them.
    assert_eq!(
        record
            .record_fields()
            .iter()
            .map(|(field, count)| (field.key(), *count))
            .collect::<Vec<_>>(),
        vec![
            ("MISSION_TIMER", 1),
            ("PLAYER_INIT", 1),
            ("RESTORE_ANIMS", 1),
            ("EXECUTE_ANIMS", 1),
            ("INVALIDATE_ANIMS", 1),
        ]
    );
    assert_eq!(
        record
            .record_field_shapes()
            .iter()
            .map(|(key, shape)| (key.as_str(), shape.label()))
            .collect::<Vec<_>>(),
        vec![
            ("MISSION_TIMER", "[float]".to_owned()),
            (
                "PLAYER_INIT",
                "[int,[float,float,float],[float,float,float],float,float]".to_owned()
            ),
            ("RESTORE_ANIMS", "[]".to_owned()),
            ("EXECUTE_ANIMS", "[]".to_owned()),
            ("INVALIDATE_ANIMS", "[]".to_owned()),
        ]
    );

    // The measured operation set: one variant per mechanism the findings cover.
    let operations: BTreeSet<DirectiveOperation> = record
        .measured()
        .into_iter()
        .map(|(key, measured)| {
            assert!(
                !measured.evidence.is_empty(),
                "{} is measured without a cited finding",
                key.key
            );
            measured.operation
        })
        .collect();
    assert_eq!(
        operations,
        BTreeSet::from([
            DirectiveOperation::AnimationStates,
            DirectiveOperation::CompletedSoundGroup,
            DirectiveOperation::DangerZoneFlags,
            DirectiveOperation::DangerZoneThreshold,
            DirectiveOperation::DependencyGate,
            DirectiveOperation::DormantStart,
            DirectiveOperation::EnemyGroupDepletion,
            DirectiveOperation::InactiveMembers,
            DirectiveOperation::InactiveThreshold,
            DirectiveOperation::KillObjectives,
            DirectiveOperation::NapObjective,
            DirectiveOperation::PresentationIdentity,
            DirectiveOperation::ReleaseTaxi,
            DirectiveOperation::SetTargetFlag {
                objective: true,
                set: false
            },
            DirectiveOperation::SetTargetFlag {
                objective: true,
                set: true
            },
            DirectiveOperation::Travelers,
            DirectiveOperation::WakeAnimation,
            DirectiveOperation::WakeEnemies,
            DirectiveOperation::WakeObjectives,
            DirectiveOperation::WakeTurrets,
            DirectiveOperation::WakeZeppelinTurrets,
        ]),
        "the measured operation set of M21's vocabulary changed"
    );
    assert_eq!(
        record.measured().len(),
        26,
        "26 of the 28 keys carry a measured effect"
    );
}

/// **The sheet's moving-guide priority: one actor, and only one, is both set in
/// motion by the control program and then measured against a position.**
///
/// The record spells six `TRAVELERS` sites. Five name `player` as the subject
/// and are the player's own approach to a fixed point or to a named object. The
/// sixth — block 20, awake from the mission's first tick — names `autogyro_1`
/// as the subject against the fixed coordinate `[-344, 250, -4347.5]`, radius
/// 1000, and deletes the subject on success; and `autogyro_1` is the only actor
/// the program itself starts moving (`WAKEUP_ENEMIES` and `START_TAXI` at block
/// 28, woken by always-awake block 2) and later retires (`REMOVE_OBJECTIVE_TARGET`
/// at block 58). The one other non-static geometry is block 51's **anchor**:
/// `piratezep`, whose `zeppelins.zrd` record declares `max_speed` 25. Both
/// readings of the sheet's label are pinned here; which one the firsthand guide
/// meant is not decidable from record data, so the label stays a research
/// label. The boundary halves — before, at, after the transition, and whether
/// the wrong actor, wrong session or repeated event can satisfy the site — are
/// runtime observations for M21-C.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m21_b_the_moving_guide_is_the_actor_the_program_starts_and_then_measures() {
    let document = control_document();
    let blocks = blocks_of(&document);

    let approaches = spelled(&blocks, "TRAVELERS");
    assert_eq!(approaches.len(), 6, "M21 spells six proximity sites");
    let mut subjects: BTreeSet<&str> = BTreeSet::new();
    let mut anchors: BTreeSet<&str> = BTreeSet::new();
    for args in &approaches {
        assert_eq!(
            args[1].as_text(),
            Some("APPROACHING"),
            "every site spells the measured polarity: {args:?}"
        );
        subjects.insert(args[0].as_text().expect("a text subject"));
        if let Some(anchor) = args[2].as_text() {
            anchors.insert(anchor);
        }
    }
    assert_eq!(
        subjects,
        BTreeSet::from(["autogyro_1", "player"]),
        "the subject set changed"
    );
    assert_eq!(
        anchors,
        BTreeSet::from(["dz1", "piratezep", "rfspt4"]),
        "the named anchors changed"
    );
    let player_sites = approaches
        .iter()
        .filter(|args| args[0].as_text() == Some("player"))
        .count();
    assert_eq!(player_sites, 5, "five sites measure the player itself");
    let guided: Vec<&Vec<ZrdValue>> = approaches
        .iter()
        .filter(|args| args[0].as_text() != Some("player"))
        .collect();
    assert_eq!(
        guided.len(),
        1,
        "more than one non-player subject: {guided:?}"
    );
    let guide = guided[0];
    assert_eq!(
        guide,
        &vec![
            ZrdValue::Text("autogyro_1".to_owned()),
            ZrdValue::Text("APPROACHING".to_owned()),
            ZrdValue::List(vec![
                ZrdValue::Float(-344.0),
                ZrdValue::Float(250.0),
                ZrdValue::Float(-4347.5),
            ]),
            ZrdValue::Float(1000.0),
            ZrdValue::Int(1),
            ZrdValue::Text("DELETE_ON_SUCCESS".to_owned()),
        ],
        "the moving-guide site changed"
    );
    assert_eq!(
        sites(&blocks, 20, "TRAVELERS").len(),
        1,
        "the guide site is not block 20's only directive of its kind"
    );
    assert!(
        !block(&blocks, 20)
            .iter()
            .any(|directive| directive.key == "BEGIN_DORMANT"),
        "OBJECTIVE20 is not awake from the start"
    );
    let delete_sites: Vec<u32> = blocks
        .iter()
        .filter(|(_, directives)| {
            directives.iter().any(|directive| {
                directive.key == "DELETE_ON_SUCCESS"
                    || arguments(directive)
                        .iter()
                        .any(|value| value.as_text() == Some("DELETE_ON_SUCCESS"))
            })
        })
        .map(|(number, _)| *number)
        .collect();
    assert_eq!(
        delete_sites,
        [20],
        "`DELETE_ON_SUCCESS` is the guide site's own token"
    );

    // The chain that puts the actor in motion and then retires it.
    assert_eq!(
        sites(&blocks, 28, "WAKEUP_ENEMIES"),
        [vec![ZrdValue::Text("autogyro_1".to_owned())]],
        "block 28 does not wake the guide"
    );
    assert_eq!(
        sites(&blocks, 28, "START_TAXI"),
        [vec![ZrdValue::Text("autogyro_1".to_owned())]],
        "block 28 does not release the guide's AI hold-off"
    );
    assert_eq!(
        sites(&blocks, 28, "WAKE_OBJECTIVE_WHEN_I_COMPLETE"),
        [vec![ZrdValue::Int(56)]]
    );
    let guide_target: Vec<Vec<ZrdValue>> = spelled(&blocks, "REMOVE_OBJECTIVE_TARGET")
        .into_iter()
        .filter(|args| {
            args.iter()
                .any(|value| value.as_text() == Some("autogyro_1"))
        })
        .collect();
    assert_eq!(
        guide_target,
        [vec![
            ZrdValue::Text("autogyro_1".to_owned()),
            ZrdValue::Text("dz1".to_owned()),
        ]],
        "the guide's target flag is retired exactly once"
    );

    // Block 2 wakes the guide's launch block: the always-awake half of the
    // chain, so nothing in this priority depends on an unstated waker.
    assert_eq!(
        sites(&blocks, 2, "WAKE_OBJECTIVE_WHEN_I_COMPLETE"),
        [vec![ZrdValue::Int(28), ZrdValue::Int(59)]],
        "block 2 no longer wakes the guide's launch"
    );
    assert!(
        !with(&blocks, "BEGIN_DORMANT").contains(&2),
        "block 2 does not start from the first tick"
    );

    // The actor is declared by this archive's own aircraft table and by nothing
    // else in the installation, so the operand cannot resolve to a different
    // mission's actor.
    let declarations = declarations_of("autogyro_1");
    assert_eq!(
        declarations,
        [(CONTAINER.to_owned(), AIRCRAFT_MEMBER.to_owned())],
        "the guide actor's declarations changed: {declarations:?}"
    );
    let names = aircraft_names(&member(AIRCRAFT_MEMBER));
    assert_eq!(
        names,
        [
            "player",
            "autogyro_1",
            "bhatwarhawk_5_1",
            "bhatwarhawk_5_2",
            "bhatwarhawk_5_3",
            "bhatbrigand_5_1",
            "bhatbrigand_5_2",
            "bhatbrigand_5_3",
            "patrolboat_1",
            "patrolboat_2",
            "t_truck_1",
            "t_truck_2",
            "t_truck_3",
            "t_truck_4",
        ],
        "the aircraft table's own records changed"
    );
    assert_eq!(
        names.iter().position(|name| name == "autogyro_1"),
        Some(1),
        "the guide is not the second record of the aircraft table"
    );

    // The other reading of the label: block 51's anchor is a *named* object,
    // and that object's own record declares it moving — so a reader who calls
    // "moving guide" the thing the player approaches rather than the thing the
    // program moves finds a different, also-measured, binding.
    assert_eq!(
        sites(&blocks, 51, "TRAVELERS"),
        [vec![
            ZrdValue::Text("player".to_owned()),
            ZrdValue::Text("APPROACHING".to_owned()),
            ZrdValue::Text("piratezep".to_owned()),
            ZrdValue::Float(1500.0),
            ZrdValue::Int(1),
        ]]
    );
    let zeppelin = member("zeppelins.zrd");
    let record = zeppelin_record(&zeppelin);
    assert_eq!(
        flat_field(record, "node")
            .and_then(|value| value.as_list())
            .and_then(|nodes| nodes.first())
            .and_then(ZrdValue::as_text),
        Some("piratezep")
    );
    let max_speed = first_float(flat_field(record, "max_speed"))
        .expect("the zeppelin record declares a maximum speed");
    assert!(
        max_speed > 0.0,
        "the anchor block 51 approaches is not a moving object: {max_speed}"
    );
}

/// **The sheet's structural-destruction priority: the six support beams, the
/// freighter and the zeppelin's panels — with one operand no member of this
/// archive declares.**
///
/// The record watches structure in three shapes: six `INACTIVE<n>` lookups over
/// the warehouse support beams at four different thresholds, the `steinmann`
/// freighter whose `steinmann_sink` animation is block 17's entire predicate,
/// and block 63's chained `piratezep gasbagN panels` lookups. The beams and the
/// freighter are declared by M21's **own** `targets.zrd` with the descriptions
/// and help labels the original stores; the panels chain ends at `gasbag6`,
/// which this archive's `zeppelins.zrd` does **not** spell (it spells
/// `gasbag1…5` and repeats `gasbag5`), so what the original resolves the sixth
/// lookup to stays unknown rather than being invented. The runtime halves —
/// whether destroying the wrong structure, or the same one twice, can satisfy a
/// block — are M21-C.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m21_b_the_structural_destruction_is_the_support_beams_the_freighter_and_the_zeppelin_panels()
 {
    let document = control_document();
    let blocks = blocks_of(&document);

    // The threshold pairs the record spells over the six beams.
    let thresholds: Vec<(u32, i64)> = with(&blocks, "INACTIVE_COMPLETION_COUNT")
        .into_iter()
        .map(|number| {
            let value = integers_of(&sites(&blocks, number, "INACTIVE_COMPLETION_COUNT")[0]);
            (number, value[0])
        })
        .collect();
    assert_eq!(
        thresholds,
        vec![(10, 1), (11, 3), (12, 5), (13, 6), (62, 1), (63, 3)],
        "M21's inactive-member thresholds changed"
    );
    let beams: Vec<&str> = ["rfspt4", "rfspt5", "rfspt6", "lfspt1", "lfspt2", "lfspt3"].to_vec();
    for number in [11, 12, 13, 62] {
        for (stage, beam) in beams.iter().enumerate() {
            let stage = stage + 1;
            assert_eq!(
                sites(&blocks, number, &format!("INACTIVE{stage}")),
                [vec![
                    ZrdValue::Text((*beam).to_owned()),
                    ZrdValue::Text("healthy".to_owned()),
                ]],
                "OBJECTIVE{number}'s INACTIVE{stage} lookup changed"
            );
        }
    }
    // Block 10 watches a different, undeclared quartet at threshold 1.
    for (stage, name) in (1..=4).zip(["w_win01", "w_win02", "w_win03", "w_win04"]) {
        assert_eq!(
            sites(&blocks, 10, &format!("INACTIVE{stage}")),
            [vec![
                ZrdValue::Text(name.to_owned()),
                ZrdValue::Text("healthy".to_owned()),
            ]],
            "OBJECTIVE10's INACTIVE{stage} lookup changed"
        );
    }
    assert_eq!(
        sites(&blocks, 10, "INACTIVE_COMPLETION_COUNT"),
        [vec![ZrdValue::Int(1)]]
    );

    // The freighter: block 17's whole predicate is the sink animation.
    assert_eq!(
        sites(&blocks, 17, "ANIM_STATE"),
        [vec![
            ZrdValue::Text("ANIM".to_owned()),
            ZrdValue::List(vec![
                ZrdValue::Text("NAME".to_owned()),
                ZrdValue::List(vec![ZrdValue::Text("steinmann_sink".to_owned())]),
                ZrdValue::Text("STATE".to_owned()),
                ZrdValue::List(vec![ZrdValue::Text("RUNNING".to_owned())]),
            ]),
        ]]
    );
    assert_eq!(
        sites(&blocks, 17, "REMOVE_OBJECTIVE_TARGET"),
        [vec![ZrdValue::Text("steinmann".to_owned())]]
    );
    assert_eq!(
        sites(&blocks, 17, "COMPLETED_SOUND_GROUP"),
        [vec![ZrdValue::Text("snd_MN1Sink".to_owned())]]
    );
    assert!(
        !with(&blocks, "BEGIN_DORMANT").contains(&17),
        "OBJECTIVE17 must start from the mission's first tick"
    );

    // The zeppelin's panels: six chained lookups, threshold 3.
    let panels: Vec<(String, String, String)> = (1..=6)
        .map(|stage| {
            let args = sites(&blocks, 63, &format!("INACTIVE{stage}"))[0].clone();
            let texts: Vec<String> = args
                .iter()
                .filter_map(ZrdValue::as_text)
                .map(str::to_owned)
                .collect();
            assert_eq!(texts.len(), 3, "a chained lookup spells three names");
            (texts[0].clone(), texts[1].clone(), texts[2].clone())
        })
        .collect();
    assert_eq!(
        panels,
        (1..=6)
            .map(|stage| (
                "piratezep".to_owned(),
                format!("gasbag{stage}"),
                "panels".to_owned()
            ))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        sites(&blocks, 63, "INACTIVE_COMPLETION_COUNT"),
        [vec![ZrdValue::Int(3)]]
    );

    // …and this archive's own zeppelin record declares a different set: five
    // gasbags with the fifth listed twice, no sixth. Recorded, not worked
    // around.
    let zeppelin = member("zeppelins.zrd");
    let record = zeppelin_record(&zeppelin);
    let healthy = flat_field(record, "healthy")
        .and_then(ZrdValue::as_list)
        .expect("the zeppelin record declares its healthy parts")
        .iter()
        .map(|entry| {
            let parts = entry.as_list().expect("a healthy part is a list");
            (
                parts[0].as_text().expect("a part name").to_owned(),
                parts[1].as_text().expect("a part group").to_owned(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        healthy,
        [
            ("gasbag1", "panels"),
            ("gasbag2", "panels"),
            ("gasbag3", "panels"),
            ("gasbag4", "panels"),
            ("gasbag5", "panels"),
            ("gasbag5", "panels"),
        ]
        .map(|(name, group)| (name.to_owned(), group.to_owned()))
    );
    assert!(
        !healthy.iter().any(|(name, _)| name == "gasbag6"),
        "the record spells gasbag6 and the zeppelin record declares it after all"
    );
    assert!(
        !declarations_of("gasbag6")
            .iter()
            .any(|(container, _)| container.eq_ignore_ascii_case(CONTAINER)),
        "gasbag6 is declared by this archive after all"
    );
    assert!(
        declarations_of("gasbag1")
            .iter()
            .any(|(container, member)| {
                container.eq_ignore_ascii_case(CONTAINER) && member == "zeppelins.zrd"
            }),
        "gasbag1 is not declared by this archive's zeppelin member"
    );
    let required = first_int(flat_field(record, "num_healthy_required"))
        .expect("the zeppelin record declares how many parts it needs");
    assert_eq!(
        required, 4,
        "the zeppelin's own part threshold changed; block 63's is 3"
    );

    // The targets the record retires are declared by M21's own `targets.zrd`
    // with the original's own descriptions — the real bindings behind the
    // label, rather than the label itself.
    let targets = member("targets.zrd");
    let wanted = [
        ("rfspt4", "MSG_TRGT_WH_SUPPORTBEAM", "MSG_OBJ_DESTROY"),
        ("rfspt5", "MSG_TRGT_WH_SUPPORTBEAM", "MSG_OBJ_DESTROY"),
        ("rfspt6", "MSG_TRGT_WH_SUPPORTBEAM", "MSG_OBJ_DESTROY"),
        ("lfspt1", "MSG_TRGT_WH_SUPPORTBEAM", "MSG_OBJ_DESTROY"),
        ("lfspt2", "MSG_TRGT_WH_SUPPORTBEAM", "MSG_OBJ_DESTROY"),
        ("lfspt3", "MSG_TRGT_WH_SUPPORTBEAM", "MSG_OBJ_DESTROY"),
        ("steinmann", "MSG_TRGT_STEINMANN", "MSG_OBJ_DESTROY"),
        ("dz1", "MSG_TRGT_PHQ", "MSG_OBJ_APPROACH"),
        ("piratezep", "MSG_OBJ_KLONDIKE", ""),
        ("pzhookpoint", "MSG_OBJ_KLONDIKEHOOK", "MSG_OBJ_DOCK"),
    ];
    let records: Vec<&[ZrdValue]> = targets
        .as_list()
        .expect("targets.zrd is a list")
        .iter()
        .filter_map(ZrdValue::as_list)
        .collect();
    assert_eq!(records.len(), 10, "M21's target table declares ten entries");
    for (node, description, help) in wanted {
        let record = records
            .iter()
            .find(|record| {
                pair_field(record, "nodes").is_some_and(|nodes| {
                    nodes.as_list().is_some_and(|entries| {
                        entries.iter().any(|entry| {
                            entry.as_text().map(|text| text == node).unwrap_or_else(|| {
                                entry
                                    .as_list()
                                    .and_then(|parts| parts.first())
                                    .and_then(ZrdValue::as_text)
                                    == Some(node)
                            })
                        })
                    })
                })
            })
            .unwrap_or_else(|| panic!("targets.zrd declares no entry for {node}"));
        assert_eq!(
            pair_field(record, "description").and_then(ZrdValue::as_text),
            Some(description),
            "{node}'s description changed"
        );
        let help_label = pair_field(record, "help_label").and_then(ZrdValue::as_text);
        if help.is_empty() {
            assert_eq!(
                help_label, None,
                "{node} carries a help label it did not have"
            );
        } else {
            assert_eq!(help_label, Some(help), "{node}'s help label changed");
        }
    }
    // The freighter is the one target the original categorises as a freighter.
    let freighter = records
        .iter()
        .find(|record| {
            pair_field(record, "category_label").and_then(ZrdValue::as_text)
                == Some("MSG_OBJ_FREIGHTER")
        })
        .expect("exactly one target carries the freighter category");
    assert_eq!(
        pair_field(freighter, "description").and_then(ZrdValue::as_text),
        Some("MSG_TRGT_STEINMANN")
    );

    // `dz1` is the one target the original marks with the bare `objective`
    // flag, and the four `w_win0N` operands are declared nowhere else: two
    // raw-byte scans, independent of any decoder.
    let dz1 = records
        .iter()
        .find(|record| {
            pair_field(record, "nodes").is_some_and(|nodes| {
                nodes.as_list().is_some_and(|entries| {
                    entries.iter().any(|entry| entry.as_text() == Some("dz1"))
                })
            })
        })
        .expect("targets.zrd declares dz1");
    assert!(
        dz1.iter().any(|pair| {
            pair.as_list()
                .is_some_and(|parts| parts.len() == 1 && parts[0].as_text() == Some("objective"))
        }),
        "dz1 is not marked with the bare `objective` flag"
    );
    for name in ["w_win01", "w_win02", "w_win03", "w_win04"] {
        assert!(
            declarations_of(name).is_empty(),
            "{name} is declared by a `.zrd` member somewhere: {:?}",
            declarations_of(name)
        );
    }
}

/// **The sheet's optional-route priority: the always-awake danger-zone
/// threshold, its SECONDARY objective, and the record's paired zone gates —
/// every zone name resolved through the production trigger-volume survey.**
///
/// Block 37 evaluates `DANGER_ZONES_COMPLETION_COUNT 4` over the six zones
/// `dzpath22…27` from the mission's first tick, wakes the `SECONDARY` objective
/// 5 (which carries `MSG_BRF_NYM1_OBJ2` and wakes the `music_secondaryobj_sg`
/// marker 49) and kills its mirror 6. Beside it, five zone gates and block 3's
/// own gate pair a "yes" block with a "no" block that kill each other and nap
/// the next gate. All six zone names are declared by **this archive's** own
/// `dzones.zrd` — under the `nosnapshot` key only, never under an
/// `objective_numbers` pair — and the production survey reports no declaration
/// gap against the chapter-5 container's 34 zone nodes. What any of it *means*
/// at runtime, and whether a wrong session or a repeated crossing can satisfy a
/// gate, is M21-C.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m21_b_the_optional_route_is_the_danger_zone_threshold_and_its_paired_gates() {
    let document = control_document();
    let blocks = blocks_of(&document);

    assert_eq!(
        sites(&blocks, 37, "DANGER_ZONES_COMPLETION_COUNT"),
        [vec![ZrdValue::Int(4)]],
        "the route threshold changed"
    );
    assert_eq!(
        sites(&blocks, 37, "DANGER_ZONES_COMPLETED"),
        [vec![
            ZrdValue::Text("dzpath22".to_owned()),
            ZrdValue::Text("dzpath23".to_owned()),
            ZrdValue::Text("dzpath24".to_owned()),
            ZrdValue::Text("dzpath25".to_owned()),
            ZrdValue::Text("dzpath26".to_owned()),
            ZrdValue::Text("dzpath27".to_owned()),
        ]]
    );
    assert_eq!(
        sites(&blocks, 37, "WAKE_OBJECTIVE_WHEN_I_COMPLETE"),
        [vec![ZrdValue::Int(5)]]
    );
    assert_eq!(
        sites(&blocks, 37, "KILL_OBJECTIVE_WHEN_I_COMPLETE"),
        [vec![ZrdValue::Int(6)]]
    );
    assert!(
        !with(&blocks, "BEGIN_DORMANT").contains(&37),
        "OBJECTIVE37 must evaluate from the mission's first tick"
    );

    // The reward branch it wakes: a SECONDARY objective with its own briefing
    // id, and the secondary-music marker behind it.
    assert_eq!(
        sites(&blocks, 5, "IDENTITY"),
        [vec![
            ZrdValue::Text("SECONDARY".to_owned()),
            ZrdValue::Int(2),
            ZrdValue::Text("MSG_BRF_NYM1_OBJ2".to_owned()),
        ]]
    );
    assert_eq!(
        sites(&blocks, 5, "WAKE_OBJECTIVE_WHEN_I_COMPLETE"),
        [vec![
            ZrdValue::Int(49),
            ZrdValue::Int(58),
            ZrdValue::Int(60)
        ]]
    );
    assert_eq!(
        sites(&blocks, 49, "COMPLETED_SOUND_GROUP"),
        [vec![ZrdValue::Text("music_secondaryobj_sg".to_owned())]]
    );
    assert!(
        with(&blocks, "BEGIN_DORMANT").contains(&49),
        "the secondary-music marker is not dormant behind block 5"
    );
    // The mirror branch it kills, with the identical proximity site.
    assert_eq!(
        sites(&blocks, 6, "TRAVELERS"),
        sites(&blocks, 5, "TRAVELERS"),
        "the two mirror branches no longer share their proximity site"
    );
    assert_eq!(
        sites(&blocks, 6, "KILL_OBJECTIVE_WHEN_I_COMPLETE"),
        [vec![
            ZrdValue::Int(2),
            ZrdValue::Int(3),
            ZrdValue::Int(4),
            ZrdValue::Int(5),
            ZrdValue::Int(38),
            ZrdValue::Int(39),
            ZrdValue::Int(37),
            ZrdValue::Int(56),
            ZrdValue::Int(57),
        ]]
    );

    // The paired gates: each "yes" block names one zone, kills its "no" mirror
    // and naps the next gate; the mirrors kill back.
    let pairs: [(u32, u32, &str, &str, u32, f32); 5] = [
        (38, 39, "dzpath23", "snd_MN1DZYes2", 41, 34.0),
        (40, 41, "dzpath24", "snd_MN1DZYes3", 43, 12.0),
        (42, 43, "dzpath25", "snd_c5-MN-m1_Cabbie_84", 45, 18.0),
        (44, 45, "dzpath26", "snd_MN1DZYes2", 47, 41.0),
        (46, 47, "dzpath27", "snd_MN1DZYes3", 0, 0.0),
    ];
    for (yes, no, zone, sound, nap_target, delay) in pairs {
        assert_eq!(
            sites(&blocks, yes, "DANGER_ZONES_COMPLETED"),
            [vec![ZrdValue::Text(zone.to_owned())]],
            "OBJECTIVE{yes} no longer gates {zone}"
        );
        assert_eq!(
            sites(&blocks, yes, "COMPLETED_SOUND_GROUP"),
            [vec![ZrdValue::Text(sound.to_owned())]],
            "OBJECTIVE{yes}'s completion sound changed"
        );
        assert_eq!(
            sites(&blocks, yes, "KILL_OBJECTIVE_WHEN_I_COMPLETE"),
            [vec![ZrdValue::Int(no)]],
            "OBJECTIVE{yes} does not kill its mirror"
        );
        assert_eq!(
            sites(&blocks, no, "KILL_OBJECTIVE_WHEN_I_COMPLETE"),
            [vec![ZrdValue::Int(yes)]],
            "OBJECTIVE{no} does not kill its mirror back"
        );
        if nap_target == 0 {
            assert!(
                sites(&blocks, yes, "NAP_OBJECTIVE_WHEN_I_COMPLETE").is_empty(),
                "OBJECTIVE{yes} naps a gate it did not nap before"
            );
        } else {
            assert_eq!(
                sites(&blocks, yes, "NAP_OBJECTIVE_WHEN_I_COMPLETE"),
                [vec![ZrdValue::Int(nap_target), ZrdValue::Float(delay)]],
                "OBJECTIVE{yes}'s nap changed"
            );
        }
        assert!(
            with(&blocks, "BEGIN_DORMANT").contains(&no),
            "OBJECTIVE{no} is not dormant"
        );
    }
    // The sixth zone's own gate pair (blocks 3 and 4) spells the first zone.
    assert_eq!(
        sites(&blocks, 3, "DANGER_ZONES_COMPLETED"),
        [vec![ZrdValue::Text("dzpath22".to_owned())]]
    );
    assert_eq!(
        sites(&blocks, 3, "KILL_OBJECTIVE_WHEN_I_COMPLETE"),
        [vec![ZrdValue::Int(4)]]
    );
    assert_eq!(
        sites(&blocks, 4, "KILL_OBJECTIVE_WHEN_I_COMPLETE"),
        [vec![ZrdValue::Int(3)]]
    );

    // Every zone the record spells resolves through the production survey.
    let survey = zone_survey();
    assert!(survey.zone_declarations_are_decoded());
    let declaration = survey
        .declarations()
        .iter()
        .find(|declaration| declaration.mission() == MISSION)
        .expect("M21's dzones.zrd decoded into a declaration");
    assert_eq!(declaration.member_container(), "zbd/c5/m01/zrdr.zbd");
    assert_eq!(declaration.member_container_sha256(), CONTAINER_SHA256);
    assert_eq!(
        declaration.member_span(),
        (DZONES_OFFSET, DZONES_LENGTH),
        "the survey's member span is not the census's dzones.zrd span"
    );
    assert_eq!(
        declaration.keys(),
        &[
            ZoneDeclarationKey::NoSnapshot,
            ZoneDeclarationKey::ObjectiveNumbers
        ],
        "the declaration's stored key order changed"
    );
    assert!(
        declaration.disable().is_empty(),
        "M21 declares a disabled zone: {:?}",
        declaration.disable()
    );
    let no_snapshot: BTreeSet<&str> = declaration
        .no_snapshot()
        .iter()
        .map(String::as_str)
        .collect();
    let numbers: BTreeSet<(&str, u32)> = declaration
        .objective_numbers()
        .iter()
        .map(|(name, number)| (name.as_str(), *number))
        .collect();
    assert_eq!(no_snapshot.len(), 20, "the no-snapshot set changed");
    assert_eq!(numbers.len(), 14, "the objective-number set changed");
    assert!(
        no_snapshot
            .iter()
            .all(|name| !numbers.iter().any(|(other, _)| other == name)),
        "the two declared sets overlap"
    );
    assert_eq!(
        declaration.named_zones().len(),
        no_snapshot.len() + numbers.len(),
        "the two declared sets are not the whole declaration"
    );

    let c5 = WorldId::from_key("c5").expect("c5 is a world key");
    let nodes: BTreeSet<String> = survey
        .volumes_in(&c5)
        .iter()
        .map(|volume| volume.zone().to_owned())
        .collect();
    assert_eq!(
        nodes.len(),
        34,
        "the chapter-5 container's numbered zones changed"
    );
    assert_eq!(
        declaration.named_zones().len(),
        nodes.len(),
        "the declaration does not account for every node"
    );
    for name in &no_snapshot {
        assert!(nodes.contains(*name), "{name} has no node in c5");
    }
    for (name, _) in &numbers {
        assert!(nodes.contains(*name), "{name} has no node in c5");
    }

    let spelled_zones: BTreeSet<&str> = blocks
        .iter()
        .flat_map(|(_, directives)| directives.iter())
        .filter(|directive| directive.key == "DANGER_ZONES_COMPLETED")
        .flat_map(|directive| texts(directive))
        .collect();
    assert_eq!(
        spelled_zones,
        BTreeSet::from([
            "dzpath22", "dzpath23", "dzpath24", "dzpath25", "dzpath26", "dzpath27"
        ]),
        "the zones the route blocks spell changed"
    );
    for zone in &spelled_zones {
        assert!(nodes.contains(*zone), "{zone} has no node in the chapter");
        assert!(
            no_snapshot.contains(zone),
            "{zone} is not declared by M21's own detection-zone member"
        );
        assert!(
            !numbers.iter().any(|(name, _)| name == zone),
            "{zone} carries an objective-number pair, which it did not have"
        );
        assert!(
            declarations_of(zone).iter().any(|(container, member)| {
                container.eq_ignore_ascii_case(CONTAINER) && member == DZONES_MEMBER
            }),
            "{zone} is not declared by M21's own detection-zone member"
        );
    }
    let gaps: Vec<_> = survey
        .declaration_gaps()
        .into_iter()
        .filter(|gap| gap.mission == MISSION)
        .collect();
    assert!(
        gaps.is_empty(),
        "M21 names a zone its world container has no node for: {gaps:?}"
    );
}

/// **Every text the control record spells is declared somewhere outside it, or
/// recorded by name — and the raw scan splits the recorded set three ways.**
///
/// The record spells 186 distinct texts. 174 of them are declared by some
/// `.zrd` member outside M21's control member. The remaining twelve are
/// recorded exactly, then read again as raw bytes over the whole installation
/// (one pass, no decoder), which is what the `.zrd` index alone cannot say:
/// three operands are carried by M21's archive and by nothing else, five
/// briefing ids are also carried by the installation's `strings.dll`, and four
/// operands are also carried by the chapter-5 world containers
/// `ZBD/C5/cam_anim.zbd` and `ZBD/C5/gamez.zbd`. A counter-example keeps the
/// scan honest: a name the installation carries in more than a hundred members
/// is not reported as absent.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m21_b_every_text_the_record_spells_is_declared_outside_it_or_recorded_as_a_gap() {
    let document = control_document();
    let mut walked = Vec::new();
    walk_texts(&document, &mut walked);
    let distinct: BTreeSet<String> = walked.into_iter().collect();
    assert_eq!(distinct.len(), 186, "the whole-document text walk changed");

    let outside = texts_declared_outside_control();
    let only: Vec<String> = distinct
        .iter()
        .filter(|text| !outside.contains(*text))
        .cloned()
        .collect();
    assert_eq!(
        only,
        RECORD_ONLY_TEXTS
            .iter()
            .map(|text| (*text).to_owned())
            .collect::<Vec<_>>(),
        "the record-only set changed"
    );
    assert_eq!(distinct.len() - only.len(), 174, "the declared count moved");
    for text in &only {
        let hits = text_index().get(text).cloned().unwrap_or_default();
        assert_eq!(
            hits,
            BTreeSet::from([(CONTAINER.to_owned(), CONTROL_MEMBER.to_owned())]),
            "{text} is declared somewhere after all: {hits:?}"
        );
    }

    // A raw-byte scan over the whole installation, in one pass, then splits the
    // twelve into what the `.zrd` index alone could not tell apart: three names
    // no other file carries, five briefing ids the string table also carries,
    // and four operands the chapter-5 world containers also carry.
    let raw = raw_files_containing(&RECORD_ONLY_TEXTS);
    for name in ["fbgun01", "fbgun02", "maagun0*"] {
        assert_eq!(
            raw[name],
            [CONTAINER.to_owned()],
            "{name} is carried by another file after all: {:?}",
            raw[name]
        );
    }
    for number in 1..=5 {
        let name = format!("MSG_BRF_NYM1_OBJ{number}");
        assert_eq!(
            raw[name.as_str()],
            [CONTAINER.to_owned(), "strings.dll".to_owned()],
            "{name} is not carried by the installation's string table exactly: {:?}",
            raw[name.as_str()]
        );
    }
    for number in 1..=4 {
        let name = format!("w_win0{number}");
        assert_eq!(
            raw[name.as_str()],
            [
                CONTAINER.to_owned(),
                "ZBD/C5/cam_anim.zbd".to_owned(),
                "ZBD/C5/gamez.zbd".to_owned(),
            ],
            "{name} is not carried by the chapter-5 world containers exactly: {:?}",
            raw[name.as_str()]
        );
    }

    // The counter-example: a name the installation declares everywhere, so the
    // scan distinguishes a declared name from an undeclared one rather than
    // always answering "absent".
    let pirate = text_index().get("piratezep").cloned().unwrap_or_default();
    assert!(
        pirate.len() >= 100,
        "the counter-example name is not carried widely: {} declarations",
        pirate.len()
    );
    assert!(
        declarations_of("piratezep").len() >= 100,
        "the counter-example lost its declarations"
    );

    // Two softer classes, recorded so they are not mistaken for record-only
    // ones: names declared only by *other* missions, and the beams the target
    // table of this archive declares (so `targets.zrd` and the control member
    // agree while no other archive names them).
    for name in ["pzhomebase", "hooked_to_klondike"] {
        let hits = declarations_of(name);
        assert!(
            !hits.iter().any(
                |(container, member)| container.eq_ignore_ascii_case(CONTAINER)
                    && member == CONTROL_MEMBER
            ),
            "{name} is not the class it was recorded as"
        );
        assert!(
            hits.iter()
                .all(|(container, _)| !container.eq_ignore_ascii_case(CONTAINER)),
            "{name} is declared inside M21's archive after all"
        );
        assert!(!hits.is_empty(), "{name} is declared nowhere at all");
    }
    for name in ["rfspt4", "lfspt1", "steinmann", "spprt"] {
        assert!(
            declarations_of(name).iter().any(|(container, member)| {
                container.eq_ignore_ascii_case(CONTAINER) && member == "targets.zrd"
            }),
            "{name} is not declared by M21's own target table"
        );
    }
    // `dz1` is declared by this archive's own target table *and* by five
    // instant-action scenarios' tables — a cross-archive name, not a gap.
    let dz1 = declarations_of("dz1");
    assert!(
        dz1.iter().any(
            |(container, member)| container.eq_ignore_ascii_case(CONTAINER)
                && member == "targets.zrd"
        ),
        "dz1 is not declared by M21's own target table"
    );
    assert!(
        dz1.len() > 10,
        "dz1 is not the cross-archive name it was recorded as: {dz1:?}"
    );
}

/// **Both terminal latches are gated, both prerequisite closures are measured,
/// every address M21 spells is in range — and the record's own dead branches
/// are counted rather than assumed away.**
///
/// `INSTANTWIN` sits on block 19 behind a ten-block prerequisite closure and a
/// `hooked_to_klondike` animation; `INSTANTLOSS` sits on block 57 behind a
/// six-block one. Kills are not edges: block 4 is killed by block 3 and nothing
/// wakes it, so the record spells two dormant blocks nothing can enter. The
/// wrong-session, wrong-actor and repeated-event halves of the sheet's row are
/// runtime observations (M21-C); what is measured here is the static gate.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m21_b_the_two_terminal_latches_are_gated_and_every_address_is_in_range() {
    let document = control_document();
    let blocks = blocks_of(&document);

    let dormant = with(&blocks, "BEGIN_DORMANT");
    assert_eq!(dormant.len(), 43, "the dormant set changed");
    let awake: Vec<u32> = (1..=BLOCKS)
        .filter(|number| !dormant.contains(number))
        .collect();
    assert_eq!(
        awake,
        [
            2, 3, 6, 7, 8, 9, 17, 20, 29, 31, 33, 35, 37, 38, 40, 42, 44, 46, 62, 63
        ],
        "the always-awake set changed"
    );
    // One block arms a clock; the rest of the dormant set is woken by edges.
    let timed: Vec<(u32, f32)> = dormant
        .iter()
        .map(|number| {
            let directive = block(&blocks, *number)
                .iter()
                .find(|directive| directive.key == "BEGIN_DORMANT")
                .expect("the block spells the marker");
            match arguments(directive).first() {
                Some(ZrdValue::Float(wake)) => (*number, *wake),
                other => panic!("OBJECTIVE{number} spells a float wake, not {other:?}"),
            }
        })
        .filter(|(_, wake)| *wake >= 0.0)
        .collect();
    assert_eq!(timed, [(1, 2.0)], "the record's timed self-wakes changed");

    // The terminal sites, each exactly once and bare.
    assert_eq!(sites(&blocks, 19, "INSTANTWIN"), [Vec::<ZrdValue>::new()]);
    assert_eq!(sites(&blocks, 57, "INSTANTLOSS"), [Vec::<ZrdValue>::new()]);
    assert_eq!(with(&blocks, "INSTANTWIN"), [19], "the success latch moved");
    assert_eq!(
        with(&blocks, "INSTANTLOSS"),
        [57],
        "the failure latch moved"
    );
    assert!(
        sites(&blocks, 19, "WAKE_ANIM") == [vec![ZrdValue::Text("pzhomebase".to_owned())]],
        "the success latch's wake animation changed"
    );
    assert_eq!(
        sites(&blocks, 19, "ANIM_STATE"),
        [vec![
            ZrdValue::Text("ANIM".to_owned()),
            ZrdValue::List(vec![
                ZrdValue::Text("NAME".to_owned()),
                ZrdValue::List(vec![ZrdValue::Text("hooked_to_klondike".to_owned())]),
                ZrdValue::Text("STATE".to_owned()),
                ZrdValue::List(vec![ZrdValue::Text("EXECUTED".to_owned())]),
            ]),
        ]]
    );

    // Every spelled address resolves inside the record under M02-B-FU3's
    // measured one-based convention, counted per key.
    let mut per_key: BTreeMap<&str, usize> = BTreeMap::new();
    let mut total = 0usize;
    for (_, directives) in &blocks {
        for directive in directives {
            if !matches!(
                directive.key.as_str(),
                "WAKE_OBJECTIVE_WHEN_I_COMPLETE"
                    | "KILL_OBJECTIVE_WHEN_I_COMPLETE"
                    | "NAP_OBJECTIVE_WHEN_I_COMPLETE"
                    | "TICK_DEPENDS_ON_OBJ"
            ) {
                continue;
            }
            let targets = addresses(directive);
            for target in &targets {
                assert!(
                    (1..=i64::from(BLOCKS)).contains(target),
                    "{} spelled out-of-range address {target}",
                    directive.key
                );
            }
            *per_key.entry(directive.key.as_str()).or_default() += targets.len();
            total += targets.len();
        }
    }
    assert_eq!(
        per_key,
        BTreeMap::from([
            ("WAKE_OBJECTIVE_WHEN_I_COMPLETE", 39),
            ("KILL_OBJECTIVE_WHEN_I_COMPLETE", 60),
            ("NAP_OBJECTIVE_WHEN_I_COMPLETE", 15),
            ("TICK_DEPENDS_ON_OBJ", 3),
        ]),
        "the address budget changed"
    );
    assert_eq!(total, 117, "the total address budget changed");
    assert_eq!(
        (23, 19, 15, 3),
        (
            with(&blocks, "WAKE_OBJECTIVE_WHEN_I_COMPLETE").len(),
            with(&blocks, "KILL_OBJECTIVE_WHEN_I_COMPLETE").len(),
            with(&blocks, "NAP_OBJECTIVE_WHEN_I_COMPLETE").len(),
            with(&blocks, "TICK_DEPENDS_ON_OBJ").len(),
        ),
        "the per-key site counts changed"
    );

    // The gate sites: which block each dependency names, and that the gated
    // blocks are exactly the three the record spells.
    let mut gates: BTreeMap<u32, u32> = BTreeMap::new();
    for (number, directives) in &blocks {
        for directive in directives {
            if directive.key == "TICK_DEPENDS_ON_OBJ" {
                gates.insert(*number, addresses(directive)[0] as u32);
            }
        }
    }
    assert_eq!(
        gates,
        BTreeMap::from([(16, 27), (60, 59), (61, 59)]),
        "the dependency gates changed"
    );

    // The prerequisite graph: kills are not edges, and the two latches' own
    // closures are exactly the blocks that must be entered before them.
    let map = prerequisites(&blocks);
    assert_eq!(
        map.get(&19),
        Some(&vec![18]),
        "the success latch's own predecessor changed"
    );
    assert_eq!(
        map.get(&57),
        Some(&vec![56, 63]),
        "the failure latch's own predecessors changed"
    );
    assert_eq!(
        closure(&map, 19),
        [7, 10, 11, 12, 13, 16, 17, 18, 19, 27],
        "the success prerequisite closure changed"
    );
    assert_eq!(
        closure(&map, 57),
        [1, 2, 28, 56, 57, 63],
        "the failure prerequisite closure changed"
    );
    assert!(
        !map.contains_key(&6),
        "block 6 is killed by block 37 and entered by none, so it has no predecessor"
    );
    // A kill is not an entry either: block 3 kills block 4, and block 4 is
    // entered only by block 2's 44-second nap.
    assert_eq!(
        map.get(&4),
        Some(&vec![2]),
        "block 4's only entry is not block 2's nap any more"
    );
    assert_eq!(
        map.get(&58),
        Some(&vec![5, 6, 56, 63]),
        "the block every late branch wakes changed"
    );

    // The record's own dead branch: the one dormant block nothing enters and no
    // clock arms. Counted, not assumed away.
    let mut unreachable: Vec<u32> = Vec::new();
    for number in &dormant {
        if map.contains_key(number) {
            continue;
        }
        let wake = arguments(
            block(&blocks, *number)
                .iter()
                .find(|directive| directive.key == "BEGIN_DORMANT")
                .expect("the block is dormant"),
        );
        if wake
            .first()
            .is_some_and(|value| matches!(value, ZrdValue::Float(seconds) if *seconds < 0.0))
        {
            unreachable.push(*number);
        }
    }
    assert_eq!(
        unreachable,
        [15],
        "the dormant block nothing can enter changed"
    );
    // Block 15 is empty and dormant; block 4 — the "no" half of the first zone
    // gate, which block 3 kills — is entered by block 2's nap instead.
    assert_eq!(
        sites(&blocks, 4, "COMPLETED_SOUND_GROUP"),
        [vec![ZrdValue::Text("snd_MN1DZNo".to_owned())]]
    );
    // Blocks 8 and 9 are empty and awake: the lowering completes them on wake.
    for number in [8, 9] {
        assert!(
            block(&blocks, number).is_empty(),
            "OBJECTIVE{number} is no longer an empty always-awake block"
        );
    }
}

/// **The player start is this archive's own aircraft table, and the control
/// record repeats it.**
///
/// `cs_app::mission_start` reads `ZBD/C5/M01/zrdr.zbd#aiv.zrd`: one player
/// record with no wingmates, the stored pose the record's `PLAYER_INIT`
/// repeats, and the campaign chain's airframe. Nothing in M21's control record
/// assigns an airframe of its own — the record's only player-spelling sites are
/// proximity subjects.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m21_b_the_player_start_is_the_aircraft_tables_own_record() {
    let configuration =
        recover_retail_start_configuration(&game_dir(), MISSION).expect("M21's start reads");
    assert_eq!(configuration.mission(), MISSION);
    assert!(
        configuration.wingmates().is_empty(),
        "M21 launches no `wingman_<n>` record: {:?}",
        configuration.wingmates()
    );
    let Resolved::Known(player) = configuration.player() else {
        panic!("M21's aircraft table does not hold exactly one player record")
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
        .expect("M21's player record has the measured pose shape");
    assert_eq!(pose.position, [-12166.0, 400.0, -13112.0]);
    assert_eq!(pose.heading, -90.0);

    match configuration.airframe() {
        Resolved::Known(known) => {
            assert_eq!(
                known.value.as_str(),
                "airframe/player_pfighter",
                "the campaign chain's own row decides M21's airframe"
            );
        }
        Resolved::Unknown { reason, .. } => {
            assert!(
                reason.contains("CS_ENGINE_IMAGE"),
                "without the owner's image the airframe must refuse by naming that input: {reason}"
            );
        }
    }

    // The record's own PLAYER_INIT repeats the aircraft table's pose.
    let document = control_document();
    let init = record_field(&document, "PLAYER_INIT");
    assert_eq!(
        init,
        vec![
            ZrdValue::Int(1),
            ZrdValue::List(vec![
                ZrdValue::Float(-12166.0),
                ZrdValue::Float(400.0),
                ZrdValue::Float(-13112.0),
            ]),
            ZrdValue::List(vec![
                ZrdValue::Float(0.0),
                ZrdValue::Float(-90.0),
                ZrdValue::Float(0.0),
            ]),
            ZrdValue::Float(0.8),
            ZrdValue::Float(180.0),
        ],
        "the authored player start, as the record spells it"
    );
    let init_position = floats(init[1].as_list().expect("a three-number vector"));
    assert_eq!(
        init_position,
        pose.position.to_vec(),
        "PLAYER_INIT's position and the aircraft table's stored position disagree"
    );
    let init_heading = floats(init[2].as_list().expect("a three-number vector"));
    assert_eq!(
        init_heading,
        [0.0, pose.heading, 0.0],
        "PLAYER_INIT's middle vector does not carry the stored heading"
    );
    let timer = record_field(&document, "MISSION_TIMER");
    assert_eq!(
        floats(&timer),
        [0.0],
        "M21's record-level mission timer changed"
    );

    // The record assigns the player no airframe: every `player` spelling in the
    // control document is a proximity subject, and the archive carries no
    // instant-action scenario whose `player_plane` could decide it instead.
    let row = census().row(MISSION).expect("M21 is in the census");
    assert!(
        !row.members.iter().any(|member| member.name == "ia.zrd"),
        "M21's archive carries an instant-action scenario"
    );
    let mut player_sites = 0usize;
    for (_, directives) in blocks_of(&document) {
        for directive in directives {
            if arguments(&directive)
                .iter()
                .any(|value| value.as_text() == Some("player"))
            {
                assert!(
                    matches!(directive.key.as_str(), "TRAVELERS" | "INACTIVE1"),
                    "the control record writes on the player with {}",
                    directive.key
                );
                player_sites += 1;
            }
        }
    }
    assert_eq!(
        player_sites, 6,
        "the five player proximity sites and the one inactive-member lookup changed"
    );
}

/// **Every condition lowers, every call binds, and M21's record completes.**
///
/// The lowering accounting is asked of the record's own attempt: 63 objectives,
/// 63 lowered conditions, 237 bound calls, a clean `MissionProgram::validate`,
/// four met requirements and no unmet row anywhere in the installation's
/// unmet-requirement view for this mission.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m21_b_every_call_binds_every_condition_lowers_and_m21s_record_completes() {
    let row = census().row(MISSION).expect("M21 is in the census");
    let record = row.record().expect("M21 has a measured control program");
    let attempt = row
        .lowering_attempt()
        .expect("M21's record produced a lowering attempt")
        .attempt();

    assert_eq!(
        attempt.mission,
        Ok("mission/ch5-m01".to_owned()),
        "the lowered program carries the wrong mission id: {:?}",
        attempt.mission
    );
    assert_eq!(attempt.objectives, BLOCKS, "not one RawObjective per block");
    assert_eq!(attempt.conditions.len(), BLOCKS as usize);
    assert!(
        attempt
            .conditions
            .iter()
            .all(|outcome| matches!(outcome, ConditionOutcome::Lowered)),
        "a block's condition did not lower: {:?}",
        attempt
            .conditions
            .iter()
            .enumerate()
            .find(|(_, outcome)| !matches!(outcome, ConditionOutcome::Lowered))
    );
    assert_eq!(attempt.calls.len(), SITES as usize, "not one call per site");
    assert!(
        attempt
            .calls
            .iter()
            .all(|outcome| matches!(outcome, CallOutcome::Bound)),
        "a site did not bind: {:?}",
        attempt
            .calls
            .iter()
            .enumerate()
            .find(|(_, outcome)| !matches!(outcome, CallOutcome::Bound))
    );
    assert!(
        attempt.unbound_keys.is_empty(),
        "the registry refused a key: {:?}",
        attempt.unbound_keys
    );
    assert_eq!(
        attempt.validation.as_ref().map(Vec::len),
        Some(0),
        "MissionProgram::validate refused the program: {:?}",
        attempt.validation
    );

    let lowering = row.lowering().expect("M21's record lowers");
    assert!(lowering.complete(), "the lowering accounting is not met");
    assert_eq!(
        lowering.unmet().count(),
        0,
        "an unmet requirement: {:?}",
        lowering.unmet().collect::<Vec<_>>()
    );
    assert_eq!(
        lowering.requirements().len(),
        4,
        "the requirement set moved"
    );
    for requirement in lowering.requirements() {
        assert!(requirement.met, "{}", requirement.label());
        assert!(
            requirement.unmeasured_fields.is_empty(),
            "{} names unmeasured fields",
            requirement.label()
        );
    }
    assert!(
        row.is_complete(),
        "the census row is not complete: {}",
        record.to_lowering_refusal(attempt)
    );
    assert!(
        record.is_complete(attempt),
        "the record's own completeness verdict changed"
    );
    assert!(lowering.unmeasured_fields().is_empty());

    // The whole installation's unmet-requirement view carries no row for M21.
    let unmet = census().unmet_by_requirement();
    for (requirement, missions) in &unmet {
        assert!(
            !missions.contains(&MISSION.to_owned()),
            "M21 is listed under the unmet requirement {requirement}: {missions:?}"
        );
    }
}

/// **M21's row is complete and the campaign gate stays shut.**
///
/// One measured, complete row among the census's; an incomplete sibling beside
/// it; and `campaign_ready()` still `false`, because the gate asks about every
/// mission-scoped reader rather than about the rows that were measured.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m21_b_m21s_row_is_complete_and_the_campaign_stays_unready() {
    let corpus = census();
    let row = corpus.row(MISSION).expect("M21 is in the census");
    assert!(row.is_measured(), "M21's control program is not measured");
    assert!(row.is_complete(), "M21's row is not complete");
    assert!(
        corpus.complete_missions().contains(&MISSION),
        "M21 is not one of the complete missions: {:?}",
        corpus.complete_missions()
    );
    assert!(
        corpus.measured_len() < corpus.len(),
        "every archive now measures, so the row count no longer distinguishes anything"
    );
    let incomplete: Vec<&str> = corpus
        .rows()
        .iter()
        .filter(|other| !other.is_complete())
        .map(|other| other.mission.as_str())
        .collect();
    assert!(
        !incomplete.is_empty(),
        "no incomplete sibling remains, so the campaign gate has nothing left to refuse"
    );
    assert!(
        !corpus.campaign_ready(),
        "the campaign gate opened on this stage's evidence"
    );
    assert!(
        !corpus.unmeasured_fields().is_empty() || corpus.campaign_ready(),
        "the corpus reports no unmeasured field at all while the gate is shut"
    );
}

// -------------------------------------------------------------- synthetic ---

/// The measured `TRAVELERS` argument list of M21's guide site, authored from
/// the retail block 20's own shape.
fn guide_arguments() -> Vec<ZrdValue> {
    vec![
        ZrdValue::Text("autogyro_1".to_owned()),
        ZrdValue::Text("APPROACHING".to_owned()),
        ZrdValue::List(vec![
            ZrdValue::Float(-344.0),
            ZrdValue::Float(250.0),
            ZrdValue::Float(-4347.5),
        ]),
        ZrdValue::Float(1000.0),
        ZrdValue::Int(1),
        ZrdValue::Text("DELETE_ON_SUCCESS".to_owned()),
    ]
}

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

fn lower(document: &ZrdValue) -> LoweredControlRecord {
    let record = measure_control_record(document);
    lower_control_record(
        ContentId::from_source(ContentKind::Mission, "accept-m21-b")
            .map_err(|error| error.to_string()),
        "accept-m21-b",
        document,
        &record,
    )
}

/// **M21's guide site binds at its measured shape and is refused when an
/// argument the IR cannot carry sits inside it.**
///
/// The signature the registry registers comes from the record's own measured
/// shapes, so a truncated site would simply measure a shorter shape and bind —
/// the honest refusal arm is a value the adapter refuses rather than a count:
/// the same site with a non-finite radius produces no `RawCall`, reports the
/// refusal by block and key, damages the block's condition and leaves
/// `MissionProgram::validate` with an error instead of a flyable program.
#[test]
fn accept_m21_b_a_guide_approach_site_binds_at_its_measured_shape_and_refuses_a_radius_the_ir_cannot_carry()
 {
    let well_formed = record_of(vec![(
        1,
        vec![text("TRAVELERS"), ZrdValue::List(guide_arguments())],
    )]);
    let record = measure_control_record(&well_formed);
    assert!(
        matches!(
            record.key("TRAVELERS").map(|key| key.disposition()),
            Some(DirectiveDisposition::Measured(_))
        ),
        "the measured proximity key lost its disposition"
    );
    assert_eq!(
        record
            .key("TRAVELERS")
            .and_then(|key| key.agreed_shape())
            .map(|shape| shape.label()),
        Some("[text,text,[float,float,float],float,int,text]".to_owned()),
        "the guide site's measured shape changed"
    );
    let lowered = lower(&well_formed);
    assert!(
        lowered
            .attempt()
            .calls
            .iter()
            .all(|call| matches!(call, CallOutcome::Bound)),
        "the measured guide site did not bind: {:?}",
        lowered.attempt().calls
    );
    assert!(lowered.program().is_some(), "no program stood");
    assert_eq!(
        lowered.attempt().validation.as_ref().map(Vec::len),
        Some(0),
        "the program did not validate: {:?}",
        lowered.attempt().validation
    );

    // The same key, one radius not a number: the adapter refuses the value
    // rather than coercing it.
    let non_finite = {
        let mut arguments = guide_arguments();
        arguments[3] = ZrdValue::Float(f32::NAN);
        record_of(vec![(
            1,
            vec![text("TRAVELERS"), ZrdValue::List(arguments)],
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
        "the non-finite radius was not refused exactly once: {:?}",
        lowered.attempt().calls
    );
    assert!(
        refusals[0].contains("not finite"),
        "the refusal does not name the value it refused: {}",
        refusals[0]
    );
    assert!(
        refusals[0].contains("TRAVELERS"),
        "the refusal does not name the key: {}",
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

/// **The route threshold pairs with its evaluator, and an unknown key refuses
/// the record rather than being ignored.**
///
/// M21's block 37 always spells the count beside the evaluator, so the pair is
/// the shape production binds; the threshold alone still stands, because the
/// count key is itself measured. A key nobody measured refuses the record
/// outright — the contract's "no binding without a measured meaning", carried
/// into CI without original data.
#[test]
fn accept_m21_b_the_zone_threshold_pairs_with_its_evaluator_and_an_unknown_key_refuses() {
    let paired = record_of(vec![
        (
            1,
            vec![
                text("DANGER_ZONES_COMPLETION_COUNT"),
                ZrdValue::List(vec![int(4)]),
                text("DANGER_ZONES_COMPLETED"),
                ZrdValue::List(vec![
                    text("dzpath22"),
                    text("dzpath23"),
                    text("dzpath24"),
                    text("dzpath25"),
                    text("dzpath26"),
                    text("dzpath27"),
                ]),
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

    // …the threshold alone: the count key is measured on its own, so the record
    // still stands while no zone evaluator exists to pair with.
    let threshold_only = record_of(vec![(
        1,
        vec![
            text("DANGER_ZONES_COMPLETION_COUNT"),
            ZrdValue::List(vec![int(4)]),
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
                ZrdValue::List(vec![int(4)]),
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
