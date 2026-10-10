//! Acceptance stage VS-M01-RT-PLAYER-AIRFRAME-VISUAL (Rally #1216): the
//! mission composition draws M01's own player airframe — the Devastator the
//! original assigns, scene root `player_pfighter`, model `piratefighter` —
//! instead of nothing.

use std::path::PathBuf;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!("CS_GAME_DIR is not set: this suite needs the retail capability")
    }))
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
