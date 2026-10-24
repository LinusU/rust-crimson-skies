# #1857 PARITY4-CRASH-E: the crash phase of the owner-command smoke

Date: 2026-10-23. Implementer: `sonnet-2`. Nothing here is `verified_original`; a scripted run is never
`human_play`.

## What is in the build

`cs --cs-path <retail> --mission M01 --smoke-seconds 60 --smoke-crash --capture-dir <dir>`
(`docs/PLAYTEST.md`, "The crash mode"). After the briefing, Flight Check and launch cinematic it places the
player 400 m above the sea ahead of it with the nose 45 degrees down (a test placement, in `report.json`
`placements`), holds the Up arrow, and waits for the production crash flow: a `CrashSequence`, the contact
frame, the detached camera frame with `Fatal Crash!`, a key press, the Mission Failed screen. The pure judging
(`smoke/crash.rs::evaluate`) is covered by `accept_crash_e_` (4 tests, `tests/campaign/accept_crash_e_smoke.rs`).

## What the retail run shows (after #1983 judged the world's solid colliders)

Run on the rebased branch (main at 72c928396) with `CS_GAME_DIR` and `CS_ENGINE_IMAGE` set:
`cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 60 --smoke-crash --capture-dir private/evidence/PARITY4-CRASH-E`,
exit 0, 8 of 8 checks passing, 16 framebuffers. The dive is placed at frame 2471 from (-1469.6, 400, -1864.9)
with Up held; the production terrain-contact rule judged 9 contacts and ended the flight at frame 2777 (tick
5549, severity 0.178) at (-1510.9, 2.7, -1913.5). The frames, read by eye:

* `frame_cr02782_crash_contact.png`: the aircraft seen from above with a smoke trail, orange fire puffs and
  yellow sparks beside the right wing, and the blue `Fatal Crash!` text at the left edge.
* `frame_cr02837_crash_camera.png`: the detached camera over the aircraft with `Fatal Crash!` still shown and
  the smoke drifting away.
* `frame_cr02888_failed.png`: the scrapbook debrief, `Mission Failed` / `Mission Results`, Run Time 0:46,
  Gun Hit Ratio 0%, Cash Earned unknown, Overall Planes Downed 0.

## Unmet

* The crash is into the **sea** near the start, not the jungle plateau the task names: the contact position is
  2.7 m above the origin of the sea, and both crash frames show water. Placing the dive over the plateau needs
  its measured coordinates; none are recorded, so none were guessed.
* No Drydock crash frames are on this machine (`private/ref-drydock/m01` holds only `orig25.png`,
  `orig70.png`), so no frame could be put beside an original. The owner's captures are needed for the
  comparison.
* The `Fatal Crash!` text sits at the left edge in blue; whether that is the original's place and colour is
  not established here.

## #2021 PARITY4-CRASH-F: the plateau dive

Implementer: `claude-2`. Nothing here is `verified_original`.

The stage now dives onto the owner's observed crash point `(-5055, 324, -4718)` (#1755; finding
`2026-10-22-crash-a-…` §5): it casts a ray down at that point, places the player 400 m above the surface
the ray found (the observed height when it found no scenery), 400 m back along the straight line from the
player's position to the point, nose 45 degrees down, Up held. The original's own heading is unknown, so
that approach is derived, not measured. New checks: `crash_on_plateau` (the fatal contact's terrain is
world scenery, not `PlaytestSea`/`PlaytestGround`) and `crash_dial_red_pair`.

**Run** (`private/evidence/PARITY4-CRASH-F`, `--smoke-seconds 60 --smoke-crash`): 9 of 10 checks pass,
`crash_on_plateau` fails, on purpose:

* The ray from 4000 m above the point hits the **sea at y = 0**, no scenery. Rays on `z = -4900, -4718,
  -4500, -4300` for `x = -7000 … -3500` (step 100) all hit the sea first. Scenery colliders exist elsewhere
  (e.g. `x ≈ -7400 … -3000`, `z ≈ -6800 … -5200` and `z ≈ -4000 … -3000`, tops 50–460 m), none at the
  observed point. So **the surface at the owner's crash point cannot be confirmed from the world's colliders: unknown.**
* The drawn world there is jungle hills (`frame_cr02491_crash_dial.png` shows them under the dive start
  at `(-4742, 724, -4469)`), yet the production flight met the sea: the dive ended at frame 4007 at
  `(-4647, 2.2, -4392)`, `contact_terrain: sea`. Either the drawn hills have no collider there or the
  observed point is in another frame; both are unresolved. I did not move the dive to the nearest scenery:
  that would be an invented coordinate.
* **The dial frame**: the production crash flow holds the gauge cluster back from the first crash frame
  (`fit_hud_viewport`, `crash.is_some()`), so no frame can show the dial red *at* contact. The stage
  scripts one wing hit (`leftwingdamage`, through the damage authority, labelled in `report.json`) at the
  dive and photographs `frame_cr02491_crash_dial.png` 20 frames later. The check reads the production zone
  pieces: 4 of 4 left-wing pieces carry the red texture, none of the other zones. On the frame, though, the
  zone overlay is barely visible: a thin red sliver on the left rim of the attitude dial, not a wing-shaped
  zone over the silhouette. The overlay quads' placement on the dial looks wrong; follow-up task filed.
* The scripted wing hit also starts the wing fire before the crash, so `crash_contact_effects` (sparks and
  puffs alive after the crash began) is weaker here than in the #1857 run; the sparks come only from the crash.

### Update after #1814 and #2032 (2026-10-24)

With the grouped-scenery colliders in, the same dive (from `(-4742, 721, -4469)`) meets scenery: the ray under
the observed point finds scenery at y = 321.3 (the owner's observed height is 324), and the fatal contact is
`scenery`, 291 m horizontally from the observed point at `(-4808, 205, -4563)`. `crash_on_plateau` now passes
without any invented coordinate, which resolves the unknown above. Retail run on this machine (claude-2, 2026-10-24, display live; `--capture-dir private/cap2021d`):
**10 of 10 checks pass**, including `crash_dial_red_pair` (4 of 4 red pieces, 995 per mille off black),
`crash_frames_drawn` (939 / 939 per mille) and `crash_failed_screen_shown` (744 per mille). An earlier run of this
branch on the same host failed three checks only because every capture was black (display asleep); the off-black
floor was not changed. Frames cited (private, not committed): `frame_cr02491_crash_dial.png` (jungle hills under
the dive, left-wing zone red on the dial, but only a thin red sliver on the attitude dial's left rim is visible,
#2031), `frame_cr03841_crash_contact.png` (plane tumbling into the jungle slope, `Fatal Crash!` in blue default
text at the left edge), `frame_cr03896_crash_camera.png` (detached crash camera) and `frame_cr03947_failed.png`
(Mission Failed). Comparison with the original's crash frames and whether the `Fatal Crash!` text style matches stay
with the owner.
