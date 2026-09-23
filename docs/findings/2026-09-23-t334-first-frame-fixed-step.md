# Rally task #334: one physics integration per declared tick (first-frame fixed-step gap)

Date: 2026-09-23. Task: #334 "Make the synthetic scene integrate physics
exactly once per declared tick" (follow-up recorded during F00-B; spec
`specs/F00-workspace-toolchain-and-first-executable.md` AC03, prior
measurement in
`docs/findings/2026-09-23-pinned-bevy-0.19-avian-0.7-schedule-api.md`).
Capabilities used: ordinary build/test only.

## Measured before the change

A `FixedPostUpdate` counter probe (the new `accept_t334_` tests) on the
pinned pair bevy 0.19.1 / avian3d 0.7.0, run against the unmodified scene:

```
step(1)   -> 1 App::update, 0 FixedPostUpdate runs, y unchanged (10.0)
step(600) -> 600 App::update calls, 599 FixedPostUpdate runs, Tick(600)
```

So `step(n)` integrated `n - 1` times while `Tick` reached `n`, matching the
earlier probe exactly.

## Cause (pinned source, not a guess)

`TimeUpdateStrategy::ManualDuration` → `Time<Real>::update_with_duration`
(`bevy_time-0.19.1/src/real.rs:88-91`) → `update_with_instant`
(`real.rs:99-109`), which returns early while `context().last_update` is
`None`: the first call records `first_update`/`last_update` and produces no
delta. `run_fixed_main_schedule` (`fixed.rs:243-258`) then accumulates a
zero `Time<Virtual>` delta, `expend()` fails, and frame 1 runs no
`FixedMain`/`FixedPostUpdate`/physics.

## Decision

**Seed the real clock's baseline instant at scene build time**, in
`SyntheticSceneBuilder::build` (`crates/cs_app/src/synthetic.rs`):

```rust
let startup = app.world().resource::<Time<Real>>().startup();
app.world_mut()
    .resource_mut::<Time<Real>>()
    .update_with_instant(startup);
```

`update_with_instant` is a public method documented as "provided for use in
tests" — the synthetic scene is exactly that harness. Seeding `last_update`
to `startup` means the first `update_with_duration(frame)` computes
`instant = startup + frame` and reports `delta = frame`, so frame 1 already
accumulates one full timestep and `expend()` succeeds once. It also records
`first_update = startup`, which is coherent for a manually driven clock:
the scene declares its clock to have started when it was built.

### Alternatives considered and rejected

* **`TimeUpdateStrategy::FixedTimesteps(1)`** ("an `App::update` will always
  run the fixed loop exactly n times", `bevy_time-0.19.1/src/lib.rs:120-122`):
  it still funnels through `update_with_duration` → `update_with_instant`
  (`lib.rs:181-183`), so the first call hits the same `last_update == None`
  early return and frame 1 stays step-free. Does not fix the gap.
* **Seeding `Time<Fixed>::accumulate_overstep(timestep)`** (public test API,
  `fixed.rs:189-191`): frame 1 would expend the seeded overstep and run one
  step. But the virtual clock would still report zero delta on frame 1, so
  `Time<Fixed>::elapsed()` would sit one timestep ahead of
  `Time<Virtual>::elapsed()` for the scene's entire life — violating the
  invariant documented at `fixed.rs:53-55` (fixed elapsed stays between the
  previous and current virtual elapsed). The baseline seed keeps all three
  clocks at `elapsed == ticks * timestep` after every step.
* **A warm-up `app.update()` during `build`**: burns the zero-delta frame
  but also runs `Update`/`PostUpdate` once outside the tick budget — probe
  systems configured through `SyntheticSceneBuilder::configure` would see an
  extra frame (the F00-B ordering test counts exactly `frames` `Update`
  samples), and the run's first recorded state would no longer be the spawn
  state. More machinery for a worse result.
* **Running `FixedMain` manually after `app.update()` when no step ran**:
  duplicates `run_fixed_main_schedule`'s accumulator/`Time`-swap bookkeeping
  in project code and would fight the very mechanism it patches.

## Measured after the change

Same probes, same pinned pair:

```
step(1)   -> 1 integration, y already below spawn, overstep 0ns,
             Time<Fixed>::elapsed() == 1 * timestep
step(600) -> 600 integrations, Tick(600), overstep 0ns,
             Time<Fixed>::elapsed() == 600 * timestep
twice in a row on two fresh scenes: identical results
```

`cargo test -p cs_app --test accept_t334_fixed_tick_integrations` passes;
the F00-A/F00-B/F00-C acceptance tests are unchanged and still pass.

## Files

- `crates/cs_app/src/synthetic.rs`: baseline seed in
  `SyntheticSceneBuilder::build`, module doc updated.
- `crates/cs_app/tests/accept_t334_fixed_tick_integrations.rs`: task tests
  under the `accept_t334_` prefix.

## Sources

- `bevy_time-0.19.1` sources: `src/real.rs`, `src/fixed.rs`, `src/lib.rs`
  (local crate cache, pinned per `Cargo.lock`).
- `docs/findings/2026-09-23-pinned-bevy-0.19-avian-0.7-schedule-api.md`
  (prior probe and the recorded quirk this task resolves).
