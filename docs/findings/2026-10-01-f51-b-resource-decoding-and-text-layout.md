# F51-B: resource decoding and text layout

Date: 2026-10-01. Task: F51-B "Implement resource decoding and text layout"
(`specs/F51-localization-fonts-text-layout-and-original-media-ids.md`, section
`### F51-B`). Shared contract: `docs/contracts/UI-NETWORK.md`. Capabilities
used: ordinary build/test only. No `$CS_GAME_DIR` read, no evidence report
required, and no capability beyond build/test is claimed.

Implemented by `deepseek-1/deepseek-1`. This file is the implementer's record;
it is not a review and awards no `verified_original`/`release_approved`.

## Observable failure (listed before editing)

An installation's localized strings cannot reach a screen at all. F12
(`cs_content::config::StringCatalog`) reads `(id, language, text)` rows, but
nothing turns them into F51 `LocalizedText` rows: the locale catalog is empty,
so there is nothing to resolve or draw — a hard capability gap, not a cosmetic
one. And there is no single production call that resolves an id, validates its
markup and lays it out, so a caller that lays a raw resource string out loses
every markup diagnostic; malformed markup and absent glyphs are only visible if
the caller happens to call `parse_markup` and inspect the document itself.

## Files and what changed

- `crates/cs_content/src/localization.rs` (owner path):
  - `LanguageMap` + `LanguageMapError` + `MAX_LANGUAGE_MAP_LEN`: the
    **caller-declared** map from a raw resource language id (`u32`) to a
    [`LocaleId`]. It refuses the empty map (nothing could ever decode), an
    over-long map and a language id mapped twice. There is deliberately **no
    built-in language table**: the original supported-locale list is
    unmeasured, so a guessed one would be a fabricated compatibility claim.
  - `TextId::from_resource_id(u32)`: the stable key
    `string_resource/<decimal id>` for a PE string unit, so a translation is a
    different *text* for one *id*.
  - `ResourceDecode::decode(&[StringRow], &LanguageMap, Origin, Provenance)`:
    the F12 bridge. A row whose language the map declares **and** whose code
    units decoded becomes a `LocalizedText`; a row in an undeclared language is
    reported in `unmapped_languages`; a row whose units did not decode
    (`StringRow::text == None`) is reported in `undecodable_ids`; a duplicate
    `(id, locale)` pair is a contradiction, so **neither** copy is kept and the
    pair is reported in `duplicates`. Nothing is replaced by a placeholder and
    no language is inferred.
- `crates/cs_app/src/text/screen.rs` (owner path, new):
  `ScreenTextRequest`, `ScreenText`, `ScreenTextError` and
  `layout_localized_text` — the one production call a screen makes. It resolves
  a `TextId` through a `LocaleChain`, parses the resolved row's text with the
  declared `MarkupGrammar`, lays the parsed document out with `layout_text` and
  returns the `ScreenText`. A miss is `ScreenTextError::Missing { id, tried }`
  (naming every locale tried), never an empty label.
- `crates/cs_app/src/text/layout.rs` (owner path): `impl Display for
  LayoutDiagnostic`, so each of the existing typed diagnostics renders as one
  human-readable line. This is what makes the diagnostics *visible* rather than
  only countable.
- `crates/cs_app/src/text/mod.rs` (wiring): `pub mod screen;` and the
  re-exports; module docs updated to describe F51-B.
- `crates/cs_app/tests/text/{main,common,resource,screen}.rs` (owner path): the
  7 `accept_f51_b_*` tests and the `resource_row`/`language_map` fixtures.
- This file.

## The design decisions a reviewer should check

1. **The language map is the caller's, not the crate's.** The mapping from a
   resource language id to a locale is exactly the unknown F51-A flagged and
   F51-B owns. The honest way to "own" an unmeasured table is a validated
   caller-declared map plus a report of every id it does not cover — not a
   hard-coded `1033 -> en-US` guess. F51-D's retail `retail` capability can
   measure the real id set and drive this map.
2. **A duplicate `(id, locale)` pair keeps neither copy.** Choosing the first
   would hide the contradiction the same way F12's `StringCatalog::resolve`
   refuses to choose between two strings sharing an `(id, language)`.
3. **The decode consumes rows, not a file.** `ResourceDecode::decode` takes
   `&[StringRow]` (what `StringCatalog::rows()` returns) so it stays a pure
   transformation and cannot open an original image; `Origin`/`Provenance` are
   supplied by the caller and never asserted by the decoder.
4. **The pipeline is the only place markup validation is guaranteed.** It
   calls `parse_markup` itself, so a screen cannot accidentally lay a raw
   resource string out and lose the grammar's diagnostics.

## Non-negotiable behaviour this stage encodes

1. **No OS or proprietary fonts.** Unchanged: no font is opened, parsed or
   embedded; `FontFace`/`FontSource` still refuse both sources.
2. **Markup only after grammar validation.** `layout_localized_text` validates
   via `parse_markup` before layout; a refused control stays literal text and
   is reported.
3. **Missing glyphs counted, not silently invisible.** The pipeline emits the
   font coverage's `MissingGlyph` diagnostics and renders them.
4. **Locale cannot change identity.** Decoding a row yields the same
   `string_resource/<id>` under every locale; the locale only chooses which
   text answers.

## Test selection and sensitivity

`cargo test --workspace --locked -- accept_f51_b_ --include-ignored` discovers
and runs **8** tests, all passing:

- `resource::accept_f51_b_decoding_maps_declared_languages_and_reports_the_rest`
- `resource::accept_f51_b_a_clean_decode_is_complete_and_locale_keyed`
- `resource::accept_f51_b_a_language_map_refuses_empty_and_duplicate_languages`
- `resource::accept_f51_b_a_duplicate_key_with_an_undecodable_copy_keeps_neither`
  (added by review, below)
- `screen::accept_f51_b_malformed_markup_and_absent_glyphs_are_visible_diagnostics`
  (the minimum scenario)
- `screen::accept_f51_b_a_clean_string_has_no_diagnostics`
- `screen::accept_f51_b_a_missing_string_is_a_named_miss_not_a_blank`
- `screen::accept_f51_b_a_fallback_locale_answers_and_is_reported`

Sensitivity was checked by mutation and then reverting:

| mutation | tests that fail |
| --- | --- |
| `layout_localized_text` skips `parse_markup` (empty document) | 3 screen tests, incl. the minimum scenario |
| `ResourceDecode::decode` keeps a duplicate `(id, locale)` pair | 1 resource test |
| `ResourceDecode::decode` excludes an undecodable row from the collision count | 1 resource test (the review fix below) |

Full local checks before handover: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
and `cargo test --workspace --locked` all pass.

## Review fix (reviewer `deepseek-1/deepseek-1`, same agent instance/model)

**Not an independent review.** The reviewer is the same agent instance and model
as the implementer, so this is an internal check-and-mend pass, not the fresh
independent review the owner asks for on format/text semantics. It is recorded
here rather than passed off as independent evidence.

One defect was found by reading the decode against its own contract and F12's:

- **A duplicated `(id, locale)` pair where one copy was undecodable silently kept
  the other copy.** The struct doc and `ResourceDecode::decode` both promise that
  a duplicate pair keeps *neither* copy, "matching F12's own refusal to choose".
  But pass one only counted rows whose units decoded (`StringRow::text.is_some()`),
  so `(0, en, None)` plus `(0, en, Some("Confirm"))` counted as a unique key and
  kept the decodable row — even though `StringCatalog::resolve(0, en)` counts both
  rows and returns `Ambiguous`. The undecodable row now still takes part in the
  collision count; a duplicated pair drops both copies (the undecodable id is
  also listed in `undecodable_ids`), and the pair is listed in `duplicates`. New
  test `accept_f51_b_a_duplicate_key_with_an_undecodable_copy_keeps_neither`,
  which fails (2 decoded rows instead of 1) when the undecodable row is excluded
  from the count again. `ResourceDecode::duplicates`'s doc comment was reworded
  ("one entry per pair", not "both copies").

## What remains unknown (owned by later stages, not guessed here)

- **The original supported-locale list and the real `StringRow.language` set.**
  The map is caller-declared; the retail measurement belongs to F51-D's AC04
  audit (`retail`). This is the same unknown F51-A recorded.
- **The original control-markup delimiter spelling.** `MarkupGrammar` remains
  caller-declared; `MarkupDelimiters::DECLARED` is still this project's
  synthetic spelling, not a measurement.
- **The original font format and per-glyph metrics.** No font is parsed; the
  `TextMetrics` seam stays a caller-supplied input, and the original-font decode
  awaits a measured format (F51-D once known). A synthetic monospace stand-in
  exercises the path headlessly.
- **Rendering, focus order and subtitles.** F51-C owns wiring this path into
  the real menus, HUD and subtitles, and the subtitle speaker/cue binding
  (non-negotiable 4).

These are already owned by F51-C (#192) and F51-D (#207) per the sheet, so no
duplicate tasks were filed.
