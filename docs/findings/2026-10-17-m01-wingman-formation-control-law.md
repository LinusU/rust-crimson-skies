# M01-WINGMAN-FORMATION-A: the original follower AI and its control writer (#1563)

Date: 2026-10-17. Task #1563. Capabilities used: `retail` (one read of the
`vehicle.zrd` `wingman` record) and `CS_ENGINE_IMAGE` (static reading of the
decrypted image, sha256 `43540fc9…6d75`, address convention `VA = file offset
+ 0x400000`; capstone linear sweep plus radare2). Implementer: sonnet-1. Every
claim is `static`; nothing is `verified_original`, no original executable ran,
and **no runtime code changed**. The constants are committed as
`[[ai_follow]]` rows of `data/owner-static/engine-facts.toml` (owner exception
of 2026-10-14, rule 3) and pinned to the image by
`accept_m01_wingman_formation_a_rows_match_the_image`.

This note replaces the lead of `2026-10-17-m01-wingman-formation-premise.md`
(task #1550): `0x48c220` is a flight-force term, and the follower law is
**`0x41e760`**, found below.

## 1. The chain from the vehicle loop to the control inputs

| step | address | measured |
| --- | --- | --- |
| vehicle-list loop | `0x4898e7`..`0x489c43` | for an *active* vehicle (`+0x944`) in mode 0, 1 or 4 it calls `0x41c270` (`0x489b14`) |
| AI step | `0x41c270` | skips when `+0xcc` is set; refreshes the route (`0x41fe10`); then `+0x67c == 4` → `0x41e760` (`0x41c2dd`..`0x41c358`); otherwise dispatches on the AI state `+0x2f0` (0/2 → `0x41d1f0`, 1 → `0x41d9f0`) |
| mode-4 follower | `0x41e760` | **this note**: picks a goal point and a reference velocity |
| control writer | `0x41b560` | `ret 0x14`, `(this, goal*, refvel*, params*, bool, bool)`: writes the throttle target `+0x124` and the stick axes `+0x100/+0x108/+0x10c` → `+0x114/+0x11c/+0x120` |
| per-vehicle physics step | `0x489ea0` → `0x48e580` | reads `+0x124` (target) and slews `+0x128` (actual) toward it at `0x71c56c × 0.5` per tick scalar (`0x48e63f`..`0x48e6c3`) |

`0x487460` is the *human* input handler (key tests through `0x537c10`, writes
`+0x124` with the constants 1.0 / 5.0 / −5.0); the AI never goes through it.

### Mode 4 is `mode: wingman`

The `vehicle.zrd` parser (`0x47afb4`..`0x47b077`) maps the record's `mode`
text to the def word `+0xa4`: `jet` 0, `heli` 1, `tank` 2, `ship` 3,
**`wingman` 4** (`mov [edi+0xa4], 4` at `0x47b05c`), `plane` 5. The vehicle
constructor copies it to `vehicle+0x67c` (`0x475abf`). Retail
`ZBD/zrdr.zbd#vehicle.zrd` record `wingman` (the record both `wingman_2` and
`wingman_3` resolve to by their name prefix, aiv-layout §3) carries
`mode: wingman`, `kind_of: devastator`, `activation: 100000`,
`target_bias: -100`, `struct_bias: -200`. The `devastator` and `rusdevastator`
records carry no `mode`, so they keep the def default and do **not** run this
routine. The loader stores the literal `player` as the primary target when a
mode-4 record has none (`0x47c6c8`..`0x47c6e5`) — which is why `+0x2fc` can
be the player.

## 2. `0x41e760`: the follower

`leader = vehicle+0x2fc → +4` (the resolved primary-target vehicle).
`edi == [0x71c298]` (the player) selects the station vector; otherwise the
routine **forces** the sub-state `+0xd8 = 2` and `+0xdd = 1` on every call
(`0x41e7a1`..`0x41e7b6`).

`+0x358` guard (`0x41e7bd`): `== 3` → goal = own position + 1000 m up
(`0x603464`) with the zero reference velocity, then return; `== 4` → return
without writing any control.

### Station vectors

Rotated into the world by the leader's 3×3 basis at `leader+0x180` (rows of
three floats, row 2 at `+0x198` is the forward axis the speed code reads the
climb term from) and added to the leader position copy at `leader+0x1a4`:

| leader | local vector `(x, y, z)` | VA |
| --- | --- | --- |
| the player | `(6, 0, 18)` | `0x61fb88` |
| any other vehicle (wingman → devastator) | `(8, −2, −8)` | `0x61fb98` |

So a follower of a non-player leader is aimed at a slot **8 m along basis
row 0, 2 m along −row 1, and 8 m behind the leader along row 2**. Which side
row 0 is on, and that row 1 is up, are *not* measured (the basis layout is
the flight code's, not read here). The player-leader vector is a point
18 m *ahead*: it is the aim point of a pursuer (`bsfury_1`'s primary target
is the player), not a formation slot.

### Sub-state `+0xd8` (two jump tables)

First table (`0x41f00c`, transitions) and second (`0x41f020`, goal):

| state | transition (first table) | goal (second table) |
| --- | --- | --- |
| 0 | `0x41e834`: leader awake (`+0x91d` clear), squared distance to the leader `< 490 000` (700 m), own speed `+0x934 > 20.576` → state 1 (and a one-shot cue through `+0xdc`) | `0x41ea9c`: route-object based (below) |
| 1 | none | **station**: slot from the table above; reference velocity = the leader's velocity (virtual slot `+4`, `0x41edba`) |
| 2 | `0x41e8bb`: no route object (`+0x948 == 0`) → state 1; with a route object, state 1 only when `dist² + (3·Δy)² >` 1 440 000 (leader is the player) or 640 000 (otherwise) | `0x41ec5a`: route-object based |
| 3 | `0x41e9ec`: route object set → 2, else squared distance to `+0xe0` `< 2500` → 4 | none (reference velocity zero) |
| 4 | `0x41ea2d`: route object set → 2, else squared distance `< 2500` → 1 | station, as state 1 |

A follower whose leader is not the player is forced to state 2 at the top of
every call, so each tick it ends in **state 1 (station) when it has no route
object, or when it is farther from the leader than 800 m (3-D with the
vertical weighted ×3), and stays in state 2 (route following) otherwise.**
The route object (`+0x948`) is created by `0x41fe30`/`0x41f9c0` from the net
graph (`+0x2e4`/`+0x344`); whether M01's wingmen ever receive one is
**unknown** (see §5).

### Goal clean-up before the call

If the squared distance to the leader is non-zero and `< 6400` (80 m), the
goal is displaced by the vector at `[ebp-0x40]` rescaled by `80 / √d²`
(`0x41ee35`..`0x41eeba`). On the station path nothing in this routine writes
that stack vector before the use, so the displacement direction is
**unrecovered**.

The routine then calls `0x41b560(this, &goal(+0xe0), &refvel, 0x61fb28, 0, 0)`
(`0x41eed3`). With a route object it first runs a line-of-sight test
(`0x460be0`) against the route and may fall back to `0x41afe0`/`0x41f420`
(abandon the route); without one it returns.

## 3. `0x41b560`: speed matching, catch-up and the throttle

Inputs: goal `G`, reference velocity `V` (the leader's), parameter block `P`
(eight floats at `0x61fb28`, committed as `follow_*` rows). Measured steps:

1. **Altitude clamp.** `G.y` is limited to the vehicle's own ceiling
   `+0x314` and raised to the world floor `[0x71c3f0]`; a clamped `G.y`
   zeroes `V.y` when `V.y` was upward (`0x41b57e`..`0x41b5f1`).
2. **Demanded speed.** With `Δ = G − position`, `û = V/|V|`, `along = Δ·û`,
   `cross = √max(|Δ|² − along², 0)`:
   `speed_wanted = |V| + P[4]·along + P[5]·cross` = `|V| + 0.15·along +
   0.025·cross` (`0x41b695`..`0x41b743`); `|V| = 0` gives a fixed 80.4672 m/s
   (180 mph, `0x41b6af`).
3. **Band.** `speed_wanted` is clamped to `|V| ± 26.8224 m/s` (60 mph,
   `0x603510`, `0x41b85b`..`0x41b891`).
4. **Caps.** Then capped at `vehicle+0x668 × P[1]` (= 1.5 ×, `0x41b893`),
   floored at 22.352 m/s (50 mph, `0x60351c`, `0x41b8ad`), and limited by the
   vehicle definition's two speed words at `def+0x1e4` / `def+0x1e8`
   (`def = vehicle+0x64`, `0x41b8c6`..`0x41b8fa`). What those words and
   `+0x668` are is not measured (§5).
5. **Throttle target `+0x124`** (`0x41bb5c`..`0x41bbfd`):
   * if the player is **farther than 2000 m** (`dist² > 4 000 000`,
     `0x60350c`) the target is set *directly* to `speed_wanted / +0x668`
     (`0x41bb8e`);
   * otherwise it **slews**: `+0x124 ± [0x9ad744] × 0.35` (`0x603508`), up
     when the current speed `+0x934` is below `speed_wanted`, down when not
     (`0x41bb99`..`0x41bbc0`);
   * either way it is clamped to `[P[0], P[1]] = [0.4, 1.5]`.
6. **Stick.** The goal direction in the body frame (Δ rotated by the
   transpose of the `+0x180` basis, `0x41ba2e`..) is reduced to a unit
   `(x, y)` aim plus a forward component; the three axes are written to
   `+0x100`, `+0x108`, `+0x10c`, multiplied by the pilot gains `+0x8c4..+0x8d0`
   (or `+0x8dc..` for the `bool` variant), clamped to the limits
   `+0x8d4..+0x8d8`, and copied through the identity helper `0x460890` into
   `+0x114`, `+0x11c`, `+0x120`. The bang-bang structure is clear; the
   thresholds `P[2]`, `P[3]`, `P[6]` and the exact axis-to-aircraft-axis
   mapping are **not** decoded here.

## 4. What this says about `wingman_2` (Betty) and `wingman_3`

Betty is the record titled `MSG_BETTY_NAME` (`wingman_2`, leader
`devastator_2`); `wingman_3` follows `devastator_3` (aiv-layout §5).

* Both run `0x41e760` with a non-player leader, so they aim at
  `leader + R·(8, −2, −8)`, and **there is no code path in the follower that
  leaves them at their stored 689 m / 1021 m stations**: demanded speed is
  `|V_leader| + 0.15·(gap along the leader's velocity)`, clamped to at most
  `+26.8224 m/s` over the leader. A 700 m gap therefore asks for the maximum
  closing speed, so the *designed* closing rate of the original is
  ≤ 26.8 m/s (60 mph) over the leader, subject to the airframe reaching it
  (cap `1.5 × +0x668`, throttle ≤ 1.5, throttle target slew 0.35 per
  `0x9ad744` unit, then the actual throttle `+0x128` slew of
  `0x71c56c × 0.5`). At that rate 689 m closes in ≈ 26 s and 1021 m in ≈ 38 s
  *if* the airframe can sustain the speed difference.
* Beyond 2000 m from the player the throttle target is set instantly to the
  demanded ratio (a rubber band); no stored M01 pose is that far.
* The leaders `devastator_2/3` are *not* followers (no `mode`), so their own
  motion is not this routine's; they are scripted/path vehicles.

This is a statement about the *law*, not an observation of the original. It
does not say that the original's wingmen are near the player in the first
seconds: the stored poses (689 m / 1021 m) are not changed by it, and the
route branch (state 2, below 800 m) is unmeasured.

## 5. Unknown (with the affected content and the resolving step)

| unknown | affects | resolving step |
| --- | --- | --- |
| whether M01's `wingman_2/3` ever hold a route object `+0x948` (state 2 vs state 1) | which of the two goal laws they fly inside 800 m of the leader | static trace of `0x41f9c0`/`0x420070` and the net data M01 loads, or a reference capture (#358) |
| the route-following goal (second table, states 0 and 2: `0x41ea9c`, `0x41ec5a`; the speed map `0x603584..0x603594`) | wingmen with a route | same |
| the meaning of `vehicle+0x668`, `def+0x1e4`, `def+0x1e8`, and the value of the frame scalar `0x9ad744` | the absolute closing speed and throttle slew in m/s | read the writers of those words / the per-frame setter; they decide whether 60 mph over the leader is reachable |
| the displacement vector `[ebp-0x40]` in the 80 m push | behaviour inside 80 m of the leader | decode with the stack frame of `0x41e760` |
| stick thresholds `P[2]`, `P[3]`, `P[6]` and the axis mapping | how the wingman banks and pitches onto the slot | decode `0x41bc2c`..`0x41bf98` against the flight handler |
| which side `row 0` of the leader basis is on, and the sign of row 1 | the lateral/vertical sign of the slot | the basis writer for `vehicle+0x180` |
| whether the first-seconds picture matches the original | the owner's "wingmen near the player" judgement | `human_play` / `REF-OWNER-FIRST-CAPTURE`; not available to an agent |

## 6. Consequence for the dependent tasks

#1564 (`M01-WINGMAN-FORMATION-B`) can implement §2/§3 as the follower law
for `mode: wingman` records: slot `(8, −2, −8)` rotated by the leader basis,
speed `|V| + 0.15·along + 0.025·cross` banded to `±26.8224` and floored at
22.352, throttle target clamped to `[0.4, 1.5]` and slewed at 0.35 per
frame-scalar unit. It must keep the §5 items declared as unknown or designed
(in particular the maximum speed mapping `+0x668`, and the absence of a
route-following law), and must not call the result `verified_original`.
