# PARITY-M01-ATMO-E3 (#1678): what places the player's body when the M01 launch drop stops

Date: 2026-10-19. Task #1678, follow-up of #1660
(`docs/findings/2026-10-18-m01-launch-end-pose.md`, unknowns `release-rule` and
`release-velocity`). Implementer: `sonnet-2`. Capabilities used: **`retail`**
(read-only `$CS_GAME_DIR`) and **static analysis** of the owner-supplied
decrypted image (`$CS_ENGINE_IMAGE`, sha-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`,
VA = file offset of the mapped image + `0x400000`, `llvm-objdump -d -M intel`).
No original executable ran and no `original-launch/` frame exists on this
machine: **nothing here is `verified_original` and no visual comparison is
claimed.** Test prefix `accept_parity_m01_atmo_e3_`. Production reader:
`cs_app::cinematics::launch_release` (next to `launch_end_pose`).

## Verdict

1. **The drop's end pose is never written to the player's body by anything in
   the launch chain.** The one routine that copies a node's world pose into
   the player body (the handler of callback id 951, `0x47e557`) is fired by
   `texdrop` and `pzep_launch_player.all_done` in M01's `mis_anim.zbd` and by
   no record of the launch chain (`generic_intro` and the 21 animations it
   reaches). The first control frame is therefore **not** the drop's end.
2. **The body is placed at the *start* of the intro, not at its end.** The
   intro's `RESET_SEQUENCE` fires callback 10, whose handler calls the
   engine's "place the player" routine `0x47f1f0(0, 0)`. With the player in
   the cinematic state (`veh+0x91d` = 1, set just before `NEW_GAME_START`
   runs, `0x46563f`) that routine adds 100.0 to the body's y, sets throttle
   0.4 and the velocity to the body's `(0,0,1)` axis times −20.0, writes the
   node from the body (`0x4d1d50`, `0x4d1a30`) and clears the cinematic bytes.
3. **While the intro runs the player's flight update does not integrate**
   (`0x4897c0` branches to `0x489e30` while `[0x71c50c]`, set by callback 20
   and cleared by callback 0 on finish, is non-zero). So the body keeps the
   state step 2 gave it until the intro finishes, whatever the node does.
4. **`STOP_ANIMATION` of the drop writes nothing to any node.** The drop's
   record has `RESET_TIME` −1.0, so the stop finishes it at once through
   `0x4ed190`, which stops effects and children, cancels the timer and sets
   the state byte. The only code that writes saved node poses back is
   `0x4ed090`, reachable only from an animation *start* and only when the
   record carries `AUTO_RESET_NODE_STATES`; none of the records that list the
   `player`, `camera1` or `piratezep` objects does.

What is **not** established (see `RELEASE_UNKNOWNS`): the body's state when
step 2 runs (hence whether the first control frame is the stored `aiv.zrd`
pose), what detaches the player node from the Pandora after the intro, which
code ends the cinematic state, and the velocity's direction convention. The
start velocity is therefore known only as far as: **magnitude 20.0 engine
units, along the body's own axis**, throttle 0.4, both read as constants, not
run.

## Corrections to #1660

* **`RESET_SEQUENCE` runs when the animation starts, not when it ends.** The
  instance is the record copied verbatim (flag word `+156` = runtime `+0x9c`,
  `RESET_TIME` `+172` = `+0xac`, reset block `+208` = `+0xd0`, damage block
  `+212` = `+0xd4`). `0x4ed340` (start) rewinds (`0x4ebfa0`) and steps
  (`0x4ecbb0`) `[+0xd0]` and schedules the per-frame tick `0x4ed2a0` that keeps
  stepping it. #1660 described it as "the state the record returns to"; for
  `generic_intro` it is the *setup* that runs first (the player leaves the
  Pandora's child list and goes to `world1`), after which `start_script`
  re-parents the player to the Pandora. That is why both orders appear in the
  record.
* The block at `+0xd4` is the **damage sequence**, run by `0x4e71e0` from the
  hit callbacks `0x4e7150`/`0x4e7220`, registered (`0x523287`..`0x5232e3`) only
  for activation bytes 0, 1 and 2. `generic_intro` is activation 3
  (`ON_CALL`, parser `0x51e350`; started by name from `startanims.zrd` through
  `0x46c370` → `0x4edda0`): nothing re-runs a reset for it at its natural end. The only
  restart found is the skip path `0x4a02ac` (`0x4ed480` on `[0x71c50c]`).

## The chain, link by link

| link | value | where |
| --- | --- | --- |
| executor table | opcode 25 `STOP_ANIMATION` → `0x4ebc80`; opcode 35 `CALLBACK` → `0x4ec5e0` (registered `0x4ee297`, `0x4ee2f6`; dispatcher `call [4*eax+0x727de0]` at `0x4ecc7f`) | image |
| stop of a child | `0x4ebc80` finds the child by name and calls `0x4ebbb0`; `[+0xac]` < 0.0 → `0x4ed190` now, otherwise a timer whose expiry restarts the child through `0x4ed4d0` | image |
| `gi_playerdrop` | activation 3, flags `0x00c00000`, `RESET_TIME` −1.0 | `ZBD/C3/cam_anim.zbd` |
| finish `0x4ed190` | calls `0x4eba60`, `0x4ebae0`, `0x4ebb30` (sound/particle/light objects) and `0x4ebbb0` (children); no transform setter | image |
| node restore `0x4ed090` | called only from `0x4ed424` (inside `0x4ed340`); begins `test [+0x9c], 0x40000`; writes `0x4d1d50`, `0x4d1a30`, `0x4d18d0` | image |
| flag `0x40000` | `AUTO_RESET_NODE_STATES ON` (parser `0x51e775`) | image |
| who carries it in the chain | `speed_cue` (`0xc40020`), `letterbox` (`0x440020`), `apzep_engines_start`, the twelve `apz?prop??` (`0x440000`), `wing_lights_blink` (`0x640020`), `spinprops` (`0x640004`). Their `Objects` tables name propellers, lights and the letterbox; none names `player`, `camera1`, `piratezep` | `cam_anim.zbd` |
| who does not | `generic_intro` (`0x00c00010`), `gi_scene1` (`0x00c00020`), `gi_scene2` (`0x00400000`), `gi_1stperson` (`0x00c00030`), `gi_playerdrop`, `gi_pfighter1/2` (`0x00400020`) | `cam_anim.zbd` |
| callback handler | `0x47e080(anim, vehicle, id)`; ids 0..13 through the dword table `0x47f160`, id 20 → `0x47e2e1`, ids ≥ 0x392 through the byte table `0x47f1b8` and dword table `0x47f198` | image |
| ids the chain fires | 1, 2, 10, 11, 14 (no handler), 20, 0x29a/0x29b (667), 0x392 (914) and, in `gi_1stperson`, 0x29a (666) | `cam_anim.zbd` |
| id 10 | HUD pieces, then `0x47f1f0(0, 0)` unless `*[0x64f750]` (via `0x440ad0`) is non-zero | `0x47e11e`..`0x47e164` |
| id 951 | `0x47e557`: world position (`0x4cf200`) and rotation (`0x4cf380`) of the node → body (`veh+0x204`, `+0x150`, `+0x1a4`), detach (`0x4cd6d0`), node write-back, velocity from `veh+0x198..0x1a0` × `[0x60803c]` (−53.6448) | image |
| who fires 951 | `texdrop` (unnamed sequence, `+432`) and `pzep_launch_player` (`all_done`) in `ZBD/C3/M01/mis_anim.zbd`; both are activation 3 (`ON_CALL`) mission animations, neither is in the launch chain | `mis_anim.zbd` |
| `0x47f1f0` | `veh+0x204/0x208/0x20c` position, `+0x1f8..0x200` euler, `+0x150/0x180` matrices, `+0x1a4` copy, `+0x924` velocity, `+0x124` throttle. Branches: debug start pose (`[0x71daca]`, set by a console command); `arg == 0` with `veh+0x91d` set (the intro): y += `[0x6032e4]` (100.0), throttle 0.4, velocity (0,0,1)-axis × `[0x608040]` (−20.0); `arg == 0` with it clear: throttle 0, zero velocity; `arg != 0`: start pose globals `0x71bb44..0x71bb60` | image |
| callers of `0x47f1f0` | `0x4440bf`, `0x47e15f` (id 10), `0x4804d9`, `0x4805ef` (the two latter in vehicle release functions that restart `[veh+0x6d0]`) | image |
| flight update gate | `0x4897dd` `mov eax,[0x71c50c]`; `0x4897f3` `jne 0x489e30` | image |

`accept_parity_m01_atmo_e3_engine_facts_are_the_bytes_the_finding_read`
re-reads the instruction bytes, call targets, table entries and constants of
every row from the image; `accept_parity_m01_atmo_e3_retail_chain_never_restores_or_releases`
recomputes the chain, the flags, the reset steps and the callback sites from
the container.

## The intro's reset steps (data)

`generic_intro` `RESET_SEQUENCE`, in order: delete `camera1`, `player` and
`piratefighter` from `piratezep`; callbacks 1, 0x392, 10, 0x29b; add `player`
to `world1`; call `speed_cue`; visibility and rotation states. The ordinary
sequences with initial state 0 (`callback_sequence`: callbacks 20, 2, 11, 14;
`start_script`: re-parent `camera1` and `player` to `piratezep`, call the
engine, letterbox and `gi_scene1`/`gi_scene2` animations) run beside it.

## What stays unknown, what it affects, who resolves it

The same table is `RELEASE_UNKNOWNS`; the production reader returns none of
these values and fills in none of them.

| unknown | affects | resolver |
| --- | --- | --- |
| `body-state-at-intro-start`: what `veh+0x204/0x1f8` hold when id 10 runs `0x47f1f0`. The loader `0x47c210` writes the stored `aiv.zrd` position and heading to the node only; the constructor `0x4aff80` and the player subclass were not read | the first control frame's position and heading (whether it is the stored record) | follow-up |
| `node-detach-after-intro`: nothing read removes the player node from the Pandora after the intro | the first frames' visible position | follow-up |
| `cinematic-flag-clear`: which code clears `veh+0x91d/0x91e` (id 11 sets them after id 10's placement cleared them) | when control returns | follow-up |
| `multiplayer-gate`: `*[0x64f750]` is zero in single player | whether the start placement runs | follow-up |
| `velocity-direction`: sign/axis of `0x53fb40(matrix,(0,0,1))` × −20.0; units | the first velocity vector | #1673 |
| `unreached-start-state`: `0x71bb44..0x71bb60` have no direct store | the `arg != 0` branch only | follow-up |
| `no-run` | every claim | the owner's reference capture |

## Files

* `crates/cs_app/src/cinematics/launch_release.rs` (new): `measure_launch_release`,
  `callback_sites`, `ENGINE_FACTS`, `CALLBACK_MEANINGS`, `RELEASE_UNKNOWNS`.
* `crates/cs_app/src/cinematics/launch_end_pose.rs`: helper visibility only
  (`pub(super)`).
* `crates/cs_app/src/cinematics/mod.rs`, `crates/cs_app/tests/campaign/mod.rs`:
  module declarations (wiring).
* `crates/cs_app/tests/campaign/parity_m01_atmo_e3.rs`: two synthetic members
  (CI), one `#[ignore]` member needing `CS_GAME_DIR`, one needing
  `CS_ENGINE_IMAGE`.

## Reproduction

```sh
CS_GAME_DIR=<original installation> CS_ENGINE_IMAGE=<crimson.decrypted.exe> \
  cargo test --locked -p cs_app --test cs_app_tests_d -- accept_parity_m01_atmo_e3_ --include-ignored
```
