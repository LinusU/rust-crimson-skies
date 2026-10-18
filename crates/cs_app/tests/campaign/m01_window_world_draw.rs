//! M01-WINDOW-FARDRAW (Rally #1554): the imported far terrain is drawn through
//! the one world-draw path, and the drawn sea sits at the world's own water.
//!
//! The owner saw the sea end at a square: `render::horizon` drew its plane at a
//! designed `-1000 m` while M01's own water quads are flat tiles at `y = 0`.
//! The sea's altitude is now measured from the world's water geometry
//! ([`MissionWorldParts::water_level`]) and bound to the horizon spec by
//! [`bind_sea_to_world_water`].
//!
//! * the synthetic member pins the binding rule (CI);
//! * the retail member measures M01's own water through the production stage,
//!   binds a spec, and asserts the far-terrain records got material-bound
//!   children. `#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]`.
//!
//! Nothing here is `verified_original`: the altitude is measured from the
//! installation's geometry, how far the original's sea reaches is not.

use bevy::prelude::{App, ChildOf, Children, Entity, MeshMaterial3d, StandardMaterial};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionWorldParts, WorldWaterLevel, bind_sea_to_world_water, build_headless, stage_for,
    water_level_of,
};
use cs_app::render::horizon::{
    DESIGNED_LABEL, HorizonField, HorizonOrigin, HorizonSpec, WORLD_WATER_LABEL,
};
use cs_app::world::contacts::WorldObjectBinding;
use cs_content::world::WorldObjectId;
use cs_types::content::Resolved;

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::game_dir;

/// The far-terrain records of M01's `horizon` subtree that carry a mesh and
/// must be drawn (`g1155` 792, `h_zone2scroll` 794, `g1164` 796).
const FAR_TERRAIN_NODES: [u32; 3] = [792, 794, 796];

/// **A world water altitude moves the drawn sea and labels the move.**
#[test]
fn accept_m01_window_world_sea_follows_the_measured_water_level() {
    let mut app = App::new();
    app.insert_resource(HorizonSpec::designed());
    let designed = app.world().resource::<HorizonSpec>().sea_level_m();

    // No measured water: the sea stays where it was designed.
    assert!(!bind_sea_to_world_water(
        &mut app,
        &MissionWorldParts::default()
    ));
    assert!((app.world().resource::<HorizonSpec>().sea_level_m() - designed).abs() < f32::EPSILON);

    let parts = MissionWorldParts::default().with_water_level(Some(WorldWaterLevel {
        level_m: 12.5,
        vertices: 4,
    }));
    assert!(bind_sea_to_world_water(&mut app, &parts));
    let spec = app.world().resource::<HorizonSpec>();
    assert!((spec.sea_level_m() - 12.5).abs() < f32::EPSILON);
    assert_eq!(
        spec.origin(HorizonField::SeaLevel).expect("recorded"),
        &HorizonOrigin::Designed(WORLD_WATER_LABEL)
    );
    assert_ne!(WORLD_WATER_LABEL, DESIGNED_LABEL);
}

/// **A sea with raised pools still has a level: the altitude most water shares.**
///
/// `ZBD/C3`'s `wtr00000` binds the sea quads at `y = 0` and also raised pools
/// and falls up to 13 m above it; demanding every vertex flat found no sea and
/// left the horizon at its designed -1000 m, whose far edge showed as a grey band.
#[test]
fn accept_m01_window_world_water_level_is_the_dominant_flat_altitude() {
    let mut altitudes = vec![0.0_f32; 4477 - 160];
    altitudes.extend([13.167; 160]);
    let water = water_level_of(&altitudes).expect("most water is at one altitude");
    assert!(water.level_m.abs() < 1e-6);
    assert_eq!(water.vertices, 4477);

    // No altitude holds a majority, or no water at all: no level is invented.
    assert!(water_level_of(&[0.0, 5.0, 10.0, 15.0]).is_none());
    assert!(water_level_of(&[]).is_none());
}

/// **M01's drawn sea is at the world's own water level, and its far terrain
/// is drawn as material-bound children.**
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_window_world_retail_sea_level_and_far_terrain() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();

    let water = app
        .world()
        .resource::<MissionWorldParts>()
        .water_level()
        .expect("M01's water quads are one flat surface");
    assert!(water.vertices >= 144 * 4, "144 quads: {}", water.vertices);
    assert!(
        water.level_m.abs() < 1e-3,
        "M01's water quads sit at y = 0, measured {}",
        water.level_m
    );

    // The windowed face's spec, bound through the production function.
    app.insert_resource(HorizonSpec::designed());
    let parts = app.world().resource::<MissionWorldParts>().clone();
    assert!(bind_sea_to_world_water(&mut app, &parts));
    assert!(
        (app.world().resource::<HorizonSpec>().sea_level_m() - water.level_m).abs() < f32::EPSILON
    );

    // The far terrain reached the draw report and is parented as
    // material-bound children of its own node.
    for slot in FAR_TERRAIN_NODES {
        let id = WorldObjectId::new(&format!("node-{slot}")).expect("a valid id");
        let object = stage
            .world
            .object(&id)
            .unwrap_or_else(|| panic!("node-{slot} is in the definition"));
        let Resolved::Known(mesh) = object.mesh() else {
            panic!("node-{slot} names a mesh");
        };
        let drawn = app
            .world()
            .resource::<MissionWorldParts>()
            .for_mesh(&mesh.value)
            .map_or(0, <[_]>::len);
        assert!(drawn > 0, "node-{slot}'s mesh has drawn parts");

        let world = app.world_mut();
        let mut bindings = world.query::<(Entity, &WorldObjectBinding)>();
        let node = bindings
            .iter(world)
            .find(|(_, binding)| binding.object() == &id)
            .map(|(entity, _)| entity)
            .unwrap_or_else(|| panic!("node-{slot} is spawned"));
        let children = world
            .get::<Children>(node)
            .unwrap_or_else(|| panic!("node-{slot} has drawn children"));
        let bound = children
            .iter()
            .filter(|&&child| {
                world
                    .get::<MeshMaterial3d<StandardMaterial>>(child)
                    .is_some()
                    && world
                        .get::<ChildOf>(child)
                        .is_some_and(|c| c.parent() == node)
            })
            .count();
        assert_eq!(
            bound, drawn,
            "node-{slot}'s parts are its material-bound children"
        );
        assert!(
            world
                .get::<MeshMaterial3d<StandardMaterial>>(node)
                .is_none(),
            "node-{slot} stays material-less; the collider is derived from it"
        );
    }
}
