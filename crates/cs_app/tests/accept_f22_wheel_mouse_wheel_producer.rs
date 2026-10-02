//! Acceptance scenarios for F22-I: the third declared mouse axis is a
//! reportable reading.
//!
//! `cs_types::input::MouseAxis` declares `X`, `Y` and `Wheel`, and
//! `AxisChannel::Mouse(MouseAxis::Wheel)` is a calibratable relative channel,
//! but `cs_app::input::DeviceEvent::MouseFrame` used to carry only `motion_x`
//! and `motion_y`: a platform that reports a wheel delta had nowhere to put it
//! and `DeviceAdapters::calibrated_readings` could never calibrate or refuse
//! that reading. This file drives the wheel end to end at the adapter level,
//! where the reading enters the pipeline, and once through the production
//! `InputSession` loop, where it is read by the simulation's control buffer.
//!
//! What makes each scenario discriminating:
//!
//! * If the wheel did **not** reach the calibration pass, a report that
//!   scrolls the wheel would drive nothing at all, and the wheel channel would
//!   be a declared channel no device report can ever reach.
//! * If it were calibrated by a different rule than the motion axes — not
//!   dead-zoned, not curved, or treating "no movement" as a deflection of zero
//!   — the shaped value asserted here would differ from the value the same
//!   `AxisCalibration` produces for any other channel.
//! * If a refused wheel reading did **not** refuse the whole report, the
//!   buttons the same report named would be half-applied: an edge the pilot
//!   pressed would be swallowed, or a hold established behind its back.
//! * If a refused wheel reading left a hold behind, the gun would stay silent
//!   until the button was released and pressed again.
//! * The wheel is deliberately **unbound** in the designed default map
//!   (`ActionMap::designed_default`), so it must drive nothing and add no axis
//!   to the frame — while still being validated, because a platform that
//!   reports a nonsense wheel is a real fault whether or not a binding reads
//!   the channel.
//!
//! Which commands the original 2000 PC game binds to a mouse wheel, and with
//! what sensitivity, is **unknown** and is not claimed here; this stage
//! measures and shapes *this project's* declared channel. Every device
//! identity, binding and reading in this file is newly authored development
//! data. No test in this file needs the original installation.

use cs_app::input::{
    AdapterError, DeviceAdapters, DeviceEvent, FrameInput, InputSession, SessionMode,
};
use cs_sim::control::LocalSeatId;
use cs_types::Tick;
use cs_types::input::{
    Action, ActionMap, AxisCalibration, AxisChannel, Binding, BindingSource, BindingTarget,
    CalibrationError, DeviceClass, DeviceId, FlightCommand, InputContext, InputFrame, MouseAxis,
    MouseButton, ResponseCurve,
};

/// The scroll a report carries, in the mouse's own device units scaled into
/// `[-1, 1]`, the same units and sign convention as its `motion_x`/`motion_y`.
const WHEEL_SCROLL: f32 = 0.6;
/// A wheel reading inside the fixture's wide dead zone.
const WHEEL_IN_DEAD_ZONE: f32 = 0.4;
/// How far a driven axis may differ from the value the report asked for:
/// `AxisValue` quantizes to `i16`, about 3e-5 of full scale.
const QUANTIZATION_TOLERANCE: f32 = 1e-3;

/// The wheel channel, as the vocabulary names it.
const WHEEL_CHANNEL: AxisChannel = AxisChannel::Mouse(MouseAxis::Wheel);

/// The pilot's seat.
fn seat() -> LocalSeatId {
    LocalSeatId(0)
}

/// The mouse of the fixture, with the stable identity a real platform reports.
fn mouse() -> DeviceId {
    DeviceId::stable(DeviceClass::Mouse, "mouse.fixture/0").expect("the fixture identity is valid")
}

/// A mouse report carrying exactly these deltas, the way a render loop polls
/// one device's whole state.
fn mouse_frame(device: &DeviceId, buttons: Vec<MouseButton>, wheel: f32) -> DeviceEvent {
    DeviceEvent::MouseFrame {
        device: device.clone(),
        buttons,
        motion_x: 0.0,
        motion_y: 0.0,
        wheel,
    }
}

/// An action map for a mouse family that is fully wired: the two motion axes
/// to two continuous commands, the wheel to a third, and the left button to
/// one edge. It is the map F22-I needs so a wheel reading has a command to
/// reach *and* a motion reading has a different one, which is what makes "the
/// wheel did not leak into the motion channels" a checkable claim. The designed
/// default map deliberately leaves the wheel unbound, which
/// `accept_f22_wheel_an_unbound_wheel_drives_nothing_and_is_still_validated`
/// pins separately.
fn wheel_map() -> ActionMap {
    ActionMap::try_new(vec![
        Binding {
            source: BindingSource::MouseAxis(MouseAxis::X),
            target: BindingTarget::Axis {
                command: FlightCommand::Yaw,
                scale: 1.0,
            },
        },
        Binding {
            source: BindingSource::MouseAxis(MouseAxis::Y),
            target: BindingTarget::Axis {
                command: FlightCommand::Pitch,
                scale: 1.0,
            },
        },
        Binding {
            source: BindingSource::MouseAxis(MouseAxis::Wheel),
            target: BindingTarget::Axis {
                command: FlightCommand::Roll,
                scale: 1.0,
            },
        },
        Binding {
            source: BindingSource::MouseButton(MouseButton::Left),
            target: BindingTarget::Command(FlightCommand::FirePrimary),
        },
    ])
    .expect("four bindings that touch different targets are well formed")
}

/// Adapters with the fixture mouse connected, ready for one report.
fn connected() -> DeviceAdapters {
    let mut adapters = DeviceAdapters::new();
    adapters
        .connect(mouse())
        .expect("the fixture mouse connects");
    adapters
}

/// The fixture's own wheel calibration: a wide dead zone and a linear curve, so
/// the shaped value is visibly the calibration's doing rather than a
/// coincidence of the transparent default.
fn wheel_calibration() -> AxisCalibration {
    AxisCalibration::try_new(0.5, false, ResponseCurve::Linear, 1.0, 0.5)
        .expect("the fixture calibration is valid")
}

/// A scrolled wheel reaches the calibration pass and is shaped by the wheel
/// channel's own calibration, exactly as any other relative channel is.
///
/// The assertions read the value the *same* `AxisCalibration` produces for the
/// raw reading, so the wheel cannot be calibrated by a different rule than the
/// motion axes: no dead zone, no curve or a different saturation all show up as
/// a mismatch.
#[test]
fn accept_f22_wheel_a_scrolled_wheel_is_calibrated_like_any_relative_channel() {
    assert!(
        WHEEL_CHANNEL.is_relative(),
        "the wheel is a relative channel: a report carries a delta, not a position"
    );
    assert_eq!(
        AxisChannel::from_source(BindingSource::MouseAxis(MouseAxis::Wheel)),
        Some(WHEEL_CHANNEL),
        "the wheel source resolves to the wheel channel"
    );

    let map = wheel_map();
    let device = mouse();
    let calibration = wheel_calibration();
    let mut adapters = connected();
    adapters
        .calibration_mut()
        .set(&device, WHEEL_CHANNEL, calibration);

    // The wheel is scrolled and the mouse itself is still.
    let mut frame = InputFrame::new(Tick(1));
    adapters
        .apply(
            &mouse_frame(&device, vec![], WHEEL_SCROLL),
            &map,
            InputContext::Flight,
            &mut frame,
        )
        .expect("a scrolled-wheel report applies");
    let roll = frame
        .axis(FlightCommand::Roll)
        .expect("the wheel reading drives the bound axis");
    let expected = calibration
        .apply(WHEEL_SCROLL)
        .expect("a scroll inside the calibrated range is accepted");
    assert!(
        (roll.as_unit() - expected).abs() < QUANTIZATION_TOLERANCE,
        "the bound axis holds the calibrated scroll {expected}, got {}",
        roll.as_unit()
    );
    assert!(
        (expected - 0.2).abs() < 1e-6,
        "the wheel channel's own dead zone shapes the reading: (0.6 - 0.5) / 0.5 = \
         0.2, got {expected}"
    );
    assert!(
        frame.axis(FlightCommand::Yaw).is_none() && frame.axis(FlightCommand::Pitch).is_none(),
        "a wheel reading is not mouse motion: it must not leak into the motion \
         channels, got {:?}",
        frame.axes()
    );
    assert!(frame.edges().is_empty(), "an axis target is not an edge");
    assert_eq!(
        adapters.driven_axes(),
        &[(device.clone(), FlightCommand::Roll)],
        "the device is recorded as driving exactly the axis its reading reached"
    );
    adapters.finish_frame(&mut frame);

    // A scroll inside the same dead zone is shaped to exactly neutral, the way
    // a stick deflection inside its dead zone is.
    let mut inside = InputFrame::new(Tick(2));
    adapters
        .apply(
            &mouse_frame(&device, vec![], WHEEL_IN_DEAD_ZONE),
            &map,
            InputContext::Flight,
            &mut inside,
        )
        .expect("a scroll inside the dead zone applies");
    assert_eq!(
        inside
            .axis(FlightCommand::Roll)
            .expect("the channel is still read")
            .quantized(),
        0,
        "a scroll inside the wheel channel's dead zone is neutral"
    );

    // A wheel that did not move is not a drive at all: a relative channel that
    // did not move reads as nothing, not as a deflection of zero.
    let mut still = InputFrame::new(Tick(3));
    adapters
        .apply(
            &mouse_frame(&device, vec![], 0.0),
            &map,
            InputContext::Flight,
            &mut still,
        )
        .expect("a still wheel applies");
    assert!(
        still.axes().is_empty(),
        "a wheel that did not move adds no axis to the frame, got {:?}",
        still.axes()
    );
    assert!(
        adapters.driven_axes().is_empty(),
        "and the device is recorded as driving nothing"
    );
}

/// A wheel reading no calibration accepts refuses the **whole** report, the
/// buttons in it included, and refuses it by name.
///
/// A report is the device's whole state and may name any number of buttons and
/// readings, so a refusal that applied the buttons first would swallow the
/// press the pilot made in the same report and leave a hold behind that keeps
/// the gun silent until the button is released and pressed again.
#[test]
fn accept_f22_wheel_a_refused_wheel_reading_refuses_the_whole_report() {
    let map = ActionMap::designed_default();
    let device = mouse();
    let mut adapters = connected();

    // The report names the left button *and* a wheel reading outside the
    // calibrated range. The refusal is by device, channel and reason.
    let mut frame = InputFrame::new(Tick(1));
    assert_eq!(
        adapters.apply(
            &mouse_frame(&device, vec![MouseButton::Left], 1.5),
            &map,
            InputContext::Flight,
            &mut frame
        ),
        Err(AdapterError::ReadingRejected {
            device: device.clone(),
            channel: WHEEL_CHANNEL,
            error: CalibrationError::ReadingOutOfRange { value: 1.5 },
        }),
        "an out-of-range wheel reading is refused by device and channel, never \
         saturated behind the pilot's back"
    );
    assert!(
        frame.is_empty(),
        "the refused report contributed nothing, got {:?} / {:?}",
        frame.axes(),
        frame.edges()
    );
    assert!(
        adapters.held_edges().is_empty(),
        "and established no hold from the button it named"
    );
    assert_eq!(
        adapters.reports(),
        0,
        "a refused report is not a report that was read"
    );

    // A wheel reading that is not a number at all is refused the same way, and
    // never propagated into the pipeline.
    let mut frame = InputFrame::new(Tick(2));
    assert!(
        matches!(
            adapters.apply(
                &mouse_frame(&device, vec![MouseButton::Left], f32::NAN),
                &map,
                InputContext::Flight,
                &mut frame
            ),
            Err(AdapterError::ReadingRejected {
                channel: WHEEL_CHANNEL,
                error: CalibrationError::NonFiniteReading { value },
                ..
            }) if value.is_nan()
        ),
        "a NaN wheel reading is refused by channel, never propagated"
    );
    assert!(frame.is_empty());
    assert!(adapters.held_edges().is_empty());
    assert_eq!(adapters.reports(), 0);

    // The refusals swallowed nothing: the very next real report of the same
    // held button is still the pilot's first press, so the gun fires once.
    let mut press = InputFrame::new(Tick(3));
    adapters
        .apply(
            &mouse_frame(&device, vec![MouseButton::Left], 0.0),
            &map,
            InputContext::Flight,
            &mut press,
        )
        .expect("a good report applies");
    assert_eq!(
        press.edges(),
        &[Action::Flight(FlightCommand::FirePrimary)],
        "the press the refused reports named was not swallowed"
    );
    assert_eq!(adapters.held_edges().len(), 1, "and it is held once");
}

/// The refusal is atomic in the other direction too: a mouse that was already
/// holding a button and driving an axis keeps both, and the report counter and
/// the driven-axis record survive, so a later loss still names what the device
/// was driving.
#[test]
fn accept_f22_wheel_a_refused_wheel_reading_leaves_no_partial_state() {
    let map = wheel_map();
    let device = mouse();
    let mut adapters = connected();

    // A good report: the button is held and the wheel drove the axis.
    let mut frame = InputFrame::new(Tick(1));
    adapters
        .apply(
            &DeviceEvent::MouseFrame {
                device: device.clone(),
                buttons: vec![MouseButton::Left],
                motion_x: 0.0,
                motion_y: 0.0,
                wheel: WHEEL_SCROLL,
            },
            &map,
            InputContext::Flight,
            &mut frame,
        )
        .expect("the good report applies");
    adapters.finish_frame(&mut frame);
    assert_eq!(adapters.reports(), 1);
    assert_eq!(adapters.held_edges().len(), 1, "the button is held");
    assert_eq!(
        adapters.driven_axes(),
        &[(device.clone(), FlightCommand::Roll)],
        "the wheel drove the axis"
    );

    // The same report with a wheel reading no calibration accepts.
    let mut frame = InputFrame::new(Tick(2));
    assert!(
        matches!(
            adapters.apply(
                &mouse_frame(&device, vec![MouseButton::Left], f32::INFINITY),
                &map,
                InputContext::Flight,
                &mut frame
            ),
            Err(AdapterError::ReadingRejected {
                channel: WHEEL_CHANNEL,
                error: CalibrationError::NonFiniteReading { value },
                ..
            }) if value.is_infinite()
        ),
        "an infinite wheel reading is refused by channel"
    );
    assert!(frame.is_empty(), "the refused report contributes nothing");
    assert_eq!(
        adapters.reports(),
        1,
        "a refused report is not counted as a report"
    );
    assert_eq!(
        adapters.driven_axes(),
        &[(device.clone(), FlightCommand::Roll)],
        "a refused report does not forget the axis the device was driving"
    );
    assert_eq!(
        adapters.held_edges().len(),
        1,
        "a refused report neither adds nor releases a hold"
    );

    // So a later loss still reports what the device was really holding and
    // driving: the refusal changed nothing the loss depends on.
    adapters.disconnect(&device).expect("the mouse disconnects");
    let losses = adapters.take_losses();
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].neutralized_axes,
        vec![FlightCommand::Roll],
        "the loss names the axis the refused report's device was driving"
    );
    assert_eq!(
        losses[0].released_edges,
        vec![Action::Flight(FlightCommand::FirePrimary)],
        "and the button the refused report named is still reported as released"
    );
}

/// The wheel is unbound in the designed default map, and that is a design
/// decision rather than a hole: an unbound wheel drives no command, adds no
/// axis to the frame and is recorded as driving nothing — while still being
/// validated, because a platform that reports a nonsense wheel is a real fault
/// whether or not a binding reads the channel.
#[test]
fn accept_f22_wheel_an_unbound_wheel_drives_nothing_and_is_still_validated() {
    let map = ActionMap::designed_default();
    let wheel_binding_count = map.bindings_for_channel(WHEEL_CHANNEL).count();
    assert_eq!(
        wheel_binding_count, 0,
        "the designed default map must not bind the wheel: which command a wheel \
         drives, if any, is this project's design decision and F22-B pins that it \
         is unbound"
    );

    let device = mouse();
    let mut adapters = connected();

    // A scrolled wheel with the left button held fires the guns and nothing
    // else.
    let mut frame = InputFrame::new(Tick(1));
    adapters
        .apply(
            &mouse_frame(&device, vec![MouseButton::Left], WHEEL_SCROLL),
            &map,
            InputContext::Flight,
            &mut frame,
        )
        .expect("the report applies");
    assert_eq!(
        frame.edges(),
        &[Action::Flight(FlightCommand::FirePrimary)],
        "the button drives its command and the unbound wheel drives nothing"
    );
    assert!(
        frame.axes().is_empty(),
        "an unbound wheel adds no axis to the frame, got {:?}",
        frame.axes()
    );
    assert!(
        adapters.driven_axes().is_empty(),
        "and the device is recorded as driving nothing"
    );

    // The channel is still calibrated, so a nonsense reading on it is still
    // refused by name even though no binding would ever read it.
    let mut frame = InputFrame::new(Tick(2));
    assert!(
        matches!(
            adapters.apply(
                &mouse_frame(&device, vec![], f32::NAN),
                &map,
                InputContext::Flight,
                &mut frame
            ),
            Err(AdapterError::ReadingRejected {
                channel: WHEEL_CHANNEL,
                ..
            })
        ),
        "an unbound channel is still calibrated: a driver that reports nonsense is \
         a real fault whether or not a binding reads the channel"
    );
    assert_eq!(
        adapters.reports(),
        1,
        "the refused unbound-wheel report is not counted"
    );
}

/// The whole production path: a wheel report is pumped through the real
/// `InputSession` loop and read at the consumer, where the simulation reads
/// its axes, and a wheel that stops moving returns the axis to exactly neutral.
#[test]
fn accept_f22_wheel_a_wheel_report_reaches_the_control_buffer() {
    let mut session = InputSession::new(wheel_map(), seat(), SessionMode::SinglePlayer, Tick(0));
    session
        .collector_mut()
        .connect_device(mouse())
        .expect("the fixture mouse connects");
    // The wheel channel carries the fixture's own calibration, so the value the
    // consumer holds is visibly the calibration's doing.
    let calibration = wheel_calibration();
    session.collector_mut().devices_mut().calibration_mut().set(
        &mouse(),
        WHEEL_CHANNEL,
        calibration,
    );

    let scrolled = session
        .pump_frame(
            FrameInput::Devices(&[mouse_frame(&mouse(), vec![], WHEEL_SCROLL)]),
            1,
        )
        .expect("the scrolled-wheel frame applies");
    assert!(
        scrolled.is_clean(),
        "the frame reports no fault: {scrolled:?}"
    );
    assert!(
        scrolled.delivered.is_empty(),
        "an axis target is not an edge: {:?}",
        scrolled.delivered
    );
    let expected = calibration
        .apply(WHEEL_SCROLL)
        .expect("a scroll inside the calibrated range is accepted");
    let roll = session
        .controls()
        .axis(FlightCommand::Roll)
        .expect("the wheel reading reached the control buffer");
    assert!(
        (roll - expected).abs() < QUANTIZATION_TOLERANCE,
        "the consumer holds the calibrated scroll {expected}, got {roll}"
    );

    // The wheel stops: the next frame states the axis exactly neutral instead
    // of leaving the last scroll in the simulation forever.
    let released = session
        .pump_frame(
            FrameInput::Devices(&[mouse_frame(&mouse(), vec![], 0.0)]),
            1,
        )
        .expect("the still frame applies");
    assert!(
        released.delivered.is_empty(),
        "a still wheel delivers no command: {:?}",
        released.delivered
    );
    assert_eq!(
        session.controls().axis(FlightCommand::Roll),
        Some(0.0),
        "a wheel that stopped returns its axis to exactly neutral"
    );
}
