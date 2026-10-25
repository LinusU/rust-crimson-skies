//! Acceptance stage M14-B: the mission-specific compatibility gaps of the
//! fourteenth mission (`missions/M14.md`, work order `M14-B`).
//!
//! M14-A bound M14's identities (`missions/bindings/M14.json`: `mission/ch3-m04`,
//! `script/c2b-m04-zrdr`, `ZBD/C2B/M04/zrdr.zbd`). The machinery that measures a
//! control program is shared and was built for M01; this stage runs it over
//! M14's own reader archive and pins what is **different** at M14.
//!
//! M14's record is `objectives.zrd`: 35 numbered blocks, 254 directive sites,
//! 39 distinct keys. All 39 keys are measured or terminal (none refused) and the
//! row lowers completely. The one gap was `OBJECTIVE25`'s `TRAVELERS`, which
//! spells its radius as the int `1000`; M14-B-FU1 made `cs_script` read it as the
//! original does (`docs/findings/2026-10-25-m14-b-fu1-travelers-int-radius.md`).
//! The sheet's three priorities are pinned where they live in the record:
//!
//! * **exposed weapon bays:** the `ANIM_STATE` blocks 11, 12 and 13 over the six
//!   `deploy_gmzep_lbroad*` animations of the Gemini zeppelin (completing on 1,
//!   3 and 5 of them), and block 14 waiting for `hooked_to_klondike`;
//! * **capital survival:** the `INACTIVE*` health blocks over both zeppelins'
//!   engines (7-10 on `geminizep`, 15-18 on `piratezep`) and gas bags (19-21),
//!   the `DEDG` depletions, the `TRAVELERS` approach test and the loss block 22;
//! * **chapter transition:** the win block 32 (`INSTANTWIN`), armed only by block
//!   14 through a nap. The record names no successor mission.
//!
//! No behaviour is invented here (see
//! `docs/findings/2026-10-25-m14-b-compatibility-gaps.md`). The retail tests are
//! `#[ignore = "requires CS_GAME_DIR"]`; the synthetic ones run in CI.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_app::control_lowering::lower_control_record;
use cs_app::mission_control::{
    RetailControlCensus, read_control_member, survey_mission_control_programs,
};
use cs_assets::install::sha256;
use cs_content::campaign_bindings::{MissionControlBinding, MissionLabel, SourceContext};
use cs_content::mission_control::{
    CallOutcome, ConditionOutcome, DirectiveDisposition, DirectiveOperation, TerminalOutcome,
    measure_control_record,
};
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{ZrdValue, objective_record, zrd_flat_fields};
use cs_types::content::{ContentId, ContentKind};

use crate::campaign::common::load_inventory;

/// The census row label of the mission.
pub(crate) const MISSION: &str = "zbd/c2b/m04";

/// The reader archive `missions/bindings/M14.json` binds as the program.
const CONTAINER: &str = "ZBD/C2B/M04/zrdr.zbd";
/// SHA-256 of that whole archive, from production discovery.
const CONTAINER_SHA256: &str = "1ba5b75637e72d5c569cb50b45589f4d63482a256fae66ecc750c08bb0e4145f";
/// The member the block-carrying rule picks (never a filename constant).
const CONTROL_MEMBER: &str = "objectives.zrd";

/// The numbered blocks of the control member.
const BLOCKS: u32 = 35;
/// The directive sites of the control member.
const SITES: u32 = 254;
/// The distinct directive keys of the control member.
const KEYS: usize = 39;
/// The spelled wake/kill/nap addresses (16 wake, 9 kill, 13 nap).
const EDGES: usize = 38;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M14-B needs the retail capability; run this suite with \
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
            .find(|(label, _)| label.as_str() == "M14")
            .map(|(_, title)| title.clone())
            .expect("the declared inventory has an M14 work order");
        let context = SourceContext::read(&game_dir())
            .expect("production source context reads the installation");
        context
            .control_program(
                MissionLabel::new("M14").expect("M14 is a valid label"),
                &title,
            )
            .expect("M14's control program binds through the measured rule")
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
        let children = value.as_list().expect("every M14 block is a list");
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
    blocks
}

fn control_document() -> ZrdValue {
    read_control_member(&game_dir(), MISSION)
        .expect("the rule finds M14's control member")
        .0
}

/// The integer addresses a directive spells in its argument list.
fn addresses(directive: &Directive) -> Vec<i64> {
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

// ----------------------------------------------------------------------------
// Retail: what M14's control program is
// ----------------------------------------------------------------------------

/// The blocks that spell `key`.
fn blocks_with(blocks: &[(u32, Vec<Directive>)], key: &str) -> Vec<u32> {
    blocks
        .iter()
        .filter(|(_, d)| d.iter().any(|d| d.key == key))
        .map(|(n, _)| *n)
        .collect()
}

/// `(block, key)` of every wake/kill/nap directive that names `target`.
fn incoming(blocks: &[(u32, Vec<Directive>)], target: i64) -> Vec<(u32, String)> {
    blocks
        .iter()
        .flat_map(|(n, d)| {
            d.iter()
                .filter(|d| {
                    d.key.ends_with("_OBJECTIVE_WHEN_I_COMPLETE") && addresses(d).contains(&target)
                })
                .map(move |d| (*n, d.key.clone()))
        })
        .collect()
}

/// The disposition of `key` as a measured operation.
fn operation_of(key: &str) -> DirectiveOperation {
    let record = census().row(MISSION).unwrap().record().unwrap();
    match record.key(key).unwrap().disposition() {
        DirectiveDisposition::Measured(measured) => measured.operation,
        other => panic!("{key} is not measured: {other:?}"),
    }
}

/// **The control program is the member that declares the numbered blocks.**
///
/// Of the 11 members of M14's reader archive exactly one declares numbered
/// `OBJECTIVE<N>` blocks. The blocks and sites the census measures equal an
/// independent walk of the same document, and the archive is the program span
/// M14-A bound.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m14_b_the_control_program_is_the_member_that_declares_the_blocks() {
    let row = census().row(MISSION).expect("M14 is in the census");
    assert_eq!(row.container, CONTAINER);
    assert_eq!(
        row.container_sha256, CONTAINER_SHA256,
        "the reader archive is the program M14-A bound"
    );
    assert_eq!(row.members.len(), 11);
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

    let record = row.record().expect("M14 has a measured control program");
    assert_eq!((record.blocks(), record.sites()), (BLOCKS, SITES));
    assert_eq!(record.keys().len(), KEYS);

    let blocks = blocks_of(&control_document());
    let numbers: Vec<u32> = blocks.iter().map(|(number, _)| *number).collect();
    assert_eq!(
        numbers,
        (1..=BLOCKS).collect::<Vec<_>>(),
        "numbered 1..=35, no gaps"
    );
    let walked: usize = blocks.iter().map(|(_, directives)| directives.len()).sum();
    assert_eq!(walked as u32, record.sites(), "the independent walk agrees");
    assert!(
        record.refusals().is_empty(),
        "every block is a readable list"
    );
    assert!(
        record.unclassified_record_keys().is_empty(),
        "M14 spells no record key outside the measured vocabulary"
    );

    let bound = control_binding();
    assert_eq!(bound.mission.as_str(), "mission/ch3-m04");
    assert_eq!(bound.program_id.as_str(), "script/c2b-m04-zrdr");
    assert_eq!(bound.program_asset, CONTAINER);
    assert_eq!(bound.program_sha256, row.container_sha256);
    assert_eq!(bound.control_member, CONTROL_MEMBER);
    assert_eq!(bound.control_offset, 14630);
    assert_eq!(bound.control_length, 19884);
    assert_eq!(
        (bound.record.blocks(), bound.record.sites()),
        (BLOCKS, SITES)
    );
    assert_eq!(bound.record.vocabulary(), KEYS as u32);

    let bytes = std::fs::read(game_dir().join(CONTAINER)).expect("the archive reads");
    assert_eq!(bytes.len() as u64, bound.program_length);
    assert_eq!(sha256(&bytes).to_hex(), CONTAINER_SHA256);
    let start = bound.control_offset as usize;
    let end = start + bound.control_length as usize;
    assert_eq!(
        sha256(&bytes[start..end]).to_hex(),
        bound.control_sha256,
        "the control member's digest re-derives from the member's own bytes"
    );
}

/// **Every directive key has exactly one disposition and none is refused.**
///
/// Two keys are terminal outcomes (`INSTANTWIN`, `INSTANTLOSS`, one site each)
/// and the other 37 have a measured effect; the keys' sites sum to the record's.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m14_b_every_directive_m14_spells_has_a_disposition_and_none_is_refused() {
    let record = census().row(MISSION).unwrap().record().unwrap();
    let mut outcomes = BTreeMap::new();
    let mut unmeasured = Vec::new();
    let mut measured = 0;
    for key in record.keys() {
        match key.disposition() {
            DirectiveDisposition::TerminalOutcome { outcome } => {
                outcomes.insert(key.key.clone(), outcome);
            }
            DirectiveDisposition::Measured(_) => measured += 1,
            DirectiveDisposition::Unmeasured { reason } => {
                unmeasured.push((key.key.clone(), reason));
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
    assert!(unmeasured.is_empty(), "{unmeasured:?}");
    assert_eq!(measured, 37);
    let sites: u32 = record.keys().iter().map(|key| key.sites).sum();
    assert_eq!(sites, SITES, "no site is dropped from the accounting");
}

/// **The record lowers: every site binds, every condition lowers and a program
/// stands.**
///
/// This is the gap M14-B-FU1 closed: `OBJECTIVE25`'s `TRAVELERS` spells an int
/// radius. All 254 sites bind, all 35 conditions lower, the lowering validates
/// and the row is complete. The census as a whole stays not campaign-ready.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m14_b_the_record_lowers_and_every_site_binds() {
    let census = census();
    let row = census.row(MISSION).unwrap();
    let attempt = row.lowering_attempt().unwrap();
    let lowered = attempt.attempt();
    assert_eq!(lowered.mission.as_deref(), Ok("mission/ch3-m04"));
    assert_eq!(lowered.objectives, BLOCKS);
    assert_eq!(lowered.calls.len() as u32, SITES);
    assert!(
        lowered
            .calls
            .iter()
            .all(|c| matches!(c, CallOutcome::Bound)),
        "no site is refused"
    );
    assert_eq!(lowered.conditions.len() as u32, BLOCKS);
    assert!(
        lowered
            .conditions
            .iter()
            .all(|c| *c == ConditionOutcome::Lowered),
        "{:?}",
        lowered.conditions
    );
    assert!(
        lowered.unbound_keys.is_empty(),
        "{:?}",
        lowered.unbound_keys
    );
    assert_eq!(lowered.validation, Some(Vec::new()), "validated clean");
    assert!(attempt.program().is_some(), "and is handed to the runtime");

    let lowering = row.lowering().unwrap();
    assert_eq!(lowering.unmet().count(), 0);
    assert!(lowering.complete());
    assert!(row.is_complete());
    assert!(census.complete_missions().contains(&MISSION));
    assert!(
        !census.campaign_ready(),
        "one complete mission does not make the campaign ready"
    );
}

/// The `(completion count, [(animation, state)])` an `ANIM_STATE` of `number`
/// watches.
fn anim_watch(
    blocks: &[(u32, Vec<Directive>)],
    number: u32,
) -> (Option<u32>, Vec<(String, String)>) {
    let block = &blocks.iter().find(|(n, _)| *n == number).unwrap().1;
    let args = block
        .iter()
        .find(|d| d.key == "ANIM_STATE")
        .unwrap_or_else(|| panic!("OBJECTIVE{number} spells no ANIM_STATE"))
        .args
        .as_ref()
        .unwrap();
    let mut count = None;
    let mut anims = Vec::new();
    for pair in args.chunks(2) {
        let ZrdValue::Text(key) = &pair[0] else {
            panic!("OBJECTIVE{number} ANIM_STATE child is not a key");
        };
        let list = pair[1].as_list().expect("every child has a list");
        match (key.as_str(), list) {
            ("COMPLETION_COUNT", [ZrdValue::Int(n)]) => count = Some(*n),
            ("ANIM", [ZrdValue::Text(_), name, ZrdValue::Text(_), state]) => {
                match (name.as_list().unwrap(), state.as_list().unwrap()) {
                    ([ZrdValue::Text(name)], [ZrdValue::Text(state)]) => {
                        anims.push((name.clone(), state.clone()));
                    }
                    other => panic!("ANIM spells {other:?}"),
                }
            }
            other => panic!("ANIM_STATE spells {other:?}"),
        }
    }
    (count, anims)
}

/// **Exposed weapon bays: the animation- and target-state blocks resolve to
/// measured operations and sit where the sheet's priority puts them.**
///
/// * `ANIM_STATE` in blocks 11, 12 and 13 watches the six
///   `deploy_gmzep_lbroad*` animations for `INVALID`, completing on 1, 3 and 5;
///   block 14 waits for `hooked_to_klondike` to be `EXECUTED`;
/// * `WAKE_ANIM` fades the Gemini zeppelin in (block 5) and wakes the home base
///   (block 14);
/// * `ADD_OBJECTIVE_TARGET` marks `geminizep` (5) and `pzhookpoint` (33);
/// * `WAKEUP_ZEP_TURRETS` wakes the pirate zeppelin's turrets (1) and the
///   Gemini's (5).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m14_b_the_weapon_bay_blocks_resolve_to_measured_operations() {
    assert_eq!(
        operation_of("ANIM_STATE"),
        DirectiveOperation::AnimationStates
    );
    assert_eq!(operation_of("WAKE_ANIM"), DirectiveOperation::WakeAnimation);
    assert_eq!(
        operation_of("WAKEUP_ZEP_TURRETS"),
        DirectiveOperation::WakeZeppelinTurrets
    );
    assert_eq!(
        operation_of("COMPLETED_ZEPCANNONS"),
        DirectiveOperation::ZeppelinCannons
    );

    let blocks = blocks_of(&control_document());
    assert_eq!(blocks_with(&blocks, "ANIM_STATE"), [11, 12, 13, 14]);
    assert_eq!(blocks_with(&blocks, "WAKE_ANIM"), [5, 14]);
    assert_eq!(blocks_with(&blocks, "ADD_OBJECTIVE_TARGET"), [5, 33]);
    assert_eq!(blocks_with(&blocks, "WAKEUP_ZEP_TURRETS"), [1, 5]);
    assert_eq!(blocks_with(&blocks, "COMPLETED_ZEPCANNONS"), [25]);

    let bays: Vec<String> = ["11", "12", "21", "22", "31", "32"]
        .iter()
        .map(|suffix| format!("deploy_gmzep_lbroad{suffix}"))
        .collect();
    for (number, needed) in [(11, 1), (12, 3), (13, 5)] {
        let (count, anims) = anim_watch(&blocks, number);
        assert_eq!(count, Some(needed), "OBJECTIVE{number}");
        assert_eq!(
            anims.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
            bays,
            "OBJECTIVE{number} watches the six broadside deployments"
        );
        assert!(anims.iter().all(|(_, state)| state == "INVALID"));
    }
    assert_eq!(
        anim_watch(&blocks, 14),
        (
            None,
            vec![("hooked_to_klondike".to_owned(), "EXECUTED".to_owned())]
        )
    );

    let targets = |key: &str| -> Vec<(u32, Vec<ZrdValue>)> {
        blocks
            .iter()
            .flat_map(|(n, d)| {
                d.iter()
                    .filter(|d| d.key == key)
                    .map(move |d| (*n, d.args.clone().unwrap()))
            })
            .collect()
    };
    assert_eq!(
        targets("ADD_OBJECTIVE_TARGET"),
        [
            (5, vec![ZrdValue::Text("geminizep".to_owned())]),
            (33, vec![ZrdValue::Text("pzhookpoint".to_owned())])
        ]
    );
    assert_eq!(
        targets("WAKEUP_ZEP_TURRETS"),
        [
            (1, vec![ZrdValue::Text("piratezep".to_owned())]),
            (5, vec![ZrdValue::Text("geminizep".to_owned())])
        ]
    );
}

/// **Capital survival: the zeppelin-health blocks, the depletion counts and the
/// approach test.**
///
/// * `INACTIVE*` blocks list a zeppelin's parts and a completion count:
///   `geminizep` engines 7-10 (14 parts, counts 1, 4, 7, 14), `piratezep`
///   engines 15-18 (12 parts, counts 2, 5, 10, 12) and gas bags 19-21 (6 parts,
///   counts 1, 2, 3);
/// * `DEDG` group depletions in blocks 2, 4, 27, 29, 30 and 31;
/// * `TRAVELERS` in block 25: `geminizep` APPROACHING `piratezep` at the int
///   radius 1000 (the gap M14-B-FU1 closed);
/// * `SET_AI_NET` retreats the Bloodhawks and Kestrels (28) and the `eg` wave
///   (34); `WAKEUP_GENERATOR` feeds `geminizep` from block 6.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m14_b_the_capital_survival_blocks_resolve_to_measured_operations() {
    assert_eq!(
        operation_of("INACTIVE1"),
        DirectiveOperation::InactiveMembers
    );
    assert_eq!(
        operation_of("INACTIVE_COMPLETION_COUNT"),
        DirectiveOperation::InactiveThreshold
    );
    assert_eq!(
        operation_of("DEDG"),
        DirectiveOperation::EnemyGroupDepletion
    );
    assert_eq!(operation_of("TRAVELERS"), DirectiveOperation::Travelers);
    assert_eq!(operation_of("SET_AI_NET"), DirectiveOperation::AssignNet);
    assert_eq!(
        operation_of("WAKEUP_GENERATOR"),
        DirectiveOperation::FeedGenerator
    );

    let blocks = blocks_of(&control_document());
    let health = |number: u32| -> (i64, usize, Vec<String>) {
        let block = &blocks.iter().find(|(n, _)| *n == number).unwrap().1;
        let count = block
            .iter()
            .find(|d| d.key == "INACTIVE_COMPLETION_COUNT")
            .map(|d| addresses(d)[0])
            .unwrap_or_else(|| panic!("OBJECTIVE{number} has no completion count"));
        let parts: Vec<&Directive> = block
            .iter()
            .filter(|d| d.key.starts_with("INACTIVE") && d.key != "INACTIVE_COMPLETION_COUNT")
            .collect();
        let mut zeppelins: Vec<String> = parts
            .iter()
            .map(|d| match d.args.as_deref() {
                Some([ZrdValue::Text(zep), ZrdValue::Text(_), ZrdValue::Text(_)]) => zep.clone(),
                other => panic!("OBJECTIVE{number} {} spells {other:?}", d.key),
            })
            .collect();
        zeppelins.sort();
        zeppelins.dedup();
        (count, parts.len(), zeppelins)
    };
    for (number, count, parts, zep) in [
        (7, 1, 14, "geminizep"),
        (8, 4, 14, "geminizep"),
        (9, 7, 14, "geminizep"),
        (10, 14, 14, "geminizep"),
        (15, 2, 12, "piratezep"),
        (16, 5, 12, "piratezep"),
        (17, 10, 12, "piratezep"),
        (18, 12, 12, "piratezep"),
        (19, 1, 6, "piratezep"),
        (20, 2, 6, "piratezep"),
        (21, 3, 6, "piratezep"),
    ] {
        assert_eq!(
            health(number),
            (count, parts, vec![zep.to_owned()]),
            "OBJECTIVE{number}"
        );
    }

    assert_eq!(blocks_with(&blocks, "DEDG"), [2, 4, 27, 29, 30, 31]);
    let dedg: Vec<Vec<i64>> = blocks
        .iter()
        .flat_map(|(_, d)| d.iter().filter(|d| d.key == "DEDG").map(addresses))
        .collect();
    assert_eq!(
        dedg,
        [
            vec![1, 2],
            vec![2, 2],
            vec![1, 4],
            vec![1, 0],
            vec![2, 0],
            vec![3, 0]
        ]
    );
    assert_eq!(blocks_with(&blocks, "SET_AI_NET"), [28, 34]);
    assert_eq!(blocks_with(&blocks, "WAKEUP_GENERATOR"), [6]);

    let travelers: Vec<(u32, Vec<ZrdValue>)> = blocks
        .iter()
        .flat_map(|(n, d)| {
            d.iter()
                .filter(|d| d.key == "TRAVELERS")
                .map(move |d| (*n, d.args.clone().unwrap()))
        })
        .collect();
    assert_eq!(
        travelers,
        [(
            25,
            vec![
                ZrdValue::Text("geminizep".to_owned()),
                ZrdValue::Text("APPROACHING".to_owned()),
                ZrdValue::Text("piratezep".to_owned()),
                ZrdValue::Int(1000),
                ZrdValue::Int(1),
            ]
        )],
        "the radius is spelled as an int, which the original reads (0x467cb6)"
    );
}

/// **The interrupt chains: which block cancels, wakes or delays which.**
///
/// Pinned edge by edge: block 13 kills the Gemini engine blocks 7-10, block 21
/// kills the pirate engine blocks 15-18 and block 31 kills 28; the wake chain
/// runs 7→8→9→10, 11→12→13, 29→30→31 and 33→{14, 23}; the nap timers are the
/// record's only delays. The record spells no `TICK_DEPENDS_ON_OBJ` gate.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m14_b_the_interrupt_chains_resolve_to_measured_operations() {
    assert_eq!(
        operation_of("KILL_OBJECTIVE_WHEN_I_COMPLETE"),
        DirectiveOperation::KillObjectives
    );
    assert_eq!(
        operation_of("WAKE_OBJECTIVE_WHEN_I_COMPLETE"),
        DirectiveOperation::WakeObjectives
    );
    assert_eq!(
        operation_of("NAP_OBJECTIVE_WHEN_I_COMPLETE"),
        DirectiveOperation::NapObjective
    );
    assert_eq!(
        operation_of("BEGIN_DORMANT"),
        DirectiveOperation::DormantStart
    );
    assert_eq!(
        operation_of("WAKEUP_ENEMIES"),
        DirectiveOperation::WakeEnemies
    );

    let blocks = blocks_of(&control_document());
    assert!(blocks_with(&blocks, "TICK_DEPENDS_ON_OBJ").is_empty());
    let edges = |key: &str| -> Vec<(u32, Vec<i64>)> {
        blocks
            .iter()
            .flat_map(|(n, d)| {
                d.iter()
                    .filter(|d| d.key == key)
                    .map(move |d| (*n, addresses(d)))
            })
            .collect()
    };
    assert_eq!(
        edges("KILL_OBJECTIVE_WHEN_I_COMPLETE"),
        [
            (13, vec![7, 8, 9, 10]),
            (21, vec![15, 16, 17, 18]),
            (31, vec![28])
        ]
    );
    assert_eq!(
        edges("WAKE_OBJECTIVE_WHEN_I_COMPLETE"),
        [
            (5, vec![25, 35]),
            (6, vec![11]),
            (7, vec![8]),
            (8, vec![9]),
            (9, vec![10]),
            (11, vec![12]),
            (12, vec![13, 29]),
            (13, vec![28, 24]),
            (28, vec![34]),
            (29, vec![30]),
            (30, vec![31]),
            (33, vec![14, 23])
        ]
    );
    assert_eq!(blocks_with(&blocks, "WAKEUP_ENEMIES"), [3, 5]);

    let naps: Vec<(u32, i64, f32)> = blocks
        .iter()
        .flat_map(|(n, d)| {
            d.iter()
                .filter(|d| d.key == "NAP_OBJECTIVE_WHEN_I_COMPLETE")
                .map(move |d| match d.args.as_deref() {
                    Some([ZrdValue::Int(target), ZrdValue::Float(seconds)]) => {
                        (*n, i64::from(*target), *seconds)
                    }
                    other => panic!("OBJECTIVE{n} NAP spells {other:?}"),
                })
        })
        .collect();
    assert_eq!(
        naps,
        [
            (2, 3, 15.0),
            (3, 4, 10.0),
            (4, 5, 15.0),
            (5, 6, 30.0),
            (13, 33, 20.0),
            (14, 32, 15.0),
            (15, 16, 1.0),
            (16, 17, 1.0),
            (17, 18, 1.0),
            (19, 20, 1.0),
            (20, 21, 1.0),
            (21, 22, 15.0),
            (25, 26, 5.0)
        ]
    );

    // Only block 1 starts on a clock (second 2); the other timed blocks are
    // woken by an address, and six blocks never start dormant.
    let timed: Vec<(u32, f32)> = blocks
        .iter()
        .flat_map(|(n, d)| {
            d.iter()
                .filter(|d| d.key == "BEGIN_DORMANT")
                .filter_map(move |d| match d.args.as_deref() {
                    Some([ZrdValue::Float(second), ..]) if *second >= 0.0 => Some((*n, *second)),
                    _ => None,
                })
        })
        .collect();
    assert_eq!(timed, [(1, 2.0)]);
    let always_awake: Vec<u32> = blocks
        .iter()
        .filter(|(_, d)| !d.iter().any(|d| d.key == "BEGIN_DORMANT"))
        .map(|(n, _)| *n)
        .collect();
    assert_eq!(always_awake, [2, 7, 15, 16, 19, 27]);
}

/// **Chapter transition: the terminal blocks are gated and every address is a
/// block of this record.**
///
/// Block 32 (`INSTANTWIN`) and block 22 (`INSTANTLOSS`) both start dormant with
/// no timed wake (`BEGIN_DORMANT -1`) and no other block ends the mission.
/// Block 32 is named only by block 14 (the `hooked_to_klondike` watch) through a
/// 15-second nap, block 22 only by block 21 (the last gas-bag block) through a
/// 15-second nap. The record names no successor mission; the transition itself
/// is not in this program (M14-C).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m14_b_the_terminal_blocks_are_gated_and_every_address_is_in_range() {
    let blocks = blocks_of(&control_document());
    let block = |number: u32| &blocks.iter().find(|(n, _)| *n == number).unwrap().1;

    for (number, outcome) in [(32, "INSTANTWIN"), (22, "INSTANTLOSS")] {
        let directives = block(number);
        assert!(
            directives
                .iter()
                .any(|d| d.key == outcome && d.args.is_none())
        );
        let dormant = directives
            .iter()
            .find(|d| d.key == "BEGIN_DORMANT")
            .unwrap();
        assert_eq!(
            dormant.args.as_deref(),
            Some(&[ZrdValue::Float(-1.0)][..]),
            "block {number} never wakes on its own clock"
        );
    }
    let outcomes: Vec<u32> = blocks
        .iter()
        .filter(|(_, d)| d.iter().any(|d| d.key.starts_with("INSTANT")))
        .map(|(n, _)| *n)
        .collect();
    assert_eq!(outcomes, [22, 32], "no other block ends the mission");

    let mut edges = 0;
    for (number, directives) in &blocks {
        for directive in directives {
            if directive.key.ends_with("_OBJECTIVE_WHEN_I_COMPLETE") {
                for address in addresses(directive) {
                    edges += 1;
                    assert!(
                        (1..=i64::from(BLOCKS)).contains(&address),
                        "OBJECTIVE{number} {} spells address {address}",
                        directive.key
                    );
                }
            }
        }
    }
    assert_eq!(
        edges, EDGES,
        "the address walk visits every spelled address"
    );

    assert_eq!(
        incoming(&blocks, 32),
        [(14, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned())],
        "the win is armed only by block 14, through a nap"
    );
    assert_eq!(
        incoming(&blocks, 22),
        [(21, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned())],
        "the loss is armed only by block 21, through a nap"
    );
    assert_eq!(
        incoming(&blocks, 14),
        [(33, "WAKE_OBJECTIVE_WHEN_I_COMPLETE".to_owned())],
        "block 14 is woken only by the hook-point block"
    );
    assert!(incoming(&blocks, 1).is_empty());
}

/// **M14's control-program row is complete; the campaign still is not ready.**
///
/// That is the control program only: no playthrough, difficulty or presentation
/// row is covered (M14-C).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m14_b_m14_lowers_completely_and_the_campaign_stays_unready() {
    let census = census();
    assert!(census.complete_missions().contains(&MISSION));
    assert!(!census.campaign_ready());
    assert!(census.measured_rows().any(|row| row.mission() == MISSION));
}

// ---------------------------------------------------------------------------
// Synthetic: the predicates the retail record leans on
// ---------------------------------------------------------------------------

fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

fn int(value: u32) -> ZrdValue {
    ZrdValue::Int(value)
}

fn record_of(blocks: Vec<(u32, Vec<ZrdValue>)>) -> ZrdValue {
    let mut children = Vec::new();
    for (number, directives) in blocks {
        children.push(text(&format!("OBJECTIVE{number}")));
        children.push(ZrdValue::List(directives));
    }
    ZrdValue::List(vec![ZrdValue::List(children)])
}

fn lower(document: &ZrdValue) -> cs_app::control_lowering::LoweredControlRecord {
    let record = measure_control_record(document);
    lower_control_record(
        ContentId::from_source(ContentKind::Mission, "accept-m14-b").map_err(|e| e.to_string()),
        "accept-m14-b",
        document,
        &record,
    )
}

fn travelers(radius: ZrdValue) -> ZrdValue {
    record_of(vec![(
        1,
        vec![
            text("TRAVELERS"),
            ZrdValue::List(vec![
                text("geminizep"),
                text("APPROACHING"),
                text("piratezep"),
                radius,
                int(1),
            ]),
        ],
    )])
}

/// **A `TRAVELERS` radius spelled as an int lowers like the float; a text radius
/// still does not.**
///
/// `OBJECTIVE25` spells `1000`. The original reads child 3 by type tag and
/// converts an int (`0x467cb6`); a string is its error path (`Expecting real
/// value`), so it must stay refused.
#[test]
fn accept_m14_b_an_int_travelers_radius_lowers_and_a_text_radius_is_refused() {
    for radius in [ZrdValue::Int(1000), ZrdValue::Float(1000.0)] {
        let lowered = lower(&travelers(radius));
        assert_eq!(lowered.attempt().conditions, [ConditionOutcome::Lowered]);
        assert_eq!(
            lowered.attempt().validation,
            Some(Vec::new()),
            "and the program validates clean"
        );
    }
    let refused = lower(&travelers(text("1000")));
    assert_ne!(refused.attempt().conditions, [ConditionOutcome::Lowered]);
}

/// **A kill that targets a terminal block is spelled data, not a verdict.**
///
/// Authored: block 1 kills block 2 (`INSTANTWIN`) and naps block 3
/// (`INSTANTLOSS`); both calls bind, as block 31's kill of 28 does in the record.
#[test]
fn accept_m14_b_a_kill_of_a_terminal_block_binds_like_any_other_address() {
    let document = record_of(vec![
        (
            1,
            vec![
                text("KILL_OBJECTIVE_WHEN_I_COMPLETE"),
                ZrdValue::List(vec![int(2)]),
                text("NAP_OBJECTIVE_WHEN_I_COMPLETE"),
                ZrdValue::List(vec![int(3), ZrdValue::Float(15.0)]),
            ],
        ),
        (
            2,
            vec![
                text("BEGIN_DORMANT"),
                ZrdValue::List(vec![ZrdValue::Float(-1.0)]),
                text("INSTANTWIN"),
            ],
        ),
        (
            3,
            vec![
                text("BEGIN_DORMANT"),
                ZrdValue::List(vec![ZrdValue::Float(-1.0)]),
                text("INSTANTLOSS"),
            ],
        ),
    ]);
    let lowered = lower(&document);
    assert!(
        lowered
            .attempt()
            .calls
            .iter()
            .all(|call| matches!(call, CallOutcome::Bound)),
        "{:?}",
        lowered.attempt().calls
    );
    assert_eq!(lowered.attempt().calls.len(), 6);
}
