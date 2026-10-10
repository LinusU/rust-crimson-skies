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
//! * M13's control program is `objectives.zrd` with 38 numbered blocks and 178
//!   directive sites, selected by the content rule and not by its name (three
//!   members of the archive are longer than it);
//! * all 36 of its directive keys have a disposition — 2 terminal outcomes and
//!   34 with a measured effect, **none refused** — and the record **lowers
//!   completely**: every site binds, every condition lowers, the program
//!   validates and M13's census row is complete;
//! * **ordered traversal** is the damage ladder: blocks 3–6 spell the same
//!   twelve `piratezep` engine chains under rising `INACTIVE_COMPLETION_COUNT`
//!   thresholds (3, 5, 7, 10), each stage entering the next only through that
//!   stage's nap, and the two `TICK_DEPENDS_ON_OBJ` gates (15 → 19, 21 → 29)
//!   lower into the program as the dependency's own `ObjectiveAwake`
//!   conjunct — order is record data, never a timing guess;
//! * **allegiance transition** has **no directive**: no key of M13's vocabulary
//!   changes an owner, a faction, a team or an attitude. What the record does
//!   spell is `SET_AI_NET` (block 25, `britkestrel_1` → node `M3BritAce`) and
//!   `WAKEUP_ENEMIES` (block 26, `britpeace_1..4`), and the stage measures
//!   where each name is declared: every other actor the record names resolves
//!   in a member of the archive, while `britkestrel_1` is declared by
//!   **no archive in the installation** — that gap is recorded, not worked
//!   around;
//! * **optional versus mandatory outcome** is the two latches: `INSTANTLOSS`
//!   (block 7) and `INSTANTWIN` (block 16), each dormant with no timed wake and
//!   each named by exactly one nap, with disjoint prerequisite closures; the 20
//!   blocks outside both closures are named by neither, and nothing here calls
//!   any of them a reward (M13-A binds no stunt or reward ids).
//!
//! A lowered program is **not** a played mission: no playthrough, difficulty,
//! media or presentation row is covered (that is M13-C, with `human_play`), and
//! the wrong-actor, wrong-session and repeated-event halves of the sheet's
//! priorities are runtime observations that stay unmeasured here. The measured
//! unknowns are written up in
//! `docs/findings/2026-10-10-m13-b-compatibility-gaps.md`.
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
use cs_script::ir::{Condition, MemberName, TravelersAnchor};
use cs_types::content::{ContentId, ContentKind};
use cs_types::install::RelativePath;

use crate::common::load_inventory;

/// The census row label of the mission (F13-B's mission-scope rule).
const MISSION: &str = "zbd/c3/m03";
/// The reader archive the installation ships for M13 — the program span
/// `missions/bindings/M13.json` cites.
const CONTAINER: &str = "ZBD/C3/M03/zrdr.zbd";
/// SHA-256 of that whole archive, from production discovery.
const CONTAINER_SHA256: &str = "37a227cce5d6fc99a12481ecb27e5e8b640bfeca1a9cf9c48c630bf9e4fd6efe";
/// The archive's length in bytes — M13-A's own source span.
const CONTAINER_LENGTH: u64 = 118_635;
/// The member the measured rule chose.
const CONTROL_MEMBER: &str = "objectives.zrd";
/// SHA-256 of the control member's own bytes.
const CONTROL_SHA256: &str = "4a691f0cd60928c9a6b350411fae52e96fd16bd692fde3b6cb58b02ce5bee400";
/// The control member's first byte inside the archive.
const CONTROL_OFFSET: u64 = 12_552;
/// The control member's length in bytes.
const CONTROL_LENGTH: u64 = 11_773;
/// The chapter-3 world-group archive, which carries the node list the record's
/// one `SET_AI_NET` site points at.
const WORLD_CONTAINER: &str = "ZBD/C3/zrdr.zbd";
/// The numbered blocks of the control member.
const BLOCKS: u32 = 38;
/// The directive sites of the control member.
const SITES: u32 = 178;
/// The distinct directive keys of the control member.
const KEYS: usize = 36;

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

// ---------------------------------------------------------------------------
// Retail: what M13's control program is
// ---------------------------------------------------------------------------

/// **The control program is the member that declares the numbered blocks.**
///
/// Of the 16 members of M13's reader archive exactly one declares numbered
/// `OBJECTIVE<N>` blocks: it is the eighth member, and the three members that
/// are longer than it (`sub_movement`, `submarine`, `destroy_cargozep`) declare
/// none, so neither size nor position picks it. The blocks and sites the census
/// measures equal an independent walk of the same document, the archive is the
/// program span M13-A bound, and the production control binding reaches the
/// same member, span and digests through its own walk.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_the_control_program_is_the_member_that_declares_the_blocks() {
    let row = census().row(MISSION).expect("M13 is in the census");
    assert_eq!(row.container, CONTAINER);
    assert_eq!(
        row.container_sha256, CONTAINER_SHA256,
        "the reader archive is the program M13-A bound"
    );
    assert_eq!(row.members.len(), 16);

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
        ["sub_movement.zrd", "submarine.zrd", "destroy_cargozep.zrd"],
        "size is not the rule: the three longest members are not the control member"
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
        "numbered 1..=38, no gaps"
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
    assert_eq!(bound.program_id.as_str(), "script/c3-m03-zrdr");
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
/// each), the other 34 have a measured effect, and no key is Unmeasured. The
/// sites are accounted for: the keys' sites sum to the record's, and the sorted
/// vocabulary is exactly the 36 keys the archive spells — so a key that appears,
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
    assert_eq!(measured, 34);
    let sites: u32 = record.keys().iter().map(|key| key.sites).sum();
    assert_eq!(sites, SITES, "no site is dropped from the accounting");

    let mut vocabulary: Vec<&str> = record.keys().iter().map(|key| key.key.as_str()).collect();
    vocabulary.sort_unstable();
    assert_eq!(
        vocabulary,
        [
            "ADD_OBJECTIVE_TARGET",
            "BEGIN_DORMANT",
            "COMPLETED_SOUND_GROUP",
            "COMPLETED_STOPPOINT",
            "DEDG",
            "IDENTITY",
            "INACTIVE1",
            "INACTIVE10",
            "INACTIVE11",
            "INACTIVE12",
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
            "SET_AI_NET",
            "SET_HELP_LABEL",
            "STOP_QUEUED_SOUNDS",
            "TICK_DEPENDS_ON_OBJ",
            "TRAVELERS",
            "WAKEUP_ENEMIES",
            "WAKEUP_GENERATOR",
            "WAKEUP_SOUND_GROUP",
            "WAKEUP_TURRETS",
            "WAKEUP_ZEP_TURRETS",
            "WAKE_ANIM",
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
        ],
        "the exact 36 keys, so a key that appears or disappears fails here"
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

/// **The sheet's three regression priorities resolve to measured operations —
/// and none of M13's keys can change an allegiance.**
///
/// * **ordered traversal**: the twelve `INACTIVE<n>` sites the four ladder
///   blocks spell, their `INACTIVE_COMPLETION_COUNT` thresholds, the wake and
///   nap operations the ladder is built from, and the two dependency gates;
/// * **allegiance transition**: the vocabulary carries **no** directive that
///   changes an owner, a faction, a team or an attitude — asserted against the
///   exact 36-key vocabulary rather than by looking for one name — and what the
///   record spells instead is `SET_AI_NET` (assign a node) and `WAKEUP_ENEMIES`
///   (wake named vehicles), both measured operations;
/// * **optional versus mandatory outcome**: exactly two terminal outcome keys,
///   one site each.
///
/// Every key is asserted to resolve to the operation the shared finding
/// measured, so a key that silently lost its meaning fails here.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_the_sheet_priorities_resolve_to_measured_operations_and_none_changes_allegiance() {
    let record = census().row(MISSION).unwrap().record().unwrap();
    let operation = |key: &str| match record.key(key).unwrap().disposition() {
        DirectiveDisposition::Measured(measured) => measured.operation,
        other => panic!("{key} is not measured: {other:?}"),
    };

    // Ordered traversal.
    for key in [
        "INACTIVE1",
        "INACTIVE2",
        "INACTIVE3",
        "INACTIVE4",
        "INACTIVE5",
        "INACTIVE6",
        "INACTIVE7",
        "INACTIVE8",
        "INACTIVE9",
        "INACTIVE10",
        "INACTIVE11",
        "INACTIVE12",
    ] {
        assert_eq!(operation(key), DirectiveOperation::InactiveMembers, "{key}");
    }
    assert_eq!(
        operation("INACTIVE_COMPLETION_COUNT"),
        DirectiveOperation::InactiveThreshold
    );
    assert_eq!(
        operation("TICK_DEPENDS_ON_OBJ"),
        DirectiveOperation::DependencyGate
    );
    assert_eq!(
        operation("NAP_OBJECTIVE_WHEN_I_COMPLETE"),
        DirectiveOperation::NapObjective
    );
    assert_eq!(
        operation("WAKE_OBJECTIVE_WHEN_I_COMPLETE"),
        DirectiveOperation::WakeObjectives
    );
    assert_eq!(
        operation("KILL_OBJECTIVE_WHEN_I_COMPLETE"),
        DirectiveOperation::KillObjectives
    );
    assert_eq!(operation("BEGIN_DORMANT"), DirectiveOperation::DormantStart);

    // Allegiance transition: what the record spells instead.
    assert_eq!(operation("SET_AI_NET"), DirectiveOperation::AssignNet);
    assert_eq!(operation("WAKEUP_ENEMIES"), DirectiveOperation::WakeEnemies);
    assert_eq!(operation("DEDG"), DirectiveOperation::EnemyGroupDepletion);

    // Optional versus mandatory outcome: two keys, one site each.
    assert_eq!(
        operation("TRAVELERS"),
        DirectiveOperation::Travelers,
        "M13's own predicate (block 30, the player approaching `barracuda`)"
    );
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

    // No directive of this record can change who an actor belongs to.
    let forbidden = [
        "FACTION",
        "OWNER",
        "TEAM",
        "ATTITUDE",
        "ALLEGIANCE",
        "LOYAL",
    ];
    let mut allegiance_keys = Vec::new();
    for key in record.keys() {
        let upper = key.key.to_ascii_uppercase();
        if forbidden.iter().any(|token| upper.contains(token)) {
            allegiance_keys.push(key.key.as_str());
        }
    }
    assert!(
        allegiance_keys.is_empty(),
        "M13's control member spells no allegiance-changing directive: {allegiance_keys:?}"
    );
    assert!(
        record.key("SET_AI_TEAM").is_none(),
        "M13 spells no AI-team assignment (unlike M10)"
    );
}

/// **The damage ladder and the two dependency gates are record data.**
///
/// Blocks 3–6 spell the same twelve `piratezep` engine chains under rising
/// thresholds — 3, 5, 7, 10 of the twelve — and each stage enters the next
/// only through that stage's own nap, on the delay the record spells (15, 15,
/// 10, 25 seconds). Block 6's nap names block 7, the `INSTANTLOSS` latch.
///
/// Block 3 is the only block of the record with **no** dormant marker, so the
/// ladder starts with the mission; blocks 4, 5, 6 and both latches spell
/// `BEGIN_DORMANT -1` (no timed self-wake), and only blocks 1 and 2 arm a clock
/// (2 s and 13.5 s). The two `TICK_DEPENDS_ON_OBJ` gates lower into the program
/// as the dependency's own `ObjectiveAwake` conjunct — order is a gate on
/// evaluation, never a timing guess.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_the_damage_ladder_and_the_dependency_gates_are_record_data() {
    let row = census().row(MISSION).unwrap();
    let blocks = blocks_of(&control_document());
    assert_eq!(
        with(&blocks, "INACTIVE_COMPLETION_COUNT"),
        [3, 4, 5, 6],
        "four ladder blocks spell a threshold"
    );
    let thresholds: Vec<i64> = spelled(&blocks, "INACTIVE_COMPLETION_COUNT")
        .iter()
        .map(|args| match args.as_slice() {
            [ZrdValue::Int(number)] => i64::from(*number),
            other => panic!("an INACTIVE_COMPLETION_COUNT site spells {other:?}"),
        })
        .collect();
    assert_eq!(
        thresholds,
        [3, 5, 7, 10],
        "the thresholds are record data, rising along the ladder"
    );

    let engines = [
        "reng11", "reng12", "reng21", "reng22", "reng31", "reng32", "leng11", "leng12", "leng21",
        "leng22", "leng31", "leng32",
    ];
    let chains: Vec<MemberName> = engines
        .iter()
        .map(|engine| {
            ["piratezep", engine, "healthy"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        })
        .collect();
    for number in [3u32, 4, 5, 6] {
        let directives = block(&blocks, number);
        let listed: Vec<&Directive> = directives
            .iter()
            .filter(|d| d.key.starts_with("INACTIVE") && d.key != "INACTIVE_COMPLETION_COUNT")
            .collect();
        assert_eq!(
            listed.len(),
            12,
            "block {number} lists the twelve pirate-zeppelin engines"
        );
        for (position, directive) in listed.iter().enumerate() {
            assert_eq!(
                directive.key,
                format!("INACTIVE{}", position + 1),
                "block {number} spells the engines in order"
            );
            let chain = directive.args.as_deref().unwrap_or(&[]);
            assert_eq!(
                chain,
                [
                    ZrdValue::Text("piratezep".to_owned()),
                    ZrdValue::Text(engines[position].to_owned()),
                    ZrdValue::Text("healthy".to_owned()),
                ],
                "block {number} {} addresses its own engine's chain",
                directive.key
            );
        }
    }

    // The ladder's own edges: each stage naps the next, block 6 naps the loss
    // latch, and no other block of the ladder is entered by anything else.
    let ladder_naps: Vec<(u32, i64, f32)> = [3u32, 4, 5, 6]
        .into_iter()
        .flat_map(|number| {
            block(&blocks, number)
                .iter()
                .filter(|d| d.key == "NAP_OBJECTIVE_WHEN_I_COMPLETE")
                .map(move |directive| {
                    let args = directive.args.as_deref().unwrap_or(&[]);
                    let target = i64::from(match args.first() {
                        Some(ZrdValue::Int(number)) => *number,
                        other => panic!("a nap spells a target, not {other:?}"),
                    });
                    let delay = match args.get(1) {
                        Some(ZrdValue::Float(seconds)) => *seconds,
                        other => panic!("a nap spells a delay, not {other:?}"),
                    };
                    (number, target, delay)
                })
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(
        ladder_naps,
        [(3, 4, 15.0), (4, 5, 15.0), (5, 6, 10.0), (6, 7, 25.0)],
        "each stage enters the next on its own spelled delay; the fourth nap is the failure latch"
    );

    // Dormancy: which blocks are entered by their own clock and which by an
    // edge.
    let mut timed_wakes = Vec::new();
    let mut dormant_count = 0;
    let mut undormant = Vec::new();
    for (number, directives) in &blocks {
        let Some(dormant) = directives.iter().find(|d| d.key == "BEGIN_DORMANT") else {
            undormant.push(*number);
            continue;
        };
        dormant_count += 1;
        let armed = match dormant.args.as_ref().and_then(|args| args.first()) {
            Some(ZrdValue::Float(seconds)) => *seconds >= 0.0,
            _ => true,
        };
        if armed {
            timed_wakes.push((*number, dormant.args.clone().unwrap_or_default()));
        }
    }
    assert_eq!(dormant_count, 37, "every block but block 3 spells one");
    assert_eq!(
        undormant,
        [3],
        "the ladder starts with the mission: block 3 is never dormant"
    );
    assert_eq!(
        timed_wakes,
        [
            (1, vec![ZrdValue::Float(2.0)]),
            (2, vec![ZrdValue::Float(13.5)]),
        ],
        "only blocks 1 and 2 arm a clock; the latches and the ladder stages spell -1"
    );
    for number in [4u32, 5, 6, 7, 16] {
        assert_eq!(
            block(&blocks, number)
                .iter()
                .find(|d| d.key == "BEGIN_DORMANT")
                .and_then(|d| d.args.as_deref()),
            Some(&[ZrdValue::Float(-1.0)][..]),
            "block {number} never wakes on its own clock"
        );
    }

    // The two gates, as the program lowered them: block 15 depends on block 19
    // and block 21 on block 29, each conjunct carrying the dependency's own
    // zero-based index — `child0 − 1`, the measured convention.
    assert_eq!(with(&blocks, "TICK_DEPENDS_ON_OBJ"), [15, 21]);
    let gates: Vec<i64> = spelled(&blocks, "TICK_DEPENDS_ON_OBJ")
        .iter()
        .flat_map(|args| {
            args.iter().filter_map(|value| match value {
                ZrdValue::Int(number) => Some(i64::from(*number)),
                _ => None,
            })
        })
        .collect();
    assert_eq!(gates, [19, 29]);

    let attempt = row.lowering_attempt().unwrap();
    let raw = attempt.raw_program().expect("the program assembled");
    assert_eq!(
        raw.objectives[14].condition,
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 14 },
            Condition::ObjectiveAwake { index: 18 },
        ]),
        "block 15's gate on block 19 is a conjunct, not a dropped directive"
    );
    assert_eq!(
        raw.objectives[20].condition,
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 20 },
            Condition::ObjectiveAwake { index: 28 },
            Condition::EnemyGroupDepletion {
                group: 1,
                remaining: 0,
                generator: None,
            },
        ]),
        "block 21 gates on block 29 and depletes group 1"
    );

    // The ladder's own conditions, as the program lowered them: the same twelve
    // chains under the threshold the block spelled, and the loss latch is
    // complete as soon as it is awake.
    let expect_ladder = |index: u32, threshold: u32| {
        assert_eq!(
            raw.objectives[index as usize].condition,
            Condition::All(vec![
                Condition::ObjectiveAwake { index },
                Condition::InactiveMembers {
                    members: chains.clone(),
                    threshold,
                },
            ]),
            "block {} lowers the ladder condition it spelled",
            index + 1
        );
    };
    expect_ladder(2, 3);
    expect_ladder(3, 5);
    expect_ladder(4, 7);
    expect_ladder(5, 10);
    assert_eq!(
        raw.objectives[6].condition,
        Condition::ObjectiveAwake { index: 6 },
        "the failure latch completes on wake alone — its nap is the gate"
    );
}

/// **Every actor and node the record names resolves in the shipped data, with
/// exactly one exception.**
///
/// The record's directives name actors by string; this walks the archive's
/// other fifteen members and says where each name is *declared*. The exception
/// is `britkestrel_1`, the actor M13's single `SET_AI_NET` site points at a node
/// for: no member of this archive declares it. Its node operand `M3BritAce` is
/// declared by the chapter world's node index, which is what makes the two
/// halves of that site distinguishable at all.
///
/// Nothing here guesses what the original does with an undeclared name; the
/// gap is measured and filed (`docs/findings/2026-10-10-m13-b-compatibility-gaps.md`).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_every_actor_the_record_names_resolves_in_the_shipped_data_but_one() {
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
    let expected: [(&str, &[&str]); 17] = [
        (
            "piratezep",
            &[
                "aiv.zrd",
                "destroy_cargozep.zrd",
                "targets.zrd",
                "zeppelins.zrd",
            ],
        ),
        ("M3PirateZep", &["zeppelins.zrd"]),
        ("reng11", &["zeppelins.zrd"]),
        ("leng32", &["zeppelins.zrd"]),
        ("britpeace_1", &["aiv.zrd"]),
        ("britpeace_4", &["aiv.zrd"]),
        ("britkestrel_1", &[]),
        ("M3BritAce", &[]),
        (
            "player",
            &[
                "aiv.zrd",
                "destroy_cargozep.zrd",
                "map.zrd",
                "zeppelins.zrd",
            ],
        ),
        (
            "barracuda",
            &[
                "egen.zrd",
                "sub_movement.zrd",
                "submarine.zrd",
                "targets.zrd",
            ],
        ),
        ("sub_tower", &["sub_movement.zrd", "targets.zrd"]),
        ("subhealthy", &["egen.zrd", "submarine.zrd"]),
        ("aagun98", &["targets.zrd"]),
        ("aagun99", &["targets.zrd"]),
        ("8igun01", &["targets.zrd"]),
        ("runway_door", &["submarine.zrd"]),
        ("movebridge1", &["destroy_cargozep.zrd"]),
    ];
    let archive_members: Vec<String> = census()
        .row(MISSION)
        .unwrap()
        .members
        .iter()
        .map(|member| member.name.clone())
        .collect();
    assert_eq!(archive_members.len(), 16);
    assert!(archive_members.iter().any(|name| name == CONTROL_MEMBER));

    // Every other member's texts, read once: the control member only *spells*
    // a name, it never declares one.
    let declarations: Vec<(String, Vec<String>)> = archive_members
        .iter()
        .filter(|member| member.as_str() != CONTROL_MEMBER)
        .map(|member| (member.clone(), member_texts(CONTAINER, member)))
        .collect();
    assert_eq!(declarations.len(), 15);

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
    // world's node index — the second half of the site resolves, the first does
    // not.
    let world = member_texts(WORLD_CONTAINER, "neindex.zrd");
    assert!(
        world.iter().any(|text| text == "M3BritAce"),
        "the chapter world's node index declares M3BritAce: {world:?}"
    );
    let mission_texts = member_texts(CONTAINER, CONTROL_MEMBER);
    assert!(mission_texts.iter().any(|text| text == "britkestrel_1"));
    assert!(
        !world.iter().any(|text| text == "britkestrel_1"),
        "the node index does not declare the actor"
    );
}

/// **`britkestrel_1` is declared by no archive in the installation.**
///
/// The whole installation, decoded member by member through production
/// discovery: every `.zbd` container, every member its index names, every text
/// node its `.zrd` decoder yields. The one name M13's `SET_AI_NET` site points at
/// appears in exactly one place — M13's own control record, the site that spells
/// it. Every other directive actor the record names has a declaration
/// (pinned by the test above); this is the measured gap, not a guess about what
/// the original does with it. The node operand `M3BritAce` is declared twice:
/// here and in the chapter world's node index.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_the_ai_net_actor_is_declared_by_no_archive_in_the_installation() {
    assert_eq!(
        declarations_of("britkestrel_1"),
        [(CONTAINER.to_owned(), CONTROL_MEMBER.to_owned())],
        "the SET_AI_NET actor is declared only by the site that spells it"
    );
    assert_eq!(
        declarations_of("M3BritAce"),
        [
            (CONTAINER.to_owned(), CONTROL_MEMBER.to_owned()),
            (WORLD_CONTAINER.to_owned(), "neindex.zrd".to_owned()),
        ],
        "the SET_AI_NET node is declared by the mission and by the chapter world's node index"
    );
}

/// **Both terminal latches are gated by exactly one nap, and every block
/// address the record spells is a block of this record.**
///
/// `INSTANTLOSS` sits in block 7 and `INSTANTWIN` in block 16; no other block
/// ends the mission. Both start dormant with no timed wake, so neither latch
/// can fire on its own clock, and each is named by exactly one completion edge
/// — block 6's nap and block 15's nap, both with the delay the record spells.
///
/// The address walk covers 41 spelled integers — 20 wake, 4 kill, 15 nap, 2
/// gates — and every one lies in `1..=38`. The addressing itself is not assumed
/// here: M02-B-FU3 (#802) measured the original's parse decrementing every
/// objective address before storing it, so a spelled number **is** the block
/// number, and M06-B-FU3 (#819) reconciled the sibling suites to it. Two values
/// of M13's record discriminate the readings: block 9 spells `38`, the block
/// count itself (an index reading would report it out of range), and nothing
/// spells `0`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_the_terminal_blocks_are_gated_and_every_address_is_in_range() {
    let blocks = blocks_of(&control_document());
    let outcomes: Vec<u32> = blocks
        .iter()
        .filter(|(_, directives)| directives.iter().any(|d| d.key.starts_with("INSTANT")))
        .map(|(number, _)| *number)
        .collect();
    assert_eq!(outcomes, [7, 16], "no other block ends the mission");
    for (number, outcome) in [(7, "INSTANTLOSS"), (16, "INSTANTWIN")] {
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
            ("KILL_OBJECTIVE_WHEN_I_COMPLETE", 4),
            ("NAP_OBJECTIVE_WHEN_I_COMPLETE", 15),
            ("TICK_DEPENDS_ON_OBJ", 2),
            ("WAKE_OBJECTIVE_WHEN_I_COMPLETE", 20),
        ]),
        "the address walk visits every spelled address: 20 wake, 4 kill, 15 nap, 2 gates"
    );
    assert!(
        out_of_range.is_empty(),
        "every address is a block of this record: {out_of_range:?}"
    );
    assert_eq!(
        highest,
        i64::from(BLOCKS),
        "block 9 spells 38 — the block count itself"
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
        naming(7),
        [(6, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![7])],
        "the failure latch has exactly one completion edge: block 6's nap"
    );
    assert_eq!(
        naming(16),
        [(15, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![16])],
        "the success latch has exactly one completion edge: block 15's nap"
    );
    assert_eq!(
        naming(38),
        [(9, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![38])],
        "the discriminating address: the last block, in range only under the measured rule"
    );
}

/// **The two outcomes have disjoint prerequisites; the other twenty blocks are
/// named by neither latch.**
///
/// "Mandatory" here is stated narrowly, as the data supports: the blocks that
/// must be *entered* before a latch can be entered — every block that wakes or
/// naps its way towards it, plus the dependencies its gates declare. Kill edges
/// are deliberately excluded (killing block 9 does not enter it; block 12's kill
/// is asserted below not to read as a prerequisite).
///
/// * success (block 16): 13 blocks — `{2, 8, 9, 12, 13, 14, 15, 16, 19, 20, 21,
///   26, 29}` — reached from the mission's own 13.5 s clock through the wake and
///   nap chain and the two gates;
/// * failure (block 7): the five-block damage ladder `{3, 4, 5, 6, 7}`, disjoint
///   from it;
/// * the remaining 20 blocks are named by no prerequisite edge of either latch.
///
/// Whether any of those 20 is an *optional reward or stunt branch* is **not**
/// measured: M13-A binds no stunt or reward ids, so nothing here calls one
/// optional in that sense. What is pinned is that 23 blocks (including both
/// latches) lower to a bare `ObjectiveAwake` — they complete on wake alone —
/// and 15 spell a real evaluator or a gate.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_the_two_outcomes_have_disjoint_prerequisites_and_the_rest_is_not_mandatory() {
    let blocks = blocks_of(&control_document());
    let prerequisites = prerequisites(&blocks);

    // Kills are not prerequisites: block 12 kills block 9, but block 9's only
    // predecessor is block 8's nap.
    assert_eq!(prerequisites.get(&9), Some(&vec![8]));
    assert_eq!(prerequisites.get(&10), Some(&vec![38]));
    assert_eq!(
        prerequisites.get(&16),
        Some(&vec![15]),
        "one nap enters the success latch"
    );
    assert_eq!(prerequisites.get(&7), Some(&vec![6]));

    let win = closure(&prerequisites, 16);
    assert_eq!(
        win,
        [2, 8, 9, 12, 13, 14, 15, 16, 19, 20, 21, 26, 29],
        "the blocks that must be entered before the success latch can be entered"
    );
    let loss = closure(&prerequisites, 7);
    assert_eq!(
        loss,
        [3, 4, 5, 6, 7],
        "the damage ladder is its own closure"
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
            1, 10, 11, 17, 18, 22, 23, 24, 25, 27, 28, 30, 31, 32, 33, 34, 35, 36, 37, 38
        ],
        "twenty blocks are named by no prerequisite edge of either latch"
    );

    // The condition shapes: 23 blocks complete on wake alone, 15 spell an
    // evaluator or a gate.
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
            1, 2, 7, 8, 9, 10, 11, 16, 17, 18, 20, 22, 23, 24, 25, 26, 27, 28, 34, 35, 36, 37, 38
        ],
        "23 blocks — including both latches — complete on wake alone"
    );
    assert_eq!(
        evaluated,
        [3, 4, 5, 6, 12, 13, 14, 15, 19, 21, 29, 30, 31, 32, 33],
        "15 blocks spell an evaluator or a dependency gate"
    );
    assert_eq!(
        raw.objectives[29].condition,
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 29 },
            Condition::Travelers {
                subject: vec!["player".to_owned()],
                anchor: TravelersAnchor::Object(vec!["barracuda".to_owned()]),
                radius: 300.0,
                approaching: true,
            },
        ]),
        "block 30 is M13's TRAVELERS site: the player approaching `barracuda` at 300"
    );
}

/// **Every call binds, every condition lowers and M13's record completes.**
///
/// All 178 sites bind — the two `KILL_OBJECTIVE_WHEN_I_COMPLETE` sites (one
/// three targets wide) among them — and all 38 block conditions lower. The
/// bound program reaches `MissionProgram::validate` and validates; no lowering
/// requirement is unmet and M13's census row is complete. The census as a whole
/// stays not campaign-ready: other missions carry their own gaps.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m13_b_every_call_binds_every_condition_lowers_and_m13s_record_completes() {
    let row = census().row(MISSION).unwrap();
    let record = row.record().unwrap();

    let kill = record
        .key("KILL_OBJECTIVE_WHEN_I_COMPLETE")
        .expect("it is spelled");
    assert_eq!((kill.blocks, kill.sites), (2, 2));
    let mut kill_shapes: Vec<(usize, u32)> = kill
        .shapes
        .iter()
        .map(|(shape, sites)| (shape.arity(), *sites))
        .collect();
    kill_shapes.sort();
    assert_eq!(
        kill_shapes,
        [(1, 1), (3, 1)],
        "one single-target site and one three-target site"
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
        "every site binds: {refused_calls:?}"
    );
    assert!(
        lowered.unbound_keys.is_empty(),
        "the registry refused no key: {:?}",
        lowered.unbound_keys
    );
    assert!(
        attempt.program().is_some(),
        "a program stands for all 38 objectives"
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
        "all 38 block conditions lower: {refused_conditions:?}"
    );
    assert_eq!(lowered.conditions.len(), BLOCKS as usize);

    // Two retail blocks spell no `INACTIVE_COMPLETION_COUNT` of their own, so
    // the threshold that reaches them is their own member count — the default
    // the synthetic member above pins on authored records.
    let raw = attempt.raw_program().expect("the program assembled");
    assert_eq!(
        raw.objectives[11].condition,
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 11 },
            Condition::InactiveMembers {
                members: vec![vec!["barracuda".to_owned(), "subhealthy".to_owned()]],
                threshold: 1,
            },
        ]),
        "block 12 spells no count: one member, threshold one"
    );
    assert_eq!(
        raw.objectives[18].condition,
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 18 },
            Condition::InactiveMembers {
                members: vec![vec!["piratezep".to_owned()]],
                threshold: 1,
            },
        ]),
        "block 19 spells no count either"
    );

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

/// M13's own `TRAVELERS` spelling (block 30) with a chosen subject.
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
                    text("barracuda"),
                    ZrdValue::Float(300.0),
                    int(1),
                ],
            ),
        ],
    )])
}

/// **M13's `TRAVELERS` site lowers as a predicate; the counting-mode spelling
/// refuses.**
///
/// The same key at two subjects, so the refusal is the counting mode and not
/// the spelling: a named subject is the side-effect-free inside/outside test
/// M13 block 30 spells and lowers clean, a numeric `child0` arms a counter write
/// and refuses. This is the failure arm that keeps a "the record lowers" claim
/// from hiding an unmeasured predicate.
#[test]
fn accept_m13_b_m13s_travelers_site_lowers_and_a_counting_mode_refuses() {
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

/// **A directive this build measures no effect for is refused, not honoured.**
///
/// The allegiance half of the sheet's priorities has no directive in M13's
/// record; if one were spelled anyway — an authored `SET_FACTION` here — the
/// production measurement reports it unmeasured and the lowering refuses the
/// site by name instead of quietly doing nothing. That is the guard against an
/// implementation "supporting" an allegiance transition the shipped data never
/// spells, and against a guessed key ever being treated as measured.
#[test]
fn accept_m13_b_an_ungrounded_allegiance_directive_is_refused_rather_than_honoured() {
    let document = control_record(vec![block_site(
        1,
        vec![
            site("BEGIN_DORMANT", vec![ZrdValue::Float(-1.0)]),
            site(
                "SET_FACTION",
                vec![text("britkestrel_1"), text("britpeace")],
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
/// The damage ladder's whole mechanism is naps carrying a delay (15, 15, 10, 25
/// seconds), so a delay must never be read as an address: a nap at `2` whose
/// delay is `9999` in a two-block record lowers and validates, while an integer
/// in an address position is walked as one by the graph test — the addressing
/// rule itself is pinned once for the reconciled suites by M06-B-FU3 (#819).
///
/// The other half is M13's own evaluator: six of its blocks spell `INACTIVE1`
/// with **no** `INACTIVE_COMPLETION_COUNT`, and the lowering writes the block's
/// own member count as the threshold — never `1`, never an invented default. A
/// spelled count wins over that default, and it is carried exactly as spelled:
/// a count of `99` against three members stays `99`, because the record is data
/// and clamping it would be a guess.
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
