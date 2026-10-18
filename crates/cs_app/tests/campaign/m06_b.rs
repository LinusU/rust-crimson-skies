//! Acceptance stage M06-B: the mission-specific compatibility gaps of the
//! sixth mission (`missions/M06.md`, work order `M06-B`).
//!
//! **Archive attribution (#1596, after #1565 `M01-ARCHIVE-DECISION`).** This
//! suite was first pinned on `zbd/c2/m01`, the archive the pre-correction
//! layout bound to M06; that record declares `snd_c3-HW-m2` — Hollywood
//! mission 2 — and the corrected content-derived campaign layout binds it to
//! **M12**. M06's own reader archive is `zbd/c1c/m01` — the Northwest
//! mission-1 archive, bound to M01 when the record's measurements were
//! written — and every pin below is re-derived from it through production
//! code. See `docs/findings/2026-10-17-m01-archive-decision.md` and the
//! 2026-10-18 section of
//! `docs/findings/2026-10-09-m06-b-compatibility-gaps.md`.
//!
//! M06-A bound M06's identities and left its program unmeasured. The
//! machinery that measures a control program (the `.zrd` reader, the census,
//! the directive dispositions, the record → `RawProgram` adapter and the
//! binding `SourceContext::control_program` adds on top of them) is shared
//! and was built for M01/M02; this stage runs all of it over M06's **own**
//! reader archive and pins what is *different* at M06, so the gaps are
//! recorded as measurements instead of surfacing later as a silent failure:
//!
//! * M06's control program is `objectives.zrd` with **58** numbered blocks
//!   and **353** directive sites across **43** keys, selected by the content
//!   rule and never by its name;
//! * every key has a disposition: two terminal outcomes and 41 measured
//!   effects, **no** unmeasured key, no refusal and no unclassified
//!   record-level key;
//! * the sheet's three regression priorities are located in the record with
//!   the actors the original spells — subsystem disablement (the Workers'
//!   Voyage zeppelin's eighteen `reng*`/`leng*` engines and nine turret
//!   parts, the pirate zeppelin's twelve engines and the `DEDG` group
//!   tallies), multi-step interaction (the tailhook boarding chain — the
//!   approach gate, the target-flag walk onto `wv_tailhook`/`peoplehook`, the
//!   `activate_*_node` animation wakeups, the 27 nap re-wakes that pace the
//!   radio warning chains, the `SET_AI_NET` re-pointings and the three
//!   `TICK_DEPENDS_ON_OBJ` gates) and passenger identity, bound end to end by
//!   name rather than by a `passenger` actor: the copilot exchange is spelled
//!   in the record's own `ANIM_STATE` sites, the archive's largest member
//!   `wv_tailhook.zrd` declares the boarding nodes and the startup event
//!   table fires `call_add_jack`, the shared crew animation M06-B-FU4
//!   (#1184) measured driving the world node `apassengers`;
//! * both terminal latches are gated and every cross-objective address the
//!   record spells is a block of this record — including the address equal to
//!   the block count, which is what makes the record's addressing measurable
//!   rather than assumed;
//! * **the record lowers completely**: all 353 sites bind, all 58 conditions
//!   lower and `MissionProgram::validate` accepts.
//!
//! No behaviour is invented here. The runtime halves of the sheet's
//! priorities — the wrong actor, the wrong session, a repeated event — need
//! ordinary play (M06-C) and stay open; nothing in this file simulates them.
//! The lowering gap this stage originally recorded and the shared fix that
//! closed it are in `docs/findings/2026-10-09-m06-b-compatibility-gaps.md`
//! and `docs/findings/2026-10-09-m04-b-fu1-anim-state-operand-list.md`.
//!
//! M06-B-FU1 (#817) adds the `accept_m06_b_fu1_*` members. The three
//! six-operand `ANIM_STATE` sites the follow-up measured sit in the archive
//! the corrected layout binds to **M12**, so the retail pins re-derive them
//! from that record — M06's own record spells `ANIM_STATE` only as
//! single-pair sites, and the pins say so — while one synthetic test still
//! carries the measured spelling into CI.
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`; the three
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
use cs_script::ir::{Action, AnimationState, Condition, DirectiveOperation as IrOperation, Value};
use cs_types::content::{ContentId, ContentKind};

use crate::campaign::common::load_inventory;

/// The census row label of the mission — M06's own reader archive under the
/// corrected campaign layout (`missions/bindings/M06.json`).
pub(crate) const MISSION: &str = "zbd/c1c/m01";

/// The reader archive M06-A bound as the mission's program.
const CONTAINER: &str = "ZBD/C1C/M01/zrdr.zbd";
/// SHA-256 of that whole archive, from production discovery.
const CONTAINER_SHA256: &str = "33b60a402dd8a8374faf02db808c074181754370e29d1b5115aad01b41b22c9f";
/// The member the block-carrying rule picks (never a filename constant).
const CONTROL_MEMBER: &str = "objectives.zrd";
/// SHA-256 of the control member's own bytes.
const CONTROL_SHA256: &str = "edb84d894ec9c424645cec0840c0c081c69642115753b44026c2157d382a8e92";

/// The numbered blocks of the control member.
const BLOCKS: u32 = 58;
/// The directive sites of the control member.
const SITES: u32 = 353;
/// The distinct directive keys of the control member.
const KEYS: usize = 43;

/// The four named locations `location.zrd` spells, in member order. The second
/// is the archive's only passenger-named string.
const LOCATIONS: [&str; 4] = ["Airport_terminal", "Passenger_hangar", "Crops", "Coast"];

/// The work order the corrected layout binds the M06-B-FU1 follow-up's
/// measured archive to: `zbd/c2/m01` declares `snd_c3-HW-m2`, Hollywood
/// mission 2.
const FOLLOWUP_WORK_ORDER: &str = "M12";
/// The census row label of that measured archive.
const FOLLOWUP_ARCHIVE: &str = "zbd/c2/m01";
/// Its container, bound by the work order the corrected layout assigns it.
const FOLLOWUP_CONTAINER: &str = "ZBD/C2/M01/zrdr.zbd";
/// Its control record's block and site counts, for the re-pinned FU1 arms.
const FOLLOWUP_BLOCKS: u32 = 82;
const FOLLOWUP_SITES: u32 = 265;

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

/// The source context, read once for the whole suite (fingerprinting the
/// installation walks every file, so it happens exactly once).
fn context() -> &'static SourceContext {
    static CONTEXT: OnceLock<SourceContext> = OnceLock::new();
    CONTEXT.get_or_init(|| {
        SourceContext::read(&game_dir()).expect("the installation yields a source context")
    })
}

/// One work order's production control-program binding.
fn control_binding_of(work_order: &str) -> MissionControlBinding {
    let title = load_inventory()
        .iter()
        .find(|(candidate, _)| candidate.as_str() == work_order)
        .map(|(_, title)| title.clone())
        .unwrap_or_else(|| panic!("the declared inventory has an {work_order} work order"));
    context()
        .control_program(
            MissionLabel::new(work_order).unwrap_or_else(|error| panic!("{work_order}: {error}")),
            &title,
        )
        .unwrap_or_else(|error| panic!("{work_order}'s control program binds: {error}"))
}

/// The production control-program binding, built once for the whole suite.
fn control_binding() -> &'static MissionControlBinding {
    static BINDING: OnceLock<MissionControlBinding> = OnceLock::new();
    BINDING.get_or_init(|| control_binding_of("M06"))
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
        context()
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

/// The directives of one numbered block.
fn directives_of(blocks: &[(u32, Vec<Directive>)], number: u32) -> &Vec<Directive> {
    &blocks
        .iter()
        .find(|(n, _)| *n == number)
        .unwrap_or_else(|| panic!("OBJECTIVE{number} exists"))
        .1
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

// ---------------------------------------------------------------------------
// Retail: what M06's control program is
// ---------------------------------------------------------------------------

/// **The control program is the member that declares the numbered blocks.**
///
/// The member is found by the content rule: of the twelve members of M06's
/// reader archive exactly one declares numbered `OBJECTIVE<N>` blocks.
/// Neither size nor position decides it — the chosen member is the seventh,
/// and `wv_tailhook.zrd`, the boarding feature's own definition, is longer
/// than it. The blocks and sites the census measures equal an independent
/// walk of the same document, the archive is the program span M06-A bound,
/// and the production control binding reaches the same member, span and
/// digests through its own walk.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m06_b_the_control_program_is_the_member_that_declares_the_blocks() {
    let row = census().row(MISSION).expect("M06 is in the census");
    assert_eq!(row.container, CONTAINER);
    assert_eq!(
        row.container_sha256, CONTAINER_SHA256,
        "the reader archive is the program M06-A bound"
    );
    assert_eq!(row.members.len(), 12);

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
    assert_eq!(index, 6, "the seventh member, not the first");
    let chosen = &row.members[index];
    let longer: Vec<&str> = row
        .members
        .iter()
        .filter(|member| member.len > chosen.len)
        .map(|member| member.name.as_str())
        .collect();
    assert_eq!(
        longer,
        ["wv_tailhook.zrd"],
        "size is not the rule: the longest member is not the control member"
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
        "numbered 1..=58, no gaps"
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
    assert_eq!(bound.program_id.as_str(), "script/c1c-m01-zrdr");
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
/// each), the other 41 have a measured effect, and no key is Unmeasured. The
/// sites are accounted for: the keys' sites sum to the record's, and the
/// sorted vocabulary is exactly the 43 keys the archive spells.
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
    assert_eq!(measured, 41);
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
            "INACTIVE16",
            "INACTIVE17",
            "INACTIVE18",
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
            "WAKEUP_ZEP_TURRETS",
            "WAKE_ANIM",
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
        ],
        "the exact 43 keys, so a key that appears or disappears fails here"
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
/// * **subsystem disablement**: the Workers' Voyage zeppelin's eighteen
///   engines, listed as `owner → part → healthy` chains under
///   `INACTIVE1..INACTIVE18` with `INACTIVE_COMPLETION_COUNT` thresholds that
///   let a *part* of the list clear a block (1/5/9 of eighteen at blocks
///   6/8/9, 2 at block 55), the nine turret parts at blocks 7 and 28, the
///   pirate zeppelin's twelve engines at blocks 36..39 (2/6/10/12 cleared,
///   the fail countdown) and the `DEDG` group tallies;
/// * **multi-step interaction**: the tailhook boarding chain — the
///   `TRAVELERS` approach gate, the target flags that walk the objective
///   pip from `workersvoyagezep` onto `wv_tailhook`/`peoplehook` and off
///   again, the `WAKE_ANIM` wakeups that open the dropoff and pickup nodes,
///   the 27 nap re-wakes that pace the radio warning chains, the kill lists
///   that cancel them, the `SET_AI_NET` re-pointings and the
///   `TICK_DEPENDS_ON_OBJ` dependency gates;
/// * **passenger identity**: bound **end to end by name** — the copilot
///   exchange the record gates as three `ANIM_STATE` sites
///   (`wv_drop_copilot`, `wv_pickup_copilot`, `hooked_to_klondike`), the
///   boarding nodes the archive's own `wv_tailhook.zrd` member declares, and
///   the `call_add_jack` crew animation the startup event table fires. No
///   actor member names a passenger and `Passenger_hangar` stays a
///   location-only string, so the identity is the crew-animation mechanism,
///   never an invented actor.
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

    // Subsystem disablement: the eighteen stage keys, the threshold and the
    // group tally.
    for index in 1..=18 {
        assert_eq!(
            operation(&format!("INACTIVE{index}")),
            DirectiveOperation::InactiveMembers
        );
    }
    assert_eq!(
        operation("INACTIVE_COMPLETION_COUNT"),
        DirectiveOperation::InactiveThreshold
    );
    assert_eq!(operation("DEDG"), DirectiveOperation::EnemyGroupDepletion);
    // Multi-step interaction: the target flags, the lifecycle keys, the
    // approach gate, the animation wakeups and the dependency gates.
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
    assert_eq!(
        operation("TICK_DEPENDS_ON_OBJ"),
        DirectiveOperation::DependencyGate
    );
    assert_eq!(operation("TRAVELERS"), DirectiveOperation::Travelers);
    assert_eq!(operation("WAKE_ANIM"), DirectiveOperation::WakeAnimation);
    assert_eq!(
        operation("WAKEUP_GENERATOR"),
        DirectiveOperation::FeedGenerator
    );
    assert_eq!(
        operation("WAKEUP_ZEP_TURRETS"),
        DirectiveOperation::WakeZeppelinTurrets
    );
    assert_eq!(operation("WAKEUP_ENEMIES"), DirectiveOperation::WakeEnemies);
    assert_eq!(
        operation("COMPLETED_STOPPOINT"),
        DirectiveOperation::AdvanceStopPoint
    );
    assert_eq!(
        operation("SET_HELP_LABEL"),
        DirectiveOperation::SetHelpLabel
    );
    assert_eq!(
        operation("IDENTITY"),
        DirectiveOperation::PresentationIdentity
    );
    // The boarding's human cargo: the animation gates.
    assert_eq!(operation("ANIM_STATE"), DirectiveOperation::AnimationStates);
    // The sound keys and the record's own lifecycle markers.
    assert_eq!(
        operation("COMPLETED_SOUND_GROUP"),
        DirectiveOperation::CompletedSoundGroup
    );
    assert_eq!(
        operation("WAKEUP_SOUND_GROUP"),
        DirectiveOperation::WakeSoundGroup
    );
    assert_eq!(
        operation("STOP_QUEUED_SOUNDS"),
        DirectiveOperation::StopQueuedSounds
    );
    assert_eq!(operation("BEGIN_DORMANT"), DirectiveOperation::DormantStart);

    let (document, _member) = read_control_member(&game_dir(), MISSION).expect("control member");
    let blocks = blocks_of(&document);

    // --- subsystem disablement -----------------------------------------
    //
    // The mission's ship-disabling work is measured by part lists: a block's
    // `INACTIVE<k>` sites each spell one `owner → part → healthy` chain and
    // the block completes when at least its `INACTIVE_COMPLETION_COUNT` of
    // them are out of play (absent, the threshold defaults to all).
    assert_eq!(
        with(&blocks, "INACTIVE_COMPLETION_COUNT"),
        [6, 7, 8, 9, 36, 37, 38, 39, 55]
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
        [1, 1, 5, 9, 2, 6, 10, 12, 2],
        "each threshold is record data"
    );

    // A block's member chains, in the order the record spells them.
    let chains = |number: u32| -> Vec<(String, Vec<ZrdValue>)> {
        directives_of(&blocks, number)
            .iter()
            .filter(|directive| {
                directive.key.starts_with("INACTIVE")
                    && directive.key != "INACTIVE_COMPLETION_COUNT"
            })
            .map(|directive| {
                let chain = directive.args.clone().unwrap_or_else(|| {
                    panic!("block {number}'s {} spells a member chain", directive.key)
                });
                (directive.key.clone(), chain)
            })
            .collect()
    };
    let expected = |owner: &str, parts: &[&str]| -> Vec<(String, Vec<ZrdValue>)> {
        parts
            .iter()
            .enumerate()
            .map(|(index, part)| {
                (
                    format!("INACTIVE{}", index + 1),
                    vec![text(owner), text(part), text("healthy")],
                )
            })
            .collect()
    };

    // The Workers' Voyage's eighteen engines — right then left, nine each —
    // gate four blocks: 1, 5 and 9 of eighteen (blocks 6, 8 and 9, the last
    // the first PRIMARY objective) and 2 (block 55, the completion echo).
    let wv_engines: Vec<String> = (1..=9)
        .map(|n| format!("reng{n}"))
        .chain((1..=9).map(|n| format!("leng{n}")))
        .collect();
    for number in [6u32, 8, 9, 55] {
        assert_eq!(
            chains(number),
            expected(
                "workersvoyagezep",
                &wv_engines.iter().map(String::as_str).collect::<Vec<_>>()
            ),
            "block {number} disables the voyage's eighteen engines in order"
        );
    }
    // The same evaluator on the voyage's nine turret parts: block 7 needs
    // one cleared, and block 28 — which spells no threshold — needs them all.
    for number in [7u32, 28] {
        assert_eq!(
            chains(number),
            expected(
                "workersvoyagezep",
                &[
                    "utur1", "utur2", "utur3", "utur4", "ctur1", "ctur2", "ctur3", "ctur4",
                    "maingun9",
                ]
            ),
            "block {number} lists the voyage's nine turret parts"
        );
    }
    // The pirate zeppelin's twelve engines gate the four-step fail
    // countdown: 2, 6, 10 and 12 cleared, each step napping the next 15 s on.
    let pz_engines: Vec<String> = ["reng", "leng"]
        .iter()
        .flat_map(|side| {
            [11, 12, 21, 22, 31, 32]
                .iter()
                .map(move |n| format!("{side}{n}"))
        })
        .collect();
    for number in [36u32, 37, 38, 39] {
        assert_eq!(
            chains(number),
            expected(
                "piratezep",
                &pz_engines.iter().map(String::as_str).collect::<Vec<_>>()
            ),
            "block {number} disables the pirate zep's twelve engines in order"
        );
    }
    // The same evaluator on a one-member chain: the pirate zep itself, the
    // condition two dependency-gated blocks and block 57's dependency spell.
    for number in [54u32, 56] {
        assert_eq!(
            chains(number),
            [("INACTIVE1".to_owned(), vec![text("piratezep")])],
            "block {number} completes when the pirate zep itself is out of play"
        );
    }
    // The part names are the zeppelin member's own: `zeppelins.zrd` spells
    // every engine part the chains disable, for both zeppelins.
    let zeppelins = member_texts("zeppelins.zrd");
    for part in wv_engines.iter().chain(pz_engines.iter()) {
        assert!(
            zeppelins.iter().any(|value| value == part),
            "zeppelins.zrd spells the part {part} the chains disable"
        );
    }
    assert_eq!(with(&blocks, "DEDG"), [2, 16, 17, 20, 21, 42, 43, 44]);
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
        [
            vec![1, 0],
            vec![1, 0],
            vec![2, 0],
            vec![3, 0],
            vec![4, 0],
            vec![2, 0],
            vec![2, 0],
            vec![4, 1],
        ],
        "each tally is record data: a group with N members left, not a guess"
    );

    // --- multi-step interaction ----------------------------------------
    //
    // The boarding is an objective chain, and the record spells every
    // mechanism it runs on: the approach gate, the target flags, the node
    // wakeups, the pacing naps, the kills that cancel the warning chains,
    // the AI re-pointings and the dependency gates.
    assert_eq!(with(&blocks, "TRAVELERS"), [3]);
    assert_eq!(
        spelled(&blocks, "TRAVELERS"),
        [vec![
            text("player"),
            text("APPROACHING"),
            text("workersvoyagezep"),
            ZrdValue::Float(700.0),
            int(1),
        ]],
        "the approach gate: the player inside 700 of the Workers' Voyage"
    );
    assert_eq!(with(&blocks, "ADD_OBJECTIVE_TARGET"), [2, 17, 47, 50]);
    assert_eq!(
        spelled(&blocks, "ADD_OBJECTIVE_TARGET"),
        [
            vec![text("workersvoyagezep")],
            vec![text("pzhookpoint")],
            vec![ZrdValue::List(vec![
                text("wv_tailhook"),
                text("peoplehook")
            ])],
            vec![ZrdValue::List(vec![
                text("wv_tailhook"),
                text("peoplehook")
            ])],
        ],
        "the objective flag walks the zeppelin to its tailhook nodes"
    );
    assert_eq!(with(&blocks, "REMOVE_OBJECTIVE_TARGET"), [2, 11, 15, 50]);
    assert_eq!(
        spelled(&blocks, "REMOVE_OBJECTIVE_TARGET"),
        [
            vec![ZrdValue::List(vec![
                text("piratezep"),
                text("rock_zeppelin")
            ])],
            vec![ZrdValue::List(vec![
                text("wv_tailhook"),
                text("peoplehook")
            ])],
            vec![ZrdValue::List(vec![
                text("wv_tailhook"),
                text("peoplehook")
            ])],
            vec![text("workersvoyagezep")],
        ],
        "the flag comes off the pirate zep at the start and off the hook twice"
    );
    assert_eq!(with(&blocks, "ADD_OTHER_TARGET"), [2, 13, 50]);
    assert_eq!(
        spelled(&blocks, "ADD_OTHER_TARGET"),
        [
            vec![ZrdValue::List(vec![
                text("piratezep"),
                text("rock_zeppelin")
            ])],
            vec![text("blackswanzep")],
            vec![text("workersvoyagezep")],
        ],
        "the other-target flag lands on the pirate zep, the Black Swan and the voyage"
    );
    assert_eq!(with(&blocks, "WAKE_ANIM"), [11, 13, 15, 18, 49]);
    assert_eq!(
        spelled(&blocks, "WAKE_ANIM"),
        [
            vec![text("activate_dropoff_node")],
            vec![text("fadein_bszep")],
            vec![text("activate_pickup_node")],
            vec![text("pzhomebase")],
            vec![text("activate_dropoff_node")],
        ],
        "the hook opens twice, the Black Swan fades in and the zep heads home"
    );
    let naps = spelled(&blocks, "NAP_OBJECTIVE_WHEN_I_COMPLETE");
    assert_eq!(naps.len(), 27, "the re-wake mechanism is spelled 27 times");
    for nap in &naps {
        assert!(
            nap.len() == 2
                && matches!(nap.first(), Some(ZrdValue::Int(_)))
                && matches!(nap.get(1), Some(ZrdValue::Float(_))),
            "a nap spells [target block, delay seconds]: {nap:?}"
        );
    }
    assert_eq!(with(&blocks, "SET_AI_NET"), [2, 19, 22]);
    assert_eq!(
        spelled(&blocks, "SET_AI_NET"),
        [
            vec![
                ZrdValue::List(vec![text("devastator_2"), text("Bravo2")]),
                ZrdValue::List(vec![text("devastator_3"), text("Charlie2")]),
            ],
            vec![ZrdValue::List(vec![text("bsfury_1"), text("BlackSwan")])],
            vec![ZrdValue::List(vec![text("blackswanzep"), text("SwanZep2")])],
        ],
        "the devastator pair and the two Black Swan re-pointings"
    );
    assert_eq!(with(&blocks, "TICK_DEPENDS_ON_OBJ"), [42, 43, 57]);
    assert_eq!(
        spelled(&blocks, "TICK_DEPENDS_ON_OBJ"),
        [vec![int(54)], vec![int(54)], vec![int(56)]],
        "three dependency gates: two on the pirate zep's fall, one on block 56"
    );
    assert_eq!(with(&blocks, "IDENTITY"), [9, 11, 15, 18, 21]);
    assert_eq!(
        spelled(&blocks, "IDENTITY"),
        [
            vec![text("PRIMARY"), int(1), text("MSG_BRF_NWM1_OBJ1")],
            vec![text("PRIMARY"), int(2), text("MSG_BRF_NWM1_OBJ2")],
            vec![text("PRIMARY"), int(3), text("MSG_BRF_NWM1_OBJ3")],
            vec![text("PRIMARY"), int(4), text("MSG_BRF_NWM1_OBJ4")],
            vec![text("SECONDARY"), int(11)],
        ],
        "four PRIMARY objectives and one SECONDARY, the NWM1 briefing strings"
    );
    assert_eq!(with(&blocks, "SET_HELP_LABEL"), [2, 50]);
    assert_eq!(
        spelled(&blocks, "SET_HELP_LABEL"),
        [
            vec![
                ZrdValue::List(vec![text("piratezep"), text("rock_zeppelin")]),
                text(" "),
            ],
            vec![text("workersvoyagezep"), text(" ")],
        ],
        "the help label clears on the pirate zep at the start and on the voyage"
    );
    assert_eq!(with(&blocks, "WAKEUP_GENERATOR"), [9, 13, 27, 45]);
    assert_eq!(
        spelled(&blocks, "WAKEUP_GENERATOR"),
        [
            vec![text("workersvoyagezep"), int(3)],
            vec![text("blackswanzep"), int(2)],
            vec![text("workersvoyagezep"), int(3)],
            vec![text("blackswanzep"), int(4)],
        ]
    );
    assert_eq!(with(&blocks, "WAKEUP_ZEP_TURRETS"), [1, 5, 13]);
    assert_eq!(
        spelled(&blocks, "WAKEUP_ZEP_TURRETS"),
        [
            vec![text("piratezep")],
            vec![text("workersvoyagezep")],
            vec![text("blackswanzep")],
        ]
    );
    assert_eq!(with(&blocks, "WAKEUP_ENEMIES"), [13]);
    assert_eq!(
        spelled(&blocks, "WAKEUP_ENEMIES"),
        [vec![text("bsfury_1"), text("blackswanzep")]]
    );
    assert_eq!(with(&blocks, "COMPLETED_STOPPOINT"), [13]);
    assert_eq!(
        spelled(&blocks, "COMPLETED_STOPPOINT"),
        [vec![ZrdValue::List(vec![text("SwanZep1"), int(1), int(0)])]]
    );
    let kills: Vec<Vec<i64>> = spelled(&blocks, "KILL_OBJECTIVE_WHEN_I_COMPLETE")
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
        kills,
        [
            vec![41, 48],
            vec![42],
            vec![43],
            vec![29, 30, 31],
            vec![33, 34, 35],
            vec![44, 45],
            vec![58],
            vec![3],
            vec![57],
        ],
        "blocks 11 and 15 kill the Jack and Tex warning chains their boarding step cancels"
    );

    // --- passenger identity --------------------------------------------
    //
    // The boarding's human cargo is bound by name, not by a `passenger`
    // actor. Three `ANIM_STATE` sites gate the exchange the `wv_tailhook`
    // member defines: the copilot drops aboard (block 11), climbs back
    // (block 15) and the captured zep ends slaved to the Klondike (block 18).
    let anim_spec = |name: &str, state: &str| {
        vec![
            text("ANIM"),
            ZrdValue::List(vec![
                text("NAME"),
                ZrdValue::List(vec![text(name)]),
                text("STATE"),
                ZrdValue::List(vec![text(state)]),
            ]),
        ]
    };
    assert_eq!(with(&blocks, "ANIM_STATE"), [11, 15, 18]);
    assert_eq!(
        spelled(&blocks, "ANIM_STATE"),
        [
            anim_spec("wv_drop_copilot", "RUNNING"),
            anim_spec("wv_pickup_copilot", "EXECUTED"),
            anim_spec("hooked_to_klondike", "EXECUTED"),
        ],
        "the copilot exchange is spelled end to end in the record"
    );
    // The archive's largest member is the boarding feature itself and
    // declares the nodes and animations the record names.
    let hook = member_texts("wv_tailhook.zrd");
    for name in [
        "wv_tailhook",
        "peoplehook",
        "dropoff_node",
        "pickup_node",
        "activate_dropoff_node",
        "activate_pickup_node",
        "wv_drop_copilot",
        "wv_pickup_copilot",
    ] {
        assert!(
            hook.iter().any(|value| value == name),
            "wv_tailhook.zrd declares the boarding's {name}"
        );
    }
    // The crew member boards through the shared crew mechanism: the startup
    // event table fires `call_add_jack`, the library crew animation that
    // drives the world node `apassengers` — the contrast M06-B-FU4 (#1184)
    // measured between M06's data and M12's.
    let startup = member_texts("startanims.zrd");
    assert!(
        startup.iter().any(|value| value == "call_add_jack"),
        "the startup event table puts the crew member aboard"
    );
    // What the archive still does not bind: no actor member names a
    // passenger, and the only passenger-named string anywhere in it is still
    // `location.zrd`'s shared teleport entry — the same bytes the sibling
    // archives carry.
    let locations = member_texts("location.zrd");
    for name in LOCATIONS {
        assert!(
            locations.iter().any(|value| value == name),
            "location.zrd spells {name}"
        );
    }
    for member in [
        "aiv.zrd",
        "egen.zrd",
        "map.zrd",
        "mis_anim.zrd",
        "net.zrd",
        "objectives.zrd",
        "placezeps.zrd",
        "startanims.zrd",
        "weather.zrd",
        "wv_tailhook.zrd",
        "zeppelins.zrd",
    ] {
        let texts = member_texts(member);
        for name in LOCATIONS {
            assert!(
                !texts.iter().any(|value| value == name),
                "only location.zrd carries {name}: {member} does not"
            );
        }
    }
    // No control directive addresses a location either.
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
/// `INSTANTWIN` sits in block 24 and `INSTANTLOSS` in block 40; no other
/// block ends the mission. Both start dormant with no timed wake
/// (`BEGIN_DORMANT` `-1`), and of the 52 dormant markers only block 1 — the
/// mission start — arms a timed self-wake, so neither latch can fire on its
/// own clock.
///
/// The address walk covers 72 spelled integers — 27 wake, 15 kill, 27 nap
/// and 3 dependency gates — and every one lies in `1..=58`. The addressing
/// itself is not assumed here: M02-B-FU3 (#802) measured the original's
/// parse decrementing every objective address before storing it, so a
/// spelled number **is** the block number, and `cs_sim::objectives::address`
/// implements that rule (`[1, objectives]`, mapping to index
/// `address - 1`). M06's record discriminates the readings three ways and
/// each agrees with the measured rule: block 17 spells `58`, the block
/// count itself (the last block, in range — an index reading would call it
/// dangling); the success latch's only edge is block 18's spelled `24`,
/// which an index reading would land on OBJECTIVE25 while block 21's
/// SECONDARY wake of `23` would reach the latch; and the failure latch's
/// only edge is block 39's nap of `40`, which an index reading would land
/// on OBJECTIVE41 while block 38's nap of `39` would reach the latch a
/// countdown step early. The sibling suites that had read an address as an
/// index (M02-B, M04-B) were reconciled to the same measured convention by
/// **M06-B-FU3** (#819), which also pins the conversion itself under
/// `accept_objaddr_`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m06_b_the_terminal_blocks_are_gated_and_every_address_is_in_range() {
    let (document, _member) = read_control_member(&game_dir(), MISSION).expect("control member");
    let blocks = blocks_of(&document);

    let outcomes: Vec<u32> = blocks
        .iter()
        .filter(|(_, directives)| directives.iter().any(|d| d.key.starts_with("INSTANT")))
        .map(|(number, _)| *number)
        .collect();
    assert_eq!(outcomes, [24, 40], "no other block ends the mission");
    for (number, outcome) in [(24, "INSTANTWIN"), (40, "INSTANTLOSS")] {
        let directives = directives_of(&blocks, number);
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
    assert_eq!(dormant_count, 52);
    assert_eq!(
        timed_wakes,
        [1],
        "only the mission-start block arms a timed self-wake; the other 51 markers spell -1"
    );
    assert!(
        !timed_wakes.contains(&24) && !timed_wakes.contains(&40),
        "neither terminal latch may wake on its own clock: {timed_wakes:?}"
    );
    let undormant: Vec<u32> = blocks
        .iter()
        .filter(|(_, directives)| !directives.iter().any(|d| d.key == "BEGIN_DORMANT"))
        .map(|(number, _)| *number)
        .collect();
    assert_eq!(
        undormant,
        [2, 3, 6, 7, 36, 55],
        "six blocks are entered through a predecessor's edge rather than a dormant marker"
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
            ("KILL_OBJECTIVE_WHEN_I_COMPLETE", 15),
            ("NAP_OBJECTIVE_WHEN_I_COMPLETE", 27),
            ("TICK_DEPENDS_ON_OBJ", 3),
            ("WAKE_OBJECTIVE_WHEN_I_COMPLETE", 27),
        ]),
        "the address walk visits every spelled address: 27 wake, 15 kill, 27 nap, 3 gates"
    );
    assert!(
        out_of_range.is_empty(),
        "every address is a block of this record: {out_of_range:?}"
    );
    assert_eq!(
        highest,
        i64::from(BLOCKS),
        "block 17 spells 58 — the block count itself, which an index reading \
         would report as dangling"
    );

    // Who may fire the two latches: a completion edge names each of them, and
    // under the index reading both latches' edges land one block late — the
    // neighbours' spelled numbers would reach the latches instead.
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
    let naming = |target: i64| -> Vec<(u32, String, Vec<i64>)> {
        spelled_addresses
            .iter()
            .filter(|(_, _, addresses)| addresses.contains(&target))
            .cloned()
            .collect()
    };
    assert_eq!(
        naming(24),
        [(18, "WAKE_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![24])],
        "the success latch's only completion edge: the hookup step's own wake"
    );
    assert_eq!(
        naming(40),
        [(39, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![40])],
        "the failure latch's only completion edge: the fail countdown's last nap"
    );
    assert_eq!(
        naming(23),
        [(
            21,
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE".to_owned(),
            vec![22, 23]
        )],
        "under an index reading the SECONDARY block's wake would reach the success latch"
    );
    assert_eq!(
        naming(39),
        [(38, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![39])],
        "under an index reading block 38's nap would reach the failure latch a step early"
    );
}

/// **Every call binds, every condition lowers and M06's record completes.**
///
/// * **calls**: all 353 sites bind. The nine
///   `KILL_OBJECTIVE_WHEN_I_COMPLETE` sites are at most three targets wide —
///   inside [`MAX_CALL_ARGS`] either way — and the wide-list arm of
///   M02-B-FU1's mechanism (#800) stays pinned by the synthetic test below
///   and by the archive the corrected layout binds to M12, which spells
///   twelve-target kills. The three `ANIM_STATE` sites are each the
///   two-operand single-pair shape; the six-operand override shape the
///   follow-up measured lives on that same M12 record;
/// * **conditions**: all 58 block conditions lower — the copilot exchange's
///   three `ANIM_STATE` evaluators among them, each a single pair under its
///   own count.
///
/// The bound program reaches `MissionProgram::validate` and validates; the
/// row is complete. The condition-shape class this pin originally recorded
/// was filed as M04-B-FU1 (#806), and the follow-up's own instance — now on
/// the record the corrected layout binds to M12 — as M06-B-FU1 (#817).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m06_b_every_call_binds_every_condition_lowers_and_m06s_record_completes() {
    let row = census().row(MISSION).unwrap();
    let record = row.record().unwrap();

    let kill = record
        .key("KILL_OBJECTIVE_WHEN_I_COMPLETE")
        .expect("it is spelled");
    assert_eq!((kill.blocks, kill.sites), (9, 9));
    let mut kill_shapes: Vec<(usize, u32)> = kill
        .shapes
        .iter()
        .map(|(shape, sites)| (shape.arity(), *sites))
        .collect();
    kill_shapes.sort();
    assert_eq!(
        kill_shapes,
        [(1, 5), (2, 2), (3, 2)],
        "five single-target sites, two two-target ones and two three-target ones"
    );
    let longest = kill_shapes
        .iter()
        .map(|(arity, _)| *arity)
        .max()
        .unwrap_or(0);
    assert!(
        longest <= MAX_CALL_ARGS,
        "the widest kill is {longest} positional operands, inside {MAX_CALL_ARGS} — \
         the wide-list case lives on M12's record and the synthetic pin below"
    );

    let anim = record.key("ANIM_STATE").expect("it is spelled");
    assert_eq!((anim.blocks, anim.sites), (3, 3));
    let mut anim_shapes: Vec<(usize, u32)> = anim
        .shapes
        .iter()
        .map(|(shape, sites)| (shape.arity(), *sites))
        .collect();
    anim_shapes.sort();
    assert_eq!(
        anim_shapes,
        [(2, 3)],
        "three single-pair sites; M06's own record spells no COMPLETION_COUNT site"
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
        "a program stands for all 58 objectives"
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
        "all 58 block conditions lower: {refused_conditions:?}"
    );

    // The three ANIM_STATE evaluators: the copilot drops aboard (RUNNING),
    // climbs back (EXECUTED) and the zeppelin ends slaved to the Klondike
    // (EXECUTED) — each a single pair under its own count.
    let raw = attempt.raw_program().expect("the program assembled");
    for (index, name, state) in [
        (10usize, "wv_drop_copilot", AnimationState::Running),
        (14, "wv_pickup_copilot", AnimationState::Executed),
        (17, "hooked_to_klondike", AnimationState::Executed),
    ] {
        let (required, pairs) = animation_evaluator(&raw.objectives[index].condition);
        assert_eq!(
            (required, pairs.as_slice()),
            (1, &[(name.to_owned(), state)][..]),
            "block {}'s evaluator is its own spelled pair",
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

/// **The completion-count sites this follow-up measured still lower — on the
/// record the corrected layout binds to M12 — and M06's own record completes
/// without them.** (M06-B-FU1, #817; re-pinned on the corrected layout,
/// #1596)
///
/// `ANIM_STATE` is spelled at three sites of the measured archive's record,
/// blocks 9, 11 and 41 of `zbd/c2/m01`'s control member — each its block's
/// first directive, `COMPLETION_COUNT [1]` plus the two `ANIM` descriptors
/// of its own patrol path (`path3`, `path4`, `path5`
/// `continue`/`accelerate`, every one wanting `RUNNING`). The record itself
/// declares `snd_c3-HW-m2`, so the corrected campaign layout binds it to
/// M12, not M06 — and M06's own record spells `ANIM_STATE` only as
/// single-pair sites: the gap the follow-up pinned has no M06 instance.
///
/// The original's parse helper `0x4691d0` walks the whole operand list —
/// every `ANIM`/spec pair appends and the `COMPLETION_COUNT` found inside
/// the same list overwrites `required` — and the operand list is the
/// evaluator's one call argument, carried as a single `Value::List`. That is
/// the mechanism M04-B-FU1 (#806) measured in the decrypted image and
/// lowered for every mission; this test pins the measured sites where they
/// sit, on the record that spells them.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m06_b_fu1_the_completion_count_sites_lower_and_m06s_record_completes() {
    // M06's own record spells no override: its ANIM_STATE vocabulary is the
    // three single-pair sites the priorities pin above.
    let own = census().row(MISSION).unwrap();
    let anim = own
        .record()
        .unwrap()
        .key("ANIM_STATE")
        .expect("it is spelled");
    assert_eq!((anim.blocks, anim.sites), (3, 3));
    let mut own_shapes: Vec<(usize, u32)> = anim
        .shapes
        .iter()
        .map(|(shape, sites)| (shape.arity(), *sites))
        .collect();
    own_shapes.sort();
    assert_eq!(
        own_shapes,
        [(2, 3)],
        "M06's own record spells only the single-pair shape — the gap class has no M06 instance"
    );

    // The three sites the follow-up measured sit in the archive the
    // corrected layout binds to M12; pin them where they were measured.
    let binding = control_binding_of(FOLLOWUP_WORK_ORDER);
    assert_eq!(
        binding.program_asset, FOLLOWUP_CONTAINER,
        "the corrected layout binds the measured archive to M12"
    );
    let row = census().row(FOLLOWUP_ARCHIVE).unwrap();
    let record = row.record().unwrap();
    assert_eq!(
        (record.blocks(), record.sites()),
        (FOLLOWUP_BLOCKS, FOLLOWUP_SITES)
    );
    let anim = record.key("ANIM_STATE").expect("it is spelled");
    assert_eq!((anim.blocks, anim.sites), (8, 8));
    let mut shapes: Vec<(usize, u32)> = anim
        .shapes
        .iter()
        .map(|(shape, sites)| (shape.arity(), *sites))
        .collect();
    shapes.sort();
    assert_eq!(
        shapes,
        [(2, 5), (6, 3)],
        "five single-pair sites and the three completion-count sites"
    );

    // The three sites are each their block's first directive and spell the
    // record's own operand list — pinned here, never assumed.
    let (document, _member) =
        read_control_member(&game_dir(), FOLLOWUP_ARCHIVE).expect("control member");
    let blocks = blocks_of(&document);
    for (number, path) in [(9u32, "path3"), (11, "path4"), (41, "path5")] {
        let directives = directives_of(&blocks, number);
        assert_eq!(
            directives.first().map(|directive| directive.key.as_str()),
            Some("ANIM_STATE"),
            "block {number}'s ANIM_STATE is its first directive"
        );
        assert_eq!(
            directives[0].args.as_deref(),
            Some(measured_anim_state_operands(path).as_slice()),
            "block {number} spells the {path} pair under COMPLETION_COUNT [1]"
        );
    }

    let attempt = row.lowering_attempt().unwrap();
    let lowered = attempt.attempt();

    // Every site binds: the operand list is one argument, so a six-operand
    // site's width is a list length, never an arity.
    assert_eq!(lowered.calls.len() as u32, FOLLOWUP_SITES);
    assert!(
        lowered
            .calls
            .iter()
            .all(|call| matches!(call, CallOutcome::Bound)),
        "every site binds: {:?}",
        lowered.calls
    );
    assert!(
        lowered.unbound_keys.is_empty(),
        "the registry refused no key: {:?}",
        lowered.unbound_keys
    );

    // The three sites' lowered evaluators: both descriptors appended and the
    // in-list count overwriting `required` — the measured walk's result.
    let raw = attempt.raw_program().expect("the program assembled");
    for (index, path) in [(8usize, "path3"), (10, "path4"), (40, "path5")] {
        let (required, pairs) = animation_evaluator(&raw.objectives[index].condition);
        assert_eq!(
            (required, pairs.as_slice()),
            (
                1,
                &[
                    (format!("{path}_continue"), AnimationState::Running),
                    (format!("{path}_accelerate"), AnimationState::Running),
                ][..]
            ),
            "block {}'s evaluator is the spelled {path} pair under its own count",
            index + 1
        );
        let call = raw.objectives[index]
            .calls
            .iter()
            .find(|call| call.name == "ANIM_STATE")
            .expect("the site produced a call");
        assert!(
            matches!(call.args.as_slice(), [Value::List(items)] if items.len() == 6),
            "block {}'s operand list is the call's one argument: {:?}",
            index + 1,
            call.args
        );
    }

    // The bound actions keep the same nested list and the measured operation.
    let program = attempt.program().expect("the program bound");
    for index in [8usize, 10, 40] {
        let action = program.objectives[index]
            .actions
            .iter()
            .find(|action| {
                matches!(
                    action,
                    Action::Directive {
                        operation: IrOperation::AnimationStates,
                        ..
                    }
                )
            })
            .unwrap_or_else(|| panic!("block {} bound the measured operation", index + 1));
        let Action::Directive { args, .. } = action else {
            unreachable!()
        };
        assert!(
            matches!(args.as_slice(), [Value::List(items)] if items.len() == 6),
            "block {}'s action carries the operand list whole: {args:?}",
            index + 1
        );
    }

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
/// (M06-B-FU1)
///
/// M06 joins the complete rows through the census's own verdict — a measured
/// record, a complete lowering attempt and a validating program — never
/// through a side channel. The campaign gate stays closed on the other
/// missions' own gaps, so closing M06's widens nothing it should not.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m06_b_fu1_m06_is_complete_and_the_campaign_stays_unready() {
    let census = census();
    let row = census.row(MISSION).unwrap();
    assert!(row.is_measured(), "the program is measured");
    let lowering = row.lowering().unwrap();
    assert_eq!(
        lowering.unmet().count(),
        0,
        "the census's own accounting reports no unmet requirement"
    );
    assert!(lowering.complete(), "the lowering itself is complete");
    assert!(
        row.is_complete(),
        "the row's own verdict, not a side channel"
    );
    assert!(
        census.complete_missions().contains(&MISSION),
        "M06 joins the census's complete rows"
    );
    assert!(
        !census.campaign_ready(),
        "the campaign is still not ready — other missions carry their own gaps"
    );
}

// ---------------------------------------------------------------------------
// Synthetic: the measured lowering, and the arms that still refuse
// ---------------------------------------------------------------------------

/// An `ANIM_STATE` operand list: an optional `COMPLETION_COUNT` override, then
/// `pairs` `ANIM` descriptors — the two shapes the measured record spells
/// (2 and 6 operands).
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

/// One `ANIM` spec record as the measured three sites spell it:
/// `[NAME [<name>], STATE [RUNNING]]`.
fn anim_descriptor(name: &str) -> ZrdValue {
    ZrdValue::List(vec![
        text("NAME"),
        ZrdValue::List(vec![text(name)]),
        text("STATE"),
        ZrdValue::List(vec![text("RUNNING")]),
    ])
}

/// The six-operand `ANIM_STATE` spelling the follow-up measured (on the
/// record the corrected layout binds to M12): `COMPLETION_COUNT [1]` then
/// the path's `continue`/`accelerate` `RUNNING` descriptors — what blocks 9,
/// 11 and 41 each spell for `path3`, `path4` and `path5` (M06-B-FU1, #817).
fn measured_anim_state_operands(path: &str) -> Vec<ZrdValue> {
    vec![
        text("COMPLETION_COUNT"),
        ZrdValue::List(vec![int(1)]),
        text("ANIM"),
        anim_descriptor(&format!("{path}_continue")),
        text("ANIM"),
        anim_descriptor(&format!("{path}_accelerate")),
    ]
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

/// **A completion-count spelling lowers: both pairs append and the in-list
/// count overwrites `required`.**
///
/// The spelling the measured record's blocks 9, 11 and 41 use — a
/// `COMPLETION_COUNT [1]` plus two `ANIM` descriptors inside the one operand
/// list, six operands — lowers the measured way: the operand list is one
/// call argument, so the site binds inside the bound either way; every pair
/// appends; and the sibling count found in the same list overwrites
/// `required`.
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

    let override_shape = authored(anim_state_operands(2, Some(1)));
    let record = measure_control_record(&override_shape);
    let lowered = lower_control_record(mission, "zbd/synth/mission", &override_shape, &record);
    assert!(
        record.is_complete(lowered.attempt()),
        "the measured spelling lowers now: {:?}",
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
/// The retail half of the wide arm sits on the archive the corrected layout
/// binds to M12, whose record spells twelve-target
/// `KILL_OBJECTIVE_WHEN_I_COMPLETE` sites — twelve positional operands, past
/// [`MAX_CALL_ARGS`], bound through M02-B-FU1's (#800) list carrying;
/// authored the same way here, the record lowers completely. The bound
/// itself was never raised, so a key whose measured operation takes no list
/// argument still refuses its whole registration when its sites are wide
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

    // The widest kill site the measured record spells, authored that way.
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

/// **The measured three-site spelling lowers on an authored record.**
/// (M06-B-FU1)
///
/// The operand lists the measured record's blocks 9, 11 and 41 spell —
/// `COMPLETION_COUNT [1]` plus the path's `continue`/`accelerate` `RUNNING`
/// descriptors — are authored here verbatim, one site per block. Each lowers
/// to its own `AnimationStates { required: 1, … }`, each site binds its
/// operand list as one `Value::List` argument and the program validates —
/// the measured mechanism carried into CI, where there is no original data.
#[test]
fn accept_m06_b_fu1_m06s_spelling_lowers_all_three_sites() {
    let document = control_record(vec![
        block(
            1,
            vec![directive(
                "ANIM_STATE",
                measured_anim_state_operands("path3"),
            )],
        ),
        block(
            2,
            vec![directive(
                "ANIM_STATE",
                measured_anim_state_operands("path4"),
            )],
        ),
        block(
            3,
            vec![directive(
                "ANIM_STATE",
                measured_anim_state_operands("path5"),
            )],
        ),
    ]);
    let record = measure_control_record(&document);
    let lowered = lower(&document);
    assert!(
        record.is_complete(lowered.attempt()),
        "the three authored sites lower and the record completes: {:?}",
        lowered.attempt()
    );
    assert!(
        lowered.attempt().unbound_keys.is_empty(),
        "each operand list registers as one list argument"
    );
    assert_eq!(
        lowered.attempt().validation,
        Some(Vec::new()),
        "the bound program validates"
    );
    let raw = lowered.raw_program().expect("the program assembled");
    for (objective, path) in raw.objectives.iter().zip(["path3", "path4", "path5"]) {
        let (required, pairs) = animation_evaluator(&objective.condition);
        assert_eq!(
            (required, pairs.as_slice()),
            (
                1,
                &[
                    (format!("{path}_continue"), AnimationState::Running),
                    (format!("{path}_accelerate"), AnimationState::Running),
                ][..]
            ),
            "the block appends its own {path} pair under its own count"
        );
        let call = objective
            .calls
            .iter()
            .find(|call| call.name == "ANIM_STATE")
            .expect("the site produced a call");
        assert!(
            matches!(call.args.as_slice(), [Value::List(items)] if items.len() == 6),
            "the six operands reach the call as one list argument: {:?}",
            call.args
        );
    }
}
