//! Diagnostic for Rally #1537 (M01-WINDOW-WORLD): which of M01's world meshes
//! the mission path binds textured, and which the player actually sees.
//!
//! Temporary investigation member; the real acceptance tests for the task
//! land here once the defect is fixed.

use bevy::mesh::{Mesh, VertexAttributeValues};
use bevy::prelude::Assets;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{MissionWorldParts, build_headless, stage_for};
use cs_types::content::Resolved;

use crate::common::label;
use crate::vs_m01_rt_window::game_dir;

fn aabb(mesh: &Mesh) -> Option<([f32; 3], [f32; 3])> {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return None;
    };
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in positions {
        for axis in 0..3 {
            min[axis] = min[axis].min(p[axis]);
            max[axis] = max[axis].max(p[axis]);
        }
    }
    Some((min, max))
}

/// Dumps, for every object M01's stage loads, the mesh it names and the drawn
/// parts that mesh bound — texture key or neutral — with the mesh's own
/// world-space box, sorted by distance to the measured start.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_window_world_diag_dump_bindings() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let start = stage.start.position;
    eprintln!("start pose: {:?} heading {}", start, stage.start.heading);

    let mut app = build_headless(&stage).expect("the stage composes headlessly");
    app.update();
    let parts = app.world().resource::<MissionWorldParts>();
    eprintln!(
        "report: {} meshes, {} parts, {} textured",
        parts.mesh_count(),
        parts.part_count(),
        parts.report().textured_parts
    );

    // Per object: mesh slot, per-part texture/neutral, mesh world-space AABB.
    let mut rows: Vec<(String, f32)> = Vec::new();
    for object in stage.world.objects() {
        let Resolved::Known(mesh) = object.mesh() else {
            continue;
        };
        let Some(drawn) = parts.for_mesh(&mesh.value) else {
            continue;
        };
        let assets = app.world().resource::<Assets<Mesh>>();
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        let mut detail = String::new();
        for part in drawn {
            if let Some(part_mesh) = assets.get(&part.mesh)
                && let Some((lo, hi)) = aabb(part_mesh)
            {
                for axis in 0..3 {
                    min[axis] = min[axis].min(lo[axis]);
                    max[axis] = max[axis].max(hi[axis]);
                }
            }
            detail.push_str(&format!(
                " {}:{}t",
                part.image.as_deref().unwrap_or("NEUTRAL"),
                part.triangles
            ));
        }
        // Local-space AABB centre carried through the object's authored
        // transform: records whose vertices are stored world-space have an
        // identity transform and this is a no-op.
        let local_center = [
            (min[0] + max[0]) / 2.0,
            (min[1] + max[1]) / 2.0,
            (min[2] + max[2]) / 2.0,
        ];
        let center = object.transform().apply([
            local_center[0] as f64,
            local_center[1] as f64,
            local_center[2] as f64,
        ]);
        let distance = ((center[0] - f64::from(start[0])).powi(2)
            + (center[2] - f64::from(start[2])).powi(2))
        .sqrt() as f32;
        let extent = (max[0] - min[0]).max(max[2] - min[2]);
        let t = object.transform();
        let tr = t.translation();
        let lin = t.linear();
        let scale = (lin[0][0].powi(2) + lin[1][0].powi(2) + lin[2][0].powi(2)).sqrt();
        rows.push((
            format!(
                "{} mesh={} t=({:.0},{:.0},{:.0}) s={:.2} wc=({:.0},{:.0},{:.0}) span={:.0} parts={}",
                object.id(),
                mesh.value,
                tr[0],
                tr[1],
                tr[2],
                scale,
                center[0],
                center[1],
                center[2],
                extent,
                detail.trim()
            ),
            distance,
        ));
    }
    rows.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    for (row, distance) in &rows {
        eprintln!("d={distance:7.0}m {row}");
    }
    panic!("diagnostic dump only — read the stderr table");
}

/// Reads `ZBD/C1C/gamez.zbd` straight off disk and dumps, for the mesh slots
/// the bindings dump showed binding NEUTRAL, each mesh's polygon count,
/// material-group counts and whether corners/groups carry UVs and normals —
/// the check for whether the terrain's texture coordinates exist in the data.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_window_world_diag_dump_terrain_meshes() {
    let root = game_dir();
    let bytes = std::fs::read(root.join("ZBD/C1C/gamez.zbd")).expect("gamez.zbd reads");
    let mut context = cs_formats::io::ParseContext::with_defaults("c1c/gamez.zbd");
    let meshes = cs_formats::gamez::read_gamez_meshes(&mut context, "c1c/gamez.zbd", &bytes)
        .expect("mesh section reads");
    eprintln!("c1c gamez: {} mesh slots", meshes.meshes.len());
    // The slots the bindings dump showed as NEUTRAL, plus a few water/cloud
    // cells as a control.
    for slot in [269u32, 274, 401, 415, 552, 553, 554, 555, 556, 557, 558, 559, 560, 561, 562, 563,
        564, 565, 566, 567, 568, 569, 570, 571, 572]
    {
        let Some(Some(mesh)) = meshes.meshes.get(slot as usize) else {
            eprintln!("mesh {slot}: absent");
            continue;
        };
        let polygons = mesh.mesh.polygons.len();
        let corners_total: usize = mesh.mesh.polygons.iter().map(|p| p.corners.len()).sum();
        let corners_uv = mesh
            .mesh
            .polygons
            .iter()
            .flat_map(|p| p.corners.iter())
            .filter(|c| c.uv.is_some())
            .count();
        let corners_normal = mesh
            .mesh
            .polygons
            .iter()
            .flat_map(|p| p.corners.iter())
            .filter(|c| c.normal.is_some())
            .count();
        let groups_total: usize = mesh.material_groups.iter().map(Vec::len).sum();
        let groups_uv: usize = mesh
            .material_groups
            .iter()
            .flatten()
            .filter(|g| !g.uvs.is_empty())
            .count();
        let materials: Vec<u32> = mesh
            .material_groups
            .iter()
            .flatten()
            .map(|g| g.material)
            .collect();
        eprintln!(
            "mesh {slot}: polys={} corners={} uv_corners={}/{} normal_corners={}/{} groups={} \
             groups_with_uvs={} materials={:?}",
            polygons,
            corners_total,
            corners_uv,
            corners_total,
            corners_normal,
            corners_total,
            groups_total,
            groups_uv,
            materials
        );
    }

    // Resolve the material indices the dump surfaced: the terrain's 158/84
    // against the water's 155 and the cloud layer's 156.
    let materials =
        cs_formats::gamez::read_gamez_materials(&mut context, "c1c/gamez.zbd", &bytes)
            .expect("material section reads");
    eprintln!(
        "c1c gamez: {} materials, {} texture names",
        materials.count(),
        materials.textures.len()
    );
    for index in [84u32, 155, 156, 158] {
        let Some(material) = materials.material(index) else {
            eprintln!("material {index}: absent");
            continue;
        };
        let texture = materials
            .texture_of(material)
            .map(|t| t.name.as_str())
            .unwrap_or("<none>");
        eprintln!(
            "material {index}: kind={:?} texture_index={} texture={}",
            material.kind(),
            material.record.texture_index,
            texture
        );
    }

    // The nodes that carried the NEUTRAL meshes: names say whether they are
    // fog volumes, terrain or props.
    let nodes = cs_formats::gamez::read_gamez_nodes(&mut context, &bytes)
        .expect("node section reads");
    for (index, node) in nodes.nodes.iter().enumerate() {
        if (552..=572).contains(&node.mesh_index()) || node.name.contains("fvol") {
            eprintln!(
                "node-{index} name={} mesh_index={} kind={:?}",
                node.name, node.info.mesh_index, node.kind
            );
        }
    }
    panic!("diagnostic dump only — read the stderr table");
}

/// Reads `ZBD/C1C/rtexture10.zbd` (the tier the mission's texture archive
/// picks) straight off disk — no install discovery — and prints every stored
/// texture's name, extent and mean decoded colour, so the "is the terrain
/// texture grey" question has a direct answer.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_window_world_diag_dump_textures() {
    let root = game_dir();
    let path = root.join("ZBD/C1C/rtexture10.zbd");
    let bytes = std::fs::read(&path).expect("rtexture10.zbd reads");
    let mut budget = cs_formats::io::AllocationBudget::new("rtexture10.zbd", 1 << 30);
    let package = cs_formats::texture::zbd::read_zbd_textures("rtexture10.zbd", &bytes, &mut budget)
        .expect("texture package parses");
    eprintln!("c1c rtexture10: {} textures", package.textures().len());
    for texture in package.textures() {
        let extent = texture.descriptor().extent();
        let mut budget = cs_formats::io::AllocationBudget::new("tex", 1 << 30);
        let summary = match texture.decode(&mut budget) {
            Ok(image) => {
                let mut sum = [0u64; 4];
                let mut count = 0u64;
                for y in 0..extent.height {
                    for x in 0..extent.width {
                        if let Some(t) = image.texel(x, y) {
                            for (i, b) in t.iter().take(4).enumerate() {
                                sum[i] += u64::from(*b);
                            }
                            count += 1;
                        }
                    }
                }
                if count == 0 {
                    "empty".to_owned()
                } else {
                    format!(
                        "mean=[{},{},{},{}]",
                        sum[0] / count,
                        sum[1] / count,
                        sum[2] / count,
                        sum[3] / count
                    )
                }
            }
            Err(error) => format!("decode error: {error}"),
        };
        eprintln!(
            "  #{} {} {}x{} {}",
            texture.entry_index(),
            texture.name(),
            extent.width,
            extent.height,
            summary
        );
    }
    panic!("diagnostic dump only — read the stderr table");
}
