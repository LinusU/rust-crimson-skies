# M01 convoy-kill owner-command smoke (#2047)

`cs --cs-path <retail> --mission M01 --smoke-seconds 240 --smoke-convoy-kill --capture-dir <dir>`

The mode waits for control, then places the player 80 m from the first truck
still standing (a test placement, re-applied every 6 frames, labelled in
`report.json`), photographs the intact convoy, and presses the guns key
(`FirePrimary` through the production cockpit-action queue) every frame. It
never writes damage and never wakes a turret. When the damage authority lists
all four trucks (`t_truck01..03`, `bridge_truck01`) destroyed it places the
player on a vantage 160 m over the convoy, waits 90 frames and photographs it.
The observation rows are read through the production writer
(`mission_world_observation`) because the mission host consumes the port inside
the tick that wrote it.

Run on the implementing commit (release build, retail, gpu): 4 checks, 0 failed.

- `convoy_destroyed_by_fire_key`: 77 guns-key presses; destroyed at frames
  2526, 2550, 2574, 2598 per the damage authority.
- `convoy_observation_follows`: all four rows `in_play` before the first shot,
  all four `no_longer_in_play` at the end.
- `convoy_frames_drawn`: 988 / 989 per mille off black.

Screenshots (private, not committed): `frame_ck02501_convoy_intact.png`,
`frame_ck02688_convoy_destroyed.png`.

## What the screenshots show (limitation)

The "destroyed" frame looks the same as the intact one: the truck, the tanker
and the three gun carriers are still drawn whole on the bridge, and no wreck or
debris is visible, although the damage authority and the observation say every
truck is destroyed (the player's slug count only fell 2000 -> 1990 in the frame
pair). So the authored `destroyed` layout (`mission_targets` reads the
`healthy`/`destroyed` children) or a debris wreck is **not presented** for M01's
ground targets in the windowed run. This is not judged by the smoke (pixels
cannot say it purely); it is recorded here and filed as a follow-up. Nothing
here is `verified_original` or `human_play`.
