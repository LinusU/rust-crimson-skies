# F23-D: stability, high-speed contact and convergence evidence

Date: 2026-09-30. Task: F23-D "Validate stability, high-speed contact and
convergence evidence"
(`specs/F23-avian-integration-collision-and-fixed-step-authority.md`, section
`### F23-D`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`. Capabilities
used: ordinary build/test only (no `CS_GAME_DIR` read, no evidence report
required).

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/physics/evidence.rs` (new): the production probe harness —
  [`ConvergenceScenario`]/`convergence_probe`/`convergence_evidence` (AC04),
  [`ContactScenario`]/`contact_probe`/`contact_sweep`, and
  [`StabilityScenario`]/`stability_probe`, plus the frozen
  `FROZEN_CONVERGENCE_BUDGETS`, `FROZEN_STABILITY_BUDGET`,
  `CONTACT_FACE_TOLERANCE_M` and `SENSOR_MIN_SPEED_FRACTION`, and the named
  violation enums every probe returns instead of a boolean.
- `crates/cs_app/src/physics/adapter.rs`: `DECLARED_SUBSTEP_COUNT` and the
  plugin's substep policy (F23-A's finding left "the product default is
  F23-B/D's decision").
- `crates/cs_app/src/physics/preflight.rs`: the swept spawn response — the
  clamp also removes the into-obstacle velocity, keeps a declared 1 mm
  overlap, and a second non-blocking cast records a sensor crossing.
- `crates/cs_app/src/physics/session.rs`: the builder gained the `configure`
  seam (the same one `PhysicsFixtureBuilder` has) so the stability probe can
  host the F24-B flight driver in the production session; `restart` replays it.
- `crates/cs_app/src/physics/fixture.rs` (wiring only): the harness now pins
  one substep through the plugin instead of overwriting the resource.
- `crates/cs_app/src/physics/mod.rs` (wiring only): module declaration and
  re-exports.
- `crates/cs_app/tests/physics/{main,evidence}.rs`: the thirteen
  `accept_f23_d_*` acceptance tests.
- This file.

**One observable failure:** a projectile fired at 300 m/s passes through a
2 cm wall at 120 Hz and the game still sees a "hit" report for a collision
that did not happen — `accept_f23_d_a_single_solver_step_tunnels_where_the_
declared_policy_does_not` fails with the projectile at `x = +0.218` (the wall
spans `-0.01..+0.01`). The F23-B tests never saw it because they probed one
speed (120 m/s = 1 m per tick) at one spawn distance.

## Two real regressions found, and their repairs

Both were found by the probes in this file, and both are production
behaviour, not test artefacts.

### 1. One solver step per tick ejects a fast body out the far side

Avian's swept CCD stops the body at the time of impact correctly — measured
`x = -0.0596` for a 2 cm wall and a 10 cm projectile — but the *next* tick's
narrow phase then generates a contact on a body that is arriving at 300 m/s.
The solver cannot arrest that approach in one step, the body penetrates
deeper than the obstacle is thick, and the penetration correction picks the
*shortest* way out — which, for a body embedded in a 2 cm wall, is forward.
The projectile is ejected at a few m/s and flies on.

Measured with the pre-F23-D schedule (`SubstepCount(1)`), 120 Hz, wall at
`x = 0` with 2 cm half-thickness, 10 cm projectile (`max_x`, the near face is
`+0.01` and the projectile is fully through at `+0.06`):

| rate | speed | travel/tick | `max_x` at 1 substep | at 2 | at 4 | at 8 |
| --- | --- | --- | --- | --- | --- | --- |
| 60 Hz | 120 m/s | 2.00 m | **0.229 (through)** | -0.060 | -0.060 | -0.060 |
| 60 Hz | 300 m/s | 5.00 m | **0.154 (through)** | -0.059 | -0.059 | -0.059 |
| 60 Hz | 600 m/s | 10.00 m | **0.368 (through)** | -0.059 | -0.059 | -0.059 |
| 120 Hz | 120 m/s | 1.00 m | -0.017 | -0.060 | -0.060 | -0.060 |
| 120 Hz | 300 m/s | 2.50 m | **0.218 (through)** | -0.060 | -0.060 | -0.060 |
| 120 Hz | 600 m/s | 5.00 m | **0.154 (through)** | -0.059 | -0.059 | -0.059 |
| 240 Hz | 300 m/s | 1.25 m | -0.006 | -0.060 | -0.060 | -0.060 |
| 240 Hz | 600 m/s | 2.50 m | **0.218 (through)** | -0.060 | -0.060 | -0.060 |

The failure threshold is between 1.0 m and 1.25 m of travel per tick. Every
crossing reported exactly one contact episode at every substep count, so the
reporter was never the problem — the geometry was.

**Repair:** the product schedule installs `DECLARED_SUBSTEP_COUNT = 2`
substeps per fixed tick, measured sufficient for the whole envelope probed
(60/120/240 Hz × 60–1200 m/s × obstacles down to 4 mm thick × 2 cm
projectiles: no tunneling anywhere, `max_x` = the contact position to four
decimals, exactly one episode per crossing). Four and eight substeps measured
the same to four decimals, so the value is not a knife edge; it is revisited
if the swept speed envelope grows. Substeps are internal to
`PhysicsSystems::StepSimulation`, so `ticks == integrations` still holds and
the F23-A "one integration per tick" accounting is unchanged.
`PhysicsFixture` keeps **one** substep (`with_substeps(1)`) so the F23-A/B
harness assertions stay literal, and the difference is stated in both places.

Alternatives measured and rejected, with numbers: a bounded
`SpeculativeMargin` of 0.5 m and 2.0 m still tunneled at 300 m/s/120 Hz
(`max_x = 0.242`), and a 5 m margin stopped the body 5.8 cm in front of the
wall face — exactly the "globally inflated hitbox" the spec's non-negotiable
behavior 3 forbids. `SpeculativeMargin::ZERO` therefore stays.

### 2. A clamped spawn is still carried through the obstacle

F23-C clamped the spawn *position* onto the contact and left
`LinearVelocity` alone. The clamp lands inside the tick that is about to run,
and that tick carries the body the rest of the tick's travel — past the
obstacle, because a body spawned this tick is still not in the broad phase
(F23-B limitation 1). Measured at 120 Hz with a 10 cm projectile 0.4 ticks
of travel short of a 2 cm wall, one substep *and* two: 60 m/s ended tick 1 at
`x = +0.44` and sailed to `+3.94`; 120 m/s to `+7.94`; 300 m/s to `+19.94`;
600 m/s to `+39.94`. **No contact report at all** in most of them — a bullet
that silently passes through an aircraft. The engine's swept CCD and the
substep policy cannot help: the pair is not in collision detection until the
tick after the spawn.

**Repair** (in `preflight.rs`), two parts, each measured on its own:

* **A declared overlap on the clamp** — `SPAWN_CONTACT_OVERLAP_M = 0.001`, a
  tenth of Avian's own 1 mm contact tolerance. This is the load-bearing part.
  The swept layers run with `SpeculativeMargin::ZERO`, so a body stopped
  exactly at the time of impact only *touches* the obstacle and generates no
  contact at all: measured, with the clamp but no overlap, **9 of the 12**
  probed rate/speed combinations sailed straight through the wall (`x = +2.94`
  to `+29.94`, at full speed) with **zero** contact reports — a bullet that
  silently passes through an aircraft. With the overlap, all 12 report exactly
  once on the spawn tick and stop at `x = -0.0593`. The deferred `Position`
  write does reach the broad phase in time: the pair *is* found on the spawn
  tick, which is why the overlap is enough.
* **The removal of the velocity component pointing into the obstacle** (the
  tangential component is untouched, so a graze keeps sliding). This is not
  what stops the projectile — the engine's solver does that, from the overlap
  above. It is what happens *next*: measured, with it the projectile stays at
  rest on the contact (`|v| = 0.001 m/s` after twelve ticks); without it the
  contact pushes it back off the wall at 1.1–16.7 m/s depending on the impact
  speed. The write is deferred into the sync point ahead of
  `PhysicsSystems::Prepare` like the clamp, and takes hold from the following
  tick; the event field [`SpawnPreflightEvent::stopped`] says exactly what it
  does — "the velocity component carrying the body into the obstacle was
  removed with the clamp" — and not more.

A 1 mm overlap is a declared, bounded quantity smaller than the thinnest
obstacle the sweep covers, not a hitbox inflation: it exists so that a
*stopped* body produces a contact, and it never makes an approach stop early
(the 0.5 m and 2.0 m speculative-margin experiments above are the inflation
this avoids).

## AC04: the 60/120/240 Hz convergence probes, before the tolerances

`ConvergenceScenario::constant_thrust` — 20 kg under 400 N (20 m/s²) for
2 s from 10 m/s, one force request per fixed tick through the production
queue. The closed-form answer is known, so the probe measures the integrator
and the schedule rather than a flight model. Avian's semi-implicit (symplectic)
Euler step is exact in velocity under a constant force and carries a
first-order position error of `0.5 · a · T · dt / substeps`.

Measured, with the declared two substeps:

| rate | ticks | position error | velocity error | symplectic residual | applied requests | dropped |
| --- | --- | --- | --- | --- | --- | --- |
| 60 Hz | 120 | 0.166668 m | 8.77e-5 m/s | 1.3e-6 m | 120 | 0 |
| 120 Hz | 240 | 0.083336 m | 1.76e-4 m/s | 2.5e-6 m | 240 | 0 |
| 240 Hz | 480 | 0.041668 m | 3.51e-4 m/s | 1.3e-6 m | 480 | 0 |

* The position error halves exactly when the rate doubles: observed order
  1.000 at both steps. First-order convergence, confirmed by measurement and
  not by the integrator's name.
* The symplectic residual is a micrometre, so the position error *is* the
  first-order term and not a bug hiding behind a small number.
* The velocity error is f32 accumulation noise that grows slowly with the step
  count; it is three orders of magnitude below the force/mass the force path
  would have to get wrong to produce it.
* The same runs under the pre-F23-D single solver step measured
  0.333328 / 0.166664 / 0.083321 m — exactly double, because the first-order
  term scales with `dt / substeps`. The frozen numbers are therefore only
  valid for the declared substep count, and the table says so.

**Frozen after measuring** (`FROZEN_CONVERGENCE_BUDGETS`, ~8% headroom on
position, 3× on velocity): 60 Hz `≤ 0.18 m`, 120 Hz `≤ 0.09 m`, 240 Hz
`≤ 0.045 m`, every rate `≤ 1e-3 m/s`, and a minimum observed order of `0.9` for
the two coarser rates. The acceptance test also requires the order to be
within `0.95..=1.05`, so a table that merely got *smaller* could not pass.

**The baseline rate stays 120 Hz.** Its measured position error over a 2 s
window (8.3 cm) is half of 60 Hz's and double of 240 Hz's; 240 Hz buys 4 cm
for twice the solver work per tick and 1200 instead of 600 ticks of gameplay
logic per simulated second. That is a design decision recorded with its
numbers — not a fidelity claim about the original, whose tick rate is unknown.

## Stability: ten seconds of production flight at every probed rate

`StabilityScenario::level_cruise` — the F24-A synthetic fixed wing through
`spawn_flight_body`, the real `FlightForcesPlugin` in the production session,
throttle 0.7 held, wings level, from 1000 m at 120 m/s, for 10 simulated
seconds:

| rate | ticks | peak speed | peak body rate | altitude band | driver ticks / driven / parked / refused | requests applied / dropped |
| --- | --- | --- | --- | --- | --- | --- |
| 60 Hz | 600 | 123.3994 m/s | 0.0000 rad/s | 1000.003–1023.408 m | 600 / 600 / 0 / 0 | 600 / 0 |
| 120 Hz | 1200 | 123.3972 m/s | 0.0000 rad/s | 1000.001–1023.393 m | 1200 / 1200 / 0 / 0 | 1200 / 0 |
| 240 Hz | 2400 | 123.3959 m/s | 0.0000 rad/s | 1000.000–1023.385 m | 2400 / 2400 / 0 / 0 | 2400 / 0 |

No non-finite state on any tick, `ticks == integrations` everywhere, no
dropped request, no skipped or refused driver tick, and a body rate of exactly
zero (the held command is neutral, and the F24-A controller is rate-commanded,
so a wings-level cruise must not rotate). Frozen
(`FROZEN_STABILITY_BUDGET`): peak speed `≤ 130 m/s`, peak body rate
`≤ 0.5 rad/s`, altitude band `900–1100 m`, and a **rate spread of the peak
speed `≤ 0.05 m/s`**. That last one is the flight path's own convergence
statement: the three rates agree to `0.0035 m/s` — 3e-5 relative — across a
four-fold change of rate, which is what a rate-independent force path looks
like from the outside. A force applied once per *frame* instead of once per
tick, a double-applied force, or a substep count leaking into the velocity
integration would all move that number by orders of magnitude more.

## The high-speed contact sweep

`contact_sweep()` runs 36 crossings: 3 rates × 4 speeds × (two spawn
distances for a solid obstacle, one for a sensor). Every solid crossing is
reported **exactly once**, classified `SolidContact`, with `unclassified`,
`ignored` and `suppressed` all zero; every sensor crossing is reported
exactly once as `SensorOverlap` and leaves at its fired speed (the frozen
rule allows a thousandth of a percent); the deepest penetration of a solid
obstacle across the whole matrix is **0.000 m** — every projectile stops on
the near face, at `x = -0.0593` for the declared 2 cm wall and 10 cm
projectile. Speeds run 60–600 m/s, i.e. 0.25–10 m of travel per tick against
a 2 cm obstacle: the regime the contract requires a synthetic test to reach
("choose `speed * dt` larger than the obstacle thickness").

Removal/mutation checks, each applied, run, and reverted on this branch. The
first seven rows are the implementer's, the last four the reviewer's (the
reviewer re-ran the first two, the substep count and the overlap, and got the
same failures):

| Mutation | Failing `accept_f23_d_*` tests (of 13) |
| --- | --- |
| `DECLARED_SUBSTEP_COUNT` back to `1` | 4: the convergence tolerances (position error 0.3333 / 0.1667 / 0.0833 m against the frozen 0.18 / 0.09 / 0.045 m), the baseline budget, the single-step comparison, and the contact sweep (8 tunneling crossings) |
| `SPAWN_CONTACT_OVERLAP_M` set to `0.0` | 3: the spawn-in-the-hole test, the contact sweep — 9 of 12 in-hole spawns pass through the wall with no report — and the reviewer's sensor-before-wall test, which rests on the same declared overlap |
| the preflight's into-obstacle velocity removal disabled | 1: the spawn-in-the-hole test (the projectile rebounds off the wall at 1–17 m/s instead of staying on the contact) |
| the preflight's second (sensor) cast made to reject everything | 1: the trigger-inside-the-spawn-tick test — the crossing leaves no trace anywhere |
| `apply_force_requests` removed from the adapter chain | 3: both convergence tests and the flight stability test (`driven` = 600 with `applied` = 0) |
| `drive_flight_aircraft` unregistered | 1: the flight stability test (the driver's own tick counter reads 0 of the world's 600) |
| `record_integration` no longer increments the ledger | 2: the flight stability test and the contact sweep (`ticks != integrations`) |
| the preflight's sensor cast made conditional again (skipped when a solid obstacle stops the same tick) | 1: the reviewer's `..._a_spawn_records_a_sensor_it_crossed_before_a_solid_stops_it` — a body that crossed a trigger and then hit a wall in the same tick left no trace |
| the convergence probe judging the run against the requested `duration_s` instead of the ticks it simulated | 1: the reviewer's `..._a_fractional_tick_duration_is_judged_on_the_time_it_simulated` — 0.59 m reported where 0.88 m is the measured first-order term |
| `rate_spread_m_s` reading `probes.first()`/`probes.last()` instead of the extremes | 0 by construction (its one caller happens to pass ascending rates): fixed because the measurement was order-dependent, and a probe that measures the wrong pair silently is worse than no probe |
| `deepest_x_m` seeded with `f64::MIN` instead of the spawn position | 0 by construction (a body that never reports a pose panics at the end of the run instead): fixed because a seed that reports "no penetration" when nothing was measured is the wrong direction to fail |

Two of the implementer's rows are why the probes are shaped the way they are.
The stability rules compare the driver's own counters with the adapter's ledger
(`DriverAbsent`, `RequestsNotApplied`) rather than trusting a single counter:
with the driver unregistered, both of *its* counters read zero, which looks
exactly like a run in which nothing needed flying. And the substep policy is
checked from both sides — the product probe must not tunnel, and the
single-step probe must — so weakening the declared count cannot make both
halves pass at once.

## What the review changed

Recorded because a reviewer's fixes are evidence too. Four defects were found
in the branch as submitted and fixed on it, all inside the owner paths:

1. **A probe that could be charged with its own rounding.**
   `convergence_probe` and `stability_probe` computed their tick count as
   `fixed_hz * round(duration_s)` and then compared the run against
   `duration_s` itself. For every *declared* scenario (2 s, 10 s) those are the
   same instant, so the frozen numbers were never wrong; for any other duration
   the run simulated a different number of seconds than the one it was judged
   at. At 10.51 s and 60 Hz the run simulates 631 ticks = 10.5167 s, and
   charging the 6.7 ms to the integrator makes the reported position error the
   difference of two first-order terms (0.88 m − 1.47 m = 0.59 m) instead of the
   0.88 m term itself — a 32% error in the number that is supposed to measure
   the integrator. The run now compares against the time it actually simulated
   (`ConvergenceProbe::simulated_seconds`), which is a public field so the
   difference is visible, and the rounding happens in exactly one place
   (`tick_count`).
2. **An unbounded run length.** The same arithmetic cast to `u32` and
   saturated, so a scenario asking for 10^9 seconds asked for 4.3 × 10^9 ticks
   and hung instead of refusing. `tick_count` now refuses a run longer than
   [`MAX_PROBE_TICKS`] by name.
3. **A sensor crossing recorded only sometimes.** The preflight's second,
   non-blocking cast ran only when no solid obstacle stopped the spawn, so a
   projectile that crossed a trigger and was then stopped by a wall in the same
   tick recorded nothing — even though the crossing is the gameplay fact the
   field exists for. The cast now runs either way, from the same spawn
   position, so `passed_distance_m` and the clamp's `distance_m` share one
   frame of reference. This is the one production behaviour the review changed,
   and `..._a_spawn_records_a_sensor_it_crossed_before_a_solid_stops_it` pins
   it at 120 Hz with a trigger at `x = -0.10` and a wall at `x = +0.10`.
4. **Two order-dependent measurements.** `rate_spread_m_s` compared the first
   and last entries of the slice it was handed rather than the extremes, and
   the contact probe seeded its penetration sampling with `f64::MIN` instead of
   the spawn position. Neither could fail in the current callers, and both are
   now measuring what their names say.

The review also added a second, independent way to catch a tunnelling
crossing: `ContactViolation::SolidEscaped` compares the *end state* against the
obstacle's far face, where `Tunnelled` compares the deepest penetration the
per-tick sampling ever saw. The substep-count mutation above fails both, which
is the point — a sampler that misses the deepest moment of a pass can no longer
hide it.

Every table above was re-measured by the review on the rebased branch and
reproduced exactly: position errors `0.166668 / 0.083336 / 0.041668 m`, velocity
errors `8.774e-5 / 1.755e-4 / 3.510e-4 m/s`, cruise peaks `123.3994 / 123.3972 /
123.3959 m/s` with the altitude bands as tabulated, and the spawn-tick trigger
row (0 episodes everywhere except 240 Hz at 60 m/s, which reports once on tick
2) with recorded distances from 0.04 m to 3.94 m. The numbers in this file are
therefore measurements, not a transcription.

## Designed values, not original data

Every number above is a measurement of **this** project on the pinned pair
(`bevy 0.19.1` / `avian3d 0.7.0`, `SubstepCount` 2 for the product session),
with synthetic masses, extents, positions and speeds. Specifically:

- the 120 Hz baseline, the substep count, the frozen tolerances and the sweep
  matrix are project design validated by measurement;
- `PROBE_SPEEDS_M_S` tops out at 600 m/s as a *declared* bound for the swept
  layers. The original game's projectile speed, tick rate, substepping,
  collision layers, hitboxes and damage rules are **unknown** and are not
  guessed here;
- the F24-A synthetic airframe is not a Crimson Skies airframe. A converged,
  stable ten-second cruise proves the integrator and the schedule, never
  faithful handling.

No original-data, visual, audible or ordinary-play claim: this stage can award
at most **checked**.

## Known limitations that gate later stages

Affected content: the swept layers' spawn path and the trigger rules.
Resolving tasks: **F24-C** (equipment/trigger consumer), **F26**
(calibration), **#415** (the trigger-crossing decision this stage declined to
invent), **#416** (the substep policy in every world bootstrap), and — *resolved
by task #401 on 2026-10-02* — the swept-CCD/sensor **hold**, which was the other
half of that pair: a `WorldCollisionRole::Sensor` object is now spawned on an
entity with no rigid body, so a swept body is no longer *held* at a sensor's face
(`docs/findings/2026-10-02-t401-trigger-volume-and-swept-ccd.md`). Limitation 1
below is unchanged and is **not** fixed by that: a crossing that no discrete
sample sees is still unreported during ordinary flight either, which is the
sibling task **#498** (`F18-trigger-swept-crossing`) filed by #401.

1. **A trigger crossed entirely inside the spawn tick is not reported.** F23-C's
   acceptance criterion `accept_f23_c_preflight_never_stops_on_a_sensor`
   forbids a sensor from stopping or delaying a spawn, and a body that keeps
   its velocity leaves a 2 cm trigger inside the very tick it is invisible to
   the broad phase. Measured (spawn 0.4 ticks of travel short of a 2 cm
   trigger): 0 episodes at 60 Hz and at 120 Hz for all four speeds, and at
   240 Hz for 120/300/600 m/s; at 240 Hz and 60 m/s a tick is only 0.25 m of
   travel, so the body lands *inside* the trigger and the next tick reports it
   (1 episode, first report on tick 2). The crossing is **recorded** either
   way: the preflight's second, non-blocking cast puts the sensor in
   `SpawnPreflightEvent::passed` with the distance to it (measured 0.04 m to
   3.94 m across the matrix) and `SpawnPreflightLog::passed` counts them. That
   cast runs whether or not a solid obstacle also stopped the same tick, since a
   body that crossed a trigger and then hit a wall crossed the trigger. Whether
   gameplay consumes that field as a trigger crossing is a rule this stage does
   not invent; it is **#415** to decide, with **F24-C** as the consumer and
   **F26** for the calibrated values.
2. **The substep policy is a measured value, not a derived one.** It is
   sufficient for the probed envelope at the declared probe geometry; the
   threshold is a function of obstacle thickness and body size, so content
   thinner than 4 mm or faster than 1200 m/s is outside what was measured.
   Re-derive rather than widen if the envelope grows.
3. **The frozen convergence budgets belong to the declared scenario.** They
   bound a constant-force integration over 2 s at 20 m/s², not a flight
   envelope, and they are only valid while the substep count and the baseline
   rate stay as declared.
4. **No original reference exists for any of this.** The original tick rate,
   force ordering, projectile speed, substep count and contact semantics stay
   unknown (F16-D, F24-D, F26, and the owner-supplied
   `REF-OWNER-FIRST-CAPTURE`).
5. **The declared substep policy is installed by the physics schedule, not by
   every world bootstrap.** `PhysicsAdapterPlugin` inserts
   `SubstepCount(DECLARED_SUBSTEP_COUNT)` and both the product session and the
   physics harness go through it — but `crates/cs_app/src/world/fixture.rs:691`
   (F18-A, outside this task's owner paths) calls the same plugin and then
   overwrites the resource with `SubstepCount(1)`, which is a declared
   measurement condition of the F18-A synthetic arch fixture. Any mission world
   runtime built on that fixture would silently run one substep per tick and
   reproduce regression 1 above. Affected content: the mission world runtime
   once it exists. Resolving task: **#416**; the fixture's own F18-A findings
   are the other place that value is declared.

## Sources

- `specs/F23-avian-integration-collision-and-fixed-step-authority.md`
  (`### F23-D`, AC01–AC04), `docs/contracts/FLIGHT-PHYSICS.md` ("Collision and
  ballistic tests", "Calibration acceptance").
- `docs/findings/2026-09-29-f23-a-avian-schedule-adapter-and-collision-layers.md`
  (limitation 4: "the product default is F23-B/D's decision" — this stage makes
  it; limitation 1: the first-tick hole),
  `docs/findings/2026-09-30-f23-b-body-creation-forces-sweeps-and-transitions.md`
  (the swept AABB is written at the end of a tick; the speculative-margin
  measurement),
  `docs/findings/2026-09-30-f23-c-authoritative-events-render-interpolation-and-session.md`
  (the preflight's clamp-only response, repaired here),
  `docs/findings/2026-09-30-f24-b-fixed-wing-forces-and-bounded-arcade-controller.md`
  (the flight driver and the synthetic airframe this probe flies),
  `docs/findings/2026-09-23-pinned-bevy-0.19-avian-0.7-schedule-api.md`.
- Pinned sources in the local crate cache: `avian3d-0.7.0`
  (`dynamics/ccd/mod.rs` — `solve_swept_ccd` sweeps from the previous pose and
  only accepts `0 < toi < dt`, with a fallback ball cast at
  `default_speculative_margin` when the shapes already touch;
  `collider_tree/update.rs` — the swept AABB is written after the step and
  `has_swept_ccd` forces it to `Scalar::MAX`; `dynamics/solver/`,
  `dynamics/integrator/`).
