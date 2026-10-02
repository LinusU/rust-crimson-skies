# Task #425: one engine mesh asset per named mesh, shared by the objects that name it

Date: 2026-10-02. Task #425, key `F18-B-followup-shared-mesh-asset`,
"Share one engine mesh asset between world objects that name the same mesh",
following #422 (`F18-B-followup-mesh-source-from-catalog`). Branch:
`rally/425-share-one-engine-mesh-asset-between-worl`. Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary build/test
only — `CS_GAME_DIR` was **not read** for this change, so nothing here is
`verified_original` and no evidence report is required.

## What was actually wrong

The **source** already de-duplicated. #422 made `WorldMeshes` a
`BTreeMap<ContentId, WorldMesh>`, so two object records naming one mesh resolve
to one `WorldMesh`: one upload, one fingerprint, one triangle count, and
`import::accept_f18_b_a_mesh_collision_is_the_geometry_the_object_draws_and_keeps_its_opening`
already read both halves off it.

The **engine** asset did not. In `crates/cs_app/src/world/spawn.rs`:

* `spawn_mesh_presentation` called `resource_mut::<Assets<Mesh>>().add(mesh)`
  with `mesh: Mesh` cloned from `upload.mesh()`;
* `spawn_mesh_collider` passed `upload.upload.mesh().clone()` into
  `crate::asset_stack::spawn_static_mesh_collider_on_body`, which added it again.

So N objects naming one mesh put N byte-identical copies of the same geometry
into `Assets<Mesh>`, and the "one asset handle behind both consumers" claim in
`spawn_mesh_collider`'s doc comment was true **per object** and not across them.
For the harbor fixture — one record per mesh — this was invisible; the copies
only accumulate when two records name one reference, which no fixture did.

## The change

`WorldMeshAssets` (a `Resource` in `spawn.rs`) records one strong
`Handle<Mesh>` per **(`ContentId`, `ContentHash`)**, and `shared_mesh` is now the
only place in the world path that puts a mesh into `Assets<Mesh>`. Every
consumer — presentation-only, solid, trigger — asks it for the handle, so four
records naming one stored mesh hold one strong handle and one asset.

**The fingerprint is part of the key on purpose.** A `ContentId` says *which*
mesh; the fingerprint says *which upload of it*. A caller that registers
different geometry under a reference it used before (`WorldMeshes::insert_render_mesh`
replacing an entry) must get a new asset, or the engine would present the
previous load's geometry. Keyed on the id alone, the cache would hand back stale
geometry silently.

### The solid mesh layout moved into `spawn.rs`

`spawn_mesh_body` is the collider-on-body bundle, written out in this module
because `asset_stack::spawn_static_mesh_collider_on_body` takes a `Mesh` and
uploads it itself, which is exactly what a shared handle forbids.
`asset_stack` is F00-A's path and **was not edited**; the new function has the
same components in the same order and derives layers through the same
`avian_layers` (`asset_stack` imports that function from here, so there is one
derivation). Adding a handle-taking variant to `asset_stack` instead would have
been a second place to keep in step with this one for no behaviour the crate did
not already have.

`spawn_mesh_trigger_volume` was already written here (task #401's decision), so
after this change both mesh layouts live in this module and
`accept_f18_b_the_trigger_and_solid_mesh_paths_differ_only_in_the_body` continues
to hold them against each other. The `#[track_assets]` mechanics of Bevy's
refcount are untouched: nothing else in the crate adds world meshes.

## The owning-handle decision, written down

**The loader keeps an owning handle, and it is released with the world.**
`WorldMeshAssets` holds the strong handles, and `residency::unload_world`
removes the resource. After the world's entities have been despawned their
`Mesh3d` components are gone, so no strong handle is left and Bevy's
`Assets::<Mesh>::track_assets` frees the assets on the next `PreUpdate`.

The alternatives were considered:

* **Entities only, no loader-owned handle.** Every `Mesh3d` would be the sole
  owner, and an object that streamed away and came back would re-upload the same
  triangles on every sector round trip. It would also make the engine asset
  count depend on which sectors happen to be loaded, which is a property no
  consumer asked for.
* **Never release.** The geometry of a finished mission stays resident in the
  engine for the rest of the process. F18 non-negotiable behavior 5 says a load
  starts from nothing; a leaked asset contradicts the same rule one layer down.

A **sector** unload deliberately releases nothing. The source the assets were
built from is caller-owned and passed to `load_world`/`load_sector` by shared
reference, so it outlives every sector (#422 recorded this); per-sector release
is a streaming decision, and the policy that would own it
(`visibility::update_visibility`) does not own the assets. Recorded rather than
guessed, and it is a limitation for F18-C's streaming work, not a claim about the
original.

## The twin harbor fixture

`fixture::twin_harbor_world` and `twin_harbor_meshes` exist because the existing
fixtures cannot express the bug: the arch world is all cuboids, and the harbor
world has exactly one record per mesh. The twin world has six records:

| record | role | shape | mesh |
| --- | --- | --- | --- |
| `shell.stand_a` | `Solid` | `FromMesh` | the arch shell (36 triangles, 3 groups) |
| `shell.stand_b` | `Solid` | `FromMesh` | **the same** shell |
| `banner.twin` | `None` | `FromMesh` | **the same** shell |
| `trigger.twin` | `Sensor` | `FromMesh` | **the same** shell |
| `panel.solo` | `Solid` | `FromMesh` | a panel of its own (12 triangles) |
| `terrain.twin` | `Solid` | `Cuboid` | named, deliberately unresolved |

Four records on one reference covers all four consumers the mesh path has, so a
shared-asset claim that reached only the solid path would fail. The fifth names a
different mesh, because "one asset per mesh" is also satisfied by "one asset for
the whole world". The sixth is a cuboid whose mesh reference resolves nothing, so
the asset count must not move when it spawns. The two shells are 100 m apart, so
they are visibly two objects and not one drawn twice. The shell is the same
stored mesh `harbor_meshes` builds, so a triangle-count assertion means the
*merged* mesh is what is shared, not one material group of it.

## Files

* `crates/cs_app/src/world/spawn.rs` (edited): `WorldMeshAssets`,
  `SharedWorldMesh`, `shared_mesh`, `spawn_mesh_body`; `spawn_mesh_presentation`,
  `spawn_mesh_collider` and `spawn_mesh_trigger_volume` now take a handle instead
  of a `Mesh`; module docs for the sharing and the owning-handle decision.
* `crates/cs_app/src/world/residency.rs` (edited): `unload_world` removes
  `WorldMeshAssets`.
* `crates/cs_app/src/world/fixture.rs` (edited): the twin harbor world, its mesh
  source, and the constants.
* `crates/cs_app/src/world/mod.rs` (wiring): re-exports `WorldMeshAssets`,
  `twin_harbor_world`, `twin_harbor_meshes` and the twin constants; the `spawn`
  bullet names the shared handle.
* `crates/cs_app/tests/world/shared_asset.rs` (new): four tests.
* `crates/cs_app/tests/world/main.rs` (wiring): `mod shared_asset`.
* This file.

## Test and mutation matrix

`cargo test -p cs_app --test world -- accept_f18_b_` — 42 tests, all pass (the
three above plus the new replacement test; none is `#[ignore]`d, so
`--include-ignored` runs the same 42).

The mutation that matters is the one that removes the behaviour: deleting the
cache lookup in `shared_mesh` (so every spawn re-adds a mesh) was applied, the
`accept_f18_b_` selection was run, and the source was restored. Result:

| mutation | tests that fail |
| --- | --- |
| `shared_mesh` always adds a fresh asset (cache lookup removed) | `shared_asset::..._records_naming_one_mesh_share_one_engine_asset_and_others_do_not`, `shared_asset::..._unloading_a_world_releases_the_shared_mesh_assets_and_a_reload_rebuilds_them` (2) |
| `unload_world` stops removing `WorldMeshAssets` | `shared_asset::..._unloading_a_world_releases_the_shared_mesh_assets_and_a_reload_rebuilds_them` (1) |
| the cache keys on the reference alone, ignoring the fingerprint | `shared_asset::..._a_replaced_upload_under_one_reference_gets_its_own_engine_asset` (1) |

Each mutation was applied, the `accept_f18_b_` selection was run, and the source
was restored. The first is the one that matters: the other 39 `accept_f18_b_`
tests stay green under it, which is the measurement worth recording — the
pre-existing suite could not see this bug at all, because no fixture had two
records on one reference. The new tests fail with `asset_count` 4 instead of 2
(three copies of the shell plus the panel), which is the whole claim.

The four new tests:

* `accept_f18_b_records_naming_one_mesh_share_one_engine_asset_and_others_do_not`
  — one asset and one handle across all four mesh layouts, a different asset for
  the panel, no asset for the cuboid, the shared asset carrying all 36 triangles,
  every derived trimesh still carrying its own upload's triangles, the banner
  still collideless, the trigger still body-less and marked, the two solids still
  two entities 100 m apart, and all four records reporting the same reference.
* `accept_f18_b_a_world_with_one_record_per_mesh_keeps_one_asset_each_and_every_triangle`
  — the harbor world, where there is nothing to share: four records, four
  references, four assets, and the arch's opening still 36 triangles and not the
  12 of a hull of the same corners.
* `accept_f18_b_unloading_a_world_releases_the_shared_mesh_assets_and_a_reload_rebuilds_them`
  — the reverse: load, settle, assert two assets; `unload_world`, assert the
  loader's record is gone and the engine frees both assets after the settle
  updates; reload, assert two assets shared the same way and 36 triangles.
* `accept_f18_b_a_replaced_upload_under_one_reference_gets_its_own_engine_asset`
  — the reason the fingerprint is in the key: spawn the twin world, register a
  **box** under the shell's own reference through
  `WorldMeshes::insert_render_mesh`, spawn again in the same Bevy world, and
  require three assets, the first spawn's objects still holding the 36-triangle
  shell, and the second spawn's objects presenting and colliding from the box's
  twelve.

## Evidence

Ordinary build/test only; no `CS_GAME_DIR` read, so no evidence report. Nothing
here is `verified_original`: the twin world is `Origin::SyntheticFixture`, and
this change is asset bookkeeping over geometry no original data contributed.

## Sources

No external sources. The record shapes follow `docs/contracts/IDENTITY-CONTENT.md`
and F18-B; the multi-group upload is #422
(`docs/findings/2026-09-30-f18-b-followup-mesh-source-from-catalog.md`, whose
"Whether a mesh may be shared by two objects, and uploaded once or twice" this
closes — that bullet is now resolved rather than outstanding); the shared-mesh
motivation and the release question are the same document's two other bullets,
the second of which ("per-sector release … is a streaming decision") is still
open and still F18-C's. The collider-on-body rule and the `asset_stack` helper
are F00-A's, measured in `docs/findings/2026-09-30-t420-mesh-ccd-decision.md`;
the body-less trigger layout is task #401's, in
`docs/findings/2026-10-02-t401-trigger-volume-and-swept-ccd.md`. Bevy's
refcount semantics (`Assets::<A>::track_assets` in `bevy_asset 0.19.1`) are the
pinned dependency's own.