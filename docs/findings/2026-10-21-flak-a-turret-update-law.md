# FLAK-A-TURRET-UPDATE-LAW (#1819): the original turret update, target step and flak burst

Date: 2026-10-21. Task #1819, split from #1741 (`PARITY3-FLAK-AND-SITE-DEFENCES`,
finding `2026-10-21-m01-flak-and-site-defences.md`). Capabilities used: a static
read of `$CS_ENGINE_IMAGE` only (VAs below; nothing of the image is committed
but cited bytes and constants). No original executable ran; every claim is
provenance `static`, none is `verified_original`.

The cited instructions are rows `[[turret_update_fact]]` of
`data/owner-static/engine-facts.toml` (claim `OWNER-STATIC-2026-10-21`): each
row names the instruction's VA, its machine-code bytes and what it decides. The
acceptance test `accept_flak_a_cited_rows_match_the_engine_image` (ignored
without `CS_ENGINE_IMAGE`) re-reads every row. Names in `code` below are the
rows' `name`.

Entry layout: `this` of the update is the turret registry entry. Offsets are
bytes into it. The class name is `Entity` (RTTI `0x611a78`) for the entry and
`TargetRate::TurretRate` (RTTI `0x611ac8`) for the target filter.

## Who calls the update

`registry.callback` (`0x4a9625`) registers `0x4ac670` as a per-frame callback on
a node; `registry.loop` (`0x4ac67f`) calls the update `0x4aabb0` for every entry
of the list at `[0x71d914]`. The same list is the `TargetTurret` list the target
search scans, so a turret is also a candidate target for other teams' turrets.

## Gates before anything happens (`0x4aabb0..0x4aac72`)

* `gate.latch` (`entry+0x6c`): while clear, the update returns unless the mount
  node `+0x70`, the node `+0xc` and the `DEACTIVATE` node `+0xb4` (when present)
  all have mask 4 set at `node+0x24`; then it sets the latch. While set it
  clears the latch again when those conditions stop holding (the node's
  ancestor chain, up to the root at `[0x71c0b8]`, is walked too). A dropped
  latch switches the `EFFECT` node off and releases the target handle, then
  returns (`0x4aacd0..0x4aad10`).
* `gate.live_byte` (`+0x6e`, `ACTIVATED`): clear returns. `gate.no_weapon`
  (`+0x160`): no `WEAPON` returns.
* `gate.player_flag`: a non-zero byte at `[[0x71c298]+0x91d]` (`0x71c298` is the
  local player's vehicle; the byte is the same one `TargetVehicle` requires to
  be clear) skips the whole engage step. Its name is **unknown**.
* `timer.freeze_flag`: a non-zero byte at `0x64f66e` skips the attack/bored
  timer step only. Name **unknown**.

## Target step (`0x4aaf38..0x4ab23c`) — which targets, which team test

Before the search the update refreshes the mount's world position (`+0x74`, via
`0x4cf490`) and a velocity (`+0x22c`): `velocity.cap` discards a velocity whose
square exceeds `199844.73` (447.04 m/s) as zero.

`target.filter_ctor` builds the filter `TurretRate`: origin `entry+0x74`, team
`*(entry+8)`, `SHOOT_UP_ONLY` (`+0x1e8`); `target.filter_range` sets
`filter+0x18 = entry+0x168`, **`DETECTION_RANGE`** (default `200.0` at
`0x4a9bf7`); `+0x1c` carries `entry+0x6f` (name unknown, default 0). The search
is `target.search` `0x41f9c0(filter, 0, 0, include_structs)` where
`target.include_structs` is true when `[0x629c20] != 0` or the turret's team
differs from the player's team (`[[0x71c298]+8]`).

**The team test.** `entry+8` is an **integer team id**, not a pointer
(`0x441b80` stores it; `TEAM` absent writes `0+2`, `field.team_default`:
**default team 2**; stated `TEAM` is copied as given, `field.team_stated`). Every
wrapper's accept method requires *both ids non-zero and different from each
other* plus a per-class liveness gate:

| Wrapper (RTTI) | List | Accept (vtable+0x34) | Row |
| --- | --- | --- | --- |
| `TargetVehicle` | `[0x71dabc]` | `obj+0x91d == 0`, `obj+0x944 != 0` | `target.accept_vehicle` |
| `TargetStruct` | `[0x71d33c..0x71d340]` (vector) | `obj+0x8c != 0`, `obj+0x90 != 0`; only scanned when `include_structs` | `target.accept_struct` |
| `TargetTurret` | `[0x71d914]` | `obj+0x6c != 0`, `obj+0x6d != 0` | `target.accept_turret` |
| `TargetProjectile` | `[0x64f78c]` | none beyond teams (list entries need `+0x6c`) | `target.accept_projectile` |

So the **hostility test is "different non-zero team id"**: a default-team-2 gun
engages team 1 (the player) and any other team; a turret with `TEAM 0` neither
targets nor is targeted. `SHOOT_UP_ONLY` additionally rejects candidates whose
y is below the filter's (`0x422890`). There is no alliance table in this path.

**Scoring** (`0x4ac720`): `target.score_range` — a candidate beyond
`DETECTION_RANGE` scores `1e21` and is never chosen. Otherwise
`score = weight * 1200 + distance`, lower wins; `weight` is 1.0 and
`target.score_player_weight` 0.8 for the local player's vehicle,
`target.score_projectile_weight` 0.6 for a projectile when the filter's `+0x1c`
byte is set, `target.score_vehicle_class4` 1.4 for a `TargetVehicle` whose
`obj+0x67c == 4`; `target.score_turret_penalty` adds 37.5 m to a turret
candidate's distance. (The `vtable+0x1c` "minus 0.5" term exists but all four
wrappers' slot returns false.) `target.priority_*`: the winner of each list is
kept, and the lists are consulted **projectile, then struct, then turret, then
vehicle** — a hit in an earlier list wins outright regardless of score.

The chosen handle is kept when it names the same object as the one already held
(`target.keep_handle`), stored at `entry+0x210`; no handle clears the engaged
flag (`+0x16c`, `0x4ab23c`).

**Line of sight.** `los.player_only`: only a target that is the local player gets
a ray test here. `los.ray` calls `0x41f130(shooter node, target pos with
y+0.2)` (a ray cast with both nodes' active masks temporarily cleared) at most
every `1.0 + rand01` seconds (`los.interval`, `+0x68`) and caches the answer in
`+0x64`; `los.result_gate`: a clear answer ends the engagement. Weapons whose
descriptor flag `0x40` is set get a second, cached ray (`0x4a5850`, result in
the handle, valid for `[0x60885c]` seconds) from the muzzle to the intercept
point before they may fire.

## Aim, limits and the engaged flag (`0x4ab550`)

`aim.call` runs once per frame while a target is held. The target direction is
taken in the mount's frame (`0x475e20`); pitch and yaw are `atan2` of it.
`aim.pitch_limited` / `aim.yaw_limited`: **limits apply only when the stored
pair is not equal** (the defaults are both 0, i.e. free); the values are `PITCH`
`+0x1d8/+0x1dc` and `YAW` `+0x1e0/+0x1e4`, radians (degrees times the f64 at
`0x6040e8`); yaw wraps by `2*pi` (`0x603d58`). The clamped angles are written into the rotations of the turret
nodes (`+0xb8`, `+0xf4` through `0x4d1f90`).

`aim.aligned_cos`: **the turret may fire only when the barrel direction is
within 15 degrees of the (lead) target direction** — `dot > 0.965926` (the f64
at `0x608b70`) — written to `entry+0x16c` by `aim.engaged_store`. For a weapon
whose descriptor flag `0x40` is set, `aim.intercept_solver` `0x460e30` must also
find an intercept using the weapon's `VELOCITY` (`weapon+0x2c`); if it does not,
the engaged flag stays 0.

The shot's direction: muzzle = `firepoint[entry+0x14c]` (`aim.firepoint`;
16-byte rows from the vector at `+0x140`, the `PARTS` firepoints, round-robin).
For flag `0x40` weapons the direction comes from `aim.dir_solver` `0x53e56d`,
otherwise it is target minus muzzle, normalised. **`aim.inaccuracy`**: when
`entry+0x1d4` (`INACCURACY`, radians) is non-zero the direction is perturbed by
`0x460940` — two `rand01` draws give a random rotation inside a cone of that
angle (`0x4608a0`).

## The attack/bored state machine and fire timing

State `entry+0x1d0`: **0 = attacking, 1 = bored** (`timer.bored_clears_engaged`:
state 1 clears the engaged flag, so a bored turret never fires). Constructor
state is 1 (`timer.initial_state`). The parser seeds the bored end
`+0x1c8 = rand01 * (an ATTACK_INTERVAL draw)` (`timer.initial_bored_end`).
`timer.bored_end`: state 1 and `now > +0x1c8` becomes state 0 with
`+0x1cc = now + ATTACK_INTERVAL.min + rand01 * (max - min)`; `timer.attack_end`:
state 0 and `now > +0x1cc` becomes state 1 with
`+0x1c8 = now + BORED_INTERVAL.min + rand01 * (max - min)`. Hence the order in
the records: a turret waits a random part of its first attack interval, attacks
for an `ATTACK_INTERVAL` draw, rests for a `BORED_INTERVAL` draw, repeats. (The
parser stores `ATTACK_INTERVAL` at `+0x1b8/+0x1bc` and `BORED_INTERVAL` at
`+0x1c0/+0x1c4`; a single stated value duplicates into both.) `rand01` is
`rand() * (1/32767)` (`0x603598`, bits `0x38000100`) with `rand` the call
through `[0xa20350]`.

**Fire gate.** A shot needs: held target, `+0x16c` set, `now >= +0x1a8`
(`fire.next_time_gate`), and for flag `0x40` weapons a visible cached ray.
**`FIRE_RATE`**: `fire.rate_draw` sets `+0x1a8 = now + FIRE_RATE.min + rand01 *
(max - min)` after a shot (the parser draws the first one at construction,
`0x4aa3a4`, as `(min+max) * rand01 * 0.5 + now + 1.0`). **`EFFECT`**
(`fire.effect_windup`, `fire.effect_off`): when the turret has an `EFFECT` node
that is inactive, the pass switches it on, sets `+0x1a8 = now + EFFECT
duration (+0x158)` and fires **nothing**; the next due pass switches it off and
fires. **`FIRE_LIMITS`** `(a, b)` (`fire.limits`, `fire.limits_recharge`): with
`a > 0`, each shot subtracts `dt` from `+0x1ac`; at `<= 0` it reloads to `a`,
clears the engaged flag and sets `+0x1a8 = now + b`; while not engaged `+0x1ac`
recharges by `dt` up to `a`. With `a == 0` there is no limit.
`fire.firepoint_next` advances the firepoint index after every pass.

`AMMO` (`+0x164`, default 50) is **not read or written** anywhere in
`0x4a9950..0x4ac720` except the parser store, the constructor default and the
save-state copy (`0x4ac693`, `0x4ac6f7`). Whether `0x5aef40` or the weapon
consumes it is **unknown**.

## The fire call

`fire.weapon_fire` `0x4ab3cc` calls **`0x5aef40`**, the generic weapon fire:
ten arguments: weapon, mount node `entry+0xc`, 0, muzzle position `+0x188`,
direction `+0x17c`, `+0x194`, 0 (no existing entity: allocate one), a lock-on
flag, a distance and the lock-on record. Just before it `0x5ac120(weapon, -1)`
and `0x5ac0a0(+0x1ec, +0x1f0)` run (the latter writes two globals,
`0xa1d7cc/0xa1d7d0`, cleared by `0x5ac0a0(0, 0)` after; both unnamed). It is the same function the other
fire sites call (`0x4b6e7b`, `0x4b7289`, `0x4b7559`, `0x4c01e0`, `0x4c0d59`):
**a turret's round is a normal weapon entity from the weapon pool** (`0x5aeca0`),
not a special spawn. `fire.post` `0x441830` then runs with the entry's team
(purpose not traced), and when the turret's owner (`entry+0x150`) is the local
player's vehicle and `[0x71c4e0]` is set, `0x481640(1)` runs (unnamed).

`fire.lock_target_gate`: only weapons whose descriptor flag `0x40` is set build
the lock-on record (the target, passed when
`sqrt(d / (r + d)) < [0x71d920]`, where `[0x71d920]` is the cosine between the
perturbed and the true aim direction left by the aim step).

## The flak rocket's burst (`0x5afd50..0x5b0770`, impact `0x5ac7a0`)

The weapon parser stores `DETONATION_DISTANCE` **squared** at `weapon+0x44`
(`weapon.detonation_distance`), `DETONATION_TIME` at `+0x48`, `IMPACT_PROXIMITY`
at `+0x3c` and squared at `+0x40` (`weapon.impact_proximity_raw/_sq`) and
`LOCK_ON` sets weapon flag `0x8000` with its value at `+0x58`
(`rocket.lock_on_flag`). `0x538880` is the squared distance, `0x5388d0` the
distance.

In the rocket update, with `travelled = rocket+0x664` against `RANGE`
(`rocket.range_test`):

* **Below `RANGE`, `LOCK_ON` weapon, held target (`rocket+0x70`), non-zero
  `DETONATION_DISTANCE`: the rocket bursts at its own position as soon as the
  squared distance to the target is `<=` `DETONATION_DISTANCE` squared**
  (`rocket.proximity_burst`). For `wep_27` that is **20 m from the target**
  (`LOCK_ON 3`, `DETONATION_DISTANCE 20`).
* Homing steering stops when the squared distance is below `4 *
  DETONATION_DISTANCE²` (that is 40 m) or below 400 (20 m) (`rocket.homing_stop`).
* `DETONATION_TIME > 0` and rocket age `rocket+0x668` beyond it bursts too
  (`rocket.time_burst`); flak states none.
* At `travelled >= RANGE` a rocket bursts only with `DETONATE_AT_RANGE`
  (flag `0x2000000`), apart from a `LOCK_ON` branch at `0x5b01f9` keyed on a
  second flag bit (`0x80`) that is not named (`rocket.range_end`).

At the burst `rocket.impact_call` runs `0x5ac7a0`, which picks the `IMPACT` row
(`0x5abcf0`) and plays its effect and sound (`0x5ad100` picks a sound by
`rand`). **`IMPACT_PROXIMITY` is the radius of the sphere query at the burst
position** (`rocket.splash_radius`: `entity+0x678 * IMPACT_PROXIMITY` through
`0x4cb420`, run when `IMPACT_PROXIMITY > 0`, `rocket.splash_gate`); it is not
the trigger distance. For `wep_27` the burst is at 20 m from the target and the
hit sphere is 22 m (times `entity+0x678`, value unknown).

## Unknowns, affected content and resolving work

| Unknown | Affected content | Resolves in |
| --- | --- | --- |
| Whether `wep_27`'s weapon descriptor (`weapon+0x210`, flag `0x40`) is set; it decides lead aiming, the intercept-solution requirement and whether a flak shot carries a lock-on target (and therefore bursts at 20 m rather than flying to range) | every flak round's burst point | a descriptor trace: the callers of the weapon constructor around `0x5ae0ed` and the static descriptors they pass |
| Which `flak_effect` parts run at the burst point: `0x5ac7a0` selects the `IMPACT` row and plays `0x5ac580/0x5ac690/0x5ad100`, the animation start itself was not followed; the `PLAYER_RANGE` read is at `0x516a94` | the puff, flash, trails and the full-screen flicker | a trace of `0x5ac580`, `0x5ac690` and the animation system's start call; or an owner capture |
| The names of `[[0x71c298]+0x91d]`, `0x64f66e`, `entry+0x6f`, `entry+0x6c/+0x6d` of the other wrappers, the rocket flag `0x80` and the `rocket+0x678` factor | the dormant/frozen cases of the gates | the same descriptor and object-layout traces |
| Who consumes `AMMO` | turret ammunition | trace of `0x5aef40` and `0x441830` |
| What happens at range end for non-`DETONATE_AT_RANGE` rockets (`0x5b0318..`) | whether an unlocked flak round vanishes or bursts | trace of `0x5b02ba..0x5b0330` |
| The `HEALTH` unit of a turret record | hits to silence a gun | #668 / F27-E |

## What this settles for #1741

Our mission has no turret runtime (the parent's finding). The law to build, as
measured: per-turret latch and `ACTIVATED`; detection by hostile team id within
`DETECTION_RANGE` with the scorer above; the 1–2 s player ray; 15-degree
alignment inside the pitch/yaw limits; attack/bored timers; the `FIRE_RATE` and
`FIRE_LIMITS` draws with the `EFFECT` wind-up; the `INACCURACY` cone; and a
normal weapon entity from `wep_*`. The flak burst rule above holds only for a
round that carries a target; that is the first item of the table.
