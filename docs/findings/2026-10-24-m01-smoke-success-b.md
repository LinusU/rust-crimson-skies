# `--smoke-success` part B: the drop-off landing and the Kestrel kills (#2082)

Date: 2026-10-24. Capability: `retail`, `gpu`. A scripted run; not `human_play`, nothing
`verified_original`. Tests: `accept_m01_smoke_success_b_` in
`crates/cs_app/tests/campaign/accept_m01_smoke_success_b.rs` (pure judging).

Retargeted from the C1C route (`wv_manual_land`, `rusdevastator_*`) to the C3/M01 chain of
`2026-10-24-m01-c3-win-route-census.md`: after OBJECTIVE12/27 the beats are the drop-off
landing (OBJECTIVE21, then 22 and 23) and the Kestrels `medkestrel_1..3` (group 1).
OBJECTIVE25 (`hydrogen_fire`) and the docking stay with #2083.

## What the stage now does

`crates/cs_app/src/mission_session/smoke/success.rs` continues after the marked frame:

1. **Landing beat.** The composed `do_approach1` node (position read through the production
   observation writer, #2094) is flown at level, 100 m/s, from 60 m away (inside the 150 m
   landing radius and the rows' `speed [50, 320]`), re-placed every 6 frames, until the host
   reports block 20 (OBJECTIVE21, `texdrop` EXECUTED).
2. **Kestrel beat.** The stage waits for block 22 (OBJECTIVE23), then places the player 60 m
   astern of each standing Kestrel (every 2 frames, at its speed) and presses the guns key
   through `MissionCockpitActions`. A Kestrel is down only when `SessionDamageResolver`
   says so. Four framebuffers are judged (`success_near/marked/landed/kestrels`).

## Measured (final, after #2098 and #2100)

`cs --cs-path <retail> --mission M01 --smoke-seconds 400 --smoke-success --capture-dir
private/evidence/M01-SMOKE-SUCCESS-B` on the branch rebased on main, all seven checks pass
without tuning any placement constant:

* Block 11 (shipwreck proximity) completed at host tick 2473, block 26 at 2593.
* Landing: 130 level passes through `do_approach1`; block 20 (`texdrop` EXECUTED) completed at
  tick 2773 with no player refusals (the earlier `Undeclared` refusal is fixed by #2098).
* Block 22 (Kestrel wake) completed at tick 3374.
* Kestrels: 1148 guns-key presses from 60 m marks astern (588 placements); the damage authority
  listed `medkestrel_1` (tick 3784), `medkestrel_2` (4168) and `medkestrel_3` (4552) destroyed.
  This relies on the **designed** aircraft integrity pool and hit box of #2100
  (`2026-10-25-m01-ai-aircraft-damage.md`), not on a measured original value.
* All four framebuffers are off black (999 per mille each). `frame_sc04612_success_kestrels.png`
  shows the astern view of a red Kestrel with the gun tracers and impact flashes on it; the
  other frames are `frame_sc02501_success_near`, `frame_sc02593_success_marked` and
  `frame_sc02833_success_landed`. They are in `private/evidence/M01-SMOKE-SUCCESS-B/`, not
  committed.

## Not established

* Whether the original's guns destroy `medkestrel_1..3` with the same effort: the pool and hit
  volume are designed placeholders (#2100), so the kill count and timing are not original
  behaviour.
* `OBJECTIVE25` (hydrogen fire) and the docking beat: #2083.
