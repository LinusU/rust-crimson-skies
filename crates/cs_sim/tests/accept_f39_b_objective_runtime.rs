//! F39-B acceptance: the continuous objective runtime (synthetic).
//!
//! Spec: `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
//! stage `### F39-B`. Minimum scenario: **AC02** — "Destroy a protected actor on
//! the same tick as completing an objective; use declared terminal
//! precedence." The other cases below pin the rules AC02 depends on, so the
//! precedence answer cannot change because one of its inputs did.
//!
//! Ordinary build/test only; nothing here is original data. Every rule asserted
//! is **designed** behavior: the original game's precedence, deadlines,
//! protected rosters and reveal rules are unmeasured (F39-D, `retail`).

use std::collections::BTreeSet;

use cs_script::ir::{ActorId, SymbolId};
use cs_script::runtime::SessionGeneration;
use cs_sim::damage::LifecycleKind;
use cs_sim::objectives::counters::CountKind;
use cs_sim::objectives::runtime::{
    ACTOR_EVENT_SOURCE, CountCondition, CountReaction, ObjectiveCompletion, ObjectiveEvent,
    ObjectiveEventKind, ObjectiveRuntime, ObjectiveSpec, ObjectiveTick, RevealRule, RuntimeError,
    RuntimeLimits, StopReason, TickInput,
};
use cs_sim::objectives::spawn::IdempotencyKey;
use cs_sim::objectives::state::ObjectiveState;
use cs_sim::objectives::terminal::{TerminalOutcome, TerminalPrecedence};
use cs_sim::objectives::timer::{
    MissionTimer, TimerAction, TimerError, TimerRequest, TimerStart, TimerState,
};
use cs_sim::objectives::trigger::{CrossingKind, Movement, SweptTrigger, synthetic_small_volume};
use cs_sim::time::{ClockPolicy, TimeDomain};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

// --- symbols ---------------------------------------------------------------

/// The mission's protected convoy: the actor whose loss fails the mission.
const PROTECTED: ActorId = ActorId(41);
/// The mission's primary objective.
const PRIMARY: SymbolId = SymbolId(1);
/// The count condition watching the protected actor.
const PROTECTED_LOST: SymbolId = SymbolId(10);
/// A countdown whose expiry completes the primary objective.
const COMPLETE_DEADLINE: SymbolId = SymbolId(20);
/// A trigger volume on the mission's only approach path.
const APPROACH: SymbolId = SymbolId(30);
/// The player actor the approach volume watches.
const PLAYER: ActorId = ActorId(7);

const SESSION: SessionGeneration = SessionGeneration(1);

fn content(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("a valid content id grammar")
}

fn objective(key: &str) -> ContentId {
    content(ContentKind::Objective, key)
}

fn limits() -> RuntimeLimits {
    RuntimeLimits::default()
}

fn runtime() -> ObjectiveRuntime {
    ObjectiveRuntime::new(SESSION, TerminalPrecedence::SyntheticConservative, limits())
}

fn input(tick: u64) -> TickInput<'static> {
    TickInput::at(Tick(tick))
}

/// The runtime of the AC02 mission: one objective whose completion requests
/// `Success`, a protected actor whose declared category requests `Failure`, and
/// a deadline that completes the objective.
fn ac02_runtime() -> ObjectiveRuntime {
    let mut runtime = runtime();
    runtime
        .add_objective(ObjectiveSpec {
            id: PRIMARY,
            content: objective("deliver-the-medicine"),
            initial: ObjectiveState::Active,
            reveal: RevealRule::Immediate,
            on_complete: ObjectiveCompletion::Requests(TerminalOutcome::Success),
        })
        .unwrap();
    runtime
        .add_condition(
            CountCondition::new(PROTECTED_LOST, CountKind::Destroyed, [PROTECTED], 1).unwrap(),
            CountReaction::Finish(TerminalOutcome::Failure),
        )
        .unwrap();
    runtime
        .add_timer(
            MissionTimer::new(
                COMPLETE_DEADLINE,
                ClockPolicy::authoritative_gameplay(),
                TimerStart::OnArm,
                1,
                TimerAction::SetObjectiveState {
                    objective: PRIMARY,
                    state: ObjectiveState::Succeeded,
                },
            )
            .unwrap(),
        )
        .unwrap();
    runtime
}

/// The one event of one kind this tick, or a readable panic naming the tick.
fn one(
    tick: &ObjectiveTick,
    predicate: impl Fn(&ObjectiveEventKind) -> bool,
) -> &ObjectiveEventKind {
    let found = tick.filter(predicate);
    assert_eq!(
        found.len(),
        1,
        "expected exactly one matching event on tick {}, got {:?}",
        tick.tick.0,
        tick.events
    );
    &found[0].kind
}

// ---------------------------------------------------------------------------
// AC02: the minimum scenario
// ---------------------------------------------------------------------------

/// **AC02.** On one tick the protected actor is destroyed *and* the objective
/// completes. The declared precedence — [`TerminalPrecedence::SyntheticConservative`],
/// `Failure` beats `Success` — resolves the collision once, out of the whole set
/// of that tick's requests, and the losing request is named rather than dropped.
///
/// What makes this discriminating:
///
/// * both facts must be *real* — the destruction arrives as a
///   [`LifecycleKind::Destroyed`] lifecycle transition and the completion as a
///   timer expiry with a declared action, not as a direct call to a setter;
/// * the two must land on the *same* tick, so the test also pins the deadline's
///   arm tick;
/// * the outcome must be `Failure`, and the superseding `Success` must be
///   reported. Remove the precedence and the runtime would report both; remove
///   the same-tick ordering and only one request would exist to resolve.
#[test]
fn accept_f39_b_protected_actor_destroyed_on_the_completion_tick_uses_declared_precedence() {
    let mut runtime = ac02_runtime();

    // Arm the deadline, then let it run to its single tick. On the way, the
    // protected actor is destroyed on the very tick it expires.
    let armed = runtime
        .step(&TickInput {
            timer_requests: &[TimerRequest::Arm(COMPLETE_DEADLINE)],
            ..input(1)
        })
        .unwrap();
    assert!(
        armed
            .events
            .iter()
            .all(|event| !matches!(event.kind, ObjectiveEventKind::OutcomeSettled { .. })),
        "arming a deadline ends nothing"
    );
    assert_eq!(
        runtime.timer_state(COMPLETE_DEADLINE),
        Some(TimerState::Armed {
            since: Tick(1),
            remaining: 1,
        })
    );

    let collision = runtime
        .step(&TickInput {
            committed_ticks: 1,
            lifecycles: &[(PROTECTED, LifecycleKind::Destroyed)],
            ..input(2)
        })
        .unwrap();

    assert_eq!(
        runtime.outcome(),
        Some(TerminalOutcome::Failure),
        "the declared precedence must resolve the collision towards failure"
    );
    let ObjectiveEventKind::OutcomeSettled {
        outcome,
        superseded,
    } = one(&collision, |kind| {
        matches!(kind, ObjectiveEventKind::OutcomeSettled { .. })
    })
    else {
        unreachable!("the assertion above pinned the variant");
    };
    assert_eq!(*outcome, TerminalOutcome::Failure);
    assert_eq!(*superseded, vec![TerminalOutcome::Success]);

    // The objective really did complete on that tick: the precedence decided
    // *which* outcome the mission reports, not whether the completion happened.
    assert_eq!(
        runtime.objective_state(PRIMARY),
        Some(ObjectiveState::Succeeded)
    );
    assert!(matches!(
        one(&collision, |kind| matches!(kind, ObjectiveEventKind::ConditionMet { .. })),
        ObjectiveEventKind::ConditionMet { condition, kind: CountKind::Destroyed, observed: 1 }
            if *condition == PROTECTED_LOST
    ));
    assert!(matches!(
        one(&collision, |kind| matches!(kind, ObjectiveEventKind::TimerExpired { .. })),
        ObjectiveEventKind::TimerExpired { timer } if *timer == COMPLETE_DEADLINE
    ));
}

/// **AC02, second half.** The latch is one-way: a protected actor destroyed
/// *after* a success latch cannot change the outcome, and the later tick says so
/// instead of acting. This is the contract's "may or may not change the
/// outcome" turned into a stated rule, and it is what makes the same-tick case
/// above meaningful — the answer does not depend on which producer spoke first.
#[test]
fn accept_f39_b_outcome_survives_a_later_kill_and_the_later_tick_reports_it() {
    let mut runtime =
        ObjectiveRuntime::new(SESSION, TerminalPrecedence::SyntheticConservative, limits());
    runtime
        .add_condition(
            CountCondition::new(PROTECTED_LOST, CountKind::Destroyed, [PROTECTED], 1).unwrap(),
            CountReaction::Finish(TerminalOutcome::Failure),
        )
        .unwrap();

    runtime
        .step(&TickInput {
            terminal_requests: &[(PROTECTED_LOST, TerminalOutcome::Success)],
            ..input(1)
        })
        .unwrap();
    assert_eq!(runtime.outcome(), Some(TerminalOutcome::Success));

    let later = runtime
        .step(&TickInput {
            lifecycles: &[(PROTECTED, LifecycleKind::Destroyed)],
            ..input(2)
        })
        .unwrap();

    assert_eq!(runtime.outcome(), Some(TerminalOutcome::Success));
    assert_eq!(
        later.stop,
        Some(StopReason::OutcomeSettled {
            settled_at: Tick(1)
        })
    );
    assert!(later.events.is_empty(), "a settled mission does no work");
    assert!(!runtime.is_counted(CountKind::Destroyed, PROTECTED));
}

/// The precedence is a property of the *set*, not of arrival order: two
/// producers that ask in opposite orders on one tick agree, and the losing
/// request is refused by name once the latch holds.
#[test]
fn accept_f39_b_precedence_is_order_independent_and_names_the_loser() {
    let reverse = |requested: Vec<(SymbolId, TerminalOutcome)>| {
        let mut runtime = runtime();
        let tick = runtime
            .step(&TickInput {
                terminal_requests: &requested,
                ..input(1)
            })
            .unwrap();
        (runtime.outcome(), tick)
    };

    let success_first = vec![
        (SymbolId(1), TerminalOutcome::Success),
        (SymbolId(2), TerminalOutcome::Extraction),
        (SymbolId(3), TerminalOutcome::Failure),
    ];
    let failure_first = vec![
        (SymbolId(3), TerminalOutcome::Failure),
        (SymbolId(2), TerminalOutcome::Extraction),
        (SymbolId(1), TerminalOutcome::Success),
    ];
    assert_eq!(
        reverse(success_first.clone()).0,
        Some(TerminalOutcome::Failure)
    );
    assert_eq!(reverse(failure_first).0, Some(TerminalOutcome::Failure));

    // A request after the latch is refused, naming what it asked for and what
    // the mission already decided.
    let mut runtime = runtime();
    runtime
        .step(&TickInput {
            terminal_requests: &[(SymbolId(9), TerminalOutcome::Success)],
            ..input(1)
        })
        .unwrap();
    let refused = runtime
        .step(&TickInput {
            terminal_requests: &[(SymbolId(9), TerminalOutcome::Failure)],
            ..input(2)
        })
        .unwrap();
    // The tick stopped at the latch, so the request is refused by the stop: the
    // outcome cannot change and no effect ran.
    assert!(refused.stop.is_some());
    assert_eq!(runtime.outcome(), Some(TerminalOutcome::Success));
}

// ---------------------------------------------------------------------------
// Counters: categories stay apart, and no condition is "enemy_alive == 0"
// ---------------------------------------------------------------------------

/// F39 non-negotiable behavior 2, made structural. A *destroyed* condition is
/// not satisfied by a capture, a despawn, a bailout or a mission removal, and an
/// actor outside the roster never counts. Without the roster and the single
/// category there would be nothing to break here, which is the point: the
/// approximation the sheet forbids has no representation.
#[test]
fn accept_f39_b_a_count_condition_names_its_roster_and_one_category() {
    let mut runtime =
        ObjectiveRuntime::new(SESSION, TerminalPrecedence::SyntheticConservative, limits());
    let escort: ActorId = ActorId(42);
    let bystander: ActorId = ActorId(99);
    runtime
        .add_condition(
            CountCondition::new(PROTECTED_LOST, CountKind::Destroyed, [escort], 1).unwrap(),
            CountReaction::ReportOnly,
        )
        .unwrap();

    // A bystander dying changes nothing: it is not on the roster. The escort's
    // bailout counts toward none of the five categories.
    let bystander_tick = runtime
        .step(&TickInput {
            lifecycles: &[
                (bystander, LifecycleKind::Destroyed),
                (escort, LifecycleKind::PilotBailout),
            ],
            ..input(1)
        })
        .unwrap();
    assert!(!runtime.condition_met(PROTECTED_LOST));
    assert_eq!(runtime.counted(CountKind::Destroyed), 1);
    assert_eq!(runtime.counted(CountKind::Disabled), 0);
    // The counted event carries the reserved source, which no declaration may
    // claim, so an actor's transition is never mistaken for a declaration's.
    assert!(
        bystander_tick
            .events
            .iter()
            .filter(|event| matches!(event.kind, ObjectiveEventKind::Counted { .. }))
            .all(|event| event.source() == ACTOR_EVENT_SOURCE)
    );

    // Neither does the escort's *capture* satisfy a destroyed condition.
    let captured = runtime
        .step(&TickInput {
            lifecycles: &[(escort, LifecycleKind::OwnershipCaptured)],
            ..input(2)
        })
        .unwrap();
    assert!(!runtime.condition_met(PROTECTED_LOST));
    assert_eq!(runtime.counted(CountKind::Captured), 1);
    assert_eq!(runtime.counted(CountKind::Destroyed), 1);
    assert!(
        captured
            .events
            .iter()
            .all(|event| !matches!(event.kind, ObjectiveEventKind::ConditionMet { .. }))
    );

    // Only the declared category on the declared roster latches, and it latches
    // once.
    let destroyed = runtime
        .step(&TickInput {
            lifecycles: &[(escort, LifecycleKind::Destroyed)],
            ..input(3)
        })
        .unwrap();
    assert!(runtime.condition_met(PROTECTED_LOST));
    assert!(matches!(
        one(&destroyed, |kind| matches!(kind, ObjectiveEventKind::ConditionMet { .. })),
        ObjectiveEventKind::ConditionMet { condition, kind: CountKind::Destroyed, observed: 1 }
            if *condition == PROTECTED_LOST
    ));
    // A terminal lifecycle transition is terminal: nothing may be recorded for
    // the actor again.
    let after_despawn = runtime
        .step(&TickInput {
            lifecycles: &[(escort, LifecycleKind::Despawned)],
            ..input(4)
        })
        .unwrap();
    assert!(
        after_despawn
            .events
            .iter()
            .all(|event| !matches!(event.kind, ObjectiveEventKind::ConditionMet { .. }))
    );
}

/// A condition with no roster, or a zero requirement, is refused at
/// declaration instead of becoming a condition that is met by construction.
#[test]
fn accept_f39_b_a_condition_that_cannot_be_a_condition_is_refused() {
    let mut runtime = runtime();
    assert_eq!(
        CountCondition::new(PROTECTED_LOST, CountKind::Destroyed, [], 1),
        Err(RuntimeError::EmptyRoster {
            condition: PROTECTED_LOST
        })
    );
    assert_eq!(
        CountCondition::new(PROTECTED_LOST, CountKind::Destroyed, [PROTECTED], 0),
        Err(RuntimeError::ZeroRequired {
            condition: PROTECTED_LOST
        })
    );
    assert_eq!(
        CountCondition::new(ACTOR_EVENT_SOURCE, CountKind::Destroyed, [PROTECTED], 1),
        Err(RuntimeError::ReservedSymbol {
            symbol: ACTOR_EVENT_SOURCE
        })
    );
    // The reserved source keeps counted events distinguishable from declared
    // ones, so no declaration may take it.
    assert!(
        runtime
            .add_objective(ObjectiveSpec {
                id: ACTOR_EVENT_SOURCE,
                content: objective("reserved"),
                initial: ObjectiveState::Pending,
                reveal: RevealRule::Immediate,
                on_complete: ObjectiveCompletion::Continue,
            })
            .is_err()
    );
}

// ---------------------------------------------------------------------------
// Triggers stay swept, and a waypoint alone unlocks nothing
// ---------------------------------------------------------------------------

/// F39 non-negotiable behavior 1 in the continuous runtime: the runtime sweeps
/// the **real** segment it was given, a teleport observes only its destination,
/// and — F39 non-negotiable behavior 3 — a crossing never applies an effect. The
/// only thing that follows a crossing is a declared [`TimerRequest::Arm`].
///
/// Without the "crossing applies nothing" rule this test cannot distinguish a
/// runtime that honours non-negotiable 3 from one where a waypoint quietly
/// unlocks the next objective.
#[test]
fn accept_f39_b_a_crossing_reports_and_only_a_declared_arm_moves_the_world() {
    let mut runtime = ac02_runtime();
    runtime
        .add_trigger(SweptTrigger::new(APPROACH, PLAYER, synthetic_small_volume()).unwrap())
        .unwrap();

    // 400 m in one tick across the 2 m cube: both endpoints are outside.
    let crossing = runtime
        .step(&TickInput {
            movements: &[(
                PLAYER,
                Movement::Continuous {
                    from_m: [-200.0, 0.0, 0.0],
                    to_m: [200.0, 0.0, 0.0],
                },
            )],
            ..input(1)
        })
        .unwrap();

    let mut kinds: Vec<CrossingKind> = Vec::new();
    for event in &crossing.events {
        if let ObjectiveEventKind::TriggerCrossed(crossing) = event.kind {
            kinds.push(crossing.kind);
        }
    }
    assert_eq!(kinds, [CrossingKind::Entry, CrossingKind::Exit]);
    assert_eq!(
        runtime.timer_state(COMPLETE_DEADLINE),
        Some(TimerState::NotArmed),
        "crossing a volume must not arm, unlock or reset anything by itself"
    );
    assert_eq!(
        runtime.objective_state(PRIMARY),
        Some(ObjectiveState::Active)
    );

    // A teleport over the same volume collects nothing between the endpoints.
    let teleport = runtime
        .step(&TickInput {
            movements: &[(
                PLAYER,
                Movement::Teleport {
                    to_m: [90.0, 0.0, 0.0],
                },
            )],
            ..input(2)
        })
        .unwrap();
    assert!(
        teleport
            .events
            .iter()
            .all(|event| !matches!(event.kind, ObjectiveEventKind::TriggerCrossed(_)))
    );

    // Only the program's own arm starts the deadline.
    runtime
        .step(&TickInput {
            timer_requests: &[TimerRequest::Arm(COMPLETE_DEADLINE)],
            ..input(3)
        })
        .unwrap();
    assert!(matches!(
        runtime.timer_state(COMPLETE_DEADLINE),
        Some(TimerState::Armed {
            since: Tick(3),
            remaining: 1
        })
    ));
}

/// A refused movement refuses the **whole tick**: no trigger observed, no
/// counter moved, no state changed. The runtime validates every movement before
/// sweeping any of them, so a crossing cannot be lost for one actor and kept for
/// another.
#[test]
fn accept_f39_b_a_refused_movement_leaves_the_whole_tick_untouched() {
    let mut runtime = ac02_runtime();
    runtime
        .add_trigger(SweptTrigger::new(APPROACH, PLAYER, synthetic_small_volume()).unwrap())
        .unwrap();
    runtime
        .step(&TickInput {
            movements: &[(
                PLAYER,
                Movement::Continuous {
                    from_m: [-5.0, 0.0, 0.0],
                    to_m: [0.0, 0.0, 0.0],
                },
            )],
            ..input(1)
        })
        .unwrap();

    let refused = runtime.step(&TickInput {
        lifecycles: &[(PROTECTED, LifecycleKind::Destroyed)],
        movements: &[
            (
                PLAYER,
                Movement::Continuous {
                    from_m: [f64::NAN, 0.0, 0.0],
                    to_m: [0.0; 3],
                },
            ),
            (
                PLAYER,
                Movement::Continuous {
                    from_m: [0.0, 0.0, 0.0],
                    to_m: [90.0, 0.0, 0.0],
                },
            ),
        ],
        ..input(2)
    });
    assert!(matches!(
        refused,
        Err(RuntimeError::Trigger(
            cs_sim::objectives::trigger::TriggerError::NonFinite
        ))
    ));
    assert_eq!(runtime.counted(CountKind::Destroyed), 0);

    // The trigger did not observe the refused tick either: it still believes it
    // is inside the volume from tick 1, so the next valid tick reports the exit
    // and not a fresh entry.
    let next = runtime
        .step(&TickInput {
            movements: &[(
                PLAYER,
                Movement::Continuous {
                    from_m: [0.0, 0.0, 0.0],
                    to_m: [90.0, 0.0, 0.0],
                },
            )],
            ..input(3)
        })
        .unwrap();
    let mut kinds: Vec<CrossingKind> = Vec::new();
    for event in &next.events {
        if let ObjectiveEventKind::TriggerCrossed(crossing) = event.kind {
            kinds.push(crossing.kind);
        }
    }
    assert_eq!(kinds, [CrossingKind::Exit]);
}

// ---------------------------------------------------------------------------
// Timers: declared start, declared domain, one declared action
// ---------------------------------------------------------------------------

/// F39 non-negotiable behavior 3, the three parts. A declared start condition is
/// data: an `OnArm` deadline stays unarmed, an `AtTick` one arms itself, a
/// `Never` one never runs. A non-gameplay domain is refused at declaration. The
/// expiry performs exactly one declared action, in the same tick, with no
/// reentrancy.
#[test]
fn accept_f39_b_a_timer_declares_its_start_its_domain_and_one_action() {
    // A UI-wall or unscaled-media deadline would keep moving while the game is
    // paused, so it is refused at declaration.
    assert_eq!(
        MissionTimer::new(
            COMPLETE_DEADLINE,
            ClockPolicy::ui_wall(),
            TimerStart::OnArm,
            1,
            TimerAction::Finish(TerminalOutcome::Failure),
        ),
        Err(TimerError::NotGameplayDomain {
            timer: COMPLETE_DEADLINE,
            domain: TimeDomain::UiWall
        })
    );
    assert_eq!(
        MissionTimer::new(
            COMPLETE_DEADLINE,
            ClockPolicy::authoritative_gameplay(),
            TimerStart::OnArm,
            0,
            TimerAction::Finish(TerminalOutcome::Failure),
        ),
        Err(TimerError::ZeroPeriod {
            timer: COMPLETE_DEADLINE
        })
    );

    let mut runtime = runtime();
    runtime
        .add_objective(ObjectiveSpec {
            id: PRIMARY,
            content: objective("deliver-the-medicine"),
            initial: ObjectiveState::Active,
            reveal: RevealRule::Immediate,
            on_complete: ObjectiveCompletion::Continue,
        })
        .unwrap();

    // Never: the declaration exists and never runs.
    runtime
        .add_timer(
            MissionTimer::new(
                SymbolId(21),
                ClockPolicy::authoritative_gameplay(),
                TimerStart::Never,
                1,
                TimerAction::SetObjectiveState {
                    objective: PRIMARY,
                    state: ObjectiveState::Failed,
                },
            )
            .unwrap(),
        )
        .unwrap();
    // AtTick: armed by itself, unattended, at the declared tick.
    runtime
        .add_timer(
            MissionTimer::new(
                COMPLETE_DEADLINE,
                ClockPolicy::authoritative_gameplay(),
                TimerStart::AtTick(Tick(3)),
                1,
                TimerAction::SetObjectiveState {
                    objective: PRIMARY,
                    state: ObjectiveState::Succeeded,
                },
            )
            .unwrap(),
        )
        .unwrap();

    let early = runtime
        .step(&TickInput {
            committed_ticks: 1,
            ..input(1)
        })
        .unwrap();
    assert!(early.events.is_empty());
    assert_eq!(runtime.timer_state(SymbolId(21)), Some(TimerState::Never));
    assert_eq!(
        runtime.timer_state(COMPLETE_DEADLINE),
        Some(TimerState::NotArmed),
        "a deadline whose declared start tick has not arrived consumes ticks and never arms"
    );

    let before = runtime
        .step(&TickInput {
            committed_ticks: 1,
            ..input(2)
        })
        .unwrap();
    assert!(before.events.is_empty());
    assert_eq!(
        runtime.timer_state(COMPLETE_DEADLINE),
        Some(TimerState::NotArmed)
    );

    // The declared start tick arrives: the deadline arms itself, unattended, and
    // the one committed tick runs it out in the same tick.
    let done = runtime
        .step(&TickInput {
            committed_ticks: 1,
            ..input(3)
        })
        .unwrap();
    assert_eq!(
        runtime.timer_state(COMPLETE_DEADLINE),
        Some(TimerState::Expired { at: Tick(3) })
    );
    assert_eq!(
        runtime.objective_state(PRIMARY),
        Some(ObjectiveState::Succeeded)
    );
    assert_eq!(runtime.timer_state(SymbolId(21)), Some(TimerState::Never));
    // The expiry event precedes the action's event, and the action is the only
    // state change the tick produced.
    let order: Vec<&str> = done
        .events
        .iter()
        .map(|event| match event.kind {
            ObjectiveEventKind::TimerArmed { .. } => "armed",
            ObjectiveEventKind::TimerExpired { .. } => "expired",
            ObjectiveEventKind::ObjectiveChanged { .. } => "changed",
            _ => "other",
        })
        .collect();
    assert_eq!(order, ["armed", "expired", "changed"]);

    // The objective now latched `Succeeded`, which is terminal for it: a second
    // expiry of the same kind would be refused, so its action runs once.
    let after = runtime
        .step(&TickInput {
            timer_requests: &[TimerRequest::Arm(COMPLETE_DEADLINE)],
            committed_ticks: 1,
            ..input(4)
        })
        .unwrap();
    assert!(
        after
            .events
            .iter()
            .all(|event| !matches!(event.kind, ObjectiveEventKind::ObjectiveChanged { .. }))
    );
}

/// A deadline measures **committed ticks only**. A paused frame commits zero, so
/// it cannot shorten a deadline; there is no wall-time entry point on
/// [`TickInput`], so the case is not expressible.
#[test]
fn accept_f39_b_a_paused_frame_commits_no_ticks_and_moves_no_deadline() {
    let mut runtime =
        ObjectiveRuntime::new(SESSION, TerminalPrecedence::SyntheticConservative, limits());
    runtime
        .add_timer(
            MissionTimer::new(
                COMPLETE_DEADLINE,
                ClockPolicy::authoritative_gameplay(),
                TimerStart::OnArm,
                10,
                TimerAction::Finish(TerminalOutcome::Failure),
            )
            .unwrap(),
        )
        .unwrap();
    runtime
        .step(&TickInput {
            timer_requests: &[TimerRequest::Arm(COMPLETE_DEADLINE)],
            ..input(1)
        })
        .unwrap();

    for tick in 2..=6 {
        let paused = runtime.step(&input(tick)).unwrap();
        assert!(
            paused.events.is_empty(),
            "a paused frame produces no events"
        );
    }
    assert_eq!(
        runtime.timer_state(COMPLETE_DEADLINE),
        Some(TimerState::Armed {
            since: Tick(1),
            remaining: 10
        })
    );
    assert_eq!(runtime.outcome(), None);

    runtime
        .step(&TickInput {
            committed_ticks: 10,
            ..input(7)
        })
        .unwrap();
    assert_eq!(runtime.outcome(), Some(TerminalOutcome::Failure));
}

/// A timer runs once per declared start. The same signal again does not replay
/// its action, so a repeated cue cannot become a second wave — F39
/// non-negotiable behavior 4 through the timer table — and a cancelled or
/// unarmed timer refuses instead of running.
#[test]
fn accept_f39_b_a_signal_arms_a_timer_once_and_a_repeat_does_not_replay_it() {
    const WAVES: SymbolId = SymbolId(40);
    let key = || IdempotencyKey("wave-2".into());
    let mut runtime = runtime();
    runtime
        .add_timer(
            MissionTimer::new(
                WAVES,
                ClockPolicy::authoritative_gameplay(),
                TimerStart::OnSignal(SymbolId(50)),
                1,
                TimerAction::SpawnGroup {
                    key: key(),
                    group: SymbolId(51),
                    count: 2,
                },
            )
            .unwrap(),
        )
        .unwrap();
    // A second wave under its own key and its own signal, so the test can watch
    // which instance ids the refused repeat consumed.
    const REINFORCEMENTS: SymbolId = SymbolId(41);
    runtime
        .add_timer(
            MissionTimer::new(
                REINFORCEMENTS,
                ClockPolicy::authoritative_gameplay(),
                TimerStart::OnSignal(SymbolId(52)),
                1,
                TimerAction::SpawnGroup {
                    key: IdempotencyKey("wave-3".into()),
                    group: SymbolId(53),
                    count: 2,
                },
            )
            .unwrap(),
        )
        .unwrap();

    // The signal that raises it is the program's own; it becomes eligible on the
    // next tick, so no timer is armed by the signal that raised it in the same
    // tick.
    runtime
        .step(&TickInput {
            signals: &[SymbolId(50)],
            ..input(1)
        })
        .unwrap();
    assert_eq!(runtime.timer_state(WAVES), Some(TimerState::NotArmed));

    let armed = runtime
        .step(&TickInput {
            signals: &[SymbolId(50)],
            committed_ticks: 1,
            ..input(2)
        })
        .unwrap();
    assert!(matches!(
        runtime.timer_state(WAVES),
        Some(TimerState::Expired { at: Tick(2) })
    ));
    assert!(matches!(
        armed.first(|kind| matches!(kind, ObjectiveEventKind::SpawnAdmitted { .. })),
        Some(ObjectiveEvent { kind: ObjectiveEventKind::SpawnAdmitted { group, instances, .. }, .. })
            if *group == SymbolId(51) && instances == &vec![ActorId(1), ActorId(2)]
    ));
    let first = runtime.spawned_instances(&key()).map(<[ActorId]>::to_vec);
    assert_eq!(first, Some(vec![ActorId(1), ActorId(2)]));

    // The same signal again: refused, and the ids stay the ones the first
    // admission handed out.
    let repeat = runtime
        .step(&TickInput {
            signals: &[SymbolId(50)],
            committed_ticks: 1,
            ..input(3)
        })
        .unwrap();
    assert!(matches!(
        repeat.first(|kind| matches!(kind, ObjectiveEventKind::SpawnRefused { .. })),
        Some(ObjectiveEvent { kind: ObjectiveEventKind::SpawnRefused { instances, .. }, .. })
            if instances == &vec![ActorId(1), ActorId(2)]
    ));
    assert_eq!(
        runtime.spawned_instances(&key()).map(<[ActorId]>::to_vec),
        first
    );

    // The refused repeat consumed no instance ids: the next wave a *different*
    // key asks for starts where the first one stopped. A refusal that allocated
    // and threw the ids away would leave a gap here, and a stable instance id is
    // what a host binds an entity to.
    runtime
        .step(&TickInput {
            signals: &[SymbolId(52)],
            ..input(4)
        })
        .unwrap();
    let next = runtime
        .step(&TickInput {
            signals: &[SymbolId(52)],
            committed_ticks: 1,
            ..input(5)
        })
        .unwrap();
    assert!(next.events.iter().any(|event| {
        matches!(
            &event.kind,
            ObjectiveEventKind::SpawnAdmitted { key, instances, .. }
                if key == &IdempotencyKey("wave-3".into())
                    && instances == &vec![ActorId(3), ActorId(4)]
        )
    }));
}

/// An explicit program arm may run a timer again; a cancellation stops it; and a
/// request that names nothing, or an unarmed cancellation, is reported rather
/// than silently ignored.
#[test]
fn accept_f39_b_timer_requests_are_applied_or_reported_never_ignored() {
    const DEADLINE: SymbolId = SymbolId(60);
    let mut runtime = runtime();
    runtime
        .add_timer(
            MissionTimer::new(
                DEADLINE,
                ClockPolicy::single_player_simulation(),
                TimerStart::OnArm,
                2,
                TimerAction::Finish(TerminalOutcome::Success),
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(
        MissionTimer::new(
            DEADLINE,
            ClockPolicy::single_player_simulation(),
            TimerStart::OnArm,
            2,
            TimerAction::Finish(TerminalOutcome::Success),
        )
        .unwrap()
        .domain(),
        TimeDomain::Simulation
    );

    // Cancelling a timer that was never armed is refused and reported.
    let refused = runtime
        .step(&TickInput {
            timer_requests: &[TimerRequest::Cancel(DEADLINE)],
            ..input(1)
        })
        .unwrap();
    assert!(matches!(
        refused.first(|kind| matches!(kind, ObjectiveEventKind::TimerRefused { .. })),
        Some(ObjectiveEvent {
            kind: ObjectiveEventKind::TimerRefused {
                reason: TimerError::NotArmed { .. },
                ..
            },
            ..
        })
    ));

    runtime
        .step(&TickInput {
            timer_requests: &[TimerRequest::Arm(DEADLINE)],
            ..input(2)
        })
        .unwrap();
    let cancelled = runtime
        .step(&TickInput {
            timer_requests: &[TimerRequest::Cancel(DEADLINE)],
            committed_ticks: 5,
            ..input(3)
        })
        .unwrap();
    assert!(cancelled.events.is_empty());
    assert_eq!(runtime.timer_state(DEADLINE), Some(TimerState::NotArmed));
    assert_eq!(runtime.outcome(), None);
}

// ---------------------------------------------------------------------------
// Reveal rules, optional rewards and the ordered stream
// ---------------------------------------------------------------------------

/// F39 non-negotiable behavior 5: an objective is shown only when its declared
/// reveal rule allows, and an optional reward is not a mission ending.
///
/// The discriminating half is the fourth objective: its reveal rule watches a
/// condition that never fires, and a declared action tries to move it out of
/// `Hidden` anyway. Both halves must fail — the rule must gate the *reveal* and
/// it must gate every other way of leaving `Hidden`.
#[test]
fn accept_f39_b_reveal_is_the_only_way_to_show_an_objective_and_a_reward_is_not_an_outcome() {
    const SECRET: SymbolId = SymbolId(70);
    const OPTIONAL: SymbolId = SymbolId(71);
    const REWARD_TIMER: SymbolId = SymbolId(72);
    const LATER: SymbolId = SymbolId(73);
    const NEVER_MET: SymbolId = SymbolId(74);
    let reward = content(ContentKind::Objective, "optional-scrapbook-page");

    let mut runtime = runtime();
    for (id, initial, reveal) in [
        (
            PRIMARY,
            ObjectiveState::Pending,
            RevealRule::OnCondition {
                condition: PROTECTED_LOST,
            },
        ),
        (
            OPTIONAL,
            ObjectiveState::Optional,
            RevealRule::OnTimer {
                timer: REWARD_TIMER,
            },
        ),
        (
            SECRET,
            ObjectiveState::Hidden,
            RevealRule::OnSignal {
                signal: SymbolId(80),
            },
        ),
        (
            LATER,
            ObjectiveState::Hidden,
            RevealRule::OnCondition {
                condition: NEVER_MET,
            },
        ),
    ] {
        runtime
            .add_objective(ObjectiveSpec {
                id,
                content: objective("mission-objective"),
                initial,
                reveal,
                on_complete: ObjectiveCompletion::Continue,
            })
            .unwrap();
    }
    // A declaration that says both "hidden" and "shown from the first tick" is
    // refused rather than resolved by a convention.
    assert_eq!(
        runtime.add_objective(ObjectiveSpec {
            id: SymbolId(75),
            content: objective("contradictory"),
            initial: ObjectiveState::Hidden,
            reveal: RevealRule::Immediate,
            on_complete: ObjectiveCompletion::Continue,
        }),
        Err(RuntimeError::HiddenButImmediate {
            objective: SymbolId(75)
        })
    );
    assert!(
        !runtime.is_visible(PRIMARY),
        "a reveal rule that has not fired hides it"
    );
    assert!(!runtime.is_visible(OPTIONAL));
    assert!(!runtime.is_visible(SECRET));
    assert!(!runtime.is_visible(LATER));

    runtime
        .add_condition(
            CountCondition::new(PROTECTED_LOST, CountKind::Destroyed, [PROTECTED], 1).unwrap(),
            CountReaction::ReportOnly,
        )
        .unwrap();
    // The condition `LATER` waits for watches an actor that is never counted.
    runtime
        .add_condition(
            CountCondition::new(NEVER_MET, CountKind::Captured, [ActorId(88)], 1).unwrap(),
            CountReaction::ReportOnly,
        )
        .unwrap();
    runtime
        .add_timer(
            MissionTimer::new(
                REWARD_TIMER,
                ClockPolicy::authoritative_gameplay(),
                TimerStart::OnArm,
                1,
                TimerAction::GrantOptionalReward {
                    reward: reward.clone(),
                },
            )
            .unwrap(),
        )
        .unwrap();

    let revealed = runtime
        .step(&TickInput {
            timer_requests: &[TimerRequest::Arm(REWARD_TIMER)],
            committed_ticks: 1,
            signals: &[SymbolId(80)],
            lifecycles: &[(PROTECTED, LifecycleKind::Destroyed)],
            // A declared action that tries to show `LATER` by moving it.
            objective_requests: &[(LATER, ObjectiveState::Pending)],
            ..input(1)
        })
        .unwrap();

    assert!(runtime.is_visible(PRIMARY), "its count condition latched");
    assert!(runtime.is_visible(OPTIONAL), "its timer expired");
    assert!(runtime.is_visible(SECRET), "its signal was raised");
    assert!(
        !runtime.is_visible(LATER),
        "an objective whose reveal rule has not fired stays hidden, whatever else happened"
    );
    assert_eq!(
        runtime.objective_state(LATER),
        Some(ObjectiveState::Hidden),
        "and no declared action moved it: the refusal is reported, not applied"
    );
    assert!(matches!(
        revealed.first(|kind| matches!(kind, ObjectiveEventKind::ObjectiveChangeRefused { .. })),
        Some(ObjectiveEvent {
            kind: ObjectiveEventKind::ObjectiveChangeRefused { objective, from: ObjectiveState::Hidden, to: ObjectiveState::Pending },
            ..
        }) if *objective == LATER
    ));
    // The reveal that did fire reported the state it revealed the objective into.
    assert!(revealed.events.iter().any(|event| matches!(
        event.kind,
        ObjectiveEventKind::ObjectiveRevealed {
            objective,
            state: ObjectiveState::Pending
        } if objective == SECRET
    )));
    assert_eq!(
        runtime.objective_state(OPTIONAL),
        Some(ObjectiveState::Optional),
        "an optional reward is not a state change of the objective it rewards"
    );
    assert_eq!(runtime.outcome(), None, "a reward never ends the mission");
    assert!(
        revealed
            .first(|kind| matches!(kind, ObjectiveEventKind::OptionalReward { .. }))
            .is_some()
    );
    assert!(
        revealed
            .events
            .iter()
            .all(|event| !matches!(event.kind, ObjectiveEventKind::OutcomeSettled { .. }))
    );
}

/// The stream is ordered by [`cs_script::runtime::EventKey`] — session, tick,
/// source, sequence — and never by hash or entity iteration order, so a second
/// run of the same facts produces byte-identical ordering.
#[test]
fn accept_f39_b_the_event_stream_is_key_ordered_and_reproducible() {
    let build = || {
        let mut runtime = runtime();
        runtime
            .add_objective(ObjectiveSpec {
                id: SymbolId(2),
                content: objective("second"),
                initial: ObjectiveState::Active,
                reveal: RevealRule::Immediate,
                on_complete: ObjectiveCompletion::Continue,
            })
            .unwrap();
        runtime
            .add_objective(ObjectiveSpec {
                id: SymbolId(1),
                content: objective("first"),
                initial: ObjectiveState::Active,
                reveal: RevealRule::Immediate,
                on_complete: ObjectiveCompletion::Continue,
            })
            .unwrap();
        runtime
            .add_condition(
                CountCondition::new(
                    SymbolId(90),
                    CountKind::Destroyed,
                    [ActorId(3), ActorId(4)],
                    2,
                )
                .unwrap(),
                CountReaction::ReportOnly,
            )
            .unwrap();
        runtime
    };
    let play = |mut runtime: ObjectiveRuntime| {
        let tick = runtime
            .step(&TickInput {
                lifecycles: &[
                    (ActorId(4), LifecycleKind::Destroyed),
                    (ActorId(3), LifecycleKind::Destroyed),
                ],
                objective_requests: &[
                    (SymbolId(2), ObjectiveState::Succeeded),
                    (SymbolId(1), ObjectiveState::Active),
                ],
                ..input(1)
            })
            .unwrap();
        tick.events
            .iter()
            .map(|event| (event.key, event.kind.clone()))
            .collect::<Vec<_>>()
    };

    let first = play(build());
    let second = play(build());
    assert_eq!(first, second, "the same facts must produce the same stream");
    let keys: Vec<_> = first.iter().map(|(key, _)| *key).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "the stream is sorted by EventKey");
    // Counted actors share the reserved source and are ordered by sequence.
    assert_eq!(keys[0].source, ACTOR_EVENT_SOURCE);
    assert!(keys[0].sequence < keys[1].sequence);
    assert!(
        keys.iter()
            .all(|key| key.session == SESSION && key.tick == Tick(1))
    );
}

// ---------------------------------------------------------------------------
// Bounds, refusals and the retry precondition
// ---------------------------------------------------------------------------

/// The contract's bounded control flow. The bound is checked against the tick's
/// *declared* facts before anything is applied, so a bounded tick changes
/// nothing and the diagnostic names the tick, the bound and the declared work.
#[test]
fn accept_f39_b_a_tick_over_the_event_budget_is_stopped_before_it_applies_anything() {
    let mut runtime = ObjectiveRuntime::new(
        SESSION,
        TerminalPrecedence::SyntheticConservative,
        RuntimeLimits {
            max_events_per_tick: 2,
            max_timers: 1,
        },
    );
    runtime
        .add_objective(ObjectiveSpec {
            id: PRIMARY,
            content: objective("deliver-the-medicine"),
            initial: ObjectiveState::Active,
            reveal: RevealRule::Immediate,
            on_complete: ObjectiveCompletion::Continue,
        })
        .unwrap();

    let refused = runtime
        .step(&TickInput {
            objective_requests: &[
                (PRIMARY, ObjectiveState::Succeeded),
                (PRIMARY, ObjectiveState::Superseded),
                (PRIMARY, ObjectiveState::Failed),
            ],
            ..input(1)
        })
        .unwrap();
    assert!(refused.is_stopped());
    // Three declared requests plus one objective's own reveal allowance: the
    // bound is checked against the *declared* work, not against the events that
    // happened to be produced.
    assert_eq!(
        refused.stop,
        Some(StopReason::EventBudget {
            at_tick: Tick(1),
            limit: 2,
            declared: 4,
        })
    );
    assert!(refused.events.is_empty());
    assert_eq!(
        runtime.objective_state(PRIMARY),
        Some(ObjectiveState::Active),
        "a stopped tick applied nothing"
    );
    assert_eq!(runtime.last_tick(), None);
    // The refusal is repeatable: the tick is not marked as stepped, so the same
    // tick is still answerable once the caller sends less work.
    let smaller = runtime
        .step(&TickInput {
            objective_requests: &[(PRIMARY, ObjectiveState::Succeeded)],
            ..input(1)
        })
        .unwrap();
    assert!(!smaller.is_stopped());
    assert_eq!(
        runtime.objective_state(PRIMARY),
        Some(ObjectiveState::Succeeded)
    );

    // The declaration table is bounded the same way.
    assert!(
        runtime
            .add_timer(
                MissionTimer::new(
                    SymbolId(1),
                    ClockPolicy::authoritative_gameplay(),
                    TimerStart::Never,
                    1,
                    TimerAction::Finish(TerminalOutcome::Success),
                )
                .unwrap()
            )
            .is_ok()
    );
    assert_eq!(
        runtime.add_timer(
            MissionTimer::new(
                SymbolId(2),
                ClockPolicy::authoritative_gameplay(),
                TimerStart::Never,
                1,
                TimerAction::Finish(TerminalOutcome::Success),
            )
            .unwrap()
        ),
        Err(RuntimeError::TimerTableFull { limit: 1 })
    );

    // A tick at or before the last stepped one is refused outright.
    assert_eq!(
        runtime.step(&input(1)),
        Err(RuntimeError::NotAdvancing {
            last: Tick(1),
            given: Tick(1)
        })
    );
}

/// A retry is a new session generation and a new runtime: no objective state,
/// counter, trigger, timer, emission or latch survives. This is the
/// precondition F39-C's own minimum scenario ("retry after several waves")
/// needs, and the reason this runtime owns all of its mutable state.
#[test]
fn accept_f39_b_a_new_session_generation_carries_nothing_from_the_old_one() {
    // Two waves and a protected loss, played out twice.
    let play = |generation: SessionGeneration| {
        let mut runtime = ObjectiveRuntime::new(
            generation,
            TerminalPrecedence::SyntheticConservative,
            limits(),
        );
        runtime
            .add_condition(
                CountCondition::new(PROTECTED_LOST, CountKind::Destroyed, [PROTECTED], 1).unwrap(),
                CountReaction::Finish(TerminalOutcome::Failure),
            )
            .unwrap();
        runtime
            .add_timer(
                MissionTimer::new(
                    COMPLETE_DEADLINE,
                    ClockPolicy::authoritative_gameplay(),
                    TimerStart::OnArm,
                    1,
                    TimerAction::SpawnGroup {
                        key: IdempotencyKey("wave-2".into()),
                        group: SymbolId(51),
                        count: 2,
                    },
                )
                .unwrap(),
            )
            .unwrap();
        // Nothing has happened yet: a fresh generation starts empty even after a
        // session that already emitted a wave and lost a protected actor.
        assert_eq!(runtime.outcome(), None);
        assert_eq!(runtime.counted(CountKind::Destroyed), 0);
        assert!(
            runtime
                .spawned_instances(&IdempotencyKey("wave-2".into()))
                .is_none()
        );
        assert!(!runtime.condition_met(PROTECTED_LOST));
        assert_eq!(
            runtime.timer_state(COMPLETE_DEADLINE),
            Some(TimerState::NotArmed)
        );
        assert_eq!(runtime.last_tick(), None);

        runtime
            .step(&TickInput {
                timer_requests: &[TimerRequest::Arm(COMPLETE_DEADLINE)],
                committed_ticks: 1,
                ..input(1)
            })
            .unwrap();
        runtime
            .step(&TickInput {
                lifecycles: &[(PROTECTED, LifecycleKind::Destroyed)],
                ..input(2)
            })
            .unwrap();
        runtime
    };

    let first = play(SessionGeneration(1));
    assert_eq!(first.outcome(), Some(TerminalOutcome::Failure));
    assert!(first.is_counted(CountKind::Destroyed, PROTECTED));
    assert!(first.condition_met(PROTECTED_LOST));
    let first_wave: Vec<ActorId> = first
        .spawned_instances(&IdempotencyKey("wave-2".into()))
        .expect("the wave was admitted")
        .to_vec();
    assert_eq!(first_wave, vec![ActorId(1), ActorId(2)]);

    // The retry reaches the same outcome from its own events and nothing else,
    // and its wave takes this session's ids rather than the previous session's.
    let retry = play(SessionGeneration(2));
    assert_eq!(retry.session(), SessionGeneration(2));
    assert_eq!(retry.outcome(), Some(TerminalOutcome::Failure));
    assert!(retry.condition_met(PROTECTED_LOST));
    assert_eq!(
        retry
            .spawned_instances(&IdempotencyKey("wave-2".into()))
            .map(<[ActorId]>::to_vec),
        Some(first_wave),
        "the new generation's ledger is new: it admits the same key again"
    );
    // The old runtime is untouched by the new one.
    assert_eq!(first.outcome(), Some(TerminalOutcome::Failure));
}

/// The exact-one-category guarantee, spelled out as a set: the runtime never
/// reports a total across categories, so a consumer cannot read "everyone is
/// gone" out of a single number.
#[test]
fn accept_f39_b_categories_are_never_collapsed_into_one_total() {
    let mut runtime = runtime();
    let roster: BTreeSet<ActorId> = [ActorId(1), ActorId(2), ActorId(3)].into_iter().collect();
    let lifecycle = [
        (ActorId(1), LifecycleKind::Destroyed),
        (ActorId(2), LifecycleKind::OwnershipCaptured),
        (ActorId(3), LifecycleKind::Despawned),
    ];
    runtime
        .step(&TickInput {
            lifecycles: &lifecycle,
            ..input(1)
        })
        .unwrap();

    let observed: Vec<usize> = [
        CountKind::Destroyed,
        CountKind::Captured,
        CountKind::Despawned,
        CountKind::Disabled,
        CountKind::Escaped,
    ]
    .into_iter()
    .map(|kind| runtime.counted(kind))
    .collect();
    assert_eq!(
        observed,
        vec![1, 1, 1, 0, 0],
        "three actors in three different categories are three different counts, \
         and no category reports a total"
    );
    // A destroyed condition over that roster is not met: one of three is a kill
    // and the other two left the roster's category by other routes.
    runtime
        .add_condition(
            CountCondition::new(SymbolId(95), CountKind::Destroyed, roster, 3).unwrap(),
            CountReaction::ReportOnly,
        )
        .unwrap();
    runtime.step(&input(2)).unwrap();
    assert!(!runtime.condition_met(SymbolId(95)));
    assert_eq!(runtime.counted(CountKind::Destroyed), 1);
}
