//! Acceptance stage M05-B: the mission-specific compatibility gaps of the
//! fifth mission (`missions/M05.md`, work order `M05-B`).
//!
//! M05-A bound M05's identities. The machinery that measures a control program
//! (the `.zrd` reader, the census, the directive dispositions and the record →
//! `RawProgram` adapter) is shared and was built for M01; this stage runs it
//! over M05's own reader archive and pins what is **different** at M05:
//!
//! * M05's control program is `objectives.zrd` with 58 numbered blocks and 208
//!   directive sites, selected by the content rule and not by its name;
//! * all 22 of its directive keys have a disposition: 2 terminal outcomes and
//!   20 with a measured effect, **none refused**;
//! * unlike M02 and M03 the record **lowers**: every site binds, every
//!   condition lowers, a `MissionProgram` stands and the census row is complete;
//! * the sheet's three priorities (wave lifecycle, protected actor damage,
//!   timed completion) are present in the record and resolve to measured
//!   operations, and the chains that reach `INSTANTWIN` and `INSTANTLOSS` are
//!   pinned address by address, including the precedence between them.
//!
//! A lowered program is **not** a played mission: no playthrough, difficulty,
//! media or presentation row is covered (that is M05-C, with `human_play`), and
//! the measured directives keep the unknowns the M01-LC findings recorded (see
//! `docs/findings/2026-10-09-m05-b-control-program.md`).
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`; the synthetic
//! tests run in CI.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_app::control_lowering::lower_control_record;
use cs_app::mission_control::{
    RetailControlCensus, read_control_member, survey_mission_control_programs,
};
use cs_content::mission_control::{
    CallOutcome, ConditionOutcome, DirectiveDisposition, DirectiveOperation, TerminalOutcome,
    measure_control_record,
};
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{ZrdValue, objective_record, zrd_flat_fields};
use cs_types::content::{ContentId, ContentKind};

/// The census row label of the mission.
pub(crate) const MISSION: &str = "zbd/c1/m05";

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
            "CS_GAME_DIR is not set: M05-B needs the retail capability; run this suite with \
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
        let children = value.as_list().expect("every M05 block is a list");
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
        .expect("the rule finds M05's control member")
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

// ---------------------------------------------------------------------------
// Retail: what M05's control program is
// ---------------------------------------------------------------------------

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
/// Of the 14 members of M05's reader archive exactly one declares numbered
/// `OBJECTIVE<N>` blocks. The blocks and sites the census measures equal an
/// independent walk of the same document, and the archive is the program span
/// M05-A bound.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m05_b_the_control_program_is_the_member_that_declares_the_blocks() {
    let row = census().row(MISSION).expect("M05 is in the census");
    assert_eq!(row.container, "ZBD/C1/M05/zrdr.zbd");
    assert_eq!(
        row.container_sha256, "0ae0341cba2548a3d1cb9f10ec06cfae3ecf4c81513bd320d882f6a57f8c34fe",
        "the reader archive is the program M05-A bound"
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

    let record = row.record().expect("M05 has a measured control program");
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
        "M05 spells no record key outside the measured vocabulary"
    );
}

/// **Every directive key has exactly one disposition and none is refused.**
///
/// Two keys are terminal outcomes (`INSTANTWIN`, `INSTANTLOSS`, one site each)
/// and the other 20 have a measured effect. The sites are accounted for: the
/// keys' sites sum to the record's.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m05_b_every_directive_m05_spells_has_a_disposition_and_none_is_refused() {
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
/// host-call bound. M05 spells no `SET_AI_NET`, no argument shape that
/// disagrees and no unknown key, so the registry binds all 208 sites, all 58
/// conditions lower, the lowering is validated and the row is complete. The
/// census as a whole stays not campaign-ready: other missions still refuse.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m05_b_the_record_lowers_and_every_site_binds() {
    let census = census();
    let row = census.row(MISSION).unwrap();
    let attempt = row.lowering_attempt().unwrap();
    let lowered = attempt.attempt();
    assert_eq!(lowered.mission.as_deref(), Ok("mission/ch1-m05"));
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
fn accept_m05_b_the_sheet_priorities_resolve_to_measured_operations() {
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
fn accept_m05_b_the_terminal_blocks_are_gated_and_every_address_is_in_range() {
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

/// **M05 is campaign-complete as a row but the census does not hide the rest.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m05_b_the_census_still_counts_every_other_mission() {
    let census = census();
    assert!(census.measured_rows().any(|row| row.mission() == MISSION));
    assert!(census.complete_missions().contains(&MISSION));
    assert!(!census.campaign_ready());
    assert!(
        census.rows().iter().any(|row| !row.is_complete()),
        "missions with refused sites stay in the denominator"
    );
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
        ContentId::from_source(ContentKind::Mission, "accept-m05-b").map_err(|e| e.to_string()),
        "accept-m05-b",
        document,
        &record,
    )
}

/// The keys of M05's loss-and-win shape, authored: a timed nap to a dormant
/// loss and a kill of the win.
fn terminal_shape() -> ZrdValue {
    record_of(vec![
        (
            1,
            vec![
                text("INACTIVE1"),
                ZrdValue::List(vec![text("lifeboat")]),
                text("NAP_OBJECTIVE_WHEN_I_COMPLETE"),
                ZrdValue::List(vec![int(2), ZrdValue::Float(30.0)]),
                text("KILL_OBJECTIVE_WHEN_I_COMPLETE"),
                ZrdValue::List(vec![int(3)]),
            ],
        ),
        (
            2,
            vec![
                text("BEGIN_DORMANT"),
                ZrdValue::List(vec![ZrdValue::Float(-1.0)]),
                text("INSTANTLOSS"),
            ],
        ),
        (
            3,
            vec![
                text("BEGIN_DORMANT"),
                ZrdValue::List(vec![ZrdValue::Float(-1.0)]),
                text("INSTANTWIN"),
            ],
        ),
    ])
}

/// **A record made only of M05's measured keys lowers to a program; an unknown
/// key in the same record refuses it.** The same record with and without the
/// extra key, so the refusal is the key and not the shape.
#[test]
fn accept_m05_b_a_record_of_measured_keys_lowers_and_an_unknown_key_refuses_it() {
    let document = terminal_shape();
    let record = measure_control_record(&document);
    assert_eq!(
        record.key("INSTANTWIN").unwrap().disposition(),
        DirectiveDisposition::TerminalOutcome {
            outcome: TerminalOutcome::Succeeded
        }
    );
    assert_eq!(
        record.key("INSTANTLOSS").unwrap().disposition(),
        DirectiveDisposition::TerminalOutcome {
            outcome: TerminalOutcome::Failed
        }
    );
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
    assert!(lowered.program().is_some());

    let with_unknown = record_of(vec![(
        1,
        vec![text("NOT_A_MEASURED_KEY"), ZrdValue::List(vec![int(1)])],
    )]);
    let lowered = lower(&with_unknown);
    assert!(lowered.program().is_none());
    assert!(
        lowered
            .attempt()
            .calls
            .iter()
            .all(|call| matches!(call, CallOutcome::Refused(_)))
    );
}

/// **A block that spells a terminal outcome and a wake of itself is read as
/// authored: the outcome is a site like any other, never inferred from the
/// block's position.** Block order does not move the terminal: swapping the
/// loss and win blocks swaps nothing about which key means which outcome.
#[test]
fn accept_m05_b_a_terminal_outcome_is_the_key_and_not_the_block_position() {
    let swapped = record_of(vec![
        (
            1,
            vec![
                text("BEGIN_DORMANT"),
                ZrdValue::List(vec![ZrdValue::Float(-1.0)]),
                text("INSTANTWIN"),
            ],
        ),
        (
            2,
            vec![
                text("BEGIN_DORMANT"),
                ZrdValue::List(vec![ZrdValue::Float(-1.0)]),
                text("INSTANTLOSS"),
            ],
        ),
    ]);
    let record = measure_control_record(&swapped);
    assert_eq!(
        record.key("INSTANTWIN").unwrap().disposition(),
        DirectiveDisposition::TerminalOutcome {
            outcome: TerminalOutcome::Succeeded
        }
    );
    assert_eq!(
        record.key("INSTANTLOSS").unwrap().disposition(),
        DirectiveDisposition::TerminalOutcome {
            outcome: TerminalOutcome::Failed
        }
    );
    assert_eq!((record.blocks(), record.sites()), (2, 4));
}
