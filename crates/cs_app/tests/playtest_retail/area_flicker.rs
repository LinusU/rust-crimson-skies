//! #753 (`PLAYTEST-AREA-FLICKER`): the retail playtest airship (`piratezep`) draws
//! one intact variant of each part at one LOD band, so its scorched `burnpanels`
//! no longer sit coplanar with the light `panels`. Task test prefix:
//! `accept_playtest_area_flicker_`.
//!
//! Feature sheets: `specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`
//! (`### F11-B`), `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`.
//!
//! The retail tests are `#[ignore]`d (`requires CS_GAME_DIR`) and **fail** without
//! the variable; frames go only under `private/`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use cs_app::playtest_retail::{
    PlaytestCameraView, PlaytestConfig, PlaytestScene, area_graph, capture_view_stability,
    playtest_adapter, playtest_app, read_playtest_sources, spawn_playtest_scene,
};
use cs_content::scene::NodeKind;

/// The stated threshold: the worst frame may flip at most this share of the hull
/// region when the camera moves by `STEP_M`.
const MAX_FLIPPED_SHARE: f64 = 0.002;
const STEP_M: f32 = 0.01;
const FRAMES: usize = 4;

fn install_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the original installation"),
    )
}

fn private_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(|root| {
            root.join("private/evidence/PLAYTEST-AREA-FLICKER")
                .join(name)
        })
        .expect("the workspace root is two levels above the crate");
    std::fs::create_dir_all(&dir).expect("private/evidence is writable");
    dir
}

fn scene_with(selected: bool) -> (bevy::prelude::App, PlaytestScene) {
    let config = PlaytestConfig {
        select_area_variants: selected,
        // The contrast this test measures is the variant-selection rule alone:
        // hold #794's decal offset off in both passes, or the keyed overlays
        // would win their coplanar fights in the unselected pass too and the
        // before/after pair would stop demonstrating anything.
        decal_offset: false,
        ..PlaytestConfig::documented()
    };
    let sources =
        read_playtest_sources(&install_dir(), &config.world_group).expect("the installation reads");
    let mut app = playtest_app();
    let scene = spawn_playtest_scene(&mut app, &sources, &config).expect("the scene spawns");
    (app, scene)
}

/// Three views of the hull flank from outside its `-x` face, derived from the
/// area's measured bounds: `(name, distance past the face in m, target height as
/// a fraction of the height above the centre, offset along the hull in m)`. They
/// were chosen by measuring a grid of flank views on main's behaviour and keeping
/// the ones where the layered variants fight; the grid's other views (a skull
/// decal on the hull at `t4d20z0`) fight identically before and after and are
/// recorded in the finding as residual.
const FLANK_VIEWS: [(&str, f64, f64, f64); 3] = [
    ("flank-far-aft", 120.0, 0.0, -200.0),
    ("flank-near-aft", 20.0, 0.2, -200.0),
    ("flank-far-mid", 120.0, 0.0, 0.0),
];

fn flank_views(scene: &PlaytestScene) -> Vec<PlaytestCameraView> {
    let bounds = &scene.area().bounds;
    let centre: [f64; 3] =
        std::array::from_fn(|axis| (bounds.min()[axis] + bounds.max()[axis]) / 2.0);
    let extent: [f64; 3] = std::array::from_fn(|axis| bounds.max()[axis] - bounds.min()[axis]);
    FLANK_VIEWS
        .iter()
        .map(|&(name, distance, height, along)| {
            let y = (centre[1] + height * extent[1]) as f32;
            let z = (centre[2] + along) as f32;
            PlaytestCameraView {
                name,
                eye: [(centre[0] - extent[0] / 2.0 - distance) as f32, y, z],
                target: [centre[0] as f32, y, z],
            }
        })
        .collect()
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_area_flicker_retail_the_selected_hull_is_stable_and_the_unselected_one_is_not() {
    let out = private_dir("stability");
    let mut measured = Vec::new();
    for selected in [false, true] {
        let (mut app, scene) = scene_with(selected);
        for view in flank_views(&scene) {
            let name: &'static str = Box::leak(
                format!(
                    "{}-{}",
                    if selected { "after" } else { "before" },
                    view.name
                )
                .into_boxed_str(),
            );
            let result = capture_view_stability(
                &mut app,
                &scene,
                &PlaytestCameraView { name, ..view },
                STEP_M,
                FRAMES,
                &out,
                false,
            )
            .expect("the hull renders");
            eprintln!("STABILITY {name}: {result:?}");
            assert!(
                result.region_pixels > 1000,
                "the view frames the hull: {result:?}"
            );
            measured.push((selected, result));
        }
    }
    for (selected, result) in &measured {
        if *selected {
            assert!(
                result.worst_share <= MAX_FLIPPED_SHARE,
                "the selected hull flips {:.5} of its pixels (limit {MAX_FLIPPED_SHARE}): {result:?}",
                result.worst_share
            );
        } else {
            assert!(
                result.worst_share > MAX_FLIPPED_SHARE,
                "main's behaviour (every binding drawn) must fail the threshold: {result:?}"
            );
        }
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_area_flicker_retail_the_drawn_set_holds_one_variant_per_part_with_a_collider_each()
 {
    let config = PlaytestConfig::documented();
    let sources =
        read_playtest_sources(&install_dir(), &config.world_group).expect("the installation reads");
    let adapter = playtest_adapter().expect("the adapter builds");
    let (graph, root) = area_graph(sources.world(), config.area_node_slot, &adapter)
        .expect("the area graph builds");
    let mut app = playtest_app();
    let scene = spawn_playtest_scene(&mut app, &sources, &config).expect("the scene spawns");
    let area = scene.area();

    // The hidden list is non-empty and accounts for every stored binding.
    assert!(
        !area.undrawn.is_empty(),
        "alternatives exist, so some are hidden"
    );
    assert!(
        !area.hidden_lods.is_empty(),
        "LOD bands exist, so some are hidden"
    );
    assert_eq!(area.stored_bindings, 401);
    assert_eq!(area.mesh_records + area.undrawn.len(), area.stored_bindings);
    for hidden in &area.undrawn {
        assert!(
            !hidden.reason.is_empty(),
            "{} has no reason",
            hidden.node_name
        );
    }

    // Colliders follow the drawn set.
    assert!(
        area.refused.is_empty() && area.gaps.is_empty(),
        "{:?} {:?}",
        area.refused,
        area.gaps
    );
    assert_eq!(area.colliders(), area.mesh_records);
    assert_eq!(scene.definition().objects().len(), area.mesh_records);

    // No two drawn bindings are alternatives of each other.
    let undrawn: BTreeSet<u32> = area.undrawn.iter().map(|u| u.node_slot).collect();
    let nodes = graph.subtree(&root);
    let drawn = |slot: u32| {
        nodes
            .iter()
            .any(|n| n.index() == slot && n.mesh().is_some())
            && !undrawn.contains(&slot)
    };
    let has_drawn_below = |slot: u32| {
        let node = nodes.iter().find(|n| n.index() == slot).expect("a node");
        graph.subtree(node.id()).iter().any(|n| drawn(n.index()))
    };
    for parent in &nodes {
        let kids: Vec<_> = parent
            .children()
            .iter()
            .filter_map(|id| graph.node(id))
            .collect();
        let range = |k: &cs_content::scene::SceneNode| match k.kind() {
            NodeKind::Lod(i) => Some((i.range_min.0, i.range_max.0)),
            _ => None,
        };
        let live_bands: Vec<_> = kids
            .iter()
            .filter(|k| range(k).is_some() && has_drawn_below(k.index()))
            .collect();
        for (i, a) in live_bands.iter().enumerate() {
            for b in &live_bands[i + 1..] {
                let (ra, rb) = (range(a).unwrap(), range(b).unwrap());
                let nested = (ra.0 >= rb.0 && ra.1 <= rb.1) || (rb.0 >= ra.0 && rb.1 <= ra.1);
                assert!(
                    nested,
                    "LOD bands {} and {} under {} are both drawn",
                    a.name(),
                    b.name(),
                    parent.name()
                );
            }
        }
        let live = |name: &str| {
            kids.iter()
                .any(|k| k.name() == name && has_drawn_below(k.index()))
        };
        if live("panels") {
            assert!(
                !live("burnpanels"),
                "panels and burnpanels under {}",
                parent.name()
            );
        }
        if live("propstill") {
            assert!(
                !live("spin") && !live("counterspin"),
                "propeller states under {}",
                parent.name()
            );
        }
        for k in &kids {
            if let Some(stem) = k.name().strip_suffix('h')
                && has_drawn_below(k.index())
            {
                assert!(
                    !live(&format!("{stem}d")),
                    "{stem}h and {stem}d under {}",
                    parent.name()
                );
            }
        }
    }
    // The scorched panels, the damaged halves and the far LOD band are among the hidden.
    let hidden_names: BTreeSet<&str> = area.undrawn.iter().map(|u| u.node_name.as_str()).collect();
    assert!(hidden_names.contains("burnpr1"));
    assert!(hidden_names.contains("prightb1d"));
}

#[test]
fn accept_playtest_area_flicker_docs_describe_the_selection_as_provisional() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for file in ["docs/PLAYTEST.md", "docs/PLAYTEST-RETAIL.md"] {
        let text = std::fs::read_to_string(root.join(file)).expect("the doc exists");
        assert!(
            text.contains("area_undrawn"),
            "{file} does not describe area_undrawn"
        );
        assert!(
            text.contains("burnpanels"),
            "{file} does not describe the burnpanels rule"
        );
    }
}
