# F51-A: locale fallback, control-markup grammar and font provenance

Date: 2026-10-01. Task: F51-A "Define locale fallback, markup and font provenance"
(`specs/F51-localization-fonts-text-layout-and-original-media-ids.md`, section
`### F51-A`). Shared contract: `docs/contracts/UI-NETWORK.md`. Capabilities
used: ordinary build/test only. No `$CS_GAME_DIR` read, no evidence report
required, and no capability beyond build/test is claimed.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/localization.rs` (new): the declared, Bevy-free
  localization contract.
  - Locale: `LocaleId` (validated, bounded, deliberately opaque — no language
    enum), `MAX_LOCALE_CHAIN_LEN`, `LocaleChain` (the *explicit* ordered
    fallback, refusing an empty chain, an over-long chain and a repeated
    locale), `LocaleIdError`, `LocaleChainError`.
  - Strings: `TextId` (a `string_resource` content id, so a localizable string
    can never be spelled like a mission, a save or a protocol value),
    `LocalizedText` (a locale-bound row with `Origin` + `Provenance` and the
    text verbatim), `TextCatalog` (keyed by `(TextId, LocaleId)`, refusing a
    duplicate pair), `TextResolution::{Resolved, Missing}` (which locale and
    which chain depth answered; a miss names every locale it tried),
    `LocaleAudit` (the machine-readable shape of AC04).
  - Markup: `MarkupDelimiters` (caller-declared spelling, validated), `ControlTag`,
    `MarkupGrammar`, `MarkupToken::{Text, LineBreak, Control, Substitution}`,
    `MarkupIssueKind`/`MarkupIssue` (with the byte offset in the original
    string), `MarkupDocument`, `parse_markup`, `SubstitutionTable`,
    `UNRESOLVED_SUBSTITUTION`.
  - Fonts: `GlyphCoverage` + `MissingGlyphReport` (a missing character is
    counted with its occurrences), `LicensePermission`, `FontLicense`,
    `FontSource` (which can *name* the two forbidden sources so they can be
    refused), `FontProvenance` (only the two shippable answers),
    `FontRefusal::{OperatingSystem, ProprietaryGame, UnverifiedLicense}`,
    `FontFace`/`FontFaceDraft`/`FontCatalog`.
  - Fixtures: `declared_synthetic_text_catalog` (one briefing in `en-us`,
    `de-de`, `fr-fr` at three different lengths, one string only `en-us` has,
    one confirm string), `synthetic_markup_grammar`, `synthetic_font_face`,
    `SYNTHETIC_LONG_TRANSLATION_KEY`.
- `crates/cs_app/src/text/mod.rs`, `metrics.rs`, `layout.rs` (new): the
  application boundary.
  - `metrics.rs`: `TextMetrics` (pixel size, per-character advances, line
    height, ascent, declared coverage; validated, with `scaled` for a UI scale
    change) and `synthetic_monospace` — the **declared** development stand-in
    where F51-B's real font parsing will land.
  - `layout.rs`: `RequiredControl` (a `ui_resource` id plus its rectangle),
    `LayoutRequest`, `layout_text`, `TextLayout` (`viewport`, `lines`,
    `painted_rect`, `visible_line_indices`, `covers`, `max_scroll`,
    `scroll_offset_for_line`), `LaidOutLine`, `TextFit::{Fits, Scrolls}`,
    `LayoutDiagnostic::{Markup, MissingGlyph, UnresolvedSubstitution,
    BrokenWord}`, `LayoutError::{EmptyPanel, NoFreeBand}`.
  - `mod.rs` re-exports the content crate's `SubstitutionTable`,
    `SubstitutionId` and `UNRESOLVED_SUBSTITUTION` so a screen needs one import.
- `crates/cs_content/src/lib.rs`, `crates/cs_app/src/lib.rs` (wiring only):
  `pub mod localization;` / `pub mod text;` and one module-doc paragraph each.
- `crates/cs_app/tests/text/{main,common,catalog,markup,fonts,layout}.rs` (new):
  the 23 `accept_f51_a_*` acceptance tests.
- This file.

**One observable failure:** a long translation wrapped into a panel whose bottom
row holds the `Ok` and `Cancel` buttons paints its last line over both buttons,
and the player can no longer read or activate them. The text viewport is the
whole panel, so every wrapped line — including the ones past the panel's
height — lands on the button rectangles. `accept_f51_a_long_translation_scrolls_
clear_of_the_required_buttons` fails: `TextLayout::covers` is `true` for the
`Ok`/`Cancel` `RequiredControl`s. Verified by mutation: making the layout use
`request.panel` instead of the free band fails 5 of the 23 tests, including this
one.

## The design decisions a reviewer should check

1. **The viewport is the panel's tallest free band, not the panel.** A required
   control blocks a full-width horizontal band (a dialog's button *row*), and
   the layout takes the tallest remaining band, ties going to the topmost. A
   control that covers the whole panel is `LayoutError::NoFreeBand` — a screen
   bug, reported rather than painted over.
2. **Line boxes span the whole viewport width** for the line's height, because
   that is the box a renderer fills. `TextLayout::covers` and
   `painted_rect` therefore evaluate the conservative geometry, and the
   clipping to the viewport is what makes a long text unable to reach a control.
3. **Greedy word wrap with a character break for an over-wide token.** A
   translation's longest word is not knowable in advance, so an unbreakable
   token is broken rather than allowed to overflow the band; the break is
   reported as `LayoutDiagnostic::BrokenWord` and no character is dropped.
4. **The fixture is deliberately uneven.** The `en-us` briefing (1077 chars)
   fits the 440px band at 16px while `de-de` (1758) and `fr-fr` (1715) do not,
   so the same screen exercises *both* halves of "fits or scrolls" and the
   classic "the translation is longer than the layout" case.
5. **A refused control stays literal text.** `parse_markup` never widens the
   grammar: an unknown tag, an argument on a tag that admits none, an
   unbalanced/unterminated/unclosed control, an empty tag and an unterminated
   or empty substitution each produce a `MarkupIssue` *and* stay verbatim in the
   token stream. An admitted closing control is itself a token, so a renderer can
   pop the style the opening control pushed.

## What is designed and what is unknown

Nothing in this stage was measured from the owner's installation, and nothing
here is an original measurement:

- **The original supported locale list is unknown.** `LocaleId` is an opaque
  bounded label and `LocaleChain` is supplied by the caller; there is no default
  chain and no language enum. F51-D's AC04 audit needs `retail`.
- **The original control-markup syntax is unknown.** `MarkupDelimiters` and the
  `ControlTag` set are *declared* by the caller; the token model (text, hard
  line break, control, substitution) is the designed part. F51-B measures the
  real delimiters and supplies them.
- **`cs_content::config::StringCatalog` is deliberately not bridged in.** F12
  gives `(id, language: u32, text)`; mapping a `u32` resource language id to a
  `LocaleId` needs the language ids the retail images carry, and inventing that
  table would guess the locale list. F51-B owns the mapping.
- **The original font formats and glyph coverage are unknown.** No font file is
  parsed, opened or embedded. `FontFace` carries provenance plus a declared
  `GlyphCoverage`; F51-B parses the original font and F51-D audits coverage per
  locale and reviews licenses.
- **Original text layout, clipping, focus order and UI-scale behaviour are
  unmeasured.** This layout is a headless deterministic bounding computation; no
  glyph is rasterized and no renderer reads it yet. Focus *order* over a
  scrolling block is F51-C's (the input/focus session is
  `cs_app::input::InputSession`); what this stage guarantees is the geometry a
  focus step needs — `scroll_offset_for_line` is clamped to
  `0..=max_scroll`, so no focus step can push a line under a control.
- **No subtitle/speaker binding** (non-negotiable 4) is declared here: that
  belongs with F51-C's dialogue integration.

## Non-negotiable behaviour this stage encodes

1. **No OS or proprietary game fonts.** `FontFace::try_new` refuses
   `FontSource::OperatingSystem` and `FontSource::ProprietaryGame`, and refuses a
   licensed fallback whose permission is `Unverified`. `FontCatalog::distributable`
   excludes an original private font, so a packaging step cannot ship one.
2. **Markup is interpreted only after grammar validation.** See decision 5.
3. **Missing glyphs are counted, not silently invisible.** `GlyphCoverage`
   answers `missing_in`; the layout emits one `MissingGlyph` diagnostic per
   (character, line) with its occurrence count, and the German fixture's
   umlauts are exactly that case.
4. **Locale cannot change identity.** `TextId` is a `string_resource` id, the
   catalog is keyed by `(TextId, LocaleId)`, and `TextResolution::Resolved`
   reports the locale and chain depth. Two chains answering the same id produce
   the same `TextId` and the same numeric values; only the text and the reported
   locale differ. `RequiredControl` keeps a `ui_resource` id for the same
   reason.

## Test selection and sensitivity

`cargo test --workspace --locked -- accept_f51_a_ --include-ignored` discovers
and runs 23 tests, all passing. Sensitivity was checked by mutating production
code and confirming failures:

| mutation | tests that fail |
| --- | --- |
| layout uses the whole panel instead of the free band | 5 (incl. the AC01 minimum scenario) |
| `FontFace::try_new` accepts an operating-system font | `accept_f51_a_operating_system_and_proprietary_fonts_are_refused` |
| `parse_markup` interprets an unknown tag instead of refusing it | 4 (incl. both markup and layout) |
| `TextCatalog::resolve` ignores the caller's chain order and reports depth 0 | 4 (incl. the fallback and layout scenarios) |

## Follow-ups not filed as new tasks

The unknowns above are already owned by the sheet's later stages, so filing
duplicates would only add noise: #191 `F51-B` (resource decoding, real markup
delimiters, the `StringRow.language` → `LocaleId` mapping, font parsing and real
metrics), #192 `F51-C` (menus/HUD/subtitles, focus order, original font
loading) and #207 `F51-D` (the per-locale string/media audit, glyph coverage and
the license review, both needing `retail`).
