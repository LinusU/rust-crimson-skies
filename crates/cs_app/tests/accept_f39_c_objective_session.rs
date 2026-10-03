//! Acceptance scenario F39-C: the declared mission program drives the wired
//! session, the ordered event stream feeds its consumers, and a retry leaves
//! no old timers, actors or cues behind.
//!
//! Spec: `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
//! stage `### F39-C`; shared contract `docs/contracts/SCRIPT-MISSION.md`.
//! Task test prefix: `accept_f39_c_`. Minimum scenario: *retry after several
//! waves and confirm no old timers, actors or cues survive*.
//!
//! These tests drive production code only:
//! [`cs_app::objectives::lower_program`] lowers the
//! [`cs_content::objectives::declared_synthetic_objectives`] fixture into a
//! launchable [`cs_app::objectives::ObjectiveSession`], whose `step` hands the
//! `cs_sim::objectives::ObjectiveRuntime` each tick's `TickInput` and
//! dispatches the ordered stream to the spawn directives, the pending cue
//! queue, the live wave registry, the objective display and the refusal
//! report — none of which exists without the wiring under test. Removing the
//! lowering, the dispatch or the retry teardown fails them.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_app::objectives::{
    LoweredObjectives, ObjectiveSession, ProgramLowerError, SessionLaunchError, SessionRefusal,
    lower_program,
};
use cs_content::objectives::{
    DeclaredObjectiveProgram, DeclaredTimeDomain, ProgramSymbol, declared_synthetic_objectives,
};
use cs_script::ir::{ActorId, SymbolId};
use cs_script::runtime::SessionGeneration;
use cs_sim::damage::LifecycleKind;
use cs_sim::objectives::counters::CountKind;
use cs_sim::objectives::runtime::{ObjectiveEventKind, RuntimeError, StopReason, TickInput};
use cs_sim::objectives::spawn::IdempotencyKey;
use cs_sim::objectives::state::ObjectiveState;
use cs_sim::objectives::terminal::TerminalOutcome;
use cs_sim::objectives::timer::{TimerError, TimerRequest, TimerState};
use cs_sim::objectives::trigger::Movement;
use cs_types::Tick;
use cs_types::content::Resolved;
use cs_types::evidence::ClaimId;

const GEN1: SessionGeneration = SessionGeneration(1);
const GEN2: SessionGeneration = SessionGeneration(2);

/// The fixture's program symbols as the runtime sees them.
const PRIMARY: SymbolId = SymbolId(1);
const SECONDARY: SymbolId = SymbolId(2);
const WAVE_1: SymbolId = SymbolId(20);
const WAVE_2: SymbolId = SymbolId(21);
const WAVE_3: SymbolId = SymbolId(22);
const RADIO: SymbolId = SymbolId(25);
const DEADLINE: SymbolId = SymbolId(30);
const RAIDERS: SymbolId = SymbolId(40);
const APPROACH: SymbolId = SymbolId(50);
const PLAYER: ActorId = ActorId(7);
const PROTECTED: ActorId = ActorId(41);
const REACHED_WRECK: SymbolId = SymbolId(60);

fn lowered() -> LoweredObjectives {
    lower_program(&declared_synthetic_objectives()).expect("the fixture lowers")
}

fn launch() -> ObjectiveSession {
    ObjectiveSession::launch(lowered(), GEN1).expect("the lowered program launches")
}

/// An input for `tick` with `committed` whole committed ticks and no facts;
/// the caller fills the fact slices before `step` borrows it.
fn input<'a>(tick: u64, committed: u64) -> TickInput<'a> {
    TickInput {
        tick: Tick(tick),
        committed_ticks: committed,
        lifecycles: &[],
        movements: &[],
        signals: &[],
        timer_requests: &[],
        objective_requests: &[],
        terminal_requests: &[],
    }
}

/// Arms `timers` on `tick`, committing `committed` whole ticks.
fn arm_step(
    session: &mut ObjectiveSession,
    tick: u64,
    timers: &[SymbolId],
    committed: u64,
) -> cs_app::objectives::SessionTick {
    let requests = timers
        .iter()
        .copied()
        .map(TimerRequest::Arm)
        .collect::<Vec<_>>();
    let mut facts = input(tick, committed);
    facts.timer_requests = &requests;
    session.step(&facts).expect("a legal tick")
}

// ---------------------------------------------------------------------------
// The lowering boundary
// ---------------------------------------------------------------------------

#[test]
fn accept_f39_c_the_declared_program_lowers_and_launches() {
    let lowered = lowered();

    // Every declaration arrives: two objectives, one condition, five timers,
    // one trigger, the spawn-group binding.
    assert_eq!(lowered.objectives.len(), 2);
    assert_eq!(lowered.conditions.len(), 1);
    assert_eq!(lowered.timers.len(), 5);
    assert_eq!(lowered.triggers.len(), 1);
    let group = lowered.spawn_groups.get(&RAIDERS).expect("raiders bound");
    assert_eq!(group.subject.as_str(), "airframe/synthetic.raider");

    let session = ObjectiveSession::launch(lowered, GEN1).expect("launch");
    assert_eq!(session.session(), GEN1);
    assert!(session.outcome().is_none());
    assert!(session.live_actors().is_empty());
    assert_eq!(session.pending_cues(), 0);

    // The display seeds from the declared specs: the primary objective is
    // born visible; the secondary waits for its signal and shows nothing.
    let visible = session.display().visible();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].symbol, PRIMARY);
    assert_eq!(visible[0].state, ObjectiveState::Active);
    let secondary = session.display().row(SECONDARY).expect("tracked");
    assert!(!secondary.revealed);
    assert_eq!(secondary.state, ObjectiveState::Hidden);
}

#[test]
fn accept_f39_c_lowering_refuses_unknowns_and_non_gameplay_domains() {
    let base = declared_synthetic_objectives();

    // An unmeasured precedence is refused by name, not defaulted.
    let unknown = DeclaredObjectiveProgram::try_new(
        base.subject().clone(),
        base.origin().clone(),
        base.provenance().clone(),
        Resolved::unknown(
            ClaimId::new("f39c.test.unmeasured-precedence").expect("claim id"),
            "the original terminal precedence is unmeasured",
        )
        .expect("a reasoned unknown"),
        base.objectives().to_vec(),
        base.conditions().to_vec(),
        base.timers().to_vec(),
        base.triggers().to_vec(),
        base.spawn_groups().to_vec(),
    )
    .expect("a program may carry an unknown precedence");
    assert!(matches!(
        lower_program(&unknown).unwrap_err(),
        ProgramLowerError::UnknownPrecedence { .. }
    ));

    // A deadline on UI wall time would advance during a pause; refused by
    // name rather than silently re-domained.
    let mut timers = base.timers().to_vec();
    timers[0].domain = DeclaredTimeDomain::UiWall;
    let wall = DeclaredObjectiveProgram::try_new(
        base.subject().clone(),
        base.origin().clone(),
        base.provenance().clone(),
        base.precedence().clone(),
        base.objectives().to_vec(),
        base.conditions().to_vec(),
        timers,
        base.triggers().to_vec(),
        base.spawn_groups().to_vec(),
    )
    .expect("a wall-domain timer is a legal declaration");
    assert_eq!(
        lower_program(&wall).unwrap_err(),
        ProgramLowerError::NonGameplayDomain {
            timer: ProgramSymbol(WAVE_1.0),
            domain: DeclaredTimeDomain::UiWall,
        }
    );
}

// ---------------------------------------------------------------------------
// Producer → runtime → consumers
// ---------------------------------------------------------------------------

#[test]
fn accept_f39_c_one_tick_dispatches_the_wave_the_cue_and_nothing_else() {
    let mut session = launch();

    let stepped = arm_step(&mut session, 1, &[WAVE_1, RADIO], 1);

    // The world-facing spawn directive: the bound subject, this session's
    // generation, the exact instance ids the runtime allocated.
    assert_eq!(stepped.spawns.len(), 1);
    let wave = &stepped.spawns[0];
    assert_eq!(wave.session, GEN1);
    assert_eq!(wave.tick, Tick(1));
    assert_eq!(wave.key, IdempotencyKey("synthetic.f39c.wave-1".to_owned()));
    assert_eq!(wave.group, RAIDERS);
    assert_eq!(wave.subject.as_str(), "airframe/synthetic.raider");
    assert_eq!(wave.instances, vec![ActorId(1), ActorId(2)]);
    assert_eq!(session.live_wave(RAIDERS), &[ActorId(1), ActorId(2)][..]);
    assert_eq!(session.live_actors(), vec![ActorId(1), ActorId(2)]);

    // The dialogue cue is pending once; draining hands it over once.
    assert_eq!(session.pending_cues(), 1);
    let cues = session.drain_cues();
    assert_eq!(cues.len(), 1);
    assert_eq!(cues[0].session, GEN1);
    assert_eq!(
        cues[0].key,
        IdempotencyKey("synthetic.f39c.radio-wave-inbound".to_owned())
    );
    assert_eq!(
        cues[0].dialogue.as_str(),
        "dialogue/synthetic.f39c.wave-inbound"
    );
    assert!(session.drain_cues().is_empty(), "a drained cue is gone");

    // No objective moved: the display is untouched and no refusal occurred.
    assert!(!stepped.display_changed);
    assert!(stepped.refusals.is_empty());
    assert!(stepped.outcome.is_none());
    assert!(stepped.stop.is_none());
}

#[test]
fn accept_f39_c_the_display_only_shows_what_the_stream_revealed() {
    let mut session = launch();

    // A declared state change against a still-hidden objective is refused,
    // reported and moves nothing on the display either.
    let objective_requests = [(SECONDARY, ObjectiveState::Active)];
    let mut facts = input(1, 0);
    facts.objective_requests = &objective_requests;
    let refused = session.step(&facts).expect("a legal tick");
    assert_eq!(
        refused.refusals,
        vec![SessionRefusal::ObjectiveChange {
            objective: SECONDARY,
            from: ObjectiveState::Hidden,
            to: ObjectiveState::Active,
        }]
    );
    assert_eq!(session.display().visible().len(), 1, "only the primary");

    // The declared signal reveals it: the stream reports it and the display
    // follows, Pending and shown.
    let signals = [REACHED_WRECK];
    let mut facts = input(2, 0);
    facts.signals = &signals;
    let revealed = session.step(&facts).expect("a legal tick");
    assert!(revealed.display_changed);
    let secondary = session.display().row(SECONDARY).expect("tracked");
    assert!(secondary.revealed);
    assert_eq!(secondary.state, ObjectiveState::Pending);
    assert_eq!(session.display().visible().len(), 2);
}

#[test]
fn accept_f39_c_refusals_and_errors_are_reported_never_dropped() {
    let mut session = launch();
    arm_step(&mut session, 1, &[WAVE_1], 1);

    // A request naming a timer the program does not declare: refused, named.
    let unknown = SymbolId(999);
    let requests = [TimerRequest::Arm(unknown)];
    let mut facts = input(2, 0);
    facts.timer_requests = &requests;
    let stepped = session
        .step(&facts)
        .expect("a stream refusal is not an error");
    assert_eq!(
        stepped.refusals,
        vec![SessionRefusal::Request {
            request: unknown,
            reason: RuntimeError::UnknownTimer { timer: unknown },
        }]
    );

    // A cancel on a timer that is not running: refused, named.
    let cancel = [TimerRequest::Cancel(RADIO)];
    let mut facts = input(3, 0);
    facts.timer_requests = &cancel;
    let stepped = session.step(&facts).expect("a legal tick");
    assert_eq!(
        stepped.refusals,
        vec![SessionRefusal::Timer {
            timer: RADIO,
            reason: TimerError::NotArmed { timer: RADIO },
        }]
    );

    // A repeat of a wave's key is refused by the ledger, carrying the ids
    // the first admission took: the world can never spawn the wave twice.
    let stepped = arm_step(&mut session, 4, &[WAVE_1], 1);
    assert!(stepped.spawns.is_empty());
    assert_eq!(
        stepped.refusals,
        vec![SessionRefusal::Spawn {
            key: IdempotencyKey("synthetic.f39c.wave-1".to_owned()),
            group: RAIDERS,
            instances: vec![ActorId(1), ActorId(2)],
        }]
    );
    assert_eq!(session.live_actors(), vec![ActorId(1), ActorId(2)]);

    // A refused tick is an error, not a silent drop: the non-advancing tick
    // propagates the runtime's own error and no consumer moved.
    let before = session.live_actors();
    let mut stale = input(3, 1);
    stale.timer_requests = &[TimerRequest::Arm(WAVE_2)];
    let error = session.step(&stale).unwrap_err();
    assert!(matches!(error, RuntimeError::NotAdvancing { .. }));
    assert_eq!(session.live_actors(), before);
    assert_eq!(session.pending_cues(), 0);
}

#[test]
fn accept_f39_c_a_settled_outcome_stops_later_objective_work() {
    let mut session = launch();

    // The protected convoy actor is destroyed: the declared condition
    // latches and its declared reaction requests failure.
    let lifecycles = [(PROTECTED, LifecycleKind::Destroyed)];
    let mut facts = input(1, 0);
    facts.lifecycles = &lifecycles;
    let settled = session.step(&facts).expect("a legal tick");
    assert_eq!(settled.outcome, Some(TerminalOutcome::Failure));
    assert!(settled.tick.events.iter().any(|event| matches!(
        event.kind,
        ObjectiveEventKind::OutcomeSettled {
            outcome: TerminalOutcome::Failure,
            ..
        }
    )));

    // The next tick does no objective work at all: the runtime reports the
    // stop and the session dispatches nothing.
    let stepped = arm_step(&mut session, 2, &[WAVE_1], 1);
    assert_eq!(
        stepped.stop,
        Some(StopReason::OutcomeSettled {
            settled_at: Tick(1)
        })
    );
    assert!(stepped.tick.events.is_empty());
    assert!(stepped.spawns.is_empty());
    assert!(session.live_actors().is_empty());
    assert_eq!(session.pending_cues(), 0);
}

// ---------------------------------------------------------------------------
// The minimum acceptance scenario: retry after several waves
// ---------------------------------------------------------------------------

#[test]
fn accept_f39_c_retry_after_several_waves_leaves_no_old_timers_actors_or_cues() {
    let mut session = launch();

    // Several waves and a cue across three ticks. Wave 3's timer is armed
    // without committed ticks, so it is still counting at teardown.
    let t1 = arm_step(&mut session, 1, &[WAVE_1, RADIO], 1);
    assert_eq!(t1.spawns.len(), 1);
    assert_eq!(session.pending_cues(), 1);

    let t2 = arm_step(&mut session, 2, &[WAVE_2], 1);
    assert_eq!(t2.spawns[0].instances, vec![ActorId(3), ActorId(4)]);

    // A spawned raider is destroyed: a gone-category count releases it from
    // the live registry — the world no longer has to tear that one down —
    // while wave 3's deadline starts counting and has not run out.
    let lifecycles = [(ActorId(1), LifecycleKind::Destroyed)];
    let arm = [TimerRequest::Arm(WAVE_3)];
    let mut facts = input(3, 0);
    facts.lifecycles = &lifecycles;
    facts.timer_requests = &arm;
    let t3 = session.step(&facts).expect("a legal tick");
    assert!(t3.spawns.is_empty(), "wave 3 was armed, not expired");
    assert_eq!(
        session.live_actors(),
        vec![ActorId(2), ActorId(3), ActorId(4)]
    );
    assert_eq!(
        session.runtime().timer_state(WAVE_3),
        Some(TimerState::Armed {
            since: Tick(3),
            remaining: 1
        })
    );

    // The approach trigger crossed this generation: a fact of session 1.
    let movements = [(
        PLAYER,
        Movement::Continuous {
            from_m: [5.0, 0.0, 0.0],
            to_m: [0.0, 0.0, 0.0],
        },
    )];
    let mut facts = input(4, 0);
    facts.movements = &movements;
    let t4 = session.step(&facts).expect("a legal tick");
    assert!(t4.tick.events.iter().any(|event| matches!(
        event.kind,
        ObjectiveEventKind::TriggerCrossed(crossing)
            if crossing.trigger == APPROACH
    )));

    // Retry: the teardown names what generation 1 still owned — the three
    // live raiders to despawn, the cue that was emitted but never drained,
    // and the one deadline still counting.
    let teardown = session.retry(GEN2).expect("retry");
    assert_eq!(teardown.session, GEN1);
    assert_eq!(teardown.actors, vec![ActorId(2), ActorId(3), ActorId(4)]);
    assert_eq!(teardown.cues.len(), 1);
    assert_eq!(teardown.cues[0].session, GEN1);
    assert_eq!(teardown.armed_timers, vec![WAVE_3]);
    assert!(teardown.outcome.is_none());

    // The new generation owns nothing of the old one: no live actors, no
    // pending cues, no armed or expired timers, no settled outcome, and the
    // display is back to its declared seed.
    assert_eq!(session.session(), GEN2);
    assert!(session.live_actors().is_empty());
    assert_eq!(session.pending_cues(), 0);
    assert!(session.outcome().is_none());
    for timer in [WAVE_1, WAVE_2, WAVE_3, RADIO, DEADLINE] {
        assert_eq!(
            session.runtime().timer_state(timer),
            Some(TimerState::NotArmed),
            "timer {timer:?} survived the retry"
        );
    }
    let visible = session.display().visible();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].symbol, PRIMARY);
    assert!(!session.display().row(SECONDARY).expect("tracked").revealed);

    // The old session's ledger does not suppress the new one's emissions:
    // the same authored keys admit fresh waves and a fresh cue, and the
    // instance ids restart at 1 — which is exactly why the teardown report
    // named the actors to despawn first.
    let gen2_t1 = arm_step(&mut session, 1, &[WAVE_1, RADIO], 1);
    assert_eq!(gen2_t1.spawns.len(), 1);
    assert_eq!(gen2_t1.spawns[0].session, GEN2);
    assert_eq!(gen2_t1.spawns[0].instances, vec![ActorId(1), ActorId(2)]);
    assert!(
        gen2_t1.refusals.is_empty(),
        "a fresh session's emission is never a stale key's refusal"
    );
    assert!(
        gen2_t1
            .tick
            .events
            .iter()
            .all(|event| event.key.session == GEN2),
        "every event is stamped with the live generation"
    );

    let cues = session.drain_cues();
    assert_eq!(cues.len(), 1);
    assert_eq!(cues[0].session, GEN2);
    assert_ne!(
        cues[0].session, teardown.cues[0].session,
        "a stale cue can never be confused with a current one"
    );

    // And the new generation plays the mission out: the deadline completes
    // the primary objective, whose completion requests success.
    arm_step(&mut session, 2, &[DEADLINE], 1);
    let stepped = session.step(&input(3, 0)).expect("a legal tick");
    assert_eq!(stepped.outcome, Some(TerminalOutcome::Success));
    assert_eq!(
        session.display().row(PRIMARY).expect("tracked").state,
        ObjectiveState::Succeeded
    );
}

#[test]
fn accept_f39_c_retry_after_a_settled_outcome_leaves_the_ending_behind() {
    let mut session = launch();

    // Generation 1 fails: the protected actor is destroyed.
    let lifecycles = [(PROTECTED, LifecycleKind::Destroyed)];
    let mut facts = input(1, 0);
    facts.lifecycles = &lifecycles;
    let failed = session.step(&facts).expect("a legal tick");
    assert_eq!(failed.outcome, Some(TerminalOutcome::Failure));

    let teardown = session.retry(GEN2).expect("retry");
    assert_eq!(teardown.outcome, Some(TerminalOutcome::Failure));
    assert!(session.outcome().is_none());
    assert!(!session.runtime().is_settled());
    assert!(
        !session
            .runtime()
            .is_counted(CountKind::Destroyed, PROTECTED),
        "the failed generation's counter did not survive"
    );

    // The retried session runs the same program fresh: a wave spawns where
    // the failed generation's settled runtime would have refused to work.
    let stepped = arm_step(&mut session, 1, &[WAVE_1], 1);
    assert_eq!(stepped.spawns.len(), 1);
    assert!(stepped.stop.is_none());
}

#[test]
fn accept_f39_c_a_retry_cannot_rebuild_the_live_generation() {
    let mut session = launch();
    arm_step(&mut session, 1, &[WAVE_1], 1);

    // A retry into the live generation would stamp the rebuilt session's
    // cues, waves and events with the very generation the torn-down
    // artifacts already carry — the confusion the stamps exist to prevent.
    assert_eq!(
        session.retry(GEN1).unwrap_err(),
        SessionLaunchError::SameGeneration { session: GEN1 }
    );

    // The refusal left the session untouched: same generation, same wave,
    // same expired timer.
    assert_eq!(session.session(), GEN1);
    assert_eq!(session.live_actors(), vec![ActorId(1), ActorId(2)]);
    assert_eq!(
        session.runtime().timer_state(WAVE_1),
        Some(TimerState::Expired { at: Tick(1) })
    );
}
