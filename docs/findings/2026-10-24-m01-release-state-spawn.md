# M01 spawn is the release state, not 55 m/s (#1845)

Date: 2026-10-24. Implementer: `sonnet-2` (Sonnet 5.5); no independent review yet. Evidence class: `static`
(claim id `OWNER-STATIC-2026-10-23`, `2026-10-23-m01-handover-dynamics-step.md` section 3) plus the one owner
run of #1746 (retail 1.02, memory reads, times +-1 s). Nothing here is `verified_original`.

## What changed

The composed M01 player no longer spawns at the declared 55 m/s along the nose with the actual throttle at the
playtest's 0.75. `MissionPlayerStart` now carries `velocity_m_s = [0, 0, 0]` and `start_throttle = 0`:

* the engine's hand-over (`0x47f1f0`) sets either the *release* state (velocity the zero vector `0x75d1b8`,
  command 0) or the *placement* state (20 m/s along the nose, command 0.4), then copies command to actual
  throttle (`veh+0x128 = veh+0x124`) and zeroes angular momentum. Neither is 55 m/s;
* the owner's run moves the body 3.3 m in its first second and accelerates over about five: the release shape.
  The placement state would already be at 20 m/s. So the release state is the one the evidence supports;
* the law slews the actual throttle toward the command at 0.5 per second (`THROTTLE_SLEW_PER_S`), which is the
  spool-up. From rest and with the declared command 0.75 our law reaches 117 m/s after 5 s and moves about
  12 m in the first second (the original: 3.3 m).

`RETAIL_START_SPEED_M_S` stays for the retail *playtest* scene and the smoke's own scripted placements; it is
no longer the M01 mission's spawn.

## What stays unknown (each gates any fidelity claim about M01's first minute)

| unknown | affects | resolver |
| --- | --- | --- |
| `launch-flag-clear-site`: release or placement at M01's end | 0 vs 20 m/s, throttle 0 vs 0.4 | #1978 open item, or the owner's per-frame capture |
| `hand-over-command`: what sets the command after the hand-over (the composed command is still the declared 0.75) | cruise speed, sink | the owner's capture of `veh+0x124`, `+0x128`, `+0x924` |
| `sink-at-rest`: our law has no lift below 2.4384 m/s, the original sinks 0.2 m in its first second | the first seconds | the same capture; `force-block` re-read |

## Consequence, recorded not hidden

With no lift at rest and no input, the composed body now falls, meets the sea and crashes (two judged contacts,
1688 hull damage) between t = 3 s and t = 9 s, where the original flies on to t = 28 and beyond. Before, the
declared 55 m/s kept it level at 100 m/s, which agreed with nothing measured either. The pin in
`accept_parity4_m01_collision_no_input_trajectory_is_pinned` moves with it. `accept_parity4_m01_spawn_is_the_release_state_and_spools_up`
pins the spawn state and the spool-up from the production recipe.

Tests that placed a body at speed through its physics velocity only (`m01_scenery_colliders`, the wing-fire
test) now also reset the law's own throttle and angular momentum, which the hand-over no longer leaves at
cruise values.
