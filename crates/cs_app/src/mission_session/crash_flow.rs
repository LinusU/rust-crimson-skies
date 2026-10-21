//! What follows a fatal crash in a composed mission (PARITY4-CRASH-C, #1855).
//!
//! [`crate::playtest::fatal_contact::judge_terrain_contacts`] ends the flight
//! and starts a [`CrashSequence`]. From then on:
//!
//! * **The camera** is the detached crash camera, held at one point above the
//!   impact (`CRASH_CAMERA_*`, `OWNER-STATIC`) and looking at the wreck;
//!   `external_view::drive_external_camera` places it, the cockpit views stand
//!   down and the view keys are inert.
//! * **The HUD** is held back with the launch cinematic's rule (the text layer,
//!   the gauge cluster and the target markers). That the original's
//!   `0x455800(0)` hides it is inferred from the code shape (finding §2).
//! * **`Fatal Crash!`** is the installation's own `MSG_CRASH` string (id 162),
//!   drawn centred in the objective marker's blue ([`MissionCrashMessagePlugin`]).
//!   The original's position, font, colour and the unit of its `5.0f` argument
//!   are unmeasured, so the layout is **designed** and the message stays up for
//!   the whole sequence.
//! * **Any key** ([`step_crash_sequence`]) releases the run: the mission host's
//!   `settle` waits for it, then settles the failure with the debrief fields
//!   read at the crash tick (the clock stopped at the crash, finding §"What
//!   else the removal path does"). `Press Any Key` (id 163) has no located
//!   consumer, so no prompt is drawn.
//! * **The placeholder target** `(1, 20, 0)` is carried on the sequence as the
//!   owner's observation; its drawing is untraced.

use bevy::input::{ButtonInput, InputSystems};
use bevy::prelude::{
    App, BackgroundColor, Color, Commands, Component, IntoScheduleConfigs, KeyCode, Node, Plugin,
    PositionType, PreUpdate, Query, Res, ResMut, Startup, Text, TextColor, TextFont, Update, Val,
    Visibility, With, default,
};
use bevy::text::FontSize;

use super::cockpit_hud::{OBJECTIVE_BLUE, message_text};
use super::view_cycle::MissionViewMode;
use crate::playtest::fatal_contact::{CrashSequence, FATAL_CRASH_MESSAGE};
use crate::ui::hud::MissionMessages;

/// Holds the chase view while a crash sequence runs and releases the run on
/// the first key pressed after the crash.
pub fn step_crash_sequence(
    crash: Option<ResMut<CrashSequence>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    view: Option<ResMut<MissionViewMode>>,
) {
    let Some(mut crash) = crash else { return };
    if let Some(mut view) = view {
        *view = MissionViewMode::Chase;
    }
    if !crash.released && keys.is_some_and(|keys| keys.get_just_pressed().next().is_some()) {
        crash.released = true;
    }
}

/// Installs the headless half: the view hold and the release key. The step
/// reads the keys right after the input system, ahead of every consumer that
/// clears a pressed key it has taken (`mint_original_chord_keys`), so any key
/// releases the run, bound or not.
pub fn install_crash_flow(app: &mut App) {
    app.add_systems(
        PreUpdate,
        step_crash_sequence
            .after(InputSystems)
            .before(crate::input::platform::pump_platform_input),
    );
}

/// The text node of `Fatal Crash!`.
#[derive(Component)]
struct CrashMessage;

/// Draws `Fatal Crash!` for the length of a crash sequence (windowed only).
#[derive(Clone, Copy, Debug, Default)]
pub struct MissionCrashMessagePlugin;

impl Plugin for MissionCrashMessagePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_crash_message)
            .add_systems(Update, update_crash_message);
    }
}

fn spawn_crash_message(mut commands: Commands) {
    let (r, g, b) = OBJECTIVE_BLUE;
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(22.0),
            ..default()
        },
        TextColor(Color::srgb(r, g, b)),
        BackgroundColor(Color::NONE),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            top: Val::Percent(30.0),
            justify_content: bevy::prelude::JustifyContent::Center,
            ..default()
        },
        Visibility::Hidden,
        CrashMessage,
    ));
}

fn update_crash_message(
    crash: Option<Res<CrashSequence>>,
    messages: Option<Res<MissionMessages>>,
    mut lines: Query<(&mut Text, &mut Visibility), With<CrashMessage>>,
) {
    for (mut text, mut visibility) in &mut lines {
        if crash.is_none() {
            *visibility = Visibility::Hidden;
            continue;
        }
        let catalog = messages.as_deref().and_then(MissionMessages::catalog);
        let line = message_text(FATAL_CRASH_MESSAGE, catalog);
        if text.0 != line {
            text.0 = line;
        }
        *visibility = Visibility::Inherited;
    }
}
