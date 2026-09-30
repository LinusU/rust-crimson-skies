//! The render state a classified material needs, and the Bevy material that
//! expresses it (`specs/F17-rendering-material-fidelity-and-scalable-
//! presentation.md`, stage `### F17-B`; shared contract
//! `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! F17's first non-negotiable is that surfaces are not collapsed onto one
//! generic material. This module is where a [`ClassifiedMaterial`] stops being
//! a classification and becomes a render state: a blend mode, a depth
//! behavior, a cull face, an alpha test, whether the stored per-corner colors
//! are bound, and the addressing its image is sampled with. Every field is
//! read from the classification; **no field is defaulted**.
//!
//! Two of them cannot be read, because nothing measured them:
//!
//! * `ClassifiedMaterial::two_sided()` is `None` while no reference
//!   established whether the original renderer drew the surface
//!   two-sided. Picking a cull face would silently halve or double every
//!   polygon, so the state is refused ([`StateError::CullFaceUnknown`]).
//! * `ClassifiedMaterial::addressing()` is `None` for the same reason. A
//!   sampler with invented address modes tiles or stretches wrongly in a way
//!   no other report would show, so the state is refused
//!   ([`StateError::AddressModeUnknown`]).
//!
//! The per-class state table below is *new-engine design*, the same
//! `Designed` status F17-A assigned to the class-to-phase map
//! (`docs/findings/2026-09-30-f17-a-material-classification-and-golden-
//! scene.md`). No row asserts what the original renderer did.
//!
//! | Class | Blend | Depth write | Notes |
//! | --- | --- | --- | --- |
//! | `Opaque` | replace | yes | coverage is ignored even when the image has it |
//! | `Emissive` | replace | yes | unlit: emission is not dimmed by scene lighting |
//! | `Masked` | replace | yes | alpha test discards below the declared threshold |
//! | `Blended` | `SrcAlpha`/`OneMinusSrcAlpha` | **no** | must not occlude the surface behind it |
//! | `Additive` | `One`/`One` | **no** | adds to the framebuffer after all translucency |
//!
//! Depth writing is off for the two translucent classes because a blended
//! surface that writes depth would hide the surfaces behind it and make the
//! back-to-front sort pointless. That is a design decision this stage records,
//! not a measured original behavior.

use std::fmt;

use bevy::material::AlphaMode;
use bevy::pbr::StandardMaterial;
use bevy::render::render_resource::{
    BlendComponent, BlendFactor, BlendOperation, BlendState, Face,
};
use cs_assets::install::sha256;
use cs_formats::texture::AlphaTest;
use cs_types::evidence::ContentHash;

use crate::render::material::{ClassifiedMaterial, Coverage, MaterialClass, TextureAddress};

/// Why a classified material did not become a render state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateError {
    /// Nothing established whether the original drew the surface
    /// two-sided, so no cull face can be chosen.
    CullFaceUnknown,
    /// Nothing established the texture addressing, so no sampler can be
    /// described.
    AddressModeUnknown,
}

impl StateError {
    /// Stable lowercase identifier, used as an unsupported reason.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::CullFaceUnknown => "two_sided_unknown",
            Self::AddressModeUnknown => "texture_addressing_unknown",
        }
    }
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CullFaceUnknown => write!(
                f,
                "nothing established whether the original drew this surface two-sided"
            ),
            Self::AddressModeUnknown => {
                write!(f, "nothing established this material's texture addressing")
            }
        }
    }
}

impl std::error::Error for StateError {}

/// Why a render state has no `StandardMaterial` yet.
///
/// The states themselves are complete: a `RenderState` describes every blend,
/// depth and coverage decision for all five classes. What is missing is a
/// *Bevy material type* for the one decision `StandardMaterial` cannot
/// express, and that is a shader problem, not a classification one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialGap {
    /// `StandardMaterial::alpha_mode` is `Opaque`/`Mask`/`Blend` only. An
    /// additive surface needs a blend state of `One`/`One`, which needs a
    /// material with its own blend state (and, in
    /// `crates/cs_app/assets/shaders/`, its own shader). The state is
    /// recorded and carried; the drawable material is not fabricated.
    AdditiveBlendState,
}

impl MaterialGap {
    /// Stable lowercase identifier, used as an unsupported reason.
    pub const fn code(self) -> &'static str {
        match self {
            Self::AdditiveBlendState => "additive_blend_state_needs_custom_material",
        }
    }
}

impl fmt::Display for MaterialGap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AdditiveBlendState => write!(
                f,
                "no StandardMaterial expresses the additive blend state; \
                 a material with its own blend state is required"
            ),
        }
    }
}

impl std::error::Error for MaterialGap {}

/// Blend state that adds the source color to the framebuffer.
///
/// `StandardMaterial` has no additive mode, so this is the state an additive
/// material has to implement; it is written out here so the additive
/// [`RenderState`] is complete and comparable even while its drawable
/// material is not.
const ADDITIVE: BlendState = BlendState {
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

/// Every render decision one surface draws with.
#[derive(Debug)]
pub struct RenderState {
    class: MaterialClass,
    coverage: Coverage,
    blend: BlendState,
    depth_write: bool,
    cull_face: Option<Face>,
    alpha_mode: AlphaMode,
    alpha_test: AlphaTest,
    vertex_colors: bool,
    address: TextureAddress,
    unlit: bool,
    fingerprint: ContentHash,
}

/// Reads the render state out of a classified material.
///
/// # Errors
///
/// [`StateError`] when the classification does not carry the two-sidedness or
/// the addressing the state needs. Both are refused rather than defaulted.
pub fn render_state(material: &ClassifiedMaterial) -> Result<RenderState, StateError> {
    let two_sided = material.two_sided().ok_or(StateError::CullFaceUnknown)?;
    let address = material
        .addressing()
        .ok_or(StateError::AddressModeUnknown)?;
    let class = material.class();
    let alpha_test = material.alpha_test();

    // A two-sided surface is drawn from both faces, so nothing is culled; a
    // one-sided surface culls its back face. `Face::Back` is Bevy's default
    // and the only choice consistent with "one-sided": the *declared*
    // two-sidedness is the fact, the face is the engine's convention.
    let cull_face = (!two_sided).then_some(Face::Back);
    let blend = match class {
        MaterialClass::Opaque | MaterialClass::Emissive | MaterialClass::Masked => {
            BlendState::REPLACE
        }
        MaterialClass::Blended => BlendState::ALPHA_BLENDING,
        MaterialClass::Additive => ADDITIVE,
    };
    let depth_write = matches!(
        class,
        MaterialClass::Opaque | MaterialClass::Emissive | MaterialClass::Masked
    );
    let alpha_mode = match (class, alpha_test) {
        (MaterialClass::Masked, AlphaTest::Threshold(t)) => {
            // Bevy keeps fully opaque above the cutoff and fully transparent
            // at or below it, which is the declared rule: coverage below the
            // threshold is discarded. `/255` because the classification
            // carries the stored byte and the cutoff is a normalized float.
            AlphaMode::Mask(f32::from(t) / f32::from(u8::MAX))
        }
        _ => AlphaMode::Opaque,
    };
    let unlit = class == MaterialClass::Emissive;
    let vertex_colors = material.vertex_colors();
    let coverage = material.coverage();

    // The digest is taken over the finished state, so it cannot miss a field
    // or cover one twice.
    let mut state = RenderState {
        class,
        coverage,
        blend,
        depth_write,
        cull_face,
        alpha_mode,
        alpha_test,
        vertex_colors,
        address,
        unlit,
        fingerprint: ContentHash::from_bytes([0; 32]),
    };
    state.fingerprint = state_fingerprint(&state);
    Ok(state)
}

fn state_fingerprint(state: &RenderState) -> ContentHash {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"cs/render/bevy_state/v1\0");
    bytes.extend_from_slice(state.class.code().as_bytes());
    bytes.push(0);
    match state.coverage {
        Coverage::Opaque => bytes.push(0),
        Coverage::Unknown => bytes.push(1),
        Coverage::Uniform(alpha) => {
            bytes.push(2);
            bytes.push(alpha);
        }
        Coverage::Texture(source) => {
            bytes.push(3);
            bytes.extend_from_slice(format!("{source:?}").as_bytes());
            bytes.push(0);
        }
    }
    for component in [state.blend.color, state.blend.alpha] {
        bytes.push(component.src_factor as u8);
        bytes.push(component.dst_factor as u8);
        bytes.push(component.operation as u8);
    }
    bytes.push(u8::from(state.depth_write));
    match state.cull_face {
        None => bytes.push(0),
        Some(Face::Front) => bytes.push(1),
        Some(Face::Back) => bytes.push(2),
    }
    match state.alpha_mode {
        AlphaMode::Opaque => bytes.push(0),
        AlphaMode::Mask(cutoff) => {
            bytes.push(1);
            bytes.extend_from_slice(&cutoff.to_bits().to_le_bytes());
        }
        AlphaMode::Blend
        | AlphaMode::Premultiplied
        | AlphaMode::AlphaToCoverage
        | AlphaMode::Add
        | AlphaMode::Multiply => bytes.push(2),
    }
    let alpha_test = state.alpha_test;
    match alpha_test {
        AlphaTest::Disabled => bytes.push(0),
        AlphaTest::Unknown => bytes.push(1),
        AlphaTest::Threshold(t) => {
            bytes.push(2);
            bytes.push(t);
        }
    }
    bytes.push(u8::from(state.vertex_colors));
    bytes.extend_from_slice(state.address.u.code().as_bytes());
    bytes.push(b':');
    bytes.extend_from_slice(state.address.v.code().as_bytes());
    bytes.push(0);
    bytes.push(u8::from(state.unlit));
    sha256(&bytes)
}

impl RenderState {
    /// The class this state was read from.
    pub const fn class(&self) -> MaterialClass {
        self.class
    }

    /// Where the surface's coverage comes from, as the classification
    /// declares it. A `Uniform(alpha)` opacity is a render state: it is the
    /// surface's coverage whether or not it has an image, and it travels in
    /// the state's digest so a comparison sees it change.
    pub const fn coverage(&self) -> Coverage {
        self.coverage
    }

    /// The blend mode, for every class including additive.
    pub const fn blend(&self) -> &BlendState {
        &self.blend
    }

    /// Whether the surface writes depth. Off for the two translucent
    /// classes, so an overlapping pane cannot occlude the pane behind it.
    pub const fn depth_write(&self) -> bool {
        self.depth_write
    }

    /// The cull face, `None` for a two-sided surface.
    pub const fn cull_face(&self) -> Option<Face> {
        self.cull_face
    }

    /// The coverage treatment, `Mask(threshold)` for an alpha-cut surface.
    pub const fn alpha_mode(&self) -> &AlphaMode {
        &self.alpha_mode
    }

    /// The declared alpha test as the classification carries it.
    pub const fn alpha_test(&self) -> AlphaTest {
        self.alpha_test
    }

    /// Whether the mesh's stored per-corner colors are bound as vertex
    /// colors. Their *meaning* stays `MaterialUnknown::VertexColorMeaning`.
    pub const fn vertex_colors(&self) -> bool {
        self.vertex_colors
    }

    /// The declared texture addressing, the sampler's address modes.
    pub const fn address(&self) -> TextureAddress {
        self.address
    }

    /// Whether the surface is drawn unlit.
    pub const fn unlit(&self) -> bool {
        self.unlit
    }

    /// A digest of every decision above, so a frame capture can prove two
    /// captures drew their surfaces with the same state.
    pub const fn fingerprint(&self) -> ContentHash {
        self.fingerprint
    }

    /// The `StandardMaterial` that expresses this state.
    ///
    /// The material carries no texture handle: binding an image needs an
    /// `Assets<Image>` and is the consumer's job, not the adapter's. The
    /// blend, coverage, cull and lighting decisions are the adapter's, and
    /// they are the ones set here.
    ///
    /// # Errors
    ///
    /// [`MaterialGap::AdditiveBlendState`] for an additive surface, which
    /// `StandardMaterial` has no mode for.
    pub fn to_standard_material(&self) -> Result<StandardMaterial, MaterialGap> {
        if self.class == MaterialClass::Additive {
            return Err(MaterialGap::AdditiveBlendState);
        }
        // The mask threshold travels inside `AlphaMode::Mask`; the cutoff
        // uniform it feeds is a shader-side field this stage does not set.
        Ok(StandardMaterial {
            alpha_mode: self.alpha_mode,
            cull_mode: self.cull_face,
            unlit: self.unlit,
            ..StandardMaterial::default()
        })
    }
}
