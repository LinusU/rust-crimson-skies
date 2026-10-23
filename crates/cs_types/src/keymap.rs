//! The player-facing control map: the original game's default bindings and
//! its rebinding rules (PARITY2-ORIGINAL-KEYBINDINGS, #1733).
//!
//! [`ControlsMap`] holds one [`ControlRow`] per original command id, each with
//! the packed inputs the original's binding store keeps: key A, key B, a
//! joystick button and a mouse button. It starts as the recorded original
//! default ([`ORIGINAL_DEFAULT_BINDINGS`]) and obeys the two rules the
//! original's binding code applies, both **static** evidence
//! (`OWNER-STATIC-2026-10-05`, never `verified_original`):
//!
//! * **Modifier exactness** ([`ORIGINAL_MODIFIER_MATCH_SPAN`]): when the plain
//!   key is bound and some modifier combination of the same scancode is also
//!   bound, the held modifiers must match a bound code exactly; when no
//!   combination is bound the held modifiers are ignored
//!   ([`ControlsMap::commands_for_key`]).
//! * **Slot uniqueness**: assigning an input to a command clears it from any
//!   other command in the same slot — key A, key B, joystick button and mouse
//!   button separately ([`ControlsMap::assign`]).
//!
//! The map is the one source the live game derives its keyboard
//! [`ActionMap`] from ([`ControlsMap::to_action_map`]) and the help screens
//! list ([`ControlsMap::help_lines`]). A command with no project action yet
//! (the camera views, weapon-group cycling, nitro, auto-dock) is still bound
//! and listed; it simply delivers nothing until its consumer exists.
//!
//! Unmeasured and therefore not claimed here: the keyboard control rate
//! constants, the analog calibration and the original's rebind screen itself.

use crate::input::{
    Action, ActionMap, Binding, BindingSource, BindingTarget, FlightCommand,
    JOYSTICK_AXIS_THROTTLE, JOYSTICK_AXIS_TWIST, JOYSTICK_AXIS_X, JOYSTICK_AXIS_Y, Key,
    MouseButton, ORIGINAL_DEFAULT_BINDINGS, ORIGINAL_JOYSTICK_AXES, ORIGINAL_KEY_ALT,
    ORIGINAL_KEY_CTRL, ORIGINAL_KEY_SHIFT, OriginalControlCategory, OriginalJoystickAxis,
    OriginalKeyCode, UiAction, original_scancode,
};

/// The Alt, Ctrl and Shift state held while a key goes down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Either Alt key is held.
    pub alt: bool,
    /// Either Ctrl key is held.
    pub ctrl: bool,
    /// Either Shift key is held.
    pub shift: bool,
}

impl Modifiers {
    /// No modifier held.
    pub const NONE: Self = Self {
        alt: false,
        ctrl: false,
        shift: false,
    };

    /// The packed modifier bits of an original key code.
    pub const fn bits(self) -> u16 {
        (if self.alt { ORIGINAL_KEY_ALT } else { 0 })
            | (if self.ctrl { ORIGINAL_KEY_CTRL } else { 0 })
            | (if self.shift { ORIGINAL_KEY_SHIFT } else { 0 })
    }
}

/// One of the four independent slots a command's input lives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ControlSlot {
    /// The primary key.
    KeyA,
    /// The secondary key.
    KeyB,
    /// The joystick button.
    JoystickButton,
    /// The mouse button.
    MouseButton,
}

/// An input to assign to a [`ControlSlot`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlInput {
    /// A packed key code (a scancode plus modifier bits), for a key slot.
    Key(OriginalKeyCode),
    /// A 1-based joystick button (1–10).
    JoystickButton(u8),
    /// A mouse button: `1` left, `2` right, `3` middle.
    MouseButton(u8),
}

impl ControlInput {
    /// The slots this input may be assigned to.
    const fn fits(self, slot: ControlSlot) -> bool {
        matches!(
            (self, slot),
            (Self::Key(_), ControlSlot::KeyA | ControlSlot::KeyB)
                | (Self::JoystickButton(_), ControlSlot::JoystickButton)
                | (Self::MouseButton(_), ControlSlot::MouseButton)
        )
    }
}

/// Why an assignment was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlsError {
    /// The map holds no such command id.
    UnknownCommand {
        /// The command id.
        command_id: u16,
    },
    /// The input is not of the kind the slot holds.
    WrongSlot,
    /// The joystick or mouse button number is outside the original's range.
    ButtonOutOfRange,
}

impl std::fmt::Display for ControlsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownCommand { command_id } => {
                write!(f, "no control command {command_id}")
            }
            Self::WrongSlot => f.write_str("that input does not fit the slot"),
            Self::ButtonOutOfRange => f.write_str("button number outside the original's range"),
        }
    }
}

impl std::error::Error for ControlsError {}

/// One command's inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlRow {
    /// The original's command id.
    pub command_id: u16,
    /// The category the controls screen groups the command under.
    pub category: OriginalControlCategory,
    /// The primary key.
    pub key_a: OriginalKeyCode,
    /// The secondary key.
    pub key_b: OriginalKeyCode,
    /// The 1-based joystick button, `0` none.
    pub joystick_button: u8,
    /// The mouse button, `0` none.
    pub mouse_button: u8,
}

/// The rebindable control table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlsMap {
    rows: Vec<ControlRow>,
}

impl Default for ControlsMap {
    fn default() -> Self {
        Self::original_default()
    }
}

impl ControlsMap {
    /// The original's default table: 64 commands, in the controls screen's
    /// order.
    pub fn original_default() -> Self {
        Self {
            rows: ORIGINAL_DEFAULT_BINDINGS
                .iter()
                .map(|row| ControlRow {
                    command_id: row.command_id,
                    category: row.category,
                    key_a: row.key_a,
                    key_b: row.key_b,
                    joystick_button: row.joystick_button,
                    mouse_button: row.mouse_button,
                })
                .collect(),
        }
    }

    /// Every row, in the controls screen's order.
    pub fn rows(&self) -> &[ControlRow] {
        &self.rows
    }

    /// The row for `command_id`.
    pub fn row(&self, command_id: u16) -> Option<&ControlRow> {
        self.rows.iter().find(|row| row.command_id == command_id)
    }

    /// Assigns `input` to `slot` of `command_id`, clearing it from every other
    /// command's same slot first (slot uniqueness). Returns the command ids
    /// that lost the input.
    ///
    /// # Errors
    ///
    /// [`ControlsError`] for an unknown command, an input of the wrong kind
    /// for the slot or a button number outside the original's range; a
    /// refused assignment changes nothing.
    pub fn assign(
        &mut self,
        command_id: u16,
        slot: ControlSlot,
        input: ControlInput,
    ) -> Result<Vec<u16>, ControlsError> {
        if self.row(command_id).is_none() {
            return Err(ControlsError::UnknownCommand { command_id });
        }
        if !input.fits(slot) {
            return Err(ControlsError::WrongSlot);
        }
        match input {
            ControlInput::JoystickButton(button) if !(1..=10).contains(&button) => {
                return Err(ControlsError::ButtonOutOfRange);
            }
            ControlInput::MouseButton(button) if !(1..=3).contains(&button) => {
                return Err(ControlsError::ButtonOutOfRange);
            }
            _ => {}
        }
        let mut cleared = Vec::new();
        for row in &mut self.rows {
            if row.command_id != command_id && slot_value(row, slot) == Some(input) {
                set_slot(row, slot, None);
                cleared.push(row.command_id);
            }
        }
        let row = self
            .rows
            .iter_mut()
            .find(|row| row.command_id == command_id)
            .expect("the command was found above");
        set_slot(row, slot, Some(input));
        Ok(cleared)
    }

    /// Clears one slot of one command.
    ///
    /// # Errors
    ///
    /// [`ControlsError::UnknownCommand`] for an unknown id.
    pub fn clear(&mut self, command_id: u16, slot: ControlSlot) -> Result<(), ControlsError> {
        let row = self
            .rows
            .iter_mut()
            .find(|row| row.command_id == command_id)
            .ok_or(ControlsError::UnknownCommand { command_id })?;
        set_slot(row, slot, None);
        Ok(())
    }

    /// Whether any bound key code on `scancode` carries a modifier bit.
    pub fn has_modifier_variants(&self, scancode: u8) -> bool {
        self.key_codes().any(|code| {
            code.scancode() == u16::from(scancode)
                && code.code() & (ORIGINAL_KEY_ALT | ORIGINAL_KEY_CTRL | ORIGINAL_KEY_SHIFT) != 0
        })
    }

    /// The commands a key press runs under the held modifiers.
    ///
    /// The exact code (scancode plus the held modifier bits) wins. Without an
    /// exact match, a scancode that has *any* modifier combination bound runs
    /// nothing (the modifiers must match exactly); a scancode with no
    /// combination bound ignores the held modifiers and runs its plain
    /// binding.
    pub fn commands_for_key(&self, scancode: u8, held: Modifiers) -> Vec<u16> {
        let exact = u16::from(scancode) | held.bits();
        let matching = |code: u16| -> Vec<u16> {
            self.rows
                .iter()
                .filter(|row| row.key_a.code() == code || row.key_b.code() == code)
                .map(|row| row.command_id)
                .collect()
        };
        let found = matching(exact);
        if !found.is_empty() || self.has_modifier_variants(scancode) {
            return found;
        }
        matching(u16::from(scancode))
    }

    /// Every bound key code, key A then key B per row.
    fn key_codes(&self) -> impl Iterator<Item = OriginalKeyCode> + '_ {
        self.rows
            .iter()
            .flat_map(|row| [row.key_a, row.key_b])
            .filter(|code| !code.is_none())
    }

    /// The keyboard/joystick [`ActionMap`] this table gives the live game.
    ///
    /// * A key row whose command has a project action
    ///   ([`original_target`]) and whose code is a plain scancode with no
    ///   modifier combination bound anywhere becomes a [`BindingSource::Key`]
    ///   binding, for keys the project's [`Key`] vocabulary names. A key with
    ///   modifier variants is **not** in the map — a [`BindingSource`] is one
    ///   physical input — and is delivered by the raw-keyboard minter through
    ///   [`commands_for_key`](Self::commands_for_key).
    /// * A joystick button `n` becomes `JoystickButton(n - 1)`.
    /// * Axis keys deflect by `key_axis_scale` (designed: the original's
    ///   keyboard rate is unmeasured).
    /// * The joystick axes are the original's assignment,
    ///   [`original_joystick_axis_bindings`].
    /// * The designed gamepad bindings and the menu navigation keys are
    ///   carried over from [`ActionMap::designed_default`] unchanged: the
    ///   original's analog calibration and menu keys are not measured. The
    ///   original's default has **no mouse binding** and the map therefore has
    ///   none.
    ///
    /// # Panics
    ///
    /// Never for the default table; a rebinding that made two same-context
    /// commands share one source panics here, which [`assign`](Self::assign)
    /// rules out by slot uniqueness.
    pub fn to_action_map(&self, key_axis_scale: f32) -> ActionMap {
        let designed = ActionMap::designed_default();
        let mut bindings: Vec<Binding> = Vec::new();
        for row in &self.rows {
            let Some(target) = original_target(row.command_id, key_axis_scale) else {
                continue;
            };
            for code in [row.key_a, row.key_b] {
                if code.is_none()
                    || code.code() & (ORIGINAL_KEY_ALT | ORIGINAL_KEY_CTRL | ORIGINAL_KEY_SHIFT)
                        != 0
                    || self.has_modifier_variants(code.scancode() as u8)
                {
                    continue;
                }
                if let Some(key) = key_from_scancode(code.scancode()) {
                    bindings.push(Binding {
                        source: BindingSource::Key(key),
                        target,
                    });
                }
            }
            if row.joystick_button != 0 && !matches!(target, BindingTarget::Axis { .. }) {
                bindings.push(Binding {
                    source: BindingSource::JoystickButton(u16::from(row.joystick_button) - 1),
                    target,
                });
            }
            let mouse = match row.mouse_button {
                1 => Some(MouseButton::Left),
                2 => Some(MouseButton::Right),
                3 => Some(MouseButton::Middle),
                _ => None,
            };
            if let Some(button) = mouse
                && !matches!(target, BindingTarget::Axis { .. })
            {
                bindings.push(Binding {
                    source: BindingSource::MouseButton(button),
                    target,
                });
            }
        }
        bindings.extend(original_joystick_axis_bindings());
        for binding in designed.bindings() {
            let carried = match binding.source {
                BindingSource::GamepadAxis(_) => true,
                BindingSource::GamepadButton(_) => true,
                BindingSource::Key(key) => {
                    matches!(binding.target, BindingTarget::Ui(action) if action != UiAction::Pause)
                        && key != Key::Escape
                }
                _ => false,
            };
            if carried {
                bindings.push(*binding);
            }
        }
        ActionMap::try_new(bindings).expect("the controls table binds no source twice")
    }

    /// The help screen's text: one line per category, listing every command
    /// with the inputs it is bound to now. Command names are this project's
    /// own wording, never the original's strings.
    pub fn help_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for category in OriginalControlCategory::ALL {
            let mut entries: Vec<String> = Vec::new();
            for row in self.rows.iter().filter(|row| row.category == *category) {
                let mut inputs: Vec<String> = [row.key_a, row.key_b]
                    .iter()
                    .filter(|code| !code.is_none())
                    .map(|code| key_code_name(*code))
                    .collect();
                if row.joystick_button != 0 {
                    inputs.push(format!("Joy{}", row.joystick_button));
                }
                if row.mouse_button != 0 {
                    inputs.push(format!("Mouse{}", row.mouse_button));
                }
                if inputs.is_empty() {
                    inputs.push("unbound".to_owned());
                }
                entries.push(format!(
                    "{} {}",
                    command_name(row.command_id),
                    inputs.join("/")
                ));
            }
            lines.push(format!("{}: {}", category.label(), entries.join("  ")));
        }
        lines
    }
}

fn slot_value(row: &ControlRow, slot: ControlSlot) -> Option<ControlInput> {
    match slot {
        ControlSlot::KeyA => (!row.key_a.is_none()).then_some(ControlInput::Key(row.key_a)),
        ControlSlot::KeyB => (!row.key_b.is_none()).then_some(ControlInput::Key(row.key_b)),
        ControlSlot::JoystickButton => {
            (row.joystick_button != 0).then_some(ControlInput::JoystickButton(row.joystick_button))
        }
        ControlSlot::MouseButton => {
            (row.mouse_button != 0).then_some(ControlInput::MouseButton(row.mouse_button))
        }
    }
}

fn set_slot(row: &mut ControlRow, slot: ControlSlot, value: Option<ControlInput>) {
    match (slot, value) {
        (ControlSlot::KeyA, Some(ControlInput::Key(code))) => row.key_a = code,
        (ControlSlot::KeyA, None) => row.key_a = OriginalKeyCode::NONE,
        (ControlSlot::KeyB, Some(ControlInput::Key(code))) => row.key_b = code,
        (ControlSlot::KeyB, None) => row.key_b = OriginalKeyCode::NONE,
        (ControlSlot::JoystickButton, Some(ControlInput::JoystickButton(n))) => {
            row.joystick_button = n;
        }
        (ControlSlot::JoystickButton, None) => row.joystick_button = 0,
        (ControlSlot::MouseButton, Some(ControlInput::MouseButton(n))) => row.mouse_button = n,
        (ControlSlot::MouseButton, None) => row.mouse_button = 0,
        _ => {}
    }
}

/// The project key a DirectInput scancode names, or `None` when the project's
/// [`Key`] vocabulary has none.
pub fn key_from_scancode(scancode: u16) -> Option<Key> {
    Key::ALL
        .iter()
        .copied()
        .find(|key| original_scancode(*key) == Some(scancode))
}

/// The joystick axis bindings of the live game: the original's assignment of
/// device axes to flight axes ([`ORIGINAL_JOYSTICK_AXES`]), in this project's
/// axis numbering and sign convention (SIDEWINDER-FFB2, #1968).
///
/// Which device axis drives which flight axis, and that the throttle is full
/// with the lever at its minimum (`0.5 - 0.5 * normalised`), are the
/// original's (static evidence). The signs are this project's: the HID axes
/// read positive with the stick right, pulled back and twisted clockwise, and
/// the flight axes read positive for roll right, nose up and nose right, so
/// the stick drives each one unscaled. The original's own `inverted` flags
/// describe its internal roll and yaw signs, which differ from this project's,
/// and are therefore not copied. The hat drives the look directions, not an
/// axis, and has no binding here.
#[must_use]
pub fn original_joystick_axis_bindings() -> Vec<Binding> {
    ORIGINAL_JOYSTICK_AXES
        .iter()
        .filter_map(|original| {
            let command = original.flight_axis?;
            let (index, inverted) = match original.axis {
                OriginalJoystickAxis::X => (JOYSTICK_AXIS_X, false),
                OriginalJoystickAxis::Y => (JOYSTICK_AXIS_Y, false),
                OriginalJoystickAxis::Z => (JOYSTICK_AXIS_THROTTLE, true),
                OriginalJoystickAxis::Rz => (JOYSTICK_AXIS_TWIST, false),
                OriginalJoystickAxis::Hat => return None,
            };
            Some(Binding {
                source: BindingSource::JoystickAxis { index, inverted },
                target: BindingTarget::Axis {
                    command,
                    scale: 1.0,
                },
            })
        })
        .collect()
}

/// The project action an original command id runs, or `None` when the
/// project has no consumer for it yet. `key_axis_scale` is the keyboard
/// deflection of the axis commands.
pub fn original_target(command_id: u16, key_axis_scale: f32) -> Option<BindingTarget> {
    use BindingTarget::{Axis, Command, Ui};
    use FlightCommand as C;
    // Pitch: command 1 points the nose down (negative pitch), command 2 up;
    // roll 3 left / 4 right; rudder 5 left / 6 right.
    let axis = |command, sign: f32| Axis {
        command,
        scale: sign * key_axis_scale,
    };
    Some(match command_id {
        1 => axis(C::Pitch, -1.0),
        2 => axis(C::Pitch, 1.0),
        3 => axis(C::Roll, -1.0),
        4 => axis(C::Roll, 1.0),
        5 => axis(C::Yaw, -1.0),
        6 => axis(C::Yaw, 1.0),
        7 => Command(C::ThrottleStepUp),
        8 => Command(C::ThrottleStepDown),
        9 => Command(C::ThrottleIdle),
        10 => Command(C::ThrottleLevel1),
        11 => Command(C::ThrottleLevel2),
        12 => Command(C::ThrottleLevel3),
        13 => Command(C::ThrottleLevel4),
        14 => Command(C::ThrottleLevel5),
        15 => Command(C::ThrottleLevel6),
        16 => Command(C::ThrottleLevel7),
        17 => Command(C::ThrottleFull),
        19 => Command(C::FirePrimary),
        20 => Command(C::FireSecondary),
        34 => Command(C::Eject),
        36 => Command(C::TargetNextEnemy),
        37 => Command(C::TargetPrevEnemy),
        38 => Command(C::TargetNearestEnemy),
        39 => Command(C::TargetNextAlly),
        40 => Command(C::TargetPrevAlly),
        41 => Command(C::TargetNearestAlly),
        42 => Command(C::TargetNextGround),
        43 => Command(C::TargetPrevGround),
        44 => Command(C::TargetNearestGround),
        45 => Command(C::TargetUnderReticule),
        46 => Command(C::TargetNothing),
        47 => Command(C::LevelOff),
        69 => Ui(UiAction::Pause),
        _ => return None,
    })
}

/// The flight/UI action an original command id delivers as an edge, for the
/// raw-keyboard minter.
pub fn original_edge_action(command_id: u16) -> Option<Action> {
    match original_target(command_id, 1.0)? {
        BindingTarget::Command(command) => Some(Action::Flight(command)),
        BindingTarget::Ui(action) => Some(Action::Ui(action)),
        BindingTarget::Axis { .. } => None,
    }
}

/// This project's own short name for an original command id (never the
/// original's strings).
pub fn command_name(command_id: u16) -> &'static str {
    match command_id {
        1 => "Nose down",
        2 => "Nose up",
        3 => "Roll left",
        4 => "Roll right",
        5 => "Rudder left",
        6 => "Rudder right",
        47 => "Level off",
        7 => "Throttle up",
        8 => "Throttle down",
        9 => "Throttle 0",
        10 => "Throttle 1/8",
        11 => "Throttle 2/8",
        12 => "Throttle 3/8",
        13 => "Throttle 4/8",
        14 => "Throttle 5/8",
        15 => "Throttle 6/8",
        16 => "Throttle 7/8",
        17 => "Throttle full",
        19 => "Fire guns",
        20 => "Fire rockets",
        25 => "Guns cycle cw",
        26 => "Guns cycle ccw",
        32 => "Rockets cycle cw",
        33 => "Rockets cycle ccw",
        36 => "Next enemy",
        37 => "Previous enemy",
        38 => "Nearest enemy",
        39 => "Next ally",
        40 => "Previous ally",
        41 => "Nearest ally",
        42 => "Next ground target",
        43 => "Previous ground target",
        44 => "Nearest ground target",
        45 => "Target at crosshairs",
        46 => "Target nothing",
        48 => "Spyglass",
        49 => "Cockpit views",
        50 => "Camera down",
        51 => "Camera back",
        52 => "Camera left",
        53 => "Camera right",
        54 => "Chase view",
        55 => "Snap look",
        57 => "Smooth look",
        56 => "Track target",
        58 => "Look down-left",
        59 => "Look down",
        60 => "Look down-right",
        61 => "Look left",
        62 => "Look middle",
        63 => "Look right",
        64 => "Look up-left",
        65 => "Look up",
        66 => "Look up-right",
        67 => "Camera zoom in",
        68 => "Camera zoom out",
        106 => "Auto-dock",
        18 => "Nitro",
        34 => "Bail out",
        69 => "Pause",
        35 => "Scores",
        109 => "Chat all",
        110 => "Chat team",
        111 => "Help",
        _ => "Command",
    }
}

/// A key code spelled for a help screen: modifiers, then the key's name.
pub fn key_code_name(code: OriginalKeyCode) -> String {
    let mut name = String::new();
    if code.has_ctrl() {
        name.push_str("Ctrl+");
    }
    if code.has_alt() {
        name.push_str("Alt+");
    }
    if code.has_shift() {
        name.push_str("Shift+");
    }
    name.push_str(&scancode_name(code.scancode() as u8));
    name
}

/// The name of a DirectInput scancode for a help screen.
pub fn scancode_name(scancode: u8) -> String {
    const LETTERS: [(u8, char); 26] = [
        (0x1e, 'A'),
        (0x30, 'B'),
        (0x2e, 'C'),
        (0x20, 'D'),
        (0x12, 'E'),
        (0x21, 'F'),
        (0x22, 'G'),
        (0x23, 'H'),
        (0x17, 'I'),
        (0x24, 'J'),
        (0x25, 'K'),
        (0x26, 'L'),
        (0x32, 'M'),
        (0x31, 'N'),
        (0x18, 'O'),
        (0x19, 'P'),
        (0x10, 'Q'),
        (0x13, 'R'),
        (0x1f, 'S'),
        (0x14, 'T'),
        (0x16, 'U'),
        (0x2f, 'V'),
        (0x11, 'W'),
        (0x2d, 'X'),
        (0x15, 'Y'),
        (0x2c, 'Z'),
    ];
    if let Some((_, letter)) = LETTERS.iter().find(|(code, _)| *code == scancode) {
        return letter.to_string();
    }
    match scancode {
        0x01 => "Esc".to_owned(),
        0x02..=0x0a => format!("{}", scancode - 1),
        0x0b => "0".to_owned(),
        0x0c => "-".to_owned(),
        0x0d => "=".to_owned(),
        0x0e => "Backspace".to_owned(),
        0x0f => "Tab".to_owned(),
        0x1c => "Enter".to_owned(),
        0x29 => "`".to_owned(),
        0x33 => ",".to_owned(),
        0x34 => ".".to_owned(),
        0x39 => "Space".to_owned(),
        0x3b..=0x44 => format!("F{}", scancode - 0x3a),
        0x57 => "F11".to_owned(),
        0x58 => "F12".to_owned(),
        0x47 => "Num7".to_owned(),
        0x48 => "Num8".to_owned(),
        0x49 => "Num9".to_owned(),
        0x4a => "Num-".to_owned(),
        0x4b => "Num4".to_owned(),
        0x4c => "Num5".to_owned(),
        0x4d => "Num6".to_owned(),
        0x4e => "Num+".to_owned(),
        0x4f => "Num1".to_owned(),
        0x50 => "Num2".to_owned(),
        0x51 => "Num3".to_owned(),
        0x52 => "Num0".to_owned(),
        0x53 => "Num.".to_owned(),
        0xc8 => "Up".to_owned(),
        0xd0 => "Down".to_owned(),
        0xcb => "Left".to_owned(),
        0xcd => "Right".to_owned(),
        other => format!("key {other:#04x}"),
    }
}

#[cfg(test)]
mod tests {
    //! Acceptance for Rally #1733 `PARITY2-ORIGINAL-KEYBINDINGS`: the pinned
    //! original table (ids and packed codes, static evidence, never
    //! `verified_original`) against the production default map, the modifier
    //! rule and slot uniqueness.

    use super::{
        ControlInput, ControlSlot, ControlsError, ControlsMap, Modifiers, original_edge_action,
    };
    use crate::input::{
        Action, BindingSource, BindingTarget, FlightCommand, Key, MouseButton, OriginalKeyCode,
        UiAction,
    };

    /// `(command id, key A, key B, joystick button)` for all 64 commands.
    const PINNED: [(u16, u16, u16, u8); 64] = [
        (1, 0x0c8, 0, 0),
        (2, 0x0d0, 0, 0),
        (3, 0x0cb, 0, 0),
        (4, 0x0cd, 0, 0),
        (5, 0x033, 0x052, 0),
        (6, 0x034, 0x053, 0),
        (47, 0x426, 0, 0),
        (7, 0x00d, 0, 0),
        (8, 0x00c, 0, 0),
        (9, 0x002, 0, 0),
        (10, 0x003, 0, 0),
        (11, 0x004, 0, 0),
        (12, 0x005, 0, 0),
        (13, 0x006, 0, 0),
        (14, 0x007, 0, 0),
        (15, 0x008, 0, 0),
        (16, 0x009, 0, 0),
        (17, 0x00a, 0, 0),
        (19, 0x039, 0, 1),
        (20, 0x02d, 0, 2),
        (25, 0x03d, 0, 0),
        (26, 0x03e, 0, 7),
        (32, 0x03f, 0, 0),
        (33, 0x040, 0, 8),
        (36, 0x012, 0, 3),
        (37, 0x412, 0, 0),
        (38, 0x212, 0, 0),
        (39, 0x011, 0, 0),
        (40, 0x411, 0, 0),
        (41, 0x211, 0, 0),
        (42, 0x013, 0, 6),
        (43, 0x413, 0, 0),
        (44, 0x213, 0, 0),
        (45, 0x010, 0, 0),
        (46, 0x014, 0, 0),
        (48, 0x41f, 0, 5),
        (49, 0x042, 0, 4),
        (50, 0x043, 0, 0),
        (51, 0x044, 0, 0),
        (52, 0x057, 0, 0),
        (53, 0x058, 0, 0),
        (54, 0x041, 0, 0),
        (55, 0x025, 0, 0),
        (57, 0x024, 0, 0),
        (56, 0x026, 0, 0),
        (58, 0x04f, 0, 0),
        (59, 0x050, 0, 0),
        (60, 0x051, 0, 0),
        (61, 0x04b, 0, 0),
        (62, 0x04c, 0, 0),
        (63, 0x04d, 0, 0),
        (64, 0x047, 0, 0),
        (65, 0x048, 0, 0),
        (66, 0x049, 0, 0),
        (68, 0x04a, 0, 0),
        (67, 0x04e, 0, 0),
        (106, 0x01e, 0, 0),
        (18, 0x031, 0, 0),
        (34, 0x22d, 0, 0),
        (69, 0x001, 0, 0),
        (35, 0x00f, 0, 0),
        (109, 0x029, 0, 0),
        (110, 0x429, 0, 0),
        (111, 0x03b, 0, 0),
    ];

    #[test]
    fn accept_parity2_original_keybindings_default_map_is_the_pinned_table() {
        let map = ControlsMap::original_default();
        assert_eq!(map.rows().len(), 64);
        for (row, (id, key_a, key_b, joy)) in map.rows().iter().zip(PINNED) {
            assert_eq!(row.command_id, id);
            assert_eq!(row.key_a.code(), key_a, "command {id} key A");
            assert_eq!(row.key_b.code(), key_b, "command {id} key B");
            assert_eq!(row.joystick_button, joy, "command {id} joystick");
            assert_eq!(row.mouse_button, 0, "command {id}: no default mouse button");
        }
    }

    #[test]
    fn accept_parity2_original_keybindings_action_map_carries_the_original_keys() {
        let map = ControlsMap::original_default().to_action_map(1.0);
        let target = |key: Key| {
            map.targets_for(BindingSource::Key(key))
                .find(|target| !matches!(target, BindingTarget::Ui(UiAction::NavigateUp)))
        };
        let command = |key: Key| match target(key) {
            Some(BindingTarget::Command(command)) => Some(command),
            _ => None,
        };
        let axis = |key: Key| match target(key) {
            Some(BindingTarget::Axis { command, scale }) => Some((command, scale)),
            _ => None,
        };
        assert_eq!(axis(Key::ArrowUp), Some((FlightCommand::Pitch, -1.0)));
        assert_eq!(axis(Key::ArrowDown), Some((FlightCommand::Pitch, 1.0)));
        assert_eq!(axis(Key::ArrowLeft), Some((FlightCommand::Roll, -1.0)));
        assert_eq!(axis(Key::ArrowRight), Some((FlightCommand::Roll, 1.0)));
        assert_eq!(axis(Key::Comma), Some((FlightCommand::Yaw, -1.0)));
        assert_eq!(axis(Key::Period), Some((FlightCommand::Yaw, 1.0)));
        assert_eq!(axis(Key::Numpad0), Some((FlightCommand::Yaw, -1.0)));
        assert_eq!(axis(Key::NumpadDecimal), Some((FlightCommand::Yaw, 1.0)));
        assert_eq!(command(Key::Equal), Some(FlightCommand::ThrottleStepUp));
        assert_eq!(command(Key::Minus), Some(FlightCommand::ThrottleStepDown));
        assert_eq!(command(Key::Digit1), Some(FlightCommand::ThrottleIdle));
        assert_eq!(command(Key::Digit5), Some(FlightCommand::ThrottleLevel4));
        assert_eq!(command(Key::Digit9), Some(FlightCommand::ThrottleFull));
        assert_eq!(command(Key::Space), Some(FlightCommand::FirePrimary));
        assert_eq!(command(Key::Q), Some(FlightCommand::TargetUnderReticule));
        assert_eq!(command(Key::T), Some(FlightCommand::TargetNothing));
        assert_eq!(
            map.target_for(BindingSource::Key(Key::Escape)),
            Some(BindingTarget::Ui(UiAction::Pause))
        );
        assert_eq!(
            map.target_for(BindingSource::JoystickButton(0)),
            Some(BindingTarget::Command(FlightCommand::FirePrimary))
        );
        // Keys with modifier variants are delivered by the chord minter, not the
        // map: E/W/R/X/L/S, and none of the old developer keys survive.
        for key in [Key::E, Key::W, Key::R, Key::X, Key::L, Key::S] {
            assert_eq!(
                map.targets_for(BindingSource::Key(key)).count(),
                0,
                "{key:?}"
            );
        }
        for key in [Key::A, Key::D, Key::F, Key::C, Key::Z, Key::G, Key::B] {
            assert!(
                !matches!(target(key), Some(BindingTarget::Axis { .. })),
                "{key:?} must not carry a developer flight axis"
            );
        }
        assert!(
            map.targets_for(BindingSource::MouseButton(MouseButton::Left))
                .next()
                .is_none(),
            "the original binds no default mouse button"
        );
    }

    #[test]
    fn accept_parity2_original_keybindings_modifier_exactness() {
        let map = ControlsMap::original_default();
        let shift = Modifiers {
            shift: true,
            ..Modifiers::NONE
        };
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        let both = Modifiers {
            shift: true,
            ctrl: true,
            alt: false,
        };
        // E: plain next, Shift previous, Ctrl nearest, Ctrl+Shift nothing.
        assert_eq!(map.commands_for_key(0x12, Modifiers::NONE), vec![36]);
        assert_eq!(map.commands_for_key(0x12, shift), vec![37]);
        assert_eq!(map.commands_for_key(0x12, ctrl), vec![38]);
        assert!(map.commands_for_key(0x12, both).is_empty());
        // Q has no modifier combination bound: held modifiers are ignored.
        assert_eq!(map.commands_for_key(0x10, Modifiers::NONE), vec![45]);
        assert_eq!(map.commands_for_key(0x10, shift), vec![45]);
        assert_eq!(map.commands_for_key(0x10, both), vec![45]);
        // X: plain rockets, Ctrl bail out; Shift+X nothing.
        assert_eq!(map.commands_for_key(0x2d, Modifiers::NONE), vec![20]);
        assert_eq!(map.commands_for_key(0x2d, ctrl), vec![34]);
        assert!(map.commands_for_key(0x2d, shift).is_empty());
        // L: plain track-target, Shift level-off; Ctrl+L nothing.
        assert_eq!(map.commands_for_key(0x26, shift), vec![47]);
        assert_eq!(map.commands_for_key(0x26, Modifiers::NONE), vec![56]);
        assert!(map.commands_for_key(0x26, ctrl).is_empty());
        // S: only Shift+S (spyglass) is bound; plain S runs nothing.
        assert_eq!(map.commands_for_key(0x1f, shift), vec![48]);
        assert!(map.commands_for_key(0x1f, Modifiers::NONE).is_empty());
        assert_eq!(
            original_edge_action(47),
            Some(Action::Flight(FlightCommand::LevelOff))
        );
    }

    #[test]
    fn accept_parity2_original_keybindings_slot_uniqueness() {
        let mut map = ControlsMap::original_default();
        // Moving key `E` (0x012) to Fire Guns key A clears it from Next Enemy
        // (key A) and leaves Fire Guns' old Space free.
        let cleared = map
            .assign(
                19,
                ControlSlot::KeyA,
                ControlInput::Key(OriginalKeyCode::new(0x012)),
            )
            .expect("a known command and a key");
        assert_eq!(cleared, vec![36]);
        assert!(map.row(36).expect("row").key_a.is_none());
        assert_eq!(map.row(19).expect("row").key_a.code(), 0x012);
        // The same code in the other key slot is a different slot: nothing else
        // loses it.
        let cleared = map
            .assign(
                20,
                ControlSlot::KeyB,
                ControlInput::Key(OriginalKeyCode::new(0x012)),
            )
            .expect("assigns");
        assert!(cleared.is_empty());
        // Joystick button 1 moves from Fire Guns to Fire Rockets.
        let cleared = map
            .assign(
                20,
                ControlSlot::JoystickButton,
                ControlInput::JoystickButton(1),
            )
            .expect("assigns");
        assert_eq!(cleared, vec![19]);
        assert_eq!(map.row(19).expect("row").joystick_button, 0);
        assert_eq!(map.row(20).expect("row").joystick_button, 1);
        // Mouse button 1 is free by default; a second assignment takes it over.
        map.assign(19, ControlSlot::MouseButton, ControlInput::MouseButton(1))
            .expect("assigns");
        let cleared = map
            .assign(20, ControlSlot::MouseButton, ControlInput::MouseButton(1))
            .expect("assigns");
        assert_eq!(cleared, vec![19]);
        // Refusals change nothing.
        let before = map.clone();
        assert_eq!(
            map.assign(
                999,
                ControlSlot::KeyA,
                ControlInput::Key(OriginalKeyCode::NONE)
            ),
            Err(ControlsError::UnknownCommand { command_id: 999 })
        );
        assert_eq!(
            map.assign(19, ControlSlot::KeyA, ControlInput::JoystickButton(2)),
            Err(ControlsError::WrongSlot)
        );
        assert_eq!(
            map.assign(
                19,
                ControlSlot::JoystickButton,
                ControlInput::JoystickButton(11)
            ),
            Err(ControlsError::ButtonOutOfRange)
        );
        assert_eq!(map, before);
    }

    #[test]
    fn accept_parity2_original_keybindings_help_lists_the_real_bindings() {
        let mut map = ControlsMap::original_default();
        let text = map.help_lines().join("\n");
        assert!(text.contains("Fire guns Space/Joy1"), "{text}");
        assert!(text.contains("Previous enemy Shift+E"), "{text}");
        assert!(text.contains("Bail out Ctrl+X"), "{text}");
        assert!(text.contains("Nose down Up"), "{text}");
        map.assign(
            19,
            ControlSlot::KeyA,
            ControlInput::Key(OriginalKeyCode::new(0x02d)),
        )
        .expect("rebinding works");
        let text = map.help_lines().join("\n");
        assert!(text.contains("Fire guns X/Joy1"), "{text}");
        assert!(text.contains("Fire rockets Joy2"), "{text}");
    }
}
