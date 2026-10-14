//! F37-D-FU8 acceptance (Rally #740), producer side: the measured
//! stale-timer guard (`remaining > -1.0f`, `0x46a517` vs the constant
//! `0xbf800000` at `0x6034e8`) as `cs_sim::mission::Countdown`'s carried
//! `[+4]` reaches `MissionCountdown::preempts` — static analysis of the
//! owner-supplied decrypted executable, sha256
//! `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`;
//! never an original run and never `verified_original`. See
//! `docs/findings/2026-10-15-f37-d-fu8-mission-countdown-end-guards.md`.
//!
//! What the production code is pinned on here:
//!
//! * `accept_f37_d_fu8_a_negative_adjust_leaves_a_stale_timer_the_end_path_skips`
//!   — the one way this layering can reach the skip: a spelled
//!   `TIMER_ADJUST` whose set takes `[+4]` past -1.0 in one step. The
//!   original's poll then reports every frame and the stale skip keeps the
//!   mission alive; the recreation does the same, where before the guard
//!   it ended the mission on the first such report.
//! * `accept_f37_d_fu8_an_in_window_expiry_still_ends_the_mission` — the
//!   guard gates staleness, not expiry: the normal path is untouched, at
//!   the designed 64 Hz and at the coarsest rate a session can declare
//!   (1 tick/s, dt = 1.0 s), where the first expiry report still lands in
//!   `(-1.0, 0.0]`.
//! * `accept_f37_d_fu8_the_producer_carries_the_observed_remainder` — the
//!   input carries `[+4]` on every tick, not only the expiring one.

use cs_script::ir::{
    Action, Condition, DirectiveOperation, IR_VERSION, MissionProgram, Objective, SymbolId, Value,
};
use cs_script::runtime::{
    EventKind, MissionCountdown, MissionFacts, RULE_LIMITATIONS, SessionGeneration, TerminalState,
};
use cs_sim::mission::{CountdownDirectiveOutcome, CountdownEffect, CountdownSpec, MissionSession};
use cs_sim::time::TickRate;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

/// The session generation every fixture here runs under.
const SESSION: SessionGeneration = SessionGeneration(41);

/// The project's designed fixed rate: 64 simulation ticks per second.
fn rate() -> TickRate {
    TickRate::new(64).unwrap()
}

/// One tick's decrement at the session's fixed rate.
const DT: f64 = 1.0 / 64.0;

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("a valid content id grammar")
}

fn reward(key: &str) -> Action {
    Action::GrantReward {
        reward: cid(ContentKind::Blueprint, key),
    }
}

fn objective(id: u32, condition: Condition, actions: Vec<Action>) -> Objective {
    Objective {
        id: SymbolId(id),
        content: cid(ContentKind::Objective, &format!("fu8p-obj-{id}")),
        condition,
        actions,
        span: None,
    }
}

fn always(id: u32, actions: Vec<Action>) -> Objective {
    objective(id, Condition::Const(true), actions)
}

fn program(objectives: Vec<Objective>) -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-f37-d-fu8-producer"),
        variables: vec![],
        objectives,
    }
}

fn facts() -> MissionFacts {
    MissionFacts::default()
}

fn completions(events: &[cs_script::runtime::MissionEvent]) -> Vec<u32> {
    events
        .iter()
        .filter(|event| matches!(event.kind, EventKind::ObjectiveCompleted))
        .map(|event| event.key.source.0)
        .collect()
}

/// **The one reachable stale case: a spelled `TIMER_ADJUST` SET takes
/// `[+4]` past -1.0 in one step — the mission runs on.**
///
/// The original applies the adjust at the completion site after that
/// tick's poll; the next frame's poll reports expiry, the `Network` gate
/// and the ended flag are clear, and the stale skip (`remaining <=
/// -1.0f`) keeps the end path from running — the timer keeps falling and
/// the mission never ends by timeout. Before this guard the recreation
/// ended the mission on that first stale report instead.
#[test]
fn accept_f37_d_fu8_a_negative_adjust_leaves_a_stale_timer_the_end_path_skips() {
    let mut s = MissionSession::launch_with_countdown(
        program(vec![always(
            1,
            vec![
                reward("r-stale"),
                Action::Directive {
                    operation: DirectiveOperation::AdjustMissionTimer,
                    args: vec![Value::Str("SET".into()), Value::Float(-5.0)],
                },
            ],
        )]),
        SESSION,
        vec![cid(ContentKind::Blueprint, "r-stale")],
        CountdownSpec::new(4.0 * DT, false, false, rate()).unwrap(),
    )
    .unwrap();

    // Tick 1: the poll runs on the armed countdown (3·dt left), then the
    // completion's SET takes it to -5.0.
    let first = s.advance(&facts(), Tick(1)).unwrap();
    assert_eq!(first.terminal, TerminalState::Running);
    assert!(
        matches!(
            first.countdown.directives.as_slice(),
            [CountdownDirectiveOutcome::Applied {
                effect: CountdownEffect::Set { seconds },
                ..
            }] if *seconds == -5.0
        ),
        "{:?}",
        first.countdown.directives
    );
    assert_eq!(s.countdown().remaining_seconds(), -5.0);

    // Tick 2: the decrement and poll report the expiry — reported, not
    // hidden — and the stale skip keeps the mission alive.
    let second = s.advance(&facts(), Tick(2)).unwrap();
    assert!(
        second.countdown.input.expired,
        "the poll observes the stale timer: remaining <= 0.0"
    );
    assert_eq!(second.countdown.input.remaining_seconds, -5.0 - DT);
    assert_eq!(
        second.terminal,
        TerminalState::Running,
        "the stale-timer skip: a timer at or past -1.0f never ends the mission"
    );
    assert!(
        s.countdown().running(),
        "the end path never ran, so its `0x46c5c0` stop never ran either"
    );

    // It keeps falling, still reporting, still never ending.
    let third = s.advance(&facts(), Tick(3)).unwrap();
    assert!(third.countdown.input.expired);
    assert_eq!(third.terminal, TerminalState::Running);
    assert_eq!(s.countdown().remaining_seconds(), -5.0 - 2.0 * DT);
    assert!(
        !completions(&third.events).contains(&1),
        "the objective already completed; the stale ticks grant nothing new"
    );
}

/// **An in-window expiry still ends the mission — at the designed rate the
/// normal path is untouched.** The first report always lands in
/// `(-1.0, 0.0]`: one tick decrements by at most the tick dt, and the poll
/// reports on the first tick at or past zero.
#[test]
fn accept_f37_d_fu8_an_in_window_expiry_still_ends_the_mission() {
    let mut s = MissionSession::launch_with_countdown(
        program(vec![always(1, vec![reward("r-window")])]),
        SESSION,
        vec![cid(ContentKind::Blueprint, "r-window")],
        CountdownSpec::new(1.5 * DT, false, false, rate()).unwrap(),
    )
    .unwrap();

    let first = s.advance(&facts(), Tick(1)).unwrap();
    assert!(!first.countdown.input.expired, "0.5·dt remained");
    assert_eq!(first.terminal, TerminalState::Running);

    let second = s.advance(&facts(), Tick(2)).unwrap();
    assert!(second.countdown.input.expired);
    assert_eq!(
        second.countdown.input.remaining_seconds,
        -0.5 * DT,
        "the first expiry lands inside (-1.0, 0.0]"
    );
    assert_eq!(
        second.terminal,
        TerminalState::Failed,
        "an in-window expiry ends the mission exactly as before the guard"
    );
    assert!(!s.countdown().running(), "the end path stopped the timer");
}

/// **The coarsest rate a session can declare — 1 tick/s, dt = 1.0 s —
/// still lands the first expiry inside the window.** `TickRate` accepts
/// any positive tick count, so dt ≤ 1.0 always; a spelled 0.5 s countdown
/// reports its first expiry at -0.5 s, never at or past -1.0.
#[test]
fn accept_f37_d_fu8_the_coarsest_legal_rate_still_lands_in_the_window() {
    let slow = TickRate::new(1).unwrap();
    let mut s = MissionSession::launch_with_countdown(
        program(vec![always(1, vec![reward("r-slow")])]),
        SESSION,
        vec![cid(ContentKind::Blueprint, "r-slow")],
        CountdownSpec::new(0.5, false, false, slow).unwrap(),
    )
    .unwrap();

    let tick = s.advance(&facts(), Tick(1)).unwrap();
    assert!(tick.countdown.input.expired);
    assert_eq!(tick.countdown.input.remaining_seconds, -0.5);
    assert_eq!(
        tick.terminal,
        TerminalState::Failed,
        "even at dt = 1.0 s the first expiry report is above -1.0f"
    );
}

/// **The producer carries `[+4]` on every tick — the input is the poll's
/// whole observation, not only its boolean.** A running countdown's input
/// is no longer `MissionCountdown::NONE`: it carries the remainder the
/// end path would read.
#[test]
fn accept_f37_d_fu8_the_producer_carries_the_observed_remainder() {
    let mut s = MissionSession::launch_with_countdown(
        program(vec![always(1, vec![])]),
        SESSION,
        [],
        CountdownSpec::new(3.5 * DT, false, false, rate()).unwrap(),
    )
    .unwrap();

    let first = s.advance(&facts(), Tick(1)).unwrap();
    assert_eq!(
        first.countdown.input,
        MissionCountdown {
            expired: false,
            no_loss: false,
            network_game: false,
            remaining_seconds: 2.5 * DT,
        },
        "the produced input carries [+4] as the decrement left it"
    );
    assert_ne!(
        first.countdown.input,
        MissionCountdown::NONE,
        "a running countdown's observation is not the no-countdown one"
    );

    // The closed limitation stays closed: the guard this input feeds is
    // modelled, so the entry may not come back.
    assert!(
        !RULE_LIMITATIONS
            .iter()
            .any(|limitation| limitation.id == "f37.d.limit.mission_countdown_end_guards"),
        "the end-path guards are measured and modelled"
    );
}
