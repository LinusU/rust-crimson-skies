//! `accept_f17_b_` tests for the deterministic frame capture:
//! `cs_app::render::capture`.
//!
//! The stage's minimum scenario is AC02 — "Screenshot same camera/tick twice
//! under fixed comparison settings" — so the first test here builds the
//! golden scene twice, from independently decoded canonical inputs, and
//! requires the two captures to be byte-identical. The rest exist because
//! that equality is worthless without the other half: a capture that ignored
//! the tick, the camera, the projection or the geometry would also be equal
//! twice, and a capture taken under unpinned settings compares nothing.

use cs_app::render::capture::{
    CaptureError, ComparisonSettings, Projection, SceneOutcome, SceneSurface, SurfaceRefusal,
    Tonemap, capture, scene_codes, upload_surface,
};
use cs_app::render::golden::golden_scene;
use cs_app::render::material::RenderPhase;
use cs_app::render::plan::{DrawItemKey, DrawPlan, SceneView, SortingLimitation};
use cs_formats::texture::{AlphaSource, AlphaTest, ColorSpace, PixelFormat};
use cs_types::Tick;

use super::fixture::{
    ImageShape, MESH_UNKNOWNS, QUAD_COLORS, QuadShape, decoded_image, quad_mesh,
    quad_mesh_with_colors,
};

/// The tick every capture in this file is taken at.
const TICK: Tick = Tick(4_242);

/// Which canonical inputs one golden-scene item is drawn from.
struct SceneInputs {
    meshes: Vec<cs_content::mesh::RenderMesh>,
    images: Vec<Option<cs_formats::texture::DecodedImage>>,
}

/// Builds the canonical inputs for the golden scene, in submission order.
///
/// Each item gets its own quad so the compaction and the attribute rules are
/// exercised per surface, and the images match the coverage each item
/// declares: the fence and the near pane sample an alpha channel, the far
/// pane declares a constant opacity and has no image, the sprite and the
/// ground are untextured.
fn scene_inputs(scene: &cs_app::render::golden::GoldenScene) -> SceneInputs {
    let mut meshes = Vec::new();
    let mut images = Vec::new();
    for (index, item) in scene.items().iter().enumerate() {
        meshes.push(quad_mesh(QuadShape::full(), index as u32));
        images.push(match item.material().class() {
            cs_app::render::material::MaterialClass::Masked => {
                Some(decoded_image(ImageShape::rgba8_srgb()))
            }
            cs_app::render::material::MaterialClass::Blended => match item.material().coverage() {
                cs_app::render::material::Coverage::Texture(_) => {
                    Some(decoded_image(ImageShape::rgba8_srgb()))
                }
                _ => None,
            },
            _ => None,
        });
    }
    SceneInputs { meshes, images }
}

/// Uploads every item of the golden scene, in submission order.
fn upload_scene(
    scene: &cs_app::render::golden::GoldenScene,
    inputs: &SceneInputs,
) -> Vec<SceneOutcome> {
    scene
        .items()
        .iter()
        .enumerate()
        .map(|(index, item)| {
            upload_surface(&SceneSurface {
                item,
                mesh: &inputs.meshes[index],
                group: 0,
                image: inputs.images[index].as_ref(),
                unknowns: &MESH_UNKNOWNS,
            })
        })
        .collect()
}

/// AC02: the same camera and tick captured twice under the fixed comparison
/// settings produce the same frame, and the frame is the golden scene's
/// ordered plan.
#[test]
fn accept_f17_b_capture_of_the_same_camera_and_tick_twice_is_identical() {
    let scene = golden_scene();
    let plan = scene.draw_plan();
    let settings = ComparisonSettings::comparison();

    // Two independent builds: the canonical inputs are decoded and the
    // meshes are resolved twice, so a non-deterministic adapter would show up
    // here rather than as two reads of one buffer.
    let first = capture(
        &upload_scene(&scene, &scene_inputs(&scene)),
        &plan,
        scene.view(),
        &Projection::comparison(),
        TICK,
        &settings,
    )
    .expect("the golden scene captures");
    let second = capture(
        &upload_scene(&scene, &scene_inputs(&scene)),
        &plan,
        scene.view(),
        &Projection::comparison(),
        TICK,
        &settings,
    )
    .expect("the golden scene captures again");

    assert_eq!(
        first.fingerprint(),
        second.fingerprint(),
        "the same camera and tick under fixed settings capture identically"
    );
    assert_eq!(
        first, second,
        "and the frames are equal, not only their digests"
    );
    assert!(
        first.refusals().is_empty(),
        "every golden surface is drawable"
    );
    assert_eq!(first.tick(), TICK);
    assert_eq!(first.settings(), &settings);
    assert_eq!(first.projection(), &Projection::comparison());

    let order: Vec<(&str, RenderPhase)> = first
        .surfaces()
        .map(|surface| (surface.key().as_str(), surface.phase()))
        .collect();
    assert_eq!(
        order,
        [
            ("percorner", RenderPhase::Opaque),
            ("ground", RenderPhase::Opaque),
            ("fence", RenderPhase::Masked),
            ("glass_far", RenderPhase::Translucent),
            ("glass_near", RenderPhase::Translucent),
            ("sprite", RenderPhase::Additive),
        ],
        "the capture keeps the plan's order: the additive sprite draws after both panes"
    );
    assert_eq!(
        first
            .pass(RenderPhase::Translucent)
            .expect("the translucent pass is captured")
            .iter()
            .map(|surface| surface.depth_m())
            .collect::<Vec<f32>>(),
        vec![6.2, 5.0],
        "the near pane draws after the far one"
    );
    let fence = first
        .surfaces()
        .find(|surface| surface.key().as_str() == "fence")
        .expect("the fence is captured");
    assert!(fence.image().is_some(), "the fence samples its image");
    assert!(
        fence.material_gap().is_none(),
        "and has a drawable material"
    );
    let sprite = first
        .surfaces()
        .find(|surface| surface.key().as_str() == "sprite")
        .expect("the sprite is captured");
    assert!(sprite.image().is_none(), "the sprite is untextured");
    assert_eq!(
        sprite.material_gap(),
        Some("additive_blend_state_needs_custom_material"),
        "the additive pass is captured with its recorded gap, not dropped"
    );
    let percorner = first
        .surfaces()
        .find(|surface| surface.key().as_str() == "percorner")
        .expect("the colored quad is captured");
    let ground = first
        .surfaces()
        .find(|surface| surface.key().as_str() == "ground")
        .expect("the ground is captured");
    assert_ne!(
        percorner.state(),
        ground.state(),
        "a surface with per-corner colors is not the same state as one without"
    );
}

/// The equality above is only worth something if the capture depends on the
/// things it claims to pin.
#[test]
fn accept_f17_b_capture_depends_on_the_tick_the_camera_and_the_geometry() {
    let scene = golden_scene();
    let plan = scene.draw_plan();
    let settings = ComparisonSettings::comparison();
    let projection = Projection::comparison();
    let base = capture(
        &upload_scene(&scene, &scene_inputs(&scene)),
        &plan,
        scene.view(),
        &projection,
        TICK,
        &settings,
    )
    .expect("the golden scene captures");

    let next_tick = capture(
        &upload_scene(&scene, &scene_inputs(&scene)),
        &plan,
        scene.view(),
        &projection,
        Tick(TICK.0 + 1),
        &settings,
    )
    .expect("the next tick captures");
    assert_ne!(
        base.fingerprint(),
        next_tick.fingerprint(),
        "a different tick is a different frame"
    );

    let other_view = SceneView::new([1.5, 0.5, 4.0], [0.2, 0.0, -1.0]).expect("a finite view");
    let moved = capture(
        &upload_scene(&scene, &scene_inputs(&scene)),
        &plan,
        &other_view,
        &projection,
        TICK,
        &settings,
    )
    .expect("the turned view captures");
    assert_ne!(
        base.fingerprint(),
        moved.fingerprint(),
        "a different camera is a different frame"
    );
    assert_ne!(
        base.position_m(),
        moved.position_m(),
        "and the pose is part of the capture"
    );

    let wider = Projection::new(1.2, 4.0 / 3.0, 0.1, 10_000.0).expect("a wider field of view");
    let reframed = capture(
        &upload_scene(&scene, &scene_inputs(&scene)),
        &plan,
        scene.view(),
        &wider,
        TICK,
        &settings,
    )
    .expect("the wider frame captures");
    assert_ne!(
        base.fingerprint(),
        reframed.fingerprint(),
        "a different projection is a different frame"
    );

    // One stored corner color changed by a quarter: the geometry digest moves
    // with it, so the capture cannot be equal by ignoring the buffers.
    let mut inputs = scene_inputs(&scene);
    let mut recolored = QUAD_COLORS;
    recolored[0][0] = 0.75;
    inputs.meshes[0] = quad_mesh_with_colors(QuadShape::full(), 0, recolored);
    let changed = capture(
        &upload_scene(&scene, &inputs),
        &plan,
        scene.view(),
        &projection,
        TICK,
        &settings,
    )
    .expect("the recolored scene captures");
    assert_ne!(
        base.fingerprint(),
        changed.fingerprint(),
        "a different vertex buffer is a different frame"
    );
}

/// Spec F17 non-negotiable 3: exposure, tonemapping and gamma are fixed in
/// comparison mode. A capture under anything else compares nothing and is
/// refused.
#[test]
fn accept_f17_b_capture_refuses_settings_that_are_not_the_fixed_set() {
    let scene = golden_scene();
    let plan = scene.draw_plan();
    let outcomes = upload_scene(&scene, &scene_inputs(&scene));

    for settings in [
        ComparisonSettings::with_exposure(1.2),
        ComparisonSettings::with_msaa_samples(4),
    ] {
        assert!(!settings.is_fixed());
        let error = capture(
            &outcomes,
            &plan,
            scene.view(),
            &Projection::comparison(),
            TICK,
            &settings,
        )
        .expect_err("an unpinned comparison is refused");
        match error {
            CaptureError::SettingsNotFixed { msaa_samples, .. } => {
                assert_eq!(msaa_samples, settings.msaa_samples())
            }
            other => panic!("an unpinned setting is SettingsNotFixed, got {other}"),
        }
    }

    let fixed = ComparisonSettings::comparison();
    assert!(fixed.is_fixed());
    assert_eq!(fixed.tonemap(), Tonemap::None);
    assert_eq!(fixed.msaa_samples(), 1);
    assert_eq!(fixed.exposure(), 1.0);
    assert_eq!(fixed.gamma(), 2.2);

    for error in [
        Projection::new(f32::NAN, 1.0, 0.1, 1.0).expect_err("a NaN field of view is refused"),
        Projection::new(0.0, 1.0, 0.1, 1.0).expect_err("a zero field of view is refused"),
        Projection::new(1.0, 0.0, 0.1, 1.0).expect_err("a zero aspect ratio is refused"),
        Projection::new(1.0, 1.0, 0.0, 1.0).expect_err("a zero near plane is refused"),
        Projection::new(1.0, 1.0, 2.0, 1.0).expect_err("a near plane beyond far is refused"),
    ] {
        assert!(error.to_string().len() > 10, "the refusal explains itself");
    }
}

/// A surface the adapters refused is not drawn and not hidden: it is reported
/// with its reason, and the plan's own sorting limitations travel with the
/// frame.
#[test]
fn accept_f17_b_capture_reports_refusals_and_the_plan_limits() {
    let scene = golden_scene();
    let plan = scene.draw_plan();
    let mut inputs = scene_inputs(&scene);
    // The fence's image now stores coverage in a plane while the material
    // declares a channel: the surface cannot be drawn and the capture has to
    // say so.
    inputs.images[3] = Some(decoded_image(ImageShape {
        format: PixelFormat::Rgb8,
        alpha_source: AlphaSource::Plane,
        alpha_test: AlphaTest::Threshold(0x10),
        color_space: ColorSpace::Srgb,
    }));
    let frame = capture(
        &upload_scene(&scene, &inputs),
        &plan,
        scene.view(),
        &Projection::comparison(),
        TICK,
        &ComparisonSettings::comparison(),
    )
    .expect("a scene with a refused surface still captures");

    assert_eq!(frame.refusals().len(), 1);
    let refusal = &frame.refusals()[0];
    assert_eq!(refusal.key().as_str(), "fence");
    assert_eq!(refusal.reasons(), ["coverage_source_mismatch"]);
    assert!(
        !frame
            .surfaces()
            .any(|surface| surface.key().as_str() == "fence"),
        "a refused surface is not drawn"
    );
    assert_eq!(
        frame
            .pass(RenderPhase::Masked)
            .expect("the masked pass exists")
            .len(),
        0,
        "and its phase is empty rather than skipped"
    );
    assert_ne!(
        frame.fingerprint(),
        capture(
            &upload_scene(&scene, &scene_inputs(&scene)),
            &plan,
            scene.view(),
            &Projection::comparison(),
            TICK,
            &ComparisonSettings::comparison(),
        )
        .expect("the complete scene captures")
        .fingerprint(),
        "a refusal is part of the frame's identity"
    );

    // Two blended panes at the same view depth: the plan reports the tie and
    // the capture carries the report.
    let tied = golden_scene();
    let far = tied
        .items()
        .iter()
        .find(|item| item.key().as_str() == "glass_far")
        .expect("the far pane");
    let view = SceneView::new([0.0, 0.0, 0.0], [0.0, 0.0, -1.0]).expect("a finite view");
    let at_far_depth = cs_app::render::plan::DrawItem::new(
        DrawItemKey::new("glass_far_tied").expect("authored key"),
        far.material().clone(),
        far.center_m(),
        None,
    )
    .expect("finite geometry");
    let items = vec![far.clone(), at_far_depth];
    let plan = DrawPlan::build(&items, &view);
    assert_eq!(
        plan.limitations(),
        [SortingLimitation::EqualViewDepth {
            phase: RenderPhase::Translucent,
            first: DrawItemKey::new("glass_far").expect("authored key"),
            second: DrawItemKey::new("glass_far_tied").expect("authored key"),
        }],
        "the tie is reported by the plan"
    );
    let frame = capture(
        &[
            upload_surface(&SceneSurface {
                item: &items[0],
                mesh: &quad_mesh(QuadShape::full(), 0),
                group: 0,
                image: None,
                unknowns: &MESH_UNKNOWNS,
            }),
            upload_surface(&SceneSurface {
                item: &items[1],
                mesh: &quad_mesh(QuadShape::full(), 0),
                group: 0,
                image: None,
                unknowns: &MESH_UNKNOWNS,
            }),
        ],
        &plan,
        &view,
        &Projection::comparison(),
        TICK,
        &ComparisonSettings::comparison(),
    )
    .expect("the tied scene captures");
    assert_eq!(
        frame.limitations().len(),
        1,
        "the capture carries the plan's limitations"
    );
}

/// The capture and the plan must describe the same scene. A short list, a
/// swapped list and a stale leftover are all refused rather than read as "the
/// same frame".
#[test]
fn accept_f17_b_capture_refuses_a_scene_that_does_not_match_the_plan() {
    let scene = golden_scene();
    let plan = scene.draw_plan();
    let view = SceneView::new([0.0, 0.0, 4.0], [0.0, 0.0, -1.0]).expect("a finite view");
    let outcomes = upload_scene(&scene, &scene_inputs(&scene));
    let capture_it = |outcomes: &[SceneOutcome]| {
        capture(
            outcomes,
            &plan,
            &view,
            &Projection::comparison(),
            TICK,
            &ComparisonSettings::comparison(),
        )
    };

    assert!(capture_it(&outcomes).is_ok(), "the complete scene captures");

    let short = &outcomes[..outcomes.len() - 1];
    match capture_it(short).expect_err("a missing surface is refused") {
        CaptureError::SceneIncomplete { code, key } => {
            assert_eq!(code, scene_codes::MISSING_OUTCOME);
            assert_eq!(key.as_str(), "ground");
        }
        other => panic!("a short scene is SceneIncomplete, got {other}"),
    }

    // A stale outcome from an earlier scene is never read as part of this
    // one, even though it is a well-formed surface.
    let mut extra = upload_scene(&scene, &scene_inputs(&scene));
    extra.push(SceneOutcome::Refused(SurfaceRefusal::new(
        DrawItemKey::new("ghost").expect("authored key"),
        vec!["color_space_unknown"],
    )));
    match capture_it(&extra).expect_err("a leftover surface is refused") {
        CaptureError::SceneIncomplete { code, key } => {
            assert_eq!(code, scene_codes::UNUSED_OUTCOME);
            assert_eq!(key.as_str(), "ghost");
        }
        other => panic!("a leftover is SceneIncomplete, got {other}"),
    }
}
