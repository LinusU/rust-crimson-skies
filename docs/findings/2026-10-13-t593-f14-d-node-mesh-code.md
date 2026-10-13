# The F14 geometry walk quotes the format-level node-to-mesh check

Date: 2026-10-13. Task: #593 (F14-D-NODE-MESH-CODE) of
`specs/F14-canonical-content-catalog-and-dependency-closure.md`, stage
`### F14-D`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`. Capabilities
used: **`retail`** (read-only access to `$CS_GAME_DIR`) and ordinary build/test.
`gpu` and `audio` were available and **not used**: nothing is rendered or played
and no original run happened, so nothing here claims `verified_original`.

This closes the limitation the F10-C.05 findings recorded under "Unknowns and
limitations": `crates/cs_content/src/catalog/baseline.rs` resolved every node's
`mesh_index` against the mesh section **a second time**, and its one code
collapsed the two facts `cs_formats::gamez::MeshSlotIssue` keeps apart. The
machined record now carries the second code as its own claim id, its own
collection gap and its own per-container count.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/catalog/baseline.rs` (extend, the F14 owner path):
  `ContainerGeometry::bindings` holding one `NodeMeshBindings::of` per container,
  `GeometryContainerReport::out_of_range_meshes`, the `named_slot_past_mesh_array`
  collection gap, `unresolved_slots`, `node_mesh_issue_unknown`,
  `out_of_range_node_mesh_unknown`, `CLAIM_OUT_OF_RANGE_NODE_MESH`, and the report
  renderer's `out_of_range_meshes` field.
- `crates/cs_content/tests/accept_f14_d_4_geometry_collections.rs` (extend):
  the new `accept_f14_d_4_an_index_past_the_mesh_array_is_out_of_range_not_absent`,
  the two existing assertions that now name the new gap and the new report field
  (the per-container sum and the rendered report), and the retail test's two new
  zero pins.
- `crates/cs_content/tests/evidence_report_t593.rs` (new) and
  `docs/findings/evidence/T593.json` (new): the evidence record
  `docs/contracts/CLI-EVIDENCE.md` requires of a task that used `retail`.
- `docs/findings/2026-10-13-t593-f14-d-node-mesh-code.md` (this file).

**One observable failure:** before the change a node whose stored `mesh_index`
was past its container's mesh array and a node naming an inside-the-array
all-zero stub produced the same claim id
(`f14.d.4.baseline.node_mesh_slot_absent`) and were counted in the same gap
(`named_slot_without_mesh`). `accept_f14_d_4_an_index_past_the_mesh_array_is_out_of_range_not_absent`
builds a container that stores one of each and fails on the row claim, on the
per-container counts and on the two gaps until the walk quotes the format-level
check's two codes.

## Design decisions

- **The walk resolves `mesh_index` nowhere.** `read_geometry_container` builds
  `ContainerGeometry::bindings` once with `NodeMeshBindings::of(&nodes, &meshes)`
  and every later decision reads it: `absent_meshes` and `out_of_range_meshes`
  count the distinct slots of its findings, `geometry_mesh_rows` skips exactly
  the slots its findings name, and `geometry_node_rows` takes a node's mesh edge
  and its unknown from `bindings.finding(node.index)`. There is one
  implementation of the rule.
- **The two facts stay two facts.** A slot the array has and stores an all-zero
  stub in is `mesh_slot_absent` → `CLAIM_ABSENT_NODE_MESH`,
  `absent_meshes`, `named_slot_without_mesh`. A stored index past the array is
  `mesh_slot_out_of_range` → the new `CLAIM_OUT_OF_RANGE_NODE_MESH`,
  `out_of_range_meshes` and the new `named_slot_past_mesh_array` gap. Neither
  count contains the other and `mesh_rows + absent_meshes +
  out_of_range_meshes == named_meshes` still holds.
- **The row's reason names the array, not only the index.** Absent already said
  "the container's mesh section holds no present record for that slot"; the new
  out-of-range reason says "which is past the last of its {slots} mesh-array
  slots, so the position does not exist in this container at all", so the two
  facts are separable from the row text alone, without the claim id.
- **The findings are keyed by node, the counts by slot.** `unresolved_slots`
  collapses `NodeMeshBindings::findings` onto the distinct slots it names,
  because the `mesh` collection has a row per slot, not per naming node. Both
  codes survive the collapse, and they must: the issue is a property of the
  slot, so two nodes naming one slot always get the same answer.
- **No row references a row that does not exist.** Both codes drop the mesh
  edge and carry an explicit unknown instead, which is what the collection
  already did for the absent case. A position the container never had is the case
  where pointing at an id nothing holds would be most misleading.
- **A new gap is a new machine-readable fact, not a changed number.** The
  `named_slot_without_mesh` gap keeps its meaning and its measured value of zero;
  `named_slot_past_mesh_array` is inserted beside it so a catalog that stores one
  kind reports it without pretending the other happened.

## Measurement: the nine GameZ archives

The acceptance suite's retail test ran with `--include-ignored` against the
owner's installation (the evidence record's hashed `baseline-report.json`
artifact is the production report over those bytes). The totals are the ones
F14-D.4 pinned and are unchanged:

- 56 620 `scene_node` rows, 17 139 `mesh` rows, 9 geometry containers, 0
  unreadable containers, 0 unterminated parent chains.
- `named_slot_without_mesh` = **0** and `named_slot_past_mesh_array` = **0**.
- Neither `f14.d.4.baseline.node_mesh_slot_absent` nor
  `f14.d.4.baseline.node_mesh_slot_out_of_range` occurs on any row.
- Every container's `NodeMeshBindings::of` over the two production readers
  reports `unresolved() == 0`, an empty finding list, a `bound` equal to the node
  reader's own `mesh_index_bounds().bound`, and `slots` equal to the mesh
  reader's own `slot_count()`. That is the cross-check that the walk's counts are
  the check's own: the harness measures both from the same bytes and compares.

So the second code is a fact the catalog can now state and that this corpus
never states. The synthetic test is what exercises it.

## Test inventory

| `accept_f14_d_4_*` test | Covers | Fails when |
| --- | --- | --- |
| `an_index_past_the_mesh_array_is_out_of_range_not_absent` (new) | a container storing one index past its three-slot mesh array and one naming the inside-the-array stub: no row for either slot, a row for the two present ones, two different claim ids on the two node rows, no mesh edge on either, the row texts naming `mesh_index 2` and `mesh_index 5` and the out-of-range one naming the 3-slot array, `absent_meshes == 1` and `out_of_range_meshes == 1`, `named_meshes == 4` and `mesh_rows == 2`, both gaps at their own values, and the rendered report carrying both numbers | the two codes collapse into one (the per-container count or the row claim changes), an unresolved slot grows a row, an edge points at a row the catalog does not hold, or the new gap or report field disappears |
| `a_named_mesh_slot_without_a_present_mesh_is_counted_not_rowed` (unchanged) | the absent case keeps its meaning: no row, `named_slot_without_mesh`, `node_mesh_slot_absent` on the naming node, no mesh edge | the absent half of the rule is weakened |
| `the_report_renders_both_collections_and_the_per_container_counts` (extended) | the rendered per-container record now carries `out_of_range_meshes` | the new count stops being reported |
| `retail_every_gamez_container_yields_its_nodes_and_named_meshes` (extended) | the measured corpus: the 56 620 and 17 139 totals, `mesh_rows + absent_meshes + out_of_range_meshes == named_meshes` per container, `named_slot_without_mesh == 0` and `named_slot_past_mesh_array == 0`, and neither claim id on any row | the walk's counts drift from the format-level check, or a new gap reports a nonzero value over the installation |

**Sensitivity check.** Two mutations applied and reverted, each killed by the
new synthetic test:

| mutation | killed by |
| --- | --- |
| every finding reports `MeshSlotIssue::Absent` in `unresolved_slots` | `an_index_past_the_mesh_array_is_out_of_range_not_absent` (the per-container count) |
| `node_mesh_issue_unknown` emits `absent_node_mesh_unknown` for an out-of-range finding | `an_index_past_the_mesh_array_is_out_of_range_not_absent` (the row's claim id) |

A third mutation, removing the `unresolved.contains_key(&slot)` guard from
`geometry_mesh_rows`, would give the absent slot a row it cannot have; it is
covered by the unchanged
`accept_f14_d_4_a_named_mesh_slot_without_a_present_mesh_is_counted_not_rowed`,
which still passes.

## Unknowns and limitations (all recorded, none guessed)

- **Nothing in the measured installation stores an index past its mesh array.**
  The out-of-range code is pinned by a synthetic container and pinned *absent*
  over the corpus. **Affected content:** the machine-readable `Mesh` collection
  status and `SceneNode` row reasons on any container variant that stores such an
  index — none measured. **Resolving task:** none required; the code path is
  exercised by the synthetic acceptance test and the retail test pins that the
  count stays zero, so a later reader that changes the answer fails a test rather
  than a reviewer's eye.
- **The slot a node names says nothing about how often.** Inherited from F10-C.05
  unchanged: `NodeMeshBindings` reports the set of nodes that name no mesh and no
  multiplicity, so this walk still cannot say "28 nodes draw this slot". The
  multiplicity was measured for the nine archives (maximum 79, in C5) and is not
  part of the type. **Affected content:** a stage that needs an instance count or
  a per-mesh "how many nodes draw this" report. **Resolving task:** it measures it
  over `GameZNodes` rather than reading it here.
- **An absent slot that no node names is unused capacity, and only that.** 484
  positions of `planes.zbd`'s 2 250, 569 of C4's 3 000 and 732 of C1C's 2 250 hold
  an all-zero stub and none is named by a node. Whether the original engine drew
  nothing, fell back or refused is **unmeasured**: the reference's assertion is
  that tool's rule, not a measurement of the game. **Affected content:** nothing in
  this task's claims. **Resolving task:** an original run.
- **The header is not cross-checked between the two sections.** Both production
  readers call the same `read_container_header` over the same bytes, so
  `NodeMeshBindings::of` would pair a node array with a mesh section from another
  container without complaint. The walk reads one container's bytes for both, so
  it cannot do that; the check itself still could. **Affected content:** a caller
  that could pass a node array from a sibling container — no caller does.
  **Resolving task:** whichever stage decides the F10 catalog reads the node
  section.
- **Which mesh a node ends up drawing is F11's and F17-B's.** This walk records
  that every stored index names a present mesh and which slot it names. It does
  not decide which node's transform applies, how a LOD node selects, or what a
  renderer uploads. **Affected content:** every rendering and LOD decision.
  **Resolving tasks:** F11-C (hierarchy import), F11-D and F17-B.
- **`mesh_index` on a record that is not an object node is checked the same way.**
  The word sits at the same offset in the shared 208-byte record and is read as
  stored; the measured corpus stores `-1` on world, window, camera, display and
  light records, so nothing here claims what a non-`-1` value there would mean.
  **Affected content:** that word on every non-object record kind.
- **The nodes this check covers are the nodes one container holds.** A container
  with no node array — every synthetic F10-C.03 fixture, for instance — cannot be
  checked and the cross-check is silent rather than failing. **Affected
  content:** any fixture or archive without a node section.
- **Geometry rows are still unreachable and unclaimed by a runtime consumer.**
  No row points at a `scene_node` or a `mesh`, and no mission program is decoded,
  so both collections stay in `coverage.unreachable_by_kind` and
  `unreachable_needing_classification` and the report says 0 ready.
  **Affected content:** the reachability accounting of both collections and every
  mission's readiness. **Resolving tasks:** F11-B, F11-D, F37 and F38.
- **A scene node row carries no normalized quantity.** The stored translation,
  euler angles, LOD bounds and zone id are in source units and nothing in this
  workspace has established the original's world-vertex or angle unit.
  **Affected content:** all 56 620 scene nodes and all 17 139 mesh rows.
  **Resolving task:** F16's unit-calibration stage.
- **Evidence class.** The layout, the two remap tables and the node reader's
  `mesh_index` offset are documented in the pinned reference (mech3ax v0.6.0,
  commit `d3521a9721be731d365504568ddcd78e3f9846bb`, [S02] and [S17] in
  `docs/research/SOURCES.md`), and the verdicts were measured from the original
  bytes across all nine archives, which is `ObservedTool`. No original run
  happened: `retail` is file access, not evidence of runtime behaviour, and the
  reference's assertion is that tool's rule rather than a measurement of the game.
  **A further agent instance with a fresh context should review this format
  work**, and no agent review replaces the owner's approval.
- **Nothing derived from the original bytes is committed.** The numbers above are
  counts, ranges and verdict totals; no name list, no mesh, no material and no
  screenshot is in the repository.

## Sources used

- `crates/cs_formats/src/gamez/bindings.rs` (`NodeMeshBindings`, `MeshSlotIssue`,
  `NodeMeshFinding`) and `crates/cs_formats/tests/gamez/bindings.rs` (the check
  this now quotes and its retail test over `planes.zbd` and C4).
- `docs/findings/2026-10-03-f10-c-05-node-mesh-index-cross-check.md` (the check,
  the nine-archive measurement, and the recorded limitation this closes).
- `docs/findings/2026-10-03-f14-d.4-scene-and-mesh-collections.md` (the stage
  whose collections and totals this must not move).
- `crates/cs_content/src/catalog/baseline.rs`, `crates/cs_content/src/mesh.rs` and
  `crates/cs_formats/src/gamez/{nodes,reader}.rs` (the walk and the readers).
- `docs/contracts/IDENTITY-CONTENT.md` (catalog collections keep failed entries,
  unsupported reasons are codes, exact lookup with no filename guessing) and
  `docs/contracts/CLI-EVIDENCE.md` (the evidence record).
- `crates/cs_content/tests/evidence_report_t584.rs` (the shape of an evidence
  harness this file's harness follows).
