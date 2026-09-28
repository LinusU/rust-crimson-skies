# F08-B: texture variants and conventional readers (integration)

Date: 2026-09-28. Task: F08-B "Implement verified texture variants and
conventional readers" (`specs/F08-texture-archives-and-conventional-image-decoding.md`,
section `### F08-B`, AC02 and non-negotiables #1–#5). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Test prefix: `accept_f08_b_`.
Capabilities used: ordinary build/test for the synthetic tests; read-only
`$CS_GAME_DIR` for three `#[ignore = "requires CS_GAME_DIR"]` census tests.
No evidence report: the task needs only ordinary build/test, and the
fingerprinted pixel audit against the pinned reference is F08-D.

F08-B was split into F08-B.01 (mip levels), B.02 (ZBD texture package),
B.03 (BMP) and B.04 (TGA); their decisions and unknowns are recorded in
the four `2026-09-28-f08-b-0*` findings next to this file. This step
checks the readers together and pins the retail census.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/tests/texture/variants.rs` (new): cross-reader tests.
- `crates/cs_formats/tests/texture/retail.rs` (new): retail census tests.
- `crates/cs_formats/tests/texture/main.rs`: two `mod` lines.

No production code changed.

**One observable failure:** if `decode_levels` accepted more levels than
the descriptor declares, a reader's single stored level followed by an
invented mip would decode, and
`accept_f08_b_readers_declare_only_the_stored_level_and_decode_levels_agrees`
fails.

## Variants the installation stores

Census taken by the production readers (`read_zbd_textures`, `read_bmp`,
`read_tga`) and asserted by `tests/texture/retail.rs`. Installation as
fingerprinted on task T340 (`b4e780ab…1978`).

| Files | Variant | Reader | Descriptor |
| --- | --- | --- | --- |
| `ZBD/rimage.zbd`, `ZBD/C*/texture.zbd`, `ZBD/C*/rtexture*.zbd` (49 archives, 37,004 textures) | CS ZBD texture package, no global palettes | `zbd` | top-down, no mips |
| — 15,399 | RGB565, `NO_ALPHA` | | `Rgb565`, `Opaque` |
| — 15,343 | RGB565, full alpha plane | | `Rgb565`, `Plane` |
| — 137 | RGB565, simple alpha | | `Rgb565`, `StoredValueKey 0x0000` |
| — 3,089 | local 565 palette, `NO_ALPHA` | | `Indexed8`, `Opaque` |
| — 3,014 | local 565 palette, full alpha plane | | `Indexed8`, `Plane` |
| — 22 | local 565 palette, simple alpha | | `Indexed8`, `Unknown` |
| `00000409.016` | BMP 640x480, 4 bpp `BI_RGB`, bottom-up | `bmp` | `Indexed8`, `Opaque` |
| `00000409.256` | BMP 640x480, 8 bpp `BI_RGB`, bottom-up | `bmp` | `Indexed8`, `Opaque` |
| `GOSDATA/ASSETS/GRAPHICS/font.tga` | TGA type 2, 128x128, 32 bpp, 8 alpha bits, bottom-left | `tga` | `Rgba8`, `Channel` |
| `GOSDATA/ASSETS/GRAPHICS/arial8.tga` | TGA type 10 (RLE), 256x256, 32 bpp, 0 alpha bits, bottom-left | `tga` | `Rgba8`, `Unknown` |

ZBD stretch words: 0: 34,820; 1: 168; 2: 294; 3: 366; 4: 648; 7: 576;
8: 132. 11,406 package textures are non-square and 217 have a
non-power-of-two side. Every image of every variant reads and decodes
without error; no palette index is out of range. No file stores mip
levels, so every descriptor declares none; the non-square chain path
(`decode_levels`) is exercised by the F08-B.01 synthetic chains only.

## AC02 coverage by the `accept_f08_b_` tests

- **Palette index out of range:** ZBD local palette
  (`accept_f08_b_02_palette_index_equal_to_palette_count_…`), BMP 8 and 4 bpp
  (`accept_f08_b_03_index_beyond_the_color_table_…`), a mip level
  (`accept_f08_b_01_palette_index_out_of_range_names_mip_level_…`). Each
  names the canonical texel; the last valid index is accepted.
- **Alpha edge:** ZBD full plane with 0 and 255 at the corners, simple alpha
  keeping black texels black (B.02); TGA 32 bpp with 8 vs. 0 alpha bits
  (B.04, and `arial8.tga` in the retail census); BMP color table reserved
  byte not taken as alpha (B.03).
- **Non-square mip chains:** 3x2→2x1→1x1, 4x1→2x1→1x1 and
  3x3→3x2→1x2→1x1 (B.01); across readers, a level beyond the declared
  chain is refused (`variants.rs`).
- **Differential decoding (non-negotiable #5):** the same 3x2 image as BMP
  8 bpp bottom-up, BMP 4 bpp top-down, TGA type 2 top-down and TGA type 10
  bottom-up decodes to identical channel bytes per coordinate
  (`accept_f08_b_bmp_and_tga_of_the_same_image_decode_to_identical_texels`).

Mutation probes (each applied, the `texture` target run, then restored):

| Mutation | Result |
| --- | --- |
| `decode_levels` accepts more levels than declared | `accept_f08_b_readers_declare_only_…` and one B.01 test fail |
| TGA origin bit 5 read inverted | `accept_f08_b_bmp_and_tga_…` and four B.04 tests fail |

Without `CS_GAME_DIR` the three retail tests panic when run with
`--include-ignored`.

## Recorded unknowns

Unchanged from the stage findings; none are new. In short: meaning of ZBD
flag bit 0, all stretch values, simple alpha on palette textures and the
`0x0000` key against the original renderer, global palette application,
ZBD row order and color space against the renderer, the meaning of the
fourth byte of `arial8.tga`, and the alpha test and color space of every
variant (all `Unknown`).
