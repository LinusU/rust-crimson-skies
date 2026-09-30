//! The additive class's drawable material: the `One`/`One` blend state a
//! [`bevy::pbr::StandardMaterial`] cannot express, and the WGSL shader that
//! draws with it.
//!
//! F17-B recorded the gap this module closes
//! (`docs/findings/2026-09-30-f17-b-canonical-mesh-and-image-to-bevy.md`): a
//! `StandardMaterial` has no field for a blend state, its `alpha_mode` is the
//! only blend input, and Bevy 0.19 maps `AlphaMode::Add` onto the
//! *premultiplied-alpha* pipeline (`alpha_mode_pipeline_key` in
//! `bevy_pbr::material`), which multiplies the source by its own alpha rather
//! than adding it. The render state for the class was already complete —
//! `One`/`One`, no depth write, the declared cull face — and it was carried
//! unrenderable, so `sync_frame` counted the additive pass in
//! `FrameSync::unmaterialed` and spawned nothing for it.
//!
//! # What this module adds, and what it deliberately does not
//!
//! * [`AdditiveMaterial`] is a [`bevy::pbr::Material`], so the class has a
//!   drawable material: an entity can carry `MeshMaterial3d<AdditiveMaterial>`
//!   and the engine builds a pipeline for it.
//! * Its blend, depth-write, cull and pass decisions are **not re-derived
//!   here**. They are copied from the
//!   [`RenderState`](crate::render::bevy_state::RenderState) that already
//!   records them, carried in the material value, and written into the pipeline
//!   by [`AdditiveMaterialKey::apply`], which is what [`Material::specialize`]
//!   calls. The additive class's own state table stays in `bevy_state`, so there
//!   is exactly one place that says what an additive surface is — including
//!   which pass it is queued in.
//! * The fragment shader is a real file in this crate's asset root,
//!   [`ADDITIVE_FRAGMENT_SHADER`], loaded by the engine's asset server
//!   (`ShaderRef::Path`). It is not a WGSL string in Rust, and it is not a file
//!   no material points at: [`Material::fragment_shader`] returns exactly that
//!   path, and the acceptance tests check the file exists at that path and
//!   binds the layout the derive generates.
//!
//! # `alpha_mode` on this material is the pass, not the blend
//!
//! [`Material::alpha_mode`] returns the material's own `alpha_mode` field, which
//! is the mode the surface's render state recorded (`AlphaMode::Blend` for this
//! class), for one reason: on a material that owns its blend state, the alpha
//! mode's only remaining meaning is which render phase the surface is queued in,
//! and an additive surface is translucent — it must be sorted back-to-front and
//! must not occlude what is behind it. `AlphaMode::Blend` is also what the base
//! mesh pipeline would read as `BlendState::ALPHA_BLENDING`;
//! [`AdditiveMaterialKey::apply`] then *replaces* that with the recorded
//! `One`/`One`, so the mode is never the blend. The engine reads both facts
//! (bevy_pbr 0.19.1: `alpha_mode()` picks `RenderPhaseType::Transparent`, and
//! `specialize` runs after the base pipeline's blend was written), and the
//! acceptance test asserts the specialized descriptor carries `One`/`One` and no
//! depth write.
//!
//! One consequence is recorded rather than fixed here: the engine's transparent
//! phase is *one sorted phase*, so an additive surface is sorted against the
//! other translucency by its mesh centre rather than drawn in a pass of its own.
//! The plan's own `RenderPhase::Additive` ordering is F17-A's decision and is
//! unaffected; whether the original drew additive surfaces after all
//! translucency is unmeasured. See
//! `docs/findings/2026-09-30-f17-c-followup-additive-material.md`.
//!
//! # Designed, not measured
//!
//! The material's shader is *unlit*, takes no part in the depth prepass and
//! casts no shadow. Each of those is a new-engine decision, not a measured
//! original behaviour, and
//! `docs/findings/2026-09-30-f17-c-followup-additive-material.md` records all
//! of them together with what is still `Designed`. Nothing here claims what the
//! 2000 renderer did with an additive surface.

use bevy::asset::{Asset, AssetPath, Handle};
use bevy::color::LinearRgba;
use bevy::image::Image;
use bevy::material::AlphaMode;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, BlendState, Face, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

/// The additive class's fragment shader, as a path inside the asset root.
///
/// The path is relative to the engine's asset folder, which for this crate is
/// `crates/cs_app/assets/`; the file is loaded through the asset server by
/// [`Material::fragment_shader`], so it is the same load any other `.wgsl` in
/// the project takes.
pub const ADDITIVE_FRAGMENT_SHADER: &str = "shaders/crimson_additive.wgsl";

/// The pipeline decisions [`AdditiveMaterial`] carries from its render state.
///
/// `Material::specialize` is an associated function: it never sees the material
/// *value*, only the specialization key, so the per-material decisions have to
/// travel in the key. `#[bind_group_data]` makes this the key's type, which
/// also means the decisions are part of the pipeline cache key: two additive
/// materials that differ in blend, depth write or cull face are two pipelines
/// rather than one pipeline drawn with the wrong state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AdditiveMaterialKey {
    blend: BlendState,
    depth_write: bool,
    cull_face: Option<Face>,
}

impl From<&AdditiveMaterial> for AdditiveMaterialKey {
    fn from(material: &AdditiveMaterial) -> Self {
        Self {
            blend: material.blend,
            depth_write: material.depth_write,
            cull_face: material.cull_face,
        }
    }
}

impl AdditiveMaterialKey {
    /// The blend state the surface draws with, as its render state recorded it.
    pub const fn blend(&self) -> &BlendState {
        &self.blend
    }

    /// Whether the surface writes depth, as its render state recorded it.
    pub const fn depth_write(&self) -> bool {
        self.depth_write
    }

    /// The cull face the surface draws with, `None` for a two-sided surface.
    pub const fn cull_face(&self) -> Option<Face> {
        self.cull_face
    }

    /// Writes these decisions into a specialized pipeline descriptor.
    ///
    /// This is the whole of what the material adds to the pipeline, and it is
    /// the one place it is done: [`Material::specialize`] does nothing else. The
    /// three writes are the three decisions a `StandardMaterial` cannot carry
    /// for this class — the blend, the depth write and the cull face — so a
    /// material that skipped any of them would draw with a decision its render
    /// state does not record.
    ///
    /// Two of the three have a fixed place to go. `cull_mode` is a
    /// `PrimitiveState` field. The blend is **not** one: in wgpu 29 a blend
    /// state belongs to each `ColorTargetState` of the fragment stage, and it
    /// is set on every target, because a target the blend did not reach would
    /// still be written with `REPLACE`. That is where the engine's own base
    /// mesh pipeline puts it for `AlphaMode::Blend` (bevy_pbr 0.19.1,
    /// `MeshPipeline::specialize`), so this replaces exactly the field that
    /// path would have written `ALPHA_BLENDING` into.
    ///
    /// `depth_stencil` is not always present: a pipeline with no depth
    /// attachment has nothing to configure, and a material that invented one
    /// would add a depth format the target does not have. A mesh pipeline
    /// always has it.
    pub fn apply(&self, descriptor: &mut RenderPipelineDescriptor) {
        descriptor.primitive.cull_mode = self.cull_face;
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(self.blend);
            }
        }
        if let Some(depth_stencil) = descriptor.depth_stencil.as_mut() {
            depth_stencil.depth_write_enabled = Some(self.depth_write);
        }
    }
}

/// The drawable material for the additive class.
///
/// It is *one* material type for the class rather than one per state, so every
/// field below is copied from the surface's
/// [`RenderState`](crate::render::bevy_state::RenderState) by
/// [`RenderState::to_drawable_material`](crate::render::bevy_state::RenderState::to_drawable_material)
/// and none of them is defaulted here.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
#[bind_group_data(AdditiveMaterialKey)]
pub struct AdditiveMaterial {
    /// The surface's own color: no tint is declared anywhere in the content
    /// pipeline, so the channels stay the identity multiplier, and the alpha is
    /// the *declared constant coverage* when the classification carries one.
    ///
    /// The `One`/`One` blend adds the source as it is, so nothing downstream
    /// can consume this alpha as a blend factor; the shader applies it to the
    /// contribution instead. That is a `Designed` decision — see the module
    /// docs.
    #[uniform(0)]
    pub color: LinearRgba,
    /// The image every instance in the batch samples, when the batch has one.
    ///
    /// Optional because an additive surface may declare no image. An absent
    /// image binds the engine's 1x1 opaque white fallback, so the shader can
    /// sample unconditionally; nothing is invented.
    #[texture(1)]
    #[sampler(2)]
    pub base_color_texture: Option<Handle<Image>>,
    /// The mode the surface's render state recorded: the *pass* it is queued
    /// in, copied rather than re-decided (see [`Material::alpha_mode`]).
    pub alpha_mode: AlphaMode,
    /// The blend state the surface's render state recorded, `One`/`One` for
    /// this class.
    pub blend: BlendState,
    /// Whether the surface writes depth, as its render state recorded it: off,
    /// so an additive surface never hides what is behind it.
    pub depth_write: bool,
    /// The cull face, as the render state recorded it, `None` for a two-sided
    /// surface.
    pub cull_face: Option<Face>,
}

impl AdditiveMaterial {
    /// The blend state this material draws with, which is the state the surface
    /// recorded and not a second table.
    pub const fn blend(&self) -> &BlendState {
        &self.blend
    }

    /// Whether this material writes depth, as the surface's state recorded it.
    pub const fn depth_write(&self) -> bool {
        self.depth_write
    }

    /// The cull face this material draws with, as the surface's state recorded
    /// it.
    pub const fn cull_face(&self) -> Option<Face> {
        self.cull_face
    }

    /// The image this material samples, when it has one.
    pub const fn base_color_texture(&self) -> Option<&Handle<Image>> {
        self.base_color_texture.as_ref()
    }

    /// The asset path of the fragment shader this material draws with.
    pub const fn fragment_shader_path() -> &'static str {
        ADDITIVE_FRAGMENT_SHADER
    }

    /// The pipeline decisions this material carries, as [`Material::specialize`]
    /// sees them.
    pub const fn pipeline_key(&self) -> AdditiveMaterialKey {
        AdditiveMaterialKey {
            blend: self.blend,
            depth_write: self.depth_write,
            cull_face: self.cull_face,
        }
    }
}

impl Material for AdditiveMaterial {
    /// The class's own fragment shader, loaded from the asset root.
    ///
    /// The vertex stage stays the engine's mesh shader, which is why this file
    /// declares a single entry point and no vertex function.
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Path(AssetPath::from(ADDITIVE_FRAGMENT_SHADER))
    }

    /// Which render phase the surface is queued in, and *not* its blend: see
    /// the module docs. An additive surface is translucent, so it belongs in
    /// the sorted transparent pass; the blend it draws with is
    /// [`AdditiveMaterialKey::blend`].
    ///
    /// This is the field [`AdditiveMaterial::alpha_mode`] holds, read back, so
    /// the pass is the surface's recorded decision rather than a second one
    /// written here. The `StandardMaterial` path reads its own `alpha_mode`
    /// field the same way, and gets its blend from it; this material's blend
    /// travels in the key instead.
    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    /// No depth prepass.
    ///
    /// The prepass writes depth for the surfaces that will be drawn later, and
    /// an additive surface is neither: it does not write depth, and it is
    /// composited against whatever is already there. Running the engine's
    /// default prepass shader for it would add a depth write the render state
    /// does not record.
    fn enable_prepass() -> bool {
        false
    }

    /// No shadow casting.
    ///
    /// Whether an additive surface casts a shadow in the original renderer is
    /// unmeasured, and an unlit additive contribution is not a lit surface, so
    /// the decision is declined rather than invented. Lighting and shadows are
    /// F19's subject.
    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        key.bind_group_data.apply(descriptor);
        Ok(())
    }
}
