# T452-R.01 (#2159) — original ordnance launch geometry

Split subtask of #2156 (`T452-RUNTIME-REGISTER`), resolving
`t452_04.limit.velocity_unit` and narrowing `t452_04.limit.release_delay`,
`t452_04.limit.no_expires_key` and `t452_04.limit.rocket_flag_missing`.

Every statement is **static analysis** of the owner-supplied decrypted
executable (`$CS_ENGINE_IMAGE`, base `0x400000`; linear disassembly with
Capstone, not committed). Nothing is original-run behaviour; claim level
`implemented`/`checked`, never `verified_original`. The byte windows and the
closed caller inventory are pinned by
`accept_t452_r01_image_pins_the_launch_geometry` and the committed
`[[ordnance_launch_fact]]` rows of `data/owner-static/engine-facts.toml` are
re-read against the image by
`accept_t452_r01_the_committed_rows_match_the_engine_image`
(`crates/cs_content/tests/accept_t452_r01_launch_geometry.rs`).

## Vocabulary

The weapon record (`ebx` in the parser `0x5ad500..0x5ae200`; the zero-filled
record is allocated at `0x5ad7bf`, proven by the `+0x74` flag word's
read-modify-write stores) holds:

| offset | meaning |
| --- | --- |
| `+0x14` | `1.0/FIRE_RATE`, seconds (`0x5adcc9..0x5adcd2`) |
| `+0x1c` | `RANGE`, metres; `+0x20` its square (`0x5adfb5`) |
| `+0x2c` | `VELOCITY`, metres/second, post-override (`0x5ae0b8`) |
| `+0x38` | `ACCELERATION`, m/s² (`0x5ad900..`) |
| `+0x44` | `DETONATION_DISTANCE` squared (`0x5adbaf`) |
| `+0x48` | `DETONATION_TIME`, seconds (`0x5adbd0`) |
| `+0x54` | `GRAVITY`, m/s²; `+0x200` keeps `VELOCITY²/(2·GRAVITY)` |
| `+0x74` | flag word (`0x2`/`0x4` BEAM, `0x80` EXPIRES, `0x800` INSTANT, `0x8000` LOCK_ON, `0x2000000` DETONATE_AT_RANGE) |

The spawned projectile node (`edi` in the spawn `0x5aef40`, `esi` in the step
`0x5afd50`) carries the velocity vector at `+0x60..+0x68`, the speed scalar at
`+0x640`, the target speed at `+0x644`, the distance flown at `+0x664` and the
age at `+0x668` (T2117).

## VELOCITY — measured: metres per second

The parser writes the declared `VELOCITY` to record `+0x2c` (`0x5ae0b8`;
skipped when the BEAM bit `0x2` of `+0x74` is set, `0x5ae0ac`) and defaults it
to `120.0` (`0x5ad812`). At spawn, `+0x644` takes `+0x2c` verbatim
(`0x5af0e7`) and the per-step displacement is `dt · node+0x60` componentwise
(`0x5afe3f`, `fmul [esi+0x60/0x64/0x68]`). The `+0x60` vector is built from
`+0x2c` and metres per second is the only reading consistent with the F16-E
world unit (metres): `VELOCITY` is **m/s along the launch direction**.

Two parser overrides can replace the declared value; the production reader
`original_launch_geometry` mirrors the measured rule order:

- `BEAM` sets flag bits `0x2|0x4` (`0x5ad942`), prewrites `+0x2c = 10000.0`
  (`0x5ad950`), then stores `1/TIME_TO_MAX_RANGE` when that key parses
  positive (`0x5ad9fc`), restores `10000.0` when it parses ≤ 0 (`0x5ad9f3`),
  and — key absent — reciprocals the `10000.0` to `1e-4`. The stored value is
  a rate, not metres per second. No retail record declares `BEAM`.
- `GRAVITY` declared **non-zero** (`+0x54 != 0`, gate `0x5ae4df..0x5ae4ed`)
  overwrites `+0x2c` with `sqrt(2·GRAVITY·RANGE)` — a bit-level square root
  (`sar`+`0x1fc00000`, `0x5ae505..0x5ae520`) — and separately keeps
  `VELOCITY²/(2·GRAVITY)` in `+0x200` (`0x5ae4ef`). **Every retail record
  that declares `GRAVITY` declares `GRAVITY 0`** (`wep_04`, `wep_25`–`wep_28`):
  the override is engine-real but unexercised by the data, so those records
  keep their declared `VELOCITY` and the step's `dt·+0x54` pull is zero.

`t452_04.limit.velocity_unit` is therefore **resolved** and no longer raised
on component rows.

## Inheritance — measured: two paths

The spawn `0x5aef40` (six direct callers, closed inventory in the pin test)
takes the launcher's direction at `[arg5]+0x17c`, position `+0x188` and
velocity `+0x194` and writes the node's `+0x60` velocity:

- `ACCELERATION != 0` and `INSTANT` clear (gate `0x5af124`): `+0x640 =
  VELOCITY`, `+0x644 = VELOCITY`, then `0x5389a0(base, dir, scalar, out)` —
  the multiply-accumulate `out = base + dir·scalar` — computes `+0x60 =
  shooter_velocity + dir·VELOCITY` (`0x5af144..0x5af15a`). **Full
  inheritance, fraction 1.0.** Retail: `wep_04`, `wep_25`, `wep_26`, `wep_27`
  (`ACCELERATION 150`).
- `ACCELERATION == 0` or `INSTANT` set (`0x5af315..0x5af3a3`): `+0x640 =
  1e-4`, `+0x60 = shooter_velocity`, then each component is reduced by
  `dir·|shooter|` — the launch-direction part of the launcher's velocity is
  **removed**, not added; a straight-ahead launch keeps only the lateral
  remainder and accelerates to `+0x644` from near zero. `INSTANT` computes
  the same vector but the step ignores it, displacing by `RANGE·dir`. Most
  retail records take this path: `ACCELERATION 0` is as common as the key
  being absent (`wep_13` declares no `ACCELERATION` at all).

During flight `+0x640` approaches `+0x644` at `dt·ACCELERATION` (`0x5afd70`),
and the steering path renormalizes `+0x60` to the scalar through the same
`0x5389a0` helper (`0x5afe1b`).

## FIRE_RATE — measured: a reciprocal refire interval

The parser stores `1.0/FIRE_RATE` at record `+0x14` (`0x5adcc9..0x5adcd2`) —
an **interval in seconds**, not a rate. In the mount fire-control loop
(`0x4b6820..0x4b6eaa`), each `0x30`-byte mount entry takes `next_fire =
shared_clock + mount+0x14` (`0x4b6b2d`, `fld [0x71c470]; fadd [edi+0x14];
fstp [edi+0x10]`), and an alternating mount biases its next-fire by
`−0.5·interval` (`0x4b6d26`, `fmul [0x6032e0]` = 0.5f). Mount entries live in
the launcher vector `+0x268..+0x26c` with the mount id at `+0x4` and the
enabled byte at `+0x2c` (`find_mount` `0x4b2080`, stride `0x30`).

`FIRE_RATE` is therefore a **refire interval**, not cluster spacing and not
unused. What is **not pinned** is the record→mount field binding: no site yet
measured copies weapon `+0x14` into mount `+0x14`, so
`t452_04.limit.release_delay` stays, narrowed to the binding question.

## Lifetime — defaults measured, the claim stays

The parser defaults `RANGE` to `500.0` (`0x5ad80b`) and `DETONATION_TIME` to
`-1.0` (`0x5ad82a`). At the end of `RANGE` (`0x5b01e6` compares `+0x664` to
`+0x1c`) the flags rule is T2117's: `LOCK_ON` without `EXPIRES`, or
`DETONATE_AT_RANGE`, detonates (`0x5ac3a0`); otherwise the node is removed
silently by clearing the state word `+0x20` (`0x5b0318`).

`wep_13` declares no `RANGE`, `VELOCITY` or `ACCELERATION`: it flies at the
`120.0` default over the `500.0` default on the lateral-remainder path.
`wep_15` declares `VELOCITY 1`, `DETONATION_TIME 2`, `ACCELERATION 0`, no
`RANGE` — a 1 m/s record whose 2 s timer ends it long before its 500 m
default. `RANGE + VELOCITY + ACCELERATION` bound the *distance* a record can
fly but the member still states no flight duration, so
`t452_04.limit.no_expires_key` stays: the time to `RANGE` depends on the
acceleration curve and the shooter's speed, which no file states.

## Mounts — the screen layout measured, `wep_25` unresolved

The screens' own declarations stand (`RKA[8]` rocket slots, `QKA[4]`
ammunition slots, `DT[2]` hardpoint points; T452.04). The engine side
measured here: launch args come from the launcher object's `+0x17c`/`+0x188`
/`+0x194`, and mount entries are `0x30` bytes keyed by mount id — but the
loadout code that maps a `wep_` record to a slot index is not pinned, and
`wep_25` (the glidebomb, `LOCK_ON`, `GRAVITY 0`, no `ROCKET` flag) still has
no defensible slot. `t452_04.limit.rocket_flag_missing` stays.

## What the production reader now states

`cs_content::ordnance` carries the measurement as
`OriginalLaunchGeometry`/`original_launch_geometry`/
`original_launch_geometry_table`: the effective `+0x2c` speed
(`OriginalLaunchSpeed::{Declared, Defaulted, Ballistic, Beam}`), the stored
`1/FIRE_RATE` interval, `ACCELERATION`, the `RANGE` bound
(`OriginalRangeBound::{Declared, Defaulted}`) and the inheritance path
(`OriginalInheritance::{Full, LateralRemainder, Instant}`). Only
`t452_04.limit.velocity_unit` is removed from generated rows; every other
claim id is kept parseable.

## Remaining unknowns (all carried, not dropped)

- `t452_04.limit.release_delay`: the weapon `+0x14` → mount `+0x14` binding
  is not pinned (all 16 rows).
- `t452_04.limit.no_expires_key`: no stated flight duration (15 rows).
- `t452_04.limit.rocket_flag_missing`: `wep_25`'s slot (1 row).
- The mount-entry init that fills `+0x14` from a weapon record, and the
  record→slot loadout map, are follow-up work (#2156 scope).
- `0x57a090`/`0x57a130` (the record key lookup and value parse helpers) are
  used as named primitives; their internals were not needed.
