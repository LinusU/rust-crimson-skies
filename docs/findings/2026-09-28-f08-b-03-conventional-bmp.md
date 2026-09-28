# F08-B.03: conventional 4 and 8 bpp BMPs, read by content

Date: 2026-09-28. Task: F08-B.03 "Read the conventional BMP variants (4bpp
and 8bpp BI_RGB) by content, not extension", a slice of F08-B
(`specs/F08-texture-archives-and-conventional-image-decoding.md`, section
`### F08-B`: "Conventional TIFF/TGA/BMP or other discovered files use
audited decoders; extension alone does not decide byte format";
non-negotiables #1, #2 and #5). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Test prefix: `accept_f08_b_03_`.
Capabilities used: ordinary build/test for the committed tests; one private,
uncommitted read-only probe of `$CS_GAME_DIR` (below). No evidence report:
the whole-corpus audit is F08-D.

## Sources

- **Published format**: the Windows bitmap structures `BITMAPFILEHEADER`
  (14 bytes), `BITMAPINFOHEADER` (40 bytes) and `RGBQUAD` (blue, green, red,
  reserved), `BI_RGB` rows padded to a multiple of 4 bytes, positive height
  = bottom-up, negative height = top-down, 4 bpp high nibble = left texel.
  No code was copied from any decoder.
- **Retail survey** recorded on the task: the installation root holds
  exactly two BMPs, `00000409.016` and `00000409.256`.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/texture/bmp.rs` (new): constants,
  `looks_like_bmp`, `BmpImage`, `BmpError`, entrypoint `read_bmp`.
- `crates/cs_formats/src/texture/mod.rs`, `crates/cs_formats/src/lib.rs`:
  wiring only (module, re-exports, doc sentences).
- `crates/cs_formats/tests/texture/bmp.rs` (new, declared in
  `tests/texture/main.rs`).

**One observable failure:** a reader that takes the low nibble as the left
texel decodes the 3x2 4 bpp fixture with green at (0, 0) instead of red, and
`accept_f08_b_03_asymmetric_3x2_4bpp_odd_width_puts_the_high_nibble_on_the_left`
fails.

## Decisions

- **Detection by content.** `looks_like_bmp` checks the `BM` signature and
  `read_bmp` validates the whole header; the container label is used only
  in errors. The same bytes under `.bmp`, `.016` and `.256` names give equal
  descriptors and decoded images; a `.bmp` name without the signature is
  `not_bmp`.
- **One variant, everything else refused.** Header size 40, 1 plane, 4 or 8
  bpp, `BI_RGB`, `colors_used` 0 (= `2^bpp`) up to `2^bpp`. Other header
  sizes (12, 108, 124, ...), compression (RLE, bitfields, ...), other bit
  depths and larger color counts are `unsupported_variant` naming the field.
- **Exact sizes.** `file_size` must equal the input length; `pixel_offset`
  must point directly after the color table (a gap is refused, not
  skipped); `image_size` is 0 or exactly `stride * |height|`; the file ends
  after the last row (`trailing_bytes`), a short last row is
  `unexpected_eof` on `bmp.pixels`. Reserved header words must be 0;
  `colors_important` must not exceed the table.
- **Padding bits must be zero.** The unused low nibble of an odd 4 bpp
  width and every row padding byte are checked (`nonzero_padding`, with the
  absolute offset and canonical row). The published format says padding is
  zero; the two retail files have 640-texel rows and therefore no padding,
  so this strictness is not tested against retail data.
- **Unpacked to `Indexed8`, not a new stored format.** Rows are unpacked to
  one index byte per texel without padding, in stored row order, and
  described as `PixelFormat::Indexed8` with the stored `RowOrder`; the flip
  stays in `decode_base_level`. The unpack buffer is charged to the
  `AllocationBudget` (`bmp.indices`) after the whole file is validated
  structurally and before it is allocated; the palette is charged as
  `bmp.color_table`.
- **Every index is checked while reading**, against the color table size
  (`colors_used` or `2^bpp`), reported as `palette_index_out_of_range` with
  the absolute file offset of the byte and the canonical texel.
- **The reserved color byte is not alpha.** It is ignored whatever its
  value; the palette is `Palette::Rgb8` in red, green, blue order.
- **Alpha `Opaque`, alpha test and color space `Unknown`.** A 4/8 bpp
  `BI_RGB` BMP stores no coverage, so the image is `AlphaSource::Opaque`.
  Whether the game applies a color key or alpha test to these images, and
  in which color space, is a material/runtime fact not established here.
- **Resolution kept raw** (`pixels_per_meter`), as are `colors_used` and
  `colors_important`.

## Tests (`crates/cs_formats/tests/texture/bmp.rs`)

| Test | Covers |
| --- | --- |
| `asymmetric_3x2_8bpp_decodes_every_texel_in_place_in_both_row_orders` | bottom-up and top-down, row padding dropped, descriptor facts |
| `asymmetric_3x2_4bpp_odd_width_puts_the_high_nibble_on_the_left` | literal nibble bytes, odd width, both row orders |
| `non_square_2x3_and_even_4bpp_width_decode_in_place` | the transposed shape, even 4 bpp width |
| `color_table_is_bgrx_and_the_reserved_byte_is_not_alpha` | BGR order, reserved 0x00/0x80/0xFF ignored, no alpha plane |
| `colors_used_zero_means_the_full_table` | 16 and 256 entries |
| `index_beyond_the_color_table_is_rejected_with_coordinates` | 8 bpp and 4 bpp low nibble, offset and texel; last valid index accepted |
| `padding_bits_must_be_zero` | odd-width nibble, row padding byte in both depths |
| `truncated_or_padded_pixel_data_is_rejected` | file size, eof, trailing byte, image size |
| `the_name_does_not_decide_the_format` | `.bmp` / `.016` / `.256` names equal; missing signature |
| `other_bmp_variants_are_explicitly_unsupported` | header sizes 12/108, compression 1/3, 1/16/24/32 bpp, 17/257 colors |
| `header_fields_and_dimensions_are_checked` | reserved words, planes, pixel offset ±1, colors important, negative width, zero and oversized dimensions, `i32::MIN` height, truncated header |
| `palette_and_indices_are_charged_to_the_budget` | exact charge, one byte short refused |

Mutation probes (each applied, the `texture` target run, then restored):

| Mutation | Result |
| --- | --- |
| low nibble taken as the left texel | 4 `accept_f08_b_03_` tests fail |
| palette index check removed | 1 fails |
| row padding check removed | 1 fails |
| positive height read as top-down | 6 fail |
| file size check removed | 1 fails |

## Private retail probe (tool observation, not an evidence report)

A throwaway test (not committed) ran `read_bmp` and `BmpImage::decode` over
`$CS_GAME_DIR/00000409.016` and `00000409.256`: both read and decoded
without error as 640x480, bottom-up, `colors_used` 0 (16 and 256 entries),
`colors_important` 0, 2834 pixels per meter on both axes, `image_size`
exact, reserved color bytes 0.

## Recorded unknowns

- What the two images are used for and when the game shows them (the
  `00000409` stem suggests a language id, 0x409 = US English; not
  established).
- Whether the game applies a color key, alpha test or color-space
  conversion to them.
- Whether any other conventional image (TGA, TIFF, ...) under another
  extension exists outside the installation root; the task survey covered
  BMPs only.
