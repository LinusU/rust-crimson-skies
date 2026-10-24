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
        ("ms_p_initialpinup1", 256, 256),
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
    ] {
        click_at(&mut app, x + 66.0, y + 14.0);
        assert_eq!(app.world().resource::<Presses>().0.last(), Some(&kind));
    }
    run(&mut app, 30);
    assert_eq!(app.world().resource::<Presses>().0.len(), 2);
    assert!(
        app.world().resource::<PlaytestState>().paused,
        "still paused"
    );
    assert!(app.world().resource::<PauseMapShown>().0, "still open");
    assert_eq!(ticks(&app), frozen);

    // A click on the chart presses nothing.
    click_at(&mut app, 400.0, 300.0);
    assert_eq!(app.world().resource::<Presses>().0.len(), 2);
}

fn exits(app: &mut App) -> Vec<bevy::app::AppExit> {
    let messages = app
        .world()
        .resource::<bevy::prelude::Messages<bevy::app::AppExit>>();
    messages.iter_current_update_messages().cloned().collect()
}

#[test]
fn accept_parity3_pause_map_02d_quit_button_abandons_the_mission_with_a_clean_exit() {
    let mut app = app();
    tap_escape(&mut app);
    run(&mut app, 2);
    // Resume, Restart and Preferences never ask the app to exit.
    click_at(&mut app, 127.0 + 66.0, 559.0 + 14.0);
    click_at(&mut app, 367.0 + 66.0, 528.0 + 14.0);
    assert!(
        !app.world()
            .resource::<bevy::prelude::Messages<bevy::app::AppExit>>()
            .iter_current_update_messages()
            .any(|_| true)
    );

    // Quit presses and the same update writes the exit.
    let window = {
        let mut query = app.world_mut().query_filtered::<Entity, With<Window>>();
        query.single(app.world()).expect("one window")
    };
    app.world_mut()
        .get_mut::<Window>(window)
        .expect("window")
        .set_cursor_position(Some(Vec2::new(347.0 + 66.0, 559.0 + 14.0)));
    app.update();
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window,
    });
    app.update();
    assert_eq!(exits(&mut app), [bevy::app::AppExit::Success]);
    assert_eq!(
        app.world().resource::<Presses>().0.last(),
        Some(&PauseButtonKind::Quit)
    );
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

// ---------------------------------------------------------------- 02-B --

use bevy::prelude::{Assets, Image, Quat, Text, UiTransform, Vec3};
use cs_app::mission_session::{
    MissionPlayerBody, PauseMapLive, PauseObjectiveRow, numbered_flag, pin_flag_bitmap, player_fix,
    world_to_chart_pixel,
};
use cs_app::ui::hud::ObjectiveHud;
use cs_content::objectives_engine::{
    DormantTimers, EngineDeclaration, EngineIdentity, EngineObjective, EngineObjectives,
};

/// Colour byte of a synthetic bitmap: its index in `synthetic_source`'s list
/// plus one (the first texel's value).
fn colour_of(source: &PauseMapSource, name: &str) -> u8 {
    source.artwork[name].rgba()[0]
}

/// M01's shape: four dormant `PRIMARY n` objectives with messages.
fn engine() -> EngineObjectives {
    let objective = |n: u32| {
        EngineObjective::loaded(EngineDeclaration {
            identity: Some(EngineIdentity::from_record(
                "PRIMARY",
                n,
                Some(format!("MSG_OBJ{n}")),
            )),
            begin_dormant: Some(DormantTimers::from_elements(&[-1.0])),
            ..EngineDeclaration::default()
        })
    };
    EngineObjectives::new((1..=4).map(objective).collect()).expect("engine objectives")
}

fn rows_of(engine: &EngineObjectives) -> Vec<PauseObjectiveRow> {
    PauseObjectiveRow::rows_of(&ObjectiveHud::from_engine(engine), None)
}

fn set_live(app: &mut App, rows: Vec<PauseObjectiveRow>) {
    let mut live = app.world_mut().resource_mut::<PauseMapLive>();
    live.rows = rows;
}

/// The colour byte of the picture drawn with its top-left corner at
/// (`left`, `top`) in window pixels, if any.
fn picture_at(app: &mut App, left: f32, top: f32) -> Option<u8> {
    let found = {
        let mut query = app
            .world_mut()
            .query_filtered::<(&bevy::prelude::Node, &ImageNode), With<PauseMapPart>>();
        query
            .iter(app.world())
            .find(|(node, _)| {
                node.left == bevy::prelude::Val::Px(left) && node.top == bevy::prelude::Val::Px(top)
            })
            .map(|(_, image)| image.image.clone())
    }?;
    let images = app.world().resource::<Assets<Image>>();
    images.get(&found)?.data.as_ref().map(|data| data[0])
}

/// The four pins' colours, in objective order (pins are 76x80 centred).
fn pins(app: &mut App) -> Vec<Option<u8>> {
    [
        (506.0, 149.0),
        (396.0, 254.0),
        (181.0, 244.0),
        (106.0, 319.0),
    ]
    .iter()
    .map(|(x, y)| picture_at(app, x - 38.0, y - 40.0))
    .collect()
}

fn list_texts(app: &mut App) -> Vec<String> {
    let mut query = app
        .world_mut()
        .query_filtered::<&Text, With<PauseMapPart>>();
    query
        .iter(app.world())
        .map(|text| text.0.clone())
        .filter(|text| text.starts_with("MSG_OBJ"))
        .collect()
}

fn checks(app: &mut App, colour: u8) -> usize {
    let mut query = app
        .world_mut()
        .query_filtered::<&ImageNode, With<PauseMapPart>>();
    let handles: Vec<_> = query.iter(app.world()).map(|i| i.image.clone()).collect();
    let images = app.world().resource::<Assets<Image>>();
    handles
        .iter()
        .filter(|handle| {
            images
                .get(*handle)
                .and_then(|image| image.data.as_ref())
                .is_some_and(|data| data[0] == colour)
        })
        .count()
}

#[test]
fn accept_parity3_pause_map_02b_numbering_rules_are_the_named_functions() {
    assert_eq!(numbered_flag(1).as_deref(), Some("pin1"));
    assert_eq!(numbered_flag(6).as_deref(), Some("pin6_1"));
    assert_eq!(numbered_flag(8).as_deref(), Some("pin8"));
    assert_eq!(numbered_flag(9), None);
    let row = |completed| PauseObjectiveRow {
        ordinal: 2,
        text: String::new(),
        completed,
    };
    assert_eq!(pin_flag_bitmap("pin6", Some(&row(false))), "pin6");
    assert_eq!(pin_flag_bitmap("pin6", None), "pin6");
    assert_eq!(pin_flag_bitmap("pin6", Some(&row(true))), "pin2");

    // The candidate mapping puts Lighthouse (-7898, 280, -5765) at the
    // finding's (405, 248) in the clip rectangle, moved to where the source
    // rectangle is drawn: (405 - 211 + 16, 248 - 51 + 19).
    let source = synthetic_source();
    let (x, y) = world_to_chart_pixel(&source.scene.chart, -7898.0, -5765.0).expect("a mapping");
    assert!(
        (x - 210.0).abs() < 1.0 && (y - 216.0).abs() < 1.0,
        "{x},{y}"
    );
    // The corners map onto the drawn chart's corners.
    let [left, top, right, bottom] = source.scene.chart.world;
    let (cx, cy) = world_to_chart_pixel(&source.scene.chart, left, -top).expect("corner");
    assert_eq!((cx.round(), cy.round()), (16.0, 19.0));
    let (cx, cy) = world_to_chart_pixel(&source.scene.chart, right, -bottom).expect("corner");
    assert_eq!((cx.round(), cy.round()), (589.0, 519.0));

    // Heading: the nose along -z is the chart's top; +x is clockwise a quarter.
    let up = player_fix(Vec3::ZERO, Quat::IDENTITY);
    assert!(up.heading.abs() < 1e-6);
    let east = player_fix(
        Vec3::ZERO,
        Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2),
    );
    assert!((east.heading - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
}

#[test]
fn accept_parity3_pause_map_02b_a_completed_objective_changes_its_pin_from_question_mark_to_number()
{
    let source = synthetic_source();
    let (question, one, two, four) = (
        colour_of(&source, "pin6"),
        colour_of(&source, "pin1"),
        colour_of(&source, "pin2"),
        colour_of(&source, "pin4"),
    );
    let mut engine = engine();
    let mut app = app();
    set_live(&mut app, rows_of(&engine));
    tap_escape(&mut app);
    run(&mut app, 3);
    // The owner's capture: objective 1 is the live one and its pin is still
    // "?", as are 2 and 3; objective 4's authored flag is already its number.
    assert_eq!(
        pins(&mut app),
        [Some(question), Some(question), Some(question), Some(four)]
    );

    // Waking objective 2 changes no pin.
    engine.wake(&[1]);
    set_live(&mut app, rows_of(&engine));
    run(&mut app, 3);
    assert_eq!(
        pins(&mut app),
        [Some(question), Some(question), Some(question), Some(four)]
    );

    // Completing objectives 1 and 2 turns their pins into their numbers.
    engine.complete(0);
    engine.complete(1);
    set_live(&mut app, rows_of(&engine));
    run(&mut app, 3);
    assert_eq!(
        pins(&mut app),
        [Some(one), Some(two), Some(question), Some(four)]
    );
}

#[test]
fn accept_parity3_pause_map_02b_the_note_lists_every_objective_with_check_marks() {
    let source = synthetic_source();
    let check = colour_of(&source, "obj_check1");
    let mut engine = engine();
    let mut app = app();
    set_live(&mut app, rows_of(&engine));
    tap_escape(&mut app);
    run(&mut app, 3);
    // The owner's capture lists all four with the player still at the coast.
    let all = ["MSG_OBJ1", "MSG_OBJ2", "MSG_OBJ3", "MSG_OBJ4"];
    assert_eq!(list_texts(&mut app), all);
    assert_eq!(checks(&mut app, check), 0);

    engine.wake(&[1]);
    set_live(&mut app, rows_of(&engine));
    run(&mut app, 3);
    assert_eq!(list_texts(&mut app), all);

    engine.complete(0);
    set_live(&mut app, rows_of(&engine));
    run(&mut app, 3);
    assert_eq!(list_texts(&mut app), all);
    assert_eq!(
        checks(&mut app, check),
        1,
        "a completed objective is ticked"
    );
}

#[test]
fn accept_parity3_pause_map_02b_the_player_icon_follows_position_and_heading() {
    let source = synthetic_source();
    let icon = colour_of(&source, "singledev");
    let mut app = app();
    let body = app
        .world_mut()
        .spawn((
            MissionPlayerBody,
            bevy::prelude::Transform::from_xyz(-7898.0, 280.0, -5765.0),
            avian3d::prelude::Position(Vec3::new(-7898.0, 280.0, -5765.0)),
            avian3d::prelude::Rotation(Quat::IDENTITY),
        ))
        .id();
    tap_escape(&mut app);
    run(&mut app, 3);

    let icon_node = |app: &mut App| {
        let mut query = app
            .world_mut()
            .query_filtered::<(&bevy::prelude::Node, &ImageNode, &UiTransform), With<PauseMapPart>>(
            );
        let found: Vec<_> = query
            .iter(app.world())
            .map(|(node, image, transform)| {
                (node.left, node.top, image.image.clone(), transform.rotation)
            })
            .collect();
        let images = app.world().resource::<Assets<Image>>();
        let mut hits = found.into_iter().filter(|(_, _, handle, _)| {
            images
                .get(handle)
                .and_then(|image| image.data.as_ref())
                .is_some_and(|data| data[0] == icon)
        });
        let hit = hits.next().expect("the icon is drawn");
        assert!(hits.next().is_none(), "one icon");
        hit
    };
    let expect = |x: f32, z: f32| {
        let (px, py) = world_to_chart_pixel(&source.scene.chart, x, z).expect("a mapping");
        (
            bevy::prelude::Val::Px(px - 16.0 - 16.0),
            bevy::prelude::Val::Px(py - 19.0 - 16.0),
        )
    };
    let (left, top, _, rotation) = icon_node(&mut app);
    assert_eq!((left, top), expect(-7898.0, -5765.0));
    assert!(rotation.as_radians().abs() < 1e-5);

    // Moved and turned a quarter to the right while the screen is up.
    {
        let mut entity = app.world_mut().entity_mut(body);
        let turn = Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2);
        entity.insert((
            bevy::prelude::Transform::from_xyz(-4897.0, 200.0, -5018.0).with_rotation(turn),
            avian3d::prelude::Position(Vec3::new(-4897.0, 200.0, -5018.0)),
            avian3d::prelude::Rotation(turn),
        ));
    }
    run(&mut app, 3);
    let (left, top, _, rotation) = icon_node(&mut app);
    assert_eq!((left, top), expect(-4897.0, -5018.0));
    assert!((rotation.as_radians() - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity3_pause_map_02b_reads_the_objective_rows_icon_and_list_records() {
    let install = game_dir();
    let screens = FrontEndScreens::open(&install).expect("the screen artwork opens");
    let strings = |id: &str| Some(format!("<{id}>"));
    let source =
        read_pause_map(&install, "zbd/c3/m01", &screens, &strings).expect("M01's Esc screen reads");
    let scene = &source.scene;
    assert_eq!(
        scene.objectives,
        [
            ("OBJ1".to_owned(), 0),
            ("OBJ2".to_owned(), 1),
            ("OBJ3".to_owned(), 2),
            ("OBJ4".to_owned(), 3),
        ]
    );
    assert_eq!(scene.own_ship, "singledev");
    assert_eq!(scene.note.check_mark, "obj_check1");
    assert_eq!(scene.note.list_at, point(580, 50));
    assert_eq!(scene.note.list_wrap, (190, 255));
    assert_eq!(scene.note.list_spacing, 5);
    for (name, width, height) in [
        ("singledev", 32, 32),
        ("obj_check1", 25, 24),
        ("pin1", 76, 80),
        ("pin6_1", 76, 80),
        ("pin8", 76, 80),
    ] {
        let art = source
            .artwork
            .get(name)
            .unwrap_or_else(|| panic!("no {name}"));
        assert_eq!((art.width(), art.height()), (width, height), "{name}");
    }
}

/// #1984: `CLIP` is the bitmap's source rectangle and `POSITION` where it is
/// drawn, so the island art fills the screen from the frame's left edge
/// rather than starting at the clip's screen offset.
#[test]
fn accept_parity3_pause_map_02a_chart_draws_the_source_rectangle_at_its_position() {
    use bevy::prelude::{Node, Overflow, Val};
    let source = synthetic_source();
    let chart_colour = colour_of(&source, "ha-m1map");
    let mut app = app();
    tap_escape(&mut app);
    run(&mut app, 3);

    let container = {
        let mut query = app
            .world_mut()
            .query_filtered::<&Node, (With<PauseMapPart>, bevy::prelude::Without<ImageNode>)>();
        query
            .iter(app.world())
            .find(|node| node.overflow == Overflow::clip())
            .map(|node| (node.left, node.top, node.width, node.height))
            .expect("the chart's clip container")
    };
    assert_eq!(
        container,
        (Val::Px(16.0), Val::Px(19.0), Val::Px(573.0), Val::Px(500.0)),
        "the container is the source rectangle's size, drawn at POSITION"
    );
    assert_eq!(
        picture_at(&mut app, -211.0, -51.0),
        Some(chart_colour),
        "the bitmap is shifted so the source rectangle's corner meets the container's"
    );
}

#[test]
fn accept_parity3_pause_map_gap_05_photo_replaces_the_placeholder_portrait() {
    let mut scene = synthetic_source().scene;
    assert_eq!(scene.memento.0, "momento_temp");
    assert_eq!(scene.memento_photo(), "ms_p_initialpinup1");
    assert!(
        scene
            .bitmaps()
            .iter()
            .any(|name| name == "ms_p_initialpinup1")
    );
    // A bitmap the script names otherwise is drawn as named.
    scene.memento.0 = "ms_p_mom".to_owned();
    assert_eq!(scene.memento_photo(), "ms_p_mom");
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity3_pause_map_gap_05_retail_photo_is_the_default_pinup_texture() {
    let install = game_dir();
    let screens = FrontEndScreens::open(&install).expect("the screen artwork opens");
    let strings = |id: &str| Some(format!("<{id}>"));
    let source =
        read_pause_map(&install, "zbd/c3/m01", &screens, &strings).expect("M01's Esc screen reads");
    assert_eq!(source.scene.memento_photo(), "ms_p_initialpinup1");
    let art = &source.artwork["ms_p_initialpinup1"];
    assert_eq!((art.width(), art.height()), (256, 256));
}

fn cursor_node(app: &mut App) -> bevy::prelude::Node {
    let mut query = app
        .world_mut()
        .query_filtered::<&bevy::prelude::Node, With<cs_app::mission_session::PauseMapPart>>();
    // The cursor is the only 40x40 picture that tracks the pointer; find it by
    // the position the pointer was moved to.
    query
        .iter(app.world())
        .find(|node| node.left == bevy::prelude::Val::Px(300.0))
        .expect("the cursor node follows the pointer")
        .clone()
}

#[test]
fn accept_parity3_pause_map_gap_07_cursor_is_drawn_once_the_pointer_has_a_position() {
    let mut app = app();
    tap_escape(&mut app);
    run(&mut app, 2);
    let window = {
        let mut query = app.world_mut().query_filtered::<Entity, With<Window>>();
        query.single(app.world()).expect("one window")
    };
    app.world_mut()
        .get_mut::<Window>(window)
        .expect("window")
        .set_cursor_position(Some(Vec2::new(300.0, 250.0)));
    run(&mut app, 2);
    let node = cursor_node(&mut app);
    assert_eq!(node.display, bevy::prelude::Display::Flex);
    assert_eq!(node.top, bevy::prelude::Val::Px(250.0));
    assert_eq!(node.width, bevy::prelude::Val::Px(40.0));
}
