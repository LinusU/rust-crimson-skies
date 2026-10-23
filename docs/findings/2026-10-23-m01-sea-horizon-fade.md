# #1796 M01 sea: the "hard horizon" was the smoke's parked fog

Date: 2026-10-23. Implementer: `sonnet-1` (Sonnet 5.5); no independent review yet.
Nothing here is `verified_original`; the original executable did not run.

## Measured

The owner-command smoke frames of #1650, #1698 and #1730 showed the sea meeting
the sky at a hard line. In those runs the scene camera's distance fog was
**parked at 1e9 m from frame 144 to the end of the run**, so every frame after
that (`scene_*`, `first_control`, all the probes) was drawn with no fog at all.
Logged `DistanceFog` of the scene camera: 1000/4500 m (the bound `ZONE1` fog)
until frame 135, then parked, restored at 143, parked again at 144 and never
restored.

Cause: `apply_probe` (`mission_session/smoke.rs`) parks the fog while a probe
hides something and restores it afterwards, keeping the value in a `Local`. The
system is registered twice (`Update` and `PostUpdate`), so there are two
`Local`s. The second instance saw the already parked value, saved *that*, and
restored it last. Not a renderer defect and not a fog-form defect.

## Changed

The value to restore is kept on the camera (`KeptFogFalloff`), so both instances
agree. Test: `accept_m01_sea_fade_probe_restores_the_scene_fog` (fails with the
old per-instance `Local`, passes now).

## Result (smoke `private/evidence/M01-SEA-HORIZON`, 69 checks, 0 failed)

With the fog really on, `frame_02411_first_control.png` shows the sea fading
into the haze before the horizon and the island hazed to near invisibility. Down
a column at x=2300 (2560x1440 frame), rows 760 to 880: sea/sky pixel
(171,171,171) at the horizon, then (162,163,164), (148,153,154), (136,146,146),
(126,137,140), (118,131,135): a smooth ramp, no line. `frame_01622_scene_27s.png`
(under the zeppelin) shows the same fade. So item 1 (soft fade) is met by the
measured linear fog 1000/4500 m (#1649) alone; no extra fade geometry or number
was invented.

## Unmet / unknown (rule 4)

* **Item 5, no pixel comparison**: the owner's reference images
  (`private/ref-drydock/m01/orig25.png`, `orig70.png`) were not compared. How
  hazy the original really shows the island at this range is unmeasured; ours is
  stronger than the earlier (unfogged) captures suggested, and that is the
  measured fog, drawn per pixel. The original computes the fog per vertex
  (`0x578120`, `2026-10-23-m01-fog-update-static.md`), so large far polygons may
  differ from a per-pixel ramp. Needs the owner's reference.
* **Item 3, the 16-frame `wtr00000..15` cycle**: still not played. The unit of
  the rate word 10.0 is unknown (atmo-a section 5), and playing it at a guessed
  rate would be an invented number. Affected content: all 255 water parts and
  the horizon plane (frame 0 shown). Resolving task: a static read of the
  material animation consumer in the engine image.
* **Item 2, shore line**: no separate beach/surf geometry was found earlier
  (#1650); this task did not re-search the archive, the thin white line is still
  taken to be the shore tiles' own art.
* **Item 4, tile seams**: the tiles carry stored per-corner colours that differ
  at coast tiles; whether the original shows the same steps is unmeasured.
* The earlier statements in `2026-10-18-parity-m01-atmo-c-sea.md` and
  `2026-10-20-m01-sea-shading.md` about a hard horizon and about frames drawn
  "through the camera fog" refer to unfogged frames and are superseded here.
