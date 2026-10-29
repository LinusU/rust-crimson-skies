# Ground-blow probe for every flying aircraft: M01 tick budget and smoke (#2226)

Follow-up to #2224 (probe cast per record) with the law consumer of #2107.
Measured on the owner's laptop (Apple M3 Pro), debug build, retail M01 composed
by `compose()` in `crates/cs_app/tests/campaign/`. Measurement, not a budget.

## Trajectory

`accept_groundblow_ai_withholding_the_probe_changes_the_ai_trajectory` runs two
M01s in lockstep with the AI aircraft on the same sea dive; one run withholds
only the AI's probe between fill and driver. The AI body's pose/velocity
diverges, so the AI term of the ground blow now fires from the filled probe.

## Tick budget

`accept_groundblow_ai_measure_m01_tick_budget`: 30 warm-up then 600
`App::update` calls, wall time per call (whole app: physics, flight, mission,
scripts, the all-seat probe), microseconds:

| p50 | p95 | p99 | max |
| --- | --- | --- | --- |
| 19420 | 20037 | 21251 | 22332 |

The fill cast 2 probes per tick (player + one AI record; `cast_per_tick=2`).
This is the **whole update in a debug build**; the fill system's own share was
not isolated, so no claim is made about its cost beyond "one SpatialQuery per
record per tick". Release timings and a per-system split are unmeasured; the
F60 tick budget (p99 <= 8333 us) is unevaluated against this number.

## Smoke

`cs --cs-path <retail> --mission M01 --smoke-seconds 45 --capture-dir
private/smoke2226` exited 0: `mission smoke: 4128 frames, 8082 ticks, 81
checks, failures: none` (81/0). The minimum `--smoke-seconds` is 45. Ten
framebuffer PNGs were written to the ignored `private/smoke2226/`.
