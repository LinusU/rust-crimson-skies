//! #794 (`PLAYTEST-DECAL-ZFIGHT`): a part whose bound texture carries keyed
//! coverage is a decal layer, drawn `DECAL_OFFSET_M` along its normals, so a
//! coplanar base surface can never win the depth test over it. Measured on the
//! `piratezep` hull emblem (`fhunter_logo2.tif`, mesh 750 material group 1) and
//! the Bloodhawk's wing insignia (`blo_winglogo.tif`). Task test prefix:
//! `accept_playtest_decal_`.
//!
//! The retail tests are `#[ignore]`d (`requires CS_GAME_DIR`) and **fail**
//! without the variable; frames go only under `private/`.

use std::path::{Path, PathBuf};

use bevy::asset::Assets;
use bevy::prelude::{AlphaMode, App, GlobalTransform, Handle, StandardMaterial};
use cs_app::playtest_retail::{
    AircraftPart, PlaytestCameraView, PlaytestConfig, PlaytestScene, PlaytestSources,
    capture_view_pixels, capture_view_stability, playtest_app, read_playtest_sources,
    spawn_playtest_scene,
};

/// The stated threshold: at most this share of a decal layer's interior pixels
/// may fall back to the base surface under a `STEP_M` move — the same bound
/// `accept_playtest_area_flicker_` holds the hull to.
const MAX_LOST_SHARE: f64 = 0.002;
const STEP_M: f32 = 0.01;
/// A visible decal draws hundreds of pixels in these views; the bound keeps
/// "the decals stay visible" from being passed by a layer that stopped drawing.
const MIN_DECAL_PIXELS: usize = 100;

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
            root.join("private/evidence/PLAYTEST-DECAL-ZFIGHT")
                .join(name)
        })
        .expect("the workspace root is two levels above the crate");
    std::fs::create_dir_all(&dir).expect("private/evidence is writable");
    dir
}

fn scene_with(decal_offset: bool) -> (App, PlaytestScene) {
    // The skull view is #753's `t4d20z0`, defined against the area's measured
    // extent as main drew it at `1c50a74e`: with #795's landing cards still in
    // it (their 500 m tail sets the z centre). Hiding them moves the centre
    // 130 m forward and the same recipe then frames a different stretch of
    // hull (measured: the emblem footprint halves and the coplanar fight does
    // not show), so this scene keeps the cards drawn — they are untextured
    // and carry no decal.
    let config = PlaytestConfig {
        decal_offset,
        hide_flat_colour_cards: false,
        ..PlaytestConfig::documented()
    };
    let sources =
        read_playtest_sources(&install_dir(), &config.world_group).expect("the installation reads");
    let mut app = playtest_app();
    let scene = spawn_playtest_scene(&mut app, &sources, &config).expect("the scene spawns");
    (app, scene)
}

/// Temporarily forces `handles` to discard every texel (a mask cutoff above
/// any stored alpha) and returns the alpha modes to restore. Hiding the layer
/// this way — not despawning it — is what lets a footprint measure "the pixels
/// this layer draws".
///
/// `handles` is deduplicated first: one bound material serves every decal
/// group it was stored for, so the report's per-group list names the same
/// handle several times. Saving a duplicate would record the already-hidden
/// mode and "restore" would hide it again — the layer would never come back.
fn hide_materials(
    app: &mut App,
    handles: &[Handle<StandardMaterial>],
) -> Vec<(Handle<StandardMaterial>, AlphaMode)> {
    let mut assets = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
    let mut saved = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for handle in handles {
        if !seen.insert(handle.id()) {
            continue;
        }
        if let Some(mut material) = assets.get_mut(handle) {
            saved.push((handle.clone(), material.alpha_mode));
            material.alpha_mode = AlphaMode::Mask(2.0);
        }
    }
    saved
}

fn restore_materials(app: &mut App, saved: Vec<(Handle<StandardMaterial>, AlphaMode)>) {
    let mut assets = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
    for (handle, mode) in saved {
        if let Some(mut material) = assets.get_mut(&handle) {
            material.alpha_mode = mode;
        }
    }
}

fn pixel_differs(a: &[u8], b: &[u8], i: usize) -> bool {
    (0..3).any(|c| a[i * 4 + c].abs_diff(b[i * 4 + c]) > 8)
}

/// What one view's target decal layers did across one `STEP_M` camera step.
#[derive(Debug)]
struct DecalStability {
    /// Pixels the target decals draw at the reference position.
    footprint: usize,
    /// Footprint pixels with four footprint neighbours — unreachable by
    /// coverage-edge motion, so only a depth winner change can alter them.
    interior: usize,
    /// Interior pixels whose colour moved at all.
    flips: usize,
    /// Interior pixels that ended on the base colour — the decal lost the
    /// depth test there. The z-fight signature.
    lost: usize,
}

impl DecalStability {
    fn lost_share(&self) -> f64 {
        self.lost as f64 / self.interior.max(1) as f64
    }
}

/// Compares the frame to itself with `handles` masked out, at both ends of a
/// `STEP_M` step: the decal's own pixels are the region, and a pixel that
/// falls back to the hidden colour on the moved frame is one the base surface
/// won back.
///
/// The shown frames are captured **first**, in the scene's unedited state:
/// the reference pixels a coplanar decal fought over are the baseline the
/// step and the hide are measured against, not a frame taken after material
/// surgery.
fn measure_decal(
    app: &mut App,
    scene: &PlaytestScene,
    view: &PlaytestCameraView,
    aircraft: bool,
    handles: &[Handle<StandardMaterial>],
) -> DecalStability {
    let mut moved = *view;
    moved.eye[1] += STEP_M;
    let s0 = capture_view_pixels(app, scene, view, aircraft, None).unwrap();
    let s1 = capture_view_pixels(app, scene, &moved, aircraft, None).unwrap();
    let saved = hide_materials(app, handles);
    for _ in 0..3 {
        app.update();
    }
    let h0 = capture_view_pixels(app, scene, view, aircraft, None).unwrap();
    let h1 = capture_view_pixels(app, scene, &moved, aircraft, None).unwrap();
    restore_materials(app, saved);
    for _ in 0..3 {
        app.update();
    }
    let (w, h) = (s0.width as usize, s0.height as usize);
    let footprint: Vec<bool> = (0..s0.pixels.len() / 4)
        .map(|i| pixel_differs(&s0.pixels, &h0.pixels, i))
        .collect();
    let interior = |i: usize| {
        let (x, y) = (i % w, i / w);
        x > 0
            && x + 1 < w
            && y > 0
            && y + 1 < h
            && [-1isize, 1, -(w as isize), w as isize]
                .iter()
                .all(|d| footprint[i.wrapping_add_signed(*d)])
    };
    let mut stability = DecalStability {
        footprint: 0,
        interior: 0,
        flips: 0,
        lost: 0,
    };
    for (i, inside) in footprint.iter().enumerate() {
        if !inside {
            continue;
        }
        stability.footprint += 1;
        if !interior(i) {
            continue;
        }
        stability.interior += 1;
        if pixel_differs(&s0.pixels, &s1.pixels, i) {
            stability.flips += 1;
            if !pixel_differs(&s1.pixels, &h1.pixels, i) {
                stability.lost += 1;
            }
        }
    }
    stability
}

/// The decal material handles of one subject whose resolved texture name
/// contains `needle`.
fn decal_handles(
    scene: &PlaytestScene,
    container_key: &str,
    needle: &str,
) -> Vec<Handle<StandardMaterial>> {
    scene
        .textures()
        .subject(container_key)
        .expect("the subject is reported")
        .decals
        .iter()
        .filter(|d| d.texture.contains(needle))
        .map(|d| d.material_handle.clone())
        .collect()
}

/// #753's `t4d20z0`: 20 m off the area's `-x` face, 0.4 of the height up,
/// mid-hull — the view the owner watched the skull emblem flicker in.
fn skull_view(scene: &PlaytestScene) -> PlaytestCameraView {
    let bounds = &scene.area().bounds;
    let centre: [f64; 3] = std::array::from_fn(|a| (bounds.min()[a] + bounds.max()[a]) / 2.0);
    let extent: [f64; 3] = std::array::from_fn(|a| bounds.max()[a] - bounds.min()[a]);
    PlaytestCameraView {
        name: "skull",
        eye: [
            (centre[0] - extent[0] / 2.0 - 20.0) as f32,
            (centre[1] + 0.4 * extent[1]) as f32,
            centre[2] as f32,
        ],
        target: [
            centre[0] as f32,
            (centre[1] + 0.4 * extent[1]) as f32,
            centre[2] as f32,
        ],
    }
}

/// The chase view framing each drawn part that carries a `winglogo` decal:
/// eye outboard, above and aft of the insignia's world-space centroid, looking
/// at it.
fn wing_chase_views(
    app: &mut App,
    scene: &PlaytestScene,
    sources: &PlaytestSources,
) -> Vec<(PlaytestCameraView, Vec<Handle<StandardMaterial>>)> {
    let aircraft_key = scene
        .textures()
        .subjects
        .iter()
        .map(|s| s.container_key.clone())
        .find(|key| key.contains("planes"))
        .expect("the aircraft subject is reported");
    let subject = scene
        .textures()
        .subject(&aircraft_key)
        .expect("the aircraft subject is reported");
    let wing_meshes: Vec<u32> = subject
        .decals
        .iter()
        .filter(|d| d.texture.contains("winglogo"))
        .map(|d| d.mesh)
        .collect();
    let mut views = Vec::new();
    for part in &scene.aircraft().parts {
        if !wing_meshes.contains(&part.mesh_index) {
            continue;
        }
        let Some(slot) = sources.aircraft().meshes().get(part.mesh_index) else {
            continue;
        };
        let logo_materials: Vec<u32> = subject
            .decals
            .iter()
            .filter(|d| d.mesh == part.mesh_index)
            .map(|d| d.material)
            .collect();
        let mut centroid = [0.0f32; 3];
        let mut count = 0usize;
        for (poly, groups) in slot.material_groups.iter().enumerate() {
            if groups.iter().any(|g| logo_materials.contains(&g.material)) {
                for corner in &slot.mesh.polygons[poly].corners {
                    let p = slot.mesh.positions[corner.position as usize];
                    centroid[0] += p[0];
                    centroid[1] += p[1];
                    centroid[2] += p[2];
                    count += 1;
                }
            }
        }
        assert!(count > 0, "{} has no winglogo polygon", part.node_name);
        centroid = [
            centroid[0] / count as f32,
            centroid[1] / count as f32,
            centroid[2] / count as f32,
        ];
        let mut query = app.world_mut().query::<(&AircraftPart, &GlobalTransform)>();
        let world = query
            .iter(app.world())
            .find(|(marker, _)| marker.node_slot == part.node_slot)
            .map(|(_, gt)| gt.transform_point(bevy::math::Vec3::from(centroid)))
            .expect("the part's transform resolved");
        let outboard = if world[0] > scene.spawn()[0] {
            2.5
        } else {
            -2.5
        };
        let view = PlaytestCameraView {
            name: Box::leak(format!("wing-{}", part.node_name).into_boxed_str()),
            eye: [world[0] + outboard, world[1] + 2.0, world[2] + 4.0],
            target: [world[0], world[1], world[2]],
        };
        let handles = subject
            .decals
            .iter()
            .filter(|d| d.mesh == part.mesh_index)
            .map(|d| d.material_handle.clone())
            .collect();
        views.push((view, handles));
    }
    views
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_decal_retail_coplanar_layers_hold_stable_and_stay_drawn() {
    let out = private_dir("stability");
    let mut measured: Vec<(bool, &'static str, DecalStability)> = Vec::new();
    let mut skull_region: Vec<(bool, f64)> = Vec::new();
    for offset in [false, true] {
        let (mut app, scene) = scene_with(offset);
        app.update(); // propagate GlobalTransforms before the centroid lookups
        let world_key = scene
            .textures()
            .subjects
            .iter()
            .map(|s| s.container_key.clone())
            .find(|key| key.contains("c1c"))
            .expect("the world subject is reported");
        let emblem = decal_handles(&scene, &world_key, "fhunter_logo");
        assert!(
            !emblem.is_empty(),
            "the hull emblem's decal layers are not reported"
        );
        let skull = skull_view(&scene);
        let name: &'static str = if offset {
            "after-skull"
        } else {
            "before-skull"
        };
        let s = measure_decal(
            &mut app,
            &scene,
            &PlaytestCameraView { name, ..skull },
            false,
            &emblem,
        );
        eprintln!("DECAL offset={offset} view={name}: {s:?}");
        measured.push((offset, name, s));

        // The same view on #753's whole-region metric: the emblem owned
        // ~99.5% of its flips, so the offset should quiet the frame too.
        let region = capture_view_stability(
            &mut app,
            &scene,
            &PlaytestCameraView { name, ..skull },
            STEP_M,
            4,
            &out,
            false,
        )
        .expect("the skull view renders");
        eprintln!("REGION offset={offset} view={name}: {region:?}");
        skull_region.push((offset, region.worst_share));

        let sources =
            read_playtest_sources(&install_dir(), &scene.config().world_group).expect("sources");
        for (view, handles) in wing_chase_views(&mut app, &scene, &sources) {
            let name: &'static str = Box::leak(
                format!("{}-{}", if offset { "after" } else { "before" }, view.name)
                    .into_boxed_str(),
            );
            let s = measure_decal(
                &mut app,
                &scene,
                &PlaytestCameraView { name, ..view },
                true,
                &handles,
            );
            eprintln!("DECAL offset={offset} view={name}: {s:?}");
            measured.push((offset, name, s));
        }
        let _ = capture_view_pixels(
            &mut app,
            &scene,
            &PlaytestCameraView {
                name: if offset {
                    "skull-after"
                } else {
                    "skull-before"
                },
                ..skull
            },
            false,
            Some(out.join(if offset {
                "skull-after.png"
            } else {
                "skull-before.png"
            })),
        );
    }

    for (offset, name, s) in &measured {
        assert!(
            s.footprint >= MIN_DECAL_PIXELS,
            "{name}: the decal layer draws only {} pixels — hidden is not a fix: {s:?}",
            s.footprint
        );
        if *offset {
            assert!(
                s.lost_share() <= MAX_LOST_SHARE,
                "{name}: the offset decal still loses {:.5} of its interior pixels \
                 (limit {MAX_LOST_SHARE}): {s:?}",
                s.lost_share()
            );
        }
    }
    // The coplanar baseline — the behaviour #794 replaces — must show the
    // fight in the reported skull view and in at least one wing insignia, or
    // the test measures nothing.
    let baseline: Vec<(&'static str, &DecalStability)> = measured
        .iter()
        .filter(|(offset, _, _)| !*offset)
        .map(|(_, name, s)| (*name, s))
        .collect();
    let skull = baseline
        .iter()
        .find(|(name, _)| name.contains("skull"))
        .expect("the skull view was measured");
    assert!(
        skull.1.lost_share() > MAX_LOST_SHARE,
        "before: the coplanar skull emblem must fail the threshold: {:?}",
        skull.1
    );
    assert!(
        baseline
            .iter()
            .filter(|(name, _)| name.contains("wing"))
            .any(|(_, s)| s.lost_share() > MAX_LOST_SHARE),
        "before: at least one wing insignia must fail the threshold: {baseline:?}"
    );

    // #753's own region metric on its own residual view: coplanar, the skull
    // fights; offset, the frame is quiet.
    let (before, after) = (
        skull_region
            .iter()
            .find(|(o, _)| !*o)
            .map(|(_, s)| *s)
            .unwrap_or_default(),
        skull_region
            .iter()
            .find(|(o, _)| *o)
            .map(|(_, s)| *s)
            .unwrap_or_default(),
    );
    assert!(
        before > MAX_LOST_SHARE,
        "before: the skull view's region must fail #753's threshold: {before:.5}"
    );
    assert!(
        after <= MAX_LOST_SHARE,
        "after: the skull view's region must pass #753's threshold: {after:.5}"
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_playtest_decal_retail_the_report_identifies_the_drawn_layers() {
    let config = PlaytestConfig::documented();
    let sources =
        read_playtest_sources(&install_dir(), &config.world_group).expect("the installation reads");
    let mut app = playtest_app();
    let scene = spawn_playtest_scene(&mut app, &sources, &config).expect("the scene spawns");
    let report = scene.textures();
    assert!(
        report
            .claims
            .contains(&cs_app::playtest_textures::PLAYTEST_DECAL_OFFSET_IS_DESIGNED),
        "the report does not carry the designed-rule claim: {:?}",
        report.claims
    );

    let world = report
        .subjects
        .iter()
        .find(|s| s.container_key.contains("c1c"))
        .expect("the world subject is reported");
    assert!(!world.decals.is_empty(), "no decal layers are reported");
    // The emblem the census attributed the flicker to, and its sibling layers.
    for (mesh, material, texture) in [
        (750, 194, "fhunter_logo2.tif"),
        (755, 194, "fhunter_logo2.tif"),
        (697, 190, "fhunter_logo4.tif"),
    ] {
        assert!(
            world
                .decals
                .iter()
                .any(|d| d.mesh == mesh && d.material == material && d.texture == texture),
            "mesh {mesh} material {material} {texture} is not a reported decal: {:?}",
            world
                .decals
                .iter()
                .filter(|d| d.texture.contains("fhunter"))
                .collect::<Vec<_>>()
        );
    }
    // The emblem layer is a second material group of the hull quad, not the
    // base surface itself.
    assert!(
        world
            .decals
            .iter()
            .any(|d| d.texture.contains("fhunter_logo") && d.group > 0),
        "no emblem layer is a later material group: {:?}",
        world.decals
    );

    let aircraft = report
        .subjects
        .iter()
        .find(|s| s.container_key.contains("planes"))
        .expect("the aircraft subject is reported");
    let wings: Vec<_> = aircraft
        .decals
        .iter()
        .filter(|d| d.texture.contains("winglogo"))
        .collect();
    assert!(
        wings.len() >= 2,
        "both wings' insignia should be reported: {wings:?}"
    );
    assert!(
        wings.iter().all(|d| d.material == 95 && d.triangles > 0),
        "wing insignia are stored material 95 drawing real triangles: {wings:?}"
    );

    // The machine-readable report lists them too.
    let json = report.json();
    assert!(json.contains("\"decals\":[{"), "no decals list: {json}");
    assert!(json.contains("fhunter_logo2.tif"), "{json}");
    assert!(json.contains("blo_winglogo.tif"), "{json}");
}

#[test]
fn accept_playtest_decal_report_json_lists_each_layer() {
    // A fixture report must serialize its decal list through the production
    // `json()` — the same path the `playtest sources` line embeds.
    let report = cs_app::playtest_textures::PlaytestTextureReport {
        claims: [
            "playtest-textures.archive-selection-is-designed",
            "playtest-textures.name-reading-is-designed",
            "playtest-textures.presentation-is-provisional",
            "playtest-textures.decal-offset-is-designed",
        ],
        archive: "ZBD/C1C/rtexture10.zbd".to_owned(),
        archive_sha256: "00".to_owned(),
        group: "c1c".to_owned(),
        selection: "the world group's highest-numbered tier".to_owned(),
        name_reading: "stored texture name up to the first dot, ASCII lower case",
        subjects: vec![cs_app::playtest_textures::SubjectTextures {
            container_key: "zbd/c1c/gamez.zbd".to_owned(),
            decals: vec![cs_app::playtest_textures::DecalGroup {
                mesh: 750,
                group: 1,
                material: 194,
                texture: "fhunter_logo2.tif".to_owned(),
                triangles: 2,
                material_handle: Handle::default(),
            }],
            ..cs_app::playtest_textures::SubjectTextures::default()
        }],
    };
    let json = report.json();
    assert!(
        json.contains(
            "\"decals\":[{\"mesh\":750,\"group\":1,\"material\":194,\"texture\":\"fhunter_logo2.tif\",\"triangles\":2}]"
        ),
        "the decal layer is not serialized: {json}"
    );
    assert!(json.contains("decal-offset-is-designed"), "{json}");
}

#[test]
fn accept_playtest_decal_docs_present_the_offset_rule_as_designed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for file in [
        "docs/PLAYTEST.md",
        "docs/PLAYTEST-RETAIL.md",
        "docs/findings/2026-10-08-playtest-decal-zfight.md",
    ] {
        let text = std::fs::read_to_string(root.join(file)).expect("the doc exists");
        assert!(
            text.contains("DECAL_OFFSET_M") || text.contains("decal"),
            "{file} does not describe the decal rule"
        );
    }
    for file in ["docs/PLAYTEST.md", "docs/PLAYTEST-RETAIL.md"] {
        let text = std::fs::read_to_string(root.join(file)).expect("the doc exists");
        assert!(
            text.contains("designed") || text.contains("Provisional"),
            "{file} does not mark the rule as designed/provisional"
        );
    }
}
