# #639: every world container imports and spawns, and the one the settle cannot finish is named

Date: 2026-10-05. Task: #639 "Import every world container and report which
ones the unit blocks" (`F18-WORLD-UNITS-CONTAINERS`), the follow-up #629 filed.
Feature sheet: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
stages `### F18-B` and `### F18-D`. Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: **`retail`** (read-only
access to `$CS_GAME_DIR`) and ordinary build/test. `gpu` and `audio` were
available and **not used**: nothing is rendered or played and no original run
happened, so nothing here is `verified_original`.

## Files

- `crates/cs_app/src/world/retail.rs` (extended, an F18 owner path):
  `read_world_containers` and `RetailWorldContainers` (**one** production
  discovery, every world group out of it), `RetailWorldContainer::partition_grid`,
  and the `uploaded_meshes` gap rule below.
- `crates/cs_app/src/world/mod.rs` (wiring only): the two new re-exports.
- `crates/cs_app/tests/world/world_units.rs` (new, an F18 owner path): the five
  `accept_f18_world_units_containers_` tests, all retail.
- `crates/cs_app/tests/world/main.rs` (wiring only): `mod world_units;` and the
  module note.
- `docs/findings/2026-10-04-m01-lc-world-import.md` (edited): its "only `c1c` is
  imported by a test" limitation and its `#639` follow-up entry are marked done
  and point here.
- This file.

**Rebased onto #638.** This branch was reworked on top of `F18-MESH-CATALOG-WORLD`
(#638), which landed while the work was in progress and replaced the per-container
`<group>.mesh-<index>` mesh identity with catalog elements
(`MeshId::content_id`, `mesh/<container>.<slot>`). Everything here was re-measured
and re-verified on that base: the per-group counts are unaffected (they come from
the node array and the partition grid, not from mesh identity), the `NoGeometry`
arm moved from `insert_render_mesh` to `insert_mesh_upload`, and the C5 test now
builds each expected mesh id through
[`cs_content::catalog::baseline::mesh_content_id`] — the catalog's **own** spelling
rule — so the test cannot disagree with the production id by construction.

**No reader refusal was weakened, and no test was weakened.** `git diff` touches
no line of `crates/cs_formats`, `crates/cs_types` or `crates/cs_assets`, and the
grid is still read from the same bytes `read_gamez_nodes` walked. `cs_content`
is unchanged: the one production change is in `cs_app`'s retail source, not in
the importer.

## What this task found

Running the production path over **all eight** world containers — rather than the
one #629 covered — turned up two things a single-container test cannot see.

### One: `c5` could not be loaded at all, and the reason was a hole, not a defect

`ZBD/C5/gamez.zbd` names **16** of its stored mesh slots, and **every one of those
slots decodes to an empty polygon list and an empty position list** — the store
holds a record there and no geometry. 61 of `c5`'s records name one of them.

`RetailWorldContainer::uploaded_meshes` propagated the first of those as
`WorldMeshBuildError::NoGeometry`, and because it was a `?` on the whole
container, **one empty slot aborted the entire container**: `c5` produced no
`WorldMeshes` at all, and its other 346 meshes were unreachable. That is the
opposite of reporting a gap. It destroys a world over a hole the store itself
states — and the function's own documentation already said what it should do:

> A mesh this container holds no geometry for is **not** registered: the spawn
> reports it as `SkipReason::MeshUnavailable` rather than being handed a
> substitute shape.

The code contradicted its own doc comment, and the contradiction was invisible
while only `c1c` was tested. **No other world container names an empty slot**, so
this was reachable through exactly the group the old test skipped.

The change is one match arm: `NoGeometry` is a **gap** (nothing is registered,
the affected records are reported) and every **other** `WorldMeshBuildError` still
refuses the container, because those are the adapter's refusals about geometry
that *is* there. Measured after the change: `c5` registers 346 meshes, spawns 576
objects and 367 colliders, and all 61 affected records are reported as
`UnknownCollisionRole` — they are **unindexed** records, so the role check reaches
them first and the reason is "never measured", not "a download failed".

### Two: `c3`'s colliders cannot finish building, and it is parry, not this project

This one is **not** fixed here, and is recorded as a limitation instead.

`spawn_world` accepts `c3` and returns `Ok` with 374 colliders. Then the settle
fails: Avian's mesh-derived collider reaches parry 0.27's binned BVH builder
(`bvh_binned_build.rs:58`), which computes its bin index as

```
bin = (NUM_BINS * (1 - eps) / (centroid_max - centroid_min)) * (c - centroid_min)
```

For one of `c3`'s stored meshes that centroid extent is **denormal**, the division
overflows, the index becomes `usize::MAX` and the builder indexes its 8-entry bin
array out of bounds. The panic escapes `App::update()`, so **33 of `c3`'s 374
colliders are never built**.

**The stored bytes are the cause, and the reader is faithful.** Mesh slot 447 of
`ZBD/C3/gamez.zbd` stores two vertices with **subnormal `f32` `y` values** —
bit patterns `0x00000003` (`4e-45`) and `0x80000006` (`-8e-45`) — on a plane that
is otherwise **exactly** `y = 0` (117 of its 120 stored corner coordinates are
`±0.0`). `read_f32` decodes the bytes verbatim, so the denormals are what the
owner shipped, and this workspace neither introduced nor lost them.

Measured, by building that one mesh both ways through the production adapter:

| mesh | result |
| --- | --- |
| `c3` slot 447 as stored | **panics** in `bvh_binned_build.rs:58` |
| the same mesh with its subnormal coordinates flushed to zero | collider built |

So the blocker is **parry's BVH builder on denormal coordinates**, not this
project's readers, importer or spawn path. The bytes reach Avian intact and the
spawn returns `Ok`. A flat-quad control (four corners, `y = 0`) builds fine, and
so does one with `y = 1e-30`, so this is not "flat meshes are refused" — it is the
denormal magnitude specifically.

**Affected content:** the collision of whichever records in `c3` name mesh slot
447, on this host and this parry version. **Not affected:** presentation (the mesh
draws), the import, the sector index, or the other seven containers — every one
of them finishes its settle. **Resolving task:** filed below; the honest fix
belongs at the parry/Avian boundary or in an explicit stated rule about denormal
stored coordinates, and neither is this task's owner path.

The blocker is **pinned, not skipped**: `SETTLE_BLOCKERS` names it, and the test
asserts that the blocked set is *exactly* `["C3"]`, so the gap fails loudly in
either direction — if the settle starts working the assertion says so and points
at the follow-up, and if a new group breaks it panics as a new blocker.

## The measurement

Every number was produced by the production path over the read-only installation.
They are counts, dimensions and relations — no name list, no mesh, no screenshot —
so nothing derived from the original bytes is committed.

| container | grid | cells | values | child list | objects | in a sector | resident | sectors | no extent | empty cells |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `c1` | 12×12 | 144 | 346 | 66 | 412 | 346 | 66 | 144 | 0 | 0 |
| `c1b` | 12×12 | 144 | 155 | 78 | 233 | 140 | 93 | 139 | 5 | 0 |
| `c1c` | 12×12 | 144 | 293 | 53 | 346 | 293 | 53 | 144 | 0 | 0 |
| `c2` | 12×12 | 144 | 258 | 24 | 282 | 258 | 24 | 144 | 0 | 0 |
| `c2b` | 12×12 | 144 | 290 | 48 | 338 | 290 | 48 | 144 | 0 | 0 |
| `c3` | 16×16 | 256 | 439 | 14 | 453 | 434 | 19 | 253 | 3 | 0 |
| `c4` | 12×12 | 144 | 350 | 51 | 401 | 350 | 51 | 144 | 0 | 0 |
| `c5` | 16×16 | 256 | 471 | 105 | 576 | 468 | 108 | 252 | 4 | 3 |

Three relations hold in **every** container, and the tests assert them as
relations rather than as more columns:

1. **`values + child_list == objects`.** The grid's records and the world node's
   stored child list are disjoint and together are exactly the records that name
   the world node. This is #629's ownership cross-check, restated per group.
2. **`in_a_sector + resident == objects`.** Every object either lands in a sector
   or stays resident.
3. **`sectors == cells - no_extent`.** A cell whose members all store an all-zero
   box gets no sector.

**`objects_in_a_sector` is not the grid's value count, and the difference is the
finding.** #629 measured `c1c` only, where the two happen to be equal (293 and
293) because `c1c` has no cell without an extent. Over all eight they come apart:
`c1b` loses 15 records, `c3` loses 5 and `c5` loses 3 to cells that store no
extent. **Residency is a separate question from the collision role**, and a test
that only covered `c1c` could not tell the difference. A record's role follows the
spatial index (`Solid`), but an indexed record whose cell could not be given an
extent has no sector to belong to and is resident anyway. The test now asserts both
halves and their bounds — `resident >= child_list` (the unindexed records are
always resident) and `resident < objects` (residency is a real state, not the
whole world).

`c5` is also the only container with **empty cells** (3 of its 256 name no record
at all), and the only one whose mesh array holds empty slots.

## Test inventory

| `accept_f18_world_units_containers_` test | Covers | Fails when |
| --- | --- | --- |
| `every_world_group_imports_with_the_measured_counts` | all eight groups discovered (and every reference lead present); per group the grid's `x_count`/`y_count`, value count, distinct-slot count, cell count and world node; the three ownership counts and the relation between them; empty-cell and no-extent counts; `sectors == cells - no_extent`; residency as a **separate** question from the role, with its bounds; `objects_with_mesh` vs `values_with_mesh`; `matrix_disagreements`; `mesh_binding_records_elsewhere`; every unresolved role and surface carrying its claim id; no invented boundary; the reported unit factor at `Unknown`; every object's provenance at `ObservedTool` with the container's own span | a reader changes its walk, a count moves, residency stops being distinguished from the role, a gap loses its claim id, or a retail-derived value stops pointing at its bytes |
| `every_world_group_spawns_and_reports_its_gaps` | the production `spawn_world` over all eight, per group into its own app with the geometry uploaded through the production F17-B adapter: every object presented; colliders equal to the import's `partition_records_with_mesh`; every collider's record declaring `FromMesh` (so no substitute shape entered, checked on the spawn report so it holds even where the settle fails); and the settle check that every built collider really is a triangle mesh | the spawn stops accepting a container, a collider stops being derived from its own record's mesh, or the skip report stops being exactly the two gaps the bytes imply |
| `a_mesh_the_store_holds_no_geometry_for_is_a_gap` | `c5`'s 16 empty mesh slots and 61 affected records: the slot **exists** in the container's array (so it is a gap, not an out-of-range reference) and stores **no geometry at all** (so it is a gap, not a decode failure); every distinct mesh that does hold geometry is registered and **nothing else**; the mesh count is below the record count because records share meshes; and each affected record is reported with no collider at all | the `NoGeometry` arm becomes a refusal again (which loses the whole container), an empty slot gets registered, or an affected record is given a substitute shape |
| `the_settle_blocker_is_named_not_hidden` | the blocker **exists**, is **exactly** `["C3"]`, and rests on the measured stored bytes: mesh slot 447 stores exactly two vertices with a subnormal coordinate, decoded verbatim by the reader | the blocker is silently skipped or silently disappears, a new group breaks the settle, or the stored subnormals are no longer there (i.e. the premise changed) |
| `every_stored_transform_places_exactly` | the per-group count of records storing a real (non-identity) transform — `c1` 72 and `c5` 165 being the two the task named — and that the production `instance_placement`, the classifier the spawn runs over every instance *before* spawning anything, places **every** record of **every** group exactly | a record's transform is dropped on the way in, or a stored matrix is placed approximately rather than refused |

All five are `#[ignore]`d (`requires CS_GAME_DIR`) and **all five run and pass**
locally: `5 passed; 0 failed` over one discovery pass.

**Sensitivity.** Four mutations were applied to the production path, the retail
selection was re-run, and the source was restored each time. **All four are
killed**:

| mutation | killed by |
| --- | --- |
| `NoGeometry` arm in `uploaded_meshes` restored to a refusal | the gap test, the spawn test, the blocker test (`C5: the geometry uploads: mesh slot 909 could not be uploaded: the stored mesh has no material group`) |
| an unindexed record's role defaulted to `None` instead of an explicit unknown | the gap test, the import test, the spawn test |
| the sector-membership loop reads no cell (everything resident) | the import test (`C1: left: 412, right: 66`) |
| the grid names nothing (`indexed` emptied) | **all five** — the ownership cross-check refuses the container before anything else can agree |

## Unknowns and limitations (recorded, not guessed)

- **`c3`'s colliders are not verified on this host.** 33 of 374 are not built,
  because parry 0.27's BVH builder cannot bin a mesh whose centroid extent is
  denormal. **Affected content:** collision for whichever `c3` records name mesh
  slot 447, on this host and this parry version. **Not affected:** presentation,
  the import, the sector index, and the other seven containers. **Resolving task:**
  the follow-up filed below. This task does **not** claim a working `c3`
  collision path.
- **Whether the 2000 engine loaded a world this way is UNMEASURED.** No original
  run happened. Every fact here is measured from the container bytes by the
  production readers; nothing claims the engine streamed, culled or collided as
  this code does.
- **Whether an indexed record was a collider in the original is UNMEASURED**, and
  unchanged: the container states no collision field at all. `Solid` is this
  project's rule over a measured fact (membership in the index).
- **The original's world-vertex unit is UNMEASURED** (task #436, blocked). The
  conversion is a parameter and the report names it, so every length in an
  imported definition is "stored units × a declared factor". **Affected
  content:** every sector extent, every object position, and therefore any
  gameplay-distance claim over retail geometry. **Resolving task:** #436.
- **The world's floor, ceiling and lateral rules are UNMEASURED**, so every
  imported definition carries an explicit unknown boundary — never an invisible
  wall (F18 non-negotiable behavior 4).
- **Every gameplay surface class is UNMEASURED**, so no contact inherits a water
  or ground rule from a guess.
- **The mesh identity is the catalog's** as of #638, which this branch is built
  on. The one remaining per-container naming is
  `cs_app::world::container_mesh_key`, which `crate::playtest_retail` still uses;
  the world import no longer does.
- **The stored child list is the world's non-spatial content** (measured by #629:
  the horizon, `fvol*` volumes, vegetation, zeppelins), and none of it appears in
  the grid. An object in that half is resident because the store says so, not
  because it was forgotten.
- **Evidence class.** The facts above are measured from the original bytes by the
  production readers across all eight world containers, which makes the layout and
  the counts `ObservedTool` + measurement. Every **rule** — indexed ⇒ static,
  identity ⇒ node slot, empty mesh slot ⇒ gap — is a designed engine contract
  carrying its own claim id or its own documented arm. No original run happened:
  `retail` is file access, not evidence of runtime behaviour. **A further agent
  instance with a fresh context should review this**, and no agent review replaces
  the owner's approval.
- **Nothing derived from the original bytes is committed.** The numbers above are
  counts, dimensions and relations; no name list, no mesh and no screenshot is in
  the repository.

## Follow-ups filed

- **#656** (`F18-PARRY-DENORMAL-BVH`) — the `c3` settle blocker: either a stated
  rule for denormal stored coordinates at the upload boundary, or a parry/Avian
  configuration that bins them. Until then `c3` has no verified collision for that
  mesh and the blocker stays pinned rather than hidden. Filed by this task.
- **The `scene_node` id grammar blocker** F11-A's world names hit (filed by
  #629, unchanged by this task): a world record's identity can come from its
  authored name-path as well as from its slot.

## Sources used

- `crates/cs_app/src/world/retail.rs` (`read_world_containers`,
  `RetailWorldContainers`, `partition_grid`, `uploaded_meshes`),
  `crates/cs_app/src/world/spawn.rs` (`spawn_world`, `instance_placement`,
  `instance_placements`, `SkipReason`, `SpawnedWorld`) and
  `crates/cs_app/src/world/meshes.rs` (`WorldMeshBuildError`,
  `WorldMeshes::insert_render_mesh`).
- `crates/cs_content/src/world.rs` (`import_world_container`, `WorldPartitionGrid`,
  `WorldImportReport`, `ImportedWorld`) — read, **not** modified.
- `crates/cs_formats/src/gamez/reader.rs` (`read_gamez_meshes`, `GameZMeshes::get`,
  `read_vec3s`) and `crates/cs_formats/src/io.rs::read_f32`, which decodes the
  stored bytes verbatim — the reason the subnormals are the store's and not ours.
- `crates/cs_app/src/render/bevy_mesh.rs` (`upload_groups`, `build_upload`),
  `crates/cs_content/src/mesh.rs` (`RenderMesh::from_stored_groups`) and
  `crates/cs_content/src/coordinates.rs` (`SourceAdapter`).
- parry3d 0.27.0, `src/partitioning/bvh/bvh_binned_build.rs:52-61` and
  `src/shape/trimesh.rs::rebuild_bvh`; avian3d 0.7.0,
  `src/collision/collider/parry/mod.rs::trimesh_from_mesh` (read as the
  dependency it is; no code copied, no version changed).
- `docs/findings/2026-10-04-m01-lc-world-import.md` (#629: the `c1c` tables and the
  "only `c1c` is imported by a test" limitation this task closes),
  `docs/findings/2026-10-03-f18-world-hierarchy-authority.md` (the ownership rule
  the cross-check rests on) and `docs/findings/2026-09-30-t333-real-asset-stack-for-collider-from-mesh.md`
  (why the collider-from-mesh path is the real one).

## Commands run

```sh
cargo fmt --all -- --check                                       # clean
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # clean
cargo test --workspace --locked                                  # exit 0
cargo test --workspace --locked -- accept_f18_world_units_containers_ --include-ignored
#   5 tests: 5 run and pass (all five need CS_GAME_DIR)
```