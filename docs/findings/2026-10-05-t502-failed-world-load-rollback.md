# Task #502: a failed world load gives back its entities and its mesh assets

Date: 2026-10-05. Task #502, key `F18-residency-failed-world-load-rollback`,
following #425 (`F18-B-followup-shared-mesh-asset`). Branch:
`rally/502-roll-a-failed-world-load-back-so-it-leav`. Capabilities used:
ordinary build/test only. `CS_GAME_DIR` was **not read**, so nothing here is
`verified_original` and no evidence report is required.

## What was wrong

`residency::load_world` had two failure paths inside its spawn loop, and they
behaved differently:

* `spawn_object(...)?` returned with no cleanup at all;
* the `stamp` arm despawned `resident.objects`, which holds the objects
  **before** the current one. The object being stamped had already spawned its
  entities, so they survived.

Neither path released the `WorldMeshAssets` handles the spawns had taken. A
failed load inserts no `WorldResidency`, so `unload_world`, the only thing
that removes `WorldMeshAssets`, could never release them either. #425 recorded
this gap and filed it as this task.

## Which failure can actually happen part-way

The task description gives `spawn_object` refusing a record (`UnplaceableAffine`)
as the example. **That refusal cannot happen after the first spawn.**
`load_world` calls `instance_placements` over the whole population before
spawning anything. `spawn_object` refuses only through `instance_placement` and
`AffinePlacement::bake` for a cuboid, which are the same functions on the same
record. So a record `spawn_object` would refuse is refused by the pre-flight, and
nothing has spawned at that point. The `spawn_object` arm still goes through the
rollback, because the pre-flight being complete is a promise this code relies on
but does not check itself.

The refusal that *does* come after objects exist is
`WorldLoadError::VanishedEntity`. It happens when something outside the
transaction despawns an entity of the object being stamped. The tests produce
that failure on purpose, with an observer on `ObjectCondition`. When the load
stamps the first entity of a two-entity cuboid object, the observer despawns
that object's other entity. The tests do **not** despawn the entity when it
spawns: the spawn path inserts components after `World::spawn`
(`spawn_cuboid_collider` inserts `RigidBody`/`Sensor`), and Bevy panics on an
insert into a despawned entity. That would test Bevy, not the rollback. This
measured panic is why the trigger is on the stamp and not on the spawn.

Acceptance criterion 3 asks for "a record that `spawn_object` refuses". That
exact form cannot be produced through the production path, for the reason
above. The criterion's purpose, a failure on purpose after at least one object
spawned, is met with the vanished entity, and each test asserts how many
objects had spawned first.

## The change

`load_world` now has the same shape as `load_sector`. An object joins
`incoming` as soon as its entities exist, before the steps that can still fail.
Every failure goes through `abandon_world`, which:

1. calls the existing `rollback(app, &incoming, err)` to despawn this call's
   entities. Its residency branch does nothing here, because `load_world` only
   inserts the record after the last object;
2. puts `WorldMeshAssets` back exactly as the call found it. A record that was
   absent becomes absent again. A record left by something else (a direct
   `spawn_world`) is put back with its own entries, which drops only the handles
   this load added.

`load_sector` does **not** use `abandon_world`. The assets it shares belong to
the resident world, which is still loaded. `unload_world` is unchanged.

## Measurements

| test | world | objects spawned before the failure | `Assets::<Mesh>::len()` before → after the failed load (settled) | `WorldMeshAssets` after |
| --- | --- | --- | --- | --- |
| `accept_f18_b_a_world_load_that_fails_part_way_leaves_no_entities_and_no_mesh_assets` | twin harbor, fails on `terrain.twin` | 5 (`shell.stand_a`, `shell.stand_b`, `panel.solo`, `banner.twin`, `trigger.twin`) | 0 → 0 (the load uploaded 2, a clean reload then holds 2) | absent |
| `accept_f18_b_a_failed_world_load_restores_the_mesh_record_it_found` | twin spawned via `spawn_world`, then harbor fails on `terrain.ground` | 4 (hangar, sensor, banner, water) | 2 → 2 | the twin's 2 entries (was 6 without the restore) |
| `accept_f18_b_a_failed_sector_load_keeps_the_resident_world_and_its_assets` | harbor resident, both sectors unloaded, `yard` fails on `terrain.ground` | 1 (hangar) | unchanged | unchanged, 4 entries |

## Mutation matrix

Each mutation was applied, `cargo test -p cs_app --test world -- failed_load`
was run, and the source was restored.

| mutation | tests that fail |
| --- | --- |
| `load_world` back to `origin/main` (`?` plus despawn of earlier objects only) | the two world-load tests (the failed object's presentation entity survives: in the restore test, 8 world entities instead of the twin's 7) |
| `abandon_world` despawns but leaves `WorldMeshAssets` alone | the two world-load tests (`Some(2)` instead of `None`; `Some(6)` instead of `Some(2)`) |
| `abandon_world` always removes `WorldMeshAssets` | `..._restores_the_mesh_record_it_found` (`None` instead of `Some(2)`) |
| `load_sector`'s stamp failure goes through `abandon_world` | `..._a_failed_sector_load_keeps_the_resident_world_and_its_assets` (`None` instead of `Some(4)`) |
| `load_sector`'s stamp failure skips `rollback` | the same sector test (the record still claims `objective.hangar`) |

## Files

* `crates/cs_app/src/world/residency.rs` (edited): `load_world` collects
  `incoming` and routes every failure through the new `abandon_world`; docs.
* `crates/cs_app/tests/world/failed_load.rs` (new): three tests.
* `crates/cs_app/tests/world/main.rs` (wiring): `mod failed_load` and one doc
  line.
* This file.

## Sources

No external sources. The rollback shape is `load_sector`'s own (F18-B). The
owning-handle decision and the gap this task closes are in
`docs/findings/2026-10-02-t425-shared-world-mesh-asset.md`. Bevy's refcounted
release (`Assets::<A>::track_assets`, `bevy_asset 0.19.1`) and the panic on an
insert into a despawned entity (`bevy_ecs 0.19.1`,
`world/entity_access/world_mut.rs`) are behaviour of the pinned dependencies,
observed in this build.
