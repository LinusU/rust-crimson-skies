//! The Bevy platform adapter of the input session (F22-F).
//!
//! Spec: `specs/F22-input-bindings-devices-and-control-ownership.md`, stages
//! `### F22-B` and `### F22-C`; the task's finding is
//! `docs/findings/2026-10-04-f22-f-bevy-platform-input-seam.md`.
//!
//! [`BevyInputPlugin`] is the one producer of [`DeviceEvent`]s from the engine.
//! Once per frame, after Bevy's own input systems, it
//!
//! 1. enumerates the devices and emits [`DeviceEvent::Connected`] /
//!    [`DeviceEvent::Removed`],
//! 2. reads the keyboard, mouse and gamepad state into one
//!    [`DeviceEvent::KeyboardFrame`] / [`DeviceEvent::MouseFrame`] /
//!    [`DeviceEvent::GamepadFrame`] per device,
//! 3. forwards the window's focus to [`InputSession::set_focus`],
//! 4. converts the frame's real wall time into committed fixed ticks with a
//!    real [`SimClock`] and calls [`InputSession::pump_frame`], and
//! 5. drains the session's UI requests, losses and faults into
//!    [`PlatformFrameReport`].
//!
//! The system owns **no policy**: the context gate, the pause rule, control
//! ownership and fault reporting all stay in [`InputSession`]. The only
//! decision made here is the platform translation (which Bevy key is which
//! `cs_types` key, and what scale a mouse delta is reported in).
//!
//! # Device identity
//!
//! Bevy 0.19 exposes no per-device stable identity: the keyboard and mouse are
//! one aggregate each, and a gamepad entity carries only an optional USB
//! vendor/product pair that two identical pads share. Every device is
//! therefore named with [`DeviceId::enumeration_fallback`] (keyboard and mouse
//! index 0, gamepads by connection order), which reports
//! [`DeviceId::is_stable`] as `false`, and [`PlatformFrameReport::devices`]
//! says which device was named. Calibration is consequently not persisted
//! against a platform device.
//!
//! # Mouse scale (designed, unmeasured)
//!
//! `AccumulatedMouseMotion` is in pixels of cursor motion per frame, which the
//! session refuses outside `[-1, 1]`. The seam divides by
//! [`MOUSE_FULL_DEFLECTION_PIXELS`] and clamps; the wheel is divided by
//! [`WHEEL_FULL_DEFLECTION_LINES`] (or the pixel equivalent) and clamped. Both
//! are designed starting values, not measurements of any platform or of the
//! original game.

use std::time::Duration;

use bevy::input::ButtonInput;
use bevy::input::InputSystems;
use bevy::input::gamepad::{Gamepad, GamepadAxis as BevyGamepadAxis, GamepadButton as BevyPad};
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::{
    AccumulatedMouseMotion, AccumulatedMouseScroll, MouseButton as BevyMouse, MouseScrollUnit,
};
use bevy::prelude::{
    App, Entity, IntoScheduleConfigs, MessageReader, Plugin, PreUpdate, Query, Res, ResMut,
    Resource,
};
use bevy::time::{Real, Time};
use bevy::window::WindowFocused;
use cs_sim::time::{SimClock, TickRate};
use cs_types::input::{
    Action, DeviceClass, DeviceId, GamepadAxis, GamepadButton, Key, MouseButton,
};

use super::devices::{DeviceEvent, DeviceLoss};
use super::session::{FrameInput, FrameOutcome, InputFault, InputSession, UiRequest};

/// Pixels of cursor motion in one frame that report a full deflection.
///
/// Designed, unmeasured; see the module docs.
pub const MOUSE_FULL_DEFLECTION_PIXELS: f32 = 64.0;

/// Wheel lines in one frame that report a full deflection.
pub const WHEEL_FULL_DEFLECTION_LINES: f32 = 4.0;

/// Wheel pixels in one frame that report a full deflection.
pub const WHEEL_FULL_DEFLECTION_PIXELS: f32 = 64.0;

/// One device the seam named, and how.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlatformDevice {
    /// The id reported to the session.
    pub device: DeviceId,
    /// What the platform called it (a gamepad's OS name is not available to
    /// the seam without an event, so this is the platform's own class label).
    pub platform_label: String,
    /// Whether the id is stable; always `false` today, see the module docs.
    pub stable: bool,
}

/// What the last frame's pump produced, for the systems that consume input.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlatformFrameReport {
    /// The session's outcome for the frame, `None` before the first pump or
    /// when the session refused the frame.
    pub outcome: Option<FrameOutcome>,
    /// The error text when the session refused the whole frame.
    pub error: Option<String>,
    /// The flight actions delivered at this frame's boundaries, in tick order.
    pub delivered: Vec<Action>,
    /// The UI requests the frame produced.
    pub ui_requests: Vec<UiRequest>,
    /// The device losses the frame reported.
    pub losses: Vec<DeviceLoss>,
    /// The faults the frame produced.
    pub faults: Vec<InputFault>,
    /// Fixed ticks the clock committed for the frame.
    pub ticks: u64,
    /// Every device the seam has named so far.
    pub devices: Vec<PlatformDevice>,
}

/// The session, its clock and the seam's device bookkeeping.
#[derive(Debug, Resource)]
pub struct PlatformInput {
    session: InputSession,
    clock: SimClock,
    keyboard: Option<DeviceId>,
    mouse: Option<DeviceId>,
    pads: Vec<(Entity, DeviceId)>,
    next_pad: u32,
    report: PlatformFrameReport,
}

impl PlatformInput {
    /// The input session.
    #[must_use]
    pub const fn session(&self) -> &InputSession {
        &self.session
    }

    /// Mutable access to the input session, for the UI and pause paths.
    pub fn session_mut(&mut self) -> &mut InputSession {
        &mut self.session
    }

    /// The clock that converts wall time into fixed ticks.
    #[must_use]
    pub const fn clock(&self) -> &SimClock {
        &self.clock
    }

    /// What the last frame produced.
    #[must_use]
    pub const fn report(&self) -> &PlatformFrameReport {
        &self.report
    }

    fn name_device(&mut self, device: &DeviceId, label: &str) {
        self.report.devices.push(PlatformDevice {
            device: device.clone(),
            platform_label: label.to_owned(),
            stable: device.is_stable(),
        });
    }
}

/// Wires the Bevy input state to an [`InputSession`] every frame.
#[derive(Clone, Debug)]
pub struct BevyInputPlugin {
    session: InputSession,
    rate: TickRate,
}

impl BevyInputPlugin {
    /// A plugin that drives `session` at the fixed tick `rate`.
    #[must_use]
    pub const fn new(session: InputSession, rate: TickRate) -> Self {
        Self { session, rate }
    }
}

impl Plugin for BevyInputPlugin {
    fn build(&self, app: &mut App) {
        let clock = SimClock::with_tick(
            self.session.mode().clock_policy(),
            self.rate,
            self.session.tick(),
        );
        app.add_message::<WindowFocused>()
            .insert_resource(PlatformInput {
                session: self.session.clone(),
                clock,
                keyboard: None,
                mouse: None,
                pads: Vec::new(),
                next_pad: 0,
                report: PlatformFrameReport::default(),
            })
            .add_systems(PreUpdate, pump_platform_input.after(InputSystems));
    }
}

/// Translates a Bevy key; `None` for a key the game has no name for.
#[must_use]
pub const fn key_from_bevy(key: KeyCode) -> Option<Key> {
    Some(match key {
        KeyCode::KeyW => Key::W,
        KeyCode::KeyA => Key::A,
        KeyCode::KeyS => Key::S,
        KeyCode::KeyD => Key::D,
        KeyCode::KeyQ => Key::Q,
        KeyCode::KeyE => Key::E,
        KeyCode::KeyR => Key::R,
        KeyCode::KeyF => Key::F,
        KeyCode::KeyZ => Key::Z,
        KeyCode::KeyX => Key::X,
        KeyCode::KeyC => Key::C,
        KeyCode::KeyB => Key::B,
        KeyCode::KeyG => Key::G,
        KeyCode::KeyT => Key::T,
        KeyCode::KeyL => Key::L,
        KeyCode::Space => Key::Space,
        KeyCode::ShiftLeft => Key::LeftShift,
        KeyCode::ControlLeft => Key::LeftControl,
        KeyCode::AltLeft => Key::LeftAlt,
        KeyCode::Escape => Key::Escape,
        KeyCode::Enter => Key::Enter,
        KeyCode::Tab => Key::Tab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::ArrowUp => Key::ArrowUp,
        KeyCode::ArrowDown => Key::ArrowDown,
        KeyCode::ArrowLeft => Key::ArrowLeft,
        KeyCode::ArrowRight => Key::ArrowRight,
        KeyCode::Digit1 => Key::Digit1,
        KeyCode::Digit2 => Key::Digit2,
        KeyCode::Digit3 => Key::Digit3,
        KeyCode::Digit4 => Key::Digit4,
        _ => return None,
    })
}

const fn mouse_button_from_bevy(button: BevyMouse) -> Option<MouseButton> {
    Some(match button {
        BevyMouse::Left => MouseButton::Left,
        BevyMouse::Right => MouseButton::Right,
        BevyMouse::Middle => MouseButton::Middle,
        BevyMouse::Back => MouseButton::Back,
        BevyMouse::Forward => MouseButton::Forward,
        BevyMouse::Other(_) => return None,
    })
}

const fn pad_button_from_bevy(button: BevyPad) -> Option<GamepadButton> {
    Some(match button {
        BevyPad::South => GamepadButton::South,
        BevyPad::East => GamepadButton::East,
        BevyPad::West => GamepadButton::West,
        BevyPad::North => GamepadButton::North,
        BevyPad::LeftTrigger => GamepadButton::LeftBumper,
        BevyPad::RightTrigger => GamepadButton::RightBumper,
        BevyPad::LeftThumb => GamepadButton::LeftStick,
        BevyPad::RightThumb => GamepadButton::RightStick,
        BevyPad::DPadUp => GamepadButton::DpadUp,
        BevyPad::DPadDown => GamepadButton::DpadDown,
        BevyPad::DPadLeft => GamepadButton::DpadLeft,
        BevyPad::DPadRight => GamepadButton::DpadRight,
        BevyPad::Start => GamepadButton::Start,
        BevyPad::Select => GamepadButton::Select,
        _ => return None,
    })
}

/// A finite value scaled by `full` and clamped into `[-1, 1]`; a non-finite
/// reading is reported as no movement rather than refusing the whole report.
fn scale(value: f32, full: f32) -> f32 {
    if value.is_finite() {
        (value / full).clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// The per-frame system. See the module docs.
#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
fn pump_platform_input(
    mut platform: ResMut<PlatformInput>,
    time: Res<Time<Real>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mouse_buttons: Option<Res<ButtonInput<BevyMouse>>>,
    motion: Option<Res<AccumulatedMouseMotion>>,
    scroll: Option<Res<AccumulatedMouseScroll>>,
    pads: Query<(Entity, &Gamepad)>,
    mut focus: MessageReader<WindowFocused>,
) {
    let platform = &mut *platform;
    let mut events = Vec::new();

    if platform.keyboard.is_none() && keys.is_some() {
        let device = DeviceId::enumeration_fallback(DeviceClass::Keyboard, 0);
        platform.name_device(&device, "keyboard");
        events.push(DeviceEvent::Connected {
            device: device.clone(),
        });
        platform.keyboard = Some(device);
    }
    if platform.mouse.is_none() && (mouse_buttons.is_some() || motion.is_some()) {
        let device = DeviceId::enumeration_fallback(DeviceClass::Mouse, 0);
        platform.name_device(&device, "mouse");
        events.push(DeviceEvent::Connected {
            device: device.clone(),
        });
        platform.mouse = Some(device);
    }

    // A gamepad entity that vanished was removed; one that is new is connected.
    let mut removed = Vec::new();
    platform.pads.retain(|(entity, device)| {
        let present = pads.get(*entity).is_ok();
        if !present {
            removed.push(device.clone());
        }
        present
    });
    for device in removed {
        events.push(DeviceEvent::Removed { device });
    }
    for (entity, _) in &pads {
        if platform.pads.iter().all(|(known, _)| *known != entity) {
            let device = DeviceId::enumeration_fallback(DeviceClass::Gamepad, platform.next_pad);
            platform.next_pad += 1;
            platform.name_device(&device, "gamepad");
            events.push(DeviceEvent::Connected {
                device: device.clone(),
            });
            platform.pads.push((entity, device));
        }
    }

    if let (Some(device), Some(keys)) = (&platform.keyboard, &keys) {
        events.push(DeviceEvent::KeyboardFrame {
            device: device.clone(),
            keys: keys
                .get_pressed()
                .filter_map(|k| key_from_bevy(*k))
                .collect(),
        });
    }
    if let Some(device) = &platform.mouse {
        let (wheel_unit, wheel) = scroll
            .as_ref()
            .map_or((MouseScrollUnit::Line, 0.0), |s| (s.unit, s.delta.y));
        let wheel_full = match wheel_unit {
            MouseScrollUnit::Line => WHEEL_FULL_DEFLECTION_LINES,
            MouseScrollUnit::Pixel => WHEEL_FULL_DEFLECTION_PIXELS,
        };
        let delta = motion.as_ref().map_or(bevy::math::Vec2::ZERO, |m| m.delta);
        events.push(DeviceEvent::MouseFrame {
            device: device.clone(),
            buttons: mouse_buttons.as_ref().map_or_else(Vec::new, |b| {
                b.get_pressed()
                    .filter_map(|b| mouse_button_from_bevy(*b))
                    .collect()
            }),
            motion_x: scale(delta.x, MOUSE_FULL_DEFLECTION_PIXELS),
            motion_y: scale(delta.y, MOUSE_FULL_DEFLECTION_PIXELS),
            wheel: scale(wheel, wheel_full),
        });
    }
    for (entity, device) in &platform.pads {
        let Ok((_, pad)) = pads.get(*entity) else {
            continue;
        };
        let mut axes = Vec::new();
        for (bevy_axis, axis) in [
            (BevyGamepadAxis::LeftStickX, GamepadAxis::LeftStickX),
            (BevyGamepadAxis::LeftStickY, GamepadAxis::LeftStickY),
            (BevyGamepadAxis::RightStickX, GamepadAxis::RightStickX),
            (BevyGamepadAxis::RightStickY, GamepadAxis::RightStickY),
        ] {
            if let Some(value) = pad.get(bevy_axis) {
                axes.push((axis, value));
            }
        }
        // Bevy reports the analog triggers as the analog value of the
        // `LeftTrigger2`/`RightTrigger2` buttons.
        for (button, axis) in [
            (BevyPad::LeftTrigger2, GamepadAxis::LeftTrigger),
            (BevyPad::RightTrigger2, GamepadAxis::RightTrigger),
        ] {
            if let Some(value) = pad.get(button) {
                axes.push((axis, value.clamp(0.0, 1.0)));
            }
        }
        events.push(DeviceEvent::GamepadFrame {
            device: device.clone(),
            buttons: pad
                .get_pressed()
                .filter_map(|b| pad_button_from_bevy(*b))
                .collect(),
            axes,
        });
    }

    for message in focus.read() {
        platform.session.set_focus(message.focused);
    }

    // A paused session runs no boundary, so the clock does not move either
    // (`PausePolicy::Freeze`); `CommandReplay::frame` does the same.
    let elapsed: Duration = time.delta();
    let ticks = if platform.session.is_paused() {
        0
    } else {
        platform.clock.advance(elapsed).unwrap_or(0)
    };
    let (outcome, error) = match platform
        .session
        .pump_frame(FrameInput::Devices(&events), ticks)
    {
        Ok(outcome) => (Some(outcome), None),
        Err(error) => (None, Some(error.to_string())),
    };
    let devices = std::mem::take(&mut platform.report.devices);
    platform.report = PlatformFrameReport {
        delivered: outcome
            .as_ref()
            .map_or_else(Vec::new, |o| o.delivered.clone()),
        ui_requests: platform.session.take_ui_requests(),
        losses: platform.session.take_losses(),
        faults: platform.session.take_faults(),
        ticks,
        devices,
        outcome,
        error,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::session::{PauseReason, SessionMode};
    use bevy::prelude::MinimalPlugins;
    use bevy::time::TimeUpdateStrategy;
    use cs_sim::control::LocalSeatId;
    use cs_types::Tick;
    use cs_types::input::{FlightCommand, InputContext};

    /// Wall time of one test frame: two ticks at 64 Hz.
    const FRAME: Duration = Duration::from_micros(31_250);

    fn app(mode: SessionMode) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<BevyMouse>>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<AccumulatedMouseScroll>()
            .add_plugins(BevyInputPlugin::new(
                InputSession::designed_default(LocalSeatId(0), mode, Tick(0)),
                TickRate::new(64).expect("64 Hz is a valid rate"),
            ));
        // The first update has a zero real delta.
        app.update();
        app
    }

    fn fire_edges(app: &App) -> usize {
        app.world()
            .resource::<PlatformInput>()
            .report()
            .delivered
            .iter()
            .filter(|a| **a == Action::Flight(FlightCommand::FirePrimary))
            .count()
    }

    #[test]
    fn accept_f22_f_held_key_produces_one_fire_edge_at_one_boundary() {
        let mut app = app(SessionMode::SinglePlayer);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);
        let mut total = 0;
        let mut boundaries = 0;
        for _ in 0..4 {
            app.update();
            let platform = app.world().resource::<PlatformInput>();
            assert_eq!(platform.report().ticks, 2, "one frame is two fixed ticks");
            assert!(platform.report().faults.is_empty());
            let edges = fire_edges(&app);
            boundaries += usize::from(edges > 0);
            total += edges;
        }
        assert_eq!(
            total, 1,
            "a held key is one edge, not one per tick or frame"
        );
        assert_eq!(boundaries, 1);
        let platform = app.world().resource::<PlatformInput>();
        assert_eq!(platform.session().tick(), platform.clock().tick());
        let named = &platform.report().devices;
        assert!(
            named
                .iter()
                .any(|d| d.platform_label == "keyboard" && !d.stable),
            "the report says the keyboard was named by enumeration fallback"
        );
    }

    #[test]
    fn accept_f22_f_mouse_motion_is_scaled_so_the_report_is_not_refused() {
        let mut app = app(SessionMode::SinglePlayer);
        app.world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = bevy::math::Vec2::new(500.0, -500.0);
        app.world_mut()
            .resource_mut::<ButtonInput<BevyMouse>>()
            .press(BevyMouse::Left);
        app.update();
        let report = app.world().resource::<PlatformInput>().report();
        assert!(report.faults.is_empty(), "{:?}", report.faults);
        assert_eq!(fire_edges(&app), 1, "the button survived the motion");
    }

    #[test]
    fn accept_f22_f_focus_lost_pauses_single_player_and_neutralizes_multiplayer() {
        for (mode, paused) in [
            (SessionMode::SinglePlayer, true),
            (SessionMode::Multiplayer, false),
        ] {
            let mut app = app(mode);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Space);
            let window = app.world_mut().spawn_empty().id();
            app.world_mut().write_message(WindowFocused {
                window,
                focused: false,
            });
            app.update();
            let platform = app.world().resource::<PlatformInput>();
            let session = platform.session();
            assert!(!session.is_focused());
            assert_eq!(session.is_paused(), paused, "{mode}");
            if paused {
                assert_eq!(session.pause_reason(), Some(PauseReason::FocusLost));
                assert_eq!(platform.report().ticks, 0, "a paused clock is frozen");
            }
            assert_eq!(session.context(), InputContext::Cinematic);
            assert_eq!(
                fire_edges(&app),
                0,
                "a press during focus loss drives nothing"
            );
        }
    }
}
