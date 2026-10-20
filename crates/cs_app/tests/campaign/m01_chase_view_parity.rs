//! Task #1731 `PARITY2-M01-CHASE-CAMERA`: the player plane seen from behind
//! shows a blur disc instead of opaque blades and no boarding-hook wires
//! (`docs/findings/2026-10-20-parity2-m01-chase-camera.md`).

use std::path::PathBuf;

use bevy::prelude::{ChildOf, Entity, Visibility, With};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MISSION_PLAYER_PIRATE_HOOK_NODE_SLOTS, MISSION_PLAYER_PROP_NODE_SLOT, MissionPlayerBody,
    build_headless, stage_for, teardown,
};
use cs_app::physics::FlightAircraft;
use cs_app::playtest::headless_app;
use cs_app::playtest::propeller::StaticPropellerBlades;
use cs_app::playtest::scene::PlaytestAircraft;
use cs_app::playtest_retail::AircraftPart;
use cs_sim::flight::EngineState;

use crate::campaign::common::label;

fn visibility_of(app: &bevy::prelude::App, entity: Entity) -> Visibility {
    *app.world().get::<Visibility>(entity).expect("a visibility")
}

/// The static blades give way to the blur disc while the engine runs and
/// return when it stops.
#[test]
fn accept_parity2_m01_chase_camera_static_blades_yield_to_the_blur_disc() {
    let mut app = headless_app();
    app.update();
    let body = app
        .world_mut()
        .query_filtered::<Entity, With<PlaytestAircraft>>()
        .single(app.world())
        .expect("one player aircraft");
    let blades = app
        .world_mut()
        .spawn((StaticPropellerBlades, Visibility::Inherited, ChildOf(body)))
        .id();
    app.update();
    assert_eq!(
        visibility_of(&app, blades),
        Visibility::Hidden,
        "a running engine hides the opaque blades"
    );
    let mut query = app
        .world_mut()
        .query_filtered::<&mut FlightAircraft, With<PlaytestAircraft>>();
    query
        .single_mut(app.world_mut())
        .expect("the production flight record")
        .set_engine(EngineState::STOPPED)
        .expect("an engine can be stopped");
    app.update();
    assert_eq!(
        visibility_of(&app, blades),
        Visibility::Inherited,
        "a stopped engine shows them"
    );
}

/// M01's player spawns its static blades under the rule and its five
/// `pirate_hook` meshes hidden.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_parity2_m01_chase_camera_retail_player_hides_hook_wires_and_static_blades() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let plan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    for _ in 0..3 {
        app.update();
    }
    let world = app.world_mut();
    let body = world
        .query_filtered::<Entity, With<MissionPlayerBody>>()
        .single(world)
        .expect("one player body");
    let parts: Vec<(Entity, u32)> = world
        .query::<(Entity, &AircraftPart, &ChildOf)>()
        .iter(world)
        .filter(|(_, _, child)| child.parent() == body)
        .map(|(entity, part, _)| (entity, part.node_slot))
        .collect();
    let mut hooks = 0;
    for (entity, slot) in &parts {
        if MISSION_PLAYER_PIRATE_HOOK_NODE_SLOTS.contains(slot) {
            hooks += 1;
            assert_eq!(
                *world.get::<Visibility>(*entity).unwrap(),
                Visibility::Hidden
            );
        }
        if *slot == MISSION_PLAYER_PROP_NODE_SLOT {
            assert!(world.get::<StaticPropellerBlades>(*entity).is_some());
            assert_eq!(
                *world.get::<Visibility>(*entity).unwrap(),
                Visibility::Hidden
            );
        }
    }
    assert_eq!(hooks, MISSION_PLAYER_PIRATE_HOOK_NODE_SLOTS.len());
    teardown(&mut app);
}
