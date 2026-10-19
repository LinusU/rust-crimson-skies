//! PARITY-M01-ATMO-E (#1652): the composed M01 start is the body callback 10
//! leaves (finding `docs/findings/2026-10-19-m01-launch-body-pose.md`).
//!
//! The synthetic member runs in CI. The retail member reads `$CS_GAME_DIR`
//! (never writes it) through the production stage and fails loudly without it.

use std::path::PathBuf;

use bevy::prelude::With;
use cs_app::cinematics::launch_body::{
    INTRO_RAISE, composed_start_pose, place_player_at_intro_start,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionPlayerBody, MissionPlayerStart, build_headless, stage_for, teardown,
};
use cs_app::mission_start::StartPose;

use crate::campaign::common::label;

#[test]
fn accept_parity_m01_atmo_e_synthetic_start_is_the_raised_stored_pose() {
    let stored = StartPose {
        position: [-1426.0, 150.0, -1813.0],
        heading: 40.0_f32.to_radians(),
    };
    let composed = composed_start_pose(stored);
    assert_eq!(
        composed.position,
        [-1426.0, 150.0 + INTRO_RAISE, -1813.0],
        "callback 10 adds 100 m to the stored y and nothing to x or z"
    );
    assert_eq!(composed.heading, stored.heading, "the heading is unchanged");

    // The same arithmetic the engine replay (#1685) performs.
    let placed = place_player_at_intro_start(stored.position, 40.0);
    assert_eq!(composed.position, placed.position);
    assert!((composed.heading - placed.euler[1]).abs() < 1e-6);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_atmo_e_retail_composed_player_spawns_at_the_placed_body() {
    let root = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("a retail test needs CS_GAME_DIR to be set"),
    );
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    // The stored record is (-1426, 150, -1813), heading 40.
    assert_eq!(stage.start.position, [-1426.0, 250.0, -1813.0]);
    let placed = place_player_at_intro_start([-1426.0, 150.0, -1813.0], 40.0);
    assert_eq!(stage.start.position, placed.position);
    assert!((stage.start.heading - placed.euler[1]).abs() < 1e-5);

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();
    let recipe = app.world().resource::<MissionPlayerStart>().clone();
    assert_eq!(recipe.position_m, placed.position);
    let world = app.world_mut();
    let bodies = world
        .query_filtered::<&bevy::prelude::Transform, With<MissionPlayerBody>>()
        .iter(world)
        .count();
    assert_eq!(bodies, 1, "one player body is spawned");
    let at = world
        .query_filtered::<&bevy::prelude::Transform, With<MissionPlayerBody>>()
        .single(world)
        .expect("the player body")
        .translation;
    assert!(
        (at.y - 250.0).abs() < 1.0,
        "the body starts at the raised y"
    );
    teardown(&mut app);
}
