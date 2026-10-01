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
  - `MarkupToken::is_opening` / `is_closing` / `control_name` (a renderer needs
    the opening/closing distinction to push and pop a style),
    `MarkupDocument::paragraph_substitutions` (the unresolved ids **per
    paragraph, in render order, with repeats**, so a layout can name the line a
    marker landed on), `LocaleAudit::coverage` / `coverage_percent`,
    `SubstitutionTable::insert`'s validated form and
    `SubstitutionValueError::{BadId, MarkerInValue}` — all added by review; see
    the review section for why each was needed.
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
  the 30 `accept_f51_a_*` acceptance tests.
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

## Review findings and the fixes applied

The implementer's branch was reviewed against the sheet, `AGENTS.md` and
`docs/contracts/UI-NETWORK.md`. The four defects below were real bugs in the
shipped code, each found by reading rather than by a failing check, and each is
now fixed with a test that fails without the fix.

1. **`TextCatalog::ids` counted rows, not strings, so the AC04 audit's
   denominator was inflated.** The map is keyed by `(TextId, LocaleId)`, so a
   string translated into three locales appeared three times in the key
   iteration; `LocaleAudit::ids` therefore reported 5 for a 3-string catalog
   and `resolved` could exceed the number of strings a locale could ever
   answer. The doc comment already claimed "deduplicated", so the code
   contradicted its own contract. `ids` now deduplicates the adjacent equal ids
   the ordered key map yields, and `LocaleAudit::coverage` /
   `coverage_percent` were added so a caller cannot re-derive a ratio with the
   wrong denominator. New test:
   `accept_f51_a_the_locale_audit_denominator_counts_strings_not_rows`.
2. **`MarkupToken::Control` could not express a closing control.** Both
   `[bold]` and `[/bold]` produced the identical token
   `Control { name: "bold", argument: None }`, while the module doc promised
   "a renderer can pop the style the opening control pushed" — a consumer had
   no way to tell the two apart except by re-deriving the nesting itself. The
   variant now carries `closing: bool` and the stream has
   `MarkupToken::is_opening` / `is_closing` / `control_name`. New test:
   `accept_f51_a_a_closing_control_is_distinguishable_from_an_opening_one`.
3. **A substitution id that failed the token grammar was mislabelled as empty.**
   `parse_substitution` mapped *every* `validate_token` failure onto
   `EmptySubstitution { offset }`, so `{first name}` was reported as "a
   substitution had no id" when the author had written one. The diagnostic sent
   the fix in the wrong direction. Added
   `MarkupIssueKind::BadSubstitutionId { id, offset }` (code
   `bad_substitution_id`), which names what was written. New test:
   `accept_f51_a_a_malformed_substitution_id_is_named_not_called_empty`.
4. **Every unresolved substitution was reported on the same line.** The layout
   searched for "the first line containing the marker", so a briefing with
   `{runway}` on line 4 and `{pilot}` on line 11 reported both on line 4 —
   pointing a content fix at the wrong place, which is the entire purpose of
   carrying a line number. `MarkupDocument::paragraph_substitutions` now returns
   the unresolved ids **per paragraph, in render order and with repeats**, and
   the layout pairs the k-th marker of a paragraph with the k-th id of that
   paragraph. New test:
   `accept_f51_a_each_unresolved_substitution_is_reported_on_its_own_line`.

Two gaps the review closed that were not outright bugs:

- **`SubstitutionTable::insert` accepted a value containing the marker**, which
  made the fix for defect 4 unsound: a supplied `Ma\ufffdr a` would have been
  counted as somebody else's unresolved marker. The marker now means exactly
  "this substitution was unresolved", and the value is refused
  (`SubstitutionValueError::MarkerInValue`); the id is validated against the
  same token grammar as a tag (`BadId`). An *empty* value is still admitted —
  that is the caller's data and a different defect from a missing entry. New
  test: `accept_f51_a_a_substitution_value_may_not_impersonate_the_marker`.
- **`TextLayout::covers` could not be shown to discriminate.** Because the free
  band is computed to exclude the declared controls, the AC01 assertion holds by
  construction, so a broken `covers` would not have failed any test. The new
  `accept_f51_a_covers_detects_a_control_inside_the_viewport` lays text out in an
  unreserved panel and shows `covers` reporting a control placed on a painted
  line, which pins the geometry check independently of the free-band
  computation. A mutation that dropped the viewport clipping from `covers` fails
  4 tests, this one included.

### Reviewer sensitivity checks (independent of the implementer's)

Every fix was mutation-checked: the reverted behaviour fails the new tests.

| mutation | tests that fail |
| --- | --- |
| `TextCatalog::ids` stops deduplicating | 2 (incl. the denominator test) |
| every control token is emitted as `closing: false` | 1 (the closing-control test) |
| `BadSubstitutionId` reported as `EmptySubstitution` | 2 |
| no marker counted per line (defect 4 restored) | 2 |
| every line counted as holding a marker | 2 |
| `SubstitutionTable` accepts a marker-carrying value | 1 |
| `coverage` divides by a wrong denominator | 1 |
| `covers` ignores clipping to the viewport | 4 |

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
and runs 30 tests, all passing. Sensitivity was checked by mutating production
code and confirming failures:

| mutation | tests that fail |
| --- | --- |
| layout uses the whole panel instead of the free band | 5 (incl. the AC01 minimum scenario) |
| `FontFace::try_new` accepts an operating-system font | `accept_f51_a_operating_system_and_proprietary_fonts_are_refused` |
| `parse_markup` interprets an unknown tag instead of refusing it | 4 (incl. both markup and layout) |
| `TextCatalog::resolve` ignores the caller's chain order and reports depth 0 | 4 (incl. the fallback and layout scenarios) |
| the eight reviewer mutations listed above | 1–4 each, see the review section |

## Follow-ups not filed as new tasks

The unknowns above are already owned by the sheet's later stages, so filing
duplicates would only add noise: #191 `F51-B` (resource decoding, real markup
delimiters, the `StringRow.language` → `LocaleId` mapping, font parsing and real
metrics), #192 `F51-C` (menus/HUD/subtitles, focus order, original font
loading) and #207 `F51-D` (the per-locale string/media audit, glyph coverage and
the license review, both needing `retail`).
