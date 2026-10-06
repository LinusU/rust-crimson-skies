# F51-D: locale, glyph, overflow and license coverage of the original string images and fonts

Date: 2026-10-01. Task: F51-D "Run locale/glyph/overflow coverage and license
review" (`specs/F51-localization-fonts-text-layout-and-original-media-ids.md`,
section `### F51-D`). Shared contracts: `docs/contracts/UI-NETWORK.md` and
`docs/contracts/CLI-EVIDENCE.md`. Capabilities used: `retail` (read access to the
original installation at `$CS_GAME_DIR`) and `gpu` (a real Apple M3 Pro / Metal
adapter), plus `synthetic` for the unignored half of the suite.

Implemented by `deepseek-1/deepseek-1`. This file is the implementer's record; it
is not an independent review and it awards no `verified_original` or
`release_approved`. The original executable was not run; `retail` here means
read access to the installation files, and it is never evidence of how the
original behaves.

## Observable failure (listed before editing)

F51-A declared the typed localization records, F51-B the fit-or-scroll layout and
F51-C the runtime menus, HUD and subtitles share — but the stage's own scenario
had no production path at all: **no code audited a string image per declared
locale, counted the strings a translation cannot fit, or reviewed the fonts'
license and glyph coverage.** Concretely:

- Nothing walked a catalog's distinct ids per declared locale, so a locale with
  no rows and a locale that answers everything through fallback produced the same
  shape: an empty screen. `TextCatalog::audit` measured **one** chain; nothing
  measured a whole declared locale *set* against the catalog.
- Nothing laid a locale's resolved strings out, so an overflow was invisible
  until a player saw it; F51 non-negotiable behavior 3 ("long translations ... no
  silent overflow") had no evidence behind it.
- The two original bitmap fonts (`font.tga`, `arial8.tga`) were never opened or
  reviewed: their provenance (never redistributable) and their glyph coverage
  (unmeasured) were not recorded anywhere.

## Files and what changed

- `crates/cs_content/src/localization.rs` (owner path): the multi-locale audit
  records.
  - `MAX_SUPPORTED_LOCALES = 64` — a *designed* bound, because the original
    release's supported-locale list is unmeasured (F51-A).
  - `SupportedLocalesError` (`Empty`, `TooLong`, `Duplicate`) and
    `SupportedLocales` — a caller-declared, order-preserving locale set that
    refuses an empty set, an over-long one and a repeated locale instead of
    silently deduplicating it.
  - `LocaleCoverage` (`locale`, `chain`, `ids`, `translated`, `via_fallback`,
    `missing`) and `MultiLocaleAudit` (`locales`, `ids`, `undeclared`,
    `missing_everywhere`, `is_complete`, `coverage_for`, `translated_total`).
  - `TextCatalog::audit_locales(&SupportedLocales)` — the whole declared set
    measured against one catalog. Each declared locale is audited against a
    chain of itself followed by the other declared locales in declaration order,
    so `translated` is what the locale answers *itself* and `via_fallback` is
    how much its chain borrows. The denominator is the catalog's own distinct
    ids. Locales the catalog holds rows for but nobody declared, and ids no
    declared locale answers, are reported, never dropped.
- `crates/cs_app/src/text/audit.rs` (owner path, new): the audit itself.
  - `GlyphEvidence` (`Declared { coverage }` / `Unmeasured { reason }`),
    `MediaSource` and `MediaAudit` (path, length, SHA-256, distributability,
    glyph evidence).
  - `StringImageSource` (path, F12 rows, origin, provenance), `StringImageAudit`
    (per image: distinct ids, per-locale coverage + overflow, undeclared
    locales, missing ids, decode accounting), `LocaleTextAudit`, `AuditBlocker`
    and `LocalizationAudit`.
  - `audit_localization(&LocalizationAuditRequest)` — decodes each image through
    the caller's `LanguageMap` exactly as `ResourceDecode` does, audits every
    declared locale, parses and lays every resolved string out in the caller's
    panel (so an overflow and a covered control are counted), audits each media
    file, and returns one flat blocker list.
  - **Each image is decoded in isolation.** The original ships several PE
    string images whose id numbering is shared, so the same id names different
    text in different images; a cross-image `(id, locale)` collision is not a
    contradiction. A duplicate inside one image still is.
- `crates/cs_app/src/text/gpu_capture.rs` (owner path, new): the `gpu` half.
  `TextBox`, `text_boxes(layout, panel)` (the laid-out lines' measured text
  extents mapped onto the frame), `TextCapture`, `TextCaptureError` and
  `capture_text_boxes(label, boxes, png)` — an offscreen Bevy 2D render of the
  production `layout_text` geometry with a named refusal
  (`NoVisibleLines`, `NoScreenshotCaptured`, `UniformFrame`, `Io`) for every way
  a written file would not be evidence of a drawn block.
- `crates/cs_app/src/text/layout.rs` (owner path): `LaidOutLine::text_width()` —
  the measured advance width of a line's own text. The conservative band-wide
  `rect` is unchanged (it is what makes `TextLayout::covers` a real check); the
  narrower extent is what lets the GPU witness show the *text*'s geometry, so
  two locales draw different frames rather than one identical band.
- `crates/cs_app/src/text/mod.rs` (wiring): the `audit` and `gpu_capture` module
  declarations, their re-exports and the F51-D module docs.
- `crates/cs_app/tests/text/audit.rs` (owner path, new): the 7 `accept_f51_d_*`
  tests.
- `crates/cs_app/tests/text/common.rs` (owner path): the three routed string
  image names, the two font media names, and the `retail_*` helpers that read
  and audit the installation through production code.
- `crates/cs_app/tests/text/evidence.rs` (owner path, new): the evidence-report
  harness (not named `accept_f51_d_*`, so the task selection never picks it up).
- `crates/cs_app/tests/text/main.rs` (owner path): `mod audit;`, `mod evidence;`
  and the target docs.
- `docs/findings/evidence/F51-D.json` and this file.

## What the retail audit measured

Read through the production `StringCatalog::read`, audited in isolation, under a
**caller-declared** supported-locale set of one locale (`en-us`) mapped from the
one language id (1033) the images carry. The declared set is not a claim about
the original release's supported-locale list, which stays unmeasured (F51-A).

| image | RT_STRING units (rows) | distinct ids | en-us translated | en-us overflowing | en-us covering a control | missing / undeclared / unmapped / undecodable / duplicate |
| --- | --- | --- | --- | --- | --- | --- |
| `strings.dll` | 1 792 | 1 792 | 1 792 | 0 | 0 | 0 / 0 / 0 / 0 / 0 |
| `GOSDATA/ASSETS/BINARIES/langui.dll` | 1 616 | 1 616 | 1 616 | **21** | 0 | 0 / 0 / 0 / 0 / 0 |
| `GOSDATA/ASSETS/BINARIES/language.dll` | 48 | 48 | 48 | 0 | 0 | 0 / 0 / 0 / 0 / 0 |

So every unit of every image decodes and is answered by `en-us` — the
single-installation, single-language observation F12-D already recorded. The
audit's own verdict is nevertheless **incomplete** (`is_complete() == false`),
and honestly so, for one measured reason and one unmeasured one:

- **21 `langui.dll` strings overflow the declared panel.** They do not fit the
  declared 640×480 panel over the reserved button row at the declared 16 px
  monospace metrics, so a screen would have to scroll them. This is the overflow
  the stage exists to surface, and it is counted, not hidden. It is a
  comparison against *declared development metrics*, not a measurement of the
  original's font or layout: with different metrics the same 21 (or a different
  set) could fit. No line paints over a required control, so AC01 holds.
- **The two original bitmap fonts have unmeasured glyph coverage.** `font.tga`
  (65 580 B) and `arial8.tga` (45 636 B) are glyph *images* whose
  cell-to-character mapping is not known, so no `GlyphCoverage` can be asserted
  for them without guessing. Each is recorded as
  `GlyphEvidence::Unmeasured` and is a named `unmeasured_glyphs` blocker;
  neither is distributable (provenance `OriginalPrivate`).
  *(Superseded 2026-10-06 by task #466, which measured the rule from the
  owner's static analysis and reclassified both files as unused by the
  original; the first reason above still blocks.)*

## The design decisions a reviewer should check

1. **The declared set is caller-declared, and the audit refuses a bad one.** An
   empty set, an over-long set and a repeated locale are errors, because each
   would make the audit's result depend on an accident. Nothing here invents the
   original supported-locale list.
2. **Ennumerators are distinct ids, not rows.** `ids` is `TextCatalog::ids()`, so
   a string translated into three locales counts once; a caller that counted
   rows could inflate coverage.
3. **Ids are per image.** Decoding the three images as one table would report
   their shared numbering as thousands of contradictions; the module is explicit
   about why it audits each image separately.
4. **Overflow is measured, not assumed.** Every resolved string is parsed with
   the declared grammar and laid out with the production `layout_text` in the
   caller's panel; a layout that cannot be produced at all counts as an overflow
   rather than a silent success.
5. **Unmeasured is a blocker, not a pass.** The original font mapping and the
   single-locale installation are the audit's asserted verdict and are why
   `is_complete()` is false. They are not in the evidence report's `unknowns`
   (which are the task's own blockers and are empty because acceptance passed);
   they are the product incompleteness this finding records.
6. **The GPU witness is geometry, not appearance and not glyphs.** It draws the
   production line boxes (the measured text extents) on the real adapter and
   refuses a frame that is uniform. It does not render the original's glyphs, use
   the original's colours, or measure the original's metrics.

## Non-negotiable behaviour this stage encodes

1. **No OS or proprietary fonts.** The two original fonts are audited and
   `distributable == false`; nothing is opened for rendering or embedded. A
   licensed fallback is distributable only with verified permission
   (`synthetic_font_face` carries one, and the synthetic test proves both
   directions).
2. **Markup only after grammar validation.** The overflow pass parses every
   resolved string with the declared `MarkupGrammar` before laying it out.
3. **Missing glyphs counted, not silently invisible.** Unchanged from F51-B, and
   the audit adds the file-level glyph blocker.
4. **Subtitles bind to exact ids/timing.** Unchanged; F51-C owns the path.
5. **Locale cannot change save ids, mission identity, numeric parsing or
   protocol values.** The audit is a read-only measurement: it never writes a
   row, a save or a setting.

## Test selection and sensitivity

`cargo test --workspace --locked -- accept_f51_d_ --include-ignored` discovers
and runs **7** tests, all passing (4 synthetic, unignored so CI runs them; 2
adapter tests and 1 retail test marked `#[ignore]`, run here with
`--include-ignored` and each also run alone with `--exact`):

- `accept_f51_d_every_declared_locale_is_audited_against_each_string_image`
- `accept_f51_d_unanswered_ids_and_unaccounted_rows_are_named_blockers`
- `accept_f51_d_overflowing_text_is_counted_per_locale_and_never_covers_a_control`
- `accept_f51_d_media_license_and_unmeasured_glyphs_are_audited_not_assumed`
- `accept_f51_d_a_gpu_capture_proves_the_laid_out_lines_were_drawn`
  (`#[ignore = "requires a GPU adapter"]`)
- `accept_f51_d_a_capture_that_drew_nothing_is_refused_rather_than_written`
  (`#[ignore = "requires a GPU adapter"]`)
- `accept_f51_d_retail_every_string_image_and_font_is_audited_for_the_declared_locale`
  (`#[ignore = "requires CS_GAME_DIR"]`)

Sensitivity was checked by mutation and then reverting:

| mutation | test that fails |
| --- | --- |
| `audit_image` stops counting a scrolling layout as an overflow | the overflow test |
| `TextCatalog::audit_locales` does not subtract `served_by_fallback` from `translated` | the per-locale coverage test |
| `audit_localization` never emits the unmeasured-glyph blocker | the media/license test |
| `text_boxes` uses the band-wide `rect` instead of the line's `text_width` | the GPU test (the three locale frames become byte-identical) |

## Review

Reviewed as Rally `#207` by `deepseek-1/deepseek-1` (opencode, model
`deepseek/deepseek-v4.1-flash`) with a **fresh context**: the reviewer did not
implement this branch and re-derived everything from the diff, the spec and the
original installation. `cargo fmt --all -- --check`, `cargo clippy --workspace
--all-targets --all-features --locked -- -D warnings`, `cargo test --workspace
--locked`, and `cargo test --workspace --locked -- accept_f51_d_
--include-ignored` (7/7, on a real Apple M3 Pro / Metal adapter with
`$CS_GAME_DIR` set) all passed on the rebased head.

The evidence report was regenerated on the rebased checkout and matched the
committed copy on every semantic field: the same 7 assertions all `pass`, the
same `source`, `tests`, `capabilities`, `claim` and `unknowns`, and a
byte-identical `string-media-census.json` and three GPU frames. Only
`candidate_tree` and the `cargo-test.log` digest differ, because the committed
copy is the follow-up evidence commit (the `F18-D` precedent has the same
shape); `tools/validate_evidence.py --require-pass` passes on it.

The tests' sensitivity was re-checked independently: mutating production code to
drop the fallback subtraction from `translated`, to never count a scrolling
layout as an overflow, and to never emit the unmeasured-glyph blocker made
exactly the per-locale, overflow and media/license tests fail and no others.
The reviewer found no code defect and changed no production or test code. It
corrected this finding's follow-up claim (the sheet names no later font-decode
stage) and filed `#466` / `#467` above. This is an agent review with a fresh
context; it is not `verified_original` and not the owner's approval.

## Evidence

- Report: `private/evidence/F51-D/acceptance.json`, validated with
  `python3 tools/validate_evidence.py private/evidence/F51-D/acceptance.json
  --artifact-root private/evidence/F51-D --require-pass` →
  `{"structurally_valid": true, "artifact_count": 5,
  "claims_semantically_verified": false}` (exit 0). A copy is committed as
  `docs/findings/evidence/F51-D.json`.
- Artifacts (stay in `private/`): `cargo-test.log`, `string-media-census.json`
  (counts and digests only), and three per-locale GPU frames
  (`render-en-us.png`, `render-de-de.png`, `render-fr-fr.png`), byte-distinct
  because the locales' text widths differ.
- Installation fingerprint (production discovery):
  `install_sha256 = b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`,
  `content_sha256 = a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d`.
- Font digests: `font.tga` `3c544ab4369f61507f5293bc08bea308b86e2d9a9a592d67a9684a6a172effc1`,
  `arial8.tga` `a8d6dca770f504c7d5094d0380341d35b3f57e6e66530292a5978dcdcc85c476`.
  No original text, byte or screenshot of original content is committed.

## What remains unknown (recorded, not guessed)

- **The original bitmap-font cell-to-character mapping.** The fonts are audited
  as media but not decoded, so per-locale glyph coverage from the original fonts
  is `Unmeasured`. This is the blocker the audit reports; it needs a font-format
  decode that does not exist yet.
- **The original supported-locale list.** `SupportedLocales` is caller-declared;
  the real list is unknown, and this installation carries only language id 1033.
- **Whether a localized installation preserves stable ids while changing text.**
  Only one (English) installation was available, so F12-D AC04 remains open and
  this audit could not compare two locales of the same id.
- **The original's own metrics.** The overflow count is against declared
  development metrics (`synthetic_monospace`), not a measurement of the
  original's font, so "21 overflow" is a property of that comparison, not a
  claim about the shipped game's layout.
- **Focus order and UI scale** remain noted by F51-C and are not claimed here.

These are the F51-D-owned unknowns; the affected content is the three string
images and the two fonts above. The sheet assigns no later stage to the
original bitmap-font decode (it names only F51-A..D) and F51-D is its last
stage, so the two blockers are filed as follow-ups that gate the fidelity
claims above rather than left without a resolving task:

- `#466 F51-FONTCELL` — decode the original bitmap-font cell-to-character
  mapping (`font.tga`, `arial8.tga`) so the audit can report measured glyph
  coverage instead of `GlyphEvidence::Unmeasured`.
  **Closed 2026-10-06 by #466**: the owner's static analysis of the decrypted
  executable (Rally #466 owner note, 2026-10-05) established that neither TGA
  is read as a font at all, and that the game's bitmap fonts are the ten
  `fonts.zrd` images in `ZBD/rimage.zbd`, whose cell rule is now measured by
  production code. Both TGAs carry a `unused_in_original` verdict with the
  cited addresses, `rimage.zbd` carries ten measured coverages (94 cells
  each), and the audit emits no `unmeasured_glyphs` blocker any more. See
  `docs/findings/2026-10-06-f51-fontcell-bitmap-font-coverage.md`.
- `#467 F51-LOCALE-SET` — measure the original supported-locale set and verify
  localized-installation id stability (F12-D AC04); it needs a second localized
  installation or owner-supplied reference material.

Both were filed during the Rally review of `#207`; neither is guessed here.

## Identities and sources

Implementer: `deepseek-1/deepseek-1` (opencode, model
`deepseek/deepseek-v4.1-flash`, Rally #207). No independent reviewer is recorded
here; the Rally review claim will supply one.

Sources: `$CS_GAME_DIR` read-only (`strings.dll`, `langui.dll`, `language.dll`,
`font.tga`, `arial8.tga`); `specs/F51-...md` (non-negotiable behaviour, AC01/AC04,
evidence rules); `docs/contracts/UI-NETWORK.md`; `docs/contracts/CLI-EVIDENCE.md`;
`schemas/evidence.schema.json`; the F51-A/F51-B/F51-C findings; the F12-D and
F12-G findings for the string-image routing and id numbering; and the F18-D
audit+GPU finding for the evidence-report precedent.
