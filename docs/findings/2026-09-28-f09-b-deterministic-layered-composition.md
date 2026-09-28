# F09-B: Deterministic layered composition

Date: 2026-09-28. Task: F09-B "Implement deterministic layered composition"
(`specs/F09-bm-multilayer-liveries-and-paint-composition.md`, section
`### F09-B`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no
evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/bm.rs`: `BM_COMPOSITION_VERSION`,
  `BM_COMPOSED_BYTES_PER_PIXEL`, `PaintColor`, `BmComposite` and
  `BmFile::compose`, plus the private helpers `shift_for_div255`, `multiply`,
  `blend`, `mask_weighted_color` and `alpha_over_opaque`.
- `crates/cs_formats/src/lib.rs` (wiring only): re-export the new items and
  extend the module doc by one sentence.
- `crates/cs_content/src/livery.rs` (new): `LiveryPaint`, `LiveryVariantKey`,
  `ComposedLivery`, `source_fingerprint` and `compose_livery`, the
  content-layer production path.
- `crates/cs_content/src/lib.rs` (wiring only): `pub mod livery;` and one
  module-doc paragraph.
- `crates/cs_formats/tests/bm.rs`, `crates/cs_content/src/livery.rs` (tests):
  ten `accept_f09_b_*` cases.
- This file.

`crates/cs_app/src/livery.rs` is left to F09-C: wiring paints into model
instances and the construction preview is that stage's declared behavior.

**One observable failure:** a composition that applies a mask the wrong way
round (a `255` mask building white instead of the paint color) returns the
base unchanged at the all-one endpoint, and
`accept_f09_b_all_one_masks_multiply_the_base_by_the_colors` fails.

## Sources

- S10, pinned blob `ccf7c4ea065c17a44d354e8d703561cdc94dd518`
  (`set_paintjob.py`), read with `gh api .../git/blobs/<sha>`:
  `apply_color_mask` fills `color`, pastes a white fill over it with the
  **inverted** mask, multiplies the base by the result, then
  `Image.alpha_composite(RGBA(base), specular)` and converts back to RGB.
  The three masks are applied in file order with `FACTION_COLORS[faction]`.
- Pillow integer arithmetic, read from the Pillow `main` branch
  (`src/libImaging/Chops.c`: `ImageChops.multiply` is
  `(int)a * (int)b / 255`; `src/libImaging/Paste.c` +
  `src/libImaging/ImagingUtils.h`: masked paste uses
  `DIV255(dst*(255-mask) + src*mask)` with `SHIFTFORDIV255(a) =
  (((a >> 8) + a) >> 8)`; `src/libImaging/AlphaComposite.c`: the 7-bit
  fixed-point source-over loop). The pinned helper does not record its Pillow
  version, so this is the *observed tool* arithmetic, not a retail claim.
- `docs/research/FORMAT-NOTES.md`, "BM observed subset".
- F09-A: `docs/findings/2026-09-28-f09-a-bm-layout-and-rectangular-fixture.md`
  (`BmFile`, canonical orientation, stored planes).

## The composed algorithm

For canonical texel `(x, y)` with mask bytes `m1, m2, m3`, paint colors
`C1, C2, C3` and overlay `(or, og, ob, oa)`:

1. **Mask-weighted color.** Following the helper, the color plane for mask
   `m` is white where `m == 0`, `C` where `m == 255`, and
   `DIV255(C*(255-inv) + white*inv)` with `inv = 255 - m` in between. (The
   helper pastes white over the solid color with the inverted mask; the
   intermediate values are a rounded linear blend, not a binary select.)
2. **Multiply.** `rgb = floor(rgb * overlay_c / 255)` per channel, in mask
   order (the helper's `ImageChops.multiply`).
3. **Overlay alpha composite.** Source-over with the RGB base as an opaque
   destination, using Pillow's 7-bit fixed-point loop; the composed image is
   RGB8 again (the helper's final `convert("RGB")`).

Claim class: *observed tool*. Whether the original renderer uses these exact
weights, this rounding or this alpha convention is **not established** and is
F09-D work.

## Design decisions

- **Colors are input, never a catalog.** `PaintColor` values come from the
  caller. The helper's `FACTION_COLORS` are hardcoded research leads, so they
  are not reproduced as an authoritative palette (non-negotiable #4). The
  fixture tests use our own synthetic colors.
- **Versioned and deterministic.** `BM_COMPOSITION_VERSION` is part of every
  variant key; the composition depends on nothing but the file and the
  colors, and the same input always produces the same bytes (tested).
- **Bounded output.** `compose` charges `3 * width * height` bytes against an
  `AllocationBudget` before allocating and refuses a budget that cannot cover
  it, so a header-only stream still cannot force a huge allocation
  (consistent with F03-B).
- **One orientation, owned output.** `BmComposite` is RGB8, canonical
  top-down, one texel per source texel. The row flip stays in
  `BmFile::sample`; composition applies it exactly once.
- **The content key covers the whole source.** `source_fingerprint` folds the
  header dimensions, the five stored planes (base, three masks and the
  overlay/decal layer) and any unsupported tail, each through its own SHA-256,
  into one digest without copying the input. `LiveryVariantKey` = that
  fingerprint + the three colors + the algorithm version, so a changed mask or
  overlay yields a different key (non-negotiable #5). The key and values are
  produced here; the per-instance cache map is F09-C.

## Test inventory (`accept_f09_b_*`)

| Test | Covers |
| --- | --- |
| `accept_f09_b_all_zero_masks_compose_to_the_base` (cs_formats) | AC02 `0` endpoint; a transparent overlay with nonzero RGB must not leak; canonical order |
| `accept_f09_b_all_one_masks_multiply_the_base_by_the_colors` (cs_formats) | AC02 `255` endpoint; `floor(base*color/255)`; a white plane is the identity in any position |
| `accept_f09_b_mixed_mask_endpoints_apply_only_the_full_plane` (cs_formats) | AC02: zero planes contribute nothing, one full plane applies |
| `accept_f09_b_opaque_overlay_replaces_the_masked_color` (cs_formats) | overlay `255` endpoint, per texel and after the flip |
| `accept_f09_b_composition_applies_the_row_flip_once` (cs_formats) | canonical orientation of the composed image |
| `accept_f09_b_composition_is_deterministic_and_bounded` (cs_formats) | same input -> same bytes; exact budget charge; over-budget refusal allocates nothing |
| `accept_f09_b_content_all_zero_masks_compose_to_the_base` (cs_content) | endpoint through `compose_livery`; key fields |
| `accept_f09_b_content_paint_selects_and_does_not_mutate_other_variants` (cs_content) | full-mask endpoint; a second paint does not mutate the first result or key |
| `accept_f09_b_content_key_covers_masks_and_overlay` (cs_content) | a changed mask or overlay changes source/fingerprint/key |
| `accept_f09_b_content_is_deterministic_and_bounded` (cs_content) | determinism and over-budget refusal through the content path |

## Mutation probes

Each mutation was applied on this branch, the task tests run and the file
restored with `git checkout --`:

| Mutation | Failing tests |
| --- | --- |
| `mask_weighted_color` uses `mask` instead of `inverted` | 6 |
| `alpha_over_opaque` always returns the destination | 1 |
| `multiply` rounds instead of truncating | 4 |
| `compose` ignores the mask planes (returns the base) | 4 |
| `source_fingerprint` omits the mask planes | 1 |

## Recorded unknowns

- **Retail agreement.** The whole pipeline is *observed tool*: the exact
  weights, the intermediate blend and the alpha convention are unverified
  against the original renderer (F09-D).
- **Pillow version.** The pinned helper does not record its Pillow version;
  the arithmetic above matches Pillow `main` at the time of writing, and a
  different Pillow could round intermediate values differently. The endpoints
  (`0` and `255` masks, transparent and opaque overlay) are independent of
  that rounding and are what this stage tests.
- **Meaning of the planes.** Which mask selects which color, and whether the
  overlay is a decal layer rather than the "specular" the helper's file name
  suggests, is still unknown; it is kept as the overlay plane.
- **Palettes and decals.** The faction color table, valid combinations and
  decal selection come from original data (F09-D); none is guessed here.
