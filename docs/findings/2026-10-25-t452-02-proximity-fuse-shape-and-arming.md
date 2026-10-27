# T452.02: proximity fuse trigger shape, sampling and arming (static)

Date: 2026-10-27. Task: #2128. Predecessors: `2026-10-11-f28-c-2-lost-target-blast-measured.md`,
`2026-10-18-t1318-blast-unknowns-measured.md`, `2026-10-25-f28-guidance-loss-per-weapon-audit.md`.
Capabilities: `retail` and the decrypted executable `$CS_ENGINE_IMAGE` (base `0x400000`). Static
analysis only: nothing here is an original run, so nothing is `verified_original`. No new
`engine-facts.toml` rows (the task does not list that file); the pins are byte windows in
`crates/cs_content/tests/accept_t452_02_proximity_fuse.rs`.

## Shape — a sphere, point-sampled once per step (not swept)

In the per-projectile step (`0x5af720`, calling `0x5afd50` at `0x5af816`), the test at
`0x5b0216..0x5b0247` computes `0x538880(&projectile+0x48, target)`, the **squared Euclidean
distance** between two 3-float positions, and bursts when it is `<=` `weapon+0x44`
(`DETONATION_DISTANCE` squared). So the fuse is a **sphere of radius `DETONATION_DISTANCE`
centred on the target's position, boundary inclusive**. No directional or shaped term exists on
this path.

It is a **point sample**: `0x5afd50` runs before the step applies its displacement. The
displacement is added afterwards, `+0x54 = +0x48 + displacement` (`0x5af854..0x5af886`), so the
sampled projectile position `+0x48` is the position at the *start* of the step, and the target
position is read at that moment. No segment, no relative-motion closest approach. The burst
effect is placed at `+0x48` too (`0x5b0249..0x5b0298`).

`cs_sim`'s `TargetPath`/`closest_approach` model (swept target and projectile, relative motion)
is therefore **not** what the original does: it detects passes the original misses (a target
crossing between two samples) and, because it uses the end-of-step position, fires up to one
step earlier or later than the original. Whether the difference matters at the original's step
rate is for F28-D to decide; this task changes no `cs_sim` code. Record: `OriginalProximitySampling::PointSamplePerStep`.

## Arming — no arming condition other than the live conditions

The proximity burst is reached only when all hold (`0x5b01ec..0x5b0247`):

| condition | test |
| --- | --- |
| distance flown `projectile+0x664` below `RANGE` (`weapon+0x1c`) | `0x5b01ec` |
| weapon flag `0x8000` (`LOCK_ON`) | `0x5b0216` |
| a held target, `projectile+0x70` non-null | `0x5b021b` |
| `DETONATION_DISTANCE` squared (`weapon+0x44`) > 0 | `0x5b0222` |

There is no time test (the `DETONATION_TIME` timer is a separate, parallel trigger at
`0x5b029a`), no minimum distance and no `RANGE_MINIMUM` test on this path. The burst call
`0x5ac3a0` is reached directly (`0x5b0310`). `RANGE_MINIMUM` is tested in the *impact* path
(`0x5b063d`: flag bit 31, `[1] == 0`, travelled `< [0]`: detonation suppressed; `[1] != 0`:
blast scale multiplied), which is `T1318` Q2's finding. Of the eleven guided records with a
fuse only `wep_14` (torpedo, `RANGE_MINIMUM 300, 0`) declares a window; its proximity fuse
(radius 1 m) is not gated by it, only its impact is.

Per record (`OriginalRocketBlast::proximity_fuse`, radius m / live below `RANGE` m):
wep_04 30/900, wep_05 15/1000, wep_06 15/1000, wep_07 50/1000, wep_08 35/1000, wep_09 30/900,
wep_10 15/1000, wep_11 15/10000, wep_14 1/1200, wep_24 15/1000, wep_27 20/900. wep_15 (flare)
has no `DETONATION_DISTANCE` and no fuse.

## Still unknown

* Where `+0x54` becomes `+0x48` for the next step (the copy was not located in the
  `0x5af720..0x5b0770` scan; it is assumed from the layout). Claim `t452_02.limit.position_advance`;
  affects the sampling phase of every fuse, not its shape.
* The target position the pointer at `+0x70` addresses (turret mount node versus vehicle
  centre) is the T2116/T2120 question; this task only shows the test reads three floats from it.
* The unnamed weapon flag `0x8` guarding the `RANGE_MINIMUM` node call at `0x5b0196`/`0x5b0193`
  (sets a node query-exclusion flag while travelled `<=` `RANGE_MINIMUM[0]`): its meaning and
  effect on proximity were not followed.

## Not claimed

No original-run behaviour, no `cs_sim` change. Awards at most **checked**.
