//! #2225 (`GROUNDBLOW-OBJECT-SWEEP`): the water tiles that are **direct
//! children** of M01's world record are world-definition flat objects — they
//! spawn through the flat world draw as `WorldColliderInstance` object
//! colliders (`node-<slot>`), so the ground-blow sweep reaches them through
//! the `WorldSweepNode` marker (#2216), not through `PlaytestSea`.
//!
//! Task test prefix: `accept_groundblow_object_`. The container census this
//! verifies is `cs_content`'s
//! `accept_groundblow_object_m01_water_nodes_split_between_flat_objects_and_scenery`
//! (finding `docs/findings/2026-10-29-groundblow-object-sweep.md`): 251 of
//! M01's 347 flagged water nodes are `world1` direct children — including
//! `g28530` (node 547, mesh 371) — and the other 96 are grouped descendants
//! that spawn `PlaytestSea` colliders.
use std::collections::BTreeSet;
use std::path::PathBuf;

use avian3d::prelude::{Collider, Position, Rotation};
use bevy::prelude::*;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    LaunchCinematic, WorldSweepNode, build_headless, stage_for, teardown,
};
use cs_app::physics::BodyLayer;
use cs_app::playtest::scene::PlaytestSea;
use cs_app::playtest::world_sweep::{class_type_intersects, probe_in_world};
use cs_app::playtest_retail::CONTACT_SWEEP_NODE_FLAGS;
use cs_app::render::bevy_mesh::{ColliderFaces, ColliderMesh};
use cs_app::world::contacts::{WorldColliderInstance, WorldObjectBinding};
use cs_content::mesh::CollisionMesh;
use cs_content::world::WorldCollisionRole;
use cs_formats::gamez::{GameZMaterials, GameZMeshes, NODE_TYPE_WORLD};
use cs_sim::collision::CollisionLayer;
use cs_sim::flight::original::SegmentProbeOwner;

use crate::campaign::common::{label, retail_lock};

fn nose_down() -> Quat {
    Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
}

/// The same water-material rule `cs_content`'s census uses: the mesh binds at
/// least one `wtr0*`/`water1*` texture group.
fn binds_water(meshes: &GameZMeshes, materials: &GameZMaterials, mesh_index: i32) -> bool {
    let Some(mesh) = meshes
        .meshes
        .get(usize::try_from(mesh_index).expect("a non-negative mesh index"))
        .and_then(Option::as_ref)
    else {
        return false;
    };
    mesh.material_groups.iter().flatten().any(|group| {
        materials
            .materials
            .get(group.material as usize)
            .and_then(|material| materials.texture_of(material))
            .is_some_and(|texture| {
                let name = texture.name.to_ascii_lowercase();
                name.starts_with("wtr0") || name.starts_with("water1")
            })
    })
}

/// The composed M01's swept water population: every direct-child water tile
/// is a solid `WorldColliderInstance` object collider marked `WorldSweepNode`
/// (the population rule the task decided), and every grouped water tile is a
/// `PlaytestSea` — together all 347 flagged water nodes the original sweeps.
///
/// The test recomputes the direct-child set straight off the container, so a
/// regression that leaves `g28xxx` tiles unmarked (the pre-#2216 population),
/// unspawned or on the wrong layer fails by name.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_groundblow_object_m01_direct_child_water_tiles_are_swept() {
    let _retail = retail_lock();
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("plan");
    let stage = stage_for(&root, &plan).expect("stage");
    let world = stage
        .world_draw
        .as_ref()
        .expect("the retail stage draws its world")
        .sources()
        .world();
    let nodes = world.nodes().clone();
    let meshes = world.meshes().clone();
    let materials = world.materials().clone();
    let mut app = build_headless(&stage).expect("M01 composes");
    app.world_mut().remove_resource::<LaunchCinematic>();
    app.update();

    // The expected population, recomputed from the stored nodes: `world1`'s
    // direct children that bind a mesh, store the sweep's `0x14` bits, have a
    // class the sweep dispatches and bind a water texture.
    let world_slot = nodes
        .nodes
        .iter()
        .find(|node| node.info.node_type == NODE_TYPE_WORLD)
        .map(|node| node.index)
        .expect("the world record");
    let expected: BTreeSet<u32> = nodes
        .nodes
        .iter()
        .filter(|node| {
            node.parent == Some(world_slot)
                && node.index != world_slot
                && node.mesh_index() >= 0
                && node.flags() & CONTACT_SWEEP_NODE_FLAGS == CONTACT_SWEEP_NODE_FLAGS
                && class_type_intersects(node.info.node_type)
                && binds_water(&meshes, &materials, node.mesh_index())
        })
        .map(|node| node.index)
        .collect();
    assert_eq!(
        expected.len(),
        251,
        "the C3 census's direct-child water tiles (accept_groundblow_object's own count)"
    );

    // Every expected tile spawned a solid static object collider and is
    // marked — the exact entity set the sweep's collider query reads.
    let mut seen = BTreeSet::new();
    let mut tile_547 = None;
    for (entity, binding, instance, marker) in app
        .world_mut()
        .query::<(
            Entity,
            &WorldObjectBinding,
            &WorldColliderInstance,
            Option<&WorldSweepNode>,
        )>()
        .iter(app.world())
    {
        let slot: u32 = binding
            .object()
            .as_str()
            .strip_prefix("node-")
            .and_then(|slot| slot.parse().ok())
            .expect("an imported object id is `node-<slot>`");
        let Some(stored) = nodes.nodes.get(slot as usize) else {
            continue;
        };
        if !expected.contains(&slot) {
            continue;
        }
        seen.insert(slot);
        let marker = marker.unwrap_or_else(|| {
            panic!(
                "node-{slot} {:?} is a flagged direct-child water tile but is not swept",
                stored.name
            )
        });
        assert_eq!(marker.node_slot, slot);
        assert_eq!(marker.class_type, stored.info.node_type);
        assert_eq!(instance.role(), WorldCollisionRole::Solid);
        assert_eq!(
            app.world().get::<BodyLayer>(entity).map(|layer| layer.0),
            Some(CollisionLayer::StaticWorld),
            "node-{slot} is a static world body"
        );
        assert!(
            app.world().get::<Position>(entity).is_some()
                && app.world().get::<Rotation>(entity).is_some()
                && app.world().get::<Collider>(entity).is_some()
                && app.world().get::<ColliderFaces>(entity).is_some(),
            "node-{slot} carries everything the sweep's query and face record need"
        );
        if slot == 547 {
            tile_547 = Some(entity);
        }
    }
    assert_eq!(
        seen.len(),
        expected.len(),
        "every direct-child water tile spawned its collider"
    );

    // The grouped water tiles are the `PlaytestSea` entities; together the two
    // populations are all 347 flagged water nodes.
    let sea = app
        .world_mut()
        .query_filtered::<Entity, With<PlaytestSea>>()
        .iter(app.world())
        .count();
    assert_eq!(
        sea, 96,
        "the grouped water tiles spawn the scenery pass's sea colliders"
    );
    assert_eq!(seen.len() + sea, 347);

    // A dive at g28530 (node 547) answers the sweep as a world contact — and
    // stops answering it when the marker is removed. Each collision
    // triangle's centroid is probed in turn: other swept surfaces can overlap
    // the tile (the `watersetup` plane is coplanar water too), so the tile
    // proves itself where the nearest answer is its own.
    let tile = tile_547.expect("g28530 spawned as node-547");
    let position = app.world().get::<Position>(tile).expect("a pose").0;
    let rotation = app.world().get::<Rotation>(tile).expect("a pose").0;
    let raw371 = meshes.meshes[371].as_ref().expect("mesh 371");
    let collision = CollisionMesh::of(&raw371.mesh);
    let canonical = ColliderMesh::of(&collision);
    let prober = app.world_mut().spawn_empty().id();
    let mut proved = None;
    for triangle in 0..collision.indices().len() {
        let [a, b, c] = collision.indices()[triangle];
        let centroid = (Vec3::from(canonical.vertices()[a as usize])
            + Vec3::from(canonical.vertices()[b as usize])
            + Vec3::from(canonical.vertices()[c as usize]))
            / 3.0;
        let start = rotation * centroid + position + Vec3::Y * 30.0;
        let Some(hit) = probe_in_world(app.world(), start, nose_down(), 400.0, prober) else {
            continue;
        };
        assert_eq!(
            hit.owner,
            SegmentProbeOwner::World,
            "a water tile answers as world: {hit:?}"
        );
        if hit.entity == tile {
            proved = Some(start);
            break;
        }
    }
    let start = proved.expect("a dive at the tile answers as that tile somewhere");
    app.world_mut().entity_mut(tile).remove::<WorldSweepNode>();
    let after = probe_in_world(app.world(), start, nose_down(), 400.0, prober);
    assert!(
        after.is_none_or(|hit| hit.entity != tile),
        "without the marker the tile leaves the sweep population"
    );

    teardown(&mut app);
}
