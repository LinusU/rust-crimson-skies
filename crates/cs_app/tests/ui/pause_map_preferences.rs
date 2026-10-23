//! Tasks PARITY3-PAUSE-MAP-SCREEN.02-E (#1877) and its .02-E.02 stage
//! (#1923): the Esc pause screen's Preferences button opens the mission
//! window's preferences overlay with the session's clock frozen, and the
//! overlay's Return to Game shows the map again — the pause reason
//! untouched throughout.
//!
//! * The `02e_02` headless test drives the production Esc path
//!   (`sync_pause_map`, `drive_pause_map`) over synthetic bitmaps with a
//!   `PreferencesOverlay` record standing in for the overlay plugin's open
//!   flag (its section's source is retail-read).
//! * The `02e` headless test — the parent's own acceptance — runs the same
//!   loop against the real [`PreferencesOverlayPlugin`] over the retail
//!   `[@Preferences@]` section itself: a `MenuShell` built from the
//!   section's layout text verbatim (its hidden `PF_B_MAINMENU` slot, its
//!   text and movie rows included) with synthetic bitmaps of the retail
//!   extents, so the press lands on the drawn settings screen and its own
//!   Return to Game row.
//! * The `02e_02` retail test runs the whole loop on the composed M01
//!   with both plugins installed.
//!
//! Prefix `accept_parity3_pause_map_02e_`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use bevy::asset::AssetApp;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput, NativeKey};
use bevy::input::mouse::{MouseButton, MouseButtonInput};
use bevy::prelude::{
    App, Entity, MessageReader, Plugin, ResMut, Resource, Vec2, Window, With, default,
};
use bevy::time::{Fixed, Time, TimeUpdateStrategy};
use cs_app::input::platform::PlatformInput;
use cs_app::input::session::PauseReason;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionStage, OpenPreferencesOverlay, PauseButtonKind, PauseMapPart, PauseMapPlugin,
    PauseMapPress, PauseMapScene, PauseMapShown, PauseMapSource, PreferencesEffect,
    PreferencesOverlay, PreferencesOverlayPlugin, PreferencesOverlaySource, build_headless,
    install_pause_map, install_preferences_overlay, read_pause_map, read_preferences_overlay,
    stage_for, teardown,
};
use cs_app::physics::PhysicsTickLedger;
use cs_app::playtest::{PlaytestState, headless_app_with};
use cs_app::ui::front_end::{Artwork, FrontEndScreens, MenuShell, OriginalLayout};
use cs_content::briefing::Point;
use cs_content::campaign_bindings::MissionLabel;

fn point(x: i32, y: i32) -> Point {
    Point { x, y }
}

/// M01's scene as the record states it, with synthetic bitmaps of the retail
/// extents (`pause_map_presenter`'s own shape).
fn synthetic_source() -> PauseMapSource {
    use cs_app::mission_session::PauseMapScene as Scene;
    let sizes = [
        ("loadframe", 801, 601),
        ("ha-m1map", 800, 600),
        ("momento_temp", 242, 305),
        ("momento_shad", 300, 300),
        ("pin6", 76, 80),
        ("pin4", 76, 80),
        ("pin1", 76, 80),
        ("pin2", 76, 80),
        ("pin3", 76, 80),
        ("pin5", 76, 80),
        ("pin6_1", 76, 80),
        ("pin7", 76, 80),
        ("pin8", 76, 80),
        ("obj_check1", 25, 24),
        ("singledev", 32, 32),
        ("parchment", 240, 312),
        ("escape_button1", 132, 28),
        ("escape_button2", 132, 28),
        ("escape_button3", 132, 28),
        ("daglove", 40, 40),
        ("dafinger", 40, 40),
    ];
    let artwork = sizes
        .iter()
        .enumerate()
        .map(|(index, (name, width, height))| {
            let rgba = vec![index as u8 + 1; (*width as usize) * (*height as usize) * 4];
            (
                (*name).to_owned(),
                Artwork::new(*width, *height, rgba).expect("artwork"),
            )
        })
        .collect();
    let picture = |id: &str, bitmap: &str, x, y| cs_app::mission_session::PausePicture {
        id: id.to_owned(),
        bitmap: bitmap.to_owned(),
        at: point(x, y),
        centered: true,
    };
    let button = |kind: PauseButtonKind, x, y, label: &str| cs_app::mission_session::PauseButton {
        kind,
        at: point(x, y),
        bitmaps: [
            "escape_button1".to_owned(),
            "escape_button2".to_owned(),
            "escape_button3".to_owned(),
        ],
        label_text: label.to_owned(),
        label: label.to_owned(),
        label_offset: point(66, 7),
    };
    let scene: Scene = PauseMapScene {
        key: "loading_c61".to_owned(),
        background: vec![("loadframe".to_owned(), point(0, 0))],
        chart: cs_app::mission_session::PauseChart {
            bitmap: "HA-m1MAP".to_owned(),
            at: point(16, 19),
            clip_top_left: point(211, 51),
            clip_bottom_right: point(784, 551),
            world: [-10818.43, 9155.875, -2204.118, 540.563],
        },
        memento: ("momento_temp".to_owned(), point(533, 326)),
        pictures: vec![
            picture("MOMENTOSHAD", "momento_shad", 661, 454),
            picture("OBJPIN1", "pin6", 506, 149),
            picture("OBJPIN2", "pin6", 396, 254),
            picture("OBJPIN3", "pin6", 181, 244),
            picture("OBJPIN4", "pin4", 106, 319),
        ],
        note: cs_app::mission_session::PauseNote {
            bitmap: "parchment".to_owned(),
            at: point(555, 6),
            title_text: "MSG_BRF_DLG_OBJECTIVES".to_owned(),
            title: "Objectives".to_owned(),
            title_at: point(595, 25),
            check_mark: "obj_check1".to_owned(),
            list_at: point(580, 50),
            list_wrap: (190, 255),
            list_spacing: 5,
        },
        buttons: vec![
            button(PauseButtonKind::Resume, 107, 528, "Resume"),
            button(PauseButtonKind::Restart, 127, 559, "Restart"),
            button(PauseButtonKind::Preferences, 367, 528, "Preferences"),
            button(PauseButtonKind::Quit, 347, 559, "Quit"),
        ],
        cursor: ("daglove".to_owned(), "dafinger".to_owned()),
        own_ship: "singledev".to_owned(),
        objectives: (0..4).map(|i| (format!("OBJ{}", i + 1), i)).collect(),
        unread_commands: Vec::new(),
    };
    PauseMapSource { scene, artwork }
}

#[derive(Resource, Default)]
struct Presses(Vec<PauseButtonKind>);

fn collect(mut reader: MessageReader<PauseMapPress>, mut out: ResMut<Presses>) {
    out.0.extend(reader.read().map(|press| press.0));
}

/// The `OpenPreferencesOverlay` messages the press wrote, counted so the
/// test sees the write itself, not only the covered map.
#[derive(Resource, Default)]
struct Opens(u32);

/// The overlay plugin's half, standing in for the retail-read section: an
/// [`OpenPreferencesOverlay`] message opens the record the pause map reads.
fn open_on_message(
    mut reader: MessageReader<OpenPreferencesOverlay>,
    mut overlay: ResMut<PreferencesOverlay>,
    mut opens: ResMut<Opens>,
) {
    for _ in reader.read() {
        overlay.open = true;
        opens.0 += 1;
    }
}

fn app() -> App {
    let mut app = headless_app_with(|app| {
        if !app
            .world()
            .contains_resource::<bevy::prelude::Assets<bevy::prelude::Image>>()
        {
            app.init_asset::<bevy::prelude::Image>();
        }
        install_pause_map(app, synthetic_source());
        app.init_resource::<PreferencesOverlay>()
            .init_resource::<Presses>()
            .init_resource::<Opens>()
            .add_systems(bevy::prelude::Last, (collect, open_on_message));
    });
    app.world_mut().spawn(Window {
        resolution: (800, 600).into(),
        ..default()
    });
    app.update();
    app
}

/// The retail `[@Preferences@]` section verbatim — its own rows, rects,
/// art names and `<…>` placeholders (the section's `V` vars and the two
/// `[GLOBALVARS]` entries they resolve through included) — plus the
/// one-line sections its option buttons name, so the shell reads their
/// `Navigate` effects the way the real layout does. `PF_B_MAINMENU` stays
/// in the section like the data holds it: the overlay hides and disarms
/// the out-of-game slot itself (`PF_B_RETURNTOGAME` starts no script; its
/// arm the retail `PREFERENCES.SCRIPT` keys by the slot object, so the
/// shell reads the same `NoHandler` it does there).
const OVERLAY_LAYOUT: &str = "\
[GLOBALVARS]
G13=PF_DESCW,310
G14=PF_DESCH,40

[@Preferences@]
V1=GX,115
V2=TCOLOR,0xFFFFDDc4
V3=TEXTX,348
PF_BACKGROUND=P,PF_BackGround.png,110,215,0,1,0,1,0
PF_B_GAMEOPTIONS=B,PF_B_GameOptions.png,<GX>,272,0,0,GameOptions,0x1000,0,,,,,,
PF_B_AUDIO=B,PF_B_Audio.png,<GX>,323,0,0,Audio,0x1000,0,,,,,,
PF_B_VIDEO=B,PF_B_Video.png,<GX>,374,0,0,Video,0x1000,0,,,,,,,
PF_B_CONTROLS=B,PF_B_Controls.png,<GX>,425,0,0,ControlsPrefs,0x1000,0,,,,,,
PF_B_MAINMENU=B,PC_B_ReturnMainMenu.png,462,504,0,0,,,,,,,,,
PF_B_RETURNTOGAME=B,GN_B_ReturnToGame.Png,455,509,0,0,,,,,,,,,
PF_T_TITLE=T,IDS_PF_TITLE,122,228,50,200,28,0XFFC0BAAD,1
PF_T_GODESC=T,IDS_PF_GO_DESC,<TEXTX>,284,0,<PF_DESCW>,<PF_DESCH>,<TCOLOR>,0
PF_T_APDESC=T,IDS_PF_AUDIO_DESC,<TEXTX>,335,0,<PF_DESCW>,<PF_DESCH>,<TCOLOR>,0
PF_T_VPDESC=T,IDS_PF_VIDEO_DESC,<TEXTX>,386,0,<PF_DESCW>,<PF_DESCH>,<TCOLOR>,0
PF_T_CPDESC=T,IDS_PF_CONTROLS_DESC,<TEXTX>,437,0,<PF_DESCW>,<PF_DESCH>,<TCOLOR>,0
PF_LOGO=P,MM_Logo.png,134,13,0,1,0,1,0
MOVIE=M,CrimFlag.MPG,0,0,0,250,250,0,0

[@GameOptions@]
GO_BACKGROUND=P,goback.png,0,0,0,1,0,1,0

[@Audio@]
AU_BACKGROUND=P,auback.png,0,0,0,1,0,1,0

[@Video@]
VI_BACKGROUND=P,viback.png,0,0,0,1,0,1,0

[@ControlsPrefs@]
CP_BACKGROUND=P,cpback.png,0,0,0,1,0,1,0
";

/// The synthetic arts' extents — the retail bitmaps' own sizes (the
/// option strips' four 42-px state frames, the return strips' four 32-px
/// ones).
fn overlay_art_extent(art: &str) -> Option<(u32, u32)> {
    match art.to_ascii_lowercase().as_str() {
        "pf_background.png" | "goback.png" | "auback.png" | "viback.png" | "cpback.png" => {
            Some((566, 331))
        }
        "mm_logo.png" => Some((544, 240)),
        "pf_b_gameoptions.png" | "pf_b_audio.png" | "pf_b_video.png" | "pf_b_controls.png" => {
            Some((221, 168))
        }
        "pc_b_returnmainmenu.png" | "gn_b_returntogame.png" => Some((200, 128)),
        _ => None,
    }
}

/// A [`PreferencesOverlaySource`] built by hand: the section measured
/// through `MenuShell` exactly like `read_preferences_overlay` measures the
/// installation's, with synthetic bitmaps of the retail extents.
fn synthetic_overlay_source() -> PreferencesOverlaySource {
    let layout = OriginalLayout::parse(OVERLAY_LAYOUT).expect("the synthetic section parses");
    let shell = MenuShell::build(&layout, &|art: &str| overlay_art_extent(art), &|_: &str| {
        None
    });
    let artwork: BTreeMap<String, Artwork> = [
        "pf_background.png",
        "mm_logo.png",
        "pf_b_gameoptions.png",
        "pf_b_audio.png",
        "pf_b_video.png",
        "pf_b_controls.png",
        "pc_b_returnmainmenu.png",
        "gn_b_returntogame.png",
    ]
    .iter()
    .enumerate()
    .map(|(index, name)| {
        let (width, height) = overlay_art_extent(name).expect("the named art has an extent");
        let rgba = vec![index as u8 + 1; (width as usize) * (height as usize) * 4];
        (
            (*name).to_owned(),
            Artwork::new(width, height, rgba).expect("artwork"),
        )
    })
    .collect();
    PreferencesOverlaySource::from_shell(shell, artwork)
}

/// The headless app with both production plugins installed — the real
/// overlay over the real pause map, not the `open_on_message` stand-in.
fn app_with_overlay() -> App {
    let mut app = headless_app_with(|app| {
        if !app
            .world()
            .contains_resource::<bevy::prelude::Assets<bevy::prelude::Image>>()
        {
            app.init_asset::<bevy::prelude::Image>();
        }
        install_pause_map(app, synthetic_source());
        install_preferences_overlay(app, synthetic_overlay_source());
    });
    app.world_mut().spawn(Window {
        resolution: (800, 600).into(),
        ..default()
    });
    app.update();
    app
}

fn key(app: &mut App, code: KeyCode, state: ButtonState) {
    app.world_mut().write_message(KeyboardInput {
        key_code: code,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
}

fn tap_escape(app: &mut App) {
    key(app, KeyCode::Escape, ButtonState::Pressed);
    app.update();
    key(app, KeyCode::Escape, ButtonState::Released);
    app.update();
}

fn click_at(app: &mut App, x: f32, y: f32) {
    let window = {
        let mut query = app.world_mut().query_filtered::<Entity, With<Window>>();
        query.single(app.world()).expect("one window")
    };
    app.world_mut()
        .get_mut::<Window>(window)
        .expect("window")
        .set_cursor_position(Some(Vec2::new(x, y)));
    app.update();
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window,
    });
    app.update();
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Released,
        window,
    });
    app.update();
}

fn ticks(app: &App) -> u64 {
    app.world().resource::<PhysicsTickLedger>().ticks
}

fn parts(app: &mut App) -> usize {
    app.world_mut()
        .query_filtered::<Entity, With<PauseMapPart>>()
        .iter(app.world())
        .count()
}

fn overlay_parts(app: &mut App) -> usize {
    app.world_mut()
        .query_filtered::<Entity, With<cs_app::mission_session::PreferencesOverlayPart>>()
        .iter(app.world())
        .count()
}

fn run(app: &mut App, frames: u32) {
    for _ in 0..frames {
        app.update();
    }
}

/// **The Preferences press writes `OpenPreferencesOverlay`, the open
/// overlay covers the map — its buttons dead — and the overlay closing
/// draws the map again; the session stays Esc-paused and the fixed clock
/// frozen at every step.**
#[test]
fn accept_parity3_pause_map_02e_02_preferences_press_covers_the_map_under_the_overlay() {
    let mut app = app();
    run(&mut app, 30);
    tap_escape(&mut app);
    run(&mut app, 2);
    assert!(app.world().resource::<PauseMapShown>().0);
    let frozen = ticks(&app);
    run(&mut app, 30);
    assert_eq!(ticks(&app), frozen, "the clock is frozen under the map");

    // The Preferences button's authored centre.
    click_at(&mut app, 367.0 + 66.0, 528.0 + 14.0);
    run(&mut app, 2);
    assert_eq!(
        app.world().resource::<Presses>().0,
        [PauseButtonKind::Preferences]
    );
    assert_eq!(
        app.world().resource::<Opens>().0,
        1,
        "the press wrote OpenPreferencesOverlay"
    );
    assert!(
        !app.world().resource::<PauseMapShown>().0,
        "the map is hidden under the overlay"
    );
    assert_eq!(parts(&mut app), 0, "the map's parts are gone");
    assert!(
        app.world().resource::<PlaytestState>().paused,
        "still paused under the overlay"
    );
    assert_eq!(
        app.world()
            .resource::<PlatformInput>()
            .session()
            .pause_reason(),
        Some(PauseReason::PlayerRequest),
        "the pause reason is unchanged"
    );
    assert_eq!(ticks(&app), frozen, "the clock stays frozen");

    // A click where Resume sits presses nothing while the map is covered:
    // its buttons are dead, so Resume never triggers.
    click_at(&mut app, 107.0 + 66.0, 528.0 + 14.0);
    run(&mut app, 2);
    assert_eq!(app.world().resource::<Presses>().0.len(), 1);
    assert!(app.world().resource::<PlaytestState>().paused);
    assert_eq!(ticks(&app), frozen);

    // The overlay's return: its flag flips and the map is drawn again over
    // the same pause.
    app.world_mut().resource_mut::<PreferencesOverlay>().open = false;
    run(&mut app, 2);
    assert!(
        app.world().resource::<PauseMapShown>().0,
        "the map is drawn again"
    );
    assert!(parts(&mut app) > 0);
    assert!(app.world().resource::<PlaytestState>().paused);
    assert_eq!(
        app.world()
            .resource::<PlatformInput>()
            .session()
            .pause_reason(),
        Some(PauseReason::PlayerRequest)
    );
    assert_eq!(ticks(&app), frozen, "Resume was never triggered");

    // Esc still closes the pause and lifts the map afterwards: the detour
    // leaves the screen's own close path working.
    tap_escape(&mut app);
    run(&mut app, 4);
    assert!(
        !app.world()
            .resource::<PlatformInput>()
            .session()
            .is_paused(),
        "Esc closes the pause after the detour"
    );
    assert!(!app.world().resource::<PauseMapShown>().0);
    assert_eq!(parts(&mut app), 0);
}

/// **The parent task's own acceptance**: the Esc map's Preferences button
/// opens the *real* overlay plugin's settings screen — its rows drawn, its
/// own Return to Game live — that row brings the map back, and the
/// session's clock never moves.
#[test]
fn accept_parity3_pause_map_02e_preferences_button_opens_the_settings_screen_and_back_returns_to_the_map()
 {
    let mut app = app_with_overlay();
    run(&mut app, 30);
    tap_escape(&mut app);
    run(&mut app, 2);
    assert!(app.world().resource::<PauseMapShown>().0);
    let map_parts = parts(&mut app);
    assert!(map_parts > 0, "the Esc screen is drawn");
    let frozen = ticks(&app);
    run(&mut app, 10);
    assert_eq!(ticks(&app), frozen, "the clock is frozen under the map");

    // The Preferences button's authored centre.
    click_at(&mut app, 367.0 + 66.0, 528.0 + 14.0);
    run(&mut app, 2);
    {
        let overlay = app.world().resource::<PreferencesOverlay>();
        assert!(overlay.open, "the press opened the settings screen");
        let ids: Vec<&str> = overlay.drawn.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "PF_BACKGROUND",
                "PF_LOGO",
                "PF_B_GAMEOPTIONS",
                "PF_B_AUDIO",
                "PF_B_VIDEO",
                "PF_B_CONTROLS",
                "PF_B_RETURNTOGAME"
            ],
            "the in-game page's own rows are drawn; the section's \
             PF_B_MAINMENU stays hidden like the script's slot swap states"
        );
        assert!(overlay.skipped.is_empty(), "{:?}", overlay.skipped);
        assert_eq!(
            overlay.other_rows,
            ["T"],
            "the five text rows counted, never drawn: {:?}",
            overlay.other_rows
        );
    }
    assert_eq!(
        app.world()
            .resource::<PreferencesOverlaySource>()
            .shell()
            .section()
            .expect("the open section")
            .movies
            .len(),
        1,
        "the CrimFlag.MPG backdrop row is counted, never drawn"
    );
    assert!(
        overlay_parts(&mut app) > 0,
        "the settings screen's rows are on screen"
    );
    assert!(
        !app.world().resource::<PauseMapShown>().0,
        "the map is hidden under the settings screen"
    );
    assert_eq!(parts(&mut app), 0, "the map's parts are gone");
    assert!(app.world().resource::<PlaytestState>().paused);
    assert_eq!(
        app.world()
            .resource::<PlatformInput>()
            .session()
            .pause_reason(),
        Some(PauseReason::PlayerRequest)
    );
    assert_eq!(ticks(&app), frozen, "the clock stays frozen");

    // A click where the hidden PF_B_MAINMENU sits (462,504 — inside its
    // strip and outside Return to Game's) presses nothing: the out-of-game
    // slot is dead like the slot swap leaves it.
    click_at(&mut app, 500.0, 506.0);
    run(&mut app, 2);
    assert!(
        app.world()
            .resource::<PreferencesOverlay>()
            .presses
            .is_empty(),
        "the hidden main-menu slot takes no press"
    );
    assert!(app.world().resource::<PreferencesOverlay>().open);

    // The settings screen's own return: its Return to Game row at its
    // authored rect's centre.
    click_at(&mut app, 455.0 + 100.0, 509.0 + 16.0);
    run(&mut app, 2);
    {
        let overlay = app.world().resource::<PreferencesOverlay>();
        assert!(!overlay.open, "the return row closed the screen");
        assert_eq!(
            overlay
                .presses
                .last()
                .map(|press| (&press.id[..], &press.effect)),
            Some(("PF_B_RETURNTOGAME", &PreferencesEffect::ReturnToGame))
        );
    }
    assert!(
        app.world().resource::<PauseMapShown>().0,
        "the map is drawn again"
    );
    assert_eq!(parts(&mut app), map_parts);
    assert!(
        app.world().resource::<PlaytestState>().paused,
        "still paused: Resume was never triggered"
    );
    assert_eq!(
        app.world()
            .resource::<PlatformInput>()
            .session()
            .pause_reason(),
        Some(PauseReason::PlayerRequest)
    );
    assert_eq!(ticks(&app), frozen);
    run(&mut app, 30);
    assert_eq!(ticks(&app), frozen, "the clock never ran");

    // Esc still closes the pause and lifts the map afterwards: the detour
    // leaves the screen's own close path working.
    tap_escape(&mut app);
    run(&mut app, 4);
    assert!(
        !app.world()
            .resource::<PlatformInput>()
            .session()
            .is_paused(),
        "Esc closes the pause after the detour"
    );
    assert!(!app.world().resource::<PauseMapShown>().0);
    assert_eq!(parts(&mut app), 0);
}

fn game_dir() -> PathBuf {
    let dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must point at the original installation"),
    );
    assert!(
        dir.is_dir(),
        "CS_GAME_DIR {} is not a directory",
        dir.display()
    );
    dir
}

/// The composed retail M01 with the pause screen and the preferences
/// overlay installed as `run_windowed` installs them.
fn composed() -> App {
    let root = game_dir();
    let plan = plan_mission_launch(
        &root,
        MissionLabel::new("M06").expect("a valid label"),
        "The Red Menace",
    )
    .expect("M01's launch closure plans");
    let stage: MissionStage =
        stage_for(&root, &plan).expect("M01's stage reads through the production readers");
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    let screens = FrontEndScreens::open(&root).expect("the screen artwork opens");
    let strings = |id: &str| Some(id.to_owned());
    let map = read_pause_map(&root, &plan.mission_dir, &screens, &strings)
        .expect("M01's Esc screen reads");
    let overlay = read_preferences_overlay(&screens).expect("the Preferences section reads");
    if !app
        .world()
        .contains_resource::<bevy::prelude::Assets<bevy::prelude::Image>>()
    {
        app.init_asset::<bevy::prelude::Image>();
    }
    // `build_headless` has finished its plugins, so `install_pause_map` /
    // `install_preferences_overlay`'s `add_plugins` would refuse; each
    // production plugin's own `build` is run on the finished app instead
    // (`run_windowed` installs them before the finish).
    app.insert_resource(map);
    PauseMapPlugin.build(&mut app);
    app.insert_resource(overlay);
    PreferencesOverlayPlugin.build(&mut app);
    app.world_mut().spawn(Window {
        resolution: (800, 600).into(),
        ..default()
    });
    let timestep = app.world().resource::<Time<Fixed>>().timestep();
    *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
        TimeUpdateStrategy::ManualDuration(timestep);
    app
}

/// **On the retail composition the Esc map's Preferences button opens the
/// overlay over the hidden map, its Return to Game brings the map back and
/// the clock never moves.**
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_parity3_pause_map_02e_02_preferences_button_round_trips_the_overlay_on_retail_m01() {
    let mut app = composed();
    run(&mut app, 120);
    assert!(ticks(&app) > 0, "the mission is flown before the pause");

    tap_escape(&mut app);
    run(&mut app, 2);
    assert!(app.world().resource::<PlaytestState>().paused);
    assert!(app.world().resource::<PauseMapShown>().0);
    let map_parts = parts(&mut app);
    assert!(map_parts > 0, "the Esc screen is drawn");
    let frozen = ticks(&app);

    // The real Preferences button's centre (the record's authored
    // position, 132x28).
    click_at(&mut app, 367.0 + 66.0, 528.0 + 14.0);
    run(&mut app, 2);
    {
        let overlay = app.world().resource::<PreferencesOverlay>();
        assert!(overlay.open, "the press opened the overlay");
        assert!(
            overlay
                .drawn
                .iter()
                .any(|row| row.kind == 'B' && row.id == "PF_B_RETURNTOGAME"),
            "the in-game page's return row is drawn: {:?}",
            overlay.drawn
        );
    }
    assert!(
        overlay_parts(&mut app) > 0,
        "the overlay's rows are on screen"
    );
    assert!(
        !app.world().resource::<PauseMapShown>().0,
        "the map is hidden under the overlay"
    );
    assert_eq!(parts(&mut app), 0, "the map's parts are gone");
    assert!(app.world().resource::<PlaytestState>().paused);
    assert_eq!(
        app.world()
            .resource::<PlatformInput>()
            .session()
            .pause_reason(),
        Some(PauseReason::PlayerRequest)
    );
    assert_eq!(ticks(&app), frozen, "the clock stays frozen");

    // The overlay's Return to Game at its authored rect's centre.
    click_at(&mut app, 455.0 + 100.0, 509.0 + 16.0);
    run(&mut app, 2);
    {
        let overlay = app.world().resource::<PreferencesOverlay>();
        assert!(!overlay.open, "Return to Game closed the overlay");
        assert_eq!(
            overlay
                .presses
                .last()
                .map(|press| (&press.id[..], &press.effect)),
            Some(("PF_B_RETURNTOGAME", &PreferencesEffect::ReturnToGame))
        );
    }
    assert!(
        app.world().resource::<PauseMapShown>().0,
        "the map is drawn again"
    );
    assert_eq!(parts(&mut app), map_parts);
    assert!(
        app.world().resource::<PlaytestState>().paused,
        "still paused: Resume was never triggered"
    );
    assert_eq!(
        app.world()
            .resource::<PlatformInput>()
            .session()
            .pause_reason(),
        Some(PauseReason::PlayerRequest)
    );
    assert_eq!(ticks(&app), frozen);
    run(&mut app, 30);
    assert_eq!(ticks(&app), frozen, "the clock never ran");

    // Esc still closes the pause and lifts the map afterwards: the detour
    // leaves the screen's own close path working.
    tap_escape(&mut app);
    run(&mut app, 4);
    assert!(
        !app.world()
            .resource::<PlatformInput>()
            .session()
            .is_paused(),
        "Esc closes the pause after the detour"
    );
    assert!(!app.world().resource::<PauseMapShown>().0);
    assert_eq!(parts(&mut app), 0);

    teardown(&mut app);
}
