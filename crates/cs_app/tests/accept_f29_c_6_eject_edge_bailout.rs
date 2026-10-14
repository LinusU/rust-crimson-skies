//! Acceptance scenario F29-C.6: the mission host's eject edge calls
//! `confirm_bailout`.
//!
//! Spec: `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
//! stage `### F39-C`, coordinating with F29-C.4's transition
//! (`specs/F29-damage-zones-armor-destruction-and-bailout.md`, acceptance case
//! **AC04**). Shared contracts: `docs/contracts/SCRIPT-MISSION.md` ("Objective
//! event ordering") and `docs/contracts/FLIGHT-PHYSICS.md` ("Commands belong to
//! one simulation tick and actor generation"). Task test prefix:
//! `accept_f29_c_6_`.
//!
//! `crates/cs_sim/tests/accept_f29_c_04_bailout_mission_transition.rs` pins the
//! ledger, and `accept_f29_c_04_bailout_session_consumer.rs` pins what a
//! launched session does with an **unconfirmed** bailout. This file pins the
//! producer surface F29-C.6 added: the input boundary records one
//! [`EjectConfirmation`] per delivered `FlightCommand::Eject` edge —
//! tick-qualified, stamped with the context that owned the devices — and
//! [`ObjectiveSession::confirm_bailout`] is the mutable path that registers it
//! with the runtime's gate before the tick that reports the lifecycle fact.
//! Remove either half and the tests below fail: no record means nothing to
//! register, no mutable session path means nothing registers it.
//!
//! Identity note: the mission `cs_script::ir::ActorId` each confirmation is
//! registered for is supplied by the caller — the world's
//! `cs_types::net::ActorId` → mission `ActorId` bridge is the host's surface
//! (`cs_sim::mission` says so in the same words), so this test names the actor
//! explicitly rather than exercising a mapping production code does not have.
//!
//! Every value here is newly authored synthetic fixture data, and the eject
//! binding is this project's **designed** default (`Key::Digit3` →
//! `FlightCommand::Eject`): the original's bindings are F22-D's measurement,
//! not this task's, so nothing here claims the original bound this key.

use cs_app::input::{DeviceEvent, EjectConfirmation, FrameInput, InputSession, SessionMode};
use cs_app::objectives::{LoweredObjectives, ObjectiveSession, SessionRefusal, lower_program};
use cs_content::objectives::declared_synthetic_objectives;
use cs_script::ir::{ActorId, SymbolId};
use cs_script::runtime::SessionGeneration;
use cs_sim::control::{ControlBuffer, LocalSeatId};
use cs_sim::damage::LifecycleKind;
use cs_sim::objectives::bailout::{BailoutConfirmation, BailoutRefusal, MissionTransition};
use cs_sim::objectives::counters::CountKind;
use cs_sim::objectives::runtime::{
    ACTOR_EVENT_SOURCE, ObjectiveEventKind, SPAWNED_ACTOR_BASE, TickInput,
};
use cs_sim::objectives::timer::TimerRequest;
use cs_types::Tick;
use cs_types::input::{
    Action, DeviceClass, DeviceId, FlightCommand, InputContext, InputFrame, Key,
};

const GEN1: SessionGeneration = SessionGeneration(1);

/// The fixture's first spawn-wave timer.
const WAVE_1: SymbolId = SymbolId(20);
/// The two raiders wave 1 admits: the first two ids of the
/// F39-E-ACTORID-SPACE spawned band.
const RAIDER_ONE: ActorId = ActorId(SPAWNED_ACTOR_BASE);
const RAIDER_TWO: ActorId = ActorId(SPAWNED_ACTOR_BASE + 1);

/// The designed eject edge: `ActionMap::designed_default` binds `Digit3` to
/// `FlightCommand::Eject`.
const EJECT: Action = Action::Flight(FlightCommand::Eject);

fn lowered() -> LoweredObjectives {
    lower_program(&declared_synthetic_objectives()).expect("the fixture lowers")
}

fn launch() -> ObjectiveSession {
    ObjectiveSession::launch(lowered(), GEN1).expect("the lowered program launches")
}

/// The fixture keyboard, with the stable identity a real platform reports.
fn keyboard() -> DeviceId {
    DeviceId::stable(DeviceClass::Keyboard, "kbd.eject/0").expect("the fixture identity is valid")
}

/// An input session with the keyboard connected, exactly as a platform's
/// device-enumeration event would leave it.
fn input_session() -> InputSession {
    let mut session =
        InputSession::designed_default(LocalSeatId(0), SessionMode::SinglePlayer, Tick(0));
    session
        .collector_mut()
        .connect_device(keyboard())
        .expect("the fixture device connects");
    session
}

/// The keyboard's report for the keys that are down.
fn keys(down: &[Key]) -> Vec<DeviceEvent> {
    vec![DeviceEvent::KeyboardFrame {
        device: keyboard(),
        keys: down.to_vec(),
    }]
}

/// An input for objective `tick` with no facts; the caller fills the fact
/// slices before `step` borrows it.
fn facts<'a>(tick: u64, committed: u64) -> TickInput<'a> {
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
    let mut input = facts(tick, committed);
    input.timer_requests = &arm;
    let stepped = session.step(&input).expect("a legal tick");
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

fn bailed_out(
    tick: &cs_app::objectives::SessionTick,
    actor: ActorId,
) -> Option<BailoutConfirmation> {
    tick.tick.events.iter().find_map(|event| match &event.kind {
        ObjectiveEventKind::PilotBailedOut {
            actor: reported,
            confirmation,
        } if *reported == actor => Some(*confirmation),
        _ => None,
    })
}

/// The discriminating case: a real eject press through the production input
/// path is the confirmation a `PilotBailout` lifecycle fact needs — one
/// record per press, delivered at the boundary that ran it, registered
/// before the tick that reports the bailout, and a repeat press is refused
/// by name rather than silently re-confirming.
#[test]
fn accept_f29_c_6_a_delivered_eject_edge_confirms_the_bailout() {
    let mut input = input_session();
    let mut session = launch();

    // Wave 1 admits two raiders, which the session now owns for teardown.
    arm_wave_one(&mut session, 1, 1);
    assert_eq!(session.live_actors(), vec![RAIDER_ONE, RAIDER_TWO]);

    // One press, one frame, two input boundaries: `begin_tick` delivers the
    // edge at the first boundary only, so one press is one edge and one
    // confirmation record — never one per boundary the frame happened to run.
    let pressed = input
        .pump_frame(FrameInput::Devices(&keys(&[Key::Digit3])), 2)
        .expect("the fixture frame applies");
    assert_eq!(pressed.ticks_ran, 2);
    assert_eq!(
        pressed.delivered,
        vec![EJECT],
        "one press delivers the eject edge exactly once"
    );
    let records = input.take_eject_confirmations();
    assert_eq!(
        records,
        vec![EjectConfirmation {
            tick: Tick(0),
            confirmation: BailoutConfirmation::PlayerEject {
                context: InputContext::Flight
            },
        }],
        "the record is tick-qualified and carries the context the press was \
         made under"
    );
    assert!(
        input.take_eject_confirmations().is_empty(),
        "a taken record is gone: the handoff is once per press, not per read"
    );

    // A second press — release, then press again — produces a second record
    // at its own boundary tick.
    input
        .pump_frame(FrameInput::Devices(&keys(&[])), 1)
        .expect("the release frame applies");
    let pressed_again = input
        .pump_frame(FrameInput::Devices(&keys(&[Key::Digit3])), 1)
        .expect("the second press applies");
    assert_eq!(pressed_again.delivered, vec![EJECT]);
    let second = input.take_eject_confirmations();
    assert_eq!(second.len(), 1, "the second press is a second record");
    assert_eq!(second[0].tick, Tick(3), "at the boundary that delivered it");

    // The host registers the confirmation *before* the tick that reports the
    // lifecycle fact, with the mission `ActorId` the runtime is keyed by.
    session
        .confirm_bailout(RAIDER_ONE, records[0].confirmation)
        .expect("a flight-context eject confirms");
    // While that confirmation still stands, a second registration for the
    // same actor is refused by name — the per-frame caller's signature: it
    // surfaces refusals rather than re-confirming.
    assert_eq!(
        session.confirm_bailout(RAIDER_ONE, second[0].confirmation),
        Err(BailoutRefusal::AlreadyConfirmed { actor: RAIDER_ONE }),
        "a repeat confirmation is refused, not silently absorbed"
    );

    // The tick that reports the pilot bailout: the transition applies with
    // the first press's confirmation, and the refused repeat is named on the
    // same tick report — a refusal is a queryable fact, not a trace line.
    let lifecycles = [(RAIDER_ONE, LifecycleKind::PilotBailout)];
    let mut bailout = facts(2, 0);
    bailout.lifecycles = &lifecycles;
    let stepped = session.step(&bailout).expect("a legal tick");

    assert_eq!(
        bailed_out(&stepped, RAIDER_ONE),
        Some(records[0].confirmation),
        "the confirmed bailout is the transition the stream reports, and it \
         carries the confirmation the real input frame produced"
    );
    assert_eq!(
        stepped.refusals,
        vec![SessionRefusal::Transition {
            reason: BailoutRefusal::AlreadyConfirmed { actor: RAIDER_ONE }
        }],
        "the second press's refused registration is reported on the tick"
    );
    assert_eq!(
        session.runtime().mission_transition(RAIDER_ONE),
        Some(MissionTransition::PilotBailedOut)
    );
    assert!(
        !counted(&stepped, RAIDER_ONE, CountKind::Destroyed),
        "a bailout is not a kill"
    );
    assert!(
        session.live_actors().contains(&RAIDER_ONE),
        "the airframe is still in the world, so it stays this session's to \
         tear down"
    );
    assert_eq!(
        stepped.outcome, None,
        "the unmeasured bailout policy grants no result"
    );
}

/// The same edge under a context that owns no flight commands: the live
/// session drops the press before the buffer, and a confirmation that still
/// arrives stamped `Cinematic` is refused by the gate — by name, on the tick
/// report — while the bailout the tick reports stays unconfirmed.
#[test]
fn accept_f29_c_6_a_cinematic_stamped_eject_is_refused_by_name() {
    let mut input = input_session();
    let mut session = launch();

    arm_wave_one(&mut session, 1, 1);
    assert_eq!(session.live_actors(), vec![RAIDER_ONE, RAIDER_TWO]);

    // With the cinematic context owning the devices, the press never reaches
    // the control buffer at all: the report is dropped unread, nothing is
    // delivered and no confirmation record exists to register.
    input.set_context(InputContext::Cinematic);
    let quiet = input
        .pump_frame(FrameInput::Devices(&keys(&[Key::Digit3])), 1)
        .expect("the fixture frame applies");
    assert_eq!(quiet.dropped_events, 1, "the report is dropped, not read");
    assert!(quiet.delivered.is_empty());
    assert!(
        input.take_eject_confirmations().is_empty(),
        "a cinematic context cannot produce an eject confirmation"
    );
    input.set_context(InputContext::Flight);

    // A caller that stamps the same real edge `Cinematic` anyway — the
    // struct-literal case the gate's re-check exists for — is refused where
    // the confirmation is registered. The edge itself is honest: a real
    // `ControlBuffer` delivers it at the boundary.
    let mut buffer = ControlBuffer::new();
    let mut frame = InputFrame::new(Tick(0));
    frame.push_edge(EJECT);
    buffer.apply_frame(&frame).expect("the frame applies");
    assert_eq!(
        buffer.begin_tick(Tick(0)),
        vec![EJECT],
        "the same edge, delivered by a real control buffer"
    );
    assert_eq!(
        BailoutConfirmation::from_action(EJECT, InputContext::Cinematic),
        Err(BailoutRefusal::InputContextRefused {
            context: InputContext::Cinematic
        }),
        "the producer-side constructor refuses it too"
    );
    assert_eq!(
        session.confirm_bailout(
            RAIDER_ONE,
            BailoutConfirmation::PlayerEject {
                context: InputContext::Cinematic
            }
        ),
        Err(BailoutRefusal::InputContextRefused {
            context: InputContext::Cinematic
        }),
        "the gate refuses the cinematic-stamped confirmation by name"
    );

    // The refusal is a queryable session fact: the tick report names the
    // refused registration first, then the bailout the same lifecycle fact
    // reports stays unconfirmed — the refused registration latched nothing.
    let lifecycles = [(RAIDER_ONE, LifecycleKind::PilotBailout)];
    let mut bailout = facts(2, 0);
    bailout.lifecycles = &lifecycles;
    let stepped = session.step(&bailout).expect("a legal tick");

    assert_eq!(
        stepped.refusals,
        vec![
            SessionRefusal::Transition {
                reason: BailoutRefusal::InputContextRefused {
                    context: InputContext::Cinematic
                }
            },
            SessionRefusal::Transition {
                reason: BailoutRefusal::Unconfirmed { actor: RAIDER_ONE }
            },
        ],
        "the refused registration is named on the tick, and the bailout it \
         could not confirm is refused as unconfirmed"
    );
    assert_eq!(bailed_out(&stepped, RAIDER_ONE), None);
    assert_eq!(
        session.runtime().mission_transition(RAIDER_ONE),
        None,
        "a refused confirmation latches nothing"
    );
    assert!(
        session.live_actors().contains(&RAIDER_ONE),
        "and the airframe is still the session's to tear down"
    );
}

/// The same session path also registers a mission program's own
/// confirmation: `Scripted` needs no local input, so the input side is not
/// consulted at all — this is the half that keeps `confirm_bailout` an
/// honest session surface rather than an input-path special case.
#[test]
fn accept_f29_c_6_a_scripted_confirmation_needs_no_input_edge() {
    let mut session = launch();
    arm_wave_one(&mut session, 1, 1);

    // The mission program asks for the pilot to leave under its own name.
    let confirmation = BailoutConfirmation::scripted(SymbolId(9));
    session
        .confirm_bailout(RAIDER_ONE, confirmation)
        .expect("a scripted confirmation is not an input edge");

    let lifecycles = [(RAIDER_ONE, LifecycleKind::PilotBailout)];
    let mut bailout = facts(2, 0);
    bailout.lifecycles = &lifecycles;
    let stepped = session.step(&bailout).expect("a legal tick");

    assert_eq!(
        bailed_out(&stepped, RAIDER_ONE),
        Some(confirmation),
        "the scripted request confirms the bailout the stream reports"
    );
    assert!(stepped.refusals.is_empty(), "{:?}", stepped.refusals);
}
