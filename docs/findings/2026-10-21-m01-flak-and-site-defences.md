# M01-FLAK-AND-SITE-DEFENCES (#1741): which M01 defences fire at the player, and what the data says about the island's flak

Date: 2026-10-21. Task #1741 (`PARITY3-FLAK-AND-SITE-DEFENCES`). Capabilities used:
`retail` (read-only `$CS_GAME_DIR`) through the production readers
(`read_scope_turrets`, `bind_mission_targets`, `decode_zrd`), and a static read of
`$CS_ENGINE_IMAGE` (VAs below; nothing of the image is committed). Archive:
**`ZBD/C3/M01`** (see `2026-10-17-m01-archive-decision.md`). No original executable
ran; nothing here is `verified_original`. Scratch probes only; nothing committed
but this document.

## Verdict

1. **The data-driven defences of M01 are the three convoy gun trucks
   (`t_truck01..03`) and the Pandora's own turrets.** The island's anti-aircraft
   guns (`aagun01`, `aagun02` on the island, `aagun30..32` at the upper camp) are
   **dormant** in M01: their turret record states `ACTIVATED 0`, the engine turns
   that into a clear *live* byte, and **no M01 program wakes them** (below). They
   *are* woken in the next mission archive, `ZBD/C3/M02`.
2. **So retail data does not explain flak at the island in an unthreatened M01.**
   The owner capture's puffs and the damage that follows are recorded as an
   **unknown source** (rule 4), not produced by waking guns the original leaves
   asleep. Candidates and what would settle them are in "Unknowns".
3. **Our mission has no turret runtime at all.** Nothing consumes the `TURRET`
   table to fire at the player (`parts.rs` only classes zeppelin part nodes as
   weapon mounts; the hostile list is the zeppelins and Kestrels). What M01 *does*
   wake — the convoy trucks at objective 6 — therefore cannot fire either. That
   gap is real and is split out below.

## Measurements

### The turret entry and its live byte (engine image, `static`)

* A turret registry entry's **live byte is `entry+0x6e`**. The turret update
  (`0x4aabb0`) returns at once while it is clear (`0x4aac16` `mov al,[esi+0x6e]` →
  `0x4aac1b` `je 0x4ab53b`).
* The `TURRET` record parser sets it from the record: `0x4aa79e` pushes the string
  at `0x629d54` (`ACTIVATED`), finds the child, and `0x4aa7b0`..`0x4aa7bb` stores
  `first-child-int != 0` into `entry+0x6e`. The constructor's default is 1
  (`0x4a9b14`). A record without `ACTIVATED` therefore starts live; `ACTIVATED [0]`
  starts asleep.
* Whole-image census of byte stores into a `+0x6e` slot with a constant or the
  parsed flag: `0x4a9812` (the `WAKEUP_TURRETS` name matcher, measured in
  `2026-10-06-m01-lc-directive-c-…`), `0x4bef3b` (the zeppelin-turret recursion
  behind `WAKEUP_ZEP_TURRETS` / `0x4bef70`, whose own callers are `0x469c29` and the
  zeppelin start/stop at `0x45b8f1`, `0x45b943`, `0x4bf0e3`), the parser `0x4aa7bb`,
  the constructor `0x4a9b14` and the save-state restore `0x4ac706`. **Nothing else
  wakes a turret.**

### What the `ai.zrd` `TURRET` table says about the guns (`static`, `zbd/zrdr.zbd`)

| Record | `NODES` | `ACTIVATED` | `HEALTH` | weapon | `FIRE_RATE` (s) | `DETECTION_RANGE` (m) | `INACCURACY` | `ATTACK_INTERVAL` / `BORED_INTERVAL` (s) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 16 | `aagun**` | 0 | 8 | `wep_27` | 1 – 1.8 | 500 | 5 | 2–4 / 3–5 |
| 17 | `maagun**` | 0 | 2 | `wep_30` | 1 – 1.8 | 500 | 5 | 2–4 / 3–5 |
| 20 | `t_truck**` | 0 | 2 | `wep_29` | 1 – 1.8 | **750** | 5 | 2–4 / 3–5 (`PITCH` −10…60) |
| 21 | `8igun**` | 0 | 8 | `wep_28` | 8 – 12 | 500 | 5 | 30 / 5 |
| 27 | `workersvoyagezep`/`cargozep1..3` `maingun*` | 0 | — | `wep_27` | 6 – 8.5 | 600 | 4.5 | 4–6 / 4–5 (`PITCH` −22.5…85, `YAW` ±90) |
| 37 | `piratezep` `doublecannon*` | 0 | — | `wep_27` | 3 – 3.5 | 600 | 6 | 6–8.5 / 4–5 (`TEAM 1`, `PITCH` −8…50, `YAW` ±90) |

`TEAM` is absent from records 16, 17, 20, 21 and 27; the loader's resolver defaults it to 2
(`2026-10-18-m01-target-roster.md`). The Pandora's records (37–40) state `TEAM 1`, the
player's side, so her woken turrets are the player's, not hostile. Whether the turret fires at *the player* is
decided in the update's target step (`0x4ab0a0`..`0x4ab23c`), which is **not
traced** here.

### `wep_27` is the flak round (`static`, `zbd/zrdr.zbd` `weapons.zrd`)

`wep_27` = `NAME FLAK`, `DESC MSG_WEAP_AAFLAK_ROCKET`, a rocket: `VELOCITY 850`,
`ACCELERATION 150`, `RANGE 900`, `DETONATION_DISTANCE 20`, `IMPACT_PROXIMITY 22`,
`LOCK_ON 3`, `LOCK_ON_LEAD [5 10]`, `ARMOR_DAMAGE 10`, `HEALTH_DAMAGE 10`,
`CLUSTER_SIZE 1`, fire effect `small_fireball` + sound `snd_enemyflak`, flyout model
`aaflak`. Its `IMPACT` rows: `default` → animation **`flak_effect`**, sound
`snd_missile_flak`; `player` → `f18sparks2` + `rcochet1`; `water` → `bsplsh.flt`.

`flak_effect` (`ZBD/zrdr.zbd` `flak_control.zrd`) is the picture the owner
describes: an `OBJECT_ACTIVE_STATE flak_explosion`, a **light flash**
(`flak_flashes`: colour `(0.95, 0.76, 0.24)` — orange — range 4→10 then
pulsing 16/40 over 0.3 s, off over 0.1 s), `call_flak_trails`
(`flak_trails.zrd`) and **`large_black_smokeball`** — the grey puff. Within
`PLAYER_RANGE 100` it also runs `frame_buffer_effects1`, a short white/black
full-screen flicker (`FBFX_COLOR_FROM_TO`, 0.2 s steps, alpha ≤ 0.5).

### Where the guns are (`bind_mission_targets`/scene graph, `ZBD/C3/gamez.zbd`)

Composed world positions (m, source axes):

| Node | Position | Woken by |
| --- | --- | --- |
| `aagun01`, `aagun02` | (−10938.3, 29.7, −11656.1), (−10543.7, 29.7, −11630.7) | **`C3/M02` `OBJECTIVE38`** — not M01 |
| `aagun30`, `aagun31`, `aagun32` | (−12599.6, 274.7, −9704.1), (−12803.5, 273.8, −9929.9), (−12833.1, 273.8, −9940.0) | **`C3/M02` `OBJECTIVE39`** — not M01 |
| `aagun98`, `aagun99`, `8igun01` | local translations near the origin (children of a template) | `C3/M03` |
| `t_truck01..03` | (−5459.3, 118.6, −5339.2), (−5461.5, 118.6, −5375.9), (−5461.4, 118.6, −5391.8) | **M01 `OBJECTIVE6`** |

The player starts at (−1426, 150, −1813); the island guns are ~13 km away and the
bridge ~5.4 km.

### Every turret wake in the retail missions (byte scan of `WAKEUP_TURRETS`)

`C3/M01`: `t_truck**` only (plus `WAKEUP_ZEP_TURRETS` for `piratezep` at
`OBJECTIVE1`, mission clock 2.0 s, and `cargozep1` at `OBJECTIVE39`). `C3/M02`:
`aagun01 aagun02` (`OBJECTIVE38`), `aagun30..32` (`OBJECTIVE39`), `b_turret*`
(`OBJECTIVE41`). `C3/M03`: `aagun98 aagun99 8igun01`. Other chapters wake
`aagun**`, `8igun**`, `tcargun**`, `bbtur*`, `maagun0*`, `thug*` the same way. The
convention is clear: **AA is woken by an objective**, and M01's program does not
wake the island's.

### M01's `OBJECTIVE6`

`OBJECTIVE5` (`TRAVELERS player APPROACHING (−5458, 120, −5390)` within **1100 m**,
`ADD_OBJECTIVE_TARGET` the three trucks and `bridge_truck01`) wakes `OBJECTIVE6`,
whose single effect is `WAKEUP_TURRETS ["t_truck**"]`. So the **gun trucks start
shooting once the player comes within 1100 m of the bridge**, then engage inside
their own 750 m detection range with `wep_29` (turret gun: `FIRE_RATE 8`,
`RANGE 1000`, `VELOCITY 450`, `HEALTH_DAMAGE 0.25`, `CANNON_SPREAD 6`, effect
`muzzle_burst2`; no flak).

## Unknowns, affected content and resolving work

| Unknown | Affected content | Resolves in |
| --- | --- | --- |
| What produced the grey puffs/orange flash and the damage in the owner capture | the capture's parity target; whether M01 has a flak source we have not found (cinematic `CALLBACK`s, `cargozep1`, an unread wake) | an owner run with a recorded input trace and the mission clock of the first puff; the nine `generic_intro` `CALLBACK`s (`2026-10-19-m01-launch-attackers.md`) |
| The turret update's target step: team test against the player, line-of-sight, pitch/yaw gating, the attack/bored state machine, how a `FIRE_RATE` pair is drawn and how `INACCURACY` aims the round | every fired round of a woken turret | static trace of `0x4aabb0`..`0x4ab53b` (task split below) |
| Whether a flak round bursts at `DETONATION_DISTANCE` of the target or at `IMPACT_PROXIMITY`, and which of `flak_effect`'s parts the engine calls at the burst point | where the puff appears relative to the aircraft | static trace of the rocket detonation path |
| `HEALTH` unit of a turret record (2 for the trucks, 8 for `aagun`) | hits to silence a gun | #668 / F27-E |

## Files

* `docs/findings/2026-10-21-m01-flak-and-site-defences.md` (this document).
* Probes were scratch tests (`read_scope_turrets`, `bind_mission_targets`, scene
  graph) and a PE-image VA reader; none committed.
