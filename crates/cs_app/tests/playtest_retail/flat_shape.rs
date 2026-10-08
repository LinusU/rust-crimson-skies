//! #795 (`PLAYTEST-AREA-FLAT-SHAPE`): the large flat grey shape that stuck out
//! of the retail playtest airship is the landing card `sphere` (node slot 3063,
//! mesh 786) under `pz_auto_land` and its sibling `half_cone` (node slot 3066,
//! mesh 787) under `pz_manual_land`: each a single planar triangle drawn with
//! flat-colour material 84, whose original render class is unmeasured, so the
//! selection hides both from drawing and collision, provisional. Task test
//! prefix: `accept_playtest_area_flat_shape_`.
//!
//! Feature sheets: `specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`
//! (`### F11-B`), `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`.
//!
//! The retail tests are `#[ignore]`d (`requires CS_GAME_DIR`) and **fail** without
//! the variable; frames go only under `private/`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use cs_app::playtest_retail::{
    PlaytestCameraView, PlaytestConfig, PlaytestScene, area_graph, capture_visibility_delta,
    playtest_adapter, playtest_app, read_playtest_sources, spawn_playtest_scene,
};

/// The two landing cards, pinned by their measured stored identity.
const CARD_SPHERE_SLOT: u32 = 3063;
const CARD_HALF_CONE_SLOT: u32 = 3066;

fn install_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the original installation"),
    )
}

fn private_dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(|root| root.join("private/evidence/PLAYTEST-AREA-FLAT-SHAPE"))
        .expect("the workspace root is two levels above the crate");
    std::fs::create_dir_all(&dir).expect("private/evidence is writable");
    dir
}

/// One binding of the area that carries the measured flat-card attributes, as
/// the test re-reads them from the production readers.
#[derive(Debug)]
struct FlatCard {
    slot: u32,
    name: String,
    parent: String,
    mesh_index: u32,
    materials: Vec<u32>,
}

/// Walks every mesh binding of the area through the production readers and
/// returns the ones whose **stored attributes** make them flat-colour cards:
/// one stored polygon, planar positions, and material groups that all resolve
/// to flat-colour (untextured) records.
fn measured_flat_cards(sources: &cs_app::playtest_retail::PlaytestSources) -> Vec<FlatCard> {
    let adapter = playtest_adapter().expect("the adapter builds");
    let (graph, root) =
        area_graph(sources.world(), PLAYTEST_AREA_SLOT, &adapter).expect("the area graph builds");
    let mut found = Vec::new();
    for node in graph.subtree(&root) {
        let Some(binding) = node.mesh() else {
            continue;
        };
        let Some(mesh) = sources.world().meshes().get(binding.index) else {
            continue;
        };
        if mesh.mesh.polygons.len() != 1 || mesh.mesh.positions.len() < 3 {
            continue;
        }
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for position in &mesh.mesh.positions {
            for axis in 0..3 {
                lo[axis] = lo[axis].min(position[axis]);
                hi[axis] = hi[axis].max(position[axis]);
            }
        }
        let extent = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        if !extent.iter().any(|side| *side == 0.0) {
            continue;
        }
        let mut materials = BTreeSet::new();
        for groups in &mesh.material_groups {
            for group in groups {
                materials.insert(group.material);
            }
        }
        if materials.is_empty()
            || !materials.iter().all(|index| {
                sources
                    .world()
                    .materials()
                    .material(*index)
                    .is_some_and(|record| !sources.world().materials().names_a_texture(record))
            })
        {
            continue;
        }
        let parent = node
            .parent()
            .and_then(|id| graph.node(id))
            .map(|parent| parent.name().to_owned())
            .unwrap_or_default();
        found.push(FlatCard {
            slot: node.index(),
            name: node.name().to_owned(),
            parent,
            mesh_index: binding.index,
            materials: materials.into_iter().collect(),
        });
    }
    found
}

// The pinned slot the test walks, spelled once for the helper above.
const PLAYTEST_AREA_SLOT: u32 = cs_app::playtest_retail::PLAYTEST_AREA_NODE_SLOT;

/// **The two landing cards are identified by their stored attributes and hidden
/// with their reason; the collider count still equals the drawn binding count.**
///
/// The measurement: over every one of the area's 401 stored bindings, exactly
/// two carry the flat-card class (one planar polygon, every material group
/// flat-colour) — `sphere` under `pz_auto_land` and `half_cone` under
/// `pz_manual_land`. The production selection hides both with the reason that
/// names their class, so they reach neither the drawn records nor the
/// colliders; with the rule off (main's behaviour at `1c50a74e`) both are drawn
/// and this test's hidden-set assertions fail.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_area_flat_shape_retail_the_landing_cards_are_identified_and_hidden() {
    let config = PlaytestConfig::documented();
    let sources =
        read_playtest_sources(&install_dir(), &config.world_group).expect("the installation reads");

    // 1. the stored attributes, re-read from the production readers.
    let cards = measured_flat_cards(&sources);
    let pinned: BTreeMap<u32, &FlatCard> = cards.iter().map(|card| (card.slot, card)).collect();
    assert_eq!(
        pinned.len(),
        2,
        "measured: exactly the two landing cards carry the class, not more: {cards:?}"
    );
    let sphere = pinned
        .get(&CARD_SPHERE_SLOT)
        .expect("the auto-landing card is measured");
    assert_eq!(sphere.name, "sphere");
    assert_eq!(sphere.parent, "pz_auto_land");
    assert_eq!(sphere.mesh_index, 786);
    let half_cone = pinned
        .get(&CARD_HALF_CONE_SLOT)
        .expect("the manual-landing card is measured");
    assert_eq!(half_cone.name, "half_cone");
    assert_eq!(half_cone.parent, "pz_manual_land");
    assert_eq!(half_cone.mesh_index, 787);
    for card in &cards {
        assert!(
            card.materials.iter().all(|index| *index == 84),
            "{} draws flat-colour material(s) {card:?}",
            card.name
        );
    }

    // 2. the production selection hides exactly those, with their reason.
    let mut app = playtest_app();
    let scene = spawn_playtest_scene(&mut app, &sources, &config).expect("the scene spawns");
    let area = scene.area();
    let undrawn: BTreeMap<u32, &cs_app::playtest_retail::UndrawnBinding> = area
        .undrawn
        .iter()
        .map(|hidden| (hidden.node_slot, hidden))
        .collect();
    for card in &cards {
        let hidden = undrawn
            .get(&card.slot)
            .unwrap_or_else(|| panic!("{} (slot {}) must be hidden", card.name, card.slot));
        assert_eq!(hidden.node_name, card.name);
        assert_eq!(hidden.mesh_index, card.mesh_index);
        assert!(
            hidden.reason.contains("flat-colour card"),
            "{}'s reason names its class: {}",
            card.name,
            hidden.reason
        );
        assert!(
            hidden.reason.contains(&card.parent),
            "{}'s reason names its parent: {}",
            card.name,
            hidden.reason
        );
        assert!(
            hidden.reason.contains("unmeasured") && hidden.reason.contains("provisional"),
            "{}'s reason labels the treatment provisional: {}",
            card.name,
            hidden.reason
        );
    }
    let drawn_ids: BTreeSet<&str> = scene
        .definition()
        .objects()
        .iter()
        .map(|object| object.id().as_str())
        .collect();
    for card in &cards {
        assert!(
            !drawn_ids.contains(&*format!("playtest.node-{}", card.slot)),
            "{} is not among the drawn records",
            card.name
        );
    }

    // 3. colliders still follow the drawn set: two cards fewer than main's 295.
    assert_eq!(area.stored_bindings, 401);
    assert_eq!(
        area.mesh_records, 293,
        "main drew 295; the two landing cards are now listed undrawn"
    );
    assert_eq!(area.mesh_records + area.undrawn.len(), area.stored_bindings);
    assert_eq!(area.colliders(), area.mesh_records);
    assert_eq!(scene.definition().objects().len(), area.mesh_records);
    assert_eq!(scene.spawned().colliders().len(), area.mesh_records);

    // 4. the baseline: with the rule off — main's behaviour — both cards are
    //    drawn, which is exactly what this task took out of the frame.
    let baseline_config = PlaytestConfig {
        hide_flat_colour_cards: false,
        ..PlaytestConfig::documented()
    };
    let mut baseline_app = playtest_app();
    let baseline =
        spawn_playtest_scene(&mut baseline_app, &sources, &baseline_config).expect("it spawns");
    let baseline_undrawn: BTreeSet<u32> = baseline
        .area()
        .undrawn
        .iter()
        .map(|hidden| hidden.node_slot)
        .collect();
    assert_eq!(
        baseline.area().mesh_records,
        295,
        "without the rule the drawn set is main's own"
    );
    for card in &cards {
        assert!(
            !baseline_undrawn.contains(&card.slot),
            "without the rule {} is drawn, as on main",
            card.name
        );
    }
}

/// **A real GPU capture frames that end of the airship before and after: the
/// card footprint is gone once the cards are hidden.**
///
/// The scene is spawned with the rule off (main's behaviour: both cards drawn),
/// the two card entities are found through the production spawn report, and one
/// view of the airship's underside-aft end — derived from the cards' own
/// composed geometry, not from a constant — is rendered twice on the real GPU:
/// once with the cards shown ("before") and once with them hidden ("after",
/// the state the fixed selection produces). Both PNGs stay under `private/`;
/// only their hashes and the measured pixel counts are reported.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_area_flat_shape_gpu_the_card_footprint_is_gone_when_they_are_hidden() {
    let out = private_dir();
    let config = PlaytestConfig {
        hide_flat_colour_cards: false,
        ..PlaytestConfig::documented()
    };
    let sources =
        read_playtest_sources(&install_dir(), &config.world_group).expect("the installation reads");
    let mut app = playtest_app();
    let scene = spawn_playtest_scene(&mut app, &sources, &config).expect("the scene spawns");

    // The two card entities, by their production object identities.
    let wanted = [
        format!("playtest.node-{CARD_SPHERE_SLOT}"),
        format!("playtest.node-{CARD_HALF_CONE_SLOT}"),
    ];
    let mut entities = Vec::new();
    for (id, entity) in scene.spawned().visuals() {
        if wanted.contains(&id.as_str().to_owned()) {
            entities.push(entity);
        }
    }
    assert_eq!(entities.len(), 2, "both cards were spawned");

    // The view: off the cards' own plane, far enough to frame the 500 m card.
    let eye_target = card_view(&scene, &sources);
    let name: &'static str = Box::leak("flat-shape-underside-aft".to_owned().into_boxed_str());
    let view = PlaytestCameraView { name, ..eye_target };
    let delta = capture_visibility_delta(&mut app, &scene, &view, &entities, &out)
        .expect("the airship end renders");
    eprintln!("FLAT-SHAPE CAPTURE {delta:?}");
    assert!(
        delta.shown_covered_pixels > 5_000,
        "the before frame drew the airship end: {delta:?}"
    );
    assert!(
        delta.changed_pixels >= 2_000,
        "the two cards are roughly a third of the hull long, so hiding them must change a \
         large region: {delta:?}"
    );
    assert!(
        delta.shown_covered_pixels > delta.hidden_covered_pixels,
        "the cards also covered sky beyond the hull: {delta:?}"
    );
    assert_ne!(
        delta.shown_png_sha256, delta.hidden_png_sha256,
        "the before and after frames are distinct artifacts"
    );
    assert!(!delta.shown_png.is_empty() && !delta.hidden_png.is_empty());
}

/// One view of the airship's underside-aft end, derived from the two cards'
/// composed geometry: the eye sits off the cards' own plane, at a distance the
/// 500 m card fits into, looking at the centre of the two cards' joint extent.
fn card_view(
    scene: &PlaytestScene,
    sources: &cs_app::playtest_retail::PlaytestSources,
) -> PlaytestCameraView {
    let mut min = [f64::MAX; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    let mut plane_normal = None;
    // The meshes the two cards bind, pinned by the measurement in the test above.
    let mesh_of = |slot: u32| if slot == CARD_SPHERE_SLOT { 786 } else { 787 };
    for object in scene.definition().objects() {
        let Some(slot) = object
            .id()
            .as_str()
            .strip_prefix("playtest.node-")
            .and_then(|slot| slot.parse::<u32>().ok())
        else {
            continue;
        };
        if slot != CARD_SPHERE_SLOT && slot != CARD_HALF_CONE_SLOT {
            continue;
        }
        let Some(mesh) = sources.world().meshes().get(mesh_of(slot)) else {
            continue;
        };
        let corners: Vec<[f64; 3]> = mesh
            .mesh
            .positions
            .iter()
            .map(|position| {
                let local = object.transform().transform_vector([
                    f64::from(position[0]),
                    f64::from(position[1]),
                    f64::from(position[2]),
                ]);
                let origin = object.transform().translation();
                [
                    local[0] + origin[0],
                    local[1] + origin[1],
                    local[2] + origin[2],
                ]
            })
            .collect();
        if slot == CARD_SPHERE_SLOT && corners.len() >= 3 {
            let a = corners[0];
            let u = [
                corners[1][0] - a[0],
                corners[1][1] - a[1],
                corners[1][2] - a[2],
            ];
            let v = [
                corners[2][0] - a[0],
                corners[2][1] - a[1],
                corners[2][2] - a[2],
            ];
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if length > 0.0 {
                plane_normal = Some([n[0] / length, n[1] / length, n[2] / length]);
            }
        }
        for corner in &corners {
            for axis in 0..3 {
                min[axis] = min[axis].min(corner[axis]);
                max[axis] = max[axis].max(corner[axis]);
            }
        }
    }
    let centre = [
        (min[0] + max[0]) / 2.0,
        (min[1] + max[1]) / 2.0,
        (min[2] + max[2]) / 2.0,
    ];
    let normal = plane_normal.expect("the sphere card is planar");
    // 500 m out along the plane's normal frames the 500 m card at the scene's
    // documented field of view; a slight lift keeps the hull end in frame.
    let distance = 500.0;
    PlaytestCameraView {
        name: "",
        eye: [
            (centre[0] + normal[0] * distance) as f32,
            (centre[1] + normal[1] * distance - 40.0) as f32,
            (centre[2] + normal[2] * distance) as f32,
        ],
        target: std::array::from_fn(|axis| centre[axis] as f32),
    }
}

/// **`docs/PLAYTEST.md` documents the treatment, provisional.**
#[test]
fn accept_playtest_area_flat_shape_docs_describe_the_treatment_as_provisional() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for file in ["docs/PLAYTEST.md", "docs/PLAYTEST-RETAIL.md"] {
        let text = std::fs::read_to_string(root.join(file)).expect("the doc exists");
        assert!(text.contains("#795"), "{file} does not name task #795");
        assert!(
            text.contains("flat-colour card"),
            "{file} does not describe the flat-colour card rule"
        );
        assert!(
            text.contains("pz_auto_land"),
            "{file} does not name the landing cards"
        );
        assert!(
            text.contains("provisional"),
            "{file} does not label the treatment provisional"
        );
    }
}
