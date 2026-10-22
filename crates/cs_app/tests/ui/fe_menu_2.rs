//! Acceptance task FE-MENU-2 (#1834): the main-menu backdrop movie, the
//! logo/frame composition and the six working buttons.
//!
//! The `accept_fe_menu_2_` tests call the production presenter:
//! [`MenuShell`] parses an `M` row into [`ShellMovie`], [`MovieBackdrop`]
//! schedules decoded frames and loops the stream, the windowed presenter
//! plays it under the panes, and the [`StartFlow`] machine routes the six
//! main-menu presses through the F45 table.
//!
//! * The synthetic tests run over layout text in the original's *format*
//!   and a scripted [`MovieSource`]; they prove the presenter's rules
//!   (fields kept, rect from the decoded extent, pts scheduling, the loop
//!   and end-of-loops hold, failures recorded) and run in CI. They prove
//!   nothing about the retail data.
//! * The `retail` test reads `$CS_GAME_DIR`: the real `CrimFlag.MPG`
//!   decodes through [`MpegVideoSource`], the six `MM_B_*` rows carry
//!   `ResID=0` (their labels are the strips' baked text), and the
//!   logo/frame panes are as written. It is `#[ignore]`d because CI has
//!   no installation.
//! * The windowed test drives `cs --front-end --smoke-seconds` on a GPU:
//!   the flag movie must open and change frames while the scripted walk
//!   visits all six buttons' destinations. It is `#[ignore]`d because CI
//!   has neither the installation nor a GPU.

use std::collections::VecDeque;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::app::{App, AppExit};
use bevy::ecs::message::{MessageWriter, Messages};
use bevy::input::ButtonInput;
use bevy::input::InputSystems;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::prelude::{Entity, IntoScheduleConfigs, PreUpdate, ResMut, Resource, Vec2, With};
use bevy::window::{CursorMoved, PrimaryWindow};

use cs_app::cinematics::mpeg::MpegVideoSource;
use cs_app::cinematics::playback::{DecodeError, DecodedFrame, FrameSource};
use cs_app::ui::front_end::{
    FrontEndScreens, MenuShell, MovieBackdrop, MovieSource, OriginalLayout, SECTION_SURFACE,
    START_SECTION, Screen, ShellMovie, ShellRect, ShellResource, SkippedReason, StartFlow,
    StartRefusal, headless_app_flow, movie_media, script_member,
};
use cs_sim::cinematic_state::MediaFailure;

// ------------------------------------------------------- the synthetic half ---

/// An invented section in the original's format: an `M` row, two `B` rows
/// (one `ResID=0`, one an `IDS_*` name) and a third section the second
/// button starts, plus malformed `M` rows for the skipped record.
const MENU: &str = "\
[@Menu@]
MOVIE   =M,flag.mpg,10,20,3,250,200,0,1
B_SUB   =B,strip.png,100,100,0,0,Sub,0x1000,1
B_LAB   =B,strip.png,100,150,0,IDS_ANY_LABEL,,0x1000,1
BANG    =M,other.mpg,0,0,0,bad,250,0,0
NOSCALE =M,other.mpg,0,0,0,250,250,,0
NOPOS   =M,other.mpg,,0,0,250,250,0,0

[@Sub@]
BACKBG  =P,pane.png,0,0,0,1,0,0,0
B_BACK  =B,strip.png,5,5,0,0,Menu,0x1000,1
";

fn extents(art: &str) -> Option<(u32, u32)> {
    match art.to_ascii_lowercase().as_str() {
        "pane.png" => Some((200, 100)),
        "strip.png" => Some((80, 40)),
        _ => None,
    }
}

fn menu_shell() -> MenuShell {
    let layout = OriginalLayout::parse(MENU).expect("the fixture parses");
    let no_scripts = |_: &str| -> Option<String> { None };
    let mut shell = MenuShell::build(&layout, &extents, &no_scripts);
    shell.open("Menu").expect("Menu is a section");
    shell
}

#[test]
fn accept_fe_menu_2_a_movie_row_presents_its_fields_and_drawn_rect() {
    let shell = menu_shell();
    let section = shell.section().expect("a current section");

    // The M row keeps every field the file writes, exactly once.
    assert_eq!(section.movies.len(), 1);
    let movie = &section.movies[0];
    assert_eq!(
        (
            movie.id.as_str(),
            movie.file.as_str(),
            movie.x,
            movie.y,
            movie.z,
            movie.scale_x,
            movie.scale_y,
            movie.loops,
            movie.region
        ),
        ("MOVIE", "flag.mpg", 10, 20, 3, 250, 200, 0, true)
    );

    // The drawn rect is the row's X,Y and the *decoded* extent at the
    // row's Scale percent: 320x240 at 250%x200%.
    assert_eq!(
        movie.rect_at((320, 240)),
        ShellRect {
            x: 10,
            y: 20,
            width: 800,
            height: 480
        }
    );

    // Malformed rows are skipped with the field named, never guessed.
    let skipped: Vec<(&str, &SkippedReason)> = section
        .skipped
        .iter()
        .map(|row| (row.id.as_str(), &row.reason))
        .collect();
    assert_eq!(
        skipped,
        vec![
            ("BANG", &SkippedReason::MovieFieldUnread { field: "ScaleX" }),
            (
                "NOSCALE",
                &SkippedReason::MovieFieldUnread { field: "# Loops" }
            ),
            ("NOPOS", &SkippedReason::PositionUnread),
        ]
    );
    // M rows parse out of `other_rows`; only the section's own kinds stay.
    assert!(section.other_rows.is_empty(), "{:?}", section.other_rows);
}

#[test]
fn accept_fe_menu_2_a_button_keeps_its_res_id_for_the_label_record() {
    let shell = menu_shell();
    let section = shell.section().expect("a current section");
    let res_ids: Vec<(&str, Option<&str>)> = section
        .buttons
        .iter()
        .map(|button| (button.id.as_str(), button.res_id.as_deref()))
        .collect();
    assert_eq!(
        res_ids,
        vec![("B_SUB", None), ("B_LAB", Some("IDS_ANY_LABEL"))],
        "ResID=0/empty is no string-table label; an IDS_* token is kept verbatim"
    );
}

// ------------------------------------------------------- the loop player ---

/// A scripted [`MovieSource`]: a fixed extent/rate, frames dealt from a
/// queue, the last dealt frame's pixels kept the way `MpegVideoSource`
/// keeps `current_rgba`.
struct ScriptSource {
    frames: VecDeque<(u64, u8)>,
    current: Option<Vec<u8>>,
    fail_at: Option<usize>,
    calls: usize,
}

impl ScriptSource {
    /// `frames` is `(pts_us, fill byte)`; the dealt frame's pixels are a
    /// solid 2x2 of the fill byte so tests can tell frames apart.
    fn new(frames: &[(u64, u8)]) -> Self {
        Self {
            frames: frames.iter().copied().collect(),
            current: None,
            fail_at: None,
            calls: 0,
        }
    }
}

impl FrameSource for ScriptSource {
    fn next_frame(&mut self) -> Result<Option<DecodedFrame>, DecodeError> {
        let call = self.calls;
        self.calls += 1;
        if self.fail_at == Some(call) {
            return Err(DecodeError("scripted decode failure".to_owned()));
        }
        match self.frames.pop_front() {
            Some((pts_us, fill)) => {
                self.current = Some(vec![fill; 2 * 2 * 4]);
                Ok(Some(DecodedFrame {
                    pts_us,
                    index: call as u64,
                }))
            }
            None => {
                self.current = None;
                Ok(None)
            }
        }
    }
}

impl MovieSource for ScriptSource {
    fn pixels(&self) -> Option<&[u8]> {
        self.current.as_deref()
    }
    fn extent(&self) -> (u32, u32) {
        (2, 2)
    }
    fn frame_rate(&self) -> f64 {
        30.0
    }
}

fn movie_row(loops: u32) -> ShellMovie {
    ShellMovie {
        id: "MOVIE".to_owned(),
        file: "flag.mpg".to_owned(),
        x: 0,
        y: 0,
        z: 0,
        scale_x: 250,
        scale_y: 250,
        loops,
        region: false,
    }
}

#[test]
fn accept_fe_menu_2_the_backdrop_plays_each_frame_at_its_time_and_loops() {
    // A two-frame stream at 0 and 40 ms, looping forever; the `open`
    // count shows every reopen the loop makes.
    let opens = Arc::new(Mutex::new(0_u32));
    let counted = opens.clone();
    let mut backdrop = MovieBackdrop::open(
        &movie_row(0),
        move || {
            *counted.lock().unwrap() += 1;
            Ok(ScriptSource::new(&[(0, 10), (40_000, 20)]))
        },
        Duration::ZERO,
    )
    .expect("the first stream opens");
    assert_eq!(*opens.lock().unwrap(), 1, "open() ran the closure once");

    // t=0: the first frame is due immediately; the second is not.
    assert!(backdrop.update(Duration::ZERO));
    assert_eq!(backdrop.pixels(), &[10; 16]);
    assert_eq!(backdrop.delivered(), 1);

    // t=20 ms: nothing new due — the surface is unchanged.
    assert!(!backdrop.update(Duration::from_millis(20)));
    assert_eq!(backdrop.delivered(), 1);

    // t=40 ms: the second frame is due; the stream is also spent, so the
    // next update past it reopens for the loop.
    assert!(backdrop.update(Duration::from_millis(40)));
    assert_eq!(backdrop.pixels(), &[20; 16]);
    assert!(!backdrop.update(Duration::from_millis(60)));
    assert!(
        backdrop.update(Duration::from_millis(80)),
        "the reopened stream's first frame is due on the new cycle"
    );
    assert_eq!(backdrop.played(), 1);
    assert_eq!(*opens.lock().unwrap(), 2, "the loop reopened the stream");
    assert!(!backdrop.ended(), "loops=0 never ends");
}

#[test]
fn accept_fe_menu_2_the_backdrop_holds_the_last_frame_when_loops_are_spent() {
    let mut backdrop = MovieBackdrop::open(
        &movie_row(1),
        || Ok(ScriptSource::new(&[(0, 7), (30_000, 9)])),
        Duration::ZERO,
    )
    .expect("the stream opens");
    assert!(backdrop.update(Duration::ZERO));
    assert!(backdrop.update(Duration::from_millis(30)));
    assert_eq!(backdrop.delivered(), 2);

    // The playthrough ends at the stream's end and loops=1 asks for no
    // replay: ended, the last delivered frame stays up, no reopen.
    assert!(!backdrop.update(Duration::from_millis(60)));
    assert!(backdrop.ended());
    assert_eq!(backdrop.played(), 1);
    assert_eq!(backdrop.pixels(), &[9; 16], "the last frame stays up");
    assert!(!backdrop.update(Duration::from_secs(60)));
    assert_eq!(backdrop.delivered(), 2);
}

#[test]
fn accept_fe_menu_2_a_failed_reopen_is_recorded_and_never_faked() {
    let opens = Arc::new(Mutex::new(0_u32));
    let counted = opens.clone();
    let mut backdrop = MovieBackdrop::open(
        &movie_row(0),
        move || {
            let call = {
                let mut count = counted.lock().unwrap();
                *count += 1;
                *count
            };
            if call == 1 {
                Ok(ScriptSource::new(&[(0, 5)]))
            } else {
                Err(MediaFailure::MissingMedia {
                    media: movie_media("flag.mpg"),
                })
            }
        },
        Duration::ZERO,
    )
    .expect("the first stream opens");
    assert!(backdrop.update(Duration::ZERO));
    // The stream ends and the loop's reopen refuses: recorded, the last
    // frame stays up, the backdrop is not ended (loops=0) and not playing.
    assert!(!backdrop.update(Duration::from_millis(10)));
    assert!(
        backdrop
            .failure()
            .is_some_and(|f| f.contains("missing media")),
        "{:?}",
        backdrop.failure()
    );
    assert!(!backdrop.ended());
    assert_eq!(backdrop.pixels(), &[5; 16]);
    assert!(!backdrop.update(Duration::from_secs(60)));
}

// -------------------------------------------------- the headless machine ---

/// An invented layout naming the *real* sections and buttons, with
/// invented art and scripts: the F45 machine's start-flow drive over
/// `headless_app_flow` — the same `ButtonInput`/`CursorMoved` path the
/// windowed run takes, without the renderer. The button ids are the real
/// `MM_B_*` rows': the machine's stand-in for the lobby press keys on the
/// id the layout writes.
const SECTIONS: &str = "\
[@MainMenu@]
MOVIE   =M,flag.mpg,0,0,0,250,250,0,0
MM_B_CAMPAIGN=B,strip.png,10,10,0,0,Campaign,0x1000,1
MM_B_INSTANTACTION=B,strip.png,10,70,0,0,InstantAction,0x1000,1
MM_B_MULTIPLAYER=B,strip.png,10,130,0,0,MultiPlayerMain,0x1000,1
MM_B_PREFERENCES=B,strip.png,10,190,0,0,Preferences,0x1000,1
MM_B_CREDITS=B,strip.png,10,250,0,0,Credits,0x1000,1
MM_B_QUIT=B,strip.png,10,310,0,0,,0x1000,1

[@Campaign@]
B_BACK  =B,strip.png,5,5,0,0,MainMenu,0x1000,1

[@InstantAction@]
B_BACK  =B,strip.png,5,5,0,0,MainMenu,0x1000,1

[@Preferences@]
B_BACK  =B,strip.png,5,5,0,0,MainMenu,0x1000,1

[@Credits@]
B_BACK  =B,strip.png,5,5,0,0,MainMenu,0x1000,1
";

const QUIT_SCRIPT: &str = "\
main
{
gui_create
{
object QW = @ctl@BE
QW.YC = \"mm_b_quit\"
}
gui_mailbox
{
switch (getmessage())
{
case QW
terminate
}
}
}
";

fn flow_shell() -> MenuShell {
    let layout = OriginalLayout::parse(SECTIONS).expect("the fixture parses");
    let scripts = |section: &str| {
        section
            .eq_ignore_ascii_case("MainMenu")
            .then(|| QUIT_SCRIPT.to_owned())
    };
    let mut shell = MenuShell::build(&layout, &extents, &scripts);
    shell.open(START_SECTION).expect("MainMenu is a section");
    shell
}

/// The scripted input the test queue applies: the same `ButtonInput` /
/// `CursorMoved` resources real input and the smoke write.
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

/// Presses the button at `index` of the current section with the pointer.
fn press(app: &mut App, index: usize) {
    let (x, y) = {
        let state = app.world().resource::<ShellResource>();
        let view = state.shell.view(SECTION_SURFACE).expect("a view");
        let button = &view.buttons[index];
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

fn flow(app: &App) -> &StartFlow {
    app.world()
        .resource::<ShellResource>()
        .flow()
        .expect("the start flow drives this app")
}

#[test]
fn accept_fe_menu_2_the_six_buttons_reach_their_machine_states_through_the_real_path() {
    let mut app = headless_app_flow(flow_shell(), Some(StartFlow::new()));
    app.init_resource::<TestInput>();
    app.add_systems(PreUpdate, inject.after(InputSystems));
    app.update();

    // Campaign → the pilot page (ProfileSelect), drawn as the `Campaign`
    // section; its back button hands home.
    press(&mut app, 0);
    assert_eq!(flow(&app).screen(), Screen::ProfileSelect);
    {
        let shell = &app.world().resource::<ShellResource>().shell;
        assert_eq!(shell.section_name(), Some("Campaign"));
    }
    press(&mut app, 0);
    assert_eq!(flow(&app).screen(), Screen::MainMenu);

    // Instant Action → IaSelect, then home.
    press(&mut app, 1);
    assert_eq!(flow(&app).screen(), Screen::IaSelect);
    {
        let shell = &app.world().resource::<ShellResource>().shell;
        assert_eq!(shell.section_name(), Some("InstantAction"));
    }
    press(&mut app, 0);
    assert_eq!(flow(&app).screen(), Screen::MainMenu);

    // Multiplayer: `MultiPlayerMain` is not a section, so the press stands
    // for the table's OpenLobby — refused while the lobby is unbuilt, and
    // the machine stays on the menu.
    press(&mut app, 2);
    assert_eq!(flow(&app).screen(), Screen::MainMenu);
    assert!(
        flow(&app).refusals().iter().any(|refusal| matches!(
            refusal,
            StartRefusal::NotYetPresented {
                screen: Screen::Lobby,
                ..
            }
        )),
        "{:?}",
        flow(&app).refusals()
    );

    // Preferences → Settings; Credits → Credits; each home by its button.
    press(&mut app, 3);
    assert_eq!(flow(&app).screen(), Screen::Settings);
    press(&mut app, 0);
    assert_eq!(flow(&app).screen(), Screen::MainMenu);
    press(&mut app, 4);
    assert_eq!(flow(&app).screen(), Screen::Credits);
    press(&mut app, 0);
    assert_eq!(flow(&app).screen(), Screen::MainMenu);

    // Quit: the section script's `terminate` → the machine's Quit →
    // AppExit on the real message path.
    press(&mut app, 5);
    let exits = app
        .world_mut()
        .resource_mut::<Messages<AppExit>>()
        .drain()
        .count();
    assert_eq!(exits, 1, "the Quit button's terminate posted AppExit");
}

// --------------------------------------------------------- the retail half ---

/// The installation, opened once (the same shape `ui::original` uses).
fn retail_screens() -> FrontEndScreens {
    let dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must point at the original installation"),
    );
    assert!(
        dir.is_dir(),
        "CS_GAME_DIR {} is not a directory",
        dir.display()
    );
    FrontEndScreens::open(&dir).expect("the original screen container opens")
}

/// The original `MainMenu` section through the shell.
fn retail_shell() -> MenuShell {
    let screens = retail_screens();
    let layout = screens.original_layout().expect("the layout member reads");
    let mut shell = MenuShell::build(
        &layout,
        &|art: &str| screens.art_extent(art),
        &|section: &str| screens.script_text(&script_member(section)),
    );
    shell.open(START_SECTION).expect("MainMenu is a section");
    shell
}

fn json_str(value: &str) -> String {
    let mut out = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// **The retail half.** The main menu as the installation writes it: the
/// `MOVIE` row's `CrimFlag.MPG` reads through the install mount and
/// decodes through the production [`MpegVideoSource`]; the six buttons
/// carry `ResID=0` — the string table holds no label for them, their text
/// is the strips' baked art (the task description's "strings table" is
/// what the data itself shows: recorded, never claimed). The logo and the
/// button frame are the section's two panes. When `CS_EVIDENCE_DIR` is
/// set the measurement is written to `front-end-mainmenu.json`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_fe_menu_2_retail_the_flag_decodes_and_the_six_buttons_are_as_written() {
    let screens = retail_screens();
    let shell = retail_shell();
    let menu = shell.section().expect("the main menu presents");

    // The logo/frame composition, in declaration order.
    assert_eq!(
        menu.panes
            .iter()
            .map(|pane| pane.id.as_str())
            .collect::<Vec<_>>(),
        vec!["MM_LOGO", "BFRAME"],
        "the logo and the button frame"
    );
    assert_eq!(menu.panes[0].art, "MM_Logo.png");
    assert_eq!(menu.panes[1].art, "MainMenuButtonsStructure.png");

    // The movie row exactly as written; the whole section is handled
    // (nothing skipped, nothing unknown-kind).
    assert_eq!(menu.movies.len(), 1);
    let movie = &menu.movies[0];
    assert_eq!(
        (
            movie.id.as_str(),
            movie.file.as_str(),
            movie.x,
            movie.y,
            movie.z,
            movie.scale_x,
            movie.scale_y,
            movie.loops,
            movie.region
        ),
        ("MOVIE", "CrimFlag.MPG", 0, 0, 0, 250, 250, 0, false)
    );
    assert!(menu.skipped.is_empty(), "{:?}", menu.skipped);
    assert!(menu.other_rows.is_empty(), "{:?}", menu.other_rows);

    // The six buttons: no ResID (the labels are the strips' baked text),
    // each effect as the data states.
    let labels: Vec<(&str, Option<&str>)> = menu
        .buttons
        .iter()
        .map(|button| (button.id.as_str(), button.res_id.as_deref()))
        .collect();
    assert_eq!(
        labels,
        vec![
            ("MM_B_CAMPAIGN", None),
            ("MM_B_INSTANTACTION", None),
            ("MM_B_MULTIPLAYER", None),
            ("MM_B_PREFERENCES", None),
            ("MM_B_CREDITS", None),
            ("MM_B_QUIT", None),
        ],
        "every main-menu button writes ResID=0: the string table is not \
         consulted for them"
    );

    // The movie file itself: read through the install mount and decoded
    // by the production MPEG-1 source.
    let bytes = screens
        .movie_bytes(&movie.file)
        .expect("the flag movie reads through the install mount");
    let media = movie_media(&movie.file);
    let mut source = MpegVideoSource::new(Cursor::new(bytes), &media)
        .expect("the flag movie opens in the production decoder");
    let info = source.info();
    assert_eq!(
        (info.width, info.height),
        (320, 240),
        "the measured flag extent"
    );
    assert!(info.frame_rate > 0.0, "the stream reports a rate");
    assert_eq!(
        movie.rect_at((info.width, info.height)),
        ShellRect {
            x: 0,
            y: 0,
            width: 800,
            height: 600
        },
        "250% of 320x240 fills the authored surface"
    );
    let first = source
        .next_frame()
        .expect("the first frame decodes")
        .expect("the stream has a first frame");
    let rgba = source.current_rgba().expect("the decoded pixels are kept");
    assert_eq!(rgba.len(), 320 * 240 * 4);
    let first_byte = rgba[0];
    assert!(
        rgba.iter().any(|&byte| byte != first_byte),
        "the flag's first frame is not uniform"
    );

    if let Ok(dir) = std::env::var("CS_EVIDENCE_DIR") {
        let dir = PathBuf::from(dir);
        let dir = if dir.is_absolute() {
            dir
        } else {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(Path::parent)
                .expect("workspace root")
                .join(dir)
        };
        std::fs::create_dir_all(&dir).expect("the evidence directory is created");
        let buttons = menu
            .buttons
            .iter()
            .map(|button| {
                json_str(&format!(
                    "{} res={} -> {:?}",
                    button.id,
                    button.res_id.as_deref().unwrap_or("0"),
                    button.effect
                ))
            })
            .collect::<Vec<_>>()
            .join(", ");
        let json = format!(
            "{{\n  \"section\": \"MainMenu\",\n  \"install_sha256\": {},\n  \"panes\": [{}],\n  \
             \"movie\": {{\"id\": {}, \"file\": {}, \"x\": {}, \"y\": {}, \"z\": {}, \"scale_x\": {}, \
             \"scale_y\": {}, \"loops\": {}, \"region\": {}, \"decoded\": {{\"width\": {}, \
             \"height\": {}, \"frame_rate\": {}, \"first_pts_us\": {}}}}},\n  \
             \"buttons\": [{}]\n}}\n",
            json_str(screens.install_sha256()),
            menu.panes
                .iter()
                .map(|pane| {
                    json_str(&format!(
                        "{} {} @{},{} {}x{} frames={}",
                        pane.id,
                        pane.art,
                        pane.rect.x,
                        pane.rect.y,
                        pane.rect.width,
                        pane.rect.height,
                        pane.frames
                    ))
                })
                .collect::<Vec<_>>()
                .join(", "),
            json_str(&movie.id),
            json_str(&movie.file),
            movie.x,
            movie.y,
            movie.z,
            movie.scale_x,
            movie.scale_y,
            movie.loops,
            movie.region,
            info.width,
            info.height,
            info.frame_rate,
            first.pts_us,
            buttons
        );
        std::fs::write(dir.join("front-end-mainmenu.json"), json).expect("the artifact is written");
    }
}

// ------------------------------------------------------ the windowed smoke ---

/// The windowed binary the ignored run drives: `CS_FRONT_END_BIN` when the
/// operator names one, else the test harness's own `cs` build.
fn front_end_bin() -> PathBuf {
    if let Ok(bin) = std::env::var("CS_FRONT_END_BIN") {
        return PathBuf::from(bin);
    }
    PathBuf::from(env!("CARGO_BIN_EXE_cs"))
}

/// **The windowed run.** `cs --cs-path $CS_GAME_DIR --front-end
/// --smoke-seconds 14 --capture-dir <dir>` drives the full scripted pass:
/// the flag movie must open and keep changing frames while the pointer
/// walks all six buttons' destinations and Quit's `terminate` ends it.
/// The test asserts the harness contract — the movie and navigation
/// records arrive and pass — never the verdicts a human would judge.
#[test]
#[ignore = "requires CS_GAME_DIR + gpu and a built windowed binary"]
fn accept_fe_menu_2_the_windowed_smoke_plays_the_flag_and_walks_the_six() {
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
    let dir = std::env::var("CS_FE_MENU_SMOKE_OUT").map_or_else(
        |_| std::env::temp_dir().join("fe-menu-2-smoke"),
        PathBuf::from,
    );
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
    assert!(
        output.status.code().is_some(),
        "the run must exit by itself, never on a signal: {output:?}\n{stderr}"
    );

    let report = std::fs::read_to_string(dir.join("report.json")).unwrap_or_else(|error| {
        panic!(
            "{} is written: {error}\n{stderr}",
            dir.join("report.json").display()
        )
    });
    assert!(
        report.contains("\"kind\":\"front-end-shell-smoke-report\""),
        "the report names its kind: {report}"
    );
    assert!(
        report.contains("MOVIE MOVIE on MainMenu: CrimFlag.MPG 320x240"),
        "the flag movie opened with its decoded extent: {report}"
    );
    assert!(
        report.contains("loops continuous"),
        "the row's # Loops=0 reads as continuous: {report}"
    );
    assert!(
        !report.contains("\"movie_frames\":0"),
        "decoded movie frames reached a texture: {report}"
    );
    assert!(
        report.contains("\"quit_seen\":true"),
        "Quit pressed terminate: {report}"
    );
    assert!(
        report.contains("\"passed\":true"),
        "the run passed its own checks: {report}\nstderr: {stderr}"
    );
    assert!(
        output.status.success(),
        "the process exits 0 exactly when the smoke passed; stderr: {stderr}"
    );
    let pngs: Vec<_> = std::fs::read_dir(&dir)
        .expect("the capture directory lists")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".png"))
        .collect();
    println!(
        "front-end smoke artifacts in {}: {} PNGs, report passed",
        dir.display(),
        pngs.len()
    );
}
