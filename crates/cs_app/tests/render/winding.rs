//! `accept_f10_d_2_` tests for task #396 (`F10-D.2`) and `accept_f10_f_1_`
//! tests for task #1406 (`F10-F.1`): the front-face winding and culling
//! decision measured from the original executable, and the stored polygon flag
//! that selects the two-sided case (`OWNER-STATIC-2026-10-05`; recorded in
//! `docs/findings/2026-10-14-f10-d-2-front-face-winding-and-culling.md`).
//!
//! The decision under test lives in `cs_app::render::bevy_mesh`
//! (`FRONT_FACE`, `CullingClass`) and is read by
//! `cs_app::render::bevy_state::render_state(_for_group)` — the production path
//! every drawable material's cull face comes from. These tests fail if the CCW
//! front face, the one-sided back-face cull, the two-sided exception, the
//! billboard exception, the pipeline's explicit front face, or the stored
//! `0x400` flag plumbing that reaches them is removed or changed.

use bevy::render::render_resource::{Face, FrontFace, PrimitiveState, RenderPipelineDescriptor};
use cs_app::render::bevy_mesh::{CullingClass, FRONT_FACE, WINDING_EVIDENCE, upload_group};
use cs_app::render::bevy_state::{StateError, render_state, render_state_for_mesh};
use cs_app::render::material::{
    AddressMode, Classification, ClassifiedMaterial, Coverage, DeclaredClass, MaterialClass,
    MaterialFacts, TextureAddress, classify,
};
use cs_content::mesh::RenderMesh;
use cs_formats::gamez::{FLAG_TWO_SIDED, PrimitiveKind, RawCorner, RawMesh, RawPolygon};
use cs_formats::texture::AlphaTest;
use cs_types::evidence::ClaimStatus;

const REPEAT: TextureAddress = TextureAddress {
    u: AddressMode::Repeat,
    v: AddressMode::Repeat,
};

/// A classified material of `class` with declared two-sidedness, built
/// through the production classifier.
fn authored(class: MaterialClass, two_sided: Option<bool>) -> ClassifiedMaterial {
    let facts = MaterialFacts {
        declared: Some(DeclaredClass::new(class, ClaimStatus::Designed).expect("Designed asserts")),
        coverage: Coverage::Opaque,
        alpha_test: AlphaTest::Disabled,
        two_sided,
        addressing: Some(REPEAT),
        vertex_colors: false,
        unknown_flag_bits: 0,
    };
    match classify(&facts) {
        Classification::Classified(material) => material,
        Classification::Unclassified { reasons } => panic!("authored material: {reasons:?}"),
    }
}

/// The measured decision itself: CCW front faces under the identity axis
/// map, back faces culled for one-sided surfaces, and no culling for the
/// two-sided (`0x400`) and billboard cases the original exempted.
///
/// Remove or change any of the three classes — or flip the winding — and
/// this fails: the decision is the deliverable of task #396, cited from the
/// adapter's docs to `OWNER-STATIC-2026-10-05`.
#[test]
fn accept_f10_d_2_front_face_winding_and_culling_follow_the_measured_evidence() {
    assert_eq!(
        WINDING_EVIDENCE, "OWNER-STATIC-2026-10-05",
        "the decision cites the owner evidence it was measured from"
    );
    assert_eq!(
        FRONT_FACE,
        FrontFace::Ccw,
        "the front face is the side from which stored corners 0\u{2192}1\u{2192}2 \
         run counter-clockwise (identity axis map, right-handed, +Y up)"
    );
    assert_eq!(
        CullingClass::OneSided.cull_face(),
        Some(Face::Back),
        "a one-sided surface keeps its CCW front face and culls the back one"
    );
    assert_eq!(
        CullingClass::TwoSided.cull_face(),
        None,
        "a two-sided (0x400) surface is drawn from both sides, nothing culled"
    );
    assert_eq!(
        CullingClass::Billboard.cull_face(),
        None,
        "a billboard mesh skips the test, nothing culled"
    );
}

/// The production path reads the decision: `render_state` picks the cull
/// face from the measured class, the drawable materials carry it, and the
/// additive pipeline gets the measured front face written into it
/// explicitly rather than left to wgpu's default.
///
/// A material record alone declares no two-sidedness, so `render_state` still
/// refuses an undeclared surface — the *stored* fact that says which surface is
/// two-sided is per-polygon `0x400` data, read by the content pipeline and
/// supplied to [`render_state_for_mesh`], not invented here.
#[test]
fn accept_f10_d_2_the_render_state_reads_its_culling_from_the_measured_decision() {
    let one_sided = render_state(&authored(MaterialClass::Opaque, Some(false)))
        .expect("a declared one-sided surface has a state");
    assert_eq!(
        one_sided.cull_face(),
        Some(Face::Back),
        "a one-sided surface culls its back face"
    );
    let standard = one_sided.to_drawable_material();
    let material = standard
        .standard()
        .expect("an opaque surface draws with a StandardMaterial");
    assert_eq!(
        material.cull_mode,
        Some(Face::Back),
        "the drawable material draws the state's culling decision, not a default"
    );

    let two_sided = render_state(&authored(MaterialClass::Opaque, Some(true)))
        .expect("a declared two-sided surface has a state");
    assert_eq!(
        two_sided.cull_face(),
        None,
        "a two-sided (0x400) surface culls nothing"
    );
    let standard = two_sided.to_drawable_material();
    let material = standard
        .standard()
        .expect("an opaque surface draws with a StandardMaterial");
    assert_eq!(material.cull_mode, None);

    // The additive class carries the same decisions through its own material,
    // and its specialize step writes the measured front face into the
    // pipeline explicitly.
    let additive_state = render_state(&authored(MaterialClass::Additive, Some(false)))
        .expect("a declared one-sided additive surface has a state");
    let additive = additive_state
        .to_drawable_material()
        .additive()
        .expect("the additive class has its own material")
        .clone();
    let mut descriptor = RenderPipelineDescriptor {
        // Start somewhere wrong on both counts, so the assertions prove the
        // specialization writes the decisions rather than inheriting them.
        primitive: PrimitiveState {
            front_face: FrontFace::Cw,
            cull_mode: Some(Face::Front),
            ..PrimitiveState::default()
        },
        ..RenderPipelineDescriptor::default()
    };
    additive.pipeline_key().apply(&mut descriptor);
    assert_eq!(
        descriptor.primitive.front_face, FRONT_FACE,
        "the additive pipeline draws with the measured winding"
    );
    assert_eq!(
        descriptor.primitive.cull_mode,
        Some(Face::Back),
        "the additive pipeline culls as the state recorded"
    );

    let undeclared = render_state(&authored(MaterialClass::Opaque, None));
    assert_eq!(
        undeclared.err(),
        Some(StateError::CullFaceUnknown),
        "a material record alone declares no two-sidedness, so `render_state` \
         still refuses it — the stored polygon data is the other input"
    );
}

/// One stored outline polygon over three positions, with `flags` as its stored
/// flag field.
fn stored_outline(flags: u32, material: u32) -> RawPolygon {
    RawPolygon {
        kind: PrimitiveKind::Polygon,
        raw_flags: flags,
        material,
        corners: [0u32, 1, 2]
            .into_iter()
            .map(|position| RawCorner {
                position,
                normal: None,
                uv: None,
                color: None,
            })
            .collect(),
    }
}

/// The stored mesh of one triangle, built only of authored values.
fn stored_mesh(polygons: Vec<RawPolygon>) -> RawMesh {
    RawMesh {
        positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        normals: Vec::new(),
        polygons,
    }
}

/// **A stored two-sided polygon reaches the renderer as
/// [`CullingClass::TwoSided`].** The production chain is driven end to end from
/// a stored flag: the polygon's `0x400` bit (`FLAG_TWO_SIDED`,
/// `OWNER-STATIC-2026-10-05`) builds a render mesh whose group records
/// two-sidedness, the F17-B adapter uploads that group and reports it, and the
/// state reader turns it into the measured culling class — no cull face for a
/// two-sided group, the back-face cull for a one-sided one, and a refusal for a
/// group that mixes both.
///
/// Remove the flag plumbing at any step (the render group's `two_sided`, the
/// adapter's `two_sided`, or the state reader's use of it) and the two-sided
/// assertions below fail.
#[test]
fn accept_f10_f_1_a_stored_two_sided_polygon_reaches_the_culling_class() {
    // Two-sided: the stored bit is on, so the group draws both sides.
    let two_sided = RenderMesh::build(&stored_mesh(vec![stored_outline(FLAG_TWO_SIDED, 0)]))
        .expect("the stored outline decodes");
    let upload = upload_group(&two_sided, 0, &[]).expect("group 0 uploads");
    assert_eq!(
        upload.two_sided(),
        Some(true),
        "the adapter reports the stored group's two-sidedness"
    );
    let material = authored(MaterialClass::Opaque, None);
    assert_eq!(
        render_state(&material).err(),
        Some(StateError::CullFaceUnknown),
        "the material record alone still declares nothing"
    );
    let state = render_state_for_mesh(&material, false, upload.two_sided())
        .expect("the stored group's two-sidedness completes the state");
    assert_eq!(
        state.cull_face(),
        CullingClass::TwoSided.cull_face(),
        "a stored two-sided polygon draws with nothing culled"
    );
    assert_eq!(state.cull_face(), None);

    // One-sided: no stored bit, so the group culls its back face.
    let one_sided =
        RenderMesh::build(&stored_mesh(vec![stored_outline(0, 0)])).expect("the outline decodes");
    let upload = upload_group(&one_sided, 0, &[]).expect("group 0 uploads");
    assert_eq!(upload.two_sided(), Some(false));
    let state =
        render_state_for_mesh(&material, false, upload.two_sided()).expect("one-sided state");
    assert_eq!(
        state.cull_face(),
        CullingClass::OneSided.cull_face(),
        "a one-sided stored polygon culls its back face"
    );
    assert_eq!(state.cull_face(), Some(Face::Back));
}

/// **A group that mixes one- and two-sided polygons settles neither.** Two
/// polygons of one material, one carrying `0x400` and one not, are one upload
/// with one cull face that cannot express both, so the group reports `None` and
/// the state is refused rather than rounded to a side.
#[test]
fn accept_f10_f_1_a_mixed_group_is_refused_not_rounded() {
    let mixed = RenderMesh::build(&stored_mesh(vec![
        stored_outline(FLAG_TWO_SIDED, 0),
        stored_outline(0, 0),
    ]))
    .expect("both outlines decode");
    let upload = upload_group(&mixed, 0, &[]).expect("group 0 uploads");
    assert_eq!(
        upload.two_sided(),
        None,
        "one flagged and one unflagged polygon with the same material is mixed"
    );
    let material = authored(MaterialClass::Opaque, None);
    assert_eq!(
        render_state_for_mesh(&material, false, upload.two_sided()).err(),
        Some(StateError::CullFaceUnknown),
        "a mixed group leaves two-sidedness unknown, never defaulted"
    );
}

/// Task #1407 (`F10-F.2`): a surface of a mesh flagged as an unculled
/// billboard draws with no cull face even when its material is one-sided;
/// without the flag it keeps the back-face cull. Remove the wiring in
/// `render_state_for_mesh` and the first assertion fails.
#[test]
fn accept_f10_f_2_billboard_mesh_surfaces_are_not_culled() {
    let material = authored(MaterialClass::Opaque, Some(false));
    assert_eq!(
        render_state_for_mesh(&material, true, None)
            .unwrap()
            .cull_face(),
        None
    );
    assert_eq!(
        render_state_for_mesh(&material, false, None)
            .unwrap()
            .cull_face(),
        Some(Face::Back)
    );
    assert_eq!(
        render_state(&material).unwrap().cull_face(),
        Some(Face::Back)
    );
}

/// The type words reach the render mesh: only word0 == 1 with word1 in {0, 1}
/// flags it, and a mesh never given type words is not a billboard.
#[test]
fn accept_f10_f_2_render_mesh_carries_the_type_words() {
    use cs_content::mesh::RenderMesh;
    use cs_formats::gamez::reader::RawMeshInfo;

    let mut info: RawMeshInfo = zeroed_info();
    assert!(!RenderMesh::default().is_unculled_billboard());
    for (word0, word1, flagged) in [(0, 0, false), (1, 0, true), (1, 1, true), (1, 2, false)] {
        info.file_ptr = word0;
        info.unk04 = word1;
        assert_eq!(
            RenderMesh::default()
                .with_type_words(&info)
                .is_unculled_billboard(),
            flagged,
            "({word0}, {word1})"
        );
    }
}

fn zeroed_info() -> cs_formats::gamez::reader::RawMeshInfo {
    use cs_formats::gamez::reader::RawMeshInfo;
    RawMeshInfo {
        file_ptr: 0,
        unk04: 0,
        unk08: 0,
        parent_count: 1,
        polygon_count: 0,
        vertex_count: 0,
        normal_count: 0,
        morph_count: 0,
        light_count: 0,
        unk36: 0,
        unk40: 0.0,
        unk44: 0.0,
        unk48: 0,
        polygons_ptr: 0,
        vertices_ptr: 0,
        normals_ptr: 0,
        lights_ptr: 0,
        morphs_ptr: 0,
        unk72: 0.0,
        unk76: 0.0,
        unk80: 0.0,
        unk84: 0.0,
        unk88: 0,
        material_count: 0,
        materials_ptr: 0,
    }
}
