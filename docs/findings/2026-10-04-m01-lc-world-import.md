# #629: an original world container becomes a `WorldDefinition`

Date: 2026-10-04. Task: #629 "Import an original world container into
WorldDefinition + collision" (`M01-LC-WORLD-IMPORT`), the step
`VS-M01-RUNTIME` (#359) waits on. Feature sheet:
`specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`, stages
`### F18-A` and `### F18-B`. Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: **`retail`**
(read-only access to `$CS_GAME_DIR`) and ordinary build/test. `gpu` and `audio`
were available and **not used**: nothing is rendered or played and no original
run happened, so nothing here is `verified_original`.

## Files

- `crates/cs_content/src/world.rs` (extended, an F18 owner path): the claim ids,
  `WorldPartitionGrid` / `WorldPartitionCell` (the world record's own spatial
  index, decoded out of the container bytes), `WorldImportError`,
  `WorldImportReport`, `ImportedWorld` and `import_world_container`.
- `crates/cs_app/src/world/retail.rs` (new, an F18 owner path):
  `read_world_container`, `RetailWorldContainer` (one container held open for
  the importer), `RetailWorldError`, `RETAIL_WORLD_IMPORT`.
- `crates/cs_app/src/world/mod.rs` (wiring only): `pub mod retail;` and its
  re-exports.
- `crates/cs_app/tests/world/import_retail.rs` (new, an F18 owner path): the
  six `accept_m01_lc_world_import_` tests, two of them retail.
- `crates/cs_app/tests/world/main.rs` (wiring only): `mod import_retail;`.
- This file.

**No reader refusal was weakened.** `git diff` touches no line of
`crates/cs_formats`, `crates/cs_types` or `crates/cs_assets`; the grid is read
from the *same* bytes `read_gamez_nodes` walked, and the walk is checked to end
exactly where the reader said the block ends.

## The measurement

Every number below was produced by the production readers over the read-only
installation. They are counts, dimensions and relations — no name list, no mesh,
no screenshot — so nothing derived from the original bytes is committed.

### The world record's partition grid *is* the world's sector index

The world node's own data record ends with a grid of `partition_x_count ×
partition_y_count` 88-byte cells, each followed by its own `count` × 12-byte
values whose first word is a **node slot**. This task is the first to interpret
it. Measured over all eight world containers:

| container | grid | cells | values | distinct | all `object3d` | out of range | repeated | world's stored child list | records naming the world node |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `zbd/c1/gamez.zbd` | 12×12 | 144 | 346 | 346 | yes | 0 | 0 | 66 | 412 |
| `zbd/c1b/gamez.zbd` | 12×12 | 144 | 155 | 155 | yes | 0 | 0 | 78 | 233 |
| **`zbd/c1c/gamez.zbd`** | **12×12** | **144** | **293** | **293** | **yes** | **0** | **0** | **53** | **346** |
| `zbd/c2/gamez.zbd` | 12×12 | 144 | 258 | 258 | yes | 0 | 0 | 24 | 282 |
| `zbd/c2b/gamez.zbd` | 12×12 | 144 | 290 | 290 | yes | 0 | 0 | 48 | 338 |
| `zbd/c3/gamez.zbd` | 16×16 | 256 | 439 | 439 | yes | 0 | 0 | 14 | 453 |
| `zbd/c4/gamez.zbd` | 12×12 | 144 | 350 | 350 | yes | 0 | 0 | 51 | 401 |
| `zbd/c5/gamez.zbd` | 16×16 | 256 | 471 | 471 | yes | 0 | 0 | 105 | 576 |

Four statements, each checked by the reader rather than assumed:

1. **Every grid value names a distinct, in-range object record.** No container
   names a slot twice, in one cell or across cells, and no container names a
   record that is not `object3d`. The conversion refuses all three refusals
   rather than importing under a rule the bytes contradict, and the corpus has
   none.
2. **Grid ∪ the world node's stored child list is exactly the set of records that
   name the world node as their parent**, and the two halves are disjoint. This
   confirms, from the production readers, the relation
   `docs/findings/2026-10-03-f18-world-hierarchy-authority.md` measured with a
   scratch walk, and `import_world_container` **cross-checks all three counts**
   (`WorldImportError::OwnershipDisagreement`) rather than trusting one.
3. **The world's own stored child list is its non-spatial content.** Read over
   the containers, those records are the horizon, the volumetric fog volumes
   (`fvol*`), vegetation instances and zeppelins; **none** of them appears in
   the grid, and no grid record appears in that list. So the two halves are not
   two views of one set: they are the world's spatially indexed geometry and the
   world's ambient content, and an object in the second half is **resident** in
   the imported definition because that is what the store says, not because it
   was forgotten.
4. **A grid record that binds no mesh is exactly a grid record whose stored
   bounding box is all zero.** The two counts agree in every container (`c1c`:
   1 and 1; `c1`: 61 and 61; `c5`: 104 and 104). That is why an all-zero box is
   read as the store's own "no extent" statement and not as a degenerate one,
   and why a cell whose members are all extent-less is reported as a cell with
   no sector rather than given one.

### Which records are static colliders, and which are cosmetic

**The container stores no per-record collision field at all.** Every bit of the
CS node flag word is `UNK*` in the pinned reference, and no field of any record
says "collides". What the container *does* state about a world record is exactly
two things: membership in the world's own spatial index, and a stored
world-space bounding box (`unk140`).

So the honest measured answer to "static collider or cosmetic" is **the spatial
index**, and this conversion follows it as a **designed rule with its own claim
id** (`f18-world.indexed-record-is-static`):

* an indexed record becomes `Solid` + `FromMesh` — the world's static spatial
  geometry, collided from the very mesh it draws;
* every record the index does not name carries an **explicit unknown** with
  `f18-world.unindexed-collision-role-unmeasured`, so a consumer sees "the
  container did not say" instead of a world in which every effect, every fog
  volume and every grass instance blocks a plane.

`WorldImportReport::objects_solid()` equals `partition_records()` in every
container, which is what makes "the world's static geometry against its ambient
content" a single number a consumer can read.

### The unit and scale

**The original's world-vertex unit is still unmeasured** (task #436, blocked; no
known-size landmark exists in the container and no original run has happened).
`import_world_container` therefore takes its conversion from the caller's
`cs_content::coordinates::SourceAdapter` and refuses to supply one, and the
report states both the factor used (`meters_per_unit`) and that factor's own
evidence class (`unit_class`, `ClaimStatus::Unknown` for every conversion this
workspace declares). The unit is a **reported** fact, never an assumed one.

The stored geometry is in **stored units** and the measured extents are: `c1c`'s
grid tiles the plane `[-12288, 0]²` in 1024-unit steps, and the tallest indexed
record's stored box reaches `y = 1091.28`.

**The cell's own six header floats are not interpreted.** They were the obvious
candidate for a sector's extent, and measurement killed it: reading them as the
cell's low/high bounds on the two horizontal axes holds for **every** cell of
`c1c` and `c2b`, for 139/144 of `c1b`, 234/256 of `c3`, 215/256 of `c5`,
124/144 of `c2`, 117/144 of `c1` and only 105/144 of `c4`. The misses are cells
whose members' stored boxes disagree, so the field is not a cell extent. The
sector extent published here is instead the **union of the stored boxes the
members state** — a per-record fact the store states, converted through the
adapter — and `WorldPartitionCell::header_floats_are_interpreted()` returns
`false` so the six floats cannot be mistaken for a reading.

## Design decisions

- **The grid is decoded, not inferred.** `WorldPartitionGrid::read` re-walks the
  block the node reader walked, for its content, and refuses when its walk does
  not end exactly where the reader said (`GridEndMismatch`). The stored pointer
  is never followed, exactly as the node reader's own rule.
- **The three ownership statements are cross-checked, not assumed.** A container
  whose grid, stored child list and parent slots disagree is blocked with the
  exact three counts. A conversion that trusted one of them would import a world
  whose membership depends on which side it happened to read.
- **Object identity is the container's own node slot** (`f18-world.object-id-is-the-node-slot`),
  stated as a **designed** identity. The container's display names are not usable
  for it: `c1c`'s world node lists **thirty-four records all named `g27816`**, and
  F11-A's `scene_node` id grammar refuses many world names outright. The stored
  slot is what the store addresses a record by, what the grid names it by, and
  the one value the key grammar always accepts.
- **Sector identity is the cell's grid coordinates** (`partition-00-00`), so a
  sector's id is stable across reads of the same container and independent of the
  order the records happen to be stored in.
- **The stored 3×3 wins over the record's euler triple** — the format reader's
  own precedence rule — and the number of records where the two disagree is
  reported (`matrix_disagreements`, 0 in `c1c`) so a reader knows how often that
  precedence was a choice.
- **A record that stores the identity gets the identity**, not a default: the
  store states it (`OBJECT3D_FLAGS_IDENTITY`), and a conversion that could not
  tell "the store said identity" from "we filled in identity" would be unable to
  report either.
- **The mesh identity is per-container and stated as such.** A stored
  mesh-array slot is named `<group>.mesh-<index>` by
  `RetailWorldContainer::mesh_key`, so the definition's mesh references and the
  uploaded geometry agree by construction. This is **not** F10-C.03's catalog
  discovery, which maps a slot to an element of the shared render-mesh catalog
  and needs the whole installation; that is the seam a catalog-backed source
  replaces, and it is named as such rather than presented as the catalog.
- **A mesh the container holds no geometry for is not registered.** The spawn then
  reports the gap (`SkipReason::UnknownMesh` /
  `MeshUnavailable`) rather than the source handing a solid object a substitute
  shape.

## Test inventory

| `accept_m01_lc_world_import_` test | Covers | Fails when |
| --- | --- | --- |
| `the_partition_grid_becomes_the_sector_index` | the shape of the grid read, `value_count`, `indexed_slots`, `empty_cells`; the two cells becoming `partition-00-00` / `partition-00-01`; each sector's extent being the **union** of its members' stored boxes, checked against a first cell that holds two records whose boxes disagree on one axis; membership per cell; the record the grid does not name staying resident; the cell header floats surviving the read **and** `header_floats_are_interpreted() == false` | the grid stops being the sector index, a sector's extent stops coming from the members' boxes (including from only one of them), the stored header floats are read as an extent, or an unindexed record stops being resident |
| `an_object_carries_its_slot_its_mesh_and_its_stored_transform` | the `node-<slot>` identity, the caller's mesh-slot table resolving the stored index, the identity record's identity transform, the meshless world-owned record resolving no mesh and staying resident, and the caller's provenance reaching every record | the identity stops being the stored slot, a mesh index resolves without the caller's table, or a stored transform is replaced by a default |
| `the_collision_role_follows_the_index_and_every_other_role_is_named` | `Solid` + `FromMesh` for an indexed record; the **claim id and reason text** on an unindexed record's role and shape; every surface and the world's boundary carrying their own claim ids; and every count of the report (4 objects, 3 with a mesh, 3 solid, 3 in a sector, 1 resident, 2 sectors) | a role is defaulted, a gap loses its claim id, or a report count stops matching the definition |
| `a_grid_that_contradicts_itself_blocks_the_container` | `PartitionSlotRepeated` for two cells naming one record; `PartitionSlotOutOfRange` with the cell and slot for a value outside the array; `OwnershipDisagreement { grid: 2, child_list: 1, naming: 4 }` for a world-owned record in neither statement | a contradictory grid is imported, or the ownership cross-check stops running |
| `spawn_world_runs_on_the_imported_definition` | the production `spawn_world` over the imported definition: every object presented, three colliders, the one record with no measured role reported as `SkipReason::UnknownCollisionRole`, and every derived collider a **triangle mesh** | the spawn stops accepting an imported definition, a role stops deciding the collider, or a `FromMesh` record is collided by a substituted shape |
| `the_conversion_is_recorded_under_its_own_claim_ids` | the six claim ids the records carry plus `RETAIL_WORLD_IMPORT` are all valid `ClaimId`s and are six **distinct** claims | two gaps start sharing a claim id, or an id is malformed |
| `retail_c1c_becomes_a_world_definition_with_every_gap_named` (retail) | `ZBD/C1C/gamez.zbd` end to end: 144 cells, 293 indexed records, 292 of them with a mesh, 0 empty cells, 53 in the world's stored child list, 346 objects, 309 with a mesh, 293 solid, 293 in a sector, 53 resident, 144 sectors, 0 without an extent, 0 matrix disagreements, 53 unresolved roles each carrying its claim id, 346 unresolved surfaces each carrying its claim id, no boundary, the reported unit factor and its `Unknown` class, 3 045 mesh-binding records the world node does not own, and the container's own logical key | any measured count moves, a gap loses its claim id, or the unit stops being reported |
| `retail_spawn_world_runs_on_the_imported_c1c_definition` (retail) | the real container's geometry uploaded through the production F17-B adapter and the production `spawn_world` run over it: 346 objects presented, 292 colliders, and the spawn's skip report being **exactly** `{unknown_collision_role: 53, unknown_mesh: 1}` with no double-reported object | a count moves, an indexed record stops colliding, a substitute shape appears, or a record reports two reasons |

The two retail tests each run one production discovery pass over the
installation, which takes about three and a half minutes on this host; they are
the slowest tests in the suite and are the reason they are two tests rather than
one.

**Sensitivity.** Seven mutations were applied to `import_world_container`, the
non-retail selection was re-run and the source restored each time. **All seven
are killed by a test CI can run** — the two retail tests are `#[ignore]`d and
were not used to kill any of them:

| mutation | killed by |
| --- | --- |
| an indexed record's role becomes `Unknown` instead of `Solid` | the role test, the spawn test |
| the sector-membership loop reads no cell (everything resident) | the sector-index test, the identity test, the role test's report counts |
| a sector's extent becomes its **first** member's stored box instead of the union | the sector-index test, whose first cell holds two records whose boxes reach past each other |
| an unindexed record's role is defaulted to `None` instead of `Unknown` | the role test (the `Unknown { claim_id }` match), the spawn test (the `UnknownCollisionRole` skip) |
| the ownership cross-check is removed | the contradiction test's third arm |
| an object's identity becomes its display name | the sector-index test's resident list, the identity test, the role test |
| an object's surface is defaulted to `Ground` instead of named | the role test |

The third row is the one worth noting: an extent built from one member's box
rather than the union **passes** every assertion a fixture with one member per
cell could make, which is why the fixture's first cell holds two records whose
stored boxes disagree on one axis.


## Unknowns and limitations (recorded, not guessed)

- **Which side the 2000 engine trusted at load time is UNMEASURED.** No original
  run happened. The partition grid is measured as *what the container's bytes
  say*; nothing here claims the engine streamed on it, and the hierarchy
  question stays open as `docs/findings/2026-10-03-f18-world-hierarchy-authority.md`
  left it. **Affected content:** every claim about the original's runtime world
  streaming and culling. **Not affected:** the conversion implemented here, which
  carries its own claim ids.
- **Whether an indexed record was a collider in the original is UNMEASURED.** The
  container states no collision field at all; `Solid` is this project's rule over
  a measured fact (membership in the index), stated as such.
- **The original's world-vertex unit and coordinate handedness are UNMEASURED**
  (task #436, blocked). The conversion is a parameter and the report names it, so
  every length in an imported definition is "stored units × a declared factor"
  until that task measures one. **Affected content:** every sector extent, every
  object position and therefore any gameplay-distance claim over retail
  geometry. **Resolving task:** #436.
- **The world's floor, ceiling and lateral rules are UNMEASURED.** The world
  record's own words are not read, so every imported definition carries an
  explicit unknown boundary — never an invisible wall (F18 non-negotiable
  behavior 4).
- **The original's gameplay surface classes are UNMEASURED.** Every imported
  object's surface is an explicit unknown, so no contact inherits a water or
  ground rule from a guess.
- **The mesh identity is per-container.** `<group>.mesh-<index>` is this
  module's naming, not the shared catalog's, so two world containers can hold
  the same mesh under different ids until the catalog-backed source replaces it.
  **Resolving task:** the F10-C.03 mesh catalog consumer for world containers.
- **F11-A's `scene_node` id grammar still refuses world node names**, which is
  why `SceneGraph::build` cannot build a world container and why this import
  addresses records by slot instead. That blocker is unchanged by this task;
  this task does not need it.
- **Evidence class.** The facts above are measured from the original bytes by the
  production readers across all nine GameZ archives, which makes the layout and
  the counts `ObservedTool` + measurement. Every **rule** — indexed ⇒ static,
  identity ⇒ node slot — is a designed engine contract carrying its own claim
  id. No original run happened: `retail` is file access, not evidence of runtime
  behaviour. **A further agent instance with a fresh context should review this
  format work**, and no agent review replaces the owner's approval.
- **Nothing derived from the original bytes is committed.** The numbers above are
  counts, dimensions and relations; no name list, no mesh and no screenshot is in
  the repository.

## Follow-ups filed

- The F10-C.03 mesh catalog as the world container's mesh source, so a world
  object's mesh identity is a catalog element rather than a per-container name.
- The `scene_node` id grammar blocker F11-A's world names hit, so a world record's
  identity can come from its authored name-path as well as from its slot.

## Sources used

- `crates/cs_formats/src/gamez/nodes.rs` (`read_gamez_nodes`, `RawNodeInfo`,
  `RawObject3dData`, `RawWorldData`, `WORLD_DATA_BYTES`, `WORLD_PARTITION_BYTES`,
  `WORLD_PARTITION_VALUE_BYTES`) and `crates/cs_formats/src/gamez/reader.rs`
  (`read_gamez_meshes`, `GameZMeshes::get`, `GameZMesh::groups`).
- `crates/cs_content/src/scene.rs` (`MeshSlot`, `CanonicalTransform`) and
  `crates/cs_content/src/coordinates.rs` (`SourceAdapter`,
  `CoordinateSource`, `SourceConvention::axes`, `meters_per_unit`,
  `UnitCalibration::claim_status`).
- `crates/cs_content/src/mesh.rs` (`RenderMesh::build` / `from_stored_groups`,
  `MeshPresentationUnknown`).
- `crates/cs_app/src/world/{spawn,meshes,residency,triggers,audit}.rs` — the
  spawn path this feeds, the mesh source it consumes and the two existing
  production readers of a world container, which set the shape of this one.
- `docs/findings/2026-10-02-gamez-node-array-layout.md` (the node array and the
  world record's grid framing), `docs/findings/2026-10-03-f18-world-hierarchy-authority.md`
  (the hierarchy rule this import's ownership cross-check rests on) and
  `docs/findings/2026-09-30-f18-{a,b,d}-*.md` (the typed contract, the
  synthetic-only fixtures this replaces, and the unmeasured-unit limitation this
  reports rather than resolves).
- Pinned reference mech3ax v0.6.0, commit
  `d3521a9721be731d365504568ddcd78e3f9846bb` ([S02], [S17] in
  `docs/research/SOURCES.md`): `WorldCsC` / `PartitionCsC` /
  `PartitionValue` in `crates/mech3ax-nodes/src/cs/world/data.rs`. No code was
  copied; mech3ax is EUPL-1.2 and is read as a reference only.
- `docs/contracts/IDENTITY-CONTENT.md` (stable ids, `Resolved<T>`, evidence
  classes).

## Commands run

```sh
cargo fmt --all -- --check                                       # see the handover
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_m01_lc_world_import_ --include-ignored
#   8 tests: 6 run and pass, 2 retail run and pass
```
