//! Acceptance stage M02-B: M02's mission-specific compatibility surface — the
//! mission control program the installation actually ships for *The Bomber
//! Heist*, bound through production engine systems and regressed against the
//! lowering that decides what the engine may honour
//! (`missions/M02.md`, work order `M02-B`, Rally #262).
//!
//! Shared contract: `docs/contracts/SCRIPT-MISSION.md` ("Source adapter
//! acceptance", "Host interface", "Objective event ordering"). Findings:
//! `docs/findings/2026-10-08-m02-b-compatibility-gaps.md`.
//!
//! # What this stage adds, and what it deliberately does not
//!
//! M02-A bound *which* retail mission the work order names; it bound no
//! objective, actor or directive. The installation ships M02's mission
//! control program as a typed keyed list inside the mission's reader
//! archive, and `SourceContext::control_program` (this stage's production
//! change, in `cs_content::campaign_bindings`) binds it to the same
//! identities the mission binding resolves: the archive the campaign layout
//! declares, every member it holds, the member the **measured rule** picks
//! (the one whose decoded record declares numbered `OBJECTIVE<N>` blocks —
//! never a filename constant) and every directive that member spells,
//! measured through `cs_content::mission_control`.
//!
//! The retail tests then hold three independent production derivations to
//! each other — the campaign binding's identities, this new control binding
//! and the `cs_app::mission_control` census — so they cannot disagree about
//! the mission without a failing test. The M02 sheet's regression
//! priorities (capture versus destruction, player-aircraft transfer,
//! remaining-target failure) are located in the measured record as the
//! directives and block graph that will carry them; their *predicates* stay
//! unmeasured until a runtime observes them, and nothing here assigns a
//! timing, count or coordinate the record does not spell.
//!
//! One compatibility gap is measured and pinned, not worked around: M02's
//! `KILL_OBJECTIVE_WHEN_I_COMPLETE` sites spell argument lists up to nine
//! integers, and the host-binding registry's per-signature bound
//! (`cs_script::bindings::MAX_CALL_ARGS`) refuses a signature longer than
//! eight — so the key cannot register, all eight of its sites refuse, and
//! M02's control record does **not** lower completely. The engine's honest
//! answer stays "Unsupported", the campaign gate stays closed, and the
//! follow-up that changes the bound owns this pin. Nothing here lowers an
//! unmeasured or refused directive to a no-op (AGENTS.md rule 4).
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`, so CI skips
//! them and the implementing and reviewing agents run them with
//! `--include-ignored`. The synthetic tests build `.zrd` values tag by tag
//! — no original game data is committed — and run in CI.

use std::path::PathBuf;
use std::sync::OnceLock;

use cs_app::control_lowering::lower_control_record;
use cs_app::mission_control::survey_mission_control_programs;
use cs_assets::install::sha256;
use cs_content::campaign_bindings::{MissionLabel, SourceContext};
use cs_content::mission_control::{
    AnimList, CONTROL_RECORD_KEY_VOCABULARY, CallOutcome, ConditionOutcome, ControlMemberError,
    ControlRecordField, DecodedMember, DirectiveDisposition, control_member,
    measure_control_record, objective_blocks_of, terminal_outcome_of,
};
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{ZrdValue, decode_zrd, objective_record, zrd_flat_fields};
use cs_formats::script_raw::discover_container;
use cs_script::bindings::MAX_CALL_ARGS;
use cs_types::content::{ContentId, ContentKind};
use cs_types::install::RelativePath;

use crate::common::{label, load_inventory};

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M02-B needs the retail capability; run this suite with \
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

/// M02's work-order label, from the committed inventory rather than a
/// literal.
fn m02() -> MissionLabel {
    label("M02")
}

/// M02's declared discovery title, from the committed inventory.
fn m02_title() -> String {
    load_inventory()
        .iter()
        .find(|(work_order, _)| work_order.as_str() == "M02")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M02 work order")
}

/// M02's control binding, derived fresh through production code.
fn control_binding() -> cs_content::campaign_bindings::MissionControlBinding {
    context()
        .control_program(m02(), &m02_title())
        .expect("M02's control program binds through the measured rule")
}

/// M02's row in the retail control census: the same installation measured a
/// second time through `cs_app::mission_control`.
fn census_row() -> &'static cs_app::mission_control::RetailControlRow {
    static ROW: OnceLock<cs_app::mission_control::RetailControlRow> = OnceLock::new();
    ROW.get_or_init(|| {
        let census = survey_mission_control_programs(&game_dir())
            .expect("the installation measures a control census");
        census
            .row("zbd/c1/m02")
            .expect("M02's reader archive is measured by the census")
            .clone()
    })
}

/// The control member's decoded document, re-read from the archive through
/// production discovery — an independent walk from the binding's, so the
/// graph assertions below cannot be satisfied by the binding's own output.
fn control_document() -> (ZrdValue, Vec<(String, u64, u64, u32)>) {
    let binding = control_binding();
    let bytes = std::fs::read(game_dir().join(&binding.program_asset))
        .expect("M02's reader archive reads from disk");
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

/// One numbered block of a decoded control record, as the graph walk below
/// reads it: its key, its record order (the zero-based index cross-objective
/// directives address) and its directive sites in spelling order.
struct Block {
    key: String,
    index: u32,
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
        let index = blocks.len() as u32;
        let mut sites = Vec::new();
        let Some(children) = value.as_list() else {
            blocks.push(Block {
                key: format!("OBJECTIVE{number}"),
                index,
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
            index,
            sites,
        });
    }
    blocks
}

// ---------------------------------------------------------------------------
// Retail: the binding ties M02's control program to M02's identities
// ---------------------------------------------------------------------------

/// **The control program binds to the same mission the mission binding
/// names.** `SourceContext::control_program` resolves the work order through
/// the same title join `SourceContext::bind` uses, so the two derivations
/// name one mission, one program and one archive — and the member the
/// binding cites is the member the measured rule picks over the archive's
/// whole member set, with a digest over that member's own bytes.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m02_b_m02s_control_program_is_bound_to_the_same_identities_as_its_mission_binding() {
    let binding = control_binding();
    let mission_binding = context()
        .bind(m02(), &m02_title())
        .expect("M02's mission binding resolves");

    // One mission, one program: the control binding cites exactly the
    // identities the mission binding resolved.
    assert_eq!(
        binding.mission,
        mission_binding
            .catalog_id
            .expect("the mission binding resolves a mission id"),
        "the control binding and the mission binding name the same mission"
    );
    assert_eq!(
        binding.mission.as_str(),
        "mission/ch1-m02",
        "the join selected campaign position {}, whose directory is chapter 1, mission 2",
        mission_binding
            .campaign_position
            .expect("a position was resolved")
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
        "script/c1-m02-zrdr",
        "the program identity is the world group's reader archive for mission 2"
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
        .expect("M02's reader archive reads from disk");
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

    // The census measured the same archive through its own discovery path:
    // same container, same member, same record. The census's container label
    // keeps the on-disk spelling's case while this binding's logical key is
    // lowercased for the mission-scope rule, so the comparison is
    // case-insensitive on both sides.
    let census_row = census_row();
    assert_eq!(
        binding.program_asset.to_lowercase(),
        census_row.container.to_lowercase(),
        "the control binding and the census read the same reader archive"
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
        binding.members.len() as u64,
        census_row.members.len() as u64,
        "both derivations account for every member the archive offers"
    );
}

/// **M02's directive vocabulary is fully measured: every key carries a
/// measured effect or is one of the two terminal outcome spellings, the
/// accounting partitions exactly, and nothing is refused.** This is what
/// makes M02's gap (pinned below) a *host-call bound* gap and not an
/// unmeasured directive.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m02_b_the_measured_vocabulary_partitions_and_refuses_no_m02_key() {
    let binding = control_binding();
    let record = &binding.record;

    assert_eq!(record.blocks(), 50, "M02 declares 50 numbered blocks");
    assert_eq!(record.sites(), 190, "M02 spells 190 directive sites");
    assert_eq!(
        record.keys().iter().map(|key| key.sites).sum::<u32>(),
        record.sites(),
        "every measured site belongs to exactly one key"
    );
    assert_eq!(
        record.vocabulary() as usize,
        record.keys().len(),
        "the vocabulary count is the key list's length"
    );
    assert_eq!(
        record.vocabulary(),
        36,
        "M02 spells 36 distinct directive keys"
    );
    assert!(
        record.refusals().is_empty(),
        "the measured directive grammar parses every M02 block: {:?}",
        record.refusals()
    );

    // The partition: two implemented outcome keys, the rest measured, none
    // unmeasured.
    let implemented: Vec<(&str, _)> = record
        .implemented()
        .into_iter()
        .map(|(key, outcome)| (key.key.as_str(), outcome))
        .collect();
    assert_eq!(
        implemented,
        [
            (
                "INSTANTLOSS",
                cs_content::mission_control::TerminalOutcome::Failed
            ),
            (
                "INSTANTWIN",
                cs_content::mission_control::TerminalOutcome::Succeeded
            ),
        ],
        "M02's only implemented directives are its two outcome spellings"
    );
    assert_eq!(
        binding.unmeasured_keys(),
        Vec::<String>::new(),
        "every key M02 spells is covered by the stage A–D findings"
    );
    assert_eq!(
        record.measured().len() + implemented.len(),
        record.vocabulary() as usize,
        "measured + implemented partitions M02's vocabulary"
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
    // names.
    for key in ["INSTANTWIN", "INSTANTLOSS"] {
        let spelled = record.key(key).expect("M02 spells its outcome keys");
        assert!(
            spelled
                .agreed_shape()
                .is_some_and(|shape| shape.label() == "bare"),
            "{key} is spelled bare in M02"
        );
        assert!(terminal_outcome_of(key).is_some(), "{key} is an outcome");
    }
    assert!(
        record
            .keys()
            .iter()
            .filter(|key| {
                key.agreed_shape()
                    .is_some_and(|shape| shape.label() == "bare")
            })
            .all(|key| terminal_outcome_of(&key.key).is_some()),
        "in M02 every bare key spells an outcome"
    );

    // The record-level fields: the five measured keys, and the five M02
    // sound keys the vocabulary does not cover — counted, named, never
    // interpreted.
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
        "M02's record carries the five measured record fields, each once"
    );
    assert_eq!(
        binding.unclassified_record_keys(),
        [
            "MISSION_LOST_SOUND",
            "MISSION_WON_SOUND",
            "PRIMARY_COMPLETE_SOUND",
            "SECONDARY_COMPLETE_SOUND",
            "TERTIARY_COMPLETE_SOUND",
        ],
        "M02 adds five record-level sound keys outside the measured record \
         vocabulary; the binding names them instead of reading them"
    );
    for key in binding.unclassified_record_keys() {
        assert!(
            !CONTROL_RECORD_KEY_VOCABULARY.contains(&key.as_str()),
            "{key} is outside the measured record vocabulary by definition"
        );
    }
}

/// **The M02 sheet's regression priorities are located in the measured
/// record — as directives and block edges the record spells, not as
/// guessed timings or coordinates.** The success latch, the failure cause
/// and the remaining-target semantics are each re-derived here by walking
/// the control member's decoded document a second time through production
/// code, and cross-checked against the measurement's counts.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m02_b_the_objective_graph_the_sheet_priorities_need_is_measured_not_invented() {
    let binding = control_binding();
    let (document, _) = control_document();
    let blocks = blocks_of(&document);
    assert_eq!(
        blocks.len() as u32,
        binding.record.blocks(),
        "the independent walk sees every numbered block the measurement counted"
    );

    // The graph is closed except for one measured dangling address: every
    // index a cross-objective directive addresses is either a block this
    // record declares (zero-based record order, the measured addressing
    // rule) or one the record spells past its own end — which is a fact
    // about the original data, not a decoding error, and what the original
    // does with such an address is unmeasured.
    let directed_keys = [
        "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
        "KILL_OBJECTIVE_WHEN_I_COMPLETE",
        "NAP_OBJECTIVE_WHEN_I_COMPLETE",
        "SLEEP_OBJECTIVE_WHEN_I_COMPLETE",
        "WAKE_OBJECTIVE",
    ];
    let mut edges = Vec::new();
    let mut dangling = Vec::new();
    for block in &blocks {
        for (key, args) in &block.sites {
            if !directed_keys.contains(&key.as_str()) {
                continue;
            }
            for arg in args {
                let Some(index) = arg.as_int() else {
                    continue;
                };
                if (index as usize) < blocks.len() {
                    edges.push((block.index, key.to_owned(), index));
                } else {
                    dangling.push((block.index, key.to_owned(), index));
                }
            }
        }
    }
    assert_eq!(
        dangling,
        [(12, "WAKE_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), 50)],
        "M02's record spells exactly one cross-objective address past its 50 \
         blocks: block OBJECTIVE13 wakes zero-based index 50. The measured rule \
         says a directive addresses a block by its zero-based index, so this is \
         the original's own out-of-range address; the engine's behaviour for it \
         is unmeasured and recorded in the findings, never silently clamped here"
    );
    assert!(
        !edges.is_empty(),
        "M02's block graph carries wake/nap/kill edges"
    );

    // **Success versus destruction.** The mission's success latch is the one
    // block that spells the bare INSTANTWIN; its failure causes are the two
    // bare INSTANTLOSS blocks. The latch is not free-running: exactly one
    // block wakes it, so nothing can satisfy it before that block completes.
    let win: Vec<&Block> = blocks
        .iter()
        .filter(|block| block.sites.iter().any(|(key, _)| key == "INSTANTWIN"))
        .collect();
    assert_eq!(
        win.len(),
        1,
        "M02 spells exactly one success latch, in {}",
        win.first()
            .map_or("no block".to_owned(), |block| block.key.clone())
    );
    let latch = win[0];
    assert_eq!(
        binding
            .record
            .key("INSTANTWIN")
            .expect("the latch key is measured")
            .sites,
        1,
        "the measurement counts the same one site"
    );
    let wakeups: Vec<u32> = edges
        .iter()
        .filter(|(_, key, index)| key == "WAKE_OBJECTIVE_WHEN_I_COMPLETE" && *index == latch.index)
        .map(|(from, _, _)| *from)
        .collect();
    assert_eq!(
        wakeups.len(),
        1,
        "the success latch is woken by exactly one block ({:?}), so before that \
         block completes the latch cannot fire — the wrong-actor and wrong-session \
         halves of the sheet's priority need a runtime to observe and stay \
         unmeasured here",
        wakeups
    );
    let loss: Vec<&str> = blocks
        .iter()
        .filter(|block| block.sites.iter().any(|(key, _)| key == "INSTANTLOSS"))
        .map(|block| block.key.as_str())
        .collect();
    assert_eq!(
        loss.len(),
        2,
        "M02 spells two failure latches ({}), matching the measurement's two \
         INSTANTLOSS sites",
        loss.join(", ")
    );

    // **Remaining-target failure.** The inactive-completion-count blocks are
    // the remaining-target semantics: each names a threshold and at least
    // that many inactive member lists, so "destroy N of M" is record data,
    // not a guess.
    let thresholds: Vec<(&Block, u32)> = blocks
        .iter()
        .filter_map(|block| {
            block.sites.iter().find_map(|(key, args)| {
                (key == "INACTIVE_COMPLETION_COUNT")
                    .then(|| args.first().and_then(ZrdValue::as_int))
                    .flatten()
                    .map(|threshold| (block, threshold))
            })
        })
        .collect();
    assert_eq!(
        thresholds.len() as u32,
        binding
            .record
            .key("INACTIVE_COMPLETION_COUNT")
            .expect("the threshold key is measured")
            .sites,
        "the independent walk sees every threshold site the measurement counted"
    );
    for (block, threshold) in &thresholds {
        let lists = block
            .sites
            .iter()
            .filter(|(key, _)| key.starts_with("INACTIVE") && key != "INACTIVE_COMPLETION_COUNT")
            .count() as u32;
        assert!(
            lists >= *threshold,
            "{}: the threshold {threshold} cannot exceed the {} member lists the \
             block spells",
            block.key,
            lists
        );
        assert!(
            *threshold >= 1,
            "{}: a remaining-target threshold is a positive count",
            block.key
        );
    }
    let counted = binding
        .record
        .key("INACTIVE1")
        .expect("M02 spells inactive member lists")
        .sites;
    assert!(
        counted >= thresholds.len() as u32,
        "every threshold block spells at least one member list"
    );

    // **Player-aircraft transfer and capture versus destruction** need the
    // target-flag and inactive-member vocabulary, which M02 spells on both
    // sides: objective targets are added and removed, other targets are
    // added and removed, and inactive member lists resolve chained names.
    for key in [
        "ADD_OBJECTIVE_TARGET",
        "REMOVE_OBJECTIVE_TARGET",
        "ADD_OTHER_TARGET",
        "REMOVE_OTHER_TARGET",
        "INACTIVE1",
        "BEGIN_DORMANT",
    ] {
        let spelled = binding
            .record
            .key(key)
            .unwrap_or_else(|| panic!("M02 spells {key}"));
        assert!(
            spelled.sites > 0,
            "{key} carries sites the runtime predicates will read"
        );
    }

    // The record the census measured and the record the binding measured are
    // the same measurement, so the sheet mapping above holds for both.
    assert_eq!(
        Some(&binding.record),
        census_row().record(),
        "one measurement, two production derivations"
    );
}

/// **The measured compatibility gap: M02's control record does not lower,
/// and the refusal names exactly the nine-argument kill sites against the
/// host-call bound.** The mission's vocabulary is fully measured (the test
/// above), so the gap is not an unknown directive — it is the
/// per-signature argument bound the registry enforces, and this test pins
/// the refusal so the follow-up that changes the bound cannot land without
/// updating this pin. The campaign gate stays closed meanwhile.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m02_b_the_lowering_gap_is_named_and_the_campaign_gate_stays_closed() {
    let binding = control_binding();
    let row = census_row();
    let lowered = row
        .lowering_attempt()
        .expect("the census lowers M02's measured record");
    let attempt = lowered.attempt();
    let record = binding.record.clone();

    assert!(
        !record.is_complete(attempt),
        "M02's control record does not lower completely"
    );
    assert!(
        !row.is_complete(),
        "the census reports M02's row incomplete"
    );
    let lowering = record.lowering(attempt);
    let unmet: Vec<&str> = lowering.unmet().map(|row| row.kind.code()).collect();
    assert_eq!(
        unmet,
        ["call_arguments"],
        "the mission identity, objective identity and completion conditions all \
         lower; only the host calls refuse"
    );

    // The registry refused the kill key at registration — one refusal for
    // the key, because a signature longer than the bound refuses the whole
    // spec rather than narrowing what the name accepts.
    assert_eq!(
        attempt.unbound_keys.len(),
        1,
        "exactly one key failed registration: {:?}",
        attempt.unbound_keys
    );
    assert!(
        attempt.unbound_keys[0].contains("KILL_OBJECTIVE_WHEN_I_COMPLETE")
            && attempt.unbound_keys[0].contains("too many arguments"),
        "the refusal names the kill key and the bound it exceeded: {}",
        attempt.unbound_keys[0]
    );

    // Every site of that key refuses, and every other site binds.
    let refused: Vec<&str> = attempt
        .calls
        .iter()
        .filter_map(|outcome| match outcome {
            CallOutcome::Refused(reason) => Some(reason.as_str()),
            CallOutcome::Bound => None,
        })
        .collect();
    assert_eq!(
        refused.len(),
        8,
        "the kill key's eight sites all refuse: {refused:?}"
    );
    assert!(
        refused
            .iter()
            .all(|reason| reason.contains("KILL_OBJECTIVE_WHEN_I_COMPLETE")),
        "every refusal names the kill key"
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
            .filter(|o| **o == CallOutcome::Bound)
            .count(),
        record.sites() as usize - refused.len(),
        "every non-kill site binds"
    );
    assert!(
        attempt.validation.is_none(),
        "no program stood to validate: the attempt refuses before it"
    );
    assert_eq!(
        attempt.conditions.len() as u32,
        record.blocks(),
        "every block carries a condition verdict"
    );
    assert!(
        attempt
            .conditions
            .iter()
            .all(|outcome| matches!(outcome, ConditionOutcome::Lowered)),
        "M02's completion conditions all lower: the gap is the host-call bound, \
         not the predicates"
    );

    // The measured bound the refusal runs against, stated as the constant
    // rather than a magic number: the kill key's own sites spell argument
    // lists longer than it, which is why the spec cannot register.
    let kill = binding
        .record
        .key("KILL_OBJECTIVE_WHEN_I_COMPLETE")
        .expect("M02 spells the kill key");
    assert!(
        kill.agreed_shape().is_none(),
        "the kill key's sites disagree about their shape — that is why the lowering registers one signature per shape, and why one long shape refuses them all"
    );
    let longest = kill
        .shapes
        .iter()
        .map(|(shape, _)| shape.arity())
        .max()
        .expect("the kill key has shapes");
    assert!(
        longest > MAX_CALL_ARGS,
        "the longest kill signature ({longest} arguments) exceeds the registry's \
         per-signature bound of {MAX_CALL_ARGS}"
    );
    let sites_over_bound: u32 = kill
        .shapes
        .iter()
        .filter(|(shape, _sites)| shape.arity() > MAX_CALL_ARGS)
        .map(|(_, sites)| *sites)
        .sum();
    assert!(
        sites_over_bound >= 1,
        "at least one measured kill site spells an over-bound list"
    );

    // The gap keeps M02 out of every readiness claim.
    let census = survey_mission_control_programs(&game_dir()).expect("the census measures");
    assert!(
        !census.complete_missions().contains(&"zbd/c1/m02"),
        "M02 is not one of the census's complete rows"
    );
    assert!(
        !census.campaign_ready(),
        "the campaign gate stays closed while M02 — a bound, fully-measured \
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
// Synthetic: the refusal arms the retail installation never reaches
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

/// **The rule refuses an archive with no control member, and an archive with
/// two — it never guesses a filename.** This is the arm M02-B's binding
/// carries into `ControlProgramError::Control`, proved here on authored
/// members because no installation produces it.
#[test]
fn accept_m02_b_the_control_rule_refuses_an_archive_without_or_with_two_control_members() {
    let plain = DecodedMember::new(
        "plain.zrd",
        control_record(vec![(
            "MISSION_TIMER".to_owned(),
            zrd_list(vec![zrd_float(0.0)]),
        )]),
    );
    let plain_two = DecodedMember::new("plain_two.zrd", control_record(vec![]));
    let one = DecodedMember::new(
        "one.zrd",
        control_record(vec![block(
            1,
            vec![directive("BEGIN_DORMANT", vec![zrd_float(-1.0)])],
        )]),
    );
    let two = DecodedMember::new(
        "two.zrd",
        control_record(vec![block(
            1,
            vec![directive("BEGIN_DORMANT", vec![zrd_float(-1.0)])],
        )]),
    );

    assert_eq!(
        objective_blocks_of(&plain),
        0,
        "a record with no numbered block carries no control program"
    );
    let none = control_member(
        "zbd/synth/mission/zrdr.zbd",
        &[plain.clone(), plain_two.clone()],
    )
    .expect_err("an archive with no control member is refused");
    assert!(
        matches!(
            &none,
            ControlMemberError::NoControlMember { container, members }
                if container == "zbd/synth/mission/zrdr.zbd" && *members == 2
        ),
        "no control member is refused by name: {none:?}"
    );
    assert_eq!(
        control_member("zbd/synth/mission/zrdr.zbd", &[plain.clone(), one.clone()])
            .expect("one carrier is the control member")
            .name,
        "one.zrd",
        "the single carrier is chosen from its own record"
    );
    let both = control_member("zbd/synth/mission/zrdr.zbd", &[plain, one, two])
        .expect_err("two carriers refuse the archive");
    assert!(
        matches!(
            &both,
            ControlMemberError::AmbiguousControlMember { members, .. }
                if members == &["one.zrd".to_owned(), "two.zrd".to_owned()]
        ),
        "two carriers are refused and both are named, sorted: {both:?}"
    );
}

/// **The vocabulary partition is exact on an authored record: an outcome key
/// is implemented, a covered key is measured, an unknown key stays
/// unmeasured, a record-level key outside the vocabulary is counted and
/// named, and the sites add up.** Every arm runs on production
/// `measure_control_record`, so the retail partition above cannot pass on a
/// walk that drops or invents sites.
#[test]
fn accept_m02_b_the_vocabulary_partition_is_exact_on_an_authored_record() {
    let document = control_record(vec![
        ("MISSION_TIMER".to_owned(), zrd_list(vec![zrd_float(0.0)])),
        (
            "MISSION_WON_SOUND".to_owned(),
            zrd_list(vec![zrd_text("group")]),
        ),
        block(1, vec![directive("INSTANTWIN", vec![])]),
        block(
            2,
            vec![
                directive("BEGIN_DORMANT", vec![zrd_float(-1.0)]),
                directive("WAKE_OBJECTIVE_WHEN_I_COMPLETE", vec![zrd_int(0)]),
            ],
        ),
        block(3, vec![directive("SET_AI_", vec![zrd_text("unknown")])]),
    ]);
    let record = measure_control_record(&document);

    assert_eq!((record.blocks(), record.sites()), (3, 4));
    assert_eq!(
        record.keys().iter().map(|key| key.sites).sum::<u32>(),
        record.sites(),
        "every authored site belongs to exactly one key"
    );
    let implemented: Vec<&str> = record
        .implemented()
        .into_iter()
        .map(|(key, _)| key.key.as_str())
        .collect();
    assert_eq!(
        implemented,
        ["INSTANTWIN"],
        "the bare outcome key implements"
    );
    let unmeasured: Vec<&str> = record
        .unmeasured()
        .into_iter()
        .map(|(key, _)| key.key.as_str())
        .collect();
    assert_eq!(
        unmeasured,
        ["SET_AI_"],
        "a key no finding covers stays unmeasured"
    );
    assert_eq!(
        record.measured().len() + implemented.len() + unmeasured.len(),
        record.vocabulary() as usize,
        "measured + implemented + unmeasured partitions the vocabulary"
    );
    assert_eq!(
        record.unclassified_record_keys(),
        ["MISSION_WON_SOUND"],
        "a record-level key outside the vocabulary is named, never read"
    );
    assert!(
        record
            .record_fields()
            .iter()
            .any(|(field, _)| field.key() == "MISSION_TIMER"),
        "a measured record field is counted"
    );
}

/// **A directive site whose argument list exceeds the registry's
/// per-signature bound refuses — and one at the bound binds.** This is the
/// mechanism behind M02's measured gap, proved on authored records so CI
/// carries the refusal arm the retail installation reaches only through the
/// kill key. The follow-up task that changes the bound owns this pin.
#[test]
fn accept_m02_b_a_site_over_the_host_call_bound_refuses_and_one_at_the_bound_binds() {
    let authored = |count: u32| {
        let args: Vec<ZrdValue> = (0..count).map(zrd_int).collect();
        control_record(vec![block(
            1,
            vec![
                directive("BEGIN_DORMANT", vec![zrd_float(-1.0)]),
                directive("KILL_OBJECTIVE_WHEN_I_COMPLETE", args),
            ],
        )])
    };
    let mission = Ok(ContentId::from_source(ContentKind::Mission, "syn-01")
        .expect("a synthetic mission id is valid"));

    // At the bound: the site binds and the record lowers completely.
    let document = authored(MAX_CALL_ARGS as u32);
    let record = measure_control_record(&document);
    let lowered = lower_control_record(mission.clone(), "zbd/synth/mission", &document, &record);
    assert!(
        record.is_complete(lowered.attempt()),
        "a site at the bound of {MAX_CALL_ARGS} arguments binds: {:?}",
        lowered.attempt().unbound_keys
    );
    assert!(
        lowered.attempt().calls.contains(&CallOutcome::Bound),
        "the kill site produced a bound call"
    );

    // One over the bound: the whole spec refuses registration and the site
    // refuses with it.
    let document = authored(MAX_CALL_ARGS as u32 + 1);
    let record = measure_control_record(&document);
    let lowered = lower_control_record(mission, "zbd/synth/mission", &document, &record);
    assert_eq!(
        lowered.attempt().unbound_keys.len(),
        1,
        "the over-bound signature refuses the key's registration"
    );
    assert!(
        lowered.attempt().unbound_keys[0].contains("too many arguments"),
        "the refusal names the bound: {}",
        lowered.attempt().unbound_keys[0]
    );
    assert!(
        !record.is_complete(lowered.attempt()),
        "an over-bound site refuses its whole record, never a narrowed binding"
    );
    let refusals: Vec<&str> = lowered
        .attempt()
        .calls
        .iter()
        .filter_map(|outcome| match outcome {
            CallOutcome::Refused(reason) => Some(reason.as_str()),
            CallOutcome::Bound => None,
        })
        .collect();
    assert_eq!(refusals.len(), 1, "the one kill site refuses: {refusals:?}");
    assert!(
        lowered
            .registry()
            .get("KILL_OBJECTIVE_WHEN_I_COMPLETE")
            .is_none(),
        "the over-bound key is not in the registry"
    );
}

/// **The measured grammar keeps a disagreeing key's shapes and reads a bare
/// key as bare — the two readings M02's own record depends on.** An
/// `IDENTITY` site with two arguments beside it and one with three are two
/// shapes of one measured key; a text follower is the next key, not an
/// argument.
#[test]
fn accept_m02_b_a_disagreeing_key_keeps_every_shape_and_a_text_follower_is_the_next_key() {
    let document = control_record(vec![block(
        1,
        vec![
            directive("IDENTITY", vec![zrd_text("PRIMARY"), zrd_int(1)]),
            directive(
                "IDENTITY",
                vec![zrd_text("PRIMARY"), zrd_int(2), zrd_text("MSG_BRF")],
            ),
            directive("INSTANTLOSS", vec![]),
        ],
    )]);
    let record = measure_control_record(&document);
    let identity = record
        .key("IDENTITY")
        .expect("the authored identity key is measured");
    assert_eq!(identity.sites, 2, "both sites belong to one key");
    assert!(
        identity.agreed_shape().is_none(),
        "the two sites disagree about their shape"
    );
    // The concrete shapes, spelled out so a reader can check the grammar:
    // `[text,int]` at the two-argument site and `[text,int,text]` at the
    // three-argument one, one site each.
    let shapes: Vec<(String, u32)> = identity
        .shapes
        .iter()
        .map(|(shape, sites)| (shape.label(), *sites))
        .collect();
    assert_eq!(
        shapes,
        [
            ("[text,int]".to_owned(), 1),
            ("[text,int,text]".to_owned(), 1),
        ],
        "the two argument shapes are kept separately: {shapes:?}"
    );
    let loss = record
        .key("INSTANTLOSS")
        .expect("the outcome key is measured");
    assert_eq!(
        loss.agreed_shape().map(|shape| shape.label()),
        Some("bare".to_owned()),
        "a text follower is the next directive's key, so the site is bare"
    );
    assert_eq!(
        record.sites(),
        3,
        "the three authored sites are counted exactly"
    );
}
