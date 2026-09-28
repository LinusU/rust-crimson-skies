# F08-B.02: the ZBD texture package variant

Date: 2026-09-28. Task: F08-B.02 "Read the Crimson Skies ZBD texture package
variant into image descriptors", a slice of F08-B
(`specs/F08-texture-archives-and-conventional-image-decoding.md`, section
`### F08-B`, AC01/AC02, non-negotiables #1–#4). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Test prefix: `accept_f08_b_02_`.
Capabilities used: ordinary build/test for the committed tests; one private,
uncommitted read-only probe of `$CS_GAME_DIR` (below). No evidence report:
the whole-corpus audit is F08-D.

## Sources

- **Pinned reference source**: mech3ax v0.6.0, commit
  `d3521a9721be731d365504568ddcd78e3f9846bb` (`docs/research/SOURCES.md`
  S02), cloned into `private/` and read only; no code copied (EUPL-1.2).
  Files: `crates/mech3ax-image/src/textures.rs` (header, entry, info, flag
  bits, read order, assertions), `crates/mech3ax-pixel-ops/src/pixel_ops/mod.rs`
  (`simple_alpha`), `crates/mech3ax-api-types/src/image.rs`
  (`TextureStretch`), `crates/mech3ax-common/src/string/mod.rs`
  (`str_from_c_padded`: terminator required, zero padding, ASCII).
- **Retail survey** recorded on the task (49 archives, 37,004 textures,
  zero layout mismatches).

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/texture/zbd.rs` (new): layout constants, flag
  bits, `ZBD_TEXTURE_LAYOUT_BASIS`, `ZBD_TEXTURE_ROW_ORDER`, `ZbdAlpha`,
  `ZbdStretch`, `ZbdTexture`, `ZbdTexturePackage`, `ZbdTextureError`,
  `ZbdTextureEntryError`, entrypoint `read_zbd_textures`.
- `crates/cs_formats/src/texture/descriptor.rs`: `PixelFormat::Rgb565`,
  `Palette` (`Rgb8` / `Rgb565`, replacing the plain `Vec<PaletteEntry>`),
  `AlphaSource::Plane`, `AlphaSource::StoredValueKey`, two new
  `DescriptorError`s, `level_bytes` / `stored_bytes_per_texel`.
- `crates/cs_formats/src/texture/decode.rs`: `DecodedFormat::Rgb565`,
  `DecodedImage::{texel565, alpha, alpha_at}`, 565 palette lookup, alpha
  plane split and flip, crate-private `check_level` (index validation
  without allocation, same errors as decoding).
- `crates/cs_formats/src/texture/mod.rs`, `crates/cs_formats/src/lib.rs`:
  wiring only (module, re-exports, doc sentences).
- `crates/cs_formats/tests/texture/zbd_package.rs` (new, declared in
  `tests/texture/main.rs`); `tests/texture/main.rs` and `mips.rs` wrap their
  palettes in `Palette::Rgb8` (assertions unchanged).

**One observable failure:** a reader that decodes a palette texture without
checking its indices accepts index 4 in a 4-entry palette, and
`accept_f08_b_02_palette_index_equal_to_palette_count_is_rejected_with_texel_coordinates`
fails; one that ignores the entry start offsets fails
`accept_f08_b_02_wrong_start_offset_is_rejected`.

## Layout (claim class *observed tool*)

As on the task and in the module doc of `zbd.rs`. Checks the reader
enforces, all taken from the pinned source's assertions: header words
`0, 1, >= 0, > 0, 0, 0`; entry palette index in `-1..global_palette_count`;
name terminated inside 32 bytes, zero padding after the terminator, ASCII;
each texture starts exactly at its declared offset; info word 8 is zero;
flags within bits 0–7; bit 0 set; alpha bits exactly `NO_ALPHA`,
`HAS_ALPHA` or `HAS_ALPHA | FULL_ALPHA`; stretch in {0,1,2,3,4,7,8}; the file
ends after the last texture. On top of the source: every palette index is
checked against the local palette while reading, and dimensions/palette size
go through `ImageDescriptor::new` (4096 texel and 256 entry limits).

## Decisions

- **Texel layout from the palette count, not from bit 0.** The source calls
  bit 0 "two bytes per pixel" and requires it, but retail palette textures
  (`0xa5`, `0xab`, `0xa3`) set it too while storing one-byte indices. The
  palette count decides; a clear bit 0 is refused as unsupported
  (`bytes_per_pixel_flag_clear`), what it means stays unknown.
- **RGB565 stays raw.** `PixelFormat::Rgb565`, `Palette::Rgb565` and
  `DecodedFormat::Rgb565` keep the stored little-endian word. The source's
  565→888 expansion table is a presentation choice (non-negotiable #3) and
  is not part of decoding.
- **Simple alpha is metadata.** For a direct-color texture it is described
  as `AlphaSource::StoredValueKey { value: 0x0000 }` (the source's
  `simple_alpha`); the texel keeps `0x0000`, no coverage plane is produced.
  The source skips simple alpha for palette textures ("how would you know
  which pixel was transparent?"), so there it is `AlphaSource::Unknown`.
  `ZbdTexture::alpha()` keeps the flag reading and `alpha_basis()` is
  `ObservedTool`.
- **Full alpha is a separate plane** (`AlphaSource::Plane`): exactly
  `width * height` bytes after the texels/indices, part of the stored level,
  decoded into `DecodedImage::alpha()` with values unchanged.
- **`NO_ALPHA` → `AlphaSource::Opaque`**, on the same observed-tool basis.
- **Row order top-down** (`ZBD_TEXTURE_ROW_ORDER`): the source hands the
  stored texels unflipped to a top-row-first image buffer. Observed tool,
  not matched against the original renderer (F08-D).
- **Alpha test and color space `Unknown`** for every package texture.
- **Global palettes refused, not guessed.** The 512-byte global palettes are
  skipped structurally; any texture with flag bit 4 or an entry palette
  index other than −1 fails as `global_palette_unsupported`. No retail
  archive declares a global palette, so the application rule (the source
  slices the palette by the texture's palette count) is unverifiable.
- **Duplicates are kept.** Textures are identified by `(entry_index, name)`;
  `ZbdTexturePackage::named` returns all same-name entries in table order.
  The source's `-1`, `-2` renaming is an extractor convention and is not
  reproduced. Decode errors carry the label `<container>#<index>:<name>`.
- **Stretch kept raw.** `ZbdStretch` uses the source's names for 0–3; 4, 7
  and 8 are `Unexplained` (the source lists them as "Crimson Skies only"
  without a meaning); other values are refused like the source does. What
  any value does in the renderer is unknown.
- **Runtime bits 5–7** are kept in `flags()` / `runtime_flags()`, not
  interpreted.
- **Bounded allocation.** The entry table must be fully present before its
  vector is reserved on the `AllocationBudget`; local palettes are charged
  before they are collected; texels stay borrowed until
  `ZbdTexture::decode`, which charges the decoded buffers.

## Tests (`crates/cs_formats/tests/texture/zbd_package.rs`)

| Test | Covers |
| --- | --- |
| `asymmetric_non_square_565_textures_decode_every_texel_in_place` | AC01 on 3x2 and 2x3 565 textures, raw little-endian words, descriptor facts |
| `local_palette_order_differs_from_index_order` | 565 palette lookup, indices kept, runtime flags `0xa5` |
| `full_alpha_plane_keeps_0_and_255_at_the_edges` | AC02 alpha edge, direct and palette, plane length |
| `simple_alpha_keeps_black_texels_black_and_flags_the_key` | non-negotiable #1, stored-value key vs. palette `Unknown`, basis `observed_tool` |
| `palette_index_equal_to_palette_count_is_rejected_with_texel_coordinates` | AC02 index range, offset and canonical texel; last valid index accepted |
| `wrong_start_offset_is_rejected` | declared vs. expected offset |
| `truncated_alpha_plane_is_rejected` | `unexpected_eof` on `texture.alpha_plane` |
| `trailing_bytes_are_rejected` | one byte after the last texture |
| `unknown_or_inconsistent_flags_are_rejected` | bits above 7, bit 0 clear, four bad alpha combinations; runtime bits accepted |
| `global_palette_texture_is_explicitly_unsupported` | flag + index, flag only, index only; index beyond the global palettes |
| `duplicate_names_are_kept_with_their_entry_index` | non-negotiable #4 |
| `header_and_info_fields_are_checked` | six header words, truncated header, info word 8, zero dimension |
| `stretch_is_kept_raw_and_unlisted_values_are_refused` | 0–3 named, 4/7/8 unexplained, 5/6/9/65535 refused |
| `name_field_must_be_terminated_ascii_with_zero_padding` | padding, non-ASCII, missing terminator |
| `decoding_is_charged_to_the_budget` | 565 texels + plane charged exactly |
| `descriptor_rejects_misplaced_565_alpha_sources` | new descriptor errors, level byte counts |

Mutation probes (each applied, the `texture` target run, then restored):

| Mutation | Result |
| --- | --- |
| reader skips the palette index check | 1 `accept_f08_b_02_` test fails |
| start offset not checked | 1 fails |
| unknown flag bits accepted | 1 fails |
| trailing bytes accepted | 1 fails |
| simple alpha on 565 described as `Unknown` | 1 fails |
| entry palette index ignored when bit 4 is clear | 1 fails |
| 565 palette entry written big-endian | 3 fail |
| alpha plane replaced by opaque 255 | 1 fails |

## Private retail probe (tool observation, not an evidence report)

A throwaway test (not committed) ran `read_zbd_textures` and
`ZbdTexture::decode` over every `texture*`, `rtexture*` and `rimage.zbd`
under `$CS_GAME_DIR`: 49 archives, 37,004 textures, all read and decoded
without error. This matches the task's survey; the fingerprinted audit
against the reference decode is F08-D.

## Recorded unknowns

- Meaning of flag bit 0 (set on every retail texture, including one-byte
  palette textures).
- Meaning of stretch values (all of them; 4, 7 and 8 have no name at all).
- Whether "simple alpha" on palette textures has any transparent index, and
  whether the `0x0000` key for direct color matches the original renderer.
- How a global palette is applied (never observed in retail).
- Row order and color space against the original renderer.
- Whether bits 5–7 in the files have any effect when loaded.
