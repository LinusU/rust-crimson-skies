//! Task #1735 `PARITY2-M01-MISSING-TERRAIN`: the island's terrain groups hang
//! below grouping nodes of the world record, which the flat world definition
//! cannot hold, so a chunk of the island was never drawn
//! (`docs/findings/2026-10-21-m01-missing-terrain.md`).

use std::path::PathBuf;

use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MAX_SKY_BELOW_HORIZON_PIXELS, MissionWorldParts, build_headless, sky_below_horizon, stage_for,
    teardown,
};
use cs_app::world::retail::read_world_container;
use cs_content::campaign_bindings::MissionLabel;
use cs_content::textures::WorldTextureLoad;

/// A `width` x `height` `sea_absent` probe frame: the clear colour down to
/// `horizon`, the water tiles below, with land over the `land` columns whose
/// skyline climbs a row per column from row 90.
fn probe(width: usize, height: usize, horizon: usize, land: std::ops::Range<usize>) -> Vec<bool> {
    let mut world = vec![false; width * height];
    for y in 0..height {
        for x in 0..width {
            let sea = y >= horizon;
            let ground = land.contains(&x) && y >= 90_usize.saturating_sub(x % 50);
            world[y * width + x] = sea || ground;
        }
    }
    world
}

/// The probe finds the sea horizon from the columns that share it and counts
/// the clear colour below it: none for a closed island, every cut pixel for a
/// hole.
#[test]
fn accept_parity2_m01_missing_terrain_clear_colour_below_the_horizon_is_counted() {
    let (width, height) = (200, 120);
    let closed = probe(width, height, 90, 0..120);
    assert_eq!(sky_below_horizon(width, &closed), (Some(89), 0));

    // The island's middle is missing below the horizon: a 40 x 30 cut that
    // shows the clear colour where the sea and the shore should be.
    let mut holed = closed.clone();
    for y in 91..height {
        for x in 40..80 {
            holed[y * width + x] = false;
        }
    }
    let (row, holes) = sky_below_horizon(width, &holed);
    assert_eq!(row, Some(89));
    let expected = (89 + 1 + 4..height).count() * 40;
    assert_eq!(holes, expected, "only the rows past the margin count");
    assert!(holes > MAX_SKY_BELOW_HORIZON_PIXELS);

    // No clear colour anywhere: nothing to measure against, nothing missing.
    assert_eq!(
        sky_below_horizon(width, &vec![true; width * height]),
        (None, 0)
    );
}

/// **Retail:** every terrain tile the container stores below a group of the
/// world record is drawn, and draws the triangles the data declares.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity2_m01_missing_terrain_every_terrain_tile_of_the_world_is_drawn() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let label = MissionLabel::new("M01").expect("label");
    let plan = plan_mission_launch(&root, label, "The Lost Treasure").expect("M01 plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01 composes");
    app.update();

    let container =
        read_world_container(&root, "C3", &WorldTextureLoad::project_default()).expect("C3 reads");
    let nodes = container.nodes();
    let world = nodes
        .nodes
        .iter()
        .find(|node| matches!(node.kind, cs_formats::gamez::NodeKind::World(_)))
        .expect("a world record")
        .index;

    // The tiles: a mesh-bearing node named `g<digits>` that hangs below a
    // direct child of the world record that is itself a `g<digits>` group,
    // directly under a `g<digits>` group — the island's terrain groups (a gun
    // turret's `g6` mesh under `gun`, behind a LOD band, is not terrain).
    let is_g = |name: &str| {
        name.strip_prefix('g')
            .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
    };
    let top_of = |mut slot: u32| {
        while let Some(parent) = nodes.get(slot).and_then(|node| node.parent) {
            if parent == world {
                return Some(slot);
            }
            slot = parent;
        }
        None
    };
    let tiles: Vec<&cs_formats::gamez::nodes::RawNode> = nodes
        .nodes
        .iter()
        .filter(|node| node.mesh_index() >= 0 && is_g(&node.name))
        .filter(|node| node.parent != Some(world))
        .filter(|node| {
            node.parent
                .and_then(|parent| nodes.get(parent))
                .is_some_and(|parent| is_g(&parent.name))
        })
        .filter(|node| {
            top_of(node.index)
                .and_then(|top| nodes.get(top))
                .is_some_and(|top| is_g(&top.name))
        })
        .collect();
    assert!(
        tiles.len() >= 7,
        "the container stores terrain tiles below its g-groups, found {}",
        tiles.len()
    );

    let parts = app.world().resource::<MissionWorldParts>();
    let scenery = parts.scenery().expect("the world draw built its scenery");
    let mut declared = 0usize;
    let mut drawn = 0usize;
    let mut dropped = Vec::new();
    for tile in &tiles {
        let mesh = container.meshes().meshes[tile.mesh_index() as usize]
            .as_ref()
            .expect("a stored tile mesh");
        let triangles = mesh.draw_topology().triangles.len();
        declared += triangles;
        let placed = scenery
            .placements
            .iter()
            .find(|placement| placement.node_slot == tile.index);
        match placed.and_then(|placement| scenery.meshes.get(&placement.mesh)) {
            Some(pieces) => {
                let got: usize = pieces.iter().map(|piece| piece.triangles).sum();
                assert_eq!(
                    got, triangles,
                    "tile {} (slot {}) draws the triangles its mesh declares",
                    tile.name, tile.index
                );
                drawn += got;
            }
            None => dropped.push(format!("{} (slot {})", tile.name, tile.index)),
        }
    }
    assert!(
        dropped.is_empty(),
        "terrain tiles the world does not draw: {dropped:?}"
    );
    assert_eq!(drawn, declared);
    teardown(&mut app);
}
