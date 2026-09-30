# F18-B: world import and static collision generation

Date: 2026-09-30. Task: F18-B "Implement world import and static collision
generation" (`specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
section `### F18-B`), acceptance scenario **AC02**. Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary build/test
only — no `CS_GAME_DIR` read, no evidence report required, nothing
`verified_original`.

## Files and the one observable failure (the slice plan)

* `crates/cs_content/src/world.rs` (edited): `WorldObjectCondition` (the two
  values an object's condition can take, for the load that owns it),
  `WorldInstance::initial_condition`, and
  `WorldError::DamagedObjectNotActivated` — a load that authors damage for an
  object its own population never activates is refused by name.
* `crates/cs_app/src/world/meshes.rs` (new): `WorldMesh` (one F17-B upload plus
  its fingerprint and triangle count) and `WorldMeshes` (the one place a mesh
  reference meets an upload).
* `crates/cs_app/src/world/spawn.rs` (edited): `spawn_object` is now the unit
  both the whole-world spawn and the sector load use; a `FromMesh` object is
  presented **and** collided by one node holding one `Mesh3d` handle;
  `SkipReason::MeshUnavailable` replaces the F18-A `MeshColliderDeferred`;
  `SpawnedWorld` is now a list of `SpawnedObject` with per-object accessors.
* `crates/cs_app/src/world/residency.rs` (new): `load_world`, `load_sector`,
  `unload_sector`, `unload_world`, `damage_object`, `WorldResidency`,
  `ResidentWorld`, `ObjectCondition`, `WorldLoadError`.
* `crates/cs_app/src/world/contacts.rs` (edited): the binding and every
  `WorldContact` now carry the object's `Resolved<SurfaceRole>`, so a contact
  says which gameplay surface rule it follows.
* `crates/cs_app/src/world/fixture.rs` (edited): `world_app` (the one headless
  composition, with the contact recorder in it), the mesh-authored
  `harbor_world`, its `harbor_meshes`, `world_instance`, `MESH_SETTLE_UPDATES`,
  and post-settle probe spawns on `WorldFixture`.
* `crates/cs_app/src/world/mod.rs`, `crates/cs_app/src/lib.rs` (wiring and
  module docs only).
* `crates/cs_app/tests/world/{main,import,residency}.rs` (new/edited): the
  acceptance tests, `accept_f18_b_*` (nineteen after review; seventeen when the
  stage was first handed over).
* This file.

**One observable failure:** the objective is damaged, its sector is unloaded,
it is loaded again, and the objective comes back **sound** — because the
condition lived on the entities the unload destroyed, or because the reload
re-read the record instead of the load's own memory. That is
`residency::accept_f18_b_a_damaged_object_survives_a_sector_unload_and_reload`,
and its damage is deliberately inflicted *while the sector is unloaded*, which
is the only version a respawn cannot fake.

## What this stage owns, and what "import" means here

F18-B is the **load** path: a `WorldDefinition` plus a `WorldInstance` plus a
mesh source become engine entities, and one sector's geometry can be moved in
and out while the load's per-object state survives. It is not a container
parser. Which GameZ member holds a world group, how a world is partitioned into
sectors in the source, and what per-object collision role the original stores
are all **unmeasured** (recorded below), so an importer that guessed them would
be inventing the very records F18-D has to measure. `WorldDefinition::try_new`
stays the record constructor, the load is the conversion, and the retail import
that fills those records is F18-B/D with `retail`.

Simplification policy: **none**, deliberately. A `FromMesh` object is collided
by `ColliderConstructor::TrimeshFromMesh` (the one constructor that keeps every
stored triangle), the upload is shared with the presentation by handle, and the
report states the upload's fingerprint and triangle count so a substituted shape
is visible rather than plausible. "We performed no simplification" is a claim
that needs a measurement to be replaced, which is why the triangle count is
asserted rather than assumed.

## Measured behavior on the pinned pair

`bevy 0.19.1` / `avian3d 0.7.0` / `parry3d 0.27.0`, `SubstepCount(1)`, 120 Hz
fixed rate, gravity zero. Probe: 0.5 m box. "Swept" = `SweptCcd` with
`SpeculativeMargin::ZERO` (the F18-A probe).

* **A mesh-derived collider keeps the stored geometry exactly.** The harbor
  shell is three boxes welded into one stored polygon soup: 24 corners, 18 quad
  faces, **36 triangles**. Avian's derived trimesh has 36 triangles (a convex
  hull of the same corners has 12), and **20** vertices: the four corners the
  lintel and the legs share are merged, and no triangle is deleted — the same
  merge the t333 fixture measured. Every derived vertex is bit-identical to an
  uploaded position.
* **The arch's opening is in the mesh, and it stays open.** A body flying the
  opening's centreline at 30 m/s from `x = -12` passes `x = 8` with an empty
  contact log and unchanged velocity; the same body at `z = 1.5` is stopped at
  `x = 0.056` with a `CollisionStart` naming `objective.hangar` with role
  `Solid` and surface `Ground`. A hull of the same corners would have stopped
  the first body too.
* **Roles are honoured on the mesh path.** The `Sensor` trigger volume reports
  a *discrete* body that crosses it and does not slow it by more than 0.01 m
  (measured drift 0.0 m at 30 m/s); the `None` banner carries a `Mesh3d` and no
  collider, no body and no `RigidBodyColliders`.
* **Water is the authored patch.** A body on the patch's own line is reported
  against `water.patch` with surface `Water`; a body 20 m beside it in `z` flies
  the whole way with an empty log. Nothing about the water object's collision
  is unbounded: it is a 8×0.4×8 m slab mesh with a bounded `ColliderAabb`.
* **The contact carries the surface rule.** `WorldContact::surface` is the
  record's own `Resolved<SurfaceRole>`: `Water` for the patch, `Ground` for the
  hangar's leg, and the sectors the touched object belongs to (empty for the
  resident water patch). F18-A's contacts had no surface at all.
* **The residency rule is the record's own.** Unloading `yard` despawns
  `objective.hangar` and `trigger.sensor` and *nothing else*: `terrain.ground`
  belongs to `approach` as well, and `water.patch` names no sector, so both stay.
  Two unload/reload round trips return the world to exactly its six objects with
  no duplicates, and `ResidentWorld::present_objects()` is the list a consumer
  can ask.
* **A second load is refused, not merged.** `load_world` over a resident world
  returns `WorldAlreadyResident { resident, requested }` and changes nothing,
  including the first load's authored damage. After `unload_world` the
  residency resource is gone and a new load starts from nothing, so a narrower
  second mission neither inherits the first's objects nor its conditions.
* **A missing upload is reported, never faked, and the two ways of missing one
  are different reasons.** `strip.absent_mesh` names a mesh the source does not
  hold: it is presented, appears in `SpawnedWorld::skipped()` as
  `SkipReason::MeshUnavailable`, has no `Collider`, and takes no other object's
  collision with it. A `FromMesh` object whose mesh *reference* is an explicit
  unknown is a different fact — a content gap, the same class as an unknown role
  or shape, which a retail import fills with evidence — and is reported as
  `SkipReason::UnknownMesh`. Collapsing the two would make "we did not load it"
  look like "we do not know what it is".

## The engine limitation this stage found and did not paper over

**Avian's swept CCD does not stop a body against a `TrimeshFromMesh` collider,
while it does against a cuboid.** Measured on the same probe, same speed, same
fixture geometry:

| body | arch wall | result |
| --- | --- | --- |
| swept, 400 m/s | `objective.hangar` (trimesh) | passes through, no contact |
| swept, 400 m/s | `arch.leg_right` (cuboid, F18-A world) | clamped at the wall, contact recorded |
| discrete, 30 m/s | `objective.hangar` (trimesh) | stopped, contact recorded |
| swept, 30 m/s | `objective.hangar` (trimesh) | stopped, contact recorded |

Cause, read in the pinned sources: Avian's swept CCD asks parry for a shape
cast (`avian3d-0.7.0/src/dynamics/ccd/mod.rs::compute_ccd_toi`, ~line 690) and
parry's `DefaultQueryDispatcher::cast_shapes`
(`parry3d-0.27.0/src/query/default_query_dispatcher.rs:437-541`) has cases for
ball, half-space, heightfield, support-map, composite and voxels — and **none
for `TriMesh`**, so the call returns `Err(Unsupported)` and no time of impact is
ever produced. A cuboid is a support map, which is why F18-A's 400 m/s arch test
clamps at `x = -0.7497`.

* Affected content: every world object whose collision is mesh-derived and thin
  relative to one tick, at any body speed above the discrete sampling rate —
  i.e. the whole of retail world geometry once F18-D imports it, and the F23
  aircraft bodies that carry `SweptCcd`. This is a **hard gap for the retail
  world**, not a fixture artifact.
* Resolving task: filed by this stage as a follow-up (see "Follow-ups" below).
  It is not solvable inside `cs_app` on the pinned pair: neither a different
  `ColliderConstructor` nor more substeps is a decision this stage may take
  silently, and inventing geometry for the miss is exactly what F18
  non-negotiable behavior 1 forbids.
* Pinned by `import::accept_f18_b_a_tunnelling_body_misses_mesh_geometry_which_is_a_pinned_engine_limit`,
  which asserts the *current* behaviour and its precondition (a tick longer than
  probe-plus-wall) and compares it with the cuboid path in the same test. It is
  expected to fail when the pinned engine learns to cast against a triangle
  mesh; that failure is the signal to re-measure the limitation, not a bug.

The F18-A measured interaction between swept CCD and *sensor* volumes
(task #401) is unchanged and still applies to mesh-derived trigger volumes.

## Other limitations this stage met

* **An authored matrix that no runtime transform can hold is still refused
  whole.** F18-A's review named "mesh colliders may follow the render path's
  full affine" as F18-B's answer for real geometry; it is **not** implemented
  here, because the *presented* half of the object needs the same decomposition
  (`Transform` is translation/rotation/scale) and F18-A's `NodeVisualTransform`
  refuses a shear the same way. Affected content: any retail world object whose
  authored matrix is sheared. Resolving task: filed as a follow-up; discovered
  for real by F18-D.
* **No simplification policy exists for retail geometry.** None is needed for
  exactness (a trimesh is exact, including under a per-axis scale, measured in
  #333), but any future decimation must record the source mesh, the operation
  and the openings it closes, because the report is what a consumer checks.
* **The mesh source is a map, not a catalog.** `WorldMeshes` is filled by the
  caller; a retail source fills it from `cs_content::mesh::MeshCatalog` through
  the same F17-B upload adapter. How a world group streams meshes in and out of
  that map, and per-sector, is F18-D's question.
* **Stored mesh units are unmeasured.** `cs_content::mesh` applies no scale to
  vertex positions and the original's world-vertex scale is unknown, so the
  fixtures author their geometry in metres *by construction* and say so. Any
  retail import must measure the scale before a position and an authored
  transform can be compared; F18-D.
* **The contact recorder moved into the app composition.** F18-A's
  `spawn_world` installed `WorldPlugin` on first call; a world load happens
  *after* `App::finish` in a real mission, where `add_plugins` panics, so the
  recorder is now added by `fixture::world_app` before the app is finished. An
  app that runs a world without it records no contacts — a gap in that
  composition rather than something a load can repair.
* **The F18-A fixtures are unchanged in content** (`arch_world`, its nine
  objects and its dimensions); `spawn_world` only gained the mesh-source
  argument, and every `accept_f18_a_*` test still passes unmodified except the
  one call that passes the new argument and one report lookup.

## Test sensitivity (mutation matrix)

Every mutation below was applied, `cargo test -p cs_app --test world --
accept_f18_b_` was run, and the source was restored. All seventeen tests pass
unmutated.

| mutation | tests that failed |
| --- | --- |
| a `FromMesh` object never gets a collider | 12: every import test plus `..._a_damaged_object_survives_...` |
| every mesh collider is marked a `Sensor` | `..._a_mesh_role_solid_stops_a_body_and_sensor_only_reports_one`, `..._a_swept_body_flies_through_the_mesh_opening_and_is_stopped_by_its_leg` |
| residency ignores sector membership | `..._a_damaged_object_survives_...`, `..._reloading_a_sector_keeps_one_entity_per_object_...`, `..._every_load_refusal_names_what_it_refused...` |
| a load seeds no authored condition | `..._another_mission_loads_its_own_population_and_damage...`, `..._a_second_load_over_a_resident_world_is_refused...`, `..._every_entity_of_a_mesh_object_carries_its_condition` |
| a second load merges into the resident one | `..._a_second_load_over_a_resident_world_is_refused_rather_than_merged` |
| damage reaches the entities but not the record | `..._a_damaged_object_survives_a_sector_unload_and_reload` |
| a contact drops its surface rule | `..._a_contact_names_the_surface_rule...`, `..._water_collision_is_the_authored_patch...` |
| an unknown mesh reference is reported as a missing upload | `..._an_object_whose_mesh_is_missing_is_reported_and_never_faked` |

The record-level additions (`WorldObjectCondition::initial_condition`,
`DamagedObjectNotActivated`) are covered by the residency and import tests that
call them; the fourteen F18-A tests are the F18-A matrix, unchanged.

## Review findings (2026-09-30, reviewer `bunny-alpha-1`)

The implementer and the reviewer are the same agent, so this is **not**
independent evidence. What was checked, and what it changed:

* **A hole in the mutation matrix: the despawn half of the transaction was
  untested.** Every residency assertion read the *record*, which is a claim
  about the world rather than the world itself. Replacing `despawn_all` with a
  no-op passed all seventeen tests: an unload could have despawned nothing, and
  nothing in the suite would have said so. `residency::
  accept_f18_b_an_unload_really_despawns_the_objects_entities_and_a_reload_restores_them`
  now reads the Bevy world itself (through the binding every entity an object
  owns carries) and fails on that mutation.
* **A mesh object's body carried no binding.** `WorldObjectBinding` documents
  itself as cloned "onto every entity the object owns", and the report, the
  despawn and the condition stamp all treat the body as owned — but only the
  node carried the binding, so a query starting at a body could not name its
  object. The body now carries the same binding, and deliberately no
  `WorldColliderInstance`: a contact must still resolve to the one entity that
  has the collider. Removing the body's binding fails the test above.
* **`load_sector`'s abort path destroyed objects it had not spawned.** It
  despawned *every* present object while leaving the residency record claiming
  they were present, which is the corrupt state this module exists to prevent.
  `rollback` now takes back exactly what the call spawned (entities and record
  entries) and leaves everything already present alone. The path is unreachable
  after the up-front transform check, so no test can reach it; the fix is
  because the code contradicted its own documentation.
* **`damage_object` changed the record before it could still fail.** The module
  claims every call "decides everything before it changes anything"; this one
  inserted the condition and only then discovered a vanished entity. It now
  checks the population and every entity first, so a refusal leaves the
  condition exactly as it was.
* **A non-colliding object with no geometry was reported nowhere.** A record
  whose role is `None` declines no collider, so `skipped` was empty, and a
  banner whose mesh nobody supplied was presented as a bare marker that draws
  nothing while the report claimed the world was complete.
  `SpawnedObject::presentation_gap` / `SpawnedWorld::presentation_gaps` report
  that half of the same gap, once per object, and
  `import::accept_f18_b_a_non_colliding_object_with_no_geometry_is_reported_as_a_presentation_gap`
  pins it.
* **A test message named the wrong colliders.**
  `..._every_mesh_collider_carries_the_designed_static_world_layers` counted four
  colliders and attributed them to "hangar, sensor, banner, water" — the banner
  has role `None` and no collider, and the fourth is the *cuboid* ground slab.
  The count is now derived from the objects themselves, so a substitution cannot
  pass on a number.
* **Verified rather than taken on trust:** parry 0.27.0's
  `DefaultQueryDispatcher::cast_shapes` (`default_query_dispatcher.rs:437-541`)
  really has no `TriMesh` case and really returns `Err(Unsupported)`, and
  Avian's `compute_ccd_toi` (`avian3d-0.7.0/src/dynamics/ccd/mod.rs:692-705`)
  really folds that into `None`. The pinned limitation the flight tests measure
  is the engine's, not a fixture artifact.


## Designed vocabulary, not original data

Designed here: `WorldObjectCondition` (two values), the `ObjectCondition`
component, the residency rule and its refusals, the harbor world and its
geometry, the mesh source map, the "one node presents and collides a mesh
object" entity layout, and the choice to perform **no** simplification. None of
it is claimed to be the original's vocabulary or behaviour.

Unknown, and not guessed:

* **How the original stores world geometry per sector**, how a sector or an
  object instance is identified in the source, and whether it stores a
  per-object collision role at all. `WorldCollisionRole`'s three values stay a
  designed vocabulary. F18-B/D.
* **The original's world-vertex unit scale** and its coordinate handedness
  relative to `CanonicalTransform`. F18-B/D.
* **Which gameplay surface classes the original distinguishes** and what rule
  each carries. `SurfaceRole` is still the two the spec names.
* **The original's floor, ceiling and world-boundary rules.** `WorldBoundary`
  still documents "no rule" rather than a wall.
* **Whether a damaged world object is destroyed, burning or merely dented.**
  `WorldObjectCondition::Damaged` is the smallest value that distinguishes "the
  record authored this" from "this happened since"; the rules that would move
  an object *into* it are `cs_content::damage` and `cs_sim::damage` (F29), not
  this stage.
* **Whether retail aircraft bodies run continuous detection.** The fixture
  proves what the *engine* does, not what the original needed (F23-D, F18-D).
* **`ContentKind` has no `sector` or `world_object` namespace** (unchanged from
  F18-A; a `cs_types` change outside this task's owner paths).

## Evidence

Ordinary build/test only; no `CS_GAME_DIR` read and no evidence report is
required for this stage. Commands run locally:

```sh
cargo fmt --all -- --check                                        # exit 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # exit 0
cargo test --workspace --locked                                   # exit 0
cargo test --workspace --locked -- accept_f18_b_ --include-ignored
#   17 tests run, 17 passed (crates/cs_app/tests/world)
```

Nothing here is `verified_original`: no original world group was visited and no
original byte was read.

## Sources

No external sources were consulted. The record shapes follow
`docs/contracts/IDENTITY-CONTENT.md` and F18-A
(`docs/findings/2026-09-30-f18-a-world-instances-sectors-and-collision-roles.md`);
the upload adapter and its fingerprint are F17-B
(`crates/cs_app/src/render/bevy_mesh.rs`); the asset stack and its
collider-from-mesh constraint are #333
(`docs/findings/2026-09-30-t333-real-asset-stack-for-collider-from-mesh.md`);
the layer vocabulary is F23-A
(`docs/findings/2026-09-29-f23-a-avian-schedule-adapter-and-collision-layers.md`).
Every Avian, parry and Bevy statement above was read from the pinned sources in
the local cargo registry and then *measured* by running the fixture.
