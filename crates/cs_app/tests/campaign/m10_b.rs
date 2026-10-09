//! Acceptance stage M10-B: the mission-specific compatibility gaps of the
//! tenth mission (`missions/M10.md`, work order `M10-B`).
//!
//! M10-A bound M10's identities and left the mission program unmeasured. The
//! machinery that measures a control program (the `.zrd` reader, the census,
//! the directive dispositions and the record → `RawProgram` adapter) is shared
//! and was built for M01; this stage runs it over M10's own reader archive and
//! pins what is **different** at M10, so the gaps are recorded as measurements
//! and not discovered later as a silent failure:
//!
//! * M10's control program is `objectives.zrd` with 49 numbered blocks and 232
//!   directive sites, selected by the content rule and not by its name;
//! * every one of its 36 directive keys is measured or terminal: none is refused
//!   and every site binds to a host call;
//! * the record still does **not** lower to a *valid* program: two
//!   `TRAVELERS` sites (blocks 22 and 35) arm the counting mode, whose
//!   evaluation writes a counter, so no side-effect-free condition can carry
//!   them (the program assembles; validation reports them). The mission is not
//!   ready;
//! * the sheet's three priorities name no key, so what is pinned is the
//!   structure they live in: the completion chain to `INSTANTWIN` (block 28) and
//!   `INSTANTLOSS` (block 12), the group-depletion blocks, the world damage
//!   state (`INACTIVE<n>`) and the `TRAVELERS` sites.
//!
//! No behaviour is invented here: the unmeasured piece is filed as a follow-up
//! (see `docs/findings/2026-10-09-m10-b-control-program-gaps.md`) and the mission
//! stays unready until it is measured.
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
use cs_content::mission_control::{
    CallOutcome, ConditionOutcome, DirectiveDisposition, DirectiveOperation, TerminalOutcome,
    measure_control_record,
};
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{ZrdValue, objective_record, zrd_flat_fields};
use cs_types::content::{ContentId, ContentKind};

/// The census row label of the mission.
pub(crate) const MISSION: &str = "zbd/c2/m05";

/// The numbered blocks of the control member.
const BLOCKS: u32 = 49;
/// The directive sites of the control member.
const SITES: u32 = 232;
/// The distinct directive keys of the control member.
const KEYS: usize = 36;

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

// ---------------------------------------------------------------------------
// Retail: what M10's control program is
// ---------------------------------------------------------------------------

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
    assert_eq!(row.container, "ZBD/C2/M05/zrdr.zbd");
    assert_eq!(
        row.container_sha256, "df57933f3a17c36ba0eced37ef7aa3fa95070c521167ea37851388d44e7b4390",
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
        "numbered 1..=49, no gaps"
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
}

/// **Every directive key has a disposition and none is refused.**
///
/// Two keys are terminal outcomes (`INSTANTWIN`, `INSTANTLOSS`, one site each);
/// the other 34 have a measured effect. Unlike M03, no key spells two argument
/// shapes. The sites are accounted for: the keys' sites sum to the record's.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m10_b_every_directive_m10_spells_is_measured_or_terminal() {
    let record = census().row(MISSION).unwrap().record().unwrap();
    let mut outcomes = BTreeMap::new();
    let mut measured = 0;
    for key in record.keys() {
        match key.disposition() {
            DirectiveDisposition::TerminalOutcome { outcome } => {
                outcomes.insert(key.key.clone(), outcome);
            }
            DirectiveDisposition::Measured(_) => measured += 1,
            DirectiveDisposition::Unmeasured { reason } => {
                panic!("{} is unmeasured ({reason:?})", key.key)
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
    assert_eq!(measured, 34);
    let sites: u32 = record.keys().iter().map(|key| key.sites).sum();
    assert_eq!(sites, SITES, "no site is dropped from the accounting");
}

/// **The record does not lower, and the reason is two named blocks.**
///
/// All 232 sites bind and 47 of 49 conditions lower, yet the two `TRAVELERS`
/// blocks (22 and 35, 1-based) arm the counting mode (a non-string `child0`),
/// whose evaluation accumulates a count into the objective: a write, so no
/// side-effect-free predicate carries it. Because every call binds, a program is
/// assembled, but its validation reports the unsupported condition instruction,
/// so `objective_condition` and `call_arguments` are the unmet rows and the
/// mission is not complete.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m10_b_the_record_does_not_lower_and_exactly_two_conditions_refuse() {
    let row = census().row(MISSION).unwrap();
    let attempt = row.lowering_attempt().unwrap();
    let lowered = attempt.attempt();
    assert_eq!(lowered.mission.as_deref(), Ok("mission/ch2-m05"));
    assert_eq!(lowered.objectives, BLOCKS);
    assert_eq!(lowered.calls.len() as u32, SITES);
    assert!(
        lowered
            .calls
            .iter()
            .all(|call| matches!(call, CallOutcome::Bound)),
        "every one of the 232 sites binds to a host call"
    );
    assert!(lowered.unbound_keys.is_empty());

    let refused: Vec<(usize, &str)> = lowered
        .conditions
        .iter()
        .enumerate()
        .filter_map(|(index, condition)| match condition {
            ConditionOutcome::Lowered => None,
            ConditionOutcome::Refused(text) => Some((index, text.as_str())),
            ConditionOutcome::Unreadable(text) => panic!("block {index} is unreadable: {text}"),
        })
        .collect();
    assert_eq!(refused.len(), 2);
    assert_eq!(
        refused.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
        [21, 34],
        "zero-based: OBJECTIVE22 and OBJECTIVE35"
    );
    for (index, text) in &refused {
        assert!(
            text.contains(&format!("`OBJECTIVE{}` `TRAVELERS`", index + 1))
                && text.contains("a non-string subject arms TRAVELERS' counting mode")
                && text.contains("a write, so no side-effect-free predicate can carry it"),
            "{text}"
        );
    }

    let validation = lowered
        .validation
        .as_ref()
        .expect("the program assembled and was validated");
    assert!(
        validation.iter().any(|problem| problem
            .contains("mission/ch2-m05 objective#21 [condition]: unsupported instruction")),
        "{validation:?}"
    );
    assert!(
        attempt.program().is_some(),
        "every call bound, so a program is assembled; it is the validation that fails"
    );

    let lowering = row.lowering().unwrap();
    let unmet: Vec<String> = lowering.unmet().map(|r| r.kind.code().to_owned()).collect();
    assert_eq!(unmet, ["objective_condition", "call_arguments"]);
    assert!(!row.is_complete());
    assert!(!lowering.complete());
}

/// **The `TRAVELERS` sites that refuse are the two that spell the counting
/// mode, and they spell the polarity token the findings left unknown.**
///
/// Both sites spell a numeric `child0` (`2`), a polarity word, the anchor
/// `cargozep2`, a radius and a trailing `1`. Block 22 spells `APPROACHING`
/// (radius 200), block 35 spells `LEAVING` (radius 2000) and `DELETE_ON_SUCCESS`.
/// The measured rule treats every spelling other than `APPROACHING` as the
/// outside case; whether the original compares the second word with `LEAVING`
/// is **not** established here, and `LEAVING` is recorded only as spelled.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m10_b_the_two_refused_conditions_are_the_travelers_counting_sites() {
    let blocks = blocks_of(&control_document());
    let travelers: Vec<(u32, &Vec<ZrdValue>)> = blocks
        .iter()
        .flat_map(|(n, d)| {
            d.iter()
                .filter(|d| d.key == "TRAVELERS")
                .map(move |d| (*n, d.args.as_ref().unwrap()))
        })
        .collect();
    assert_eq!(travelers.len(), 2);
    assert_eq!(travelers[0].0, 22);
    assert_eq!(
        travelers[0].1.as_slice(),
        [
            ZrdValue::Int(2),
            ZrdValue::Text("APPROACHING".to_owned()),
            ZrdValue::Text("cargozep2".to_owned()),
            ZrdValue::Float(200.0),
            ZrdValue::Int(1),
        ]
    );
    assert_eq!(travelers[1].0, 35);
    assert_eq!(
        travelers[1].1.as_slice(),
        [
            ZrdValue::Int(2),
            ZrdValue::Text("LEAVING".to_owned()),
            ZrdValue::Text("cargozep2".to_owned()),
            ZrdValue::Float(2000.0),
            ZrdValue::Int(1),
            ZrdValue::Text("DELETE_ON_SUCCESS".to_owned()),
        ]
    );

    let record = census().row(MISSION).unwrap().record().unwrap();
    let operation = match record.key("TRAVELERS").unwrap().disposition() {
        DirectiveDisposition::Measured(measured) => measured.operation,
        other => panic!("TRAVELERS is not measured: {other:?}"),
    };
    assert_eq!(operation, DirectiveOperation::Travelers);
}

/// **The sheet's three priorities resolve to measured operations.**
///
/// The sheet names a protected neutral actor, attached payloads and air/surface
/// threats without naming a key, so none is invented here. What the record does
/// spell:
///
/// * threats: `DEDG` (enemy-group depletion) completes blocks 26, 27 and 34 with
///   the group at child0 and a remaining count; `WAKEUP_ENEMIES` (block 21) and
///   `WAKEUP_GENERATOR` (blocks 18 and 47) bring them in;
/// * world damage state: 97 `INACTIVE<n>` sites with 10 completion thresholds;
/// * the zeppelin cargo actor `cargozep2` is the anchor of both `TRAVELERS`
///   sites; the AI net and team assignments are blocks 23 and 36.
///
/// Which actor is "protected", "neutral" or "attached" is **not** read from the
/// record; every key is asserted to resolve to the operation the shared
/// finding measured.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m10_b_the_sheet_priorities_resolve_to_measured_operations() {
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
    assert_eq!(operation("WAKEUP_ENEMIES"), DirectiveOperation::WakeEnemies);
    assert_eq!(
        operation("WAKEUP_GENERATOR"),
        DirectiveOperation::FeedGenerator
    );
    assert_eq!(operation("SET_AI_NET"), DirectiveOperation::AssignNet);
    assert_eq!(operation("SET_AI_TEAM"), DirectiveOperation::AssignTeam);

    let blocks = blocks_of(&control_document());
    let with = |key: &str| -> Vec<u32> {
        blocks
            .iter()
            .filter(|(_, d)| d.iter().any(|d| d.key == key))
            .map(|(n, _)| *n)
            .collect()
    };
    assert_eq!(with("DEDG"), [26, 27, 34]);
    let dedg: Vec<Vec<i64>> = blocks
        .iter()
        .flat_map(|(_, d)| d.iter().filter(|d| d.key == "DEDG").map(addresses))
        .collect();
    assert_eq!(dedg, [vec![1, 0], vec![1, 2], vec![2, 0]]);

    let inactive_sites: usize = blocks
        .iter()
        .flat_map(|(_, d)| d.iter())
        .filter(|d| d.key.starts_with("INACTIVE") && d.key != "INACTIVE_COMPLETION_COUNT")
        .count();
    assert_eq!(inactive_sites, 97);
    assert_eq!(with("INACTIVE_COMPLETION_COUNT").len(), 10);

    assert_eq!(with("WAKEUP_ENEMIES"), [21]);
    assert_eq!(with("WAKEUP_GENERATOR"), [18, 47]);
    assert_eq!(with("SET_AI_NET"), [23]);
    assert_eq!(with("SET_AI_TEAM"), [36]);
    assert_eq!(with("TRAVELERS"), [22, 35]);
    assert_eq!(with("WAKE_OBJECTIVE_WHEN_I_COMPLETE").len(), 12);
    assert_eq!(with("KILL_OBJECTIVE_WHEN_I_COMPLETE").len(), 6);
    assert_eq!(with("NAP_OBJECTIVE_WHEN_I_COMPLETE").len(), 12);
    assert_eq!(with("TICK_DEPENDS_ON_OBJ"), [19]);
}

/// **The completion chain to `INSTANTWIN` and `INSTANTLOSS` is gated, and every
/// cross-objective address is a block of this record.**
///
/// Both terminal blocks start dormant with no timed wake (`BEGIN_DORMANT`
/// `-1`). Block 28 (`INSTANTWIN`) is named by block 11 (a kill) and block 26 (a
/// nap); block 12 (`INSTANTLOSS`) by blocks 11 and 34, both naps. Block 11 both
/// kills the win and naps the loss: a branch of two systems, pinned here as
/// spelled and not as a verdict on which fires first. Every wake, kill, nap and
/// gate address lies in `1..=49`: the spelled value is the block number (the
/// original `dec`s it to an index), so none points past the record.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m10_b_the_terminal_blocks_are_gated_and_every_address_is_in_range() {
    let blocks = blocks_of(&control_document());
    let block = |number: u32| &blocks.iter().find(|(n, _)| *n == number).unwrap().1;

    for (number, outcome) in [(28, "INSTANTWIN"), (12, "INSTANTLOSS")] {
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
    assert_eq!(outcomes, [12, 28], "no other block ends the mission");

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
    assert_eq!(edges, 52, "the address walk visits every spelled integer");

    let incoming = |target: i64| -> Vec<(u32, String)> {
        blocks
            .iter()
            .flat_map(|(n, d)| {
                d.iter()
                    .filter(|d| {
                        (d.key.ends_with("_OBJECTIVE_WHEN_I_COMPLETE")
                            || d.key == "TICK_DEPENDS_ON_OBJ")
                            && addresses(d).contains(&target)
                    })
                    .map(move |d| (*n, d.key.clone()))
            })
            .collect()
    };
    assert_eq!(
        incoming(28),
        [
            (11, "KILL_OBJECTIVE_WHEN_I_COMPLETE".to_owned()),
            (26, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned()),
        ]
    );
    assert_eq!(
        incoming(12),
        [
            (11, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned()),
            (34, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned()),
        ]
    );
}

/// **M10 is not campaign-ready and the census does not hide it.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m10_b_the_mission_stays_unready_until_the_counting_mode_is_measured() {
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

/// **A numeric `TRAVELERS` subject refuses the condition; a named one lowers.**
///
/// The same key at two subjects, so the refusal is the counting mode and not
/// the spelling: a numeric `child0` arms a counter write and the block does not
/// lower, a named subject is the side-effect-free inside/outside test and does.
#[test]
fn accept_m10_b_a_numeric_travelers_subject_refuses_and_a_named_one_lowers() {
    let counting = lower(&travelers(int(2)));
    assert!(
        matches!(
            counting.attempt().conditions.as_slice(),
            [ConditionOutcome::Refused(text)] if text.contains("counting mode")
        ),
        "{:?}",
        counting.attempt().conditions
    );
    assert!(
        counting
            .attempt()
            .validation
            .as_ref()
            .is_some_and(|problems| !problems.is_empty()),
        "validation reports the unsupported condition"
    );

    let named = lower(&travelers(text("player")));
    assert_eq!(
        named.attempt().conditions,
        [ConditionOutcome::Lowered],
        "a named subject is a predicate"
    );
    assert_eq!(
        named.attempt().validation,
        Some(Vec::new()),
        "and the program validates clean"
    );
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
