# M01-LC-FC-D: the owner-command smoke passes through the Flight Check (#1672)

Command (release build of the task commit, retail install, GPU and audio):

```sh
CS_CAPABILITIES=retail,gpu,audio target/release/cs --cs-path "$CS_GAME_DIR" \
  --mission M01 --smoke-seconds 45 --capture-dir private/evidence/M01-LC-FLIGHTCHECK
```

Result: exit 0, 18 checks, 0 failed (`world_sea_visible` skipped by name: no
declared water). The four new checks passed on the 2560x1440 framebuffer:
`flight_check_screen_drawn` (739 per mille off black), `flight_check_text_matches`
(all 8 rows; 1 031 to 19 891 added pixels each), `flight_check_buttons_drawn`
(all 6 buttons; 10 232 to 32 872 added pixels each) and `flight_check_fly_mission`.
The framebuffers (`frame_fc00020_flight_check.png`, `frame_fc00028_flight_check_bare.png`)
stay in `private/` (original artwork is never committed).

## What the first retail run found

The first run failed `flight_check_text_matches` with 0 added pixels on every
text row: the screen's text nodes were laid out and held the right strings but
were not drawn. The cover, the background pane and the overlay all carried
`GlobalZIndex(2000)`, and roots of one global index are stacked in no stated
order, so the pane covered the text. `spawn_screen` now layers them 2000/2001/2002.
Headless tests could not see this; the framebuffer comparison does.

## Still open

* The `Change Ammo` / `Change Plane` button labels are spawned but not visible
  on the pill art in the screenshot (the check counts the art's own pixels for
  buttons). Not judged here.
* Every value the data does not state (gun and rocket lists, the wingman's
  plane) is drawn as `unknown`, per M01-LC-FC-C. Nothing here is
  `verified_original`; the floors are designed.
