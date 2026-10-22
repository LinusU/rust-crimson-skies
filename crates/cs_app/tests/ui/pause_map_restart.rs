//! Task PARITY3-PAUSE-MAP-SCREEN.02-C (#1875): the Esc pause screen's Restart
//! button restarts the mission through the host's one restart path.
//!
//! Over the composed retail M01 (`build_headless`, the production stage and
//! host) the pause screen is installed from the real `escape.zrd` entry, the
//! mission is flown, Esc opens the screen and the pointer presses the real
//! Restart button. The host must then serve a new session generation with the
//! authored initial state, the screen must be closed and the clock running.
//! Prefix `accept_parity3_pause_map_02c_`.

use std::path::PathBuf;

use bevy::asset::AssetApp;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput, NativeKey};
use bevy::input::mouse::{MouseButton, MouseButtonInput};
use bevy::prelude::{App, Entity, Plugin, Vec2, Window, With, default};
use bevy::time::{Fixed, Time, TimeUpdateStrategy};
use cs_app::mission_actors::ScriptedAircraftActor;
use cs_app::mission_ai::MissionAiReport;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHost, MissionHostRestartReport, MissionStage, PauseMapPart, PauseMapPlugin,
    PauseMapShown, build_headless, read_pause_map, stage_for, teardown,
};
use cs_app::physics::PhysicsTickLedger;
use cs_app::playtest::PlaytestState;
use cs_app::score::SessionScoreLedger;
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

/// The composed retail M01 with the pause screen installed from its entry.
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
    let source = read_pause_map(&root, &plan.mission_dir, &screens, &strings)
        .expect("M01's Esc screen reads");
    if !app
        .world()
        .contains_resource::<bevy::prelude::Assets<bevy::prelude::Image>>()
    {
        app.init_asset::<bevy::prelude::Image>();
    }
    // `build_headless` has finished its plugins, so `install_pause_map`'s
    // `add_plugins` would refuse; the production plugin's own `build` is run on
    // the finished app instead (`run_windowed` installs it before the finish).
    app.insert_resource(source);
    PauseMapPlugin.build(&mut app);
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

fn entity_named(app: &mut App, name: &str) -> Entity {
    let world = app.world_mut();
    let mut query = world.query::<(Entity, &ScriptedAircraftActor)>();
    query
        .iter(world)
        .find(|(_, actor)| actor.record.name == name)
        .map(|(entity, _)| entity)
        .unwrap_or_else(|| panic!("{name} spawns with the mission's roster"))
}

/// **The Restart button of M01's own pause screen restarts the mission.**
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_parity3_pause_map_02c_restart_button_restarts_the_mission_under_a_new_generation() {
    let mut app = composed();
    run(&mut app, 120);
    let flying = ticks(&app);
    assert!(flying > 0, "the mission is flown before the pause");
    let served_before = app.world().resource::<MissionHost>().served();
    let roster_before = entity_named(&mut app, "bsfury_1");
    assert!(
        !app.world().contains_resource::<MissionHostRestartReport>(),
        "no restart has happened yet"
    );

    tap_escape(&mut app);
    run(&mut app, 2);
    assert!(app.world().resource::<PlaytestState>().paused);
    assert!(app.world().resource::<PauseMapShown>().0);
    let frozen = ticks(&app);
    run(&mut app, 30);
    assert_eq!(ticks(&app), frozen, "the clock is frozen under the screen");

    // The real Restart button's centre (the record's authored position).
    click_at(&mut app, 127.0 + 66.0, 559.0 + 14.0);
    run(&mut app, 2);

    let report = app
        .world()
        .get_resource::<MissionHostRestartReport>()
        .expect("the press reached the host's restart path")
        .clone();
    assert_ne!(
        report.started, report.torn_down,
        "a fresh session generation"
    );
    let host = app.world().resource::<MissionHost>();
    assert_eq!(host.served(), report.served);
    assert_ne!(
        host.served(),
        served_before,
        "the host serves a new session"
    );
    assert_eq!(
        app.world().resource::<SessionScoreLedger>().session(),
        report.served,
        "the score ledger is the fresh session's"
    );
    let ai = app.world().resource::<MissionAiReport>();
    assert!(
        ai.held.iter().any(|entry| entry.starts_with("bsfury_1")),
        "the authored initial state holds the measured sleeper again: {ai:?}"
    );
    assert_ne!(
        entity_named(&mut app, "bsfury_1"),
        roster_before,
        "the roster is respawned for the fresh generation"
    );

    assert!(
        !app.world().resource::<PlaytestState>().paused,
        "pause closed"
    );
    assert!(!app.world().resource::<PauseMapShown>().0);
    assert_eq!(parts(&mut app), 0, "the screen is gone");
    let after_restart = ticks(&app);
    run(&mut app, 60);
    assert!(
        ticks(&app) > after_restart,
        "the clock runs under the fresh generation"
    );

    teardown(&mut app);
}

/// A press on Resume restarts nothing.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_parity3_pause_map_02c_resume_button_does_not_restart() {
    let mut app = composed();
    run(&mut app, 30);
    tap_escape(&mut app);
    run(&mut app, 2);
    click_at(&mut app, 107.0 + 66.0, 528.0 + 14.0);
    run(&mut app, 2);
    assert!(!app.world().resource::<PlaytestState>().paused);
    assert!(
        !app.world().contains_resource::<MissionHostRestartReport>(),
        "Resume asks the host for nothing"
    );
    teardown(&mut app);
}
