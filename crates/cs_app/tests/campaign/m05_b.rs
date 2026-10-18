//! Acceptance stage M05-B: the mission-specific compatibility gaps of the
//! fifth mission (`missions/M05.md`, work order `M05-B`).
//!
//! M05-A bound M05's identities. The machinery that measures a control program
//! (the `.zrd` reader, the census, the directive dispositions and the record →
//! `RawProgram` adapter) is shared and was built for M01; this stage runs it
//! over M05's own reader archive and pins what is **different** at M05:
//!
//! * M05's control program is `objectives.zrd` with 68 numbered blocks and 200
//!   directive sites, selected by the content rule and not by its name;
//! * all 20 of its directive keys have a disposition: 2 terminal outcomes and
//!   18 with a measured effect, **none refused**;
//! * unlike M02 and M03 the record **lowers**: every site binds, every
//!   condition lowers, a `MissionProgram` stands and the census row is complete;
//! * the sheet's three priorities (wave lifecycle, protected actor damage,
//!   timed completion) are present in the record and resolve to measured
//!   operations, and the chains that reach `INSTANTWIN` and `INSTANTLOSS` are
//!   pinned address by address, including the precedence between them.
//!
//! **Which archive.** Under the corrected campaign layout
//! `missions/bindings/M05.json` binds work order M05 to `mission/ch1-m05` /
//! `script/c3-m05-zrdr` / `ZBD/C3/M05/zrdr.zbd` (69 252 bytes). The earlier pins
//! of this suite measured `ZBD/C1/M05/zrdr.zbd` (58 blocks, 208 sites, 22 keys),
//! which the corrected layout binds to work order **M10** (`mission/ch2-m05`,
//! `script/c1-m05-zrdr`). They stayed silent because every assertion — the
//! container digest, the counts, even `lowered.mission == "mission/ch2-m05"` —
//! was pinned from the archive itself and none compared it with the production
//! binding. `accept_m05_b_the_control_program_is_the_member_that_declares_the_blocks`
//! now asserts the binding's mission, program id, asset, length and digests
//! (task #1619; the same class as M04-B and M13-B #1600).
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
pub(crate) const MISSION: &str = "zbd/c3/m05";
/// The reader archive `missions/bindings/M05.json` cites as the program span.
const CONTAINER: &str = "ZBD/C3/M05/zrdr.zbd";
/// SHA-256 of that whole archive, from production discovery.
const CONTAINER_SHA256: &str = "78e7ad3f65747fd8f7f8db77b9b9db4a5d84429203b1d5a6bd2b857210585d17";
/// The archive's length in bytes.
const CONTAINER_LENGTH: u64 = 69_252;
/// The member the measured rule chose.
const CONTROL_MEMBER: &str = "objectives.zrd";
/// SHA-256 of the control member's own bytes.
const CONTROL_SHA256: &str = "aed2a5da44086274c4ea3bb1426fd71721a44ee14125d21c82704ab816b164b5";
/// The control member's first byte inside the archive.
const CONTROL_OFFSET: u64 = 15_718;
/// The control member's length in bytes.
const CONTROL_LENGTH: u64 = 13_998;

/// The numbered blocks of the control member.
const BLOCKS: u32 = 68;
/// The directive sites of the control member.
const SITES: u32 = 200;
/// The distinct directive keys of the control member.
const KEYS: usize = 20;
/// The spelled wake/kill/nap/gate addresses.
const EDGES: usize = 97;

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

/// The production control-program binding of `M05`, built once for the suite.
fn control_binding() -> &'static MissionControlBinding {
    static BINDING: OnceLock<MissionControlBinding> = OnceLock::new();
    BINDING.get_or_init(|| {
        let title = load_inventory()
            .iter()
            .find(|(label, _)| label.as_str() == "M05")
            .map(|(_, title)| title.clone())
            .expect("the declared inventory has an M05 work order");
        SourceContext::read(&game_dir())
            .expect("production source context reads the installation")
            .control_program(
                MissionLabel::new("M05").expect("M05 is a valid label"),
                &title,
            )
            .expect("M05's control program binds through the measured rule")
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
/// Of the 15 members of M05's reader archive exactly one declares numbered
/// `OBJECTIVE<N>` blocks. The blocks and sites the census measures equal an
/// independent walk of the same document, and the production control binding
/// names the mission, program, archive, member and digests the layout binds to
/// work order M05.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m05_b_the_control_program_is_the_member_that_declares_the_blocks() {
    let row = census().row(MISSION).expect("M05 is in the census");
    assert_eq!(row.container, CONTAINER);
    assert_eq!(
        row.container_sha256, CONTAINER_SHA256,
        "the reader archive is the program M05-A bound"
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

    let record = row.record().expect("M05 has a measured control program");
    assert_eq!((record.blocks(), record.sites()), (BLOCKS, SITES));
    assert_eq!(record.keys().len(), KEYS);

    let blocks = blocks_of(&control_document());
    let numbers: Vec<u32> = blocks.iter().map(|(number, _)| *number).collect();
    assert_eq!(
        numbers,
        (1..=BLOCKS).collect::<Vec<_>>(),
        "numbered 1..=68, no gaps"
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

    // The production binding and the census must not disagree about the
    // mission, the program, the member or the record. This is the assertion
    // that would have caught the stale `ZBD/C1/M05` pin.
    let bound = control_binding();
    assert_eq!(bound.mission.as_str(), "mission/ch1-m05");
    assert_eq!(bound.program_id.as_str(), "script/c3-m05-zrdr");
    assert_eq!(bound.program_asset, CONTAINER);
    assert_eq!(bound.program_length, CONTAINER_LENGTH);
    assert_eq!(bound.program_sha256, row.container_sha256);
    assert_eq!(bound.control_member, CONTROL_MEMBER);
    assert_eq!(
        (bound.control_offset, bound.control_length),
        (CONTROL_OFFSET, CONTROL_LENGTH)
    );
    assert_eq!(
        (bound.record.blocks(), bound.record.sites()),
        (BLOCKS, SITES)
    );
    assert_eq!(bound.record.vocabulary(), KEYS as u32);

    // The spans and digests re-derive from the archive's own bytes.
    let bytes = std::fs::read(game_dir().join(CONTAINER)).expect("the archive reads");
    assert_eq!(bytes.len() as u64, CONTAINER_LENGTH);
    assert_eq!(sha256(&bytes).to_hex(), CONTAINER_SHA256);
    let start = CONTROL_OFFSET as usize;
    let end = start + CONTROL_LENGTH as usize;
    assert_eq!(
        sha256(&bytes[start..end]).to_hex(),
        CONTROL_SHA256,
        "the control member's digest re-derives from the member's own bytes"
    );
    assert_eq!(bound.control_sha256, CONTROL_SHA256);
}

/// **Every directive key has exactly one disposition and none is refused.**
///
/// Two keys are terminal outcomes (`INSTANTWIN`, `INSTANTLOSS`, one site each)
/// and the other 18 have a measured effect. The sites are accounted for: the
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
    assert_eq!(measured, 18);
    let sites: u32 = record.keys().iter().map(|key| key.sites).sum();
    assert_eq!(sites, SITES, "no site is dropped from the accounting");
}

/// **The record lowers: every site binds, every condition lowers and a program
/// stands.**
///
/// This is the difference from M02 and M03, whose records refuse at the
/// host-call bound. M05's keys all resolve and no argument shape disagrees, so
/// the registry binds all 200 sites, all 68 conditions lower, the lowering is validated and the row is complete. The
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
/// * wave lifecycle: the single `WAKEUP_ENEMIES` site (block 8), `WAKE_ANIM`,
///   `ANIM_STATE`, `WAKEUP_ZEP_TURRETS` and the wake chains through
///   `WAKE_OBJECTIVE_WHEN_I_COMPLETE`;
/// * protected actor damage: `INACTIVE1` (the in-play bit of the listed
///   zeppelin) in blocks 21, 24 and 47, `DEDG` group depletion and `TRAVELERS`;
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
    assert_eq!(
        operation("WAKEUP_ZEP_TURRETS"),
        DirectiveOperation::WakeZeppelinTurrets
    );
    assert_eq!(operation("WAKE_ANIM"), DirectiveOperation::WakeAnimation);
    assert_eq!(operation("ANIM_STATE"), DirectiveOperation::AnimationStates);
    assert_eq!(
        operation("WAKE_OBJECTIVE_WHEN_I_COMPLETE"),
        DirectiveOperation::WakeObjectives
    );
    assert_eq!(operation("INACTIVE1"), DirectiveOperation::InactiveMembers);
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
    assert_eq!(
        operation("IDENTITY"),
        DirectiveOperation::PresentationIdentity
    );
    assert_eq!(operation("SET_AI_NET"), DirectiveOperation::AssignNet);
    assert_eq!(
        operation("SET_HELP_LABEL"),
        DirectiveOperation::SetHelpLabel
    );
    assert_eq!(
        operation("STOP_QUEUED_SOUNDS"),
        DirectiveOperation::StopQueuedSounds
    );
    assert_eq!(
        operation("COMPLETED_SOUND_GROUP"),
        DirectiveOperation::CompletedSoundGroup
    );
    assert_eq!(
        operation("ADD_OBJECTIVE_TARGET"),
        DirectiveOperation::SetTargetFlag {
            objective: true,
            set: true
        }
    );

    let blocks = blocks_of(&control_document());
    let with = |key: &str| blocks_with(&blocks, key);

    // Wave lifecycle: the zeppelin turrets wake at block 1, the escort wing wakes
    // once (block 8), and the bomb runs are `WAKE_ANIM` launchers.
    assert_eq!(with("WAKEUP_ZEP_TURRETS"), [1]);
    assert_eq!(with("WAKEUP_ENEMIES"), [8]);
    assert_eq!(
        with("WAKE_ANIM"),
        [
            19, 26, 35, 36, 37, 38, 39, 40, 42, 48, 49, 50, 51, 52, 53, 56
        ]
    );
    assert_eq!(with("ANIM_STATE"), [18, 19]);
    assert_eq!(with("SET_AI_NET"), [4, 23, 68]);

    // Protected actor damage: the `INACTIVE` blocks and the depletion counts.
    assert_eq!(with("INACTIVE1"), [21, 24, 47]);
    assert_eq!(
        with("DEDG"),
        [5, 10, 11, 12, 20, 22, 30, 44, 45, 46, 55, 63]
    );
    let dedg: Vec<Vec<i64>> = blocks
        .iter()
        .flat_map(|(_, d)| d.iter().filter(|d| d.key == "DEDG").map(addresses))
        .collect();
    assert_eq!(
        dedg,
        [
            vec![1, 0],
            vec![4, 0],
            vec![2, 0],
            vec![1, 1],
            vec![5, 1],
            vec![5, 0],
            vec![2, 1],
            vec![1, 0],
            vec![2, 0],
            vec![4, 0],
            vec![5, 1],
            vec![5, 1]
        ]
    );
    assert_eq!(with("TRAVELERS"), [32, 33, 34, 57]);
    assert_eq!(with("TICK_DEPENDS_ON_OBJ"), [11, 43]);
    assert_eq!(with("ADD_OBJECTIVE_TARGET"), [43]);

    // Timed completion: three blocks carry their own clock (block 1 at second
    // 2, block 3 at 10, block 58 at 7), the naps carry the other timers.
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
    assert_eq!(timed, [(1, 2.0), (3, 10.0), (58, 7.0)]);
    let always_awake: Vec<u32> = blocks
        .iter()
        .filter(|(_, d)| !d.iter().any(|d| d.key == "BEGIN_DORMANT"))
        .map(|(n, _)| *n)
        .collect();
    assert_eq!(
        always_awake,
        [5, 12, 18, 22, 29, 55, 63],
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
            (5, 8, 15.0),
            (8, 68, 2.0),
            (11, 2, 3.0),
            (20, 26, 5.0),
            (22, 25, 20.0),
            (33, 41, 10.0),
            (34, 42, 10.0),
            (35, 48, 0.5),
            (36, 49, 0.5),
            (37, 50, 0.5),
            (38, 48, 0.5),
            (39, 49, 0.5),
            (40, 50, 0.5),
            (41, 34, 40.0),
            (42, 25, 10.0),
            (48, 51, 0.5),
            (49, 52, 0.5),
            (50, 53, 0.5),
            (55, 7, 2.0)
        ]
    );
}

/// **The completion chains to `INSTANTWIN` and `INSTANTLOSS` are gated, the
/// loss is on a timer, and every cross-objective address is a block of this
/// record.**
///
/// Both terminal blocks start dormant with no timed wake (`BEGIN_DORMANT`
/// `-1`). Block 25 (`INSTANTLOSS`) is named only by two naps, blocks 22 (20
/// seconds) and 42 (10 seconds); block 19 (`INSTANTWIN`) is woken only by block
/// 43. No other block ends the mission. Every wake, kill, nap and gate address
/// lies in `1..=68`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m05_b_the_terminal_blocks_are_gated_and_every_address_is_in_range() {
    let blocks = blocks_of(&control_document());
    let block = |number: u32| &blocks.iter().find(|(n, _)| *n == number).unwrap().1;

    for (number, outcome) in [(19, "INSTANTWIN"), (25, "INSTANTLOSS")] {
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
    assert_eq!(outcomes, [19, 25], "no other block ends the mission");

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
        incoming(&blocks, 25),
        [
            (22, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned()),
            (42, "NAP_OBJECTIVE_WHEN_I_COMPLETE".to_owned())
        ],
        "the loss is armed only by two timed naps"
    );
    assert_eq!(
        incoming(&blocks, 19),
        [(43, "WAKE_OBJECTIVE_WHEN_I_COMPLETE".to_owned())],
        "the win is woken only by block 43"
    );
    // The mission's opening route: the timed wake at second 2 is block 1, which
    // nothing else addresses, and block 30 wakes the bomb-run block 4, which block 67's
    // cleanup kills.
    assert_eq!(
        incoming(&blocks, 4),
        [
            (30, "WAKE_OBJECTIVE_WHEN_I_COMPLETE".to_owned()),
            (67, "KILL_OBJECTIVE_WHEN_I_COMPLETE".to_owned())
        ]
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
