# #1856 PARITY4-CRASH-D: integration of D.01-D.03 with the crash smoke

Date: 2026-10-23. Implementer: `sonnet-2`. Nothing here is `verified_original`; a scripted run is never
`human_play`.

## Tests

`cargo test --locked -p cs_app -- accept_crash_d_ --include-ignored` with `CS_GAME_DIR` and `CS_ENGINE_IMAGE`
set: 13 passed, 0 failed (contact fx and wing-zone routing, the red pair on the dial, the retail spark
emitters and switch-offs, the pdp3 wing fire on retail M01).

## Owner-command smoke

`cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 60 --smoke-crash --capture-dir private/evidence/PARITY4-CRASH-D`
on main at e99d52fd7: exit 0, 8 of 8 checks, 16 framebuffers. The dive was placed at frame 2471 and the
production contact rule ended the flight at frame 2777 (tick 5549) at (-1510.9, 2.7, -1913.5).
`crash_contact_effects` passed (at most 1395 contact sparks and 319 wing-fire puffs alive in one frame).

Frames read by eye (kept in `private/`, not committed):

* `frame_cr02782_crash_contact.png`: the aircraft from above with a dark smoke trail, orange fire puffs and
  yellow sparks beside the right wing, and `Fatal Crash!` at the left edge.
* `frame_cr02837_crash_camera.png`, `frame_cr02888_failed.png`: the detached camera and the Mission Failed screen.

## Unmet

* The damage dial is not in the crash frames, so the **red wing zone is shown only by the test**
  (`accept_crash_d_01_destroyed_wing_zone_draws_the_red_pair_on_the_dial`), not by a smoke frame.
* The crash is into the sea, not the jungle plateau; no plateau coordinates are measured.
* No original crash frames exist on this machine, so nothing is put beside an original.
* Whether the smoke, fire and spark look match the original is not established.
