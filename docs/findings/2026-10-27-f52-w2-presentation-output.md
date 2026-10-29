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

## F52-W2-wire (#2140): the draw-loop wiring

`camera::effects::emit_combat_events` (in `FixedLast`, after contact
recording) turns aircraft-layer contacts into `Impact`, accepted shots and
ordnance launches into `WeaponFlash`, and published hits on the player victim
into `PlayerHit`; `emit_targeting_events` (in `Update`, after
`drive_targeting_schedule`) emits `TargetChanged`. `present_game_events`
drains the `GameEventQueue` once per rendered frame through
`EffectFrame::present` and the live session into `PresentedEffects`;
`apply_presented_shake` offsets the camera pose in `PostUpdate`, after
`follow_camera` and the mission's view passes (cockpit eye, external and
crash cameras overwrite the transform after `Update`). `drive_hud_presentation`
lays the active radio line's subtitle and the presented flash into
`PresentedHud`, ordered after `advance_radio` so the line is this frame's;
`SubtitleHudPlugin` draws them in windowed compositions. `present_captured_frame` filters every delivered
screenshot before it is measured or saved, recording `FrameFilterReport`.
`AudioOutput::leveled` wraps the device in `BusLevelDevice` for the whole
`mix_session` pass and for every direct call site routed through
`with_live_device` (launch voices, ambient voices, measured voices); the
briefing voice scales through `apply_bus_level` on its own device.
`run_playtest` and `run_windowed` install the session through
`install_live_presentation` before the first frame and flush it at teardown.

Remaining limits:

- **The cinematic *measurement* probe `play_on_device` deliberately stays at
  unity** on the Music bus: its reported timings must not move with the
  player's settings. It is the only unscaled voice left — the player-facing
  `MoviePlayer` voice is scaled by `cinematic_voice_start` (main,
  `f52_w2_cinematics`) and `MenuAudio` by `f52_w2_menu_audio`.
- **The colour filter reaches delivered captures, not the swapchain.** What
  `present_frame`/`filter_frame` can do is multiply a CPU-side RGBA buffer,
  and the only production buffers are the screenshots the smoke, the
  playtest capture and the front-end capture driver deliver; the live
  window's image is untouched (a filtered swapchain is a post-process pass
  `filter_frame` cannot express). Captures under a session record the
  filtered frame; everything the player sees stays unfiltered until the
  post-process exists.
- **Camera shake magnitude (`SHAKE_METERS`) and its phase are designed**; the
  original's shake is unrecovered.
- **The flash veil's alpha (`FLASH_MAX_ALPHA`) and the subtitle style are
  designed**; no original HUD reference is measured.
- **Gun-shot flash is wired at the `WeaponScheduleReport`/`MissionOrdnanceReport`
  seam**: a shot an engine-side producer accepts without writing those reports
  would not flash.
- Presentation still never reaches `Settings::gameplay_inputs()`; the
  `accept_f52_w2_wire_` tests prove it.
