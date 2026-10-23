//! Acceptance task FE-MENU-4 (#1836): the front end's music and
//! click/rollover/text-entry cues.
//!
//! The `accept_fe_menu_4_` tests call the production module
//! (`cs_app::ui::front_end::sound`) and the production presenter:
//! [`MenuSoundTable`] reads `GLOBALS.SCRIPT`'s `sound` declarations and the
//! music control, [`MenuAudio`] plays them over any audio device, and the
//! presenter's frame systems start the music and fire the cues from the
//! shell's own button states and the pilot page's typing.
//!
//! * The synthetic tests run a script in the original's *format* over a
//!   recording device: they prove the reading, the bus/loop/one-shot shape
//!   of each command and the presenter's triggers, in CI. They prove nothing
//!   about the retail data.
//! * The `retail` test reads `$CS_GAME_DIR`: the real declarations and the
//!   decoded members. `#[ignore]`d because CI has no installation.
//! * The windowed test drives `cs --front-end --smoke-seconds` on a GPU and
//!   the machine's real output and checks the report's audio record.
//!   `#[ignore]`d because CI has neither.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bevy::app::App;
use bevy::ecs::message::MessageWriter;
use bevy::input::ButtonInput;
use bevy::input::ButtonState;
use bevy::input::InputSystems;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput, NativeKeyCode};
use bevy::input::mouse::MouseButton;
use bevy::prelude::{Entity, IntoScheduleConfigs, PreUpdate, ResMut, Resource, Vec2, With};
use bevy::window::{CursorMoved, PrimaryWindow};

use cs_app::profile::ProfileSession;
use cs_app::ui::front_end::{
    FrontEndScreens, MenuAudio, MenuAudioEvent, MenuCue, MenuShell, MenuSoundError, MenuSoundTable,
    OriginalLayout, PilotPage, PilotSetup, SECTION_SURFACE, START_SECTION, ShellResource,
    StartFlow, cue_sample_id, headless_app_audio, music_sample_id,
};
use cs_content::save::settings::SettingCatalog;
use cs_sim::audio_events::{AudioBus, DeviceCommand, RecordingAudioDevice};

// ------------------------------------------------------- the synthetic half ---

/// `GLOBALS.SCRIPT`'s shape: six `sound` statements and the music control.
const SCRIPT: &str = "\
main
{
gui_create
{
int XQ = 0
sound DR = \"assets\\\\sounds\\\\\" \"mouseclick.wav\"
sound ER = \"assets\\\\sounds\\\\\" \"mouseover.wav\"
sound FR = \"assets\\\\sounds\\\\\" \"mouseclick.wav\"
sound GR = \"assets\\\\sounds\\\\\" \"mouseover.wav\"
sound HR = \"assets\\\\sounds\\\\\" \"entertext.wav\"
sound IR = \"assets\\\\sounds\\\\\" \"entertext_error.wav\"
setvolume(1, JR)
object RR = @ctl@SK
RR.UK = \"music_splash.wav\"
RR.WK = 5
RR.XK = JR
RR.ZB = 0
initialize(RR)
}
}
";

const LAYOUT: &str = "\
[@MainMenu@]
MM_B_CAMPAIGN=B,strip.png,10,10,0,0,Campaign,0x1000,1
MM_B_QUIT=B,strip.png,10,200,0,0,,0x1000,0

[@Campaign@]
CM_B_START=B,strip.png,300,10,0,0,,0x1000,0
CM_B_CANCEL=B,strip.png,300,110,0,0,MainMenu,0x1000,0
CM_E_NAME=E,FONT_CM_EDIT,20,20,0,200,22,4,0xFFFF0000,,0xFFEF0010
CM_L_PLAYERS=L,bar.png,up.png,down.png,20,60,0,200,20,3
";

fn extents(art: &str) -> Option<(u32, u32)> {
    match art.to_ascii_lowercase().as_str() {
        "strip.png" => Some((80, 160)),
        _ => None,
    }
}

fn table() -> MenuSoundTable {
    MenuSoundTable::parse(SCRIPT).expect("the fixture script parses")
}

#[test]
fn accept_fe_menu_4_a_the_table_reads_the_scripts_sounds_and_music() {
    let table = table();
    let declared: Vec<(&str, &str)> = table
        .sounds
        .iter()
        .map(|sound| (sound.variable.as_str(), sound.file.as_str()))
        .collect();
    assert_eq!(
        declared,
        [
            ("DR", "mouseclick.wav"),
            ("ER", "mouseover.wav"),
            ("FR", "mouseclick.wav"),
            ("GR", "mouseover.wav"),
            ("HR", "entertext.wav"),
            ("IR", "entertext_error.wav"),
        ],
        "script order, files verbatim"
    );
    assert_eq!(table.music.variable, "RR");
    assert_eq!(table.music.file, "music_splash.wav");
    let properties: Vec<&str> = table
        .music
        .properties
        .iter()
        .map(|(property, _)| property.as_str())
        .collect();
    assert_eq!(
        properties,
        ["WK", "XK", "ZB"],
        "the control's other assignments are kept unread"
    );
    for cue in MenuCue::ALL {
        assert!(table.declared(cue).is_some(), "{cue:?} is declared");
    }

    // A script that does not state the sounds is refused, never defaulted.
    assert_eq!(
        MenuSoundTable::parse("main\n{\n}\n").unwrap_err(),
        MenuSoundError::NoSounds
    );
    let no_music = SCRIPT.replace("RR.UK = \"music_splash.wav\"", "RR.UK = 7");
    assert_eq!(
        MenuSoundTable::parse(&no_music).unwrap_err(),
        MenuSoundError::NoMusic
    );
    let no_control = SCRIPT.replace("@ctl@SK", "@ctl@ZZ");
    assert_eq!(
        MenuSoundTable::parse(&no_control).unwrap_err(),
        MenuSoundError::NoMusic
    );
    let torn = SCRIPT.replace("\"mouseover.wav\"", "mouseover.wav");
    assert!(matches!(
        MenuSoundTable::parse(&torn).unwrap_err(),
        MenuSoundError::Malformed { .. }
    ));
    // A file the script never declares has no cue.
    let no_error = SCRIPT.replace("entertext_error.wav", "other.wav");
    assert!(
        MenuSoundTable::parse(&no_error)
            .unwrap()
            .declared(MenuCue::TextRejected)
            .is_none()
    );
}

fn audio() -> (MenuAudio, RecordingAudioDevice) {
    let device = RecordingAudioDevice::new();
    let handle = device.handle();
    let audio = MenuAudio::new(Box::new(device), &table()).expect("the device opens");
    (audio, handle)
}

#[test]
fn accept_fe_menu_4_b_music_loops_on_the_music_bus_and_cues_play_once_on_ui() {
    let (mut audio, device) = audio();
    audio.start_music();
    audio.start_music();
    let started: Vec<_> = device
        .commands()
        .into_iter()
        .filter_map(|command| match command {
            DeviceCommand::Started { start, .. } => Some(start),
            _ => None,
        })
        .collect();
    assert_eq!(started.len(), 1, "a second start is a no-op");
    assert_eq!(started[0].bus, AudioBus::Music);
    assert_eq!(
        started[0].asset,
        music_sample_id("music_splash.wav").unwrap()
    );
    assert!(audio.music_playing());

    audio.cue(MenuCue::Rollover);
    audio.cue(MenuCue::Press);
    let one_shots: Vec<_> = device
        .commands()
        .into_iter()
        .filter_map(|command| match command {
            DeviceCommand::OneShotStarted { start, .. } => Some(start),
            _ => None,
        })
        .collect();
    assert_eq!(one_shots.len(), 2);
    assert!(one_shots.iter().all(|start| start.bus == AudioBus::Ui));
    assert_eq!(one_shots[0].asset, cue_sample_id("mouseover.wav").unwrap());
    assert_eq!(one_shots[1].asset, cue_sample_id("mouseclick.wav").unwrap());
    assert_ne!(
        cue_sample_id("mouseover.wav").unwrap(),
        music_sample_id("music_splash.wav").unwrap()
    );

    audio.stop_music();
    assert!(!audio.music_playing());
    assert!(
        device
            .commands()
            .iter()
            .any(|command| matches!(command, DeviceCommand::Stopped { .. })),
        "the loop was stopped on the device"
    );
}

#[test]
fn accept_fe_menu_4_c_a_refused_command_is_recorded_by_code_never_silent() {
    // A device that was closed behind the audio's back refuses by name.
    let device = RecordingAudioDevice::new();
    let mut handle = device.handle();
    let mut audio = MenuAudio::new(Box::new(device), &table()).expect("opens");
    cs_sim::audio_events::AudioDevice::close(&mut handle);
    audio.start_music();
    audio.cue(MenuCue::Press);
    assert!(!audio.music_playing());
    assert_eq!(audio.cue_count(MenuCue::Press), 0);
    let refused: Vec<_> = audio
        .events()
        .iter()
        .filter_map(|event| match event {
            MenuAudioEvent::Refused { what, code } => Some((what.as_str(), *code)),
            _ => None,
        })
        .collect();
    assert_eq!(refused, [("music", "closed"), ("press", "closed")]);

    // A cue whose file the script never declared is refused by name.
    let no_error = SCRIPT.replace("entertext_error.wav", "other.wav");
    let table = MenuSoundTable::parse(&no_error).unwrap();
    let mut audio = MenuAudio::new(Box::new(RecordingAudioDevice::new()), &table).unwrap();
    audio.cue(MenuCue::TextRejected);
    assert_eq!(
        audio.events(),
        [MenuAudioEvent::Refused {
            what: "text_rejected".to_owned(),
            code: "cue_not_declared"
        }]
    );
}

// -------------------------------------------------- the headless presenter ---

static NEXT: AtomicU32 = AtomicU32::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "cs-fe-menu-4-{}-{}",
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
    mouse: Vec<bool>,
    keys: Vec<(KeyCode, bool)>,
    typed: Vec<KeyboardInput>,
}

fn inject(
    keys: ResMut<ButtonInput<KeyCode>>,
    mouse: ResMut<ButtonInput<MouseButton>>,
    mut cursor: MessageWriter<CursorMoved>,
    mut typed: MessageWriter<KeyboardInput>,
    mut queue: ResMut<TestInput>,
    window: bevy::prelude::Query<Entity, With<PrimaryWindow>>,
) {
    let mut keys = keys;
    keys.clear();
    for (key, down) in queue.keys.drain(..) {
        if down {
            keys.press(key);
        } else {
            keys.release(key);
        }
    }
    let mut mouse = mouse;
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

fn type_text(app: &mut App, text: &str) {
    for character in text.chars() {
        let typed = character.to_string();
        drive(app, |queue| {
            queue.typed.push(KeyboardInput {
                key_code: KeyCode::Unidentified(NativeKeyCode::Unidentified),
                logical_key: Key::Character(typed.as_str().into()),
                state: ButtonState::Pressed,
                text: Some(typed.as_str().into()),
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
        });
    }
}

fn centre(app: &App, id: &str) -> Vec2 {
    let state = app.world().resource::<ShellResource>();
    let view = state.shell.view(SECTION_SURFACE).expect("a view");
    let button = view
        .buttons
        .iter()
        .find(|button| button.id.eq_ignore_ascii_case(id))
        .unwrap_or_else(|| panic!("the section shows {id}"));
    Vec2::new(
        button.rect.x as f32 + button.rect.width as f32 * 0.5,
        button.rect.y as f32 + button.rect.height as f32 * 0.5,
    )
}

fn hover(app: &mut App, id: &str) {
    let at = centre(app, id);
    drive(app, |queue| queue.cursor = Some(at));
}

fn press(app: &mut App, id: &str) {
    let at = centre(app, id);
    drive(app, |queue| {
        queue.cursor = Some(at);
        queue.mouse.push(true);
    });
    drive(app, |queue| queue.mouse.push(false));
}

fn presenter(dir: &Path) -> (App, RecordingAudioDevice) {
    let layout = OriginalLayout::parse(LAYOUT).expect("the fixture parses");
    let no_scripts = |_: &str| -> Option<String> { None };
    let mut shell = MenuShell::build(&layout, &extents, &no_scripts);
    shell.open(START_SECTION).expect("MainMenu is a section");
    let setup = PilotSetup {
        page: PilotPage::from_layout(&layout).expect("the rows"),
        session: ProfileSession::open_sandbox(dir, &SettingCatalog::empty()).expect("sandbox"),
    };
    let (audio, device) = audio();
    let mut app = headless_app_audio(shell, Some(StartFlow::new()), setup, audio);
    app.init_resource::<TestInput>();
    app.add_systems(PreUpdate, inject.after(InputSystems));
    app.update();
    (app, device)
}

fn cues(app: &App) -> Vec<MenuCue> {
    app.world()
        .resource::<ShellResource>()
        .audio()
        .expect("the presenter holds the audio")
        .events()
        .iter()
        .filter_map(|event| match event {
            MenuAudioEvent::Cue(cue) => Some(*cue),
            _ => None,
        })
        .collect()
}

#[test]
fn accept_fe_menu_4_d_the_presenter_starts_the_music_once_and_cues_hover_and_press() {
    let dir = TempDir::new();
    let (mut app, device) = presenter(dir.path());
    let loops = |device: &RecordingAudioDevice| {
        device
            .commands()
            .iter()
            .filter(|command| matches!(command, DeviceCommand::Started { .. }))
            .count()
    };
    assert_eq!(loops(&device), 1, "the music starts with the front end");
    assert!(cues(&app).is_empty(), "nothing is under the pointer yet");

    // The first button holds the initial focus without a cue; the pointer
    // going to another button is the first rollover.
    hover(&mut app, "MM_B_QUIT");
    assert_eq!(cues(&app), [MenuCue::Rollover]);
    hover(&mut app, "MM_B_QUIT");
    assert_eq!(
        cues(&app),
        [MenuCue::Rollover],
        "staying on a button is silent"
    );

    press(&mut app, "MM_B_CAMPAIGN");
    assert_eq!(
        cues(&app),
        [MenuCue::Rollover, MenuCue::Rollover, MenuCue::Press],
        "entering the button, then the press; the release is silent"
    );
    // The Campaign screen is up: the music went on, uninterrupted.
    assert_eq!(
        loops(&device),
        1,
        "changing screen does not restart the music"
    );
    assert!(
        !device
            .commands()
            .iter()
            .any(|command| matches!(command, DeviceCommand::Stopped { .. })),
        "nothing stopped the music"
    );

    // The new screen's initial focus is silent; a pointer move is not.
    assert_eq!(cues(&app).len(), 3);
    hover(&mut app, "CM_B_CANCEL");
    assert_eq!(cues(&app).len(), 4);
    assert_eq!(cues(&app).last(), Some(&MenuCue::Rollover));
}

#[test]
fn accept_fe_menu_4_e_typing_on_the_pilot_page_cues_accepted_and_refused_characters() {
    let dir = TempDir::new();
    let (mut app, _device) = presenter(dir.path());
    // Typed on the menu: the page is not up, so no entry cue.
    type_text(&mut app, "x");
    assert!(cues(&app).is_empty());

    press(&mut app, "MM_B_CAMPAIGN");
    let base = cues(&app).len();
    // MaxChars is 4 in the fixture: four accepted, then the box is full.
    type_text(&mut app, "Ace");
    type_text(&mut app, "X");
    type_text(&mut app, "Y");
    let typed: Vec<MenuCue> = cues(&app)[base..].to_vec();
    assert_eq!(
        typed,
        [
            MenuCue::TextAccepted,
            MenuCue::TextAccepted,
            MenuCue::TextAccepted,
            MenuCue::TextAccepted,
            MenuCue::TextRejected,
        ],
        "accepted until the box is full, then refused"
    );
}

// --------------------------------------------------------- the retail half ---

/// **The retail half.** The installation's own `GLOBALS.SCRIPT`: the six
/// declarations and the music control as written, and every member they
/// name decoded by the production decoder.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_fe_menu_4_retail_the_globals_script_names_members_that_decode() {
    let dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must point at the original installation"),
    );
    let screens = FrontEndScreens::open(&dir).expect("the original screen container opens");
    let table = MenuSoundTable::from_screens(&screens).expect("GLOBALS.SCRIPT reads");
    let files: Vec<&str> = table.sounds.iter().map(|s| s.file.as_str()).collect();
    assert_eq!(
        files,
        [
            "mouseclick.wav",
            "mouseover.wav",
            "mouseclick.wav",
            "mouseover.wav",
            "entertext.wav",
            "entertext_error.wav"
        ]
    );
    assert_eq!(table.music.file, "music_splash.wav");
    assert!(
        !table.music.properties.is_empty(),
        "the control's numeric properties are recorded, unread"
    );

    let samples = cs_app::ui::front_end::MenuSamples::load(&screens, &table);
    assert!(samples.failures.is_empty(), "{:?}", samples.failures);
    assert_eq!(samples.music_source.as_deref(), Some("crimson.rof"));
    let library = &samples.library;
    for cue in MenuCue::ALL {
        let pcm = cs_app::audio::SampleLibrary::pcm(
            library,
            &cue_sample_id(&table.declared(cue).unwrap().file).unwrap(),
        )
        .unwrap_or_else(|| panic!("{cue:?} decoded"));
        assert!(pcm.duration().as_secs_f64() < 1.0, "{cue:?} is a short cue");
        assert!(
            pcm.samples().iter().any(|sample| sample.abs() > 0.001),
            "{cue:?} is not silence"
        );
    }
    let music =
        cs_app::audio::SampleLibrary::pcm(library, &music_sample_id(&table.music.file).unwrap())
            .expect("the music decoded");
    assert!(music.duration().as_secs() > 30, "a track, not a cue");
    assert!(music.samples().iter().any(|sample| sample.abs() > 0.05));

    // The two copies of the track: the container's and the archive's both
    // exist, so the choice between them is a recorded unknown.
    let archive = screens
        .sound_member("ZBD/soundsh.zbd", &table.music.file)
        .expect("the archive holds a copy too");
    assert!(archive.pcm.duration().as_secs() > 30);
}

// --------------------------------------------------------- the windowed run ---

fn front_end_bin() -> PathBuf {
    if let Ok(bin) = std::env::var("CS_FRONT_END_BIN") {
        return PathBuf::from(bin);
    }
    PathBuf::from(env!("CARGO_BIN_EXE_cs"))
}

/// **The windowed run.** `cs --cs-path $CS_GAME_DIR --front-end
/// --smoke-seconds 14 --capture-dir <dir>` on this machine's real audio
/// output: the report must say the music loop started and the scripted
/// pointer and typing produced rollover, press and text-entry cues. The
/// assertions are the harness contract (what the output was asked), not a
/// listener's verdict.
#[test]
#[ignore = "requires CS_GAME_DIR + gpu + audio and a built windowed binary"]
fn accept_fe_menu_4_the_windowed_smoke_plays_the_music_and_the_cues() {
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is unset: this run needs the original installation");
    };
    let Ok(capabilities) = std::env::var("CS_CAPABILITIES") else {
        panic!("CS_CAPABILITIES is unset: the front-end launch needs gpu and audio");
    };
    for needed in ["gpu", "audio"] {
        assert!(
            capabilities.split(',').any(|part| part.trim() == needed),
            "CS_CAPABILITIES must list {needed}: {capabilities}"
        );
    }
    let dir = std::env::var("CS_FE_MENU_SMOKE_OUT").map_or_else(
        |_| std::env::temp_dir().join("fe-menu-4-smoke"),
        PathBuf::from,
    );
    let _ = std::fs::remove_dir_all(dir.join(cs_app::ui::front_end::SMOKE_PROFILE_SUBDIR));
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == "report.json" || name.ends_with(".png") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    std::fs::create_dir_all(&dir).expect("the capture directory is created");
    let output = std::process::Command::new(front_end_bin())
        .args(["--cs-path", &game_dir, "--front-end"])
        .args(["--smoke-seconds", "14", "--capture-dir"])
        .arg(&dir)
        .env("CS_CAPABILITIES", capabilities)
        .output()
        .expect("the cs binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let report = std::fs::read_to_string(dir.join("report.json"))
        .unwrap_or_else(|error| panic!("report.json is written: {error}\n{stderr}"));
    assert!(
        report.contains("\"passed\":true"),
        "the run passed its own checks: {report}\nstderr: {stderr}"
    );
    assert!(
        report.contains("\"music_started\":true"),
        "the music loop started on the output: {report}"
    );
    for cue in ["rollover", "press", "text_accepted"] {
        assert!(
            report.contains(&format!("cue {cue} x")),
            "the report records the {cue} cue: {report}"
        );
    }
    assert!(
        !report.contains("refused ") && !report.contains("no audio output"),
        "the output refused nothing: {report}"
    );
    assert!(output.status.success(), "exit 0; stderr: {stderr}");
}
