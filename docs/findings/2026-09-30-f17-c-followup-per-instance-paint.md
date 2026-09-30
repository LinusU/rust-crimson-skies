# F17-C follow-up: how a per-instance paint reaches the GPU

Date: 2026-09-30. Task: Rally #410, `F17-C-followup-per-instance-paint`
("Decide how a per-instance paint reaches the GPU"), the gap
`docs/findings/2026-09-30-f17-c-profiles-and-instance-batching.md` left open:
batching kept the livery *identity*, nothing put the paint in the *texels*.
Spec: `specs/F17-rendering-material-fidelity-and-scalable-presentation.md`
(F17-C, AC03). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: `retail` (read of `$CS_GAME_DIR`), plus ordinary
build/test. No GPU run, no original run. Implementation and verification by
the agent instance `devin-1` in one session; **the same identity implemented
and reviewed, so this is not independent evidence** and nothing here is
raised above `implemented` on the strength of it.

## The question and the candidate mechanisms

The task asked for a decision, not a guess: does a per-aircraft paint reach
the GPU as

1. a composed image bound per batch (the paint is in the texels),
2. a shared atlas with a per-instance region,
3. a texture-array layer selected per instance, or
4. some other per-instance image mechanism?

## What the evidence establishes

**Storage — retail-measured (`Documented` + this task's corpus run).** The
installation stores no painted texture. `GOSDATA/ASSETS/crimson.rof` holds
184 `.bm` members under `GRAPHICS/<FACTION>/<PREFIX>_<PART>.bm`; each is one
image with an RGB base plane, three grayscale mask planes and an RGBA
overlay (observed subset, `docs/research/FORMAT-NOTES.md` "BM observed
subset [S09, S10]"). A paint is the mask planes plus a color choice, never a
baked image (F09-D,
`docs/findings/2026-09-29-f09-d-stock-liveries-and-combinations.md`).

**Composition — `ObservedTool`.** The pinned helper [S10]
(`set_paintjob.py`, blob `ccf7c4ea065c17a44d354e8d703561cdc94dd518`)
composes the planes into **one whole RGB8 image** per (member, paint), and
F09-D matched the production `compose_livery` to it at every texel of the
library. The paint therefore reaches the texture **in the texels**:
composition happens on the CPU, not in a per-pixel shader selection.

**Measured this task.** The `accept_f17_c_paint_` retail test composes every
member under two paints that differ on mask planes 1 and 3 only
(`PAINT_ONE`/`PAINT_TWO` in `crates/cs_app/tests/render/paint.rs`):

- 159 of 184 members' composed texels depend on the paint.
- The 25 unchanged members — spinners, engines, flaps, gun parts — are
  *exactly* the members whose first and third masks are all-zero (asserted
  per texel, not assumed), so only the shared second mask contributes.
- Every member's variant key still differs between paints — the paint is
  always part of the identity even where the masks make no difference.
- Per-member detail: `private/evidence/F17-C-PAINT/paint-members.json`
  (digests only, no pixels); committed report
  `docs/findings/evidence/F17-C-PAINT.json`.

That is the strongest statement the data supports: *where a mask plane is
nonzero, the paint is in the composed bytes; where it is zero, it is not.*
The paint is stored in the masks.

**The member naming — observed, unresolved.** The `<PREFIX>_<PART>` spelling
is per airframe part (`BRI_WING`, `DEV_ENGINE`, `PEA_COWLING`, `FUR_CANNON`,
…), so a livery addresses a part of the airframe, not the whole model.
Which scene node a part names is not established (F09-PREFIX, Rally #386).

**The alternatives — `Unknown`, not implemented.** No atlas layout, region
table, array stride or layer index exists anywhere in the observed data, so
mechanisms 2 and 3 have no evidence. Mechanism 4 is unbounded; the observed
data contains no hint of one either.

## The decision

**One image per variant, bound per batch** — mechanism 1, the only one the
evidence supports. The `BatchKey` already carries the committed
`LiveryVariantKey` (batch.rs now keeps the key itself, not only its digest),
so every instance in a batch paints the same and "per-instance paint"
reduces to "per-variant image". At sync time a textured batch with an
established paint binds the composed variant image **in place of** the
surface's canonical image — for a livery surface those are two reads of the
same texture, and the painted variant is what the original's paint data
describes. Two batches sharing a variant *and* its sampling contract (other
phase, non-consecutive items) bind the same Bevy `Image` handle — the dedupe
key is the upload's fingerprint, which carries variant, extent, addressing
and texels, so a variant sampled under two different address modes fills two
textures.

The binding mechanism is `Designed`: how the *original renderer* bound a
composed texture is unmeasured, because no original run exists.

## What reaches the GPU now

`cs_app::livery::LiveryRuntime` (F09-C) → `InstanceVisuals` bind →
`BatchKey::paint()` carries the `LiveryVariantKey` → `sync_frame` resolves
the variant through `render::paint::PaintSource` → `render::paint::upload_paint`
expands the composed RGB8 to opaque RGBA8 (`Rgba8UnormSrgb`, declared
addressing, linear filters, one mip) → bound as the batch's
`base_color_texture` → every `BatchInstancePlacement` samples it through the
shared `MeshMaterial3d`.

## The boundaries the decision does not cross

- **A surface that samples no image is not painted.** The paint is a
  texture; a material with no texture slot has no texels to paint.
- **A committed paint nobody composed is `SyncError::PaintNotComposed`**,
  refusing the frame before a single entity is written — never a fallback
  to the unpainted image, which would draw the aircraft in a paint it never
  chose. The caller composes and retries (tested).
- **Painted masked surfaces sample opaque alpha.** The composed RGB8 has no
  coverage plane; where coverage for a painted surface comes from is
  `Unknown`, recorded here rather than patched over with another plane.
- **The color space is `Inferred`.** The composed bytes are authored 8-bit
  RGB read as sRGB — the same single-correction rule as `bevy_image`. The
  original renderer's color handling is unmeasured.
- **Which sampled surfaces of a painted instance are its livery surface** is
  not established; the consumer binds the committed variant on every
  sampled surface of the instance. The per-part member names are the lead;
  resolving task F09-PREFIX (#386) plus the later wiring.

## Recorded unknowns (affected content / resolving task)

| Unknown | Affected content | Resolving task |
| --- | --- | --- |
| What the original renderer did with the composed image (per-variant texture, atlas region, array layer) | every painted aircraft | F17-D: GPU consumer + owner-run capture |
| Coverage for a painted masked surface | any painted masked surface | F17-D |
| Part-name → scene-node livery association | every painted multi-part aircraft | F09-PREFIX (#386) + wiring |
| Original color handling of painted textures | every painted surface | F17-D |
| Original faction palette | every faction's base/mask colors | F09-PALETTE (#385) |
| On-screen comparison vs the original | any visual-fidelity claim | F17-D + owner capture |

## Production changes

- `crates/cs_app/src/render/paint.rs` (new): `PaintSource` (implemented for
  `LiveryRuntime` and `LiveryVariantStore`), `PaintUpload`, `upload_paint`.
- `crates/cs_app/src/render/batch.rs`: `InstanceVisual` and `BatchKey` carry
  the `LiveryVariantKey` (`variant()` / `paint()` accessors; `livery()`
  still answers the digest); the frame fingerprint and `batch_key` digest
  are byte-identical to before.
- `crates/cs_app/src/render/sync.rs`: `sync_frame` takes a
  `paints: &dyn PaintSource`; resolves each textured painted batch's variant
  before writing anything; binds it deduplicated per variant;
  `SyncError::PaintNotComposed`; `FrameSync::painted`.
- `crates/cs_app/src/render/bevy_image.rs`: `address_mode` is now
  `pub(crate)` so the paint adapter applies the same declared addressing.
- `crates/cs_app/src/render/mod.rs`: module doc + `pub mod paint` (wiring).

## Tests

`crates/cs_app/tests/render/paint.rs`, prefix `accept_f17_c_paint_` (the
evidence harness is deliberately *not* under the prefix):

- `..._two_paints_batched_sample_different_texels`: three aircraft, one
  geometry, one canonical image, two paints — bound texels equal the
  composed variants byte-for-byte, differ between paints, differ from the
  canonical image; the two batches of one variant share one handle; every
  placement samples the batch's paint material; untextured painted parts
  bind nothing.
- `..._a_resynced_frame_binds_no_new_textures`: reuse keeps handles; the
  store holds one texture per variant.
- `..._one_variant_under_two_addressings_is_two_textures`: a variant sampled
  under clamp and repeat binds two textures, each with its batch's
  addressing (review fix — see below).
- `..._an_unbound_instance_samples_the_canonical_image`: the gap is still
  reported and the surface's own image is bound byte-for-byte.
- `..._an_uncomposed_variant_is_refused_and_the_retry_binds_it`:
  `paint_not_composed`, nothing spawned, retry binds.
- `..._upload_is_the_composed_rgb_widened_to_opaque_rgba`: texels, opaque
  alpha, sRGB format, declared addressing, fingerprint covers variant and
  sampler.
- `..._retail_library_composes_paint_dependent_texels` (`#[ignore]`,
  `CS_GAME_DIR`): the corpus measurement above, pinned at 159/184 with the
  all-zero-masks proof for the rest.
- `evidence::evidence_report_f17_c_paint_writes_the_acceptance_report`
  (`#[ignore]`): writes `private/evidence/F17-C-PAINT/acceptance.json`;
  validated with `tools/validate_evidence.py --require-pass`; committed as
  `docs/findings/evidence/F17-C-PAINT.json`.

### Review findings and fixes

Same-agent review (the implementer `devin-1` held the review claim; **not
independent evidence** per the owner directive) found and fixed:

1. **Paint dedupe keyed on the variant digest only.** Two batches sharing a
   variant but declaring different addressing would have bound the first
   batch's sampler for both. The dedupe key is now the upload fingerprint,
   which covers variant, extent, addressing and texels. Regression test:
   `accept_f17_c_paint_one_variant_under_two_addressings_is_two_textures`.
2. A duplicated `# Errors` doc section on `sync_frame`.

### Mutation probes (run, then reverted)

| Change | Result |
| --- | --- |
| `sync.rs`: never resolve the batch's paint (`if false`) | 5 of 6 synthetic paint tests fail — bound texels equal the shared canonical image, `painted` counts 0, the refusal never fires; the upload-shape test alone passes because it exercises `upload_paint`, not the binding |
| (inherited F17-C probe) batch key drops the paint | the different-paint aircraft merges into the shared batch — `own.len()` and key assertions fail |

## Commands

- `cargo fmt --all -- --check` — clean.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — clean.
- `cargo test --workspace --locked` — pass.
- `cargo test --workspace --locked -- accept_f17_c_paint_ --include-ignored` — 6 tests, all pass.
- `python3 tools/validate_evidence.py private/evidence/F17-C-PAINT/acceptance.json --artifact-root private/evidence/F17-C-PAINT --require-pass` — valid.

## Sources

- `GOSDATA/ASSETS/crimson.rof` in the owner's installation (the 184-member
  corpus, read through `cs_assets::install` + `mount_rof_into`).
- `docs/research/FORMAT-NOTES.md` "BM observed subset [S09, S10]" and
  `docs/research/SOURCES.md` S09 (`extract_bm.py` blob
  `ec196de05f532cc3c286ccf8fead363bf76e5c63`) / S10 (`set_paintjob.py` blob
  `ccf7c4ea065c17a44d354e8d703561cdc94dd518`).
- `docs/findings/2026-09-28-f08-c-texture-catalog-and-upload-boundary.md`
  (the upload boundary and per-variant identity this stage binds through).
- `docs/findings/2026-09-29-f09-c-model-instances-and-construction-preview.md`
  and `...-f09-d-stock-liveries-and-combinations.md` (the `ComposedLivery`
  producer and the texel-exact tool agreement).
- `docs/findings/2026-09-30-f17-c-profiles-and-instance-batching.md` (the
  batch key and the open gap this task closes).

Synthetic fixture note, in the task's own words: the synthetic tests prove
production behavior — that the bound texels are the composed variant's — and
**cannot and do not** certify original renderer behavior; the binding
mechanism claim is `Designed`, and only an original-run capture (F17-D plus
the owner) can raise it.
