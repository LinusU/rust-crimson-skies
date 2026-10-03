//! Acceptance suite F39-D: original branching, optional and failure conditions
//! validated, and the AC04 out-of-order completion made order-independent.
//!
//! Spec: `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
//! stage `### F39-D`; shared contract: `docs/contracts/SCRIPT-MISSION.md`
//! ("Objective event ordering" and its rule that an undecodable mission program
//! leaves the mission Unsupported). Task test prefix: `accept_f39_d_`.
//! Minimum scenario: **complete supported objectives out of the common order
//! without deadlocking the program**.
//!
//! # What each test drives
//!
//! * `accept_f39_d_supported_objectives_complete_out_of_the_common_order` —
//!   **AC04.** A program whose four objectives are completed in an order that is
//!   *not* the authored one: a revealed-but-unpursued secondary completes first,
//!   an optional objective completes next, a reward intent is granted, the
//!   primary completes last. Every supported objective reaches a terminal state,
//!   exactly one terminal outcome latches, and no tick stalls. Removing the
//!   `Pending -> Succeeded` row makes the secondary's completion a refusal and
//!   the primary's outcome unreachable.
//! * `accept_f39_d_a_revealed_objective_can_complete_before_it_is_pursued` — the
//!   regression on its own, as the sheet's "including its failure cases" asks: a
//!   single-objective program whose reveal and completion land on consecutive
//!   ticks. With the row gone the objective can never succeed and the mission
//!   has no reachable ending at all.
//! * `accept_f39_d_optional_failure_and_success_stay_distinct` — the three
//!   endings never collapse into one another: an optional reward never settles
//!   an outcome, an optional objective's completion does not end the mission, a
//!   protected actor's declared category ends it, and a settled outcome is
//!   never changed by a later request.
//! * `accept_f39_d_a_branch_that_can_never_fire_is_refused_by_name` — the two
//!   dead declarations F39-D found: a reveal rule waiting for the objective it
//!   reveals, and a watch (a reveal or a timer) on a state the watched
//!   objective already holds. Each is refused by `try_new`, with the rule
//!   named, instead of producing a mission that waits forever.
//! * `accept_f39_d_a_watch_on_an_objective_born_finished_is_refused` — the
//!   second way a watch is dead: an objective born in a final state has no row
//!   leaving it, so a watch on any *other* state can never fire either, while
//!   the same watch stays legal from every birth state that has a row leaving
//!   it.
//! * `accept_f39_d_a_mutually_watching_branch_still_fires` — the other side of
//!   that check, so the refusal cannot be widened into "no watch is ever
//!   allowed": two objectives that watch each other's `Active` are legal, and
//!   the session reveals the second when the first is activated.
//! * `accept_f39_d_an_original_record_is_never_played_as_design` — the support
//!   gate. A record whose bytes came from the owner's installation is refused by
//!   `lower_program`, by name and with its own reason, whether or not the F39-D
//!   measurement is attached; a newly authored record is lowered and plays.
//! * `accept_f39_d_an_measured_record_is_kept_and_a_bare_one_is_refused` — the
//!   measurement is data a refusal can carry, and an empty measurement is
//!   refused rather than attached.
//! * `accept_f39_d_retail_objective_records_declare_branching_outcomes_and_optionality`
//!   (`#[ignore]`, needs `CS_GAME_DIR`) — the retail census over the owner's
//!   installation: every mission-scoped reader archive is measured, and the
//!   campaign's own records declare branching, optionality and outcome sites.
//!   The same test asserts the gate's verdict follows: an original record stays
//!   unplayable, because declaration sites are not rules; that
//!   `is_optional_objective_key` matches nothing outside the measured
//!   `INACTIVE1`…`INACTIVE18` range; and that the census's container spelling
//!   and mission scope name the same archive.
//!
//! Every value the non-retail tests use is newly authored synthetic fixture
//! data, never original game data.

use std::path::PathBuf;

use cs_app::objectives::{
    ObjectiveSession, ProgramLowerError, lower_program, survey_retail_objective_records,
};
use cs_content::objectives::{
    BRANCH_KEY_VOCABULARY, DeclaredCondition, DeclaredCountKind, DeclaredCountReaction,
    DeclaredObjective, DeclaredObjectiveProgram, DeclaredObjectiveState, DeclaredPrecedence,
    DeclaredRevealRule, DeclaredTerminalOutcome, DeclaredTimeDomain, DeclaredTimer,
    DeclaredTimerAction, DeclaredTimerStart, DeclaredTrigger, DeclaredVolume,
    FAILURE_KEY_VOCABULARY, MeasuredBranchPrecedence, MeasuredObjectiveRecord,
    OBJECTIVE_INACTIVE_COUNT_KEY, OBJECTIVE_INACTIVE_STAGE_PREFIX, ObjectivesSchemaError,
    ProgramActor, ProgramSymbol, UNMEASURED_OBJECTIVE_SEMANTICS, is_optional_objective_key,
};
use cs_script::ir::{ActorId, SymbolId};
use cs_script::runtime::SessionGeneration;
use cs_sim::damage::LifecycleKind;
use cs_sim::objectives::runtime::{ObjectiveEventKind, TickInput};
use cs_sim::objectives::state::ObjectiveState;
use cs_sim::objectives::terminal::TerminalOutcome;
use cs_sim::objectives::timer::TimerRequest;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ContentHash};

const GEN: SessionGeneration = SessionGeneration(1);

/// The out-of-order program's symbols.
const PRIMARY: SymbolId = SymbolId(1);
const SECONDARY: SymbolId = SymbolId(2);
const SIDE: SymbolId = SymbolId(3);
const PROTECTED_LOST: u32 = 10;
const T_SECONDARY: u32 = 20;
const T_SIDE: u32 = 21;
const T_REWARD: u32 = 22;
const T_PRIMARY: u32 = 23;
const PROTECTED: ActorId = ActorId(41);
/// An open signal name: it collides with no declaration, so the schema accepts
/// it and the session may raise it.
const REACHED_WRECK: u32 = 70;
/// The highest `INACTIVE<n>` stage the F39-D census measured, which the retail
/// test pins `is_optional_objective_key` against.
const MEASURED_INACTIVE_STAGES: u32 = 18;

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

/// Arms `timer` on `tick` and commits `committed` whole ticks.
fn arm(
    session: &mut ObjectiveSession,
    tick: u64,
    timer: SymbolId,
    committed: u64,
) -> cs_app::objectives::SessionTick {
    let requests = [TimerRequest::Arm(timer)];
    let mut facts = input(tick, committed);
    facts.timer_requests = &requests;
    session.step(&facts).expect("a legal tick")
}

fn designed_precedence(claim: &str) -> Resolved<DeclaredPrecedence> {
    Resolved::Known(Known::new(
        DeclaredPrecedence::SyntheticConservative,
        Provenance::designed(ClaimId::new(claim).expect("a valid claim id")),
    ))
}

fn objective_id(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Objective, key).expect("an objective id")
}

fn provenance(claim: &str) -> Provenance {
    Provenance::designed(ClaimId::new(claim).expect("a valid claim id"))
}

fn mission_id(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Mission, key).expect("a mission id")
}

fn timer(symbol: ProgramSymbol, action: DeclaredTimerAction) -> DeclaredTimer {
    DeclaredTimer {
        symbol,
        domain: DeclaredTimeDomain::AuthoritativeGameplay,
        start: DeclaredTimerStart::OnArm,
        period_ticks: 1,
        action,
    }
}

/// The out-of-order program of the AC04 scenario.
///
/// Four objectives and five deadlines:
///
/// * [`PRIMARY`] is born `Active` and requests `Success` when it completes —
///   the *last* thing this scenario completes, so the mission's ending depends
///   on a completion that happens out of the authored order;
/// * [`SECONDARY`] is born `Hidden` and revealed by a raised signal, which
///   leaves it `Pending` and never `Active`. Its deadline completes it — the
///   row `Pending -> Succeeded` that F39-D added;
/// * [`SIDE`] is born `Optional` and completes without ending the mission;
/// * a protected-actor `Destroyed` condition requests `Failure`.
fn out_of_order_program() -> DeclaredObjectiveProgram {
    DeclaredObjectiveProgram::try_new(
        mission_id("synthetic.f39d.branching"),
        Origin::SyntheticFixture,
        provenance("f39d.synthetic-branching"),
        designed_precedence("f39d.synthetic-branching"),
        vec![
            DeclaredObjective {
                symbol: ProgramSymbol(PRIMARY.0),
                content: objective_id("synthetic.f39d.primary"),
                initial: DeclaredObjectiveState::Active,
                reveal: DeclaredRevealRule::Immediate,
                on_complete: cs_content::objectives::DeclaredCompletion::Requests(
                    DeclaredTerminalOutcome::Success,
                ),
                completion_effects: Vec::new(),
            },
            DeclaredObjective {
                symbol: ProgramSymbol(SECONDARY.0),
                content: objective_id("synthetic.f39d.secondary"),
                initial: DeclaredObjectiveState::Hidden,
                reveal: DeclaredRevealRule::OnSignal(ProgramSymbol(REACHED_WRECK)),
                on_complete: cs_content::objectives::DeclaredCompletion::Continue,
                completion_effects: Vec::new(),
            },
            DeclaredObjective {
                symbol: ProgramSymbol(SIDE.0),
                content: objective_id("synthetic.f39d.side"),
                initial: DeclaredObjectiveState::Optional,
                reveal: DeclaredRevealRule::Immediate,
                on_complete: cs_content::objectives::DeclaredCompletion::Continue,
                completion_effects: Vec::new(),
            },
        ],
        vec![DeclaredCondition {
            symbol: ProgramSymbol(PROTECTED_LOST),
            kind: DeclaredCountKind::Destroyed,
            roster: vec![ProgramActor(PROTECTED.0)],
            required: 1,
            reaction: DeclaredCountReaction::Finish(DeclaredTerminalOutcome::Failure),
        }],
        vec![
            timer(
                ProgramSymbol(T_SECONDARY),
                DeclaredTimerAction::SetObjectiveState {
                    objective: ProgramSymbol(SECONDARY.0),
                    state: DeclaredObjectiveState::Succeeded,
                },
            ),
            timer(
                ProgramSymbol(T_SIDE),
                DeclaredTimerAction::SetObjectiveState {
                    objective: ProgramSymbol(SIDE.0),
                    state: DeclaredObjectiveState::Succeeded,
                },
            ),
            timer(
                ProgramSymbol(T_REWARD),
                DeclaredTimerAction::GrantOptionalReward {
                    reward: objective_id("synthetic.f39d.bonus"),
                },
            ),
            timer(
                ProgramSymbol(T_PRIMARY),
                DeclaredTimerAction::SetObjectiveState {
                    objective: ProgramSymbol(PRIMARY.0),
                    state: DeclaredObjectiveState::Succeeded,
                },
            ),
        ],
        vec![DeclaredTrigger {
            symbol: ProgramSymbol(50),
            actor: ProgramActor(7),
            volume: DeclaredVolume::Aabb {
                min_m: [-1.0, -1.0, -1.0],
                max_m: [1.0, 1.0, 1.0],
            },
        }],
        vec![],
    )
    .expect("the out-of-order program is valid")
}

fn launch(program: &DeclaredObjectiveProgram) -> ObjectiveSession {
    ObjectiveSession::launch(lower_program(program).expect("the program lowers"), GEN)
        .expect("the lowered program launches")
}

// ---------------------------------------------------------------------------
// AC04: supported objectives complete out of the common order
// ---------------------------------------------------------------------------

#[test]
fn accept_f39_d_supported_objectives_complete_out_of_the_common_order() {
    let program = out_of_order_program();
    let mut session = launch(&program);

    // The authored order would complete the primary first. This scenario does
    // the opposite: the signal reveals the secondary, and the secondary's own
    // deadline completes it while it is still `Pending`.
    let signals = [SymbolId(REACHED_WRECK)];
    let mut facts = input(1, 0);
    facts.signals = &signals;
    let revealed = session.step(&facts).expect("a legal tick");
    assert_eq!(
        session.display().row(SECONDARY).expect("tracked").state,
        ObjectiveState::Pending,
        "a revealed objective is shown, not yet pursued"
    );
    assert!(revealed.display_changed);

    let completed = arm(&mut session, 2, SymbolId(T_SECONDARY), 1);
    assert!(
        completed.refusals.is_empty(),
        "a revealed objective completed out of order must not be refused: {:?}",
        completed.refusals
    );
    assert_eq!(
        session.display().row(SECONDARY).expect("tracked").state,
        ObjectiveState::Succeeded,
        "the out-of-order completion latched"
    );
    assert!(
        completed.tick.events.iter().any(|event| matches!(
            event.kind,
            ObjectiveEventKind::ObjectiveChanged {
                to: ObjectiveState::Succeeded,
                ..
            }
        )),
        "the stream reports the completion"
    );
    assert!(
        completed.outcome.is_none(),
        "an objective whose completion is `Continue` never ends the mission"
    );

    // The optional objective completes next, and still ends nothing.
    let side = arm(&mut session, 3, SymbolId(T_SIDE), 1);
    assert!(side.refusals.is_empty());
    assert_eq!(
        session.display().row(SIDE).expect("tracked").state,
        ObjectiveState::Succeeded
    );
    assert!(side.outcome.is_none());

    // An optional reward is an intent, never a terminal outcome.
    let reward = arm(&mut session, 4, SymbolId(T_REWARD), 1);
    assert!(reward.outcome.is_none());
    assert!(
        reward
            .tick
            .events
            .iter()
            .any(|event| matches!(event.kind, ObjectiveEventKind::OptionalReward { .. })),
        "the reward intent is on the stream"
    );

    // The protected actor is lost *before* the primary completes: the declared
    // condition requests failure, and the latch holds it.
    let lifecycles = [(PROTECTED, LifecycleKind::Destroyed)];
    let mut facts = input(5, 0);
    facts.lifecycles = &lifecycles;
    let failed = session.step(&facts).expect("a legal tick");
    assert_eq!(failed.outcome, Some(TerminalOutcome::Failure));

    // Nothing after that changes the answer, however the objectives are ordered.
    let late = arm(&mut session, 6, SymbolId(T_PRIMARY), 1);
    assert_eq!(late.outcome, Some(TerminalOutcome::Failure));
    assert_eq!(
        late.stop,
        Some(cs_sim::objectives::runtime::StopReason::OutcomeSettled {
            settled_at: Tick(5)
        }),
        "a settled mission does no objective work again"
    );
    assert_eq!(
        session.display().row(PRIMARY).expect("tracked").state,
        ObjectiveState::Active,
        "the late completion never applied"
    );
}

#[test]
fn accept_f39_d_a_revealed_objective_can_complete_before_it_is_pursued() {
    // The regression alone: a one-objective program whose objective is revealed
    // by a signal and completed by the very next tick's deadline. With
    // `Pending -> Succeeded` missing, this program's mission has *no* reachable
    // ending: the completion is refused every time it is asked for.
    let program = DeclaredObjectiveProgram::try_new(
        mission_id("synthetic.f39d.revealed"),
        Origin::SyntheticFixture,
        provenance("f39d.synthetic-revealed"),
        designed_precedence("f39d.synthetic-revealed"),
        vec![DeclaredObjective {
            symbol: ProgramSymbol(PRIMARY.0),
            content: objective_id("synthetic.f39d.revealed-objective"),
            initial: DeclaredObjectiveState::Hidden,
            reveal: DeclaredRevealRule::OnSignal(ProgramSymbol(REACHED_WRECK)),
            on_complete: cs_content::objectives::DeclaredCompletion::Requests(
                DeclaredTerminalOutcome::Success,
            ),
            completion_effects: Vec::new(),
        }],
        vec![],
        vec![timer(
            ProgramSymbol(T_PRIMARY),
            DeclaredTimerAction::SetObjectiveState {
                objective: ProgramSymbol(PRIMARY.0),
                state: DeclaredObjectiveState::Succeeded,
            },
        )],
        vec![],
        vec![],
    )
    .expect("the revealed program is valid");
    let mut session = launch(&program);

    let signals = [SymbolId(REACHED_WRECK)];
    let mut facts = input(1, 0);
    facts.signals = &signals;
    session.step(&facts).expect("a legal tick");

    let stepped = arm(&mut session, 2, SymbolId(T_PRIMARY), 1);
    assert!(
        stepped.refusals.is_empty(),
        "the completion was refused: {:?}",
        stepped.refusals
    );
    assert_eq!(
        session.display().row(PRIMARY).expect("tracked").state,
        ObjectiveState::Succeeded
    );
    assert_eq!(stepped.outcome, Some(TerminalOutcome::Success));
}

// ---------------------------------------------------------------------------
// Optional, failure and success stay distinct (F39 non-negotiable 5)
// ---------------------------------------------------------------------------

#[test]
fn accept_f39_d_optional_failure_and_success_stay_distinct() {
    // An optional objective and an optional reward, in a program whose primary
    // ends the mission.
    let program = out_of_order_program();
    let mut session = launch(&program);

    // The optional objective completes: no outcome, no refusal.
    let side = arm(&mut session, 1, SymbolId(T_SIDE), 1);
    assert!(side.refusals.is_empty());
    assert_eq!(session.outcome(), None);

    // The optional reward is an intent, not an ending.
    let reward = arm(&mut session, 2, SymbolId(T_REWARD), 1);
    assert!(reward.outcome.is_none());
    assert_eq!(session.outcome(), None);

    // Failure is its own outcome, requested by the declared category of a
    // protected actor — not by "no enemies left".
    let lifecycles = [(PROTECTED, LifecycleKind::Destroyed)];
    let mut facts = input(3, 0);
    facts.lifecycles = &lifecycles;
    let failed = session.step(&facts).expect("a legal tick");
    assert_eq!(failed.outcome, Some(TerminalOutcome::Failure));
    assert!(failed.tick.events.iter().any(|event| matches!(
        event.kind,
        ObjectiveEventKind::OutcomeSettled {
            outcome: TerminalOutcome::Failure,
            ..
        }
    )));
}

#[test]
fn accept_f39_d_a_captured_actor_never_satisfies_a_destroyed_condition() {
    // The failure condition names one category. A *disabled* convoy is not a
    // destroyed one, so the mission must not end.
    let program = out_of_order_program();
    let mut session = launch(&program);

    let lifecycles = [(PROTECTED, LifecycleKind::OwnershipCaptured)];
    let mut facts = input(1, 0);
    facts.lifecycles = &lifecycles;
    let stepped = session.step(&facts).expect("a legal tick");
    assert_eq!(stepped.outcome, None);
    assert!(!session.runtime().is_settled());

    // Only the declared category ends it.
    let lifecycles = [(PROTECTED, LifecycleKind::Destroyed)];
    let mut facts = input(2, 0);
    facts.lifecycles = &lifecycles;
    let failed = session.step(&facts).expect("a legal tick");
    assert_eq!(failed.outcome, Some(TerminalOutcome::Failure));
}

// ---------------------------------------------------------------------------
// Dead branches are refused by name
// ---------------------------------------------------------------------------

/// A reveal rule waiting for the objective it reveals.
///
/// Returns the error `try_new` produced, so the caller can name the branch.
fn self_reveal_error(reveal: DeclaredRevealRule) -> ObjectivesSchemaError {
    let base = out_of_order_program();
    let mut objectives = base.objectives().to_vec();
    objectives[1].reveal = reveal;
    DeclaredObjectiveProgram::try_new(
        base.subject().clone(),
        base.origin().clone(),
        base.provenance().clone(),
        base.precedence().clone(),
        objectives,
        base.conditions().to_vec(),
        base.timers().to_vec(),
        base.triggers().to_vec(),
        base.spawn_groups().to_vec(),
    )
    .expect_err("a reveal rule that waits for the objective it reveals is dead")
}

#[test]
fn accept_f39_d_a_branch_that_can_never_fire_is_refused_by_name() {
    // The objective reveals itself by reaching `Succeeded`: the reveal rule is
    // the only thing that may leave `Hidden`, so the state change it waits for
    // can never happen first.
    assert_eq!(
        self_reveal_error(DeclaredRevealRule::OnObjectiveState {
            objective: ProgramSymbol(SECONDARY.0),
            state: DeclaredObjectiveState::Succeeded,
        }),
        ObjectivesSchemaError::DeadSelfReveal {
            objective: ProgramSymbol(SECONDARY.0),
        }
    );

    // The same rule reaching a state that is legal: still a self-watch, still
    // dead.
    assert_eq!(
        self_reveal_error(DeclaredRevealRule::OnObjectiveState {
            objective: ProgramSymbol(SECONDARY.0),
            state: DeclaredObjectiveState::Active,
        }),
        ObjectivesSchemaError::DeadSelfReveal {
            objective: ProgramSymbol(SECONDARY.0),
        }
    );

    // A watch on a state the watched objective already holds. The primary is
    // born `Active`, so a reveal rule waiting for it to become `Active` waits
    // for a state change no row can produce.
    let base = out_of_order_program();
    let mut objectives = base.objectives().to_vec();
    objectives[1].reveal = DeclaredRevealRule::OnObjectiveState {
        objective: ProgramSymbol(PRIMARY.0),
        state: DeclaredObjectiveState::Active,
    };
    assert_eq!(
        DeclaredObjectiveProgram::try_new(
            base.subject().clone(),
            base.origin().clone(),
            base.provenance().clone(),
            base.precedence().clone(),
            objectives,
            base.conditions().to_vec(),
            base.timers().to_vec(),
            base.triggers().to_vec(),
            base.spawn_groups().to_vec(),
        )
        .expect_err("a watch on the state a born objective holds is dead"),
        ObjectivesSchemaError::DeadWatch {
            by: ProgramSymbol(SECONDARY.0),
            objective: ProgramSymbol(PRIMARY.0),
            state: DeclaredObjectiveState::Active,
        }
    );

    // A *timer* armed by the same dead watch is refused too: a deadline that can
    // never arm is a program waiting on an event nothing produces.
    let base = out_of_order_program();
    let mut timers = base.timers().to_vec();
    timers[3].start = DeclaredTimerStart::OnObjectiveState {
        objective: ProgramSymbol(PRIMARY.0),
        state: DeclaredObjectiveState::Active,
    };
    assert_eq!(
        DeclaredObjectiveProgram::try_new(
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
        .expect_err("a deadline armed by a dead watch is dead"),
        ObjectivesSchemaError::DeadWatch {
            by: ProgramSymbol(T_PRIMARY),
            objective: ProgramSymbol(PRIMARY.0),
            state: DeclaredObjectiveState::Active,
        }
    );

    // And the refusals say why, by name.
    assert_eq!(
        ObjectivesSchemaError::DeadSelfReveal {
            objective: ProgramSymbol(SECONDARY.0)
        }
        .to_string(),
        "objective symbol(2) is revealed by its own state change, which it cannot make while hidden"
    );
}

/// Builds a program whose only change is one objective's initial state, and
/// returns the error `try_new` produced.
fn born_terminal_error(initial: DeclaredObjectiveState) -> ObjectivesSchemaError {
    let base = out_of_order_program();
    let mut objectives = base.objectives().to_vec();
    // The primary is the watched objective: it is the one this helper re-births.
    objectives[0].initial = initial;
    objectives[1].reveal = DeclaredRevealRule::OnObjectiveState {
        objective: ProgramSymbol(PRIMARY.0),
        state: DeclaredObjectiveState::Succeeded,
    };
    objectives[2].reveal = DeclaredRevealRule::Immediate;
    DeclaredObjectiveProgram::try_new(
        base.subject().clone(),
        base.origin().clone(),
        base.provenance().clone(),
        base.precedence().clone(),
        objectives,
        base.conditions().to_vec(),
        base.timers().to_vec(),
        base.triggers().to_vec(),
        base.spawn_groups().to_vec(),
    )
    .expect_err("no row leaves a final state, so the watch can never fire")
}

#[test]
fn accept_f39_d_a_watch_on_an_objective_born_finished_is_refused() {
    // The second way a watch is dead. The primary is born `Succeeded`, and no
    // row leaves a final state, so nothing can ever move it to the `Succeeded`
    // state the secondary's reveal rule waits for. Watching a state the
    // objective does not *already* hold is therefore not enough to be live.
    for born in [
        DeclaredObjectiveState::Succeeded,
        DeclaredObjectiveState::Failed,
        DeclaredObjectiveState::Superseded,
    ] {
        assert_eq!(
            born_terminal_error(born),
            ObjectivesSchemaError::DeadWatch {
                by: ProgramSymbol(SECONDARY.0),
                objective: ProgramSymbol(PRIMARY.0),
                state: DeclaredObjectiveState::Succeeded,
            },
            "an objective born {born:?} can never change state again"
        );
    }

    // The refusal is not "no watch is ever allowed": born in any state that has
    // a row leaving it, the same watch is accepted.
    for live in [
        DeclaredObjectiveState::Active,
        DeclaredObjectiveState::Optional,
        // `Pending` is born-hidden-below: an objective born `Pending` reaches
        // `Succeeded`, so the watch can fire.
        DeclaredObjectiveState::Pending,
    ] {
        let base = out_of_order_program();
        let mut objectives = base.objectives().to_vec();
        objectives[0].initial = live;
        objectives[0].reveal = DeclaredRevealRule::Immediate;
        objectives[1].reveal = DeclaredRevealRule::OnObjectiveState {
            objective: ProgramSymbol(PRIMARY.0),
            state: DeclaredObjectiveState::Succeeded,
        };
        DeclaredObjectiveProgram::try_new(
            base.subject().clone(),
            base.origin().clone(),
            base.provenance().clone(),
            base.precedence().clone(),
            objectives,
            base.conditions().to_vec(),
            base.timers().to_vec(),
            base.triggers().to_vec(),
            base.spawn_groups().to_vec(),
        )
        .unwrap_or_else(|error| panic!("an objective born {live:?} can reach Succeeded: {error}"));
    }
}

#[test]
fn accept_f39_d_a_mutually_watching_branch_still_fires() {
    // The other side of the dead-branch check, so the refusal cannot be widened
    // into "no watch is ever allowed": two objectives that watch each other
    // become `Active` are both live, because each can reach `Active`.
    let program = DeclaredObjectiveProgram::try_new(
        mission_id("synthetic.f39d.mutual"),
        Origin::SyntheticFixture,
        provenance("f39d.synthetic-mutual"),
        designed_precedence("f39d.synthetic-mutual"),
        vec![
            DeclaredObjective {
                symbol: ProgramSymbol(PRIMARY.0),
                content: objective_id("synthetic.f39d.first"),
                initial: DeclaredObjectiveState::Pending,
                reveal: DeclaredRevealRule::OnSignal(ProgramSymbol(REACHED_WRECK)),
                on_complete: cs_content::objectives::DeclaredCompletion::Continue,
                completion_effects: Vec::new(),
            },
            DeclaredObjective {
                symbol: ProgramSymbol(SECONDARY.0),
                content: objective_id("synthetic.f39d.second"),
                initial: DeclaredObjectiveState::Hidden,
                reveal: DeclaredRevealRule::OnObjectiveState {
                    objective: ProgramSymbol(PRIMARY.0),
                    state: DeclaredObjectiveState::Active,
                },
                on_complete: cs_content::objectives::DeclaredCompletion::Continue,
                completion_effects: Vec::new(),
            },
        ],
        vec![],
        vec![timer(
            ProgramSymbol(T_PRIMARY),
            DeclaredTimerAction::SetObjectiveState {
                objective: ProgramSymbol(PRIMARY.0),
                state: DeclaredObjectiveState::Active,
            },
        )],
        vec![],
        vec![],
    )
    .expect("two objectives may watch each other");
    let mut session = launch(&program);

    // The primary is revealed `Pending`; the secondary stays hidden until the
    // primary is activated.
    let signals = [SymbolId(REACHED_WRECK)];
    let mut facts = input(1, 0);
    facts.signals = &signals;
    session.step(&facts).expect("a legal tick");
    assert!(!session.display().row(SECONDARY).expect("tracked").revealed);

    let activated = arm(&mut session, 2, SymbolId(T_PRIMARY), 1);
    assert!(activated.refusals.is_empty());
    let second = session.display().row(SECONDARY).expect("tracked");
    assert!(second.revealed, "the watch fired on the state change");
    assert_eq!(second.state, ObjectiveState::Pending);
}

// ---------------------------------------------------------------------------
// The support gate
// ---------------------------------------------------------------------------

/// An installation-origin record over the same declarations, with `origin`
/// carrying a span over a container this project does not invent.
fn original_program() -> DeclaredObjectiveProgram {
    let base = out_of_order_program();
    let span = cs_types::asset_id::SourceSpan::new(
        ContentHash::from_hex(&"0".repeat(64)).expect("a 64-nibble hash"),
        "ZBD/C1/M02/zrdr.zbd",
        Some("objectives.zrd"),
        0,
        1,
        None,
    )
    .expect("a valid source span");
    DeclaredObjectiveProgram::try_new(
        base.subject().clone(),
        Origin::Installation { source: span },
        base.provenance().clone(),
        base.precedence().clone(),
        base.objectives().to_vec(),
        base.conditions().to_vec(),
        base.timers().to_vec(),
        base.triggers().to_vec(),
        base.spawn_groups().to_vec(),
    )
    .expect("an original-origin record validates its declarations")
}

#[test]
fn accept_f39_d_an_original_record_is_never_played_as_design() {
    // F39 AC04 speaks of *supported* objectives. This record's bytes came from
    // the owner's installation, so its branching, optional and failure
    // conditions are unknown and a designed progression must not stand in for
    // them: `lower_program` refuses by name, with the record's own reason.
    let program = original_program();
    assert!(!program.is_playable());
    assert_eq!(
        program.support().refusal(),
        Some(UNMEASURED_OBJECTIVE_SEMANTICS),
        "the record names why it may not run"
    );
    assert!(matches!(
        lower_program(&program).unwrap_err(),
        ProgramLowerError::UnsupportedProgram { .. }
    ));
    let refusal = lower_program(&program).unwrap_err().to_string();
    assert!(
        refusal.contains("no recovered objectives"),
        "the refusal names the mission: {refusal}"
    );

    // A newly authored record over the *same declarations* lowers and plays, so
    // the gate is about provenance and not about the content.
    let authored = out_of_order_program();
    assert!(authored.is_playable());
    assert_eq!(authored.support().refusal(), None);
    let mut session = launch(&authored);
    let stepped = arm(&mut session, 1, SymbolId(T_SIDE), 1);
    assert!(stepped.refusals.is_empty());
    assert_eq!(session.outcome(), None);
}

#[test]
fn accept_f39_d_a_measured_record_is_kept_and_a_bare_one_is_refused() {
    // The census measurement is data a refusal can carry, so a reader knows
    // *what was measured* and not only that something was not.
    let measured = MeasuredObjectiveRecord {
        container: "ZBD/C1/M02/zrdr.zbd".to_owned(),
        member: "objectives.zrd".to_owned(),
        sha256: "9f3077c6dccfad01297fbd27b7d591cc055d0674f1b967131558a0211e2114a0".to_owned(),
        byte_len: 12_410,
        blocks: 50,
        branching_sites: 42,
        optional_sites: 30,
        failure_sites: 2,
        // F39-E2 measured the per-block completion-effect reading as part of what
        // a census carries; this mission declares none of its blocks' effects in
        // the reading, which the default states explicitly.
        branch_precedence: MeasuredBranchPrecedence::default(),
    };
    let program = original_program()
        .with_measured_record(measured.clone())
        .expect("a measurement with an archive and a member is kept");
    match program.support() {
        cs_content::objectives::DeclaredSupport::Original { record, reason } => {
            assert_eq!(record.as_deref(), Some(&measured));
            assert_eq!(reason, UNMEASURED_OBJECTIVE_SEMANTICS);
        }
        other => panic!("an installation record keeps its original support: {other:?}"),
    }
    // Attaching the measurement does **not** make it playable: a declaration
    // site is not a rule.
    assert!(!program.is_playable());
    assert!(matches!(
        lower_program(&program).unwrap_err(),
        ProgramLowerError::UnsupportedProgram { .. }
    ));

    // A measurement of nothing is refused rather than attached.
    for bare in [
        MeasuredObjectiveRecord {
            container: String::new(),
            ..measured.clone()
        },
        MeasuredObjectiveRecord {
            member: "   ".to_owned(),
            ..measured.clone()
        },
    ] {
        assert_eq!(
            original_program()
                .with_measured_record(bare)
                .expect_err("an empty measurement measures nothing"),
            ObjectivesSchemaError::EmptyMeasurement
        );
    }

    // A newly authored record stays `Authored` even with a measurement attached,
    // so attaching one cannot promote design into an original claim.
    let authored = out_of_order_program()
        .with_measured_record(measured)
        .expect("the measurement is accepted");
    assert_eq!(
        authored.support(),
        &cs_content::objectives::DeclaredSupport::Authored
    );
}

// ---------------------------------------------------------------------------
// Retail: the census over the owner's installation
// ---------------------------------------------------------------------------

/// Whether a census mission label names a **campaign** mission.
///
/// F14-D.1's own directory-name rule: a leaf named `m<nn>` is a campaign
/// mission, an `ia<n>` leaf an instant-action scenario and an `mp<n>` leaf a
/// multiplayer scenario. Only the campaign row is asserted to declare branching
/// sites, because only the campaign is the content F39's objectives govern.
fn is_campaign_mission(mission: &str) -> bool {
    let Some(leaf) = mission.rsplit('/').next() else {
        return false;
    };
    let Some(rest) = leaf.strip_prefix('m') else {
        return false;
    };
    !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit())
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f39_d_retail_objective_records_declare_branching_outcomes_and_optionality() {
    let game_dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR names the read-only installation"),
    );
    let census = survey_retail_objective_records(&game_dir)
        .expect("the mission-scoped objective records survey");

    // The denominator: every mission-scoped reader archive was measured, and
    // each one resolved its `objectives.zrd` member. A mission missing from the
    // set would be a mission whose record was silently skipped.
    assert!(
        census.len() >= 24,
        "the campaign's mission readers are missing: {}",
        census.len()
    );
    assert!(
        census.blocks() > 1000,
        "the measured objective blocks collapsed: {}",
        census.blocks()
    );
    assert_eq!(census.install_sha256().len(), 64);

    // The three families F39 names are all declared by the original's own
    // records. A `false` here would mean the census changed shape, not that the
    // original has no such rule.
    assert!(
        census.declares_branching(),
        "no measured objective block declares a branching site"
    );
    assert!(
        census.declares_optionality(),
        "no measured objective block declares an optionality site"
    );
    assert!(
        census.declares_outcome(),
        "no measured objective block declares an outcome site"
    );
    // Every campaign mission declares branching. A mission directory named
    // `m<nn>` is a campaign mission — the name rule F14-D.1 measured — and the
    // leaf is what separates it from an `ia<n>` scenario or an `mp<n>`
    // multiplayer scenario, which the census also measures.
    let campaign: Vec<&cs_app::objectives::RetailObjectiveRow> = census
        .rows()
        .iter()
        .filter(|row| is_campaign_mission(&row.mission))
        .collect();
    assert_eq!(
        campaign.len(),
        24,
        "the campaign's mission readers moved: {:?}",
        campaign
            .iter()
            .map(|row| row.mission.as_str())
            .collect::<Vec<_>>()
    );
    for row in &campaign {
        assert!(
            row.branching_sites > 0,
            "{} declares no branching site",
            row.mission
        );
        assert!(
            row.blocks > 0,
            "{} declares no objective block",
            row.mission
        );
    }

    // Every site count is accounted for by the published vocabulary: a site the
    // vocabulary cannot name would be a silent hole in the census.
    let vocabulary = census.vocabulary();
    let counted: u32 = census
        .vocabulary()
        .iter()
        .filter(|(key, _)| {
            BRANCH_KEY_VOCABULARY.contains(&key.as_str())
                || FAILURE_KEY_VOCABULARY.contains(&key.as_str())
                || is_optional_objective_key(key)
        })
        .map(|(_, count)| count)
        .sum();
    assert_eq!(
        counted,
        census.branching_sites() + census.failure_sites() + census.optional_sites(),
        "the published vocabulary does not explain the site totals"
    );
    assert!(
        vocabulary
            .iter()
            .any(|(key, _)| key == "WAKE_OBJECTIVE_WHEN_I_COMPLETE"),
        "the measured branching vocabulary is missing its largest member: {vocabulary:?}"
    );

    // `is_optional_objective_key` admits `INACTIVE` plus *any* stage number, so
    // the measured range is pinned here: a key outside `INACTIVE1`…
    // `INACTIVE18` that matches the rule would be counted as optionality
    // without ever having been measured, and this is where that shows up.
    for (key, _) in &vocabulary {
        if !is_optional_objective_key(key) {
            continue;
        }
        let measured = *key == OBJECTIVE_INACTIVE_COUNT_KEY
            || key
                .strip_prefix(OBJECTIVE_INACTIVE_STAGE_PREFIX)
                .and_then(|stage| stage.parse::<u32>().ok())
                .is_some_and(|stage| (1..=MEASURED_INACTIVE_STAGES).contains(&stage));
        assert!(
            measured,
            "{key} matches the optionality rule but is outside the measured \
             {OBJECTIVE_INACTIVE_STAGE_PREFIX}1..{MEASURED_INACTIVE_STAGES} range"
        );
    }

    // The census's two path fields describe the same archive: `container` is the
    // installation's own spelling and `mission` its lowercase logical scope, so
    // the two must agree component for component.
    for row in census.rows() {
        let logical = cs_types::install::RelativePath::new(&row.container)
            .expect("the census spells a relative path")
            .logical_key();
        assert_eq!(
            logical,
            format!("{}/zrdr.zbd", row.mission),
            "{}: the container and the mission scope disagree",
            row.mission
        );
    }

    // The gate's verdict follows from the census: declarations were read, no
    // rule was recovered, so an original record still may not be played.
    let row = census
        .row("zbd/c1/m02")
        .expect("mission zbd/c1/m02 is in the census");
    let record = row.measured();
    assert!(record.branching_sites > 0);
    assert!(record.blocks > 0);
    assert_eq!(record.byte_len, row.member_len);
    assert_eq!(record.sha256, row.member_sha256);
    assert_eq!(record.container, row.container);
}
