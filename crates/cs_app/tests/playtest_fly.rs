//! Task #647 (PLAYTEST-FLY-NOW) acceptance tests. Prefix: `accept_playtest_fly_`.
//!
//! These drive [`cs_app::playtest::headless_app`], which is the playtest's own
//! composition (the same plugin, systems, input session, production flight
//! driver, Avian world and chase camera as the window) minus the window, the
//! GPU and wall-clock time. Keys are pressed in Bevy's `ButtonInput<KeyCode>`
//! exactly as the keyboard system would leave them.

use avian3d::prelude::{Position, Rotation};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput, NativeKey};
use bevy::prelude::{App, Entity, Transform, Vec3, With};
use bevy::window::WindowFocused;
use cs_app::cli::{self, CliRequest};
use cs_app::input::platform::PlatformInput;
use cs_app::physics::{FlightAircraft, PhysicsTickLedger};
use cs_app::playtest::scene::{PlaytestAircraft, SPAWN_POSITION_M};
use cs_app::playtest::smoke::{SMOKE_FRAME_HZ, SmokePlugin};
use cs_app::playtest::{
    PlaytestCamera, PlaytestCameraMarker, PlaytestState, SmokeRequest, headless_app,
    headless_app_with,
};

fn frames(seconds: f64) -> u32 {
    (seconds * SMOKE_FRAME_HZ).round() as u32
}

fn run(app: &mut App, seconds: f64) {
    for _ in 0..frames(seconds) {
        app.update();
    }
}

fn key_message(app: &mut App, key: KeyCode, state: ButtonState) {
    app.world_mut().write_message(KeyboardInput {
        key_code: key,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
}

fn press(app: &mut App, key: KeyCode) {
    key_message(app, key, ButtonState::Pressed);
}

fn release(app: &mut App, key: KeyCode) {
    key_message(app, key, ButtonState::Released);
}

fn tap(app: &mut App, key: KeyCode) {
    press(app, key);
    app.update();
    release(app, key);
    app.update();
}

fn state(app: &App) -> PlaytestState {
    app.world().resource::<PlaytestState>().clone()
}

fn ticks(app: &App) -> u64 {
    app.world().resource::<PhysicsTickLedger>().ticks
}

fn count<T: bevy::prelude::Component>(app: &mut App) -> usize {
    app.world_mut().query::<&T>().iter(app.world()).count()
}

/// Flies `key` for `seconds` from a fresh start and returns the final state.
fn fly_with(key: Option<KeyCode>, seconds: f64) -> PlaytestState {
    let mut app = headless_app();
    app.update();
    if let Some(key) = key {
        press(&mut app, key);
    }
    run(&mut app, seconds);
    state(&app)
}

#[test]
fn accept_playtest_fly_keys_change_the_production_aircraft_in_both_directions() {
    let neutral = fly_with(None, 1.0);
    // Pitch: S is nose up, W nose down.
    let up = fly_with(Some(KeyCode::KeyS), 1.0);
    let down = fly_with(Some(KeyCode::KeyW), 1.0);
    assert!(
        up.telemetry.pitch_deg > neutral.telemetry.pitch_deg + 5.0,
        "{up:?}"
    );
    assert!(
        down.telemetry.pitch_deg < neutral.telemetry.pitch_deg - 5.0,
        "{down:?}"
    );
    // Roll: E is right wing down (positive bank), Q the other way.
    let right = fly_with(Some(KeyCode::KeyE), 1.5);
    let left = fly_with(Some(KeyCode::KeyQ), 1.5);
    assert!(right.telemetry.roll_deg > 2.0, "{right:?}");
    assert!(left.telemetry.roll_deg < -2.0, "{left:?}");
    // Yaw: D turns the heading to the right (increasing), A to the left.
    let yaw_right = fly_with(Some(KeyCode::KeyD), 1.5);
    let yaw_left = fly_with(Some(KeyCode::KeyA), 1.5);
    assert!(yaw_right.telemetry.heading_deg > 1.0, "{yaw_right:?}");
    assert!(yaw_left.telemetry.heading_deg < -1.0, "{yaw_left:?}");
    // Throttle: Left Shift steps up, F steps down, and the engine answers.
    let more = fly_with(Some(KeyCode::Digit4), 4.0);
    let less = fly_with(Some(KeyCode::Digit1), 4.0);
    assert!(more.command.throttle > 0.99 && less.command.throttle < 0.01);
    assert!(more.telemetry.speed_m_s > less.telemetry.speed_m_s + 3.0);
    assert!(more.input_changes >= 1);

    let mut app = headless_app();
    app.update();
    tap(&mut app, KeyCode::ShiftLeft);
    let stepped_up = state(&app).command.throttle;
    tap(&mut app, KeyCode::KeyF);
    tap(&mut app, KeyCode::KeyF);
    let stepped_down = state(&app).command.throttle;
    assert!(stepped_up > 0.75 && stepped_down < stepped_up);
}

#[test]
fn accept_playtest_fly_camera_follows_the_simulated_pose() {
    let mut app = headless_app();
    app.update();
    run(&mut app, 1.0);
    let aircraft_at = |app: &mut App| {
        let (p, r) = app
            .world_mut()
            .query_filtered::<(&Position, &Rotation), With<PlaytestAircraft>>()
            .single(app.world())
            .map(|(p, r)| (p.0, r.0))
            .expect("one aircraft");
        (p, r)
    };
    let camera_at = |app: &mut App| {
        app.world_mut()
            .query_filtered::<&Transform, With<PlaytestCameraMarker>>()
            .single(app.world())
            .expect("one camera")
            .translation
    };
    let (before_p, _) = aircraft_at(&mut app);
    let camera_before = camera_at(&mut app);
    run(&mut app, 2.0);
    let (after_p, rotation) = aircraft_at(&mut app);
    let camera_after = camera_at(&mut app);
    assert!(before_p.distance(after_p) > 100.0, "the aircraft flew");
    assert!(
        camera_before.distance(camera_after) > 100.0,
        "the camera moved with it"
    );
    // The chase rig sits behind and above the authoritative pose.
    let offset = camera_after - after_p;
    let behind = offset.dot(rotation * Vec3::Z);
    assert!(
        behind > 5.0 && behind < 20.0,
        "camera is astern: {offset:?}"
    );
    assert!(offset.dot(rotation * Vec3::Y) > 0.5, "camera is above");
    let camera = app.world().resource::<PlaytestCamera>();
    assert!(camera.frame.is_some());
}

#[test]
fn accept_playtest_fly_collision_with_the_obstacle_is_observable_and_solid() {
    let mut app = headless_app();
    app.update();
    for _ in 0..frames(15.0) {
        app.update();
        if state(&app).obstacle_contacts > 0 {
            break;
        }
    }
    run(&mut app, 1.0);
    let s = state(&app);
    assert_eq!(s.obstacle_contacts, 1, "{s:?}");
    assert!(s.first_obstacle_contact_tick.is_some());
    // The tower's near face is at z = -425; the aircraft is stopped by it.
    assert!(s.telemetry.position_m[2] > -425.0 - 1.0, "{s:?}");
    assert!(s.telemetry.speed_m_s < 40.0, "the impact slowed it: {s:?}");
}

#[test]
fn accept_playtest_fly_repeated_reset_keeps_one_player_one_camera_one_clock() {
    let mut app = headless_app();
    app.update();
    press(&mut app, KeyCode::KeyS);
    run(&mut app, 1.0);
    release(&mut app, KeyCode::KeyS);
    let mut last_ticks = ticks(&app);
    for reset in 1..=3 {
        tap(&mut app, KeyCode::KeyR);
        run(&mut app, 0.5);
        assert_eq!(count::<PlaytestAircraft>(&mut app), 1);
        assert_eq!(count::<FlightAircraft>(&mut app), 1);
        assert_eq!(count::<PlaytestCameraMarker>(&mut app), 1);
        assert_eq!(count::<PlatformInput>(&mut app), 1, "one input session");
        let s = state(&app);
        assert_eq!(s.resets, reset);
        assert!(
            ticks(&app) > last_ticks,
            "the one fixed clock keeps counting"
        );
        last_ticks = ticks(&app);
        // Known flyable state: level, near spawn, cruise throttle, no stale input.
        assert!(s.telemetry.pitch_deg.abs() < 1.0, "{s:?}");
        assert!((s.telemetry.position_m[1] - SPAWN_POSITION_M[1]).abs() < 5.0);
        assert!((s.command.throttle - 0.75).abs() < 1e-6);
        assert_eq!(s.command.pitch, 0.0);
    }
}

#[test]
fn accept_playtest_fly_pause_resume_and_focus_loss_use_the_session_policy() {
    let mut app = headless_app();
    app.update();
    run(&mut app, 0.5);
    tap(&mut app, KeyCode::Escape);
    app.update();
    assert!(state(&app).paused);
    let frozen = ticks(&app);
    run(&mut app, 1.0);
    assert_eq!(ticks(&app), frozen, "a paused session runs no fixed ticks");
    tap(&mut app, KeyCode::Escape);
    run(&mut app, 0.5);
    assert!(!state(&app).paused);
    assert!(ticks(&app) > frozen);

    // Focus loss with a key held: the held control is cleared, the clock stops.
    press(&mut app, KeyCode::KeyS);
    run(&mut app, 0.2);
    assert!(state(&app).command.pitch > 0.1);
    app.world_mut().write_message(WindowFocused {
        window: Entity::PLACEHOLDER,
        focused: false,
    });
    app.update();
    app.update();
    let s = state(&app);
    assert!(s.paused, "focus loss pauses a single-player session");
    assert_eq!(s.command.pitch, 0.0, "held controls are cleared");
    let frozen = ticks(&app);
    run(&mut app, 0.5);
    assert_eq!(ticks(&app), frozen);
    // Regaining focus resumes the pause that only the focus loss caused.
    app.world_mut().write_message(WindowFocused {
        window: Entity::PLACEHOLDER,
        focused: true,
    });
    run(&mut app, 0.5);
    assert!(!state(&app).paused);
    assert!(ticks(&app) > frozen);
}

#[test]
fn accept_playtest_fly_scripted_smoke_passes_through_the_same_path() {
    let dir = std::env::temp_dir().join(format!("cs_playtest_smoke_{}", std::process::id()));
    let plugin = SmokePlugin::headless(SmokeRequest {
        seconds: 20,
        capture_dir: dir.clone(),
    });
    let handle = plugin.handle();
    let mut app = headless_app_with(|app| {
        app.add_plugins(plugin);
    });
    let mut outcome = None;
    for _ in 0..2000 {
        app.update();
        if let Some(done) = handle.take() {
            outcome = Some(done);
            break;
        }
    }
    let report = outcome
        .expect("the smoke run ends")
        .expect("artifacts written");
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(report.resets, 2);
    assert!(report.obstacle_contacts >= 1);
    assert!(report.input_changes >= 8);
    assert_eq!(report.ticks_while_paused, 0);
    assert!(dir.join("trace.jsonl").is_file() && dir.join("report.json").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn accept_playtest_fly_cli_advertises_and_parses_the_playtest() {
    assert!(cli::HELP_TEXT.contains("--playtest"));
    let args = |list: &[&str]| list.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
    match cli::parse(args(&["--playtest"])) {
        CliRequest::Playtest(request) => assert!(request.smoke.is_none()),
        other => panic!("{other:?}"),
    }
    match cli::parse(args(&[
        "--playtest",
        "--smoke-seconds",
        "20",
        "--capture-dir",
        "x",
    ])) {
        CliRequest::Playtest(request) => {
            let smoke = request.smoke.expect("smoke");
            assert_eq!(smoke.seconds, 20);
            assert_eq!(smoke.capture_dir, std::path::PathBuf::from("x"));
        }
        other => panic!("{other:?}"),
    }
    for bad in [
        &["--playtest", "--smoke-seconds", "5"][..],
        &["--playtest", "--synthetic", "--headless", "--ticks", "3"][..],
        &["--playtest", "--capture-dir", "x"][..],
        &["--smoke-seconds", "20"][..],
    ] {
        assert!(
            matches!(cli::parse(args(bad)), CliRequest::Invalid { .. }),
            "{bad:?}"
        );
    }
}

/// Opens the real window and renders GPU frames through the `cs` binary.
#[test]
#[ignore = "requires a window-capable GPU host"]
fn accept_playtest_fly_windowed_binary_smoke_renders_real_frames() {
    let dir = std::env::temp_dir().join(format!("cs_playtest_window_{}", std::process::id()));
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cs"))
        .args(["--playtest", "--smoke-seconds", "20", "--capture-dir"])
        .arg(&dir)
        .output()
        .expect("the cs binary runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = std::fs::read_to_string(dir.join("report.json")).expect("report.json");
    assert!(report.contains("\"passed\":true") && report.contains("\"shots\":[{"));
    assert!(
        std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| { e.path().extension().is_some_and(|x| x == "png") })
    );
    let _ = std::fs::remove_dir_all(&dir);
}
