//! Acceptance stage M09-B: the mission-specific compatibility gaps of the
//! ninth mission (`missions/M09.md`, work order `M09-B`).
//!
//! M09-A bound M09's identities (`missions/bindings/M09.json`: `mission/ch2-m04`,
//! `script/c1-m04-zrdr`, `ZBD/C1/M04/zrdr.zbd`) and left the mission program
//! unmeasured. The machinery that measures a control program is shared and was
//! built for M01; this stage runs it over M09's own reader archive and pins what
//! is **different** at M09, so the gaps are recorded as measurements.
//!
//! M09's record is `objectives.zrd`: 52 numbered blocks, 201 directive sites,
//! 40 distinct keys. All 40 keys are measured or terminal (none refused), every
//! site binds to a host call, every condition lowers, the lowering validates and
//! the row is complete. The sheet's three priorities name no key, so what is
//! pinned is the structure they live in:
//!
//! * **weakpoint exposure:** the `ANIM_STATE` blocks over the zeppelin's
//!   `destroy_hkzep_*` animations (23, 37), the `INACTIVE1` gas-bag/panel
//!   block 30 and the `ADD_OBJECTIVE_TARGET pzhookpoint` block 31;
//! * **interrupted attack priority:** the wake/kill/nap chains (block 16 kills
//!   15 and 17, block 36 kills 7, block 44 kills 39, block 30 kills 46);
//! * **carrier survival:** the `INACTIVE1 piratezep` blocks 29, 40 and 45, the
//!   two `TRAVELERS` approach tests (47 at 1500, 52 at 500) and the loss block
//!   41 (`INSTANTLOSS`, armed only by block 27's 20-second nap).
//!
//! No behaviour is invented here (see
//! `docs/findings/2026-10-25-m09-b-control-program-gaps.md`). The retail tests
//! are `#[ignore = "requires CS_GAME_DIR"]`; the synthetic ones run in CI.

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
pub(crate) const MISSION: &str = "zbd/c1/m04";

/// The reader archive `missions/bindings/M09.json` binds as the program.
const CONTAINER: &str = "ZBD/C1/M04/zrdr.zbd";
/// SHA-256 of that whole archive, from production discovery.
const CONTAINER_SHA256: &str = "5ffc1abd91a5919179448b397ec6bde48dd19fd8ba6f311b4362d76cb84c4263";
/// The member the block-carrying rule picks (never a filename constant).
const CONTROL_MEMBER: &str = "objectives.zrd";

/// The numbered blocks of the control member.
const BLOCKS: u32 = 52;
/// The directive sites of the control member.
const SITES: u32 = 201;
/// The distinct directive keys of the control member.
const KEYS: usize = 40;
/// The spelled wake/kill/nap/gate addresses.
const EDGES: usize = 58;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M09-B needs the retail capability; run this suite with \
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
            .find(|(label, _)| label.as_str() == "M09")
            .map(|(_, title)| title.clone())
            .expect("the declared inventory has an M09 work order");
        let context = SourceContext::read(&game_dir())
            .expect("production source context reads the installation");
        context
            .control_program(
                MissionLabel::new("M09").expect("M09 is a valid label"),
                &title,
            )
            .expect("M09's control program binds through the measured rule")
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
        let children = value.as_list().expect("every M09 block is a list");
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
        .expect("the rule finds M09's control member")
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
// Retail: what M09's control program is
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
/// Of the 15 members of M09's reader archive exactly one declares numbered
/// `OBJECTIVE<N>` blocks. The blocks and sites the census measures equal an
/// independent walk of the same document, and the archive is the program span
/// M09-A bound.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m09_b_the_control_program_is_the_member_that_declares_the_blocks() {
    let row = census().row(MISSION).expect("M09 is in the census");
    assert_eq!(row.container, CONTAINER);
    assert_eq!(
        row.container_sha256, CONTAINER_SHA256,
        "the reader archive is the program M09-A bound"
    );
    assert_eq!(row.members.len(), 15);
    let with_blocks: Vec<&str> = row
        .members
        .iter()
        .filter(|member| member.objective_blocks > 0)
        .map(|member| member.name.as_str())
        .collect();
    assert_eq!(with_blocks, ["objectives.zrd"]);
    let control: Vec<&str> = row
        .members
        .iter()
        .filter(|member| member.is_control)
        .map(|member| member.name.as_str())
        .collect();
    assert_eq!(control, ["objectives.zrd"]);

    let record = row.record().expect("M09 has a measured control program");
    assert_eq!((record.blocks(), record.sites()), (BLOCKS, SITES));
    assert_eq!(record.keys().len(), KEYS);

    let blocks = blocks_of(&control_document());
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
        "M09 spells no record key outside the measured vocabulary"
    );

    // The production binding and the census must not disagree about the
    // mission, the program, the member or the record.
    let bound = control_binding();
    assert_eq!(bound.mission.as_str(), "mission/ch2-m04");
    assert_eq!(bound.program_id.as_str(), "script/c1-m04-zrdr");
    assert_eq!(bound.program_asset, CONTAINER);
    assert_eq!(bound.program_sha256, row.container_sha256);
    assert_eq!(bound.control_member, CONTROL_MEMBER);
    assert_eq!(
        (bound.record.blocks(), bound.record.sites()),
        (BLOCKS, SITES)
    );
    assert_eq!(bound.record.vocabulary(), KEYS as u32);

    // The spans and digests re-derive from the archive's own bytes.
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
/// and the other 38 have a measured effect. The sites are accounted for: the
/// keys' sites sum to the record's.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m09_b_every_directive_m09_spells_has_a_disposition_and_none_is_refused() {
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
    assert_eq!(measured, 38);
    let sites: u32 = record.keys().iter().map(|key| key.sites).sum();
    assert_eq!(sites, SITES, "no site is dropped from the accounting");
}

/// **The record lowers: every site binds, every condition lowers and a program
/// stands.**
///
/// M09 spells `SET_AI_NET` (blocks 23 and 25) and the `{object, node}` pair
/// shapes that M02 and M03 could not bind; here all 201 sites bind, all 52
/// conditions lower, the lowering is validated and the row is complete. The
/// census as a whole stays not campaign-ready: other missions still refuse.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m09_b_the_record_lowers_and_every_site_binds() {
    let census = census();
    let row = census.row(MISSION).unwrap();
    let attempt = row.lowering_attempt().unwrap();
    let lowered = attempt.attempt();
    assert_eq!(lowered.mission.as_deref(), Ok("mission/ch2-m04"));
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
            .all(|c| *c == ConditionOutcome::Lowered)
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

/// **Weakpoint exposure: the animation- and part-state blocks resolve to
/// measured operations and sit where the sheet's priority puts them.**
///
/// * `ANIM_STATE` in blocks 23, 32 and 37: 23 and 37 watch the eight
///   `destroy_hkzep_*` animations for `INVALID` (23 completes on one, 37 on
///   three), 32 waits for `hooked_to_klondike` to be `EXECUTED`;
/// * `INACTIVE1` in block 30 names the zeppelin's gas bag and panel parts
///   (`lkgasbag05`, `panelleft1`);
/// * `ADD_OBJECTIVE_TARGET` is spelled by blocks 23 and 31; block 31 names the
///   hook point `pzhookpoint`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m09_b_the_weakpoint_blocks_resolve_to_measured_operations() {
    assert_eq!(
        operation_of("ANIM_STATE"),
        DirectiveOperation::AnimationStates
    );
    assert_eq!(operation_of("WAKE_ANIM"), DirectiveOperation::WakeAnimation);
    assert_eq!(
        operation_of("INACTIVE1"),
        DirectiveOperation::InactiveMembers
    );
    assert_eq!(
        operation_of("INACTIVE_COMPLETION_COUNT"),
        DirectiveOperation::InactiveThreshold
    );

    let blocks = blocks_of(&control_document());
    assert_eq!(blocks_with(&blocks, "ANIM_STATE"), [23, 32, 37]);
    assert_eq!(blocks_with(&blocks, "WAKE_ANIM"), [1, 32]);
    assert_eq!(blocks_with(&blocks, "ADD_OBJECTIVE_TARGET"), [23, 31]);
    assert_eq!(
        blocks_with(&blocks, "REMOVE_OBJECTIVE_TARGET"),
        [20, 25, 30, 33]
    );

    let block = |number: u32| &blocks.iter().find(|(n, _)| *n == number).unwrap().1;
    let directive = |number: u32, key: &str| {
        block(number)
            .iter()
            .find(|d| d.key == key)
            .unwrap_or_else(|| panic!("OBJECTIVE{number} spells no {key}"))
    };

    // The animation watches: (completion count, animation names, wanted state).
    let watch = |number: u32| {
        let args = directive(number, "ANIM_STATE").args.as_ref().unwrap();
        let mut count = None;
        let mut anims = Vec::new();
        let mut index = 0;
        while index < args.len() {
            let ZrdValue::Text(key) = &args[index] else {
                panic!("OBJECTIVE{number} ANIM_STATE child {index} is not a key");
            };
            let list = args[index + 1].as_list().expect("every child has a list");
            match key.as_str() {
                "COMPLETION_COUNT" => match list {
                    [ZrdValue::Int(n)] => count = Some(*n),
                    other => panic!("COMPLETION_COUNT spells {other:?}"),
                },
                "ANIM" => match list {
                    [ZrdValue::Text(_), name, ZrdValue::Text(_), state] => {
                        let (name, state) = (name.as_list().unwrap(), state.as_list().unwrap());
                        match (name, state) {
                            ([ZrdValue::Text(name)], [ZrdValue::Text(state)]) => {
                                anims.push((name.clone(), state.clone()));
                            }
                            other => panic!("ANIM spells {other:?}"),
                        }
                    }
                    other => panic!("ANIM spells {other:?}"),
                },
                other => panic!("ANIM_STATE spells key {other}"),
            }
            index += 2;
        }
        (count, anims)
    };
    let (count23, anims23) = watch(23);
    let (count37, anims37) = watch(37);
    assert_eq!((count23, count37), (Some(1), Some(3)));
    assert_eq!(anims23.len(), 8);
    assert_eq!(anims23, anims37, "both watch the same eight animations");
    assert!(
        anims23
            .iter()
            .all(|(name, state)| name.starts_with("destroy_hkzep_") && state == "INVALID")
    );
    assert_eq!(
        watch(32),
        (
            None,
            vec![("hooked_to_klondike".to_owned(), "EXECUTED".to_owned())]
        )
    );

    // The gas bag / panel block and the unbounded zeppelin-part blocks.
    assert_eq!(
        directive(30, "INACTIVE1").args.as_deref(),
        Some(
            &[
                ZrdValue::Text("lkgasbag05".to_owned()),
                ZrdValue::Text("panelleft1".to_owned())
            ][..]
        )
    );
    assert_eq!(
        directive(31, "ADD_OBJECTIVE_TARGET").args.as_deref(),
        Some(&[ZrdValue::Text("pzhookpoint".to_owned())][..])
    );
    // The two zeppelin-engine blocks: 26 needs 3 of its cleared engines, 27 needs 6.
    for (number, needed) in [(26, 3), (27, 6)] {
        assert_eq!(
            directive(number, "INACTIVE_COMPLETION_COUNT")
                .args
                .as_deref(),
            Some(&[ZrdValue::Int(needed)][..])
        );
        let listed = block(number)
            .iter()
            .filter(|d| d.key.starts_with("INACTIVE") && d.key != "INACTIVE_COMPLETION_COUNT")
            .count();
        assert_eq!(listed, 12, "OBJECTIVE{number} lists twelve engine parts");
    }
}

/// **Interrupted attack priority: the kill, nap and wake chains.**
///
/// Which block cancels which is the whole of the record's attack-priority
/// logic and is pinned edge by edge: block 16 kills 15 and 17 and wakes 18,
/// block 36 kills 7 and wakes 8, block 44 kills 39, block 30 kills 46. The
/// `TICK_DEPENDS_ON_OBJ` gates (7 on 4, 18 and 19 on 29, 42 on 40) stop a block
/// ticking while its dependency is not awake. The nap timers are the record's
/// only delays.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m09_b_the_interrupt_chains_resolve_to_measured_operations() {
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
        operation_of("TICK_DEPENDS_ON_OBJ"),
        DirectiveOperation::DependencyGate
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
            (7, vec![36]),
            (15, vec![16]),
            (16, vec![15, 17]),
            (30, vec![46]),
            (36, vec![7]),
            (44, vec![39])
        ]
    );
    assert_eq!(
        edges("TICK_DEPENDS_ON_OBJ"),
        [(7, vec![4]), (18, vec![29]), (19, vec![29]), (42, vec![40])]
    );
    assert_eq!(
        blocks_with(&blocks, "WAKEUP_ENEMIES"),
        [20, 24],
        "the two Bloodhawk waves"
    );
    assert_eq!(
        edges("WAKE_OBJECTIVE_WHEN_I_COMPLETE")
            .into_iter()
            .filter(|(n, _)| [20, 24, 30].contains(n))
            .collect::<Vec<_>>(),
        [
            (20, vec![22, 40, 50]),
            (24, vec![28, 49]),
            (30, vec![38, 42, 35])
        ]
    );

    // The nap timers, (block, target, seconds).
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
    assert_eq!(naps.len(), 23);
    assert!(naps.contains(&(3, 4, 17.0)));
    assert!(naps.contains(&(27, 41, 20.0)));
    assert!(naps.contains(&(31, 32, 1.0)));
    assert!(naps.contains(&(18, 20, 30.0)) && naps.contains(&(19, 20, 90.0)));

    // Only block 1 and block 2 run on a clock; five blocks never start dormant.
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
    assert_eq!(timed, [(1, 2.0), (2, 15.0)]);
    let always_awake: Vec<u32> = blocks
        .iter()
        .filter(|(_, d)| !d.iter().any(|d| d.key == "BEGIN_DORMANT"))
        .map(|(n, _)| *n)
        .collect();
    assert_eq!(always_awake, [7, 23, 26, 30, 33]);
}

/// **Carrier survival: the zeppelin-health blocks, the depletion counts and the
/// approach tests.**
///
/// `INACTIVE1 piratezep` (blocks 29, 40, 45), `DEDG` group depletions and the
/// two `TRAVELERS` approach tests resolve to measured operations. Both
/// `TRAVELERS` sites name `player` as subject, so neither is the counting mode.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m09_b_the_carrier_survival_blocks_resolve_to_measured_operations() {
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
    assert_eq!(blocks_with(&blocks, "DEDG"), [7, 22, 25, 28, 42, 43, 44]);
    let dedg: Vec<Vec<i64>> = blocks
        .iter()
        .flat_map(|(_, d)| d.iter().filter(|d| d.key == "DEDG").map(addresses))
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
            vec![5, 0]
        ]
    );
    assert_eq!(blocks_with(&blocks, "SET_AI_NET"), [23, 25]);
    assert_eq!(blocks_with(&blocks, "WAKEUP_GENERATOR"), [12, 13]);

    let piratezep: Vec<u32> = blocks
        .iter()
        .filter(|(_, d)| {
            d.iter().any(|d| {
                d.key == "INACTIVE1"
                    && d.args.as_deref() == Some(&[ZrdValue::Text("piratezep".to_owned())][..])
            })
        })
        .map(|(n, _)| *n)
        .collect();
    assert_eq!(piratezep, [29, 40, 45], "the zeppelin's own health blocks");

    let travelers: Vec<(u32, Vec<ZrdValue>)> = blocks
        .iter()
        .flat_map(|(n, d)| {
            d.iter()
                .filter(|d| d.key == "TRAVELERS")
                .map(move |d| (*n, d.args.clone().unwrap()))
        })
        .collect();
    let site = |radius: f32| {
        vec![
            ZrdValue::Text("player".to_owned()),
            ZrdValue::Text("APPROACHING".to_owned()),
            ZrdValue::Text("piratezep".to_owned()),
            ZrdValue::Float(radius),
            ZrdValue::Int(1),
        ]
    };
    assert_eq!(travelers, [(47, site(1500.0)), (52, site(500.0))]);
}

/// **The completion chains to `INSTANTWIN` and `INSTANTLOSS` are gated and
/// every cross-objective address is a block of this record.**
///
/// Both terminal blocks start dormant with no timed wake (`BEGIN_DORMANT`
/// `-1`). Block 41 (`INSTANTLOSS`) is named only by block 27 through a
/// 20-second nap; block 32 (`INSTANTWIN`) only by block 31 through a 1-second
/// nap, and block 31 is the hook-point objective (`ADD_OBJECTIVE_TARGET
/// pzhookpoint`). No other block ends the mission. Every wake, kill, nap and
/// gate address lies in `1..=52`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m09_b_the_terminal_blocks_are_gated_and_every_address_is_in_range() {
    let blocks = blocks_of(&control_document());
    let block = |number: u32| &blocks.iter().find(|(n, _)| *n == number).unwrap().1;

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
    assert_eq!(outcomes, [32, 41], "no other block ends the mission");

    let mut edges = 0;
    for (number, directives) in &blocks {
        for directive in directives {
            if directive.key.ends_with("_OBJECTIVE_WHEN_I_COMPLETE")
                || directive.key == "TICK_DEPENDS_ON_OBJ"
            {
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
        incoming(&blocks, 41),
        [(27, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned())],
        "the loss is armed only by block 27, through a nap"
    );
    assert_eq!(
        incoming(&blocks, 32),
        [(31, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned())],
        "the win is armed only by block 31, through a nap"
    );
    // The mission's opening route: block 1 wakes on its clock at second 2 and
    // block 2 at second 15; nothing else wakes them.
    assert_eq!(incoming(&blocks, 1), []);
    assert_eq!(incoming(&blocks, 2), []);
}

/// **M09's control-program row is complete; the campaign still is not ready.**
///
/// The census row for M09 has no unmet lowering requirement. That is the
/// control program only: no playthrough, difficulty or presentation row is
/// covered (M09-C), and the campaign as a whole stays unready.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m09_b_m09_lowers_completely_and_the_campaign_stays_unready() {
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
        ContentId::from_source(ContentKind::Mission, "accept-m09-b").map_err(|e| e.to_string()),
        "accept-m09-b",
        document,
        &record,
    )
}

fn travelers(subject: ZrdValue) -> ZrdValue {
    record_of(vec![(
        1,
        vec![
            text("TRAVELERS"),
            ZrdValue::List(vec![
                subject,
                text("APPROACHING"),
                text("cargozep2"),
                ZrdValue::Float(200.0),
                int(1),
            ]),
        ],
    )])
}

/// **A numeric `TRAVELERS` subject lowers to the counting condition and a named
/// one to the subject test.**
///
/// The same key at two subjects: both lower and the program validates, and the
/// two are different conditions (M09-B).
#[test]
fn accept_m09_b_a_numeric_travelers_subject_lowers_to_the_counting_mode() {
    for subject in [int(2), text("player")] {
        let lowered = lower(&travelers(subject));
        assert_eq!(lowered.attempt().conditions, [ConditionOutcome::Lowered]);
        assert_eq!(
            lowered.attempt().validation,
            Some(Vec::new()),
            "and the program validates clean"
        );
    }
}

/// **The counting site's group members and anchor reach the facts.**
///
/// The world fact table carries the awake members' positions of the group the
/// counting `TRAVELERS` names, keyed by the group id the record spells, and
/// the anchor chain; a group the world did not report carries nothing.
#[test]
fn accept_m09_b_the_counting_sites_group_members_reach_the_facts() {
    use cs_app::world_facts::{
        MemberObservation, MemberResolver, WorldFactTable, WorldObservation, WorldOperands,
        member_chain,
    };

    let lowered = lower(&travelers(int(2)));
    let program = lowered.program().expect("the authored record assembles");
    let operands = WorldOperands::of(program);
    assert_eq!(
        operands.group_members.iter().copied().collect::<Vec<_>>(),
        [2]
    );
    assert!(operands.members.contains(&member_chain(["cargozep2"])));

    let resolver = MemberResolver::from_hierarchy(vec![
        ("world1".to_owned(), None),
        ("cargozep2".to_owned(), Some(0)),
    ]);
    let mut table = WorldFactTable::new(resolver);
    table.observe(
        WorldObservation::new()
            .member(
                member_chain(["cargozep2"]),
                MemberObservation::held_in_play([0.0, 0.0, 0.0]),
            )
            .group_members(2, vec![[10.0, 0.0, 0.0], [0.0, 20.0, 0.0]]),
    );
    let facts = table.facts(&operands);
    assert_eq!(
        facts.group_members[&2],
        vec![[10.0, 0.0, 0.0], [0.0, 20.0, 0.0]]
    );

    let mut silent = WorldFactTable::new(MemberResolver::from_hierarchy(vec![(
        "world1".to_owned(),
        None,
    )]));
    silent.observe(WorldObservation::new());
    assert!(silent.facts(&operands).group_members.is_empty());
}

/// **A kill that targets a terminal block is spelled data, not a verdict.**
///
/// Blocks that kill or nap a terminal block lower to the same host calls whatever
/// the target is; the record never marks one terminal block as winning over the
/// other. Authored: block 1 kills block 2 (`INSTANTWIN`) and naps block 3
/// (`INSTANTLOSS`); both calls bind.
#[test]
fn accept_m09_b_a_kill_of_a_terminal_block_binds_like_any_other_address() {
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
