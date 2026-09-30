//! Device adapters and axis calibration (F22-B).
//!
//! Spec: `specs/F22-input-bindings-devices-and-control-ownership.md`, stage
//! `### F22-B`. Shared contract: `docs/contracts/UI-NETWORK.md` ("Network
//! ownership table": a client owns only local input requests; a UI action
//! requests a domain transaction rather than editing game state).
//!
//! This is the **producer** half of the input boundary, the half F22-A left
//! typed: a [`DeviceEvent`] is what a platform layer (Bevy, `winit`, `gilrs`)
//! reports about one device in one render frame, and [`DeviceAdapters`] turns
//! those events into the calibrated edges and axes of a
//! `cs_types::input::InputFrame`.
//!
//! The rules this module makes structural rather than conventional:
//!
//! 1. **Calibration is keyed by device identity, never by an enumeration index
//!    alone** (non-negotiable behavior 1). Calibration lives in
//!    `cs_types::input::CalibrationStore`, keyed by `(device, axis channel)`
//!    and reached through [`DeviceId::stable`] identities;
//!    [`DeviceLoss::stable_identity`] is `None` for a device that was only
//!    known by its index, so a caller cannot persist its tuning by accident.
//! 2. **A report is level-triggered.** A device report is the full state of
//!    that device, so a source it no longer lists was released, and a held
//!    digital source drives its bound axis at full scale while it is listed
//!    and exactly neutral once it is not. A held *edge* fires once, however
//!    many frames it stays down.
//! 3. **Device removal releases held buttons and neutralizes unsafe controls**
//!    (non-negotiable behavior 3): [`DeviceAdapters::disconnect`] drops the
//!    device's held edges, forgets the axes it drove and records a
//!    [`DeviceLoss`] naming them, so a removed stick cannot leave the guns
//!    firing forever. An edge already handed to a consumer is *not* withdrawn
//!    — the input layer never rewrites a command that was delivered.
//! 4. **An axis no device drives is exactly neutral.** Every frame states the
//!    axes it drives, and [`finish_frame`](DeviceAdapters::finish_frame)
//!    writes an explicit zero for each axis the previous frame drove and this
//!    one does not, so a released key or a removed stick cannot leave a stale
//!    deflection in the simulation.
//! 5. **The context gate still decides.** Every resolved target passes
//!    `cs_types::input::InputContext::accepts`, so text entry and cinematics
//!    emit nothing whatever a device reports (non-negotiable behavior 5).
//!
//! **Designed, not original.** The event vocabulary, the axis channels, the
//! designed dead zone and the composition rules are newly authored project
//! design. Which devices the original 2000 PC game supports, how it names
//! them, and what its stick, trigger and mouse-flying defaults are are
//! **unknown** until F22-D measures them; nothing here claims otherwise
//! (`docs/findings/2026-09-30-f22-b-device-adapters-and-calibration.md`).
//!
//! The module is deliberately ECS-free: it is plain typed state so a headless
//! test can drive it exactly like the render loop, and so no game state hides
//! in UI code (`docs/01-ARCHITECTURE.md`). F22-C wires it to the real platform
//! sources, focus/UI state, replay and control ownership:
//! [`DeviceAdapters::suppress`] is the release path a focus loss, a pause and
//! a control handover use when no device was removed, and the `session` module
//! is the loop that drives these adapters into `cs_sim::control`.

use std::fmt;

use cs_types::input::{
    Action, ActionMap, AxisCalibration, AxisChannel, AxisValue, AxisValueError, BindingSource,
    BindingTarget, CalibrationError, CalibrationStore, CalibrationStoreError, DeviceClass,
    DeviceId, FlightCommand, GamepadAxis, GamepadButton, InputContext, InputFrame, Key, MouseAxis,
    MouseButton, ResponseCurve,
};

/// Why a device adapter operation was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum AdapterError {
    /// A device was registered that the session already has.
    AlreadyConnected {
        /// The device that is already connected.
        device: DeviceId,
    },
    /// An event named a device the session has not registered.
    ///
    /// A stale event from a device that was already removed, or one that never
    /// existed, is refused by name instead of creating state behind the
    /// producer's back.
    NotConnected {
        /// The device the event named.
        device: DeviceId,
    },
    /// The event's variant disagrees with the device's class, so its readings
    /// would be calibrated under the wrong device.
    ClassMismatch {
        /// The class the device reports as.
        expected: DeviceClass,
        /// The class the event's variant implies.
        reported: DeviceClass,
        /// The device.
        device: DeviceId,
    },
    /// A device reported a raw reading no calibration accepts.
    ReadingRejected {
        /// The device that produced it.
        device: DeviceId,
        /// The channel that produced it.
        channel: AxisChannel,
        /// The calibration's own refusal.
        error: CalibrationError,
    },
    /// A binding drives a continuous axis with an edge command, or the other
    /// way round: a refused map, never a silently dropped reading.
    AxisNotContinuous {
        /// The offending command.
        command: FlightCommand,
    },
    /// A calibrated value could not be turned into an axis sample.
    AxisValue {
        /// The command the value was for.
        command: FlightCommand,
        /// The value's own refusal.
        error: AxisValueError,
    },
    /// A calibration re-key was refused.
    CalibrationStore {
        /// The store's own refusal.
        error: CalibrationStoreError,
    },
}

impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyConnected { device } => write!(f, "{device} is already connected"),
            Self::NotConnected { device } => {
                write!(f, "{device} is not connected in this session")
            }
            Self::ClassMismatch {
                expected,
                reported,
                device,
            } => write!(
                f,
                "{device} is a {expected} device, but the event reports {reported}"
            ),
            Self::ReadingRejected {
                device,
                channel,
                error,
            } => write!(f, "{device} reported {channel}: {error}"),
            Self::AxisNotContinuous { command } => {
                write!(f, "{command} cannot be driven as a continuous axis")
            }
            Self::AxisValue { command, error } => write!(f, "the {command} axis: {error}"),
            Self::CalibrationStore { error } => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for AdapterError {}

/// What one device did in one render frame.
///
/// The event carries the device's **raw** readings, not calibrated values, and
/// the caller passes the session's [`InputContext`] separately: the adapter
/// never owns the context, so a menu, a text field and a cinematic cannot
/// disagree with the simulation about who currently owns the devices.
#[derive(Clone, Debug, PartialEq)]
pub enum DeviceEvent {
    /// A device appeared.
    Connected {
        /// The device, with a stable identity where the platform reports one.
        device: DeviceId,
    },
    /// A device went away. The session releases what it held and reports it.
    Removed {
        /// The device that went away.
        device: DeviceId,
    },
    /// A keyboard reported the keys that are down.
    KeyboardFrame {
        /// The keyboard.
        device: DeviceId,
        /// The keys down during this frame.
        keys: Vec<Key>,
    },
    /// A mouse reported its buttons and the motion since the last frame.
    MouseFrame {
        /// The mouse.
        device: DeviceId,
        /// The buttons down during this frame.
        buttons: Vec<MouseButton>,
        /// Horizontal motion since the last frame, in device counts.
        motion_x: f32,
        /// Vertical motion since the last frame, in device counts.
        motion_y: f32,
    },
    /// A gamepad reported its buttons and axes.
    GamepadFrame {
        /// The gamepad.
        device: DeviceId,
        /// The buttons down during this frame.
        buttons: Vec<GamepadButton>,
        /// The axis readings in the driver's own units: the sticks in
        /// `[-1, 1]`, the triggers in `[0, 1]`. See
        /// [`normalize_gamepad_axis`].
        axes: Vec<(GamepadAxis, f32)>,
    },
    /// A joystick or HOTAS reported its buttons and axes.
    JoystickFrame {
        /// The joystick.
        device: DeviceId,
        /// The button indices down during this frame.
        buttons: Vec<u16>,
        /// The raw axis readings in `[-1, 1]`, indexed by axis index.
        axes: Vec<(u16, f32)>,
    },
}

impl DeviceEvent {
    /// The device this event is about.
    #[must_use]
    pub const fn device(&self) -> &DeviceId {
        match self {
            Self::Connected { device }
            | Self::Removed { device }
            | Self::KeyboardFrame { device, .. }
            | Self::MouseFrame { device, .. }
            | Self::GamepadFrame { device, .. }
            | Self::JoystickFrame { device, .. } => device,
        }
    }

    /// The device class the event's variant implies.
    #[must_use]
    pub const fn event_class(&self) -> Option<DeviceClass> {
        match self {
            Self::Connected { .. } | Self::Removed { .. } => None,
            Self::KeyboardFrame { .. } => Some(DeviceClass::Keyboard),
            Self::MouseFrame { .. } => Some(DeviceClass::Mouse),
            Self::GamepadFrame { .. } => Some(DeviceClass::Gamepad),
            Self::JoystickFrame { .. } => Some(DeviceClass::Joystick),
        }
    }
}

/// What a device removal released, and what it stopped driving.
///
/// This is the report non-negotiable behavior 3 asks for: the caller can tell
/// the player which controls the lost device was holding instead of
/// discovering later that a trigger is stuck down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceLoss {
    /// The device that went away.
    pub device: DeviceId,
    /// The edges the removed device was holding, now released. An edge that
    /// was already delivered to a consumer stays delivered; what stops is any
    /// further fire.
    pub released_edges: Vec<Action>,
    /// The continuous commands the removed device was driving. Each is
    /// neutralized by the next frame unless another device drives it.
    pub neutralized_axes: Vec<FlightCommand>,
    /// The device's stable identity, when it had one. `None` for a device that
    /// was only known by its enumeration index, which is exactly the case a
    /// caller must not persist calibration against.
    pub stable_identity: Option<String>,
}

/// What releasing the session's holds released, without removing a device.
///
/// This is the record of a [`suppress`](DeviceAdapters::suppress): a focus
/// loss, a pause, a control handover or a teardown released the edges the
/// devices were holding and forgot the axes they were driving. An edge already
/// delivered to a consumer stays delivered; what stops is any further command.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SuppressedHolds {
    /// The edges that were being held and are now released.
    pub released_edges: Vec<Action>,
    /// The continuous commands the devices were driving. The next finished
    /// frame states each of them neutral.
    pub neutralized_axes: Vec<FlightCommand>,
}

impl SuppressedHolds {
    /// Whether nothing was held and nothing was being driven.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.released_edges.is_empty() && self.neutralized_axes.is_empty()
    }
}

/// One edge a device is holding down.
///
/// The report that established the hold is part of the record because a device
/// report is the device's whole state: a hold the current report did not
/// re-establish was released, and that is how a held trigger both fires once
/// and lets go when the device stops reporting it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldEdge {
    device: DeviceId,
    action: Action,
    established_in: u64,
}

impl HeldEdge {
    /// The device holding the edge.
    #[must_use]
    pub const fn device(&self) -> &DeviceId {
        &self.device
    }

    /// The action the hold keeps active.
    #[must_use]
    pub const fn action(&self) -> Action {
        self.action
    }

    /// The sequence number of the report that last established this hold.
    #[must_use]
    pub const fn established_in(&self) -> u64 {
        self.established_in
    }
}

/// One analog reading of a report, after the channel's calibration.
///
/// The reading is calibrated in a pass of its own
/// ([`DeviceAdapters::calibrated_readings`]) **before** any part of the report
/// is applied, so a report that carries a reading no calibration accepts is
/// refused whole instead of half-applied.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CalibratedReading {
    /// The channel the reading came from.
    channel: AxisChannel,
    /// The calibrated deflection, in `[-1, 1]`.
    value: f32,
    /// Whether the reading counts as pressed for an edge target on this
    /// channel. The threshold is
    /// [`AxisCalibration::activation`](cs_types::input::AxisCalibration::activation),
    /// measured from the channel's resting end
    /// ([`AxisChannel::is_unipolar`](cs_types::input::AxisChannel::is_unipolar)).
    active: bool,
}

/// How a device's state changed relative to the previous frame.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Reading {
    /// A digital source is down.
    Pressed,
    /// A calibrated analog reading, with the decision whether an analog source
    /// counts as pressed for an edge target. The decision is made once per
    /// channel, in [`DeviceAdapters::calibrate`], because it depends on which
    /// end of the channel is its resting end.
    Analog { value: f32, active: bool },
}

/// The session's device adapters: which devices exist, what their axes read
/// as, and what is currently held down.
///
/// Built with an [`ActionMap`] and driven one [`DeviceEvent`] at a time, which
/// is exactly how a Bevy system would read a message stream.
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceAdapters {
    calibration: CalibrationStore,
    connected: Vec<DeviceId>,
    /// The edges currently held down, and the report that established each.
    held_edges: Vec<HeldEdge>,
    /// The number of device reports this session has read. It stamps a hold so
    /// a report that no longer lists a source releases it.
    reports: u64,
    /// The continuous commands each connected device is driving right now.
    driven_axes: Vec<(DeviceId, FlightCommand)>,
    /// The commands the previous finished frame drove, so the next one can
    /// neutralize whatever nothing drives any more.
    driven_last_frame: Vec<FlightCommand>,
    losses: Vec<DeviceLoss>,
}

impl Default for DeviceAdapters {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceAdapters {
    /// Adapters for one session with no device connected and no calibration.
    ///
    /// The action map is not stored here: the session's map is the single
    /// source of truth and is passed to every [`apply`](Self::apply), so the
    /// adapters and the simulation can never resolve a source against
    /// different bindings.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            calibration: CalibrationStore::new(),
            connected: Vec::new(),
            held_edges: Vec::new(),
            reports: 0,
            driven_axes: Vec::new(),
            driven_last_frame: Vec::new(),
            losses: Vec::new(),
        }
    }

    /// The session's calibration records.
    #[must_use]
    pub const fn calibration(&self) -> &CalibrationStore {
        &self.calibration
    }

    /// Mutable access to the session's calibration records, for the settings
    /// path that calibrates a device.
    pub fn calibration_mut(&mut self) -> &mut CalibrationStore {
        &mut self.calibration
    }

    /// The connected devices, in connection order.
    #[must_use]
    pub fn connected(&self) -> &[DeviceId] {
        &self.connected
    }

    /// Whether a device is connected.
    #[must_use]
    pub fn is_connected(&self, device: &DeviceId) -> bool {
        self.connected.contains(device)
    }

    /// How many devices are connected.
    #[must_use]
    pub fn connected_count(&self) -> usize {
        self.connected.len()
    }

    /// The edges currently held down, with the device holding each one.
    #[must_use]
    pub fn held_edges(&self) -> &[HeldEdge] {
        &self.held_edges
    }

    /// How many device reports this session has read.
    #[must_use]
    pub const fn reports(&self) -> u64 {
        self.reports
    }

    /// The continuous commands each device is driving right now.
    #[must_use]
    pub fn driven_axes(&self) -> &[(DeviceId, FlightCommand)] {
        &self.driven_axes
    }

    /// The device losses reported since the last
    /// [`take_losses`](Self::take_losses), oldest first.
    #[must_use]
    pub fn losses(&self) -> &[DeviceLoss] {
        &self.losses
    }

    /// Takes the reported losses, so a caller handles each exactly once.
    pub fn take_losses(&mut self) -> Vec<DeviceLoss> {
        std::mem::take(&mut self.losses)
    }

    /// Registers a device.
    ///
    /// # Errors
    ///
    /// [`AdapterError::AlreadyConnected`] when the session already has it.
    pub fn connect(&mut self, device: DeviceId) -> Result<(), AdapterError> {
        if self.is_connected(&device) {
            return Err(AdapterError::AlreadyConnected { device });
        }
        self.connected.push(device);
        Ok(())
    }

    /// Removes a device: its held edges are released, the axes it drove are
    /// forgotten so the next frame neutralizes them, and a [`DeviceLoss`]
    /// naming both is recorded.
    ///
    /// Its calibration records are **kept**: a device that comes back with the
    /// same stable identity finds its own tuning, and a device that returns
    /// under a different index finds none (that is what device identity means).
    /// Use [`CalibrationStore::forget_device`] to drop them deliberately.
    ///
    /// # Errors
    ///
    /// [`AdapterError::NotConnected`] when the session never had the device.
    pub fn disconnect(&mut self, device: &DeviceId) -> Result<(), AdapterError> {
        let Some(index) = self.connected.iter().position(|known| known == device) else {
            return Err(AdapterError::NotConnected {
                device: device.clone(),
            });
        };
        self.connected.remove(index);

        let mut released_edges = Vec::new();
        self.held_edges.retain(|held| {
            if held.device() == device {
                released_edges.push(held.action());
                false
            } else {
                true
            }
        });

        let mut neutralized_axes = Vec::new();
        self.driven_axes.retain(|(driving, command)| {
            if driving == device {
                neutralized_axes.push(*command);
                false
            } else {
                true
            }
        });

        self.losses.push(DeviceLoss {
            device: device.clone(),
            released_edges,
            neutralized_axes,
            stable_identity: device.stable_identity().map(str::to_owned),
        });
        Ok(())
    }

    /// Adopts the stable identity the platform revealed for a device that was
    /// only known by its enumeration index.
    ///
    /// This is the second half of non-negotiable behavior 1. A stick that
    /// enumerated without an identity is connected under an index; when the
    /// platform later reports its real identity, the session adopts it and
    /// **re-keys everything that was keyed by the index**: the calibration
    /// records (through [`CalibrationStore::promote`]), the connected device,
    /// and the holds and axes the index was driving. Nothing is left behind
    /// under the index, so the same device keeps its calibration, its held
    /// buttons and its control across the reveal instead of losing all three.
    ///
    /// # Errors
    ///
    /// [`AdapterError::NotConnected`] when `provisional` is not connected,
    /// [`AdapterError::AlreadyConnected`] when the session already has
    /// `stable`, and [`AdapterError::CalibrationStore`] when the identities
    /// disagree: a stable source, a non-stable target, a class mismatch, or a
    /// record the target already has. A refused adoption changes nothing.
    pub fn adopt_identity(
        &mut self,
        provisional: &DeviceId,
        stable: DeviceId,
    ) -> Result<usize, AdapterError> {
        if !self.is_connected(provisional) {
            return Err(AdapterError::NotConnected {
                device: provisional.clone(),
            });
        }
        if self.is_connected(&stable) {
            return Err(AdapterError::AlreadyConnected { device: stable });
        }
        let moved = self
            .calibration
            .promote(provisional, &stable)
            .map_err(|error| AdapterError::CalibrationStore { error })?;
        if let Some(known) = self
            .connected
            .iter_mut()
            .find(|known| *known == provisional)
        {
            *known = stable.clone();
        }
        for held in &mut self.held_edges {
            if held.device() == provisional {
                held.device = stable.clone();
            }
        }
        for (driving, _) in &mut self.driven_axes {
            if driving == provisional {
                *driving = stable.clone();
            }
        }
        Ok(moved)
    }

    /// Applies one device event, appending what it produced to `frame`.
    ///
    /// A [`DeviceEvent::Connected`] or [`DeviceEvent::Removed`] event changes
    /// the session's device set and produces no edge or axis of its own: a
    /// removal's effect is the [`DeviceLoss`] report plus the neutral axes the
    /// next finished frame carries.
    ///
    /// `map` is the session's action map and `context` its current input
    /// context; a target the context does not accept is not applied at all.
    ///
    /// # Errors
    ///
    /// [`AdapterError::NotConnected`] for an event from a device the session
    /// has not registered, [`AdapterError::ClassMismatch`] when the event's
    /// variant disagrees with the device's class,
    /// [`AdapterError::ReadingRejected`] for a reading no calibration accepts,
    /// and [`AdapterError::AxisNotContinuous`] /
    /// [`AdapterError::AxisValue`] for a value that is not a valid axis sample.
    ///
    /// A refused event changes **nothing**: not the frame, not the holds, not
    /// the axes the device was driving and not the report counter. Every
    /// reading the report carries is calibrated before any of it is applied
    /// ([`calibrated_readings`](Self::calibrated_readings)), and the input the
    /// frame already collected from other devices is left alone, so a bad
    /// report cannot swallow a press or drop another device's command.
    pub fn apply(
        &mut self,
        event: &DeviceEvent,
        map: &ActionMap,
        context: InputContext,
        frame: &mut InputFrame,
    ) -> Result<(), AdapterError> {
        match event {
            DeviceEvent::Connected { device } => return self.connect(device.clone()),
            DeviceEvent::Removed { device } => return self.disconnect(device),
            _ => {}
        }
        let device = event.device();
        if !self.is_connected(device) {
            return Err(AdapterError::NotConnected {
                device: device.clone(),
            });
        }
        let reported = event.event_class().unwrap_or(DeviceClass::Keyboard);
        if reported != device.class() {
            return Err(AdapterError::ClassMismatch {
                expected: device.class(),
                reported,
                device: device.clone(),
            });
        }
        // The whole report is calibrated before any of it is applied, so a
        // reading the calibration refuses cannot leave the buttons this same
        // report pressed half-applied.
        let readings = self.calibrated_readings(event)?;
        // The report is level-triggered: what this device drove is re-stated
        // from scratch, and a hold the report does not re-establish is released
        // once the report has been read.
        self.reports += 1;
        self.forget_driven_axes(device);
        let report = self.reports;

        let result = match event {
            DeviceEvent::KeyboardFrame { device, keys } => {
                for key in keys {
                    self.apply_source(
                        device,
                        BindingSource::Key(*key),
                        Reading::Pressed,
                        map,
                        context,
                        frame,
                    )?;
                }
                Ok(())
            }
            DeviceEvent::MouseFrame {
                device, buttons, ..
            } => {
                for button in buttons {
                    self.apply_source(
                        device,
                        BindingSource::MouseButton(*button),
                        Reading::Pressed,
                        map,
                        context,
                        frame,
                    )?;
                }
                Ok(())
            }
            DeviceEvent::GamepadFrame {
                device, buttons, ..
            } => {
                for button in buttons {
                    self.apply_source(
                        device,
                        BindingSource::GamepadButton(*button),
                        Reading::Pressed,
                        map,
                        context,
                        frame,
                    )?;
                }
                Ok(())
            }
            DeviceEvent::JoystickFrame {
                device, buttons, ..
            } => {
                for index in buttons {
                    self.apply_source(
                        device,
                        BindingSource::JoystickButton(*index),
                        Reading::Pressed,
                        map,
                        context,
                        frame,
                    )?;
                }
                Ok(())
            }
            // Handled above.
            DeviceEvent::Connected { .. } | DeviceEvent::Removed { .. } => Ok(()),
        };
        // The remaining refusals can only come from a target the action map
        // itself would have refused at construction (`try_new` rejects an axis
        // target that names an edge, a non-finite or zero scale, and a
        // same-context conflict), and from a sample that is already in `[-1, 1]`
        // after the clamp in `apply_target`. They are still propagated rather
        // than swallowed.
        let result = result.and_then(|()| {
            for reading in &readings {
                self.apply_calibrated(device, reading, map, context, frame)?;
            }
            Ok(())
        });
        if result.is_ok() {
            self.release_unreported(device, report);
        }
        result
    }

    /// Closes a frame: every continuous axis the previous frame drove and this
    /// one does not is written as exactly neutral.
    ///
    /// Without this a released key or a removed stick would leave its last
    /// deflection in `cs_sim::control::AxisState` forever, because the
    /// simulation only ever moves an axis a frame names.
    pub fn finish_frame(&mut self, frame: &mut InputFrame) {
        for command in self.neutralize_unreported(frame) {
            // Only a continuous command ever reaches `driven_last_frame`, so
            // a neutral sample for it always exists.
            let neutral = AxisValue::from_quantized(command, 0)
                .expect("a driven command is a continuous axis");
            frame.set_axis(neutral);
        }
        self.driven_last_frame = frame.axes().iter().map(|axis| axis.command()).collect();
    }

    /// The commands the previous frame drove and this one does not, and
    /// re-bases the record of what the last frame drove to this frame's axes.
    ///
    /// This is the part of [`finish_frame`](Self::finish_frame) that decides
    /// *which* axes need a neutral sample. [`suppress`](Self::suppress) and a
    /// caller that has no frame to close — a session whose window is not
    /// focused, whose input path is closed, or whose controls are being torn
    /// down — use it to state exactly the same neutrals without inventing a
    /// second, different neutralization rule.
    pub fn neutralize_unreported(&mut self, frame: &InputFrame) -> Vec<FlightCommand> {
        let driven_now: Vec<FlightCommand> =
            frame.axes().iter().map(|axis| axis.command()).collect();
        let mut neutral = Vec::new();
        for command in self.driven_last_frame.clone() {
            if !driven_now.contains(&command) {
                neutral.push(command);
            }
        }
        self.driven_last_frame = driven_now;
        neutral
    }

    /// Forgets the axes a device was driving, before its new report is read: a
    /// report is the device's whole state, so what it omits is no longer
    /// driven.
    fn forget_driven_axes(&mut self, device: &DeviceId) {
        self.driven_axes.retain(|(driving, _)| driving != device);
    }

    /// Releases every hold and forgets every driven axis, without removing a
    /// device (F22-C).
    ///
    /// A focus loss, a pause and a control handover are not removals: the
    /// devices are still there, but the session can no longer trust what they
    /// were holding, because nothing has told it the pilot let go. F22-B's
    /// limit — "a device that stops reporting without a removal event keeps
    /// its hold" — is closed here by the one owner that *does* know the input
    /// state is untrustworthy: it drops the holds itself instead of waiting
    /// for a report that may never come.
    ///
    /// The axes the devices were driving are forgotten, so the next
    /// [`finish_frame`](Self::finish_frame) states them neutral; the devices
    /// stay connected and their calibration and connection records are
    /// untouched, so a focus gain re-establishes control from a fresh report.
    /// It is not a [`DeviceLoss`]: nothing was lost, and a caller that told
    /// the player their joystick disappeared would be lying.
    pub fn suppress(&mut self) -> SuppressedHolds {
        let released_edges = self.held_edges.iter().map(HeldEdge::action).collect();
        let mut neutralized_axes: Vec<FlightCommand> = self
            .driven_axes
            .iter()
            .map(|(_, command)| *command)
            .collect();
        for command in self.driven_last_frame.clone() {
            if !neutralized_axes.contains(&command) {
                neutralized_axes.push(command);
            }
        }
        self.held_edges.clear();
        self.driven_axes.clear();
        // The record of what the last frame drove goes too, so a frame that is
        // closed after the suppression does not try to neutralize an axis this
        // call has already reported as neutral.
        self.driven_last_frame.clear();
        SuppressedHolds {
            released_edges,
            neutralized_axes,
        }
    }

    /// Releases the holds this device's report did not re-establish.
    fn release_unreported(&mut self, device: &DeviceId, report: u64) {
        self.held_edges
            .retain(|held| held.device() != device || held.established_in() == report);
    }

    /// Applies one digital source that the report lists as down.
    fn apply_source(
        &mut self,
        device: &DeviceId,
        source: BindingSource,
        reading: Reading,
        map: &ActionMap,
        context: InputContext,
        frame: &mut InputFrame,
    ) -> Result<(), AdapterError> {
        for target in map.targets_for(source) {
            if !context.accepts(target.action()) {
                continue;
            }
            self.apply_target(device, target, reading, frame)?;
        }
        Ok(())
    }

    /// Calibrates every analog reading a report carries, before any part of the
    /// report is applied.
    ///
    /// This pass is what makes a refused report change nothing. A report may
    /// name any number of buttons and axes, and only a reading can be refused,
    /// so calibrating them all first means the refusal happens before the
    /// buttons the same report pressed were applied — no swallowed press, no
    /// half-established hold, no forgotten axis and no counted report.
    ///
    /// An unbound channel is calibrated too: a driver that reports nonsense is
    /// a real fault whether or not a binding happens to read the channel.
    fn calibrated_readings(
        &self,
        event: &DeviceEvent,
    ) -> Result<Vec<CalibratedReading>, AdapterError> {
        let device = event.device();
        let mut readings = Vec::new();
        match event {
            DeviceEvent::MouseFrame {
                motion_x, motion_y, ..
            } => {
                for (axis, motion) in [(MouseAxis::X, *motion_x), (MouseAxis::Y, *motion_y)] {
                    // A relative channel that did not move is not a drive at
                    // all, so a stopped mouse reads as nothing rather than as
                    // a deflection of zero.
                    if motion == 0.0 {
                        continue;
                    }
                    readings.push(self.calibrate(device, AxisChannel::Mouse(axis), motion)?);
                }
            }
            DeviceEvent::GamepadFrame { axes, .. } => {
                for (axis, raw) in axes {
                    readings.push(self.calibrate(
                        device,
                        AxisChannel::Gamepad(*axis),
                        normalize_gamepad_axis(*axis, *raw),
                    )?);
                }
            }
            DeviceEvent::JoystickFrame { axes, .. } => {
                for (index, raw) in axes {
                    readings.push(self.calibrate(device, AxisChannel::Joystick(*index), *raw)?);
                }
            }
            DeviceEvent::Connected { .. }
            | DeviceEvent::Removed { .. }
            | DeviceEvent::KeyboardFrame { .. } => {}
        }
        Ok(readings)
    }

    /// Calibrates one raw reading of one channel of one device.
    ///
    /// The reading's own calibration decides whether it counts as pressed for
    /// an edge target, and the channel decides from which end it is measured: a
    /// one-directional channel rests at the axis **minimum**
    /// ([`normalize_gamepad_axis`] maps a trigger's `[0, 1]` into `[-1, 1]`),
    /// so an untriggered trigger reads `-1.0`, whose magnitude would clear any
    /// threshold and leave a trigger bound to a weapon firing from the moment
    /// it is touched. The threshold is therefore measured from the channel's
    /// resting end: the pull of a trigger, the deflection of a stick.
    fn calibrate(
        &self,
        device: &DeviceId,
        channel: AxisChannel,
        raw: f32,
    ) -> Result<CalibratedReading, AdapterError> {
        let calibration = self.calibration.calibration_or_default(device, channel);
        let value = calibration
            .apply(raw)
            .map_err(|error| AdapterError::ReadingRejected {
                device: device.clone(),
                channel,
                error,
            })?;
        let activation = calibration.activation();
        let active = if channel.is_unipolar() {
            let pull = (value + 1.0) * 0.5;
            pull >= activation
        } else {
            value.abs() >= activation
        };
        Ok(CalibratedReading {
            channel,
            value,
            active,
        })
    }

    /// Applies one already calibrated channel reading to every binding of that
    /// channel.
    ///
    /// The reading was calibrated once per device and channel by
    /// [`calibrate`](Self::calibrate); each binding then applies its own scale
    /// and, for a source that is physically wired backwards
    /// ([`BindingSource::JoystickAxis`]'s `inverted` flag), its own inversion. A
    /// source's wiring inversion and the player's calibration inversion are
    /// different facts and are applied in that order, so the two compose
    /// instead of cancelling by accident.
    fn apply_calibrated(
        &mut self,
        device: &DeviceId,
        reading: &CalibratedReading,
        map: &ActionMap,
        context: InputContext,
        frame: &mut InputFrame,
    ) -> Result<(), AdapterError> {
        for binding in map.bindings_for_channel(reading.channel) {
            let wired_inverted = matches!(
                binding.source,
                BindingSource::JoystickAxis { inverted: true, .. }
            );
            let target = binding.target;
            if !context.accepts(target.action()) {
                continue;
            }
            let value = if wired_inverted {
                -reading.value
            } else {
                reading.value
            };
            self.apply_target(
                device,
                target,
                Reading::Analog {
                    value,
                    active: reading.active,
                },
                frame,
            )?;
        }
        Ok(())
    }

    /// Folds one resolved target of one device into the frame.
    fn apply_target(
        &mut self,
        device: &DeviceId,
        target: BindingTarget,
        reading: Reading,
        frame: &mut InputFrame,
    ) -> Result<(), AdapterError> {
        match target {
            BindingTarget::Axis { command, scale } => {
                if !command.is_continuous() {
                    return Err(AdapterError::AxisNotContinuous { command });
                }
                let raw = match reading {
                    // A digital source bound to an axis is at full scale while
                    // it is held; the map's scale carries its direction.
                    Reading::Pressed => scale,
                    Reading::Analog { value, .. } => value * scale,
                };
                let value = raw.clamp(-1.0, 1.0);
                let axis = AxisValue::from_unit(command, value)
                    .map_err(|error| AdapterError::AxisValue { command, error })?;
                keep_strongest(frame, command, axis);
                if !self
                    .driven_axes
                    .iter()
                    .any(|(driving, driven)| driving == device && *driven == command)
                {
                    self.driven_axes.push((device.clone(), command));
                }
                Ok(())
            }
            BindingTarget::Command(_) | BindingTarget::Ui(_) => {
                let action = target.action();
                // An analog source bound to an edge needs a declared crossing
                // point, which is what the calibration's activation threshold
                // is for: without it a trigger could never fire and could
                // never stop. `active` was decided once for the channel, from
                // that channel's resting end.
                let active = match reading {
                    Reading::Pressed => true,
                    Reading::Analog { active, .. } => active,
                };
                if !active {
                    return Ok(());
                }
                let already_held = self
                    .held_edges
                    .iter()
                    .any(|held| held.device() == device && held.action() == action);
                if already_held {
                    // The report still lists the source, so the hold survives
                    // and keeps its place in the current report.
                    if let Some(held) = self
                        .held_edges
                        .iter_mut()
                        .find(|held| held.device() == device && held.action() == action)
                    {
                        held.established_in = self.reports;
                    }
                    return Ok(());
                }
                self.held_edges.push(HeldEdge {
                    device: device.clone(),
                    action,
                    established_in: self.reports,
                });
                frame.push_edge(action);
                Ok(())
            }
        }
    }
}

/// Folds a calibrated axis into the frame, keeping the strongest deflection.
///
/// When more than one device drives the same axis in one frame the strongest
/// reading wins, so the result does not depend on the order the platform
/// happened to deliver its events, and a keyboard's full-scale digital axis is
/// not silently replaced by a smaller stick reading.
fn keep_strongest(frame: &mut InputFrame, command: FlightCommand, axis: AxisValue) {
    match frame.axis(command) {
        Some(existing) if existing.as_unit().abs() >= axis.as_unit().abs() => {}
        _ => frame.set_axis(axis),
    }
}

/// Maps a gamepad axis reading from the driver's units into the canonical
/// signed `[-1, 1]` pipeline.
///
/// The sticks already report `[-1, 1]`. A trigger is unipolar: it reports
/// `[0, 1]`, where rest is `0` and full pull is `1`. It is mapped into the
/// signed range so one pipeline serves every channel, which puts a resting
/// trigger at the axis **minimum** (`-1.0`) and a full pull at the maximum
/// (`+1.0`).
///
/// The minimum is a designed convention, not a mid-axis neutral: a trigger is a
/// one-directional lever, so on a throttle axis the resting end is idle and the
/// pulled end is full, and a consumer that wants a `[0, 1]` throttle maps the
/// axis with `(axis + 1.0) / 2.0`. Which end is which in the original game, and
/// whether it is even the original's model, is F22-D's measurement.
#[must_use]
pub fn normalize_gamepad_axis(axis: GamepadAxis, raw: f32) -> f32 {
    match axis {
        GamepadAxis::LeftTrigger | GamepadAxis::RightTrigger => raw * 2.0 - 1.0,
        GamepadAxis::LeftStickX
        | GamepadAxis::LeftStickY
        | GamepadAxis::RightStickX
        | GamepadAxis::RightStickY => raw,
    }
}

/// The designed starting dead zone of an absolute axis, as a fraction of full
/// deflection.
///
/// **Designed, not original.** The original game's resting stick and trigger
/// positions are unknown until F22-D measures them; this is the project's
/// starting value and is replaceable per device through
/// [`CalibrationStore::set`].
pub const DESIGNED_DEAD_ZONE: f32 = 0.08;

/// The designed starting calibration of one channel: a small dead zone, no
/// inversion, proportional response and full deflection.
///
/// **Designed, not original**; see [`DESIGNED_DEAD_ZONE`].
#[must_use]
pub fn designed_axis_calibration() -> AxisCalibration {
    AxisCalibration::try_new(DESIGNED_DEAD_ZONE, false, ResponseCurve::Linear, 1.0, 0.5)
        .expect("the designed dead zone is inside [0, 1)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_types::Tick;

    fn stick(id: &str) -> DeviceId {
        DeviceId::stable(DeviceClass::Joystick, id).expect("the test identity is valid")
    }

    fn keyboard() -> DeviceId {
        DeviceId::stable(DeviceClass::Keyboard, "kbd/0").expect("the test identity is valid")
    }

    /// The action map most of the adapter tests run with.
    fn map() -> ActionMap {
        ActionMap::designed_default()
    }

    /// The device an event names, for the refusal assertions.
    fn device_value(event: &DeviceEvent) -> DeviceId {
        event.device().clone()
    }

    /// A connected adapter, ready for one frame of events.
    fn connected(devices: &[DeviceId]) -> DeviceAdapters {
        let mut adapters = DeviceAdapters::new();
        for device in devices {
            adapters
                .connect(device.clone())
                .expect("the device connects");
        }
        adapters
    }

    /// A connected adapter for exactly one device.
    fn connected_one(device: &DeviceId) -> DeviceAdapters {
        connected(std::slice::from_ref(device))
    }

    /// The minimum acceptance scenario (AC02) at the adapter level: the stick
    /// is firing and driving an axis, it is unplugged, its held trigger is
    /// released, the axis it drove is reported for neutralization and no
    /// further fire is produced.
    #[test]
    fn accept_f22_b_joystick_removal_releases_held_fire_and_reports_loss() {
        let device = stick("joy.stick.test/0");
        let mut adapters = connected_one(&device);

        let mut frame = InputFrame::new(Tick(1));
        adapters
            .apply(
                &DeviceEvent::JoystickFrame {
                    device: device.clone(),
                    buttons: vec![0],
                    axes: vec![(0, 0.9)],
                },
                &map(),
                InputContext::Flight,
                &mut frame,
            )
            .expect("the frame applies");
        assert_eq!(
            frame.edges(),
            &[Action::Flight(FlightCommand::FirePrimary)],
            "button 0 fires the primary guns"
        );
        let roll = frame.axis(FlightCommand::Roll).expect("axis 0 drives roll");
        assert!(
            roll.as_unit() > 0.5,
            "the stick is deflected, got {}",
            roll.as_unit()
        );
        adapters.finish_frame(&mut frame);

        adapters.disconnect(&device).expect("the stick disconnects");

        let losses = adapters.take_losses();
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].device, device);
        assert_eq!(
            losses[0].released_edges,
            vec![Action::Flight(FlightCommand::FirePrimary)],
            "the held trigger is reported as released"
        );
        assert_eq!(
            losses[0].neutralized_axes,
            vec![FlightCommand::Roll],
            "the axis the stick drove is reported for neutralization"
        );
        assert_eq!(
            losses[0].stable_identity.as_deref(),
            Some("joy.stick.test/0"),
            "a stable identity is reported so a caller may persist calibration"
        );
        assert!(adapters.held_edges().is_empty(), "nothing is still held");
        assert!(adapters.driven_axes().is_empty(), "nothing is still driven");
        assert_eq!(adapters.connected_count(), 0);
        assert!(adapters.losses().is_empty(), "the loss was taken once");

        // The frame after the removal neutralizes the axis and fires nothing.
        let mut after = InputFrame::new(Tick(2));
        adapters.finish_frame(&mut after);
        assert!(
            after.edges().is_empty(),
            "a removed stick cannot keep firing"
        );
        let roll = after
            .axis(FlightCommand::Roll)
            .expect("the axis is explicitly neutralized");
        assert_eq!(roll.quantized(), 0, "the removed stick's axis is neutral");
    }

    /// A device that was only known by its enumeration index reports **no**
    /// stable identity, so a caller cannot persist its calibration against the
    /// index. This is non-negotiable behavior 1's persistence half.
    #[test]
    fn accept_f22_b_index_only_device_reports_no_stable_identity() {
        let provisional = DeviceId::enumeration_fallback(DeviceClass::Joystick, 3);
        let mut adapters = connected_one(&provisional);

        // It drives an axis and is calibrated while it is only an index.
        adapters.calibration_mut().set(
            &provisional,
            AxisChannel::Joystick(0),
            designed_axis_calibration(),
        );
        let mut frame = InputFrame::new(Tick(1));
        adapters
            .apply(
                &DeviceEvent::JoystickFrame {
                    device: provisional.clone(),
                    buttons: vec![],
                    axes: vec![(0, 0.5)],
                },
                &map(),
                InputContext::Flight,
                &mut frame,
            )
            .expect("the frame applies");
        adapters
            .disconnect(&provisional)
            .expect("the stick disconnects");

        let losses = adapters.take_losses();
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].stable_identity, None,
            "an enumeration index is not an identity"
        );
        assert_eq!(
            adapters.calibration().unstable_devices(),
            vec![&provisional],
            "the record is visible as unsafe to persist until it is promoted"
        );
    }

    /// Non-negotiable behavior 1 end to end: a stick that first enumerated
    /// under an index keeps its calibration when the platform reveals its
    /// identity, and keeps it across a reboot at a different index. An
    /// implementation that re-keys only the store, or only the connection,
    /// loses the calibration on the very next report.
    #[test]
    fn accept_f22_b_calibration_survives_identity_adoption_and_replug() {
        let provisional = DeviceId::enumeration_fallback(DeviceClass::Joystick, 0);
        let stable = stick("joy.stick.test/0");
        let calibration = AxisCalibration::try_new(0.5, false, ResponseCurve::Linear, 1.0, 0.5)
            .expect("the test calibration is valid");

        let mut adapters = connected_one(&provisional);
        adapters
            .calibration_mut()
            .set(&provisional, AxisChannel::Joystick(0), calibration);

        // The stick is deflected and its trigger held while it is still only an
        // index: the wide dead zone reads the deflection as exactly neutral.
        let mut frame = InputFrame::new(Tick(1));
        adapters
            .apply(
                &DeviceEvent::JoystickFrame {
                    device: provisional.clone(),
                    buttons: vec![0],
                    axes: vec![(0, 0.4)],
                },
                &map(),
                InputContext::Flight,
                &mut frame,
            )
            .expect("the frame applies");
        assert_eq!(
            frame.axis(FlightCommand::Roll).map(AxisValue::quantized),
            Some(0),
            "a reading inside the stick's dead zone is neutral"
        );
        assert_eq!(adapters.held_edges().len(), 1, "the trigger is held");

        // The platform reveals the identity: everything keyed by the index is
        // re-keyed, so the hold, the axes and the calibration all survive.
        let moved = adapters
            .adopt_identity(&provisional, stable.clone())
            .expect("the provisional stick adopts its stable identity");
        assert_eq!(moved, 1, "one calibration record moved");
        assert!(adapters.is_connected(&stable));
        assert!(!adapters.is_connected(&provisional));
        assert_eq!(adapters.connected(), std::slice::from_ref(&stable));
        assert_eq!(
            adapters
                .calibration()
                .get(&stable, AxisChannel::Joystick(0)),
            Some(calibration)
        );
        assert!(adapters.calibration().unstable_devices().is_empty());
        assert_eq!(
            adapters.held_edges()[0].device(),
            &stable,
            "the held trigger follows the identity"
        );
        assert!(
            adapters
                .driven_axes()
                .iter()
                .all(|(driving, _)| driving == &stable),
            "no axis is left keyed by the index"
        );

        // The still-held trigger is not re-fired by the reveal, and the
        // calibrated deflection past the dead zone now drives the axis.
        let mut frame = InputFrame::new(Tick(2));
        adapters
            .apply(
                &DeviceEvent::JoystickFrame {
                    device: stable.clone(),
                    buttons: vec![0],
                    axes: vec![(0, 0.6)],
                },
                &map(),
                InputContext::Flight,
                &mut frame,
            )
            .expect("the frame applies");
        assert!(
            frame.edges().is_empty(),
            "adopting an identity must not re-fire a held trigger"
        );
        assert!(
            frame
                .axis(FlightCommand::Roll)
                .expect("roll is driven")
                .as_unit()
                > 0.0,
            "past the dead zone the retained calibration drives the axis"
        );

        // Unplugging does not throw the tuning away: the record is keyed by
        // the identity, so the stick comes back at a different index, adopts
        // its identity again, and is calibrated from the first report.
        adapters.disconnect(&stable).expect("the stick disconnects");
        let replugged = DeviceId::enumeration_fallback(DeviceClass::Joystick, 5);
        adapters
            .connect(replugged.clone())
            .expect("the stick replugs");
        assert_eq!(
            adapters.adopt_identity(&replugged, stable.clone()),
            Ok(0),
            "nothing is left to move: the records are already under the identity"
        );
        let mut frame = InputFrame::new(Tick(3));
        adapters
            .apply(
                &DeviceEvent::JoystickFrame {
                    device: stable.clone(),
                    buttons: vec![],
                    axes: vec![(0, 0.6)],
                },
                &map(),
                InputContext::Flight,
                &mut frame,
            )
            .expect("the frame applies");
        let roll = frame.axis(FlightCommand::Roll).expect("roll is driven");
        assert!(
            (roll.as_unit() - 0.2).abs() < 0.01,
            "the retained dead zone shapes the replugged stick: (0.6 - 0.5) / \
             0.5 = 0.2, got {}",
            roll.as_unit()
        );

        // A different stick never inherits a stranger's calibration.
        let other = stick("joy.stick.other/0");
        assert_eq!(
            adapters
                .calibration()
                .calibration_or_default(&other, AxisChannel::Joystick(0)),
            AxisCalibration::designed_default(),
            "another device starts from the default"
        );
        // Surviving a *reboot* needs a saved profile, which is F22-C's
        // persistence path; this session keeps its records in memory only.
    }

    /// An adoption is refused by name when the identities disagree, and a
    /// refused adoption moves nothing.
    #[test]
    fn accept_f22_b_identity_adoption_is_refused_by_name() {
        let provisional = DeviceId::enumeration_fallback(DeviceClass::Joystick, 0);
        let stable = stick("joy.stick.test/0");
        let mut adapters = connected_one(&provisional);

        // A target that is not a stable identity.
        let still_an_index = DeviceId::enumeration_fallback(DeviceClass::Joystick, 1);
        assert_eq!(
            adapters.adopt_identity(&provisional, still_an_index.clone()),
            Err(AdapterError::CalibrationStore {
                error: CalibrationStoreError::TargetNotStable {
                    device: still_an_index
                }
            })
        );
        // A target of another class.
        let pad = DeviceId::stable(DeviceClass::Gamepad, "pad.test/0")
            .expect("the test identity is valid");
        assert_eq!(
            adapters.adopt_identity(&provisional, pad.clone()),
            Err(AdapterError::CalibrationStore {
                error: CalibrationStoreError::ClassMismatch {
                    provisional: provisional.clone(),
                    stable: pad,
                }
            })
        );
        // A source the session does not have.
        let stranger = DeviceId::enumeration_fallback(DeviceClass::Joystick, 8);
        assert_eq!(
            adapters.adopt_identity(&stranger, stable),
            Err(AdapterError::NotConnected { device: stranger })
        );
        // A target the session already has.
        let second = stick("joy.stick.second/0");
        adapters.connect(second.clone()).expect("connects");
        assert_eq!(
            adapters.adopt_identity(&provisional, second.clone()),
            Err(AdapterError::AlreadyConnected {
                device: second.clone()
            })
        );

        // None of the refusals changed the device set.
        assert_eq!(adapters.connected(), &[provisional, second]);
    }

    /// A report is level-triggered: a held key fires once, stays held, and the
    /// frame that no longer lists it releases it — and a key bound to a
    /// continuous axis is at full scale while held and exactly neutral when
    /// released, never an edge.
    #[test]
    fn accept_f22_b_reports_are_level_triggered_for_edges_and_axes() {
        let device = keyboard();
        let mut adapters = connected_one(&device);

        let mut press = InputFrame::new(Tick(1));
        adapters
            .apply(
                &DeviceEvent::KeyboardFrame {
                    device: device.clone(),
                    keys: vec![Key::Space, Key::W],
                },
                &map(),
                InputContext::Flight,
                &mut press,
            )
            .expect("the frame applies");
        assert_eq!(
            press.edges(),
            &[Action::Flight(FlightCommand::FirePrimary)],
            "only the edge fires; a digital axis is not an edge"
        );
        let pitch = press.axis(FlightCommand::Pitch).expect("pitch is driven");
        assert!(
            (pitch.as_unit() + 1.0).abs() < 1e-3,
            "W drives pitch at full negative scale, got {}",
            pitch.as_unit()
        );
        adapters.finish_frame(&mut press);

        // Still held: no second fire, the axis still driven.
        let mut held = InputFrame::new(Tick(2));
        adapters
            .apply(
                &DeviceEvent::KeyboardFrame {
                    device: device.clone(),
                    keys: vec![Key::Space, Key::W],
                },
                &map(),
                InputContext::Flight,
                &mut held,
            )
            .expect("the frame applies");
        assert!(
            held.edges().is_empty(),
            "a key held across frames fires once, not once per frame"
        );
        assert_eq!(adapters.held_edges().len(), 1, "the fire is still held");
        adapters.finish_frame(&mut held);

        // Released: no fire, and the axis the key drove is explicitly neutral.
        let mut release = InputFrame::new(Tick(3));
        adapters
            .apply(
                &DeviceEvent::KeyboardFrame {
                    device: device.clone(),
                    keys: vec![],
                },
                &map(),
                InputContext::Flight,
                &mut release,
            )
            .expect("the frame applies");
        assert!(release.edges().is_empty());
        assert!(adapters.held_edges().is_empty(), "release clears the hold");
        adapters.finish_frame(&mut release);
        let pitch = release
            .axis(FlightCommand::Pitch)
            .expect("the released axis is explicitly neutralized");
        assert_eq!(pitch.quantized(), 0, "release is exactly neutral");
    }

    /// The context gate holds on the device path: the same key that fires the
    /// guns in flight emits nothing in text entry or in a cinematic
    /// (non-negotiable behavior 5).
    #[test]
    fn accept_f22_b_device_path_honors_the_input_context() {
        let device = keyboard();
        let mut adapters = connected_one(&device);
        let event = DeviceEvent::KeyboardFrame {
            device: device.clone(),
            keys: vec![Key::Space, Key::Enter],
        };

        for (context, expected) in [
            (InputContext::Flight, 1),
            (InputContext::UiNavigation, 1),
            (InputContext::TextEntry, 0),
            (InputContext::Cinematic, 0),
        ] {
            let mut frame = InputFrame::new(Tick(1));
            adapters
                .apply(&event, &map(), context, &mut frame)
                .expect("the frame applies");
            assert_eq!(
                frame.edges().len(),
                expected,
                "{context} must accept {expected} of the two bound keys"
            );
        }

        // The UiNavigation frame resolved Enter as a UI action, not a command.
        let mut ui = InputFrame::new(Tick(2));
        adapters
            .apply(
                &DeviceEvent::KeyboardFrame {
                    device,
                    keys: vec![Key::Enter],
                },
                &map(),
                InputContext::UiNavigation,
                &mut ui,
            )
            .expect("the frame applies");
        assert!(
            ui.edges()
                .iter()
                .all(|action| matches!(action, Action::Ui(_))),
            "a UI context resolves only UI actions"
        );
    }

    /// Every declared device class reaches the frame through its own adapter,
    /// including the gamepad trigger's re-centering and the joystick's
    /// source-level inversion.
    #[test]
    fn accept_f22_b_every_device_family_reaches_the_frame() {
        let keyboard = keyboard();
        let mouse = DeviceId::stable(DeviceClass::Mouse, "mouse/0").expect("valid identity");
        let pad = DeviceId::stable(DeviceClass::Gamepad, "pad/0").expect("valid identity");
        let devices = vec![keyboard.clone(), mouse.clone(), pad.clone(), stick("joy/0")];
        let mut adapters = connected(&devices);

        // Keyboard: the left mouse button's keyboard twin, space, fires.
        let mut frame = InputFrame::new(Tick(1));
        adapters
            .apply(
                &DeviceEvent::KeyboardFrame {
                    device: keyboard,
                    keys: vec![Key::Space],
                },
                &map(),
                InputContext::Flight,
                &mut frame,
            )
            .expect("the frame applies");
        assert_eq!(frame.edges().len(), 1);

        // Mouse: the left button fires and the motion drives yaw and pitch.
        let mut frame = InputFrame::new(Tick(2));
        adapters
            .apply(
                &DeviceEvent::MouseFrame {
                    device: mouse,
                    buttons: vec![MouseButton::Left],
                    motion_x: 0.5,
                    motion_y: -0.25,
                },
                &map(),
                InputContext::Flight,
                &mut frame,
            )
            .expect("the frame applies");
        assert_eq!(
            frame.edges(),
            &[Action::Flight(FlightCommand::FirePrimary)],
            "the left mouse button fires the primary guns"
        );
        let yaw = frame
            .axis(FlightCommand::Yaw)
            .expect("the mouse drives yaw");
        assert!((yaw.as_unit() - 0.5).abs() < 1e-3, "got {}", yaw.as_unit());
        let pitch = frame
            .axis(FlightCommand::Pitch)
            .expect("the mouse drives pitch");
        assert!(
            (pitch.as_unit() - 0.25).abs() < 1e-3,
            "the designed binding inverts the vertical axis, got {}",
            pitch.as_unit()
        );

        // Gamepad: the right trigger at rest is neutral throttle, at full pull
        // it is a full deflection, and the face button fires.
        let mut frame = InputFrame::new(Tick(3));
        adapters
            .apply(
                &DeviceEvent::GamepadFrame {
                    device: pad.clone(),
                    buttons: vec![GamepadButton::South],
                    axes: vec![(GamepadAxis::RightTrigger, 0.0)],
                },
                &map(),
                InputContext::Flight,
                &mut frame,
            )
            .expect("the frame applies");
        assert_eq!(frame.edges().len(), 1, "the face button fires");
        let throttle = frame
            .axis(FlightCommand::Throttle)
            .expect("the trigger drives the throttle");
        assert_eq!(
            throttle.quantized(),
            AxisValue::from_unit(FlightCommand::Throttle, -1.0)
                .expect("the axis minimum")
                .quantized(),
            "an untriggered trigger reads the axis minimum, the idle end of a \
             throttle, not a mid-axis deflection; got {}",
            throttle.as_unit()
        );
        let mut frame = InputFrame::new(Tick(4));
        adapters
            .apply(
                &DeviceEvent::GamepadFrame {
                    device: pad,
                    buttons: vec![],
                    axes: vec![(GamepadAxis::RightTrigger, 1.0)],
                },
                &map(),
                InputContext::Flight,
                &mut frame,
            )
            .expect("the frame applies");
        let throttle = frame
            .axis(FlightCommand::Throttle)
            .expect("the trigger drives the throttle");
        assert!(
            (throttle.as_unit() - 1.0).abs() < 1e-3,
            "a full pull is full scale"
        );

        assert_eq!(normalize_gamepad_axis(GamepadAxis::RightTrigger, 0.0), -1.0);
        assert_eq!(normalize_gamepad_axis(GamepadAxis::RightTrigger, 1.0), 1.0);
        assert_eq!(normalize_gamepad_axis(GamepadAxis::LeftStickX, -0.5), -0.5);
    }

    /// A source's own inverted wiring flag composes with the player's
    /// calibration inversion instead of replacing it.
    #[test]
    fn accept_f22_b_source_inversion_composes_with_calibration_inversion() {
        let inverted_map = ActionMap::try_new(vec![cs_types::input::Binding {
            source: BindingSource::JoystickAxis {
                index: 0,
                inverted: true,
            },
            target: BindingTarget::Axis {
                command: FlightCommand::Roll,
                scale: 1.0,
            },
        }])
        .expect("one binding is well formed");
        let device = stick("joy.stick.test/0");

        let mut adapters = connected_one(&device);
        let mut frame = InputFrame::new(Tick(1));
        adapters
            .apply(
                &DeviceEvent::JoystickFrame {
                    device: device.clone(),
                    buttons: vec![],
                    axes: vec![(0, 0.5)],
                },
                &inverted_map,
                InputContext::Flight,
                &mut frame,
            )
            .expect("the frame applies");
        assert!(
            frame.axis(FlightCommand::Roll).expect("roll").as_unit() < -0.4,
            "the source is wired backwards, so the reading is inverted"
        );

        // The player's own inversion flips it back.
        adapters.calibration_mut().set(
            &device,
            AxisChannel::Joystick(0),
            AxisCalibration::try_new(0.0, true, ResponseCurve::Linear, 1.0, 0.5)
                .expect("valid calibration"),
        );
        let mut frame = InputFrame::new(Tick(2));
        adapters
            .apply(
                &DeviceEvent::JoystickFrame {
                    device,
                    buttons: vec![],
                    axes: vec![(0, 0.5)],
                },
                &inverted_map,
                InputContext::Flight,
                &mut frame,
            )
            .expect("the frame applies");
        assert!(
            frame.axis(FlightCommand::Roll).expect("roll").as_unit() > 0.4,
            "the two inversions compose to the original direction"
        );
    }

    /// An analog source bound to an edge fires while it is past the
    /// calibration's activation threshold and releases below it, so a trigger
    /// that fires the guns can also stop.
    #[test]
    fn accept_f22_b_analog_source_bound_to_an_edge_uses_the_activation_threshold() {
        let trigger_map = ActionMap::try_new(vec![cs_types::input::Binding {
            source: BindingSource::JoystickAxis {
                index: 0,
                inverted: false,
            },
            target: BindingTarget::Command(FlightCommand::FirePrimary),
        }])
        .expect("one binding is well formed");
        let device = stick("joy.stick.test/0");
        let mut adapters = connected_one(&device);

        let mut low = InputFrame::new(Tick(1));
        adapters
            .apply(
                &DeviceEvent::JoystickFrame {
                    device: device.clone(),
                    buttons: vec![],
                    axes: vec![(0, 0.4)],
                },
                &trigger_map,
                InputContext::Flight,
                &mut low,
            )
            .expect("the frame applies");
        assert!(low.edges().is_empty(), "below the threshold nothing fires");

        let mut pulled = InputFrame::new(Tick(2));
        adapters
            .apply(
                &DeviceEvent::JoystickFrame {
                    device: device.clone(),
                    buttons: vec![],
                    axes: vec![(0, 0.8)],
                },
                &trigger_map,
                InputContext::Flight,
                &mut pulled,
            )
            .expect("the frame applies");
        assert_eq!(
            pulled.edges(),
            &[Action::Flight(FlightCommand::FirePrimary)],
            "past the threshold the trigger fires"
        );
        assert_eq!(adapters.held_edges().len(), 1, "the trigger is held");

        // Still pulled: no second shot, and the hold survives.
        let mut still = InputFrame::new(Tick(3));
        adapters
            .apply(
                &DeviceEvent::JoystickFrame {
                    device: device.clone(),
                    buttons: vec![],
                    axes: vec![(0, 0.9)],
                },
                &trigger_map,
                InputContext::Flight,
                &mut still,
            )
            .expect("the frame applies");
        assert!(still.edges().is_empty(), "a held trigger fires once");

        // Released: the hold is dropped, so the gun can fire again later.
        let mut released = InputFrame::new(Tick(4));
        adapters
            .apply(
                &DeviceEvent::JoystickFrame {
                    device: device.clone(),
                    buttons: vec![],
                    axes: vec![(0, 0.0)],
                },
                &trigger_map,
                InputContext::Flight,
                &mut released,
            )
            .expect("the frame applies");
        assert!(adapters.held_edges().is_empty(), "the trigger released");

        let mut again = InputFrame::new(Tick(5));
        adapters
            .apply(
                &DeviceEvent::JoystickFrame {
                    device,
                    buttons: vec![],
                    axes: vec![(0, 0.8)],
                },
                &trigger_map,
                InputContext::Flight,
                &mut again,
            )
            .expect("the frame applies");
        assert_eq!(
            again.edges(),
            &[Action::Flight(FlightCommand::FirePrimary)],
            "a re-pulled trigger fires again"
        );
    }

    /// Two devices may drive the same axis; the strongest deflection wins, so
    /// the outcome does not depend on the order the platform delivered its
    /// events in.
    #[test]
    fn accept_f22_b_strongest_deflection_wins_when_two_devices_drive_one_axis() {
        let keyboard = keyboard();
        let stick_device = stick("joy.stick.test/0");
        let adapters = connected(&[keyboard.clone(), stick_device.clone()]);

        for order in [[false, true], [true, false]] {
            let mut adapters = adapters.clone();
            let mut frame = InputFrame::new(Tick(1));
            for keyboard_first in order {
                if keyboard_first {
                    adapters
                        .apply(
                            &DeviceEvent::KeyboardFrame {
                                device: keyboard.clone(),
                                keys: vec![Key::W],
                            },
                            &map(),
                            InputContext::Flight,
                            &mut frame,
                        )
                        .expect("the keyboard frame applies");
                    adapters
                        .apply(
                            &DeviceEvent::JoystickFrame {
                                device: stick_device.clone(),
                                buttons: vec![],
                                axes: vec![(1, 0.5)],
                            },
                            &map(),
                            InputContext::Flight,
                            &mut frame,
                        )
                        .expect("the stick frame applies");
                } else {
                    adapters
                        .apply(
                            &DeviceEvent::JoystickFrame {
                                device: stick_device.clone(),
                                buttons: vec![],
                                axes: vec![(1, 0.5)],
                            },
                            &map(),
                            InputContext::Flight,
                            &mut frame,
                        )
                        .expect("the stick frame applies");
                    adapters
                        .apply(
                            &DeviceEvent::KeyboardFrame {
                                device: keyboard.clone(),
                                keys: vec![Key::W],
                            },
                            &map(),
                            InputContext::Flight,
                            &mut frame,
                        )
                        .expect("the keyboard frame applies");
                }
            }
            let pitch = frame
                .axis(FlightCommand::Pitch)
                .expect("both devices drive pitch");
            assert_eq!(
                pitch.quantized(),
                AxisValue::from_unit(FlightCommand::Pitch, -1.0)
                    .expect("full scale")
                    .quantized(),
                "the keyboard's full-scale pitch wins whatever the event order"
            );
        }
    }

    /// Malformed events are refused by name and change nothing: an event from
    /// a device that is not connected, a class mismatch, a non-finite reading,
    /// a double connection and an unknown removal.
    #[test]
    fn accept_f22_b_malformed_device_events_are_refused_by_name() {
        let device = stick("joy.stick.test/0");
        let orphan = keyboard();
        let mut adapters = connected_one(&device);
        let mut frame = InputFrame::new(Tick(1));

        // A keyboard event for a device that was never connected.
        let orphan_event = DeviceEvent::KeyboardFrame {
            device: orphan.clone(),
            keys: vec![Key::Space],
        };
        assert_eq!(
            adapters.apply(&orphan_event, &map(), InputContext::Flight, &mut frame),
            Err(AdapterError::NotConnected {
                device: orphan.clone()
            }),
            "a stale event must be refused by name"
        );
        assert!(frame.is_empty(), "a refused event changes nothing");
        assert_eq!(
            adapters.disconnect(&orphan),
            Err(AdapterError::NotConnected { device: orphan })
        );
        assert_eq!(
            adapters.connect(device.clone()),
            Err(AdapterError::AlreadyConnected {
                device: device.clone()
            })
        );

        // A gamepad event that names a joystick device.
        assert_eq!(
            adapters.apply(
                &DeviceEvent::GamepadFrame {
                    device: device.clone(),
                    buttons: vec![],
                    axes: vec![],
                },
                &map(),
                InputContext::Flight,
                &mut frame,
            ),
            Err(AdapterError::ClassMismatch {
                expected: DeviceClass::Joystick,
                reported: DeviceClass::Gamepad,
                device: device.clone(),
            })
        );

        // A non-finite reading, named by device and channel.
        let nan_event = DeviceEvent::JoystickFrame {
            device: device.clone(),
            buttons: vec![],
            axes: vec![(0, f32::NAN)],
        };
        assert!(
            matches!(
                adapters.apply(&nan_event, &map(), InputContext::Flight, &mut frame),
                Err(AdapterError::ReadingRejected {
                    device,
                    channel: AxisChannel::Joystick(0),
                    error: CalibrationError::NonFiniteReading { value },
                }) if device == device_value(&nan_event) && value.is_nan()
            ),
            "a NaN reading is refused by device and channel, never propagated"
        );

        // A reading beyond full deflection is refused too, never saturated
        // behind the player's back.
        let wild = DeviceEvent::JoystickFrame {
            device: device.clone(),
            buttons: vec![],
            axes: vec![(0, 1.5)],
        };
        assert_eq!(
            adapters.apply(&wild, &map(), InputContext::Flight, &mut frame),
            Err(AdapterError::ReadingRejected {
                device: device.clone(),
                channel: AxisChannel::Joystick(0),
                error: CalibrationError::ReadingOutOfRange { value: 1.5 },
            })
        );
        assert!(frame.is_empty(), "no refused reading reached the frame");
        assert!(adapters.losses().is_empty());

        // A good frame still applies afterwards.
        adapters
            .apply(
                &DeviceEvent::JoystickFrame {
                    device: device.clone(),
                    buttons: vec![0],
                    axes: vec![(0, 0.5)],
                },
                &map(),
                InputContext::Flight,
                &mut frame,
            )
            .expect("a good frame applies");
        assert_eq!(frame.edges().len(), 1);
        assert!(adapters.held_edges().len() <= 1);
    }

    /// A gamepad trigger is one-directional: the platform reports `[0, 1]` and
    /// the canonical pipeline maps rest to the axis **minimum**, so an
    /// untouched trigger reads `-1.0`. A trigger bound to a weapon — the
    /// natural face-button replacement on a pad — must not fire from the moment
    /// it is touched: the activation threshold is measured from the trigger's
    /// resting end (its pull), not from the middle of the signed axis. A
    /// magnitude test would see `|-1.0|` clear every threshold and hold the
    /// guns on forever.
    #[test]
    fn accept_f22_b_a_resting_trigger_does_not_fire_an_edge_binding() {
        let trigger_map = ActionMap::try_new(vec![cs_types::input::Binding {
            source: BindingSource::GamepadAxis(GamepadAxis::RightTrigger),
            target: BindingTarget::Command(FlightCommand::FirePrimary),
        }])
        .expect("one binding is well formed");
        let device = DeviceId::stable(DeviceClass::Gamepad, "pad.test/0")
            .expect("the test identity is valid");
        assert!(
            AxisChannel::Gamepad(GamepadAxis::RightTrigger).is_unipolar(),
            "a trigger rests at the axis minimum, not at its middle"
        );
        assert!(!AxisChannel::Joystick(0).is_unipolar());
        let mut adapters = connected_one(&device);

        // Untouched: the platform's rest reads as the axis minimum.
        let mut rest = InputFrame::new(Tick(1));
        adapters
            .apply(
                &DeviceEvent::GamepadFrame {
                    device: device.clone(),
                    buttons: vec![],
                    axes: vec![(GamepadAxis::RightTrigger, 0.0)],
                },
                &trigger_map,
                InputContext::Flight,
                &mut rest,
            )
            .expect("the rest report applies");
        assert!(
            rest.edges().is_empty(),
            "an untouched trigger must not fire, got {:?}",
            rest.edges()
        );
        assert!(adapters.held_edges().is_empty(), "and holds nothing");

        // Half pulled: past the designed 0.5 activation, measured as the pull.
        let mut pulled = InputFrame::new(Tick(2));
        adapters
            .apply(
                &DeviceEvent::GamepadFrame {
                    device: device.clone(),
                    buttons: vec![],
                    axes: vec![(GamepadAxis::RightTrigger, 0.5)],
                },
                &trigger_map,
                InputContext::Flight,
                &mut pulled,
            )
            .expect("the pulled report applies");
        assert_eq!(
            pulled.edges(),
            &[Action::Flight(FlightCommand::FirePrimary)],
            "a half-pulled trigger crosses the threshold and fires"
        );

        // Released: the hold is dropped, so the trigger can fire again.
        let mut released = InputFrame::new(Tick(3));
        adapters
            .apply(
                &DeviceEvent::GamepadFrame {
                    device: device.clone(),
                    buttons: vec![],
                    axes: vec![(GamepadAxis::RightTrigger, 0.0)],
                },
                &trigger_map,
                InputContext::Flight,
                &mut released,
            )
            .expect("the release report applies");
        assert!(adapters.held_edges().is_empty(), "the trigger released");

        let mut again = InputFrame::new(Tick(4));
        adapters
            .apply(
                &DeviceEvent::GamepadFrame {
                    device,
                    buttons: vec![],
                    axes: vec![(GamepadAxis::RightTrigger, 1.0)],
                },
                &trigger_map,
                InputContext::Flight,
                &mut again,
            )
            .expect("the report applies");
        assert_eq!(
            again.edges(),
            &[Action::Flight(FlightCommand::FirePrimary)],
            "a re-pulled trigger fires again"
        );
    }

    /// A report that is refused half way through changes nothing. The report
    /// below names the fire button **and** a reading no calibration accepts,
    /// and the buttons are applied before the axes — so an implementation that
    /// validated lazily dropped the edge from the frame while keeping the hold
    /// it had established: the press was swallowed and the gun would stay
    /// silent until the trigger was released and pressed again. The device's
    /// driven axes and the report counter must survive the refusal too, or a
    /// later loss would under-report what the device was driving.
    #[test]
    fn accept_f22_b_a_refused_report_leaves_no_partial_state() {
        let device = stick("joy.stick.test/0");
        let mut adapters = connected_one(&device);

        // A good report first, so the device is holding and driving something.
        let mut frame = InputFrame::new(Tick(1));
        adapters
            .apply(
                &DeviceEvent::JoystickFrame {
                    device: device.clone(),
                    buttons: vec![0],
                    axes: vec![(0, 0.5)],
                },
                &map(),
                InputContext::Flight,
                &mut frame,
            )
            .expect("the good report applies");
        adapters.finish_frame(&mut frame);
        assert_eq!(adapters.reports(), 1);
        assert_eq!(adapters.held_edges().len(), 1);
        assert_eq!(adapters.driven_axes().len(), 1);

        // Now a report whose axis reading is refused.
        let mut frame = InputFrame::new(Tick(2));
        assert!(matches!(
            adapters.apply(
                &DeviceEvent::JoystickFrame {
                    device: device.clone(),
                    buttons: vec![0],
                    axes: vec![(0, f32::NAN)],
                },
                &map(),
                InputContext::Flight,
                &mut frame,
            ),
            Err(AdapterError::ReadingRejected {
                channel: AxisChannel::Joystick(0),
                ..
            })
        ));
        assert!(frame.is_empty(), "a refused report contributes nothing");
        assert_eq!(adapters.reports(), 1, "a refused report is not a report");
        assert_eq!(
            adapters.driven_axes().len(),
            1,
            "a refused report does not forget the axis the device was driving"
        );
        assert_eq!(
            adapters.held_edges().len(),
            1,
            "a refused report neither adds nor releases a hold"
        );

        // The axis the device drives is still reported as driven when it is
        // removed, and the very next good report is not a second press.
        adapters.disconnect(&device).expect("the stick disconnects");
        assert_eq!(
            adapters.take_losses()[0].neutralized_axes,
            vec![FlightCommand::Roll],
            "the loss still names the axis the device was driving"
        );
    }

    /// The half-way refusal above on a device that was **not** already holding
    /// the button: the press must not be swallowed. A good report afterwards is
    /// still a first press, so the gun fires exactly once.
    #[test]
    fn accept_f22_b_a_refused_report_does_not_swallow_the_press_it_named() {
        let device = stick("joy.stick.test/0");
        let mut adapters = connected_one(&device);

        let mut frame = InputFrame::new(Tick(1));
        assert!(
            adapters
                .apply(
                    &DeviceEvent::JoystickFrame {
                        device: device.clone(),
                        buttons: vec![0],
                        axes: vec![(1, 1.5)],
                    },
                    &map(),
                    InputContext::Flight,
                    &mut frame,
                )
                .is_err(),
            "the out-of-range reading is refused"
        );
        assert!(frame.is_empty(), "the refused press produced no edge");
        assert!(
            adapters.held_edges().is_empty(),
            "and left no hold that would silence the next real press"
        );

        let mut frame = InputFrame::new(Tick(2));
        adapters
            .apply(
                &DeviceEvent::JoystickFrame {
                    device,
                    buttons: vec![0],
                    axes: vec![],
                },
                &map(),
                InputContext::Flight,
                &mut frame,
            )
            .expect("the good report applies");
        assert_eq!(
            frame.edges(),
            &[Action::Flight(FlightCommand::FirePrimary)],
            "the pilot's press is not swallowed by the refused report before it"
        );
    }

    /// A connect or remove event is the device-set half of the same stream, so
    /// a caller can pump one message type.
    #[test]
    fn accept_f22_b_connect_and_remove_events_drive_the_device_set() {
        let device = stick("joy.stick.test/0");
        let mut adapters = DeviceAdapters::new();
        let mut frame = InputFrame::new(Tick(1));

        adapters
            .apply(
                &DeviceEvent::Connected {
                    device: device.clone(),
                },
                &map(),
                InputContext::Flight,
                &mut frame,
            )
            .expect("the device connects");
        assert!(adapters.is_connected(&device));
        assert!(frame.is_empty(), "connecting produces no input");

        adapters
            .apply(
                &DeviceEvent::Removed {
                    device: device.clone(),
                },
                &map(),
                InputContext::Flight,
                &mut frame,
            )
            .expect("the device disconnects");
        assert!(!adapters.is_connected(&device));
        assert_eq!(adapters.losses().len(), 1, "the removal is reported");
    }

    /// F22-C: the release a focus loss, a pause and a control handover use when
    /// nothing was removed. It drops the holds and forgets the driven axes
    /// without touching the device set, the calibration or the report counter,
    /// and it is not a `DeviceLoss` — nothing was lost.
    #[test]
    fn accept_f22_c_suppress_releases_holds_without_removing_a_device() {
        let device = stick("joy.stick.test/0");
        let keyboard = DeviceId::stable(DeviceClass::Keyboard, "kbd.test/0")
            .expect("the test identity is valid");
        let mut adapters = connected(&[device.clone(), keyboard.clone()]);
        let mut frame = InputFrame::new(Tick(0));
        // The trigger fires and the stick deflects roll.
        adapters
            .apply(
                &DeviceEvent::JoystickFrame {
                    device: device.clone(),
                    buttons: vec![0],
                    axes: vec![(0, 0.75)],
                },
                &map(),
                InputContext::Flight,
                &mut frame,
            )
            .expect("the report applies");
        // The keyboard is down at the same time.
        adapters
            .apply(
                &DeviceEvent::KeyboardFrame {
                    device: keyboard,
                    keys: vec![Key::S],
                },
                &map(),
                InputContext::Flight,
                &mut frame,
            )
            .expect("the report applies");
        assert_eq!(adapters.held_edges().len(), 1, "the trigger is held");
        assert_eq!(adapters.driven_axes().len(), 2, "two axes are driven");
        let reports = adapters.reports();
        let calibration = adapters.calibration().len();

        let released = adapters.suppress();
        assert_eq!(
            released.released_edges,
            vec![Action::Flight(FlightCommand::FirePrimary)]
        );
        let mut neutralized = released.neutralized_axes.clone();
        neutralized.sort();
        assert_eq!(
            neutralized,
            vec![FlightCommand::Pitch, FlightCommand::Roll],
            "every axis any device was driving is named, including one whose last \\
             finished frame is the only record of it"
        );
        assert!(!released.is_empty());
        assert!(
            adapters.held_edges().is_empty(),
            "the held trigger is gone, so the next report presses again"
        );
        assert!(adapters.driven_axes().is_empty());
        assert!(
            adapters.losses().is_empty(),
            "a suppression is not a device loss: the player was not told their \\
             joystick disappeared"
        );
        assert!(adapters.is_connected(&device), "the stick is still there");
        assert_eq!(adapters.connected_count(), 2);
        assert_eq!(
            adapters.calibration().len(),
            calibration,
            "and its calibration is untouched"
        );
        assert_eq!(
            adapters.reports(),
            reports,
            "the report counter only counts reports that were read"
        );

        // A second suppression has nothing to release.
        assert!(adapters.suppress().is_empty());

        // The next finished frame states nothing neutral a second time, because
        // the suppression already reported those axes.
        let mut after = InputFrame::new(Tick(1));
        adapters.finish_frame(&mut after);
        assert!(
            after.is_inert(),
            "no axis is left to neutralize: {:?}",
            after.axes()
        );

        // A fresh report re-establishes control from the device's own state.
        let mut fresh = InputFrame::new(Tick(2));
        adapters
            .apply(
                &DeviceEvent::JoystickFrame {
                    device: device.clone(),
                    buttons: vec![0],
                    axes: vec![(0, 0.75)],
                },
                &map(),
                InputContext::Flight,
                &mut fresh,
            )
            .expect("the fresh report applies");
        assert_eq!(
            fresh.edges(),
            &[Action::Flight(FlightCommand::FirePrimary)],
            "the trigger fires again after the suppression, exactly once"
        );
        assert!(fresh.axis(FlightCommand::Roll).is_some());
    }
}
