# #2001 BUG-M01-WHITE-WEATHER-SHEET: the white sheet was the designed haze plane

Date: 2026-10-23. Implementer: `claude-2` (Sonnet 5.5); no independent review yet. Nothing here is `verified_original`.

## Cause (by source and by construction, not by a toggle run)

`render::weather` drew a **designed** `Haze` quad: horizontal, 40 km across, unlit, `AlphaMode::Blend`, fixed world
altitude `DESIGNED_HAZE_ALTITUDE_M = 300`, coverage 0.35, centred under the camera. It is not a record value. A
translucent horizontal plane blends over every ground pixel when seen from above and cuts straight through hills
where terrain crosses its altitude. That matches the owner's report (washed out from above, normal from below, hard
border through hills). The owner's "about 1000" is **approximate**; the plane was designed at 300 m and no measured
altitude exists for it.

The actual M01 (`ZBD/C3/M01`) authors `CLOUD_COVER` `BOTTOM 10000`, `TOP 11000`
(`2026-10-18-parity-m01-atmo-a-weather-units-and-sky-consumers.md`), a volume that the original tints, not a sheet.
The earlier bind test and the weather docs quoted `BOTTOM 1055` / fog 0.69 from `ZBD/C1C/M01`, a different archive.

I did not toggle the layers on a GPU before the fix; the diagnosis rests on the geometry above and on the post-fix
frame, which shows no sheet.

## Change

* The haze layer is deleted, not tuned. Distance fog stays the horizon's (`FOG_COLOR`/`FOG_RANGES`).
* An authored cloud band draws nothing (no coplanar deck). How the volume looks from inside it is still open
  (#1643, #1745); no altitude or opacity is invented.
* The labelled designed deck remains for an environment with no cloud record (the original-assets playtest).
* Tests: `accept_m01_play_weather_*` now read `zbd/c3/m01` (band 10000–11000, zone fog colours differ, no drawn
  layer) and assert no layer at the old haze altitude. The C1C harness constant is renamed `C1C_FIXTURE`.

## Not done

* No altitude/pitch smoke probes were added, and no toggled-layer shots were taken. The owner-command smoke on the
  fixed build passed (71 checks, no failures); `frame_02451_world.png` shows island, sea and sky with no sheet.
* No comparison against the owner's original frames was made.
