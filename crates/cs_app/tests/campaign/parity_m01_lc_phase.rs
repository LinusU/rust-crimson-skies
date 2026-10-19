//! PARITY-M01-LC-PHASE (#1663): the launch cinematic is a session phase in
//! integer fixed ticks that holds control and the HUD and then hands over.
//!
//! The synthetic members run in CI. The retail member reads `$CS_GAME_DIR`
//! (never writes it) and fails loudly without it.

use std::path::PathBuf;

use avian3d::prelude::LinearVelocity;
use bevy::app::{App, FixedFirst, FixedLast, Update};
use bevy::math::Vec3;
use cs_app::mission_session::{
    LaunchCinematic, LaunchLetterbox, MissionPlayerBody, install_launch_cinematic,
    launch_cinematic_playing, read_launch_cinematic, ticks_for,
};

#[test]
fn accept_parity_m01_lc_phase_ticks_round_up_and_zero_is_refused() {
    // 40.15 s at 120 Hz is exactly 4818 ticks; a length that is not a whole
    // number of ticks never ends early.
    assert_eq!(ticks_for(40.15, 120), 4818);
    assert_eq!(ticks_for(30.25, 120), 3630);
    assert_eq!(ticks_for(0.01, 120), 2);
    assert_eq!(ticks_for(0.0, 120), 0);
    assert!(LaunchCinematic::new(0).is_err());

    let mut phase = LaunchCinematic::new(3).expect("a phase");
    assert!(phase.is_playing());
    assert!(!phase.complete_tick());
    assert!(!phase.complete_tick());
    assert!(phase.complete_tick(), "the third tick ends it");
    assert!(!phase.is_playing());
    assert!(!phase.complete_tick(), "an ended phase does not end twice");
    assert_eq!(phase.elapsed_ticks(), 3);
}

#[test]
fn accept_parity_m01_lc_phase_holds_the_body_and_bars_then_hands_over() {
    let mut app = App::new();
    app.init_resource::<LaunchLetterbox>();
    install_launch_cinematic(&mut app, 5).expect("installs");
    let body = app
        .world_mut()
        .spawn((
            MissionPlayerBody,
            LinearVelocity(Vec3::new(3.0, 0.0, -20.0)),
        ))
        .id();
    let velocity = |app: &App| app.world().get::<LinearVelocity>(body).expect("body").0;

    for tick in 0..5 {
        app.update();
        assert!(
            app.world().resource::<LaunchLetterbox>().is_active(),
            "{tick}"
        );
        assert!(launch_cinematic_playing(app.world()));
        app.world_mut().run_schedule(FixedFirst);
        assert_eq!(velocity(&app), Vec3::ZERO, "held on tick {tick}");
        app.world_mut().run_schedule(FixedLast);
    }
    // The fifth tick ended the phase: control is back and the velocity the
    // body carried before the hold is restored.
    assert!(!launch_cinematic_playing(app.world()));
    assert_eq!(velocity(&app), Vec3::new(3.0, 0.0, -20.0));
    app.world_mut().run_schedule(FixedFirst);
    assert_eq!(velocity(&app), Vec3::new(3.0, 0.0, -20.0), "no longer held");
    app.world_mut().run_schedule(Update);
    assert!(!app.world().resource::<LaunchLetterbox>().is_active());
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_lc_phase_retail_length_follows_the_airframe() {
    let root = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("a retail test needs CS_GAME_DIR to be set"),
    );
    let long = read_launch_cinematic(&root, "player_pfighter").expect("measures");
    assert!((long.seconds - 40.15).abs() < 1e-3, "{}", long.seconds);
    assert_eq!(long.ticks(120), 4818);
    let short = read_launch_cinematic(&root, "player_balmoral").expect("measures");
    assert!((short.seconds - 30.25).abs() < 1e-3, "{}", short.seconds);
}
