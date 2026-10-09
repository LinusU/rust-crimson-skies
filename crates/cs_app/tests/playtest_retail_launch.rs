//! Task #649 (PLAYTEST-RETAIL-HANDOFF) acceptance tests. Prefix:
//! `accept_playtest_retail_launch_`.
//!
//! The retail half drives [`cs_app::playtest::headless_app_with`] with the
//! original area installed by [`cs_app::playtest::retail::install`]: the same
//! plugin, input session, F24 flight driver, Avian world and chase camera as the
//! window, minus the window and the GPU. It is `#[ignore]`d (`requires
//! CS_GAME_DIR`) and **fails** without the variable. The windowed half runs the
//! `cs` binary on a GPU host.
//!
//! Task #797 (FLIGHT-ORIGINAL-PLAYTEST) folds its `accept_flight_original_playtest_`
//! tests into this binary for the same reason #666, #709, #710, #753, #795 and
//! #794 did: CI's runner disk is nearly full, and that task's retail half reads
//! the installation this binary already reads.

use std::path::PathBuf;

use avian3d::prelude::{Collider, LinearVelocity, Mass, Position, Rotation};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput, NativeKey};
use bevy::math::{Quat, Vec3};
use bevy::prelude::{App, Entity, With};
use cs_app::cli::{self, CliRequest};
use cs_app::physics::FlightAircraft;
use cs_app::playtest::retail::{
    self, PlaytestAreaBody, RETAIL_START_SPEED_M_S, RetailContent, RetailFlight, RetailRequest,
};
use cs_app::playtest::scene::{PlaytestAircraft, PlaytestOriginalFlight};
use cs_app::playtest::smoke::{SMOKE_FRAME_HZ, SmokePlugin};
use cs_app::playtest::{
    PlaytestCameraMarker, PlaytestError, PlaytestRequest, PlaytestState, SmokeRequest,
    headless_app_with, run_playtest,
};
use cs_app::playtest_retail::PLAYTEST_LABEL;
use cs_content::original_airframe::import_retail_airframe;
use cs_sim::flight::OriginalAirframe;

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
    // The Startup spawn is the designed pose; read it on the first frame,
    // before the aircraft's own start speed carries it away. Over original
    // content the body starts at the scene's declared start speed and cruises
    // at the imported `fd_speed` (#797), so half a second of flight is already
    // tens of metres down the spawn heading — the pose assertion is about the
    // spawn, not about where the aircraft has got to.
    app.update();
    let spawn = app.world().resource::<RetailContent>().spawn_m;
    let mut aircraft = app
        .world_mut()
        .query_filtered::<(&Position, &Collider), With<PlaytestAircraft>>();
    let (position, _) = aircraft.single(app.world()).expect("one player aircraft");
    assert!(
        (position.0 - bevy::math::Vec3::from(spawn)).length() < 40.0,
        "{position:?} vs {spawn:?}"
    );
    for _ in 0..29 {
        app.update();
    }
    let content = app.world().resource::<RetailContent>();
    let records = content.area.mesh_records;
    let entities = content.area_entities.clone();
    assert!(records > 0 && !entities.is_empty());
    // Content → collider: every spawned record carries a mesh-derived collider.
    assert_eq!(retail::area_colliders(app.world_mut()), records);
    // Content → pose: the player aircraft is the one flight body at the designed
    // spawn, drawn from the original mesh (the visual lives in the window build).
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

// -------------------------------------------- the original flight law (#797) --

/// How long a scripted run of `seconds` takes, in rendered frames: the same
/// frame clock the smoke uses, so one `update` is one 1/60 s frame.
fn run(app: &mut App, seconds: f64) {
    for _ in 0..(seconds * SMOKE_FRAME_HZ).round() as u32 {
        app.update();
    }
}

/// Holds a key down without a release: the session keeps the deflection.
fn hold(app: &mut App, key: KeyCode) {
    app.world_mut().write_message(KeyboardInput {
        key_code: key,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
    app.update();
}

/// The designed retail spawn position, metres: what the scene's own pose is
/// before any of this test's initial conditions move it.
fn spawn_position(app: &mut App) -> Vec3 {
    let spawn = app.world().resource::<RetailContent>().spawn_m;
    Vec3::from(spawn)
}

/// The one player aircraft as the retail flight path spawned it.
fn original_flight(app: &mut App) -> (Entity, f32) {
    let mut query = app
        .world_mut()
        .query_filtered::<(Entity, &PlaytestOriginalFlight, &Position), With<PlaytestAircraft>>();
    let (entity, _, position) = query
        .single(app.world())
        .expect("the retail scene spawns exactly one aircraft on the original law");
    (entity, position.0.y)
}

/// Sets the body's own pose and velocity: the law seeds itself from them each
/// fixed tick, so this is the run's declared initial condition rather than an
/// instruction to the simulation.
fn set_initial_condition(
    app: &mut App,
    position: Vec3,
    rotation: Quat,
    velocity: Vec3,
    hold_attitude: bool,
) {
    let entity = original_flight(app).0;
    {
        let world = app.world_mut();
        let mut record = world.entity_mut(entity);
        let mut flight = record
            .get_mut::<PlaytestOriginalFlight>()
            .expect("the original law record is on the body");
        flight.hold_attitude = hold_attitude;
    }
    let world = app.world_mut();
    let mut body = world.entity_mut(entity);
    body.insert((
        Position(position),
        Rotation(rotation),
        LinearVelocity(velocity),
    ));
    // Nothing may be easing the pose the test is about to measure.
    if let Some(mut transform) = body.get_mut::<bevy::prelude::Transform>() {
        transform.translation = position;
        transform.rotation = rotation;
    }
}

/// **The playtest label and the playtest docs say what flies the aircraft: the
/// statically recovered original law, `OWNER-STATIC-2026-10-08`, still
/// uncalibrated against an original run (#358).**
///
/// This is the criterion that does not need an installation, so it runs in CI:
/// the label is what a screenshot, a log line and the window title show, and the
/// two playtest documents are what a reader is pointed at.
#[test]
fn accept_flight_original_playtest_label_and_docs_name_the_statically_recovered_law() {
    for required in [
        "ORIGINAL ASSETS",
        "PROVISIONAL TUNING",
        "ORIGINAL FLIGHT LAW",
        "OWNER-STATIC-2026-10-08",
        "UNCALIBRATED",
        "#358",
    ] {
        assert!(
            PLAYTEST_LABEL.contains(required),
            "the label must state {required:?}: {PLAYTEST_LABEL}"
        );
    }
    for forbidden in ["M01", "faithful", "campaign", "verified_original"] {
        assert!(
            !PLAYTEST_LABEL.contains(forbidden),
            "the label must never claim it: {forbidden}"
        );
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for doc in ["docs/PLAYTEST.md", "docs/PLAYTEST-RETAIL.md"] {
        let text = std::fs::read_to_string(root.join(doc))
            .unwrap_or_else(|error| panic!("cannot read {doc}: {error}"));
        for required in ["OWNER-STATIC-2026-10-08", "358"] {
            assert!(
                text.contains(required),
                "{doc} must state the flight law's provenance ({required})"
            );
        }
        assert!(
            text.to_lowercase().contains("uncalibrated"),
            "{doc} must say the flight law is uncalibrated against an original run"
        );
        assert!(
            text.contains("pbloodhawk"),
            "{doc} must name the imported record the aircraft flies"
        );
    }
}

/// **Over retail content the playtest's flight body is built from the imported
/// original parameters, not from the synthetic fixture.**
///
/// Everything the assertion reads is production code twice over: the body's
/// record is what `scene::spawn_aircraft` built, and the values it is compared
/// against come from a second run of `cs_content::original_airframe` over the
/// installation in this same process. If `scene.rs` went back to
/// `synthetic_fixed_wing()`, there would be no [`PlaytestOriginalFlight`] on the
/// body at all (the F24 [`FlightAircraft`] would be there instead) and this test
/// would fail at its first query.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_flight_original_playtest_retail_body_flies_the_imported_parameters() {
    let mut app = retail_app(|_| {});
    app.update();
    let mut query = app.world_mut().query_filtered::<(
        &PlaytestOriginalFlight,
        &Position,
        &Rotation,
        &LinearVelocity,
        &Mass,
        Option<&FlightAircraft>,
    ), With<PlaytestAircraft>>();
    let (flight, position, rotation, velocity, mass, f24) = query
        .single(app.world())
        .expect("the retail scene spawns exactly one aircraft on the original law");
    assert!(
        f24.is_none(),
        "the retail body must not carry the designed F24 record: the original law flies it"
    );

    // The parameters are re-read here through the production importer, so the
    // comparison is against the installation and never against a copy.
    let parameters = import_retail_airframe(&install_dir(), "pbloodhawk")
        .expect("the player Bloodhawk record imports");
    let expected = OriginalAirframe::from_values(&parameters.field_values())
        .expect("the law accepts the imported record");
    assert_eq!(
        flight.model.airframe, expected,
        "the body flies exactly what the importer just read from vehicle.zrd"
    );
    let airframe = &flight.model.airframe;
    // The table the task states, in the units the record stores. The values are
    // f32-derived doubles, so they are compared within one part in a million
    // rather than bit-equal.
    let close = |name: &str, actual: f64, expected: f64| {
        assert!(
            (actual - expected).abs() <= expected.abs() * 1.0e-6 + 1.0e-9,
            "{name} = {actual}, expected {expected}"
        );
    };
    close("engine_factor", airframe.engine_factor, 0.62);
    close("pitch_torque", airframe.pitch_torque, 3.3);
    close("roll_torque", airframe.roll_torque, 7.5);
    close("rudder_torque", airframe.rudder_torque, 2.0);
    close("return_rate", airframe.return_rate, 3.0);
    close("ang_momentum_damp", airframe.ang_momentum_damp, 5.0);
    close(
        "rec_moments_inertia_x",
        airframe.rec_moments_inertia[0],
        1.18,
    );
    close(
        "rec_moments_inertia_y",
        airframe.rec_moments_inertia[1],
        1.0,
    );
    close(
        "rec_moments_inertia_z",
        airframe.rec_moments_inertia[2],
        1.1,
    );
    close("fd_speed", airframe.fd_speed, 135.0);
    close("drag_factor", airframe.drag_factor, 0.37);
    close("veh_weight", airframe.veh_weight, 1900.0);
    close("ref_area", airframe.ref_area, 330.0);
    close("gravity", airframe.gravity, 20.0);
    close("nom_gravity", flight.model.globals.nom_gravity, 20.0);
    assert_eq!(
        flight.provenance, "OWNER-STATIC-2026-10-08",
        "static evidence, never verified_original"
    );

    // The start speed is the scene's own declared start (the original's spawn
    // speed is unknown), while the *cruise* these parameters decide is the
    // imported `fd_speed` asserted above and measured by the level test. The
    // start is level and heading the spawn heading (-Z).
    let speed = velocity.0.length();
    assert!(
        (speed - RETAIL_START_SPEED_M_S as f32).abs() < 0.5,
        "the retail start speed is the declared {RETAIL_START_SPEED_M_S} m/s, got {speed}"
    );
    assert!(
        velocity.0.x.abs() < 1.0e-3 && velocity.0.y.abs() < 1.0e-3,
        "level spawn: {:?}",
        velocity.0
    );
    assert!(
        velocity.0.z < -(RETAIL_START_SPEED_M_S as f32) + 0.5,
        "the spawn heading is -Z, got {:?}",
        velocity.0
    );
    assert!(
        (rotation.0 * Vec3::NEG_Z).dot(Vec3::new(0.0, 0.0, -1.0)) > 0.999,
        "the spawn attitude is level"
    );
    // The body's one mass is the imported weight over the law's own force
    // scale, so the submitted force accelerates it by the law's acceleration.
    let expected_mass = 1900.0_f64 / 9.82;
    assert!(
        (f64::from(mass.0) - expected_mass).abs() < 1.0e-3,
        "the declared mass is W / 9.82 = {expected_mass}, got {}",
        mass.0
    );
    {
        let retail_flight = app.world().resource::<RetailFlight>();
        println!(
            "PLAYTEST-ORIGINAL-BODY record={} chain={} engine_id={} fd_speed_m_s={} \
             start_speed_m_s={} mass_kg={} fuel={:.3} provenance={} law_model_kind=original_fixed_wing",
            retail_flight.record,
            retail_flight.inheritance_chain.join("->"),
            retail_flight.engine.0,
            retail_flight.fd_speed_m_s,
            retail_flight.start_speed_m_s(),
            mass.0,
            flight.state.fuel,
            flight.provenance
        );
    }
    // The spawn is below the 2000 m density ceiling the law switches at.
    assert!(
        position.0.y < 2000.0,
        "the retail spawn must start in the low layer, got {} m",
        position.0.y
    );
    // The spawned state is the documented one: level, cruise throttle, the
    // imported fuel load (the spawn's own two fixed ticks of cruise have
    // already burned a fraction of it), Level-Off off (no input slot for the
    // original's command 47).
    let imported_fuel = parameters
        .initial_fuel
        .clone()
        .known()
        .expect("player_airplane states fuel");
    assert!(
        (flight.state.fuel - imported_fuel).abs() < 1.0,
        "the body starts from the imported fuel load {imported_fuel}, got {}",
        flight.state.fuel
    );
    assert!((flight.state.throttle - 0.75).abs() < 1.0e-6, "cruise");
    assert!(!flight.state.level_off, "command 47 has no input slot");
    assert_eq!(
        app.world().resource::<PlaytestState>().original_step_errors,
        0
    );

    // And the same body after a reset: one aircraft, one law, same record.
    tap_r(&mut app);
    run(&mut app, 0.5);
    assert_eq!(count::<PlaytestAircraft>(&mut app), 1);
    assert_eq!(count::<PlaytestOriginalFlight>(&mut app), 1);
    assert_eq!(count::<FlightAircraft>(&mut app), 0, "never the F24 record");
    assert_eq!(
        app.world().resource::<PlaytestState>().original_step_errors,
        0
    );
}

/// **Level flight at full throttle reaches the cruise the imported parameters
/// say it does: 134 m/s +/-2 %.**
///
/// The run starts **below** that speed (60 m/s, level) on purpose: starting at
/// the answer would let a body that never felt the law pass, and this way the
/// assertion only holds if the law's thrust, drag and lift really run for the
/// 11 simulated seconds. The playtest's own start speed is asserted separately,
/// in the test above.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_flight_original_playtest_level_full_throttle_reaches_the_cruise_speed() {
    let mut app = retail_app(|_| {});
    app.update();
    let spawn = spawn_position(&mut app);
    set_initial_condition(
        &mut app,
        Vec3::new(spawn.x, 100.0, spawn.z),
        Quat::IDENTITY,
        Vec3::new(0.0, 0.0, -60.0),
        false,
    );
    hold(&mut app, KeyCode::Digit4);
    run(&mut app, 11.0);

    let speed = app
        .world_mut()
        .query_filtered::<&LinearVelocity, With<PlaytestAircraft>>()
        .single(app.world())
        .expect("one aircraft")
        .0
        .length();
    let state = app.world().resource::<PlaytestState>().clone();
    assert_eq!(
        state.original_step_errors, 0,
        "the law stepped every tick without a refusal"
    );
    println!(
        "PLAYTEST-ORIGINAL-LEVEL start_speed_m_s=60 full_throttle_seconds=11 \
         settled_speed_m_s={speed:.4} step_errors={}",
        state.original_step_errors
    );
    assert!(
        (speed - 134.0).abs() <= 134.0 * 0.02,
        "full-throttle level flight settles at {speed} m/s, expected 134 +/-2 %"
    );
}

/// **With the nose held vertical at full throttle from 500 m, the aircraft
/// climbs for 5 seconds.**
///
/// "Held vertical" is the law's own held-attitude mode (`OriginalInput::
/// hold_attitude`, the original dev tool `0x491c60`), which is what #796's
/// vertical probe flies: the attitude stays exactly nose-up while the forces
/// integrate, so what is measured is the recovered law's own vertical climb
/// rather than a stick input's approximation of it. The start is the retail
/// spawn's own x/z, lifted to 500 m — above the drawn area, below the 2000 m
/// density ceiling.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_flight_original_playtest_vertical_nose_from_500_m_gains_altitude() {
    let mut app = retail_app(|_| {});
    app.update();
    let spawn = spawn_position(&mut app);
    let (spawn_x, spawn_z) = (spawn.x, spawn.z);
    // Nose up: the body's forward (-Z) rotated onto world +Y.
    set_initial_condition(
        &mut app,
        Vec3::new(spawn_x, 500.0, spawn_z),
        Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        Vec3::ZERO,
        true,
    );
    hold(&mut app, KeyCode::Digit4);
    run(&mut app, 5.0);

    let (entity, altitude) = original_flight(&mut app);
    let rotation = app
        .world_mut()
        .get::<Rotation>(entity)
        .expect("the body keeps its pose");
    let nose = rotation.0 * Vec3::NEG_Z;
    assert!(
        nose.y > 0.99,
        "the held attitude keeps the nose vertical, got {nose:?}"
    );
    let state = app.world().resource::<PlaytestState>().clone();
    assert_eq!(state.original_step_errors, 0);
    println!(
        "PLAYTEST-ORIGINAL-VERTICAL start_altitude_m=500 seconds=5 gained_m={:.3} \
         nose_y={:.4} step_errors={}",
        altitude - 500.0,
        nose.y,
        state.original_step_errors
    );
    assert!(
        altitude > 500.0 + 50.0,
        "a full-throttle vertical climb from 500 m gained only {} m in 5 s",
        altitude - 500.0
    );
}
