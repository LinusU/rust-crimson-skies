//! Acceptance stage M18-B: the mission-specific compatibility gaps of the
//! eighteenth mission (`missions/M18.md`, work order `M18-B`).
//!
//! M18-A bound M18's identities and left the mission program unmeasured. The
//! machinery that measures a control program (the `.zrd` reader, the census,
//! the directive dispositions and the record → `RawProgram` adapter) is shared
//! and was built for M01; this stage runs it over M18's own reader archive and
//! pins what is **different** at M18, together with the three regression
//! priorities the sheet names:
//!
//! * M18's control program is `objectives.zrd` — the eighth of seventeen
//!   members of `ZBD/C4/M03/zrdr.zbd`, with 52 numbered blocks and 238
//!   directive sites under a 29-key vocabulary, selected by the content rule
//!   and not by its name (`aiv.zrd` and `zep_dock.zrd` are both longer);
//! * all 29 keys have a disposition — 2 terminal outcomes and 27 with a
//!   measured effect, **none refused** — and the record **lowers
//!   completely**: all 238 sites bind, all 52 conditions lower,
//!   `MissionProgram::validate` accepts and M18's census row is complete;
//! * **alternative action order**: seven blocks are awake when the mission
//!   starts (2, 8, 9, 10, 14, 46, 50) and only block 1 arms a clock, so the
//!   order is record data. The same five-clamp release is observed twice —
//!   block 46 from the mission's first tick and block 4 only after block 3's
//!   approach predicate — and both lower to the same `InactiveMembers`
//!   condition; the five per-clamp blocks 26–30 are entered together by one
//!   wake list and share no edge, so the record permits their releases in any
//!   order;
//! * **release dependencies**: exactly two `TICK_DEPENDS_ON_OBJ` gates (blocks
//!   24 and 45, on blocks 23 and 44), each lowering into the gated block's own
//!   `ObjectiveAwake` conjunct with the dependency's zero-based index — an
//!   order gate on evaluation, never a timing guess;
//! * **rescue interaction**: the sheet's label names no key, so what is pinned
//!   is what the record spells — the one `TRAVELERS` approach site (block 3:
//!   `player` approaching `cargozep1` at 1500), the five per-clamp
//!   `INACTIVE`/`REMOVE_OBJECTIVE_TARGET` pairs, and the measured absence of
//!   any docking, pickup, boarding or transfer directive in the vocabulary;
//! * **one measured gap**: the sound group `snd_c4-RM-m3_BlackSwan_27`, which
//!   blocks 12 and 50 stop, is declared by **no shipped record** — it occurs in
//!   exactly one file of the installation (the control member itself), while
//!   every one of the other 23 sound names the record spells is carried by the
//!   shared `ZBD/zrdr.zbd` sounds table. It is recorded, not worked around.
//!
//! A lowered program is **not** a played mission: no playthrough, difficulty,
//! media or presentation row is covered (that is M18-C, with `human_play`), and
//! the wrong-actor, wrong-session and repeated-event halves of the sheet's
//! priorities are runtime observations that stay unmeasured here. The measured
//! unknowns are written up in
//! `docs/findings/2026-10-10-m18-b-compatibility-gaps.md`.
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`; the synthetic
//! tests run in CI.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_app::animation::mission::bind_mission_animation;
use cs_app::control_lowering::{LoweredControlRecord, lower_control_record};
use cs_app::mission_control::{
    RetailControlCensus, read_control_member, survey_mission_control_programs,
};
use cs_assets::install::{discover, sha256};
use cs_content::campaign_bindings::{
    MissionControlBinding, MissionLabel, SourceBinding, SourceContext,
};
use cs_content::mission_control::{
    CallOutcome, ConditionOutcome, DirectiveDisposition, TerminalOutcome, measure_control_record,
};
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{ZrdValue, objective_record, zrd_flat_fields};
use cs_formats::script_raw::discover_container;
use cs_script::ir::{Condition, MemberName, TravelersAnchor};
use cs_types::content::{ContentId, ContentKind};
use cs_types::install::RelativePath;

use crate::common::load_inventory;

/// The census row label of the mission (F13-B's mission-scope rule).
const MISSION: &str = "zbd/c4/m03";
/// The reader archive the installation ships for M18 — the program span
/// `missions/bindings/M18.json` cites.
const CONTAINER: &str = "ZBD/C4/M03/zrdr.zbd";
/// The measured SHA-256 of that whole archive.
const CONTAINER_SHA256: &str = "0eff1e9444c23a1cc0fcba3d7a5c18f0c3b53fc5c9b967cad40ada242687c002";
/// The archive's length in bytes — M18-A's own source span.
const CONTAINER_LENGTH: u64 = 90_978;
/// The member the measured rule chose.
const CONTROL_MEMBER: &str = "objectives.zrd";
/// The control member's first byte inside the archive.
const CONTROL_OFFSET: u64 = 22_381;
/// The control member's length in bytes.
const CONTROL_LENGTH: u64 = 16_300;
/// SHA-256 of the control member's own bytes.
const CONTROL_SHA256: &str = "83e5f70d51c92cb19c5a249c7abbea3de9813eb928233271bdfdad695d6c191d";
/// The numbered blocks of the control member.
const BLOCKS: u32 = 52;
/// The directive sites of the control member.
const SITES: u32 = 238;
/// The distinct directive keys of the control member.
const KEYS: usize = 29;
/// The distinct texts the record's directive sites spell — actors, targets,
/// animations, sound groups, message operands, keywords and block addresses'
/// operands. Full-coverage claims below are measured against this count.
const NAMED_TEXTS: usize = 86;

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M18-B needs the retail capability; run this suite with \
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

/// The declared discovery title of `M18`, read from the committed inventory.
fn declared_title() -> String {
    load_inventory()
        .iter()
        .find(|(label, _)| label.as_str() == "M18")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M18 work order")
}

/// The production control-program binding, built once for the whole suite.
fn control_binding() -> &'static MissionControlBinding {
    static BINDING: OnceLock<MissionControlBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        let context = SourceContext::read(&game_dir())
            .expect("production source context reads the installation");
        context
            .control_program(
                MissionLabel::new("M18").expect("M18 is a valid label"),
                &declared_title(),
            )
            .expect("M18's control program binds through the measured rule")
    })
}

/// The M18 mission binding M18-A derives, built once — this stage consumes its
/// identities and adds no second evidence for the join itself.
fn mission_binding() -> &'static SourceBinding {
    static BINDING: OnceLock<SourceBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        let context = SourceContext::read(&game_dir())
            .expect("production source context reads the installation");
        context
            .bind(
                MissionLabel::new("M18").expect("M18 is a valid label"),
                &declared_title(),
            )
            .expect("M18 binds to the original data")
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
        let children = value.as_list().expect("every M18 block is a list");
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

/// The control member's decoded document, re-read through production discovery.
fn control_document() -> ZrdValue {
    read_control_member(&game_dir(), MISSION)
        .expect("the rule finds M18's control member")
        .0
}

/// The blocks of the record that spell `key`, in block order.
fn with(blocks: &[(u32, Vec<Directive>)], key: &str) -> Vec<u32> {
    blocks
        .iter()
        .filter(|(_, directives)| directives.iter().any(|directive| directive.key == key))
        .map(|(number, _)| *number)
        .collect()
}

/// The argument lists a key is spelled with, in block order.
fn spelled(blocks: &[(u32, Vec<Directive>)], key: &str) -> Vec<Vec<ZrdValue>> {
    blocks
        .iter()
        .flat_map(|(_, directives)| directives.iter())
        .filter(|directive| directive.key == key)
        .map(|directive| directive.args.clone().unwrap_or_default())
        .collect()
}

/// The directives of one numbered block, in spelling order.
fn block(blocks: &[(u32, Vec<Directive>)], number: u32) -> &[Directive] {
    &blocks
        .iter()
        .find(|(candidate, _)| *candidate == number)
        .unwrap_or_else(|| panic!("the record has no OBJECTIVE{number}"))
        .1
}

/// The integer addresses a directive spells in its argument list. A nap's
/// delay is a float, so it never contributes an address (M02-B-FU3 #802,
/// re-derived by M06-B-FU3 #819: a spelled address is the one-based block
/// number, which the original's parse decrements to the record index).
fn addresses(directive: &Directive) -> Vec<i64> {
    directive
        .args
        .iter()
        .flatten()
        .filter_map(|value| match value {
            ZrdValue::Int(int) => Some(i64::from(*int)),
            _ => None,
        })
        .collect()
}

/// Whether a directive addresses other blocks at all.
fn addresses_blocks(directive: &Directive) -> bool {
    directive.key.ends_with("_OBJECTIVE_WHEN_I_COMPLETE") || directive.key == "TICK_DEPENDS_ON_OBJ"
}

/// `(block → its prerequisites)`: every wake or nap that enters the block, plus
/// the dependency each `TICK_DEPENDS_ON_OBJ` gate declares. Kill edges are
/// deliberately absent — killing a block prevents it, it does not enter it.
fn prerequisites(blocks: &[(u32, Vec<Directive>)]) -> BTreeMap<u32, Vec<u32>> {
    let mut map: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (number, directives) in blocks {
        for directive in directives {
            if !addresses_blocks(directive) || directive.key == "KILL_OBJECTIVE_WHEN_I_COMPLETE" {
                continue;
            }
            if directive.key == "TICK_DEPENDS_ON_OBJ" {
                // A gate runs the other way round: the *gated* block depends on
                // the block its `child0` names, so the dependency becomes a
                // prerequisite of the block that spells the gate.
                for address in addresses(directive) {
                    map.entry(*number).or_default().push(address as u32);
                }
                continue;
            }
            for address in addresses(directive) {
                map.entry(address as u32).or_default().push(*number);
            }
        }
    }
    map
}

/// Every block reachable from `target` backwards — the blocks that must be
/// entered before `target` can be entered, including `target` itself.
fn closure(map: &BTreeMap<u32, Vec<u32>>, target: u32) -> Vec<u32> {
    let mut seen = BTreeSet::new();
    seen.insert(target);
    let mut stack = vec![target];
    while let Some(node) = stack.pop() {
        let Some(predecessors) = map.get(&node) else {
            continue;
        };
        for predecessor in predecessors {
            if seen.insert(*predecessor) {
                stack.push(*predecessor);
            }
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

/// Every distinct `.zrd` text node of one container's member: names only,
/// never bytes.
fn member_texts(container: &str, member: &str) -> Vec<String> {
    let bytes = std::fs::read(game_dir().join(container)).expect("the archive reads");
    let relative =
        RelativePath::new(&container.to_lowercase()).expect("the archive path is relative");
    let discovery = discover_container(&relative.logical_key(), &relative, &bytes);
    let mut texts = Vec::new();
    for program in discovery.programs() {
        if program.locator().member() != Some(member) {
            continue;
        }
        let document = cs_content::stunts::decode_zrd(program.bytes()).expect("the member decodes");
        walk_texts(&document, &mut texts);
    }
    texts.sort();
    texts.dedup();
    texts
}

/// Every distinct `.zrd` text node the whole installation declares **outside**
/// M18's control member, collected in one pass (the control member only
/// *spells* a name, it never declares one).
fn installation_texts_outside_control() -> BTreeSet<String> {
    let found = discover(&game_dir()).expect("production discovery reads the installation");
    let mut texts = Vec::new();
    for record in &found.manifest.files {
        let spelling = record.relative_spelling.as_str();
        if !spelling.to_lowercase().ends_with(".zbd") {
            continue;
        }
        let Ok(relative) = RelativePath::new(&spelling.to_lowercase()) else {
            continue;
        };
        let bytes = std::fs::read(found.manifest.host_root.join(spelling))
            .unwrap_or_else(|error| panic!("read {spelling}: {error}"));
        let discovery = discover_container(&relative.logical_key(), &relative, &bytes);
        for program in discovery.programs() {
            let Some(member) = program.locator().member() else {
                continue;
            };
            if spelling.eq_ignore_ascii_case(CONTAINER) && member == CONTROL_MEMBER {
                continue;
            }
            let Ok(document) = cs_content::stunts::decode_zrd(program.bytes()) else {
                continue;
            };
            walk_texts(&document, &mut texts);
        }
    }
    texts.sort();
    texts.dedup();
    texts.into_iter().collect()
}

/// Every `(container, member)` of the installation whose decoded `.zrd` texts
/// spell any of `names` exactly — one whole-installation pass for all of them.
fn declarations_of(names: &[&str]) -> BTreeMap<String, Vec<(String, String)>> {
    let found = discover(&game_dir()).expect("production discovery reads the installation");
    let mut hits: BTreeMap<String, Vec<(String, String)>> = names
        .iter()
        .map(|name| (name.to_string(), Vec::new()))
        .collect();
    for record in &found.manifest.files {
        let spelling = record.relative_spelling.as_str();
        if !spelling.to_lowercase().ends_with(".zbd") {
            continue;
        }
        let Ok(relative) = RelativePath::new(&spelling.to_lowercase()) else {
            continue;
        };
        let bytes = std::fs::read(found.manifest.host_root.join(spelling))
            .unwrap_or_else(|error| panic!("read {spelling}: {error}"));
        let discovery = discover_container(&relative.logical_key(), &relative, &bytes);
        for program in discovery.programs() {
            let Some(member) = program.locator().member() else {
                continue;
            };
            let Ok(document) = cs_content::stunts::decode_zrd(program.bytes()) else {
                continue;
            };
            let mut texts = Vec::new();
            walk_texts(&document, &mut texts);
            for name in names {
                if texts.iter().any(|text| text == name) {
                    hits.entry((*name).to_owned())
                        .or_default()
                        .push((spelling.to_owned(), member.to_owned()));
                }
            }
        }
    }
    for sites in hits.values_mut() {
        sites.sort();
        sites.dedup();
    }
    hits
}

/// Every installation file whose **raw bytes** contain each of `needles`, in
/// one pass over the manifest. A raw scan and a decoded `.zrd` walk are
/// different claims: this one says what the shipped bytes carry, whatever
/// format carries it.
fn files_containing(needles: &[&[u8]]) -> BTreeMap<Vec<u8>, Vec<String>> {
    let found = discover(&game_dir()).expect("production discovery reads the installation");
    let mut hits: BTreeMap<Vec<u8>, Vec<String>> = needles
        .iter()
        .map(|needle| (needle.to_vec(), Vec::new()))
        .collect();
    for record in &found.manifest.files {
        let spelling = record.relative_spelling.as_str();
        let bytes = std::fs::read(found.manifest.host_root.join(spelling))
            .unwrap_or_else(|error| panic!("read {spelling}: {error}"));
        for needle in needles {
            if bytes.windows(needle.len()).any(|window| window == *needle) {
                hits.entry((*needle).to_vec())
                    .or_default()
                    .push(spelling.to_owned());
            }
        }
    }
    for files in hits.values_mut() {
        files.sort();
        files.dedup();
    }
    hits
}

// ---------------------------------------------------------------------------
// Retail: what M18's control program is
// ---------------------------------------------------------------------------

/// **The control program is the member that declares the numbered blocks.**
///
/// Of the seventeen members of M18's reader archive exactly one declares
/// numbered `OBJECTIVE<N>` blocks: it is the eighth member, and the two
/// members that are longer than it (`aiv.zrd`, `zep_dock.zrd`) declare none,
/// so neither size nor position picks it. The blocks and sites the census
/// measures equal an independent walk of the same document, the archive is the
/// program span M18-A bound, and the production control binding reaches the
/// same member, span and digests through its own walk.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m18_b_the_control_program_is_the_member_that_declares_the_blocks() {
    let row = census().row(MISSION).expect("M18 is in the census");
    assert_eq!(row.container, CONTAINER);
    assert_eq!(
        row.container_sha256, CONTAINER_SHA256,
        "the reader archive is the program M18-A bound"
    );
    assert_eq!(row.members.len(), 17);

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
        ["aiv.zrd", "zep_dock.zrd"],
        "size is not the rule: the two longest members are not the control member"
    );

    let record = row.record().expect("M18 has a measured control program");
    assert_eq!((record.blocks(), record.sites()), (BLOCKS, SITES));
    assert_eq!(record.keys().len(), KEYS);

    let (document, member) = read_control_member(&game_dir(), MISSION)
        .expect("the rule finds M18's control member again");
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
        "numbered 1..=52, no gaps"
    );
    let walked: usize = blocks.iter().map(|(_, directives)| directives.len()).sum();
    assert_eq!(walked as u32, record.sites(), "the independent walk agrees");
    assert!(
        record.refusals().is_empty(),
        "every block is a readable list"
    );
    assert!(
        record.unclassified_record_keys().is_empty(),
        "M18 spells no record-level key outside the measured vocabulary"
    );

    // The production binding and the census must not disagree about the
    // mission, the program, the member or the record.
    let bound = control_binding();
    assert_eq!(bound.mission.as_str(), "mission/ch4-m03");
    assert_eq!(bound.program_id.as_str(), "script/c4-m03-zrdr");
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

    // …and both agree with the mission binding M18-A committed: one mission id,
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
}

/// **Every directive key M18 spells has exactly one disposition, and none is
/// refused.**
///
/// Two keys are terminal outcomes (`INSTANTWIN`, `INSTANTLOSS`, one site each);
/// the other 27 have a measured effect. The sites are accounted for: the keys'
/// sites sum to the record's, and the sorted vocabulary is exactly the 29 keys
/// the archive spells — so a key that appears, disappears or silently loses its
/// meaning fails here.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m18_b_every_directive_m18_spells_has_a_disposition_and_none_is_refused() {
    let record = census().row(MISSION).unwrap().record().unwrap();
    let mut outcomes = BTreeMap::new();
    let mut measured = 0;
    for key in record.keys() {
        match key.disposition() {
            DirectiveDisposition::TerminalOutcome { outcome } => {
                outcomes.insert(key.key.clone(), outcome);
            }
            DirectiveDisposition::Measured(_) => measured += 1,
            DirectiveDisposition::Unmeasured { reason } => {
                panic!("{} is unmeasured ({reason:?})", key.key)
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
    assert_eq!(measured, 27);
    let sites: u32 = record.keys().iter().map(|key| key.sites).sum();
    assert_eq!(sites, SITES, "no site is dropped from the accounting");

    let vocabulary: BTreeSet<&str> = record.keys().iter().map(|key| key.key.as_str()).collect();
    assert_eq!(
        vocabulary,
        [
            "ADD_OBJECTIVE_TARGET",
            "ADD_OTHER_TARGET",
            "ANIM_STATE",
            "BEGIN_DORMANT",
            "COMPLETED_SOUND_GROUP",
            "DEDG",
            "IDENTITY",
            "INACTIVE1",
            "INACTIVE2",
            "INACTIVE3",
            "INACTIVE4",
            "INACTIVE5",
            "INACTIVE6",
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
            "TICK_DEPENDS_ON_OBJ",
            "TRAVELERS",
            "WAKEUP_ENEMIES",
            "WAKEUP_ZEP_TURRETS",
            "WAKE_ANIM",
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
        ]
        .into_iter()
        .collect::<BTreeSet<&str>>(),
        "the record's vocabulary is exactly these 29 keys"
    );

    // The record-level keys are the five measured fields, no more and no less.
    let fields: Vec<&str> = record
        .record_fields()
        .iter()
        .map(|(field, _)| field.key())
        .collect();
    assert_eq!(
        fields,
        [
            "MISSION_TIMER",
            "PLAYER_INIT",
            "RESTORE_ANIMS",
            "EXECUTE_ANIMS",
            "INVALIDATE_ANIMS"
        ]
    );
    assert!(record.record_sounds().is_empty());
}

/// **Every call binds, every condition lowers and M18's record completes.**
///
/// All 238 sites bind and all 52 block conditions lower, so a program stands
/// and reaches `MissionProgram::validate` clean; no lowering requirement is
/// unmet and M18's census row is complete. The census as a whole stays not
/// campaign-ready: other missions carry their own gaps.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m18_b_every_call_binds_every_condition_lowers_and_m18s_record_completes() {
    let row = census().row(MISSION).unwrap();
    let record = row.record().unwrap();

    for (key, (blocks, sites)) in [
        ("KILL_OBJECTIVE_WHEN_I_COMPLETE", (5u32, 5u32)),
        ("NAP_OBJECTIVE_WHEN_I_COMPLETE", (19, 19)),
        ("WAKE_OBJECTIVE_WHEN_I_COMPLETE", (14, 14)),
        ("TICK_DEPENDS_ON_OBJ", (2, 2)),
        ("TRAVELERS", (1, 1)),
        ("SET_AI_NET", (1, 1)),
    ] {
        let measured = record.key(key).expect("it is spelled");
        assert_eq!(
            (measured.blocks, measured.sites),
            (blocks, sites),
            "{key} was measured over another record"
        );
    }

    let attempt = row.lowering_attempt().unwrap();
    let lowered = attempt.attempt();
    assert_eq!(lowered.mission.as_deref(), Ok("mission/ch4-m03"));
    assert_eq!(lowered.objectives, BLOCKS);
    assert_eq!(lowered.calls.len() as u32, SITES);
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
        "the registry refused no key: {:?}",
        lowered.unbound_keys
    );
    assert!(
        attempt.program().is_some(),
        "a program stands for all 52 objectives"
    );
    assert_eq!(
        lowered.validation,
        Some(Vec::new()),
        "the bound program reaches MissionProgram::validate and validates"
    );
    let refused_conditions: Vec<(usize, &str)> = lowered
        .conditions
        .iter()
        .enumerate()
        .filter_map(|(index, outcome)| match outcome {
            ConditionOutcome::Refused(text) | ConditionOutcome::Unreadable(text) => {
                Some((index, text.as_str()))
            }
            ConditionOutcome::Lowered => None,
        })
        .collect();
    assert!(
        refused_conditions.is_empty(),
        "all 52 block conditions lower: {refused_conditions:?}"
    );
    assert_eq!(lowered.conditions.len(), BLOCKS as usize);

    let lowering = row.lowering().unwrap();
    assert_eq!(
        lowering.unmet().count(),
        0,
        "every lowering requirement is met: {:?}",
        lowering
            .unmet()
            .map(|row| row.kind.code())
            .collect::<Vec<_>>()
    );
    assert!(lowering.complete());
    assert!(row.is_complete());
}

// ---------------------------------------------------------------------------
// The sheet's three regression priorities, as the record spells them
// ---------------------------------------------------------------------------

/// **Release dependencies: exactly two gates, each lowering into the gated
/// block's own wake conjunct.**
///
/// `TICK_DEPENDS_ON_OBJ` is the only dependency directive M18 spells: block 24
/// on block 23 and block 45 on block 44, both `child0 − 1` in the lowered
/// program — the dependency's own zero-based index, the measured convention
/// (M02-B-FU3 #802, re-derived by M06-B-FU3 #819). The two dependency blocks
/// are dormant and identical in shape: each spells `BEGIN_DORMANT -1` and one
/// `INACTIVE1("piratezep")`, so both lower to the same in-play predicate under
/// the default threshold. Order is therefore a gate on evaluation, never a
/// timing guess: before the dependency is awake the gated block runs no timers
/// and evaluates no conditions, and what happens at that boundary in the
/// original (wrong actor, wrong session, a repeated event) is a runtime
/// observation left to M18-C.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m18_b_release_dependencies_are_two_gates_that_lower_as_wake_conjuncts() {
    let row = census().row(MISSION).unwrap();
    let blocks = blocks_of(&control_document());
    let record = row.record().unwrap();
    assert_eq!(
        match record.key("TICK_DEPENDS_ON_OBJ").unwrap().disposition() {
            DirectiveDisposition::Measured(measured) => measured.operation,
            other => panic!("TICK_DEPENDS_ON_OBJ is not measured: {other:?}"),
        },
        cs_content::mission_control::DirectiveOperation::DependencyGate
    );

    assert_eq!(with(&blocks, "TICK_DEPENDS_ON_OBJ"), [24, 45]);
    assert_eq!(
        spelled(&blocks, "TICK_DEPENDS_ON_OBJ"),
        [vec![ZrdValue::Int(23)], vec![ZrdValue::Int(44)]],
        "the two dependencies are record data"
    );

    // The dependency blocks themselves: dormant, one predicate each.
    for number in [23u32, 44] {
        let directives = block(&blocks, number);
        assert_eq!(
            directives
                .iter()
                .map(|directive| directive.key.as_str())
                .collect::<Vec<_>>(),
            ["BEGIN_DORMANT", "INACTIVE1"],
            "block {number} spells only its dormancy and one in-play predicate"
        );
        assert_eq!(
            directives[1].args.as_deref(),
            Some(&[ZrdValue::Text("piratezep".to_owned())][..]),
            "block {number} watches the same actor as the other gate's dependency"
        );
    }

    // …and how the program lowered them: the gate is a conjunct carrying the
    // dependency's own zero-based index, beside the block's own wake state.
    let attempt = row.lowering_attempt().unwrap();
    let raw = attempt.raw_program().expect("the program assembled");
    let piratezep = |index: u32| {
        Condition::All(vec![
            Condition::ObjectiveAwake { index },
            Condition::InactiveMembers {
                members: vec![vec!["piratezep".to_owned()]],
                threshold: 1,
            },
        ])
    };
    assert_eq!(
        raw.objectives[22].condition,
        piratezep(22),
        "block 23 lowers its own predicate with the default threshold"
    );
    assert_eq!(
        raw.objectives[43].condition,
        piratezep(43),
        "block 44 is the same predicate, spelled a second time"
    );
    assert_eq!(
        raw.objectives[23].condition,
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 23 },
            Condition::ObjectiveAwake { index: 22 },
            Condition::EnemyGroupDepletion {
                group: 2,
                remaining: 0,
                generator: None,
            },
        ]),
        "block 24's gate is a conjunct, not a dropped directive, and its DEDG survives"
    );
    assert_eq!(
        raw.objectives[44].condition,
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 44 },
            Condition::ObjectiveAwake { index: 43 },
        ]),
        "block 45's gate is the whole condition: nothing else is spelled"
    );
}

/// **Alternative action order: the record observes one release twice, and the
/// five per-clamp blocks are free of each other.**
///
/// Seven blocks are awake when the mission starts — 2, 8, 9, 10, 14, 46, 50 —
/// and only block 1 arms a clock (2 s); every other dormant block spells `-1`.
/// Which of the two observers of the clamp release runs first is therefore not
/// a race the record invents:
///
/// * blocks 4 and 46 spell the **same** five-member chain
///   (`tiedown01..04`, `zmainclamp`, each `…/healthy`) under the same
///   threshold 1, and both lower to the same `InactiveMembers` condition;
/// * block 46 is eligible from the first tick (no `BEGIN_DORMANT`) and wakes 5
///   and 47 and kills 2, while block 4 is entered only by block 3's wake list
///   and naps 5 after 1 s. Block 5's two prerequisites are exactly those two
///   blocks, so the same transition is reachable through either path;
/// * the five per-clamp blocks 26–30 are entered together by block 3's single
///   wake list and no prerequisite edge joins any pair of them: the record
///   permits the five releases in any order, and nothing here claims which
///   order the original takes.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m18_b_alternative_action_order_is_two_observers_of_one_release_and_free_per_clamp_blocks()
{
    let row = census().row(MISSION).unwrap();
    let blocks = blocks_of(&control_document());

    let awake: Vec<u32> = blocks
        .iter()
        .filter(|(_, directives)| !directives.iter().any(|d| d.key == "BEGIN_DORMANT"))
        .map(|(number, _)| *number)
        .collect();
    assert_eq!(
        awake,
        [2, 8, 9, 10, 14, 46, 50],
        "seven blocks are awake when the mission starts"
    );
    let mut timed_wakes = Vec::new();
    for (number, directives) in &blocks {
        let Some(dormant) = directives.iter().find(|d| d.key == "BEGIN_DORMANT") else {
            continue;
        };
        if dormant
            .args
            .as_ref()
            .and_then(|args| args.first())
            .is_some_and(|value| matches!(value, ZrdValue::Float(seconds) if *seconds >= 0.0))
        {
            timed_wakes.push((*number, dormant.args.clone().unwrap_or_default()));
        }
    }
    assert_eq!(
        timed_wakes,
        [(1, vec![ZrdValue::Float(2.0)])],
        "only block 1 arms a clock; every other dormant block spells -1"
    );
    assert_eq!(
        row.record()
            .unwrap()
            .key("BEGIN_DORMANT")
            .map(|key| key.sites),
        Some(45),
        "45 of the 52 blocks spell BEGIN_DORMANT"
    );

    // The two observers: identical chains and threshold, different eligibility.
    let chains = |number: u32| -> Vec<MemberName> {
        block(&blocks, number)
            .iter()
            .filter(|directive| {
                directive.key.starts_with("INACTIVE")
                    && directive.key != "INACTIVE_COMPLETION_COUNT"
            })
            .map(|directive| {
                directive
                    .args
                    .as_ref()
                    .expect("an INACTIVE<n> site spells a chain")
                    .iter()
                    .map(|value| match value {
                        ZrdValue::Text(text) => text.clone(),
                        other => panic!("a chain link is not a name: {other:?}"),
                    })
                    .collect()
            })
            .collect()
    };
    let expected: Vec<MemberName> = [
        ["tiedown01", "healthy"],
        ["tiedown02", "healthy"],
        ["tiedown03", "healthy"],
        ["tiedown04", "healthy"],
        ["zmainclamp", "healthy"],
    ]
    .into_iter()
    .map(|chain| chain.into_iter().map(str::to_owned).collect())
    .collect();
    for number in [4u32, 46] {
        assert_eq!(
            chains(number),
            expected,
            "block {number} watches the five clamps in record order"
        );
        let directives = block(&blocks, number);
        let threshold = directives
            .iter()
            .find(|directive| directive.key == "INACTIVE_COMPLETION_COUNT")
            .and_then(|directive| directive.args.clone())
            .expect("the observer spells its threshold");
        assert_eq!(
            threshold,
            [ZrdValue::Int(1)],
            "block {number} completes on the first clamp released"
        );
    }
    assert!(
        block(&blocks, 4)
            .iter()
            .any(|directive| directive.key == "BEGIN_DORMANT"),
        "block 4 starts dormant"
    );
    assert!(
        !block(&blocks, 46)
            .iter()
            .any(|directive| directive.key == "BEGIN_DORMANT"),
        "block 46 is eligible from the mission's first tick"
    );

    let prerequisites = prerequisites(&blocks);
    assert_eq!(
        prerequisites.get(&4),
        Some(&vec![3]),
        "block 4 is entered only by the approach block"
    );
    assert!(
        !prerequisites.contains_key(&46),
        "block 46 has no prerequisite at all"
    );
    assert_eq!(
        prerequisites.get(&5),
        Some(&vec![4, 46]),
        "the same release reaches block 5 through either observer"
    );
    assert_eq!(
        block(&blocks, 46)
            .iter()
            .filter(|directive| directive.key == "WAKE_OBJECTIVE_WHEN_I_COMPLETE")
            .flat_map(addresses)
            .collect::<Vec<_>>(),
        [5, 47],
        "block 46 wakes the second ladder stage and block 47"
    );
    assert_eq!(
        block(&blocks, 46)
            .iter()
            .filter(|directive| directive.key == "KILL_OBJECTIVE_WHEN_I_COMPLETE")
            .flat_map(addresses)
            .collect::<Vec<_>>(),
        [2],
        "block 46 kills the opening block when the release precedes it"
    );
    assert_eq!(
        block(&blocks, 4)
            .iter()
            .find(|directive| directive.key == "NAP_OBJECTIVE_WHEN_I_COMPLETE")
            .and_then(|directive| directive.args.clone()),
        Some(vec![ZrdValue::Int(5), ZrdValue::Float(1.0)]),
        "block 4's own edge is a 1-second nap of block 5"
    );

    // The five per-clamp blocks: one wake list, no edge among them.
    let approach_wake: Vec<i64> = block(&blocks, 3)
        .iter()
        .filter(|directive| directive.key == "WAKE_OBJECTIVE_WHEN_I_COMPLETE")
        .flat_map(addresses)
        .collect();
    assert_eq!(
        approach_wake,
        [4, 26, 27, 28, 29, 30],
        "one site enters both observers and all five per-clamp blocks"
    );
    for number in 26u32..=30 {
        assert_eq!(
            prerequisites.get(&number),
            Some(&vec![3]),
            "block {number} is entered only by the approach block, so no other per-clamp block \
             gates it"
        );
    }

    // Both observers lower to the same predicate, differing only in their own
    // wake conjunct, so the order between them is not a lowering difference.
    let attempt = row.lowering_attempt().unwrap();
    let raw = attempt.raw_program().expect("the program assembled");
    for index in [3u32, 45] {
        assert_eq!(
            raw.objectives[index as usize].condition,
            Condition::All(vec![
                Condition::ObjectiveAwake { index },
                Condition::InactiveMembers {
                    members: expected.clone(),
                    threshold: 1,
                },
            ]),
            "block {} lowers the release predicate it spelled",
            index + 1
        );
    }
}

/// **Rescue interaction: the one approach site, the five per-clamp pairs, and
/// no interaction directive in the vocabulary at all.**
///
/// The sheet's label names no key, so nothing here invents one. What M18
/// spells on that theme:
///
/// * the record's only `TRAVELERS` site — block 3, `player` approaching
///   `cargozep1` at 1500 — which lowers to the measured inside/outside
///   predicate;
/// * the five per-clamp blocks 26–30, each pairing `INACTIVE1(name, healthy)`
///   with a `REMOVE_OBJECTIVE_TARGET` of the same name, and the approach
///   block's own add/remove pair (`tiedown01..04` + `zmainclamp` added,
///   `cargozep1` removed);
/// * the actors themselves, declared by the mission archive's own members:
///   the four tiedowns and `zmainclamp` by `targets.zrd` (`zmainclamp` also by
///   `zep_dock.zrd`), `cargozep1` by six;
/// * and the measured **absence** of any docking, pickup, boarding or transfer
///   key, asserted against the whole vocabulary rather than by looking for one
///   name — the interaction family of `docs/contracts/SCRIPT-MISSION.md` has no
///   M18 instance in this record.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m18_b_rescue_interaction_is_the_approach_site_and_the_per_clamp_pairs() {
    let row = census().row(MISSION).unwrap();
    let record = row.record().unwrap();
    let blocks = blocks_of(&control_document());

    assert_eq!(
        match record.key("TRAVELERS").unwrap().disposition() {
            DirectiveDisposition::Measured(measured) => measured.operation,
            other => panic!("TRAVELERS is not measured: {other:?}"),
        },
        cs_content::mission_control::DirectiveOperation::Travelers
    );
    assert_eq!(with(&blocks, "TRAVELERS"), [3]);
    assert_eq!(
        spelled(&blocks, "TRAVELERS"),
        [vec![
            ZrdValue::Text("player".to_owned()),
            ZrdValue::Text("APPROACHING".to_owned()),
            ZrdValue::Text("cargozep1".to_owned()),
            ZrdValue::Float(1500.0),
            ZrdValue::Int(1),
        ]],
        "the approach site is record data: subject, polarity, anchor, radius"
    );
    let attempt = row.lowering_attempt().unwrap();
    let raw = attempt.raw_program().expect("the program assembled");
    assert_eq!(
        raw.objectives[2].condition,
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 2 },
            Condition::Travelers {
                subject: vec!["player".to_owned()],
                anchor: TravelersAnchor::Object(vec!["cargozep1".to_owned()]),
                radius: 1500.0,
                approaching: true,
            },
        ]),
        "block 3 lowers the approach predicate it spelled"
    );

    // The approach block's own target edits, as spelled.
    let texts_of = |directive: &Directive| -> Vec<String> {
        let mut texts = Vec::new();
        for value in directive.args.iter().flatten() {
            walk_texts(value, &mut texts);
        }
        texts
    };
    let approach = block(&blocks, 3);
    let added = approach
        .iter()
        .find(|directive| directive.key == "ADD_OBJECTIVE_TARGET")
        .expect("the approach block adds the clamps");
    assert_eq!(
        texts_of(added),
        [
            "tiedown01",
            "healthy",
            "tiedown02",
            "healthy",
            "tiedown03",
            "healthy",
            "tiedown04",
            "healthy",
            "zmainclamp",
        ],
        "the four tiedowns and the main clamp become targets"
    );
    let removed = approach
        .iter()
        .find(|directive| directive.key == "REMOVE_OBJECTIVE_TARGET")
        .expect("the approach block removes the cargo zeppelin");
    assert_eq!(texts_of(removed), ["cargozep1"]);

    // The five per-clamp pairs: one `INACTIVE1` and one removal each, the
    // main clamp spelled without its `healthy` leaf in the removal.
    let names = [
        "tiedown01",
        "tiedown02",
        "tiedown03",
        "tiedown04",
        "zmainclamp",
    ];
    for (offset, name) in names.into_iter().enumerate() {
        let number = 26 + offset as u32;
        let directives = block(&blocks, number);
        assert_eq!(
            directives
                .iter()
                .map(|directive| directive.key.as_str())
                .collect::<Vec<_>>(),
            ["BEGIN_DORMANT", "INACTIVE1", "REMOVE_OBJECTIVE_TARGET"],
            "block {number} pairs one predicate with one removal"
        );
        assert_eq!(
            directives[1].args.as_deref(),
            Some(
                [
                    ZrdValue::Text(name.to_owned()),
                    ZrdValue::Text("healthy".to_owned())
                ]
                .as_slice()
            ),
            "block {number} watches {name}/healthy"
        );
        assert!(
            texts_of(&directives[2]).contains(&name.to_owned()),
            "block {number} removes {name} from the target set"
        );
    }

    // Where the actors are declared, by the mission's own archive.
    let members: Vec<String> = census()
        .row(MISSION)
        .unwrap()
        .members
        .iter()
        .map(|member| member.name.clone())
        .collect();
    let declaring = |name: &str| -> Vec<String> {
        members
            .iter()
            .filter(|member| {
                member.as_str() != CONTROL_MEMBER
                    && member_texts(CONTAINER, member)
                        .iter()
                        .any(|text| text == name)
            })
            .cloned()
            .collect()
    };
    for tiedown in ["tiedown01", "tiedown02", "tiedown03", "tiedown04"] {
        assert_eq!(
            declaring(tiedown),
            ["targets.zrd".to_owned()],
            "{tiedown} is declared by exactly one member of the mission archive"
        );
    }
    assert_eq!(
        declaring("zmainclamp"),
        ["targets.zrd".to_owned(), "zep_dock.zrd".to_owned()]
    );
    assert_eq!(
        declaring("cargozep1"),
        [
            "aiv.zrd",
            "egen.zrd",
            "targets.zrd",
            "cghookup.zrd",
            "zep_dock.zrd",
            "zepstate.zrd"
        ]
        .map(str::to_owned),
        "the cargo zeppelin is declared by six members of the archive"
    );

    // The interaction family has no M18 instance: no key of the exact
    // vocabulary docks, picks up, boards or transfers anything.
    let interaction_keys: Vec<&str> = record
        .keys()
        .iter()
        .map(|key| key.key.as_str())
        .filter(|key| {
            let upper = key.to_ascii_uppercase();
            ["DOCK", "PICKUP", "BOARD", "TRANSFER", "RESCUE", "HOSTAGE"]
                .iter()
                .any(|token| upper.contains(token))
        })
        .collect();
    assert!(
        interaction_keys.is_empty(),
        "M18's vocabulary spells no interaction directive: {interaction_keys:?}"
    );
}

// ---------------------------------------------------------------------------
// Retail: the graph the two outcomes sit in
// ---------------------------------------------------------------------------

/// **Both terminal latches are dormant with no timed wake, and every block
/// address the record spells is a block of this record.**
///
/// `INSTANTLOSS` sits in block 11 and `INSTANTWIN` in block 25; no other block
/// ends the mission, and neither can fire on its own clock. The address walk
/// covers 65 spelled integers — 26 wake, 18 kill, 19 nap, 2 gates — and every
/// one lies in `1..=52`. Block 9 spells `52`, the block count itself (an index
/// reading would report it out of range) and nothing spells `0`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m18_b_the_terminal_blocks_are_gated_and_every_address_is_in_range() {
    let blocks = blocks_of(&control_document());
    let outcomes: Vec<u32> = blocks
        .iter()
        .filter(|(_, directives)| directives.iter().any(|d| d.key.starts_with("INSTANT")))
        .map(|(number, _)| *number)
        .collect();
    assert_eq!(outcomes, [11, 25], "no other block ends the mission");
    for (number, outcome) in [(11, "INSTANTLOSS"), (25, "INSTANTWIN")] {
        let directives = block(&blocks, number);
        assert!(
            directives
                .iter()
                .any(|d| d.key == outcome && d.args.is_none()),
            "block {number} spells {outcome} bare"
        );
        assert_eq!(
            directives
                .iter()
                .find(|d| d.key == "BEGIN_DORMANT")
                .and_then(|d| d.args.as_deref()),
            Some(&[ZrdValue::Float(-1.0)][..]),
            "block {number} never wakes on its own clock"
        );
    }

    // The addresses, walked with the measured child rules.
    let mut edges: BTreeMap<&str, u32> = BTreeMap::new();
    let mut out_of_range = Vec::new();
    let mut highest = 0i64;
    let mut spelled_addresses: Vec<(u32, String, Vec<i64>)> = Vec::new();
    for (number, directives) in &blocks {
        for directive in directives {
            if !addresses_blocks(directive) {
                continue;
            }
            spelled_addresses.push((*number, directive.key.clone(), addresses(directive)));
            for address in addresses(directive) {
                *edges.entry(directive.key.as_str()).or_default() += 1;
                highest = highest.max(address);
                if !(1..=i64::from(BLOCKS)).contains(&address) {
                    out_of_range.push((*number, directive.key.clone(), address));
                }
            }
        }
    }
    assert_eq!(
        edges,
        BTreeMap::from([
            ("KILL_OBJECTIVE_WHEN_I_COMPLETE", 18),
            ("NAP_OBJECTIVE_WHEN_I_COMPLETE", 19),
            ("TICK_DEPENDS_ON_OBJ", 2),
            ("WAKE_OBJECTIVE_WHEN_I_COMPLETE", 26),
        ]),
        "the address walk visits every spelled address: 26 wake, 18 kill, 19 nap, 2 gates"
    );
    assert!(
        out_of_range.is_empty(),
        "every address is a block of this record: {out_of_range:?}"
    );
    assert_eq!(
        highest,
        i64::from(BLOCKS),
        "block 9 spells 52 — the block count itself"
    );
    assert!(
        !spelled_addresses
            .iter()
            .any(|(_, _, addresses)| addresses.contains(&0)),
        "nothing spells 0, which an index reading would require"
    );

    // Who may fire the two latches.
    let naming = |target: i64| -> Vec<(u32, String, Vec<i64>)> {
        spelled_addresses
            .iter()
            .filter(|(_, _, addresses)| addresses.contains(&target))
            .cloned()
            .collect()
    };
    assert_eq!(
        naming(11),
        [
            (17, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![11]),
            (35, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![11]),
        ],
        "the failure latch has exactly two completion edges, both 20-second naps"
    );
    assert_eq!(
        naming(25),
        [(
            24,
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE".to_owned(),
            vec![25, 43]
        )],
        "the success latch has exactly one completion edge: block 24's wake"
    );
    assert_eq!(
        naming(52),
        [
            (9, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![52]),
            (
                12,
                "KILL_OBJECTIVE_WHEN_I_COMPLETE".to_owned(),
                vec![8, 9, 10, 35, 51, 52]
            ),
            (
                50,
                "KILL_OBJECTIVE_WHEN_I_COMPLETE".to_owned(),
                vec![8, 9, 10, 35, 51, 52]
            ),
        ],
        "the discriminating address: the last block, in range only under the measured rule —          block 9 naps it and blocks 12 and 50 kill it"
    );

    // The two naps into the failure latch, spelled.
    for number in [17u32, 35] {
        let nap = block(&blocks, number)
            .iter()
            .find(|directive| directive.key == "NAP_OBJECTIVE_WHEN_I_COMPLETE")
            .expect("the block naps the failure latch");
        assert_eq!(
            nap.args,
            Some(vec![ZrdValue::Int(11), ZrdValue::Float(20.0)]),
            "block {number} enters the failure latch on its own 20-second delay"
        );
    }
}

/// **The two outcomes have disjoint prerequisites; twenty-four blocks are
/// named by neither.**
///
/// "Mandatory" is stated as narrowly as the data supports: the blocks that
/// must be *entered* before a latch can be entered — every wake or nap that
/// leads to it, plus the dependency its gate declares. Kill edges are
/// deliberately excluded (killing a block does not enter it), and the pin
/// below is that block 35's only predecessors are block 10's nap even though
/// blocks 12 and 50 both kill it.
///
/// * failure (block 11): seven blocks — the gasbag chain `{14, 15, 16, 17}` of
///   one zeppelin, the `{10, 35}` chain of the other, and the latch itself;
/// * success (block 25): twenty-one blocks, the clamp-release chain through
///   `{2, 3, 4, 5, 6, 7, 12, …}` and the two dependency-gated blocks 44 and 45;
/// * the two sets are disjoint, and the remaining 24 blocks (both latches
///   included) are named by no prerequisite edge of either.
///
/// Whether any of those 24 is an *optional reward or stunt branch* is **not**
/// measured: M18-A binds no stunt or reward ids, so nothing here calls one
/// optional in that sense.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m18_b_the_two_outcomes_have_disjoint_prerequisites_and_the_rest_is_not_mandatory() {
    let blocks = blocks_of(&control_document());
    let prerequisites = prerequisites(&blocks);

    // Kills are not prerequisites: blocks 12 and 50 both kill block 35, but
    // block 35's only predecessor is block 10's nap.
    assert_eq!(prerequisites.get(&35), Some(&vec![10]));
    assert_eq!(prerequisites.get(&11), Some(&vec![17, 35]));
    assert_eq!(prerequisites.get(&25), Some(&vec![24]));
    assert!(
        !prerequisites.contains_key(&8),
        "block 8 is awake at the start and nothing enters it"
    );

    let win = closure(&prerequisites, 25);
    assert_eq!(
        win,
        [
            2, 3, 4, 5, 6, 7, 12, 13, 18, 19, 20, 21, 22, 23, 24, 25, 33, 34, 44, 45, 46
        ],
        "the blocks that must be entered before the success latch can be entered"
    );
    let loss = closure(&prerequisites, 11);
    assert_eq!(
        loss,
        [10, 11, 14, 15, 16, 17, 35],
        "the two gasbag chains are the failure latch's own closure"
    );
    assert!(
        win.iter().all(|number| !loss.contains(number)),
        "the two latches have disjoint prerequisites: {win:?} vs {loss:?}"
    );

    let outside: Vec<u32> = (1..=BLOCKS)
        .filter(|number| !win.contains(number) && !loss.contains(number))
        .collect();
    assert_eq!(
        outside,
        [
            1, 8, 9, 26, 27, 28, 29, 30, 31, 32, 36, 37, 38, 39, 40, 41, 42, 43, 47, 48, 49, 50,
            51, 52
        ],
        "twenty-four blocks are named by no prerequisite edge of either latch"
    );

    // The condition shapes, as the program lowered them: 22 blocks complete on
    // wake alone, 30 spell an evaluator or a gate.
    let attempt = census().row(MISSION).unwrap().lowering_attempt().unwrap();
    let raw = attempt.raw_program().expect("the program assembled");
    let mut wake_only = Vec::new();
    let mut evaluated = Vec::new();
    for index in 0..raw.objectives.len() {
        match &raw.objectives[index].condition {
            Condition::ObjectiveAwake { index: own } => {
                assert_eq!(*own, index as u32, "the block's own index");
                wake_only.push(index as u32 + 1);
            }
            Condition::All(items) => {
                assert_eq!(
                    items.first(),
                    Some(&Condition::ObjectiveAwake {
                        index: index as u32
                    }),
                    "every evaluated block is still gated on its own wake state"
                );
                evaluated.push(index as u32 + 1);
            }
            other => panic!("block {} lowered {other:?}", index + 1),
        }
    }
    assert_eq!(
        wake_only,
        [
            1, 11, 13, 17, 18, 20, 31, 32, 33, 35, 37, 38, 39, 40, 41, 42, 43, 47, 48, 49, 51, 52
        ],
        "22 blocks — including the failure latch — complete on wake alone"
    );
    assert_eq!(
        evaluated,
        [
            2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 14, 15, 16, 19, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
            34, 36, 44, 45, 46, 50
        ],
        "30 blocks spell an evaluator or a dependency gate"
    );
}

// ---------------------------------------------------------------------------
// Retail: what the record names, and what ships it
// ---------------------------------------------------------------------------

/// **Every text the record spells is declared outside the control member,
/// except six measured ones.**
///
/// The record's sites spell [`NAMED_TEXTS`] distinct texts — actors, target
/// chains, animation names, sound groups, keywords, message operands. This asks
/// the same question of every one of them in a single whole-installation walk,
/// so a name that stops resolving anywhere fails here rather than hiding behind
/// a sample table. The six exceptions are the five `MSG_…` operands of the
/// `IDENTITY` sites and one sound group; the next test measures each of them
/// byte for byte.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m18_b_every_text_the_record_spells_is_declared_outside_the_control_member_but_six() {
    let blocks = blocks_of(&control_document());
    let mut spelled: Vec<String> = Vec::new();
    for (_, directives) in &blocks {
        for directive in directives {
            for value in directive.args.iter().flatten() {
                walk_texts(value, &mut spelled);
            }
        }
    }
    let named: BTreeSet<String> = spelled.into_iter().collect();
    assert_eq!(
        named.len(),
        NAMED_TEXTS,
        "the record spells this many distinct texts, so the check below covers all of them"
    );
    // The sample the next test measures in depth is really spelled here.
    for sample in [
        "cargozep1",
        "tiedown01",
        "zmainclamp",
        "bhatbrigand_1",
        "bhatwarhawk_14",
        "M3Cargo",
        "hooked_to_klondike",
        "snd_c4-RM-m3_BlackSwan_27",
        "MSG_BRF_RMM3_OBJ5",
        "MSG_OBJ_DEFEND",
    ] {
        assert!(
            named.contains(sample),
            "the record no longer spells {sample:?}, so the table below is stale"
        );
    }

    let declared_elsewhere = installation_texts_outside_control();
    let record_only: Vec<&str> = named
        .iter()
        .filter(|name| !declared_elsewhere.contains(name.as_str()))
        .map(String::as_str)
        .collect();
    assert_eq!(
        record_only,
        [
            "MSG_BRF_RMM3_OBJ1",
            "MSG_BRF_RMM3_OBJ2",
            "MSG_BRF_RMM3_OBJ3",
            "MSG_BRF_RMM3_OBJ4",
            "MSG_BRF_RMM3_OBJ5",
            "snd_c4-RM-m3_BlackSwan_27",
        ],
        "every text the record spells has a declaration outside M18's control member, except \
         these six"
    );

    // Two halves of one `SET_AI_NET` site, as a sample of the resolving names:
    // the node operand is declared by the chapter world's node index, the four
    // actors by the mission's own `aiv.zrd`.
    let world = member_texts("ZBD/C4/zrdr.zbd", "neindex.zrd");
    assert!(
        world.iter().any(|text| text == "M3Cargo"),
        "the chapter world's node index declares M3Cargo: {} entries",
        world.len()
    );
    let archive_aiv = member_texts(CONTAINER, "aiv.zrd");
    for actor in [
        "bhatbrigand_1",
        "bhatbrigand_2",
        "bhatbrigand_3",
        "bhatbrigand_4",
    ] {
        assert!(
            archive_aiv.iter().any(|text| text == actor),
            "the mission's own aiv member declares {actor}"
        );
    }
}

/// **The one measured gap: a sound group no shipped record declares.**
///
/// `snd_c4-RM-m3_BlackSwan_27` is spelled twice — the `STOP_QUEUED_SOUNDS`
/// sites of blocks 12 and 50 — and, decoded member by member over the whole
/// installation, its only declaration is the site that spells it. A raw byte
/// scan of all 228 files agrees: the bytes occur in **one** file,
/// `ZBD/C4/M03/zrdr.zbd` itself, while its sibling
/// `snd_c4-RM-m3_BlackSwan_28` occurs in the shared sounds container as well.
/// The five `MSG_…` operands behave differently: their bytes are carried by
/// `strings.dll`, the shipped message table, so they are declared by a
/// resource that is not `.zrd` data.
///
/// Nothing here says what the original does with a sound name no shipped file
/// declares (an unreachable stop, a name resolved by a path this build has not
/// measured, or a table entry this build does not decode); that is recorded as
/// unknown, not guessed, and filed as a follow-up in the findings.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m18_b_the_sound_group_no_shipped_record_declares_is_the_one_blocks_12_and_50_stop() {
    let blocks = blocks_of(&control_document());
    let stopping: Vec<(u32, Vec<String>)> = blocks
        .iter()
        .filter(|(_, directives)| {
            directives
                .iter()
                .any(|directive| directive.key == "STOP_QUEUED_SOUNDS")
        })
        .map(|(number, directives)| {
            let names = directives
                .iter()
                .filter(|directive| directive.key == "STOP_QUEUED_SOUNDS")
                .flat_map(|directive| {
                    directive
                        .args
                        .iter()
                        .flatten()
                        .filter_map(|value| match value {
                            ZrdValue::Text(text) => Some(text.clone()),
                            _ => None,
                        })
                })
                .collect();
            (*number, names)
        })
        .collect();
    assert_eq!(
        stopping,
        vec![
            (
                12u32,
                vec![
                    "snd_c4-RM-m3_BlackSwan_27".to_owned(),
                    "snd_c4-RM-m3_BlackSwan_28".to_owned()
                ]
            ),
            (
                50u32,
                vec![
                    "snd_c4-RM-m3_BlackSwan_27".to_owned(),
                    "snd_c4-RM-m3_BlackSwan_28".to_owned()
                ]
            ),
        ],
        "the two stop sites spell the same pair"
    );

    // Decoded `.zrd` declarations across the whole installation: the missing
    // name is declared only by the site that spells it; its sibling is also
    // carried by the shared sounds table.
    let names = [
        "snd_c4-RM-m3_BlackSwan_27",
        "snd_c4-RM-m3_BlackSwan_28",
        "MSG_BRF_RMM3_OBJ1",
        "MSG_BRF_RMM3_OBJ5",
        "MSG_OBJ_DEFEND",
    ];
    let declarations = declarations_of(&names);
    assert_eq!(
        declarations["snd_c4-RM-m3_BlackSwan_27"],
        [(CONTAINER.to_owned(), CONTROL_MEMBER.to_owned())],
        "the missing sound group is declared only by the record that stops it"
    );
    assert_eq!(
        declarations["snd_c4-RM-m3_BlackSwan_28"],
        [
            (CONTAINER.to_owned(), CONTROL_MEMBER.to_owned()),
            ("ZBD/zrdr.zbd".to_owned(), "sounds.zrd".to_owned()),
        ],
        "its sibling is declared by the shared sounds table as well"
    );
    for message in ["MSG_BRF_RMM3_OBJ1", "MSG_BRF_RMM3_OBJ5"] {
        assert_eq!(
            declarations[message],
            [(CONTAINER.to_owned(), CONTROL_MEMBER.to_owned())],
            "{message} is declared by no .zrd record but its own site"
        );
    }
    assert!(
        declarations["MSG_OBJ_DEFEND"].len() > 20,
        "MSG_OBJ_DEFEND is a real, widely declared label, so the six exceptions are not a \
         class the walk invents: {} sites",
        declarations["MSG_OBJ_DEFEND"].len()
    );

    // A raw byte scan of every file of the installation: the missing name is
    // carried by exactly one file, its sibling by two, and the message
    // operands by the shipped message table.
    let needles: Vec<&[u8]> = vec![
        b"snd_c4-RM-m3_BlackSwan_27",
        b"snd_c4-RM-m3_BlackSwan_28",
        b"MSG_BRF_RMM3_OBJ1",
    ];
    let found = files_containing(&needles);
    let files = |needle: &[u8]| -> Vec<String> {
        found
            .get(needle)
            .unwrap_or_else(|| panic!("the scan recorded no entry for {needle:?}"))
            .clone()
    };
    assert_eq!(
        files(b"snd_c4-RM-m3_BlackSwan_27"),
        [CONTAINER.to_owned()],
        "the bytes occur in exactly one file of the installation"
    );
    assert_eq!(
        files(b"snd_c4-RM-m3_BlackSwan_28"),
        [CONTAINER.to_owned(), "ZBD/zrdr.zbd".to_owned()],
        "its sibling is carried by the shared sounds container too"
    );
    assert_eq!(
        files(b"MSG_BRF_RMM3_OBJ1"),
        [CONTAINER.to_owned(), "strings.dll".to_owned()],
        "the message operand is carried by the shipped message table"
    );
}

/// **The production animation binding reads M18's own scope.**
///
/// `bind_mission_animation` is the production reader that resolves one mission
/// scope's startup animations to the `.zrd` member declaring them and the
/// carrier record storing them. For `zbd/c4/m03` it reads the mission's own
/// archive plus the chapter's and the shared root, both carriers of the
/// mission directory, and ten startup rows: seven playable, three refused with
/// their own reasons (one ambiguous declaration, one name no carrier record
/// carries, one no member declares). The refusal arms are what this stage pins:
/// a scope whose animations are half-resolved must say so instead of starting
/// them.
///
/// The one name that appears in both worlds is `deactivate_cghookup_node`: a
/// `NEW_GAME_START` row here and block 12's `ANIM_STATE` operand in the
/// control record, declared by `startanims.zrd` and `cghookup.zrd` of the same
/// archive. Whether the original binds the control record's animation names
/// through this reader is **not** asserted here (M01's data measured that
/// join); it is recorded as unknown in the findings.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m18_b_the_production_animation_binding_reads_m18s_own_scope() {
    let binding = bind_mission_animation(&game_dir(), MISSION)
        .expect("M18's animation scope binds through the production reader");
    assert_eq!(binding.scope(), MISSION);
    assert_eq!(binding.group(), "c4");
    assert_eq!(binding.world_container(), "zbd/c4/gamez.zbd");
    assert_eq!(
        binding.archives(),
        ["zbd/c4/m03/zrdr.zbd", "zbd/c4/zrdr.zbd", "zbd/zrdr.zbd"],
        "the mission's own archive, then its world group, then the shared root"
    );

    let carriers: Vec<(&str, &str, usize, u16, usize)> = binding
        .carriers()
        .iter()
        .map(|carrier| {
            (
                carrier.container_key.as_str(),
                match format!("{:?}", carrier.kind).as_str() {
                    "Mission" => "mis_anim",
                    "Camera" => "cam_anim",
                    other => panic!("unknown carrier kind {other}"),
                },
                carrier.record_count,
                carrier.declared_record_count,
                carrier.blockers.len(),
            )
        })
        .collect();
    assert_eq!(
        carriers,
        [
            ("zbd/c4/m03/mis_anim.zbd", "mis_anim", 355, 355, 0),
            ("zbd/c4/cam_anim.zbd", "cam_anim", 380, 380, 0),
        ],
        "both carriers read completely, with no blocker"
    );

    let rows: Vec<(&str, &str, bool)> = binding
        .startup()
        .iter()
        .map(|row| (row.event(), row.identity(), row.is_playable()))
        .collect();
    assert_eq!(
        rows,
        [
            ("NEW_GAME_START", "player_setup", true),
            ("NEW_GAME_START", "deactivate_cghookup_node", true),
            ("NEW_GAME_START", "cgzepstate", true),
            ("NEW_GAME_START", "cg1zep_engines_stop", false),
            ("NEW_GAME_START", "pzep_engines_stop", true),
            ("NEW_GAME_START", "deactivate_bmhookup_node", false),
            ("NEW_GAME_START", "cargozep1_close_doors", false),
            ("NEW_GAME_START", "call_add_boothe", true),
            ("NEW_GAME_START", "place_tntboxes", true),
            ("LOAD_GAME_START", "player_setup", true),
        ],
        "M18's startup table, in stored order"
    );
    assert_eq!((binding.playable_count(), binding.refused_count()), (7, 3));
    assert_eq!(binding.startup_of("NEW_GAME_START").len(), 9);
    assert_eq!(binding.startup_of("LOAD_GAME_START").len(), 1);
    assert_eq!(binding.placements().len(), 2);

    // The refusals say which half failed, and never nothing.
    let refusal_of = |identity: &str| -> String {
        binding
            .startup()
            .iter()
            .find(|row| row.identity() == identity)
            .unwrap_or_else(|| panic!("the startup table has no {identity}"))
            .refusals()
            .iter()
            .map(|refusal| format!("{refusal:?}"))
            .collect::<Vec<_>>()
            .join("; ")
    };
    assert!(
        refusal_of("cargozep1_close_doors").contains("Undeclared"),
        "{}",
        refusal_of("cargozep1_close_doors")
    );
    assert!(
        refusal_of("deactivate_bmhookup_node").contains("NoRecord"),
        "{}",
        refusal_of("deactivate_bmhookup_node")
    );
    assert!(
        refusal_of("cg1zep_engines_stop").contains("AmbiguousDeclaration"),
        "{}",
        refusal_of("cg1zep_engines_stop")
    );

    // The name both readers spell: a startup row and a control-record operand.
    let blocks = blocks_of(&control_document());
    let mut anim_state: Vec<String> = Vec::new();
    for directive in block(&blocks, 12)
        .iter()
        .filter(|directive| directive.key == "ANIM_STATE")
    {
        for value in directive.args.iter().flatten() {
            walk_texts(value, &mut anim_state);
        }
    }
    anim_state.sort();
    anim_state.dedup();
    assert!(
        anim_state.contains(&"deactivate_cghookup_node".to_owned()),
        "block 12 spells the same animation the startup table starts: {anim_state:?}"
    );
    assert!(
        binding
            .startup()
            .iter()
            .any(|row| row.identity() == "deactivate_cghookup_node"),
        "and the startup table carries it too"
    );

    // Where the control record's own animation names are declared. Four of the
    // six resolve inside M18's reader archive…
    let members: Vec<String> = census()
        .row(MISSION)
        .unwrap()
        .members
        .iter()
        .map(|member| member.name.clone())
        .collect();
    for (name, member) in [
        ("dropped_blacke", "blacke_drop.zrd"),
        ("deactivate_cghookup_node", "startanims.zrd"),
        ("too_late_to_hook", "cghookup.zrd"),
        ("call_destroy_the_cargozep", "zep_dock.zrd"),
    ] {
        assert!(
            member_texts(CONTAINER, member)
                .iter()
                .any(|text| text == name),
            "{member} declares {name}"
        );
    }
    // …and two do not: `hooked_to_klondike` (block 25's win condition) and
    // `pzhomebase` (block 25's wake animation) are declared by no other member
    // of the archive. They are declared elsewhere in the installation (the
    // full-coverage walk above checks that), so this is a binding question and
    // not an absent name — recorded as unknown, filed as a follow-up, never
    // guessed.
    for name in ["hooked_to_klondike", "pzhomebase"] {
        let declaring: Vec<&str> = members
            .iter()
            .filter(|member| {
                member.as_str() != CONTROL_MEMBER
                    && member_texts(CONTAINER, member)
                        .iter()
                        .any(|text| text == name)
            })
            .map(String::as_str)
            .collect();
        assert!(
            declaring.is_empty(),
            "{name} is now declared by {declaring:?} of M18's own reader archive, so the \
             follow-up this test records has been answered elsewhere"
        );
    }
}

/// **M18 is a complete census row; the campaign is still not ready.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m18_b_m18_is_a_complete_census_row_and_the_campaign_stays_unready() {
    let census = census();
    assert!(
        census.complete_missions().contains(&MISSION),
        "M18 joins the census's complete rows"
    );
    assert!(
        !census.campaign_ready(),
        "the campaign is still not ready — other missions carry their own gaps"
    );
    assert!(census.measured_rows().any(|row| row.mission() == MISSION));
    let row = census.row(MISSION).unwrap();
    assert!(row.is_measured(), "the program is measured");
    assert!(row.is_complete(), "M18's row is complete");
    assert_eq!(row.container_sha256, CONTAINER_SHA256);
}

// ---------------------------------------------------------------------------
// Synthetic: the predicates and the refusal arms the retail record leans on
// ---------------------------------------------------------------------------

/// One `.zrd` text node.
fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

/// One `.zrd` int node.
fn int(value: u32) -> ZrdValue {
    ZrdValue::Int(value)
}

/// One authored numbered block.
fn block_site(number: u32, directives: Vec<Vec<ZrdValue>>) -> (String, ZrdValue) {
    let mut children = Vec::new();
    for site in directives {
        children.extend(site);
    }
    (format!("OBJECTIVE{number}"), ZrdValue::List(children))
}

/// One authored directive site: the key plus its argument list.
fn site(key: &str, args: Vec<ZrdValue>) -> Vec<ZrdValue> {
    let mut children = vec![text(key)];
    if !args.is_empty() {
        children.push(ZrdValue::List(args));
    }
    children
}

/// A wrapped control record: the measured one-element wrapper around the flat
/// record.
fn control_record(fields: Vec<(String, ZrdValue)>) -> ZrdValue {
    let mut children = Vec::new();
    for (key, value) in fields {
        children.push(text(&key));
        children.push(value);
    }
    ZrdValue::List(vec![ZrdValue::List(children)])
}

/// Lowers an authored record the way the census lowers a retail one.
fn lower(document: &ZrdValue) -> LoweredControlRecord {
    let record = measure_control_record(document);
    lower_control_record(
        ContentId::from_source(ContentKind::Mission, "accept-m18-b").map_err(|e| e.to_string()),
        "accept-m18-b",
        document,
        &record,
    )
}

/// M18's own `TRAVELERS` spelling (block 3) with a chosen subject.
fn travelers(subject: ZrdValue) -> ZrdValue {
    control_record(vec![block_site(
        1,
        vec![
            site("BEGIN_DORMANT", vec![ZrdValue::Float(-1.0)]),
            site(
                "TRAVELERS",
                vec![
                    subject,
                    text("APPROACHING"),
                    text("cargozep1"),
                    ZrdValue::Float(1500.0),
                    int(1),
                ],
            ),
        ],
    )])
}

/// **M18's `TRAVELERS` site lowers as a predicate; the counting-mode spelling
/// refuses.**
///
/// The same key at two subjects, so the refusal is the counting mode and not
/// the spelling: a named subject is the side-effect-free inside/outside test
/// M18 block 3 spells and lowers clean, a numeric `child0` arms a counter write
/// and refuses. This is the failure arm that keeps a "the record lowers" claim
/// from hiding an unmeasured predicate.
#[test]
fn accept_m18_b_m18s_travelers_site_lowers_and_a_counting_mode_refuses() {
    let named = lower(&travelers(text("player")));
    assert_eq!(
        named.attempt().conditions,
        [ConditionOutcome::Lowered],
        "M18's own spelling is a predicate"
    );
    assert_eq!(named.attempt().validation, Some(Vec::new()));
    assert!(
        named
            .attempt()
            .calls
            .iter()
            .all(|call| matches!(call, CallOutcome::Bound)),
        "{:?}",
        named.attempt().calls
    );

    let counting = lower(&travelers(int(2)));
    assert!(
        matches!(
            counting.attempt().conditions.as_slice(),
            [ConditionOutcome::Refused(text)] if text.contains("counting mode")
        ),
        "{:?}",
        counting.attempt().conditions
    );
    assert!(
        counting
            .attempt()
            .validation
            .as_ref()
            .is_some_and(|problems| !problems.is_empty()),
        "validation reports the unsupported condition"
    );
}

/// **A dependency gate lowers into the gated block as a wake conjunct.**
///
/// M18 spells two of these (blocks 24 and 45), so the shape is pinned here on
/// an authored record where both halves are visible: the gated block keeps its
/// own `ObjectiveAwake` and gains the dependency's zero-based index. A gate is
/// never lowered as a timing hint and never dropped, and a record whose only
/// other directive is unmeasured refuses the site by name instead of quietly
/// standing.
#[test]
fn accept_m18_b_a_dependency_gate_lowers_as_the_dependency_wake_conjunct() {
    let gated = control_record(vec![
        block_site(
            1,
            vec![
                site("BEGIN_DORMANT", vec![ZrdValue::Float(-1.0)]),
                site("TICK_DEPENDS_ON_OBJ", vec![int(2)]),
            ],
        ),
        block_site(
            2,
            vec![
                site("BEGIN_DORMANT", vec![ZrdValue::Float(-1.0)]),
                site("INACTIVE1", vec![text("piratezep"), text("healthy")]),
            ],
        ),
    ]);
    let lowered = lower(&gated);
    let raw = lowered.raw_program().expect("the program assembles");
    assert_eq!(
        raw.objectives[0].condition,
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 0 },
            Condition::ObjectiveAwake { index: 1 },
        ]),
        "the gate is the dependency's own wake state, by index child0 − 1"
    );
    assert_eq!(
        raw.objectives[1].condition,
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 1 },
            Condition::InactiveMembers {
                members: vec![vec!["piratezep".to_owned(), "healthy".to_owned()]],
                threshold: 1,
            },
        ]),
        "the dependency block keeps its own predicate"
    );
    assert_eq!(lowered.attempt().validation, Some(Vec::new()));
    assert!(
        lowered
            .attempt()
            .calls
            .iter()
            .all(|call| matches!(call, CallOutcome::Bound)),
        "{:?}",
        lowered.attempt().calls
    );
}

/// **A directive this build measures no effect for is refused, not honoured.**
///
/// M18's vocabulary carries no allegiance-changing key (pinned against the
/// whole 29-key vocabulary in the retail suite); if one were spelled anyway —
/// an authored `SET_FACTION` here — the production measurement reports it
/// unmeasured and the lowering refuses the site by name instead of quietly
/// doing nothing. That is the guard against an implementation "supporting" a
/// transition the shipped data never spells, and against a guessed key ever
/// being treated as measured.
#[test]
fn accept_m18_b_an_ungrounded_allegiance_directive_is_refused_rather_than_honoured() {
    let document = control_record(vec![block_site(
        1,
        vec![
            site("BEGIN_DORMANT", vec![ZrdValue::Float(-1.0)]),
            site(
                "SET_FACTION",
                vec![text("bhatbrigand_1"), text("bhatpeace")],
            ),
        ],
    )]);
    let record = measure_control_record(&document);
    let key = record
        .key("SET_FACTION")
        .expect("the record reports the key it spelled rather than dropping it");
    assert_eq!(key.sites, 1);
    assert!(
        matches!(key.disposition(), DirectiveDisposition::Unmeasured { .. }),
        "an unmeasurable directive stays unmeasured: {:?}",
        key.disposition()
    );
    assert!(
        record
            .unmeasured()
            .iter()
            .any(|(measured, _)| measured.key == "SET_FACTION"),
        "the record's own unmeasured walk carries it"
    );

    let lowered = lower(&document);
    assert_eq!(
        lowered.attempt().mission.as_deref(),
        Ok("mission/accept-m18-b"),
        "the mission identity still derives"
    );
    let refused: Vec<&str> = lowered
        .attempt()
        .calls
        .iter()
        .filter_map(|call| match call {
            CallOutcome::Refused(text) => Some(text.as_str()),
            CallOutcome::Bound => None,
        })
        .collect();
    assert_eq!(
        refused.len(),
        1,
        "exactly the unknown site refuses: {:?}",
        lowered.attempt().calls
    );
    assert!(
        refused[0].contains("unknown host call `SET_FACTION`"),
        "the refusal names the key: {refused:?}"
    );
    assert!(
        lowered.program().is_none(),
        "no program stands, so an unmeasured directive can never reach the engine"
    );
    assert!(
        !record.is_complete(lowered.attempt()),
        "the record does not read as complete"
    );
}
