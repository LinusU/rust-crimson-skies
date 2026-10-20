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
//! * [`MissionControlsHelpPlugin`] draws the table's real bindings on the
//!   `F1` help screen and the `Esc` pause screen.
//!
//! The old developer layout (`W/S` pitch, `Left Shift`/`F` throttle, ...)
//! survives only behind `--dev-keys` ([`super::targeting_input::mission_action_map`]).

use bevy::prelude::{
    App, BackgroundColor, ButtonInput, Color, Commands, Component, IntoScheduleConfigs, KeyCode,
    Node, Plugin, PositionType, Query, Res, ResMut, Resource, Startup, Text, TextColor, TextFont,
    UiRect, Update, Val, Visibility, With, World, default,
};
use bevy::text::FontSize;

use cs_types::input::{Action, ActionMap};
use cs_types::keymap::{ControlsMap, Modifiers, original_edge_action};

use crate::input::platform::{PlatformInput, scancode_from_bevy};
use crate::playtest::command::KEY_DEFLECTION;

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

/// Mints the frame's modifier-sensitive key presses into the cockpit queue.
///
/// Every just-pressed key whose scancode has a modifier combination bound is
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
    let Some(keys) = world.get_resource::<ButtonInput<KeyCode>>() else {
        return;
    };
    let held = held_modifiers(keys);
    let mut minted: Vec<Action> = Vec::new();
    let mut consumed: Vec<KeyCode> = Vec::new();
    for code in keys.get_just_pressed() {
        let Some(scancode) = scancode_from_bevy(*code) else {
            continue;
        };
        if !controls.0.has_modifier_variants(scancode) {
            continue;
        }
        consumed.push(*code);
        if paused {
            continue;
        }
        minted.extend(
            controls
                .0
                .commands_for_key(scancode, held)
                .into_iter()
                .filter_map(original_edge_action)
                .filter(|action| matches!(action, Action::Flight(_))),
        );
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

/// Marks the help/pause panel.
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
    let hint = if paused {
        "Esc resumes"
    } else {
        "F1 closes"
    };
    let mut text = format!("{header} - controls as bound ({hint})\n");
    text.push_str(&controls.help_lines().join("\n"));
    text
}

fn spawn_controls_panel(mut commands: Commands) {
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(13.0),
            ..default()
        },
        TextColor(Color::WHITE),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(6.0),
            right: Val::Px(6.0),
            width: Val::Percent(46.0),
            padding: UiRect::all(Val::Px(6.0)),
            ..default()
        },
        Visibility::Hidden,
        ControlsHelpPanel,
    ));
}

fn update_controls_panel(
    controls: Res<MissionControls>,
    open: Res<ControlsHelpOpen>,
    platform: Option<Res<PlatformInput>>,
    mut panels: Query<(&mut Text, &mut Visibility), With<ControlsHelpPanel>>,
) {
    let paused = platform.is_some_and(|platform| platform.session().is_paused());
    let shown = paused || open.0;
    for (mut text, mut visibility) in &mut panels {
        let wanted = if shown {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
        if shown {
            let body = controls_screen_text(&controls.0, paused, open.0);
            if text.0 != body {
                text.0 = body;
            }
        }
    }
}

/// Adds the help/pause screen. Windowed compositions only.
#[derive(Clone, Copy, Debug, Default)]
pub struct MissionControlsHelpPlugin;

impl Plugin for MissionControlsHelpPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ControlsHelpOpen>()
            .add_systems(Startup, spawn_controls_panel)
            .add_systems(
                Update,
                (toggle_controls_help, update_controls_panel).chain(),
            );
    }
}
