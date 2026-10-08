//! Acceptance stage M03-B: the mission-specific compatibility gaps of the
//! third mission (`missions/M03.md`, work order `M03-B`).
//!
//! M03-A bound M03's identities and left the mission program unmeasured. The
//! machinery that measures a control program (the `.zrd` reader, the
//! census, the directive dispositions and the record → `RawProgram` adapter)
//! is shared and was built for M01; this stage runs it over M03's own
//! reader archive and pins what is **different** at M03, so that the gaps are
//! recorded as measurements and not discovered later as a silent failure:
//!
//! * M03's control program is `objectives.zrd` with 55 numbered blocks and 313
//!   directive sites, selected by the content rule and not by its name;
//! * 38 of its 40 directive keys have a measured effect, two are terminal
//!   outcomes and **one** (`WAKEUP_OBJECTIVE_WHEN_I_COMPLETE`) is refused,
//!   because its two sites spell two argument shapes;
//! * the record does **not** lower: three sites refuse (the two refused wake
//!   sites and one `SET_AI_NET` site that exceeds the host-call bound), so no
//!   `MissionProgram` stands and the mission is not ready;
//! * the sheet's three priorities (grouped objectives, world damage state,
//!   completion ordering) are present in the record and resolve to measured
//!   operations, and the completion chain that reaches `INSTANTWIN` is pinned
//!   address by address.
//!
//! No behaviour is invented here: the two unmeasured pieces are filed as
//! follow-up tasks (see `docs/findings/2026-10-08-m03-b-control-program-gaps.md`)
//! and the mission stays unready until they are measured.
//!
//! The retail tests are `#[ignore = "requires CS_GAME_DIR"]`; the two synthetic
//! tests run in CI.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_app::control_lowering::lower_control_record;
use cs_app::mission_control::{
    RetailControlCensus, read_control_member, survey_mission_control_programs,
};
use cs_content::mission_control::{
    CallOutcome, DirectiveDisposition, DirectiveOperation, TerminalOutcome, UnmeasuredReason,
    measure_control_record,
};
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{ZrdValue, objective_record, zrd_flat_fields};
use cs_types::content::{ContentId, ContentKind};

/// The census row label of the mission.
pub(crate) const MISSION: &str = "zbd/c1b/m03";

/// The numbered blocks of the control member.
const BLOCKS: u32 = 55;
/// The directive sites of the control member.
const SITES: u32 = 313;
/// The distinct directive keys of the control member.
const KEYS: usize = 40;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: M03-B needs the retail capability; run this suite with \
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
        let children = value.as_list().expect("every M03 block is a list");
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
        .expect("the rule finds M03's control member")
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
// Retail: what M03's control program is
// ---------------------------------------------------------------------------

/// **The control program is the member that declares the numbered blocks.**
///
/// The member is found by the content rule: of the 22 members of M03's reader
/// archive exactly one declares numbered `OBJECTIVE<N>` blocks. The blocks and
/// sites the census measures equal an independent walk of the same document,
/// and the archive is the program span M03-A bound.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m03_b_the_control_program_is_the_member_that_declares_the_blocks() {
    let row = census().row(MISSION).expect("M03 is in the census");
    assert_eq!(row.container, "ZBD/C1B/M03/zrdr.zbd");
    assert_eq!(
        row.container_sha256, "5a3051e025b1877eae01be1c78136a99c6650f599bf20ca858eb3c54e4f76ddc",
        "the reader archive is the program M03-A bound"
    );
    assert_eq!(row.members.len(), 22);
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

    let record = row.record().expect("M03 has a measured control program");
    assert_eq!((record.blocks(), record.sites()), (BLOCKS, SITES));
    assert_eq!(record.keys().len(), KEYS);

    let blocks = blocks_of(&control_document());
    let numbers: Vec<u32> = blocks.iter().map(|(number, _)| *number).collect();
    assert_eq!(
        numbers,
        (1..=BLOCKS).collect::<Vec<_>>(),
        "numbered 1..=55, no gaps"
    );
    let walked: usize = blocks.iter().map(|(_, directives)| directives.len()).sum();
    assert_eq!(walked as u32, record.sites(), "the independent walk agrees");
    assert!(
        record.refusals().is_empty(),
        "every block is a readable list"
    );
    assert!(
        record.unclassified_record_keys().is_empty(),
        "unlike one other mission, M03 spells no record key outside the measured vocabulary"
    );
}

/// **Every directive key has exactly one disposition, and one is refused.**
///
/// Two keys are terminal outcomes (`INSTANTWIN`, `INSTANTLOSS`, one site each),
/// one is unmeasured because its two sites disagree about their shape, and the
/// other 37 have a measured effect. The sites are accounted for: the keys'
/// sites sum to the record's.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m03_b_every_directive_m03_spells_has_a_disposition_and_one_is_refused() {
    let record = census().row(MISSION).unwrap().record().unwrap();
    let mut outcomes = BTreeMap::new();
    let mut refused = Vec::new();
    let mut measured = 0;
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
    assert_eq!(
        refused,
        [(
            "WAKEUP_OBJECTIVE_WHEN_I_COMPLETE".to_owned(),
            UnmeasuredReason::DisagreeingArgumentShape { shapes: 2 },
            2
        )],
        "the one refused key spells [int] at one site and [int,int] at the other"
    );
    assert_eq!(measured, 37);
    let sites: u32 = record.keys().iter().map(|key| key.sites).sum();
    assert_eq!(sites, SITES, "no site is dropped from the accounting");
}

/// **The record does not lower, and the reason is three named sites.**
///
/// Two refused `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` sites (blocks 13 and 14,
/// 1-based) and the `SET_AI_NET` site of block 10, whose ten `{actor, net}`
/// pairs exceed the host-call argument bound. All 310 other sites bind and all
/// 55 conditions lower, yet no program stands: `validation` is `None`, the
/// `call_arguments` requirement is the only unmet one, and the row is not
/// complete.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m03_b_the_record_does_not_lower_and_exactly_three_sites_refuse() {
    let row = census().row(MISSION).unwrap();
    let attempt = row.lowering_attempt().unwrap();
    let lowered = attempt.attempt();
    assert_eq!(lowered.mission.as_deref(), Ok("mission/ch1-m03"));
    assert_eq!(lowered.objectives, BLOCKS);
    assert_eq!(lowered.calls.len() as u32, SITES);
    assert!(
        lowered
            .conditions
            .iter()
            .all(|c| *c == cs_content::mission_control::ConditionOutcome::Lowered)
    );
    let refusals: Vec<(usize, &str)> = lowered
        .calls
        .iter()
        .enumerate()
        .filter_map(|(index, call)| match call {
            CallOutcome::Refused(text) => Some((index, text.as_str())),
            CallOutcome::Bound => None,
        })
        .collect();
    assert_eq!(refusals.len(), 3);
    assert!(
        refusals[0]
            .1
            .contains("objective#9 call 3: unknown host call `SET_AI_NET`")
    );
    assert!(
        refusals[1]
            .1
            .contains("objective#12 call 3: unknown host call `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE`")
    );
    assert!(
        refusals[2]
            .1
            .contains("objective#13 call 4: unknown host call `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE`")
    );
    assert_eq!(
        lowered.unbound_keys,
        ["`SET_AI_NET`: binding `SET_AI_NET`: too many arguments"],
        "the registry refused SET_AI_NET at registration, on the host-call bound"
    );
    assert!(
        lowered.validation.is_none(),
        "no program stood to be validated"
    );
    assert!(
        attempt.program().is_none(),
        "and none is handed to the runtime"
    );

    let lowering = row.lowering().unwrap();
    let unmet: Vec<String> = lowering.unmet().map(|r| r.kind.code().to_owned()).collect();
    assert_eq!(unmet, ["call_arguments"]);
    assert!(!row.is_complete());
    assert!(!lowering.complete());
}

/// **The sheet's three priorities are in the record and resolve to measured
/// operations.**
///
/// * grouped objectives: `DEDG` (enemy-group depletion) completes blocks 11, 31,
///   32 and 46, with the group at child0 and a remaining count of 0;
/// * world damage state: 111 `INACTIVE<n>` sites (in-play bit of listed
///   members) with their completion counts;
/// * completion ordering: wake, kill and nap chains plus two
///   `TICK_DEPENDS_ON_OBJ` gates, both on block 54.
///
/// Every key is asserted to resolve to the operation the shared finding
/// measured, so a key that silently lost its meaning fails here.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m03_b_the_sheet_priorities_resolve_to_measured_operations() {
    let record = census().row(MISSION).unwrap().record().unwrap();
    let operation = |key: &str| match record.key(key).unwrap().disposition() {
        DirectiveDisposition::Measured(measured) => measured.operation,
        other => panic!("{key} is not measured: {other:?}"),
    };
    assert_eq!(operation("DEDG"), DirectiveOperation::EnemyGroupDepletion);
    assert_eq!(operation("INACTIVE1"), DirectiveOperation::InactiveMembers);
    assert_eq!(
        operation("INACTIVE_COMPLETION_COUNT"),
        DirectiveOperation::InactiveThreshold
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
        operation("NAP_OBJECTIVE_WHEN_I_COMPLETE"),
        DirectiveOperation::NapObjective
    );
    assert_eq!(
        operation("TICK_DEPENDS_ON_OBJ"),
        DirectiveOperation::DependencyGate
    );

    let blocks = blocks_of(&control_document());
    let with = |key: &str| -> Vec<u32> {
        blocks
            .iter()
            .filter(|(_, d)| d.iter().any(|d| d.key == key))
            .map(|(n, _)| *n)
            .collect()
    };
    assert_eq!(with("DEDG"), [11, 31, 32, 46]);
    let dedg: Vec<Vec<i64>> = blocks
        .iter()
        .flat_map(|(_, d)| d.iter().filter(|d| d.key == "DEDG").map(addresses))
        .collect();
    assert_eq!(dedg, [vec![3, 0], vec![1, 0], vec![2, 0], vec![3, 0]]);

    let inactive_sites: usize = blocks
        .iter()
        .flat_map(|(_, d)| d.iter())
        .filter(|d| d.key.starts_with("INACTIVE") && d.key != "INACTIVE_COMPLETION_COUNT")
        .count();
    assert_eq!(inactive_sites, 111);
    assert_eq!(with("INACTIVE_COMPLETION_COUNT").len(), 8);

    assert_eq!(with("WAKE_OBJECTIVE_WHEN_I_COMPLETE").len(), 20);
    assert_eq!(with("KILL_OBJECTIVE_WHEN_I_COMPLETE").len(), 15);
    assert_eq!(with("NAP_OBJECTIVE_WHEN_I_COMPLETE").len(), 18);
    assert_eq!(with("WAKEUP_OBJECTIVE_WHEN_I_COMPLETE"), [13, 14]);
    assert_eq!(with("TICK_DEPENDS_ON_OBJ"), [31, 41]);
}

/// **The completion chain to `INSTANTWIN` and `INSTANTLOSS` is gated, and every
/// cross-objective address is a block of this record.**
///
/// Both terminal blocks start dormant with no timed wake (`BEGIN_DORMANT`
/// `-1`), so neither can fire on its own clock. Block 33 (`INSTANTWIN`) is named
/// by exactly one other block, 32, through a nap; block 7 (`INSTANTLOSS`) by
/// blocks 6, 22 and 23, also through naps. Every wake, kill, nap and gate
/// address lies in `1..=55`: the spelled value is the block number (the
/// original `dec`s it to an index), so none points past the record.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m03_b_the_terminal_blocks_are_gated_and_every_address_is_in_range() {
    let blocks = blocks_of(&control_document());
    let block = |number: u32| &blocks.iter().find(|(n, _)| *n == number).unwrap().1;

    for (number, outcome) in [(33, "INSTANTWIN"), (7, "INSTANTLOSS")] {
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
    assert_eq!(outcomes, [7, 33], "no other block ends the mission");

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
        edges, 84,
        "the address walk visits every spelled address: 25 wake, 3 wakeup, 36 kill, 18 nap, 2 gate"
    );

    let incoming = |target: i64| -> Vec<(u32, String)> {
        blocks
            .iter()
            .flat_map(|(n, d)| {
                d.iter()
                    .filter(|d| {
                        d.key.ends_with("_OBJECTIVE_WHEN_I_COMPLETE")
                            && addresses(d).contains(&target)
                    })
                    .map(move |d| (*n, d.key.clone()))
            })
            .collect()
    };
    assert_eq!(
        incoming(33),
        [(32, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned())]
    );
    assert_eq!(
        incoming(7),
        [
            (6, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned()),
            (22, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned()),
            (23, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned()),
        ]
    );
}

/// **M03 is not campaign-ready and the census does not hide it.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m03_b_the_mission_stays_unready_until_the_refused_sites_are_measured() {
    let census = census();
    assert!(!census.complete_missions().contains(&MISSION));
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
        ContentId::from_source(ContentKind::Mission, "accept-m03-b").map_err(|e| e.to_string()),
        "accept-m03-b",
        document,
        &record,
    )
}

/// **A wake key that spells two shapes is refused and no majority is taken.**
///
/// M03's two `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` sites spell `[int]` and
/// `[int,int]`. Authored the same way, the key is unmeasured with a
/// disagreeing-shape reason and the program does not assemble.
#[test]
fn accept_m03_b_a_key_with_two_shapes_is_refused_and_no_program_assembles() {
    let document = record_of(vec![
        (
            1,
            vec![
                text("WAKEUP_OBJECTIVE_WHEN_I_COMPLETE"),
                ZrdValue::List(vec![int(2)]),
            ],
        ),
        (
            2,
            vec![
                text("WAKEUP_OBJECTIVE_WHEN_I_COMPLETE"),
                ZrdValue::List(vec![int(1), int(1)]),
            ],
        ),
    ]);
    let record = measure_control_record(&document);
    let key = record.key("WAKEUP_OBJECTIVE_WHEN_I_COMPLETE").unwrap();
    assert_eq!(
        key.disposition(),
        DirectiveDisposition::Unmeasured {
            reason: UnmeasuredReason::DisagreeingArgumentShape { shapes: 2 }
        }
    );
    let lowered = lower(&document);
    assert!(lowered.program().is_none());
    assert!(
        lowered
            .attempt()
            .calls
            .iter()
            .all(|call| matches!(call, CallOutcome::Refused(_)))
    );
}

/// **A `SET_AI_NET` site with ten pairs is refused on the host-call bound; one
/// pair binds.** The same key at two sizes, so the refusal is the size and not
/// the spelling.
#[test]
fn accept_m03_b_a_net_assignment_past_the_host_call_bound_refuses_and_a_small_one_binds() {
    let pairs = |count: usize| -> ZrdValue {
        ZrdValue::List(
            (0..count)
                .map(|n| ZrdValue::List(vec![text(&format!("actor{n}")), text("net")]))
                .collect(),
        )
    };
    let small = record_of(vec![(1, vec![text("SET_AI_NET"), pairs(1)])]);
    assert!(
        lower(&small).attempt().unbound_keys.is_empty(),
        "one pair is inside the host-call bound"
    );
    let big = record_of(vec![(1, vec![text("SET_AI_NET"), pairs(10)])]);
    let lowered = lower(&big);
    assert!(lowered.program().is_none());
    assert!(
        lowered
            .attempt()
            .unbound_keys
            .iter()
            .any(|key| key.contains("SET_AI_NET") && key.contains("too many arguments")),
        "{:?}",
        lowered.attempt().unbound_keys
    );
}
