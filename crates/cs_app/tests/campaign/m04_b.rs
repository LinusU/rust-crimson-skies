//! Acceptance stage M04-B: the mission-specific compatibility gaps of the
//! fourth mission (`missions/M04.md`, work order `M04-B`).
//!
//! M04-A bound M04's identities and left its program unmeasured. The
//! machinery that measures a control program (the `.zrd` reader, the census,
//! the directive dispositions, the record → `RawProgram` adapter and the
//! binding `SourceContext::control_program` adds on top of them) is shared
//! and was built for M01/M02; this stage runs all of it over M04's **own**
//! reader archive and pins what is *different* at M04, so the gaps are
//! recorded as measurements instead of surfacing later as a silent failure:
//!
//! * M04's control program is `objectives.zrd` with **52** numbered blocks
//!   and **201** directive sites across **40** keys, selected by the content
//!   rule and never by its name;
//! * every key has a disposition: two terminal outcomes and 38 measured
//!   effects, **no** unmeasured key, no refusal and no unclassified
//!   record-level key;
//! * the sheet's three regression priorities are located in the record with
//!   the actors the original spells — reveal triggers (`TRAVELERS`,
//!   `WAKEUP_ENEMIES`, the target-flag vocabulary), launch interruption
//!   (`START_TAXI`, `WAKE_ANIM`, `WAKEUP_GENERATOR`) and the protected
//!   carrier (`SET_HELP_LABEL … MSG_OBJ_DEFEND`, `DEDG`, and the two
//!   `INACTIVE_COMPLETION_COUNT` thresholds) — and every key is asserted to
//!   resolve to the operation the shared findings measured;
//! * both terminal latches are gated and every cross-objective address the
//!   record spells is a block of this record;
//! * the record does **not** lower: `ANIM_STATE` is refused in both halves of
//!   the lowering (its 18-operand shape is past the host-call bound, so the
//!   key never registers, and the two multi-pair sites are not the measured
//!   single-pair shape the condition parser accepts), so no `MissionProgram`
//!   stands and M04 is not ready.
//!
//! No behaviour is invented here. The runtime halves of the sheet's
//! priorities — the wrong actor, the wrong session, a repeated event — need
//! ordinary play (M04-C) and stay open; nothing in this file simulates them.
//! The `ANIM_STATE` gap is filed as a follow-up task and recorded in
//! `docs/findings/2026-10-08-m04-b-compatibility-gaps.md`.
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
use cs_content::campaign_bindings::{MissionControlBinding, MissionLabel, SourceContext};
use cs_content::mission_control::{
    CallOutcome, ConditionOutcome, DirectiveDisposition, DirectiveOperation, DirectiveShape,
    TerminalOutcome, measure_control_record,
};
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{ZrdValue, objective_record, zrd_flat_fields};
use cs_script::bindings::MAX_CALL_ARGS;
use cs_types::content::{ContentId, ContentKind};

use crate::common::load_inventory;

/// The census row label of the mission.
pub(crate) const MISSION: &str = "zbd/c1/m04";

/// The reader archive M04-A bound as the mission's program.
const CONTAINER: &str = "ZBD/C1/M04/zrdr.zbd";
/// SHA-256 of that whole archive, from production discovery.
const CONTAINER_SHA256: &str = "5ffc1abd91a5919179448b397ec6bde48dd19fd8ba6f311b4362d76cb84c4263";
/// The member the block-carrying rule picks (never a filename constant).
const CONTROL_MEMBER: &str = "objectives.zrd";
/// SHA-256 of the control member's own bytes.
const CONTROL_SHA256: &str = "0bf89dc73fca3b9d7746ccd66821281044d0899305315a98e454baf3bb12fb5a";

/// The numbered blocks of the control member.
const BLOCKS: u32 = 52;
/// The directive sites of the control member.
const SITES: u32 = 201;
/// The distinct directive keys of the control member.
const KEYS: usize = 40;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M04-B needs the retail capability; run this suite with \
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
            .find(|(label, _)| label.as_str() == "M04")
            .map(|(_, title)| title.clone())
            .expect("the declared inventory has an M04 work order");
        let context = SourceContext::read(&game_dir())
            .expect("production source context reads the installation");
        context
            .control_program(
                MissionLabel::new("M04").expect("M04 is a valid label"),
                &title,
            )
            .expect("M04's control program binds through the measured rule")
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
        let children = value.as_list().expect("every M04 block is a list");
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
    directive
        .args
        .iter()
        .flatten()
        .filter_map(|value| match value {
            ZrdValue::Int(int) => Some(i64::from(*int)),
            _ => None,
        })
        .collect()
}

/// The cross-objective **block addresses** a directive spells.
///
/// The measured effects split the two children of a nap: child0 is the
/// targeted block index and child1 is the number of seconds after which the
/// target re-wakes
/// (`docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`),
/// so a nap contributes one address while a wake or a kill contributes every
/// integer it spells. Taking the nap's seconds for a range check would report
/// a dangling block that the record never addresses.
fn addresses(directive: &Directive) -> Vec<i64> {
    let ints = arguments(directive);
    if directive.key == "NAP_OBJECTIVE_WHEN_I_COMPLETE" {
        ints.into_iter().take(1).collect()
    } else {
        ints
    }
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
        ContentId::from_source(ContentKind::Mission, "accept-m04-b").map_err(|e| e.to_string()),
        "accept-m04-b",
        document,
        &record,
    )
}

// ---------------------------------------------------------------------------
// Retail: what M04's control program is
// ---------------------------------------------------------------------------

/// **The control program is the member that declares the numbered blocks.**
///
/// The member is found by the content rule: of the 15 members of M04's reader
/// archive exactly one declares numbered `OBJECTIVE<N>` blocks. Neither size
/// nor position decides it — the chosen member is the eighth, and longer
/// members sit beside it. The blocks and sites the census measures equal an
/// independent walk of the same document, the archive is the program span
/// M04-A bound, and the production control binding reaches the same member,
/// span and digests through its own walk.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m04_b_the_control_program_is_the_member_that_declares_the_blocks() {
    let row = census().row(MISSION).expect("M04 is in the census");
    assert_eq!(row.container, CONTAINER);
    assert_eq!(
        row.container_sha256, CONTAINER_SHA256,
        "the reader archive is the program M04-A bound"
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
    assert!(
        !longer.is_empty(),
        "size is not the rule: {} member(s) are longer than the control member",
        longer.len()
    );

    let record = row.record().expect("M04 has a measured control program");
    assert_eq!((record.blocks(), record.sites()), (BLOCKS, SITES));
    assert_eq!(record.keys().len(), KEYS);

    let (document, member) = read_control_member(&game_dir(), MISSION)
        .expect("the rule finds M04's control member again");
    assert_eq!(member.name, CONTROL_MEMBER);
    assert_eq!(member.objective_blocks, BLOCKS);
    assert!(member.is_control);

    let blocks = blocks_of(&document);
    let numbers: Vec<u32> = blocks.iter().map(|(number, _)| *number).collect();
    assert_eq!(
        numbers,
        (1..=BLOCKS).collect::<Vec<_>>(),
        "numbered 1..=52, no gaps"
    );
    let walked: usize = blocks.iter().map(|(_, directives)| directives.len()).sum();
    assert_eq!(walked as u32, record.sites(), "the independent walk agrees");
    assert!(
        record.refusals().is_empty(),
        "every block is a readable list"
    );
    assert!(
        record.unclassified_record_keys().is_empty(),
        "M04 spells no record-level key outside the measured vocabulary"
    );

    // The production binding and the census must not disagree about the
    // mission, the program, the member or the record.
    let bound = control_binding();
    assert_eq!(bound.mission.as_str(), "mission/ch1-m04");
    assert_eq!(bound.program_id.as_str(), "script/c1-m04-zrdr");
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

/// **Every directive key M04 spells has exactly one disposition, and none is
/// refused.**
///
/// Two keys are terminal outcomes (`INSTANTWIN`, `INSTANTLOSS`, one bare site
/// each), the other 38 have a measured effect, and no key is Unmeasured. The
/// sites are accounted for: the keys' sites sum to the record's.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m04_b_every_directive_m04_spells_has_a_disposition_and_none_is_refused() {
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
        "M04's vocabulary is fully measured: {refused:?}"
    );
    assert_eq!(measured, 38);
    let sites: u32 = record.keys().iter().map(|key| key.sites).sum();
    assert_eq!(sites, SITES, "no site is dropped from the accounting");

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
/// * **reveal triggers**: two `TRAVELERS` proximity boundaries on `player`
///   about `piratezep` (1500 and 500), the two `WAKEUP_ENEMIES` sites, and the
///   target-flag vocabulary that makes an object appear on the target list;
/// * **launch interruption**: four `START_TAXI` releases of the parked
///   `blakepeace_2_*` vehicles, the two `WAKE_ANIM` sites and the two
///   `WAKEUP_GENERATOR` feeds;
/// * **protected carrier**: `SET_HELP_LABEL … MSG_OBJ_DEFEND` over
///   `piratezep` and `rock_zeppelin`, seven `DEDG` group-depletion
///   thresholds, and the two `INACTIVE_COMPLETION_COUNT` thresholds that let
///   3 (and 6) of twelve listed members be cleared instead of all of them.
///
/// Every key is asserted to resolve to the operation the shared finding
/// measured, so a key that silently lost its meaning fails here.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m04_b_the_sheet_priorities_are_located_and_resolve_to_measured_operations() {
    let row = census().row(MISSION).unwrap();
    let record = row.record().unwrap();
    let operation = |key: &str| match record.key(key).unwrap().disposition() {
        DirectiveDisposition::Measured(measured) => measured.operation,
        other => panic!("{key} is not measured: {other:?}"),
    };

    // Reveal triggers.
    assert_eq!(operation("TRAVELERS"), DirectiveOperation::Travelers);
    assert_eq!(operation("WAKEUP_ENEMIES"), DirectiveOperation::WakeEnemies);
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
    // Launch interruption.
    assert_eq!(operation("START_TAXI"), DirectiveOperation::ReleaseTaxi);
    assert_eq!(operation("WAKE_ANIM"), DirectiveOperation::WakeAnimation);
    assert_eq!(
        operation("WAKEUP_GENERATOR"),
        DirectiveOperation::FeedGenerator
    );
    // Protected carrier.
    assert_eq!(
        operation("SET_HELP_LABEL"),
        DirectiveOperation::SetHelpLabel
    );
    assert_eq!(operation("DEDG"), DirectiveOperation::EnemyGroupDepletion);
    assert_eq!(
        operation("INACTIVE_COMPLETION_COUNT"),
        DirectiveOperation::InactiveThreshold
    );
    assert_eq!(operation("INACTIVE1"), DirectiveOperation::InactiveMembers);
    // And the gates the mission's ordering leans on.
    assert_eq!(
        operation("TICK_DEPENDS_ON_OBJ"),
        DirectiveOperation::DependencyGate
    );
    assert_eq!(operation("BEGIN_DORMANT"), DirectiveOperation::DormantStart);

    let (document, _member) = read_control_member(&game_dir(), MISSION).expect("control member");
    let blocks = blocks_of(&document);

    // Reveal triggers, with the actors and boundaries the record spells.
    assert_eq!(with(&blocks, "TRAVELERS"), [47, 52]);
    assert_eq!(
        spelled(&blocks, "TRAVELERS"),
        [
            vec![
                text("player"),
                text("APPROACHING"),
                text("piratezep"),
                ZrdValue::Float(1500.0),
                int(1)
            ],
            vec![
                text("player"),
                text("APPROACHING"),
                text("piratezep"),
                ZrdValue::Float(500.0),
                int(1)
            ],
        ]
    );
    assert_eq!(with(&blocks, "WAKEUP_ENEMIES"), [20, 24]);
    assert_eq!(with(&blocks, "ADD_OBJECTIVE_TARGET"), [23, 31]);
    assert_eq!(with(&blocks, "REMOVE_OBJECTIVE_TARGET"), [20, 25, 30, 33]);
    assert_eq!(with(&blocks, "ADD_OTHER_TARGET"), [25]);
    assert_eq!(with(&blocks, "REMOVE_OTHER_TARGET"), [23]);

    // Launch interruption.
    assert_eq!(with(&blocks, "START_TAXI"), [8, 9, 10, 11]);
    let taxis: Vec<String> = spelled(&blocks, "START_TAXI")
        .iter()
        .map(|args| match args.as_slice() {
            [ZrdValue::Text(name)] => name.clone(),
            other => panic!("a START_TAXI site spells {other:?}"),
        })
        .collect();
    assert_eq!(
        taxis,
        [
            "blakepeace_2_3".to_owned(),
            "blakepeace_2_5".to_owned(),
            "blakepeace_2_4".to_owned(),
            "blakepeace_2_6".to_owned(),
        ]
    );
    assert_eq!(with(&blocks, "WAKE_ANIM"), [1, 32]);
    assert_eq!(
        spelled(&blocks, "WAKE_ANIM"),
        [vec![text("hangar3_doors")], vec![text("pzhomebase")],]
    );
    assert_eq!(with(&blocks, "WAKEUP_GENERATOR"), [12, 13]);

    // Protected carrier.
    assert_eq!(with(&blocks, "SET_HELP_LABEL"), [23, 25]);
    assert_eq!(
        spelled(&blocks, "SET_HELP_LABEL")[0],
        vec![
            ZrdValue::List(vec![text("piratezep"), text("rock_zeppelin")]),
            text("MSG_OBJ_DEFEND")
        ],
        "the help label the defend objective posts names both carriers"
    );
    assert_eq!(with(&blocks, "DEDG"), [7, 22, 25, 28, 42, 43, 44]);
    let dedg: Vec<Vec<i64>> = spelled(&blocks, "DEDG")
        .iter()
        .map(|args| {
            args.iter()
                .filter_map(|v| match v {
                    ZrdValue::Int(int) => Some(i64::from(*int)),
                    _ => None,
                })
                .collect()
        })
        .collect();
    assert_eq!(
        dedg,
        [
            vec![5, 2],
            vec![1, 0],
            vec![2, 0],
            vec![2, 2],
            vec![1, 0],
            vec![2, 0],
            vec![5, 0],
        ],
        "each threshold is record data: a group with N members left, not a guess"
    );
    assert_eq!(with(&blocks, "INACTIVE_COMPLETION_COUNT"), [26, 27]);
    let thresholds: Vec<i64> = spelled(&blocks, "INACTIVE_COMPLETION_COUNT")
        .iter()
        .map(|args| match args.as_slice() {
            [ZrdValue::Int(number)] => i64::from(*number),
            other => panic!("an INACTIVE_COMPLETION_COUNT site spells {other:?}"),
        })
        .collect();
    assert_eq!(thresholds, [3, 6]);
    for (number, wanted) in [(26u32, 3usize), (27u32, 6usize)] {
        let directives = &blocks
            .iter()
            .find(|(n, _)| *n == number)
            .unwrap_or_else(|| panic!("block {number} exists"))
            .1;
        let listed = directives
            .iter()
            .filter(|d| d.key.starts_with("INACTIVE") && d.key != "INACTIVE_COMPLETION_COUNT")
            .collect::<Vec<_>>();
        let listed = listed.len();
        assert_eq!(
            listed, 12,
            "block {number} lists twelve members and needs {wanted} of them cleared"
        );
        for directive in directives {
            if !directive.key.starts_with("INACTIVE")
                || directive.key == "INACTIVE_COMPLETION_COUNT"
            {
                continue;
            }
            let chain = directive.args.as_deref().unwrap_or(&[]);
            assert!(
                chain.len() >= 2,
                "block {number} {} spells a name chain: {chain:?}",
                directive.key
            );
            assert_eq!(
                chain.first(),
                Some(&text("piratezep")),
                "every listed member hangs off the protected carrier"
            );
        }
        assert!(wanted <= listed, "the threshold never exceeds the list");
    }
    let travelers = match record.key("TRAVELERS").unwrap().disposition() {
        DirectiveDisposition::Measured(measured) => measured,
        other => panic!("TRAVELERS is not measured: {other:?}"),
    };
    assert_eq!(
        travelers.unknowns.len(),
        3,
        "the TRAVELERS polarity, mode and group-0 unknowns are still recorded, never dropped"
    );
}

/// **Both terminal latches are gated, and every block address the record
/// spells is a block of this record.**
///
/// `INSTANTWIN` sits in block 32 and `INSTANTLOSS` in block 41; no other block
/// ends the mission. Both start dormant with no timed wake (`BEGIN_DORMANT`
/// `-1`), and of the 47 dormant markers only blocks 1 and 2 — the mission
/// start — arm a timed self-wake, so neither latch can fire on its own clock.
/// Block 32 is named by exactly one other block, 44, through a nap; block 41
/// by block 20's wake. Every wake, kill, nap and gate address lies in
/// `1..=52`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m04_b_the_terminal_blocks_are_gated_and_every_address_is_in_range() {
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
    assert_eq!(outcomes, [32, 41], "no other block ends the mission");
    for (number, outcome) in [(32, "INSTANTWIN"), (41, "INSTANTLOSS")] {
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
    assert_eq!(dormant_count, 47);
    assert_eq!(
        timed_wakes,
        [1, 2],
        "only the two mission-start blocks arm a timed self-wake; the other 45 markers spell -1"
    );
    assert!(
        !timed_wakes.contains(&32) && !timed_wakes.contains(&41),
        "neither terminal latch may wake on its own clock: {timed_wakes:?}"
    );
    let undormant: Vec<u32> = blocks
        .iter()
        .filter(|(_, directives)| !directives.iter().any(|d| d.key == "BEGIN_DORMANT"))
        .map(|(number, _)| *number)
        .collect();
    assert_eq!(undormant, [7, 23, 26, 30, 33]);

    // The addresses, walked with the measured child rules.
    let mut edges: BTreeMap<&str, u32> = BTreeMap::new();
    let mut out_of_range = Vec::new();
    for (number, directives) in &blocks {
        for directive in directives {
            if !addresses_blocks(directive) {
                continue;
            }
            for address in addresses(directive) {
                *edges.entry(directive.key.as_str()).or_default() += 1;
                if !(1..=i64::from(BLOCKS)).contains(&address) {
                    out_of_range.push((*number, directive.key.clone(), address));
                }
            }
        }
    }
    assert_eq!(
        edges,
        BTreeMap::from([
            ("KILL_OBJECTIVE_WHEN_I_COMPLETE", 7),
            ("NAP_OBJECTIVE_WHEN_I_COMPLETE", 23),
            ("TICK_DEPENDS_ON_OBJ", 4),
            ("WAKE_OBJECTIVE_WHEN_I_COMPLETE", 24),
        ]),
        "the address walk visits every spelled address: 24 wake, 7 kill, 23 nap, 4 gate"
    );
    assert!(
        out_of_range.is_empty(),
        "every address is a block of this record: {out_of_range:?}"
    );

    // Who may fire the two latches: only a completion edge names them.
    let incoming = |target: u32| -> Vec<(u32, String, Vec<i64>)> {
        blocks
            .iter()
            .flat_map(|(number, directives)| {
                directives
                    .iter()
                    .filter(|d| {
                        d.key.ends_with("_OBJECTIVE_WHEN_I_COMPLETE")
                            && addresses(d).contains(&(i64::from(target) - 1))
                    })
                    .map(|d| (*number, d.key.clone(), addresses(d)))
            })
            .collect()
    };
    assert_eq!(
        incoming(32),
        [(44, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(), vec![31])],
        "the success latch has in-degree one and cannot fire early"
    );
    assert_eq!(
        incoming(41),
        [(
            20,
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE".to_owned(),
            vec![22, 40, 50]
        )],
        "the failure latch is woken by exactly one block"
    );
    let gates: Vec<u32> = blocks
        .iter()
        .flat_map(|(number, directives)| {
            directives
                .iter()
                .filter(|d| d.key == "TICK_DEPENDS_ON_OBJ")
                .map(|d| (*number, addresses(d)))
                .filter(|(_, targets)| targets.contains(&(i64::from(41) - 1)))
                .map(|(number, _)| number)
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(
        gates,
        [42],
        "block 42 runs only while the failure latch's block is awake — a gate, not a wake"
    );
}

/// **`ANIM_STATE` is M04's only compatibility gap, and the record does not
/// lower.**
///
/// The key spells three sites in two shapes: one single-pair site (2
/// operands) and two multi-pair sites of 18 operands — a leading
/// `COMPLETION_COUNT` list plus eight `ANIM` descriptors. Both halves of the
/// lowering refuse them:
///
/// * **calls**: `registry_for` registers one signature per measured shape, and
///   the 18-operand shape is past [`MAX_CALL_ARGS`], so the whole key fails
///   registration and all three sites refuse as `unknown host call`;
/// * **conditions**: the measured condition parser takes exactly the tag `ANIM`
///   and one spec record, so the two 18-operand sites refuse and 50 of 52
///   blocks lower.
///
/// `call_arguments` and `objective_condition` are the only unmet requirements,
/// no program stood to be validated, and the row is not complete.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m04_b_anims_state_is_the_only_gap_and_the_record_does_not_lower() {
    let row = census().row(MISSION).unwrap();

    let key = row
        .record()
        .unwrap()
        .key("ANIM_STATE")
        .expect("it is spelled");
    assert_eq!((key.blocks, key.sites), (3, 3));
    let mut shapes: Vec<(usize, u32)> = key
        .shapes
        .iter()
        .map(|(shape, sites)| (shape.arity(), *sites))
        .collect();
    shapes.sort();
    assert_eq!(
        shapes,
        [(2, 1), (18, 2)],
        "two multi-pair sites share one shape and the single-pair site spells the other"
    );
    let longest = shapes.iter().map(|(arity, _)| *arity).max().unwrap_or(0);
    assert!(
        longest > MAX_CALL_ARGS,
        "the multi-pair shape has {longest} operands, past the registry's bound of {MAX_CALL_ARGS}"
    );

    let attempt = row.lowering_attempt().unwrap();
    let lowered = attempt.attempt();
    assert_eq!(lowered.mission.as_deref(), Ok("mission/ch1-m04"));
    assert_eq!(lowered.objectives, BLOCKS);
    assert_eq!(lowered.calls.len() as u32, SITES);
    let (bound, refused): (usize, Vec<(usize, &str)>) = (
        lowered
            .calls
            .iter()
            .filter(|call| matches!(call, CallOutcome::Bound))
            .count(),
        lowered
            .calls
            .iter()
            .enumerate()
            .filter_map(|(index, call)| match call {
                CallOutcome::Refused(text) => Some((index, text.as_str())),
                CallOutcome::Bound => None,
            })
            .collect(),
    );
    assert_eq!(bound, 198, "every site but the three ANIM_STATE ones binds");
    assert_eq!(refused.len(), 3);
    for (index, text) in &refused {
        assert!(
            text.contains("unknown host call `ANIM_STATE`"),
            "call {index} refuses on ANIM_STATE: {text}"
        );
    }
    assert_eq!(
        refused.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
        [77, 149, 160],
        "objective#22, objective#31 and objective#36 — the three blocks that spell the key"
    );
    assert_eq!(
        lowered.unbound_keys,
        ["`ANIM_STATE`: binding `ANIM_STATE`: too many arguments"],
        "the registry refused ANIM_STATE at registration, on the host-call bound"
    );
    assert!(
        attempt.program().is_none(),
        "no bound program stands when a site refuses"
    );
    assert!(
        lowered.validation.is_none(),
        "no program stood to be validated"
    );

    let conditions = &lowered.conditions;
    assert_eq!(conditions.len() as u32, BLOCKS);
    let refused_conditions: Vec<(usize, &str)> = conditions
        .iter()
        .enumerate()
        .filter_map(|(index, outcome)| match outcome {
            ConditionOutcome::Refused(text) | ConditionOutcome::Unreadable(text) => {
                Some((index, text.as_str()))
            }
            ConditionOutcome::Lowered => None,
        })
        .collect();
    assert_eq!(
        refused_conditions.len(),
        2,
        "50 of 52 block conditions lower: {refused_conditions:?}"
    );
    assert_eq!(
        refused_conditions
            .iter()
            .map(|(index, _)| *index + 1)
            .collect::<Vec<_>>(),
        [23, 37],
        "the two blocks that spell the multi-pair ANIM_STATE form"
    );
    for (index, text) in &refused_conditions {
        assert!(
            text.contains("`ANIM_STATE`")
                && text.contains("the measured shape is the tag `ANIM` and one spec record"),
            "condition {index} refuses on ANIM_STATE's operands: {text}"
        );
    }

    let lowering = row.lowering().unwrap();
    let unmet: Vec<&str> = lowering.unmet().map(|row| row.kind.code()).collect();
    assert_eq!(
        unmet,
        ["objective_condition", "call_arguments"],
        "the mission identity and all 52 objective identities are met"
    );
    assert!(!lowering.complete());
    assert!(!row.is_complete());
}

/// **M04 is not campaign-ready and the census does not hide it.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m04_b_the_mission_stays_unready_until_anims_state_is_measured() {
    let census = census();
    assert!(!census.complete_missions().contains(&MISSION));
    assert!(!census.campaign_ready());
    assert!(census.measured_rows().any(|row| row.mission() == MISSION));
    let row = census.row(MISSION).unwrap();
    assert!(
        row.is_measured(),
        "the program is measured even though it does not lower"
    );
}

// ---------------------------------------------------------------------------
// Synthetic: the two refusal arms the retail gap rests on
// ---------------------------------------------------------------------------

/// An `ANIM_STATE` operand list: an optional `COMPLETION_COUNT` override,
/// then `pairs` `ANIM` descriptors — the shape M04 spells.
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
            ZrdValue::List(vec![text("EXECUTED")]),
        ]));
    }
    operands
}

/// **A single animation pair lowers and a multi-pair site is refused.**
///
/// The condition parser's measured shape is the tag `ANIM` and one spec
/// record. Authored that way, the block's condition lowers and the record
/// completes; adding a `COMPLETION_COUNT` override and a second descriptor in
/// the same list — the spelling M04's blocks 23 and 37 use — is refused per
/// site by the condition accounting, with the measured shape named, and the
/// record does not complete. Nothing about the key itself changed between the
/// two, so the refusal is the shape.
#[test]
fn accept_m04_b_a_single_animation_pair_lowers_and_a_multi_pair_site_is_refused() {
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
        "one pair is the measured shape and the record completes: {:?}",
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

    let multi = authored(anim_state_operands(2, Some(1)));
    let record = measure_control_record(&multi);
    let lowered = lower_control_record(mission, "zbd/synth/mission", &multi, &record);
    assert!(
        !record.is_complete(lowered.attempt()),
        "the multi-pair spelling refuses: {:?}",
        lowered.attempt().conditions
    );
    let refused: Vec<&str> = lowered
        .attempt()
        .conditions
        .iter()
        .filter_map(|outcome| match outcome {
            ConditionOutcome::Refused(text) | ConditionOutcome::Unreadable(text) => {
                Some(text.as_str())
            }
            ConditionOutcome::Lowered => None,
        })
        .collect();
    assert_eq!(refused.len(), 1, "the one block refuses: {refused:?}");
    assert!(
        refused[0].contains("`ANIM_STATE`")
            && refused[0].contains("the measured shape is the tag `ANIM` and one spec record"),
        "{}",
        refused[0]
    );
    assert!(
        lowered.attempt().unbound_keys.is_empty(),
        "four operands are inside the host-call bound, so the key still registers"
    );
}

/// **An `ANIM_STATE` site past the host-call bound refuses the whole key; one
/// at the bound binds.**
///
/// This is the registration half of M04's gap: the retail key's 18-operand
/// shape is over the bound, so the whole spec refuses and all three retail
/// sites refuse with it. Here the same mechanism is proved on authored
/// records at the two sides of [`MAX_CALL_ARGS`], so CI covers the refusal arm
/// without original data.
#[test]
fn accept_m04_b_an_anim_state_site_past_the_host_call_bound_refuses_the_whole_key() {
    let authored = |pairs: usize| {
        control_record(vec![block(
            1,
            vec![
                directive("BEGIN_DORMANT", vec![ZrdValue::Float(-1.0)]),
                directive("ANIM_STATE", anim_state_operands(pairs, Some(1))),
            ],
        )])
    };
    // `COMPLETION_COUNT [n]` is two operands, so `pairs` descriptors add
    // two each: 3 pairs sit at the bound of 8, 4 pairs are one over it.
    let at_bound_pairs = (MAX_CALL_ARGS - 2) / 2;
    let over_bound_pairs = at_bound_pairs + 1;

    let at_bound = authored(at_bound_pairs);
    let record = measure_control_record(&at_bound);
    let lowered = lower(&at_bound);
    assert!(
        lowered.attempt().unbound_keys.is_empty(),
        "{} operands are at the bound of {MAX_CALL_ARGS} and register: {:?}",
        2 + 2 * at_bound_pairs,
        lowered.attempt().unbound_keys
    );
    assert!(
        record.key("ANIM_STATE").is_some(),
        "the key is measured either way"
    );

    let over_bound = authored(over_bound_pairs);
    let lowered = lower(&over_bound);
    assert_eq!(
        lowered.attempt().unbound_keys.len(),
        1,
        "the over-bound signature refuses the key's registration"
    );
    assert!(
        lowered.attempt().unbound_keys[0].contains("ANIM_STATE")
            && lowered.attempt().unbound_keys[0].contains("too many arguments"),
        "{}",
        lowered.attempt().unbound_keys[0]
    );
    assert!(
        lowered
            .attempt()
            .calls
            .iter()
            .filter(|call| matches!(call, CallOutcome::Refused(_)))
            .count()
            == 1,
        "only the ANIM_STATE site refuses: {:?}",
        lowered.attempt().calls
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
        refusals[0].contains("unknown host call `ANIM_STATE`"),
        "{}",
        refusals[0]
    );
    assert!(
        lowered.registry().get("ANIM_STATE").is_none(),
        "the over-bound key is not in the registry"
    );
    assert!(
        !measure_control_record(&over_bound).is_complete(lowered.attempt()),
        "an over-bound site refuses its whole record, never a narrowed binding"
    );
}
