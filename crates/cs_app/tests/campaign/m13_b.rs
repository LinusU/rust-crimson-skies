//! Acceptance stage M13-B: the mission-specific compatibility gaps of the
//! thirteenth mission (`missions/M13.md`, work order `M13-B`).
//!
//! M13-A bound M13's identities and left the mission program unmeasured. The
//! machinery that measures a control program (the `.zrd` reader, the census,
//! the directive dispositions and the record → `RawProgram` adapter) is shared
//! and was built for M01; this stage runs it over M13's own reader archive and
//! pins what is **different** at M13, together with the three regression
//! priorities the sheet names:
//!
//! * M13's control program is `objectives.zrd` with 57 numbered blocks and 209
//!   directive sites, selected by the content rule and not by its name (one
//!   member is longer than it);
//! * all 22 of its directive keys have a disposition — 2 terminal outcomes and
//!   20 with a measured effect, **none refused** — and the record **lowers
//!   completely**: every site binds, every condition lowers, the program
//!   validates and M13's census row is complete;
//! * **ordered traversal** is the danger-zone course: blocks 17–24 spell the
//!   `dzpath1→dzpath2→dzpath3→dzpath5→dzpath4→dzpath6→dzpath7→dzpath9`
//!   flythrough with an add/remove target hand-off at each step, each block
//!   entering the next only through its own `WAKE_OBJECTIVE_WHEN_I_COMPLETE`
//!   edge (34–37 are the optional timed short-cuts, 38–41 their reciprocal
//!   timed kills), and the ace chain (blocks 6→27 then 28–33) is a run of
//!   `DEDG` group depletions whose only terminal edge is the failure latch;
//! * **allegiance transition** **is** spelled here, unlike the record this
//!   stage once measured: block 3's `SET_AI_TEAM` assigns the six `hafury_*`
//!   flight members teams 2/1/2/2/1/2 and block 3's `SET_AI_NET` points each
//!   of them at the node `M3Aces` (declared by the chapter world's
//!   `neindex.zrd`, not by any member of M13's archive), and
//!   `WAKEUP_ENEMIES` wakes the two `devastator_*` and three `wingman_*`
//!   friendly aircraft;
//! * **optional versus mandatory outcome** is the two latches: `INSTANTWIN`
//!   (block 8, on the zeppelin's `ANIM_STATE`) and `INSTANTLOSS` (block 49,
//!   reached from the six ace depletions), each dormant with no timed wake and
//!   each named by a disjoint prerequisite closure; the 32 blocks outside both
//!   closures are named by neither, and nothing here calls any of them a reward
//!   (M13-A binds no stunt or reward ids).
//!
//! A lowered program is **not** a played mission: no playthrough, difficulty,
//! media or presentation row is covered (that is M13-C, with `human_play`), and
//! the wrong-actor, wrong-session and repeated-event halves of the sheet's
//! priorities are runtime observations that stay unmeasured here. The measured
//! unknowns are written up in
//! `docs/findings/2026-10-10-m13-b-compatibility-gaps.md`.
//!
//! The pins below were re-derived from `ZBD/C2/M03/zrdr.zbd` — the archive
//! `missions/bindings/M13.json` names after the corrected campaign layout
//! (`docs/findings/2026-10-17-m01-archive-decision.md`, re-pinned by Rally
//! #1600). The earlier pins measured `ZBD/C3/M03/zrdr.zbd`, which the corrected
//! layout binds to work order M03.
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`; the synthetic
//! tests run in CI.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_app::control_lowering::{LoweredControlRecord, lower_control_record};
use cs_app::mission_control::{
    RetailControlCensus, read_control_member, survey_mission_control_programs,
};
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
use cs_formats::script_raw::discover_container;
use cs_script::ir::{AnimationState, Condition, MemberName, TravelersAnchor};
use cs_types::content::{ContentId, ContentKind};
use cs_types::install::RelativePath;

use crate::campaign::common::load_inventory;

/// The census row label of the mission (F13-B's mission-scope rule).
const MISSION: &str = "zbd/c2/m03";
/// The reader archive the installation ships for M13 — the program span
/// `missions/bindings/M13.json` cites.
const CONTAINER: &str = "ZBD/C2/M03/zrdr.zbd";
/// SHA-256 of that whole archive, from production discovery.
const CONTAINER_SHA256: &str = "a72c66b14ea74055e3cee09967218971f7e1fc1fcd357d2e505d769953295886";
/// The archive's length in bytes — M13-A's own source span.
const CONTAINER_LENGTH: u64 = 41_769;
/// The member the measured rule chose.
const CONTROL_MEMBER: &str = "objectives.zrd";
/// SHA-256 of the control member's own bytes.
const CONTROL_SHA256: &str = "b0db161f97772ab6f4c4ee8ada8a554169b9cd0412bd88abc4058848d130531e";
/// The control member's first byte inside the archive.
const CONTROL_OFFSET: u64 = 17_212;
/// The control member's length in bytes.
const CONTROL_LENGTH: u64 = 13_980;
/// The chapter-2 world-group archive, which carries the node list the record's
/// one `SET_AI_NET` site points at.
const WORLD_CONTAINER: &str = "ZBD/C2/zrdr.zbd";
/// The numbered blocks of the control member.
const BLOCKS: u32 = 57;
/// The directive sites of the control member.
const SITES: u32 = 209;
/// The distinct directive keys of the control member.
const KEYS: usize = 22;
/// The distinct text nodes the record's directive sites spell — actors, engine
/// and state chains, keywords, sound and message operands. Full-coverage claims
/// below are measured against this count.
const NAMED_TEXTS: usize = 90;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M13-B needs the retail capability; run this suite with \
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

/// The declared discovery title of `M13`, read from the committed inventory.
fn declared_title() -> String {
    let inventory = load_inventory();
    inventory
        .iter()
        .find(|(label, _)| label.as_str() == "M13")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M13 work order")
}

/// The production control-program binding, built once for the whole suite.
fn control_binding() -> &'static MissionControlBinding {
    static BINDING: OnceLock<MissionControlBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        let context = SourceContext::read(&game_dir())
            .expect("production source context reads the installation");
        context
            .control_program(
                MissionLabel::new("M13").expect("M13 is a valid label"),
                &declared_title(),
            )
            .expect("M13's control program binds through the measured rule")
    })
}

/// The M13 mission binding M13-A derives, built once — this stage consumes its
/// identities and adds no second evidence for the join itself.
fn mission_binding() -> &'static SourceBinding {
    static BINDING: OnceLock<SourceBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        let context = SourceContext::read(&game_dir())
            .expect("production source context reads the installation");
        context
            .bind(
                MissionLabel::new("M13").expect("M13 is a valid label"),
                &declared_title(),
            )
            .expect("M13 binds to the original data")
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
        let children = value.as_list().expect("every M13 block is a list");
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
        .expect("the rule finds M13's control member")
        .0
}

/// The integer arguments a directive spells.
fn arguments(directive: &Directive) -> Vec<i64> {
    directive
        .args
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter_map(|value| match value {
            ZrdValue::Int(int) => Some(i64::from(*int)),
            _ => None,
        })
        .collect()
}

/// The cross-objective **block addresses** a directive spells.
///
/// Measured (M02-B-FU3 #802, re-derived by M06-B-FU3 #819): a spelled address is
/// the one-based number of a numbered block and the original's parse decrements
/// it to the record index `address − 1`. A nap contributes only child0 — child1
/// is the delay in seconds after which the target re-wakes, never an address.
fn addresses(directive: &Directive) -> Vec<i64> {
    let mut ints = arguments(directive);
    if directive.key == "NAP_OBJECTIVE_WHEN_I_COMPLETE" {
        ints.truncate(1);
    }
    ints
}

/// Whether a directive addresses other blocks at all.
fn addresses_blocks(directive: &Directive) -> bool {
    directive.key.ends_with("_OBJECTIVE_WHEN_I_COMPLETE") || directive.key == "TICK_DEPENDS_ON_OBJ"
}

/// The blocks of the record that spell a key.
fn with(blocks: &[(u32, Vec<Directive>)], key: &str) -> Vec<u32> {
    blocks
        .iter()
        .filter(|(_, directives)| directives.iter().any(|d| d.key == key))
        .map(|(number, _)| *number)
        .collect()
}

/// The argument lists a key is spelled with, in block order.
fn spelled(blocks: &[(u32, Vec<Directive>)], key: &str) -> Vec<Vec<ZrdValue>> {
    blocks
        .iter()
        .flat_map(|(_, directives)| directives.iter())
        .filter(|d| d.key == key)
        .map(|d| d.args.clone().unwrap_or_default())
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

/// `(predecessor, target)` of every completion edge — a wake or a nap plus
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

/// Every distinct `.zrd` text node of one container's member, for the actor and
/// node resolutions below: names only, never bytes.
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
        let document = decode_zrd(program.bytes()).expect("the member decodes");
        walk_texts(&document, &mut texts);
    }
    texts.sort();
    texts.dedup();
    texts
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

/// Every `(container, member)` of the installation whose decoded texts spell
/// `name` exactly — a whole-installation declaration search, derived by
/// production discovery and the production `.zrd` decoder.
fn declarations_of(name: &str) -> Vec<(String, String)> {
    let found = discover(&game_dir()).expect("production discovery reads the installation");
    let mut hits = Vec::new();
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
            let Ok(document) = decode_zrd(program.bytes()) else {
                continue;
            };
            let mut texts = Vec::new();
            walk_texts(&document, &mut texts);
            if texts.iter().any(|text| text == name) {
                hits.push((spelling.to_owned(), member.to_owned()));
            }
        }
    }
    hits.sort();
    hits.dedup();
    hits
}

/// Every distinct `.zrd` text node the whole installation declares **outside**
/// M13's control member, collected in one pass.
///
/// [`declarations_of`] decodes every reader container once per name, which is
/// right for the one or two names a test asks about. The full-coverage
/// assertion below asks the same question of every one of the
/// [`NAMED_TEXTS`] texts the record spells, so it pays the walk once and
/// skips the site that spells them — that is what keeps "the installation
/// declares this name" and "only this record mentions it" apart.
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
            let Ok(document) = decode_zrd(program.bytes()) else {
                continue;
            };
            walk_texts(&document, &mut texts);
        }
    }
    texts.sort();
    texts.dedup();
    texts.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Retail: what M13's control program is
// ---------------------------------------------------------------------------

/// **The control program is the member that declares the numbered blocks.**
///
/// Of the 13 members of M13's reader archive exactly one declares numbered
/// `OBJECTIVE<N>` blocks: it is the eighth member, and the one member that is
/// longer than it (`aiv.zrd`) declares none, so neither size nor position picks
/// it. The blocks and sites the census measures equal an independent walk of
/// the same document, the archive is the program span M13-A bound, and the
/// production control binding reaches the same member, span and digests through
/// its own walk.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_the_control_program_is_the_member_that_declares_the_blocks() {
    let row = census().row(MISSION).expect("M13 is in the census");
    assert_eq!(row.container, CONTAINER);
    assert_eq!(
        row.container_sha256, CONTAINER_SHA256,
        "the reader archive is the program M13-A bound"
    );
    assert_eq!(row.members.len(), 13);

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
        ["aiv.zrd"],
        "size is not the rule: the longest member is not the control member"
    );

    let record = row.record().expect("M13 has a measured control program");
    assert_eq!((record.blocks(), record.sites()), (BLOCKS, SITES));
    assert_eq!(record.keys().len(), KEYS);

    let (document, member) = read_control_member(&game_dir(), MISSION)
        .expect("the rule finds M13's control member again");
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
        "numbered 1..=57, no gaps"
    );
    let walked: usize = blocks.iter().map(|(_, directives)| directives.len()).sum();
    assert_eq!(walked as u32, record.sites(), "the independent walk agrees");
    assert!(
        record.refusals().is_empty(),
        "every block is a readable list"
    );
    assert!(
        record.unclassified_record_keys().is_empty(),
        "M13 spells no record-level key outside the measured vocabulary"
    );

    // The production binding and the census must not disagree about the
    // mission, the program, the member or the record.
    let bound = control_binding();
    assert_eq!(bound.mission.as_str(), "mission/ch3-m03");
    assert_eq!(bound.program_id.as_str(), "script/c2-m03-zrdr");
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

    // …and both agree with the mission binding M13-A committed: one mission id,
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

/// **Every directive key M13 spells has exactly one disposition, and none is
/// refused.**
///
/// Two keys are terminal outcomes (`INSTANTWIN`, `INSTANTLOSS`, one bare site
/// each), the other 20 have a measured effect, and no key is Unmeasured. The
/// sites are accounted for: the keys' sites sum to the record's, and the sorted
/// vocabulary is exactly the 22 keys the archive spells — so a key that appears,
/// disappears or silently loses its meaning fails here.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_every_directive_m13_spells_has_a_disposition_and_none_is_refused() {
    let record = census().row(MISSION).unwrap().record().unwrap();
    let mut outcomes = BTreeMap::new();
    let mut measured = 0;
    let mut refused = Vec::new();
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
        "M13's vocabulary is fully measured: {refused:?}"
    );
    assert_eq!(measured, 20);
    let sites: u32 = record.keys().iter().map(|key| key.sites).sum();
    assert_eq!(sites, SITES, "no site is dropped from the accounting");

    let mut vocabulary: Vec<&str> = record.keys().iter().map(|key| key.key.as_str()).collect();
    vocabulary.sort_unstable();
    assert_eq!(
        vocabulary,
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
            "INSTANTLOSS",
            "INSTANTWIN",
            "KILL_OBJECTIVE_WHEN_I_COMPLETE",
            "NAP_OBJECTIVE_WHEN_I_COMPLETE",
            "REMOVE_OBJECTIVE_TARGET",
            "SET_AI_NET",
            "SET_AI_TEAM",
            "TRAVELERS",
            "WAKEUP_ENEMIES",
            "WAKEUP_SOUND_GROUP",
            "WAKEUP_ZEP_TURRETS",
            "WAKE_ANIM",
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
        ],
        "the exact 22 keys, so a key that appears or disappears fails here"
    );

    // The two outcome keys answer for their own name and take no arguments.
    for name in ["INSTANTWIN", "INSTANTLOSS"] {
        let key = record.key(name).expect("the outcome key is spelled");
        assert_eq!(key.sites, 1);
        assert_eq!(
            key.agreed_shape(),
            Some(&cs_content::mission_control::DirectiveShape::Bare),
            "{name} is spelled bare, so a text follower is the next key"
        );
    }
}

/// **The sheet's three regression priorities resolve to measured operations,
/// and M13's record does spell an allegiance-changing directive.**
///
/// * **ordered traversal**: the danger-zone course, the depletion and wake/nap
///   completion edges, the kill edges and the animation-state gate — all
///   measured operations;
/// * **allegiance transition**: the record's vocabulary carries
///   `SET_AI_TEAM` (owner/team assignment) and `SET_AI_NET` (node assignment),
///   both measured, plus `WAKEUP_ENEMIES`. Unlike the archive this stage once
///   measured, **the fleet's teams really are reassigned here**, so the
///   negative assertion that "no key can change an allegiance" would be false
///   on this record and is replaced by the positive one;
/// * **optional versus mandatory outcome**: exactly two terminal outcome keys,
///   one site each, closed at the structure level by the disjoint prerequisite
///   closures the next test pins.
///
/// Every key is asserted to resolve to the operation the shared finding
/// measured, so a key that silently lost its meaning fails here.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_the_sheet_priorities_resolve_to_measured_operations_and_the_allegiance_key_is_set_ai_team()
 {
    let record = census().row(MISSION).unwrap().record().unwrap();
    let operation = |key: &str| match record.key(key).unwrap().disposition() {
        DirectiveDisposition::Measured(measured) => measured.operation,
        other => panic!("{key} is not measured: {other:?}"),
    };

    // Ordered traversal.
    for key in [
        "DANGER_ZONES_COMPLETED",
        "DEDG",
        "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
        "NAP_OBJECTIVE_WHEN_I_COMPLETE",
        "KILL_OBJECTIVE_WHEN_I_COMPLETE",
        "ANIM_STATE",
        "BEGIN_DORMANT",
    ] {
        let measured = match record.key(key).unwrap().disposition() {
            DirectiveDisposition::Measured(measured) => measured.operation,
            other => panic!("{key} is not measured: {other:?}"),
        };
        assert!(
            matches!(
                measured,
                DirectiveOperation::DangerZoneFlags
                    | DirectiveOperation::EnemyGroupDepletion
                    | DirectiveOperation::WakeObjectives
                    | DirectiveOperation::NapObjective
                    | DirectiveOperation::KillObjectives
                    | DirectiveOperation::AnimationStates
                    | DirectiveOperation::DormantStart
            ),
            "{key} resolved to {measured:?}"
        );
    }
    assert_eq!(
        operation("DANGER_ZONES_COMPLETED"),
        DirectiveOperation::DangerZoneFlags
    );
    assert_eq!(operation("DEDG"), DirectiveOperation::EnemyGroupDepletion);
    assert_eq!(
        operation("TRAVELERS"),
        DirectiveOperation::Travelers,
        "M13's own predicate (block 48, the player approaching `piratezep`)"
    );

    // Allegiance transition: what this record really spells.
    assert_eq!(operation("SET_AI_TEAM"), DirectiveOperation::AssignTeam);
    assert_eq!(operation("SET_AI_NET"), DirectiveOperation::AssignNet);
    assert_eq!(operation("WAKEUP_ENEMIES"), DirectiveOperation::WakeEnemies);
    assert!(
        record.key("SET_FACTION").is_none(),
        "this record spells no faction directive"
    );

    // Ordered traversal, asserted on the record's own vocabulary: this archive
    // spells no `INACTIVE*` member-count key at all (the record this stage once
    // measured did), so a re-pin that kept those keys would be measuring a
    // different archive.
    assert!(
        record.key("INACTIVE1").is_none(),
        "M13's corrected record spells no INACTIVE member key"
    );
    assert!(
        record.key("TICK_DEPENDS_ON_OBJ").is_none(),
        "M13's corrected record spells no dependency gate"
    );

    // Optional versus mandatory outcome: two keys, one site each.
    for (name, outcome) in [
        ("INSTANTWIN", TerminalOutcome::Succeeded),
        ("INSTANTLOSS", TerminalOutcome::Failed),
    ] {
        let key = record.key(name).expect("the outcome key is spelled");
        assert_eq!(
            key.disposition(),
            DirectiveDisposition::TerminalOutcome { outcome }
        );
        assert_eq!(key.sites, 1);
    }
}

/// **The allegiance transition is spelled by one `SET_AI_TEAM` and one
/// `SET_AI_NET` site, both on the `hafury_*` flight, and the node it points at
/// is declared by the chapter world rather than by M13's own archive.**
///
/// Block 3 assigns teams 2/1/2/2/1/2 to `hafury_1..6` and points every one of
/// them at the node `M3Aces`. Six of the seven distinct names that site spells
/// are declared by the archive's own `aiv.zrd`; `M3Aces` is not declared by any
/// member of M13's archive — it is a node of the chapter-2 world, whose node
/// index `ZBD/C2/zrdr.zbd:neindex.zrd` carries it.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_the_allegiance_transition_is_spelled_by_set_ai_team_and_set_ai_net() {
    let blocks = blocks_of(&control_document());
    assert_eq!(with(&blocks, "SET_AI_TEAM"), [3]);
    assert_eq!(with(&blocks, "SET_AI_NET"), [3]);
    assert_eq!(with(&blocks, "WAKEUP_ENEMIES"), [4, 5]);

    let teams = spelled(&blocks, "SET_AI_TEAM");
    assert_eq!(teams.len(), 1, "one team site");
    let expected_teams: Vec<ZrdValue> = [
        ("hafury_1", 2i64),
        ("hafury_2", 1),
        ("hafury_3", 2),
        ("hafury_4", 2),
        ("hafury_5", 1),
        ("hafury_6", 2),
    ]
    .into_iter()
    .flat_map(|(name, team)| {
        vec![ZrdValue::List(vec![
            ZrdValue::Text(name.to_owned()),
            ZrdValue::Int(u32::try_from(team).expect("team fits")),
        ])]
    })
    .collect();
    assert_eq!(
        teams[0], expected_teams,
        "block 3 assigns the six flight members their teams in spelling order"
    );

    let nets = spelled(&blocks, "SET_AI_NET");
    assert_eq!(nets.len(), 1, "one node site");
    let expected_nets: Vec<ZrdValue> = (1..=6)
        .map(|position| {
            ZrdValue::List(vec![
                ZrdValue::Text(format!("hafury_{position}")),
                ZrdValue::Text("M3Aces".to_owned()),
            ])
        })
        .collect();
    assert_eq!(
        nets[0], expected_nets,
        "block 3 points every flight member at the node M3Aces"
    );

    // The six actors are declared by the archive's own aiv.zrd; the node is
    // declared by the chapter world's node index, not by M13's archive.
    let archive = member_texts(CONTAINER, "aiv.zrd");
    for name in [
        "hafury_1", "hafury_2", "hafury_3", "hafury_4", "hafury_5", "hafury_6",
    ] {
        assert!(
            archive.iter().any(|text| text == name),
            "aiv.zrd declares {name:?}"
        );
    }
    assert!(
        !archive.iter().any(|text| text == "M3Aces"),
        "no member of M13's archive declares the node M3Aces"
    );
    let world = member_texts(WORLD_CONTAINER, "neindex.zrd");
    assert!(
        world.iter().any(|text| text == "M3Aces"),
        "the chapter world's node index declares M3Aces: {world:?}"
    );
}

/// **The danger-zone course and the ace wake graph are record data.**
///
/// * The eight `DANGER_ZONES_COMPLETED` steps are blocks 17–24, spelling
///   `dzpath1, dzpath2, dzpath3, dzpath5, dzpath4, dzpath6, dzpath7, dzpath9`
///   in order, and each step wakes the next only through its own spelled
///   completion edge.
/// * The target hand-off is the guided course: block 17 removes `sghangar` and
///   adds `dz2`, and each following block removes the zone it just completed
///   and adds the next — block 24 removes `dz9` and adds nothing.
/// * The ace chain is a run of `DEDG` group depletions: block 6 depletes group
///   3 and wakes block 25, blocks 25→26→27 walk groups 4, 5, 7, block 27 wakes
///   block 47 and naps block 7, and block 7's completion wakes the success latch
///   (8), the optional music block (16) and the approach predicate (48).
/// * `TICK_DEPENDS_ON_OBJ` is absent from this record: every gate is a spelled
///   wake or nap edge, never a dependency conjunct.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_the_danger_zone_course_and_the_ace_wake_graph_are_record_data() {
    let row = census().row(MISSION).unwrap();
    let blocks = blocks_of(&control_document());

    // The course, step by step.
    assert_eq!(
        with(&blocks, "DANGER_ZONES_COMPLETED"),
        [17, 18, 19, 20, 21, 22, 23, 24],
        "eight danger-zone steps, in block order"
    );
    let zones: Vec<String> = spelled(&blocks, "DANGER_ZONES_COMPLETED")
        .iter()
        .map(|args| match args.as_slice() {
            [ZrdValue::Text(zone)] => zone.clone(),
            other => panic!("a DANGER_ZONES_COMPLETED site spells {other:?}"),
        })
        .collect();
    assert_eq!(
        zones,
        [
            "dzpath1", "dzpath2", "dzpath3", "dzpath5", "dzpath4", "dzpath6", "dzpath7", "dzpath9"
        ],
        "the zones are record data, in the order the course walks them"
    );

    // The add/remove hand-off: each step removes the zone it completed and adds
    // the next; the last removes and adds nothing.
    let names_of = |number: u32, key: &str| -> Vec<String> {
        block(&blocks, number)
            .iter()
            .filter(|d| d.key == key)
            .flat_map(|d| d.args.as_deref().unwrap_or(&[]).iter())
            .filter_map(|value| match value {
                ZrdValue::Text(name) => Some(name.clone()),
                _ => None,
            })
            .collect()
    };
    let removes: Vec<Vec<String>> = (17..=24)
        .map(|number| names_of(number, "REMOVE_OBJECTIVE_TARGET"))
        .collect();
    assert_eq!(
        removes,
        [
            vec!["sghangar"],
            vec!["dz2"],
            vec!["dz3"],
            vec!["dz5"],
            vec!["dz4"],
            vec!["dz6"],
            vec!["dz7"],
            vec!["dz9"],
        ],
        "each step removes the zone it just completed"
    );
    let adds: Vec<Vec<String>> = (17..=24)
        .map(|number| names_of(number, "ADD_OBJECTIVE_TARGET"))
        .collect();
    assert_eq!(
        adds,
        [
            vec!["dz2"],
            vec!["dz3"],
            vec!["dz5"],
            vec!["dz4"],
            vec!["dz6"],
            vec!["dz7"],
            vec!["dz9"],
            Vec::<&str>::new(),
        ],
        "each step adds the next zone; the last step adds nothing"
    );
    assert_eq!(
        with(&blocks, "ADD_OBJECTIVE_TARGET"),
        [7, 17, 18, 19, 20, 21, 22, 23],
        "block 7 adds the first target and the course adds the rest"
    );

    // Every step enters the next only through its own wake edge.
    let edges = |number: u32| -> Vec<i64> {
        block(&blocks, number)
            .iter()
            .filter(|d| d.key == "WAKE_OBJECTIVE_WHEN_I_COMPLETE")
            .flat_map(addresses)
            .collect()
    };
    assert_eq!(
        edges(17),
        [18, 34],
        "the first step also opens the `34` short-cut"
    );
    assert_eq!(edges(18), [19, 35]);
    assert_eq!(edges(19), [20, 36]);
    assert_eq!(edges(20), [21]);
    assert_eq!(edges(21), [22]);
    assert_eq!(edges(22), [23, 37]);
    assert_eq!(edges(23), [24]);
    assert_eq!(edges(24), [42], "the course ends at the race secondary");

    // Block 42 hands the race back to block 2 as a nap, so the course is data.
    let naps = |number: u32| -> Vec<(i64, f32)> {
        block(&blocks, number)
            .iter()
            .filter(|d| d.key == "NAP_OBJECTIVE_WHEN_I_COMPLETE")
            .map(|d| {
                let args = d.args.as_deref().unwrap_or(&[]);
                let target = match args.first() {
                    Some(ZrdValue::Int(number)) => i64::from(*number),
                    other => panic!("a nap spells a target, not {other:?}"),
                };
                let delay = match args.get(1) {
                    Some(ZrdValue::Float(seconds)) => *seconds,
                    other => panic!("a nap spells a delay, not {other:?}"),
                };
                (target, delay)
            })
            .collect()
    };
    assert_eq!(naps(24), [(2, 5.0)], "the finish re-arms the race clock");
    assert_eq!(naps(3), [(4, 90.0)]);
    assert_eq!(naps(2), [(3, 20.0)]);

    // The ace chain: DEDG groups in block order, and the wake that closes it.
    let groups: Vec<i64> = spelled(&blocks, "DEDG")
        .iter()
        .map(|args| match args.as_slice() {
            [ZrdValue::Int(group), ZrdValue::Int(remaining)] => {
                assert_eq!(*remaining, 0, "every M13 depletion needs zero survivors");
                i64::from(*group)
            }
            other => panic!("a DEDG site spells {other:?}"),
        })
        .collect();
    assert_eq!(groups.len(), 17, "seventeen group depletions");
    assert_eq!(edges(6), [25]);
    assert_eq!(edges(25), [26]);
    assert_eq!(edges(26), [27]);
    let mut block_27 = edges(27);
    block_27.sort_unstable();
    assert_eq!(block_27, [47], "the last ace phase wakes its music block");
    assert_eq!(
        naps(27),
        [(7, 20.0)],
        "…and naps the shared completion block 7"
    );
    assert_eq!(edges(4), [6], "the enemy wake opens the ace chain");

    // The lowered program carries the course's own conditions, not a generic
    // completion: every step is gated on its own zone flag.
    let attempt = row.lowering_attempt().unwrap();
    let raw = attempt.raw_program().expect("the program assembled");
    for (offset, zone) in zones.iter().enumerate() {
        let index = u32::try_from(16 + offset).expect("index fits");
        assert_eq!(
            raw.objectives[index as usize].condition,
            Condition::All(vec![
                Condition::ObjectiveAwake { index },
                Condition::DangerZoneFlags {
                    zones: vec![zone.clone()],
                    required: 1,
                },
            ]),
            "block {} lowers the danger-zone condition it spelled",
            index + 1
        );
    }
}

// ---------------------------------------------------------------------------
// Retail: actors, latches and the lowered program
// ---------------------------------------------------------------------------

/// **Every text M13's record spells resolves in the shipped data.**
///
/// The record's directives name actors, zones, nodes, sounds and message
/// operands by string; this walks the archive's other twelve members and says
/// where each name is *declared*. **Every** name resolves somewhere:
///
/// * the flight and enemy actors (`hafury_*`, `habloodhawk_*`, `devastator_*`,
///   `wingman_*`) are declared by the archive's own `aiv.zrd`;
/// * the course zones, the player and the zeppelin are declared by
///   `dzones.zrd`, `map.zrd`, `targets.zrd` and `zeppelins.zrd`;
/// * the sound groups and the node `M3Aces` are declared by the shared root
///   archive `ZBD/zrdr.zbd` and the chapter world's `neindex.zrd` — none of them
///   by M13's archive.
///
/// All but three of the [`NAMED_TEXTS`] distinct texts the record spells are
/// declared somewhere outside M13's control member; the exception set is
/// measured, not chosen — the three `MSG_BRF_HWM3_OBJ*` operands the `IDENTITY`
/// sites spell (blocks 7, 8 and 24), which no other `.zrd` member of the
/// installation spells. `IDENTITY`'s other operand (`PRIMARY`/`SECONDARY`), the
/// key itself and every actor and zone all resolve elsewhere, so the record is
/// covered, not only the twenty-two-name table.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_every_actor_the_record_names_resolves_in_the_shipped_data() {
    let blocks = blocks_of(&control_document());
    // The names a directive spells — walked recursively, because a site's
    // argument list may nest one level deeper (`COMPLETED_STOPPOINT` and
    // `SET_AI_NET` spell their records inside a list of their own).
    let mut named: BTreeSet<String> = BTreeSet::new();
    let mut spelled: Vec<String> = Vec::new();
    for (_, directives) in &blocks {
        for directive in directives {
            for value in directive.args.iter().flatten() {
                walk_texts(value, &mut spelled);
            }
        }
    }
    named.extend(spelled);

    // (name, members of M13's archive, other than the control member, that
    // declare it) — the control member only *spells* a name.
    let expected: [(&str, &[&str]); 22] = [
        ("hafury_1", &["aiv.zrd"]),
        ("hafury_2", &["aiv.zrd"]),
        ("hafury_3", &["aiv.zrd"]),
        ("hafury_4", &["aiv.zrd"]),
        ("hafury_5", &["aiv.zrd"]),
        ("hafury_6", &["aiv.zrd"]),
        ("habloodhawk_1", &["aiv.zrd"]),
        ("habloodhawk_5", &["aiv.zrd"]),
        ("devastator_2", &["aiv.zrd"]),
        ("devastator_3", &["aiv.zrd"]),
        ("wingman_1", &["aiv.zrd"]),
        ("wingman_2", &["aiv.zrd"]),
        ("wingman_3", &["aiv.zrd"]),
        ("player", &["aiv.zrd", "map.zrd", "zeppelins.zrd"]),
        ("piratezep", &["targets.zrd", "zeppelins.zrd"]),
        ("M3Piratezep", &["zeppelins.zrd"]),
        ("M3Aces", &[]),
        ("pzhookpoint", &["targets.zrd"]),
        ("rock_zeppelin", &["targets.zrd"]),
        ("sghangar", &["targets.zrd"]),
        ("dzpath1", &["dzones.zrd"]),
        ("dz9", &["targets.zrd"]),
    ];
    let archive_members: Vec<String> = census()
        .row(MISSION)
        .unwrap()
        .members
        .iter()
        .map(|member| member.name.clone())
        .collect();
    assert_eq!(archive_members.len(), 13);
    assert!(archive_members.iter().any(|name| name == CONTROL_MEMBER));

    // Every other member's texts, read once: the control member only *spells*
    // a name, it never declares one.
    let declarations: Vec<(String, Vec<String>)> = archive_members
        .iter()
        .filter(|member| member.as_str() != CONTROL_MEMBER)
        .map(|member| (member.clone(), member_texts(CONTAINER, member)))
        .collect();
    assert_eq!(declarations.len(), 12);

    for (name, declaring) in expected {
        assert!(
            named.contains(name),
            "the control record really spells {name:?}, so the table below is not vacuous"
        );
        let mut found: Vec<&str> = declarations
            .iter()
            .filter(|(_, texts)| texts.iter().any(|text| text == name))
            .map(|(member, _)| member.as_str())
            .collect();
        found.sort_unstable();
        assert_eq!(
            found,
            declaring.to_vec(),
            "{name:?} is declared by exactly these members (empty means nowhere but the \
             control record itself)"
        );
    }

    // The node operand of the one `SET_AI_NET` site is declared by the chapter
    // world's node index, the second half of the site that no member of M13's
    // archive carries.
    let world = member_texts(WORLD_CONTAINER, "neindex.zrd");
    assert!(
        world.iter().any(|text| text == "M3Aces"),
        "the chapter world's node index declares M3Aces: {world:?}"
    );
    let mission_texts = member_texts(CONTAINER, CONTROL_MEMBER);
    assert!(mission_texts.iter().any(|text| text == "M3Aces"));

    // Full coverage: the table above is a sample, not the whole record, so ask
    // the same question of every text the record spells. All but three are
    // declared somewhere outside the control member that spells them; that
    // exception set is measured over the installation, not chosen — the three
    // `MSG_BRF_HWM3_OBJ*` operands the `IDENTITY` sites of blocks 7, 8 and 24
    // spell.
    assert_eq!(
        named.len(),
        NAMED_TEXTS,
        "the record spells this many distinct texts, so the check below covers all of them"
    );
    let declared_elsewhere = installation_texts_outside_control();
    let record_only: Vec<&str> = named
        .iter()
        .filter(|name| !declared_elsewhere.contains(name.as_str()))
        .map(String::as_str)
        .collect();
    assert_eq!(
        record_only,
        [
            "MSG_BRF_HWM3_OBJ1",
            "MSG_BRF_HWM3_OBJ2",
            "MSG_BRF_HWM3_OBJ3"
        ],
        "every other text the record spells has a declaration outside M13's control member"
    );
}

/// **The node `M3Aces` is declared by the mission and by the chapter world's
/// node index, and by no other archive.**
///
/// The whole installation is decoded member by member through production
/// discovery: every `.zbd` container, every member its index names, every text
/// node its `.zrd` decoder yields. The one non-archive name M13's `SET_AI_NET`
/// site spells resolves exactly twice, which is what makes the second half of
/// that site distinguishable from a record-only operand.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_the_ai_net_node_is_declared_by_the_mission_and_the_chapter_world() {
    assert_eq!(
        declarations_of("M3Aces"),
        [
            (CONTAINER.to_owned(), CONTROL_MEMBER.to_owned()),
            (WORLD_CONTAINER.to_owned(), "neindex.zrd".to_owned()),
        ],
        "the SET_AI_NET node is declared by the mission and by the chapter world's node index"
    );
}

/// **Both terminal latches are gated by their completion edges, and every block
/// address the record spells is a block of this record.**
///
/// `INSTANTWIN` sits in block 8 and `INSTANTLOSS` in block 49; no other block
/// ends the mission. Both start dormant with no timed wake, so neither latch
/// can fire on its own clock: block 8 is named by block 7's wake alone, and
/// block 49 by the six naps the ace depletions (blocks 28–33) all spell, each
/// with the delay the record spells.
///
/// The address walk covers 151 spelled integers — 43 wake, 12 nap, 96 kill —
/// and every one lies in `1..=57`. The addressing itself is not assumed here:
/// M02-B-FU3 (#802) measured the original's parse decrementing every objective
/// address before storing it, so a spelled number **is** the block number, and
/// M06-B-FU3 (#819) reconciled the sibling suites to it. Blocks 28–33 spell
/// `56, 57`, and `57` is the block count itself — the value a zero-based
/// reading would report out of range — and nothing spells `0`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_the_terminal_blocks_are_gated_and_every_address_is_in_range() {
    let blocks = blocks_of(&control_document());
    let outcomes: Vec<u32> = blocks
        .iter()
        .filter(|(_, directives)| directives.iter().any(|d| d.key.starts_with("INSTANT")))
        .map(|(number, _)| *number)
        .collect();
    assert_eq!(outcomes, [8, 49], "no other block ends the mission");
    for (number, outcome) in [(8, "INSTANTWIN"), (49, "INSTANTLOSS")] {
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
            ("KILL_OBJECTIVE_WHEN_I_COMPLETE", 96),
            ("NAP_OBJECTIVE_WHEN_I_COMPLETE", 12),
            ("WAKE_OBJECTIVE_WHEN_I_COMPLETE", 43),
        ]),
        "the address walk visits every spelled address: 43 wake, 12 nap, 96 kill, no gate"
    );
    assert!(
        out_of_range.is_empty(),
        "every address is a block of this record: {out_of_range:?}"
    );
    assert_eq!(
        highest,
        i64::from(BLOCKS),
        "blocks 28–33 spell 57 — the block count itself"
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
        naming(8),
        [(
            7,
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE".to_owned(),
            vec![8, 16, 48]
        )],
        "the success latch has exactly one completion edge: block 7's wake"
    );
    assert_eq!(
        naming(49),
        (28..=33)
            .map(|number| (number, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![49]))
            .collect::<Vec<_>>(),
        "the failure latch is named by the six ace depletions' naps"
    );
    assert_eq!(
        naming(57),
        (28..=33)
            .map(|number| (
                number,
                "WAKE_OBJECTIVE_WHEN_I_COMPLETE".to_owned(),
                vec![56, 57]
            ))
            .chain(std::iter::once((
                42,
                "WAKE_OBJECTIVE_WHEN_I_COMPLETE".to_owned(),
                vec![45, 56, 57]
            )))
            .collect::<Vec<_>>(),
        "the discriminating address: the last block, in range only under the measured rule"
    );
}

/// **The two outcomes have disjoint prerequisites; the other thirty-two blocks
/// are named by neither latch.**
///
/// "Mandatory" here is stated narrowly, as the data supports: the blocks that
/// must be *entered* before a latch can be entered — every block that wakes or
/// naps its way towards it. Kill edges are deliberately excluded (killing a
/// block prevents it; it does not enter it) and no dependency gate exists on
/// this record.
///
/// * success (block 8): 18 blocks — `{2, 3, 4, 6, 7, 8, 17, 18, 19, 20, 21, 22,
///   23, 24, 25, 26, 27, 43}` — the ace chain and the whole danger-zone course,
///   reached from block 2's own 20-second nap;
/// * failure (block 49): the seven-block ace phase `{28, 29, 30, 31, 32, 33,
///   49}`, disjoint from it;
/// * the remaining 32 blocks are named by no prerequisite edge of either latch.
///
/// Whether any of those 32 is an *optional reward or stunt branch* is **not**
/// measured: M13-A binds no stunt or reward ids, so nothing here calls one
/// optional in that sense. What is pinned is that 30 blocks (including both
/// latches) lower to a bare `ObjectiveAwake` — they complete on wake alone —
/// and 27 spell a real evaluator.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_the_two_outcomes_have_disjoint_prerequisites_and_the_rest_is_not_mandatory() {
    let blocks = blocks_of(&control_document());
    let prerequisites = prerequisites(&blocks);

    // Kills are not prerequisites: block 34 kills block 38, but block 38's
    // predecessor is the clock, not block 34.
    assert_eq!(prerequisites.get(&8), Some(&vec![7]));
    assert_eq!(
        prerequisites.get(&49),
        Some(&vec![28, 29, 30, 31, 32, 33]),
        "six naps enter the failure latch"
    );
    assert_eq!(prerequisites.get(&15), Some(&vec![48]));
    assert_eq!(
        prerequisites.get(&2),
        Some(&vec![24, 43]),
        "the race restarts from the finish and from the timeout"
    );
    assert!(
        !prerequisites.contains_key(&38),
        "the timed short-cut is entered by its own clock, not by the block that kills it"
    );

    let win = closure(&prerequisites, 8);
    assert_eq!(
        win,
        [
            2, 3, 4, 6, 7, 8, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 43
        ],
        "the blocks that must be entered before the success latch can be entered"
    );
    let loss = closure(&prerequisites, 49);
    assert_eq!(
        loss,
        [28, 29, 30, 31, 32, 33, 49],
        "the ace phase is its own closure"
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
            1, 5, 9, 10, 11, 12, 13, 14, 15, 16, 34, 35, 36, 37, 38, 39, 40, 41, 42, 44, 45, 46,
            47, 48, 50, 51, 52, 53, 54, 55, 56, 57
        ],
        "thirty-two blocks are named by no prerequisite edge of either latch"
    );

    // The condition shapes: 30 blocks complete on wake alone, 27 spell an
    // evaluator.
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
            1, 2, 3, 4, 5, 15, 16, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 49, 50,
            51, 52, 53, 54, 55, 56, 57
        ],
        "30 blocks — including both latches — complete on wake alone"
    );
    assert_eq!(
        evaluated,
        [
            6, 7, 8, 9, 10, 11, 12, 13, 14, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
            31, 32, 33, 48
        ],
        "27 blocks spell an evaluator"
    );

    // Two of the evaluators, spelled out: the success latch's animation gate and
    // M13's own approach predicate.
    assert_eq!(
        raw.objectives[7].condition,
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 7 },
            Condition::AnimationStates {
                required: 1,
                animations: vec![("hooked_to_klondike".to_owned(), AnimationState::Executed)],
            },
        ]),
        "block 8 completes when the zeppelin's hook-up animation has executed"
    );
    assert_eq!(
        raw.objectives[47].condition,
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 47 },
            Condition::Travelers {
                subject: vec!["player".to_owned()],
                anchor: TravelersAnchor::Object(vec!["piratezep".to_owned()]),
                radius: 1500.0,
                approaching: true,
            },
        ]),
        "block 48 is M13's TRAVELERS site: the player approaching `piratezep` at 1500"
    );
}

/// **Every call binds, every condition lowers and M13's record completes.**
///
/// All 209 sites bind — including the six ten-target kill sites of blocks
/// 28–33, whose long index list the adapter carries as one list argument
/// (#800) — and all 57 block conditions lower. The bound program reaches
/// `MissionProgram::validate` and validates; no lowering requirement is unmet
/// and M13's census row is complete. The census as a whole stays not
/// campaign-ready: other missions carry their own gaps.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_every_call_binds_every_condition_lowers_and_m13s_record_completes() {
    let row = census().row(MISSION).unwrap();
    let record = row.record().unwrap();

    let kill = record
        .key("KILL_OBJECTIVE_WHEN_I_COMPLETE")
        .expect("it is spelled");
    assert_eq!((kill.blocks, kill.sites), (19, 19));
    let mut kill_shapes: Vec<(usize, u32)> = kill
        .shapes
        .iter()
        .map(|(shape, sites)| (shape.arity(), *sites))
        .collect();
    kill_shapes.sort();
    assert_eq!(
        kill_shapes,
        [(1, 9), (5, 1), (6, 1), (7, 1), (9, 1), (10, 6)],
        "six ten-target sites and one three-target site, all bound"
    );
    let travelers = record.key("TRAVELERS").expect("it is spelled");
    assert_eq!((travelers.blocks, travelers.sites), (1, 1));

    let attempt = row.lowering_attempt().unwrap();
    let lowered = attempt.attempt();
    assert_eq!(lowered.mission.as_deref(), Ok("mission/ch3-m03"));
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
        "every site binds, the long kill lists included: {refused_calls:?}"
    );
    assert!(
        lowered.unbound_keys.is_empty(),
        "the registry refused no key: {:?}",
        lowered.unbound_keys
    );
    assert!(
        attempt.program().is_some(),
        "a program stands for all 57 objectives"
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
        "all 57 block conditions lower: {refused_conditions:?}"
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

/// **M13 is a complete census row; the campaign is still not ready.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_m13_is_complete_and_the_campaign_stays_unready() {
    let census = census();
    assert!(
        census.complete_missions().contains(&MISSION),
        "M13 joins the census's complete rows"
    );
    assert!(
        !census.campaign_ready(),
        "the campaign is still not ready — other missions carry their own gaps"
    );
    assert!(census.measured_rows().any(|row| row.mission() == MISSION));
    let row = census.row(MISSION).unwrap();
    assert!(row.is_measured(), "the program is measured");
    assert!(row.is_complete(), "M13's row is complete");
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
        ContentId::from_source(ContentKind::Mission, "accept-m13-b").map_err(|e| e.to_string()),
        "accept-m13-b",
        document,
        &record,
    )
}

/// M13's own `TRAVELERS` spelling (block 48) with a chosen subject.
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
                    text("piratezep"),
                    ZrdValue::Float(1500.0),
                    int(1),
                ],
            ),
        ],
    )])
}

/// **M13's `TRAVELERS` site lowers as a predicate; the counting-mode spelling
/// lowers to the counting condition.**
///
/// The same key at two subjects, so the lowering differs by mode and not by the
/// spelling: a named subject is the side-effect-free inside/outside test M13
/// block 48 spells and lowers clean, a numeric `child0` arms a counter write and
/// refuses. This is the failure arm that keeps a "the record lowers" claim from
/// hiding an unmeasured predicate.
#[test]
fn accept_m13_b_m13s_travelers_site_lowers_and_a_counting_mode_lowers_too() {
    let named = lower(&travelers(text("player")));
    assert_eq!(
        named.attempt().conditions,
        [ConditionOutcome::Lowered],
        "M13's own spelling is a predicate"
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
    assert_eq!(counting.attempt().conditions, [ConditionOutcome::Lowered]);
    assert_eq!(counting.attempt().validation, Some(Vec::new()));
}

/// **A directive this build measures no effect for is refused, not honoured.**
///
/// M13's record spells `SET_AI_TEAM` but no faction directive; if one were
/// spelled anyway — an authored `SET_FACTION` here — the production measurement
/// reports it unmeasured and the lowering refuses the site by name instead of
/// quietly doing nothing. That is the guard against an implementation
/// "supporting" a directive the shipped data never spells, and against a
/// guessed key ever being treated as measured.
#[test]
fn accept_m13_b_an_ungrounded_allegiance_directive_is_refused_rather_than_honoured() {
    let document = control_record(vec![block_site(
        1,
        vec![
            site("BEGIN_DORMANT", vec![ZrdValue::Float(-1.0)]),
            site("SET_FACTION", vec![text("hafury_1"), text("britpeace")]),
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
        Ok("mission/accept-m13-b"),
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

/// **A nap's delay is data, and an `INACTIVE` threshold defaults to the list it
/// spells.**
///
/// The nap arm is M13's own mechanism: block 24 naps block 2 with a 5-second
/// delay and block 27 naps block 7 with 20, so a delay must never be read as an
/// address — a nap at `2` whose delay is `9999` in a two-block record lowers and
/// validates, while an integer in an address position is walked as one by the
/// graph test; the addressing rule itself is pinned once for the reconciled
/// suites by M06-B-FU3 (#819).
///
/// The other half is a shared evaluator M13's corrected record does **not**
/// spell (`INACTIVE*` belongs to the archive this stage once measured): six
/// authored blocks spell `INACTIVE1` with **no** `INACTIVE_COMPLETION_COUNT`, and
/// the lowering writes the block's own member count as the threshold — never
/// `1`, never an invented default. A spelled count wins over that default, and
/// it is carried exactly as spelled: a count of `99` against three members stays
/// `99`, because the record is data and clamping it would be a guess.
#[test]
fn accept_m13_b_a_nap_delay_is_data_and_an_inactive_threshold_defaults_to_its_list() {
    let nap = control_record(vec![
        block_site(
            1,
            vec![
                site("BEGIN_DORMANT", vec![ZrdValue::Float(-1.0)]),
                site(
                    "NAP_OBJECTIVE_WHEN_I_COMPLETE",
                    vec![int(2), ZrdValue::Float(9999.0)],
                ),
            ],
        ),
        block_site(2, vec![site("BEGIN_DORMANT", vec![ZrdValue::Float(-1.0)])]),
    ]);
    let lowered = lower(&nap);
    assert!(
        lowered
            .attempt()
            .calls
            .iter()
            .all(|call| matches!(call, CallOutcome::Bound)),
        "the delay is a delay: {:?}",
        lowered.attempt().calls
    );
    assert_eq!(
        lowered.attempt().validation,
        Some(Vec::new()),
        "a nap delay past the block count is not an address"
    );

    let members: Vec<MemberName> = [
        ["piratezep", "reng11", "healthy"],
        ["piratezep", "reng12", "healthy"],
        ["piratezep", "reng21", "healthy"],
    ]
    .into_iter()
    .map(|chain| chain.into_iter().map(str::to_owned).collect())
    .collect();

    // One authored block with the three engine chains and, unless spelled, no
    // completion count.
    let ladder_step = |threshold: Option<u32>| {
        let mut directives = vec![site("BEGIN_DORMANT", vec![ZrdValue::Float(-1.0)])];
        if let Some(count) = threshold {
            directives.push(site("INACTIVE_COMPLETION_COUNT", vec![int(count)]));
        }
        for (position, chain) in ["reng11", "reng12", "reng21"].into_iter().enumerate() {
            directives.push(site(
                &format!("INACTIVE{}", position + 1),
                vec![text("piratezep"), text(chain), text("healthy")],
            ));
        }
        control_record(vec![block_site(1, directives)])
    };
    let condition = |threshold: u32| {
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 0 },
            Condition::InactiveMembers {
                members: members.clone(),
                threshold,
            },
        ])
    };

    let default = lower(&ladder_step(None));
    assert_eq!(
        default
            .raw_program()
            .expect("the program assembles")
            .objectives[0]
            .condition,
        condition(3),
        "no count spelled: the threshold is the block's own member count"
    );
    let override_ = lower(&ladder_step(Some(1)));
    assert_eq!(
        override_
            .raw_program()
            .expect("the program assembles")
            .objectives[0]
            .condition,
        condition(1),
        "a spelled count wins over the default"
    );
    let unclamped = lower(&ladder_step(Some(99)));
    assert_eq!(
        unclamped
            .raw_program()
            .expect("the program assembles")
            .objectives[0]
            .condition,
        condition(99),
        "the spelled count is carried as spelled — the record is data, never clamped"
    );
}
