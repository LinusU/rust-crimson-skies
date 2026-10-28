# T452-R.04 (#2169) — ordnance flight duration and the weapon-to-mount release binding

Split subtask of #2156 (`T452-RUNTIME-REGISTER`), resolving
`t452_04.limit.no_expires_key` (15 entries) and `t452_04.limit.release_delay`
(16 entries, 14 resolved, 2 kept) and `t452_r01.limit.mount_interval_source`.

Every statement is **static analysis** of the owner-supplied decrypted
executable (`$CS_ENGINE_IMAGE`, base `0x400000`; linear disassembly with
Capstone, not committed). Nothing is original-run behaviour; claim level
`implemented`/`checked`, never `verified_original`. The byte windows are the
`[[ordnance_flight_fact]]` rows of `data/owner-static/engine-facts.toml`,
re-read against the image by
`accept_t452_r04_the_committed_rows_match_the_engine_image`
(`crates/cs_content/tests/accept_t452_r04_flight_and_release.rs`).

## 0. Correction to T452-R.01: the spawn inheritance paths were swapped

The spawn gate `0x5af124` compares `ACCELERATION` (`+0x38`) with `0.0` and
takes `je 0x5af315` when they **differ**. So `ACCELERATION == 0` (or absent)
falls through to full inheritance (`+0x60 = shooter + dir·VELOCITY`, constant,
no ramp), and a non-zero `ACCELERATION` takes `0x5af315`: start at the
launcher's speed and ramp. T452-R.01 had them swapped, which would have left
every gun at `1e-4` m/s forever. `OriginalInheritance`, its tests and
`2026-10-28-t452-r01-ordnance-launch-geometry.md` are corrected in this task.

## 1. FIRE_RATE and the release interval — the binding

**The mount's `+0x14` is not weapon record `+0x14`.** The mount builder
`0x4b59b0` (one caller, `0x47b089`, the vehicle-record loader) looks up the
`weapons` key (string `0x62b128`) of a vehicle record and, for every row, builds
one `0x30`-byte mount entry:

| entry | source |
| --- | --- |
| `+0x00` | the weapon record found by name (`0x5abfd0`) |
| `+0x04` | mount id = `name[4]*10 + name[5] - 0x210`, the two digits of `wep_NN` (so `wep_130` is mount 13, the id of `wep_13`) |
| `+0x08` | row element 1 (the count the fire loop tests `> 0`) |
| `+0x14` | **row element 2 — the refire interval, seconds** (`0x4b5a79`) |
| `+0x18`, `+0x1c` | squares of row elements 3 and 4 (meaning not measured) |
| `+0x2c` | enabled byte, 0 on construction |

A row is `(wep_NN count interval value value)`: `(wep_04 1000 1.5 50 10000)`
in `player_airplane`, `(wep_07 2 200 30 800)` on a `firebrand`. Weapon record
`+0x14` (`1/FIRE_RATE`) has **one** reader in the weapon module: the projectile
pool size, `pool += ftol(RANGE / VELOCITY / (1/FIRE_RATE)) + 1` for every
non-`BEAM` record (`0x5ae564`), the pool being `[0xa1d790]` nodes of `0x694`
bytes (`0x5ae5a2`). The scan is the same negative search as T452-R.03-B (a
second `fdiv [ebx+0x14]` is pinned absent in `0x5ac000..0x5af000`, the 18
callers of `0x5abfd0` scanned forward for `+0x14` reads of a record pointer);
it is not proof against a read through a copied pointer. So `FIRE_RATE` does not
set a release interval at all; the airframe row does.

**What the interval delays.** On a release the fire loop sets the mount's
next-fire `+0x10 = shared_clock + mount+0x14` (`0x4b6b2d`) and, when the flag
bit it tests into `bl` (bit `0x40` of the first dword of the `record+0x210`
object) is **clear**, also the launcher-wide next-release time `launcher+0xec`
to the same value; a clear-bit mount is disabled while the clock is below
`launcher+0xec` (`0x4b6ad4`). The interval therefore delays the **next** release
(of that mount, and of the launcher's clear-bit mounts), never the current one.
`0x4aab00` copies the player launcher's mount `+0x14` into a slot object's
timers (`+0x1a0`, `+0x1a4`, default `1.0`, halved when its flag argument is
set).

**Retail.** `cs_content::ordnance::read_original_mount_rows` reads every row of
every airframe through the `kind_of` chain. Player (`player_airplane`)
intervals: guns `0.012` (`wep_00`) to `0.166` (`wep_70`..`73`), rockets
`wep_04`..`wep_13`, `wep_15` `1.5` s, `wep_14` `3` s, `wep_25`/`27`/`28` `1` s.
The AI airframes state `30` or `200` s for rockets and `0.05`–`0.06` s for guns.
They disagree with record `1/FIRE_RATE` (`wep_00` 0.095, `wep_04` 1.0), which is
why the claim could not be settled by the record.

Resolution: `t452_04.limit.release_delay` is recorded against a component row
**only when no airframe row names the record** — `wep_24` and `wep_26` (2 of the
16 rows); the other 14 have their interval stated per airframe by the rows.
`t452_r01.limit.mount_interval_source` is resolved by the table above.

## 2. Flight duration — the rule, per record

The step `0x5afd50` adds `|dt·node+0x60|` to the path length (`node+0x664`,
`0x5b0160`), the age (`node+0x668`) grows by the frame `dt` (`0x5b0863`), and the
flight ends at the first of (the range-end behaviour itself is T452-R.02's):

1. path length `>= RANGE` (`0x5b01ec`; default 500 m);
2. below `RANGE`, `DETONATION_TIME > 0` and age `> DETONATION_TIME`
   (`0x5b029a`; default `-1.0`, no timer);
3. `INSTANT`: the first step displaces by `RANGE·dir`;
4. `MINE` (key at `0x63d070`, flag `0x4000`): the ramp is skipped (`0x5afd6e`)
   and the path length is **halved every step** (`0x5b01cc`), so it never reaches
   `RANGE`; only a timer ends it.

The path length is counted along `node+0x60`:

- **Full inheritance** (`ACCELERATION` zero or absent): constant `shooter +
  dir·VELOCITY`; time to range `RANGE / |shooter + dir·VELOCITY|`.
- **Ramping** (`ACCELERATION != 0`): scalar `s = |shooter| + 1e-4` rising at
  `ACCELERATION` to `VELOCITY + |shooter|` (`0x5af350`, `0x5afd70`); distance
  `s·t + a·t²/2` to the ramp end `t = VELOCITY/a`, then `VELOCITY + s`.

`OriginalLaunchGeometry::flight_duration(shooter_velocity, launch_direction)`
implements this; the derived value depends on the launcher's velocity at release,
which is a runtime input, not a data gap. From a stationary launcher: `wep_04`
3.5 s (3 s ramp covers 675 m, then 225 m at 450 m/s); `wep_13` `500/120` s;
`wep_15` the 2 s timer; `wep_00` 1 s. No retail record is `MINE`, `INSTANT` or
`BEAM`. The step ends a flight on the first step at or after the continuous
time, so a runtime rounds up to its tick.

Not modelled (named, not hidden): `LOCK_ON` steering with a live target and
`RANDOM_DEVIATION` (`0x5afe5a..`) rotate `node+0x60`; the duration is for an
unguided straight flight. The ramp renormalizes with `base = node+0x30`, which
no store in `0x5ac000..0x5b2000` writes (nodes come from a calloc'd pool and a
free list), taken as zero; a negative search.

`t452_04.limit.no_expires_key` is therefore **resolved**: it is no longer
recorded on component rows. The nitro catalogue row keeps its own use of the
constant (a booster is actor state, not a projectile).

## Remaining unknowns (carried)

- `t452_04.limit.release_delay`: `wep_24`, `wep_26` (no airframe row names them).
- `t452_04.limit.rocket_flag_missing`: `wep_25`'s slot (unchanged).
- Row elements 3 and 4 (squared at entry `+0x18`/`+0x1c`): meaning not measured.
- The flag bit `0x40` of the `record+0x210` object that selects a launcher-wide
  versus per-mount release gate has no name.
