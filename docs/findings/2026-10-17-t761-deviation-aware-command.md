# #761: the command the follower emits is the step it committed, and what a committed dodge can and cannot do

Date: 2026-10-17. Task: **#761** "Make the F31 command cascade deviation-aware
and prove obstacle avoidance through the production loop" (review follow-up
recorded against #526). Spec:
`specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, non-negotiable
behaviour 2 and AC01. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
Capabilities used: ordinary build/test only — no `CS_GAME_DIR` read, no
evidence report required.

## 1. The defect the review found

After #526, `Navigator::decide_with_tie_break` fed `command_for` the heading
error **to the target**:

```rust
let heading_error_rad = wrap_pi(desired_heading - state.heading_rad);
let command = self.command_for(state, heading_error_rad, &step);
```

while the planner's committed `RouteStep` already carries the heading it
bounded away from any blocker (`step.heading_rad`). When the target lay
straight ahead behind a blocker, the target error was ~0, the commanded bank
was ~0, and the kinematic plan deviated while the physical body flew at the
wall. `RoutePursuit::with_blockers` had no production caller yet, so the
latent path was uncovered.

`Blocked` had a second gap: the stationary special-case set
`desired_bank = state.bank_rad`, so `roll = 0` — a banked aircraft held its
bank and kept sinking/turning while the throttle decayed, instead of rolling
level.

## 2. The law that is now committed

`command_for` now takes the committed step error
`wrap_pi(step.heading_rad - state.heading_rad)` beside the target error and
selects the turn-rate demand by avoidance state:

```text
OnRoute:    omega = clamp(GAIN * heading_error,          +/- max_yaw_rate)
Deviating:  omega = clamp(committed_error / dt,          +/- max_yaw_rate)
Blocked:    omega = clamp(committed_error / dt,          +/- max_yaw_rate) = 0
```

- **`OnRoute` keeps the verified proportional outer loop.** An all-states
  `committed_error / dt` law was tried first and measured to **diverge**: the
  per-tick committed bound is only `max_yaw_rate * dt` = 0.0083 rad at
  120 Hz, so the demand saturates for every residual error above ~0.003 rad —
  inside the airframe's bank rollout lag. `accept_t526_lateral_rejoin`
  spiralled: max bank held continuously, the target heading swept through
  repeatedly, and the body ended 1300 m off route having never reached the
  marker. Restoring `HEADING_LOOP_GAIN_PER_S * heading_error` recovers the
  rejoin; the committed step and the target bearing agree modulo the bound
  on-route anyway.
- **`Deviating` uses the step's own rate.** The committed dodge is a rate the
  plan itself declares for the tick(s) the dodge lasts — transient, so the
  saturation that winds the on-route case cannot accumulate. The emitted
  roll signs the committed side (`DeviationSide::Left` -> `roll < 0` in the
  `FlightInput` sign convention).
- **`Blocked` falls out for free.** `stationary_step` keeps the current
  heading, so `committed_error = 0` and the demand is zero; the inner bank
  loop then commands `(0 - bank)`, i.e. wings-level, and the pitch loop
  damps the measured climb against the step's zero climb. The
  `desired_bank = state.bank_rad` special-case is gone.

## 3. What the integrated loop measured

`crates/cs_app/tests/accept_t761_deviating_command.rs` flies the production
`AiNavigationPlugin` + `FlightForcesPlugin` fixed-tick loop with
`RoutePursuit::with_blockers`:

- **Corridor keep-out (`t761_a`).** Two slabs leave a `x in [2, 40]` opening
  at `z = -60`; a straight `-Z` run ends inside the left slab. The declared
  route runs a ~9 deg bearing through the opening (the bend at the gap
  marker is ~2 deg) to the next mandatory marker (authored radius 8 m).
  Across 6000 ticks the body's raw per-tick segments **never intersect
  either slab**; the swept arrival fires at 7.97 m from the gap marker —
  just inside the authored radius — and the route completes with
  `stalled_ticks = 0`. What keeps the airframe out is the plan refusing to
  commit a step that crosses a (clearance-grown) blocker — the corridor —
  plus the follower tracking it.
- **Committed dodge (`t761_b`).** A 2 mm wire plate astride the leg is the
  geometry the bounded per-tick authority can still dodge: the direct step
  sweeps it, the yaw-bounded offset exits the band inside one tick. The
  decision comes back `Deviating` and the roll applied that same tick is the
  dodge's sign (`Left -> -1.000`), where the pre-#761 law emitted `0.0`.
  **Honest bound:** the body's raw path still crosses the hair plate — the
  plan sees a blocker one bounded step (~0.33 m at 40 m/s) ahead, and no
  command displaces a 40 m/s airframe out of a blocker's line inside one
  tick. The deviate's measured contribution is the *command* steering the
  committed way; physical keep-out belongs to the corridor check and the
  hold. The test asserts the command and the arrival, not the hair-plate
  miss.
- **Blocked hold (`t761_c`).** A banked spawn (1.0 rad about the forward
  axis) meets an enclosing sphere: `stalled_ticks` grows, throttle is 0 on
  every held tick, roll stays saturated toward level while the body-up `y`
  rises 0.62 -> 0.75 — the hold rolls the aircraft wings-level instead of
  holding its bank, and the route rejoins to reach the next mandatory
  marker.

Mutation check: reverting `command_for` to the pre-#761 law fails the two
cs_sim pins (`roll 0` on both the deviating and blocked legs) and the
`t761_b`/`t761_c` integrated legs, while the corridor leg — which pins
routing, not the command law — stays green, as it should.

## 4. What did not survive contact with the airframe

The kinematic `SyntheticArchProbe` threads its arch fixture; the production
airframe cannot — its measured sustained turn (~0.42 rad/s at cruise) is
below what the arch's tight funnel leg demands, so it overshoots the funnel
marker, goes `Blocked` against a slab and plows through. That is the
envelope/airframe mismatch `docs/findings/2026-10-07-t526-follower-bank-hold.md`
already records, not this task's law. The integrated fixtures above were
sized to the airframe's real authority (~9 deg bends, wide openings,
generous authored radii), which is also why the corridor leg spawns
pre-aligned: the rejoin-from-displacement case belongs to `accept_t526`.
