//! Acceptance scenario F22-B (AC02): unplug a joystick while firing; stop
//! fire and report device loss.
//!
//! These tests drive the whole production path the way a render loop would:
//! a `cs_app::input::InputCollector` fed one `DeviceEvent` per device per
//! render frame, whose taken `cs_types::input::InputFrame` is folded into a
//! `cs_sim::control::ControlBuffer` and drained once per fixed tick, with the
//! keyboard's throttle edges handed to a `cs_sim::control::ThrottleSteps`.
//! Nothing here re-implements the input pipeline.
//!
//! What makes the scenario discriminating:
//!
//! * If device removal did **not** release the held trigger, the simulation
//!   would keep receiving `FirePrimary` for as long as the session lives — the
//!   "weapons firing forever" failure non-negotiable behavior 3 forbids. The
//!   test counts the fire edges the buffer delivers after the unplug and
//!   requires exactly zero.
//! * If removal did **not** neutralize the axes the stick was driving, the
//!   aircraft would keep the stick's last deflection forever, because
//!   `AxisState` only ever moves an axis a frame names.
//! * If the loss were not **reported**, the caller would have no way to tell
//!   the player which device went away, so the test reads the report itself.
//! * If the released trigger could not fire again after a fresh press, the
//!   session would be permanently deaf to that stick; the test presses again
//!   after plugging it back in.
//!
//! The two other non-negotiables F22-B owns are covered the same way: the
//! calibration pipeline reaches the simulation's `AxisState` (behavior 1), the
//! keyboard throttle and the same input trace at different render frame
//! groupings agree (behavior 2), and a text field closes the whole device path
//! (behavior 5).
//!
//! Every device identity, binding, axis reading and step size here is newly
//! authored development data, not measured original game data. No test in
//! this file needs the original installation.

use cs_app::input::{
    AdapterError, DESIGNED_DEAD_ZONE, DeviceEvent, InputCollector, designed_axis_calibration,
};
use cs_sim::control::{ControlBuffer, ThrottleSteps};
use cs_types::Tick;
use cs_types::input::{
    Action, ActionMap, AxisCalibration, AxisChannel, AxisValue, BindingSource, CalibrationStore,
    DeviceClass, DeviceId, FlightCommand, GamepadAxis, GamepadButton, InputContext, Key, MouseAxis,
    MouseButton, ResponseCurve,
};

/// The fixed simulation rate of the fixture: 60 Hz.
const TICK_HZ: u64 = 60;
/// Ticks each run covers.
const TICKS: u64 = 120;

/// The stick of the fixture, with the stable identity a real platform
/// reports.
fn stick() -> DeviceId {
    DeviceId::stable(DeviceClass::Joystick, "joy.stick.fixture/0")
        .expect("the fixture identity is valid")
}

/// The keyboard of the fixture.
fn keyboard() -> DeviceId {
    DeviceId::stable(DeviceClass::Keyboard, "kbd.fixture/0").expect("the fixture identity is valid")
}

/// The mouse of the fixture.
fn mouse() -> DeviceId {
    DeviceId::stable(DeviceClass::Mouse, "mouse.fixture/0").expect("the fixture identity is valid")
}

/// The gamepad of the fixture.
fn gamepad() -> DeviceId {
    DeviceId::stable(DeviceClass::Gamepad, "pad.fixture/0").expect("the fixture identity is valid")
}

/// One session: a collector, the simulation's control buffer and the keyboard
/// throttle, wired the way the render loop wires them.
struct Session {
    collector: InputCollector,
    controls: ControlBuffer,
    throttle: ThrottleSteps,
    tick: Tick,
}

impl Session {
    /// A session in flight context with the stick, keyboard, mouse and gamepad
    /// connected.
    fn new() -> Self {
        let mut session = Self {
            collector: InputCollector::designed_default(Tick(0)),
            controls: ControlBuffer::new(),
            throttle: ThrottleSteps::designed_default(),
            tick: Tick(0),
        };
        for device in [stick(), keyboard(), mouse(), gamepad()] {
            session
                .collector
                .connect_device(device)
                .expect("the fixture device connects");
        }
        session
    }

    /// Runs one render frame's worth of ticks: the events are folded into one
    /// frame, the frame is applied, and `ticks_per_frame` input boundaries run.
    fn frame(&mut self, events: &[DeviceEvent], ticks_per_frame: u64) -> Vec<Action> {
        self.collector.begin_frame(self.tick);
        for event in events {
            self.collector
                .observe_device(event)
                .expect("the fixture event applies");
        }
        let frame = self.collector.take_frame();
        self.controls
            .apply_frame(&frame)
            .expect("the fixture frame applies");
        let mut edges = Vec::new();
        for _ in 0..ticks_per_frame {
            let tick_edges = self.controls.begin_tick(self.tick);
            self.throttle.apply_tick(&tick_edges);
            edges.extend(tick_edges);
            self.tick = Tick(self.tick.0 + 1);
        }
        edges
    }

    /// The fire edges the session's consumers have seen, without stepping any
    /// ticks: a render frame whose ticks are drained by the next frame's
    /// boundaries.
    fn render_frame(&mut self, events: &[DeviceEvent]) -> Vec<Action> {
        self.frame(events, 0)
    }

    /// Drains the edges that are due at the current tick.
    fn tick_boundary(&mut self) -> Vec<Action> {
        let edges = self.controls.begin_tick(self.tick);
        self.throttle.apply_tick(&edges);
        self.tick = Tick(self.tick.0 + 1);
        edges
    }

    /// The deflection the simulation currently holds for an axis.
    fn axis(&self, command: FlightCommand) -> Option<f32> {
        self.controls.axis(command)
    }

    /// How many edges are queued for a boundary and not yet delivered.
    fn pending_edges(&self) -> usize {
        self.controls.pending_edges()
    }
}

/// The stick is deflected and its trigger is held, exactly as a pilot flying
/// with a joystick would be.
fn stick_firing() -> DeviceEvent {
    DeviceEvent::JoystickFrame {
        device: stick(),
        buttons: vec![0],
        axes: vec![(0, 0.8), (1, -0.4)],
    }
}

/// A stick report with nothing held and centred.
fn stick_idle() -> DeviceEvent {
    DeviceEvent::JoystickFrame {
        device: stick(),
        buttons: vec![],
        axes: vec![(0, 0.0), (1, 0.0)],
    }
}

/// AC02, the F22-B minimum scenario: the player is firing from a joystick, the
/// joystick is unplugged, and from that moment the session stops firing, drops
/// the stick's deflection, and reports the loss.
#[test]
fn accept_f22_b_unplug_joystick_while_firing_stops_fire_and_reports_loss() {
    let mut session = Session::new();

    // Three render frames of held trigger and deflection: the first queues one
    // fire edge and the following two add nothing, because a held trigger is
    // one press however many frames it stays down.
    for frame in 0..3 {
        session.render_frame(&[stick_firing()]);
        assert_eq!(
            session.pending_edges(),
            1,
            "a held trigger queues exactly one fire edge, frame {frame}"
        );
    }
    let edges = session.tick_boundary();
    assert_eq!(
        edges,
        vec![Action::Flight(FlightCommand::FirePrimary)],
        "the one fire edge is delivered at the input boundary"
    );
    assert_eq!(session.pending_edges(), 0, "and it is delivered only once");
    assert!(
        session
            .axis(FlightCommand::Roll)
            .is_some_and(|roll| roll > 0.5),
        "the stick's roll is held by the simulation, got {:?}",
        session.axis(FlightCommand::Roll)
    );
    // The designed map binds the stick's second axis to pitch with a negative
    // scale, so a raw -0.4 reads as a positive deflection.
    assert!(
        session
            .axis(FlightCommand::Pitch)
            .is_some_and(|pitch| (pitch - 0.4).abs() < 0.01),
        "the stick's pitch is held by the simulation, got {:?}",
        session.axis(FlightCommand::Pitch)
    );

    // The pilot releases the trigger; the gun is no longer firing but the stick
    // still flies the aircraft.
    session.render_frame(&[stick_idle()]);
    let edges = session.tick_boundary();
    assert!(
        edges.is_empty(),
        "releasing the trigger produces no command, got {edges:?}"
    );

    // Now the joystick is unplugged mid-flight.
    session
        .collector
        .observe_device(&DeviceEvent::Removed { device: stick() })
        .expect("the removal applies and reports nothing else");

    // The caller is told what was lost, by name.
    let losses = session.collector.take_device_losses();
    assert_eq!(losses.len(), 1, "the loss is reported exactly once");
    assert_eq!(losses[0].device, stick());
    assert_eq!(
        losses[0].stable_identity.as_deref(),
        Some("joy.stick.fixture/0"),
        "the loss names the device's stable identity"
    );
    assert!(
        session.collector.take_device_losses().is_empty(),
        "a loss is reported once, not on every frame"
    );

    // The session keeps running for a second of simulated time with no stick
    // reporting at all: no fire, and the axes the stick drove are neutral.
    let mut fires = 0;
    let mut ticks = 0;
    while ticks < TICK_HZ {
        fires += session.frame(&[], 1).len();
        ticks += 1;
    }
    assert_eq!(fires, 0, "a removed stick cannot leave the guns firing");
    assert_eq!(
        session.axis(FlightCommand::Roll),
        Some(0.0),
        "the removed stick's roll is neutralized, not left stuck"
    );
    assert_eq!(
        session.axis(FlightCommand::Pitch),
        Some(0.0),
        "the removed stick's pitch is neutralized, not left stuck"
    );

    // Plugging it back in restores control, including firing: the removal
    // released the hold rather than poisoning the session.
    session
        .collector
        .observe_device(&DeviceEvent::Connected { device: stick() })
        .expect("the stick reconnects");
    let edges = session.frame(&[stick_firing()], 1);
    assert_eq!(
        edges,
        vec![Action::Flight(FlightCommand::FirePrimary)],
        "the reconnected stick can fire again"
    );
    assert!(
        session
            .axis(FlightCommand::Roll)
            .is_some_and(|roll| roll > 0.5),
        "the reconnected stick flies the aircraft again"
    );
}

/// The failure cases the scenario has to survive: a removal the session never
/// had, a stale event from the removed stick, a report whose class disagrees
/// with its device, and a reading no calibration accepts. None of them may
/// fire a weapon, move an axis, or leave a hold behind.
#[test]
fn accept_f22_b_refused_device_events_change_nothing_while_firing() {
    let mut session = Session::new();
    session.render_frame(&[stick_firing()]);
    session.tick_boundary();

    let stranger = DeviceId::stable(DeviceClass::Joystick, "joy.stick.stranger/0")
        .expect("the fixture identity is valid");

    // A frame from a device that was never connected.
    let orphan = DeviceEvent::JoystickFrame {
        device: stranger.clone(),
        buttons: vec![0],
        axes: vec![(0, 1.0)],
    };
    assert_eq!(
        session
            .collector
            .observe_device(&orphan)
            .expect_err("a stale event must be refused"),
        AdapterError::NotConnected {
            device: stranger.clone()
        }
    );

    // A removal the session never had.
    assert_eq!(
        session.collector.disconnect_device(&stranger),
        Err(AdapterError::NotConnected {
            device: stranger.clone()
        })
    );

    // A gamepad report that names the joystick.
    assert_eq!(
        session
            .collector
            .observe_device(&DeviceEvent::GamepadFrame {
                device: stick(),
                buttons: vec![GamepadButton::South],
                axes: vec![(GamepadAxis::LeftTrigger, 1.0)],
            }),
        Err(AdapterError::ClassMismatch {
            expected: DeviceClass::Joystick,
            reported: DeviceClass::Gamepad,
            device: stick(),
        })
    );

    // A reading beyond full deflection, and one that is not a number.
    let wild = DeviceEvent::JoystickFrame {
        device: stick(),
        buttons: vec![0],
        axes: vec![(0, 1.5)],
    };
    assert_eq!(
        session.collector.observe_device(&wild),
        Err(AdapterError::ReadingRejected {
            device: stick(),
            channel: AxisChannel::Joystick(0),
            error: cs_types::input::CalibrationError::ReadingOutOfRange { value: 1.5 },
        })
    );
    let not_a_number = DeviceEvent::JoystickFrame {
        device: stick(),
        buttons: vec![0],
        axes: vec![(0, f32::INFINITY)],
    };
    assert!(
        matches!(
            session.collector.observe_device(&not_a_number),
            Err(AdapterError::ReadingRejected {
                device,
                channel: AxisChannel::Joystick(0),
                ..
            }) if device == stick()
        ),
        "a non-finite reading is refused by device and channel"
    );

    // None of the refusals fired, moved an axis or changed the session.
    assert_eq!(
        session.pending_edges(),
        0,
        "a refused event must not queue a command"
    );
    assert!(
        session
            .axis(FlightCommand::Roll)
            .is_some_and(|roll| roll > 0.5),
        "a refused event must not move an axis, got {:?}",
        session.axis(FlightCommand::Roll)
    );
    assert_eq!(
        session.collector.devices().connected().len(),
        4,
        "a refused event must not change the device set"
    );
    assert!(session.collector.take_device_losses().is_empty());
    assert_eq!(
        session.collector.devices().held_edges().len(),
        1,
        "a refused event must not release or add a hold"
    );

    // The real report that follows still works, and produces no fire edge,
    // because the hold the refused reports named was never established twice.
    let edges = session.render_frame(&[stick_firing()]);
    let delivered = session.tick_boundary();
    assert!(
        edges.is_empty(),
        "the held trigger is still held, so it fires again only after a release"
    );
    assert!(
        delivered.is_empty(),
        "a still-held trigger produces no further command, got {delivered:?}"
    );
}

/// Non-negotiable behavior 1 reaches the simulation: the stick's dead zone,
// curve, saturation and identity are all applied before the axis reaches
/// `AxisState`, and a stick that re-enumerates at another index keeps its
/// tuning through its stable identity.
#[test]
fn accept_f22_b_calibrated_axes_reach_the_control_buffer_by_device_identity() {
    let mut session = Session::new();

    // Calibrate this stick: a wide dead zone, a squared response, a player
    // inversion and 80% of nominal travel.
    let calibration = AxisCalibration::try_new(0.25, true, ResponseCurve::Power(2.0), 0.8, 0.5)
        .expect("the fixture calibration is in range");
    session.collector.devices_mut().calibration_mut().set(
        &stick(),
        AxisChannel::Joystick(0),
        calibration,
    );
    assert_eq!(
        session
            .collector
            .devices()
            .calibration()
            .calibration_or_default(&stick(), AxisChannel::Joystick(0)),
        calibration
    );

    // A deflection inside the dead zone is exactly neutral.
    let edges = session.frame(
        &[
            DeviceEvent::JoystickFrame {
                device: stick(),
                buttons: vec![],
                axes: vec![(0, 0.2)],
            },
            stick_idle(),
        ],
        1,
    );
    assert!(edges.is_empty());
    assert_eq!(
        session.axis(FlightCommand::Roll),
        Some(0.0),
        "a reading inside the dead zone is neutral in the simulation too"
    );

    // A deflection past the dead zone reaches `AxisState` as the calibrated
    // value: inverted, squared and saturated.
    session.frame(
        &[
            DeviceEvent::JoystickFrame {
                device: stick(),
                buttons: vec![],
                axes: vec![(0, 0.7)],
            },
            stick_idle(),
        ],
        1,
    );
    let roll = session.axis(FlightCommand::Roll).expect("roll is driven");
    // raw 0.7 -> past the 0.25 dead zone -> (0.7 - 0.25) / 0.75 = 0.6 ->
    // inverted -> -0.6 -> squared -> -0.36 -> binding scale 1.0 -> -0.36.
    assert!(
        (roll + 0.36).abs() < 0.01,
        "the calibrated deflection reaches the simulation, got {roll}"
    );
    assert!(
        roll.abs() < 0.8,
        "the saturation is a ceiling, not a stretch: {roll}"
    );

    // The stick is unplugged and comes back at a different enumeration index.
    // Adopting its stable identity again re-keys the connection, so its
    // calibration is applied from the very first report; another stick never
    // inherits it.
    session
        .collector
        .observe_device(&DeviceEvent::Removed { device: stick() })
        .expect("the stick is unplugged");
    let replugged = DeviceId::enumeration_fallback(DeviceClass::Joystick, 4);
    session
        .collector
        .connect_device(replugged.clone())
        .expect("the stick replugs at another index");
    assert_eq!(
        session
            .collector
            .adopt_device_identity(&replugged, stick())
            .expect("the stick adopts its identity again"),
        0,
        "the records are already under the stable identity"
    );
    // The adopted device now reports under its stable identity again.
    session.frame(
        &[
            DeviceEvent::JoystickFrame {
                device: stick(),
                buttons: vec![],
                axes: vec![(0, 0.7)],
            },
            stick_idle(),
        ],
        1,
    );
    let roll = session.axis(FlightCommand::Roll).expect("roll is driven");
    assert!(
        (roll + 0.36).abs() < 0.01,
        "the calibration survived the replug, got {roll}"
    );

    let other = DeviceId::stable(DeviceClass::Joystick, "joy.stick.other/0")
        .expect("the fixture identity is valid");
    assert_eq!(
        session
            .collector
            .devices()
            .calibration()
            .calibration_or_default(&other, AxisChannel::Joystick(0)),
        AxisCalibration::designed_default(),
        "another stick never inherits this one's calibration"
    );

    // An index-keyed record is never reported as a persistable identity.
    let provisional = DeviceId::enumeration_fallback(DeviceClass::Joystick, 9);
    session.collector.devices_mut().calibration_mut().set(
        &provisional,
        AxisChannel::Joystick(0),
        calibration,
    );
    assert_eq!(
        session.collector.devices().calibration().unstable_devices(),
        vec![&provisional],
        "an index-keyed record is visible as unsafe to persist"
    );
}

/// Non-negotiable behavior 2: the keyboard's throttle steps and direct
/// settings are applied at the input boundary, so the same input trace reaches
/// the same throttle at 12, 30 and 60 render FPS.
#[test]
fn accept_f22_b_keyboard_throttle_is_identical_at_12_30_and_60_render_fps() {
    /// What one run of the trace produced: the throttle after the step
    /// segment, after the direct setting, at the end, and how many changes the
    /// whole trace reported.
    #[derive(Debug, PartialEq)]
    struct Trace {
        after_steps: f32,
        after_direct: f32,
        final_position: f32,
        changes: usize,
    }

    /// Replays one tick-defined trace with `ticks_per_frame` ticks per render
    /// frame. The schedule is defined in **ticks**, not in frames, so the same
    /// number of presses is delivered however the frames are grouped: the
    /// throttle-up key is down on every 20th tick and up on the next one, then
    /// the direct full setting, then one press down.
    fn trace(ticks_per_frame: u64) -> Trace {
        let mut session = Session::new();
        let mut changes = 0;
        let mut tick = 0_u64;
        let mut after_steps = None;
        while tick < TICKS {
            let keys = if tick.is_multiple_of(20) {
                vec![Key::R]
            } else if tick % 20 == 1 {
                vec![]
            } else {
                // A frame that observes nothing still polls the keyboard, so
                // the rest of the schedule is the keyboard's empty state.
                Vec::new()
            };
            changes += session
                .frame(
                    &[DeviceEvent::KeyboardFrame {
                        device: keyboard(),
                        keys,
                    }],
                    ticks_per_frame,
                )
                .len();
            if tick + ticks_per_frame >= TICKS {
                after_steps = Some(session.throttle.position());
            }
            tick += ticks_per_frame;
        }
        let after_steps = after_steps.expect("the trace covers at least one frame");
        changes += session
            .frame(
                &[DeviceEvent::KeyboardFrame {
                    device: keyboard(),
                    keys: vec![Key::Digit4],
                }],
                ticks_per_frame,
            )
            .len();
        let after_direct = session.throttle.position();
        changes += session
            .frame(
                &[
                    DeviceEvent::KeyboardFrame {
                        device: keyboard(),
                        keys: vec![Key::F],
                    },
                    DeviceEvent::KeyboardFrame {
                        device: keyboard(),
                        keys: vec![],
                    },
                ],
                ticks_per_frame,
            )
            .len();
        Trace {
            after_steps,
            after_direct,
            final_position: session.throttle.position(),
            changes,
        }
    }

    let twelve = trace(5);
    let thirty = trace(2);
    let sixty = trace(1);
    assert_eq!(twelve, thirty, "12 and 30 render FPS disagree");
    assert_eq!(thirty, sixty, "30 and 60 render FPS disagree");

    // Six presses of the throttle-up key, one step each, wherever the frames
    // fell. A per-frame rate or a step per frame would give a different count.
    assert_eq!(
        twelve.changes, 8,
        "six presses, one direct setting, one press down"
    );
    assert!(
        (twelve.after_steps - ThrottleSteps::DESIGNED_STEP * 6.0).abs() < 1e-6,
        "six presses move the throttle six steps, got {}",
        twelve.after_steps
    );
    assert_eq!(
        twelve.after_direct,
        ThrottleSteps::FULL,
        "the direct setting is applied regardless of the frame grouping"
    );
    assert!(
        (twelve.final_position - (ThrottleSteps::FULL - ThrottleSteps::DESIGNED_STEP)).abs() < 1e-6,
        "the press down after the direct setting moves one step, got {}",
        twelve.final_position
    );
}

/// Non-negotiable behavior 5: a text field takes the whole input path, so no
/// device can fire a weapon or move a flight axis while it owns the keyboard,
/// and the axes it silenced are neutralized.
#[test]
fn accept_f22_b_text_entry_silences_every_device_and_neutralizes_the_axes() {
    let mut session = Session::new();

    // In flight the stick fires and flies.
    let edges = session.frame(&[stick_firing()], 1);
    assert_eq!(
        edges,
        vec![Action::Flight(FlightCommand::FirePrimary)],
        "the stick fires in flight context"
    );
    assert!(
        session
            .axis(FlightCommand::Roll)
            .is_some_and(|roll| roll > 0.5),
        "the stick flies in flight context"
    );

    // A text field opens while the trigger is still down.
    session.collector.set_context(InputContext::TextEntry);
    let edges = session.frame(&[stick_firing()], 1);
    assert!(
        edges.is_empty(),
        "text entry must not also fire weapons, got {edges:?}"
    );
    assert_eq!(
        session.axis(FlightCommand::Roll),
        Some(0.0),
        "text entry also neutralizes the flight axes, got {:?}",
        session.axis(FlightCommand::Roll)
    );

    // A keyboard press in text entry is text, not a command.
    let edges = session.frame(
        &[
            DeviceEvent::KeyboardFrame {
                device: keyboard(),
                keys: vec![Key::Space, Key::W, Key::R],
            },
            stick_idle(),
        ],
        1,
    );
    assert!(
        edges.is_empty(),
        "no key bound to a flight command may reach the simulation, got {edges:?}"
    );
    assert_eq!(
        session.throttle.position(),
        ThrottleSteps::IDLE,
        "the throttle does not step while a text field owns the keyboard"
    );

    // Closing the field restores control without needing a fresh press.
    session.collector.set_context(InputContext::Flight);
    let edges = session.frame(&[stick_firing()], 1);
    assert_eq!(
        edges,
        vec![Action::Flight(FlightCommand::FirePrimary)],
        "the still-held trigger resumes firing when the field closes"
    );
}

/// Every declared device family reaches the simulation through its own
/// adapter, with the designed binding of each class: the keyboard's digital
/// axes, the mouse's buttons and relative motion, the gamepad's sticks and its
/// re-centered trigger, and the joystick's indexed axes and buttons.
#[test]
fn accept_f22_b_every_device_family_reaches_the_control_buffer() {
    let mut session = Session::new();

    // Keyboard: a digital axis at full scale, and two real presses of the
    // throttle key. A report is the keyboard's whole state, so a press needs a
    // release between it and the next one.
    let press = DeviceEvent::KeyboardFrame {
        device: keyboard(),
        keys: vec![Key::A, Key::R, Key::R],
    };
    let release = DeviceEvent::KeyboardFrame {
        device: keyboard(),
        keys: vec![Key::A],
    };
    session.frame(std::slice::from_ref(&press), 1);
    session.frame(std::slice::from_ref(&release), 1);
    session.frame(std::slice::from_ref(&press), 1);
    session.frame(std::slice::from_ref(&release), 1);
    assert_eq!(
        session.axis(FlightCommand::Yaw),
        Some(-1.0),
        "A is a full-scale digital yaw axis"
    );
    assert!(
        (session.throttle.position() - ThrottleSteps::DESIGNED_STEP * 2.0).abs() < 1e-6,
        "two presses stepped the throttle twice, got {}",
        session.throttle.position()
    );
    assert_eq!(
        session.pending_edges(),
        0,
        "one key listed twice in one report is one hold, not two presses"
    );

    // Mouse: the left button fires, and the relative motion drives yaw and
    // pitch, which the calibration pipeline still passes through.
    let mut mouse_session = Session::new();
    let edges = mouse_session.frame(
        &[
            DeviceEvent::MouseFrame {
                device: mouse(),
                buttons: vec![MouseButton::Left],
                motion_x: 0.5,
                motion_y: -0.5,
            },
            DeviceEvent::KeyboardFrame {
                device: keyboard(),
                keys: vec![],
            },
            stick_idle(),
        ],
        1,
    );
    assert_eq!(
        edges,
        vec![Action::Flight(FlightCommand::FirePrimary)],
        "the left mouse button fires the primary guns"
    );
    let yaw = mouse_session
        .axis(FlightCommand::Yaw)
        .expect("yaw is driven");
    let pitch = mouse_session
        .axis(FlightCommand::Pitch)
        .expect("pitch is driven");
    assert!(
        (yaw - 0.5).abs() < 0.01,
        "mouse motion drives yaw, got {yaw}"
    );
    assert!(
        (pitch - 0.5).abs() < 0.01,
        "mouse motion drives pitch, got {pitch}"
    );

    // Gamepad: the left stick drives roll, the right trigger the throttle, and
    // an untriggered trigger reads the axis minimum.
    let mut pad_session = Session::new();
    pad_session.frame(
        &[
            DeviceEvent::GamepadFrame {
                device: gamepad(),
                buttons: vec![GamepadButton::South],
                axes: vec![
                    (GamepadAxis::LeftStickX, -0.6),
                    (GamepadAxis::RightTrigger, 0.0),
                ],
            },
            DeviceEvent::KeyboardFrame {
                device: keyboard(),
                keys: vec![],
            },
            stick_idle(),
        ],
        1,
    );
    assert_eq!(
        pad_session.axis(FlightCommand::Throttle),
        Some(-1.0),
        "an untriggered trigger is the throttle axis' idle end"
    );
    assert!(
        pad_session
            .axis(FlightCommand::Roll)
            .is_some_and(|roll| (roll + 0.6).abs() < 0.01),
        "the left stick drives roll, got {:?}",
        pad_session.axis(FlightCommand::Roll)
    );

    // A frame in which no device drove the throttle leaves the simulation's
    // throttle axis undriven rather than guessing one.
    let mut quiet = Session::new();
    quiet.frame(&[stick_idle()], 1);
    assert_eq!(
        quiet.axis(FlightCommand::Throttle),
        None,
        "no device drove the throttle, so no throttle axis exists"
    );
}

/// The designed starting calibration and the axis-channel vocabulary are the
/// project's own, and they cover every analog source the designed map binds.
#[test]
fn accept_f22_b_designed_calibration_covers_every_bound_analog_channel() {
    let designed = designed_axis_calibration();
    assert!(
        !designed.is_identity(),
        "the designed calibration shapes the axis"
    );
    assert_eq!(designed.deadzone(), DESIGNED_DEAD_ZONE);
    assert_eq!(
        designed.apply(0.05),
        Ok(0.0),
        "the designed dead zone applies"
    );
    assert_eq!(
        designed.apply(1.0),
        Ok(1.0),
        "full travel is unaffected by the dead zone"
    );

    // Every analog channel the designed default binds is a calibratable
    // channel of a named device.
    let map = ActionMap::designed_default();
    let channels: Vec<AxisChannel> = map
        .bindings()
        .iter()
        .filter_map(|binding| AxisChannel::from_source(binding.source))
        .collect();
    assert!(
        channels.len() > 4,
        "the designed default binds analog channels"
    );
    assert!(channels.contains(&AxisChannel::Joystick(0)));
    assert!(channels.contains(&AxisChannel::Gamepad(GamepadAxis::RightTrigger)));
    assert!(channels.contains(&AxisChannel::Mouse(MouseAxis::X)));
    for channel in &channels {
        assert_eq!(
            channel.device_class(),
            map.bindings_for_channel(*channel)
                .next()
                .expect("a bound channel is walkable")
                .source
                .device_class(),
            "{channel} must belong to the device class that reports it"
        );
    }

    // The wheel is a mouse channel in the vocabulary and is unbound in the
    // designed default map, so it drives nothing and calibrates nothing.
    let wheel = AxisChannel::Mouse(MouseAxis::Wheel);
    assert_eq!(
        wheel,
        AxisChannel::from_source(BindingSource::MouseAxis(MouseAxis::Wheel))
            .expect("the wheel is a channel")
    );
    assert!(map.bindings_for_channel(wheel).next().is_none());
    assert!(wheel.is_relative(), "a mouse channel is relative");

    // An uncalibrated channel is used exactly as the device reports itself.
    let store = CalibrationStore::new();
    assert!(store.is_empty());
    assert_eq!(
        store.calibration_or_default(&stick(), AxisChannel::Joystick(0)),
        AxisCalibration::designed_default()
    );
    assert!(
        AxisCalibration::designed_default()
            .apply(0.5)
            .is_ok_and(|value| value == 0.5),
        "the default calibration is transparent"
    );

    // Every axis a frame carries stays a quantized sample.
    let value = AxisValue::from_unit(FlightCommand::Roll, 0.25).expect("a valid deflection");
    assert_eq!(value.quantized(), 8192, "the frame's axes stay quantized");
}
