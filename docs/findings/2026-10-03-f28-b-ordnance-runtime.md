# F28-B: the per-tick ordnance runtime — direct, proximity, guidance, status and boost mechanisms

Date: 2026-10-03. Task: #121 "Implement direct/proximity/guidance/status/boost
mechanisms" (`specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
section `### F28-B`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`
("Boost and special models", "Collision and ballistic tests").
Capabilities used: ordinary build/test only — no `CS_GAME_DIR` read, no render,
no audio, so no `private/evidence/` report is produced and none is claimed.

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/weapons/ordnance.rs` (extend): the F28-B runtime —
  `OrdnanceRuntimeError`, `LiveOrdnance`, `OrdnanceTick`, `GuidanceTick`,
  `OrdnanceRuntime` (with `launch`, `advance`, `decide`, `guidance_tick`,
  `apply_statuses`, `advance_status`, `route_trigger`, `register_nitro`,
  `request_nitro`, `remove`) and the private `check_finite_wind`. One F28-A
  function, `OrdnanceState::fuse_decision`, gains the retired-item branch
  described below.
- `crates/cs_sim/src/weapons/mod.rs` (wiring only): the flat re-export list.
- `crates/cs_sim/tests/accept_f28_b_ordnance_runtime.rs` (**new**, 12 tests).
- This file.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data, no
binary. Task test prefix `accept_f28_b_`; 12 tests, all passing.

**One observable failure, before the change:** F28-A defined every record and
every *pure* decision, but nothing owned a **live** item or drove those
decisions per tick. A caller could build a `ProjectileOrdnance` and an
`OrdnanceState`, but nothing advanced the item and produced the
`ProjectileSegment` its fuse tests; nothing turned a `GuidanceSet` update into
the item's consequence; nothing bridged a declared status effect into the
ledger keyed by the launching item; nothing turned a triggered item's declared
damage channels into `HitEvent`s; and nothing owned an actor's nitro capacity
between ticks. The minimum scenario makes it concrete: a guided item whose
target is destroyed reported `Lost { Detonate }` from `GuidanceSet::session_tick`
alone, but with no runtime the item kept flying and its tracker could outlive
it — the loss was announced and never enforced.
`accept_f28_b_guidance_loses_a_destroyed_target_and_ends_safely` asserts that
the same tick removes the live item and its tracker and that no later tick can
re-acquire, so removing `guidance_tick`'s end handling makes it fail.

## The runtime is the production path F28-A left unowned

`OrdnanceRuntime` is a session-confined per-tick owner of the live ordnance.
Every durable map is keyed by a stable id — a `ProjectileId` per item, an
`ActorId` per booster, a `StatusEffectInstanceId` per status effect — and the
whole teardown is dropping the value. It has no Bevy, Avian or renderer
dependency, exactly the split F27-B made for gun rounds
(`docs/findings/2026-10-02-f27-b-gun-cadence-mounts-and-swept-ballistics.md`).

* **`launch`** composes the world release velocity from the *declared*
  `LaunchGeometry` through F27's `MountTransform::world_velocity_mps` (never a
  second composition rule) and removes the wind **once** through the shared
  `air_relative_velocity_m_s`, so the item flies a constant air-relative
  velocity and `advance` rebuilds the world frame each tick through
  `world_velocity_from_air_m_s`. A targeted component registers its
  `GuidanceTracker` at launch. A foreign-session item, a foreign-session
  shooter, a duplicate id, a non-finite wind or release velocity, and a
  zero-tick lifetime are each refused **by name**. The shooter and the item are
  checked separately so the error's `found` names the generation that was
  actually wrong.
* **`advance`** is **all-or-nothing**: every item's next position is validated
  before any item moves, so a single non-representable next position refuses
  the whole tick and leaves every item exactly where it was. The returned
  `OrdnanceTick` names each item's swept `ProjectileSegment`, the items whose
  lifetime ended, and the items removed because their fuse had already
  triggered. Expired and triggered items are removed *after* their final
  segment is recorded, and their guidance trackers are dropped with them, so no
  stale tracker can outlive its item.
* **`decide`** runs the item's own current segment through
  `OrdnanceState::fuse_decision` (F28-A), so the arming gate, the swept
  proximity query and the once-only trigger latch are unchanged. Only a live
  item is accepted; an unknown id is refused.
* **`guidance_tick`** drives the lost-target contract. A session change is a
  loss (`ForeignSession`) applied to every tracker; a `Coast` loss leaves the
  item flying its last vector; a `Disarm` loss retires the item so it deals
  nothing; a `Detonate` loss removes the item from the live set and reports it
  in `GuidanceTick::detonated` so the caller applies the blast. The spent
  tracker is dropped in every ending case, and there is no re-acquisition and
  no second announcement.
* **`apply_statuses` / `advance_status`** are the bridge from a component's
  declared `OrdnanceStatusEffect` list to the bounded `StatusEffectLedger`,
  naming the launching item's own `OrdnanceId` as the source so an effect
  survives a reload and names what applied it.
* **`route_trigger`** turns one triggered item's declared damage channels into
  `HitEvent`s, emitting one hit per **non-zero** channel, stamped with the
  item's shooter, the caller-supplied target and node, and a per-session hit
  id. Each item routes **once**: a second call is
  `OrdnanceRuntimeError::AlreadyRouted`, which is how "apply damage once even
  if several collision features report the same hit" becomes structural here
  too. An un-triggered item is refused by name rather than silently skipped.
* **`register_nitro` / `request_nitro`** wrap one actor's `NitroLedger`. The
  request takes the held control and a tick, never a duration, and a refused
  activation consumes nothing. The whole runtime exposes no pose and no method
  takes a `std::time::Duration`, so AC04's "boost must never teleport or scale
  render dt" is structural rather than a convention.

## The one F28-A function F28-B changes

`OrdnanceState::fuse_decision` now returns `FuseInert::AlreadyTriggered` first
for a **retired** (`destroyed`) item. F28-A's `retire` documents exactly this
contract ("it does not fabricate a cause: after this the item reports
AlreadyTriggered rather than pretending a detonation happened"), but
`fuse_decision` did not implement it: a retired item fell through to the
arming/impact/proximity tests and could still trigger. That is the case
`accept_f28_b_a_disarmed_item_can_never_fire_its_fuse` pins:
`guidance_tick` retires the item on a `Disarm` loss, and its fuse must never
fire afterwards. The change is a two-line early return at the top of the
function and no F28-A test regressed.

## What this stage does not do (F28-C's and F28-D's halves)

There is **no** ECS system or schedule registration, no Avian projectile body
or collider, no cockpit launch gesture or hardpoint firing order, no bank
selection, no muzzle/blast effect or sound consumer, no network event, no
status-effect consumer in the flight model and no nitro consumer of
`cs_sim::flight::BoostParameters`. `OrdnanceRuntime` is the authoritative
calculation an ECS body mirrors, never a second integrator. `cs_content` and
`cs_app` keep the F28-A declared/lowering boundary unchanged; F28-C wires the
runtime into its producers and consumers, and F28-D closes the original
catalogue.

## Unknowns recorded (not guessed)

Everything numeric here is the F28-A synthetic fixture's own value, carried
unchanged (`SYNTHETIC_LAUNCH_SPEED_MPS`, `SYNTHETIC_ARMING_TICKS`,
`SYNTHETIC_TRIGGER_RADIUS_M`, the lifetimes, damage, choke duration, nitro
capacity/consumption/recovery/thrust). None of it is original. The F28-B
runtime is designed vocabulary; the unmeasured original facts are:

- **The original ordnance catalogue** and which of the six families it has.
- **The proximity fuse's trigger shape** — the runtime tests a swept center
  path against a radius, as F28-A declared; a spherical fuse, shaped zone and
  directional sensor remain three different unmeasured rules.
- **The arming condition** (time, distance, launch-vehicle state or lock) and
  whether an item whose lifetime has run out may still trigger its proximity
  fuse.
- **Every guided weapon's lost-target behavior** and whether the original
  re-acquires a temporarily lost tagged target.
- **The nitro activation rule, burn duration, recovery model, tradeoffs and
  whether capacity carries across a mission.**
- **Whether the original's area effects damage, choke, stall or mark**, and
  whether the combination is data or code.

Recording them here keeps the stage honest; F28-D owns measuring them and
`docs/findings/2026-10-01-f28-a-ordnance-behavior-and-effect-registry.md`
already lists them in full, so no duplicate task is created.

## Test sensitivity (measured, one mutation at a time)

Seven mutations were applied to the production code on the submitted
signatures and the F28-B test binary re-run after each (a plain
`cargo test -p cs_sim --test accept_f28_b_ordnance_runtime` is enough because
the mutated code is exercised from that binary); the file was restored from a
byte-identical backup (`shasum`) after each probe:

| mutation | caught by |
| --- | --- |
| `remove` no longer drops the item's guidance tracker | `accept_f28_b_removing_an_item_drops_its_tracker` |
| a `Disarm` loss no longer retires the item | `accept_f28_b_a_disarmed_item_can_never_fire_its_fuse` |
| a `Detonate` loss no longer removes the live item | `accept_f28_b_guidance_loses_a_destroyed_target_and_ends_safely`, `accept_f28_b_guidance_loses_its_target_on_a_session_change` |
| `route_trigger` allows the same item to route twice | `accept_f28_b_an_impact_trigger_routes_its_declared_damage_once` |
| `route_trigger` drops the un-triggered check | `accept_f28_b_routing_refuses_an_untriggered_or_unknown_item` |
| `advance` no longer removes expired items | `accept_f28_b_an_item_retires_after_its_declared_lifetime` |
| `fuse_decision` drops the retired-item branch | `accept_f28_b_a_disarmed_item_can_never_fire_its_fuse` |

The minimum scenario itself is the first four rows: guidance loss is reported,
enforced (the item ends or is retired) and terminal, and the tracker cannot
outlive its item. Any one of them removed turns a named `accept_f28_b_*` test
red.

## Not claimed

No original-data verification, no catalogue audit, no ECS/Avian binding, no
cockpit input, no effects or audio consumer, no status-effect or nitro consumer
in the flight model, and no network event. This stage awards at most
**checked**; F28-C wires the runtime, F28-D and the owner's evidence gate the
rest.
