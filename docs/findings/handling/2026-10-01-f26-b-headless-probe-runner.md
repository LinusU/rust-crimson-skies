# F26-B: headless maneuver probe runner

Task F26-B (#102). Code: `crates/cs_sim/src/probes/runner.rs`; tests:
`crates/cs_sim/tests/accept_f26_b_headless_probes.rs`.

## What exists

`ProbeRunner` flies one `EnvelopeEntry`'s recorded input schedule through the
production `FlightModel` at a fixed tick (default 1/120 s) and extracts the
quantity the entry bounds. `run_envelope` returns a `ProbeTrace` for
`compare`, the per-maneuver `ProbeRun`s (value, ticks, boost consumed, FNV-1a
`trace_hash`) and the maneuvers that could not be measured.

## Limits (not original behavior)

- The integrator is a **measurement integrator**: semi-implicit Euler, point
  mass, diagonal inertia, no gyroscopic coupling, no ground or collision. It is
  not the physics pose owner and says nothing about the original's integration.
- **Probe horizons are authored** (`horizon_s`): the F26-A `EnvelopeEntry`
  records no duration, so a real original trace cannot state how long it ran.
  Needed before F26-D: a duration (and the measuring window for turn, climb)
  on the envelope entry. `specs/` and the F26-A schema are outside this task's
  owner paths; raise it with the owner.
- **Quantity definitions are authored**: climb = final vertical speed, turn =
  horizontal speed over heading rate in the last third, roll/yaw = peak body
  rate, stall recovery = time from first |AoA| >= stall angle to the first
  later tick inside it, damage = peak body-rate norm relative to a pristine run,
  boost/acceleration/dive = speed gained. None is verified against an original
  capture; F26-D must confirm or replace each.
- Boost capacity is not tracked; `boost_available` stays at its initial value
  and accepted consumption is only summed.

## Determinism

Same build and platform and same inputs give bit-identical `trace_hash`
(tested). Cross-platform agreement is by envelope tolerance only.

## Observations on the F26-A synthetic fixture

Flown on the synthetic airframe, the authored fixture references are not met
(e.g. acceleration gain ~83 m/s vs 25 m/s), which is expected: they were
authored, not fitted. One fixture entry is not measurable and is reported as
unavailable, never passed: `stall_recovery` (the airframe does not recover
within the horizon). The fixture's `roll` and `damage` entries also pass
arguments to `FlightInput::try_new(pitch, roll, yaw, ...)` that do not match
their names (roll flies pitch 1.0, damage flies roll 0.5), so the damage metric
uses the peak body-rate norm rather than one axis. This is a fixture wording
issue, not a runner defect.
