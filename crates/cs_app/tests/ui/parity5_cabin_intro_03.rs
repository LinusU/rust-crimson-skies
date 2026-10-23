//! Task #1839 `PARITY5-CABIN-INTRO.03`: the intro movie between the pilot page
//! and the cabin. Task test prefix: `accept_parity5_cabin_intro_03_`.
//!
//! The tests drive the production headless presenter ([`headless_app_intro`])
//! through its real input path over a synthetic one-frame MPEG-1 stream (the
//! one `parity5_cabin_intro_02` builds) and a sandbox pilot population, so CI
//! runs them. They prove the flow, not the retail movie: the retail `chap1.mpg`
//! is played by task 02's retail test and shown by the smoke of task 06. Not
//! claimed: the once-per-pilot rule (callback 2151 is unread), see
//! `src/ui/front_end/intro.rs`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use bevy::asset::AssetPlugin;
use bevy::ecs::message::MessageWriter;
use bevy::input::ButtonInput;
use bevy::input::ButtonState;
use bevy::input::InputSystems;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput, NativeKeyCode};
use bevy::input::mouse::MouseButton;
use bevy::prelude::{
    App, AssetApp, Entity, Image, IntoScheduleConfigs, PreUpdate, ResMut, Resource, Vec2, With,
};
use bevy::window::{CursorMoved, PrimaryWindow};

use cs_app::cinematics::movie_player::{AudioOutput, MovieOutcome, MoviePresenter};
use cs_app::profile::ProfileSession;
use cs_app::ui::front_end::{
    IntroMovie, MenuShell, OriginalLayout, PilotPage, PilotSetup, SECTION_SURFACE, START_SECTION,
    Screen, ShellResource, StartFlow, StartStep, headless_app_intro, intro_follows,
};
use cs_content::save::settings::SettingCatalog;

const LAYOUT: &str = "\
[@MainMenu@]
MM_B_CAMPAIGN=B,strip.png,10,10,0,0,Campaign,0x1000,1

[@Campaign@]
CM_BACKGROUND=P,pane.png,0,0,0,1,0,1,0
CM_B_START=B,strip.png,300,10,0,0,,0x1000,0
CM_B_DELETEPLAYER=B,strip.png,300,60,0,0,,0x1000,0
CM_B_CANCEL=B,strip.png,300,110,0,0,MainMenu,0x1000,0
CM_E_NAME=E,FONT_CM_EDIT,20,20,0,200,22,8,0xFFFF0000,,0xFFEF0010
CM_L_PLAYERS=L,bar.png,up.png,down.png,20,60,0,200,20,3

[@PassengerCabin@]
PC_B_BACK=B,strip.png,5,5,0,0,MainMenu,0x1000,1
";

fn extents(art: &str) -> Option<(u32, u32)> {
    match art.to_ascii_lowercase().as_str() {
        "pane.png" => Some((400, 300)),
        "strip.png" => Some((80, 160)),
        _ => None,
    }
}

static NEXT: AtomicU32 = AtomicU32::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "cs-parity5-cabin-intro-03-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the temp directory is created");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Resource, Default)]
struct TestInput {
    cursor: Option<Vec2>,
    keys: Vec<(KeyCode, bool)>,
    mouse: Vec<bool>,
    typed: Vec<KeyboardInput>,
}

fn inject(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut cursor: MessageWriter<CursorMoved>,
    mut typed: MessageWriter<KeyboardInput>,
    mut queue: ResMut<TestInput>,
    window: bevy::prelude::Query<Entity, With<PrimaryWindow>>,
) {
    keys.clear();
    for (key, down) in queue.keys.drain(..) {
        if down {
            keys.press(key);
        } else {
            keys.release(key);
        }
    }
    mouse.clear();
    for down in queue.mouse.drain(..) {
        if down {
            mouse.press(MouseButton::Left);
        } else {
            mouse.release(MouseButton::Left);
        }
    }
    for event in queue.typed.drain(..) {
        typed.write(event);
    }
    if let Some(position) = queue.cursor.take() {
        cursor.write(CursorMoved {
            window: window.single().unwrap_or(Entity::PLACEHOLDER),
            position,
            delta: None,
        });
    }
}

fn drive(app: &mut App, batch: impl FnOnce(&mut TestInput)) {
    batch(&mut app.world_mut().resource_mut::<TestInput>());
    app.update();
}

fn tap(app: &mut App, key: KeyCode) {
    drive(app, |queue| queue.keys.push((key, true)));
    drive(app, |queue| queue.keys.push((key, false)));
}

fn type_name(app: &mut App, name: &str) {
    for character in name.chars() {
        let text = character.to_string();
        drive(app, |queue| {
            queue.typed.push(KeyboardInput {
                key_code: KeyCode::Unidentified(NativeKeyCode::Unidentified),
                logical_key: Key::Character(text.as_str().into()),
                state: ButtonState::Pressed,
                text: Some(text.as_str().into()),
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
        });
    }
}

fn press_campaign(app: &mut App) {
    let (x, y) = {
        let state = app.world().resource::<ShellResource>();
        let view = state.shell.view(SECTION_SURFACE).expect("a view");
        let button = view
            .buttons
            .iter()
            .find(|button| button.id.eq_ignore_ascii_case("MM_B_CAMPAIGN"))
            .expect("the main menu shows Campaign");
        (
            button.rect.x as f32 + button.rect.width as f32 * 0.5,
            button.rect.y as f32 + button.rect.height as f32 * 0.5,
        )
    };
    drive(app, |queue| {
        queue.cursor = Some(Vec2::new(x, y));
        queue.mouse.push(true);
    });
    drive(app, |queue| queue.mouse.push(false));
}

fn movie_file(dir: &Path) -> PathBuf {
    let path = dir.join("intro.mpg");
    std::fs::write(&path, super::parity5_cabin_intro_02::program_stream()).unwrap();
    path
}

/// An app on the pilot page with `movie` as the intro.
fn pilot_app(dir: &Path, movie: PathBuf) -> App {
    let layout = OriginalLayout::parse(LAYOUT).expect("the fixture parses");
    let no_scripts = |_: &str| -> Option<String> { None };
    let mut shell = MenuShell::build(&layout, &extents, &no_scripts);
    shell.open(START_SECTION).expect("MainMenu is a section");
    let setup = PilotSetup {
        page: PilotPage::from_layout(&layout).expect("the rows"),
        session: ProfileSession::open_sandbox(&dir.join("profiles"), &SettingCatalog::empty())
            .expect("the sandbox opens"),
    };
    let movie = IntroMovie {
        path: movie,
        output: AudioOutput::Silent,
    };
    let mut app = headless_app_intro(shell, Some(StartFlow::new()), setup, movie);
    app.add_plugins(AssetPlugin::default())
        .init_asset::<Image>();
    app.init_resource::<TestInput>();
    app.add_systems(PreUpdate, inject.after(InputSystems));
    app.update();
    press_campaign(&mut app);
    app
}

fn screen(app: &App) -> Screen {
    app.world()
        .resource::<ShellResource>()
        .flow()
        .expect("the flow")
        .screen()
}

fn outcomes(app: &App) -> Vec<MovieOutcome> {
    app.world()
        .resource::<ShellResource>()
        .intro()
        .outcomes
        .clone()
}

/// Types a pilot name and confirms it with Enter; returns once the machine is
/// in the cabin.
fn confirm_pilot(app: &mut App) {
    assert_eq!(screen(app), Screen::ProfileSelect);
    type_name(app, "Eve");
    tap(app, KeyCode::Enter);
    assert_eq!(screen(app), Screen::Cabin);
}

fn run_until_idle(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.world().resource::<ShellResource>().intro().active() {
        assert!(Instant::now() < deadline, "the intro never ended");
        app.update();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn accept_parity5_cabin_intro_03_the_intro_plays_after_the_pilot_and_ends_in_the_cabin() {
    let dir = TempDir::new("natural");
    let movie = movie_file(dir.path());
    let mut app = pilot_app(dir.path(), movie);
    confirm_pilot(&mut app);

    // The confirming Enter must not skip the movie it starts: it is up on the
    // next frames, with the cabin entered behind it.
    assert!(app.world().resource::<ShellResource>().intro().active());
    app.update();
    assert!(app.world().resource::<MoviePresenter>().is_playing());
    assert_eq!(screen(&app), Screen::Cabin);

    run_until_idle(&mut app);
    assert_eq!(outcomes(&app), vec![MovieOutcome::Completed]);
    assert!(!app.world().resource::<MoviePresenter>().is_playing());
    assert_eq!(screen(&app), Screen::Cabin);
}

#[test]
fn accept_parity5_cabin_intro_03_escape_and_click_skip_to_the_cabin_without_leaving_it() {
    for skip in ["escape", "click"] {
        let dir = TempDir::new(skip);
        let movie = movie_file(dir.path());
        let mut app = pilot_app(dir.path(), movie);
        confirm_pilot(&mut app);
        app.update();
        assert!(app.world().resource::<MoviePresenter>().is_playing());

        if skip == "escape" {
            drive(&mut app, |queue| queue.keys.push((KeyCode::Escape, true)));
            drive(&mut app, |queue| queue.keys.push((KeyCode::Escape, false)));
        } else {
            drive(&mut app, |queue| queue.mouse.push(true));
            drive(&mut app, |queue| queue.mouse.push(false));
        }
        run_until_idle(&mut app);
        assert_eq!(outcomes(&app), vec![MovieOutcome::Skipped], "{skip}");
        assert!(!app.world().resource::<MoviePresenter>().is_playing());
        assert_eq!(
            screen(&app),
            Screen::Cabin,
            "{skip} must not leave the cabin"
        );
    }
}

#[test]
fn accept_parity5_cabin_intro_03_a_movie_that_cannot_open_is_recorded_and_the_cabin_follows() {
    let dir = TempDir::new("missing");
    let mut app = pilot_app(dir.path(), dir.path().join("absent.mpg"));
    confirm_pilot(&mut app);
    run_until_idle(&mut app);
    let outcomes = outcomes(&app);
    assert!(
        matches!(outcomes.as_slice(), [MovieOutcome::Failed(_)]),
        "{outcomes:?}"
    );
    assert!(!app.world().resource::<MoviePresenter>().is_playing());
    assert_eq!(screen(&app), Screen::Cabin);
}

#[test]
fn accept_parity5_cabin_intro_03_only_the_pilot_page_entry_plays_the_intro() {
    let from = |from, to| intro_follows(&StartStep::Moved { from, to });
    assert!(from(Screen::ProfileSelect, Screen::Cabin));
    for back in [
        Screen::Scrapbook,
        Screen::Briefing,
        Screen::Construction,
        Screen::Results,
    ] {
        assert!(
            !from(back, Screen::Cabin),
            "{back:?} returns skip the intro"
        );
    }
    assert!(!from(Screen::MainMenu, Screen::ProfileSelect));
    assert!(!intro_follows(&StartStep::Stay));
}
