# T1318 — the ordnance blast's remaining unknowns, measured

Follow-up to task #548 (`2026-10-11-f28-c-2-lost-target-blast-measured.md`),
which measured the lost-target blast rule and left seven fidelity questions
open. Every statement below is **static analysis** of the owner-supplied
decrypted executable (`$CS_ENGINE_IMAGE`, sha-256 `43540fc9…6c37d75`, base
`0x400000`) plus the retail record data (`$CS_GAME_DIR`, `ZBD/zrdr.zbd`
`weapons.zrd`). Nothing here is original-run behaviour; no agent can run the
original. Claim level: `checked` (static), never `verified_original`.

The byte windows every measurement rests on are pinned by
`accept_t1318_image_pins_the_blast_unknowns`
(`crates/cs_content/tests/accept_t1318_retail_blast_unknowns.rs`); the
addresses below name them.

## Q1 — the unit scale of `IMPACT_PROXIMITY` / `DETONATION_DISTANCE` — measured: metres

At `0x5add70` (IMPACT_PROXIMITY) and `0x5adbb2` (DETONATION_DISTANCE) the
record value is loaded, squared (`fld; fmul st(1)`) and stored at weapon
`+0x40`/`+0x44` — no scale constant, no conversion. The squared copies are
compared directly against world-space squared distances (positions come from
the world grid; the distance helper `0x5388d0` is a Euclidean 3D distance over
world coordinates). The F16-E calibration landmarks
(`docs/findings/2026-10-06-f16-e-original-unit-calibration-landmarks.md`)
establish the world unit is the metre, so `IMPACT_PROXIMITY 25` is a 25 m
blast radius. `RANGE` and `RANGE_MINIMUM[0]` ride the same world-space
distance, so they are metres too.

`cs_content::ordnance::OriginalRocketBlast` now documents its distance fields
as metres, and `OriginalRocketBlast` gained the `RANGE_MINIMUM` pair (below).

## Q2 — the projectile `+0x678` multiplier — measured: the blast-and-damage scale (the arming factor)

* Writer: projectile spawn at `0x5af098` copies global `0xa1e1b0` into
  `[projectile + 0x678]`; `0x5af0c3` immediately resets the global to `1.0f`,
  and nothing else writes it — every projectile starts at scale **1.0**.
* Consumers: the apply loop at `0x5acb56`/`0x5acb60` multiplies **both**
  stored damage channels by `+0x678`.
* Arming writer: the impact/detonation branch of the step (`0x5b063d` onward)
  tests flag bit 31 (`RANGE_MINIMUM` declared, set at `0x5adfcd` alongside the
  pair's store at weapon `+0x24`/`+0x28`) and the travelled distance
  `[esi+0x664]` against `RANGE_MINIMUM[0]`:
  * `[1] == 0` and travelled < `[0]`: the branch returns early at
    `0x5b0668` — the detonation/impact is **suppressed** and the projectile is
    not consumed (`+0x674` cleared, `ret`). The torpedo's `300, 0` is a true
    arming range: it sails through targets closer than 300 m.
  * `[1] != 0` and travelled < `[0]`: `0x5b06c6`–`0x5b06cf` multiplies
    `+0x678` by `RANGE_MINIMUM[1]` — a partial arming penalty on blast and
    damage.

So `+0x678` is the projectile's blast-and-damage scale, and `RANGE_MINIMUM`'s
second element is the in-window scale, with `0` meaning "suppress the
detonation". `OriginalRocketBlast::range_minimum` /
`range_minimum_blast_scale` carry the pair; `cs_sim`'s `ArmingRule` models
the distance half and the scale semantics are documented where the falloff
is applied.

## Q3 — object `+0xbc` and global `0xa1d7d4` — measured: exclusion key and re-entrancy guard

* `+0xbc`: at `0x5acb0f` the apply loop compares `[recipient + 0xbc]` against
  the loop's fourth argument and skips equal recipients (`je 0x5acc37`).
  Timed/proximity detonations call the loop with that argument `0` (from
  `0x5ac3a0`); the impact path at `0x5b0741` passes
  `[hit_object + 0x24] -> +0xbc` — the *hit object's* group key. Measured
  meaning: on impact, the object (or group) the projectile directly hit is
  excluded from its own area damage — it already took the impact hit. The
  field's full domain (who else shares a key) is not traced; see
  limitations.
* `0xa1d7d4`: a global flag, set to `1` at init (`0x5ad4e7`, `0x5ae8e1`),
  read at `0x5ac2be` (impact routine `0x5ac210`) and `0x5ac415` (detonation
  `0x5ac3a0`) to gate the area call, cleared to `0` at `0x5b09f8` around the
  nested call into `0x5ac210`, restored to `1` at `0x5b0a0b`. Measured
  meaning: a **re-entrancy guard** — a detonation that funnels into nested
  impact processing does not re-enter its own blast.

## Q4 — the area-query entry value — measured: squared distance to the bounding surface

In the query `0x4cb420`:

* `0x4cb519`–`0x4cb540`: the radius is squared and stored at scratch
  `0x71e898`.
* `0x4cb708`: candidate distance from helper `0x5388d0` (Euclidean 3D).
* `0x4cb70d`: `fsub [esp+0x58]` — the centre distance minus the candidate's
  bound (bounding-sphere extent, from `0x4d8bd0`).
* `0x4cb714`–`0x4cb723`: clamped at `0.0` — a centre inside the bound counts
  as distance 0.
* `0x4cb72d`–`0x4cb731`: `fld st(0); fmul st(1)` — **squared**.
* `0x4cb73b`: compared against `0x71e898` (radius²); `0x4cb80a` stores the
  squared value at entry `+0x1c`, which `0x5acb37` divides by `r²` for the
  falloff.

So the falloff distance `d` in `1 − d²/r²` is the distance from the blast
centre to the recipient's bounding surface, never negative — not the distance
to its centre. `cs_sim`'s `AreaEffect::blast_fraction` documents this; a
producer supplying a point inside the blast's reach models it by giving the
surface distance.

## Q5 — removal past `RANGE` — measured: the active word and the sweep

* The projectile step clears the projectile's `+0x20` active word when
  travelled distance passes `RANGE` without a detonation (`0x5b0318`,
  `mov [esi+0x20], 0`).
* The update loop (`0x5af720` region) skips projectiles whose `+0x20` is `0`.
* The sweep `0x5af900` unlinks any projectile with `+0x20 == 0`
  (`0x5af917`–`0x5af933`) and frees it through `0x5b08d0` (resource teardown,
  grid unregister, generation bump at `+0x66c`).

So removal is deferred one pass: the step marks the projectile inactive and
the list sweep frees it. There is no "fly off" — a `RANGE`-past projectile
without `DETONATE_AT_RANGE` just stops existing.

## Q6 — the special keys and the `RANGE_MINIMUM` node call — measured at the parser; partially measured downstream

A second parser, `0x4ba6f0`, fills a `0x38`-byte spec record hung off
`weapon + 0x210`. Verified key → bit/param map (`or` sites pinned):

| key                    | record `+0x00` bit | parameters                                   |
| ---------------------- | ------------------ | -------------------------------------------- |
| `SONIC`                | `0x200`            | —                                            |
| `HIGH_EXPLOSIVE`       | `0x400`            | —                                            |
| `FLASH`                | `0x800`            | —                                            |
| `DAMAGES_ZEPPELIN`     | `0x1000`           | —                                            |
| `SHAKES_CAMERA`        | `0x2000`           | —                                            |
| `BEEPER`               | `0x4000`           | `TIME` → `+0x18`, default `5.0f`             |
| `BEEPER_SEEKER`        | `0x8000`           | —                                            |
| `SMOKE_SCREEN`         | `0x10000`          | `TIME` → `+0x18`, default `5.0f`             |
| `REAR`                 | `0x20000`          | —                                            |
| `TANGLER`              | `0x40000`          | `TIME` `+0x18` default `5.0f`, `RADIUS` `+0x1c` default `10.0f`, `ENGINE_DEAD[2]` |
| `DETONATION_DOT_PRODUCT` | `0x80000`        | value → `+0x20`                              |

Also parsed into the record: `PRIORITY` `+0x04`, `CANNON_SPREAD` `−cos` of
the angle → `+0x08`, `CLUSTER_SIZE` `+0x0c`, `CALIBER` `+0x10`,
`FIRING_HEAT` `+0x14`, `SOUND_NAME` `+0x24`; `TORPEDO` `0x8`, `ROCKET`
`0x10`, `TARGETABLE` `0x20`, `CANNON` `0x40`.

`TANGLER` additionally registers a tick callback `0x4ba660` via `0x5aec90`,
stored at `weapon + 0x20c` (pin `0x5aec98`). The callback checks the bit,
looks up the hit object in the object map and spawns a `0x1c`-byte
status-like object against it (`0x4b94e0`) — the choke/stall effect object,
with `ENGINE_DEAD` as its parameter block.

Measured consumers:

* `SMOKE_SCREEN` (`0x10000`): tested on the weapon flag word at `0x5af98b`
  inside the projectile step — after the `TIME` age passes, `0x53e56d` is
  called to spawn the smoke emitter at the projectile; also tested at
  `0x4b71b1`/`0x4b74a7` (effect-object spawn).
* `TANGLER` (`0x40000`): tested on the weapon flag word at `0x5ac850` and
  `0x5ac8c8` in the impact path — gates the status-application call
  `0x5ac580` against the hit object.
* `REAR` (`0x20000`): `0x4b68d0`, `0x4b6fa1`, `0x4b7069`, `0x4b743e` —
  launch-direction handling.
* `DETONATION_DOT_PRODUCT` (`0x80000`), `DAMAGES_ZEPPELIN` (`0x1000`),
  `CANNON` (`0x40`): tested at `0x4b6363`/`0x4b998d`, `0x42008c`/`0x4d9035`,
  `0x4ba049`/`0x4bad07`.

`RANGE_MINIMUM`'s call `0x4cd210(node, flag)` sets or clears node flag
bit `0x10` at `node + 0x24` (pin `0x4cd245`; getter `0x4cd260`). The
detonation path clears the projectile's own node flag before the area query
and restores it after — the flag gates whether the node participates in the
query (the query itself calls `0x4cd210` on candidates carrying object flag
`0x400000` at `0x4cb77a`).

## Still unknown (recorded, not claimed)

* The gameplay consumers of the `SONIC` (`0x200`), `FLASH` (`0x800`),
  `BEEPER` (`0x4000`) and `BEEPER_SEEKER` (`0x8000`) bits: the flag-word
  stores are pinned but no test site was located in the scanned regions.
  Claim `t1318.limit.status_consumers`.
* The full domain of the `+0xbc` exclusion key (its writers and which
  objects share one). Claim `t1318.limit.exclusion_domain`.
* `step_ordnance_session` still does not call `route_detonation_area` — the
  session owns no actor list. Claim `t1318.limit.session_wiring` (carried
  over from #548).
