# F10-C.01: render vertex splitting by per-corner attributes

Date: 2026-09-29. Task: **F10-C.01** "Split render vertices by per-corner
attributes (AC03 visible UV seam)" (Rally #364), a slice of F10-C (#43) of
`specs/F10-gamez-mesh-topology-and-material-records.md`. Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary build/test
only — no `CS_GAME_DIR` read, no evidence report required. Implementer:
deepseek-1.

## Scope of this slice

`crates/cs_content/src/mesh.rs` (new) with a Bevy-free canonical render mesh
over F10-B's lossless IR. It builds from `RawMesh` + `MeshTopology`:

* one render vertex per distinct
  `(position index, normal index, uv, color, material)` tuple, compared
  bit-exactly, so render vertices split wherever UV, color, normal or
  material differ even when the position index is shared (F10 non-negotiable
  #3, AC03);
* a source-corner map per render vertex and a source `(polygon, step)` per
  render triangle;
* a validation gate that refuses an incomplete topology naming every rejected
  polygon and its `FaceIssue` code, and a bounds re-check while resolving
  stored indices (F10 non-negotiable #4);
* triangles grouped by raw material index, with polygon flags never
  interpreted (F10 non-negotiable #5).

**Not in this slice** (deliberately, per the task and `docs/TASK-SPLITTING.md`):
parsing `gamez.zbd` / `planes.zbd` bytes (F10-C.03 via #363), the content
session and catalog wiring, the upload boundary and any Bevy/Avian adapter
(F10-C.03, #366), and the F10-D private-corpus counts. This module only turns
an already-decoded IR into render vertices and triangles.

## Files and the one observable failure (listed before editing)

* `crates/cs_content/src/mesh.rs` (new): `RenderMesh`,
  `RenderVertex`, `RenderTriangle`, `RenderGroup`, `SourceCorner`,
  `SourceTriangle`, `RejectedFace`, `RenderMeshError`,
  `RenderMesh::build` / `RenderMesh::from_parts` and the private `VertexKey`.
* `crates/cs_content/src/lib.rs` (wiring only): `pub mod mesh;` and one
  module-doc paragraph.
* This file.

**One observable failure:** splitting by position index alone keeps one
vertex at a shared position, so the second triangle samples the first
polygon's UV and the authored seam disappears. With the two-triangle quad of
`accept_f10_c_01_uv_seam_keeps_two_vertices_at_one_position` the mesh has
**4** vertices instead of 5 and `uvs_of(render, 1)[0]` is `[0.0, 1.0]`
instead of the authored `[0.5, 0.5]`, so the test fails.

## Design decisions

* **The vertex key is the tuple's indices and stored values, bit-exact.** The
  `uv` and `color` values enter the key as `f32::to_bits` patterns, so
  `-0.0` and `0.0` differ and no tolerance or welding is applied. The
  position and normal **indices** are in the key, so two coincident positions
  with different indices stay two vertices ("no welding of different position
  indices").
* **Material is part of the vertex identity.** A material difference is a per
  polygon fact, so two polygons with different materials share no vertex even
  where every corner value agrees.
* **Source maps.** A deduplicated vertex records the first `(polygon,
  corner)` that produced it, which is enough to look the authored attributes
  back up; a render triangle records its `(polygon, step)`, the topology's
  own source identity. Vertices are emitted in first-encounter order and
  triangles in topology order, so the output is deterministic; the lookup
  `HashMap` is never iterated.
* **The gate refuses, it never drops.** `from_parts` first requires one face
  status per polygon (`TopologyFaceCount`), then refuses when any face is
  `Rejected`, listing every polygon and its `FaceIssue` code
  (`IncompleteTopology`). The rejected list is complete: a mesh with several
  broken faces names all of them.
* **Bounds are re-checked, not trusted.** `from_parts` accepts a caller's
  topology, so every stored position/normal index (and the topology's own
  polygon/corner references) is looked up through `stored`, which returns
  `OutOfRange` instead of indexing. A topology that claims a face decoded
  while the mesh stores an out-of-range index is therefore refused rather
  than panicking.
* **Degenerate triangles are kept.** A degenerate triangle is one with two
  equal stored *position indices* (`MeshTriangle::is_degenerate`, F10-A).
  They are kept, marked `RenderTriangle::degenerate`; nothing is dropped and
  the source maps, the face count and the triangle count stay exact. A
  consumer that draws may skip them. Dropping them was the alternative the
  task left open; keeping them matches F10-A's lossless rule and avoids a
  separate count that could disagree with the topology.
* **No value is interpreted.** Normals are stored unnormalized, UVs get no
  flip/wrap, colors no clamp or color-space change, and polygon `raw_flags`
  are never read: two meshes differing only in `raw_flags` build the same
  render mesh.

## Test inventory (`accept_f10_c_01_*`)

All in the `#[cfg(test)]` module of `crates/cs_content/src/mesh.rs`; every one
calls `RenderMesh::build` (and one also `RenderMesh::from_parts`). The fixture
is a unit square (four positions, two normals) authored in the test file. UV,
color and normal sampling is checked through the vertex indices the render
mesh produced, not through a parallel implementation.

| Test | Covers |
| --- | --- |
| `uv_seam_keeps_two_vertices_at_one_position` | AC03: the two triangles of a quad share positions 1 and 2; position 2's UV differs and splits, position 1's agrees and merges; each triangle samples its own authored UVs |
| `color_difference_splits_a_shared_position` | a color-only difference at a shared position splits |
| `normal_difference_splits_a_shared_position` | a normal-only difference (resolved from the normal index) splits |
| `material_difference_splits_every_shared_position` | a material-only difference splits every shared position |
| `identical_corners_merge` | identical corners share one vertex; shared position indices resolve to the same index |
| `distinct_position_indices_are_never_welded` | two coincident positions with different indices stay two vertices |
| `incomplete_topology_is_refused_with_exact_face_codes` | decoded + out-of-range + unsupported n-gon + too-few-corners: exact polygons and codes, all named in the error |
| `out_of_range_bounds_are_refused_not_panicked` | a supplied topology claiming the face decoded cannot index past `positions` (`OutOfRange`); through `build`, the same mesh is refused with the face's code |
| `material_groups_keep_raw_indices_and_ignore_flags` | groups by ascending raw material; `raw_flags` differences change nothing |
| `source_maps_cover_vertices_and_triangles` | vertex `(polygon, corner)` and triangle `(polygon, step)` maps |
| `degenerate_triangles_are_kept_and_counted` | strip `[0,1,2,1,3]`: 3 triangles, 1 degenerate, 1 face, counts exact |

## Mutation probes

Applied to `crates/cs_content/src/mesh.rs`, `cargo test -p cs_content --lib
-- accept_f10_c_01` run, file restored:

| Mutation | Failing tests |
| --- | --- |
| key ignores uv, color and normal | 3 (seam, color, normal) |
| material removed from the key | 1 (material split) |
| completeness gate removed | 2 (incomplete topology, out-of-range through `build`) |
| bounds check replaced by direct indexing | 1 (out-of-range test panics) |

## Recorded unknowns

* **Front-face winding and handedness** (carried from F10-A/F10-B): a render
  triangle keeps the stored winding; the render mesh does not decide which
  winding faces the viewer. F10-D measures it on the private corpus.
* **Material record layout and flag bit meanings**: the material index is
  kept raw and unresolved and `raw_flags` are discarded on purpose
  (non-negotiable #5). Resolving a material to its textures is F10-C.02.
* **How the upload consumer wants the data**: this slice exposes vertices
  plus per-material triangle-index groups; whether an upload builds a
  contiguous index range per material, splits into one buffer per material or
  keeps one buffer is F10-C.03 / F17-B. The groups are ordered by ascending
  material index for determinism, which that consumer may reorder.
* **Degenerate triangles in retail content**: whether original meshes contain
  them, and whether the original renderer skipped them, is unknown; kept
  triangles are marked so a consumer can decide. The count per private asset
  belongs to F10-D's AC04.
* **Mesh-to-node association**: `RenderMesh` carries no node id; the IR has
  none yet (F10-A) and node binding is F11-A.

None of these is filed as a new task: each is already the subject of F10-C.02,
F10-C.03, F10-D or F11-A.
