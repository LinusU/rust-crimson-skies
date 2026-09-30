# F23-B: body creation, forces, sweeps and transitions

Date: 2026-09-30. Task: F23-B "Implement body creation, forces, sweeps and
transitions"
(`specs/F23-avian-integration-collision-and-fixed-step-authority.md`, section
`### F23-B`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`. Capabilities
used: ordinary build/test only (no `CS_GAME_DIR` read, no evidence report
required).

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/physics/body.rs` (new): `BodySpec`/`BodyError` (validated
  at the boundary), `BodyMode`, `BodyLayer`, `spawn_body` — the single entry
  point that binds `cs_sim::collision` to Avian (`CollisionLayers`
  membership/filter from the designed matrix, `ShapeClass::Sensor`, the event
  flag, `SweptCcd` + `SpeculativeMargin::ZERO` on the layers that require
  continuous detection, an explicit `Mass` for dynamic bodies) — and
  `set_body_mode`, the pose/velocity-preserving control-mode transition.
- `crates/cs_app/src/physics/contacts.rs` (new): `ContactReport`,
  `ContactReports`, `record_contact_reports` and `PhysicsBodiesPlugin` —
  Avian's collision events classified back into `ContactKind`, one report per
  contact episode.
- `crates/cs_app/src/physics/adapter.rs` (F23-A limitation 1): the queue now
  wakes a sleeping target before it drains, and counts requests that reach no
  dynamic body as dropped (`PhysicsTickLedger::{woken,dropped}_requests`).
- `crates/cs_app/src/physics/{mod,fixture}.rs` (wiring only): module
  declarations, re-exports, and the fixture adds `PhysicsBodiesPlugin`.
- `crates/cs_sim/src/collision.rs`: `CollisionLayer::emits_contact_reports`
  and `CollisionLayer::designed_partners` (the matrix row as a set).
- `crates/cs_app/tests/physics/{main,common,bodies,sweeps,wake}.rs`: the
  `accept_f23_b_*` acceptance tests. (The review pass added
  `crates/cs_app/tests/physics/reports.rs` — see below.)
- This file.

**One observable failure:** a projectile crosses a 2 cm wall at 1 m per tick
(120 Hz) and the game never sees the hit — no contact report at all — because
no body is bound to swept detection and nothing turns Avian's raw event into a
classified report. `accept_f23_b_fast_crossing_of_a_thin_wall_is_detected_exactly_once`
fails exactly that way (`total = 0`, `max_x = 30.63`: the body sailed past).

Removal checks were run against this branch, each then reverted:

| Removed | Result |
| --- | --- |
| `SweptCcd` binding in `spawn_body` | 3 tests fail: both AC02 crossings (`total = 0`, `max_x = 30.63` — tunnelling at every tested speed) and the binding test |
| `wake_requested_bodies` from the adapter chain | `accept_f23_b_force_wakes_a_sleeping_body_in_the_same_tick` fails with `[0.0, 0.0, 0.0]` instead of `F/m*dt` |
| `record_contact_reports` registration | both AC02 tests fail with `total = 0` while the physics still stops the body (`max_x = -0.017`) |
| event flag inserted *after* the collider (found during development, see below) | both AC02 tests fail with `total = 0`, `unclassified = 0` — no event is ever written |

## Measured behavior on the pinned pair

`bevy 0.19.1` / `avian3d 0.7.0`, `SubstepCount(1)`, 120 Hz, zero gravity.
All numbers below come from throwaway probes on this branch, deleted before
hand-over; the surviving tests assert the outcomes.

* **Fast crossing needs the sweep.** With the production binding
  (`SweptCcd` + `SpeculativeMargin::ZERO` on projectile/trigger) a 2 cm static
  wall and a 2 cm static trigger were each crossed at 30, 60, 120, 300 and
  600 m/s (0.25–5 m per tick) and every crossing produced **exactly one**
  `CollisionStart`; the wall stopped the body (never past the wall face), the
  trigger let it through. Removing `SweptCcd` (and keeping the bounded
  margin) produced **zero** detections and clean tunnelling at all five
  speeds — the endpoint-only implementation the contract asks us to make fail.
* **The speculative margin is the hitbox inflation the spec forbids.**
  Avian's default `NarrowPhaseConfig::default_speculative_margin` is
  `Scalar::MAX`: the broad phase sweeps a body's AABB over one frame of its
  *velocity* and the narrow phase predicts contacts that far ahead, so a fast
  body stops metres in front of an obstacle (measured: a body at 30 m/s was
  halted at `-0.08` before touching a wall at `-0.01`). Binding
  `SpeculativeMargin::ZERO` to the fast layers keeps their contacts real
  while `SweptCcd` (whose AABB update ignores the bound and sweeps the body's
  actual motion) still finds the pair in time.
* **Avian's swept AABB is updated at the end of a tick**, from the
  post-integration position to the predicted next one. A body spawned *already
  at speed* therefore has a tight AABB for its first tick: spawned 2 m in
  front of a wall at 5 m/tick it tunnels on tick 1 (measured `starts = []`),
  while spawned 10 m away it is caught on the crossing tick. Detection for a
  freshly spawned fast body is only guaranteed from its second tick on (see
  limitations).
* **A force on a sleeping body is silently wrong without a wake.** The
  `Forces` query *does* match a sleeping body (so the F23-A queue counted it
  as applied), `apply_force` only resets the sleep timer, and the sleeping
  step has no `SolverBody` to apply or clear the increment. Measured: velocity
  stayed `0` in the submitting tick and `0.000347` the next — exactly
  `F/m * dt * dt`, the increment scaled twice. Removing `Sleeping` before the
  drain and letting the chain's automatic sync point rebuild the solver body
  yields exactly `F/m * dt` in the submitting tick.
* **`CollisionEventsEnabled` is sampled when the collider is inserted.** The
  collider-tree proxy flag comes from `Has<CollisionEventsEnabled>` at tree
  insertion, and no observer refreshes it later; adding the flag *after* the
  collider (which the first draft of `spawn_body` did) produced zero events
  with no error. The production path now inserts every flag first and the
  collider last, and the AC02 tests are what caught it.
* **The message stream carries one event per contact pair; the per-side
  fan-out is observers only.** `NarrowPhase::update` writes a single
  `CollisionStart`/`CollisionEnd` for the pair
  (`avian3d-0.7.0/src/collision/narrow_phase/system_param.rs`), and only
  `trigger_collision_events` — in `PhysicsStepSystems::Finalize`, the channel
  this reporter does *not* read — duplicates it to one observer per side that
  carries `CollisionEventsEnabled`. The reporter still keys on the unordered
  pair and counts a repeat start of an active pair as `suppressed`, so a
  second copy can never double-report; every measured run (AC02 wall and
  trigger crossings, the reflected second crossing, the mis-bound pair) gave
  `suppressed = 0`.
* **A kinematic body integrates position from `LinearVelocity` but never its
  velocity** (`integrate_velocities` skips `flags.is_kinematic()`), and
  switching `RigidBody` kind preserves `Position`/`Rotation`/`LinearVelocity`
  exactly — measured byte-identical samples across the switch.

Schedule hooks remain the ones measured in F00-B and F23-A
(`docs/findings/2026-09-23-pinned-bevy-0.19-avian-0.7-schedule-api.md`,
`docs/findings/2026-09-29-f23-a-avian-schedule-adapter-and-collision-layers.md`);
the reporter runs after `PhysicsSystems::StepSimulation`, where
`CollisionEventSystems` has already written this tick's events.

## Designed behavior, not original data

Every binding, flag and rule here is newly authored project design:

* which layers get swept detection, a bounded speculative margin, the event
  flag, and that a dynamic body carries an explicit mass — all follow the
  spec's non-negotiable behaviors 2/3/4, not measured original values;
* `emits_contact_reports` (aircraft, projectile, trigger) and the "one report
  per contact episode" dedup rule are the designed stand-ins for the
  contract's "apply damage once even if several collision features report the
  same hit";
* the three control modes and the rule that a transition writes nothing but
  the `RigidBody` kind and clears sleep are designed; whether the original
  used kinematic scripted actors at all is **unknown**;
* the 120 Hz rate and `SubstepCount(1)` remain F23-A's designed baseline.

The following stay **unknown** and are not guessed: the original tick rate
and force ordering (F23-D, F16-D), the original collision layers/matrix and
whether its triggers dealt damage (F18-A, F23-D), the original mass/inertia
sources (F24/F26 — here mass is explicit input, never autogenerated convex
mass), and how the original deduplicated hits.

## Known limitations that gate later stages

Affected content: the F23-B body-creation path, the contact reporter and the
force queue. Resolving tasks: **F23-C** (producer/consumer wiring,
retention), **F23-D** (stability/convergence evidence).

1. **First-tick spawn hole.** A fast body spawned within one tick's travel of
   an obstacle is not in the broad phase for its first tick and can tunnel
   once. Nothing in the creation path forces an initial swept AABB. Affects:
   projectiles and aircraft spawned at speed near geometry. Resolved by
   F23-C's spawn wiring (spawn a tick early / seed the swept AABB) and
   re-measured by F23-D.
2. **Reporter retention.** `ContactReports` keeps only the current tick's
   batch plus monotonic counters, and its `active` set is only cleared by
   `CollisionEnd` or `clear()`; Avian does not guarantee an end event when a
   collider despawns, so an active pair can be retained after its body is
   gone (harmless: entity generations differ). Who consumes reports, and with
   what retention, is F23-C's wiring decision.
3. **No producer or consumer yet.** Nothing in the runtime submits
   `BodySpec`s, switches modes or reads `ContactReports`; F23-C wires the
   actual aircraft release, projectile impacts and trigger rules.
4. **Static ↔ dynamic resumption.** A transition writes no velocity, so a
   `Static` body that returns to `Dynamic` resumes its last stored velocity.
   That is the designed rule (never rewrite state behind the caller's back);
   whether gameplay wants a zeroed release is F23-C's call. AC03's full
   no-discontinuity scenario remains F23-C's minimum scenario.
5. **Only box colliders.** `spawn_body` takes half extents; mesh/compound
   colliders and loadout-derived mass properties are F11/F24/F26 content.
6. **Swept detection is opt-in per layer, not measured.** Which original
   objects needed continuous detection is unknown until F23-D.

## Review pass (2026-09-30)

Reviewed against `### F23-B`, `docs/contracts/FLIGHT-PHYSICS.md` and the agent
contract. Implementer: mimo-1 (earlier session, submitted `496db86`).
Reviewer: mimo-1, a fresh session with no context from the implementation —
this is an independent code/test review of synthetic-only work, not original
reference evidence and not human review. Only owner paths changed
(`crates/cs_app/src/physics/`, `crates/cs_sim/src/collision.rs`,
`crates/cs_app/tests/physics/`, `docs/findings/`); no protected path.

What the review added:

* **`crates/cs_app/tests/physics/reports.rs`** (new, wired in the test
  `main.rs`): the reporter's two refusal paths had no coverage.
  `accept_f23_b_an_event_from_an_unbound_body_is_counted_unclassified` strips
  the `BodyLayer` marker from a trigger the projectile crosses and requires
  `total = 0`, `unclassified = 1` — an event from a body outside
  `spawn_body` must be counted, never guessed.
  `accept_f23_b_a_pair_the_declared_matrix_forbids_is_counted_ignored`
  force-binds two `Trigger` bodies wider than `designed_partners` (a pair the
  declared matrix keeps apart, i.e. exactly the binding mistake the counter
  exists for) and requires `total = 0`, `ignored = 1`.
* **`accept_f23_b_a_later_crossing_of_the_same_pair_is_reported_again`**
  (`sweeps.rs`): the same projectile is reflected back through the same
  trigger, and the second crossing must be its own episode
  (`total = 2`, `suppressed = 0`). "Exactly once" is per crossing, not once
  ever; this is what a retained active-pair set would break.
* **`PhysicsTickLedger` wake-counter docs** now say per *request*: two
  requests aimed at the same sleeping body count twice and share one wake,
  which is what the implementation does. The old wording ("sleeping bodies
  woken") described a per-body count that was never implemented.

Removal checks re-run on this branch (each applied, run, reverted):

| Removed | Result |
| --- | --- |
| `SweptCcd` binding in `spawn_body` | 5 failures: the original 3 plus both new crossing tests |
| `wake_requested_bodies` from the adapter chain | `accept_f23_b_force_wakes_a_sleeping_body_in_the_same_tick` fails, as recorded above |
| `PhysicsBodiesPlugin` registration in the fixture | every reporter-driven test fails on the missing resource |
| `CollisionEnd` handling in `record_contact_reports` | only `accept_f23_b_a_later_crossing_of_the_same_pair_is_reported_again` fails (`total = 1`, `suppressed = 1`) |
| `unclassified` / `ignored` increments | only the two `reports.rs` tests fail |

Not re-run here: the "event flag inserted after the collider" development
mutation; its consequence (`total = 0`, `unclassified = 0`) is re-measured
every time the AC02 tests pass, because `spawn_body` inserts the flags before
the collider and both tests read that event stream.

## Evidence

Synthetic fixtures and pins only: authored masses, extents, positions and
speeds, no `CS_GAME_DIR` read, no original data. No visual, audible or
ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F23-avian-integration-collision-and-fixed-step-authority.md`
  (`### F23-B`), `docs/contracts/FLIGHT-PHYSICS.md`.
- `docs/findings/2026-09-29-f23-a-avian-schedule-adapter-and-collision-layers.md`
  (carried-over limitations 1, 2 and 4 — all three are addressed here),
  `docs/findings/2026-09-23-pinned-bevy-0.19-avian-0.7-schedule-api.md`,
  `docs/findings/2026-09-23-t334-first-frame-fixed-step.md`.
- Pinned sources in the local crate cache: `avian3d-0.7.0`
  (`collision/collider/backend.rs`, `collider_tree/{tree,update}.rs`,
  `collision/narrow_phase/{mod,system_param}.rs`, `dynamics/ccd/mod.rs`,
  `dynamics/rigid_body/forces/query_data.rs`,
  `dynamics/solver/solver_body/plugin.rs`, `dynamics/integrator/mod.rs`),
  `bevy_ecs-0.19.1` (`schedule/auto_insert_apply_deferred.rs`).
