//! Acceptance scenario F29-C.4 at the **session consumer**: the mission host
//! sees the pilot bailout's transition, and a refusal reaches a consumer that
//! does not re-walk the event stream.
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-C` (task F29-C.4), acceptance case **AC04** with non-negotiable
//! behaviors 3 and 4. Shared contract: `docs/contracts/SCRIPT-MISSION.md`.
//! Task test prefix: `accept_f29_c_04_`.
//!
//! `crates/cs_sim/tests/accept_f29_c_04_bailout_mission_transition.rs` pins the
//! transition decision itself. This file pins what the **wired session** does
//! with the resulting stream, because that dispatch is production code of its
//! own: [`cs_app::objectives::ObjectiveSession::step`] must name a refused
//! transition as [`SessionRefusal::Transition`] rather than dropping it, and it
//! must keep an airframe that only *failed* to transition in the wave registry,
//! because the world still owns it. Remove either dispatch arm and the test
//! below fails.
//!
//! The case this file pins is the **honest current state**, not the intended
//! one: no host registers a bailout confirmation yet (F29-C.6 / Rally #674 owns
//! the producer that calls `ObjectiveRuntime::confirm_bailout`, and
//! `ObjectiveSession` exposes no mutable path to its runtime at all), so every
//! bailout reported to a launched session is refused as unconfirmed. That is
//! the refusal working, not a silent pass — and it must not shield the actor:
//! a real destruction afterwards still counts as a kill.
//!
//! Every value here is newly authored synthetic fixture data. **No rule here is
//! an original-fidelity claim**: the mission-result policy a bailout should
//! apply is unmeasured
//! (`docs/findings/2026-10-05-f29-c-4-bailout-mission-transition.md`), so it
//! grants nothing and this test asserts the withholding is structural.

use cs_app::objectives::{LoweredObjectives, ObjectiveSession, SessionRefusal, lower_program};
use cs_content::objectives::declared_synthetic_objectives;
use cs_script::ir::{ActorId, SymbolId};
use cs_script::runtime::SessionGeneration;
use cs_sim::damage::LifecycleKind;
use cs_sim::objectives::bailout::{BailoutRefusal, MissionTransition};
use cs_sim::objectives::counters::CountKind;
use cs_sim::objectives::runtime::{ACTOR_EVENT_SOURCE, ObjectiveEventKind, TickInput};
use cs_sim::objectives::timer::TimerRequest;
use cs_types::Tick;

const GEN1: SessionGeneration = SessionGeneration(1);

/// The fixture's first spawn-wave timer.
const WAVE_1: SymbolId = SymbolId(20);
/// The two raiders wave 1 admits.
const RAIDER_ONE: ActorId = ActorId(1);
const RAIDER_TWO: ActorId = ActorId(2);

fn lowered() -> LoweredObjectives {
    lower_program(&declared_synthetic_objectives()).expect("the fixture lowers")
}

fn launch() -> ObjectiveSession {
    ObjectiveSession::launch(lowered(), GEN1).expect("the lowered program launches")
}

/// An input for `tick` with no facts; the caller fills the fact slices before
/// `step` borrows it.
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

/// Arms wave 1 on `tick`, committing `committed` whole committed ticks so the
/// deadline runs out and admits the wave.
fn arm_wave_one(session: &mut ObjectiveSession, tick: u64, committed: u64) {
    let arm = [TimerRequest::Arm(WAVE_1)];
    let mut facts = input(tick, committed);
    facts.timer_requests = &arm;
    let stepped = session.step(&facts).expect("a legal tick");
    assert!(stepped.stop.is_none(), "{:?}", stepped.stop);
}

fn counted(tick: &cs_app::objectives::SessionTick, actor: ActorId, kind: CountKind) -> bool {
    tick.tick.events.iter().any(|event| {
        event.source() == ACTOR_EVENT_SOURCE
            && matches!(
                event.kind,
                ObjectiveEventKind::Counted {
                    actor: reported,
                    kind: counted,
                } if reported == actor && counted == kind
            )
    })
}

#[test]
fn accept_f29_c_04_the_session_names_a_refused_bailout_and_keeps_the_airframe() {
    let mut session = launch();

    // Wave 1 admits two raiders, which the session now owns for teardown.
    arm_wave_one(&mut session, 1, 1);
    assert_eq!(session.live_actors(), vec![RAIDER_ONE, RAIDER_TWO]);

    // A pilot bails out of one of them. Nothing can confirm it through the
    // session yet (F29-C.6 / #674 owns that producer), so the transition is
    // refused by name on the stream *and* in the refusal report.
    let lifecycles = [(RAIDER_ONE, LifecycleKind::PilotBailout)];
    let mut facts = input(2, 0);
    facts.lifecycles = &lifecycles;
    let refused = session.step(&facts).expect("a legal tick");

    assert_eq!(
        refused.refusals,
        vec![SessionRefusal::Transition {
            reason: BailoutRefusal::Unconfirmed { actor: RAIDER_ONE }
        }],
        "a refused transition is reported, not dropped: a consumer that reads \
         only the refusals still learns the pilot left with nothing to confirm it"
    );
    assert!(
        refused
            .tick
            .events
            .iter()
            .all(|event| !matches!(event.kind, ObjectiveEventKind::PilotBailedOut { .. })),
        "and no bailout transition is claimed on the stream either"
    );
    assert!(
        !counted(&refused, RAIDER_ONE, CountKind::Destroyed),
        "a bailout is not a kill, refused or not"
    );
    assert_eq!(
        session.runtime().mission_transition(RAIDER_ONE),
        None,
        "a refused transition latches nothing"
    );
    assert_eq!(
        session.live_actors(),
        vec![RAIDER_ONE, RAIDER_TWO],
        "the airframe is still in the world, so it stays this session's to tear down"
    );
    assert_eq!(
        refused.outcome, None,
        "a bailout settles nothing: the declared policy withholds every result"
    );

    // The refusal must not shield the actor either: the transition record is
    // still empty, so a real destruction afterwards is the transition this
    // actor reaches and it counts once.
    let lifecycles = [(RAIDER_ONE, LifecycleKind::Destroyed)];
    let mut facts = input(3, 0);
    facts.lifecycles = &lifecycles;
    let killed = session.step(&facts).expect("a legal tick");

    assert!(
        killed.refusals.is_empty(),
        "an ordinary destruction after a refused bailout is not a refusal: {:?}",
        killed.refusals
    );
    assert!(
        counted(&killed, RAIDER_ONE, CountKind::Destroyed),
        "the kill counts"
    );
    assert_eq!(session.runtime().counted(CountKind::Destroyed), 1);
    assert_eq!(
        session.runtime().mission_transition(RAIDER_ONE),
        Some(MissionTransition::Destroyed),
        "and it is the transition this actor reached"
    );
    assert_eq!(
        session.live_actors(),
        vec![RAIDER_TWO],
        "a gone category releases the wreck from the live registry"
    );
    assert_eq!(
        killed.outcome, None,
        "this fixture's declared failure roster is the protected convoy actor, \
         which this test never touches"
    );
}
