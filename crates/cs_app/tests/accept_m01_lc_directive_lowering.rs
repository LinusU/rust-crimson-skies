//! Integration acceptance for `M01-LC-DIRECTIVE-LOWERING` (#717): the three
//! stages compose, and what they produce is **runnable**.
//!
//! Stages `.01` (#724), `.02` (#725) and `.03` (#726) each proved their own
//! layer — the binding vocabulary, the side-effect-free block conditions and
//! the record→`RawProgram` adapter. This suite is the seam between them and
//! `cs_sim::mission`: a control record the census measures lowers through
//! `lower_control_record`, and the resulting `MissionProgram` is handed to
//! [`MissionSession`], which validates it, steps it and — once a caller
//! supplies the lifecycle facts the record's own `BEGIN_DORMANT` spelling
//! declares — drives it to a terminal outcome with every measured directive
//! site reaching the host's directive log.
//!
//! What this suite does **not** claim: no original executable was run and no
//! mission was played, so nothing here is `verified_original`. The world-side
//! half of [`cs_script::runtime::MissionFacts`] — `members`, `groups`,
//! `generators` and `animations` — has had production writers since task
//! `M01-LC-WORLD-FACTS` (#751, `cs_app::world_facts`), which is exactly why
//! the retail case below can still advance with an unpopulated map and assert
//! that nothing completes: an unpopulated [`MissionFacts`] completes nothing
//! rather than completing everything, and that contract survives the writers
//! landing beside it. That suite owns the populated half; this one owns the
//! empty one.
//!
//! Test prefix `accept_m01_lc_directive_lowering_`; the retail case needs
//! `$CS_GAME_DIR` and is `#[ignore]`d so CI runs the synthetic case.

use std::path::PathBuf;

use cs_app::control_lowering::{LoweredControlRecord, lower_control_record};
use cs_app::mission_control::survey_mission_control_programs;
use cs_content::mission_control::measure_control_record;
use cs_content::stunts::ZrdValue;
use cs_script::ir::{DirectiveOperation, Outcome, SymbolId, Value};
use cs_script::runtime::{
    EventKind, MissionFacts, ObjectiveLifecycle, SessionGeneration, TerminalState,
};
use cs_sim::mission::{BlockLifecycleTable, LifecycleDecl, MissionSession};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

/// The census row label of the mission the parent task is about.
const M01: &str = "zbd/c1c/m01";

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

// ------------------------------------------------- the .zrd authoring helpers ---

fn zrd_float(value: f32) -> ZrdValue {
    ZrdValue::Float(value)
}

fn zrd_text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

fn zrd_list(children: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(children)
}

/// One authored directive of a block: the key, and — unless authored bare —
/// its argument list beside it, in the asymmetric grammar the census reads.
fn directive(key: &str, args: Vec<ZrdValue>) -> Vec<ZrdValue> {
    let mut children = vec![zrd_text(key)];
    if !args.is_empty() {
        children.push(zrd_list(args));
    }
    children
}

fn block(number: u32, directives: Vec<Vec<ZrdValue>>) -> (String, ZrdValue) {
    let mut children = Vec::new();
    for children_of_directive in directives {
        children.extend(children_of_directive);
    }
    (format!("OBJECTIVE{number}"), zrd_list(children))
}

fn control_record(fields: Vec<(String, ZrdValue)>) -> ZrdValue {
    let mut children = Vec::new();
    for (key, value) in fields {
        children.push(zrd_text(&key));
        children.push(value);
    }
    zrd_list(vec![zrd_list(children)])
}

/// Runs the production adapter over an authored record: measured, lowered and
/// bound through the record's own key dispositions, exactly as the census runs
/// it for a retail row.
fn lower(document: &ZrdValue) -> LoweredControlRecord {
    let record = measure_control_record(document);
    lower_control_record(
        ContentId::from_source(ContentKind::Mission, "accept-mission")
            .map_err(|error| error.to_string()),
        "accept-mission",
        document,
        &record,
    )
}

// ---------------------------------------------------------------------------
// The synthetic seam: record → program → runtime → terminal outcome
// ---------------------------------------------------------------------------

/// **The lowered program runs, and the record's own lifecycle spellings drive
/// it to a terminal outcome.**
///
/// Two blocks are authored with the spellings stage `.02` measured: `OBJECTIVE1`
/// starts dormant with a timed self-wake at 2 s and carries a wake directive
/// plus the win marker; `OBJECTIVE2` starts awake with a wake directive of its
/// own. The record lowers (every site bound, validation clean), launches in
/// [`MissionSession`] and then:
///
/// * at tick 1 only the awake block completes — the dormant one must not latch,
///   and its `BEGIN_DORMANT` site does **not** run;
/// * the record's own wake time (the `BEGIN_DORMANT` child0, in mission-clock
///   seconds) wakes the dormant block through the production lifecycle table,
///   so at tick 2 it completes;
/// * both completions emit their measured `WakeAnimation`/`DormantStart`
///   operations to the host's directive log with the arguments the sites
///   spelled, and the win marker reaches the runtime as
///   [`TerminalState::Succeeded`].
///
/// Nothing here is a fixture assertion about the original: the record is
/// authored above, and every step below is production code.
#[test]
fn accept_m01_lc_directive_lowering_a_lowered_record_runs_to_a_terminal_outcome() {
    let document = control_record(vec![
        block(
            1,
            vec![
                directive("BEGIN_DORMANT", vec![zrd_float(2.0)]),
                directive("WAKE_ANIM", vec![zrd_text("wv_hookup")]),
                directive("INSTANTWIN", Vec::new()),
            ],
        ),
        block(
            2,
            vec![directive("WAKE_ANIM", vec![zrd_text("other_anim")])],
        ),
    ]);
    let lowered = lower(&document);
    let attempt = lowered.attempt();
    assert_eq!(
        attempt.validation.as_deref(),
        Some(&[][..]),
        "the record validates before anything runs: {:?}",
        attempt.validation
    );
    assert_eq!(
        (attempt.objectives, attempt.calls.len()),
        (2, 4),
        "both blocks and all four sites reached the attempt"
    );

    let program = lowered
        .program()
        .expect("every site bound, so the program assembled")
        .clone();

    // The facts are the caller's (SCRIPT-MISSION, "Objective event ordering"):
    // here they are declared from the record's own spellings — `OBJECTIVE1`
    // begins dormant with its child0 wake time, `OBJECTIVE2` is left awake.
    let mut table = BlockLifecycleTable::new();
    assert_eq!(
        table
            .declare(0, LifecycleDecl::dormant(Some(2.0)))
            .expect("block 1 is declared once"),
        ObjectiveLifecycle::Dormant,
        "the record's BEGIN_DORMANT spells a dormant start"
    );
    table
        .declare(1, LifecycleDecl::awake())
        .expect("block 2 is declared once");

    let mut session =
        MissionSession::launch(program, SessionGeneration(1), []).expect("the program launches");

    let first = session
        .advance(&table.facts(), Tick(1))
        .expect("tick 1 advances");
    assert_eq!(
        first.terminal,
        TerminalState::Running,
        "the dormant block does not latch, so the mission keeps running"
    );
    assert!(
        !session.state().is_completed(SymbolId(0)),
        "the block that starts dormant must not complete at tick 0"
    );
    assert!(
        session.state().is_completed(SymbolId(1)),
        "the block the record leaves awake completes on the first tick it is awake"
    );
    assert!(
        first
            .events
            .iter()
            .any(|event| event.kind == EventKind::ObjectiveCompleted),
        "the completion is an ordered runtime event: {:?}",
        first.events
    );
    let directives = session.state().directives();
    assert_eq!(
        directives.len(),
        1,
        "only the awake block emitted, and the dormant block's own site did not run: {directives:?}"
    );
    assert_eq!(
        (directives[0].operation, directives[0].args.clone()),
        (
            DirectiveOperation::WakeAnimation,
            vec![Value::Str("other_anim".to_owned())]
        ),
        "the measured operation and the site's own arguments reach the host log"
    );

    // The record's own wake time: the BEGIN_DORMANT child0, in mission-clock
    // seconds (finding B), advances the production lifecycle table.
    assert_eq!(
        table.tick(2.0),
        vec![0],
        "the record's own wake time wakes exactly the block it declared"
    );
    let second = session
        .advance(&table.facts(), Tick(2))
        .expect("tick 2 advances");
    assert_eq!(
        second.terminal,
        TerminalState::Succeeded,
        "the win marker the record spells ends the mission"
    );
    assert!(
        second
            .events
            .iter()
            .any(|event| event.kind == EventKind::TerminalRequested(Outcome::Succeeded)),
        "the terminal request is an ordered runtime event: {:?}",
        second.events
    );
    let directives = session.state().directives();
    assert_eq!(
        directives.len(),
        3,
        "both completions emitted, in EventKey order: {directives:?}"
    );
    assert_eq!(
        (
            directives[1].operation,
            directives[1].args.clone(),
            directives[2].operation,
            directives[2].args.clone()
        ),
        (
            DirectiveOperation::DormantStart,
            vec![Value::Float(2.0)],
            DirectiveOperation::WakeAnimation,
            vec![Value::Str("wv_hookup".to_owned())]
        ),
        "the woken block's sites emit their measured operations with the args \
         the record spelled, nested structure intact"
    );
}

// ---------------------------------------------------------------------------
// The retail seam: M01's own record, on the owner's installation
// ---------------------------------------------------------------------------

/// **M01's lowered program launches and steps in the mission runtime.**
///
/// The whole task in one case: `survey_mission_control_programs` measures
/// `zbd/c1c/m01` over the installation, its lowering attempt has met every
/// requirement row (the rows `plan_mission_launch` reads, so the
/// `mission_program` and `mission_objectives` surfaces report Supported), the
/// bound `MissionProgram` is accepted by [`MissionSession`] — the same
/// validation the runtime performs — and advancing it is clean: no budget
/// stop, no refusal, no terminal state.
///
/// The assertions on what *does not* happen are the fail-closed half: with an
/// unpopulated [`MissionFacts`] no numbered block can be awake, so the mission
/// completes nothing and emits nothing. The world-side half of the fact map now
/// has production writers (`cs_app::world_facts`, task `M01-LC-WORLD-FACTS`
/// #751) but **none of them runs here**: this case supplies no world
/// observation at all, so `MissionState::holds` still answers `false` for
/// every key the maps do not hold, and a report that claimed completions from
/// facts nobody observed would be exactly the guesswork this project refuses.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_directive_lowering_m01_lowers_launches_and_steps_in_the_runtime() {
    let census =
        survey_mission_control_programs(&game_dir()).expect("the census runs on the installation");
    let row = census.row(M01).expect("M01 is measured");

    let lowering = row
        .lowering()
        .expect("a measured row lowers, so a launch path can read it");
    assert!(
        lowering.complete() && row.is_complete(),
        "every requirement row is met, which is what the mission_program and \
         mission_objectives surfaces read: {:?}",
        lowering.unmet().map(|row| row.label()).collect::<Vec<_>>()
    );

    let program = row
        .lowering_attempt()
        .expect("a measured row carries the attempt")
        .program()
        .expect("every one of M01's sites bound")
        .clone();

    let mut session = MissionSession::launch(program, SessionGeneration(7), [])
        .expect("the runtime accepts M01's lowered program");
    let mut observed = Vec::new();
    for tick in 1..=3u64 {
        let result = session
            .advance(&MissionFacts::default(), Tick(tick))
            .unwrap_or_else(|error| panic!("tick {tick} advances: {error:?}"));
        assert!(
            result.stop.is_none(),
            "tick {tick} ran inside the work budget: {:?}",
            result.stop
        );
        assert_eq!(
            result.terminal,
            TerminalState::Running,
            "tick {tick} resolves no terminal state on its own"
        );
        observed.extend(result.events);
    }
    assert!(
        observed.is_empty(),
        "an unpopulated MissionFacts completes nothing, so no event fires: {observed:?}"
    );
    assert!(
        session.state().directives().is_empty(),
        "no block completes, so no measured directive reaches the host: {:?}",
        session.state().directives()
    );
}
