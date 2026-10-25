# #2071 FORCE-BLOCK-REREAD: `0x48c470..0x48ff88` against `OriginalFlightModel`, and the sink

Date: 2026-10-25. Static analysis only: `$CS_ENGINE_IMAGE` (read-only, never committed; sha-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, VA = file offset + `0x400000`) was disassembled
with `llvm-objdump -d -M intel` into `private/`. The original executable was not run: every claim is `static`
(claim id `OWNER-STATIC-2026-10-25`), nothing is `verified_original`. Implementer: `sonnet-2` (Sonnet 5.5); no independent
review yet. This is the "force-block" row of `2026-10-23-m01-handover-dynamics-step.md` section 6 ("#796's reading of
`0x48c470..0x48ff88` was never reviewed against the image"). The cited bytes are the machine-readable block at the end;
`accept_force_block_reread_*` re-reads every row from the image.

## 0. Answer

1. **The block matches the law in every term that decides lift, thrust, drag and gravity.** The sink is **not** a
   force-block error: lift capacity at 113 m/s is 19.9 times the weight (`qS * CLmax` = 56 700 against `W` = 2850, pdevastator `S` = 515), the
   demand is `g / 9.82` = 2.04 times the weight, and the image's lift, drag, thrust and gravity are the law's (section 2).
2. **Three proven divergences were fixed in `cs_sim`** (section 3): the approximate square root the image uses for the
   lift normal, `Gcmd` and the rotation vectors (`0x48c824`, `0x53fcf5`, `0x539087`); the stall, which is the player's alone
   and acts along the rotation from the body +Z axis to world up, not the body X axis; and the fake-dynamics branch, which
   adds 5 m/s for a non-player and switches off the speed fades, the authority limit, the bank coupling, the weathervane and
   the stall. None changes the cruise by more than 0.4 m/s of sink.
3. **What does set the sink is the hand-over start state, not a force.** The law stalls at low speed (`qS * CLmax < W` below
   about 25 m/s for the pdevastator), the stall drops the nose, and nothing in the law brings it back (there is no pitch restoring term
   in the image either). Re-flown with the placement speed (20 m/s along the nose) but **throttle 1.0**, the law settles at
   **113.2 m/s sinking 3.80 m/s** (section 5); the capture cruises 113.5 m/s sinking 5.6 m/s with the nose 2.8 degrees down.
   At throttle 0.4 it settles at 83.0 m/s sinking 9.73 m/s. So the capture is consistent with the placement state and a
   throttle of about 1.0, which stays the `hand-over-command` unknown (section 7); it is not a lead to set any throttle.
4. **`veh+0x384` (fake dynamics) is a developer switch for the player.** The only writers are the vehicle reset
   (`0x4b08c5`), the console commands `fd` and `ifon` (`0x43e5dc`, `0x43e626`) and the command-line option `-fd`
   (`0x4753ef`). The coincidence `fd_speed = 113` (pdevastator) with the capture's 113.5 m/s is therefore a lead that needs
   one owner answer: was the capture run with `-fd`? Real dynamics at throttle 1.0 give 113.2 m/s in the same state, so the
   coincidence is not conclusive either way.
5. **Not modelled and recorded** (section 4): the ground-blow assist `0x48c220` / `0x48bf60` (retail values 400 / 10 /
   0.5, found in `player.zrd` where the 2026-10-08 finding listed only some of its keys), and `high_speed_pitch_fade`
   (retail 1000 / 1001 mph, unreachable).

## 1. How the block is laid out

| function | role |
| --- | --- |
| `0x48e580` | the step; calls `0x48c470(veh, &force, &torque, &stall, &yawmax)` at `0x48e6da`, then integrates (finding 2026-10-23) |
| `0x48c470` .. `0x48d2b7` | forces and rotation terms: the fake gate, the lift demand, the call into `0x48fc40`, the torque terms, the ground-blow call, the stall |
| `0x48bdd0` | speed fades: roll `t`, pitch `t` (tapered by `high_speed_pitch_fade`), yaw (three segments) |
| `0x48bf60`, `0x48c220` | the ground-blow probe and its torque and velocity effect |
| `0x48d2c0` .. `0x48fc3f` | contact and collision handling (not forces; not read here) |
| `0x48fc40` | atmosphere, Mach, `q`, thrust, `CL`, drag, the lift, thrust and gravity vectors, the load factor |
| `0x48ffb0` | the lift at `Gcmd = 9`, for the stall margin |
| `0x41abc0`, `0x41abd0`, `0x41ac80`, `0x41aca0`, `0x41acf0`, `0x41ad80`, `0x41ada0` | `qS`, `CL`, `q`, atmosphere, thrust coefficient, `CLmax`, drag coefficient |
| `0x53fd40`, `0x53fca0`, `0x539040` | the half-angle quaternion, its rotation vector, a perpendicular for opposite directions |

Frame time is `[0x9ad744]`, copied to `[0x71c56c]`. Floats are `f32` throughout (the x87 stores to `f32` between steps).

## 2. Term by term

"Match" means the instructions compute what `OriginalFlightModel::step` computes, under the same names. VAs are the
instruction sites; the bytes are in the block.

| term | image | cs_sim | verdict |
| --- | --- | --- | --- |
| fake gate | `veh+0x384` set, or (non-player and horizontal distance^2 to the player `> 1e6`) (`0x48c4ba`, `0x48c4e9`) | `DynamicsKind::Fake` chosen by the caller | match (no production caller outside the law's own module selects Fake today: grep; section 7) |
| speed used | `veh+0x934`, written as `sqrt(veh+0x930)` after each velocity update (`0x48e9d3`) | `norm(velocity)` | match (the hand-over writes `veh+0x924`; whether it also writes `+0x934` was not read) |
| lift demand floor | speed `<= 2.4384` skips the demand: `Gcmd = 0`, normal `(0.7, 0.7)` (`0x48c50f`, `0x48c857`) | `LIFT_MIN_SPEED_MPS`, now with the same normal | match (normal fixed, no effect: `CL = 0`) |
| player target | `cos(alpha) >= cos(liftAOAs[0])` keeps `V`; `<= cos(liftAOAs[1])` is `|v| * nose`; between, `t = (c0 - cos) / (c0 - c1)` (`0x48c565`..`0x48c6e7`); AI always `|v| * nose` (`0x48c6e9`) | `lift_target` | match |
| demand | `a = lift_accel_rate * (Vt - V) + (0, g, 0)`, `g = veh[+0x64][+0xc4]` (`0x48c746`, `0x48c77b`), the vehicle record's `gravity`, whose default is `nom_gravity` (`0x478b58`) | `steer`, `airframe.gravity` | match |
| projection | `ax = a . X (veh+0x180)`, `ay = a . Y (veh+0x18c)` | `dot(steer, right)`, `dot(steer, up)` | match |
| load command, normal | `root = approx(ax^2 + ay^2)` (`0x48c824`); `Gcmd = root / 9.82`; normal `= a / root` (`0x48c82e`) | exact `sqrt` | **diverged, fixed** (section 3.1) |
| `CL` | `clamp(Gcmd, -5, 9) * W / qS`, clamp `+-1.8`, then `min(., 0.75 - 0.15 M)` (`0x41abd0`); `qS = 0` gives `-1.8` through NaN compares, with `qS * CL = 0` | same, `0` at `qS = 0` | match |
| thrust | `Tc(max(M, 0.1))` (`0x41acf0`) `* throttle * engine_factor * S`, `* (1 + 0.13 Z.y)` when `Z.y <= 0`, `* (1 + 0.24 Z.y)`; nitro `1.8` and drag `0.8`; zero while `veh+0x2dc & 2` (`0x48fcb6`..`0x48fde1`) | `aero` | match |
| drag | `qS * drag_factor * CD(M)`, `CD = 0.73 (0.12 + 0.8 M + 0.5 M^2)`, no induced term (`0x41ada0` ignores its second argument), against `v / |v|` above 0.1 m/s (`0x48fe2b`) | `drag_coefficient` | match |
| lift vector | `qS * CL * (nx X + ny Y)` (`0x48fee9`, `0x48ff37`) | `direction * lift_magnitude` | match (with the normal of section 3.1) |
| load factor | `q * S * CL * ny / W` (`0x48fd51`) | `load_factor` | match |
| gravity | `force.y -= g / 9.82 * W`, then the force `* 9.82 / W` (`0x48ff85`, `0x48c88a`) | `-g` on `y` | match |
| atmosphere, Mach, `q` | `alt_ft = y * 3.2808399`, two layers, `q = 0.5 rho v_ft^2`, `M = v / (a_ft * 0.3048)` (`0x48fc4b`..`0x48fca6`, `0x41aca0`, `0x41ac80`) | `atmosphere`, `mach_from_speed` | match |
| stall margin | `1 - lift(9 G) / W`, player only (`0x48cfa3`, `0x48cfb7`) | `1 - qS * min(CLmax, 1.8) / W`, every aircraft | margin equal where positive; **gate diverged, fixed** (3.2) |
| speed fades | roll `t`, pitch `t` tapered between `high_speed_pitch_fade`, yaw three segments (`0x48bdd0`) | roll and pitch `ramp`, `yaw_authority` | match, except the pitch taper (section 4.2) |
| authority `lambda` | `(cos - c46) / (1 - c46)` if `cos > c46`, else 0; `min` with `(15 - n) / 6` above `n = 9`, `(-9 - n) / -3` below `-6`; no floor (`0x48c9f4`..`0x48ca73`) | `authority`, floored at 0 | match (`n` never exceeds 9, so no case differs) |
| sign rule for `lambda` | pitch and yaw scaled when the term's sign differs from the sign of `axis . r`, `r` the nose-to-velocity rotation (`0x48cb5a`); level-off when `dL . r < 0` | `rotation_increases_attack` | match |
| roll, pitch, yaw | `input * fade * torque * dt` about `veh+0x198` (Z), `+0x180` (X), `+0x18c` (Y) | same | match |
| bank coupling | `fall_off X.y dt` about Y; `(bank_off |X.y| - fall_off Y.y [Y.y < 0]) dt` about X; constants `0.205`, `0.165` (`0x48cc61`..`0x48cd18`) | same | match (skipped under fake: **fixed**, 3.3) |
| weathervane | player, `veh+0x384` clear, speed `> 0`: `return_rate dt` times the nose-to-velocity rotation, no `lambda` (`0x48cd3e`..`0x48ce42`) | same | match (rotation magnitude: **fixed**, 3.1) |
| level-off | `veh+0x12c`, roll and pitch inputs zero: `level_off_rate dt` times the rotation from Y to world up, `lambda` if `dL . r < 0` (`0x48ce45`..`0x48cf7b`) | same | match |
| ground blow | `0x48c220` for every non-player and for the player with `veh+0x384` clear, after the level-off | none | **not modelled** (section 4.1) |
| stall rotation | player only; `r` = rotation from body +Z to world up; `dL += r (strip + stall_mag s dt)`, `strip = -(dL . r) / (r . r)` if `dL . r < 0` else 0 (`0x48d05f`..`0x48d13b`) | along body X, unit | **diverged, fixed** (3.2) |
| damping | `L *= f(dt * ang_momentum_damp)`, `f(x) = 1 - x + x^2/2 - 0.166666 x^3` below 0.1, a table `exp(-x)` above (`0x4606d0`, `0x460410`) | `exp(-damp dt)` | match to table precision |
| fake force | `target - V`, `target = -(fd_speed * throttle [+ ... ]) * Z`; non-player `- 5.0` more (`0x48c5a0`..`0x48c603`) | `fd_speed * throttle * nose - v` | **diverged, fixed** (3.3) |
| step | `v += force dt` (+ AI floor `-4.4704` along Z), `veh+0x924 = v`, then `pos += dt * v` (`0x48e8b9`..`0x48eab3`) | semi-implicit | match |

## 3. Divergences fixed in `cs_sim`

Each is in `crates/cs_sim/src/flight/original.rs`; each has an `accept_force_block_reread_*` unit test with hand values.

### 3.1 The approximate square root

Three sites replace `sqrt` by `float_from_bits((bits >> 1) + 0x1fc00000)` on the `f32` pattern (`approx_sqrt`): the lift
demand `0x48c824` (the `|(ax, ay)|` behind `Gcmd` and the normal), the rotation vector `0x53fcf5` (the `|v|` behind
`atan2(s, w) / s`) and the perpendicular `0x539087`. It is exact at powers of four and high by up to 6.07 % between.

* In the lift, `Gcmd` is high by the factor `k` and the normal short by `1/k`, so an unclamped demand gives the same
  force and the same load factor, and a clamped one (`CLmax` or 9 G binding) gives a lift up to 6 % short. In level flight
  `g^2 = 400`, root 20.5: `CL` is 2.5 % high, the normal 20/20.5 short.
* In `half_angle_rotation`, the rotation is `v * atan2(s_approx, w) / s_approx`: for the quarter turn it is 0.768226, not
  `pi/8 = 0.785398`. It scales the weathervane, the level-off assist and the stall's `r`. Exactly opposite directions use
  `(a.y, -a.x, 0) / root`, `w = 0`, angle `pi/2` (the model had a quarter of `pi`).

### 3.2 The stall

The stall block runs for the player only and not under fake dynamics (`0x48cf9d` skips every other caller, whose margin
stays 0). Its axis is `r`, the rotation from the body +Z axis to world up (`0x48d05f`..`0x48d09e`), not the body X axis,
and `|r|` is `(theta/2)` of that angle: level, `0.768`. The component of the accumulated rotation along `r` is stripped when
it opposes `r` and `stall_mag * s * dt` is added along `r`. The model dropped the nose along the unit body X axis for
every aircraft, 23 % harder in level flight and in a different direction when banked.

### 3.3 Fake dynamics

`0x48c5a0`: `target = -(fd_speed * throttle) * Z`, and for a non-player `- 5.0` more (`FAKE_AI_SPEED_BONUS_MPS`; an idle AI
is pulled toward 5 m/s ahead). The rotation terms under fake dynamics (`0x48c8e7`..`0x48c8fc`, `0x48cc5b`, `0x48cd52`,
`0x48cfb1`) use speed factors and `lambda` of 1, no bank coupling, no weathervane and no stall; the level-off assist still
runs, never scaled.

## 4. Recorded, not fixed

### 4.1 The ground blow (`0x48c220`, `0x48bf60`)

Called after the level-off term for every non-player and for the player with `veh+0x384` clear (`0x48cf7e`..`0x48cf95`), and
skipped when `veh+0x358 == 4` or, for a non-player, while the clock is before `veh+0xac` (`0x48c22b`..`0x48c252`).
`0x48bf60` casts a segment of length `groundblow_elev` from the body's position along the nose through the world ray test
(`0x4c8ec0`). On a hit where the back axis points along the surface normal (`Z . N > 0`) it returns `amount = (elev - d) /
elev * sqrt(Z . N)` and the vector `B = normalize(N x Z) * amount`; with no hit both are zero.

* Player: `d = T . B` with `T` the torque accumulated so far. `d >= 0`: `T += B * d * groundblow_mag`, and the velocity direction is
  turned toward the nose by `0x460410(dt * 2.0 * amount)` with its speed kept (`0x48c370`..`0x48c42b`). `d < 0`: `T += B * d *
  (-0.05) * groundblow_mag` and `amount` is zeroed, so no velocity steering.
* Non-player: `T += B * ai_groundblow * groundblow_mag` unconditionally, both factors times 0.15 while the clock is before
  `veh+0xb4`; the same velocity steering.

Retail `player.zrd` (first entry) has `groundblow_elev = 400`, `groundblow_mag = 10`, `ai_groundblow = 0.5` (read through the
production importer; the 2026-10-08 finding did not list them). With no hit `B` is zero and so is every term. At the hand-over
(150 m, nose within 3 degrees of level) a 400 m segment does not reach the sea, so nothing happens there; the term acts in a
dive toward a surface within 400 m, and the release state of section 5 (into the sea in 6 s in our law) is such a case. What
this task could not settle: whether the sea answers the ray test, the unit of the probe length, `0x4c8ec0`'s hit record
(`0x48c036`..`0x48c0e9` skip some hits for the player), and `veh+0xac` / `veh+0xb4`. Follow-up task filed.

### 4.2 `high_speed_pitch_fade`

The pitch factor tapers to 0 between `high_speed_pitch_fade[0]` and `[1]` (`0x48be22`..`0x48be68`; the roll factor does not).
Retail has `1000, 1001` mph = 447.04 and 447.49 m/s, above anything the law reaches (level top speed 112 m/s); the 2026-10-08
vocabulary does not carry the key. Recorded, not added: no reachable state differs.

### 4.3 `is_autogyro`

`2026-10-08-flight-original-fixed-wing-law.md` section 9 gives the fake branch to "the autogyro or far AI". Nothing at
`0x48c4ba` says so: the gate is `veh+0x384` and the horizontal squared distance (> 1e6, i.e. 1000 m) of a non-player from the
player. The writers of `veh+0x384` are listed in section 0.4; none is a vehicle class. Class dispatch (`0x489ea0`) sends classes
0 and 4 to `0x48e580`, 2 to `0x48a880`, 3 and 5 to `0x48b480`, 1 to `0x48ffe0`.

## 5. Re-flown numbers (our law, state-for-state, 60 Hz, pdevastator, 150 m, heading 40 degrees, fuel as imported)

| start (throttle, forward speed) | t=2 | t=6 | t=10 | t=20 | sink at t=20 | note |
| --- | --- | --- | --- | --- | --- | --- |
| 0.4, 20 m/s (placement) | 55.8 | 80.9 | 82.8 | 83.0 | **9.73 m/s** (was 9.35) | 82.4 horizontal |
| 0.0, 0 (release) | 32.2 | 70.1 | 75.5 | 76.0 | 69.9, below the sea by t = 6 | no lift at rest, no thrust |
| 0.4, 0 | 55.7 | 93.7 | 95.1 | 95.1 | 49.8 | dives |
| 1.0, 0 | 92.1 | 117.7 | 117.9 | 117.9 | 26.3 (was 28.8) | dives |
| **1.0, 20 m/s** | 94.6 | 113.0 | 113.2 | 113.2 | **3.80 m/s** (was 3.82) | horizontal 113.10 |
| 1.0, 55 m/s | 104.3 | 112.2 | 112.3 | 112.3 | 0 | level, no stall |
| 0.4, 55 m/s | 71.0 | 78.2 | 78.7 | 78.8 | 0 | level |

The capture (`2026-10-21-m01-hand-over-pose.md`): 113.5 m/s, 5.6 m/s sink, nose 2.8 degrees down, 111.8-113.6 over the
run. The rows above are the law flown from states the image can produce; they are not observations of the original.
Starting with enough speed (55 m/s) the law never stalls and flies level at its throttle's equilibrium (112.3 at 1.0,
78.8 at 0.4); starting slow it stalls, the stall drops the nose (throttle 1.0 from rest ends in a 12.9 degree glide, 26.3 m/s of sink at 117.9 m/s), and the law
has no term that raises it again. Starting at 20 m/s at throttle 1.0 the stall is brief and the dive is the capture's
size (3.80 against 5.6 m/s).

## 6. What the divergences changed

Corrections 3.1-3.3 moved the placement settle by 0.4 m/s of sink and the throttle-1.0 rest dive by 2.5 m/s of sink;
`accept_m01_handover_dynamics_step_placement_state_settles_below_the_capture` pinned 9.35 and now pins 9.73.
No retail pose of the cruise (level, no stall) changes.

## 7. Unknown, and who resolves it

| unknown | affects | resolver |
| --- | --- | --- |
| `launch-flag-clear-site`, `hand-over-command` (finding 2026-10-23, section 6) | start speed (0 or 20), throttle (0, 0.4, ~1.0?), hence cruise and sink | the owner's capture of `veh+0x124`, `+0x128`, `+0x924`, `+0x934` per frame, or the key-path read `0x487a30` |
| was the capture run with `-fd` (fake dynamics)? | `fd_speed 113` against the capture's 113.5 | one owner answer |
| does the hand-over write `veh+0x934` with `+0x924`? | the first step's speed read (it gates the lift demand below 2.4384 m/s) | static read of `0x47f1f0` for writes to `+0x930/+0x934` |
| how callers pick Fake for AI beyond 1000 m | far-AI motion | static read of the AI loop; not the player's law |
| ground blow (4.1) | any flight with the nose aimed at a surface within 400 m | follow-up task |
| `no-run` | every claim | the owner's reference capture |

## Instruction bytes cited (re-read by the ignored test)

```bytes
48c4ba: 8a868403000084c08b9634090000895580  # 0x48c470: veh+0x384 (fake dynamics) gate and the speed read veh+0x934
48c4e9: e832c40a00d81d187a600083c408dfe0f6c441  # 0x48c470: AI horizontal squared distance to the player (0x538920) against 1e6, beyond which the AI is fake
48c50f: d81d1c816000dfe0f6c441  # 0x48c470: speed <= 2.4384 jumps to the no-demand fallback
48c5a0: d88e28010000c6451b01d9e07406d825bc366000  # 0x48c470 fake branch: -(fd_speed*throttle), and for a non-player minus 5.0
48c565: d81d30c47100dfe0f6c401  # 0x48c470 player: cos(alpha) against cos(liftAOAs[0]); below it the blend runs
48c676: d90530c471008b16d865e48bced90530c47100d82534c47100def9  # 0x48c470: blend parameter t = (c0 - cos) / (c0 - c1)
48c746: d90548c47100d945c08b4e64  # 0x48c470: a_req = lift_accel_rate * (Vt - V)
48c77b: d881c4000000  # 0x48c470: a_req.y += veh[+0x64][+0xc4] (gravity)
48c7d1: d945e4d81dc8326000dfe0f6c440  # 0x48c470: demand with both components zero takes the fallback
48c7f7: c745e43333333fd835d4806000c745e03333333f  # 0x48c470: fallback normal (0.7, 0.7) and Gcmd = 0
48c81e: d95dcc8b45ccd1f8050000c01f  # 0x48c470: |a|^2 stored as f32, then sar 1 / add 0x1fc00000 (approximate root)
48c82e: d945dcd905dc326000d875dcd945e4d8c9d95de4d945e0d8c9d95de0ddd8d835d4806000  # 0x48c470: normal = a / root, Gcmd = root / 9.82
48c883: e8b8330000  # 0x48c470: call 0x48fc40 (thrust, drag, lift, gravity)
48c88a: d905d48060008b450cd8b674060000  # 0x48c470: force = F * (9.82 / W)
48c8b8: e8f3360000  # 0x48c470: call 0x48ffb0 (the 9 G lift for the stall margin)
48c919: e8b2f4ffff  # 0x48c470: call 0x48bdd0 (speed fades)
48c9ae: e88d330b00  # 0x48c470: call 0x53fd40 (half-angle quaternion nose to velocity)
48c9be: e8dd320b00  # 0x48c470: call 0x53fca0 (quaternion to rotation vector)
48c9f4: d81d2cc47100  # 0x48c470: cos(alpha) against cos(maxAOA)
48ca01: d945dcd8252cc47100d905dc326000d8252cc47100def9  # 0x48c470: lambda = (cos - c46) / (1 - c46), no floor
48ca1b: d94508d81d38c47100dfe0f6c4417517  # 0x48c470: n against highGs[0]
48ca73: d95d0ceb02  # 0x48c470: lambda = min(lambda, window factor)
48caea: d9861c010000d81dc8326000dfe0f6c4400f859f000000d945ec8d55a8  # 0x48c470: roll: veh+0x114 * roll fade (ebp-0x18) * roll_torque (veh+0x644) * dt
48cb07: d88e480600008d86800100008955cc8945dc  # 0x48c470: pitch: fade (ebp-0x14) * pitch_torque (veh+0x648) * veh+0x11c * dt
48cb5a: 894dcc7403d84d0c  # 0x48c470: pitch scaled by lambda only when its sign differs from X.r
48cbb7: d9864c0600008d4da8d88e200100008d868c010000894dcc8945dcd84df0  # 0x48c470: yaw: veh+0x120 * yaw factor (ebp-0x10) * rudder_torque (veh+0x64c) * dt
48cc56: 8a451b84c00f85dd000000d905f88962008d45d0  # 0x48c470: bank coupling is skipped under fake dynamics; fall_off * X.y
48ccbb: d98684010000d9e1d80dfc896200  # 0x48c470: bank_off * |X.y|
48cce2: d905f8896200d88e90010000d80d6cc57100  # 0x48c470: Y.y < 0: minus fall_off * Y.y
48cd3e: 3b3598c271000f85fb0000008a868403000084c0  # 0x48c470: weathervane only for the player with veh+0x384 clear and speed > 0
48cdee: e8ad2e0b00d9056cc57100d88e54060000  # 0x48c470: weathervane: rotation * return_rate (veh+0x654) * dt
48ce45: 8a862c01000084c00f842b010000  # 0x48c470: level-off flag veh+0x12c
48ce53: d98614010000d81dc8326000dfe0f6c4400f8414010000d9861c010000d81dc8326000dfe0f6c4400f84fd000000  # 0x48c470: level-off only with veh+0x114 and veh+0x11c both zero
48cf7e: 3b3598c27100750a8a868403000084c0750d8b45105056e886f2ffff83c408  # 0x48c470: ground-blow call (0x48c220) for a non-player or a player with veh+0x384 clear
48cf9d: 3b3598c271000f85b70100008a868403000084c0  # 0x48c470: the stall block is the player's alone
48cfb7: d986740600008b5d14d8bd6cffffffd82ddc326000d9c0d91bd81dc8326000  # 0x48c470: stall margin s = 1 - lift(9 G)/W
48d05f: a1c0796300894db88b0dc47963008955bc8b15c87963008945848d8570ffffff894d88508d4d8489558c518d55b452  # 0x48c470 stall: r = rotation from the body +Z axis to world up (0x6379c0)
48d0cb: d9450cd81dc8326000dfe0f6c401  # 0x48c470 stall: strip only when dL . r < 0
48d0e1: 8b4d18d901d809d94104d84904d94108d84908d9c9dec2dec1d95d14d94514d87d0cd9e0eb06  # 0x48c470 stall: strip = -(dL . r) / (r . r)
48d10d: d9051cc471008d4dc0d80b894d0cd80d6cc57100  # 0x48c470 stall: strip + stall_mag * s * dt, applied along r
48d123: d945c0d8c9d95dc0d945c4d8c9d95dc4d945c8d8c9d95dc8  # 0x48c470 stall: dL += r * that
48fc4b: 89354ce86400d98608020000d80d3c8160008b8634090000576880c5710051d91c24  # 0x48fc40: altitude y * 3.2808399, atmosphere (0x41aca0)
48fc89: e8f2aff8ffd91d68c57100d90580c5710083c408d80d38816000d87dfc  # 0x48fc40: Mach and q from the speed read veh+0x934
48fcb6: c745086666e63fc745f4cdcc4c3feb108b8e28010000894d08c745f40000803f  # 0x48fc40: veh+0x947 (nitro) throttle 1.8 and drag 0.8, else veh+0x128 and 1.0
48fce2: e809b0f8ff  # 0x48fc40: call 0x41acf0 (thrust coefficient)
48fcea: d9869c01000083c408d81dc8326000dfe0f6c4417414d905d8806000d88e9c010000d805dc326000dec9  # 0x48fc40: thrust * (1 + 0.13 Z.y) when Z.y <= 0
48fd14: d905dc8060008b450cd88e9c0100008b0d68c571008b1554c5710050d805dc3260005152d8c9d95df8ddd8e88caef8ff  # 0x48fc40: thrust * (1 + 0.24 Z.y), call 0x41abd0 (lift coefficient)
48fd4d: 85c0741ad90568c57100d88e78060000d84d08d84d18d8b674060000d918  # 0x48fc40: load factor = q * S * CL * ny / W
48fd76: e825b0f8ffd84df4bb0200000083c408  # 0x48fc40: call 0x41ada0 (drag coefficient), times the nitro factor
48fd89: d90568c57100d88e78060000  # 0x48fc40: q * S
48fde1: d9866c060000d88e78060000d84df8  # 0x48fc40: thrust = Tc * throttle * engine_factor (veh+0x66c) * S (veh+0x678)
48fdf3: d90590c57100a1b8d17500d88e700600008b7d1c8bd7d84d0cd90590c571008902d84d088b0dbcd17500894a04a1c0d17500  # 0x48fc40: drag = qS * drag_factor (veh+0x670) * CD; lift = qS * CL
48fe2b: d945fcd81da8346000dfe0f6c4417554  # 0x48fc40: drag only above 0.1 m/s
48fe3b: d875fc8b168bced9e0d95d0c  # 0x48fc40: drag along -velocity / speed
48fe91: d945f88d4de8d9e0894d0cd9c0d88e98010000d95de8d9c0d88e9c010000d95decd88ea0010000  # 0x48fc40: thrust along -Z (the nose)
48fee9: d945088d55e8d84d14895510d9c0d88e80010000d95de8d9c0d88e84010000d95decd88e88010000d95df0  # 0x48fc40: lift * nx along the body X axis
48ff37: d945088d45e8d84d18894510d9c0d88e8c010000d95de8d9c0d88e90010000d95decd88e94010000d95df0  # 0x48fc40: lift * ny along the body Y axis
48ff85: 8b4e64d981c4000000d835d4806000d88e74060000d86f04  # 0x48fc40: force.y -= g / 9.82 * W
48ffb0: 8b0d68c571008b1554c571008b44240468000010415152a34ce86400e8ffabf8ff  # 0x48ffb0: lift coefficient at Gcmd = 9.0
41abc0: d9442404a14ce86400d88878060000c3  # 0x41abc0: q * S
41abde: d8157034600083c404dfe0f6c441750addd8d90570346000eb15  # 0x41abd0: Gcmd above 9 becomes 9
41abf8: d8156c346000dfe0f6c4017408ddd8d9056c346000  # 0x41abd0: Gcmd below -5 becomes -5
41ac0d: 8b0d4ce86400d88974060000d8f1d95c2408  # 0x41abd0: CL = Gcmd * W / qS
41ac21: d9442408d81d7c346000dfe0f6c441750ac74424086666e63feb19d9442408d81d78346000dfe0f6c4017408c74424086666e6bf  # 0x41abd0: CL clamped to +-1.8
41ac55: 8b54240452e821010000d80de0326000d944240c83c404d8d9dfe0f6c4417406ddd8d9442408  # 0x41abd0: CLmax = 0.75 - 0.15 M; the result is min(CL, CLmax)
41ad80: d9442404d80dac346000d82d60346000c3  # 0x41ad80: 1.5 - 0.3 M
41ada0: d9442404d80de0326000d805b0346000d84c2404d80590346000d80d74346000c3  # 0x41ada0: CD = 0.73 * (0.12 + 0.8 M + 0.5 M^2), no induced term
48bdd0: d9442404d81df8c37100dfe0f6c4417408d905c8326000eb31d9442404d81dfcc37100dfe0f6c4017508  # 0x48bdd0: speed ramp t between turn_fade_in and turn_fade_out
48be22: d9442404d81d04c47100dfe0f6c4017508c70100000000eb2dd9442404d81d00c47100dfe0f6c441751cd90504c47100d8642404d90504c47100d82500c47100def9d8c9d919  # 0x48bdd0: pitch factor = t, tapered to 0 between high_speed_pitch_fade[0] and [1]
48be68: 8b442410d918  # 0x48bdd0: roll factor = t, untapered
48be6e: d9442404d81d10c47100dfe0f6c441740bd90508c47100e984000000d9442404d81d18c47100dfe0f6c4017508d9050cc47100eb6bd9442404d81d14c47100dfe0f6c401752ed90518c47100d8642404d905dc326000d8250cc47100dec9d90518c47100d82514c47100def9d8050cc47100eb2cd9442404d82510c47100d905dc326000d82508c47100dec9d90514c47100d82510c47100def9d80508c47100  # 0x48bdd0: yaw factor: yaw_low, rising to 1 at yaw_max, falling to yaw_high
53fcf5: d1f8050000c01f  # 0x53fca0: sar 1 / add 0x1fc00000 (approximate root of |v|^2)
53fcff: d906d81dc8326000d94508dfe0f6c4017506d906d9f3eb08d906d9e0d9f3d9e0d87508  # 0x53fca0: angle = atan2(s, w), rotation = v * angle / s
539087: d1f8050000c01f  # 0x539040: the same approximate root, for the perpendicular
48c3da: d905bc2b6200  # 0x48c220: ground-blow velocity steering rate 2.0 * amount
48bfa3: a120c47100  # 0x48bf60: ground-blow probe length groundblow_elev
48c1f1: 8a5dc88b1633c984db0f95c1  # 0x48bf60: end of the probe
48e6da: e891ddffff  # 0x48e580: call 0x48c470 with (veh, force, torque, stall, yaw max)
48e9d3: d99730090000d9fad99f34090000  # 0x48e580: veh+0x930 = |v|^2, veh+0x934 = sqrt (speed for the next step)
43e5d3: 85c0740b85c90f95c1888884030000  # command dispatch: player+0x384 = result for the 2-byte name 'fd'
43e61e: 8b0d98c271005f5ec6818403000001  # command dispatch: 'ifon' sets player+0x920 and player+0x384
4753e1: a0c9da710084c0740ca198c27100c6808403000001  # startup: option '-fd' ([0x71dac9]) sets player+0x384
4b08c5: 889e84030000  # vehicle reset: veh+0x384 = 0
603470: 00001041  # f32 9.0: Gcmd upper window
60346c: 0000a0c0  # f32 -5.0: Gcmd lower window
60347c: 6666e63f  # f32 1.8: CL upper window
603478: 6666e6bf  # f32 -1.8: CL lower window
6080d4: b81e1d41  # f32 9.82: the force scale
6080d8: b81e053e  # f32 0.13
6080dc: 8fc2753e  # f32 0.24
60811c: bf0e1c40  # f32 2.4384: speed floor of the lift demand
6034a8: cdcccc3d  # f32 0.1: drag floor and Mach floor
6036bc: 0000a040  # f32 5.0: AI fake-dynamics speed term (and the fuel factor)
6032dc: 0000803f  # f32 1.0
6289f8: 85eb513e  # f32 0.205: fall_off
6289fc: c3f5283e  # f32 0.165: bank_off
6379c0: 000000000000803f00000000  # vec3 (0, 1, 0): world up
622bbc: 00000040  # f32 2.0: ground-blow steering rate
6036ac: cdcc4cbd  # f32 -0.05: ground-blow torque factor
607a18: 00247449  # f32 1e6: AI fake distance^2
6034fc: cdcc4c3e  # f32 0.2
6298d4: 2d666400  # '-fd'
622ee8: 666400  # 'fd'
622eec: 69666f6e00  # 'ifon'
```

## Review addendum: the `--smoke-no-teleport` wingman check (designed, not CI)

Measured on the branch with the release `cs` (`--mission M01 --smoke-seconds 45
--smoke-no-teleport`, retail). Main passes `wingmen_near_player` (wingman 188 m
at t+15, 446 m at t+30, sampled at the 500 m threshold by luck); the branch
fails it (wingman 607 m from t+18). A temporary per-second position print (not
committed) shows why: the idle-input player dives from 131 m to 3 m between
t+4 and t+7 (the same crash as on main, see #1845), `wingman_1` follows the
leader down, reaches y = 5 m at t+8 (sea level) and stays at that point while the
player slides on at about 100 m/s. So the freeze is the AI wingman following a
crashed leader into the sea, not a numerical fault.

Attribution (each experiment reverted afterwards): the AI +5 m/s, the player-only
stall, the approximate square root, the fake-dynamics fade/lambda/yaw factors and
the fake-dynamics bank-coupling skip, each reverted alone and the first three
together, all still freeze, only at a different distance (579..1288 m). The
outcome is therefore a sensitive consequence of the combined image-proven
changes (wingman geometry at the moment the leader crashes), not of one term.
The wingman controller itself is a designed port (`mission_ai`); it has no
image-derived sea avoidance. No law constant was tuned to restore the pass.

## Reproduction

```sh
CS_ENGINE_IMAGE=<crimson.decrypted.exe> CS_GAME_DIR=<retail> \
  cargo test --locked -p cs_app --test cs_app_tests_d -- accept_force_block_reread_ --include-ignored
cargo test --locked -p cs_sim --lib -- accept_force_block_reread_
```
