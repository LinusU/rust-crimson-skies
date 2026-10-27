//! The mission's player-facing keyboard layout: the original game's default
//! bindings (PARITY2-ORIGINAL-KEYBINDINGS, #1733).
//!
//! [`MissionControls`] holds the rebindable [`ControlsMap`] — the original's
//! 64-command table (`OWNER-STATIC-2026-10-05`, static evidence, never
//! `verified_original`). Three seams read it:
//!
//! * [`original_mission_action_map`] derives the input session's
//!   [`ActionMap`](cs_types::input::ActionMap): the keys whose plain scancode
//!   has no modifier combination bound (arrows, `,` `.`, `=` `-`, the digits,
//!   `Space`, `Q`, `T`, `Esc`).
//! * [`mint_original_chord_keys`] delivers the keys that *do* have modifier
//!   combinations (`E`/`W`/`R` targeting, `X` rockets and `Ctrl+X` bail out,
//!   `Shift+L` level-off, ...) from the raw keyboard state, by the original's
//!   exact-modifier rule ([`ControlsMap::commands_for_key`]).
//! * The same minter reads the host joystick (SIDEWINDER-FFB2, #1968): a
//!   joystick button whose row is a view or look-mode command selects it as
//!   the key would (the flight commands of the joystick column are in the
//!   action map), and the hat is the look direction — the original's joystick
//!   mode reads the hat first and falls back to the numpad look keys when it is
//!   centred.
//! * [`MissionControlsHelpPlugin`] draws the table's real bindings on the
//!   `F1` keymap overlay ([`super::keymap_overlay`]); the `Esc` pause
//!   screen is the chart of [`super::pause_map`].
//!
//! The old developer layout (`W/S` pitch, `Left Shift`/`F` throttle, ...)
//! survives only behind `--dev-keys` ([`super::targeting_input::mission_action_map`]).

use bevy::prelude::{
    App, ButtonInput, Component, IntoScheduleConfigs, KeyCode, Plugin, Res, ResMut, Resource,
    Startup, Update, World,
};

use cs_types::input::{Action, ActionMap};
use cs_types::keymap::{ControlsMap, Modifiers, original_edge_action};

use crate::input::hid::HostJoysticks;
use crate::input::hid::layout::HatDirection;
use crate::input::platform::{PlatformInput, scancode_from_bevy};
use crate::playtest::command::KEY_DEFLECTION;

use super::look_view::{
    LOOK_MIDDLE_COMMAND_ID, MissionLook, SMOOTH_LOOK_COMMAND_ID, SNAP_LOOK_COMMAND_ID,
    TRACK_TARGET_COMMAND_ID, look_direction,
};
use super::view_cycle::{
    CycleMemory, MissionViewMode, MissionZoom, VIEW_CYCLE_COMMAND_ID, ZOOM_IN_COMMAND_ID,
    ZOOM_OUT_COMMAND_ID, select_view,
};
use super::weapons::MissionCockpitActions;

/// The command id of the original's "View Help" row.
pub const HELP_COMMAND_ID: u16 = 111;

/// The mission's live control table. Its presence selects the original
/// layout; a run with `--dev-keys` never inserts it.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct MissionControls(pub ControlsMap);

/// The input session's map for the original layout. Axis keys deflect by the
/// playtest's disclosed [`KEY_DEFLECTION`] (the original's keyboard rate is
/// unmeasured).
#[must_use]
pub fn original_mission_action_map(controls: &ControlsMap) -> ActionMap {
    controls.to_action_map(KEY_DEFLECTION)
}

/// The Alt/Ctrl/Shift state of a keyboard frame.
#[must_use]
pub fn held_modifiers(keys: &ButtonInput<KeyCode>) -> Modifiers {
    Modifiers {
        alt: keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight),
        ctrl: keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight),
        shift: keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight),
    }
}

/// The original's `Toggle Spyglass` command id (`MSG_CAM2_TOG`).
const SPYGLASS_COMMAND_ID: u16 = 48;

/// The keys held at the end of the last [`mint_original_chord_keys`] run, so a
/// press is an edge against the last frame the minter saw rather than against
/// Bevy's per-frame `just_pressed` flag (which a press injected before the
/// input systems run never carries).
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct ChordKeyMemory(pub std::collections::BTreeSet<KeyCode>);

/// The joystick buttons held at the end of the last [`mint_original_chord_keys`]
/// run (bit `n` is button `n + 1`), so a button press is an edge.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JoystickButtonMemory(pub u32);

/// The look direction of a hat, in [`look_direction`]'s terms: `x` `+1` left,
/// `y` `+1` up.
#[must_use]
pub const fn hat_look(hat: HatDirection) -> (i8, i8) {
    match hat {
        HatDirection::Up => (0, 1),
        HatDirection::UpRight => (-1, 1),
        HatDirection::Right => (-1, 0),
        HatDirection::DownRight => (-1, -1),
        HatDirection::Down => (0, -1),
        HatDirection::DownLeft => (1, -1),
        HatDirection::Left => (1, 0),
        HatDirection::UpLeft => (1, 1),
    }
}

/// The original command ids bound to the 1-based joystick `button`.
fn joystick_commands(controls: &ControlsMap, button: u8) -> Vec<u16> {
    controls
        .rows()
        .iter()
        .filter(|row| row.joystick_button == button)
        .map(|row| row.command_id)
        .collect()
}

/// Sorts one pressed command into the view and look-mode presses.
fn classify_view_press(
    command: u16,
    view_presses: &mut Vec<Option<MissionViewMode>>,
    look_presses: &mut Vec<u16>,
) {
    if command == VIEW_CYCLE_COMMAND_ID {
        view_presses.push(None);
    } else if let Some(target) = MissionViewMode::for_command(command) {
        view_presses.push(Some(target));
    } else if matches!(
        command,
        SNAP_LOOK_COMMAND_ID | SMOOTH_LOOK_COMMAND_ID | TRACK_TARGET_COMMAND_ID
    ) {
        look_presses.push(command);
    }
}

/// Mints the frame's modifier-sensitive key presses into the cockpit queue.
///
/// Every newly pressed key whose scancode has a modifier combination bound is
/// **consumed** (its `just_pressed` edge is cleared so no raw-key reader runs
/// it a second time) and resolved with the original's exact-modifier rule;
/// each command with a project action joins [`MissionCockpitActions`]. A
/// paused session mints nothing: the pause gate of the input session applies
/// to these keys exactly as to the mapped ones.
pub fn mint_original_chord_keys(world: &mut World) {
    let Some(controls) = world.get_resource::<MissionControls>().cloned() else {
        return;
    };
    let paused = world
        .get_resource::<PlatformInput>()
        .is_some_and(|platform| platform.session().is_paused());
    // After a fatal crash the camera is held at its crash point: the view and
    // look keys are inert (the original's guard `V+0x91d`, finding
    // `2026-10-22-crash-a-fatal-contact-and-crash-camera.md`).
    let crashed = world.contains_resource::<crate::playtest::fatal_contact::CrashSequence>();
    let Some(keys) = world.get_resource::<ButtonInput<KeyCode>>() else {
        return;
    };
    let held = held_modifiers(keys);
    let now: std::collections::BTreeSet<KeyCode> = keys.get_pressed().copied().collect();
    let before = world
        .get_resource::<ChordKeyMemory>()
        .map(|memory| memory.0.clone())
        .unwrap_or_default();
    let mut minted: Vec<Action> = Vec::new();
    let mut consumed: Vec<KeyCode> = Vec::new();
    // The view keys pressed this frame, in order: `Cockpit views` is `None`.
    let mut view_presses: Vec<Option<MissionViewMode>> = Vec::new();
    // The look-mode keys (`K`, `J`, `L`) pressed this frame, in order.
    let mut look_presses: Vec<u16> = Vec::new();
    for code in now.difference(&before) {
        let Some(scancode) = scancode_from_bevy(*code) else {
            continue;
        };
        let commands = controls.0.commands_for_key(scancode, held);
        // `Cockpit views` (F8) has no modifier combination, so it is read
        // here, ahead of the modifier filter, and left standing.
        if !paused && !crashed {
            for command in &commands {
                classify_view_press(*command, &mut view_presses, &mut look_presses);
            }
        }
        if !controls.0.has_modifier_variants(scancode) {
            continue;
        }
        // The spyglass reads its own key (`Shift+S` with the exact-modifier
        // rule) from the raw input, so its edge is left standing.
        if !commands.contains(&SPYGLASS_COMMAND_ID) {
            consumed.push(*code);
        }
        if paused {
            continue;
        }
        minted.extend(
            commands
                .into_iter()
                .filter_map(original_edge_action)
                .filter(|action| matches!(action, Action::Flight(_))),
        );
    }
    // The zoom keys are polled while held, not edge-triggered.
    let (mut zoom_in, mut zoom_out) = (false, false);
    // The look directions are polled the same way.
    let (mut look_held, mut centre_held) = ((0i8, 0i8), false);
    if !paused {
        for code in &now {
            let Some(scancode) = scancode_from_bevy(*code) else {
                continue;
            };
            let commands = controls.0.commands_for_key(scancode, held);
            zoom_in |= commands.contains(&ZOOM_IN_COMMAND_ID);
            zoom_out |= commands.contains(&ZOOM_OUT_COMMAND_ID);
            for command in &commands {
                if let Some((x, y)) = look_direction(*command) {
                    look_held = (look_held.0 + x, look_held.1 + y);
                    centre_held |= *command == LOOK_MIDDLE_COMMAND_ID;
                }
            }
        }
    }
    world.insert_resource(ChordKeyMemory(now));
    // The host joystick: new button presses of view and look-mode rows, and
    // the hat as the look direction ahead of the numpad.
    let (joy_buttons, hat) = world
        .get_resource::<HostJoysticks>()
        .filter(|joysticks| joysticks.connected().is_some())
        .map_or((0, None), |joysticks| {
            (joysticks.state().buttons, joysticks.hat())
        });
    let joy_before = world
        .get_resource::<JoystickButtonMemory>()
        .map_or(0, |memory| memory.0);
    world.insert_resource(JoystickButtonMemory(joy_buttons));
    if !paused && !crashed {
        let mut spyglass = false;
        for bit in 0..10u8 {
            if joy_buttons & !joy_before & (1 << bit) != 0 {
                for command in joystick_commands(&controls.0, bit + 1) {
                    classify_view_press(command, &mut view_presses, &mut look_presses);
                    spyglass ^= command == SPYGLASS_COMMAND_ID;
                }
            }
        }
        // The spyglass reads its own key from the raw keyboard; its joystick
        // button toggles it here.
        if spyglass
            && let Some(mut state) = world.get_resource_mut::<super::spyglass::SpyglassState>()
        {
            state.enabled = !state.enabled;
        }
        if let Some(hat) = hat {
            look_held = hat_look(hat);
            centre_held = false;
        }
    }
    {
        let mut zoom = world.get_resource_or_insert_with(MissionZoom::default);
        zoom.in_held = zoom_in;
        zoom.out_held = zoom_out;
    }
    {
        let mut look = world.get_resource_or_insert_with(MissionLook::default);
        look.held = look_held;
        look.centre_held = centre_held;
        for command in look_presses {
            look.press(command);
        }
    }
    if !view_presses.is_empty() {
        let mut look = world.remove_resource::<MissionLook>().unwrap_or_default();
        let mut view = world
            .remove_resource::<MissionViewMode>()
            .unwrap_or_default();
        let mut memory = world.remove_resource::<CycleMemory>().unwrap_or_default();
        let mut zoom = world.remove_resource::<MissionZoom>().unwrap_or_default();
        for press in view_presses {
            let target = press.unwrap_or_else(|| view.next_from(memory.0));
            select_view(&mut view, &mut memory, &mut zoom, &mut look, target);
        }
        world.insert_resource(view);
        world.insert_resource(memory);
        world.insert_resource(zoom);
        world.insert_resource(look);
    }
    if !minted.is_empty() {
        world
            .get_resource_or_insert_with(MissionCockpitActions::default)
            .actions
            .extend(minted);
    }
    if let Some(mut keys) = world.get_resource_mut::<ButtonInput<KeyCode>>() {
        for code in consumed {
            keys.clear_just_pressed(code);
        }
    }
}

/// Whether the `F1` help screen is open.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ControlsHelpOpen(pub bool);

/// Marks the help panels (and the pause map, which the smoke probes treat alike).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ControlsHelpPanel;

/// Toggles the help screen on the table's own "View Help" key.
pub fn toggle_controls_help(
    keys: Res<ButtonInput<KeyCode>>,
    controls: Res<MissionControls>,
    mut open: ResMut<ControlsHelpOpen>,
) {
    let held = held_modifiers(&keys);
    for code in keys.get_just_pressed() {
        if let Some(scancode) = scancode_from_bevy(*code)
            && controls
                .0
                .commands_for_key(scancode, held)
                .contains(&HELP_COMMAND_ID)
        {
            open.0 = !open.0;
        }
    }
}

/// The panel's text for the current state: the real bindings under a header
/// naming the screen.
#[must_use]
pub fn controls_screen_text(controls: &ControlsMap, paused: bool, help_open: bool) -> String {
    let header = if help_open { "HELP" } else { "PAUSED" };
    let hint = if paused { "Esc resumes" } else { "F1 closes" };
    let mut text = format!("{header} - controls as bound ({hint})\n");
    text.push_str(&controls.help_lines().join("\n"));
    text
}

/// Adds the F1 help screen (the Esc pause screen is `super::pause_map`).
/// Windowed compositions only.
#[derive(Clone, Copy, Debug, Default)]
pub struct MissionControlsHelpPlugin;

impl Plugin for MissionControlsHelpPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ControlsHelpOpen>()
            .add_systems(Startup, super::keymap_overlay::spawn_keymap_overlay)
            .add_systems(
                Update,
                (
                    toggle_controls_help,
                    super::keymap_overlay::update_keymap_overlay,
                )
                    .chain(),
            );
    }
}
