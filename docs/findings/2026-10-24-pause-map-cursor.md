# Pause map: the arrow cursor (#1990)

Task `PARITY3-PAUSE-MAP-GAP.07`. Provisional: a scripted run, never `human_play`,
nothing here is `verified_original`.

## Cause

The pause screen already spawns the cursor (`PauseCursor`, `daglove` with
`dafinger` on rollover, from the `CURSOR` command of `escape.zrd`) as a UI node,
so it is part of the framebuffer. `drive_pause_map` shows it only while
`Window::cursor_position()` is `Some`. The smoke script moved the pointer only
for its Preferences click, after the open-map frame was photographed, so that
frame had no cursor. This is not a capture limit and not a missing feature.

## Change

The smoke rests the scripted pointer at logical (300, 250) on the open map,
clear of every button, one frame after the map opens. The run of 2026-10-24
(`cs --cs-path <retail> --mission M01 --smoke-seconds 45 --capture-dir <dir>`)
photographed the arrow (orange arrow with a brass rim, `daglove`) in
`frame_*_pause_map.png`. Test: `accept_parity3_pause_map_gap_07_`.

**Not measured:** where the original's pointer rests in the owner's screenshot
(the position is the player's), and that `daglove` is the art the original
draws; the owner's screenshot shows an orange arrow, the frame matches by eye.

## Unrelated

The same run failed four flak checks (`flak_turret_fired`, `flak_burst_drawn`,
`flak_flicker_drawn`, `flak_sounds_queued`), with the weapon refusal
`wep_28 has neither CANNON nor ROCKET`. Nothing in the pause path is involved.
