//! Acceptance stage M10-B: the mission-specific compatibility gaps of the
//! tenth mission (`missions/M10.md`, work order `M10-B`).
//!
//! M10-A bound M10's identities and left the mission program unmeasured. The
//! machinery that measures a control program (the `.zrd` reader, the census,
//! the directive dispositions and the record → `RawProgram` adapter) is shared
//! and was built for M01; this stage runs it over M10's own reader archive and
//! pins what is **different** at M10, so the gaps are recorded as measurements
//! and not discovered later as a silent failure.
//!
//! **Which archive this measures.** Under the corrected campaign layout
//! `missions/bindings/M10.json` binds work order M10 to `mission/ch2-m05`,
//! `script/c1-m05-zrdr` and `ZBD/C1/M05/zrdr.zbd`. The earlier pins of this
//! suite measured `ZBD/C2/M05/zrdr.zbd` (`mission/ch3-m05`, 49 blocks, 232
//! sites, 36 keys, two counting `TRAVELERS` sites): a different campaign
//! mission, not bound in `missions/bindings/`, that the suite never noticed
//! because it asserted the archive's own declared identity. Those numbers
//! belong to that archive and are not M10's. The same stale-pin class was
//! fixed for M04-B (#1617); `ZBD/C1/M05` is also the archive `m05_b.rs`
//! measures, so the two suites agree on the record by construction.
//!
//! What M10's own record is:
//!
//! * its control program is `objectives.zrd` with 58 numbered blocks and 208
//!   directive sites, selected by the content rule and not by its name, and the
//!   production binding reaches the same member, archive and digests;
//! * all 22 of its directive keys are measured or terminal: none is refused and
//!   every site binds to a host call;
//! * its only `TRAVELERS` site (block 53) names `player` as subject, so the
//!   counting mode M10-B-FU1 (#812) lowers to `Condition::TravelersCount` is
//!   **not spelled** in M10's record: that mechanism belonged to the
//!   first-measured archive and is exercised here by the synthetic tests only;
//! * the sheet's three priorities name no key, so what is pinned is the
//!   structure they live in: the completion chain to `INSTANTWIN` (block 42) and
//!   `INSTANTLOSS` (block 4), the group-depletion and world damage state
//!   (`INACTIVE<n>`) blocks and the `TRAVELERS` site.
//!
//! No behaviour is invented here (see
//! `docs/findings/2026-10-09-m10-b-control-program-gaps.md` and
//! `docs/findings/2026-10-18-m10-b-fu1-travelers-counting-mode.md`).
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`; the synthetic ones
//! run in CI.

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
pub(crate) const MISSION: &str = "zbd/c1/m05";

/// The reader archive `missions/bindings/M10.json` binds as the program.
const CONTAINER: &str = "ZBD/C1/M05/zrdr.zbd";
/// SHA-256 of that whole archive, from production discovery.
const CONTAINER_SHA256: &str = "0ae0341cba2548a3d1cb9f10ec06cfae3ecf4c81513bd320d882f6a57f8c34fe";
/// The member the block-carrying rule picks (never a filename constant).
const CONTROL_MEMBER: &str = "objectives.zrd";

/// The numbered blocks of the control member.
const BLOCKS: u32 = 58;
/// The directive sites of the control member.
const SITES: u32 = 208;
/// The distinct directive keys of the control member.
const KEYS: usize = 22;
/// The spelled wake/kill/nap/gate addresses.
const EDGES: usize = 92;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M10-B needs the retail capability; run this suite with \
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
            .find(|(label, _)| label.as_str() == "M10")
            .map(|(_, title)| title.clone())
            .expect("the declared inventory has an M10 work order");
        let context = SourceContext::read(&game_dir())
            .expect("production source context reads the installation");
        context
            .control_program(
                MissionLabel::new("M10").expect("M10 is a valid label"),
                &title,
            )
            .expect("M10's control program binds through the measured rule")
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
        let children = value.as_list().expect("every M10 block is a list");
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
        .expect("the rule finds M10's control member")
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
// Retail: what M10's control program is
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

/// **The control program is the member that declares the numbered blocks.**
///
/// Of the 14 members of M10's reader archive exactly one declares numbered
/// `OBJECTIVE<N>` blocks. The blocks and sites the census measures equal an
/// independent walk of the same document, and the archive is the program span
/// M10-A bound.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m10_b_the_control_program_is_the_member_that_declares_the_blocks() {
    let row = census().row(MISSION).expect("M10 is in the census");
    assert_eq!(row.container, CONTAINER);
    assert_eq!(
        row.container_sha256, CONTAINER_SHA256,
        "the reader archive is the program M10-A bound"
    );
    assert_eq!(row.members.len(), 14);
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

    let record = row.record().expect("M10 has a measured control program");
    assert_eq!((record.blocks(), record.sites()), (BLOCKS, SITES));
    assert_eq!(record.keys().len(), KEYS);

    let blocks = blocks_of(&control_document());
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
        "M10 spells no record key outside the measured vocabulary"
    );

    // The production binding and the census must not disagree about the
    // mission, the program, the member or the record.
    let bound = control_binding();
    assert_eq!(bound.mission.as_str(), "mission/ch2-m05");
    assert_eq!(bound.program_id.as_str(), "script/c1-m05-zrdr");
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
/// and the other 20 have a measured effect. The sites are accounted for: the
/// keys' sites sum to the record's.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m10_b_every_directive_m10_spells_has_a_disposition_and_none_is_refused() {
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
    assert_eq!(measured, 20);
    let sites: u32 = record.keys().iter().map(|key| key.sites).sum();
    assert_eq!(sites, SITES, "no site is dropped from the accounting");
}

/// **The record lowers: every site binds, every condition lowers and a program
/// stands.**
///
/// This is the difference from M02 and M03, whose records refuse at the
/// host-call bound. M10 spells no `SET_AI_NET`, no argument shape that
/// disagrees and no unknown key, so the registry binds all 208 sites, all 58
/// conditions lower, the lowering is validated and the row is complete. The
/// census as a whole stays not campaign-ready: other missions still refuse.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m10_b_the_record_lowers_and_every_site_binds() {
    let census = census();
    let row = census.row(MISSION).unwrap();
    let attempt = row.lowering_attempt().unwrap();
    let lowered = attempt.attempt();
    assert_eq!(lowered.mission.as_deref(), Ok("mission/ch2-m05"));
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
    assert!(lowered.validation.is_some(), "the program was validated");
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

/// **The sheet's three priorities are in the record and resolve to measured
/// operations.**
///
/// * wave lifecycle: `WAKEUP_ENEMIES` (11 sites), `WAKE_ANIM`, `ANIM_STATE`
///   and the wake chains through `WAKE_OBJECTIVE_WHEN_I_COMPLETE`;
/// * protected actor damage: `INACTIVE<n>` (the in-play bit of listed
///   members) in blocks 3 and 52, `DEDG` group depletion and `TRAVELERS`;
/// * timed completion: `BEGIN_DORMANT` clocks and the `NAP_OBJECTIVE_WHEN_I_COMPLETE`
///   timers.
///
/// Every key is asserted to resolve to the operation the shared finding
/// measured, so a key that silently lost its meaning fails here.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m10_b_the_sheet_priorities_resolve_to_measured_operations() {
    let record = census().row(MISSION).unwrap().record().unwrap();
    let operation = |key: &str| match record.key(key).unwrap().disposition() {
        DirectiveDisposition::Measured(measured) => measured.operation,
        other => panic!("{key} is not measured: {other:?}"),
    };
    assert_eq!(operation("WAKEUP_ENEMIES"), DirectiveOperation::WakeEnemies);
    assert_eq!(operation("WAKE_ANIM"), DirectiveOperation::WakeAnimation);
    assert_eq!(operation("ANIM_STATE"), DirectiveOperation::AnimationStates);
    assert_eq!(
        operation("WAKE_OBJECTIVE_WHEN_I_COMPLETE"),
        DirectiveOperation::WakeObjectives
    );
    assert_eq!(operation("INACTIVE1"), DirectiveOperation::InactiveMembers);
    assert_eq!(operation("INACTIVE2"), DirectiveOperation::InactiveMembers);
    assert_eq!(operation("INACTIVE3"), DirectiveOperation::InactiveMembers);
    assert_eq!(operation("DEDG"), DirectiveOperation::EnemyGroupDepletion);
    assert_eq!(operation("TRAVELERS"), DirectiveOperation::Travelers);
    assert_eq!(operation("BEGIN_DORMANT"), DirectiveOperation::DormantStart);
    assert_eq!(
        operation("NAP_OBJECTIVE_WHEN_I_COMPLETE"),
        DirectiveOperation::NapObjective
    );
    assert_eq!(
        operation("KILL_OBJECTIVE_WHEN_I_COMPLETE"),
        DirectiveOperation::KillObjectives
    );
    assert_eq!(
        operation("TICK_DEPENDS_ON_OBJ"),
        DirectiveOperation::DependencyGate
    );

    let blocks = blocks_of(&control_document());
    let with = |key: &str| blocks_with(&blocks, key);

    // Wave lifecycle: ten waves of an animation-gated enemy wake, each a
    // `WAKE_ANIM` launcher block (10..=12), an `ANIM_STATE` block and a
    // `WAKEUP_ENEMIES` block.
    assert_eq!(with("WAKE_ANIM"), [10, 11, 12, 42]);
    assert_eq!(with("ANIM_STATE"), [13, 15, 17, 19, 21, 23, 25, 27, 29, 42]);
    assert_eq!(
        with("WAKEUP_ENEMIES"),
        [9, 14, 16, 18, 20, 22, 24, 26, 28, 30, 40]
    );

    // Protected actor damage: the `INACTIVE` blocks and the depletion counts.
    assert_eq!(
        with("INACTIVE1"),
        [3, 31, 32, 33, 34, 35, 36, 37, 38, 39, 43, 44, 45, 52]
    );
    assert_eq!(with("INACTIVE2"), [43, 44, 45]);
    assert_eq!(with("INACTIVE3"), [43, 44, 45]);
    assert_eq!(with("DEDG"), [2, 7, 46, 47, 48, 50, 51]);
    let dedg: Vec<Vec<i64>> = blocks
        .iter()
        .flat_map(|(_, d)| d.iter().filter(|d| d.key == "DEDG").map(addresses))
        .collect();
    assert_eq!(
        dedg,
        [
            vec![5, 0],
            vec![1, 0],
            vec![2, 0],
            vec![3, 0],
            vec![4, 0],
            vec![3, 0],
            vec![4, 0]
        ]
    );
    assert_eq!(with("TRAVELERS"), [53]);
    assert_eq!(with("TICK_DEPENDS_ON_OBJ"), [48, 51]);

    // Timed completion: the only timed wake is block 1's, the naps carry the
    // other timers.
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
    assert_eq!(timed, [(1, 2.0)], "block 1 wakes itself at second 2");
    let always_awake: Vec<u32> = blocks
        .iter()
        .filter(|(_, d)| !d.iter().any(|d| d.key == "BEGIN_DORMANT"))
        .map(|(n, _)| *n)
        .collect();
    assert_eq!(
        always_awake,
        [3, 7],
        "the only blocks that never start dormant"
    );
    let naps: Vec<(u32, i64, f32)> = blocks
        .iter()
        .flat_map(|(n, d)| {
            d.iter()
                .filter(|d| d.key == "NAP_OBJECTIVE_WHEN_I_COMPLETE")
                .map(move |d| {
                    let args = d.args.as_deref().unwrap();
                    match args {
                        [ZrdValue::Int(target), ZrdValue::Float(seconds)] => {
                            (*n, i64::from(*target), *seconds)
                        }
                        other => panic!("OBJECTIVE{n} NAP spells {other:?}"),
                    }
                })
        })
        .collect();
    assert_eq!(
        naps,
        [
            (3, 4, 30.0),
            (7, 8, 5.0),
            (8, 46, 70.0),
            (9, 47, 75.0),
            (40, 48, 75.0),
            (46, 9, 1.0),
            (47, 40, 1.0),
            (48, 41, 1.0),
            (49, 9, 1.0),
            (50, 40, 1.0),
            (51, 41, 1.0)
        ]
    );
}

/// **The completion chains to `INSTANTWIN` and `INSTANTLOSS` are gated, the
/// loss is on a timer, and every cross-objective address is a block of this
/// record.**
///
/// Both terminal blocks start dormant with no timed wake (`BEGIN_DORMANT`
/// `-1`). Block 4 (`INSTANTLOSS`) is named only by block 3 through a 30-second
/// nap; block 42 (`INSTANTWIN`) is woken by block 41 and **killed** by block 3,
/// so block 3's completion arms the loss and removes the win. No other block
/// ends the mission. Every wake, kill, nap and gate address lies in `1..=58`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m10_b_the_terminal_blocks_are_gated_and_every_address_is_in_range() {
    let blocks = blocks_of(&control_document());
    let block = |number: u32| &blocks.iter().find(|(n, _)| *n == number).unwrap().1;

    for (number, outcome) in [(42, "INSTANTWIN"), (4, "INSTANTLOSS")] {
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
    assert_eq!(outcomes, [4, 42], "no other block ends the mission");

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
        incoming(&blocks, 4),
        [(3, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned())],
        "the loss is armed only by block 3, through a nap"
    );
    assert_eq!(
        incoming(&blocks, 42),
        [
            (3, "KILL_OBJECTIVE_WHEN_I_COMPLETE".to_owned()),
            (41, "WAKE_OBJECTIVE_WHEN_I_COMPLETE".to_owned())
        ],
        "the win is woken by block 41 and killed by block 3"
    );
    // The mission's opening route: the timed wake at second 2 is block 1 and
    // the approach test of block 53 wakes block 5.
    assert_eq!(
        incoming(&blocks, 5),
        [(53, "WAKE_OBJECTIVE_WHEN_I_COMPLETE".to_owned())]
    );
    assert_eq!(
        incoming(&blocks, 1),
        [],
        "block 1 is woken only by its clock"
    );
}

/// **M10's record spells no counting `TRAVELERS` site.**
///
/// The one `TRAVELERS` site (block 53) names `player` as subject, so it lowers
/// to the subject test and not to the counting condition. The two counting
/// sites M10-B-FU1 (#812) lowered (a numeric `child0`) were spelled by
/// `ZBD/C2/M05`, the archive this suite measured before the layout was
/// corrected; the counting mechanism stays covered by the synthetic tests below
/// and is not evidence about M10.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m10_b_the_one_travelers_site_is_block_53_and_spells_no_counting_mode() {
    let blocks = blocks_of(&control_document());
    let travelers: Vec<(u32, &Vec<ZrdValue>)> = blocks
        .iter()
        .flat_map(|(n, d)| {
            d.iter()
                .filter(|d| d.key == "TRAVELERS")
                .map(move |d| (*n, d.args.as_ref().unwrap()))
        })
        .collect();
    assert_eq!(travelers.len(), 1);
    assert_eq!(travelers[0].0, 53);
    assert_eq!(
        travelers[0].1.as_slice(),
        [
            ZrdValue::Text("player".to_owned()),
            ZrdValue::Text("APPROACHING".to_owned()),
            ZrdValue::Text("piratezep".to_owned()),
            ZrdValue::Float(1500.0),
            ZrdValue::Int(1),
        ]
    );

    let record = census().row(MISSION).unwrap().record().unwrap();
    let operation = match record.key("TRAVELERS").unwrap().disposition() {
        DirectiveDisposition::Measured(measured) => measured.operation,
        other => panic!("TRAVELERS is not measured: {other:?}"),
    };
    assert_eq!(operation, DirectiveOperation::Travelers);
}

/// **M10's control-program row is complete; the campaign still is not ready.**
///
/// The census row for M10 has no unmet lowering requirement. That is the
/// control program only: no playthrough, difficulty or presentation row is
/// covered (M10-C), and the campaign as a whole stays unready.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m10_b_m10_lowers_completely_and_the_campaign_stays_unready() {
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
        ContentId::from_source(ContentKind::Mission, "accept-m10-b").map_err(|e| e.to_string()),
        "accept-m10-b",
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
/// two are different conditions (M10-B-FU1).
#[test]
fn accept_m10_b_fu1_a_numeric_travelers_subject_lowers_to_the_counting_mode() {
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
fn accept_m10_b_fu1_the_counting_sites_group_members_reach_the_facts() {
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
fn accept_m10_b_a_kill_of_a_terminal_block_binds_like_any_other_address() {
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
