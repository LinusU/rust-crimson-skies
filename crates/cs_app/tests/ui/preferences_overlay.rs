//! Task PARITY3-PAUSE-MAP-SCREEN.02-E.01 (#1922): the retail `Preferences`
//! section of `ASSETS/LAYOUT.CSV` is drawn over the mission window.
//! Prefix `accept_parity3_pause_map_02e_01_`.

use std::path::PathBuf;

use bevy::asset::AssetApp;
use bevy::input::ButtonState;
use bevy::input::mouse::{MouseButton, MouseButtonInput};
use bevy::prelude::{App, Entity, Node, Plugin, Val, Vec2, Window, With, default};
use bevy::time::{Fixed, Time, TimeUpdateStrategy};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    ClosePreferencesOverlay, MissionStage, OpenPreferencesOverlay, PreferencesEffect,
    PreferencesOverlay, PreferencesOverlayPart, PreferencesOverlayPlugin, build_headless,
    read_preferences_overlay, stage_for, teardown,
};
use cs_app::physics::PhysicsTickLedger;
use cs_app::ui::front_end::FrontEndScreens;
use cs_content::campaign_bindings::MissionLabel;

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

/// The composed retail M01 with the preferences overlay installed.
fn composed() -> App {
    let root = game_dir();
    let plan = plan_mission_launch(
        &root,
        MissionLabel::new("M06").expect("a valid label"),
        "The Red Menace",
    )
    .expect("the launch closure plans");
    let stage: MissionStage = stage_for(&root, &plan).expect("the stage reads");
    let mut app = build_headless(&stage).expect("the stage composes headlessly");
    let screens = FrontEndScreens::open(&root).expect("the screen artwork opens");
    let source = read_preferences_overlay(&screens).expect("the Preferences section reads");
    if !app
        .world()
        .contains_resource::<bevy::prelude::Assets<bevy::prelude::Image>>()
    {
        app.init_asset::<bevy::prelude::Image>();
    }
    app.insert_resource(source);
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

fn run(app: &mut App, frames: u32) {
    for _ in 0..frames {
        app.update();
    }
}

fn parts(app: &mut App) -> usize {
    app.world_mut()
        .query_filtered::<Entity, With<PreferencesOverlayPart>>()
        .iter(app.world())
        .count()
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

fn open(app: &mut App) {
    app.world_mut().write_message(OpenPreferencesOverlay);
    run(app, 3);
}

fn ticks(app: &App) -> u64 {
    app.world().resource::<PhysicsTickLedger>().ticks
}

/// Every part's authored-space placement (the window is 800x600, so scale 1).
fn placements(app: &mut App) -> Vec<(f32, f32, f32, f32)> {
    let px = |v: Val| match v {
        Val::Px(px) => px,
        other => panic!("a part is placed in pixels, not {other:?}"),
    };
    app.world_mut()
        .query_filtered::<&Node, With<PreferencesOverlayPart>>()
        .iter(app.world())
        .map(|n| (px(n.left), px(n.top), px(n.width), px(n.height)))
        .collect()
}

/// **The retail Preferences section is drawn at its authored positions, the
/// Return to Game button closes it, and the clock is not touched.**
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_parity3_pause_map_02e_01_rows_spawn_at_authored_positions_and_return_closes() {
    let mut app = composed();
    run(&mut app, 30);
    assert!(!app.world().resource::<PreferencesOverlay>().open);
    assert_eq!(parts(&mut app), 0, "nothing is drawn before it is opened");

    open(&mut app);
    let record = app.world().resource::<PreferencesOverlay>();
    assert!(record.open);
    let drawn: Vec<_> = record
        .drawn
        .iter()
        .map(|row| {
            let r = row.logical;
            (row.id.as_str(), row.kind, (r.x, r.y, r.width, r.height))
        })
        .collect();
    assert_eq!(
        drawn,
        vec![
            ("PF_BACKGROUND", 'P', (110, 215, 566, 331)),
            ("PF_LOGO", 'P', (134, 13, 544, 240)),
            ("PF_B_GAMEOPTIONS", 'B', (115, 272, 221, 42)),
            ("PF_B_AUDIO", 'B', (115, 323, 221, 42)),
            ("PF_B_VIDEO", 'B', (115, 374, 221, 42)),
            ("PF_B_CONTROLS", 'B', (115, 425, 221, 42)),
            ("PF_B_RETURNTOGAME", 'B', (455, 509, 200, 32)),
        ],
        "the in-game page's rows at the layout's own positions; the out-of-game \
         PF_B_MAINMENU is not among them"
    );
    assert!(record.skipped.is_empty(), "{:?}", record.skipped);
    assert!(
        record.other_rows.iter().any(|kind| kind == "M"),
        "the backdrop movie row is counted, never drawn: {:?}",
        record.other_rows
    );
    let placed = placements(&mut app);
    assert_eq!(placed.len(), 7, "one UI node per drawn row");
    assert!(placed.contains(&(455.0, 509.0, 200.0, 32.0)));
    assert!(placed.contains(&(110.0, 215.0, 566.0, 331.0)));

    let before = ticks(&app);
    // A press where only the out-of-game button would be does nothing.
    click_at(&mut app, 500.0, 506.0);
    assert!(
        app.world()
            .resource::<PreferencesOverlay>()
            .presses
            .is_empty()
    );
    // A press on Audio is recorded with its stated destination; the page stays.
    click_at(&mut app, 115.0 + 100.0, 323.0 + 20.0);
    {
        let record = app.world().resource::<PreferencesOverlay>();
        assert!(record.open, "Audio's section is not the overlay's to show");
        assert_eq!(record.presses.len(), 1);
        assert_eq!(record.presses[0].id, "PF_B_AUDIO");
        assert_eq!(
            record.presses[0].effect,
            PreferencesEffect::Navigate("Audio".to_owned())
        );
    }
    // The return button closes it.
    click_at(&mut app, 455.0 + 100.0, 509.0 + 16.0);
    let record = app.world().resource::<PreferencesOverlay>();
    assert!(!record.open, "Return to Game closes the page");
    assert_eq!(
        record
            .presses
            .last()
            .map(|press| (&press.id[..], &press.effect)),
        Some(("PF_B_RETURNTOGAME", &PreferencesEffect::ReturnToGame))
    );
    assert_eq!(parts(&mut app), 0, "its nodes are gone");
    assert!(ticks(&app) > before, "the overlay never held the clock");

    // It opens again, fresh.
    open(&mut app);
    assert!(app.world().resource::<PreferencesOverlay>().open);
    assert_eq!(parts(&mut app), 7);
    app.world_mut().write_message(ClosePreferencesOverlay);
    run(&mut app, 2);
    assert!(!app.world().resource::<PreferencesOverlay>().open);
    assert_eq!(parts(&mut app), 0);

    teardown(&mut app);
}
