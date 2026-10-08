# Task #466: the original fonts' glyph coverage (`F51-FONTCELL`)

Date: 2026-10-06. Task: #466 (`F51-FONTCELL`), the F51-D follow-up that closes
the `unmeasured_glyphs` blocker of
`docs/findings/2026-10-01-f51-d-locale-glyph-overflow-and-license-audit.md`.
Test prefix: `accept_f51_fontcell_`. Owner paths:
`crates/cs_content/src/localization.rs`, `crates/cs_app/src/text/`,
`crates/cs_app/tests/text/`, `docs/findings/`.

**Outcome: the glyph question is answered by measurement, not by a guess.**

1. `GOSDATA/ASSETS/GRAPHICS/font.tga` and `arial8.tga` are recorded as **not
   read as fonts by the original**, with the owner-note evidence cited in the
   verdict itself. They are no longer `GlyphEvidence::Unmeasured`.
2. The ten `fonts.zrd` `FONTS` bitmap fonts of `ZBD/rimage.zbd` are measured by
   the production cell rule: **94 cells each**, coverage = space + `'!'`..`'~'`
   (95 characters), zero unresolved characters, zero unaddressed cells.
3. `gfont3d`/`print3d` text is recorded as the Windows-1252 design target of
   the OS font the original resolves to (218 assigned characters), an OS
   property and never game data.
4. The F51-D audit over the original installation now has **exactly one**
   remaining blocker, and it names itself: `overflow` — 21 `langui.dll`
   strings that do not fit the declared panel at declared development metrics
   (F51-D's own measurement). No glyph verdict is unknown any more, so
   `is_complete() == false` still holds for a measured reason.

This is static code evidence plus a read-only scan of retail files. Nothing
here is a run of the original executable, so nothing here is `verified_original`.

## Sources

### Owner static analysis (Rally #466 owner note, 2026-10-05)

| Item | Value |
| --- | --- |
| Decrypted image | `$CS_ENGINE_IMAGE` |
| SHA-256 | `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` |
| Method | Kuna decompiler v1.692 plus a disassembly check, at the owner's request |
| Addresses | virtual addresses; for VA < 0x643000 the file offset is VA − 0x400000 |

The image, the decompiled code and every glyph pixel are **not** committed.
Only addresses, ids, sizes, counts and behaviour appear in
`crates/cs_app/src/text/original_font.rs`, which documents each of them:

* `gos_LoadFont` `0x5ce9e0` passes only the *base name* to `0x530700`
  (`0x5ce9e0`–`0x5cea34` contains no file, ROF or reader call), so
  `font3d "…arial8.tga"` never opens the path; `0x530700` looks the name up in
  the `fonts.zrd` `WINDOWS_FONTS` registry (`0x7581e0`, built by `0x52fc00`) and
  `0x530360` calls `CreateFontIndirectA`. Descriptor defaults `0x530330`:
  `lfCharSet = 0` (`ANSI_CHARSET`), `lfOutPrecision = 4`, `lfQuality = 1`.
  Neither `font.tga` nor `arial8` occurs in the executable (byte search).
* The ten `fonts.zrd` `FONTS` names, loaded by `0x5345c0`, fetched by the
  texture lookup `0x531b60(name)`, scanned by `0x534750`/`0x534830`, measured
  by `0x534890`, drawn by `0x5bfad0` (wrapped draw `0x5bfb90`); colour key set
  to `0` by zImgInit `0x52fa50`.
* The mapping: cell `c - 0x21`; `' '` advances and draws nothing; `'\r'` is
  ignored; `'\n'` starts a line; out of `[0, 0x5f)` draws cell 0 (`'!'`). `char`
  is signed, so every byte `>= 0x80` renders as `'!'`.

### Retail data used

| Item | Value |
| --- | --- |
| `ZBD/rimage.zbd` | 37 970 361 B, SHA-256 `fc5f07385b72297e1de0ff05a07188cd0e09ae8609c3f38a7aedee1e2d4e1f13` |
| Archive shape | 254 textures, 0 global palettes; every font image is direct-colour RGB565, flags `0x5`, no palette |
| Reading | production `cs_formats::texture::read_zbd_textures` + `ZbdTexture::decode`, read-only through `$CS_GAME_DIR` |

Two stored spellings differ from the `fonts.zrd` request and are found only
because the request is folded first (`5PointHUD` → `5pointhud`,
`verdana_Offwhite_11` → `verdana_offwhite_11`): the fold measured by task
#689 (`cs_content::textures::folded_texture_name`) is used, never a fold of a
*stored* name.

### Measured result of the cell rule on retail

| font | size | cells | coverage chars | unresolved | stray | average advance |
| --- | --- | --- | --- | --- | --- | --- |
| `lucida_console_8` | 670×12 | 94 | 95 | 0 | 0 | 6 |
| `lucida_console_8b` | 761×12 | 94 | 95 | 0 | 0 | 7 |
| `modern_white_12` | 718×12 | 94 | 95 | 0 | 0 | 6 |
| `modern_red_12` | 718×12 | 94 | 95 | 0 | 0 | 6 |
| `verdana_red_11` | 712×12 | 94 | 95 | 0 | 0 | 6 |
| `verdana_white_11` | 712×12 | 94 | 95 | 0 | 0 | 6 |
| `verdana_green_11` | 712×12 | 94 | 95 | 0 | 0 | 6 |
| `verdana_offwhite_11` | 712×12 | 94 | 95 | 0 | 0 | 6 |
| `5pointhud` | 463×6 | 94 | 95 | 0 | 0 | 3 |
| `5pointhudbrite` | 463×6 | 94 | 95 | 0 | 0 | 3 |

`coverage chars` counts the space plus the 94 mapped cells. `'!'`..`'~'` are
covered in every font; `0x7f`, `é`, `€` and every other byte `>= 0x80` have no
cell and are **not** claimed — in the original they draw the `'!'` cell, which
every font covers, so a missing glyph is counted *and* visible (F51
non-negotiable 3).

The scan's resume point (`x1 + gap/2`) is always inside the separator run that
follows a cell, so any resume point in that run reaches the same next glyph
column: the cells are exactly the maximal runs of non-separator columns, each
extended by one separator column. The rule is therefore deterministic without
assuming what `gap` is.

## What changed

* `crates/cs_app/src/text/original_font.rs` (new): `ORIGINAL_BITMAP_FONT_NAMES`,
  `COLOR_KEY`, the cell rule (`scan_cells`/`build_font`),
  `measure_rimage_bitmap_fonts`, `gfont3d_coverage`, `UNUSED_FONT_TGA_REASON`
  and the refusal [`BitmapFontError`] (unreadable package, ambiguous name,
  image without a stored RGB565 colour key, failed decode).
* `crates/cs_app/src/text/audit.rs`: two new measured `GlyphEvidence` verdicts
  (`UnusedByOriginal`, `BitmapFonts`) beside `Declared`/`Unmeasured`, and three
  new named blockers — `bitmap_font_missing`, `bitmap_font_unmapped`,
  `bitmap_font_stray_cell` — so an unknown cell can never be claimed covered.
  `unmeasured_glyphs` is now reserved for media nobody could measure.
* `crates/cs_app/src/text/mod.rs`: wiring (module declaration, re-exports) and
  the module docs that asserted the mapping was unknown.
* `crates/cs_app/tests/text/common.rs`: the retail audit hands the audit the
  TGAs with their recorded verdict and `ZBD/rimage.zbd` with the measured
  scan.
* `crates/cs_app/tests/text/audit.rs`: the F51-D retail test now pins the new
  verdicts and asserts the blocker list is exactly `["overflow"]`.
* `crates/cs_app/tests/text/evidence.rs` (F51-D harness): its narrative claimed
  the mapping was unmeasured; it now records the successor state, and it
  additionally asserts that every audited media carries a measured verdict and
  that all ten fonts were measured, so a silent revert fails the harness.
* `crates/cs_app/tests/text/original_font.rs` (new): the six
  `accept_f51_fontcell_` tests.
* `crates/cs_app/tests/text/evidence_fontcell.rs` (new): this task's evidence
  harness.

**One observable failure removed:** before this task, running the F51-D audit
over the original installation produced two `unmeasured_glyphs` blockers for
`font.tga`/`arial8.tga`, so every audit of the retail installation was
`is_complete() == false` with per-locale glyph coverage unknown. After it, the
same run produces no `unmeasured_glyphs` blocker at all, each TGA carries
`unused_in_original` with its cited addresses, and `rimage.zbd` carries ten
measured coverages.

## Tests

`cargo test --workspace --locked -- accept_f51_fontcell_ --include-ignored`
selects six tests, four of them unignored so CI runs them:

| test | capability | what it would miss |
| --- | --- | --- |
| `accept_f51_fontcell_cell_rule_maps_authored_cells_to_characters_in_order` | synthetic | the separator/cell/`c - 0x21` rule |
| `accept_f51_fontcell_an_unmapped_character_is_a_named_audit_blocker_not_a_claim` | synthetic | the unmapped-cell blocker |
| `accept_f51_fontcell_a_declared_font_the_package_lacks_is_reported` | synthetic | the missing-font blocker |
| `accept_f51_fontcell_a_font_image_without_a_stored_colour_key_is_refused` | synthetic | the palette refusal |
| `accept_f51_fontcell_retail_ten_bitmap_fonts_measure_94_cells_each` | retail | the real scan (ignored without `$CS_GAME_DIR`) |
| `accept_f51_fontcell_retail_no_media_is_unmeasured_and_every_blocker_names_its_cause` | retail | the real audit verdicts (ignored without `$CS_GAME_DIR`) |

Evidence: `docs/findings/evidence/F51-FONTCELL.json`, generated by the harness
of `crates/cs_app/tests/text/evidence_fontcell.rs` and validated with
`tools/validate_evidence.py --require-pass`.

## Recorded unknowns (they gate claims, they are not dropped)

* **Not `verified_original`.** Static analysis of the executable and a scan of
  retail files do not show the original running. No original capture, no
  `human_play`, no `human_review` was involved.
* **Only the average advance is measured.** Per-character advance widths and
  the vertical metrics (line height, ascent) of the ten bitmap fonts are still
  unmeasured, so `TextMetrics` stays a caller-supplied input and the layout
  tests keep using `synthetic_monospace`. Affected: any claim about the
  original's wrapping or line breaks. Resolving task: a future F51 metrics
  slice (none is filed yet).
* **A 95th cell has no established character.** The original's bound accepts
  `c <= 0x7f`, so cell 94 would be `0x7f`; retail stores exactly 94 cells, so
  `0x7f` is not claimed covered and any cell beyond the mapped positions is
  reported as `bitmap_font_stray_cell` rather than mapped.
* **`gfont3d` renders through Windows.** The recorded coverage is the
  Windows-1252 assigned set, the *design target*; which glyphs one particular
  Arial file contains on a user's machine is an OS property, never bundled
  (F51 non-negotiable 1).
* **The palette variant of a font strip is refused, not scanned.** The colour
  key rule is measured on stored RGB565 words; no retail font image uses a
  palette, and what the original would do with one is not established.
* **One (English) installation** was available, so no localized installation
  was compared (F51-D's own unknown, task #467).
* **The 21 `langui.dll` overflow** is the audit's remaining blocker. It is a
  comparison against declared development metrics, not a measurement of the
  original's layout.
