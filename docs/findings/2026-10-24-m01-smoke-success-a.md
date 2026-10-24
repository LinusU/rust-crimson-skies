# `--smoke-success` part A: the shipwreck proximity beat (#2081)

Date: 2026-10-24. Capability: `retail`, `gpu`. A scripted run; not `human_play`, nothing
`verified_original`. Tests: `accept_m01_smoke_success_a_` in
`crates/cs_app/tests/campaign/accept_m01_smoke_success_a.rs` (pure judging and flag parsing).

Retargeted from the C1C engine route (`2026-10-24-m01-smoke-success-a-wrong-archive-premise.md`)
to the C3/M01 census (`2026-10-24-m01-c3-win-route-census.md`): C3/M01 has no engine parts.

## Run

`cs --cs-path <retail> --mission M01 --smoke-seconds 120 --smoke-success --capture-dir <dir>`:
4 checks, 0 failed, exit 0, 2596 frames.

| Objective (block index) | What | Completed (script frame) |
| --- | --- | --- |
| OBJECTIVE12 (11) | `TRAVELERS` player -> `shipwreck` <= 200 m; the player was placed 120 m away at frame 2471 | 2473 |
| OBJECTIVE27 (26) | woken by 12, naps 2 s, then marks the site | 2593 |
| OBJECTIVE10 (9), OBJECTIVE1 (0) | no condition (OBJECTIVE1 self-wakes at 2 s) | 2471, 2531 |

Not completed in this run, as expected: OBJECTIVE21 (needs the `texdrop` landing), and
everything after it. The site node was found as a `MissionGroundTarget` named `shipwreck`
at (-8891, 5, -4685) m. Both framebuffers (`success_near`, `success_marked`) are 999 per
mille off black and show the wreck ahead of the player's plane.

## Still unknown (owned by #2082)

The `do_approach1..6` landing nodes compose no position, and nothing is known to start
`hydrogen_fire` (see the census, section 4). This stage does not touch either.
