//! Acceptance stage VS-M01-RT-MISSION-HOST.02: a terminal state ends the run
//! cleanly, with the exit its outcome maps to (Rally #1279).
//!
//! [`cs_app::mission_session::build_headless`] composes the same systems the
//! `--mission` window runs, so the composed entry
//! ([`cs_app::mission_session::mission_host_tick`]) is what this suite holds
//! to. Three things have to be proved:
//!
//! * **Both sources end the run, and the report line says which.** The
//!   composed entry funnels the F39 objective session's own
//!   `SessionTick::outcome` and the declared control program's own terminal
//!   state into one [`cs_app::mission_session::MissionTerminal`]. Two
//!   synthetic stages drive one settling program each through the composed
//!   entry: one ending `Success`, one ending `Failure`. This member fails if
//!   the terminal path is removed — the assertions are on the `AppExit`
//!   message the composed entry writes and on the report line the terminal
//!   carries, both produced by that path and by nothing else.
//! * **The exit is never a swallowed failure.** `0` for `Success`, `1` for
//!   `Failure` and `Extraction`, and the message the windowed runner exits on
//!   is the same mapping (`docs/contracts/CLI-EVIDENCE.md`: never return zero
//!   after only logging a failure).
//! * **A settled run advances nothing.** After the terminal lands, the host
//!   tick does not move, no second terminal and no second `AppExit` are
//!   written, and [`cs_app::mission_session::MissionHost::step`] itself
//!   answers "already settled" rather than stepping.
//!
//! The objective session's cue queue is drained as part of the terminal
//! sequence, so a third synthetic member counts the cues the session still
//! owned when the run ended — emitted cues the player will never hear are
//! **named** on the terminal, then gone from the queue.
//!
//! Against the owner's installation, M01's real program runs through the same
//! composed entry and settles **no** terminal: both of its `Finish` blocks
//! start dormant behind the `-1` sentinel and every wake path needs world
//! facts a headless run does not have. That is a measured property of the
//! content (#1278's fact 7), so the retail member holds the host to settling
//! nothing and writing no exit for it. It is
//! `#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]`.
//!
//! Every synthetic value here is authored in this file and every stage is the
//! synthetic harbor stage `vs_m01_rt_window.rs` builds. No
//! `ObjectiveSpec`, `CountCondition`, `MissionTimer`, `SweptTrigger` or spawn
//! group is ever constructed for an original mission (AGENTS.md rules 4 and
//! 5); the declared objective program below is synthetic content with an
//! `Origin::SyntheticFixture` and designed provenance, exactly like
//! `cs_content::objectives::declared_synthetic_objectives`.

use std::num::NonZero;
use std::path::Path;
use std::time::Duration;

use bevy::app::AppExit;
use bevy::ecs::message::{MessageCursor, Messages};
use cs_app::mission_launch::{MissionLaunchPlan, plan_mission_launch};
use cs_app::mission_session::{
    MissionHost, MissionHostReport, MissionHostStepError, MissionStage, MissionTerminal,
    MissionTerminalSource, build_headless, stage_for, teardown,
};
use cs_app::objectives::{LoweredObjectives, lower_program};
use cs_content::objectives::{
    DeclaredCompletion, DeclaredObjective, DeclaredObjectiveProgram, DeclaredObjectiveState,
    DeclaredPrecedence, DeclaredRevealRule, DeclaredTerminalOutcome, DeclaredTimeDomain,
    DeclaredTimer, DeclaredTimerAction, DeclaredTimerStart, ProgramSymbol,
};
use cs_script::ir::{Action, Condition, IR_VERSION, MissionProgram, Objective, Outcome, SymbolId};
use cs_script::runtime::TerminalState;
use cs_sim::objectives::terminal::TerminalOutcome;
use cs_types::content::{ContentKind, Known, Origin, Provenance, Resolved};

use crate::common::{cid, claim, label};
use crate::vs_m01_rt_window::{Scratch, game_dir, synthetic_stage};

/// The synthetic stage's settling control program: one objective whose
/// condition is `true`, so the composed entry's script half completes it on
/// its first tick, and whose one action asks for `outcome` — the mission IR's
/// own terminal action, the same one M01's measured `INSTANTWIN`/`INSTANTLOSS`
/// lowering registers. Authored here as synthetic content, never a reading of
/// any original mission's record.
fn settling_control_program(outcome: Outcome) -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-mission-host-terminal"),
        variables: Vec::new(),
        objectives: vec![Objective {
            id: SymbolId(0),
            content: cid(
                ContentKind::Objective,
                "synthetic-mission-host-terminal-objective",
            ),
            condition: Condition::Const(true),
            actions: vec![Action::Finish(outcome)],
            span: None,
        }],
    }
}

/// A synthetic stage whose control program settles `outcome` on its first
/// tick, so the composed entry's funnel has something real to end the run on.
fn settling_stage(scratch: &Path, outcome: Outcome) -> MissionStage {
    let mut stage = synthetic_stage(scratch);
    stage.host.control = settling_control_program(outcome);
    stage
}

/// The objective session's own source for a terminal it settles: the symbol
/// of the declared timer whose expiry asked for the outcome.
const CUE_TIMER: SymbolId = SymbolId(31);
/// The declared timer that asks for the mission's terminal outcome.
const FINISH_TIMER: SymbolId = SymbolId(32);

/// A synthetic declared objective program that settles **through the F39
/// runtime**: one objective that never completes, a radio-cue timer that runs
/// out at tick 2 and leaves one dialogue cue in the session's queue, and a
/// finish timer that runs out at tick 3 and requests `Success`.
///
/// It is lowered through the production
/// [`cs_app::objectives::lower_program`], so nothing here is hand-assembled
/// `LoweredObjectives` state: the composed entry reads the outcome off the
/// objective session's own `SessionTick`, the source this funnel names first.
fn cue_then_finish_objectives() -> LoweredObjectives {
    let provenance = Provenance::designed(claim("vs-m01-rt-host-02.synthetic-cue"));
    let declared = DeclaredObjectiveProgram::try_new(
        cid(ContentKind::Mission, "synthetic.vs-m01-rt.host-02"),
        Origin::SyntheticFixture,
        provenance.clone(),
        Resolved::Known(Known::new(
            DeclaredPrecedence::SyntheticConservative,
            provenance.clone(),
        )),
        vec![DeclaredObjective {
            symbol: ProgramSymbol(1),
            content: cid(
                ContentKind::Objective,
                "synthetic.vs-m01-rt.host-02.objective",
            ),
            initial: DeclaredObjectiveState::Active,
            reveal: DeclaredRevealRule::Immediate,
            on_complete: DeclaredCompletion::Continue,
            completion_effects: Vec::new(),
        }],
        Vec::new(),
        vec![
            DeclaredTimer {
                symbol: ProgramSymbol(CUE_TIMER.0),
                domain: DeclaredTimeDomain::AuthoritativeGameplay,
                start: DeclaredTimerStart::AtTick(1),
                period_ticks: 1,
                action: DeclaredTimerAction::Cue {
                    key: "synthetic.vs-m01-rt.host-02.radio".to_owned(),
                    dialogue: cid(ContentKind::Dialogue, "synthetic.vs-m01-rt.host-02.radio"),
                },
            },
            DeclaredTimer {
                symbol: ProgramSymbol(FINISH_TIMER.0),
                domain: DeclaredTimeDomain::AuthoritativeGameplay,
                start: DeclaredTimerStart::AtTick(2),
                period_ticks: 1,
                action: DeclaredTimerAction::Finish(DeclaredTerminalOutcome::Success),
            },
        ],
        Vec::new(),
        Vec::new(),
    )
    .expect("the synthetic cue program is structurally valid");
    lower_program(&declared).expect("the synthetic cue program lowers")
}

/// Drives the composed entry until the host settles, and answers with the
/// terminal it settled on.
fn run_until_terminal(app: &mut bevy::prelude::App) -> MissionTerminal {
    for _ in 0..16 {
        app.update();
        if let Some(host) = app.world().get_resource::<MissionHost>()
            && let Some(terminal) = host.terminal()
        {
            return terminal.clone();
        }
    }
    panic!("the composed entry never settled the run");
}

/// Every [`AppExit`] message the world has been handed, read straight off the
/// message queue the windowed runner exits on.
fn exits(app: &bevy::prelude::App) -> Vec<AppExit> {
    let mut cursor = MessageCursor::<AppExit>::default();
    read_exits(app, &mut cursor)
}

/// The [`AppExit`] messages `cursor` has not read yet, straight off the
/// message queue. The queue double-buffers, so a caller that wants to prove a
/// **later** frame wrote no second exit keeps one cursor across the frames.
fn read_exits(app: &bevy::prelude::App, cursor: &mut MessageCursor<AppExit>) -> Vec<AppExit> {
    let Some(messages) = app.world().get_resource::<Messages<AppExit>>() else {
        return Vec::new();
    };
    cursor.read(messages).cloned().collect()
}

/// **A control program that settles `Success` ends the run with
/// `AppExit::Success` / code 0, and the host never advances again.**
#[test]
fn accept_vs_m01_runtime_host_02_a_settled_success_ends_the_run_on_a_zero_exit() {
    let scratch = Scratch::new("terminal-success");
    let stage = settling_stage(scratch.path(), Outcome::Succeeded);
    let mut app = build_headless(&stage).expect("the settling synthetic stage composes");

    let terminal = run_until_terminal(&mut app);

    // The outcome, the source that reported it and the record that asked.
    assert_eq!(
        terminal.outcome,
        TerminalOutcome::Success,
        "a program whose Finish asks for Succeeded must settle Success"
    );
    assert_eq!(
        terminal.source,
        MissionTerminalSource::ControlProgram,
        "the control program's own terminal is this run's source"
    );
    assert_eq!(
        terminal.requested_by,
        Some(SymbolId(0)),
        "the objective whose Finish asked is the record the terminal must name"
    );
    assert!(
        terminal.tick.0 >= 1,
        "the terminal belongs to a committed host tick, got {:?}",
        terminal.tick
    );
    assert_eq!(
        terminal.session,
        app.world().resource::<MissionHost>().generation(),
        "the terminal is stamped with the generation this host launched"
    );

    // The exit, and the message the windowed runner ends on.
    assert_eq!(terminal.exit.outcome(), TerminalOutcome::Success);
    assert_eq!(terminal.exit.code(), 0, "success is the only zero exit");
    assert_eq!(terminal.exit.app_exit(), AppExit::Success);
    assert_eq!(
        exits(&app),
        vec![AppExit::Success],
        "the composed entry must send the terminal's exit as an AppExit message"
    );

    // The one report line the run writes, naming every part of it.
    let line = terminal.report_line.as_str();
    assert!(line.contains("source=control_program"), "{line}");
    assert!(
        line.contains(&format!("tick={}", terminal.tick.0)),
        "{line}"
    );
    assert!(
        line.contains(&format!("session={}", terminal.session.0)),
        "{line}"
    );
    assert!(line.contains("requested_by=SymbolId(0)"), "{line}");
    assert!(line.contains("exit_code=0"), "{line}");

    // A settled run answers "already settled" and advances nothing.
    let settled_at = app.world().resource::<MissionHost>().last_tick();
    let report_at = app.world().resource::<MissionHostReport>().tick;
    // One cursor across the later frames: the message queue double-buffers,
    // so this is what proves the *later* frames wrote no second exit.
    let mut cursor = MessageCursor::<AppExit>::default();
    assert_eq!(
        read_exits(&app, &mut cursor),
        vec![AppExit::Success],
        "the settling frame wrote exactly the terminal's exit"
    );
    for _ in 0..4 {
        app.update();
    }
    assert!(
        read_exits(&app, &mut cursor).is_empty(),
        "a settled run writes no second AppExit"
    );
    assert_eq!(
        app.world().resource::<MissionHost>().last_tick(),
        settled_at,
        "the host tick must not move after the run settled"
    );
    assert_eq!(
        app.world().resource::<MissionHostReport>().tick,
        report_at,
        "no further composed step may stand for a settled run"
    );
    assert_eq!(
        app.world().resource::<MissionHost>().terminal(),
        Some(&terminal),
        "the terminal is the one the settling step produced"
    );

    // And the host itself refuses to step, rather than answering a settled
    // run with a half-advanced one.
    let mut host = app
        .world_mut()
        .remove_resource::<MissionHost>()
        .expect("the composed entry inserted the host");
    let refused = host.step(app.world_mut(), Duration::from_secs_f64(1.0 / 120.0));
    assert!(
        matches!(refused, Err(MissionHostStepError::Settled)),
        "a settled host must answer 'already settled', got {refused:?}"
    );
    assert_eq!(
        host.last_tick(),
        settled_at,
        "a settled host must not advance its tick when it refuses"
    );
    app.world_mut().insert_resource(host);

    teardown(&mut app);
}

/// **A control program that settles `Failure` ends the run with
/// `AppExit::Error(1)` / code 1 — the "never swallow a failure as success"
/// case.**
#[test]
fn accept_vs_m01_runtime_host_02_a_settled_failure_exits_nonzero() {
    let scratch = Scratch::new("terminal-failure");
    let stage = settling_stage(scratch.path(), Outcome::Failed);
    let mut app = build_headless(&stage).expect("the settling synthetic stage composes");

    let terminal = run_until_terminal(&mut app);

    assert_eq!(
        terminal.outcome,
        TerminalOutcome::Failure,
        "a program whose Finish asks for Failed must settle Failure"
    );
    assert_eq!(terminal.source, MissionTerminalSource::ControlProgram);
    assert_eq!(terminal.exit.outcome(), TerminalOutcome::Failure);
    assert_eq!(
        terminal.exit.code(),
        1,
        "a failure is never reported as a zero exit"
    );
    assert_eq!(
        terminal.exit.app_exit(),
        AppExit::Error(NonZero::<u8>::MIN),
        "the windowed run must end on the nonzero exit its outcome maps to"
    );
    assert_eq!(
        exits(&app),
        vec![AppExit::Error(NonZero::<u8>::MIN)],
        "the AppExit message the composed entry writes is the nonzero one"
    );
    let line = terminal.report_line.as_str();
    assert!(line.contains("outcome=failure"), "{line}");
    assert!(line.contains("source=control_program"), "{line}");
    assert!(line.contains("exit_code=1"), "{line}");

    teardown(&mut app);
}

/// **The cues the objective session still owned when the run ended are
/// counted on the terminal and are gone from the queue afterwards.**
///
/// This member drives the *other* source: the declared objective program
/// settles the mission through its own timer, and an earlier timer leaves one
/// dialogue cue in the session's queue. The cue is emitted but never drained
/// by any dialogue consumer, so the terminal must name it
/// ([`MissionTerminal::undrained_cues`]) and the terminal sequence must empty
/// the queue — a line the player will never hear is reported, never silently
/// dropped.
#[test]
fn accept_vs_m01_runtime_host_02_undrained_cues_are_counted_on_the_terminal_and_gone_afterwards() {
    let scratch = Scratch::new("terminal-cues");
    let mut stage = synthetic_stage(scratch.path());
    stage.host.objectives = cue_then_finish_objectives();
    let mut app = build_headless(&stage).expect("the cue stage composes");

    let terminal = run_until_terminal(&mut app);

    assert_eq!(
        terminal.source,
        MissionTerminalSource::Objectives,
        "the F39 objective session's own outcome is the first source the funnel reads"
    );
    assert_eq!(terminal.outcome, TerminalOutcome::Success);
    assert_eq!(
        terminal.requested_by,
        Some(FINISH_TIMER),
        "the timer whose expiry asked is the record the terminal names"
    );
    assert_eq!(
        terminal.undrained_cues, 1,
        "the radio cue the session emitted before the terminal is the one it still owned"
    );
    assert_eq!(
        app.world()
            .resource::<MissionHost>()
            .objectives()
            .pending_cues(),
        0,
        "the terminal sequence drains the cue queue"
    );
    assert_eq!(terminal.exit.code(), 0);
    assert_eq!(exits(&app), vec![AppExit::Success]);
    let line = terminal.report_line.as_str();
    assert!(line.contains("source=objectives"), "{line}");
    assert!(
        line.contains("undrained_cues=1"),
        "the report line must name the cues the player will never hear: {line}"
    );

    teardown(&mut app);
}

/// **Against the owner's installation, M01's real program runs and settles no
/// terminal — so the run neither exits nor claims an outcome.**
///
/// #1278's fact 7 measured this: M01's two `Finish` blocks (Succeeded is
/// block 23, Failed is block 39) both start dormant behind the `-1` sentinel
/// and every path to waking either needs world facts a headless run does not
/// have. A terminal here would mean a block woke without the facts that wake
/// it, so this member holds the composed entry to writing no terminal and no
/// `AppExit` for M01's real program.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_vs_m01_runtime_host_02_retail_m01_settles_no_terminal_and_writes_no_exit() {
    let root = game_dir();
    let plan: MissionLaunchPlan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    assert!(
        plan.launchable(),
        "every launch surface must be satisfied before the composition runs: {}",
        plan.gaps()
            .map(|gap| format!("{}: {}", gap.surface.label(), gap.verdict.describe()))
            .collect::<Vec<_>>()
            .join("; ")
    );
    let stage = stage_for(&root, &plan).expect("M01's stage reads through the production readers");
    drop(plan);

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    for _ in 0..8 {
        app.update();
    }

    let host = app.world().resource::<MissionHost>();
    assert!(
        host.terminal().is_none(),
        "M01 cannot reach a terminal headlessly: a terminal here would mean a block woke \
         without the facts that wake it"
    );
    assert!(
        app.world().resource::<MissionHostReport>().answer.is_ok(),
        "M01's real program still runs through the composed entry"
    );
    assert!(
        host.script().state().terminal() == TerminalState::Running,
        "M01's control program is still running at the last composed tick"
    );
    assert!(
        exits(&app).is_empty(),
        "a run that settled nothing must write no AppExit"
    );

    teardown(&mut app);
}
