# PARITY-M01-ATMO-E5 (#1685): what writes the M01 player body before launch callback 10, and what ends the cinematic state

Date: 2026-10-19. Task #1685, follow-up of #1678
(`docs/findings/2026-10-18-m01-launch-release.md`, unknowns
`body-state-at-intro-start`, `node-detach-after-intro`, `cinematic-flag-clear`)
and part of the evidence #1652 needs. Implementer: `claude-2`. Capabilities used:
**`retail`** (read-only `$CS_GAME_DIR`) and **static analysis** of the
owner-supplied decrypted image (`$CS_ENGINE_IMAGE`, sha-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, VA = file
offset + `0x400000`, `llvm-objdump -d -M intel`). No original executable ran and
no `original-launch/` frame is used as a source of numbers: nothing here is
`verified_original`. Test prefix `accept_parity_m01_atmo_e5_`. Production
reader and replay: `cs_app::cinematics::launch_body`.

## Verdict

1. **The body's pose at callback 10 is determined, and it is the stored record,
   not something else.** The loader `0x47c210` sets the player's scene node from
   the `aiv.zrd` `player` record, constructs the vehicle (which zeroes its pose)
   and then calls `0x476250`, which copies the node's world position and Euler
   angles into the body. Nothing between that call and callback 10 writes the
   body. Callback 10 then runs `0x47f1f0(0, 0)` **with `veh+0x91d` set** (the
   mission-load code sets it right before it starts `NEW_GAME_START`), so the
   intro branch runs. Result, for M01 (stored position (−1426, 150, −1813),
   heading 40):

   | body field | value |
   | --- | --- |
   | position `veh+0x204..0x20c` | **(−1426, 250, −1813)** (stored y + 100.0) |
   | Euler `veh+0x1f8..0x200` | (0, **0.698132 rad** = 40°, 0) |
   | throttle `veh+0x124` | 0.4 |
   | velocity `veh+0x924` | **(−12.856, 0, −15.321)** = `(0,0,1)` rotated by the orientation × −20.0 |

   The node is written to the same position and angles by the same routine
   (`0x4d1d50`, `0x4d1a30`). The pose is conditional on two readings that were
   not made (below: `airframe-mode`, `network-gate-value`), so it is *determined
   by the code read*, not verified against a run.
2. **Units and sign.** The stored position is in world metres (owner note
   2026-10-05, 1 unit = 1 metre, +Y up, right-handed), and `+100.0` is added to
   the same y, so the raise is **100 m up**. The `(0,0,1)` axis is rotated by the
   quaternion `0x53f610` builds from `(yaw, pitch, roll) = (+0x1fc, +0x1f8,
   +0x200)`; for a pure yaw ψ it is `(cos ψ/2, 0, sin ψ/2, 0)`, and `0x53fb40`
   rotates actively (`(2w²−1)v + 2(q·v)q + 2w(q×v)`), so the body's +Z axis is
   `(sin ψ, 0, cos ψ)` = **(0.643, 0, 0.766)** at 40°, i.e. heading 40 turns +Z
   toward +X (the same convention #1673 found). The velocity is **−20.0 times that
   axis**: the body moves along **−Z of its own frame**, opposite to the +Z
   axis, at a speed of 20 (units per second is the unit convention; the
   integrator was not read: `velocity-integration`). As a cross-check only, not a
   source of numbers: `private/ref-drydock/m01/orig25.png` shows the first
   control frame heading north-west with the island ahead, which agrees with a
   forward direction (−0.64, 0, −0.77) when −Z is north.
3. **The cinematic state and the node are not released by the launch chain.**
   The chain moves the player node exactly four times, all at START_TIME 0
   (delete from the Pandora, add to `world1` around callback 10 in the reset
   block; delete from `world1`, add to the Pandora in `start_script`). Nothing
   later detaches it. The only code read that detaches a node and copies its
   world pose into the body is the callback-951 handler, and no data calls the
   animations that fire 951. `veh+0x91d/0x91e` are cleared by callback 10, set by
   callback 11 right after, and no cleaner reachable from the chain was
   identified. Both stay **unknown**; see the table at the end.

For #1652: the **body state at the end of callback 10 is fully determined**
(table above); the **pose at the first control frame is not**, because what ends
the cinematic state and detaches the node is unknown. Binding the table above as
"the first control frame" would be a guess.

## 1. What writes the body before callback 10

| step | address | what it does | status |
| --- | --- | --- | --- |
| player pointer | `0x473060` | stores its argument in `[0x71c298]`, the player vehicle. Callers `0x475397` (the first entry of the loaded list), `0x47bc35`, `0x48003c` | static |
| first list entry | retail `aiv.zrd` | `recover_retail_aircraft_records`: the first enabled record is `player`, stored position (−1426, 150, −1813), heading 40 | measured |
| node placed | `0x47c4c0`, `0x47c4d8`/`0x47c4fa` | the loader calls `0x4d1d50(node, record+0x18..0x20)` and `0x4d1a30(node, 0, record+0x24 × π/180, 0)` (the double at `0x6040e8` is 0.017453292519943…) | static |
| vehicle built | `0x47c51d` → `0x4aff80` | the constructor stores the zero vector (`0x75d1b8`, in `.bss`) in `veh+0x1f8` (euler) and `veh+0x204` (position) and zero in `veh+0x150`: a new body has no pose of its own | static |
| **node → body** | `0x47c77b` → `0x476250` | reads the node's world position (`0x4cf200`) into `veh+0x204`, its stored angles (`0x4d1b40`) into `veh+0x1f8..0x200`, builds the quaternion (`0x53f610`), the matrix copy (`0x53bf40`) and `veh+0x1a4`, then sets `veh+0x91d = 1`, `veh+0x91e = 0` (`0x47636b`, `0x476375`). Its only caller is the loader | static |
| list registered | `0x4735b0` (`0x475397`, `0x4753a1`) | registers the player and clears `veh+0x91d/0x91f/0x91e` | static |
| flags set, intro started | `0x46563f`, `0x465646`, then `0x46c370("StartAnims.zrd", "NEW_GAME_START")` at `0x465659` | `veh+0x91d = 1`, `veh+0x91e = 0` immediately before the start animations run, so callback 10 sees `0x91d` set | static |
| debug override | `[0x71daca]` | one setter (`0x4a75fe`, the console command that parses six numbers) and one clear (`0x4b376e`); `0x47f1f0` and the post-load code only take the debug branch when it is set, so an ordinary start does not | static |

### Ruled out as the body's writer

* **`0x47b1fe` / `0x47b2e8`**: they store through `[edi+0x204]`, but `edi` is the
  *airframe type record*, not the vehicle: the same routine reads the keys
  `dare_devil`, `natural_touch`, `sixth_sense`, `stun_recovery` and
  `preferred_engagement_altitude` (`0x628104`..`0x628160`) into `+0x1ec..0x210`
  and `mode` (`tank`, `ship`, `jet`, `heli`, `wingman`, `plane` → `+0xa4` =
  2, 3, 0, 1, 4, 5). The constructor family around `0x478d80` writes the same
  offsets of that struct.
* **`0x47f02e`, `0x45bc48`, `0x4524c4`, `0x45a3c0`**: other routines (a vehicle
  placed beside the player at `[0x71c4f0]`, the debug start pose, two others
  that restart a vehicle). They write other vehicles' or the debug pose; none
  was shown to run for the player between the load and callback 10, and the
  load-to-intro path read above calls none of them.
* **The per-frame flight update `0x4897c0`**: skipped while `[0x71c50c]` is set,
  the load code calls `0x46c370` directly after the flags are set, and the
  reset block (callback 10) is stepped inside the animation start, so no frame
  (and no integration) falls between the load and callback 10.

### The terrain snap

The loader calls `0x47fad0` on the position (adding the terrain correction to
y) only when `type+0xa4` is 2 (`tank`) or 3 (`ship`) (`0x47c2b4`..`0x47c2e5`).
The M01 player airframe's `mode` was not read from the retail configuration;
an aircraft is neither, but this is recorded as `airframe-mode`.

## 2. Callback 10 and what follows it

`generic_intro`'s `RESET_SEQUENCE`, in stream order (all START_TIME 0): delete
`player` from `piratezep`; **callback 10**; add `player` to `world1`; call
`speed_cue`. The ordinary sequences then run at the same instant:
`callback_sequence` (callback 20, then callback 11), `start_script` (delete
`player` from `world1`, add it to `piratezep`, the engine, letterbox and scene
animations). So at callback 10 the node has just left the Pandora and the body
is still the loader's copy of the stored pose.

`0x47f1f0(0, 0)` with `veh+0x91d` set (`0x47f25f`): `veh+0x208 += 100.0`
(`0x6032e4`), throttle 0.4 (`0x3ecccccd`), velocity = `0x53fb40(veh+0x150,
(0,0,1))` × −20.0 (`0x608040`) per component; it then rebuilds the matrices
(`0x53f610`, `0x53fa40`), writes the node (`0x47f5ea`, `0x47f60d`) and clears
`veh+0x91d/0x91f/0x91e` (`0x47f625`..`0x47f631`). Callback 11 (`0x47e1ae`)
sets `0x91d = 1`, `0x91e = 0`, then `0x91e = 1`.

`launch_body::place_player_at_intro_start(position, heading_degrees)` replays
exactly this arithmetic (in `f32`, like the stores) and is what
`accept_parity_m01_atmo_e5_retail_player_is_the_first_record_and_the_placed_pose_follows`
checks against the retail record.

## 3. Node detach and the end of the cinematic state

| fact | evidence |
| --- | --- |
| the chain's four child events on `player` are at START_TIME 0 only: reset block (delete from `piratezep`, add to `world1`) and `start_script` (delete from `world1`, add to `piratezep`) | `measure_intro_body_events`, test `..._retail_chain_moves_the_player_node_only_at_time_zero` |
| body callbacks fired by the chain: 10 (reset block), 20 and 11 (`callback_sequence`); never 0 (the engine fires it on finish; it only clears `[0x71c50c]`), 12 or 951 | same |
| 951 (`0x47e557`: node world pose → body, detach `0x4cd6d0`, rewrite the node, velocity from `veh+0x198..0x1a0` × −53.6448) is fired only by `texdrop` (the dropoff approach, called from `zrdr.zbd`'s program) and `pzep_launch_player`; the latter's name occurs once in the three carriers, in its own record | test `..._retail_no_data_names_the_release_animations` |
| the reset block is stepped by the animation tick `0x4ed2a0`; the instance ends through `0x4ed190` when the block finishes | `0x4ed2c7`..`0x4ed332` |
| a key press while `[0x71c50c]` is set clears it and calls `0x4ed480(instance, 0)` | `0x4a02ac`..`0x4a02ce` |
| callback 12 (`0x47e1ed`) is fired only by the `player_crash_*` / `player` reset blocks; in single player it hides the node `veh+0x3a0` and starts the animation named by `[0x71bb68]` | `callback_sites` over `cam_anim.zbd` |

What the data does **not** show is any event or engine path that takes the
player node out of the Pandora after the drop, or that clears the cinematic
flags after callback 11. Both are recorded as unknown, with the candidates
listed.

## What stays unknown, what it affects, who resolves it

The same table is `launch_body::BODY_UNKNOWNS`.

| unknown | affects | resolver |
| --- | --- | --- |
| `node-detach-after-intro` | the node's frame after the intro and the pose the first control frame starts from (the body keeps the callback-10 pose while the node keeps the last drop pose in the Pandora's frame) | tracing the per-frame node write (`0x4b3050` and callers) against a node that still has a parent, or the owner's capture |
| `cinematic-flag-clear` | when control returns | tracing the other clear sites (`0x4763df`, `0x47fecd`, `0x48049e`, `0x4805ad`, `0x493e4e`, `0x497cf5`, `0x498209`, `0x49ca0a`, `0x4b05e5`, `0x4b1013`) |
| `airframe-mode` | whether the loader snaps the node's y to the terrain before the body copy | reading the airframe record's `mode` |
| `network-gate-value` | whether callback 10 places the body at all (`*[0x64f750]`, config variable `Network`, set at `0x440247`, must be zero) | the default `0x5bde90` returns for `Network`, or the owner's run |
| `velocity-integration` | the unit of the −20.0 and the position a moment after control returns | reading the integrator behind `0x4897c0` |
| `no-run` | every claim | the owner's reference capture (REF-OWNER-FIRST-CAPTURE) |

## Files

* `crates/cs_app/src/cinematics/launch_body.rs` (new): `BODY_ENGINE_FACTS`,
  `BODY_UNKNOWNS`, `measure_intro_body_events`, `place_player_at_intro_start`,
  `name_occurrences`.
* `crates/cs_app/src/cinematics/launch_release.rs`: `with_records` made
  `pub(super)` (visibility only).
* `crates/cs_app/src/cinematics/mod.rs`, `crates/cs_app/tests/campaign/mod.rs`:
  module declarations (wiring).
* `crates/cs_app/tests/campaign/parity_m01_atmo_e5.rs`: two synthetic members
  (CI), three `#[ignore]` members needing `CS_GAME_DIR`, one needing
  `CS_ENGINE_IMAGE`.

## Reproduction

```sh
CS_GAME_DIR=<original installation> CS_ENGINE_IMAGE=<crimson.decrypted.exe> \
  cargo test --locked -p cs_app --test cs_app_tests_d -- accept_parity_m01_atmo_e5_ --include-ignored
```
