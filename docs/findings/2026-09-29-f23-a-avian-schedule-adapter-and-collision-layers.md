# F23-A: Avian schedule adapter and collision layers

Date: 2026-09-29. Task: F23-A "Build verified Avian schedule adapter and
collision layers"
(`specs/F23-avian-integration-collision-and-fixed-step-authority.md`, section
`### F23-A`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`. Capabilities
used: ordinary build/test only (no `CS_GAME_DIR` read, no evidence report
required).

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/collision.rs` (new): the dependency-free layer vocabulary
  — `CollisionLayer` (aircraft, projectile, static world, debris, trigger,
  camera), `CollisionLayers` (bitmask), the designed interaction matrix
  (`CollisionLayer::designed_collides_with`), `ShapeClass`, `ContactKind` and
  `classify_contact`, which makes a sensor overlap a distinct classification
  from a solid contact.
- `crates/cs_app/src/physics/adapter.rs` (new): `ForceRequest`/`ForceRequests`
  (one-tick force/torque queue, non-finite input rejected at construction),
  `PhysicsAdapterPlugin`, and `PhysicsTickLedger`. The plugin installs
  `Time<Fixed>` at the declared rate and adds the real pinned systems: drain
  forces in `FixedPostUpdate` before `PhysicsSystems::Prepare`, count the
  integration after `PhysicsSystems::StepSimulation`.
- `crates/cs_app/src/physics/fixture.rs` (new): `PhysicsFixture`, the
  asset-free one-body harness with an explicit `Mass`, zero gravity and
  `SubstepCount(1)`, used by the acceptance tests to drive the production force
  path.
- `crates/cs_app/src/physics/mod.rs`, `crates/cs_sim/src/lib.rs`,
  `crates/cs_app/src/lib.rs` (wiring only): module declarations and docs.
- `crates/cs_app/tests/physics/{main,common,forces,schedule,layers}.rs` (new):
  the `accept_f23_a_*` acceptance tests.
- This file.

**One observable failure:** gameplay submits a force for one tick and the body
never accelerates — or accelerates twice — because the force queue is not
drained into Avian's accumulator before `PhysicsSystems::Prepare` (it would be
too late, after the step) or because the accumulator is not cleared between
ticks. `accept_f23_a_known_force_on_known_mass_gives_one_tick_delta_v` fails
both ways: measured delta-v is `0` (hook removed) or `2 * F/m * dt` (applied
twice), never `F/m * dt`. Verified by removing the apply calls: five force /
torque / ordering tests fail, the layer tests still pass.

## Measured behavior on the pinned pair

`bevy 0.19.1` / `avian3d 0.7.0`, `SubstepCount(1)`:

* one declared tick = one `FixedPostUpdate` = one
  `PhysicsSystems::StepSimulation`; `Time<Fixed>::overstep()` returns to zero
  and `elapsed == ticks * timestep`;
* a request is applied by exactly the tick that submitted it: `F = 12 N` on
  `m = 2 kg` at 120 Hz yields `delta-v = 6 * (1/120) = 0.05 m/s` in one tick
  and no further change on the next tick;
* the pre-integration probe (a system `.before(PhysicsSystems::Prepare)`) still
  sees the untouched start velocity while the tick's end already carries the
  delta-v, so the hook is in the right slot;
* `apply_force` writes Avian's `VelocityIntegrationData`; `pre_process_
  velocity_increments` scales it by the substep delta and the integrator clears
  it after the step, so the net tick delta is `a * dt` independent of substeps.

Schedule hooks are the ones measured in F00-B
(`docs/findings/2026-09-23-pinned-bevy-0.19-avian-0.7-schedule-api.md`). The
fixed clock's first-frame gap fix is reused from
`docs/findings/2026-09-23-t334-first-frame-fixed-step.md`.

## Designed vocabulary, not original data

Every type, layer name, layer bit, matrix pair and the 120 Hz rate are newly
authored project design. The following are **unknown** and are not guessed
here:

- the original game's tick rate and whether its integrator ran before or after
  its gameplay forces (compatibility/convergence stages: **F23-D**, **F16-D**);
- which collision layers the original used, which pairs it let interact and
  whether its triggers dealt damage directly (declared here from the spec's
  non-negotiable behavior 2; measured by **F18-A**, **F23-D**);
- whether the original ever applied a force for more than one tick without an
  explicit resubmission (the contract forbids an undocumented persistent force;
  `ConstantForce` stays a separate, explicit component owned by F23-B);
- Avian's shipping substep count (default 6) — the fixture pins 1 for a literal
  one-integration-per-tick assertion, and the product default is **F23-B/D**'s
  decision, not asserted on original behavior.

## Known limitations that gate later stages (not silently dropped)

Affected content: the whole F23 force path. Resolving tasks: **F23-B**,
**F23-C**, **F23-D**.

1. The queue drops a request whose entity no longer matches Avian's `Forces`
   query. A sleeping body loses its `SolverBody` (and therefore
   `VelocityIntegrationData`), so a force aimed at a sleeping body would be
   dropped instead of waking it. F23-B owns body creation and must decide
   whether to ensure/wake the solver body before applying, and cover it with a
   test.
2. `CollisionLayer` is declared but not yet bound to an Avian collision-groups
   component; no runtime body carries a layer yet. F23-B binds it; the matrix
   is designed engine content until F23-D measures the original.
3. The adapter installs `Time<Fixed>` at its declared rate and does not
   reconcile with a pre-existing clock; the single-clock authority contract is
   F23-C's wiring, exercised here only through the fixture.
4. Continuous detection is declared per layer
   (`CollisionLayer::requires_continuous_detection`) but not enabled on any
   body; thin-wall/tunnel regression tests are F23-B's AC02.

A note with limitations 1 and 2 was attached to F23-B (#84) so the next agent
starts from it.

## Evidence

Synthetic fixtures and pins only. No original-data, visual, audible or
ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F23-avian-integration-collision-and-fixed-step-authority.md`
  (`### F23-A`), `docs/contracts/FLIGHT-PHYSICS.md`.
- `docs/findings/2026-09-23-pinned-bevy-0.19-avian-0.7-schedule-api.md`,
  `docs/findings/2026-09-23-t334-first-frame-fixed-step.md`.
- Pinned sources in the local crate cache: `avian3d-0.7.0`
  (`dynamics/rigid_body/forces/query_data.rs`,
  `dynamics/integrator/mod.rs`, `dynamics/solver/solver_body/`,
  `dynamics/solver/schedule.rs`), `bevy_time-0.19.1`.
