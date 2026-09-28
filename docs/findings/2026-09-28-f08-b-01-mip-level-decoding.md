# F08-B.01: decoding every declared mip level

Date: 2026-09-28. Task: F08-B.01 "Decode every declared mip level of a
described image, including non-square chains" (split from F08-B,
`specs/F08-texture-archives-and-conventional-image-decoding.md`, section
`### F08-B`, AC02 and non-negotiables #2/#3). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary
build/test only (no `CS_GAME_DIR` read, no evidence report required).

## Status of this path

This is a **new-engine** path. The read-only retail survey recorded in the
F08-B split notes found that no Crimson Skies ZBD texture package stores
mip levels, so no original texture is known to reach `decode_levels`
today. It exists so that any variant that *does* carry stored levels (a
conventional reader, or a CS variant found later) has one validated path
instead of decoding the base level and discarding or trusting the rest.
Nothing here claims a CS texture has mips, how they would be laid out or
what their reduction rule would be.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/texture/decode.rs`: new entrypoint
  `decode_levels(container, &ImageDescriptor, levels: &[&[u8]], budget)`
  returning `DecodedLevels` (`base`, `mips`, `level(n)`, `levels`).
  `decode_base_level`'s body moved into a private `decode_level` that takes
  the level's extent and checked-read field name; `decode_base_level` and
  `decode_levels` both call it, so the base level and every mip share one
  length check, one palette check and one row-order flip. New
  `TextureError` variants `LevelCountMismatch` (`level_count_mismatch`)
  and `InMipLevel { level, error }` (code delegated to the inner error),
  plus `TextureError::mip_level()`.
- `crates/cs_formats/src/texture/mod.rs`: module doc and re-exports.
- `crates/cs_formats/src/lib.rs` (wiring only): `DecodedLevels` and
  `decode_levels` added to the existing `pub use texture::{...}`.
- `crates/cs_formats/tests/texture/mips.rs` (new) and one `mod mips;` line
  in `crates/cs_formats/tests/texture/main.rs`: six `accept_f08_b_01_*`
  tests.
- This file.

**One observable failure:** a decoder that decodes only the base level (or
decodes mip bytes without checking them) returns no texel at mip level 2
of the 3x2 → 2x1 → 1x1 chain, or accepts a 1x1 level padded to 2 texels,
and `accept_f08_b_01_non_square_3x2_chain_decodes_every_level_at_named_texels`
/ `accept_f08_b_01_missing_extra_short_or_padded_level_is_rejected` fail.

## Design decisions

- **Per-level slices, not a layout.** The caller supplies each level's
  bytes separately; the extent of level `n` is `descriptor.mips()[n - 1]`.
  The decoder does not concatenate, offset, align or derive extents
  (no halving/rounding rule is assumed). How a variant stores its chain is
  the variant reader's job and needs its own evidence.
- **All or nothing.** `levels.len()` must equal `mips().len() + 1`. A
  missing or extra level is `level_count_mismatch`, checked before anything
  is decoded or charged to the budget. A failure in any level rejects the
  whole chain; no partial chain is returned.
- **Same rules at every level.** Exact length (short =
  `unexpected_eof` with field `texture.mip_level`, long =
  `trailing_bytes`), every palette index checked, one row-order flip,
  budget reserved per level before that level's buffers are allocated.
  Levels are never resized: bytes of the wrong size for a level are
  rejected even when they would fit another level.
- **Errors name the level.** A mip error is `InMipLevel { level, error }`
  with the numbering `DescriptorError` already uses (1 is the first level
  below the base). Offsets and texel coordinates inside it are relative to
  that level's stored bytes and extent; the texel is canonical (from the
  top-left). A base-level error is exactly what `decode_base_level` returns,
  so F08-A behaviour and its tests are unchanged.
- **Metadata is per image, not per level.** Every decoded level carries
  the descriptor's alpha source, alpha test and color space unchanged
  (non-negotiable #1/#3).

## Test inventory (`accept_f08_b_01_*`)

All in `crates/cs_formats/tests/texture/mips.rs`; every one calls
`decode_levels` (the first also compares against `decode_base_level`).
Fixtures are authored synthetic bytes in the test file.

| Test | Covers |
| --- | --- |
| `non_square_3x2_chain_decodes_every_level_at_named_texels` | RGB 3x2 → 2x1 → 1x1: every texel of every level at its named coordinate, out-of-level coordinates `None`, base equals `decode_base_level` |
| `non_square_4x1_indexed_chain_decodes_every_level` | indexed 4x1 → 2x1 → 1x1: palette lookup and index plane per level, metadata passed through every level, keyed entry keeps its color |
| `bottom_up_storage_flips_every_level_exactly_once` | 3x3 → 3x2 → 1x2 → 1x1: bottom-up equals top-down at every level; top-down bytes declared bottom-up mirror every multi-row level; bottom-up chain A |
| `palette_index_out_of_range_names_mip_level_and_canonical_texel` | bad index in mip level 2 (1x2, bottom-up) reported as level 2, offset 0, texel (0, 1); last level checked; bad base index is not a mip error |
| `missing_extra_short_or_padded_level_is_rejected` | missing, extra, short, padded, swapped and empty levels; base-only descriptor takes exactly one level |
| `budget_is_charged_for_every_level` | exact budget for the whole chain accepted and fully used; one byte less fails in mip level 3; base-only budget fails in mip level 1; a wrong level count charges nothing |

## Mutation probes

Each mutation was applied to `crates/cs_formats/src/texture/decode.rs`,
`cargo test -p cs_formats --test texture -- accept_f08_b_01_` run, and the
file restored:

| Mutation | Failing `accept_f08_b_01_` tests (of 6) |
| --- | --- |
| level count check removed | 2 |
| extra levels accepted (only too few rejected) | 1 |
| mip levels skipped (base-only decoding) | 6 |
| trailing bytes accepted | 1 |
| mip levels decoded at the base extent | 6 |
| bottom-up rows not flipped | 2 |
| out-of-range palette index clamped to the last entry | 1 |
| mip errors not wrapped with their level | 3 |
| mip levels charged to a throwaway budget | 1 |

## Recorded unknowns

- **Mip storage and reduction rule of any CS variant.** Unknown; the
  survey found none stored. If a variant with stored levels is found, its
  reader must establish the layout and each level's extent with evidence
  before calling `decode_levels`.
- **Whether the runtime generates mips.** Out of scope for raw decoding
  (non-negotiable #3: no enhancement in the decoder); a presentation-side
  decision for F08-C or later.
