# F51-FONTCELL: the bitmap-font cell-to-character mapping is not in the installation

Date: 2026-10-01. Task: F51-FONTCELL "#466 Decode the original bitmap-font
cell-to-character mapping (`font.tga`, `arial8.tga`)". Follow-up from F51-D
(`specs/F51-localization-fonts-text-layout-and-original-media-ids.md`, section
`### F51-D`; `docs/findings/2026-10-01-f51-d-locale-glyph-overflow-and-license-audit.md`,
`## What remains unknown`, first bullet). Capabilities used: `retail` (read
access to the original installation at `$CS_GAME_DIR`). No `human_review` and no
`human_play`.

Authored by `deepseek-1/deepseek-1`. This file is the implementer's record of a
**blocked** stage, not a review; it awards no `verified_original` or
`release_approved`. No original byte, extracted atlas or screenshot of original
content is committed; every original-data operation below was read-only and its
derived bytes were kept under `private/` (Git-ignored).

## Result

**The task is blocked: the cell-to-character mapping of `font.tga` and
`arial8.tga` is not declared anywhere in the available installation.** The two
files are conventional Truevision TGA images with no font semantics; the glyph
images are only *visually* legible, and no descriptor, metrics table, script or
engine resource in the installation assigns a character code to a cell. Any
`GlyphCoverage` asserted from them would therefore be a human visual reading of
a bitmap, not a decode — exactly the guess the task says to avoid.

## Observable failure (listed before editing)

F51-D's production audit (`crates/cs_app/src/text/audit.rs`,
`cs_app::text::audit::audit_localization`) audits `GOSDATA/ASSETS/GRAPHICS/font.tga`
and `.../arial8.tga` as media and must pass `GlyphEvidence::Unmeasured` for both,
so every audit of the original installation is `is_complete() == false` and
per-locale glyph coverage from the original fonts is unknown. The follow-up task
assumed a font-format decode exists; the check below shows the installation
carries no such mapping.

## What was searched (all read-only)

Fingerprints of the two files, for identification (the F51-D finding already
recorded these digests):

| file | bytes | sha256 |
| --- | --- | --- |
| `GOSDATA/ASSETS/GRAPHICS/font.tga` | 65 580 | `3c544ab4369f61507f5293bc08bea308b86e2d9a9a592d67a9684a6a172effc1` |
| `GOSDATA/ASSETS/GRAPHICS/arial8.tga` | 45 636 | `a8d6dca770f504c7d5094d0380341d35b3f57e6e66530292a5978dcdcc85c476` |

1. **The TGA files themselves carry no font semantics.** Parsed with the
   production TGA reader's rules: `font.tga` is type 2, 128×128, 32 bpp, no image
   id (`idlen = 0`), no colour map, no extension area, standard
   `TRUEVISION-XFILE.` footer; `arial8.tga` is type 10 (RLE), 256×256, 32 bpp,
   same empty id field and footer. A TGA stores pixels, not a code→cell table.
2. **`crimson.rof` has no companion.** Through the production ROF reader
   (`cs-inspect rof --container GOSDATA/ASSETS/crimson.rof`) the container has
   **846 members**; the only font-related members are
   `ASSETS/GRAPHICS/ARIAL8.TGA` and `ASSETS/GRAPHICS/FONT.TGA`. There is no
   `.fnt`, metrics, charset or descriptor member, and no second file beside
   either TGA.
3. **`zrdr.zbd` declares only GDI fonts.** The reader archive's `FONTS` section
   lists logical names (`lucida_console_8`, `lucida_console_8b`,
   `modern_white_12`, `verdana_red_11`, `verdana_white_11`, `modern_red_12`,
   `verdana_green_11`, `5PointHUD`, `5PointHUDBrite`, `verdana_Offwhite_11`,
   `WINDOWS_FONTS`). `WINDOWS_FONTS` is a table of **Windows GDI font
   definitions** (`face`, `height`, `color`, `shadow`, `weight`) — including an
   entry literally named `arial8` with `face = "Arial"`, `height = -8`. None of
   the entries carries a character range or a bitmap cell map. The
   `5PointHUD`/`modern_*`/`lucida_*` names are the engine's *own* bitmap fonts,
   which live as a different, named resource family in `rimage.zbd`
   (`lucida_console_8` at `0x00e372b9`, `modern_white_12` at `0x012db155`, …) —
   they are not `font`/`arial8`, and they are not TGAs.
4. **The UI string ids are GDI font ids, not atlas cells.** `strings.dll` and
   `RESOURCE.H` define `DEFAULTFONT 9`, `FONT_FONTTABLE 10` and dozens of
   `FONT_*` ids (`FONT_ARIAL8`, `FONT_TNR36B`, …); `UNINSTAL.EXE` loads
   TrueType faces with `AddFontResourceA` from a `SYSTEM\FONTS\*.TTF` list. The
   shell/menu text is therefore Windows-GDI text, not a decode of these atlases.
5. **The engine script that uses the atlas declares no mapping.**
   `GLOBALS.SCRIPT` (extracted read-only from `crimson.rof`) contains the only
   `font3d` *declaration*, `font3d gfont3d = "assets\\graphics\\" "arial8.tga"`:
   it hands the whole image to the engine's `font3d` type and states no first
   code point, column count or cell size. Every other extracted script
   (`CTL.SCRIPT`, `MAINMENU.SCRIPT`, `LAYOUT.CSV`) only *consumes* that handle
   (`print3d_attributes = @globals@gfont3d, …`) or names a `FONT_*` resource id;
   none declares a cell→character map. (`font.tga` is named by no extracted
   script; the installation-wide case-insensitive search finds it only in the
   installer manifest `UNINSTAL.EXE` and as a `crimson.rof` member name.) The
   mapping is an engine-internal convention that is not expressed in any
   readable data.
6. **No data file stores a character-order table.** A search for an alphabet
   string (`ABCDEFGHIJKLMNOPQRSTUVWXYZ` / `abcdefghijklmnopqrstuvwxyz`) across
   the installation matches only the unrelated system libraries `mfc42.dll` and
   `ztiff.dll`; no game archive or script declares a glyph order.

## Why that means blocked, not implemented

The atlas *geometry* is measurable (a 16-column, 8-pixel grid for `font.tga`;
a 32-column, ~12-pixel-pitch strip for `arial8.tga`), and the glyph bitmaps are
legible to a human. But decoding a `GlyphCoverage` needs the *character code of
each cell*, and that association exists only (a) in the original executable's
hardcoded logic, which is not readable data and cannot be reverse-engineered
within this task's scope, or (b) in a human's visual identification of the
glyphs. Neither is a value this repository may copy into production code and
call measured:

- Asserting the obvious-looking ASCII order (`0x20`–`0x7F` left-to-right,
  top-to-bottom) would be **guessing a layout**, which F51 and
  `AGENTS.md` rule 4 forbid, and a reviewer has no in-data evidence to check it
  against.
- The `human_review` capability that could confirm a visual reading is never
  available to an agent, and is not among the task's required capabilities.
- The task's own escape clause applies verbatim: *"If the mapping cannot be
  measured from the available installation, `block_task` with the evidence
  rather than guessing a coverage."*
- Spec hard-failure rule: a missing gameplay/fidelity input blocks; synthetic
  stand-ins (the audit's `synthetic_monospace`) cannot certify original-font
  coverage.

The F51-D audit therefore keeps its honest `GlyphEvidence::Unmeasured` verdict.
This finding is the evidence the task asked to be recorded.

## What would unblock the task

Any one of:

1. An owner-supplied **reference capture** (or manual identification) that binds
   specific `font.tga`/`arial8.tga` cells to characters, with `human_review`.
2. The location of an original data file that declares the mapping (a
   `.fnt`/metrics/`.zrd` companion, or the executable's font table exposed as
   readable data). None exists in this installation as far as the search above
   reaches.
3. Confirmation that the intended assets are the engine's *named* bitmap fonts
   in `rimage.zbd` (`lucida_console_8`, `modern_white_12`, …) rather than the
   loose TGAs; that is a different, larger format-decode task, and the audit's
   media list does not name those.

## Affected content

`GOSDATA/ASSETS/GRAPHICS/font.tga`, `GOSDATA/ASSETS/GRAPHICS/arial8.tga`; the
F51-D audit's glyph-coverage verdict for the original installation
(`crates/cs_app/src/text/audit.rs`, `crates/cs_app/tests/text/audit.rs`,
`crates/cs_app/tests/text/common.rs`).

## Identities and sources

Implementer: `deepseek-1/deepseek-1` (opencode, model
`deepseek/deepseek-v4.1-flash`, Rally #466). No independent reviewer is recorded;
the task was blocked rather than merged.

Sources: `$CS_GAME_DIR` read-only (`GOSDATA/ASSETS/GRAPHICS/font.tga`,
`.../arial8.tga`, `GOSDATA/ASSETS/crimson.rof`, `ZBD/zrdr.zbd`,
`ZBD/rimage.zbd`, `ZBD/interp.zbd`, `strings.dll`, `UNINSTAL.EXE`); the
production readers `cs_formats::texture::tga`, `cs_assets::rof`,
`cs_assets::zbd` and the `cs-inspect rof` command; `specs/F51-...md`
(non-negotiable 1/3, AC02/AC04, evidence rules); the F08-B TGA findings and the
F51-D finding; `AGENTS.md` rules 4 and 5.
