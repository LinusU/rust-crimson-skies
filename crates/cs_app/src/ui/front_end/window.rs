//! The windowed front-end presenter: `cs --cs-path <retail> --front-end`
//! (FE-MENU-1, task #1833).
//!
//! Its app systems are `fn(&mut World)` exclusive systems on purpose: every
//! `Res<T>`/`Query<..>`/message param on a scheduled system is monomorphized
//! into `cs`, whose `__eh_frame` sits ~1 KiB under the 16 MiB compact-unwind
//! ceiling with this presenter included (task #336's gate,
//! `accept_t336_linked_images_*`). Exclusive systems cost one fn item each
//! instead of a param-machinery instantiation per system.
//!
//! Spec context: `specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`;
//! shared contract `docs/contracts/UI-NETWORK.md` (aspect-fit + shared
//! transform) and `docs/contracts/CLI-EVIDENCE.md` (a scripted smoke run is
//! never `human_play`, and a uniform framebuffer is a failure).
//!
//! # What runs
//!
//! The window shows one [`MenuShell`] section at a time on its own logical
//! 800x600 surface, aspect-fit into the window with black bars. Every `P` row
//! is a sprite of its art's frame 0 at its authored `X,Y`; every `B` row is a
//! sprite of its strip's current state frame — `Disabled`, `Active`,
//! `Rollover` (pointer hover *or* keyboard focus) or `Depressed` — picked out
//! of the same [`Sprite::rect`] every frame, so a state change needs no
//! respawn. The cursor art the shell found is drawn at the pointer and the
//! OS cursor is hidden; when no cursor art decoded the record says so
//! ([`ShellRun::cursor`]) and the OS cursor is left visible.
//!
//! Input is the section's own semantics, not the designed table's: the
//! layout's focus order is unread, so `Up`/`Down` walk the declaration
//! order; `Enter` presses the focused button; `Esc` drops a held press, steps
//! one section back, or quits on the root; the pointer hovers and presses.
//! What a press *does* is the row's own stated effect ([`ButtonEffect`]):
//! the `ScriptToExe` section, `terminate`, the `activate(@…@)`+`script_end`
//! hand-over a screen script states, or a recorded unhandled press — never
//! a guessed transition.
//!
//! # The smoke run
//!
//! `--smoke-seconds <n> --capture-dir <dir>` drives the same windowed app
//! with a deterministic script (injected `ButtonInput`/`CursorMoved`, exactly
//! [`SMOKE_FRAME_HZ`] frames a simulated second): settle on `MainMenu`,
//! hover the Campaign button, press it with the pointer, land on `Campaign`,
//! `Esc` back, `Down`-walk focus to Quit, `Enter` it. The run records
//! framebuffer PNGs, a trace and `report.json` into the capture directory,
//! and **fails** (exit nonzero) when a shot is empty/uniform, the navigation
//! did not happen, the depressed frame never drew, or the quit never
//! arrived.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bevy::app::AppExit;
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::ecs::message::{MessageCursor, Messages};
use bevy::image::{Image, ImageSampler};
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::prelude::{
    App, Camera, Camera2d, ClearColorConfig, Color, DefaultPlugins, Entity, On, Plugin,
    PluginGroup, Resource, Sprite, Startup, Transform, Update, Vec2, Visibility, Window,
    WindowPlugin, World, default,
};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::time::TimeUpdateStrategy;
use bevy::window::{CursorMoved, CursorOptions, PrimaryWindow};

use super::original::script_member;
use super::retail::{FrontEndScreens, RetailError};
use super::shell::{
    ButtonVisual, MenuShell, SECTION_SURFACE, START_SECTION, ShellOutcome, ShellView,
};
use super::{Artwork, ShellError};
use crate::playtest::SmokeRequest;
use crate::playtest::smoke::SMOKE_FRAME_HZ;

/// The scripted pass needs this many simulated seconds: the last scripted
/// input lands at 6.0 s and the run wants settling frames after it.
pub const MIN_SHELL_SMOKE_SECONDS: u32 = 8;

/// The default capture directory (ignored by Git).
pub const DEFAULT_FRONT_END_CAPTURE_DIR: &str = "private/front-end";

/// A screenshot whose frame holds fewer distinct luminance levels than this
/// is an empty framebuffer.
const MIN_DISTINCT_LUMINANCE: usize = 8;

/// Frames to wait for outstanding screenshots after the script ends.
const SCREENSHOT_GRACE_FRAMES: u32 = 600;

/// What `cs --cs-path <retail> --front-end` was asked to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrontEndRequest {
    /// The original installation root.
    pub cs_path: PathBuf,
    /// `Some` runs the finite deterministic smoke instead of the open-ended
    /// interactive session.
    pub smoke: Option<SmokeRequest>,
}

/// Why the front-end run could not proceed or failed its own checks.
#[derive(Debug)]
pub enum FrontEndError {
    /// The installation could not be read.
    Retail(RetailError),
    /// The shell's entry section is absent from the layout.
    Shell(ShellError),
    /// The app exited with an error, or the smoke ended before it finished.
    Exit(String),
    /// The smoke run failed its checks; each entry names one.
    SmokeFailed(Vec<String>),
}

impl fmt::Display for FrontEndError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Retail(error) => write!(f, "the front end cannot read the installation: {error}"),
            Self::Shell(error) => write!(f, "the front end cannot open: {error}"),
            Self::Exit(detail) => write!(f, "the front-end run ended: {detail}"),
            Self::SmokeFailed(failures) => {
                write!(f, "the front-end smoke failed:")?;
                for failure in failures {
                    write!(f, "\n  - {failure}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for FrontEndError {}

// ----------------------------------------------------------------- model ---

/// The live presenter state: the [`MenuShell`] the run drives plus the
/// surface, input and observation record the systems mutate. Public so a
/// headless drive (the `accept_fe_menu_1_` tests) can read the model the
/// real input path produced; the windowed run treats it as its own.
#[derive(Resource)]
pub struct ShellResource {
    /// The shell model.
    pub shell: MenuShell,
    /// The window's logical size this frame — the surface the view fits
    /// into; `(800, 600)` when there is no window (a headless drive).
    surface: (u32, u32),
    /// The frame's collected input edges plus the pointer position.
    input: ShellInput,
    /// What the run observed, for the smoke's checks and the record.
    run: ShellRun,
    /// The decoded art: lower-cased art name → the whole image (a button's
    /// 4-frame strip among them); the cursor is kept apart so its absence is
    /// a visible record, not a missing map entry.
    art: ShellArt,
    /// The entities the presenter spawned.
    scene: ShellScene,
    /// The section's sprites are stale: respawn them.
    redraw: bool,
}

/// The decoded art.
#[derive(Default)]
struct ShellArt {
    /// Strip/pane textures by lower-cased layout art name.
    images: BTreeMap<String, Handle<Image>>,
    /// The cursor texture found, and its native extent.
    cursor: Option<(Handle<Image>, (u32, u32))>,
}

/// One edge-triggered input for the shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellEvent {
    /// The pointer's left button went down.
    PointerDown,
    /// The pointer's left button came up.
    PointerUp,
    /// `ArrowDown`: focus the next enabled button.
    FocusNext,
    /// `ArrowUp`: focus the previous enabled button.
    FocusPrev,
    /// `Enter` went down on the focused button.
    ActivateDown,
    /// `Enter` came up.
    ActivateUp,
    /// `Esc` went down.
    Escape,
}

/// The input the frame applies: the latest pointer position plus the edges,
/// and the `CursorMoved` reader that survives between frames.
struct ShellInput {
    /// Latest window-space pointer position (logical px, top-left origin).
    cursor: Option<Vec2>,
    /// The input edges this frame, in order.
    events: Vec<ShellEvent>,
    /// The persistent `CursorMoved` reader.
    cursor_events: MessageCursor<CursorMoved>,
}

/// What the run observed, for the smoke's checks and the record.
#[derive(Default)]
struct ShellRun {
    /// Sections visited in order, starting with the root.
    visited: Vec<String>,
    /// A `Quit` outcome arrived.
    quit_seen: bool,
    /// Focus moved this many times.
    focus_moves: u32,
    /// Buttons the pointer hovered (row ids).
    hovered: BTreeSet<String>,
    /// A button drew its depressed frame.
    depressed_seen: bool,
    /// Presses with no known effect.
    unhandled: usize,
    /// The cursor art found (`name`) or the recorded unknown.
    cursor: Option<String>,
}

/// The live scene: every entity the presenter spawned, so `sync_shell` can
/// touch them with `World::get_mut` — each `query_filtered::<D, F>`
/// instantiation is a `QueryState` + fetch monomorphization `cs`'s
/// `__eh_frame` budget cannot afford.
#[derive(Default)]
struct ShellScene {
    /// The primary window, when one exists.
    window: Option<Entity>,
    /// The section's pane sprites.
    panes: Vec<DrawnSprite>,
    /// The section's button sprites.
    buttons: Vec<DrawnSprite>,
    /// The cursor sprite, once spawned.
    cursor: Option<Entity>,
}

/// One sprite the presenter owns: the view index it answers to, its draw
/// layer and its entity.
struct DrawnSprite {
    index: usize,
    z: f32,
    entity: Entity,
}

// --------------------------------------------------------------- plugin ---

/// The presenter as a Bevy plugin: model + input + (optionally) sprites.
///
/// The windowed run uses [`Self::windowed`]; a headless drive uses
/// [`Self::headless`], which skips sprite spawning so the input/model path
/// runs under `MinimalPlugins` with no renderer.
pub struct FrontEndShellPlugin {
    shell: Mutex<Option<MenuShell>>,
    art: BTreeMap<String, Artwork>,
    cursor: Option<(String, Artwork)>,
    visuals: bool,
}

impl FrontEndShellPlugin {
    /// The windowed presenter: sprites draw from `art` (lower-cased art name
    /// → decoded strip/pane) and `cursor`.
    #[must_use]
    #[inline(always)]
    pub fn windowed(
        shell: MenuShell,
        art: BTreeMap<String, Artwork>,
        cursor: Option<(String, Artwork)>,
    ) -> Self {
        Self {
            shell: Mutex::new(Some(shell)),
            art,
            cursor,
            visuals: true,
        }
    }

    /// The model-only presenter for a headless drive: no sprites, no image
    /// assets needed.
    #[must_use]
    #[inline(always)]
    pub fn headless(shell: MenuShell) -> Self {
        Self {
            shell: Mutex::new(Some(shell)),
            art: BTreeMap::new(),
            cursor: None,
            visuals: false,
        }
    }
}

impl Plugin for FrontEndShellPlugin {
    fn build(&self, app: &mut App) {
        // `take`, not `clone`: the recursive `Clone` impl on `MenuShell`'s
        // section tree costs `cs` several KiB of `__eh_frame` for a call a
        // plugin build makes exactly once.
        let shell = self
            .shell
            .lock()
            .map(|mut slot| slot.take())
            .unwrap_or_default()
            .expect("a plugin builds once");
        let run = ShellRun {
            cursor: shell.cursor().map(|(name, _)| name.clone()),
            visited: shell
                .section_name()
                .map(|name| vec![name.to_owned()])
                .unwrap_or_default(),
            ..ShellRun::default()
        };
        let mut art = ShellArt::default();
        if self.visuals {
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            for (name, artwork) in &self.art {
                art.images
                    .insert(name.clone(), images.add(artwork_image(artwork)));
            }
            if let Some((_, artwork)) = &self.cursor {
                art.cursor = Some((
                    images.add(artwork_image(artwork)),
                    (artwork.width(), artwork.height()),
                ));
            }
        }
        app.insert_resource(ShellResource {
            shell,
            surface: SECTION_SURFACE,
            input: ShellInput {
                cursor: None,
                events: Vec::new(),
                cursor_events: MessageCursor::default(),
            },
            run,
            art,
            scene: ShellScene::default(),
            redraw: true,
        });
        if self.visuals {
            app.add_systems(Startup, startup_shell);
        }
        app.add_systems(Update, frame_shell);
    }
}

/// A model-only drive for tests: the input resources and messages the
/// windowed app already carries, no renderer. Kept out of the plugin so
/// `cs`'s linker drops the headless-only glue (`add_message`,
/// `init_resource`, `add_plugins::<InputPlugin>`) the production run never
/// calls.
pub fn headless_app(shell: MenuShell) -> App {
    let mut app = App::new();
    app.add_plugins(bevy::input::InputPlugin);
    app.add_message::<CursorMoved>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.init_resource::<ButtonInput<MouseButton>>();
    app.add_plugins(FrontEndShellPlugin::headless(shell));
    app
}

/// The primary window's entity, when the world has one.
#[inline(always)]
fn primary_window(world: &World) -> Option<Entity> {
    world
        .iter_entities()
        .find(|entity| entity.get::<PrimaryWindow>().is_some())
        .map(|entity| entity.id())
}

/// Startup: the 2D camera the whole surface draws through, the window entity
/// remembered, the OS cursor hidden only when cursor art drew — a missing
/// texture leaves the pointer visible, the recorded unknown it is — and the
/// smoke's capture directory created up front so a bad path fails before the
/// window even renders.
fn startup_shell(world: &mut World) {
    world.spawn((
        Camera2d,
        Camera {
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
    ));
    let window = primary_window(world);
    let has_cursor = world
        .get_resource_mut::<ShellResource>()
        .map(|mut state| {
            state.scene.window = window;
            state.art.cursor.is_some()
        })
        .unwrap_or(false);
    if has_cursor
        && let Some(mut cursor) = window.and_then(|entity| world.get_mut::<CursorOptions>(entity))
    {
        cursor.visible = false;
    }
    if let Some(mut smoke) = world.get_resource_mut::<ShellSmoke>()
        && let Err(error) = std::fs::create_dir_all(&smoke.request.capture_dir)
    {
        smoke.io_error = Some(format!(
            "cannot create {}: {error}",
            smoke.request.capture_dir.display()
        ));
    }
}

/// The frame's whole update, one exclusive system: injects a smoke's
/// scripted input into the real `ButtonInput`/`CursorMoved` path (the same
/// resources real input writes, so a scripted run goes through the same path
/// as a human's), collects it into [`ShellInput`] edges, tracks the window's
/// logical size as the fitting surface, applies the input to the
/// [`MenuShell`], rebuilds and refreshes the section's sprites, draws the
/// cursor art at the pointer, and finishes the smoke run when its script has
/// run out.
fn frame_shell(world: &mut World) {
    let Some(mut state) = world.remove_resource::<ShellResource>() else {
        return;
    };
    if world.contains_resource::<ShellSmoke>() {
        inject_smoke_input(world, &mut state);
    }
    let mut events = std::mem::take(&mut state.input.events);
    {
        let keys = world.resource::<ButtonInput<KeyCode>>();
        if keys.just_pressed(KeyCode::ArrowDown) {
            events.push(ShellEvent::FocusNext);
        }
        if keys.just_pressed(KeyCode::ArrowUp) {
            events.push(ShellEvent::FocusPrev);
        }
        if keys.just_pressed(KeyCode::Enter) {
            events.push(ShellEvent::ActivateDown);
        }
        if keys.just_released(KeyCode::Enter) {
            events.push(ShellEvent::ActivateUp);
        }
        if keys.just_pressed(KeyCode::Escape) {
            events.push(ShellEvent::Escape);
        }
        let mouse = world.resource::<ButtonInput<MouseButton>>();
        if mouse.just_pressed(MouseButton::Left) {
            events.push(ShellEvent::PointerDown);
        }
        if mouse.just_released(MouseButton::Left) {
            events.push(ShellEvent::PointerUp);
        }
    }
    {
        let messages = world.resource::<Messages<CursorMoved>>();
        for event in state.input.cursor_events.read(messages) {
            state.input.cursor = Some(event.position);
        }
    }
    if state.scene.window.is_none() {
        state.scene.window = primary_window(world);
    }
    if let Some(window) = state
        .scene
        .window
        .and_then(|entity| world.get::<Window>(entity))
    {
        state.surface = (window.width() as u32, window.height() as u32);
    }
    let in_smoke = world.contains_resource::<ShellSmoke>();
    let shell = &mut state.shell;
    if let Some(position) = state.input.cursor {
        if position.x < 0.0 || position.y < 0.0 {
            shell.pointer_leave();
        } else if let Some((x, y)) =
            MenuShell::unmap_point(state.surface, position.x as u32, position.y as u32)
        {
            shell.pointer_move(x, y);
        } else {
            shell.pointer_leave();
        }
    }
    for event in events.drain(..) {
        let outcome = match event {
            ShellEvent::PointerDown => {
                shell.pointer_down();
                ShellOutcome::Nothing
            }
            ShellEvent::PointerUp => shell.pointer_up(),
            ShellEvent::FocusNext => {
                shell.move_focus(true);
                state.run.focus_moves += 1;
                ShellOutcome::Nothing
            }
            ShellEvent::FocusPrev => {
                shell.move_focus(false);
                state.run.focus_moves += 1;
                ShellOutcome::Nothing
            }
            ShellEvent::ActivateDown => {
                shell.activate_down();
                ShellOutcome::Nothing
            }
            ShellEvent::ActivateUp => shell.activate_up(),
            ShellEvent::Escape => shell.escape(),
        };
        match outcome {
            ShellOutcome::Entered { section } | ShellOutcome::Back { section } => {
                state.run.visited.push(section);
                state.redraw = true;
            }
            ShellOutcome::Unhandled { .. } => state.run.unhandled += 1,
            ShellOutcome::Quit => {
                if in_smoke {
                    state.run.quit_seen = true;
                } else {
                    world
                        .resource_mut::<Messages<AppExit>>()
                        .write(AppExit::Success);
                }
            }
            ShellOutcome::Nothing => {}
        }
    }
    // The observable states the report checks: hovers and the depressed frame.
    if let Some(section) = shell.section() {
        if let Some(index) = shell.hover()
            && let Some(button) = section.buttons.get(index)
        {
            state.run.hovered.insert(button.id.clone());
        }
        let depressed = section
            .buttons
            .iter()
            .enumerate()
            .any(|(index, _)| shell.visual(index) == ButtonVisual::Depressed);
        if depressed {
            state.run.depressed_seen = true;
        }
    }
    let cursor_extent = state.art.cursor.as_ref().map(|(_, extent)| *extent);
    if let Some(view) = state.shell.view(state.surface) {
        if state.redraw {
            state.redraw = false;
            for entity in state
                .scene
                .panes
                .drain(..)
                .chain(state.scene.buttons.drain(..))
                .map(|drawn| drawn.entity)
            {
                let _ = world.despawn(entity);
            }
            let art = std::mem::take(&mut state.art);
            spawn_section(world, &art, state.surface, &view, &mut state.scene);
            state.art = art;
        }
        for drawn in &state.scene.panes {
            if let Some(pane) = view.panes.iter().find(|pane| pane.index == drawn.index) {
                place(
                    world,
                    drawn,
                    pane.texel_offset,
                    pane.logical,
                    pane.rect,
                    state.surface,
                );
            }
        }
        for drawn in &state.scene.buttons {
            if let Some(button) = view
                .buttons
                .iter()
                .find(|button| button.index == drawn.index)
            {
                place(
                    world,
                    drawn,
                    button.texel_offset,
                    button.logical,
                    button.rect,
                    state.surface,
                );
            }
        }
    }
    if let Some(cursor) = state.scene.cursor
        && let Ok(mut entity) = world.get_entity_mut(cursor)
    {
        match (state.input.cursor, cursor_extent) {
            (Some(position), Some(extent)) => {
                let scale = fit_scale(state.surface);
                if let Some(mut visibility) = entity.get_mut::<Visibility>() {
                    *visibility = Visibility::Visible;
                }
                if let Some(mut sprite) = entity.get_mut::<Sprite>() {
                    sprite.custom_size =
                        Some(Vec2::new(extent.0 as f32 * scale, extent.1 as f32 * scale));
                }
                if let Some(mut transform) = entity.get_mut::<Transform>() {
                    transform.translation.x = position.x - state.surface.0 as f32 * 0.5;
                    transform.translation.y = state.surface.1 as f32 * 0.5 - position.y;
                }
            }
            _ => {
                if let Some(mut visibility) = entity.get_mut::<Visibility>() {
                    *visibility = Visibility::Hidden;
                }
            }
        }
    }
    if let Some(mut smoke) = world.remove_resource::<ShellSmoke>() {
        step_smoke(world, &state, &mut smoke);
        world.insert_resource(smoke);
    }
    world.insert_resource(state);
}

/// The scale the aspect-fit applies: fitted width over logical width.
#[inline(always)]
fn fit_scale(surface: (u32, u32)) -> f32 {
    cs_content::ui_layout::AspectFit::new(SECTION_SURFACE, surface)
        .map(|fit| fit.image_rect().width as f32 / SECTION_SURFACE.0 as f32)
        .unwrap_or(1.0)
}

/// Spawns the sprites of one section frame (panes under buttons), records
/// them in `scene`, and spawns the cursor sprite once — it lives outside the
/// section's lists so navigation never respawns it.
#[inline(always)]
fn spawn_section(
    world: &mut World,
    art: &ShellArt,
    surface: (u32, u32),
    view: &ShellView,
    scene: &mut ShellScene,
) {
    for (order, pane) in view.panes.iter().enumerate() {
        let Some(image) = art.images.get(&pane.art.to_ascii_lowercase()) else {
            continue;
        };
        let z = 1.0 + order as f32 * 0.01;
        let entity = world
            .spawn((
                Sprite {
                    image: image.clone(),
                    rect: Some(texel_rect(pane.texel_offset, pane.logical)),
                    custom_size: Some(rect_size(pane.rect)),
                    ..default()
                },
                Transform::from_translation(centre(pane.rect, surface, z)),
            ))
            .id();
        scene.panes.push(DrawnSprite {
            index: pane.index,
            z,
            entity,
        });
    }
    for button in &view.buttons {
        let Some(image) = art.images.get(&button.art.to_ascii_lowercase()) else {
            continue;
        };
        let z = 10.0 + button.index as f32 * 0.01;
        let entity = world
            .spawn((
                Sprite {
                    image: image.clone(),
                    rect: Some(texel_rect(button.texel_offset, button.logical)),
                    custom_size: Some(rect_size(button.rect)),
                    ..default()
                },
                Transform::from_translation(centre(button.rect, surface, z)),
            ))
            .id();
        scene.buttons.push(DrawnSprite {
            index: button.index,
            z,
            entity,
        });
    }
    if scene.cursor.is_none()
        && let Some((image, _)) = &art.cursor
    {
        // `(Sprite, Transform)` like the panes and buttons — `Visibility`
        // arrives as a required component — so one bundle type serves every
        // sprite the presenter spawns.
        let cursor = world
            .spawn((
                Sprite {
                    image: image.clone(),
                    ..default()
                },
                Transform::from_xyz(0.0, 0.0, 100.0),
            ))
            .id();
        if let Some(mut visibility) = world.get_mut::<Visibility>(cursor) {
            *visibility = Visibility::Hidden;
        }
        scene.cursor = Some(cursor);
    }
}

/// The texture sub-rect a drawn element shows: `texel_offset` in the art plus
/// the element's own (clipped) logical size.
#[inline(always)]
fn texel_rect(offset: (u32, u32), logical: cs_content::ui_layout::Rect) -> bevy::math::Rect {
    bevy::math::Rect::new(
        offset.0 as f32,
        offset.1 as f32,
        offset.0 as f32 + logical.width as f32,
        offset.1 as f32 + logical.height as f32,
    )
}

/// A surface rect's size as a sprite size.
#[inline(always)]
fn rect_size(rect: cs_content::ui_layout::Rect) -> Vec2 {
    Vec2::new(rect.width as f32, rect.height as f32)
}

/// A surface rect's centre in world space: the surface is centred on the
/// camera, y up.
#[inline(always)]
fn centre(rect: cs_content::ui_layout::Rect, surface: (u32, u32), z: f32) -> bevy::math::Vec3 {
    bevy::math::Vec3::new(
        rect.x as f32 + rect.width as f32 * 0.5 - surface.0 as f32 * 0.5,
        surface.1 as f32 * 0.5 - (rect.y as f32 + rect.height as f32 * 0.5),
        z,
    )
}

/// Refreshes one sprite's frame crop, size and placement from the view.
#[inline(always)]
fn place(
    world: &mut World,
    drawn: &DrawnSprite,
    texel_offset: (u32, u32),
    logical: cs_content::ui_layout::Rect,
    rect: cs_content::ui_layout::Rect,
    surface: (u32, u32),
) {
    let Ok(mut entity) = world.get_entity_mut(drawn.entity) else {
        return;
    };
    if let Some(mut sprite) = entity.get_mut::<Sprite>() {
        sprite.rect = Some(texel_rect(texel_offset, logical));
        sprite.custom_size = Some(rect_size(rect));
    }
    if let Some(mut transform) = entity.get_mut::<Transform>() {
        transform.translation = centre(rect, surface, drawn.z);
    }
}

/// The artwork as a Bevy texture (the same orientation
/// [`super::capture::artwork_image`] uses: row-major from the top).
#[inline(always)]
fn artwork_image(artwork: &Artwork) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: artwork.width(),
            height: artwork.height(),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        artwork.rgba().to_vec(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::linear();
    image.texture_descriptor.usage |= TextureUsages::TEXTURE_BINDING;
    image
}

// ----------------------------------------------------------------- smoke ---

/// The smoke run, one resource: the collected data, the delivered shots,
/// the remaining script and the static config.
#[derive(Resource)]
struct ShellSmoke {
    /// Rendered frames so far.
    frames: u32,
    /// Screenshots requested.
    shots_requested: usize,
    /// An artifact write failed.
    io_error: Option<String>,
    /// Screenshots delivered by the renderer, shared with the observer.
    shots: Arc<Mutex<Vec<ShellShotRecord>>>,
    /// The scripted steps.
    steps: Vec<(u32, Step)>,
    /// Steps already applied.
    step: usize,
    /// The run's parameters.
    request: SmokeRequest,
    /// The slot the outcome is reported through.
    handle: ShellSmokeHandle,
}

/// Shared slot the smoke fills when the run ends.
#[derive(Clone, Default)]
pub struct ShellSmokeHandle(Arc<Mutex<Option<Result<ShellSmokeReport, String>>>>);

impl ShellSmokeHandle {
    /// The outcome once the run has ended.
    #[inline(always)]
    pub fn take(&self) -> Option<Result<ShellSmokeReport, String>> {
        self.0.lock().ok().and_then(|mut slot| slot.take())
    }
}

/// What one captured screenshot measured.
#[derive(Clone, Debug, PartialEq)]
pub struct ShellShotRecord {
    /// Why the frame was taken.
    pub label: String,
    /// The PNG the renderer wrote.
    pub path: PathBuf,
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Distinct luminance levels in the frame.
    pub distinct_luminance: usize,
    /// Mean luminance, 0-255.
    pub mean_luminance: f32,
}

/// The finished smoke run, for the caller of [`run_front_end`].
#[derive(Clone, Debug, PartialEq)]
pub struct ShellSmokeReport {
    /// Rendered frames.
    pub frames: u32,
    /// Sections visited, root first.
    pub visited: Vec<String>,
    /// Focus moves applied.
    pub focus_moves: u32,
    /// Buttons hovered (row ids).
    pub hovered: Vec<String>,
    /// A button drew its depressed frame.
    pub depressed_seen: bool,
    /// The quit outcome arrived.
    pub quit_seen: bool,
    /// Presses with no known effect.
    pub unhandled: usize,
    /// The cursor art found.
    pub cursor: Option<String>,
    /// The screenshots taken.
    pub shots: Vec<ShellShotRecord>,
    /// Every failed check; empty means the run passed.
    pub failures: Vec<String>,
}

/// One scripted input.
#[derive(Clone)]
enum Step {
    /// Move the pointer to the centre of the named button (resolved at run
    /// time from the model, so the same step works at any surface size).
    HoverButton(&'static str),
    /// Left button down/up.
    Mouse(bool),
    /// A key down/up.
    Key(KeyCode, bool),
    /// A framebuffer screenshot.
    Shot(&'static str),
}

/// The deterministic pass the smoke drives over `MainMenu`:
///
/// settle → screenshot → hover `MM_B_CAMPAIGN` (rollover frame) → press it
/// (depressed frame) → land on `Campaign` → screenshot → `Esc` back →
/// `Down` five times to `MM_B_QUIT` (the focused/rollover frame) → `Enter`
/// (depressed, then the `terminate` the script states).
#[inline(always)]
fn script() -> Vec<(u32, Step)> {
    let at = |seconds: f64| (seconds * SMOKE_FRAME_HZ) as u32;
    let mut steps = vec![
        (at(0.5), Step::Shot("mainmenu")),
        (at(1.0), Step::HoverButton("MM_B_CAMPAIGN")),
        (at(1.5), Step::Shot("hover")),
        (at(2.0), Step::Mouse(true)),
        (at(2.2), Step::Shot("pressed")),
        (at(2.4), Step::Mouse(false)),
        (at(3.0), Step::Shot("campaign")),
        (at(3.5), Step::Key(KeyCode::Escape, true)),
        (at(3.6), Step::Key(KeyCode::Escape, false)),
        (at(4.0), Step::Shot("back")),
    ];
    for i in 0..5 {
        steps.push((
            at(4.3 + f64::from(i) * 0.2),
            Step::Key(KeyCode::ArrowDown, true),
        ));
        steps.push((
            at(4.35 + f64::from(i) * 0.2),
            Step::Key(KeyCode::ArrowDown, false),
        ));
    }
    steps.push((at(5.4), Step::Shot("focus-quit")));
    steps.push((at(5.6), Step::Key(KeyCode::Enter, true)));
    steps.push((at(5.7), Step::Shot("depressed")));
    steps.push((at(5.8), Step::Key(KeyCode::Enter, false)));
    steps
}

/// The smoke run as a plugin.
pub struct ShellSmokePlugin {
    request: SmokeRequest,
    steps: Vec<(u32, Step)>,
    handle: ShellSmokeHandle,
}

impl ShellSmokePlugin {
    /// A windowed smoke for `request`.
    #[must_use]
    #[inline(always)]
    pub fn windowed(request: SmokeRequest) -> Self {
        Self {
            request,
            steps: script(),
            handle: ShellSmokeHandle::default(),
        }
    }

    /// The handle the caller takes the outcome from.
    #[must_use]
    #[inline(always)]
    pub fn handle(&self) -> ShellSmokeHandle {
        self.handle.clone()
    }
}

impl Plugin for ShellSmokePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ShellSmoke {
            frames: 0,
            shots_requested: 0,
            io_error: None,
            shots: Arc::new(Mutex::new(Vec::new())),
            steps: self.steps.clone(),
            step: 0,
            request: self.request.clone(),
            handle: self.handle.clone(),
        });
    }
}

/// Injects the frame's scripted input into the real `ButtonInput`/`CursorMoved`
/// path — the input systems' own per-frame clear already ran in `PreUpdate`,
/// so scripted `just_*` edges survive exactly one frame like real ones.
#[inline(always)]
fn inject_smoke_input(world: &mut World, state: &mut ShellResource) {
    let Some(mut smoke) = world.remove_resource::<ShellSmoke>() else {
        return;
    };
    world.resource_mut::<ButtonInput<KeyCode>>().clear();
    world.resource_mut::<ButtonInput<MouseButton>>().clear();
    let frame = smoke.frames;
    while smoke
        .steps
        .get(smoke.step)
        .is_some_and(|(at, _)| *at <= frame)
    {
        match &smoke.steps[smoke.step].1 {
            Step::HoverButton(id) => {
                // The centre of the named button's surface rect this frame —
                // `None` when the section shows no such button (the step then
                // does nothing, and the run's own checks fail it).
                let position = state.shell.view(state.surface).and_then(|view| {
                    view.buttons
                        .iter()
                        .find(|button| button.id == *id)
                        .map(|button| {
                            Vec2::new(
                                button.rect.x as f32 + button.rect.width as f32 * 0.5,
                                button.rect.y as f32 + button.rect.height as f32 * 0.5,
                            )
                        })
                });
                if let Some(position) = position {
                    world
                        .resource_mut::<Messages<CursorMoved>>()
                        .write(CursorMoved {
                            window: state.scene.window.unwrap_or(Entity::PLACEHOLDER),
                            position,
                            delta: None,
                        });
                }
            }
            Step::Mouse(down) => {
                let mut mouse = world.resource_mut::<ButtonInput<MouseButton>>();
                if *down {
                    mouse.press(MouseButton::Left);
                } else {
                    mouse.release(MouseButton::Left);
                }
            }
            Step::Key(key, down) => {
                let mut keys = world.resource_mut::<ButtonInput<KeyCode>>();
                if *down {
                    keys.press(*key);
                } else {
                    keys.release(*key);
                }
            }
            Step::Shot(label) => {
                smoke.shots_requested += 1;
                let dir = smoke.request.capture_dir.clone();
                let shots = smoke.shots.clone();
                screenshot(world, &dir, label, &shots);
            }
        }
        smoke.step += 1;
    }
    world.insert_resource(smoke);
}

/// Counts the frame and finishes the run once the script has played out:
/// writes the artifacts, posts the outcome and asks the app to exit.
#[inline(always)]
fn step_smoke(world: &mut World, state: &ShellResource, smoke: &mut ShellSmoke) {
    let frame = smoke.frames;
    let end_frame = smoke.request.seconds * SMOKE_FRAME_HZ as u32;
    // The final framebuffer arrives a little before the run ends.
    if frame == end_frame.saturating_sub(12) {
        smoke.shots_requested += 1;
        let dir = smoke.request.capture_dir.clone();
        let shots = smoke.shots.clone();
        screenshot(world, &dir, "final", &shots);
    }
    let delivered = smoke.shots.lock().map_or(0, |shots| shots.len());
    let finished = delivered >= smoke.shots_requested;
    if frame >= end_frame && (finished || frame >= end_frame + SCREENSHOT_GRACE_FRAMES) {
        let report = evaluate(smoke, &state.run);
        let outcome = write_artifacts(&smoke.request.capture_dir, &report)
            .map(|()| report.clone())
            .map_err(|(path, error)| format!("cannot write {}: {error}", path.display()));
        if let Ok(mut slot) = smoke.handle.0.lock() {
            *slot = Some(outcome);
        }
        world
            .resource_mut::<Messages<AppExit>>()
            .write(AppExit::Success);
        return;
    }
    smoke.frames += 1;
}

/// The screenshot spawn shared by the script and the final frame.
#[inline(always)]
fn screenshot(
    world: &mut World,
    dir: &Path,
    label: &'static str,
    shots: &Arc<Mutex<Vec<ShellShotRecord>>>,
) {
    let path = dir.join(format!("{label}.png"));
    let saved = path.clone();
    let label = label.to_owned();
    let shots = shots.clone();
    world
        .spawn(Screenshot::primary_window())
        .observe(move |captured: On<ScreenshotCaptured>| {
            let record = measure_shot(&captured.image, label.clone(), saved.clone());
            let delivered = captured.image.data.is_some();
            save_to_disk(saved.clone())(captured);
            if delivered && let Ok(mut shots) = shots.lock() {
                shots.push(record);
            }
        });
}

/// The luminance facts of one captured frame.
#[inline(always)]
fn measure_shot(image: &Image, label: String, path: PathBuf) -> ShellShotRecord {
    let mut levels = BTreeSet::new();
    let mut sum = 0_u64;
    let mut count = 0_u64;
    if let Some(data) = image.data.as_ref() {
        for pixel in data.as_chunks::<4>().0 {
            let luminance =
                ((29 * u32::from(pixel[0]) + 150 * u32::from(pixel[1]) + 77 * u32::from(pixel[2]))
                    >> 8) as u8;
            levels.insert(luminance);
            sum += u64::from(luminance);
            count += 1;
        }
    }
    ShellShotRecord {
        label,
        path,
        width: image.width(),
        height: image.height(),
        distinct_luminance: levels.len(),
        mean_luminance: if count == 0 {
            0.0
        } else {
            sum as f32 / count as f32
        },
    }
}

/// The pass/fail checks of a finished run.
#[inline(always)]
fn evaluate(smoke: &ShellSmoke, run: &ShellRun) -> ShellSmokeReport {
    let shots = smoke
        .shots
        .lock()
        .map(|shots| shots.clone())
        .unwrap_or_default();
    let mut failures = Vec::new();
    if let Some(error) = &smoke.io_error {
        failures.push(error.clone());
    }
    if shots.len() < smoke.shots_requested || shots.is_empty() {
        failures.push(format!(
            "only {} of {} requested framebuffer screenshots arrived",
            shots.len(),
            smoke.shots_requested
        ));
    }
    for shot in &shots {
        if shot.distinct_luminance < MIN_DISTINCT_LUMINANCE || shot.mean_luminance < 4.0 {
            failures.push(format!(
                "framebuffer {} is empty or uniform ({} luminance levels, mean {:.1})",
                shot.path.display(),
                shot.distinct_luminance,
                shot.mean_luminance
            ));
        }
    }
    let expected = ["MainMenu", "Campaign", "MainMenu"];
    if run.visited != expected {
        failures.push(format!(
            "the script visited {:?}, expected MainMenu → Campaign → MainMenu",
            run.visited
        ));
    }
    if !run.hovered.iter().any(|id| id == "MM_B_CAMPAIGN") {
        failures.push("the pointer never hovered MM_B_CAMPAIGN".to_owned());
    }
    if !run.depressed_seen {
        failures.push("no button ever drew its depressed frame".to_owned());
    }
    if run.focus_moves < 5 {
        failures.push(format!(
            "focus moved only {} times (expected 5 Down presses)",
            run.focus_moves
        ));
    }
    if !run.quit_seen {
        failures.push("the Quit button's terminate never arrived".to_owned());
    }
    if run.unhandled != 0 {
        failures.push(format!(
            "{} presses landed on buttons with no known effect",
            run.unhandled
        ));
    }
    ShellSmokeReport {
        frames: smoke.frames,
        visited: run.visited.clone(),
        focus_moves: run.focus_moves,
        hovered: run.hovered.iter().cloned().collect(),
        depressed_seen: run.depressed_seen,
        quit_seen: run.quit_seen,
        unhandled: run.unhandled,
        cursor: run.cursor.clone(),
        shots,
        failures,
    }
}

/// Writes `report.json` (the trace is the visited/hover record the report
/// already carries; the framebuffers are the PNGs themselves).
#[inline(always)]
fn write_artifacts(dir: &Path, report: &ShellSmokeReport) -> Result<(), (PathBuf, std::io::Error)> {
    std::fs::create_dir_all(dir).map_err(|error| (dir.to_path_buf(), error))?;
    let shots = report
        .shots
        .iter()
        .map(|shot| {
            format!(
                "{{\"label\":{},\"path\":{},\"width\":{},\"height\":{},\"distinct_luminance\":{},\"mean_luminance\":{}}}",
                json_string(&shot.label),
                json_string(&shot.path.display().to_string()),
                shot.width,
                shot.height,
                shot.distinct_luminance,
                json_f32(shot.mean_luminance)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let visited = report
        .visited
        .iter()
        .map(|name| json_string(name))
        .collect::<Vec<_>>()
        .join(",");
    let hovered = report
        .hovered
        .iter()
        .map(|id| json_string(id))
        .collect::<Vec<_>>()
        .join(",");
    let failures = report
        .failures
        .iter()
        .map(|failure| json_string(failure))
        .collect::<Vec<_>>()
        .join(",");
    let body = format!(
        "{{\"kind\":\"front-end-shell-smoke-report\",\"frames\":{},\"visited\":[{}],\"focus_moves\":{},\"hovered\":[{}],\"depressed_seen\":{},\"quit_seen\":{},\"unhandled\":{},\"cursor\":{},\"shots\":[{}],\"failures\":[{}],\"passed\":{}}}\n",
        report.frames,
        visited,
        report.focus_moves,
        hovered,
        report.depressed_seen,
        report.quit_seen,
        report.unhandled,
        report
            .cursor
            .as_ref()
            .map_or("null".to_owned(), |name| json_string(name)),
        shots,
        failures,
        report.failures.is_empty(),
    );
    let report_path = dir.join("report.json");
    std::fs::write(&report_path, body).map_err(|error| (report_path, error))
}

#[inline(always)]
fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[inline(always)]
fn json_f32(value: f32) -> String {
    if value.is_finite() {
        value.to_string()
    } else {
        "null".to_owned()
    }
}

// -------------------------------------------------------------------- run ---

/// Builds the windowed front-end app: reads the installation's layout,
/// presents every section through the shell, decodes the art the sections
/// reference and installs the presenter (plus the smoke, when requested).
///
/// # Errors
///
/// [`FrontEndError::Retail`] when the installation cannot be read,
/// [`FrontEndError::Shell`] when the entry section is absent — both before
/// any window exists.
pub fn shell_app(
    request: &FrontEndRequest,
) -> Result<(App, Option<ShellSmokeHandle>), FrontEndError> {
    let screens = FrontEndScreens::open(&request.cs_path).map_err(FrontEndError::Retail)?;
    let layout = screens.original_layout().map_err(FrontEndError::Retail)?;
    let mut shell = MenuShell::build(
        &layout,
        &|art: &str| screens.art_extent(art),
        &|section: &str| screens.script_text(&script_member(section)),
    );
    shell.open(START_SECTION).map_err(FrontEndError::Shell)?;

    // Every art name the sections reference, decoded once: the shell can
    // reach any of them, so the texture cache is complete up front.
    let mut art: BTreeMap<String, Artwork> = BTreeMap::new();
    for section in shell.sections().values() {
        for element in section
            .panes
            .iter()
            .map(|pane| &pane.art)
            .chain(section.buttons.iter().map(|button| &button.art))
        {
            if let Some(artwork) = screens.graphic_artwork(element) {
                art.entry(element.to_ascii_lowercase()).or_insert(artwork);
            }
        }
    }
    let cursor = shell.cursor().and_then(|(name, _)| {
        screens
            .graphic_artwork(name)
            .map(|artwork| (name.clone(), artwork))
    });

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Crimson Skies — front end".to_owned(),
            resolution: (SECTION_SURFACE.0, SECTION_SURFACE.1).into(),
            ..Window::default()
        }),
        ..WindowPlugin::default()
    }));
    if request.smoke.is_some() {
        // A smoke run is deterministic: every rendered frame is exactly
        // 1/60 s of simulated time whatever the machine's frame pacing.
        app.insert_resource(TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f64(1.0 / SMOKE_FRAME_HZ),
        ));
    }
    app.add_plugins(FrontEndShellPlugin::windowed(shell, art, cursor));
    let handle = request.smoke.as_ref().map(|smoke| {
        let plugin = ShellSmokePlugin::windowed(smoke.clone());
        let handle = plugin.handle();
        app.add_plugins(plugin);
        handle
    });
    Ok((app, handle))
}

/// Turns the finished smoke into the process result and prints the summary.
///
/// # Errors
///
/// [`FrontEndError::SmokeFailed`] when any check failed,
/// [`FrontEndError::Exit`] when the run produced no report.
#[inline(always)]
fn finish(
    handle: &ShellSmokeHandle,
    request: &SmokeRequest,
) -> Result<ShellSmokeReport, FrontEndError> {
    let outcome = handle
        .take()
        .ok_or_else(|| FrontEndError::Exit("the smoke run ended before it finished".to_owned()))?;
    let report = outcome.map_err(FrontEndError::Exit)?;
    println!(
        "front-end smoke: {}\n  frames {} visited [{}] focus moves {} unhandled {} cursor {}\n  framebuffers {} in {}",
        report.visited.join(" → "),
        report.frames,
        report.visited.join(", "),
        report.focus_moves,
        report.unhandled,
        report.cursor.as_deref().unwrap_or("none recorded"),
        report.shots.len(),
        request.capture_dir.display(),
    );
    if report.failures.is_empty() {
        Ok(report)
    } else {
        Err(FrontEndError::SmokeFailed(report.failures))
    }
}

/// Runs `cs --cs-path <retail> --front-end`: opens the window and blocks
/// until it closes (or the smoke run ends).
///
/// # Errors
///
/// [`FrontEndError`] when the installation cannot be read, the entry section
/// is absent, the smoke fails its checks or the app exits with an error.
pub fn run_front_end(request: &FrontEndRequest) -> Result<(), FrontEndError> {
    let (mut app, handle) = shell_app(request)?;
    let exit = app.run();
    if let (Some(handle), Some(smoke)) = (&handle, &request.smoke) {
        finish(handle, smoke)?;
    }
    match exit {
        AppExit::Success => Ok(()),
        AppExit::Error(code) => Err(FrontEndError::Exit(format!("exit code {code}"))),
    }
}
