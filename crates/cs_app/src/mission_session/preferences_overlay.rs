//! The Preferences screen inside the mission window (PARITY3-PAUSE-MAP-SCREEN
//! .02-E.01, #1922).
//!
//! `front_end::Screen::Settings` cannot be drawn by the deck (`NoFullPane`,
//! `docs/findings/2026-10-15-f45-b-1-original-layout.md`); only the standalone
//! shell presents the `[@Preferences@]` section of `ASSETS/LAYOUT.CSV`, on its
//! own camera. This module draws the same section over the mission window as
//! Bevy UI nodes, through the same [`MenuShell`] the shell uses — so the rows
//! it draws, their authored positions, the strip frames and each button's
//! stated effect are the shell's, not a second reading.
//!
//! * [`read_preferences_overlay`] builds the section and decodes its art from
//!   the retail installation.
//! * [`PreferencesOverlayPlugin`] draws the section on an
//!   [`OpenPreferencesOverlay`] message, aspect-fit to the window like the
//!   pause map (800x600 logical), and removes it on [`ClosePreferencesOverlay`]
//!   or when a button's own effect leaves the section.
//! * [`PreferencesOverlay`] records what was drawn and which button was
//!   pressed with which stated effect.
//!
//! The page is the **in-game** variant. `PREFERENCES.SCRIPT` sets the
//! in-game flag `$$KW$$` when the Esc screen opens it: the one bottom slot is
//! then `PF_B_RETURNTOGAME` instead of `PF_B_MAINMENU`, and the section's
//! logo and movie rows are created. [`read_preferences_overlay`] checks the
//! script text for exactly those statements and refuses a script that reads
//! otherwise; `PF_B_MAINMENU` is then neither drawn nor hit.
//!
//! A press has the effect the layout or the section's screen script states:
//!
//! * **Return to Game** (`PF_B_RETURNTOGAME`): the arm is `callback($$A$$, 24)`
//!   under `$$KW$$`. Native `A 24` is the statically read "resume from the
//!   in-game Preferences" (`docs/findings/2026-10-22-parity5-start-flow-
//!   transitions.md`, provenance `static`). The overlay closes and records
//!   the press; resuming the session is the caller's, the overlay never
//!   touches the clock.
//! * Game Options / Audio / Video / Controls start layout sections the
//!   overlay does not present: recorded as [`PreferencesEffect::Navigate`],
//!   the overlay stays on its page.
//! * Anything else is recorded unhandled with the shell's reason.
//!
//! What stays unknown: the section's backdrop is a movie row (`M`) that is
//! not read, so nothing is drawn behind the panes
//! (`docs/findings/2026-10-23-preferences-overlay.md`).

use std::collections::BTreeMap;
use std::fmt;

use bevy::asset::RenderAssetUsages;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::math::Rect as UiRect;
use bevy::prelude::{
    App, ButtonInput, Commands, Component, Entity, GlobalZIndex, Handle, Image, ImageNode, Message,
    MessageReader, MessageWriter, MouseButton, Node, Plugin, PositionType, Query, Res, ResMut,
    Resource, Update, Val, Window, With, default,
};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::ui::front_end::{
    Artwork, FrontEndScreens, MenuShell, ShellOutcome, ShellView, SkippedRow, UnhandledEffect,
    script_member,
};

/// The `LAYOUT.CSV` section this overlay presents.
pub const PREFERENCES_SECTION: &str = "Preferences";

/// The slot's two buttons: the out-of-game one the overlay never shows and the
/// in-game one it does.
const OUT_OF_GAME_BUTTON: &str = "PF_B_MAINMENU";
const IN_GAME_BUTTON: &str = "PF_B_RETURNTOGAME";
/// The statement the in-game arm runs.
const RETURN_STATEMENT: &str = "callback($$A$$, 24)";

/// Above the pause map's cursor layer (`pause_map` uses 3000..=3020).
const Z_PANE: i32 = 3100;
const Z_BUTTON: i32 = 3200;

// --------------------------------------------------------------- source --

/// Why the section could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreferencesOverlayError(pub String);

impl fmt::Display for PreferencesOverlayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the preferences overlay cannot be read: {}", self.0)
    }
}

impl std::error::Error for PreferencesOverlayError {}

/// The section and its decoded art, read once from the installation.
#[derive(Resource)]
pub struct PreferencesOverlaySource {
    shell: MenuShell,
    artwork: BTreeMap<String, Artwork>,
}

/// Whether the screen script reads, line by line, the way the in-game
/// variant needs: `$$KW$$` picks `pf_b_returntogame` (else `pf_b_mainmenu`)
/// for the slot, and the slot's mailbox arm runs `callback($$A$$, 24)` under
/// `$$KW$$`.
fn script_states_in_game_variant(script: &str) -> bool {
    let lines: Vec<&str> = script
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let has = |wanted: &[&str]| {
        lines.windows(wanted.len()).any(|window| {
            window
                .iter()
                .zip(wanted)
                .all(|(a, b)| a.eq_ignore_ascii_case(b))
        })
    };
    has(&[
        "if ($$KW$$)",
        "{",
        "MPA.YC = \"pf_b_returntogame\"",
        "}",
        "else",
        "{",
        "MPA.YC = \"pf_b_mainmenu\"",
        "}",
    ]) && has(&["case MPA:", "if ($$KW$$)", "{", RETURN_STATEMENT, "}"])
}

impl PreferencesOverlaySource {
    /// The shell holding every section of the layout (the overlay opens
    /// [`PREFERENCES_SECTION`] on it).
    #[must_use]
    pub fn shell(&self) -> &MenuShell {
        &self.shell
    }
}

/// Reads the retail `Preferences` section through the shell's own loaders.
///
/// # Errors
///
/// [`PreferencesOverlayError`] when the layout cannot be read or holds no
/// `Preferences` section.
pub fn read_preferences_overlay(
    screens: &FrontEndScreens,
) -> Result<PreferencesOverlaySource, PreferencesOverlayError> {
    let script = screens
        .script_text(&script_member(PREFERENCES_SECTION))
        .ok_or_else(|| PreferencesOverlayError("PREFERENCES.SCRIPT is not readable".to_owned()))?;
    if !script_states_in_game_variant(&script) {
        return Err(PreferencesOverlayError(
            "PREFERENCES.SCRIPT does not state the in-game Return to Game arm".to_owned(),
        ));
    }
    let layout = screens
        .original_layout()
        .map_err(|error| PreferencesOverlayError(error.to_string()))?;
    let mut shell = MenuShell::build(
        &layout,
        &|art: &str| screens.art_extent(art),
        &|section: &str| screens.script_text(&script_member(section)),
    );
    shell
        .open(PREFERENCES_SECTION)
        .map_err(|error| PreferencesOverlayError(error.to_string()))?;
    let section = shell
        .section()
        .ok_or_else(|| PreferencesOverlayError("the section did not open".to_owned()))?;
    let mut artwork = BTreeMap::new();
    for art in section
        .panes
        .iter()
        .map(|pane| &pane.art)
        .chain(section.buttons.iter().map(|button| &button.art))
    {
        if let Some(decoded) = screens.graphic_artwork(art) {
            artwork.entry(art.to_ascii_lowercase()).or_insert(decoded);
        }
    }
    Ok(PreferencesOverlaySource { shell, artwork })
}

// --------------------------------------------------------------- record --

/// Opens the overlay (restarting the section).
#[derive(Message, Clone, Copy, Debug, Default)]
pub struct OpenPreferencesOverlay;

/// Closes the overlay.
#[derive(Message, Clone, Copy, Debug, Default)]
pub struct ClosePreferencesOverlay;

/// One row that was drawn, at its authored logical position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrawnRow {
    /// The row id as the layout writes it (upper-cased).
    pub id: String,
    /// `P` (pane) or `B` (button).
    pub kind: char,
    /// The art name as the row writes it.
    pub art: String,
    /// The visible region in the 800x600 logical space.
    pub logical: cs_content::ui_layout::Rect,
}

/// What a press did, as the data states it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreferencesEffect {
    /// `PF_B_RETURNTOGAME`: the arm's `callback($$A$$, 24)`; the overlay
    /// closed. Resuming the session is the caller's.
    ReturnToGame,
    /// The button starts a layout section the overlay does not present; the
    /// overlay stays on its page.
    Navigate(String),
    /// A `terminate` handler: not the overlay's to run.
    Terminate,
    /// No effect the data states.
    Unhandled(UnhandledEffect),
}

/// One button press and what the data states it does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreferencesPress {
    /// The button's row id.
    pub id: String,
    /// What the press did.
    pub effect: PreferencesEffect,
}

/// What the overlay is showing and what it has seen, public so a test or the
/// pause map can read it.
#[derive(Resource, Default, Debug)]
pub struct PreferencesOverlay {
    /// Whether the section is on screen.
    pub open: bool,
    /// The rows drawn when it last opened, panes then buttons.
    pub drawn: Vec<DrawnRow>,
    /// The `P`/`B` rows the section could not draw, with the shell's reason.
    pub skipped: Vec<SkippedRow>,
    /// The other row kinds the section holds besides `P`/`B`/`M` (`T`, `E`,
    /// …), counted and never drawn; the `M` movie backdrop is parsed into
    /// the section's `movies` and is not among them.
    pub other_rows: Vec<String>,
    /// Every press in order.
    pub presses: Vec<PreferencesPress>,
}

/// Everything the overlay spawns.
#[derive(Component)]
pub struct PreferencesOverlayPart;

/// A spawned row: which pane or button of the section it draws.
#[derive(Component, Clone, Copy)]
struct Row {
    button: bool,
    index: usize,
}

fn to_image(art: &Artwork) -> Image {
    Image::new(
        Extent3d {
            width: art.width(),
            height: art.height(),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        art.rgba().to_vec(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

fn surface_of(window: &Window) -> (u32, u32) {
    (window.width() as u32, window.height() as u32)
}

fn crop(offset: (u32, u32), logical: cs_content::ui_layout::Rect) -> UiRect {
    UiRect::new(
        offset.0 as f32,
        offset.1 as f32,
        (offset.0 + logical.width) as f32,
        (offset.1 + logical.height) as f32,
    )
}

fn place(node: &mut Node, rect: cs_content::ui_layout::Rect) {
    node.left = Val::Px(rect.x as f32);
    node.top = Val::Px(rect.y as f32);
    node.width = Val::Px(rect.width as f32);
    node.height = Val::Px(rect.height as f32);
}

// -------------------------------------------------------------- systems --

fn spawn_rows(
    commands: &mut Commands,
    source: &PreferencesOverlaySource,
    view: &ShellView,
    images: &mut bevy::prelude::Assets<Image>,
    record: &mut PreferencesOverlay,
) {
    let handles: BTreeMap<&String, Handle<Image>> = source
        .artwork
        .iter()
        .map(|(name, art)| (name, images.add(to_image(art))))
        .collect();
    record.drawn.clear();
    for pane in &view.panes {
        let Some(handle) = handles.get(&pane.art.to_ascii_lowercase()) else {
            continue;
        };
        let mut image = ImageNode::new(handle.clone());
        image.rect = Some(crop(pane.texel_offset, pane.logical));
        let mut node = Node {
            position_type: PositionType::Absolute,
            ..default()
        };
        place(&mut node, pane.rect);
        commands.spawn((
            node,
            image,
            GlobalZIndex(Z_PANE + pane.index as i32),
            Row {
                button: false,
                index: pane.index,
            },
            PreferencesOverlayPart,
        ));
        record.drawn.push(DrawnRow {
            id: pane.id.clone(),
            kind: 'P',
            art: pane.art.clone(),
            logical: pane.logical,
        });
    }
    for button in view.buttons.iter().filter(|b| shown(&b.id)) {
        let Some(handle) = handles.get(&button.art.to_ascii_lowercase()) else {
            continue;
        };
        let mut image = ImageNode::new(handle.clone());
        image.rect = Some(crop(button.texel_offset, button.logical));
        let mut node = Node {
            position_type: PositionType::Absolute,
            ..default()
        };
        place(&mut node, button.rect);
        commands.spawn((
            node,
            image,
            GlobalZIndex(Z_BUTTON + button.index as i32),
            Row {
                button: true,
                index: button.index,
            },
            PreferencesOverlayPart,
        ));
        record.drawn.push(DrawnRow {
            id: button.id.clone(),
            kind: 'B',
            art: button.art.clone(),
            logical: button.logical,
        });
    }
}

/// Whether a button belongs to the in-game page.
fn shown(id: &str) -> bool {
    !id.eq_ignore_ascii_case(OUT_OF_GAME_BUTTON)
}

fn despawn_parts(commands: &mut Commands, parts: &Query<Entity, With<PreferencesOverlayPart>>) {
    for part in parts {
        commands.entity(part).despawn();
    }
}

/// Opens and closes the overlay on its messages.
#[allow(clippy::too_many_arguments)]
fn sync_overlay(
    mut commands: Commands,
    mut source: ResMut<PreferencesOverlaySource>,
    mut record: ResMut<PreferencesOverlay>,
    mut images: ResMut<bevy::prelude::Assets<Image>>,
    mut opens: MessageReader<OpenPreferencesOverlay>,
    mut closes: MessageReader<ClosePreferencesOverlay>,
    windows: Query<&Window>,
    parts: Query<Entity, With<PreferencesOverlayPart>>,
) {
    let open = opens.read().count() > 0;
    let close = closes.read().count() > 0;
    if close && record.open {
        despawn_parts(&mut commands, &parts);
        record.open = false;
    }
    if !open || record.open {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    if source.shell.open(PREFERENCES_SECTION).is_err() {
        return;
    }
    // The out-of-game slot is not part of the in-game page: no draw, no hit.
    let _ = source.shell.set_enabled(OUT_OF_GAME_BUTTON, false);
    let Some(view) = source.shell.view(surface_of(window)) else {
        return;
    };
    if let Some(section) = source.shell.section() {
        record.skipped = section.skipped.clone();
        record.other_rows = section.other_rows.clone();
    }
    spawn_rows(&mut commands, &source, &view, &mut images, &mut record);
    record.open = true;
}

/// Pointer rollover/press through the shell, then re-lays every row out for
/// the window's current size and strip state.
#[allow(clippy::too_many_arguments)]
fn drive_overlay(
    mut commands: Commands,
    mut source: ResMut<PreferencesOverlaySource>,
    mut record: ResMut<PreferencesOverlay>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    parts: Query<Entity, With<PreferencesOverlayPart>>,
    mut rows: Query<(&Row, &mut Node, &mut ImageNode)>,
    mut presses: MessageWriter<PreferencesPressed>,
) {
    if !record.open {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let surface = surface_of(window);
    let logical = window
        .cursor_position()
        .and_then(|at| MenuShell::unmap_point(surface, at.x as u32, at.y as u32));
    match logical {
        Some((x, y)) => source.shell.pointer_move(x, y),
        None => source.shell.pointer_leave(),
    }
    if mouse.just_pressed(MouseButton::Left) {
        source.shell.pointer_down();
    }
    if mouse.just_released(MouseButton::Left) {
        let held = source
            .shell
            .hover()
            .and_then(|index| source.shell.section()?.buttons.get(index))
            .map(|button| button.id.clone())
            .unwrap_or_default();
        let outcome = source.shell.pointer_up();
        if let Some(press) = press_of(&mut source.shell, held, outcome) {
            let leaves = press.effect == PreferencesEffect::ReturnToGame;
            presses.write(PreferencesPressed(press.clone()));
            record.presses.push(press);
            if leaves {
                despawn_parts(&mut commands, &parts);
                record.open = false;
                return;
            }
        }
    }
    let Some(view) = source.shell.view(surface) else {
        return;
    };
    for (row, mut node, mut image) in &mut rows {
        if row.button {
            if let Some(button) = view.buttons.iter().find(|b| b.index == row.index) {
                place(&mut node, button.rect);
                image.rect = Some(crop(button.texel_offset, button.logical));
            }
        } else if let Some(pane) = view.panes.iter().find(|p| p.index == row.index) {
            place(&mut node, pane.rect);
            image.rect = Some(crop(pane.texel_offset, pane.logical));
        }
    }
}

/// A button was released over itself: what it did.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct PreferencesPressed(pub PreferencesPress);

/// Turns the shell's outcome into the overlay's record of the press.
///
/// The shell navigates on a `Navigate`; the overlay presents no other
/// section, so it steps the shell back to its own page.
fn press_of(
    shell: &mut MenuShell,
    held: String,
    outcome: ShellOutcome,
) -> Option<PreferencesPress> {
    match outcome {
        ShellOutcome::Nothing | ShellOutcome::Back { .. } => None,
        ShellOutcome::Entered { section } => {
            shell.escape();
            Some(PreferencesPress {
                id: held,
                effect: PreferencesEffect::Navigate(section),
            })
        }
        ShellOutcome::Unhandled { id, effect } => {
            let effect = if id.eq_ignore_ascii_case(IN_GAME_BUTTON) {
                PreferencesEffect::ReturnToGame
            } else {
                PreferencesEffect::Unhandled(effect)
            };
            Some(PreferencesPress { id, effect })
        }
        ShellOutcome::Quit => Some(PreferencesPress {
            id: held,
            effect: PreferencesEffect::Terminate,
        }),
    }
}

/// Installs the overlay over a mission composition.
pub fn install_preferences_overlay(app: &mut App, source: PreferencesOverlaySource) {
    app.insert_resource(source);
    app.add_plugins(PreferencesOverlayPlugin);
}

/// The window's face of the Preferences section.
#[derive(Clone, Copy, Debug, Default)]
pub struct PreferencesOverlayPlugin;

impl Plugin for PreferencesOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PreferencesOverlay>()
            .add_message::<OpenPreferencesOverlay>()
            .add_message::<ClosePreferencesOverlay>()
            .add_message::<PreferencesPressed>()
            .add_systems(Update, (sync_overlay, drive_overlay).chain());
    }
}
