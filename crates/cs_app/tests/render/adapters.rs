//! `accept_f17_b_` tests for the canonical-to-Bevy adapters:
//! `cs_app::render::bevy_mesh`, `cs_app::render::bevy_image`,
//! `cs_app::render::bevy_state` and `cs_app::render::capture::upload_surface`.
//!
//! The stage's slice is "the smallest production path that exercises the
//! declared behavior". What is pinned here is that the adapters are
//! *bit-exact* and *refusing*: stored values reach the Bevy buffers
//! unchanged, and a fact nothing established becomes a reason code instead of
//! a value.

use bevy::color::Color;
use bevy::image::ImageAddressMode;
use bevy::material::AlphaMode;
use bevy::mesh::{Indices, Mesh, VertexAttributeValues};
use bevy::render::render_resource::{BlendFactor, BlendOperation, Face, TextureFormat};
use cs_app::render::bevy_image::{CoveragePlane, ImageAdapterError, upload_image};
use cs_app::render::bevy_mesh::{AttributeKind, MeshAdapterError, upload_group, upload_groups};
use cs_app::render::bevy_state::{MaterialGap, StateError, render_state};
use cs_app::render::capture::{SceneSurface, surface_codes, upload_surface};
use cs_app::render::material::{
    AddressMode, Classification, ClassifiedMaterial, Coverage, DeclaredClass, MaterialClass,
    MaterialFacts, TextureAddress, classify,
};
use cs_app::render::plan::{DrawItem, DrawItemKey};
use cs_formats::texture::{AlphaSource, AlphaTest, ColorSpace, PixelFormat};
use cs_types::evidence::ClaimStatus;

use super::fixture::{
    IMAGE_TEXELS, MESH_UNKNOWNS, QUAD_COLORS, QUAD_NORMALS, QUAD_POSITIONS, QUAD_UVS, QuadShape,
    decoded_image, degenerate_strip_mesh, quad_mesh,
};

const REPEAT: TextureAddress = TextureAddress {
    u: AddressMode::Repeat,
    v: AddressMode::Repeat,
};

/// A classified material, built through the production classifier.
fn material(
    class: MaterialClass,
    coverage: Coverage,
    alpha_test: AlphaTest,
    two_sided: Option<bool>,
    addressing: Option<TextureAddress>,
    vertex_colors: bool,
) -> ClassifiedMaterial {
    let facts = MaterialFacts {
        declared: Some(DeclaredClass::new(class, ClaimStatus::Designed).expect("Designed asserts")),
        coverage,
        alpha_test,
        two_sided,
        addressing,
        vertex_colors,
        unknown_flag_bits: 0,
    };
    match classify(&facts) {
        Classification::Classified(material) => material,
        Classification::Unclassified { reasons } => panic!("authored material: {reasons:?}"),
    }
}

fn masked(two_sided: Option<bool>, addressing: Option<TextureAddress>) -> ClassifiedMaterial {
    material(
        MaterialClass::Masked,
        Coverage::Texture(AlphaSource::Channel),
        AlphaTest::Threshold(0x80),
        two_sided,
        addressing,
        false,
    )
}

fn item(key: &str, material: ClassifiedMaterial, center_m: [f32; 3]) -> DrawItem {
    DrawItem::new(
        DrawItemKey::new(key).expect("authored key"),
        material,
        center_m,
        None,
    )
    .expect("authored geometry is finite")
}

/// A total order over three `f32` bit patterns, so a set comparison is
/// exact and never depends on float equality.
fn order<const N: usize>(point: [f32; N]) -> [u32; N] {
    point.map(f32::to_bits)
}

fn positions_of(mesh: &Mesh) -> Vec<[f32; 3]> {
    match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(values)) => values.clone(),
        other => panic!("positions are Float32x3, got {other:?}"),
    }
}

fn vectors_of(mesh: &Mesh, attribute: MeshVertexAttribute<'_>) -> Vec<[f32; 3]> {
    match mesh.attribute(attribute) {
        Some(VertexAttributeValues::Float32x3(values)) => values.clone(),
        other => panic!("attribute is Float32x3, got {other:?}"),
    }
}

fn uvs_of(mesh: &Mesh) -> Vec<[f32; 2]> {
    match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(VertexAttributeValues::Float32x2(values)) => values.clone(),
        other => panic!("uvs are Float32x2, got {other:?}"),
    }
}

fn colors_of(mesh: &Mesh) -> Vec<[f32; 4]> {
    match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
        Some(VertexAttributeValues::Float32x4(values)) => values.clone(),
        other => panic!("colors are Float32x4, got {other:?}"),
    }
}

/// A four-component attribute id, spelled so the helper above takes Bevy's
/// own associated constants without importing every attribute type.
type MeshVertexAttribute<'a> = bevy::mesh::MeshVertexAttribute;

/// One uploaded vertex, all four attributes together.
type UploadedVertex = ([f32; 3], [f32; 3], [f32; 2], [f32; 4]);

fn texels(image: &bevy::image::Image) -> Vec<[u8; 4]> {
    let data = image.data.as_ref().expect("an upload has CPU-side data");
    data.as_chunks::<4>()
        .0
        .iter()
        .map(|texel| [texel[0], texel[1], texel[2], texel[3]])
        .collect()
}

/// The mesh adapter copies every stored value bit-exactly into Bevy buffers,
/// keeps the triangle list, and carries the presentation unknowns it was
/// handed instead of settling them.
///
/// The fixtures are chosen so that each of the three ways an adapter could
/// quietly change data shows up: a normal of length two (normalization), a
/// UV outside `0..=1` on both axes (wrapping or clamping) and four distinct
/// corner colors (a colorspace or gamma change). The uploaded vertices are
/// compared as a set, because compaction is allowed to renumber them, and the
/// drawn triangles are then compared through the index buffer — so "nothing
/// changed" is checked at the level the GPU reads it.
#[test]
fn accept_f17_b_mesh_upload_keeps_stored_values_bit_exact() {
    let render = quad_mesh(QuadShape::full(), 7);
    let upload = upload_group(&render, 0, &MESH_UNKNOWNS).expect("a full quad uploads");
    let mesh = upload.mesh();

    assert_eq!(
        upload.material(),
        7,
        "the raw stored material index rides along"
    );

    let mut uploaded: Vec<UploadedVertex> = positions_of(mesh)
        .into_iter()
        .zip(vectors_of(mesh, Mesh::ATTRIBUTE_NORMAL))
        .zip(uvs_of(mesh))
        .zip(colors_of(mesh))
        .map(|(((position, normal), uv), color)| (position, normal, uv, color))
        .collect();
    let mut expected: Vec<UploadedVertex> = QUAD_POSITIONS
        .iter()
        .zip(QUAD_NORMALS.iter())
        .zip(QUAD_UVS.iter())
        .zip(QUAD_COLORS.iter())
        .map(|(((position, normal), uv), color)| {
            (*position, *normal, *uv, [color[0], color[1], color[2], 1.0])
        })
        .collect();
    uploaded.sort_by_key(|vertex| order(vertex.0));
    expected.sort_by_key(|vertex| order(vertex.0));
    assert_eq!(
        uploaded, expected,
        "every stored position, normal, coordinate and color reaches the buffer unchanged"
    );
    assert!(
        uploaded.iter().all(|vertex| vertex.1 == [0.0, 0.0, 2.0]),
        "the unnormalized stored normal is not normalized on the way out"
    );
    let mut coordinates: Vec<[f32; 2]> = uploaded.iter().map(|vertex| vertex.2).collect();
    let mut stored_coordinates = QUAD_UVS;
    coordinates.sort_by_key(|uv| order(*uv));
    stored_coordinates.sort_by_key(|uv| order(*uv));
    assert_eq!(
        coordinates, stored_coordinates,
        "a stored coordinate outside 0..=1 is neither wrapped nor clamped"
    );

    let indices = match mesh.indices() {
        Some(Indices::U32(indices)) => indices.clone(),
        other => panic!("the index buffer is U32, got {other:?}"),
    };
    let positions = positions_of(mesh);
    // The drawn triangles, read the way the GPU reads them: three index-buffer
    // slots per triangle, each a vertex of the uploaded buffer.
    let drawn: Vec<[[f32; 3]; 3]> = indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|slots| {
            slots.map(|slot| positions[usize::try_from(slot).expect("a u32 slot fits in usize")])
        })
        .collect();
    let stored: Vec<[[f32; 3]; 3]> = render.groups()[0]
        .triangles
        .iter()
        .map(|&triangle| {
            render.triangles()[triangle]
                .vertices
                .map(|vertex| render.vertices()[vertex as usize].position)
        })
        .collect();
    assert_eq!(
        drawn, stored,
        "the index buffer draws the IR's triangles, in its order and winding"
    );

    let report = upload.report();
    assert_eq!(report.vertices, 4);
    assert_eq!(report.triangles, 2);
    assert_eq!(report.degenerate_triangles, 0);
    assert!(report.normals && report.uvs && report.colors);
    assert_eq!(
        upload.unknowns(),
        MESH_UNKNOWNS,
        "winding, uv origin and vertex-color meaning are still open"
    );
}

/// A group that stores no UV at all gets no UV buffer, and a group that
/// stores one on some corners is refused instead of being padded: a padded
/// attribute is an invented value that changes every shaded fragment.
#[test]
fn accept_f17_b_mesh_upload_refuses_a_partially_stored_attribute() {
    let bare = upload_group(&quad_mesh(QuadShape::bare(), 0), 0, &MESH_UNKNOWNS)
        .expect("a quad with no attributes uploads");
    assert!(
        !bare.report().uvs && !bare.report().normals && !bare.report().colors,
        "an absent attribute produces an absent buffer"
    );
    assert!(
        bare.mesh().attribute(Mesh::ATTRIBUTE_UV_0).is_none(),
        "and no attribute is invented"
    );
    assert!(
        bare.mesh().attribute(Mesh::ATTRIBUTE_NORMAL).is_none(),
        "a missing normal is not generated"
    );

    let partial = quad_mesh(
        QuadShape {
            normals: false,
            uvs: 2,
            colors: 0,
        },
        0,
    );
    let error =
        upload_group(&partial, 0, &MESH_UNKNOWNS).expect_err("a half-stored UV set is refused");
    assert_eq!(error.code(), "incomplete_vertex_attribute");
    assert_eq!(
        error,
        MeshAdapterError::IncompleteAttribute {
            group: 0,
            attribute: AttributeKind::Uv,
            present: 2,
            total: 4,
        }
    );

    assert_eq!(
        upload_group(&partial, 1, &MESH_UNKNOWNS)
            .expect_err("an absent group is refused")
            .code(),
        "group_out_of_range",
        "a caller cannot reach past the mesh's groups"
    );
    assert_eq!(
        upload_groups(&quad_mesh(QuadShape::full(), 3), &MESH_UNKNOWNS)
            .expect("every group of a clean mesh uploads")
            .len(),
        1
    );
}

/// A degenerate triangle stays in the index buffer and is counted. Dropping
/// it here would be a presentation decision made where nothing measured one.
#[test]
fn accept_f17_b_mesh_upload_keeps_degenerate_triangles() {
    let upload =
        upload_group(&degenerate_strip_mesh(), 0, &MESH_UNKNOWNS).expect("the strip uploads");
    let report = upload.report();
    assert_eq!(report.triangles, 3, "the strip decodes to three triangles");
    assert_eq!(
        report.degenerate_triangles, 1,
        "and the repeated position is counted"
    );
    match upload.mesh().indices() {
        Some(Indices::U32(indices)) => assert_eq!(
            indices.len(),
            9,
            "the degenerate triangle is still indexed, not culled away"
        ),
        other => panic!("the index buffer is U32, got {other:?}"),
    }
}

/// The upload's texture format comes from the stored color space, which is
/// what keeps the GPU from correcting the original's texels twice (spec F17
/// non-negotiable 3).
#[test]
fn accept_f17_b_image_upload_chooses_the_sampling_format_from_the_color_space() {
    let srgb = decoded_image(super::fixture::ImageShape {
        format: PixelFormat::Rgba8,
        alpha_source: AlphaSource::Channel,
        alpha_test: AlphaTest::Threshold(0x80),
        color_space: ColorSpace::Srgb,
    });
    let upload = upload_image(&srgb, Some(REPEAT)).expect("an sRGB image uploads");
    assert_eq!(upload.format(), TextureFormat::Rgba8UnormSrgb);
    assert_eq!(upload.color_space(), ColorSpace::Srgb);
    assert_eq!(upload.extent().width, 2);
    assert_eq!(upload.mip_levels(), 1, "no mip level is generated");
    assert_eq!(upload.coverage(), CoveragePlane::Channel);
    assert_eq!(
        upload.covered_texels(),
        2,
        "two of the four texels are covered"
    );
    assert_eq!(
        texels(upload.image()),
        vec![
            IMAGE_TEXELS[0],
            IMAGE_TEXELS[1],
            IMAGE_TEXELS[2],
            IMAGE_TEXELS[3],
        ],
        "the stored rows reach the texture in image order, top row first"
    );

    let linear = decoded_image(super::fixture::ImageShape {
        format: PixelFormat::Rgba8,
        alpha_source: AlphaSource::Channel,
        alpha_test: AlphaTest::Threshold(0x80),
        color_space: ColorSpace::Linear,
    });
    let upload = upload_image(&linear, Some(REPEAT)).expect("a linear image uploads");
    assert_eq!(
        upload.format(),
        TextureFormat::Rgba8Unorm,
        "already-linear values must not be linearized a second time"
    );
    assert_eq!(
        texels(upload.image())[3],
        IMAGE_TEXELS[3],
        "values are unchanged"
    );
}

/// A coverage plane stored after the color texels is composed into the alpha
/// channel here, once, and the composition is countable.
#[test]
fn accept_f17_b_image_upload_composes_a_separate_coverage_plane() {
    let image = decoded_image(super::fixture::ImageShape {
        format: PixelFormat::Rgb8,
        alpha_source: AlphaSource::Plane,
        alpha_test: AlphaTest::Threshold(0x10),
        color_space: ColorSpace::Srgb,
    });
    let upload = upload_image(&image, Some(REPEAT)).expect("a plane-covered image uploads");
    assert_eq!(upload.coverage(), CoveragePlane::Separate);
    let texels = texels(upload.image());
    // The stored buffer is bottom-up and the decoder flips it once, so the
    // texture reads in `IMAGE_TEXELS` order; the plane was authored with only
    // the second texel clear, and the texel whose stored alpha was 0x80 now
    // carries the plane's 255.
    assert_eq!(
        texels,
        vec![
            [
                IMAGE_TEXELS[0][0],
                IMAGE_TEXELS[0][1],
                IMAGE_TEXELS[0][2],
                255
            ],
            [
                IMAGE_TEXELS[1][0],
                IMAGE_TEXELS[1][1],
                IMAGE_TEXELS[1][2],
                0
            ],
            [
                IMAGE_TEXELS[2][0],
                IMAGE_TEXELS[2][1],
                IMAGE_TEXELS[2][2],
                255
            ],
            [
                IMAGE_TEXELS[3][0],
                IMAGE_TEXELS[3][1],
                IMAGE_TEXELS[3][2],
                255
            ],
        ],
        "the coverage plane lands in the alpha channel of the right texels"
    );
    assert_eq!(upload.covered_texels(), 1);
    assert_eq!(upload.format(), TextureFormat::Rgba8UnormSrgb);
}

/// Every fact the image adapter would have to invent is a refusal, and no
/// refusal falls back to a default.
#[test]
fn accept_f17_b_image_upload_refuses_what_it_would_have_to_guess() {
    let shape = |format, alpha_source, alpha_test, color_space| super::fixture::ImageShape {
        format,
        alpha_source,
        alpha_test,
        color_space,
    };
    let cases: [(ImageAdapterError, super::fixture::ImageShape); 6] = [
        (
            ImageAdapterError::ColorSpaceUnknown,
            shape(
                PixelFormat::Rgba8,
                AlphaSource::Opaque,
                AlphaTest::Disabled,
                ColorSpace::Unknown,
            ),
        ),
        (
            ImageAdapterError::AlphaSourceUnknown,
            shape(
                PixelFormat::Rgba8,
                AlphaSource::Unknown,
                AlphaTest::Disabled,
                ColorSpace::Srgb,
            ),
        ),
        (
            ImageAdapterError::AlphaTestUnknown,
            shape(
                PixelFormat::Rgba8,
                AlphaSource::Channel,
                AlphaTest::Unknown,
                ColorSpace::Srgb,
            ),
        ),
        (
            ImageAdapterError::Rgb565ExpansionUnknown,
            shape(
                PixelFormat::Rgb565,
                AlphaSource::Opaque,
                AlphaTest::Disabled,
                ColorSpace::Srgb,
            ),
        ),
        (
            ImageAdapterError::PaletteKeyCoverage { index: 1 },
            shape(
                PixelFormat::Indexed8,
                AlphaSource::PaletteKey { index: 1 },
                AlphaTest::Threshold(0x10),
                ColorSpace::Srgb,
            ),
        ),
        (
            ImageAdapterError::StoredValueKeyCoverage { value: 0xF800 },
            shape(
                PixelFormat::Rgb565,
                AlphaSource::StoredValueKey { value: 0xF800 },
                AlphaTest::Threshold(0x10),
                ColorSpace::Srgb,
            ),
        ),
    ];
    for (expected, shape) in cases {
        let image = decoded_image(shape);
        let error = upload_image(&image, Some(REPEAT)).expect_err("this case is refused");
        assert_eq!(error, expected);
    }

    let refusals = [
        ImageAdapterError::ColorSpaceUnknown.code(),
        ImageAdapterError::AlphaSourceUnknown.code(),
        ImageAdapterError::AlphaTestUnknown.code(),
        ImageAdapterError::Rgb565ExpansionUnknown.code(),
        ImageAdapterError::AddressModeUnknown.code(),
    ];
    assert_eq!(
        refusals,
        [
            "color_space_unknown",
            "alpha_source_unknown",
            "alpha_test_unknown",
            "rgb565_expansion_unknown",
            "address_mode_unknown",
        ],
        "every refusal has a stable reason code"
    );

    let image = decoded_image(super::fixture::ImageShape::rgba8_srgb());
    assert_eq!(
        upload_image(&image, None)
            .expect_err("no declared addressing is refused")
            .code(),
        "address_mode_unknown"
    );
}

/// The sampler's address modes are the material's declared ones, and only
/// those two.
#[test]
fn accept_f17_b_image_upload_uses_the_declared_addressing() {
    let image = decoded_image(super::fixture::ImageShape::rgba8_srgb());
    let repeat = upload_image(&image, Some(REPEAT)).expect("repeat uploads");
    let clamp = upload_image(
        &image,
        Some(TextureAddress {
            u: AddressMode::Clamp,
            v: AddressMode::Clamp,
        }),
    )
    .expect("clamp uploads");
    let descriptor =
        |upload: &cs_app::render::bevy_image::ImageUpload| match &upload.image().sampler {
            bevy::image::ImageSampler::Descriptor(descriptor) => descriptor.clone(),
            other => panic!("the upload pins its own sampler, got {other:?}"),
        };
    assert_eq!(descriptor(&repeat).address_mode_u, ImageAddressMode::Repeat);
    assert_eq!(descriptor(&repeat).address_mode_v, ImageAddressMode::Repeat);
    assert_eq!(
        descriptor(&clamp).address_mode_u,
        ImageAddressMode::ClampToEdge
    );
    assert_ne!(
        repeat.fingerprint(),
        clamp.fingerprint(),
        "a different sampler is a different upload"
    );
    assert_eq!(
        repeat.fingerprint(),
        upload_image(&image, Some(REPEAT))
            .expect("repeat uploads again")
            .fingerprint(),
        "the same input uploads to the same bytes"
    );
}

/// Each class reads out a different render state, and the two facts nothing
/// measured are refusals rather than defaults.
#[test]
fn accept_f17_b_render_state_differs_per_class_and_refuses_unmeasured_facts() {
    let opaque = render_state(&material(
        MaterialClass::Opaque,
        Coverage::Opaque,
        AlphaTest::Disabled,
        Some(false),
        Some(REPEAT),
        false,
    ))
    .expect("an opaque surface has a state");
    assert_eq!(
        *opaque.blend(),
        bevy::render::render_resource::BlendState::REPLACE
    );
    assert!(opaque.depth_write());
    assert_eq!(
        opaque.cull_face(),
        Some(Face::Back),
        "one-sided culls its back face"
    );
    assert_eq!(*opaque.alpha_mode(), AlphaMode::Opaque);
    assert!(!opaque.unlit());
    assert!(!opaque.vertex_colors());

    let fence = render_state(&masked(Some(true), Some(REPEAT))).expect("the fence has a state");
    assert_eq!(*fence.alpha_mode(), AlphaMode::Mask(128.0_f32 / 255.0));
    assert!(fence.depth_write(), "an alpha-cut surface writes depth");
    assert_eq!(fence.cull_face(), None, "a two-sided surface culls nothing");
    assert_eq!(fence.coverage(), Coverage::Texture(AlphaSource::Channel));

    let glass = render_state(&material(
        MaterialClass::Blended,
        Coverage::Uniform(102),
        AlphaTest::Disabled,
        Some(true),
        Some(REPEAT),
        false,
    ))
    .expect("the glass has a state");
    let blend = *glass.blend();
    assert_eq!(blend.color.src_factor, BlendFactor::SrcAlpha);
    assert_eq!(blend.color.dst_factor, BlendFactor::OneMinusSrcAlpha);
    assert_eq!(blend.color.operation, BlendOperation::Add);
    assert!(
        !glass.depth_write(),
        "a blended surface must not occlude the surface behind it"
    );
    assert_eq!(glass.coverage(), Coverage::Uniform(102));
    // A `StandardMaterial` takes its blend from `alpha_mode`, so the material
    // the state hands out has to say the same thing the state says. A state
    // that records a blend and a material that does not blend is the one
    // combination the rest of this module exists to prevent.
    let glass_material = glass
        .to_standard_material()
        .expect("a blended surface has a StandardMaterial");
    assert_eq!(
        glass_material.alpha_mode,
        AlphaMode::Blend,
        "the drawable material blends exactly as the state does"
    );
    assert_eq!(
        glass_material.base_color,
        Color::srgba(1.0, 1.0, 1.0, 102.0_f32 / 255.0),
        "the declared constant opacity is the coverage the blend factor reads, \
         not a value the state records and the material drops; the color \
         channels stay neutral because no tint is declared anywhere"
    );

    let emissive = render_state(&material(
        MaterialClass::Emissive,
        Coverage::Opaque,
        AlphaTest::Disabled,
        Some(false),
        Some(REPEAT),
        false,
    ))
    .expect("an emissive surface has a state");
    assert!(emissive.unlit(), "emission is not dimmed by scene lighting");

    let sprite = render_state(&material(
        MaterialClass::Additive,
        Coverage::Opaque,
        AlphaTest::Disabled,
        Some(false),
        Some(REPEAT),
        false,
    ))
    .expect("the sprite has a state");
    let blend = *sprite.blend();
    assert_eq!(blend.color.src_factor, BlendFactor::One);
    assert_eq!(blend.color.dst_factor, BlendFactor::One);
    assert!(!sprite.depth_write());
    assert_eq!(
        sprite
            .to_standard_material()
            .expect_err("no additive mode exists"),
        MaterialGap::AdditiveBlendState,
        "the state is complete even where the drawable material is not"
    );
    assert!(
        fence
            .to_standard_material()
            .expect("a masked surface has a StandardMaterial")
            .alpha_mode
            == AlphaMode::Mask(128.0_f32 / 255.0)
    );
    assert_eq!(
        opaque
            .to_standard_material()
            .expect("an opaque surface has a StandardMaterial")
            .base_color,
        Color::WHITE,
        "a class that ignores coverage keeps the neutral opacity"
    );

    assert_eq!(
        render_state(&masked(None, Some(REPEAT)))
            .expect_err("unmeasured two-sidedness is refused")
            .code(),
        "two_sided_unknown"
    );
    assert_eq!(
        render_state(&masked(Some(true), None))
            .expect_err("unmeasured addressing is refused")
            .code(),
        "texture_addressing_unknown"
    );
    assert_eq!(
        StateError::CullFaceUnknown.to_string(),
        "nothing established whether the original drew this surface two-sided"
    );
}

/// The scene-level upload checks the material's declared coverage against the
/// image it was given: a missing texture never becomes a default material,
/// and two sources that contradict each other never produce a surface whose
/// coverage means something else.
#[test]
fn accept_f17_b_surface_upload_refuses_a_missing_or_contradicting_image() {
    let render = quad_mesh(QuadShape::full(), 0);
    let masked_item = item("fence", masked(Some(true), Some(REPEAT)), [0.0, 0.0, 0.0]);

    let no_image = upload_surface(&SceneSurface {
        item: &masked_item,
        mesh: &render,
        group: 0,
        image: None,
        unknowns: &MESH_UNKNOWNS,
    });
    let reasons = no_image.key().as_str();
    assert_eq!(reasons, "fence");
    match no_image {
        cs_app::render::capture::SceneOutcome::Refused(refusal) => assert_eq!(
            refusal.reasons(),
            [surface_codes::MISSING_IMAGE],
            "a texture-covered material with no image is not drawable"
        ),
        other => panic!("a missing image is a refusal, got {other:?}"),
    }

    let wrong_plane = upload_surface(&SceneSurface {
        item: &masked_item,
        mesh: &render,
        group: 0,
        image: Some(&decoded_image(super::fixture::ImageShape {
            format: PixelFormat::Rgb8,
            alpha_source: AlphaSource::Plane,
            alpha_test: AlphaTest::Threshold(0x10),
            color_space: ColorSpace::Srgb,
        })),
        unknowns: &MESH_UNKNOWNS,
    });
    match wrong_plane {
        cs_app::render::capture::SceneOutcome::Refused(refusal) => assert_eq!(
            refusal.reasons(),
            [surface_codes::COVERAGE_SOURCE_MISMATCH],
            "coverage declared as a channel and stored as a plane is a contradiction"
        ),
        other => panic!("a contradicting image is a refusal, got {other:?}"),
    }

    let uniform = item(
        "glass",
        material(
            MaterialClass::Blended,
            Coverage::Uniform(102),
            AlphaTest::Disabled,
            Some(true),
            Some(REPEAT),
            false,
        ),
        [0.0, 0.0, 0.0],
    );
    let conflicting = upload_surface(&SceneSurface {
        item: &uniform,
        mesh: &render,
        group: 0,
        image: Some(&decoded_image(super::fixture::ImageShape::rgba8_srgb())),
        unknowns: &MESH_UNKNOWNS,
    });
    match conflicting {
        cs_app::render::capture::SceneOutcome::Refused(refusal) => assert_eq!(
            refusal.reasons(),
            [surface_codes::UNIFORM_COVERAGE_WITH_IMAGE]
        ),
        other => panic!("a declared constant opacity with an image is a refusal, got {other:?}"),
    }

    // The consistent case uploads, and the same surface with a partial
    // attribute is refused by the geometry adapter too — the reasons of every
    // adapter are collected rather than short-circuited.
    let good = upload_surface(&SceneSurface {
        item: &masked_item,
        mesh: &render,
        group: 0,
        image: Some(&decoded_image(super::fixture::ImageShape::rgba8_srgb())),
        unknowns: &MESH_UNKNOWNS,
    });
    assert!(matches!(
        good,
        cs_app::render::capture::SceneOutcome::Uploaded(_)
    ));
    let partial = quad_mesh(
        QuadShape {
            normals: false,
            uvs: 2,
            colors: 0,
        },
        0,
    );
    let bad_geometry = upload_surface(&SceneSurface {
        item: &masked_item,
        mesh: &partial,
        group: 0,
        image: Some(&decoded_image(super::fixture::ImageShape::rgba8_srgb())),
        unknowns: &MESH_UNKNOWNS,
    });
    match bad_geometry {
        cs_app::render::capture::SceneOutcome::Refused(refusal) => {
            assert_eq!(refusal.reasons(), ["incomplete_vertex_attribute"])
        }
        other => panic!("a partially stored attribute is a refusal, got {other:?}"),
    }
}
