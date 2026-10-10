//! Acceptance stage M19-B: the mission-specific compatibility gaps of the
//! nineteenth mission, "Rescue the Black Swan" (`missions/M19.md`, work order
//! `M19-B`).
//!
//! M19-A bound M19's identities and left the mission program unmeasured. The
//! machinery that measures a control program (the `.zrd` reader, the census,
//! the directive dispositions and the record → `RawProgram` adapter) is shared
//! and was built for M01; this stage runs it over M19's own reader archive and
//! pins what is **different** at M19, together with the three regression
//! priorities the sheet names:
//!
//! * M19's control program is `objectives.zrd` of `ZBD/C4/M04/zrdr.zbd` — the
//!   **largest measured record in the census**: 108 numbered blocks and 440
//!   directive sites under 36 keys, and it **lowers completely** (every call
//!   binds, every condition lowers, `MissionProgram::validate` accepts);
//! * **world unlock conditions** are staged by the record itself: ten
//!   undormant watchers, one 2-second clock (block 1) and three
//!   `TICK_DEPENDS_ON_OBJ` gates — `23 → 24` (the `piratezep`-inactive
//!   watcher), `65 → 106` and `91 → 103` (the launch waves) — plus the target
//!   flag operations that add and remove the fortress's targets stage by
//!   stage;
//! * **player-aircraft transfer** is spelled, not directived: no key of the
//!   vocabulary transfers an aircraft. Block 19 (`PRIMARY` slot 3) arms the
//!   `activate_bmhookup_node` animation on wake and completes on
//!   `ANIM_STATE bm_hookup_player EXECUTED`, while blocks 18/19/27/46 move
//!   the `player_bmhook`/`bm_hook` objective-target pair between stages;
//! * **ally extraction** is the win predicate itself: block 30 (`PRIMARY`
//!   slot 5) carries `INSTANTWIN` and completes only when
//!   `ANIM_STATE hooked_to_klondike EXECUTED`, reached through exactly one
//!   edge — block 49's nap — behind the mansion and escort-depletion chain
//!   `19 → 22 → 23 → 28 → 29 → 49`;
//! * the **failure side is measured and state-dependent**: four watcher
//!   chains — gasbag panels `9 → 12`, engines `14 → 16`, hangar supports
//!   `20 | 21` and the helium tank `39` — each end by napping the one
//!   `INSTANTLOSS` latch (block 13) and killing the same nine primary blocks
//!   `{5, 6, 7, 17, 18, 19, 28, 30, 38}`; and the goods block 17 kills the
//!   helium-tank watcher 39 while waking block 26, which watches the **same**
//!   `[bhf_heliumtank1, tank1_healthy]` chain — the same world event is a
//!   failure before the goods stage and the dock release after it;
//! * every actor the record names resolves in shipped data across three
//!   scopes — this archive, the chapter-4 world container and the shared
//!   `ZBD/zrdr.zbd` — with two measured footnotes: `b_turret1`/`b_turret2`
//!   are spelled literally only in control records (the world declares them
//!   through the `b_turret*` wildcard), and `klondike`, the rescued ally, is
//!   named by no `.zrd` text anywhere — it exists only inside the animation
//!   name `hooked_to_klondike`.
//!
//! A lowered program is **not** a played mission: no playthrough, difficulty,
//! media or presentation row is covered (that is M19-C, with `human_play`),
//! and the wrong-actor, wrong-session and repeated-event halves of the
//! sheet's priorities are runtime observations that stay unmeasured here.
//! The measured unknowns are written up in
//! `docs/findings/2026-10-10-m19-b-compatibility-gaps.md`.
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
use cs_script::ir::{AnimationState, Condition, MemberName, Value};
use cs_types::content::{ContentId, ContentKind};
use cs_types::install::RelativePath;

use crate::common::load_inventory;

/// The census row label of the mission (F13-B's mission-scope rule).
const MISSION: &str = "zbd/c4/m04";
/// The reader archive the installation ships for M19 — the program span
/// `missions/bindings/M19.json` cites.
const CONTAINER: &str = "ZBD/C4/M04/zrdr.zbd";
/// SHA-256 of that whole archive, from production discovery.
const CONTAINER_SHA256: &str = "c59692a9891775cc7f31cf26bcb4928e99084f64abcb73a93901c08f4ad5cbd6";
/// The archive's length in bytes — M19-A's own source span.
const CONTAINER_LENGTH: u64 = 61_137;
/// The member the measured rule chose.
const CONTROL_MEMBER: &str = "objectives.zrd";
/// SHA-256 of the control member's own bytes.
const CONTROL_SHA256: &str = "907df44904feb9e447c80fb3f2594a5ab7b5d549dae5ec349a9db356fed48c96";
/// The control member's first byte inside the archive.
const CONTROL_OFFSET: u64 = 19_740;
/// The control member's length in bytes.
const CONTROL_LENGTH: u64 = 28_323;
/// The chapter-4 world-group archive, which carries the hookup, launch,
/// fortress-animation and node-index members the record's names resolve in.
const WORLD_CONTAINER: &str = "ZBD/C4/zrdr.zbd";
/// The shared animation/turret bank the hook, cargo and wildcard names
/// resolve in.
const SHARED_CONTAINER: &str = "ZBD/zrdr.zbd";
/// The numbered blocks of the control member.
const BLOCKS: u32 = 108;
/// The directive sites of the control member.
const SITES: u32 = 440;
/// The distinct directive keys of the control member.
const KEYS: usize = 36;
/// The distinct text nodes the record's directive sites spell — actors,
/// member chains, keywords, animation, sound and message operands. The
/// full-coverage claim below is measured against this count.
const NAMED_TEXTS: usize = 111;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M19-B needs the retail capability; run this suite with \
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

/// The declared discovery title of `M19`, read from the committed inventory.
fn declared_title() -> String {
    let inventory = load_inventory();
    inventory
        .iter()
        .find(|(label, _)| label.as_str() == "M19")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M19 work order")
}

/// The production control-program binding, built once for the whole suite.
fn control_binding() -> &'static MissionControlBinding {
    static BINDING: OnceLock<MissionControlBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        let context = SourceContext::read(&game_dir())
            .expect("production source context reads the installation");
        context
            .control_program(
                MissionLabel::new("M19").expect("M19 is a valid label"),
                &declared_title(),
            )
            .expect("M19's control program binds through the measured rule")
    })
}

/// The M19 mission binding M19-A derives, built once — this stage consumes its
/// identities and adds no second evidence for the join itself.
fn mission_binding() -> &'static SourceBinding {
    static BINDING: OnceLock<SourceBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        let context = SourceContext::read(&game_dir())
            .expect("production source context reads the installation");
        context
            .bind(
                MissionLabel::new("M19").expect("M19 is a valid label"),
                &declared_title(),
            )
            .expect("M19 binds to the original data")
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
        let children = value.as_list().expect("every M19 block is a list");
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
        .expect("the rule finds M19's control member")
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
/// animation resolutions below: names only, never bytes.
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

/// Every distinct `.zrd` text node the whole installation declares **outside**
/// M19's control member, collected in one pass.
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
// Retail: what M19's control program is
// ---------------------------------------------------------------------------

/// **The control program is the member that declares the numbered blocks.**
///
/// Of the 13 members of M19's reader archive exactly one declares numbered
/// `OBJECTIVE<N>` blocks: `objectives.zrd`, the eighth member — which happens
/// also to be the archive's longest member, so this test pins that the rule
/// chose it by the block declaration, not by position or size. The blocks and
/// sites the census measures equal an independent walk of the same document,
/// the archive is the program span M19-A bound, and the production control
/// binding reaches the same member, span and digests through its own walk.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m19_b_the_control_program_is_the_member_that_declares_the_blocks() {
    let row = census().row(MISSION).expect("M19 is in the census");
    assert_eq!(row.container, CONTAINER);
    assert_eq!(
        row.container_sha256, CONTAINER_SHA256,
        "the reader archive is the program M19-A bound"
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
    assert!(
        row.members.iter().all(|member| member.len <= chosen.len),
        "M19's control member is coincidentally its longest — the block \
         declaration, not the length, is what selects it"
    );

    let record = row.record().expect("M19 has a measured control program");
    assert_eq!((record.blocks(), record.sites()), (BLOCKS, SITES));
    assert_eq!(record.keys().len(), KEYS);

    let (document, member) = read_control_member(&game_dir(), MISSION)
        .expect("the rule finds M19's control member again");
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
        "numbered 1..=108, no gaps"
    );
    let walked: usize = blocks.iter().map(|(_, directives)| directives.len()).sum();
    assert_eq!(walked as u32, record.sites(), "the independent walk agrees");
    assert!(
        record.refusals().is_empty(),
        "every block is a readable list"
    );
    assert!(
        record.unclassified_record_keys().is_empty(),
        "M19 spells no record-level key outside the measured vocabulary"
    );

    // The production binding and the census must not disagree about the
    // mission, the program, the member or the record.
    let bound = control_binding();
    assert_eq!(bound.mission.as_str(), "mission/ch4-m04");
    assert_eq!(bound.program_id.as_str(), "script/c4-m04-zrdr");
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

    // …and both agree with the mission binding M19-A committed: one mission id,
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

/// **Every directive key M19 spells has exactly one disposition, and none is
/// refused.**
///
/// Two keys are terminal outcomes (`INSTANTWIN`, `INSTANTLOSS`, one bare site
/// each), the other 34 have a measured effect, and no key is Unmeasured. The
/// sites are accounted for: the keys' sites sum to the record's 440, and the
/// sorted vocabulary is exactly the 36 keys the archive spells — so a key that
/// appears, disappears or silently loses its meaning fails here.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m19_b_every_directive_m19_spells_has_a_disposition_and_none_is_refused() {
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
        "M19's vocabulary is fully measured: {refused:?}"
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
            "ADD_OTHER_TARGET",
            "ANIM_STATE",
            "BEGIN_DORMANT",
            "COMPLETED_SOUND_GROUP",
            "COMPLETED_STOPPOINT",
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
            "SET_HELP_LABEL",
            "TICK_DEPENDS_ON_OBJ",
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
/// and none of M19's keys transfers an aircraft or changes an allegiance.**
///
/// * **world unlock conditions**: the three `TICK_DEPENDS_ON_OBJ` gates, the
///   dormancy markers the ten watchers lack, and the `ADD_/REMOVE_*_TARGET`,
///   `WAKEUP_TURRETS`/`WAKEUP_ZEP_TURRETS`, `WAKE_ANIM` and
///   `COMPLETED_STOPPOINT` operations the fortress unlock is staged with;
/// * **player-aircraft transfer**: `ANIM_STATE` (the hookup completion
///   predicate), `WAKE_ANIM` (arming the hookup node) and the target-flag
///   pair — asserted against the exact 36-key vocabulary, none of which is a
///   transfer;
/// * **ally extraction**: the `INSTANTWIN` outcome, the `ANIM_STATE`
///   predicate that gates it, and the `NAP`/`DEDG`/`COMPLETED_SOUND_GROUP`
///   operations on the extraction path.
///
/// Every key is asserted to resolve to the operation the shared findings
/// measured, so a key that silently lost its meaning fails here.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m19_b_the_sheet_priorities_resolve_to_measured_operations_and_none_transfers() {
    let record = census().row(MISSION).unwrap().record().unwrap();
    let operation = |key: &str| match record.key(key).unwrap().disposition() {
        DirectiveDisposition::Measured(measured) => measured.operation,
        other => panic!("{key} is not measured: {other:?}"),
    };

    // World unlock: gates, dormancy, target flags, turret and animation wakes.
    assert_eq!(
        operation("TICK_DEPENDS_ON_OBJ"),
        DirectiveOperation::DependencyGate
    );
    assert_eq!(operation("BEGIN_DORMANT"), DirectiveOperation::DormantStart);
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
    assert_eq!(operation("WAKEUP_TURRETS"), DirectiveOperation::WakeTurrets);
    assert_eq!(
        operation("WAKEUP_ZEP_TURRETS"),
        DirectiveOperation::WakeZeppelinTurrets
    );
    assert_eq!(operation("WAKE_ANIM"), DirectiveOperation::WakeAnimation);
    assert_eq!(
        operation("COMPLETED_STOPPOINT"),
        DirectiveOperation::AdvanceStopPoint
    );

    // Player-aircraft transfer: the animation evaluator and the wakes — the
    // record's only mechanism, since no transfer key exists.
    assert_eq!(operation("ANIM_STATE"), DirectiveOperation::AnimationStates);
    for stage in (1..=15).map(|stage| format!("INACTIVE{stage}")) {
        assert_eq!(
            operation(&stage),
            DirectiveOperation::InactiveMembers,
            "{stage}"
        );
    }
    assert_eq!(
        operation("INACTIVE_COMPLETION_COUNT"),
        DirectiveOperation::InactiveThreshold
    );

    // Ally extraction: the outcome, the nap edge into it and the depletions.
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
    assert_eq!(operation("DEDG"), DirectiveOperation::EnemyGroupDepletion);
    assert_eq!(
        operation("COMPLETED_SOUND_GROUP"),
        DirectiveOperation::CompletedSoundGroup
    );
    assert_eq!(
        operation("IDENTITY"),
        DirectiveOperation::PresentationIdentity
    );
    assert_eq!(
        operation("SET_HELP_LABEL"),
        DirectiveOperation::SetHelpLabel
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

    // No directive of this record transfers the player's aircraft, re-seats a
    // pilot or changes who an actor belongs to: the transfer the sheet asks
    // about is spelled as animation state, not as a key.
    let forbidden = [
        "TRANSFER",
        "SWAP",
        "SEAT",
        "BOARD",
        "ENTER",
        "VEHICLE",
        "FACTION",
        "OWNER",
        "TEAM",
        "ATTITUDE",
        "ALLEGIANCE",
        "LOYAL",
    ];
    let mut transfer_keys = Vec::new();
    for key in record.keys() {
        let upper = key.key.to_ascii_uppercase();
        if forbidden.iter().any(|token| upper.contains(token)) {
            transfer_keys.push(key.key.as_str());
        }
    }
    assert!(
        transfer_keys.is_empty(),
        "M19's control member spells no transfer or allegiance directive: {transfer_keys:?}"
    );
}

/// **The hookup transfer and the extraction are record data — both spelled as
/// `ANIM_STATE` predicates on animations declared outside this archive.**
///
/// Block 18 (undormant) counts the eight fortress turrets and, on completion,
/// targets the hook pair `[player_bmhook, bm_hook]` and wakes block 19 and the
/// rail watcher 46. Block 19 (`PRIMARY` slot 3, `MSG_BRF_RMM4_OBJ3`) arms
/// `activate_bmhookup_node` on wake, completes on the `bm_hookup_player`
/// animation reaching `EXECUTED`, removes the hook targets, naps the dock
/// response 22 and wakes the rail watcher 27 and the secondary marker 78 —
/// killing blocks `{20, 21, 61, 62, 63, 64}`. Block 30 (`PRIMARY` slot 5,
/// `INSTANTWIN`) wakes `pzhomebase` and completes only when
/// `hooked_to_klondike` has executed.
///
/// Both animation operands resolve in shipped data: `bm_hookup_player` and
/// `activate_bmhookup_node` in the chapter world's `bhmhookup.zrd`,
/// `hooked_to_klondike` and `pzhomebase` in the shared `pzep_hookup.zrd`, and
/// the hook targets in this archive's `targets.zrd`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m19_b_the_hookup_transfer_and_the_extraction_are_record_data() {
    let blocks = blocks_of(&control_document());

    // The transfer half: blocks 18/19/27/46 and the hook-target pair.
    assert_eq!(
        with(&blocks, "ANIM_STATE"),
        [19, 30],
        "exactly two animation predicates: the hookup and the extraction"
    );
    let add_hook = |number: u32| -> bool {
        block(&blocks, number).iter().any(|d| {
            d.key == "ADD_OBJECTIVE_TARGET"
                && d.args.as_deref().is_some_and(|args| {
                    args.iter().any(|value| {
                        matches!(value, ZrdValue::List(pair)
                        if pair.as_slice() == [
                            ZrdValue::Text("player_bmhook".to_owned()),
                            ZrdValue::Text("bm_hook".to_owned()),
                        ])
                    })
                })
        })
    };
    assert!(add_hook(18), "block 18 targets the hook pair");
    assert!(
        !with(&blocks, "ADD_OBJECTIVE_TARGET")
            .iter()
            .any(|number| *number != 18 && add_hook(*number)),
        "only block 18 adds the hook pair"
    );

    let remove_hook = |directive: &Directive| {
        directive.key == "REMOVE_OBJECTIVE_TARGET"
            && directive.args.as_deref().is_some_and(|args| {
                args.iter().any(|value| {
                    matches!(value, ZrdValue::List(pair)
                    if pair.as_slice() == [
                        ZrdValue::Text("player_bmhook".to_owned()),
                        ZrdValue::Text("bm_hook".to_owned()),
                    ])
                })
            })
    };
    assert_eq!(
        with(&blocks, "REMOVE_OBJECTIVE_TARGET")
            .into_iter()
            .filter(|number| { block(&blocks, *number).iter().any(remove_hook) })
            .collect::<Vec<_>>(),
        [19, 27, 46],
        "the hook targets are removed at the three stages"
    );

    // Block 19, as spelled.
    let nineteen: Vec<String> = block(&blocks, 19).iter().map(|d| d.key.clone()).collect();
    assert_eq!(
        nineteen,
        [
            "BEGIN_DORMANT",
            "IDENTITY",
            "WAKE_ANIM",
            "ANIM_STATE",
            "REMOVE_OBJECTIVE_TARGET",
            "NAP_OBJECTIVE_WHEN_I_COMPLETE",
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
            "KILL_OBJECTIVE_WHEN_I_COMPLETE",
        ],
        "the hookup block's own directive order"
    );
    assert_eq!(
        block(&blocks, 19)[1].args.as_deref(),
        Some(
            &[
                ZrdValue::Text("PRIMARY".to_owned()),
                ZrdValue::Int(3),
                ZrdValue::Text("MSG_BRF_RMM4_OBJ3".to_owned()),
            ][..]
        ),
        "PRIMARY slot 3 carries the third briefing message"
    );

    // The two ANIM_STATE predicates as the program lowered them.
    let attempt = census().row(MISSION).unwrap().lowering_attempt().unwrap();
    let raw = attempt.raw_program().expect("the program assembled");
    assert_eq!(
        raw.objectives[18].condition,
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 18 },
            Condition::AnimationStates {
                required: 1,
                animations: vec![("bm_hookup_player".to_owned(), AnimationState::Executed)],
            },
        ]),
        "block 19 completes when the player-hookup animation has executed"
    );
    assert_eq!(
        raw.objectives[29].condition,
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 29 },
            Condition::AnimationStates {
                required: 1,
                animations: vec![("hooked_to_klondike".to_owned(), AnimationState::Executed)],
            },
        ]),
        "block 30 — the INSTANTWIN latch — completes when the extraction \
         animation has executed: the ally extraction is the win predicate"
    );

    // The names resolve outside this archive's control member.
    let hookup = member_texts(WORLD_CONTAINER, "bhmhookup.zrd");
    for name in ["bm_hookup_player", "activate_bmhookup_node"] {
        assert!(
            hookup.iter().any(|text| text == name),
            "the chapter world's hookup member declares {name:?}"
        );
    }
    let shared_hook = member_texts(SHARED_CONTAINER, "pzep_hookup.zrd");
    for name in ["hooked_to_klondike", "pzhomebase"] {
        assert!(
            shared_hook.iter().any(|text| text == name),
            "the shared zeppelin-hook bank declares {name:?}"
        );
    }
    let targets = member_texts(CONTAINER, "targets.zrd");
    for name in ["player_bmhook", "bm_hook"] {
        assert!(
            targets.iter().any(|text| text == name),
            "the archive's own targets member declares {name:?}"
        );
    }
}

/// **The unlock gates and the undormant watchers are record data.**
///
/// Ten blocks carry no `BEGIN_DORMANT` and watch from the mission's first
/// tick — `{2, 5, 9, 14, 18, 20, 21, 25, 39, 60}`: the first-hit detector, the
/// primary turret count, both damage-ladder starts, the rail and support
/// watchers, the zeppelin's in-play bit, the helium tank and the half-turret
/// sound. Block 1 is the record's **only** armed clock (2 s: the zeppelin
/// turret wake and the start sound). The three `TICK_DEPENDS_ON_OBJ` gates —
/// `23 → 24`, `65 → 106`, `91 → 103` — lower as the dependency's own
/// `ObjectiveAwake` conjunct: block 23 (payback) evaluates only while the
/// `piratezep`-inactive watcher 24 is awake, and the warhawk and brigand
/// launch waves are gated on their own zeppelin watchers. Order is a gate on
/// evaluation, never a timing guess.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m19_b_the_unlock_gates_and_the_watchers_are_record_data() {
    let blocks = blocks_of(&control_document());
    let mut timed_wakes = Vec::new();
    let mut undormant = Vec::new();
    for (number, directives) in &blocks {
        let Some(dormant) = directives.iter().find(|d| d.key == "BEGIN_DORMANT") else {
            undormant.push(*number);
            continue;
        };
        let armed = match dormant.args.as_ref().and_then(|args| args.first()) {
            Some(ZrdValue::Float(seconds)) => *seconds >= 0.0,
            _ => true,
        };
        if armed {
            timed_wakes.push((*number, dormant.args.clone().unwrap_or_default()));
        }
    }
    assert_eq!(
        undormant,
        [2, 5, 9, 14, 18, 20, 21, 25, 39, 60],
        "ten watchers run from the first tick — every other block is entered"
    );
    assert_eq!(
        timed_wakes,
        [(1, vec![ZrdValue::Float(2.0)])],
        "block 1's two seconds is the record's only armed clock"
    );

    assert_eq!(
        with(&blocks, "TICK_DEPENDS_ON_OBJ"),
        [23, 65, 91],
        "three dependency gates: payback, the warhawk wave, the brigand wave"
    );
    let gates: Vec<i64> = spelled(&blocks, "TICK_DEPENDS_ON_OBJ")
        .iter()
        .flat_map(|args| {
            args.iter().filter_map(|value| match value {
                ZrdValue::Int(number) => Some(i64::from(*number)),
                _ => None,
            })
        })
        .collect();
    assert_eq!(gates, [24, 106, 103]);

    // The dependencies are all piratezep in-play watchers, spelled as one
    // chain — 24 is undormant, 103 and 106 are dormant entries re-armed inside
    // the launch waves.
    for number in [24u32, 103, 106] {
        let directives = block(&blocks, number);
        let chains: Vec<&Vec<ZrdValue>> = directives
            .iter()
            .filter(|d| d.key.starts_with("INACTIVE") && d.key != "INACTIVE_COMPLETION_COUNT")
            .filter_map(|d| d.args.as_ref())
            .collect();
        assert_eq!(
            chains,
            [&vec![ZrdValue::Text("piratezep".to_owned())]],
            "block {number} watches the zeppelin's in-play bit"
        );
    }

    // The gates as the program lowered them: each conjunct carries the
    // dependency's own zero-based index — `child0 − 1`, the measured
    // convention.
    let attempt = census().row(MISSION).unwrap().lowering_attempt().unwrap();
    let raw = attempt.raw_program().expect("the program assembled");
    assert_eq!(
        raw.objectives[22].condition,
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 22 },
            Condition::ObjectiveAwake { index: 23 },
        ]),
        "block 23 evaluates only while the zeppelin watcher 24 is awake"
    );
    assert_eq!(
        raw.objectives[64].condition,
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 64 },
            Condition::ObjectiveAwake { index: 105 },
        ]),
        "the warhawk wave is gated on watcher 106"
    );
    assert_eq!(
        raw.objectives[90].condition,
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 90 },
            Condition::ObjectiveAwake { index: 102 },
            Condition::EnemyGroupDepletion {
                group: 2,
                remaining: 0,
                generator: None,
            },
        ]),
        "the brigand wave gates on watcher 103 and depletes escort group 2"
    );
}

/// **The failure watchers share one latch and kill the same nine primaries —
/// and the helium tank is a state-dependent guard.**
///
/// Four undormant (or nap-entered) chains watch the rescue's fragile states:
///
/// * the gasbag-panel ladder `9 → 10 → 11 → 12` counts 1, 2, 3 and 4 of the
///   zeppelin's six `[piratezep, gasbag<n>, panels]` chains gone quiet;
/// * the engine ladder `14 → 15 → 16` counts 3, 5 and 7 of the twelve
///   `reng`/`leng` engine chains;
/// * the hangar supports: block 20 sounds at 1 of 4 (a warning) while block
///   21 — spelling no count — fails at its own member count, all four;
/// * block 39 watches `[bhf_heliumtank1, tank1_healthy]` alone.
///
/// The worst rung of each — 12, 16, 21 and 39 — naps the one `INSTANTLOSS`
/// latch (block 13, after the spelled 20 s) and kills the same nine blocks
/// `{5, 6, 7, 17, 18, 19, 28, 30, 38}` (39 adds the dock target 48), so a
/// failure ends the primaries instead of leaving them dangling.
///
/// And block 39 is **killed by block 17** — the goods primary — which in the
/// same completion wakes block 26, a second watcher on the *same* helium-tank
/// chain that removes the `bhf_dock` target and kills the dock target block
/// 48. One world event is failure before the goods stage and the dock
/// release after it; the record spells that transition as a kill plus a
/// re-watch, never a flag.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m19_b_the_failure_watchers_share_one_latch_and_the_kill_lists_match() {
    let blocks = blocks_of(&control_document());

    // The two ladders: same chains, rising thresholds.
    let panels: Vec<MemberName> = (1..=6)
        .map(|index| {
            ["piratezep", &format!("gasbag{index}"), "panels"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        })
        .collect();
    let engines = [
        "reng11", "reng12", "reng21", "reng22", "reng31", "reng32", "leng11", "leng12", "leng21",
        "leng22", "leng31", "leng32",
    ];
    let engine_chains: Vec<MemberName> = engines
        .iter()
        .map(|engine| {
            ["piratezep", engine, "healthy"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        })
        .collect();
    let ladder = |number: u32, expected: &[MemberName], threshold: i64| {
        let directives = block(&blocks, number);
        let listed: Vec<&Directive> = directives
            .iter()
            .filter(|d| d.key.starts_with("INACTIVE") && d.key != "INACTIVE_COMPLETION_COUNT")
            .collect();
        assert_eq!(
            listed.len(),
            expected.len(),
            "block {number} lists the chain set"
        );
        for (position, directive) in listed.iter().enumerate() {
            assert_eq!(
                directive.key,
                format!("INACTIVE{}", position + 1),
                "block {number} spells the chains in order"
            );
            assert_eq!(
                directive.args.as_deref().unwrap_or(&[]),
                expected[position]
                    .iter()
                    .map(|text| ZrdValue::Text(text.clone()))
                    .collect::<Vec<_>>()
                    .as_slice(),
                "block {number} {} addresses its own chain",
                directive.key
            );
        }
        let count = directives
            .iter()
            .find(|d| d.key == "INACTIVE_COMPLETION_COUNT")
            .and_then(|d| d.args.as_deref())
            .and_then(|args| args.first());
        assert_eq!(
            count,
            Some(&ZrdValue::Int(threshold as u32)),
            "block {number} spells the {threshold}-of-{} threshold",
            expected.len()
        );
    };
    for (number, threshold) in [(9, 1), (10, 2), (11, 3), (12, 4)] {
        ladder(number, &panels, threshold);
    }
    for (number, threshold) in [(14, 3), (15, 5), (16, 7)] {
        ladder(number, &engine_chains, threshold);
    }

    // The support watchers: 20 warns at one of four, 21 fails at all four
    // (no count spelled — the default is its own member list's length).
    let supports: Vec<MemberName> = (1..=4)
        .map(|index| {
            ["bhf_hangar", &format!("bhf_support{index}")]
                .into_iter()
                .map(str::to_owned)
                .collect()
        })
        .collect();
    ladder(20, &supports, 1);
    let twenty_one = block(&blocks, 21);
    assert!(
        !twenty_one
            .iter()
            .any(|d| d.key == "INACTIVE_COMPLETION_COUNT"),
        "block 21 spells no count: the failure threshold is all four supports"
    );
    let listed: Vec<&Directive> = twenty_one
        .iter()
        .filter(|d| d.key.starts_with("INACTIVE") && d.key != "INACTIVE_COMPLETION_COUNT")
        .collect();
    assert_eq!(listed.len(), 4);

    // The latch entries: exactly four naps name block 13, each after the same
    // spelled delay, and each worst rung kills the same nine primaries.
    let latch_naps: Vec<(u32, f32)> = blocks
        .iter()
        .flat_map(|(number, directives)| {
            directives.iter().filter_map(move |d| {
                if d.key != "NAP_OBJECTIVE_WHEN_I_COMPLETE" {
                    return None;
                }
                match d.args.as_deref() {
                    Some([ZrdValue::Int(target), ZrdValue::Float(delay)]) if *target == 13 => {
                        Some((*number, *delay))
                    }
                    _ => None,
                }
            })
        })
        .collect();
    assert_eq!(
        latch_naps,
        [(12, 20.0), (16, 20.0), (21, 20.0), (39, 20.0)],
        "four watchers enter the failure latch, each on the spelled 20 s"
    );
    let primaries = [5, 6, 7, 17, 18, 19, 28, 30, 38];
    for number in [12u32, 16, 21] {
        let kills: Vec<i64> = block(&blocks, number)
            .iter()
            .filter(|d| d.key == "KILL_OBJECTIVE_WHEN_I_COMPLETE")
            .flat_map(addresses)
            .collect();
        assert_eq!(
            kills, primaries,
            "block {number} kills the same nine primary blocks"
        );
    }
    let mut thirty_nine: Vec<i64> = primaries.to_vec();
    thirty_nine.push(48);
    let kills: Vec<i64> = block(&blocks, 39)
        .iter()
        .filter(|d| d.key == "KILL_OBJECTIVE_WHEN_I_COMPLETE")
        .flat_map(addresses)
        .collect();
    assert_eq!(kills, thirty_nine, "block 39 also kills the dock target");

    // The state-dependent guard: blocks 26 and 39 spell the same helium-tank
    // chain; block 17's completion kills 39 and wakes 26, so the tank's
    // destruction reads differently before and after the goods stage.
    let tank_chain = [
        ZrdValue::Text("bhf_heliumtank1".to_owned()),
        ZrdValue::Text("tank1_healthy".to_owned()),
    ];
    for number in [26u32, 39] {
        assert_eq!(
            block(&blocks, number)
                .iter()
                .find(|d| d.key == "INACTIVE1")
                .and_then(|d| d.args.as_deref()),
            Some(&tank_chain[..]),
            "block {number} watches the helium tank's health chain"
        );
    }
    let seventeen_edges: Vec<(String, Vec<i64>)> = block(&blocks, 17)
        .iter()
        .filter(|d| {
            d.key == "KILL_OBJECTIVE_WHEN_I_COMPLETE" || d.key == "WAKE_OBJECTIVE_WHEN_I_COMPLETE"
        })
        .map(|d| (d.key.clone(), addresses(d)))
        .collect();
    assert_eq!(
        seventeen_edges,
        [
            ("KILL_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![39]),
            (
                "WAKE_OBJECTIVE_WHEN_I_COMPLETE".to_owned(),
                vec![26, 64, 80]
            ),
        ],
        "the goods primary ends the failure watcher and arms the release"
    );
}

/// **Every actor and node the record names resolves in the shipped data — the
/// balloon turrets only through their wildcard, and `klondike` nowhere.**
///
/// The record's directives name actors by string; this walks the archive's
/// other twelve members plus the chapter-4 world and the shared `ZBD/zrdr.zbd`
/// members the names resolve in, and says where each is *declared*. Two
/// measured footnotes:
///
/// * `b_turret1`/`b_turret2` — the balloon turrets the INACTIVE chains spell
///   literally — are declared by no member literally; the only declaration
///   anywhere is the `b_turret*` wildcard in this archive's `aiv.zrd` and the
///   shared `ai.zrd`/`balloon_turret.zrd` records (the one-digit-consuming
///   wildcard of finding C);
/// * `klondike` — the Black Swan the mission rescues — is spelled by no
///   `.zrd` text in the installation: the extracted ally exists only inside
///   the animation name `hooked_to_klondike`.
///
/// The table is representative, so the last block below closes it: every one
/// of the [`NAMED_TEXTS`] distinct texts the record's sites spell is declared
/// somewhere outside M19's control member, except a measured set of five —
/// the `MSG_BRF_RMM4_OBJ*` operands of the `IDENTITY` sites, message ids that
/// no `.zrd` member spells (the measured parse never re-reads that child).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m19_b_every_actor_the_record_names_resolves_in_the_shipped_data() {
    let blocks = blocks_of(&control_document());
    // The names a directive spells — walked recursively, because a site's
    // argument list may nest one level deeper (`COMPLETED_STOPPOINT`,
    // `SET_HELP_LABEL`, `ADD_/REMOVE_OTHER_TARGET` spell their records inside
    // a list of their own).
    let mut named: BTreeSet<String> = BTreeSet::new();
    let mut spelled_texts: Vec<String> = Vec::new();
    for (_, directives) in &blocks {
        for directive in directives {
            for value in directive.args.iter().flatten() {
                walk_texts(value, &mut spelled_texts);
            }
        }
    }
    named.extend(spelled_texts);

    // (name, members of M19's archive, other than the control member, that
    // declare it) — the control member only *spells* a name.
    let own: [(&str, &[&str]); 15] = [
        (
            "piratezep",
            &[
                "aiv.zrd",
                "pzep_getcargo3.zrd",
                "targets.zrd",
                "zeppelins.zrd",
            ],
        ),
        ("M4Piratezep", &["zeppelins.zrd"]),
        ("player_bmhook", &["targets.zrd"]),
        ("bm_hook", &["targets.zrd"]),
        ("pzhookpoint", &["targets.zrd"]),
        ("zepgetcargo", &["pzep_getcargo3.zrd"]),
        ("bhf_dock", &["aiv.zrd", "targets.zrd"]),
        ("bhf_hangar", &["aiv.zrd", "targets.zrd"]),
        ("pwr_station", &["aiv.zrd"]),
        ("pwr_engines", &["targets.zrd"]),
        ("aagun99", &["targets.zrd"]),
        ("balloon_t1", &["targets.zrd"]),
        ("t_truck01", &["targets.zrd"]),
        ("gasbag1", &["zeppelins.zrd"]),
        ("reng11", &["zeppelins.zrd"]),
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

    let declarations: Vec<(String, Vec<String>)> = archive_members
        .iter()
        .filter(|member| member.as_str() != CONTROL_MEMBER)
        .map(|member| (member.clone(), member_texts(CONTAINER, member)))
        .collect();
    assert_eq!(declarations.len(), 12);

    for (name, declaring) in own {
        assert!(
            named.contains(name),
            "the control record really spells {name:?}, so the table is not vacuous"
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
            "{name:?} is declared by exactly these members of M19's archive"
        );
    }

    // The same question in the chapter-4 world container and the shared bank:
    // the transfer and extraction animation names, and the fortress members
    // the INACTIVE chains address.
    let world: [(&str, &str, &[&str]); 9] = [
        (
            "bm_hookup_player",
            "bhmhookup.zrd",
            &["activate_bmhookup_node", "bm_hookup_player"],
        ),
        ("activate_bmhookup_node", "bhmhookup.zrd", &[]),
        ("launch_warhawk", "bhm_warhawks.zrd", &[]),
        ("launch_autogyro", "bhm_warhawks.zrd", &[]),
        ("launch_brigand", "bhm_warhawks.zrd", &[]),
        ("bhf_dockdoors", "bhf_dockdoors.zrd", &[]),
        ("bhf_heliumtank1", "bhf_docktankboom.zrd", &[]),
        ("tank1_healthy", "bhf_docktankboom.zrd", &[]),
        ("rail", "bhf_hangarboom.zrd", &[]),
    ];
    for (name, member, _) in world {
        assert!(named.contains(name), "the record spells {name:?}");
        let texts = member_texts(WORLD_CONTAINER, member);
        assert!(
            texts.iter().any(|text| text == name),
            "the chapter world's {member} declares {name:?}"
        );
    }
    let shared: [(&str, &str); 4] = [
        ("hooked_to_klondike", "pzep_hookup.zrd"),
        ("pzhomebase", "pzep_hookup.zrd"),
        ("pzep_stop_loading", "pzep_cargo.zrd"),
        ("M4Piratezep", "neindex.zrd"),
    ];
    for (name, member) in shared {
        let container = if member == "neindex.zrd" {
            WORLD_CONTAINER
        } else {
            SHARED_CONTAINER
        };
        assert!(named.contains(name), "the record spells {name:?}");
        let texts = member_texts(container, member);
        assert!(
            texts.iter().any(|text| text == name),
            "{container}'s {member} declares {name:?}"
        );
    }

    // The two measured footnotes.
    for turret in ["b_turret1", "b_turret2"] {
        assert!(named.contains(turret), "the record spells {turret:?}");
        assert!(
            declarations
                .iter()
                .all(|(_, texts)| !texts.iter().any(|text| text == turret)),
            "no member of M19's archive declares {turret:?} literally"
        );
    }
    let aiv = member_texts(CONTAINER, "aiv.zrd");
    assert!(
        aiv.iter().any(|text| text == "b_turret*"),
        "the world-side declaration is the one-digit wildcard"
    );
    assert!(
        !named.contains("klondike"),
        "the record never names the rescued ally as a text"
    );

    // Full coverage: the table is a sample, so ask the same question of every
    // text the record spells. All but five are declared somewhere outside the
    // control member — the exception set is measured over the installation,
    // not chosen.
    assert_eq!(
        named.len(),
        NAMED_TEXTS,
        "the record's sites spell this many distinct texts"
    );
    let declared_elsewhere = installation_texts_outside_control();
    assert!(
        !declared_elsewhere.contains("klondike"),
        "no .zrd member of the installation names the rescued ally either"
    );
    let record_only: Vec<&str> = named
        .iter()
        .filter(|name| !declared_elsewhere.contains(name.as_str()))
        .map(String::as_str)
        .collect();
    assert_eq!(
        record_only,
        [
            "MSG_BRF_RMM4_OBJ1",
            "MSG_BRF_RMM4_OBJ2",
            "MSG_BRF_RMM4_OBJ3",
            "MSG_BRF_RMM4_OBJ4",
            "MSG_BRF_RMM4_OBJ5",
        ],
        "every text the record's sites spell is declared outside M19's control \
         member, except the five briefing-message operands"
    );
}

/// **Both terminal blocks are dormant entries; every block address the record
/// spells is a block of this record.**
///
/// `INSTANTLOSS` sits in block 13 and `INSTANTWIN` in block 30; no other block
/// ends the mission. Both spell `BEGIN_DORMANT -1`, so neither can fire on its
/// own clock. The failure latch is named by exactly four completion edges —
/// the naps of the four worst watcher rungs 12, 16, 21 and 39 — and the
/// success latch by exactly one, block 49's nap. The success latch is itself
/// **evaluated** (the `hooked_to_klondike` predicate), unlike a wake-only
/// latch.
///
/// The address walk covers 147 spelled integers — 41 wake, 47 kill, 56 nap,
/// 3 gates — and every one lies in `1..=108`. The addressing itself is not
/// assumed here: M02-B-FU3 (#802) measured the original's parse decrementing
/// every objective address before storing it, so a spelled number **is** the
/// block number, and M06-B-FU3 (#819) reconciled the sibling suites to it.
/// M19's record discriminates the readings the same way M13's did: block 25
/// spells `108`, the block count itself, and nothing spells `0`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m19_b_the_terminal_blocks_are_gated_and_every_address_is_in_range() {
    let blocks = blocks_of(&control_document());
    let outcomes: Vec<u32> = blocks
        .iter()
        .filter(|(_, directives)| directives.iter().any(|d| d.key.starts_with("INSTANT")))
        .map(|(number, _)| *number)
        .collect();
    assert_eq!(outcomes, [13, 30], "no other block ends the mission");
    for (number, outcome) in [(13, "INSTANTLOSS"), (30, "INSTANTWIN")] {
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
            ("KILL_OBJECTIVE_WHEN_I_COMPLETE", 47),
            ("NAP_OBJECTIVE_WHEN_I_COMPLETE", 56),
            ("TICK_DEPENDS_ON_OBJ", 3),
            ("WAKE_OBJECTIVE_WHEN_I_COMPLETE", 41),
        ]),
        "the address walk visits every spelled address: 41 wake, 47 kill, 56 nap, 3 gates"
    );
    assert!(
        out_of_range.is_empty(),
        "every address is a block of this record: {out_of_range:?}"
    );
    assert_eq!(
        highest,
        i64::from(BLOCKS),
        "block 25 spells 108 — the block count itself"
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
        naming(13),
        [
            (12, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![13]),
            (16, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![13]),
            (21, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![13]),
            (39, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![13]),
        ],
        "the failure latch has exactly four completion edges, all naps"
    );
    let mut win_edges = naming(30);
    win_edges.retain(|(_, key, _)| key != "KILL_OBJECTIVE_WHEN_I_COMPLETE");
    assert_eq!(
        win_edges,
        [(49, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![30])],
        "the success latch has exactly one completion edge: block 49's nap \
         (the kills that also name it are not entries)"
    );
    assert_eq!(
        naming(108),
        [(25, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![108])],
        "the discriminating address: the last block, in range only under the \
         measured rule"
    );
}

/// **The two outcomes have disjoint prerequisites; the other 89 blocks are
/// named by neither latch.**
///
/// "Mandatory" here is stated narrowly, as the data supports: the blocks that
/// must be *entered* before a latch can be entered — every block that wakes or
/// naps its way towards it, plus the dependencies its gates declare. Kill
/// edges are deliberately excluded (killing block 30 does not enter it; the
/// four failure watchers all name it in their kill lists and it is asserted
/// below that its only predecessor is block 49's nap).
///
/// * success (block 30): thirteen blocks — `{5, 6, 7, 17, 18, 19, 22, 23, 24,
///   28, 29, 30, 49}` — the dock chain `5 → 6 → 7 → 17` whose last nap
///   re-arms the `piratezep` watcher 24 (undormant, yet also napped by block
///   17), plus the turret count waking the hookup, the hookup napping the
///   dock response, the payback block gated on 24, the mansion primary, the
///   two escort depletions and the latch itself;
/// * failure (block 13): ten blocks — `{9, 10, 11, 12, 14, 15, 16, 21, 39,
///   13}` — the two ladders, the all-supports watcher, the helium tank and
///   the latch, disjoint from it;
/// * the remaining 85 blocks — the launch waves, the target add/remove
///   staging, the music and dialogue markers — are named by no prerequisite
///   edge of either latch.
///
/// Whether any of those 85 is an *optional reward or stunt branch* is **not**
/// measured: M19-A binds no stunt or reward ids, so nothing here calls one
/// optional in that sense. What is pinned is the condition split: 64 blocks
/// lower to a bare `ObjectiveAwake` — they complete on wake alone — and 44
/// spell an evaluator or a gate.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m19_b_the_two_outcomes_have_disjoint_prerequisites_and_the_rest_is_not_mandatory() {
    let blocks = blocks_of(&control_document());
    let prerequisites = prerequisites(&blocks);

    // Kills are not prerequisites: all four failure watchers kill block 30,
    // but block 30's only predecessor is block 49's nap.
    assert_eq!(prerequisites.get(&30), Some(&vec![49]));
    assert_eq!(prerequisites.get(&13), Some(&vec![12, 16, 21, 39]));
    assert_eq!(
        prerequisites.get(&23),
        Some(&vec![22, 24]),
        "the payback block is entered by the dock response's nap and gated on \
         the zeppelin watcher"
    );

    let win = closure(&prerequisites, 30);
    assert_eq!(
        win,
        [5, 6, 7, 17, 18, 19, 22, 23, 24, 28, 29, 30, 49],
        "the blocks that must be entered before the extraction latch can be \
         entered — including the dock chain 5 → 6 → 7 → 17, whose last nap \
         re-arms the zeppelin watcher 24 that gates the payback block"
    );
    let loss = closure(&prerequisites, 13);
    assert_eq!(
        loss,
        [9, 10, 11, 12, 13, 14, 15, 16, 21, 39],
        "the failure watchers are their own closure"
    );
    assert!(
        win.iter().all(|number| !loss.contains(number)),
        "the two latches have disjoint prerequisites: {win:?} vs {loss:?}"
    );

    let outside: Vec<u32> = (1..=BLOCKS)
        .filter(|number| !win.contains(number) && !loss.contains(number))
        .collect();
    assert_eq!(
        outside.len(),
        85,
        "85 blocks are named by no prerequisite edge of either latch"
    );

    // The condition shapes: 64 blocks complete on wake alone, 44 spell an
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
    assert_eq!(wake_only.len(), 64, "64 blocks complete on wake alone");
    assert_eq!(
        evaluated.len(),
        44,
        "44 blocks spell an evaluator or a gate"
    );
    assert!(
        !wake_only.contains(&30),
        "the success latch is evaluated — the extraction predicate is part \
         of its condition, unlike M13's wake-only latches"
    );
}

/// **Every call binds, every condition lowers and M19's record completes —
/// the census's largest record.**
///
/// All 440 sites bind and all 108 block conditions lower. The bound program
/// reaches `MissionProgram::validate` and validates; no lowering requirement
/// is unmet and M19's census row is complete — and no measured row of the
/// census carries more blocks or more sites. The census as a whole stays not
/// campaign-ready: other missions carry their own gaps.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m19_b_every_call_binds_every_condition_lowers_and_m19s_record_is_the_largest_complete() {
    let census = census();
    let row = census.row(MISSION).unwrap();

    let attempt = row.lowering_attempt().unwrap();
    let lowered = attempt.attempt();
    assert_eq!(lowered.mission.as_deref(), Ok("mission/ch4-m04"));
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
        "a program stands for all 108 objectives"
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
        "all 108 block conditions lower: {refused_conditions:?}"
    );
    assert_eq!(lowered.conditions.len(), BLOCKS as usize);

    // M19 is the census's largest measured record, on both counts.
    let (blocks_max, sites_max) = census
        .measured_rows()
        .filter_map(|row| row.record().map(|record| (record.blocks(), record.sites())))
        .fold((0, 0), |(blocks, sites), (b, s)| {
            (blocks.max(b), sites.max(s))
        });
    assert_eq!(
        (blocks_max, sites_max),
        (BLOCKS, SITES),
        "no measured record is larger than M19's, on blocks or on sites"
    );

    // Retail defaults: the blocks that spell no INACTIVE_COMPLETION_COUNT
    // take their own member count, never 1 and never an invented default.
    let raw = attempt.raw_program().expect("the program assembled");
    assert_eq!(
        raw.objectives[20].condition,
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 20 },
            Condition::InactiveMembers {
                members: vec![
                    vec!["bhf_hangar".to_owned(), "bhf_support1".to_owned()],
                    vec!["bhf_hangar".to_owned(), "bhf_support2".to_owned()],
                    vec!["bhf_hangar".to_owned(), "bhf_support3".to_owned()],
                    vec!["bhf_hangar".to_owned(), "bhf_support4".to_owned()],
                ],
                threshold: 4,
            },
        ]),
        "block 21 spells no count: the failure threshold is all four supports"
    );
    assert_eq!(
        raw.objectives[23].condition,
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 23 },
            Condition::InactiveMembers {
                members: vec![vec!["piratezep".to_owned()]],
                threshold: 1,
            },
        ]),
        "block 24: one member, threshold one"
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

/// **M19 is a complete census row; the campaign is still not ready.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m19_b_m19_is_complete_and_the_campaign_stays_unready() {
    let census = census();
    assert!(
        census.complete_missions().contains(&MISSION),
        "M19 joins the census's complete rows"
    );
    assert!(
        !census.campaign_ready(),
        "the campaign is still not ready — other missions carry their own gaps"
    );
    assert!(census.measured_rows().any(|row| row.mission() == MISSION));
    let row = census.row(MISSION).unwrap();
    assert!(row.is_measured(), "the program is measured");
    assert!(row.is_complete(), "M19's row is complete");
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
        ContentId::from_source(ContentKind::Mission, "accept-m19-b").map_err(|e| e.to_string()),
        "accept-m19-b",
        document,
        &record,
    )
}

/// M19's own `ANIM_STATE` spelling (blocks 19 and 30): one `ANIM` descriptor
/// record `[NAME [<name>], STATE [EXECUTED]]`.
fn hookup_operands(name: &str, state: &str) -> Vec<ZrdValue> {
    vec![
        text("ANIM"),
        ZrdValue::List(vec![
            text("NAME"),
            ZrdValue::List(vec![text(name)]),
            text("STATE"),
            ZrdValue::List(vec![text(state)]),
        ]),
    ]
}

/// **The hookup `ANIM_STATE` lowers as the one-pair predicate; a second site
/// is inert and an unmeasured state token drops its pair.**
///
/// Measured (finding C): the parse's helper runs once per block and takes the
/// **first** `ANIM_STATE` text in the block's depth-first order — a later
/// site is never reached, which is M19's answer to a repeated event at the
/// authoring level. A `STATE` token outside `RUNNING`/`EXECUTED`/`INVALID`
/// leaves the pair unappended, so an evaluator built on it wants zero of zero
/// animations.
#[test]
fn accept_m19_b_the_hookup_anim_state_lowers_and_a_second_site_is_inert() {
    let record_of = |sites: Vec<Vec<ZrdValue>>| {
        let mut directives = vec![site("BEGIN_DORMANT", vec![ZrdValue::Float(-1.0)])];
        directives.extend(sites);
        control_record(vec![block_site(1, directives)])
    };
    let condition_of = |sites: Vec<Vec<ZrdValue>>| {
        lower(&record_of(sites))
            .raw_program()
            .expect("the program assembled")
            .objectives[0]
            .condition
            .clone()
    };

    // M19's own spelling: the hookup completes when the animation executed.
    assert_eq!(
        condition_of(vec![site(
            "ANIM_STATE",
            hookup_operands("bm_hookup_player", "EXECUTED")
        )]),
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 0 },
            Condition::AnimationStates {
                required: 1,
                animations: vec![("bm_hookup_player".to_owned(), AnimationState::Executed)],
            },
        ]),
        "the authored hookup site lowers M19's own way"
    );

    // A repeated site is never reached: the first ANIM_STATE text wins.
    assert_eq!(
        condition_of(vec![
            site(
                "ANIM_STATE",
                hookup_operands("bm_hookup_player", "EXECUTED")
            ),
            site(
                "ANIM_STATE",
                hookup_operands("hooked_to_klondike", "RUNNING")
            ),
        ]),
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 0 },
            Condition::AnimationStates {
                required: 1,
                animations: vec![("bm_hookup_player".to_owned(), AnimationState::Executed)],
            },
        ]),
        "the second site is inert — the measured lookup takes the first only"
    );

    // An unmeasured state token drops its pair rather than arming a fourth
    // state or refusing the block.
    assert_eq!(
        condition_of(vec![site(
            "ANIM_STATE",
            hookup_operands("bm_hookup_player", "PAUSED")
        )]),
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 0 },
            Condition::AnimationStates {
                required: 0,
                animations: vec![],
            },
        ]),
        "an out-of-vocabulary state appends no pair — zero of zero"
    );
}

/// **A spelled `0` dependency arms no gate; the wildcard turret names arrive
/// as data.**
///
/// Two boundaries the record sits next to. `TICK_DEPENDS_ON_OBJ`'s measured
/// parse stores `child0 − 1` unconditionally, so a spelled `0` lands on the
/// record's "no dependency" sentinel — the same as not spelling the key —
/// while M19's own `24` gates on the watcher at index 23. And the turret
/// wakes carry their wildcard spellings — `aagun**`, `t_truck**`, `b_turret*`
/// — into the bound call as data: the one-digit-consuming match is a runtime
/// rule of the turret registry, and nothing in the lowering resolves it.
#[test]
fn accept_m19_b_a_zero_dependency_arms_no_gate_and_the_wildcards_arrive_as_data() {
    let gated = |dependency: u32| {
        let document = control_record(vec![
            block_site(
                1,
                vec![
                    site("BEGIN_DORMANT", vec![ZrdValue::Float(-1.0)]),
                    site("TICK_DEPENDS_ON_OBJ", vec![int(dependency)]),
                ],
            ),
            block_site(2, vec![site("BEGIN_DORMANT", vec![ZrdValue::Float(-1.0)])]),
        ]);
        lower(&document)
            .raw_program()
            .expect("the program assembled")
            .objectives[0]
            .condition
            .clone()
    };
    assert_eq!(
        gated(0),
        Condition::ObjectiveAwake { index: 0 },
        "a spelled 0 is the no-dependency sentinel, not a gate on block 0"
    );
    assert_eq!(
        gated(1),
        Condition::All(vec![
            Condition::ObjectiveAwake { index: 0 },
            Condition::ObjectiveAwake { index: 0 },
        ]),
        "a spelled 1 gates on the first block — child0 − 1, never the \
         spelled number itself"
    );

    // The wildcard spellings are carried as authored data.
    let document = control_record(vec![block_site(
        1,
        vec![
            site("BEGIN_DORMANT", vec![ZrdValue::Float(-1.0)]),
            site(
                "WAKEUP_TURRETS",
                vec![text("aagun**"), text("8igun**"), text("b_turret*")],
            ),
            site("WAKEUP_TURRETS", vec![text("t_truck**")]),
        ],
    )]);
    let lowered = lower(&document);
    assert!(
        lowered
            .attempt()
            .calls
            .iter()
            .all(|call| matches!(call, CallOutcome::Bound)),
        "both wildcard sites bind: {:?}",
        lowered.attempt().calls
    );
    let raw = lowered.raw_program().expect("the program assembled");
    let wildcards: Vec<&[Value]> = raw.objectives[0]
        .calls
        .iter()
        .filter(|call| call.name == "WAKEUP_TURRETS")
        .map(|call| call.args.as_slice())
        .collect();
    assert_eq!(
        wildcards,
        [
            &[
                Value::Str("aagun**".to_owned()),
                Value::Str("8igun**".to_owned()),
                Value::Str("b_turret*".to_owned())
            ][..],
            &[Value::Str("t_truck**".to_owned())][..],
        ],
        "the one- and two-digit wildcard spellings arrive as spelled data — \
         including the two-star forms M19 adds over M12's single-star site"
    );
}

/// **A directive this build measures no effect for is refused, not honoured.**
///
/// The transfer half of the sheet's priorities has no directive in M19's
/// record — the hookup is spelled as animation state and target flags. If a
/// dedicated transfer key were spelled anyway — an authored
/// `TRANSFER_PLAYER_TO_HOOK` here — the production measurement reports it
/// unmeasured and the lowering refuses the site by name instead of quietly
/// doing nothing. That is the guard against an implementation "supporting" a
/// transfer the shipped data never spells, and against a guessed key ever
/// being treated as measured.
#[test]
fn accept_m19_b_an_ungrounded_transfer_directive_is_refused_rather_than_honoured() {
    let document = control_record(vec![block_site(
        1,
        vec![
            site("BEGIN_DORMANT", vec![ZrdValue::Float(-1.0)]),
            site(
                "TRANSFER_PLAYER_TO_HOOK",
                vec![text("player"), text("bm_hook")],
            ),
        ],
    )]);
    let record = measure_control_record(&document);
    let key = record
        .key("TRANSFER_PLAYER_TO_HOOK")
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
            .any(|(measured, _)| measured.key == "TRANSFER_PLAYER_TO_HOOK"),
        "the record's own unmeasured walk carries it"
    );

    let lowered = lower(&document);
    assert_eq!(
        lowered.attempt().mission.as_deref(),
        Ok("mission/accept-m19-b"),
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
        refused[0].contains("unknown host call `TRANSFER_PLAYER_TO_HOOK`"),
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
