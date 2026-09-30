# F17-C: Faithful and optional enhanced rendering profiles

Date: 2026-09-30. Task: F17-C "Add faithful and optional enhanced rendering
profiles" (`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
stage `### F17-C`, AC03). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only — no `CS_GAME_DIR`, no GPU, no
original run. Every fixture is newly authored synthetic content decoded and
composed through the production readers (`cs_content::mesh`, `cs_formats`,
`cs_app::livery`, `cs_app::scene`), so nothing here claims original behavior.
Every value in this stage is `designed`.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/render/profile.rs` (new): `Resolution`,
  `EnhancementKind`, `Enhancement`, `Presentation`, `ProfileParity`,
  `ProfileError`, `msaa_for`, `bevy_tonemapping`, `RenderProfile`.
- `crates/cs_app/src/render/batch.rs` (new): `InstanceVisual`,
  `InstanceVisuals`, `PartRef`, `SubmittedDraw`, `BatchKey`, `BatchInstance`,
  `InstanceBatch`, `WithheldDraw`, `withheld_codes`, `BatchingLimitation`,
  `limitation_codes`, `BatchError`, `BatchedFrame`, `batch_frame`.
- `crates/cs_app/src/render/sync.rs` (new): `RenderSession`,
  `RenderSessionState`, `RenderProfileRequest`, `BatchDraw`, `FrameSync`,
  `PresentationReach`, `RenderTeardown`, `SyncError`, `ProfileEvent`,
  `RenderProfileLog`, `process_render_profile_request`, `batch_key`,
  `sync_frame`, `teardown`.
- `crates/cs_app/src/render/capture.rs` (F17-B's type, completed here — see
  "One F17-B type grew two fields"): `ComparisonSettings::shadows`,
  `ComparisonSettings::render_resolution`, `for_presentation`,
  `with_shadows`, `with_render_resolution`, and the matching
  `CaptureError::SettingsNotFixed` fields and digest bytes.
- `crates/cs_app/src/render/mod.rs`: module doc and the three `pub mod` lines
  (F17-A's and F17-B's contracts untouched).
- Tests (`crates/cs_app/tests/render/`, selected by `accept_f17_c_`):
  `profiles.rs` (9 tests), plus the `main.rs` module line.

**One observable failure:** two aircraft that share a mesh, a render state and
an image, painted differently, batched into one draw. The draw is not wrong in
any way the frame reports — the buffers really are shared — it simply draws
both aircraft with one of the two paints, and nothing downstream can see it.
The symmetric failure is a batcher that ignores damage and keeps drawing a
destroyed wing. Both are invisible unless the per-instance state is part of
what makes a batch a batch, which is what this stage makes it.

## The profile

`RenderProfile::faithful()` is the fidelity baseline: it resolves to exactly
F17-B's fixed comparison set (exposure 1.0, no tone curve, gamma 2.2, one
sample per pixel) plus no shadows and no resolution override. An **enhancement**
is a designed improvement, switchable on its own and recorded as
`ProfileParity::DesignedImprovement`:

| Option | Value | What it reaches | Refused when |
| --- | --- | --- | --- |
| `Antialiasing` | 1, 2, 4 or 8 samples per pixel | the camera's `Msaa` | any other count (Bevy 0.19 has no such mode) |
| `ToneMapping` | `None` or `Filmic` | the camera's `Tonemapping` | never |
| `ShadowMapping` | on | every `DirectionalLight::shadow_maps_enabled` | never |
| `RenderResolution` | a positive `Resolution` | the window's `resolution` | a zero extent |
| `TexturalUpscaling` | — | — | always (`ProfileError::RefusedOption`) |
| `AssetRedistribution` | — | — | always (`ProfileError::RefusedOption`) |

A profile holds at most one option per `EnhancementKind`, stored in the
canonical order of `EnhancementKind::ALL`, so the order of switching cannot
change the profile or its fingerprint, and setting one decision leaves the
others alone.

Three things are structural rather than promised:

1. **An enhancement can only reach `Presentation`.** That type has four fields
   (samples, tone curve, shadows, resolution) and no field for a material
   class, a blend state, a depth write, a cull face, an alpha test, a sort key
   or a collider. Spec F17's deliverable ("enhanced options are independently
   switchable and cannot alter collision or visibility rules") is therefore a
   property of the type; the acceptance test compares the frame batched under
   the fully enhanced profile with the faithful one and requires the draw
   content to be identical.
2. **An enhanced profile is not comparison evidence.** `comparison_settings()`
   returns `ProfileError::NotComparisonEvidence` naming the options that are on,
   and the frame records the parity so a consumer cannot mistake it for a
   baseline.
3. **The two forbidden options are in the vocabulary and always refused.** Spec
   F17 non-negotiable 5 forbids "automatic texture upscaling or asset
   redistribution pipeline"; naming them as `Enhancement` variants that
   `with()`/`new()` reject means a caller who asks for one gets a reason code
   instead of a silent no-op.

### One F17-B type grew two fields

`ComparisonSettings` gained `shadows` and `render_resolution`. Without them the
fixed set accepted the settings of a profile with shadows on, because shadows
change the image and nothing in the set could see it: `is_fixed()` would have
compared equal and `capture` would have produced a "comparable" frame of a
scene that was not rendered the same way. Every field the profile's
`Presentation` owns is now pinned, and `RenderProfile::settings()` builds the
set through one constructor (`ComparisonSettings::for_presentation`) so the
fixed set and the profile cannot disagree about the baseline. F17-B's own test
already matched the error with `..` and constructs its variants with struct
update syntax, so it is unchanged and still passes; its fingerprint covers the
two new values.

`Msaa::Off` has the value 1 in Bevy 0.19, which is why the one-sample baseline
needs no translation table between `Presentation::msaa_samples()` and
`ComparisonSettings::msaa_samples()`. `msaa_for(1) == Msaa::Off` is asserted, so
that coincidence is checked rather than assumed.

## The batch key, and why damage is not in it

`batch_frame` walks the plan in draw order and merges only **consecutive**
entries whose key is equal. The key is the phase, the geometry, the render state
and the image digests F17-B's adapters produced, **and the committed livery
digest** read from the F09-C `ModelLivery` the instance is bound to.

* A different paint is a different batch. Two `LiveryVariantKey`s differ for
  two paints (`cs_content::livery`), so two aircraft painted differently never
  share a draw.
* **An unresolved paint never merges, not even with another unresolved one.**
  Two unresolved paints may be different, so merging them would invent a match.
  Such a batch holds exactly one row (`InstanceBatch::mergeable() == false`)
  and the item is reported in `BatchedFrame::limitations` as
  `UNBOUND_INSTANCE`. This is stronger than a key: a key alone would merge two
  `None`s.
* Damage is not in the key because a destroyed part is not drawn. The item is
  withheld with `DESTROYED_PART` and keeps its key, its instance and its
  reason in the frame's report, so a gameplay, collision or evidence consumer
  can see that the part exists and is not being drawn. Withholding a *draw*
  touches no collider: collision reads the canonical scene, not the frame
  (spec F17 non-negotiable 4).
* Order is never traded for a batch. Two items with the same key that another
  item sits between are two batches; reordering them to merge would change the
  order F17-A established and F17 non-negotiable 1 preserves.

`BatchedFrame::fingerprint` covers the profile, the tick, every batch's key,
merge flag and per-instance rows in draw order, the withheld records with their
reasons, and the limitations. A comparison therefore sees a changed paint, a
destroyed part, a changed profile and a changed *reason* for a missing draw as
four different frames.

## AC03

The stage's minimum scenario is "Two instances with different paint/damage
remain visually independent after batching". The acceptance test draws three
aircraft that share one quad and one image, gives two of them the same committed
paint (through the F09-C `LiveryRuntime`) and one a different one, and destroys
one aircraft's wing (through the F11-C `AirframeDamageState`), then requires:

* the two aircraft with the same paint **are** in one batch — a test that
  passed by never batching anything would prove nothing;
* the third is in a draw of its own, with a different `batch_key`;
* the destroyed wing is withheld with its reason while the identical wing of the
  two intact aircraft is drawn;
* every row keeps its own place and view depth;
* repainting the third aircraft merges it into the shared batch and leaves the
  first aircraft's row **byte-identical**;
* repairing the destroyed wing draws it again into its aircraft-mate's batch;
* the same five draws are a different frame when the sixth is withheld as an
  adapter refusal rather than as a destroyed part.

The consumer test then syncs the frame into a real Bevy `World` and checks one
entity per batch, the per-instance rows on the shared entity, the image handle
each textured batch binds, that a re-sync of the same frame reuses every entity
and does not grow the image store, and that the next frame — which claims less —
despawns what it does not claim.

## Producer and consumer, teardown, retry, error propagation

* **Producer:** `RenderProfileRequest::{Set, TearDown}`, served once per run by
  `process_render_profile_request`, which publishes `RenderSessionState`. A
  `Set` for a different session ends the previous one first; a `Set` for the
  open session replaces only the profile and counts the application. Every
  outcome is appended to `RenderProfileLog` (`Applied`, `Refused`, `TornDown`).
* **Consumer:** `sync_frame` turns a `BatchedFrame` into entities — one per
  batch, with the batch's `Mesh3d`, a `MeshMaterial3d<StandardMaterial>` with
  the batch's image bound, and `BatchDraw` (the batch key, the phase, the image
  handle and the per-instance rows).
* **Retry:** the whole frame is resolved before the first entity is written, so
  a refusal leaves the live frame exactly as it was. A frame built under a
  profile nobody applied is refused (`profile_mismatch`), applying that profile
  and syncing the same frame again succeeds, and the test does exactly that.
* **Session ownership:** a foreign session is refused by the sync, by the
  profile hand-off and by `InstanceVisuals::bind` (which propagates F09-C's
  `foreign_session`), so work built for a finished session is never served.
* **Teardown:** `teardown` despawns every batch entity and drops the state; it
  is a no-op when nothing is live, and a frame synced afterwards is refused with
  `no_render_session`.
* **Error codes:** `foreign_session`, `profile_mismatch`, `no_asset_store`,
  `no_submitted_draw`, `no_render_session`, plus the profile's own
  (`unsupported_sample_count`, `invalid_render_resolution`,
  `textural_upscaling`, `asset_redistribution`, `not_comparison_evidence`) and
  the batcher's (`plan_entry_without_scene_outcome`,
  `submitted_draw_key_mismatch`, `submitted_draw_without_plan_entry`).

## Recorded unknowns and limitations

- **The per-instance paint is not in the pixels yet.** The batch key carries the
  committed livery variant digest, so two paints are two draws, and the batch's
  material binds the batch's own image. How a paint *reaches* the GPU per
  instance — an atlas with a per-instance selection, a texture array layer, or a
  per-instance image handle — is not implemented and not guessed here: it needs
  F08's texture-catalog evidence about how a paint is stored and sampled, and it
  is F17-D's material-coverage question. Until it exists, a batch's material
  samples the shared image and the paint lives in the frame's identity, not in
  the texels.
- **The additive class still has no drawable material.** F17-B's
  `MaterialGap::AdditiveBlendState` is unchanged: a `StandardMaterial` cannot
  express `One`/`One`. The batch carries the gap code and the consumer counts it
  in `FrameSync::unmaterialed` and spawns **nothing** for it, so the additive
  pass is visible in the report rather than faked with another class's blend or
  silently dropped. The material with its own blend state and the WGSL shader in
  `crates/cs_app/assets/shaders/` (still empty) is filed as a follow-up task
  below; it is a separate bounded piece and needs a render app to be more than
  an unconsumed file.
- **The image bound has no load identity.** `sync_frame` adds the image F17-B
  produced to whatever `Assets<Image>` the world has; it is not stamped with an
  installation span, a derived cache key or a load transaction, because this
  stage has no load identity to stamp it with. F15's
  `crates/cs_app/src/assets.rs` `ConvertedAsset` cache is the owner of that, and
  nothing here pretends to be it.
- **Mip levels and a better minification filter are not enhancements here.**
  F17-B recorded `Linear` magnification/minification and no mips, and named
  mip generation a possible F17-C enhancement. It is deliberately not added: a
  mip pyramid synthesizes texels the original never stored and needs a measured
  minification policy and filter, which is F17-D's evidence question. Adding it
  as an option now would be an invented rule with no evidence behind it.
- **The projection and the render resolution are two independent decisions.**
  `Projection::comparison()` is 4:3 and an enhanced profile may set a 16:9
  resolution, so a frame captured at that resolution does not match the pinned
  projection's aspect. The capture refuses any resolution override, so a
  *comparison* frame never has one, but the live path can. Making the camera's
  aspect follow the profile's resolution is the F21 camera stage's decision; the
  profile only resolves and applies a resolution.
- **The shadow option only reaches directional lights.** `apply_presentation`
  sets `DirectionalLight::shadow_maps_enabled`; point and spot light shadow
  settings are not touched, because nothing in the app yet owns those lights
  (F19 owns lighting). `NotShadowCaster`/`NotShadowReceiver` are also not used.
- **Which camera components the presentation reaches is an engine fact, not a
  measurement.** `Msaa` and `Tonemapping` are written on the entities that
  already carry them, and the count of what was reached is reported — including
  zero. What the original 2000 renderer used for sample count, tone curve,
  shadows or resolution is unknown and nothing here claims otherwise.
- **A batch that changed its instance set is a new draw.** `batch_key` covers
  the rows as well as the resources, so two batches can never collide (in
  particular two unresolved-paint batches with identical resources) and a batch
  whose rows changed is respawned rather than quietly redrawn with the old rows.
  The cost is a respawn where a resource-only key would have reused the entity.
- **`instance_count` counts rows, not draws.** A batch of three aircraft is one
  `InstanceBatch` and three `BatchInstance`s; both counts are exposed because a
  report that confuses them would understate what a frame draws.
- **No evidence beyond artifacts.** Every fingerprint here is a
  `cs_types::evidence::FingerprintKind::Artifact` product over synthetic
  fixtures. No pixel value is claimed, no GPU was involved, and no original data
  was read.

## Mutation verification (run, then reverted)

Each mutation was applied to the production modules, the `accept_f17_c_`
selection was run, and the change was reverted. Every one failed at least the
test that names the behavior:

| Mutation | Failing test |
| --- | --- |
| the committed paint dropped from the batch key | `..._two_instances_with_different_paint_and_damage_stay_independent_after_batching`, `..._the_consumer_draws_one_entity_per_batch_with_every_row`, `..._a_frame_under_an_unapplied_profile_is_refused_and_retries_after_applying_it` |
| a destroyed part drawn anyway | `..._two_instances_...`, `..._the_consumer_...`, `..._unresolved_paint_and_part_identity_are_reported_not_assumed` |
| an unresolved paint merges like any other | `..._unresolved_paint_and_part_identity_are_reported_not_assumed` |
| the withheld records dropped from the frame digest | `..._two_instances_...` |
| the profile digest dropped from the frame digest | `..._enhancements_change_no_draw_decision` |
| an enhanced profile accepted as comparison evidence | `..._an_enhanced_profile_is_refused_as_comparison_evidence` |
| the sync stops checking the applied profile | `..._a_frame_under_an_unapplied_profile_is_refused_and_retries_after_applying_it` |
| a stale batch entity not released | `..._the_consumer_draws_one_entity_per_batch_with_every_row` |
| the presentation never applied | `..._the_applied_presentation_reaches_the_camera_light_and_window` |
| the fixed set stops pinning the shadow setting | `..._an_enhanced_profile_is_refused_as_comparison_evidence` |
| an inexpressible sample count accepted | `..._enhancements_are_independently_switchable_and_the_forbidden_ones_refused` |

The first pass found three mutations that no test noticed — the withheld
records, the profile digest and the merge rule were not load-bearing — and the
tests were extended (three unresolved paints in a row, two different enhanced
profiles, a refused surface against a destroyed part) until each one failed.

## Commands run

From the repository root on branch
`rally/71-add-faithful-and-optional-enhanced-rende`, Rust 1.98.

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f17_c_ --include-ignored` | 0 (9 tests, all in `tests/render/profiles.rs`; the 30 F17-A/F17-B tests in the same binary are filtered out) |

## Wiring edits (owner paths only)

All changes are inside the task's owner paths
(`crates/cs_app/src/render/`, `crates/cs_app/tests/render/`,
`docs/findings/`). No protected path, no original datum and no binary file is
involved. `crates/cs_app/Cargo.toml` is unchanged: the stage uses only the
crate's existing dependencies (`bevy` for `Assets`, `Mesh3d`, `MeshMaterial3d`,
`Msaa`, `Tonemapping`, `DirectionalLight`, `Window`, `World`; `cs_assets` for
`install::sha256`; `cs_content` for `SceneNodeId` and `LiveryPaint`; `cs_types`
for `Tick`/`ContentHash`; and the crate's own `livery` and `scene` modules for
the producers). `crates/cs_app/assets/shaders/` is still empty; see the
additive note above.

## Sources

`specs/F17-rendering-material-fidelity-and-scalable-presentation.md` (F17-C
section, AC03, non-negotiables 1–5),
`docs/contracts/IDENTITY-CONTENT.md` (evidence classes, stable ids, session
generations),
`crates/cs_app/src/livery.rs` and `crates/cs_content/src/livery.rs` (the
per-instance paint producer and its deterministic variant key),
`crates/cs_app/src/scene.rs` (`AirframeDamageState`, `SceneNodeId`, the
generation-stamped load/teardown pattern this stage's session follows),
`docs/findings/2026-09-30-f17-b-canonical-mesh-and-image-to-bevy.md` (the
adapters, the fixed comparison set and the two gaps this stage inherited: the
additive material and the texture handle),
`docs/findings/2026-09-30-f17-a-material-classification-and-golden-scene.md`
(the class, phase and ordering rules batching must not disturb),
Bevy 0.19.1 `bevy_render::view::Msaa` (an enum whose `Off` is one sample per
pixel), `bevy_core_pipeline::tonemapping::Tonemapping` (a camera component in
0.19) and `bevy_light::DirectionalLight::shadow_maps_enabled` — engine facts,
not measurements of the original game.
