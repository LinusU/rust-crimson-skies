//! Acceptance stage M24-B: M24's mission-specific compatibility surface — the
//! mission control program the installation ships for the campaign's final
//! mission, bound through production engine systems and regressed against the
//! lowering that decides what the engine may honour
//! (`missions/M24.md`, work order `M24-B`).
//!
//! Shared contract: `docs/contracts/SCRIPT-MISSION.md` ("Source adapter
//! acceptance", "Host interface", "Objective event ordering"). Findings:
//! `docs/findings/2026-10-10-m24-b-compatibility-gaps.md`.
//!
//! # What this stage adds, and what it deliberately does not
//!
//! M24-A bound *which* retail mission the work order names
//! (`mission/ch5-m04`, `script/c5-m04-zrdr`, the reader archive
//! `ZBD/C5/M04/zrdr.zbd`) and left every objective, actor and directive
//! unbound. This stage binds the archive through the production systems
//! M02-B built (`SourceContext::control_program`) and measures it a second
//! time through `cs_app::mission_control`, so three independent derivations —
//! the mission binding, the control binding and the retail census — must
//! agree before any assertion below can pass.
//!
//! The stage's minimum acceptance scenario is *"All discovered
//! mission-specific behavior uses production engine systems and regression
//! tests."* M24's discovered mission-specific behavior is its control
//! record: 63 numbered blocks, 267 directive sites, 40 distinct keys, the
//! block graph those spell, and the two lowering gaps measured below. What
//! is different at M24, the twenty-fourth and last campaign position:
//!
//! * **The capital battle transition is a three-zeppelin chain.** Two
//!   `COMPLETED_ZEPCANNONS` sites (blocks 14 and 15) arm when `dantezep`
//!   closes to 2000 about `piratezep` and `blackswanzep`; the friendly
//!   `dantezep` falls through four measured thresholds — five gasbag panels
//!   at counts 1/2/3, fourteen engine nodes at counts 7/12 — and its fall
//!   block (OBJECTIVE10, the record's first PRIMARY) retires the capital's
//!   objective-target flag, plays the fall sound and kills ten blocks; the
//!   enemy `piratezep` retreats exactly when a second of its six gasbag
//!   panels no longer carries the in-play bit (`SET_AI_NET` onto
//!   `M4PZRetreat`); the final `blackswanzep` is woken with its turrets and
//!   is never an objective target at all.
//! * **The target-eligibility change is five flag writes and no add.** The
//!   record spells no `ADD_OBJECTIVE_TARGET` site anywhere: it only clears
//!   two objective-target flags (`dantezep`, and the chained
//!   `[piratezep, rock_zeppelin]`) and sets three other-target flags — the
//!   chain again, and `blackswanzep` twice. Eligibility only decreases in
//!   this record; the initial target set is not this member's data.
//! * **The campaign ending is one win and one loss latch, both nap-armed.**
//!   `INSTANTWIN` (OBJECTIVE21, the record's only SECONDARY identity) is
//!   armed solely by a 45-second nap from OBJECTIVE28; `INSTANTLOSS`
//!   (OBJECTIVE39) by two naps, 15 seconds from OBJECTIVE38 and 20 from
//!   OBJECTIVE62. Nothing addresses OBJECTIVE1, and M24 is the campaign's
//!   last row, so no successor mission follows the win.
//! * **The record does not lower, by two named gaps.**
//!   `STOP_QUEUED_SOUNDS` is spelled six times with 1, 9, 7, 8, 7 and 8
//!   names; the 9-name shape exceeds `MAX_CALL_ARGS`, so the whole spec is
//!   refused and all six sites report `unknown host call` — the same
//!   measured gap M16-B, M02-B, M03-B and M06-B record. And `SET_AI_`, the
//!   single site of block 19's five-pair list, is a truncated spelling no
//!   finding covers; the engine-image test shows it absent from the
//!   directive-key table the original parser looks names up in, exactly
//!   where M16-B found its four comment words.
//!
//! Nothing here is `verified_original` (AGENTS.md rule 8): the work-order ↔
//! mission join remains M24-A's inference, directive *effects* are the
//! M01-LC findings' static readings of the original code, and no original
//! executable has been run. The wrong-actor, wrong-session and
//! repeated-event halves of the sheet's three priorities are runtime
//! observations: what this stage pins is the *spelled data* they are built
//! from — names, members, edges and thresholds — never a timing or a
//! verdict the record does not carry.
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
use cs_app::world::triggers::survey_retail_trigger_volumes;
use cs_assets::install::sha256;
use cs_content::campaign_bindings::{MissionLabel, SourceContext};
use cs_content::coordinates::load_engine_image;
use cs_content::mission_control::{
    AnimList, CallOutcome, ConditionOutcome, ControlRecordField, DecodedMember,
    DirectiveDisposition, DirectiveOperation, TerminalOutcome, measure_control_record,
    objective_blocks_of, terminal_outcome_of,
};
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{ZrdValue, decode_zrd, objective_record, zrd_flat_fields};
use cs_content::world::{RetailTriggerVolumeSurvey, WorldId};
use cs_formats::script_raw::discover_container;
use cs_script::ir::Value;
use cs_types::content::{ContentId, ContentKind};
use cs_types::install::RelativePath;

use crate::common::{label, load_inventory};

/// The census row label of the mission: the mission-scoped reader archive
/// F13-B's rule derives from the installation.
const MISSION: &str = "zbd/c5/m04";

/// The reader archive the installation ships for M24 — the program span
/// `missions/bindings/M24.json` cites.
const CONTAINER: &str = "ZBD/C5/M04/zrdr.zbd";
/// SHA-256 of that whole archive, from production discovery.
const CONTAINER_SHA256: &str = "28d0076b4a335c19dd886772c120bd298bda8544196f15c2427becfe63dcfef4";
/// The archive's length in bytes — M24-A's own source span.
const CONTAINER_LENGTH: u64 = 62_785;
/// The member the measured rule chose.
const CONTROL_MEMBER: &str = "objectives.zrd";
/// The control member's first byte inside the archive.
const CONTROL_OFFSET: u64 = 23_007;
/// The control member's length in bytes.
const CONTROL_LENGTH: u64 = 19_590;
/// SHA-256 of the control member's own bytes.
const CONTROL_SHA256: &str = "b0430f95ddf4bf522207284eafb2e946a1cb8d1aecd0c69e83270947c01f2da8";
/// The detection-zone member of this archive.
const DZONES_MEMBER: &str = "dzones.zrd";
/// The zeppelin-declaration member of this archive.
const ZEPPELINS_MEMBER: &str = "zeppelins.zrd";
/// The chapter-5 world container the node declarations live in.
const CHAPTER_CONTAINER: &str = "ZBD/C5/zrdr.zbd";

/// The numbered blocks of the control member.
const BLOCKS: u32 = 63;
/// The directive sites of the control member.
const SITES: u32 = 267;
/// The distinct directive keys of the control member.
const KEYS: usize = 40;
/// The `STOP_QUEUED_SOUNDS` sites and the one `SET_AI_` site that refuse.
const REFUSED: usize = 7;

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M24-B needs the retail capability; run this suite with \
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

/// M24's work-order label, from the committed inventory rather than a
/// literal.
fn m24() -> MissionLabel {
    label("M24")
}

/// M24's declared discovery title, from the committed inventory.
fn m24_title() -> String {
    load_inventory()
        .iter()
        .find(|(work_order, _)| work_order.as_str() == "M24")
        .map(|(_, title)| title.clone())
        .expect("the declared inventory has an M24 work order")
}

/// M24's control binding, derived fresh through production code.
fn control_binding() -> cs_content::campaign_bindings::MissionControlBinding {
    context()
        .control_program(m24(), &m24_title())
        .expect("M24's control program binds through the measured rule")
}

/// M24's row in the retail control census: the same installation measured a
/// second time through `cs_app::mission_control`.
fn census_row() -> &'static RetailControlRow {
    static ROW: OnceLock<RetailControlRow> = OnceLock::new();
    ROW.get_or_init(|| {
        let census = survey_mission_control_programs(&game_dir())
            .expect("the installation measures a control census");
        census
            .row(MISSION)
            .expect("M24's reader archive is measured by the census")
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

/// The trigger-volume survey with every campaign mission's `dzones.zrd`
/// declaration attached, built once for the whole suite.
fn zone_survey() -> &'static RetailTriggerVolumeSurvey {
    static SURVEY: OnceLock<RetailTriggerVolumeSurvey> = OnceLock::new();
    SURVEY.get_or_init(|| {
        survey_retail_trigger_volumes(&game_dir())
            .expect("the production trigger-volume survey runs on the installation")
    })
}

/// The control member's decoded document, re-read from the archive through
/// production discovery — an independent walk from the binding's, so the
/// graph assertions below cannot be satisfied by the binding's own output.
fn control_document() -> (ZrdValue, Vec<(String, u64, u64, u32)>) {
    let binding = control_binding();
    let bytes = std::fs::read(game_dir().join(&binding.program_asset))
        .expect("M24's reader archive reads from disk");
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

/// Every distinct text a `.zrd` container's decoded members spell — the
/// declarations an operand can resolve against, through production
/// discovery and the production `.zrd` decoder.
fn texts_in_container(spelling: &str) -> std::collections::BTreeSet<String> {
    let path = game_dir().join(spelling);
    let bytes =
        std::fs::read(&path).unwrap_or_else(|error| panic!("cannot read {spelling}: {error}"));
    let logical = spelling.to_lowercase();
    let relative = RelativePath::new(&logical).expect("the container path is relative");
    let discovery = discover_container(&relative.logical_key(), &relative, &bytes);
    let mut texts = std::collections::BTreeSet::new();
    for program in discovery.programs() {
        let Ok(decoded) = decode_zrd(program.bytes()) else {
            continue;
        };
        collect_texts(&decoded, &mut texts);
    }
    texts
}

/// Collects every text node of a decoded `.zrd` document.
fn collect_texts(value: &ZrdValue, texts: &mut std::collections::BTreeSet<String>) {
    match value {
        ZrdValue::Text(text) => {
            texts.insert(text.clone());
        }
        ZrdValue::List(children) => {
            for child in children {
                collect_texts(child, texts);
            }
        }
        _ => {}
    }
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

/// The sites one key is spelled with inside one block.
fn sites_of(blocks: &[Block], number: u32, key: &str) -> Vec<Vec<ZrdValue>> {
    blocks
        .iter()
        .find(|block| number_of(block) == number)
        .unwrap_or_else(|| panic!("the record declares OBJECTIVE{number}"))
        .sites
        .iter()
        .filter(|(name, _)| name == key)
        .map(|(_, args)| args.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Retail: the binding ties M24's control program to M24's identities
// ---------------------------------------------------------------------------

/// **M24's control program is bound to the same identities as its mission
/// binding.** `SourceContext::control_program` resolves the work order
/// through the same title join `SourceContext::bind` uses, so the two
/// derivations name one mission, one program and one archive; the member
/// the binding cites is the member the measured rule picked over the
/// archive's whole member set, with a digest over that member's own bytes;
/// and the retail census — a third derivation through its own discovery
/// path — measured the same archive, the same member and the same record.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m24_b_m24s_control_program_is_bound_to_the_same_identities_as_its_mission_binding() {
    let binding = control_binding();
    let mission_binding = context()
        .bind(m24(), &m24_title())
        .expect("M24's mission binding resolves");

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
        "mission/ch5-m04",
        "M24 is the fifth chapter's fourth mission, as M24-A bound it"
    );
    assert_eq!(
        mission_binding.campaign_position,
        Some(23),
        "the join selected campaign position 23, the twenty-fourth mission — \
         the campaign's last"
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
        "script/c5-m04-zrdr",
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
        .expect("M24's reader archive reads from disk");
    assert_eq!(
        bytes.len() as u64,
        CONTAINER_LENGTH,
        "the archive on disk is the length M24-A's source span cites"
    );
    assert_eq!(
        binding.program_length,
        bytes.len() as u64,
        "the bound length is the archive's own"
    );
    assert_eq!(
        binding.program_sha256, CONTAINER_SHA256,
        "the bound digest is the one M24-A's source span cites"
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
        CONTROL_MEMBER,
        "on this installation the rule picks the member production discovery \
         spells `objectives.zrd` — as a *result* of the rule, not as its input"
    );
    assert_eq!(
        binding.members.len(),
        14,
        "M24's archive offers fourteen members"
    );
    let row = binding.control_row().expect("the chosen member has a row");
    assert_eq!((row.offset, row.len), (CONTROL_OFFSET, CONTROL_LENGTH));
    let end = (row.offset + row.len) as usize;
    assert!(
        end <= bytes.len(),
        "the member's range lies inside the archive"
    );
    assert_eq!(
        binding.control_sha256, CONTROL_SHA256,
        "the member digest is the one this stage pins"
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

    // Unlike M16 and M17, the control member is *also* the archive's longest
    // member — so for this mission length would coincide with the rule. The
    // rule is still the block count: the walk below re-derives it, and the
    // member is not the archive's first, so position still cannot be the
    // selection.
    let longest = binding
        .members
        .iter()
        .max_by_key(|member| member.len)
        .expect("the archive declares members");
    assert_eq!(
        longest.name.to_lowercase(),
        CONTROL_MEMBER,
        "for M24 the control member is also the longest — recorded as a fact, \
         not as the rule"
    );
    assert_ne!(
        binding.members.first().map(|member| member.name.as_str()),
        Some(binding.control_member.as_str()),
        "the control member is not the archive's first member, so position \
         cannot be the selection either"
    );
    // …and the two mission-unique members this chapter-5 finale carries —
    // the zeppelin declarations and the Miles drop animation — are offered
    // beside it, so the battle data below has a home in the archive.
    for member in [ZEPPELINS_MEMBER, "miles_drop.zrd", "glidebomb.zrd"] {
        assert!(
            binding
                .members
                .iter()
                .any(|row| row.name.eq_ignore_ascii_case(member)),
            "the archive no longer offers the {member} member"
        );
    }

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

// ---------------------------------------------------------------------------
// Retail: the vocabulary partitions, and the one truncated key
// ---------------------------------------------------------------------------

/// **M24's directive vocabulary partitions exactly and refuses no key — it
/// spells both terminal latches and exactly one truncated key.** Every key
/// M24 spells carries a measured disposition except `SET_AI_`, the single
/// site of OBJECTIVE19. Both outcome keys are in the record: `INSTANTWIN`
/// and `INSTANTLOSS`, one site each. No block is unreadable, and no
/// record-level key falls outside the measured record vocabulary.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m24_b_the_measured_vocabulary_partitions_and_refuses_no_m24_key() {
    let binding = control_binding();
    let record = &binding.record;

    assert_eq!(
        (record.blocks(), record.sites()),
        (BLOCKS, SITES),
        "M24 declares {BLOCKS} numbered blocks and {SITES} directive sites"
    );
    assert_eq!(
        record.keys().iter().map(|key| key.sites).sum::<u32>(),
        record.sites(),
        "every measured site belongs to exactly one key"
    );
    assert_eq!(
        record.vocabulary() as usize,
        KEYS,
        "M24 spells {KEYS} distinct directive keys"
    );
    assert!(
        record.refusals().is_empty(),
        "the measured directive grammar parses every M24 block: {:?}",
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
        "M24's only implemented directives are its two terminal outcomes, one \
         site each"
    );
    assert_eq!(
        binding.unmeasured_keys(),
        ["SET_AI_"],
        "the only key no finding covers is the truncated `SET_AI_` spelling \
         of OBJECTIVE19 — the stage's one measured vocabulary gap"
    );
    assert_eq!(
        record.measured().len() + implemented.len() + binding.unmeasured_keys().len(),
        record.vocabulary() as usize,
        "measured + implemented + unmeasured partitions M24's vocabulary"
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

    // The outcome keys are bare spellings; `SET_AI_` is not — it carries the
    // five-pair list, which is why its refusal is a vocabulary gap and not
    // a comment word.
    for key in ["INSTANTWIN", "INSTANTLOSS"] {
        let outcome = record
            .key(key)
            .unwrap_or_else(|| panic!("M24 spells {key}"));
        assert!(
            outcome
                .agreed_shape()
                .is_some_and(|shape| shape.label() == "bare"),
            "{key} is spelled bare in M24"
        );
        assert!(terminal_outcome_of(key).is_some(), "{key} is an outcome");
    }
    let truncated = record
        .key("SET_AI_")
        .expect("M24 spells the truncated SET_AI_ key");
    assert_eq!(truncated.sites, 1, "the truncated key is a single site");
    assert!(
        matches!(
            truncated.disposition(),
            DirectiveDisposition::Unmeasured { .. }
        ),
        "SET_AI_ stays unmeasured: no original observation states what it does"
    );
    assert!(
        truncated
            .agreed_shape()
            .is_some_and(|shape| shape.label() != "bare"),
        "SET_AI_ carries an argument list — it is a truncated directive, not \
         a comment word"
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
        "M24's record carries the five measured record fields, each once"
    );
    assert!(
        binding.unclassified_record_keys().is_empty(),
        "M24 spells no record-level key outside the measured vocabulary"
    );

    // The exact key list: pinning it is also the proof that the vocabulary
    // carries no collision, damage or interaction directive — the families
    // the sheet's priorities might have lived in are not in this member at
    // all, and no `ADD_OBJECTIVE_TARGET` exists to inspect (the target-
    // eligibility test below measures that half).
    let mut keys: Vec<&str> = record.keys().iter().map(|key| key.key.as_str()).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "ADD_OTHER_TARGET",
            "BEGIN_DORMANT",
            "COMPLETED_SOUND_GROUP",
            "COMPLETED_ZEPCANNONS",
            "DANGER_ZONES_COMPLETED",
            "DANGER_ZONES_COMPLETION_COUNT",
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
            "SET_AI_",
            "SET_AI_NET",
            "SET_AI_TEAM",
            "SET_HELP_LABEL",
            "STOP_QUEUED_SOUNDS",
            "TRAVELERS",
            "WAKEUP_ENEMIES",
            "WAKEUP_GENERATOR",
            "WAKEUP_SOUND_GROUP",
            "WAKEUP_ZEP_TURRETS",
            "WAKE_ANIM",
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
        ],
        "M24's whole directive vocabulary, sorted byte-wise, so an added or \
         dropped key fails here"
    );
}

// ---------------------------------------------------------------------------
// Retail: the block graph, and the nap-armed latches
// ---------------------------------------------------------------------------

/// **Every cross-objective address M24 spells names a block this record
/// declares, both terminal latches are armed by naps alone, and the five
/// empty blocks are stubs no edge reaches.**
///
/// The record declares `OBJECTIVE1` … `OBJECTIVE63`; the three
/// cross-objective keys M24 spells carry their addresses, none zero, none
/// negative and none past the last block. A spelled address names the block
/// it decrements to (the parse stores `address - 1`; that rule, and the
/// refusal it raises past the count, are M02-B-FU3's measurement of the
/// original — Rally #802 — not re-measured here), so under it every address
/// resolves to a block the record declares. The wrong-actor, wrong-session
/// and repeated-event halves of the sheet's priorities are runtime
/// observations and stay unmeasured (M24-C).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m24_b_the_block_graph_is_closed_under_the_records_own_numbering() {
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

    // The three cross-objective keys M24 spells (it spells no `SLEEP_…`, no
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
            "M24 does not spell {absent}"
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
                    .unwrap_or_else(|| panic!("M24 spells {key}"))
                    .sites
            })
            .sum::<u32>(),
        "the walk visits every cross-objective site the measurement counted"
    );

    // Every address names a block this record declares: none is zero or
    // negative, none is past the last block, so `address - 1` — the index
    // the parse stores — always lands inside the record's own 63 blocks.
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
        "the lowest address M24 spells names OBJECTIVE2 — nothing ever \
         addresses OBJECTIVE1, the mission's two-second opener, so no edge \
         can re-enter it"
    );
    assert_eq!(
        addresses.iter().copied().max(),
        Some(i64::from(BLOCKS)),
        "the highest address M24 spells names the last declared block — the \
         record's own boundary is live and nothing crosses it"
    );

    // **Both latches are nap-armed.** Exactly two blocks spell an outcome —
    // OBJECTIVE21's `INSTANTWIN` and OBJECTIVE39's `INSTANTLOSS` — each
    // starts dormant with no timed wake, and each is reachable only through
    // naps.
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
        [(21, "INSTANTWIN"), (39, "INSTANTLOSS")],
        "OBJECTIVE21 ends the mission in success and OBJECTIVE39 in failure — \
         the only two outcome spellings M24 carries"
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
    for latch in [21_i64, 39] {
        let edges = incoming(latch);
        assert!(
            !edges.is_empty(),
            "OBJECTIVE{latch} receives at least one arming edge"
        );
        assert!(
            edges
                .iter()
                .filter(|(_, key, _)| key == "NAP_OBJECTIVE_WHEN_I_COMPLETE")
                .count()
                >= 1,
            "OBJECTIVE{latch} is armed by at least one nap: {edges:?}"
        );
        assert!(
            blocks
                .iter()
                .find(|block| number_of(block) == latch as u32)
                .is_some_and(|block| {
                    block.sites.iter().any(|(key, args)| {
                        key == "BEGIN_DORMANT" && args.as_slice() == [ZrdValue::Float(-1.0)]
                    })
                }),
            "OBJECTIVE{latch} starts dormant with no timed wake, so only a \
             nap completion can arm it"
        );
    }
    // The loss latch's one non-nap edge is a kill, not an arming: block 34
    // retires it — the mutual exclusion the campaign-ending test walks.
    assert_eq!(
        incoming(21)
            .iter()
            .filter(|(_, key, _)| *key != "NAP_OBJECTIVE_WHEN_I_COMPLETE")
            .count(),
        0,
        "the win latch receives nothing but its one nap"
    );
    assert_eq!(
        incoming(39)
            .iter()
            .filter(|(_, key, _)| *key == "KILL_OBJECTIVE_WHEN_I_COMPLETE")
            .count(),
        1,
        "the loss latch receives exactly one kill edge beside its two naps"
    );

    // The start structure: four blocks self-wake on their dormant timers —
    // OBJECTIVE1 at two seconds, OBJECTIVE16 at five, OBJECTIVE26 at seven
    // and OBJECTIVE45 at two minutes; every other `BEGIN_DORMANT` disables
    // the timed wake at −1.0, and the five empty blocks spell no site at
    // all — stubs that receive no edge, as the address set above shows.
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
        46,
        "46 of the 63 blocks spell BEGIN_DORMANT — the five empty blocks and \
         the twelve always-awake blocks spell none"
    );
    let timed: Vec<u32> = dormant
        .iter()
        .filter(|(_, seconds)| *seconds >= 0.0)
        .map(|(block, _)| *block)
        .collect();
    assert_eq!(
        timed,
        [1, 16, 26, 45],
        "OBJECTIVE1 wakes itself at two seconds, OBJECTIVE16 at five, \
         OBJECTIVE26 at seven and OBJECTIVE45 at 120; the other 42 timers \
         are off"
    );
    let empty: Vec<u32> = blocks
        .iter()
        .filter(|block| block.sites.is_empty())
        .map(number_of)
        .collect();
    assert_eq!(
        empty,
        [40, 41, 42, 43, 44],
        "OBJECTIVE40…44 spell no directive — inert stubs no edge reaches"
    );
    for stub in [40_i64, 41, 42, 43, 44] {
        assert!(
            incoming(stub).is_empty(),
            "OBJECTIVE{stub} receives no wake, nap or kill edge"
        );
    }

    // The kill sites, as spelled: M24 spells seventeen of them, and the
    // largest retires the whole danger-zone chain at once.
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
        killed_by
            .iter()
            .map(|(block, _)| *block)
            .collect::<Vec<_>>(),
        [
            10, 13, 25, 34, 38, 48, 49, 50, 51, 52, 54, 55, 56, 57, 58, 59, 62
        ],
        "the seventeen blocks that spell a kill site, in record order"
    );
    assert_eq!(
        killed_by
            .iter()
            .find(|(block, _)| *block == 59)
            .map(|(_, list)| list.len()),
        Some(12),
        "OBJECTIVE59's kill list retires the twelve danger-zone blocks at once"
    );
    // A kill is not an arming: the win latch is never killed anywhere in the
    // record, and the loss latch is killed by exactly one site — the
    // detour gate OBJECTIVE34, the mutual exclusion the campaign-ending
    // test walks.
    assert!(
        !killed_by.iter().any(|(_, list)| list.contains(&21)),
        "the win latch is never killed"
    );
    assert_eq!(
        killed_by
            .iter()
            .filter(|(_, list)| list.contains(&39))
            .map(|(block, _)| *block)
            .collect::<Vec<_>>(),
        [34],
        "the loss latch is killed by OBJECTIVE34 and nothing else"
    );

    // The record the census measured and the record the binding measured are
    // the same measurement, so every pin above holds for both derivations.
    assert_eq!(
        Some(&binding.record),
        census_row().record(),
        "one measurement, two production derivations"
    );
}

// ---------------------------------------------------------------------------
// Retail: where the sheet's three regression priorities live
// ---------------------------------------------------------------------------

/// **The sheet's capital-battle priority is the record's three-zeppelin
/// chain — turret wakes, approach gates, cannon writes, damage thresholds
/// and the retreat reassignment — spelled as directives, operands, block
/// edges and member bytes, never as a guessed timing or coordinate.**
///
/// M24-A left every actor and predicate unbound. What this stage adds is
/// the *data* the transition is built from: the two `COMPLETED_ZEPCANNONS`
/// sites that arm when the friendly `dantezep` closes on each enemy
/// zeppelin, the four thresholds the friendly capital falls through, the
/// one threshold at which the enemy `piratezep` retreats, and the members
/// that declare the zeppelins, the zones and the nets those directives name.
/// The half a runtime must observe — what the +0xc cannon byte drives, who
/// wins a window in which a success and a failure latch are both armed —
/// stays unmeasured (M24-C).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m24_b_the_capital_battle_transition_is_the_zeppelin_chain_the_record_spells() {
    let binding = control_binding();
    let (document, _) = control_document();
    let blocks = blocks_of(&document);

    // The mission opens by waking both capitals' turrets two seconds in, and
    // the player's own approach to the friendly capital is the record's
    // second and third always-awake gate.
    assert_eq!(
        sites_of(&blocks, 1, "WAKEUP_ZEP_TURRETS"),
        [vec![
            ZrdValue::Text("piratezep".to_owned()),
            ZrdValue::Text("dantezep".to_owned()),
        ]],
        "OBJECTIVE1 wakes the pirate and Dante zeppelin turrets at the \
         two-second mark"
    );
    assert_eq!(
        sites_of(&blocks, 2, "TRAVELERS"),
        [vec![
            ZrdValue::Text("player".to_owned()),
            ZrdValue::Text("APPROACHING".to_owned()),
            ZrdValue::Text("dantezep".to_owned()),
            ZrdValue::Float(2000.0),
            ZrdValue::Int(1),
        ]],
        "OBJECTIVE2 completes when the player closes to 2000 about dantezep"
    );
    assert_eq!(
        sites_of(&blocks, 3, "TRAVELERS"),
        [vec![
            ZrdValue::Text("player".to_owned()),
            ZrdValue::Text("APPROACHING".to_owned()),
            ZrdValue::Text("dantezep".to_owned()),
            ZrdValue::Float(1500.0),
            ZrdValue::Int(1),
        ]],
        "OBJECTIVE3 closes the radius to 1500 — the deeper approach gate"
    );

    // The capital battle transition itself: each enemy zeppelin gets one
    // always-awake block whose only conditions are dantezep approaching
    // within 2000 and the cannon write that arms both zeppelins' +0xc byte.
    assert_eq!(
        sites_of(&blocks, 14, "TRAVELERS"),
        [vec![
            ZrdValue::Text("dantezep".to_owned()),
            ZrdValue::Text("APPROACHING".to_owned()),
            ZrdValue::Text("piratezep".to_owned()),
            ZrdValue::Float(2000.0),
            ZrdValue::Int(1),
        ]],
        "OBJECTIVE14 completes when dantezep closes on the pirate zeppelin"
    );
    assert_eq!(
        sites_of(&blocks, 14, "COMPLETED_ZEPCANNONS"),
        [vec![
            ZrdValue::List(vec![
                ZrdValue::Text("dantezep".to_owned()),
                ZrdValue::Int(1),
            ]),
            ZrdValue::List(vec![
                ZrdValue::Text("piratezep".to_owned()),
                ZrdValue::Int(1),
            ]),
        ]],
        "the same completion stores the cannon byte on both zeppelins — the \
         measured capital-battle transition"
    );
    assert_eq!(
        sites_of(&blocks, 15, "TRAVELERS"),
        [vec![
            ZrdValue::Text("dantezep".to_owned()),
            ZrdValue::Text("APPROACHING".to_owned()),
            ZrdValue::Text("blackswanzep".to_owned()),
            ZrdValue::Float(2000.0),
            ZrdValue::Int(1),
        ]],
        "OBJECTIVE15 is the same gate for the Black Swan, behind its own \
         dormant timer"
    );
    assert_eq!(
        sites_of(&blocks, 15, "COMPLETED_ZEPCANNONS"),
        [vec![
            ZrdValue::List(vec![
                ZrdValue::Text("dantezep".to_owned()),
                ZrdValue::Int(1),
            ]),
            ZrdValue::List(vec![
                ZrdValue::Text("blackswanzep".to_owned()),
                ZrdValue::Int(1),
            ]),
        ]],
        "the Black Swan's cannon write is the same two-zeppelin shape"
    );

    // The friendly capital's fall, threshold by threshold: five gasbag
    // panels at counts 1 and 2 (blocks 4 and 5), fourteen engine nodes at
    // counts 7 and 12 (blocks 8 and 9), then OBJECTIVE10 — the record's
    // first PRIMARY identity — at three panels, which retires the
    // capital's objective-target flag and kills ten blocks.
    let gasbags: Vec<ZrdValue> = (1..=5)
        .map(|number| ZrdValue::Text(format!("gasbag{number}")))
        .collect();
    for (block, threshold) in [(4, 1), (5, 2)] {
        assert_eq!(
            sites_of(&blocks, block, "INACTIVE_COMPLETION_COUNT"),
            [vec![ZrdValue::Int(threshold)]],
            "OBJECTIVE{block} completes at {threshold} inactive gasbag panel(s)"
        );
        for (number, leaf) in gasbags.iter().enumerate() {
            assert_eq!(
                sites_of(&blocks, block, &format!("INACTIVE{}", number + 1)),
                [vec![
                    ZrdValue::Text("dantezep".to_owned()),
                    leaf.clone(),
                    ZrdValue::Text("panels".to_owned()),
                ]],
                "OBJECTIVE{block} watches gasbag panel {} through its chained \
                 member name",
                number + 1
            );
        }
    }
    for (block, threshold) in [(8, 7), (9, 12)] {
        assert_eq!(
            sites_of(&blocks, block, "INACTIVE_COMPLETION_COUNT"),
            [vec![ZrdValue::Int(threshold)]],
            "OBJECTIVE{block} completes at {threshold} inactive engine node(s)"
        );
        assert_eq!(
            (1..=14)
                .map(|number| sites_of(&blocks, block, &format!("INACTIVE{number}")).len())
                .sum::<usize>(),
            14,
            "OBJECTIVE{block} watches the fourteen chained engine nodes"
        );
        assert_eq!(
            sites_of(&blocks, block, "INACTIVE1"),
            [vec![
                ZrdValue::Text("dantezep".to_owned()),
                ZrdValue::Text("reng11".to_owned()),
                ZrdValue::Text("healthy".to_owned()),
            ]],
            "OBJECTIVE{block}'s first engine node is dantezep's own reng11"
        );
    }
    assert_eq!(
        sites_of(&blocks, 10, "IDENTITY"),
        [vec![ZrdValue::Text("PRIMARY".to_owned()), ZrdValue::Int(1)]],
        "the capital's fall is the mission's first primary objective"
    );
    assert_eq!(
        sites_of(&blocks, 10, "REMOVE_OBJECTIVE_TARGET"),
        [vec![ZrdValue::Text("dantezep".to_owned())]],
        "the fall retires the friendly capital's objective-target flag"
    );
    assert_eq!(
        sites_of(&blocks, 10, "KILL_OBJECTIVE_WHEN_I_COMPLETE"),
        [vec![
            ZrdValue::Int(2),
            ZrdValue::Int(3),
            ZrdValue::Int(4),
            ZrdValue::Int(5),
            ZrdValue::Int(6),
            ZrdValue::Int(7),
            ZrdValue::Int(8),
            ZrdValue::Int(9),
            ZrdValue::Int(24),
            ZrdValue::Int(25),
        ]],
        "the fall kills the approach gates, the four thresholds, the Black \
         Swan opener and the pirate-damage blocks"
    );
    assert_eq!(
        sites_of(&blocks, 11, "WAKE_ANIM"),
        [vec![ZrdValue::Text("all_dtzep_gasbags".to_owned())]],
        "a half-second nap wakes OBJECTIVE11, which executes the gasbag-drop \
         animation and feeds dantezep's generator one more unit"
    );
    assert_eq!(
        sites_of(&blocks, 11, "WAKEUP_GENERATOR"),
        [vec![
            ZrdValue::Text("dantezep".to_owned()),
            ZrdValue::Int(1),
        ]],
        "the falling capital's generator is fed one unit — the spelled \
         respawn the fall itself schedules"
    );

    // The enemy capital's retreat: the pirate zeppelin's six gasbag panels
    // are watched by three always-awake blocks at thresholds 1, 2 and 3;
    // the two-panel completion reassigns the zeppelin onto its retreat net.
    for (block, threshold) in [(24, 1), (25, 2), (62, 3)] {
        assert_eq!(
            sites_of(&blocks, block, "INACTIVE_COMPLETION_COUNT"),
            [vec![ZrdValue::Int(threshold)]],
            "OBJECTIVE{block} completes at {threshold} inactive pirate \
             gasbag panel(s)"
        );
        for number in 1..=6 {
            assert_eq!(
                sites_of(&blocks, block, &format!("INACTIVE{number}")),
                [vec![
                    ZrdValue::Text("piratezep".to_owned()),
                    ZrdValue::Text(format!("gasbag{number}")),
                    ZrdValue::Text("panels".to_owned()),
                ]],
                "OBJECTIVE{block} watches the pirate zeppelin's gasbag panel \
                 {number} through its chained member name"
            );
        }
    }
    assert_eq!(
        sites_of(&blocks, 25, "SET_AI_NET"),
        [vec![ZrdValue::List(vec![
            ZrdValue::Text("piratezep".to_owned()),
            ZrdValue::Text("M4PZRetreat".to_owned()),
        ])]],
        "the two-panel completion points the pirate zeppelin at its retreat \
         net — the spelled capital retreat transition"
    );

    // The final capital, the Black Swan: woken with its turrets in block 6,
    // reinforced and re-marked by blocks 27 and 46 — and never an objective
    // target anywhere in the record (the eligibility test below pins that).
    assert_eq!(
        sites_of(&blocks, 6, "WAKEUP_ENEMIES"),
        [vec![ZrdValue::Text("blackswanzep".to_owned())]],
        "OBJECTIVE6 wakes the Black Swan"
    );
    assert_eq!(
        sites_of(&blocks, 6, "WAKEUP_ZEP_TURRETS"),
        [vec![ZrdValue::Text("blackswanzep".to_owned())]],
        "and its turrets, in the same block"
    );
    assert_eq!(
        sites_of(&blocks, 27, "WAKEUP_ENEMIES"),
        [vec![
            ZrdValue::Text("blackswanzep".to_owned()),
            ZrdValue::Text("bsfury_5_1".to_owned()),
            ZrdValue::Text("bsfury_5_2".to_owned()),
            ZrdValue::Text("bsfury_5_3".to_owned()),
        ]],
        "OBJECTIVE27 wakes the Black Swan's three Furies beside it"
    );
    assert_eq!(
        sites_of(&blocks, 46, "SET_AI_NET"),
        [vec![
            ZrdValue::List(vec![
                ZrdValue::Text("bsfury_5_1".to_owned()),
                ZrdValue::Text("M4Miles".to_owned()),
            ]),
            ZrdValue::List(vec![
                ZrdValue::Text("bsfury_5_2".to_owned()),
                ZrdValue::Text("M4Miles".to_owned()),
            ]),
            ZrdValue::List(vec![
                ZrdValue::Text("bsfury_5_3".to_owned()),
                ZrdValue::Text("M4Miles".to_owned()),
            ]),
        ]],
        "OBJECTIVE46 points the three Furies at the Miles net — a two-second \
         nap after OBJECTIVE27"
    );

    // The declaration half: the three zeppelin names the chain directs are
    // declared by this archive's own zeppelin member, and the five nets the
    // record assigns are declared by the chapter-5 world container.
    let zeppelin_texts = texts_in_container(CONTAINER);
    let zeppelin_member = binding
        .members
        .iter()
        .find(|row| row.name.eq_ignore_ascii_case(ZEPPELINS_MEMBER))
        .expect("the archive offers its zeppelin member");
    assert_eq!(
        (zeppelin_member.offset, zeppelin_member.len),
        (46_864, 7_535),
        "the zeppelin member's span changed"
    );
    let dzones_member = binding
        .members
        .iter()
        .find(|row| row.name.eq_ignore_ascii_case(DZONES_MEMBER))
        .expect("the archive offers its detection-zone member");
    assert_eq!(
        (dzones_member.offset, dzones_member.len),
        (18_561, 826),
        "the detection-zone member's span changed"
    );
    for name in ["dantezep", "piratezep", "blackswanzep"] {
        assert!(
            zeppelin_texts.contains(name),
            "{name} is directed by the control record but not declared \
             anywhere in this archive's members"
        );
    }
    let chapter_texts = texts_in_container(CHAPTER_CONTAINER);
    for net in [
        "M4Attack",
        "M4Defend",
        "M4PZRetreat",
        "M4Miles",
        "M4MilesRun",
    ] {
        assert!(
            chapter_texts.contains(net),
            "{net} is assigned by the control record but the chapter-5 world \
             container declares no such node"
        );
    }

    // The battle map's five danger zones are declared by this archive's own
    // detection-zone member and carried as nodes by the chapter container,
    // through the production trigger-volume survey.
    let survey = zone_survey();
    assert!(survey.zone_declarations_are_decoded());
    let declaration = survey
        .declarations()
        .iter()
        .find(|declaration| declaration.mission() == MISSION)
        .expect("M24's dzones.zrd decoded into a declaration");
    assert_eq!(declaration.member_container(), "zbd/c5/m04/zrdr.zbd");
    assert_eq!(
        declaration.member_container_sha256(),
        CONTAINER_SHA256,
        "the survey's container digest is the census's"
    );
    assert_eq!(
        declaration.member_span(),
        (18_561, 826),
        "the survey's member span is the census's dzones.zrd span"
    );
    let c5 = WorldId::from_key("c5").expect("c5 is a world key");
    let nodes: std::collections::BTreeSet<String> = survey
        .volumes_in(&c5)
        .iter()
        .map(|volume| volume.zone().to_owned())
        .collect();
    let spelled_zones: std::collections::BTreeSet<&str> = blocks
        .iter()
        .flat_map(|block| block.sites.iter())
        .filter(|(key, _)| key == "DANGER_ZONES_COMPLETED")
        .flat_map(|(_, args)| texts(args))
        .collect();
    assert_eq!(
        spelled_zones,
        std::collections::BTreeSet::from([
            "dzpath28", "dzpath29", "dzpath30", "dzpath31", "dzpath32"
        ]),
        "the five zones the danger-zone chain spells changed"
    );
    for zone in &spelled_zones {
        assert!(
            nodes.contains(*zone),
            "the control record spells {zone}, which the chapter container \
             carries no node for"
        );
        assert!(
            declaration
                .named_zones()
                .iter()
                .any(|(_, name)| *name == *zone),
            "{zone} is spelled by the control record but not declared by M24's \
             dzones.zrd"
        );
    }
    let gaps: Vec<_> = survey
        .declaration_gaps()
        .into_iter()
        .filter(|gap| gap.mission == MISSION)
        .collect();
    assert!(
        gaps.is_empty(),
        "M24 names a zone its world container has no node for: {gaps:?}"
    );
    // Whether the wrong actor, the wrong session or a repeated event can
    // satisfy any of these transitions is runtime behaviour: nothing here
    // simulates it, and the campaign gate stays shut while any row carries a
    // gap.
    assert!(
        !census().campaign_ready(),
        "M24's transitions stay unobserved: no campaign runtime may start \
         while the campaign gate is closed"
    );
}

/// **The sheet's target-eligibility priority is five flag writes and no
/// add: the record only ever retires eligibility.**
///
/// `REMOVE_OBJECTIVE_TARGET` clears the objective-target flag of `dantezep`
/// (the falling friendly capital) and of the chained `[piratezep,
/// rock_zeppelin]`; `ADD_OTHER_TARGET` sets the other-target flag of that
/// same chain and of `blackswanzep` twice. No `ADD_OBJECTIVE_TARGET` site
/// exists anywhere in M24's 267 sites, so the initial target set is not
/// this member's data — the eligibility change this mission spells is a
/// one-way decrease, exactly where the sheet's priority points. What the
/// target-info layer draws from the flag is the disposition's own recorded
/// unknown and stays untraced.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m24_b_the_target_eligibility_changes_are_five_writes_and_the_record_never_adds_a_target()
{
    let binding = control_binding();
    let (document, _) = control_document();
    let blocks = blocks_of(&document);
    let record = &binding.record;

    assert!(
        record.key("ADD_OBJECTIVE_TARGET").is_none(),
        "M24 spells no ADD_OBJECTIVE_TARGET site: nothing in this record can \
         create an objective target"
    );

    // The two retirements, pinned with their operands and blocks.
    let removes: Vec<(u32, Vec<ZrdValue>)> = blocks
        .iter()
        .flat_map(|block| {
            block
                .sites
                .iter()
                .filter(|(key, _)| key == "REMOVE_OBJECTIVE_TARGET")
                .map(|(_, args)| (number_of(block), args.clone()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(
        removes,
        [
            (10, vec![ZrdValue::Text("dantezep".to_owned())]),
            (
                20,
                vec![ZrdValue::List(vec![
                    ZrdValue::Text("piratezep".to_owned()),
                    ZrdValue::Text("rock_zeppelin".to_owned()),
                ])],
            ),
        ],
        "the two objective-target retirements, as spelled"
    );
    assert!(
        matches!(
            record
                .key("REMOVE_OBJECTIVE_TARGET")
                .expect("M24 spells the key")
                .disposition(),
            DirectiveDisposition::Measured(directive)
                if matches!(
                    directive.operation,
                    DirectiveOperation::SetTargetFlag {
                        objective: true,
                        set: false
                    }
                )
        ),
        "the retirement clears the +0x4d objective-target flag of the leaf \
         the name chain resolves to"
    );

    // The three other-target marks, pinned with their operands and blocks —
    // the chain again, and the final capital twice.
    let adds: Vec<(u32, Vec<ZrdValue>)> = blocks
        .iter()
        .flat_map(|block| {
            block
                .sites
                .iter()
                .filter(|(key, _)| key == "ADD_OTHER_TARGET")
                .map(|(_, args)| (number_of(block), args.clone()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(
        adds,
        [
            (
                20,
                vec![ZrdValue::List(vec![
                    ZrdValue::Text("piratezep".to_owned()),
                    ZrdValue::Text("rock_zeppelin".to_owned()),
                ])],
            ),
            (23, vec![ZrdValue::Text("blackswanzep".to_owned())]),
            (46, vec![ZrdValue::Text("blackswanzep".to_owned())]),
        ],
        "the three other-target marks, as spelled"
    );
    assert!(
        matches!(
            record
                .key("ADD_OTHER_TARGET")
                .expect("M24 spells the key")
                .disposition(),
            DirectiveDisposition::Measured(directive)
                if matches!(
                    directive.operation,
                    DirectiveOperation::SetTargetFlag {
                        objective: false,
                        set: true
                    }
                )
        ),
        "the mark sets the +0x4c other-target flag of the leaf the name \
         chain resolves to"
    );

    // The eligibility swap is one block: OBJECTIVE20 retires the chain and
    // marks it as an other target in the same completion, gated on its own
    // enemy-group depletion, and it is the block OBJECTIVE6's wake reaches.
    assert_eq!(
        sites_of(&blocks, 20, "DEDG"),
        [vec![ZrdValue::Int(1), ZrdValue::Int(0)]],
        "OBJECTIVE20 completes when enemy group 1 is empty — the gate beside \
         the swap"
    );
    assert_eq!(
        sites_of(&blocks, 20, "IDENTITY"),
        [vec![
            ZrdValue::Text("SECONDARY".to_owned()),
            ZrdValue::Int(12)
        ]],
        "the swap carries the record's second secondary identity"
    );
    // The two blackswanzep marks sit on their own waking edges: block 23 is
    // woken by both the Black Swan opener (6) and its reinforcement (27),
    // and block 46 is a two-second nap after 27.
    assert_eq!(
        sites_of(&blocks, 23, "BEGIN_DORMANT"),
        [vec![ZrdValue::Float(-1.0)]],
        "OBJECTIVE23 starts dormant with no timed wake — only its two waking \
         edges can arm it"
    );
    assert_eq!(
        sites_of(&blocks, 46, "BEGIN_DORMANT"),
        [vec![ZrdValue::Float(-1.0)]],
        "OBJECTIVE46 starts dormant too"
    );
    assert!(
        !census().campaign_ready(),
        "the eligibility flags stay unobserved at runtime: the campaign gate \
         is closed"
    );
}

/// **The sheet's campaign-ending priority is one win and one loss latch,
/// both nap-armed, with the record's own measured arming chains — and the
/// campaign has no successor to the win.**
///
/// `INSTANTWIN` lives in OBJECTIVE21 alone and `INSTANTLOSS` in OBJECTIVE39
/// alone; each starts dormant with no timed wake. The win latch's only
/// incoming edge is a 45-second nap from OBJECTIVE28, whose own entry runs
/// back through the record's wake chain to OBJECTIVE10 — the friendly
/// capital's fall. The loss latch is armed by two naps (15 seconds from
/// OBJECTIVE38, 20 from OBJECTIVE62) and is *killed* by OBJECTIVE34 — the
/// mid-battle detour gate that OBJECTIVE38 and OBJECTIVE62 in turn kill, so
/// the record spells a mutual exclusion between the detour and the failure
/// path. Whether both latches can be armed in one window, and which wins, is
/// the precedence the contract requires measuring — a runtime question left
/// open for M24-C. M24 is the campaign's last row, so nothing follows the
/// win in the record or the layout.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m24_b_the_campaign_ending_is_one_win_and_one_loss_latch_with_measured_arming_chains() {
    let (document, _) = control_document();
    let blocks = blocks_of(&document);

    let incoming = |target: u32| -> Vec<(u32, &'static str, Vec<ZrdValue>)> {
        blocks
            .iter()
            .flat_map(|block| {
                block
                    .sites
                    .iter()
                    .filter(|(key, args)| {
                        (key == "WAKE_OBJECTIVE_WHEN_I_COMPLETE"
                            || key == "NAP_OBJECTIVE_WHEN_I_COMPLETE"
                            || key == "KILL_OBJECTIVE_WHEN_I_COMPLETE")
                            && integers(args).contains(&i64::from(target))
                    })
                    .map(|(key, args)| {
                        let kind: &'static str = match key.as_str() {
                            "WAKE_OBJECTIVE_WHEN_I_COMPLETE" => "wake",
                            "NAP_OBJECTIVE_WHEN_I_COMPLETE" => "nap",
                            _ => "kill",
                        };
                        (number_of(block), kind, args.clone())
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    };

    // The win latch: one identity, one arming, one nap.
    assert_eq!(
        sites_of(&blocks, 21, "IDENTITY"),
        [vec![
            ZrdValue::Text("SECONDARY".to_owned()),
            ZrdValue::Int(11)
        ]],
        "the win latch carries the record's eleventh-slot secondary identity"
    );
    assert_eq!(
        incoming(21),
        [(28, "nap", vec![ZrdValue::Int(21), ZrdValue::Float(45.0)])],
        "OBJECTIVE28's completion is the win latch's only arming: a 45-second nap"
    );

    // The loss latch: two naps arm it, one kill retires it.
    assert_eq!(
        sites_of(&blocks, 39, "BEGIN_DORMANT"),
        [vec![ZrdValue::Float(-1.0)]],
        "the loss latch starts dormant with no timed wake"
    );
    let mut loss = incoming(39);
    loss.sort_by_key(|(block, kind, _)| (*block, *kind));
    assert_eq!(
        loss,
        [
            (34, "kill", vec![ZrdValue::Int(38), ZrdValue::Int(39)]),
            (38, "nap", vec![ZrdValue::Int(39), ZrdValue::Float(15.0)]),
            (62, "nap", vec![ZrdValue::Int(39), ZrdValue::Float(20.0)]),
        ],
        "the loss latch is armed by OBJECTIVE38's 15-second nap and \
         OBJECTIVE62's 20-second nap, and retired by OBJECTIVE34's kill list"
    );

    // The two arming chains, walked backwards over wake and nap edges — the
    // blocks that must have been entered before each latch can be entered.
    let closure = |target: u32| -> Vec<u32> {
        let mut seen: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
        let mut stack = vec![target];
        while let Some(node) = stack.pop() {
            if !seen.insert(node) {
                continue;
            }
            for (block, kind, _) in incoming(node) {
                if kind != "kill" {
                    stack.push(block);
                }
            }
        }
        seen.into_iter().collect()
    };
    assert_eq!(
        closure(28),
        [10, 11, 13, 28, 34, 35, 47],
        "the win chain runs back from OBJECTIVE28 through 13, 35, 47, 34 and \
         11 to OBJECTIVE10 — the friendly capital's fall is its root"
    );
    assert_eq!(
        closure(38),
        [10, 11, 37, 38],
        "OBJECTIVE38's chain is the shorter fall chain: 10, 11, 37"
    );
    assert_eq!(
        closure(62),
        [62],
        "OBJECTIVE62 is always awake — its three-panel condition arms the \
         loss latch with no predecessor at all"
    );

    // The mutual exclusion, as spelled: the detour gate and the two
    // failure-path blocks kill each other's chains.
    assert_eq!(
        sites_of(&blocks, 34, "TRAVELERS"),
        [vec![
            ZrdValue::Text("stihellhound_5_eg0".to_owned()),
            ZrdValue::Text("APPROACHING".to_owned()),
            ZrdValue::List(vec![
                ZrdValue::Float(-2675.9),
                ZrdValue::Float(200.0),
                ZrdValue::Float(-14920.9),
            ]),
            ZrdValue::Float(2000.0),
            ZrdValue::Int(1),
        ]],
        "OBJECTIVE34's gate is the escorted-hound closing on the spelled point"
    );
    assert_eq!(
        sites_of(&blocks, 38, "KILL_OBJECTIVE_WHEN_I_COMPLETE"),
        [vec![ZrdValue::Int(34), ZrdValue::Int(35)]],
        "the failure chain retires the detour gate and its target block"
    );
    assert_eq!(
        sites_of(&blocks, 62, "KILL_OBJECTIVE_WHEN_I_COMPLETE"),
        [vec![ZrdValue::Int(34), ZrdValue::Int(35)]],
        "and so does the three-panel completion — the same exclusion from \
         the pirate-zeppelin path"
    );

    // The five identity sites of the record: two primary objectives, three
    // secondary — and the two latches are secondary-classed blocks.
    let identities: Vec<(u32, Vec<ZrdValue>)> = blocks
        .iter()
        .flat_map(|block| {
            block
                .sites
                .iter()
                .filter(|(key, _)| key == "IDENTITY")
                .map(|(_, args)| (number_of(block), args.clone()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(
        identities
            .iter()
            .map(|(block, args)| (*block, args.first().cloned()))
            .collect::<Vec<_>>(),
        [
            (10, Some(ZrdValue::Text("PRIMARY".to_owned()))),
            (13, Some(ZrdValue::Text("PRIMARY".to_owned()))),
            (20, Some(ZrdValue::Text("SECONDARY".to_owned()))),
            (21, Some(ZrdValue::Text("SECONDARY".to_owned()))),
            (53, Some(ZrdValue::Text("SECONDARY".to_owned()))),
        ],
        "M24's five identity sites: two primary objectives and three secondary"
    );

    // And the campaign has nothing after the win: M24 is the last row, so
    // the ending's continuity — profile, records, the successor step — is
    // not this member's data and not this stage's evidence (M24-C).
    let campaign = context().campaign();
    let position = context()
        .bind(m24(), &m24_title())
        .expect("M24's mission binding resolves")
        .campaign_position
        .expect("a position was resolved");
    assert_eq!(
        position,
        campaign.len() - 1,
        "M24 is the campaign's final row — no successor mission follows the \
         ending"
    );
}

// ---------------------------------------------------------------------------
// Retail: the record does not lower, by two named gaps
// ---------------------------------------------------------------------------

/// **M24's record does not lower, and the refusal is exactly seven named
/// calls of two kinds.** All 63 conditions lower and 260 of 267 calls bind,
/// yet `STOP_QUEUED_SOUNDS` never registers: its measured shapes include a
/// 9-name list, and a signature past `MAX_CALL_ARGS` makes the whole spec
/// unfit — "accepting a name means accepting every measured shape it was
/// registered with" — so all six sites refuse `unknown host call`, including
/// the one-name site. The truncated `SET_AI_` refuses the same way, because
/// an unmeasured key registers nothing by design. No program is assembled,
/// `validation` never runs and the only unmet row is `call_arguments`; the
/// mission is not complete. Carrying the name list as one list argument is
/// the follow-up the findings name — the same one M16-B names.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m24_b_m24s_record_does_not_lower_and_its_calls_refuse_by_two_named_gaps() {
    let row = census_row();
    let lowered = row
        .lowering_attempt()
        .expect("the census lowers M24's measured record");
    let attempt = lowered.attempt();

    assert_eq!(
        attempt.mission.as_ref().map(String::as_str),
        Ok("mission/ch5-m04"),
        "the lowered program carries a different mission id"
    );
    assert_eq!(
        attempt.objectives, BLOCKS,
        "one objective per numbered block"
    );
    assert_eq!(attempt.conditions.len(), BLOCKS as usize);
    assert!(
        attempt
            .conditions
            .iter()
            .all(|outcome| matches!(outcome, ConditionOutcome::Lowered)),
        "a condition refused: {:?}",
        attempt.conditions
    );
    assert_eq!(attempt.calls.len(), SITES as usize, "one call per site");

    // The refused calls, pinned by their flat site index — the record order
    // the lowering walks: six sound-cleanup sites and the one truncated key.
    let refused: Vec<(usize, &str)> = attempt
        .calls
        .iter()
        .enumerate()
        .filter_map(|(index, call)| match call {
            CallOutcome::Bound => None,
            CallOutcome::Refused(text) => Some((index, text.as_str())),
        })
        .collect();
    assert_eq!(
        refused.len(),
        REFUSED,
        "exactly {REFUSED} of M24's {SITES} calls refuse: {refused:?}"
    );
    for (index, _) in &refused {
        assert!(
            refused_sites().contains(index),
            "site {index} refused but is not one of the six sound sites or \
             the truncated key: {refused:?}"
        );
    }
    for (index, text) in &refused {
        let key = if *index == 109 {
            "SET_AI_"
        } else {
            "STOP_QUEUED_SOUNDS"
        };
        assert!(
            text.contains(&format!(
                "mission/ch5-m04 objective#{} call {}: unknown host call `{key}`",
                refused_block(*index) - 1,
                refused_within_block(*index)
            )),
            "{text}"
        );
    }
    assert_eq!(
        attempt.unbound_keys,
        ["`STOP_QUEUED_SOUNDS`: binding `STOP_QUEUED_SOUNDS`: too many arguments"],
        "the only registration refusal is the sound key's oversized signature \
         — the truncated SET_AI_ was never registered, so it names no \
         binding error"
    );
    assert!(
        lowered.program().is_none(),
        "refused calls mean no program is assembled — validation never runs"
    );
    assert!(
        attempt.validation.is_none(),
        "no program, so no validation verdict"
    );

    let lowering = row.lowering().expect("the accounting is derived");
    let unmet: Vec<String> = lowering
        .unmet()
        .map(|row| row.kind.code().to_owned())
        .collect();
    assert_eq!(
        unmet,
        ["call_arguments"],
        "the one unmet row is the call binding — the conditions all lowered"
    );
    assert!(!row.is_complete());
    assert!(!lowering.complete());
    assert!(!row.is_complete());

    // The poison is one site: OBJECTIVE10's nine-name cleanup. Its nine
    // names exceed `MAX_CALL_ARGS`, and the bound is the signature's
    // argument count, not the key's — the one-name site of OBJECTIVE5
    // refuses because the whole spec is gone.
    let (document, _) = control_document();
    let blocks = blocks_of(&document);
    let name_counts: Vec<(u32, usize)> = blocks
        .iter()
        .flat_map(|block| {
            block
                .sites
                .iter()
                .filter(|(key, _)| key == "STOP_QUEUED_SOUNDS")
                .map(|(_, args)| (number_of(block), texts(args).len()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(
        name_counts,
        [(5, 1), (10, 9), (13, 7), (38, 8), (59, 7), (62, 8)],
        "the six sound-cleanup sites and their name counts — the 9-name site \
         is the one past the bound"
    );
}

/// The flat site indices of the seven refused calls, as the record order
/// spells them: six `STOP_QUEUED_SOUNDS` sites and the truncated key.
fn refused_sites() -> Vec<usize> {
    vec![22, 74, 92, 109, 183, 246, 263]
}

/// The numbered block a flat site index sits in, from the record's own
/// site accounting.
fn refused_block(flat: usize) -> u32 {
    refused_locations()
        .into_iter()
        .find(|(index, _, _)| *index == flat)
        .unwrap_or_else(|| panic!("site {flat} is one of the refused sites"))
        .1
}

/// A refused site's zero-based position inside its own block.
fn refused_within_block(flat: usize) -> u32 {
    refused_locations()
        .into_iter()
        .find(|(index, _, _)| *index == flat)
        .unwrap_or_else(|| panic!("site {flat} is one of the refused sites"))
        .2
}

/// `(flat index, block number, position in block)` of every refused site,
/// re-derived from the independent document walk so the pinned flat indices
/// above cannot drift from the record they describe.
fn refused_locations() -> Vec<(usize, u32, u32)> {
    let (document, _) = control_document();
    let blocks = blocks_of(&document);
    let mut flat = 0;
    let mut found = Vec::new();
    for block in &blocks {
        for (position, (key, _)) in block.sites.iter().enumerate() {
            if key == "STOP_QUEUED_SOUNDS" || key == "SET_AI_" {
                found.push((flat, number_of(block), position as u32));
            }
            flat += 1;
        }
    }
    found
}

/// **M24 is not campaign-ready and the census does not hide it.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m24_b_m24_stays_unready_while_its_sound_cleanup_and_truncated_key_refuse() {
    let census = census();
    assert!(!census.complete_missions().contains(&MISSION));
    assert!(!census.campaign_ready());
    assert!(census.measured_rows().any(|row| row.mission() == MISSION));
    let unmet = census.unmet_by_requirement();
    assert!(
        unmet
            .get("call_arguments")
            .is_some_and(|missions| missions.iter().any(|mission| mission == MISSION)),
        "the census reports M24 under call_arguments: {unmet:?}"
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
            "M24 must not appear under {requirement}: its conditions all lower \
             and its identity resolves — {unmet:?}"
        );
    }
    // The campaign's last row being incomplete is exactly why the gate
    // cannot open: M24-A left every other cell unknown, and this stage adds
    // one measured gap beside them.
    let binding = context()
        .bind(m24(), &m24_title())
        .expect("M24's mission binding resolves");
    assert!(
        !binding.is_verified(),
        "the mission binding stays unverified while the checklist entries \
         M24-A left unknown are still unknown"
    );
}

// ---------------------------------------------------------------------------
// Engine image: the truncated SET_AI_ key is not a directive key
// ---------------------------------------------------------------------------

/// The directive-key string table's file extent in the owner's decrypted
/// executable — the range `docs/findings/2026-10-06-m01-lc-directive-a-…`
/// measured (VA `0x626040..0x6268c0`; for `.data`, RVA equals file offset).
const DIRECTIVE_TABLE: (usize, usize) = (0x226040, 0x2268c0);

/// **The truncated `SET_AI_` spelling is not in the directive-key table the
/// original parser looks names up in — so the original ignores it, and the
/// record's unmeasured disposition is the faithful one.**
///
/// The M01-LC-A finding measured both facts this test combines: the
/// parser's `0x57a090(record, "KEY")` lookups mean any key it does not look
/// up is ignored, and the table's string extent is `0x226040..0x2268c0`.
/// M16-B proved the same rule for its four comment words; this test proves
/// it for M24's one truncated site — `SET_AI_`, whose five-pair list is the
/// shape `SET_AI_NET` spells, so the record carries faithful data under a
/// name the shipped parser cannot resolve.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m24_b_the_truncated_set_ai_key_is_not_in_the_measured_directive_table() {
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
    assert_eq!(
        entries.len(),
        87,
        "the measured table carries its 87 strings, the last truncated at the \
         extent's end"
    );

    // Every key of M24's vocabulary this table carries is present — the
    // completion-effect family the parser looks up here, including the two
    // keys M24 spells that M16 did not.
    for key in [
        "ADD_OTHER_TARGET",
        "BEGIN_DORMANT",
        "COMPLETED_SOUND_GROUP",
        "DEDG",
        "IDENTITY",
        "INSTANTLOSS",
        "INSTANTWIN",
        "KILL_OBJECTIVE_WHEN_I_COMPLETE",
        "REMOVE_OBJECTIVE_TARGET",
        "SET_AI_NET",
        "SET_AI_TEAM",
        "SET_HELP_LABEL",
        "STOP_QUEUED_SOUNDS",
        "TRAVELERS",
        "WAKEUP_ENEMIES",
        "WAKEUP_ZEP_TURRETS",
        "WAKE_ANIM",
        "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
    ] {
        assert!(
            entries.contains(&key),
            "{key} must be a string in the measured directive table"
        );
    }
    let nap_start = file_offset_of(&image.bytes, b"NAP_OBJECTIVE_WHEN_I_COMPLETE\0")
        .expect("the nap key's string is in the image");
    assert_eq!(
        nap_start, 0x2268b0,
        "the nap key sits at the extent's end, as the finding measured"
    );

    // The truncated spelling is absent from the table — the original's
    // lookup cannot find it, so under the measured "unlooked-up keys are
    // ignored" rule it is inert text the shipped record carries.
    assert!(
        !entries.contains(&"SET_AI_"),
        "the truncated SET_AI_ must not be a directive-table entry"
    );
    assert!(
        standalone_offsets(&image.bytes, b"SET_AI_").is_empty(),
        "SET_AI_ is not a standalone string anywhere in the image — only as \
         the prefix of the longer SET_AI_NET / SET_AI_TEAM / SET_AI_ATTACK_ \
         spellings"
    );
    for longer in [&b"SET_AI_NET\0"[..], b"SET_AI_TEAM\0".as_slice()] {
        assert!(
            file_offset_of(&image.bytes, longer).is_some(),
            "{longer:?} must be in the image — the truncated spelling differs \
             from the measured keys by exactly its missing tail"
        );
    }
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
// Synthetic: the refusal mechanisms the retail record leans on
// ---------------------------------------------------------------------------

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

/// One authored numbered block: its `OBJECTIVE<N>` field and its sites.
fn block(number: u32, directives: Vec<Vec<ZrdValue>>) -> (String, ZrdValue) {
    let mut children = Vec::new();
    for site in directives {
        children.extend(site);
    }
    (format!("OBJECTIVE{number}"), ZrdValue::List(children))
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

/// Lowers one authored document through the production lowering, against
/// the mission identity this stage binds.
fn lower(document: &ZrdValue) -> cs_app::control_lowering::LoweredControlRecord {
    let record = measure_control_record(document);
    lower_control_record(
        ContentId::from_source(ContentKind::Mission, "mission-ch5-m04")
            .map_err(|error| error.to_string()),
        "mission-ch5-m04",
        document,
        &record,
    )
}

/// **M24's own site sizes, in miniature: one oversized sound site poisons
/// the whole key, and eight names is the bound.**
///
/// The retail record spells six `STOP_QUEUED_SOUNDS` sites with 1, 9, 7, 8,
/// 7 and 8 names; this authors exactly those shapes and pins both halves of
/// the refusal: at the retail sizes every site refuses, and at the same
/// sizes with the 9-name site reduced to 8 the whole key registers and every
/// site binds. The bound is the signature's argument count, not the key.
#[test]
fn accept_m24_b_one_oversized_sound_site_poisons_the_whole_sound_key() {
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

    // M24's shape, exactly: the six retail name counts.
    let document = control_record(sound_sites(&[1, 9, 7, 8, 7, 8]));
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
    assert_eq!(refused.len(), 6, "all six sound sites refuse: {refused:?}");
    assert!(
        refused
            .iter()
            .all(|text| text.contains("unknown host call `STOP_QUEUED_SOUNDS`")),
        " the one-name site refuses too — the key itself is gone: {refused:?}"
    );
    assert!(lowered.program().is_none(), "no program assembles");

    // The bound is the signature's argument count, not the key: the same
    // six sites with the 9-name list at eight register and bind.
    let document = control_record(sound_sites(&[1, 8, 7, 8, 7, 8]));
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

/// **The truncated `SET_AI_` site refuses like any unknown call, and the
/// same pairs under the measured `SET_AI_NET` key bind — beside a
/// two-latch, nap-armed program that validates clean.**
///
/// OBJECTIVE19's site is a truncated spelling of a net assignment: the same
/// `{actor, net}` pairs the record's five measured `SET_AI_NET` sites carry.
/// This authors both shapes and pins the refusal arm (the truncated name
/// registers nothing, the site refuses, no program assembles) and the
/// binding arm (the measured key emits the pairs field for field).
#[test]
fn accept_m24_b_the_truncated_set_ai_site_refuses_but_the_measured_net_site_binds() {
    let pairs = |key: &str| {
        control_record(vec![block(
            19,
            vec![
                directive("BEGIN_DORMANT", vec![zrd_float(-1.0)]),
                directive(
                    key,
                    vec![
                        zrd_list(vec![zrd_text("devastator_1"), zrd_text("M4Defend")]),
                        zrd_list(vec![zrd_text("wingman_1"), zrd_text("M4Defend")]),
                    ],
                ),
            ],
        )])
    };

    // The retail shape: the truncated key refuses as an unknown call.
    let document = pairs("SET_AI_");
    let record = measure_control_record(&document);
    assert_eq!(
        record
            .unmeasured()
            .into_iter()
            .map(|(key, _)| key.key.as_str())
            .collect::<Vec<_>>(),
        ["SET_AI_"],
        "the truncated spelling is the record's only unmeasured key"
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
    assert_eq!(refused.len(), 1, "the truncated site refuses: {refused:?}");
    assert!(
        refused[0].contains("unknown host call `SET_AI_`"),
        "{}",
        refused[0]
    );
    assert!(
        attempt.unbound_keys.is_empty(),
        "an unmeasured key registers nothing, so it names no binding error: \
         {:?}",
        attempt.unbound_keys
    );
    assert!(lowered.program().is_none(), "no program assembles");

    // The measured key: the same pairs bind and arrive as spelled.
    let document = pairs("SET_AI_NET");
    let lowered = lower(&document);
    let attempt = lowered.attempt();
    assert!(
        attempt
            .calls
            .iter()
            .all(|call| matches!(call, CallOutcome::Bound)),
        "the measured net site binds: {:?}",
        attempt.calls
    );
    assert_eq!(
        attempt.validation,
        Some(Vec::new()),
        "the one-block net record validates clean"
    );
    let raw = lowered.raw_program().expect("the program assembled");
    let net = raw.objectives[0]
        .calls
        .iter()
        .find(|call| call.name == "SET_AI_NET")
        .expect("the net assignment is emitted");
    assert_eq!(
        net.args.as_slice(),
        // The measured `AssignNet` family takes the spelled list as one
        // argument (M03-B-FU2 #810), so the pair array arrives as a single
        // `Value::List` — the shape that keeps a long site inside the
        // host-call bound without raising it.
        [Value::List(vec![
            Value::List(vec![
                Value::Str("devastator_1".to_owned()),
                Value::Str("M4Defend".to_owned()),
            ]),
            Value::List(vec![
                Value::Str("wingman_1".to_owned()),
                Value::Str("M4Defend".to_owned()),
            ]),
        ])],
        "the two actor/net pairs arrive inside the one spelled list argument"
    );
}
