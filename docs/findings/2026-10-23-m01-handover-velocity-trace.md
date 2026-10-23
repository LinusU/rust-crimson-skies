# #1978 M01-HANDOVER-VELOCITY-TRACE: what clears `[0x71c50c]` / `veh+0x91d` and what writes the player's throttle and velocity after the launch cinematic

Date: 2026-10-23. Static analysis only: `$CS_ENGINE_IMAGE` (read-only, never committed; sha-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, VA = file offset of the mapped image + `0x400000`)
was disassembled with `llvm-objdump -d -M intel` into `private/`. The original executable was not run: every claim is
`static`, nothing is `verified_original`. Implementer: `sonnet-1` (Sonnet 5.5); no independent review yet.
Builds on `2026-10-18-m01-launch-release.md` (#1678) and the cinematic pair of
`2026-10-23-m01-fog-update-static.md`. The cited instruction bytes are the machine-readable block at the end;
`accept_m01_handover_velocity_trace_*` re-reads every row from the image.

## 0. Answer for #1938

**No instruction in the image yields 111.8 m/s, and none yields a hand-over throttle above 0.4.** Everything
the image assigns to the player's throttle or velocity around the launch is one of: throttle 0.4 and velocity
`-20.0 x axis` (cinematic placement), throttle 0 and velocity = the zero vector global (release), or a throttle
changed by player input. The cruise the capture shows (113.6 m/s) is therefore not an initial assignment. If it is
explained at all statically it is by the flight model's thrust/drag integration, which this trace did **not** read
(section 5). #1938 can implement nothing new from this trace without inventing a number; it needs either the
flight-model read (a follow-up) or the owner's capture of `veh+0x124` and `veh+0x924` in the first control frames.

Correction to the task text: **`veh+0x124` is the throttle, not the velocity.** The velocity is the vec3 at
`veh+0x924` (`+0x930` its square, `+0x934` its length), as `0x47f1f0` writes them.

## 1. `[0x71c50c]`: the cinematic-owner flag

It is the gate of the player's whole flight update, not an attach flag of the vehicle: `0x4897c0` (the per-frame world
update) loads it at `0x4897dd` and `jne 0x489e30` at `0x4897f3`, which skips the vehicle loop (no dynamics, no
integration). Other readers: `0x46a496` (returns 0 at once when set) and `0x4afd00` (skips a block when set).

| writer | effect | when |
| --- | --- | --- |
| `0x47e2e6` (callback id 20, `0x47e080` case at `0x47e2e1`) | `[0x71c50c]` = the first argument | the intro's `callback_sequence` fires id 20 (see #1678) |
| `0x47e0bc` (callback id 0) | cleared if the argument equals it | the callback the animation fires when it finishes |
| `0x47e22a` (callback id 12, branch where `0x440ad0()` is non-zero) | cleared if equal, then `0x470a10` | multiplayer gate, `*[0x64f750]` is zero in single player |
| `0x4a02c8` (frame function `0x4a0220`, the skip path) | cleared after `0x4ed480(old, 0)` finishes that animation | `0x536000(0)` is non-zero, i.e. `[0x75cb60]`, the last key code the DirectInput loop at `0x535a80` stored (`0x535c8b`): **any key** ends the intro |
| `0x472e5f` | stores the zero register | a world/mission reset path (function ending at `0x472e70`) |

So the cinematic ends in one of two ways: the animation reaches its end and fires callback 0, or any key press takes
the skip path. Both clear **only** this flag (the skip path also stops the animation); neither touches `veh+0x91d`.

## 2. `veh+0x91d`: dynamics-frozen flag, and who clears it

The vehicle loop at `0x4898f9` reads `veh+0x91d` and, when it is set and `veh+0x91f` is clear, zeroes `veh+0x944` and
skips the whole dynamics step (`jmp 0x489b22`, so `0x41f810`/`0x41c270` do not run for that vehicle). Setting it is
what keeps the player rigid in the cinematic; the hand-over is whatever clears it.

* **Set to 1:** `0x46563f` (new-game start), callback id 11 `0x47e1ae` (also `veh+0x91e` = 0, then 1 at `0x47e1c4`),
  `0x47eb5b`, `0x47efd6`, `0x47e8e0`, `0x47636b`, `0x47bb1f`, `0x48ba35`, `0x49c966`, `0x4b0f76`, `0x4b8433`.
* **Cleared (constant 0 or a zero register):** `0x47f625` and `0x47f967` (tail of `0x47f1f0`, together with `0x91f`),
  `0x48049e` (the release `0x480480`, together with `0x91f`, `0x91e`), `0x4805ad`, `0x4753a1`, `0x4763df`, `0x47fecd`,
  `0x493e4e` (`0x493e40(veh)`: only when `veh` is the player), `0x4b05e5`, `0x4b1013`, `0x497cf5`, `0x498209`;
  computed from a source: `0x4b424c`, `0x494bb9`, `0x49ca0a`.
* **The release that the cinematic end can reach: `0x480480(veh)`.** It clears `0x91d/0x91f/0x91e`, finishes the
  vehicle's animation `[veh+0x6d0]` (`0x4ed480`) and, for the player, calls `0x47f1f0(0, 0)` (`0x4804d9`). Its callers:
  `0x470a10` (`0x470a91`) and the two player respawn paths `0x49ba27` / `0x49bb46` (not the launch).
  `0x470a10` is reached from callback 12 when `0x440ad0()` is non-zero (`0x47e234`, multiplayer) and from the
  input-action handler `0x4709f0` (action `0x13`, registered at `0x470a40`/`0x537bf0` by `0x470a10` itself).
  In the single-player branch of callback 12 (`0x47e240..0x47e2b4`) the code instead starts the animation named by
  `[0x71bb68]` (`0x4edda0`) and stores it in `[0x71bb6c]`; it calls `0x480480` nowhere.

**Not established:** which of these runs at the end of M01's launch. The static chain (callback 12 in single player
starts `[0x71bb68]`; the input action `0x13` handler is registered only from `0x470a10`) does not show a caller of
`0x480480` on the single-player launch path, so the unknown `cinematic-flag-clear` of #1678 is **narrowed, not
closed**: candidates are the animation `[0x71bb68]` naming a record whose callbacks reach one of the clearers above, and
the input-action `0x13`. What `[0x71bb68]` holds in M01 is not read here.

## 3. Writers of the throttle `veh+0x124` that can run for the player

| site | value | condition |
| --- | --- | --- |
| `0x47f28b` | **0.4** (`0x3ecccccd`) | `0x47f1f0(0, ...)` with `veh+0x91d` set (cinematic placement, callback 10) |
| `0x47f313` | **0** | `0x47f1f0(0, ...)` with `veh+0x91d` clear, i.e. the release `0x480480` |
| `0x47f3fb` | 1.0 | `0x47f1f0(non-zero, ...)` when `0x4639b0` is true (not the launch) |
| `0x47f45c` | `[0x71bb5c]` | `0x47f1f0(non-zero, ...)` start-state path, no direct store to `0x71bb5c` (unreached-start-state of #1678) |
| `0x487ab0`, `0x487ac7`, `0x487af7`, `0x487b10` | `+= dt * 0.5` (keys 7/8) clamped to 1.0 (or +/-5.0 when `veh+0x384`) | player input handler `0x487a30` |
| `0x487c8a` | 0, 0.125, 0.25 ... 1.0 | the number-key presets (keys 9..0x11) |
| `0x420a30` | slewed by `dt * [0x603508]` toward a speed threshold, clamped to `[0x61fb68]`/`[0x61fb6c]` | route-following controller in `0x41f810` using `veh+0x89c`, `0x9a4`, `0x9b0` (AI waypoints); whether the player takes it is not determined |

Others (`0x417fbd`..`0x4524a1`, `0x4578c3`.. etc.) write `+0x124` of other object classes (the `ebp`/`esi` bases of
unrelated structures); the player-vehicle ones are those above. **So the throttle after hand-over is 0.4 or 0, plus
whatever input the player gives; no instruction raises it above 0.4 by itself.**

## 4. Writers of the velocity `veh+0x924`

The vector is only ever addressed by `lea reg, [veh+0x924]` (about 35 sites) and passed on. The assignments the
launch can reach are in `0x47f1f0`: with `veh+0x91d` set, `0x47f2b5..0x47f2ec` store `axis * [0x608040]`
(`-20.0`, f32) where the axis is `0x53fb40` of the body matrix and `(0,0,1)`; with it clear, `0x47f324..0x47f33d` copy
the vec3 at `0x75d1b8` (above `.data`'s raw size, no direct store anywhere in the image, used as the zero vector by
about 30 call sites: `static` reading, not proven zero) and set `veh+0x930/0x934` from it. The handler of callback 951
`0x47e557` (x `-53.6448`, `0x60803c`) is, as #1678 found, fired by no record of the launch chain. The per-frame
integration is in the dynamics step `0x41c270`/`0x41f810` and the `0x48axxx..0x492xxx` callers that take `veh+0x924`
(`0x48a4e2`, `0x48a991`, `0x48b16b`, `0x48b182`, `0x48b56e`, `0x48c41e`, `0x48e470`, `0x48e99b`, `0x490321`,
`0x490810`, ...); those were listed, **not read**.

## 5. What stays unknown, what it affects, who resolves it

| unknown | affects | resolver |
| --- | --- | --- |
| `launch-flag-clear-site`: which of the clearers in section 2 runs for M01's single-player launch, and when | when the dynamics start, and whether the release (throttle 0, velocity 0) or the cinematic placement (throttle 0.4, velocity -20 units/s) is the state at hand-over | static: what `[0x71bb68]` holds in M01 and its records' callbacks; or the owner's per-frame capture |
| `flight-model-equilibrium`: thrust/drag integration of `veh+0x924` from `veh+0x124` (`0x41c270`, `0x41f810`) | whether 113.6 m/s at throttle 0.4 (or at the throttle the capture shows) is the model's own equilibrium, and the unit of the `-20.0` | a follow-up static read of the dynamics step with the vehicle record's numbers |
| `velocity-unit`: `-20.0` units/s vs the capture's m/s | the first velocity | the same follow-up |
| `no-run` | every claim | the owner's reference capture of `veh+0x124`/`veh+0x924` from the cinematic end to t=3 s |

Claim ids: `OWNER-STATIC-2026-10-23` class `static` for every row below; none is `verified_original`.

## Instruction bytes cited (re-read by the ignored test)

```bytes
4897dd: a10cc57100  # flight update reads [0x71c50c]
4897f3: 0f8537060000  # jne 0x489e30: no integration while [0x71c50c] is set
472e5f: 891d0cc57100  # store of the zero register into [0x71c50c]
47e0bc: c7050cc5710000000000  # callback 0: clear [0x71c50c] when it equals the argument
47e22a: c7050cc5710000000000  # callback 12, branch where 0x440ad0 returns non-zero: clear [0x71c50c] when equal
47e2e6: 89150cc57100  # callback 20: [0x71c50c] = argument
4a02ac: 391d0cc5710074  # skip path: cmp [0x71c50c], 0
4a02c8: 891d0cc57100  # skip path: clear [0x71c50c]
4898f9: 8a861d09000084c074168a861f09000084c0750cc6864409000000  # vehicle loop: veh+0x91d set and veh+0x91f clear skips the dynamics step
46563f: c6801d09000001  # new-game start: veh+0x91d = 1
47e1ae: c6801d09000001c6801e09000000  # callback 11: veh+0x91d = 1, veh+0x91e = 0
47f25f: 38981d0900000f  # 0x47f1f0: branch on veh+0x91d
47f28b: c78024010000cdcccc3e  # 0x47f1f0 with veh+0x91d set: throttle veh+0x124 = 0.4
47f2aa: d80d40806000  # 0x47f1f0 with veh+0x91d set: velocity = axis x [0x608040] (-20.0)
47f313: 899824010000  # 0x47f1f0 with veh+0x91d clear: throttle veh+0x124 = 0
47f31e: 8b35b8d175008d88240900008bd189328b35bcd175008972048b35c0d1750089  # 0x47f1f0 with veh+0x91d clear: velocity veh+0x924 = vec3 at 0x75d1b8
47f625: 88981d09000088981f090000  # 0x47f1f0 tail: veh+0x91d and veh+0x91f cleared
480485: 8a861d090000  # 0x480480: release reads veh+0x91d
48049e: c6861d09000000c6861f09000000c6861e09000000  # 0x480480: clears veh+0x91d, +0x91f, +0x91e
4804d9: e812edffff  # 0x480480: calls 0x47f1f0(0, 0) for the player
470a91: e8eaf90000  # 0x470a10 calls the release 0x480480
47e234: e8d727ffff  # callback 12 calls 0x470a10
4709f0: c605a3c1710001e814000000  # input-action handler 0x4709f0 calls 0x470a10
536006: a160cb7500  # 0x536000 returns [0x75cb60]
535c8b: 893560cb7500  # DirectInput loop stores the key code into [0x75cb60]
487ab0: d99624010000  # player input: throttle veh+0x124 integrated from key rate
487ac7: c786240100000000803f  # player input: throttle clamp 1.0
487af7: c786240100000000a040  # player input: throttle clamp 5.0 when veh+0x384 is set
487b10: c786240100000000a0c0  # player input: throttle clamp -5.0 when veh+0x384 is set
487c8a: 898e24010000  # player input: throttle preset from the number keys
```

## Reproduction

```sh
CS_ENGINE_IMAGE=<crimson.decrypted.exe> \
  cargo test --locked -p cs_app --test cs_app_tests_d -- accept_m01_handover_velocity_trace_ --include-ignored
```
