# F17-C follow-up: the additive class's material and its WGSL shader

Date: 2026-09-30. Task: #409 `F17-C-followup-additive-material` "Give the
additive class a drawable material and its WGSL shader", a follow-up to
`specs/F17-rendering-material-fidelity-and-scalable-presentation.md` stage
`### F17-C`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only — no `CS_GAME_DIR`, no GPU, no
original run, no original executable. Every input is newly authored synthetic
content decoded through the production readers, so nothing here claims original
behavior and every value in this stage is `Designed`.

This record supersedes the additive note in
`2026-09-30-f17-b-canonical-mesh-and-image-to-bevy.md` and in
`2026-09-30-f17-c-profiles-and-instance-batching.md`. Both said the gap was
open; it is closed. Their other content is unchanged and still true.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/assets/shaders/crimson_additive.wgsl` (new): the class's one
  fragment entry point.
- `crates/cs_app/src/render/additive.rs` (new): `ADDITIVE_FRAGMENT_SHADER`,
  `AdditiveMaterialKey`, `AdditiveMaterial`, `impl Material for
  AdditiveMaterial`.
- `crates/cs_app/src/render/bevy_state.rs`: `MaterialGap` and
  `RenderState::to_standard_material` are replaced by `MaterialKind`,
  `DrawableMaterial` and `RenderState::to_drawable_material`; the additive
  class's `alpha_mode` becomes `Blend`; `standard_base_color` and
  `additive_color`.
- `crates/cs_app/src/render/capture.rs`: `SurfaceUpload::material` /
  `material_kind` in place of `standard_material` / `material_gap`;
  `CapturedSurface::material_kind`.
- `crates/cs_app/src/render/batch.rs`: `InstanceBatch::material_kind` in place
  of `material_gap`.
- `crates/cs_app/src/render/sync.rs`: `BatchMaterial`, `add_material`,
  `stored_material`, `set_material`; `Assets<AdditiveMaterial>` joins the
  required stores; `FrameSync::unmaterialed` is removed.
- `crates/cs_app/src/render/mod.rs`, `crates/cs_app/src/lib.rs`: module
  declarations and the two module docs (doc comments only).
- Tests (`crates/cs_app/tests/render/`, selected by `accept_f17_c_additive_`):
  `additive_material.rs` (10 tests), plus the `main.rs` module line and the
  four existing call sites that named the removed API.

**One observable failure:** an additive sprite with a complete render state
(`One`/`One`, no depth write) and no drawable material. F17-C's consumer counted
such a batch in `FrameSync::unmaterialed` and spawned **nothing** for it, so the
additive pass — muzzle flash, engine glow, tracer light — was absent from every
frame with no error anywhere. The symmetric failure is a "fix" that gave the
class a material by mapping `AlphaMode::Add` onto a `StandardMaterial`: that
blend is Bevy's *premultiplied* pipeline, which multiplies the source by its own
alpha instead of adding it, so a half-bright sprite that is bright where it is
transparent and dark where it is not, with nothing to report it.

## What the class draws with now

`RenderState::to_drawable_material()` returns a `DrawableMaterial`:

| Class | Material | Blend | Depth write | Why |
| --- | --- | --- | --- | --- |
| `Opaque`, `Masked`, `Emissive` | `Standard` | `REPLACE` | yes | a `StandardMaterial` mode carries it |
| `Blended` | `Standard` | `SrcAlpha`/`OneMinusSrcAlpha` | no | `AlphaMode::Blend` *is* the blend input |
| `Additive` | `Additive` | `One`/`One` | **no** | no `alpha_mode` reaches it |

The `Standard` variant boxes its `StandardMaterial`. The two variants are very
different sizes (`StandardMaterial` is a full PBR description with a dozen
optional textures, `AdditiveMaterial` is one uniform and one optional image),
and this enum sits inside every `SurfaceUpload`, so one allocation per upload
beats carrying ~350 unused bytes per submitted surface. Clippy's
`large_enum_variant` is what surfaced the difference; the size ratio is the
reason, not the lint.

The additive material's blend, depth write, cull face **and** pass are **copied
from the `RenderState`**, not re-derived. `bevy_state::ADDITIVE` remains the only
place the `One`/`One` value is written down, and
`accept_f17_c_additive_the_additive_class_yields_a_drawable_one_over_one_material`
asserts each of the four equals `state.*` — a second table in the additive module
would fail it. (The pass was the fourth: it began as a hard-coded
`AlphaMode::Blend` in `additive.rs`, which left the state's `alpha_mode` for this
class a decision nothing read. See the review section at the end.)

### The blend is written where wgpu 29 keeps it

An engine fact this stage had to establish rather than assume: in wgpu 29 a
blend state is a field of each `ColorTargetState` in the **fragment stage**, not
of `PrimitiveState` (`bevy_pbr` 0.19.1's own `MeshPipeline::specialize` puts
`ALPHA_BLENDING` there for `AlphaMode::Blend`). So
`AdditiveMaterialKey::apply` writes it into every fragment target, replacing
exactly the field the alpha mode would have written, and
`accept_f17_c_additive_the_material_specializes_its_blend_and_depth_write_into_the_pipeline`
starts from a descriptor in the `AlphaMode::Blend` state and requires `One`/`One`
and `depth_write_enabled: false` out of it. A material that only *carried* a
blend would pass a value assertion and draw with the alpha blend.

### `alpha_mode` on this material is the pass, not the blend

`AdditiveMaterial` carries the mode its surface's render state recorded (the
state table gives the additive class `AlphaMode::Blend`) and
`Material::alpha_mode` returns that field, read back. The reason it is not the
blend: on a material that owns its blend state, the alpha mode's only remaining
meaning is which render phase the surface is queued in, and an additive surface
is translucent. `AlphaMode::Blend` maps to `RenderPhaseType::Transparent` in the
engine (bevy_pbr 0.19.1, `queue_material_meshes` and the extraction that sets
`MaterialProperties::render_phase_type`), so an additive surface is depth-sorted
against the translucency in front of it. `Opaque` would have put it in the
binned opaque phase, where it is neither sorted nor mixed in the engine's order
— and F17 non-negotiable 1 preserves material ordering and 2 asks for
translucent layers to be handled consistently. The blend is
`AdditiveMaterialKey::blend`, and the specialization overwrites the mode's own
blend.

What the mode does *not* give this class is a pass of its own: the engine's
transparent phase is one sorted phase, so an additive surface is sorted against
the other translucency by its mesh centre rather than drawn after it. The plan's
`RenderPhase::Additive` ordering is F17-A's decision and is unchanged; the
mapping from that phase to the engine's single transparent phase is a
`Designed` choice recorded in the assumptions table (row 10) and the limitation
below.

### The consumer

`sync_frame` now requires `Assets<AdditiveMaterial>` alongside the other three
stores, and refuses a world without it with
`SyncError::NoAssetStore { kind: "AdditiveMaterial" }` before writing anything —
a missing store is a refusal, not a half-written frame. Per batch it binds
either `MeshMaterial3d<StandardMaterial>` or
`MeshMaterial3d<AdditiveMaterial>`, so the component's own type is the material
and the two cannot be confused; `BatchMaterial` carries the handle plus that
distinction through the placement path, which is why `place_rows` inserts the
material after the spawn rather than in the bundle.

`FrameSync::unmaterialed` is **removed** rather than left at zero: no class can
reach the consumer without a material, so a counter that can only ever be zero
is a claim nothing can keep true. `MaterialGap` goes with it, and the reason
code `additive_blend_state_needs_custom_material` is gone from the machine-
readable surface. What replaces the report is `MaterialKind`, carried on
`CapturedSurface` and `InstanceBatch` and part of neither digest (see below).

### The shader

`crates/cs_app/assets/shaders/crimson_additive.wgsl` is loaded by
`Material::fragment_shader()` as a `ShaderRef::Path`, which the engine resolves
through the asset server from the crate's asset root — the same load any other
`.wgsl` in the project takes, and the one place a WGSL file belongs rather than
a Rust string. The vertex stage stays the engine's mesh shader
(`ShaderRef::Default`), so the file declares exactly one entry point: the render
pipeline resolves a `None` entry point by finding the module's *single* entry
point for that stage, and two would be `MultipleEntryPointsFound`
(wgpu-core 29.0.4, `finalize_entry_point_name`).

The bindings are the ones the `AsBindGroup` derive generates on the material:
`0` the `LinearRgba` color, `1` the optional base-color image, `2` its sampler,
at `#{MATERIAL_BIND_GROUP}` — the index the engine substitutes, not a hard-coded
`3`. An absent image binds the engine's 1×1 opaque white `FallbackImage`, which
is why the shader may sample unconditionally and why an untextured additive
surface contributes the material's own color rather than an invented texel.

Two tests read the file from disk, so removing or renaming it fails:
`..._the_material_loads_a_wgsl_shader_that_exists` (the `fragment` entry point,
the three bindings, the `MATERIAL_BIND_GROUP` placeholder, and the *absence* of
a `vertex` function) and `..._the_shader_file_is_the_one_the_additive_material_uses`
(the file is the only one in `assets/shaders/`, so there is no unconsumed shader
beside it). A WGSL string in Rust, or a file no material loads, would fail both.
The second test is a deliberately strict inventory: a second shader in that
directory has to extend the test rather than appear beside an unconsumed one,
which is the friction it is for.

## What the shader assumes, and each assumption's status

Every row is a `Designed` new-engine decision. None is a measurement of the
2000 renderer.

| # | Assumption | Status | Why it is open |
| --- | --- | --- | --- |
| 1 | The surface is unlit; nothing reads a light. | `Designed` | the original's treatment of an additive surface's brightness is unmeasured. F19 owns lighting. |
| 2 | The stored texels are already in the target's space; the shader does not convert again. | `Designed`, and *bounded* by F17-B | the upload format F17-B's image adapter chose (`Rgba8Unorm` for linear, `Rgba8UnormSrgb` for sRGB) does the conversion once, so this is F17 non-negotiable 3 held rather than a new claim. |
| 3 | A declared constant coverage scales the contribution, because an `One`/`One` blend has no blend factor to read it with. | `Designed` | nothing measured how the original modulated an additive surface. The uniform's alpha is the declared byte over 255, normalized and not re-encoded, asserted in `..._the_additive_class_records_a_blend_no_alpha_mode_carries`. |
| 4 | The stored alpha accumulates in the destination alpha channel. | `Designed` and **inherent** | an `One`/`One` blend adds both channels; this is not a choice the shader can make. |
| 5 | Per-corner colors multiply the surface, rather than replacing the base color as `pbr_fragment.wgsl` does. | `Designed` | `MaterialUnknown::VertexColorMeaning` is open: how the original applied per-corner colors was never measured, and this differs from the PBR path on purpose so a declared coverage is not discarded. |
| 6 | No depth prepass. | `Designed` | the prepass writes depth for surfaces drawn later, and an additive surface neither writes depth nor is composited against. The engine's default prepass shader would add a depth write the state does not record. |
| 7 | No shadow casting. | `Designed` | whether an additive surface casts a shadow in the original is unmeasured; an unlit contribution is not a lit surface. F19 owns shadows. |
| 8 | The image is sampled with the *image's own* sampler (`Linear` magnification, no mips). | `Designed`, inherited | F17-B recorded the sampler's filters and refused mip generation; unchanged here. |
| 9 | The additive surface is depth-*tested* but writes no depth. | `Designed`, and bounded by the state | "no depth write" is the render state's own recorded decision; the test is the engine's default (`GreaterEqual`) and is not a decision this stage makes. |
| 10 | The additive surface is drawn in the engine's *one* sorted transparent phase, interleaved by depth with the other translucency — not in a pass of its own after all of it. | `Designed` | the material's `alpha_mode` is `Blend`, which is the only mode the engine maps to `RenderPhaseType::Transparent`; the engine has no additive phase, so `RenderPhase::Additive`'s "after all translucency" order is a *plan* order only. What the original did with a sprite that is both additive and among transparent surfaces was never measured, and this is the decision F17-D's `gpu` + `retail` evidence has to look at. |

## Recorded unknowns and limitations

- **Nothing here is pixel-verified.** A blend state, a material type, a
  specialization key and a file's contents are all checkable without a device,
  and that is all this stage checks. Whether the additive pass comes out
  additive, sorted and un-occluded is F17-D's `gpu` + `retail` evidence, and
  this stage is not a substitute for it.
- **Nothing here parses the WGSL either.** The two shader tests read the file
  and check its entry point, its three bindings and the `MATERIAL_BIND_GROUP`
  placeholder, so the file cannot go missing or be renamed unnoticed. A
  *syntactic* error inside it, or a binding the engine's generated layout does
  not match, would still pass: the engine's preprocessor owns `#import` and
  `#{...}`, so naga cannot read the file as it stands, and a first `App` is the
  only thing that will actually compile it (see the next bullet). The bindings
  were therefore checked by hand against `bevy_pbr::forward_io::VertexOutput`
  and the `AsBindGroup` derive rather than by a compiler, and this is a known
  gap rather than a claim that the file is known good.
- **The shader is loaded from an asset root the app has to agree on.**
  `ShaderRef::Path` resolves through the asset server against
  `AssetPlugin::file_path` (default `assets`) joined onto bevy_asset's
  `get_base_path()`: `$BEVY_ASSET_ROOT`, else `$CARGO_MANIFEST_DIR`, else the
  executable's parent directory. Because the `cs` binary lives in `cs_app`, the
  default finds `crates/cs_app/assets/` under `cargo run`/`cargo test`; a
  release build, or a future binary in another crate, has to set one of those
  itself. Nothing in this stage sets it, because there is no `App` to set it
  from.
- **The pipeline is never built here.** `AdditiveMaterialKey::apply` is checked
  against a hand-built `RenderPipelineDescriptor` in the state `AlphaMode::Blend`
  leaves it in, not against a descriptor the engine's `MeshPipeline` produced. No
  render device is available without a GPU, so the base pipeline's own
  contributions (topology, sample count, target format) are the engine's and
  were read in the 0.19.1 sources rather than observed. A first `App` with a
  render sub-app is F00+/F20's work and will compile this shader for real.
- **`Assets<AdditiveMaterial>` must be registered by whatever builds the app.**
  `sync_frame` refuses a world without the store, which is honest, but the crate
  has no `App` yet (F00-A's synthetic scene is asset-free and headless — see the
  root `Cargo.toml` note on `avian3d`'s `collider-from-mesh` feature). Nothing
  calls `app.init_asset::<AdditiveMaterial>()`; there is nowhere to call it from.
- **The material-kind field is in neither digest, deliberately.** The class is
  already inside `RenderState`'s fingerprint, which `CapturedSurface::state` and
  `BatchKey::state` both digest, so a separate kind byte could only differ where
  the state already differs. It is redundancy, not a claim, and it was removed
  after mutation testing showed no test noticed its removal. The field is still
  the *reported* value on `CapturedSurface` and `InstanceBatch`, and
  `accept_f17_c_additive_the_reported_material_kind_follows_the_state_class`
  pins it to the class through all three report sites for all five classes.
- **`AdditiveMaterialKey`'s three accessors are not on a mutation-checked path.**
  `AdditiveMaterialKey::cull_face()` returning a constant instead of the field
  changed no test, because nothing calls it: the specialization reads `self`
  directly and `AdditiveMaterial::pipeline_key()` builds the key from the
  material. They are kept because they are the key's public read surface and a
  consumer asking "what does this specialization do?" has to be able to ask, but
  the honest statement is that they are not proven load-bearing.
- **The additive class is still `RenderPhase::Additive`, and the material did
  not change the plan.** The ordering is F17-A's, and the mutation check confirms
  it: mapping `MaterialClass::Additive` to `RenderPhase::Translucent` fails
  `..._the_additive_pass_is_drawn_after_the_translucency_before_it`.
- **The per-instance paint still does not reach the texels.** Unchanged by this
  stage and still owned by #410: a batch's material binds the batch's own image,
  and how a paint reaches the GPU per instance is F08's texture-catalog question
  plus F17-D's.
- **A batch that changed its material kind is a new draw, and it cannot happen.**
  `BatchKey::state` digests the render state, which digests the class, which
  decides the kind. The kind is in the batcher's merge predicate as a second
  guard, not because a key collision is possible.
- **The image bound to the additive material has no load identity**, exactly as
  F17-C recorded for every other class: `sync_frame` adds the image to whatever
  `Assets<Image>` the world has, with no installation span, derived cache key or
  load transaction. F15's `ConvertedAsset` cache owns that and nothing here
  pretends to be it.
- **No evidence beyond artifacts.** Every fingerprint involved is a
  `FingerprintKind::Artifact` product over synthetic fixtures. No pixel value is
  claimed, no GPU was involved, and no original data was read.

## Mutation verification (run, then reverted)

Each mutation was applied to the production modules, the selection was run, and
the change was reverted. Nine of eleven failed at least the test that names the
behavior:

| Mutation | Failing test |
| --- | --- |
| the additive class takes the `StandardMaterial` path (`if false` on the match arm) | `..._yields_a_drawable_one_over_one_material`, `..._no_alpha_mode_carries`, `..._a_non_additive_class_is_unaffected`, `..._an_additive_batch_is_spawned...`, `..._specializes_its_blend...` |
| the specialization writes `ALPHA_BLENDING` instead of the recorded blend | `..._specializes_its_blend_and_depth_write_into_the_pipeline` |
| the specialization forces `depth_write_enabled: Some(true)` | `..._specializes_its_blend_and_depth_write_into_the_pipeline` |
| the WGSL file removed | `..._the_material_loads_a_wgsl_shader_that_exists`, `..._the_shader_file_is_the_one_the_additive_material_uses` |
| the shader path names a file that does not exist | both shader tests |
| `AdditiveMaterial::cull_face()` returns `Some(Face::Back)` regardless of the state | `..._yields_a_drawable_one_over_one_material` |
| `Assets<AdditiveMaterial>` no longer required by `sync_frame` | `..._an_additive_batch_is_spawned_with_its_own_material_and_image` |
| the additive class ordered as `RenderPhase::Translucent` | `..._the_additive_pass_is_drawn_after_the_translucency_before_it` |

**Two mutations failed nothing, and both are documented above rather than
papered over:**

1. `material_kind`'s byte dropped from `FrameCapture`'s digest — nothing failed,
   because the class is already inside the state digest those bytes sit beside.
   The byte was **removed** and the redundancy is now stated in the code, the
   findings and a test that pins the *reported* kind to the class instead.
2. `AdditiveMaterialKey::cull_face()` returning a constant instead of the field
   — nothing failed, because no caller reaches it (see the recorded limitation).
   The accessor is kept and the fact is recorded.

A third attempt — giving the material its own *different* `One`/`One`-shaped
blend constant instead of copying the state's — also failed nothing, which is
correct: a constant that is byte-equal to the state's is not a second table in
any observable sense. The divergence case (a constant that differs) does fail
`..._yields_a_drawable_one_over_one_material`, since that test compares the
material's blend with `state.blend()` rather than with a literal.

## Commands run

From the repository root on branch
`rally/409-give-the-additive-class-a-drawable-mater`, Rust 1.98.1.

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f17_c_additive_ --include-ignored` | 0 (10 tests, all in `tests/render/additive_material.rs`; the 44 other render tests in the same binary are filtered out) |

`cargo test --locked -p cs_app --test render` (the whole F17 selection, 54
tests) also passes: the four existing call sites that named the removed API were
updated and their assertions preserved, not deleted (see below).

### What changed in the earlier stages' tests

Four call sites named API this stage replaced. Each was updated to the new name
with its assertion's *intent* intact, and two gained a strictly stronger form:

- `adapters.rs` (F17-B): the additive branch used to assert
  `to_standard_material()` returns `MaterialGap::AdditiveBlendState`, "the state
  is complete even where the drawable material is not". It now asserts the
  additive state produces `MaterialKind::Additive`, that no `StandardMaterial`
  is what it draws with, and that the material's blend, depth write and cull
  face are the state's — a superset of the old assertion. The other three
  classes' `to_standard_material` calls became `standard_material(&...
  .to_drawable_material())` with their assertions unchanged.
- `frame_capture.rs` (F17-B): the fence's `material_gap().is_none()` became
  `material_kind() == MaterialKind::Standard`; the sprite's
  `material_gap() == Some("additive_blend_state_needs_custom_material")`
  became `material_kind() == MaterialKind::Additive`. The *reason code* string
  is gone because there is no longer a gap to report — but the surface is still
  captured and still named, which is what the assertion was for.
- `profiles.rs` (F17-C): `report.unmaterialed == 0` was removed with the field
  (it was an assertion that a counter was zero). The additive class is covered
  by `..._an_additive_batch_is_spawned_with_its_own_material_and_image` instead.
  The test world gained `Assets<AdditiveMaterial>`, because the consumer now
  requires it.
- No assertion was weakened, skipped or deleted anywhere. The F17-A and F17-B
  selections are unchanged in count.

## Wiring edits (owner paths only)

All changes are inside the task's owner paths (`crates/cs_app/src/render/`,
`crates/cs_app/assets/shaders/`, `crates/cs_app/tests/render/`,
`docs/findings/`), except two doc comments, which are permitted wiring under
AGENTS.md rule 1: `crates/cs_app/src/lib.rs` (the crate-level `render`
paragraph, which listed the render modules and now names the additive material
and its shader) and `crates/cs_app/src/render/mod.rs` (the module
declaration). No logic is in either, and `crates/cs_app/Cargo.toml` is unchanged:
the stage uses only the crate's existing `bevy` dependency
(`Asset`, `AsBindGroup`, `TypePath`, `Material`, `MaterialPipeline`,
`MaterialPipelineKey`, `ShaderRef`, `BlendState`, `ColorTargetState`,
`RenderPipelineDescriptor`, `MeshMaterial3d`, `Assets`, `AlphaMode`) and the
crate's own `render` modules. `Cargo.lock` is unchanged. No protected path, no
original datum and no binary file is involved.

The new `crates/cs_app/assets/` directory is the crate's asset root, which
Bevy's `AssetPlugin` resolves by default (`assets` under `$BEVY_ASSET_ROOT`, else
`$CARGO_MANIFEST_DIR`, else the executable's directory — see the limitation
above). It contains exactly one file, the shader, and a test asserts that.

## Sources

`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`
(non-negotiables 1, 2, 3; AC01's additive sprite),
`docs/contracts/IDENTITY-CONTENT.md` (evidence classes, stable ids, artifact
fingerprints),
`docs/findings/2026-09-30-f17-b-canonical-mesh-and-image-to-bevy.md` (the gap,
and why `AlphaMode::Add` is the wrong fix),
`docs/findings/2026-09-30-f17-c-profiles-and-instance-batching.md` (the consumer
and what it did with a gapped batch),
`docs/findings/2026-09-30-f17-a-material-classification-and-golden-scene.md`
(the class-to-phase map and the coverage vocabulary),
`crates/cs_app/src/render/bevy_state.rs` (the `ADDITIVE` constant and the
per-class state table, unchanged by this stage),
Bevy 0.19.1 / wgpu 29.0.4 sources read rather than assumed: `Material::specialize`
and `MaterialPipelineSpecializer` (the specialization order — the user
specialize runs after the base pipeline's blend was written),
`queue_material_meshes` and `alpha_mode_pipeline_key` (`AlphaMode::Add` shares
`BLEND_PREMULTIPLIED_ALPHA`; `Blend` maps to `RenderPhaseType::Transparent`),
`MeshPipeline::specialize` (blend lives on the fragment `ColorTargetState`;
`cull_mode` on `PrimitiveState`), `AsBindGroup`'s `bind_group_data` docs (an
`Option<Handle<Image>>` binds the 1×1 white `FallbackImage`; the key must be
`Clone + Hash + PartialEq + Send + Sync`), and wgpu-core's
`finalize_entry_point_name` (a `None` entry point requires a single entry point
for the stage) — all engine facts, not measurements of the original game.

## Review (bunny-2, second pass)

The same agent that implemented this stage reviewed it: the context was fresh
(a new session with no history of the implementation) but the identity is the
same, so this is **not** independent evidence. It is recorded here so the merge
decision carries the fact rather than a fiction.

Every engine claim above was re-checked against the 0.19.1 / 29.0.4 sources in
the cargo registry rather than accepted, and one candidate finding was
**discarded** because the source refuted it: in Bevy 0.19 tonemapping is a
post-processing node, not per-material shader code, so the additive shader
bypassing the PBR fragment chunks does not bypass `apply_presentation`'s
`Tonemapping`. Nothing was recorded on that basis.

Four fixes:

1. **`AdditiveMaterial` now carries the pass instead of hard-coding it.**
   `Material::alpha_mode` returned a literal `AlphaMode::Blend`, so the additive
   class's own `RenderState::alpha_mode` — which this stage changed from
   `Opaque` to `Blend` and the findings above explain — was a decision *nothing
   read*: a class whose state said `Opaque` would still have been queued in the
   transparent pass, silently. The mode is now a field copied from the state
   (the fourth decision, beside blend, depth write and cull face) and
   `Material::alpha_mode` returns the field, exactly as the `StandardMaterial`
   path reads its own. Mutation-checked: making the additive arm of the state's
   `alpha_mode` table fall through to `Opaque` now fails
   `..._yields_a_drawable_one_over_one_material` **and**
   `..._records_a_blend_no_alpha_mode_carries`; before this change it failed
   nothing.
2. **`sync_frame` no longer clones a material per batch per frame.** The refactor
   took `upload.material().clone()` before the reuse check, so a stable frame —
   every batch reused — paid a heap allocation of a whole `StandardMaterial` per
   batch per frame that the previous code only paid when it added one. The
   upload's material is now borrowed and cloned inside `add_material`, i.e. only
   in the branch that adds a value to a store. Same behaviour, no per-frame
   allocation on the steady-state path.
3. **Three stale docs, one of them a broken link.** `sync_frame`'s doc still
   claimed "a batch whose material gap is still open writes nothing and is
   counted in `FrameSync::unmaterialed`" — a field this stage removed, so the
   intra-doc link did not resolve (`cargo doc` reported it) and the sentence was
   simply false; `render/mod.rs` linked the same removed field; and
   `bevy_state.rs`'s `MaterialKind::Additive` doc linked the *private*
   `ADDITIVE` constant. All three rewritten, and the sync doc now says which
   material each class draws with. `cargo doc --no-deps -p cs_app` now emits 25
   warnings, down from 27 on the branch head and all of them pre-existing on
   `main`: nothing this stage documents produces one.
4. **Two limitations recorded that the stage did not state**: that nothing
   *parses* the WGSL (the engine's preprocessor owns `#import` and `#{...}`, so
   naga cannot read the file as it stands, and a typo inside it would pass
   every test here), and how the shader path is actually resolved
   (`$BEVY_ASSET_ROOT` / `$CARGO_MANIFEST_DIR` / executable directory), which
   only finds `crates/cs_app/assets/` by default because the `cs` binary lives in
   this crate. Plus assumption row 10: the engine's transparent phase is one
   sorted phase, so an additive surface is interleaved by depth with the other
   translucency rather than drawn after it — the plan's `RenderPhase::Additive`
   order is a plan order only, and that gap is F17-D's `gpu` + `retail` evidence
   to look at.

### Commands run by the review

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f17_c_additive --include-ignored` | 0 (10 tests) |
| `cargo doc --no-deps -p cs_app --locked` | 0 (25 warnings, all pre-existing on `main`) |

Verdict: the stage meets its acceptance criteria, the earlier stages'
assertions were updated without being weakened, and the four fixes above are the
only changes the review made. What is still not claimed is unchanged: nothing in
this stage is pixel-verified, and the assumptions table is all `Designed`.
