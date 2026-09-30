//! `accept_f17_c_additive_` tests for the additive class's drawable material
//! and the WGSL shader it loads.
//!
//! F17-B recorded the gap this selection closes
//! (`docs/findings/2026-09-30-f17-b-canonical-mesh-and-image-to-bevy.md`): the
//! additive class has a complete render state — `One`/`One`, no depth write —
//! and *no drawable material*, because a `StandardMaterial` takes its blend
//! from `alpha_mode` and Bevy 0.19 maps `AlphaMode::Add` onto the
//! premultiplied-alpha pipeline. F17-C's consumer counted such a batch in
//! `FrameSync::unmaterialed` and spawned nothing for it. Every class has a
//! material now, so the questions here are:
//!
//! * does the additive class produce a drawable material, and is that
//!   material's blend the state that was already recorded rather than a second
//!   table that could drift from it?
//! * does the material actually reach a pipeline — is the `One`/`One` blend
//!   and the "no depth write" written into a specialized descriptor rather than
//!   left on a field nothing reads?
//! * is the shader a real file this material loads, or a WGSL string nobody
//!   compiles?
//! * is a **non-additive** class unaffected — the material is the additive
//!   class's, and it must not change what the other four draw with?
//!
//! No GPU is involved: a blend state, a material type, a specialization key and
//! a file's contents are all checkable without a device. Whether the pixels
//! come out right is F17-D's `gpu` evidence, and the assumptions the shader
//! makes are recorded in
//! `docs/findings/2026-09-30-f17-c-followup-additive-material.md`.
//!
//! Every input is newly authored synthetic content decoded through the
//! production readers. No `CS_GAME_DIR`, no original-behavior claim.

use std::path::{Path, PathBuf};

use bevy::asset::Assets;
use bevy::ecs::prelude::World;
use bevy::image::Image;
use bevy::material::AlphaMode;
use bevy::mesh::{Mesh, Mesh3d};
use bevy::pbr::{Material, MeshMaterial3d, StandardMaterial};
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    BlendComponent, BlendFactor, BlendOperation, BlendState, ColorTargetState, CompareFunction,
    DepthBiasState, DepthStencilState, Face, FragmentState, MultisampleState, PrimitiveState,
    RenderPipelineDescriptor, StencilFaceState, StencilState, TextureFormat, VertexState,
};
use bevy::transform::prelude::Transform;
use bevy::window::{Window, WindowResolution};

use cs_app::render::additive::{ADDITIVE_FRAGMENT_SHADER, AdditiveMaterial};
use cs_app::render::batch::{InstanceVisuals, SubmittedDraw, batch_frame};
use cs_app::render::bevy_state::{MaterialKind, render_state};
use cs_app::render::capture::{
    ComparisonSettings, Projection, SceneOutcome, SceneSurface, capture, upload_surface,
};
use cs_app::render::material::{
    AddressMode, ClassifiedMaterial, Coverage, DeclaredClass, MaterialClass, MaterialFacts,
    RenderPhase, TextureAddress, classify,
};
use cs_app::render::plan::{DrawItem, DrawItemKey, DrawPlan, SceneView};
use cs_app::render::profile::RenderProfile;
use cs_app::render::sync::{
    BatchDraw, RenderProfileRequest, RenderSession, process_render_profile_request, sync_frame,
};
use cs_types::Tick;
use cs_types::evidence::ClaimStatus;

use super::fixture::{ImageShape, QuadShape, decoded_image, quad_mesh};

/// The tick the frames in this file are at.
const TICK: Tick = Tick(9_101);

/// The render session the consumer test opens.
const SESSION: RenderSession = RenderSession(21);

/// The blend the additive class is *specified* to draw with: `One`/`One`.
///
/// Written out here rather than taken from the production `ADDITIVE` constant so
/// the assertions below compare the material against the blend the stage
/// promised, not against whatever the code happens to hold.
const ONE_OVER_ONE: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
};

const REPEAT: TextureAddress = TextureAddress {
    u: AddressMode::Repeat,
    v: AddressMode::Repeat,
};

/// One classified surface, authored here.
///
/// The alpha test follows the class: a masked surface must declare a threshold
/// (`classify` refuses one that does not) and no other class declares one.
fn classified(class: MaterialClass, coverage: Coverage, two_sided: bool) -> ClassifiedMaterial {
    let facts = MaterialFacts {
        declared: Some(
            DeclaredClass::new(class, ClaimStatus::Designed).expect("Designed asserts a class"),
        ),
        coverage,
        alpha_test: if class == MaterialClass::Masked {
            cs_formats::texture::AlphaTest::Threshold(0x80)
        } else {
            cs_formats::texture::AlphaTest::Disabled
        },
        two_sided: Some(two_sided),
        addressing: Some(REPEAT),
        vertex_colors: false,
        unknown_flag_bits: 0,
    };
    match classify(&facts) {
        cs_app::render::material::Classification::Classified(material) => material,
        cs_app::render::material::Classification::Unclassified { reasons } => {
            panic!("the authored {class} surface stopped classifying: {reasons:?}")
        }
    }
}

/// The render state of an authored surface.
fn state_of(
    class: MaterialClass,
    coverage: Coverage,
    two_sided: bool,
) -> cs_app::render::bevy_state::RenderState {
    render_state(&classified(class, coverage, two_sided))
        .expect("the authored facts are established")
}

/// The additive class yields a drawable material, and its blend, depth write and
/// cull face are the ones the render state already records.
///
/// This is the gap-closing assertion: F17-B refused an additive surface with
/// `MaterialGap::AdditiveBlendState` because no `StandardMaterial` reaches
/// `One`/`One`. The material now exists, and a second table of "what additive
/// means" would be the failure mode this checks against — the material's blend
/// must be *the state's* value.
#[test]
fn accept_f17_c_additive_the_additive_class_yields_a_drawable_one_over_one_material() {
    let state = state_of(MaterialClass::Additive, Coverage::Opaque, false);

    // The state is unchanged by this stage: the blend and the depth write were
    // already recorded, and the material is built from them.
    assert_eq!(
        *state.blend(),
        ONE_OVER_ONE,
        "the additive class's render state still records One/One"
    );
    assert!(!state.depth_write(), "and still records no depth write");

    let material = state.to_drawable_material();
    assert_eq!(
        material.kind(),
        MaterialKind::Additive,
        "the additive class is drawn with its own material, not a StandardMaterial"
    );
    assert!(
        material.standard().is_none(),
        "a StandardMaterial cannot express One/One, so it must not be what the \
         additive class draws with"
    );
    let additive = material
        .additive()
        .expect("the additive class has a drawable material now");
    assert_eq!(
        *additive.blend(),
        *state.blend(),
        "the material's blend is the one the state records, copied — not a second table"
    );
    assert_eq!(
        ONE_OVER_ONE.color.src_factor,
        BlendFactor::One,
        "and it is really One/One, not an alpha blend with a different name"
    );
    assert_eq!(ONE_OVER_ONE.color.dst_factor, BlendFactor::One);
    assert_eq!(ONE_OVER_ONE.alpha.src_factor, BlendFactor::One);
    assert_eq!(ONE_OVER_ONE.alpha.dst_factor, BlendFactor::One);

    assert_eq!(
        additive.depth_write(),
        state.depth_write(),
        "the material writes depth exactly as the state records: not at all"
    );
    assert!(
        !additive.depth_write(),
        "an additive surface that wrote depth would occlude what is behind it"
    );
    assert_eq!(
        additive.cull_face(),
        state.cull_face(),
        "and culls exactly the face the state recorded"
    );

    // The two-sided decision still decides the cull face through this material
    // as it does through a `StandardMaterial`, which is what "reuse the state"
    // has to mean for a material of its own.
    let two_sided = state_of(MaterialClass::Additive, Coverage::Opaque, true);
    let two_sided = two_sided
        .to_drawable_material()
        .additive()
        .expect("a two-sided additive surface still has the material")
        .clone();
    assert_eq!(
        two_sided.cull_face(),
        None,
        "a two-sided additive surface culls nothing"
    );
    assert_eq!(
        *two_sided.blend(),
        ONE_OVER_ONE,
        "and its blend is the same recorded state"
    );
}

/// The material's recorded decisions reach the pipeline: the specialization
/// writes the blend into the fragment targets and the depth write into the
/// depth state.
///
/// A material that merely *carried* a blend would pass the test above and draw
/// with whatever the base pipeline chose for `AlphaMode::Blend` — which is
/// `ALPHA_BLENDING`, not `One`/`One`. That is the failure this test exists for,
/// so the descriptor starts in exactly the state `AlphaMode::Blend` would leave
/// it in and must come out additive.
#[test]
fn accept_f17_c_additive_the_material_specializes_its_blend_and_depth_write_into_the_pipeline() {
    let state = state_of(MaterialClass::Additive, Coverage::Opaque, false);
    let material = state
        .to_drawable_material()
        .additive()
        .expect("the additive class has its own material")
        .clone();

    // The material asks for the transparent pass, so the base pipeline has
    // already written `ALPHA_BLENDING` and `depth_write_enabled: false`. The
    // blend is what has to be replaced.
    let mut descriptor = descriptor_with_alpha_blend();
    assert_eq!(
        *descriptor
            .fragment
            .as_ref()
            .expect("a mesh pipeline has a fragment stage")
            .targets[0]
            .as_ref()
            .expect("a color target")
            .blend
            .as_ref()
            .expect("AlphaMode::Blend selects a blend"),
        BlendState::ALPHA_BLENDING,
        "the descriptor starts where AlphaMode::Blend leaves it"
    );

    material.pipeline_key().apply(&mut descriptor);

    for target in descriptor
        .fragment
        .as_ref()
        .expect("a mesh pipeline has a fragment stage")
        .targets
        .iter()
        .flatten()
    {
        assert_eq!(
            target.blend,
            Some(ONE_OVER_ONE),
            "the specialized target blends One/One, not the alpha blend the \
             alpha mode alone would have selected"
        );
    }
    assert_eq!(
        descriptor
            .depth_stencil
            .as_ref()
            .expect("a mesh pipeline has a depth state")
            .depth_write_enabled,
        Some(false),
        "the additive surface does not write depth"
    );
    assert_eq!(
        descriptor.primitive.cull_mode,
        state.cull_face(),
        "the cull face is the state's"
    );

    // A two-sided surface reaches `None` here too, so the specialization is
    // where the decision lands rather than a value the material happened to
    // carry unused.
    let two_sided = state_of(MaterialClass::Additive, Coverage::Opaque, true)
        .to_drawable_material()
        .additive()
        .expect("a two-sided additive surface has the material")
        .clone();
    let mut two_sided_descriptor = descriptor_with_alpha_blend();
    two_sided.pipeline_key().apply(&mut two_sided_descriptor);
    assert_eq!(
        two_sided_descriptor.primitive.cull_mode, None,
        "a two-sided additive surface culls nothing"
    );

    // The two materials are two *pipelines*, not one pipeline drawn with the
    // wrong state: the key carries the decisions, so they specialize apart.
    let key = material.pipeline_key();
    let two_sided_key = two_sided.pipeline_key();
    assert_ne!(
        key, two_sided_key,
        "two additive materials with different cull faces are different specializations"
    );
}

/// The reported material kind follows the class, for every class — it is the
/// render state's decision exposed, not a parallel classification.
///
/// Neither frame digest carries the kind as its own byte: the class is already
/// inside the render-state digest, so such a byte could only differ where the
/// state already differs. That is a redundancy, not a claim, and the field the
/// consumer and the comparison actually read is the reported kind — which is
/// what this test pins, for all five classes and through all three
/// report sites (the material itself, the capture's surface, the batch).
#[test]
fn accept_f17_c_additive_the_reported_material_kind_follows_the_state_class() {
    for class in [
        MaterialClass::Opaque,
        MaterialClass::Masked,
        MaterialClass::Blended,
        MaterialClass::Additive,
        MaterialClass::Emissive,
    ] {
        let coverage = match class {
            MaterialClass::Masked | MaterialClass::Blended => {
                Coverage::Texture(cs_formats::texture::AlphaSource::Channel)
            }
            _ => Coverage::Opaque,
        };
        let state = state_of(class, coverage, false);
        let expected = match class {
            MaterialClass::Additive => MaterialKind::Additive,
            _ => MaterialKind::Standard,
        };
        assert_eq!(
            state.to_drawable_material().kind(),
            expected,
            "{class}'s state produces the {expected} material"
        );

        // The same fact as the capture and the batcher report it, so a consumer
        // reading either one is reading the class's own decision.
        let scene = one_item_scene(class, coverage);
        let plan = DrawPlan::build(&scene.items, &scene.view);
        let outcomes = scene.outcomes();
        let captured = capture(
            &outcomes,
            &plan,
            &scene.view,
            &Projection::comparison(),
            TICK,
            &ComparisonSettings::comparison(),
        )
        .expect("the one-item scene captures");
        assert_eq!(
            captured
                .surfaces()
                .next()
                .expect("one surface")
                .material_kind(),
            expected,
            "the capture reports {class}'s material kind"
        );
        let frame = batch_frame(
            &scene.submitted(&outcomes),
            &plan,
            &InstanceVisuals::new(),
            &RenderProfile::faithful(),
            TICK,
        )
        .expect("the one-item scene batches");
        assert_eq!(
            frame.batches()[0].material_kind(),
            expected,
            "and the batcher reports it too"
        );
    }
}

/// The shader is a real file at the path the material names, and it is the
/// material's `Material::fragment_shader` that names it.
///
/// A WGSL string in Rust, or a file no material loads, would both pass a test
/// that only checked the path was well-formed. This one reads the file, so
/// removing it fails.
#[test]
fn accept_f17_c_additive_the_material_loads_a_wgsl_shader_that_exists() {
    let bevy::shader::ShaderRef::Path(path) = <AdditiveMaterial as Material>::fragment_shader()
    else {
        panic!("the additive material's fragment shader is not an asset path")
    };
    assert_eq!(
        path.path(),
        ADDITIVE_FRAGMENT_SHADER,
        "the material's shader is the path this stage recorded"
    );

    let file = shader_path();
    let source = std::fs::read_to_string(&file)
        .unwrap_or_else(|error| panic!("{} must exist: {error}", file.display()));
    assert!(
        source.contains("fn fragment("),
        "the file declares the one fragment entry point the render pipeline resolves"
    );
    assert!(
        source.contains("@fragment"),
        "and it is a fragment entry point, not a vertex one: the vertex stage is \
         the engine's own mesh shader"
    );
    assert!(
        !source.contains("fn vertex("),
        "a second entry point in the same file would make the pipeline's \
         single-entry-point resolution ambiguous"
    );
    for binding in ["binding(0)", "binding(1)", "binding(2)"] {
        assert!(
            source.contains(binding),
            "the shader binds {binding}, which the AsBindGroup layout on \
             AdditiveMaterial generates"
        );
    }
    assert!(
        source.contains("MATERIAL_BIND_GROUP"),
        "the material's bind group is at the index the engine substitutes, not a \
         hard-coded one"
    );
}

/// The shader file is the only thing in `crates/cs_app/assets/shaders/`, and
/// the material is the thing that loads it — the two are checked together
/// because either alone is a claim nothing backs.
///
/// The path is read from the production constant, so this cannot pass against a
/// file the material does not use, and it fails if the file is renamed, moved or
/// deleted.
#[test]
fn accept_f17_c_additive_the_shader_file_is_the_one_the_additive_material_uses() {
    let directory = shader_path()
        .parent()
        .expect("the shader path has a parent directory")
        .to_path_buf();
    let mut present: Vec<String> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("{} must exist: {error}", directory.display()))
        .map(|entry| entry.expect("a readable directory entry").file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    present.sort();
    assert_eq!(
        present,
        vec![
            std::path::Path::new(ADDITIVE_FRAGMENT_SHADER)
                .file_name()
                .expect("the shader path names a file")
                .to_string_lossy()
                .into_owned()
        ],
        "the additive material's shader is the one file in assets/shaders/, so \
         there is no unconsumed shader beside it"
    );
}

/// The additive pass reaches the ECS: a batch of the additive class is spawned
/// with `MeshMaterial3d<AdditiveMaterial>` carrying the recorded state, and its
/// per-instance rows are placed like any other batch's.
///
/// F17-C's consumer spawned *nothing* for an additive batch and counted it in
/// `FrameSync::unmaterialed`. That counter no longer exists, so this is the
/// positive statement of what replaced it.
#[test]
fn accept_f17_c_additive_an_additive_batch_is_spawned_with_its_own_material_and_image() {
    let scene = additive_scene();
    let plan = DrawPlan::build(&scene.items, &scene.view);
    let outcomes = scene.outcomes();
    let submitted = scene.submitted(&outcomes);
    let frame = batch_frame(
        &submitted,
        &plan,
        &InstanceVisuals::new(),
        &RenderProfile::faithful(),
        TICK,
    )
    .expect("the additive scene batches");

    let additive = frame
        .batches()
        .iter()
        .find(|batch| batch.phase() == RenderPhase::Additive)
        .expect("the additive batch is in the frame");
    assert_eq!(
        additive.material_kind(),
        MaterialKind::Additive,
        "and it draws with the additive material"
    );

    let mut world = render_world();
    world.insert_resource(RenderProfileRequest::set(
        SESSION,
        RenderProfile::faithful(),
    ));
    process_render_profile_request(&mut world);
    let report = sync_frame(&mut world, &submitted, &frame, SESSION).expect("the frame syncs");

    // Three draws: the additive sprite, the opaque ground and the masked
    // fence. Nothing is skipped for want of a material any more.
    assert_eq!(report.spawned, 3, "every batch in the frame is spawned");
    assert_eq!(report.placed, 3, "every row is placed");
    assert_eq!(report.withheld, 0);
    assert_eq!(report.released, 0);

    let entity = world
        .iter_entities()
        .find(|entity| {
            entity.get::<BatchDraw>().is_some_and(|draw| {
                draw.phase() == RenderPhase::Additive
                    && draw.key() == cs_app::render::sync::batch_key(additive)
            })
        })
        .map(|entity| entity.id())
        .expect("the additive batch has an entity");
    assert!(
        world
            .get::<MeshMaterial3d<AdditiveMaterial>>(entity)
            .is_some(),
        "it draws with MeshMaterial3d<AdditiveMaterial>"
    );
    assert!(
        world
            .get::<MeshMaterial3d<StandardMaterial>>(entity)
            .is_none(),
        "and not with a StandardMaterial as well: one entity, one material type"
    );

    // The stored material is the one the render state produced, with the
    // image bound.
    let handle = world
        .get::<MeshMaterial3d<AdditiveMaterial>>(entity)
        .expect("the additive material component")
        .0
        .clone();
    let stored = world
        .resource::<Assets<AdditiveMaterial>>()
        .get(&handle)
        .expect("the handle resolves to a stored material");
    assert_eq!(
        *stored.blend(),
        ONE_OVER_ONE,
        "the stored material draws with One/One"
    );
    assert!(!stored.depth_write(), "and writes no depth");
    let image = stored
        .base_color_texture()
        .expect("the textured additive surface binds its image")
        .clone();
    let texture = world
        .resource::<Assets<Image>>()
        .get(&image)
        .expect("the handle resolves to stored texels");
    assert_eq!(texture.width(), 2, "the canonical image reached the store");

    // The row is placed at its own place, sharing the batch's one material.
    let children: Vec<_> = world
        .iter_entities()
        .filter(|child| {
            child
                .get::<bevy::ecs::hierarchy::ChildOf>()
                .is_some_and(|parent| parent.parent() == entity)
        })
        .map(|child| child.id())
        .collect();
    assert_eq!(children.len(), 1, "one placed draw per row");
    let child = children[0];
    assert_eq!(
        world.get::<MeshMaterial3d<AdditiveMaterial>>(child),
        world.get::<MeshMaterial3d<AdditiveMaterial>>(entity),
        "the placement shares the batch's one material"
    );
    assert_eq!(
        world
            .get::<Transform>(child)
            .expect("a placement is placed")
            .translation
            .to_array(),
        [0.3, 0.0, -1.6],
        "at the sprite's own recorded place"
    );

    // A re-sync reuses the entity and grows no store: the additive material is
    // a function of the batch key, so a stable frame must not accumulate one
    // material per frame either.
    let materials_before = world.resource::<Assets<AdditiveMaterial>>().len();
    let images_before = world.resource::<Assets<Image>>().len();
    let repeat = sync_frame(&mut world, &submitted, &frame, SESSION).expect("the frame resyncs");
    assert_eq!(repeat.spawned, 0);
    assert_eq!(repeat.reused, 3);
    assert_eq!(repeat.placed, 3);
    assert_eq!(
        world.resource::<Assets<AdditiveMaterial>>().len(),
        materials_before,
        "a reused batch keeps its material instead of orphaning one per frame"
    );
    assert_eq!(
        world.resource::<Assets<Image>>().len(),
        images_before,
        "and keeps its image handle"
    );

    // A world without the additive store is refused rather than drawn with the
    // other three classes' materials: a missing store is a refusal, not a
    // half-written frame.
    let mut bare = render_world();
    bare.remove_resource::<Assets<AdditiveMaterial>>();
    bare.insert_resource(RenderProfileRequest::set(
        SESSION,
        RenderProfile::faithful(),
    ));
    process_render_profile_request(&mut bare);
    assert_eq!(
        sync_frame(&mut bare, &submitted, &frame, SESSION)
            .expect_err("the additive store is required")
            .code(),
        "no_asset_store",
    );
}

/// The other four classes are unaffected: they still draw with a
/// `StandardMaterial`, still get their own alpha mode, and still do not get the
/// additive material's shader or blend.
///
/// A material whose specialization leaked into every other class would turn the
/// whole frame additive; this is the check that the additive path is additive
/// and nothing else is.
#[test]
fn accept_f17_c_additive_a_non_additive_class_is_unaffected() {
    for (class, coverage, two_sided) in [
        (MaterialClass::Opaque, Coverage::Opaque, false),
        (
            MaterialClass::Masked,
            Coverage::Texture(cs_formats::texture::AlphaSource::Channel),
            true,
        ),
        (MaterialClass::Blended, Coverage::Uniform(102), false),
        (MaterialClass::Emissive, Coverage::Opaque, false),
    ] {
        let state = state_of(class, coverage, two_sided);
        let material = state.to_drawable_material();
        assert_eq!(
            material.kind(),
            MaterialKind::Standard,
            "{class} is still drawn with a StandardMaterial"
        );
        assert!(
            material.additive().is_none(),
            "{class} does not get the additive material"
        );
        let standard = material
            .standard()
            .unwrap_or_else(|| panic!("{class} has a StandardMaterial"));
        assert_eq!(standard.alpha_mode, *state.alpha_mode(), "{class}'s mode");
        assert_eq!(standard.cull_mode, state.cull_face(), "{class}'s cull face");
        assert_eq!(standard.unlit, state.unlit(), "{class}'s lighting");
    }

    // Specifically: a blended surface is *not* additive, and its own blend
    // survives. This is the case a "fix" that mapped every translucent class to
    // the additive material would break.
    let blended = state_of(MaterialClass::Blended, Coverage::Uniform(102), false);
    let blend = *blended.blend();
    assert_eq!(blend.color.src_factor, BlendFactor::SrcAlpha);
    assert_eq!(
        blend.color.dst_factor,
        BlendFactor::OneMinusSrcAlpha,
        "the blended class still blends by its own source alpha"
    );
    let standard = blended
        .to_drawable_material()
        .standard()
        .expect("a blended surface has a StandardMaterial")
        .clone();
    assert_eq!(standard.alpha_mode, AlphaMode::Blend);
    assert_eq!(
        standard.base_color,
        bevy::color::Color::srgba(1.0, 1.0, 1.0, 102.0_f32 / 255.0),
        "and its declared constant coverage still reaches the material"
    );

    // The additive class's own alpha mode is the pass, not the blend, and the
    // frame records the two classes as different draws.
    let scene = additive_scene();
    let plan = DrawPlan::build(&scene.items, &scene.view);
    let outcomes = scene.outcomes();
    let settings = ComparisonSettings::comparison();
    let frame = capture(
        &outcomes,
        &plan,
        &scene.view,
        &Projection::comparison(),
        TICK,
        &settings,
    )
    .expect("the additive scene captures");
    let kinds: Vec<(RenderPhase, MaterialKind)> = frame
        .surfaces()
        .map(|surface| (surface.phase(), surface.material_kind()))
        .collect();
    assert_eq!(
        kinds,
        vec![
            (RenderPhase::Opaque, MaterialKind::Standard),
            (RenderPhase::Masked, MaterialKind::Standard),
            (RenderPhase::Additive, MaterialKind::Additive),
        ],
        "the capture records which material each surface is drawn with, so the \
         additive pass is visible in the frame's identity rather than reported \
         as a gap"
    );
}

/// The material is an asset with a stable type identity, because a
/// `MeshMaterial3d<AdditiveMaterial>` in the world and the store the renderer
/// reads are the same asset only if the type is named.
///
/// This is a small assertion, but it is the one that makes the ECS wiring
/// meaningful: an unregistered type would still compile here and still hold a
/// handle, and would draw nothing at all.
#[test]
fn accept_f17_c_additive_the_additive_material_is_a_named_asset_the_ecs_can_hold() {
    assert_eq!(
        AdditiveMaterial::type_path(),
        "cs_app::render::additive::AdditiveMaterial",
        "the material is registered under its own stable type path"
    );
    let mut world = World::new();
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<AdditiveMaterial>::default());
    let handle = world
        .resource_mut::<Assets<AdditiveMaterial>>()
        .add(AdditiveMaterial {
            color: bevy::color::LinearRgba::WHITE,
            base_color_texture: None,
            blend: ONE_OVER_ONE,
            depth_write: false,
            cull_face: Some(Face::Back),
        });
    let mesh = world.resource_mut::<Assets<Mesh>>().add(Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    ));
    let entity = world
        .spawn((Mesh3d(mesh), MeshMaterial3d(handle.clone())))
        .id();
    assert_eq!(
        world.get::<MeshMaterial3d<AdditiveMaterial>>(entity),
        Some(&MeshMaterial3d(handle)),
        "the world holds the material the store owns"
    );
}

/// The additive class's `alpha_mode` is the pass, and a `One`/`One` blend is not
/// reachable by it — the reason the class has its own material.
///
/// Bevy 0.19 maps `AlphaMode::Add` onto the premultiplied-alpha pipeline, so a
/// `StandardMaterial` asked to add multiplies the source by its own alpha
/// instead. That is an engine fact, asserted here from the *state* side: the
/// additive state records a blend no alpha mode carries, which is what made the
/// gap real and what the custom material exists to express.
#[test]
fn accept_f17_c_additive_the_additive_class_records_a_blend_no_alpha_mode_carries() {
    // Every blend the four alpha modes a `StandardMaterial` can express map to,
    // so the additive state's blend is provably not one of them.
    for expressed in [
        None,
        Some(BlendState::REPLACE),
        Some(BlendState::ALPHA_BLENDING),
        Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
    ] {
        assert_ne!(
            expressed,
            Some(ONE_OVER_ONE),
            "One/One is not reachable through an alpha mode, which is why the \
             class needs a material of its own"
        );
    }

    // And the material covers the case instead: it is a `Material`, it is in the
    // transparent pass, and it carries the blend.
    let state = state_of(MaterialClass::Additive, Coverage::Opaque, false);
    let material = state
        .to_drawable_material()
        .additive()
        .expect("the additive material")
        .clone();
    assert_eq!(
        material.alpha_mode(),
        AlphaMode::Blend,
        "the additive surface is in the sorted transparent pass"
    );
    assert_eq!(
        *material.blend(),
        ONE_OVER_ONE,
        "and the blend it draws with is its own"
    );
    // A declared constant coverage on an additive surface scales the
    // contribution, because an One/One blend has no blend factor to read it
    // with. The alpha is the declared byte, normalized and not re-encoded.
    let translucent = state_of(MaterialClass::Additive, Coverage::Uniform(102), false)
        .to_drawable_material()
        .additive()
        .expect("the additive material")
        .clone();
    assert!(
        translucent.color.alpha > 0.0 && translucent.color.alpha < 1.0,
        "a declared 102/255 coverage reaches the additive material as a partial alpha"
    );
    assert!(
        (translucent.color.alpha - 102.0_f32 / 255.0).abs() < 1e-6,
        "exactly the stored byte over 255, which is what the standard path does"
    );
    assert_eq!(
        translucent.color.red, 1.0,
        "and no tint is invented: the color channels stay the identity multiplier"
    );
}

/// The material does not reorder the additive pass, and the reason the additive
/// pass is where it is stays the phase order rather than a coincidence.
///
/// The class's whole point in the plan is that it draws after all translucency.
/// This is checked in the two ways that can go wrong: a *capture* must list the
/// additive surface after both panes while a *frame digest* must change if the
/// class moved out of the additive phase. The second is the load-bearing part —
/// without it, a plan that quietly sorted the sprite among the panes would
/// still produce a capture whose first four entries looked right.
#[test]
fn accept_f17_c_additive_the_additive_pass_is_drawn_after_the_translucency_before_it() {
    let scene = translucent_scene();
    let plan = DrawPlan::build(&scene.items, &scene.view);
    let phases: Vec<(RenderPhase, f32)> = plan
        .entries()
        .iter()
        .map(|entry| (entry.phase, entry.depth_m))
        .collect();
    let order: Vec<&str> = phases.iter().map(|(phase, _)| phase.code()).collect();
    assert_eq!(
        order,
        vec!["opaque", "translucent", "translucent", "additive"],
        "the additive pass still draws after both translucent surfaces"
    );
    let translucent_depths: Vec<f32> = phases
        .iter()
        .filter(|(phase, _)| *phase == RenderPhase::Translucent)
        .map(|(_, depth)| *depth)
        .collect();
    let mut sorted = translucent_depths.clone();
    sorted.sort_by(|a, b| b.total_cmp(a));
    assert_eq!(
        translucent_depths, sorted,
        "and the translucency in front of it is still sorted back-to-front"
    );
    // The sprite sits *between* the two panes in depth: nearer than the far
    // pane, farther than the near one. So it is drawn after both, and a plan
    // that sorted the whole translucent tail by depth alone — or that ignored
    // the phase order — would put it between them.
    let additive_depth = phases
        .iter()
        .find(|(phase, _)| *phase == RenderPhase::Additive)
        .expect("the additive entry is in the plan")
        .1;
    let (nearest_pane, farthest_pane) = (
        *translucent_depths.last().expect("a nearer pane"),
        translucent_depths[0],
    );
    assert!(
        nearest_pane < additive_depth && additive_depth < farthest_pane,
        "the additive surface is between the two panes in depth ({nearest_pane} < {additive_depth} < {farthest_pane}), so drawing it last is a phase decision, not a depth sort"
    );

    // The capture agrees with the plan, and the frame's identity depends on the
    // phase: the additive surface is listed last, and a plan that drew it among
    // the panes would be a different frame even though every surface in it
    // would be the same surface.
    let outcomes = scene.outcomes();
    let captured = capture(
        &outcomes,
        &plan,
        &scene.view,
        &Projection::comparison(),
        TICK,
        &ComparisonSettings::comparison(),
    )
    .expect("the translucent scene captures");
    let captured_phases: Vec<RenderPhase> =
        captured.surfaces().map(|surface| surface.phase()).collect();
    assert_eq!(
        captured_phases,
        vec![
            RenderPhase::Opaque,
            RenderPhase::Translucent,
            RenderPhase::Translucent,
            RenderPhase::Additive,
        ],
        "the capture keeps the plan's order, and the additive surface is last"
    );
    let frame = batch_frame(
        &scene.submitted(&outcomes),
        &plan,
        &InstanceVisuals::new(),
        &RenderProfile::faithful(),
        TICK,
    )
    .expect("the translucent scene batches");
    let batched_phases: Vec<RenderPhase> =
        frame.batches().iter().map(|batch| batch.phase()).collect();
    assert_eq!(
        batched_phases, captured_phases,
        "the batcher keeps the same order, so the additive batch is the last draw"
    );
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A scene with one surface of each of three classes: an additive sprite that
/// samples an image, an opaque ground and an alpha-cut fence.
struct Scene {
    items: Vec<DrawItem>,
    view: SceneView,
    textures: Vec<Option<cs_formats::texture::DecodedImage>>,
}

impl Scene {
    /// The scene's own outcomes, uploaded through the production adapters.
    ///
    /// Uploaded once and owned by the fixture, because a `SubmittedDraw` borrows
    /// its outcome: the batcher and the consumer both read the list several
    /// times, and re-uploading per call would make each pass a different
    /// allocation with the same values.
    fn outcomes(&self) -> Vec<SceneOutcome> {
        self.items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                upload_surface(&SceneSurface {
                    item,
                    mesh: &quad_mesh(QuadShape::full(), 0),
                    group: 0,
                    image: self.textures[index].as_ref(),
                    unknowns: &[],
                })
            })
            .collect()
    }

    /// The scene in submission order, against `outcomes`.
    fn submitted<'a>(&'a self, outcomes: &'a [SceneOutcome]) -> Vec<SubmittedDraw<'a>> {
        self.items
            .iter()
            .enumerate()
            .map(|(index, item)| SubmittedDraw {
                item,
                outcome: &outcomes[index],
                instance: cs_app::livery::ModelInstanceId(index as u64 + 1),
                part: cs_app::render::batch::PartRef::Unresolved("fixture"),
            })
            .collect()
    }
}

fn additive_scene() -> Scene {
    Scene {
        items: vec![
            item(
                "sprite",
                MaterialClass::Additive,
                Coverage::Texture(cs_formats::texture::AlphaSource::Channel),
                [0.3, 0.0, -1.6],
            ),
            item(
                "ground",
                MaterialClass::Opaque,
                Coverage::Opaque,
                [0.0, -0.5, -6.0],
            ),
            item(
                "fence",
                MaterialClass::Masked,
                Coverage::Texture(cs_formats::texture::AlphaSource::Channel),
                [0.0, 0.0, -3.0],
            ),
        ],
        view: SceneView::new([0.0, 0.0, 4.0], [0.0, 0.0, -1.0]).expect("a finite view"),
        textures: vec![
            Some(decoded_image(ImageShape::rgba8_srgb())),
            None,
            Some(decoded_image(ImageShape::rgba8_srgb())),
        ],
    }
}

/// A scene with exactly one surface, of `class`.
fn one_item_scene(class: MaterialClass, coverage: Coverage) -> Scene {
    Scene {
        items: vec![item("only", class, coverage, [0.0, 0.0, -2.0])],
        view: SceneView::new([0.0, 0.0, 4.0], [0.0, 0.0, -1.0]).expect("a finite view"),
        textures: vec![Some(decoded_image(ImageShape::rgba8_srgb()))],
    }
}

/// A scene with one opaque, two translucent and one additive surface, at four
/// distinct view depths, so the phase order and the depth sort are both
/// exercised.
fn translucent_scene() -> Scene {
    Scene {
        items: vec![
            item(
                "sprite",
                MaterialClass::Additive,
                Coverage::Opaque,
                [0.0, 0.0, -1.6],
            ),
            item(
                "glass_near",
                MaterialClass::Blended,
                Coverage::Uniform(102),
                [0.0, 0.0, -1.0],
            ),
            item(
                "ground",
                MaterialClass::Opaque,
                Coverage::Opaque,
                [0.0, -0.5, -6.0],
            ),
            item(
                "glass_far",
                MaterialClass::Blended,
                Coverage::Uniform(102),
                [0.0, 0.0, -2.2],
            ),
        ],
        view: SceneView::new([0.0, 0.0, 4.0], [0.0, 0.0, -1.0]).expect("a finite view"),
        textures: vec![None, None, None, None],
    }
}

fn item(
    key: &'static str,
    class: MaterialClass,
    coverage: Coverage,
    center_m: [f32; 3],
) -> DrawItem {
    DrawItem::new(
        DrawItemKey::new(key).expect("an authored key is valid"),
        classified(class, coverage, false),
        center_m,
        None,
    )
    .expect("authored geometry is finite")
}

/// A mesh-pipeline descriptor in the state `AlphaMode::Blend` leaves it in.
///
/// Built by hand rather than through the engine's `MeshPipeline` (which needs
/// a render device), so the assertions are about *where the blend lives*: in
/// wgpu 29 a blend state is a field of each `ColorTargetState`, not of
/// `PrimitiveState`, and the additive material has to write it there.
fn descriptor_with_alpha_blend() -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        vertex: VertexState::default(),
        fragment: Some(FragmentState {
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rgba8UnormSrgb,
                blend: Some(BlendState::ALPHA_BLENDING),
                write_mask: bevy::render::render_resource::ColorWrites::ALL,
            })],
            ..FragmentState::default()
        }),
        primitive: PrimitiveState {
            cull_mode: Some(Face::Back),
            ..PrimitiveState::default()
        },
        depth_stencil: Some(DepthStencilState {
            format: TextureFormat::Depth32Float,
            depth_write_enabled: Some(false),
            depth_compare: Some(CompareFunction::GreaterEqual),
            stencil: StencilState {
                front: StencilFaceState::IGNORE,
                back: StencilFaceState::IGNORE,
                read_mask: 0,
                write_mask: 0,
            },
            bias: DepthBiasState::default(),
        }),
        multisample: MultisampleState::default(),
        ..RenderPipelineDescriptor::default()
    }
}

/// The world the consumer test syncs into: the asset stores, and the camera,
/// light and window the presentation reaches.
fn render_world() -> World {
    let mut world = World::new();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<StandardMaterial>::default());
    world.insert_resource(Assets::<AdditiveMaterial>::default());
    world.spawn((
        Transform::default(),
        bevy::render::view::Msaa::Sample4,
        bevy::core_pipeline::tonemapping::Tonemapping::TonyMcMapface,
    ));
    world.spawn(bevy::light::DirectionalLight::default());
    world.spawn(Window {
        resolution: WindowResolution::new(1280, 720),
        ..Window::default()
    });
    world
}

/// The additive shader's file, resolved inside this crate's asset root.
fn shader_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join(ADDITIVE_FRAGMENT_SHADER)
}
