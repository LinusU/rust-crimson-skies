# Pause map: 4:3 screen in a 16:9 window is an explicit decision (#1991)

Task `PARITY3-PAUSE-MAP-GAP.08`. Provisional; nothing here is `verified_original`.

## Observation

The owner-command smoke (#1818) renders the pause map as an 800x600 frame
centred in a 1280x720 window with side bars. The owner's original screenshot
is a full 800x600 frame, because it was taken at the original's own 800x600
display mode.

## Policy check

`Fit::of` in `crates/cs_app/src/mission_session/pause_map.rs` maps the
designed 800x600 space to the window with `cs_content::ui_layout::AspectFit`
(`SCREEN_SIZE` = 800x600), the same helper the front-end screens use
(`ui/front_end/screens.rs`, `shell.rs`, `pilot_view.rs`, `audio_view.rs`,
`previous_missions_view.rs`). Pointer input uses the inverse of the same
transform, so clicks stay aligned with the drawn plates.

## Decision

Keep `AspectFit`. The pause screen is authored for 800x600; stretching it to
16:9 would distort the chart and portrait, and other screens already
pillarbox. In an 800x600 window the fit is the identity and the frame matches
the original's geometry. Side bars in a 16:9 window are therefore intended, not
a defect. Compare the pause map to the original screenshot only at 800x600.

No code changed. The remaining pause-screen gaps in
`2026-10-23-pause-map-smoke-frame-vs-original.md` are tracked by their own
tasks; this row is resolved as a recorded decision.
