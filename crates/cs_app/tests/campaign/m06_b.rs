//! Acceptance stage M06-B: the mission-specific compatibility gaps of the
//! sixth mission (`missions/M06.md`, work order `M06-B`).
//!
//! M06-A bound M06's identities and left its program unmeasured. The
//! machinery that measures a control program (the `.zrd` reader, the census,
//! the directive dispositions, the record → `RawProgram` adapter and the
//! binding `SourceContext::control_program` adds on top of them) is shared
//! and was built for M01/M02; this stage runs all of it over M06's **own**
//! reader archive and pins what is *different* at M06, so the gaps are
//! recorded as measurements instead of surfacing later as a silent failure:
//!
//! * M06's control program is `objectives.zrd` with **82** numbered blocks
//!   and **265** directive sites across **26** keys, selected by the content
//!   rule and never by its name;
//! * every key has a disposition: two terminal outcomes and 24 measured
//!   effects, **no** unmeasured key, no refusal and no unclassified
//!   record-level key;
//! * the sheet's three regression priorities are located in the record with
//!   the actors the original spells — subsystem disablement (the eight
//!   `g_engine*` in-play parts and the threshold that clears 4/1/2/3 of them,
//!   beside the barge and gate chains), multi-step interaction (the
//!   target-flag chain that walks `propane` → `sprucegoose` →
//!   `tugandbarge01..04`, the 35 nap re-wakes and the `SET_AI_NET`
//!   re-pointings) and passenger identity, whose **only expression in the
//!   whole archive is an unreferenced location node** — no directive, target
//!   or actor names a passenger, so no predicate is assigned here;
//! * both terminal latches are gated and every cross-objective address the
//!   record spells is a block of this record — including the address equal to
//!   the block count, which is what makes the record's addressing measurable
//!   rather than assumed;
//! * **the record lowers completely**: the fifteen
//!   `KILL_OBJECTIVE_WHEN_I_COMPLETE` sites (two of them twelve targets wide)
//!   bind because M02-B-FU1 (#800) reshaped list-taking directives, and the
//!   three `ANIM_STATE` sites that spell a `COMPLETION_COUNT` override with
//!   two descriptors lower because M04-B-FU1 (#806) generalized the same
//!   mechanism to the operand list an `ANIM_STATE` evaluator reads — every
//!   pair appends and the in-list count overwrites `required`. All 265 sites
//!   bind, all 82 conditions lower and `MissionProgram::validate` accepts.
//!
//! No behaviour is invented here. The runtime halves of the sheet's
//! priorities — the wrong actor, the wrong session, a repeated event — need
//! ordinary play (M06-C) and stay open; nothing in this file simulates them.
//! The lowering gap this stage recorded and the shared fix that closed it
//! are in `docs/findings/2026-10-09-m06-b-compatibility-gaps.md` and
//! `docs/findings/2026-10-09-m04-b-fu1-anim-state-operand-list.md`.
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`; the two
//! synthetic tests run in CI.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_app::control_lowering::{LoweredControlRecord, lower_control_record};
use cs_app::mission_control::{
    RetailControlCensus, read_control_member, survey_mission_control_programs,
};
use cs_assets::install::sha256;
use cs_content::campaign_bindings::{
    MissionControlBinding, MissionLabel, SourceBinding, SourceContext,
};
use cs_content::mission_control::{
    CallOutcome, ConditionOutcome, DirectiveDisposition, DirectiveOperation, DirectiveShape,
    TerminalOutcome, measure_control_record,
};
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{ZrdValue, objective_record, zrd_flat_fields};
use cs_script::bindings::{ArgDomain, MAX_CALL_ARGS};
use cs_script::ir::{AnimationState, Condition};
use cs_types::content::{ContentId, ContentKind};

use crate::common::load_inventory;

/// The census row label of the mission.
pub(crate) const MISSION: &str = "zbd/c2/m01";

/// The reader archive M06-A bound as the mission's program.
const CONTAINER: &str = "ZBD/C2/M01/zrdr.zbd";
/// SHA-256 of that whole archive, from production discovery.
const CONTAINER_SHA256: &str = "d6e9315d580a0570ae8744d2c1b154c4c1af9086bf45cc2fcc125f0921fd9f63";
/// The member the block-carrying rule picks (never a filename constant).
const CONTROL_MEMBER: &str = "objectives.zrd";
/// SHA-256 of the control member's own bytes.
const CONTROL_SHA256: &str = "3b315798547528764afe5ae6c0370ba7f99ba946f386ffce2682eb7d353b534c";

/// The numbered blocks of the control member.
const BLOCKS: u32 = 82;
/// The directive sites of the control member.
const SITES: u32 = 265;
/// The distinct directive keys of the control member.
const KEYS: usize = 26;

/// The four named locations `location.zrd` spells, in member order. The second
/// is the archive's only passenger-named string.
const LOCATIONS: [&str; 4] = ["Airport_terminal", "Passenger_hangar", "Crops", "Coast"];

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M06-B needs the retail capability; run this suite with \
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

/// The production control-program binding, built once for the whole suite.
fn control_binding() -> &'static MissionControlBinding {
    static BINDING: OnceLock<MissionControlBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        let inventory = load_inventory();
        let title = inventory
            .iter()
            .find(|(label, _)| label.as_str() == "M06")
            .map(|(_, title)| title.clone())
            .expect("the declared inventory has an M06 work order");
        let context = SourceContext::read(&game_dir())
            .expect("production source context reads the installation");
        context
            .control_program(
                MissionLabel::new("M06").expect("M06 is a valid label"),
                &title,
            )
            .expect("M06's control program binds through the measured rule")
    })
}

/// The M06 mission binding M06-A derives, built once — this stage consumes its
/// unknowns and adds no second evidence for the join itself.
fn mission_binding() -> &'static SourceBinding {
    static BINDING: OnceLock<SourceBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        let inventory = load_inventory();
        let title = inventory
            .iter()
            .find(|(label, _)| label.as_str() == "M06")
            .map(|(_, title)| title.clone())
            .expect("the declared inventory has an M06 work order");
        let context = SourceContext::read(&game_dir())
            .expect("production source context reads the installation");
        context
            .bind(
                MissionLabel::new("M06").expect("M06 is a valid label"),
                &title,
            )
            .expect("M06 binds to the original data")
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
        let children = value.as_list().expect("every M06 block is a list");
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

/// The integer arguments a directive spells.
fn arguments(directive: &Directive) -> Vec<i64> {
    let empty: [ZrdValue; 0] = [];
    directive
        .args
        .as_deref()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|value| match value {
            ZrdValue::Int(int) => Some(i64::from(*int)),
            _ => None,
        })
        .collect()
}

/// The cross-objective **block addresses** a directive spells.
///
/// The measured effects split the two children of a nap: child0 is the
/// targeted block and child1 is the number of seconds after which the target
/// re-wakes
/// (`docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`),
/// so a nap contributes one address while a wake or a kill contributes every
/// integer it spells. Taking the nap's seconds for a range check would report
/// a dangling block the record never addresses.
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

/// One `.zrd` text node.
fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

/// One `.zrd` int node.
fn int(value: u32) -> ZrdValue {
    ZrdValue::Int(value)
}

/// One authored numbered block.
fn block(number: u32, directives: Vec<Vec<ZrdValue>>) -> (String, ZrdValue) {
    let mut children = Vec::new();
    for site in directives {
        children.extend(site);
    }
    (format!("OBJECTIVE{number}"), ZrdValue::List(children))
}

/// One authored directive site: the key plus its argument list.
fn directive(key: &str, args: Vec<ZrdValue>) -> Vec<ZrdValue> {
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
        ContentId::from_source(ContentKind::Mission, "accept-m06-b").map_err(|e| e.to_string()),
        "accept-m06-b",
        document,
        &record,
    )
}

/// Every distinct `.zrd` text node of one archive member, for the location and
/// actor-name checks below: names only, never bytes.
fn member_texts(name: &str) -> Vec<String> {
    let bytes = std::fs::read(game_dir().join(CONTAINER)).expect("the archive reads");
    let relative = cs_types::install::RelativePath::new(&CONTAINER.to_lowercase())
        .expect("the archive path is relative");
    let container_key = relative.logical_key();
    let discovery = cs_formats::script_raw::discover_container(&container_key, &relative, &bytes);
    let mut texts = Vec::new();
    fn walk(value: &ZrdValue, texts: &mut Vec<String>) {
        match value {
            ZrdValue::Text(text) => texts.push(text.clone()),
            ZrdValue::List(children) => {
                for child in children {
                    walk(child, texts);
                }
            }
            _ => {}
        }
    }
    for program in discovery.programs() {
        if program.locator().member() != Some(name) {
            continue;
        }
        let document = cs_content::stunts::decode_zrd(program.bytes())
            .unwrap_or_else(|error| panic!("member {name} decodes: {error}"));
        walk(&document, &mut texts);
    }
    texts.sort();
    texts.dedup();
    texts
}

/// One `targets.zrd` record as authored: its `(key, value)` pairs in order.
fn target_records() -> Vec<Vec<(String, ZrdValue)>> {
    let bytes = std::fs::read(game_dir().join(CONTAINER)).expect("the archive reads");
    let relative = cs_types::install::RelativePath::new(&CONTAINER.to_lowercase())
        .expect("the archive path is relative");
    let container_key = relative.logical_key();
    let discovery = cs_formats::script_raw::discover_container(&container_key, &relative, &bytes);
    for program in discovery.programs() {
        if program.locator().member() != Some("targets.zrd") {
            continue;
        }
        let document =
            cs_content::stunts::decode_zrd(program.bytes()).expect("targets.zrd decodes");
        let mut records = Vec::new();
        let outer = document.as_list().expect("targets.zrd is a list");
        for record in outer {
            let mut pairs = Vec::new();
            for pair in record.as_list().expect("a target record is a list") {
                let pair = pair.as_list().expect("a target field is a pair");
                let key = pair
                    .first()
                    .and_then(ZrdValue::as_text)
                    .expect("a target field starts with its key")
                    .to_owned();
                // A field may be spelled as its key alone (`objective` is, in
                // M06's propane target): no value node follows it.
                let value = pair.get(1).cloned().unwrap_or(ZrdValue::List(Vec::new()));
                pairs.push((key, value));
            }
            records.push(pairs);
        }
        return records;
    }
    panic!("targets.zrd is in {CONTAINER}");
}

// ---------------------------------------------------------------------------
// Retail: what M06's control program is
// ---------------------------------------------------------------------------

/// **The control program is the member that declares the numbered blocks.**
///
/// The member is found by the content rule: of the 15 members of M06's reader
/// archive exactly one declares numbered `OBJECTIVE<N>` blocks. Neither size
/// nor position decides it — the chosen member is the eighth, and `barge.zrd`
/// and `goosepath.zrd` are both longer than it. The blocks and sites the
/// census measures equal an independent walk of the same document, the
/// archive is the program span M06-A bound, and the production control
/// binding reaches the same member, span and digests through its own walk.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m06_b_the_control_program_is_the_member_that_declares_the_blocks() {
    let row = census().row(MISSION).expect("M06 is in the census");
    assert_eq!(row.container, CONTAINER);
    assert_eq!(
        row.container_sha256, CONTAINER_SHA256,
        "the reader archive is the program M06-A bound"
    );
    assert_eq!(row.members.len(), 15);

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
    let longer: Vec<&str> = row
        .members
        .iter()
        .filter(|member| member.len > chosen.len)
        .map(|member| member.name.as_str())
        .collect();
    assert_eq!(
        longer,
        ["barge.zrd", "goosepath.zrd"],
        "size is not the rule: the two longest members are not the control member"
    );

    let record = row.record().expect("M06 has a measured control program");
    assert_eq!((record.blocks(), record.sites()), (BLOCKS, SITES));
    assert_eq!(record.keys().len(), KEYS);

    let (document, member) = read_control_member(&game_dir(), MISSION)
        .expect("the rule finds M06's control member again");
    assert_eq!(member.name, CONTROL_MEMBER);
    assert_eq!(member.objective_blocks, BLOCKS);
    assert!(member.is_control);

    let blocks = blocks_of(&document);
    let numbers: Vec<u32> = blocks.iter().map(|(number, _)| *number).collect();
    assert_eq!(
        numbers,
        (1..=BLOCKS).collect::<Vec<_>>(),
        "numbered 1..=82, no gaps"
    );
    let walked: usize = blocks.iter().map(|(_, directives)| directives.len()).sum();
    assert_eq!(walked as u32, record.sites(), "the independent walk agrees");
    assert!(
        record.refusals().is_empty(),
        "every block is a readable list"
    );
    assert!(
        record.unclassified_record_keys().is_empty(),
        "M06 spells no record-level key outside the measured vocabulary"
    );

    // The production binding and the census must not disagree about the
    // mission, the program, the member or the record.
    let bound = control_binding();
    assert_eq!(bound.mission.as_str(), "mission/ch2-m01");
    assert_eq!(bound.program_id.as_str(), "script/c2-m01-zrdr");
    assert_eq!(bound.program_asset, CONTAINER);
    assert_eq!(bound.program_sha256, row.container_sha256);
    assert_eq!(bound.control_member, CONTROL_MEMBER);
    assert_eq!(
        (bound.record.blocks(), bound.record.sites()),
        (BLOCKS, SITES)
    );
    assert_eq!(bound.record.vocabulary(), KEYS as u32);
    let control_row = bound.control_row().expect("the chosen member has a row");
    assert_eq!(
        (control_row.offset, control_row.len),
        (chosen.offset, chosen.len)
    );

    // The spans and digests re-derive from the archive's own bytes.
    let bytes = std::fs::read(game_dir().join(CONTAINER)).expect("the archive reads");
    assert_eq!(bytes.len() as u64, bound.program_length);
    assert_eq!(sha256(&bytes).to_hex(), CONTAINER_SHA256);
    let start = bound.control_offset as usize;
    let end = start + bound.control_length as usize;
    assert_eq!(
        sha256(&bytes[start..end]).to_hex(),
        CONTROL_SHA256,
        "the control member's digest re-derives from the member's own bytes"
    );
    assert_eq!(bound.control_sha256, CONTROL_SHA256);
}

/// **Every directive key M06 spells has exactly one disposition, and none is
/// refused.**
///
/// Two keys are terminal outcomes (`INSTANTWIN`, `INSTANTLOSS`, one bare site
/// each), the other 24 have a measured effect, and no key is Unmeasured. The
/// sites are accounted for: the keys' sites sum to the record's, and the
/// sorted vocabulary is exactly the 26 keys the archive spells.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m06_b_every_directive_m06_spells_has_a_disposition_and_none_is_refused() {
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
        "M06's vocabulary is fully measured: {refused:?}"
    );
    assert_eq!(measured, 24);
    let sites: u32 = record.keys().iter().map(|key| key.sites).sum();
    assert_eq!(sites, SITES, "no site is dropped from the accounting");

    let mut vocabulary: Vec<&str> = record.keys().iter().map(|key| key.key.as_str()).collect();
    vocabulary.sort_unstable();
    assert_eq!(
        vocabulary,
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
        "the exact 26 keys, so a key that appears or disappears fails here"
    );

    // The two outcome keys answer for their own name and take no arguments.
    for name in ["INSTANTWIN", "INSTANTLOSS"] {
        let key = record.key(name).expect("the outcome key is spelled");
        assert_eq!(key.sites, 1);
        assert_eq!(
            key.agreed_shape(),
            Some(&DirectiveShape::Bare),
            "{name} is spelled bare, so a text follower is the next key"
        );
    }
}

/// **The sheet's three regression priorities are in the record, with the
/// actors the original spells, and resolve to measured operations.**
///
/// * **subsystem disablement**: the eight `g_engine1..g_engine8` parts, each
///   listed as a `healthy_part` chain, and the four `INACTIVE_COMPLETION_COUNT`
///   thresholds (4, 1, 2, 3) that let a *part* of the list clear a block —
///   beside the barge and gate chains that use the same evaluator with a
///   single member each, and the target-list entry the engine part draws;
/// * **multi-step interaction**: the target-flag chain that walks `propane` →
///   `sprucegoose` → `tugandbarge01..04` across blocks 4, 8, 13, 17, 21 and
///   25 (each step adds the next object, and the later ones also remove the
///   objects the earlier steps added), the 35 nap re-wakes that clear a
///   completed flag so a step can complete again, and the eight `SET_AI_NET`
///   re-pointings of the patrol boats and firebrands;
/// * **passenger identity**: **no directive, target or actor names a
///   passenger.** The archive's only passenger-named string is the
///   `Passenger_hangar` location node, which no other member and no directive
///   site references — so this priority has no program binding to predicate
///   yet, and nothing here invents one.
///
/// Every key is asserted to resolve to the operation the shared finding
/// measured, so a key that silently lost its meaning fails here, and the
/// measured unknowns the keys carry are asserted to still be there.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m06_b_the_sheet_priorities_are_located_and_resolve_to_measured_operations() {
    let row = census().row(MISSION).unwrap();
    let record = row.record().unwrap();
    let operation = |key: &str| match record.key(key).unwrap().disposition() {
        DirectiveDisposition::Measured(measured) => measured.operation,
        other => panic!("{key} is not measured: {other:?}"),
    };

    // Subsystem disablement.
    for key in [
        "INACTIVE1",
        "INACTIVE2",
        "INACTIVE3",
        "INACTIVE4",
        "INACTIVE5",
        "INACTIVE6",
        "INACTIVE7",
        "INACTIVE8",
    ] {
        assert_eq!(operation(key), DirectiveOperation::InactiveMembers);
    }
    assert_eq!(
        operation("INACTIVE_COMPLETION_COUNT"),
        DirectiveOperation::InactiveThreshold
    );
    assert_eq!(operation("DEDG"), DirectiveOperation::EnemyGroupDepletion);
    // Multi-step interaction.
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
    assert_eq!(operation("SET_AI_NET"), DirectiveOperation::AssignNet);
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
    // And the record's own lifecycle markers.
    assert_eq!(operation("BEGIN_DORMANT"), DirectiveOperation::DormantStart);

    let (document, _member) = read_control_member(&game_dir(), MISSION).expect("control member");
    let blocks = blocks_of(&document);

    // --- subsystem disablement -----------------------------------------
    assert_eq!(with(&blocks, "INACTIVE_COMPLETION_COUNT"), [50, 72, 73, 74]);
    let thresholds: Vec<i64> = spelled(&blocks, "INACTIVE_COMPLETION_COUNT")
        .iter()
        .map(|args| match args.as_slice() {
            [ZrdValue::Int(number)] => i64::from(*number),
            other => panic!("an INACTIVE_COMPLETION_COUNT site spells {other:?}"),
        })
        .collect();
    assert_eq!(thresholds, [4, 1, 2, 3], "each threshold is record data");
    for number in [50u32, 72, 73, 74] {
        let directives = &blocks
            .iter()
            .find(|(n, _)| *n == number)
            .unwrap_or_else(|| panic!("block {number} exists"))
            .1;
        let listed: Vec<&Directive> = directives
            .iter()
            .filter(|d| d.key.starts_with("INACTIVE") && d.key != "INACTIVE_COMPLETION_COUNT")
            .collect();
        assert_eq!(
            listed.len(),
            8,
            "block {number} lists the eight engine parts"
        );
        for (position, directive) in listed.iter().enumerate() {
            assert_eq!(
                directive.key,
                format!("INACTIVE{}", position + 1),
                "block {number} spells the parts in order"
            );
            let chain = directive.args.as_deref().unwrap_or(&[]);
            assert_eq!(
                chain,
                [
                    text(&format!("g_engine{}", position + 1)),
                    text("healthy_part")
                ],
                "block {number} {} addresses its own part's in-play flag",
                directive.key
            );
        }
    }
    // The same evaluator with a single member elsewhere: the barge and gate
    // chains, in the order the record spells them.
    assert_eq!(
        with(&blocks, "INACTIVE1"),
        [4, 13, 17, 21, 25, 33, 34, 35, 36, 50, 72, 73, 74]
    );
    let single_member: Vec<Vec<ZrdValue>> = spelled(&blocks, "INACTIVE1")
        .iter()
        .take(9)
        .cloned()
        .collect();
    assert_eq!(
        single_member,
        [
            vec![text("kkgate"), text("healthy")],
            vec![text("tugandbarge01"), text("thlthy")],
            vec![text("tugandbarge02"), text("thlthy")],
            vec![text("tugandbarge03"), text("thlthy")],
            vec![text("tugandbarge04"), text("thlthy")],
            vec![text("tugandbarge01"), text("thlthy")],
            vec![text("tugandbarge02"), text("thlthy")],
            vec![text("tugandbarge03"), text("thlthy")],
            vec![text("tugandbarge04"), text("thlthy")],
        ]
    );
    assert_eq!(
        with(&blocks, "DEDG"),
        [12, 43, 46, 65, 68],
        "five group-depletion conditions"
    );
    let dedg: Vec<Vec<i64>> = spelled(&blocks, "DEDG")
        .iter()
        .map(|args| {
            args.iter()
                .filter_map(|value| match value {
                    ZrdValue::Int(int) => Some(i64::from(*int)),
                    _ => None,
                })
                .collect()
        })
        .collect();
    assert_eq!(
        dedg,
        [vec![1, 2], vec![2, 0], vec![1, 0], vec![1, 2], vec![2, 0]],
        "each threshold is record data: a group with N members left, not a guess"
    );

    // The target list carries the engine part as a target of its own, which is
    // how the disabled subsystem reaches the player's target info.
    let targets = target_records();
    assert_eq!(targets.len(), 7, "seven authored targets");
    let descriptions: Vec<&str> = targets
        .iter()
        .map(|record| {
            record
                .iter()
                .find(|(key, _)| key == "description")
                .map(|(_, value)| value.as_text().unwrap_or("?"))
                .expect("every target spells a description")
        })
        .collect();
    assert_eq!(
        descriptions,
        [
            "MSG_TRGT_SGOOSE",
            "MSG_TRGT_SGOOSE_ENGINE",
            "MSG_TRGT_PROPANE_TANKS",
            "MSG_TRGT_BARGE",
            "MSG_TRGT_BARGE",
            "MSG_TRGT_BARGE",
            "MSG_TRGT_BARGE",
        ]
    );
    let engine = &targets[1];
    let nodes = engine
        .iter()
        .find(|(key, _)| key == "nodes")
        .map(|(_, value)| value.clone())
        .expect("the engine target lists its nodes");
    assert_eq!(
        nodes,
        ZrdValue::List(vec![text("healthy_part")]),
        "the engine target resolves to the same part chain the blocks disable"
    );

    // --- multi-step interaction ----------------------------------------
    assert_eq!(with(&blocks, "ADD_OBJECTIVE_TARGET"), [4, 8, 13, 17, 21]);
    assert_eq!(
        spelled(&blocks, "ADD_OBJECTIVE_TARGET"),
        [
            vec![text("sprucegoose")],
            vec![text("tugandbarge01")],
            vec![text("tugandbarge02")],
            vec![text("tugandbarge03")],
            vec![text("tugandbarge04")],
        ]
    );
    assert_eq!(
        with(&blocks, "REMOVE_OBJECTIVE_TARGET"),
        [4, 13, 17, 21, 25]
    );
    assert_eq!(
        spelled(&blocks, "REMOVE_OBJECTIVE_TARGET"),
        [
            vec![text("propane")],
            vec![text("tugandbarge01")],
            vec![text("tugandbarge01"), text("tugandbarge02")],
            vec![
                text("tugandbarge01"),
                text("tugandbarge02"),
                text("tugandbarge03")
            ],
            vec![
                text("tugandbarge01"),
                text("tugandbarge02"),
                text("tugandbarge03"),
                text("tugandbarge04")
            ],
        ],
        "each step removes the objects the previous steps added"
    );
    let naps = spelled(&blocks, "NAP_OBJECTIVE_WHEN_I_COMPLETE");
    assert_eq!(naps.len(), 35, "the re-wake mechanism is spelled 35 times");
    for nap in &naps {
        assert!(
            nap.len() == 2
                && matches!(nap.first(), Some(ZrdValue::Int(_)))
                && matches!(nap.get(1), Some(ZrdValue::Float(_))),
            "a nap spells [target block, delay seconds]: {nap:?}"
        );
    }
    assert_eq!(
        with(&blocks, "SET_AI_NET"),
        [58, 59, 60, 61, 62, 63, 70, 71]
    );
    assert_eq!(
        spelled(&blocks, "SET_AI_NET"),
        [
            vec![ZrdValue::List(vec![
                text("patrolboat_eg0"),
                text("M2GoosePatrol")
            ])],
            vec![ZrdValue::List(vec![
                text("patrolboat_eg1"),
                text("M2GoosePatrol")
            ])],
            vec![ZrdValue::List(vec![
                text("patrolboat_eg2"),
                text("M2GoosePatrol")
            ])],
            vec![ZrdValue::List(vec![
                text("patrolboat_eg3"),
                text("M2GoosePatrol")
            ])],
            vec![ZrdValue::List(vec![
                text("patrolboat_eg4"),
                text("M2GoosePatrol")
            ])],
            vec![ZrdValue::List(vec![
                text("patrolboat_eg5"),
                text("M2GoosePatrol")
            ])],
            vec![
                ZrdValue::List(vec![text("patrolboat_eg0"), text("M2PatrolStop")]),
                ZrdValue::List(vec![text("patrolboat_eg1"), text("M2PatrolStop")]),
                ZrdValue::List(vec![text("patrolboat_eg2"), text("M2PatrolStop")]),
                ZrdValue::List(vec![text("patrolboat_eg3"), text("M2PatrolStop")]),
                ZrdValue::List(vec![text("patrolboat_eg4"), text("M2PatrolStop")]),
                ZrdValue::List(vec![text("patrolboat_eg5"), text("M2PatrolStop")]),
            ],
            vec![
                ZrdValue::List(vec![text("hkfirebrand_1"), text("M2Third")]),
                ZrdValue::List(vec![text("hkfirebrand_2"), text("M2Third")]),
                ZrdValue::List(vec![text("hkfirebrand_3"), text("M2Third")]),
                ZrdValue::List(vec![text("hkfirebrand_9"), text("M2Third")]),
            ],
        ],
        "six single re-pointings, then the six-boat and four-firebrand moves"
    );

    // --- passenger identity --------------------------------------------
    // The archive spells four locations, each in `location.zrd` and nowhere
    // else, and no directive site names any of them.
    let locations = member_texts("location.zrd");
    for name in LOCATIONS {
        assert!(
            locations.iter().any(|value| value == name),
            "location.zrd spells {name}"
        );
    }
    for member in [
        "aiv.zrd",
        "dzones.zrd",
        "egen.zrd",
        "map.zrd",
        "mis_anim.zrd",
        "net.zrd",
        "objectives.zrd",
        "startanims.zrd",
        "targets.zrd",
        "weather.zrd",
        "barge.zrd",
        "goosepath.zrd",
        "security_destroy.zrd",
        "zepstate.zrd",
    ] {
        let texts = member_texts(member);
        for name in LOCATIONS {
            assert!(
                !texts.iter().any(|value| value == name),
                "only location.zrd carries {name}: {member} does not"
            );
        }
    }
    // No control directive addresses a location, so the passenger-identity
    // priority has no program binding in this record.
    let addressed: Vec<u32> = blocks
        .iter()
        .filter(|(_, directives)| {
            directives.iter().any(|directive| {
                directive.args.iter().flatten().any(|arg| {
                    LOCATIONS
                        .iter()
                        .any(|name| matches!(arg, ZrdValue::Text(text) if text.as_str() == *name))
                })
            })
        })
        .map(|(number, _)| *number)
        .collect();
    assert!(
        addressed.is_empty(),
        "no directive names a location: {addressed:?}"
    );
    // The whole archive carries exactly one passenger-named string, and it is
    // the location node — no actor in the actor-init member is a passenger.
    let actors = member_texts("aiv.zrd");
    let passengers: Vec<&String> = actors
        .iter()
        .filter(|value| value.to_lowercase().contains("passenger"))
        .collect();
    assert!(
        passengers.is_empty(),
        "the actor-init member spells no passenger: {passengers:?}"
    );
    // M06-A's binding still records interaction authorizations as unbound, so
    // this stage cannot quietly read one into the record.
    assert!(
        mission_binding()
            .unknowns
            .iter()
            .any(|unknown| unknown.starts_with("interaction authorizations:")),
        "the mission binding still names interaction authorizations as unbound"
    );

    // The measured unknowns the priority keys carry are still carried.
    let inactive = match record.key("INACTIVE1").unwrap().disposition() {
        DirectiveDisposition::Measured(measured) => measured,
        other => panic!("INACTIVE1 is not measured: {other:?}"),
    };
    assert!(
        !inactive.unknowns.is_empty(),
        "the in-play bit's untraced writers are still recorded, never dropped"
    );
}

/// **Both terminal latches are gated, and every block address the record
/// spells is a block of this record.**
///
/// `INSTANTWIN` sits in block 47 and `INSTANTLOSS` in block 51; no other block
/// ends the mission. Both start dormant with no timed wake (`BEGIN_DORMANT`
/// `-1`), and of the 64 dormant markers only blocks 1, 2 and 3 — the mission
/// start — arm a timed self-wake, so neither latch can fire on its own clock.
///
/// The address walk covers 107 spelled integers — 25 wake, 47 kill and 35 nap
/// — and every one lies in `1..=82`. The addressing itself is not assumed
/// here: M02-B-FU3 (#802) measured the original's parse decrementing every
/// objective address before storing it, so a spelled number **is** the block
/// number, and `cs_sim::objectives::address` implements that rule
/// (`[1, objectives]`, mapping to index `address - 1`). Two values of M06's
/// record discriminate the readings and both agree with the measured rule:
/// block 67 spells `82`, the block count itself (the last block, in range — an
/// index reading would call it dangling), and **nothing spells `50`** although
/// block 50 naps `51`, so the failure latch has the completion edge the
/// block-number reading gives it. The two latches' edges are pinned below;
/// the one sibling suite that still reads an address as an index is held by
/// **M06-B-FU3** (#819), not edited here.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m06_b_the_terminal_blocks_are_gated_and_every_address_is_in_range() {
    let (document, _member) = read_control_member(&game_dir(), MISSION).expect("control member");
    let blocks = blocks_of(&document);
    let block = |number: u32| {
        &blocks
            .iter()
            .find(|(n, _)| *n == number)
            .unwrap_or_else(|| panic!("OBJECTIVE{number} exists"))
            .1
    };

    let outcomes: Vec<u32> = blocks
        .iter()
        .filter(|(_, directives)| directives.iter().any(|d| d.key.starts_with("INSTANT")))
        .map(|(number, _)| *number)
        .collect();
    assert_eq!(outcomes, [47, 51], "no other block ends the mission");
    for (number, outcome) in [(47, "INSTANTWIN"), (51, "INSTANTLOSS")] {
        let directives = block(number);
        assert!(
            directives
                .iter()
                .any(|d| d.key == outcome && d.args.is_none())
        );
        let dormant = directives
            .iter()
            .find(|d| d.key == "BEGIN_DORMANT")
            .unwrap_or_else(|| panic!("block {number} starts dormant"));
        assert_eq!(
            dormant.args.as_deref(),
            Some(&[ZrdValue::Float(-1.0)][..]),
            "block {number} never wakes on its own clock"
        );
    }

    let mut timed_wakes = Vec::new();
    let mut dormant_count = 0;
    for (number, directives) in &blocks {
        let Some(dormant) = directives.iter().find(|d| d.key == "BEGIN_DORMANT") else {
            continue;
        };
        dormant_count += 1;
        // Measured: child0 is the mission-clock second at which the block wakes
        // itself, and a value below zero disables the timed wake.
        let armed = match dormant.args.as_ref().and_then(|args| args.first()) {
            Some(ZrdValue::Float(seconds)) => *seconds >= 0.0,
            _ => true,
        };
        if armed {
            timed_wakes.push(*number);
        }
    }
    assert_eq!(dormant_count, 64);
    assert_eq!(
        timed_wakes,
        [1, 2, 3],
        "only the three mission-start blocks arm a timed self-wake; the other 61 markers spell -1"
    );
    assert!(
        !timed_wakes.contains(&47) && !timed_wakes.contains(&51),
        "neither terminal latch may wake on its own clock: {timed_wakes:?}"
    );
    let undormant: Vec<u32> = blocks
        .iter()
        .filter(|(_, directives)| !directives.iter().any(|d| d.key == "BEGIN_DORMANT"))
        .map(|(number, _)| *number)
        .collect();
    assert_eq!(
        undormant,
        [
            4, 9, 11, 33, 34, 35, 36, 37, 38, 39, 40, 41, 50, 64, 69, 72, 73, 74
        ],
        "18 blocks are entered through a predecessor's edge rather than a dormant marker"
    );

    // The addresses, walked with the measured child rules.
    let mut edges: BTreeMap<&str, u32> = BTreeMap::new();
    let mut out_of_range = Vec::new();
    let mut highest = 0i64;
    for (number, directives) in &blocks {
        for directive in directives {
            if !addresses_blocks(directive) {
                continue;
            }
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
            ("NAP_OBJECTIVE_WHEN_I_COMPLETE", 35),
            ("WAKE_OBJECTIVE_WHEN_I_COMPLETE", 25),
        ]),
        "the address walk visits every spelled address: 25 wake, 47 kill, 35 nap"
    );
    assert!(
        out_of_range.is_empty(),
        "every address is a block of this record: {out_of_range:?}"
    );
    assert_eq!(
        highest,
        i64::from(BLOCKS),
        "block 67 spells 82 — the block count itself, which an index reading \
         would report as dangling"
    );

    // Who may fire the two latches: a completion edge names each of them, and
    // no address equals the failure latch's predecessor-less number 50.
    let spelled_addresses: Vec<(u32, String, Vec<i64>)> = blocks
        .iter()
        .flat_map(|(number, directives)| {
            directives
                .iter()
                .filter(|directive| addresses_blocks(directive))
                .map(|directive| (*number, directive.key.clone(), addresses(directive)))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        !spelled_addresses
            .iter()
            .any(|(_, _, addresses)| addresses.contains(&50)),
        "nothing spells 50, so an index reading would leave the failure latch unreachable"
    );
    let naming = |target: i64| -> Vec<(u32, String, Vec<i64>)> {
        spelled_addresses
            .iter()
            .filter(|(_, _, addresses)| addresses.contains(&target))
            .cloned()
            .collect()
    };
    assert_eq!(
        naming(47),
        [
            (46, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![47]),
            (
                50,
                "KILL_OBJECTIVE_WHEN_I_COMPLETE".to_owned(),
                vec![29, 30, 31, 32, 37, 38, 39, 40, 44, 45, 46, 47]
            ),
        ],
        "the success latch is napped by block 46 and killed by block 50's twelve-part kill"
    );
    assert_eq!(
        naming(51),
        [(50, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![51])],
        "the failure latch has exactly one completion edge: block 50's nap"
    );
}

/// **Every call binds, every condition lowers and M06's record completes.**
///
/// * **calls**: all 265 sites bind — the fifteen
///   `KILL_OBJECTIVE_WHEN_I_COMPLETE` sites (two of them twelve targets wide,
///   past [`MAX_CALL_ARGS`] as positional operands) through M02-B-FU1's
///   list-argument carrying (#800), and the eight `ANIM_STATE` sites through
///   M04-B-FU1's generalization of the same mechanism (#806): the operand
///   list an `ANIM_STATE` evaluator reads is carried as one `Value::List`
///   argument. The registry's bound itself is unchanged, which the synthetic
///   pair at the end of this file pins on a key that takes no index list;
/// * **conditions**: all 82 block conditions lower — the three `ANIM_STATE`
///   sites at blocks 9, 11 and 41 that spell a `COMPLETION_COUNT` override
///   with two descriptors append both pairs and let the in-list count
///   overwrite `required` to 1, the way the original's operand-list walk
///   reads them.
///
/// The bound program reaches `MissionProgram::validate` and validates; the
/// row is complete. The condition-shape class this pin recorded was filed as
/// M04-B-FU1 (#806), and M06's own instance as M06-B-FU1 (#817).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m06_b_every_call_binds_every_condition_lowers_and_m06s_record_completes() {
    let row = census().row(MISSION).unwrap();
    let record = row.record().unwrap();

    let kill = record
        .key("KILL_OBJECTIVE_WHEN_I_COMPLETE")
        .expect("it is spelled");
    assert_eq!((kill.blocks, kill.sites), (15, 15));
    let mut kill_shapes: Vec<(usize, u32)> = kill
        .shapes
        .iter()
        .map(|(shape, sites)| (shape.arity(), *sites))
        .collect();
    kill_shapes.sort();
    assert_eq!(
        kill_shapes,
        [(1, 8), (3, 5), (12, 2)],
        "eight single-target sites, five three-target ones and two twelve-target ones"
    );
    let longest = kill_shapes
        .iter()
        .map(|(arity, _)| *arity)
        .max()
        .unwrap_or(0);
    assert!(
        longest > MAX_CALL_ARGS,
        "the twelve-target shape is {longest} positional operands, past {MAX_CALL_ARGS} — \
         it only binds because the list is carried as one argument"
    );

    let anim = record.key("ANIM_STATE").expect("it is spelled");
    assert_eq!((anim.blocks, anim.sites), (8, 8));
    let mut anim_shapes: Vec<(usize, u32)> = anim
        .shapes
        .iter()
        .map(|(shape, sites)| (shape.arity(), *sites))
        .collect();
    anim_shapes.sort();
    assert_eq!(
        anim_shapes,
        [(2, 5), (6, 3)],
        "five single-pair sites and the three COMPLETION_COUNT sites"
    );

    let attempt = row.lowering_attempt().unwrap();
    let lowered = attempt.attempt();
    assert_eq!(lowered.mission.as_deref(), Ok("mission/ch2-m01"));
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
        "the registry refused no key"
    );
    assert!(
        attempt.program().is_some(),
        "a program stands for all 82 objectives"
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
        "all 82 block conditions lower: {refused_conditions:?}"
    );

    // The three override sites carry the measured evaluators: both
    // descriptors appended, and each in-list COMPLETION_COUNT [1] overwrites
    // `required`.
    let raw = attempt.raw_program().expect("the program assembled");
    for index in [8usize, 10, 40] {
        let (required, pairs) = animation_evaluator(&raw.objectives[index].condition);
        assert_eq!(
            required,
            1,
            "block {}'s COMPLETION_COUNT [1] overwrites the two-pair count",
            index + 1
        );
        assert_eq!(
            pairs.len(),
            2,
            "block {} appends both its descriptors",
            index + 1
        );
    }

    let lowering = row.lowering().unwrap();
    assert_eq!(
        lowering.unmet().count(),
        0,
        "every lowering requirement is met"
    );
    assert!(lowering.complete());
    assert!(row.is_complete());
}

/// **M06 is a complete census row; the campaign is still not ready.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m06_b_m06_is_complete_and_the_campaign_stays_unready() {
    let census = census();
    assert!(
        census.complete_missions().contains(&MISSION),
        "M06 joins the census's complete rows"
    );
    assert!(
        !census.campaign_ready(),
        "the campaign is still not ready — other missions carry their own gaps"
    );
    assert!(census.measured_rows().any(|row| row.mission() == MISSION));
    let row = census.row(MISSION).unwrap();
    assert!(row.is_measured(), "the program is measured");
    assert!(row.is_complete(), "M06's row is complete");
}

// ---------------------------------------------------------------------------
// Synthetic: the measured lowering, and the arms that still refuse
// ---------------------------------------------------------------------------

/// An `ANIM_STATE` operand list: an optional `COMPLETION_COUNT` override, then
/// `pairs` `ANIM` descriptors — the shapes M06 spells (2 and 6 operands).
fn anim_state_operands(pairs: usize, completion_count: Option<u32>) -> Vec<ZrdValue> {
    let mut operands = Vec::new();
    if let Some(count) = completion_count {
        operands.push(text("COMPLETION_COUNT"));
        operands.push(ZrdValue::List(vec![int(count)]));
    }
    for index in 0..pairs {
        operands.push(text("ANIM"));
        operands.push(ZrdValue::List(vec![
            text("NAME"),
            ZrdValue::List(vec![text(&format!("anim{index}"))]),
            text("STATE"),
            ZrdValue::List(vec![text("RUNNING")]),
        ]));
    }
    operands
}

/// The block's one animation evaluator out of a lowered condition, wherever
/// it sits inside the gate — `All` when it is the only evaluator, `Any`
/// beside another kind otherwise.
fn animation_evaluator(condition: &Condition) -> (u32, &Vec<(String, AnimationState)>) {
    fn find(condition: &Condition) -> Option<(u32, &Vec<(String, AnimationState)>)> {
        match condition {
            Condition::AnimationStates {
                required,
                animations,
            } => Some((*required, animations)),
            Condition::All(items) | Condition::Any(items) => items.iter().find_map(find),
            _ => None,
        }
    }
    find(condition)
        .unwrap_or_else(|| panic!("the block carries no animation evaluator: {condition:?}"))
}

/// **M06's completion-count spelling lowers: both pairs append and the
/// in-list count overwrites `required`.**
///
/// The spelling M06's blocks 9, 11 and 41 use — a `COMPLETION_COUNT [1]`
/// plus two `ANIM` descriptors inside the one operand list, six operands —
/// lowers the measured way: the operand list is one call argument, so the
/// site binds inside the bound either way; every pair appends; and the
/// sibling count found in the same list overwrites `required`.
#[test]
fn accept_m06_b_a_completion_count_site_lowers_with_its_override() {
    let authored = |operands: Vec<ZrdValue>| {
        control_record(vec![block(
            1,
            vec![
                directive("BEGIN_DORMANT", vec![ZrdValue::Float(-1.0)]),
                directive("ANIM_STATE", operands),
            ],
        )])
    };
    let mission = Ok(ContentId::from_source(ContentKind::Mission, "syn-01")
        .expect("a synthetic mission id is valid"));

    let single = authored(anim_state_operands(1, None));
    let record = measure_control_record(&single);
    let lowered = lower_control_record(mission.clone(), "zbd/synth/mission", &single, &record);
    assert!(
        record.is_complete(lowered.attempt()),
        "one pair lowers and the record completes: {:?}",
        lowered.attempt()
    );
    assert!(
        lowered
            .attempt()
            .conditions
            .iter()
            .all(|outcome| *outcome == ConditionOutcome::Lowered),
        "the single-pair condition lowers"
    );
    assert!(
        lowered.attempt().calls.contains(&CallOutcome::Bound),
        "the single-pair site produced a bound call"
    );

    let m06_shape = authored(anim_state_operands(2, Some(1)));
    let record = measure_control_record(&m06_shape);
    let lowered = lower_control_record(mission, "zbd/synth/mission", &m06_shape, &record);
    assert!(
        record.is_complete(lowered.attempt()),
        "M06's spelling lowers now: {:?}",
        lowered.attempt()
    );
    let raw = lowered.raw_program().expect("the program assembled");
    let (required, pairs) = animation_evaluator(&raw.objectives[0].condition);
    assert_eq!(
        required, 1,
        "the in-list COMPLETION_COUNT overwrites the two-pair count"
    );
    assert_eq!(
        pairs.len(),
        2,
        "both descriptors appended, in declaration order"
    );
    assert!(
        lowered.attempt().unbound_keys.is_empty(),
        "the operand list registers as one list argument"
    );
}

/// **A twelve-target kill binds as one list argument; a wide key that takes no
/// list argument still refuses.**
///
/// M06's retail record leans on both halves. Its widest
/// `KILL_OBJECTIVE_WHEN_I_COMPLETE` site spells twelve targets — twelve
/// positional operands, past [`MAX_CALL_ARGS`] — and binds because M02-B-FU1
/// (#800) carries a list-taking directive's spelled list as one `Value::List`
/// argument; authored the same way here, the record lowers completely. The
/// bound itself was never raised, so a key whose measured operation takes no
/// list argument still refuses its whole registration when its sites are wide
/// enough: a positional `IDENTITY` site of [`MAX_CALL_ARGS`] + 1 arguments
/// refuses, its site refuses with it and the key never enters the registry.
/// Both arms run in CI without original data, and whichever follow-up widens
/// either mechanism must update both pins in its own change — never delete
/// them to get green.
///
/// M03-B-FU2 (#810) is that follow-up for `SET_AI_NET`: it gave the
/// `{actor, net}` pair list the same one-list-argument shape, so this arm's
/// refusal moved from `SET_AI_NET` to the positional `IDENTITY` exactly as
/// M02-B-FU1 kept it, and `SET_AI_NET`'s own pin — nine pairs now binding as
/// one list argument — is asserted beside it.
#[test]
fn accept_m06_b_a_wide_kill_list_binds_and_a_wide_non_index_key_still_refuses() {
    let kill_record = |targets: usize| {
        let mut kill = Vec::new();
        for target in 0..targets {
            kill.push(int(target as u32));
        }
        control_record(vec![block(
            1,
            vec![
                directive("BEGIN_DORMANT", vec![ZrdValue::Float(-1.0)]),
                directive("DEDG", vec![int(1), int(0)]),
                directive("KILL_OBJECTIVE_WHEN_I_COMPLETE", kill),
            ],
        )])
    };

    // M06's widest kill site, authored as the archive spells it.
    let widest = kill_record(12);
    let record = measure_control_record(&widest);
    let lowered = lower(&widest);
    assert!(
        lowered.attempt().unbound_keys.is_empty(),
        "twelve targets carried as one list argument register: {:?}",
        lowered.attempt().unbound_keys
    );
    assert!(
        lowered
            .registry()
            .get("KILL_OBJECTIVE_WHEN_I_COMPLETE")
            .is_some(),
        "the key is in the registry"
    );
    assert!(
        record.is_complete(lowered.attempt()),
        "the record lowers completely: {:?}",
        lowered.attempt()
    );
    assert!(
        lowered.attempt().calls.contains(&CallOutcome::Bound),
        "the kill site bound"
    );

    // The bound is unchanged for a key whose measured operation takes no list
    // argument. This arm spelled `SET_AI_NET` until M03-B-FU2 (#810) gave its
    // pair list the one-list-argument shape; the positional `IDENTITY` keeps
    // the refusal, exactly as M02-B-FU1 left it when the objective-index
    // lists moved.
    let positional_record = |args: usize| {
        let mut spelled = Vec::new();
        for arg in 0..args {
            spelled.push(int(arg as u32));
        }
        control_record(vec![block(
            1,
            vec![
                directive("BEGIN_DORMANT", vec![ZrdValue::Float(-1.0)]),
                directive("DEDG", vec![int(1), int(0)]),
                directive("IDENTITY", spelled),
            ],
        )])
    };

    let at_bound = positional_record(MAX_CALL_ARGS);
    let lowered = lower(&at_bound);
    assert!(
        lowered.attempt().unbound_keys.is_empty(),
        "{MAX_CALL_ARGS} positional arguments are at the bound of {MAX_CALL_ARGS} and register: {:?}",
        lowered.attempt().unbound_keys
    );

    let over_bound = positional_record(MAX_CALL_ARGS + 1);
    let lowered = lower(&over_bound);
    assert_eq!(
        lowered.attempt().unbound_keys.len(),
        1,
        "the over-bound signature refuses the key's registration"
    );
    assert!(
        lowered.attempt().unbound_keys[0].contains("IDENTITY")
            && lowered.attempt().unbound_keys[0].contains("too many arguments"),
        "{}",
        lowered.attempt().unbound_keys[0]
    );
    let refusals: Vec<&str> = lowered
        .attempt()
        .calls
        .iter()
        .filter_map(|call| match call {
            CallOutcome::Refused(text) => Some(text.as_str()),
            CallOutcome::Bound => None,
        })
        .collect();
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert!(
        refusals[0].contains("unknown host call `IDENTITY`"),
        "{}",
        refusals[0]
    );
    assert!(
        lowered.registry().get("IDENTITY").is_none(),
        "the over-bound key is not in the registry"
    );
    assert!(
        !measure_control_record(&over_bound).is_complete(lowered.attempt()),
        "an over-bound site refuses its whole record, never a narrowed binding"
    );

    // `SET_AI_NET` since M03-B-FU2 (#810): its pair list is one list
    // argument, so nine pairs register where nine positional operands would
    // refuse, and the record lowers with them.
    let net_record = |pairs: usize| {
        let mut spelled = Vec::new();
        for pair in 0..pairs {
            spelled.push(ZrdValue::List(vec![
                text(&format!("actor{pair}")),
                text("net"),
            ]));
        }
        control_record(vec![block(
            1,
            vec![
                directive("BEGIN_DORMANT", vec![ZrdValue::Float(-1.0)]),
                directive("DEDG", vec![int(1), int(0)]),
                directive("SET_AI_NET", spelled),
            ],
        )])
    };

    let widened = net_record(MAX_CALL_ARGS + 1);
    let record = measure_control_record(&widened);
    let lowered = lower(&widened);
    assert!(
        lowered.attempt().unbound_keys.is_empty(),
        "{MAX_CALL_ARGS} + 1 pairs are one list argument, so the key registers: {:?}",
        lowered.attempt().unbound_keys
    );
    let spec = lowered
        .registry()
        .get("SET_AI_NET")
        .expect("the pair list registers the key");
    assert!(
        spec.signatures
            .iter()
            .all(|signature| matches!(signature.as_slice(), [ArgDomain::List(_)])),
        "one list argument per measured shape: {:?}",
        spec.signatures
    );
    assert!(
        lowered.attempt().calls.contains(&CallOutcome::Bound),
        "the `SET_AI_NET` site bound"
    );
    assert!(
        record.is_complete(lowered.attempt()),
        "the record lowers with nine pairs: {:?}",
        lowered.attempt()
    );
}
