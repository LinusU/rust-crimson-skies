# F51-A: locale fallback, control-markup grammar and font provenance

Date: 2026-10-01. Task: F51-A "Define locale fallback, markup and font provenance"
(`specs/F51-localization-fonts-text-layout-and-original-media-ids.md`, section
`### F51-A`). Shared contract: `docs/contracts/UI-NETWORK.md`. Capabilities
used: ordinary build/test only. No `$CS_GAME_DIR` read, no evidence report
required, no `CS_CAPABILITIES` beyond build/test is claimed.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/localization.rs` (new): the declared, Bevy-free
  localization contract. `LocaleId` (validated, bounded, deliberately opaque),
  `LocaleChain` (the explicit ordered fallback, refusing an empty chain, an
  over-long chain and a repeated locale), `TextId` (a `string_resource` content
  id, so a string identity can never be a mission or save id), `LocalizedText`
  (a locale-bound row with `Origin` + `Provenance`), `TextCatalog` +
  `TextResolution::{Resolved, Missing}` and `LocaleAudit`, the declared control
  markup — `MarkupDelimiters`, `ControlTag`, `MarkupGrammar`, `MarkupToken`,
  `MarkupIssue` and `parse_markup` — and the font half: `GlyphCoverage`,
  `MissingGlyphReport`, `LicensePermission`, `FontLicense`, `FontSource`,
  `FontProvenance`, `FontFace`, `FontCatalog` and the
  `FontRefusal::{OperatingSystem, ProprietaryGame, UnverifiedLicense}` path.
  Fixtures: `declared_synthetic_text_catalog`, `synthetic_markup_grammar`,
  `synthetic_font_face`, `SYNTHETIC_LONG_TRANSLATION_KEY`.
- `crates/cs_app/src/text/mod.rs`, `metrics.rs`, `layout.rs` (new): the
  application boundary. `TextMetrics` (advance width, line height, ascent and
  glyph coverage at a pixel size, plus `synthetic_monospace`), `SubstitutionTable`
  re-export, `RequiredControl`, `LayoutRequest`, `LaidOutLine`, `TextFit`,
  `TextLayout` (with `covers`, `visible_lines`, `max_scroll`,
  `scroll_offset_for_line`), `LayoutDiagnostic` and `layout_text`.
- `crates/cs_content/src/lib.rs`, `crates/cs_app/src/lib.rs` (wiring only):
  `pub mod localization;` / `pub mod text;` and one module-doc paragraph each.
- `crates/cs_app/tests/text/{main,fixtures,catalog,markup,fonts,layout}.rs`
  (new): the `accept_f51_a_*` acceptance tests. The minimum scenario,
  "long localized text fits or scrolls without covering required buttons", is
  `accept_f51_a_long_translation_scrolls_clear_of_the_required_buttons` in
  `layout`.
- This file.

**One observable failure:** a long translation wrapped into a panel whose
bottom row holds the `Ok` and `Cancel` buttons paints its last line over both
buttons, and the player can no longer read or activate them. The text viewport
is the whole panel, so every wrapped line — including the ones past the panel's
height — lands on the button rectangles. `accept_f51_a_long_translation_scrolls_
clear_of_the_required_buttons` fails: `TextLayout::covers` is `true` for the
`Ok`/`Cancel` `RequiredControl`s and the fit is reported as
`TextFit::Scrolls` with the overflowing lines drawn rather than scrolled inside
the free band above the buttons.

## What is designed and what is unknown

Nothing in this stage was measured from the owner's installation, and nothing
here is an original measurement:

- **The original supported locale list is unknown.** `LocaleId` is an opaque
  bounded label with no enum of languages, and `LocaleChain` is *supplied* by
  the caller. No default chain is invented. F51-D's AC04 audit needs `retail`.
- **The original control-markup syntax is unknown.** `MarkupDelimiters` and the
  `ControlTag` set are this project's *declared* grammar, supplied to
  `parse_markup` by the caller; the token model (text, hard line break,
  control, substitution) is the part that is designed, and the F12
  `StringRow` verbatim code units are what a real importer would feed it.
  F51-B owns measuring the real delimiters.
- **The original font formats and their glyph coverage are unknown.** No font
  file is parsed, opened or embedded here. `FontFace` carries provenance and a
  `GlyphCoverage` set; F51-B parses the original font and F51-D audits the
  coverage per locale.
- **A `StringRow.language` (a `u32` from the F12 PE reader) is not mapped to a
  `LocaleId` here.** The mapping table is unmeasured, so `cs_content::config`
  is deliberately *not* bridged into `TextCatalog` in this stage: doing so
  would guess the locale list. F51-B owns that mapping with the language ids
  the retail images actually carry.
- **Original text layout, clipping, focus order and UI-scale behaviour are
  unmeasured.** The layout here is a headless, deterministic bounding
  computation; no glyph is rasterized and no renderer reads it yet.

## Non-negotiable behaviour this stage encodes

1. **No OS or proprietary game fonts.** `FontSource` can *name* an operating
   system font and a proprietary game font only so that
   `FontFace::try_new` refuses them (`FontRefusal::OperatingSystem`,
   `FontRefusal::ProprietaryGame`); a `FontFace` can only hold
   `FontProvenance::OriginalPrivate` (an installation span, never
   redistributed) or `FontProvenance::LicensedFallback` carrying a license
   whose permission is `Verified`. An unverified license is refused too.
2. **Markup is only interpreted after grammar validation.** `parse_markup`
   consults the supplied `MarkupGrammar`; an unknown tag, an argument on a tag
   that does not take one, an unbalanced or unterminated control and an
   unterminated substitution are each a `MarkupIssue` and stay **literal text**
   in the output. No resource string is ever interpreted as executable markup.
3. **Missing glyphs are counted, not silently invisible.** `GlyphCoverage`
   answers `missing_in`, and the layout turns every absent glyph into a
   `LayoutDiagnostic::MissingGlyph` naming the character, the line and the
   count.
4. **Locale cannot change identity.** `TextId` is a `string_resource` id, the
   catalog is keyed by `(TextId, LocaleId)`, and `TextResolution::Resolved`
   reports the locale and the chain depth that answered. Two different chains
   answering the same id produce the same `TextId` and the same numeric
   values; only the display text and the reported locale differ.

## Test selection

`cargo test --workspace --locked -- accept_f51_a_ --include-ignored`
