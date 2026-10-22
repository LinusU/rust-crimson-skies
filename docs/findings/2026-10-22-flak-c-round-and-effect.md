# FLAK-C-FLAK-ROUND-AND-EFFECT (#1821): the split's integration step

Date: 2026-10-22. Follow-up of
`2026-10-22-flak-c1-burst-presentation-and-descriptor-flag.md`,
`2026-10-22-flak-c4-flak-sounds.md` and
`2026-10-22-flak-c5-owner-command-smoke.md`. Capabilities: `retail,gpu,audio`
— the windowed run is this machine's own frames of the reimplementation; no
original executable ran, so nothing here is `verified_original`. The test
wake, the placement and the presentation gating are designed.

This is the integration step of the five-part split, not a re-run of it:
each subtask's tests (`accept_flak_c1_` … `accept_flak_c5_`) stay as they
landed, and the parent adds its own `accept_flak_c_` prefix covering the
chain end to end — a `wep_27` turret test-woken, the production turret law
firing `FlakRound`, the round's `OrdnanceBurst`, the `flak_effect`
presentation (flash, smoke ball, close-range flicker) and the
`snd_enemyflak` / `snd_missile_flak` cues.

## The four `accept_flak_c_` tests

`crates/cs_app/tests/accept_flak_c_round_and_effect.rs`, module of
`cs_app_tests_a`:

- `accept_flak_c_a_fired_rounds_burst_feeds_the_presentation_and_the_cues`
  (CI): a production `FlakRound` fired at a held target bursts inside
  `DETONATION_DISTANCE` of it; the same `OrdnanceBurst`
  `step_mission_turrets` publishes is pushed through `PendingFlakBursts`
  into `FlakBurstFxPlugin` (1 presented, 1 `FlakFlash`, 1 smoke-ball handoff
  to `PendingOrdnanceBursts`, 1 `FlakFlicker` inside the 100 m gate, 1
  undrawn trail) and through `play_flak_burst` — after `play_flak_shot`
  played `snd_enemyflak` at the muzzle.
- `accept_flak_c_a_silent_end_publishes_nothing` (CI, negative): a record
  with `LOCK_ON` + `EXPIRES` and no `DETONATE_AT_RANGE` ends
  `FlakStep::SilentEnd` with no held target, so no burst is published and
  the presentation reports the empty default — an absent burst is never
  faked.
- `accept_flak_c_retail_a_test_woken_aagun_fires_bursts_presents_and_sounds`
  (`#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]`): real M01
  headless. `aagun99` — the first world turret whose `WEAPON` binds a
  `ROCKET` record — stays silent for 240 ticks while asleep; after the
  ledger's `wake_for_test` the production law acquires the pinned player,
  fires `wep_27`, the round bursts within `DETONATION_DISTANCE`, the
  `flak_effect` presentation counts the flash/smoke/flicker, and the turret
  report counts one shot cue and one burst cue with no refusals.
- `accept_flak_c_the_windowed_smoke_shows_the_burst_puff_and_plays_the_cues`
  (`#[ignore = "requires CS_GAME_DIR + gpu + audio and a built windowed
  binary"]`): runs the owner's own command
  `cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 45
  --capture-dir <dir>`, then checks `report.json`'s five flak checks and
  that the `flak_burst` / `flak_burst_tail` framebuffers arrived non-black.

## The 2026-10-22 run (swe2-max-1, Apple M3 Pro, Metal)

`cargo test --locked -p cs_app -- accept_flak_c_ --include-ignored`:
4 discovered, 4 executed, 4 passed, 0 failed (127 s; the windowed smoke
itself 45 s). Windowed `report.json` `flak` object, verbatim:

```json
{"turret": "aagun99", "test_wake": true, "test_placement": true,
 "detection_range_m": 200, "sound_measured": true, "rockets_fired": 1,
 "shots_logged": 1, "bursts": 1, "bursts_presented": 1, "flashes_spawned": 1,
 "smoke_balls": 1, "flickers_started": 1, "flickers_skipped": 0,
 "trails_undrawn": 1, "shot_cues": 1, "burst_cues": 1, "sound_refusals": [],
 "refusals": ["turret weapon wep_28: wep_28 has neither CANNON nor ROCKET",
              "8igun01: weapon wep_28 was not read (see the weapon refusals)"]}
```

All five flak checks passed, and the smoke passed overall (64 checks, 0
failures, `private/evidence/FLAK-C-FLAK-ROUND-AND-EFFECT/report.json`):

| check | verdict |
| --- | --- |
| `flak_turret_woken` | pass — the test wake set `aagun99`'s live byte through the ledger's own matcher |
| `flak_turret_fired` | pass — 1 `wep_27` fired; the `wep_28` read refusals are recorded, not swallowed |
| `flak_burst_drawn` | pass — 1 burst presented: 1 flash light, 1 smoke ball, 1 trail undrawn (unmeasured object motion) |
| `flak_flicker_drawn` | pass — 1 close-range flicker started inside the stored 100 m gate |
| `flak_sounds_queued` | pass — 1 `snd_enemyflak` shot cue, 1 `snd_missile_flak` burst cue, no refusals |

Framebuffers (non-black, 2560×1440):
`private/evidence/FLAK-C-FLAK-ROUND-AND-EFFECT/frame_03032_flak_burst.png`
(burst alive) and
`private/evidence/FLAK-C-FLAK-ROUND-AND-EFFECT/frame_03074_flak_burst_tail.png`
(tail, 42 frames later). The run's `cargo-test.log` and `acceptance.json`
(schema `schemas/evidence.schema.json`, validated with
`tools/validate_evidence.py --require-pass`) sit beside them; a copy is
committed as `docs/findings/evidence/FLAK-C-FLAK-ROUND-AND-EFFECT.json`.

## Scope and limits

- The wake is the smoke's own: it exercises the production wake ledger and
  the production turret-update → fire → burst → present → audio chain, but
  it is not evidence the original mission wakes `aagun*` — it does not
  (FLAK-B1's M01 trace).
- The placement (0.6 × `DETECTION_RANGE` horizontal, 0.3 × that vertical)
  is a designed stand-off, labelled in `report.json` under `placements`.
- `call_flak_trails` stays undrawn: its `OBJECT_MOTION` units and frame are
  unmeasured (FLAK-C1/C3); the report counts it under `trails_undrawn`.
- The burst's splash applies no damage: its falloff and the entity+0x678
  factor are unmeasured (FLAK-C2).
- `8igun01`'s `wep_28` still refuses by name (no `CANNON`/`ROCKET` row) —
  unresolved by design.
- The negative test guards the honest path: a record whose range end is
  silent publishes no burst, so no shortcut can always draw or sound one.
