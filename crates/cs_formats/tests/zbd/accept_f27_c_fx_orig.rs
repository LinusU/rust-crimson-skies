//! Acceptance for task #1684 (`F27-C-FX-ORIG`): the original's muzzle-effect
//! binding, measured from the retail installation
//! (`docs/findings/2026-10-24-f27-c-fx-orig-muzzle-effect-binding.md`).
//!
//! Every test reads `$CS_GAME_DIR` (never writes it) and fails loudly without
//! it. Task test prefix: `accept_f27_c_fx_orig_`.

use std::path::Path;

use cs_formats::ParseContext;
use cs_formats::gamez::{read_gamez_materials, read_gamez_meshes, read_gamez_nodes};
use cs_formats::zbd::{EffectEvent, ReaderNode, read_animation_definitions, read_reader_nodes};

use super::accept_parity2_dmg_fx_a::with_zrdr;
use super::t340::game_dir;

const GAMEZ: [&str; 8] = [
    "ZBD/C1/gamez.zbd",
    "ZBD/C1B/gamez.zbd",
    "ZBD/C1C/gamez.zbd",
    "ZBD/C2/gamez.zbd",
    "ZBD/C2B/gamez.zbd",
    "ZBD/C3/gamez.zbd",
    "ZBD/C4/gamez.zbd",
    "ZBD/C5/gamez.zbd",
];

/// `(weapon record key, FIRE ANIMATION)` for every `BALLISTICS` record of
/// `weapons.zrd` whose `FIRE` block names an animation.
fn fire_animations() -> Vec<(String, String)> {
    with_zrdr(|members| {
        let (_, bytes) = members
            .iter()
            .find(|(name, _)| name == "weapons.zrd")
            .expect("zrdr.zbd holds weapons.zrd");
        let nodes = read_reader_nodes(bytes).expect("weapons.zrd decodes");
        let mut out = Vec::new();
        for root in &nodes {
            let Some(document) = root.as_list().and_then(<[ReaderNode<'_>]>::first) else {
                continue;
            };
            let Some(ballistics) = document.field("BALLISTICS") else {
                continue;
            };
            for (key, record) in ballistics.fields().expect("a record list") {
                let Some(fire) = record.field("FIRE") else {
                    continue;
                };
                let Some(animation) = fire.field("ANIMATION") else {
                    continue;
                };
                let name = animation
                    .as_list()
                    .and_then(|children| children.first())
                    .and_then(ReaderNode::as_str);
                if let (Some(key), Some(name)) = (key, name) {
                    out.push((key.to_owned(), name.to_owned()));
                }
            }
        }
        out
    })
}

/// The gun kinds `weapons.zrd` binds to one `muzzle_burst*` animation each, and
/// the textures their two billboard nodes use.
const GUN_BURSTS: [(&str, &str, &str); 4] = [
    ("muzzle_burst_slug", "slug_muzzle1.tif", "slug_muzzle2.tif"),
    ("muzzle_burst_dum", "dum_muzzle1.tif", "dum_muzzle2.tif"),
    ("muzzle_burst_ap", "ap_muzzle1.tif", "ap_muzzle2.tif"),
    ("muzzle_burst_mag", "mag_muzzle1.tif", "mag_muzzle2.tif"),
];

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_c_fx_orig_weapon_rows_bind_a_fire_animation_by_name() {
    let fire = fire_animations();
    let of = |key: &str| {
        fire.iter()
            .find(|(k, _)| k == key)
            .map(|(_, animation)| animation.as_str())
    };
    // The M01 selected gun (`wep_50`, `50slug`) and the 30/40-calibre guns.
    assert_eq!(of("wep_50"), Some("muzzle_burst_slug"));
    assert_eq!(of("wep_00"), Some("muzzle_burst_slug"));
    assert_eq!(of("wep_51"), Some("muzzle_burst_dum"));
    assert_eq!(of("wep_52"), Some("muzzle_burst_ap"));
    assert_eq!(of("wep_53"), Some("muzzle_burst_mag"));
    assert_eq!(of("wep_23"), Some("muzzle_burst2"));
    assert_eq!(fire.len(), 35, "35 weapon records name a FIRE ANIMATION");
    // Every `muzzle_burst*` binding resolves to exactly one definition of
    // `muzzle_burst.zrd`.
    with_zrdr(|members| {
        let (_, bytes) = members
            .iter()
            .find(|(name, _)| name == "muzzle_burst.zrd")
            .expect("zrdr.zbd holds muzzle_burst.zrd");
        let nodes = read_reader_nodes(bytes).expect("muzzle_burst.zrd decodes");
        let defs = read_animation_definitions(&nodes).expect("animation definitions");
        let mut bound = 0;
        for (_, animation) in fire.iter().filter(|(_, a)| a.starts_with("muzzle_burst")) {
            let hits = defs.iter().filter(|d| d.name == animation).count();
            assert_eq!(hits, 1, "{animation} is defined exactly once");
            bound += 1;
        }
        assert_eq!(bound, 31);
    });
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_c_fx_orig_slug_burst_definition_is_one_short_billboard_pair() {
    with_zrdr(|members| {
        let (_, bytes) = members
            .iter()
            .find(|(name, _)| name == "muzzle_burst.zrd")
            .expect("zrdr.zbd holds muzzle_burst.zrd");
        let nodes = read_reader_nodes(bytes).expect("decodes");
        let defs = read_animation_definitions(&nodes).expect("definitions");
        let slug = defs.iter().find(|d| d.name == "muzzle_burst_slug").unwrap();
        // The node is called at its own offset (0, -0.2, -1) for the shared
        // `muzzleburst_effects` animation.
        let calls: Vec<_> = slug.calls().collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "muzzleburst_effects");
        let at = calls[0].at_node.as_ref().expect("AT_NODE");
        assert_eq!(at.node, "muzzle_burst_slug");
        assert_eq!(at.words, vec![0.0, -0.2, -1.0]);
        // Shown at scale 1 and switched off 0.001 s (EVENT_OFFSET) later.
        let other = |keyword: &str| -> Vec<&ReaderNode<'_>> {
            slug.sequences
                .iter()
                .flat_map(|s| s.events.iter())
                .filter_map(|e| match e {
                    EffectEvent::Other { keyword: k, value } if *k == keyword => Some(value),
                    _ => None,
                })
                .collect()
        };
        let scales = other("OBJECT_SCALE_STATE");
        assert_eq!(scales.len(), 1);
        let state = |node: &ReaderNode<'_>| -> Vec<f32> {
            node.field("STATE")
                .and_then(ReaderNode::as_list)
                .unwrap()
                .iter()
                .filter_map(ReaderNode::as_number)
                .collect()
        };
        assert_eq!(state(scales[0]), vec![1.0, 1.0, 1.0]);
        let actives = other("OBJECT_ACTIVE_STATE");
        assert_eq!(actives.len(), 2);
        let off = actives[1]
            .field("START_TIME")
            .and_then(ReaderNode::as_list)
            .unwrap();
        assert_eq!(off[0].as_str(), Some("EVENT_OFFSET"));
        assert_eq!(off[1].as_number(), Some(0.001));
        // `mb_spinflame` is turned to one of three roll angles, 30/80/140.
        let spins: Vec<Vec<f32>> = other("OBJECT_ROTATE_STATE")
            .into_iter()
            .map(state)
            .collect();
        assert_eq!(
            spins,
            vec![
                vec![0.0, 0.0, 30.0],
                vec![0.0, 0.0, 80.0],
                vec![0.0, 0.0, 140.0]
            ]
        );
        // The shared `muzzleburst_effects` animation: a 0.05 s smoke puffer
        // (0.1-0.2 s lifetime, size 0.3-0.6) and the lights.
        let shared = defs
            .iter()
            .find(|d| d.id() == "muzzleburst_effects")
            .expect("the shared effects animation");
        let puffers: Vec<_> = shared.puffers().collect();
        assert_eq!(puffers[0].name, "muzzlepuffer");
        assert_eq!(puffers[0].time_interval, Some(0.05));
        assert_eq!(puffers[0].size_range, Some([0.3, 0.6]));
        assert_eq!(puffers[0].lifetime_range, Some([0.1, 0.2]));
        assert_eq!(puffers[0].local_velocity, Some([0.0, 0.0, -20.0]));
        assert_eq!(
            puffers[0].textures,
            vec!["smoke101", "smoke102", "smoke103"]
        );
        assert_eq!(puffers[1].start_time, Some(("EVENT_OFFSET", 0.3)));
        assert_eq!(shared.lights().count(), 6);
    });
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_c_fx_orig_burst_nodes_are_in_every_gamez_container() {
    for spelling in GAMEZ {
        let bytes = std::fs::read(Path::new(&game_dir()).join(spelling)).expect("reads");
        let nodes = read_gamez_nodes(&mut ParseContext::with_defaults(spelling), &bytes).unwrap();
        let meshes =
            read_gamez_meshes(&mut ParseContext::with_defaults(spelling), spelling, &bytes)
                .unwrap();
        let mats =
            read_gamez_materials(&mut ParseContext::with_defaults(spelling), spelling, &bytes)
                .unwrap();
        for (name, first, spin) in GUN_BURSTS {
            let burst = nodes
                .nodes
                .iter()
                .find(|n| n.name == name)
                .unwrap_or_else(|| panic!("{spelling}: node {name}"));
            assert_eq!(burst.children.len(), 2, "{spelling} {name}");
            let child = |label: &str| {
                let node = burst
                    .children
                    .iter()
                    .map(|c| nodes.get(*c).unwrap())
                    .find(|n| n.name == label)
                    .unwrap_or_else(|| panic!("{spelling} {name}/{label}"));
                let mesh = meshes
                    .get(u32::try_from(node.info.mesh_index).expect("a mesh"))
                    .expect("the mesh is present");
                let textures: Vec<String> = mesh
                    .mesh
                    .polygons
                    .iter()
                    .map(|p| {
                        let material = mats.material(p.material).unwrap();
                        mats.texture_of(material).unwrap().name.clone()
                    })
                    .collect();
                (mesh.mesh.positions.clone(), textures)
            };
            // `mb1`: one 0.34 m wide quad in the node's XZ plane, z from
            // -0.195 to 0.4.
            let (quad, quad_textures) = child("mb1");
            assert_eq!(quad.len(), 4, "{spelling} {name}/mb1");
            assert_eq!(quad_textures, vec![first.to_owned()]);
            let z = quad.iter().map(|p| p[2]);
            assert_eq!(z.clone().fold(f32::MAX, f32::min), -0.195_000_02);
            assert_eq!(z.fold(f32::MIN, f32::max), 0.4);
            assert!(quad.iter().all(|p| p[1] == 0.0 && p[0].abs() == 0.17));
            // `mb_spinflame`: three quads rotated 60 degrees apart about +Z.
            let (star, star_textures) = child("mb_spinflame");
            assert_eq!(star.len(), 12, "{spelling} {name}/mb_spinflame");
            assert_eq!(star_textures, vec![spin.to_owned(); 3]);
        }
    }
}
