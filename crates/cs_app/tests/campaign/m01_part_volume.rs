//! Acceptance suite M01-PART-VOLUME (Rally #2211): which collision volume each
//! M01 target part carries, and the swept box the runtime sweeps against.
//!
//! `docs/findings/2026-10-29-m01-target-part-collision-volumes.md` records the
//! measurement: no M01 target part sets node flag `0x20` (the bound-reject
//! box), every collidable leaf carries a mesh (the polygon soup the contact
//! query tests), and the grouping nodes the targets are bound at carry none.
//! The runtime therefore sweeps the union of the leaf parts' bounds instead of
//! the grouping node's box. Task test prefix: `accept_part_volume_`.
//!
//! Nothing here is `verified_original`: the volumes are a static read of the
//! installation and the engine image, and no original executable ran.

use std::path::PathBuf;

use cs_app::mission_targets::{BoundTarget, bind_mission_targets};
use cs_formats::gamez::{read_gamez_meshes, read_gamez_nodes};
use cs_formats::io::ParseContext;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

/// Parameter in `[0, 1]` where the segment first enters the box
/// `[centre ± half]`, or `None` when it never does.
fn entry_time(from: [f64; 3], to: [f64; 3], (centre, half): ([f64; 3], [f64; 3])) -> Option<f64> {
    let (mut near, mut far) = (0.0_f64, 1.0_f64);
    for axis in 0..3 {
        let delta = to[axis] - from[axis];
        let (low, high) = (centre[axis] - half[axis], centre[axis] + half[axis]);
        if delta.abs() < 1e-12 {
            if from[axis] < low || from[axis] > high {
                return None;
            }
            continue;
        }
        let (a, b) = ((low - from[axis]) / delta, (high - from[axis]) / delta);
        near = near.max(a.min(b));
        far = far.min(a.max(b));
    }
    (near <= far).then_some(near)
}

fn target<'a>(targets: &'a [BoundTarget], node: &str) -> &'a BoundTarget {
    targets
        .iter()
        .find(|target| target.node == node)
        .unwrap_or_else(|| panic!("{node} is bound"))
}

/// **Retail — the parts carry soup volumes, and the sweep narrows to them.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_part_volume_convoy_parts_carry_soup_and_the_sweep_narrows() {
    let root = game_dir();
    let binding = bind_mission_targets(&root, "zbd/c3/m01", "zbd/c3");
    let bytes = std::fs::read(root.join("ZBD/C3/gamez.zbd")).expect("the C3 world container");
    let mut context = ParseContext::with_defaults("zbd/c3/gamez.zbd");
    let nodes = read_gamez_nodes(&mut context, &bytes).expect("the node array");
    let mut context = ParseContext::with_defaults("zbd/c3/gamez.zbd");
    let meshes = read_gamez_meshes(&mut context, "zbd/c3/gamez.zbd", &bytes).expect("the meshes");

    for name in ["t_truck01", "t_truck02", "t_truck03", "bridge_truck01"] {
        let bound = target(&binding.targets, name);
        let root_node = nodes.get(bound.index).expect("the target node");
        // The grouping node the target is bound at carries no soup of its own.
        assert!(root_node.info.mesh_index < 0, "{name} is a grouping node");
        assert_eq!(root_node.info.flags & 0x20, 0, "{name} sets no bound box");
        let mut collidable = 0;
        for slot in &bound.intact_slots {
            let raw = nodes.get(*slot).expect("an intact slot");
            let info = &raw.info;
            assert_eq!(info.flags & 0x20, 0, "{} sets flag 0x20", raw.name);
            if info.mesh_index < 0 || info.flags & 0x14 != 0x14 {
                continue;
            }
            collidable += 1;
            // The soup lies inside the node's stored bound.
            let mesh = meshes.get(info.mesh_index as u32).expect("the leaf's mesh");
            let bound_box = [info.unk116, info.unk140, info.unk164]
                .into_iter()
                .find(|b| b.iter().flatten().any(|v| *v != 0.0))
                .expect("a collidable leaf stores a bound");
            for vertex in &mesh.mesh.positions {
                for axis in 0..3 {
                    assert!(
                        vertex[axis] >= bound_box[0][axis] - 1e-3
                            && vertex[axis] <= bound_box[1][axis] + 1e-3,
                        "{} soup vertex outside its bound",
                        raw.name
                    );
                }
            }
        }
        assert!(collidable > 0, "{name} has collidable leaf parts");

        let (centre, half) = bound.contact_box_m.expect("a leaf contact box");
        // The leaf union lies inside the grouping node's box, strictly
        // smaller on at least one axis.
        for axis in 0..3 {
            assert!(half[axis] <= bound.target_half_extents_m[axis] + 1e-6);
            assert!(
                (centre[axis] - bound.target_point_m[axis]).abs()
                    <= bound.target_half_extents_m[axis] - half[axis] + 1e-6
            );
        }
        assert!(
            (0..3).any(|axis| half[axis] < bound.target_half_extents_m[axis] - 1e-3),
            "{name}: the contact box is narrower than the grouping box"
        );
        eprintln!(
            "PART-VOLUME {name}: group centre {:?} half {:?}; leaf union centre {centre:?} half {half:?}",
            bound.target_point_m, bound.target_half_extents_m
        );
    }

    // Quantify `time_of_impact` on t_truck01: a 40 m tick (wep_12, 1200 m/s at
    // 30 Hz) flying along each world axis at the target point's other axes.
    let truck = target(&binding.targets, "t_truck01");
    let old = (truck.target_point_m, truck.target_half_extents_m);
    let new = truck.contact_box_m.expect("a leaf contact box");
    for axis in 0..3 {
        for sign in [-1.0, 1.0] {
            let mut from = new.0;
            let mut to = new.0;
            from[axis] -= sign * 20.0;
            to[axis] += sign * 20.0;
            let t_old = entry_time(from, to, old).expect("the old box is entered");
            let t_new = entry_time(from, to, new).expect("the new box is entered");
            assert!(t_new >= t_old - 1e-6);
            eprintln!(
                "PART-VOLUME t_truck01 axis {axis} sign {sign:+}: t {t_old:.5} -> {t_new:.5} ({:.3} m)",
                (t_new - t_old) * 40.0
            );
        }
    }
}
