# F29-C.1: wiring the propulsion system-disable into the flight gate

Date: 2026-10-05. Task: F29-C.1 (Rally #517) "Wire the Propulsion
system-disable into the flight-authority gate", the follow-up filed by F29-C
(`docs/findings/2026-10-02-f29-c-damage-consumers.md`). Spec:
`specs/F29-damage-zones-armor-destruction-and-bailout.md`, `### F29-C`.
Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`. Capabilities used:
ordinary build/test only — no `CS_GAME_DIR` read, no render, no audio, so no
`private/evidence/` report is produced.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/damage.rs` (extend): `apply_propulsion_state`, the
  `DamageConsumerEvent::ThrustCut` / `ThrustRestored` events and the
  `DamageConsumerReport::thrust_cut` / `thrust_restored` counters; module
  docs updated.
- `crates/cs_app/tests/damage/propulsion_gate.rs` (**new**, compiled into the
  F29-C damage-consumer binary as a module rather than as a target of its own —
  see "The first two landing attempts died of the runner's disk"): 7
  `accept_f29_c_propulsion_` tests.
- This file.

No protected path, no `cs_sim` change, no original data, no binary.

**One observable failure, before the change:** the resolver answers
`system_state(actor, SystemKind::Propulsion) == Disabled` once `engine_1` of
the synthetic airframe is destroyed, but nothing read it. The spawned body's
`FlightAircraft` kept `DamageState::thrust_authority == 1`, so the production
flight tick went on producing full thrust from a destroyed engine.

## The consumer

The flight-authority gate is the per-body `FlightAircraft` record
(`cs_app::physics::flight`): its `DamageState` is what `FlightModel::compute`
scales thrust and boost by (`damage.thrust_authority`) on every fixed tick.
`cs_sim::flight` holds only the pure equations; it has no per-actor record to
gate, so the consumer is written against `FlightAircraft::set_damage` and
`cs_sim::flight` is unchanged.

## The designed rule

`apply_propulsion_state(resolver, actor, flight) -> DamageConsumerOutcome`
is state-driven like `apply_damage_state`: it reads the resolver's aggregate
`system_state(actor, Propulsion)`, never the `SystemDisabled` event history.

| propulsion state | flight gate |
| --- | --- |
| `Disabled` (any declaring part destroyed) | `thrust_authority` cut to `0`; `ThrustCut` logged once |
| `Enabled` | a cut (`thrust_authority == 0`) is lifted to `1`; any other value is left alone; `ThrustRestored` logged on a real change |
| `Unknown` (an unresolved declaring pool, nothing destroyed) | nothing asserted; each unresolved declaring pool refused as `UnresolvedIntegrity` |
| `None` (graph declares no propulsion carrier) | untouched, nothing logged |

A foreign session generation and an unregistered actor are refused by name
(`ForeignSession`, `UnknownActor`) and change nothing. Control authority and
lift scale are never written: the gate is the propulsion system's, not the
airframe's. The pass is convergent — the same state applied twice changes
nothing the second time.

**Aggregation of several engines** is the resolver's existing F29-B rule: any
destroyed part declaring `Propulsion` disables the whole system, so on a
multi-engine airframe one destroyed engine cuts *all* thrust. That is the
designed vocabulary of `SystemState`, not a measured original behavior.

**Designed behavior, not original data.** Whether the original cut thrust on a
destroyed engine at all, whether it cut it fully or partially, and how it
aggregated several engines are **unmeasured** (F29 "Research boundary"; the
manual and guides name no damage equations). **Affected content:** every
airframe whose damage graph declares an `Engine` (or any node) disabling
`Propulsion` — the synthetic `engine_1` here and, once lowered from original
data, every retail airframe with an engine part. **Resolving task:** F29-D
keeps the original-family gate; nothing here claims an original behaviour.

## Known limits

- **Producer overlap.** `FlightEquipment` (the mission/loadout producer) also
  writes `DamageState` through `FlightAircraft::bind_equipment` whenever its
  record changes, which replaces the gate's cut until the next consumer pass.
  Rebinding the same record is a no-op, so the cut holds while the equipment
  is unchanged. Merging the two producers needs a change in
  `cs_app::physics::flight`, outside this task's owner paths; filed as a
  follow-up.
- **A partial authority is not remembered across a destroy/repair.** The gate
  lifts its own cut to full (`1`), not to a partial value another producer
  had written before the cut.
- **The gate cannot tell its own cut from another producer's zero.** An
  `Enabled` state lifts any `thrust_authority == 0`, including a zero a
  `FlightEquipment` record wrote. No such producer writes zero on this branch.
- **Repair is modelled, not driven.** `DamageResolver` has no repair entry
  yet, so the repair test applies an intact authoritative state (a freshly
  registered actor), as F29-C's convergence test does. A real repair path
  must reach the same `Enabled` state.
- No scheduled ECS system calls the pass yet; like `apply_damage_state` it is
  the seam the session tick calls with the actor's resolver and body.

## Tests

Every test drives production code — `lower_graph` / `lower_policy`, the real
`DamageResolver`, `spawn_flight_body` and the production `FlightForcesPlugin`
tick in a `PhysicsFixture` — and observes the thrust off the flight tick's own
`last_output()`.

| test | what it pins |
| --- | --- |
| `accept_f29_c_propulsion_a_destroyed_engine_cuts_thrust_in_the_flight_tick` (minimum) | destroying `engine_1` cuts thrust authority once; the next tick produces 0 N with the engine running at full throttle; control/lift untouched; convergent |
| `accept_f29_c_propulsion_a_scratched_engine_keeps_full_thrust` | `Damaged` is not `Destroyed`: no change, same thrust |
| `accept_f29_c_propulsion_a_repair_restores_thrust` | an intact state lifts the cut; the next tick thrusts again |
| `accept_f29_c_propulsion_an_enabled_engine_leaves_a_producers_partial_authority` | an enabled engine does not overwrite a producer's 0.5 |
| `accept_f29_c_propulsion_an_unresolved_engine_pool_is_refused_and_leaves_the_gate` | unknown pool refused by name; neither an open nor a cut gate is changed |
| `accept_f29_c_propulsion_a_foreign_or_unregistered_actor_is_refused` | `ForeignSession` / `UnknownActor`, gate untouched |
| `accept_f29_c_propulsion_a_graph_without_an_engine_leaves_the_gate` | no propulsion carrier → nothing gated |

## Mutation probes

Each probe edited one production line, ran the selection and restored the file
from a saved copy:

| probe | edit | result |
| --- | --- | --- |
| the gate ignores a disabled engine | `if damage.thrust_authority != 0.0` → `if false && …` | `…_a_destroyed_engine_cuts_thrust_in_the_flight_tick` and `…_a_repair_restores_thrust` failed |
| the gate never lifts its cut | `if damage.thrust_authority == 0.0` → `if false && …` | `…_a_repair_restores_thrust` failed |

### Review pass (bunny-alpha-1, 2026-10-06)

Added by the review that re-landed this stage after the first landing attempt
failed. Reviewer: `bunny-alpha-1`, a session with fresh context that did not
implement the change — a different agent name and model from the implementer,
so it is independent of the implementer's own probes but **not** independent
evidence about the original game. Same method (edit one production line, run
`cargo test --workspace --locked -- accept_f29_c_propulsion_ --include-ignored`,
restore from a saved copy; the restored file's SHA-1 was checked equal to the
committed one), three probes chosen to cover what the two above do not:

| probe | edit | result |
| --- | --- | --- |
| the whole consumer is removed | `return outcome;` inserted as the first statement of `apply_propulsion_state` | **4 of 7 failed**: `…_a_destroyed_engine_cuts_thrust_in_the_flight_tick`, `…_a_repair_restores_thrust`, `…_an_unresolved_engine_pool_is_refused_and_leaves_the_gate`, `…_a_foreign_or_unregistered_actor_is_refused` |
| a scratch is treated as a destruction | the cut arm's pattern `Some(SystemState::Disabled)` → `Some(SystemState::Disabled \| SystemState::Enabled)` | **4 of 7 failed**: `…_a_scratched_engine_keeps_full_thrust`, `…_an_enabled_engine_leaves_a_producers_partial_authority`, `…_a_destroyed_engine_cuts_thrust_in_the_flight_tick`, `…_a_repair_restores_thrust` |

The three tests that survive the first probe — the scratch, the partial
producer's authority and the graph with no engine — all assert that **nothing
changed**, so a consumer that does nothing satisfies them by construction. They
are not dead weight: the second probe shows each of them failing when the
`Damaged`/`Enabled` distinction is removed, so all seven tests are pinned by
production code and none of them passes vacuously.

## The first two landing attempts died of the runner's disk, not of a test

Worth recording, because it looks like a red branch and is not one. Rally
rebased this stage onto `main` as `ff317657a` and its landing attempt 1 of 3
failed (Rally event `landing.failed`, 2026-10-05T22:38Z), and the re-push as
`f44319ff` failed the same way. The `rust` job's `cargo test` step has **no
test failure in it at all**, in either run: the job's only `failure`-level
annotation is the runner itself,

```
Unhandled exception. System.IO.IOException: No space left on device :
'/home/runner/actions-runner/cached/2.337.0/_diag/Worker_20261005-222831-utc.log'
```

— the runner could not write its own diagnostic log, so `cargo test` never
reported a `test result: FAILED` for any binary. The same tree was green on
`cc84263c`, and `main` was green on the `cd42cd17` this was rebased onto in
the same minute. This is the fault measured and diagnosed in
`docs/findings/2026-09-30-t430-rust-lld-sigbus-in-ci.md` (runner disk, not
content), still unfixed on the owner's side because every lever it names is a
`.github/` change.

It was not bad luck either: the branch failed the same way **twice**, and the
one thing it added to every `cargo test` run was its own test target.
`crates/cs_app/tests/accept_f29_c_propulsion_gate.rs` was a **123.5 MB** test
binary on this machine, measured with the committed `[profile.dev] debug =
"line-tables-only"`, and Bevy/Avian is already linked by `tests/flight/`, so
every byte of it was budget the runner did not have. The fix inside this task's
owner paths was to compile those same seven tests into the F29-C damage-consumer
binary as a module (`tests/damage/propulsion_gate.rs`, `mod`-per-file like
`tests/world/`, `tests/physics/` and `tests/campaign/`). Same test names, same
`accept_f29_c_propulsion_` prefix, same production path, one binary fewer — and
CI run **37391915689** on the resulting `a7af5746` is green.

### The margin, measured

That green run also answers the one thing
`docs/findings/2026-09-30-t432-ci-disk-verification.md` lists as unmet
criterion 4 ("the `df` margin is unmeasured"): the workflow's own `df -h /`
steps are in its log, and they were never readable before because the failing
runs died before reaching them.

| when | `Filesystem` line from run 37391915689 |
| --- | --- |
| after the workflow's "Free runner disk space" step, before the build | `/dev/root 145G 41G 105G 28% /` |
| after `cargo test` | `/dev/root 145G 144G 428M 100% /` |

So the job's own footprint is **~104.6 GB** of a 145 GB runner, and it finishes
with **428 MB — 0.29% of the disk — of headroom, at 100% used**. The 123.5 MB
this branch added is ~29% of what was left. That is the whole mechanism: not a
defect in the code, a budget with nothing in it.

Two limits on what that table proves, stated rather than glossed:

* `df` is sampled before the build and after the tests, never during, so the
  **peak** is still unmeasured and the 428 MB is an end-of-job figure, not the
  figure the largest link actually ran against.
* 428 MB was the margin *on this run, on this runner image, with a ~1082 MB
  restored `rust-cache`*. It is one observation, not a distribution, and
  `docs/findings/2026-09-30-t430-rust-lld-sigbus-in-ci.md` records that the
  same tree, cache key and step both pass and fail. What it does establish is
  the order of magnitude: this repository now needs roughly 105 GB of the
  runner's 145 GB to be green, and there is no room left in it for one more
  100 MB test binary.

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F29-damage-zones-armor-destruction-and-bailout.md` (`### F29-C`),
  `docs/contracts/STATE-TRANSACTIONS.md`.
- `docs/findings/2026-10-02-f29-c-damage-consumers.md` (the consumer pattern
  and the follow-up this task resolves).
- `docs/findings/2026-10-02-f29-b-zones-armor-and-system-disablement.md`
  (`SystemState` aggregation).
- `crates/cs_sim/src/damage/{resolver,graph}.rs`,
  `crates/cs_sim/src/flight/{model,tuning}.rs`,
  `crates/cs_app/src/physics/flight.rs`, `crates/cs_app/src/damage.rs`.
