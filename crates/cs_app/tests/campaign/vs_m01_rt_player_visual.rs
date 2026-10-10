//! Acceptance stage VS-M01-RT-PLAYER-AIRFRAME-VISUAL (Rally #1216): the
//! mission composition draws M01's own player airframe — the Devastator the
//! original assigns, scene root `player_pfighter`, model `piratefighter` —
//! instead of nothing.

use std::path::PathBuf;

use bevy::prelude::{ChildOf, Children, Entity, With};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionPlayerBody, MissionPlayerVisual, build_headless, stage_for, teardown,
};
use cs_app::playtest::PlaytestRequests;
use cs_app::playtest_retail::AircraftPart;

use crate::common::label;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!("CS_GAME_DIR is not set: this suite needs the retail capability")
    }))
}

/// The measured intact selection of M01's player airframe
/// (`docs/findings/2026-10-10-vs-m01-rt-player-airframe-visual.md` §3): every
/// mesh-bearing node under `healthy` (slot 149)'s chosen `nearest` band plus
/// `pirate_hook` (outside every band, inside `healthy`), plus the pinned
/// `staticprop1` propeller — the draw set `select_aircraft_parts` produces
/// for the `player` root of `zbd/planes.zbd`.
const MEASURED_DRAWN_SLOTS: [u32; 34] = [
    151, 152, 153, 156, 157, 158, 161, 163, 165, 167, 168, 171, 173, 175, 177, 178, 179, 180, 185,
    186, 187, 188, 189, 190, 191, 192, 193, 194, 210, 2330, 2336, 2360, 2511, 3005,
];

/// Every `AircraftPart` under `body`, with its stored node slot.
fn drawn_parts(world: &mut bevy::prelude::World, body: Entity) -> Vec<(Entity, u32)> {
    let mut query = world.query_filtered::<(Entity, &AircraftPart, &ChildOf), With<AircraftPart>>();
    let mut parts: Vec<(Entity, u32)> = query
        .iter(world)
        .filter(|(_, _, child)| child.parent() == body)
        .map(|(entity, part, _)| (entity, part.node_slot))
        .collect();
    parts.sort_by_key(|(_, slot)| *slot);
    parts
}

fn only_player(world: &mut bevy::prelude::World) -> Entity {
    let mut query = world.query_filtered::<Entity, With<MissionPlayerBody>>();
    let players: Vec<Entity> = query.iter(world).collect();
    assert_eq!(players.len(), 1, "exactly one player body must exist");
    players[0]
}

/// **M01's composition draws the measured player airframe**: the pinned
/// `player`/`healthy`/`staticprop1` selection resolves to the measured slot
/// and name set, and exactly one drawn body carries its parts.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_vs_m01_runtime_player_visual_retail_m01_draws_the_measured_airframe() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    assert!(plan.launchable(), "M01's launch surfaces are satisfied");
    let stage = stage_for(&root, &plan).expect("M01's stage reads through the production readers");
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();

    // The selection the stage measured: the `player` root's `healthy` intact
    // node 149, `nearest` chosen at the designed 20 m, `staticprop1` 210.
    let (report_fields, propeller, mut drawn) = {
        let visual = app
            .world()
            .get_resource::<MissionPlayerVisual>()
            .expect("a retail stage builds the player airframe's visual");
        let report = visual.report();
        (
            (
                report.root_name.clone(),
                report.intact_node_slot,
                report.intact_node_name.clone(),
                report.lod_distance_m,
                report.selected_lod.clone(),
            ),
            visual
                .propeller()
                .map(|spec| (spec.node_slot, spec.node_name.clone())),
            visual
                .parts()
                .iter()
                .map(|part| part.node_slot)
                .collect::<Vec<u32>>(),
        )
    };
    let (root_name, intact_slot, intact_name, lod_m, selected_lod) = report_fields;
    assert_eq!(root_name, "player");
    assert_eq!(intact_slot, 149);
    assert_eq!(intact_name, "healthy");
    assert_eq!(lod_m, 20.0);
    assert_eq!(
        selected_lod,
        Some((150, "nearest".to_owned())),
        "the intact selection must choose the measured nearest band"
    );
    assert_eq!(
        propeller,
        Some((210, "staticprop1".to_owned())),
        "the pinned propeller node is drawn"
    );

    // The drawn set is the measured one: every slot the finding lists,
    // nothing else.
    drawn.sort_unstable();
    assert_eq!(
        drawn,
        MEASURED_DRAWN_SLOTS.to_vec(),
        "the drawn airframe set must be the measured intact selection"
    );

    // One body, its parts its own children — and every part draws at least
    // one textured piece.
    let world = app.world_mut();
    let body = only_player(world);
    let parts = drawn_parts(world, body);
    assert_eq!(
        parts.len(),
        MEASURED_DRAWN_SLOTS.len(),
        "one drawn body carries exactly the measured parts"
    );
    for (entity, slot) in &parts {
        let children = world
            .get::<Children>(*entity)
            .expect("a drawn part holds its textured pieces");
        assert!(
            !children.is_empty(),
            "aircraft part slot {slot} draws no material-group piece"
        );
    }

    teardown(&mut app);
}

/// **Restart and teardown leave exactly one drawn airframe**: `R` respawns
/// the body with a fresh copy of the parts and despawns the old one, and
/// teardown leaves nothing behind.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_vs_m01_runtime_player_visual_restart_and_teardown_leave_one() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    assert!(plan.launchable(), "M01's launch surfaces are satisfied");
    let stage = stage_for(&root, &plan).expect("M01's stage reads through the production readers");
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();

    let first_body = only_player(app.world_mut());
    let first_parts: Vec<Entity> = drawn_parts(app.world_mut(), first_body)
        .into_iter()
        .map(|(entity, _)| entity)
        .collect();
    assert!(
        !first_parts.is_empty(),
        "the first spawn draws the airframe parts"
    );

    // `R` through the playtest's own request resource: the same seam
    // `meta_controls` sets from the key.
    app.world_mut().resource_mut::<PlaytestRequests>().reset = true;
    app.update();

    let second_body = only_player(app.world_mut());
    assert_ne!(
        first_body, second_body,
        "the restart spawns a fresh player body"
    );
    let second_parts = drawn_parts(app.world_mut(), second_body);
    assert_eq!(
        second_parts.len(),
        first_parts.len(),
        "the restarted body draws the same parts"
    );
    for entity in &first_parts {
        assert!(
            app.world_mut().get::<AircraftPart>(*entity).is_none(),
            "the first spawn's part {entity:?} must be despawned with its body"
        );
    }

    teardown(&mut app);
    let world = app.world_mut();
    let players = world
        .query_filtered::<Entity, With<MissionPlayerBody>>()
        .iter(world)
        .count();
    let parts = world
        .query_filtered::<Entity, With<AircraftPart>>()
        .iter(world)
        .count();
    assert_eq!(
        (players, parts),
        (0, 0),
        "teardown must leave no player body and no drawn airframe part"
    );
    assert!(
        !world.contains_resource::<MissionPlayerVisual>(),
        "teardown must remove the player visual's resource"
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn probe_dump_player_airframe_subtrees() {
    let root = game_dir();
    let sources = cs_app::playtest_retail::read_playtest_sources(&root, "C1C")
        .expect("the retail sources read");
    let adapter = cs_app::playtest_retail::playtest_adapter().expect("the adapter builds");
    let planes = cs_app::playtest_retail::aircraft_graph(sources.aircraft(), &adapter)
        .expect("planes.zbd builds a graph");

    eprintln!("== roots ==");
    for id in planes.roots() {
        let node = planes.node(id).unwrap();
        eprintln!("root slot {} name {:?}", node.index(), node.name());
    }

    eprintln!("\n== duplicated node names ==");
    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for node in planes.nodes() {
        *counts.entry(node.name()).or_default() += 1;
    }
    for (name, count) in &counts {
        if *count > 1 {
            eprintln!("{name}: {count}");
        }
    }
    for wanted in [
        "bloodhawk",
        "player_pfighter",
        "piratefighter",
        "healthy",
        "staticprop1",
    ] {
        eprintln!("{wanted}: {}", counts.get(wanted).copied().unwrap_or(0));
    }

    eprintln!("\n== transforms of the airframe path ==");
    for slot in [1418_u32, 44, 2324, 153, 2407, 179, 2433] {
        let Some(node) = planes.nodes().iter().find(|n| n.index() == slot) else {
            continue;
        };
        let local = node.local_transform();
        let world = node.world_transform();
        eprintln!(
            "slot {:>5} {:<18} local t={:?} l={:?}\n        world t={:?} l={:?}",
            slot,
            node.name(),
            local.translation(),
            local.linear(),
            world.translation(),
            world.linear(),
        );
    }

    eprintln!("\n== materials of selected meshes ==");
    for slot in [
        153_u32, 179, 180, 185, 186, 187, 188, 189, 190, 191, 192, 193, 194, 234, 79, 80, 81, 82,
        2360, 2336, 3005, 2511, 2330, 208, 209, 210, 211, 212, 213, 2436, 2437,
    ] {
        let Some(node) = planes.nodes().iter().find(|n| n.index() == slot) else {
            continue;
        };
        let Some(binding) = node.mesh() else {
            eprintln!("slot {slot:>5} {:<18} no mesh", node.name());
            continue;
        };
        let Some(mesh) = sources.aircraft().meshes().get(binding.index) else {
            eprintln!(
                "slot {slot:>5} {:<18} mesh#{} missing",
                node.name(),
                binding.index
            );
            continue;
        };
        let mut names = Vec::new();
        for groups in &mesh.material_groups {
            for group in groups {
                let material = sources.aircraft().materials().material(group.material);
                let texture = material
                    .and_then(|m| sources.aircraft().materials().texture_of(m))
                    .map(|t| t.name.clone())
                    .unwrap_or_else(|| "<none>".to_owned());
                names.push(format!("mat{}:{texture}", group.material));
            }
        }
        eprintln!(
            "slot {slot:>5} {:<18} mesh#{} [{}]",
            node.name(),
            binding.index,
            names.join(", ")
        );
    }

    for wanted in [
        "piratefighter",
        "player_bhawk",
        "player",
        "player_pfighter",
        "player_damage_on",
        "player_damage_off",
    ] {
        for start in planes.nodes().iter().filter(|node| node.name() == wanted) {
            eprintln!("\n== subtree of {wanted:?} slot {} ==", start.index());
            let mut chain = Vec::new();
            let mut cursor = start.parent().and_then(|id| planes.node(id));
            while let Some(current) = cursor {
                chain.push(format!("{}({})", current.name(), current.index()));
                cursor = current.parent().and_then(|id| planes.node(id));
            }
            chain.reverse();
            eprintln!(
                "start: slot {} name {:?} ancestors [{}] children {}",
                start.index(),
                start.name(),
                chain.join(" <- "),
                start.children().len(),
            );
            let members = planes.subtree(start.id());
            eprintln!("subtree: {} nodes", members.len());
            for node in &members {
                let parent_slot = node
                    .parent()
                    .and_then(|id| planes.node(id))
                    .map(|n| n.index());
                let kind = match node.kind() {
                    cs_content::scene::NodeKind::Lod(info) => {
                        format!("Lod({}-{})", info.range_min.0, info.range_max.0)
                    }
                    other => format!("{other:?}"),
                };
                let mesh = node
                    .mesh()
                    .map(|b| format!(" mesh#{}", b.index))
                    .unwrap_or_default();
                eprintln!(
                    "  slot {:>5} parent {:>5?} name {:<24} kind {:<40}{}",
                    node.index(),
                    parent_slot,
                    node.name(),
                    kind,
                    mesh
                );
            }
        }
    }
}
