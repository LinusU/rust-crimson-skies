//! Task #1667 `PARITY-M01-PLAYER-VIEW-B`: the player's propeller blur disc,
//! shown from the engine state (`docs/findings/2026-10-18-m01-propeller-blur-disc.md`).

use std::path::PathBuf;

use bevy::asset::Assets;
use bevy::prelude::{AlphaMode, ChildOf, Children, Entity, StandardMaterial, Visibility, With};
use bevy::prelude::{Mesh3d, MeshMaterial3d};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MISSION_PLAYER_BLUR_NODE_NAME, MISSION_PLAYER_BLUR_NODE_SLOT,
    MISSION_PLAYER_BLUR_SECOND_NODE_SLOT, MissionPlayerBody, MissionPlayerVisual, build_headless,
    stage_for, teardown,
};
use cs_app::physics::FlightAircraft;
use cs_app::playtest::headless_app;
use cs_app::playtest::propeller::{PropellerBlur, PropellerBlurSecond, propeller_blur_visible};
use cs_app::playtest::scene::PlaytestAircraft;
use cs_app::playtest_retail::AircraftPart;
use cs_sim::flight::EngineState;

use crate::campaign::common::label;

fn visibility_of(app: &bevy::prelude::App, entity: Entity) -> Visibility {
    *app.world().get::<Visibility>(entity).expect("a visibility")
}

#[test]
fn accept_parity_m01_player_view_b_blur_rule_is_the_running_flag() {
    assert!(propeller_blur_visible(EngineState {
        running: true,
        spool: 0.0
    }));
    assert!(!propeller_blur_visible(EngineState::STOPPED));
}

/// The disc shows while the flight body's engine runs and hides when it stops.
#[test]
fn accept_parity_m01_player_view_b_blur_follows_the_engine_state() {
    let mut app = headless_app();
    app.update();
    let body = app
        .world_mut()
        .query_filtered::<Entity, With<PlaytestAircraft>>()
        .single(app.world())
        .expect("one player aircraft");
    let blur = app
        .world_mut()
        .spawn((PropellerBlur, Visibility::Hidden, ChildOf(body)))
        .id();
    app.update();
    assert_eq!(
        visibility_of(&app, blur),
        Visibility::Inherited,
        "a running engine shows the disc"
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
        visibility_of(&app, blur),
        Visibility::Hidden,
        "a stopped engine hides it"
    );
    // A blur whose parent has no engine is left as spawned.
    let orphan_parent = app.world_mut().spawn_empty().id();
    let stray = app
        .world_mut()
        .spawn((PropellerBlur, Visibility::Hidden, ChildOf(orphan_parent)))
        .id();
    app.update();
    assert_eq!(visibility_of(&app, stray), Visibility::Hidden);
}

/// **M01's player carries the measured `prop1` blur disc**, translucent from
/// the texture's coverage, hidden until the engine runs.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_parity_m01_player_view_b_retail_player_draws_the_measured_blur_disc() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let plan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();

    let radius = {
        let visual = app
            .world()
            .get_resource::<MissionPlayerVisual>()
            .expect("the player visual");
        let blur = visual.blur().expect("the blur disc is drawn");
        assert_eq!(blur.node_slot, MISSION_PLAYER_BLUR_NODE_SLOT);
        assert_eq!(blur.node_name, MISSION_PLAYER_BLUR_NODE_NAME);
        eprintln!("blur radius_m = {}", blur.radius_m);
        blur.radius_m
    };
    assert!(
        radius > 0.0 && radius.is_finite(),
        "measured radius {radius}"
    );

    let world = app.world_mut();
    let body = world
        .query_filtered::<Entity, With<MissionPlayerBody>>()
        .single(world)
        .expect("one player body");
    let blurs: Vec<(Entity, u32)> = world
        .query_filtered::<(Entity, &AircraftPart, &ChildOf), With<PropellerBlur>>()
        .iter(world)
        .filter(|(_, _, child)| child.parent() == body)
        .map(|(entity, part, _)| (entity, part.node_slot))
        .collect();
    assert_eq!(blurs.len(), 1, "exactly one blur disc on the body");
    assert_eq!(blurs[0].1, MISSION_PLAYER_BLUR_NODE_SLOT);
    let disc = blurs[0].0;

    let pieces: Vec<Entity> = world
        .get::<Children>(disc)
        .expect("pieces")
        .iter()
        .copied()
        .collect();
    assert!(!pieces.is_empty());
    for piece in pieces {
        let handle = world
            .get::<MeshMaterial3d<StandardMaterial>>(piece)
            .expect("a piece material")
            .0
            .clone();
        let material = world
            .resource::<Assets<StandardMaterial>>()
            .get(&handle)
            .expect("the material is stored");
        assert_eq!(material.alpha_mode, AlphaMode::Blend, "translucent disc");
        assert!(
            material.base_color_texture.is_some(),
            "rotorblur texture bound"
        );
        let _ = world.get::<Mesh3d>(piece).expect("a mesh");
    }

    // Engine state drives it, over the mission's own flight body.
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        visibility_of(&app, disc),
        Visibility::Inherited,
        "the mission starts with the engine running, so the disc shows"
    );

    teardown(&mut app);
}

/// **The blur texture's own ring**: the drawn `rotorblur.tif` is a 64x64
/// translucent disc whose coverage falls from the centre outwards and whose
/// red channel peaks in an annulus at about 0.6 of the radius. That annulus is
/// the "ring detail" (`docs/findings/2026-10-21-parity2-dmg-fx-c-propeller-blur-profile.md`);
/// it is texture content, not a record, and nothing here invents a ring count.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_parity2_dmg_fx_c_retail_blur_texture_is_translucent_with_a_ring() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let plan = plan_mission_launch(&root, label("M06"), "The Red Menace").expect("plan");
    let stage = stage_for(&root, &plan).expect("stage");
    let mut app = build_headless(&stage).expect("compose");
    app.update();
    let world = app.world_mut();
    let disc = world
        .query_filtered::<Entity, With<PropellerBlur>>()
        .single(world)
        .expect("disc");
    let pieces: Vec<Entity> = world
        .get::<Children>(disc)
        .unwrap()
        .iter()
        .copied()
        .collect();
    assert!(!pieces.is_empty());
    for piece in pieces {
        let handle = world
            .get::<MeshMaterial3d<StandardMaterial>>(piece)
            .unwrap()
            .0
            .clone();
        let tex = world
            .resource::<Assets<StandardMaterial>>()
            .get(&handle)
            .unwrap()
            .base_color_texture
            .clone()
            .expect("rotorblur texture");
        let image = world
            .resource::<Assets<bevy::prelude::Image>>()
            .get(&tex)
            .unwrap();
        let (w, h) = (image.width() as usize, image.height() as usize);
        assert_eq!((w, h), (64, 64));
        let data = image.data.as_ref().unwrap();
        let bins = 16usize;
        let mut red = vec![0f32; bins];
        let mut alpha = vec![0f32; bins];
        let mut n = vec![0f32; bins];
        for y in 0..h {
            for x in 0..w {
                let r = ((x as f32 + 0.5 - 32.0).powi(2) + (y as f32 + 0.5 - 32.0).powi(2)).sqrt()
                    / 32.0;
                let b = (r * bins as f32) as usize;
                if b < bins {
                    let i = (y * w + x) * 4;
                    red[b] += f32::from(data[i]);
                    alpha[b] += f32::from(data[i + 3]);
                    n[b] += 1.0;
                }
            }
        }
        for b in 0..bins {
            red[b] /= n[b];
            alpha[b] /= n[b];
        }
        assert!(
            alpha.iter().all(|a| *a < 128.0),
            "translucent everywhere: {alpha:?}"
        );
        assert!(
            alpha[0] > alpha[bins - 1] * 4.0,
            "coverage falls outwards: {alpha:?}"
        );
        let peak = (0..bins)
            .max_by(|a, b| red[*a].total_cmp(&red[*b]))
            .unwrap();
        assert!(
            (8..=11).contains(&peak) && red[peak] > red[0] + 10.0,
            "red annulus near 0.6 of the radius: peak bin {peak}, {red:?}"
        );
    }
    teardown(&mut app);
}

/// `prop1b` follows the same engine flag as `prop1` (`spinprops` makes both
/// active), and is not the damage trail's `PropellerBlur` anchor.
#[test]
fn accept_parity2_m01_damage_effects_second_blur_disc_follows_the_engine_state() {
    let mut app = headless_app();
    app.update();
    let body = app
        .world_mut()
        .query_filtered::<Entity, With<PlaytestAircraft>>()
        .single(app.world())
        .expect("one player aircraft");
    let second = app
        .world_mut()
        .spawn((PropellerBlurSecond, Visibility::Hidden, ChildOf(body)))
        .id();
    app.update();
    assert_eq!(visibility_of(&app, second), Visibility::Inherited);
    let mut query = app
        .world_mut()
        .query_filtered::<&mut FlightAircraft, With<PlaytestAircraft>>();
    query
        .single_mut(app.world_mut())
        .expect("the production flight record")
        .set_engine(EngineState::STOPPED)
        .expect("an engine can be stopped");
    app.update();
    assert_eq!(visibility_of(&app, second), Visibility::Hidden);
}

/// M01's player draws `prop1b` beside `prop1`, both translucent.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_parity2_m01_damage_effects_retail_player_draws_both_blur_discs() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let plan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();
    let world = app.world_mut();
    let body = world
        .query_filtered::<Entity, With<MissionPlayerBody>>()
        .single(world)
        .expect("one player body");
    let second: Vec<(Entity, u32)> = world
        .query_filtered::<(Entity, &AircraftPart, &ChildOf), With<PropellerBlurSecond>>()
        .iter(world)
        .filter(|(_, _, child)| child.parent() == body)
        .map(|(entity, part, _)| (entity, part.node_slot))
        .collect();
    assert_eq!(second.len(), 1, "exactly one second blur disc");
    assert_eq!(second[0].1, MISSION_PLAYER_BLUR_SECOND_NODE_SLOT);
    let pieces: Vec<Entity> = world
        .get::<Children>(second[0].0)
        .expect("pieces")
        .iter()
        .copied()
        .collect();
    assert!(!pieces.is_empty());
    for piece in pieces {
        let handle = world
            .get::<MeshMaterial3d<StandardMaterial>>(piece)
            .expect("a piece material")
            .0
            .clone();
        let material = world
            .resource::<Assets<StandardMaterial>>()
            .get(&handle)
            .expect("the material is stored");
        assert_eq!(material.alpha_mode, AlphaMode::Blend);
    }
}
