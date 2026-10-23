# #1982 M01-HANDOVER-DYNAMICS-STEP: what `0x41c270` / `0x41f810` are, and the equilibrium the real flight step gives after the hand-over

Date: 2026-10-23. Static analysis only: `$CS_ENGINE_IMAGE` (read-only, never committed; sha-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, VA = file offset + `0x400000`) was disassembled
with `llvm-objdump -d -M intel` into `private/`. The original executable was not run: every claim is `static`
(claim id `OWNER-STATIC-2026-10-23`), nothing is `verified_original`. The numbers in section 4 are **our law**
(`OriginalFlightModel`, #796) flown from states the image produces; they are not observations of the original.
Implementer: `sonnet-2` (Sonnet 5.5); no independent review yet. Builds on `2026-10-23-m01-handover-velocity-trace.md`
(#1978) and `2026-10-08-flight-original-fixed-wing-law.md` (#796). The cited bytes are the machine-readable block at the
end; `accept_m01_handover_dynamics_step_*` re-reads every row from the image.

## 0. Answer for #1938

1. **`0x41c270` and `0x41f810` are not the thrust/drag/lift integration.** They are the AI behaviour and route
   controller (section 1). The player's thrust, drag, lift and integration are in `0x48e580`, reached through `0x489ea0`
   (section 2) — the function #796 already recovered. #1978's "remaining lead" was therefore a mis-pointer.
2. **The throttle has two fields:** `veh+0x124` is the *command*, `veh+0x128` the *actual* throttle that the thrust uses
   and that slews toward the command at `0.5 / s`. Both hand-over states set them equal (section 3).
3. **Our law, flown from the two hand-over states the image can produce, does not reach 111.8-113.6 m/s at throttle 0 or
   0.4** (section 4): throttle 0.4 with the placement speed settles at **82.8 m/s sinking 9.35 m/s**, and its level
   (thrust = drag) speed is **78.8 m/s**; throttle 0 has no level equilibrium and falls. The same law gives **112.3 m/s level
   at throttle 1.0**, 1.2 % below the capture's 113.6. So the capture's cruise is *consistent with* a command of about 1.0 and
   is not the model's equilibrium at 0.4. No static writer of a command above 0.4 on the launch path is known
   (#1978 section 3), so which input raised it stays **unknown**.
4. **The capture's "111.8 m/s at t=0" is not the hand-over velocity.** The same table's positions (`m01_free_flight.rs`
   `MEASURED`) move 3.3 m in the first second, then 48, 85, 104, 111, 113 m in the next five: the body starts almost at
   rest and accelerates for about five seconds, which matches the *release* state (velocity the zero vector), not a
   launch at 111.8.
5. What #1938 can implement without inventing a number: the hand-over state is `v = 0` (release) or `v = 20 m/s along the
   nose, throttle 0.4` (placement), never 55.5 m/s; angular momentum zero; actual throttle equal to the command. What it
   cannot: the 113.6 m/s cruise or the 5.6 m/s sink. That needs the owner's capture of `veh+0x124` and `veh+0x128` (and
   `veh+0x924`) for every frame from the end of the cinematic to t = 6 s (section 6).

## 1. `0x41c270` and `0x41f810`: the AI behaviour and route controller

The per-vehicle loop at `0x4898f9` (`0x4898c0` region, #1978 section 2) runs, for a vehicle whose dynamics are not frozen:

* `0x41f810(veh)` at `0x489abc`, only when `veh+0x358 <= 3` (`0x489ab6`): a timer-driven behaviour state step. It reads
  the clock `[0x71c470]` against `veh+0xf0`, uses the C `rand` (`[0xa20350]`), changes `veh+0x358` (the behaviour state:
  writes of 3, 0, 4, 1, 2, 5 in `0x41f98e`..`0x42180f`) and calls the `0x4a60xx` / `0x422xxx` helpers. It contains **no
  access** to `veh+0x124`, `+0x128`, `+0x924` or `+0x930/+0x934`.
* `0x41c270(veh)` at `0x489b14`: a mode dispatcher on `veh+0x2f0` (`0x41c2e6`): modes 0 and 2 call the route controller
  `0x41d1f0` (`0x41c305`), mode 1 `0x41d9f0`, and `veh+0x67c == 4` `0x41e760`. `0x41d1f0` indexes the route tables
  `[0x64f610]`/`[0x64f614]` with `veh+0x2e4/0x2e8/0x2ec`; its helpers `0x41afe0` (nothing to do when `veh+0x948`, the
  route object, is null: `0x41afea`, `0x41aff3`) and `0x41b560` follow a path.
* The only writes in `0x41afd0..0x420000` of the velocity `veh+0x924` are in `0x41b560` (`0x41b7df`, `0x41bb16`) and of
  the throttle command `veh+0x124` are `0x41bbc3`, `0x41bbde`, `0x41bbf7`, `0x41be41` (a slew toward a speed target by
  `dt * [0x603508]`, `[0x603508]` = 0.35, clamped to a per-record window). They run for a vehicle with a route
  (`veh+0x948`); this is the "route-following controller" #1978 listed at `0x420a30`'s neighbourhood.

Not established: whether `veh+0x948` is non-null for the player at the hand-over (writers: `0x47ffb6`, `0x486158`,
`0x488737`.. and others, not read). The player's throttle is changed by input (`0x487a30`, #1978 section 3); nothing in
the image shows the route controller running for the player in M01, and nothing shows it does not. It is **unknown**; it
affects whether a route could set the player's throttle or velocity after the hand-over.

## 2. The real step: `0x489ea0` -> `0x48e580`

After the integration gate (`0x489bb4`: `mov ecx, esi; call 0x489ea0`), `0x489ea0` returns at once when
`veh+0x91d` is set and `veh+0x91f` clear (`0x489ea3..0x489eb5`), and otherwise jumps through the table at `0x489f48` on the
class `veh+0x67c`: classes 0 and 4 -> `0x48e580` (fixed-wing), 2 -> `0x48a880`, 3 and 5 -> `0x48b480`, 1 -> `0x48ffe0`.
`0x48e580` is #796's law: it reads command and actual throttle, burns fuel, slews, calls `0x48c470` (forces and torques)
and, through it, the atmosphere `0x41aca0`, thrust `0x41acf0` and drag `0x41ada0`. The frame time is `[0x9ad744]`,
copied to `[0x71c56c]` at `0x4897d8`.

Terms of `0x48e580` re-read here, each equal to `cs_sim`'s step 1 (`OriginalFlightModel::step`):

| term | image | cs_sim |
| --- | --- | --- |
| slew | `actual += dt * 0.5` toward the command, clamped to it (`0x48e63f..0x48e6c9`, `[0x6032e0]` = 0.5) | `THROTTLE_SLEW_PER_S = 0.5` |
| fuel | `fuel -= dt * actual * 5.0` for the player when `veh+0x384` is clear (`0x48e5e6..0x48e615`, `[0x6036bc]` = 5.0); `fuel <= 0` jumps over burn **and** slew (`0x48e5f7`) | `FUEL_PER_S = 5.0`, `frozen` skips both |
| thrust uses | the *actual* throttle (`veh+0x128`) | `state.throttle` |

The force block `0x48c470..0x48ff88` was not re-read in this task; #796's reading of it stands unreviewed by this one.

## 3. The two hand-over states, exactly

`0x47f1f0(0, ...)` (#1978 section 3) sets either

* **placement** (`veh+0x91d` set, callback 10): command `veh+0x124 = 0.4` and the velocity `veh+0x924` = the vector
  `(0, 0, 1)` (`0x6379d0`) rotated by the body attitude `veh+0x150` (`0x53fb40`, `0x47f2a2`) times `[0x608040] = -20.0`.
  With the nose along body `-Z` (the law's convention, `BODY_FORWARD`) that is **20 m/s along the nose**, not backwards;
  the unit is the law's own (metres, as `nom_gravity = 20`), not separately measured; or
* **release** (`veh+0x91d` clear): command `0`, velocity the vec3 at `0x75d1b8` (the zero vector, #1978).

Both then run the common tail: `veh+0x91d/0x91f/0x91e` cleared (`0x47f625`), **`veh+0x128 = veh+0x124`** (the actual
throttle equals the command, `0x47f637`), angular momentum `veh+0x160` and `veh+0x16c` set to the zero vector
(`0x47f648`, `0x47f66c`). Nothing reads `[0x71c50c]`'s clearing here; which state the launch ends in is still #1978's
`launch-flag-clear-site`.

## 4. Equilibrium, from the law and the hand-over states

Formula (level, hands off, `S` cancels): the speed where `Tc(max(M, 0.1)) * throttle * engine = 0.5 * rho * v_ft^2 *
drag_factor * CD(M)`, with the two-layer atmosphere (150 m is the low layer: `k = 0.9884208`, `rho = 0.9544815 * 0.002377`),
`Tc` and `CD` as in #796. With the `pdevastator` record's `engine_factor = 0.65`, `drag_factor = 0.62`:

| throttle | level equilibrium speed | production law, attitude held (`0x491c60` probe) |
| --- | --- | --- |
| 0 | none (no thrust): falls, 33.4 m/s terminal in the held attitude | 33.40 m/s vertical |
| 0.4 | 78.77 m/s | 78.77 m/s |
| 1.0 | 112.29 m/s | 112.29 m/s |

Free flight of the production law from the hand-over pose (150 m, heading 40 degrees, no input, fuel as imported, 60 Hz):

| start state | t=2 s | t=6 s | t=10 s | settled | sink |
| --- | --- | --- | --- | --- | --- |
| placement: throttle 0.4, 20 m/s along the nose | 55.8 | 80.7 | 82.7 | **82.8 m/s** (82.3 horizontal) | **9.35 m/s** (steady) |
| release: throttle 0, v = 0 | 33.1 | 71.0 | 76.1 | still diving, no equilibrium | 71 m/s and growing, below the sea by t = 6 s |
| throttle 0.4, v = 0 | 57.0 | 94.5 | 95.8 | 95.8 m/s | 52.9 m/s (a dive: no lift below 2.4384 m/s) |
| throttle 1.0, v = 0 | 92.9 | 118.2 | 118.3 | 118.3 m/s | 28.8 m/s |

Answers to the task's question: at throttle 0.4 the model does **not** reach 111.8-113.6 m/s (82.8 m/s at best, within
~6 s of the placement speed); at throttle 0 it does not converge. It sinks in every case (its glide path is steeper than the
capture's 5.6 m/s). The speed at throttle 1.0 (112.3 level) is close to the capture's cruise; that closeness is recorded as
a lead, not used as a source: **nothing is tuned and no throttle is set to 1.0**.

## 5. Terms of our law that differ from the image at the hand-over

| term | image | main today |
| --- | --- | --- |
| start velocity | 0 (release) or 20 m/s along the nose (placement), never 55 | `RETAIL_START_SPEED_M_S` = 55, a declared development value (`cs_app::playtest::retail`) |
| throttle at hand-over | command and actual both 0 (release) or 0.4 (placement) | the launch throttle declared by the staging code |
| angular momentum | zero | zero in `OriginalState::at`; not re-read in the staging |
| lift at rest | the capture sinks 0.2 m in its first second, which the law (no lift below 2.4384 m/s, gravity 20 m/s^2) cannot give from rest | none below 2.4384 m/s; origin of the capture's behaviour **unknown** (dynamics may start up to 1 s into the capture: its clock is read +-1 s) |
| glide / sink | 5.6 m/s at 113.5 m/s (capture, not static) | 9.35 m/s at 82.8 m/s (placement), 28.8 m/s at 118 m/s (from rest, 1.0) |
| route controller | `0x41c270` chain may run for a vehicle with `veh+0x948` | not modelled; no evidence it runs for the player |

No term of the slew, the fuel rule or the actual-throttle use differs (section 2).

## 6. What stays unknown, what it affects, who resolves it

| unknown | affects | resolver |
| --- | --- | --- |
| `launch-flag-clear-site`: release or placement at M01's end | start velocity 0 vs 20 m/s, throttle 0 vs 0.4 | #1978's open item, or the capture below |
| `hand-over-command`: what raises the command above 0.4 (the capture's cruise needs about 1.0 in our law) | cruise speed, sink | the owner's capture of `veh+0x124`, `veh+0x128`, `veh+0x924` every frame from the cinematic end to t = 6 s; or a static read of the key path `0x487a30` for the key the owner pressed |
| `player-route-object`: is `veh+0x948` set for the player | whether `0x41b560` writes the player's velocity/throttle | static: the writers of `veh+0x948` for the player (`0x47ffb6`..) |
| `force-block`: `0x48c470..0x48ff88` re-read | any sink / lift difference | a review of #796 against the image |
| `sink-at-rest`: the capture's 0.2 m first-second sink | the first second of the flight | the same capture |
| `no-run` | every claim | the owner's reference capture |

## Instruction bytes cited (re-read by the ignored test)

```bytes
489ab6: 83f9037f09  # vehicle loop: cmp ecx,3 / jg skips 0x41f810 (ecx = veh+0x358)
489abc: e84f5df9ff  # vehicle loop: call 0x41f810 (control state step)
489b14: e85727f9ff  # vehicle loop: call 0x41c270 (mode dispatcher)
489bb4: 8bcee8e5020000  # vehicle loop: mov ecx,esi / call 0x489ea0 (class dispatcher), after the integration gate
489edb: ff2485489f4800  # 0x489ea0: jmp [4*class + 0x489f48] on veh+0x67c
489f48: f29e4800fa9e4800e29e4800ea9e4800f29e4800ea9e4800  # class jump table, six dwords: 0->0x489ef2 1->0x489efa 2->0x489ee2 3->0x489eea 4->0x489ef2 5->0x489eea
489ef3: e888460000  # class 0 and 4: call 0x48e580 (fixed-wing step)
41c273: 8a86cc00000084c00f85e000  # 0x41c270: veh+0xcc gate
41c2e6: 8b86f002000083e80074  # 0x41c270: dispatch on veh+0x2f0 mode
41c305: e8e60e0000  # 0x41c270: mode 0 and 2 call 0x41d1f0 (AI behaviour and route controller)
41afea: 8b864809000085c0570f  # 0x41afe0: the route step has nothing to do without veh+0x948 (route object)
41b7df: 8d86240900008bd0890a8b4dac89  # 0x41b560: AI path follow writes veh+0x924 (velocity) from the path
41bbc3: d99e24010000  # 0x41b560: AI path follow writes the throttle veh+0x124 toward a speed target
4897c6: a144d79a00d90570c47100d80544d79a00  # 0x4897c0: dt [0x9ad744] copied to [0x71c56c]
48e58f: d987240100008bb7b0020000  # 0x48e580: throttle command veh+0x124 minus actual veh+0x128
48e5e6: d98734010000d81dc8326000df  # 0x48e580: fuel veh+0x134 <= 0 skips burn and slew (player, veh+0x384 clear)
48e5fd: d90544d79a00d88f28010000d80dbc366000d8af34010000  # 0x48e580: fuel -= dt * veh+0x128 * 5.0
48e63f: d98728010000d89f24010000  # 0x48e580: actual < command: slew up
48e652: d9056cc57100d80de0326000d88728010000d99728010000  # 0x48e580: actual += dt * 0.5, clamped to the command
48e685: d98728010000d89f24010000  # 0x48e580: actual > command: slew down
48e698: d9056cc57100d80de0326000d8af28010000d99728010000  # 0x48e580: actual -= dt * 0.5, clamped to the command
6032e0: 0000003f  # f32 0.5: the throttle slew rate per second
6036bc: 0000a040  # f32 5.0: fuel burn factor
608040: 0000a0c1  # f32 -20.0: the cinematic placement speed factor
6379d0: 00000000000000000000803f  # vec3 (0, 0, 1) rotated by the body attitude for the placement velocity
47f625: 88981d09000088981f09000088981e090000  # 0x47f1f0 tail: veh+0x91d, +0x91f, +0x91e cleared
47f637: a198c271008b8824010000898828010000  # 0x47f1f0 tail: actual throttle veh+0x128 = command veh+0x124
47f648: 8b1598c27100a1b8d1750081c26001000089028b0dbcd17500894a04a1c0d17500  # 0x47f1f0 tail: angular momentum veh+0x160 = zero vector 0x75d1b8
47f2a2: e899080c00  # 0x47f1f0 placement: call 0x53fb40 (rotate (0,0,1) by veh+0x150)
48fc70: e82bb0f8ff  # 0x48e580 callee: call 0x41aca0 (atmosphere)
48fce2: e809b0f8ff  # 0x48e580 callee: call 0x41acf0 (thrust coefficient)
48fd76: e825b0f8ff  # 0x48e580 callee: call 0x41ada0 (drag coefficient)
48e6da: e891ddffff  # 0x48e580: call 0x48c470 (forces and torques)
```

## Reproduction

```sh
CS_ENGINE_IMAGE=<crimson.decrypted.exe> CS_GAME_DIR=<retail> \
  cargo test --locked -p cs_app --test cs_app_tests_d -- accept_m01_handover_dynamics_step_ --include-ignored
```
