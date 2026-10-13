# F29-C.1a: the propulsion gate is a record of its own, composed at read

Date: 2026-10-13. Task: F29-C.1a (Rally #664) "Keep the propulsion thrust cut
when FlightEquipment rebinds damage", the follow-up F29-C.1 filed under
"Known limits / producer overlap". Spec:
`specs/F29-damage-zones-armor-destruction-and-bailout.md`, `### F29-C`.
Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`. Capabilities used:
ordinary build/test only — no `CS_GAME_DIR` read, no render, no audio, so no
`private/evidence/` report is produced.

## The failure being fixed (observed, before the change)

`FlightAircraft` held one `damage: DamageState` field with two producers.
`apply_propulsion_state` (F29-C.1) wrote the resolver's `Disabled` verdict
into `damage.thrust_authority = 0`; `bind_equipment` (F24-C) replaced the
whole `DamageState` with `FlightEquipment::damage` whenever the producer's
record *changed* — a fuel burn or ordnance release — which silently lifted
the cut until the next consumer pass. Rebinding an identical record is a
no-op, so the cut survived only while the equipment was unchanged. A
probe-only demonstration on this branch: `bind_equipment` re-opening the
gate puts a destroyed engine back to 9000 N on the next production tick
(mutation probe 2 below).

## The chosen design

Of the two options the task named — compose in the flight driver, or
schedule the consumer pass after equipment binding every tick — this branch
implements the first:

* `FlightAircraft` now holds `declared_damage` (the equipment/mission
  producer's record: `FlightEquipment`, `set_damage`, `FlightSpawnSpec`)
  **and** `propulsion_gate: PropulsionGate` (`Open` / `Cut`), the damage
  consumer's own record. Each field has exactly one writer; there is no
  shared `DamageState` left for one producer to clobber the other through.
* `FlightAircraft::damage()` returns the *composition* — the declared record
  with `thrust_authority` forced to `0` while the gate is `Cut` — and
  `compute_tick` feeds that to the law. `damage()` remains "the damage state
  the equations see", so every existing reader and test observes the same
  contract.
* `apply_propulsion_state` now reads and writes the gate
  (`propulsion_gate()` / `set_propulsion_gate`) instead of rewriting the
  damage state: `Disabled` cuts the gate, `Enabled` opens a gate it cut.
* `bind_equipment` needed no special-casing: it writes `declared_damage` and
  never touches the gate, which is the fix.

The gate is a binary enum, not an `f64` multiplier, because the resolver's
`SystemState` vocabulary is binary and no producer owns a partial gate — a
partial propulsion authority is declared damage, not a gate state.

**Why not the scheduling option.** `apply_propulsion_state` is deliberately
a seam the session tick calls with the actor's resolver and body — no ECS
system owns it yet. Making the flight driver call it after `bind_equipment`
would put a `DamageResolver` resource and the actor binding inside
`FlightForcesPlugin`, a larger wiring change for a fix that is fundamentally
about *two records sharing one field*, not about order: even perfectly
ordered, a shared field still loses whichever write ran first this tick.

## Known limits resolved and carried over

From `docs/findings/2026-10-05-f29-c1-propulsion-consumer.md`:

* **Producer overlap** — resolved: the two producers compose.
* **"The gate cannot tell its own cut from another producer's zero"** —
  resolved by construction: a producer-declared `thrust_authority == 0` now
  stays `0` under `Enabled` (`…_an_enabled_engine_keeps_an_equipment_declared_zero`).
* **"A partial authority is not remembered across a destroy/repair"** —
  narrowed, not removed: the gate still opens to whatever the *declared*
  record currently holds, so a repair after an equipment change restores the
  new declared authority (`…_a_repair_restores_thrust_across_the_rebind`
  pins 0.5). A pre-damage declared value overwritten mid-cut is the
  producer's own record-keeping, not the gate's.
* **Repair is modelled, not driven** — unchanged: `DamageResolver` still has
  no repair entry; tests model `Enabled` with a freshly registered actor.
* **No scheduled ECS system calls the pass** — unchanged; this task did not
  add the session wiring.

## Files

- `crates/cs_app/src/physics/flight.rs`: `PropulsionGate`, the
  `declared_damage` / `propulsion_gate` split, `damage()` composition,
  `propulsion_gate()` / `set_propulsion_gate()`.
- `crates/cs_app/src/damage.rs`: `apply_propulsion_state` drives the gate;
  `set_thrust_authority` removed; `ThrustRestored` doc corrected.
- `crates/cs_app/src/physics/mod.rs`: re-export `PropulsionGate`.
- `crates/cs_app/tests/damage/propulsion_equipment.rs` (**new** module in the
  F29-C damage-consumer binary, for the measured disk reason
  `propulsion_gate.rs` records): 3 `accept_f29_c_propulsion_equipment_` tests.
- `crates/cs_app/tests/accept_f29_c_damage_consumers.rs`: module wiring only.
- This file.

No protected path, no `cs_sim` change, no original data, no binary.

## Tests

| test | what it pins |
| --- | --- |
| `accept_f29_c_propulsion_equipment_a_destroyed_engine_stays_cut_across_an_equipment_change` (minimum) | a destroyed engine produces 0 N across an equipment *change* in the production `FlightForcesPlugin` tick; the rebind still lands (mass moves 1750→1500 kg); convergence after the rebind |
| `accept_f29_c_propulsion_equipment_a_repair_restores_the_declared_authority` | repair opens the gate and thrust returns at the *declared* 0.5, not an invented pristine 1 |
| `accept_f29_c_propulsion_equipment_an_enabled_engine_keeps_an_equipment_declared_zero` | `Enabled` lifts only the gate's own cut — a producer's declared 0 stands |

## Mutation probes

Each probe edited one production line, ran
`cargo test -p cs_app --test accept_f29_c_damage_consumers -- accept_f29_c_propulsion`
and restored the file from a saved copy:

| probe | edit | result |
| --- | --- | --- |
| the composition is removed | `damage()` returns `self.declared_damage` unconditionally | **4 failed**: `…_a_destroyed_engine_stays_cut_across_an_equipment_change`, `…_a_repair_restores_the_declared_authority`, plus the F29-C.1 `…_a_destroyed_engine_cuts_thrust_in_the_flight_tick` and `…_a_repair_restores_thrust` |
| a rebind lifts the cut (the original bug shape) | `bind_equipment` writes `self.propulsion_gate = PropulsionGate::Open` | **2 failed**: `…_a_destroyed_engine_stays_cut_across_an_equipment_change` (9000 N after the rebind), `…_a_repair_restores_the_declared_authority` (4500 N while destroyed) |
| the consumer never cuts | `Disabled` arm predicated with `false &&` | **4 failed**: both new destroy/repair tests and the same two F29-C.1 tests as probe 1 |

`…_an_enabled_engine_keeps_an_equipment_declared_zero` survives probes 1 and
2 by construction (a gate that is never cut composes to the declared zero);
probe 3's surviving scratch/refusal tests likewise assert no-change
behaviour. Between the three probes every new test fails at least once, and
the F24-C equipment tests keep passing throughout.

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F29-damage-zones-armor-destruction-and-bailout.md` (`### F29-C`),
  `docs/contracts/STATE-TRANSACTIONS.md`.
- `docs/findings/2026-10-05-f29-c1-propulsion-consumer.md` (the producer
  overlap this resolves).
- `crates/cs_app/src/physics/flight.rs`, `crates/cs_app/src/damage.rs`,
  `crates/cs_sim/src/flight/tuning.rs`,
  `crates/cs_sim/src/damage/{graph,resolver}.rs`.
