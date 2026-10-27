# F52-W2: presentation settings reach the output paths

Task #781. Code: `crates/cs_app/src/camera/effects.rs`,
`render/presentation.rs`, `audio/bus_levels.rs` (+ the `mix_session` call),
`ui/hud/presentation.rs`. Tests: `crates/cs_app/tests/accessibility/present.rs`
(prefix `accept_f52_w2_`, 5 tests).

## What exists

- **Effect producer.** `camera::effects::effects_of` turns a `GameEvent`
  (player hit, impact, weapon flash, target change) into `Effect` requests;
  `EffectFrame::present` runs them through `SettingsSession::present`, so
  shake/flash obey the live switches and the notices always pass. A negative or
  non-finite magnitude is an `EffectError` in `EffectFrame::errors`.
- **Colour filter.** `render::presentation::filter_frame` / `present_frame`
  filter an RGBA8 frame under the live session's filter; alpha is untouched, a
  non-RGBA buffer is refused unchanged.
- **UI scale and subtitles.** `ui::hud::presentation::HudPresentation` reads
  scale, filter and subtitle switch from the session each frame; empty
  subtitle lines are reported.
- **Bus levels.** `mix_session` multiplies each emitter's gain by
  `Presentation::bus_level` of its bus from the `LivePresentation` resource
  (optional: no resource, no scaling). A product above `MAX_AUDIO_GAIN` is held
  at the ceiling.
- **One live session.** `LivePresentation(SettingsSession)` is the resource all
  of them read; nothing copies the settings.

## Limits (nothing guessed)

1. **No gameplay event is wired to `GameEvent` yet.** The producer exists and is
   tested with authored events; the damage/ordnance/targeting systems do not
   call it, and nothing inserts `LivePresentation` at boot (that is F52-W1,
   #780, whose `SettingsSession::open` it needs). Affected: every on-screen
   shake/flash.
2. **The filter and HUD layout are not yet in the draw loop.** `present_frame`
   works on a pixel buffer and `HudPresentation` is a layout value; no render
   system calls them on the swapchain/HUD pass. No `gpu` evidence, so no visible
   claim is made. The filter matrices and the base text size are designed.
3. **Only emitter loops honour bus levels.** Radio, music, one-shot and UI
   voices start without passing through `apply_bus_level`; their buses are not
   scaled yet. No `audio` evidence, so no audible claim is made.
4. **Shake amplitude (`SHAKE_PER_DAMAGE`) and flash from damage are designed**;
   the original's values are unknown.
5. Presentation never reaches `Settings::gameplay_inputs()`; unchanged.
