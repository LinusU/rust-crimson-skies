//! Scratch dump of M01's bound targets: positions, target points, boxes.

use std::path::PathBuf;

use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::stage_for;
use cs_app::mission_targets::bind_mission_targets;
use cs_app::world_facts::WorldOperands;
use cs_assets::install::discover;
use cs_formats::gamez::read_gamez_nodes;
use cs_formats::io::ParseContext;

use crate::campaign::common::label;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn scratch_dump_m01_targets() {
    let root = game_dir();
    let plan =
        plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("launch closure plans");
    println!(
        "mission_dir={} group_dir={}",
        plan.mission_dir, plan.group_dir
    );
    let binding = bind_mission_targets(&root, &plan.mission_dir, &plan.group_dir);
    println!("=== {} bound targets ===", binding.targets.len());
    for target in &binding.targets {
        println!(
            "target {:>3?} {:28} pos={:?} tp={:?} obj={} other={} h={:?} d={:?} coupling={:?} allegiance={:?}",
            target.declared,
            target.node,
            target.position_m,
            target.target_point_m,
            target.objective,
            target.other_target,
            target.healthy,
            target.destroyed,
            target.coupling.fields,
            target.allegiance,
        );
    }
    for refusal in &binding.refusals {
        println!("REFUSAL {refusal}");
    }

    // Raw node boxes for every bound target.
    let found = discover(&root).expect("installation discovers");
    let key = format!("{}/gamez.zbd", plan.group_dir);
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == key)
        .expect("world container manifests");
    let bytes =
        std::fs::read(root.join(record.relative_spelling.as_str())).expect("container reads");
    let mut context = ParseContext::with_defaults(&key);
    let nodes = read_gamez_nodes(&mut context, &bytes).expect("node array reads");
    println!("=== raw boxes ===");
    for target in &binding.targets {
        let Some(raw) = nodes.get(target.index) else {
            println!("{}: no raw node at index {}", target.node, target.index);
            continue;
        };
        let info = &raw.info;
        println!(
            "{:28} idx={} u116={:?} u140={:?} u164={:?}",
            target.node, target.index, info.unk116, info.unk140, info.unk164,
        );
    }

    // And every node's box in the container, for overlap review.
    println!("=== all node boxes (non-empty only) ===");
    for (index, raw) in nodes.nodes.iter().enumerate() {
        let info = &raw.info;
        for (slot, bounds) in [
            ("u116", info.unk116),
            ("u140", info.unk140),
            ("u164", info.unk164),
        ] {
            if bounds.iter().flatten().any(|value| *value != 0.0) {
                println!("node {index:4} {slot}={bounds:?}");
            }
        }
    }

    // The four zeppelin-container names the task asks about.
    println!("=== zeppelin-container name search ===");
    for needle in [
        "rock_zeppelin",
        "workersvoyagezep",
        "blackswanzep",
        "hooks",
        "pzhookpoint",
        "piratezep",
    ] {
        let mut hits = Vec::new();
        for raw in &nodes.nodes {
            if raw.name == needle {
                hits.push(raw.index);
            }
        }
        println!("{needle}: {hits:?}");
    }

    // Targets whose composed position lands near the world origin — the
    // container-internal phantom set.
    println!("=== targets near origin ===");
    for target in &binding.targets {
        let near_origin = target
            .target_point_m
            .iter()
            .map(|v| v * v)
            .sum::<f64>()
            .sqrt()
            < 300.0;
        if near_origin {
            println!(
                "{:28} declared={:?} tp={:?} chain={:?} path={}",
                target.node, target.declared, target.target_point_m, target.chain, target.path
            );
        }
    }

    // Node kinds for the bound targets: object vs container/group.
    println!("=== bound-target node kinds ===");
    for target in &binding.targets {
        if let Some(raw) = nodes.get(target.index) {
            println!("{:28} idx={} kind={:?}", target.node, target.index, raw.kind.tag());
        }
    }

    // Do the other names exist in a sibling world container (zbd/c1c = M06)?
    for group in ["zbd/c1c", "zbd/c2", "zbd/c4"] {
        let key = format!("{group}/gamez.zbd");
        let Some(record) = found
            .manifest
            .files
            .iter()
            .find(|record| record.relative_spelling.logical_key() == key)
        else {
            println!("{key}: not in manifest");
            continue;
        };
        let bytes =
            std::fs::read(root.join(record.relative_spelling.as_str())).expect("container reads");
        let mut context = ParseContext::with_defaults(&key);
        let group_nodes = match read_gamez_nodes(&mut context, &bytes) {
            Ok(nodes) => nodes,
            Err(error) => {
                println!("{key}: {error}");
                continue;
            }
        };
        for needle in [
            "rock_zeppelin",
            "workersvoyagezep",
            "blackswanzep",
            "hooks",
        ] {
            let hits: Vec<u32> = group_nodes
                .nodes
                .iter()
                .filter(|node| node.name == needle)
                .map(|node| node.index)
                .collect();
            if !hits.is_empty() {
                println!("{key} {needle}: {hits:?}");
            }
        }
    }
}

/// Compose M01 headless and dump where every swept box sits vs the world
/// actors' bind poses — the item-4 overlap review.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn scratch_dump_m01_sweep_overlap() {
    use bevy::prelude::{Entity, GlobalTransform};
    use cs_app::mission_actors::{
        MissionGroundTarget, MissionGroundTargetSweep, MissionZeppelinActor,
    };
    use cs_app::mission_session::{MissionPartState, MissionPartSweep, build_headless};
    use cs_app::weapons::PartSweptBox;

    let root = game_dir();
    let plan =
        plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("launch closure plans");
    let stage = stage_for(&root, &plan).expect("stage reads");
    let mut app = build_headless(&stage).expect("M01 composes headlessly");
    app.update();
    app.update();

    println!("=== zeppelin actor bind poses ===");
    {
        let world = app.world_mut();
        let mut query = world.query::<&MissionZeppelinActor>();
        for actor in query.iter(world) {
            println!(
                "zeppelin actor={:?} pos={:?}",
                actor.damage_actor, actor.position_m
            );
        }
    }
    println!("=== ground targets: node, entity pos, sweep centre, half extents ===");
    {
        let world = app.world_mut();
        let mut query = world.query::<(&MissionGroundTarget, &MissionGroundTargetSweep)>();
        for (target, sweep) in query.iter(world) {
            println!(
                "{:28} pos={:?} centre={:?} half={:?}",
                target.node, target.position_m, sweep.centre_m, sweep.half_extents_m
            );
        }
    }
    println!("=== part-state sweeps within 800 m of the origin ===");
    {
        let world = app.world_mut();
        let mut query =
            world.query::<(&MissionPartState, &GlobalTransform, &MissionPartSweep)>();
        for (part, transform, sweep) in query.iter(world) {
            let c = transform.translation();
            if c.length() < 800.0 {
                println!("part {:?} centre={:?} half={:?}", part.chain, c, sweep.half_extents_m);
            }
        }
    }
    // Every PartSweptBox within 200 m of a ground-target box centre → overlap
    // candidates for the four at-origin container targets.
    println!("=== sweep candidates whose box overlaps a ground target box ===");
    {
        let world = app.world_mut();
        let mut boxes = world
            .query::<(Entity, &PartSweptBox, &GlobalTransform)>()
            .iter(world)
            .map(|(e, b, t)| (e, b.half_extents_m, t.translation()))
            .collect::<Vec<_>>();
        let mut grounds = world
            .query::<(Entity, &MissionGroundTarget, &MissionGroundTargetSweep)>()
            .iter(world)
            .map(|(e, t, s)| (e, t.node.clone(), s.centre_m, s.half_extents_m))
            .collect::<Vec<_>>();
        for (ge, node, gc, gh) in grounds.drain(..) {
            let gv = bevy::prelude::Vec3::new(gc[0] as f32, gc[1] as f32, gc[2] as f32);
            for (e, bh, bc) in boxes.iter() {
                if *e == ge {
                    continue;
                }
                // AABB overlap in each axis (axis-aligned, centres + half extents).
                let overlap = (0..3).all(|axis| {
                    let (ga, gb) = match axis {
                        0 => (gv.x, bc.x),
                        1 => (gv.y, bc.y),
                        _ => (gv.z, bc.z),
                    };
                    (ga - gb).abs() < (gh[axis] + bh[axis]) as f32
                });
                if overlap {
                    println!("{node:28} (centre {gv:?} half {gh:?}) overlaps entity {e:?} (centre {bc:?} half {bh:?})");
                }
            }
        }
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn scratch_dump_m01_operands() {
    let root = game_dir();
    let plan =
        plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("launch closure plans");
    let stage = stage_for(&root, &plan).expect("stage reads");
    let program = &stage.host.control;
    let operands = WorldOperands::of(program);
    println!("=== {} objectives, operand members ===", program.objectives.len());
    for member in &operands.members {
        println!("member {:?}", member.join("."));
    }
    println!(
        "groups {:?} group_members {:?} animations {:?} zones {:?}",
        operands.groups, operands.group_members, operands.animations, operands.danger_zones
    );
    // Every objective, full condition + actions.
    for (index, objective) in program.objectives.iter().enumerate() {
        println!(
            "block {index}: id={:?} {:?}\n   actions: {:?}",
            objective.id, objective.condition, objective.actions
        );
    }

    // The objectives.zrd and aiv.zrd members themselves, raw.
    for member in ["objectives.zrd", "aiv.zrd", "targets.zrd"] {
        println!("=== {member} ===");
        println!("{:?}", member_document(&root, "zbd/c3/m01/zrdr.zbd", member));
    }
}

/// Reads one member of a container as a decoded zrd document.
fn member_document(
    root: &std::path::Path,
    container_key: &str,
    member: &str,
) -> cs_content::stunts::ZrdValue {
    let found = cs_assets::install::discover(root).expect("the installation discovers");
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| {
            record.relative_spelling.logical_key() == container_key.to_ascii_lowercase()
        })
        .unwrap_or_else(|| panic!("the installation has {container_key}"));
    let spelling = record.relative_spelling.as_str().to_owned();
    let path =
        cs_types::install::RelativePath::new(&spelling.to_lowercase()).expect("a path");
    let bytes = std::fs::read(found.manifest.host_root.join(&spelling)).expect("readable");
    let discovery =
        cs_formats::script_raw::discover_container(container_key, &path, &bytes);
    let program = discovery
        .programs()
        .iter()
        .find(|program| {
            program
                .locator()
                .member()
                .is_some_and(|name| name.eq_ignore_ascii_case(member))
        })
        .unwrap_or_else(|| panic!("{container_key} declares a {member} member"));
    cs_content::stunts::decode_zrd(program.bytes())
        .unwrap_or_else(|error| panic!("{member} decodes: {error:?}"))
}
