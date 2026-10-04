# F37-E1: the writer for `MissionFacts::actors`, measured or refused by name

Date: 2026-10-04. Task: #612 "Populate MissionFacts so actor-state conditions
can be observed (F37/F38)". Test prefix `accept_f37_e1_` in
`crates/cs_sim/tests/accept_f37_e1_mission_actor_facts.rs`. Capability used:
none beyond ordinary build/test — every mapping below was already measured by
F39-E4 and F39-E7, so no `retail` read was needed for this stage.

## The gap this closed

`Condition::ActorIs` compares an actor against
`cs_script::runtime::MissionFacts::actors`. F39-E7 measured that **no writer
existed**: every caller in the repository passed `MissionFacts::default()`, so
none of the seven `ActorState` values could satisfy a scripted condition. The
evaluator takes facts from its caller and must not invent world state, so the
writer belongs on the simulation side.

## What was built

`cs_sim::mission::ActorFactTable` — the per-session map from mission actors
(`cs_script::ir::ActorId`, `MissionFacts`' own key space) to the `ActorState`
the authoritative lifecycle record puts them in. It keeps the same
once-per-kind ledger shape the damage resolver's record keeps: registration
admits an actor `Alive`, one `LifecycleKind` is recorded at most once per
actor, and a terminal transition closes the record. `MissionSession` owns the
table, exposes `register_actor` / `actor_facts_mut` for admissions, and offers
`advance_observed` / `step_observed` — the wired path that folds one tick's
`ActorFactInput` (the same `(actor, kind)` surface `TickInput::lifecycles`
carries) and then steps on the populated map. A refused input folds nothing
and does not step, so the producer may fix a defect and offer the same tick
again. The table crosses `MissionSessionSnapshot` — an unfired `ActorIs` must
still observe the state an actor reached before the save.

## The gate

`lifecycle_fact_effect` is the single place a `LifecycleKind` maps to an
`ActorState`, in the shape of F39-E4's `CountKind::from_lifecycle` gate —
measured or refused by name, never because the variant exists:

| `LifecycleKind` | fact effect | why |
| --- | --- | --- |
| `Destroyed` | writes `Dead` | the measured producer (F39-E4) |
| `OwnershipCaptured` | writes `Captured` | the measured producer (F39-E4) |
| `Despawned` | writes `Despawned` | the measured producer (F39-E4) |
| `PilotBailout` | recorded, writes nothing | `pilot_bailout_writes_no_actor_state` — F29 keeps bailout apart from death; nothing measured maps it to a state |
| `MissionRemoved` | removes from accounting | the actor is **absent** from `facts()` — the event's own meaning, deliberately not `Despawned` and never a kill |

The producer direction (`actor_state_producer`) is derived from the same gate
so the two cannot drift. `Alive`'s producer is registration — the admission
event — not a default a caller can reach for. The unproduced states refuse by
name: `Disabled` and `Escaped` (`no_lifecycle_transition_writes_this_actor_state`
— the same `None` F39-E4's producer gate answers), `Detached`
(`detached_is_an_event_not_an_actor_state` — F39-E7 measured the original
writes a detach as an event, never as a category).

## Decisions that are designed, not measured

- **Precedence inside a row.** A set holding several transitions resolves to
  one state for the exact-match `ActorIs` read: terminal wins
  (`MissionRemoved` → absent, `Despawned` → `Despawned`), then `Destroyed` →
  `Dead`, then `OwnershipCaptured` → `Captured`, else `Alive` — the same
  question `AlliesRoster::status` answers the same way. Whether the
  original's condition latched on the earlier state of a same-tick pair is
  unmeasured; this is the recorded designed reading.
- **`MissionRemoved` reads as absent, not as `Despawned`.** The contract's
  six distinctions keep a cinematic removal apart from a kill and from a
  despawn; absence from `MissionFacts` is what "left mission accounting"
  says, so no `ActorIs` state claims it.
- **The source is the damage lifecycle, never `NetLifecycle`.** The net
  record folds removal, capture and unload into one `Despawned` kind — it
  cannot be read without conflating exactly the distinctions the contract
  demands kept apart.
- **Unproduced states are unreachable, not writable.** No caller-declared
  write path exists: a host-declared `Disabled`/`Escaped`/`Detached` would
  need a designed merge rule against the recorded set that nothing measures,
  so the states refuse by name until a producer is measured (for `Detached`,
  the candidate is F34's release path keeping objective identity — the
  wiring question belongs to the task that measures it).

## Follow-ups recorded here, not done

- `cs_app::objectives`' `condition_state` doc note still describes the
  pre-writer state ("nothing in the engine writes one yet") — the writer
  now exists, and the note should be revisited by the F39 series' owner when
  the app-side wiring lands.
- The mission host still must feed `advance_observed` (and map
  `cs_types::net::ActorId` to `ir::ActorId` on input, as `TickInput` already
  does): the table is the writer; a caller wiring it is the remaining F39-C
  surface.

## Non-goals kept

F39-E4's committed census is untouched — `Detached` was not added to the
counted categories, and `cs_content::objectives::original_count_category_refusal`
was not restated. `CountKind` remains the counted vocabulary; `ActorState` is
the condition vocabulary; this gate maps the lifecycle record onto the latter
without widening the former.
