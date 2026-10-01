# F51-C: integrate menus, HUD, subtitles and original font loading

Date: 2026-10-01. Task: F51-C "Integrate menus, HUD, subtitles and original font
loading" (`specs/F51-localization-fonts-text-layout-and-original-media-ids.md`,
section `### F51-C`). Shared contract: `docs/contracts/UI-NETWORK.md`.
Capabilities used: ordinary build/test only. No `$CS_GAME_DIR` read, no evidence
report required, and no capability beyond build/test is claimed.

Implemented by `deepseek-1/deepseek-1`. This file is the implementer's record; it
is not a review and awards no `verified_original`/`release_approved`.

## Observable failure (listed before editing)

F51-B built `layout_localized_text`, the one production call a screen makes, but
left nobody to make it. There is no runtime that holds the selected locale, the
decoded catalog and the loaded fonts, so menus, HUD and subtitles would each have
to assemble them independently — and each could resolve the same id against a
different locale. Two concrete holes follow:

- Changing locale has no owner: nothing validates a locale switch, so a
  half-loaded catalog silently blanks every label instead of being refused and
  retried, and nothing carries the choice across a save round-trip.
- A font that cannot be measured has no load transaction: a caller cannot tell
  "this face loaded" from "this face was left half-built", and text could be laid
  out from metrics that were never read.

And the minimum scenario of the stage — *switch locale and reopen the same save
without losing unlocks* — has no production path at all, because locale is not
yet a persisted setting separate from the campaign/record/unlock fields.

## Files and what changed

- `crates/cs_content/src/localization.rs` (owner path): the locale-setting bridge.
  - `LOCALE_SETTING_KEY = "ui.locale"` — a **designed** engine key, explicitly
    separate from every campaign/record/unlock field.
  - `LocaleSettingError` (`NoLabels`, `DuplicateLabel`, `DuplicateEntry`,
    `MalformedStoredValue`).
  - `LocaleSetting::rule(&'static [&'static str])` builds the localization
    feature's own `SettingRule`: the value is a `ValueRule::Choice` over exactly
    the caller's declared labels, the change is `SettingApply::Live` (a locale
    change takes effect on the next layout, not after a restart) and the default
    is the first declared label. It refuses an empty declaration and a repeated
    label.
  - `LocaleSetting::read(&ProfileDocument)` returns the stored locale, refusing a
    key stored twice and a stored value that is not a valid `LocaleId`.
  - `LocaleSetting::write(&mut ProfileDocument, &LocaleId)` touches **only** the
    locale entry: it replaces in place when present, appends when absent, leaves
    every other field untouched, and refuses a key stored twice rather than
    silently rewriting a damaged save.
- `crates/cs_app/src/text/fonts.rs` (owner path, new): the font load/retry
  transaction.
  - `FontMeasurer` — the declared seam where F51-D's real font decode lands. It
    is deliberately not implemented in this crate: the only honest production
    answer today is "unmeasured".
  - `FontLoadError` (`Empty`, `Unmeasured { family, reason }`, `Duplicate`,
    `Absent`).
  - `LoadedFont` (`id`, `face`, `metrics`, `family`, `is_distributable`) and
    `FontSet` (`new`, `load`, `insert`, `reload`, `get`, `metrics`, `contains`,
    `len`, `is_empty`, `fonts`, `distributable`). `load` measures **every** face
    before returning, so one unmeasurable face fails the whole load and leaves no
    half-populated set; an empty catalog is `Empty`; `reload` retries one face
    without storing a failed attempt.
- `crates/cs_app/src/text/session.rs` (owner path, new): `TextSession` — the one
  runtime menus, HUD and subtitles share. It owns exactly one chain, catalog,
  grammar and `FontSet`, and whose default font must already be loaded
  (`try_new` -> `MissingFont`). Its operations:
  - `switch_locale(chain, catalog)` — the locale teardown/retry. The offered
    catalog is checked with `TextCatalog::audit` before it is installed; a catalog
    that answers not one id for its chain is refused with
    `NoContentForLocale { chain }` and the previous locale stays fully in force,
    so a half-loaded locale never blanks the screen. The caller retries the same
    call once the content is repaired.
  - `retry_font(face, measurer)` — the same teardown/retry for a font a measurer
    could not read on an earlier attempt; a failed reload leaves the loaded set
    untouched.
  - `set_default_font(id)` -> `MissingFont` when not loaded.
  - `menu_text`, `hud_text`, `subtitle_text` bind the resolved `ScreenText` to the
    **locale-free** identity the consumer keys on: a menu element `ui_resource`
    id, a HUD field `ui_resource` id, or a speaker plus the exact cue id and its
    tick window. A miss is a named `TextSessionError::Missing`, never an empty
    label. A subtitle with no speaker (`EmptySpeaker`), zero duration
    (`ZeroDuration`) or an overflowing tick window (`TickOverflow`) is refused
    before it is bound. `SubtitleText::is_visible_at(tick)` is the half-open
    `[start_tick, end_tick)` window.
- `crates/cs_app/src/text/mod.rs` (wiring): `pub mod fonts;`, `pub mod session;`,
  their re-exports and the module docs for F51-C.
- `crates/cs_app/tests/text/session.rs` (owner path, new): the 6 `accept_f51_c_*`
  tests.
- `crates/cs_app/tests/text/main.rs` (owner path): `mod session;` and the target
  docs for F51-C.
- This file.

## The design decisions a reviewer should check

1. **The locale is a setting, and only a setting.** A locale change is persisted
   through the ordinary profile settings list (`set_setting("ui.locale", ...)` in
   the test, driven by the feature's own `SettingRule`), never by rewriting the
   document wholesale. `LocaleSetting::write` is a settings-only edit that cannot
   reach `campaign`, `records` or `blueprints`; the AC03 test asserts those are
   unchanged after a locale switch and a reopen. This is F51 non-negotiable
   behavior 5 made structural rather than a rule a caller must remember.
2. **A locale switch is checked before it is installed.** A catalog that answers
   nothing is refused with the previous locale still live, so the failure mode is
   "the old screen stays readable and the caller retries", not "every label
   disappears". This is the teardown/retry the stage requires.
3. **Font loading is a whole-set transaction.** One face that cannot be measured
   fails the load and nothing is stored, so a session never starts with a subset
   of the fonts its content asked for. The format is unmeasured, so `FontMeasurer`
   returns the numbers (`TextMetrics`) a real decode will supply; a measurer that
   cannot read a face says so ("unmeasured") instead of inventing numbers.
4. **Identity is locale-free.** `MenuText::element`, `HudText::field` and
   `SubtitleText::{speaker, cue, start_tick, end_tick}` do not change when the
   locale does; only the text does. That is what keeps a locale change away from
   save ids and mission identity.
5. **No original data and no invented tables.** The supported-locale list, the
   markup spelling, the font format and the metrics all stay caller-declared; the
   test declares them explicitly and says so.

## Non-negotiable behaviour this stage encodes

1. **No OS or proprietary fonts.** No font is opened, parsed or embedded; the
   only faces that can reach `FontSet` are `FontFace`s, whose provenance
   `FontFace::try_new` already restricts. `LoadedFont::is_distributable` reads
   that provenance so a packaging step cannot ship a private font.
2. **Markup only after grammar validation.** Every `TextSession` layout goes
   through `layout_localized_text`, which parses with the declared grammar before
   laying out; the session never sees raw resource text.
3. **Missing glyphs counted, not silently invisible.** Unchanged from F51-B: the
   layout emits the coverage's `MissingGlyph` diagnostics and the session returns
   them on the `ScreenText`.
4. **Subtitles bind to exact speaker/cue ids and timing.** `subtitle_text`
   requires a speaker, refuses zero duration and overflow, and binds the cue id
   and tick window exactly; generated replacement dialogue is not in this path.
5. **Changing locale cannot change save ids, mission identity, numeric parsing or
   protocol values.** The locale lives in one settings entry; the AC03 test proves
   unlocks survive a switch and a reopen.

## Test selection and sensitivity

`cargo test --workspace --locked -- accept_f51_c_ --include-ignored` discovers and
runs **7** tests, all passing:

- `session::accept_f51_c_switching_locale_and_reopening_the_save_keeps_unlocks`
  (the minimum scenario AC03: production `ProfileSession` + `SettingCatalog`,
  locale stored, unlocks asserted byte-for-byte after reopen, runtime rebuilt from
  the reopened save resolves German)
- `session::accept_f51_c_a_refused_locale_switch_keeps_the_previous_catalog`
- `session::accept_f51_c_menus_hud_and_subtitles_bind_locale_independent_identity`
- `session::accept_f51_c_a_subtitle_requires_a_speaker_and_a_nonzero_window`
- `session::accept_f51_c_font_loading_propagates_failure_and_retries`
- `session::accept_f51_c_the_locale_setting_preserves_unlocks_and_save_identity`
- `session::accept_f51_c_the_locale_rule_refuses_a_label_it_cannot_read_back`
  (added by review, below)

Sensitivity was checked by mutation and then reverting:

| mutation | tests that fail |
| --- | --- |
| `TextSession::switch_locale` installs the catalog without the `audit` check | the refused-switch test |
| `LocaleSetting::write` returns early without touching `settings` | the settings test (value not written) |
| `LocaleSetting::write` appends instead of replacing | the settings test (two entries for the key) |
| `TextSession::try_new` skips the default-font check | the font-load test (`MissingFont`) |
| `subtitle_text` uses `start_tick + duration_ticks` instead of `checked_add` | the subtitle test (`TickOverflow`) |
| `LocaleSetting::rule` accepts a label without checking it is a canonical `LocaleId` | the label test |

Full local checks before handover: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
and `cargo test --workspace --locked` all pass.

## Review fix (reviewer `deepseek-1/deepseek-1`, same agent instance/model)

**Not an independent review.** The reviewer is the same agent instance and model
as the implementer, so this is an internal check-and-mend pass, not the fresh
independent review the owner asks for on text/locale semantics. It is recorded
here rather than passed off as independent evidence.

One defect was found by reading the rule against its own reader and writer:

- **`LocaleSetting::rule` accepted a declared label that could never be read
  back.** The rule's value space is a `ValueRule::Choice` over the caller's raw
  label strings, while `LocaleSetting::read` and `LocaleSetting::write` carry a
  `LocaleId` — which the constructor *trims* and validates. A caller that
  declared `" en-us "`, `""`, `"en us"` or an over-long label got a rule whose
  default (`labels[0]`) the `SettingCatalog` seeds into a new profile, but
  `LocaleSetting::read` would then either trim it to a different value or refuse
  it as `MalformedStoredValue`, and no `LocaleChain` could be built from it. The
  declared value space and the persisted/loaded value must be the same set, or
  the feature's own default is unusable — which is exactly the locale/chain
  hand-off AC03 relies on. `LocaleSetting::rule` now validates every label with a
  new private `canonical_locale_label` helper and returns
  `LocaleSettingError::InvalidLabel { label, reason }` for an empty, over-long,
  out-of-grammar or non-canonical label. `LocaleSettingError` gained that variant
  and its `Display` arm. The existing settings test also gained a
  `MalformedStoredValue` assertion for a stored value that is not a locale label.
- New test `accept_f51_c_the_locale_rule_refuses_a_label_it_cannot_read_back`
  covers whitespace-padded, empty, bad-character and over-long labels, then
  round-trips every accepted label through `write`/`read` and asserts the reader
  returns exactly the declared spelling. It fails (`expect_err`-style
  `matches!` on `InvalidLabel`) when the validation is removed.

## What remains unknown (owned by later stages, not guessed here)

- **The original supported-locale list.** `LocaleSetting::rule` takes the caller's
  declared labels; the real set is F51-D's AC04 audit (`retail`).
- **The original control-markup delimiter spelling.** `MarkupGrammar` stays
  caller-declared.
- **The original font format and per-glyph metrics.** No font is parsed;
  `FontMeasurer` is the declared seam and a session never lays text out from
  invented numbers. F51-D's real decode fills it once the format is known.
- **Focus order and UI scale.** F51's non-negotiable 3 also names focus order;
  the focus-order helper is not part of this stage's sheet, so it is noted here
  rather than guessed. It is not claimed as implemented.
- **Rendering and audible playback.** No renderer and no audio: this stage is the
  data/runtime boundary only.

These are already owned by F51-D (#207) and the rendering/audio features per the
sheet, so no duplicate tasks were filed.
