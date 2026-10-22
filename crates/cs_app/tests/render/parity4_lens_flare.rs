//! Acceptance for PARITY4-LENS-FLARE (#1756) — the split's integration step:
//! the stored mesh light that flags the horizon's `sun` node (A, #1858), the
//! `lflare` quad draw it feeds (B, #1859) and the owner-command smoke frame
//! that shows it (C, #1860), joined end to end.
//!
//! Test prefix: `accept_parity4_lens_flare_` — the parent's own selection,
//! which also reaches every subtask test (`accept_parity4_lens_flare_a_`,
//! `_b_`, `_c_`). The synthetic tests run in CI and drive the production
//! `MissionLensFlarePlugin` over a stand-in scene camera and a bound
//! `node-3237` object. The ignored tests need `retail` (M01's real stored
//! record and `lflare` art, through the production readers) and
//! `retail,gpu,audio` (the owner's own windowed command). Nothing here is
//! `verified_original`: the bulb's world placement is a test one, labelled
//! designed in the binding.

use std::path::{Path, PathBuf};

use bevy::camera::{
    Camera, CameraProjection, PerspectiveProjection, Projection, RenderTargetInfo, Viewport,
};
use bevy::math::UVec2;
use bevy::prelude::{
    App, Assets, Camera3d, GlobalTransform, Handle, Image, Node, Val, Vec2, Vec3, Visibility, With,
};
use bevy::ui::widget::ImageNode;
use cs_app::mission_session::{
    FLARE_ALPHA, FlareQuadNode, FlareSource, HorizonSubtree, LensFlareSources,
    MIN_FLARE_QUAD_PIXELS, MissionLensFlare, MissionLensFlarePlugin, SMOKE_KIND, SMOKE_REPORT_FILE,
    SUN_SCREEN_FRACTION, read_flare_images,
};
use cs_app::playtest_retail::read_world_sources;
use cs_app::render::lens_flare::flare_quads;
use cs_app::world::contacts::WorldObjectBinding;
use cs_content::world::{SurfaceRole, WorldId, WorldObjectId};
use cs_types::content::{Provenance, Resolved};
use cs_types::evidence::ClaimId;

// ------------------------------------------------- the shared fixtures ---

/// The logical viewport the tests draw in.
const VIEWPORT: [f32; 2] = [1280.0, 720.0];

/// The M01 `sun` source's stored numbers (mesh 491's flagged light, finding
/// `2026-10-22-parity4-lens-flare-a-sun-source-and-blend.md` §1): the bulb
/// `extra[0]` in node space and the fade pair `unk68`/`unk72`.
const SUN_BULB: Vec3 = Vec3::new(6.184_89, 18.294_209, 0.0);
const SUN_FADE: (f32, f32) = (5.0, 10000.0);
/// The `sun` node's stored slot (it owns mesh 491).
const SUN_SLOT: u32 = 3237;

/// The sun source as `LensFlareSources::read` recovers it from C3: the node
/// record `sun` owns mesh 491's flagged light.
fn sun_source() -> FlareSource {
    FlareSource {
        slot: SUN_SLOT,
        name: "sun".to_owned(),
        bulb: SUN_BULB,
        fade_min: SUN_FADE.0,
        fade_max: SUN_FADE.1,
    }
}

/// A source set with `sun` drawn and the two `lens_flash` records held back,
/// the shape M01's container produces.
fn m01_sources() -> LensFlareSources {
    LensFlareSources {
        drawn: vec![sun_source()],
        held_back: vec![
            (
                "lens_flash".to_owned(),
                "placement and trigger of a flare outside the horizon are not recovered",
            ),
            (
                "lens_flash".to_owned(),
                "placement and trigger of a flare outside the horizon are not recovered",
            ),
        ],
    }
}

/// Spawns the stand-in scene camera: a real perspective projection, a fixed
/// logical viewport and `Camera` computed values the projection helpers read
/// — the same stand-in the smoke's flare phase tests use, no window.
fn spawn_scene_camera(app: &mut App) {
    let vfov = 60.0_f32.to_radians();
    let projection = PerspectiveProjection {
        fov: vfov,
        aspect_ratio: VIEWPORT[0] / VIEWPORT[1],
        ..Default::default()
    };
    let mut camera = Camera {
        is_active: true,
        viewport: Some(Viewport {
            physical_size: UVec2::new(VIEWPORT[0] as u32, VIEWPORT[1] as u32),
            ..Default::default()
        }),
        ..Default::default()
    };
    camera.computed.target_info = Some(RenderTargetInfo {
        physical_size: UVec2::new(VIEWPORT[0] as u32, VIEWPORT[1] as u32),
        scale_factor: 1.0,
    });
    camera.computed.clip_from_view = projection.get_clip_from_view();
    app.world_mut().spawn((
        camera,
        Camera3d::default(),
        Projection::Perspective(projection),
        GlobalTransform::IDENTITY,
    ));
}

/// Binds the world object `place_flares` resolves the source through: the
/// `node-<slot>` id the world draw gives the node's owner, placed so `bulb`
/// lands at `bulb_world` — a test placement, labelled designed.
fn bind_object(app: &mut App, slot: u32, bulb: Vec3, bulb_world: Vec3) {
    app.world_mut().spawn((
        WorldObjectBinding::new(
            WorldId::from_key("fixture").expect("the fixture world key is valid"),
            WorldObjectId::new(&format!("node-{slot}")).expect("the node slot key is valid"),
            Vec::new(),
            Resolved::<SurfaceRole>::unknown(
                ClaimId::new("test.sun.surface").expect("the claim id is valid"),
                "the sun sits in the sky",
            )
            .expect("the explicit unknown resolves"),
            Provenance::designed(ClaimId::new("test.sun.fixture").expect("the claim id is valid")),
        ),
        GlobalTransform::from_translation(bulb_world - bulb),
    ));
}

/// The app the plugin's systems run in: `MissionLensFlare` carries the
/// sources and the art, one scene camera watches. `spawn_flare_nodes` makes
/// the quad nodes at `Startup`, `place_flares` lays them out at `PostUpdate`.
fn flare_app(sources: LensFlareSources, images: [Handle<Image>; 4]) -> App {
    let mut app = App::new();
    app.add_plugins(MissionLensFlarePlugin);
    app.insert_resource(MissionLensFlare::new(sources, images));
    spawn_scene_camera(&mut app);
    app
}

/// The quad nodes the plugin spawned, sorted by `(source, texture)`.
fn laid_out(app: &mut App) -> Vec<(FlareQuadNode, Node, Visibility)> {
    let mut query = app
        .world_mut()
        .query::<(&FlareQuadNode, &Node, &Visibility)>();
    let mut rows: Vec<_> = query
        .iter(app.world())
        .map(|(quad, node, visibility)| (*quad, node.clone(), *visibility))
        .collect();
    rows.sort_by_key(|row| (row.0.source, row.0.texture));
    rows
}

/// The projection `place_flares` read for `bulb_world`, measured back
/// through the spawned scene camera.
fn projected(app: &mut App, bulb_world: Vec3) -> Vec2 {
    let mut query = app
        .world_mut()
        .query_filtered::<(&Camera, &GlobalTransform), With<Camera3d>>();
    let (camera, transform) = query.single(app.world()).expect("one scene camera");
    camera
        .world_to_viewport(transform, bulb_world)
        .expect("the test sun projects")
}

/// Every drawn quad sits where the engine's draw rule puts it, at the stored
/// fade, with the designed 50 percent alpha the sprite routine is handed.
fn assert_quads_laid_out(app: &mut App, bulb_world: Vec3) {
    let at = projected(app, bulb_world);
    let expected = flare_quads(VIEWPORT, [at.x, at.y], 1.0);
    assert_eq!(
        expected.len(),
        4,
        "the sun's stored pair fades in full: {expected:?}"
    );
    let quads = laid_out(app);
    assert_eq!(quads.len(), 4, "one quad per lflare texture: {quads:?}");
    for ((quad, node, visibility), want) in quads.iter().zip(&expected) {
        assert_eq!(quad.source, 0);
        assert_eq!(quad.texture, want.texture);
        assert_eq!(
            *visibility,
            Visibility::Inherited,
            "the quad shows: {quad:?}"
        );
        let half = want.half_extent;
        assert_eq!(node.left, Val::Px(want.centre[0] - half), "{want:?}");
        assert_eq!(node.top, Val::Px(want.centre[1] - half), "{want:?}");
        assert_eq!(node.width, Val::Px(half * 2.0), "{want:?}");
        assert_eq!(node.height, Val::Px(half * 2.0), "{want:?}");
    }
    let mut images = app.world_mut().query::<(&FlareQuadNode, &ImageNode)>();
    for (quad, image) in images.iter(app.world()) {
        let alpha = image.color.to_srgba().alpha;
        assert!(
            (alpha - FLARE_ALPHA).abs() < 1e-3,
            "{quad:?} draws at the sprite routine's opacity: {alpha}"
        );
    }
}

// -------------------------------------------------- the integration leg ---

/// **`accept_parity4_lens_flare_` a stored flagged bulb in view lays out all
/// four quads by the engine's draw rule.**
///
/// The chain the subtasks built: `LensFlareSources::read`'s output (the `sun`
/// record A recovered), the `lflare` art slots B loads and the
/// `MissionLensFlarePlugin` draw — `spawn_flare_nodes` makes one quad per
/// `lflare` texture, `place_flares` projects the bound `node-3237` bulb
/// through the scene camera and places each quad at its recovered centre and
/// half-extent at the stored fade, the sprite routine's 50 percent alpha on
/// the image node.
#[test]
fn accept_parity4_lens_flare_a_flagged_bulb_in_view_lays_out_all_four_quads() {
    let mut app = flare_app(m01_sources(), [(); 4].map(|_| Handle::<Image>::default()));
    let bulb_world = Vec3::new(-1500.0, 2200.0, -8000.0);
    bind_object(&mut app, SUN_SLOT, SUN_BULB, bulb_world);
    app.update();
    assert_quads_laid_out(&mut app, bulb_world);
}

/// **`accept_parity4_lens_flare_` a source that projects outside the
/// viewport draws nothing, and a held-back source spawns nothing.**
///
/// The engine's record queue drops a light behind the near plane or outside
/// the viewport (the A finding §2); `place_flares` hides every quad first,
/// so a source that fails the same tests leaves all four hidden. A
/// `held_back` row — the two `lens_flash` nodes whose placement is not
/// recovered — never becomes a quad at all: `spawn_flare_nodes` iterates
/// `drawn` only, so an unknown placement is never guessed.
#[test]
fn accept_parity4_lens_flare_the_gates_hide_what_the_engine_drops() {
    // Behind the camera: the depth is not positive, the fade refuses.
    let mut app = flare_app(m01_sources(), [(); 4].map(|_| Handle::<Image>::default()));
    bind_object(
        &mut app,
        SUN_SLOT,
        SUN_BULB,
        Vec3::new(-100.0, 50.0, 5000.0),
    );
    app.update();
    for (quad, _, visibility) in laid_out(&mut app) {
        assert_eq!(
            visibility,
            Visibility::Hidden,
            "behind the camera draws nothing: {quad:?}"
        );
    }

    // Projected outside the viewport: the record the engine drops.
    let mut app = flare_app(m01_sources(), [(); 4].map(|_| Handle::<Image>::default()));
    bind_object(
        &mut app,
        SUN_SLOT,
        SUN_BULB,
        Vec3::new(-40_000.0, 0.0, -8000.0),
    );
    app.update();
    for (quad, _, visibility) in laid_out(&mut app) {
        assert_eq!(
            visibility,
            Visibility::Hidden,
            "outside the viewport draws nothing: {quad:?}"
        );
    }

    // Only held-back sources: no `drawn` entry, no quad node exists to show.
    let mut app = flare_app(
        LensFlareSources {
            drawn: Vec::new(),
            held_back: vec![("lens_flash".to_owned(), "not recovered")],
        },
        [(); 4].map(|_| Handle::<Image>::default()),
    );
    app.update();
    assert!(
        laid_out(&mut app).is_empty(),
        "a held-back source spawns nothing"
    );
}

// ------------------------------------------------------ the retail leg ---

/// **Retail M01: the stored `sun` record and the real `lflare` art drive the
/// same draw end to end.**
///
/// The production readers measure C3's world container, the horizon subtree
/// holds exactly the `sun` flagged light and `read_flare_images` resolves
/// `lflare1..4` from the group's own archive — then the plugin lays the four
/// quads out for the real record's stored bulb and fade pair. The
/// integration the parent asked for, on the bytes the original ships.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity4_lens_flare_retail_m01_the_stored_sun_drives_the_four_quads() {
    let root = std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!("CS_GAME_DIR is not set: this retail test needs the original installation")
    });
    let sources = read_world_sources(Path::new(&root), "C3")
        .expect("M01's world group reads through the production readers");
    let horizon = HorizonSubtree::read(sources.world().nodes()).expect("C3 stores a horizon");
    let flare = LensFlareSources::from_container(sources.world(), Some(&horizon));
    assert_eq!(flare.drawn.len(), 1, "{flare:?}");
    let sun = flare.drawn[0].clone();
    assert_eq!(sun.name, "sun");
    assert!(
        (sun.bulb - SUN_BULB).length() < 1e-3,
        "the stored bulb: {:?}",
        sun.bulb
    );
    assert_eq!(
        (sun.fade_min, sun.fade_max),
        SUN_FADE,
        "the stored fade pair"
    );
    let held: Vec<&str> = flare.held_back.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(held, ["lens_flash", "lens_flash"], "{flare:?}");
    let mut images = Assets::<Image>::default();
    let handles = read_flare_images(sources.textures(), &mut images)
        .expect("lflare1..4 resolve in C3's archive");

    let mut app = flare_app(flare, handles);
    let bulb_world = Vec3::new(-1500.0, 2200.0, -8000.0);
    bind_object(&mut app, sun.slot, sun.bulb, bulb_world);
    app.update();
    assert_quads_laid_out(&mut app, bulb_world);
}

// ---------------------------------------------------- the windowed half ---

/// The windowed binary the ignored run drives: `CS_MISSION_BIN` when the
/// operator names one, else the release build, else this harness's own.
fn mission_bin() -> PathBuf {
    if let Ok(bin) = std::env::var("CS_MISSION_BIN") {
        return PathBuf::from(bin);
    }
    let release = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/release/cs");
    if release.is_file() {
        return release;
    }
    PathBuf::from(env!("CARGO_BIN_EXE_cs"))
}

/// The mean and peak 8-bit luminance of a captured framebuffer PNG — the
/// evidence that the `lens_flare` frame is the drawn scene and not black.
fn luminance(path: &std::path::Path) -> (u64, u8) {
    let image = image::ImageReader::open(path)
        .unwrap_or_else(|error| panic!("{} decodes: {error}", path.display()))
        .decode()
        .unwrap_or_else(|error| panic!("{} is a PNG: {error}", path.display()))
        .to_luma8();
    let mut sum = 0u64;
    let mut peak = 0u8;
    for pixel in image.pixels() {
        sum += u64::from(pixel.0[0]);
        peak = peak.max(pixel.0[0]);
    }
    let count = image.pixels().len().max(1) as u64;
    (sum / count, peak)
}

/// **The owner's own command shows it: the sun's glow and the flare-ring
/// chain, in the framebuffer the smoke's sun-aim phase photographs.**
///
/// This is the same 45 s smoke the LENS-FLARE-C phase drives; this task's
/// copy is the integration acceptance the parent still owes (rule 6: seen,
/// not just tested). The capture dir is `CS_LENS_FLARE_SMOKE_OUT` when the
/// evidence harness names it, else a temp dir.
#[test]
#[ignore = "requires CS_GAME_DIR + gpu + audio and a built windowed binary"]
fn accept_parity4_lens_flare_the_windowed_smoke_shows_the_sun_and_its_rings() {
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is unset: this run needs the original installation");
    };
    let Ok(capabilities) = std::env::var("CS_CAPABILITIES") else {
        panic!("CS_CAPABILITIES is unset: the mission launch needs gpu and audio");
    };
    for capability in ["gpu", "audio"] {
        assert!(
            capabilities
                .split(',')
                .any(|part| part.trim() == capability),
            "CS_CAPABILITIES must list {capability}: {capabilities}"
        );
    }
    let engine_image = std::env::var("CS_ENGINE_IMAGE")
        .expect("CS_ENGINE_IMAGE names the owner's decrypted executable");
    let dir = std::env::var("CS_LENS_FLARE_SMOKE_OUT").map_or_else(
        |_| std::env::temp_dir().join("parity4-lens-flare-smoke"),
        PathBuf::from,
    );
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the capture directory is created");
    let output = std::process::Command::new(mission_bin())
        .args(["--cs-path", &game_dir, "--mission", "M01"])
        .args(["--smoke-seconds", "45", "--capture-dir"])
        .arg(&dir)
        .env("CS_CAPABILITIES", &capabilities)
        .env("CS_ENGINE_IMAGE", engine_image)
        .output()
        .expect("the cs binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.code().is_some(),
        "the run must exit by itself, never on a signal: {output:?}\n{stderr}"
    );

    let report = std::fs::read_to_string(dir.join(SMOKE_REPORT_FILE))
        .unwrap_or_else(|error| panic!("{} is written: {error}\n{stderr}", SMOKE_REPORT_FILE));
    assert!(
        report.contains(&format!("\"kind\":\"{SMOKE_KIND}\"")),
        "the report names its kind: {report}"
    );
    assert!(
        report.contains("\"human_play\":false"),
        "a scripted run is never human_play: {report}"
    );
    // The sun-aim checks judged it: the sun in view on the capture's
    // fraction and every `lflare` quad drawn, beside the delivered shots.
    for id in [
        "lens_flare_sun_in_view",
        "lens_flare_quads_drawn",
        "smoke_shots_delivered",
    ] {
        assert!(
            report.contains(&format!("\"id\":\"{id}\",\"applies\":true,\"passed\":true")),
            "the report judges {id} applied and passed: {report}"
        );
    }
    let flare = report
        .split_once("\"lens_flare\":{")
        .and_then(|(_, rest)| {
            rest.split_once("]},\"actors\"")
                .map(|(block, _)| block.to_owned())
        })
        .unwrap_or_else(|| panic!("the report serialises the lens_flare measure: {report}"));
    assert!(
        flare.contains("\"source\":\"sun\""),
        "the aimed source is the sun node: {flare}"
    );
    assert!(
        flare.contains("\"placed\":true"),
        "the aimed placement was written: {flare}"
    );
    assert!(
        flare.contains("\"frames_delivered\":true"),
        "the raw frame pair arrived: {flare}"
    );
    assert_eq!(
        flare.matches("\"drawn\":true").count(),
        4,
        "all four lflare quads drew: {flare}"
    );
    for quad in flare.split('{').skip(1) {
        let pixels: Option<usize> = quad.split_once("\"pixels_added\":").and_then(|(_, rest)| {
            rest.trim_start()
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .ok()
        });
        assert!(
            pixels.is_some_and(|added| added >= MIN_FLARE_QUAD_PIXELS),
            "each quad added at least {MIN_FLARE_QUAD_PIXELS} pixels over the hidden frame: {flare}"
        );
    }
    // The sun sat where the aim put it: the designed fraction of the
    // viewport, the composition of the owner's capture.
    let sun_px = flare
        .split_once("\"sun_px\":[")
        .and_then(|(_, rest)| rest.split_once(']').map(|(pair, _)| pair.to_owned()))
        .and_then(|pair| {
            let mut parts = pair.split(',').map(|part| part.parse::<f32>());
            Some((parts.next()?.ok()?, parts.next()?.ok()?))
        })
        .expect("the report names the sun's measured position");
    let viewport = flare
        .split_once("\"viewport_px\":[")
        .and_then(|(_, rest)| rest.split_once(']').map(|(pair, _)| pair.to_owned()))
        .and_then(|pair| {
            let mut parts = pair.split(',').map(|part| part.parse::<f32>());
            Some((parts.next()?.ok()?, parts.next()?.ok()?))
        })
        .expect("the report names the viewport the sun was measured in");
    let fraction = (sun_px.0 / viewport.0, sun_px.1 / viewport.1);
    assert!(
        (fraction.0 - SUN_SCREEN_FRACTION[0]).abs() <= 0.05
            && (fraction.1 - SUN_SCREEN_FRACTION[1]).abs() <= 0.05,
        "the sun sat at {fraction:?} of {viewport:?}, not {SUN_SCREEN_FRACTION:?}"
    );
    // The framebuffer pair itself: lit captures, never black frames.
    for label in ["lens_flare", "lens_flare_hidden"] {
        let pngs: Vec<_> = std::fs::read_dir(&dir)
            .expect("the capture directory lists")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(&format!("_{label}.png")))
            })
            .collect();
        assert_eq!(
            pngs.len(),
            1,
            "exactly one frame_*_{label}.png arrives: {pngs:?} in {}",
            dir.display()
        );
        let (mean, peak) = luminance(&pngs[0]);
        assert!(
            peak > 24,
            "{label} is a lit framebuffer, not black (mean {mean}, peak {peak})"
        );
    }
    let passed = report
        .rfind("\"passed\":")
        .is_some_and(|index| report[index..].starts_with("\"passed\":true"));
    assert_eq!(
        output.status.success(),
        passed,
        "the process exits non-zero exactly when a check failed; stderr: {stderr}"
    );
    println!(
        "lens-flare smoke artifacts in {}: the sun sat at {fraction:?}, report passed = {passed}",
        dir.display()
    );
}
