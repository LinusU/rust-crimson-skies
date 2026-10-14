//! `accept_f10_d_2_` tests for task #396 (`F10-D.2`): the front-face winding
//! and culling decision measured from the original executable
//! (`OWNER-STATIC-2026-10-05`; recorded in
//! `docs/findings/2026-10-14-f10-d-2-front-face-winding-and-culling.md`).
//!
//! The decision under test lives in `cs_app::render::bevy_mesh`
//! (`FRONT_FACE`, `CullingClass`) and is read by
//! `cs_app::render::bevy_state::render_state` — the production path every
//! drawable material's cull face comes from. These tests fail if the CCW
//! front face, the one-sided back-face cull, the two-sided exception, the
//! billboard exception or the pipeline's explicit front face is removed or
//! changed.

use bevy::render::render_resource::{Face, FrontFace, PrimitiveState, RenderPipelineDescriptor};
use cs_app::render::bevy_mesh::{CullingClass, FRONT_FACE, WINDING_EVIDENCE};
use cs_app::render::bevy_state::{StateError, render_state};
use cs_app::render::material::{
    AddressMode, Classification, ClassifiedMaterial, Coverage, DeclaredClass, MaterialClass,
    MaterialFacts, TextureAddress, classify,
};
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
/// The undeclared case stays refused — the *rule* is measured, but which
/// stored surface is two-sided is per-polygon `0x400` data no reader has
/// plumbed yet, and an invented class would silently halve or double a
/// surface.
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
        "which stored surface carries 0x400 is still unread data: an \
         undeclared surface is refused, never defaulted to a class"
    );
}
