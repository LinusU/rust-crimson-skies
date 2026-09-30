//! Typed command schema, device bindings and the action map (F22-A).
//!
//! Spec: `specs/F22-input-bindings-devices-and-control-ownership.md`, stage
//! `### F22-A`. Shared contract: `docs/contracts/UI-NETWORK.md` — the client
//! owns local input requests only ("Network ownership table") and a UI action
//! requests a domain transaction rather than editing game state directly
//! ("UI transition discipline").
//!
//! The deliverable is the **typed interface**, not a runtime: what a
//! [`FlightCommand`] is, which [`UiAction`] a menu can receive, how a
//! physical [`BindingSource`] resolves to an [`Action`] through an
//! [`ActionMap`], and how continuous [`AxisValue`]s travel separately from
//! once-per-press [`Action`] edges in an [`InputFrame`]. The adapters that
//! read real devices and calibrate axes are F22-B; focus, replay and control
//! ownership wiring are F22-C; original command coverage is F22-D.
//!
//! **Designed vocabulary, never original data.** Every label, the action set
//! and the [`ActionMap::designed_default`] map are newly authored project
//! design. Which commands the original 2000 PC game exposes, which keys and
//! devices it binds them to and how it labels them are **unknown** until
//! F22-D measures them; nothing here claims otherwise
//! (`docs/findings/2026-09-29-f22-a-command-schema-and-action-map.md`).
//!
//! **`cs_types` stays dependency-free** (`docs/01-ARCHITECTURE.md`), so the
//! device vocabulary is a small engine-owned enum set instead of winit,
//! `gilrs` or Bevy input types. The app-side collector that consumes these
//! records is `cs_app::input`; the buffering and ownership policy is
//! `cs_sim::control`.

use std::fmt;

use crate::Tick;

/// Maximum byte length of a [`DeviceId`]'s stable identity text.
pub const MAX_DEVICE_IDENTITY_LEN: usize = 256;

/// A family of physical input devices.
///
/// The class is part of a binding's meaning: the same button index on a
/// gamepad and a joystick are different sources, and calibration is stored
/// per family (F22-B).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeviceClass {
    /// A keyboard.
    Keyboard,
    /// A mouse.
    Mouse,
    /// A gamepad (XInput-style, two sticks and a d-pad).
    Gamepad,
    /// A joystick or HOTAS stick with arbitrary axis and button counts.
    Joystick,
}

impl DeviceClass {
    /// Every class, in a stable order.
    pub const ALL: &'static [DeviceClass] =
        &[Self::Keyboard, Self::Mouse, Self::Gamepad, Self::Joystick];

    /// The stable label used in reports and persisted bindings.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Keyboard => "keyboard",
            Self::Mouse => "mouse",
            Self::Gamepad => "gamepad",
            Self::Joystick => "joystick",
        }
    }

    /// Looks a class up by its label; `None` for an unknown class.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|class| class.label() == label)
    }
}

impl fmt::Display for DeviceClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The identity of one physical device.
///
/// Non-negotiable behavior 1: a controller is never tied to an unstable
/// enumeration index **alone**. A device that exposes a stable identity (a
/// device path, a serial or a GUID) is identified by it, so it keeps its
/// calibration across plug order and reboots; the enumeration index survives
/// only as an explicitly flagged [`EnumerationFallback`](Self::EnumerationFallback)
/// that may not be persisted as if it were an identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeviceIdentity {
    /// A stable per-device identity reported by the platform.
    Stable(String),
    /// The device's order of enumeration. It is a placeholder used until a
    /// stable identity is observed; it is not stable across sessions.
    EnumerationFallback(u32),
}

impl DeviceIdentity {
    /// Whether this is a stable identity (safe to persist calibration for).
    pub const fn is_stable(&self) -> bool {
        matches!(self, Self::Stable(_))
    }

    /// A stable diagnostic spelling.
    pub fn label(&self) -> String {
        match self {
            Self::Stable(identity) => identity.clone(),
            Self::EnumerationFallback(index) => format!("index:{index}"),
        }
    }

    /// The stable identity text, or `None` for an enumeration fallback.
    ///
    /// The one accessor a persistence writer needs: `Some` is a real identity
    /// that may key a saved record, `None` is an index that may not.
    #[must_use]
    pub fn stable_identity(&self) -> Option<&str> {
        match self {
            Self::Stable(identity) => Some(identity),
            Self::EnumerationFallback(_) => None,
        }
    }
}

/// Why a stable [`DeviceIdentity`] was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceIdError {
    /// The identity text was empty.
    Empty,
    /// The identity text exceeded [`MAX_DEVICE_IDENTITY_LEN`] bytes.
    TooLong {
        /// Its length in bytes.
        len: usize,
    },
    /// The identity text contained a control character.
    ControlCharacter {
        /// The offending character.
        ch: char,
    },
}

impl fmt::Display for DeviceIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "a stable device identity must not be empty"),
            Self::TooLong { len } => write!(
                f,
                "the device identity is {len} bytes, max is {MAX_DEVICE_IDENTITY_LEN}"
            ),
            Self::ControlCharacter { ch } => {
                write!(f, "the device identity contains control character {ch:?}")
            }
        }
    }
}

impl std::error::Error for DeviceIdError {}

/// A typed, class-qualified physical device id.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId {
    class: DeviceClass,
    identity: DeviceIdentity,
}

impl DeviceId {
    /// Builds a device id from a stable platform identity.
    ///
    /// # Errors
    ///
    /// [`DeviceIdError`] when the identity is empty, too long or contains a
    /// control character (which would corrupt a persisted binding record).
    pub fn stable(class: DeviceClass, identity: &str) -> Result<Self, DeviceIdError> {
        if identity.is_empty() {
            return Err(DeviceIdError::Empty);
        }
        if identity.len() > MAX_DEVICE_IDENTITY_LEN {
            return Err(DeviceIdError::TooLong {
                len: identity.len(),
            });
        }
        if let Some(ch) = identity.chars().find(|ch| ch.is_control()) {
            return Err(DeviceIdError::ControlCharacter { ch });
        }
        Ok(Self {
            class,
            identity: DeviceIdentity::Stable(identity.to_owned()),
        })
    }

    /// Builds a provisional device id from its enumeration index.
    ///
    /// Used only before a stable identity is observed; the result reports
    /// [`DeviceId::is_stable`] as `false` so calibration is not persisted
    /// against it.
    pub fn enumeration_fallback(class: DeviceClass, index: u32) -> Self {
        Self {
            class,
            identity: DeviceIdentity::EnumerationFallback(index),
        }
    }

    /// The device family.
    pub const fn class(&self) -> DeviceClass {
        self.class
    }

    /// The identity this device was built from.
    pub const fn identity(&self) -> &DeviceIdentity {
        &self.identity
    }

    /// Whether the identity is stable and may key a persisted calibration.
    pub const fn is_stable(&self) -> bool {
        self.identity.is_stable()
    }

    /// The stable identity text, or `None` when this device is only known by
    /// its enumeration index.
    #[must_use]
    pub fn stable_identity(&self) -> Option<&str> {
        self.identity.stable_identity()
    }

    /// A stable diagnostic spelling, `class:identity`.
    pub fn label(&self) -> String {
        format!("{}:{}", self.class.label(), self.identity.label())
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.class.label(), self.identity.label())
    }
}

/// A keyboard key in the engine's device vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    /// `W`.
    W,
    /// `A`.
    A,
    /// `S`.
    S,
    /// `D`.
    D,
    /// `Q`.
    Q,
    /// `E`.
    E,
    /// `R`.
    R,
    /// `F`.
    F,
    /// `Z`.
    Z,
    /// `X`.
    X,
    /// `C`.
    C,
    /// `B`.
    B,
    /// `G`.
    G,
    /// `T`.
    T,
    /// The space bar.
    Space,
    /// Left shift.
    LeftShift,
    /// Left control.
    LeftControl,
    /// Left alt.
    LeftAlt,
    /// Escape.
    Escape,
    /// Enter (return).
    Enter,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// The up arrow.
    ArrowUp,
    /// The down arrow.
    ArrowDown,
    /// The left arrow.
    ArrowLeft,
    /// The right arrow.
    ArrowRight,
    /// `1`.
    Digit1,
    /// `2`.
    Digit2,
    /// `3`.
    Digit3,
    /// `4`.
    Digit4,
}

impl Key {
    /// Every key, in a stable order.
    pub const ALL: &'static [Key] = &[
        Self::W,
        Self::A,
        Self::S,
        Self::D,
        Self::Q,
        Self::E,
        Self::R,
        Self::F,
        Self::Z,
        Self::X,
        Self::C,
        Self::B,
        Self::G,
        Self::T,
        Self::Space,
        Self::LeftShift,
        Self::LeftControl,
        Self::LeftAlt,
        Self::Escape,
        Self::Enter,
        Self::Tab,
        Self::Backspace,
        Self::ArrowUp,
        Self::ArrowDown,
        Self::ArrowLeft,
        Self::ArrowRight,
        Self::Digit1,
        Self::Digit2,
        Self::Digit3,
        Self::Digit4,
    ];

    /// The stable label used in persisted bindings and reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::W => "w",
            Self::A => "a",
            Self::S => "s",
            Self::D => "d",
            Self::Q => "q",
            Self::E => "e",
            Self::R => "r",
            Self::F => "f",
            Self::Z => "z",
            Self::X => "x",
            Self::C => "c",
            Self::B => "b",
            Self::G => "g",
            Self::T => "t",
            Self::Space => "space",
            Self::LeftShift => "left_shift",
            Self::LeftControl => "left_control",
            Self::LeftAlt => "left_alt",
            Self::Escape => "escape",
            Self::Enter => "enter",
            Self::Tab => "tab",
            Self::Backspace => "backspace",
            Self::ArrowUp => "arrow_up",
            Self::ArrowDown => "arrow_down",
            Self::ArrowLeft => "arrow_left",
            Self::ArrowRight => "arrow_right",
            Self::Digit1 => "digit1",
            Self::Digit2 => "digit2",
            Self::Digit3 => "digit3",
            Self::Digit4 => "digit4",
        }
    }

    /// Looks a key up by its label; `None` for an unknown key.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|key| key.label() == label)
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MouseButton {
    /// The primary (left) button.
    Left,
    /// The secondary (right) button.
    Right,
    /// The middle button.
    Middle,
    /// The first side button.
    Back,
    /// The second side button.
    Forward,
}

impl MouseButton {
    /// Every button, in a stable order.
    pub const ALL: &'static [MouseButton] = &[
        Self::Left,
        Self::Right,
        Self::Middle,
        Self::Back,
        Self::Forward,
    ];

    /// The stable label used in persisted bindings and reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Middle => "middle",
            Self::Back => "back",
            Self::Forward => "forward",
        }
    }

    /// Looks a button up by its label; `None` for an unknown button.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|button| button.label() == label)
    }
}

/// A mouse axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MouseAxis {
    /// Horizontal motion.
    X,
    /// Vertical motion.
    Y,
    /// The scroll wheel.
    Wheel,
}

impl MouseAxis {
    /// Every axis, in a stable order.
    pub const ALL: &'static [MouseAxis] = &[Self::X, Self::Y, Self::Wheel];

    /// The stable label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
            Self::Wheel => "wheel",
        }
    }

    /// Looks an axis up by its label; `None` for an unknown axis.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|axis| axis.label() == label)
    }
}

/// A gamepad button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GamepadButton {
    /// The bottom face button.
    South,
    /// The right face button.
    East,
    /// The left face button.
    West,
    /// The top face button.
    North,
    /// Left shoulder button.
    LeftBumper,
    /// Right shoulder button.
    RightBumper,
    /// The left stick pressed in.
    LeftStick,
    /// The right stick pressed in.
    RightStick,
    /// D-pad up.
    DpadUp,
    /// D-pad down.
    DpadDown,
    /// D-pad left.
    DpadLeft,
    /// D-pad right.
    DpadRight,
    /// Start.
    Start,
    /// Select / back.
    Select,
}

impl GamepadButton {
    /// Every button, in a stable order.
    pub const ALL: &'static [GamepadButton] = &[
        Self::South,
        Self::East,
        Self::West,
        Self::North,
        Self::LeftBumper,
        Self::RightBumper,
        Self::LeftStick,
        Self::RightStick,
        Self::DpadUp,
        Self::DpadDown,
        Self::DpadLeft,
        Self::DpadRight,
        Self::Start,
        Self::Select,
    ];

    /// The stable label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::South => "south",
            Self::East => "east",
            Self::West => "west",
            Self::North => "north",
            Self::LeftBumper => "left_bumper",
            Self::RightBumper => "right_bumper",
            Self::LeftStick => "left_stick",
            Self::RightStick => "right_stick",
            Self::DpadUp => "dpad_up",
            Self::DpadDown => "dpad_down",
            Self::DpadLeft => "dpad_left",
            Self::DpadRight => "dpad_right",
            Self::Start => "start",
            Self::Select => "select",
        }
    }

    /// Looks a button up by its label; `None` for an unknown button.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|button| button.label() == label)
    }
}

/// A gamepad analog axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GamepadAxis {
    /// The left stick's horizontal axis.
    LeftStickX,
    /// The left stick's vertical axis.
    LeftStickY,
    /// The right stick's horizontal axis.
    RightStickX,
    /// The right stick's vertical axis.
    RightStickY,
    /// The left trigger.
    LeftTrigger,
    /// The right trigger.
    RightTrigger,
}

impl GamepadAxis {
    /// Every axis, in a stable order.
    pub const ALL: &'static [GamepadAxis] = &[
        Self::LeftStickX,
        Self::LeftStickY,
        Self::RightStickX,
        Self::RightStickY,
        Self::LeftTrigger,
        Self::RightTrigger,
    ];

    /// The stable label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::LeftStickX => "left_stick_x",
            Self::LeftStickY => "left_stick_y",
            Self::RightStickX => "right_stick_x",
            Self::RightStickY => "right_stick_y",
            Self::LeftTrigger => "left_trigger",
            Self::RightTrigger => "right_trigger",
        }
    }

    /// Looks an axis up by its label; `None` for an unknown axis.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|axis| axis.label() == label)
    }
}

/// One physical source an [`ActionMap`] can bind.
///
/// A binding names a source by **device class**, not by device instance: the
/// map is authored once and every keyboard or every gamepad of the matching
/// class uses it. Which specific [`DeviceId`] currently fulfils a class (and
/// its calibration) is F22-B's session state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BindingSource {
    /// A keyboard key.
    Key(Key),
    /// A mouse button.
    MouseButton(MouseButton),
    /// A mouse axis.
    MouseAxis(MouseAxis),
    /// A gamepad button.
    GamepadButton(GamepadButton),
    /// A gamepad axis.
    GamepadAxis(GamepadAxis),
    /// A joystick button by its device index.
    JoystickButton(u16),
    /// A joystick axis by its device index, with the source's own sign.
    JoystickAxis {
        /// The device axis index.
        index: u16,
        /// Whether the raw axis reads inverted relative to the canonical
        /// rising direction (a device property, kept distinct from the
        /// binding's [`BindingTarget::Axis`] scale).
        inverted: bool,
    },
}

impl BindingSource {
    /// The device family this source belongs to.
    pub const fn device_class(self) -> DeviceClass {
        match self {
            Self::Key(_) => DeviceClass::Keyboard,
            Self::MouseButton(_) | Self::MouseAxis(_) => DeviceClass::Mouse,
            Self::GamepadButton(_) | Self::GamepadAxis(_) => DeviceClass::Gamepad,
            Self::JoystickButton(_) | Self::JoystickAxis { .. } => DeviceClass::Joystick,
        }
    }

    /// Whether this source only reports an analog value instead of a press.
    ///
    /// Axes and buttons both resolve to a [`BindingTarget`], but only an
    /// analog source can drive a continuous axis without a digital ramp
    /// (F22-B's calibration).
    pub const fn is_analog(self) -> bool {
        matches!(
            self,
            Self::MouseAxis(_) | Self::GamepadAxis(_) | Self::JoystickAxis { .. }
        )
    }

    /// A stable diagnostic spelling.
    pub fn label(self) -> String {
        match self {
            Self::Key(key) => format!("key.{}", key.label()),
            Self::MouseButton(button) => format!("mouse_button.{}", button.label()),
            Self::MouseAxis(axis) => format!("mouse_axis.{}", axis.label()),
            Self::GamepadButton(button) => format!("gamepad_button.{}", button.label()),
            Self::GamepadAxis(axis) => format!("gamepad_axis.{}", axis.label()),
            Self::JoystickButton(index) => format!("joystick_button.{index}"),
            Self::JoystickAxis { index, inverted } => {
                format!(
                    "joystick_axis.{index}{}",
                    if inverted { ".inverted" } else { "" }
                )
            }
        }
    }
}

impl fmt::Display for BindingSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label())
    }
}

/// A typed command the flight simulation consumes.
///
/// The set is the engine's designed vocabulary; which of these the original
/// game has and what else it exposes is F22-D's measurement. The
/// [`continuous`](Self::is_continuous) members are buffered as analog values,
/// the rest as once-per-press edges (`cs_sim::control`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FlightCommand {
    /// Pitch control axis.
    Pitch,
    /// Roll control axis.
    Roll,
    /// Yaw control axis.
    Yaw,
    /// Throttle control axis.
    Throttle,
    /// Fire the primary guns.
    FirePrimary,
    /// Fire the secondary weapon.
    FireSecondary,
    /// Select the next weapon.
    CycleWeapon,
    /// Release a dropped ordnance.
    DropOrdnance,
    /// Eject from the aircraft.
    Eject,
    /// Toggle landing gear.
    ToggleGear,
    /// Advance the flaps one step.
    FlapStep,
    /// Select the next target.
    TargetNext,
    /// Select the previous target.
    TargetPrev,
    /// Release a countermeasure.
    Countermeasure,
    /// Step the keyboard throttle up.
    ThrottleStepUp,
    /// Step the keyboard throttle down.
    ThrottleStepDown,
    /// Set throttle to idle.
    ThrottleIdle,
    /// Set throttle to full.
    ThrottleFull,
}

impl FlightCommand {
    /// Every command, in a stable order.
    pub const ALL: &'static [FlightCommand] = &[
        Self::Pitch,
        Self::Roll,
        Self::Yaw,
        Self::Throttle,
        Self::FirePrimary,
        Self::FireSecondary,
        Self::CycleWeapon,
        Self::DropOrdnance,
        Self::Eject,
        Self::ToggleGear,
        Self::FlapStep,
        Self::TargetNext,
        Self::TargetPrev,
        Self::Countermeasure,
        Self::ThrottleStepUp,
        Self::ThrottleStepDown,
        Self::ThrottleIdle,
        Self::ThrottleFull,
    ];

    /// The continuous axes, in a stable order.
    pub const CONTINUOUS: &'static [FlightCommand] =
        &[Self::Pitch, Self::Roll, Self::Yaw, Self::Throttle];

    /// Whether this command is a continuous axis rather than an edge.
    pub const fn is_continuous(self) -> bool {
        matches!(self, Self::Pitch | Self::Roll | Self::Yaw | Self::Throttle)
    }

    /// The stable label used in reports and persisted bindings.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pitch => "pitch",
            Self::Roll => "roll",
            Self::Yaw => "yaw",
            Self::Throttle => "throttle",
            Self::FirePrimary => "fire_primary",
            Self::FireSecondary => "fire_secondary",
            Self::CycleWeapon => "cycle_weapon",
            Self::DropOrdnance => "drop_ordnance",
            Self::Eject => "eject",
            Self::ToggleGear => "toggle_gear",
            Self::FlapStep => "flap_step",
            Self::TargetNext => "target_next",
            Self::TargetPrev => "target_prev",
            Self::Countermeasure => "countermeasure",
            Self::ThrottleStepUp => "throttle_step_up",
            Self::ThrottleStepDown => "throttle_step_down",
            Self::ThrottleIdle => "throttle_idle",
            Self::ThrottleFull => "throttle_full",
        }
    }

    /// Looks a command up by its label; `None` for an unknown command.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|command| command.label() == label)
    }
}

impl fmt::Display for FlightCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A typed user-interface action a menu can receive.
///
/// Non-negotiable behavior 5: these are the only actions a UI context may
/// receive, and a text-entry context receives none of them — text entry
/// cannot also fire weapons or eject.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum UiAction {
    /// Confirm the focused element.
    Confirm,
    /// Back out of the current screen.
    Cancel,
    /// Move the focus up.
    NavigateUp,
    /// Move the focus down.
    NavigateDown,
    /// Move the focus left.
    NavigateLeft,
    /// Move the focus right.
    NavigateRight,
    /// Pause the local session.
    Pause,
}

impl UiAction {
    /// Every action, in a stable order.
    pub const ALL: &'static [UiAction] = &[
        Self::Confirm,
        Self::Cancel,
        Self::NavigateUp,
        Self::NavigateDown,
        Self::NavigateLeft,
        Self::NavigateRight,
        Self::Pause,
    ];

    /// The stable label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Confirm => "confirm",
            Self::Cancel => "cancel",
            Self::NavigateUp => "navigate_up",
            Self::NavigateDown => "navigate_down",
            Self::NavigateLeft => "navigate_left",
            Self::NavigateRight => "navigate_right",
            Self::Pause => "pause",
        }
    }

    /// Looks an action up by its label; `None` for an unknown action.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|action| action.label() == label)
    }
}

impl fmt::Display for UiAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A resolved input action: a flight command or a UI action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    /// A command the flight simulation consumes.
    Flight(FlightCommand),
    /// An action a menu or the pause path consumes.
    Ui(UiAction),
}

impl Action {
    /// The stable label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Flight(command) => command.label(),
            Self::Ui(action) => action.label(),
        }
    }

    /// Looks an action up by its label; `None` for an unknown action.
    pub fn from_label(label: &str) -> Option<Self> {
        FlightCommand::from_label(label)
            .map(Self::Flight)
            .or_else(|| UiAction::from_label(label).map(Self::Ui))
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// What currently owns the keyboard, mouse and other devices.
///
/// The context is the gate of non-negotiable behavior 5: a context accepts
/// only actions of its own kind, and text entry and cinematics accept none at
/// all, so opening text entry cannot also fire weapons or eject.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum InputContext {
    /// Normal in-flight control.
    Flight,
    /// Menu and UI navigation.
    UiNavigation,
    /// Text entry: a text widget reads the keys directly and no flight or UI
    /// action is emitted.
    TextEntry,
    /// A cinematic or cutscene: no local action is emitted.
    Cinematic,
}

impl InputContext {
    /// Every context, in a stable order.
    pub const ALL: &'static [InputContext] = &[
        Self::Flight,
        Self::UiNavigation,
        Self::TextEntry,
        Self::Cinematic,
    ];

    /// The stable label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Flight => "flight",
            Self::UiNavigation => "ui_navigation",
            Self::TextEntry => "text_entry",
            Self::Cinematic => "cinematic",
        }
    }

    /// Whether this context may receive `action`.
    ///
    /// This is the whole of the context gate: [`Flight`](Self::Flight)
    /// accepts flight commands, [`UiNavigation`](Self::UiNavigation) accepts
    /// UI actions, and [`TextEntry`](Self::TextEntry) and
    /// [`Cinematic`](Self::Cinematic) accept neither.
    pub const fn accepts(self, action: Action) -> bool {
        match self {
            Self::Flight => matches!(action, Action::Flight(_)),
            Self::UiNavigation => matches!(action, Action::Ui(_)),
            Self::TextEntry | Self::Cinematic => false,
        }
    }
}

impl fmt::Display for InputContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Why an [`AxisValue`] was rejected.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AxisValueError {
    /// The command is an edge, not a continuous axis.
    NotContinuous {
        /// The offending command.
        command: FlightCommand,
    },
    /// The value was NaN or infinite.
    NonFinite {
        /// The offending command.
        command: FlightCommand,
    },
    /// The value fell outside the normalized `[-1, 1]` range.
    OutOfRange {
        /// The offending command.
        command: FlightCommand,
        /// The rejected value.
        value: f32,
    },
}

impl fmt::Display for AxisValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotContinuous { command } => {
                write!(f, "{command} is not a continuous axis")
            }
            Self::NonFinite { command } => {
                write!(f, "the {command} axis value must be finite")
            }
            Self::OutOfRange { command, value } => write!(
                f,
                "the {command} axis value {value} is outside the normalized [-1, 1] range"
            ),
        }
    }
}

impl std::error::Error for AxisValueError {}

/// The full-scale magnitude of a quantized axis value.
pub const AXIS_QUANT_MAX: i16 = i16::MAX;

/// One continuous axis value of an [`InputFrame`].
///
/// The value is **quantized** to a signed 16-bit integer, so the same
/// recorded command stream replays identically at any display rate (F22-C
/// AC03): two float samples that differ below the quantization step are the
/// same command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AxisValue {
    command: FlightCommand,
    quantized: i16,
}

impl AxisValue {
    /// Quantizes a normalized `[-1, 1]` value.
    ///
    /// # Errors
    ///
    /// [`AxisValueError::NotContinuous`] when `command` is an edge,
    /// [`AxisValueError::NonFinite`] for a NaN/infinite value, and
    /// [`AxisValueError::OutOfRange`] outside `[-1, 1]`. The input is never
    /// clamped or repaired silently.
    pub fn from_unit(command: FlightCommand, value: f32) -> Result<Self, AxisValueError> {
        if !command.is_continuous() {
            return Err(AxisValueError::NotContinuous { command });
        }
        if !value.is_finite() {
            return Err(AxisValueError::NonFinite { command });
        }
        if !(-1.0..=1.0).contains(&value) {
            return Err(AxisValueError::OutOfRange { command, value });
        }
        let quantized = (value * f32::from(AXIS_QUANT_MAX)).round() as i16;
        Ok(Self { command, quantized })
    }

    /// Builds a value from an already quantized sample.
    ///
    /// # Errors
    ///
    /// [`AxisValueError::NotContinuous`] when `command` is an edge.
    pub fn from_quantized(command: FlightCommand, quantized: i16) -> Result<Self, AxisValueError> {
        if !command.is_continuous() {
            return Err(AxisValueError::NotContinuous { command });
        }
        Ok(Self { command, quantized })
    }

    /// The axis this value drives.
    pub const fn command(self) -> FlightCommand {
        self.command
    }

    /// The quantized sample.
    pub const fn quantized(self) -> i16 {
        self.quantized
    }

    /// The sample as a normalized `[-1, 1]` value.
    pub fn as_unit(self) -> f32 {
        f32::from(self.quantized) / f32::from(AXIS_QUANT_MAX)
    }
}

/// One render frame's resolved input, stamped with the simulation tick it is
/// meant for.
///
/// Continuous [`axes`](Self::axes) describe the current deflection; each
/// [`edges`](Self::edges) entry is a one-shot action produced by a press. The
/// two are buffered separately by `cs_sim::control`, which is what makes a
/// one-frame key edge produce exactly one action across several physics
/// substeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputFrame {
    frame_tick: Tick,
    edges: Vec<Action>,
    axes: Vec<AxisValue>,
}

impl InputFrame {
    /// An empty frame for `frame_tick`.
    pub const fn new(frame_tick: Tick) -> Self {
        Self {
            frame_tick,
            edges: Vec::new(),
            axes: Vec::new(),
        }
    }

    /// The simulation tick this frame is stamped for.
    pub const fn frame_tick(&self) -> Tick {
        self.frame_tick
    }

    /// The one-shot edges of this frame, in observation order.
    pub fn edges(&self) -> &[Action] {
        &self.edges
    }

    /// The continuous axis values of this frame, one per driven axis.
    pub fn axes(&self) -> &[AxisValue] {
        &self.axes
    }

    /// Whether the frame carries no edge and no axis.
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty() && self.axes.is_empty()
    }

    /// Appends a one-shot edge.
    pub fn push_edge(&mut self, action: Action) {
        self.edges.push(action);
    }

    /// Sets the value of an axis, replacing any earlier value for it.
    pub fn set_axis(&mut self, value: AxisValue) {
        if let Some(existing) = self
            .axes
            .iter_mut()
            .find(|axis| axis.command() == value.command())
        {
            *existing = value;
        } else {
            self.axes.push(value);
        }
    }

    /// The value of `command` in this frame, when it was driven.
    pub fn axis(&self, command: FlightCommand) -> Option<AxisValue> {
        self.axes
            .iter()
            .copied()
            .find(|axis| axis.command() == command)
    }
}

/// How a [`BindingSource`] drives an [`Action`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BindingTarget {
    /// A continuous flight axis; `scale` gives the source's direction and
    /// magnitude (`+1.0`/`-1.0` for a digital source).
    Axis {
        /// The continuous command driven.
        command: FlightCommand,
        /// The signed magnitude applied to the source value.
        scale: f32,
    },
    /// A one-shot flight command.
    Command(FlightCommand),
    /// A UI action.
    Ui(UiAction),
}

impl BindingTarget {
    /// The action this target produces.
    pub const fn action(self) -> Action {
        match self {
            Self::Axis { command, .. } | Self::Command(command) => Action::Flight(command),
            Self::Ui(action) => Action::Ui(action),
        }
    }

    /// The input context this target belongs to.
    pub const fn context(self) -> InputContext {
        match self {
            Self::Axis { .. } | Self::Command(_) => InputContext::Flight,
            Self::Ui(_) => InputContext::UiNavigation,
        }
    }

    /// The continuous command, when this is an axis target.
    pub const fn axis_command(self) -> Option<FlightCommand> {
        match self {
            Self::Axis { command, .. } => Some(command),
            Self::Command(_) | Self::Ui(_) => None,
        }
    }

    /// The signed scale, when this is an axis target.
    pub const fn scale(self) -> Option<f32> {
        match self {
            Self::Axis { scale, .. } => Some(scale),
            Self::Command(_) | Self::Ui(_) => None,
        }
    }
}

/// One binding: a physical source and the target it drives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Binding {
    /// The physical source.
    pub source: BindingSource,
    /// What the source drives.
    pub target: BindingTarget,
}

/// Why an [`ActionMap`] was rejected.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ActionMapError {
    /// An axis target named an edge command.
    AxisTargetNotContinuous {
        /// The offending command.
        command: FlightCommand,
    },
    /// A command target named a continuous axis.
    CommandTargetNotDiscrete {
        /// The offending command.
        command: FlightCommand,
    },
    /// An axis target's scale was NaN or infinite.
    NonFiniteScale {
        /// The binding's source.
        source: BindingSource,
    },
    /// An axis target's scale was zero.
    ZeroScale {
        /// The binding's source.
        source: BindingSource,
    },
    /// One source drove two different targets in the same context.
    ConflictingBinding {
        /// The doubly-bound source.
        source: BindingSource,
        /// The first target's action.
        first: Action,
        /// The second target's action.
        second: Action,
    },
}

impl fmt::Display for ActionMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AxisTargetNotContinuous { command } => write!(
                f,
                "an axis target must name a continuous command, not {command}"
            ),
            Self::CommandTargetNotDiscrete { command } => write!(
                f,
                "a command target must name an edge command, not {command}"
            ),
            Self::NonFiniteScale { source } => {
                write!(f, "the axis scale of {source} must be finite")
            }
            Self::ZeroScale { source } => {
                write!(f, "the axis scale of {source} must not be zero")
            }
            Self::ConflictingBinding {
                source,
                first,
                second,
            } => write!(
                f,
                "{source} is bound to both {first} and {second} in the same context"
            ),
        }
    }
}

impl std::error::Error for ActionMapError {}

/// The action map: which physical sources drive which actions.
///
/// A source may appear twice only when the two targets live in different
/// contexts (a key that fires a weapon in flight and confirms in a menu is
/// legitimate); two different targets in the *same* context are an error, so
/// a conflicted binding set is visible instead of silently resolved by order
/// (non-negotiable behavior 5).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActionMap {
    bindings: Vec<Binding>,
}

impl ActionMap {
    /// Validates and builds a map.
    ///
    /// # Errors
    ///
    /// [`ActionMapError`] for a malformed target or a same-context conflict.
    pub fn try_new(bindings: Vec<Binding>) -> Result<Self, ActionMapError> {
        let mut seen: Vec<Binding> = Vec::new();
        for binding in &bindings {
            match binding.target {
                BindingTarget::Axis { command, scale } => {
                    if !command.is_continuous() {
                        return Err(ActionMapError::AxisTargetNotContinuous { command });
                    }
                    if !scale.is_finite() {
                        return Err(ActionMapError::NonFiniteScale {
                            source: binding.source,
                        });
                    }
                    if scale == 0.0 {
                        return Err(ActionMapError::ZeroScale {
                            source: binding.source,
                        });
                    }
                }
                BindingTarget::Command(command) => {
                    if command.is_continuous() {
                        return Err(ActionMapError::CommandTargetNotDiscrete { command });
                    }
                }
                BindingTarget::Ui(_) => {}
            }
            for previous in &seen {
                if previous.source == binding.source
                    && previous.target != binding.target
                    && previous.target.context() == binding.target.context()
                {
                    return Err(ActionMapError::ConflictingBinding {
                        source: binding.source,
                        first: previous.target.action(),
                        second: binding.target.action(),
                    });
                }
            }
            seen.push(*binding);
        }
        Ok(Self { bindings })
    }

    /// An empty map: no source is bound.
    pub const fn empty() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }

    /// The bindings, in insertion order.
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// The raw target bound to `source`, the first one found.
    pub fn target_for(&self, source: BindingSource) -> Option<BindingTarget> {
        self.bindings
            .iter()
            .find(|binding| binding.source == source)
            .map(|binding| binding.target)
    }

    /// Resolves `source` in `context`, applying the context gate.
    ///
    /// Returns `None` when the source is unbound or when the active context
    /// does not accept any action bound to it (text entry and cinematics
    /// accept none). When one source is bound in two different contexts, the
    /// target of the **active** context is returned, regardless of the order
    /// the bindings were inserted in.
    pub fn resolve(&self, context: InputContext, source: BindingSource) -> Option<Action> {
        self.bindings
            .iter()
            .find(|binding| binding.source == source && context.accepts(binding.target.action()))
            .map(|binding| binding.target.action())
    }

    /// A designed keyboard/mouse/gamepad default map.
    ///
    /// **Designed, not original.** The bindings below are newly authored
    /// development defaults that exercise every device class; they are not
    /// measurements of the original 2000 PC game, whose command coverage and
    /// bindings are unknown until F22-D. F22-B may replace them per profile.
    /// The map validates itself, so a future edit that introduces a conflict
    /// fails at construction instead of silently shadowing a binding.
    pub fn designed_default() -> Self {
        use BindingSource as S;
        use BindingTarget::{Axis, Command, Ui};
        let bindings = vec![
            // Keyboard flight axes (digital keys, signed scale).
            Binding {
                source: S::Key(Key::W),
                target: Axis {
                    command: FlightCommand::Pitch,
                    scale: -1.0,
                },
            },
            Binding {
                source: S::Key(Key::S),
                target: Axis {
                    command: FlightCommand::Pitch,
                    scale: 1.0,
                },
            },
            Binding {
                source: S::Key(Key::A),
                target: Axis {
                    command: FlightCommand::Yaw,
                    scale: -1.0,
                },
            },
            Binding {
                source: S::Key(Key::D),
                target: Axis {
                    command: FlightCommand::Yaw,
                    scale: 1.0,
                },
            },
            Binding {
                source: S::Key(Key::Q),
                target: Axis {
                    command: FlightCommand::Roll,
                    scale: -1.0,
                },
            },
            Binding {
                source: S::Key(Key::E),
                target: Axis {
                    command: FlightCommand::Roll,
                    scale: 1.0,
                },
            },
            // Keyboard throttle steps and direct settings.
            Binding {
                source: S::Key(Key::R),
                target: Command(FlightCommand::ThrottleStepUp),
            },
            Binding {
                source: S::Key(Key::F),
                target: Command(FlightCommand::ThrottleStepDown),
            },
            Binding {
                source: S::Key(Key::Digit1),
                target: Command(FlightCommand::ThrottleIdle),
            },
            Binding {
                source: S::Key(Key::Digit4),
                target: Command(FlightCommand::ThrottleFull),
            },
            // Keyboard weapons and systems.
            Binding {
                source: S::Key(Key::Space),
                target: Command(FlightCommand::FirePrimary),
            },
            Binding {
                source: S::Key(Key::LeftControl),
                target: Command(FlightCommand::FireSecondary),
            },
            Binding {
                source: S::Key(Key::Tab),
                target: Command(FlightCommand::CycleWeapon),
            },
            Binding {
                source: S::Key(Key::Z),
                target: Command(FlightCommand::DropOrdnance),
            },
            Binding {
                source: S::Key(Key::X),
                target: Command(FlightCommand::Countermeasure),
            },
            Binding {
                source: S::Key(Key::T),
                target: Command(FlightCommand::TargetNext),
            },
            Binding {
                source: S::Key(Key::G),
                target: Command(FlightCommand::ToggleGear),
            },
            Binding {
                source: S::Key(Key::B),
                target: Command(FlightCommand::FlapStep),
            },
            // Mouse flight mode (a disclosed designed option).
            Binding {
                source: S::MouseAxis(MouseAxis::X),
                target: Axis {
                    command: FlightCommand::Yaw,
                    scale: 1.0,
                },
            },
            Binding {
                source: S::MouseAxis(MouseAxis::Y),
                target: Axis {
                    command: FlightCommand::Pitch,
                    scale: -1.0,
                },
            },
            Binding {
                source: S::MouseButton(MouseButton::Left),
                target: Command(FlightCommand::FirePrimary),
            },
            // Gamepad stick axes and buttons.
            Binding {
                source: S::GamepadAxis(GamepadAxis::LeftStickX),
                target: Axis {
                    command: FlightCommand::Roll,
                    scale: 1.0,
                },
            },
            Binding {
                source: S::GamepadAxis(GamepadAxis::LeftStickY),
                target: Axis {
                    command: FlightCommand::Pitch,
                    scale: -1.0,
                },
            },
            Binding {
                source: S::GamepadAxis(GamepadAxis::RightStickX),
                target: Axis {
                    command: FlightCommand::Yaw,
                    scale: 1.0,
                },
            },
            Binding {
                source: S::GamepadAxis(GamepadAxis::RightTrigger),
                target: Axis {
                    command: FlightCommand::Throttle,
                    scale: 1.0,
                },
            },
            Binding {
                source: S::GamepadButton(GamepadButton::South),
                target: Command(FlightCommand::FirePrimary),
            },
            Binding {
                source: S::GamepadButton(GamepadButton::East),
                target: Command(FlightCommand::FireSecondary),
            },
            // Joystick / HOTAS axes and buttons.
            Binding {
                source: S::JoystickAxis {
                    index: 0,
                    inverted: false,
                },
                target: Axis {
                    command: FlightCommand::Roll,
                    scale: 1.0,
                },
            },
            Binding {
                source: S::JoystickAxis {
                    index: 1,
                    inverted: false,
                },
                target: Axis {
                    command: FlightCommand::Pitch,
                    scale: -1.0,
                },
            },
            Binding {
                source: S::JoystickButton(0),
                target: Command(FlightCommand::FirePrimary),
            },
            Binding {
                source: S::JoystickButton(1),
                target: Command(FlightCommand::FireSecondary),
            },
            // UI navigation.
            Binding {
                source: S::Key(Key::Escape),
                target: Ui(UiAction::Pause),
            },
            Binding {
                source: S::Key(Key::Enter),
                target: Ui(UiAction::Confirm),
            },
            Binding {
                source: S::Key(Key::Backspace),
                target: Ui(UiAction::Cancel),
            },
            Binding {
                source: S::Key(Key::ArrowUp),
                target: Ui(UiAction::NavigateUp),
            },
            Binding {
                source: S::Key(Key::ArrowDown),
                target: Ui(UiAction::NavigateDown),
            },
            Binding {
                source: S::Key(Key::ArrowLeft),
                target: Ui(UiAction::NavigateLeft),
            },
            Binding {
                source: S::Key(Key::ArrowRight),
                target: Ui(UiAction::NavigateRight),
            },
        ];
        Self::try_new(bindings).expect("the designed default action map is conflict-free")
    }

    /// Every target bound to `source`, in insertion order.
    ///
    /// One source may legitimately be bound in two contexts, so a caller that
    /// resolves through the context gate must filter
    /// [`targets_for`](Self::targets_for) with [`InputContext::accepts`]
    /// instead of taking the first entry.
    pub fn targets_for(&self, source: BindingSource) -> impl Iterator<Item = BindingTarget> + '_ {
        self.bindings
            .iter()
            .filter(move |binding| binding.source == source)
            .map(|binding| binding.target)
    }

    /// Every binding whose source reads `channel`, in insertion order.
    ///
    /// A device adapter reports a *channel*; every binding on that channel (in
    /// any direction, since [`BindingSource::JoystickAxis`] carries its own
    /// `inverted` wiring flag) is driven by the same reading.
    pub fn bindings_for_channel(&self, channel: AxisChannel) -> impl Iterator<Item = &Binding> {
        self.bindings
            .iter()
            .filter(move |binding| AxisChannel::from_source(binding.source) == Some(channel))
    }
}

/// One analog channel of a device, the key calibration is stored under.
///
/// Calibration is keyed by `(device, channel)` and not by binding, so moving a
/// binding from one channel of a stick to another does not discard the
/// player's calibration, and a channel no binding uses can still be
/// calibrated. A [`Digital`](BindingSource::is_analog)-opposite source has no
/// channel: only an analog source is calibrated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AxisChannel {
    /// A mouse's motion or wheel axis. The channel is **relative**: a reading
    /// describes the movement of one sample, not a position.
    Mouse(MouseAxis),
    /// A gamepad axis, including its unipolar triggers.
    Gamepad(GamepadAxis),
    /// A joystick or HOTAS raw axis index.
    Joystick(u16),
}

impl AxisChannel {
    /// The channel a binding source reads, or `None` for a digital source.
    #[must_use]
    pub const fn from_source(source: BindingSource) -> Option<Self> {
        match source {
            BindingSource::MouseAxis(axis) => Some(Self::Mouse(axis)),
            BindingSource::GamepadAxis(axis) => Some(Self::Gamepad(axis)),
            BindingSource::JoystickAxis { index, .. } => Some(Self::Joystick(index)),
            BindingSource::Key(_)
            | BindingSource::MouseButton(_)
            | BindingSource::GamepadButton(_)
            | BindingSource::JoystickButton(_) => None,
        }
    }

    /// The stable diagnostic spelling.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Mouse(axis) => format!("mouse.{}", axis.label()),
            Self::Gamepad(axis) => format!("gamepad.{}", axis.label()),
            Self::Joystick(index) => format!("joystick.{index}"),
        }
    }

    /// The device class that reports this channel.
    #[must_use]
    pub const fn device_class(self) -> DeviceClass {
        match self {
            Self::Mouse(_) => DeviceClass::Mouse,
            Self::Gamepad(_) => DeviceClass::Gamepad,
            Self::Joystick(_) => DeviceClass::Joystick,
        }
    }

    /// Whether the channel reports relative motion instead of a position.
    ///
    /// A relative channel is calibrated in the units of one sample, so a
    /// deadzone is not a resting-jitter filter for it and a reading beyond
    /// full deflection is saturation rather than a broken driver.
    #[must_use]
    pub const fn is_relative(self) -> bool {
        matches!(self, Self::Mouse(_))
    }
}

impl fmt::Display for AxisChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label())
    }
}

/// The largest response exponent [`ResponseCurve::Power`] accepts.
pub const MAX_RESPONSE_EXPONENT: f32 = 8.0;

/// The shape applied to a calibrated axis reading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ResponseCurve {
    /// Proportional: the normalized deflection is passed through unchanged.
    Linear,
    /// `sign(x) * |x| ** exponent` over the normalized deflection. An exponent
    /// below one makes the axis more sensitive near neutral, above one makes
    /// it softer near neutral. The exponent must be finite and in
    /// `(0, MAX_RESPONSE_EXPONENT]`.
    Power(f32),
}

impl ResponseCurve {
    /// The largest magnitude this curve can report for a normalized deflection
    /// in `[-1, 1]`; the input magnitude itself.
    #[must_use]
    pub fn apply(self, magnitude: f32) -> f32 {
        match self {
            Self::Linear => magnitude,
            Self::Power(exponent) => magnitude.powf(exponent),
        }
    }

    /// Whether this curve changes the deflection it is given.
    #[must_use]
    pub const fn is_identity(self) -> bool {
        matches!(self, Self::Linear | Self::Power(1.0))
    }
}

/// Why an [`AxisCalibration`] or a calibrated reading was refused.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CalibrationError {
    /// A configured field was NaN or infinite.
    NonFiniteField {
        /// The field's name.
        field: &'static str,
    },
    /// The deadzone fell outside `[0, 1)`. A deadzone of one would make the
    /// axis unreachable.
    DeadzoneOutOfRange {
        /// The rejected deadzone.
        deadzone: f32,
    },
    /// The saturation fell outside `(0, 1]`.
    SaturationOutOfRange {
        /// The rejected saturation.
        saturation: f32,
    },
    /// The analog activation threshold fell outside `(0, 1]`.
    ActivationOutOfRange {
        /// The rejected threshold.
        activation: f32,
    },
    /// The response exponent fell outside `(0, MAX_RESPONSE_EXPONENT]`.
    ExponentOutOfRange {
        /// The rejected exponent.
        exponent: f32,
    },
    /// A raw reading was NaN or infinite.
    NonFiniteReading {
        /// The rejected reading.
        value: f32,
    },
    /// A raw reading fell outside the normalized `[-1, 1]` range an absolute
    /// channel reports. A reading is never clamped or repaired silently.
    ReadingOutOfRange {
        /// The rejected reading.
        value: f32,
    },
}

impl fmt::Display for CalibrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteField { field } => write!(f, "the {field} must be finite"),
            Self::DeadzoneOutOfRange { deadzone } => {
                write!(f, "the deadzone {deadzone} is outside [0, 1)")
            }
            Self::SaturationOutOfRange { saturation } => {
                write!(f, "the saturation {saturation} is outside (0, 1]")
            }
            Self::ActivationOutOfRange { activation } => {
                write!(f, "the activation threshold {activation} is outside (0, 1]")
            }
            Self::ExponentOutOfRange { exponent } => write!(
                f,
                "the response exponent {exponent} is outside (0, {MAX_RESPONSE_EXPONENT}]"
            ),
            Self::NonFiniteReading { value } => {
                write!(f, "the axis reading {value} must be finite")
            }
            Self::ReadingOutOfRange { value } => write!(
                f,
                "the axis reading {value} is outside the normalized [-1, 1] range"
            ),
        }
    }
}

impl std::error::Error for CalibrationError {}

/// The player-facing calibration of one axis of one device.
///
/// Non-negotiable behavior 1 asks for deadzone, inversion, response curve,
/// saturation and device identity. Four of the five are this record; device
/// identity is the key it is stored under in a [`CalibrationStore`].
///
/// The stages are applied in a fixed order so a calibration is reproducible:
///
/// 1. **inversion** — the player's own preference, applied to the raw reading
///    and distinct from a source's `inverted` wiring flag, which describes how
///    the device is physically wired;
/// 2. **deadzone** — a magnitude at or below the deadzone reads as exactly
///    neutral, and the remaining travel is rescaled so full deflection still
///    reads as full deflection;
/// 3. **response curve** — the shape of the remaining travel;
/// 4. **saturation** — the largest magnitude the calibrated axis may report.
///    A stick whose end travel is 85% of nominal therefore reads `0.85` at
///    full deflection instead of being stretched to `1.0`.
///
/// **Designed, not original.** Every field is a newly authored project
/// default; the original game's stick, trigger and mouse shapes are unknown
/// until F22-D measures them. [`AxisCalibration::designed_default`] is the
/// neutral record that changes nothing, so an uncalibrated device is used
/// exactly as it reports itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisCalibration {
    deadzone: f32,
    inverted: bool,
    response: ResponseCurve,
    saturation: f32,
    activation: f32,
}

impl AxisCalibration {
    /// The neutral calibration: no deadzone, no inversion, proportional
    /// response, full deflection and a mid-travel analog activation
    /// threshold.
    ///
    /// The analog activation threshold is not one of the sheet's five listed
    /// fields, but an analog source bound to an edge target (a trigger that
    /// fires the guns) needs a declared crossing point; without it such a
    /// binding could never be pressed or released, and an unplugged device
    /// could leave the weapon firing (non-negotiable behavior 3).
    #[must_use]
    pub const fn designed_default() -> Self {
        Self {
            deadzone: 0.0,
            inverted: false,
            response: ResponseCurve::Linear,
            saturation: 1.0,
            activation: 0.5,
        }
    }

    /// Builds a calibration, refusing every malformed field.
    ///
    /// # Errors
    ///
    /// [`CalibrationError`] naming the first field that is not finite or not in
    /// its range. Nothing is clamped or repaired.
    pub fn try_new(
        deadzone: f32,
        inverted: bool,
        response: ResponseCurve,
        saturation: f32,
        activation: f32,
    ) -> Result<Self, CalibrationError> {
        for (field, value) in [
            ("deadzone", deadzone),
            ("saturation", saturation),
            ("activation", activation),
        ] {
            if !value.is_finite() {
                return Err(CalibrationError::NonFiniteField { field });
            }
        }
        if !(0.0..1.0).contains(&deadzone) {
            return Err(CalibrationError::DeadzoneOutOfRange { deadzone });
        }
        if !(0.0..=1.0).contains(&saturation) || saturation == 0.0 {
            return Err(CalibrationError::SaturationOutOfRange { saturation });
        }
        if !(0.0..=1.0).contains(&activation) || activation == 0.0 {
            return Err(CalibrationError::ActivationOutOfRange { activation });
        }
        if let ResponseCurve::Power(exponent) = response
            && (!exponent.is_finite() || exponent <= 0.0 || exponent > MAX_RESPONSE_EXPONENT)
        {
            return Err(CalibrationError::ExponentOutOfRange { exponent });
        }
        Ok(Self {
            deadzone,
            inverted,
            response,
            saturation,
            activation,
        })
    }

    /// The ignored magnitude.
    #[must_use]
    pub const fn deadzone(self) -> f32 {
        self.deadzone
    }

    /// Whether the reading is inverted by the player's preference.
    #[must_use]
    pub const fn inverted(self) -> bool {
        self.inverted
    }

    /// The response shape.
    #[must_use]
    pub const fn response(self) -> ResponseCurve {
        self.response
    }

    /// The largest magnitude this calibration can report.
    #[must_use]
    pub const fn saturation(self) -> f32 {
        self.saturation
    }

    /// The calibrated magnitude at which an analog source bound to an edge
    /// target counts as pressed.
    #[must_use]
    pub const fn activation(self) -> f32 {
        self.activation
    }

    /// Whether this calibration leaves every reading it is given unchanged
    /// (within the quantization of a floating-point comparison).
    #[must_use]
    pub fn is_identity(self) -> bool {
        self.deadzone == 0.0
            && !self.inverted
            && self.response.is_identity()
            && self.saturation == 1.0
    }

    /// Calibrates one raw reading.
    ///
    /// # Errors
    ///
    /// [`CalibrationError::NonFiniteReading`] for a NaN/infinite reading and
    /// [`CalibrationError::ReadingOutOfRange`] outside `[-1, 1]`. A reading is
    /// never clamped: saturation is this calibration's own declared limit, not
    /// a silent repair of a driver that reports nonsense.
    pub fn apply(self, raw: f32) -> Result<f32, CalibrationError> {
        if !raw.is_finite() {
            return Err(CalibrationError::NonFiniteReading { value: raw });
        }
        if !(-1.0..=1.0).contains(&raw) {
            return Err(CalibrationError::ReadingOutOfRange { value: raw });
        }
        let signed = if self.inverted { -raw } else { raw };
        let magnitude = signed.abs();
        if magnitude <= self.deadzone {
            return Ok(0.0);
        }
        let rescaled = (magnitude - self.deadzone) / (1.0 - self.deadzone);
        let curved = self.response.apply(rescaled);
        Ok(signed.signum() * curved.min(self.saturation))
    }
}

/// One stored calibration record.
#[derive(Clone, Debug, PartialEq)]
struct CalibrationEntry {
    device: DeviceId,
    channel: AxisChannel,
    calibration: AxisCalibration,
}

/// Why a [`CalibrationStore`] operation was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum CalibrationStoreError {
    /// The provisional id is already a stable identity, so there is nothing
    /// to promote away from.
    ProvisionalIsStable {
        /// The id that was offered as provisional.
        device: DeviceId,
    },
    /// The target id is not a stable identity, so calibration must not be
    /// moved onto an index (non-negotiable behavior 1).
    TargetNotStable {
        /// The id that was offered as stable.
        device: DeviceId,
    },
    /// The two ids name different device classes, so they cannot be the same
    /// physical device.
    ClassMismatch {
        /// The provisional id.
        provisional: DeviceId,
        /// The target id.
        stable: DeviceId,
    },
    /// The stable identity already has its own calibration for that channel;
    /// promoting would silently overwrite the player's settings.
    AlreadyCalibrated {
        /// The stable identity that is already calibrated.
        device: DeviceId,
        /// The channel that is already calibrated on it.
        channel: AxisChannel,
    },
}

impl fmt::Display for CalibrationStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProvisionalIsStable { device } => write!(
                f,
                "{device} is already a stable identity, so it has no provisional calibration"
            ),
            Self::TargetNotStable { device } => write!(
                f,
                "{device} is not a stable identity, so calibration must not be moved onto it"
            ),
            Self::ClassMismatch {
                provisional,
                stable,
            } => write!(f, "{provisional} and {stable} are different device classes"),
            Self::AlreadyCalibrated { device, channel } => {
                write!(f, "{device} already has a calibration for {channel}")
            }
        }
    }
}

impl std::error::Error for CalibrationStoreError {}

/// The per-device, per-channel calibration of a session.
///
/// Non-negotiable behavior 1: a device is keyed by its
/// [`DeviceIdentity`], never by an enumeration index alone. A record stored
/// under [`DeviceIdentity::EnumerationFallback`] works for the current
/// session but is reported by [`unstable_devices`](Self::unstable_devices) so
/// it is never persisted as if it were an identity, and
/// [`promote`](Self::promote) moves it onto the stable identity as soon as the
/// platform reports one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CalibrationStore {
    entries: Vec<CalibrationEntry>,
}

impl CalibrationStore {
    /// An empty store: every device uses
    /// [`AxisCalibration::designed_default`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Records the calibration of one channel of one device, replacing any
    /// earlier record for the same pair.
    pub fn set(&mut self, device: &DeviceId, channel: AxisChannel, calibration: AxisCalibration) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.device == *device && entry.channel == channel)
        {
            entry.calibration = calibration;
        } else {
            self.entries.push(CalibrationEntry {
                device: device.clone(),
                channel,
                calibration,
            });
        }
    }

    /// The recorded calibration of a channel, or `None` when the channel is
    /// uncalibrated.
    #[must_use]
    pub fn get(&self, device: &DeviceId, channel: AxisChannel) -> Option<AxisCalibration> {
        self.entries
            .iter()
            .find(|entry| entry.device == *device && entry.channel == channel)
            .map(|entry| entry.calibration)
    }

    /// The calibration in force for a channel: the recorded one, or
    /// [`AxisCalibration::designed_default`] when the channel is uncalibrated.
    #[must_use]
    pub fn calibration_or_default(
        &self,
        device: &DeviceId,
        channel: AxisChannel,
    ) -> AxisCalibration {
        self.get(device, channel)
            .unwrap_or_else(AxisCalibration::designed_default)
    }

    /// Drops one record, returning the calibration it held.
    pub fn forget(&mut self, device: &DeviceId, channel: AxisChannel) -> Option<AxisCalibration> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.device == *device && entry.channel == channel)?;
        Some(self.entries.remove(index).calibration)
    }

    /// Drops every record of a device, returning how many were dropped.
    ///
    /// The device adapter calls this when a device is removed, so a later
    /// device that happens to enumerate at the same index does not inherit
    /// another device's calibration.
    pub fn forget_device(&mut self, device: &DeviceId) -> usize {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.device != *device);
        before - self.entries.len()
    }

    /// Moves every record of a provisional id onto a stable identity.
    ///
    /// This is how a device that first enumerated without an identity keeps its
    /// calibration: the same physical stick is re-keyed onto the identity the
    /// platform reports, and the record never has to be persisted against an
    /// index.
    ///
    /// # Errors
    ///
    /// [`CalibrationStoreError`] when `provisional` is already stable, when
    /// `stable` is not a stable identity, when the two are different device
    /// classes, or when `stable` already has its own record for one of the
    /// channels. Nothing is moved when the promotion is refused.
    pub fn promote(
        &mut self,
        provisional: &DeviceId,
        stable: &DeviceId,
    ) -> Result<usize, CalibrationStoreError> {
        if provisional.is_stable() {
            return Err(CalibrationStoreError::ProvisionalIsStable {
                device: provisional.clone(),
            });
        }
        if !stable.is_stable() {
            return Err(CalibrationStoreError::TargetNotStable {
                device: stable.clone(),
            });
        }
        if provisional.class() != stable.class() {
            return Err(CalibrationStoreError::ClassMismatch {
                provisional: provisional.clone(),
                stable: stable.clone(),
            });
        }
        let channels: Vec<AxisChannel> = self
            .entries
            .iter()
            .filter(|entry| entry.device == *provisional)
            .map(|entry| entry.channel)
            .collect();
        if let Some(channel) = channels
            .iter()
            .find(|channel| self.get(stable, **channel).is_some())
        {
            return Err(CalibrationStoreError::AlreadyCalibrated {
                device: stable.clone(),
                channel: *channel,
            });
        }
        for entry in &mut self.entries {
            if entry.device == *provisional {
                entry.device = stable.clone();
            }
        }
        Ok(channels.len())
    }

    /// The devices whose calibration is keyed by an enumeration index.
    ///
    /// A persistence writer must refuse these records (non-negotiable
    /// behavior 1): an index is not an identity.
    #[must_use]
    pub fn unstable_devices(&self) -> Vec<&DeviceId> {
        let mut devices: Vec<&DeviceId> = self
            .entries
            .iter()
            .filter(|entry| !entry.device.is_stable())
            .map(|entry| &entry.device)
            .collect();
        devices.sort();
        devices.dedup();
        devices
    }

    /// Every record, in insertion order.
    pub fn entries(&self) -> impl Iterator<Item = (&DeviceId, AxisChannel, AxisCalibration)> {
        self.entries
            .iter()
            .map(|entry| (&entry.device, entry.channel, entry.calibration))
    }

    /// How many records are stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store holds no record.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim_action(label: &str) -> Action {
        Action::from_label(label).expect("the test action label is known")
    }

    /// AC01's map half and non-negotiable behavior 5: the same physical
    /// source resolves differently per context, and text entry and cinematics
    /// emit neither flight nor UI actions.
    #[test]
    fn accept_f22_a_action_map_gates_text_entry_and_ui_navigation() {
        let map = ActionMap::designed_default();

        assert_eq!(
            map.resolve(InputContext::Flight, BindingSource::Key(Key::Space)),
            Some(Action::Flight(FlightCommand::FirePrimary)),
            "space fires the primary weapons in flight"
        );
        assert_eq!(
            map.resolve(InputContext::TextEntry, BindingSource::Key(Key::Space)),
            None,
            "text entry must not also fire weapons"
        );
        assert_eq!(
            map.resolve(InputContext::Cinematic, BindingSource::Key(Key::Space)),
            None,
            "a cinematic emits no local action"
        );

        assert_eq!(
            map.resolve(InputContext::UiNavigation, BindingSource::Key(Key::Escape)),
            Some(Action::Ui(UiAction::Pause)),
            "escape pauses in a menu"
        );
        assert_eq!(
            map.resolve(InputContext::Flight, BindingSource::Key(Key::Escape)),
            None,
            "a UI action is not a flight command"
        );
        assert_eq!(
            map.resolve(InputContext::Flight, BindingSource::Key(Key::ArrowUp)),
            None,
            "navigation is not a flight command"
        );

        assert_eq!(
            map.resolve(InputContext::Flight, BindingSource::Key(Key::Digit2)),
            None,
            "an unbound source resolves to nothing"
        );
    }

    /// Non-negotiable behavior 5: a conflicted binding set is refused and
    /// named, and malformed targets are refused before a map exists.
    #[test]
    fn accept_f22_a_action_map_rejects_conflicts_and_malformed_targets() {
        let conflict = ActionMap::try_new(vec![
            Binding {
                source: BindingSource::Key(Key::Space),
                target: BindingTarget::Command(FlightCommand::FirePrimary),
            },
            Binding {
                source: BindingSource::Key(Key::Space),
                target: BindingTarget::Command(FlightCommand::Eject),
            },
        ]);
        assert_eq!(
            conflict,
            Err(ActionMapError::ConflictingBinding {
                source: BindingSource::Key(Key::Space),
                first: Action::Flight(FlightCommand::FirePrimary),
                second: Action::Flight(FlightCommand::Eject),
            })
        );

        // The same source in two different contexts is not a conflict.
        let split = ActionMap::try_new(vec![
            Binding {
                source: BindingSource::Key(Key::Space),
                target: BindingTarget::Command(FlightCommand::FirePrimary),
            },
            Binding {
                source: BindingSource::Key(Key::Space),
                target: BindingTarget::Ui(UiAction::Confirm),
            },
        ]);
        assert!(split.is_ok(), "one source may serve two contexts");

        assert_eq!(
            ActionMap::try_new(vec![Binding {
                source: BindingSource::Key(Key::W),
                target: BindingTarget::Axis {
                    command: FlightCommand::FirePrimary,
                    scale: 1.0,
                },
            }]),
            Err(ActionMapError::AxisTargetNotContinuous {
                command: FlightCommand::FirePrimary
            })
        );
        assert_eq!(
            ActionMap::try_new(vec![Binding {
                source: BindingSource::Key(Key::W),
                target: BindingTarget::Command(FlightCommand::Pitch),
            }]),
            Err(ActionMapError::CommandTargetNotDiscrete {
                command: FlightCommand::Pitch
            })
        );
        assert_eq!(
            ActionMap::try_new(vec![Binding {
                source: BindingSource::Key(Key::W),
                target: BindingTarget::Axis {
                    command: FlightCommand::Pitch,
                    scale: 0.0,
                },
            }]),
            Err(ActionMapError::ZeroScale {
                source: BindingSource::Key(Key::W)
            })
        );
        assert_eq!(
            ActionMap::try_new(vec![Binding {
                source: BindingSource::Key(Key::W),
                target: BindingTarget::Axis {
                    command: FlightCommand::Pitch,
                    scale: f32::NAN,
                },
            }]),
            Err(ActionMapError::NonFiniteScale {
                source: BindingSource::Key(Key::W)
            })
        );
    }

    /// A source legitimately bound in two contexts resolves to the target of
    /// whichever context is active, regardless of the bindings' insertion
    /// order. A `resolve` that returned only the first binding for a source
    /// would silently drop the other context's action when that binding is
    /// not the active one.
    #[test]
    fn accept_f22_a_multi_context_binding_resolves_in_both_orders() {
        // The UI binding is inserted first, the flight binding second.
        let ui_first = ActionMap::try_new(vec![
            Binding {
                source: BindingSource::Key(Key::Space),
                target: BindingTarget::Ui(UiAction::Confirm),
            },
            Binding {
                source: BindingSource::Key(Key::Space),
                target: BindingTarget::Command(FlightCommand::FirePrimary),
            },
        ])
        .expect("one source may serve two contexts");
        assert_eq!(
            ui_first.resolve(InputContext::UiNavigation, BindingSource::Key(Key::Space)),
            Some(Action::Ui(UiAction::Confirm)),
            "the UI binding wins in UI context when it is first"
        );
        assert_eq!(
            ui_first.resolve(InputContext::Flight, BindingSource::Key(Key::Space)),
            Some(Action::Flight(FlightCommand::FirePrimary)),
            "the flight binding still wins in flight context when a UI binding precedes it"
        );
        assert_eq!(
            ui_first.resolve(InputContext::TextEntry, BindingSource::Key(Key::Space)),
            None,
            "text entry accepts neither bound action"
        );

        // The symmetric order: flight first, UI second.
        let flight_first = ActionMap::try_new(vec![
            Binding {
                source: BindingSource::Key(Key::Space),
                target: BindingTarget::Command(FlightCommand::FirePrimary),
            },
            Binding {
                source: BindingSource::Key(Key::Space),
                target: BindingTarget::Ui(UiAction::Confirm),
            },
        ])
        .expect("one source may serve two contexts");
        assert_eq!(
            flight_first.resolve(InputContext::UiNavigation, BindingSource::Key(Key::Space)),
            Some(Action::Ui(UiAction::Confirm)),
            "the UI binding wins in UI context when the flight binding is first"
        );
        assert_eq!(
            flight_first.resolve(InputContext::Flight, BindingSource::Key(Key::Space)),
            Some(Action::Flight(FlightCommand::FirePrimary)),
            "the flight binding wins in flight context when it is first"
        );
    }

    /// Non-negotiable behavior 1: a device is identified by a stable identity
    /// where one exists; only the explicitly flagged enumeration fallback is
    /// index-based, and it is never called stable.
    #[test]
    fn accept_f22_a_device_identity_prefers_stable_identity_over_enumeration() {
        let stable = DeviceId::stable(DeviceClass::Joystick, "usb:vid=1234&pid=5678#0")
            .expect("a platform path is a valid stable identity");
        assert!(stable.is_stable());
        assert_eq!(stable.class(), DeviceClass::Joystick);
        assert_eq!(
            stable.identity(),
            &DeviceIdentity::Stable("usb:vid=1234&pid=5678#0".to_owned())
        );
        assert!(stable.label().starts_with("joystick:"));

        let fallback = DeviceId::enumeration_fallback(DeviceClass::Gamepad, 2);
        assert!(!fallback.is_stable());
        assert_eq!(fallback.identity(), &DeviceIdentity::EnumerationFallback(2));
        assert_ne!(stable, fallback);

        assert_eq!(
            DeviceId::stable(DeviceClass::Keyboard, ""),
            Err(DeviceIdError::Empty)
        );
        assert_eq!(
            DeviceId::stable(DeviceClass::Keyboard, "line\nbreak"),
            Err(DeviceIdError::ControlCharacter { ch: '\n' })
        );
        let long = "d".repeat(MAX_DEVICE_IDENTITY_LEN + 1);
        assert_eq!(
            DeviceId::stable(DeviceClass::Keyboard, &long),
            Err(DeviceIdError::TooLong {
                len: MAX_DEVICE_IDENTITY_LEN + 1
            })
        );
    }

    /// Continuous values are quantized, validated and never repaired; an
    /// edge command cannot masquerade as an axis.
    #[test]
    fn accept_f22_a_axis_values_are_quantized_and_validated() {
        let full = AxisValue::from_unit(FlightCommand::Throttle, 1.0).expect("full scale");
        assert_eq!(full.quantized(), AXIS_QUANT_MAX);
        assert!((full.as_unit() - 1.0).abs() < 1e-6);

        let neutral = AxisValue::from_unit(FlightCommand::Pitch, 0.0).expect("neutral");
        assert_eq!(neutral.quantized(), 0);
        assert_eq!(neutral.as_unit(), 0.0);

        let negative = AxisValue::from_unit(FlightCommand::Roll, -1.0).expect("negative full");
        assert_eq!(negative.quantized(), -AXIS_QUANT_MAX);

        let round_trip =
            AxisValue::from_quantized(FlightCommand::Yaw, -12345).expect("a quantized sample");
        assert_eq!(round_trip.quantized(), -12345);
        assert_eq!(
            AxisValue::from_quantized(FlightCommand::FirePrimary, 0),
            Err(AxisValueError::NotContinuous {
                command: FlightCommand::FirePrimary
            })
        );
        assert_eq!(
            AxisValue::from_unit(FlightCommand::Pitch, f32::NAN),
            Err(AxisValueError::NonFinite {
                command: FlightCommand::Pitch
            })
        );
        assert_eq!(
            AxisValue::from_unit(FlightCommand::Pitch, 1.5),
            Err(AxisValueError::OutOfRange {
                command: FlightCommand::Pitch,
                value: 1.5
            })
        );
    }

    /// The materialized default map is conflict-free, covers every device
    /// class, and the vocabulary labels are unique and round-trip.
    #[test]
    fn accept_f22_a_vocabulary_and_default_map_are_well_formed() {
        let map = ActionMap::designed_default();
        assert!(
            map.bindings().len() > 20,
            "the designed default binds a useful set"
        );
        let classes: Vec<DeviceClass> = map
            .bindings()
            .iter()
            .map(|binding| binding.source.device_class())
            .collect();
        for class in DeviceClass::ALL {
            assert!(
                classes.contains(class),
                "the designed default covers {class}"
            );
        }

        let mut labels: Vec<&str> = FlightCommand::ALL
            .iter()
            .map(|command| command.label())
            .collect();
        let count = labels.len();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), count, "flight command labels are unique");
        for command in FlightCommand::ALL {
            assert_eq!(FlightCommand::from_label(command.label()), Some(*command));
        }
        for action in UiAction::ALL {
            assert_eq!(UiAction::from_label(action.label()), Some(*action));
            assert_eq!(
                Action::from_label(action.label()),
                Some(Action::Ui(*action))
            );
        }
        assert_eq!(
            Action::from_label("fire_primary"),
            Some(Action::Flight(FlightCommand::FirePrimary))
        );
        assert_eq!(Action::from_label("nonsense"), None);
        assert!(FlightCommand::Pitch.is_continuous());
        assert!(!FlightCommand::FirePrimary.is_continuous());
        assert_eq!(FlightCommand::CONTINUOUS.len(), 4);

        for key in Key::ALL {
            assert_eq!(Key::from_label(key.label()), Some(*key));
        }
        assert_eq!(
            DeviceClass::from_label("gamepad"),
            Some(DeviceClass::Gamepad)
        );
        assert_eq!(
            BindingSource::JoystickAxis {
                index: 3,
                inverted: true
            }
            .device_class(),
            DeviceClass::Joystick
        );

        // The labels used by the tests above are the vocabulary's own.
        assert_eq!(
            claim_action("fire_primary"),
            Action::Flight(FlightCommand::FirePrimary)
        );
    }

    /// The joystick of the calibration tests: one stable stick plus the
    /// provisional id the same device has before the platform reports it.
    fn test_joystick() -> DeviceId {
        DeviceId::stable(DeviceClass::Joystick, "joy.stick.test/0")
            .expect("the test identity is a valid stable identity")
    }

    /// A calibration with one field changed from
    /// [`AxisCalibration::designed_default`].
    fn calibration(deadzone: f32, inverted: bool, power: f32, saturation: f32) -> AxisCalibration {
        AxisCalibration::try_new(
            deadzone,
            inverted,
            ResponseCurve::Power(power),
            saturation,
            0.5,
        )
        .expect("the test calibration is in range")
    }

    /// Non-negotiable behavior 1, the four calibration stages: deadzone,
    /// inversion, response curve and saturation, applied in the documented
    /// order. An implementation that dropped any stage (or applied the
    /// deadzone without rescaling the remaining travel) changes these values.
    #[test]
    fn accept_f22_b_axis_calibration_applies_deadzone_inversion_curve_and_saturation() {
        let identity = AxisCalibration::designed_default();
        assert!(identity.is_identity());
        assert_eq!(
            identity.activation(),
            0.5,
            "a designed mid-travel threshold"
        );
        for raw in [-1.0_f32, -0.5, 0.0, 0.25, 1.0] {
            assert_eq!(
                identity.apply(raw),
                Ok(raw),
                "the designed default changes nothing"
            );
        }

        // Deadzone: at or below it reads exactly neutral, and the remaining
        // travel is rescaled so full deflection still reads as full deflection.
        let deadzone = calibration(0.25, false, 1.0, 1.0);
        assert_eq!(
            deadzone.apply(0.25),
            Ok(0.0),
            "the deadzone edge is neutral"
        );
        assert_eq!(deadzone.apply(-0.25), Ok(0.0));
        assert_eq!(deadzone.apply(0.0), Ok(0.0));
        assert_eq!(
            deadzone.apply(0.1),
            Ok(0.0),
            "a reading inside the dead zone is exactly neutral, never the small \
             reversed deflection the rescale would produce on its own"
        );
        assert_eq!(
            deadzone.apply(-0.1),
            Ok(0.0),
            "a reading inside the dead zone is exactly neutral in both directions"
        );
        let half = deadzone.apply(0.625).expect("a valid reading");
        assert!(
            (half - 0.5).abs() < 1e-6,
            "half of the travel past the deadzone reads as half scale, got {half}"
        );
        assert_eq!(deadzone.apply(1.0), Ok(1.0), "full travel is unaffected");

        // Inversion is the player's preference and flips the sign only.
        let inverted = calibration(0.0, true, 1.0, 1.0);
        assert_eq!(inverted.apply(0.5), Ok(-0.5));
        assert!(inverted.inverted());

        // A power curve shapes the travel: 2.0 is softer near neutral, 0.5 is
        // sharper.
        let soft = calibration(0.0, false, 2.0, 1.0);
        let sharp = calibration(0.0, false, 0.5, 1.0);
        let soft_half = soft.apply(0.5).expect("a valid reading");
        let sharp_half = sharp.apply(0.5).expect("a valid reading");
        assert!(
            soft_half < 0.5 && sharp_half > 0.5,
            "the curve must change the reading, got soft={soft_half} sharp={sharp_half}"
        );
        assert_eq!(soft.apply(1.0), Ok(1.0), "a curve leaves the ends alone");

        // Saturation is a declared ceiling, not a stretch: limited end travel
        // reads as the limit instead of being normalized to full scale.
        let limited = calibration(0.0, false, 1.0, 0.85);
        assert_eq!(
            limited.apply(1.0),
            Ok(0.85),
            "the stick keeps its 85% travel"
        );
        assert_eq!(limited.apply(-1.0), Ok(-0.85));
        assert!(!limited.is_identity());
        assert_eq!(limited.saturation(), 0.85);

        // The stages compose in the documented order: inversion, then
        // deadzone, then curve, then saturation.
        let composed = AxisCalibration::try_new(0.2, true, ResponseCurve::Power(2.0), 0.5, 0.75)
            .expect("the composed calibration is in range");
        assert_eq!(
            composed.apply(0.2),
            Ok(0.0),
            "the deadzone wins over the curve"
        );
        let curved = composed.apply(0.6).expect("a valid reading");
        let expected = -0.5_f32.powf(2.0).min(0.5);
        assert!(
            (curved - expected).abs() < 1e-6,
            "inversion then deadzone rescale then curve then saturation, got {curved}"
        );
    }

    /// Malformed calibration fields and malformed readings are refused by
    /// name, never clamped or repaired.
    #[test]
    fn accept_f22_b_axis_calibration_refuses_malformed_fields_and_readings() {
        assert_eq!(
            AxisCalibration::try_new(f32::NAN, false, ResponseCurve::Linear, 1.0, 0.5),
            Err(CalibrationError::NonFiniteField { field: "deadzone" })
        );
        assert_eq!(
            AxisCalibration::try_new(1.0, false, ResponseCurve::Linear, 1.0, 0.5),
            Err(CalibrationError::DeadzoneOutOfRange { deadzone: 1.0 })
        );
        assert_eq!(
            AxisCalibration::try_new(0.0, false, ResponseCurve::Linear, 0.0, 0.5),
            Err(CalibrationError::SaturationOutOfRange { saturation: 0.0 })
        );
        assert_eq!(
            AxisCalibration::try_new(0.0, false, ResponseCurve::Linear, f32::INFINITY, 0.5),
            Err(CalibrationError::NonFiniteField {
                field: "saturation"
            })
        );
        assert_eq!(
            AxisCalibration::try_new(0.0, false, ResponseCurve::Linear, 1.0, 0.0),
            Err(CalibrationError::ActivationOutOfRange { activation: 0.0 })
        );
        assert_eq!(
            AxisCalibration::try_new(0.0, false, ResponseCurve::Power(0.0), 1.0, 0.5),
            Err(CalibrationError::ExponentOutOfRange { exponent: 0.0 })
        );
        assert_eq!(
            AxisCalibration::try_new(
                0.0,
                false,
                ResponseCurve::Power(MAX_RESPONSE_EXPONENT + 1.0),
                1.0,
                0.5
            ),
            Err(CalibrationError::ExponentOutOfRange {
                exponent: MAX_RESPONSE_EXPONENT + 1.0
            })
        );

        let identity = AxisCalibration::designed_default();
        assert!(
            matches!(
                identity.apply(f32::NAN),
                Err(CalibrationError::NonFiniteReading { value }) if value.is_nan()
            ),
            "a NaN reading is refused, never propagated into an axis value"
        );
        assert_eq!(
            identity.apply(1.5),
            Err(CalibrationError::ReadingOutOfRange { value: 1.5 })
        );
        assert!(
            identity.apply(1.5).is_err(),
            "a reading beyond full scale must not be silently saturated"
        );
    }

    /// Non-negotiable behavior 1's device identity half: calibration is keyed
    /// by the device, survives a change of enumeration index, and is never
    /// persisted against an index. A store that keyed calibration by anything
    /// other than the stable identity, or that lost it on a re-enumeration,
    /// fails this test.
    #[test]
    fn accept_f22_b_calibration_is_keyed_by_device_identity_not_enumeration() {
        let device = test_joystick();
        let roll = AxisChannel::Joystick(0);
        let other_index = DeviceId::enumeration_fallback(DeviceClass::Joystick, 0);
        let later_index = DeviceId::enumeration_fallback(DeviceClass::Joystick, 7);
        let deadzone = calibration(0.3, false, 1.0, 1.0);

        let mut store = CalibrationStore::new();
        assert!(store.is_empty());
        assert_eq!(store.get(&device, roll), None);
        assert_eq!(
            store.calibration_or_default(&device, roll),
            AxisCalibration::designed_default(),
            "an uncalibrated channel is used exactly as it reports itself"
        );

        // The same physical stick, calibrated while it had no identity yet.
        store.set(&other_index, roll, deadzone);
        assert_eq!(store.get(&other_index, roll), Some(deadzone));
        assert_eq!(store.len(), 1);
        assert_eq!(
            store.unstable_devices(),
            vec![&other_index],
            "an index-keyed record must be visible as persistable-unsafe"
        );

        // It enumerates at a different index this session: the index-keyed
        // record is not what the player calibrated, so the new index starts
        // from the default rather than inheriting it.
        assert_eq!(store.get(&later_index, roll), None);
        assert_eq!(
            store.calibration_or_default(&later_index, roll),
            AxisCalibration::designed_default()
        );

        // The platform reports the stable identity: the calibration moves onto
        // it, and the record stops being persistable-unsafe.
        let moved = store
            .promote(&other_index, &device)
            .expect("a provisional joystick promotes onto its stable identity");
        assert_eq!(moved, 1);
        assert_eq!(store.get(&device, roll), Some(deadzone));
        assert_eq!(store.get(&other_index, roll), None);
        assert!(
            store.unstable_devices().is_empty(),
            "nothing is keyed by an index any more"
        );
        assert_eq!(store.len(), 1, "promotion moves, it does not copy");

        // Across a reboot the stick comes back at yet another index but with
        // the same stable identity, so it keeps its calibration.
        store.set(&later_index, roll, deadzone);
        assert_eq!(
            store.promote(&later_index, &device),
            Err(CalibrationStoreError::AlreadyCalibrated {
                device: device.clone(),
                channel: roll,
            }),
            "promotion must not silently overwrite an existing record"
        );
        assert_eq!(
            store.get(&later_index, roll),
            Some(deadzone),
            "atomic refusal"
        );
        assert_eq!(store.get(&device, roll), Some(deadzone), "atomic refusal");

        // A refused promotion for a wrong class or a non-stable target is also
        // refused by name.
        let gamepad = DeviceId::stable(DeviceClass::Gamepad, "pad.test/0")
            .expect("the test identity is valid");
        assert_eq!(
            store.promote(&later_index, &gamepad),
            Err(CalibrationStoreError::ClassMismatch {
                provisional: later_index.clone(),
                stable: gamepad.clone(),
            })
        );
        assert_eq!(
            store.promote(&later_index, &later_index),
            Err(CalibrationStoreError::TargetNotStable {
                device: later_index.clone()
            })
        );
        assert_eq!(
            store.promote(&device, &device),
            Err(CalibrationStoreError::ProvisionalIsStable {
                device: device.clone()
            })
        );
    }

    /// The channel vocabulary is the calibration key, so it must cover every
    /// analog source and refuse every digital one.
    #[test]
    fn accept_f22_b_axis_channels_cover_every_analog_source() {
        assert_eq!(
            AxisChannel::from_source(BindingSource::MouseAxis(MouseAxis::X)),
            Some(AxisChannel::Mouse(MouseAxis::X))
        );
        assert_eq!(
            AxisChannel::from_source(BindingSource::GamepadAxis(GamepadAxis::RightTrigger)),
            Some(AxisChannel::Gamepad(GamepadAxis::RightTrigger))
        );
        assert_eq!(
            AxisChannel::from_source(BindingSource::JoystickAxis {
                index: 4,
                inverted: true
            }),
            Some(AxisChannel::Joystick(4)),
            "a source's inversion flag is a wiring fact and does not split the channel"
        );
        for source in [
            BindingSource::Key(Key::W),
            BindingSource::MouseButton(MouseButton::Left),
            BindingSource::GamepadButton(GamepadButton::South),
            BindingSource::JoystickButton(2),
        ] {
            assert_eq!(
                AxisChannel::from_source(source),
                None,
                "a digital source has no calibration channel"
            );
        }
        assert_eq!(
            AxisChannel::Joystick(2).label(),
            "joystick.2",
            "labels are the vocabulary's own"
        );
        assert!(AxisChannel::Mouse(MouseAxis::X).is_relative());
        assert!(!AxisChannel::Joystick(0).is_relative());
        assert_eq!(
            AxisChannel::Gamepad(GamepadAxis::LeftStickY).device_class(),
            DeviceClass::Gamepad
        );

        // Every analog binding of the designed default map has a channel, and
        // the map can be walked by channel.
        let map = ActionMap::designed_default();
        let mut channels: Vec<AxisChannel> = map
            .bindings()
            .iter()
            .filter_map(|binding| AxisChannel::from_source(binding.source))
            .collect();
        let count = channels.len();
        channels.sort();
        channels.dedup();
        assert!(
            count > 4,
            "the designed default binds several analog channels"
        );
        for channel in channels {
            assert!(
                map.bindings_for_channel(channel).next().is_some(),
                "{channel} must be walkable from the map"
            );
        }
        assert_eq!(
            map.bindings_for_channel(AxisChannel::Joystick(0))
                .map(|binding| binding.target.action())
                .collect::<Vec<_>>(),
            vec![Action::Flight(FlightCommand::Roll)]
        );
    }
}
