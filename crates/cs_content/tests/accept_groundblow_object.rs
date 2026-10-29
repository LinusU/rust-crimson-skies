//! Task #2225 (`GROUNDBLOW-OBJECT-SWEEP`): the water tiles that are **direct
//! children** of M01's world record spawn through the flat world draw as
//! `WorldColliderInstance` object colliders, so the ground-blow sweep reaches
//! them through `WorldSweepNode`, not `PlaytestSea`.
//!
//! Finding: `docs/findings/2026-10-29-groundblow-object-sweep.md`.
//! Task test prefix: `accept_groundblow_object_`.
//!
//! This file pins the container census the task's decision rests on, read
//! through the production `cs_formats` gamez readers: `world1` (slot 0) has
//! 453 direct children, **251** of which are flagged mesh nodes that bind a
//! water texture — the `g28xxx` tiles `g28530` (node 547, mesh 371) belongs
//! to. The other **96** water nodes sit below grouping nodes and reach the
//! sweep through the scenery pass's `PlaytestSea` colliders.

use cs_formats::ParseContext;
use cs_formats::gamez::{
    NODE_TYPE_WORLD, read_gamez_materials, read_gamez_meshes, read_gamez_nodes,
};

/// The two bits of `node+0x24` the world sweep requires before it tests a node
/// (`docs/findings/2026-10-29-groundblow-world-segment-sweep.md`).
const CONTACT_SWEEP_NODE_FLAGS: u32 = 0x14;

/// The stored flag word every `world1` direct-child water tile carries: the
/// `0x0308_831c` row of `2026-10-29-groundblow-sweep-props.md`'s census.
const DIRECT_CHILD_WATER_FLAGS: u32 = 0x0308_831c;

/// The stored flag word every grouped water tile carries.
const GROUPED_WATER_FLAGS: u32 = 0x0308_031c;

/// One container read through the production readers.
fn read_container(
    root: &str,
    path: &str,
) -> (
    cs_formats::gamez::GameZNodes,
    cs_formats::gamez::GameZMeshes,
    cs_formats::gamez::GameZMaterials,
) {
    let bytes = std::fs::read(format!("{root}/{path}")).expect("the container reads");
    let mut context = ParseContext::with_defaults(path.to_ascii_lowercase());
    let nodes = read_gamez_nodes(&mut context, &bytes).expect("nodes");
    let meshes = read_gamez_meshes(&mut context, path, &bytes).expect("meshes");
    let materials = read_gamez_materials(&mut context, path, &bytes).expect("materials");
    (nodes, meshes, materials)
}

/// Whether a mesh binds at least one water-textured material group, and
/// whether it binds **only** water textures — the two populations
/// `accept_groundblow_class_` measures (347 any-water, 344 only-water).
fn binds_water(
    meshes: &cs_formats::gamez::GameZMeshes,
    materials: &cs_formats::gamez::GameZMaterials,
    mesh_index: i32,
) -> (bool, bool) {
    let Some(mesh) = meshes
        .meshes
        .get(usize::try_from(mesh_index).expect("a non-negative mesh index"))
        .and_then(Option::as_ref)
    else {
        return (false, false);
    };
    let mut any = false;
    let mut all = true;
    let mut textured = false;
    for group in mesh.material_groups.iter().flatten() {
        let Some(material) = materials.materials.get(group.material as usize) else {
            all = false;
            continue;
        };
        let Some(name) = materials
            .texture_of(material)
            .map(|texture| texture.name.to_ascii_lowercase())
        else {
            continue;
        };
        textured = true;
        if name.starts_with("wtr0") || name.starts_with("water1") {
            any = true;
        } else {
            all = false;
        }
    }
    (any, any && all && textured)
}

/// M01's 347 flagged water nodes split into **251 `world1` direct children**
/// (which spawn through the flat world draw as `WorldColliderInstance` object
/// colliders) and **96 grouped descendants** (which spawn through the scenery
/// pass as `PlaytestSea` colliders). Every one is class 5 and stores the
/// sweep's `0x14` bits, so every spawned water collider is a sweep target.
///
/// `g28530` — node 547, mesh 371 — is one of the 251, not one of "~3": the
/// task's 344-vs-347 gap is a *material* distinction (three water nodes also
/// bind a non-water group), not a placement one, and only one of the three
/// (`g28533`, node 571) is a direct child.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_groundblow_object_m01_water_nodes_split_between_flat_objects_and_scenery() {
    let root = std::env::var("CS_GAME_DIR")
        .expect("CS_GAME_DIR is not set: this census needs the owner installation's ZBD/C3 data");
    let (nodes, meshes, materials) = read_container(&root, "ZBD/C3/gamez.zbd");
    let world = nodes
        .nodes
        .iter()
        .find(|node| node.info.node_type == NODE_TYPE_WORLD)
        .expect("the world record");
    assert_eq!(world.index, 0, "world1 is the container's first record");
    assert_eq!(world.name, "world1");

    let mut direct_children = 0usize;
    let mut direct_water = Vec::new();
    let mut grouped_water = Vec::new();
    let mut mixed = Vec::new();
    let mut water_only = 0usize;
    for node in &nodes.nodes {
        let direct_child = node.parent == Some(world.index) && node.index != world.index;
        if direct_child {
            direct_children += 1;
        }
        if node.mesh_index() < 0
            || node.flags() & CONTACT_SWEEP_NODE_FLAGS != CONTACT_SWEEP_NODE_FLAGS
        {
            continue;
        }
        let (any, only) = binds_water(&meshes, &materials, node.mesh_index());
        if !any {
            continue;
        }
        if only {
            water_only += 1;
        } else {
            mixed.push(node);
        }
        if direct_child {
            direct_water.push(node);
        } else {
            grouped_water.push(node);
        }
    }

    assert_eq!(
        direct_children, 453,
        "world1's direct-child records — the flat world draw's object list"
    );
    assert_eq!(
        direct_water.len(),
        251,
        "the direct-child water tiles: the object-collider population"
    );
    assert!(
        direct_water
            .iter()
            .all(|node| node.flags() == DIRECT_CHILD_WATER_FLAGS && node.info.node_type == 5),
        "every direct-child water tile is a flagged grid-indexed class-5 node"
    );
    assert_eq!(
        grouped_water.len(),
        96,
        "the grouped water tiles: the PlaytestSea population"
    );
    assert!(
        grouped_water
            .iter()
            .all(|node| node.flags() == GROUPED_WATER_FLAGS && node.info.node_type == 5),
        "every grouped water tile is a flagged class-5 node"
    );
    assert_eq!(direct_water.len() + grouped_water.len(), 347);
    assert_eq!(water_only, 344, "the class census's water-only count");

    // The three water nodes that also bind a non-water group — the 344-vs-347
    // gap the task asked about. Only `g28533` is a `world1` direct child;
    // `ter_poly` and `craterlake`'s `waterbottom` are grouped descendants that
    // spawn `PlaytestSea` colliders like the other 96.
    let mut mixed_names: Vec<(&str, u32, bool)> = mixed
        .iter()
        .map(|node| {
            (
                node.name.as_str(),
                node.index,
                node.parent == Some(world.index),
            )
        })
        .collect();
    mixed_names.sort();
    assert_eq!(
        mixed_names,
        [
            ("g28533", 571, true),
            ("ter_poly", 2866, false),
            ("waterbottom", 2996, false),
        ]
    );

    // The tile the task names: g28530 is a world1 direct child that binds
    // mesh 371, flagged and class-5 — one of the 251.
    let g28530 = nodes
        .nodes
        .iter()
        .find(|node| node.name == "g28530")
        .expect("g28530");
    assert_eq!(g28530.index, 547);
    assert_eq!(g28530.mesh_index(), 371);
    assert_eq!(g28530.parent, Some(world.index));
    assert_eq!(g28530.flags(), DIRECT_CHILD_WATER_FLAGS);
    assert!(
        direct_water.iter().any(|node| node.index == g28530.index),
        "g28530 is counted among the direct-child water tiles"
    );
}
