//! Acceptance stage M01-WINDOW-ACTORS (Rally #1538): the windowed M01
//! composition **draws** its scripted aircraft and its zeppelins, instead of
//! spawning bodies nobody can see.

use std::path::PathBuf;

use bevy::prelude::{Children, With};
use cs_app::mission_actors::{MissionZeppelinActor, ScriptedAircraftActor};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{ActorVisualDrawn, build_headless, stage_for, teardown};

use crate::common::label;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!("CS_GAME_DIR is not set: this suite needs the retail capability")
    }))
}

/// Every spawned aircraft and zeppelin carries a drawn model with textured
/// pieces under it, and the model each aircraft draws follows its record's
/// `kind_of` chain.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_window_actors_retail_every_actor_draws_a_model() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01 composes headlessly");
    app.update();
    app.update();

    let world = app.world_mut();
    let mut aircraft =
        world.query::<(&ScriptedAircraftActor, &ActorVisualDrawn, Option<&Children>)>();
    let rows: Vec<(String, String, String, bool)> = aircraft
        .iter(world)
        .map(|(actor, drawn, children)| {
            (
                actor.record.name.clone(),
                actor.vehicle_record.clone(),
                drawn.root.clone().unwrap_or_default(),
                children.is_some_and(|children| !children.is_empty()),
            )
        })
        .collect();
    assert_eq!(rows.len(), 9, "M01 spawns nine scripted aircraft");
    for (name, prefix, model, has_parts) in &rows {
        let expected = if prefix == "bsfury" {
            "fury"
        } else {
            "piratefighter"
        };
        assert_eq!(model, expected, "{name} draws the wrong model");
        assert!(has_parts, "{name} has no drawn parts under its body");
    }

    let mut zeppelins = world
        .query_filtered::<(&ActorVisualDrawn, Option<&Children>), With<MissionZeppelinActor>>();
    let zeppelins: Vec<(Option<String>, bool)> = zeppelins
        .iter(world)
        .map(|(drawn, children)| {
            (
                drawn.root.clone(),
                children.is_some_and(|children| !children.is_empty()),
            )
        })
        .collect();
    assert_eq!(zeppelins.len(), 3, "M01 binds three zeppelins");
    for (node, has_parts) in &zeppelins {
        assert!(node.is_some(), "a zeppelin has no drawn node");
        assert!(has_parts, "a zeppelin has no drawn parts");
    }
    teardown(&mut app);
}
