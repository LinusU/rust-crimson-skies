//! Acceptance stage M12-B: M12's mission-specific compatibility surface — the
//! mission control program the installation ships for *The Great Plane
//! Robbery*, bound through production engine systems and regressed against
//! the lowering that decides what the engine may honour
//! (`missions/M12.md`, work order `M12-B`).
//!
//! Shared contract: `docs/contracts/SCRIPT-MISSION.md` ("Source adapter
//! acceptance", "Host interface", "Objective event ordering"). Findings:
//! `docs/findings/2026-10-10-m12-b-compatibility-gaps.md`.
//!
//! # What this stage adds, and what it deliberately does not
//!
//! M12-A bound *which* retail mission the work order names
//! (`mission/ch3-m02`, program `script/c2-m01-zrdr`, the reader archive
//! `ZBD/C2/M01/zrdr.zbd`; the census row `zbd/c2/m01`) and left every
//! objective, actor and directive unbound. This stage binds the archive
//! through the production systems M02-B built
//! (`SourceContext::control_program`) and measures it a second time through
//! `cs_app::mission_control`, so three independent derivations — the mission
//! binding, the control binding and the retail census — must agree before
//! any assertion below can pass.
//!
//! The stage's minimum acceptance scenario is *"All discovered
//! mission-specific behavior uses production engine systems and regression
//! tests."* M12's discovered mission-specific behavior is its control
//! record: 82 numbered blocks, 265 directive sites, 26 distinct keys, the
//! block graph those spell, and the lowering that decides the engine may
//! honour all of it. What is different at M12:
//!
//! * **It spells both terminal outcomes.** `INSTANTWIN` (OBJECTIVE47) and
//!   `INSTANTLOSS` (OBJECTIVE51) are the record's only outcome latches, one
//!   site each, both dormant at start. Completion of OBJECTIVE50 (four of the
//!   eight spelled `g_engine*` members counted by `INACTIVE_COMPLETION_COUNT`)
//!   both kills OBJECTIVE47 and naps OBJECTIVE51 awake.
//! * **No condition is a counting site.** The record spells no `TRAVELERS`
//!   and no `WAKEUP_TURRETS`; its conditions are animation states, member
//!   health and the inactive-completion counts.
//! * **It spells `DEDG`, `SET_AI_NET`, `WAKEUP_ENEMIES` and
//!   `WAKEUP_GENERATOR`** — directive keys the other mission pins here do not
//!   spell — all of which carry a measured disposition and lower.
//!
//! Nothing here is `verified_original` (AGENTS.md rule 8): the
//! work-order ↔ mission join remains M12-A's inference, directive *effects*
//! are the M01-LC findings' static readings of the original code, and no
//! original executable has been run. The wrong-actor, wrong-session and
//! repeated-event halves of the sheet's three priorities (door and route
//! clearance, oversized collision, escort survival) are runtime
//! observations: what this stage pins is the *spelled data* they are built
//! from — targets, members, edges and radii — never a timing or a verdict
//! the record does not carry.
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`, so CI (which
//! has no original data) skips them and the implementing and reviewing
//! agents run them with `--include-ignored`. The synthetic tests build `.zrd`
//! values tag by tag — no original game data is committed — and run in CI.

use std::path::PathBuf;
use std::sync::OnceLock;

use cs_app::control_lowering::lower_control_record;
use cs_app::mission_control::{RetailControlRow, survey_mission_control_programs};
use cs_assets::install::sha256;
use cs_content::campaign_bindings::{MissionLabel, SourceContext};
use cs_content::mission_control::{
    AnimList, CallOutcome, ConditionOutcome, ControlRecordField, DecodedMember,
    DirectiveDisposition, TerminalOutcome, measure_control_record, objective_blocks_of,
    terminal_outcome_of,
};
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{ZrdValue, decode_zrd, objective_record, zrd_flat_fields};
use cs_formats::script_raw::discover_container;
use cs_script::ir::{Condition, TravelersAnchor, Value};
use cs_types::content::{ContentId, ContentKind};
use cs_types::install::RelativePath;

use crate::campaign::common::{label, load_inventory};

/// The census row label of the mission: the mission-scoped reader archive
/// F13-B's rule derives from the installation.
const MISSION: &str = "zbd/c2/m01";

/// The numbered blocks of the control member.
const BLOCKS: u32 = 82;
/// The directive sites of the control member.
const SITES: u32 = 265;
/// The distinct directive keys of the control member.
const KEYS: usize = 26;
/// The cross-objective addresses the directed keys spell (wake, kill, nap
/// and dependency-gate lists).
const EDGES: usize = 107;

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M12-B needs the retail capability; run this suite with \
             `--include-ignored` and CS_GAME_DIR pointing at the read-only installation"
        )
    }))
}

/// The source context, read once for the whole suite (fingerprinting the
/// installation walks every file, so it happens exactly once).
fn context() -> &'static SourceContext {
    static CONTEXT: OnceLock<SourceContext> = OnceLock::new();
    CONTEXT.get_or_init(|| {
        SourceContext::read(&game_dir()).expect("the installation yields a source context")
    })
}

/// M12's work-order label, from the committed inventory rather than a
/// literal.
fn m12() -> MissionLabel {
    label("M12")
}

/// M12's declared discovery title, from the committed inventory.
fn m12_title() -> String {
    load_inventory()
        .iter()
        .find(|(work_order, _)| work_order.as_str() == "M12")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M12 work order")
}

/// M12's control binding, derived fresh through production code.
fn control_binding() -> cs_content::campaign_bindings::MissionControlBinding {
    context()
        .control_program(m12(), &m12_title())
        .expect("M12's control program binds through the measured rule")
}

/// M12's row in the retail control census: the same installation measured a
/// second time through `cs_app::mission_control`.
fn census_row() -> &'static RetailControlRow {
    static ROW: OnceLock<RetailControlRow> = OnceLock::new();
    ROW.get_or_init(|| {
        let census = survey_mission_control_programs(&game_dir())
            .expect("the installation measures a control census");
        census
            .row(MISSION)
            .expect("M12's reader archive is measured by the census")
            .clone()
    })
}

/// The census, built once for the whole suite.
fn census() -> &'static cs_app::mission_control::RetailControlCensus {
    static CENSUS: OnceLock<cs_app::mission_control::RetailControlCensus> = OnceLock::new();
    CENSUS.get_or_init(|| {
        survey_mission_control_programs(&game_dir()).expect("the census runs on the installation")
    })
}

/// The control member's decoded document, re-read from the archive through
/// production discovery — an independent walk from the binding's, so the
/// graph assertions below cannot be satisfied by the binding's own output.
fn control_document() -> (ZrdValue, Vec<(String, u64, u64, u32)>) {
    let binding = control_binding();
    let bytes = std::fs::read(game_dir().join(&binding.program_asset))
        .expect("M12's reader archive reads from disk");
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
        let decoded = decode_zrd(program.bytes())
            .unwrap_or_else(|error| panic!("member {name} decodes: {error}"));
        let span = program.locator().span();
        members.push((
            name.to_owned(),
            span.offset,
            span.len,
            objective_blocks_of(&DecodedMember::new(name, decoded.clone())),
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

/// One numbered block of a decoded control record, as the graph walk reads
/// it: its key (the record's own `OBJECTIVE<N>` spelling, which is also the
/// address other blocks spell for it) and its directive sites in spelling
/// order.
struct Block {
    key: String,
    /// `(directive key, argument values beside it)`; an empty argument vec
    /// is the measured bare spelling.
    sites: Vec<(String, Vec<ZrdValue>)>,
}

/// Walks every numbered `OBJECTIVE<N>` block of a decoded control record in
/// record order, pairing each directive key with the list beside it exactly
/// as the measured grammar reads sites.
fn blocks_of(document: &ZrdValue) -> Vec<Block> {
    let mut blocks = Vec::new();
    for (key, value) in zrd_flat_fields(objective_record(document)) {
        let Some(number) = objective_block_number(key) else {
            continue;
        };
        let mut sites = Vec::new();
        let Some(children) = value.as_list() else {
            blocks.push(Block {
                key: format!("OBJECTIVE{number}"),
                sites,
            });
            continue;
        };
        let mut cursor = 0;
        while cursor < children.len() {
            let Some(name) = children[cursor].as_text() else {
                break;
            };
            match children.get(cursor + 1) {
                Some(next) if next.as_list().is_some() => {
                    sites.push((name.to_owned(), next.as_list().unwrap_or_default().to_vec()));
                    cursor += 2;
                }
                Some(next) if next.as_text().is_some() => {
                    // A text follower is the next directive's key.
                    sites.push((name.to_owned(), Vec::new()));
                    cursor += 1;
                }
                Some(_) => {
                    // A scalar follower: measured `not_a_list`. Keep the site
                    // so the walk accounts for it, with the scalar carried as
                    // a one-element argument list.
                    sites.push((name.to_owned(), vec![children[cursor + 1].clone()]));
                    cursor += 2;
                }
                None => {
                    sites.push((name.to_owned(), Vec::new()));
                    cursor += 1;
                }
            }
        }
        blocks.push(Block {
            key: format!("OBJECTIVE{number}"),
            sites,
        });
    }
    blocks
}

/// The integer arguments of one directive site.
fn integers(args: &[ZrdValue]) -> Vec<i64> {
    args.iter()
        .filter_map(|value| match value {
            ZrdValue::Int(int) => Some(i64::from(*int)),
            _ => None,
        })
        .collect()
}

/// The text arguments of one directive site, in spelling order.
fn texts(args: &[ZrdValue]) -> Vec<&str> {
    args.iter()
        .filter_map(ZrdValue::as_text)
        .collect::<Vec<_>>()
}

// ---------------------------------------------------------------------------
// Retail: the binding ties M12's control program to M12's identities
// ---------------------------------------------------------------------------

/// **M12's control program is bound to the same identities as its mission
/// binding.** `SourceContext::control_program` resolves the work order
/// through the same title join `SourceContext::bind` uses, so the two
/// derivations name one mission, one program and one archive; the member the
/// binding cites is the member the measured rule picked over the archive's
/// whole member set, with a digest over that member's own bytes; and the
/// retail census — a third derivation through its own discovery path —
/// measured the same archive, the same member and the same record.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m12_b_m12s_control_program_is_bound_to_the_same_identities_as_its_mission_binding() {
    let binding = control_binding();
    let mission_binding = context()
        .bind(m12(), &m12_title())
        .expect("M12's mission binding resolves");

    assert_eq!(
        binding.mission.as_str(),
        mission_binding
            .catalog_id
            .expect("the mission binding resolves a mission id")
            .as_str(),
        "the control binding and the mission binding name the same mission"
    );
    assert_eq!(
        binding.mission.as_str(),
        "mission/ch3-m02",
        "M12 is the second mission of chapter 3, as M12-A bound it"
    );
    assert_eq!(
        mission_binding.campaign_position,
        Some(11),
        "the join selected campaign position 11, the twelfth mission"
    );
    assert_eq!(
        binding.program_id,
        mission_binding
            .program_id
            .expect("the mission binding resolves a program identity"),
        "the control binding and the mission binding cite one program"
    );
    assert_eq!(
        binding.program_id.as_str(),
        "script/c2-m01-zrdr",
        "the program identity is the reader archive the layout declares at position 11 \
         (`ZBD/C2/M01`, the first mission of the second world group)"
    );

    // The archive is the one the campaign layout declares at that position,
    // and the whole-file digest re-derives from the bytes on disk.
    let position = mission_binding
        .campaign_position
        .expect("a position was resolved");
    let entry = &context().campaign()[position];
    assert!(
        binding
            .program_asset
            .eq_ignore_ascii_case(&entry.program_asset),
        "the control binding reads the archive the layout declares at position \
         {position}: {} vs {}",
        binding.program_asset,
        entry.program_asset
    );
    assert!(entry.program_present, "the archive is present on disk");
    let bytes = std::fs::read(game_dir().join(&binding.program_asset))
        .expect("M12's reader archive reads from disk");
    assert_eq!(
        binding.program_length,
        bytes.len() as u64,
        "the bound length is the archive's own"
    );
    assert_eq!(
        binding.program_sha256,
        sha256(&bytes).to_hex(),
        "the bound digest re-derives from the archive's bytes"
    );

    // The rule chose the member: exactly one member declares numbered
    // blocks, it is the one the binding names, and its byte range digests
    // to the bound member digest.
    let block_carriers: Vec<&str> = binding
        .members
        .iter()
        .filter(|row| row.objective_blocks > 0)
        .map(|row| row.name.as_str())
        .collect();
    assert_eq!(
        block_carriers,
        [binding.control_member.as_str()],
        "exactly one member declares numbered objective blocks, and the rule \
         picked it — the members the rule judged are {:?}",
        binding
            .members
            .iter()
            .map(|row| (row.name.as_str(), row.objective_blocks))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        binding.control_member.to_lowercase(),
        "objectives.zrd",
        "on this installation the rule picks the member production discovery \
         spells `objectives.zrd` — as a *result* of the rule, not as its input"
    );
    assert_eq!(
        binding.members.len(),
        15,
        "M12's archive offers fifteen members"
    );
    let row = binding.control_row().expect("the chosen member has a row");
    let end = (row.offset + row.len) as usize;
    assert!(
        end <= bytes.len(),
        "the member's range lies inside the archive"
    );
    assert_eq!(
        binding.control_sha256,
        sha256(&bytes[row.offset as usize..end]).to_hex(),
        "the member digest re-derives from the member's own bytes"
    );
    assert_eq!(
        binding.control_offset, row.offset,
        "the binding's member span is the row's own"
    );

    // Size and position are measurably not the rule: a member longer than
    // the control member exists, and the control member is not the first.
    let longest = binding
        .members
        .iter()
        .max_by_key(|member| member.len)
        .expect("the archive declares members");
    assert!(
        longest.len > row.len,
        "a member longer than the control member is offered ({} > {} bytes), so \
         length cannot be the selection",
        longest.len,
        row.len
    );
    assert_ne!(
        binding.members.first().map(|member| member.name.as_str()),
        Some(binding.control_member.as_str()),
        "the control member is not the archive's first member, so position \
         cannot be the selection either"
    );

    // The census measured the same archive through its own discovery path.
    let census_row = census_row();
    assert_eq!(
        binding.program_asset.to_lowercase(),
        census_row.container.to_lowercase(),
        "the control binding and the census read the same reader archive"
    );
    assert_eq!(
        binding.program_sha256, census_row.container_sha256,
        "both derivations digest the same archive"
    );
    assert_eq!(
        binding.control_member.to_lowercase(),
        census_row
            .members
            .iter()
            .find(|member| member.is_control)
            .expect("the census marks one control member")
            .name
            .to_lowercase(),
        "the rule picked the same member for both derivations"
    );
    assert_eq!(
        Some(&binding.record),
        census_row.record(),
        "the two derivations measured the same directive record"
    );
    assert_eq!(
        binding.members.len(),
        census_row.members.len(),
        "both derivations account for every member the archive offers"
    );
    // …and the document re-read through discovery carries the same blocks,
    // so the graph walk below cannot be satisfied by a cached measurement.
    let (document, members) = control_document();
    assert_eq!(members.len(), binding.members.len());
    assert_eq!(
        blocks_of(&document).len() as u32,
        binding.record.blocks(),
        "the independent walk sees every numbered block the measurement counted"
    );
}

/// **M12's directive vocabulary partitions exactly and refuses no key — and
/// it carries one win latch and one loss latch.** Every key M12 spells
/// carries a measured disposition except its two terminal spellings:
/// `INSTANTWIN` and `INSTANTLOSS`, one site each. No block is unreadable,
/// and no record-level key falls outside the measured record vocabulary.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m12_b_the_measured_vocabulary_partitions_and_refuses_no_m12_key() {
    let binding = control_binding();
    let record = &binding.record;

    assert_eq!(
        (record.blocks(), record.sites()),
        (BLOCKS, SITES),
        "M12 declares {BLOCKS} numbered blocks and {SITES} directive sites"
    );
    assert_eq!(
        record.keys().iter().map(|key| key.sites).sum::<u32>(),
        record.sites(),
        "every measured site belongs to exactly one key"
    );
    assert_eq!(
        record.vocabulary() as usize,
        KEYS,
        "M12 spells {KEYS} distinct directive keys"
    );
    assert!(
        record.refusals().is_empty(),
        "the measured directive grammar parses every M12 block: {:?}",
        record.refusals()
    );

    let implemented: Vec<(&str, TerminalOutcome)> = record
        .implemented()
        .into_iter()
        .map(|(key, outcome)| (key.key.as_str(), outcome))
        .collect();
    assert_eq!(
        implemented,
        [
            ("INSTANTLOSS", TerminalOutcome::Failed),
            ("INSTANTWIN", TerminalOutcome::Succeeded)
        ],
        "M12's only implemented directives are its two outcome spellings"
    );
    for key in ["INSTANTWIN", "INSTANTLOSS"] {
        assert_eq!(
            record.key(key).map(|spelled| spelled.sites),
            Some(1),
            "{key} is spelled at exactly one site"
        );
    }
    assert_eq!(
        binding.unmeasured_keys(),
        Vec::<String>::new(),
        "every key M12 spells is covered by the M01-LC findings"
    );
    assert_eq!(
        record.measured().len() + implemented.len(),
        record.vocabulary() as usize,
        "measured + implemented partitions M12's vocabulary"
    );
    for (key, directive) in record.measured() {
        assert!(
            !directive.summary.is_empty()
                && !directive.evidence.is_empty()
                && !directive.operation.code().is_empty(),
            "{}: a measured disposition names the operation, the effect and the \
             evidence",
            key.key
        );
        assert!(
            !matches!(key.disposition(), DirectiveDisposition::Unmeasured { .. }),
            "{}: measured is not unmeasured",
            key.key
        );
    }

    // The outcome keys are bare spellings and answer only for their own
    // names; no other key in M12's vocabulary is spelled bare.
    for key in ["INSTANTWIN", "INSTANTLOSS"] {
        let spelled = record.key(key).expect("M12 spells its outcome keys");
        assert!(
            spelled
                .agreed_shape()
                .is_some_and(|shape| shape.label() == "bare"),
            "{key} is spelled bare in M12"
        );
        assert!(terminal_outcome_of(key).is_some(), "{key} is an outcome");
    }
    let bare: Vec<&str> = record
        .keys()
        .iter()
        .filter(|key| {
            key.agreed_shape()
                .is_some_and(|shape| shape.label() == "bare")
        })
        .map(|key| key.key.as_str())
        .collect();
    assert_eq!(
        bare,
        ["INSTANTLOSS", "INSTANTWIN"],
        "in M12 the only bare keys spell the two outcomes"
    );

    // The record-level fields: the five measured keys, each once, and no
    // record-level key outside the vocabulary.
    let fields: Vec<(ControlRecordField, u32)> = record.record_fields().to_vec();
    assert_eq!(
        fields,
        [
            (ControlRecordField::MissionTimer, 1),
            (ControlRecordField::PlayerInit, 1),
            (ControlRecordField::AnimList(AnimList::Restore), 1),
            (ControlRecordField::AnimList(AnimList::Execute), 1),
            (ControlRecordField::AnimList(AnimList::Invalidate), 1),
        ],
        "M12's record carries the five measured record fields, each once"
    );
    assert!(
        binding.unclassified_record_keys().is_empty(),
        "M12 spells no record-level key outside the measured vocabulary"
    );
    assert_eq!(
        record.record_sounds(),
        Vec::<(cs_content::mission_control::ControlRecordSound, u32)>::new(),
        "M12's record spells none of the record-level sound keys"
    );

    // The exact key list: pinning it is also the proof that the vocabulary
    // carries no collision, damage or interaction directive — the families
    // the sheet's priorities might have lived in are not in this member at
    // all, which the priorities test below relies on.
    let mut keys: Vec<&str> = record.keys().iter().map(|key| key.key.as_str()).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "ADD_OBJECTIVE_TARGET",
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
            "INACTIVE7",
            "INACTIVE8",
            "INACTIVE_COMPLETION_COUNT",
            "INSTANTLOSS",
            "INSTANTWIN",
            "KILL_OBJECTIVE_WHEN_I_COMPLETE",
            "NAP_OBJECTIVE_WHEN_I_COMPLETE",
            "REMOVE_OBJECTIVE_TARGET",
            "SET_AI_NET",
            "STOP_QUEUED_SOUNDS",
            "WAKEUP_ENEMIES",
            "WAKEUP_GENERATOR",
            "WAKEUP_SOUND_GROUP",
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
        ],
        "M12's whole directive vocabulary, sorted byte-wise, so an added or \
         dropped key fails here"
    );
}

// ---------------------------------------------------------------------------
// Retail: the block graph, and the lowering
// ---------------------------------------------------------------------------

/// **Every cross-objective address M12 spells names a block this record
/// declares, both outcome latches are gated by the chain the record spells,
/// and OBJECTIVE50 is the one block that reaches both.**
///
/// The record declares `OBJECTIVE1` … `OBJECTIVE82`; the three
/// cross-objective keys M12 spells (wake, kill and nap) carry {EDGES}
/// addresses, none zero, none negative and none past the last block — the
/// largest value M12 spells *is* the block count. A spelled address names the
/// block it decrements to (the parse stores `address - 1`; that rule, and the
/// refusal it raises past the count, are M02-B-FU3's measurement of the
/// original — Rally #802 — not re-measured here), so under it every address
/// resolves to a block the record declares and M12 carries **no** out-of-range
/// address. The wrong-actor, wrong-session and repeated-event halves of the
/// sheet's priorities are runtime observations and stay unmeasured (M12-C).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m12_b_the_block_graph_is_closed_under_the_records_own_numbering() {
    let binding = control_binding();
    let (document, _) = control_document();
    let blocks = blocks_of(&document);
    assert_eq!(
        blocks.len() as u32,
        binding.record.blocks(),
        "the independent walk sees every numbered block the measurement counted"
    );
    let numbers: Vec<u32> = (1..=BLOCKS).collect();
    let spelled: Vec<u32> = blocks
        .iter()
        .map(|block| {
            block
                .key
                .trim_start_matches("OBJECTIVE")
                .parse::<u32>()
                .expect("a block key carries its number")
        })
        .collect();
    assert_eq!(spelled, numbers, "numbered 1..={BLOCKS}, no gaps");

    // The three cross-objective keys M12 spells (it spells no `SLEEP_…`, no
    // bare `WAKE_OBJECTIVE`, no `HIDE_OBJ` and no `TICK_DEPENDS_ON_OBJ`), and
    // every integer their sites address.
    let directed = [
        "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
        "KILL_OBJECTIVE_WHEN_I_COMPLETE",
        "NAP_OBJECTIVE_WHEN_I_COMPLETE",
    ];
    for absent in [
        "SLEEP_OBJECTIVE_WHEN_I_COMPLETE",
        "WAKE_OBJECTIVE",
        "HIDE_OBJ",
        "TICK_DEPENDS_ON_OBJ",
    ] {
        assert!(
            binding.record.key(absent).is_none(),
            "M12 does not spell {absent}"
        );
    }
    let mut addresses: Vec<i64> = Vec::new();
    let mut sites = 0;
    for block in &blocks {
        for (key, args) in &block.sites {
            if directed.contains(&key.as_str()) {
                sites += 1;
                addresses.extend(integers(args));
            }
        }
    }
    assert_eq!(
        sites as u32,
        directed
            .iter()
            .map(|key| {
                binding
                    .record
                    .key(key)
                    .unwrap_or_else(|| panic!("M12 spells {key}"))
                    .sites
            })
            .sum::<u32>(),
        "the walk visits every cross-objective site the measurement counted"
    );
    assert_eq!(
        addresses.len(),
        EDGES,
        "M12 spells {EDGES} cross-objective addresses"
    );

    // Every address names a block this record declares: none is zero or
    // negative, none is past the last block, so `address - 1` — the index
    // the parse stores — always lands inside the record's own 82 blocks.
    assert!(
        addresses
            .iter()
            .all(|address| (1..=i64::from(BLOCKS)).contains(address)),
        "every address lies in 1..={BLOCKS}, so every one resolves to a block \
         the record declares: {:?}",
        addresses
            .iter()
            .filter(|address| !(1..=i64::from(BLOCKS)).contains(address))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        addresses.iter().copied().min(),
        Some(5),
        "the lowest address M12 spells names OBJECTIVE5"
    );
    assert_eq!(
        addresses.iter().copied().max(),
        Some(i64::from(BLOCKS)),
        "the highest address M12 spells names the last declared block — the \
         record's own boundary is live and nothing crosses it"
    );

    // **Both outcome latches are gated.** Exactly two blocks spell an
    // outcome: the success latch and the failure latch.
    let number_of = |block: &Block| -> u32 {
        block
            .key
            .trim_start_matches("OBJECTIVE")
            .parse::<u32>()
            .expect("a block key carries its number")
    };
    let outcomes: Vec<(u32, &str)> = blocks
        .iter()
        .flat_map(|block| {
            block
                .sites
                .iter()
                .filter(|(key, _)| key.starts_with("INSTANT"))
                .map(|(key, _)| (number_of(block), key.as_str()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(
        outcomes,
        [(47, "INSTANTWIN"), (51, "INSTANTLOSS")],
        "OBJECTIVE47 is the success latch and OBJECTIVE51 the failure latch — \
         the only blocks that end the mission"
    );
    for latch in [47, 51] {
        let block = blocks
            .iter()
            .find(|block| number_of(block) == latch)
            .expect("the latch block is declared");
        assert!(
            block
                .sites
                .iter()
                .any(|(key, args)| key == "BEGIN_DORMANT"
                    && args.as_slice() == [ZrdValue::Float(-1.0)]),
            "OBJECTIVE{latch} starts dormant with no timed wake"
        );
    }

    // Completion edges into each latch. The success latch is napped awake by
    // OBJECTIVE46 and *killed* by OBJECTIVE50; the failure latch is napped
    // awake by OBJECTIVE50 alone. The chain the record spells is
    // `68 →wake→ 46 →nap→ 47` for success and `50 →nap→ 51` for failure.
    let incoming = |target: i64| -> Vec<(u32, String)> {
        blocks
            .iter()
            .flat_map(|block| {
                block
                    .sites
                    .iter()
                    .filter(|(key, args)| {
                        directed.contains(&key.as_str()) && integers(args).contains(&target)
                    })
                    .map(|(key, _)| (number_of(block), key.clone()))
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    assert_eq!(
        incoming(47),
        [
            (46, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned()),
            (50, "KILL_OBJECTIVE_WHEN_I_COMPLETE".to_owned())
        ],
        "OBJECTIVE47 is napped by OBJECTIVE46 and named in OBJECTIVE50's kill list"
    );
    assert_eq!(
        incoming(51),
        [(50, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned())],
        "only OBJECTIVE50's completion naps the failure latch awake"
    );
    assert_eq!(
        incoming(46),
        [
            (50, "KILL_OBJECTIVE_WHEN_I_COMPLETE".to_owned()),
            (68, "WAKE_OBJECTIVE_WHEN_I_COMPLETE".to_owned())
        ],
        "OBJECTIVE46 is woken by OBJECTIVE68 and killed by OBJECTIVE50"
    );
    assert!(
        incoming(50).is_empty(),
        "no completion edge reaches OBJECTIVE50: it is the record's own \
         engine-count condition, not a chained block"
    );

    // The record the census measured and the record the binding measured are
    // the same measurement, so every pin above holds for both derivations.
    assert_eq!(
        Some(&binding.record),
        census_row().record(),
        "one measurement, two production derivations"
    );
}

/// **M12's record lowers completely: all 82 conditions, all 265 sites, and
/// the assembled program validates.** The record spells no counting site;
/// its `INACTIVE_COMPLETION_COUNT` blocks lower as ordinary conditions.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m12_b_fu1_m12s_record_lowers_completely() {
    let binding = control_binding();
    let row = census_row();
    let lowered = row
        .lowering_attempt()
        .expect("the census lowers M12's measured record");
    let attempt = lowered.attempt();
    let record = &binding.record;

    assert_eq!(
        attempt.mission.as_deref(),
        Ok("mission/ch3-m02"),
        "the lowering resolved M12's mission id"
    );
    assert_eq!(
        attempt.objectives, BLOCKS,
        "every block carries a condition verdict"
    );
    assert_eq!(
        attempt.calls.len() as u32,
        SITES,
        "every site carries a call outcome"
    );
    assert!(
        attempt
            .calls
            .iter()
            .all(|call| matches!(call, CallOutcome::Bound)),
        "every one of the {SITES} sites binds to a host call"
    );
    assert!(
        attempt.unbound_keys.is_empty(),
        "no key refuses registration: {:?}",
        attempt.unbound_keys
    );
    assert!(lowered.program().is_some(), "the program is assembled");
    assert_eq!(
        attempt.conditions.len() as u32,
        BLOCKS,
        "every block carries a condition verdict"
    );
    assert!(
        attempt
            .conditions
            .iter()
            .all(|condition| matches!(condition, ConditionOutcome::Lowered)),
        "{:?}",
        attempt.conditions
    );
    assert_eq!(
        attempt.validation,
        Some(Vec::new()),
        "the assembled program validates"
    );

    let lowering = record.lowering(attempt);
    let unmet: Vec<String> = lowering.unmet().map(|r| r.kind.code().to_owned()).collect();
    assert!(
        !unmet
            .iter()
            .any(|code| code == "objective_condition" || code == "call_arguments"),
        "{unmet:?}"
    );
}

// ---------------------------------------------------------------------------
// Retail: where the sheet's three regression priorities live
// ---------------------------------------------------------------------------

/// **The three M12 regression priorities are located in the measured record
/// — as directives, operands and block edges the record spells, not as
/// guessed timings or coordinates — and the half of each that a runtime
/// must observe stays unmeasured.**
///
/// M12-A left all three unbound. What this stage can honestly add is the
/// *data* each one is built from, plus the negative half: the control
/// member's whole 26-key vocabulary carries no collision, damage, docking,
/// pickup or transfer directive (pinned by the vocabulary test), so the
/// priorities' actor/interaction halves are not in this member at all.
/// Which actor a spelled name resolves to is the world's binding, not this
/// record's: nothing below claims it.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m12_b_the_three_sheet_priorities_locate_in_the_measured_record() {
    let binding = control_binding();
    let (document, _) = control_document();
    let blocks = blocks_of(&document);
    let block = |number: u32| -> &Block {
        let key = format!("OBJECTIVE{number}");
        blocks
            .iter()
            .find(|block| block.key == key)
            .unwrap_or_else(|| panic!("the record declares {key}"))
    };
    let sites = |number: u32, key: &str| -> Vec<Vec<ZrdValue>> {
        block(number)
            .sites
            .iter()
            .filter(|(name, _)| name == key)
            .map(|(_, args)| args.clone())
            .collect()
    };
    let text = |value: &str| ZrdValue::Text(value.to_owned());
    let part_list = |name: &str, state: &str| {
        vec![
            ZrdValue::Text("ANIM".to_owned()),
            ZrdValue::List(vec![
                ZrdValue::Text("NAME".to_owned()),
                ZrdValue::List(vec![ZrdValue::Text(name.to_owned())]),
                ZrdValue::Text("STATE".to_owned()),
                ZrdValue::List(vec![ZrdValue::Text(state.to_owned())]),
            ]),
        ]
    };

    // --- Escort survival: the failure latch OBJECTIVE51 is reached only
    // through OBJECTIVE50 (graph test), whose condition counts four of eight
    // spelled `g_engine*` members through `INACTIVE_COMPLETION_COUNT`; the
    // blocks 72, 73 and 74 carry the same eight members with counts 1, 2
    // and 3. Whose engines those are is the world's binding and is not
    // spelled here.
    for (number, count) in [(72, 1), (73, 2), (74, 3), (50, 4)] {
        assert_eq!(
            sites(number, "INACTIVE_COMPLETION_COUNT"),
            [vec![ZrdValue::Int(count)]],
            "OBJECTIVE{number} counts {count} inactive members"
        );
        for member in 1..=8 {
            assert_eq!(
                sites(number, &format!("INACTIVE{member}")),
                [vec![
                    text(&format!("g_engine{member}")),
                    text("healthy_part")
                ]],
                "OBJECTIVE{number} lists g_engine{member}'s healthy part as member {member}"
            );
        }
    }
    assert_eq!(
        sites(50, "NAP_OBJECTIVE_WHEN_I_COMPLETE"),
        [vec![ZrdValue::Int(51), ZrdValue::Float(30.0)]],
        "OBJECTIVE50's completion naps the failure latch awake after the spelled 30 seconds"
    );
    assert_eq!(
        sites(4, "ADD_OBJECTIVE_TARGET"),
        [vec![text("sprucegoose")]],
        "OBJECTIVE4 arms `sprucegoose` as an objective target at completion"
    );
    assert_eq!(
        sites(69, "ANIM_STATE"),
        [part_list("fly_the_goose", "RUNNING")],
        "OBJECTIVE69 completes when the spelled animation reports RUNNING"
    );

    // --- Door and route clearance: the gates and chains the record spells,
    // as data — `kkgate` is a healthy-state member of OBJECTIVE4's
    // condition, the four tug-and-barge targets are armed and retired in
    // turn by OBJECTIVE8, 13, 17, 21 and 25, and the path animations gate
    // the blocks that wake the next leg.
    assert_eq!(
        sites(4, "INACTIVE1"),
        [vec![text("kkgate"), text("healthy")]],
        "OBJECTIVE4's in-play member is `kkgate`'s healthy state"
    );
    assert_eq!(
        sites(8, "ADD_OBJECTIVE_TARGET"),
        [vec![text("tugandbarge01")]],
        "OBJECTIVE8 arms the first barge as an objective target"
    );
    for (number, armed) in [
        (13, "tugandbarge02"),
        (17, "tugandbarge03"),
        (21, "tugandbarge04"),
    ] {
        assert_eq!(
            sites(number, "ADD_OBJECTIVE_TARGET"),
            [vec![text(armed)]],
            "OBJECTIVE{number} arms {armed}"
        );
    }
    assert!(
        sites(25, "ADD_OBJECTIVE_TARGET").is_empty()
            && sites(25, "REMOVE_OBJECTIVE_TARGET")
                == [vec![
                    text("tugandbarge01"),
                    text("tugandbarge02"),
                    text("tugandbarge03"),
                    text("tugandbarge04")
                ]],
        "OBJECTIVE25 arms nothing and retires all four barges"
    );
    assert_eq!(
        sites(9, "ANIM_STATE")
            .first()
            .map(|args| texts(args).into_iter().filter(|t| *t == "ANIM").count()),
        Some(2),
        "OBJECTIVE9's condition spells two animation members"
    );
    for (number, name) in [
        (37, "path2_decelerate"),
        (38, "path3_decelerate"),
        (39, "path4_decelerate"),
        (40, "path5_decelerate"),
    ] {
        assert_eq!(
            sites(number, "ANIM_STATE"),
            [part_list(name, "RUNNING")],
            "OBJECTIVE{number} completes when {name} reports RUNNING"
        );
    }

    // --- Oversized collision: no key in the vocabulary names a collision,
    // a damage value or a size, and the record spells no proximity
    // evaluator at all.
    for key in binding.record.keys() {
        let lowered = key.key.to_lowercase();
        for word in [
            "collid",
            "collision",
            "damage",
            "size",
            "dock",
            "pickup",
            "board",
            "transfer",
            "authoriz",
            "interact",
        ] {
            assert!(
                !lowered.contains(word),
                "the vocabulary carries an interaction directive nobody measured: \
                 {}",
                key.key
            );
        }
    }
    for absent in ["TRAVELERS", "WAKEUP_TURRETS", "WAKEUP_ZEP_TURRETS"] {
        assert!(
            binding.record.key(absent).is_none(),
            "M12 spells no {absent}"
        );
    }

    // The mission's own start data, spelled at record level: the player
    // starts at the spelled position with the spelled attitude values and the
    // mission timer at zero — the record's initial player configuration, not
    // a walkthrough's.
    let (document, _) = control_document();
    let record_fields: Vec<(String, &ZrdValue)> = zrd_flat_fields(objective_record(&document))
        .into_iter()
        .filter(|(key, _)| objective_block_number(key).is_none())
        .map(|(key, value)| (key.to_owned(), value))
        .collect();
    let field = |name: &str| -> &ZrdValue {
        record_fields
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| *value)
            .unwrap_or_else(|| panic!("the record fields carry {name}"))
    };
    assert_eq!(
        field("MISSION_TIMER").as_list(),
        Some(&[ZrdValue::Float(0.0)][..]),
        "the mission timer starts at zero"
    );
    assert_eq!(
        field("PLAYER_INIT").as_list(),
        Some(
            &[
                ZrdValue::Int(1),
                ZrdValue::List(vec![
                    ZrdValue::Float(-6389.0),
                    ZrdValue::Float(250.0),
                    ZrdValue::Float(-2849.0),
                ]),
                ZrdValue::List(vec![
                    ZrdValue::Float(0.0),
                    ZrdValue::Float(0.0),
                    ZrdValue::Float(0.0),
                ]),
                ZrdValue::Float(0.8),
                ZrdValue::Float(180.0),
            ][..]
        ),
        "the authored player start, as the record spells it"
    );
    for name in ["RESTORE_ANIMS", "EXECUTE_ANIMS", "INVALIDATE_ANIMS"] {
        assert_eq!(
            field(name).as_list(),
            Some(&[][..]),
            "{name} is spelled empty"
        );
    }

    // The objective classes the record spells: three PRIMARY blocks carrying
    // localized-message ids and one SECONDARY block with none.
    for (number, order, message) in [
        (4, 1, "MSG_BRF_HWM2_OBJ1"),
        (25, 2, "MSG_BRF_HWM2_OBJ2"),
        (46, 3, "MSG_BRF_HWM2_OBJ3"),
    ] {
        assert_eq!(
            sites(number, "IDENTITY"),
            [vec![text("PRIMARY"), ZrdValue::Int(order), text(message)]],
            "OBJECTIVE{number} is primary objective {order}"
        );
    }
    assert_eq!(
        sites(43, "IDENTITY"),
        [vec![text("SECONDARY"), ZrdValue::Int(11)]],
        "OBJECTIVE43 is the secondary objective, spelled without a message id"
    );
    let identity = binding
        .record
        .key("IDENTITY")
        .expect("M12 spells the identity key");
    assert_eq!(identity.sites, 4, "four identity sites, no more");

    // Whether the wrong actor, the wrong session or a repeated event can
    // satisfy any of these transitions is runtime behaviour: nothing here
    // simulates it, and a complete census row says nothing about it — it is
    // a lowering claim. The runtime halves stay open for M12-C, and the
    // campaign gate still keeps every runtime out while any measured row
    // carries a gap.
    assert!(
        !census().campaign_ready(),
        "M12's transitions stay unobserved: no campaign runtime may start while the \
         campaign gate is closed"
    );
}

/// **M12's control-program row is complete; the campaign still is not ready.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m12_b_fu1_m12_lowers_completely_and_the_campaign_stays_unready() {
    let census = census();
    assert!(census.complete_missions().contains(&MISSION));
    assert!(!census.campaign_ready());
    assert!(census.measured_rows().any(|row| row.mission() == MISSION));
}

// ---------------------------------------------------------------------------
// Synthetic: the spellings the retail record leans on
// ---------------------------------------------------------------------------

/// A `.zrd` int node.
fn zrd_int(value: u32) -> ZrdValue {
    ZrdValue::Int(value)
}

/// A `.zrd` float node.
fn zrd_float(value: f32) -> ZrdValue {
    ZrdValue::Float(value)
}

/// A `.zrd` text node.
fn zrd_text(text: &str) -> ZrdValue {
    ZrdValue::Text(text.to_owned())
}

/// A `.zrd` list node.
fn zrd_list(children: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(children)
}

/// One authored directive site: the key, plus its argument list unless the
/// site is authored bare.
fn directive(key: &str, args: Vec<ZrdValue>) -> Vec<ZrdValue> {
    let mut children = vec![zrd_text(key)];
    if !args.is_empty() {
        children.push(zrd_list(args));
    }
    children
}

/// One authored numbered block.
fn block(number: u32, directives: Vec<Vec<ZrdValue>>) -> (String, ZrdValue) {
    let mut children = Vec::new();
    for site in directives {
        children.extend(site);
    }
    (format!("OBJECTIVE{number}"), zrd_list(children))
}

/// A wrapped control record: the measured one-element wrapper around the
/// flat record.
fn control_record(fields: Vec<(String, ZrdValue)>) -> ZrdValue {
    let mut children = Vec::new();
    for (key, value) in fields {
        children.push(zrd_text(&key));
        children.push(value);
    }
    zrd_list(vec![zrd_list(children)])
}

/// Lowers an authored record through the production adapter.
fn lower(document: &ZrdValue) -> cs_app::control_lowering::LoweredControlRecord {
    let record = measure_control_record(document);
    lower_control_record(
        ContentId::from_source(ContentKind::Mission, "accept-m12-b")
            .map_err(|error| error.to_string()),
        "accept-m12-b",
        document,
        &record,
    )
}

/// The `Travelers` condition a lowered program carries, or a failure naming
/// the condition it actually carries.
fn travelers_condition(program: &cs_script::ir::MissionProgram, block: usize) -> Condition {
    fn find(condition: &Condition, found: &mut Vec<Condition>) {
        if matches!(condition, Condition::Travelers { .. }) {
            found.push(condition.clone());
        }
        match condition {
            Condition::All(items) | Condition::Any(items) => {
                for item in items {
                    find(item, found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    find(&program.objectives[block].condition, &mut found);
    assert_eq!(
        found.len(),
        1,
        "block {block} carries exactly one travelers condition: {found:?}"
    );
    found.pop().expect("one condition")
}

/// The one `TravelersCount` condition in the block's lowered condition tree.
fn counting_condition(program: &cs_script::ir::MissionProgram, block: usize) -> Condition {
    fn find(condition: &Condition, found: &mut Vec<Condition>) {
        if matches!(condition, Condition::TravelersCount { .. }) {
            found.push(condition.clone());
        }
        if let Condition::All(items) | Condition::Any(items) = condition {
            for item in items {
                find(item, found);
            }
        }
    }
    let mut found = Vec::new();
    find(&program.objectives[block].condition, &mut found);
    assert_eq!(found.len(), 1, "exactly one counting condition: {found:?}");
    found.pop().expect("one condition")
}

/// **The three `TRAVELERS` shapes M12 spells behave exactly as the measured
/// rule says: the counting-mode subject, the point anchor and the named
/// anchor all lower.**
///
/// The retail test observes that result on the installation; this carries
/// the mechanism into CI: an authored record spelling each of M12's three
/// sites — OBJECTIVE36's numeric subject, OBJECTIVE2's three-real point and
/// OBJECTIVE51's named zeppelin — gets the refusal on the first and a
/// `Travelers` predicate carrying the spelled subject, anchor and radius on
/// the other two. A non-`APPROACHING` polarity refuses too, so a shortcut
/// that accepts every token fails here.
#[test]
fn accept_m12_b_m12s_travelers_shapes_lower_as_the_record_spells_them() {
    let authored = |args: Vec<ZrdValue>| {
        control_record(vec![block(
            1,
            vec![
                directive("BEGIN_DORMANT", vec![zrd_float(-1.0)]),
                directive("TRAVELERS", args),
            ],
        )])
    };

    // OBJECTIVE36's shape: a numeric child0 arms the counting mode, which
    // lowers to the counting condition carrying the group id and the total.
    let document = authored(vec![
        zrd_int(4),
        zrd_text("APPROACHING"),
        zrd_text("unit03"),
        zrd_float(700.0),
        zrd_int(1),
    ]);
    let lowered = lower(&document);
    assert_eq!(
        lowered.attempt().conditions,
        [ConditionOutcome::Lowered],
        "{:?}",
        lowered.attempt().conditions
    );
    assert_eq!(lowered.attempt().validation, Some(Vec::new()));
    let program = lowered.program().expect("the authored record assembles");
    match counting_condition(program, 0) {
        Condition::TravelersCount {
            group,
            anchor,
            approaching,
            required,
            delete_matched,
            ..
        } => {
            assert_eq!(group, 4);
            assert_eq!(anchor, TravelersAnchor::Object(vec!["unit03".to_owned()]));
            assert!(approaching);
            assert_eq!(required, 1);
            assert!(!delete_matched);
        }
        other => panic!("a numeric child0 lowers to the counting mode: {other:?}"),
    }

    // OBJECTIVE2's shape: the anchor is a three-real point — a spelling no
    // earlier measured mission carried — and the predicate holds the
    // spelled coordinates.
    let document = authored(vec![
        zrd_text("player"),
        zrd_text("APPROACHING"),
        zrd_list(vec![
            zrd_float(-12419.9),
            zrd_float(134.0),
            zrd_float(-10429.8),
        ]),
        zrd_float(1500.0),
        zrd_int(1),
    ]);
    let lowered = lower(&document);
    assert_eq!(
        lowered.attempt().conditions,
        [ConditionOutcome::Lowered],
        "a point anchor is a predicate: {:?}",
        lowered.attempt().conditions
    );
    let program = lowered.program().expect("the authored record assembles");
    match travelers_condition(program, 0) {
        Condition::Travelers {
            subject,
            anchor,
            radius,
            approaching,
        } => {
            assert_eq!(subject, ["player".to_owned()]);
            assert_eq!(
                anchor,
                TravelersAnchor::Point([
                    f64::from(-12419.9_f32),
                    f64::from(134.0_f32),
                    f64::from(-10429.8_f32),
                ]),
                "the predicate carries the spelled point"
            );
            assert_eq!(radius, f64::from(1500.0_f32));
            assert!(approaching, "the site spells APPROACHING");
        }
        other => panic!("a travelers site lowers to a travelers condition: {other:?}"),
    }

    // OBJECTIVE51's shape: the anchor is a member name, resolved at
    // runtime — the predicate holds the spelled name.
    let document = authored(vec![
        zrd_text("player"),
        zrd_text("APPROACHING"),
        zrd_text("piratezep"),
        zrd_float(1500.0),
        zrd_int(1),
    ]);
    let lowered = lower(&document);
    assert_eq!(
        lowered.attempt().conditions,
        [ConditionOutcome::Lowered],
        "a named anchor is a predicate: {:?}",
        lowered.attempt().conditions
    );
    let program = lowered.program().expect("the authored record assembles");
    match travelers_condition(program, 0) {
        Condition::Travelers {
            subject, anchor, ..
        } => {
            assert_eq!(subject, ["player".to_owned()]);
            assert_eq!(
                anchor,
                TravelersAnchor::Object(vec!["piratezep".to_owned()]),
                "the predicate carries the spelled member name"
            );
        }
        other => panic!("a travelers site lowers to a travelers condition: {other:?}"),
    }

    // The other pole: the parse stores 1 for `APPROACHING` and 0 for every
    // other string, so any other token is the outside pole.
    let document = authored(vec![
        zrd_text("player"),
        zrd_text("DEPARTING"),
        zrd_text("piratezep"),
        zrd_float(1500.0),
        zrd_int(1),
    ]);
    let lowered = lower(&document);
    let program = lowered.program().expect("the authored record assembles");
    match travelers_condition(program, 0) {
        Condition::Travelers { approaching, .. } => {
            assert!(
                !approaching,
                "any token but APPROACHING is the outside pole"
            );
        }
        other => panic!("a travelers site lowers to a travelers condition: {other:?}"),
    }
}

/// **A wildcard turret name binds like any spelled name, and a record that
/// spells a win latch but no loss latch lowers completely.**
///
/// OBJECTIVE40's `b_turret*` arrives as data, not as a wildcard evaluation:
/// the adapter carries the spelled name into the bound host call, while the
/// wildcard's one-digit-consuming intent stays the recorded unknown. And a
/// program whose only terminal block is `INSTANTWIN` assembles and
/// validates clean — the no-failure-latch shape M12 spells is a valid
/// program shape, so the record's missing `INSTANTLOSS` is a fact about
/// M12, not a shape the engine cannot carry.
#[test]
fn accept_m12_b_a_wildcard_turret_name_binds_and_a_win_only_latch_completes() {
    let document = control_record(vec![
        block(
            1,
            vec![
                directive("WAKEUP_TURRETS", vec![zrd_text("b_turret*")]),
                directive("INSTANTWIN", vec![]),
            ],
        ),
        block(2, vec![directive("BEGIN_DORMANT", vec![zrd_float(-1.0)])]),
    ]);
    let record = measure_control_record(&document);
    let lowered = lower(&document);
    let attempt = lowered.attempt();

    assert!(
        attempt
            .calls
            .iter()
            .all(|call| matches!(call, CallOutcome::Bound)),
        "every authored site binds: {:?}",
        attempt.calls
    );
    let raw = lowered.raw_program().expect("the program assembled");
    let wildcard_call = raw.objectives[0]
        .calls
        .iter()
        .find(|call| call.name == "WAKEUP_TURRETS")
        .expect("the turret wake call is emitted");
    assert_eq!(
        wildcard_call.args.as_slice(),
        [Value::Str("b_turret*".to_owned())],
        "the wildcard name arrives as spelled data in the call's arguments"
    );

    assert_eq!(
        attempt.validation,
        Some(Vec::new()),
        "a record with only a success latch validates clean"
    );
    assert!(
        record.is_complete(attempt),
        "a win-only record lowers completely: unbound={:?}",
        attempt.unbound_keys
    );
}
