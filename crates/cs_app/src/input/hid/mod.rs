//! Host joysticks read and driven through raw HID: input for every HID joystick,
//! and USB PID force feedback for the Microsoft SideWinder Force Feedback 2 and
//! other PID devices (SIDEWINDER-FFB2, #1968).
//!
//! Bevy's gamepad input (gilrs) presents a joystick as a gamepad, which has no
//! twist or throttle-lever axes of its own and no force feedback at all, and on
//! macOS the OS offers no force-feedback API for a USB PID stick. This module
//! therefore reads and drives the device itself, through its HID report
//! descriptor:
//!
//! * [`descriptor`] parses the report descriptor; [`layout`] decodes the
//!   controls from it; [`effect`] and [`pid`] are the force-feedback effect model
//!   and its USB PID driver; [`device`] is the hidapi thread that owns the
//!   device.
//! * [`HostJoystickPlugin`] runs that thread and hands its reports to the
//!   F22 seam: [`crate::input::platform::pump_platform_input`] takes the frame's
//!   [`DeviceEvent`]s from [`HostJoysticks::take_events`] and skips Bevy's own
//!   gamepad for the same device, so the stick is one input source, not two.
//! * [`HostJoysticks`] also carries the [`FlightFeel`] force feedback, which the
//!   mission drives from the flight's state
//!   ([`crate::mission_session::flight_feel`]).
//!
//! Input and force feedback are independent: a stick whose motors cannot be
//! taken over still flies, and the reason is reported.

pub mod descriptor;
pub mod device;
pub mod effect;
pub mod layout;
pub mod pid;
pub mod probe;

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use bevy::app::AppExit;
use bevy::prelude::{App, Last, MessageReader, Plugin, ResMut, Resource};
use cs_types::input::{
    AxisChannel, DeviceClass, DeviceId, JOYSTICK_AXIS_THROTTLE, JOYSTICK_AXIS_TWIST,
    JOYSTICK_AXIS_X, JOYSTICK_AXIS_Y,
};

use self::device::{Candidate, JoystickMessage, Options, Running};
use self::effect::Command;
use self::layout::{HatDirection, JoystickState};
use crate::force_feedback::{
    FeelGate, FlightCues, FlightFeel, ForceFeedbackOutput, ForceFeedbackSettings,
};
use crate::input::devices::{DeviceEvent, designed_axis_calibration};
use crate::input::session::InputSession;

/// How the game should use host joysticks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostJoystickOptions {
    /// Whether to read host joysticks at all.
    pub enabled: bool,
    /// Which joystick to use ("045e:001b" or part of its name); the first otherwise.
    pub filter: Option<String>,
    /// Force feedback.
    pub force_feedback: ForceFeedbackSettings,
}

impl Default for HostJoystickOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            filter: None,
            force_feedback: ForceFeedbackSettings::default(),
        }
    }
}

/// The joystick the thread has open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectedJoystick {
    /// The device as the host listed it.
    pub info: Candidate,
    /// The id the input session knows it by.
    pub device: DeviceId,
    /// Whether its force feedback was taken over.
    pub force_feedback: bool,
}

/// The device id of a joystick: its stable identity where it has one, else an
/// explicitly unstable enumeration fallback.
#[must_use]
pub fn joystick_device_id(info: &Candidate, fallback_index: u32) -> DeviceId {
    info.stable_identity()
        .and_then(|identity| DeviceId::stable(DeviceClass::Joystick, &identity).ok())
        .unwrap_or_else(|| DeviceId::enumeration_fallback(DeviceClass::Joystick, fallback_index))
}

/// Sends force-feedback commands to the joystick thread.
struct ThreadOutput<'a>(Option<&'a Sender<Command>>);

impl ForceFeedbackOutput for ThreadOutput<'_> {
    fn send(&mut self, command: Command) {
        if let Some(tx) = self.0 {
            let _ = tx.send(command);
        }
    }
}

/// The host joystick: its thread, its latest controls and its force feedback.
#[derive(Resource)]
pub struct HostJoysticks {
    options: HostJoystickOptions,
    running: Option<Running>,
    messages: Mutex<Option<Receiver<JoystickMessage>>>,
    connected: Option<ConnectedJoystick>,
    state: JoystickState,
    pending: Vec<DeviceEvent>,
    status: Vec<String>,
    connections: u32,
    feel: FlightFeel,
}

impl std::fmt::Debug for HostJoysticks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostJoysticks")
            .field("options", &self.options)
            .field("running", &self.running.is_some())
            .field("connected", &self.connected)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl HostJoysticks {
    /// A joystick seam fed by `messages` with no device thread, for a caller that
    /// produces the thread's messages itself (a test, a replay).
    #[must_use]
    pub fn from_messages(
        options: HostJoystickOptions,
        messages: Receiver<JoystickMessage>,
    ) -> Self {
        Self {
            options,
            running: None,
            messages: Mutex::new(Some(messages)),
            connected: None,
            state: JoystickState::default(),
            pending: Vec::new(),
            status: Vec::new(),
            connections: 0,
            feel: FlightFeel::new(),
        }
    }

    /// Start the device thread (when `options.enabled`).
    #[must_use]
    pub fn start(options: HostJoystickOptions) -> Self {
        if !options.enabled {
            let (_, rx) = channel();
            return Self::from_messages(options, rx);
        }
        let (tx, rx) = channel();
        let force_feedback = options
            .force_feedback
            .enabled
            .then_some(options.force_feedback.gain_percent);
        let running = device::spawn(
            Options {
                filter: options.filter.clone(),
                force_feedback,
            },
            tx,
        );
        let mut joysticks = Self::from_messages(options, rx);
        joysticks.running = Some(running);
        joysticks
    }

    /// The options the seam runs with.
    #[must_use]
    pub const fn options(&self) -> &HostJoystickOptions {
        &self.options
    }

    /// The open joystick, if any.
    #[must_use]
    pub const fn connected(&self) -> Option<&ConnectedJoystick> {
        self.connected.as_ref()
    }

    /// The latest controls of the open joystick.
    #[must_use]
    pub const fn state(&self) -> &JoystickState {
        &self.state
    }

    /// The hat's direction, `None` when centred or with no joystick.
    #[must_use]
    pub fn hat(&self) -> Option<HatDirection> {
        self.connected.as_ref().and(self.state.hat)
    }

    /// Everything the thread reported for the player, oldest first.
    #[must_use]
    pub fn status(&self) -> &[String] {
        &self.status
    }

    /// The force-feedback state.
    #[must_use]
    pub const fn feel(&self) -> &FlightFeel {
        &self.feel
    }

    /// Whether `vid:pid` is the model of the open joystick, so Bevy's gamepad for
    /// the same device is skipped.
    #[must_use]
    pub fn owns_model(&self, vid: Option<u16>, pid: Option<u16>) -> bool {
        self.connected
            .as_ref()
            .is_some_and(|c| Some(c.info.vid) == vid && Some(c.info.pid) == pid)
    }

    /// Read what the thread reported since the last frame. Gives the joystick's
    /// axes their calibration in `session` when it connects.
    pub fn drain(&mut self, session: &mut InputSession) {
        let messages: Vec<JoystickMessage> = {
            let guard = self
                .messages
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard
                .as_ref()
                .map(|rx| rx.try_iter().collect())
                .unwrap_or_default()
        };
        for message in messages {
            match message {
                JoystickMessage::Connected {
                    info,
                    axes,
                    buttons,
                    hat,
                    force_feedback,
                    force_feedback_error,
                } => {
                    if let Some(old) = self.connected.take() {
                        self.pending
                            .push(DeviceEvent::Removed { device: old.device });
                    }
                    let device = joystick_device_id(&info, self.connections);
                    self.connections += 1;
                    calibrate(session, &device);
                    let ff = if force_feedback {
                        format!(
                            "force feedback at {}%",
                            self.options.force_feedback.gain_percent
                        )
                    } else if let Some(error) = &force_feedback_error {
                        format!("no force feedback ({error})")
                    } else if self.options.force_feedback.enabled {
                        "no force feedback".to_owned()
                    } else {
                        "force feedback off".to_owned()
                    };
                    self.note(format!(
                        "joystick: {info} connected as {}: {} axes, {buttons} buttons{}, {ff}",
                        device.identity().label(),
                        axes.len(),
                        if hat { ", hat" } else { "" },
                    ));
                    self.pending.push(DeviceEvent::Connected {
                        device: device.clone(),
                    });
                    self.state = JoystickState::default();
                    self.connected = Some(ConnectedJoystick {
                        info,
                        device,
                        force_feedback,
                    });
                }
                JoystickMessage::State(state) => self.state = state,
                JoystickMessage::Disconnected { info, reason } => {
                    self.note(format!("joystick: {info} disconnected ({reason})"));
                    if let Some(old) = self.connected.take() {
                        self.pending
                            .push(DeviceEvent::Removed { device: old.device });
                    }
                    self.state = JoystickState::default();
                    // The thread dropped the device (and with it every effect); the
                    // next device starts from nothing.
                    self.feel.stop(&mut Vec::new());
                }
                JoystickMessage::Status(text) => self.note(text),
            }
        }
    }

    fn note(&mut self, text: String) {
        bevy::log::info!("{text}");
        self.status.push(text);
    }

    /// The frame's device events: connections and removals since the last frame,
    /// then the open joystick's controls.
    pub fn take_events(&mut self) -> Vec<DeviceEvent> {
        let mut events = std::mem::take(&mut self.pending);
        if let Some(connected) = &self.connected {
            events.push(self.state.to_event(connected.device.clone()));
        }
        events
    }

    /// One frame of force feedback: flight is `gate` and did `cues`.
    pub fn update_feel(&mut self, gate: FeelGate, dt_s: f32, cues: &FlightCues) {
        let gate = if self.connected.as_ref().is_some_and(|c| c.force_feedback) {
            gate
        } else {
            FeelGate::NoDevice
        };
        let tx = self.running.as_ref().map(|r| &r.commands);
        self.feel.update(gate, dt_s, cues, &mut ThreadOutput(tx));
    }

    /// Stop every effect now.
    pub fn stop_feel(&mut self) {
        let tx = self.running.as_ref().map(|r| &r.commands);
        self.feel.stop(&mut ThreadOutput(tx));
    }
}

/// The designed calibration of a joystick's axes: the stick's three axes get the
/// designed dead zone around their centre, and the throttle lever none, so every
/// lever position is a throttle setting and both ends are reached exactly.
fn calibrate(session: &mut InputSession, device: &DeviceId) {
    let store = session.collector_mut().devices_mut().calibration_mut();
    for axis in [JOYSTICK_AXIS_X, JOYSTICK_AXIS_Y, JOYSTICK_AXIS_TWIST] {
        store.set(
            device,
            AxisChannel::Joystick(axis),
            designed_axis_calibration(),
        );
    }
    store.set(
        device,
        AxisChannel::Joystick(JOYSTICK_AXIS_THROTTLE),
        cs_types::input::AxisCalibration::designed_default(),
    );
}

/// Reads the host joystick into the F22 input seam and owns its force feedback.
///
/// Add it next to [`crate::input::platform::BevyInputPlugin`]; without it the
/// seam reads no HID joystick.
#[derive(Clone, Debug, Default)]
pub struct HostJoystickPlugin {
    options: HostJoystickOptions,
}

impl HostJoystickPlugin {
    /// A plugin with `options`.
    #[must_use]
    pub const fn new(options: HostJoystickOptions) -> Self {
        Self { options }
    }
}

impl Plugin for HostJoystickPlugin {
    fn build(&self, app: &mut App) {
        install_panic_reset();
        app.insert_resource(HostJoysticks::start(self.options.clone()))
            .add_systems(Last, stop_feel_on_exit);
    }
}

/// Stops every effect the frame the app is asked to exit, before the window and
/// the device thread go.
fn stop_feel_on_exit(mut exits: MessageReader<AppExit>, mut joysticks: ResMut<HostJoysticks>) {
    if exits.read().next().is_some() {
        joysticks.stop_feel();
    }
}

/// Resets every joystick's force feedback when the game panics, before the
/// default hook reports the panic.
fn install_panic_reset() {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            device::shutdown_all(Duration::from_millis(300));
            previous(info);
        }));
    });
}
