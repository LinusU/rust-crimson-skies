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
}
