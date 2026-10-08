//! Scratch probe: dump the world-side condition operands of M01's lowered
//! program plus the world node names and animation record names.
use std::path::PathBuf;

use cs_app::animation::mission::bind_mission_animation;
use cs_app::mission_control::survey_mission_control_programs;
use cs_app::world::retail::read_world_containers;
use cs_content::textures::WorldTextureLoad;
use cs_script::ir::Condition;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

fn walk(condition: &Condition, depth: usize, out: &mut Vec<String>) {
    match condition {
        Condition::InactiveMembers { members, threshold } => {
            out.push(format!(
                "{:depth$}INACTIVE threshold={threshold} members={members:?}",
                ""
            ));
        }
        Condition::EnemyGroupDepletion {
            group,
            remaining,
            generator,
        } => {
            out.push(format!(
                "{:depth$}DEDG group={group} remaining={remaining} generator={generator:?}",
                ""
            ));
        }
        Condition::Travelers {
            subject,
            anchor,
            radius,
            approaching,
        } => {
            out.push(format!(
                "{:depth$}TRAVELERS subject={subject:?} anchor={anchor:?} radius={radius} approaching={approaching}",
                ""
            ));
        }
        Condition::AnimationStates {
            required,
            animations,
        } => {
            out.push(format!(
                "{:depth$}ANIM_STATE required={required} animations={animations:?}",
                ""
            ));
        }
        Condition::All(inner) | Condition::Any(inner) => {
            for c in inner {
                walk(c, depth + 1, out);
            }
        }
        Condition::Not(inner) => walk(inner, depth + 1, out),
        _ => {}
    }
}

#[test]
#[ignore = "scratch probe"]
fn dump_m01_world_conditions() {
    let census = survey_mission_control_programs(&game_dir()).expect("census");
    let row = census.row("zbd/c1c/m01").expect("M01");
    let program = row
        .lowering_attempt()
        .expect("attempt")
        .program()
        .expect("program")
        .clone();
    for (i, objective) in program.objectives.iter().enumerate() {
        let mut lines = Vec::new();
        walk(&objective.condition, 0, &mut lines);
        if !lines.is_empty() || [5, 53, 2, 14].contains(&i) {
            eprintln!("objective[{i}] id={:?} condition={:?}", objective.id, objective.condition);
            for line in lines {
                eprintln!("  {line}");
            }
        }
    }
}

#[test]
#[ignore = "scratch probe"]
fn dump_m01_world_names() {
    let root = game_dir();
    let containers = read_world_containers(&root).expect("containers");
    let container = containers
        .container("c1c", &WorldTextureLoad::project_default())
        .expect("c1c container");
    for node in &container.nodes().nodes {
        eprintln!(
            "node[{}] {:?} type={} parent={:?} children={:?}",
            node.index,
            node.name,
            node.kind.label(),
            node.parent,
            node.children
        );
    }
}

#[test]
#[ignore = "scratch probe"]
fn dump_m01_member_subtrees() {
    let root = game_dir();
    let containers = read_world_containers(&root).expect("containers");
    let container = containers
        .container("c1c", &WorldTextureLoad::project_default())
        .expect("c1c container");
    let nodes = &container.nodes().nodes;
    // For every distinct chain element spelled by M01, count candidate nodes
    // in the whole tree and show ancestry.
    let census = survey_mission_control_programs(&root).expect("census");
    let row = census.row("zbd/c1c/m01").expect("M01");
    let program = row
        .lowering_attempt()
        .expect("attempt")
        .program()
        .expect("program")
        .clone();
    let mut chains = Vec::new();
    for objective in &program.objectives {
        collect_chains(&objective.condition, &mut chains);
    }
    let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for chain in &chains {
        for part in chain {
            names.insert(part.clone());
        }
    }
    let path_of = |idx: usize| -> String {
        let mut parts = vec![nodes[idx].name.clone()];
        let mut cur = nodes[idx].parent;
        while let Some(p) = cur {
            let p = p as usize;
            parts.push(nodes[p].name.clone());
            cur = nodes[p].parent;
        }
        parts.reverse();
        parts.join("/")
    };
    for name in &names {
        let hits: Vec<usize> = nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| (n.name == *name).then_some(i))
            .collect();
        eprintln!("name {name:?}: {} node(s)", hits.len());
        for h in hits {
            eprintln!("    {}", path_of(h));
        }
    }
}

fn collect_chains(condition: &Condition, out: &mut Vec<Vec<String>>) {
    match condition {
        Condition::InactiveMembers { members, .. } => out.extend(members.iter().cloned()),
        Condition::Travelers {
            subject, anchor, ..
        } => {
            out.push(subject.clone());
            if let cs_script::ir::TravelersAnchor::Object(m) = anchor {
                out.push(m.clone());
            }
        }
        Condition::All(inner) | Condition::Any(inner) => {
            for c in inner {
                collect_chains(c, out);
            }
        }
        Condition::Not(inner) => collect_chains(inner, out),
        _ => {}
    }
}

#[test]
#[ignore = "scratch probe"]
fn dump_m01_anim_names() {
    let binding = bind_mission_animation(&game_dir(), "zbd/c1c/m01").expect("binding");
    eprintln!("--- carrier anim_names ---");
    for fact in binding.carriers() {
        eprintln!(
            "carrier {:?} {} records={}",
            fact.kind(),
            fact.container_key(),
            fact.record_count()
        );
    }
    let survey =
        cs_app::animation::carrier::survey_animation_bindings(&game_dir()).expect("survey");
    for key in ["zbd/c1c/m01/mis_anim.zbd", "zbd/c1c/cam_anim.zbd"] {
        let carrier = survey.carrier(key).expect("carrier binding");
        if let Some(records) = carrier.payload.as_ref().and_then(|p| p.records.as_ref()) {
            for (i, name) in records.anim_names.iter().enumerate() {
                eprintln!("{key} record[{i}] anim_name={:?}", String::from_utf8_lossy(name));
            }
        }
    }
    eprintln!("--- startup rows ---");
    for row in binding.startup() {
        eprintln!(
            "startup: event={} identity={} record={:?} targets={:?}",
            row.event(),
            row.identity(),
            row.bound_record().map(|r| (r.carrier, r.index, r.anim_name.clone(), r.status, r.activation)),
            row.targets().iter().map(|t| (t.stored().to_owned(), t.resolution().occurrences())).collect::<Vec<_>>(),
        );
    }
    for carrier in binding.carriers() {
        eprintln!("carrier {:?} {}", carrier.kind(), carrier.container_key());
    }
}
