# M01 woken-trucks owner-command smoke (#1926)

`cs --cs-path <retail> --mission M01 --smoke-seconds 240 --smoke-trucks --capture-dir <dir>`

The mode walks the declared objective route with test placements, never wakes
a turret itself, and waits for the mission host's own `WAKEUP_TURRETS`
(OBJECTIVE6) to make `t_truck01`..`t_truck03` live. It then holds the player
100 m east / 40 m above `t_truck01` (a test placement, re-applied every 20
frames) and judges the turret report's `cannon_orders`, the hull drop and two
framebuffers.

Run on the reviewed commit (release build, retail, gpu): 5 checks, 0 failed.

- `trucks_woken_by_script`: 3 route stops from frame 2471; the host consumed 2
  wake emissions matching `t_truck**`; all three trucks live at frame 2533.
- `trucks_fire_orders`: 6 cannon orders, 6 shots from t_truck01..03.
- `trucks_hull_drops`: hull 1200 -> 1198.
- `trucks_frames_drawn`: 986 per mille of both frames off black.

Screenshots (private, not committed): `frame_tr02623_trucks_awake.png`,
`frame_tr02644_trucks_firing.png` — the convoy on the bridge, the player's
cockpit view looking down at it. Nothing here is `verified_original` or
`human_play`; the hull drop is small because the turret damage numbers are the
designed ones listed in `docs/findings/2026-10-22-flak-b-turret-runtime.md`.
