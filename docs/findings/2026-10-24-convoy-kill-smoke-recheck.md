# M01 convoy-kill smoke re-check (#2064)

#2064 reported `cs --smoke-convoy-kill` failing on main: fire-key presses never
registered a destroyed truck. On main at 9cda71366 it passes.

The smoke used to judge kills from the schedule's one-tick `destroyed`
publication, which a later tick of the same frame overwrites. Commits 47efb6908
and 9cda71366 read the damage resolver's recorded lifecycle instead. (Cause
taken from the commit history, not re-bisected.)

Run (release, retail, gpu), twice:
`cs --cs-path <retail> --mission M01 --smoke-seconds 60 --smoke-convoy-kill --capture-dir <dir>`

Both runs: exit 0, 5 checks, 0 failed.

- `convoy_destroyed_by_fire_key`: 78 guns-key presses; the damage authority
  listed `t_truck01`, `t_truck02`, `t_truck03`, `bridge_truck01` destroyed at
  frames 2527, 2551, 2575, 2599.
- `convoy_observation_follows`: all four rows `in_play` before the first shot,
  all four `no_longer_in_play` at the end.
- `convoy_layout_swaps`: passes.
- `convoy_frames_drawn`: 988 / 989 per mille off black.

Screenshots (private, not committed): `frame_ck02501_convoy_intact.png`,
`frame_ck02689_convoy_destroyed.png`.

Nothing here is `verified_original` or `human_play`.
