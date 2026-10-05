//! Task #649 (PLAYTEST-RETAIL-HANDOFF) acceptance tests. Prefix:
//! `accept_playtest_retail_launch_`.
//!
//! The retail half drives [`cs_app::playtest::headless_app_with`] with the
//! original area installed by [`cs_app::playtest::retail::install`]: the same
//! plugin, input session, F24 flight driver, Avian world and chase camera as the
//! window, minus the window and the GPU. It is `#[ignore]`d (`requires
//! CS_GAME_DIR`) and **fails** without the variable. The windowed half runs the
//! `cs` binary on a GPU host.

use std::path::PathBuf;

use avian3d::prelude::{Collider, Position};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput, NativeKey};
use bevy::prelude::{App, Entity, With};
use cs_app::cli::{self, CliRequest};
use cs_app::playtest::retail::{self, PlaytestAreaBody, RetailContent, RetailRequest};
use cs_app::playtest::scene::PlaytestAircraft;
use cs_app::playtest::smoke::{SMOKE_FRAME_HZ, SmokePlugin};
use cs_app::playtest::{
    PlaytestCameraMarker, PlaytestError, PlaytestRequest, PlaytestState, SmokeRequest,
    headless_app_with, run_playtest,
};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|a| (*a).to_owned()).collect()
}

fn install_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the original installation"),
    )
}

fn request() -> RetailRequest {
    RetailRequest::new(install_dir(), None, None).expect("the documented defaults are valid")
}

fn retail_app(extra: impl FnOnce(&mut App)) -> App {
    let request = request();
    let sources = retail::read_sources(&request).expect("the installation reads");
    headless_app_with(|app| {
        retail::install(app, &sources, &request).expect("the original area installs");
        extra(app);
    })
}

fn count<T: bevy::prelude::Component>(app: &mut App) -> usize {
    app.world_mut().query::<&T>().iter(app.world()).count()
}

fn tap_r(app: &mut App) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyR,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }
}

#[test]
fn accept_playtest_retail_launch_cli_routes_the_exact_command_to_original_assets() {
    let routed = cli::parse(args(&[
        "--playtest",
        "--cs-path",
        "/some/install",
        "--world",
        "c1c",
    ]));
    match routed {
        CliRequest::Playtest(PlaytestRequest {
            smoke: None,
            retail: Some(retail),
        }) => {
            assert_eq!(retail.cs_path, PathBuf::from("/some/install"));
            assert_eq!(retail.world, "c1c");
            assert_eq!(retail.aircraft, "bloodhawk", "documented default aircraft");
        }
        other => panic!("{other:?}"),
    }
    // Plain --playtest stays synthetic.
    match cli::parse(args(&["--playtest"])) {
        CliRequest::Playtest(request) => assert!(request.retail.is_none()),
        other => panic!("{other:?}"),
    }
    for bad in [
        &["--playtest", "--world", "c1c"][..],
        &["--playtest", "--aircraft", "bloodhawk"][..],
        &["--playtest", "--cs-path", "/x", "--world", "nowhere"][..],
        &["--playtest", "--cs-path", "/x", "--aircraft", "nothing"][..],
        &["--cs-path", "/x"][..],
    ] {
        assert!(
            matches!(cli::parse(args(bad)), CliRequest::Invalid { .. }),
            "{bad:?}"
        );
    }
    assert!(cli::HELP_TEXT.contains("--cs-path"));
}

#[test]
fn accept_playtest_retail_launch_an_explicit_bad_path_fails_and_never_falls_back() {
    let missing = std::env::temp_dir().join("cs_playtest_retail_no_such_installation");
    let retail = RetailRequest::new(missing.clone(), None, None).expect("valid selectors");
    let request = PlaytestRequest {
        smoke: None,
        retail: Some(retail),
    };
    let error = run_playtest(&request).expect_err("a missing installation must fail");
    assert!(matches!(error, PlaytestError::Retail { .. }), "{error}");
    let text = error.to_string();
    assert!(text.contains(&missing.display().to_string()), "{text}");
    assert!(text.contains("no fallback"), "{text}");

    // The binary: nonzero, names the path, opens no window (it returns at once).
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cs"))
        .args(["--playtest", "--cs-path"])
        .arg(&missing)
        .output()
        .expect("the cs binary runs");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no fallback"), "{stderr}");
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_retail_launch_content_pose_and_collider_are_connected() {
    let mut app = retail_app(|_| {});
    for _ in 0..30 {
        app.update();
    }
    let content = app.world().resource::<RetailContent>();
    let records = content.area.mesh_records;
    let spawn = content.spawn_m;
    let entities = content.area_entities.clone();
    assert!(records > 0 && !entities.is_empty());
    // Content → collider: every spawned record carries a mesh-derived collider.
    assert_eq!(retail::area_colliders(app.world_mut()), records);
    // Content → pose: the player aircraft is the one flight body at the designed
    // spawn, drawn from the original mesh (the visual lives in the window build).
    let mut aircraft = app
        .world_mut()
        .query_filtered::<(&Position, &Collider), With<PlaytestAircraft>>();
    let (position, _) = aircraft.single(app.world()).expect("one player aircraft");
    assert!(
        (position.0 - bevy::math::Vec3::from(spawn)).length() < 40.0,
        "{position:?} vs {spawn:?}"
    );
    assert_eq!(
        app.world().resource::<PlaytestState>().spawn_m,
        spawn,
        "smoke and HUD read the same spawn"
    );
    let content = app.world().resource::<RetailContent>();
    let extent = content.aircraft.extent_m;
    for axis in 0..3 {
        assert!(
            (f64::from(content.half_extents_m[axis]) - extent[axis] / 2.0).abs() < 1e-4
                || content.half_extents_m[axis] == 0.25,
            "the collider box is half the composed extent of the drawn set: {:?} vs {extent:?}",
            content.half_extents_m
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_retail_launch_reset_owns_only_the_aircraft() {
    let mut app = retail_app(|_| {});
    for _ in 0..30 {
        app.update();
    }
    let before = app
        .world()
        .resource::<RetailContent>()
        .area_entities
        .clone();
    // A reset respawns only the aircraft, from handles the content already holds:
    // it must create no image, material or mesh asset (no leak across resets).
    let assets = |app: &App| {
        (
            app.world()
                .resource::<bevy::prelude::Assets<bevy::image::Image>>()
                .len(),
            app.world()
                .resource::<bevy::prelude::Assets<bevy::prelude::StandardMaterial>>()
                .len(),
            app.world()
                .resource::<bevy::prelude::Assets<bevy::mesh::Mesh>>()
                .len(),
        )
    };
    let assets_before = assets(&app);
    assert!(
        assets_before.0 > 0 && assets_before.1 > 0,
        "the original textures are live: {assets_before:?}"
    );
    for reset in 1..=3 {
        tap_r(&mut app);
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(count::<PlaytestAircraft>(&mut app), 1);
        assert_eq!(count::<PlaytestCameraMarker>(&mut app), 1);
        assert_eq!(app.world().resource::<PlaytestState>().resets, reset);
        assert_eq!(assets(&app), assets_before, "reset {reset} leaked an asset");
    }
    assert!(
        before.iter().all(|e| app.world().get_entity(*e).is_ok()),
        "a reset never respawns or removes the area"
    );
    assert_eq!(count::<PlaytestAreaBody>(&mut app), {
        let mut distinct = before.clone();
        distinct.sort();
        distinct.dedup();
        distinct.len()
    });
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_retail_launch_scripted_smoke_collides_with_the_original_area() {
    let dir = std::env::temp_dir().join(format!("cs_playtest_retail_{}", std::process::id()));
    let plugin = SmokePlugin::headless(SmokeRequest {
        seconds: 60,
        capture_dir: dir.clone(),
    });
    let handle = plugin.handle();
    let mut app = retail_app(|app| {
        app.add_plugins(plugin);
    });
    let mut outcome = None;
    for _ in 0..(60.0 * SMOKE_FRAME_HZ) as u32 + 600 {
        app.update();
        if let Some(done) = handle.take() {
            outcome = Some(done);
            break;
        }
    }
    let report = outcome.expect("the run ends").expect("artifacts written");
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(report.resets, 6, "three passes of two resets");
    assert!(
        report.obstacle_contacts >= 1,
        "contact with the original area"
    );
    assert!(report.label.contains("ORIGINAL ASSETS"));
    // Source manifest: the report names the installation and both containers.
    let manifest = report.retail.expect("retail runs record their sources");
    let sources = retail::read_sources(&request()).expect("reads");
    assert!(manifest.contains(sources.installation()), "{manifest}");
    assert!(manifest.contains(sources.world().container_sha256()));
    assert!(manifest.contains(sources.aircraft().container_sha256()));
    assert!(
        manifest.contains("\"aircraft_mesh_bindings\":")
            && manifest.contains("\"aircraft_undrawn\":["),
        "the smoke report lists the aircraft's drawn bindings and its undrawn ones"
    );
    // The report records the chosen texture archive and the textured counts.
    assert!(
        manifest.contains(sources.textures().path()) && manifest.contains("\"textured_materials\""),
        "{manifest}"
    );
    let report_json = std::fs::read_to_string(dir.join("report.json")).expect("report.json");
    assert!(
        report_json.contains("\"passed\":true") && report_json.contains(sources.installation())
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The exact owner command's scripted twin: opens the real window over the
/// original assets, flies >= 2 minutes with >= 3 reset/pause cycles, renders
/// framebuffers and writes the report. Artifacts: `CS_PLAYTEST_RETAIL_OUT` or a
/// temporary directory.
#[test]
#[ignore = "requires CS_GAME_DIR and a window-capable GPU host"]
fn accept_playtest_retail_launch_windowed_binary_smoke_renders_original_frames() {
    let dir = std::env::var("CS_PLAYTEST_RETAIL_OUT").map_or_else(
        |_| std::env::temp_dir().join(format!("cs_playtest_retail_win_{}", std::process::id())),
        PathBuf::from,
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cs"))
        .args(["--playtest", "--cs-path"])
        .arg(install_dir())
        .args(["--world", "c1c", "--smoke-seconds", "120", "--capture-dir"])
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
    assert!(report.contains("ORIGINAL ASSETS"));
}
