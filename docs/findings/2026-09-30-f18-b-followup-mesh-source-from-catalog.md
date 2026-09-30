# F18-B follow-up: world mesh references from the mesh catalog

Date: 2026-09-30. Task #422, key `F18-B-followup-mesh-source-from-catalog`,
"Feed world mesh references from the mesh catalog instead of a map", filed by
F18-B (#86). Branch:
`rally/422-feed-world-mesh-references-from-the-mesh`. Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary build/test
only — `CS_GAME_DIR` was present but **no original byte was read** for this
change, so nothing here is `verified_original` and no evidence report is
required.

## The one behavior this slice changes

`cs_app::world::WorldMeshes` was a `BTreeMap<ContentId, WorldMesh>` filled by
the F18-B fixture from one F17-B `GroupUpload` per stored mesh — the group the
caller asked for, which the fixture always chose as group `0`. A real stored
mesh carries one F17-B `RenderGroup` per **stored raw material index**
(`cs_content::mesh::RenderMesh::groups`), so a source that took only group `0`
would present and collide the smallest material's triangles and silently drop
every other group's.

The behavior now: `WorldMeshes::insert_mesh_upload` takes the
`cs_content::mesh::MeshUpload` payload `MeshCatalog::prepare_upload` hands over
and uploads **every** material group through the same F17-B adapter, then merges
them into **one** engine mesh. The harbor fixture's hangar shell is authored on
three different material indices and its three boxes are each flown at, so a
single-group source fails the tests rather than passing quietly.

## Files

* `crates/cs_app/src/world/meshes.rs` (edited): `WorldMesh::from_group_uploads`,
  `WorldMeshGroup` (per-group material, fingerprint, triangle and vertex
  counts), `WorldMeshBuildError`, the merge and its attribute rule,
  `WorldMeshes::insert_render_mesh` (a `&RenderMesh` + unknowns) and
  `WorldMeshes::insert_mesh_upload` (the catalog payload). The single-group
  `insert` / `WorldMesh::from_upload` are kept: exact for a mesh whose polygons
  all store one material index, which is what a synthetic single-material
  fixture authors.
* `crates/cs_app/src/world/fixture.rs` (edited): `StoredMesh::box_at` takes the
  stored material index; the hangar shell's left leg, right leg and lintel are
  materials `0`, `1`, `2`; `harbor_meshes` fills the source through
  `insert_render_mesh` (every group), not `upload_group(..., 0)`.
* `crates/cs_app/src/world/mod.rs` (wiring): re-export `WorldMeshGroup` and
  `WorldMeshBuildError`.
* `crates/cs_app/tests/world/import.rs` (edited): the new acceptance test
  `accept_f18_b_every_material_group_of_a_stored_mesh_reaches_the_collider`.
* `crates/cs_app/src/render/bevy_mesh.rs` (edited — see "The adapter bug"
  below): a one-place fix to the per-group vertex compaction.
* This file.

## The multi-group decision: one merged collider

A stored multi-group mesh has exactly one geometry. F17-B's adapter groups
triangles by stored raw material index, and F10-E measured that a multi-group
polygon is one polygon re-drawn per group, not two polygons
(`docs/findings/2026-09-29-f10-e-material-groups-into-the-render-mesh.md`). A
static world object must collide with the union of every stored triangle, so the
world path builds **one** mesh and Avian derives **one** `TrimeshFromMesh`
collider from it. The alternatives were considered and rejected on evidence:

* **one collider (and one node) per group** is the same geometry with more
  entities and N colliders for one object; nothing measured needs the split.
* **refusing a multi-group mesh** would drop 307 of the installation's 17 139
  stored meshes (F10-E), i.e. real world geometry, for a decision F18-B already
  made: never simplify away authored geometry.

The merge is lossless for geometry: positions and indices are appended verbatim,
every index offset by the preceding groups' vertex count. A per-corner attribute
that every group stores is concatenated; one that no group stores is simply
absent, as it was in every group; one only *some* groups store cannot be
concatenated without padding the others, so it is dropped and named by
`WorldMesh::dropped_attributes` rather than hidden. A merged mesh's
`fingerprint` is a digest over every group's material index, fingerprint and
counts, and `WorldMesh::groups` keeps each group's own provenance, so "every
group was merged" is checkable.

## The adapter bug this task found (and fixed)

`crates/cs_app/src/render/bevy_mesh.rs::upload_group` compacted each group's
triangles into their own slot numbering (`slot_of: vertex_index -> slot`,
`indices` holding slots) but rebuilt the vertex arrays by indexing
`render.vertices()` with the **slot** numbers:

```rust
let mut slots: Vec<u32> = slot_of.values().copied().collect(); // = 0..total
slots.sort_unstable();
for &vertex_index in &slots { let vertex = vertices.get(vertex_index as usize) }
```

That is only the group's own vertices when the group's render-vertex indices are
exactly `0..total` in that order. F17-B's own tests only ever uploaded group `0`
of a single-group or first-in-stored-order mesh, so the bug was invisible:
`accept_f17_b_mesh_upload_keeps_stored_values_bit_exact` uploads group `0` of a
one-group quad, and `upload_groups(&quad_mesh(..., 3), ...)` builds one group.

The bug reproduced exactly the failure this task exists to prevent: the merged
mesh's group `1` and `2` positions were copies of group `0`'s left-leg box
(24 positions, three identical octets), so the derived collider was the left leg
drawn three times — 36 triangles, 8 welded vertices — and a body aimed at the
right leg or the lintel flew through. The fix inverts the map instead: slot `s`
holds the vertex index whose values belong at array position `s`, which keeps the
index buffer (slots) valid and the vertex arrays in slot order.

**Owner-path note.** This fix is in `crates/cs_app/src/render/bevy_mesh.rs`,
which the F18 spec's owner paths (`crates/cs_content/src/world.rs`,
`crates/cs_app/src/world/`, `crates/cs_app/tests/world/`) do not name; the
adapter is F17-B's. It is included because the task's sanctioned decision — one
merged collider over every group — cannot be built and tested without it, and the
alternative (a documented refusal of every multi-group mesh) would drop 307 of
17 139 real meshes for a one-place indexing bug. The regression test that pins it
lives in the task's own owner path
(`tests/world/import.rs::accept_f18_b_every_material_group_of_a_stored_mesh_reaches_the_collider`),
not in F17-B's suite.

## Open questions the task named, answered or left explicitly open

* **How a mesh is keyed when a node references `mesh_index` plus a variant.**
  Still **unmeasured**. No original world group has been parsed (F18-D #88 is
  `todo`, behind F18-C #87); there is no `mesh_index` reader in this tree, so
  there is nothing to map from. The source keys by the object record's own
  `Resolved<ContentId>` (`WorldObjectInstance::mesh`), which the importer fills;
  how a retail node's index and variant produce that id is F18-D's, and a
  guess would be exactly the invented layout F18-D exists to replace.
* **Whether a mesh may be shared by two objects, and uploaded once or twice.**
  The source holds **one** entry per `ContentId`, so two object records naming
  one mesh resolve to one `WorldMesh` (one upload, one fingerprint). The
  *engine* asset is not yet shared: `spawn_mesh_presentation` /
  `spawn_mesh_collider` clone the mesh into `Assets<Mesh>` per spawn, so two
  objects naming one mesh add it twice. Deduplicating that is a follow-up task
  (filed with this change); it changes presentation bookkeeping, not the source.
* **What the residency layer does with a mesh a reloaded sector needs after the
  previous sector released it.** Nothing releases one today: the source is
  caller-owned and passed to `load_world`/`load_sector` by shared reference, so
  it outlives every sector and a reload finds the same geometry. Per-sector mesh
  release is a **streaming** decision — the trigger is "which sector should be
  resident", which is F18-C #87's remit and does not exist yet — so this slice
  does not add a release path and records it here instead. Implementing it needs
  the residency layer, or a policy above it, to own the source rather than
  borrow it; that is F18-C.
* **Per-sector release of the mesh source.** Not implemented, for the reason
  above; recorded so the gap is visible rather than implied complete.

## Test and mutation matrix

`cargo test -p cs_app --test world -- accept_f18_b_` — 20 tests, all pass
(`--include-ignored` runs the same 20; none of them is `#[ignore]`d).

Every mutation below was applied, the world suite was run, and the source was
restored:

| mutation | tests that fail |
| --- | --- |
| `insert_render_mesh` uploads only `upload_group(render, 0)` | `..._every_material_group_of_a_stored_mesh_reaches_the_collider`, `..._a_swept_body_flies_through_the_mesh_opening_and_is_stopped_by_its_leg`, `..._a_mesh_role_solid_stops_a_body_and_sensor_only_reports_one`, `..._a_contact_names_the_surface_rule_its_object_was_authored_with` |
| `upload_group`'s vertex arrays indexed by slot again (the original bug) | `..._every_material_group_of_a_stored_mesh_reaches_the_collider` (fails at the right-leg/lintel flight); the F17-B suite stays green, which is why the bug survived |
| the fixture's three boxes all on material `0` | `..._every_material_group_of_a_stored_mesh_reaches_the_collider` (group count is not three): the fixture itself is part of the test |

The new test asserts, in order: three groups, materials `0,1,2`, twelve
triangles each and thirty-six together, three distinct group fingerprints, an
empty `dropped_attributes`, a merged fingerprint that is none of the group
fingerprints, a derived collider of thirty-six triangles, and then flies a body
at the right leg (group `1`) and the lintel (group `2`) and requires both
contacts and both stops before `x = 1`. The source's own provenance is checked
against the record's reference, so the test cannot be satisfied by a mesh the
fixture did not name.

## Evidence

Ordinary build/test only; no `CS_GAME_DIR` read, so no evidence report. Commands
run locally from a clean tree:

```sh
cargo fmt --all -- --check                                        # exit 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # exit 0
cargo test --workspace --locked                                   # exit 0
cargo test --workspace --locked -- accept_f18_b_ --include-ignored
#   crates/cs_app/tests/world: 20 tests run, 20 passed
```

`CS_GAME_DIR` was set (`retail`), but nothing in this change reads it; the
adapter fix and the fixture are synthetic. Nothing here is `verified_original`.

## Sources

No external sources. The record shapes follow `docs/contracts/IDENTITY-CONTENT.md`
and F18-B (`docs/findings/2026-09-30-f18-b-world-import-and-static-collision.md`);
the upload adapter and its fingerprint are F17-B
(`docs/findings/2026-09-30-f17-b-canonical-mesh-and-image-to-bevy.md`,
`crates/cs_app/src/render/bevy_mesh.rs`); the multi-group truth is F10-E
(`docs/findings/2026-09-29-f10-e-material-groups-into-the-render-mesh.md`); the
mesh IR and where its groups come from are F10-A
(`docs/findings/2026-09-28-f10-a-lossless-mesh-ir-and-strip-fixtures.md`); the
catalog boundary is `crates/cs_content/src/mesh.rs`
(`MeshCatalog::prepare_upload`, `MeshUpload`).
