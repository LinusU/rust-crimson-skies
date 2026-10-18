# PARITY-M01-LC-LETTERBOX (#1664): the launch cinematic's letterbox rows, measured

Date: 2026-10-18. Slice (iii) of #1642. Evidence: the owner's three Drydock
frames of the retail 1.02 launch cinematic, 1024x768 RGBA PNG (`d1` Pandora from
behind, `d3` plane leaving the tail, `d5` plane below the hull), downloaded into
the gitignored `private/ref-drydock/m01/`. Nothing here is `verified_original`;
the frames are reference captures supplied through the owner.

## Measured

For every row, the maximum of the R, G and B channels over all 1024 columns:

| frame | rows with max = 0 at the top | first picture row | first row of the bottom bar | rows black at the bottom |
| --- | --- | --- | --- | --- |
| d1 | 0..89 | 90 | 675 | 675..767 |
| d3 | 0..89 | 90 | 675 | 675..767 |
| d5 | 0..89 | 90 | 675 | 675..767 |

The transition is a single row in each frame (row 89 is exactly 0, row 90 is
picture, row 674 is picture, row 675 is exactly 0). The picture spans the whole
width (columns 0 and 1023 carry picture). So the bars are **90 rows on top and
93 rows at the bottom** (the picture is 585 rows high); the task text's
"roughly y=90..675" is confirmed exactly, and the bars are *not* symmetric.

## Implemented

`mission_session/letterbox.rs`: `letterbox_bars(width, height)` scales these rows
to the largest 4:3 rectangle in the window (same rule as the cockpit HUD) and
extends the bars to the window edge outside it; `MissionLetterboxPlugin` spawns
two black full-width nodes above all other UI, shown only while the
`LaunchLetterbox` resource is active. The resource defaults to off.

## Still unknown

* **Timing.** All three frames show the bars complete, so whether they slide in
  (and over how many frames) or appear at once, and when they leave, is not
  measurable from them. A capture of the first frames after Fly Mission and of
  the hand-over is needed from the owner. The implementation switches instantly.
* **Switch.** Nothing sets `LaunchLetterbox` yet: the cinematic phase is
  #1663 (blocked on the hand-over pose and duration). Until it lands, the bars
  cannot appear in an M01 run, so the owner-command smoke has no letterboxed
  frame to cite; that belongs to the integration (#1642).
