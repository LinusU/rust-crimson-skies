# `--smoke-success` part B: the drop-off landing reaches a refused record (#2082)

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

## Measured: the run stops at the landing

`cs --cs-path <retail> --mission M01 --smoke-seconds 400 --smoke-success`: the proximity
beat passes as in part A. The landing beat then flies 250 placements through `do_approach1`
(at (-8886.5, 85.2, -4678.0) m, 80 m over the wreck). The host's landing ledger shows the
consumer **did** land: all six rows attempted at the same tick, nodes armed by
`enable_dropoff`, no window refusals. Every one of the six activations is
`ActivationOutcome::Refused`, because the animation player refuses the row:

> `Undeclared`: no member of the mission archive, its world group or the shared root
> declares this animation name; whether the original engine resolved it elsewhere is
> unmeasured

So no `texdrop` is ever called, OBJECTIVE21 never completes, OBJECTIVE22/23 never run, the
Kestrels are never woken, and the Kestrel beat never starts. The census (section 1) says the
`do_approachN` records run `do_direction` and `CALL_ANIMATION texdrop`, so a record exists;
what is missing is a declaring member that names `do_approach1..6` (and, probably,
`texdrop`, `do_direction`) for the landing player's join. Nothing was guessed or forced.

## Not established

* The Kestrel beat (placement astern, guns key, damage-authority kills) has not run on
  retail: its checks pass only on the synthetic measurements of the pure tests. It needs the
  landing to complete first.
* Where `do_approach1..6` are declared in C3 (or how the original resolves them).
