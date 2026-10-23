//! M01 rocket smoke trail acceptance (#1827): the BOOM rocket's flyout
//! animation `he_rocket` (`wep_06` `FLYOUT MODEL_ANIMATION`, member
//! `missile_puffers.zrd`) states the trail emitter `he_rocket_trail`, and the
//! composed mission's launched rocket leaves its puffs.
//!
//! Task test prefix: `accept_m01_rocket_trail_`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use bevy::asset::Assets;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::prelude::{Image, Mesh, Rectangle};

use cs_app::mission_session::{
    RocketTrailArt, RocketTrailDefinition, RocketTrailPuff, RocketTrailReport, TrailEmitter,
    read_rocket_trail_definition, teardown,
};

use crate::campaign::m01_play_weapons::armed_stage;

const MAX_FRAMES: usize = 240;

fn synthetic_definition() -> RocketTrailDefinition {
    RocketTrailDefinition {
        emitter: TrailEmitter {
            distance_interval: 1.5,
            size: [0.3, 0.9],
            lifetime_s: [3.0, 4.5],
            min_velocity: [-0.8; 3],
            max_velocity: [0.8; 3],
            friction: 1.0,
            textures: vec!["splashbase".to_owned()],
            frames: Vec::new(),
            colors: vec![
                [0.0, 255.0, 180.0, 0.0, 1.0],
                [0.1, 100.0, 100.0, 100.0, 1.0],
            ],
        },
        switch_off_offset_s: Some(10.0),
    }
}

/// **A rocket the player launches leaves puffs along its flight, one per
/// `DISTANCE_INTERVAL` of travel, through the production emit pass.**
#[test]
fn accept_m01_rocket_trail_a_launched_rocket_leaves_puffs_along_its_flight() {
    let (_scratch, mut app, _actor) = armed_stage("m01-rocket-trail");
    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Rectangle::new(1.0, 1.0));
    let _ = app.world_mut().resource_mut::<Assets<Image>>();
    app.insert_resource(RocketTrailArt::from_handles(
        synthetic_definition(),
        BTreeMap::new(),
        mesh,
    ));

    let mut spawned = 0;
    for _ in 0..MAX_FRAMES {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyX);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyX);
        spawned = app.world().resource::<RocketTrailReport>().puffs_spawned;
        if spawned >= 3 {
            break;
        }
    }
    assert!(spawned >= 3, "the launched rocket left puffs: {spawned}");
    let live = app
        .world_mut()
        .query::<&RocketTrailPuff>()
        .iter(app.world())
        .count();
    assert!(live > 0, "the puffs are live entities");
    teardown(&mut app);
}

/// **Without the installed art the trail draws nothing**, so the test above
/// fails if the emit pass is removed.
#[test]
fn accept_m01_rocket_trail_no_art_no_puffs() {
    let (_scratch, mut app, _actor) = armed_stage("m01-rocket-trail-none");
    for _ in 0..60 {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyX);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyX);
    }
    assert_eq!(app.world().resource::<RocketTrailReport>().puffs_spawned, 0);
    teardown(&mut app);
}

/// **The retail `he_rocket` states the trail emitter.** Read from the
/// installation on every run, so a stale expectation fails.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_rocket_trail_retail_he_rocket_states_the_trail_emitter() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let definition = read_rocket_trail_definition(&root).expect("he_rocket is measured");
    let e = &definition.emitter;
    assert_eq!(e.distance_interval, 1.5);
    assert_eq!(e.size, [0.3, 0.9]);
    assert_eq!(e.lifetime_s, [3.0, 4.5]);
    assert_eq!(e.min_velocity, [-0.8; 3]);
    assert_eq!(e.max_velocity, [0.8; 3]);
    assert_eq!(e.friction, 1.0);
    assert_eq!(e.textures, vec!["splashbase".to_owned()]);
    assert_eq!(
        e.colors,
        vec![
            [0.0, 255.0, 180.0, 0.0, 1.0],
            [0.1, 100.0, 100.0, 100.0, 1.0]
        ]
    );
    assert_eq!(definition.switch_off_offset_s, Some(10.0));
}

/// **The M01 texture archive decodes the trail's `splashbase`.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_rocket_trail_retail_texture_decodes_from_the_world_archive() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let source = cs_app::mission_session::measure_rocket_trail(&root, "c3")
        .expect("the trail and its texture are measured");
    assert_eq!(source.images.len(), 1);
    assert_eq!(source.images[0].0, "splashbase");
}
