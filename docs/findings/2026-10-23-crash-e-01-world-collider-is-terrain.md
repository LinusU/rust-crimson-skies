# Crash E.01: the world's own collider counts as terrain in the fatal contact rule (#1983)

Date: 2026-10-23. Implementer: `Devin SWE-2/swe2-max-1`. Evidence class: headless
compositions of M01 (`build_headless`, the owner-command's own mission host) on
this branch with `CS_GAME_DIR` set. Nothing here is `verified_original`.

## The gap this closes

Logging inside `judge_terrain_contacts` during #1857 (finding
`2026-10-23-crash-e-owner-command-smoke.md`, branch
`rally/1857-crash-flow-5-5-owner-command-smoke-phase`) showed that in the
composed M01 mission the player's contact partner is the imported world's own
mesh collider — one static body at `Position(0,0,0)` carrying
`WorldColliderInstance { role: Solid }` + `WorldVisual` — which the judge's
`TerrainFilter` (`PlaytestGround` | `PlaytestSea` | `PlaytestAreaBody`) did not
name. A flight into the island, the plateau or the sea was answered by the
contact-response law (the bounce) but never judged: no damage, no wing zone,
no sparks, no wing fire, no `Fatal Crash!`, no crash camera, no debrief.

## What changed

`TerrainFilter` now also names `With<WorldColliderInstance>`, and a world
collider counts only while its declared role is `WorldCollisionRole::Solid`
(`world::spawn` marks trigger volumes `Sensor`: they report an overlap and
never block, so flying through one can never be a fatal terrain contact).
Actors do not carry `WorldColliderInstance` and are untouched. The hull pool
stays the designed `DESIGNED_PLAYER_INTEGRITY` (1200) and the per-contact rule
stays the retail `crash` `health_damage_range` — `max(range[1] * s^3, range[0])`,
doubled for the player.

## Which contacts now do damage

Measured on this branch (temporary instrumentation of the judge; the pinned
test's `judged`/`hull` columns carry the same numbers permanently):

* The **no-input M01 window** of
  `accept_parity4_m01_collision_no_input_trajectory_is_pinned` now judges nine
  contact episodes for 1049 of the 1200 hull: a world-mesh graze over the
  island's high ground at t≈50 (tick 6005, severity 0.32), three `PlaytestSea`
  tile contacts of the sea skim at t≈54.5..58.6 (only these three were judged
  before this change — 300 damage), and five world-collider contacts at
  t≈69.6..71.1 while the stopped aircraft pushes into the shore terrain
  (severities 0.41/0.71/0.05/0.00/0.61). The flight still survives and is
  never reset; the trajectory is unchanged because damage moves the health
  model, not the body — only a fatal contact stops the flight.
* The **120 s idle run** of
  `accept_m01_outcome_a_an_idle_player_never_wakes_the_kestrels`
  (`m01_hostile_engage.rs`, M01-OUTCOME-A's file — not this task's) pins
  "nothing damaged the idle player's hull". That pin was already false on
  main: the sea-tile contacts alone left the hull at 600 of 1200 there
  (measured on the unmodified tree). On this branch the same drift judges the
  world-collider contacts too and the pool reaches 0 — the idle player now
  crashes. The pin belongs to that task's file and needs its own rewording
  task; the wake-chain claim the test exists for (the Kestrels stay asleep)
  still holds.
* `accept_crash_e1_a_dive_into_the_world_collider_is_judged_and_fatal`
  composes M01 and places the player in the dive the #1857 smoke makes,
  steeper (400 m over the sea, nose straight down, the original's ArrowUp
  nose-down key held). The
  contacts on the world collider are judged and the pool empties: a
  `PlayerCrashed` is written whose `FailureCause::TerrainContact` names the
  `WorldColliderInstance { role: Solid }` body, and `CrashSequence` begins.
  Without the filter change the same placement produces zero judged contacts
  and no crash in 60 s (verified by reverting `TerrainFilter`: the test
  fails with "never judged a terrain contact after 3600 frames").

## Still unmeasured

* **Contact damage vs. the original is uncompared.** The original survived
  both measured contacts of the #1752 run; whether it would call any of the
  nine contacts above — or the dive — fatal is unknown, because the original's
  per-zone pools are unknown (`2026-10-22-crash-b-fatal-terrain-contact.md`,
  "Unknown"). The designed 1200 and the retail range stand until that is
  measured. If the owner must rule on a destruction threshold the task for it
  is a follow-up, not this one.
* The near-zero severities (0.00..0.05) that still draw the range's floor
  damage are the same designed floor rule as before; the world collider only
  made more of them reachable.
