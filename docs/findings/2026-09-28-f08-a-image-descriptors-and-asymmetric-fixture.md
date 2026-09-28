# F08-A: image descriptors and asymmetric fixture

Date: 2026-09-28. Task: F08-A "Define image descriptors and asymmetric
fixtures" (`specs/F08-texture-archives-and-conventional-image-decoding.md`,
section `### F08-A`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no
evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/texture/descriptor.rs` (new): `MAX_DIMENSION`,
  `MAX_PALETTE_ENTRIES`, `MAX_MIP_LEVELS`, `Extent`, `PixelFormat`
  (`Rgb8`, `Rgba8`, `Indexed8`), `RowOrder`, `PaletteEntry`,
  `AlphaSource`, `AlphaTest`, `ColorSpace`, `DescriptorParts`,
  `ImageDescriptor` (`new` validates; accessors; `base_level_bytes`),
  `DescriptorError` (`code`, `Display`, `Error`).
- `crates/cs_formats/src/texture/decode.rs` (new): `DecodedFormat`,
  `DecodedImage` (`texel(x, y)`, `index(x, y)`, `texels`, `indices`,
  pass-through metadata), `TextureError` (`Parse`, `TrailingBytes`,
  `PaletteIndexOutOfRange`; `code`, `container`) and the entrypoint
  `decode_base_level`.
- `crates/cs_formats/src/texture/mod.rs` (new): module doc and re-exports.
- `crates/cs_formats/src/lib.rs` (wiring only): `pub mod texture;`, a
  `pub use texture::{...}` line and one module-doc sentence.
- `crates/cs_formats/tests/texture/main.rs` (new; integration-test target
  `texture`): the authored 3x2 fixture and nine `accept_f08_a_*` tests.
- This file.

**Not created in this stage:** `crates/cs_content/src/textures.rs` (the
catalog/identity wiring is F08-C: "Connect images to the content catalog")
and `tools/cs_inspect/src/textures.rs` (nothing to inspect until F08-B reads
an actual archive variant). Same reasoning F06-A and F07-A recorded.

**One observable failure:** a decoder that ignores the stored row order (or
flips twice) returns yellow instead of red at texel (0, 0) of the bottom-up
3x2 fixture, and `accept_f08_a_bottom_up_rows_are_flipped_exactly_once`
fails; one that computes the row stride from the height instead of the
width fails `accept_f08_a_row_and_column_counts_are_not_interchangeable`.

## Design decisions

- **Descriptor first, variants later.** The research boundary forbids
  assuming DXTC, 565, indexed color or any other encoding for a CS archive
  variant. `PixelFormat` therefore holds only conventional self-describing
  layouts (8-bit RGB, RGBA, palette index) that a variant reader or a
  TGA/BMP reader maps *established* bytes onto. Nothing claims a CS texture
  uses any of them. New variants (16-bit, block compression, row padding,
  right-to-left columns) are added in F08-B with their evidence.
- **Every spec fact is a field without a default.** Size (`Extent`),
  layout (`PixelFormat`), row order, channels (from the format), palette,
  mip levels, alpha interpretation and color space are all required in
  `DescriptorParts`. Unknown alpha, alpha test and color space are explicit
  `Unknown` variants.
- **Alpha is three separate facts** (non-negotiable #1): where coverage
  comes from (`AlphaSource`: opaque, stored channel, palette key, unknown),
  the alpha-test threshold (`AlphaTest`) and the palette itself. A palette
  key is metadata: the decoded texel keeps its palette color and the index
  plane is kept (`DecodedImage::indices`) so transparency is evaluated at
  presentation. A black texel is never transparent by accident (tested).
- **One canonical orientation.** `DecodedImage` is always top row first,
  columns left to right, `texel(x, y)` with `y` from the top. The single
  flip for `RowOrder::BottomUp` happens in `decode_base_level`.
- **Values are not touched** (non-negotiable #3): no color-space
  conversion, premultiplication or resizing. `ColorSpace` is carried to the
  presentation boundary.
- **Exact length, bounded allocation** (non-negotiable #2). The stored base
  level must be exactly `width * height * bytes_per_texel` bytes: short
  input is a checked-read `unexpected_eof`, extra input is
  `trailing_bytes`. Decoded buffers are booked on the F03
  `AllocationBudget` before allocation. Every palette index is checked and
  a bad one is reported with its stored offset and canonical texel.
- **Mip chains are validated for shape only.** Each declared mip must be
  non-zero, within the dimension limit and smaller than the level above
  (no larger on either axis, smaller on at least one), so non-square chains
  such as 3x2 → 2x1 → 1x1 are accepted. The exact reduction rule (halving
  with floor? rounding?) is a variant fact and is **not** assumed; mip
  bytes are not decoded in this stage (F08-B, AC02).
- **Design limits, not observed limits.** `MAX_DIMENSION = 4096` and
  `MAX_MIP_LEVELS = 12` are new-engine designed bounds against corrupted
  headers (claim class *Designed*). If a retail texture exceeds them, the
  limit is raised with that evidence; nothing is resized to fit.
  `MAX_PALETTE_ENTRIES = 256` follows from the 8-bit index.
- **Fixtures are authored in the test file**, never committed as binaries
  (`fixtures/synthetic/README.md`). The expected texels are written as a
  per-coordinate table independent of the byte builders, and the palette
  order differs from the image order, so a writer and reader cannot share
  one wrong assumption unnoticed.

## Test inventory (`accept_f08_a_*`)

All in `crates/cs_formats/tests/texture/main.rs`; every one calls
`ImageDescriptor::new` and/or `decode_base_level`.

| Test | Covers |
| --- | --- |
| `asymmetric_3x2_top_down_decodes_every_texel_in_place` | AC01: six distinct colors at their named coordinates, out-of-range coordinates are `None` |
| `bottom_up_rows_are_flipped_exactly_once` | AC01: bottom-up storage yields the same image; top-down bytes declared bottom-up are the vertical mirror |
| `row_and_column_counts_are_not_interchangeable` | AC01: the same 18 bytes as 2x3 are a different image |
| `indexed_fixture_matches_direct_color_and_keeps_indices` | palette lookup, index plane kept and reordered with the texels |
| `black_is_not_transparent_and_alpha_is_reported_as_stored` | non-negotiable #1/#3: stored alpha unchanged, palette key not baked, metadata passes through |
| `partial_or_padded_base_level_is_rejected` | one byte short, one byte too many, no resize to a larger declared size |
| `palette_index_out_of_range_names_the_canonical_texel` | AC02 preview: bad index reported with offset and top-left coordinate |
| `decoded_allocation_is_charged_to_the_budget` | exact budget accepted, one byte less rejected |
| `descriptor_rejects_inconsistent_parts` | every `DescriptorError` variant; limit itself accepted; non-square mip chain accepted |

## Mutation probes

Each mutation was applied to production code, the `texture` target run and
the file restored with `git checkout`:

| Mutation | Failing tests |
| --- | --- |
| bottom-up rows not flipped | 3 |
| stored row order ignored entirely | 3 |
| texel stride computed from height instead of width | 5 |
| trailing bytes accepted | 1 (`partial_or_padded_…`) |
| out-of-range palette index clamped to the last entry | 1 (`palette_index_out_of_range_…`) |
| decoded texels charged to a throwaway budget | 1 (`decoded_allocation_…`) |
| palette on a direct-color format accepted | 1 (`descriptor_rejects_…`) |
| mip equal to the level above accepted | 1 (`descriptor_rejects_…`) |

## Recorded unknowns

- **Every CS texture archive variant.** No header, texel layout, row order,
  palette storage, alpha convention or mip layout of `texture.zbd` /
  `rtexture*.zbd` is established in the committed research pack (F06-A
  records the texture family header as undocumented). F08-B must read the
  pinned source (S02/S06) and check the installation before mapping any
  variant onto a `PixelFormat`.
- **Color space of original textures.** Unknown; descriptors for original
  content should say `ColorSpace::Unknown` until evidence exists.
- **Mip reduction rule.** Unknown; only the shape constraint above is
  enforced.
