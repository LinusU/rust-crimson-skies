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

## What the retail run showed: the crash never starts in M01

Run on this branch with `CS_GAME_DIR` set, `--smoke-seconds 60`, exit 1, 1 of 8 checks passing
(`smoke_shots_delivered`). The dive worked as a placement: `report.json` `crash.track` shows the player
descending from 400 m to about 1 m over the sea in about 4.5 s and bouncing back up to 370 m, three times in
the 60 s watched (`contacts_judged` 0, no `CrashSequence`).

The cause was found by logging inside `judge_terrain_contacts` (the logging is not committed): the aircraft
**is** in contact, every 2-3 ticks while it skims, with one static body at `Position(0,0,0)` that carries
`WorldColliderInstance { role: Solid }` and `WorldVisual` — the world's own mesh collider. `judge_terrain_contacts`
only counts a body with `PlaytestGround`, `PlaytestSea` or `PlaytestAreaBody` as terrain
(`playtest/fatal_contact.rs`, `TerrainFilter`), and the composed mission's world collider carries none of them.
So in the real M01 composition a terrain contact is answered by the contact-response law (the bounce) but never
judged: no damage, no wing zone, no sparks, no wing fire, no `Fatal Crash!`, no camera.

The same holds for the jungle plateau the task names: it is part of the same world collider, so flying into it
produces the same silent bounce. The `accept_crash_b`/`c`/`d` tests pass because they lay a `PlaytestGround`
slab under the aircraft.

## Unmet

* The crash frames (contact sparks and fire, detached camera, `Fatal Crash!`, Mission Failed) were **not
  photographed**: the sequence never starts. The two frames that exist in the capture directory are the
  placement's first-control frames, not crash frames.
* No Drydock crash frames are available on this machine (`private/ref-drydock/m01` holds only `orig25.png`,
  `orig70.png`), so nothing could be put beside them either way.
* Resolving task: judge `WorldColliderInstance` `Solid` contacts as terrain in `fatal_contact.rs`. It changes
  M01's no-input flight (the sea skim at t=55 would now do damage), so the pinned
  `accept_parity4_m01_collision_` trajectory moves with it; the destruction threshold stays the designed one.
