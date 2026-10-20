//! PARITY-M01-LAUNCH-CINEMATIC (#1642): the player-facing launch of M01 is
//! the measured sequence end to end — the Flight Check holds the mission's
//! fixed clock until Fly Mission is delivered through the front-end machine,
//! then the launch cinematic holds control, the HUD and the letterbox bars
//! for exactly its measured ticks while the camera plays the measured
//! Pandora-frame script, then the body is handed over at the composed start
//! pose and ordinary play resumes.
//!
//! The synthetic member runs in CI over the synthetic stage with an authored
//! tick count standing in for the measured length (the measurement itself is
//! retail). The retail members read `$CS_GAME_DIR` — never write it — and
//! fail loudly without it. The windowed pixel side (bars, HUD and cockpit
//! hidden while the cinematic plays) is the owner-command smoke's; these
//! members prove the ordering and the hand-over on the same production seams
//! `run_windowed` drives.

use avian3d::prelude::LinearVelocity;
use bevy::math::Vec3;
use bevy::prelude::{Transform, With};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    FlightCheckPhase, LaunchCameraPlayback, LaunchCinematic, LaunchLetterbox, MissionPlayerBody,
    PhaseResponse, build_headless, install_flight_check, install_launch_camera,
    install_launch_cinematic, launch_cinematic_playing, stage_for, teardown,
};
use cs_app::physics::BASELINE_FIXED_HZ;
use cs_app::playtest::fixed_ticks;
use cs_app::ui::front_end::Screen;

use crate::campaign::common::label;
use crate::campaign::m01_lc_fc_c::phase as flight_check;
use crate::campaign::vs_m01_rt_window::{Scratch, game_dir, synthetic_stage};

fn player_body(app: &mut bevy::prelude::App) -> bevy::prelude::Entity {
    let world = app.world_mut();
    world
        .query_filtered::<bevy::prelude::Entity, With<MissionPlayerBody>>()
        .single(world)
        .expect("the composed mission spawns one player body")
}

fn player_transform(app: &bevy::prelude::App, body: bevy::prelude::Entity) -> Transform {
    *app.world()
        .get::<Transform>(body)
        .expect("the player body carries a transform")
}

fn player_velocity(app: &bevy::prelude::App, body: bevy::prelude::Entity) -> Vec3 {
    app.world()
        .get::<LinearVelocity>(body)
        .expect("the player body carries a velocity")
        .0
}

#[test]
fn accept_parity_m01_launch_cinematic_the_check_holds_then_the_cinematic_then_control() {
    let scratch = Scratch::new("launch_cinematic_gate");
    let stage = synthetic_stage(scratch.path());
    // The synthetic stage carries no measured cinematic: the phase below is
    // installed with an authored tick count exactly where `run_windowed`
    // installs the measured one — after the Flight Check.
    assert!(stage.launch_cinematic.is_none());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    app.init_resource::<LaunchLetterbox>();
    install_flight_check(&mut app, flight_check("Alpha - One"));
    install_launch_cinematic(&mut app, 6).expect("a nonzero phase installs");

    // While the Flight Check is up the fixed clock does not run, so the
    // armed cinematic cannot tick: it stays at elapsed zero — it does not
    // consume the check's hold.
    for _ in 0..20 {
        app.update();
    }
    assert_eq!(fixed_ticks(&app), 0, "the check holds the fixed clock");
    {
        let world = app.world();
        let cinematic = world.resource::<LaunchCinematic>();
        assert!(cinematic.is_playing(), "the cinematic is armed");
        assert_eq!(cinematic.elapsed_ticks(), 0, "no cinematic tick has run");
        assert!(world.resource::<LaunchLetterbox>().is_active());
        assert!(world.resource::<FlightCheckPhase>().is_pending());
    }

    // A velocity the flight law is not allowed to change while control is
    // held: the phase puts it aside and must hand it back untouched.
    let body = player_body(&mut app);
    app.world_mut()
        .get_mut::<LinearVelocity>(body)
        .expect("body")
        .0 = Vec3::new(3.0, 0.0, -20.0);

    // Fly Mission, the way Enter delivers it: the machine's Launch +
    // LoadSucceeded leave the check on Screen::Flight, released.
    let response = app
        .world_mut()
        .resource_mut::<FlightCheckPhase>()
        .press_enter();
    assert_eq!(response, PhaseResponse::Launched);
    {
        let phase = app.world().resource::<FlightCheckPhase>();
        assert!(!phase.is_pending());
        assert_eq!(phase.front_end().screen(), Screen::Flight);
    }

    // The release lands through `release_clock` in PreUpdate: the frame the
    // press precedes still runs zero fixed ticks; ticking starts the frame
    // after. Six ticks at 120 Hz is then three ticking updates.
    app.update();
    assert_eq!(
        app.world().resource::<LaunchCinematic>().elapsed_ticks(),
        0,
        "the release frame itself ticks nothing"
    );
    for update in 0..2 {
        app.update();
        assert!(
            launch_cinematic_playing(app.world()),
            "the cinematic still plays on update {update}"
        );
        assert_eq!(
            player_velocity(&app, body),
            Vec3::ZERO,
            "the body is held on update {update}"
        );
        assert!(app.world().resource::<LaunchLetterbox>().is_active());
    }
    app.update();
    assert!(
        !launch_cinematic_playing(app.world()),
        "the sixth tick hands over"
    );
    {
        let cinematic = app.world().resource::<LaunchCinematic>();
        assert_eq!(cinematic.elapsed_ticks(), 6);
        assert_eq!(cinematic.total_ticks(), 6);
    }
    assert!(
        !app.world().resource::<LaunchLetterbox>().is_active(),
        "the bars drop with the phase"
    );
    assert_eq!(
        player_velocity(&app, body),
        Vec3::new(3.0, 0.0, -20.0),
        "control gets the pre-cinematic velocity back"
    );
    assert!(
        fixed_ticks(&app) >= 6,
        "the mission ticked under the cinematic"
    );
    teardown(&mut app);
}

/// The retail stage itself carries the cinematic `run_windowed` installs:
/// measured length, the Pandora's scene root and the composed start pose.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_launch_cinematic_retail_stage_binds_the_measured_launch() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let length = stage
        .launch_cinematic
        .as_ref()
        .expect("M01's stage carries the measured launch cinematic");
    // Devastator is the campaign airframe (#770): the measured 40.15 s intro.
    assert_eq!(length.scene_root, "player_pfighter");
    assert!((length.seconds - 40.15).abs() < 1e-3, "{}", length.seconds);
    assert_eq!(length.ticks(BASELINE_FIXED_HZ), 4818);
    // The composed start the hand-over lands at: the stored pose itself, as
    // the original run measured it (#1746).
    assert_eq!(stage.start.position, [-1426.0, 150.0, -1813.0]);
    assert!((stage.start.heading - 40.0_f32.to_radians()).abs() < 1e-5);

    // The check `run_windowed` opens first: pending on the real screen, with
    // the layout's Fly Mission button drawn.
    let (phase, _artwork) =
        FlightCheckPhase::from_retail(&root, "zbd/c3/m01", &stage).expect("M01's check opens");
    assert!(phase.is_pending());
    assert_eq!(phase.front_end().screen(), Screen::FlightCheck);
    assert!(
        phase
            .scene()
            .buttons
            .iter()
            .any(|button| button.hotspot == "FC_B_FLYMISSION"),
        "the retail screen draws Fly Mission"
    );
}

/// The retail run: the composed M01 holds on the check, then plays all 4818
/// measured ticks, then hands the body over at the composed pose with the
/// camera having driven every measured scene.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_launch_cinematic_retail_runs_the_measured_sequence() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let length = stage.launch_cinematic.clone().expect("the measured length");
    let ticks = length.ticks(BASELINE_FIXED_HZ);
    let (phase, _artwork) =
        FlightCheckPhase::from_retail(&root, "zbd/c3/m01", &stage).expect("M01's check opens");
    let script =
        cs_app::cinematics::launch_camera::read_launch_camera(&root, &length.scene_root, 7)
            .expect("the measured camera script reads");

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    // The windowed build owns the bars' resource through the letterbox
    // plugin; headless observes the same flag directly.
    app.init_resource::<LaunchLetterbox>();
    install_flight_check(&mut app, phase);
    install_launch_cinematic(&mut app, ticks).expect("the measured phase installs");
    install_launch_camera(&mut app, script);

    for _ in 0..20 {
        app.update();
    }
    assert_eq!(fixed_ticks(&app), 0, "the check holds M01's fixed clock");
    {
        let cinematic = app.world().resource::<LaunchCinematic>();
        assert_eq!(
            cinematic.elapsed_ticks(),
            0,
            "the cinematic does not run under the check"
        );
    }

    let response = app
        .world_mut()
        .resource_mut::<FlightCheckPhase>()
        .press_enter();
    assert_eq!(response, PhaseResponse::Launched);

    // The measured run: the press's own frame runs zero ticks (the release
    // lands in PreUpdate), then 4818 fixed ticks, two per update, is exactly
    // 2409 more. Bound the drive and count them.
    let mut updates = 0_u64;
    while launch_cinematic_playing(app.world()) && updates < 2500 {
        app.update();
        updates += 1;
    }
    assert_eq!(updates, 2410, "one release frame + 2409 ticking frames");
    assert!(
        !launch_cinematic_playing(app.world()),
        "the measured phase ends"
    );
    assert_eq!(
        app.world().resource::<LaunchCinematic>().elapsed_ticks(),
        4818
    );
    assert!(!app.world().resource::<LaunchLetterbox>().is_active());
    assert_eq!(
        app.world()
            .resource::<FlightCheckPhase>()
            .front_end()
            .screen(),
        Screen::Flight
    );

    // The camera drove every measured scene off the Pandora's frame.
    let stats = app.world().resource::<LaunchCameraPlayback>().stats();
    assert_eq!(stats.scenes_total, 4, "the Devastator intro's four scenes");
    assert_eq!(stats.scenes_seen, stats.scenes_total, "{stats:?}");
    assert!(
        stats.frames_driven > 2000,
        "the script drove the camera through the cinematic: {stats:?}"
    );
    assert_eq!(
        stats.frames_without_frame_node, 0,
        "the Pandora frame node was never missing: {stats:?}"
    );

    // The hand-over: the body is at the composed start pose, control is back,
    // and the flight law carries it forward from here.
    let body = player_body(&mut app);
    let at = player_transform(&app, body).translation;
    let expect = Vec3::from_array(stage.start.position);
    assert!(
        (at - expect).length() < 2.0,
        "the hand-over pose is the composed start: at {at}, staged {expect}"
    );
    let before = player_transform(&app, body);
    for _ in 0..60 {
        app.update();
    }
    let after = player_transform(&app, body);
    let turned = (after.rotation * before.rotation.inverse()).w.abs() < 1.0 - 1e-6;
    assert!(
        (after.translation - before.translation).length() > 0.5 || turned,
        "the flight law carries the body again once control returns"
    );
    teardown(&mut app);
}
