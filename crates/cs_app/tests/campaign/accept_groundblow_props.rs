//! #2216 (`GROUNDBLOW-SWEEP-PROPS`): the world-definition objects whose source
//! nodes the original's world sweep tests are part of the port's sweep
//! population as `SegmentProbeOwner::World`.
//!
//! Task test prefix: `accept_groundblow_props_`. The population rule is the
//! decode's (`docs/findings/2026-10-29-groundblow-world-segment-sweep.md`: a
//! node is tested when `node+0x24 & 0x14 == 0x14`) with the class-type filter
//! of `docs/findings/2026-10-29-groundblow-class-type.md`; the finding of this
//! task is `docs/findings/2026-10-29-groundblow-sweep-props.md`.
use std::path::PathBuf;

use avian3d::prelude::{Collider, Position, Rotation};
use bevy::prelude::*;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{WorldSceneryCollider, WorldSweepNode, build_headless, stage_for};
use cs_app::physics::BodyLayer;
use cs_app::playtest::scene::PlaytestSea;
use cs_app::playtest::world_sweep::{REFUSED_CLASS_TYPES, class_type_intersects, probe_in_world};
use cs_app::playtest_retail::CONTACT_SWEEP_NODE_FLAGS;
use cs_app::world::contacts::{WorldColliderInstance, WorldObjectBinding};
use cs_sim::collision::CollisionLayer;
use cs_sim::flight::original::SegmentProbeOwner;

use crate::campaign::common::label;

fn nose_down() -> Quat {
    Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
}

/// A static box the sweep tests only when the world node marker says so.
fn prop_box(world: &mut World, marked: bool) -> Entity {
    let entity = world
        .spawn((
            Collider::cuboid(20.0, 10.0, 20.0),
            Position(Vec3::new(0.0, 5.0, 0.0)),
            Rotation(Quat::IDENTITY),
            BodyLayer(CollisionLayer::StaticWorld),
        ))
        .id();
    if marked {
        world.entity_mut(entity).insert(WorldSweepNode {
            node_slot: 7,
            class_type: 5,
        });
    }
    entity
}

/// A prop whose node the original sweeps answers as the world; the same body
/// without the marker (the port's synthetic ground and obstacle) answers
/// nothing.
#[test]
fn accept_groundblow_props_marked_node_answers_and_an_unmarked_body_does_not() {
    let mut world = World::new();
    let prober = world.spawn_empty().id();
    let prop = prop_box(&mut world, true);
    let hit = probe_in_world(
        &world,
        Vec3::new(0.0, 100.0, 0.0),
        nose_down(),
        400.0,
        prober,
    )
    .expect("the marked node answers");
    assert_eq!(hit.entity, prop);
    assert_eq!(hit.owner, SegmentProbeOwner::World);
    assert!((hit.distance_m - 90.0).abs() < 1e-3, "{hit:?}");

    let mut world = World::new();
    let prober = world.spawn_empty().id();
    prop_box(&mut world, false);
    assert!(
        probe_in_world(
            &world,
            Vec3::new(0.0, 100.0, 0.0),
            nose_down(),
            400.0,
            prober
        )
        .is_none(),
        "an unmarked StaticWorld body is not a world-database node"
    );

    // A node of a class the sweep's dispatch refuses answers nothing even when
    // marked: the sweep's own class filter applies to the marker's class.
    let mut world = World::new();
    let prober = world.spawn_empty().id();
    let refused = prop_box(&mut world, true);
    world.entity_mut(refused).insert(WorldSweepNode {
        node_slot: 7,
        class_type: REFUSED_CLASS_TYPES[0],
    });
    assert!(
        probe_in_world(
            &world,
            Vec3::new(0.0, 100.0, 0.0),
            nose_down(),
            400.0,
            prober
        )
        .is_none()
    );
}

fn collider_top_centre(app: &App, entity: Entity) -> Option<Vec3> {
    let collider = app.world().get::<Collider>(entity)?;
    let position = app.world().get::<Position>(entity)?.0;
    let rotation = app.world().get::<Rotation>(entity)?.0;
    let local = collider.shape_scaled().compute_local_aabb();
    let mut top = f32::NEG_INFINITY;
    let mut centre = Vec3::ZERO;
    for corner in 0..8 {
        let point = Vec3::new(
            if corner & 1 == 0 {
                local.mins.x
            } else {
                local.maxs.x
            },
            if corner & 2 == 0 {
                local.mins.y
            } else {
                local.maxs.y
            },
            if corner & 4 == 0 {
                local.mins.z
            } else {
                local.maxs.z
            },
        );
        let world = position + rotation * point;
        centre += world / 8.0;
        top = top.max(world.y);
    }
    Some(Vec3::new(centre.x, top, centre.z))
}

/// M01's own container decides the population: every marked object stores
/// `0x04|0x10` and a dispatched class, no marked object is also a sea or
/// scenery collider, every solid world-definition object whose node stores the
/// bits is marked (and no other is), and a dive at a marked prop answers
/// through the sweep — and stops answering when the marker is taken away.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_groundblow_props_m01_population_is_the_container_nodes_with_the_bits() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("plan");
    let stage = stage_for(&root, &plan).expect("stage");
    let nodes = stage
        .world_draw
        .as_ref()
        .expect("the retail stage draws its world")
        .sources()
        .world()
        .nodes()
        .clone();
    let mut app = build_headless(&stage).expect("M01 composes");
    app.update();

    let mut objects = app.world_mut().query::<(
        Entity,
        &WorldObjectBinding,
        &WorldColliderInstance,
        Option<&WorldSweepNode>,
        Has<PlaytestSea>,
        Has<WorldSceneryCollider>,
    )>();
    let mut marked = Vec::new();
    let mut expected = 0usize;
    for (entity, binding, instance, node, is_sea, is_scenery) in objects.iter(app.world()) {
        let slot: usize = binding
            .object()
            .as_str()
            .strip_prefix("node-")
            .and_then(|slot| slot.parse().ok())
            .expect("an imported object id is `node-<slot>`");
        let stored = &nodes.nodes[slot];
        let swept = instance.role() == cs_content::world::WorldCollisionRole::Solid
            && stored.flags() & CONTACT_SWEEP_NODE_FLAGS == CONTACT_SWEEP_NODE_FLAGS
            && class_type_intersects(stored.info.node_type);
        assert_eq!(
            node.is_some(),
            swept,
            "node-{slot} {:?}: flags {:#x} class {}",
            stored.name,
            stored.flags(),
            stored.info.node_type
        );
        if let Some(node) = node {
            expected += 1;
            assert_eq!(node.node_slot as usize, slot);
            assert_eq!(node.class_type, stored.info.node_type);
            assert_eq!(
                stored.flags() & CONTACT_SWEEP_NODE_FLAGS,
                CONTACT_SWEEP_NODE_FLAGS
            );
            assert!(!REFUSED_CLASS_TYPES.contains(&node.class_type));
            assert!(!is_sea && !is_scenery, "{:?} is counted twice", stored.name);
            marked.push((entity, stored.name.clone()));
        }
    }
    assert_eq!(marked.len(), expected);
    assert!(
        marked.len() > 300,
        "M01's world definition holds 374 such nodes, found {}",
        marked.len()
    );
    assert!(
        marked.iter().any(|(_, name)| name.starts_with("zepbridge")),
        "the zeppelin bridge props are among them"
    );

    // A dive at a marked prop answers through the sweep.
    let prober = app.world_mut().spawn_empty().id();
    let mut proved = None;
    for (entity, name) in &marked {
        let Some(top) = collider_top_centre(&app, *entity) else {
            continue;
        };
        let start = top + Vec3::Y * 30.0;
        let Some(hit) = probe_in_world(app.world(), start, nose_down(), 400.0, prober) else {
            continue;
        };
        if hit.entity == *entity {
            assert_eq!(hit.owner, SegmentProbeOwner::World, "{name}");
            proved = Some((*entity, start));
            break;
        }
    }
    let (entity, start) = proved.expect("a dive at a marked prop answers as that prop");
    app.world_mut()
        .entity_mut(entity)
        .remove::<WorldSweepNode>();
    let after = probe_in_world(app.world(), start, nose_down(), 400.0, prober);
    assert!(
        after.is_none_or(|hit| hit.entity != entity),
        "without the marker the prop is not swept"
    );
}
