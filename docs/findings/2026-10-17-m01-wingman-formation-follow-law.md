# M01-WINGMAN-FORMATION-B (#1564): the measured follow law in `cs_app::mission_ai`

Date: 2026-10-17. Task #1564. Implementer: claude-2. Capabilities used: `retail`
(local runs of the campaign test). Nothing here is `verified_original`.

## What changed

`mission_ai` no longer flies the designed `STATION_OFFSET_M` (45, 18, 75) and
`WINGMAN_THROTTLE_GAIN`. A `Wingman` now follows the law recovered in
`2026-10-17-m01-wingman-formation-control-law.md` (#1563): slot `(8, -2, 8)`
(astern is `+Z` here) in the leader frame; `follow_demanded_speed` =
`|V| + 0.15·along + 0.025·cross`, banded to `|V| ± 26.8224`, capped at
`1.5 × fd_speed`, floored at 22.352; `follow_throttle_target` slews 0.35 per
second (inside 2000 m of the player) or jumps to `demand / fd_speed` (beyond),
clamped to `[0.4, 1.5]`; the command carries `min(1, target)`. Constants are
pinned to the committed `[[ai_follow]]` rows by
`accept_m01_wingman_formation_b_constants_match_the_committed_rows`. Stored
`aiv.zrd` poses are untouched.

## Measured on retail (headless)

`accept_m01_play_ai_hostiles_close_and_betty_holds_station_within_n_seconds`:
Betty's gap to `devastator_2` goes 270 m -> 53 m (closest 43 m) over the run.

## Still designed or unknown

* Cap by `vehicle+0x668` is read as the airframe's `fd_speed` (designed).
* The frame scalar `0x9ad744` is read as seconds (designed).
* Stick axes keep the designed `pursuit_command` toward the slot; the original's
  thresholds `P[2]`, `P[3]`, `P[6]` and axis mapping are undecoded.
* Not applied: the altitude clamp, `def+0x1e4/0x1e8` speed words, the 80 m push
  (direction unrecovered), the route-following states, the player-leader slot.
* The slot's lateral sign and the up sign of basis row 1 are unmeasured.

## Not done

The windowed picture ("wingmen visible near the player", census lines
`mission wingmen t+Ns`) was not run or seen here: it needs the owner's
`human_play`. Betty's stored 733 m / 1037 m start offsets from the *player* are
unchanged, and she follows `devastator_2`, not the player.
