# F52-B: the objectives page read through the accessibility cues

Spec: `specs/F52-accessibility-and-explicitly-separated-modern-options.md`, `### F52-B`.
Code: `crates/cs_app/src/accessibility/objective_page.rs`, `cues.rs`.
Tests: `crates/cs_app/tests/accessibility/objective_page.rs` (prefix `accept_f52_b_`).

## What exists

- `objective_page` consumes the F46-C `PageView::Objectives` rows
  (`from_view`), giving each row a `Cue` for the colour filter and metrics at
  the UI scale. `status_of` maps the objective runtime's seven
  `ObjectiveState`s; `Hidden` has no cue and is never listed. The row skip
  predicate is the display's own `revealed && state.is_visible()`, so a row
  `ObjectiveDisplay::visible` would hide — an unrevealed objective born into
  a shown state under a deferred reveal rule — stays unlisted.
- `ObjectiveStatus` grew `Optional` and `Superseded` (diamond, strike) so every
  shown state has its own shape and text key; none relies on colour.
- At 300 % scale the content outgrows the viewport; the page reports the scroll
  range and `scroll_to_show` brings any line fully into view. No row is
  dropped or truncated.

## Unknowns and limits (nothing guessed)

1. Row geometry (line height = max(glyph, text), gap 4 px at 100 %) is designed.
   The original objective page layout is F46-B/F47's. Affects: F52-D.
2. Nothing here draws. The glyphs, the localized status words (`objective.*`
   keys have no catalogue entry yet; F51-B) and the real wrapping of long
   objective descriptions at large scale (`text::layout`) are not exercised.
   Affects: render of the page. Resolving: F52-D (`gpu`).
3. Subtitles, per-bus audio levels, the colour filter on the final image and
   the `--safe-settings` flag are still not applied to real render/audio/CLI
   (see F52-A limit 6). Affects: F52-C/D.
