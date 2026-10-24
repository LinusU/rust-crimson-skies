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
//! respawn. Every `M` row is a looping movie decoded through the
//! cinematics' [`MpegVideoSource`] into a texture refreshed per frame,
//! drawn under the panes at the row's authored `X,Y` and `ScaleX`/`ScaleY`
//! percent (FE-MENU-2). The cursor art the shell found is drawn at the
//! pointer and the OS cursor is hidden; when no cursor art decoded the
//! record says so ([`ShellRun::cursor`]) and the OS cursor is left visible.
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
//! cancel back, then walk Instant Action, Multiplayer (the recorded
//! unhandled press — its `MultiPlayerMain` section does not exist),
//! Preferences and Credits, each back to `MainMenu` by its own button; then
//! `Down`-walk focus to Quit and `Enter` it. The run records framebuffer
//! PNGs, a trace and `report.json` into the capture directory, and **fails**
//! (exit nonzero) when a shot is empty/uniform, the navigation did not
//! happen, the flag movie never opened or never changed a frame, the
//! depressed frame never drew, or the quit never arrived.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fmt::Write as _;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::app::AppExit;
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::ecs::message::{MessageCursor, Messages};
use bevy::image::{Image, ImageSampler};
use bevy::input::ButtonInput;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::input::mouse::MouseButton;
use bevy::prelude::{
    App, Camera, Camera2d, ClearColorConfig, Color, DefaultPlugins, Entity, On, Plugin,
    PluginGroup, PreUpdate, Resource, Sprite, Startup, Time, Transform, Update, Vec2, Visibility,
    Window, WindowPlugin, World, default,
};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::sprite::Anchor;
use bevy::time::TimeUpdateStrategy;
use bevy::window::{CursorMoved, CursorOptions, PrimaryWindow};
use cs_sim::cinematic_state::MediaFailure;

use super::audio_page::{
    ACCEPT_BUTTON, AUDIO_SECTION, AudioControl, AudioPage, AudioSettingsFile, MUSIC_BUTTON,
};
use super::cabin::CabinHub;
use super::intro::{self, IntroMovie, IntroState};
use super::movie::{MovieBackdrop, movie_media};
use super::original::script_member;
use super::pilot_page::{CONTINUE_BUTTON, DELETE_BUTTON, PILOT_SECTION, PilotOutcome, PilotPage};
use super::plane_name::{NAME_SECTION, NameOutcome, PlaneNamePage};
use super::previous_missions::{PreviousMissionsPage, TOC_SECTION, TocOutcome};
use super::retail::{FrontEndScreens, RetailError};
use super::shell::{
    ButtonVisual, MenuShell, SECTION_SURFACE, START_SECTION, ShellOutcome, ShellView,
};
use super::sound::{MenuAudio, MenuAudioEvent, MenuCue, MenuMix, MenuSamples, MenuSoundTable};
use super::start_flow::{
    CONSTRUCTION_NOT_BUILT, FIRST_MISSION, NotImplemented, StartFlow, StartStep,
};
use super::{Artwork, ShellError};
use crate::audio::CapabilityDeclaration;
use crate::cinematics::movie_player::{AudioOutput, MoviePlayerPlugin};
use crate::cinematics::mpeg::MpegVideoSource;
use crate::playtest::SmokeRequest;
use crate::playtest::smoke::SMOKE_FRAME_HZ;
use crate::profile::ProfileSession;

/// The scripted pass needs this many simulated seconds: the last scripted
/// input lands at 12.6 s and the run wants settling frames after it.
pub const MIN_SHELL_SMOKE_SECONDS: u32 = 14;

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
    /// The section `M` rows' movie files, lower-cased file name → the read
    /// (bytes, or the recorded read refusal). Empty on a headless drive,
    /// where no movie is attempted.
    movie_files: BTreeMap<String, Result<Vec<u8>, String>>,
    /// Whether sprites draw at all: `false` on the headless drive, where
    /// movies are neither opened nor recorded.
    visuals: bool,
    /// The entities the presenter spawned.
    scene: ShellScene,
    /// The section's sprites are stale: respawn them.
    redraw: bool,
    /// The F45 machine that decides which moves the shell may make; `Some` in
    /// the start flow (`cs --cs-path <retail>`), `None` in `--front-end`.
    flow: Option<StartFlow>,
    /// The Campaign pilot page and the profile session behind it, when the
    /// run has them (FE-MENU-3).
    pilots: Option<PilotState>,
    /// The front end's music and cues, when the run has an output (FE-MENU-4).
    audio: Option<MenuAudio>,
    /// The button the pointer or focus was last on, so a cue plays on entry.
    watch: AudioWatch,
    /// The intro movie between the pilot page and the cabin.
    intro: IntroState,
    /// The not-implemented notice's entities and the surface they were laid
    /// out for; empty while no notice is up.
    notice_scene: (Vec<Entity>, (u32, u32)),
    /// The Preferences → Audio page (#1996), when the run has one.
    audio_page: Option<AudioPageState>,
    /// The cabin's Previous Missions page (#1956), when the run has one.
    previous: Option<PreviousState>,
    /// The cabin's Plane Name page (#1958), when the run has one.
    plane: Option<PlaneState>,
}

/// The Plane Name page's live state.
struct PlaneState {
    page: PlaneNamePage,
    /// What the page's buttons did, in order.
    outcomes: Vec<NameOutcome>,
    /// Typed keys the page has not read yet.
    keys: MessageCursor<KeyboardInput>,
    /// The overlay's entities while the page is on screen.
    scene: Vec<Entity>,
    /// The surface the overlay was laid out for.
    drawn_surface: (u32, u32),
}

/// The Previous Missions page's live state.
struct PreviousState {
    page: PreviousMissionsPage,
    /// What the page's buttons did, in order.
    outcomes: Vec<TocOutcome>,
    /// The overlay's entities while the page is on screen.
    scene: Vec<Entity>,
    /// The surface the overlay was laid out for.
    drawn_surface: (u32, u32),
}

/// The Audio page's live state.
struct AudioPageState {
    page: AudioPage,
    /// The overlay's entities while the page is on screen.
    scene: Vec<Entity>,
    /// The surface the overlay was laid out for.
    drawn_surface: (u32, u32),
    /// The page was on screen last frame: leaving it commits or discards.
    was_on: bool,
    /// The button the last release was on (`Accept` commits, anything else
    /// discards).
    released: Option<String>,
    /// The mix last handed to the menu audio.
    applied: Option<MenuMix>,
}

/// What the audio hooks last saw of the shell.
#[derive(Default)]
struct AudioWatch {
    /// The section last watched: a screen change (or the first frame) moves
    /// the focus to its first button without anyone touching anything, and
    /// that is not a rollover.
    section: Option<String>,
    /// `(section, button index)` of the button in rollover or depressed.
    target: Option<(String, usize)>,
    /// A button was held down.
    depressed: bool,
}

/// The pilot page's live state.
struct PilotState {
    page: PilotPage,
    session: ProfileSession,
    /// What Continue / Delete did, in order.
    outcomes: Vec<PilotOutcome>,
    /// The overlay's entities while the page is on screen.
    scene: Vec<Entity>,
    /// The surface the overlay was laid out for.
    drawn_surface: (u32, u32),
    /// The keyboard message reader that survives between frames.
    keys: MessageCursor<KeyboardInput>,
}

/// The pilot page and the profile session it drives, for
/// [`FrontEndShellPlugin::with_pilots`].
pub struct PilotSetup {
    /// The page, read from the layout.
    pub page: PilotPage,
    /// The session the page creates, chooses and deletes pilots through.
    pub session: ProfileSession,
}

impl ShellResource {
    /// The Audio page, when the run has one.
    #[must_use]
    pub fn audio_page(&self) -> Option<&AudioPage> {
        self.audio_page.as_ref().map(|state| &state.page)
    }

    /// The menu audio, when the run has an output.
    #[must_use]
    pub fn menu_audio(&self) -> Option<&MenuAudio> {
        self.audio.as_ref()
    }

    /// The pilot page, when the run has one.
    #[must_use]
    pub fn pilot_page(&self) -> Option<&PilotPage> {
        self.pilots.as_ref().map(|pilots| &pilots.page)
    }

    /// The Previous Missions page, when the run has one.
    #[must_use]
    pub fn previous_missions(&self) -> Option<&PreviousMissionsPage> {
        self.previous.as_ref().map(|state| &state.page)
    }

    /// The Plane Name page, when the run has one.
    #[must_use]
    pub fn plane_name(&self) -> Option<&PlaneNamePage> {
        self.plane.as_ref().map(|state| &state.page)
    }

    /// What the Plane Name page's buttons did, in order.
    #[must_use]
    pub fn plane_outcomes(&self) -> &[NameOutcome] {
        self.plane
            .as_ref()
            .map_or(&[], |state| state.outcomes.as_slice())
    }

    /// What the Previous Missions page's buttons did, in order.
    #[must_use]
    pub fn previous_outcomes(&self) -> &[TocOutcome] {
        self.previous
            .as_ref()
            .map_or(&[], |state| state.outcomes.as_slice())
    }

    /// The profile session behind the pilot page.
    #[must_use]
    pub fn pilot_session(&self) -> Option<&ProfileSession> {
        self.pilots.as_ref().map(|pilots| &pilots.session)
    }

    /// What the pilot page's Continue and Delete did, in order.
    #[must_use]
    pub fn pilot_outcomes(&self) -> &[PilotOutcome] {
        self.pilots
            .as_ref()
            .map_or(&[], |pilots| pilots.outcomes.as_slice())
    }

    /// The intro movie's state: whether it is up and how each one ended.
    #[must_use]
    pub fn intro(&self) -> &IntroState {
        &self.intro
    }

    /// The front end's audio, when this run has an output.
    #[must_use]
    pub fn audio(&self) -> Option<&MenuAudio> {
        self.audio.as_ref()
    }

    /// The start flow's machine, when this run has one.
    #[must_use]
    pub fn flow(&self) -> Option<&StartFlow> {
        self.flow.as_ref()
    }
}

/// The decoded art.
#[derive(Default)]
struct ShellArt {
    /// Strip/pane textures by lower-cased layout art name.
    images: BTreeMap<String, Handle<Image>>,
    /// Logical-to-texture texel scale of each texture in `images` that is
    /// drawn from an `--hd` replacement ([`Artwork::presented_texel`]).
    scales: BTreeMap<String, (f32, f32)>,
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
    /// `ArrowLeft`: the Audio page's focused slider or list steps down.
    Left,
    /// `ArrowRight`: the Audio page's focused slider or list steps up.
    Right,
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
    /// What each presented `M` row did: `MOVIE <id> on <section>: <file>
    /// <video extent> @<rate> fps, loops <n|continuous>` once it opened, or
    /// `refused (<reason>)` when the bytes or the decode refused.
    movies: Vec<String>,
    /// Decoded movie frames delivered to a texture, all sections.
    movie_frames: u64,
    /// Why the run has no audio output, when it has none.
    audio_note: Option<String>,
}

/// The live scene: every entity the presenter spawned, so `sync_shell` can
/// touch them with `World::get_mut` — each `query_filtered::<D, F>`
/// instantiation is a `QueryState` + fetch monomorphization `cs`'s
/// `__eh_frame` budget cannot afford.
#[derive(Default)]
struct ShellScene {
    /// The primary window, when one exists.
    window: Option<Entity>,
    /// The section's movie sprites (under the panes).
    movies: Vec<MovieSprite>,
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
    /// The texture's texel scale (`(1, 1)` for original art).
    scale: (f32, f32),
}

/// The main menu's panes the Campaign page keeps showing under its own: the
/// logo, per the owner's Drydock capture (the button frame is hidden).
const PILOT_CARRIED_PANES: &[&str] = &["MM_LOGO"];

/// The decoder a retail movie file opens over its read bytes.
type RetailMovie = MpegVideoSource<Cursor<Vec<u8>>>;

/// The reopen closure a [`MovieBackdrop`] calls at each loop point.
type RetailMovieOpen = Box<dyn FnMut() -> Result<RetailMovie, MediaFailure> + Send + Sync>;

/// One movie sprite: the player writing frames into its texture, the
/// texture and the entity.
struct MovieSprite {
    /// The draw layer, under the panes.
    z: f32,
    /// The playing movie. Locked so the resource stays `Sync` whatever the
    /// decoder's auto-traits are; it is touched once a frame.
    backdrop: Mutex<MovieBackdrop<RetailMovie, RetailMovieOpen>>,
    /// The drawn rect, computed once from the decoded extent.
    rect: super::shell::ShellRect,
    /// The row's `ScaleX`/`ScaleY` percent — drawn px → video texels.
    scale: (u32, u32),
    /// The texture the delivered frame is written to.
    image: Handle<Image>,
    /// The sprite entity.
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
    movie_files: Mutex<BTreeMap<String, Result<Vec<u8>, String>>>,
    visuals: bool,
    flow: Mutex<Option<StartFlow>>,
    pilots: Mutex<Option<PilotSetup>>,
    audio_page: Mutex<Option<AudioPage>>,
    previous: Mutex<Option<PreviousMissionsPage>>,
    plane: Mutex<Option<PlaneNamePage>>,
    audio: Mutex<Option<MenuAudio>>,
    audio_note: Mutex<Option<String>>,
    intro: Mutex<Option<IntroMovie>>,
}

impl FrontEndShellPlugin {
    /// The windowed presenter: sprites draw from `art` (lower-cased art name
    /// → decoded strip/pane) and `cursor`, and `movie_files` (lower-cased
    /// movie file name → its bytes, or the recorded read refusal) feeds the
    /// `M` rows' backdrops.
    #[must_use]
    #[inline(always)]
    pub fn windowed(
        shell: MenuShell,
        art: BTreeMap<String, Artwork>,
        cursor: Option<(String, Artwork)>,
        movie_files: BTreeMap<String, Result<Vec<u8>, String>>,
    ) -> Self {
        Self {
            shell: Mutex::new(Some(shell)),
            art,
            cursor,
            movie_files: Mutex::new(movie_files),
            visuals: true,
            flow: Mutex::new(None),
            pilots: Mutex::new(None),
            audio_page: Mutex::new(None),
            previous: Mutex::new(None),
            plane: Mutex::new(None),
            audio: Mutex::new(None),
            audio_note: Mutex::new(None),
            intro: Mutex::new(None),
        }
    }

    /// Puts the F45 machine in charge of the shell's moves (the start flow).
    #[must_use]
    pub fn with_flow(self, flow: StartFlow) -> Self {
        *self.flow.lock().expect("the plugin is not shared yet") = Some(flow);
        self
    }

    /// Gives the Campaign section its pilot page (FE-MENU-3).
    #[must_use]
    pub fn with_pilots(self, setup: PilotSetup) -> Self {
        *self.pilots.lock().expect("the plugin is not shared yet") = Some(setup);
        self
    }

    /// Gives the Audio section its page (#1996).
    #[must_use]
    pub fn with_audio_page(self, page: AudioPage) -> Self {
        *self
            .audio_page
            .lock()
            .expect("the plugin is not shared yet") = Some(page);
        self
    }

    /// Gives the `PlaneName` section its page (#1958).
    #[must_use]
    pub fn with_plane_name(self, page: PlaneNamePage) -> Self {
        *self.plane.lock().expect("the plugin is not shared yet") = Some(page);
        self
    }

    /// Gives the `ScrapBook_TOC` section its page (#1956).
    #[must_use]
    pub fn with_previous_missions(self, page: PreviousMissionsPage) -> Self {
        *self.previous.lock().expect("the plugin is not shared yet") = Some(page);
        self
    }

    /// Plays `movie` between the pilot page and the cabin
    /// (PARITY5-CABIN-INTRO.03), over the [`MoviePlayerPlugin`] this adds.
    #[must_use]
    pub fn with_intro(self, movie: IntroMovie) -> Self {
        *self.intro.lock().expect("the plugin is not shared yet") = Some(movie);
        self
    }

    /// Gives the presenter its music and cues (FE-MENU-4). `Err` carries why
    /// the run has no output, recorded in the smoke report.
    #[must_use]
    pub fn with_audio(self, audio: Result<MenuAudio, String>) -> Self {
        match audio {
            Ok(audio) => *self.audio.lock().expect("the plugin is not shared yet") = Some(audio),
            Err(note) => {
                *self
                    .audio_note
                    .lock()
                    .expect("the plugin is not shared yet") = Some(note);
            }
        }
        self
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
            movie_files: Mutex::new(BTreeMap::new()),
            visuals: false,
            flow: Mutex::new(None),
            pilots: Mutex::new(None),
            audio_page: Mutex::new(None),
            previous: Mutex::new(None),
            plane: Mutex::new(None),
            audio: Mutex::new(None),
            audio_note: Mutex::new(None),
            intro: Mutex::new(None),
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
            audio_note: self.audio_note.lock().ok().and_then(|mut note| note.take()),
            ..ShellRun::default()
        };
        let mut art = ShellArt::default();
        if self.visuals {
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            for (name, artwork) in &self.art {
                art.images
                    .insert(name.clone(), images.add(artwork_image(artwork)));
                if artwork.is_presented() {
                    art.scales
                        .insert(name.clone(), artwork.presented_texel(1.0, 1.0));
                }
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
            movie_files: self
                .movie_files
                .lock()
                .map(|mut slot| std::mem::take(&mut *slot))
                .unwrap_or_default(),
            visuals: self.visuals,
            scene: ShellScene::default(),
            redraw: true,
            flow: self
                .flow
                .lock()
                .map(|mut slot| slot.take())
                .unwrap_or_default(),
            pilots: self
                .pilots
                .lock()
                .map(|mut slot| slot.take())
                .unwrap_or_default()
                .map(|mut setup| {
                    setup.page.refresh(&setup.session);
                    PilotState {
                        page: setup.page,
                        session: setup.session,
                        outcomes: Vec::new(),
                        scene: Vec::new(),
                        drawn_surface: (0, 0),
                        keys: MessageCursor::default(),
                    }
                }),
            audio: self.audio.lock().ok().and_then(|mut slot| slot.take()),
            watch: AudioWatch::default(),
            intro: IntroState::new(self.intro.lock().ok().and_then(|mut slot| slot.take())),
            notice_scene: (Vec::new(), (0, 0)),
            audio_page: self
                .audio_page
                .lock()
                .ok()
                .and_then(|mut slot| slot.take())
                .map(|page| AudioPageState {
                    page,
                    scene: Vec::new(),
                    drawn_surface: (0, 0),
                    was_on: false,
                    released: None,
                    applied: None,
                }),
            previous: self
                .previous
                .lock()
                .ok()
                .and_then(|mut slot| slot.take())
                .map(|page| PreviousState {
                    page,
                    outcomes: Vec::new(),
                    scene: Vec::new(),
                    drawn_surface: (0, 0),
                }),
            plane: self
                .plane
                .lock()
                .ok()
                .and_then(|mut slot| slot.take())
                .map(|page| PlaneState {
                    page,
                    outcomes: Vec::new(),
                    keys: MessageCursor::default(),
                    scene: Vec::new(),
                    drawn_surface: (0, 0),
                }),
        });
        if app.world().resource::<ShellResource>().intro.enabled()
            && !app.is_plugin_added::<MoviePlayerPlugin>()
        {
            app.add_plugins(MoviePlayerPlugin);
        }
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
    headless_app_flow(shell, None)
}

/// As [`headless_app`], with the F45 machine in charge of the shell's moves
/// when `flow` is given — the route a start-flow drive takes without a
/// renderer (FE-MENU-2's route tests).
pub fn headless_app_flow(shell: MenuShell, flow: Option<StartFlow>) -> App {
    let mut app = App::new();
    app.add_plugins(bevy::input::InputPlugin);
    app.add_message::<CursorMoved>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.init_resource::<ButtonInput<MouseButton>>();
    let plugin = FrontEndShellPlugin::headless(shell);
    app.add_plugins(match flow {
        Some(flow) => plugin.with_flow(flow),
        None => plugin,
    });
    app
}

/// As [`headless_app_flow`], with the Campaign pilot page over `setup`'s
/// profile session (FE-MENU-3's route tests).
pub fn headless_app_pilots(shell: MenuShell, flow: Option<StartFlow>, setup: PilotSetup) -> App {
    let mut app = App::new();
    app.add_plugins(bevy::input::InputPlugin);
    app.add_message::<CursorMoved>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.init_resource::<ButtonInput<MouseButton>>();
    let plugin = FrontEndShellPlugin::headless(shell).with_pilots(setup);
    app.add_plugins(match flow {
        Some(flow) => plugin.with_flow(flow),
        None => plugin,
    });
    app
}

/// As [`headless_app_pilots`], with the front end's music and cues over
/// `audio` (FE-MENU-4's route tests; the device is the test's recording one).
pub fn headless_app_audio(
    shell: MenuShell,
    flow: Option<StartFlow>,
    setup: PilotSetup,
    audio: MenuAudio,
) -> App {
    let mut app = App::new();
    app.add_plugins(bevy::input::InputPlugin);
    app.add_message::<CursorMoved>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.init_resource::<ButtonInput<MouseButton>>();
    let plugin = FrontEndShellPlugin::headless(shell)
        .with_pilots(setup)
        .with_audio(Ok(audio));
    app.add_plugins(match flow {
        Some(flow) => plugin.with_flow(flow),
        None => plugin,
    });
    app
}

/// As [`headless_app_flow`], with the Audio page (and, when given, the menu
/// audio it mixes) — the #1996 route tests.
pub fn headless_app_audio_page(
    shell: MenuShell,
    flow: Option<StartFlow>,
    page: AudioPage,
    audio: Option<MenuAudio>,
) -> App {
    let mut app = App::new();
    app.add_plugins(bevy::input::InputPlugin);
    app.add_message::<CursorMoved>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.init_resource::<ButtonInput<MouseButton>>();
    let mut plugin = FrontEndShellPlugin::headless(shell).with_audio_page(page);
    if let Some(audio) = audio {
        plugin = plugin.with_audio(Ok(audio));
    }
    app.add_plugins(match flow {
        Some(flow) => plugin.with_flow(flow),
        None => plugin,
    });
    app
}

/// As [`headless_app_flow`], with the Plane Name page — the #1958 route
/// tests.
pub fn headless_app_plane_name(
    shell: MenuShell,
    flow: Option<StartFlow>,
    page: PlaneNamePage,
) -> App {
    let mut app = App::new();
    app.add_plugins(bevy::input::InputPlugin);
    app.add_message::<CursorMoved>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.init_resource::<ButtonInput<MouseButton>>();
    let plugin = FrontEndShellPlugin::headless(shell).with_plane_name(page);
    app.add_plugins(match flow {
        Some(flow) => plugin.with_flow(flow),
        None => plugin,
    });
    app
}

/// As [`headless_app_flow`], with the Previous Missions page — the #1956
/// route tests.
pub fn headless_app_previous_missions(
    shell: MenuShell,
    flow: Option<StartFlow>,
    page: PreviousMissionsPage,
) -> App {
    let mut app = App::new();
    app.add_plugins(bevy::input::InputPlugin);
    app.add_message::<CursorMoved>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.init_resource::<ButtonInput<MouseButton>>();
    let plugin = FrontEndShellPlugin::headless(shell).with_previous_missions(page);
    app.add_plugins(match flow {
        Some(flow) => plugin.with_flow(flow),
        None => plugin,
    });
    app
}

/// As [`headless_app_pilots`], with `movie` played between the pilot page
/// and the cabin (PARITY5-CABIN-INTRO.03's route tests). The caller supplies
/// the asset store the movie presenter draws into.
pub fn headless_app_intro(
    shell: MenuShell,
    flow: Option<StartFlow>,
    setup: PilotSetup,
    movie: IntroMovie,
) -> App {
    let mut app = App::new();
    app.add_plugins(bevy::input::InputPlugin);
    app.add_message::<CursorMoved>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.init_resource::<ButtonInput<MouseButton>>();
    let plugin = FrontEndShellPlugin::headless(shell)
        .with_pilots(setup)
        .with_intro(movie);
    app.add_plugins(match flow {
        Some(flow) => plugin.with_flow(flow),
        None => plugin,
    });
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
    // The music control is initialised by the shared script when the front
    // end starts (`GLOBALS.SCRIPT` `gui_create`), not by any one screen.
    let intro_busy = state.intro.advance(world);
    if let Some(audio) = &mut state.audio
        && !state.intro.active()
    {
        audio.start_music();
    }
    let mut events = std::mem::take(&mut state.input.events);
    let on_audio = state.audio_page.is_some()
        && state
            .shell
            .section_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(AUDIO_SECTION));
    let on_page = state.pilots.is_some()
        && state
            .shell
            .section_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(PILOT_SECTION));
    let on_toc = state.previous.is_some()
        && state
            .shell
            .section_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(TOC_SECTION));
    let on_name = state.plane.is_some()
        && state
            .shell
            .section_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(NAME_SECTION));
    let notice_up = state
        .flow
        .as_ref()
        .is_some_and(|flow| flow.notice().is_some());
    if let Some(plane) = &mut state.plane {
        // As the pilot page: typed text reaches the edit box only while the
        // page is on screen and no notice is over it.
        let typed: Vec<KeyboardInput> = plane
            .keys
            .read(world.resource::<Messages<KeyboardInput>>())
            .filter(|event| event.state == ButtonState::Pressed)
            .cloned()
            .collect();
        if on_name && !notice_up {
            for event in typed {
                if event.logical_key == Key::Backspace {
                    plane.page.backspace();
                } else if let Some(text) = &event.text {
                    plane.page.type_text(text);
                }
            }
        }
    }
    if let Some(pilots) = &mut state.pilots {
        // Typed text reaches the page only while it is on screen; the
        // reader advances either way so keys typed elsewhere never replay.
        let typed: Vec<KeyboardInput> = pilots
            .keys
            .read(world.resource::<Messages<KeyboardInput>>())
            .filter(|event| event.state == ButtonState::Pressed)
            .cloned()
            .collect();
        if on_page {
            for event in typed {
                if event.logical_key == Key::Backspace {
                    pilots.page.backspace();
                } else if let Some(text) = &event.text {
                    let before = pilots.page.text().chars().count();
                    pilots.page.type_text(text);
                    let printable = text.chars().any(|c| !c.is_control());
                    if let Some(audio) = &mut state.audio
                        && printable
                    {
                        audio.cue(if pilots.page.text().chars().count() > before {
                            MenuCue::TextAccepted
                        } else {
                            MenuCue::TextRejected
                        });
                    }
                }
            }
        }
    }
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
        if keys.just_pressed(KeyCode::ArrowLeft) {
            events.push(ShellEvent::Left);
        }
        if keys.just_pressed(KeyCode::ArrowRight) {
            events.push(ShellEvent::Right);
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
    let flow = &mut state.flow;
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
    let point = state.input.cursor.and_then(|position| {
        (position.x >= 0.0 && position.y >= 0.0)
            .then(|| MenuShell::unmap_point(state.surface, position.x as u32, position.y as u32))
            .flatten()
    });
    let pilots = &mut state.pilots;
    if on_page && let Some(pilots) = pilots.as_mut() {
        pilots.page.hover_at(point);
    }
    let previous = &mut state.previous;
    let plane = &mut state.plane;
    if on_toc && let Some(previous) = previous.as_mut() {
        previous.page.hover_at(point);
    }
    let audio_page = &mut state.audio_page;
    if on_audio
        && let Some(page) = audio_page.as_mut()
        && page.page.dragging()
        && let Some((x, _)) = point
    {
        // A held slider follows the pointer, wherever on the surface it is.
        page.page.drag_to(x);
    }
    let mut exit = false;
    if intro_busy {
        // The movie owns the input: its skip keys must not reach the cabin.
        events.clear();
    }
    if let Some(flow) = flow.as_mut()
        && flow.notice().is_some()
    {
        // The not-implemented screen owns the input: Esc, Return or a click
        // closes it, everything else is ignored, and none reaches the cabin.
        if events.iter().any(|event| {
            matches!(
                event,
                ShellEvent::Escape | ShellEvent::ActivateDown | ShellEvent::PointerDown
            )
        }) {
            flow.dismiss_notice();
        }
        events.clear();
    }
    for event in events.drain(..) {
        // The pilot page takes the arrows, Enter and Esc for itself (the
        // edit box has the focus; the list is arrow-driven) and a press on
        // a list row; Continue and Delete arrive below as button presses.
        let mut pilot_step = None;
        let outcome = match event {
            ShellEvent::PointerDown => {
                let audio_hit = on_audio
                    && match (audio_page.as_mut(), point) {
                        (Some(audio), Some((x, y))) => {
                            audio.page.press_dropdown(x, y) || audio.page.begin_drag(x, y)
                        }
                        _ => false,
                    };
                let row = pilots
                    .as_ref()
                    .filter(|_| on_page)
                    .and_then(|pilots| point.and_then(|(x, y)| pilots.page.row_at(x, y)));
                let toc_row = previous
                    .as_ref()
                    .filter(|_| on_toc)
                    .and_then(|previous| point.and_then(|(x, y)| previous.page.row_at(x, y)));
                if audio_hit {
                    // The Audio page's slider or drop-down took the press.
                } else if let (Some(previous), Some(row)) = (previous.as_mut(), toc_row) {
                    previous.page.select_row(row);
                } else if let (Some(pilots), Some(row)) = (pilots.as_mut(), row) {
                    pilots.page.select_row(row);
                } else {
                    shell.pointer_down();
                }
                ShellOutcome::Nothing
            }
            ShellEvent::PointerUp => {
                if on_audio && let Some(audio) = audio_page.as_mut() {
                    audio.page.end_drag();
                    audio.released = shell.pressed_id().map(str::to_owned);
                }
                shell.pointer_up()
            }
            ShellEvent::FocusNext | ShellEvent::FocusPrev if on_audio => {
                if let Some(audio) = audio_page.as_mut() {
                    let control = audio.page.move_focus(event == ShellEvent::FocusNext);
                    shell.set_focus_id(control.button());
                    state.run.focus_moves += 1;
                }
                ShellOutcome::Nothing
            }
            ShellEvent::Left | ShellEvent::Right => {
                if on_audio && let Some(audio) = audio_page.as_mut() {
                    audio
                        .page
                        .adjust(if event == ShellEvent::Right { 1 } else { -1 });
                }
                ShellOutcome::Nothing
            }
            ShellEvent::FocusNext | ShellEvent::FocusPrev if on_page => {
                if let Some(pilots) = pilots.as_mut() {
                    pilots.page.move_selection(event == ShellEvent::FocusNext);
                }
                ShellOutcome::Nothing
            }
            ShellEvent::FocusNext | ShellEvent::FocusPrev if on_toc => {
                if let Some(previous) = previous.as_mut() {
                    previous.page.move_selection(event == ShellEvent::FocusNext);
                }
                ShellOutcome::Nothing
            }
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
            ShellEvent::ActivateDown if on_name => {
                if let Some(plane) = plane.as_mut() {
                    let done = plane.page.press_enter();
                    pilot_step = name_step(plane, done, flow, shell);
                }
                ShellOutcome::Nothing
            }
            ShellEvent::ActivateUp if on_name => ShellOutcome::Nothing,
            ShellEvent::Escape if on_name => {
                if let Some(plane) = plane.as_mut() {
                    let done = plane.page.escape();
                    pilot_step = name_step(plane, done, flow, shell);
                }
                ShellOutcome::Nothing
            }
            ShellEvent::ActivateDown if on_page => {
                if let Some(pilots) = pilots.as_mut() {
                    pilot_step = Some(pilot_continue(pilots, flow, shell));
                }
                ShellOutcome::Nothing
            }
            ShellEvent::ActivateUp if on_page => ShellOutcome::Nothing,
            ShellEvent::ActivateDown => {
                let page_took_it =
                    on_audio && audio_page.as_mut().is_some_and(|a| a.page.activate());
                if !page_took_it {
                    shell.activate_down();
                }
                ShellOutcome::Nothing
            }
            ShellEvent::ActivateUp => {
                if on_audio && let Some(audio) = audio_page.as_mut() {
                    audio.released = shell.pressed_id().map(str::to_owned);
                }
                shell.activate_up()
            }
            ShellEvent::Escape
                if on_audio && audio_page.as_mut().is_some_and(|a| a.page.close_list()) =>
            {
                ShellOutcome::Nothing
            }
            ShellEvent::Escape
                if on_page && pilots.as_mut().is_some_and(|pilots| pilots.page.escape()) =>
            {
                ShellOutcome::Nothing
            }
            ShellEvent::Escape => match flow {
                // The machine decides where Esc is bound (Credits only).
                Some(flow) => {
                    if matches!(flow.route_escape(shell), StartStep::Moved { .. }) {
                        state
                            .run
                            .visited
                            .push(shell.section_name().unwrap_or_default().to_owned());
                        state.redraw = true;
                    }
                    ShellOutcome::Nothing
                }
                None => shell.escape(),
            },
        };
        if let ShellOutcome::Toggled { id, checked } = &outcome {
            if on_name
                && id.eq_ignore_ascii_case(super::plane_name::DEFAULT_BUTTON)
                && let Some(plane) = plane.as_mut()
            {
                plane.page.set_default_loadout(*checked);
            }
            if on_audio
                && id.eq_ignore_ascii_case(MUSIC_BUTTON)
                && let Some(audio) = audio_page.as_mut()
            {
                audio.page.set_music_on(*checked);
                audio.page.set_focus(Some(AudioControl::Music));
            }
            continue;
        }
        if pilot_step.is_none()
            && on_name
            && let ShellOutcome::Unhandled { id, .. } = &outcome
            && let Some(plane) = plane.as_mut()
        {
            let done = plane.page.press(id);
            pilot_step = name_step(plane, done, flow, shell);
        }
        if pilot_step.is_none()
            && on_toc
            && let ShellOutcome::Unhandled { id, .. } = &outcome
            && let Some(previous) = previous.as_mut()
        {
            let pressed = previous.page.press(id);
            if pressed != TocOutcome::Nothing {
                previous.outcomes.push(pressed.clone());
            }
            pilot_step = match pressed {
                TocOutcome::JournalNotBuilt { button } => flow.as_mut().map(|flow| {
                    flow.show_not_implemented(NotImplemented {
                        section: "ScrapBook",
                        button,
                        what: "the Scrapbook journal",
                    })
                }),
                TocOutcome::Replay { mission } => {
                    eprintln!(
                        "cs: replaying mission {mission} needs the per-pilot progress store, \
                         which is not built"
                    );
                    Some(StartStep::Stay)
                }
                TocOutcome::ReplayOff | TocOutcome::Nothing => None,
            };
        }
        if pilot_step.is_none()
            && on_page
            && let ShellOutcome::Unhandled { id, .. } = &outcome
            && let Some(pilots) = pilots.as_mut()
        {
            if id.eq_ignore_ascii_case(CONTINUE_BUTTON) {
                pilot_step = Some(pilot_continue(pilots, flow, shell));
            } else if id.eq_ignore_ascii_case(DELETE_BUTTON) {
                let done = pilots.page.press_delete(&mut pilots.session);
                if done != PilotOutcome::Nothing {
                    pilots.outcomes.push(done);
                }
                pilot_step = Some(StartStep::Stay);
            }
        }
        if flow.is_some() || pilot_step.is_some() {
            let step = match (pilot_step, flow.as_mut()) {
                (Some(step), _) => step,
                (None, Some(flow)) => flow.route(shell, outcome.clone()),
                (None, None) => StartStep::Stay,
            };
            let plays_intro = intro::follows(&step);
            match step {
                StartStep::Moved { .. } => {
                    state
                        .run
                        .visited
                        .push(shell.section_name().unwrap_or_default().to_owned());
                    state.redraw = true;
                    if plays_intro && state.intro.begin(state.audio.as_mut()) {
                        break;
                    }
                }
                StartStep::NotImplemented(entry) => {
                    eprintln!("cs: {} is not implemented", entry.what);
                }
                StartStep::Memento => {
                    state.redraw = true;
                    let now = shell.section_name().unwrap_or_default();
                    if state.run.visited.last().map(String::as_str) != Some(now) {
                        state.run.visited.push(now.to_owned());
                    }
                }
                StartStep::Launch { .. } | StartStep::Quit => exit = true,
                StartStep::Refused(refusal) => eprintln!("cs: {refusal}"),
                StartStep::Stay => {}
            }
            continue;
        }
        match outcome {
            ShellOutcome::Entered { section }
            | ShellOutcome::Returned { section, .. }
            | ShellOutcome::Back { section } => {
                state.run.visited.push(section);
                state.redraw = true;
            }
            ShellOutcome::Unhandled { .. } => state.run.unhandled += 1,
            ShellOutcome::Quit => {
                if in_smoke {
                    state.run.quit_seen = true;
                } else {
                    exit = true;
                }
            }
            ShellOutcome::Nothing | ShellOutcome::Toggled { .. } => {}
        }
    }
    if let Some(flow) = flow.as_ref()
        && flow.sync_memento(shell)
    {
        state.redraw = true;
    }
    if exit {
        // A Launch step exits through the same AppExit a Quit does; the
        // mission it named is what the caller's `hand_over` needs, and the
        // world it lived in is the one `App::run` is about to take away.
        if let Some(cell) = world.get_resource::<HandoverCell>()
            && let Some(mission) = flow.as_ref().and_then(StartFlow::handover)
        {
            *cell.0.lock().unwrap_or_else(|poison| poison.into_inner()) = Some(mission.to_owned());
        }
        world
            .resource_mut::<Messages<AppExit>>()
            .write(AppExit::Success);
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
    watch_buttons(&mut state);
    sync_audio_page(&mut state);
    let cursor_extent = state.art.cursor.as_ref().map(|(_, extent)| *extent);
    let now = world
        .get_resource::<Time>()
        .map(Time::elapsed)
        .unwrap_or_default();
    if let Some(view) = state.shell.view(state.surface) {
        if state.redraw {
            state.redraw = false;
            for entity in state
                .scene
                .panes
                .drain(..)
                .chain(state.scene.buttons.drain(..))
                .map(|drawn| drawn.entity)
                .chain(
                    state
                        .scene
                        .movies
                        .drain(..)
                        .map(|sprite| sprite.entity)
                        .collect::<Vec<_>>(),
                )
            {
                let _ = world.despawn(entity);
            }
            if state.visuals {
                spawn_movies(world, &mut state, &view, now);
            }
            let art = std::mem::take(&mut state.art);
            spawn_section(world, &art, state.surface, &view, &mut state.scene);
            state.art = art;
        }
        for sprite in &mut state.scene.movies {
            if let Ok(mut backdrop) = sprite.backdrop.lock()
                && backdrop.update(now)
            {
                let pixels = backdrop.pixels().to_vec();
                drop(backdrop);
                if let Some(mut image) =
                    world.resource_mut::<Assets<Image>>().get_mut(&sprite.image)
                {
                    image.data = Some(pixels);
                    state.run.movie_frames += 1;
                }
            }
            place_movie(world, sprite, &view, state.surface);
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
                if let Some(placement) = pane_placement(pane, &view.fit, state.surface, drawn.z)
                    && let Ok(mut entity) = world.get_entity_mut(drawn.entity)
                {
                    if let Some(mut sprite) = entity.get_mut::<Sprite>() {
                        sprite.custom_size = Some(placement.size);
                    }
                    if let Some(mut transform) = entity.get_mut::<Transform>() {
                        *transform = placement.transform;
                    }
                }
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
    sync_pilot_scene(world, &mut state);
    sync_previous_scene(world, &mut state);
    sync_plane_scene(world, &mut state);
    sync_audio_scene(world, &mut state);
    sync_notice_scene(world, &mut state);
    if let Some(cursor) = state.scene.cursor
        && let Ok(mut entity) = world.get_entity_mut(cursor)
    {
        match (state.input.cursor, cursor_extent) {
            (Some(position), Some(extent)) => {
                let (size, translation) = cursor_placement(position, extent, state.surface);
                if let Some(mut visibility) = entity.get_mut::<Visibility>() {
                    *visibility = Visibility::Visible;
                }
                if let Some(mut sprite) = entity.get_mut::<Sprite>() {
                    sprite.custom_size = Some(size);
                }
                if let Some(mut transform) = entity.get_mut::<Transform>() {
                    transform.translation.x = translation.x;
                    transform.translation.y = translation.y;
                }
            }
            _ => {
                if let Some(mut visibility) = entity.get_mut::<Visibility>() {
                    *visibility = Visibility::Hidden;
                }
            }
        }
    }
    // The movie's sprite sits at z 0 under the section's panes (the movie
    // camera draws every sprite), so the screen is hidden while the intro
    // plays and shown again on the frame it ends.
    if state.intro.active() || intro_busy {
        let shown = if state.intro.active() {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
        let scene = &state.scene;
        let covered = scene
            .panes
            .iter()
            .chain(&scene.buttons)
            .map(|sprite| sprite.entity)
            .chain(scene.cursor);
        for entity in covered {
            if let Some(mut visibility) = world.get_mut::<Visibility>(entity) {
                *visibility = shown;
            }
        }
    }
    if let Some(mut smoke) = world.remove_resource::<ShellSmoke>() {
        step_smoke(world, &state, &mut smoke);
        world.insert_resource(smoke);
    }
    world.insert_resource(state);
}

/// Plays the rollover cue when a button newly takes the pointer or focus and
/// the press cue when one is newly held down.
///
/// Read from the shell's own button states after the frame's input: which
/// control class plays which declared sound is the executable's, so the cues
/// follow the visible states (see [`super::sound`]).
fn watch_buttons(state: &mut ShellResource) {
    let Some(section) = state.shell.section() else {
        return;
    };
    let name = state.shell.section_name().unwrap_or_default().to_owned();
    let mut target = None;
    let mut depressed = false;
    for index in 0..section.buttons.len() {
        match state.shell.visual(index) {
            ButtonVisual::Rollover => target = Some((name.clone(), index)),
            ButtonVisual::Depressed => {
                target = Some((name.clone(), index));
                depressed = true;
            }
            ButtonVisual::Disabled | ButtonVisual::Active => {}
        }
    }
    let moved_screen = state.watch.section.as_deref() != Some(name.as_str());
    let entered = target.is_some() && target != state.watch.target && !moved_screen;
    let pressed = depressed && !state.watch.depressed;
    state.watch.section = Some(name);
    state.watch.target = target;
    state.watch.depressed = depressed;
    if let Some(audio) = &mut state.audio {
        if entered {
            audio.cue(MenuCue::Rollover);
        }
        if pressed {
            audio.cue(MenuCue::Press);
        }
    }
}

/// Continue (the button or Enter) on the pilot page: creates or chooses the
/// pilot, and with the F45 machine in charge takes its `NewProfile` /
/// `ContinueProfile` then `ConfirmProfile` rows to the cabin.
fn pilot_continue(
    pilots: &mut PilotState,
    flow: &mut Option<StartFlow>,
    shell: &mut MenuShell,
) -> StartStep {
    let outcome = pilots.page.press_continue(&mut pilots.session);
    let created = match &outcome {
        PilotOutcome::Continued { created, .. } => Some(*created),
        _ => None,
    };
    if outcome != PilotOutcome::Nothing {
        pilots.outcomes.push(outcome);
    }
    let (Some(created), Some(flow)) = (created, flow.as_mut()) else {
        return StartStep::Stay;
    };
    let step = flow.choose_pilot(created);
    if matches!(step, StartStep::Moved { .. })
        && let Some(section) = flow.section()
    {
        let _ = shell.open(section);
    }
    step
}

/// What the flow does with a Plane Name page outcome (#1958): Cancel is the
/// table's `Cancel` row back to the cabin, OK with a name the way on to the
/// construction screen — which is not built, so the marked notice is up
/// over the dialog — and an empty name stays.
fn name_step(
    plane: &mut PlaneState,
    done: NameOutcome,
    flow: &mut Option<StartFlow>,
    shell: &mut MenuShell,
) -> Option<StartStep> {
    if done == NameOutcome::Nothing {
        return None;
    }
    plane.outcomes.push(done.clone());
    match done {
        NameOutcome::Nothing => None,
        NameOutcome::NameRequired => Some(StartStep::Stay),
        NameOutcome::Cancel => match flow.as_mut() {
            Some(flow) => {
                let step = flow.press_machine(super::Action::Cancel);
                if matches!(step, StartStep::Moved { .. })
                    && let Some(section) = flow.section()
                {
                    let _ = shell.open(section);
                }
                Some(step)
            }
            None => {
                let _ = shell.escape();
                Some(StartStep::Stay)
            }
        },
        NameOutcome::Construct { .. } => flow
            .as_mut()
            .map(|flow| flow.show_not_implemented(CONSTRUCTION_NOT_BUILT)),
    }
}

/// Keeps the Plane Name page's overlay in step with its model: drawn while the
/// `PlaneName` section is shown, redrawn when the model or the surface
/// changed, gone otherwise.
fn sync_plane_scene(world: &mut World, state: &mut ShellResource) {
    if !state.visuals {
        return;
    }
    let active = state
        .shell
        .section_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(NAME_SECTION));
    let surface = state.surface;
    let buttons: Vec<(String, super::ShellRect)> = state
        .shell
        .section()
        .map(|section| {
            section
                .buttons
                .iter()
                .map(|button| (button.id.to_ascii_uppercase(), button.rect))
                .collect()
        })
        .unwrap_or_default();
    let Some(plane) = state.plane.as_mut() else {
        return;
    };
    if !active {
        for entity in plane.scene.drain(..) {
            let _ = world.despawn(entity);
        }
        return;
    }
    let dirty = plane.page.take_dirty();
    if plane.scene.is_empty() || dirty || plane.drawn_surface != surface {
        for entity in plane.scene.drain(..) {
            let _ = world.despawn(entity);
        }
        plane.scene = super::plane_name_view::spawn_page(world, &plane.page, &buttons, surface);
        plane.drawn_surface = surface;
    }
}

/// Keeps the not-implemented notice drawn while the flow has one up.
fn sync_notice_scene(world: &mut World, state: &mut ShellResource) {
    if !state.visuals {
        return;
    }
    let entry = state.flow.as_ref().and_then(|flow| flow.notice()).copied();
    let surface = state.surface;
    if entry.is_none() || state.notice_scene.1 != surface {
        for entity in state.notice_scene.0.drain(..) {
            let _ = world.despawn(entity);
        }
    }
    if let Some(entry) = entry
        && state.notice_scene.0.is_empty()
    {
        state.notice_scene = (
            super::notice_view::spawn_notice(world, &entry, surface),
            surface,
        );
    }
}

/// The Audio page's per-frame bookkeeping (#1996): leaving the section
/// commits (the release was on Accept) or discards, the check box mirrors the
/// staged value, and the menu audio plays the mix on screen — the staged
/// values while the page is open (`AUDIO.SCRIPT` applies them live), the
/// committed ones otherwise (Cancel puts them back).
fn sync_audio_page(state: &mut ShellResource) {
    let on = state
        .shell
        .section_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(AUDIO_SECTION));
    let Some(audio) = state.audio_page.as_mut() else {
        return;
    };
    if audio.was_on && !on {
        if audio
            .released
            .as_deref()
            .is_some_and(|id| id.eq_ignore_ascii_case(ACCEPT_BUTTON))
        {
            let accepted = audio.page.accept();
            if let Some(error) = &accepted.save_error {
                eprintln!("cs: the audio settings were not saved: {error}");
            }
        } else {
            audio.page.cancel();
        }
        audio.released = None;
    }
    if on && !audio.was_on {
        // Every visit starts from what is committed.
        audio.page.cancel();
    }
    audio.was_on = on;
    if on {
        let staged = audio.page.staged().music_on;
        if state.shell.is_checked(MUSIC_BUTTON) != Some(staged) {
            let _ = state.shell.set_checked(MUSIC_BUTTON, staged);
        }
    }
    let values = if on {
        audio.page.staged()
    } else {
        audio.page.committed()
    };
    let mix = MenuMix {
        music_on: values.music_on,
        music_gain: values.music_gain(),
        effects_gain: values.effects_gain(),
    };
    if audio.applied != Some(mix) {
        if let Some(menu) = &mut state.audio {
            menu.set_mix(mix);
        }
        audio.applied = Some(mix);
    }
}

/// Keeps the Audio page's overlay in step with its model: drawn while the
/// `Audio` section is shown, redrawn when the model or the surface changed.
fn sync_audio_scene(world: &mut World, state: &mut ShellResource) {
    if !state.visuals {
        return;
    }
    let active = state
        .shell
        .section_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(AUDIO_SECTION));
    let surface = state.surface;
    let Some(audio) = state.audio_page.as_mut() else {
        return;
    };
    if !active {
        for entity in audio.scene.drain(..) {
            let _ = world.despawn(entity);
        }
        return;
    }
    let dirty = audio.page.take_dirty();
    if audio.scene.is_empty() || dirty || audio.drawn_surface != surface {
        for entity in audio.scene.drain(..) {
            let _ = world.despawn(entity);
        }
        audio.scene = super::audio_view::spawn_page(world, &audio.page, &state.art.images, surface);
        audio.drawn_surface = surface;
    }
}

/// Keeps the Previous Missions page's overlay in step with its model: drawn
/// while the `ScrapBook_TOC` section is shown, redrawn when the model or the
/// surface changed, gone otherwise.
fn sync_previous_scene(world: &mut World, state: &mut ShellResource) {
    if !state.visuals {
        return;
    }
    let active = state
        .shell
        .section_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(TOC_SECTION));
    let surface = state.surface;
    let buttons: Vec<(String, super::ShellRect)> = state
        .shell
        .section()
        .map(|section| {
            section
                .buttons
                .iter()
                .map(|button| (button.id.to_ascii_uppercase(), button.rect))
                .collect()
        })
        .unwrap_or_default();
    let Some(previous) = state.previous.as_mut() else {
        return;
    };
    if !active {
        for entity in previous.scene.drain(..) {
            let _ = world.despawn(entity);
        }
        return;
    }
    let dirty = previous.page.take_dirty();
    if previous.scene.is_empty() || dirty || previous.drawn_surface != surface {
        for entity in previous.scene.drain(..) {
            let _ = world.despawn(entity);
        }
        previous.scene = super::previous_missions_view::spawn_page(
            world,
            &previous.page,
            &state.art.images,
            &buttons,
            surface,
        );
        previous.drawn_surface = surface;
    }
}

/// Keeps the pilot page's overlay in step with the model: drawn while the
/// `Campaign` section is shown, redrawn when the model or the surface
/// changed, gone otherwise.
fn sync_pilot_scene(world: &mut World, state: &mut ShellResource) {
    if !state.visuals {
        return;
    }
    let active = state
        .shell
        .section_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(PILOT_SECTION));
    let surface = state.surface;
    let Some(pilots) = state.pilots.as_mut() else {
        return;
    };
    if !active {
        for entity in pilots.scene.drain(..) {
            let _ = world.despawn(entity);
        }
        return;
    }
    let dirty = pilots.page.take_dirty();
    if pilots.scene.is_empty() || dirty || pilots.drawn_surface != surface {
        for entity in pilots.scene.drain(..) {
            let _ = world.despawn(entity);
        }
        pilots.scene = super::pilot_view::spawn_page(world, &pilots.page, surface);
        pilots.drawn_surface = surface;
    }
}

/// The scale the aspect-fit applies: fitted width over logical width.
#[inline(always)]
fn fit_scale(surface: (u32, u32)) -> f32 {
    cs_content::ui_layout::AspectFit::new(SECTION_SURFACE, surface)
        .map(|fit| fit.image_rect().width as f32 / SECTION_SURFACE.0 as f32)
        .unwrap_or(1.0)
}

/// Opens and spawns the current section's `M` rows, each a [`MovieBackdrop`]
/// feeding one texture, drawn under the panes in declaration order. A movie
/// whose bytes were never read or whose stream refuses is recorded on
/// `run.movies` and the section still draws — the backdrop is never faked.
#[inline(always)]
fn spawn_movies(world: &mut World, state: &mut ShellResource, view: &ShellView, now: Duration) {
    let Some(section) = state.shell.section() else {
        return;
    };
    let section_name = section.name.clone();
    for (index, movie) in section.movies.iter().enumerate() {
        let key = movie.file.to_ascii_lowercase();
        let read = state
            .movie_files
            .get(&key)
            .cloned()
            .unwrap_or_else(|| Err("the file was never read at open".to_owned()));
        let mut record = |detail: String| {
            state
                .run
                .movies
                .push(format!("MOVIE {} on {section_name}: {detail}", movie.id));
        };
        let bytes = match read {
            Ok(bytes) => bytes,
            Err(reason) => {
                record(format!("refused ({reason})"));
                continue;
            }
        };
        let media = movie_media(&movie.file);
        let open: RetailMovieOpen =
            Box::new(move || MpegVideoSource::new(Cursor::new(bytes.clone()), &media));
        let mut backdrop = match MovieBackdrop::open(movie, open, now) {
            Ok(backdrop) => backdrop,
            Err(failure) => {
                record(format!("refused ({failure})"));
                continue;
            }
        };
        // The first decoded frame lands before the sprite exists, so the
        // texture never shows a black first frame.
        let _ = backdrop.update(now);
        let rect = backdrop.rect();
        let visible = rect.clip(SECTION_SURFACE).is_some();
        record(format!(
            "{} {}x{} @{:.2} fps, loops {}{}",
            movie.file,
            backdrop.extent().0,
            backdrop.extent().1,
            backdrop.frame_rate(),
            if movie.loops == 0 {
                "continuous".to_owned()
            } else {
                movie.loops.to_string()
            },
            if visible {
                ""
            } else {
                ", off-surface (not drawn)"
            }
        ));
        if !visible {
            continue;
        }
        let scale = (movie.scale_x, movie.scale_y);
        let pixels = if backdrop.pixels().is_empty() {
            vec![0; backdrop.extent().0 as usize * backdrop.extent().1 as usize * 4]
        } else {
            backdrop.pixels().to_vec()
        };
        let image = world
            .resource_mut::<Assets<Image>>()
            .add(movie_image(backdrop.extent(), pixels));
        let z = 0.5 + index as f32 * 0.01;
        let entity = world
            .spawn((
                Sprite {
                    image: image.clone(),
                    ..default()
                },
                Transform::from_xyz(0.0, 0.0, z),
            ))
            .id();
        let sprite = MovieSprite {
            z,
            backdrop: Mutex::new(backdrop),
            rect,
            scale,
            image,
            entity,
        };
        place_movie(world, &sprite, view, state.surface);
        state.scene.movies.push(sprite);
    }
}

/// The movie texture: the decoded extent and the current frame's pixels.
#[inline(always)]
fn movie_image(extent: (u32, u32), pixels: Vec<u8>) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: extent.0,
            height: extent.1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::linear();
    image.texture_descriptor.usage |= TextureUsages::TEXTURE_BINDING;
    image
}

/// Maps one movie's logical rect to its sprite, like [`place`] but with the
/// texel region the row's `ScaleX`/`ScaleY` implies: one drawn logical pixel
/// is `scale/100` video texels.
#[inline(always)]
fn place_movie(world: &mut World, sprite: &MovieSprite, view: &ShellView, surface: (u32, u32)) {
    let Some((visible, cut)) = sprite.rect.clip(SECTION_SURFACE) else {
        return;
    };
    let rect = view.fit.map_rect(visible);
    // Video texels of the visible region: the row's `ScaleX`/`ScaleY` are
    // the *drawn* size as a percent of the video, so a drawn logical pixel
    // is `100/scale` video texels — texels = logical * 100 / scale.
    let texel = |drawn: u32, scale: u32| drawn.saturating_mul(100).checked_div(scale).unwrap_or(0);
    let Ok(mut entity) = world.get_entity_mut(sprite.entity) else {
        return;
    };
    if let Some(mut draw) = entity.get_mut::<Sprite>() {
        draw.rect = Some(bevy::math::Rect::new(
            texel(cut.0, sprite.scale.0) as f32,
            texel(cut.1, sprite.scale.1) as f32,
            texel(cut.0.saturating_add(visible.width), sprite.scale.0) as f32,
            texel(cut.1.saturating_add(visible.height), sprite.scale.1) as f32,
        ));
        draw.custom_size = Some(rect_size(rect));
    }
    if let Some(mut transform) = entity.get_mut::<Transform>() {
        transform.translation = centre(rect, surface, sprite.z);
    }
}

/// The cursor sprite: anchored at its top-left corner, where the arrow's tip
/// is, so the sprite's origin is the pointer position the hit tests use.
/// (`Sprite` alone anchors at the image centre.)
#[inline(always)]
fn cursor_bundle(image: Handle<Image>) -> (Sprite, Anchor, Transform) {
    (
        Sprite { image, ..default() },
        Anchor::TOP_LEFT,
        Transform::from_xyz(0.0, 0.0, 100.0),
    )
}

/// The cursor sprite's size and the world position of its top-left corner for
/// a pointer at `position` (window pixels, y down) over a `surface`-sized
/// window: the arrow's tip lands exactly on the pointer, the art scaled by the
/// same aspect fit as the sections.
#[inline(always)]
fn cursor_placement(position: Vec2, extent: (u32, u32), surface: (u32, u32)) -> (Vec2, Vec2) {
    let scale = fit_scale(surface);
    (
        Vec2::new(extent.0 as f32 * scale, extent.1 as f32 * scale),
        Vec2::new(
            position.x - surface.0 as f32 * 0.5,
            surface.1 as f32 * 0.5 - position.y,
        ),
    )
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
        let name = pane.art.to_ascii_lowercase();
        let Some(image) = art.images.get(&name) else {
            continue;
        };
        let scale = art.scales.get(&name).copied().unwrap_or((1.0, 1.0));
        let z = 1.0 + order as f32 * 0.01;
        let placement = pane_placement(pane, &view.fit, surface, z);
        let entity = world
            .spawn((
                Sprite {
                    image: image.clone(),
                    rect: Some(texel_rect(pane.texel_offset, pane.logical, scale)),
                    custom_size: Some(placement.map_or(rect_size(pane.rect), |p| p.size)),
                    ..default()
                },
                placement.map_or_else(
                    || Transform::from_translation(centre(pane.rect, surface, z)),
                    |p| p.transform,
                ),
            ))
            .id();
        scene.panes.push(DrawnSprite {
            index: pane.index,
            z,
            entity,
            scale,
        });
    }
    for button in &view.buttons {
        let name = button.art.to_ascii_lowercase();
        let Some(image) = art.images.get(&name) else {
            continue;
        };
        let scale = art.scales.get(&name).copied().unwrap_or((1.0, 1.0));
        let z = 10.0 + button.index as f32 * 0.01;
        let entity = world
            .spawn((
                Sprite {
                    image: image.clone(),
                    rect: Some(texel_rect(button.texel_offset, button.logical, scale)),
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
            scale,
        });
    }
    if scene.cursor.is_none()
        && let Some((image, _)) = &art.cursor
    {
        // `(Sprite, Transform)` like the panes and buttons — `Visibility`
        // arrives as a required component — so one bundle type serves every
        // sprite the presenter spawns.
        let cursor = world.spawn(cursor_bundle(image.clone())).id();
        if let Some(mut visibility) = world.get_mut::<Visibility>(cursor) {
            *visibility = Visibility::Hidden;
        }
        scene.cursor = Some(cursor);
    }
}

/// The texture sub-rect a drawn element shows: `texel_offset` in the art plus
/// the element's own (clipped) logical size.
#[inline(always)]
fn texel_rect(
    offset: (u32, u32),
    logical: cs_content::ui_layout::Rect,
    scale: (f32, f32),
) -> bevy::math::Rect {
    bevy::math::Rect::new(
        offset.0 as f32 * scale.0,
        offset.1 as f32 * scale.1,
        (offset.0 as f32 + logical.width as f32) * scale.0,
        (offset.1 as f32 + logical.height as f32) * scale.1,
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

/// Where a pane with a script transform lands: its sprite's size and
/// transform (the sprite turns about its centre).
#[derive(Clone, Copy)]
struct PanePlacement {
    size: Vec2,
    transform: Transform,
}

/// Places a transformed pane (`None` for a plain one, or when the engine's
/// x/y rotation, which is not decoded, is set). The pane's `logical` box is
/// the picture at the `render` position; the corners come from
/// [`PaneTransform::corners`], the box centre is the parallelogram's, and the
/// sprite is turned by the z rotation (counter-clockwise on screen, y up here).
#[inline(always)]
fn pane_placement(
    pane: &super::shell::ShellPaneView,
    fit: &cs_content::ui_layout::AspectFit,
    surface: (u32, u32),
    z: f32,
) -> Option<PanePlacement> {
    let transform = pane.transform?;
    let size = (pane.logical.width, pane.logical.height);
    let corners = transform.corners(size, (pane.logical.x as f32, pane.logical.y as f32))?;
    let image = fit.image_rect();
    let scale = image.width as f32 / SECTION_SURFACE.0 as f32;
    let (sx, sy) = transform.scale_factors();
    let centre_x = (corners[0].0 + corners[3].0) * 0.5;
    let centre_y = (corners[0].1 + corners[3].1) * 0.5;
    Some(PanePlacement {
        size: Vec2::new(size.0 as f32 * sx * scale, size.1 as f32 * sy * scale),
        transform: Transform {
            translation: bevy::math::Vec3::new(
                image.x as f32 + centre_x * scale - surface.0 as f32 * 0.5,
                surface.1 as f32 * 0.5 - (image.y as f32 + centre_y * scale),
                z,
            ),
            rotation: bevy::math::Quat::from_rotation_z(transform.rotate_z_radians()),
            ..default()
        },
    })
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
        sprite.rect = Some(texel_rect(texel_offset, logical, drawn.scale));
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
    let mut image = artwork.image();
    image.sampler = ImageSampler::linear();
    image.texture_descriptor.usage |= TextureUsages::TEXTURE_BINDING;
    image
}

// ----------------------------------------------------------------- smoke ---

/// Which scripted pass a smoke run drives. The main menu's is the default; the
/// cabin's (PARITY5-CABIN-INTRO.04) opens the start flow's `PassengerCabin`
/// and is chosen by [`SMOKE_SCENE_ENV`], because `cli.rs` has no flag for it
/// yet (the owner-command smoke of the whole flow is #1842).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmokeScene {
    /// The main-menu walk ([`script`]).
    MainMenu,
    /// The cabin hub's hover, press and focus pass ([`cabin_script`]).
    Cabin,
    /// The whole start flow (#1842): main menu → Campaign → a new pilot →
    /// intro movie → cabin → a not-implemented notice → back to the main
    /// menu ([`flow_script`]).
    Flow,
    /// The owner-command smoke's first leg (#1832 `PARITY5-START-FLOW.04`):
    /// `cs --cs-path <retail> --smoke-seconds <n> --capture-dir <dir>` walks
    /// the main menu → Campaign → a typed new pilot → Continue → the intro
    /// movie → the cabin, then presses **Next Mission** so the run hands the
    /// process over to the mission instead of ending on a timer
    /// ([`start_flow_script`]).
    StartFlow,
    /// The owner-command smoke's return leg (#1832):
    /// `cs --cs-path <retail> --resume-start-flow <end> --smoke-seconds <n>
    /// --capture-dir <dir>` opens on the cabin, photographs it and presses
    /// **Next Mission** again, replaying the mission the player left
    /// ([`start_flow_resumed_script`]).
    StartFlowResumed,
    /// The Preferences → Audio page (#1996): Main Menu → Preferences →
    /// Audio, the check box, a slider drag, the drop-down, Accept, a second
    /// visit that Cancels ([`audio_script`]).
    Audio,
    /// The cabin's Change Memento screen (#1957): Change Memento, the arrows,
    /// Cancel, then again and Accept ([`memento_script`]).
    Memento,
}

/// The environment variable that selects [`SmokeScene::Cabin`] for
/// `cs --cs-path <retail> --front-end --smoke-seconds <n> --capture-dir <dir>`:
/// `cabin`, or [`SmokeScene::Flow`]: `flow`. Any other value, or none, is the
/// main-menu walk.
pub const SMOKE_SCENE_ENV: &str = "CS_FRONT_END_SMOKE_SCENE";

impl SmokeScene {
    /// The scene the environment names.
    #[must_use]
    pub fn from_env() -> Self {
        std::env::var(SMOKE_SCENE_ENV).map_or(Self::MainMenu, |value| Self::from_name(&value))
    }

    /// The scene a [`SMOKE_SCENE_ENV`] value names: `cabin` or `flow`, any
    /// case; anything else is the main-menu walk.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "cabin" => Self::Cabin,
            "flow" => Self::Flow,
            "audio" => Self::Audio,
            "memento" => Self::Memento,
            _ => Self::MainMenu,
        }
    }
}

/// The smoke run, one resource: the collected data, the delivered shots,
/// the remaining script and the static config.
#[derive(Resource)]
struct ShellSmoke {
    /// Which scripted pass this is.
    scene: SmokeScene,
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

/// Shared cell the shell fills with the mission its Launch step named, the
/// frame it writes the run's `AppExit`: `App::run` leaves the caller an
/// empty world, so the hand-over travels through this like the smoke's
/// report through [`ShellSmokeHandle`] (#1832).
#[derive(Resource, Clone, Default)]
pub(super) struct HandoverCell(Arc<Mutex<Option<String>>>);

impl HandoverCell {
    /// The mission the hand-over was made for, once it was.
    #[inline(always)]
    pub fn take(&self) -> Option<String> {
        self.0.lock().ok().and_then(|mut slot| slot.take())
    }
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
    /// FNV-1a of the frame's pixels: two shots of an animated scene differ.
    pub frame_hash: u64,
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
    /// What each presented `M` row did (see [`ShellRun::movies`]).
    pub movies: Vec<String>,
    /// Decoded movie frames delivered to a texture.
    pub movie_frames: u64,
    /// Whether the menu music loop started.
    pub music_started: bool,
    /// The audio the run asked of its output, one line per kind
    /// (`cue rollover x3`, `refused music: <code>`), or the note saying why
    /// the run had no output.
    pub audio: Vec<String>,
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
    /// Move the pointer to a logical (800x600) point, mapped through the
    /// fit the surface has this frame.
    HoverPoint(i32, i32),
    /// Left button down/up.
    Mouse(bool),
    /// A key down/up.
    Key(KeyCode, bool),
    /// Typed text, as one `KeyboardInput` press per character (the message
    /// the OS text input writes), for the pilot page's edit box.
    Type(&'static str),
    /// A framebuffer screenshot.
    Shot(&'static str),
}

/// The deterministic pass the smoke drives over `MainMenu` (FE-MENU-2):
///
/// settle → screenshot → hover `MM_B_CAMPAIGN` (rollover frame) → press it
/// (depressed frame) → land on `Campaign` → screenshot → its `CM_B_CANCEL`
/// back; then the same press-and-back walk over `MM_B_INSTANTACTION`
/// (`InstantAction`/`IA_B_Exit`) and `MM_B_CREDITS` (`Credits`/`CR_B_Exit`),
/// with `MM_B_MULTIPLAYER` pressed in between — the recorded unhandled
/// press, its `MultiPlayerMain` section absent — and `MM_B_PREFERENCES`
/// (`Preferences`), left by `Esc` (its main-menu button's handler guards on
/// a runtime flag, not a plain hand-over, so the shell records it
/// unhandled); a second main-menu shot shows the flag movie still moving;
/// then `Down` five times to `MM_B_QUIT` and `Enter` it (depressed, then
/// the `terminate` the script states).
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
        // The pilot page (FE-MENU-3): type a name, then Down selects the
        // first listed pilot (when the population has one), copying its name.
        (at(3.05), Step::Type("Eve")),
        (at(3.1), Step::Shot("campaign-typed")),
        (at(3.15), Step::Key(KeyCode::ArrowDown, true)),
        (at(3.2), Step::Key(KeyCode::ArrowDown, false)),
        (at(3.3), Step::Shot("campaign-list")),
        (at(3.4), Step::HoverButton("CM_B_CANCEL")),
        (at(3.6), Step::Mouse(true)),
        (at(3.8), Step::Mouse(false)),
        (at(4.2), Step::HoverButton("MM_B_INSTANTACTION")),
        (at(4.4), Step::Mouse(true)),
        (at(4.6), Step::Mouse(false)),
        (at(5.0), Step::Shot("instantaction")),
        (at(5.4), Step::HoverButton("IA_B_Exit")),
        (at(5.6), Step::Mouse(true)),
        (at(5.8), Step::Mouse(false)),
        (at(6.2), Step::HoverButton("MM_B_MULTIPLAYER")),
        (at(6.4), Step::Mouse(true)),
        (at(6.6), Step::Mouse(false)),
        (at(7.0), Step::HoverButton("MM_B_PREFERENCES")),
        (at(7.2), Step::Mouse(true)),
        (at(7.4), Step::Mouse(false)),
        (at(7.8), Step::Shot("preferences")),
        (at(8.2), Step::Key(KeyCode::Escape, true)),
        (at(8.3), Step::Key(KeyCode::Escape, false)),
        (at(9.0), Step::HoverButton("MM_B_CREDITS")),
        (at(9.2), Step::Mouse(true)),
        (at(9.4), Step::Mouse(false)),
        (at(9.8), Step::Shot("credits")),
        (at(10.2), Step::HoverButton("CR_B_Exit")),
        (at(10.4), Step::Mouse(true)),
        (at(10.6), Step::Mouse(false)),
        (at(10.9), Step::Shot("menu-later")),
    ];
    for i in 0..5 {
        steps.push((
            at(11.2 + f64::from(i) * 0.2),
            Step::Key(KeyCode::ArrowDown, true),
        ));
        steps.push((
            at(11.25 + f64::from(i) * 0.2),
            Step::Key(KeyCode::ArrowDown, false),
        ));
    }
    steps.push((at(12.3), Step::Shot("focus-quit")));
    steps.push((at(12.5), Step::Key(KeyCode::Enter, true)));
    steps.push((at(12.6), Step::Shot("depressed")));
    steps.push((at(12.7), Step::Key(KeyCode::Enter, false)));
    steps
}

/// The deterministic pass over the Audio page (#1996): Main Menu →
/// Preferences → Audio → screenshot → the check box off → a drag of the music
/// slider to its left end → the drop-down open → Low picked → screenshot →
/// Accept (back on Preferences) → Audio again (the committed values return)
/// → screenshot → the check box on and a drag, then Cancel → Audio a third
/// time → screenshot (the discarded change is gone).
#[inline(always)]
fn audio_script() -> Vec<(u32, Step)> {
    let at = |seconds: f64| (seconds * SMOKE_FRAME_HZ) as u32;
    let click = |t: f64| {
        [
            (at(t), Step::Mouse(true)),
            (at(t + 0.1), Step::Mouse(false)),
        ]
    };
    let mut steps = vec![(at(0.5), Step::HoverButton("MM_B_PREFERENCES"))];
    steps.extend(click(0.7));
    steps.push((at(1.2), Step::HoverButton("PF_B_AUDIO")));
    steps.extend(click(1.4));
    steps.push((at(2.0), Step::Shot("audio-open")));
    // The check box, then the music slider's left end.
    steps.push((at(2.2), Step::HoverButton("AP_B_MUSIC")));
    steps.extend(click(2.4));
    steps.push((at(2.8), Step::Shot("audio-music-off")));
    steps.push((at(3.0), Step::HoverPoint(200, 351)));
    steps.push((at(3.1), Step::Mouse(true)));
    steps.push((at(3.2), Step::HoverPoint(150, 351)));
    steps.push((at(3.3), Step::HoverPoint(139, 351)));
    steps.push((at(3.4), Step::Mouse(false)));
    // The drop-down: open, pick the first item (Low).
    steps.push((at(3.6), Step::HoverPoint(200, 513)));
    steps.extend(click(3.7));
    steps.push((at(4.1), Step::Shot("audio-list")));
    steps.push((at(4.2), Step::HoverPoint(200, 530)));
    steps.extend(click(4.3));
    steps.push((at(4.7), Step::Shot("audio-edited")));
    steps.push((at(4.9), Step::HoverButton("AP_B_ACCEPTCHANGES")));
    steps.extend(click(5.0));
    steps.push((at(5.5), Step::HoverButton("PF_B_AUDIO")));
    steps.extend(click(5.6));
    steps.push((at(6.1), Step::Shot("audio-reopened")));
    steps.push((at(6.3), Step::HoverButton("AP_B_MUSIC")));
    steps.extend(click(6.4));
    steps.push((at(6.8), Step::HoverPoint(300, 408)));
    steps.push((at(6.9), Step::Mouse(true)));
    steps.push((at(7.0), Step::HoverPoint(310, 408)));
    steps.push((at(7.1), Step::Mouse(false)));
    steps.push((at(7.3), Step::Shot("audio-unsaved")));
    steps.push((at(7.5), Step::HoverButton("AP_B_CANCELCHANGES")));
    steps.extend(click(7.6));
    steps.push((at(8.1), Step::HoverButton("PF_B_AUDIO")));
    steps.extend(click(8.2));
    steps.push((at(8.7), Step::Shot("audio-cancelled")));
    steps
}

/// The deterministic pass over the cabin hub: settle → screenshot → hover
/// Next Mission (rollover) → screenshot → press Previous Missions
/// (depressed) → screenshot → slide off it and release (nothing activates) →
/// `Down` three times (the focus walk) → screenshot. Next Mission is not
/// pressed: that hands the process over to the mission.
#[inline(always)]
fn cabin_script() -> Vec<(u32, Step)> {
    let at = |seconds: f64| (seconds * SMOKE_FRAME_HZ) as u32;
    let mut steps = vec![
        (at(0.6), Step::Shot("cabin")),
        (at(0.9), Step::HoverButton("PC_B_NEWMISSION")),
        (at(1.2), Step::Shot("cabin-hover")),
        (at(1.5), Step::HoverButton("PC_B_PREVIOUS")),
        (at(1.7), Step::Mouse(true)),
        (at(1.9), Step::Shot("cabin-pressed")),
        (at(2.1), Step::HoverButton("PC_B_PLANEX")),
        (at(2.3), Step::Mouse(false)),
    ];
    for i in 0..3 {
        steps.push((
            at(2.6 + f64::from(i) * 0.2),
            Step::Key(KeyCode::ArrowDown, true),
        ));
        steps.push((
            at(2.65 + f64::from(i) * 0.2),
            Step::Key(KeyCode::ArrowDown, false),
        ));
    }
    steps.push((at(3.5), Step::Shot("cabin-focus")));
    steps
}

/// The deterministic pass over the Change Memento screen (#1957): the cabin,
/// Change Memento (the pilot's memento), the right arrow (the next picture),
/// the left arrow twice (back, and wrapping to the last offered), Cancel (the
/// cabin), Change Memento again (the stored memento, unchanged) and Accept
/// after an arrow (the cabin). Needs `--smoke-seconds 12` or more.
#[inline(always)]
fn memento_script() -> Vec<(u32, Step)> {
    let at = |seconds: f64| (seconds * SMOKE_FRAME_HZ) as u32;
    let click = |steps: &mut Vec<(u32, Step)>, from: f64, button: &'static str| {
        steps.push((at(from), Step::HoverButton(button)));
        steps.push((at(from + 0.2), Step::Mouse(true)));
        steps.push((at(from + 0.3), Step::Mouse(false)));
    };
    let mut steps = vec![(at(0.6), Step::Shot("cabin"))];
    click(&mut steps, 1.0, "PC_B_CHANGEMOMENTO");
    steps.push((at(2.0), Step::Shot("memento")));
    click(&mut steps, 2.2, "MS_B_ARROWR");
    steps.push((at(3.0), Step::Shot("memento-next")));
    click(&mut steps, 3.2, "MS_B_ARROWL");
    click(&mut steps, 3.8, "MS_B_ARROWL");
    steps.push((at(4.6), Step::Shot("memento-wrapped")));
    click(&mut steps, 4.8, "MS_B_CANCEL");
    steps.push((at(5.8), Step::Shot("memento-cancelled")));
    click(&mut steps, 6.0, "PC_B_CHANGEMOMENTO");
    steps.push((at(7.0), Step::Shot("memento-reopened")));
    click(&mut steps, 7.2, "MS_B_ARROWR");
    click(&mut steps, 7.8, "MS_B_ACCEPT");
    steps.push((at(9.0), Step::Shot("memento-accepted")));
    click(&mut steps, 9.2, "PC_B_CHANGEMOMENTO");
    steps.push((at(10.2), Step::Shot("memento-stored")));
    steps
}

/// The deterministic pass over the whole start flow (#1842): the main menu,
/// Campaign, a typed new pilot and Continue (the intro movie starts), three
/// frames of the movie, Esc to skip it, the cabin, a press on Previous
/// Missions (the list screen), its Return to Cabin, and Return to Main Menu. Needs
/// `--smoke-seconds 19` or more.
#[inline(always)]
fn flow_script() -> Vec<(u32, Step)> {
    let at = |seconds: f64| (seconds * SMOKE_FRAME_HZ) as u32;
    vec![
        (at(0.5), Step::Shot("flow-mainmenu")),
        (at(1.0), Step::HoverButton("MM_B_CAMPAIGN")),
        (at(1.4), Step::Mouse(true)),
        (at(1.6), Step::Mouse(false)),
        (at(2.0), Step::Shot("flow-pilot")),
        (at(2.1), Step::Type("Eve")),
        (at(2.3), Step::Shot("flow-pilot-typed")),
        (at(2.5), Step::HoverButton("CM_B_START")),
        (at(2.7), Step::Mouse(true)),
        (at(2.9), Step::Mouse(false)),
        (at(3.5), Step::Shot("intro-a")),
        (at(5.0), Step::Shot("intro-b")),
        (at(8.0), Step::Shot("intro-c")),
        (at(10.0), Step::Key(KeyCode::Escape, true)),
        (at(10.1), Step::Key(KeyCode::Escape, false)),
        (at(11.0), Step::Shot("flow-cabin")),
        (at(11.3), Step::HoverButton("PC_B_PREVIOUS")),
        (at(11.5), Step::Mouse(true)),
        (at(11.7), Step::Mouse(false)),
        (at(12.2), Step::Shot("flow-previous-missions")),
        (at(12.4), Step::HoverButton("SBTOC_B_RETURN")),
        (at(12.5), Step::Mouse(true)),
        (at(12.6), Step::Mouse(false)),
        (at(13.0), Step::Shot("flow-cabin-after")),
        (at(13.2), Step::HoverButton("PC_B_PLANEX")),
        (at(13.4), Step::Mouse(true)),
        (at(13.5), Step::Mouse(false)),
        (at(14.0), Step::Shot("flow-plane-name")),
        (at(14.1), Step::Type("Skylark")),
        (at(14.3), Step::Shot("flow-plane-name-typed")),
        (at(14.5), Step::HoverButton("PN_B_CANCEL")),
        (at(14.6), Step::Mouse(true)),
        (at(14.7), Step::Mouse(false)),
        (at(15.1), Step::Shot("flow-cabin-again")),
        (at(15.3), Step::HoverButton("PC_B_RETURNMM")),
        (at(15.5), Step::Mouse(true)),
        (at(15.7), Step::Mouse(false)),
        (at(16.3), Step::Shot("flow-mainmenu-again")),
    ]
}

/// The owner-command smoke's first leg (#1832 `PARITY5-START-FLOW.04`):
/// settle → `mainmenu` → hover and click `MM_B_CAMPAIGN` → `campaign` → type
/// the pilot's name → `pilot` → `CM_B_START` (the intro movie starts) →
/// `intro` → Esc skips the movie → `cabin` → hover and click
/// `PC_B_NEWMISSION`, which takes the mission hand-over the table's Launch
/// row names. The run does not end on a timer: [`step_smoke`] finishes it the
/// frame the hand-over lands, and `run_start_flow` replaces the process with
/// the mission leg. Every required screen names itself in a failure when its
/// framebuffer never arrives.
#[inline(always)]
fn start_flow_script() -> Vec<(u32, Step)> {
    let at = |seconds: f64| (seconds * SMOKE_FRAME_HZ) as u32;
    vec![
        (at(0.5), Step::Shot("mainmenu")),
        (at(1.0), Step::HoverButton("MM_B_CAMPAIGN")),
        (at(1.4), Step::Mouse(true)),
        (at(1.6), Step::Mouse(false)),
        (at(2.0), Step::Shot("campaign")),
        (at(2.1), Step::Type("Eve")),
        (at(2.3), Step::Shot("pilot")),
        (at(2.5), Step::HoverButton("CM_B_START")),
        (at(2.7), Step::Mouse(true)),
        (at(2.9), Step::Mouse(false)),
        (at(4.0), Step::Shot("intro")),
        (at(10.0), Step::Key(KeyCode::Escape, true)),
        (at(10.1), Step::Key(KeyCode::Escape, false)),
        (at(11.0), Step::Shot("cabin")),
        (at(11.4), Step::HoverButton("PC_B_NEWMISSION")),
        (at(11.6), Step::Mouse(true)),
        (at(11.8), Step::Mouse(false)),
    ]
}

/// The owner-command smoke's return leg (#1832): the run opens on the cabin
/// (the flow resumed after the mission the player left), photographs it and
/// presses **Next Mission** — the replay the hand-over then launches.
#[inline(always)]
fn start_flow_resumed_script() -> Vec<(u32, Step)> {
    let at = |seconds: f64| (seconds * SMOKE_FRAME_HZ) as u32;
    vec![
        (at(0.6), Step::Shot("cabin")),
        (at(1.0), Step::HoverButton("PC_B_NEWMISSION")),
        (at(1.2), Step::Mouse(true)),
        (at(1.4), Step::Mouse(false)),
    ]
}

/// The smoke run as a plugin.
pub struct ShellSmokePlugin {
    scene: SmokeScene,
    request: SmokeRequest,
    steps: Vec<(u32, Step)>,
    handle: ShellSmokeHandle,
}

impl ShellSmokePlugin {
    /// A windowed smoke for `request`.
    #[must_use]
    #[inline(always)]
    pub fn windowed(request: SmokeRequest) -> Self {
        Self::windowed_scene(request, SmokeScene::MainMenu)
    }

    /// A windowed smoke for `request` driving `scene`.
    #[must_use]
    #[inline(always)]
    pub fn windowed_scene(request: SmokeRequest, scene: SmokeScene) -> Self {
        Self {
            scene,
            request,
            steps: match scene {
                SmokeScene::MainMenu => script(),
                SmokeScene::Cabin => cabin_script(),
                SmokeScene::Flow => flow_script(),
                SmokeScene::StartFlow => start_flow_script(),
                SmokeScene::StartFlowResumed => start_flow_resumed_script(),
                SmokeScene::Audio => audio_script(),
                SmokeScene::Memento => memento_script(),
            },
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
            scene: self.scene,
            frames: 0,
            shots_requested: 0,
            io_error: None,
            shots: Arc::new(Mutex::new(Vec::new())),
            steps: self.steps.clone(),
            step: 0,
            request: self.request.clone(),
            handle: self.handle.clone(),
        });
        // After the input systems' per-frame clear and before `Update`, where
        // `frame_shell` and the movie's skip both read `just_pressed`: a
        // scripted key reaches every reader as a real one does.
        app.add_systems(
            PreUpdate,
            bevy::ecs::schedule::IntoScheduleConfigs::after(
                inject_smoke_input_system,
                bevy::input::InputSystems,
            ),
        );
    }
}

/// Injects the frame's scripted input into the real `ButtonInput`/`CursorMoved`
/// path — the input systems' own per-frame clear already ran in `PreUpdate`,
/// so scripted `just_*` edges survive exactly one frame like real ones.
#[inline(always)]
fn inject_smoke_input_system(world: &mut World) {
    let Some(mut state) = world.remove_resource::<ShellResource>() else {
        return;
    };
    inject_smoke_input(world, &mut state);
    world.insert_resource(state);
}

/// The body of [`inject_smoke_input_system`].
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
                // does nothing, and the run's own checks fail it). Row ids
                // are stored upper-cased; the steps name them as the file
                // writes them, so the match is case-insensitive.
                let position = state.shell.view(state.surface).and_then(|view| {
                    view.buttons
                        .iter()
                        .find(|button| button.id.eq_ignore_ascii_case(id))
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
            Step::HoverPoint(x, y) => {
                if let Some(view) = state.shell.view(state.surface) {
                    let at = view.fit.map_rect(cs_content::ui_layout::Rect {
                        x: (*x).max(0) as u32,
                        y: (*y).max(0) as u32,
                        width: 1,
                        height: 1,
                    });
                    world
                        .resource_mut::<Messages<CursorMoved>>()
                        .write(CursorMoved {
                            window: state.scene.window.unwrap_or(Entity::PLACEHOLDER),
                            position: Vec2::new(at.x as f32 + 0.5, at.y as f32 + 0.5),
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
            Step::Type(text) => {
                let window = state.scene.window.unwrap_or(Entity::PLACEHOLDER);
                let mut keyboard = world.resource_mut::<Messages<KeyboardInput>>();
                for character in text.chars() {
                    let typed = character.to_string();
                    keyboard.write(KeyboardInput {
                        key_code: KeyCode::Unidentified(
                            bevy::input::keyboard::NativeKeyCode::Unidentified,
                        ),
                        logical_key: Key::Character(typed.as_str().into()),
                        state: ButtonState::Pressed,
                        text: Some(typed.as_str().into()),
                        repeat: false,
                        window,
                    });
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
    // The owner-command smoke's legs end on the mission hand-over, not on
    // the timer: `frame_shell` writes the Launch step's AppExit this same
    // frame, so the report is written now or never. A required framebuffer
    // still in flight lands in the report as missing, named by its label.
    let handed_over = matches!(
        smoke.scene,
        SmokeScene::StartFlow | SmokeScene::StartFlowResumed
    ) && state
        .flow
        .as_ref()
        .is_some_and(|flow| flow.handover().is_some());
    if handed_over
        || (frame >= end_frame && (finished || frame >= end_frame + SCREENSHOT_GRACE_FRAMES))
    {
        let facts = flow_facts(state);
        let report = evaluate(smoke, &state.run, state.audio.as_ref(), &facts);
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
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    if let Some(data) = image.data.as_ref() {
        for &byte in data.iter() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
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
        frame_hash: hash,
    }
}

/// The pass/fail checks of a finished run.
#[inline(always)]
fn evaluate(
    smoke: &ShellSmoke,
    run: &ShellRun,
    audio: Option<&MenuAudio>,
    facts: &FlowFacts,
) -> ShellSmokeReport {
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
    // The owner-command smoke's legs (#1832 `PARITY5-START-FLOW.04`): the
    // first leg walks MainMenu → Campaign → PassengerCabin and hands over to
    // the mission; the resumed leg opens on the cabin and replays. Every
    // required screen names itself when its framebuffer never arrived, and a
    // screen the machine refused names itself in `refusals`.
    if matches!(
        smoke.scene,
        SmokeScene::StartFlow | SmokeScene::StartFlowResumed
    ) {
        let resumed = smoke.scene == SmokeScene::StartFlowResumed;
        let expected: &[&str] = if resumed {
            &["PassengerCabin"]
        } else {
            &["MainMenu", "Campaign", "PassengerCabin"]
        };
        if run.visited != expected {
            failures.push(format!(
                "the start-flow leg visited {:?}, expected {}",
                run.visited,
                expected.join(" → ")
            ));
        }
        let intro_wanted = usize::from(!resumed);
        if facts.intro.len() != intro_wanted {
            failures.push(format!(
                "the intro ended {} times ({:?}); expected {intro_wanted}",
                facts.intro.len(),
                facts.intro
            ));
        }
        if !facts.notices.is_empty() {
            failures.push(format!(
                "not-implemented notices opened during the leg: {:?}",
                facts.notices
            ));
        }
        if facts.notice_open {
            failures.push("a not-implemented notice was still up at the hand-over".to_owned());
        }
        match facts.handover.as_deref() {
            Some(mission) if mission == FIRST_MISSION => {}
            other => failures.push(format!(
                "the leg handed over to {other:?}; expected Some(\"{FIRST_MISSION}\")"
            )),
        }
        for refusal in &facts.refusals {
            failures.push(format!("the start flow refused a screen: {refusal}"));
        }
        let required: &[&str] = if resumed {
            &["cabin"]
        } else {
            &["mainmenu", "campaign", "pilot", "intro", "cabin"]
        };
        for label in required {
            if !shots.iter().any(|shot| shot.label == *label) {
                failures.push(format!(
                    "the {label} framebuffer never arrived — that screen was not captured"
                ));
            }
        }
        if run.unhandled != 0 {
            failures.push(format!(
                "{} presses landed on buttons with no known effect (expected none)",
                run.unhandled
            ));
        }
        let mut audio_lines = Vec::new();
        if let Some(note) = &run.audio_note {
            audio_lines.push(format!("no audio output: {note}"));
        }
        audio_lines.push(format!("intro outcomes {:?}", facts.intro));
        return smoke_report(smoke, run, shots, failures, audio_lines, audio);
    }
    if smoke.scene == SmokeScene::Flow {
        if run.visited
            != [
                "MainMenu",
                "Campaign",
                "PassengerCabin",
                "ScrapBook_TOC",
                "PassengerCabin",
                "PlaneName",
                "PassengerCabin",
                "MainMenu",
            ]
        {
            failures.push(format!(
                "the flow visited {:?}, expected MainMenu → Campaign → PassengerCabin → \
                 ScrapBook_TOC → PassengerCabin → PlaneName → PassengerCabin → MainMenu",
                run.visited
            ));
        }
        if facts.intro.len() != 1 {
            failures.push(format!(
                "the intro ended {} times ({:?}); expected once, by the skip key",
                facts.intro.len(),
                facts.intro
            ));
        }
        if !facts.notices.is_empty() {
            failures.push(format!(
                "not-implemented notices opened during the flow: {:?}",
                facts.notices
            ));
        }
        if facts.notice_open {
            failures.push("the notice was still up at the end of the run".to_owned());
        }
        let mut audio_lines = Vec::new();
        if let Some(note) = &run.audio_note {
            audio_lines.push(format!("no audio output: {note}"));
        }
        audio_lines.push(format!("intro outcomes {:?}", facts.intro));
        return smoke_report(smoke, run, shots, failures, audio_lines, audio);
    }
    if smoke.scene == SmokeScene::Audio {
        let expected = [
            "MainMenu",
            "Preferences",
            "Audio",
            "Preferences",
            "Audio",
            "Preferences",
            "Audio",
        ];
        if run.visited != expected {
            failures.push(format!(
                "the audio pass visited {:?}, expected MainMenu → Preferences → Audio → \
                 Preferences (Accept) → Audio → Preferences (Cancel) → Audio",
                run.visited
            ));
        }
        if facts.audio_accepts != 1 {
            failures.push(format!(
                "the Audio page committed {} times (expected the one Accept)",
                facts.audio_accepts
            ));
        }
        for label in [
            "audio-open",
            "audio-music-off",
            "audio-list",
            "audio-edited",
            "audio-reopened",
            "audio-unsaved",
            "audio-cancelled",
        ] {
            if !shots.iter().any(|shot| shot.label == label) {
                failures.push(format!("the {label} framebuffer never arrived"));
            }
        }
        if run.unhandled != 0 {
            failures.push(format!(
                "{} presses landed on buttons with no known effect (expected none)",
                run.unhandled
            ));
        }
        let mut audio_lines = Vec::new();
        if let Some(note) = &run.audio_note {
            audio_lines.push(format!("no audio output: {note}"));
        }
        if let Some(committed) = &facts.audio_committed {
            audio_lines.push(format!("audio committed {committed}"));
        }
        return smoke_report(smoke, run, shots, failures, audio_lines, audio);
    }
    if smoke.scene == SmokeScene::Memento {
        let section = "MomentoSelection";
        let cabin = "PassengerCabin";
        if run.visited != [cabin, section, cabin, section, cabin, section] {
            failures.push(format!(
                "the memento pass visited {:?}; expected the cabin, then the screen opened three times and left twice",
                run.visited
            ));
        }
        for label in [
            "memento",
            "memento-next",
            "memento-wrapped",
            "memento-cancelled",
            "memento-reopened",
            "memento-accepted",
            "memento-stored",
        ] {
            if !shots.iter().any(|shot| shot.label == label) {
                failures.push(format!("the {label} framebuffer never arrived"));
            }
        }
        return smoke_report(smoke, run, shots, failures, Vec::new(), None);
    }
    if smoke.scene == SmokeScene::Cabin {
        if run.visited != ["PassengerCabin"] {
            failures.push(format!(
                "the cabin pass visited {:?}; it opens the cabin and only hovers, presses and slides off",
                run.visited
            ));
        }
        if !run.hovered.iter().any(|id| id == "PC_B_NEWMISSION") {
            failures.push("the pointer never hovered PC_B_NEWMISSION".to_owned());
        }
        if !run.depressed_seen {
            failures.push("no button ever drew its depressed frame".to_owned());
        }
        if run.focus_moves < 3 {
            failures.push(format!(
                "focus moved only {} times (expected 3 Down presses)",
                run.focus_moves
            ));
        }
        if run.unhandled != 0 {
            failures.push(format!(
                "{} presses landed on buttons with no known effect (expected none)",
                run.unhandled
            ));
        }
        return smoke_report(smoke, run, shots, failures, Vec::new(), None);
    }
    let expected = [
        "MainMenu",
        "Campaign",
        "MainMenu",
        "InstantAction",
        "MainMenu",
        "Preferences",
        "MainMenu",
        "Credits",
        "MainMenu",
    ];
    if run.visited != expected {
        failures.push(format!(
            "the script visited {:?}, expected MainMenu → Campaign/InstantAction/Preferences/Credits → MainMenu",
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
    // The Multiplayer press is the run's one recorded unhandled press: the
    // `MultiPlayerMain` section its script names does not exist, and the
    // lobby's presenter is unbuilt — recorded, never faked.
    if run.unhandled != 1 {
        failures.push(format!(
            "{} presses landed on buttons with no known effect (expected the one Multiplayer press)",
            run.unhandled
        ));
    }
    // FE-MENU-2: the main menu's CrimFlag.MPG must have opened and played.
    let movie_opened = run
        .movies
        .iter()
        .any(|line| line.starts_with("MOVIE MOVIE on MainMenu:") && !line.contains("refused"));
    if !movie_opened {
        failures.push(format!(
            "the main menu's CrimFlag.MPG never opened ({:?})",
            run.movies
        ));
    }
    if run.movie_frames == 0 {
        failures.push("no decoded movie frame ever reached a texture".to_owned());
    }
    match (
        shots.iter().find(|shot| shot.label == "mainmenu"),
        shots.iter().find(|shot| shot.label == "menu-later"),
    ) {
        (Some(first), Some(later)) if first.frame_hash == later.frame_hash => {
            failures.push(
                "the flag movie never changed a frame (mainmenu and menu-later are identical)"
                    .to_owned(),
            );
        }
        _ => {}
    }
    let mut audio_lines = Vec::new();
    if let Some(note) = &run.audio_note {
        audio_lines.push(format!("no audio output: {note}"));
    }
    if let Some(audio) = audio {
        for cue in MenuCue::ALL {
            let count = audio.cue_count(cue);
            if count > 0 {
                audio_lines.push(format!("cue {} x{count}", cue.label()));
            }
        }
        for event in audio.events() {
            if let MenuAudioEvent::Refused { what, code } = event {
                audio_lines.push(format!("refused {what}: {code}"));
            }
        }
        // FE-MENU-4: with an output, the music must sound from the start and
        // the main menu walk must have produced both pointer cues.
        if !audio.music_playing() {
            failures.push("the menu music never started on the output".to_owned());
        }
        for cue in [MenuCue::Rollover, MenuCue::Press] {
            if audio.cue_count(cue) == 0 {
                failures.push(format!("the {} cue never played", cue.label()));
            }
        }
        if audio.cue_count(MenuCue::TextAccepted) == 0 {
            failures.push("the typed name never played the text-entry cue".to_owned());
        }
    }
    smoke_report(smoke, run, shots, failures, audio_lines, audio)
}

/// What the start flow did that the shell's own record does not hold.
struct FlowFacts {
    /// How each intro ended (`Debug` of the outcome).
    intro: Vec<String>,
    /// The `button` of each not-implemented screen opened.
    notices: Vec<String>,
    /// A notice is up now.
    notice_open: bool,
    /// The mission the hand-over named (`Some` once the Launch row ran).
    handover: Option<String>,
    /// Every screen the machine refused, in order.
    refusals: Vec<String>,
    /// The Audio page's Accepts and the values it holds committed.
    audio_accepts: usize,
    /// The committed values (`Debug`), when the run has an Audio page.
    audio_committed: Option<String>,
}

/// Collects the [`FlowFacts`] of the run.
fn flow_facts(state: &ShellResource) -> FlowFacts {
    FlowFacts {
        intro: state
            .intro
            .outcomes
            .iter()
            .map(|outcome| format!("{outcome:?}"))
            .collect(),
        notices: state.flow.as_ref().map_or_else(Vec::new, |flow| {
            flow.not_implemented_seen()
                .iter()
                .map(|entry| entry.button.to_owned())
                .collect()
        }),
        notice_open: state.flow.as_ref().is_some_and(|f| f.notice().is_some()),
        handover: state
            .flow
            .as_ref()
            .and_then(|flow| flow.handover().map(str::to_owned)),
        refusals: state.flow.as_ref().map_or_else(Vec::new, |flow| {
            flow.refusals().iter().map(ToString::to_string).collect()
        }),
        audio_accepts: state.audio_page().map_or(0, |page| page.accepts().len()),
        audio_committed: state
            .audio_page()
            .map(|page| format!("{:?}", page.committed())),
    }
}

/// The report of a finished pass.
fn smoke_report(
    smoke: &ShellSmoke,
    run: &ShellRun,
    shots: Vec<ShellShotRecord>,
    failures: Vec<String>,
    audio_lines: Vec<String>,
    audio: Option<&MenuAudio>,
) -> ShellSmokeReport {
    ShellSmokeReport {
        music_started: audio.is_some_and(MenuAudio::music_playing),
        audio: audio_lines,
        frames: smoke.frames,
        visited: run.visited.clone(),
        focus_moves: run.focus_moves,
        hovered: run.hovered.iter().cloned().collect(),
        depressed_seen: run.depressed_seen,
        quit_seen: run.quit_seen,
        unhandled: run.unhandled,
        cursor: run.cursor.clone(),
        movies: run.movies.clone(),
        movie_frames: run.movie_frames,
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
                "{{\"label\":{},\"path\":{},\"width\":{},\"height\":{},\"distinct_luminance\":{},\"mean_luminance\":{},\"frame_hash\":{}}}",
                json_string(&shot.label),
                json_string(&shot.path.display().to_string()),
                shot.width,
                shot.height,
                shot.distinct_luminance,
                json_f32(shot.mean_luminance),
                shot.frame_hash
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
    let movies = report
        .movies
        .iter()
        .map(|line| json_string(line))
        .collect::<Vec<_>>()
        .join(",");
    let failures = report
        .failures
        .iter()
        .map(|failure| json_string(failure))
        .collect::<Vec<_>>()
        .join(",");
    let audio = report
        .audio
        .iter()
        .map(|line| json_string(line))
        .collect::<Vec<_>>()
        .join(",");
    let body = format!(
        "{{\"kind\":\"front-end-shell-smoke-report\",\"frames\":{},\"visited\":[{}],\"focus_moves\":{},\"hovered\":[{}],\"depressed_seen\":{},\"quit_seen\":{},\"unhandled\":{},\"cursor\":{},\"movies\":[{}],\"movie_frames\":{},\"music_started\":{},\"audio\":[{}],\"shots\":[{}],\"failures\":[{}],\"passed\":{}}}\n",
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
        movies,
        report.movie_frames,
        report.music_started,
        audio,
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
    let scene = SmokeScene::from_env();
    if request.smoke.is_some() && scene == SmokeScene::Flow {
        return shell_app_with(request, Some(StartFlow::new()), scene);
    }
    if request.smoke.is_some() && matches!(scene, SmokeScene::Cabin | SmokeScene::Memento) {
        let flow = StartFlow::after_mission(super::start_flow::MissionEnd::ReturnedToCabin)
            .map_err(|refusal| FrontEndError::Exit(refusal.to_string()))?;
        return shell_app_with(request, Some(flow), scene);
    }
    shell_app_with(request, None, scene)
}

/// As [`shell_app`], with the F45 machine in charge of the shell's moves when
/// `flow` is given (the start flow, [`super::start_flow`]) and `scene` picking
/// the smoke run's script and checks when `request.smoke` is set.
///
/// # Errors
///
/// As [`shell_app`].
pub(super) fn shell_app_with(
    request: &FrontEndRequest,
    flow: Option<StartFlow>,
    scene: SmokeScene,
) -> Result<(App, Option<ShellSmokeHandle>), FrontEndError> {
    let screens = FrontEndScreens::open(&request.cs_path).map_err(FrontEndError::Retail)?;
    let layout = screens.original_layout().map_err(FrontEndError::Retail)?;
    let mut shell = MenuShell::build(
        &layout,
        &|art: &str| screens.art_extent(art),
        &|section: &str| screens.script_text(&script_member(section)),
    );
    shell.carry_under(PILOT_SECTION, START_SECTION, PILOT_CARRIED_PANES);
    // The cabin hub (#1840): a layout without the hangar art keeps the
    // section as written.
    let _ = CabinHub::NEW_PILOT.apply(&mut shell, &|art: &str| screens.art_extent(art));
    let entry = flow
        .as_ref()
        .and_then(StartFlow::section)
        .unwrap_or(START_SECTION);
    shell.open(entry).map_err(FrontEndError::Shell)?;

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
    // The Campaign section's pilot page and the profile session behind it
    // (FE-MENU-3). A page the layout cannot supply, or a population that
    // cannot open, is reported and the section still draws its panes and
    // buttons.
    let mut flow = flow;
    if let Some(flow) = flow.as_mut() {
        // The Change Memento screen's pictures: decoded up front like the
        // rest of the art, their extents kept for placing the one shown.
        let mut pictures = BTreeMap::new();
        for entry in &super::memento::MEMENTOS {
            let name = super::memento::picture_art(entry.name);
            if let Some(extent) = screens.art_extent(&name) {
                pictures.insert(name.clone(), extent);
            }
            if let Some(artwork) = screens.graphic_artwork(&name) {
                art.entry(name.to_ascii_lowercase()).or_insert(artwork);
            }
        }
        flow.set_memento_pictures(pictures);
    }
    let pilots = pilot_setup(request, &layout);
    let audio_page = audio_page_setup(request, &screens, &layout);
    let previous = previous_missions_setup(request, &screens, &layout);
    let plane = plane_name_setup(request, &screens, &layout);
    if previous.is_some()
        && let Some(artwork) = screens.graphic_artwork(super::previous_missions::ICON_ART)
    {
        // The list's icon strip, which no pane or button references.
        art.entry(super::previous_missions::ICON_ART.to_owned())
            .or_insert(artwork);
    }
    if let Some(page) = &audio_page {
        // The slider and drop-down art the generic rows do not reference.
        let rows = page
            .sliders()
            .iter()
            .flat_map(|slider| [&slider.slot_art, &slider.knob_art])
            .chain(Some(&page.dropdown().arrow_art));
        for name in rows {
            if let Some(artwork) = screens.graphic_artwork(name) {
                art.entry(name.to_ascii_lowercase()).or_insert(artwork);
            }
        }
    }
    let cursor = shell.cursor().and_then(|(name, _)| {
        screens
            .graphic_artwork(name)
            .map(|artwork| (name.clone(), artwork))
    });

    // Every `M` row's movie file, read once through the install mount: the
    // shell can reach any section, so the reads happen up front and a
    // refusal is carried (never retried, never hidden) for the record the
    // section's spawn makes.
    let mut movie_files: BTreeMap<String, Result<Vec<u8>, String>> = BTreeMap::new();
    for section in shell.sections().values() {
        for movie in &section.movies {
            let key = movie.file.to_ascii_lowercase();
            movie_files
                .entry(key)
                .or_insert_with(|| screens.movie_bytes(&movie.file).map_err(|e| e.to_string()));
        }
    }

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
    let mut plugin = FrontEndShellPlugin::windowed(shell, art, cursor, movie_files)
        .with_audio(menu_audio(&screens));
    if flow.is_some() {
        plugin = plugin.with_intro(IntroMovie::retail(
            &request.cs_path,
            AudioOutput::Device(CapabilityDeclaration::from_environment()),
        ));
    }
    if let Some(setup) = pilots {
        plugin = plugin.with_pilots(setup);
    }
    if let Some(page) = audio_page {
        plugin = plugin.with_audio_page(page);
    }
    if let Some(page) = previous {
        plugin = plugin.with_previous_missions(page);
    }
    if let Some(page) = plane {
        plugin = plugin.with_plane_name(page);
    }
    app.add_plugins(match flow {
        Some(flow) => plugin.with_flow(flow),
        None => plugin,
    });
    let handle = request.smoke.as_ref().map(|smoke| {
        let plugin = ShellSmokePlugin::windowed_scene(smoke.clone(), scene);
        let handle = plugin.handle();
        app.add_plugins(plugin);
        handle
    });
    Ok((app, handle))
}

/// The front end's music and cues over the machine's real output, or the
/// named reason there is none (no `audio` capability, no output device, a
/// script without the declarations, a member that will not decode). A run
/// without output still presents; the reason is printed and reported.
fn menu_audio(screens: &FrontEndScreens) -> Result<MenuAudio, String> {
    let table = MenuSoundTable::from_screens(screens).map_err(|error| error.to_string())?;
    let samples = MenuSamples::load(screens, &table);
    if let Some(failure) = samples.failures.first() {
        return Err(format!("sound members unavailable: {failure}"));
    }
    let declaration = CapabilityDeclaration::from_environment();
    MenuAudio::audible(samples, &table, &declaration).map_err(|error| {
        eprintln!("cs: the front end plays no sound: {error}");
        error.to_string()
    })
}

/// The environment variable naming the directory pilots persist in.
pub const PROFILE_DIR_ENV: &str = "CS_PROFILE_DIR";

/// Where a smoke run keeps its pilots, inside the capture directory.
pub const SMOKE_PROFILE_SUBDIR: &str = "profiles";

/// The directory the profile population and the audio settings live in: a
/// smoke's own capture directory, else `$CS_PROFILE_DIR`, else
/// `~/.crimson-skies`.
fn profile_base(request: &FrontEndRequest) -> Option<PathBuf> {
    match &request.smoke {
        Some(smoke) => Some(smoke.capture_dir.join(SMOKE_PROFILE_SUBDIR)),
        None => std::env::var_os(PROFILE_DIR_ENV)
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".crimson-skies"))
            }),
    }
}

/// The Audio page for a run (#1996): its rows from the layout, its labels
/// from the installation's own string table and its values from the audio
/// settings file. A page the installation cannot supply is reported and the
/// section still draws its panes and buttons.
fn audio_page_setup(
    request: &FrontEndRequest,
    screens: &FrontEndScreens,
    layout: &super::OriginalLayout,
) -> Option<AudioPage> {
    let strings = match crate::mission_session::FlightCheckStrings::read(&request.cs_path, screens)
    {
        Ok(strings) => strings,
        Err(error) => {
            eprintln!("cs: no audio page: {error}");
            return None;
        }
    };
    let store = profile_base(request).map(|base| AudioSettingsFile::in_directory(&base));
    match AudioPage::from_layout(
        layout,
        &|name: &str| strings.resolve_tagged(name),
        &|art: &str| screens.art_extent(art),
        store,
    ) {
        Ok(page) => Some(page),
        Err(error) => {
            eprintln!("cs: no audio page: {error}");
            None
        }
    }
}

/// The Previous Missions page for a run (#1956): its rows from the layout,
/// its captions and list text from the installation's own string table. The
/// start flow keeps no per-pilot progress yet, so the pilot is a fresh one
/// ([`MissionProgress::fresh`]). A page the installation cannot supply is
/// reported and the section still draws its panes and buttons.
fn previous_missions_setup(
    request: &FrontEndRequest,
    screens: &FrontEndScreens,
    layout: &super::OriginalLayout,
) -> Option<PreviousMissionsPage> {
    let strings = match crate::mission_session::FlightCheckStrings::read(&request.cs_path, screens)
    {
        Ok(strings) => strings,
        Err(error) => {
            eprintln!("cs: no previous-missions page: {error}");
            return None;
        }
    };
    match PreviousMissionsPage::from_layout(
        layout,
        &|name: &str| strings.resolve_tagged(name),
        &|id: u32| strings.resolve_id_tagged(id),
        &|art: &str| screens.art_extent(art),
        super::MissionProgress::fresh(),
    ) {
        Ok(page) => Some(page),
        Err(error) => {
            eprintln!("cs: no previous-missions page: {error}");
            None
        }
    }
}

/// The Plane Name page for a run (#1958): its rows from the layout, its
/// captions and the empty-name message from the installation's own string
/// table. A page the installation cannot supply is reported and the section
/// still draws its panes and buttons.
fn plane_name_setup(
    request: &FrontEndRequest,
    screens: &FrontEndScreens,
    layout: &super::OriginalLayout,
) -> Option<PlaneNamePage> {
    let strings = match crate::mission_session::FlightCheckStrings::read(&request.cs_path, screens)
    {
        Ok(strings) => strings,
        Err(error) => {
            eprintln!("cs: no plane-name page: {error}");
            return None;
        }
    };
    match PlaneNamePage::from_layout(
        layout,
        &|name: &str| strings.resolve_tagged(name),
        &|id: u32| strings.resolve_id_tagged(id),
        &|art: &str| screens.art_extent(art),
    ) {
        Ok(page) => Some(page),
        Err(error) => {
            eprintln!("cs: no plane-name page: {error}");
            None
        }
    }
}

/// The pilot page and its session for a run: automation (a smoke) gets the
/// synthetic population under its capture directory — the profile library
/// refuses an automated caller the production tree — and a person gets the
/// production population in `$CS_PROFILE_DIR`, else `~/.crimson-skies`.
fn pilot_setup(request: &FrontEndRequest, layout: &super::OriginalLayout) -> Option<PilotSetup> {
    let page = match PilotPage::from_layout(layout) {
        Ok(page) => page,
        Err(error) => {
            eprintln!("cs: no pilot page: {error}");
            return None;
        }
    };
    let catalog = cs_content::save::settings::SettingCatalog::empty();
    let opened = match &request.smoke {
        Some(smoke) => {
            ProfileSession::open_sandbox(&smoke.capture_dir.join(SMOKE_PROFILE_SUBDIR), &catalog)
        }
        None => {
            let base = std::env::var_os(PROFILE_DIR_ENV)
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".crimson-skies"))
                })?;
            ProfileSession::open(
                &base,
                crate::profile::SessionOrigin::Interactive,
                cs_types::profile::ProfileKind::Production,
                &catalog,
            )
        }
    };
    match opened {
        Ok(session) => Some(PilotSetup { page, session }),
        Err(error) => {
            eprintln!("cs: the pilot population cannot open: {error}");
            None
        }
    }
}

/// Turns the finished smoke into the process result and prints the summary.
///
/// # Errors
///
/// [`FrontEndError::SmokeFailed`] when any check failed,
/// [`FrontEndError::Exit`] when the run produced no report.
#[inline(always)]
pub(super) fn finish(
    handle: &ShellSmokeHandle,
    request: &SmokeRequest,
) -> Result<ShellSmokeReport, FrontEndError> {
    let outcome = handle
        .take()
        .ok_or_else(|| FrontEndError::Exit("the smoke run ended before it finished".to_owned()))?;
    let report = outcome.map_err(FrontEndError::Exit)?;
    println!(
        "front-end smoke: {}\n  frames {} visited [{}] focus moves {} unhandled {} cursor {}\n  movies [{}] movie frames {} framebuffers {} in {}\n  music {} audio [{}]",
        report.visited.join(" → "),
        report.frames,
        report.visited.join(", "),
        report.focus_moves,
        report.unhandled,
        report.cursor.as_deref().unwrap_or("none recorded"),
        report.movies.join(", "),
        report.movie_frames,
        report.shots.len(),
        request.capture_dir.display(),
        if report.music_started {
            "playing"
        } else {
            "silent"
        },
        report.audio.join("; "),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// BUG-FE-CURSOR-TOPLEFT (#1995): the arrow's tip, the sprite's top-left
    /// corner, sits on the pointer position the hit tests receive — for any
    /// cursor art extent and window size, not the image centre.
    #[test]
    fn accept_bug_fe_cursor_topleft_tip_is_the_pointer() {
        let mut world = World::new();
        let cursor = world.spawn(cursor_bundle(Handle::default())).id();
        assert_eq!(
            world.get::<Anchor>(cursor).copied(),
            Some(Anchor::TOP_LEFT),
            "the production cursor sprite is anchored at its top-left"
        );
        for surface in [SECTION_SURFACE, (1920, 1080), (800, 900), (1024, 768)] {
            for extent in [(32, 32), (19, 27), (64, 48)] {
                for pointer in [
                    Vec2::new(0.0, 0.0),
                    Vec2::new(123.0, 456.0),
                    Vec2::new(surface.0 as f32 - 1.0, surface.1 as f32 - 1.0),
                ] {
                    let (size, translation) = cursor_placement(pointer, extent, surface);
                    let tip_x = translation.x + surface.0 as f32 * 0.5;
                    let tip_y = surface.1 as f32 * 0.5 - translation.y;
                    assert_eq!(Vec2::new(tip_x, tip_y), pointer, "{surface:?} {extent:?}");
                    let scale = fit_scale(surface);
                    assert_eq!(
                        size,
                        Vec2::new(extent.0 as f32 * scale, extent.1 as f32 * scale)
                    );
                }
            }
        }
    }
}
