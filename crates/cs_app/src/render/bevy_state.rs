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
//!
//! # How the state reaches a drawable material
//!
//! [`RenderState::to_drawable_material`] may only produce a material that draws
//! the way the state says, and in Bevy 0.19 that constrains the fields each
//! material type can carry. A `StandardMaterial` has no blend-state field: its
//! `alpha_mode` *is* the blend input, the cull mode is the cull face, and
//! `unlit` is the lighting decision. Four of the five classes are expressed
//! that way; a second can only be expressed *through* a mode — a blended
//! surface is `AlphaMode::Blend`, which is what asks Bevy for
//! `BlendState::ALPHA_BLENDING` and a pipeline that does not write depth,
//! exactly the two decisions the state records. A declared constant opacity is
//! applied to the base color's alpha, which is where a PBR material keeps
//! coverage.
//!
//! The fifth class cannot be expressed by a `StandardMaterial` at all: additive
//! `One`/`One` has no mode, because Bevy maps `AlphaMode::Add` onto the
//! premultiplied-alpha pipeline. It gets a material of its own —
//! [`AdditiveMaterial`](crate::render::additive::AdditiveMaterial) with
//! [`ADDITIVE_FRAGMENT_SHADER`](crate::render::additive::ADDITIVE_FRAGMENT_SHADER),
//! whose blend state is the one `ADDITIVE` above already records. There is
//! still only *one* table of what an additive surface is: this module's. The
//! material copies the recorded decisions and specializes its pipeline with
//! them; it does not decide any of them.

use std::fmt;

use bevy::color::{Color, LinearRgba};
use bevy::material::AlphaMode;
use bevy::pbr::StandardMaterial;
use bevy::render::render_resource::{
    BlendComponent, BlendFactor, BlendOperation, BlendState, Face,
};
use cs_assets::install::sha256;
use cs_formats::texture::AlphaTest;
use cs_types::evidence::ContentHash;

use crate::render::additive::AdditiveMaterial;
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

/// Which drawable material a render state became.
///
/// The class decides this, and every class has one: F17-B recorded that the
/// additive class had *no* drawable material because a `StandardMaterial` cannot
/// reach a blend state of `One`/`One`, and that gap is closed — the additive
/// class draws with [`AdditiveMaterial`], which owns its own blend state and
/// its own shader. The variant exists so a frame can report *which* material a
/// surface was given, which is a render decision a comparison can see: two
/// captures that differ only in the additive class's material are different
/// frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MaterialKind {
    /// A `StandardMaterial` with this class's own alpha mode, cull face and
    /// lighting decision.
    Standard,
    /// The additive class's own material, because `StandardMaterial` cannot
    /// reach its blend: its `alpha_mode` is the only blend input, and Bevy maps
    /// `AlphaMode::Add` onto the *premultiplied* alpha pipeline, which
    /// multiplies the source by its own alpha rather than adding it. The blend
    /// is the [`ADDITIVE`] state below, not a mode.
    Additive,
}

impl MaterialKind {
    /// Every kind, in class order. The additive class is the last one.
    pub const ALL: [Self; 2] = [Self::Standard, Self::Additive];

    /// Stable lowercase identifier, used in a frame's identity.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Additive => "additive",
        }
    }
}

impl fmt::Display for MaterialKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// The drawable material a render state became.
///
/// A value, not a handle: binding an image needs an `Assets<Image>` and is the
/// consumer's job, exactly as it is for a `StandardMaterial`. The blend,
/// coverage, cull and lighting decisions are the adapter's, and they are the
/// ones each variant carries.
///
/// The `StandardMaterial` is boxed because the two variants are very different
/// sizes (`StandardMaterial` is a full PBR description with a dozen optional
/// textures, `AdditiveMaterial` is one uniform and one optional image) and this
/// enum sits in every `SurfaceUpload`, i.e. once per submitted surface. One
/// allocation per upload is the smaller cost next to carrying 350 unused bytes
/// per surface in a frame.
///
/// It is compared by *kind*, not by value: neither `StandardMaterial` nor
/// [`AdditiveMaterial`] implements `PartialEq` (a `Handle<Image>` is a
/// generation-stamped identity, not a value), and what a consumer needs to know
/// about two surfaces is which material type each was given, not whether two
/// independently built materials happen to be equal. [`DrawableMaterial::eq_kind`]
/// is that comparison.
#[derive(Clone, Debug)]
pub enum DrawableMaterial {
    /// A `StandardMaterial` with the class's own alpha mode, cull face and
    /// unlit flag.
    Standard(Box<StandardMaterial>),
    /// The additive class's own material, carrying the recorded `One`/`One`
    /// blend and "no depth write".
    Additive(AdditiveMaterial),
}

impl DrawableMaterial {
    /// Which material this is.
    pub const fn kind(&self) -> MaterialKind {
        match self {
            Self::Standard(_) => MaterialKind::Standard,
            Self::Additive(_) => MaterialKind::Additive,
        }
    }

    /// The `StandardMaterial`, when this class has one.
    pub const fn standard(&self) -> Option<&StandardMaterial> {
        match self {
            Self::Standard(material) => Some(material),
            Self::Additive(_) => None,
        }
    }

    /// The additive material, when this class has one.
    pub const fn additive(&self) -> Option<&AdditiveMaterial> {
        match self {
            Self::Standard(_) => None,
            Self::Additive(material) => Some(material),
        }
    }

    /// Whether `other` is drawn with the same kind of material.
    ///
    /// The comparison a consumer can actually make: two surfaces are bound the
    /// same way when their classes are, and a value comparison of the materials
    /// themselves is not available (see the type's docs).
    pub const fn eq_kind(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::Standard(_), Self::Standard(_)) | (Self::Additive(_), Self::Additive(_))
        )
    }
}

/// Blend state that adds the source color to the framebuffer.
///
/// The additive class's state, and the *only* place it is written down: the
/// material that draws with it copies this one, so a state and a material can
/// never disagree about what "additive" means.
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
            // Bevy keeps a texel whose coverage is *at or above* the cutoff
            // (`color.a >= material.alpha_cutoff` in the forward PBR shader,
            // drawn fully opaque) and discards one below it, which is the
            // declared rule: coverage below the threshold is discarded.
            // `/255` because the classification carries the stored byte and
            // the cutoff is a normalized float.
            AlphaMode::Mask(f32::from(t) / f32::from(u8::MAX))
        }
        (MaterialClass::Blended, _) => {
            // A `StandardMaterial` has no blend-state field: Bevy picks the
            // pipeline from `alpha_mode`, and `Blend` is what requests
            // `BlendState::ALPHA_BLENDING` and a pipeline that does not write
            // depth — the two decisions this state records for a blended
            // surface. Anything else here would hand the consumer a material
            // that contradicts the state beside it.
            AlphaMode::Blend
        }
        (MaterialClass::Additive, _) => {
            // The additive class draws with its own material, so this field is
            // not its blend — [`ADDITIVE`] above is, and the material
            // specializes its pipeline with that. What the mode still decides
            // is the *pass*: an additive surface is translucent, so it is
            // queued in the sorted transparent phase and the depth write is
            // already off, which is exactly the two decisions this class
            // records. Reporting `Opaque` here would put the surface in the
            // binned opaque phase, where it is neither depth-sorted against the
            // translucency in front of it nor mixed in the engine's order.
            AlphaMode::Blend
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

    /// The coverage treatment, `Mask(threshold)` for an alpha-cut surface,
    /// `Blend` for a blended or additive one. In a `StandardMaterial` this
    /// field *is* the blend input, so the state and the material it produces
    /// agree on the blend and on the depth write; on the additive material it
    /// is the pass, and the blend travels in
    /// [`RenderState::blend`](RenderState::blend) instead.
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

    /// The drawable material that expresses this state.
    ///
    /// The material carries no image handle: binding an image needs an
    /// `Assets<Image>` and is the consumer's job, not the adapter's. The
    /// blend, coverage, cull and lighting decisions are the adapter's, and
    /// they are the ones set here.
    ///
    /// Every class has one. Four become a `StandardMaterial`, which takes the
    /// blend from [`RenderState::alpha_mode`]; the additive class becomes an
    /// [`AdditiveMaterial`], which takes it from
    /// [`RenderState::blend`](RenderState::blend) because no `alpha_mode`
    /// reaches `One`/`One`. Either way the decisions are this state's, copied
    /// rather than re-decided.
    pub fn to_drawable_material(&self) -> DrawableMaterial {
        match self.class {
            MaterialClass::Additive => DrawableMaterial::Additive(AdditiveMaterial {
                color: self.additive_color(),
                base_color_texture: None,
                blend: self.blend,
                depth_write: self.depth_write,
                cull_face: self.cull_face,
            }),
            _ => DrawableMaterial::Standard(Box::new(StandardMaterial {
                alpha_mode: self.alpha_mode,
                cull_mode: self.cull_face,
                unlit: self.unlit,
                // The mask threshold travels inside `AlphaMode::Mask`; the
                // cutoff uniform it feeds is a shader-side field this stage
                // does not set.
                base_color: self.standard_base_color(),
                ..StandardMaterial::default()
            })),
        }
    }

    /// The base color a `StandardMaterial` draws with.
    ///
    /// A declared constant opacity is the whole coverage of a surface that
    /// carries no image, and in a PBR material coverage *is* the base color's
    /// alpha: the blend factor and the mask both read the output alpha. It is
    /// applied here because the classification declares it and nothing
    /// downstream would otherwise. The color channels stay the identity
    /// multiplier `1.0` — no tint is declared anywhere, so none is invented —
    /// and Bevy passes an `Srgba` value's alpha through unchanged, so the
    /// stored byte reaches the shader as `alpha / 255` with no encoding change
    /// on the way.
    fn standard_base_color(&self) -> Color {
        match (self.class, self.coverage) {
            (MaterialClass::Masked | MaterialClass::Blended, Coverage::Uniform(alpha)) => {
                Color::srgba(1.0, 1.0, 1.0, f32::from(alpha) / f32::from(u8::MAX))
            }
            _ => Color::WHITE,
        }
    }

    /// The color the additive material draws with.
    ///
    /// Same reasoning as [`RenderState::standard_base_color`] for the alpha —
    /// a declared constant opacity is coverage, and `classify` accepts it for
    /// this class — with one difference forced by the blend: an `One`/`One`
    /// blend adds the source as it is, so no blend factor downstream can read
    /// that alpha. The shader applies it to the contribution instead, which is
    /// a `Designed` decision recorded in
    /// `docs/findings/2026-09-30-f17-c-followup-additive-material.md`.
    fn additive_color(&self) -> LinearRgba {
        match self.coverage {
            Coverage::Uniform(alpha) => {
                LinearRgba::new(1.0, 1.0, 1.0, f32::from(alpha) / f32::from(u8::MAX))
            }
            _ => LinearRgba::WHITE,
        }
    }
}
