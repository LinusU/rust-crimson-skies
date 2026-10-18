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
//! `ZBD/C1B/M03/zrdr.zbd`, and this stage binds it through the production
//! systems M02-B built (`SourceContext::control_program`) and measures it a
//! second time through `cs_app::mission_control`, so three independent
//! derivations — the mission binding, the control binding and the retail
//! census — must agree before any assertion below can pass.
//!
//! The stage's minimum acceptance scenario is *"All discovered
//! mission-specific behavior uses production engine systems and regression
//! tests."* M08's discovered mission-specific behavior is its control
//! record: 55 numbered blocks, 313 directive sites, 40 distinct keys, the
//! block graph those spell, and the one spelling the original ignores.
//!
//! * **The lowering is pinned as it stands.** The record spells
//!   `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` twice, a spelling the original's
//!   directive table does not hold (M03-B-FU1 / #803), so those two sites are
//!   dropped and the other 311 bind — including the 15 `KILL_…` sites, whose
//!   index lists the adapter carries as one list argument (#800) — and all 55
//!   completion conditions lower: the record lowers completely, the census row
//!   is complete, and the campaign gate stays closed on the rows whose gaps
//!   remain. The synthetic danger-zones test keeps the predicate arm
//!   (M07-B-FU1, #813) that this record does not reach.
//! * **The three sheet priorities are located in the measured record** as
//!   directives, operands and block edges the record spells — never as a
//!   timing, count or coordinate it does not. Their predicates (the wrong
//!   actor, the wrong session, a repeated event) are runtime observations
//!   and stay unmeasured: lowering says nothing about them and ordinary-play
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
    DirectiveDisposition, TerminalOutcome, UnmeasuredReason, measure_control_record,
    objective_blocks_of, terminal_outcome_of,
};
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{ZrdValue, decode_zrd, objective_record, zrd_flat_fields};
use cs_formats::script_raw::discover_container;
use cs_script::bindings::MAX_CALL_ARGS;
use cs_script::ir::Value;
use cs_types::content::{ContentId, ContentKind};
use cs_types::install::RelativePath;

use crate::campaign::common::{label, load_inventory};

/// The census row label of the mission: the mission-scoped reader archive
/// F13-B's rule derives from the installation.
const MISSION: &str = "zbd/c1b/m03";

/// The numbered blocks of the control member.
const BLOCKS: u32 = 55;
/// The directive sites of the control member.
const SITES: u32 = 313;
/// The distinct directive keys of the control member.
const KEYS: usize = 40;
/// The sites of the kill key, every one of which binds now that the adapter
/// carries an objective-index list as one list argument (#800).
const KILL_SITES: u32 = 15;

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
        "script/c1b-m03-zrdr",
        "the program identity is the world group's reader archive for world c1b, mission 3"
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
        22,
        "M08's archive offers twenty-two members"
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
/// unknown directive: every key M08 spells carries a measured disposition,
/// is one of the two terminal outcome spellings, or is the one spelling the
/// original's directive table does not hold (`WAKEUP_OBJECTIVE_WHEN_I_COMPLETE`,
/// M03-B-FU1 / #803); no block is unreadable, and no record-level key falls
/// outside the measured record vocabulary.
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
        ["WAKEUP_OBJECTIVE_WHEN_I_COMPLETE"],
        "the one key without a measured effect is the `WAKEUP_` spelling the \
         original never looks up (finding 2026-10-18-m03-b-fu1-wakeup-objective-spelling)"
    );
    assert!(
        matches!(
            record
                .key("WAKEUP_OBJECTIVE_WHEN_I_COMPLETE")
                .expect("M08 spells the ignored wake key")
                .disposition(),
            DirectiveDisposition::Unmeasured {
                reason: UnmeasuredReason::IgnoredByOriginal
            }
        ),
        "that key is unmeasured because the original ignores it"
    );
    assert_eq!(
        record.measured().len() + implemented.len() + binding.unmeasured_keys().len(),
        record.vocabulary() as usize,
        "measured + implemented + the ignored spelling partitions M08's vocabulary"
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
            "DEDG",
            "IDENTITY",
            "INACTIVE1",
            "INACTIVE10",
            "INACTIVE11",
            "INACTIVE12",
            "INACTIVE13",
            "INACTIVE14",
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
            "SET_AI_NET",
            "SET_HELP_LABEL",
            "STOP_QUEUED_SOUNDS",
            "TICK_DEPENDS_ON_OBJ",
            "WAKEUP_ENEMIES",
            "WAKEUP_GENERATOR",
            "WAKEUP_OBJECTIVE_WHEN_I_COMPLETE",
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
/// The record declares `OBJECTIVE1` … `OBJECTIVE55`; the three
/// cross-objective keys M08 spells carry 79 addresses, none zero, none
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
    // `SLEEP_…`, no bare `WAKE_OBJECTIVE` and no `HIDE_OBJ`; its two
    // `TICK_DEPENDS_ON_OBJ` sites address another block too, but are not an
    // edge of the completion graph), and every integer their sites address.
    let directed = [
        "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
        "KILL_OBJECTIVE_WHEN_I_COMPLETE",
        "NAP_OBJECTIVE_WHEN_I_COMPLETE",
    ];
    for absent in [
        "SLEEP_OBJECTIVE_WHEN_I_COMPLETE",
        "WAKE_OBJECTIVE",
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
        79,
        "M08 spells 79 cross-objective addresses"
    );

    // Every address names a block this record declares: none is zero or
    // negative, none is past the last block, so `address - 1` — the index
    // the parse stores — always lands inside the record's own 55 blocks.
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
    // below lands on the block the mission's structure names. Neither latch
    // has a wake edge: each is named only by nap sites, which re-wake their
    // target after the spelled seconds — a structure the runtime must
    // observe before any wrong-actor / wrong-session claim is possible
    // (M08-C).
    let number_of = |block: &Block| -> u32 {
        block
            .key
            .trim_start_matches("OBJECTIVE")
            .parse::<u32>()
            .expect("a block key carries its number")
    };
    let naming = |key: &str, target: u32| -> Vec<u32> {
        blocks
            .iter()
            .filter(|block| {
                block
                    .sites
                    .iter()
                    .any(|(name, args)| name == key && integers(args).contains(&i64::from(target)))
            })
            .map(number_of)
            .collect()
    };
    let win: Vec<&Block> = blocks
        .iter()
        .filter(|block| block.sites.iter().any(|(key, _)| key == "INSTANTWIN"))
        .collect();
    assert_eq!(win.len(), 1, "M08 spells exactly one success latch");
    let latch_number = number_of(win[0]);
    assert_eq!(latch_number, 33, "the success latch is OBJECTIVE33");
    assert!(
        naming("WAKE_OBJECTIVE_WHEN_I_COMPLETE", latch_number).is_empty(),
        "nothing wakes the success latch directly"
    );
    assert_eq!(
        naming("NAP_OBJECTIVE_WHEN_I_COMPLETE", latch_number),
        [32],
        "the success latch is named by exactly one block — OBJECTIVE32's \
         `NAP_OBJECTIVE_WHEN_I_COMPLETE [33]` — so before that block completes \
         the latch cannot fire"
    );
    let loss: Vec<&Block> = blocks
        .iter()
        .filter(|block| block.sites.iter().any(|(key, _)| key == "INSTANTLOSS"))
        .collect();
    assert_eq!(loss.len(), 1, "M08 spells exactly one failure latch");
    let failure_number = number_of(loss[0]);
    assert_eq!(failure_number, 7, "the failure latch is OBJECTIVE7");
    assert!(
        naming("WAKE_OBJECTIVE_WHEN_I_COMPLETE", failure_number).is_empty(),
        "nothing wakes the failure latch directly"
    );
    assert_eq!(
        naming("NAP_OBJECTIVE_WHEN_I_COMPLETE", failure_number),
        [6, 22, 23],
        "OBJECTIVE6, OBJECTIVE22 and OBJECTIVE23 nap the failure latch, and \
         the nap re-wakes its target after the spelled seconds"
    );

    // The record the census measured and the record the binding measured are
    // the same measurement, so every pin above holds for both derivations.
    assert_eq!(
        Some(&binding.record),
        census_row().record(),
        "one measurement, two production derivations"
    );
}

/// **M08's record lowers completely, and the campaign gate stays closed on the
/// rows whose gaps remain.** The two `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` sites
/// are dropped as the original drops them (M03-B-FU1 / #803), so 311 of the
/// 313 sites carry a call and bind; the 15 `KILL_…` sites carry their
/// objective-index lists as one list argument (#800); and all 55 completion
/// conditions lower. Nothing refuses.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m08_b_m08s_record_lowers_completely_and_the_gate_stays_closed_on_other_rows() {
    let binding = control_binding();
    let row = census_row();
    let lowered = row
        .lowering_attempt()
        .expect("the census lowers M08's measured record");
    let attempt = lowered.attempt();
    let record = &binding.record;

    assert!(
        record.is_complete(attempt),
        "M08's control record lowers completely"
    );
    assert!(row.is_complete(), "the census reports M08's row complete");
    let lowering = record.lowering(attempt);
    let unmet: Vec<&str> = lowering.unmet().map(|row| row.kind.code()).collect();
    assert!(
        unmet.is_empty(),
        "neither requirement is unmet any more: {unmet:?} (unbound={:?} program={})",
        attempt.unbound_keys,
        lowered.program().is_some()
    );

    // The host-call half is closed: no key refuses registration and every
    // carried site has a bound verdict; the two ignored sites carry no call.
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
        "every carried site binds, so nothing refuses: {refused:?}"
    );
    assert_eq!(
        attempt.ignored_sites, 2,
        "the two `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` sites are the original's ignored spelling"
    );
    assert_eq!(
        attempt.calls.len() as u32,
        record.sites() - attempt.ignored_sites,
        "every site but the ignored ones carries an outcome"
    );
    assert!(
        attempt
            .calls
            .iter()
            .all(|outcome| *outcome == CallOutcome::Bound),
        "all {} carried sites bind",
        attempt.calls.len()
    );
    assert!(
        lowered
            .raw_program()
            .expect("a raw program stood")
            .objectives
            .iter()
            .flat_map(|objective| objective.calls.iter())
            .all(|call| call.name != "WAKEUP_OBJECTIVE_WHEN_I_COMPLETE"),
        "no call is emitted for the ignored spelling"
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
        "the kill key's sites spell four shapes, so the lowering registers one \
         signature per shape"
    );
    let longest = kill
        .shapes
        .iter()
        .map(|(shape, _)| shape.arity())
        .max()
        .expect("the kill key has shapes");
    assert_eq!(
        longest, 5,
        "M08's longest kill list is the five-index one OBJECTIVE27 and OBJECTIVE30 spell"
    );
    assert!(
        longest <= MAX_CALL_ARGS,
        "this record's kill lists ({longest}) are within MAX_CALL_ARGS \
         ({MAX_CALL_ARGS}); the over-wide case is pinned by the synthetic kill-shape test"
    );
    let spec = lowered
        .registry()
        .get("KILL_OBJECTIVE_WHEN_I_COMPLETE")
        .expect("the kill key registers as list-argument signatures");
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
    assert!(
        validation.is_empty(),
        "validation accepts every lowered condition: {validation:?}"
    );

    // Every block's completion condition lowers.
    assert_eq!(
        attempt.conditions.len() as u32,
        record.blocks(),
        "every block carries a condition verdict"
    );
    assert!(
        attempt
            .conditions
            .iter()
            .all(|outcome| *outcome == ConditionOutcome::Lowered),
        "every completion condition lowers: {:?}",
        attempt.conditions
    );

    // A complete row is a lowering claim, not a launchable mission: the gate
    // still needs every measured row complete, and rows whose gaps remain keep
    // it closed.
    let census = census();
    assert!(
        census.complete_missions().contains(&MISSION),
        "M08 is one of the census's complete rows"
    );
    assert!(
        !census.campaign_ready(),
        "the campaign gate stays closed while any measured row carries a gap"
    );
    assert!(
        census
            .measured_rows()
            .filter(|row| row.mission() != MISSION)
            .any(|row| !row.is_complete()),
        "some other measured row is still incomplete, which is what keeps the gate closed"
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
/// member's whole 40-key vocabulary carries no interaction, docking,
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

    // --- Logistics state: the record's only actor-ownership write and its
    // world-state evaluators. The record spells no `SET_AI_TEAM`; the net
    // assignment is one site in OBJECTIVE10 with ten named pairs, so who is
    // pointed at which node list is spelled data — while *whether the wrong
    // actor or a repeated event can satisfy it* is a runtime observation
    // that stays unmeasured.
    assert!(
        binding.record.key("SET_AI_TEAM").is_none(),
        "M08's record spells no team assignment"
    );
    assert_eq!(
        binding.record.key("SET_AI_NET").map(|key| key.sites),
        Some(1),
        "the net assignment is one site"
    );
    assert_eq!(
        sites(10, "SET_AI_NET").len(),
        1,
        "…and it is in OBJECTIVE10"
    );
    let nets: Vec<(String, String)> = sites(10, "SET_AI_NET")
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
        [
            ("devastator_2", "Bravo2"),
            ("devastator_3", "Charlie2"),
            ("blakepeace_2_1", "Boeing2"),
            ("blakepeace_2_2", "Boeing2"),
            ("blakepeace_2_3", "Boeing2"),
            ("blakepeace_2_4", "Boeing2"),
            ("patrolboat_1", "Patrolboat1"),
            ("patrolboat_2", "Patrolboat2"),
            ("patrolboat_3", "Patrolboat3"),
            ("patrolboat_4", "Patrolboat4"),
        ]
        .map(|(name, net)| (name.to_owned(), net.to_owned())),
        "ten actors, each pointed at the node list the record spells"
    );
    let dedg = binding
        .record
        .key("DEDG")
        .expect("M08 spells the group-depletion evaluator");
    assert_eq!(dedg.sites, 4, "four group-depletion sites");
    let generators: Vec<(u32, Vec<ZrdValue>)> = [12, 19, 26, 38]
        .into_iter()
        .map(|number| {
            (
                number,
                sites(number, "WAKEUP_GENERATOR")
                    .pop()
                    .unwrap_or_else(|| panic!("OBJECTIVE{number} wakes a generator")),
            )
        })
        .collect();
    assert_eq!(
        generators,
        [
            (
                12,
                vec![ZrdValue::Text("vostokzep".to_owned()), ZrdValue::Int(2)]
            ),
            (
                19,
                vec![ZrdValue::Text("vostokzep".to_owned()), ZrdValue::Int(3)]
            ),
            (
                26,
                vec![ZrdValue::Text("vostokzep".to_owned()), ZrdValue::Int(3)]
            ),
            (
                38,
                vec![ZrdValue::Text("vostokzep".to_owned()), ZrdValue::Int(2)]
            ),
        ],
        "the four generator wake-ups the record spells, each naming the zeppelin and a number"
    );

    // --- Alternative objective paths: the timed blocks and the mutually
    // exclusive trio. Every number is the record's own spelling, never an
    // assumed meaning of the sound-group names.
    let timers: Vec<(u32, Vec<ZrdValue>)> = [1, 6, 8, 9, 10, 13]
        .into_iter()
        .map(|number| {
            (
                number,
                sites(number, "BEGIN_DORMANT")
                    .pop()
                    .unwrap_or_else(|| panic!("OBJECTIVE{number} starts dormant")),
            )
        })
        .collect();
    assert_eq!(
        timers,
        [
            (1, vec![ZrdValue::Float(2.0)]),
            (6, vec![ZrdValue::Float(180.0)]),
            (8, vec![ZrdValue::Float(181.0)]),
            (9, vec![ZrdValue::Float(215.0)]),
            (10, vec![ZrdValue::Float(230.0)]),
            (13, vec![ZrdValue::Float(260.0)]),
        ],
        "the six blocks the record arms at a spelled mission-clock second"
    );
    for (number, others) in [(36, [37, 47]), (37, [36, 47]), (47, [37, 36])] {
        assert_eq!(
            integers(
                &sites(number, "KILL_OBJECTIVE_WHEN_I_COMPLETE")
                    .pop()
                    .unwrap_or_else(|| panic!("OBJECTIVE{number} kills its siblings"))
            ),
            others,
            "OBJECTIVE{number} retires the other two blocks of the 36/37/47 trio"
        );
        assert_eq!(
            integers(
                &sites(number, "WAKE_OBJECTIVE_WHEN_I_COMPLETE")
                    .pop()
                    .unwrap_or_else(|| panic!("OBJECTIVE{number} wakes the join"))
            ),
            [38],
            "whichever of the trio completes first wakes OBJECTIVE38"
        );
    }

    // --- Protected transfer: the stoppoint advances, the hook-point target
    // and the zeppelin wake-ups the record spells — and nothing that
    // authorises an interaction.
    assert_eq!(
        sites(10, "COMPLETED_STOPPOINT").pop(),
        Some(vec![
            ZrdValue::List(vec![
                ZrdValue::Text("Vostok1".to_owned()),
                ZrdValue::Int(1),
                ZrdValue::Int(0),
            ]),
            ZrdValue::List(vec![
                ZrdValue::Text("Klondike1".to_owned()),
                ZrdValue::Int(6),
                ZrdValue::Int(0),
            ]),
        ]),
        "the two named stoppoint pairs OBJECTIVE10 forwards on completion"
    );
    assert_eq!(
        sites(32, "ADD_OBJECTIVE_TARGET").pop().map(|args| {
            texts(&args)
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        }),
        Some(vec!["pzhookpoint".to_owned()]),
        "the hook point becomes an objective target when OBJECTIVE32 completes"
    );
    assert_eq!(
        texts(
            &sites(1, "WAKEUP_ZEP_TURRETS")
                .pop()
                .expect("OBJECTIVE1 wakes turrets")
        ),
        ["piratezep"],
        "OBJECTIVE1 wakes the named zeppelin's turrets at start"
    );
    assert!(
        binding.record.key("TRAVELERS").is_none()
            && binding.record.key("ADD_OTHER_TARGET").is_some(),
        "M08 spells no travelers evaluator; its other-target writes are the \
         only transfer-adjacent target change"
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
    // simulates it, and a complete census row says nothing about it — it is
    // a lowering claim. The runtime halves stay open for M08-C, and the
    // campaign gate still keeps every runtime out while any measured row
    // carries a gap.
    assert!(
        !census().campaign_ready(),
        "M08's transitions stay unobserved: no campaign runtime may start while the \
         campaign gate is closed"
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

/// **A danger-zones completion condition lowers the measured predicate while
/// a site the parse cannot read still refuses** — the danger-zones surface the
/// stage first measured, kept on authored records (the corrected record
/// spells no such key), so CI covers both arms without original data. The
/// measured shape (zone names beside the key) lowers and completes the
/// authored record; the same key spelled with no list after it — the record
/// the original's own lookup cannot read as a zone vector — is reported under
/// `objective_condition`, and the same record with the group-depletion
/// evaluator in its place lowers, so a refusal is the site's spelling and not
/// the shape of the authored record.
#[test]
fn accept_m08_b_a_danger_zones_condition_lowers_and_an_unreadable_one_refuses() {
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

    // The measured shape: the zone-name list, and the count that defaults to
    // its length when the block spells no `DANGER_ZONES_COMPLETION_COUNT`.
    let document = authored("DANGER_ZONES_COMPLETED", vec![zrd_text("dzpath1")]);
    let record = measure_control_record(&document);
    let lowered = lower(&document);
    let attempt = lowered.attempt();
    assert_eq!(
        attempt.conditions,
        [ConditionOutcome::Lowered],
        "the measured danger-zones site lowers: {:?}",
        attempt.conditions
    );
    let lowering = record.lowering(attempt);
    let unmet: Vec<&str> = lowering.unmet().map(|row| row.kind.code()).collect();
    assert!(
        unmet.is_empty(),
        "the authored record now lacks nothing: {unmet:?} (unbound={:?} validation={:?})",
        attempt.unbound_keys,
        attempt.validation
    );
    assert!(
        record.is_complete(attempt),
        "the same authored record with a lowered condition completes: {:?}",
        attempt.unbound_keys
    );
    assert_eq!(
        attempt
            .calls
            .iter()
            .filter(|outcome| **outcome == CallOutcome::Bound)
            .count(),
        attempt.calls.len(),
        "every authored site binds"
    );
    // The predicate itself: the zone name as spelled, threshold 1 of 1.
    let program = lowered.program().expect("the authored record assembles");
    let mut carried = Vec::new();
    fn find(condition: &cs_script::ir::Condition, zones: &mut Vec<(u32, Vec<String>)>) {
        match condition {
            cs_script::ir::Condition::DangerZoneFlags {
                zones: names,
                required,
            } => {
                zones.push((*required, names.clone()));
            }
            cs_script::ir::Condition::All(items) | cs_script::ir::Condition::Any(items) => {
                for item in items {
                    find(item, zones);
                }
            }
            _ => {}
        }
    }
    find(&program.objectives[0].condition, &mut carried);
    assert_eq!(
        carried,
        [(1, vec!["dzpath1".to_owned()])],
        "the predicate carries the listed zone and the threshold its count defaults to"
    );

    // The unreadable site: the key stands with no record after it, so the
    // original's lookup would take whatever follows as the zone vector.
    let document = control_record(vec![block(
        1,
        vec![
            directive("BEGIN_DORMANT", vec![zrd_float(-1.0)]),
            vec![zrd_text("DANGER_ZONES_COMPLETED")],
            directive("INSTANTWIN", vec![]),
        ],
    )]);
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
        conditions[0].contains("would be a guess at its predicate"),
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
        "every authored site binds: the gap is the site's spelling, not the call"
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
