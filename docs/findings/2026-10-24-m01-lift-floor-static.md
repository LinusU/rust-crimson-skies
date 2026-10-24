# #1843 M01 free flight: the image has the same lift floor as our law, so "no lift from rest" is not the cause

Date: 2026-10-24. Static analysis only: `$CS_ENGINE_IMAGE` (read-only, never committed; sha-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, VA = file offset + `0x400000`) disassembled with
`llvm-objdump -d -M intel` into `private/`. The original executable was not run: every claim is `static`
(claim id `OWNER-STATIC-2026-10-24`), nothing is `verified_original`. Implementer: `sonnet-2` (Sonnet 5.5); no
independent review yet. Builds on `2026-10-21-m01-hand-over-pose.md` (#1746) and `2026-10-23-m01-handover-dynamics-step.md`.

## 0. Answer

The task asked for the engine source of the original's lift at rest (hand-over velocity, lift floor or a body hold).
**There is none: the image's force routine `0x48c470` has the same floor and the same fallback as `cs_sim`.** Changing the
law to give lift below `LIFT_MIN_SPEED_MPS` would contradict the image, so nothing was changed.

## 1. What the image does below the floor

* `0x48c4c2` loads the speed `veh+0x934` into `[ebp-0x80]`. For the player (`veh == [0x71c298]`, with `veh+0x384`
  clear) `0x48c50f` compares it with the f32 at `0x60811c` = **2.4384** (8 ft/s, `LIFT_MIN_SPEED_MPS`) and, when
  `speed <= 2.4384`, jumps to `0x48c857`. (The non-player branch before it compares the distance to the player with
  `1e6` (`0x607a18`) and takes the `+0x384` path instead; not the player's.)
* `0x48c857` is the low-speed path: it calls the aero routine `0x48fc40(veh, g = 0, 1, 0.7, 0.7, &force, &lift)`. The
  `0.7, 0.7` are the lift normal our law already falls back to, and `g = 0` is `g_command = 0`.
* `0x48fc40` (atmosphere `0x41aca0`, thrust `0x41acf0`, drag `0x41ada0`) then subtracts the gravity term at
  `0x48ff85..0x48ff9d`: `force.y -= [model+0xc4] / [0x6080d4] * veh+0x674`. So below the floor the body has thrust,
  drag and gravity and **no lift**, exactly as `cs_sim` computes.
* The normal path (`0x48c648..0x48c855`) steers velocity toward the nose-aligned target by `[0x71c448]` (lift accel
  rate), adds gravity, projects onto the right/up axes and uses `sqrt(ax^2 + ay^2)` as the demanded load; when both
  projections are zero it falls back to `(0.7, 0.7)` too. Same structure as `cs_sim`'s step 5.

## 2. What that leaves

A body released at the placement pose with `v = 0` falls in the original as in ours. The capture's 0.2 m sink in the
first second is then **consistent with free fall for ~0.14 s** (`0.5 * 20 * 0.14^2`): the dynamics starting late in
the capture's first second (its clock is read +-1 s). It is not evidence of lift. That is a reading, not a source: the
start of the dynamics is #1978's open `launch-flag-clear-site`.

The remaining divergences (cruise 113.5 m/s, sink 5.6 m/s, 5 s spool-up) are not explained by the floor either: from
rest at throttle 1.0 our law sinks 28.8 m/s (`2026-10-23-m01-handover-dynamics-step.md` section 4), so something other than
the floor (the hand-over throttle, attitude or angular state) differs. Those need the owner's per-frame capture of
`veh+0x124`, `veh+0x128`, `veh+0x924` and `veh+0x150` from the cinematic end.

## Instruction bytes cited

```bytes
48c4c2: 8b96340900008955800f85c2  # mov edx,[esi+0x934] ; [ebp-0x80] = speed ; jne (veh+0x384 set) 0x48c593
48c50b: c6451b00d81d1c816000dfe0f6c4410f  # fcomp [0x60811c] (2.4384) ; test ah,0x41 ; jne 0x48c857 (speed <= floor)
48c857: c745dc00000000c745e43333333f  # low-speed path: g command = 0, normal 0.7
48c86c: 8b55e08d4508508b45e48d4d9c518b4ddc52506a015156e8  # push args, call 0x48fc40
48ff85: 8b4e64d981c4000000d835d4806000d88e74060000d86f04d95f  # 0x48fc40: force.y -= gravity / [0x6080d4] * mass
```
