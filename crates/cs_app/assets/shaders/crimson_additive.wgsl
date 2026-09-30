// The additive class's fragment shader: the draw call behind
// `AdditiveMaterial` (`crates/cs_app/src/render/additive.rs`).
//
// Every other class draws with `bevy::pbr::StandardMaterial`. This class
// cannot: a `StandardMaterial` takes its blend from its `alpha_mode` and Bevy
// 0.19 maps `AlphaMode::Add` onto the *premultiplied* alpha pipeline, which
// multiplies the source by its own alpha instead of adding it. The material
// type therefore owns its blend state, and this file is the other half of it:
// `AdditiveMaterial::specialize` writes the recorded `One`/`One` blend and the
// recorded "no depth write" into the pipeline descriptor, and the code below
// is what the fragment stage then contributes.
//
// The vertex stage is Bevy's own mesh shader — the material returns
// `ShaderRef::Default` for it — so this file is imported as a fragment shader
// and must therefore declare exactly one fragment entry point: the render
// pipeline resolves a `None` entry point by finding the module's single entry
// point for that stage, and two would be an error.
//
// # What the shader assumes
//
// Every one of these is a *new-engine design decision*, not a measured
// property of the original renderer. `docs/findings/2026-09-30-f17-c-followup-
// additive-material.md` records them with their status.
//
// * The surface is unlit. Nothing here reads a light, and the original's
//   treatment of an additive surface's brightness is unmeasured.
// * The image is sampled with the *image's own* sampler, and an absent image
//   binds Bevy's 1x1 opaque white `FallbackImage`, so an untextured surface
//   contributes the material's own color unchanged.
// * The stored texels are already in the space the render target wants: the
//   upload format F17-B's image adapter chose (`Rgba8Unorm` for a linear
//   image, `Rgba8UnormSrgb` for an sRGB one) does the conversion once, and
//   this shader does not convert again (spec F17 non-negotiable 3).
// * A `One`/`One` blend adds the source *as it is*, so nothing downstream can
//   consume the source alpha. The declared coverage therefore scales the
//   contribution here instead: the material's own alpha (a declared constant
//   opacity) times the texel's alpha times any per-corner color. The
//   destination alpha accumulates with the color, which is inherent to
//   additive blending and not a decision.
// * The surface is drawn with a depth *test* but no depth *write*, so a
//   sprite behind an opaque surface is hidden and a sprite in front of it is
//   not occluded by its neighbours.

#import bevy_pbr::forward_io::VertexOutput

// The material's own uniform, bound by the `AsBindGroup` derive on
// `AdditiveMaterial`: `color` is binding 0, the image 1 and its sampler 2.
// `MATERIAL_BIND_GROUP` is the bind-group index the engine substitutes.
@group(#{MATERIAL_BIND_GROUP}) @binding(0)
var<uniform> color: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1)
var base_color_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2)
var base_color_sampler: sampler;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
#ifdef VERTEX_UVS_A
    let texel = textureSample(base_color_texture, base_color_sampler, mesh.uv);
#else
    // A mesh with no UV attribute cannot address its image. The fallback
    // image bound to an absent `base_color_texture` is opaque white, so this
    // is the same contribution an untextured surface makes: the material's own
    // color. Anything else would be a value this stage cannot establish.
    let texel = vec4<f32>(1.0, 1.0, 1.0, 1.0);
#endif

#ifdef VERTEX_COLORS
    let surface = color * texel * mesh.color;
#else
    let surface = color * texel;
#endif

    // Nothing downstream can read `surface.a` — the blend adds the source
    // whole — so the coverage is applied to what is added.
    return vec4<f32>(surface.rgb * surface.a, surface.a);
}
