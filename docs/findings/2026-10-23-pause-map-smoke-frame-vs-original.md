# Pause map: owner-command smoke frame beside the owner's original (#1818)

Task `PARITY3-PAUSE-MAP-SCREEN.03`. Provisional: a scripted run, never
`human_play`, nothing here is `verified_original`.

## What the smoke now does

`cs --cs-path <retail> --mission M01 --smoke-seconds 45 --capture-dir <dir>`
presses Esc in flight, photographs the map (`frame_*_pause_map.png`), takes the
Preferences detour, then **clicks Resume** through the cursor path (it used to
close with Esc). Two new checks:

* `pause_map_clock_frozen`: the physics tick count read when the map was first
  drawn equals the count read just before the Resume press edge, after the
  detour (run of 2026-10-23: 6290 both).
* `pause_map_resumed`: the Resume click closed the map and the run went on.

The run passed: 3988 frames, 7802 ticks, 71 checks, no failures.

## Differences from the owner's original screenshot

The screenshot is the owner's (not committed). Each gap has a task:

| Difference | Task |
| --- | --- |
| Left ~40 % of the chart is a flat white panel, the island chart is offset and cut off | #1984 |
| Whole screen is washed out, the original is opaque | #1985 |
| Note lists one objective, the original four; flag set and positions differ | #1986 |
| No green player aircraft icon | #1987 |
| Portrait is a man, the original a woman in a spotlight pose | #1988 |
| Monospace labels, left-aligned and overflowing the plates; original centred proportional faces | #1989 |
| No arrow cursor | #1990 |
| 4:3 screen letterboxed in a 16:9 window | #1991 |

Until these are resolved the pause screen makes no fidelity claim.
