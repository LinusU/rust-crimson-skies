# F08-B.04: conventional type 2 and RLE type 10 TGAs, explicit alpha bits

Date: 2026-09-28. Task: F08-B.04 "Read the conventional TGA variants (type 2
and RLE type 10, 32bpp) with explicit alpha-bit handling", a slice of F08-B
(`specs/F08-texture-archives-and-conventional-image-decoding.md`, section
`### F08-B`, AC02 "alpha edge", non-negotiables #1–#3 and #5). Shared
contract: `docs/contracts/IDENTITY-CONTENT.md`. Test prefix:
`accept_f08_b_04_`. Capabilities used: ordinary build/test for the committed
tests, plus one private, uncommitted read-only probe of `$CS_GAME_DIR`
(below). No evidence report: the whole-corpus audit is F08-D.

## Sources

- **Published format**: Truevision TGA File Format Specification 2.0.
  18-byte header (id length, color map type, image type, color map
  specification, x/y origin, width, height, pixel depth, image descriptor
  with alpha bits 3-0, right-to-left bit 4, top-to-bottom bit 5, bits 7-6
  interleave/reserved); image id; texels as blue, green, red (, alpha); RLE
  packets whose header bit 7 selects a run (one texel repeated) or raw
  packet, count `(header & 0x7F) + 1`; a 495-byte extension area (color
  correction, postage stamp and scan line table offsets at 482/486/490,
  attributes type at 494); a 26-byte footer (extension offset, developer
  offset, `"TRUEVISION-XFILE."` NUL). No code was copied from any decoder.
- **Retail survey** recorded on the task: `GOSDATA/ASSETS/GRAPHICS/font.tga`
  and `GOSDATA/ASSETS/GRAPHICS/arial8.tga`, the only two `*.tga` files in
  the installation.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/texture/tga.rs` (new): constants, `TgaImage`,
  `TgaFooter`, `TgaExtension`, `TgaRleStats`, `TgaError`, entrypoint
  `read_tga`.
- `crates/cs_formats/src/texture/mod.rs`, `crates/cs_formats/src/lib.rs`:
  wiring only (module, re-exports, doc sentences).
- `crates/cs_formats/tests/texture/tga.rs` (new, declared in
  `tests/texture/main.rs`).

**One observable failure:** a reader that keeps the stored blue, green,
red order decodes texel (0, 0) of the 3x2 fixture as `00 00 FF 00` instead
of red `FF 00 00 00`, and
`accept_f08_b_04_uncompressed_3x2_decodes_every_texel_as_rgba_in_both_origins`
fails.

## Decisions

- **One variant, everything else refused.** No color map (type 0), image
  type 2 or 10, 24 or 32 bpp, columns left to right, bits 7-6 clear. Color
  mapped / grayscale / other RLE types, other depths, right-to-left columns
  and interleaving are `unsupported_variant` naming the field. The color map
  specification words must be zero when there is no map (`header_field`).
- **Alpha bits decide the alpha source, nothing else does.**
  24 bpp + 0 alpha bits → `PixelFormat::Rgb8`, `AlphaSource::Opaque`;
  32 bpp + 8 alpha bits → `Rgba8`, `AlphaSource::Channel`;
  32 bpp + 0 alpha bits → `Rgba8`, `AlphaSource::Unknown` (the fourth byte
  is reported as stored and no transparency decision is made);
  24 bpp with alpha bits is a `header_field` contradiction; 32 bpp with
  another count is `unsupported_variant`. The extension area's attributes
  type is kept raw (`TgaExtension::attributes_type`) and does not change the
  alpha source. Alpha test and color space stay `Unknown`.
- **Texels reordered, values unchanged.** Every texel is rewritten from
  blue, green, red (, alpha) to red, green, blue (, alpha) into one owned
  buffer in stored row order, so the existing `PixelFormat::Rgb8`/`Rgba8`
  describe it; no new stored format was added. Bit 5 set is
  `RowOrder::TopDown`, clear is `BottomUp`; the flip stays in
  `decode_base_level`. The buffer is charged to the `AllocationBudget`
  (`tga.texels`) before allocation, as is the image id (`tga.image_id`).
- **RLE is bounded and exact.** Packets are read until exactly
  `width * height` texels exist. A packet declaring more texels than remain
  is `rle_run_past_image` with the packet offset, its count and the
  remainder, raised before its texels are read. A missing header, run texel
  or raw texel is `unexpected_eof` on `tga.rle.packet_header`,
  `tga.rle.run_texel` or `tga.rle.raw_texels`. Packets may span rows (TGA
  1.0 writers produce that; TGA 2.0 discourages it); the count of such
  packets is reported in `TgaRleStats`.
- **Every trailing byte is accounted for.** After the pixels the file must
  end, or end in a TGA 2.0 footer recognised by its signature. The footer's
  developer offset must be 0 (`unsupported_variant`). Its extension offset is
  0 (footer directly after the pixels) or exactly the end of the pixels,
  where a 495-byte extension area must end exactly at the footer, with its
  three table offsets 0. Any other byte is `trailing_bytes` with its offset
  and length; a wrong extension offset is `header_field`.
- **Image id kept raw**, bounded by the input; x/y origin words kept raw.

## Tests (`crates/cs_formats/tests/texture/tga.rs`)

| Test | Covers |
| --- | --- |
| `uncompressed_3x2_decodes_every_texel_as_rgba_in_both_origins` | type 2, bottom-left and top-left, alpha 0/255 on corner texels, descriptor facts |
| `rle_3x2_decodes_every_texel_as_rgba_in_both_origins` | type 10, a run of one and a row-crossing raw packet, equal to type 2 |
| `bgr_order_is_reordered_per_texel_and_values_are_kept` | every channel byte distinct |
| `alpha_bits_8_and_0_differ_only_in_alpha_metadata` | identical texel bytes, `Channel` vs `Unknown`, both types |
| `24bpp_is_opaque_rgb_in_both_types` | 24 bpp, `Opaque` |
| `non_square_2x3_rle_runs_may_span_rows` | the transposed shape, a run across rows |
| `rle_packets_running_past_the_image_are_rejected` | run and raw packets one/several past the end, 128-texel packet |
| `truncated_rle_packets_are_rejected` | raw texels, run texel (none and half), packet header; type 2 one byte short |
| `trailing_bytes_are_an_error_unless_they_are_a_tga2_footer` | footer, footer + extension, garbage, gaps |
| `footer_and_extension_fields_are_checked` | extension offset, developer offset, extension size, table offsets, extension cut short |
| `other_tga_variants_are_explicitly_unsupported` | color map, types 0/1/3/9/11, 8/16 bpp, right-to-left, interleave, 4 alpha bits |
| `header_fields_image_id_and_dimensions_are_checked` | color map words, 24 bpp alpha bits, image id kept and truncated, zero and oversized dimensions, short header |
| `image_id_and_texels_are_charged_to_the_budget` | exact charge, one byte short refused |

Mutation probes (each applied, the `texture` target run, then restored):

| Mutation | Result |
| --- | --- |
| blue and red not swapped | 8 `accept_f08_b_04_` tests fail |
| top-to-bottom bit ignored | 4 fail |
| 32 bpp with 0 alpha bits described as `Channel` | 1 fails |
| run-past-image check removed | 1 fails |
| plain trailing bytes accepted | 1 fails |
| RLE count without the `+ 1` | 10 fail |

## Private retail probe (tool observation, not an evidence report)

A throwaway test (not committed) ran `read_tga` and `TgaImage::decode` over
both files. Both read and decoded without error:

- `font.tga`: type 2, 128x128, 32 bpp, 8 alpha bits → `Channel`,
  bottom-up, origin (0, 0), no image id, footer at 65554 with extension
  offset 0 and developer offset 0. Stored alpha bytes take exactly the
  values 0 and 255.
- `arial8.tga`: type 10, 256x256, 32 bpp, 0 alpha bits → `Unknown`,
  bottom-up, origin (0, 0), no image id, 4733 packets, none spanning rows;
  a 495-byte extension area directly after the pixels (offset 45115, all
  three table offsets 0, attributes type 0, software field naming a paint
  program) and a footer at 45610 pointing at it. Stored fourth bytes take
  exactly the values 0 and 255.

## Recorded unknowns

- **Whether the fourth byte of `arial8.tga` is coverage.** The header
  declares 0 alpha bits and the extension area's attributes type is 0 ("no
  alpha data" in the published format), yet the fourth bytes are 0 and 255,
  which looks like a coverage mask. Which of the header, the attributes type
  or the bytes the game follows is not established; the reader reports
  `AlphaSource::Unknown`. Settling it needs runtime or disassembly evidence
  of how the game uses this font image.
- Whether the game applies an alpha test (and at which threshold) or a
  color-space conversion to either image.
- What the images are used for (the names suggest UI/debug fonts; not
  established) and whether any TGA exists inside archives under another
  extension; the survey covered loose `*.tga` files only.
