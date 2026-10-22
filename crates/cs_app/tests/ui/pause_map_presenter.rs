//! Task PARITY3-PAUSE-MAP-SCREEN.02-A (#1873): the Esc pause screen presenter.
//!
//! The headless tests drive the production Esc path (`playtest::meta_controls`
//! through the input session's pause and `sync_pause`'s clock freeze) with the
//! presenter installed over the playtest app, a window for the pointer and
//! synthetic bitmaps of the retail sizes. The retail test reads M01's entry of
//! `escape.zrd` through the production reader. Prefix
//! `accept_parity3_pause_map_02a_`.

use std::path::PathBuf;

use bevy::asset::AssetApp;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput, NativeKey};
use bevy::input::mouse::{MouseButton, MouseButtonInput};
use bevy::prelude::{App, Entity, ImageNode, Resource, Vec2, Window, With, default};
use cs_app::mission_session::{
    PauseButtonKind, PauseMapPart, PauseMapPress, PauseMapScene, PauseMapShown, PauseMapSource,
    install_pause_map, read_pause_map,
};
use cs_app::physics::PhysicsTickLedger;
use cs_app::playtest::{PlaytestState, headless_app_with};
use cs_app::ui::front_end::{Artwork, FrontEndScreens};
use cs_content::briefing::Point;

fn point(x: i32, y: i32) -> Point {
    Point { x, y }
}

/// M01's scene as the record states it, with synthetic bitmaps of the retail
/// extents.
fn synthetic_source() -> PauseMapSource {
    use cs_app::mission_session::PauseMapScene as Scene;
    let sizes = [
        ("loadframe", 801, 601),
        ("ha-m1map", 800, 600),
        ("momento_temp", 242, 305),
        ("momento_shad", 300, 300),
        ("pin6", 76, 80),
        ("pin4", 76, 80),
        ("parchment", 240, 312),
        ("escape_button1", 132, 28),
        ("escape_button2", 132, 28),
        ("escape_button3", 132, 28),
        ("daglove", 40, 40),
        ("dafinger", 40, 40),
    ];
    let artwork = sizes
        .iter()
        .map(|(name, width, height)| {
            let rgba = vec![200u8; (*width as usize) * (*height as usize) * 4];
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
        },
        buttons: vec![
            button(PauseButtonKind::Resume, 107, 528, "Resume"),
            button(PauseButtonKind::Restart, 127, 559, "Restart"),
            button(PauseButtonKind::Preferences, 367, 528, "Preferences"),
            button(PauseButtonKind::Quit, 347, 559, "Quit"),
        ],
        cursor: ("daglove".to_owned(), "dafinger".to_owned()),
        unread_commands: Vec::new(),
    };
    PauseMapSource { scene, artwork }
}

#[derive(Resource, Default)]
struct Presses(Vec<PauseButtonKind>);

fn collect(
    mut reader: bevy::prelude::MessageReader<PauseMapPress>,
    mut out: bevy::prelude::ResMut<Presses>,
) {
    out.0.extend(reader.read().map(|press| press.0));
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
        app.init_resource::<Presses>()
            .add_systems(bevy::prelude::Last, collect);
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

fn images(app: &mut App) -> usize {
    app.world_mut()
        .query_filtered::<Entity, (With<PauseMapPart>, With<ImageNode>)>()
        .iter(app.world())
        .count()
}

fn run(app: &mut App, frames: u32) {
    for _ in 0..frames {
        app.update();
    }
}

#[test]
fn accept_parity3_pause_map_02a_esc_opens_the_chart_screen_with_the_clock_frozen() {
    let mut app = app();
    run(&mut app, 30);
    assert_eq!(parts(&mut app), 0, "nothing is drawn while flying");
    assert!(ticks(&app) > 0, "the clock runs while flying");

    tap_escape(&mut app);
    run(&mut app, 2);
    assert!(app.world().resource::<PlaytestState>().paused);
    assert!(app.world().resource::<PauseMapShown>().0);
    // background, the chart's clipped bitmap, shadow, photo, parchment, four
    // pins, four buttons and the cursor are pictures; the backdrop, the chart's
    // clip container, the title and four labels are not.
    assert_eq!(images(&mut app), 1 + 1 + 1 + 1 + 1 + 4 + 4 + 1);
    assert_eq!(parts(&mut app), 1 + images(&mut app) + 1 + 1 + 4);

    let frozen = ticks(&app);
    run(&mut app, 60);
    assert_eq!(
        ticks(&app),
        frozen,
        "no fixed tick runs while the map is open"
    );
    assert_eq!(
        app.world().resource::<PlaytestState>().ticks_while_paused,
        0
    );

    // Esc closes it again.
    tap_escape(&mut app);
    run(&mut app, 30);
    assert_eq!(parts(&mut app), 0);
    assert!(ticks(&app) > frozen);
}

#[test]
fn accept_parity3_pause_map_02a_resume_button_closes_it_and_the_clock_runs() {
    let mut app = app();
    tap_escape(&mut app);
    run(&mut app, 2);
    let frozen = ticks(&app);

    // The Resume button's centre.
    click_at(&mut app, 107.0 + 66.0, 528.0 + 14.0);
    run(&mut app, 30);
    assert!(!app.world().resource::<PlaytestState>().paused);
    assert!(!app.world().resource::<PauseMapShown>().0);
    assert_eq!(parts(&mut app), 0);
    assert!(ticks(&app) > frozen, "the clock runs after Resume");
    assert_eq!(
        app.world().resource::<Presses>().0,
        [PauseButtonKind::Resume]
    );
}

#[test]
fn accept_parity3_pause_map_02a_other_buttons_report_a_press_and_change_nothing() {
    let mut app = app();
    tap_escape(&mut app);
    run(&mut app, 2);
    let frozen = ticks(&app);
    for (kind, x, y) in [
        (PauseButtonKind::Restart, 127.0, 559.0),
        (PauseButtonKind::Preferences, 367.0, 528.0),
        (PauseButtonKind::Quit, 347.0, 559.0),
    ] {
        click_at(&mut app, x + 66.0, y + 14.0);
        assert_eq!(app.world().resource::<Presses>().0.last(), Some(&kind));
    }
    run(&mut app, 30);
    assert_eq!(app.world().resource::<Presses>().0.len(), 3);
    assert!(
        app.world().resource::<PlaytestState>().paused,
        "still paused"
    );
    assert!(app.world().resource::<PauseMapShown>().0, "still open");
    assert_eq!(ticks(&app), frozen);

    // A click on the chart presses nothing.
    click_at(&mut app, 400.0, 300.0);
    assert_eq!(app.world().resource::<Presses>().0.len(), 3);
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

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity3_pause_map_02a_reads_the_m01_entry_and_its_art() {
    let install = game_dir();
    let screens = FrontEndScreens::open(&install).expect("the screen artwork opens");
    let strings = |id: &str| Some(format!("<{id}>"));
    let source =
        read_pause_map(&install, "zbd/c3/m01", &screens, &strings).expect("M01's Esc screen reads");
    let scene = &source.scene;
    assert_eq!(scene.key, "loading_c61");
    assert_eq!(scene.chart.bitmap, "HA-m1MAP");
    assert_eq!(scene.chart.at, point(16, 19));
    assert_eq!(scene.chart.clip_top_left, point(211, 51));
    assert_eq!(scene.chart.clip_bottom_right, point(784, 551));
    assert_eq!(scene.memento, ("momento_temp".to_owned(), point(533, 326)));
    let pins: Vec<_> = scene
        .pins()
        .map(|pin| (pin.bitmap.as_str(), pin.at.x, pin.at.y, pin.centered))
        .collect();
    assert_eq!(
        pins,
        [
            ("pin6", 506, 149, true),
            ("pin6", 396, 254, true),
            ("pin6", 181, 244, true),
            ("pin4", 106, 319, true),
        ]
    );
    assert_eq!(scene.note.title, "<MSG_BRF_DLG_OBJECTIVES>");
    assert_eq!(scene.note.at, point(555, 6));
    let buttons: Vec<_> = scene
        .buttons
        .iter()
        .map(|button| {
            (
                button.kind,
                button.at.x,
                button.at.y,
                button.label_text.as_str(),
            )
        })
        .collect();
    assert_eq!(
        buttons,
        [
            (PauseButtonKind::Resume, 107, 528, "MSG_BTN_RESUME"),
            (PauseButtonKind::Restart, 127, 559, "MSG_BTN_RESTART"),
            (
                PauseButtonKind::Preferences,
                367,
                528,
                "MSG_BTN_PREFERENCES"
            ),
            (PauseButtonKind::Quit, 347, 559, "MSG_BTN_QUIT"),
        ]
    );
    assert_eq!(scene.cursor, ("daglove".to_owned(), "dafinger".to_owned()));
    assert!(
        scene.unread_commands.is_empty(),
        "unread script commands: {:?}",
        scene.unread_commands
    );
    // Every bitmap the scene names was decoded at its retail extent.
    for (name, width, height) in [
        ("ha-m1map", 800, 600),
        ("loadframe", 801, 601),
        ("pin6", 76, 80),
        ("pin4", 76, 80),
        ("parchment", 240, 312),
        ("momento_temp", 242, 305),
        ("momento_shad", 300, 300),
        ("escape_button1", 132, 28),
        ("escape_button3", 132, 28),
        ("daglove", 40, 40),
    ] {
        let art = source
            .artwork
            .get(name)
            .unwrap_or_else(|| panic!("no {name}"));
        assert_eq!((art.width(), art.height()), (width, height), "{name}");
    }
    for name in scene.bitmaps() {
        assert!(
            source.artwork.contains_key(&name.to_ascii_lowercase()),
            "{name}"
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity3_pause_map_02a_a_missing_entry_or_string_is_refused() {
    let install = game_dir();
    let screens = FrontEndScreens::open(&install).expect("the screen artwork opens");
    // A string that does not resolve is an error, not a blank label.
    let none = |_: &str| None;
    let error = read_pause_map(&install, "zbd/c3/m01", &screens, &none)
        .expect_err("unresolved strings refuse");
    assert!(error.0.contains("does not resolve"), "{error}");
    // A mission that is not in the campaign layout is refused.
    let strings = |id: &str| Some(id.to_owned());
    let error = read_pause_map(&install, "zbd/c9/m99", &screens, &strings)
        .expect_err("an unknown mission refuses");
    assert!(error.0.contains("not in the campaign layout"), "{error}");
}
