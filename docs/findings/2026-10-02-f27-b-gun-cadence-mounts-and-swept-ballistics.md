# F27-B: the gun cadence, the live mounts and the swept ballistics

Date: 2026-10-02. Task: #118 "Implement gun cadence, mounts and swept
ballistics" (`specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`,
section `### F27-B`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
Capabilities used: ordinary build/test only — no `CS_GAME_DIR` read, no render,
no audio, so no `private/evidence/` report is produced and none is claimed.

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/weapons/guns.rs` (extend): `LiveProjectile`,
  `ProjectileTick`, `ProjectileRuntimeError`, `ProjectileRuntime`,
  `CadenceRefusal`, `GunCadence`.
- `crates/cs_sim/src/weapons/mod.rs` (wiring only): the flat re-export list.
- `crates/cs_app/src/weapons.rs` (extend): `MountPoseRefusal`,
  `LiveMountTransforms`, `live_mount_transforms`, `PartSweptBox`,
  `PartSweepRefusal`, `PartSweepCandidates`, `part_sweep_candidates`.
- `crates/cs_sim/tests/accept_f27_b_guns_runtime.rs` (**new**, 8 tests).
- `crates/cs_app/tests/accept_f27_b_live_mounts_and_parts.rs` (**new**, 6 tests).
- This file.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data, no
binary. Task test prefix `accept_f27_b_`; 14 tests, all passing.

**One observable failure, before the change:** F27-A resolved a fire intent
into accepted `FireEvent`s and F27-C routed a swept contact into `HitEvent`s,
but the round *between* them had no owner. Nothing advanced a live projectile
from its muzzle to its current position, so there was no per-tick
`ProjectileSegment` to sweep; nothing read the mount pose from the live
hierarchy, so a resolver could only be handed a hand-built `MountTransform`;
and nothing produced `SweepCandidate`s from live part boxes, so a sweep could
only be fed hand-built candidates. A caller could resolve a shot and forget to
spawn its round, and a disabled mount's silence came only from F27-A's
per-mount refusal, with no cadence to observe it through.

## The `cs_sim` half: the cadence and the live rounds

`ProjectileRuntime` is the per-session set of live projectiles F27-A left
unowned:

* `spawn(&FireEvent, wind)` removes the wind from the accepted event's world
  velocity **once**, through the shared
  `cs_sim::environment::air_relative_velocity_m_s`, and stores the constant
  air-relative velocity. It refuses a foreign-session event, a duplicate id, a
  non-finite velocity or wind, and a zero-tick lifetime, each by name.
* `advance(dt_s, wind)` reconstructs the round's world velocity every tick
  through the shared `cs_sim::environment::world_velocity_from_air_m_s`, moves
  it, records the swept segment and retires the round after its final
  segment. Expired ids are removed and never reissued (the id cursor is the
  resolver's).

`GunCadence` owns one `FireResolver` and one `ProjectileRuntime` and steps them
as a unit: `fire(intent, transforms, wind)` resolves the intent and spawns
**every accepted shot's** round in the same call, so a caller cannot resolve a
shot and forget to spawn its round, and a refused intent (`IntentRefusal`)
spawns nothing because the resolver produced no event. `advance_to` ticks the
cooldowns; `advance_projectiles` moves the rounds.

This is why AC02 holds through the cadence: a disabled mount is refused by the
resolver per mount, so it is absent from `accepted`, so `fire` spawns no round
for it and consumes no round. An enabled sibling mount in the same bank still
fires.

### The wind conversion is shared, never re-derived

The only subtraction in the whole path is inside
`air_relative_velocity_m_s` at spawn, and the world frame is rebuilt only
through `world_velocity_from_air_m_s` each tick. `guns.rs` writes no wind
arithmetic of its own, which is the requirement
`docs/findings/2026-09-30-f19-wind-conversion-ownership.md` records for any
later velocity consumer. The gust acceptance test pins both directions: a
round spawned in still air and then flown through a side gust moves with the
gust, which a round that baked its world velocity at spawn would not.

## The `cs_app` half: live mounts and live parts

`cs_app::weapons` turns live ECS state into the runtime geometry the resolver
and the sweep consume.

* `live_mount_transforms(world, actor, generation)` selects the actor root by
  the matching [`WeaponActorBinding`] (the same generation-stamped record F27-A
  defined) and reads every descendant [`MountPoseBinding`] into a
  `DamageNodeKey -> MountTransform` map. The origin is the node's composed
  `NodeVisualTransform` translation, the forward is its `-Z` axis (the
  canonical forward of `FLIGHT-PHYSICS`, "Coordinate convention"), and the
  inherited velocity is the first `LinearVelocity` found walking from the mount
  up to the root — the airframe body's live velocity. That is F27
  non-negotiable 2 taken literally: the pose comes from the live hierarchy and
  the damage state (the mount key is the same identity F29 disables), never
  from a fixed center-screen origin.
* `part_sweep_candidates(world, dt_s, relation)` reads every [`PartSweptBox`]
  entity into a `SweepCandidate`: the centre is the part's composed
  `NodeVisualTransform`, the previous centre is that less the airframe body's
  live `LinearVelocity` over the tick, and the relation comes from the
  declared allegiance vocabulary. It is the live producer of the candidates
  `Ballistics::sweep` tests (F27 non-negotiable 3).

Both are pure `&World` reads and both report a refusal **by name** for every
node they could not read (stale generation, no pose, non-finite or zero
geometry, no reachable airframe velocity, a box the shared geometry vocabulary
refuses). Nothing is dropped silently: a mount with no readable pose is named
here and refused again by the resolver as `MissingMountTransform`, so a gun
never fires from the world origin because a walk quietly skipped its mount, and
one bad part never drops the rest of a round's candidates.

## What this stage does not do (F27-C's application half)

There is **no** ECS system, schedule registration, Avian projectile body or
collider, muzzle-effect render, sound trigger or cockpit bank-selection input
here. `live_mount_transforms` and `part_sweep_candidates` are the producers
F27-C's systems will call; the Avian body that mirrors an authoritative round
and the effects/audio consumers are F27-C's, exactly as the F27-C finding's
"Not claimed" section assigns them
(`docs/findings/2026-10-02-f27-c-candidate-filtering-and-hit-damage-routing.md`).
The projectile's authoritative position lives in `cs_sim`; an ECS body is a
mirror, not a second integrator.

## Unknowns recorded (not guessed)

- **The original's per-part collision geometry and part-to-node mapping.** The
  part provider consumes whatever declared boxes the live world carries; where
  the original's part shapes are unmeasured it invents none. The F27-C finding
  already assigns the measurement to F27-D.
- **The angular contribution to a part's motion within a tick.** The provider
  reconstructs the previous centre from the airframe body's *linear* velocity
  only. Whether the original's parts move enough between ticks for the angular
  term to matter, and what the original's mount-pose/inheritance rules were,
  are cases for F27-D.
- **Convergence, gravity, drag and wind shear.** None are applied. The F27
  research boundary leaves them unmeasured, and F19-B deliberately modelled
  none of them; F27-D owns the measurement.
- **Every numeric value here** (caliber, cadence, muzzle velocity, lifetime,
  spread, damage) is the synthetic fixture's own, carried unchanged. No
  original ammo/loadout pair is claimed.

None of these is a new task: the spec already assigns the first, second and
fourth to F27-D and the effects/audio/body wiring to F27-C, and the F27-C
finding names the same owners. Recording them here keeps the stage honest
without queueing duplicate work.

## Test sensitivity (measured, one mutation at a time)

Seven mutations were applied to the production code on the submitted
signatures and the two new test binaries re-run after each (plain
`cargo test -p … --test …` is enough here because the two mutated binaries are
the only ones the tests live in):

| mutation | caught by |
| --- | --- |
| `GunCadence::fire` no longer spawns accepted shots | 4 cs_sim tests + `accept_f27_b_a_disabled_wing_gun_emits_nothing_through_the_live_read` (5 total) |
| `ProjectileRuntime::advance` ignores the wind (uses air velocity) | `accept_f27_b_a_round_carries_its_air_velocity_through_a_gust` |
| `ProjectileRuntime::advance` never retires a spent round | `accept_f27_b_a_round_retires_after_its_declared_lifetime` |
| `live_mount_transforms` ignores the stale-generation gate | `accept_f27_b_unreadable_mounts_are_refused_by_name` |
| `part_sweep_candidates` uses the current centre for the previous centre | `accept_f27_b_parts_become_swept_candidates_with_relative_motion` |
| `part_sweep_candidates` drops a refused box instead of reporting it | `accept_f27_b_unreadable_parts_are_refused_without_losing_the_rest` |
| `live_mount_transforms` hardcodes the inherited velocity to zero | `accept_f27_b_the_live_mount_inherits_the_airframe_velocity` |

The disabled-mount gate itself is F27-A's (`FireResolver`) and is exercised
unchanged through the cadence by both AC02 tests; this stage adds no second
gate.

## Review notes (2026-10-02, reviewing agent deepseek-1)

The implementation and its 12 tests were reviewed against the F27-B section,
the contract and this record. Two defects were found and fixed; the rest of
the stage stands.

1. **`part_sweep_candidates` silently substituted a zero airframe velocity.**
   The provider computed `airframe_velocity(world, entity).unwrap_or([0.0; 3])`,
   so a part with no reachable `LinearVelocity` was treated as stationary. That
   contradicts this file's own "nothing is dropped silently ... no reachable
   airframe velocity" claim and the project's "unknown means unknown" rule: an
   unmeasurable relative motion is not the same statement as a still target,
   and the sibling `live_mount_transforms` already refuses the identical case
   (`MountPoseRefusal::MissingAirframeVelocity`). It now reports
   `PartSweepRefusal::MissingAirframeVelocity { node }` and fabricates no
   candidate. New test:
   `accept_f27_b_a_part_with_no_airframe_velocity_is_refused` (cs_app).
2. **`GunCadence::fire` could change state on a fire it refused.** The wind was
   only checked inside `ProjectileRuntime::spawn`, which runs *after*
   `FireResolver::resolve` had already consumed a round and started a cooldown,
   so a non-finite wind returned `Err(CadenceRefusal::Projectile)` with the
   round already spent and the intent already marked resolved (unretryable).
   `fire` now validates the caller-supplied wind with the same `check_finite_wind`
   before resolving, so the refusal is state-free and the intent is retryable.
   New test: `accept_f27_b_a_refused_fire_changes_no_state` (cs_sim).

Both fixes are small and local to the owner paths. The mutation table above
predates them; the two new tests are themselves sensitivity cases (removing the
missing-velocity refusal makes the first fail; moving the wind check back below
`resolve` makes the second fail).

## Not claimed

No original-data verification, no ECS system or schedule wiring, no Avian
projectile body, no muzzle-effect or audio consumer, no bank-selection input,
no convergence/drag/gravity/ricochet/penetration model. This stage awards at
most **checked**; F27-D and the owner's evidence gate the rest.
