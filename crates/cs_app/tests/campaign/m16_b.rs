//! Acceptance stage M16-B: M16's mission-specific compatibility surface — the
//! mission control program the installation ships for *Raid on the Rocky
//! Express*, bound through production engine systems and regressed against the
//! lowering that decides what the engine may honour
//! (`missions/M16.md`, work order `M16-B`).
//!
//! Shared contract: `docs/contracts/SCRIPT-MISSION.md` ("Source adapter
//! acceptance", "Host interface", "Objective event ordering"). Findings:
//! `docs/findings/2026-10-10-m16-b-compatibility-gaps.md`.
//!
//! # What this stage adds, and what it deliberately does not
//!
//! M16-A bound *which* retail mission the work order names (`mission/ch4-m01`,
//! `script/c4-m01-zrdr`, the reader archive `ZBD/C4/M01/zrdr.zbd`) and left
//! every objective, actor and directive unbound. This stage binds the archive
//! through the production systems M02-B built
//! (`SourceContext::control_program`) and measures it a second time through
//! `cs_app::mission_control`, so three independent derivations — the mission
//! binding, the control binding and the retail census — must agree before
//! any assertion below can pass.
//!
//! The stage's minimum acceptance scenario is *"All discovered
//! mission-specific behavior uses production engine systems and regression
//! tests."* M16's discovered mission-specific behavior is its control
//! record: 37 numbered blocks, 133 directive sites, 28 distinct keys, the
//! block graph those spell, and the two lowering gaps measured below. What is
//! different at M16:
//!
//! * **Both terminal outcomes are spelled, and both are nap-armed.**
//!   `INSTANTWIN` (OBJECTIVE20) and `INSTANTLOSS` (OBJECTIVE19) each start
//!   dormant with no timed wake; the pickup gate OBJECTIVE11 (`got_sparks`
//!   reporting EXECUTED) is the win latch's only incoming edge — a
//!   15-second nap — and the train-gone block OBJECTIVE17 (`train01` leaving
//!   play) is the loss latch's only incoming edge — also a 15-second nap —
//!   beside the nine-block kill list and the fourteen-name sound cleanup it
//!   schedules. Whether a success and a failure armed in the same window can
//!   conflict is the precedence the contract says must be measured, and it
//!   stays a runtime question (M16-C).
//! * **`STOP_QUEUED_SOUNDS` does not register.** M16 spells it seven times
//!   with 1, 3, 3, 4, 7, 9 and 14 names; the measured shapes produce one
//!   signature each, and the 9- and 14-name signatures exceed
//!   `MAX_CALL_ARGS` (8), so the whole spec is refused and every one of the
//!   seven sites — even the one-name site — reports `unknown host call`.
//!   The original reads at most ten names (`cmp esi, 0xa`, the M01-LC-D
//!   finding), so the fourteen-name site is faithful data the original
//!   truncates; carrying the name list as one list argument, the shape the
//!   other list-taking keys already use, is the follow-up this stage names.
//! * **Four spelled words are not directives.** OBJECTIVE24 spells the bare
//!   words `Change`, `to`, `mobile`, `net` — a designer note labelling the
//!   `SET_AI_NET` site below it, which the engine-image test shows absent
//!   from the measured directive table the original parser looks keys up in.
//!   The original ignores what it does not look up (the M01-LC-A finding);
//!   the measurement keeps them as unmeasured keys, and the lowering refuses
//!   rather than guesses.
//!
//! Nothing here is `verified_original` (AGENTS.md rule 8): the
//! work-order ↔ mission join remains M16-A's inference, directive *effects*
//! are the M01-LC findings' static readings of the original code, and no
//! original executable has been run. The wrong-actor, wrong-session and
//! repeated-event halves of the sheet's three priorities (moving reference
//! frame, pickup authorization, post-pickup terminal state) are runtime
//! observations: what this stage pins is the *spelled data* they are built
//! from — names, members, edges and radii — never a timing or a verdict the
//! record does not carry.
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`, the
//! engine-image member is `#[ignore = "requires CS_ENGINE_IMAGE"]`, so CI
//! (which has neither) skips them and the implementing and reviewing agents
//! run them with `--include-ignored`. The synthetic tests build `.zrd`
//! values tag by tag — no original game data is committed — and run in CI.

use std::path::PathBuf;
use std::sync::OnceLock;

use cs_app::control_lowering::lower_control_record;
use cs_app::mission_control::{RetailControlRow, survey_mission_control_programs};
use cs_assets::install::sha256;
use cs_content::campaign_bindings::{MissionLabel, SourceContext};
use cs_content::coordinates::load_engine_image;
use cs_content::mission_control::{
    AnimList, CallOutcome, ConditionOutcome, ControlRecordField, DecodedMember,
    DirectiveDisposition, TerminalOutcome, measure_control_record, objective_blocks_of,
    terminal_outcome_of,
};
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{ZrdValue, decode_zrd, objective_record, zrd_flat_fields};
use cs_formats::script_raw::discover_container;
use cs_script::ir::Value;
use cs_types::content::{ContentId, ContentKind};
use cs_types::install::RelativePath;

use crate::common::{label, load_inventory};

/// The census row label of the mission: the mission-scoped reader archive
/// F13-B's rule derives from the installation.
const MISSION: &str = "zbd/c4/m01";

/// The numbered blocks of the control member.
const BLOCKS: u32 = 37;
/// The directive sites of the control member.
const SITES: u32 = 133;
/// The distinct directive keys of the control member.
const KEYS: usize = 28;
/// The cross-objective addresses the directed keys spell (wake, nap and
/// kill lists).
const EDGES: usize = 41;
/// The `STOP_QUEUED_SOUNDS` sites and the comment-word sites that refuse.
const REFUSED: usize = 11;

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M16-B needs the retail capability; run this suite with \
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

/// M16's work-order label, from the committed inventory rather than a
/// literal.
fn m16() -> MissionLabel {
    label("M16")
}

/// M16's declared discovery title, from the committed inventory.
fn m16_title() -> String {
    load_inventory()
        .iter()
        .find(|(work_order, _)| work_order.as_str() == "M16")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M16 work order")
}

/// M16's control binding, derived fresh through production code.
fn control_binding() -> cs_content::campaign_bindings::MissionControlBinding {
    context()
        .control_program(m16(), &m16_title())
        .expect("M16's control program binds through the measured rule")
}

/// M16's row in the retail control census: the same installation measured a
/// second time through `cs_app::mission_control`.
fn census_row() -> &'static RetailControlRow {
    static ROW: OnceLock<RetailControlRow> = OnceLock::new();
    ROW.get_or_init(|| {
        let census = survey_mission_control_programs(&game_dir())
            .expect("the installation measures a control census");
        census
            .row(MISSION)
            .expect("M16's reader archive is measured by the census")
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
        .expect("M16's reader archive reads from disk");
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

/// The number a block's own key carries.
fn number_of(block: &Block) -> u32 {
    block
        .key
        .trim_start_matches("OBJECTIVE")
        .parse::<u32>()
        .expect("a block key carries its number")
}

// ---------------------------------------------------------------------------
// Retail: the binding ties M16's control program to M16's identities
// ---------------------------------------------------------------------------

/// **M16's control program is bound to the same identities as its mission
/// binding.** `SourceContext::control_program` resolves the work order
/// through the same title join `SourceContext::bind` uses, so the two
/// derivations name one mission, one program and one archive; the member the
/// binding cites is the member the measured rule picked over the archive's
/// whole member set, with a digest over that member's own bytes; and the
/// retail census — a third derivation through its own discovery path —
/// measured the same archive, the same member and the same record.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m16_b_m16s_control_program_is_bound_to_the_same_identities_as_its_mission_binding() {
    let binding = control_binding();
    let mission_binding = context()
        .bind(m16(), &m16_title())
        .expect("M16's mission binding resolves");

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
        "mission/ch4-m01",
        "M16 is the first mission of chapter 4, as M16-A bound it"
    );
    assert_eq!(
        mission_binding.campaign_position,
        Some(15),
        "the join selected campaign position 15, the sixteenth mission"
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
        "script/c4-m01-zrdr",
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
        .expect("M16's reader archive reads from disk");
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
        "M16's archive offers sixteen members"
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

    // Size and position are measurably not the rule: `sparks_pkup.zrd`, the
    // member the pickup sequence lives in, is longer than the control
    // member, and the control member is not the first.
    let longest = binding
        .members
        .iter()
        .max_by_key(|member| member.len)
        .expect("the archive declares members");
    assert_eq!(
        longest.name.to_lowercase(),
        "sparks_pkup.zrd",
        "the archive's longest member is the pickup member, not the control member"
    );
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

/// **M16's directive vocabulary partitions exactly and refuses no key — and
/// it spells both terminal latches.** Every key M16 spells carries a
/// measured disposition except four bare words: `Change`, `to`, `mobile`,
/// `net`, one site each — the designer note OBJECTIVE24 carries. Both
/// outcome keys are in the record: `INSTANTWIN` and `INSTANTLOSS`, one site
/// each. No block is unreadable, and no record-level key falls outside the
/// measured record vocabulary.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m16_b_the_measured_vocabulary_partitions_and_refuses_no_m16_key() {
    let binding = control_binding();
    let record = &binding.record;

    assert_eq!(
        (record.blocks(), record.sites()),
        (BLOCKS, SITES),
        "M16 declares {BLOCKS} numbered blocks and {SITES} directive sites"
    );
    assert_eq!(
        record.keys().iter().map(|key| key.sites).sum::<u32>(),
        record.sites(),
        "every measured site belongs to exactly one key"
    );
    assert_eq!(
        record.vocabulary() as usize,
        KEYS,
        "M16 spells {KEYS} distinct directive keys"
    );
    assert!(
        record.refusals().is_empty(),
        "the measured directive grammar parses every M16 block: {:?}",
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
            ("INSTANTWIN", TerminalOutcome::Succeeded),
        ],
        "M16's only implemented directives are its two terminal outcomes, one \
         site each — the loss latch M12's record did not carry"
    );
    assert_eq!(
        binding.unmeasured_keys(),
        ["Change", "mobile", "net", "to"],
        "the only keys no finding covers are the four bare words of OBJECTIVE24's \
         designer note"
    );
    assert_eq!(
        record.measured().len() + implemented.len() + binding.unmeasured_keys().len(),
        record.vocabulary() as usize,
        "measured + implemented + unmeasured partitions M16's vocabulary"
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

    // The outcome keys and the four comment words are the record's only
    // bare spellings; each comment word is a single bare site.
    for key in ["INSTANTWIN", "INSTANTLOSS"] {
        let outcome = record
            .key(key)
            .unwrap_or_else(|| panic!("M16 spells {key}"));
        assert!(
            outcome
                .agreed_shape()
                .is_some_and(|shape| shape.label() == "bare"),
            "{key} is spelled bare in M16"
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
        ["Change", "INSTANTLOSS", "INSTANTWIN", "mobile", "net", "to"],
        "in M16 the bare spellings are the two latches and the four comment \
         words — every measured directive spells an argument list"
    );
    for word in ["Change", "to", "mobile", "net"] {
        let key = record
            .key(word)
            .unwrap_or_else(|| panic!("M16 spells {word:?}"));
        assert_eq!(key.sites, 1, "{word:?} is a single site");
        assert!(
            matches!(key.disposition(), DirectiveDisposition::Unmeasured { .. }),
            "{word:?} stays unmeasured: no original observation states what it \
             does"
        );
    }

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
        "M16's record carries the five measured record fields, each once"
    );
    assert!(
        binding.unclassified_record_keys().is_empty(),
        "M16 spells no record-level key outside the measured vocabulary"
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
            "Change",
            "DEDG",
            "IDENTITY",
            "INACTIVE1",
            "INACTIVE2",
            "INACTIVE3",
            "INACTIVE_COMPLETION_COUNT",
            "INSTANTLOSS",
            "INSTANTWIN",
            "KILL_OBJECTIVE_WHEN_I_COMPLETE",
            "NAP_OBJECTIVE_WHEN_I_COMPLETE",
            "REMOVE_OBJECTIVE_TARGET",
            "SET_AI_NET",
            "SET_HELP_LABEL",
            "STOP_QUEUED_SOUNDS",
            "TRAVELERS",
            "WAKEUP_ENEMIES",
            "WAKEUP_TURRETS",
            "WAKEUP_ZEP_TURRETS",
            "WAKE_ANIM",
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
            "mobile",
            "net",
            "to",
        ],
        "M16's whole directive vocabulary, sorted byte-wise, so an added or \
         dropped key fails here"
    );
}

// ---------------------------------------------------------------------------
// Retail: the block graph, and the nap-armed latches
// ---------------------------------------------------------------------------

/// **Every cross-objective address M16 spells names a block this record
/// declares, both terminal latches are armed by exactly one nap each, and
/// the four empty blocks are stubs no edge reaches.**
///
/// The record declares `OBJECTIVE1` … `OBJECTIVE37`; the three
/// cross-objective keys M16 spells carry {EDGES} addresses, none zero, none
/// negative and none past the last block — the largest value M16 spells
/// *is* the block count. A spelled address names the block it decrements to
/// (the parse stores `address - 1`; that rule, and the refusal it raises
/// past the count, are M02-B-FU3's measurement of the original — Rally #802
/// — not re-measured here), so under it every address resolves to a block
/// the record declares and M16 carries **no** out-of-range address. The
/// edge assertions below are compared on that spelling. The wrong-actor,
/// wrong-session and repeated-event halves of the sheet's priorities are
/// runtime observations and stay unmeasured (M16-C).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m16_b_the_block_graph_is_closed_under_the_records_own_numbering() {
    let binding = control_binding();
    let (document, _) = control_document();
    let blocks = blocks_of(&document);
    assert_eq!(
        blocks.len() as u32,
        binding.record.blocks(),
        "the independent walk sees every numbered block the measurement counted"
    );
    let numbers: Vec<u32> = (1..=BLOCKS).collect();
    let spelled: Vec<u32> = blocks.iter().map(number_of).collect();
    assert_eq!(spelled, numbers, "numbered 1..={BLOCKS}, no gaps");

    // The three cross-objective keys M16 spells (it spells no `SLEEP_…`, no
    // `TICK_DEPENDS_ON_OBJ`, no bare `WAKE_OBJECTIVE` and no `HIDE_OBJ`),
    // and every integer their sites address.
    let directed = [
        "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
        "NAP_OBJECTIVE_WHEN_I_COMPLETE",
        "KILL_OBJECTIVE_WHEN_I_COMPLETE",
    ];
    for absent in [
        "SLEEP_OBJECTIVE_WHEN_I_COMPLETE",
        "TICK_DEPENDS_ON_OBJ",
        "WAKE_OBJECTIVE",
        "HIDE_OBJ",
    ] {
        assert!(
            binding.record.key(absent).is_none(),
            "M16 does not spell {absent}"
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
                    .unwrap_or_else(|| panic!("M16 spells {key}"))
                    .sites
            })
            .sum::<u32>(),
        "the walk visits every cross-objective site the measurement counted"
    );
    assert_eq!(
        addresses.len(),
        EDGES,
        "M16 spells {EDGES} cross-objective addresses"
    );

    // Every address names a block this record declares: none is zero or
    // negative, none is past the last block, so `address - 1` — the index
    // the parse stores — always lands inside the record's own 37 blocks.
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
        Some(1),
        "the lowest address M16 spells names OBJECTIVE1 — OBJECTIVE13 wakes \
         the record's own first block"
    );
    assert_eq!(
        addresses.iter().copied().max(),
        Some(i64::from(BLOCKS)),
        "the highest address M16 spells names the last declared block — the \
         record's own boundary is live and nothing crosses it"
    );

    // **Both latches are nap-armed.** Exactly two blocks spell an outcome —
    // OBJECTIVE19's `INSTANTLOSS` and OBJECTIVE20's `INSTANTWIN` — each
    // starts dormant with no timed wake, and each is reachable through
    // exactly one incoming edge: a 15-second nap.
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
        [(19, "INSTANTLOSS"), (20, "INSTANTWIN")],
        "OBJECTIVE19 ends the mission in failure and OBJECTIVE20 in success — \
         the only two outcome spellings M16 carries"
    );
    let incoming = |target: i64| -> Vec<(u32, String, Vec<ZrdValue>)> {
        blocks
            .iter()
            .flat_map(|block| {
                block
                    .sites
                    .iter()
                    .filter(|(key, args)| {
                        directed.contains(&key.as_str()) && integers(args).contains(&target)
                    })
                    .map(|(key, args)| (number_of(block), key.clone(), args.clone()))
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    for latch in [19, 20] {
        let edges = incoming(i64::from(latch));
        assert_eq!(
            edges.len(),
            1,
            "OBJECTIVE{latch} has exactly one incoming edge"
        );
        let (source, key, args) = &edges[0];
        assert_eq!(
            key.as_str(),
            "NAP_OBJECTIVE_WHEN_I_COMPLETE",
            "the latch's only arming is a nap — completion puts it to sleep \
             and re-wakes it after the spelled seconds"
        );
        assert_eq!(
            args.as_slice(),
            [ZrdValue::Int(latch), ZrdValue::Float(15.0)],
            "the nap re-wakes OBJECTIVE{latch} 15 seconds after its source \
             completes (the float child spells 15.0)"
        );
        assert!(
            blocks
                .iter()
                .find(|block| number_of(block) == latch)
                .is_some_and(
                    |block| block.sites.iter().any(|(key, args)| key == "BEGIN_DORMANT"
                        && args.as_slice() == [ZrdValue::Float(-1.0)])
                ),
            "OBJECTIVE{latch} starts dormant with no timed wake, so only \
             OBJECTIVE{source}'s completion can arm it"
        );
    }
    assert_eq!(
        incoming(20)[0].0,
        11,
        "the pickup gate OBJECTIVE11 is the win latch's only edge"
    );
    assert_eq!(
        incoming(19)[0].0,
        17,
        "the train-gone block OBJECTIVE17 is the loss latch's only edge"
    );

    // OBJECTIVE17's failure shape, as spelled: when `train01` no longer
    // carries the in-play bit it retires the train's target flag, plays the
    // spelled line, naps the loss latch, wakes the fourteen-name cleanup of
    // OBJECTIVE37 and kills nine blocks — the pickup gate among them, so a
    // train gone after the pickup can no longer complete the win chain
    // through the spelled record.
    let killed_by: Vec<(u32, Vec<i64>)> = blocks
        .iter()
        .map(|block| {
            (
                number_of(block),
                block
                    .sites
                    .iter()
                    .filter(|(key, _)| *key == "KILL_OBJECTIVE_WHEN_I_COMPLETE")
                    .flat_map(|(_, args)| integers(args))
                    .collect::<Vec<_>>(),
            )
        })
        .filter(|(_, list)| !list.is_empty())
        .collect();
    assert_eq!(
        killed_by,
        [(2, vec![31, 32]), (17, vec![2, 4, 5, 6, 7, 8, 11, 31, 32]),],
        "the two kill sites M16 spells, with their target lists — OBJECTIVE17's \
         retires the pickup gate and the whole turret/objective chain"
    );
    assert_eq!(
        incoming(37),
        [(
            17,
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE".to_owned(),
            vec![ZrdValue::Int(37)]
        )],
        "the fourteen-name queued-sound cleanup runs only on the failure path"
    );

    // OBJECTIVE13 — the Black Hat group's depletion — is what wakes both the
    // intro block OBJECTIVE1 and the battle-success music OBJECTIVE23: the
    // mission's armed train-turret phase starts only once that group is
    // gone, as spelled.
    assert_eq!(
        incoming(1),
        [(
            13,
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE".to_owned(),
            vec![ZrdValue::Int(1), ZrdValue::Int(23)]
        )],
        "OBJECTIVE13's completion re-wakes OBJECTIVE1 and the success-music block"
    );

    // The start structure: two blocks self-wake on their dormant timers —
    // OBJECTIVE29 at one second (the pirate zeppelin's turrets and the start
    // sound), OBJECTIVE3 at ten (the Black Hat brigands); every other
    // `BEGIN_DORMANT` disables the timed wake at −1.0, and the four empty
    // blocks spell no site at all — stubs that receive no edge, as the
    // address set above shows.
    let dormant: Vec<(u32, f32)> = blocks
        .iter()
        .filter_map(|block| {
            block
                .sites
                .iter()
                .find(|(key, _)| *key == "BEGIN_DORMANT")
                .and_then(|(_, args)| match args.as_slice() {
                    [ZrdValue::Float(seconds)] => Some((number_of(block), *seconds)),
                    _ => None,
                })
        })
        .collect();
    assert_eq!(
        dormant.len(),
        33,
        "33 of the 37 blocks spell BEGIN_DORMANT — the four empty blocks spell none"
    );
    let timed: Vec<u32> = dormant
        .iter()
        .filter(|(_, seconds)| *seconds >= 0.0)
        .map(|(block, _)| *block)
        .collect();
    assert_eq!(
        timed,
        [3, 29],
        "OBJECTIVE3 wakes itself at ten seconds and OBJECTIVE29 at one; the \
         other 31 timers are off"
    );
    let empty: Vec<u32> = blocks
        .iter()
        .filter(|block| block.sites.is_empty())
        .map(number_of)
        .collect();
    assert_eq!(
        empty,
        [9, 10, 15, 16],
        "OBJECTIVE9, 10, 15 and 16 spell no directive — inert stubs no edge \
         reaches"
    );
    for stub in [9_i64, 10, 15, 16] {
        assert!(
            incoming(stub).is_empty(),
            "OBJECTIVE{stub} receives no wake, nap or kill edge"
        );
    }
    // …and OBJECTIVE33 is the second kind of stub: it spells only a disabled
    // dormant timer and no edge reaches it either, so the record's own
    // structure never wakes it.
    let block33 = blocks
        .iter()
        .find(|block| number_of(block) == 33)
        .expect("the record declares OBJECTIVE33");
    assert_eq!(
        block33.sites.as_slice(),
        [("BEGIN_DORMANT".to_owned(), vec![ZrdValue::Float(-1.0)])],
        "OBJECTIVE33 spells only BEGIN_DORMANT(-1.0)"
    );
    assert!(
        incoming(33).is_empty(),
        "no edge reaches OBJECTIVE33 — a permanently dormant stub"
    );

    // The record the census measured and the record the binding measured are
    // the same measurement, so every pin above holds for both derivations.
    assert_eq!(
        Some(&binding.record),
        census_row().record(),
        "one measurement, two production derivations"
    );
}

/// **M16's record does not lower, and the refusal is exactly eleven named
/// calls of two kinds.** All 37 conditions lower and 122 of 133 calls bind,
/// yet `STOP_QUEUED_SOUNDS` never registers: its measured shapes include a
/// 9-name and a 14-name list, and a signature past `MAX_CALL_ARGS` makes the
/// whole spec unfit — "accepting a name means accepting every measured
/// shape it was registered with" — so all seven sites refuse `unknown host
/// call`, including the one-name site. The four bare comment words refuse
/// the same way, because an unmeasured key registers nothing by design. No
/// program is assembled, `validation` never runs and the only unmet row is
/// `call_arguments`; the mission is not complete.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m16_b_m16s_record_does_not_lower_and_eleven_of_its_calls_refuse() {
    let binding = control_binding();
    let row = census_row();
    let lowered = row
        .lowering_attempt()
        .expect("the census lowers M16's measured record");
    let attempt = lowered.attempt();
    let record = &binding.record;

    assert_eq!(
        attempt.mission.as_deref(),
        Ok("mission/ch4-m01"),
        "the lowering resolved M16's mission id"
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
    assert_eq!(
        attempt.conditions,
        vec![ConditionOutcome::Lowered; BLOCKS as usize],
        "all {BLOCKS} conditions lower — nothing in M16's condition family \
         refuses"
    );

    // Exactly eleven refused calls: the seven `STOP_QUEUED_SOUNDS` sites
    // (one registration refusal poisons the key) and the four comment words
    // of OBJECTIVE24 (unmeasured keys register nothing).
    let refused: Vec<(usize, &str)> = attempt
        .calls
        .iter()
        .enumerate()
        .filter_map(|(index, call)| match call {
            CallOutcome::Bound => None,
            CallOutcome::Refused(text) => Some((index, text.as_str())),
        })
        .collect();
    let expected: Vec<(usize, u32, u32, &str)> = vec![
        // (flat site index, zero-based block, call inside block, key)
        (14, 1, 7, "STOP_QUEUED_SOUNDS"),
        (36, 5, 7, "STOP_QUEUED_SOUNDS"),
        (45, 6, 8, "STOP_QUEUED_SOUNDS"),
        (62, 11, 5, "STOP_QUEUED_SOUNDS"),
        (68, 12, 5, "STOP_QUEUED_SOUNDS"),
        (79, 16, 7, "STOP_QUEUED_SOUNDS"),
        (93, 23, 1, "Change"),
        (94, 23, 2, "to"),
        (95, 23, 3, "mobile"),
        (96, 23, 4, "net"),
        (132, 36, 1, "STOP_QUEUED_SOUNDS"),
    ];
    assert_eq!(
        refused.len(),
        REFUSED,
        "exactly {REFUSED} of M16's {SITES} calls refuse: {refused:?}"
    );
    for ((flat, text), (site, objective, call, key)) in refused.iter().zip(&expected) {
        assert_eq!(flat, site, "site {site} is the refused call");
        assert!(
            text.contains(&format!(
                "mission/ch4-m01 objective#{objective} call {call}: unknown host call `{key}`"
            )),
            "{text}"
        );
    }
    assert_eq!(
        attempt.unbound_keys,
        ["`STOP_QUEUED_SOUNDS`: binding `STOP_QUEUED_SOUNDS`: too many arguments"],
        "the only registration refusal is the sound key's oversized signature — \
         the comment words were never registered, so they name no binding error"
    );
    assert!(
        lowered.program().is_none(),
        "refused calls mean no program is assembled — validation never runs"
    );
    assert!(
        attempt.validation.is_none(),
        "no program, so no validation verdict"
    );

    let lowering = record.lowering(attempt);
    let unmet: Vec<String> = lowering.unmet().map(|r| r.kind.code().to_owned()).collect();
    assert_eq!(
        unmet,
        ["call_arguments"],
        "the one unmet row is the call binding — the conditions all lowered"
    );
    assert!(!record.is_complete(attempt));
    assert!(!lowering.complete());
    assert!(!row.is_complete());
}

// ---------------------------------------------------------------------------
// Retail: where the sheet's three regression priorities live
// ---------------------------------------------------------------------------

/// **The three M16 regression priorities are located in the measured record
/// — as directives, operands, block edges and member bytes the archive
/// spells, not as guessed timings or coordinates — and the half of each
/// that a runtime must observe stays unmeasured.**
///
/// M16-A left all three unbound. What this stage can honestly add is the
/// *data* each one is built from, plus the negative half: the control
/// member's whole 28-key vocabulary carries no collision, damage, docking,
/// pickup or transfer directive (pinned by the vocabulary test), so the
/// priorities' actor/interaction halves live in the record's names and
/// edges and in the archive's other members — never in a directive nobody
/// measured.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m16_b_the_three_sheet_priorities_locate_in_the_measured_record() {
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

    // --- Moving reference frame: the mission rides on the train. `train01`
    // is the only name the record's target lists and membership lists agree
    // on: it is an objective target at start and again for the dock phase,
    // retired three times, it is the `INACTIVE1` member whose leaving play
    // fires the failure path, the travelers site anchors on it, the help
    // label marks it for docking, and the hellhound escorts are assigned to
    // its AI net — `M1Train` — by the record's second `SET_AI_NET` site.
    // The designer note in OBJECTIVE24 ("Change to mobile net") sits
    // directly above the other `SET_AI_NET` site, which moves the Black Hat
    // brigands onto `M1Intercept`: the mobile-net assignment is the record's
    // own spelled mechanism for putting an escort on a moving frame.
    assert_eq!(
        sites(25, "SET_AI_NET"),
        [vec![
            ZrdValue::List(vec![
                ZrdValue::Text("stihellhound_1".to_owned()),
                ZrdValue::Text("M1Train".to_owned()),
            ]),
            ZrdValue::List(vec![
                ZrdValue::Text("stihellhound_2".to_owned()),
                ZrdValue::Text("M1Train".to_owned()),
            ]),
            ZrdValue::List(vec![
                ZrdValue::Text("stihellhound_3".to_owned()),
                ZrdValue::Text("M1Train".to_owned()),
            ]),
            ZrdValue::List(vec![
                ZrdValue::Text("stihellhound_4".to_owned()),
                ZrdValue::Text("M1Train".to_owned()),
            ]),
        ]],
        "OBJECTIVE25 puts the four hellhound escorts on the train's AI net"
    );
    assert_eq!(
        sites(24, "SET_AI_NET"),
        [vec![
            ZrdValue::List(vec![
                ZrdValue::Text("bhatbrigand_1".to_owned()),
                ZrdValue::Text("M1Intercept".to_owned()),
            ]),
            ZrdValue::List(vec![
                ZrdValue::Text("bhatbrigand_2".to_owned()),
                ZrdValue::Text("M1Intercept".to_owned()),
            ]),
            ZrdValue::List(vec![
                ZrdValue::Text("bhatbrigand_3".to_owned()),
                ZrdValue::Text("M1Intercept".to_owned()),
            ]),
            ZrdValue::List(vec![
                ZrdValue::Text("bhatbrigand_4".to_owned()),
                ZrdValue::Text("M1Intercept".to_owned()),
            ]),
        ]],
        "OBJECTIVE24 moves the four Black Hat brigands onto the intercept net"
    );
    let comment_block = &block(24).sites;
    let change_at = comment_block
        .iter()
        .position(|(key, _)| key == "Change")
        .expect("OBJECTIVE24 spells the note");
    assert_eq!(
        comment_block[change_at..change_at + 4]
            .iter()
            .map(|(key, args)| (key.as_str(), args.as_slice()))
            .collect::<Vec<_>>(),
        [
            ("Change", &[][..]),
            ("to", &[][..]),
            ("mobile", &[][..]),
            ("net", &[][..]),
        ],
        "the four bare words spell the note `Change to mobile net`, directly \
         above the mobile-net assignment"
    );
    assert_eq!(
        sites(34, "TRAVELERS"),
        [vec![
            ZrdValue::Text("player".to_owned()),
            ZrdValue::Text("APPROACHING".to_owned()),
            ZrdValue::Text("train01".to_owned()),
            ZrdValue::Float(1500.0),
            ZrdValue::Int(1),
        ]],
        "OBJECTIVE34 completes when the player closes on the train within \
         the spelled radius — the approach gate that plays `snd_RM1Train`"
    );
    assert_eq!(
        sites(17, "INACTIVE1"),
        [vec![
            ZrdValue::Text("train01".to_owned()),
            ZrdValue::Text("healthy".to_owned()),
        ]],
        "OBJECTIVE17 completes when the train no longer carries the in-play \
         bit — the spelled failure trigger"
    );
    assert_eq!(
        sites(8, "SET_HELP_LABEL"),
        [vec![
            ZrdValue::Text("train01".to_owned()),
            ZrdValue::Text("MSG_OBJ_DOCK".to_owned()),
        ]],
        "OBJECTIVE8 marks the train with the dock help label"
    );
    let train_targets: Vec<u32> = blocks
        .iter()
        .filter(|block| {
            block.sites.iter().any(|(key, args)| {
                (key == "ADD_OBJECTIVE_TARGET" || key == "REMOVE_OBJECTIVE_TARGET")
                    && texts(args).contains(&"train01")
            })
        })
        .map(number_of)
        .collect();
    assert_eq!(
        train_targets,
        [1, 2, 8, 11, 17],
        "the train is targeted and retired in these blocks, as spelled"
    );
    assert_eq!(
        sites(1, "WAKE_ANIM"),
        [vec![ZrdValue::Text("tsega1".to_owned())]],
        "OBJECTIVE1 wakes the train segment animation at start of the armed phase"
    );
    assert_eq!(
        sites(1, "WAKEUP_TURRETS"),
        [vec![ZrdValue::Text("tcargun**".to_owned())]],
        "OBJECTIVE1 wakes the train cargo guns through the two-wildcard \
         spelling — each `*` consumes one character of an entry's name, so \
         `tcargun**` matches the two-digit tcargun01…03"
    );

    // --- Pickup authorization: the rescue is an animation gate, not a
    // directive. OBJECTIVE11 — a PRIMARY objective — completes only when the
    // named animation `got_sparks` reports EXECUTED, and its completion is
    // the win latch's only incoming edge (graph test). The archive carries
    // the pickup sequence's own members (`pickups`, `sparks_pkup`, `ladder`)
    // beside the control member; the control member itself spells no
    // docking or transfer directive at all.
    assert_eq!(
        sites(11, "ANIM_STATE"),
        [vec![
            ZrdValue::Text("ANIM".to_owned()),
            ZrdValue::List(vec![
                ZrdValue::Text("NAME".to_owned()),
                ZrdValue::List(vec![ZrdValue::Text("got_sparks".to_owned())]),
                ZrdValue::Text("STATE".to_owned()),
                ZrdValue::List(vec![ZrdValue::Text("EXECUTED".to_owned())]),
            ]),
        ]],
        "OBJECTIVE11 completes when the spelled pickup animation reports \
         EXECUTED — the rescue is an animation-state gate"
    );
    assert_eq!(
        sites(11, "IDENTITY"),
        [vec![
            ZrdValue::Text("PRIMARY".to_owned()),
            ZrdValue::Int(3),
            ZrdValue::Text("MSG_BRF_RMM1_OBJ3".to_owned()),
        ]],
        "the pickup gate is the mission's third primary objective"
    );
    for member in ["pickups.zrd", "sparks_pkup.zrd", "ladder.zrd"] {
        assert!(
            binding
                .members
                .iter()
                .any(|row| row.name.eq_ignore_ascii_case(member)),
            "the archive carries the pickup member {member}"
        );
    }

    // --- Post-pickup terminal state: the pickup gate's only outgoing
    // completion edge is the 15-second nap that arms INSTANTWIN; the
    // failure path (train gone) is the same 15-second nap arming
    // INSTANTLOSS, and it kills the pickup gate itself. Whether the two
    // naps can fire in one window, and which outcome then wins, is the
    // precedence the contract requires measuring — a runtime question this
    // stage leaves open (M16-C).
    let nap_edges: Vec<(u32, Vec<i64>, f32)> = blocks
        .iter()
        .flat_map(|block| {
            block
                .sites
                .iter()
                .filter(|(key, _)| *key == "NAP_OBJECTIVE_WHEN_I_COMPLETE")
                .filter_map(|(_, args)| match args.as_slice() {
                    [ZrdValue::Int(target), ZrdValue::Float(seconds)] => {
                        Some((number_of(block), vec![i64::from(*target)], *seconds))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(
        nap_edges
            .iter()
            .filter(|(_, targets, _)| targets.contains(&20))
            .cloned()
            .collect::<Vec<_>>()
            .as_slice(),
        [(11, vec![20], 15.0)],
        "OBJECTIVE11's completion is the win latch's only arming"
    );
    assert_eq!(
        nap_edges
            .iter()
            .filter(|(_, targets, _)| targets.contains(&19))
            .cloned()
            .collect::<Vec<_>>()
            .as_slice(),
        [(17, vec![19], 15.0)],
        "OBJECTIVE17's completion is the loss latch's only arming"
    );

    // The objective classes the record spells: four PRIMARY blocks and one
    // SECONDARY — OBJECTIVE8's and OBJECTIVE12's identities stop after
    // their slot ordinals with no `MSG_*` text.
    let identities: Vec<(u32, Vec<ZrdValue>)> = blocks
        .iter()
        .flat_map(|block| {
            block
                .sites
                .iter()
                .filter(|(key, _)| *key == "IDENTITY")
                .map(|(_, args)| (number_of(block), args.clone()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(
        identities
            .iter()
            .map(|(block, args)| (*block, args.first()))
            .collect::<Vec<_>>(),
        [
            (1, Some(&ZrdValue::Text("PRIMARY".to_owned()))),
            (7, Some(&ZrdValue::Text("PRIMARY".to_owned()))),
            (8, Some(&ZrdValue::Text("PRIMARY".to_owned()))),
            (11, Some(&ZrdValue::Text("PRIMARY".to_owned()))),
            (12, Some(&ZrdValue::Text("SECONDARY".to_owned()))),
        ],
        "M16's five identity sites: four primary objectives and one secondary"
    );
    assert_eq!(
        sites(8, "IDENTITY"),
        [vec![ZrdValue::Text("PRIMARY".to_owned()), ZrdValue::Int(4),]],
        "OBJECTIVE8's identity carries no MSG_* text — with the SECONDARY \
         OBJECTIVE12, the two of the five that omit it"
    );

    // The negative half, checked on the whole vocabulary rather than by
    // absence of evidence: no key M16 spells names a collision, a damage
    // value, a docking or a transfer — the interaction families are not in
    // this member.
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

    // The mission's own start data, spelled at record level: the player
    // starts at the spelled position with **zero** spelled velocity and the
    // mission timer at zero — the checklist's "initial player
    // configuration" entry as the record carries it, not a walkthrough's.
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
                    ZrdValue::Float(-7634.0),
                    ZrdValue::Float(600.0),
                    ZrdValue::Float(-1363.0),
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
        "the authored player start: airborne at the spelled position with a \
         zero spelled velocity — the record's own data, not a walkthrough's"
    );
    for name in ["RESTORE_ANIMS", "EXECUTE_ANIMS", "INVALIDATE_ANIMS"] {
        assert_eq!(
            field(name).as_list(),
            Some(&[][..]),
            "{name} is spelled empty"
        );
    }

    // Whether the wrong actor, the wrong session or a repeated event can
    // satisfy any of these transitions is runtime behaviour: nothing here
    // simulates it, and a complete census row says nothing about it — it is
    // a lowering claim. The runtime halves stay open for M16-C, and the
    // campaign gate still keeps every runtime out while any measured row
    // carries a gap.
    assert!(
        !census().campaign_ready(),
        "M16's transitions stay unobserved: no campaign runtime may start while the \
         campaign gate is closed"
    );
}

// ---------------------------------------------------------------------------
// Engine image: the four comment words are not directive keys
// ---------------------------------------------------------------------------

/// The directive-key string table's file extent in the owner's decrypted
/// executable — the range `docs/findings/2026-10-06-m01-lc-directive-a-…`
/// measured (VA `0x626040..0x6268c0`; for `.data`, RVA equals file offset).
const DIRECTIVE_TABLE: (usize, usize) = (0x226040, 0x2268c0);

/// **The four bare words OBJECTIVE24 spells are not in the directive-key
/// table the original parser looks names up in — so the original ignores
/// them, and the record's unmeasured disposition is the faithful one.**
///
/// The M01-LC-A finding measured both facts this test combines: the
/// parser's `0x57a090(record, "KEY")` lookups mean any key it does not look
/// up is ignored, and the table's string extent is `0x226040..0x2268c0`.
/// Every completion-effect key M16 spells sits in that extent; `Change`,
/// `to`, `mobile` and `net` do not — and `Change`, `to` and `mobile` are
/// not standalone strings anywhere in the image at all. (`net` is: once, at
/// `0x22b75c`, inside the airframe-parameter name cluster beside
/// `max_accel` and `cannon_fire_delay` — a different lookup with no
/// mission-directive meaning.)
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m16_b_the_four_comment_words_are_not_in_the_measured_directive_table() {
    let image = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let (start, end) = DIRECTIVE_TABLE;
    let region = image
        .bytes
        .get(start..end)
        .expect("the directive table extent lies inside the image");

    // The NUL-terminated entries of the measured extent.
    let entries: Vec<&str> = region
        .split(|byte| *byte == 0)
        .filter(|word| !word.is_empty())
        .map(|word| {
            std::str::from_utf8(word).unwrap_or_else(|error| panic!("the table is ASCII: {error}"))
        })
        .collect();
    assert!(
        entries.len() >= 80,
        "the measured table carries its strings: {}",
        entries.len()
    );

    // Every key of M16's vocabulary that lives in this table is present —
    // the completion-effect family the parser looks up here. `ANIM_STATE`,
    // the `INACTIVE*` members and `INACTIVE_COMPLETION_COUNT` live in the
    // evaluator table (it spells `INACTIVE%d`, `EXECUTED`, `TEST_EQ`), and
    // `NAP_OBJECTIVE_WHEN_I_COMPLETE`'s string begins inside this extent
    // but ends past it, so it is checked at its own address below.
    for key in binding_keys_in_table() {
        assert!(
            entries.contains(key),
            "{key} must be a string in the measured directive table"
        );
    }
    let nap_start = file_offset_of(&image.bytes, b"NAP_OBJECTIVE_WHEN_I_COMPLETE\0")
        .expect("the nap key's string is in the image");
    assert_eq!(
        nap_start, 0x2268b0,
        "the nap key sits at the extent's end, as the finding measured"
    );

    // The four comment words are absent from the table — the original's
    // lookup cannot find them, so under the measured "unlooked-up keys are
    // ignored" rule they are inert text the shipped record carries.
    for word in ["Change", "to", "mobile", "net"] {
        assert!(
            !entries.contains(&word),
            "{word:?} must not be a directive-table entry"
        );
    }
    for word in ["Change", "to", "mobile"] {
        assert!(
            standalone_offsets(&image.bytes, word.as_bytes()).is_empty(),
            "{word:?} is not a standalone string anywhere in the image — only \
             inside longer strings such as `Save Changes?` or `Change Net Params`"
        );
    }
    // `net` is the honest exception in detail, not in kind: exactly one
    // standalone `net` exists in the image, and it sits in the
    // airframe-parameter name cluster — outside every mission-directive
    // table — so the record's `net` still resolves to no directive.
    let nets = standalone_offsets(&image.bytes, b"net");
    assert_eq!(
        nets,
        [0x22b75c],
        "the image's one standalone `net` is the parameter name, not a directive"
    );
    for neighbour in [b"max_accel\0".as_slice(), b"cannon_fire_delay\0".as_slice()] {
        let offset = file_offset_of(&image.bytes, neighbour)
            .expect("the parameter cluster names its fields");
        assert!(
            (0x22b700..0x22b800).contains(&offset),
            "{neighbour:?} sits beside the lone `net` in the parameter cluster"
        );
    }
}

/// M16's directive keys whose names the measured table carries.
fn binding_keys_in_table() -> &'static [&'static str] {
    &[
        "ADD_OBJECTIVE_TARGET",
        "BEGIN_DORMANT",
        "COMPLETED_SOUND_GROUP",
        "DEDG",
        "IDENTITY",
        "INSTANTLOSS",
        "INSTANTWIN",
        "KILL_OBJECTIVE_WHEN_I_COMPLETE",
        "REMOVE_OBJECTIVE_TARGET",
        "SET_AI_NET",
        "SET_HELP_LABEL",
        "STOP_QUEUED_SOUNDS",
        "TRAVELERS",
        "WAKEUP_ENEMIES",
        "WAKEUP_TURRETS",
        "WAKEUP_ZEP_TURRETS",
        "WAKE_ANIM",
        "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
    ]
}

/// The first file offset of `needle`, or `None` when it is absent.
fn file_offset_of(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Every file offset where `word` stands alone — NUL on both sides.
fn standalone_offsets(haystack: &[u8], word: &[u8]) -> Vec<usize> {
    let mut found = Vec::new();
    let mut needle = Vec::with_capacity(word.len() + 2);
    needle.push(0);
    needle.extend_from_slice(word);
    needle.push(0);
    let mut start = 0;
    while let Some(at) = haystack[start..]
        .windows(needle.len())
        .position(|window| window == needle.as_slice())
    {
        found.push(start + at + 1);
        start += at + 1;
    }
    found
}

// ---------------------------------------------------------------------------
// Retail: M16 is not campaign-ready and the census does not hide it
// ---------------------------------------------------------------------------

/// **M16 is not campaign-ready and the census does not hide it.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m16_b_m16_stays_unready_while_its_sound_cleanup_and_comment_calls_refuse() {
    let census = census();
    assert!(!census.complete_missions().contains(&MISSION));
    assert!(!census.campaign_ready());
    assert!(census.measured_rows().any(|row| row.mission() == MISSION));
    let unmet = census.unmet_by_requirement();
    assert!(
        unmet
            .get("call_arguments")
            .is_some_and(|missions| missions.iter().any(|mission| mission == MISSION)),
        "the census reports M16 under call_arguments: {unmet:?}"
    );
    for requirement in [
        "objective_condition",
        "mission_identity",
        "objective_identity",
    ] {
        assert!(
            !unmet
                .get(requirement)
                .is_some_and(|missions| missions.iter().any(|mission| mission == MISSION)),
            "M16 must not appear under {requirement}: its conditions all lower \
             and its identity resolves — {unmet:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Synthetic: the refusal mechanisms the retail record leans on
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

/// One authored bare directive site (a key with a text follower or none).
fn bare(key: &str) -> Vec<ZrdValue> {
    vec![zrd_text(key)]
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
        ContentId::from_source(ContentKind::Mission, "accept-m16-b")
            .map_err(|error| error.to_string()),
        "accept-m16-b",
        document,
        &record,
    )
}

/// **One `STOP_QUEUED_SOUNDS` site past `MAX_CALL_ARGS` poisons the whole
/// key — a shorter site in the same record refuses too — and a record whose
/// sites all fit registers and binds the key.**
///
/// The retail test observes M16's refusal on the installation; this carries
/// the mechanism into CI: an authored record spelling a nine-name site —
/// inside the original's measured ten-name cap but outside the registry's
/// eight-argument bound — beside a one-name site refuses both, because one
/// unfit signature makes the spec unfit rather than narrowing what the name
/// accepts. The same record with every site at eight names or fewer binds
/// every call, so the failure is the oversized signature, not the key.
#[test]
fn accept_m16_b_one_oversized_sound_site_poisons_the_whole_sound_key() {
    let sound_sites = |counts: &[usize]| -> Vec<(String, ZrdValue)> {
        counts
            .iter()
            .enumerate()
            .map(|(index, count)| {
                block(
                    (index + 1) as u32,
                    vec![
                        directive("BEGIN_DORMANT", vec![zrd_float(-1.0)]),
                        directive(
                            "STOP_QUEUED_SOUNDS",
                            (0..*count).map(|n| zrd_text(&format!("snd_{n}"))).collect(),
                        ),
                    ],
                )
            })
            .collect()
    };

    // M16's shape, in miniature: a nine-name site and a one-name site.
    let document = control_record(sound_sites(&[9, 1]));
    let lowered = lower(&document);
    let attempt = lowered.attempt();
    assert_eq!(
        attempt.unbound_keys,
        ["`STOP_QUEUED_SOUNDS`: binding `STOP_QUEUED_SOUNDS`: too many arguments"],
        "one oversized signature unregisters the whole key: {:?}",
        attempt.unbound_keys
    );
    let refused: Vec<&str> = attempt
        .calls
        .iter()
        .filter_map(|call| match call {
            CallOutcome::Bound => None,
            CallOutcome::Refused(text) => Some(text.as_str()),
        })
        .collect();
    assert_eq!(refused.len(), 2, "both sound sites refuse: {refused:?}");
    assert!(
        refused
            .iter()
            .all(|text| text.contains("unknown host call `STOP_QUEUED_SOUNDS`")),
        "the one-name site refuses too — the key itself is gone: {refused:?}"
    );
    assert!(lowered.program().is_none(), "no program assembles");

    // The bound is the signature's argument count, not the key: the same
    // sites at eight names or fewer register and bind.
    let document = control_record(sound_sites(&[8, 1]));
    let lowered = lower(&document);
    let attempt = lowered.attempt();
    assert!(
        attempt.unbound_keys.is_empty(),
        "a record whose sites fit the bound registers the key: {:?}",
        attempt.unbound_keys
    );
    assert!(
        attempt
            .calls
            .iter()
            .all(|call| matches!(call, CallOutcome::Bound)),
        "every site binds: {:?}",
        attempt.calls
    );
}

/// **Unmeasured bare keys refuse like any unknown call — and a record that
/// spells both latches, dormant timers, a nap edge and a travelers gate
/// lowers and validates clean.**
///
/// OBJECTIVE24's four bare words are registered to nothing, because an
/// unmeasured key registers nothing by design; this authors M16's comment
/// block and pins the refusal shape. The companion record spells M16's
/// terminal *shape* — a nap-armed `INSTANTWIN` beside a nap-armed
/// `INSTANTLOSS` — with every call bindable: it assembles, validates clean
/// and counts complete, so what keeps M16 out is the two named gaps, not a
/// program shape the engine cannot carry.
#[test]
fn accept_m16_b_unmeasured_bare_keys_refuse_but_a_two_latch_program_validates() {
    // OBJECTIVE24's comment block: the four bare words plus a bindable
    // list-argument site, exactly as spelled.
    let document = control_record(vec![block(
        24,
        vec![
            directive("BEGIN_DORMANT", vec![zrd_float(-1.0)]),
            bare("Change"),
            bare("to"),
            bare("mobile"),
            bare("net"),
            directive(
                "SET_AI_NET",
                vec![zrd_list(vec![
                    zrd_list(vec![zrd_text("bhatbrigand_1"), zrd_text("M1Intercept")]),
                    zrd_list(vec![zrd_text("bhatbrigand_2"), zrd_text("M1Intercept")]),
                ])],
            ),
        ],
    )]);
    let record = measure_control_record(&document);
    assert_eq!(
        record
            .unmeasured()
            .into_iter()
            .map(|(key, _)| key.key.as_str())
            .collect::<Vec<_>>(),
        ["Change", "mobile", "net", "to"],
        "the four bare words are the record's unmeasured keys"
    );
    let lowered = lower(&document);
    let attempt = lowered.attempt();
    let refused: Vec<&str> = attempt
        .calls
        .iter()
        .filter_map(|call| match call {
            CallOutcome::Bound => None,
            CallOutcome::Refused(text) => Some(text.as_str()),
        })
        .collect();
    assert_eq!(
        refused.len(),
        4,
        "the four bare words refuse, the measured sites bind: {refused:?}"
    );
    for (text, word) in refused.iter().zip(["Change", "to", "mobile", "net"]) {
        assert!(
            text.contains(&format!("unknown host call `{word}`")),
            "{text}"
        );
    }
    assert!(lowered.program().is_none(), "no program assembles");

    // M16's terminal shape: a nap edge arms each latch after the pickup
    // gate and the train-gone block complete. Both latches, the nap and
    // the gate are bindable, so this program validates clean.
    let document = control_record(vec![
        block(
            11,
            vec![
                directive("BEGIN_DORMANT", vec![zrd_float(-1.0)]),
                directive(
                    "TRAVELERS",
                    vec![
                        zrd_text("player"),
                        zrd_text("APPROACHING"),
                        zrd_text("train01"),
                        zrd_float(1500.0),
                        zrd_int(1),
                    ],
                ),
                directive(
                    "NAP_OBJECTIVE_WHEN_I_COMPLETE",
                    vec![zrd_int(3), zrd_float(15.0)],
                ),
            ],
        ),
        block(
            17,
            vec![
                directive("BEGIN_DORMANT", vec![zrd_float(-1.0)]),
                directive("INACTIVE1", vec![zrd_text("train01"), zrd_text("healthy")]),
                directive(
                    "NAP_OBJECTIVE_WHEN_I_COMPLETE",
                    vec![zrd_int(4), zrd_float(15.0)],
                ),
            ],
        ),
        block(
            19,
            vec![
                directive("BEGIN_DORMANT", vec![zrd_float(-1.0)]),
                bare("INSTANTLOSS"),
            ],
        ),
        block(
            20,
            vec![
                directive("BEGIN_DORMANT", vec![zrd_float(-1.0)]),
                bare("INSTANTWIN"),
            ],
        ),
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
    assert_eq!(
        attempt.validation,
        Some(Vec::new()),
        "a two-latch, nap-armed record validates clean"
    );
    assert!(
        record.is_complete(attempt),
        "a record of M16's shape lowers completely — the refusal is the two \
         named gaps, not the shape: unbound={:?}",
        attempt.unbound_keys
    );
    // The emitted calls carry the spelled data field for field: the nap's
    // target and seconds, the travelers site's five operands.
    let raw = lowered.raw_program().expect("the program assembled");
    let nap = raw.objectives[0]
        .calls
        .iter()
        .find(|call| call.name == "NAP_OBJECTIVE_WHEN_I_COMPLETE")
        .expect("the nap edge is emitted");
    assert_eq!(
        nap.args.as_slice(),
        [Value::Int(3), Value::Float(15.0)],
        "the nap's target and seconds arrive as the two spelled arguments"
    );
}
