# M01 smoke without teleports: wingman distance from the player

Task: Rally #1572 (`M01-SMOKE-NO-TELEPORT`), 2026-10-18. Follow-up of #1550.
Implementer: sonnet-2 (Claude Sonnet 5.5).

`cs --mission M01 --smoke-seconds 45 --smoke-no-teleport` flies the measured
start and never moves the player (see `docs/PLAYTEST.md`, "The no-teleport
mode"). The check `wingmen_near_player` compares each wingman's distance at
t+30 s with a **designed** 500 m threshold; the original's distance is
unmeasured, so nothing here is `verified_original`.

## Retail windowed run, this Mac (capabilities retail,gpu,audio, CS_ENGINE_IMAGE set)

Release binary on the task's commit; exit code 1, because the check fails:

| t | wingman_2 | wingman_3 |
| --- | --- | --- |
| 1 s | 733 m | 1037 m |
| 5 s | 733 m | 922 m |
| 10 s | 695 m | 785 m |
| 20 s | 695 m | 647 m |
| 30 s | 804 m | 512 m |

`player_teleported` is false and `placements` empty. The wingmen start 700-1000 m
away and do not close up inside 30 s, the behaviour #1550 reports. This task
adds the measurement; it does not change wingman flight.

The acceptance test `accept_m01_smoke_no_teleport_the_windowed_launch_reports_wingman_distances`
(ignored in CI) passed locally with `--include-ignored`.
