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
  seven `accept_m01_lc_world_import_` tests, two of them retail.
- `crates/cs_app/tests/world/main.rs` (wiring only): `mod import_retail;`.
- This file.

**No reader refusal was weakened.** `git diff` touches no line of
`crates/cs_formats`, `crates/cs_types` or `crates/cs_assets`; the grid is read
from the *same* bytes `read_gamez_nodes` walked, and the walk is checked to end
exactly where the reader said the block ends — against **two** ends, the block
length recomputed from the grid counts and the reader's own recorded
`data_bytes`, which also covers the world's child slots the reader walked after
the grid (see "Review" below for the second check and why it is there).

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

> **Superseded in part by #727** (2026-10-07,
> `docs/findings/2026-10-07-f18-grid-collision-origin.md`): that equality held
> for every container of the corpus until six grid-named `fvol*` records — four
> in `c1c`, two in `c5` — were bound to their own claim instead of `Solid`, so
> the relation is now `partition_records() == objects_solid() +
> partition_records_fog_volume()`. The rule itself (`INDEXED_RECORD_IS_STATIC`)
> is unchanged for the rest of the index and stays what this file says it is: a
> claim about this conversion, never about how the 2000 engine collided.

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
  reported (`matrix_disagreements`, counted over **every** imported record, 0 in
  `c1c`) so a reader knows how often that precedence was a choice.
- **A record that stores the identity gets the identity**, not a default: the
  store states it (`OBJECT3D_FLAGS_IDENTITY`), and a conversion that could not
  tell "the store said identity" from "we filled in identity" would be unable to
  report either.
- **The mesh identity is the catalog's (#638).** A stored mesh-array slot is an
  element of the shared render-mesh collection: `read_world_container` opens a
  content session scoped to the group and a `cs_content::mesh::MeshCatalog` over
  its `gamez.zbd` (F10-C.03), the definition's mesh references are
  `MeshId::content_id` — `mesh/<container path as the baseline spells it>.<slot>`,
  the very id `catalog::baseline` gives the mesh — and the geometry is uploaded
  from the catalog's own `MeshUpload`. What is measured: the ids are catalog
  elements (every referenced id is among `MeshCatalog::records`), no
  `<group>.mesh-<index>` name survives in the world import (the retail playtest
  scene, `playtest_retail`, still names its meshes that way and is not yet on the
  catalog), the catalog and the container's session
  share a generation, the reference's source span names the same bytes as the
  catalog's container (the span itself is spelled as discovery spells it, not as a
  group member), and c1c still spawns **292** colliders. What is **not** measured:
  that the original engine addressed meshes this way; the id is the catalog's
  convention. The definition's mesh reference keeps the import's claim id and span
  (`RETAIL_WORLD_IMPORT`, `ObservedTool`), not the catalog's own row provenance.
- **A mesh the container holds no geometry for is not registered.** The spawn then
  reports the gap (`SkipReason::UnknownMesh` /
  `MeshUnavailable`) rather than the source handing a solid object a substitute
  shape.

## Test inventory

| `accept_m01_lc_world_import_` test | Covers | Fails when |
| --- | --- | --- |
| `the_partition_grid_becomes_the_sector_index` | the shape of the grid read, `value_count`, `indexed_slots`, `empty_cells`; the two cells becoming `partition-00-00` / `partition-00-01`; each sector's extent being the **union** of its members' stored boxes, checked against a first cell that holds two records whose boxes disagree on one axis; membership per cell; the record the grid does not name staying resident; the cell header floats surviving the read **and** `header_floats_are_interpreted() == false` | the grid stops being the sector index, a sector's extent stops coming from the members' boxes (including from only one of them), the stored header floats are read as an extent, or an unindexed record stops being resident |
| `an_object_carries_its_slot_its_mesh_and_its_stored_transform` | the `node-<slot>` identity, the caller's mesh-slot table resolving the stored index, the identity record's identity transform, the meshless world-owned record resolving **no** mesh with its **own** claim id and reason (`f18-world.object-stores-no-mesh-index`, not the identity claim) and staying resident, and the caller's provenance reaching every record | the identity stops being the stored slot, a mesh index resolves without the caller's table, a stored transform is replaced by a default, or the meshless record starts reporting a claim id that is about something else |
| `the_collision_role_follows_the_index_and_every_other_role_is_named` | `Solid` + `FromMesh` for an indexed record; the **claim id and reason text** on an unindexed record's role and shape; every surface and the world's boundary carrying their own claim ids; and every count of the report (4 objects, 3 with a mesh, 3 solid, 3 in a sector, 1 resident, 2 sectors) | a role is defaulted, a gap loses its claim id, or a report count stops matching the definition |
| `a_grid_that_contradicts_itself_blocks_the_container` | `PartitionSlotRepeated` for two cells naming one record; `PartitionSlotOutOfRange` with the cell and slot for a value outside the array; `PartitionSlotNotAnObject { cell: 0, slot: 0, kind: "world" }` for a value naming the world record itself; `OwnershipDisagreement { grid: 3, child_list: 1, naming: 5 }` for a world-owned record in neither statement | a contradictory grid is imported, or the ownership cross-check stops running |
| `a_mesh_index_the_callers_table_does_not_hold_refuses_the_container` | `MeshSlotMissing { index: 7, slots: 7 }` when the caller's mesh table is one slot short of a stored index | a record resolves a mesh the caller's table does not hold, or the refusal loses the index it names |
| `spawn_world_runs_on_the_imported_definition` | the production `spawn_world` over the imported definition: every object presented, three colliders, the one record with no measured role reported as `SkipReason::UnknownCollisionRole`, and every derived collider a **triangle mesh** | the spawn stops accepting an imported definition, a role stops deciding the collider, or a `FromMesh` record is collided by a substituted shape |
| `the_conversion_is_recorded_under_its_own_claim_ids` | the seven claim ids the records carry plus `RETAIL_WORLD_IMPORT` are all valid `ClaimId`s and are seven **distinct** claims | two gaps start sharing a claim id, or an id is malformed |
| `retail_c1c_becomes_a_world_definition_with_every_gap_named` (retail) | `ZBD/C1C/gamez.zbd` end to end: 144 cells, 293 indexed records, 292 of them with a mesh, 4 of them the original's fog volumes (#727), 0 empty cells, 53 in the world's stored child list, 346 objects, 309 with a mesh, 289 solid, 293 in a sector, 53 resident, 144 sectors, 0 without an extent, 0 matrix disagreements, 21 unresolved roles split 17 + 4 across their two claim ids, 346 unresolved surfaces each carrying its claim id, no boundary, the reported unit factor and its `Unknown` class, 3 045 mesh-binding records the world node does not own, the container's own logical key, and **every** object's provenance being `RETAIL_WORLD_IMPORT` at `ObservedTool` with the container's own `SourceSpan` (and the same for a resolved collision value) | any measured count moves, a gap loses its claim id, a retail-derived value stops pointing back at its bytes, the class is inflated to `VerifiedOriginal`, or the unit stops being reported |
| `retail_spawn_world_runs_on_the_imported_c1c_definition` (retail) | the real container's geometry uploaded through the production F17-B adapter and the production `spawn_world` run over it: 346 objects presented, 288 colliders, and the spawn's skip report being **exactly** `{unknown_collision_role: 21, unknown_mesh: 1}` with no double-reported object (counts as of #727, 2026-10-07: 17 unindexed `fvol*` volumes + the four grid-named ones) | a count moves, an indexed record stops colliding, a substitute shape appears, or a record reports two reasons |
| `accept_f18_mesh_catalog_world_references_are_catalog_elements` (retail, #638) | every object's mesh reference is a `mesh` id among `MeshCatalog::records`, with no `.mesh-` naming, naming the catalog container's bytes, the catalog and session sharing a generation, and one engine mesh per referenced id uploaded under it | a per-container name returns, a reference is not a catalog element, or an upload is registered under another id |
| `accept_f18_mesh_catalog_world_the_catalog_id_is_the_baseline_mesh_id` (`cs_content`, synthetic) | `MeshId::content_id` equals the baseline inventory's mesh id and differs per slot | the two collections name one mesh differently |

The two retail tests each run one production discovery pass over the
installation, which takes about three and a half minutes on this host; they are
the slowest tests in the suite and are the reason they are two tests rather than
one. Neither test imports anything the other does not: both call the same
`retail()` helper, so a failing import fails both.

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

- ~~**Only `c1c` is imported by a test.**~~ **Closed by #639** on 2026-10-05:
  `crates/cs_app/tests/world/world_units.rs` runs the production path over **all
  eight** world containers and pins the per-group counts. See
  `docs/findings/2026-10-05-f18-world-units-containers.md`, which also records the
  two things covering all eight exposed: `c5` names 16 mesh slots the store holds
  no geometry for, and `uploaded_meshes` was aborting the whole container over the
  first of them (now a named gap); and `objects_in_a_sector` is **not** the grid's
  value count outside `c1c` — residency and the collision role are separate
  questions, and `c1b`/`c3`/`c5` lose 15/5/3 indexed records to cells that store
  no extent.
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

- **#638** — done: the F10-C.03 mesh catalog is the world container's mesh source
  (see "The mesh identity is the catalog's").
- ~~**#639** — import and spawn **all eight** world containers, not only `c1c`.~~
  **Done** on 2026-10-05; see
  `docs/findings/2026-10-05-f18-world-units-containers.md`. `c1` and `c5` did
  exercise the affine path on retail data: all 3 041 records across the eight
  containers place exactly, with 0 shears and 0 refusals.
- **#645** — one rule for a stored `object3d` transform. This conversion and
  `scene::canonical_local` use different precedence (see "Review"), which the
  corpus shows to be latent rather than harmful today.
- The `scene_node` id grammar blocker F11-A's world names hit, so a world record's
  identity can come from its authored name-path as well as from its slot.

## Review

Reviewed 2026-10-04 by **bunny-2** — the same agent instance that implemented
this stage, so this review is **not independent evidence**. A fresh-context
reviewer should still read it; the measurements below were re-taken by the
reviewer rather than copied.

Seven defects were found and fixed in the branch:

1. **A claim id that named the wrong thing.** A record that stores no mesh index
   resolved its mesh to `Resolved::Unknown` under
   `f18-world.object-id-is-the-node-slot` — a claim about *identity*, carried by
   a value about *mesh binding*. It now has its own claim,
   `f18-world.object-stores-no-mesh-index`, and a test asserts the claim id and
   the reason rather than only `!is_known()`.
2. **`matrix_disagreements` undercounted.** It counted only *indexed* records
   while its own doc said "imported records", so a disagreement on an unindexed
   record reported 0. It now counts every imported record. `c1c` is 0 either way,
   which the retail test pins.
3. **The walk's end was cross-checked against itself.** `GridEndMismatch`
   compared the re-walk's end with the block length recomputed from the same two
   grid counts, which a walk wrong in the same way as the reader would satisfy.
   It now also compares against the reader's own recorded `data_bytes`. Measured
   while fixing this: the reader walks the grid and **then** the world's child
   slots, so the grid's end plus `4 × children` is where `data_bytes` points —
   a first version that compared `data_bytes` directly failed the fixture by
   exactly one child slot, which is how that relation became known.
4. **A silent skip in `mesh_index_of`.** A mesh name of this container's own
   shape that carried no decimal index returned `Ok(None)` and was skipped, which
   is the exact outcome the function's doc says it exists to prevent. It is now
   a typed `UnknownMeshReference`.
5. **`imported_objects` was dead API** — a one-line wrapper over
   `definition.objects()` with no caller. Removed, with its re-export.
6. **A duplicated comment paragraph** in the sector-index test said the same
   thing twice; and this file's test inventory claimed six claim ids and
   `OwnershipDisagreement { grid: 2, child_list: 1, naming: 4 }`, neither of which
   matched the test that ran. Both corrected.
7. **Uncovered production refusals.** `PartitionSlotNotAnObject` and
   `MeshSlotMissing` were reachable and typed but untested; both now have an arm.

The re-walk's second end also fixed a documentation claim: this file previously
said the walk "is checked to end exactly where the reader said the block ends",
which was true only in the weaker sense above.

### The transform rule, measured

This conversion returns the identity whenever a record flags
`OBJECT3D_FLAGS_IDENTITY`, and otherwise always uses the stored 3×3.
`scene::canonical_local` (`crates/cs_content/src/scene.rs`) is stricter in both
places: identity only when the record flags it **and** really is the identity,
and the euler-derived matrix when the stored one agrees with the triple.

Measured over all eight world containers with the production reader:

| container | world-owned object records | flagged identity | flagged identity but not really identity | transformed | stored matrix disagrees |
| --- | --- | --- | --- | --- | --- |
| c1 | 412 | 340 | 0 | 72 | **1** |
| c1b | 233 | 154 | 0 | 79 | 0 |
| c1c | 346 | 316 | 0 | 30 | 0 |
| c2 | 282 | 252 | 0 | 30 | 0 |
| c2b | 338 | 304 | 0 | 34 | 0 |
| c3 | 453 | 436 | 0 | 17 | 0 |
| c4 | 401 | 344 | 0 | 57 | 0 |
| c5 | 576 | 411 | 0 | 165 | 0 |

So the divergence is **latent, not observed**: no world-owned record flags
identity while storing something else, and exactly one record in the corpus
(`c1`) is one where the two rules differ. Every other record is placed
identically by both paths. Two rules for one conversion is still a canonical
contract defect (AGENTS rule 7), and unifying them means changing the scene
layer, which is outside this task's owner paths — so it is filed as **#645**
rather than done here.

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
