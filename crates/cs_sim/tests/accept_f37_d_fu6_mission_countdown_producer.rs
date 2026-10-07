//! F37-D-FU6 acceptance (Rally #737): the mission-countdown **producer** —
//! `cs_sim::mission::Countdown`, the recreation of the original's one mission
//! timer (the global at `0x71b468`), feeding `MissionState::step_with_countdown`
//! on every real mission tick.
//!
//! The semantics are measured (owner note on Rally #589 and the M01-LC timer
//! findings, static analysis of the owner-supplied decrypted executable,
//! sha256 `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`;
//! never an original run and never `verified_original`): `[+4]` remaining
//! seconds decremented by game dt (`0x46c5f0`), the expiry poll `0x46c640`
//! reporting `remaining <= 0.0` while running, the NOLOSS clamp and the
//! network-game skip, the mission-start arm rule `remaining > 0.0f`
//! (`0x469741`), and the timer directives `RESET_TIMER`/`END_TIMER`/
//! `TIMER_ADJUST`/`ADJUST_TIMER_WHEN_I_COMPLETE` applied once each.
//!
//! What the production code is pinned on here:
//!
//! * `accept_f37_d_fu6_running_countdown_reaches_step_with_countdown` — the
//!   acceptance sentence: a real mission tick with a running countdown feeds
//!   the produced input to `step_with_countdown`, and the mission fails on
//!   the tick it expires.
//! * `accept_f37_d_fu6_expired_countdown_preempts_the_same_ticks_objectives`
//!   — the expiry ends the mission *before* that tick's objectives: no
//!   completion, no reward, no requested outcome — a failure the host
//!   settles even though the tick carries no events.
//! * `accept_f37_d_fu6_noloss_clamps_the_countdown_and_the_mission_wins` and
//!   `accept_f37_d_fu6_network_game_skips_the_expiry_check` — the two
//!   measured exclusions, sourced from the spec's `MISSION_TIMER` NOLOSS
//!   flag and the session's own mode.
//! * `accept_f37_d_fu6_zero_seconds_arms_nothing_at_mission_start` — M01's
//!   own `MISSION_TIMER [0.0]` case: the `> 0.0f` start rule leaves it
//!   stopped forever.
//! * `accept_f37_d_fu6_timer_directives_drive_the_countdown_exactly_once`,
//!   `accept_f37_d_fu6_adjust_directives_set_and_add_without_starting` and
//!   `accept_f37_d_fu6_refusals_are_named_never_silent` — the RESET/END/
//!   ADJUST emissions apply after the step that emitted them, exactly once
//!   by execution key, and a spelling the measured parse could not produce
//!   is refused by name.
//! * `accept_f37_d_fu6_the_countdown_crosses_a_save_restore` — remaining
//!   time, running state, flags, rate and the consumed-directive set all
//!   cross the save record, checked rather than trusted.
//! * `accept_f37_d_fu6_a_refused_tick_costs_no_countdown_time` — a
//!   not-advancing tick is refused before the decrement, on every path.
//! * `accept_f37_d_fu6_producer_limitation_is_closed_and_the_residuals_recorded`
//!   — `f37.d.limit.mission_countdown_producer` is closed only together
//!   with these tests, and what stays open is named.

use cs_script::ir::{
    Action, CompareOp, Condition, DirectiveOperation, IR_VERSION, MissionProgram, Objective,
    Outcome, SymbolId, Value, Variable,
};
use cs_script::runtime::{
    EventKind, ExecutionKey, MissionCountdown, MissionEndPresentation, MissionEvent, MissionFacts,
    RULE_LIMITATIONS, SessionGeneration, TERMINAL_PRECEDENCE_RULE, TerminalState, TickError,
};
use cs_sim::mission::{
    ActorFactInput, CountdownDirectiveOutcome, CountdownEffect, CountdownFault,
    CountdownRestoreError, CountdownSpec, CountdownSpecError, MissionSession, SessionRestoreError,
};
use cs_sim::time::TickRate;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

/// The session generation every fixture here runs under.
const SESSION: SessionGeneration = SessionGeneration(23);

/// The project's designed fixed rate: 64 simulation ticks per second.
fn rate() -> TickRate {
    TickRate::new(64).unwrap()
}

/// One tick's decrement at the session's fixed rate — the recreation's
/// `game_dt`. The tests spell times in units of it, so nothing converts.
const DT: f64 = 1.0 / 64.0;

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("a valid content id grammar")
}

fn reward(key: &str) -> Action {
    Action::GrantReward {
        reward: cid(ContentKind::Blueprint, key),
    }
}

fn reward_id(key: &str) -> ContentId {
    cid(ContentKind::Blueprint, key)
}

fn objective(id: u32, condition: Condition, actions: Vec<Action>) -> Objective {
    Objective {
        id: SymbolId(id),
        content: cid(ContentKind::Objective, &format!("fu6-obj-{id}")),
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
        mission: cid(ContentKind::Mission, "synthetic-f37-d-fu6"),
        variables: vec![],
        objectives,
    }
}

/// A program whose second objective waits one tick for the first's write:
/// the completion on tick N sets `phase`, the gated objective fires on
/// tick N+1.
fn phased_program(first_actions: Vec<Action>, second_actions: Vec<Action>) -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-f37-d-fu6-phased"),
        variables: vec![Variable {
            id: SymbolId(100),
            name: "phase".into(),
            initial: Value::Int(0),
        }],
        objectives: vec![
            objective(
                1,
                Condition::Const(true),
                [
                    first_actions,
                    vec![Action::SetVariable {
                        variable: SymbolId(100),
                        value: Value::Int(1),
                    }],
                ]
                .concat(),
            ),
            objective(
                2,
                Condition::Compare {
                    variable: SymbolId(100),
                    op: CompareOp::Ge,
                    value: Value::Int(1),
                },
                second_actions,
            ),
        ],
    }
}

fn spec(seconds: f64) -> CountdownSpec {
    CountdownSpec::new(seconds, false, false, rate()).unwrap()
}

fn countdown_session(
    objectives: Vec<Objective>,
    rewards: Vec<ContentId>,
    spec: CountdownSpec,
) -> MissionSession {
    MissionSession::launch_with_countdown(program(objectives), SESSION, rewards, spec).unwrap()
}

fn facts() -> MissionFacts {
    MissionFacts::default()
}

fn completions(events: &[MissionEvent]) -> Vec<u32> {
    events
        .iter()
        .filter(|event| matches!(event.kind, EventKind::ObjectiveCompleted))
        .map(|event| event.key.source.0)
        .collect()
}

/// **A real mission tick with a running countdown reaches
/// `step_with_countdown` — and the mission fails on the tick it expires.**
///
/// The producer is observable on the production path, not asserted in a
/// harness double: every `advance` reports the `MissionCountdown` input the
/// step consumed. The countdown runs down one fixed tick dt per tick at the
/// session's declared rate — seconds arm it, seconds decrement it, seconds
/// compare it — and on the tick it reaches zero the produced input carries
/// `expired`, ending the mission before that tick's objectives.
#[test]
fn accept_f37_d_fu6_running_countdown_reaches_step_with_countdown() {
    // 3.5 ticks of countdown: ticks 1–3 report the still-running poll
    // (`expired == false`), tick 4 reports the expiry.
    let mut s = countdown_session(
        vec![always(1, vec![reward("r-once")])],
        vec![reward_id("r-once")],
        spec(3.5 * DT),
    );

    for tick in 1..=3u64 {
        let advanced = s.advance(&facts(), Tick(tick)).unwrap();
        assert_eq!(
            advanced.countdown.input,
            MissionCountdown::NONE,
            "tick {tick}: a still-running countdown reports no expiry and no flags"
        );
        assert_eq!(advanced.terminal, TerminalState::Running, "tick {tick}");
        assert!(
            (s.countdown().remaining_seconds() - (3.5 - tick as f64) * DT).abs() < 1e-9,
            "tick {tick}: the countdown decremented by exactly the session's tick dt",
        );
    }

    let expiring = s.advance(&facts(), Tick(4)).unwrap();
    assert!(
        expiring.countdown.input.expired,
        "the produced input carries the expiry the poll observed"
    );
    assert_eq!(
        completions(&expiring.events),
        Vec::<u32>::new(),
        "the expiring tick completes nothing — the input pre-empted it"
    );
    assert_eq!(expiring.terminal, TerminalState::Failed);
    assert_eq!(
        s.state().terminal(),
        TerminalState::Failed,
        "step_with_countdown recorded the failure"
    );
}

/// **Expiry fails the mission *before* the same tick's objectives — and the
/// host settles the failure even though the tick emits no events.**
///
/// One tick, one countdown already inside it: the program's always-true
/// objective would complete, grant its reward and request success on this
/// very tick — none of that runs. `TerminalState::Failed` is the recorded
/// result (`0x4194e0`: success iff WON, and an expiry sets neither flag),
/// and `HostLedger::apply` settles it.
#[test]
fn accept_f37_d_fu6_expired_countdown_preempts_the_same_ticks_objectives() {
    let mut s = countdown_session(
        vec![
            always(
                1,
                vec![reward("r-first"), Action::Finish(Outcome::Succeeded)],
            ),
            always(2, vec![reward("r-second")]),
        ],
        vec![reward_id("r-first"), reward_id("r-second")],
        // Half a tick of countdown: the first decrement already takes it
        // past zero, so tick 1 itself expires.
        spec(0.5 * DT),
    );

    let tick = s.advance(&facts(), Tick(1)).unwrap();
    assert_eq!(
        tick.countdown.input,
        MissionCountdown {
            expired: true,
            no_loss: false,
            network_game: false,
        },
        "the produced input is exactly the measured observation"
    );
    assert_eq!(tick.terminal, TerminalState::Failed);
    assert!(
        tick.events.is_empty(),
        "no objective completed, no reward granted, no outcome requested on the expiring tick"
    );
    assert_eq!(
        s.host().settled(),
        Some((Tick(1), TerminalState::Failed)),
        "the host settled the failure with no event to drive it"
    );

    // The expiry's measured end shape (F37-D-FU5): no flag set, so no
    // OBJECTIVES_* branch ran — the loss side of the mission sound and the
    // animation answer the clear WON flag, at the standard 3.0 s wait.
    assert_eq!(
        tick.presentation,
        Some(MissionEndPresentation::new(
            false,
            false,
            false,
            TerminalState::Failed
        )),
        "the expiry path selects the no-flag end presentation"
    );

    // Latched like every terminal path: a later tick changes nothing.
    let later = s.advance(&facts(), Tick(2)).unwrap();
    assert_eq!(later.terminal, TerminalState::Failed);
    assert!(later.events.is_empty());
}

/// **NOLOSS: the poll clamps the countdown at zero and reports no expiry —
/// the mission can win on that very tick.**
///
/// `[+0x14]` is set by `MISSION_TIMER`'s exact 7-byte `NOLOSS` second child
/// (`0x466cd4` → `0x46c540`); the producer carries it as `no_loss`. When the
/// countdown reaches zero the poll zeroes `[+4]` instead of expiring
/// (`0x46c640`): the input still reports the observation (`expired` and
/// `no_loss` both true) and `MissionCountdown::preempts` — the single place
/// the end-decision lives — suppresses the end, so the win the same tick
/// requests stands.
#[test]
fn accept_f37_d_fu6_noloss_clamps_the_countdown_and_the_mission_wins() {
    // 2·dt of countdown under NOLOSS: tick 2 reaches zero (and clamps),
    // which is exactly the tick the phased objective wins on.
    let mut s = MissionSession::launch_with_countdown(
        phased_program(
            vec![reward("r-noloss")],
            vec![Action::Finish(Outcome::Succeeded)],
        ),
        SESSION,
        vec![reward_id("r-noloss")],
        CountdownSpec::new(2.0 * DT, true, false, rate()).unwrap(),
    )
    .unwrap();

    let first = s.advance(&facts(), Tick(1)).unwrap();
    assert_eq!(
        first.countdown.input,
        MissionCountdown {
            expired: false,
            no_loss: true,
            network_game: false,
        },
        "still running under NOLOSS: the flag is reported every tick"
    );
    assert_eq!(s.countdown().remaining_seconds(), DT);

    let second = s.advance(&facts(), Tick(2)).unwrap();
    assert_eq!(
        second.countdown.input,
        MissionCountdown {
            expired: true,
            no_loss: true,
            network_game: false,
        },
        "the poll observed zero under NOLOSS — reported, not hidden"
    );
    assert_eq!(
        s.countdown().remaining_seconds(),
        0.0,
        "the poll's side effect: [+4] clamped to zero"
    );
    assert_eq!(
        second.terminal,
        TerminalState::Succeeded,
        "the exclusion held and the same tick's success is recorded"
    );

    // The clamp holds for every later tick: decrement, hit zero, clamp
    // again — never an end.
    let third = s.advance(&facts(), Tick(3)).unwrap();
    assert_eq!(third.terminal, TerminalState::Succeeded);
    assert_eq!(s.countdown().remaining_seconds(), 0.0);
}

/// **Network game: the expiry check is skipped — the countdown decrements
/// past zero and the mission runs on.** No clamp: the original skips the
/// whole check, so `[+4]` keeps falling — the distinguishing mark from
/// NOLOSS, which zeroes it.
#[test]
fn accept_f37_d_fu6_network_game_skips_the_expiry_check() {
    let mut s = countdown_session(
        vec![always(1, vec![reward("r-net")])],
        vec![reward_id("r-net")],
        CountdownSpec::new(1.5 * DT, false, true, rate()).unwrap(),
    );

    let first = s.advance(&facts(), Tick(1)).unwrap();
    assert_eq!(first.terminal, TerminalState::Running);
    assert!(first.countdown.input.network_game);

    let second = s.advance(&facts(), Tick(2)).unwrap();
    assert_eq!(
        second.countdown.input,
        MissionCountdown {
            expired: true,
            no_loss: false,
            network_game: true,
        },
        "below zero in a network game: the observation is reported, the check is skipped"
    );
    assert_eq!(second.terminal, TerminalState::Running);
    assert!(
        s.countdown().remaining_seconds() < 0.0,
        "no clamp: [+4] keeps falling while the check stays skipped"
    );

    let third = s.advance(&facts(), Tick(3)).unwrap();
    assert_eq!(third.terminal, TerminalState::Running);
    assert_eq!(s.countdown().remaining_seconds(), -1.5 * DT);
}

/// **`MISSION_TIMER [0.0]` arms nothing — the measured start rule.**
///
/// Mission start runs the countdown iff the spelled value is `> 0.0f`
/// (`0x469741`): zero and negative values leave it stopped — which is how
/// M01's own `MISSION_TIMER [0.0]` never runs in the original. A stopped
/// countdown reports `MissionCountdown::NONE` forever: no decrement, no
/// expiry, the mission ends only on its program's own request.
#[test]
fn accept_f37_d_fu6_zero_seconds_arms_nothing_at_mission_start() {
    for seconds in [0.0, -3.0] {
        let mut s = countdown_session(
            vec![always(
                1,
                vec![reward("r-m01"), Action::Finish(Outcome::Succeeded)],
            )],
            vec![reward_id("r-m01")],
            spec(seconds),
        );
        assert!(!s.countdown().running(), "{seconds}: never started");
        let tick = s.advance(&facts(), Tick(1)).unwrap();
        assert_eq!(
            tick.countdown.input,
            MissionCountdown::NONE,
            "{seconds}: a stopped countdown reports nothing, forever"
        );
        assert_eq!(
            s.countdown().remaining_seconds(),
            seconds,
            "{seconds}: never running, never decremented"
        );
        assert_eq!(tick.terminal, TerminalState::Succeeded);
    }
    // A non-finite spec is refused by name — the record's parse takes a
    // real or an int, which is always finite.
    assert!(matches!(
        CountdownSpec::new(f64::NAN, false, false, rate()),
        Err(CountdownSpecError::NonFiniteRemaining { .. })
    ));
    assert!(matches!(
        CountdownSpec::new(f64::INFINITY, false, false, rate()),
        Err(CountdownSpecError::NonFiniteRemaining { .. })
    ));
}

/// **The timer directives drive the countdown — after the step that
/// emitted them, exactly once each.**
///
/// `RESET_TIMER` arms on the tick after its emission (the wake effect runs
/// after that tick's poll — the measured order inside `0x46a490`);
/// `END_TIMER` stops. Each is consumed by its execution key once — the
/// emission stays in the evaluator's log but is never re-applied.
#[test]
fn accept_f37_d_fu6_timer_directives_drive_the_countdown_exactly_once() {
    // Objective 1 completes on tick 1 and emits RESET_TIMER [4·dt] plus the
    // phase write; objective 2 completes on tick 2 and emits END_TIMER —
    // stopping the countdown it armed two ticks before it could expire.
    let mut s = MissionSession::launch_with_countdown(
        phased_program(
            vec![
                reward("r-phase1"),
                Action::Directive {
                    operation: DirectiveOperation::ResetMissionTimer,
                    args: vec![Value::Float(4.0 * DT)],
                },
            ],
            vec![
                reward("r-phase2"),
                Action::Directive {
                    operation: DirectiveOperation::EndMissionTimer,
                    args: vec![],
                },
                Action::Finish(Outcome::Succeeded),
            ],
        ),
        SESSION,
        vec![reward_id("r-phase1"), reward_id("r-phase2")],
        // Launched unarmed: the session declared its rate but the record
        // spelled no MISSION_TIMER — a countdown a directive alone arms.
        spec(0.0),
    )
    .unwrap();
    assert!(!s.countdown().running(), "nothing armed at mission start");

    let first = s.advance(&facts(), Tick(1)).unwrap();
    assert_eq!(first.terminal, TerminalState::Running);
    assert!(
        s.countdown().running(),
        "the reset directive armed the countdown"
    );
    assert_eq!(s.countdown().remaining_seconds(), 4.0 * DT);
    assert!(
        matches!(
            first.countdown.directives.as_slice(),
            [CountdownDirectiveOutcome::Applied {
                effect: CountdownEffect::Armed { seconds },
                ..
            }] if *seconds == 4.0 * DT
        ),
        "the tick reports the applied reset: {:?}",
        first.countdown.directives
    );

    let second = s.advance(&facts(), Tick(2)).unwrap();
    // Tick 2's own poll ran on the armed countdown — 3·dt left — then
    // END_TIMER stopped it; the success the same objective requested stands.
    assert_eq!(second.terminal, TerminalState::Succeeded);
    assert!(!s.countdown().running(), "END_TIMER stopped the countdown");
    assert!(
        matches!(
            second.countdown.directives.as_slice(),
            [CountdownDirectiveOutcome::Applied {
                effect: CountdownEffect::Stopped,
                ..
            }]
        ),
        "the tick reports the applied stop: {:?}",
        second.countdown.directives
    );

    // Exactly-once: both emissions stay in the evaluator's log, the
    // countdown never re-applies them, and the stopped timer never ticks.
    let third = s.advance(&facts(), Tick(3)).unwrap();
    assert!(
        third.countdown.directives.is_empty(),
        "no directive is applied twice: {:?}",
        third.countdown.directives
    );
    assert_eq!(s.countdown().remaining_seconds(), 3.0 * DT);
    assert!(!s.countdown().running());
}

/// **`TIMER_ADJUST` adds and `ADJUST_TIMER_WHEN_I_COMPLETE`'s `SET`
/// replaces — neither starts a stopped timer.** The original's set/adjust
/// calls touch no flag, so an adjust alone never arms the countdown.
#[test]
fn accept_f37_d_fu6_adjust_directives_set_and_add_without_starting() {
    // Tick 1: SET [10·dt] on a still-stopped countdown — remaining replaced,
    // running untouched. Tick 2: TIMER_ADJUST [5] — five seconds added,
    // still stopped. The mission then finishes; the countdown never ran.
    let mut s = MissionSession::launch_with_countdown(
        phased_program(
            vec![Action::Directive {
                operation: DirectiveOperation::AdjustMissionTimer,
                args: vec![Value::Str("SET".into()), Value::Float(10.0 * DT)],
            }],
            vec![
                Action::Directive {
                    operation: DirectiveOperation::AdjustMissionTimer,
                    args: vec![Value::Int(5)],
                },
                Action::Finish(Outcome::Succeeded),
            ],
        ),
        SESSION,
        [],
        spec(0.0),
    )
    .unwrap();

    let first = s.advance(&facts(), Tick(1)).unwrap();
    assert!(
        matches!(
            first.countdown.directives.as_slice(),
            [CountdownDirectiveOutcome::Applied {
                effect: CountdownEffect::Set { seconds },
                ..
            }] if *seconds == 10.0 * DT
        ),
        "{:?}",
        first.countdown.directives
    );
    assert_eq!(s.countdown().remaining_seconds(), 10.0 * DT);
    assert!(
        !s.countdown().running(),
        "a SET adjust is not a start — the original's adjust call touches no flag"
    );

    let second = s.advance(&facts(), Tick(2)).unwrap();
    assert!(
        matches!(
            second.countdown.directives.as_slice(),
            [CountdownDirectiveOutcome::Applied {
                effect: CountdownEffect::Adjusted { seconds },
                ..
            }] if *seconds == 5.0
        ),
        "{:?}",
        second.countdown.directives
    );
    assert_eq!(s.countdown().remaining_seconds(), 10.0 * DT + 5.0);
    assert!(!s.countdown().running());
    assert_eq!(second.terminal, TerminalState::Succeeded);
}

/// **A spelling the measured parse could not produce — or a session with
/// no honest dt — is refused by name, never a silent no-op.**
#[test]
fn accept_f37_d_fu6_refusals_are_named_never_silent() {
    // RESET_TIMER [-1]: the wake gate (`+0x5e0 >= 0.0f`) declines it.
    let mut negative = MissionSession::launch_with_countdown(
        program(vec![always(
            1,
            vec![Action::Directive {
                operation: DirectiveOperation::ResetMissionTimer,
                args: vec![Value::Float(-1.0)],
            }],
        )]),
        SESSION,
        [],
        spec(0.0),
    )
    .unwrap();
    let tick = negative.advance(&facts(), Tick(1)).unwrap();
    assert!(
        matches!(
            tick.countdown.directives.as_slice(),
            [CountdownDirectiveOutcome::Refused {
                fault: CountdownFault::NegativeReset { seconds },
                ..
            }] if *seconds == -1.0
        ),
        "{:?}",
        tick.countdown.directives
    );
    assert!(!negative.countdown().running());

    // ADJUST_TIMER_WHEN_I_COMPLETE ["FORGET", x]: the parse writes no mode —
    // the site does nothing, reported rather than guessed.
    let mut unknown = MissionSession::launch_with_countdown(
        program(vec![always(
            1,
            vec![Action::Directive {
                operation: DirectiveOperation::AdjustMissionTimer,
                args: vec![Value::Str("FORGET".into()), Value::Float(1.0)],
            }],
        )]),
        SESSION,
        [],
        spec(1.0),
    )
    .unwrap();
    let tick = unknown.advance(&facts(), Tick(1)).unwrap();
    assert!(
        matches!(
            tick.countdown.directives.as_slice(),
            [CountdownDirectiveOutcome::Refused {
                fault: CountdownFault::UnknownAdjustMode,
                ..
            }]
        ),
        "{:?}",
        tick.countdown.directives
    );

    // RESET_TIMER on a session launched without a spec: the plain `launch`
    // path declared no rate, so there is no honest dt to arm with.
    let mut no_rate = MissionSession::launch(
        program(vec![always(
            1,
            vec![Action::Directive {
                operation: DirectiveOperation::ResetMissionTimer,
                args: vec![Value::Float(1.0)],
            }],
        )]),
        SESSION,
        [],
    )
    .unwrap();
    let tick = no_rate.advance(&facts(), Tick(1)).unwrap();
    assert!(
        matches!(
            tick.countdown.directives.as_slice(),
            [CountdownDirectiveOutcome::Refused {
                fault: CountdownFault::NoDeclaredTickRate,
                ..
            }]
        ),
        "{:?}",
        tick.countdown.directives
    );
    assert!(!no_rate.countdown().running());

    // A non-timer directive is not the countdown's: it is not consumed and
    // not reported as one.
    let mut other = MissionSession::launch_with_countdown(
        program(vec![always(
            1,
            vec![Action::Directive {
                operation: DirectiveOperation::PresentationIdentity,
                args: vec![Value::Int(2), Value::Int(0)],
            }],
        )]),
        SESSION,
        [],
        spec(0.0),
    )
    .unwrap();
    let tick = other.advance(&facts(), Tick(1)).unwrap();
    assert!(
        tick.countdown.directives.is_empty(),
        "a directive that is not a timer directive is none of the countdown's: {:?}",
        tick.countdown.directives
    );
}

/// **The countdown crosses a save/restore: remaining time, running state
/// and the consumed-directive set.** A restored session keeps counting
/// from exactly where the save left it — it does not restart, and a
/// directive already consumed is not re-applied.
#[test]
fn accept_f37_d_fu6_the_countdown_crosses_a_save_restore() {
    let build = || {
        phased_program(
            vec![Action::Directive {
                operation: DirectiveOperation::ResetMissionTimer,
                args: vec![Value::Float(8.0 * DT)],
            }],
            vec![],
        )
    };
    // Launch unarmed; the reset on tick 1 arms 8·dt, so after two ticks the
    // countdown has 7·dt left and the save lands mid-flight.
    let mut s = MissionSession::launch_with_countdown(build(), SESSION, [], spec(0.0)).unwrap();
    s.advance(&facts(), Tick(1)).unwrap();
    s.advance(&facts(), Tick(2)).unwrap();
    assert_eq!(s.countdown().remaining_seconds(), 7.0 * DT);

    let snapshot = s.snapshot();
    let mut restored = MissionSession::restore(build(), snapshot.clone()).unwrap();
    assert_eq!(
        restored.countdown().remaining_seconds(),
        7.0 * DT,
        "the remaining time crosses the save"
    );
    assert!(restored.countdown().running());

    // The consumed reset is not re-applied: if it were, remaining would
    // jump back to 8·dt — instead the countdown keeps falling and expires
    // on the same tick the live run would fail.
    let third = restored.advance(&facts(), Tick(3)).unwrap();
    assert!(
        third.countdown.directives.is_empty(),
        "an already-consumed directive is never re-applied: {:?}",
        third.countdown.directives
    );
    for tick in 4..=8u64 {
        assert_eq!(
            restored.advance(&facts(), Tick(tick)).unwrap().terminal,
            TerminalState::Running,
            "tick {tick}"
        );
    }
    let ninth = restored.advance(&facts(), Tick(9)).unwrap();
    assert!(ninth.countdown.input.expired);
    assert_eq!(ninth.terminal, TerminalState::Failed);

    // And a record no live session could write is refused, not trusted.
    let mut foreign = snapshot;
    foreign.countdown.consumed.push(ExecutionKey {
        session: SessionGeneration(99),
        source: SymbolId(1),
        sequence: 1,
    });
    assert!(matches!(
        MissionSession::restore(build(), foreign),
        Err(SessionRestoreError::Countdown(
            CountdownRestoreError::ForeignConsumed { .. }
        ))
    ));
}

/// **A tick that does not advance costs the countdown no time.** The
/// refusal precedes the decrement — replaying a tick consumes nothing —
/// and the evaluation-only paths drive the same producer.
#[test]
fn accept_f37_d_fu6_a_refused_tick_costs_no_countdown_time() {
    let mut s = countdown_session(vec![always(1, vec![])], vec![], spec(4.0 * DT));
    s.advance(&facts(), Tick(1)).unwrap();
    assert_eq!(s.countdown().remaining_seconds(), 3.0 * DT);

    assert_eq!(
        s.advance(&facts(), Tick(1)),
        Err(TickError::NotAdvancing {
            last: Tick(1),
            given: Tick(1),
        }),
        "the same tick again is refused"
    );
    assert_eq!(
        s.countdown().remaining_seconds(),
        3.0 * DT,
        "and it consumed no countdown time"
    );

    // `step` — the evaluation-only path — drives the same producer.
    let stepped = s.step(&facts(), Tick(2)).unwrap();
    assert_eq!(stepped.terminal, TerminalState::Running);
    assert_eq!(s.countdown().remaining_seconds(), 2.0 * DT);

    // `step_observed` does too, and reports the input it produced.
    let observed = s
        .step_observed(
            &ActorFactInput {
                registered: &[],
                lifecycles: &[],
            },
            Tick(3),
        )
        .unwrap();
    assert!(!observed.countdown.input.expired);
    assert_eq!(s.countdown().remaining_seconds(), DT);
}

/// **The producer limitation is closed — only together with these tests —
/// and the residuals are recorded.** `f37.d.limit.mission_countdown_producer`
/// may not come back while `cs_sim::mission::Countdown` feeds
/// `step_with_countdown`; what remains open (the tick dt, the end-path
/// guards, the spec sourcing) names its affected content and resolving
/// task, and the measured fact they gate points at all three.
#[test]
fn accept_f37_d_fu6_producer_limitation_is_closed_and_the_residuals_recorded() {
    assert!(
        !RULE_LIMITATIONS
            .iter()
            .any(|limitation| limitation.id == "f37.d.limit.mission_countdown_producer"),
        "the producer exists: its old entry must not come back"
    );
    for (id, needle) in [
        ("f37.d.limit.mission_countdown_tick_dt", "MISSION_TIMER"),
        ("f37.d.limit.mission_countdown_end_guards", "countdown"),
        (
            "f37.d.limit.mission_countdown_spec_sourcing",
            "MISSION_TIMER",
        ),
    ] {
        let open = RULE_LIMITATIONS
            .iter()
            .find(|limitation| limitation.id == id)
            .unwrap_or_else(|| panic!("the residual {id} is recorded"));
        assert!(
            open.affected_content.contains(needle),
            "{id} must name its affected content: {}",
            open.affected_content
        );
        assert!(
            !open.resolving_task.is_empty(),
            "{id} must name what resolves it"
        );
    }
    let fact = TERMINAL_PRECEDENCE_RULE
        .facts
        .iter()
        .find(|fact| fact.id == "f37.rule.terminal_precedence.countdown_preempts")
        .expect("the countdown fact is recorded");
    assert_eq!(
        fact.limitations,
        [
            "f37.d.limit.mission_countdown_tick_dt",
            "f37.d.limit.mission_countdown_end_guards",
            "f37.d.limit.mission_countdown_spec_sourcing",
        ],
        "the fact gates exactly the producer's residuals"
    );
}
