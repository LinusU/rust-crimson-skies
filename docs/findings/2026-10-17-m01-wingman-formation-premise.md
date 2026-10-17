# M01-WINGMAN-FORMATION (#1550): `0x48c220` is not the AI law; no formation law recovered

Date: 2026-10-17. Task #1550. Capabilities used: `retail` (aiv.zrd poses
already measured by #1453) and `CS_ENGINE_IMAGE` (static reading of the
decrypted image with radare2). Implementer: sonnet-1. Nothing here is
`verified_original`, and no runtime code changed: the task's acceptance
(wingmen near the player in the windowed M01, measured source recorded) is
**not met**; this note records what static reading established and where the
search has to continue.

## 1. Premise correction: `0x48c220` is a flight-force term, not the AI

Earlier notes (`2026-10-15-m01-play-ai-designed-law.md` §3,
`2026-10-16-m01-play-ai-restart.md`) call `0x48c220` "the original AI control
law". Reading it:

* One caller, `0x48cf95`, inside the flight-force routine `0x48c470` (the
  routine the per-mode flight handler `0x48e580` calls at `0x48e6da`).
* Signature `(vehicle, vec3 *accumulator)`. It skips when `vehicle+0x358 == 4`
  (`0x48c22b`), builds a vector from the vehicle's `+0x198` triple negated
  (`0x48c28f..0x48c2a5`), calls `0x48bf60` (a probe with a ~1.4 KiB frame), scales the
  result by globals `0x71c424/0x71c428` and **adds `vector * scalar` into the
  caller's accumulator** (`0x48c34b..0x48c369`), then a second part writes a
  clamped vector at `vehicle+0x924` via `0x460700`. It reads no target, no
  leader, no stick or throttle demand.

That is a force/torque contribution, not a decision routine. Gates removed
by this: none; but the "AI law is at `0x48c220`" claim in the two notes above
should not be used as a lead.

## 2. What the per-vehicle dispatch looks like (measured, `0x489ea0`)

`0x489ea0(vehicle)` is the per-vehicle step (called from the vehicle-list loop
at `0x4898e7..0x489c43`). Unless `+0x91d` (set) and `+0x91f` (clear) skip it,
and `+0xcc` diverts to `0x48a110`, it indexes a six-entry jump table at
`0x489f48` with the mode word `vehicle+0x67c` (`ja` rejects `> 5`):

| mode `+0x67c` | handler |
| --- | --- |
| 0 | `0x48e580` (flight integration; calls the force routine `0x48c470`) |
| 1 | `0x48ffe0` |
| 2 | `0x48a880` |
| 3 | `0x48b480` |
| 4 | `0x48e580` (same as 0) |
| 5 | `0x48b480` (same as 3) |

`0x49c920` (reached from the vehicle loop at `0x49c8f8` when mode is 4, the
leader pointer `+0x2fc` is non-null and its vtable slot `+0x38` answers true)
resolves a net (`+0x2e4`, `+0x2e8`, `+0x2ec`, via `0x431900`/`0x431e40`) and
then sets mode 0 (or `+0x2f0 = 2`). So mode 4 is "assigned to a net, not yet
placed on it" (the `SET_AI_NET` path), not "following a leader".

## 3. What the primary target is (measured)

`aiv.zrd` field "primary target" (`wingman_2` -> `devastator_2`, `wingman_3` ->
`devastator_3`) is stored as a **strdup'd name at `vehicle+0x2f8`**
(`0x475352..0x475371`, `0x453602`). It is resolved by `0x4a3360` into a
pointer at `+0x2fc` and the name freed (`0x45a9af..0x45a9d5`); `+0x2fc` is then
used through vtable calls (`+0x14` at `0x489a07`, `+0x38` at `0x49c8f0`, `+0x48`
as a deleter at `0x4a66b5`). The readers of `+0x2fc` found
(`0x41c9a1`, `0x41e5a6`, `0x41e780`, `0x41f048`, `0x41fea2..0x41ff3a`,
`0x47ffbf`, `0x486173`, `0x4899fd`, `0x48ba73`, `0x49c8e4`, `0x4a6654`,
`0x4b0ba9`, `0x4b83c4`) are ownership/liveness checks in what was read; **none
of them computes a station, offset or heading from the target**. The code that
turns that target into control inputs (stick, throttle) was not found.

## 4. Betty (measured from data, not from behaviour)

`aiv.zrd` record `wingman_2` carries title `MSG_BETTY_NAME`; `wingman_3`
carries `MSG_BUCK_NAME`; their leaders `devastator_2` / `devastator_3` carry
`MSG_BJOHN_NAME` / `MSG_TEX_NAME` (`2026-10-15-m01-aiv-layout.md` §5). By the
title key, Betty is `wingman_2`. The title's runtime consumers are still
unmeasured (aiv-layout §7.7), so this is a data reading, not a behaviour one.

## 5. Stored poses (from #1453's retail test), distances

The player is stored at (-3694, 1318, -12482), heading 180. All four flight
mates are stored *behind* it, so the data itself puts the flight astern and
the original must either close up through its AI or start that way:

| record | stored position | straight-line distance to the player |
| --- | --- | --- |
| `devastator_2` | (-3590, 1300, -12909) | ~440 m |
| `devastator_3` | (-4355, 1350, -12881) | ~773 m |
| `wingman_2` (Betty) | (-3421, 1350, -13114) | ~689 m |
| `wingman_3` | (-4531, 1280, -13065) | ~1021 m |

`WARP_VEHICLE` (the one directive that teleports a vehicle) is used by **no
M01 directive** (it appears only in M17; `2026-10-10-m17-b-compatibility-gaps.md`),
so M01 does not warp the wingmen next to the player by script.

(#1538 reports 733 m / 1037 m from the player's *measured* start, which differs
slightly from the stored pose used above; both put the wingmen 0.7-1 km away.)

## 6. Not done, with the resolving steps

1. **Find the code that writes the AI's control inputs** (throttle target
   `+0x124`/`+0x128`, stick) for a non-player vehicle in modes 0/4, and what
   it reads from `+0x2fc`. Static route: the writers of those fields outside
   the player-input path, or the callers of the vtable slots listed in §3.
2. **Measure the follow/formation constants** it uses (offsets, speed match,
   catch-up gain) as engine-facts rows with virtual addresses.
3. **Only then** implement the follow in `cs_app::mission_ai` from the
   measured law, and run the windowed M01 acceptance (needs `gpu`; the "near
   the player in the first seconds" judgement is the owner's `human_play`).

Nothing was spawned, moved or tuned in this task; the designed `STATION_OFFSET_M`
and `WINGMAN_THROTTLE_GAIN` of the AI-designed-law note stay designed.
