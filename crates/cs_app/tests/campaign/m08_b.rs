//! Acceptance stage M08-B: M08's mission-specific compatibility surface — the
//! mission control program the installation ships for *The Petrol Plot*,
//! bound through production engine systems and regressed against the lowering
//! that decides what the engine may honour
//! (`missions/M08.md`, work order `M08-B`, Rally #280).
//!
//! Shared contract: `docs/contracts/SCRIPT-MISSION.md` ("Source adapter
//! acceptance", "Host interface", "Objective event ordering"). Findings:
//! `docs/findings/2026-10-09-m08-b-compatibility-gaps.md`.
//!
//! # What this stage adds, and what it deliberately does not
//!
//! M08-A bound *which* retail mission the work order names and left every
//! objective, actor and directive unbound: its own note says the three
//! regression priorities (logistics state, alternative objective paths,
//! protected transfer) "remain for M08-B once the mission program is
//! decoded". M08's program is the reader archive
//! `ZBD/C2/M03/zrdr.zbd`, and this stage binds it through the production
//! systems M02-B built (`SourceContext::control_program`) and measures it a
//! second time through `cs_app::mission_control`, so three independent
//! derivations — the mission binding, the control binding and the retail
//! census — must agree before any assertion below can pass.
//!
//! The stage's minimum acceptance scenario is *"All discovered
//! mission-specific behavior uses production engine systems and regression
//! tests."* M08's discovered mission-specific behavior is its control
//! record: 57 numbered blocks, 209 directive sites, 22 distinct keys, the
//! block graph those spell, and the two lowering gaps measured below.
//!
//! * **The lowering is pinned on both sides of the gap it was written
//!   against.** When this stage measured M08, its 19
//!   `KILL_OBJECTIVE_WHEN_I_COMPLETE` sites refused against the host-call
//!   registry's per-signature bound and eight `DANGER_ZONES_COMPLETED`
//!   completion conditions were refused because this build lowers no
//!   condition for the danger-zones flag evaluator. #800 landed on `main`
//!   while this branch was in flight and closed the first half — the
//!   adapter now carries an objective-index list as one list argument — so
//!   the tests pin that every one of M08's 209 sites binds *and* that the
//!   eight danger-zones predicates are the only unmet requirement left
//!   (#813 owns them). The record still does not lower completely, the
//!   census row stays incomplete and the campaign gate stays closed.
//! * **The three sheet priorities are located in the measured record** as
//!   directives, operands and block edges the record spells — never as a
//!   timing, count or coordinate it does not. Their predicates (the wrong
//!   actor, the wrong session, a repeated event) are runtime observations
//!   and stay unmeasured: M08 has no runtime consumer yet and ordinary-play
//!   evidence is M08-C's.
//!
//! Nothing here is `verified_original` (AGENTS.md rule 8): the
//! work-order ↔ mission join remains M08-A's inference, directive *effects*
//! are the M01-LC findings' static readings of the original code, and no
//! original executable has been run.
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
use cs_script::bindings::MAX_CALL_ARGS;
use cs_script::ir::Value;
use cs_types::content::{ContentId, ContentKind};
use cs_types::install::RelativePath;

use crate::common::{label, load_inventory};

/// The census row label of the mission: the mission-scoped reader archive
/// F13-B's rule derives from the installation.
const MISSION: &str = "zbd/c2/m03";

/// The numbered blocks of the control member.
const BLOCKS: u32 = 57;
/// The directive sites of the control member.
const SITES: u32 = 209;
/// The distinct directive keys of the control member.
const KEYS: usize = 22;
/// The sites of the kill key, every one of which binds now that the adapter
/// carries an objective-index list as one list argument (#800).
const KILL_SITES: u32 = 19;
/// The completion conditions this build refuses (the danger-zones sites).
const REFUSED_CONDITIONS: usize = 8;

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M08-B needs the retail capability; run this suite with \
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

/// M08's work-order label, from the committed inventory rather than a
/// literal.
fn m08() -> MissionLabel {
    label("M08")
}

/// M08's declared discovery title, from the committed inventory.
fn m08_title() -> String {
    load_inventory()
        .iter()
        .find(|(work_order, _)| work_order.as_str() == "M08")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M08 work order")
}

/// M08's control binding, derived fresh through production code.
fn control_binding() -> cs_content::campaign_bindings::MissionControlBinding {
    context()
        .control_program(m08(), &m08_title())
        .expect("M08's control program binds through the measured rule")
}

/// M08's row in the retail control census: the same installation measured a
/// second time through `cs_app::mission_control`.
fn census_row() -> &'static RetailControlRow {
    static ROW: OnceLock<RetailControlRow> = OnceLock::new();
    ROW.get_or_init(|| {
        let census = survey_mission_control_programs(&game_dir())
            .expect("the installation measures a control census");
        census
            .row(MISSION)
            .expect("M08's reader archive is measured by the census")
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
        .expect("M08's reader archive reads from disk");
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
// Retail: the binding ties M08's control program to M08's identities
// ---------------------------------------------------------------------------

/// **M08's control program is bound to the same identities as its mission
/// binding.** `SourceContext::control_program` resolves the work order
/// through the same title join `SourceContext::bind` uses, so the two
/// derivations name one mission, one program and one archive; the member the
/// binding cites is the member the measured rule picked over the archive's
/// whole member set, with a digest over that member's own bytes; and the
/// retail census — a third derivation through its own discovery path —
/// measured the same archive, the same member and the same record.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m08_b_m08s_control_program_is_bound_to_the_same_identities_as_its_mission_binding() {
    let binding = control_binding();
    let mission_binding = context()
        .bind(m08(), &m08_title())
        .expect("M08's mission binding resolves");

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
        "mission/ch2-m03",
        "M08 is the third mission of chapter 2, as M08-A bound it"
    );
    assert_eq!(
        mission_binding.campaign_position,
        Some(7),
        "the join selected campaign position 7, which M08-A measured as the \
         third mission of chapter 2"
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
        "script/c2-m03-zrdr",
        "the program identity is the world group's reader archive for chapter 2, mission 3"
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
        .expect("M08's reader archive reads from disk");
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
        13,
        "M08's archive offers thirteen members, as M02's did twenty"
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

/// **M08's directive vocabulary partitions exactly and refuses no key.**
/// This is what makes the gap pinned below a *lowering* gap and not an
/// unknown directive: every key M08 spells carries a measured disposition
/// or is one of the two terminal outcome spellings, no block is unreadable,
/// and — unlike M02 — no record-level key falls outside the measured record
/// vocabulary.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m08_b_the_measured_vocabulary_partitions_and_refuses_no_m08_key() {
    let binding = control_binding();
    let record = &binding.record;

    assert_eq!(
        (record.blocks(), record.sites()),
        (BLOCKS, SITES),
        "M08 declares {BLOCKS} numbered blocks and {SITES} directive sites"
    );
    assert_eq!(
        record.keys().iter().map(|key| key.sites).sum::<u32>(),
        record.sites(),
        "every measured site belongs to exactly one key"
    );
    assert_eq!(
        record.vocabulary() as usize,
        KEYS,
        "M08 spells {KEYS} distinct directive keys"
    );
    assert!(
        record.refusals().is_empty(),
        "the measured directive grammar parses every M08 block: {:?}",
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
        "M08's only implemented directives are its two outcome spellings, one \
         site each"
    );
    assert_eq!(
        binding.unmeasured_keys(),
        Vec::<String>::new(),
        "every key M08 spells is covered by the M01-LC findings"
    );
    assert_eq!(
        record.measured().len() + implemented.len(),
        record.vocabulary() as usize,
        "measured + implemented partitions M08's vocabulary"
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
    // names; no other key in M08's vocabulary is spelled bare.
    for key in ["INSTANTWIN", "INSTANTLOSS"] {
        let spelled = record.key(key).expect("M08 spells its outcome keys");
        assert!(
            spelled
                .agreed_shape()
                .is_some_and(|shape| shape.label() == "bare"),
            "{key} is spelled bare in M08"
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
        "in M08 every bare key spells an outcome"
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
        "M08's record carries the five measured record fields, each once"
    );
    assert!(
        binding.unclassified_record_keys().is_empty(),
        "M08 spells no record-level key outside the measured vocabulary (M02 \
         spelled five sound keys there)"
    );

    // The exact key list: pinning it is also the proof that the vocabulary
    // carries no interaction, docking, pickup or transfer directive — the
    // contract's interaction family is not in this member at all, which the
    // priorities test below relies on.
    let mut keys: Vec<&str> = record.keys().iter().map(|key| key.key.as_str()).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
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
        "M08's whole directive vocabulary, sorted byte-wise, so an added or \
         dropped key fails here"
    );
}

// ---------------------------------------------------------------------------
// Retail: the block graph, and both measured lowering gaps
// ---------------------------------------------------------------------------

/// **Every cross-objective address M08 spells names a block this record
/// declares, and every edge lands where the mission's structure says it
/// does.**
///
/// The record declares `OBJECTIVE1` … `OBJECTIVE57`; the three
/// cross-objective keys M08 spells carry 151 addresses, none zero, none
/// negative and none past the last block — the largest value M08 spells
/// *is* the block count. A spelled address names the block it decrements
/// to (the parse stores `address - 1`; that rule, and the refusal it raises
/// past the count, are M02-B-FU3's measurement of the original — Rally
/// #802 — not re-measured here), so under it every address resolves to a
/// block the record declares and M08 carries **no** out-of-range address.
/// The edge assertions below are compared on that spelling, which is also
/// what makes the success latch land on the block the mission's structure
/// names. The wrong-actor, wrong-session and repeated-event halves of the
/// sheet's priorities are runtime observations and stay unmeasured (M08-C).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m08_b_the_block_graph_is_closed_under_the_records_own_numbering() {
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

    // The three cross-objective keys M08 spells (it spells no
    // `SLEEP_…`, no bare `WAKE_OBJECTIVE`, no `TICK_DEPENDS_ON_OBJ` and no
    // `HIDE_OBJ`), and every integer their sites address.
    let directed = [
        "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
        "KILL_OBJECTIVE_WHEN_I_COMPLETE",
        "NAP_OBJECTIVE_WHEN_I_COMPLETE",
    ];
    for absent in [
        "SLEEP_OBJECTIVE_WHEN_I_COMPLETE",
        "WAKE_OBJECTIVE",
        "TICK_DEPENDS_ON_OBJ",
        "HIDE_OBJ",
    ] {
        assert!(
            binding.record.key(absent).is_none(),
            "M08 does not spell {absent}"
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
        binding
            .record
            .key("WAKE_OBJECTIVE_WHEN_I_COMPLETE")
            .expect("M08 spells the wake key")
            .sites
            + binding
                .record
                .key("KILL_OBJECTIVE_WHEN_I_COMPLETE")
                .expect("M08 spells the kill key")
                .sites
            + binding
                .record
                .key("NAP_OBJECTIVE_WHEN_I_COMPLETE")
                .expect("M08 spells the nap key")
                .sites,
        "the walk visits every cross-objective site the measurement counted"
    );
    assert_eq!(
        addresses.len(),
        151,
        "M08 spells 151 cross-objective addresses"
    );

    // Every address names a block this record declares: none is zero or
    // negative, none is past the last block, so `address - 1` — the index
    // the parse stores — always lands inside the record's own 57 blocks.
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
        Some(2),
        "the lowest address M08 spells names OBJECTIVE2"
    );
    assert_eq!(
        addresses.iter().copied().max(),
        Some(i64::from(BLOCKS)),
        "the highest address M08 spells names the last declared block — the \
         record's own boundary is live and nothing crosses it"
    );

    // **The terminal latches are gated.** Exactly one block spells each
    // outcome. Cross-objective edges are compared on the *spelled* value —
    // the record's own block numbering, the reading under which every edge
    // below lands on the block the mission's structure names. The success
    // latch is woken by exactly one other block, so it cannot fire before
    // that block completes; the failure latch has no wake edge at all and is
    // named only by the six nap sites of the group-depletion announcer blocks — a
    // structure the runtime must observe before any wrong-actor /
    // wrong-session claim is possible (M08-C).
    let number_of = |block: &Block| -> u32 {
        block
            .key
            .trim_start_matches("OBJECTIVE")
            .parse::<u32>()
            .expect("a block key carries its number")
    };
    let win: Vec<&Block> = blocks
        .iter()
        .filter(|block| block.sites.iter().any(|(key, _)| key == "INSTANTWIN"))
        .collect();
    assert_eq!(win.len(), 1, "M08 spells exactly one success latch");
    let latch = win[0];
    let latch_number = number_of(latch);
    assert_eq!(latch_number, 8, "the success latch is OBJECTIVE8");
    let wakeups: Vec<u32> = blocks
        .iter()
        .filter(|block| {
            block.sites.iter().any(|(key, args)| {
                key == "WAKE_OBJECTIVE_WHEN_I_COMPLETE"
                    && integers(args).contains(&i64::from(latch_number))
            })
        })
        .map(number_of)
        .collect();
    assert_eq!(
        wakeups,
        [7],
        "the success latch is woken by exactly one block — OBJECTIVE7, whose \
         completion also advances the stoppoint and arms the hook point — so \
         before that block completes the latch cannot fire"
    );
    let loss: Vec<&Block> = blocks
        .iter()
        .filter(|block| block.sites.iter().any(|(key, _)| key == "INSTANTLOSS"))
        .collect();
    assert_eq!(loss.len(), 1, "M08 spells exactly one failure latch");
    let failure = loss[0];
    let failure_number = number_of(failure);
    assert_eq!(failure_number, 49, "the failure latch is OBJECTIVE49");
    let loss_wakes: Vec<u32> = blocks
        .iter()
        .filter(|block| {
            block.sites.iter().any(|(key, args)| {
                key == "WAKE_OBJECTIVE_WHEN_I_COMPLETE"
                    && integers(args).contains(&i64::from(failure_number))
            })
        })
        .map(number_of)
        .collect();
    assert!(
        loss_wakes.is_empty(),
        "nothing wakes the failure latch directly: {:?}",
        loss_wakes
    );
    let loss_naps: Vec<u32> = blocks
        .iter()
        .filter(|block| {
            block.sites.iter().any(|(key, args)| {
                key == "NAP_OBJECTIVE_WHEN_I_COMPLETE"
                    && integers(args).contains(&i64::from(failure_number))
            })
        })
        .map(number_of)
        .collect();
    assert_eq!(
        loss_naps,
        [28, 29, 30, 31, 32, 33],
        "the six group-depletion announcer blocks OBJECTIVE28…OBJECTIVE33 nap the failure \
         latch (record order 27…32), and the nap re-wakes its target after the \
         spelled seconds"
    );

    // The record the census measured and the record the binding measured are
    // the same measurement, so every pin above holds for both derivations.
    assert_eq!(
        Some(&binding.record),
        census_row().record(),
        "one measurement, two production derivations"
    );
}

/// **The one gap that still keeps M08 from lowering is named, and the
/// campaign gate stays closed.** M08's vocabulary is fully measured, so the
/// refusal is not an unknown directive: eight `DANGER_ZONES_COMPLETED`
/// completion conditions are refused because this build lowers no predicate
/// for the danger-zones flag evaluator (#813). The other half this pin was
/// written against — nineteen `KILL_…` sites running into the registry's
/// per-signature argument bound — was closed on `main` by #800 while this
/// stage was in flight: the adapter now carries an objective-index list as
/// one list argument, so every one of M08's 209 sites binds and the kill
/// key registers. Both halves are asserted here, so a regression in either
/// direction fails this test.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m08_b_the_danger_zones_condition_is_the_gap_that_keeps_m08_unlowered() {
    let binding = control_binding();
    let row = census_row();
    let lowered = row
        .lowering_attempt()
        .expect("the census lowers M08's measured record");
    let attempt = lowered.attempt();
    let record = &binding.record;

    assert!(
        !record.is_complete(attempt),
        "M08's control record does not lower completely"
    );
    assert!(
        !row.is_complete(),
        "the census reports M08's row incomplete"
    );
    let lowering = record.lowering(attempt);
    let unmet: Vec<&str> = lowering.unmet().map(|row| row.kind.code()).collect();
    assert_eq!(
        unmet,
        ["objective_condition", "call_arguments"],
        "the eight danger-zones predicates are unmet twice over: as the \
         condition row itself, and through the `MissionProgram::validate` \
         error the unknown condition raises — no host call is refused, which \
         is what the assertions below prove — unbound={:?} program={}",
        attempt.unbound_keys,
        lowered.program().is_some()
    );

    // The host-call half is closed: no key refuses registration and every
    // site carries a bound verdict — including the 19 kill sites whose lists
    // are the longest the record spells.
    assert!(
        attempt.unbound_keys.is_empty(),
        "no key refuses registration: {:?}",
        attempt.unbound_keys
    );
    let refused: Vec<&str> = attempt
        .calls
        .iter()
        .filter_map(|outcome| match outcome {
            CallOutcome::Refused(reason) => Some(reason.as_str()),
            CallOutcome::Bound => None,
        })
        .collect();
    assert!(
        refused.is_empty(),
        "every site binds, so nothing refuses: {refused:?}"
    );
    assert_eq!(
        attempt.calls.len() as u32,
        record.sites(),
        "every site carries an outcome"
    );
    assert_eq!(
        attempt
            .calls
            .iter()
            .filter(|outcome| **outcome == CallOutcome::Bound)
            .count() as u32,
        record.sites(),
        "all {} sites bind",
        record.sites()
    );
    let kill = record
        .key("KILL_OBJECTIVE_WHEN_I_COMPLETE")
        .expect("M08 spells the kill key");
    assert_eq!(
        kill.sites, KILL_SITES,
        "the kill key carries M08's {KILL_SITES} sites"
    );
    assert!(
        kill.agreed_shape().is_none(),
        "the kill key's sites spell six shapes, so the lowering registers one \
         signature per shape"
    );
    let longest = kill
        .shapes
        .iter()
        .map(|(shape, _)| shape.arity())
        .max()
        .expect("the kill key has shapes");
    assert_eq!(
        longest, 10,
        "M08's longest kill list is the ten-index one OBJECTIVE28…OBJECTIVE33 spell"
    );
    assert!(
        longest > MAX_CALL_ARGS,
        "that length ({longest}) is over MAX_CALL_ARGS ({MAX_CALL_ARGS}), which is \
         exactly why #800's list-argument shaping is what lets these sites bind"
    );
    let spec = lowered
        .registry()
        .get("KILL_OBJECTIVE_WHEN_I_COMPLETE")
        .expect("the kill key registers now that its lists are one argument");
    assert!(
        spec.signatures.iter().all(|signature| signature.len() == 1),
        "every measured kill shape registers as a single-argument signature: {:?}",
        spec.signatures
    );
    assert!(
        lowered.program().is_some(),
        "a MissionProgram assembles from M08's calls and conditions alike"
    );
    let validation = attempt
        .validation
        .clone()
        .expect("the assembled program stood to be validated");
    assert_eq!(
        validation.len(),
        1,
        "validation reports exactly one unsupported instruction: {validation:?}"
    );
    assert!(
        validation[0].contains("DANGER_ZONES_COMPLETED")
            && validation[0].contains("unsupported instruction"),
        "and it is the danger-zones condition of OBJECTIVE17, not a host call: \
         {}",
        validation[0]
    );

    // The gap itself — the danger-zones completion conditions. Eight blocks
    // refuse, every one of them the same measured evaluator, and every other
    // block lowers.
    assert_eq!(
        attempt.conditions.len() as u32,
        record.blocks(),
        "every block carries a condition verdict"
    );
    let condition_refusals: Vec<&str> = attempt
        .conditions
        .iter()
        .filter_map(|outcome| match outcome {
            ConditionOutcome::Refused(field) => Some(field.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        condition_refusals.len(),
        REFUSED_CONDITIONS,
        "exactly eight completion conditions refuse: {condition_refusals:?}"
    );
    assert!(
        condition_refusals
            .iter()
            .all(|field| field.contains("DANGER_ZONES_COMPLETED")
                && field.contains("objective_condition")),
        "every refusal names the danger-zones evaluator and the requirement it \
         leaves unmet"
    );
    for number in 17..=24 {
        assert!(
            condition_refusals
                .iter()
                .any(|field| field.contains(&format!("`OBJECTIVE{number}`"))),
            "OBJECTIVE{number} is one of the eight danger-zones blocks: \
             {condition_refusals:?}"
        );
    }
    let lowered_conditions = attempt
        .conditions
        .iter()
        .filter(|outcome| **outcome == ConditionOutcome::Lowered)
        .count();
    assert_eq!(
        lowered_conditions,
        record.blocks() as usize - REFUSED_CONDITIONS,
        "the other 49 completion conditions lower"
    );

    // The gap keeps M08 out of every readiness claim, even though every host
    // call binds and a program assembles: `MissionProgram::validate` refuses
    // the unknown condition, so the census row stays incomplete.
    let census = census();
    assert!(
        !census.complete_missions().contains(&MISSION),
        "M08 is not one of the census's complete rows"
    );
    assert!(
        !census.campaign_ready(),
        "the campaign gate stays closed while M08 — a bound, fully-measured \
         mission — cannot lower"
    );
    assert!(
        lowering
            .unmeasured_fields()
            .iter()
            .all(|field| !field.is_empty()),
        "every unmet row names what it lacks"
    );
}

// ---------------------------------------------------------------------------
// Retail: where the sheet's three regression priorities live
// ---------------------------------------------------------------------------

/// **The three M08 regression priorities are located in the measured record
/// — as directives, operands and block edges the record spells, not as
/// guessed timings or coordinates — and the half of each that a runtime
/// must observe stays unmeasured.**
///
/// M08-A left all three unbound. What this stage can honestly add is the
/// *data* each one is built from, plus the negative half: the control
/// member's whole 22-key vocabulary carries no interaction, docking,
/// pickup, boarding or transfer directive (pinned by the vocabulary test),
/// so the authorisation half of "protected transfer" is not in this member
/// at all.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m08_b_the_three_sheet_priorities_locate_in_the_measured_record() {
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

    // --- Logistics state: the record's only actor-ownership writes and its
    // world-state evaluators. The team and net assignments are one site each
    // in the same block, six named pairs apiece, so who owns which actor is
    // spelled data — while *whether the wrong actor or a repeated event can
    // satisfy it* is a runtime observation that stays unmeasured.
    assert_eq!(
        binding.record.key("SET_AI_TEAM").map(|key| key.sites),
        Some(1),
        "the team assignment is one site"
    );
    assert_eq!(
        binding.record.key("SET_AI_NET").map(|key| key.sites),
        Some(1),
        "the net assignment is one site"
    );
    assert_eq!(
        sites(3, "SET_AI_TEAM").len(),
        1,
        "both ownership writes are spelled in OBJECTIVE3"
    );
    assert_eq!(
        sites(3, "SET_AI_NET").len(),
        1,
        "…and both are in OBJECTIVE3"
    );
    let teams: Vec<(String, i64)> = sites(3, "SET_AI_TEAM")
        .pop()
        .expect("one team site")
        .iter()
        .filter_map(|pair| match pair {
            ZrdValue::List(pair) => match (&pair[0], &pair[1]) {
                (ZrdValue::Text(name), ZrdValue::Int(team)) => {
                    Some((name.clone(), i64::from(*team)))
                }
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(
        teams,
        [
            ("hafury_1".to_owned(), 2),
            ("hafury_2".to_owned(), 1),
            ("hafury_3".to_owned(), 2),
            ("hafury_4".to_owned(), 2),
            ("hafury_5".to_owned(), 1),
            ("hafury_6".to_owned(), 2),
        ],
        "six actors, each with the team the record spells"
    );
    let nets: Vec<(String, String)> = sites(3, "SET_AI_NET")
        .pop()
        .expect("one net site")
        .iter()
        .filter_map(|pair| match pair {
            ZrdValue::List(pair) => match (&pair[0], &pair[1]) {
                (ZrdValue::Text(name), ZrdValue::Text(net)) => Some((name.clone(), net.clone())),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(
        nets,
        (1..=6)
            .map(|n| (format!("hafury_{n}"), "M3Aces".to_owned()))
            .collect::<Vec<_>>(),
        "the same six actors, each pointed at the one node-list entry"
    );
    let dedg = binding
        .record
        .key("DEDG")
        .expect("M08 spells the group-depletion evaluator");
    assert_eq!(
        dedg.sites, 17,
        "seventeen group-depletion sites carry the mission's enemy state"
    );

    // --- Alternative objective paths: the danger-zone chain and the timed
    // race pair. The chain's zone order is spelled data, and it is *not*
    // the numeric order: dzpath5 comes before dzpath4 and dzpath8 never
    // appears.
    let zones: Vec<String> = (17..=24)
        .map(|number| {
            let args = sites(number, "DANGER_ZONES_COMPLETED")
                .pop()
                .unwrap_or_else(|| panic!("OBJECTIVE{number} reads a danger zone"));
            assert_eq!(texts(&args).len(), 1, "one zone per block");
            texts(&args)[0].to_owned()
        })
        .collect();
    assert_eq!(
        zones,
        [
            "dzpath1", "dzpath2", "dzpath3", "dzpath5", "dzpath4", "dzpath6", "dzpath7", "dzpath9",
        ],
        "the chain M08 spells: seven of the nine dzpath names, 4 and 5 \
         interleaved, 8 absent — a reordering question the record answers but \
         no walkthrough may"
    );
    // The target each block hands to the next: sghangar is superseded by
    // dz2 … dz9, and the last block of the chain retires it.
    let targets: Vec<(String, String)> = (17..=24)
        .map(|number| {
            let removed = sites(number, "REMOVE_OBJECTIVE_TARGET")
                .pop()
                .map(|args| texts(&args)[0].to_owned())
                .unwrap_or_default();
            let added = sites(number, "ADD_OBJECTIVE_TARGET")
                .pop()
                .map(|args| texts(&args)[0].to_owned())
                .unwrap_or_default();
            (removed, added)
        })
        .collect();
    assert_eq!(
        targets,
        [
            ("sghangar".to_owned(), "dz2".to_owned()),
            ("dz2".to_owned(), "dz3".to_owned()),
            ("dz3".to_owned(), "dz5".to_owned()),
            ("dz5".to_owned(), "dz4".to_owned()),
            ("dz4".to_owned(), "dz6".to_owned()),
            ("dz6".to_owned(), "dz7".to_owned()),
            ("dz7".to_owned(), "dz9".to_owned()),
            ("dz9".to_owned(), String::new()),
        ],
        "each block retires the target its predecessor added, in the record's \
         own order"
    );
    // The timed alternative: OBJECTIVE43 wakes itself at 190 seconds and
    // retires the whole chain, while OBJECTIVE42 — woken by the chain's last
    // block — retires OBJECTIVE43. Both are pinned by their own spellings,
    // never by an assumed meaning of the two sound-group names.
    let timer = sites(43, "BEGIN_DORMANT")
        .pop()
        .expect("OBJECTIVE43 starts dormant");
    assert_eq!(
        timer,
        [ZrdValue::Float(190.0)],
        "the alternative branch arms itself at 190 mission-clock seconds"
    );
    assert_eq!(
        sites(43, "REMOVE_OBJECTIVE_TARGET").pop().map(|args| {
            texts(&args)
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        }),
        Some(
            ["sghangar", "dz2", "dz3", "dz4", "dz5", "dz6", "dz7", "dz9"]
                .map(str::to_owned)
                .to_vec()
        ),
        "…and names every target the chain added, retiring the whole path with them"
    );
    assert_eq!(
        integers(
            &sites(43, "KILL_OBJECTIVE_WHEN_I_COMPLETE")
                .pop()
                .expect("the timed branch kills")
        ),
        [17, 18, 19, 20, 21, 22, 23, 24, 42],
        "…including the win branch of the pair"
    );
    assert_eq!(
        integers(
            &sites(42, "KILL_OBJECTIVE_WHEN_I_COMPLETE")
                .pop()
                .expect("the win branch kills")
        ),
        [43],
        "the win branch retires the timed branch, so exactly one of the pair \
         survives each outcome"
    );
    assert_eq!(
        integers(
            &sites(24, "WAKE_OBJECTIVE_WHEN_I_COMPLETE")
                .pop()
                .expect("the last chain block wakes")
        ),
        [42],
        "the chain's last block is what arms the win branch"
    );

    // --- Protected transfer: the stoppoint advance, the hook-point target
    // and the player-to-zeppelin approach the record spells — and nothing
    // that authorises an interaction.
    let stoppoint = sites(7, "COMPLETED_STOPPOINT")
        .pop()
        .expect("OBJECTIVE7 advances a stoppoint");
    assert_eq!(
        stoppoint.len(),
        1,
        "the stoppoint record is one nested list"
    );
    assert_eq!(
        stoppoint,
        [ZrdValue::List(vec![
            ZrdValue::Text("M3Piratezep".to_owned()),
            ZrdValue::Int(1),
            ZrdValue::Int(0),
        ])],
        "the named stoppoint pair OBJECTIVE7 forwards on completion"
    );
    assert_eq!(
        sites(7, "ADD_OBJECTIVE_TARGET").pop().map(|args| {
            texts(&args)
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        }),
        Some(vec!["pzhookpoint".to_owned()]),
        "the hook point becomes an objective target when OBJECTIVE7 completes"
    );
    assert_eq!(
        sites(7, "ADD_OTHER_TARGET").pop().map(|args| args.len()),
        Some(1),
        "the other-target site carries one nested pair record"
    );
    let travelers = binding
        .record
        .key("TRAVELERS")
        .expect("M08 spells the travelers evaluator");
    assert_eq!(
        travelers.sites, 1,
        "one travelers site — the record holds one slot for it"
    );
    assert_eq!(
        sites(48, "TRAVELERS").pop(),
        Some(vec![
            ZrdValue::Text("player".to_owned()),
            ZrdValue::Text("APPROACHING".to_owned()),
            ZrdValue::Text("piratezep".to_owned()),
            ZrdValue::Float(1500.0),
            ZrdValue::Int(1),
        ]),
        "the subject, the polarity token (the only spelling the findings \
         measured), the anchor and the two numbers the record spells"
    );
    // The interaction/authorisation half is absent, by the pinned vocabulary:
    // no key names a docking, pickup, boarding, transfer or authorisation.
    for key in binding.record.keys() {
        let lowered = key.key.to_lowercase();
        for word in [
            "dock", "pickup", "board", "transfer", "authoriz", "interact",
        ] {
            assert!(
                !lowered.contains(word),
                "the vocabulary carries an interaction directive nobody measured: \
                 {}",
                key.key
            );
        }
    }

    // Whether the wrong actor, the wrong session or a repeated event can
    // satisfy any of these transitions is runtime behaviour: nothing here
    // simulates it, and M08 has no runtime consumer yet.
    assert!(
        !census().complete_missions().contains(&MISSION),
        "no runtime may observe M08's transitions while the record cannot lower"
    );
}

// ---------------------------------------------------------------------------
// Synthetic: the refusal arms the retail installation reaches only through
// M08's own keys
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
        ContentId::from_source(ContentKind::Mission, "accept-m08-b")
            .map_err(|error| error.to_string()),
        "accept-m08-b",
        document,
        &record,
    )
}

/// **M08's six kill shapes each arrive as one list argument and bind, and an
/// index list wider than a value can hold refuses.** The retail test
/// observes that result on the installation; this carries the mechanism into
/// CI: an authored record spelling the shapes M08 spells (1, 5, 6, 7, 9 and
/// 10 indices) registers the key, lowers every site and assembles its
/// program, while a list past `MAX_VALUE_ITEMS` refuses the record and
/// nothing truncates. This is the arm #800's list-argument shaping left
/// standing, pinned from M08's side.
#[test]
fn accept_m08_b_m08s_kill_shapes_bind_as_one_list_argument_and_an_over_wide_one_refuses() {
    let authored = |count: u32| {
        let args: Vec<ZrdValue> = (1..=count).map(zrd_int).collect();
        control_record(vec![block(
            1,
            vec![
                directive("BEGIN_DORMANT", vec![zrd_float(-1.0)]),
                directive("KILL_OBJECTIVE_WHEN_I_COMPLETE", args),
            ],
        )])
    };

    for count in [1u32, 5, 6, 7, 9, 10] {
        let document = authored(count);
        let record = measure_control_record(&document);
        let lowered = lower(&document);
        assert!(
            record.is_complete(lowered.attempt()),
            "M08's {count}-index kill shape binds and lowers: {:?}",
            lowered.attempt().unbound_keys
        );
        let raw = lowered.raw_program().expect("the program assembled");
        assert_eq!(
            raw.objectives[0].calls[1].args,
            [Value::List(
                (1..=count).map(|n| Value::Int(n as i32)).collect()
            )],
            "the {count} indices arrive as one list, in order"
        );
        let spec = lowered
            .registry()
            .get("KILL_OBJECTIVE_WHEN_I_COMPLETE")
            .expect("the kill key registers");
        assert_eq!(
            spec.signatures.len(),
            1,
            "one measured shape in this record"
        );
        assert_eq!(
            spec.signatures[0].len(),
            1,
            "and it is a single-argument (one list) signature"
        );
    }

    // A list wider than a value can be refuses; nothing truncates.
    let document = authored(cs_script::ir::MAX_VALUE_ITEMS as u32 + 1);
    let record = measure_control_record(&document);
    let lowered = lower(&document);
    assert!(
        !record.is_complete(lowered.attempt()),
        "an over-wide index list refuses its record"
    );
    assert!(
        lowered.raw_program().is_some_and(|raw| {
            raw.objectives[0]
                .calls
                .iter()
                .all(|call| call.name != "KILL_OBJECTIVE_WHEN_I_COMPLETE")
        }),
        "the refused site is dropped by name rather than clamped"
    );
}

/// **The danger-zones completion condition refuses while a measured
/// condition lowers** — M08's second gap on authored records, so CI covers
/// both arms without original data. The block with the unlowerable
/// evaluator is reported under `objective_condition`; the same record with
/// the group-depletion evaluator in its place lowers, so the refusal is the
/// evaluator and not the shape of the authored record.
#[test]
fn accept_m08_b_the_danger_zones_condition_refuses_while_a_measured_condition_lowers() {
    let authored = |key: &str, args: Vec<ZrdValue>| {
        control_record(vec![block(
            1,
            vec![
                directive("BEGIN_DORMANT", vec![zrd_float(-1.0)]),
                directive(key, args),
                directive("INSTANTWIN", vec![]),
            ],
        )])
    };

    let document = authored("DANGER_ZONES_COMPLETED", vec![zrd_text("dzpath1")]);
    let record = measure_control_record(&document);
    let lowered = lower(&document);
    let attempt = lowered.attempt();
    let conditions: Vec<&str> = attempt
        .conditions
        .iter()
        .filter_map(|outcome| match outcome {
            ConditionOutcome::Refused(field) => Some(field.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        conditions.len(),
        1,
        "the authored block's condition refuses: {:?}",
        attempt.conditions
    );
    assert!(
        conditions[0].contains("DANGER_ZONES_COMPLETED")
            && conditions[0].contains("objective_condition"),
        "the refusal names the evaluator and the requirement: {}",
        conditions[0]
    );
    assert!(
        conditions[0].contains("lowers no condition for it"),
        "the refusal states why this build offers no predicate: {}",
        conditions[0]
    );
    let lowering = record.lowering(attempt);
    let unmet: Vec<&str> = lowering.unmet().map(|row| row.kind.code()).collect();
    assert_eq!(
        unmet,
        ["objective_condition", "call_arguments"],
        "the refused condition is the only thing the authored record lacks, and \
         the accounting names it under both rows the refusal reaches: the \
         predicate itself and the validation the unknown condition keeps from \
         passing — unbound={:?} validation={:?} calls={:?}",
        attempt.unbound_keys,
        attempt.validation,
        attempt.calls
    );
    assert_eq!(
        attempt
            .calls
            .iter()
            .filter(|outcome| **outcome == CallOutcome::Bound)
            .count(),
        attempt.calls.len(),
        "every authored site binds: the gap is the predicate, not the call"
    );

    // The control arm: the group-depletion evaluator is lowered by this
    // build, so the same authored record completes.
    let document = authored("DEDG", vec![zrd_int(3), zrd_int(0)]);
    let record = measure_control_record(&document);
    let lowered = lower(&document);
    assert!(
        lowered
            .attempt()
            .conditions
            .iter()
            .all(|outcome| *outcome == ConditionOutcome::Lowered),
        "the measured group-depletion condition lowers: {:?}",
        lowered.attempt().conditions
    );
    assert!(
        record.is_complete(lowered.attempt()),
        "the same authored record with a lowered condition completes: {:?}",
        lowered.attempt().unbound_keys
    );
}
