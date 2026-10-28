# T452.R02: the measured original ordnance effect, fuse and swarm behavior

Date: 2026-10-28. Task: #2160 (T452.R02), the second measured subtask of
#2156 (`T452-RUNTIME-REGISTER`). Spec:
`specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`
(non-negotiables 1-4, stage `### F28-D`). Shared contract:
`docs/contracts/FLIGHT-PHYSICS.md`, "Boost and special models". Predecessors
cited, not repeated: `2026-10-25-t452-01-weapon-census.md` (the record census),
`2026-10-25-t452-02-proximity-fuse-shape-and-arming.md` (the trigger sphere),
`2026-10-25-t452-04-component-hardpoints-launch-speeds-and-lifetimes.md` (the
per-component table), `2026-10-19-t1601-status-bit-consumers.md` (the four
effect bits this task extends), `2026-10-11-f28-c-2-lost-target-blast-measured.md`
and `2026-10-18-t1318-blast-unknowns-measured.md`.

Capabilities used: `retail` (read access to `$CS_GAME_DIR`) and the
owner-supplied decrypted engine image `$CS_ENGINE_IMAGE` (sha-256
`43540fc97347210d…`, image base `0x400000`) for static analysis. **Not** used
and not claimed: any run of the original executable, any GPU, audio or human
observation. Every statement below is what the image's code *does* and what the
records *declare*; nothing is `verified_original`.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/ordnance.rs` (extend): `StaticProvenance`,
  `MeasuredEffectAction`, `MeasuredEffectDuration`, `MeasuredEffect`,
  `MeasuredAreaSource`, `MeasuredArea`, `MeasuredRangeEnd`, `MeasuredFuse`,
  `MeasuredLostTarget`, `MeasuredSwarm`, `OriginalBehaviorMeasurement`,
  `measure_component_behavior`, `measured_area` and the
  `T452_R02_*` claim ids.
- `crates/cs_content/tests/accept_t452_r02_behavior.rs` (new, 11 tests, module
  of `tests/main.rs`).
- `crates/cs_content/tests/main.rs` (wiring: one `mod` declaration).
- This file and `docs/findings/evidence/T452-R02.json`.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data, no
binary file, no original display text and no script text.

**One observable failure, before the change:** the T452.04 table could say
*which* effect keys a record declares and *what duration the key states*, and
the catalogue could carry the fuse, lost-target and swarm claims — but nothing
in the project could answer **what any of those effects does**, **what radius
and lifetime the two area-denial components actually have**, **how a projectile
ends its flight**, **whether the lost-target rule needs a `ROCKET` flag**, or
**what `CLUSTER_SIZE` launches**. `OriginalStatusEffectDeclaration` deliberately
assigned no `DeclaredStatusEffectKind`; `t452_04.limit.area_radius`,
`t452_04.limit.area_lifetime`, `t452.limit.fuse`, `t452.limit.lost_target` and
`t454.limit.swarm` were named claims with no measured value behind them.

## Method

Linear and targeted disassembly of the image with Capstone 5.0.7 (not
committed), following each record key to the instruction that stores it and each
stored word to the instruction that consumes it. Every measured fact cites a
virtual address and a byte window; `accept_t452_r02_the_image_pins_every_measured_site`
re-reads all 24 windows from `$CS_ENGINE_IMAGE` on every run, so a stale VA or a
wrong instruction fails rather than passes. The record side is the existing
production reader (`read_original_component_table`), so the retail test measures
the owner's own installation through the same path the project already uses.

## 1. Effect flags: what each one does, and how long

The engine parses each effect key into a second, per-actor weapon object (0x38
bytes at `actor + 0x210`) whose first word is a behavior-flag set
(`0x4ba6f0`..`0x4baa7b`). Measured flags and their consumers:

| key | flag (`+0x00`) | consumer | measured behavior |
| --- | --- | --- | --- |
| `TARGETABLE` | `0x20` | — | not an effect |
| `SONIC` | `0x200` | `0x4b9d8c`–`0x4b9d8f` | with `FLASH`, jumps to the exposure path |
| `FLASH` | `0x800` | same | exposure path, and bit 11 is its facing flag |
| `HIGH_EXPLOSIVE` | `0x400` | `0x4b9c95` | damage-kind selector |
| `DAMAGES_ZEPPELIN` | `0x1000` | `0x4ba94e` | zep-only damage |
| `SHAKES_CAMERA` | `0x2000` | `0x4b9ce2` | camera shake on the player |
| `BEEPER` | `0x4000` | `0x4b9d95`, `0x4b8ce0` | timed marker |
| `BEEPER_SEEKER` | `0x8000` | `0x441864` | guidance callback |
| `SMOKE_SCREEN` | `0x10000` | `0x4b71b1` | scene emitter |
| `REAR` | `0x20000` | `0x4b68d0` | launch direction |
| `TANGLER` | `0x40000` | `0x4ba841` | area field + per-tick callback |

### SONIC and FLASH: a per-hit exposure with a measurable duration

The measured consumers are `0x4b9d8c` (`test ch, 0x0a` — SONIC|FLASH together)
and `0x4ba564`, which loads `FLASH` (bit 11 of the 0x38 object's flags) as the
exposure routine's first argument and calls `0x42e840`. That routine:

- returns 0 at once for a victim whose `+0x91d` is set (`0x42e84a`);
- forms `ratio = arg[0x18] / arg[0x1c]` (`0x42e85b`, `0x42e862`) — the per-hit
  measure over `attacker + 0x3c` squared (the caller squares
  `[ebp+8] + 0x3c` at `0x4ba590`–`0x4ba5a9`);
- intensity is `clamp(1 - 2.5 * (ratio - 0.6), 0, 1)` (`0x42e865`..`0x42e88a`;
  constants `0x60411c` = 0.6 and `0x6040a8` = 2.5): 1.0 at or below ratio 0.6,
  falling linearly to 0 at ratio 1.0 and above;
- with the FLASH flag and a position, the intensity is multiplied by
  `clamp(2 * dot(victim_forward, victim_position - source_position), 0, 1)`
  (`0x42e8ed`..`0x42e966`), i.e. a hit from behind does nothing and a hit past
  0.5 of the facing dot is full; SONIC skips this scaling;
- the out-value used as the AI's incapacitation duration is
  `intensity * 5.0` seconds (`0x42e9b1`, constant `0x6036bc` = 5.0), and the
  second out-value is the 0..1 intensity the player's screen effect takes.

The AI path `0x4200d0` (T1601) stores `clock + duration` at victim `+0xc0`,
sets mode `+0x358 = 4` and zeroes the control words — the struck actor is
**incapacitated for that duration**.

**Runtime mapping: none is forced.** `DeclaredStatusEffectKind` is
Damage/Choke/Stall/Marker; the measured effect is pilot incapacitation (controls
zeroed, mode 4), which is not damage, not engine output loss, not a stall and
not a marker. The row keeps `t452_r02.limit.status_incapacitation` instead of
being lowered to a variant it is not. The duration side *is* resolved: the
record states no duration, and the formula above is the measured duration, so
`t452_04.limit.status_effect_duration` is resolved for SONIC and FLASH.

### BEEPER: a timed marker — the one status with a fitting runtime kind

`0x4ba779` stores the default `TIME` 5.0 at the 0x38 object's `+0x18`; the
record's `TIME` overrides it (`0x4ba7ad`/`0x4ba7b0` for an int, `0x4ba7a3` for
a float). `wep_10` states 20, so its marker lasts 20 s. The admission gate
`0x4b8ce0` and the list at `0x71dbac` (T1601) place one 0x10-byte marker on the
victim; the BEEPER shot itself is consumed. This is the runtime `Marker` kind —
"a marker for other systems and the pilot; no damage, no loss" — and the
duration is the record's `TIME`. The kind claim `t452_04.limit.status_effect_kind`
is resolved for BEEPER; the duration claim too.

### TANGLER: an area field, not a per-victim status

Parsing `TANGLER` (`0x4ba841`..) sets flag `0x40000` (`0x4ba859`), stores the
default `TIME` 5.0 at `+0x18` (`0x4ba869`), the default `RADIUS` 10.0 at `+0x1c`
(`0x4ba870`), installs the tick callback `0x4ba660` (`0x4ba877`), then overrides
`+0x18` from the record's `TIME` (`0x4ba8a4`/`0x4ba8b1`) and `+0x1c` from the
record's `RADIUS` (`0x4ba8df`/`0x4ba8ec`).

The field object is built by `0x4b94e0`: `+0x00` the owning actor, `+0x04` the
weapon object, `+0x08` the position, `+0x14` = `RADIUS²` (`0x4b950f`–`0x4b9518`)
and `+0x18` = `TIME` (`0x4b9523`–`0x4b9526`). Its tick `0x4b9590` subtracts `dt`
from `+0x18` (`0x4b9597`), and while the remaining time is positive walks the
actor list at `0x71dabc`; for each live actor it compares the squared distance to
the field position with `+0x14` (`0x4b95db`–`0x4b95e4`) and, inside the radius,
applies the weapon's own per-hit routine `0x4b9bc0` with zero damage arguments
(`0x4b95f1`–`0x4b961a`). At `<= 0` the field is unlinked and freed
(`0x4b96f2`–`0x4b9736`).

So **the tangler is an area of `RADIUS` around the field position that lasts
`TIME` and re-applies the weapon's hit every tick to every actor inside it**.
For `wep_12` the record states `TIME` 2 and no `RADIUS`, so the field is 10 m
(image default) for 2 s. `t452_04.limit.area_radius` and
`t452_04.limit.area_lifetime` are resolved for `wep_12`.

**The gameplay consequence for `wep_12` is still a claim**
(`t452_r02.limit.tangler_flight_effect`): the record declares 0 armor and 0
health damage and none of SONIC/FLASH/BEEPER, so the per-hit routine inside the
field applies no measured effect. The field exists and is measured; what a
zero-damage tangler does to a flight model is not visible in the measured path,
and no drag or control-lock store is reachable from it.

### SMOKE_SCREEN: a visual emitter, an area with a lifetime and no radius

Parsing `SMOKE_SCREEN` sets flag `0x10000` (`0x4ba7f8`), default `TIME` 5.0 at
`+0x18` (`0x4ba806`), then the record's `TIME` (`0x4ba82d`/`0x4ba83a`). At fire
time the fire path tests the flag (`0x4b71a6`–`0x4b71b6`), allocates a 0x18-byte
node, reads `TIME` (`0x4b71da`) and calls `0x4b8d50`, which attaches a scene
emitter node with an update callback (`0x4b8d50`–`0x4b8db2`). `wep_13` states
`TIME` 8, so the smoke lasts 8 s. **No code reads a gameplay radius for it**:
the emitter is presentation, not a gameplay area. `t452_04.limit.area_lifetime`
is resolved for `wep_13` (8 s); `t452_04.limit.area_radius` stays open as
`t452_r02.limit.smoke_radius`, because a visual emitter has no measured radius
and defaulting one would fabricate an area.

### BEEPER_SEEKER: a guidance modifier, not a status

`0x441864` tests bit 15 and installs the callback `0x441780` into the
projectile's `+0x688` slot (`0x441877`); the update loop calls it once per live
projectile per step (`0x5af745`, T1601). The callback walks the beeper list and
writes the best live beeper into the projectile's target slot, so the seeker
retargets every step and flies unguided when no beeper is live. That *replaces*
the one-target homing for as long as a beeper exists. `GuidanceRule::Targeted`
names one target and cannot express repeated retargeting, so `wep_11` keeps
`t452_r02.limit.beeper_seeker_guidance` as a named claim.

## 2. Area radius and lifetime

| component | radius | lifetime | source |
| --- | --- | --- | --- |
| every blast (`IMPACT_PROXIMITY`) | the record's own value, metres (T1318: no conversion) | **instantaneous** — damage applies once per detonation | `0x5ac3a0` explosion path; T1318 |
| `wep_12` tangler | 10.0 m (image default, record declares no `RADIUS`) | 2.0 s (`TIME`) | `0x4ba870`, `0x4b94e0`, `0x4b9590` |
| `wep_13` smoke | **not measured** — visual emitter only | 8.0 s (`TIME`) | `0x4b8d50`, `0x4ba82d` |

A blast having no area lifetime is a measurement, not an omission: the
detonation path calls the blast once and the projectile is then dead, so there
is no area that persists. The layer records `InstantaneousBlast` for those rows
and resolves `t452_04.limit.area_lifetime` for them.

## 3. Fuses: how `wep_13` and `wep_26` end

The unguided projectile step `0x5afd50` compares the distance travelled
(`projectile + 0x664`) with the weapon object's `RANGE` (`+0x1c`, stored by
`0x5adfb5`) at `0x5b01ec`, and at the end of range (`0x5b01f9`–`0x5b0211`):

- if `LOCK_ON` (`0x8000`, set by the key at `0x5addd5`) is set **and**
  `EXPIRES` (`0x80`, parsed at `0x5adca6`) is not, it detonates;
- otherwise, if `DETONATE_AT_RANGE` (`0x2000000`, parsed at `0x5adbf5`) is set,
  it detonates;
- otherwise it takes `0x5b0318`, which stores 0 to `projectile + 0x20` — the
  projectile is marked dead and is consumed with no detonation.

So the range-end detonation is **gated on `LOCK_ON` (or the explicit
`DETONATE_AT_RANGE` key), not generic**. Measured consequences:

- `wep_26` declares `ROCKET`, `IMPACT_PROXIMITY` 100, `RANGE` 900, no
  `LOCK_ON`, no `EXPIRES`, no `DETONATE_AT_RANGE` and no
  `DETONATION_DISTANCE`. It **does not detonate at the end of range**: it is
  consumed silently. `IMPACT_PROXIMITY` is its blast radius, not a range-end
  trigger, and the proximity branch at `0x5b0216` is gated on `LOCK_ON`, which
  the record lacks. This resolves `t452.limit.fuse` for `wep_26` at the end of
  flight; what a collision or another path does with the shell is not part of
  this measurement.
- `wep_12` declares `DETONATION_DISTANCE` and no `LOCK_ON`, so its proximity
  trigger is unreachable for the same reason: its only measured end of flight
  is the silent removal at `0x5b0318`. This resolves `t452.limit.fuse` for
  `wep_12` too — the claim the catalogue closure left open precisely because
  the trigger was unreachable, not absent.
- No unguided record has another end: none of the fifteen `ROCKET` records
  declares `DETONATE_AT_RANGE` (T548's 41-key census), so the measured rule
  leaves every unguided row with the silent removal and the layer resolves
  `t452.limit.fuse` for all of them.
- `wep_13` declares neither `RANGE`, `VELOCITY`, `LOCK_ON` nor any detonation
  key. No record key writes the `RANGE` field (`+0x1c`) it is compared with, so
  there is no record-supplied bound to fly to; the projectile is consumed
  (`0x5b0318`) without detonating. The smoke it leaves is the emitter
  created at fire time and lasts `TIME` = 8 s independently of the projectile.
  The task hint that `0x53e56d` (called from `0x5af98b`) is a smoke emitter is
  **wrong**: that call site is inside the guided-weapon lead/facing path gated
  on the `LOCK_ON`-paired bit `0x10000`, and `0x53e56d` is a geometric
  ray/segment test, not an emitter. The emitter is `0x4b8d50`, reached from the
  fire path. This resolves `t452.limit.fuse` for `wep_13` honestly, with the
  corrected site.

The `+0x674` word the task asked about is the timed-fuse countdown: the
per-projectile update tests it at `0x5af778` and, when it is non-zero, takes the
cleanup path and clears `+0x20`; `wep_13` and `wep_26` declare no
`DETONATION_TIME`, so it stays 0 and the normal step runs.

## 4. `wep_25`'s lost target

The steering guard at `0x5af7f4`–`0x5af805` reads the **weapon object's
`+0x74`**, tests bit 15 (`0x8000`), and only steers when the projectile's target
pointer `+0x70` is non-null. The proximity branch at `0x5b0216`–`0x5b022b` tests
the same bit 15 before it reads `DETONATION_DISTANCE` and the target. The bit is
the one `LOCK_ON` sets (`0x5addd5`), and nothing in either gate tests the
`ROCKET` flag (`0x10` of the 0x38 object, `0x10` of the 0x214 record object).
The rule is therefore **generic over `LOCK_ON`**, not over `ROCKET`-flagged
records: `wep_25`, the one `LOCK_ON` record with no `ROCKET` flag, inherits
the measured coast-then-detonate-at-end-of-range rule. Its `RANGE` is 900, it
declares `LOCK_ON` and no `EXPIRES`/`DETONATE_AT_RANGE`, so it detonates at the
end of range. `t452.limit.lost_target` is resolved.

## 5. `CLUSTER_SIZE` and the swarm

`CLUSTER_SIZE` is parsed into the 0x38 weapon object at `+0x0c` (`0x4baa17`).
The launch path does **one** spawn per pass: `0x5aef40` builds exactly one
projectile no matter the count, and the fire function decrements the object's
countdown (`0x4b72be`) and returns to the next hardpoint; it fires again only
while the countdown is still positive (`0x4b72e1`–`0x4b72e6`). The same field is
the round counter on the guns, where `CLUSTER_SIZE` equals `AMMO_LIMIT`
(T452.04). So a `CLUSTER_SIZE` of N is **N launches of one projectile each, one
per launch step — never one projectile that expands into N**. `t454.limit.swarm`
is resolved for every volley row: what a sub-munition *does* is the record's
own declared behavior, now measured above; the launch itself is one projectile
per sub-munition.

## The measured layer and the claims it resolves

| claim | status after this task |
| --- | --- |
| `t452_04.limit.status_effect_kind` (BEEPER, TANGLER, SMOKE_SCREEN) | resolved (marker / area / area) |
| `t452_04.limit.status_effect_kind` (SONIC, FLASH) | **kept**, as `t452_r02.limit.status_incapacitation` — no runtime variant fits |
| `t452_04.limit.status_effect_duration` | resolved (BEEPER/TANGLER/SMOKE `TIME`; SONIC/FLASH the measured formula) |
| `t452_04.limit.area_radius` (`wep_12`) | resolved (image default 10 m) |
| `t452_04.limit.area_radius` (`wep_13`) | **kept**, as `t452_r02.limit.smoke_radius` |
| `t452_04.limit.area_lifetime` | resolved (`wep_12`/`wep_13` `TIME`; blasts are instantaneous) |
| `t452.limit.fuse` (`wep_12`, `wep_13`, `wep_26`, and every unguided row) | resolved (each expires silently at range end) |
| `t452.limit.lost_target` (`wep_25`) | resolved (the gates test `LOCK_ON`) |
| `t454.limit.swarm` | resolved (one projectile per sub-munition per launch) |

A record that states **no** `TIME` for BEEPER, TANGLER or SMOKE_SCREEN reports
the image's own `5.0 s` default (`MeasuredEffectDuration::ImageDefaultSeconds`,
`MeasuredAreaSource::ImageDefault`), never the record's `TIME` source — the
provenance label follows the value it actually used.

**Why the T452.04 claim ids still appear in `OriginalComponentLaunch::unknowns`.**
The T452.04 acceptance tests pin the exact claim set and per-claim row counts of
that stage, and AGENTS.md forbids weakening or deleting another stage's tests. A
resolved claim is therefore not deleted from the T452.04 row here; the new
measured layer records it in `OriginalBehaviorMeasurement::resolved` and a test
asserts that no row both resolves and still names the same claim. Removing the
now-empty claim entries from the T452.04 rows, and dropping them from the
`cs_app` catalogue mapping, belongs to the parent runtime-registration task
(#2156) that consumes this layer; doing it here would edit `cs_app`, outside
this task's owner paths.

## Test sensitivity (measured, one mutation at a time)

Three mutations were applied to the production file, the `accept_t452_r02_`
selection re-run, and the file restored byte-identically each time
(`shasum d2c127e8…`):

| mutation | caught by |
| --- | --- |
| the tangler default radius becomes 9.0 m instead of the image's 10.0 | `accept_t452_r02_a_tangler_is_an_area_with_the_image_default_radius`, `accept_t452_r02_retail_the_measured_facts_read_the_original_rows` |
| `projectiles_per_launch` becomes 2 (one projectile expands) | `accept_t452_r02_a_volley_launches_one_projectile_per_sub_munition`, the retail test |
| an absent `TIME` is labelled `RecordTime`/`RecordKey { TIME }` instead of the image's 5.0 s default | `accept_t452_r02_a_time_the_record_does_not_state_is_the_image_default` |

The 24 image pins are the second half: a changed VA, a changed constant or a
moved store fails `accept_t452_r02_the_image_pins_every_measured_site` rather
than passes. The retail test re-reads the owner's installation through the
production table reader, so a record that grows a `RADIUS`, loses `LOCK_ON` or
changes `CLUSTER_SIZE` fails the measured expectations.

The selection was run with `--include-ignored` and `CS_GAME_DIR` /
`CS_ENGINE_IMAGE` set: 11 tests, 11 passed, 0 failed.

## Evidence

`private/evidence/T452-R02/acceptance.json`, committed as
`docs/findings/evidence/T452-R02.json`, validates with
`python3 tools/validate_evidence.py private/evidence/T452-R02/acceptance.json
--artifact-root private/evidence/T452-R02 --require-pass`. It records
`capabilities: ["retail", "synthetic"]`, the recorded acceptance log, the
installation digests measured by `cs_assets::install` discovery (never typed
in) and this task's claim.

## Not claimed

No original-run behavior; `retail` here is read access to original files and
static analysis of the owner's decrypted image, never evidence that the original
executable ran. No statement that the original applies any of these effects in
flight, that a tangler slows a plane, that smoke hides anything, or that a
`CLUSTER_SIZE` salvo reaches a target. What is verified is what the image's
**code** does at the cited windows and what the records **declare**, with every
unmeasured consequence named rather than defaulted. This awards at most
**checked**; no agent review replaces the owner's human approval.
