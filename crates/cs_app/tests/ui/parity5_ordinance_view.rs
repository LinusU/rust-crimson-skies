//! Acceptance task #2068: the `OrdinanceLayout` screen in the shell window.
//! Task test prefix: `accept_parity5_ordinance_view_`.
//!
//! Every test runs over **synthetic** data: layout text in the original's
//! *format* and a fixed [`OrdinanceFeed`] standing in for the engine's natives
//! (the real feed is task #2066's). They prove the window's routing and the
//! drawing, and nothing about retail data.

use bevy::app::App;
use bevy::asset::AssetApp;
use bevy::input::InputSystems;
use bevy::input::keyboard::KeyCode;
use bevy::prelude::{IntoScheduleConfigs, PreUpdate, Text2d, With};

use cs_app::ui::front_end::ordinance_layout::{
    OrdinanceLayoutScreen, OrdinanceList, OrdinancePress, OrdinanceSide,
};
use cs_app::ui::front_end::{
    FrontEndShellPlugin, MenuShell, OriginalLayout, headless_app_ordinance_layout,
};

use super::parity5_cabin_plane_construction_hub::{
    TestInput, click_logical, drive, inject, press_button, section, state, tap,
};
use super::parity5_ordinance_layout::{ROWS, feed, names};

fn extents(art: &str) -> Option<(u32, u32)> {
    match art.to_ascii_lowercase().as_str() {
        "bg.jpg" => Some((800, 600)),
        "accept.png" | "cancel.png" => Some((80, 128)),
        _ => None,
    }
}

fn layout() -> OriginalLayout {
    OriginalLayout::parse(ROWS).expect("the fixture parses")
}

fn screen(entered: i32) -> OrdinanceLayoutScreen {
    let mut screen =
        OrdinanceLayoutScreen::from_layout(&layout(), OrdinanceSide::Pilot, entered, &names)
            .expect("the fixture has the rows");
    // A fixed synthetic feed, not the engine's.
    screen.init(feed());
    screen
}

fn shell() -> MenuShell {
    let mut shell = MenuShell::build(&layout(), &extents, &|_: &str| None);
    shell.open("OrdinanceLayout").expect("the section opens");
    shell
}

fn app(entered: i32) -> App {
    let mut app = headless_app_ordinance_layout(shell(), None, screen(entered));
    app.init_resource::<TestInput>();
    app.add_systems(PreUpdate, inject.after(InputSystems));
    app.update();
    app
}

fn screen_of(app: &App) -> &OrdinanceLayoutScreen {
    state(app)
        .ordinance_layout()
        .expect("the app has the screen")
}

fn row_point(app: &App, list: OrdinanceList, slot: usize, row: usize) -> (i32, i32) {
    let r = screen_of(app)
        .item_rect(list, slot, row)
        .expect("a row rect");
    (r.x + 2, r.y + 2)
}

#[test]
fn accept_parity5_ordinance_view_a_a_click_opens_a_list_hover_rolls_and_a_click_chooses() {
    let mut app = app(0);
    assert_eq!(section(&app), "OrdinanceLayout");
    let b = screen_of(&app).ammo_box(0).expect("box").rect;
    click_logical(&mut app, b.x + 2, b.y + 2);
    assert_eq!(screen_of(&app).open_list(), Some((OrdinanceList::Ammo, 0)));
    let (x, y) = row_point(&app, OrdinanceList::Ammo, 0, 3);
    drive(&mut app, |queue| {
        queue.cursor = Some(bevy::prelude::Vec2::new(x as f32, y as f32));
    });
    assert_eq!(screen_of(&app).rolled(), Some(3));
    assert_eq!(screen_of(&app).ammo_description_entry(), 3);
    click_logical(&mut app, x, y);
    assert_eq!(screen_of(&app).open_list(), None);
    assert_eq!(screen_of(&app).ammo_choice(0), Some(3));
    assert_eq!(
        state(&app).ordinance_outcomes().last(),
        Some(&OrdinancePress::Ammo { slot: 0, choice: 3 })
    );
}

#[test]
fn accept_parity5_ordinance_view_b_escape_closes_an_open_list_and_a_dead_box_stays_shut() {
    let mut app = app(0);
    // Position 1 is empty in the feed: its box does not open.
    let dead = screen_of(&app).ammo_box(1).expect("box").rect;
    click_logical(&mut app, dead.x + 2, dead.y + 2);
    assert_eq!(screen_of(&app).open_list(), None);
    assert!(state(&app).ordinance_outcomes().is_empty());
    let b = screen_of(&app).rocket_box(4).expect("box").rect;
    click_logical(&mut app, b.x + 2, b.y + 2);
    assert!(screen_of(&app).open_list().is_some());
    tap(&mut app, KeyCode::Escape);
    assert_eq!(screen_of(&app).open_list(), None);
    assert_eq!(screen_of(&app).rocket_choice(4), Some(0), "unchanged");
    assert_eq!(section(&app), "OrdinanceLayout", "Esc only closed the list");
}

#[test]
fn accept_parity5_ordinance_view_c_accept_and_cancel_buttons_report_by_xq() {
    let mut app = app(1);
    press_button(&mut app, "OL_B_ACCEPT");
    press_button(&mut app, "OL_B_CANCEL");
    assert_eq!(
        state(&app).ordinance_outcomes(),
        [
            OrdinancePress::Accept { leave: false },
            OrdinancePress::Cancel
        ]
    );
    // Entered from the start (XQ 0): cancel is not the screen's.
    let mut app = self::app(0);
    press_button(&mut app, "OL_B_CANCEL");
    press_button(&mut app, "OL_B_ACCEPT");
    assert_eq!(
        state(&app).ordinance_outcomes(),
        [OrdinancePress::Accept { leave: true }]
    );
}

#[test]
fn accept_parity5_ordinance_view_d_the_window_draws_titles_names_boxes_and_the_open_list() {
    let mut shell_app = App::new();
    shell_app.add_plugins(bevy::input::InputPlugin);
    shell_app.add_plugins(bevy::asset::AssetPlugin::default());
    shell_app.init_asset::<bevy::prelude::Image>();
    shell_app.add_message::<bevy::window::CursorMoved>();
    shell_app.init_resource::<bevy::input::ButtonInput<KeyCode>>();
    shell_app.init_resource::<bevy::input::ButtonInput<bevy::input::mouse::MouseButton>>();
    shell_app.add_plugins(
        FrontEndShellPlugin::windowed(shell(), Default::default(), None, Default::default())
            .with_ordinance_layout(screen(0)),
    );
    shell_app.init_resource::<TestInput>();
    shell_app.add_systems(PreUpdate, inject.after(InputSystems));
    shell_app.update();
    let texts = |app: &mut App| {
        let mut query = app.world_mut().query_filtered::<&Text2d, With<Text2d>>();
        query
            .iter(app.world())
            .map(|t| t.0.clone())
            .collect::<Vec<_>>()
    };
    let closed = texts(&mut shell_app);
    // Seven titles, the plane line, four gun names, the held rows of the
    // three live ammunition boxes and the three live rocket boxes.
    for want in [
        "Plane", "Gun A", "None", "Gun C", "Gun D", "ammo 1", "ammo 3", "rocket 2",
    ] {
        assert!(
            closed.iter().any(|t| t.contains(want)),
            "{want} is drawn: {closed:?}"
        );
    }
    assert!(!closed.iter().any(|t| t == "ammo 4"), "no list is open");
    let b = shell_app
        .world()
        .resource::<cs_app::ui::front_end::ShellResource>()
        .ordinance_layout()
        .expect("screen")
        .ammo_box(0)
        .expect("box")
        .rect;
    click_logical(&mut shell_app, b.x + 2, b.y + 2);
    let open = texts(&mut shell_app);
    assert!(
        open.iter().any(|t| t == "ammo 4"),
        "the open list shows its rows"
    );
    assert!(open.len() > closed.len());
    tap(&mut shell_app, KeyCode::Escape);
    assert_eq!(
        texts(&mut shell_app).len(),
        closed.len(),
        "the list is gone"
    );
}
