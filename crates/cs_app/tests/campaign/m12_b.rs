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
//! M12-A bound *which* retail mission the work order names (`mission/ch3-m02`,
//! `script/c3-m02-zrdr`, the reader archive `ZBD/C3/M02/zrdr.zbd`) and left
//! every objective, actor and directive unbound. This stage binds the archive
//! through the production systems M02-B built
//! (`SourceContext::control_program`) and measures it a second time through
//! `cs_app::mission_control`, so three independent derivations — the mission
//! binding, the control binding and the retail census — must agree before
//! any assertion below can pass.
//!
//! The stage's minimum acceptance scenario is *"All discovered
//! mission-specific behavior uses production engine systems and regression
//! tests."* M12's discovered mission-specific behavior is its control
//! record: 52 numbered blocks, 187 directive sites, 29 distinct keys, the
//! block graph those spell, and the one lowering gap measured below. What is
//! different at M12:
//!
//! * **There is no failure latch.** `INSTANTWIN` (OBJECTIVE48) is the record's
//!   only terminal outcome; the record spells no `INSTANTLOSS` block at all.
//!   Whatever ends the mission short of success is not a spelled latch of
//!   this member — a fact the escort-survival priority is measured against.
//! * **One completion condition refuses.** OBJECTIVE36's `TRAVELERS` spells a
//!   numeric child0 (`[4, APPROACHING, unit03, 700, 1]`), which arms the
//!   counting mode the M01-LC findings measured as a counter write inside
//!   evaluation — the same unimplemented gap M10-B-FU1 (#812) tracks for
//!   M10's two sites. M12's is the first *counting* site that spells
//!   `APPROACHING`, and the first whose subject mode siblings both lower:
//!   OBJECTIVE2 anchors on a spelled point (a shape no earlier mission
//!   spelled) and OBJECTIVE51 on `piratezep`.
//! * **The `WAKEUP_TURRETS` wildcard is exercised.** OBJECTIVE40 spells
//!   `b_turret*`, the one-digit-consuming wildcard the findings measured but
//!   no measured site spelled before.
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

use crate::common::{label, load_inventory};

/// The census row label of the mission: the mission-scoped reader archive
/// F13-B's rule derives from the installation.
const MISSION: &str = "zbd/c3/m02";

/// The numbered blocks of the control member.
const BLOCKS: u32 = 52;
/// The directive sites of the control member.
const SITES: u32 = 187;
/// The distinct directive keys of the control member.
const KEYS: usize = 29;
/// The cross-objective addresses the directed keys spell (wake, kill, nap
/// and dependency-gate lists).
const EDGES: usize = 25;

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
        "script/c3-m02-zrdr",
        "the program identity is the mission's own reader archive"
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
        16,
        "M12's archive offers sixteen members"
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
/// it carries a win latch but no loss latch.** Every key M12 spells carries
/// a measured disposition except its one terminal spelling: `INSTANTWIN`,
/// one site. `INSTANTLOSS` is not in the record at all — the first measured
/// mission here that spells no failure outcome — so whatever ends the
/// mission short of success is not a latch this member spells. No block is
/// unreadable, and no record-level key falls outside the measured record
/// vocabulary.
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
        [("INSTANTWIN", TerminalOutcome::Succeeded)],
        "M12's only implemented directive is its success outcome, one site"
    );
    assert!(
        record.key("INSTANTLOSS").is_none(),
        "M12 spells no failure latch: INSTANTLOSS is not in its vocabulary"
    );
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

    // The outcome key is a bare spelling and answers only for its own name;
    // no other key in M12's vocabulary is spelled bare.
    let win = record
        .key("INSTANTWIN")
        .expect("M12 spells its outcome key");
    assert!(
        win.agreed_shape()
            .is_some_and(|shape| shape.label() == "bare"),
        "INSTANTWIN is spelled bare in M12"
    );
    assert!(
        terminal_outcome_of("INSTANTWIN").is_some(),
        "INSTANTWIN is an outcome"
    );
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
        ["INSTANTWIN"],
        "in M12 the only bare key spells the success outcome"
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
            "IDENTITY",
            "INACTIVE1",
            "INACTIVE10",
            "INACTIVE11",
            "INACTIVE2",
            "INACTIVE3",
            "INACTIVE4",
            "INACTIVE5",
            "INACTIVE6",
            "INACTIVE7",
            "INACTIVE8",
            "INACTIVE9",
            "INACTIVE_COMPLETION_COUNT",
            "INSTANTWIN",
            "KILL_OBJECTIVE_WHEN_I_COMPLETE",
            "NAP_OBJECTIVE_WHEN_I_COMPLETE",
            "REMOVE_OBJECTIVE_TARGET",
            "REMOVE_OTHER_TARGET",
            "TICK_DEPENDS_ON_OBJ",
            "TRAVELERS",
            "WAKEUP_SOUND_GROUP",
            "WAKEUP_TURRETS",
            "WAKEUP_ZEP_TURRETS",
            "WAKE_ANIM",
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
        ],
        "M12's whole directive vocabulary, sorted byte-wise, so an added or \
         dropped key fails here"
    );
}

// ---------------------------------------------------------------------------
// Retail: the block graph, and the one measured lowering gap
// ---------------------------------------------------------------------------

/// **Every cross-objective address M12 spells names a block this record
/// declares, the success latch is gated by the hook chain, and no block ends
/// the mission in failure.**
///
/// The record declares `OBJECTIVE1` … `OBJECTIVE52`; the four
/// cross-objective keys M12 spells carry {EDGES} addresses, none zero, none
/// negative and none past the last block — the largest value M12 spells
/// *is* the block count. A spelled address names the block it decrements
/// to (the parse stores `address - 1`; that rule, and the refusal it raises
/// past the count, are M02-B-FU3's measurement of the original — Rally
/// #802 — not re-measured here), so under it every address resolves to a
/// block the record declares and M12 carries **no** out-of-range address.
/// The edge assertions below are compared on that spelling, which is also
/// what makes the success latch land on the block the mission's structure
/// names. The wrong-actor, wrong-session and repeated-event halves of the
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

    // The four cross-objective keys M12 spells (it spells no
    // `SLEEP_…`, no bare `WAKE_OBJECTIVE` and no `HIDE_OBJ`), and every
    // integer their sites address.
    let directed = [
        "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
        "KILL_OBJECTIVE_WHEN_I_COMPLETE",
        "NAP_OBJECTIVE_WHEN_I_COMPLETE",
        "TICK_DEPENDS_ON_OBJ",
    ];
    for absent in [
        "SLEEP_OBJECTIVE_WHEN_I_COMPLETE",
        "WAKE_OBJECTIVE",
        "HIDE_OBJ",
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
    // the parse stores — always lands inside the record's own 52 blocks.
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
        Some(4),
        "the lowest address M12 spells names OBJECTIVE4"
    );
    assert_eq!(
        addresses.iter().copied().max(),
        Some(i64::from(BLOCKS)),
        "the highest address M12 spells names the last declared block — the \
         record's own boundary is live and nothing crosses it"
    );

    // **The success latch is gated.** Exactly one block spells an outcome,
    // and no block spells a failure outcome at all.
    let number_of = |block: &Block| -> u32 {
        block
            .key
            .trim_start_matches("OBJECTIVE")
            .parse::<u32>()
            .expect("a block key carries its number")
    };
    let outcomes: Vec<u32> = blocks
        .iter()
        .filter(|block| {
            block
                .sites
                .iter()
                .any(|(key, _)| key.starts_with("INSTANT"))
        })
        .map(number_of)
        .collect();
    assert_eq!(
        outcomes,
        [48],
        "OBJECTIVE48 is the only block that ends the mission — M12 spells no \
         INSTANTLOSS block anywhere, so no latch can end it in failure"
    );
    let win: Vec<&Block> = blocks
        .iter()
        .filter(|block| block.sites.iter().any(|(key, _)| key == "INSTANTWIN"))
        .collect();
    assert_eq!(win.len(), 1, "M12 spells exactly one success latch");
    let latch = win[0];
    assert!(
        latch
            .sites
            .iter()
            .any(|(key, args)| key == "BEGIN_DORMANT" && args.as_slice() == [ZrdValue::Float(-1.0)]),
        "the success latch starts dormant with no timed wake"
    );

    // The latch is woken by exactly one block — OBJECTIVE9, whose own wake
    // is the nap OBJECTIVE8's completion schedules, and OBJECTIVE8 ticks
    // only while its dependency gate OBJECTIVE5 is awake. The chain the
    // record spells is `8 →nap→ 9 →wake→ 48`, gated on `5`.
    // Completion edges only: `TICK_DEPENDS_ON_OBJ` addresses its gate, not a
    // completion effect, so it is excluded from the incoming-edge walk.
    let completion = &directed[..3];
    let incoming = |target: i64| -> Vec<(u32, String)> {
        blocks
            .iter()
            .flat_map(|block| {
                block
                    .sites
                    .iter()
                    .filter(|(key, args)| {
                        completion.contains(&key.as_str()) && integers(args).contains(&target)
                    })
                    .map(|(key, _)| (number_of(block), key.clone()))
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    assert_eq!(
        incoming(48),
        [(9, "WAKE_OBJECTIVE_WHEN_I_COMPLETE".to_owned())],
        "only OBJECTIVE9's completion wakes the success latch"
    );
    assert_eq!(
        incoming(9),
        [(8, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned())],
        "OBJECTIVE9's only incoming edge is the nap OBJECTIVE8 schedules — the \
         nap re-wakes its target after the spelled seconds"
    );
    let gates: Vec<(u32, Vec<i64>)> = blocks
        .iter()
        .map(|block| {
            (
                number_of(block),
                block
                    .sites
                    .iter()
                    .filter(|(key, _)| key == "TICK_DEPENDS_ON_OBJ")
                    .flat_map(|(_, args)| integers(args))
                    .collect::<Vec<_>>(),
            )
        })
        .filter(|(_, deps)| !deps.is_empty())
        .collect();
    assert_eq!(
        gates,
        [(8, vec![5]), (15, vec![13])],
        "OBJECTIVE8 ticks only while OBJECTIVE5 is awake, OBJECTIVE15 only \
         while OBJECTIVE13 is — the two dependency gates M12 spells"
    );
    assert_eq!(
        incoming(5),
        [(4, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned())],
        "OBJECTIVE5's only incoming edge is the nap OBJECTIVE4 schedules"
    );

    // The record the census measured and the record the binding measured are
    // the same measurement, so every pin above holds for both derivations.
    assert_eq!(
        Some(&binding.record),
        census_row().record(),
        "one measurement, two production derivations"
    );
}

/// **M12's record does not lower, and the reason is exactly one named
/// block.** All 187 sites bind and 51 of 52 conditions lower, yet
/// OBJECTIVE36's `TRAVELERS` spells a numeric child0 — the counting mode
/// whose evaluation accumulates a matching-member count into `+0x5b8`: a
/// write, so no side-effect-free predicate carries it. Because every call
/// binds, a program is assembled, but its validation reports the unsupported
/// condition instruction, so `objective_condition` and `call_arguments` are
/// the unmet rows and the mission is not complete. This is the same
/// unimplemented gap M10-B-FU1 (#812) tracks for M10's two sites.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m12_b_m12s_record_does_not_lower_and_exactly_one_condition_refuses() {
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
    assert!(
        lowered.program().is_some(),
        "every call bound, so a program is assembled; it is the validation \
         that fails"
    );

    assert_eq!(
        attempt.conditions.len() as u32,
        BLOCKS,
        "every block carries a condition verdict"
    );
    let refused: Vec<(usize, &str)> = attempt
        .conditions
        .iter()
        .enumerate()
        .filter_map(|(index, condition)| match condition {
            ConditionOutcome::Lowered => None,
            ConditionOutcome::Refused(text) => Some((index, text.as_str())),
            ConditionOutcome::Unreadable(text) => panic!("block {index} is unreadable: {text}"),
        })
        .collect();
    assert_eq!(
        refused.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
        [35],
        "zero-based: OBJECTIVE36 alone refuses"
    );
    for (index, text) in &refused {
        assert!(
            text.contains(&format!("`OBJECTIVE{}` `TRAVELERS`", index + 1))
                && text.contains("a non-string subject arms TRAVELERS' counting mode")
                && text.contains("a write, so no side-effect-free predicate can carry it"),
            "{text}"
        );
    }

    let validation = attempt
        .validation
        .as_ref()
        .expect("the program assembled and was validated");
    assert!(
        validation.iter().any(|problem| problem
            .contains("mission/ch3-m02 objective#35 [condition]: unsupported instruction")),
        "{validation:?}"
    );

    let lowering = record.lowering(attempt);
    let unmet: Vec<String> = lowering.unmet().map(|r| r.kind.code().to_owned()).collect();
    assert_eq!(unmet, ["objective_condition", "call_arguments"]);
    assert!(!record.is_complete(attempt));
    assert!(!lowering.complete());
    assert!(!row.is_complete());
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
/// member's whole 29-key vocabulary carries no collision, damage, docking,
/// pickup or transfer directive (pinned by the vocabulary test), so the
/// priorities' actor/interaction halves are not in this member at all.
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

    // --- Escort survival: `piratezep` is the only name the record spells
    // through three different evaluators. Its zeppelin turrets are woken at
    // start; it is an in-play member of two blocks; and it is the anchor of
    // the last travelers site, whose completion wakes the success music.
    // Whether its destruction *fails* the mission is not spelled: M12 has no
    // INSTANTLOSS latch (vocabulary test), so the record names no failure.
    assert_eq!(
        sites(1, "WAKEUP_ZEP_TURRETS"),
        [vec![ZrdValue::Text("piratezep".to_owned())]],
        "OBJECTIVE1 wakes the named zeppelin's turrets at start"
    );
    for number in [5, 13] {
        assert_eq!(
            sites(number, "INACTIVE1"),
            [vec![ZrdValue::Text("piratezep".to_owned())]],
            "OBJECTIVE{number} lists piratezep as its in-play member"
        );
    }
    assert_eq!(
        sites(51, "TRAVELERS"),
        [vec![
            ZrdValue::Text("player".to_owned()),
            ZrdValue::Text("APPROACHING".to_owned()),
            ZrdValue::Text("piratezep".to_owned()),
            ZrdValue::Float(1500.0),
            ZrdValue::Int(1),
        ]],
        "OBJECTIVE51 completes when the player closes on piratezep within \
         the spelled radius"
    );
    assert_eq!(
        integers(
            &sites(51, "WAKE_OBJECTIVE_WHEN_I_COMPLETE")
                .pop()
                .expect("OBJECTIVE51 wakes")
        ),
        [10],
        "…and its completion wakes OBJECTIVE10"
    );
    assert_eq!(
        sites(10, "COMPLETED_SOUND_GROUP"),
        [vec![ZrdValue::Text("music_missionsuccess_sg".to_owned())]],
        "OBJECTIVE10 plays the mission-success music state"
    );

    // --- Door and route clearance: the hook chain the record spells, as
    // data — the hook point becomes an objective target when the gated
    // OBJECTIVE8 completes, OBJECTIVE9 wakes the `pzhomebase` animation and
    // completes only when `hooked_to_klondike` reports EXECUTED, and that
    // completion is the latch's only wake edge (graph test). The airdock's
    // `dock2` target is retired by OBJECTIVE43.
    assert_eq!(
        sites(8, "ADD_OBJECTIVE_TARGET"),
        [vec![ZrdValue::Text("pzhookpoint".to_owned())]],
        "OBJECTIVE8 arms the hook point as an objective target at completion"
    );
    assert_eq!(
        sites(9, "WAKE_ANIM"),
        [vec![ZrdValue::Text("pzhomebase".to_owned())]],
        "OBJECTIVE9 wakes the named animation"
    );
    assert_eq!(
        sites(9, "ANIM_STATE"),
        [vec![
            ZrdValue::Text("ANIM".to_owned()),
            ZrdValue::List(vec![
                ZrdValue::Text("NAME".to_owned()),
                ZrdValue::List(vec![ZrdValue::Text("hooked_to_klondike".to_owned())]),
                ZrdValue::Text("STATE".to_owned()),
                ZrdValue::List(vec![ZrdValue::Text("EXECUTED".to_owned())]),
            ]),
        ]],
        "OBJECTIVE9 completes when the spelled animation reports EXECUTED — \
         the measured operand-list shape, name and state as spelled"
    );
    assert_eq!(
        sites(43, "REMOVE_OTHER_TARGET"),
        [vec![ZrdValue::Text("dock2".to_owned())]],
        "OBJECTIVE43 retires the airdock's `dock2` other-target at completion"
    );
    let airdock: Vec<u32> = blocks
        .iter()
        .filter(|block| {
            block
                .sites
                .iter()
                .any(|(key, args)| key.starts_with("INACTIVE") && texts(args).contains(&"airdock2"))
        })
        .map(|block| {
            block
                .key
                .trim_start_matches("OBJECTIVE")
                .parse::<u32>()
                .expect("a block key carries its number")
        })
        .collect();
    assert_eq!(
        airdock,
        [14, 43, 44, 45],
        "the airdock is an in-play member of these blocks, as spelled"
    );

    // --- Oversized collision: no key in the vocabulary names a collision,
    // a damage value or a size — the measurable proximity structures are
    // the three travelers sites, all three shapes of them, and the counting
    // site is exactly the lowering gap the record test pins.
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
    let travelers = binding
        .record
        .key("TRAVELERS")
        .expect("M12 spells the travelers evaluator");
    assert_eq!(
        travelers.sites, 3,
        "three travelers sites — the record holds three slots for them"
    );
    assert_eq!(
        sites(2, "TRAVELERS"),
        [vec![
            ZrdValue::Text("player".to_owned()),
            ZrdValue::Text("APPROACHING".to_owned()),
            ZrdValue::List(vec![
                ZrdValue::Float(-12419.9),
                ZrdValue::Float(134.0),
                ZrdValue::Float(-10429.8),
            ]),
            ZrdValue::Float(1500.0),
            ZrdValue::Int(1),
        ]],
        "OBJECTIVE2's anchor is a spelled point — the first site measured \
         here that anchors on coordinates, not on a name"
    );
    assert_eq!(
        sites(36, "TRAVELERS"),
        [vec![
            ZrdValue::Int(4),
            ZrdValue::Text("APPROACHING".to_owned()),
            ZrdValue::Text("unit03".to_owned()),
            ZrdValue::Float(700.0),
            ZrdValue::Int(1),
        ]],
        "OBJECTIVE36's counting mode: a numeric subject, the measured \
         polarity, the anchor member and the radius — the one site the \
         lowering refuses"
    );

    // The `WAKEUP_TURRETS` wildcard: OBJECTIVE40 spells `b_turret*`, the
    // one-digit-consuming wildcard the findings measured but no measured
    // site had spelled before this mission.
    assert_eq!(
        sites(40, "WAKEUP_TURRETS"),
        [vec![ZrdValue::Text("b_turret*".to_owned())]],
        "OBJECTIVE40 wakes the balloon turrets through the wildcard spelling"
    );
    let turret_sites = binding
        .record
        .key("WAKEUP_TURRETS")
        .expect("M12 spells the turret wake key");
    assert_eq!(turret_sites.sites, 3, "three turret wake sites");

    // The mission's own start data, spelled at record level: the player
    // starts airborne at the spelled position and velocity with the
    // mission timer at zero — the checklist's "initial player
    // configuration" entry as the record carries it, not a walkthrough's.
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
                    ZrdValue::Float(-8198.0),
                    ZrdValue::Float(200.0),
                    ZrdValue::Float(-11804.0),
                ]),
                ZrdValue::List(vec![
                    ZrdValue::Float(0.0),
                    ZrdValue::Float(95.0),
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

    // The objective classes the record spells: two PRIMARY blocks and one
    // SECONDARY, carrying the localized-message ids the HUD rows resolve.
    assert_eq!(
        sites(8, "IDENTITY"),
        [vec![
            ZrdValue::Text("PRIMARY".to_owned()),
            ZrdValue::Int(1),
            ZrdValue::Text("MSG_BRF_HAM2_OBJ1".to_owned()),
        ]],
        "OBJECTIVE8 is the first primary objective"
    );
    assert_eq!(
        sites(9, "IDENTITY"),
        [vec![
            ZrdValue::Text("PRIMARY".to_owned()),
            ZrdValue::Int(3),
            ZrdValue::Text("MSG_BRF_HAM2_OBJ2".to_owned()),
        ]],
        "OBJECTIVE9 is the second primary objective — the hook chain's"
    );
    assert_eq!(
        sites(15, "IDENTITY"),
        [vec![
            ZrdValue::Text("SECONDARY".to_owned()),
            ZrdValue::Int(2),
            ZrdValue::Text("MSG_BRF_HAM2_OBJO".to_owned()),
        ]],
        "OBJECTIVE15 is the secondary objective"
    );
    let identity = binding
        .record
        .key("IDENTITY")
        .expect("M12 spells the identity key");
    assert_eq!(identity.sites, 3, "three identity sites, no more");

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

/// **M12 is not campaign-ready and the census does not hide it.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m12_b_m12_stays_unready_until_the_counting_mode_is_measured() {
    let census = census();
    assert!(!census.complete_missions().contains(&MISSION));
    assert!(!census.campaign_ready());
    assert!(census.measured_rows().any(|row| row.mission() == MISSION));
    let unmet = census.unmet_by_requirement();
    for requirement in ["objective_condition", "call_arguments"] {
        assert!(
            unmet
                .get(requirement)
                .is_some_and(|missions| missions.iter().any(|mission| mission == MISSION)),
            "the census reports M12 under {requirement}: {unmet:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Synthetic: the refusal and the spellings the retail record leans on
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

/// **The three `TRAVELERS` shapes M12 spells behave exactly as the measured
/// rule says: the counting-mode subject refuses, the point anchor and the
/// named anchor both lower.**
///
/// The retail test observes that result on the installation; this carries
/// the mechanism into CI: an authored record spelling each of M12's three
/// sites — OBJECTIVE36's numeric subject, OBJECTIVE2's three-real point and
/// OBJECTIVE51's named zeppelin — gets the refusal on the first and a
/// `Travelers` predicate carrying the spelled subject, anchor and radius on
/// the other two. A non-`APPROACHING` polarity refuses too, so a shortcut
/// that accepts every token fails here.
#[test]
fn accept_m12_b_m12s_travelers_shapes_refuse_or_lower_as_the_record_spells_them() {
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
    // writes during evaluation — refused by name, as on the installation.
    let document = authored(vec![
        zrd_int(4),
        zrd_text("APPROACHING"),
        zrd_text("unit03"),
        zrd_float(700.0),
        zrd_int(1),
    ]);
    let lowered = lower(&document);
    assert!(
        matches!(
            lowered.attempt().conditions.as_slice(),
            [ConditionOutcome::Refused(text)] if text.contains("counting mode")
        ),
        "{:?}",
        lowered.attempt().conditions
    );
    assert!(
        lowered
            .attempt()
            .validation
            .as_ref()
            .is_some_and(|problems| !problems.is_empty()),
        "validation reports the unsupported condition"
    );

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

    // The other pole is still unmeasured: a polarity token the findings
    // never recorded refuses by name, so accepting every token is a failure
    // here — exactly what keeps `LEAVING`-class spellings unknown.
    let document = authored(vec![
        zrd_text("player"),
        zrd_text("DEPARTING"),
        zrd_text("piratezep"),
        zrd_float(1500.0),
        zrd_int(1),
    ]);
    let lowered = lower(&document);
    assert!(
        matches!(
            lowered.attempt().conditions.as_slice(),
            [ConditionOutcome::Refused(text)] if text.contains("APPROACHING")
        ),
        "{:?}",
        lowered.attempt().conditions
    );
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
