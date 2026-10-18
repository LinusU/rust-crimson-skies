# PARITY-M01-HUD: the original-style in-flight HUD of M01

Date: 2026-10-18. Task: #1641. Capabilities: `retail`, `gpu`, `audio`. Nothing here is
`verified_original`.

## Source of the layout

The owner's original captures (`ref-drydock/m01/original-flight/c05.png`, `c14.png`) are outside
this checkout and were not readable (AGENTS.md rule 10). The layout is therefore the task
description's measured description of them: compass strip top centre; bottom-left ROCKETS
(`BOOM`), ALT and a green-to-red damage silhouette; bottom-right GUNS (`SLUG`) and MPH; a small
centre reticle; red hostile markers `Name 7 o'clock`. No pixel comparison against the captures
was made.

## What changed

* The developer overlay (banner, telemetry block, key help, HOSTILES list and brackets, the
  `OBJECTIVE - DESIGNED LAYOUT` line, the weapon refusal notice) is hidden in mission mode and
  returns with `--cs-path <dir> --mission M01 --dev-overlay`. The playtest keeps it.
* `mission_session::cockpit_hud` draws the gauges inside the largest 4:3 rectangle of the window.
  Rockets, rounds, damage come from the weapon/ordnance/damage authorities (the F46-B gathers);
  altitude, speed and heading from the telemetry. Nothing is a constant.
* Hostiles are red; on screen the bare name, off screen an edge arrow plus `Name N o'clock`
  (hour 12 ahead, 3 right, from the camera heading).
* The smoke's `hud_objective_matches` now requires the designed objective line to be *hidden*
  when the overlay is hidden.

## Unknown (recorded, not guessed)

1. Fonts, pixel positions, ring sizes, colours (green is designed), needle/ring fill ranges.
2. The ALT unit (feet used) and whether MPH is air or ground speed (telemetry speed used).
3. What `/ 2000` beside the guns count means; only the live selected-bank count is shown.
4. The compass tick spacing and window (15 degrees, 90 degree span used) and the silhouette shape.
5. The damage mapping: damaged node counts half, destroyed whole, unknown ignored (designed).
6. Off-screen marker placement: the original's arrow geometry and the circle with the target
   image are unmeasured; markers stack at the screen edge, level.

## Not done

* **The blue objective marker** (`[Examine] - Site #1`, off-screen circle with image, arrow and
  clock bearing). The recovered lifecycle model carries no objective world position, so no
  marker can be placed without guessing. Needs the objective-position source identified.
* Pixel comparison with `c05.png`/`c14.png` and the owner's visual approval.
