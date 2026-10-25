# `--smoke-success` part C: the hydrogen tank, the docking and the settled Success (#2083)

Date: 2026-10-25. Capability: `retail`, `gpu`. A scripted run; not `human_play`, nothing
`verified_original`. Tests: `accept_m01_smoke_success_c_` in
`crates/cs_app/tests/campaign/accept_m01_smoke_success_c.rs` (pure judging).

## Which blocks

The task text names "OBJECTIVE18, 24" and "OBJECTIVE24's 123 s NAP". That is the C1C numbering
from before the retarget; the C3/M01 program has no such nap (`2026-10-24-m01-c3-win-route-census.md`).
The end of the C3 route is OBJECTIVE25 (`hydrogen_fire` RUNNING, block index 24) -> OBJECTIVE13
(group 1 down and block 26 awake, index 12) -> 5 s -> OBJECTIVE14 (`hooked_to_klondike` EXECUTED,
index 13) -> OBJECTIVE15 (`INSTANTWIN`, index 14). The control lowering numbers a block's symbol by
its index (`control_lowering.rs`), so the terminal's `requested_by` is `SymbolId(14)`, OBJECTIVE15.

## What the stage now does

After the Kestrel beat (`smoke/success.rs`):

1. **Tank beat.** The player is placed (test placement, every 2 frames) 60 m south of and 45 m
   below `hydrogentank1`, nose on it, and presses the guns key through `MissionCockpitActions`
   until the host reports block 24 completed. Abeam at tank height the guns put no hit on the pool:
   1771 presses from 60 m level left it at 15/15 with the girder frame of the zeppelin's underside
   in the line of fire (frame `success_tank` of that run). From below, 22 presses emptied it.
2. **Dock beat.** A level pass at 100 m/s, 60 m out, re-placed every 6 frames on `pz_manual_land`'s
   latest composed position (the zeppelin moves), until block 13 completes.
3. **Settle beat.** Waits for `MissionHost::terminal()`; no flag is invented. Production writes the
   app exit the moment a Success terminal settles (there is no success debrief screen, #1972), which
   ended the run before any frame could follow. The smoke therefore inserts `MissionExitHeld`
   (host.rs): the terminal still settles, the exit message is not written, and the exit code the
   terminal asked for stays on `MissionTerminal::exit` and is judged (`0`).

## Measured

`cs --cs-path <retail> --mission M01 --smoke-seconds 700 --smoke-success --capture-dir
private/evidence/M01-SMOKE-SUCCESS-C`, all eleven checks pass:

* Blocks 11, 26, 20, 22 as in part B (ticks 2473, 2593, 2773, 3374); Kestrels down at 3784/4168/4552.
* Tank: pool `hydrobombboom1 on hydrogentank1` 15 -> -3.75, `hydrobombboom1` Started at host tick
  9325; block 24 completed at script frame 4664, block 12 at 4665.
* Dock: 55 passes; `pz_manual_land -> hooked_to_klondike` Started at tick 9928 (`pz_auto_land`
  AlreadyRunning); block 13 completed at 5085.
* Terminal: `outcome=success source=control_program tick=10169 requested_by=SymbolId(14)`
  `exit_code=0`, run time 1:24, `planes_downed=4`.
* Frames off black (floor 300 per mille): near/marked/landed/Kestrels 999, plus
  `frame_sc04702_success_tank`, `frame_sc04754_success_hydrogen` and `frame_sc05206_success_settled`.
  They are in `private/evidence/M01-SMOKE-SUCCESS-C/`, not committed. The hydrogen frame shows the
  zeppelin's underside (no fire visible from there); the settled frame shows open sky behind the
  player, still placed-flying.

## Not established

* The tank kill relies on designed rules: the `NonFriendly` gun rule and tank hit volume (#2099),
  the child-target seating (#2096), not measured original behaviour. That a level approach is
  blocked by the girder frame is the composed model's, not a measurement of the original.
* What the original shows after the win: the Mission Succeeded debrief and Return to Cabin (#1972).
* Whether the original docks in one pass at 100 m/s: the landing rows are the designed rules.
