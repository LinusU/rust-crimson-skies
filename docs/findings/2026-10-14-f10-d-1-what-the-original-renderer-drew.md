# F10-D.1: what the original renderer drew for the eleven faces — reconciled

**Task:** #395 / `F10-D.1` — *Reconcile the eleven faces that reach no
drawable triangle with the original's measured behaviour* (follow-up of F10-D
#44, deferred item 1 in
`docs/findings/2026-09-30-f10-d-private-corpus-face-census.md`;
`specs/F10-gamez-mesh-topology-and-material-records.md`, stage `### F10-D`).
**Capabilities used:** `retail` (read access to `$CS_GAME_DIR`), read access
to the owner-supplied decrypted executable at `$CS_ENGINE_IMAGE`, and ordinary
build/test. No original executable was run, no GPU render, no audio, no human
play.
**Claim class:** `implemented`, with **static code evidence** for the
original's behaviour, cited as `OWNER-STATIC-2026-10-05`. This is disassembly
of the owner-supplied image plus checks against the stored bytes — never
`verified_original`, and no original run happened.
**Contract:** `docs/contracts/IDENTITY-CONTENT.md`.

## The evidence

`OWNER-STATIC-2026-10-05` is the owner's static analysis of the decrypted
executable `crimson.decrypted.exe` (SHA-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`; the file
lives outside `$CS_GAME_DIR`, at the path `CS_ENGINE_IMAGE` names, because an
extra file inside the installation would change its fingerprint), with every
face's own bytes checked in the retail archives. Addresses below are virtual
addresses in that image (file offset = VA − `0x400000` below `0x643000`); only
addresses, indices and behaviour are recorded — no decompiled code, no
executable bytes, no stored coordinates.

The four rules that decide every face:

* **R1 — no triangulation.** An outline polygon is sent to Direct3D as
  `D3DPT_TRIANGLEFAN` over its stored corners, fanned from corner 0, with
  culling off (primitive setup `0x557334`/`0x5573f6` → `DrawPrimitive`).
  A zero-area fan triangle rasterises nothing; a concave or self-intersecting
  outline draws whatever the fan covers.
* **R2 — the per-frame face test reads only the first three corners.**
  `N = (P1 − P0) × (P2 − P0)` and `d = N · P0` in camera space; the face is
  drawn only when `d < −tol`, or `d > +tol` under the two-sided bit `0x400`,
  with `tol = 0.00005` (`SetBFETolerance`, configured from
  `support\init.gw`). A zero `N` keeps `|d| ≤ tol` whatever the transform or
  the flag, so the fan is never submitted (`0x554bf7`–`0x554ce9` D3D,
  `0x552e1f`–`0x552f19` software) — **a zero first-three-corner normal is
  always culled, two-sided or not**.
* **R3 — polygons are independent.** The draw loops (`0x554b91`…`0x555144`,
  `0x552915`) handle each polygon on its own. Nothing validates a mesh or
  drops it because one face is bad — **no mesh is ever dropped for a bad
  face**.
* **R4 — `dzpath*` nodes are removed at world load.** `0x4459f0`
  (`dzpath.cpp`, the `DZPathList` build) collects every node whose name
  starts with `dzpath`, detaches it (`0x4cd800`) and destroys it with its
  subtree (`0x4d7a50` → mesh release `0x561910`, node free `0x4cc710`). The
  AI looks the routes up later as `dzpath%d` (`0x41d2d0`). These meshes are
  never drawn — they are AI danger-zone route data, not scenery.

## The eleven faces, decided

Stored indices only. "Face test" is R2 over the stored corners.

| archive | mesh | polygon | node(s) | stored corners (shape) | original behaviour |
| --- | ---: | ---: | --- | --- | --- |
| `ZBD/C4` | 191 | 20 | `pod6_lolevel` (1881/1887) | 5, first two coincide | **Culled every frame** (zero N). It is the only face on its four vertices, so that quad is invisible in the original |
| `ZBD/C4` | 191 | 55 | `pod6_lolevel` | 3, first and last coincide | **Culled every frame** (zero N); zero-area anyway, on the edge of strip poly 26 |
| `ZBD/C4` | 192 | 142 | `pod6_hi` (1883/1889) | 3, first and last coincide | **Culled every frame** (zero N); on the edge of strip poly 49 |
| `ZBD/C5` | 370 | 1 | `g14332` | 5, last two coincide | **Drawn**: the first three corners are distinct, so the fan `(0,1,2),(0,2,3),(0,3,4)` is submitted; the last triangle is zero-area and rasterises nothing. Visible result: the quad over its four distinct corners |
| `ZBD/C5` | 970 | 0 | `dzpath22` | 17 | **Never drawn**: node destroyed at load (R4) |
| `ZBD/C5` | 973 | 0 | `dzpath25` | 19 | **Never drawn** (R4) |
| `ZBD/C5` | 974 | 0 | `dzpath26` | 24 | **Never drawn** (R4) |
| `ZBD/C5` | 978 | 0 | `dzpath30` | 30 | **Never drawn** (R4) |
| `ZBD/C5` | 980 | 0 | `dzpath32` | 36 | **Never drawn** (R4) |
| `ZBD/C5` | 981 | 0 | `dzpath33` | 124 | **Never drawn** (R4) |
| `ZBD/C5` | 982 | 0 | `dzpath34` | 124 | **Never drawn** (R4) |

That is **3 faces culled, 1 drawn as a quad, 7 never present in the scene.**

## The nine meshes the old gate dropped — none is dropped by the original

Under the pre-F10-D.1 gate, one refused face refused its whole mesh: nine
rows failed and 89 stored faces never reached a renderer. Under the measured
behaviour:

* C4 mesh 191 draws its other 57 faces; only poly 20 and poly 55 are culled,
  per face, every frame;
* C4 mesh 192 draws everything except poly 142;
* C5 mesh 370 draws fully, poly 1 as the quad;
* the seven C5 `dzpath` meshes are absent because their nodes are destroyed
  at load (R4), not because of their faces — the faces themselves are
  ordinary stored outlines that decode cleanly.

The render gate now drops **zero** meshes on the retail corpus: every present
mesh builds a render mesh, and the three C4 faces are named per face on
`RenderMesh::culled_faces` / `MeshFaceCounts::culled` /
`FaceCensus::culled_faces` with reason `culled_zero_normal`.

## The fan-vs-ear-clip divergence — adopted

The corpus holds **83 067** n-gon outlines (≥4 corners, non-strip). Fanned
from corner 0 as the original does, **56** of them produce at least one fan
triangle that winds against the outline (a fold) — for those, ear-clipping
and the fan draw different regions, and the original draws the fan.

| archive | n-gons | folding when fanned |
| --- | ---: | ---: |
| `ZBD/C1` | 12 163 | 12 |
| `ZBD/C1B` | 5 820 | 2 |
| `ZBD/C1C` | 5 924 | 0 |
| `ZBD/C2` | 8 587 | 2 |
| `ZBD/C2B` | 5 353 | 0 |
| `ZBD/C3` | 9 336 | 0 |
| `ZBD/C4` | 11 008 | 10 |
| `ZBD/C5` | 15 908 | 13 |
| `ZBD/planes.zbd` | 8 968 | 17 |
| **total** | **83 067** | **56** |

Measured by running the production reader over the retail archives and
testing each outline's corner-0 fan for a negative signed-area triangle in
the outline's dominant-axis projection (exact `f64` arithmetic over the
stored `f32`s — the same convention `triangulate_polygon` states). The total
reproduces the owner's 56 exactly. The C5 row reads 13 here against the owner
note's 14: the difference is mesh 976 poly 0 (`dzpath29`), whose fold is real
in 3-D — the fan triangle's normal opposes the outline's Newell normal — but
projects to exactly zero area in the dominant plane, so this measurement
counts it as degenerate rather than folding. It is a `dzpath` route that is
never drawn either way (R4), so the definitional boundary changes nothing
visible.

**Decision (AC2): the fan is adopted, not just recorded.** `RawMesh` exposes
two views on purpose:

* `RawMesh::topology()` — the canonical, validated view. Ear-clips n-gon
  outlines and still rejects what it cannot triangulate; this is the view for
  consumers that need a simple triangulation of a simple polygon (collision,
  analysis).
* `RawMesh::draw_topology()` — the measured original view. Every outline fans
  from corner 0 exactly as the `D3DPT_TRIANGLEFAN` submission does; a
  zero-first-three-corner-normal outline is `FaceStatus::Culled` per face;
  faces are processed independently so nothing refuses its mesh. The render
  gate (`RenderMesh::build`, `MeshCatalog`) and the face census
  (`GameZMeshes::face_census`) walk this view.

The 56 folding outlines therefore draw exactly what the original submits —
the fan, folds and all — rather than an ear-clipped approximation of it.

## The `dzpath` routes — removed at load, never drawn, still read

`GameZNodes::removed_at_load()` implements the measured removal: the plain
`dzpath` name prefix (the `dzpaths` grouping node itself matches) plus each
matched node's whole subtree. `NodeMeshBindings` reports the bound removed
nodes (`removed`) and the mesh slots no *surviving* node binds
(`never_drawn`). Pinned on the retail corpus by
`accept_f10_d_retail_dzpath_route_nodes_are_removed_at_load_and_never_drawn`:

| archive | nodes removed at load | of them bound | never-drawn mesh slots |
| --- | ---: | ---: | --- |
| `ZBD/C1` | 7 | 6 | 998–1003 |
| `ZBD/C1B` | 8 | 7 | 432–438 |
| `ZBD/C1C` | 0 | 0 | – |
| `ZBD/C2` | 14 | 13 | 593–605 |
| `ZBD/C2B` | 0 | 0 | – |
| `ZBD/C3` | 6 | 5 | 802–806 |
| `ZBD/C4` | 16 | 15 | 863–877 |
| `ZBD/C5` | 35 | 34 | 949–982 |
| `ZBD/planes.zbd` | 0 | 0 | – |

Every container's `dzpaths` group is a direct child of the world node and
binds no mesh itself; the numbered zones sit under it, outside the world
node's ownership union, so the world importer never instantiated them as
objects even before this task — the removal was already the structural
outcome, and it is now a measured and reported one. The zones' route data is
still read by the detection-zone consumer
(`cs_content::world::is_detection_zone_name`, the `cs_app::world::triggers`
survey): R4 removes them from the *scene*, not from the file, and the AI
owner's consumption of `dzpath<N>` is exactly the consumer the task asked to
have them filed with.

## What the reconciliation changed (AC2)

* `crates/cs_formats/src/gamez/polygon.rs` — `outline_draw` /
  `OutlineDraw`: the measured rule (R1 + R2) as data — `Culled` for a zero
  first-three-corner normal, else the corner-0 fan's corner-index triples.
* `crates/cs_formats/src/gamez/mesh.rs` — `FaceStatus::Culled`,
  `RawMesh::draw_topology`, `MeshTopology::culled_faces`.
* `crates/cs_formats/src/gamez/census.rs` — the census walks
  `draw_topology`; `MissingFaceReason::Culled` (`culled_zero_normal`) and the
  `culled_faces` count; `rejected_meshes` now means only invalid/unsupported
  faces, which is what can actually refuse a mesh.
* `crates/cs_formats/src/gamez/nodes.rs` — `LOAD_REMOVED_NODE_PREFIX`,
  `GameZNodes::removed_at_load` (R4, prefix + subtree).
* `crates/cs_formats/src/gamez/bindings.rs` — `RemovedBinding`,
  `NodeMeshBindings::removed` / `never_drawn`.
* `crates/cs_content/src/mesh.rs` — the gate builds from `draw_topology`;
  `RenderMesh::culled_faces`, `MeshFaceCounts::culled`; a rejected face still
  refuses its mesh, a culled one never does.

Retail-corpus result: missing faces are now **3, all `culled_zero_normal`,
all in C4** (down from 11); rejected meshes are **0** (down from 9); every
present mesh builds a render mesh.

## Tests

* `accept_f10_d_a_culled_face_is_named_and_never_refuses_its_mesh`
  (`cs_content::mesh`, synthetic): a coincident-corner outline is `Culled`
  under the draw view, `Rejected` under the canonical view, and its mesh
  still builds with the polygon named on `culled_faces`.
* `accept_f10_d_a_dzpath_subtree_is_removed_at_load_and_its_mesh_is_never_drawn`
  (`cs_formats`, synthetic): prefix match, subtree propagation and the
  live-binding exception of `removed_at_load` / `never_drawn`.
* `accept_f10_d_retail_dzpath_route_nodes_are_removed_at_load_and_never_drawn`
  (`cs_formats`, `#[ignore = "requires CS_GAME_DIR"]`): the pinned
  per-archive removal and never-drawn-slot tables above.
* `accept_f10_d_retail_every_world_and_airframe_reports_exact_missing_and_invalid_face_counts`
  (retail): re-pinned to the reconciled counts — C4 missing 3
  (`culled_zero_normal`), C5 missing 0.
* `accept_f10_d_retail_render_gate_drops_exactly_the_refused_faces_of_every_archive`
  (retail, `cs_content`): the gate now fails zero meshes; the culled faces
  are reported per row, and a mesh that built lost nothing.

## Honest limits

* `OWNER-STATIC-2026-10-05` is static code evidence. The per-frame `d` runs
  in camera space, so for a *nonzero* stored `N` whether a given frame draws
  the fan is a runtime question this task does not claim to answer — the code
  keeps the culled set to exactly the always-culled (zero `N`) case.
* The software renderer's projected-area drop (`|shoelace| < 4.0` px²,
  `0x9fddfc`/`0x63b214`) applies only on the software path and is not
  modelled; the engine's own path is D3D.
* Front-face winding/handedness stays deferred to F10-D.2 (#396) — the fan
  order is stored order, and which side a fan shows is that task's question.
