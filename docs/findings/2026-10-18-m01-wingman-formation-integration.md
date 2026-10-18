# M01-WINGMAN-FORMATION: the C3/M01 wingman is produced, bound to the player, and stays in formation

Rally task #1550. Integrates the recovered player-led wingman path into the
runtime so Betty (`wingman_1`) on the real campaign M01 — `ZBD/C3/M01` "The
Lost Treasure", corrected by #1567 — starts near the player and remains inside
the no-teleport smoke's designed 500 m bound instead of falling 700–1000 m
behind.

This note records what was changed, what the windowed smoke measured, and —
explicitly — what is still designed rather than measured.

## 1. What the original does (all `static` provenance)

The earlier findings hold the disassembly detail; the integrated facts are:

- `wingman_1`'s spawn dispatch is a special case: its `vehicle.zrd` template is
  the record named by the **player airframe row's `w` pointer** (`0x426d60`),
  not a name prefix of its own. On the Devastator row M01 flies that is
  `wingman`; `wkestrel`, `wfury`, … on the other rows
  (`docs/findings/2026-10-18-m01-wingman-player-leader.md` §1).
- The follower's leader is resolved **by identity**: `aiv.zrd` field 6 stores
  the leader's name; after the spawn loop, `0x475347`..`0x475371` binds each
  name to a vehicle and the follower tests `leader == [0x71c298]` — the global
  player vehicle — to select the player branch (`0x41e78a`). Betty's stored
  primary target is the literal `player`.
- The player branch runs a `+0xd8` sub-state: the wingman holds its unforced
  state until the player is awake, the squared gap is under `follow_engage_dist2`
  (490 000 = 700 m) **and** the follower's own speed exceeds
  `follow_engage_speed` (20.576 m/s) — then it transitions to the station law
  (finding §2, rows `follow_engage_*`).
- The player-leader station is a different vector from the non-player one:
  `(6, 0, 18)` at `0x61fb88` vs `(8, -2, -8)` for a scripted leader (rows
  `station_offset_player_leader_*` / `station_offset_other_leader_*`).

## 2. What the runtime now does

- `MissionFlight::wingman_record` carries the row's `w` record off the same
  committed `airframe` table that supplies the player's own `p` record, so the
  wingman's template can never disagree with the player's airframe.
- `MissionAiAirframes::prepare` imports that record alongside the generic
  name-prefix dispatches; `StageBinding` carries it across spawn and restart
  (`wingman_one_record`) so a restart does not re-read the world or lose the
  resolution. A `WingmanOne` dispatch with no supplied resolution is still a
  named refusal, never a guess.
- `decide_mission_aircraft_ai` builds a second cast entry for the player body
  and resolves the leader name `player` to that ECS entity; `ai.leader` keeps
  the resolved entity, and `player_leader` is decided by identity — exactly
  the original's `leader == [0x71c298]` shape.
- A player-led wingman latches `follow_engaged` once the gate fires (player
  awake is implicit: the body exists and is flown), then flies the
  `(6, 0, -18)` station — `-Z` being this crate's nose — through the measured
  demanded-speed law and throttle slew already used for scripted leaders.
- Scripted leaders (`wingman_2 → devastator_2` on C1C/M01 and elsewhere) keep
  the non-player path: no engage gate, `(8, -2, 8)` station.

## 3. What the windowed smoke measured

Owner command, run on this machine (Apple M3 Pro, Metal) with
`retail,gpu,audio`:

```sh
cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 45 \
  --smoke-no-teleport --capture-dir private/evidence/M01-WINGMAN-FORMATION
```

`report.json` (`mode: no_teleport`, `player_teleported: false`,
`placements: []`, `human_play: false`) samples Betty's distance to the player:

| t | distance |
| - | -------- |
| +1 s  | 163 m |
| +5 s  | 178 m |
| +10 s | 128 m |
| +20 s | 32 m  |
| +30 s | 40 m  |

`wingmen_near_player` **passed** — every sample inside the designed 500 m —
with `smoke_shots_delivered` green and the framebuffer PNG captured. Betty
spawns ~120 m behind the player (stored offset `(48, 0, 107)`), holds her
spawn heading while the gate is open (speed builds past 20.576 m/s), then
converges on the `(6, 0, -18)` station: ~40 m off the player at t+30 s against
a ~19 m ideal slot — the residual is the follow law's own tracking, not a
missed station.

The full machine-generated report is `docs/findings/evidence/M01-WINGMAN-FORMATION.json`
(claim `implemented` — this is our build's own run, never the original).

## 4. Honestly unmeasured / designed

- The original's **initial `+0xd8` state** and its **route-object goal** for
  the unengaged wingman are unmeasured (player-leader finding §5): until the
  gate fires, the designed stand-in holds the spawn heading. Betty spawns
  ~120 m behind with the player's own heading, so the hold is also the most
  conservative reading.
- The **signs** of the player slot's basis rows are unmeasured (finding §2/§5);
  `+x` right, `+y` up, `-z` ahead are the readings this crate keeps,
  consistent with the scripted-leader slot.
- The **500 m** threshold is the smoke's designed judgement, not an original
  constant.
- Whether `0x4a3360`'s name compare ignores case is unmeasured; the runtime
  compares exactly.
- The death/replacement link path (`0x4a6610`, `0x47ff4e`..) is read but not
  exercised — the player lives for the whole M01 smoke.
- `WingmanFour`'s measured alternative (the player's own airframe on c3/m05,
  c4/m04) is recorded in the code comment but not built; it stays a named
  refusal.

## 5. Sources

- `docs/findings/2026-10-17-m01-wingman-formation-control-law.md`,
  `-follow-law.md` (non-player branch, upstream tasks #1563/#1564).
- `docs/findings/2026-10-18-m01-wingman-player-leader.md` (#1571: leader
  resolution, player slot, engage gate).
- `docs/findings/2026-10-18-m01-runtime-archive-repoint.md` (#1567: C3/M01 is
  the real M01).
- `docs/findings/2026-10-18-m01-smoke-no-teleport.md` (#1572: the check and
  its samples).
- `data/owner-static/engine-facts.toml` rows `airframe[5].w`,
  `station_offset_player_leader_*`, `follow_engage_dist2`,
  `follow_engage_speed` — all `static` provenance.
