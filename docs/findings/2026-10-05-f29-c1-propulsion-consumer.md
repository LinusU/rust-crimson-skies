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
- `crates/cs_app/tests/accept_f29_c_propulsion_gate.rs` (**new**): 7
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
