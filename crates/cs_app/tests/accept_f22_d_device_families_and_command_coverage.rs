//! Acceptance scenarios for F22-D: every declared device family and the
//! declared command coverage.
//!
//! The sheet's minimum scenario for this stage is **AC04: "Open text entry
//! and confirm flight commands are not emitted"**, which
//! `accept_f22_d_open_text_entry_and_confirm_flight_commands_are_not_emitted`
//! covers end to end. The other tests cover the rest of the stage's title:
//! that *every* declared device family (`keyboard`, `mouse`, `gamepad`,
//! `joystick`) drives the simulation through its own adapter, and that *every*
//! declared command — `FlightCommand::ALL` and `UiAction::ALL` — is reachable
//! from the designed default map and observed at the consumer.
//!
//! Every test here drives production code the way a render loop would and
//! nothing else:
//!
//! * `cs_app::input::InputSession::pump_frame` is the loop, one call per
//!   render frame, with `cs_app::input::DeviceEvent`s as the producer.
//! * The consumer is the session's own `cs_sim::control::ControlBuffer`,
//!   `ThrottleSteps` and the `FrameOutcome::delivered` trace they produce, so
//!   "the command arrived" is read where the simulation reads it.
//! * The recorded `cs_types::input::CommandStream` (and its fingerprint) is
//!   the evidence of what the input boundary actually handed over.
//!
//! Nothing here re-implements the input pipeline: every report is built from
//! the *production* `ActionMap`, so a binding the map loses makes the
//! coverage assertions fail instead of quietly shrinking the test.
//!
//! Every device identity, binding, reading and tick here is newly authored
//! development data, not measured original game data. No test in this file
//! needs the original installation, and none of them claims anything about
//! which commands the original 2000 PC game exposes — that measurement is
//! still open and is recorded in the stage's findings.

use cs_app::input::{
    DeviceEvent, FrameInput, FrameOutcome, InputFault, InputSession, SessionMode, SuppressReason,
};
use cs_sim::control::{LocalSeatId, ThrottleSteps};
use cs_types::Tick;
use cs_types::input::{
    Action, ActionMap, Binding, BindingSource, BindingTarget, DeviceClass, DeviceId, FlightCommand,
    GamepadAxis, InputContext, Key, MouseAxis, UiAction,
};

/// A stick reading well past the designed dead zone, in the canonical
/// `[-1, 1]` pipeline every analog channel feeds.
const ANALOG_READING: f32 = 0.85;
/// A trigger pulled all the way: the driver reports `[0, 1]`, and
/// `normalize_gamepad_axis` maps a full pull onto the axis maximum.
const TRIGGER_PULL: f32 = 1.0;
/// One mouse frame's motion, in device units scaled into `[-1, 1]`, on a
/// channel that moved.
const MOUSE_MOTION: f32 = 0.8;
/// One mouse frame's wheel scroll, in the same units as the motion.
const MOUSE_WHEEL: f32 = 0.6;
/// How far a measured axis value may differ from the value the report asked
/// for: `AxisValue` quantizes to `i16`, about 3e-5 of full scale.
const QUANTIZATION_TOLERANCE: f32 = 1e-3;

/// The pilot's seat.
fn seat() -> LocalSeatId {
    LocalSeatId(0)
}

/// The fixture device of one declared family, with the stable identity a
/// real platform reports.
fn device(class: DeviceClass) -> DeviceId {
    DeviceId::stable(class, &format!("{}.fixture/0", class.label()))
        .expect("the fixture identity is valid")
}

/// A session with every declared device family connected, as a platform's
/// device-enumeration events would leave it, in flight context.
fn session() -> InputSession {
    let mut session = InputSession::designed_default(seat(), SessionMode::SinglePlayer, Tick(0));
    for class in DeviceClass::ALL {
        session
            .collector_mut()
            .connect_device(device(*class))
            .expect("the fixture device connects");
    }
    session
}

/// The bindings of `map` that resolve in the flight context, in map order.
fn flight_bindings(map: &ActionMap) -> impl Iterator<Item = Binding> + '_ {
    map.bindings()
        .iter()
        .filter(|binding| binding.target.context() == InputContext::Flight)
        .copied()
}

/// The command a binding drives, for an edge (`Command`) target.
fn edge_command(binding: Binding) -> Option<FlightCommand> {
    match binding.target {
        BindingTarget::Command(command) => Some(command),
        BindingTarget::Axis { .. } | BindingTarget::Ui(_) => None,
    }
}

/// Every edge command the designed map binds in the flight context, sorted
/// and deduplicated: the set one full report per family must deliver.
fn expected_edges(map: &ActionMap) -> Vec<Action> {
    let mut actions: Vec<Action> = flight_bindings(map)
        .filter_map(|binding| edge_command(binding).map(Action::Flight))
        .collect();
    actions.sort_unstable();
    actions.dedup();
    actions
}

/// The reading one source reports while it is held or deflected, in the
/// canonical `[-1, 1]` pipeline — the value the adapter multiplies by the
/// binding's `scale`.
fn reading(source: BindingSource) -> f32 {
    match source {
        // A digital source is at full scale while it is down; the binding's
        // own scale carries its direction.
        BindingSource::Key(_)
        | BindingSource::MouseButton(_)
        | BindingSource::GamepadButton(_)
        | BindingSource::JoystickButton(_) => 1.0,
        BindingSource::MouseAxis(MouseAxis::X | MouseAxis::Y) => MOUSE_MOTION,
        // The wheel is a declared mouse channel with a field in
        // `DeviceEvent::MouseFrame` (#412 F22-I), so a report can carry it
        // like any other relative reading. It is unbound in the designed map,
        // so the case below never drives it; the reading is here so a binding
        // that does bind it is exercised rather than panicking.
        BindingSource::MouseAxis(MouseAxis::Wheel) => MOUSE_WHEEL,
        BindingSource::GamepadAxis(GamepadAxis::LeftTrigger | GamepadAxis::RightTrigger) => {
            TRIGGER_PULL
        }
        BindingSource::GamepadAxis(_) => ANALOG_READING,
        // The device's raw reading; a binding wired backwards inverts it.
        BindingSource::JoystickAxis {
            inverted: false, ..
        } => ANALOG_READING,
        BindingSource::JoystickAxis { inverted: true, .. } => -ANALOG_READING,
    }
}

/// The raw reading a driver reports for `source`: sticks in `[-1, 1]`, a
/// trigger in `[0, 1]`, mouse motion in device units scaled into `[-1, 1]`.
fn raw_for(source: BindingSource) -> f32 {
    match source {
        BindingSource::GamepadAxis(GamepadAxis::LeftTrigger | GamepadAxis::RightTrigger) => {
            TRIGGER_PULL
        }
        BindingSource::GamepadAxis(_) | BindingSource::JoystickAxis { .. } => ANALOG_READING,
        BindingSource::MouseAxis(MouseAxis::Wheel) => MOUSE_WHEEL,
        BindingSource::MouseAxis(_) => MOUSE_MOTION,
        BindingSource::Key(_)
        | BindingSource::MouseButton(_)
        | BindingSource::GamepadButton(_)
        | BindingSource::JoystickButton(_) => 0.0,
    }
}

/// One device's report while `sources` are down (or deflected). The report is
/// the device's whole state, exactly as a render loop would poll it.
fn report(class: DeviceClass, sources: &[BindingSource]) -> DeviceEvent {
    let device = device(class);
    match class {
        DeviceClass::Keyboard => DeviceEvent::KeyboardFrame {
            device,
            keys: sources
                .iter()
                .filter_map(|source| match source {
                    BindingSource::Key(key) => Some(*key),
                    _ => None,
                })
                .collect(),
        },
        DeviceClass::Mouse => {
            let buttons = sources
                .iter()
                .filter_map(|source| match source {
                    BindingSource::MouseButton(button) => Some(*button),
                    _ => None,
                })
                .collect();
            let moves = |wanted: MouseAxis| sources.contains(&BindingSource::MouseAxis(wanted));
            DeviceEvent::MouseFrame {
                device,
                buttons,
                motion_x: if moves(MouseAxis::X) {
                    MOUSE_MOTION
                } else {
                    0.0
                },
                motion_y: if moves(MouseAxis::Y) {
                    MOUSE_MOTION
                } else {
                    0.0
                },
                wheel: if moves(MouseAxis::Wheel) {
                    MOUSE_WHEEL
                } else {
                    0.0
                },
            }
        }
        DeviceClass::Gamepad => DeviceEvent::GamepadFrame {
            device,
            buttons: sources
                .iter()
                .filter_map(|source| match source {
                    BindingSource::GamepadButton(button) => Some(*button),
                    _ => None,
                })
                .collect(),
            axes: sources
                .iter()
                .filter_map(|source| match source {
                    BindingSource::GamepadAxis(axis) => Some((*axis, raw_for(*source))),
                    _ => None,
                })
                .collect(),
        },
        DeviceClass::Joystick => DeviceEvent::JoystickFrame {
            device,
            buttons: sources
                .iter()
                .filter_map(|source| match source {
                    BindingSource::JoystickButton(index) => Some(*index),
                    _ => None,
                })
                .collect(),
            axes: sources
                .iter()
                .filter_map(|source| match source {
                    BindingSource::JoystickAxis { index, .. } => Some((*index, raw_for(*source))),
                    _ => None,
                })
                .collect(),
        },
    }
}

/// The flight-context sources of `class`, in map order.
fn sources_of(map: &ActionMap, class: DeviceClass) -> Vec<BindingSource> {
    flight_bindings(map)
        .filter(|binding| binding.source.device_class() == class)
        .map(|binding| binding.source)
        .collect()
}

/// One report per declared device family, each carrying every source the
/// designed map binds in the flight context.
fn full_reports(map: &ActionMap) -> Vec<DeviceEvent> {
    DeviceClass::ALL
        .iter()
        .map(|class| report(*class, &sources_of(map, *class)))
        .collect()
}

/// An idle report per declared device family: every source released and
/// every axis back at rest.
fn idle_reports() -> Vec<DeviceEvent> {
    DeviceClass::ALL
        .iter()
        .map(|class| report(*class, &[]))
        .collect()
}

/// One render frame through the production loop. A refused pump is a failure
/// of the fixture, not of the case under test, so it is asserted here.
fn pump(session: &mut InputSession, events: &[DeviceEvent], ticks: u64) -> FrameOutcome {
    session
        .pump_frame(FrameInput::Devices(events), ticks)
        .expect("the fixture frame applies")
}

/// The binding of `map` that drives `command` in the flight context, or
/// `None` when the declared command cannot be reached at all.
fn binding_for(map: &ActionMap, command: FlightCommand) -> Option<Binding> {
    flight_bindings(map).find(|binding| binding.target.action() == Action::Flight(command))
}

/// **All declared device families drive the simulation.**
///
/// Each of `keyboard`, `mouse`, `gamepad` and `joystick` is exercised through
/// its own adapter with every source the designed map binds for it: an edge
/// target must arrive as exactly one delivered command, an axis target must
/// arrive at the consumer's buffer as the reading times the binding's scale,
/// and releasing the source must return the axis to exactly neutral without
/// re-firing the edge. A family the map binds nothing to fails the first
/// assertion, so a declared family can never silently stop being reachable.
#[test]
fn accept_f22_d_every_declared_device_family_drives_the_simulation() {
    let map = ActionMap::designed_default();

    for class in DeviceClass::ALL {
        let sources = sources_of(&map, *class);
        assert!(
            !sources.is_empty(),
            "the designed default gives the declared family {class} at least one \
             binding, otherwise the family is declared but unreachable"
        );

        let mut session = session();
        for source in sources {
            let binding = flight_bindings(&map)
                .find(|binding| binding.source == source)
                .expect("the source came from the map");

            match binding.target {
                BindingTarget::Axis { command, scale } => {
                    let driving = pump(&mut session, &[report(*class, &[source])], 1);
                    assert!(
                        driving.is_clean(),
                        "{class} reports cleanly: {:?}",
                        driving.faults
                    );
                    assert!(
                        driving.delivered.is_empty(),
                        "an axis target is not an edge: {:?}",
                        driving.delivered
                    );
                    let expected = reading(source) * scale;
                    let actual = session
                        .controls()
                        .axis(command)
                        .unwrap_or_else(|| panic!("{source} must drive {command} at the consumer"));
                    assert!(
                        (actual - expected).abs() < QUANTIZATION_TOLERANCE,
                        "{class}'s {source} must drive {command} to {expected}, got {actual}"
                    );

                    let released = pump(&mut session, &[report(*class, &[])], 1);
                    assert!(
                        released.delivered.is_empty(),
                        "releasing {source} must not fire anything: {:?}",
                        released.delivered
                    );
                    assert_eq!(
                        session.controls().axis(command),
                        Some(0.0),
                        "releasing {class}'s {source} returns {command} to exactly neutral"
                    );
                }
                BindingTarget::Command(command) => {
                    let pressing = pump(&mut session, &[report(*class, &[source])], 1);
                    assert!(
                        pressing.is_clean(),
                        "{class} reports cleanly: {:?}",
                        pressing.faults
                    );
                    assert_eq!(
                        pressing.delivered,
                        vec![Action::Flight(command)],
                        "{class}'s {source} must deliver {command} to the consumer"
                    );
                    assert_eq!(
                        session.controls().pending_edges(),
                        0,
                        "the delivered press left nothing queued"
                    );

                    let released = pump(&mut session, &[report(*class, &[])], 1);
                    assert!(
                        released.delivered.is_empty(),
                        "releasing {class}'s {source} must not fire {command} again: {:?}",
                        released.delivered
                    );
                }
                BindingTarget::Ui(_) => {
                    panic!("{class}'s {source} is a flight-context binding, not a UI one")
                }
            }
        }
    }
}

/// **All declared commands reach the consumer.**
///
/// Every `FlightCommand::ALL` entry must have a source in the designed map —
/// a declared command the pilot cannot reach is a coverage hole, not a design
/// choice — and driving that source must be observed where the simulation
/// reads it: an edge in `FrameOutcome::delivered`, an axis in the control
/// buffer, a throttle command as the position it moved the throttle to.
#[test]
fn accept_f22_d_every_declared_flight_command_reaches_the_consumer() {
    let map = ActionMap::designed_default();
    let mut exercised: Vec<FlightCommand> = Vec::new();

    for command in FlightCommand::ALL {
        let binding = binding_for(&map, *command).unwrap_or_else(|| {
            panic!(
                "the declared command {command} has no source in the designed default \
                 map, so nothing can reach the consumer with it"
            )
        });
        let source = binding.source;
        let mut session = session();

        // A throttle command needs a position it can visibly move away from.
        match command {
            FlightCommand::ThrottleStepDown | FlightCommand::ThrottleIdle => session
                .throttle_mut()
                .set_position(ThrottleSteps::FULL)
                .expect("the fixture throttle accepts the full position"),
            FlightCommand::ThrottleStepUp | FlightCommand::ThrottleFull => session
                .throttle_mut()
                .set_position(ThrottleSteps::IDLE)
                .expect("the fixture throttle accepts the idle position"),
            _ => {}
        }

        let outcome = pump(&mut session, &[report(source.device_class(), &[source])], 1);
        assert!(
            outcome.is_clean(),
            "{command} arrives without a fault: {:?}",
            outcome.faults
        );
        assert_eq!(
            session.controls().pending_edges(),
            0,
            "{command} was consumed by the boundary"
        );

        if command.is_continuous() {
            assert!(
                outcome.delivered.is_empty(),
                "{command} is an axis, not an edge: {:?}",
                outcome.delivered
            );
            let expected = reading(source) * binding.target.scale().expect("an axis has a scale");
            let actual = session
                .controls()
                .axis(*command)
                .unwrap_or_else(|| panic!("{command} must reach the control buffer"));
            assert!(
                (actual - expected).abs() < QUANTIZATION_TOLERANCE,
                "{command} must reach the control buffer as {expected}, got {actual}"
            );
        } else {
            assert_eq!(
                outcome.delivered,
                vec![Action::Flight(*command)],
                "{command} must be delivered to the consumer exactly once"
            );
            let position = session.throttle().position();
            match command {
                FlightCommand::ThrottleStepUp => assert!(
                    (position - (ThrottleSteps::IDLE + ThrottleSteps::DESIGNED_STEP)).abs() < 1e-6,
                    "one step up from idle leaves the throttle at {position}"
                ),
                FlightCommand::ThrottleStepDown => assert!(
                    (position - (ThrottleSteps::FULL - ThrottleSteps::DESIGNED_STEP)).abs() < 1e-6,
                    "one step down from full leaves the throttle at {position}"
                ),
                FlightCommand::ThrottleIdle => assert_eq!(
                    position,
                    ThrottleSteps::IDLE,
                    "the direct idle setting wins over anything else in its tick"
                ),
                FlightCommand::ThrottleFull => assert_eq!(
                    position,
                    ThrottleSteps::FULL,
                    "the direct full setting wins over anything else in its tick"
                ),
                _ => {}
            }
        }
        exercised.push(*command);
    }

    assert_eq!(
        exercised,
        FlightCommand::ALL,
        "every declared flight command was exercised, in the vocabulary's own order"
    );
}

/// **All declared UI actions reach the screen path and only the screen path.**
///
/// Each `UiAction::ALL` entry must be bound (a menu action nobody can press
/// is a coverage hole), must arrive as a `UiRequest` stamped with the context
/// that produced it, and must reach neither the control buffer nor the
/// flight-consumer trace. The failure case is text entry: the same source
/// emits nothing there at all, because a text field neither fires weapons nor
/// navigates a menu.
#[test]
fn accept_f22_d_every_declared_ui_action_reaches_only_the_screen_path() {
    let map = ActionMap::designed_default();

    for action in UiAction::ALL {
        let binding = map
            .bindings()
            .iter()
            .find(|binding| binding.target == BindingTarget::Ui(*action))
            .copied()
            .unwrap_or_else(|| {
                panic!(
                    "the declared UI action {action} has no source in the designed \
                     default map, so a screen can never receive it"
                )
            });
        let source = binding.source;
        let class = source.device_class();
        let mut session = session();

        session.set_context(InputContext::UiNavigation);
        let opened = pump(&mut session, &[report(class, &[source])], 1);
        assert!(
            opened.is_clean(),
            "{action} arrives without a fault: {:?}",
            opened.faults
        );
        assert_eq!(opened.ui_requests, 1, "{action} reaches the screen path");
        assert!(
            opened.delivered.is_empty(),
            "a UI action must never reach the flight consumer: {:?}",
            opened.delivered
        );
        assert_eq!(
            session.controls().pending_edges(),
            0,
            "a UI action never enters the control buffer"
        );
        let requests = session.take_ui_requests();
        assert_eq!(requests.len(), 1, "{action} is requested exactly once");
        assert_eq!(requests[0].action, *action);
        assert_eq!(
            requests[0].context,
            InputContext::UiNavigation,
            "a request says which context produced it"
        );
        assert_eq!(requests[0].tick, Tick(0), "and the tick it belongs to");
        let resting = pump(&mut session, &[report(class, &[])], 1);
        assert!(resting.delivered.is_empty());

        // Failure case: text entry accepts neither kind of action.
        session.set_context(InputContext::TextEntry);
        let typing = pump(&mut session, &[report(class, &[source])], 1);
        assert_eq!(
            typing.ui_requests, 0,
            "text entry must not navigate a menu either"
        );
        assert!(
            typing.delivered.is_empty(),
            "nor fire weapons: {:?}",
            typing.delivered
        );
        assert!(session.take_ui_requests().is_empty());
        assert!(
            typing.is_clean(),
            "a text field is not a fault: {:?}",
            typing.faults
        );
    }
}

/// **AC04: open text entry and confirm flight commands are not emitted.**
///
/// The scenario runs the contrast the sheet asks for, with all four declared
/// device families in one session:
///
/// 1. **The path is open** (the control case): one report per family carrying
///    every flight-bound source delivers every edge command the map binds and
///    drives all four continuous axes, so the assertions below cannot pass
///    because nothing works.
/// 2. **A press is queued** by a frame the clock committed no tick to — the
///    ordinary case at a display rate above the tick rate — and the text
///    field opens before the next input boundary.
/// 3. **Text entry emits nothing**: a full report from every family over four
///    ticks, and the consumer sees no flight command, no UI request, no
///    queued press, no axis deflection and no throttle movement. The world
///    keeps ticking, the frame is fault-free, and the press queued before the
///    field opened is reported as discarded rather than delivered later.
/// 4. **A device removed while typing is still reported**, because the device
///    set is maintained even while the readings are not read.
/// 5. **Closing the field restores the path**, and the recorded stream — its
///    edges and its fingerprint — shows that not one flight command was
///    handed over while the field owned the devices.
#[test]
fn accept_f22_d_open_text_entry_and_confirm_flight_commands_are_not_emitted() {
    let map = ActionMap::designed_default();
    let expected = expected_edges(&map);
    assert!(
        expected.len() >= 12,
        "the fixture presses every edge command the map binds, got {expected:?}"
    );
    let full = full_reports(&map);
    let idle = idle_reports();
    let mut session = session();

    // 1. The path is open: every declared family's report delivers every
    // bound edge command and drives every continuous axis.
    let open = pump(&mut session, &full, 1);
    assert!(open.is_clean(), "{:?}", open.faults);
    let mut delivered = open.delivered.clone();
    delivered.sort_unstable();
    delivered.dedup();
    assert_eq!(
        delivered, expected,
        "one report from every declared family delivers every bound edge command"
    );
    for command in FlightCommand::CONTINUOUS {
        let value = session
            .controls()
            .axis(*command)
            .unwrap_or_else(|| panic!("the declared axis {command} is driven"));
        assert!(
            value.abs() > QUANTIZATION_TOLERANCE,
            "{command} is driven while the path is open, got {value}"
        );
    }
    assert_eq!(
        session.throttle().position(),
        ThrottleSteps::FULL,
        "the bound throttle settings moved the throttle"
    );
    let rest = pump(&mut session, &idle, 1);
    assert!(rest.delivered.is_empty());
    assert_eq!(session.controls().pending_edges(), 0);
    for command in FlightCommand::CONTINUOUS {
        assert_eq!(
            session.controls().axis(*command),
            Some(0.0),
            "{command} is neutral again once every family reports idle"
        );
    }

    // 2. A press the clock has not committed a tick to, so it is still queued
    // when the field opens.
    let space = BindingSource::Key(Key::Space);
    let pressed = pump(&mut session, &[report(DeviceClass::Keyboard, &[space])], 0);
    assert!(pressed.delivered.is_empty(), "no tick ran yet");
    assert_eq!(
        session.controls().pending_edges(),
        1,
        "the press is queued for the next input boundary"
    );

    // 3. The field opens. The queued press is discarded and reported, not
    // left for a boundary that would fire it while the pilot is typing.
    let text_start = session.tick();
    session.set_context(InputContext::TextEntry);
    let discarded = session.take_faults();
    assert_eq!(
        discarded.len(),
        1,
        "the press queued before the field opened is reported, not swallowed: {discarded:?}"
    );
    match &discarded[0] {
        InputFault::Suppressed {
            content, reason, ..
        } => {
            assert_eq!(*reason, SuppressReason::Context, "the context refused it");
            assert_eq!(
                content.edges,
                vec![Action::Flight(FlightCommand::FirePrimary)],
                "the discarded press is named"
            );
        }
        other => panic!("a discarded press reports as a suppression, got {other:?}"),
    }
    session.start_recording();

    let typing = pump(&mut session, &full, 4);
    assert_eq!(
        typing.delivered,
        Vec::<Action>::new(),
        "AC04: text entry must not emit flight commands"
    );
    assert_eq!(typing.ui_requests, 0, "nor UI actions");
    assert_eq!(
        session.controls().pending_edges(),
        0,
        "and nothing is left queued to fire after the field closes"
    );
    for command in FlightCommand::CONTINUOUS {
        assert_eq!(
            session.controls().axis(*command),
            Some(0.0),
            "{command} stays exactly neutral while the field owns the devices"
        );
    }
    assert_eq!(
        session.throttle().position(),
        ThrottleSteps::FULL,
        "the throttle does not move while the pilot is typing"
    );
    assert_eq!(typing.ticks_ran, 4, "the world keeps ticking");
    assert_eq!(
        typing.dropped_events,
        full.len(),
        "every family's report is read as dropped input, not as a fault"
    );
    assert!(
        typing.suppressed.is_empty() && typing.suppress_reason.is_none(),
        "an inert frame suppresses nothing: {:?}",
        typing.suppress_reason
    );
    assert!(
        typing.is_clean(),
        "typing is not an input fault: {:?}",
        typing.faults
    );
    assert!(session.take_faults().is_empty());

    // 4. A device unplugged while typing is still a lost device.
    let removal = pump(
        &mut session,
        &[DeviceEvent::Removed {
            device: device(DeviceClass::Joystick),
        }],
        1,
    );
    assert!(removal.delivered.is_empty());
    let losses = session.take_losses();
    assert_eq!(
        losses.len(),
        1,
        "the device set is maintained while the field owns the devices"
    );
    assert_eq!(losses[0].device.class(), DeviceClass::Joystick);
    let text_end = session.tick();

    // 5. Closing the field restores the whole path: the leftover holds are
    // released, and the same reports deliver again. The stick is gone by
    // now, so its family stays silent — a report from a device the session
    // no longer has would be refused by name (F22-B), not silently ignored.
    session.set_context(InputContext::Flight);
    assert!(
        session.take_faults().is_empty(),
        "returning to flight is not a fault"
    );
    let without_stick = |reports: &[DeviceEvent]| -> Vec<DeviceEvent> {
        reports
            .iter()
            .filter(|event| event.device().class() != DeviceClass::Joystick)
            .cloned()
            .collect()
    };
    let released = pump(&mut session, &without_stick(&idle), 1);
    assert!(released.is_clean(), "{:?}", released.faults);
    assert!(released.delivered.is_empty());
    let restored = pump(&mut session, &without_stick(&full), 1);
    assert!(restored.is_clean(), "{:?}", restored.faults);
    let mut restored_delivered = restored.delivered.clone();
    restored_delivered.sort_unstable();
    restored_delivered.dedup();
    assert_eq!(
        restored_delivered, expected,
        "closing the field restores every command the pilot had"
    );

    // The recorded evidence: the stream the input boundary actually produced.
    let stream = session.stop_recording();
    assert!(!stream.is_empty(), "the field's window was recorded");
    let mut text_edges: Vec<Action> = Vec::new();
    for record in stream.records() {
        if record.frame_tick() >= text_start && record.frame_tick() < text_end {
            text_edges.extend(record.edges().iter().copied());
            for axis in record.axes() {
                assert_eq!(
                    axis.quantized(),
                    0,
                    "tick {} commands no axis while the field is open",
                    record.frame_tick().0
                );
            }
        }
    }
    assert!(
        text_edges.is_empty(),
        "the recorded stream holds no flight command made while the field was \
         open: {text_edges:?}"
    );
    assert_eq!(
        stream.edges(),
        restored.delivered,
        "the only commands the boundary handed over in this window are the ones \
         the restored path delivered"
    );
    let fingerprint = stream.fingerprint();
    assert_ne!(
        fingerprint, 0,
        "the recorded stream has a non-trivial fingerprint"
    );
    assert_eq!(
        fingerprint,
        stream.clone().fingerprint(),
        "the fingerprint is stable for the same recorded input"
    );
    let trace: Vec<&str> = stream.edges().iter().map(|action| action.label()).collect();
    println!(
        "F22-D recorded stream: {} ticks, fingerprint {fingerprint:#018x}, \
         consumer trace: {trace:?}",
        stream.len()
    );
}
