//! Acceptance scenarios for F22-C: focus, UI, control ownership and replay.
//!
//! The sheet's minimum scenario for this stage is **AC03: "Replay the same
//! quantized command stream at different display FPS"**, which
//! `accept_f22_c_replay_the_same_quantized_command_stream_at_different_display_fps`
//! covers end to end. The other tests cover the rest of the stage's wiring:
//! non-negotiable behavior 4 (a focus loss pauses single-player and only
//! neutralizes multiplayer), behavior 5 (text entry and UI navigation cannot
//! also fire weapons), the single-authority rule of
//! `docs/contracts/UI-NETWORK.md` ("Network ownership table"), and the
//! teardown/retry path.
//!
//! Every test here drives production code the way a render loop would and
//! nothing else:
//!
//! * `cs_app::input::InputSession` is the loop: one `pump_frame` per render
//!   frame, with the `ticks` a real `cs_sim::time::SimClock` committed from that
//!   frame's wall time.
//! * `cs_app::input::DeviceEvent`s are the producer, the session's own
//!   `cs_sim::control` buffer/gate/throttle are the consumer, and
//!   `cs_sim::time::SimClock` is the clock both the live run and the replay use.
//! * The recorded command stream is a `cs_types::input::CommandStream` the
//!   session produced at its input boundaries, and the replay feeds it back
//!   through the same `pump_frame`.
//!
//! Nothing in this file re-implements the input pipeline, and the only
//! test-side code is a tiny consumer that turns the delivered commands into an
//! observable world trace, so "the same commands were executed" is checked as
//! "the same world came out of them" rather than by comparing the pipeline with
//! itself.
//!
//! Every device identity, binding, script, tick and threshold here is newly
//! authored development data, not measured original game data. No test in this
//! file needs the original installation.

use std::time::Duration;

use cs_app::input::{
    CommandReplay, ControlHandover, DeviceEvent, FrameInput, HandoverReason, InputFault,
    InputSession, PauseDecision, PauseReason, ReplayError, SessionError, SessionMode,
    SuppressReason, UiRequest,
};
use cs_sim::control::{ControlAuthority, LocalSeatId, ThrottleSteps};
use cs_sim::time::{ClockPolicy, NANOS_PER_SECOND, SimClock, TickRate};
use cs_types::Tick;
use cs_types::input::{
    Action, CommandStream, DeviceClass, DeviceId, FlightCommand, InputContext, Key, UiAction,
};

/// The fixed simulation rate of every fixture: 64 Hz.
const TICK_HZ: u32 = 64;
/// One second of wall time at [`TICK_HZ`], in ticks.
const TICKS_PER_SECOND: u64 = TICK_HZ as u64;
/// The display rates the replay is compared at: a coarse one, the reference one
/// and a fine one.
const DISPLAY_RATES: [u32; 3] = [30, 60, 144];
/// The reference recording's display rate, the finest sampling compared.
const REFERENCE_FPS: u32 = 144;

/// The pilot's seat.
fn seat() -> LocalSeatId {
    LocalSeatId(0)
}

/// The fixture stick, with the stable identity a real platform reports.
fn stick() -> DeviceId {
    DeviceId::stable(DeviceClass::Joystick, "joy.fixture/0").expect("the fixture identity is valid")
}

/// The fixture keyboard.
fn keyboard() -> DeviceId {
    DeviceId::stable(DeviceClass::Keyboard, "kbd.fixture/0").expect("the fixture identity is valid")
}

/// The fixture gamepad.
fn gamepad() -> DeviceId {
    DeviceId::stable(DeviceClass::Gamepad, "pad.fixture/0").expect("the fixture identity is valid")
}

/// One second of wall time, split into `frame_rate` render frames: `frame_rate`
/// equal spans plus a final remainder frame, so the sum is exactly one second
/// at any rate.
fn frame_spans(frame_rate: u32) -> Vec<Duration> {
    let frames = u128::from(frame_rate);
    let per_frame = NANOS_PER_SECOND / frames;
    let rest = NANOS_PER_SECOND - per_frame * frames;
    let mut spans =
        vec![
            Duration::from_nanos(u64::try_from(per_frame).expect("a frame is under a second"));
            frame_rate as usize
        ];
    spans.push(Duration::from_nanos(
        u64::try_from(rest).expect("the remainder frame is under a second"),
    ));
    spans
}

/// A clock at the fixture's fixed rate, starting at `tick`.
fn clock(tick: Tick) -> SimClock {
    SimClock::with_tick(
        SessionMode::SinglePlayer.clock_policy(),
        TickRate::new(TICK_HZ).expect("64 Hz is a valid rate"),
        tick,
    )
}

/// A session with the fixture's devices connected, as a platform's
/// device-enumeration events would leave it.
fn session(mode: SessionMode) -> InputSession {
    let mut session = InputSession::designed_default(seat(), mode, Tick(0));
    for device in [stick(), keyboard(), gamepad()] {
        session
            .collector_mut()
            .connect_device(device)
            .expect("the fixture device connects");
    }
    session
}

/// The keyboard's report for the keys that are down.
fn keys(down: &[Key]) -> Vec<DeviceEvent> {
    vec![DeviceEvent::KeyboardFrame {
        device: keyboard(),
        keys: down.to_vec(),
    }]
}

/// The stick's report.
fn stick_report(buttons: &[u16], axes: &[(u16, f32)]) -> Vec<DeviceEvent> {
    vec![DeviceEvent::JoystickFrame {
        device: stick(),
        buttons: buttons.to_vec(),
        axes: axes.to_vec(),
    }]
}

fn fire() -> Action {
    Action::Flight(FlightCommand::FirePrimary)
}

/// A test-side consumer of the production command stream: it turns the commands
/// a tick executed into an observable world trace, so two runs can be compared
/// on what they *did* rather than on the pipeline that produced them.
///
/// This is deliberately not a flight model: it integrates each axis sample
/// into a position-like accumulator and counts the one-shot commands, which is
/// enough for a difference in the command stream to show up as a difference in
/// the world.
#[derive(Clone, Debug, Default, PartialEq)]
struct WorldTrace {
    /// The axes' accumulated deflection, per command, in the order first driven.
    deflection: Vec<(FlightCommand, f32)>,
    /// How many times each one-shot command executed.
    fired: Vec<(FlightCommand, u32)>,
    /// The throttle the keyboard steps moved to, in design steps.
    throttle_steps: u32,
    /// How many ticks ran.
    ticks: u64,
    /// How many UI actions reached the consumer. A UI action is a request for
    /// the screen path and must never be executed by the simulation, so this is
    /// zero in every run and a non-zero value fails the comparison below.
    ui_actions: u32,
}

impl WorldTrace {
    /// Folds one tick's executed commands into the trace.
    fn tick(&mut self, edges: &[Action], axes: &[(FlightCommand, f32)]) {
        self.ticks += 1;
        for edge in edges {
            match edge {
                Action::Flight(FlightCommand::ThrottleStepUp) => self.throttle_steps += 1,
                Action::Flight(FlightCommand::ThrottleStepDown) => {
                    self.throttle_steps = self.throttle_steps.saturating_sub(1)
                }
                Action::Flight(command) => {
                    let count = self
                        .fired
                        .iter_mut()
                        .find(|(known, _)| known == command)
                        .map(|(_, count)| count);
                    match count {
                        Some(count) => *count += 1,
                        None => self.fired.push((*command, 1)),
                    }
                }
                Action::Ui(_) => self.ui_actions += 1,
            }
        }
        for (command, value) in axes {
            match self
                .deflection
                .iter_mut()
                .find(|(known, _)| known == command)
            {
                Some((_, held)) => *held += value,
                None => self.deflection.push((*command, *value)),
            }
        }
    }
}

/// Whether an executed action was a flight command. A UI action reaching the
/// consumer is a failure the caller must not have to detect by hand.
fn action_is_flight(action: &Action) -> bool {
    matches!(action, Action::Flight(_))
}

/// One run's recorded stream and the world it produced.
#[derive(Debug)]
struct Run {
    /// The command stream the session recorded at its input boundaries.
    stream: CommandStream,
    /// The world the recording's consumer trace produces.
    world: WorldTrace,
    /// The throttle position the session's own throttle reached.
    throttle: f32,
    /// The final deflection of every axis the buffer holds.
    axes: Vec<(FlightCommand, f32)>,
    /// The render frames the run pumped.
    frames: u64,
    /// The tick each recorded edge executed on, in order.
    tick_of_edge: Vec<Tick>,
}

/// The pilot script of the fixtures, as the keys that are down at `tick`.
///
/// A pitch key held from the fourth tick to the end of the run, one press of
/// the trigger in the middle, and two throttle steps: continuous axes, a
/// one-shot edge and a step, which is what a replay has to keep apart.
fn pilot_keys(tick: u64) -> Vec<Key> {
    let mut down = Vec::new();
    if tick >= 4 {
        down.push(Key::S);
    }
    if tick == 12 {
        down.push(Key::Space);
    }
    if tick == 20 || tick == 30 {
        down.push(Key::R);
    }
    down
}

/// Records one second of the pilot script at `frame_rate` through the live
/// device path, and reports the stream and the world it produced.
fn record_run(frame_rate: u32) -> Run {
    let mut clock = clock(Tick(0));
    let mut session = session(SessionMode::SinglePlayer);
    session.start_recording();
    let mut frames = 0;
    for span in frame_spans(frame_rate) {
        let ticks = clock.advance(span).expect("the fixture clock advances");
        // The report belongs to the tick this frame is stamped for, exactly as
        // a producer that polls once per render frame feeds the frame it builds.
        let events = keys(&pilot_keys(session.tick().0));
        let outcome = session
            .pump_frame(FrameInput::Devices(&events), ticks)
            .expect("the fixture frame applies");
        assert!(
            outcome.is_clean(),
            "the live run is fault-free at {frame_rate} FPS: {:?}",
            outcome.faults
        );
        frames += 1;
    }
    finish_run(session.stop_recording(), session, frames)
}

/// The parts of a run that are read back from a session after it stopped
/// recording.
fn finish_run(stream: CommandStream, session: InputSession, frames: u64) -> Run {
    assert_eq!(
        session.tick(),
        Tick(TICKS_PER_SECOND),
        "one second at {TICK_HZ} Hz is {TICKS_PER_SECOND} ticks whatever the display rate"
    );
    let mut world = WorldTrace::default();
    let mut tick_of_edge = Vec::new();
    for record in stream.records() {
        let axes: Vec<(FlightCommand, f32)> = record
            .axes()
            .iter()
            .map(|axis| (axis.command(), axis.as_unit()))
            .collect();
        world.tick(record.edges(), &axes);
        for edge in record.edges() {
            assert!(
                action_is_flight(edge),
                "a UI action must never reach the consumer, got {edge} on tick {}",
                record.frame_tick().0
            );
            tick_of_edge.push(record.frame_tick());
        }
    }
    let axes = FlightCommand::CONTINUOUS
        .iter()
        .filter_map(|command| {
            session
                .controls()
                .axis(*command)
                .map(|value| (*command, value))
        })
        .collect();
    Run {
        stream,
        world,
        throttle: session.throttle().position(),
        axes,
        frames,
        tick_of_edge,
    }
}

/// Replays `recorded` for one second at `frame_rate` through a fresh session and
/// reports the world it produced.
fn replay_run(recorded: &CommandStream, frame_rate: u32) -> Run {
    let mut session = session(SessionMode::SinglePlayer);
    session.start_recording();
    let mut replay = CommandReplay::new(
        recorded.clone(),
        TickRate::new(TICK_HZ).expect("64 Hz is a valid rate"),
        SessionMode::SinglePlayer,
        Tick(0),
    )
    .expect("the replay clock is valid");
    let mut frames = 0;
    for span in frame_spans(frame_rate) {
        replay
            .frame(&mut session, span)
            .expect("the replay frame applies");
        frames += 1;
    }
    let report = replay.report();
    assert!(
        report.is_complete(),
        "the replay delivered the whole stream: {report:?}"
    );
    finish_run(session.stop_recording(), session, frames)
}

/// **AC03: replay the same quantized command stream at different display FPS.**
///
/// One live run at 144 FPS records the quantized command stream the consumer
/// executed. That one stream is then replayed at 30, 60 and 144 FPS through the
/// same pump and the same fixed-tick clock, and every run must execute the same
/// commands in the same order, move the throttle the same way and end in the
/// same world state.
///
/// What makes the scenario discriminating:
///
/// * A step applied per *render frame* instead of per executed edge would reach
///   a different throttle at 30 FPS than at 144 FPS.
/// * An edge delivered per frame rather than per tick, or re-delivered on every
///   substep, would change the ordered command sequence and the fire count.
/// * A held axis sampled at frame resolution would leave a coarser deflection
///   at a low display rate, and the accumulated world trace would differ.
/// * A replay that stopped when the stream ran out, or that orphaned records,
///   would report `is_complete() == false`; the fixture asserts it.
/// * A replay that used one frame's worth of ticks instead of the clock's would
///   run a different number of ticks, and the `ticks` field of the world trace
///   would differ.
///
/// The display rate's real effect is asserted too, and it is the honest one: a
/// coarser frame delivers a tick's commands at its own first boundary, so a
/// command can arrive up to one frame's worth of ticks *early*. The command is
/// never a different one, and never later than its own tick.
#[test]
fn accept_f22_c_replay_the_same_quantized_command_stream_at_different_display_fps() {
    let recorded_run = record_run(REFERENCE_FPS);
    let recorded = recorded_run.stream.clone();

    // The recording really is the consumer trace, one record per committed
    // tick, with a continuous axis, a one-shot edge and two steps in it.
    assert_eq!(recorded.len() as u64, TICKS_PER_SECOND);
    assert_eq!(
        recorded.edges(),
        vec![
            fire(),
            Action::Flight(FlightCommand::ThrottleStepUp),
            Action::Flight(FlightCommand::ThrottleStepUp),
        ],
        "the pilot pressed the trigger once and the throttle twice"
    );
    assert_eq!(recorded_run.world.throttle_steps, 2);
    assert_eq!(
        recorded_run
            .world
            .fired
            .iter()
            .find(|(command, _)| *command == FlightCommand::FirePrimary)
            .map(|(_, count)| *count),
        Some(1),
        "and the trigger fired exactly once"
    );
    assert_eq!(
        recorded_run.tick_of_edge,
        vec![Tick(12), Tick(20), Tick(30)]
    );
    // The quantization is what makes the comparison exact.
    let reference_fingerprint = recorded.fingerprint();

    let mut runs = Vec::new();
    for frame_rate in DISPLAY_RATES {
        let run = replay_run(&recorded, frame_rate);
        // The frame splits really are different, so the agreement below is not
        // vacuous.
        assert_eq!(run.frames, u64::from(frame_rate) + 1);
        runs.push((frame_rate, run));
    }

    let (reference_rate, reference) = &runs[2];
    assert_eq!(*reference_rate, REFERENCE_FPS);
    assert_eq!(
        reference.stream.fingerprint(),
        reference_fingerprint,
        "at the recording's own display rate a replay is the recording, bit for bit"
    );

    for (frame_rate, run) in &runs {
        assert_eq!(
            run.world, reference.world,
            "{frame_rate} FPS must produce the same world as {REFERENCE_FPS} FPS"
        );
        assert_eq!(
            run.throttle, reference.throttle,
            "{frame_rate} FPS must reach the same throttle"
        );
        assert_eq!(
            run.axes, reference.axes,
            "{frame_rate} FPS must end with the same axis state"
        );
        assert_eq!(
            run.world.ticks, TICKS_PER_SECOND,
            "{frame_rate} FPS must commit the same number of ticks"
        );
        assert_eq!(
            run.world.throttle_steps, 2,
            "{frame_rate} FPS must move the throttle by two steps, not one per frame"
        );
    }

    // The display rate moves *when* a command executes, never which one.
    let frame_ticks = TICKS_PER_SECOND / u64::from(runs[0].0) + 1;
    for (frame_rate, run) in &runs {
        for (index, tick) in run.tick_of_edge.iter().enumerate() {
            let recorded_tick = reference.tick_of_edge[index];
            assert!(
                *tick <= recorded_tick,
                "{frame_rate} FPS executed command {index} on tick {}, after its recorded \
                 tick {}",
                tick.0,
                recorded_tick.0
            );
            assert!(
                recorded_tick.0 - tick.0 < frame_ticks,
                "{frame_rate} FPS executed command {index} {} ticks early, beyond the \
                 {frame_ticks} ticks one frame covers",
                recorded_tick.0 - tick.0
            );
        }
    }

    // The runs really do differ where they are allowed to.
    assert_ne!(
        runs[0].1.tick_of_edge, reference.tick_of_edge,
        "a 30 FPS run is a different frame split, so its commands land elsewhere"
    );
}

/// Non-negotiable behavior 4, end to end: a single-player session pauses when
/// the window loses focus, and a networked session only neutralizes — its
/// client keeps producing nothing while the world's ticks keep running, because
/// the local input path has no authority to pause a server.
#[test]
fn accept_f22_c_focus_loss_pauses_single_player_and_only_neutralizes_multiplayer() {
    // Single player: the pilot is holding the trigger on the stick and the
    // pitch key on the keyboard.
    let mut single = session(SessionMode::SinglePlayer);
    let held = [
        DeviceEvent::JoystickFrame {
            device: stick(),
            buttons: vec![0],
            axes: vec![(0, 0.75)],
        },
        DeviceEvent::KeyboardFrame {
            device: keyboard(),
            keys: vec![Key::S],
        },
    ];
    let mut single_clock = clock(Tick(0));
    let outcome = single
        .pump_frame(FrameInput::Devices(&held), 2)
        .expect("the fixture frame applies");
    assert_eq!(outcome.delivered, vec![fire()]);
    assert!(single.controls().axis(FlightCommand::Pitch).is_some());
    assert!(single.controls().axis(FlightCommand::Roll).is_some());

    let lost = single.set_focus(false);
    assert_eq!(lost.pause, PauseDecision::Paused(PauseReason::FocusLost));
    assert!(single.is_paused());
    assert_eq!(single.context(), InputContext::Cinematic);
    assert_eq!(
        lost.released.released.released_edges,
        vec![fire()],
        "the held trigger is released, not left firing"
    );
    assert_eq!(single.controls().pending_edges(), 0);
    assert_eq!(single.controls().axis(FlightCommand::Pitch), Some(0.0));
    assert_eq!(single.controls().axis(FlightCommand::Roll), Some(0.0));

    // A backgrounded window is closed: no report is read and no command is
    // produced. The session's clock is frozen as well — a paused simulation's
    // clock freezes — so the render loop asks for zero ticks.
    single_clock.set_paused(true);
    let mut closed = 0;
    for span in frame_spans(REFERENCE_FPS) {
        let ticks = single_clock.advance(span).expect("the clock advances");
        assert_eq!(ticks, 0, "a frozen clock commits nothing");
        let outcome = single
            .pump_frame(FrameInput::Devices(&held), ticks)
            .expect("the fixture frame applies");
        assert_eq!(
            outcome.dropped_events, 2,
            "no report is read while unfocused"
        );
        assert!(outcome.delivered.is_empty());
        assert!(outcome.is_clean(), "{:?}", outcome.faults);
        closed += outcome.ticks_ran;
    }
    assert_eq!(closed, 0, "a paused simulation runs no input boundary");
    assert_eq!(single.tick(), Tick(2));

    // A caller that asks for boundaries anyway is refused and reported, never
    // executed into a frozen world.
    let refused = single
        .pump_frame(FrameInput::Devices(&held), 2)
        .expect("the fixture frame applies");
    assert_eq!(refused.ticks_ran, 0);
    assert_eq!(refused.ticks_refused, 2);
    assert_eq!(single.tick(), Tick(2), "and the session did not advance");
    assert!(
        matches!(
            refused.faults.as_slice(),
            [InputFault::TicksWhilePaused { requested: 2, .. }]
        ),
        "the refusal is reported: {:?}",
        refused.faults
    );
    single.take_faults();

    // Focus gain restores the context and does not resume the mission; the
    // caller decides, and the clock is unfrozen with it.
    single_clock.set_paused(false);
    let back = single.set_focus(true);
    assert_eq!(back.context, InputContext::Flight);
    assert!(single.is_paused(), "a focus gain must not resume the world");
    let resumed = single.resume();
    assert_eq!(resumed.reason, Some(HandoverReason::Paused));
    assert!(!single.is_paused());
    let after = single
        .pump_frame(FrameInput::Devices(&held), 2)
        .expect("the fixture frame applies");
    assert_eq!(
        after.delivered,
        vec![fire()],
        "control returns from the platform's own report, not from memory"
    );

    // Multiplayer: the same focus loss neutralizes and does not pause.
    let mut net = session(SessionMode::Multiplayer);
    net.pump_frame(FrameInput::Devices(&held), 2)
        .expect("the fixture frame applies");
    let before = net.tick();
    let lost = net.set_focus(false);
    assert_eq!(
        lost.pause,
        PauseDecision::NoLocalAuthority,
        "a client has no local pause authority"
    );
    assert!(!net.is_paused());
    assert_eq!(net.context(), InputContext::Cinematic);
    assert_eq!(lost.released.released.released_edges, vec![fire()]);

    // The world keeps running and the client keeps quiet: nothing in this path
    // can pause a server, because it has no channel to one.
    let mut net_clock = clock(Tick(0));
    let mut ran = 0;
    for span in frame_spans(REFERENCE_FPS) {
        let ticks = net_clock.advance(span).expect("the clock advances");
        let outcome = net
            .pump_frame(FrameInput::Devices(&held), ticks)
            .expect("the fixture frame applies");
        assert!(outcome.delivered.is_empty(), "the client sends no request");
        assert_eq!(outcome.dropped_events, 2);
        ran += outcome.ticks_ran;
    }
    assert_eq!(ran, TICKS_PER_SECOND, "the world's ticks keep committing");
    assert_eq!(net.tick(), Tick(before.0 + TICKS_PER_SECOND));
    assert!(!net.is_paused());

    // A device removed behind the window's back is still removed and reported.
    net.set_focus(true);
    net.pump_frame(
        FrameInput::Devices(&[DeviceEvent::Removed { device: stick() }]),
        0,
    )
    .expect("the fixture removal applies");
    let losses = net.take_losses();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].device, stick());
    assert!(net.take_losses().is_empty());
}

/// Non-negotiable behavior 5 and the contract's UI discipline: a text field and
/// a menu close the flight path, a UI action is a request the input session
/// never performs, and the pause screen is the caller's transaction.
#[test]
fn accept_f22_c_a_text_field_and_a_pause_screen_close_the_whole_input_path() {
    let mut session = session(SessionMode::SinglePlayer);

    // A menu: the trigger stops being a flight command and the menu keys become
    // requests for the screen path.
    session.set_context(InputContext::UiNavigation);
    let menu = session
        .pump_frame(
            FrameInput::Devices(&[
                DeviceEvent::JoystickFrame {
                    device: stick(),
                    buttons: vec![0],
                    axes: vec![(0, 0.75)],
                },
                DeviceEvent::KeyboardFrame {
                    device: keyboard(),
                    keys: vec![Key::Enter, Key::Space, Key::ArrowDown],
                },
            ]),
            1,
        )
        .expect("the fixture frame applies");
    assert_eq!(menu.ui_requests, 2);
    assert!(
        menu.delivered.is_empty(),
        "a menu cannot also fire the guns: {:?}",
        menu.delivered
    );
    assert!(menu.is_clean(), "{:?}", menu.faults);
    assert_eq!(session.controls().pending_edges(), 0);
    let requests: Vec<UiRequest> = session.take_ui_requests();
    assert_eq!(
        requests
            .iter()
            .map(|request| request.action)
            .collect::<Vec<_>>(),
        vec![UiAction::Confirm, UiAction::NavigateDown]
    );
    assert!(
        requests
            .iter()
            .all(|request| request.context == InputContext::UiNavigation),
        "a request says which context produced it"
    );
    assert!(
        session.take_ui_requests().is_empty(),
        "each request is taken once"
    );

    // The input session performed nothing: the pause is the screen path's
    // transaction, and it reports what entering it released.
    assert!(!session.is_paused());
    let paused = session.pause(PauseReason::PlayerRequest);
    assert_eq!(
        paused.decision,
        PauseDecision::Paused(PauseReason::PlayerRequest)
    );
    assert_eq!(paused.released.reason, Some(HandoverReason::Paused));
    assert!(session.is_paused());

    // Text entry emits nothing at all — not a flight command, not a UI action —
    // and it is not a fault either.
    session.set_context(InputContext::TextEntry);
    let typing = session
        .pump_frame(
            FrameInput::Devices(&[
                DeviceEvent::JoystickFrame {
                    device: stick(),
                    buttons: vec![0],
                    axes: vec![(0, 0.75)],
                },
                DeviceEvent::KeyboardFrame {
                    device: keyboard(),
                    keys: vec![Key::Space, Key::Enter, Key::S],
                },
            ]),
            0,
        )
        .expect("the fixture frame applies");
    assert!(
        typing.delivered.is_empty(),
        "text entry cannot also fire weapons: {:?}",
        typing.delivered
    );
    assert_eq!(typing.ui_requests, 0, "nor navigate a menu");
    assert!(typing.is_clean(), "{:?}", typing.faults);
    assert!(session.ui_requests().is_empty());
    assert!(session.take_faults().is_empty());

    // The context is one value: the session, its control gate and its bindings
    // all report the same one, in every direction. A divergence here is exactly
    // what would let a text field fire the guns, or silently swallow the pilot's
    // input after a screen closed.
    for context in [
        InputContext::Flight,
        InputContext::UiNavigation,
        InputContext::TextEntry,
        InputContext::Cinematic,
        InputContext::Flight,
    ] {
        session.set_context(context);
        assert_eq!(
            session.context(),
            context,
            "the session reports what it set"
        );
        assert_eq!(session.gate().context(), context, "the control gate agrees");
        assert_eq!(
            session.bindings().context(),
            context,
            "and so do the bindings that resolve the sources"
        );
    }

    // Back in flight the whole path works again, and the pause's queued input
    // did not survive it.
    session.set_context(InputContext::Flight);
    assert!(!session.resume().reason.is_none());
    let after = session
        .pump_frame(FrameInput::Devices(&[]), 2)
        .expect("the fixture frame applies");
    assert!(
        after.delivered.is_empty(),
        "a press made before the pause is not resurrected by the resume: {:?}",
        after.delivered
    );
    let fresh = session
        .pump_frame(FrameInput::Devices(&keys(&[Key::Space])), 1)
        .expect("the fixture frame applies");
    assert_eq!(fresh.delivered, vec![fire()]);
}

/// The single-authority rule and the teardown/retry path: a handover releases
/// the whole local input path, a torn-down session accepts no frames, and a
/// restart arms a genuinely new session that never inherits the old one's
/// stream.
#[test]
fn accept_f22_c_ownership_handover_and_teardown_never_leave_input_for_the_next_owner() {
    let mut owned = session(SessionMode::SinglePlayer);

    // The pilot is holding the trigger, the stick is deflected and a press is
    // queued that no boundary has delivered.
    owned
        .pump_frame(FrameInput::Devices(&stick_report(&[0], &[(0, 0.75)])), 1)
        .expect("the fixture frame applies");
    owned
        .pump_frame(FrameInput::Devices(&stick_report(&[], &[(0, 0.75)])), 0)
        .expect("the fixture release applies");
    let outcome = owned
        .pump_frame(FrameInput::Devices(&stick_report(&[0], &[(0, 0.75)])), 0)
        .expect("the fixture frame applies");
    assert!(outcome.delivered.is_empty(), "no boundary ran yet");
    assert_eq!(owned.controls().pending_edges(), 1);

    // The server takes the actor.
    let handed: ControlHandover = owned
        .assign(ControlAuthority::RemoteAuthority)
        .expect("the transfer is accepted");
    assert_eq!(handed.reason, Some(HandoverReason::OwnershipLost));
    assert_eq!(
        handed.released.released_edges,
        vec![fire()],
        "the held trigger went with the handover"
    );
    assert_eq!(
        handed.discarded_edges,
        vec![fire()],
        "and so did the queued press"
    );
    assert!(
        handed.neutralized_axes.contains(&FlightCommand::Roll),
        "and the deflection the stick was driving"
    );
    assert_eq!(owned.controls().pending_edges(), 0);
    assert_eq!(owned.controls().axis(FlightCommand::Roll), Some(0.0));
    assert_eq!(
        owned.gate().authority(),
        Some(ControlAuthority::RemoteAuthority)
    );

    // A server-owned actor executes nothing, and a recorded stream replayed
    // into it is refused by name instead of executed.
    let refused = owned
        .pump_frame(FrameInput::Devices(&stick_report(&[0], &[(0, 0.75)])), 3)
        .expect("the fixture frame applies");
    assert!(refused.delivered.is_empty());
    assert_eq!(
        refused.dropped_events, 1,
        "a server-owned actor is not read"
    );
    assert!(refused.is_clean(), "{:?}", refused.faults);

    let mut recorded = CommandStream::new();
    let mut stale = cs_types::input::InputFrame::new(owned.tick());
    stale.push_edge(fire());
    recorded
        .record_tick(stale)
        .expect("the first record is accepted");
    let mut cursor = cs_app::input::ReplayCursor::new(recorded);
    let replayed = owned
        .pump_frame(FrameInput::Replay(&mut cursor), 2)
        .expect("a refused replay is not a session error");
    assert!(replayed.delivered.is_empty());
    assert_eq!(replayed.suppress_reason, Some(SuppressReason::Context));
    assert!(
        matches!(
            replayed.faults.as_slice(),
            [InputFault::NotAuthoritative { content, .. }] if content.edges == vec![fire()]
        ),
        "a replay at a server-owned actor is reported by name: {:?}",
        replayed.faults
    );
    assert_eq!(owned.controls().pending_edges(), 0);

    // A transfer can never steal the actor, and the refusal changes nothing.
    let blocked = owned.assign(ControlAuthority::LocalSeat(LocalSeatId(1)));
    assert!(matches!(
        blocked,
        Err(SessionError::Control(
            cs_sim::control::ControlError::AuthorityAlreadyOwned {
                existing: ControlAuthority::RemoteAuthority,
                requested: ControlAuthority::LocalSeat(LocalSeatId(1)),
            }
        ))
    ));
    assert_eq!(
        owned.gate().authority(),
        Some(ControlAuthority::RemoteAuthority)
    );

    // The server hands it back and the local seat drives again.
    owned
        .release(ControlAuthority::RemoteAuthority)
        .expect("the owner releases");
    assert!(owned.assign(ControlAuthority::LocalSeat(seat())).is_ok());
    let resumed = owned
        .pump_frame(FrameInput::Devices(&stick_report(&[0], &[(0, 0.75)])), 1)
        .expect("the fixture frame applies");
    assert_eq!(
        resumed.delivered,
        vec![fire()],
        "the pilot's trigger is read again from the device's own state"
    );

    // Teardown: the session gives up the actor and stops accepting frames.
    let teardown = owned.teardown();
    assert_eq!(teardown.reason, Some(HandoverReason::Teardown));
    assert!(!owned.is_active());
    assert_eq!(owned.gate().authority(), None);
    assert_eq!(
        owned.pump_frame(FrameInput::Devices(&[]), 1),
        Err(SessionError::Inactive),
        "a stale render loop cannot keep commanding a session that ended"
    );
    assert!(owned.teardown().is_empty(), "a second teardown is safe");

    // The retry: a new session with its own authority and its own stream.
    let leftover = owned.restart().expect("the retry is accepted");
    assert!(leftover.discarded_edges.is_empty());
    assert!(owned.is_active());
    assert_eq!(
        owned.gate().authority(),
        Some(ControlAuthority::LocalSeat(seat()))
    );
    assert!(owned.stream().is_empty());
    assert!(!owned.is_recording());
    let mut restarted = owned;
    restarted.start_recording();
    restarted
        .pump_frame(FrameInput::Devices(&keys(&[Key::Space])), 2)
        .expect("the fixture frame applies");
    let stream = restarted.stop_recording();
    assert_eq!(stream.len(), 2, "the new session records its own ticks");
    assert!(
        stream.first_tick().is_some(),
        "and the ticks it recorded are its own"
    );
}

/// The replay's own error paths: a torn-down session and a drifted clock are
/// refused by name rather than replaying into nothing.
#[test]
fn accept_f22_c_replay_refuses_a_torn_down_session_and_a_drifted_clock() {
    let mut recorded = CommandStream::new();
    let mut record = cs_types::input::InputFrame::new(Tick(0));
    record.push_edge(fire());
    recorded
        .record_tick(record)
        .expect("the first record is accepted");

    let mut stopped = session(SessionMode::SinglePlayer);
    let mut replay = CommandReplay::new(
        recorded.clone(),
        TickRate::new(TICK_HZ).expect("64 Hz is a valid rate"),
        SessionMode::SinglePlayer,
        Tick(0),
    )
    .expect("the replay clock is valid");
    stopped.teardown();
    assert!(matches!(
        replay.frame(&mut stopped, frame_spans(60)[0]),
        Err(ReplayError::SessionInactive)
    ));

    // A session that has moved past the replay's clock is refused: the recorded
    // ticks would land in the wrong window, which is a silent wrongness.
    let mut fresh = session(SessionMode::SinglePlayer);
    let mut replay = CommandReplay::new(
        recorded,
        TickRate::new(TICK_HZ).expect("64 Hz is a valid rate"),
        SessionMode::SinglePlayer,
        Tick(9),
    )
    .expect("the replay clock is valid");
    assert!(matches!(
        replay.frame(&mut fresh, frame_spans(60)[0]),
        Err(ReplayError::TickDivergence {
            session: Tick(0),
            clock: Tick(9)
        })
    ));

    // A multiplayer replay runs under the networked clock policy, which grants
    // the local client no speed-up authority.
    let multiplayer = CommandReplay::new(
        CommandStream::new(),
        TickRate::new(TICK_HZ).expect("64 Hz is a valid rate"),
        SessionMode::Multiplayer,
        Tick(0),
    )
    .expect("the replay clock is valid");
    assert_eq!(
        multiplayer.clock().policy(),
        ClockPolicy::multiplayer_simulation()
    );
    assert_eq!(
        multiplayer.clock().policy().speed_up(),
        cs_sim::time::SpeedUpPolicy::NoLocalAuthority
    );
    assert_eq!(
        multiplayer.clock().policy().pause(),
        cs_sim::time::PausePolicy::Freeze
    );
    let _ = ThrottleSteps::DESIGNED_STEP;
}
