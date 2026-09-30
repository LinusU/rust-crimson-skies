# F17-B: Canonical mesh and image to Bevy adapters

Date: 2026-09-30. Task: F17-B "Implement canonical mesh/image to Bevy
adapters" (`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
stage `### F17-B`, AC02). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary build/test
only — no `CS_GAME_DIR`, no GPU, no original run. Every fixture is newly
authored synthetic content decoded through the production readers, so nothing
here claims original behavior; the whole design is `designed` and every
declaration carries that status.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/render/bevy_mesh.rs` (new): `AttributeKind`,
  `MeshAdapterError`, `GroupReport`, `GroupUpload`, `upload_group`,
  `upload_groups`.
- `crates/cs_app/src/render/bevy_image.rs` (new): `ImageAdapterError`,
  `CoveragePlane`, `ImageUpload`, `upload_image`.
- `crates/cs_app/src/render/bevy_state.rs` (new): `StateError`, `MaterialGap`,
  `RenderState`, `render_state`.
- `crates/cs_app/src/render/capture.rs` (new): `surface_codes`, `SceneSurface`,
  `upload_surface`, `SurfaceUpload`, `SurfaceRefusal`, `SceneOutcome`,
  `Tonemap`, `ComparisonSettings`, `Projection`/`ProjectionError`,
  `CapturedSurface`, `CapturedPass`, `FrameCapture`, `scene_codes`,
  `CaptureError`, `capture`.
- `crates/cs_app/src/render/mod.rs`: module doc and the four `pub mod` lines
  (F17-A's own contract untouched).
- Tests (`crates/cs_app/tests/render/`, selected by `accept_f17_b_`):
  `fixture.rs` (synthetic canonical inputs), `adapters.rs` (7 tests),
  `frame_capture.rs` (4 tests), plus the `main.rs` module lines.

**One observable failure:** a texture whose stored values are already linear
uploaded as an sRGB texture is corrected a second time by the GPU, and every
surface that samples it comes out washed out — spec F17 non-negotiable 3's
"original texture decoding and GPU sRGB sampling must not double-correct
colors". Symmetrically, a material whose two-sidedness or addressing nothing
established, drawn with an invented cull face or sampler, silently halves or
tiles a surface with no report anywhere.

## What the adapters do

`cs_content::mesh::RenderMesh` (F10-A/C) and
`cs_formats::texture::DecodedImage` (F08) are the canonical handoffs. F08's
findings name this stage as the one that must make the deferred choices
("no color-space conversion, no 565 channel expansion, no alpha baking and no
mip generation happen here: they happen once, in the renderer adapter
(F17-B)"). F17-B makes four of them and refuses the rest:

| Choice | Made from | Refused when |
| --- | --- | --- |
| buffer layout | one compacted buffer per `RenderGroup` | never — the split is this stage's decision |
| color space / texture format | `DecodedImage::color_space` | `ColorSpace::Unknown` |
| coverage encoding | `AlphaSource` | `Unknown`, `PaletteKey`, `StoredValueKey` |
| alpha test | `AlphaTest` | `Unknown` on an image with coverage |
| channel layout | `DecodedFormat` | `Rgb565` (no measured 8-bit expansion) |
| addressing | `ClassifiedMaterial::addressing` | never declared |
| blend / depth / cull / unlit | `MaterialClass` + the declared facts | two-sidedness never declared |

Every refusal is a `&'static str` reason code on a `SurfaceRefusal` the
capture reports next to the surfaces that drew; there is no path that draws a
surface with a default. The rule is uniform: an attribute a group stores for
some corners but not all is refused rather than padded, a texture coverage
source the image contradicts is refused rather than reconciled, a
`Coverage::Texture` with no image is refused rather than given a default
material (F10-C.02's instruction), and a `Coverage::Uniform` with an image is
refused rather than overwriting the image's alpha with the declared constant.

The per-class state table, the class→phase order, the fixed comparison values
and the 60°/4:3 projection are `designed`, the same status F17-A gave the
class→phase map. Nothing asserts what the original renderer did.

## AC02 as an artifact, not a picture

"Screenshot same camera/tick twice under fixed comparison settings" is
implemented as a `FrameCapture`: a value holding the comparison settings, the
camera (`SceneView` pose plus a validated `Projection`), the tick, every
surface's geometry/state/image digests in draw order, every refusal with its
reasons, and the plan's sorting limitations — digested into one
`ContentHash`. Two captures of the same camera and tick are equal exactly when
that digest matches, and the test builds the golden scene twice from
independently decoded inputs to prove it is the adapter that is deterministic
rather than one buffer read twice.

A capture is only produced from the *fixed* comparison set (exposure 1.0, no
tone curve, gamma 2.2, one sample per pixel); anything else is
`CaptureError::SettingsNotFixed`, and the scene must match the plan index for
index or the capture is refused (`MISSING_OUTCOME`, `KEY_MISMATCH`,
`UNUSED_OUTCOME`). The other half of the property is pinned too: a different
tick, pose, projection, vertex buffer or declared render state must all move
the digest, so the equality cannot be an artifact of the capture ignoring
them.

No pixel value is claimed and no GPU was involved. The capture pins what the
renderer was *handed*; whether the GPU then draws it correctly is F17-D's
screenshot matrix with the `gpu` capability.

## Recorded unknowns and limitations

- **Additive surfaces have no drawable material.** `StandardMaterial::alpha_mode`
  is `Opaque`/`Mask`/`Blend` only, so `RenderState::to_standard_material`
  returns `MaterialGap::AdditiveBlendState` for the additive class. The
  *state* is complete (blend `One`/`One`, no depth write) and the capture
  records the gap, so the additive pass is neither dropped nor faked with
  `Blend`. The material with its own blend state — and the shader in
  `crates/cs_app/assets/shaders/` — is F17-C's wiring. `assets/shaders/` is
  still empty, deliberately: a shader with no render app and no material type
  to load it would be an unconsumed file.
- **`Mesh::ATTRIBUTE_COLOR` is a four-component attribute and the IR stores
  three.** The fourth is the declared constant `1.0`, recorded on
  `GroupReport::colors` and covered by `MeshPresentationUnknown::VertexColor`.
  It is not coverage: coverage is `MaterialFacts::coverage`.
- **Front-face winding is still unmeasured.** Bevy treats counter-clockwise
  vertices as front faces; the adapter never reverses an index triple, and the
  `MeshPresentationUnknown` list the caller hands in is carried through
  untouched. Any consumer that inverts the winding must do it from measured
  evidence, not here.
- **A group with no normals gets no normal buffer, and the surface is then
  unlit-dark.** `Mesh::ATTRIBUTE_NORMAL` absent is the only honest outcome
  when no corner stores one; generating normals is a guess, and
  `FrontFaceWinding` is open. Retail rows must state this per mesh in
  F17-D's coverage table.
- **No mip levels, minification filter `Linear`.** F08 forbids generating
  them at decode time, the original's mip policy is unmeasured, and the
  sheet's non-negotiable 5 forbids automatic texture upscaling. Mip
  generation and a better minification filter are F17-C's optional
  enhancements and would change the fixed comparison set, which is why the
  sample count is part of it.
- **Rgb565 textures are refused.** `PresentationUnknown::Rgb565Expansion` is
  unmeasured and the engine would have to pick an expansion, so a 565 image
  cannot be uploaded at all today. This is the adapter's largest known gap
  and it is expected to matter: the expansion policy is a designed engine
  decision that needs a task of its own, not a default. Filed as **#408
  `F17-B-followup-rgb565-expansion`**, which also has to quantify how many
  real textures are affected.
- **Palette-keyed and 565-word-keyed coverage are refused** for the same
  reason: the key lives in a plane the GPU never sees, and two palette
  entries may resolve to the same color, so the key cannot survive the
  resolve.
- **The sampler's magnification filter is `Linear` and is a designed value.**
  Nothing measured the original's filtering, and no per-material filter fact
  exists to read.
- **`StandardMaterial` is a PBR material.** It is used per surface with the
  class's own alpha mode, cull face and unlit flag, so no two surfaces share
  a state, but the *lighting model* is the engine's. The sheet's
  non-negotiable 1 forbids one generic material for every surface; that is
  about per-surface state, and the state is per surface. Whether the engine's
  PBR response is acceptable for the original's materials is F17-D's
  evidence question and may need a custom shader.
- **Binding a texture handle is not done here.** The adapters produce the
  `Image` and the state; turning them into `Handle<Image>` needs an
  `Assets<Image>` and is the consumer's job, which is F17-C's
  producer/consumer wiring together with the `ConvertedAsset`
  (`crates/cs_app/src/assets.rs`) cache/load stamping this stage does not
  have a load identity for.
- **`MaterialUnknown::VertexColorMeaning` and the `Emissive` emission
  color.** `ClassifiedMaterial` carries no emission color, so an emissive
  surface is uploaded `unlit` (F17-A's definition: not dimmed by scene
  lighting) and nothing emits. Where an emissive color comes from is open and
  is F17-D's evidence question; no new task, it is inside the declared scope
  of the material classification's own follow-up.
- **Unknowns needing no other new task:** every other item above is either
  F17-C's declared wiring, F17-D's declared evidence collection, or already
  tracked upstream (`MeshPresentationUnknown`, `PresentationUnknown`). The one
  item no declared stage covers is the Rgb565 expansion policy, filed as #408.

## Mutation verification (run, then reverted)

All eight mutations were applied to the production modules, the `accept_f17_b_`
selection was run, and the change was reverted. Each one failed exactly the
test that names the behavior:

| Mutation | Failing test |
| --- | --- |
| `ColorSpace::Linear` uploaded as `Rgba8UnormSrgb` | `accept_f17_b_image_upload_chooses_the_sampling_format_from_the_color_space` |
| normals normalized on the way to the buffer | `accept_f17_b_mesh_upload_keeps_stored_values_bit_exact` |
| `if !settings.is_fixed()` → `if false` | `accept_f17_b_capture_refuses_settings_that_are_not_the_fixed_set` |
| the tick dropped from the capture digest | `accept_f17_b_capture_depends_on_the_tick_the_camera_and_the_geometry` |
| missing UVs/normals padded with zeroes | `accept_f17_b_mesh_upload_refuses_a_partially_stored_attribute` |
| refusals dropped from the capture | `accept_f17_b_capture_reports_refusals_and_the_plan_limits` |
| the coverage/image agreement check removed | `accept_f17_b_surface_upload_refuses_a_missing_or_contradicting_image` |
| the render state dropped from the capture digest | `accept_f17_b_capture_depends_on_the_tick_the_camera_and_the_geometry` |

## Commands run

From the repository root on branch
`rally/70-implement-canonical-mesh-image-to-bevy-a`, Rust 1.98.

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f17_b_ --include-ignored` | 0 (14 tests: 7 adapters, 4 frame capture, plus 16 F17-A tests filtered out) |

## Wiring edits (outside owner paths, logic-free)

- `crates/cs_app/src/lib.rs`: no change. The four new modules are declared in
  `crates/cs_app/src/render/mod.rs`, which is inside the owner path
  `crates/cs_app/src/render/`.
- `crates/cs_app/Cargo.toml`: no change. The adapters use only the crate's
  existing dependencies (`bevy` for `Mesh`/`Image`/`BlendState`/
  `StandardMaterial`, `cs_content` for `RenderMesh`/`RenderGroup`/
  `MeshPresentationUnknown`, `cs_formats` for `DecodedImage`/`AlphaSource`/
  `AlphaTest`/`ColorSpace`, `cs_assets` for `install::sha256`, `cs_types` for
  `Tick`/`ContentHash`).
- `crates/cs_app/assets/shaders/`: still empty, see the additive note above.

No protected path, original datum or binary file is involved.

## Sources

`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`
(F17-B section, AC02, non-negotiables 1–5),
`docs/contracts/IDENTITY-CONTENT.md` (evidence classes, stable ids,
fingerprint kind),
`crates/cs_content/src/mesh.rs` (`RenderMesh`, `RenderGroup`,
`RenderVertex`, `RenderTriangle::degenerate`, `MeshPresentationUnknown`),
`docs/findings/2026-09-29-f10-c-01-render-vertex-splitting.md` (the buffer
split this stage had to decide),
`docs/findings/2026-09-29-f10-c-03-mesh-container-catalog-and-upload.md`
(the `MeshUpload` unknown list the caller hands in),
`docs/findings/2026-09-29-f10-c-02-gamez-material-records.md` (a missing
texture may never become a default material),
`crates/cs_content/src/textures.rs` and
`docs/findings/2026-09-28-f08-c-texture-catalog-and-upload-boundary.md`
(the conversion this stage owes, and every `PresentationUnknown` it read
before choosing),
`docs/findings/2026-09-30-f17-a-material-classification-and-golden-scene.md`
(the classification, the phase order and the class→phase map this stage
extends),
`docs/findings/2026-09-28-f10-a-lossless-mesh-ir-and-strip-fixtures.md` (the
"nothing is changed on the way out" rule the mesh adapter holds to).
