# FLAK-C5-SMOKE (#1887): owner-command smoke with a woken flak turret firing `wep_27`

Date: 2026-10-22. Follow-up of `2026-10-22-flak-b-turret-runtime.md`,
`2026-10-22-flak-c1-burst-presentation-and-descriptor-flag.md`,
`2026-10-22-flak-c4-flak-sounds.md`. Capabilities: `retail,gpu,audio` — the
windowed run is this machine's own frames of the reimplementation; no original
executable ran, so nothing here is `verified_original`. The test wake, the
placement and the presentation gating are designed.

## What the phase does

The owner-command smoke (`cs --cs-path <retail> --mission M01 --smoke-seconds
45 --capture-dir <dir>`) gained a `Phase::Flak` between the player's rocket
and the help/pause/cockpit phases:

1. It walks the bound `TURRET` table for the first world entry whose `WEAPON`
   the runtime classified as a `ROCKET` — in retail M01 that is `aagun99`, a
   `wep_27` flak gun which starts asleep and which **no M01 program wakes**
   (FLAK-B1).
2. It **test-wakes** it through `TurretWakeLedger::wake_for_test`, which calls
   the ledger's own `wake` matcher with `DirectiveOperation::WakeTurrets` —
   the same bookkeeping a `WAKEUP_TURRETS` emission produces (`wakes()` and
   `refused()` record it exactly like a mission-program wake).
3. It makes a **test placement** of the player at 0.6 × the record's
   `DETECTION_RANGE` (200 m → 120 m horizontal, 0.3 × that vertical),
   heading at the mount, listed in `report.json` under `placements` as
   `flak turret aagun99 (test-woken, test placement)`. This is the script's
   own placement — not original behaviour.
4. The production path then runs untouched: the turret step mirrors the
   ledger's live byte, the update law acquires the player (hostile team, in
   range, line of sight), `take_shot` publishes a `FlakRound`, the round
   steps and bursts, the burst lands on `PendingFlakBursts`, the
   `flak_effect` presentation draws flash light + smoke ball + close-range
   framebuffer flicker, and `snd_enemyflak`/`snd_missile_flak` play through
   the audio session (FLAK-C4).
5. The phase waits for the first drawn burst, screenshots it
   (`frame_*_flak_burst.png`) while the flash/smoke/flicker live, then takes
   a tail frame (`frame_*_flak_burst_tail.png`) after the flash and flicker
   expire; `FLAK_BUDGET_FRAMES` (720) bounds the wait.

## The 2026-10-22 run (claude-1, Apple M3 Pro, Metal)

`report.json` `flak` object, verbatim:

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
failures, `private/evidence/FLAK-C5-SMOKE/report.json`):

| check | verdict |
| --- | --- |
| `flak_turret_woken` | pass — the test wake set `aagun99`'s live byte through the ledger's own matcher |
| `flak_turret_fired` | pass — 1 `wep_27` fired; the `wep_28` read refusals (that weapon names neither `CANNON` nor `ROCKET`) are recorded, not swallowed |
| `flak_burst_drawn` | pass — 1 burst presented: 1 flash light, 1 smoke ball, 1 trail undrawn (unmeasured object motion, FLAK-C3) |
| `flak_flicker_drawn` | pass — 1 close-range flicker started inside the stored 100 m gate |
| `flak_sounds_queued` | pass — 1 `snd_enemyflak` shot cue, 1 `snd_missile_flak` burst cue, no refusals |

Framebuffers (non-black, 2560×1440):
`private/evidence/FLAK-C5-SMOKE/frame_03032_flak_burst.png` (burst alive) and
`private/evidence/FLAK-C5-SMOKE/frame_03074_flak_burst_tail.png` (tail, 42
frames later). The run's `cargo-test.log` and `acceptance.json` (schema
`schemas/evidence.schema.json`, validated with
`tools/validate_evidence.py --require-pass`) sit beside them; a copy is
committed as `docs/findings/evidence/FLAK-C5-SMOKE.json`.

## Scope and limits

- The wake is the smoke's own: it exercises the production wake ledger and
  the production turret-update → fire → burst → present → audio chain, but
  it is not evidence the original mission wakes `aagun**` — it does not
  (FLAK-B1's M01 trace).
- The placement numbers (`FLAK_RANGE_FRACTION` 0.6, `FLAK_HEIGHT_FRACTION`
  0.3, stand-off 80..400 m) are designed stand-offs, labelled in the source
  with their rationale (inside `DETECTION_RANGE`, above the mount so a
  `SHOOT_UP_ONLY` record admits the approach and `PITCH` limits hold).
- `flak_sounds_queued` records itself not applicable when the stage measured
  no flak sounds — it does not silently pass.
- `aagun01/02/30..32` remain asleep: only the first matching world entry is
  woken. `8igun01`'s `wep_28` still refuses by name (no `CANNON`/`ROCKET`
  row) — unresolved by design.
- Which entry the phase picks is the table's first `wep_27` world turret; a
  mission that bound none fails `flak_turret_woken` by name.
