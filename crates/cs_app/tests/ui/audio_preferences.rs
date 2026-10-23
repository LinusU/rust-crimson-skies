//! Acceptance task BUG-AUDIO-PREFERENCES-UI (#1996): the Preferences → Audio
//! page.
//!
//! The `accept_bug_audio_preferences_ui_` tests call the production page:
//! [`AudioPage`] reads the `T`, `Z` and `D` rows of the `Audio` section, the
//! shell presents its check-box button, and the headless presenter takes the
//! pointer and keyboard through the same path the windowed run uses —
//! including a real window size so the 800x600 mapping is exercised at
//! several surfaces.
//!
//! * The synthetic tests run over layout text in the original's *format*
//!   (the real column layout, invented art extents). They prove the page's
//!   rules and run in CI; they prove nothing about the retail data.
//! * The `retail` test reads `$CS_GAME_DIR`: the real rows, labels and the
//!   check box's art. `#[ignore]`d because CI has no installation.
//! * The windowed test drives `cs --front-end` with
//!   `CS_FRONT_END_SMOKE_SCENE=audio` on a GPU and checks the page's
//!   framebuffers arrive. `#[ignore]`d because CI has neither.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bevy::app::App;
use bevy::ecs::message::MessageWriter;
use bevy::input::ButtonInput;
use bevy::input::InputSystems;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::prelude::{Entity, IntoScheduleConfigs, PreUpdate, ResMut, Resource, Vec2, With};
use bevy::window::{CursorMoved, PrimaryWindow, Window};

use cs_app::ui::front_end::audio_page::{
    ACCEPT_BUTTON, CANCEL_BUTTON, EFFECTS_SLIDER, MUSIC_BUTTON, MUSIC_SLIDER, SETTINGS_FILE_NAME,
};
use cs_app::ui::front_end::{
    AUDIO_SECTION, AudioControl, AudioPage, AudioPageError, AudioSettingsFile, AudioValues,
    FrontEndScreens, MenuAudio, MenuMix, MenuShell, MenuSoundTable, OriginalLayout,
    SECTION_SURFACE, START_SECTION, ShellResource, SoundQuality, headless_app_audio_page,
    script_member,
};
use cs_sim::audio_events::{DeviceCommand, RecordingAudioDevice};

// ------------------------------------------------------- the synthetic half ---

/// An invented layout in the original's format: the real section's row kinds
/// and column layout (the data rows omit the header's `ResID`, `HelpID`,
/// `TabOrder` and `ScriptPointer` columns, as the file does).
const LAYOUT: &str = "\
[@MainMenu@]
MM_B_PREFERENCES=B,strip.png,10,10,0,0,Preferences,0x1000,1

[@Preferences@]
PF_B_AUDIO=B,strip.png,10,10,0,0,Audio,0x1000,0,,,,,,
PF_B_RETURN=B,strip.png,10,300,0,0,MainMenu,0x1000,0

[@Audio@]
V1=GX,536
V2=TCOLOR,0xFFFFDDc4
V3=TITLEX,137
V4=DESCX,348
V5=DROPX,137
AP_BACKGROUND=P,pane.png,110,215,0,1,0,1,0
AP_B_CANCELCHANGES=B,strip.png,451,<GX>,0,0,Preferences,0x1020,1,,,,,,
AP_B_ACCEPTCHANGES=B,strip.png,219,<GX>,0,0,Preferences,0x1020,1,,,,,,
AP_B_MUSIC=B,check.png,259,273,0,0,,0x1000,0,,,,,1,0
AP_T_TITLE=T,IDS_AP_TITLE,122,228,50,200,28,0XFFC0BAAD,1
AP_T_MusicTitle=T,IDS_AP_MUSIC_TITLE,<TITLEX>,266,50,112,40,<TCOLOR>,0
AP_T_MusicDesc=T,IDS_AP_MUSIC_DESC,<DESCX>,278,0,310,40,<TCOLOR>,0
AP_T_Unanswered=T,IDS_AP_NOT_A_STRING,<DESCX>,300,0,310,40,<TCOLOR>,0
AP_D_SQuality=D,bar.png,up.png,down.png,dropup.png,dropdown.png,<DROPX>,505,0,144,17,4
AP_S_MVOLUME=Z,<DROPX>,350,100,1,100,50,slot.png,knob.png,0,-10,1,-10
AP_S_EVOLUME=Z,<DROPX>,407,100,1,100,50,slot.png,knob.png,0,-10,1,-10
AP_S_VVOLUME=Z,<DROPX>,461,100,1,100,50,slot.png,knob.png,0,-10,1,-10
";

fn extents(art: &str) -> Option<(u32, u32)> {
    match art.to_ascii_lowercase().as_str() {
        "pane.png" => Some((566, 366)),
        "strip.png" => Some((80, 168)),
        "check.png" => Some((45, 232)),
        "slot.png" => Some((171, 3)),
        "knob.png" => Some((43, 21)),
        _ => None,
    }
}

fn strings(name: &str) -> Option<String> {
    Some(
        match name {
            "IDS_AP_TITLE" => "[COUR12B]AUDIO",
            "IDS_AP_MUSIC_TITLE" => "[COUR12B]In-Game Music",
            "IDS_AP_MUSIC_DESC" => "[TREB9B]Select to hear in-game music.",
            "IDS_LOW" => "[COUR12B]Low",
            "IDS_HIGH" => "[COUR12B]High",
            _ => return None,
        }
        .to_owned(),
    )
}

fn layout() -> OriginalLayout {
    OriginalLayout::parse(LAYOUT).expect("the fixture parses")
}

fn page_in(dir: &Path) -> AudioPage {
    AudioPage::from_layout(
        &layout(),
        &strings,
        &extents,
        Some(AudioSettingsFile::in_directory(dir)),
    )
    .expect("the fixture has the rows")
}

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A fresh, empty directory the settings file lives in.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "cs-audio-prefs-{label}-{}-{}",
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

#[test]
fn accept_bug_audio_preferences_ui_a_the_page_reads_labels_sliders_and_the_list_as_written() {
    let dir = TempDir::new("rows");
    let page = page_in(dir.path());

    // Labels come from the string table, tag stripped and sized from the tag;
    // a name the table does not answer draws nothing (never invented text).
    let labels: Vec<(&str, &str, Option<u32>)> = page
        .labels()
        .iter()
        .map(|label| (label.id.as_str(), label.text.as_str(), label.size))
        .collect();
    assert_eq!(
        labels,
        [
            ("AP_T_TITLE", "AUDIO", Some(12)),
            ("AP_T_MUSICTITLE", "In-Game Music", Some(12)),
            ("AP_T_MUSICDESC", "Select to hear in-game music.", Some(9)),
        ]
    );
    let title = &page.labels()[0];
    assert_eq!(
        (
            title.rect.x,
            title.rect.y,
            title.rect.width,
            title.rect.height
        ),
        (122, 228, 200, 28)
    );
    assert_eq!((title.color, title.justify), (0xFFC0_BAAD, 1));
    // `<TITLEX>` / `<TCOLOR>` resolve through the section's own variables.
    assert_eq!(page.labels()[1].rect.x, 137);
    assert_eq!(page.labels()[1].color, 0xFFFF_DDC4);

    // The three sliders: 1..=100, authored 50, slot and knob measured.
    let slider = page.slider_of(AudioControl::MusicVolume).expect("a slider");
    assert_eq!((slider.min, slider.max, slider.initial), (1, 100, 50));
    assert_eq!((slider.x, slider.y), (137, 350));
    assert_eq!((slider.slot, slider.knob), ((171, 3), (43, 21)));
    assert_eq!(slider.knob_offset, (0, -10));
    assert_eq!(slider.knob_at(1), (137, 340), "the knob rests at the left");
    assert_eq!(
        slider.knob_at(100),
        (137 + 171 - 43, 340),
        "and stops at the slot's right end"
    );
    assert_eq!(slider.value_at(0), 1);
    assert_eq!(slider.value_at(10_000), 100);
    assert!(
        (49..=51).contains(&slider.value_at(137 + 21 + 64)),
        "the middle"
    );

    // The drop-down: the real column layout (no ScriptPointer).
    let dropdown = page.dropdown();
    assert_eq!(
        (
            dropdown.rect.x,
            dropdown.rect.y,
            dropdown.rect.width,
            dropdown.rect.height
        ),
        (137, 505, 144, 17)
    );
    assert_eq!(dropdown.displayed, 4);
    let items: Vec<&str> = page.items().iter().map(|(_, text)| text.as_str()).collect();
    assert_eq!(items, ["Low", "High"]);

    // A section without the rows is refused by name, not guessed.
    let without = OriginalLayout::parse(&LAYOUT.replace("AP_S_VVOLUME=Z", "OTHER=Z"))
        .expect("the variant parses");
    assert_eq!(
        AudioPage::from_layout(&without, &strings, &extents, None).unwrap_err(),
        AudioPageError::MissingRow("AP_S_VVOLUME")
    );
}

#[test]
fn accept_bug_audio_preferences_ui_b_the_check_box_is_an_eight_frame_toggle_not_a_four_frame_strip()
{
    let no_scripts = |_: &str| -> Option<String> { None };
    let mut shell = MenuShell::build(&layout(), &extents, &no_scripts);
    shell.open(AUDIO_SECTION).expect("Audio is a section");
    let section = shell.section().expect("a section");
    assert!(
        section.skipped.is_empty(),
        "no Audio row is skipped: {:?}",
        section.skipped
    );
    let check = section
        .buttons
        .iter()
        .find(|button| button.id == MUSIC_BUTTON)
        .expect("the check box is presented");
    assert_eq!(check.check, Some(false));
    assert_eq!(
        (check.rect.width, check.rect.height),
        (45, 29),
        "232 rows are eight frames of 29, not four of 58"
    );

    // The unchecked half is frames 0..4, the checked half 4..8.
    let offset = |shell: &MenuShell| {
        shell
            .view(SECTION_SURFACE)
            .expect("a view")
            .buttons
            .iter()
            .find(|button| button.id == MUSIC_BUTTON)
            .expect("drawn")
            .texel_offset
            .1
    };
    assert_eq!(
        offset(&shell),
        29,
        "the resting frame of the unchecked half"
    );
    assert!(shell.set_checked(MUSIC_BUTTON, true));
    assert_eq!(
        offset(&shell),
        29 * 5,
        "the resting frame of the checked half"
    );
    assert_eq!(shell.is_checked(MUSIC_BUTTON), Some(true));
    assert_eq!(shell.is_checked(ACCEPT_BUTTON), None, "a plain button");
}

// -------------------------------------------------- the headless presenter ---

#[derive(Resource, Default)]
struct TestInput {
    cursor: Option<Vec2>,
    keys: Vec<(KeyCode, bool)>,
    mouse: Vec<bool>,
}

fn inject(
    keys: ResMut<ButtonInput<KeyCode>>,
    mouse: ResMut<ButtonInput<MouseButton>>,
    mut cursor: MessageWriter<CursorMoved>,
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

/// The window's logical size, or the 800x600 surface of a headless drive.
struct Surface((u32, u32));

/// A window-space point for the logical 800x600 point `(x, y)`.
fn at(surface: &Surface, x: i32, y: i32) -> Vec2 {
    let fit = cs_content::ui_layout::AspectFit::new(SECTION_SURFACE, surface.0).expect("a fit");
    let rect = fit.map_rect(cs_content::ui_layout::Rect {
        x: x as u32,
        y: y as u32,
        width: 1,
        height: 1,
    });
    Vec2::new(rect.x as f32 + 0.5, rect.y as f32 + 0.5)
}

fn press_at(app: &mut App, surface: &Surface, x: i32, y: i32) {
    drive(app, |queue| {
        queue.cursor = Some(at(surface, x, y));
        queue.mouse.push(true);
    });
    drive(app, |queue| queue.mouse.push(false));
}

fn press_button(app: &mut App, surface: &Surface, id: &str) {
    let (x, y) = {
        let state = app.world().resource::<ShellResource>();
        let view = state.shell.view(surface.0).expect("a view");
        let button = view
            .buttons
            .iter()
            .find(|button| button.id.eq_ignore_ascii_case(id))
            .unwrap_or_else(|| panic!("the section shows {id}"));
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

fn state(app: &App) -> &ShellResource {
    app.world().resource::<ShellResource>()
}

fn page(app: &App) -> &AudioPage {
    state(app).audio_page().expect("the app has the Audio page")
}

fn section(app: &App) -> String {
    state(app)
        .shell
        .section_name()
        .unwrap_or_default()
        .to_owned()
}

/// The app on the main menu with the Audio page, optionally in a window of
/// `size` (logical px) and with a recording menu audio.
fn audio_app(dir: &Path, size: Option<(f32, f32)>, audio: Option<MenuAudio>) -> (App, Surface) {
    let layout = layout();
    let no_scripts = |_: &str| -> Option<String> { None };
    let mut shell = MenuShell::build(&layout, &extents, &no_scripts);
    shell.open(START_SECTION).expect("MainMenu is a section");
    let mut app = headless_app_audio_page(shell, None, page_in(dir), audio);
    app.init_resource::<TestInput>();
    app.add_systems(PreUpdate, inject.after(InputSystems));
    let surface = match size {
        Some((width, height)) => {
            app.world_mut().spawn((
                Window {
                    resolution: (width as u32, height as u32).into(),
                    ..Window::default()
                },
                PrimaryWindow,
            ));
            Surface((width as u32, height as u32))
        }
        None => Surface(SECTION_SURFACE),
    };
    app.update();
    (app, surface)
}

fn open_audio(app: &mut App, surface: &Surface) {
    press_button(app, surface, "MM_B_PREFERENCES");
    assert_eq!(section(app), "Preferences");
    press_button(app, surface, "PF_B_AUDIO");
    assert_eq!(section(app), AUDIO_SECTION);
}

#[test]
fn accept_bug_audio_preferences_ui_c_one_click_toggles_and_accept_commits_cancel_discards_and_reopen_reads_back()
 {
    let dir = TempDir::new("commit");
    let (mut app, surface) = audio_app(dir.path(), None, None);
    open_audio(&mut app, &surface);
    let defaults = page(&app).committed();
    assert_eq!(
        defaults,
        AudioValues {
            music_on: true,
            music_volume: 50,
            effects_volume: 50,
            voice_volume: 50,
            quality: SoundQuality::High,
        },
        "the layout's authored 50s, music on, High"
    );
    assert_eq!(state(&app).shell.is_checked(MUSIC_BUTTON), Some(true));

    // One click flips the check box — no double click.
    press_button(&mut app, &surface, MUSIC_BUTTON);
    assert!(!page(&app).staged().music_on);
    assert_eq!(state(&app).shell.is_checked(MUSIC_BUTTON), Some(false));
    assert!(page(&app).committed().music_on, "staged, not committed");

    // The sliders: a press on the slot sets the value.
    let slider = page(&app)
        .slider_of(AudioControl::EffectsVolume)
        .unwrap()
        .clone();
    press_at(&mut app, &surface, slider.x + 5, slider.y + 1);
    assert_eq!(page(&app).staged().effects_volume, 1);
    press_at(
        &mut app,
        &surface,
        slider.x + slider.slot.0 as i32 - 2,
        slider.y + 1,
    );
    assert_eq!(page(&app).staged().effects_volume, 100);

    // The drop-down: open, pick Low.
    let list = page(&app).dropdown().rect;
    press_at(&mut app, &surface, list.x + 10, list.y + 5);
    assert!(page(&app).is_open());
    let low = page(&app).item_rect(0);
    press_at(&mut app, &surface, low.x + 10, low.y + 5);
    assert!(!page(&app).is_open());
    assert_eq!(page(&app).staged().quality, SoundQuality::Low);

    // Accept commits and writes; the page is left for Preferences.
    press_button(&mut app, &surface, ACCEPT_BUTTON);
    assert_eq!(section(&app), "Preferences");
    let committed = page(&app).committed();
    assert_eq!(
        committed,
        AudioValues {
            music_on: false,
            music_volume: 50,
            effects_volume: 100,
            voice_volume: 50,
            quality: SoundQuality::Low,
        }
    );
    assert_eq!(page(&app).accepts().len(), 1);
    assert!(page(&app).accepts()[0].quality_changed);
    assert!(page(&app).accepts()[0].save_error.is_none());
    assert!(dir.path().join(SETTINGS_FILE_NAME).is_file());

    // Cancel discards: edit again, Cancel, reopen — the committed values.
    press_button(&mut app, &surface, "PF_B_AUDIO");
    assert_eq!(page(&app).staged(), committed, "a visit starts committed");
    assert_eq!(state(&app).shell.is_checked(MUSIC_BUTTON), Some(false));
    press_button(&mut app, &surface, MUSIC_BUTTON);
    press_at(&mut app, &surface, slider.x + 5, slider.y + 1);
    assert_ne!(page(&app).staged(), committed);
    press_button(&mut app, &surface, CANCEL_BUTTON);
    assert_eq!(section(&app), "Preferences");
    assert_eq!(page(&app).committed(), committed, "nothing was committed");
    assert_eq!(page(&app).accepts().len(), 1);
    press_button(&mut app, &surface, "PF_B_AUDIO");
    assert_eq!(page(&app).staged(), committed, "the edit is gone");
    assert_eq!(state(&app).shell.is_checked(MUSIC_BUTTON), Some(false));

    // A new run reads the file back: the edited values, not the defaults.
    let (mut again, again_surface) = audio_app(dir.path(), None, None);
    open_audio(&mut again, &again_surface);
    assert_eq!(page(&again).committed(), committed);
    assert_eq!(page(&again).staged(), committed);
    assert_eq!(state(&again).shell.is_checked(MUSIC_BUTTON), Some(false));
}

#[test]
fn accept_bug_audio_preferences_ui_d_pointer_mapping_holds_at_several_window_sizes() {
    for size in [
        (800.0, 600.0),
        (1600.0, 1200.0),
        (1024.0, 768.0),
        (1280.0, 720.0),
        (640.0, 960.0),
    ] {
        let dir = TempDir::new("sizes");
        let (mut app, surface) = audio_app(dir.path(), Some(size), None);
        open_audio(&mut app, &surface);
        let slider = page(&app)
            .slider_of(AudioControl::MusicVolume)
            .unwrap()
            .clone();
        let y = slider.y + 1;

        // A drag: press near the left end, move to the right end, release.
        drive(&mut app, |queue| {
            queue.cursor = Some(at(&surface, slider.x + 60, y));
            queue.mouse.push(true);
        });
        let early = page(&app).staged().music_volume;
        assert!(
            (20..=45).contains(&early),
            "{size:?}: a press 60 px in reads {early}"
        );
        drive(&mut app, |queue| {
            queue.cursor = Some(at(&surface, slider.x + slider.slot.0 as i32 + 30, y + 40));
        });
        assert_eq!(
            page(&app).staged().music_volume,
            100,
            "{size:?}: the held slider follows the pointer past its end"
        );
        drive(&mut app, |queue| queue.mouse.push(false));
        assert!(!page(&app).dragging());
        drive(&mut app, |queue| {
            queue.cursor = Some(at(&surface, slider.x + 5, y));
        });
        assert_eq!(
            page(&app).staged().music_volume,
            100,
            "{size:?}: a released slider ignores the pointer"
        );

        // The check box and Accept press where they are drawn.
        press_button(&mut app, &surface, MUSIC_BUTTON);
        assert!(!page(&app).staged().music_on, "{size:?}");
        press_button(&mut app, &surface, ACCEPT_BUTTON);
        assert_eq!(section(&app), "Preferences", "{size:?}");
        assert_eq!(page(&app).committed().music_volume, 100, "{size:?}");
    }
}

#[test]
fn accept_bug_audio_preferences_ui_e_the_keyboard_reaches_every_control() {
    let dir = TempDir::new("keys");
    let (mut app, surface) = audio_app(dir.path(), None, None);
    open_audio(&mut app, &surface);

    // Down walks Music → three sliders → Quality → Cancel → Accept → Music.
    let mut visited = Vec::new();
    for _ in 0..AudioControl::ALL.len() {
        tap(&mut app, KeyCode::ArrowDown);
        visited.push(page(&app).focus().expect("a focus"));
    }
    assert_eq!(visited, AudioControl::ALL);
    tap(&mut app, KeyCode::ArrowDown);
    assert_eq!(page(&app).focus(), Some(AudioControl::Music));
    tap(&mut app, KeyCode::ArrowUp);
    assert_eq!(page(&app).focus(), Some(AudioControl::Accept));
    assert_eq!(
        state(&app).shell.focus().and_then(|index| state(&app)
            .shell
            .section()
            .map(|section| section.buttons[index].id.clone())),
        Some(ACCEPT_BUTTON.to_owned()),
        "a button control moves the shell's own focus"
    );

    // Enter on the check box toggles it.
    tap(&mut app, KeyCode::ArrowDown);
    tap(&mut app, KeyCode::Enter);
    assert!(!page(&app).staged().music_on);

    // Left/Right step the focused slider by five.
    tap(&mut app, KeyCode::ArrowDown);
    assert_eq!(page(&app).focus(), Some(AudioControl::MusicVolume));
    assert_eq!(state(&app).shell.focus(), None, "the page holds the focus");
    tap(&mut app, KeyCode::ArrowRight);
    tap(&mut app, KeyCode::ArrowRight);
    assert_eq!(page(&app).staged().music_volume, 60);
    tap(&mut app, KeyCode::ArrowLeft);
    assert_eq!(page(&app).staged().music_volume, 55);

    // The quality drop-down: Left picks Low, Enter opens, Esc closes it
    // (staying on the page), Esc again cancels and leaves.
    for _ in 0..3 {
        tap(&mut app, KeyCode::ArrowDown);
    }
    assert_eq!(page(&app).focus(), Some(AudioControl::Quality));
    tap(&mut app, KeyCode::ArrowLeft);
    assert_eq!(page(&app).staged().quality, SoundQuality::Low);
    tap(&mut app, KeyCode::Enter);
    assert!(page(&app).is_open());
    tap(&mut app, KeyCode::Escape);
    assert!(!page(&app).is_open());
    assert_eq!(section(&app), AUDIO_SECTION, "Esc closed the list only");
    tap(&mut app, KeyCode::Escape);
    assert_eq!(section(&app), "Preferences");
    assert_eq!(page(&app).accepts().len(), 0, "Esc is Cancel");
    assert_eq!(page(&app).committed().music_volume, 50);
    assert!(page(&app).committed().music_on);
}

fn recording_audio() -> (MenuAudio, RecordingAudioDevice) {
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
    let device = RecordingAudioDevice::new();
    let handle = device.handle();
    let table = MenuSoundTable::parse(SCRIPT).expect("the fixture script parses");
    let audio = MenuAudio::new(Box::new(device), &table).expect("the device opens");
    (audio, handle)
}

#[test]
fn accept_bug_audio_preferences_ui_f_the_menu_audio_plays_the_staged_mix_and_cancel_restores_it() {
    let dir = TempDir::new("mix");
    let (audio, device) = recording_audio();
    let (mut app, surface) = audio_app(dir.path(), None, Some(audio));
    let mix = |app: &App| state(app).menu_audio().expect("audio").mix();
    assert_eq!(
        mix(&app),
        MenuMix {
            music_on: true,
            music_gain: 0.5,
            effects_gain: 0.5
        },
        "the committed defaults reach the audio before the page opens"
    );
    assert!(state(&app).menu_audio().unwrap().music_playing());
    open_audio(&mut app, &surface);

    // The slider is live: the sounding loop is told the new gain at once.
    let slider = page(&app)
        .slider_of(AudioControl::MusicVolume)
        .unwrap()
        .clone();
    press_at(
        &mut app,
        &surface,
        slider.x + slider.slot.0 as i32 - 2,
        slider.y + 1,
    );
    assert_eq!(mix(&app).music_gain, 1.0);
    assert!(
        device.commands().iter().any(|command| matches!(
            command,
            DeviceCommand::Updated { update, .. } if (update.gain - 1.0).abs() < 1e-9
        )),
        "{:?}",
        device.commands()
    );

    // The check box stops the loop; the effects slider sets the cues' gain.
    press_button(&mut app, &surface, MUSIC_BUTTON);
    assert!(!state(&app).menu_audio().unwrap().music_playing());
    assert!(!mix(&app).music_on);
    let effects = page(&app)
        .slider_of(AudioControl::EffectsVolume)
        .unwrap()
        .clone();
    press_at(&mut app, &surface, effects.x + 5, effects.y + 1);
    assert!((mix(&app).effects_gain - 0.01).abs() < 1e-9);

    // Cancel puts everything back: the loop sounds again at the old gain.
    press_button(&mut app, &surface, CANCEL_BUTTON);
    assert_eq!(
        mix(&app),
        MenuMix {
            music_on: true,
            music_gain: 0.5,
            effects_gain: 0.5
        }
    );
    assert!(state(&app).menu_audio().unwrap().music_playing());

    // Accept keeps it: effects at the left end, music off.
    press_button(&mut app, &surface, "PF_B_AUDIO");
    press_button(&mut app, &surface, MUSIC_BUTTON);
    press_at(&mut app, &surface, effects.x + 5, effects.y + 1);
    press_button(&mut app, &surface, ACCEPT_BUTTON);
    assert!(!mix(&app).music_on);
    assert!((mix(&app).effects_gain - 0.01).abs() < 1e-9);
    assert!(!state(&app).menu_audio().unwrap().music_playing());
    let name = EFFECTS_SLIDER;
    assert!(page(&app).sliders().iter().any(|row| row.id == name));
    assert!(
        page(&app)
            .sliders()
            .iter()
            .any(|row| row.id == MUSIC_SLIDER)
    );
}

#[test]
fn accept_bug_audio_preferences_ui_g_the_settings_file_keeps_unusable_lines_out() {
    let dir = TempDir::new("file");
    let store = AudioSettingsFile::in_directory(dir.path());
    let defaults = AudioValues {
        music_on: true,
        music_volume: 50,
        effects_volume: 50,
        voice_volume: 50,
        quality: SoundQuality::High,
    };
    assert_eq!(store.load(defaults), defaults, "no file: the defaults");
    std::fs::write(
        store.path(),
        "MusicOn=0\nMusicVolume=0\nSfxVolume=75\nVoiceVolume=banana\nSoundQuality=low\nStray=1\nnot a line\n",
    )
    .unwrap();
    assert_eq!(
        store.load(defaults),
        AudioValues {
            music_on: false,
            music_volume: 50,
            effects_volume: 75,
            voice_volume: 50,
            quality: SoundQuality::Low,
        },
        "an out-of-range, unreadable or unknown line leaves the default"
    );
    let saved = AudioValues {
        music_on: true,
        music_volume: 7,
        effects_volume: 100,
        voice_volume: 1,
        quality: SoundQuality::Low,
    };
    store.save(&saved).expect("the file is written");
    assert_eq!(store.load(defaults), saved);
    assert!(!dir.path().join("audio-settings.tmp").exists());
}

// ------------------------------------------------------------ the retail half ---

/// **Retail.** The real `[@Audio@]` section: eleven labels with the
/// installation's own text, the five-control rows, a check box whose art is
/// eight frames and no skipped row.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_bug_audio_preferences_ui_retail_the_original_audio_rows_present_with_the_installations_text()
 {
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is unset: this run needs the original installation");
    };
    let screens = FrontEndScreens::open(Path::new(&game_dir)).expect("the screens open");
    let layout = screens.original_layout().expect("the layout reads");
    let strings = cs_app::mission_session::FlightCheckStrings::read(Path::new(&game_dir), &screens)
        .expect("the string table reads");
    let dir = TempDir::new("retail");
    let page = AudioPage::from_layout(
        &layout,
        &|name: &str| strings.resolve_tagged(name),
        &|art: &str| screens.art_extent(art),
        Some(AudioSettingsFile::in_directory(dir.path())),
    )
    .expect("the retail Audio rows read");
    let texts: Vec<&str> = page
        .labels()
        .iter()
        .map(|label| label.text.as_str())
        .collect();
    assert_eq!(
        texts,
        [
            "AUDIO",
            "In-Game Music",
            "Music Volume",
            "Effects Volume",
            "Voice Volume",
            "Sound Quality",
            "Select to hear in-game music.",
            "Set the volume of the in-game music.",
            "Set the volume of the sound effects.",
            "Set the volume of the voices.",
            "Select the sound quality. Low may improve performance.",
        ]
    );
    let items: Vec<&str> = page.items().iter().map(|(_, text)| text.as_str()).collect();
    assert_eq!(items, ["Low", "High"]);
    let ys: Vec<i32> = page.sliders().iter().map(|slider| slider.y).collect();
    assert_eq!(ys, [350, 407, 461]);
    assert!(
        page.sliders()
            .iter()
            .all(|s| (s.min, s.max, s.initial) == (1, 100, 50))
    );
    assert_eq!(page.sliders()[0].slot, (171, 3));
    assert_eq!(page.sliders()[0].knob, (43, 21));
    assert_eq!(
        (
            page.dropdown().rect.x,
            page.dropdown().rect.y,
            page.dropdown().rect.width
        ),
        (137, 505, 144)
    );

    let mut shell = MenuShell::build(
        &layout,
        &|art: &str| screens.art_extent(art),
        &|section: &str| screens.script_text(&script_member(section)),
    );
    shell.open(AUDIO_SECTION).expect("Audio is a section");
    let section = shell.section().expect("a section");
    assert!(
        section.skipped.is_empty(),
        "no Audio row is skipped: {:?}",
        section.skipped
    );
    let check = section
        .buttons
        .iter()
        .find(|button| button.id == MUSIC_BUTTON)
        .expect("the check box");
    assert_eq!(
        (check.check, check.rect.width, check.rect.height),
        (Some(false), 45, 29)
    );
    let ids: Vec<&str> = section.buttons.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, [CANCEL_BUTTON, ACCEPT_BUTTON, MUSIC_BUTTON]);
}

// ------------------------------------------------------- the windowed run ---

fn front_end_bin() -> PathBuf {
    if let Ok(bin) = std::env::var("CS_FRONT_END_BIN") {
        return PathBuf::from(bin);
    }
    PathBuf::from(env!("CARGO_BIN_EXE_cs"))
}

/// **The windowed run.** `cs --front-end` with `CS_FRONT_END_SMOKE_SCENE=audio`
/// walks Main Menu → Preferences → Audio, edits the controls, Accepts, comes
/// back, edits and Cancels. The test asserts the harness contract — the seven
/// page framebuffers arrive, the run passes and the committed values are the
/// accepted ones — never the verdict a human would give the art.
#[test]
#[ignore = "requires CS_GAME_DIR + gpu and a built windowed binary"]
fn accept_bug_audio_preferences_ui_the_windowed_smoke_draws_the_audio_page_and_applies_accept_and_cancel()
 {
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is unset: this run needs the original installation");
    };
    let Ok(capabilities) = std::env::var("CS_CAPABILITIES") else {
        panic!("CS_CAPABILITIES is unset: the front-end launch needs gpu");
    };
    assert!(
        capabilities.split(',').any(|part| part.trim() == "gpu"),
        "CS_CAPABILITIES must list gpu: {capabilities}"
    );
    let dir = std::env::var("CS_AUDIO_SMOKE_OUT").map_or_else(
        |_| std::env::temp_dir().join("audio-preferences-smoke"),
        PathBuf::from,
    );
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the capture directory is created");
    let output = std::process::Command::new(front_end_bin())
        .args(["--cs-path", &game_dir, "--front-end"])
        .args(["--smoke-seconds", "14", "--capture-dir"])
        .arg(&dir)
        .env("CS_CAPABILITIES", capabilities)
        .env("CS_FRONT_END_SMOKE_SCENE", "audio")
        .output()
        .expect("the cs binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let report = std::fs::read_to_string(dir.join("report.json"))
        .unwrap_or_else(|error| panic!("report.json is written: {error}\n{stderr}"));
    assert!(
        report.contains("\"passed\":true"),
        "the run passed its own checks: {report}\nstderr: {stderr}"
    );
    for label in [
        "audio-open",
        "audio-music-off",
        "audio-list",
        "audio-edited",
        "audio-reopened",
        "audio-unsaved",
        "audio-cancelled",
    ] {
        assert!(
            report.contains(&format!("{label}.png")) && dir.join(format!("{label}.png")).is_file(),
            "{label}.png was captured: {report}"
        );
    }
    assert!(
        report.contains("music_on: false") && report.contains("quality: Low"),
        "the Accept committed the edited values: {report}"
    );
    let settings = std::fs::read_to_string(
        dir.join(cs_app::ui::front_end::SMOKE_PROFILE_SUBDIR)
            .join(SETTINGS_FILE_NAME),
    )
    .expect("Accept wrote the settings file");
    assert!(settings.contains("MusicOn=0") && settings.contains("SoundQuality=low"));
    assert!(output.status.success(), "exit 0; stderr: {stderr}");
    println!("audio-page smoke artifacts in {}", dir.display());
}
