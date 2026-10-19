//! M01-SEA-SOURCE-RECONCILE (Rally #1698): what draws M01's sea is the world's
//! own `wtr00000` water tiles; the horizon plane only continues them.
//!
//! * the synthetic member pins the smoke's judge (CI): a sea whose water tiles
//!   draw nothing fails `world_sea_visible`, however much of the frame the
//!   horizon plane covers, and a world with no water tiles and no water
//!   records is the honest skip;
//! * the retail member composes M01 through the production stage and counts the
//!   drawn children that bind the water texture, then checks the horizon sea
//!   clones the tiles' own material. `#[ignore = "requires CS_GAME_DIR"]`.
//!
//! Unknown, recorded in `docs/findings/2026-10-19-m01-sea-source-reconcile.md`:
//! the unit of the 16-frame cycle and the horizon fade form.

use bevy::asset::Assets;
use bevy::prelude::{Mesh3d, MeshMaterial3d, StandardMaterial, With};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MIN_SEA_COVERAGE_PERMILLE, MissionWorldParts, SeaMetrics, SmokeMeasurements, WorldWaterPart,
    build_headless, evaluate_mission_smoke, stage_for,
};

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::game_dir;

fn sea_verdict(sea: SeaMetrics) -> (bool, bool) {
    let measurements = SmokeMeasurements {
        sea: Some(sea),
        ..SmokeMeasurements::default()
    };
    let checks = evaluate_mission_smoke(&measurements);
    let check = checks
        .iter()
        .find(|check| check.id == "world_sea_visible")
        .expect("the smoke reports world_sea_visible");
    (check.applies, check.passed)
}

fn sea(tile_permille: u32, tile_parts: usize) -> SeaMetrics {
    let total = 1_000_000;
    SeaMetrics {
        sea_pixels: 400_000,
        total_pixels: total,
        covered_permille: 400,
        tile_pixels: (tile_permille as usize) * 1000,
        tile_permille,
        water_objects: 0,
        water_tile_parts: tile_parts,
    }
}

/// **The sea is the world's tiles: a plane alone does not pass the check.**
#[test]
fn accept_parity_m01_sea_source_the_judge_requires_the_world_tiles_to_draw_the_sea() {
    assert_eq!(
        sea_verdict(sea(MIN_SEA_COVERAGE_PERMILLE + 10, 255)),
        (true, true)
    );
    assert_eq!(
        sea_verdict(sea(0, 255)),
        (true, false),
        "tiles that draw nothing are not the sea"
    );
    let mut records_only = sea(300, 0);
    records_only.water_objects = 1;
    assert_eq!(
        sea_verdict(records_only),
        (true, false),
        "water records without a drawn water part: the plane is not the world's sea"
    );
    let mut dry = sea(0, 0);
    dry.sea_pixels = 0;
    dry.covered_permille = 0;
    assert_eq!(
        sea_verdict(dry),
        (false, false),
        "no water: the honest skip"
    );
}

/// **M01's world draw binds `wtr00000` tiles as drawn children, and the horizon
/// sea is a clone of their material.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_sea_source_retail_wtr00000_tiles_are_the_drawn_sea() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();
    app.update();
    let parts = app.world().resource::<MissionWorldParts>().clone();
    let art = parts.water_art().expect("M01's world binds water art");
    assert!(
        art.image.to_ascii_lowercase().ends_with("wtr00000")
            || art.image.to_ascii_lowercase().contains("wtr00000"),
        "the water art is wtr00000: {}",
        art.image
    );

    let mut tiles = 0_usize;
    let mut textured = 0_usize;
    let mut query = app
        .world_mut()
        .query_filtered::<(&Mesh3d, &MeshMaterial3d<StandardMaterial>), With<WorldWaterPart>>();
    let handles: Vec<_> = query
        .iter(app.world())
        .map(|(_, material)| material.0.clone())
        .collect();
    let materials = app.world().resource::<Assets<StandardMaterial>>();
    for handle in &handles {
        tiles += 1;
        if materials
            .get(handle)
            .is_some_and(|m| m.base_color_texture.is_some())
        {
            textured += 1;
        }
    }
    assert!(
        tiles >= 100,
        "the world draws its own water tiles as the sea: {tiles} parts"
    );
    assert_eq!(textured, tiles, "every water tile part carries the texture");
}
